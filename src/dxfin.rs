//! Reading a DXF drawing: lines, arcs, circles, ellipses, polylines (with their bulges) and splines,
//! each with the layer it is on, and blocks placed by INSERT (moved, scaled, mirrored, turned, in
//! rows and columns, blocks within blocks). Text and hatching are left out. Burin's own reader.

use crate::geom::{arc, circle, dxf_spline, unbow, Path, P};
use std::collections::HashMap;

/// How deep blocks within blocks are followed.
const DEEPEST_BLOCK: usize = 8;
/// The most copies an INSERT's rows and columns make.
const MOST_COPIES: usize = 10_000;

/// A line of the drawing and the DXF layer it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub layer: String,
    pub path: Path,
}

/// One entity: its kind and its code/value pairs (a POLYLINE with its VERTEX pairs after it).
struct Entity<'a> {
    kind: &'a str,
    fields: Vec<(i32, &'a str)>,
}

impl Entity<'_> {
    fn text(&self, code: i32) -> Option<&str> {
        self.fields.iter().find(|f| f.0 == code).map(|f| f.1)
    }
    fn num(&self, code: i32, default: f64) -> f64 {
        self.text(code).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }
}

/// A block: its base point and what it holds.
struct Block<'a> {
    base: P,
    entities: Vec<Entity<'a>>,
}

/// [a b c d e f]: x' = a x + c y + e, y' = b x + d y + f.
type Affine = [f64; 6];
const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn apply(m: &Affine, p: P) -> P {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

/// `inner` then `outer`.
fn then(inner: &Affine, outer: &Affine) -> Affine {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

/// Every line of a DXF drawing, with its layer, blocks placed where they are inserted.
pub fn read(text: &str) -> Vec<Shape> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(i32, &str)> = lines.chunks_exact(2).filter_map(|c| c[0].parse::<i32>().ok().map(|code| (code, c[1]))).collect();
    let mut blocks: HashMap<String, Block> = HashMap::new();
    let mut top: Vec<Entity> = Vec::new();
    let mut section = "";
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code == 0 && value == "SECTION" {
            section = pairs.get(i + 1).filter(|p| p.0 == 2).map_or("", |p| p.1);
            i += 2;
            continue;
        }
        if code == 0 && value == "ENDSEC" {
            section = "";
            i += 1;
            continue;
        }
        match section {
            "BLOCKS" if code == 0 && value == "BLOCK" => {
                let (header, next) = entity_at(&pairs, i);
                let name = header.text(2).unwrap_or("").to_string();
                let base = [header.num(10, 0.0), header.num(20, 0.0)];
                let mut entities = Vec::new();
                i = next;
                while i < pairs.len() && !(pairs[i].0 == 0 && (pairs[i].1 == "ENDBLK" || pairs[i].1 == "ENDSEC")) {
                    let (e, next) = entity_at(&pairs, i);
                    entities.push(e);
                    i = next.max(i + 1);
                }
                blocks.insert(name, Block { base, entities });
            }
            "ENTITIES" if code == 0 => {
                let (e, next) = entity_at(&pairs, i);
                top.push(e);
                i = next.max(i + 1);
            }
            _ => i += 1,
        }
    }
    let mut out = Vec::new();
    for e in &top {
        place(e, &blocks, &IDENTITY, "0", 0, &mut out);
    }
    out
}

/// The entity starting at pair `i` (a 0 code), and where the next one starts.
fn entity_at<'a>(pairs: &[(i32, &'a str)], i: usize) -> (Entity<'a>, usize) {
    let kind = pairs[i].1;
    let mut j = i + 1;
    while j < pairs.len() {
        let (c, v) = pairs[j];
        // A POLYLINE goes on through its VERTEX records to SEQEND.
        if c == 0 && !(kind == "POLYLINE" && v == "VERTEX") {
            break;
        }
        j += 1;
    }
    let fields = pairs[i + 1..j].to_vec();
    if kind == "POLYLINE" && pairs.get(j).is_some_and(|p| p.0 == 0 && p.1 == "SEQEND") {
        // SEQEND has its own pairs (its layer): skipped with it.
        j += 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
    }
    (Entity { kind, fields }, j)
}

/// An entity's lines placed through `m`, on its own layer (one on layer 0 within a block takes the
/// INSERT's, as the DXF format says), blocks followed.
fn place(e: &Entity, blocks: &HashMap<String, Block>, m: &Affine, outer_layer: &str, depth: usize, out: &mut Vec<Shape>) {
    let own = e.text(8).unwrap_or("0");
    let layer = if own == "0" || own.is_empty() { outer_layer } else { own };
    if e.kind == "INSERT" {
        if depth >= DEEPEST_BLOCK {
            return;
        }
        let Some(block) = e.text(2).and_then(|n| blocks.get(n)) else { return };
        let at = [e.num(10, 0.0), e.num(20, 0.0)];
        let (sx, sy) = (e.num(41, 1.0), e.num(42, 1.0));
        let turn = e.num(50, 0.0).to_radians();
        let (cols, rows) = (e.num(70, 1.0).max(1.0) as usize, e.num(71, 1.0).max(1.0) as usize);
        let (col_step, row_step) = (e.num(44, 0.0), e.num(45, 0.0));
        let (cos, sin) = (turn.cos(), turn.sin());
        for r in 0..rows.min(MOST_COPIES) {
            for c in 0..cols.min(MOST_COPIES / rows.max(1)) {
                // Moved to the block's base, scaled, then the copy's place in the rows, turned, then moved.
                let (dx, dy) = (c as f64 * col_step, r as f64 * row_step);
                let local: Affine = [sx, 0.0, 0.0, sy, -block.base[0] * sx + dx, -block.base[1] * sy + dy];
                let turned: Affine = [cos, sin, -sin, cos, at[0], at[1]];
                let here = then(&then(&local, &turned), m);
                for inner in &block.entities {
                    place(inner, blocks, &here, layer, depth + 1, out);
                }
            }
        }
        return;
    }
    for path in entity_paths(e) {
        let pts: Vec<P> = path.pts.iter().map(|p| apply(m, *p)).collect();
        out.push(Shape { layer: layer.to_string(), path: Path { pts, closed: path.closed } });
    }
}

/// An entity's own lines, where it is drawn (before any block's placing).
fn entity_paths(e: &Entity) -> Vec<Path> {
    // Drawn seen from below (extrusion 0, 0, -1): its x is mirrored.
    let below = e.num(230, 1.0) < 0.0;
    let flip = |pts: Vec<P>| -> Vec<P> { if below { pts.into_iter().map(|p| [-p[0], p[1]]).collect() } else { pts } };
    let num = |c: i32| e.num(c, 0.0);
    match e.kind {
        "LINE" => vec![Path::open(vec![[num(10), num(20)], [num(11), num(21)]])],
        "CIRCLE" => vec![Path::closed(flip(circle([num(10), num(20)], num(40))))],
        "ARC" => {
            let sweep = (num(51) - num(50)).rem_euclid(360.0);
            vec![Path::open(flip(arc([num(10), num(20)], num(40), num(50), if sweep == 0.0 { 360.0 } else { sweep })))]
        }
        "ELLIPSE" => {
            let c = [num(10), num(20)];
            let major = [num(11), num(21)];
            let ratio = e.num(40, 1.0);
            let minor = [-major[1] * ratio, major[0] * ratio];
            let (t0, mut t1) = (e.num(41, 0.0), e.num(42, std::f64::consts::TAU));
            if t1 <= t0 {
                t1 += std::f64::consts::TAU;
            }
            let whole = (t1 - t0 - std::f64::consts::TAU).abs() < 1e-9;
            let size = major[0].hypot(major[1]).max(1e-9);
            // Steps no longer than a 200th of a turn, nor straying more than the arcs do.
            let steps = (((t1 - t0) / std::f64::consts::TAU * 200.0).ceil() as usize).max(((t1 - t0) * (size / (8.0 * crate::geom::ARC_SAG)).sqrt()).ceil() as usize).clamp(8, 4000);
            let count = if whole { steps } else { steps + 1 };
            let pts: Vec<P> = (0..count)
                .map(|k| {
                    let t = t0 + (t1 - t0) * k as f64 / steps as f64;
                    [c[0] + t.cos() * major[0] + t.sin() * minor[0], c[1] + t.cos() * major[1] + t.sin() * minor[1]]
                })
                .collect();
            vec![if whole { Path::closed(pts) } else { Path::open(pts) }]
        }
        "LWPOLYLINE" | "POLYLINE" => {
            let mut poly: Vec<P> = Vec::new();
            let mut bulges: Vec<f64> = Vec::new();
            let mut pending_x = None;
            let mut flags = 0i64;
            let mut seen_flags = false;
            for &(c, v) in &e.fields {
                match c {
                    70 if !seen_flags => {
                        flags = v.trim().parse().unwrap_or(0);
                        seen_flags = true;
                    }
                    10 => pending_x = v.trim().parse::<f64>().ok(),
                    20 => {
                        if let (Some(x), Ok(y)) = (pending_x.take(), v.trim().parse::<f64>()) {
                            poly.push([x, y]);
                            bulges.resize(poly.len(), 0.0);
                        }
                    }
                    42 if !poly.is_empty() => {
                        let last = poly.len() - 1;
                        bulges[last] = v.trim().parse().unwrap_or(0.0);
                    }
                    _ => {}
                }
            }
            if e.kind == "POLYLINE" && poly.len() > 1 {
                // The POLYLINE header's own 10/20 is a dummy origin, not a vertex.
                poly.remove(0);
                bulges.remove(0);
            }
            if poly.len() < 2 {
                return Vec::new();
            }
            let closed = flags & 1 == 1;
            vec![Path { pts: flip(unbow(&poly, &bulges, closed)), closed }]
        }
        "SPLINE" => dxf_spline(&e.fields).into_iter().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(blocks: &str, entities: &str) -> String {
        format!("0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    /// A block of a 10 × 5 rectangle inserted twice, once moved, once turned a quarter and doubled;
    /// its layer-0 lines take each INSERT's layer.
    #[test]
    fn blocks_are_placed_where_they_are_inserted() {
        let block = "0\nBLOCK\n8\n0\n2\nPLATE\n10\n0\n20\n0\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n5\n10\n0\n20\n5\n0\nENDBLK\n";
        let inserts = "0\nINSERT\n8\nCUT\n2\nPLATE\n10\n100\n20\n50\n0\nINSERT\n8\nENGRAVE\n2\nPLATE\n10\n0\n20\n0\n41\n2\n42\n2\n50\n90\n";
        let shapes = read(&dxf(block, inserts));
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0].layer, "CUT");
        let (lo, hi) = crate::geom::bbox(std::slice::from_ref(&shapes[0].path)).unwrap();
        assert!((lo[0] - 100.0).abs() < 1e-9 && (hi[0] - 110.0).abs() < 1e-9 && (hi[1] - 55.0).abs() < 1e-9);
        assert_eq!(shapes[1].layer, "ENGRAVE");
        let (lo, hi) = crate::geom::bbox(std::slice::from_ref(&shapes[1].path)).unwrap();
        // Turned a quarter: 20 wide becomes 20 high, going left.
        assert!((lo[0] + 10.0).abs() < 1e-9 && (hi[1] - 20.0).abs() < 1e-9 && hi[0].abs() < 1e-9, "{lo:?} {hi:?}");
    }

    /// An ellipse 20 across and 10 up, whole; half of one; and a circle drawn seen from below.
    #[test]
    fn ellipses_and_mirrored_arcs() {
        let e = "0\nELLIPSE\n8\nA\n10\n50\n20\n50\n11\n10\n21\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n0\nELLIPSE\n8\nA\n10\n0\n20\n0\n11\n10\n21\n0\n40\n0.5\n41\n0\n42\n3.141592653589793\n0\nCIRCLE\n8\nB\n10\n30\n20\n0\n40\n5\n230\n-1\n";
        let shapes = read(&dxf("", e));
        assert_eq!(shapes.len(), 3);
        let (lo, hi) = crate::geom::bbox(std::slice::from_ref(&shapes[0].path)).unwrap();
        assert!(shapes[0].path.closed && (hi[0] - lo[0] - 20.0).abs() < 1e-6 && (hi[1] - lo[1] - 10.0).abs() < 1e-3, "{lo:?} {hi:?}");
        let half = &shapes[1].path;
        assert!(!half.closed && half.pts.iter().all(|p| p[1] >= -1e-9), "the upper half");
        let (lo, _) = crate::geom::bbox(std::slice::from_ref(&shapes[2].path)).unwrap();
        assert!((lo[0] + 35.0).abs() < 1e-6, "mirrored to x = -30: {lo:?}");
        assert_eq!(shapes[2].layer, "B");
    }
}

#[cfg(test)]
mod try_files {
    /// BURIN_DXF_LIST=<file of paths> cargo test try_dxf -- --ignored --nocapture
    #[test]
    #[ignore]
    fn try_dxf() {
        let Some(list) = std::env::var_os("BURIN_DXF_LIST") else { return };
        for f in std::fs::read_to_string(list).unwrap().lines() {
            let Ok(bytes) = std::fs::read(f) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            let new = super::read(&text);
            let old = crate::geom::dxf_read(&text);
            let mut layers: Vec<&str> = new.iter().map(|s| s.layer.as_str()).collect();
            layers.sort();
            layers.dedup();
            let inserts = text.matches("\nINSERT").count();
            eprintln!("{} | old {} new {} layers {:?} inserts {}", f.rsplit('/').next().unwrap_or(f), old.len(), new.len(), &layers[..layers.len().min(6)], inserts);
        }
    }
}
