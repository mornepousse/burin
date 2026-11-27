//! Smoothing shapes made of many short lines, such as a traced picture or a scanned outline
//! (Edit > Curve Fit Vectors): into circular arcs, smooth curves or straight lines kept within a
//! tolerance of the shape, sharp corners kept sharp if asked. Burin keeps shapes as points, so arcs
//! and curves come as points close enough along them.

use crate::app::*;
use crate::geom::{Path, P};
use crate::i18n::tr;
use egui::{Align2, RichText};

fn distance_to_segment(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// Douglas and Peucker: only the points further than `tol` from the line kept.
fn simplified(pts: &[P], tol: f64) -> Vec<P> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut spans = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        let far = (a + 1..b).map(|i| (i, distance_to_segment(pts[i], pts[a], pts[b]))).fold((0, 0.0), |m, x| if x.1 > m.1 { x } else { m });
        if far.1 > tol {
            keep[far.0] = true;
            spans.push((a, far.0));
            spans.push((far.0, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// A Catmull-Rom curve through the points, `per` points to each span, round a loop if `closed`.
fn spline(pts: &[P], closed: bool, per: usize) -> Vec<P> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let at = |i: isize| if closed { pts[i.rem_euclid(n as isize) as usize] } else { pts[i.clamp(0, n as isize - 1) as usize] };
    let spans = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(spans * per + 1);
    for i in 0..spans as isize {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for s in 0..per {
            let t = s as f64 / per as f64;
            let f = |k: usize| 0.5 * (2.0 * p1[k] + (p2[k] - p0[k]) * t + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t * t + (3.0 * p1[k] - p0[k] - 3.0 * p2[k] + p3[k]) * t * t * t);
            out.push([f(0), f(1)]);
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

/// What a fit makes of a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitKind {
    Arcs,
    Bezier,
    Lines,
}

/// The circle through three points, unless they're in a line.
fn circle3(a: P, b: P, c: P) -> Option<(P, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: P| p[0] * p[0] + p[1] * p[1];
    let x = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let y = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    let r = (a[0] - x).hypot(a[1] - y);
    (r.is_finite() && r < 1e7).then_some(([x, y], r))
}

/// Points along `pts` as circular arcs and straight lines, each run as long as it stays within
/// `tol` of all the points it stands for.
fn arcs_through(pts: &[P], tol: f64) -> Vec<P> {
    const LONGEST_RUN: usize = 600;
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    let mut s = 0;
    while s < n - 1 {
        let mut best = s + 1;
        let mut best_arc: Option<(P, f64, usize)> = None;
        let mut j = s + 2;
        while j < n && j - s <= LONGEST_RUN {
            let m = (s + j) / 2;
            let straight = pts[s..=j].iter().all(|p| distance_to_segment(*p, pts[s], pts[j]) <= tol);
            if straight {
                best = j;
                best_arc = None;
            } else if let Some((c, r)) = circle3(pts[s], pts[m], pts[j]) {
                // Every point near the circle, and none of the arc's span swept the wrong way round.
                if pts[s..=j].iter().all(|p| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() <= tol) {
                    best = j;
                    best_arc = Some((c, r, m));
                } else {
                    break;
                }
            } else {
                break;
            }
            j += 1;
        }
        match best_arc {
            Some((c, r, m)) => {
                let arc = crate::edit::arc_through(c, r, pts[s], pts[best], pts[m]);
                out.extend(arc.into_iter().skip(1));
                // The arc's end exactly where the shape's point is.
                if let Some(last) = out.last_mut() {
                    *last = pts[best];
                }
            }
            None => out.push(pts[best]),
        }
        s = best;
    }
    out
}

/// Where a line turns sharply: the indexes of points where its way changes by more than
/// `most_degrees`, judged over a stretch a little longer than the tolerance either side, so a
/// traced picture's tiny steps don't count.
fn sharp_corners(pts: &[P], closed: bool, tol: f64, most_degrees: f64) -> Vec<usize> {
    let n = pts.len();
    let reach = (tol * 3.0).max(1e-9);
    let step_back = |i: usize| -> Option<P> {
        let mut k = i;
        for _ in 0..n {
            k = if k == 0 { if closed { n - 1 } else { return None } } else { k - 1 };
            if (pts[k][0] - pts[i][0]).hypot(pts[k][1] - pts[i][1]) >= reach {
                return Some(pts[k]);
            }
        }
        None
    };
    let step_on = |i: usize| -> Option<P> {
        let mut k = i;
        for _ in 0..n {
            k = if k + 1 == n { if closed { 0 } else { return None } } else { k + 1 };
            if (pts[k][0] - pts[i][0]).hypot(pts[k][1] - pts[i][1]) >= reach {
                return Some(pts[k]);
            }
        }
        None
    };
    let mut out = Vec::new();
    for i in 0..n {
        let (Some(a), Some(b)) = (step_back(i), step_on(i)) else { continue };
        let c = pts[i];
        let (u, w) = ([c[0] - a[0], c[1] - a[1]], [b[0] - c[0], b[1] - c[1]]);
        let turn = (u[0] * w[1] - u[1] * w[0]).atan2(u[0] * w[0] + u[1] * w[1]).abs().to_degrees();
        if turn > most_degrees {
            out.push(i);
        }
    }
    // One point per corner: of neighbours all counted, the one turning most stands.
    out.dedup_by(|b, a| *b == *a + 1);
    out
}

/// The shape fitted the chosen way: `corners` (degrees) keeps turns sharper than that as corners.
pub fn fitted_with(path: &Path, tol: f64, kind: FitKind, corners: Option<f64>) -> Path {
    let mut pts = path.pts.clone();
    if path.closed && pts.len() > 3 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 3 {
        return path.clone();
    }
    // Runs between the sharp corners, each fitted on its own so the corners stay where they are.
    let mut cuts = corners.map_or_else(Vec::new, |a| sharp_corners(&pts, path.closed, tol, a));
    let runs: Vec<Vec<P>> = if path.closed {
        if cuts.is_empty() {
            cuts.push(0);
        }
        let n = pts.len();
        (0..cuts.len()).map(|k| {
            let (from, to) = (cuts[k], cuts[(k + 1) % cuts.len()]);
            let len = if to > from { to - from } else { to + n - from };
            (0..=len).map(|i| pts[(from + i) % n]).collect()
        })
        .collect()
    } else {
        let mut bounds = vec![0];
        bounds.extend(cuts.iter().copied().filter(|c| *c > 0 && *c < pts.len() - 1));
        bounds.push(pts.len() - 1);
        bounds.windows(2).map(|w| pts[w[0]..=w[1]].to_vec()).collect()
    };
    let mut out: Vec<P> = Vec::new();
    for run in runs {
        let done = match kind {
            FitKind::Lines => simplified(&run, tol),
            FitKind::Bezier => spline(&simplified(&run, tol), false, 8),
            FitKind::Arcs => arcs_through(&run, tol),
        };
        if out.is_empty() {
            out = done;
        } else {
            out.extend(done.into_iter().skip(1));
        }
    }
    if path.closed && out.len() > 3 && out.first() == out.last() {
        out.pop();
    }
    if path.closed && corners.is_none() && kind == FitKind::Bezier {
        // A loop with no corners comes round smooth, the way the plain fit makes it.
        return fitted(path, tol);
    }
    Path { pts: out, closed: path.closed }
}

/// The same shape, smooth, kept within about `tol` of where it was.
pub fn fitted(path: &Path, tol: f64) -> Path {
    let mut pts = path.pts.clone();
    if path.closed && pts.len() > 3 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 4 {
        return path.clone();
    }
    let kept = if path.closed {
        // Split the loop at its first point so both ends stay put, then join it up again.
        let mut open = pts.clone();
        open.push(pts[0]);
        let mut k = simplified(&open, tol);
        k.pop();
        k
    } else {
        simplified(&pts, tol)
    };
    let pts = spline(&kept, path.closed, 8);
    Path { pts, closed: path.closed }
}

impl App {
    /// The shapes the fit would make of the chosen ones, as the window's settings are now.
    pub(crate) fn curve_fit_results(&self) -> Vec<(u64, Vec<Path>)> {
        let corners = self.curve_fit_corners.then_some(self.curve_fit_corner_angle);
        self.doc
            .vectors
            .iter()
            .filter(|v| self.selected.contains(&v.id))
            .map(|v| (v.id, v.paths.iter().map(|p| fitted_with(p, self.curve_fit_tolerance, self.curve_fit_kind, corners)).collect()))
            .collect()
    }

    pub(crate) fn curve_fit_window(&mut self, ctx: &egui::Context) {
        if !self.curve_fit_open {
            self.curve_fit_preview = None;
            return;
        }
        let mut open = true;
        let mut apply = false;
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Curve Fit Vectors")).id(egui::Id::unique("curve-fit")).open(&mut open).collapsible(false).resizable(false).default_width(360.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(tr("Fits the selected shapes with arcs, smooth curves or straight lines, kept within the tolerance. The result shows on the sheet before you apply it.")).color(crate::theme::MUTED()));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Fit with")).strong());
            for (kind, label) in [(FitKind::Arcs, tr("Circular arcs")), (FitKind::Bezier, tr("Smooth curves")), (FitKind::Lines, tr("Straight lines"))] {
                ui.radio_value(&mut self.curve_fit_kind, kind, label);
            }
            ui.horizontal(|ui| {
                ui.label(tr("Tolerance"));
                if crate::ui::number(ui, &mut self.curve_fit_tolerance, 60.0) {
                    self.curve_fit_tolerance = self.curve_fit_tolerance.clamp(0.0001, 100.0);
                }
                ui.label(unit);
            });
            ui.checkbox(&mut self.curve_fit_corners, tr("Keep sharp corners"));
            ui.add_enabled_ui(self.curve_fit_corners, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Turning more than"));
                    if crate::ui::number(ui, &mut self.curve_fit_corner_angle, 46.0) {
                        self.curve_fit_corner_angle = self.curve_fit_corner_angle.clamp(1.0, 179.0);
                    }
                    ui.label(tr("degrees"));
                });
            });
            ui.checkbox(&mut self.curve_fit_replace, tr("Replace the selected shapes"));
            if ui.add_enabled(!self.selected.is_empty(), egui::Button::new(tr("Apply"))).clicked() {
                apply = true;
            }
        });
        // What it would make, kept while nothing changes.
        let key = format!("{:?}{}{:?}{}{}", self.selected, self.curve_fit_tolerance, self.curve_fit_kind, self.curve_fit_corners, self.curve_fit_corner_angle);
        if self.curve_fit_preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let shown = self.curve_fit_results().into_iter().flat_map(|(_, p)| p).collect();
            self.curve_fit_preview = Some((key, shown));
        }
        if apply {
            self.checkpoint();
            let results = self.curve_fit_results();
            let before: usize = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).map(|v| v.paths.iter().map(|p| p.pts.len()).sum::<usize>()).sum();
            let after: usize = results.iter().map(|(_, p)| p.iter().map(|q| q.pts.len()).sum::<usize>()).sum();
            if self.curve_fit_replace {
                for (id, paths) in results {
                    if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == id) {
                        v.paths = paths;
                        v.prim = Prim::Free;
                    }
                }
            } else {
                let mut made = Vec::new();
                for (id, paths) in results {
                    let layer = self.doc.vectors.iter().find(|v| v.id == id).map_or(self.doc.active_layer, |v| v.layer);
                    let new = self.doc.add_vector(paths, Prim::Free);
                    if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == new) {
                        v.layer = layer;
                    }
                    made.push(new);
                }
                self.selected = made;
            }
            self.curve_fit_preview = None;
            self.status = tf!("Fitted: {} points before, {} now.", before, after);
            self.note_changed_shapes();
        }
        self.curve_fit_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As the person asked: arcs, curves or lines, within the
    /// tolerance, sharp corners kept.
    #[test]
    fn arcs_lines_and_corners_as_asked() {
        // A circle of radius 10 drawn as 400 short pieces, a little rough.
        let pts: Vec<P> = (0..400).map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 400.0;
            let r = 10.0 + if i % 2 == 0 { 0.002 } else { -0.002 };
            [r * t.cos(), r * t.sin()]
        }).collect();
        let circle = Path::closed(pts);
        let arcs = fitted_with(&circle, 0.01, FitKind::Arcs, Some(60.0));
        assert!(arcs.closed && arcs.pts.len() < 200, "{}", arcs.pts.len());
        assert!(arcs.pts.iter().all(|p| (p[0].hypot(p[1]) - 10.0).abs() < 0.02));
        // A square with wobbly sides: its four corners stay where they are, sharp.
        let mut sq: Vec<P> = Vec::new();
        for (a, b) in [([0.0, 0.0], [10.0, 0.0]), ([10.0, 0.0], [10.0, 10.0]), ([10.0, 10.0], [0.0, 10.0]), ([0.0, 10.0], [0.0, 0.0])] {
            for k in 0..50 {
                let t = k as f64 / 50.0;
                let wob = if k % 2 == 0 { 0.003 } else { -0.003 };
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                sq.push([a[0] + dx * t - dy / 10.0 * wob, a[1] + dy * t + dx / 10.0 * wob]);
            }
        }
        let square = Path::closed(sq);
        for kind in [FitKind::Lines, FitKind::Arcs] {
            let out = fitted_with(&square, 0.01, kind, Some(60.0));
            for c in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
                assert!(out.pts.iter().any(|p| (p[0] - c[0]).hypot(p[1] - c[1]) < 0.01), "{kind:?}: corner {c:?} kept");
            }
            assert!(out.pts.len() <= 12, "{kind:?}: straight sides become single lines: {}", out.pts.len());
        }
    }

    #[test]
    fn a_jagged_line_comes_out_smooth_and_close() {
        // A sine drawn with steps, the way a traced picture comes in.
        let pts: Vec<P> = (0..200).map(|i| {
            let x = i as f64 * 0.05;
            [x, (x.sin() * 20.0).round() / 20.0]
        }).collect();
        let path = Path { pts: pts.clone(), closed: false };
        let out = fitted(&path, 0.05);
        assert_eq!(out.pts.first(), pts.first());
        assert_eq!(out.pts.last(), pts.last());
        for p in &out.pts {
            assert!((p[1] - p[0].sin()).abs() < 0.15, "{p:?}");
        }
        // Smooth: no sharp turn left between neighbouring pieces.
        for w in out.pts.windows(3) {
            let (a, b) = ([w[1][0] - w[0][0], w[1][1] - w[0][1]], [w[2][0] - w[1][0], w[2][1] - w[1][1]]);
            let turn = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]).abs();
            assert!(turn < 0.6, "{turn}");
        }
    }

    #[test]
    fn a_loop_stays_a_loop() {
        let pts: Vec<P> = (0..90).map(|i| {
            let a = i as f64 * 4.0_f64.to_radians();
            [10.0 * a.cos(), 10.0 * a.sin()]
        }).collect();
        let out = fitted(&Path { pts, closed: true }, 0.05);
        assert!(out.closed);
        for p in &out.pts {
            assert!((p[0].hypot(p[1]) - 10.0).abs() < 0.1, "{p:?}");
        }
        let few = Path { pts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], closed: true };
        assert_eq!(fitted(&few, 0.1), few, "too few points to change");
    }
}
