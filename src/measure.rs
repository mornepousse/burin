//! Measure and inspect: the original's form has three choices one under the other (between two
//! points, a span and its contour, a section of the model) with every value as a line of text.
//! Burin's own layout: the three as pictured tabs at the top, the main result written large, the
//! others in small tiles, a little picture of what is measured, and the points as X/Y pairs.

use crate::app::{App, Prim, Tool2};
use crate::geom::{self, Path, P};
use crate::i18n::tr;
use crate::icons::Icon;
use crate::ui::{card, icon_caps, pictured_pills, Pic};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui};

/// What the measure tool is doing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MeasureMode {
    /// The distance between two points.
    #[default]
    Points,
    /// A piece of a shape (line, arc or curve) and the whole outline it belongs to.
    Shape,
    /// The model's height along a line, drawn as a new shape.
    Section,
}

/// A piece of an outline between two corners: a straight line, an arc of one circle, or a curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    pub pts: Vec<P>,
    pub length: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PieceKind {
    Line,
    Arc { c: P, r: f64 },
    Curve,
}

/// The shape clicked with the measure tool: which outline of which shape, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspected {
    pub vector: u64,
    pub path: usize,
    pub edge: usize,
}

/// How much an outline may turn at a point and still be one piece, in degrees: past it, a corner.
const CORNER_DEGREES: f64 = 20.0;
/// Below this turn, in degrees, two edges are one straight line.
const STRAIGHT_DEGREES: f64 = 0.5;

/// How far a point may stray from the circle of an arc, in the drawing's units.
const ON_CIRCLE: f64 = 0.01;

/// The piece of `path` that its edge `edge` (from point `edge` to the next) belongs to, found from
/// that edge outwards: an arc while the points stay on one circle; else a straight line, edges
/// going on straight or one edge much longer than its neighbours (a side between two rounded
/// corners); else a curve, between two corners.
pub fn piece_of(path: &Path, edge: usize) -> Option<Piece> {
    let pts = &path.pts;
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let edges = if path.closed { n } else { n - 1 };
    if edge >= edges {
        return None;
    }
    let at = |k: isize| pts[k.rem_euclid(n as isize) as usize];
    let exists = |k: isize| path.closed || (0..edges as isize).contains(&k);
    let len = |k: isize| {
        let (a, b) = (at(k), at(k + 1));
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    // The signed turn, in degrees, from edge k to edge k + 1.
    let turn = |k: isize| -> Option<f64> {
        if !(exists(k) && exists(k + 1)) {
            return None;
        }
        let (a, b, c) = (at(k), at(k + 1), at(k + 2));
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        Some((u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).to_degrees())
    };
    let e = edge as isize;
    let total = edges as isize;
    let piece = |first: isize, last: isize, kind: PieceKind| {
        let run: Vec<P> = (first..=last + 1).map(at).collect();
        let length = run.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        Piece { kind, pts: run, length }
    };
    // Grows a run from the edge while `keep(first, last)` holds, one edge at a time each way.
    let grow = |keep: &dyn Fn(isize, isize) -> bool, from: (isize, isize)| -> (isize, isize) {
        let (mut first, mut last) = from;
        loop {
            let mut moved = false;
            if last - first + 1 < total && exists(last + 1) && keep(first, last + 1) {
                last += 1;
                moved = true;
            }
            if last - first + 1 < total && exists(first - 1) && keep(first - 1, last) {
                first -= 1;
                moved = true;
            }
            if !moved {
                return (first, last);
            }
        }
    };
    // Edges first to last on one circle: through the run's first, middle and last points, each
    // point on it, each edge short against it, all turning the same way.
    let circle_of = |first: isize, last: isize| -> Option<crate::solid::Circle> {
        if last - first < 2 {
            return None;
        }
        let c = crate::solid::circle_through(at(first), at((first + last + 1) / 2), at(last + 1))?;
        let on = (first..=last + 1).all(|k| ((at(k)[0] - c.c[0]).hypot(at(k)[1] - c.c[1]) - c.r).abs() <= ON_CIRCLE);
        // Short against the circle, and alike: an arc is drawn in even steps, where a straight
        // side between two rounded corners is one long edge.
        let lens: Vec<f64> = (first..=last).map(len).collect();
        let (shortest, longest) = lens.iter().fold((f64::MAX, 0.0f64), |(a, b), l| (a.min(*l), b.max(*l)));
        let short = longest <= c.r * 0.35 && longest <= shortest * 3.0;
        let signs: Vec<f64> = (first..last).filter_map(|k| turn(k)).collect();
        let one_way = signs.iter().all(|t| t.signum() == signs[0].signum() && t.abs() > 1e-9);
        (on && short && one_way && c.r < 1e5).then_some(c)
    };
    // An arc needs three edges to be told: a window of three holding the edge, then grown.
    let seed = [(e - 1, e + 1), (e, e + 2), (e - 2, e)].into_iter().find(|&(f, l)| (f..=l).all(exists) && circle_of(f, l).is_some());
    if let Some(from) = seed {
        let (af, al) = grow(&|f, l| circle_of(f, l).is_some(), from);
        if let Some(c) = circle_of(af, al) {
            return Some(piece(af, al, PieceKind::Arc { c: c.c, r: c.r }));
        }
    }
    // Straight on.
    let (sf, sl) = grow(&|f, l| (f..l).all(|k| turn(k).is_some_and(|t| t.abs() < STRAIGHT_DEGREES)), (e, e));
    let neighbours: Vec<f64> = [e - 1, e + 1].into_iter().filter(|k| exists(*k)).map(len).collect();
    // Gently turning on from it on either side: then it may be a step of a curve.
    let gentle = [e - 1, e].into_iter().filter_map(turn).any(|t| t.abs() < CORNER_DEGREES);
    if sl > sf || !gentle || neighbours.iter().all(|l| len(e) >= l * 3.0) {
        return Some(piece(sf, sl, PieceKind::Line));
    }
    // Turning gently one way or the other, up to a corner.
    let (cf, cl) = grow(&|f, l| (f..l).all(|k| turn(k).is_some_and(|t| t.abs() < CORNER_DEGREES)), (e, e));
    Some(piece(cf, cl, PieceKind::Curve))
}

/// The edge of the visible shapes nearest `w`, within `reach`: (shape, outline, edge).
fn edge_near(app: &App, w: P, reach: f64) -> Option<Inspected> {
    let mut best: Option<(f64, Inspected)> = None;
    for v in app.visible_vectors() {
        for (pi, p) in v.paths.iter().enumerate() {
            let n = p.pts.len();
            let edges = if p.closed { n } else { n.saturating_sub(1) };
            for k in 0..edges {
                let d = geom::dist2_seg(w, p.pts[k], p.pts[(k + 1) % n]);
                if d <= reach * reach && best.as_ref().is_none_or(|(old, _)| d < *old) {
                    best = Some((d, Inspected { vector: v.id, path: pi, edge: k }));
                }
            }
        }
    }
    best.map(|(_, i)| i)
}

/// The model's height along the line from `a` to `b`, as an open line drawn on it: each height
/// stands off the line to its left, so the section reads upright when the line goes left to
/// right. Burin's choice of where to draw it; the original's wasn't read.
pub fn section_path(grid: &crate::relief::Grid, a: P, b: P) -> Option<Path> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len < 1e-9 {
        return None;
    }
    let (ux, uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
    let steps = ((len / grid.cell).ceil() as usize).clamp(2, 4000);
    let pts: Vec<P> = (0..=steps)
        .map(|k| {
            let s = len * k as f64 / steps as f64;
            let (x, y) = (a[0] + ux * s, a[1] + uy * s);
            let h = grid.sample(x, y) as f64;
            [x - uy * h, y + ux * h]
        })
        .collect();
    Some(Path::open(pts))
}

impl App {
    /// The measure tool on the drawing: by mode, two points dragged, a shape clicked, or a line
    /// dragged for a section. The pointer's place is kept to show in the panel.
    pub(crate) fn handle_measure(&mut self, response: &egui::Response, pointer: Option<egui::Pos2>, press: Option<egui::Pos2>) {
        if let Some(p) = response.hover_pos().or(pointer) {
            self.measure_cursor = Some(self.snapped(self.to_world(p)));
        }
        if self.measure_mode == MeasureMode::Shape {
            if response.clicked() {
                if let Some(p) = pointer {
                    let reach = (8.0 / self.scale) as f64;
                    self.inspected = edge_near(self, self.to_world(p), reach);
                    if self.inspected.is_none() {
                        self.status = tr("Click on the line of a shape.").into();
                    }
                }
            }
            return;
        }
        // Two clicks, as the original: the first places the first point, the line then follows
        // the pointer, the second click places the other end (the person found holding the button
        // down awkward). Pressing, dragging and letting go far enough away also ends it.
        let now = pointer.map(|p| self.snapped(self.to_world(p)));
        if let (Some((start, _)), Some(now)) = (self.measure.filter(|_| self.measure_pending), now) {
            self.measure = Some((start, now));
            let d = (now[0] - start[0]).hypot(now[1] - start[1]);
            let angle = (now[1] - start[1]).atan2(now[0] - start[0]).to_degrees();
            self.status = tf!("Distance {:.4} {}, angle {angle:.1} degrees", d, self.doc.job.unit());
        }
        // A click without moving is no drag: it counts as a press where it was.
        let press = press.or_else(|| response.clicked().then_some(pointer).flatten());
        if let Some(p) = press {
            let at = self.snapped(self.to_world(p));
            match self.measure.filter(|_| self.measure_pending) {
                Some((start, _)) => self.end_measure(start, at),
                None => {
                    self.measure = Some((at, at));
                    self.measure_pending = true;
                    self.measure_press = Some(p);
                    self.status = tr("Click the second point. Escape starts again.").into();
                }
            }
        }
        if response.drag_stopped() && self.measure_pending {
            let far = matches!((self.measure_press, pointer), (Some(a), Some(b)) if a.distance(b) > 6.0);
            if let (true, Some((start, _)), Some(now)) = (far, self.measure, now) {
                self.end_measure(start, now);
            }
        }
    }

    /// The second point placed: the measure stays shown, and in a section the shape is drawn.
    fn end_measure(&mut self, start: P, end: P) {
        self.measure = Some((start, end));
        self.measure_pending = false;
        self.measure_press = None;
        let d = (end[0] - start[0]).hypot(end[1] - start[1]);
        let angle = (end[1] - start[1]).atan2(end[0] - start[0]).to_degrees();
        self.status = tf!("Distance {:.4} {}, angle {angle:.1} degrees", d, self.doc.job.unit());
        if self.measure_mode == MeasureMode::Section {
            self.make_section(start, end);
        }
    }

    /// Draws the model's section between two points as a new shape, chosen.
    fn make_section(&mut self, a: P, b: P) {
        if self.model_dirty {
            self.refresh_model();
        }
        let Some(grid) = self.model.as_ref() else {
            self.status = tr("There is no 3D model to cut through.").into();
            return;
        };
        let Some(path) = section_path(grid, a, b) else { return };
        let high = path.pts.iter().map(|q| geom::dist2_seg(*q, a, b).sqrt()).fold(0.0, f64::max);
        self.checkpoint();
        let id = self.doc.add_vector(vec![path], Prim::Free);
        self.selected = vec![id];
        self.status = tf!("Drew the model's section: {} long, {} high at most.", crate::toolpath::trim((b[0] - a[0]).hypot(b[1] - a[1])), crate::toolpath::trim(high));
    }

    /// What the measure tool shows on the drawing: the line between the points, or the piece
    /// clicked, thick, with its ends and its centre.
    pub(crate) fn paint_measure(&self, painter: &egui::Painter) {
        if self.tool != Tool2::Measure {
            return;
        }
        let red = Color32::from_rgb(210, 40, 40);
        // Where the pointer's point lands: a smart catch draws its own mark (`paint_snap`); on the
        // grid or free, a small crosshair.
        if self.measure_mode != MeasureMode::Shape {
            if let Some(c) = self.measure_cursor.filter(|_| self.cursor.is_some_and(|w| self.smart_snap(w).is_none())) {
                let at = self.to_screen(c);
                let ink = Stroke::new(1.2, red);
                painter.line_segment([at - vec2(9.0, 0.0), at - vec2(3.0, 0.0)], ink);
                painter.line_segment([at + vec2(3.0, 0.0), at + vec2(9.0, 0.0)], ink);
                painter.line_segment([at - vec2(0.0, 9.0), at - vec2(0.0, 3.0)], ink);
                painter.line_segment([at + vec2(0.0, 3.0), at + vec2(0.0, 9.0)], ink);
                painter.circle_filled(at, 1.6, red);
            }
        } else if let Some(w) = self.cursor {
            // The piece a click would take, lit faintly under the pointer.
            let reach = (8.0 / self.scale) as f64;
            let hover = edge_near(self, w, reach).filter(|h| Some(h) != self.inspected.as_ref());
            if let Some(piece) = hover.and_then(|h| self.doc.vectors.iter().find(|v| v.id == h.vector).and_then(|v| piece_of(v.paths.get(h.path)?, h.edge))) {
                let pts: Vec<egui::Pos2> = piece.pts.iter().map(|q| self.to_screen(*q)).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(2.5, crate::theme::ACCENT.gamma_multiply(0.55))));
            }
        }
        match self.measure_mode {
            MeasureMode::Points | MeasureMode::Section => {
                if let Some((a, b)) = self.measure {
                    let (pa, pb) = (self.to_screen(a), self.to_screen(b));
                    painter.line_segment([pa, pb], Stroke::new(1.5, red));
                    if self.measure_mode == MeasureMode::Points {
                        // The two legs, faint: across then up.
                        let corner = self.to_screen([b[0], a[1]]);
                        let faint = Stroke::new(1.0, Color32::from_rgba_unmultiplied(210, 40, 40, 110));
                        painter.line_segment([pa, corner], faint);
                        painter.line_segment([corner, pb], faint);
                    }
                    for q in [pa, pb] {
                        painter.circle_stroke(q, 3.5, Stroke::new(1.5, red));
                    }
                    let d = (b[0] - a[0]).hypot(b[1] - a[1]);
                    painter.text(pb + vec2(8.0, 8.0), Align2::LEFT_TOP, format!("{d:.4}"), FontId::proportional(12.0), Color32::from_rgb(170, 20, 20));
                }
            }
            MeasureMode::Shape => {
                if let Some(piece) = self.inspected_piece() {
                    let pts: Vec<egui::Pos2> = piece.pts.iter().map(|q| self.to_screen(*q)).collect();
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0, crate::theme::ACCENT)));
                    for q in [pts[0], pts[pts.len() - 1]] {
                        painter.circle_filled(q, 3.5, crate::theme::ACCENT);
                    }
                    if let PieceKind::Arc { c, .. } = piece.kind {
                        let cc = self.to_screen(c);
                        painter.line_segment([cc - vec2(5.0, 0.0), cc + vec2(5.0, 0.0)], Stroke::new(1.5, crate::theme::ACCENT));
                        painter.line_segment([cc - vec2(0.0, 5.0), cc + vec2(0.0, 5.0)], Stroke::new(1.5, crate::theme::ACCENT));
                    }
                }
            }
        }
    }

    /// The piece clicked, if its shape is still there.
    pub fn inspected_piece(&self) -> Option<Piece> {
        let i = self.inspected.as_ref()?;
        let v = self.doc.vectors.iter().find(|v| v.id == i.vector)?;
        piece_of(v.paths.get(i.path)?, i.edge)
    }

    /// The panel of the measure tool, in the left panel while the tool is on.
    pub(crate) fn measure_panel(&mut self, ui: &mut Ui) {
        let unit = self.doc.job.unit();
        let zero = self.doc.job.origin();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            crate::icons::paint(ui.painter(), r, Icon::Measure, true);
            crate::ui::bold_sized(ui, tr("Measure and inspect"), 15.0, crate::theme::TEXT());
        });
        let modes = [MeasureMode::Points, MeasureMode::Shape, MeasureMode::Section];
        let at = modes.iter().position(|m| *m == self.measure_mode).unwrap_or(0);
        let choices = [(Some(Pic::Icon(Icon::Measure)), tr("Two points")), (Some(Pic::Icon(Icon::Select)), tr("A shape")), (Some(Pic::Icon(Icon::ShowModel)), tr("Model section"))];
        if let Some(k) = pictured_pills(ui, &choices, at) {
            self.measure_mode = modes[k];
        }
        ui.add_space(2.0);
        match self.measure_mode {
            MeasureMode::Points => self.points_panel(ui, unit, zero),
            MeasureMode::Shape => self.shape_panel(ui, unit, zero),
            MeasureMode::Section => {
                card(ui, |ui| {
                    hint(ui, tr("Click two points across the model: its height along the line is drawn as a new shape, standing on the line."));
                    if self.model.is_none() && !self.model_dirty {
                        hint(ui, tr("There is no 3D model to cut through."));
                    }
                });
            }
        }
        if let Some(c) = self.measure_cursor {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Pointer")).size(11.0).color(crate::theme::MUTED()));
                ui.label(RichText::new(format!("X {}   Y {}", fmt(c[0] - zero[0]), fmt(c[1] - zero[1]))).size(11.0).monospace());
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if crate::ui::wide(ui, tr("Close"), 74.0).clicked() {
                self.tool = Tool2::Select;
                self.measure = None;
                self.inspected = None;
            }
        });
    }

    fn points_panel(&mut self, ui: &mut Ui, unit: &str, zero: P) {
        let Some((a, b)) = self.measure else {
            card(ui, |ui| hint(ui, tr("Click the first point, then the second. The points catch on shapes and the grid when snapping is on; hold Alt to place them freely.")));
            return;
        };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let d = dx.hypot(dy);
        let angle = dy.atan2(dx).to_degrees();
        card(ui, |ui| {
            // The distance, large, beside a little picture of the two legs.
            ui.horizontal(|ui| {
                legs_picture(ui, dx, dy);
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr("Distance")).size(11.0).color(crate::theme::MUTED()));
                    ui.label(RichText::new(format!("{} {unit}", fmt(d))).size(22.0).strong().color(crate::theme::TEXT()));
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                tile(ui, tr("Angle"), &format!("{angle:.2}°"));
                tile(ui, tr("Across (X)"), &fmt(dx));
                tile(ui, tr("Up (Y)"), &fmt(dy));
            });
        });
        card(ui, |ui| {
            icon_caps(ui, Icon::Move, tr("The two points"));
            pair(ui, tr("From"), [a[0] - zero[0], a[1] - zero[1]]);
            pair(ui, tr("To"), [b[0] - zero[0], b[1] - zero[1]]);
            if crate::ui::worded_button(ui, true, Icon::Copy, tr("Copy the values")).clicked() {
                let text = format!("{} {d:.4} {unit}\t{} {angle:.2}\tX {dx:.4}\tY {dy:.4}", tr("Distance"), tr("Angle"));
                ui.ctx().copy_text(text);
                self.status = tr("The values are on the clipboard.").into();
            }
        });
    }

    fn shape_panel(&mut self, ui: &mut Ui, unit: &str, zero: P) {
        let found = self.inspected.as_ref().and_then(|i| self.doc.vectors.iter().find(|v| v.id == i.vector).map(|v| (v.clone(), i.path)));
        let (Some(piece), Some((v, pi))) = (self.inspected_piece(), found) else {
            card(ui, |ui| hint(ui, tr("Click on the line of a shape: the piece clicked (straight line, arc or curve) and the whole outline are measured.")));
            return;
        };
        let rel = |q: P| [q[0] - zero[0], q[1] - zero[1]];
        card(ui, |ui| {
            let (kind, pic) = match piece.kind {
                PieceKind::Line => (tr("Straight line"), Icon::Lines),
                PieceKind::Arc { .. } => (tr("Arc"), Icon::Arc),
                PieceKind::Curve => (tr("Curve"), Icon::Curve),
            };
            icon_caps(ui, pic, kind);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Length")).size(11.0).color(crate::theme::MUTED()));
                ui.label(RichText::new(format!("{} {unit}", fmt(piece.length))).size(20.0).strong());
            });
            pair(ui, tr("Starts at"), rel(piece.pts[0]));
            pair(ui, tr("Ends at"), rel(piece.pts[piece.pts.len() - 1]));
            if let PieceKind::Arc { c, r } = piece.kind {
                pair(ui, tr("Centre"), rel(c));
                ui.horizontal(|ui| {
                    tile(ui, tr("Radius"), &fmt(r));
                    tile(ui, tr("Diameter"), &fmt(r * 2.0));
                });
            }
        });
        let path = &v.paths[pi];
        let perimeter: f64 = path.segments().map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        let (lo, hi) = geom::bbox(std::slice::from_ref(path)).unwrap_or(([0.0; 2], [0.0; 2]));
        card(ui, |ui| {
            icon_caps(ui, Icon::Select, if path.closed { tr("The outline: closed") } else { tr("The outline: open") });
            ui.horizontal(|ui| {
                if path.closed {
                    tile(ui, tr("Area"), &format!("{} {unit}²", fmt(geom::signed_area(&path.pts).abs())));
                }
                tile(ui, tr("Perimeter"), &fmt(perimeter));
            });
            ui.horizontal(|ui| {
                tile(ui, tr("Width"), &fmt(hi[0] - lo[0]));
                tile(ui, tr("Height"), &fmt(hi[1] - lo[1]));
                tile(ui, tr("Nodes"), &path.pts.len().to_string());
            });
            let layer = self.doc.layers.get(v.layer).map_or(String::new(), |l| l.name.clone());
            let outlines = v.paths.len();
            let mut line = tf!("Layer: {}", layer);
            if outlines > 1 {
                line += &tf!(" · outline {} of {}", pi + 1, outlines);
            }
            if let Prim::Circle { r, .. } = v.prim {
                line += &tf!(" · a circle of diameter {}", fmt(r * 2.0));
            }
            ui.label(RichText::new(line).size(11.0).color(crate::theme::MUTED()));
        });
    }
}

/// A number as the panel shows it: four places, no trailing zeros past one.
fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let t = s.trim_end_matches('0');
    if t.ends_with('.') { format!("{t}0") } else { t.to_string() }
}

fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(crate::theme::MUTED())).wrap());
}

/// A small box with a name over a value.
fn tile(ui: &mut Ui, name: &str, value: &str) {
    egui::Frame::new().fill(crate::theme::INPUT()).corner_radius(6).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(name).size(10.5).color(crate::theme::MUTED()));
            ui.label(RichText::new(value).size(13.0).strong().monospace());
        });
    });
}

/// A point as "name  X …  Y …", counted from the job's zero.
fn pair(ui: &mut Ui, name: &str, q: P) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(70.0, 16.0), egui::Label::new(RichText::new(name).size(11.5).color(crate::theme::MUTED())));
        ui.label(RichText::new(format!("X {}", fmt(q[0]))).monospace());
        ui.label(RichText::new(format!("Y {}", fmt(q[1]))).monospace());
    });
}

/// A little picture of the measure: the line between the points and its two legs, across then up,
/// in the direction they go.
fn legs_picture(ui: &mut Ui, dx: f64, dy: f64) {
    let (r, _) = ui.allocate_exact_size(vec2(64.0, 52.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, crate::theme::INPUT());
    let inner = r.shrink(10.0);
    let big = dx.abs().max(dy.abs()).max(1e-12);
    let (w, h) = ((dx.abs() / big) as f32 * inner.width(), (dy.abs() / big) as f32 * inner.height());
    let a = pos2(if dx >= 0.0 { inner.left() } else { inner.right() }, if dy >= 0.0 { inner.bottom() } else { inner.top() });
    let b = a + vec2(if dx >= 0.0 { w } else { -w }, if dy >= 0.0 { -h } else { h });
    let corner = pos2(b.x, a.y);
    let faint = Stroke::new(1.0, crate::theme::MUTED());
    p.line_segment([a, corner], faint);
    p.line_segment([corner, b], faint);
    p.line_segment([a, b], Stroke::new(2.0, Color32::from_rgb(220, 70, 60)));
    p.circle_filled(a, 2.5, crate::theme::ACCENT);
    p.circle_filled(b, 2.5, crate::theme::ACCENT);
    let _ = Rect::NOTHING;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rectangle with a rounded corner: clicking one of its sides gives a straight line from
    /// corner to corner; clicking the rounded corner gives an arc with its centre and radius.
    #[test]
    fn a_click_finds_the_straight_side_or_the_arc_it_is_on() {
        let path = Path::closed(geom::rect([50.0, 25.0], 100.0, 50.0, 10.0));
        let n = path.pts.len();
        // The edge nearest the middle of the bottom side.
        let k = (0..n).min_by(|&i, &j| geom::dist2_seg([50.0, 0.0], path.pts[i], path.pts[(i + 1) % n]).total_cmp(&geom::dist2_seg([50.0, 0.0], path.pts[j], path.pts[(j + 1) % n]))).unwrap();
        let side = piece_of(&path, k).unwrap();
        assert_eq!(side.kind, PieceKind::Line);
        assert!((side.length - 80.0).abs() < 1e-6, "between the two rounded corners: {}", side.length);
        // An edge on the rounded corner at the bottom right.
        let q = [100.0 - 10.0 + 10.0 * (-45f64).to_radians().cos(), 10.0 + 10.0 * (-45f64).to_radians().sin()];
        let k = (0..n).min_by(|&i, &j| geom::dist2_seg(q, path.pts[i], path.pts[(i + 1) % n]).total_cmp(&geom::dist2_seg(q, path.pts[j], path.pts[(j + 1) % n]))).unwrap();
        let arc = piece_of(&path, k).unwrap();
        let PieceKind::Arc { c, r } = arc.kind else { panic!("an arc: {:?}", arc.kind) };
        assert!((r - 10.0).abs() < 0.02 && (c[0] - 90.0).abs() < 0.02 && (c[1] - 10.0).abs() < 0.02, "{c:?} {r}");
        assert!((arc.length - std::f64::consts::FRAC_PI_2 * 10.0).abs() < 0.1, "a quarter circle: {}", arc.length);
    }

    /// The section of a model: a bump in the middle of the line stands off it by its height.
    #[test]
    fn a_section_stands_the_heights_off_the_line() {
        let mut grid = crate::relief::Grid::new(100.0, 100.0, 10_000, 0.0);
        for y in 0..grid.ny {
            for x in 0..grid.nx {
                let (px, py) = (x as f64 * grid.cell, y as f64 * grid.cell);
                if (px - 50.0).hypot(py - 50.0) < 10.0 {
                    grid.data[y * grid.nx + x] = 5.0;
                }
            }
        }
        let path = section_path(&grid, [10.0, 50.0], [90.0, 50.0]).unwrap();
        let top = path.pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((top - 55.0).abs() < 1e-6, "the bump 5 high above the line: {top}");
        assert!((path.pts[0][1] - 50.0).abs() < 1e-9 && (path.pts[path.pts.len() - 1][0] - 90.0).abs() < 1e-9);
    }
}
