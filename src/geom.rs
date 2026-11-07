//! 2D geometry: shapes as polylines, distance fields, booleans, text outlines, DXF.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type P = [f64; 2];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub pts: Vec<P>,
    pub closed: bool,
}

impl Path {
    pub fn closed(pts: Vec<P>) -> Self {
        Self { pts, closed: true }
    }
    pub fn open(pts: Vec<P>) -> Self {
        Self { pts, closed: false }
    }
    /// Segments, including the closing one for closed paths.
    pub fn segments(&self) -> impl Iterator<Item = (P, P)> + '_ {
        let n = self.pts.len();
        let count = if self.closed { n } else { n.saturating_sub(1) };
        (0..count).map(move |i| (self.pts[i], self.pts[(i + 1) % n]))
    }
}

const TAU: f64 = std::f64::consts::TAU;

pub fn circle(c: P, r: f64) -> Vec<P> {
    ellipse(c, r, r)
}

pub fn ellipse(c: P, rx: f64, ry: f64) -> Vec<P> {
    (0..128)
        .map(|i| {
            let a = TAU * i as f64 / 128.0;
            [c[0] + rx * a.cos(), c[1] + ry * a.sin()]
        })
        .collect()
}

pub fn rect(c: P, w: f64, h: f64, corner: f64) -> Vec<P> {
    let (hw, hh) = (w.abs() / 2.0, h.abs() / 2.0);
    let r = corner.clamp(0.0, hw.min(hh));
    if r <= 0.0 {
        return vec![
            [c[0] - hw, c[1] - hh],
            [c[0] + hw, c[1] - hh],
            [c[0] + hw, c[1] + hh],
            [c[0] - hw, c[1] + hh],
        ];
    }
    let corners = [
        ([c[0] + hw - r, c[1] - hh + r], -0.25),
        ([c[0] + hw - r, c[1] + hh - r], 0.0),
        ([c[0] - hw + r, c[1] + hh - r], 0.25),
        ([c[0] - hw + r, c[1] - hh + r], 0.5),
    ];
    let mut out = Vec::new();
    for (centre, start) in corners {
        for i in 0..=12 {
            let a = TAU * (start + 0.25 * i as f64 / 12.0);
            out.push([centre[0] + r * a.cos(), centre[1] + r * a.sin()]);
        }
    }
    out
}

pub fn polygon(c: P, r: f64, sides: usize) -> Vec<P> {
    let n = sides.max(3);
    (0..n)
        .map(|i| {
            let a = TAU * (0.25 + i as f64 / n as f64);
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

pub fn star(c: P, outer: f64, inner: f64, points: usize) -> Vec<P> {
    let n = points.max(3) * 2;
    (0..n)
        .map(|i| {
            let a = TAU * (0.25 + i as f64 / n as f64);
            let r = if i % 2 == 0 { outer } else { inner };
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

pub fn spiral(c: P, r: f64, turns: f64) -> Vec<P> {
    let steps = (turns * 72.0).ceil().max(8.0) as usize;
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let a = TAU * turns * t;
            [c[0] + r * t * a.cos(), c[1] + r * t * a.sin()]
        })
        .collect()
}

pub fn arc(c: P, r: f64, start_deg: f64, sweep_deg: f64) -> Vec<P> {
    let steps = (sweep_deg.abs() / 3.0).ceil().max(4.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = (start_deg + sweep_deg * i as f64 / steps as f64).to_radians();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// A smooth curve through the given points.
pub fn smooth_curve(points: &[P]) -> Vec<P> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    let mut out = Vec::new();
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for s in 0..12 {
            let t = s as f64 / 12.0;
            let f = |k: usize| {
                0.5 * (2.0 * p1[k]
                    + (p2[k] - p0[k]) * t
                    + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t * t
                    + (3.0 * p1[k] - p0[k] - 3.0 * p2[k] + p3[k]) * t * t * t)
            };
            out.push([f(0), f(1)]);
        }
    }
    out.push(*points.last().unwrap());
    out
}

pub fn bbox<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<(P, P)> {
    let mut out: Option<(P, P)> = None;
    for p in paths.into_iter().flat_map(|path| path.pts.iter()) {
        let (lo, hi) = out.get_or_insert((*p, *p));
        lo[0] = lo[0].min(p[0]);
        lo[1] = lo[1].min(p[1]);
        hi[0] = hi[0].max(p[0]);
        hi[1] = hi[1].max(p[1]);
    }
    out
}

pub fn dist2_seg(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    };
    let (ex, ey) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    ex * ex + ey * ey
}

pub fn inside_even_odd<'a>(paths: impl IntoIterator<Item = &'a Path>, p: P) -> bool {
    let mut inside = false;
    for path in paths {
        if !path.closed {
            continue;
        }
        for (a, b) in path.segments() {
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            {
                inside = !inside;
            }
        }
    }
    inside
}

/// Positive when counter-clockwise.
pub fn signed_area(pts: &[P]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

/// A regular grid of signed distances: positive inside the shape, negative outside.
#[derive(Clone, Debug)]
pub struct Field {
    pub x0: f64,
    pub y0: f64,
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub d: Vec<f32>,
}

impl Field {
    /// Exact signed distance to the closed paths, with `margin` of room around them.
    pub fn build(paths: &[Path], margin: f64, max_cells: usize) -> Option<Field> {
        Self::build_to(paths, margin, max_cells, 0.0)
    }

    /// The grid holds about `max_cells` squared cells whatever its shape, so a long thin slot is
    /// seen as finely as a square of the same area; none is smaller than `finest`.
    pub fn build_to(paths: &[Path], margin: f64, max_cells: usize, finest: f64) -> Option<Field> {
        let closed: Vec<&Path> = paths.iter().filter(|p| p.closed && p.pts.len() >= 3).collect();
        let (lo, hi) = bbox(closed.iter().copied())?;
        let (wide, tall) = (hi[0] - lo[0] + 2.0 * margin, hi[1] - lo[1] + 2.0 * margin);
        let coarsest = wide.max(tall) / 12_000.0;
        let cell = ((wide.max(1e-6) * tall.max(1e-6)).sqrt() / max_cells as f64).max(finest).max(coarsest).max(1e-6);
        let x0 = lo[0] - margin - cell * 2.0;
        let y0 = lo[1] - margin - cell * 2.0;
        let nx = ((hi[0] - lo[0] + 2.0 * margin) / cell).ceil() as usize + 5;
        let ny = ((hi[1] - lo[1] + 2.0 * margin) / cell).ceil() as usize + 5;
        let segs: Vec<(P, P)> = closed.iter().flat_map(|p| p.segments()).collect();
        let mut d = vec![0f32; nx * ny];
        // One core is left for the window, so it stays alive on a modest computer.
        let threads = crate::platform::threads(true);
        let rows_per = ny.div_ceil(threads).max(1);
        crate::platform::scope(|s| {
            for (ci, chunk) in d.chunks_mut(nx * rows_per).enumerate() {
                let segs = &segs;
                s.spawn(move || {
                    for (r, row) in chunk.chunks_mut(nx).enumerate() {
                        let y = y0 + (ci * rows_per + r) as f64 * cell;
                        // Even-odd crossings of this row, for a fast inside test.
                        let mut xs: Vec<f64> = segs
                            .iter()
                            .filter(|(a, b)| (a[1] > y) != (b[1] > y))
                            .map(|(a, b)| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
                            .collect();
                        xs.sort_by(f64::total_cmp);
                        let mut crossed = 0;
                        for (i, v) in row.iter_mut().enumerate() {
                            let x = x0 + i as f64 * cell;
                            while crossed < xs.len() && xs[crossed] <= x {
                                crossed += 1;
                            }
                            let best = segs
                                .iter()
                                .map(|(a, b)| dist2_seg([x, y], *a, *b))
                                .fold(f64::MAX, f64::min)
                                .sqrt();
                            *v = if crossed % 2 == 1 { best as f32 } else { -(best as f32) };
                        }
                    }
                });
            }
        });
        Some(Field { x0, y0, cell, nx, ny, d })
    }

    pub fn at(&self, ix: usize, iy: usize) -> f32 {
        self.d[iy * self.nx + ix]
    }

    /// Interpolated value at a point; far negative outside the grid.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let (fx, fy) = ((x - self.x0) / self.cell, (y - self.y0) / self.cell);
        if fx < 0.0 || fy < 0.0 || fx >= (self.nx - 1) as f64 || fy >= (self.ny - 1) as f64 {
            return -1e9;
        }
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
        let a = self.at(ix, iy) * (1.0 - tx) + self.at(ix + 1, iy) * tx;
        let b = self.at(ix, iy + 1) * (1.0 - tx) + self.at(ix + 1, iy + 1) * tx;
        (a * (1.0 - ty) + b * ty) as f64
    }

    /// Closed loops where the field equals `level` (marching squares).
    pub fn contours(&self, level: f64) -> Vec<Vec<P>> {
        let level = level as f32;
        // An edge id: 2*(node index) for the horizontal edge from that node, +1 for the vertical.
        let h = |ix: usize, iy: usize| 2 * (iy * self.nx + ix);
        let v = |ix: usize, iy: usize| 2 * (iy * self.nx + ix) + 1;
        let point = |edge: usize| -> P {
            let node = edge / 2;
            let (ix, iy) = (node % self.nx, node / self.nx);
            let a = self.at(ix, iy);
            let (b, dx, dy) = if edge % 2 == 0 {
                (self.at(ix + 1, iy), 1.0, 0.0)
            } else {
                (self.at(ix, iy + 1), 0.0, 1.0)
            };
            let t = ((level - a) / (b - a)).clamp(0.0, 1.0) as f64;
            [
                self.x0 + (ix as f64 + dx * t) * self.cell,
                self.y0 + (iy as f64 + dy * t) * self.cell,
            ]
        };
        let mut next: HashMap<usize, usize> = HashMap::new();
        for iy in 0..self.ny - 1 {
            for ix in 0..self.nx - 1 {
                let (a, b, c, d) = (
                    self.at(ix, iy) >= level,
                    self.at(ix + 1, iy) >= level,
                    self.at(ix + 1, iy + 1) >= level,
                    self.at(ix, iy + 1) >= level,
                );
                let (bottom, right, top, left) = (h(ix, iy), v(ix + 1, iy), h(ix, iy + 1), v(ix, iy));
                let case = a as u8 | (b as u8) << 1 | (c as u8) << 2 | (d as u8) << 3;
                // Each segment keeps the higher values on its right.
                let mut seg = |from: usize, to: usize| {
                    next.insert(from, to);
                };
                match case {
                    1 => seg(left, bottom),
                    2 => seg(bottom, right),
                    3 => seg(left, right),
                    4 => seg(right, top),
                    6 => seg(bottom, top),
                    7 => seg(left, top),
                    8 => seg(top, left),
                    9 => seg(top, bottom),
                    11 => seg(top, right),
                    12 => seg(right, left),
                    13 => seg(right, bottom),
                    14 => seg(bottom, left),
                    5 | 10 => {
                        let centre = (self.at(ix, iy)
                            + self.at(ix + 1, iy)
                            + self.at(ix + 1, iy + 1)
                            + self.at(ix, iy + 1))
                            / 4.0
                            >= level;
                        match (case, centre) {
                            (5, true) => {
                                seg(left, top);
                                seg(right, bottom);
                            }
                            (5, false) => {
                                seg(left, bottom);
                                seg(right, top);
                            }
                            (_, true) => {
                                seg(bottom, right);
                                seg(top, left);
                            }
                            (_, false) => {
                                seg(bottom, left);
                                seg(top, right);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut loops = Vec::new();
        let mut starts: Vec<usize> = next.keys().copied().collect();
        starts.sort_unstable();
        for start in starts {
            if !next.contains_key(&start) {
                continue;
            }
            let mut pts = Vec::new();
            let mut at = start;
            let mut closed = false;
            while let Some(to) = next.remove(&at) {
                pts.push(point(at));
                at = to;
                if at == start {
                    closed = true;
                    break;
                }
            }
            if closed && pts.len() >= 3 {
                loops.push(simplify_loop(&pts, self.cell * 0.02));
            }
        }
        loops
    }
}

/// Removes points that lie within `tol` of the straight line between their neighbours.
pub fn simplify_loop(pts: &[P], tol: f64) -> Vec<P> {
    let tol2 = tol * tol;
    let mut out: Vec<P> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let next = pts[(i + 1) % pts.len()];
        if let Some(prev) = out.last() {
            if dist2_seg(*p, *prev, next) < tol2 {
                continue;
            }
        }
        out.push(*p);
    }
    if out.len() >= 3 {
        out
    } else {
        pts.to_vec()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoolOp {
    Weld,
    Subtract,
    Intersect,
}

/// Combines the shapes: `Subtract` removes `clip` from `subject`.
pub fn boolean(subject: &[Path], clip: &[Path], op: BoolOp) -> Vec<Path> {
    let loops = |paths: &[Path]| -> Vec<Vec<P>> {
        paths.iter().filter(|p| p.closed && p.pts.len() >= 3).map(|p| p.pts.clone()).collect()
    };
    let rule = match op {
        BoolOp::Weld => OverlayRule::Union,
        BoolOp::Subtract => OverlayRule::Difference,
        BoolOp::Intersect => OverlayRule::Intersect,
    };
    let shapes = loops(subject).overlay(&loops(clip), rule, FillRule::EvenOdd);
    shapes.into_iter().flatten().map(Path::closed).collect()
}

/// Letter outlines of `text` in Burin's own font; `height` is the capital height, `origin` the start of the baseline.
pub fn text_paths(text: &str, height: f64, origin: P) -> Vec<Path> {
    crate::fonts::text_paths(text, height, origin, &crate::fonts::TextStyle::default())
}

/// Reads LINE, ARC, CIRCLE, LWPOLYLINE and POLYLINE entities. Touching lines stay separate paths.
/// The points between two ends of a bowed side. A bulge is the tangent of a quarter of the
/// angle the side turns through, positive turning anticlockwise.
pub fn bow_points(a: P, b: P, bulge: f64) -> Vec<P> {
    let chord = (b[0] - a[0]).hypot(b[1] - a[1]);
    if bulge.abs() < 1e-9 || chord < 1e-12 {
        return Vec::new();
    }
    let turn = 4.0 * bulge.atan();
    let radius = chord / (2.0 * (turn / 2.0).sin()).abs();
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    // From the middle of the chord to the centre, to the left going from a to b when it bows right.
    let back = radius * (turn / 2.0).cos() * if bulge > 0.0 { 1.0 } else { -1.0 };
    let (nx, ny) = (-(b[1] - a[1]) / chord, (b[0] - a[0]) / chord);
    let c = [mid[0] + nx * back, mid[1] + ny * back];
    let start = (a[1] - c[1]).atan2(a[0] - c[0]);
    let steps = ((turn.abs() / (TAU / 128.0)).ceil() as usize).max(2);
    (1..steps)
        .map(|s| {
            let t = start + turn * s as f64 / steps as f64;
            [c[0] + radius * t.cos(), c[1] + radius * t.sin()]
        })
        .collect()
}

/// Turns the bowed sides of a polyline into curves of small straight steps.
fn unbow(pts: &[P], bulges: &[f64], closed: bool) -> Vec<P> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(pts[i]);
        if i + 1 == n && !closed {
            break;
        }
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        out.extend(bow_points(pts[i], pts[(i + 1) % n], bulge));
    }
    out
}

/// Reads a DXF spline: its control points when it has them, otherwise the points it passes through.
fn dxf_spline(fields: &[(i32, &str)]) -> Option<Path> {
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    let (mut flags, mut degree) = (0i64, 3usize);
    let (mut knots, mut weights, mut control, mut fit) = (Vec::new(), Vec::new(), Vec::<P>::new(), Vec::<P>::new());
    let mut pending = None;
    for (code, value) in fields {
        match code {
            70 => flags = value.parse().unwrap_or(0),
            71 => degree = value.parse().unwrap_or(3),
            40 => knots.push(num(value)),
            41 => weights.push(num(value)),
            10 | 11 => pending = Some(num(value)),
            20 => control.extend(pending.take().map(|x| [x, num(value)])),
            21 => fit.extend(pending.take().map(|x| [x, num(value)])),
            _ => {}
        }
    }
    let mut pts = if control.len() > degree && knots.len() == control.len() + degree + 1 {
        spline_points(&control, &weights, &knots, degree)
    } else if fit.len() >= 2 {
        smooth_curve(&fit)
    } else {
        return None;
    };
    let first = pts[0];
    let meets = pts.last().is_some_and(|l| (l[0] - first[0]).hypot(l[1] - first[1]) < 1e-6);
    if meets && pts.len() > 2 {
        pts.pop();
    }
    let closed = (flags & 1 == 1 || meets) && pts.len() >= 3;
    Some(Path { pts, closed })
}

/// Follows a spline along its knots, in steps fine enough for its number of control points.
fn spline_points(control: &[P], weights: &[f64], knots: &[f64], degree: usize) -> Vec<P> {
    let weight = |i: usize| weights.get(i).copied().filter(|w| *w > 0.0).unwrap_or(1.0);
    let (from, to) = (knots[degree], knots[control.len()]);
    let count = (control.len() * 8).clamp(16, 1024);
    let mut out = Vec::with_capacity(count + 1);
    for s in 0..=count {
        let t = from + (to - from) * s as f64 / count as f64;
        let mut span = degree;
        while span + 1 < control.len() && knots[span + 1] <= t {
            span += 1;
        }
        // De Boor's method, carrying each point's weight along.
        let mut d: Vec<[f64; 3]> = (0..=degree)
            .map(|j| {
                let i = span + j - degree;
                let w = weight(i);
                [control[i][0] * w, control[i][1] * w, w]
            })
            .collect();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = span + j - degree;
                let under = knots[i + degree - r + 1] - knots[i];
                let a = if under.abs() < 1e-12 { 0.0 } else { (t - knots[i]) / under };
                d[j] = std::array::from_fn(|k| (1.0 - a) * d[j - 1][k] + a * d[j][k]);
            }
        }
        let [x, y, w] = d[degree];
        out.push([x / w, y / w]);
    }
    out
}

pub fn dxf_read(text: &str) -> Vec<Path> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(i32, &str)> = lines
        .chunks_exact(2)
        .filter_map(|c| c[0].parse::<i32>().ok().map(|code| (code, c[1])))
        .collect();
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    let mut out = Vec::new();
    let mut in_entities = false;
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        i += 1;
        if code == 2 && value == "ENTITIES" {
            in_entities = true;
        }
        if code != 0 || !in_entities {
            continue;
        }
        if value == "ENDSEC" {
            in_entities = false;
            continue;
        }
        let kind = value;
        if !matches!(kind, "LINE" | "ARC" | "CIRCLE" | "LWPOLYLINE" | "POLYLINE" | "SPLINE") {
            continue;
        }
        if kind == "SPLINE" {
            let start = i;
            while i < pairs.len() && pairs[i].0 != 0 {
                i += 1;
            }
            if let Some(p) = dxf_spline(&pairs[start..i]) {
                out.push(p);
            }
            continue;
        }
        let mut fields: Vec<(i32, f64)> = Vec::new();
        let mut poly: Vec<P> = Vec::new();
        // How much each side of a polyline bows out, by the point it starts from.
        let mut bulges: Vec<f64> = Vec::new();
        let mut flags = 0i64;
        let mut pending_x = None;
        while i < pairs.len() {
            let (c, v) = pairs[i];
            if c == 0 && !(kind == "POLYLINE" && v == "VERTEX") {
                break;
            }
            i += 1;
            if c == 70 {
                flags = v.parse().unwrap_or(0);
            }
            if kind == "LWPOLYLINE" || kind == "POLYLINE" {
                if c == 10 {
                    pending_x = Some(num(v));
                } else if c == 20 {
                    if let Some(x) = pending_x.take() {
                        poly.push([x, num(v)]);
                        bulges.resize(poly.len(), 0.0);
                    }
                } else if c == 42 && !poly.is_empty() {
                    let last = poly.len() - 1;
                    bulges[last] = num(v);
                }
            } else {
                fields.push((c, num(v)));
            }
        }
        if kind == "POLYLINE" {
            // The POLYLINE header's own 10/20 is a dummy origin, not a vertex.
            if poly.len() > 1 {
                poly.remove(0);
                bulges.remove(0);
            }
            while i < pairs.len() && pairs[i].1 != "ENTITIES" && pairs[i].0 == 0 && pairs[i].1 == "SEQEND" {
                i += 1;
            }
        }
        let get = |c: i32| fields.iter().find(|f| f.0 == c).map_or(0.0, |f| f.1);
        match kind {
            "LINE" => out.push(Path::open(vec![[get(10), get(20)], [get(11), get(21)]])),
            "CIRCLE" => out.push(Path::closed(circle([get(10), get(20)], get(40)))),
            "ARC" => {
                let sweep = (get(51) - get(50)).rem_euclid(360.0);
                out.push(Path::open(arc([get(10), get(20)], get(40), get(50), sweep)));
            }
            _ if poly.len() >= 2 => {
                let closed = flags & 1 == 1;
                out.push(Path { pts: unbow(&poly, &bulges, closed), closed });
            }
            _ => {}
        }
    }
    out
}

pub fn dxf_write(paths: &[Path]) -> String {
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for p in paths {
        s += &format!("0\nLWPOLYLINE\n8\n0\n90\n{}\n70\n{}\n", p.pts.len(), p.closed as i32);
        for q in &p.pts {
            s += &format!("10\n{:.5}\n20\n{:.5}\n", q[0], q[1]);
        }
    }
    s + "0\nENDSEC\n0\nEOF\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_of_a_circle_has_the_right_radius() {
        let c = [Path::closed(circle([5.0, 5.0], 1.2))];
        let f = Field::build(&c, 0.5, 300).unwrap();
        let out = f.contours(-0.125);
        assert_eq!(out.len(), 1);
        for p in &out[0] {
            let r = ((p[0] - 5.0).powi(2) + (p[1] - 5.0).powi(2)).sqrt();
            assert!((r - 1.325).abs() < 0.01, "radius {r}");
        }
    }

    #[test]
    fn inside_contour_has_the_right_size() {
        let c = [Path::closed(rect([0.0, 0.0], 4.0, 4.0, 0.0))];
        let f = Field::build(&c, 0.2, 200).unwrap();
        let loops = f.contours(0.5);
        assert_eq!(loops.len(), 1);
        assert!((signed_area(&loops[0]).abs() - 9.0).abs() < 0.05);
    }

    #[test]
    fn holes_are_respected() {
        let shape = vec![
            Path::closed(rect([0.0, 0.0], 6.0, 6.0, 0.0)),
            Path::closed(rect([0.0, 0.0], 2.0, 2.0, 0.0)),
        ];
        assert!(!inside_even_odd(&shape, [0.0, 0.0]));
        assert!(inside_even_odd(&shape, [2.0, 0.0]));
        let f = Field::build(&shape, 0.2, 200).unwrap();
        assert!(f.sample(0.0, 0.0) < 0.0);
        assert!(f.sample(2.0, 0.0) > 0.0);
    }

    #[test]
    fn weld_and_subtract_give_the_right_area() {
        let a = [Path::closed(rect([0.0, 0.0], 2.0, 2.0, 0.0))];
        let b = [Path::closed(rect([1.0, 0.0], 2.0, 2.0, 0.0))];
        let area = |p: Vec<Path>| p.iter().map(|q| signed_area(&q.pts).abs()).sum::<f64>();
        assert!((area(boolean(&a, &b, BoolOp::Weld)) - 6.0).abs() < 1e-3);
        assert!((area(boolean(&a, &b, BoolOp::Subtract)) - 2.0).abs() < 1e-3);
        assert!((area(boolean(&a, &b, BoolOp::Intersect)) - 2.0).abs() < 1e-3);
    }

    #[test]
    fn text_makes_letter_outlines_with_the_asked_height() {
        let t = text_paths("H", 1.0, [0.0, 0.0]);
        assert_eq!(t.len(), 1);
        let (lo, hi) = bbox(&t).unwrap();
        assert!((hi[1] - lo[1] - 1.0).abs() < 0.01, "height {}", hi[1] - lo[1]);
        assert!(lo[1].abs() < 0.01, "sits on the baseline, bottom {}", lo[1]);
        // The letter O has an outer outline and a hole.
        assert_eq!(text_paths("O", 1.0, [0.0, 0.0]).len(), 2);
    }

    #[test]
    fn dxf_round_trip_keeps_shapes() {
        let shapes = vec![
            Path::closed(rect([1.0, 1.0], 2.0, 2.0, 0.0)),
            Path::open(vec![[0.0, 0.0], [3.0, 4.0]]),
        ];
        assert_eq!(dxf_read(&dxf_write(&shapes)).len(), 2);
        let read = dxf_read(&dxf_write(&shapes));
        assert!(read[0].closed && !read[1].closed);
        assert_eq!(read[1].pts[1], [3.0, 4.0]);
        let circle = "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1\n20\n2\n40\n3\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
        let read = dxf_read(circle);
        assert_eq!(read.len(), 2);
        assert!(read[0].closed);
    }

    /// A DXF file with these entities, each given as its lines of code and value.
    fn dxf(entities: &str) -> String {
        format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    #[test]
    fn a_spline_is_followed_by_its_control_points() {
        // A clamped cubic from (0,0) to (3,0): one Bezier curve, whose middle is at (1.5, 1.5).
        let text = dxf("0\nSPLINE\n70\n8\n71\n3\n72\n8\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n10\n0\n20\n0\n10\n0\n20\n2\n10\n3\n20\n2\n10\n3\n20\n0\n");
        let paths = dxf_read(&text);
        assert_eq!(paths.len(), 1);
        let p = &paths[0];
        assert!(!p.closed);
        assert_eq!(p.pts[0], [0.0, 0.0]);
        assert!((p.pts[p.pts.len() - 1][0] - 3.0).abs() < 1e-9);
        let top = p.pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max);
        assert!((top - 1.5).abs() < 0.01, "{top}");
    }

    #[test]
    fn a_rational_spline_draws_a_true_circle() {
        // The classic circle from nine weighted points on a square.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let pts = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0), (-1.0, 1.0, s), (-1.0, 0.0, 1.0), (-1.0, -1.0, s), (0.0, -1.0, 1.0), (1.0, -1.0, s), (1.0, 0.0, 1.0)];
        let mut e = String::from("0\nSPLINE\n70\n13\n71\n2\n72\n12\n73\n9\n");
        for k in [0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0] {
            e += &format!("40\n{k}\n");
        }
        for (x, y, w) in pts {
            e += &format!("41\n{w}\n10\n{x}\n20\n{y}\n");
        }
        let paths = dxf_read(&dxf(&e));
        assert!(paths[0].closed);
        for q in &paths[0].pts {
            assert!((q[0].hypot(q[1]) - 1.0).abs() < 1e-6, "{q:?}");
        }
    }

    #[test]
    fn a_spline_with_only_points_to_pass_through_still_comes_in() {
        let text = dxf("0\nSPLINE\n71\n3\n74\n3\n11\n0\n21\n0\n11\n1\n21\n1\n11\n2\n21\n0\n");
        let p = &dxf_read(&text)[0];
        assert!(p.pts.len() > 3);
        assert!(p.pts.iter().any(|q| (q[0] - 1.0).abs() < 0.02 && (q[1] - 1.0).abs() < 0.02));
    }

    #[test]
    fn a_bowed_polyline_side_becomes_a_curve_the_right_way_round() {
        // Bulge 1 is a half circle; positive turns counter-clockwise, so it passes under the chord.
        let text = dxf("0\nLWPOLYLINE\n90\n2\n70\n0\n10\n0\n20\n0\n42\n1\n10\n2\n20\n0\n");
        let p = &dxf_read(&text)[0];
        assert!(p.pts.len() > 8);
        assert_eq!(p.pts[0], [0.0, 0.0]);
        assert_eq!(p.pts[p.pts.len() - 1], [2.0, 0.0]);
        let low = p.pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min);
        assert!((low + 1.0).abs() < 0.01, "{low}");
        for q in &p.pts {
            assert!(((q[0] - 1.0).hypot(q[1]) - 1.0).abs() < 1e-6, "{q:?}");
        }
        // Negative goes the other way, and a closed outline bows on its closing side too.
        let up = dxf("0\nLWPOLYLINE\n70\n1\n10\n0\n20\n0\n10\n2\n20\n0\n42\n-1\n");
        let p = &dxf_read(&up)[0];
        assert!(p.closed);
        let high = p.pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max);
        assert!(high.abs() < 1e-9, "the side from the second point back to the first bows down: {high}");
    }

    #[test]
    fn straight_polylines_are_left_alone() {
        let text = dxf("0\nLWPOLYLINE\n70\n1\n10\n0\n20\n0\n10\n2\n20\n0\n10\n2\n20\n2\n");
        assert_eq!(dxf_read(&text)[0].pts.len(), 3);
    }
}
