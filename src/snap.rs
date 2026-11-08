//! Smart snapping: the pointer catches on what is already drawn, and on guide lines.

use crate::app::*;
use crate::canvas::Drag;
use crate::geom::{self, Path, P};
use egui::{pos2, vec2, Color32, FontId, Painter, Rect, Shape, Stroke};
use serde::{Deserialize, Serialize};

/// What the pointer caught on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind {
    Point,
    Middle,
    Center,
    Crossing,
    Edge,
    Guide,
    Aligned,
}

impl SnapKind {
    fn label(self) -> &'static str {
        match self {
            SnapKind::Point => crate::i18n::tr("Point"),
            SnapKind::Middle => crate::i18n::tr("Midpoint"),
            SnapKind::Center => crate::i18n::tr("Center"),
            SnapKind::Crossing => crate::i18n::tr("Crossing"),
            SnapKind::Edge => crate::i18n::tr("On shape"),
            SnapKind::Guide => crate::i18n::tr("Guide"),
            SnapKind::Aligned => crate::i18n::tr("Aligned"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapHit {
    pub at: P,
    pub kind: SnapKind,
    /// The points it lines up with, across and up.
    pub along: [Option<P>; 2],
}

/// A straight line across the job to draw against.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub vertical: bool,
    pub at: f64,
}

/// A point on the circle `c`, `r` from it, at each quarter turn.
fn quarters(c: P, rx: f64, ry: f64) -> [P; 4] {
    [[c[0] + rx, c[1]], [c[0], c[1] + ry], [c[0] - rx, c[1]], [c[0], c[1] - ry]]
}

fn path_points(path: &Path, long_enough: f64, out: &mut Vec<(P, SnapKind)>) {
    if let Some((c, r)) = crate::bulk::as_circle(path) {
        out.push((c, SnapKind::Center));
        out.extend(quarters(c, r, r).map(|q| (q, SnapKind::Point)));
        return;
    }
    let n = path.pts.len();
    if n < 2 {
        return;
    }
    // Curves are drawn as many small sides: only real corners count as points.
    for i in 0..n {
        let ends = !path.closed && (i == 0 || i + 1 == n);
        if !ends {
            let (a, p, b) = (path.pts[(i + n - 1) % n], path.pts[i], path.pts[(i + 1) % n]);
            let (ux, uy, vx, vy) = (p[0] - a[0], p[1] - a[1], b[0] - p[0], b[1] - p[1]);
            let turn = (ux * vy - uy * vx).atan2(ux * vx + uy * vy).abs();
            if turn < 20f64.to_radians() {
                continue;
            }
        }
        out.push((path.pts[i], SnapKind::Point));
    }
    for (a, b) in path.segments() {
        if (a[0] - b[0]).hypot(a[1] - b[1]) >= long_enough {
            out.push(([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], SnapKind::Middle));
        }
    }
}

/// The spots of a shape worth catching on.
fn shape_points(v: &Vector, long_enough: f64) -> Vec<(P, SnapKind)> {
    let mut out = Vec::new();
    match &v.prim {
        Prim::Text { at, .. } => {
            out.push((*at, SnapKind::Point));
            return out;
        }
        Prim::Ellipse { c, rx, ry } => {
            out.push((*c, SnapKind::Center));
            out.extend(quarters(*c, *rx, *ry).map(|q| (q, SnapKind::Point)));
            return out;
        }
        Prim::Rect { c, .. } | Prim::Polygon { c, .. } | Prim::Star { c, .. } | Prim::Spiral { c, .. } | Prim::Arc { c, .. } => {
            out.push((*c, SnapKind::Center));
        }
        _ => {}
    }
    for p in &v.paths {
        path_points(p, long_enough, &mut out);
    }
    out
}

/// Where two segments cross, if they do.
fn crossing(a: (P, P), b: (P, P)) -> Option<P> {
    let (r, s) = ([a.1[0] - a.0[0], a.1[1] - a.0[1]], [b.1[0] - b.0[0], b.1[1] - b.0[1]]);
    let under = r[0] * s[1] - r[1] * s[0];
    if under.abs() < 1e-12 {
        return None;
    }
    let (qx, qy) = (b.0[0] - a.0[0], b.0[1] - a.0[1]);
    let t = (qx * s[1] - qy * s[0]) / under;
    let u = (qx * r[1] - qy * r[0]) / under;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| [a.0[0] + r[0] * t, a.0[1] + r[1] * t])
}

fn nearest_on(p: P, a: P, b: P) -> P {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
    [a[0] + dx * t, a[1] + dy * t]
}

impl App {
    /// Shapes being moved or reshaped can't catch on themselves.
    fn snap_skips(&self) -> Vec<u64> {
        match &self.drag {
            Drag::Move { .. } => self.moving_ids(),
            Drag::Scale { .. } | Drag::Rotate { .. } | Drag::RotateCentre => self.selected.clone(),
            Drag::Node { id, .. } => vec![*id],
            Drag::Sketch { elem, .. } => vec![elem.vector_id()],
            _ => Vec::new(),
        }
    }

    /// What the pointer at `w` catches on, nearest kind first.
    pub fn smart_snap(&self, w: P) -> Option<SnapHit> {
        if self.snap_held_off {
            return None;
        }
        let px = 1.0 / self.scale as f64;
        let (reach, tight) = (8.0 * px, 5.0 * px);
        let mut guide: [Option<f64>; 2] = [None, None];
        if self.show_guides {
            for g in &self.doc.guides {
                let (axis, value) = if g.vertical { (0, w[0]) } else { (1, w[1]) };
                if (value - g.at).abs() < tight && guide[axis].is_none_or(|old| (value - g.at).abs() < (value - old).abs()) {
                    guide[axis] = Some(g.at);
                }
            }
        }
        if !self.snap_shapes {
            return (guide[0].is_some() || guide[1].is_some()).then(|| SnapHit {
                at: [guide[0].unwrap_or(w[0]), guide[1].unwrap_or(w[1])],
                kind: SnapKind::Guide,
                along: [None, None],
            });
        }
        let skips = self.snap_skips();
        let shapes = || self.doc.vectors.iter().filter(|v| !skips.contains(&v.id) && self.doc.shows(v));
        let long_enough = 16.0 * px;
        // The sheet itself has points worth catching.
        let (jw, jh) = (self.doc.job.width, self.doc.job.height);
        let mut keys: Vec<(P, SnapKind)> = vec![([jw / 2.0, jh / 2.0], SnapKind::Center)];
        for p in [[0.0, 0.0], [jw, 0.0], [jw, jh], [0.0, jh]] {
            keys.push((p, SnapKind::Point));
        }
        for p in [[jw / 2.0, 0.0], [jw, jh / 2.0], [jw / 2.0, jh], [0.0, jh / 2.0]] {
            keys.push((p, SnapKind::Middle));
        }
        for v in shapes() {
            keys.extend(shape_points(v, long_enough));
        }
        let gap = |p: P| (p[0] - w[0]).hypot(p[1] - w[1]);
        let best_of = |kinds: &[SnapKind], within: f64| {
            keys.iter()
                .filter(|(p, k)| kinds.contains(k) && gap(*p) < within)
                .min_by(|a, b| gap(a.0).total_cmp(&gap(b.0)))
                .map(|(p, k)| SnapHit { at: *p, kind: *k, along: [None, None] })
        };
        if let Some(hit) = best_of(&[SnapKind::Point, SnapKind::Center], reach) {
            return Some(hit);
        }
        // Sides close to the pointer, for crossings and for catching on the shape itself.
        let mut near: Vec<(P, P, u64)> = Vec::new();
        for v in shapes() {
            for (a, b) in v.paths.iter().flat_map(|p| p.segments()) {
                if geom::dist2_seg(w, a, b) < reach * reach {
                    near.push((a, b, v.id));
                    if near.len() >= 256 {
                        break;
                    }
                }
            }
        }
        let mut cross: Option<P> = None;
        for (i, a) in near.iter().enumerate() {
            for b in &near[i + 1..] {
                if a.2 != b.2 {
                    if let Some(c) = crossing((a.0, a.1), (b.0, b.1)) {
                        if gap(c) < reach && cross.is_none_or(|old| gap(c) < gap(old)) {
                            cross = Some(c);
                        }
                    }
                }
            }
        }
        if let Some(at) = cross {
            return Some(SnapHit { at, kind: SnapKind::Crossing, along: [None, None] });
        }
        if let Some(hit) = best_of(&[SnapKind::Middle], reach) {
            return Some(hit);
        }
        if guide[0].is_some() && guide[1].is_some() {
            return Some(SnapHit { at: [guide[0].unwrap(), guide[1].unwrap()], kind: SnapKind::Guide, along: [None, None] });
        }
        if let Some(on) = near
            .iter()
            .filter(|s| geom::dist2_seg(w, s.0, s.1) < tight * tight)
            .map(|s| nearest_on(w, s.0, s.1))
            .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
        {
            return Some(SnapHit { at: on, kind: SnapKind::Edge, along: [None, None] });
        }
        // Lining up with points elsewhere: the extensions that aren't drawn.
        let mut along: [Option<P>; 2] = [None, None];
        for axis in 0..2 {
            if guide[axis].is_some() {
                continue;
            }
            along[axis] = keys
                .iter()
                .filter(|(p, k)| matches!(k, SnapKind::Point | SnapKind::Center) && (p[axis] - w[axis]).abs() < tight)
                .min_by(|a, b| (a.0[axis] - w[axis]).abs().total_cmp(&(b.0[axis] - w[axis]).abs()))
                .map(|(p, _)| *p);
        }
        let aligned = along[0].is_some() || along[1].is_some();
        if aligned || guide[0].is_some() || guide[1].is_some() {
            let at = [guide[0].or(along[0].map(|p| p[0])).unwrap_or(w[0]), guide[1].or(along[1].map(|p| p[1])).unwrap_or(w[1])];
            let kind = if guide[0].is_some() || guide[1].is_some() { SnapKind::Guide } else { SnapKind::Aligned };
            return Some(SnapHit { at, kind, along });
        }
        None
    }

    /// Where the pointer ends up: on what it catches, otherwise on the grid if that is on.
    pub(crate) fn snapped(&self, w: P) -> P {
        let hit = self.smart_snap(w);
        self.snap_hit.set(hit);
        let mut p = w;
        let mut free = [true, true];
        if let Some(h) = hit {
            p = h.at;
            free = match h.kind {
                SnapKind::Aligned | SnapKind::Guide => [p[0] == w[0], p[1] == w[1]],
                _ => [false, false],
            };
        }
        if self.snap {
            // On the grid, which counts from the X/Y zero.
            let (step, zero) = (self.grid_steps().0, self.doc.job.origin());
            for axis in 0..2 {
                if free[axis] {
                    p[axis] = zero[axis] + ((p[axis] - zero[axis]) / step).round() * step;
                }
            }
        }
        p
    }

    /// Shows what the pointer is catching on, while drawing or dragging.
    pub(crate) fn paint_snap(&self, painter: &Painter) {
        let drawing = matches!(self.left, LeftForm::Draw(_));
        let dragging = matches!(self.drag, Drag::Move { .. } | Drag::Scale { .. } | Drag::RotateCentre | Drag::Node { .. } | Drag::Draw { .. } | Drag::Measure { .. } | Drag::Envelope(_) | Drag::Sketch { .. });
        if !(drawing || dragging) {
            return;
        }
        let Some(cursor) = self.cursor else { return };
        let Some(hit) = self.smart_snap(cursor) else { return };
        let colour = Color32::from_rgb(235, 120, 20);
        let stroke = Stroke::new(1.6, colour);
        let at = self.to_screen(hit.at);
        for from in hit.along.iter().flatten() {
            painter.extend(crate::canvas::dashed_within(&[self.to_screen(*from), at], Stroke::new(1.0, colour), 4.0, 3.0, painter.clip_rect()));
        }
        let r = 5.0;
        match hit.kind {
            SnapKind::Point => {
                painter.rect_stroke(Rect::from_center_size(at, vec2(r * 2.0, r * 2.0)), 0.0, stroke, egui::StrokeKind::Middle);
            }
            SnapKind::Center => {
                painter.circle_stroke(at, r, stroke);
                painter.circle_filled(at, 1.5, colour);
            }
            SnapKind::Middle => {
                let corners = vec![pos2(at.x, at.y - r), pos2(at.x + r, at.y + r), pos2(at.x - r, at.y + r)];
                painter.add(Shape::closed_line(corners, stroke));
            }
            SnapKind::Crossing => {
                painter.line_segment([at + vec2(-r, -r), at + vec2(r, r)], stroke);
                painter.line_segment([at + vec2(-r, r), at + vec2(r, -r)], stroke);
            }
            SnapKind::Edge => {
                let corners = vec![pos2(at.x, at.y - r), pos2(at.x + r, at.y), pos2(at.x, at.y + r), pos2(at.x - r, at.y)];
                painter.add(Shape::closed_line(corners, stroke));
            }
            SnapKind::Guide | SnapKind::Aligned => {
                painter.line_segment([at + vec2(-r, 0.0), at + vec2(r, 0.0)], stroke);
                painter.line_segment([at + vec2(0.0, -r), at + vec2(0.0, r)], stroke);
            }
        }
        painter.text(at + vec2(9.0, -14.0), egui::Align2::LEFT_BOTTOM, hit.kind.label(), FontId::proportional(11.5), colour);
    }

    /// Draws the guide lines across the view.
    pub(crate) fn paint_guides(&self, painter: &Painter, rect: Rect) {
        if !self.show_guides {
            return;
        }
        let stroke = Stroke::new(1.0, Color32::from_rgb(20, 170, 190));
        for g in &self.doc.guides {
            let ends = if g.vertical {
                // On a pixel's centre, so the thin line keeps its colour.
                let x = self.to_screen([g.at, 0.0]).x.round() + 0.5;
                [pos2(x, rect.top()), pos2(x, rect.bottom())]
            } else {
                let y = self.to_screen([0.0, g.at]).y.round() + 0.5;
                [pos2(rect.left(), y), pos2(rect.right(), y)]
            };
            painter.extend(crate::canvas::dashed_within(&ends, stroke, 8.0, 4.0, painter.clip_rect()));
        }
    }

    /// The guide close to a screen position.
    pub(crate) fn guide_at(&self, p: egui::Pos2) -> Option<usize> {
        if !self.show_guides || self.guides_locked {
            return None;
        }
        self.doc.guides.iter().position(|g| {
            if g.vertical {
                (self.to_screen([g.at, 0.0]).x - p.x).abs() < 5.0
            } else {
                (self.to_screen([0.0, g.at]).y - p.y).abs() < 5.0
            }
        })
    }

    /// Follows a guide being dragged; dropped outside the view, it goes away.
    pub(crate) fn drag_guide(&mut self, index: usize, p: Option<egui::Pos2>, down: bool, view: Rect) {
        let Some(p) = p else { return };
        let world = self.to_world(p);
        let (step, zero) = (self.grid_steps().0, self.doc.job.origin());
        let snap = self.snap;
        if let Some(g) = self.doc.guides.get_mut(index) {
            let (raw, from) = if g.vertical { (world[0], zero[0]) } else { (world[1], zero[1]) };
            g.at = if snap { from + ((raw - from) / step).round() * step } else { raw };
        }
        if !down {
            self.drag = Drag::None;
            if !view.contains(p) && index < self.doc.guides.len() {
                self.doc.guides.remove(index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::rect;

    fn app() -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        a.scale = 50.0;
        a.snap = false;
        a
    }

    fn near(p: P, x: f64, y: f64) {
        assert!((p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-6, "{p:?} is not {x},{y}");
    }

    fn square(a: &mut App, c: P) -> u64 {
        a.doc.add_vector(vec![Path::closed(rect(c, 2.0, 2.0, 0.0))], Prim::Free)
    }

    #[test]
    fn corners_and_midpoints_of_a_shape_catch_the_pointer() {
        let mut a = app();
        square(&mut a, [3.3, 3.3]);
        let corner = a.smart_snap([2.33, 2.33]).unwrap();
        assert_eq!(corner.kind, SnapKind::Point);
        near(corner.at, 2.3, 2.3);
        let mid = a.smart_snap([3.33, 2.33]).unwrap();
        assert_eq!(mid.kind, SnapKind::Middle);
        near(mid.at, 3.3, 2.3);
    }

    #[test]
    fn circles_catch_on_their_centre_and_quarters_not_their_many_small_sides() {
        let mut a = app();
        a.doc.add_vector(vec![Path::closed(geom::circle([3.3, 3.3], 1.0))], Prim::Free);
        let c = a.smart_snap([3.32, 3.28]).unwrap();
        assert_eq!(c.kind, SnapKind::Center);
        near(c.at, 3.3, 3.3);
        let q = a.smart_snap([4.32, 3.32]).unwrap();
        assert_eq!(q.kind, SnapKind::Point);
        near(q.at, 4.3, 3.3);
        // Part way round is only "on the shape".
        let (x, y) = (3.3 + 45f64.to_radians().cos(), 3.3 + 45f64.to_radians().sin());
        assert_eq!(a.smart_snap([x + 0.02, y + 0.02]).unwrap().kind, SnapKind::Edge);
    }

    #[test]
    fn where_two_shapes_cross_is_caught() {
        let mut a = app();
        a.doc.add_vector(vec![Path::open(vec![[1.3, 3.3], [7.7, 3.3]])], Prim::Free);
        a.doc.add_vector(vec![Path::open(vec![[3.7, 1.3], [3.7, 7.7]])], Prim::Free);
        let hit = a.smart_snap([3.75, 3.35]).unwrap();
        assert_eq!(hit.kind, SnapKind::Crossing);
        near(hit.at, 3.7, 3.3);
    }

    #[test]
    fn points_beat_midpoints_and_midpoints_beat_the_edge() {
        let mut a = app();
        a.doc.add_vector(vec![Path::open(vec![[1.3, 3.3], [1.5, 3.3], [7.7, 3.3]])], Prim::Free);
        // Close to the line's middle section: its midpoint, not the edge.
        assert_eq!(a.smart_snap([4.61, 3.31]).unwrap().kind, SnapKind::Middle);
        assert_eq!(a.smart_snap([6.0, 3.32]).unwrap().kind, SnapKind::Edge);
    }

    #[test]
    fn lining_up_with_a_far_point_is_offered_on_each_axis() {
        let mut a = app();
        square(&mut a, [3.3, 3.3]);
        // Straight above the square's top left corner, far from anything.
        let hit = a.smart_snap([2.32, 7.77]).unwrap();
        assert_eq!(hit.kind, SnapKind::Aligned);
        near(hit.at, 2.3, 7.77);
        assert!((hit.along[0].unwrap()[0] - 2.3).abs() < 1e-9, "{:?}", hit.along);
        // Level with one point and above another: both at once.
        square(&mut a, [7.7, 7.7]);
        let both = a.smart_snap([2.32, 6.72]).unwrap();
        near(both.at, 2.3, 6.7);
        assert!(both.along[0].is_some() && both.along[1].is_some());
    }

    #[test]
    fn the_sheet_has_points_too() {
        let a = app();
        let c = a.smart_snap([5.02, 4.98]).unwrap();
        assert_eq!(c.kind, SnapKind::Center);
        near(c.at, 5.0, 5.0);
        near(a.smart_snap([0.03, 0.03]).unwrap().at, 0.0, 0.0);
    }

    #[test]
    fn nothing_close_means_no_catch_and_the_reach_follows_the_zoom() {
        let mut a = app();
        square(&mut a, [3.3, 3.3]);
        assert!(a.smart_snap([8.13, 7.27]).is_none());
        a.scale = 500.0;
        assert_eq!(a.smart_snap([2.33, 2.33]).map(|h| h.kind), None, "30 thousandths is 15 pixels here");
    }

    #[test]
    fn hidden_layers_and_the_switch_turn_it_off() {
        let mut a = app();
        square(&mut a, [3.3, 3.3]);
        a.snap_held_off = true;
        assert!(a.smart_snap([2.33, 2.33]).is_none(), "holding Alt");
        a.snap_held_off = false;
        a.snap_shapes = false;
        assert!(a.smart_snap([2.33, 2.33]).is_none());
        a.snap_shapes = true;
        a.doc.layers[0].visible = false;
        assert_ne!(a.smart_snap([2.33, 2.33]).map(|h| h.at), Some([2.3, 2.3]));
    }

    #[test]
    fn a_shape_being_moved_does_not_catch_on_itself() {
        let mut a = app();
        let s = square(&mut a, [3.3, 3.3]);
        a.selected = vec![s];
        a.drag = Drag::Move { last: [0.0, 0.0], start: [0.0, 0.0] };
        assert!(a.smart_snap([2.33, 2.33]).is_none_or(|h| h.at != [2.3, 2.3]));
        a.drag = Drag::None;
        near(a.smart_snap([2.33, 2.33]).unwrap().at, 2.3, 2.3);
    }

    #[test]
    fn guides_catch_alone_and_where_they_cross() {
        let mut a = app();
        a.doc.guides.push(Guide { vertical: true, at: 2.3 });
        let one = a.smart_snap([2.33, 7.77]).unwrap();
        assert_eq!(one.kind, SnapKind::Guide);
        near(one.at, 2.3, 7.77);
        a.doc.guides.push(Guide { vertical: false, at: 7.7 });
        let both = a.smart_snap([2.33, 7.73]).unwrap();
        assert_eq!(both.kind, SnapKind::Guide);
        near(both.at, 2.3, 7.7);
        a.show_guides = false;
        assert!(a.smart_snap([2.33, 7.77]).is_none());
        a.show_guides = true;
        a.snap_shapes = false;
        assert_eq!(a.smart_snap([2.33, 7.77]).unwrap().kind, SnapKind::Guide, "guides work on their own");
    }

    #[test]
    fn the_grid_only_decides_what_nothing_else_has() {
        let mut a = app();
        a.snap = true;
        let step = a.grid_steps().0;
        a.doc.guides.push(Guide { vertical: true, at: 2.3 });
        let p = a.snapped([2.33, 7.77]);
        assert!((p[0] - 2.3).abs() < 1e-9, "the guide decides X: {p:?}");
        assert!(((p[1] / step).round() * step - p[1]).abs() < 1e-9, "the grid decides Y: {p:?}");
        let free = a.snapped([7.13, 7.19]);
        assert!(((free[0] / step).round() * step - free[0]).abs() < 1e-9);
    }

    #[test]
    fn guides_follow_a_change_of_units_and_saving() {
        let mut a = app();
        a.doc.guides.push(Guide { vertical: false, at: 2.0 });
        let back: Doc = serde_json::from_str(&serde_json::to_string(&a.doc).unwrap()).unwrap();
        assert_eq!(back.guides, a.doc.guides);
        a.doc.set_units(Units::Mm);
        assert!((a.doc.guides[0].at - 50.8).abs() < 1e-9);
    }
}
