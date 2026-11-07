//! Smoothing shapes made of many short lines, such as a traced picture or a scanned outline
//! (Edit > Curve Fit Vectors). The original fits arcs and curves; Burin keeps shapes as points, so it
//! drops points within the tolerance and puts a smooth curve through the rest. Burin's own way.

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
    pub(crate) fn curve_fit_window(&mut self, ctx: &egui::Context) {
        if !self.curve_fit_open {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Curve Fit Vectors")).id(egui::Id::unique("curve-fit")).open(&mut open).collapsible(false).resizable(false).default_width(360.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(tr("Smooths the selected shapes: points closer than the tolerance to the line go, and a smooth curve passes through the rest.")).color(crate::theme::MUTED));
            ui.horizontal(|ui| {
                ui.label(tr("Tolerance"));
                ui.add(egui::DragValue::new(&mut self.curve_fit_tolerance).speed(0.01).range(0.0001..=100.0).suffix(format!(" {unit}")));
            });
            if ui.add_enabled(!self.selected.is_empty(), egui::Button::new(tr("Apply"))).clicked() {
                apply = true;
            }
        });
        if apply {
            self.checkpoint();
            let tol = self.curve_fit_tolerance;
            let (mut before, mut after) = (0, 0);
            for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
                before += v.paths.iter().map(|p| p.pts.len()).sum::<usize>();
                v.paths = v.paths.iter().map(|p| fitted(p, tol)).collect();
                after += v.paths.iter().map(|p| p.pts.len()).sum::<usize>();
                v.prim = Prim::Free;
            }
            self.status = tf!("Smoothed: {} points before, {} now.", before, after);
            self.note_changed_shapes();
        }
        self.curve_fit_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
