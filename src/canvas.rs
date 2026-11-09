//! The 2D view: rulers, material sheet, shapes and the mouse tools.

use crate::app::{App, DrawKind, LeftForm, Prim, Tool2, Xform};
use crate::theme;
use crate::geom::{self, Path, P};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};

pub const MAGENTA: Color32 = crate::theme::PICK;

/// A press-and-sweep with the Trim tool: everything it passes over is trimmed, as one undo.
#[derive(Clone, Debug)]
pub struct TrimStroke {
    pub last: Pos2,
    pub undo_mark: usize,
    pub trail: Vec<Pos2>,
    pub trimmed: usize,
}

#[derive(Clone, Debug, Default)]
pub enum Drag {
    #[default]
    None,
    Box(Pos2),
    Move { last: P, start: P },
    /// `corner` by which handle it is, as the original tells corners by their type (6 to 9, `0x140a72f00`).
    /// `out`: how far the handle is drawn from its own place, taken off the pointer while dragging.
    Scale { anchor: P, middle: P, start: P, corner: bool, out: Vec2, orig: Vec<(u64, Vec<Path>)> },
    Rotate { centre: P, start: P, orig: Vec<(u64, Vec<Path>)> },
    RotateCentre,
    /// `start`: where the point was when the drag began, for values typed during it.
    Node { id: u64, path: usize, index: usize, start: P },
    Draw { start: P },
    Measure { start: P },
    Zoom(Pos2),
    DimLabel { index: usize, last: Pos2 },
    Envelope(usize),
    Guide(usize),
    Sketch { elem: crate::constraints::Elem, last: P },
}

/// A corner's new place when the shape keeps its proportions: the shorter way wins (`0x140a70f40`).
pub(crate) fn keep_proportions(start: P, now: P, anchor: P) -> P {
    let (x0, y0) = (start[0] - anchor[0], start[1] - anchor[1]);
    let aspect = (x0 / y0).abs();
    let nudge = |d: f64| if d == 0.0 { 0.001 } else { d };
    let (dx, dy) = (nudge(now[0] - anchor[0]), nudge(now[1] - anchor[1]));
    let (mut w, mut h) = (dx.abs(), dy.abs());
    if h <= w {
        w = h * aspect;
    } else {
        h = w / aspect;
    }
    [anchor[0] + w.copysign(dx), anchor[1] + h.copysign(dy)]
}

/// A dashed line, drawn only where it crosses `clip` (with a little room). A shape scaled up hugely, or with
/// points that aren't numbers, would otherwise ask for billions of dashes and stop the program.
pub(crate) fn dashed_within(points: &[Pos2], stroke: Stroke, dash: f32, gap: f32, clip: Rect) -> Vec<Shape> {
    let room = clip.expand(dash + gap + 4.0);
    let mut out = Vec::new();
    let mut run: Vec<Pos2> = Vec::new();
    let flush = |run: &mut Vec<Pos2>, out: &mut Vec<Shape>| {
        if run.len() >= 2 {
            out.extend(Shape::dashed_line(run, stroke, dash, gap));
        }
        run.clear();
    };
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
            flush(&mut run, &mut out);
            continue;
        }
        match clip_segment(a, b, room) {
            Some((p, q)) => {
                if run.last().is_none_or(|l| (*l - p).length() > 0.01) {
                    flush(&mut run, &mut out);
                    run.push(p);
                }
                run.push(q);
                if q != b {
                    flush(&mut run, &mut out);
                }
            }
            None => flush(&mut run, &mut out),
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The part of a segment inside a rectangle (Liang-Barsky), if any. Worked out in double precision, as a
/// segment can run a long way off screen.
fn clip_segment(a: Pos2, b: Pos2, r: Rect) -> Option<(Pos2, Pos2)> {
    let (ax, ay, dx, dy) = (a.x as f64, a.y as f64, b.x as f64 - a.x as f64, b.y as f64 - a.y as f64);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, ax - r.min.x as f64), (dx, r.max.x as f64 - ax), (-dy, ay - r.min.y as f64), (dy, r.max.y as f64 - ay)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    let at = |t: f64| pos2((ax + dx * t) as f32, (ay + dy * t) as f32);
    Some((at(t0), if t1 >= 1.0 { b } else { at(t1) }))
}

/// How far a point has gone round the centre, in degrees. With `steps`, to the nearest 15.
pub(crate) fn turned_degrees(centre: P, start: P, now: P, steps: bool) -> f64 {
    let (ax, ay) = (start[0] - centre[0], start[1] - centre[1]);
    let (bx, by) = (now[0] - centre[0], now[1] - centre[1]);
    let degrees = (ax * by - ay * bx).atan2(ax * bx + ay * by).to_degrees();
    if steps { (degrees / 15.0).round() * 15.0 } else { degrees }
}

/// A selection box drawn from right to left also takes what it touches.
fn box_touches(start: Pos2, now: Pos2) -> bool {
    now.x < start.x
}

/// Whether any part of a straight line lies in a box, even with both its ends outside.
fn segment_meets_box(a: P, b: P, lo: P, hi: P) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for axis in 0..2 {
        let d = b[axis] - a[axis];
        if d.abs() < 1e-12 {
            if a[axis] < lo[axis] || a[axis] > hi[axis] {
                return false;
            }
            continue;
        }
        let (p, q) = ((lo[axis] - a[axis]) / d, (hi[axis] - a[axis]) / d);
        let (near, far) = if p < q { (p, q) } else { (q, p) };
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return false;
        }
    }
    true
}

const RULER: f32 = 24.0;
const SCROLL: f32 = 16.0;

impl App {
    pub fn to_screen(&self, p: P) -> Pos2 {
        pos2(self.origin.x + p[0] as f32 * self.scale, self.origin.y - p[1] as f32 * self.scale)
    }

    pub fn to_world(&self, p: Pos2) -> P {
        [((p.x - self.origin.x) / self.scale) as f64, ((self.origin.y - p.y) / self.scale) as f64]
    }

    /// Fits the whole sheet in `rect`, with the same small margin as the pictures.
    pub fn fit_view(&mut self, rect: Rect) {
        let (w, h) = (self.doc.job.width as f32, self.doc.job.height as f32);
        self.scale = (rect.width() * 0.953 / w).min(rect.height() * 0.953 / h);
        let c = rect.center();
        self.origin = pos2(c.x - w * self.scale / 2.0, c.y + h * self.scale / 2.0);
    }

    fn zoom_about(&mut self, at: Pos2, factor: f32) {
        let world = self.to_world(at);
        self.scale = (self.scale * factor).clamp(0.2, 100_000.0);
        let now = self.to_screen(world);
        self.origin += at - now;
    }

    /// Trims under the pointer: the moment the button goes down, then all along the sweep.
    fn continue_trim_stroke(&mut self, at: Pos2, shift: bool) {
        let Some(stroke) = &self.trim_stroke else {
            self.trim_stroke = Some(TrimStroke { last: at, undo_mark: self.undo.len(), trail: vec![at], trimmed: 0 });
            let world = self.to_world(at);
            let hit = self.trim_at(world, (8.0 / self.scale) as f64, shift);
            if let Some(s) = &mut self.trim_stroke {
                s.trimmed += usize::from(hit);
            }
            return;
        };
        let last = stroke.last;
        let gap = (at - last).length();
        if gap < 1.0 {
            return;
        }
        // Tighter than a click, so a sweep only takes the lines it really crosses.
        let reach = (3.0 / self.scale) as f64;
        let steps = (gap / 2.0).ceil() as usize;
        let mut hits = 0;
        for i in 1..=steps {
            let world = self.to_world(last + (at - last) * (i as f32 / steps as f32));
            hits += usize::from(self.trim_at(world, reach, shift));
        }
        if let Some(s) = &mut self.trim_stroke {
            s.last = at;
            s.trail.push(at);
            s.trimmed += hits;
        }
    }

    fn finish_trim_stroke(&mut self) {
        let Some(stroke) = self.trim_stroke.take() else { return };
        // However many pieces went, one undo brings them all back.
        self.undo.truncate(stroke.undo_mark + usize::from(stroke.trimmed > 0));
        if stroke.trimmed > 1 && !self.status.contains(crate::i18n::tr("rule")) {
            self.status = tf!("Trimmed {} sections.", stroke.trimmed);
        } else if stroke.trimmed > 0 && self.status == crate::i18n::tr("That line doesn't cross anything. Hold Shift and click to delete it.") {
            self.status = crate::i18n::tr("Ready").into();
        }
    }

    /// The grid's fine and bold spacings at this zoom, in job units.
    pub fn grid_steps(&self) -> (f64, f64) {
        if self.grid_spacing > 0.0 {
            return (self.grid_spacing, self.grid_spacing * 5.0);
        }
        let fine = nice_step(16.0 / self.scale as f64);
        (fine, fine * 5.0)
    }

    /// The spacing the grid is drawn at: a set spacing, five times over as often as needed so the
    /// lines stay at least 6 pixels apart when zoomed out.
    fn grid_drawn_steps(&self) -> (f64, f64) {
        let (mut fine, _) = self.grid_steps();
        while fine * (self.scale as f64) < 6.0 && fine > 0.0 {
            fine *= 5.0;
        }
        (fine, fine * 5.0)
    }

    fn visible_vectors(&self) -> impl Iterator<Item = &crate::app::Vector> {
        self.doc.vectors.iter().filter(|v| self.doc.shows(v))
    }

    fn pickable(&self, v: &crate::app::Vector) -> bool {
        self.doc.can_pick(v)
    }

    /// The shape whose outline is under the pointer.
    fn pick_edge(&self, at: Pos2) -> Option<u64> {
        let w = self.to_world(at);
        let reach = (6.0 / self.scale) as f64;
        let mut best: Option<(f64, u64)> = None;
        for v in self.visible_vectors().filter(|v| self.pickable(v)) {
            for (a, b) in v.paths.iter().flat_map(|p| p.segments()) {
                let d = geom::dist2_seg(w, a, b);
                if d <= reach * reach && best.is_none_or(|(old, _)| d < old) {
                    best = Some((d, v.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// The shape under the pointer: nearest edge first, then the smallest shape around it.
    fn pick(&self, at: Pos2) -> Option<u64> {
        if let Some(id) = self.pick_edge(at) {
            return Some(id);
        }
        let w = self.to_world(at);
        let mut inside: Option<(f64, u64)> = None;
        for v in self.visible_vectors().filter(|v| self.pickable(v)) {
            if geom::inside_even_odd(&v.paths, w) {
                let (lo, hi) = geom::bbox(&v.paths)?;
                let area = (hi[0] - lo[0]) * (hi[1] - lo[1]);
                if inside.is_none_or(|(old, _)| area < old) {
                    inside = Some((area, v.id));
                }
            }
        }
        inside.map(|(_, id)| id)
    }

    /// The shapes a selection box takes: those wholly inside it, and those it `touches` as well.
    /// A box lying entirely within a large shape doesn't touch its outline, so doesn't take it.
    fn box_picks(&self, a: P, b: P, touches: bool) -> Vec<u64> {
        let lo = [a[0].min(b[0]), a[1].min(b[1])];
        let hi = [a[0].max(b[0]), a[1].max(b[1])];
        self.visible_vectors()
            .filter(|v| self.pickable(v))
            .filter(|v| {
                let Some((low, high)) = geom::bbox(&v.paths) else { return false };
                if low[0] >= lo[0] && low[1] >= lo[1] && high[0] <= hi[0] && high[1] <= hi[1] {
                    return true;
                }
                touches && v.paths.iter().any(|p| p.segments().any(|(s, e)| segment_meets_box(s, e, lo, hi)))
            })
            .map(|v| v.id)
            .collect()
    }

    /// The handles for resizing, each with the point that stays put: the corners, then the middles
    /// of the sides. Only once a click on the selection has asked for them.
    pub(crate) fn handle_rects(&self) -> Vec<(Pos2, P)> {
        self.handle_points().into_iter().enumerate().map(|(i, (corner, anchor))| (self.to_screen(corner) + HANDLE_OUT[i], anchor)).collect()
    }

    /// The same handles in the drawing's own units: each one's place and the point that stays put.
    pub(crate) fn handle_points(&self) -> Vec<(P, P)> {
        if self.handles_on.as_deref() != Some(self.selected.as_slice()) {
            return Vec::new();
        }
        let Some((lo, hi)) = self.selected_bbox() else { return Vec::new() };
        let (mx, my) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
        [
            ([lo[0], lo[1]], [hi[0], hi[1]]),
            ([hi[0], lo[1]], [lo[0], hi[1]]),
            ([hi[0], hi[1]], [lo[0], lo[1]]),
            ([lo[0], hi[1]], [hi[0], lo[1]]),
            ([mx, lo[1]], [mx, hi[1]]),
            ([hi[0], my], [lo[0], my]),
            ([mx, hi[1]], [mx, lo[1]]),
            ([lo[0], my], [hi[0], my]),
        ]
        .into()
    }

    /// With one text chosen, the gap between its letters or lines under the pointer.
    pub(crate) fn text_gap_under(&self, p: Pos2) -> Option<(u64, crate::fonts::TextGap)> {
        let [only] = self.selected.as_slice() else { return None };
        let v = self.doc.vectors.iter().find(|v| v.id == *only)?;
        let Prim::Text { at, height, text, style } = &v.prim else { return None };
        if style.vertical {
            return None;
        }
        let reach = (TEXT_GAP_REACH / self.scale) as f64;
        crate::fonts::gap_at(text, *height, *at, style, self.to_world(p), reach).map(|g| (v.id, g))
    }

    /// The original's text spacing clicks (`0x140a84a90`): letters or lines closer, apart with
    /// Shift, by a bigger step with Ctrl. One undo step each.
    pub(crate) fn move_text_gap(&mut self, id: u64, gap: crate::fonts::TextGap, apart: bool, coarse: bool) {
        let Some(i) = self.doc.vectors.iter().position(|v| v.id == id) else { return };
        let Prim::Text { at, height, text, mut style } = self.doc.vectors[i].prim.clone() else { return };
        if !crate::fonts::adjust_gap(&text, height, at, &mut style, gap, apart, coarse) {
            return;
        }
        self.checkpoint();
        let prim = Prim::Text { at, height, text, style };
        self.doc.vectors[i].paths = prim.paths();
        self.doc.vectors[i].prim = prim;
        self.dirty = true;
    }

    /// The handles for turning, a little beyond the corners (`0x140a6f810`).
    pub(crate) fn rotate_handles(&self) -> Vec<Pos2> {
        if self.handles_on.as_deref() != Some(self.selected.as_slice()) {
            return Vec::new();
        }
        let Some((lo, hi)) = self.selected_bbox() else { return Vec::new() };
        let beyond = 17.0;
        [([lo[0], lo[1]], vec2(-beyond, beyond)), ([hi[0], lo[1]], vec2(beyond, beyond)), ([hi[0], hi[1]], vec2(beyond, -beyond)), ([lo[0], hi[1]], vec2(-beyond, -beyond))].into_iter().map(|(corner, away)| self.to_screen(corner) + away).collect()
    }

    /// What the selection turns about: the middle of what moves with it, unless it has been moved.
    pub(crate) fn rotation_centre(&self) -> Option<P> {
        if let Some((ids, c)) = &self.rotate_centre {
            if ids == &self.selected {
                return Some(*c);
            }
        }
        let (lo, hi) = self.moving_bbox()?;
        Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
    }

    pub fn fit_selection(&mut self, rect: Rect) {
        let Some((lo, hi)) = self.selected_bbox() else {
            self.fit_view(rect);
            return;
        };
        let (w, h) = (((hi[0] - lo[0]) as f32).max(0.01), ((hi[1] - lo[1]) as f32).max(0.01));
        self.scale = (rect.width() * 0.7 / w).min(rect.height() * 0.7 / h);
        let c = rect.center();
        self.origin = pos2(
            c.x - ((lo[0] + hi[0]) / 2.0) as f32 * self.scale,
            c.y + ((lo[1] + hi[1]) / 2.0) as f32 * self.scale,
        );
        self.auto_fit = false;
    }

    pub fn view_2d(&mut self, ui: &mut egui::Ui, full: Rect) {
        let sheet_rect = Rect::from_min_max(
            pos2(full.left() + RULER + 1.0, full.top() + RULER + 1.0),
            pos2(full.right() - SCROLL, full.bottom() - SCROLL),
        );
        if self.auto_fit && sheet_rect != self.last_rect {
            self.fit_view(sheet_rect);
        }
        self.last_rect = sheet_rect;
        self.view_rect = sheet_rect;

        let response = ui.allocate_rect(sheet_rect, Sense::click_and_drag());
        self.snap_held_off = ui.input(|i| i.modifiers.alt);
        self.handle_pointer(ui, &response, sheet_rect);
        // Guides are pulled out of the rulers.
        let top = Rect::from_min_max(pos2(sheet_rect.left(), full.top()), pos2(sheet_rect.right(), sheet_rect.top()));
        let side = Rect::from_min_max(pos2(full.left(), sheet_rect.top()), pos2(sheet_rect.left(), sheet_rect.bottom()));
        for (rect, vertical, name) in [(top, false, crate::i18n::tr("Top ruler")), (side, true, crate::i18n::tr("Side ruler"))] {
            let ruler = ui.interact(rect, ui.scope_id().with(name), Sense::drag()).on_hover_text(crate::i18n::tr("Drag onto the sheet to add a guide line"));
            crate::ui::label_widget(&ruler, name);
            if ruler.drag_started() {
                self.checkpoint();
                self.show_guides = true;
                self.doc.guides.push(crate::snap::Guide { vertical, at: -1e6 });
                self.drag = Drag::Guide(self.doc.guides.len() - 1);
            }
        }
        if let Drag::Guide(index) = self.drag {
            let (at, down) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
            self.drag_guide(index, at, down, sheet_rect);
        }
        self.paint_sheet(ui, sheet_rect);
        self.paint_rulers(ui, full, sheet_rect);
        self.scrollbars(ui, sheet_rect);
    }

    fn handle_pointer(&mut self, ui: &mut egui::Ui, response: &egui::Response, rect: Rect) {
        let (modifiers, scroll) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y));
        let hover = response.hover_pos();
        self.cursor = hover.map(|p| self.to_world(p));
        if let Some(p) = hover {
            if scroll != 0.0 {
                self.zoom_about(p, (scroll * 0.002).exp());
                self.auto_fit = false;
            }
        }
        if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) || (self.tool == Tool2::Pan && response.dragged_by(egui::PointerButton::Primary)) {
            self.origin += response.drag_delta();
            self.auto_fit = false;
            return;
        }
        if self.tool == Tool2::Pan {
            return;
        }
        // A drag is only recognised after some movement, so its start is where the button went down.
        let press = response
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| ui.input(|i| i.pointer.press_origin()))
            .flatten();

        let pointer = response.interact_pointer_pos().or(hover);
        if matches!(self.drag, Drag::Guide(_)) {
            return;
        }
        // An existing guide can be picked up, unless a tool or a shape comes first.
        if let Some(p) = press {
            let free = matches!(self.left, LeftForm::None) && self.tool == Tool2::Select && self.pick(p).is_none();
            if let Some(i) = self.guide_at(p).filter(|_| free) {
                self.checkpoint();
                self.drag = Drag::Guide(i);
                return;
            }
        }

        // Dimension labels: drag to place, double click to change the value.
        if let Drag::DimLabel { index, last } = self.drag {
            if let Some(p) = pointer {
                let d = (p - last) / self.scale;
                if let Some(c) = self.doc.constraints.get_mut(index) {
                    c.place[0] += d.x as f64;
                    c.place[1] -= d.y as f64;
                }
                self.drag = Drag::DimLabel { index, last: p };
            }
            if response.drag_stopped() {
                self.drag = Drag::None;
            }
            return;
        }
        let trimming = matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Trim);
        if let Some(i) = press.filter(|_| !trimming).and_then(|p| self.dim_at(p)) {
            self.drag = Drag::DimLabel { index: i, last: press.unwrap() };
            return;
        }
        if response.double_clicked() && !trimming {
            if let Some(i) = pointer.and_then(|p| self.dim_at(p)) {
                self.open_dimension(i);
                return;
            }
        }

        // Line tools add a point per click and finish on a double click.
        let line_tool = matches!(&self.left, LeftForm::Draw(f) if matches!(f.kind, DrawKind::Polyline | DrawKind::Curve));
        if line_tool {
            if response.secondary_clicked() {
                self.finish_line(false);
                self.left = LeftForm::None;
                return;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Space)) && !ui.ctx().egui_wants_keyboard_input() {
                self.finish_line(false);
                return;
            }
            if response.double_clicked() {
                self.finish_line(false);
            } else if response.clicked() {
                if let Some(p) = pointer {
                    let world = self.snapped(self.to_world(p));
                    let first = match &self.left {
                        LeftForm::Draw(f) => f.points.first().copied(),
                        _ => None,
                    };
                    let count = match &self.left {
                        LeftForm::Draw(f) => f.points.len(),
                        _ => 0,
                    };
                    let closes = first.is_some_and(|f| count >= 3 && (self.to_screen(f) - p).length() < 8.0);
                    if closes {
                        self.finish_line(true);
                    } else if let LeftForm::Draw(f) = &mut self.left {
                        f.points.push(world);
                        f.next = world;
                        f.delta = [0.0, 0.0];
                        f.step = [0.0, 0.0];
                    }
                }
            }
            return;
        }

        if let LeftForm::Xform(f) = &self.left {
            let f = f.clone();
            let reach = (8.0 / self.scale) as f64;
            match f.kind {
                Xform::Fillet => {
                    if response.clicked() {
                        if let Some(p) = pointer {
                            let at = self.to_world(p);
                            self.fillet_at(at, reach, f.distance, f.fillet);
                        }
                    }
                    return;
                }
                Xform::Trim => {
                    let (down, shift) = ui.input(|i| (i.pointer.primary_down(), i.modifiers.shift));
                    if down && (self.trim_stroke.is_some() || response.is_pointer_button_down_on()) {
                        if let Some(p) = pointer {
                            self.continue_trim_stroke(p, shift);
                        }
                    } else if self.trim_stroke.is_some() {
                        self.finish_trim_stroke();
                    } else if response.clicked() {
                        // Pressed and released within one frame.
                        if let Some(p) = pointer {
                            let at = self.to_world(p);
                            self.trim_at(at, reach, shift);
                        }
                    }
                    return;
                }
                Xform::Distort if self.envelope.as_ref().is_some_and(|e| e.has_handles()) => {
                    if let Some(p) = press {
                        let corners = self.envelope.as_ref().unwrap().corners;
                        let hit = (0..4).find(|i| (self.to_screen(corners[*i]) - p).length() < 10.0);
                        if let Some(i) = hit {
                            self.drag = Drag::Envelope(i);
                        }
                    }
                    if let (Drag::Envelope(i), Some(p)) = (&self.drag, pointer) {
                        let (i, to) = (*i, self.snapped(self.to_world(p)));
                        self.move_envelope_corner(i, to);
                    }
                    if response.drag_stopped() {
                        self.drag = Drag::None;
                    }
                    return;
                }
                Xform::Constraints => {
                    use crate::constraints::Want;
                    let want = match (f.smart, f.constraint) {
                        (false, Some(kind)) => kind.want(&self.picks),
                        _ => Some(Want::Any),
                    };
                    self.hover_elem = match (want, hover) {
                        (Some(w), Some(p)) => self.pick_element(self.to_world(p), reach, w),
                        _ => None,
                    };
                    // The rules' badges answer to the pointer first.
                    let chip_at = |app: &App, at: Pos2| app.rule_chips().into_iter().find(|(_, r)| r.contains(at)).map(|(i, _)| i);
                    if self.show_constraints {
                        if let Some(i) = hover.and_then(|p| chip_at(self, p)) {
                            self.hover_rule = Some(i);
                            self.hover_elem = None;
                        }
                    }
                    // Drag anything to see how the rules let it move.
                    if let Some(p) = press {
                        let at = self.to_world(p);
                        if let Some(e) = self.pick_element(at, reach, Want::Any) {
                            self.selected_rule = None;
                            self.begin_sketch_drag(e, self.snapped(at));
                        }
                    }
                    if let (Drag::Sketch { elem, last }, Some(p)) = (self.drag.clone(), pointer) {
                        let now = self.snapped(self.to_world(p));
                        if now != last {
                            self.sketch_drag(elem, last, now);
                            self.drag = Drag::Sketch { elem, last: now };
                        }
                        if response.drag_stopped() {
                            self.end_sketch_drag();
                        }
                        return;
                    }
                    if response.clicked() {
                        if let Some(p) = pointer.filter(|p| self.dim_at(*p).is_none()) {
                            if let Some(i) = chip_at(self, p).filter(|_| self.show_constraints) {
                                self.selected_rule = if self.selected_rule == Some(i) { None } else { Some(i) };
                                return;
                            }
                            self.selected_rule = None;
                            self.sketch_note = None;
                            let at = self.to_world(p);
                            self.sketch_click(&f, at, reach);
                        }
                    }
                    return;
                }
                Xform::Dimension => {
                    if let Some(p) = press {
                        self.drag = Drag::Measure { start: self.snapped(self.to_world(p)) };
                    }
                    if response.drag_stopped() {
                        if let (Drag::Measure { start }, Some(p)) = (self.drag.clone(), pointer) {
                            let end = self.snapped(self.to_world(p));
                            self.add_dimension(start, end, f.dim, &f);
                        }
                        self.drag = Drag::None;
                    }
                    return;
                }
                _ => {}
            }
        }
        let is_draw = matches!(&self.left, LeftForm::Draw(_));
        if is_draw {
            self.handle_draw(response, pointer, press, modifiers.shift);
            return;
        }

        match self.tool {
            Tool2::Measure => self.handle_measure(response, pointer, press),
            Tool2::ZoomBox => self.handle_zoom_box(response, pointer, press, rect),
            Tool2::Nodes => self.handle_nodes(response, pointer, press),
            Tool2::Select | Tool2::Pan => self.handle_select(response, pointer, press, modifiers.shift),
        }
    }

    fn handle_draw(&mut self, response: &egui::Response, pointer: Option<Pos2>, press: Option<Pos2>, shift: bool) {
        // Shift + click loads an existing shape into the form.
        if shift && response.clicked() {
            if let Some(id) = pointer.and_then(|p| self.pick(p)) {
                let shape = self.doc.vectors.iter().find(|v| v.id == id).cloned();
                if let (Some(shape), LeftForm::Draw(f)) = (shape, &mut self.left) {
                    if f.load_shape(&shape) {
                        self.selected = vec![id];
                    } else {
                        self.status = crate::i18n::tr("That shape is a different kind, so this form can't edit it.").into();
                    }
                }
            }
            return;
        }
        if response.clicked() {
            if let (Some(p), LeftForm::Draw(f)) = (pointer, &self.left) {
                if f.kind == DrawKind::Text {
                    let world = self.snapped(self.to_world(p));
                    if let LeftForm::Draw(f) = &mut self.left {
                        f.c = world;
                        f.editing = None;
                    }
                    self.apply_draw();
                }
            }
            return;
        }
        if let Some(p) = press {
            let start = self.snapped(self.to_world(p));
            self.drag = Drag::Draw { start };
            if let LeftForm::Draw(f) = &mut self.left {
                f.editing = None;
            }
        }
        if let (Drag::Draw { start }, Some(p)) = (&self.drag, pointer) {
            let start = *start;
            let now = self.snapped(self.to_world(p));
            if let LeftForm::Draw(f) = &mut self.left {
                let radius = (now[0] - start[0]).hypot(now[1] - start[1]);
                match f.kind {
                    DrawKind::Rect => {
                        f.c = [(start[0] + now[0]) / 2.0, (start[1] + now[1]) / 2.0];
                        f.w = (now[0] - start[0]).abs();
                        f.h = (now[1] - start[1]).abs();
                    }
                    DrawKind::Ellipse => {
                        f.c = start;
                        f.rx = (now[0] - start[0]).abs();
                        f.ry = (now[1] - start[1]).abs();
                    }
                    DrawKind::Star => {
                        f.c = start;
                        f.r = radius;
                        f.inner = radius * 0.45;
                    }
                    DrawKind::Arc => {
                        f.c = start;
                        f.r = radius;
                        f.start = (now[1] - start[1]).atan2(now[0] - start[0]).to_degrees();
                    }
                    DrawKind::Text => {}
                    _ => {
                        f.c = start;
                        f.r = radius;
                    }
                }
            }
            if response.drag_stopped() {
                self.drag = Drag::None;
                let valid = match &self.left {
                    LeftForm::Draw(f) => match f.kind {
                        DrawKind::Rect => f.w > 0.0 && f.h > 0.0,
                        DrawKind::Ellipse => f.rx > 0.0 && f.ry > 0.0,
                        DrawKind::Text => false,
                        _ => f.r > 0.0,
                    },
                    _ => false,
                };
                if valid {
                    self.apply_draw();
                }
            }
        }
    }

    fn handle_measure(&mut self, response: &egui::Response, pointer: Option<Pos2>, press: Option<Pos2>) {
        if let Some(p) = press {
            let start = self.snapped(self.to_world(p));
            self.drag = Drag::Measure { start };
            self.measure = Some((start, start));
        }
        if let (Drag::Measure { start }, Some(p)) = (&self.drag, pointer) {
            let start = *start;
            let now = self.snapped(self.to_world(p));
            self.measure = Some((start, now));
            let d = (now[0] - start[0]).hypot(now[1] - start[1]);
            let angle = (now[1] - start[1]).atan2(now[0] - start[0]).to_degrees();
            self.status = tf!("Distance {:.4} {}, angle {angle:.1} degrees", d, self.doc.job.unit());
            if response.drag_stopped() {
                self.drag = Drag::None;
            }
        }
    }

    fn handle_zoom_box(&mut self, response: &egui::Response, pointer: Option<Pos2>, press: Option<Pos2>, rect: Rect) {
        if let Some(p) = press {
            self.drag = Drag::Zoom(p);
        }
        if response.drag_stopped() {
            if let (Drag::Zoom(start), Some(p)) = (&self.drag, pointer) {
                let bounds = Rect::from_two_pos(*start, p);
                if bounds.width() > 4.0 && bounds.height() > 4.0 {
                    let factor = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
                    let world = self.to_world(bounds.center());
                    self.scale = (self.scale * factor).clamp(0.2, 100_000.0);
                    let now = self.to_screen(world);
                    self.origin += rect.center() - now;
                    self.auto_fit = false;
                }
            }
            self.drag = Drag::None;
            self.tool = Tool2::Select;
        }
    }

    fn handle_nodes(&mut self, response: &egui::Response, pointer: Option<Pos2>, press: Option<Pos2>) {
        if response.clicked() {
            self.selected = pointer.and_then(|p| self.pick(p)).into_iter().collect();
            return;
        }
        if let Some(p) = press {
            let mut best: Option<(f32, u64, usize, usize)> = None;
            for v in self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)) {
                for (pi, path) in v.paths.iter().enumerate() {
                    for (ni, n) in path.pts.iter().enumerate() {
                        let d = (self.to_screen(*n) - p).length();
                        if d < 8.0 && best.is_none_or(|b| d < b.0) {
                            best = Some((d, v.id, pi, ni));
                        }
                    }
                }
            }
            if let Some((_, id, path, index)) = best {
                self.checkpoint();
                let start = self.doc.vectors.iter().find(|v| v.id == id).map_or([0.0, 0.0], |v| v.paths[path].pts[index]);
                self.drag = Drag::Node { id, path, index, start };
            }
        }
        // Values typed while dragging a point place it (the original's quick keys for editing points).
        if self.typed_value(&response.ctx) {
            return;
        }
        if let (Drag::Node { id, path, index, .. }, Some(p)) = (&self.drag, pointer) {
            let (id, path, index) = (*id, *path, *index);
            let world = self.snapped(self.to_world(p));
            if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == id) {
                v.paths[path].pts[index] = world;
                v.prim = Prim::Free;
            }
            if response.drag_stopped() {
                self.drag = Drag::None;
            }
        }
    }

    fn handle_select(&mut self, response: &egui::Response, pointer: Option<Pos2>, press: Option<Pos2>, shift: bool) {
        if let Some(p) = response.hover_pos() {
            use egui::CursorIcon::{ResizeHorizontal, ResizeNeSw, ResizeNwSe, ResizeVertical};
            // The sheet's Y goes up: its first corner is bottom left on the screen.
            let icons = [ResizeNeSw, ResizeNwSe, ResizeNeSw, ResizeNwSe, ResizeVertical, ResizeHorizontal, ResizeVertical, ResizeHorizontal];
            if let Some(i) = self.handle_rects().iter().position(|(h, _)| (*h - p).length() < 7.0) {
                response.ctx.set_cursor_icon(icons[i]);
            } else if self.rotate_handles().iter().any(|h| (*h - p).length() < 7.0) {
                response.ctx.set_cursor_icon(egui::CursorIcon::Alias);
            } else if !self.rotate_handles().is_empty() && self.rotation_centre().is_some_and(|c| (self.to_screen(c) - p).length() < 7.0) {
                response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            } else if let Some((_, gap)) = self.text_gap_under(p) {
                // Between letters or lines of the chosen text: a click there moves them (`0x140a84e50`).
                response.ctx.set_cursor_icon(if matches!(gap, crate::fonts::TextGap::Letter(_)) { ResizeHorizontal } else { ResizeVertical });
            }
        }
        // A double click takes in the whole of a piece, without choosing it from the buttons.
        if response.double_clicked() && !shift && !self.adjust_piece {
            let hit = pointer.and_then(|p| self.pick(p)).filter(|id| self.doc.vectors.iter().any(|v| v.id == *id && v.piece.is_some()));
            if let Some(id) = hit {
                self.selected = vec![id];
                self.select_piece();
                self.handles_on = None;
                return;
            }
        }
        if response.clicked() {
            if let Some((id, gap)) = pointer.and_then(|p| self.text_gap_under(p)) {
                let coarse = response.ctx.input(|i| i.modifiers.command);
                self.move_text_gap(id, gap, shift, coarse);
                return;
            }
            let hit = pointer.and_then(|p| self.pick(p));
            match (hit, shift) {
                (Some(id), true) => {
                    self.handles_on = None;
                    if let Some(i) = self.selected.iter().position(|s| *s == id) {
                        self.selected.remove(i);
                    } else {
                        self.selected.push(id);
                    }
                }
                // A click on what's already selected changes between just moving it and resizing it.
                (Some(id), false) if self.selected.contains(&id) => {
                    let shown = self.handles_on.as_deref() == Some(self.selected.as_slice());
                    self.handles_on = (!shown).then(|| self.selected.clone());
                    self.rotate_centre = None;
                }
                (Some(id), false) => {
                    self.handles_on = None;
                    self.selected = vec![id];
                }
                (None, false) => {
                    self.handles_on = None;
                    self.selected.clear();
                }
                (None, true) => {}
            }
            return;
        }
        if let Some(p) = press {
            // The handle's own place, not its place on screen turned back: the screen rounds it, and a side
            // handle a hair off its anchor would be taken for a corner and stretch the shape a million times.
            let handle = self.handle_points().into_iter().enumerate().find(|(i, (corner, _))| (self.to_screen(*corner) + HANDLE_OUT[*i] - p).length() < 7.0);
            if let Some((index, (start, anchor))) = handle {
                self.checkpoint();
                let orig = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).map(|v| (v.id, v.paths.clone())).collect();
                let middle = self.selected_bbox().map_or(anchor, |(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
                // The first four handles are the corners.
                self.drag = Drag::Scale { anchor, middle, start, corner: index < 4, out: HANDLE_OUT[index], orig };
            } else if let (Some(centre), true) = (self.rotation_centre(), self.rotate_handles().iter().any(|h| (*h - p).length() < 7.0)) {
                self.checkpoint();
                let moving = self.moving_ids();
                let orig = self.doc.vectors.iter().filter(|v| moving.contains(&v.id)).map(|v| (v.id, v.paths.clone())).collect();
                self.drag = Drag::Rotate { centre, start: self.to_world(p), orig };
            } else if !self.rotate_handles().is_empty() && self.rotation_centre().is_some_and(|c| (self.to_screen(c) - p).length() < 7.0) {
                self.drag = Drag::RotateCentre;
            } else {
                // Dragging from inside a shape that isn't selected draws a box, so the shapes
                // within a panel can be selected without moving the panel.
                let hit = self.pick_edge(p).or_else(|| self.pick(p).filter(|id| self.selected.contains(id)));
                match hit {
                    Some(id) => {
                        if !self.selected.contains(&id) {
                            self.selected = vec![id];
                        }
                        self.checkpoint();
                        let at = self.snapped(self.to_world(p));
                        self.drag = Drag::Move { last: at, start: at };
                    }
                    None => self.drag = Drag::Box(p),
                }
            }
        }
        if self.typed_value(&response.ctx) {
            return;
        }
        let Some(p) = pointer else { return };
        match self.drag.clone() {
            Drag::Move { last, start } => {
                let now = self.snapped(self.to_world(p));
                self.translate_selected(now[0] - last[0], now[1] - last[1]);
                self.drag = Drag::Move { last: now, start };
            }
            Drag::Scale { anchor, middle, start, corner, out, orig } => {
                let alt = response.ctx.input(|i| i.modifiers.alt);
                // With Shift it grows from its middle (`0x140a72f00`).
                let anchor = if shift { middle } else { anchor };
                let now = self.snapped(self.to_world(p - out));
                let now = if corner && !alt { keep_proportions(start, now, anchor) } else { now };
                // Each way stretches by how far the handle has gone from what stays put; a handle level with it
                // (exactly: both are the drawing's own numbers) leaves that way alone (`0x140a73262`).
                let ratio = |n: f64, s: f64, a: f64| if s == a { 1.0 } else { (n - a) / (s - a) };
                let (sx, sy) = (ratio(now[0], start[0], anchor[0]), ratio(now[1], start[1], anchor[1]));
                for (id, paths) in &orig {
                    if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == *id) {
                        v.paths = paths.clone();
                        for q in v.paths.iter_mut().flat_map(|q| q.pts.iter_mut()) {
                            *q = [anchor[0] + (q[0] - anchor[0]) * sx, anchor[1] + (q[1] - anchor[1]) * sy];
                        }
                        v.prim = Prim::Free;
                    }
                }
            }
            Drag::Rotate { centre, start, orig } => {
                let alt = response.ctx.input(|i| i.modifiers.alt);
                let now = self.to_world(p);
                let degrees = turned_degrees(centre, start, now, alt);
                self.turn_from(&orig, centre, degrees);
                self.status = tf!("Turned {:.1} degrees", degrees);
            }
            Drag::RotateCentre => {
                self.rotate_centre = Some((self.selected.clone(), self.snapped(self.to_world(p))));
            }
            _ => {}
        }
        if response.drag_stopped() {
            if let Drag::Box(start) = self.drag {
                self.handles_on = None;
                let ids = self.box_picks(self.to_world(start), self.to_world(p), box_touches(start, p));
                if !shift {
                    self.selected.clear();
                }
                for id in ids {
                    if !self.selected.contains(&id) {
                        self.selected.push(id);
                    }
                }
            }
            self.drag = Drag::None;
        }
    }

    fn paint_sheet(&mut self, ui: &egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::WORKSPACE);
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let sheet = Rect::from_two_pos(self.to_screen([0.0, 0.0]), self.to_screen([w, h]));
        // Faint lines through the origin, as in the pictures.
        let axis = Stroke::new(1.0, theme::CARD);
        let zero = self.to_screen([0.0, 0.0]);
        painter.line_segment([pos2(rect.left(), zero.y), pos2(rect.right(), zero.y)], axis);
        painter.line_segment([pos2(zero.x, rect.top()), pos2(zero.x, rect.bottom())], axis);
        // The sheet lies on the bench.
        painter.rect_filled(sheet.translate(vec2(4.0, 5.0)).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 110));
        painter.rect_filled(sheet, 0.0, theme::SHEET);

        self.paint_bitmaps(ui, &painter);
        if self.grid {
            self.paint_grid(&painter, rect, sheet);
        }
        if self.show_model2d {
            if let Some(model) = &self.model {
                self.paint_model_outline(&painter, model);
            }
        }

        // The machine's zero: Burin's own mark, a ring on two short axes, X along and Y up, in the
        // teal of its other zero marks (the original puts a red square there).
        let origin = self.doc.job.origin();
        let datum = self.to_screen(origin);
        let ink = Stroke::new(1.8, crate::theme::ACCENT);
        let halo = Stroke::new(4.0, Color32::from_white_alpha(200));
        for stroke in [halo, ink] {
            painter.line_segment([datum, datum + vec2(14.0, 0.0)], stroke);
            painter.line_segment([datum, datum - vec2(0.0, 14.0)], stroke);
            painter.circle_stroke(datum, 4.5, stroke);
        }
        painter.circle_filled(datum, 1.6, crate::theme::ACCENT);

        if self.show_toolpaths {
            self.paint_toolpaths_2d(&painter);
        }

        if self.show_vectors {
            let dash = Stroke::new(1.2, MAGENTA);
            let in_selected_piece = self.doc.with_their_pieces(&self.selected);
            for v in self.visible_vectors() {
                let chosen = self.selected.contains(&v.id);
                let piece_mate = !chosen && in_selected_piece.contains(&v.id);
                let colour = self.doc.layers.get(v.layer).map_or([24, 26, 30], |l| l.color);
                // Lines under rules show how settled they are: blue can still move, red disagrees.
                let colour = match (self.constraint_conflict, self.defined.get(&v.id)) {
                    (true, Some(_)) => [200, 50, 40],
                    (_, Some(false)) => [28, 92, 196],
                    _ => colour,
                };
                let line = Stroke::new(1.0, Color32::from_rgb(colour[0], colour[1], colour[2]));
                for p in &v.paths {
                    let mut pts: Vec<Pos2> = p.pts.iter().map(|q| self.to_screen(*q)).collect();
                    if chosen {
                        if p.closed && !pts.is_empty() {
                            pts.push(pts[0]);
                        }
                        painter.extend(dashed_within(&pts, dash, 4.0, 3.0, painter.clip_rect()));
                    } else if piece_mate {
                        if p.closed && !pts.is_empty() {
                            pts.push(pts[0]);
                        }
                        painter.add(Shape::line(pts, Stroke::new(1.6, theme::PIECE)));
                    } else if v.construction {
                        if p.closed && !pts.is_empty() {
                            pts.push(pts[0]);
                        }
                        painter.extend(dashed_within(&pts, Stroke::new(1.0, theme::CONSTRUCTION), 9.0, 4.0, painter.clip_rect()));
                    } else if p.closed {
                        painter.add(Shape::closed_line(pts, line));
                    } else {
                        painter.add(Shape::line(pts, line));
                    }
                }
            }
        }

        // A frame around each piece with something selected in it.
        if self.show_vectors {
            let mut pieces: Vec<u64> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).filter_map(|v| v.piece).collect();
            pieces.sort_unstable();
            pieces.dedup();
            for piece in pieces {
                let lines = self.doc.vectors.iter().filter(|v| v.piece == Some(piece)).flat_map(|v| v.paths.iter());
                let Some((lo, hi)) = geom::bbox(lines) else { continue };
                let (a, b) = (self.to_screen(lo), self.to_screen(hi));
                let frame = Rect::from_two_pos(a, b).expand(7.0);
                let corners = [frame.left_top(), frame.right_top(), frame.right_bottom(), frame.left_bottom(), frame.left_top()];
                painter.extend(dashed_within(&corners, Stroke::new(1.2, theme::PIECE), 7.0, 4.0, painter.clip_rect()));
                let name = self.doc.piece_name(piece);
                let label = if self.adjust_piece { tf!("{}: adjusting", name) } else { name };
                painter.text(frame.left_top() + vec2(2.0, -3.0), Align2::LEFT_BOTTOM, label, FontId::proportional(12.0), theme::PIECE);
            }
        }

        // What the vector validator found.
        for issue in &self.issues {
            let colour = match issue.kind {
                crate::edit::IssueKind::ZeroLength => theme::RAPID,
                crate::edit::IssueKind::Overlap => theme::WARNING,
                crate::edit::IssueKind::Intersection => Color32::from_rgb(30, 120, 220),
            };
            let at = self.to_screen(issue.at);
            painter.rect_stroke(Rect::from_center_size(at, vec2(9.0, 9.0)), 1.0, Stroke::new(1.6, colour), StrokeKind::Inside);
        }

        self.paint_constraints(&painter);
        self.paint_guides(&painter, rect);
        self.paint_snap(&painter);
        // Where an imported relief will land.
        if let LeftForm::Relief(f) = &self.left {
            let far = [f.at[0] + f.width, f.at[1] + f.width * f.h as f64 / f.w as f64];
            let (lo, hi) = (self.to_screen(f.at), self.to_screen(far));
            let outline = [lo, pos2(hi.x, lo.y), hi, pos2(lo.x, hi.y), lo];
            painter.extend(dashed_within(&outline, Stroke::new(1.5, Color32::from_rgb(30, 80, 200)), 6.0, 4.0, painter.clip_rect()));
        }
        // What Trace Bitmap would make.
        for p in self.trace_preview.iter() {
            let mut pts: Vec<Pos2> = p.pts.iter().map(|q| self.to_screen(*q)).collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            painter.add(Shape::line(pts, Stroke::new(1.5, Color32::from_rgb(30, 110, 230))));
        }
        // The corners of a distortion.
        if let Some(env) = self.envelope.as_ref().filter(|e| e.has_handles()) {
            let mut outline: Vec<Pos2> = env.corners.iter().map(|c| self.to_screen(*c)).collect();
            outline.push(outline[0]);
            painter.extend(dashed_within(&outline, Stroke::new(1.0, Color32::from_rgb(30, 80, 200)), 5.0, 4.0, painter.clip_rect()));
            for c in &env.corners {
                let r = Rect::from_center_size(self.to_screen(*c), vec2(9.0, 9.0));
                painter.rect_filled(r, 0.0, Color32::from_rgb(30, 80, 200));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
            }
        }
        if let Some(stroke) = &self.trim_stroke {
            painter.add(Shape::line(stroke.trail.clone(), Stroke::new(1.5, theme::PICK)));
        }

        // Handles of the current tool.
        let draw_open = matches!(self.left, LeftForm::Draw(_));
        if !draw_open && self.tool == Tool2::Select {
            for (h, _) in self.handle_rects() {
                let r = Rect::from_center_size(h, vec2(8.0, 8.0));
                painter.rect_filled(r, 0.0, Color32::WHITE);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::SHEET_INK), StrokeKind::Inside);
            }
            if matches!(self.drag, Drag::Move { .. } | Drag::Rotate { .. } | Drag::Scale { .. }) {
                if let Some(at) = ui.ctx().pointer_latest_pos() {
                    let shown = if self.typed.is_empty() { crate::i18n::tr("Type a value, then Enter").to_string() } else { self.typed.clone() };
                    let r = painter.text(at + vec2(16.0, 18.0), egui::Align2::LEFT_TOP, shown, egui::FontId::proportional(12.5), theme::TEXT);
                    painter.rect_filled(r.expand(4.0), 4.0, theme::CARD);
                    painter.text(at + vec2(16.0, 18.0), egui::Align2::LEFT_TOP, if self.typed.is_empty() { crate::i18n::tr("Type a value, then Enter").to_string() } else { self.typed.clone() }, egui::FontId::proportional(12.5), theme::TEXT);
                }
            }
            let turning = self.rotate_handles();
            for h in &turning {
                painter.circle(*h, 4.5, Color32::WHITE, Stroke::new(1.0, theme::PICK));
            }
            if let (false, Some(c)) = (turning.is_empty(), self.rotation_centre()) {
                let c = self.to_screen(c);
                let line = Stroke::new(1.5, theme::PICK);
                painter.circle_stroke(c, 5.0, line);
                painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], line);
                painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], line);
            }
        }
        if self.tool == Tool2::Nodes {
            for v in self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)) {
                for n in v.paths.iter().flat_map(|p| p.pts.iter()) {
                    let r = Rect::from_center_size(self.to_screen(*n), vec2(6.0, 6.0));
                    painter.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(40, 80, 220));
                }
            }
        }

        // What is being drawn right now.
        let preview = Stroke::new(1.0, Color32::from_rgb(0, 110, 230));
        if let LeftForm::Draw(f) = &self.left {
            // A text shows as it will be while its form is open: its font, size and place change it.
            if matches!(self.drag, Drag::Draw { .. }) || (f.kind == DrawKind::Text && !f.text.trim().is_empty()) {
                if let Some(prim) = f.prim() {
                    for p in prim.paths() {
                        let mut pts: Vec<Pos2> = p.pts.iter().map(|q| self.to_screen(*q)).collect();
                        if p.closed && !pts.is_empty() {
                            pts.push(pts[0]);
                        }
                        painter.add(Shape::line(pts, preview));
                    }
                }
            }
            if !f.points.is_empty() {
                let mut pts: Vec<P> = if f.kind == DrawKind::Curve { geom::smooth_curve(&f.points) } else { f.points.clone() };
                if let Some(c) = self.cursor {
                    pts.push(c);
                }
                painter.add(Shape::line(pts.iter().map(|q| self.to_screen(*q)).collect(), preview));
                for q in &f.points {
                    painter.circle_filled(self.to_screen(*q), 3.0, Color32::from_rgb(0, 110, 230));
                }
            }
            if let Some(c) = self.cursor {
                let at = self.to_screen(c) + vec2(14.0, 14.0);
                let zero = self.doc.job.origin();
                let text = format!("X:{:.4}\nY:{:.4}", c[0] - zero[0], c[1] - zero[1]);
                painter.text(at, Align2::LEFT_TOP, text, FontId::proportional(12.0), Color32::BLACK);
            }
        }
        match &self.drag {
            Drag::Box(start) => {
                if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                    // Blue and solid takes what is inside; green and dashed also takes what it touches.
                    let r = Rect::from_two_pos(*start, p);
                    if box_touches(*start, p) {
                        let green = Color32::from_rgb(20, 150, 70);
                        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(20, 150, 70, 28));
                        let outline = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                        painter.extend(dashed_within(&outline, Stroke::new(1.0, green), 5.0, 4.0, painter.clip_rect()));
                    } else {
                        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0, 110, 230, 28));
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0, 110, 230)), StrokeKind::Inside);
                    }
                }
            }
            Drag::Zoom(start) => {
                if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                    painter.rect_stroke(Rect::from_two_pos(*start, p), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
                }
            }
            _ => {}
        }
        if let Some((a, b)) = self.measure.filter(|_| self.tool == Tool2::Measure) {
            let d = (b[0] - a[0]).hypot(b[1] - a[1]);
            let (pa, pb) = (self.to_screen(a), self.to_screen(b));
            painter.line_segment([pa, pb], Stroke::new(1.5, Color32::from_rgb(210, 40, 40)));
            painter.text(pb + vec2(8.0, 8.0), Align2::LEFT_TOP, format!("{d:.4}"), FontId::proportional(12.0), Color32::from_rgb(170, 20, 20));
        }
    }

    /// Fine and bold grid lines across the whole view, tuned to read on the sheet and around it.
    fn paint_grid(&self, painter: &egui::Painter, rect: Rect, sheet: Rect) {
        let (fine, _) = self.grid_drawn_steps();
        let (lo, hi) = (self.to_world(rect.left_bottom()), self.to_world(rect.right_top()));
        // The grid counts from the X/Y zero, like the rulers.
        let zero = self.doc.job.origin();
        let on_sheet = painter.with_clip_rect(sheet.intersect(rect));
        let colours = |bold: bool, paper: bool| match (bold, paper) {
            (false, true) => theme::GRID_FINE,
            (true, true) => theme::GRID_BOLD,
            (false, false) => theme::GRID_FINE_DARK,
            (true, false) => theme::GRID_BOLD_DARK,
        };
        for paper in [false, true] {
            let target = if paper { &on_sheet } else { painter };
            for bold in [false, true] {
                let stroke = Stroke::new(1.0, colours(bold, paper));
                let mut i = ((lo[0] - zero[0]) / fine).floor() as i64;
                while zero[0] + i as f64 * fine <= hi[0] {
                    if (i.rem_euclid(5) == 0) == bold {
                        let x = self.to_screen([zero[0] + i as f64 * fine, 0.0]).x.round() + 0.5;
                        target.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
                    }
                    i += 1;
                }
                let mut j = ((lo[1] - zero[1]) / fine).floor() as i64;
                while zero[1] + j as f64 * fine <= hi[1] {
                    if (j.rem_euclid(5) == 0) == bold {
                        let y = self.to_screen([0.0, zero[1] + j as f64 * fine]).y.round() + 0.5;
                        target.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
                    }
                    j += 1;
                }
            }
        }
    }

    /// Each visible path as a thin line, with arrows for its direction and a mark where it starts.
    fn paint_toolpaths_2d(&self, painter: &egui::Painter) {
        const SPACING: f32 = 72.0;
        let line = Stroke::new(1.0, Color32::from_rgb(196, 96, 96));
        let arrow = Stroke::new(1.0, theme::SHEET_INK);
        let view = painter.clip_rect().expand(12.0);
        // Filled: each cut as wide as its tool, to see what will be taken away.
        if self.solid_previews {
            for tp in self.doc.toolpaths.iter().filter(|t| t.visible && self.doc.toolpath_on_side(t)) {
                let width = (tp.op.tool.dia as f32 * self.scale).max(1.0);
                let fill = Color32::from_rgba_unmultiplied(196, 96, 96, 70);
                let mut run: Vec<Pos2> = Vec::new();
                for m in crate::toolpath::working_moves(&tp.moves) {
                    let q = self.to_screen([m.p[0], m.p[1]]);
                    if m.rapid {
                        if run.len() >= 2 {
                            painter.add(egui::Shape::line(std::mem::take(&mut run), Stroke::new(width, fill)));
                        }
                        run.clear();
                    }
                    if run.last().is_none_or(|l| (*l - q).length() >= MOST_UNSEEN_PIXELS) {
                        run.push(q);
                    }
                }
                if run.len() >= 2 {
                    painter.add(egui::Shape::line(run, Stroke::new(width, fill)));
                }
            }
        }
        for tp in self.doc.toolpaths.iter().filter(|t| t.visible && self.doc.toolpath_on_side(t)) {
            let mut drawn = std::collections::HashSet::new();
            let mut line_points: Vec<Pos2> = Vec::new();
            let mut at_screen: Option<Pos2> = None;
            let mut run = SPACING / 2.0;
            let flush = |points: &mut Vec<Pos2>| {
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(std::mem::take(points), line));
                } else {
                    points.clear();
                }
            };
            for m in crate::toolpath::working_moves(&tp.moves) {
                let q = self.to_screen([m.p[0], m.p[1]]);
                if m.rapid {
                    flush(&mut line_points);
                    run = SPACING / 2.0;
                    at_screen = Some(q);
                    continue;
                }
                let Some(p) = at_screen else {
                    at_screen = Some(q);
                    continue;
                };
                // What can't be seen isn't drawn: moves too small to show, and those off the screen.
                let d = q - p;
                let len = d.length();
                if len < MOST_UNSEEN_PIXELS {
                    continue;
                }
                at_screen = Some(q);
                if off_to_one_side(p, q, view) {
                    flush(&mut line_points);
                    continue;
                }
                if line_points.is_empty() {
                    line_points.push(p);
                }
                line_points.push(q);
                let dir = d / len;
                let side = vec2(-dir.y, dir.x);
                let mut along = run;
                while along <= len {
                    let at = p + dir * along;
                    // Passes stacked at the same place share their arrows.
                    if drawn.insert(((at.x / (SPACING * 0.6)) as i32, (at.y / (SPACING * 0.6)) as i32)) {
                        painter.line_segment([at - dir * 5.0 + side * 3.5, at], arrow);
                        painter.line_segment([at - dir * 5.0 - side * 3.5, at], arrow);
                    }
                    along += SPACING;
                }
                run = along - len;
            }
            flush(&mut line_points);
            if let Some(i) = tp.moves.iter().position(|m| !m.rapid) {
                let start = tp.moves[i.saturating_sub(1)].p;
                let r = Rect::from_center_size(self.to_screen([start[0], start[1]]), vec2(8.0, 8.0));
                painter.rect_filled(r, 0.0, Color32::from_rgb(46, 160, 72));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::SHEET_INK), StrokeKind::Inside);
            }
        }
    }

    fn paint_model_outline(&self, painter: &egui::Painter, _model: &crate::relief::Grid) {
        let line = Stroke::new(1.0, Color32::from_rgb(120, 160, 110));
        for c in self.doc.components.iter().filter(|c| c.visible) {
            for p in &c.paths {
                let mut pts: Vec<Pos2> = p.pts.iter().map(|q| self.to_screen(*q)).collect();
                if p.closed && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                painter.add(Shape::line(pts, line));
            }
        }
    }

    fn paint_rulers(&self, ui: &egui::Ui, full: Rect, sheet: Rect) {
        let painter = ui.painter_at(full);
        // On the bottom side of a two-sided job the rulers turn orange, as the original's do
        // (its Bottom Side Ruler Colour, `secondaryRulerColour`, `0x64C2ED` at `0x142e6100c`).
        let bottom = self.doc.job.two_sided && self.doc.active_face == 1;
        let bg = if bottom { theme::BOTTOM_RULER } else { theme::CHROME };
        let top = Rect::from_min_max(pos2(sheet.left(), full.top()), pos2(sheet.right(), sheet.top()));
        let left = Rect::from_min_max(pos2(full.left(), sheet.top()), pos2(sheet.left(), sheet.bottom()));
        painter.rect_filled(top, 0.0, bg);
        painter.rect_filled(left, 0.0, bg);
        let corner = Rect::from_min_max(full.min, sheet.min);
        painter.rect_filled(corner, 0.0, theme::CHROME);
        painter.rect_stroke(corner.shrink(4.0), 0.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
        let edge = Stroke::new(1.0, theme::LINE);
        painter.line_segment([top.left_bottom(), top.right_bottom()], edge);
        painter.line_segment([left.right_top(), left.right_bottom()], edge);

        let major = nice_step(170.0 / self.scale as f64);
        let minor = major / 16.0;
        let tick = Stroke::new(1.0, if bottom { theme::SHEET_INK } else { theme::FAINT });
        let font = FontId::proportional(12.0);
        let text_c = if bottom { theme::SHEET_INK } else { theme::MUTED };
        let tick_len = |i: i64| -> f32 {
            if i % 16 == 0 { 10.0 } else if i % 8 == 0 { 8.0 } else if i % 4 == 0 { 6.0 } else if i % 2 == 0 { 4.0 } else { 2.5 }
        };

        // The numbers count from the job's origin.
        let zero = self.doc.job.origin();
        let (x0, x1) = (self.to_world(top.left_bottom())[0] - zero[0], self.to_world(top.right_bottom())[0] - zero[0]);
        let mut i = (x0 / minor).floor() as i64;
        while i as f64 * minor <= x1 {
            let x = i as f64 * minor;
            let sx = self.to_screen([x + zero[0], 0.0]).x;
            if self.scale as f64 * minor >= 3.0 || i % 16 == 0 {
                painter.line_segment([pos2(sx, top.bottom() - tick_len(i)), pos2(sx, top.bottom())], tick);
            }
            if i % 16 == 0 {
                painter.text(pos2(sx + 2.0, top.top() + 1.0), Align2::LEFT_TOP, format!("{x:.1}"), font.clone(), text_c);
            }
            i += 1;
        }
        let (y0, y1) = (self.to_world(left.right_bottom())[1] - zero[1], self.to_world(left.right_top())[1] - zero[1]);
        let mut i = (y0 / minor).floor() as i64;
        while i as f64 * minor <= y1 {
            let y = i as f64 * minor;
            let sy = self.to_screen([0.0, y + zero[1]]).y;
            if self.scale as f64 * minor >= 3.0 || i % 16 == 0 {
                painter.line_segment([pos2(left.right() - tick_len(i), sy), pos2(left.right(), sy)], tick);
            }
            if i % 16 == 0 {
                let galley = painter.layout_no_wrap(format!("{y:.1}"), font.clone(), text_c);
                let at = pos2(left.left() + 2.0, sy - 2.0);
                painter.add(egui::epaint::TextShape::new(at, galley, text_c).with_angle(-std::f32::consts::FRAC_PI_2));
            }
            i += 1;
        }
        if let Some(c) = self.cursor {
            let p = self.to_screen(c);
            let mark = Stroke::new(1.0, theme::ACCENT);
            painter.line_segment([pos2(p.x, top.top()), pos2(p.x, top.bottom())], mark);
            painter.line_segment([pos2(left.left(), p.y), pos2(left.right(), p.y)], mark);
        }
    }

    fn scrollbars(&mut self, ui: &mut egui::Ui, sheet: Rect) {
        let (w, h) = (self.doc.job.width as f32 * self.scale, self.doc.job.height as f32 * self.scale);
        // The scrollable range: the sheet plus one view on each side.
        let bar_h = Rect::from_min_max(pos2(sheet.left(), sheet.bottom()), pos2(sheet.right(), sheet.bottom() + SCROLL));
        let total_x = w + 2.0 * sheet.width();
        let thumb_w = (sheet.width() / total_x * bar_h.width() * 1.6).clamp(30.0, bar_h.width());
        let pos_x = (sheet.left() - self.origin.x + sheet.width()) / total_x;
        let bar_v = Rect::from_min_max(pos2(sheet.right(), sheet.top()), pos2(sheet.right() + SCROLL, sheet.bottom()));
        let total_y = h + 2.0 * sheet.height();
        let thumb_h = (sheet.height() / total_y * bar_v.height() * 1.6).clamp(30.0, bar_v.height());
        let pos_y = (self.origin.y - h - sheet.top() + sheet.height()) / total_y;

        let painter = ui.painter();
        let trough = theme::CHROME;
        let thumb_fill = theme::BUTTON;
        let thumb_line = Stroke::NONE;
        for bar in [bar_h, bar_v] {
            painter.rect_filled(bar, 0.0, trough);
        }
        let h_thumb = Rect::from_min_size(
            pos2(bar_h.left() + 16.0 + (bar_h.width() - 32.0 - thumb_w) * pos_x.clamp(0.0, 1.0), bar_h.top() + 1.0),
            vec2(thumb_w, SCROLL - 2.0),
        );
        let v_thumb = Rect::from_min_size(
            pos2(bar_v.left() + 1.0, bar_v.top() + 16.0 + (bar_v.height() - 32.0 - thumb_h) * (1.0 - pos_y.clamp(0.0, 1.0))),
            vec2(SCROLL - 2.0, thumb_h),
        );
        for t in [h_thumb, v_thumb] {
            painter.rect(t, 4.0, thumb_fill, thumb_line, StrokeKind::Inside);
        }
        let arrow = theme::MUTED;
        let tri = |c: Pos2, dir: egui::Vec2| {
            let side = vec2(-dir.y, dir.x);
            Shape::convex_polygon(vec![c + dir * 3.0, c - dir * 2.0 + side * 3.0, c - dir * 2.0 - side * 3.0], arrow, Stroke::NONE)
        };
        let arrows = [
            (Rect::from_min_size(bar_h.left_top(), vec2(16.0, SCROLL)), vec2(-1.0, 0.0)),
            (Rect::from_min_size(bar_h.right_top() - vec2(16.0, 0.0), vec2(16.0, SCROLL)), vec2(1.0, 0.0)),
            (Rect::from_min_size(bar_v.left_top(), vec2(SCROLL, 16.0)), vec2(0.0, -1.0)),
            (Rect::from_min_size(bar_v.left_bottom() - vec2(0.0, 16.0), vec2(SCROLL, 16.0)), vec2(0.0, 1.0)),
        ];
        let mut shift = egui::Vec2::ZERO;
        for (i, (r, dir)) in arrows.iter().enumerate() {
            painter.add(tri(r.center(), *dir));
            if ui.interact(*r, ui.scope_id().with(("arrow", i)), Sense::click()).clicked() {
                shift -= *dir * 40.0;
            }
        }
        let h_resp = ui.interact(bar_h.shrink2(vec2(16.0, 0.0)), ui.scope_id().with("hbar"), Sense::drag());
        if h_resp.dragged() {
            shift.x -= h_resp.drag_delta().x * total_x / (bar_h.width() - 32.0 - thumb_w).max(1.0);
        }
        let v_resp = ui.interact(bar_v.shrink2(vec2(0.0, 16.0)), ui.scope_id().with("vbar"), Sense::drag());
        if v_resp.dragged() {
            shift.y -= v_resp.drag_delta().y * total_y / (bar_v.height() - 32.0 - thumb_h).max(1.0);
        }
        if shift != egui::Vec2::ZERO {
            self.origin += shift;
            self.auto_fit = false;
        }
    }
}

/// The next round number (1, 2 or 5 times a power of ten) at or above `v`.
pub fn nice_step(v: f64) -> f64 {
    let p = 10f64.powf(v.max(1e-9).log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= v {
            return m * p;
        }
    }
    10.0 * p
}

/// Moves shorter than this on the screen don't show: they are joined to the next.
pub(crate) const MOST_UNSEEN_PIXELS: f32 = 1.5;

/// Whether a line can't cross the view, both its ends being beyond the same edge.
pub(crate) fn off_to_one_side(p: Pos2, q: Pos2, view: Rect) -> bool {
    (p.x < view.left() && q.x < view.left())
        || (p.x > view.right() && q.x > view.right())
        || (p.y < view.top() && q.y < view.top())
        || (p.y > view.bottom() && q.y > view.bottom())
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn a_line_is_only_left_out_when_it_cannot_cross_the_view() {
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        assert!(off_to_one_side(pos2(-5.0, 10.0), pos2(-1.0, 90.0), view));
        assert!(off_to_one_side(pos2(110.0, 10.0), pos2(120.0, 90.0), view));
        assert!(off_to_one_side(pos2(10.0, -5.0), pos2(90.0, -1.0), view));
        assert!(off_to_one_side(pos2(10.0, 105.0), pos2(90.0, 101.0), view));
        assert!(!off_to_one_side(pos2(10.0, 10.0), pos2(20.0, 20.0), view), "inside");
        assert!(!off_to_one_side(pos2(-50.0, 50.0), pos2(150.0, 50.0), view), "right across, with both ends outside");
    }
}

/// What can be typed while dragging: a number, or two separated by a comma.
pub(crate) fn typed_numbers(text: &str) -> Option<Vec<f64>> {
    let values: Option<Vec<f64>> = text.split([',', ';']).map(|t| t.trim().parse::<f64>().ok()).collect();
    values.filter(|v| (1..=2).contains(&v.len()))
}

impl App {
    /// Puts the shapes back as they were, then turns them about `centre`.
    pub(crate) fn turn_from(&mut self, orig: &[(u64, Vec<Path>)], centre: P, degrees: f64) {
        let (s, k) = degrees.to_radians().sin_cos();
        for (id, paths) in orig {
            if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == *id) {
                v.paths = paths.clone();
                for q in v.paths.iter_mut().flat_map(|q| q.pts.iter_mut()) {
                    let (x, y) = (q[0] - centre[0], q[1] - centre[1]);
                    *q = [centre[0] + x * k - y * s, centre[1] + x * s + y * k];
                }
                v.prim = Prim::Free;
            }
        }
    }

    /// While dragging with the select tool, a value can be typed and Enter makes it exact, as the
    /// original allows (`0x140a751e0`): a distance along the drag or `x,y` for a move, an angle for a
    /// turn, a percentage for a resize. Escape forgets what was typed. True when Enter finished the drag.
    pub(crate) fn typed_value(&mut self, ctx: &egui::Context) -> bool {
        if !matches!(self.drag, Drag::Move { .. } | Drag::Rotate { .. } | Drag::Scale { .. } | Drag::Node { .. }) {
            self.typed.clear();
            return false;
        }
        let (text, enter, escape, back) = ctx.input(|i| {
            let text: String = i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect();
            (text, i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Backspace))
        });
        self.typed.extend(text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | ',' | ';')));
        if back {
            self.typed.pop();
        }
        if escape {
            self.typed.clear();
        }
        if !enter || self.typed.is_empty() {
            return false;
        }
        let Some(values) = typed_numbers(&self.typed) else {
            self.status = tf!("Can't read {} as a value.", self.typed);
            self.typed.clear();
            return false;
        };
        match self.drag.clone() {
            Drag::Move { last, start } => {
                let so_far = [last[0] - start[0], last[1] - start[1]];
                let wanted = match values.as_slice() {
                    [x, y] => [*x, *y],
                    [d] => {
                        let len = so_far[0].hypot(so_far[1]);
                        if len > 1e-12 { [so_far[0] / len * d, so_far[1] / len * d] } else { [*d, 0.0] }
                    }
                    _ => so_far,
                };
                self.translate_selected(wanted[0] - so_far[0], wanted[1] - so_far[1]);
                self.status = tf!("Moved {}, {}", crate::toolpath::trim(wanted[0]), crate::toolpath::trim(wanted[1]));
            }
            Drag::Rotate { centre, orig, .. } => {
                self.turn_from(&orig, centre, values[0]);
                self.status = tf!("Turned {:.1} degrees", values[0]);
            }
            Drag::Node { id, path, index, start } => {
                // One value: that far from where it was, towards the mouse; two: by that much each way.
                let now = self.doc.vectors.iter().find(|v| v.id == id).map_or(start, |v| v.paths[path].pts[index]);
                let wanted = match values.as_slice() {
                    [x, y] => [start[0] + x, start[1] + y],
                    [d] => {
                        let (dx, dy) = (now[0] - start[0], now[1] - start[1]);
                        let len = dx.hypot(dy);
                        if len > 1e-12 { [start[0] + dx / len * d, start[1] + dy / len * d] } else { [start[0] + d, start[1]] }
                    }
                    _ => now,
                };
                if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == id) {
                    v.paths[path].pts[index] = wanted;
                    v.prim = Prim::Free;
                }
                self.status = tf!("Moved the point to {}, {}", crate::toolpath::trim(wanted[0]), crate::toolpath::trim(wanted[1]));
            }
            Drag::Scale { anchor, orig, .. } => {
                let (sx, sy) = match values.as_slice() {
                    [x, y] => (x / 100.0, y / 100.0),
                    [p] => (p / 100.0, p / 100.0),
                    _ => (1.0, 1.0),
                };
                for (id, paths) in &orig {
                    if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == *id) {
                        v.paths = paths.clone();
                        for q in v.paths.iter_mut().flat_map(|q| q.pts.iter_mut()) {
                            *q = [anchor[0] + (q[0] - anchor[0]) * sx, anchor[1] + (q[1] - anchor[1]) * sy];
                        }
                        v.prim = Prim::Free;
                    }
                }
                self.status = tf!("Resized to {}%", crate::toolpath::trim(sx * 100.0));
            }
            _ => {}
        }
        self.typed.clear();
        self.drag = Drag::None;
        true
    }
}

/// How far each resizing handle sits out from the selection's box on screen, in the order of
/// `handle_points` (the sheet's Y goes up, the screen's down): the original draws them
/// `HANDLE_GAP` pixels out from the box.
const HANDLE_GAP: f32 = 8.0;
const HANDLE_OUT: [Vec2; 8] = [
    Vec2::new(-HANDLE_GAP, HANDLE_GAP),
    Vec2::new(HANDLE_GAP, HANDLE_GAP),
    Vec2::new(HANDLE_GAP, -HANDLE_GAP),
    Vec2::new(-HANDLE_GAP, -HANDLE_GAP),
    Vec2::new(0.0, HANDLE_GAP),
    Vec2::new(HANDLE_GAP, 0.0),
    Vec2::new(0.0, -HANDLE_GAP),
    Vec2::new(-HANDLE_GAP, 0.0),
];

/// How near a gap between letters a click has to be, in pixels on screen.
const TEXT_GAP_REACH: f32 = 4.0;
