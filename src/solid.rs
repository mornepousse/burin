//! Machined parts as clean solids for CAD programs, built as a CAD program models them: each
//! part's outline from the drawing extruded to the board's thickness, less its holes, less each
//! pocket, drilling and groove cut flat to its depth; written as a STEP solid or an STL. Burin's
//! own: the original exports only its 3D model.
//!
//! A part is a stack of flat-topped slabs, one per depth, each inside the one below; faces and
//! walls meet point for point, so the surface is closed.

use crate::geom::{dist2_seg, signed_area, P};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use std::collections::HashMap;


/// A flat face: its height, its loops (outline then holes, seen from above) and which way it faces.
pub struct Flat {
    pub z: f64,
    pub loops: Vec<Vec<P>>,
    pub up: bool,
}

/// A wall all round one slab: its outline, turning with the part on its left, from `low` up to `top`.
#[derive(Clone)]
pub struct Ring {
    pub pts: Vec<P>,
    pub low: f64,
    pub top: f64,
}

/// A carved piece made of flat faces and straight walls, in millimetres.
pub struct Solid {
    pub flats: Vec<Flat>,
    /// The walls by outline, for the STEP file's arcs and cylinders.
    pub rings: Vec<Ring>,
    /// Each wall a flat polygon, its corners anticlockwise seen from outside.
    pub walls: Vec<Vec<[f64; 3]>>,
}

/// Shapes: each an outline turning anticlockwise, then its holes turning clockwise.
pub type Shapes = Vec<Vec<Vec<P>>>;

/// Coordinates are kept on a grid this fine (in the job's unit), so that the same edge met by two
/// boolean operations comes out at the very same points.
const SNAP: f64 = 1e-4;

fn snap_loops(loops: Vec<Vec<P>>) -> Vec<Vec<P>> {
    loops
        .into_iter()
        .map(|l| {
            let mut out: Vec<P> = Vec::with_capacity(l.len());
            for p in l {
                let q = [(p[0] / SNAP).round() * SNAP, (p[1] / SNAP).round() * SNAP];
                if out.last() != Some(&q) {
                    out.push(q);
                }
            }
            while out.len() > 1 && out.first() == out.last() {
                out.pop();
            }
            // No slivers either: a line run out and back would make a wall of no thickness.
            drop_spikes(out)
        })
        .filter(|l| l.len() >= 3 && signed_area(l).abs() > SNAP * SNAP * 10.0)
        .collect()
}

/// `subject` combined with `clip` by `rule`, outlines anticlockwise and holes clockwise.
pub fn overlay(subject: &[Vec<P>], clip: &[Vec<P>], rule: OverlayRule) -> Shapes {
    let shapes: Shapes = snap_loops(subject.to_vec()).overlay(&snap_loops(clip.to_vec()), rule, FillRule::EvenOdd);
    shapes
        .into_iter()
        .map(|shape| {
            snap_loops(shape)
                .into_iter()
                .enumerate()
                .map(|(k, c)| if (signed_area(&c) > 0.0) == (k == 0) { c } else { c.into_iter().rev().collect() })
                .collect::<Vec<_>>()
        })
        .filter(|s: &Vec<Vec<P>>| !s.is_empty() && signed_area(&s[0]) > 0.0)
        .collect()
}

/// The region the closed loops enclose (even-odd) shrunk by `inset`, exactly: straight lines stay
/// straight, corners where two offsets meet stay sharp, and the region's inward corners become
/// arcs. Each arc is drawn in pieces no longer than `arc_step` of its radius.
pub fn shrunk(loops: &[Vec<P>], inset: f64, arc_step: f64) -> Vec<Vec<P>> {
    use i_overlay::mesh::float::outline::offset::OutlineOffset;
    use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
    let region = overlay(loops, &[], OverlayRule::Subject);
    if region.is_empty() {
        return Vec::new();
    }
    let style = OutlineStyle::new(-inset).line_join(LineJoin::Round(arc_step));
    let out: Shapes = region.outline(&style);
    out.into_iter().flatten().filter(|l| l.len() >= 3).collect()
}

pub fn loops_of(shapes: &Shapes) -> Vec<Vec<P>> {
    shapes.iter().flatten().cloned().collect()
}

/// The band a tool of `width` cuts along a line, round at its ends and corners.
pub fn band(path: &[P], closed: bool, width: f64) -> Vec<Vec<P>> {
    use i_overlay::mesh::float::stroke::offset::StrokeOffset;
    use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle};
    let style = StrokeStyle::new(width).line_join(LineJoin::Round(0.1)).start_cap(LineCap::Round(0.1)).end_cap(LineCap::Round(0.1));
    let shapes: Shapes = path.to_vec().stroke(style, closed);
    loops_of(&overlay(&shapes.into_iter().flatten().collect::<Vec<_>>(), &[], OverlayRule::Subject))
}

/// The union of several regions, each read even-odd on its own: overlapping regions stay
/// removed where they overlap (put together as loops, even-odd would cancel the overlap).
fn union_all<'a>(regions: impl IntoIterator<Item = &'a Vec<Vec<P>>>) -> Vec<Vec<P>> {
    let mut acc: Vec<Vec<P>> = Vec::new();
    for r in regions {
        let own = loops_of(&overlay(r, &[], OverlayRule::Subject));
        acc = if acc.is_empty() { own } else { loops_of(&overlay(&acc, &own, OverlayRule::Union)) };
    }
    acc
}

/// A loop with its slivers taken out: where it comes back to a point it has already passed, the
/// part in between is a loop of its own, dropped when it is only a sliver (narrower on average
/// than a few grid steps), kept in place otherwise.
fn drop_spikes(l: Vec<P>) -> Vec<P> {
    // Twice, the second time starting halfway round, so a sliver across the loop's start goes too.
    let once = drop_spikes_from_start(l);
    let half = once.len() / 2;
    let mut turned = once;
    turned.rotate_left(half);
    let mut out = drop_spikes_from_start(turned);
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn drop_spikes_from_start(l: Vec<P>) -> Vec<P> {
    let thin = |part: &[P]| {
        let perimeter: f64 = (0..part.len()).map(|k| (part[(k + 1) % part.len()][0] - part[k][0]).hypot(part[(k + 1) % part.len()][1] - part[k][1])).sum();
        part.len() < 3 || 2.0 * signed_area(part).abs() / perimeter.max(1e-12) <= SNAP * 10.0
    };
    let mut out: Vec<P> = Vec::with_capacity(l.len());
    for p in l {
        if let Some(i) = out.iter().rposition(|q| *q == p) {
            if thin(&out[i..]) {
                out.truncate(i + 1);
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// Material taken away down to `depth` from the top.
pub struct Removal {
    pub region: Vec<Vec<P>>,
    pub depth: f64,
}

/// One part as a solid, the way a CAD program models it: its outline extruded to the board's
/// `thickness`, less each removal cut straight down to its depth (through when as deep as the
/// board). Flat floors and straight walls, every line from the drawing. Lengths times `scale`.
pub fn part_solid(outline: &Shapes, removals: &[Removal], thickness: f64, scale: f64) -> Option<Solid> {
    let part = loops_of(outline);
    let bottom = -thickness;
    let through: Vec<Vec<P>> = union_all(removals.iter().filter(|r| r.depth >= thickness - 1e-6).map(|r| &r.region));
    let base = if through.is_empty() { overlay(&part, &[], OverlayRule::Subject) } else { overlay(&part, &through, OverlayRule::Difference) };
    if base.is_empty() {
        return None;
    }
    // The levels: the top, then each depth something is cut to, deepest last.
    let mut levels: Vec<f64> = vec![0.0];
    for r in removals.iter().filter(|r| r.depth > 1e-6 && r.depth < thickness - 1e-6) {
        levels.push(-r.depth);
    }
    levels.sort_by(|a, b| b.total_cmp(a));
    levels.dedup_by(|a, b| (*b - *a).abs() < 1e-6);
    let below = |j: usize| levels.get(j + 1).copied().unwrap_or(bottom);
    // Each slab: the part less everything cut deeper than its top; each inside the one below.
    let slabs: Vec<Shapes> = (0..levels.len())
        .map(|j| {
            let deeper: Vec<Vec<P>> = union_all(removals.iter().filter(|r| r.depth > -levels[j] + 1e-6 && r.depth < thickness - 1e-6).map(|r| &r.region));
            if deeper.is_empty() { base.clone() } else { overlay(&loops_of(&base), &deeper, OverlayRule::Difference) }
        })
        .collect();
    // The faces: each slab's top less the slab above it, and the underside.
    let mut flats: Vec<(f64, Shapes, bool)> = Vec::new();
    for j in 0..slabs.len() {
        let top = if j == 0 { slabs[0].clone() } else { overlay(&loops_of(&slabs[j]), &loops_of(&slabs[j - 1]), OverlayRule::Difference) };
        if !top.is_empty() {
            flats.push((levels[j], top, true));
        }
    }
    flats.push((bottom, slabs.last().cloned().unwrap_or_default(), false));
    // Each face's points welded onto the slabs' outline points they stand for: a boolean
    // operation can round a point one step of the grid the other way, which would leave an edge
    // matching no wall.
    let weld_key = |p: P| ((p[0] / (SNAP * 4.0)).floor() as i64, (p[1] / (SNAP * 4.0)).floor() as i64);
    let mut slab_points: HashMap<(i64, i64), Vec<P>> = HashMap::new();
    for p in slabs.iter().flatten().flatten().flatten() {
        slab_points.entry(weld_key(*p)).or_default().push(*p);
    }
    let weld = |p: P| -> P {
        let (kx, ky) = weld_key(p);
        let mut best: Option<(f64, P)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for q in slab_points.get(&(kx + dx, ky + dy)).into_iter().flatten() {
                    let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                    if d <= SNAP * 3.0 && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, *q));
                    }
                }
            }
        }
        best.map_or(p, |(_, q)| q)
    };
    let flats: Vec<(f64, Shapes, bool)> = flats
        .into_iter()
        .map(|(z, shapes, up)| {
            let shapes: Shapes = shapes
                .into_iter()
                .filter_map(|shape| {
                    let loops: Vec<Vec<P>> = shape
                        .into_iter()
                        .map(|l| {
                            let mut out: Vec<P> = Vec::with_capacity(l.len());
                            for p in l.into_iter().map(weld) {
                                if out.last() != Some(&p) {
                                    out.push(p);
                                }
                            }
                            while out.len() > 1 && out.first() == out.last() {
                                out.pop();
                            }
                            drop_spikes(out)
                        })
                        // A loop narrower on average than a few grid steps is a sliver left
                        // between two edges that are one, not a face.
                        .filter(|l| {
                            let perimeter: f64 = (0..l.len()).map(|k| (l[(k + 1) % l.len()][0] - l[k][0]).hypot(l[(k + 1) % l.len()][1] - l[k][1])).sum();
                            l.len() >= 3 && 2.0 * signed_area(l).abs() / perimeter.max(1e-12) > SNAP * 10.0
                        })
                        .collect();
                    (!loops.is_empty() && signed_area(&loops[0]) > 0.0).then_some(loops)
                })
                .collect();
            (z, shapes, up)
        })
        .collect();
    // Every corner of every face and slab, to put the same points on every edge through them.
    let key = |p: P| ((p[0] / SNAP).round() as i64, (p[1] / SNAP).round() as i64);
    let mut corners: Vec<P> = Vec::new();
    let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    for p in flats.iter().flat_map(|(_, s, _)| s.iter().flatten().flatten()).chain(slabs.iter().flatten().flatten().flatten()) {
        if seen.insert(key(*p)) {
            corners.push(*p);
        }
    }
    let (lo, hi) = corners.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    let bucket = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / (corners.len() as f64).sqrt().max(1.0)).max(SNAP * 100.0);
    let cell = |p: P| (((p[0] - lo[0]) / bucket).floor() as i64, ((p[1] - lo[1]) / bucket).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in corners.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(i);
    }
    let on_edge = |a: P, b: P| -> Vec<P> {
        let len2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
        let steps = (len2.sqrt() / bucket).ceil() as usize + 1;
        let mut cells: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let c = cell([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    cells.insert((c.0 + dx, c.1 + dy));
                }
            }
        }
        let mut found: Vec<(f64, P)> = Vec::new();
        for c in cells {
            for &i in grid.get(&c).into_iter().flatten() {
                let q = corners[i];
                if key(q) == key(a) || key(q) == key(b) || dist2_seg(q, a, b) > (SNAP * 3.0).powi(2) {
                    continue;
                }
                let t = ((q[0] - a[0]) * (b[0] - a[0]) + (q[1] - a[1]) * (b[1] - a[1])) / len2.max(1e-30);
                if t > 0.0 && t < 1.0 {
                    found.push((t, q));
                }
            }
        }
        found.sort_by(|x, y| x.0.total_cmp(&y.0));
        found.dedup_by(|x, y| key(x.1) == key(y.1));
        found.into_iter().map(|(_, q)| q).collect()
    };
    // Every outline with the shared points put on its edges, then cleared of the slivers that
    // can show up only then (a line run out to a point and back past where it started).
    let refine = |ring: &Vec<P>| -> Vec<P> {
        let mut full: Vec<P> = Vec::new();
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            full.push(a);
            full.extend(on_edge(a, b));
        }
        drop_spikes(full)
    };
    let s = |p: [f64; 3]| [p[0] * scale, p[1] * scale, p[2] * scale];
    let mut walls: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut rings: Vec<Ring> = Vec::new();
    for (j, slab) in slabs.iter().enumerate() {
        let (top, low) = (levels[j], below(j));
        for ring in slab.iter().flatten().map(refine).filter(|r| r.len() >= 3) {
            rings.push(Ring { pts: ring.iter().map(|p| [p[0] * scale, p[1] * scale]).collect(), low: low * scale, top: top * scale });
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                walls.push(vec![s([a[0], a[1], low]), s([b[0], b[1], low]), s([b[0], b[1], top]), s([a[0], a[1], top])]);
            }
        }
    }
    let mut out: Vec<Flat> = Vec::new();
    for (z, shapes, up) in &flats {
        for shape in shapes {
            let loops: Vec<Vec<P>> = shape
                .iter()
                .map(refine)
                .filter(|r| r.len() >= 3)
                .map(|full| {
                    let full: Vec<P> = full.into_iter().map(|p| [p[0] * scale, p[1] * scale]).collect();
                    if *up { full } else { full.into_iter().rev().collect() }
                })
                .collect();
            if !loops.is_empty() {
                out.push(Flat { z: z * scale, loops, up: *up });
            }
        }
    }
    Some(Solid { flats: out, rings, walls })
}

/// A circle the drawing's points lie on, in millimetres.
#[derive(Clone, Copy, Debug)]
struct Circle {
    c: P,
    r: f64,
}

/// The circle through three points, if they aren't in a line.
fn circle_through(a: P, b: P, c: P) -> Option<Circle> {
    let (bx, by, cx, cy) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    if d.abs() < 1e-15 {
        return None;
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let x = (cy * b2 - by * c2) / d;
    let y = (bx * c2 - cx * b2) / d;
    Some(Circle { c: [a[0] + x, a[1] + y], r: x.hypot(y) })
}

/// The fewest edges in a row taken for an arc: the drawing's arcs have at least four
/// (`geom::arc`), a corner cut by two lines has two.
const ARC_LEAST_EDGES: usize = 4;
/// The longest edge of an arc against its radius, about 20 degrees: the drawing's arcs step by
/// 3 degrees or less.
const ARC_LONGEST_EDGE: f64 = 0.35;
/// The widest circle taken for an arc, in millimetres: beyond, a slightly wavy line.
const ARC_WIDEST: f64 = 10_000.0;

/// For each edge of a closed outline (from point k to k + 1), the circle it runs along: runs of
/// at least four short edges turning one way, every point on one circle within `tol`.
fn ring_arcs(pts: &[P], tol: f64) -> Vec<Option<Circle>> {
    let n = pts.len();
    let mut out = vec![None; n];
    if n <= ARC_LEAST_EDGES {
        return out;
    }
    let at = |i: usize| pts[i % n];
    let on = |c: &Circle, p: P| ((p[0] - c.c[0]).hypot(p[1] - c.c[1]) - c.r).abs() <= tol;
    // The turn at point i + 1.
    let turn = |i: usize| {
        let (a, b, c) = (at(i), at(i + 1), at(i + 2));
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    };
    let short = |c: &Circle, i: usize| (at(i + 1)[0] - at(i)[0]).hypot(at(i + 1)[1] - at(i)[1]) <= c.r * ARC_LONGEST_EDGE;
    let sane = |c: &Circle| c.r > tol * 10.0 && c.r < ARC_WIDEST;
    let one_way = |from: usize, edges: usize| {
        let sign = turn(from).signum();
        (0..edges - 1).all(|k| turn(from + k).signum() == sign && turn(from + k) != 0.0)
    };
    // The whole outline on one circle: a round hole.
    if let Some(c) = circle_through(at(0), at(n / 3), at(2 * n / 3)) {
        if sane(&c) && (0..n).all(|k| on(&c, at(k)) && short(&c, k)) && one_way(0, n) {
            return vec![Some(c); n];
        }
    }
    // Points `from` to `from + edges` on one circle, curving more than the tolerance.
    let fits = |from: usize, edges: usize| -> Option<Circle> {
        let c = circle_through(at(from), at(from + edges / 2), at(from + edges))?;
        let chord = (at(from + edges)[0] - at(from)[0]).hypot(at(from + edges)[1] - at(from)[1]).min(2.0 * c.r);
        let sagitta = c.r - (c.r * c.r - chord * chord / 4.0).max(0.0).sqrt();
        (sane(&c) && sagitta > tol * 2.0 && (0..=edges).all(|k| on(&c, at(from + k))) && (0..edges).all(|k| short(&c, from + k)) && one_way(from, edges)).then_some(c)
    };
    // From a place no arc runs through, each arc as long as it goes.
    let start = (0..n).find(|i| fits(*i, ARC_LEAST_EDGES).is_none()).unwrap_or(0);
    let mut i = 0;
    while i + ARC_LEAST_EDGES <= n {
        let Some(c) = fits(start + i, ARC_LEAST_EDGES) else {
            i += 1;
            continue;
        };
        let sign = turn(start + i).signum();
        let mut edges = ARC_LEAST_EDGES;
        while i + edges < n && on(&c, at(start + i + edges + 1)) && short(&c, start + i + edges) && turn(start + i + edges - 1).signum() == sign {
            edges += 1;
        }
        let c = fits(start + i, edges).unwrap_or(c);
        for k in 0..edges {
            out[(start + i + k) % n] = Some(c);
        }
        i += edges;
    }
    out
}

type K2 = (i64, i64);
type K3 = [i64; 3];

fn k2(p: P) -> K2 {
    ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64)
}
fn kz(z: f64) -> i64 {
    (z * 1e6).round() as i64
}
fn k3(p: [f64; 3]) -> K3 {
    p.map(|v| (v * 1e6).round() as i64)
}
fn edge_key(a: K2, b: K2) -> (K2, K2) {
    if a < b { (a, b) } else { (b, a) }
}

/// Which circle each edge of the part runs along, and where edges end: an arc of the drawing is
/// one edge from where it meets something else to where it does again (a full circle in two
/// halves), every other edge one line.
struct Arcs {
    circles: Vec<Circle>,
    on: HashMap<(K2, K2), usize>,
    ends: std::collections::HashSet<(i64, K2)>,
    /// Arc ends put exactly on their circle.
    moved: HashMap<K2, P>,
}

/// The walls with those standing one on the other merged into one, where no face meets them in
/// between: a hole through a part with a pocket is one wall from top to bottom, as drawn.
fn stacked(solid: &Solid) -> Vec<Ring> {
    let at_faces: std::collections::HashSet<(i64, K2)> = solid.flats.iter().flat_map(|f| f.loops.iter().flatten().map(move |p| (kz(f.z), k2(*p)))).collect();
    let mut rings: Vec<Option<Ring>> = solid.rings.iter().cloned().map(Some).collect();
    let sign = |r: &Ring| {
        let mut keys: Vec<K2> = r.pts.iter().map(|p| k2(*p)).collect();
        keys.sort();
        keys
    };
    let mut by_foot: HashMap<(i64, Vec<K2>), usize> = HashMap::new();
    for (i, r) in rings.iter().enumerate() {
        by_foot.insert((kz(r.as_ref().unwrap().low), sign(r.as_ref().unwrap())), i);
    }
    let mut order: Vec<usize> = (0..rings.len()).collect();
    order.sort_by(|a, b| rings[*a].as_ref().unwrap().low.total_cmp(&rings[*b].as_ref().unwrap().low));
    for i in order {
        let Some(mut r) = rings[i].take() else { continue };
        let key = sign(&r);
        while !r.pts.iter().any(|p| at_faces.contains(&(kz(r.top), k2(*p)))) {
            let Some(&j) = by_foot.get(&(kz(r.top), key.clone())) else { break };
            let Some(above) = rings[j].take() else { break };
            r.top = above.top;
        }
        rings[i] = Some(r);
    }
    rings.into_iter().flatten().collect()
}

impl Arcs {
    fn find(flats: &[Flat], rings: &[Ring], tol: f64) -> Arcs {
        let mut circles: Vec<Circle> = Vec::new();
        let mut on: HashMap<(K2, K2), usize> = HashMap::new();
        for ring in rings {
            let n = ring.pts.len();
            for (k, c) in ring_arcs(&ring.pts, tol).into_iter().enumerate() {
                let Some(c) = c else { continue };
                let id = match circles.iter().position(|d| (d.c[0] - c.c[0]).hypot(d.c[1] - c.c[1]) <= tol * 2.0 && (d.r - c.r).abs() <= tol * 2.0) {
                    Some(id) => id,
                    None => {
                        circles.push(c);
                        circles.len() - 1
                    }
                };
                on.entry(edge_key(k2(ring.pts[k]), k2(ring.pts[(k + 1) % n]))).or_insert(id);
            }
        }
        // Every edge at each height, from the faces and the walls' tops and feet.
        let mut meeting: HashMap<(i64, K2), std::collections::HashSet<(K2, K2)>> = HashMap::new();
        let mut add = |z: f64, l: &[P]| {
            for k in 0..l.len() {
                let (a, b) = (k2(l[k]), k2(l[(k + 1) % l.len()]));
                for v in [a, b] {
                    meeting.entry((kz(z), v)).or_default().insert(edge_key(a, b));
                }
            }
        };
        for flat in flats {
            for l in &flat.loops {
                add(flat.z, l);
            }
        }
        for ring in rings {
            add(ring.low, &ring.pts);
            add(ring.top, &ring.pts);
        }
        let mut ends = std::collections::HashSet::new();
        let mut bounds: HashMap<(usize, i64), (K2, K2)> = HashMap::new();
        for (&(z, v), edges) in &meeting {
            let circles_here: Vec<Option<usize>> = edges.iter().map(|e| on.get(e).copied()).collect();
            if edges.len() != 2 || circles_here[0].is_none() || circles_here[0] != circles_here[1] {
                ends.insert((z, v));
            }
            for c in circles_here.into_iter().flatten() {
                let b = bounds.entry((c, z)).or_insert((v, v));
                b.0 = b.0.min(v);
                b.1 = b.1.max(v);
            }
        }
        // A whole circle is cut in two, so that no edge comes back on itself.
        for ((_, z), (lo, hi)) in bounds {
            ends.insert((z, lo));
            ends.insert((z, hi));
        }
        // A wall's top and foot end at the same places: an end at one height is one at the other,
        // and a wall all round at least twice.
        loop {
            let mut changed = false;
            for ring in rings {
                let (lo, hi) = (kz(ring.low), kz(ring.top));
                for p in &ring.pts {
                    let v = k2(*p);
                    if ends.contains(&(lo, v)) != ends.contains(&(hi, v)) {
                        ends.insert((lo, v));
                        ends.insert((hi, v));
                        changed = true;
                    }
                }
                if ring.pts.iter().filter(|p| ends.contains(&(hi, k2(**p)))).count() < 2 {
                    for p in [ring.pts[0], ring.pts[ring.pts.len() / 2]] {
                        ends.insert((lo, k2(p)));
                        ends.insert((hi, k2(p)));
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut moved = HashMap::new();
        for ring in rings {
            let n = ring.pts.len();
            for (i, p) in ring.pts.iter().enumerate() {
                let v = k2(*p);
                if moved.contains_key(&v) {
                    continue;
                }
                let circle = on.get(&edge_key(v, k2(ring.pts[(i + 1) % n]))).or_else(|| on.get(&edge_key(k2(ring.pts[(i + n - 1) % n]), v)));
                if let Some(&c) = circle {
                    let c = circles[c];
                    let (dx, dy) = (p[0] - c.c[0], p[1] - c.c[1]);
                    let d = dx.hypot(dy).max(1e-12);
                    moved.insert(v, [c.c[0] + dx / d * c.r, c.c[1] + dy / d * c.r]);
                }
            }
        }
        Arcs { circles, on, ends, moved }
    }
    fn at(&self, p: P) -> P {
        self.moved.get(&k2(p)).copied().unwrap_or(p)
    }
    fn circle(&self, a: P, b: P) -> Option<usize> {
        self.on.get(&edge_key(k2(a), k2(b))).copied()
    }
    /// A closed loop at height `z` cut where its edges end: each piece's points and its circle.
    fn pieces(&self, l: &[P], z: f64) -> Vec<(Vec<P>, Option<usize>)> {
        let n = l.len();
        let end = |p: P| self.ends.contains(&(kz(z), k2(p)));
        let first = (0..n).find(|i| end(l[*i])).unwrap_or(0);
        let mut out = Vec::new();
        let mut piece = vec![l[first]];
        for step in 1..=n {
            let p = l[(first + step) % n];
            piece.push(p);
            if end(p) || step == n {
                let circle = self.circle(piece[0], piece[1]);
                out.push((std::mem::replace(&mut piece, vec![p]), circle));
            }
        }
        out
    }
}

/// Numbered STEP entities, each point, vertex and edge written once.
struct Writer {
    lines: Vec<String>,
    vertices: HashMap<K3, usize>,
    edges: HashMap<(K3, K3, usize, Option<K3>), (usize, K3)>,
}

impl Writer {
    fn add(&mut self, entity: String) -> usize {
        self.lines.push(format!("#{}={entity};", self.lines.len() + 1));
        self.lines.len()
    }
    fn point(&mut self, p: [f64; 3]) -> usize {
        self.add(format!("CARTESIAN_POINT('',({:.6},{:.6},{:.6}))", p[0], p[1], p[2]))
    }
    fn direction(&mut self, d: [f64; 3]) -> usize {
        self.add(format!("DIRECTION('',({:.9},{:.9},{:.9}))", d[0], d[1], d[2]))
    }
    fn placement(&mut self, at: [f64; 3], axis: [f64; 3], along: [f64; 3]) -> usize {
        let o = self.point(at);
        let n = self.direction(axis);
        let r = self.direction(along);
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{n},#{r})"))
    }
    fn vertex(&mut self, p: [f64; 3]) -> usize {
        if let Some(v) = self.vertices.get(&k3(p)) {
            return *v;
        }
        let at = self.point(p);
        let v = self.add(format!("VERTEX_POINT('',#{at})"));
        self.vertices.insert(k3(p), v);
        v
    }
    /// The edge through `pts` (a line, or an arc of `circle` at their height), and whether it
    /// runs their way.
    fn edge(&mut self, pts: &[[f64; 3]], circle: Option<(usize, Circle)>) -> (usize, bool) {
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        let (ka, kb) = (k3(a), k3(b));
        let middle = pts[1..pts.len() - 1].iter().map(|p| k3(*p)).min();
        let key = (ka.min(kb), ka.max(kb), circle.map_or(usize::MAX, |c| c.0), circle.and(middle));
        if let Some((e, from)) = self.edges.get(&key) {
            return (*e, *from == ka);
        }
        let (va, vb) = (self.vertex(a), self.vertex(b));
        let e = match circle {
            None => {
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
                let o = self.point(a);
                let dir = self.direction(d.map(|v| v / len));
                let vector = self.add(format!("VECTOR('',#{dir},{len:.6})"));
                let line = self.add(format!("LINE('',#{o},#{vector})"));
                self.add(format!("EDGE_CURVE('',#{va},#{vb},#{line},.T.)"))
            }
            Some((_, c)) => {
                let m = pts[1];
                let anticlockwise = (a[0] - c.c[0]) * (m[1] - c.c[1]) - (a[1] - c.c[1]) * (m[0] - c.c[0]) > 0.0;
                let axis = self.placement([c.c[0], c.c[1], a[2]], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
                let curve = self.add(format!("CIRCLE('',#{axis},{:.6})", c.r));
                self.add(format!("EDGE_CURVE('',#{va},#{vb},#{curve},{})", if anticlockwise { ".T." } else { ".F." }))
            }
        };
        self.edges.insert(key, (e, ka));
        (e, true)
    }
    fn bound(&mut self, edges: &[(usize, bool)], outer: bool) -> usize {
        let oriented: Vec<String> = edges.iter().map(|(e, way)| format!("#{}", self.add(format!("ORIENTED_EDGE('',*,*,#{e},{})", if *way { ".T." } else { ".F." })))).collect();
        let lp = self.add(format!("EDGE_LOOP('',({}))", oriented.join(",")));
        self.add(format!("{}('',#{lp},.T.)", if outer { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }))
    }
}

/// A STEP name: no quote, backslash or letter outside ASCII.
fn step_name(name: &str) -> String {
    name.chars().map(|c| if c == '\'' || c == '\\' || !c.is_ascii() { '_' } else { c }).collect()
}

/// The solid's faces, edges and points written out, and its boundary (`MANIFOLD_SOLID_BREP`):
/// flat faces and walls, the drawing's arcs as arcs and the walls along them as cylinders.
fn brep_into(w: &mut Writer, solid: &Solid, name: &str) -> usize {
    // Within the drawing's points: kept on a grid of 1e-4 of the job's unit, an inch at most.
    let rings = stacked(solid);
    let arcs = Arcs::find(&solid.flats, &rings, SNAP * 4.0 * 25.4);
    w.vertices.clear();
    w.edges.clear();
    let lift = |p: P, z: f64| {
        let p = arcs.at(p);
        [p[0], p[1], z]
    };
    let mut faces: Vec<usize> = Vec::new();
    for flat in &solid.flats {
        let mut bounds = Vec::new();
        for (k, l) in flat.loops.iter().enumerate() {
            let edges: Vec<(usize, bool)> = arcs
                .pieces(l, flat.z)
                .into_iter()
                .map(|(pts, c)| {
                    let pts: Vec<[f64; 3]> = pts.iter().map(|p| lift(*p, flat.z)).collect();
                    w.edge(&pts, c.map(|c| (c, arcs.circles[c])))
                })
                .collect();
            bounds.push(format!("#{}", w.bound(&edges, k == 0)));
        }
        let axis = w.placement([0.0, 0.0, flat.z], [0.0, 0.0, if flat.up { 1.0 } else { -1.0 }], [1.0, 0.0, 0.0]);
        let plane = w.add(format!("PLANE('',#{axis})"));
        faces.push(w.add(format!("ADVANCED_FACE('',({}),#{plane},.T.)", bounds.join(","))));
    }
    // Each wall from one end to the next: its foot along, up, its top back, down.
    for ring in &rings {
        let pieces = arcs.pieces(&ring.pts, ring.top);
        for (pts, c) in pieces {
            let low: Vec<[f64; 3]> = pts.iter().map(|p| lift(*p, ring.low)).collect();
            let top: Vec<[f64; 3]> = pts.iter().rev().map(|p| lift(*p, ring.top)).collect();
            let circle = c.map(|c| (c, arcs.circles[c]));
            let (a, b) = (low[0], low[low.len() - 1]);
            let edges = [
                w.edge(&low, circle),
                w.edge(&[b, top[0]], None),
                w.edge(&top, circle),
                w.edge(&[top[top.len() - 1], a], None),
            ];
            let bound = w.bound(&edges, true);
            let (dx, dy) = (low[1][0] - a[0], low[1][1] - a[1]);
            let face = match circle {
                None => {
                    let len = dx.hypot(dy).max(1e-12);
                    let axis = w.placement(a, [dy / len, -dx / len, 0.0], [0.0, 0.0, 1.0]);
                    let plane = w.add(format!("PLANE('',#{axis})"));
                    w.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"))
                }
                Some((_, c)) => {
                    let axis = w.placement([c.c[0], c.c[1], 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
                    let surface = w.add(format!("CYLINDRICAL_SURFACE('',#{axis},{:.6})", c.r));
                    // Facing away from the axis where the part is on the axis's side.
                    let mid = [(a[0] + low[1][0]) / 2.0, (a[1] + low[1][1]) / 2.0];
                    let outwards = dy * (mid[0] - c.c[0]) - dx * (mid[1] - c.c[1]) > 0.0;
                    w.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},{})", if outwards { ".T." } else { ".F." }))
                }
            };
            faces.push(face);
        }
    }
    let list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
    let shell = w.add(format!("CLOSED_SHELL('',({}))", list.join(",")));
    w.add(format!("MANIFOLD_SOLID_BREP('{name}',#{shell})"))
}

/// The entities every STEP file here shares: millimetres, the geometric context, and the
/// application, product and definition contexts.
struct StepContexts {
    geometry: usize,
    product: usize,
    definition: usize,
}

fn contexts_into(w: &mut Writer) -> StepContexts {
    let mm = w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = w.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sr = w.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let fuzz = w.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{mm},'distance_accuracy_value','')"));
    let geometry = w.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{fuzz}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('',''))"));
    let app = w.add("APPLICATION_CONTEXT('automotive design')".into());
    w.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let product = w.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let definition = w.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    StepContexts { geometry, product, definition }
}

/// A product named `name` with its shape `items` (a representation of `kind`): its definition,
/// its representation and its placement at the origin.
fn product_into(w: &mut Writer, c: &StepContexts, name: &str, kind: &str, items: &[usize]) -> (usize, usize, usize) {
    let axis = w.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let list: Vec<String> = items.iter().chain(std::iter::once(&axis)).map(|i| format!("#{i}")).collect();
    let rep = w.add(format!("{kind}('{name}',({}),#{})", list.join(","), c.geometry));
    let product = w.add(format!("PRODUCT('{name}','{name}','',(#{}))", c.product));
    w.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}))"));
    let formation = w.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let definition = w.add(format!("PRODUCT_DEFINITION('design','',#{formation},#{})", c.definition));
    let shape = w.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    w.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"));
    (definition, rep, axis)
}

fn step_file(name: &str, w: Writer) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Burin carved piece'),'2;1');\nFILE_NAME('{name}.step','',(''),(''),'Burin','Burin','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n{}\nENDSEC;\nEND-ISO-10303-21;\n",
        w.lines.join("\n")
    )
}

/// The solid as a STEP file (AP214) named `name`: flat faces and walls, the drawing's arcs as
/// arcs and the walls along them as cylinders, as a CAD program models them.
pub fn step_text(solid: &Solid, name: &str) -> String {
    let name = step_name(name);
    let mut w = Writer { lines: Vec::new(), vertices: HashMap::new(), edges: HashMap::new() };
    let brep = brep_into(&mut w, solid, &name);
    let c = contexts_into(&mut w);
    product_into(&mut w, &c, &name, "ADVANCED_BREP_SHAPE_REPRESENTATION", &[brep]);
    step_file(&name, w)
}

/// Every part in one STEP assembly named `name`: each part a product of its own placed in the
/// assembly where it lies on the sheet (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`, with an identity
/// placement), so a CAD program opens them all at once as separate components to put together.
pub fn step_assembly(parts: &[(String, Solid)], name: &str) -> String {
    let name = step_name(name);
    let mut w = Writer { lines: Vec::new(), vertices: HashMap::new(), edges: HashMap::new() };
    let c = contexts_into(&mut w);
    let (whole, whole_rep, whole_axis) = product_into(&mut w, &c, &name, "SHAPE_REPRESENTATION", &[]);
    for (k, (part, solid)) in parts.iter().enumerate() {
        let part = step_name(part);
        let brep = brep_into(&mut w, solid, &part);
        let (definition, rep, axis) = product_into(&mut w, &c, &part, "ADVANCED_BREP_SHAPE_REPRESENTATION", &[brep]);
        let usage = w.add(format!("NEXT_ASSEMBLY_USAGE_OCCURRENCE('{}','{part}','',#{whole},#{definition},$)", k + 1));
        let usage_shape = w.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{usage})"));
        let moved = w.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{axis},#{whole_axis})"));
        let relation = w.add(format!("(REPRESENTATION_RELATIONSHIP('','',#{rep},#{whole_rep})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{moved})SHAPE_REPRESENTATION_RELATIONSHIP())"));
        w.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relation},#{usage_shape})"));
    }
    step_file(&name, w)
}

/// One flat face cut into triangles that keep every edge of its loops: a constrained Delaunay
/// triangulation of its points, of which the triangles inside are kept (reached from the left of
/// a loop's edges without crossing one).
fn flat_triangles(flat: &Flat) -> Vec<[P; 3]> {
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut loops = Vec::new();
    for l in &flat.loops {
        let handles: Vec<_> = l.iter().filter_map(|p| cdt.insert(Point2::new(p[0], p[1])).ok()).collect();
        loops.push(handles);
    }
    for h in &loops {
        for k in 0..h.len() {
            let (a, b) = (h[k], h[(k + 1) % h.len()]);
            if a != b {
                cdt.try_add_constraint(a, b);
            }
        }
    }
    // A triangle belongs to the face when its middle does: crossings of the face's loops along a
    // line to its left, odd inside. The loops' edges are filed by rows for speed.
    let edges: Vec<(P, P)> = flat.loops.iter().flat_map(|l| (0..l.len()).map(move |k| (l[k], l[(k + 1) % l.len()]))).collect();
    let (lo, hi) = edges.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (a, b)| (lo.min(a[1]).min(b[1]), hi.max(a[1]).max(b[1])));
    let rows = ((edges.len() as f64).sqrt().ceil() as usize).max(1);
    let height = ((hi - lo) / rows as f64).max(1e-9);
    let row = |y: f64| (((y - lo) / height).floor().max(0.0) as usize).min(rows - 1);
    let mut filed: Vec<Vec<usize>> = vec![Vec::new(); rows];
    for (i, (a, b)) in edges.iter().enumerate() {
        for r in row(a[1].min(b[1]))..=row(a[1].max(b[1])) {
            filed[r].push(i);
        }
    }
    let inside = |p: P| {
        let mut odd = false;
        for &i in &filed[row(p[1])] {
            let (a, b) = edges[i];
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
        odd
    };
    cdt.inner_faces()
        .map(|f| {
            let v = f.vertices().map(|v| [v.position().x, v.position().y]);
            [v[0], v[1], v[2]]
        })
        .filter(|t| inside([(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0]))
        .collect()
}

/// The solid's faces cut into triangles, as a binary STL.
pub fn stl_bytes(solid: &Solid) -> Vec<u8> {
    let mut triangles: Vec<[[f64; 3]; 3]> = Vec::new();
    for flat in &solid.flats {
        for [a, b, c] in flat_triangles(flat) {
            let lift = |p: P| [p[0], p[1], flat.z];
            let anticlockwise = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0;
            triangles.push(if anticlockwise == flat.up { [lift(a), lift(b), lift(c)] } else { [lift(a), lift(c), lift(b)] });
        }
    }
    // The walls are flat four-sided pieces, their corners turning as seen from outside.
    for wall in &solid.walls {
        for k in 1..wall.len() - 1 {
            triangles.push([wall[0], wall[k], wall[k + 1]]);
        }
    }
    let mut out = Vec::with_capacity(84 + triangles.len() * 50);
    let mut header = [0u8; 80];
    let words = b"Burin carved piece, millimetres";
    header[..words.len()].copy_from_slice(words);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for [a, b, c] in triangles {
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-30);
        for v in n.map(|x| x / len).iter().chain(&a).chain(&b).chain(&c) {
            out.extend_from_slice(&(*v as f32).to_le_bytes());
        }
        out.extend_from_slice(&[0u8; 2]);
    }
    out
}
