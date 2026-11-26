//! The machined parts as a SolidCraft design, built the way a person models them there: a
//! `thickness` parameter, then for each part a component holding a sketch of its outline (lines,
//! arcs and circles, its through holes in the same sketch) extruded to the thickness, and each
//! pocket, drilling or groove sketched and cut down from the top to its own depth parameter.
//! Change a parameter in SolidCraft and the parts rebuild. Written as a script of SolidCraft's
//! own commands, which its command line replays into a design (`.sc`). Burin's own.

use crate::geom::P;
use crate::solid::{loops_of, overlay, ring_arcs, union_all, Removal, Shapes};
use i_overlay::core::overlay_rule::OverlayRule;
use serde_json::{json, Value};
use std::path::PathBuf;

/// How far a point may stray from the circle it is taken to lie on, in the drawing's units.
const ARC_TOLERANCE: f64 = 0.01;

/// One part: the name it gets in SolidCraft, its outline and what is cut into it.
pub struct Part {
    pub name: String,
    pub outline: Shapes,
    pub removals: Vec<Removal>,
}

/// The script building every part, lengths times `scale` (to millimetres).
pub fn script(parts: &[Part], thickness: f64, scale: f64) -> Value {
    let mm = |v: f64| v * scale;
    let through = |d: f64| d >= thickness - 1e-6;
    // Each depth something is cut to, shallowest first: one parameter each.
    let mut depths: Vec<f64> = parts.iter().flat_map(|p| p.removals.iter().map(|r| r.depth)).filter(|d| *d > 1e-6 && !through(*d)).collect();
    depths.sort_by(f64::total_cmp);
    depths.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut commands = vec![json!({"command": "parameters.add", "params": {"name": "thickness", "expression": format!("{} mm", number(mm(thickness)))}})];
    for (k, d) in depths.iter().enumerate() {
        commands.push(json!({"command": "parameters.add", "params": {"name": format!("depth_{}", k + 1), "expression": format!("{} mm", number(mm(*d)))}}));
    }
    let point = |p: P| json!([number(mm(p[0])), number(mm(p[1]))]);
    for part in parts {
        let outline = loops_of(&part.outline);
        let holes: Vec<Vec<P>> = union_all(part.removals.iter().filter(|r| through(r.depth)).map(|r| &r.region));
        let base = clean(if holes.is_empty() { overlay(&outline, &[], OverlayRule::Subject) } else { overlay(&outline, &holes, OverlayRule::Difference) });
        if base.is_empty() {
            continue;
        }
        let base_loops = loops_of(&base);
        // The floors: each depth's face is what is cut to that depth and no deeper.
        let covered = |d: f64| -> Vec<Vec<P>> {
            let at: Vec<Vec<P>> = union_all(part.removals.iter().filter(|r| r.depth > d - 1e-6 && !through(r.depth)).map(|r| &r.region));
            if at.is_empty() { at } else { loops_of(&overlay(&at, &base_loops, OverlayRule::Intersect)) }
        };
        let floors: Vec<(usize, Shapes)> = depths
            .iter()
            .enumerate()
            .filter_map(|(k, d)| {
                let here = covered(*d);
                if here.is_empty() {
                    return None;
                }
                let deeper = depths.get(k + 1).map(|n| covered(*n)).unwrap_or_default();
                let face = clean(if deeper.is_empty() { overlay(&here, &[], OverlayRule::Subject) } else { overlay(&here, &deeper, OverlayRule::Difference) });
                (!face.is_empty()).then_some((k, face))
            })
            .collect();
        // Every edge once, in pieces between the points where outlines meet, each piece drawn
        // with the same curves in every sketch it bounds: a pocket's wall along the part's edge
        // is then the very same surface as the edge, as when a person reuses one sketch.
        let mut faces: Vec<&Shapes> = vec![&base];
        faces.extend(floors.iter().map(|(_, f)| f));
        let pieces = Pieces::of(&faces);
        commands.push(json!({"command": "component.create", "params": {"name": part.name}}));
        pieces.sketch(&mut commands, 0, scale);
        commands.push(json!({"command": "solid.extrude", "params": {
            "distance": "thickness",
            "profiles": base.iter().map(|shape| json!({"point": point(inner_point(shape))})).collect::<Vec<_>>(),
            "operation": "new",
            "name": "Outline",
            "body_name": part.name,
        }}));
        for (f, (k, face)) in floors.iter().enumerate() {
            pieces.sketch(&mut commands, f + 1, scale);
            commands.push(json!({"command": "solid.extrude", "params": {
                "distance": format!("depth_{}", k + 1),
                "profiles": face.iter().map(|shape| json!({"point": point(inner_point(shape))})).collect::<Vec<_>>(),
                "operation": "cut",
                "start_offset": "thickness",
                "direction": "negative",
                "name": format!("Cut {}", k + 1),
                "targets": [part.name],
            }}));
        }
    }
    Value::Array(commands)
}

/// The narrowest loop kept, on average (twice its area over its perimeter), in the drawing's
/// units: narrower, it is a sliver left where two tool bands meet, which no one would draw and
/// which leaves a CAD program unsure what region a point is in.
const NARROWEST: f64 = 0.01;

/// `shapes` without their slivers and with points closer than a thousandth taken as one.
fn clean(shapes: Shapes) -> Shapes {
    let wide = |l: &Vec<P>| {
        let perimeter: f64 = (0..l.len()).map(|k| (l[(k + 1) % l.len()][0] - l[k][0]).hypot(l[(k + 1) % l.len()][1] - l[k][1])).sum();
        l.len() >= 3 && 2.0 * crate::geom::signed_area(l).abs() / perimeter.max(1e-12) > NARROWEST
    };
    let loops: Vec<Vec<P>> = loops_of(&shapes)
        .into_iter()
        .map(|l| {
            let mut out: Vec<P> = Vec::with_capacity(l.len());
            for p in l {
                if out.last().is_none_or(|q: &P| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-3) {
                    out.push(p);
                }
            }
            while out.len() > 1 && (out[0][0] - out[out.len() - 1][0]).hypot(out[0][1] - out[out.len() - 1][1]) <= 1e-3 {
                out.pop();
            }
            out
        })
        .filter(wide)
        .collect();
    overlay(&loops, &[], OverlayRule::Subject).into_iter().map(|shape| shape.into_iter().filter(wide).collect::<Vec<_>>()).filter(|s| !s.is_empty() && wide(&s[0])).collect()
}

/// A number as SolidCraft reads it, without the noise of the last digits.
fn number(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

/// The edges of several faces, welded where they meet and cut into pieces at every point where
/// more than two edges meet or the faces an edge bounds change, each piece fitted with its curves.
struct Pieces {
    /// Each piece's curves, and the faces it bounds (bit k for face k).
    pieces: Vec<(Vec<Curve>, u64)>,
}

impl Pieces {
    fn of(faces: &[&Shapes]) -> Pieces {
        // Points closer than a thousandth are one: the faces come from separate boolean
        // operations, which may round a shared point differently.
        let mut points: Vec<P> = Vec::new();
        let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
        let mut weld = |p: P| -> usize {
            let cell = ((p[0] / 1e-3).floor() as i64, (p[1] / 1e-3).floor() as i64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &i in grid.get(&(cell.0 + dx, cell.1 + dy)).into_iter().flatten() {
                        if (points[i][0] - p[0]).hypot(points[i][1] - p[1]) <= 1e-3 {
                            return i;
                        }
                    }
                }
            }
            points.push(p);
            grid.entry(cell).or_default().push(points.len() - 1);
            points.len() - 1
        };
        let welded: Vec<(usize, Vec<usize>)> = faces.iter().enumerate().take(64).flat_map(|(f, face)| face.iter().flatten().map(move |l| (f, l))).map(|(f, l)| (f, l.iter().map(|p| weld(*p)).collect())).collect();
        // A point of one outline lying on another's edge splits that edge there: a pocket
        // running along the part's edge meets it between the edge's own points, and both must
        // draw that stretch as the same piece.
        let mut coarse: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
        for (i, p) in points.iter().enumerate() {
            coarse.entry(((p[0] / 0.5).floor() as i64, (p[1] / 0.5).floor() as i64)).or_default().push(i);
        }
        let split = |ids: &[usize]| -> Vec<usize> {
            let mut out = Vec::with_capacity(ids.len());
            for k in 0..ids.len() {
                let (a, b) = (ids[k], ids[(k + 1) % ids.len()]);
                out.push(a);
                let (pa, pb) = (points[a], points[b]);
                let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
                let len2 = dx * dx + dy * dy;
                if len2 < 1e-12 {
                    continue;
                }
                let steps = (len2.sqrt() / 0.5).ceil() as usize;
                let mut cells: Vec<(i64, i64)> = Vec::new();
                for t in 0..=steps {
                    let q = [pa[0] + dx * t as f64 / steps.max(1) as f64, pa[1] + dy * t as f64 / steps.max(1) as f64];
                    let c = ((q[0] / 0.5).floor() as i64, (q[1] / 0.5).floor() as i64);
                    for ox in -1..=1 {
                        for oy in -1..=1 {
                            cells.push((c.0 + ox, c.1 + oy));
                        }
                    }
                }
                cells.sort_unstable();
                cells.dedup();
                let mut on: Vec<(f64, usize)> = cells
                    .iter()
                    .filter_map(|c| coarse.get(c))
                    .flatten()
                    .copied()
                    .filter(|&v| v != a && v != b)
                    .filter_map(|v| {
                        let q = points[v];
                        let t = ((q[0] - pa[0]) * dx + (q[1] - pa[1]) * dy) / len2;
                        (t > 0.0 && t < 1.0 && crate::geom::dist2_seg(q, pa, pb) <= 1e-6).then_some((t, v))
                    })
                    .collect();
                on.sort_by(|x, y| x.0.total_cmp(&y.0));
                on.dedup_by_key(|x| x.1);
                out.extend(on.into_iter().map(|(_, v)| v));
            }
            // No spikes: an edge run out and straight back bounds nothing.
            loop {
                let n = out.len();
                let Some(k) = (0..n).find(|&k| n >= 3 && (out[(k + n - 1) % n] == out[(k + 1) % n] || out[k] == out[(k + 1) % n])) else { break };
                if out[k] == out[(k + 1) % n] {
                    out.remove(k);
                } else {
                    out.remove((k + 1) % n);
                    let k = if (k + 1) % n < k { k - 1 } else { k };
                    out.remove(k);
                }
            }
            out
        };
        // Each edge with the faces it bounds.
        let mut edges: std::collections::BTreeMap<(usize, usize), u64> = std::collections::BTreeMap::new();
        for (f, ids) in &welded {
            let f = *f;
            {
                let ids = split(ids);
                if ids.len() < 3 {
                    continue;
                }
                for k in 0..ids.len() {
                    let (a, b) = (ids[k], ids[(k + 1) % ids.len()]);
                    if a != b {
                        *edges.entry((a.min(b), a.max(b))).or_default() |= 1 << f;
                    }
                }
            }
        }
        let mut around: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
        for &(a, b) in edges.keys() {
            around.entry(a).or_default().push(b);
            around.entry(b).or_default().push(a);
        }
        let key = |a: usize, b: usize| (a.min(b), a.max(b));
        // A point a piece passes through: two edges, bounding the same faces.
        let inner = |v: usize| {
            let n = &around[&v];
            n.len() == 2 && edges[&key(v, n[0])] == edges[&key(v, n[1])]
        };
        let mut used: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
        let mut pieces = Vec::new();
        let walk = |from: usize, next: usize, used: &mut std::collections::HashSet<(usize, usize)>| -> Vec<usize> {
            let mut chain = vec![from, next];
            used.insert(key(from, next));
            let (mut prev, mut at) = (from, next);
            while at != from && inner(at) {
                let Some(&n) = around[&at].iter().find(|n| **n != prev && !used.contains(&key(at, **n))) else { break };
                used.insert(key(at, n));
                chain.push(n);
                prev = at;
                at = n;
            }
            chain
        };
        let mut ends: Vec<usize> = around.keys().copied().filter(|v| !inner(*v)).collect();
        ends.sort_unstable();
        for v in ends {
            for n in around[&v].clone() {
                if !used.contains(&key(v, n)) {
                    let chain = walk(v, n, &mut used);
                    let faces = edges[&key(chain[0], chain[1])];
                    let pts: Vec<P> = chain.iter().map(|i| points[*i]).collect();
                    pieces.push((open_curves(&pts), faces));
                }
            }
        }
        // What is left are loops nothing meets: each drawn whole.
        let rest: Vec<(usize, usize)> = edges.keys().copied().filter(|e| !used.contains(e)).collect();
        for (a, b) in rest {
            if used.contains(&key(a, b)) {
                continue;
            }
            let mut chain = walk(a, b, &mut used);
            if chain.last() == chain.first() {
                chain.pop();
            }
            let faces = edges[&key(chain[0], chain[1])];
            let pts: Vec<P> = chain.iter().map(|i| points[*i]).collect();
            pieces.push((curves(&pts), faces));
        }
        Pieces { pieces }
    }

    /// A sketch on the XY plane of face `f`'s edges: whole circles as circles, runs of points on
    /// one circle as arcs, the rest as lines.
    fn sketch(&self, commands: &mut Vec<Value>, f: usize, scale: f64) {
        let point = |p: P| json!([number(p[0] * scale), number(p[1] * scale)]);
        commands.push(json!({"command": "sketch.create", "params": {"plane": "XY"}}));
        for (curves, faces) in &self.pieces {
            if faces & (1 << f) == 0 {
                continue;
            }
            for curve in curves {
                commands.push(match curve {
                    Curve::Circle(c) => json!({"command": "sketch.circle.center", "params": {"center": point(c.c), "diameter": number(scale * 2.0 * c.r)}}),
                    Curve::Arc(a, through, b) => json!({"command": "sketch.arc.three_point", "params": {"start": point(*a), "end": point(*b), "through": point(*through)}}),
                    Curve::Lines(pts) => json!({"command": "sketch.line", "params": {"points": pts.iter().map(|p| point(*p)).collect::<Vec<_>>()}}),
                });
            }
        }
        commands.push(json!({"command": "sketch.finish", "params": {}}));
    }
}

/// A run of points on circle `c` as one arc from its first point to its last, through the point
/// halfway round: only when the points go round one way, less than a whole turn. A run that
/// turns back on itself would give three points of a circle going the other way round, nearly
/// whole, so it stays lines.
fn arc_or_lines(run: &[P], c: crate::solid::Circle) -> Curve {
    let angle = |p: P| (p[1] - c.c[1]).atan2(p[0] - c.c[0]);
    let steps: Vec<f64> = run
        .windows(2)
        .map(|w| {
            let mut d = angle(w[1]) - angle(w[0]);
            if d > std::f64::consts::PI {
                d -= std::f64::consts::TAU;
            } else if d < -std::f64::consts::PI {
                d += std::f64::consts::TAU;
            }
            d
        })
        .collect();
    let sweep: f64 = steps.iter().sum();
    let one_way = steps.iter().all(|d| d.signum() == sweep.signum() && d.abs() > 1e-12);
    if run.len() < 3 || !one_way || sweep.abs() >= std::f64::consts::TAU - 0.05 {
        return Curve::Lines(straightened(run.to_vec()));
    }
    let half = angle(run[0]) + sweep / 2.0;
    Curve::Arc(run[0], [c.c[0] + c.r * half.cos(), c.c[1] + c.r * half.sin()], run[run.len() - 1])
}

/// An open run of points as curves, end to end: runs of at least four short edges turning one way
/// with every point on one circle as arcs, the rest as lines.
fn open_curves(l: &[P]) -> Vec<Curve> {
    let n = l.len();
    let on = |c: &crate::solid::Circle, p: P| ((p[0] - c.c[0]).hypot(p[1] - c.c[1]) - c.r).abs() <= ARC_TOLERANCE;
    let turn = |i: usize| {
        let (a, b, c) = (l[i], l[i + 1], l[i + 2]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    };
    // Points i to j on one circle, curving more than the tolerance.
    let fits = |i: usize, j: usize| -> Option<crate::solid::Circle> {
        let c = crate::solid::circle_through(l[i], l[(i + j) / 2], l[j])?;
        let chord = (l[j][0] - l[i][0]).hypot(l[j][1] - l[i][1]).min(2.0 * c.r);
        let sagitta = c.r - (c.r * c.r - chord * chord / 4.0).max(0.0).sqrt();
        let sign = turn(i).signum();
        let ok = c.r > ARC_TOLERANCE * 10.0
            && c.r < 10_000.0
            && sagitta > ARC_TOLERANCE * 2.0
            && (i..=j).all(|k| on(&c, l[k]))
            && (i..j).all(|k| (l[k + 1][0] - l[k][0]).hypot(l[k + 1][1] - l[k][1]) <= c.r * 0.35)
            && (i..j - 1).all(|k| turn(k).signum() == sign && turn(k) != 0.0)
            // Less than a whole turn, for three points to say which arc.
            && chord > 1e-6;
        ok.then_some(c)
    };
    let mut out = Vec::new();
    let mut lines: Vec<P> = vec![l[0]];
    let mut i = 0;
    while i + 1 < n {
        let mut j = i + 4;
        if j < n && fits(i, j).is_some() {
            while j + 1 < n && fits(i, j + 1).is_some() {
                j += 1;
            }
            let c = fits(i, j).unwrap_or(crate::solid::Circle { c: [0.0, 0.0], r: 0.0 });
            if lines.len() >= 2 {
                out.push(Curve::Lines(straightened(std::mem::take(&mut lines))));
            }
            out.push(arc_or_lines(&l[i..=j], c));
            lines = vec![l[j]];
            i = j;
        } else {
            lines.push(l[i + 1]);
            i += 1;
        }
    }
    if lines.len() >= 2 {
        out.push(Curve::Lines(straightened(lines)));
    }
    out
}

#[derive(Debug)]
enum Curve {
    Circle(crate::solid::Circle),
    /// From, through, to.
    Arc(P, P, P),
    Lines(Vec<P>),
}

impl PartialEq for crate::solid::Circle {
    fn eq(&self, o: &Self) -> bool {
        self.c == o.c && self.r == o.r
    }
}

/// A closed loop as the curves it is drawn with, end to end.
fn curves(l: &[P]) -> Vec<Curve> {
    let n = l.len();
    if n < 3 {
        return Vec::new();
    }
    let arcs = ring_arcs(l, ARC_TOLERANCE);
    if let Some(c) = arcs[0] {
        if arcs.iter().all(|a| *a == Some(c)) {
            return vec![Curve::Circle(c)];
        }
    }
    // Start where one curve ends and another begins.
    let start = (0..n).find(|k| arcs[*k] != arcs[(k + n - 1) % n] || arcs[*k].is_none()).unwrap_or(0);
    let mut out = Vec::new();
    let mut k = 0;
    while k < n {
        let edge = (start + k) % n;
        let mut j = k + 1;
        match arcs[edge] {
            Some(c) => {
                while j < n && arcs[(start + j) % n] == Some(c) {
                    j += 1;
                }
                let run: Vec<P> = (k..=j).map(|i| l[(start + i) % n]).collect();
                out.push(arc_or_lines(&run, c));
            }
            None => {
                while j < n && arcs[(start + j) % n].is_none() {
                    j += 1;
                }
                let pts: Vec<P> = (k..=j).map(|i| l[(start + i) % n]).collect();
                out.push(Curve::Lines(straightened(pts)));
            }
        }
        k = j;
    }
    out
}

/// A run of lines without the points that lie on the line through their neighbours.
fn straightened(pts: Vec<P>) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let (dx, dy) = (p[0] - a[0], p[1] - a[1]);
            let len = dx.hypot(dy);
            let off = ((b[0] - a[0]) * dy - (b[1] - a[1]) * dx).abs() / len.max(1e-12);
            let between = (b[0] - a[0]) * dx + (b[1] - a[1]) * dy >= 0.0 && (p[0] - b[0]) * dx + (p[1] - b[1]) * dy >= 0.0;
            if off < 1e-6 && between {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// A point well inside one shape (its outline first, then its holes), to pick its region of a
/// sketch by: beside the middle of an edge of the outline, as far in as the shape allows.
pub fn inner_point(shape: &[Vec<P>]) -> P {
    let edges: Vec<(P, P)> = shape.iter().flat_map(|l| (0..l.len()).map(move |k| (l[k], l[(k + 1) % l.len()]))).collect();
    let clearance = |q: P| edges.iter().map(|(a, b)| crate::geom::dist2_seg(q, *a, *b).sqrt()).fold(f64::INFINITY, f64::min);
    let paths: Vec<crate::geom::Path> = shape.iter().map(|l| crate::geom::Path::closed(l.clone())).collect();
    let outline = &shape[0];
    let mut order: Vec<usize> = (0..outline.len()).collect();
    let length = |k: usize| {
        let (a, b) = (outline[k], outline[(k + 1) % outline.len()]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    order.sort_by(|a, b| length(*b).total_cmp(&length(*a)));
    for off in [2.0, 0.5, 0.1, 0.02] {
        for &k in &order {
            let (a, b) = (outline[k], outline[(k + 1) % outline.len()]);
            let len = length(k);
            if len < off {
                continue;
            }
            // The outline runs anticlockwise: inside is on its left.
            let q = [(a[0] + b[0]) / 2.0 - (b[1] - a[1]) / len * off, (a[1] + b[1]) / 2.0 + (b[0] - a[0]) / len * off];
            if crate::geom::inside_even_odd(paths.iter(), q) && clearance(q) >= off * 0.9 {
                return q;
            }
        }
    }
    let (a, b) = (outline[0], outline[1]);
    let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
    [(a[0] + b[0]) / 2.0 - (b[1] - a[1]) / len * 1e-3, (a[1] + b[1]) / 2.0 + (b[0] - a[0]) / len * 1e-3]
}

/// SolidCraft's command line, when it is on this computer: the AppImage the desktop starts (kept
/// up to date by SolidCraft itself), else a `solidcraft-cli` that still runs (an AppImage's
/// launcher can outlive the AppImage it was written for once SolidCraft is updated), else the
/// newest AppImage where people keep them.
#[cfg(not(target_arch = "wasm32"))]
pub fn command_line() -> Option<std::process::Command> {
    let from_image = |image: PathBuf| {
        let mut command = std::process::Command::new(image);
        command.env("SOLIDCRAFT_APPIMAGE_CLI", "1");
        command
    };
    if let Some(image) = desktop_image() {
        return Some(from_image(image));
    }
    if let Some(found) = on_path(if cfg!(windows) { "solidcraft-cli.exe" } else { "solidcraft-cli" }).filter(|f| launcher_works(f)) {
        return Some(std::process::Command::new(found));
    }
    app_image().map(from_image)
}

/// SolidCraft itself, to open a design in.
#[cfg(not(target_arch = "wasm32"))]
pub fn program() -> Option<PathBuf> {
    desktop_image().or_else(|| on_path(if cfg!(windows) { "solidcraft.exe" } else { "solidcraft" }).filter(|f| launcher_works(f))).or_else(app_image)
}

/// Whether a program found on the path can start: a small launcher script naming a program that
/// is no longer there can't.
#[cfg(not(target_arch = "wasm32"))]
fn launcher_works(file: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read(file) else { return false };
    if text.len() > 64 * 1024 || !text.starts_with(b"#!") {
        return true;
    }
    let text = String::from_utf8_lossy(&text);
    text.split(['"', '\'', ' ', '\n']).filter(|w| w.starts_with('/') && w.contains("AppImage")).all(|w| std::path::Path::new(w).is_file())
}

/// The AppImage SolidCraft's desktop entry starts, when it is there.
#[cfg(not(target_arch = "wasm32"))]
fn desktop_image() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let folder = home.join(".local/share/applications");
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().contains("solidcraft") && n.ends_with(".desktop")))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|text| {
            let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
            let program = exec.trim().strip_prefix('"').and_then(|e| e.split('"').next()).unwrap_or_else(|| exec.split_whitespace().next().unwrap_or(""));
            let program = PathBuf::from(program);
            (program.is_file() && program.to_string_lossy().ends_with(".AppImage")).then_some(program)
        })
        .next()
}

#[cfg(not(target_arch = "wasm32"))]
fn on_path(name: &str) -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        places.push(PathBuf::from(home).join(".local/bin"));
    }
    places.into_iter().map(|d| d.join(name)).find(|f| f.is_file())
}

/// The newest SolidCraft AppImage where people keep them.
#[cfg(not(target_arch = "wasm32"))]
fn app_image() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let mut found: Vec<PathBuf> = ["Applications", "Downloads", "Téléchargements", "bin", ".local/bin"]
        .iter()
        .filter_map(|d| std::fs::read_dir(home.join(d)).ok())
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().starts_with("solidcraft") && n.ends_with(".AppImage")))
        .collect();
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    found.pop()
}

/// Replays `script` into a SolidCraft design at `target`, with SolidCraft's command line.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_design(script: &Value, target: &std::path::Path) -> Result<(), String> {
    let mut command = command_line().ok_or_else(|| crate::i18n::tr("SolidCraft isn't on this computer.").to_string())?;
    // A folder of its own, even with two built at once.
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!("burin-solidcraft-{}-{}", std::process::id(), COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join("parts.json");
    std::fs::write(&file, serde_json::to_vec_pretty(script).unwrap_or_default()).map_err(|e| e.to_string())?;
    let out = command.arg("run").arg(&file).arg("--save").arg(target).arg("--quiet").stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&folder);
    if out.status.success() && target.is_file() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        // What SolidCraft says went wrong, from its error line on.
        let lines: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty() && !l.contains("installed in")).collect();
        let from = lines.iter().position(|l| l.trim_start().starts_with("error")).unwrap_or(0);
        Err(lines[from..].iter().take(4).copied().collect::<Vec<_>>().join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<P> {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn a_round_hole_is_a_circle_and_a_rounded_corner_an_arc() {
        let hole = crate::geom::circle([30.0, 30.0], 5.0);
        assert!(matches!(curves(&hole)[..], [Curve::Circle(c)] if (c.r - 5.0).abs() < 0.01 && (c.c[0] - 30.0).abs() < 0.01));
        // A square with one corner rounded: lines, then one arc, then lines.
        let mut l = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]];
        for k in 0..=12 {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 12.0;
            l.push([90.0 + 10.0 * a.cos(), 50.0 + 10.0 * a.sin()]);
        }
        l.push([0.0, 60.0]);
        let found = curves(&l);
        let arcs: Vec<&Curve> = found.iter().filter(|c| matches!(c, Curve::Arc(..))).collect();
        assert_eq!(arcs.len(), 1, "{found:?}");
        let Curve::Arc(a, _, b) = arcs[0] else { unreachable!() };
        assert!((a[0] - 100.0).abs() < 1e-9 && (a[1] - 50.0).abs() < 1e-9 && (b[0] - 90.0).abs() < 1e-9 && (b[1] - 60.0).abs() < 1e-9);
        // The straight sides are one line each, not many little ones.
        let points: usize = found.iter().map(|c| if let Curve::Lines(p) = c { p.len() } else { 0 }).sum();
        assert!(points <= 6, "{found:?}");
    }

    #[test]
    fn a_part_is_its_outline_extruded_less_its_pockets_cut_to_their_depth() {
        let part = Part {
            name: "Plate".into(),
            outline: vec![vec![square(0.0, 0.0, 100.0)]],
            removals: vec![Removal { region: vec![crate::geom::circle([20.0, 20.0], 4.0)], depth: 18.0 }, Removal { region: vec![square(50.0, 50.0, 30.0)], depth: 6.0 }],
        };
        let s = script(&[part], 18.0, 1.0);
        let commands: Vec<&str> = s.as_array().unwrap().iter().map(|c| c["command"].as_str().unwrap()).collect();
        assert_eq!(commands.iter().filter(|c| **c == "solid.extrude").count(), 2);
        assert_eq!(commands.iter().filter(|c| **c == "sketch.circle.center").count(), 1);
        let params: Vec<&Value> = s.as_array().unwrap().iter().filter(|c| c["command"] == "parameters.add").map(|c| &c["params"]).collect();
        assert_eq!(params[0]["expression"], "18 mm");
        assert_eq!(params[1]["name"], "depth_1");
        assert_eq!(params[1]["expression"], "6 mm");
        let cut = s.as_array().unwrap().iter().rfind(|c| c["command"] == "solid.extrude").unwrap();
        assert_eq!(cut["params"]["operation"], "cut");
        assert_eq!(cut["params"]["distance"], "depth_1");
        // The outline's profile is picked by a point in the material, not in the hole.
        let first = s.as_array().unwrap().iter().find(|c| c["command"] == "solid.extrude").unwrap();
        let q = &first["params"]["profiles"][0]["point"];
        let q = [q[0].as_f64().unwrap(), q[1].as_f64().unwrap()];
        assert!(q[0] > 0.0 && q[0] < 100.0 && q[1] > 0.0 && q[1] < 100.0 && (q[0] - 20.0).hypot(q[1] - 20.0) > 4.0);
    }

    /// A pocket running into the edge of a round part, and a deeper one inside it: SolidCraft
    /// builds it (when it is on this computer), one closed body of the right volume. A pocket's
    /// wall along the edge drawn apart from the edge was refused by its kernel.
    #[test]
    fn a_pocket_reaching_the_edge_of_a_round_part_is_built() {
        let disc = crate::geom::circle([0.0, 0.0], 50.0);
        let notch = vec![[20.0, -15.0], [70.0, -15.0], [70.0, 15.0], [20.0, 15.0]];
        let deep = vec![[30.0, -5.0], [40.0, -5.0], [40.0, 5.0], [30.0, 5.0]];
        let part = Part {
            name: "Disc".into(),
            outline: vec![vec![disc.clone()]],
            removals: vec![Removal { region: vec![notch.clone()], depth: 4.0 }, Removal { region: vec![deep.clone()], depth: 8.0 }],
        };
        let script = script(&[part], 12.0, 1.0);
        let Some(mut cli) = command_line() else { return };
        let folder = std::env::temp_dir().join(format!("burin-sc-edge-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let design = folder.join("disc.sc");
        write_design(&script, &design).unwrap();
        let out = cli.arg("eval").arg("--in").arg(&design).output().unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        let text = String::from_utf8_lossy(&out.stdout);
        let measured: Value = serde_json::from_str(&text[text.find('{').unwrap()..]).unwrap();
        let body = &measured["bodies"][0];
        assert_eq!(body["closed"], true);
        let area = |shapes: &Shapes| shapes.iter().map(|s| s.iter().map(|l| crate::geom::signed_area(l)).sum::<f64>()).sum::<f64>();
        let notched = area(&overlay(&[notch], &[disc.clone()], OverlayRule::Intersect));
        let expected = area(&overlay(&[disc], &[], OverlayRule::Subject)) * 12.0 - notched * 4.0 - 100.0 * 4.0;
        let volume = body["volume_mm3"].as_f64().unwrap();
        assert!((volume - expected).abs() < 0.002 * expected, "volume {volume:.1} against {expected:.1}");
    }

    #[test]
    fn a_point_inside_a_ring_is_in_its_material() {
        let shape = vec![square(0.0, 0.0, 10.0), square(1.0, 1.0, 8.0).into_iter().rev().collect()];
        let q = inner_point(&shape);
        assert!(q[0] > 0.0 && q[0] < 10.0 && q[1] > 0.0 && q[1] < 10.0);
        assert!(!(q[0] > 1.0 && q[0] < 9.0 && q[1] > 1.0 && q[1] < 9.0), "{q:?}");
    }
}
