//! The machined parts as a SolidCraft design, built the way a person models them there: a
//! `thickness` parameter, then for each part a component holding a sketch of its outline (lines,
//! arcs and circles, its through holes in the same sketch) extruded to the thickness, and each
//! pocket, drilling or groove sketched and cut down from the top to its own depth parameter.
//! Change a parameter in SolidCraft and the parts rebuild. Written as a script of SolidCraft's
//! own commands, which its command line replays into a design (`.sc`). Burin's own: the original
//! exports no parts.

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
        commands.push(json!({"command": "component.create", "params": {"name": part.name}}));
        sketch(&mut commands, &base, scale);
        commands.push(json!({"command": "solid.extrude", "params": {
            "distance": "thickness",
            "profiles": base.iter().map(|shape| json!({"point": point(inner_point(shape))})).collect::<Vec<_>>(),
            "operation": "new",
            "name": "Outline",
            "body_name": part.name,
        }}));
        let base_loops = loops_of(&base);
        for (k, d) in depths.iter().enumerate() {
            let at: Vec<Vec<P>> = union_all(part.removals.iter().filter(|r| (r.depth - d).abs() < 1e-6).map(|r| &r.region));
            if at.is_empty() {
                continue;
            }
            let region = clean(overlay(&at, &base_loops, OverlayRule::Intersect));
            if region.is_empty() {
                continue;
            }
            sketch(&mut commands, &region, scale);
            commands.push(json!({"command": "solid.extrude", "params": {
                "distance": format!("depth_{}", k + 1),
                "profiles": region.iter().map(|shape| json!({"point": point(inner_point(shape))})).collect::<Vec<_>>(),
                "operation": "cut",
                "start_offset": "thickness",
                "direction": "negative",
                "name": format!("Cut {}", k + 1),
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

/// A sketch on the XY plane holding every loop of `shapes`: whole circles as circles, runs of
/// points on one circle as arcs, the rest as lines.
fn sketch(commands: &mut Vec<Value>, shapes: &Shapes, scale: f64) {
    let point = |p: P| json!([number(p[0] * scale), number(p[1] * scale)]);
    commands.push(json!({"command": "sketch.create", "params": {"plane": "XY"}}));
    for l in shapes.iter().flatten() {
        for curve in curves(l) {
            commands.push(match curve {
                Curve::Circle(c) => json!({"command": "sketch.circle.center", "params": {"center": point(c.c), "diameter": number(scale * 2.0 * c.r)}}),
                Curve::Arc(a, through, b) => json!({"command": "sketch.arc.three_point", "params": {"start": point(a), "end": point(b), "through": point(through)}}),
                Curve::Lines(pts) => json!({"command": "sketch.line", "params": {"points": pts.into_iter().map(point).collect::<Vec<_>>()}}),
            });
        }
    }
    commands.push(json!({"command": "sketch.finish", "params": {}}));
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
                let (a, b) = (l[edge], l[(start + j) % n]);
                // The middle point put back onto its circle.
                let m = l[(start + (k + j) / 2) % n];
                let len = (m[0] - c.c[0]).hypot(m[1] - c.c[1]).max(1e-12);
                let through = if j - k >= 2 { [c.c[0] + (m[0] - c.c[0]) / len * c.r, c.c[1] + (m[1] - c.c[1]) / len * c.r] } else { m };
                out.push(Curve::Arc(a, through, b));
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

/// SolidCraft's command line, when it is on this computer.
#[cfg(not(target_arch = "wasm32"))]
pub fn command_line() -> Option<std::process::Command> {
    if let Some(found) = on_path(if cfg!(windows) { "solidcraft-cli.exe" } else { "solidcraft-cli" }) {
        return Some(std::process::Command::new(found));
    }
    let image = app_image()?;
    let mut command = std::process::Command::new(image);
    command.env("SOLIDCRAFT_APPIMAGE_CLI", "1");
    Some(command)
}

/// SolidCraft itself, to open a design in.
#[cfg(not(target_arch = "wasm32"))]
pub fn program() -> Option<PathBuf> {
    on_path(if cfg!(windows) { "solidcraft.exe" } else { "solidcraft" }).or_else(app_image)
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
    let folder = std::env::temp_dir().join(format!("burin-solidcraft-{}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join("parts.json");
    std::fs::write(&file, serde_json::to_vec_pretty(script).unwrap_or_default()).map_err(|e| e.to_string())?;
    let out = command.arg("run").arg(&file).arg("--save").arg(target).arg("--quiet").stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&folder);
    if out.status.success() && target.is_file() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(said.lines().filter(|l| !l.trim().is_empty() && !l.contains("installed in")).last().unwrap_or("").to_string())
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

    #[test]
    fn a_point_inside_a_ring_is_in_its_material() {
        let shape = vec![square(0.0, 0.0, 10.0), square(1.0, 1.0, 8.0).into_iter().rev().collect()];
        let q = inner_point(&shape);
        assert!(q[0] > 0.0 && q[0] < 10.0 && q[1] > 0.0 && q[1] < 10.0);
        assert!(!(q[0] > 1.0 && q[0] < 9.0 && q[1] > 1.0 && q[1] < 9.0), "{q:?}");
    }
}
