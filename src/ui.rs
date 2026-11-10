//! The window: menus, side panels, forms and the status bar.

use crate::app::*;
use crate::geom::{self, Path};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::keys::{Action, Shortcut};
use crate::relief::{Combine, Profile};
use crate::toolpath::{self, Order, Side};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui};

use crate::theme::{self, ACCENT, CARD, CHROME, INPUT, LINE, MUTED, PANEL, TEXT, WORKSPACE};
pub use crate::theme::setup_style;
pub const SIDE_W: f32 = 248.0;
/// The strip of section icons along the left edge.
pub const RAIL_W: f32 = 44.0;
pub const RIGHT_W: f32 = 282.0;


/// Gives a painted control a name for screen readers and tests.
pub fn label_widget(resp: &egui::Response, name: &str) {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, resp.enabled(), name));
}

/// Dark, heavier text for headings. The built-in font has no bold, so it is drawn twice.
pub fn bold(ui: &mut Ui, text: &str) -> egui::Response {
    bold_sized(ui, text, 12.5, TEXT())
}

pub(crate) fn bold_sized(ui: &mut Ui, text: &str, size: f32, color: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(size), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(1.0, 0.0), Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), color);
    ui.painter().galley(rect.min + vec2(0.7, 0.0), galley, color);
    resp
}

/// A white box grouping related controls, as in the side panels of the pictures.
/// A group of controls, set off by a fine scribed line above it.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = Frame::new().inner_margin(Margin { left: 2, right: 2, top: 8, bottom: 5 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    let r = out.response.rect;
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, LINE()));
    out.inner
}

/// A number box that accepts typing and only changes the value on valid numbers.
/// A position on the sheet, shown and typed as a distance from `zero` (the job's origin).
pub fn number_from(ui: &mut Ui, value: &mut f64, zero: f64, width: f32) -> bool {
    let mut shown = *value - zero;
    let changed = number(ui, &mut shown, width);
    if changed {
        *value = shown + zero;
    }
    changed
}

/// A number box. A sum typed in it is worked out as it's typed (`crate::formula`), with the answer
/// shown beside it, as the original's boxes do.
pub fn number(ui: &mut Ui, value: &mut f64, width: f32) -> bool {
    let mut none = String::new();
    number_box(ui, value, &mut none, false, width)
}

/// The same, keeping the sum it was typed as in `formula`, and showing it rather than its answer, so it
/// can be worked out again when the document's variables change. A plain number clears it.
pub fn number_formula(ui: &mut Ui, value: &mut f64, formula: &mut String, width: f32) -> bool {
    number_box(ui, value, formula, true, width)
}

fn number_box(ui: &mut Ui, value: &mut f64, formula: &mut String, keep: bool, width: f32) -> bool {
    let id = ui.next_auto_id();
    let resting = if keep && !formula.is_empty() { formula.clone() } else { toolpath::trim(*value) };
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or(resting);
    let resp = ui.add(TextEdit::singleline(&mut text).id(id).desired_width(width).margin(vec2(3.0, 1.0)));
    let context = crate::formula::context();
    let unit = if context.metric { "mm" } else { "in" };
    let mut changed = false;
    if resp.has_focus() {
        let typed = text.trim();
        if crate::formula::is_formula(typed) {
            match crate::formula::evaluate(typed, &context) {
                Ok(v) => {
                    if v != *value {
                        *value = v;
                        changed = true;
                    }
                    if keep && formula.as_str() != typed {
                        *formula = typed.to_string();
                        changed = true;
                    }
                    resp.show_tooltip_text(crate::formula::shown(typed, v, unit, context.metric));
                }
                Err(e) => resp.show_tooltip_text(e.text()),
            }
        } else if let Ok(v) = typed.parse::<f64>() {
            if v.is_finite() && v != *value {
                *value = v;
                changed = true;
            }
            if keep && !formula.is_empty() {
                formula.clear();
                changed = true;
            }
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
        if keep && !formula.is_empty() {
            resp.on_hover_text(crate::formula::shown(formula, *value, unit, context.metric));
        }
    }
    changed
}

pub(crate) fn count_box(ui: &mut Ui, value: &mut usize, width: f32) -> bool {
    let id = ui.next_auto_id();
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let resp = ui.add(TextEdit::singleline(&mut text).id(id).desired_width(width).margin(vec2(3.0, 1.0)));
    let mut changed = false;
    if resp.has_focus() {
        if let Ok(v) = text.trim().parse::<usize>() {
            changed = v != *value;
            *value = v;
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    changed
}

/// Like `labelled`, for a position on the sheet: shown and typed as a distance from `zero`.
pub(crate) fn labelled_from(ui: &mut Ui, label: &str, label_w: f32, value: &mut f64, zero: f64, width: f32, unit: &str) -> bool {
    let mut shown = *value - zero;
    let changed = labelled(ui, label, label_w, &mut shown, width, unit);
    if changed {
        *value = shown + zero;
    }
    changed
}

/// A labelled number box that keeps the sum it was typed as.
pub(crate) fn labelled_formula(ui: &mut Ui, label: &str, label_w: f32, value: &mut f64, formula: &mut String, width: f32, unit: &str) -> bool {
    ui.horizontal(|ui| {
        let needs = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), TEXT()).size().x + 2.0;
        let (r, _) = ui.allocate_exact_size(vec2(label_w.max(needs), 18.0), Sense::hover());
        ui.painter().text(r.right_center() - vec2(1.0, 0.0), Align2::RIGHT_CENTER, label, FontId::proportional(12.5), TEXT());
        let changed = number_formula(ui, value, formula, width);
        if !unit.is_empty() {
            ui.add(egui::Label::new(unit).wrap_mode(egui::TextWrapMode::Extend));
        }
        changed
    })
    .inner
}

pub(crate) fn labelled(ui: &mut Ui, label: &str, label_w: f32, value: &mut f64, width: f32, unit: &str) -> bool {
    ui.horizontal(|ui| {
        // A wording longer than its place takes the room it needs, rather than running out of the panel.
        let needs = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), TEXT()).size().x + 2.0;
        let (r, _) = ui.allocate_exact_size(vec2(label_w.max(needs), 18.0), Sense::hover());
        ui.painter().text(r.right_center() - vec2(1.0, 0.0), Align2::RIGHT_CENTER, label, FontId::proportional(12.5), TEXT());
        let changed = number(ui, value, width);
        if !unit.is_empty() {
            ui.add(egui::Label::new(unit).wrap_mode(egui::TextWrapMode::Extend));
        }
        changed
    })
    .inner
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, active: bool) -> egui::Response {
    icon_button_saying(ui, icon, tip, tip, enabled, active)
}

/// The same, with more to say when the pointer rests on it than what it's called.
pub fn icon_button_saying(ui: &mut Ui, icon: Icon, name: &str, tip: &str, enabled: bool, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    if resp.clicked() {
        crate::crash::note(name);
    }
    if active {
        ui.painter().rect_filled(rect, 7.0, theme::ACCENT_SOFT());
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, theme::BUTTON());
    }
    icons::paint(ui.painter(), rect.shrink(2.0), icon, enabled);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    resp.on_hover_text(tip)
}

impl App {
    /// A section's title, which folds it away when clicked. Says whether what's under it is showing.
    pub(crate) fn section_open(&mut self, ui: &mut Ui, title: &str) -> bool {
        let mut open = !self.folded.iter().any(|f| f == title);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, theme::CARD());
        }
        // Each family in its own colour, its icons too, until the next section.
        let hue = theme::section_hue(title);
        crate::icons::set_tint(hue);
        let colour = if resp.hovered() { TEXT() } else { hue.unwrap_or(theme::MUTED()) };
        let c = rect.left_center() + vec2(9.0, 0.0);
        let arrow = if open { vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)] } else { vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)] };
        ui.painter().add(egui::Shape::convex_polygon(arrow, colour, Stroke::NONE));
        ui.painter().text(rect.left_center() + vec2(19.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(12.5), colour);
        label_widget(&resp, title);
        if resp.clicked() {
            open = !open;
            self.folded.retain(|f| f != title);
            if !open {
                self.folded.push(title.to_string());
            }
            crate::app::save_folded(&self.folded);
        }
        open
    }
}

/// One of the two big choices on the opening screen.
fn start_tile(ui: &mut Ui, icon: Icon, label: &str, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 92.0), Sense::click());
    ui.painter().rect_filled(rect, 12.0, if resp.hovered() { theme::BUTTON() } else { CARD() });
    icons::paint(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 16.0), vec2(30.0, 30.0)), icon, true);
    ui.painter().text(rect.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, label, FontId::proportional(13.0), TEXT());
    label_widget(&resp, label);
    resp.clicked()
}

/// A row of tool icons at the original's 30 pixel pitch.
pub(crate) fn icon_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
        add(ui);
    });
}

pub(crate) fn title_card(ui: &mut Ui, icon: Icon, title: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, true);
        ui.add_space(4.0);
        // A long title gives way rather than pushing the form wider than its panel.
        ui.add(egui::Label::new(RichText::new(title).size(15.0).strong().color(TEXT())).truncate());
    });
}


/// A flat tab: the chosen one is lifted onto a card with an accent underline.
fn side_tab(ui: &Ui, rect: Rect, label: &str, active: bool) -> egui::Response {
    let resp = ui.interact(rect, ui.make_persistent_id(("tab", label)), Sense::click());
    if active {
        ui.painter().rect_filled(rect, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, CARD());
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(2.0, ACCENT));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, theme::BUTTON());
    }
    let colour = if active || resp.hovered() { TEXT() } else { MUTED() };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.5), colour);
    label_widget(&resp, label);
    resp
}

/// The button that does the job of a form: filled in amber, one to a form.
pub(crate) fn primary(ui: &mut Ui, label: &str, width: f32) -> egui::Response {
    ui.add_sized([width, 24.0], egui::Button::new(RichText::new(label).color(theme::ON_ACCENT).strong()).fill(ACCENT))
}

/// A wide button.
pub(crate) fn wide(ui: &mut Ui, label: &str, width: f32) -> egui::Response {
    ui.add_sized([width, 22.0], egui::Button::new(label))
}

/// A small muted heading in capitals, for a row of controls.
pub(crate) fn small_caps(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(10.5).color(MUTED()).strong());
}

/// A small heading with its icon, so a section says what it is about at a glance.
pub(crate) fn icon_caps(ui: &mut Ui, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, true);
        small_caps(ui, text);
    });
}

/// A small button with an icon before its words.
pub(crate) fn worded_button(ui: &mut Ui, enabled: bool, icon: Icon, label: &str) -> egui::Response {
    let font = FontId::proportional(12.0);
    let colour = if enabled { TEXT() } else { MUTED() };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, colour);
    let (r, resp) = ui.allocate_exact_size(vec2(16.0 + 14.0 + galley.size().x + 6.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if enabled && resp.hovered() { theme::BUTTON_HOVER() } else { theme::BUTTON() };
    ui.painter().rect_filled(r, 6.0, fill);
    icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(5.0, 3.0), vec2(16.0, 16.0)), icon, enabled);
    ui.painter().galley(pos2(r.min.x + 25.0, r.center().y - galley.size().y / 2.0), galley, colour);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    resp
}

/// A small picture that shows what a choice does, drawn in a pill beside its words.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Pic {
    Icon(Icon),
    SideOutside,
    SideInside,
    SideOn,
    Climb,
    Conventional,
    ClearOffset,
    ClearRaster,
    ClearAdaptive,
    RampSmooth,
    RampZigZag,
    RampSpiral,
    RampDistance,
    RampAngle,
    LeadStraight,
    LeadCircular,
    StartDrawn,
    StartNearest,
    StartPoint,
    OneFace,
    TwoFaces,
    FlipLeftRight,
    FlipTopBottom,
    Radius,
    Diameter,
    MarginNone,
    MarginNormal,
    MarginWide,
    Outline,
    Fill,
    Hatch,
    FluteBoth,
    FluteStart,
    FluteEnd,
    WallFirst,
    WallLast,
    WallNever,
    CutBoard,
    CutOneColour,
    CutByToolpath,
    MoveTo,
    MoveBy,
    OffsetOut,
    OffsetIn,
    AlignLeft,
    AlignCentreX,
    AlignRight,
    AlignTop,
    AlignCentreY,
    AlignBottom,
    AlignCentre,
    SpaceAcross,
    SpaceUpDown,
    GridGap,
    GridOffset,
    TotalAngle,
    StepAngle,
    FlipEdgeLeft,
    FlipEdgeRight,
    FlipEdgeTop,
    FlipEdgeBottom,
    AlongObjects,
    AlongCircles,
    ByDistance,
    ByCount,
    FilletNormal,
    FilletDogBone,
    FilletTBone,
    NestAlongX,
    NestAlongY,
    EnvelopeBox,
    EnvelopeOneCurve,
    EnvelopeTwoCurves,
    TextFollows,
    TextUpright,
    JoinEnds,
    ProfileRound,
    ProfileAngular,
    ProfileFlat,
    CombineAdd,
    CombineSubtract,
    CombineHighest,
    CombineLowest,
    PointAt,
    PointPolar,
    PointStep,
    BrushSmooth,
    BrushSmudge,
    BrushDeposit,
    BrushRemove,
    BrushSwirl,
    BrushBoth,
    BrushOnlyUp,
    BrushOnlyDown,
    DimAlong,
    DimVertical,
    DimHorizontal,
    IssueZero,
    IssueOverlap,
    IssueCross,
}

/// Draws a choice's picture into a square, in the given colour.
pub(crate) fn draw_pic(p: &egui::Painter, r: Rect, pic: Pic, c: Color32) {
    if let Pic::Icon(icon) = pic {
        icons::paint(p, r, icon, true);
        return;
    }
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new(1.3, c);
    let faint = Stroke::new(1.0, MUTED());
    let line = |pts: &[(f32, f32)], st: Stroke| {
        p.add(Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), st));
    };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, st: Stroke| {
        p.rect_stroke(Rect::from_two_pos(at(x0, y0), at(x1, y1)), 1.0, st, StrokeKind::Inside);
    };
    let arc = |cx: f32, cy: f32, rad: f32, from: f32, to: f32, st: Stroke| {
        let n = 16;
        let pts: Vec<egui::Pos2> = (0..=n)
            .map(|i| {
                let t = (from + (to - from) * i as f32 / n as f32).to_radians();
                at(cx + rad * t.cos(), cy - rad * t.sin())
            })
            .collect();
        p.add(Shape::line(pts, st));
    };
    let w = r.width();
    match pic {
        Pic::SideOutside | Pic::SideInside | Pic::SideOn => {
            p.rect_filled(Rect::from_two_pos(at(0.5, 0.15), at(1.0, 0.85)), 1.0, theme::BUTTON());
            line(&[(0.5, 0.1), (0.5, 0.9)], s);
            let x = match pic {
                Pic::SideOutside => 0.27,
                Pic::SideInside => 0.73,
                _ => 0.5,
            };
            p.circle_stroke(at(x, 0.5), w * 0.2, s);
        }
        Pic::Climb | Pic::Conventional => {
            // The cutter turning, and the way it moves along the wall below it.
            line(&[(0.0, 0.95), (1.0, 0.95)], faint);
            p.circle_stroke(at(0.5, 0.45), w * 0.26, faint);
            let (from, to) = if pic == Pic::Climb { (200.0, -40.0) } else { (-20.0, 220.0) };
            arc(0.5, 0.45, 0.26, from, to, s);
            let end = to.to_radians();
            let tip = at(0.5 + 0.26 * end.cos(), 0.45 - 0.26 * end.sin());
            p.circle_filled(tip, 1.8, c);
            line(&[(0.15, 0.82), (0.85, 0.82)], s);
            line(&[(0.72, 0.74), (0.85, 0.82), (0.72, 0.9)], s);
        }
        Pic::ClearOffset => {
            rect(0.05, 0.05, 0.95, 0.95, faint);
            rect(0.2, 0.2, 0.8, 0.8, s);
            rect(0.37, 0.37, 0.63, 0.63, s);
        }
        Pic::ClearRaster => {
            rect(0.05, 0.05, 0.95, 0.95, faint);
            line(&[(0.18, 0.22), (0.82, 0.22), (0.82, 0.4), (0.18, 0.4), (0.18, 0.58), (0.82, 0.58), (0.82, 0.76), (0.18, 0.76)], s);
        }
        Pic::ClearAdaptive => {
            rect(0.05, 0.05, 0.95, 0.95, faint);
            for k in 0..4 {
                let x = 0.25 + k as f32 * 0.17;
                arc(x, 0.5, 0.13, 0.0, 330.0, s);
            }
        }
        Pic::RampSmooth => {
            line(&[(0.0, 0.25), (1.0, 0.25)], faint);
            line(&[(0.08, 0.25), (0.92, 0.85)], s);
        }
        Pic::RampZigZag => {
            line(&[(0.0, 0.2), (1.0, 0.2)], faint);
            line(&[(0.08, 0.2), (0.92, 0.42), (0.08, 0.64), (0.92, 0.86)], s);
        }
        Pic::RampSpiral => {
            arc(0.5, 0.32, 0.38, 0.0, 360.0, faint);
            arc(0.5, 0.5, 0.38, 180.0, 360.0, s);
            arc(0.5, 0.68, 0.38, 0.0, 180.0, s);
        }
        Pic::RampDistance => {
            line(&[(0.05, 0.5), (0.95, 0.5)], s);
            line(&[(0.05, 0.3), (0.05, 0.7)], s);
            line(&[(0.95, 0.3), (0.95, 0.7)], s);
        }
        Pic::RampAngle => {
            line(&[(0.05, 0.85), (0.95, 0.85)], faint);
            line(&[(0.05, 0.85), (0.9, 0.2)], s);
            arc(0.05, 0.85, 0.45, 0.0, 37.0, s);
        }
        Pic::LeadStraight | Pic::LeadCircular => {
            p.rect_filled(Rect::from_two_pos(at(0.68, 0.0), at(1.0, 1.0)), 1.0, theme::BUTTON());
            line(&[(0.68, 0.0), (0.68, 1.0)], faint);
            if pic == Pic::LeadStraight {
                line(&[(0.1, 0.9), (0.68, 0.45)], s);
            } else {
                arc(0.18, 0.45, 0.5, 0.0, -80.0, s);
            }
            p.circle_filled(at(0.68, 0.45), 2.0, c);
        }
        Pic::StartDrawn | Pic::StartNearest | Pic::StartPoint => {
            rect(0.25, 0.25, 0.95, 0.95, faint);
            match pic {
                Pic::StartDrawn => {
                    p.circle_filled(at(0.25, 0.25), 2.4, c);
                }
                Pic::StartNearest => {
                    p.circle_filled(at(0.25, 0.6), 2.4, c);
                    line(&[(0.0, 0.6), (0.2, 0.6)], s);
                }
                _ => {
                    line(&[(0.05, 0.05), (0.2, 0.2)], s);
                    line(&[(0.0, 0.12), (0.12, 0.0)], s);
                    p.circle_filled(at(0.25, 0.25), 2.4, c);
                }
            }
        }
        Pic::OneFace | Pic::TwoFaces => {
            rect(0.05, 0.35, 0.95, 0.65, faint);
            line(&[(0.05, 0.35), (0.95, 0.35)], Stroke::new(2.2, c));
            if pic == Pic::TwoFaces {
                line(&[(0.05, 0.65), (0.95, 0.65)], Stroke::new(2.2, c));
            }
        }
        Pic::FlipLeftRight => {
            rect(0.25, 0.4, 0.75, 0.6, faint);
            arc(0.5, 0.5, 0.42, 160.0, 20.0, s);
            p.circle_filled(at(0.89, 0.36), 1.8, c);
        }
        Pic::FlipTopBottom => {
            rect(0.4, 0.25, 0.6, 0.75, faint);
            arc(0.5, 0.5, 0.42, 110.0, -110.0, s);
            p.circle_filled(at(0.36, 0.89), 1.8, c);
        }
        Pic::Radius | Pic::Diameter => {
            p.circle_stroke(at(0.5, 0.5), w * 0.42, faint);
            if pic == Pic::Radius {
                line(&[(0.5, 0.5), (0.92, 0.5)], s);
                p.circle_filled(at(0.5, 0.5), 1.6, c);
            } else {
                line(&[(0.08, 0.5), (0.92, 0.5)], s);
            }
        }
        Pic::MarginNone | Pic::MarginNormal | Pic::MarginWide => {
            rect(0.02, 0.15, 0.98, 0.85, faint);
            let m = match pic {
                Pic::MarginNone => 0.06,
                Pic::MarginNormal => 0.18,
                _ => 0.3,
            };
            p.rect_filled(Rect::from_two_pos(at(m, 0.35), at(1.0 - m, 0.65)), 1.0, c);
        }
        Pic::Outline => {
            rect(0.15, 0.15, 0.85, 0.85, s);
        }
        Pic::Fill => {
            rect(0.15, 0.15, 0.85, 0.85, s);
            for y in [0.32, 0.5, 0.68] {
                line(&[(0.2, y), (0.8, y)], s);
            }
        }
        Pic::Hatch => {
            rect(0.15, 0.15, 0.85, 0.85, faint);
            line(&[(0.15, 0.55), (0.55, 0.15)], s);
            line(&[(0.15, 0.85), (0.85, 0.15)], s);
            line(&[(0.45, 0.85), (0.85, 0.45)], s);
        }
        Pic::FluteBoth => line(&[(0.0, 0.3), (0.3, 0.75), (0.7, 0.75), (1.0, 0.3)], s),
        Pic::FluteStart => line(&[(0.0, 0.3), (0.3, 0.75), (1.0, 0.75)], s),
        Pic::FluteEnd => line(&[(0.0, 0.75), (0.7, 0.75), (1.0, 0.3)], s),
        Pic::WallFirst | Pic::WallLast | Pic::WallNever => {
            for y in [0.38, 0.62] {
                line(&[(0.22, y), (0.78, y)], faint);
            }
            if pic != Pic::WallNever {
                rect(0.05, 0.12, 0.95, 0.88, s);
                let x = if pic == Pic::WallFirst { 0.05 } else { 0.95 };
                p.circle_filled(at(x, 0.12), 2.2, c);
            } else {
                rect(0.05, 0.12, 0.95, 0.88, faint);
            }
        }
        Pic::CutBoard => {
            p.rect_filled(Rect::from_two_pos(at(0.1, 0.2), at(0.9, 0.8)), 2.0, Color32::from_rgb(214, 160, 104));
        }
        Pic::CutOneColour => {
            p.rect_filled(Rect::from_two_pos(at(0.1, 0.2), at(0.9, 0.8)), 2.0, Color32::from_rgb(230, 230, 230));
        }
        Pic::CutByToolpath => {
            for (k, col) in [Color32::from_rgb(240, 110, 90), Color32::from_rgb(110, 170, 250), Color32::from_rgb(130, 210, 110)].into_iter().enumerate() {
                let x = 0.1 + k as f32 * 0.27;
                p.rect_filled(Rect::from_two_pos(at(x, 0.2), at(x + 0.26, 0.8)), 1.0, col);
            }
        }
        Pic::MoveTo => {
            line(&[(0.1, 0.1), (0.1, 0.9), (0.9, 0.9)], faint);
            rect(0.55, 0.15, 0.85, 0.45, s);
            p.circle_filled(at(0.55, 0.45), 2.0, c);
            line(&[(0.1, 0.9), (0.55, 0.45)], Stroke::new(1.0, c));
        }
        Pic::MoveBy => {
            rect(0.05, 0.55, 0.35, 0.85, faint);
            rect(0.6, 0.15, 0.9, 0.45, s);
            line(&[(0.3, 0.6), (0.6, 0.4)], s);
            line(&[(0.5, 0.4), (0.6, 0.4), (0.57, 0.5)], s);
        }
        Pic::OffsetOut | Pic::OffsetIn => {
            let (inner, outer) = if pic == Pic::OffsetOut { (faint, s) } else { (s, faint) };
            rect(0.3, 0.3, 0.7, 0.7, if pic == Pic::OffsetOut { faint } else { s });
            rect(0.08, 0.08, 0.92, 0.92, outer);
            if pic == Pic::OffsetIn {
                rect(0.08, 0.08, 0.92, 0.92, faint);
                rect(0.3, 0.3, 0.7, 0.7, inner);
            }
        }
        Pic::AlignLeft | Pic::AlignCentreX | Pic::AlignRight => {
            let x = match pic {
                Pic::AlignLeft => 0.1,
                Pic::AlignCentreX => 0.5,
                _ => 0.9,
            };
            line(&[(x, 0.0), (x, 1.0)], faint);
            let (a, b) = match pic {
                Pic::AlignLeft => ((0.1, 0.7), (0.1, 0.5)),
                Pic::AlignCentreX => ((0.15, 0.85), (0.3, 0.7)),
                _ => ((0.2, 0.9), (0.4, 0.9)),
            };
            rect(a.0, 0.12, a.1, 0.42, s);
            rect(b.0, 0.58, b.1, 0.88, s);
        }
        Pic::AlignTop | Pic::AlignCentreY | Pic::AlignBottom => {
            let y = match pic {
                Pic::AlignTop => 0.1,
                Pic::AlignCentreY => 0.5,
                _ => 0.9,
            };
            line(&[(0.0, y), (1.0, y)], faint);
            let (a, b) = match pic {
                Pic::AlignTop => ((0.1, 0.7), (0.1, 0.5)),
                Pic::AlignCentreY => ((0.15, 0.85), (0.3, 0.7)),
                _ => ((0.2, 0.9), (0.4, 0.9)),
            };
            rect(0.12, a.0, 0.42, a.1, s);
            rect(0.58, b.0, 0.88, b.1, s);
        }
        Pic::AlignCentre => {
            line(&[(0.5, 0.0), (0.5, 1.0)], faint);
            line(&[(0.0, 0.5), (1.0, 0.5)], faint);
            rect(0.25, 0.25, 0.75, 0.75, s);
        }
        Pic::SpaceAcross => {
            for x in [0.05, 0.4, 0.75] {
                rect(x, 0.3, x + 0.2, 0.7, s);
            }
        }
        Pic::SpaceUpDown => {
            for y in [0.05, 0.4, 0.75] {
                rect(0.3, y, 0.7, y + 0.2, s);
            }
        }
        Pic::GridGap | Pic::GridOffset => {
            for (x, y) in [(0.05, 0.05), (0.55, 0.05), (0.05, 0.55), (0.55, 0.55)] {
                rect(x, y, x + 0.4, y + 0.4, faint);
            }
            if pic == Pic::GridGap {
                line(&[(0.45, 0.25), (0.55, 0.25)], Stroke::new(2.0, c));
            } else {
                line(&[(0.05, 0.25), (0.55, 0.25)], Stroke::new(2.0, c));
            }
        }
        Pic::TotalAngle | Pic::StepAngle => {
            p.circle_filled(at(0.5, 0.5), 1.6, c);
            if pic == Pic::TotalAngle {
                arc(0.5, 0.5, 0.4, 0.0, 270.0, s);
            } else {
                arc(0.5, 0.5, 0.4, 0.0, 60.0, s);
                arc(0.5, 0.5, 0.4, 60.0, 360.0, faint);
            }
        }
        Pic::FlipEdgeLeft | Pic::FlipEdgeRight | Pic::FlipEdgeTop | Pic::FlipEdgeBottom => {
            rect(0.3, 0.3, 0.7, 0.7, s);
            match pic {
                Pic::FlipEdgeLeft => line(&[(0.3, 0.0), (0.3, 1.0)], Stroke::new(2.0, c)),
                Pic::FlipEdgeRight => line(&[(0.7, 0.0), (0.7, 1.0)], Stroke::new(2.0, c)),
                Pic::FlipEdgeTop => line(&[(0.0, 0.3), (1.0, 0.3)], Stroke::new(2.0, c)),
                _ => line(&[(0.0, 0.7), (1.0, 0.7)], Stroke::new(2.0, c)),
            }
        }
        Pic::AlongObjects | Pic::AlongCircles => {
            arc(0.5, 1.2, 0.85, 145.0, 35.0, faint);
            for (x, y) in [(0.18, 0.62), (0.5, 0.38), (0.82, 0.62)] {
                if pic == Pic::AlongObjects {
                    rect(x - 0.1, y - 0.1, x + 0.1, y + 0.1, s);
                } else {
                    p.circle_stroke(at(x, y), w * 0.1, s);
                }
            }
        }
        Pic::ByDistance => {
            p.circle_filled(at(0.15, 0.5), 2.2, c);
            p.circle_filled(at(0.85, 0.5), 2.2, c);
            line(&[(0.28, 0.5), (0.72, 0.5)], s);
            line(&[(0.28, 0.35), (0.28, 0.65)], s);
            line(&[(0.72, 0.35), (0.72, 0.65)], s);
        }
        Pic::ByCount => {
            for x in [0.12, 0.37, 0.63, 0.88] {
                p.circle_filled(at(x, 0.5), 2.2, c);
            }
        }
        Pic::FilletNormal | Pic::FilletDogBone | Pic::FilletTBone => {
            match pic {
                Pic::FilletNormal => {
                    line(&[(0.1, 0.9), (0.1, 0.45)], s);
                    arc(0.55, 0.45, 0.45, 180.0, 90.0, s);
                    line(&[(0.55, 0.0), (0.95, 0.0)], s);
                }
                Pic::FilletDogBone => {
                    line(&[(0.15, 0.95), (0.15, 0.15), (0.95, 0.15)], faint);
                    p.circle_stroke(at(0.25, 0.25), w * 0.17, s);
                }
                _ => {
                    line(&[(0.15, 0.95), (0.15, 0.15), (0.95, 0.15)], faint);
                    p.circle_stroke(at(0.15, 0.35), w * 0.17, s);
                }
            }
        }
        Pic::NestAlongX | Pic::NestAlongY => {
            rect(0.02, 0.02, 0.98, 0.98, faint);
            let cells: [(f32, f32); 3] = if pic == Pic::NestAlongX { [(0.1, 0.62), (0.4, 0.62), (0.7, 0.62)] } else { [(0.1, 0.62), (0.1, 0.32), (0.1, 0.02)] };
            for (x, y) in cells {
                rect(x + 0.02, y + 0.06, x + 0.24, y + 0.3, s);
            }
            if pic == Pic::NestAlongX {
                line(&[(0.15, 0.4), (0.85, 0.4)], Stroke::new(1.0, c));
            } else {
                line(&[(0.6, 0.85), (0.6, 0.15)], Stroke::new(1.0, c));
            }
        }
        Pic::EnvelopeBox => {
            rect(0.1, 0.25, 0.9, 0.75, s);
            line(&[(0.25, 0.6), (0.4, 0.4), (0.55, 0.6), (0.7, 0.4)], faint);
        }
        Pic::EnvelopeOneCurve => {
            arc(0.5, 1.4, 0.95, 120.0, 60.0, s);
            line(&[(0.25, 0.45), (0.4, 0.25), (0.55, 0.45), (0.7, 0.25)], faint);
        }
        Pic::EnvelopeTwoCurves => {
            arc(0.5, 1.4, 0.95, 120.0, 60.0, s);
            arc(0.5, 1.0, 0.95, 120.0, 60.0, s);
            line(&[(0.3, 0.5), (0.45, 0.35), (0.6, 0.5)], faint);
        }
        Pic::TextFollows | Pic::TextUpright => {
            arc(0.5, 1.3, 0.9, 150.0, 30.0, faint);
            for (x, y, a) in [(0.2, 0.62, 0.5f32), (0.5, 0.42, 0.0), (0.8, 0.62, -0.5)] {
                let t = if pic == Pic::TextFollows { a } else { 0.0 };
                let (ox, oy) = (t.sin() * 0.14, t.cos() * 0.14);
                p.line_segment([at(x, y), at(x + ox, y - oy)], Stroke::new(2.4, c));
            }
        }
        Pic::JoinEnds => {
            line(&[(0.05, 0.75), (0.45, 0.3)], s);
            line(&[(0.55, 0.3), (0.95, 0.75)], s);
            p.circle_stroke(at(0.5, 0.3), w * 0.1, faint);
        }
        Pic::ProfileRound => {
            line(&[(0.0, 0.85), (1.0, 0.85)], faint);
            arc(0.5, 0.85, 0.42, 180.0, 0.0, s);
        }
        Pic::ProfileAngular => {
            line(&[(0.0, 0.85), (1.0, 0.85)], faint);
            line(&[(0.08, 0.85), (0.5, 0.2), (0.92, 0.85)], s);
        }
        Pic::ProfileFlat => {
            line(&[(0.0, 0.85), (1.0, 0.85)], faint);
            line(&[(0.1, 0.85), (0.1, 0.4), (0.9, 0.4), (0.9, 0.85)], s);
        }
        Pic::CombineAdd | Pic::CombineSubtract | Pic::CombineHighest | Pic::CombineLowest => {
            // The model as a low hump, the new shape over it.
            line(&[(0.0, 0.7), (0.3, 0.55), (0.7, 0.55), (1.0, 0.7)], faint);
            match pic {
                Pic::CombineAdd => arc(0.5, 0.55, 0.3, 180.0, 0.0, s),
                Pic::CombineSubtract => arc(0.5, 0.55, 0.3, 180.0, 360.0, s),
                Pic::CombineHighest => line(&[(0.2, 0.55), (0.5, 0.15), (0.8, 0.55)], s),
                _ => line(&[(0.2, 0.55), (0.5, 0.85), (0.8, 0.55)], s),
            }
        }
        Pic::PointAt => {
            line(&[(0.1, 0.1), (0.1, 0.9), (0.9, 0.9)], faint);
            p.circle_filled(at(0.7, 0.3), 2.4, c);
            line(&[(0.1, 0.3), (0.62, 0.3)], Stroke::new(1.0, c));
            line(&[(0.7, 0.9), (0.7, 0.38)], Stroke::new(1.0, c));
        }
        Pic::PointPolar => {
            p.circle_filled(at(0.15, 0.8), 2.0, MUTED());
            line(&[(0.15, 0.8), (0.85, 0.25)], s);
            arc(0.15, 0.8, 0.35, 0.0, 38.0, faint);
        }
        Pic::PointStep => {
            p.circle_filled(at(0.15, 0.8), 2.0, MUTED());
            line(&[(0.15, 0.8), (0.85, 0.8), (0.85, 0.25)], s);
        }
        Pic::BrushSmooth => {
            line(&[(0.0, 0.5), (0.2, 0.3), (0.35, 0.7), (0.5, 0.5)], faint);
            line(&[(0.5, 0.55), (1.0, 0.55)], s);
        }
        Pic::BrushSmudge => {
            line(&[(0.05, 0.7), (0.4, 0.7)], faint);
            line(&[(0.3, 0.45), (0.9, 0.45)], s);
            line(&[(0.75, 0.32), (0.9, 0.45), (0.75, 0.58)], s);
        }
        Pic::BrushDeposit | Pic::BrushRemove => {
            line(&[(0.0, 0.8), (1.0, 0.8)], faint);
            if pic == Pic::BrushDeposit {
                arc(0.5, 0.8, 0.3, 180.0, 0.0, s);
                line(&[(0.5, 0.35), (0.5, 0.05)], s);
                line(&[(0.38, 0.17), (0.5, 0.05), (0.62, 0.17)], s);
            } else {
                arc(0.5, 0.45, 0.3, 180.0, 360.0, s);
                line(&[(0.5, 0.05), (0.5, 0.35)], s);
                line(&[(0.38, 0.23), (0.5, 0.35), (0.62, 0.23)], s);
            }
        }
        Pic::BrushSwirl => {
            arc(0.5, 0.5, 0.4, 0.0, 300.0, s);
            arc(0.5, 0.5, 0.2, 60.0, 330.0, s);
        }
        Pic::BrushBoth | Pic::BrushOnlyUp | Pic::BrushOnlyDown => {
            line(&[(0.0, 0.5), (1.0, 0.5)], faint);
            if pic != Pic::BrushOnlyDown {
                line(&[(0.3, 0.42), (0.3, 0.1)], s);
                line(&[(0.18, 0.22), (0.3, 0.1), (0.42, 0.22)], s);
            }
            if pic != Pic::BrushOnlyUp {
                line(&[(0.7, 0.58), (0.7, 0.9)], s);
                line(&[(0.58, 0.78), (0.7, 0.9), (0.82, 0.78)], s);
            }
        }
        Pic::DimAlong => {
            p.circle_filled(at(0.1, 0.85), 1.8, MUTED());
            p.circle_filled(at(0.9, 0.35), 1.8, MUTED());
            line(&[(0.05, 0.62), (0.85, 0.12)], s);
            line(&[(0.1, 0.85), (0.04, 0.7)], faint);
            line(&[(0.9, 0.35), (0.84, 0.2)], faint);
        }
        Pic::DimVertical => {
            p.circle_filled(at(0.3, 0.1), 1.8, MUTED());
            p.circle_filled(at(0.3, 0.9), 1.8, MUTED());
            line(&[(0.75, 0.1), (0.75, 0.9)], s);
            line(&[(0.6, 0.1), (0.9, 0.1)], s);
            line(&[(0.6, 0.9), (0.9, 0.9)], s);
        }
        Pic::DimHorizontal => {
            p.circle_filled(at(0.1, 0.7), 1.8, MUTED());
            p.circle_filled(at(0.9, 0.7), 1.8, MUTED());
            line(&[(0.1, 0.3), (0.9, 0.3)], s);
            line(&[(0.1, 0.15), (0.1, 0.45)], s);
            line(&[(0.9, 0.15), (0.9, 0.45)], s);
        }
        Pic::IssueZero => {
            line(&[(0.05, 0.5), (0.45, 0.5)], faint);
            line(&[(0.55, 0.5), (0.95, 0.5)], faint);
            p.circle_filled(at(0.5, 0.5), 3.0, c);
        }
        Pic::IssueOverlap => {
            line(&[(0.05, 0.4), (0.7, 0.4)], faint);
            line(&[(0.3, 0.6), (0.95, 0.6)], faint);
            line(&[(0.3, 0.5), (0.7, 0.5)], Stroke::new(3.0, c));
        }
        Pic::IssueCross => {
            line(&[(0.1, 0.1), (0.9, 0.9)], faint);
            line(&[(0.1, 0.9), (0.9, 0.1)], faint);
            p.circle_stroke(at(0.5, 0.5), w * 0.16, Stroke::new(1.6, c));
        }
        Pic::Icon(_) => {}
    }
}

/// A cut through a 3D shape: its base, then the round, pointed or flat top above it.
pub(crate) fn section_preview(ui: &mut Ui, profile: Profile, height: f64, base: f64, unit: &str) {
    let w = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(w, 78.0), Sense::hover());
    let p = ui.painter_at(area);
    p.rect_filled(area, 8.0, CARD());
    let total = (height + base).max(1e-9);
    let room = area.shrink2(vec2(40.0, 12.0));
    let ground = room.bottom();
    let k = room.height() as f64 / total;
    let base_top = ground - (base * k) as f32;
    let top = ground - (total * k) as f32;
    let (l, r) = (room.left(), room.right());
    p.line_segment([pos2(area.left() + 6.0, ground), pos2(area.right() - 6.0, ground)], Stroke::new(1.0, MUTED()));
    if base > 0.0 {
        p.rect_filled(Rect::from_min_max(pos2(l, base_top), pos2(r, ground)), 1.0, theme::ACCENT_SOFT());
    }
    let pts: Vec<egui::Pos2> = match profile {
        Profile::Round => (0..=24).map(|i| {
            let t = std::f32::consts::PI * i as f32 / 24.0;
            pos2(l + (r - l) * (1.0 - t.cos()) / 2.0, base_top - (base_top - top) * t.sin())
        }).collect(),
        Profile::Angular => vec![pos2(l, base_top), pos2((l + r) / 2.0, top), pos2(r, base_top)],
        Profile::Flat => vec![pos2(l, base_top), pos2(l, top), pos2(r, top), pos2(r, base_top)],
    };
    p.add(Shape::line(pts, Stroke::new(1.8, ACCENT)));
    let tag = |y: f32, text: String| {
        p.text(pos2(area.right() - 6.0, y), Align2::RIGHT_CENTER, text, FontId::proportional(10.5), MUTED());
    };
    tag(top, format!("{} {unit}", crate::toolpath::trim(height + base)));
    if base > 0.0 {
        tag(base_top + 6.0, format!("{} {unit}", crate::toolpath::trim(base)));
    }
}

/// A small button with a picture of what it does before its words.
pub(crate) fn pic_button(ui: &mut Ui, pic: Pic, label: &str) -> egui::Response {
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, TEXT());
    let (r, resp) = ui.allocate_exact_size(vec2(16.0 + 14.0 + galley.size().x + 6.0, 24.0), Sense::click());
    let fill = if resp.hovered() { theme::BUTTON_HOVER() } else { theme::BUTTON() };
    ui.painter().rect_filled(r, 6.0, fill);
    draw_pic(ui.painter(), Rect::from_min_size(r.min + vec2(5.0, 4.0), vec2(16.0, 16.0)), pic, if resp.hovered() { ACCENT } else { TEXT() });
    ui.painter().galley(pos2(r.min.x + 25.0, r.center().y - galley.size().y / 2.0), galley, TEXT());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// What climb and conventional cutting mean, with their pictures.
pub(crate) fn explain_direction(ui: &mut Ui, climb: bool) {
    if climb {
        explain(ui, Pic::Climb, crate::i18n::tr("Climb: the cutter rolls along the wall. A cleaner edge, for a stiff machine."));
    } else {
        explain(ui, Pic::Conventional, crate::i18n::tr("Conventional: the cutter bites against its way. Gentler on a light machine."));
    }
}

/// The chosen option drawn large, with a sentence on what it does: so a choice can be understood
/// without knowing the word for it.
pub(crate) fn explain(ui: &mut Ui, pic: Pic, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
        ui.painter().rect_filled(r, 8.0, CARD());
        draw_pic(ui.painter(), r.shrink(7.0), pic, ACCENT);
        ui.add(egui::Label::new(RichText::new(text).size(11.5).color(MUTED())).wrap());
    });
}

/// A row of joined pill buttons, one chosen; returns the one clicked, if any. When they don't
/// fit across, they go on as many rows as they need.
pub(crate) fn pills(ui: &mut Ui, options: &[&str], chosen: usize) -> Option<usize> {
    let plain: Vec<(Option<Pic>, &str)> = options.iter().map(|o| (None, *o)).collect();
    pictured_pills(ui, &plain, chosen)
}

/// Pills that each show a small picture of what they do before their words.
pub(crate) fn pictured_pills(ui: &mut Ui, options: &[(Option<Pic>, &str)], chosen: usize) -> Option<usize> {
    let font = FontId::proportional(12.0);
    let widths: Vec<f32> = options.iter().map(|(pic, o)| ui.painter().layout_no_wrap(o.to_string(), font.clone(), TEXT()).size().x + 20.0 + if pic.is_some() { 20.0 } else { 0.0 }).collect();
    let tall = if options.iter().any(|(p, _)| p.is_some()) { 26.0 } else { 22.0 };
    let room = ui.available_width().max(60.0);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (k, w) in widths.iter().enumerate() {
        if used + w > room && !rows.last().unwrap().is_empty() {
            rows.push(Vec::new());
            used = 0.0;
        }
        rows.last_mut().unwrap().push(k);
        used += w;
    }
    let mut clicked = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for row in &rows {
            let total: f32 = row.iter().map(|k| widths[*k]).sum();
            let (whole, _) = ui.allocate_exact_size(vec2(total, tall), Sense::hover());
            ui.painter().rect(whole, tall / 2.0, INPUT(), Stroke::new(1.0, LINE()), StrokeKind::Inside);
            let mut x = whole.left();
            for &k in row {
                let ((pic, o), w) = (options[k], widths[k]);
                let r = Rect::from_min_size(pos2(x, whole.top()), vec2(w, whole.height()));
                let resp = ui.interact(r, ui.scope_id().with(("pill", o)), Sense::click());
                if k == chosen {
                    ui.painter().rect(r.shrink(2.0), tall / 2.0 - 2.0, theme::ACCENT_SOFT(), Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r.shrink(2.0), tall / 2.0 - 2.0, theme::BUTTON());
                }
                let colour = if k == chosen { TEXT() } else { MUTED() };
                let text_left = if let Some(pic) = pic {
                    let square = Rect::from_min_size(pos2(r.left() + 9.0, r.center().y - 8.0), vec2(16.0, 16.0));
                    draw_pic(ui.painter(), square, pic, if k == chosen { ACCENT } else { MUTED() });
                    r.left() + 29.0
                } else {
                    r.left() + 10.0
                };
                ui.painter().text(pos2(text_left, r.center().y), Align2::LEFT_CENTER, o, font.clone(), colour);
                label_widget(&resp, o);
                if resp.clicked() {
                    clicked = Some(k);
                }
                x += w;
            }
        }
    });
    clicked
}

/// The shape the form will make, drawn small from its numbers as they're typed.
pub(crate) fn shape_preview(ui: &mut Ui, f: &DrawForm) {
    let w = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(w, 84.0), Sense::hover());
    let p = ui.painter_at(area);
    p.rect_filled(area, 8.0, CARD());
    let Some(prim) = f.prim() else { return };
    let paths = prim.paths();
    let Some((lo, hi)) = geom::bbox(&paths) else { return };
    let size = [(hi[0] - lo[0]).max(1e-9), (hi[1] - lo[1]).max(1e-9)];
    let room = area.shrink(12.0);
    let k = (room.width() as f64 / size[0]).min(room.height() as f64 / size[1]);
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let at = |q: [f64; 2]| pos2(room.center().x + ((q[0] - mid[0]) * k) as f32, room.center().y - ((q[1] - mid[1]) * k) as f32);
    for path in &paths {
        let mut pts: Vec<egui::Pos2> = path.pts.iter().map(|q| at(*q)).collect();
        if path.closed {
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
        }
        p.add(Shape::line(pts, Stroke::new(1.6, ACCENT)));
    }
}


impl App {
    // ----- File dialogs -----

    /// Something in this project that a reopened file wouldn't have.
    pub fn has_unsaved_work(&self) -> bool {
        let d = &self.doc;
        self.dirty && !(d.vectors.is_empty() && d.toolpaths.is_empty() && d.components.is_empty() && d.bitmaps.is_empty())
    }

    /// Goes on with this, asking first if unsaved work would be lost.
    pub fn leave_for(&mut self, leave: Leave) {
        if self.has_unsaved_work() {
            self.leave = Some(leave);
        } else {
            match leave {
                Leave::New => self.new_file(),
                Leave::Open => self.dialog_open(),
                Leave::ToStart => self.close_project(),
                Leave::Recent => self.open_recent_wanted(),
                // Closing is only asked about once the window has been told to close.
                Leave::Close => {}
            }
        }
    }

    fn recalc_question(&mut self, ctx: &egui::Context) {
        if !self.ask_recalc {
            return;
        }
        let (mut now, mut later) = (false, false);
        let shown = egui::Modal::new(egui::Id::NULL.with("material-changed")).show(ctx, |ui| {
            ui.set_width(350.0);
            bold_sized(ui, crate::i18n::tr("Recalculate the toolpaths?"), 14.0, TEXT());
            ui.add_space(6.0);
            match &self.recalc_note {
                Some(note) => ui.label(note.as_str()),
                None => ui.label(crate::i18n::tr("The material's settings have changed, so all the toolpaths have to be worked out again to be right.")),
            };
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                now = primary(ui, crate::i18n::tr("Recalculate Now"), 120.0).clicked();
                ui.add_space((ui.available_width() - 74.0).max(0.0));
                later = ui.add_sized([74.0, 24.0], egui::Button::new(crate::i18n::tr("Later"))).clicked();
            });
        });
        if now {
            self.ask_recalc = false;
            self.recalc_note = None;
            self.recalculate_all();
        } else if later || shown.should_close() {
            self.ask_recalc = false;
            self.recalc_note = None;
            self.all_behind = true;
        }
    }

    fn leave_window(&mut self, ctx: &egui::Context) {
        let Some(leave) = self.leave else { return };
        let (mut save, mut drop_it, mut cancel) = (false, false, false);
        let shown = egui::Modal::new(egui::Id::NULL.with("unsaved-work")).show(ctx, |ui| {
            ui.set_width(350.0);
            bold_sized(ui, crate::i18n::tr("Save your changes first?"), 14.0, TEXT());
            ui.add_space(6.0);
            ui.label(crate::i18n::tr("This project has changes that haven't been saved. They'll be lost if you go on without saving."));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                save = primary(ui, crate::i18n::tr("Save"), 74.0).clicked();
                drop_it = ui.button(crate::i18n::tr("Don't Save")).clicked();
                ui.add_space((ui.available_width() - 74.0).max(0.0));
                cancel = ui.add_sized([74.0, 24.0], egui::Button::new(crate::i18n::tr("Cancel"))).clicked();
            });
        });
        if cancel || shown.should_close() {
            self.leave = None;
        } else if save || drop_it {
            self.leave = None;
            if save {
                self.dialog_save(false);
                // Its file window was closed without choosing, or it couldn't be written.
                if self.dirty {
                    return;
                }
            }
            match leave {
                Leave::New => self.new_file(),
                // The project stays until another has really been opened.
                Leave::Open => self.dialog_open(),
                Leave::ToStart => self.close_project(),
                Leave::Recent => self.open_recent_wanted(),
                Leave::Close => {
                    self.dirty = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    pub fn dialog_open(&mut self) {
        if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Projects"), &["burin", "crv3d", "crv"]).add_filter(crate::i18n::tr("Burin project"), &["burin"]).add_filter(crate::i18n::tr(".crv3d project"), &["crv3d", "crv"]).pick_file() {
            if let Err(e) = self.open_path(path) {
                self.message = Some(e);
            }
        }
    }

    pub fn dialog_save(&mut self, save_as: bool) {
        let target = match (&self.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Burin project"), &["burin"]).set_file_name("project.burin").save_file(),
        };
        if let Some(path) = target {
            if let Err(e) = self.save_to(path) {
                self.message = Some(e);
            }
        }
    }

    pub fn dialog_import(&mut self) {
        let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Drawing or 3D clipart"), &["svg", "dxf", crate::modeltools::CLIP3D_EXTENSION]).pick_file() else { return };
        let clip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(crate::modeltools::CLIP3D_EXTENSION));
        if let Err(why) = if clip { self.import_3d_clipart(&path) } else { self.import_drawing(&path) } {
            self.message = Some(why);
        }
    }

    /// Adds the shapes of a DXF or SVG file to the job, selected.
    pub fn import_drawing(&mut self, path: &std::path::Path) -> Result<(), String> {
        // Older drawing files are often in other character sets; only the numbers matter here.
        let bytes = crate::platform::read(path).map_err(|e| tf!("Could not read {}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        let is_svg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        let mut masked = 0;
        let (objects, skipped) = if is_svg {
            let per_inch = if self.doc.job.units == Units::Mm { 25.4 } else { 1.0 };
            let d = crate::svg::read(&text, per_inch)?;
            masked = d.masked;
            (d.objects, d.skipped)
        } else {
            (geom::dxf_read(&text).into_iter().map(|p| vec![p]).collect::<Vec<_>>(), 0)
        };
        if objects.is_empty() {
            return Err(tf!("{} has no lines, curves or shapes this program can read.", path.display()));
        }
        self.checkpoint();
        self.selected.clear();
        for paths in objects {
            let id = self.doc.add_vector(paths, Prim::Free);
            self.selected.push(id);
        }
        self.view3d = false;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let mut note = tf!("Imported {name}.");
        if skipped > 0 {
            note += &tf!(" {skipped} objects were left out: text must be converted to outlines first.");
        }
        if masked > 0 {
            note += &tf!(" {masked} objects are hidden in part by a mask in the file: they come in whole.");
        }
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let picked = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id));
        if geom::bbox(picked.flat_map(|v| v.paths.iter())).is_some_and(|(lo, hi)| lo[0] < -1e-6 || lo[1] < -1e-6 || hi[0] > w + 1e-6 || hi[1] > h + 1e-6) {
            note += crate::i18n::tr(" Part of it lies outside the job.");
        }
        self.status = note;
        Ok(())
    }

    pub fn dialog_export(&mut self) {
        let paths = if self.selected.is_empty() {
            self.doc.vectors.iter().filter(|v| !v.construction).flat_map(|v| v.paths.iter().cloned()).collect::<Vec<Path>>()
        } else {
            self.selected_paths()
        };
        if paths.is_empty() {
            self.status = crate::i18n::tr("There are no shapes to export.").into();
            return;
        }
        if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Drawing"), &["dxf"]).set_file_name("shapes.dxf").save_file() {
            match crate::platform::write(&path, geom::dxf_write(&paths)) {
                Ok(()) => self.status = tf!("Exported {}", path.display()),
                Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
            }
        }
    }


    // ----- The whole window -----

    pub fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        crate::icons::set_tint(None);
        crate::formula::set_context(self.formula_context());
        if let Some(filter) = self.open_font_list.take() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique(crate::fonts::OPEN_FONT_LIST), filter));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.has_unsaved_work() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.leave = Some(Leave::Close);
        }
        self.leave_window(&ctx);
        self.preferences_window(&ctx);
        self.merge_window(&ctx);
        self.template_window(&ctx);
        self.support_window(&ctx);
        self.help_window(&ctx);
        self.scale_z_window(&ctx);
        self.smooth_window(&ctx);
        self.offset_model_window(&ctx);
        self.texture_area_window(&ctx);
        self.sculpt_window(&ctx);
        self.emboss_window(&ctx);
        self.sweep_window(&ctx);
        self.extrude_window(&ctx);
        self.slice_model_window(&ctx);
        self.draft_model_window(&ctx);
        self.turn_spin_window(&ctx);
        self.variables_window(&ctx);
        self.print_setup_window(&ctx);
        self.notes_window(&ctx);
        self.backups_window(&ctx);
        self.checks_window(&ctx);
        self.resume_window(&ctx);
        self.curve_fit_window(&ctx);
        self.feedback_window(&ctx);
        self.crash_window(&ctx);
        self.after_crash_window(&ctx);
        self.palette_window(&ctx);
        self.keep_recovery();
        self.files_dropped(&ctx);
        crate::crash::set_job(self.doc.vectors.len(), self.doc.toolpaths.len(), self.doc.toolpaths.iter().map(|t| t.moves.len() as u64).sum());
        self.recalc_question(&ctx);
        self.poll_recalc();
        self.keyboard(ui);
        self.maintain_constraints();
        if self.anim.playing {
            let total: usize = self.anim.queue.iter().filter_map(|i| self.doc.toolpaths.get(*i)).map(|t| t.moves.len()).sum();
            let mut left = if self.anim.rush { usize::MAX } else { 1 + (self.speed.powi(4) * total as f32 / 45.0) as usize };
            // A little at a time, and never for long: the window must go on answering.
            let started = crate::platform::Instant::now();
            let budget = if self.surface_in_background { std::time::Duration::from_millis(14) } else { std::time::Duration::MAX };
            while left > 0 {
                let some = left.min(500);
                let more = self.advance_preview(some);
                left -= some;
                if !more || started.elapsed() >= budget {
                    break;
                }
            }
            if !self.anim.playing {
                self.anim.rush = false;
            }
            self.bump_surface();
            ui.ctx().request_repaint();
        }

        egui::Panel::top("menu").exact_size(34.0).frame(Frame::new().fill(CHROME()).inner_margin(Margin::symmetric(8, 3))).show(ui, |ui| self.menu_bar(ui));
        if self.screen == Screen::Start {
            self.start_screen(ui);
        } else {
            egui::Panel::bottom("status").exact_size(22.0).frame(Frame::new().fill(CHROME()).inner_margin(Margin::symmetric(4, 2))).show(ui, |ui| self.status_bar(ui));
            self.follow_persona();
            self.left_panel(ui);
            self.right_panel(ui);
            egui::CentralPanel::default().frame(Frame::new().fill(WORKSPACE())).show(ui, |ui| self.centre(ui));
            self.inspector(&ctx);
        }
        self.windows(ui);
        // Last, so that a shape let go this frame counts at once, whatever is on the screen.
        self.note_changed_shapes();
        // Started just now, by the shape let go or a button above.
        if self.recalc.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// The component list takes the keys when a component is chosen and no shape is, standing in for
    /// the original's tree having the focus (its key handler `0x1403ba190`).
    pub(crate) fn component_list_in_use(&self) -> bool {
        self.left_tab == LeftTab::Modeling && self.selected.is_empty() && self.selected_comp.is_some_and(|i| i < self.doc.components.len())
    }

    /// Up and Down choose the component above or below the chosen one, and stop at the ends,
    /// as the original's component tree does with one component chosen (`0x1403ba190`).
    fn component_keys(&mut self, ctx: &egui::Context) {
        if !self.component_list_in_use() || self.recalc.is_some() || ctx.egui_wants_keyboard_input() || self.screen != Screen::Work {
            return;
        }
        let step = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) as i32 - i.key_pressed(egui::Key::ArrowUp) as i32);
        if let Some(i) = self.selected_comp.filter(|_| step != 0) {
            let j = i as i32 + step;
            if j >= 0 && (j as usize) < self.doc.components.len() {
                self.selected_comp = Some(j as usize);
            }
        }
    }

    /// The arrow keys move the selection: Shift ten times as far, Ctrl a hundred, Alt a tenth,
    /// and Ctrl with Shift the original's fixed distance.
    fn nudge(&mut self, ctx: &egui::Context) {
        use egui::Key;
        let arrows = [(Key::ArrowLeft, -1.0, 0.0), (Key::ArrowRight, 1.0, 0.0), (Key::ArrowDown, 0.0, -1.0), (Key::ArrowUp, 0.0, 1.0)];
        if self.nudging && !ctx.input(|i| arrows.iter().any(|(k, ..)| i.key_down(*k))) {
            self.nudging = false;
            ctx.request_repaint();
        }
        if self.selected.is_empty() || self.view3d || self.recalc.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (mut dx, mut dy) = (0.0, 0.0);
        ctx.input(|i| {
            for (k, x, y) in arrows {
                if i.key_pressed(k) {
                    dx += x;
                    dy += y;
                }
            }
        });
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let (cmd, shift, alt) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift, i.modifiers.alt));
        let times = if cmd { 100.0 } else if shift { 10.0 } else if alt { 0.1 } else { 1.0 };
        let metric = self.doc.job.units == Units::Mm;
        let step = match (cmd && shift, metric) {
            (true, true) => 0.1,
            (true, false) => 0.125,
            (false, true) => times,
            (false, false) => 0.05 * times,
        };
        if !self.nudging {
            self.checkpoint();
            self.nudging = true;
        }
        self.translate_selected(dx * step, dy * step);
        // A key let go within this same frame wouldn't bring another one, and the move would never end.
        ctx.request_repaint();
        self.status = tf!("Moved the selection by {} {}. Shift: x10, Ctrl: x100, Alt: x0.1, Ctrl+Shift: fixed.", toolpath::trim(step), self.doc.job.unit());
    }

    fn keyboard(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // Keys pressed while choosing them don't do anything else.
        if self.prefs_open {
            return;
        }
        self.nudge(&ctx);
        self.component_keys(&ctx);
        if self.sculpt.is_some() {
            // While sculpting, the numbers choose the tool (the original's form).
            self.sculpt_keys(&ctx);
            return;
        }
        if self.recalc.is_some() {
            return;
        }
        if ctx.egui_wants_keyboard_input() || self.screen != Screen::Work {
            return;
        }
        // Keys typed while dragging are a value, not shortcuts.
        if matches!(self.drag, crate::canvas::Drag::Move { .. } | crate::canvas::Drag::Rotate { .. } | crate::canvas::Drag::Scale { .. }) {
            return;
        }
        let keys = self.keys;
        // An action chosen in the command search counts as its key pressed.
        let forced = self.forced_action.take();
        let fired = |action: Action| forced == Some(action) || ctx.input(|i| keys.fired(action, i));
        if !self.view3d {
            if fired(Action::Group) {
                self.group_selected();
            }
            // All the levels or one, and everything back on its own layer or staying on the group's.
            for (action, deep, own_layers) in [(Action::Ungroup, false, true), (Action::UngroupAll, true, true), (Action::UngroupStay, false, false), (Action::UngroupAllStay, true, false)] {
                if fired(action) {
                    self.ungroup_with(deep, own_layers);
                }
            }
            if fired(Action::MakePiece) {
                self.make_piece();
            }
            if fired(Action::SelectPiece) && self.selection_in_piece() {
                self.select_piece();
            }
            if fired(Action::AdjustPiece) && (self.selection_in_piece() || self.adjust_piece) {
                self.adjust_piece = !self.adjust_piece;
            }
            if fired(Action::ReleasePiece) {
                self.release_piece();
            }
            // These belong to the select tool, with nothing being drawn or dragged.
            if self.tool == Tool2::Select && matches!(self.left, LeftForm::None) && matches!(self.drag, crate::canvas::Drag::None) {
                for (action, across, about_job, copy) in [
                    (Action::MirrorX, true, false, false),
                    (Action::MirrorXJob, true, true, false),
                    (Action::MirrorXCopy, true, false, true),
                    (Action::MirrorXJobCopy, true, true, true),
                    (Action::MirrorY, false, false, false),
                    (Action::MirrorYJob, false, true, false),
                    (Action::MirrorYCopy, false, false, true),
                    (Action::MirrorYJobCopy, false, true, true),
                ] {
                    if fired(action) {
                        self.mirror(across, None, about_job, copy);
                    }
                }
                if fired(Action::TurnLeft) {
                    self.turn_selected(45.0);
                }
                if fired(Action::TurnRight) {
                    self.turn_selected(-45.0);
                }
            }
        }
        if fired(Action::Delete) || fired(Action::DeleteAgain) {
            // A chosen rule goes first, and the shapes stay; with no shape chosen, the component goes.
            match self.selected_rule.filter(|_| matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Constraints)) {
                Some(i) => self.remove_rule(i),
                None if self.component_list_in_use() => self.delete_component(),
                None => self.delete_selected(),
            }
        }
        use egui::Key;
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if pressed(Key::Escape) {
            // Only the select tool's own Escape drops the selection: anywhere else it just ends that tool.
            if self.tool == Tool2::Select && matches!(self.left, LeftForm::None) {
                self.selected.clear();
            }
            if let LeftForm::Draw(f) = &mut self.left {
                f.points.clear();
            }
            self.picks.clear();
            self.close_left_form();
            self.tool = Tool2::Select;
            self.handles_on = None;
        }
        if pressed(Key::Enter) && matches!(&self.left, LeftForm::Draw(f) if !f.points.is_empty()) {
            self.finish_line(false);
        }
        if fired(Action::Undo) {
            self.do_undo();
        }
        if fired(Action::Redo) || fired(Action::RedoAgain) {
            self.do_redo();
        }
        if fired(Action::SelectAll) {
            self.select_all();
        }
        if fired(Action::SelectSimilar) {
            let tol = self.default_similar_tolerance();
            self.select_similar(tol);
        }
        if fired(Action::Save) {
            self.dialog_save(false);
        }
        if fired(Action::SaveAs) {
            self.dialog_save(true);
        }
        if fired(Action::Open) {
            self.leave_for(Leave::Open);
        }
        if fired(Action::New) {
            self.leave_for(Leave::New);
        }
        if fired(Action::Copy) {
            self.copy_selected();
        }
        if fired(Action::Cut) {
            self.copy_selected();
            self.delete_selected();
        }
        if fired(Action::Paste) {
            self.paste();
        }
        if fired(Action::Import) {
            self.dialog_import();
        }
        if fired(Action::IncrementSave) {
            self.increment_and_save();
        }
        if fired(Action::Refresh) {
            ctx.request_repaint();
        }
        if fired(Action::View2d) {
            self.view3d = false;
        }
        if fired(Action::View3d) {
            self.view3d = true;
        }
        // Space opens the last drawing form again (the manual's Shortcut Keys page); with a line
        // being drawn it finishes the line instead, which is handled by the line tool.
        if !self.view3d && matches!(self.left, LeftForm::None) && matches!(self.drag, crate::canvas::Drag::None) && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            if let Some(kind) = self.last_draw {
                self.start_draw(kind);
            }
        }
        if fired(Action::TopSide) {
            self.set_active_face(0);
        }
        if fired(Action::BottomSide) {
            self.set_active_face(1);
        }
        if fired(Action::MultiSided) && self.doc.job.two_sided {
            self.multi_sided = !self.multi_sided;
        }
        if fired(Action::TileSideBySide) {
            self.tile(crate::app::Tiling::SideBySide);
        }
        if fired(Action::TileStacked) {
            self.tile(crate::app::Tiling::Stacked);
        }
        if fired(Action::ZoomFit) || fired(Action::ZoomFitAgain) {
            self.zoom_to_job();
        }
        let tab = if fired(Action::DrawingOrModeling) {
            Some(if self.left_tab == LeftTab::Modeling { LeftTab::Drawing } else { LeftTab::Modeling })
        } else if fired(Action::DrawingTab) {
            Some(LeftTab::Drawing)
        } else if fired(Action::LayersTab) {
            Some(LeftTab::Layers)
        } else {
            None
        };
        if let Some(tab) = tab {
            self.left_tab = tab;
            self.left_open = true;
            self.close_left_form();
            if self.persona == Persona::Machining {
                self.set_persona(Persona::Design);
            }
        }
        if fired(Action::LeftPanel) {
            if self.persona == Persona::Machining {
                self.set_persona(Persona::Design);
            } else {
                self.left_open = !self.left_open;
            }
            self.auto_fit = true;
        }
        if fired(Action::RightPanel) {
            if self.persona == Persona::Design {
                self.set_persona(Persona::Machining);
            } else {
                self.right_open = !self.right_open;
            }
            self.auto_fit = true;
        }
        if !self.view3d {
            if fired(Action::ZoomBox) {
                self.tool = if self.tool == Tool2::ZoomBox { Tool2::Select } else { Tool2::ZoomBox };
            }
            if fired(Action::NodesOrSelect) {
                self.tool = if self.tool == Tool2::Nodes { Tool2::Select } else { Tool2::Nodes };
                self.left = LeftForm::None;
            }
            if fired(Action::Measure) {
                self.tool = Tool2::Measure;
                self.left = LeftForm::None;
            }
            for (action, form) in [
                (Action::MoveForm, Xform::Move),
                (Action::SizeForm, Xform::Scale),
                (Action::RotateForm, Xform::Rotate),
                (Action::JoinForm, Xform::Join),
                (Action::AlignForm, Xform::Align),
            ] {
                if fired(action) {
                    self.start_xform(form);
                }
            }
            if fired(Action::CentreOnMaterial) {
                self.align_selected(Some(0), Some(0));
            }
            for (action, kind) in [
                (Action::DrawLines, DrawKind::Polyline),
                (Action::DrawCircle, DrawKind::Circle),
                (Action::DrawEllipse, DrawKind::Ellipse),
                (Action::DrawRect, DrawKind::Rect),
                (Action::DrawPolygon, DrawKind::Polygon),
                (Action::DrawStar, DrawKind::Star),
                (Action::DrawArc, DrawKind::Arc),
                (Action::DrawCurve, DrawKind::Curve),
                (Action::DrawSpiral, DrawKind::Spiral),
                (Action::DrawText, DrawKind::Text),
            ] {
                if fired(action) {
                    self.start_draw(kind);
                }
            }
        }
    }

    pub(crate) fn zoom_to_job(&mut self) {
        if self.view3d {
            self.cam.zoom = 1.0;
            self.cam.pan = egui::Vec2::ZERO;
        } else {
            self.auto_fit = true;
            self.last_rect = Rect::NOTHING;
        }
    }

    /// Choosing the language and the keys. The list waits for a key while one is being changed.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        if !self.prefs_open {
            return;
        }
        if let Some(action) = self.prefs_waiting {
            if let Some(key) = ctx.input(Shortcut::pressed_now) {
                if key.key == egui::Key::Escape {
                    self.prefs_waiting = None;
                    self.prefs_note.clear();
                } else if crate::keys::reserved(key.key) {
                    self.prefs_note = tf!("{} can't be changed: it already has its own use.", key.text());
                } else if let Some(other) = self.keys.taken_by(key, action) {
                    self.prefs_note = tf!("{} is already used by: {}", key.text(), crate::keys::action_name(other));
                } else {
                    self.keys.set(action, Some(key));
                    self.prefs_waiting = None;
                    self.prefs_note.clear();
                    crate::keys::save(self.keys);
                }
            }
        }
        let mut open = true;
        egui::Window::new(crate::i18n::tr("Preferences")).id(egui::Id::unique("preferences")).open(&mut open).collapsible(false).default_width(470.0).default_height(560.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Language")).strong());
                for lang in crate::i18n::Lang::ALL {
                    if ui.selectable_label(crate::i18n::language() == lang, lang.name()).clicked() && crate::i18n::language() != lang {
                        crate::i18n::set_language(lang);
                        crate::keys::save(self.keys);
                    }
                }
            });
            // The dark look, or the light "workshop" one: warm paper, wood and ink.
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Look")).strong());
                let light = theme::light();
                for (on, name) in [(false, crate::i18n::tr("Dark")), (true, crate::i18n::tr("Light (workshop)"))] {
                    if ui.selectable_label(light == on, name).clicked() && light != on {
                        theme::set_light(on);
                        theme::save_look();
                        setup_style(ui.ctx());
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("Keyboard Shortcuts")).strong());
            ui.label(RichText::new(crate::i18n::tr("Click a key to change it, then press the new one. Escape cancels.")).color(MUTED()));
            if !self.prefs_note.is_empty() {
                ui.label(RichText::new(self.prefs_note.as_str()).color(theme::ERROR));
            }
            egui::ScrollArea::vertical().max_height(ui.available_height() - 74.0).auto_shrink([false, false]).show(ui, |ui| {
                for (section, actions) in crate::keys::SECTIONS {
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::i18n::tr(section)).strong().color(ACCENT));
                    egui::Grid::new(section).num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                        for action in actions {
                            ui.allocate_ui_with_layout(vec2(290.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_width(290.0);
                                ui.add(egui::Label::new(crate::keys::action_name(*action)).truncate());
                            });
                            let waiting = self.prefs_waiting == Some(*action);
                            let text = if waiting { crate::i18n::tr("Press a key...").to_string() } else { self.keys.text(*action) };
                            if ui.add_sized([130.0, 22.0], egui::Button::new(text).selected(waiting)).clicked() {
                                self.prefs_waiting = (!waiting).then_some(*action);
                                self.prefs_note.clear();
                            }
                            if ui.add_enabled(self.keys.get(*action).is_some(), egui::Button::new("x")).on_hover_text(crate::i18n::tr("Remove this key")).clicked() {
                                self.keys.set(*action, None);
                                self.prefs_waiting = None;
                                self.prefs_note.clear();
                                crate::keys::save(self.keys);
                            }
                            ui.end_row();
                        }
                    });
                }
            });
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("These keys can't be changed: the arrows move the selection, Escape cancels, Enter finishes a line, and Space starts a new one.")).size(11.5).color(MUTED()));
            if ui.button(crate::i18n::tr("Restore Default Keys")).clicked() {
                self.keys = crate::keys::Keys::default();
                self.prefs_waiting = None;
                self.prefs_note.clear();
                crate::keys::save(self.keys);
            }
        });
        if !open {
            self.prefs_open = false;
            self.prefs_waiting = None;
            self.prefs_note.clear();
        }
    }

    pub fn select_all(&mut self) {
        self.selected = self.doc.vectors.iter().filter(|v| self.doc.can_pick(v)).map(|v| v.id).collect();
    }

    pub fn close_left_form(&mut self) {
        if !matches!(self.left, LeftForm::Job { .. }) {
            self.left = LeftForm::None;
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.bg_stroke = Stroke::NONE;
            w.inactive.weak_bg_fill = Color32::TRANSPARENT;
            let work = self.screen == Screen::Work;
            ui.add_space(2.0);
            bold_sized(ui, "Burin", 14.0, ACCENT);
            ui.add_space(4.0);
            if work && self.persona != Persona::Both {
                self.persona_switch(ui);
                ui.add_space(6.0);
            }
            self.main_menus(ui, work);
            if work {
                self.quick_actions(ui);
            }
        });
    }

    /// What is wanted from anywhere: the file, undo, the clipboard, and the job itself.
    fn quick_actions(&mut self, ui: &mut Ui) {
        let some = !self.selected.is_empty();
        let divider = |ui: &mut Ui| {
            let (r, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
            ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, LINE()));
        };
        // The job's size stays on the far right, however little room is left.
        let job = self.doc.job.clone();
        let text = format!("{} \u{d7} {} \u{d7} {} {}", toolpath::trim(job.width), toolpath::trim(job.height), toolpath::trim(job.thickness), job.unit());
        let galley = ui.painter().layout_no_wrap(text, FontId::proportional(12.5), TEXT());
        let chip_width = galley.size().x + 44.0;
        ui.add_space(6.0);
        divider(ui);
        ui.spacing_mut().item_spacing.x = 1.0;
        let actions: [(Icon, &str, bool); 3] = [(Icon::New, crate::i18n::tr("New"), true), (Icon::Open, crate::i18n::tr("Open"), true), (Icon::Save, crate::i18n::tr("Save"), true)];
        for (icon, tip, on) in actions {
            if icon_button(ui, icon, tip, on, false).clicked() {
                match icon {
                    Icon::New => self.leave_for(Leave::New),
                    Icon::Open => self.leave_for(Leave::Open),
                    _ => self.dialog_save(false),
                }
            }
        }
        divider(ui);
        if icon_button(ui, Icon::Undo, crate::i18n::tr("Undo"), !self.undo.is_empty(), false).clicked() {
            self.do_undo();
        }
        if icon_button(ui, Icon::Redo, crate::i18n::tr("Redo"), !self.redo.is_empty(), false).clicked() {
            self.do_redo();
        }
        // Everything from here on is also in the menus, so it can give way on a narrow window.
        const ICON: f32 = 29.0;
        const DIVIDER: f32 = 15.0;
        let room = ui.available_width() - chip_width - 12.0;
        if room >= 4.0 * ICON + DIVIDER {
            divider(ui);
            if icon_button(ui, Icon::Cut, crate::i18n::tr("Cut"), some, false).clicked() {
                self.copy_selected();
                self.delete_selected();
            }
            if icon_button(ui, Icon::Copy, crate::i18n::tr("Copy"), some, false).clicked() {
                self.copy_selected();
            }
            if icon_button(ui, Icon::Paste, crate::i18n::tr("Paste"), !self.clipboard.is_empty(), false).clicked() {
                self.paste();
            }
            if icon_button(ui, Icon::SelectAll, crate::i18n::tr("Select all"), true, false).clicked() {
                self.select_all();
            }
        }
        if room >= 6.0 * ICON + 2.0 * DIVIDER {
            divider(ui);
            if icon_button(ui, Icon::Import, crate::i18n::tr("Import shapes"), true, false).clicked() {
                self.dialog_import();
            }
            if icon_button(ui, Icon::Export, crate::i18n::tr("Export shapes"), true, false).clicked() {
                self.dialog_export();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(chip_width, 24.0), Sense::click());
            let lit = resp.hovered();
            ui.painter().rect(rect, 12.0, if lit { theme::BUTTON_HOVER() } else { theme::CARD() }, Stroke::NONE, StrokeKind::Inside);
            icons::paint(ui.painter(), Rect::from_center_size(rect.left_center() + vec2(15.0, 0.0), vec2(20.0, 20.0)), Icon::JobSetup, true);
            ui.painter().galley(pos2(rect.left() + 29.0, rect.center().y - galley.size().y / 2.0), galley.clone(), TEXT());
            label_widget(&resp, crate::i18n::tr("Stock & origin..."));
            let tip = tf!("Stock & origin: change the size, thickness, units and origin.\nOrigin: {}", job.datum.label());
            if resp.on_hover_text(tip).clicked() {
                self.open_job_setup();
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        let font = FontId::proportional(12.0);
        let c = TEXT();
        // The status message gives way to the sketch state when the window is narrow.
        let state_width = self.definition_text().map_or(0.0, |(text, colour)| p.layout_no_wrap(text, font.clone(), colour).size().x + 18.0);
        let status_end = (rect.right() - 546.0 - state_width).max(rect.left() + 40.0);
        let status_area = Rect::from_min_max(rect.min, pos2(status_end, rect.bottom()));
        p.with_clip_rect(status_area).text(rect.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, &self.status, font.clone(), c);
        let x = rect.right() - 540.0;
        let y = rect.right() - 358.0;
        if self.grid && !self.view3d {
            let unit = self.doc.job.unit();
            p.text(pos2(y + 186.0, rect.center().y), Align2::LEFT_CENTER, tf!("Grid {} {unit}", toolpath::trim(self.grid_steps().0)), font.clone(), MUTED());
        }
        if let Some((text, colour)) = self.definition_text() {
            p.text(pos2(x - 12.0, rect.center().y), Align2::RIGHT_CENTER, text, font.clone(), colour);
        }
        let line = Stroke::new(1.0, LINE());
        p.line_segment([pos2(x - 6.0, rect.top()), pos2(x - 6.0, rect.bottom())], line);
        p.line_segment([pos2(y - 6.0, rect.top()), pos2(y - 6.0, rect.bottom())], line);
        if !self.view3d {
            if let Some(cursor) = self.cursor {
                let zero = self.doc.job.origin();
                p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, format!("X: {:.4} Y: {:.4}", cursor[0] - zero[0], cursor[1] - zero[1]), font.clone(), c);
            }
        }
        if let Some((lo, hi)) = self.selected_bbox() {
            let layer = self.doc.vectors.iter().find(|v| self.selected.contains(&v.id)).and_then(|v| self.doc.layers.get(v.layer)).map_or("", |l| l.name.as_str());
            p.text(pos2(y, rect.center().y), Align2::LEFT_CENTER, format!("W:{:.4}  H:{:.4}  L:{layer}", hi[0] - lo[0], hi[1] - lo[1]), font, c);
        }
    }

    // ----- Start screen -----

    fn ornament(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.ornament
            .get_or_insert_with(|| {
                // A carved rosette, lit from the top left and blended with the panel grey.
                let (w, h) = (250usize, 340usize);
                let height = |x: f32, y: f32| -> f32 {
                    let (dx, dy) = (x - 125.0, y - 180.0);
                    let r = (dx * dx + dy * dy).sqrt();
                    let a = dy.atan2(dx);
                    let disc = ((118.0 - r) / 3.0).clamp(0.0, 1.0) * 5.0 - ((104.0 - r) / 3.0).clamp(0.0, 1.0) * 3.0;
                    let petals = (84.0 * (0.72 + 0.28 * (8.0 * a).cos()) - r).clamp(0.0, 16.0);
                    let centre = (16.0 - r).clamp(0.0, 8.0) * 1.2;
                    disc + petals * 0.7 + centre
                };
                let mut pixels = Vec::with_capacity(w * h);
                for y in 0..h {
                    for x in 0..w {
                        let (fx, fy) = (x as f32, y as f32);
                        let shade = (height(fx + 1.0, fy + 1.0) - height(fx - 1.0, fy - 1.0)) * 7.0;
                        let fade = (y as f32 / 90.0).min(1.0);
                        let v = (-shade * fade * 0.35).clamp(-16.0, 34.0) + 3.0 * fade;
                        pixels.push(Color32::from_rgb((PANEL().r() as f32 + v) as u8, (PANEL().g() as f32 + v) as u8, (PANEL().b() as f32 + v) as u8));
                    }
                }
                ctx.load_texture("ornament", egui::ColorImage::new([w, h], pixels), egui::TextureOptions::LINEAR)
            })
            .id()
    }

    fn paint_ornament(&mut self, ui: &Ui, rect: Rect, top_limit: f32) {
        let tex = self.ornament(ui.ctx());
        let area = Rect::from_min_size(rect.left_bottom() - vec2(0.0, 340.0), vec2(rect.width().min(250.0), 340.0));
        // In short windows the ornament is cut off rather than running behind the controls.
        let clip = Rect::from_min_max(pos2(rect.left(), rect.top().max(area.top()).max(top_limit)), rect.max);
        ui.painter_at(clip).image(tex, area, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }

    /// The opening screen: just what can be done first, in the middle of the window.
    fn start_screen(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(Frame::new().fill(theme::WORKSPACE())).show(ui, |ui| {
            let whole = ui.max_rect();
            let width = 460.0_f32.min(whole.width() - 32.0);
            let recent = self.recent.clone();
            let height = 340.0 + if recent.is_empty() { 0.0 } else { 34.0 + recent.len().div_ceil(3) as f32 * 120.0 };
            let top = (whole.center().y - height / 2.0).max(whole.top() + 16.0);
            let column = Rect::from_min_max(pos2(whole.center().x - width / 2.0, top), pos2(whole.center().x + width / 2.0, whole.bottom()));
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(column));
            let ui = &mut body;
            let (logo, _) = ui.allocate_exact_size(vec2(width, 92.0), Sense::hover());
            // The logo on a cream ground in the light look, on slate in the dark one.
            if self.start_logo.as_ref().is_some_and(|(light, _)| *light != theme::light()) {
                self.start_logo = None;
            }
            let (_, picture) = self.start_logo.get_or_insert_with(|| {
                // Made small here, as its fine lines would shimmer if the screen did it.
                let full = if theme::light() {
                    image::load_from_memory(include_bytes!("../release/burin-light.png")).expect("the light logo is part of the program").into_rgba8()
                } else {
                    let icon = crate::icon();
                    image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba).expect("the icon has its full size")
                };
                let small = image::imageops::resize(&full, 184, 184, image::imageops::FilterType::Lanczos3);
                let colours = egui::ColorImage::from_rgba_unmultiplied([184, 184], small.as_raw());
                (theme::light(), ui.ctx().load_texture("burin-logo", colours, egui::TextureOptions::LINEAR))
            });
            let picture = picture.id();
            let p = ui.painter();
            let name = p.layout_no_wrap("Burin".to_string(), FontId::proportional(64.0), TEXT());
            let left = logo.center().x - (92.0 + 18.0 + name.size().x) / 2.0;
            let square = Rect::from_min_size(pos2(left, logo.top()), vec2(92.0, 92.0));
            p.image(picture, square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            p.galley(pos2(square.right() + 18.0, logo.center().y - name.size().y / 2.0), name, TEXT());
            let (line, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
            ui.painter().text(line.center(), Align2::CENTER_CENTER, crate::i18n::tr("Design it, then carve it"), FontId::proportional(14.0), theme::MUTED());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tf!("Version {}", env!("CARGO_PKG_VERSION"))).size(12.0).color(theme::MUTED()));
            });
            ui.add_space(22.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let tile = (width - 12.0) / 2.0;
                if start_tile(ui, Icon::New, crate::i18n::tr("Start a new project"), tile) {
                    self.leave_for(Leave::New);
                }
                if start_tile(ui, Icon::Folder, crate::i18n::tr("Open a project"), tile) {
                    self.leave_for(Leave::Open);
                }
            });
            ui.add_space(24.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            if !recent.is_empty() {
                bold_sized(ui, crate::i18n::tr("Recently opened files"), 12.5, theme::MUTED());
                // Each as a card with a small drawing of it, three to a row.
                let card_w = (width - 2.0 * 10.0) / 3.0;
                for row in recent.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for path in row {
                            if self.recent_card(ui, path, card_w) {
                                if let Err(e) = self.open_path(path.clone()) {
                                    self.message = Some(e);
                                }
                            }
                        }
                    });
                }
                ui.add_space(10.0);
            }
            // The sample, or one of the models to start from, drawn and with their toolpaths, two to a line.
            bold_sized(ui, crate::i18n::tr("Start from a model"), 12.5, theme::MUTED());
            let half = (width - 8.0) / 2.0;
            let mut choice: Option<Option<crate::starters::Starter>> = None;
            let models: Vec<(Option<crate::starters::Starter>, Icon, &str)> = std::iter::once((None, Icon::Sample, crate::i18n::tr("Open the sample project")))
                .chain(crate::starters::Starter::ALL.into_iter().zip([Icon::Text, Icon::Circle, Icon::Nest]).map(|(s, i)| (Some(s), i, s.name())))
                .collect();
            for pair in models.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (which, icon, name) in pair {
                        ui.allocate_ui(vec2(half, 22.0), |ui| {
                            if self.start_link(ui, *icon, name) {
                                choice = Some(*which);
                            }
                        });
                    }
                });
            }
            match choice {
                Some(None) => self.load_sample(),
                Some(Some(which)) => self.load_starter(which),
                None => {}
            }
        });
    }

    /// A recent project on the start screen: its drawing on a small sheet, and its name.
    fn recent_card(&mut self, ui: &mut Ui, path: &std::path::PathBuf, width: f32) -> bool {
        let thumb = self.thumbs.entry(path.clone()).or_insert_with(|| crate::thumbs::read(path)).clone();
        let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 112.0), Sense::click());
        ui.painter().rect(rect, 8.0, if resp.hovered() { theme::BUTTON() } else { CARD() }, Stroke::new(1.0, if resp.hovered() { theme::HUE_ORANGE() } else { LINE() }), StrokeKind::Inside);
        let room = Rect::from_min_max(rect.min + vec2(8.0, 8.0), pos2(rect.right() - 8.0, rect.bottom() - 26.0));
        // Shapes off the sheet are drawn too, but never past the card.
        let p = ui.painter().with_clip_rect(room);
        match &thumb {
            Some(t) => {
                // The sheet, as wide as the job, in the room left.
                let (sw, sh) = if t.aspect >= room.width() / room.height() { (room.width(), room.width() / t.aspect) } else { (room.height() * t.aspect, room.height()) };
                let sheet = Rect::from_center_size(room.center(), vec2(sw, sh));
                p.rect_filled(sheet, 3.0, theme::SHEET);
                let side = sw.max(sh);
                let at = |q: &[f32; 2]| pos2(sheet.left() + q[0] * side, sheet.bottom() - q[1] * side);
                for line in &t.lines {
                    p.add(egui::Shape::line(line.iter().map(at).collect(), Stroke::new(1.0, theme::SHEET_INK)));
                }
            }
            None => icons::paint(&p, Rect::from_center_size(room.center(), vec2(30.0, 30.0)), Icon::Doc, true),
        }
        // One line, cut short with an ellipsis when the name is long.
        let fits = |t: &str| ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(12.0), TEXT()).size().x <= width - 16.0;
        let mut shown = name.clone();
        if !fits(&shown) {
            let mut kept: Vec<char> = name.chars().collect();
            while kept.len() > 1 {
                kept.pop();
                shown = format!("{}…", kept.iter().collect::<String>().trim_end());
                if fits(&shown) {
                    break;
                }
            }
        }
        ui.painter().text(pos2(rect.left() + 8.0, rect.bottom() - 13.0), Align2::LEFT_CENTER, shown, FontId::proportional(12.0), TEXT());
        label_widget(&resp, &name);
        resp.on_hover_text(path.display().to_string()).clicked()
    }

    pub(crate) fn start_link(&mut self, ui: &mut Ui, icon: Icon, label: &str) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, CARD());
        }
        icons::paint(ui.painter(), Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(20.0, 20.0)), icon, true);
        ui.painter().text(rect.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), TEXT());
        label_widget(&resp, label);
        resp.clicked()
    }

    // ----- Left panel -----

    /// Goes to the workspace that what was just opened belongs to: a toolpath's form, the
    /// preview or the material to Machining; a drawing form or the job's setup to Design.
    fn follow_persona(&mut self) {
        let right = match self.right {
            RightMode::List => 0,
            RightMode::Op { .. } => 1,
            RightMode::Preview => 2,
            RightMode::Material(_) => 3,
            RightMode::Save => 4,
        };
        let left = match self.left {
            LeftForm::Job { .. } => 2,
            LeftForm::Draw(_) | LeftForm::Xform(_) | LeftForm::Shape(_) | LeftForm::Trace(_) | LeftForm::Relief(_) => 1,
            _ => 0,
        };
        let (was_right, was_left) = (self.persona_seen.0, self.persona_seen.1);
        self.persona_seen = (right, left);
        if self.persona == Persona::Both {
            return;
        }
        if right != was_right && right != 0 {
            self.set_persona(Persona::Machining);
        } else if left != was_left && left != 0 {
            self.set_persona(Persona::Design);
        }
    }

    /// Shows one workspace and puts the other's panels away.
    pub fn set_persona(&mut self, persona: Persona) {
        if self.persona != persona {
            self.auto_fit = true;
        }
        self.persona = persona;
        match persona {
            Persona::Design => self.left_open = true,
            Persona::Machining => self.right_open = true,
            Persona::Both => {}
        }
    }

    /// The two workspace buttons, at the far left as in Affinity.
    fn persona_switch(&mut self, ui: &mut Ui) {
        let both = [(Persona::Design, Icon::Curve, crate::i18n::tr("Design"), crate::i18n::tr("Draw and model: shapes, text, 3D")), (Persona::Machining, Icon::TpPocket, crate::i18n::tr("Machining"), crate::i18n::tr("Toolpaths, preview and machine files"))];
        let (outer, _) = ui.allocate_exact_size(vec2(0.0, 26.0), Sense::hover());
        let mut x = outer.left();
        let mut rects = Vec::new();
        for (_, _, label, _) in both {
            let w = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), TEXT()).size().x + 34.0;
            rects.push(Rect::from_min_size(pos2(x, outer.top()), vec2(w, 26.0)));
            x += w;
        }
        let whole = Rect::from_min_max(rects[0].min, rects[rects.len() - 1].max);
        ui.allocate_rect(whole, Sense::hover());
        ui.painter().rect(whole, 7.0, CARD(), Stroke::new(1.0, LINE()), StrokeKind::Inside);
        for ((persona, icon, label, tip), r) in both.into_iter().zip(rects) {
            let resp = ui.interact(r, ui.scope_id().with(("persona", label)), Sense::click()).on_hover_text(tip);
            let on = self.persona == persona;
            // Each workspace in its own colour: drawing in the logo's orange, machining in the gold of its top.
            let hue = if persona == Persona::Machining { theme::HUE_AMBER() } else { theme::HUE_ORANGE() };
            if on {
                ui.painter().rect_filled(r.shrink(2.0), 5.0, theme::hue_soft(hue));
                ui.painter().rect_stroke(r.shrink(2.0), 5.0, Stroke::new(1.0, hue.gamma_multiply(0.6)), StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(r.shrink(2.0), 5.0, LINE());
            }
            icons::set_tint(Some(hue));
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 15.0, r.center().y), vec2(18.0, 18.0)), icon, true);
            icons::set_tint(None);
            ui.painter().text(pos2(r.left() + 27.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), if on && !theme::light() { Color32::WHITE } else { TEXT() });
            label_widget(&resp, label);
            if resp.clicked() {
                self.set_persona(persona);
            }
        }
    }

    /// The sections of the left panel as a strip of icons along the window's edge, as drawing
    /// programs such as Affinity keep theirs, rather than tabs under the panel. Clicking the open
    /// one folds the panel away; any other opens it there. Burin's own.
    fn section_rail(&mut self, ui: &mut Ui) {
        if self.persona == Persona::Machining {
            self.machining_rail(ui);
            return;
        }
        egui::Panel::left("rail").exact_size(RAIL_W).resizable(false).frame(Frame::new().fill(CHROME())).show(ui, |ui| {
            let whole = ui.max_rect();
            let specs = [
                (LeftTab::Drawing, Icon::Curve, crate::i18n::tr("Draw"), theme::HUE_ORANGE()),
                (LeftTab::Modeling, Icon::Dome, crate::i18n::tr("3D Model"), theme::HUE_AMBER()),
                (LeftTab::Clipart, Icon::ImportModel, crate::i18n::tr("Library"), theme::HUE_VIOLET()),
                (LeftTab::Layers, Icon::Layer, crate::i18n::tr("Layers"), theme::HUE_BLUE()),
            ];
            let mut y = whole.top() + 8.0;
            for (tab, icon, label, hue) in specs {
                let r = Rect::from_min_size(pos2(whole.left() + 4.0, y), vec2(RAIL_W - 8.0, RAIL_W - 8.0));
                let resp = ui.interact(r, ui.make_persistent_id(("rail", label)), Sense::click());
                let active = self.left_open && self.left_tab == tab;
                if active {
                    ui.painter().rect_filled(r, 8.0, theme::hue_soft(hue));
                    ui.painter().line_segment([pos2(whole.left() + 1.0, r.top() + 6.0), pos2(whole.left() + 1.0, r.bottom() - 6.0)], Stroke::new(3.0, hue));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, theme::BUTTON());
                }
                crate::icons::set_tint(Some(hue));
                crate::icons::paint(ui.painter(), r.shrink(7.0), icon, true);
                crate::icons::set_tint(None);
                label_widget(&resp, label);
                let resp = resp.on_hover_text(label);
                if resp.clicked() {
                    if active {
                        self.left_open = false;
                    } else {
                        self.left_open = true;
                        self.left_tab = tab;
                        self.close_left_form();
                    }
                }
                y += RAIL_W - 4.0;
            }
        });
    }

    /// The same strip in Machining, for the toolpath panel that sits on the left there: the list,
    /// the preview and saving machine files.
    fn machining_rail(&mut self, ui: &mut Ui) {
        egui::Panel::left("rail").exact_size(RAIL_W).resizable(false).frame(Frame::new().fill(CHROME())).show(ui, |ui| {
            let whole = ui.max_rect();
            let specs = [
                (0, Icon::TpPocket, crate::i18n::tr("Toolpath list"), theme::HUE_AMBER()),
                (1, Icon::Preview, crate::i18n::tr("Toolpath preview"), theme::HUE_BLUE()),
                (2, Icon::SaveTp, crate::i18n::tr("Machine files"), theme::HUE_GREEN()),
            ];
            let showing = match self.right {
                RightMode::Preview => 1,
                RightMode::Save => 2,
                _ => 0,
            };
            let mut y = whole.top() + 8.0;
            for (which, icon, label, hue) in specs {
                let r = Rect::from_min_size(pos2(whole.left() + 4.0, y), vec2(RAIL_W - 8.0, RAIL_W - 8.0));
                let resp = ui.interact(r, ui.make_persistent_id(("rail-machining", label)), Sense::click());
                let active = self.right_open && showing == which;
                if active {
                    ui.painter().rect_filled(r, 8.0, theme::hue_soft(hue));
                    ui.painter().line_segment([pos2(whole.left() + 1.0, r.top() + 6.0), pos2(whole.left() + 1.0, r.bottom() - 6.0)], Stroke::new(3.0, hue));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, theme::BUTTON());
                }
                crate::icons::set_tint(Some(hue));
                crate::icons::paint(ui.painter(), r.shrink(7.0), icon, true);
                crate::icons::set_tint(None);
                label_widget(&resp, label);
                let resp = resp.on_hover_text(label);
                if resp.clicked() {
                    if active {
                        self.right_open = false;
                    } else {
                        self.right_open = true;
                        match which {
                            1 => {
                                self.right = RightMode::Preview;
                                self.view3d = true;
                            }
                            2 => self.right = RightMode::Save,
                            // Back to the list, unless a form is half filled in.
                            _ if !matches!(self.right, RightMode::Op { .. } | RightMode::Material(_)) => self.right = RightMode::List,
                            _ => {}
                        }
                    }
                }
                y += RAIL_W - 4.0;
            }
        });
    }

    fn left_panel(&mut self, ui: &mut Ui) {
        self.section_rail(ui);
        if self.persona == Persona::Machining {
            return;
        }
        if !self.left_open {
            return;
        }
        let job_open = matches!(self.left, LeftForm::Job { .. });
        egui::Panel::left("left").exact_size(SIDE_W).resizable(false).frame(Frame::new().fill(PANEL())).show(ui, |ui| {
            let whole = ui.max_rect();
            if job_open {
                self.paint_ornament(ui, whole, whole.top() + 660.0);
                let inner = whole.shrink2(vec2(10.0, 8.0));
                let mut body = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                self.job_setup(&mut body);
                return;
            }
            let header = Rect::from_min_size(whole.min, vec2(whole.width(), 22.0));
            let body = Rect::from_min_max(pos2(whole.left(), header.bottom()), whole.max);
            let title = match self.left_tab {
                LeftTab::Drawing => crate::i18n::tr("Draw"),
                LeftTab::Modeling => crate::i18n::tr("3D Model"),
                LeftTab::Clipart => crate::i18n::tr("Library"),
                LeftTab::Layers => crate::i18n::tr("Layers"),
            };
            self.panel_header(ui, header, title, true);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(vec2(10.0, 6.0))));
            egui::ScrollArea::vertical().auto_shrink(false).show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let form_open = matches!(self.left, LeftForm::Draw(_) | LeftForm::Xform(_) | LeftForm::Shape(_) | LeftForm::Trace(_) | LeftForm::Relief(_));
                if form_open {
                    self.left_form(ui);
                } else {
                    match self.left_tab {
                        LeftTab::Drawing => self.drawing_tab(ui),
                        LeftTab::Modeling => self.modeling_tab(ui),
                        LeftTab::Clipart => self.clipart_tab(ui),
                        LeftTab::Layers => self.layers_tab(ui),
                    }
                }
            });
            crate::icons::set_tint(None);
        });
    }

    fn collapsed_strip(&mut self, ui: &mut Ui, left: bool) {
        let panel = if left { egui::Panel::left("left_strip") } else { egui::Panel::right("right_strip") };
        let label = if left { crate::i18n::tr("Draw") } else { crate::i18n::tr("Toolpaths") };
        panel.exact_size(22.0).resizable(false).frame(Frame::new().fill(CHROME())).show(ui, |ui| {
            let r = ui.max_rect();
            let tab = Rect::from_min_size(r.min + vec2(1.0, 6.0), vec2(20.0, 70.0));
            let resp = ui.interact(tab, ui.scope_id().with("open"), Sense::click());
            ui.painter().rect(tab, 4.0, CARD(), Stroke::NONE, StrokeKind::Inside);
            let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.0), TEXT());
            let at = tab.left_top() + vec2(16.0, 6.0);
            ui.painter().add(egui::epaint::TextShape::new(at, galley, TEXT()).with_angle(std::f32::consts::FRAC_PI_2));
            label_widget(&resp, label);
            if resp.clicked() {
                if left {
                    self.left_open = true;
                } else {
                    self.right_open = true;
                }
                self.auto_fit = true;
            }
        });
    }

    fn panel_header(&mut self, ui: &mut Ui, rect: Rect, title: &str, left: bool) {
        ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(12.5), TEXT());
        let b = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(16.0, 16.0));
        let name = if left {
            crate::i18n::tr("Hide left panel")
        } else if self.persona == Persona::Machining {
            crate::i18n::tr("Hide the toolpath panel")
        } else {
            crate::i18n::tr("Hide right panel")
        };
        let resp = ui.interact(b, ui.scope_id().with(name), Sense::click()).on_hover_text(name);
        icons::paint(ui.painter(), b, Icon::Collapse, true);
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), vec2(12.0, 12.0)), Icon::Pin, true);
        label_widget(&resp, name);
        if resp.clicked() {
            if left {
                self.left_open = false;
            } else {
                self.right_open = false;
            }
            self.auto_fit = true;
        }
    }

    pub fn open_job_setup(&mut self) {
        // Already open: keep the state it started from, so Cancel still goes all the way back.
        if !matches!(self.left, LeftForm::Job { .. }) {
            self.left = LeftForm::Job { backup: Some(Box::new(self.doc.clone())), first: false };
        }
        self.left_open = true;
        self.auto_fit = true;
        self.view3d = false;
    }

    fn job_setup(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.job_setup_body(ui));
    }

    fn job_setup_body(&mut self, ui: &mut Ui) {
        // Burin's own layout: the board drawn large, with its sizes along its edges and the zero
        // picked on it, then a few rows of pills and boxes, the rarer settings last.
        Frame::new().fill(PANEL()).corner_radius(6).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::JobSetup, true);
                bold_sized(ui, crate::i18n::tr("Stock & origin"), 15.0, TEXT());
            });
            let unit = self.doc.job.unit();
            let mut size_changed = false;
            self.stock_picture(ui);
            // Size, in one row, with the units beside it.
            icon_caps(ui, Icon::Measure, crate::i18n::tr("Size"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (k, caption) in [(0, crate::i18n::tr("width")), (1, crate::i18n::tr("depth")), (2, crate::i18n::tr("thickness"))] {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        let value = match k {
                            0 => &mut self.doc.job.width,
                            1 => &mut self.doc.job.height,
                            _ => &mut self.doc.job.thickness,
                        };
                        size_changed |= number(ui, value, 50.0);
                        ui.label(RichText::new(caption).size(10.5).color(MUTED()));
                    });
                    if k < 2 {
                        ui.label(RichText::new("×").color(MUTED()));
                    }
                }
            });
            // What the board lies on stays in sight: it decides when a cut too deep is warned about.
            if labelled(ui, crate::i18n::tr("Sacrificial board"), 0.0, &mut self.doc.job.spoil_board, 44.0, unit) {
                self.doc.job.spoil_board = self.doc.job.spoil_board.max(0.0);
            }
            ui.label(RichText::new(crate::i18n::tr("Cutting into it brings no warning; going past it does.")).size(11.0).color(MUTED()));
            ui.horizontal(|ui| {
                icon_caps(ui, Icon::Grid, crate::i18n::tr("Units"));
                ui.add_space(8.0);
                let inches = self.doc.job.units == Units::Inches;
                match pills(ui, &["mm", crate::i18n::tr("inches")], if inches { 1 } else { 0 }) {
                    Some(0) if inches => {
                        self.doc.set_units(Units::Mm);
                        size_changed = true;
                    }
                    Some(1) if !inches => {
                        self.doc.set_units(Units::Inches);
                        size_changed = true;
                    }
                    _ => {}
                }
            });
            ui.horizontal(|ui| {
                icon_caps(ui, Icon::SideTop, crate::i18n::tr("Faces"));
                ui.add_space(8.0);
                if let Some(k) = pictured_pills(ui, &[(Some(Pic::OneFace), crate::i18n::tr("One face")), (Some(Pic::TwoFaces), crate::i18n::tr("Two faces"))], usize::from(self.doc.job.two_sided)) {
                    self.doc.job.two_sided = k == 1;
                }
            });
            if self.doc.job.two_sided {
                ui.horizontal(|ui| {
                    icon_caps(ui, if self.doc.job.flip_around_y { Icon::FlipAroundY } else { Icon::FlipAroundX }, crate::i18n::tr("Turned"));
                    ui.add_space(8.0);
                    let around_y = self.doc.job.flip_around_y;
                    if let Some(k) = pictured_pills(ui, &[(Some(Pic::FlipLeftRight), crate::i18n::tr("left to right")), (Some(Pic::FlipTopBottom), crate::i18n::tr("top to bottom"))], if around_y { 0 } else { 1 }) {
                        if (k == 0) != around_y {
                            self.change_flip_direction();
                        }
                    }
                });
            }
            // The rarer settings, folded away at first.
            egui::CollapsingHeader::new(RichText::new(crate::i18n::tr("More settings")).size(12.0).color(MUTED())).id_salt("stock-more").default_open(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.checkbox(&mut self.doc.job.use_offset, crate::i18n::tr("Move the origin by"));
                ui.add_enabled_ui(self.doc.job.use_offset, |ui| {
                    ui.horizontal(|ui| {
                        labelled(ui, "X", 14.0, &mut self.doc.job.offset[0], 46.0, "");
                        labelled(ui, "Y", 14.0, &mut self.doc.job.offset[1], 46.0, "");
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("3D detail"));
                    let before = self.doc.job.resolution;
                    egui::ComboBox::from_id_salt("res").selected_text(tr(RESOLUTIONS[before.min(3)].0)).width(150.0).show_ui(ui, |ui| {
                        for (i, r) in RESOLUTIONS.iter().enumerate() {
                            ui.selectable_value(&mut self.doc.job.resolution, i, tr(r.0));
                        }
                    });
                    if self.doc.job.resolution != before {
                        self.model_dirty = true;
                    }
                });
                ui.label(RichText::new(tr(RESOLUTIONS[self.doc.job.resolution.min(3)].1)).size(11.0).color(MUTED()));
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Looks like"));
                    let name = self.doc.job.material.map_or(crate::i18n::tr("Solid Color"), |m| tr(MATERIALS[m.min(4)].0));
                    egui::ComboBox::from_id_salt("appearance").selected_text(name).width(130.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.doc.job.material, None, crate::i18n::tr("Solid Color"));
                        for (i, m) in MATERIALS.iter().enumerate() {
                            ui.selectable_value(&mut self.doc.job.material, Some(i), tr(m.0));
                        }
                    });
                    if self.doc.job.material.is_none() {
                        ui.color_edit_button_srgb(&mut self.doc.job.solid);
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if wide(ui, crate::i18n::tr("Cancel"), 74.0).clicked() {
                    self.cancel_job();
                }
                ui.add_space((ui.available_width() - 76.0).max(0.0));
                if primary(ui, "OK", 74.0).clicked() {
                    self.confirm_job();
                }
            });
            if size_changed {
                self.auto_fit = true;
                self.last_rect = Rect::NOTHING;
                self.sim = None;
                self.model_dirty = true;
            }
        });
    }

    /// The board drawn large in the logo's colours, its sizes written along its edges. The zero
    /// on the sheet is picked by clicking a corner or the middle of its top, and the height zero
    /// by clicking the top or the foot of its front edge.
    fn stock_picture(&mut self, ui: &mut Ui) {
        let w = ui.available_width();
        let job = &self.doc.job;
        // The board in proportion, a thin one drawn thin (but never thinner than the two height
        // spots need): its depth goes back at an angle, shortened, as on a drawing of a part.
        let big = job.width.max(job.height).max(1e-9);
        let (a, b) = ((job.width / big) as f32, (job.height / big) as f32);
        let (dx, dy) = (0.7 * 40f32.to_radians().cos(), -0.7 * 40f32.to_radians().sin());
        let (left, right, top, bottom) = (30.0, 64.0, 16.0, 38.0);
        let s = (w - left - right) / (a + b * dx);
        let t = ((job.thickness / big) as f32).clamp(12.0 / s, 0.3);
        let raw = |x: f32, y: f32, z: f32| vec2(x + y * dx, y * dy - z);
        let tall = (b * -dy + t) * s;
        let (area, _) = ui.allocate_exact_size(vec2(w, tall + top + bottom), Sense::hover());
        let p = ui.painter_at(area);
        p.rect_filled(area, 8.0, theme::CARD());
        let offset = pos2(area.left() + left, area.top() + top + b * -dy * s + t * s);
        let at = |x: f32, y: f32, z: f32| offset + raw(x, y, z) * s;
        let edge = Stroke::new(1.0, Color32::from_rgb(90, 28, 0));
        let face = |pts: [egui::Pos2; 4], fill: Color32| {
            p.add(Shape::convex_polygon(pts.to_vec(), fill, edge));
        };
        face([at(0.0, 0.0, 0.0), at(a, 0.0, 0.0), at(a, 0.0, t), at(0.0, 0.0, t)], Color32::from_rgb(255, 122, 0));
        face([at(a, 0.0, 0.0), at(a, b, 0.0), at(a, b, t), at(a, 0.0, t)], Color32::from_rgb(224, 74, 0));
        face([at(0.0, 0.0, t), at(a, 0.0, t), at(a, b, t), at(0.0, b, t)], Color32::from_rgb(255, 180, 0));
        let unit = job.unit();
        let size = |v: f64| format!("{} {unit}", crate::toolpath::trim(v));
        let caption = |pos: egui::Pos2, text: String, align: Align2| {
            p.text(pos, align, text, FontId::proportional(11.5), TEXT());
        };
        // Width under the front, depth beside the receding right side, thickness right of the
        // front right edge.
        caption(at(a / 2.0, 0.0, 0.0) + vec2(0.0, 3.0), size(job.width), Align2::CENTER_TOP);
        let x_label = area.right() - right + 8.0;
        for (edge_mid, y, text) in [(at(a, b * 0.5, t), at(a, b * 0.75, t).y - 6.0, size(job.height)), (at(a + b * dx * 0.5, b * 0.5, t * 0.5) - vec2(b * dx * 0.5 * s, 0.0), at(a, b * 0.15, 0.0).y + 6.0, size(job.thickness))] {
            let label_at = pos2(x_label, y);
            p.line_segment([edge_mid, label_at - vec2(3.0, 0.0)], Stroke::new(1.0, MUTED()));
            caption(label_at, text, Align2::LEFT_CENTER);
        }
        let (edge_top, edge_foot) = (at(0.0, 0.0, t), at(0.0, 0.0, 0.0));
        // The height zero: level with the top or the foot of the front edge, just left of it.
        let z_on_bed = job.z_on_bed;
        let rail = edge_top.x - 16.0;
        p.line_segment([pos2(rail, edge_top.y), pos2(rail, edge_foot.y)], Stroke::new(1.0, MUTED()));
        for (spot, on_bed, name) in [(edge_top, false, "Z zero on the material surface"), (edge_foot, true, "Z zero on the machine bed")] {
            let dot = pos2(rail, spot.y);
            p.line_segment([dot, spot], Stroke::new(1.0, MUTED()));
            let resp = ui.interact(Rect::from_center_size(dot, vec2(16.0, 16.0)), ui.scope_id().with(name), Sense::click());
            let chosen = z_on_bed == on_bed;
            p.circle(dot, if chosen { 5.0 } else { 4.0 }, if chosen { ACCENT } else { INPUT() }, Stroke::new(1.0, if resp.hovered() { TEXT() } else { MUTED() }));
            label_widget(&resp, crate::i18n::tr(name));
            if resp.on_hover_text(crate::i18n::tr(name)).clicked() {
                self.doc.job.z_on_bed = on_bed;
            }
        }
        // The zero on the sheet: four corners and the middle of the top.
        let datum = self.doc.job.datum;
        for (u, v, d) in [(0.0, 0.0, Datum::BottomLeft), (1.0, 0.0, Datum::BottomRight), (0.0, 1.0, Datum::TopLeft), (1.0, 1.0, Datum::TopRight), (0.5, 0.5, Datum::Centre)] {
            let spot = at(u * a, v * b, t);
            let resp = ui.interact(Rect::from_center_size(spot, vec2(18.0, 18.0)), ui.scope_id().with(("datum", format!("{d:?}"))), Sense::click());
            if datum == d {
                p.circle_filled(spot, 7.0, theme::ON_ACCENT);
                p.circle_stroke(spot, 7.0, Stroke::new(2.0, Color32::WHITE));
                p.circle_filled(spot, 3.0, Color32::WHITE);
            } else {
                p.circle(spot, 4.5, Color32::from_white_alpha(if resp.hovered() { 230 } else { 150 }), Stroke::new(1.0, theme::ON_ACCENT));
            }
            label_widget(&resp, &format!("Datum {d:?}"));
            if resp.on_hover_text(crate::i18n::tr("Put the origin here")).clicked() {
                self.doc.job.datum = d;
            }
        }
        let where_ = match self.doc.job.datum {
            Datum::BottomLeft => crate::i18n::tr("front left"),
            Datum::BottomRight => crate::i18n::tr("front right"),
            Datum::TopLeft => crate::i18n::tr("back left"),
            Datum::TopRight => crate::i18n::tr("back right"),
            Datum::Centre => crate::i18n::tr("middle"),
        };
        let height = if self.doc.job.z_on_bed { crate::i18n::tr("on the bed") } else { crate::i18n::tr("on top") };
        p.text(area.left_bottom() + vec2(8.0, -6.0), Align2::LEFT_BOTTOM, crate::i18n::tr("Origin: {where}, Z {height}").replace("{where}", where_).replace("{height}", height), FontId::proportional(11.0), MUTED());
    }

    pub fn confirm_job(&mut self) {
        let j = &mut self.doc.job;
        j.width = j.width.max(0.01);
        j.height = j.height.max(0.01);
        j.thickness = j.thickness.max(0.001);
        // A new job's material is kept for the next new job (its 3D detail only below the
        // highest), as the original does (`0x140332cc0`).
        if matches!(self.left, LeftForm::Job { first: true, .. }) {
            remember_new_job(&self.doc.job);
        }
        // Editing an existing job is one step of undo, and what it affects gets pointed out.
        if let LeftForm::Job { backup: Some(before), .. } = &self.left {
            let (old, new) = (&before.job, &self.doc.job);
            if serde_json::to_string(old).ok() != serde_json::to_string(new).ok() {
                let (w, h) = (new.width, new.height);
                let outside = crate::geom::bbox(self.doc.vectors.iter().flat_map(|v| v.paths.iter())).is_some_and(|(lo, hi)| lo[0] < -1e-6 || lo[1] < -1e-6 || hi[0] > w + 1e-6 || hi[1] > h + 1e-6);

                self.status = if outside { crate::i18n::tr("Job changed: some shapes are now outside the job.").into() } else { crate::i18n::tr("Job changed.").into() };
                let LeftForm::Job { backup: Some(before), .. } = std::mem::replace(&mut self.left, LeftForm::None) else { unreachable!() };
                self.material_accepted(&before);
                self.undo.push(*before);
                if self.undo.len() > 60 {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.dirty = true;
            }
        }
        self.left = LeftForm::None;
        self.auto_fit = true;
        self.last_rect = Rect::NOTHING;
        self.model_dirty = true;
        self.sim = None;
        self.bump_surface();
    }

    pub fn cancel_job(&mut self) {
        match std::mem::replace(&mut self.left, LeftForm::None) {
            LeftForm::Job { backup: Some(doc), .. } => {
                self.doc = *doc;
                self.auto_fit = true;
                self.last_rect = Rect::NOTHING;
                self.model_dirty = true;
                self.sim = None;
            }
            LeftForm::Job { first: true, .. } => self.screen = Screen::Start,
            other => self.left = other,
        }
    }

    pub(crate) fn start_draw(&mut self, kind: DrawKind) {
        self.tool = Tool2::Select;
        let mut form = DrawForm::new(kind, &self.doc.job);
        // With one shape of this kind selected, the tool edits it rather than making another.
        let chosen = match self.selected.as_slice() {
            [only] => self.doc.vectors.iter().find(|v| v.id == *only),
            _ => None,
        };
        // A text keeps the shapes chosen before it: it can be fitted into them.
        if !chosen.is_some_and(|v| form.load_shape(v)) && kind != DrawKind::Text {
            self.selected.clear();
        }
        self.left = LeftForm::Draw(form);
        self.view3d = false;
        self.last_draw = Some(kind);
    }









    fn left_form(&mut self, ui: &mut Ui) {
        match self.left.clone() {
            LeftForm::Draw(f) => self.draw_form(ui, f),
            LeftForm::Xform(f) => self.xform_form(ui, f),
            LeftForm::Shape(f) => self.shape_form(ui, f),
            LeftForm::Trace(f) => self.trace_form(ui, *f),
            LeftForm::Relief(f) => self.relief_form_ui(ui, *f),
            _ => {}
        }
    }

    pub(crate) fn form_buttons(&mut self, ui: &mut Ui, apply: &str) -> bool {
        let mut pressed = false;
        ui.horizontal(|ui| {
            pressed = primary(ui, apply, 74.0).clicked();
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                self.left = LeftForm::None;
            }
        });
        pressed
    }

    fn draw_form(&mut self, ui: &mut Ui, mut f: DrawForm) {
        if matches!(f.kind, DrawKind::Polyline | DrawKind::Curve) {
            self.line_form(ui, f);
            return;
        }
        let icon = match f.kind {
            DrawKind::Circle => Icon::Circle,
            DrawKind::Ellipse => Icon::Ellipse,
            DrawKind::Rect => Icon::Rect,
            DrawKind::Polygon => Icon::Polygon,
            DrawKind::Star => Icon::Star,
            DrawKind::Polyline => Icon::Lines,
            DrawKind::Arc => Icon::Arc,
            DrawKind::Curve => Icon::Curve,
            DrawKind::Spiral => Icon::Spiral,
            DrawKind::Text => Icon::Text,
        };
        let editing = f.editing.is_some_and(|id| self.doc.vectors.iter().any(|v| v.id == id));
        // Burin's own head: the shape's icon and its name, and whether a shape is being changed.
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            icons::paint(ui.painter(), r, icon, true);
            let name = match f.kind {
                DrawKind::Circle => crate::i18n::tr("Circle"),
                DrawKind::Ellipse => crate::i18n::tr("Ellipse"),
                DrawKind::Rect => crate::i18n::tr("Rectangle"),
                DrawKind::Polygon => crate::i18n::tr("Polygon"),
                DrawKind::Star => crate::i18n::tr("Star"),
                DrawKind::Polyline => crate::i18n::tr("Lines"),
                DrawKind::Arc => crate::i18n::tr("Arc"),
                DrawKind::Curve => crate::i18n::tr("Curve"),
                DrawKind::Spiral => crate::i18n::tr("Spiral"),
                DrawKind::Text => crate::i18n::tr("Text"),
            };
            bold_sized(ui, name, 15.0, TEXT());
            if editing {
                ui.label(RichText::new(crate::i18n::tr("changing it")).size(11.0).color(ACCENT));
            }
        });
        let unit = self.doc.job.unit();
        let line_tool = matches!(f.kind, DrawKind::Polyline | DrawKind::Curve);
        if line_tool {
            card(ui, |ui| {
                ui.label(crate::i18n::tr("Click on the sheet to add points."));
                ui.label(crate::i18n::tr("Double click or press Enter to finish."));
                ui.label(crate::i18n::tr("Click the first point to close the shape."));
                ui.add_space(4.0);
                ui.label(tf!("Points: {}", f.points.len()));
            });
        } else {
            shape_preview(ui, &f);
            card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                icon_caps(ui, Icon::Move, if f.kind == DrawKind::Text { crate::i18n::tr("Starts at") } else { crate::i18n::tr("Middle at") });
                let zero = self.doc.job.origin();
                ui.horizontal(|ui| {
                    ui.label("X");
                    number_from(ui, &mut f.c[0], zero[0], 54.0);
                    ui.add_space(4.0);
                    ui.label("Y");
                    number_from(ui, &mut f.c[1], zero[1], 54.0);
                });
                ui.add_space(4.0);
                if f.kind == DrawKind::Text {
                    icon_caps(ui, Icon::Text, crate::i18n::tr("Text"));
                } else {
                    icon_caps(ui, Icon::Measure, crate::i18n::tr("Size"));
                }
                match f.kind {
                    DrawKind::Circle => {
                        if let Some(k) = pictured_pills(ui, &[(Some(Pic::Radius), crate::i18n::tr("Radius")), (Some(Pic::Diameter), crate::i18n::tr("Diameter"))], usize::from(f.use_dia)) {
                            f.use_dia = k == 1;
                        }
                        ui.horizontal(|ui| {
                            let mut shown = if f.use_dia { f.r * 2.0 } else { f.r };
                            if number(ui, &mut shown, 54.0) {
                                f.r = if f.use_dia { shown / 2.0 } else { shown };
                            }
                            ui.label(unit);
                        });
                    }
                    DrawKind::Ellipse => {
                        labelled(ui, crate::i18n::tr("Across"), 54.0, &mut f.rx, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Up"), 54.0, &mut f.ry, 54.0, unit);
                    }
                    DrawKind::Rect => {
                        labelled(ui, crate::i18n::tr("Width"), 54.0, &mut f.w, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Height"), 54.0, &mut f.h, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Rounded corners"), 54.0, &mut f.corner, 54.0, unit);
                    }
                    DrawKind::Polygon => {
                        labelled(ui, crate::i18n::tr("Radius"), 54.0, &mut f.r, 54.0, unit);
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Sides"));
                            count_box(ui, &mut f.sides, 54.0);
                        });
                    }
                    DrawKind::Star => {
                        labelled(ui, crate::i18n::tr("Tips at"), 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Hollows at"), 54.0, &mut f.inner, 54.0, unit);
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Points"));
                            count_box(ui, &mut f.sides, 54.0);
                        });
                    }
                    DrawKind::Spiral => {
                        labelled(ui, crate::i18n::tr("Radius"), 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Turns"), 54.0, &mut f.turns, 54.0, "");
                    }
                    DrawKind::Arc => {
                        labelled(ui, crate::i18n::tr("Radius"), 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, crate::i18n::tr("Starts at angle"), 54.0, &mut f.start, 54.0, crate::i18n::tr("degrees"));
                        labelled(ui, crate::i18n::tr("Goes round"), 54.0, &mut f.sweep, 54.0, crate::i18n::tr("degrees"));
                    }
                    DrawKind::Text => {
                        ui.add(TextEdit::multiline(&mut f.text).desired_rows(2).desired_width(f32::INFINITY));
                        crate::fonts::style_picker(ui, &mut f.text_style);
                        labelled(ui, crate::i18n::tr("Height"), 54.0, &mut f.text_height, 54.0, unit);
                        // Filling a shape: the selected shapes that aren't this text.
                        let target = geom::bbox(self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id) && Some(v.id) != f.editing && !matches!(v.prim, Prim::Text { .. })).flat_map(|v| v.paths.iter()));
                        ui.horizontal(|ui| {
                            let fit = ui.add_enabled(target.is_some(), egui::Button::new(crate::i18n::tr("Fit to Selected Shape"))).on_disabled_hover_text(crate::i18n::tr("Select the shape to fill first."));
                            number(ui, &mut f.fill_percent, 40.0);
                            ui.label("%");
                            if fit.clicked() {
                                let layout = crate::fonts::BoxLayout { percent: f.fill_percent, margin: f.text_margin, spread_letters: f.spread_letters, centre_across: false };
                                if let Some((at, height, spacing)) = target.and_then(|(lo, hi)| crate::fonts::lay_out_in_box(&f.text, &f.text_style, lo, hi, &layout)) {
                                    f.c = at;
                                    f.text_height = height;
                                    f.text_style.spacing = spacing;
                                }
                            }
                        });
                        ui.label(crate::i18n::tr("Margin Size"));
                        {
                            use crate::fonts::Margin;
                            let margins = [Margin::None, Margin::Normal, Margin::Wide];
                            let at = margins.iter().position(|m| *m == f.text_margin).unwrap_or(1);
                            if let Some(k) = pictured_pills(ui, &[(Some(Pic::MarginNone), crate::i18n::tr("None")), (Some(Pic::MarginNormal), crate::i18n::tr("Normal")), (Some(Pic::MarginWide), crate::i18n::tr("Wide"))], at) {
                                f.text_margin = margins[k];
                            }
                        }
                        ui.checkbox(&mut f.spread_letters, crate::i18n::tr("Spread the letters to the width"));
                    }
                    _ => {}
                }
            });
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Select, true);
                let hint = if editing { crate::i18n::tr("Changes go to the selected shape. Draw on the sheet for a new one.") } else { crate::i18n::tr("Shift-click a shape to change it here.") };
                ui.add(egui::Label::new(RichText::new(hint).size(11.5).color(MUTED())).wrap());
            });
        }
        // Keep typed values, then apply them.
        let same_kind = matches!(&self.left, LeftForm::Draw(now) if now.kind == f.kind);
        if same_kind {
            if let LeftForm::Draw(now) = &self.left {
                f.points = now.points.clone();
            }
            self.left = LeftForm::Draw(f);
        }
        if self.form_buttons(ui, crate::i18n::tr("Apply")) {
            self.apply_draw();
        }
    }


    fn shape_form(&mut self, ui: &mut Ui, mut f: ShapeForm) {
        // Burin's own layout: a cut through the shape, then pictured choices.
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Dome, true);
            bold_sized(ui, if f.editing.is_some() { crate::i18n::tr("Changing a 3D shape") } else { crate::i18n::tr("3D shape") }, 15.0, TEXT());
        });
        let unit = self.doc.job.unit();
        section_preview(ui, f.profile, f.height, f.base, unit);
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            icon_caps(ui, Icon::Dome, crate::i18n::tr("Shape across"));
            let profiles = [Profile::Round, Profile::Angular, Profile::Flat];
            let at = profiles.iter().position(|p| *p == f.profile).unwrap_or(0);
            if let Some(k) = pictured_pills(ui, &[(Some(Pic::ProfileRound), crate::i18n::tr("Round")), (Some(Pic::ProfileAngular), crate::i18n::tr("Angular")), (Some(Pic::ProfileFlat), crate::i18n::tr("Flat"))], at) {
                f.profile = profiles[k];
            }
            labelled(ui, crate::i18n::tr("Height"), 0.0, &mut f.height, 54.0, unit);
            labelled(ui, crate::i18n::tr("On a base of"), 0.0, &mut f.base, 54.0, unit);
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            icon_caps(ui, Icon::Layer, crate::i18n::tr("Combine with the model"));
            let ways = [Combine::Add, Combine::Subtract, Combine::Merge, Combine::Low];
            let at = ways.iter().position(|c| *c == f.combine).unwrap_or(0);
            if let Some(k) = pictured_pills(ui, &[(Some(Pic::CombineAdd), crate::i18n::tr("Add")), (Some(Pic::CombineSubtract), crate::i18n::tr("Subtract")), (Some(Pic::CombineHighest), crate::i18n::tr("Keep highest")), (Some(Pic::CombineLowest), crate::i18n::tr("Keep lowest"))], at) {
                f.combine = ways[k];
            }
            explain(ui, [Pic::CombineAdd, Pic::CombineSubtract, Pic::CombineHighest, Pic::CombineLowest][at], match f.combine {
                Combine::Add => crate::i18n::tr("Stacks on top of what's there."),
                Combine::Subtract => crate::i18n::tr("Digs into what's there."),
                Combine::Merge => crate::i18n::tr("Keeps whichever is higher at each point."),
                Combine::Low => crate::i18n::tr("Keeps whichever is lower at each point."),
            });
        });
        if f.editing.is_none() {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Select, true);
                ui.label(tf!("Selected shapes: {}", self.selected.len()));
            });
        }
        self.left = LeftForm::Shape(f.clone());
        if self.form_buttons(ui, crate::i18n::tr("Apply")) {
            self.add_component(f);
        }
    }

    // ----- Right panel -----

    fn right_panel(&mut self, ui: &mut Ui) {
        if self.persona == Persona::Design {
            return;
        }
        // In Machining the toolpath panel sits on the left, by the strip that opens it again.
        let on_left = self.persona == Persona::Machining;
        if !self.right_open {
            if !on_left {
                self.collapsed_strip(ui, false);
            }
            return;
        }
        let panel = if on_left { egui::Panel::left("toolpaths-left") } else { egui::Panel::right("right") };
        panel.exact_size(RIGHT_W).resizable(false).frame(Frame::new().fill(PANEL())).show(ui, |ui| {
            let whole = ui.max_rect();
            let header = Rect::from_min_size(whole.min, vec2(whole.width(), 22.0));
            self.panel_header(ui, header, crate::i18n::tr("Toolpaths"), false);
            let body = Rect::from_min_max(pos2(whole.left(), header.bottom()), whole.max);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            match self.right.clone() {
                RightMode::List => {
                    self.toolpath_ops(&mut child);
                    self.toolpath_list(&mut child);
                }
                RightMode::Op { op, editing, tab, error } => self.op_form(&mut child, *op, editing, tab, error),
                RightMode::Save => {
                    self.save_form(&mut child);
                    self.toolpath_list(&mut child);
                }
                RightMode::Preview => {
                    self.preview_panel(&mut child);
                    self.toolpath_list(&mut child);
                }
                RightMode::Material(backup) => self.material_form(&mut child, backup),
            }
            crate::icons::set_tint(None);
        });
    }


    fn toolpath_list(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(3.0, LINE()));
        let width = rect.width();
        ui.add_space(6.0);
        Frame::new().inner_margin(Margin { left: 12, right: 2, top: 2, bottom: 2 }).show(ui, |ui| {
            ui.set_width(width - 14.0);
            ui.horizontal(|ui| {
                let side = |d: &Doc, t: &Toolpath| d.toolpath_on_side(t);
                let all = self.doc.toolpaths.iter().any(|t| side(&self.doc, t)) && self.doc.toolpaths.iter().filter(|t| side(&self.doc, t)).all(|t| t.visible);
                let mut toggle = all;
                if ui.checkbox(&mut toggle, "").on_hover_text(crate::i18n::tr("Show or hide all toolpaths")).changed() {
                    let doc = &mut self.doc;
                    let (two, face) = (doc.job.two_sided, doc.active_face);
                    for t in doc.toolpaths.iter_mut().filter(|t| !two || t.face == face) {
                        t.visible = toggle;
                    }
                }
                // A two-sided job lists one side's toolpaths, and says which (the original's guide).
                let title = match (self.doc.job.two_sided, self.doc.active_face) {
                    (false, _) => crate::i18n::tr("Toolpaths").to_string(),
                    (true, 0) => crate::i18n::tr("Toolpaths (Top)").to_string(),
                    (true, _) => crate::i18n::tr("Toolpaths (Bottom)").to_string(),
                };
                let r = bold_sized(ui, &title, 13.5, TEXT());
                label_widget(&r, &title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let some = self.selected_tp.is_some_and(|i| i > 0);
                    if icon_button(ui, Icon::Up, crate::i18n::tr("Move toolpath up"), some, false).clicked() {
                        if let Some(i) = self.selected_tp {
                            self.checkpoint();
                            self.doc.toolpaths.swap(i, i - 1);
                            self.selected_tp = Some(i - 1);
                        }
                    }
                });
            });
            let r = ui.available_rect_before_wrap();
            ui.painter().line_segment([r.left_top(), r.right_top() - vec2(0.0, 0.0)], Stroke::new(1.0, LINE()));
            ui.add_space(4.0);
            if !self.doc.toolpaths.iter().any(|t| self.doc.toolpath_on_side(t)) {
                let rest = ui.available_rect_before_wrap();
                ui.painter().text(rest.center(), Align2::CENTER_CENTER, crate::i18n::tr("No Toolpaths Created Yet"), FontId::proportional(13.5), MUTED());
                return;
            }
            let mut clicked_edit = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for i in 0..self.doc.toolpaths.len() {
                    if !self.doc.toolpath_on_side(&self.doc.toolpaths[i]) {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.checkbox(&mut self.doc.toolpaths[i].visible, "");
                        let kind = self.doc.toolpaths[i].op.kind;
                        let (r, kind_resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                        icons::paint(ui.painter(), r, crate::panels::op_icon(kind), true);
                        label_widget(&kind_resp, kind.title());
                        kind_resp.on_hover_text(kind.title());
                        let name = self.doc.toolpaths[i].op.name.clone();
                        let selected = self.selected_tp == Some(i);
                        let behind = self.behind.contains(&i);
                        let colour = if behind { theme::ERROR } else if selected { ACCENT } else { TEXT() };
                        let galley = ui.painter().layout_no_wrap(name.clone(), FontId::proportional(13.5), colour);
                        let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(6.0, 2.0), Sense::click());
                        if selected {
                            ui.painter().rect_filled(rect, 3.0, theme::ACCENT_SOFT());
                        }
                        ui.painter().galley(rect.min + vec2(3.0, 1.0), galley, TEXT());
                        label_widget(&resp, &name);
                        let resp = if behind { resp.on_hover_text(crate::i18n::tr("Its shapes have changed since it was calculated. Use Recalculate when you're ready.")) } else { resp };
                        if resp.clicked() {
                            self.selected_tp = Some(i);
                        }
                        if resp.double_clicked() {
                            clicked_edit = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = clicked_edit {
                self.selected_tp = Some(i);
                self.edit_toolpath();
            }
        });
    }




    pub(crate) fn profile_cards(&mut self, ui: &mut Ui, op: &mut crate::toolpath::Op, unit: &str, tab: &mut usize) {
        // Burin's own layout: pills for the side and the direction, then what to leave on the wall.
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            icon_caps(ui, Icon::TpProfile, crate::i18n::tr("Cut"));
            let side = match op.side {
                Side::Outside => 0,
                Side::Inside => 1,
                Side::On => 2,
            };
            if let Some(k) = pictured_pills(ui, &[(Some(Pic::SideOutside), crate::i18n::tr("Outside")), (Some(Pic::SideInside), crate::i18n::tr("Inside")), (Some(Pic::SideOn), crate::i18n::tr("On the line"))], side) {
                op.side = [Side::Outside, Side::Inside, Side::On][k];
            }
            explain(ui, [Pic::SideOutside, Pic::SideInside, Pic::SideOn][side.min(2)], match op.side {
                Side::Outside => crate::i18n::tr("The tool runs outside the line: the part keeps its size."),
                Side::Inside => crate::i18n::tr("The tool runs inside the line: the hole keeps its size."),
                Side::On => crate::i18n::tr("The middle of the tool follows the line."),
            });
            icon_caps(ui, Icon::Rotate, crate::i18n::tr("Direction"));
            if let Some(k) = pictured_pills(ui, &[(Some(Pic::Climb), crate::i18n::tr("Climb")), (Some(Pic::Conventional), crate::i18n::tr("Conventional"))], if op.climb { 0 } else { 1 }) {
                op.climb = k == 0;
            }
            explain_direction(ui, op.climb);
            labelled(ui, crate::i18n::tr("Leave on the wall"), 0.0, &mut op.allowance, 50.0, unit);
        });
        if !self.show_advanced {
            return;
        }
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.checkbox(&mut op.last_pass, crate::i18n::tr("Finishing pass"));
            if op.last_pass {
                labelled(ui, crate::i18n::tr("It takes off"), 0.0, &mut op.last_allowance, 50.0, unit);
                ui.checkbox(&mut op.last_reverse, crate::i18n::tr("The other way round"));
            }
            ui.checkbox(&mut op.tabs, crate::i18n::tr("Tabs to hold the part"));
            if op.tabs {
                ui.horizontal_wrapped(|ui| {
                    labelled(ui, crate::i18n::tr("Length"), 0.0, &mut op.tab_length, 44.0, unit);
                    labelled(ui, crate::i18n::tr("Thickness"), 0.0, &mut op.tab_thickness, 44.0, unit);
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Tabs per shape"));
                    count_box(ui, &mut op.tab_count, 44.0);
                });
            }
        });
        // The rarer choices, one section open at a time.
        for (i, name) in [crate::i18n::tr("Ramps"), crate::i18n::tr("Leads"), crate::i18n::tr("Order"), crate::i18n::tr("Start point"), crate::i18n::tr("Corners")].into_iter().enumerate() {
            let shown = egui::CollapsingHeader::new(RichText::new(name).strong()).id_salt(("op-section", i)).open(Some(*tab == i)).show(ui, |ui| match i {
            0 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                use crate::toolpath::RampKind;
                ui.checkbox(&mut op.ramp, crate::i18n::tr("Ramp down instead of plunging"));
                if op.ramp {
                    let kinds = [RampKind::Smooth, RampKind::ZigZag, RampKind::Spiral];
                    if let Some(k) = pictured_pills(ui, &[(Some(Pic::RampSmooth), crate::i18n::tr("Smooth")), (Some(Pic::RampZigZag), crate::i18n::tr("Zig Zag")), (Some(Pic::RampSpiral), crate::i18n::tr("Spiral"))], kinds.iter().position(|k| *k == op.ramp_kind).unwrap_or(0)) {
                        op.ramp_kind = kinds[k];
                    }
                    explain(ui, [Pic::RampSmooth, Pic::RampZigZag, Pic::RampSpiral][kinds.iter().position(|k| *k == op.ramp_kind).unwrap_or(0)], match op.ramp_kind {
                        RampKind::Smooth => crate::i18n::tr("Goes down along the cut in one slope."),
                        RampKind::ZigZag => crate::i18n::tr("Goes down back and forth over a short stretch."),
                        RampKind::Spiral => crate::i18n::tr("Goes down little by little all the way round."),
                    });
                    if op.ramp_kind == RampKind::Spiral {
                        ui.label(RichText::new(crate::i18n::tr("Slopes all the way around each closed shape.")).size(11.5).color(MUTED()));
                    } else {
                        ui.add_enabled_ui(op.lead, |ui| {
                            ui.checkbox(&mut op.ramp_on_lead, crate::i18n::tr("Ramp on Lead In"));
                        });
                        ui.add_enabled_ui(!(op.lead && op.ramp_on_lead), |ui| {
                            if let Some(k) = pictured_pills(ui, &[(Some(Pic::RampDistance), crate::i18n::tr("Distance")), (Some(Pic::RampAngle), crate::i18n::tr("Angle"))], usize::from(op.ramp_by_angle)) {
                                op.ramp_by_angle = k == 1;
                            }
                            if op.ramp_by_angle {
                                labelled(ui, crate::i18n::tr("Angle"), 96.0, &mut op.ramp_angle, 50.0, crate::i18n::tr("degrees"));
                                labelled(ui, crate::i18n::tr("Longest"), 96.0, &mut op.ramp_angle_limit, 50.0, unit);
                            } else {
                                labelled(ui, crate::i18n::tr("Distance"), 96.0, &mut op.ramp_distance, 50.0, unit);
                            }
                        });
                    }
                }
            }
            1 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.checkbox(&mut op.lead, crate::i18n::tr("Come in from the side"));
                if op.lead {
                    if let Some(k) = pictured_pills(ui, &[(Some(Pic::LeadStraight), crate::i18n::tr("Straight")), (Some(Pic::LeadCircular), crate::i18n::tr("Circular"))], usize::from(op.lead_circular)) {
                        op.lead_circular = k == 1;
                    }
                    explain(ui, if op.lead_circular { Pic::LeadCircular } else { Pic::LeadStraight }, if op.lead_circular { crate::i18n::tr("Curves into the cut: no mark where it starts.") } else { crate::i18n::tr("Comes in on a straight line from the waste side.") });
                    if op.lead_circular {
                        labelled(ui, crate::i18n::tr("Radius"), 96.0, &mut op.lead_radius, 50.0, unit);
                    } else {
                        labelled(ui, crate::i18n::tr("Angle"), 96.0, &mut op.lead_angle, 50.0, crate::i18n::tr("degrees"));
                    }
                    labelled(ui, crate::i18n::tr("Lead"), 96.0, &mut op.lead_distance, 50.0, unit);
                    ui.checkbox(&mut op.lead_out, crate::i18n::tr("Leave the same way"));
                    labelled(ui, crate::i18n::tr("Overcut"), 96.0, &mut op.overcut, 50.0, unit);
                }
            }
            2 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(crate::i18n::tr("Select one or more ways to try. With several, the one with the shortest travel between shapes is used.")).size(11.5).color(MUTED()));
                let mut c = op.order_choices.unwrap_or_default();
                ui.checkbox(&mut c.selection, crate::i18n::tr("Vector Selection Order"));
                ui.checkbox(&mut c.left_to_right, crate::i18n::tr("Left to Right"));
                ui.checkbox(&mut c.bottom_to_top, crate::i18n::tr("Bottom to Top"));
                ui.checkbox(&mut c.grid, crate::i18n::tr("Grid"));
                ui.checkbox(&mut c.shortest, crate::i18n::tr("Shortest Path"));
                op.order_choices = Some(c);
                // Keeping the order of clicks needs the shapes gathered that way.
                op.order = if c.selection { Order::AsDrawn } else { Order::Nearest };
            }
            3 => {
                use crate::toolpath::StartAt;
                ui.spacing_mut().item_spacing.y = 2.0;
                let chosen = match op.start_at {
                    StartAt::Keep => 0,
                    StartAt::Optimize => 1,
                    StartAt::Near(_) => 2,
                };
explain(ui, [Pic::StartDrawn, Pic::StartNearest, Pic::StartPoint][chosen], match chosen {
                    0 => crate::i18n::tr("Each shape starts where it was drawn from."),
                    1 => crate::i18n::tr("Each shape starts at its point nearest the tool."),
                    _ => crate::i18n::tr("Each shape starts nearest the point picked below."),
                });
                                match pictured_pills(ui, &[(Some(Pic::StartDrawn), crate::i18n::tr("As drawn")), (Some(Pic::StartNearest), crate::i18n::tr("Nearest")), (Some(Pic::StartPoint), crate::i18n::tr("Near a point"))], chosen) {
                    Some(0) => {
                        op.start_at = StartAt::Keep;
                        op.start_mid_span = false;
                    }
                    Some(1) => op.start_at = StartAt::Optimize,
                    Some(2) if chosen != 2 => op.start_at = StartAt::Near(0),
                    _ => {}
                }
                let placed = matches!(op.start_at, StartAt::Near(_));
                ui.add_enabled_ui(placed, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 70.0), Sense::hover());
                    let inner = rect.shrink(9.0);
                    ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, LINE()), StrokeKind::Inside);
                    let (l, r, t, b, cx, cy) = (inner.left(), inner.right(), inner.top(), inner.bottom(), inner.center().x, inner.center().y);
                    let spots = [pos2(l, t), pos2(cx, t), pos2(r, t), pos2(r, cy), pos2(r, b), pos2(cx, b), pos2(l, b), pos2(l, cy)];
                    for (i, spot) in spots.iter().enumerate() {
                        let hit = Rect::from_center_size(*spot, vec2(14.0, 14.0));
                        let resp = ui.interact(hit, ui.scope_id().with(("start_spot", i)), if placed { Sense::click() } else { Sense::hover() });
                        ui.painter().circle(*spot, 5.0, INPUT(), Stroke::new(1.0, MUTED()));
                        if op.start_at == StartAt::Near(i as u8) {
                            ui.painter().circle_filled(*spot, 3.0, ACCENT);
                        }
                        label_widget(&resp, &tf!("Start point {}", i + 1));
                        if resp.clicked() {
                            op.start_at = StartAt::Near(i as u8);
                        }
                    }
                });
            }
            _ => {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add_enabled_ui(op.side == Side::Outside, |ui| {
                    ui.checkbox(&mut op.sharp_corners, crate::i18n::tr("Sharp external corners"));
                });
                if op.side != Side::Outside {
                    ui.label(RichText::new(crate::i18n::tr("Only for cuts on the outside of a shape.")).size(11.5).color(MUTED()));
                } else {
                    ui.label(RichText::new(crate::i18n::tr("The tool goes all the way into each outside corner, instead of rounding it. Shapes inside each other stay rounded.")).size(11.5).color(MUTED()));
                }
            }
            });
            if shown.header_response.clicked() {
                *tab = if *tab == i { 99 } else { i };
            }
        }
    }

    pub(crate) fn quality_list(&mut self, ui: &mut Ui) {
        for q in Quality3d::ALL {
            if ui.selectable_label(self.quality3d == q, tr(q.name())).on_hover_text(q.hint()).clicked() {
                self.set_quality3d(q);
                ui.close();
            }
        }
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        // Burin's own layout: say what to carve, carve it, then a player bar; the looks of the
        // carving come last.
        let width = ui.available_width();
        Frame::new().inner_margin(Margin { left: 12, right: 12, top: 8, bottom: 6 }).show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Preview, true);
                bold_sized(ui, crate::i18n::tr("Simulation"), 15.0, TEXT());
            });
            // In a two-sided job these are the active side's toolpaths, as the list shows.
            let all: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|i| self.doc.toolpath_on_side(&self.doc.toolpaths[*i])).collect();
            let visible: Vec<usize> = all.iter().copied().filter(|i| self.doc.toolpaths[*i].visible).collect();
            let two = self.doc.job.two_sided && !self.doc.toolpaths.is_empty();
            let scope_id = ui.make_persistent_id("carve-scope");
            let default_scope = if self.selected_tp.is_some() { 0 } else { 1 };
            let mut scope: usize = ui.data(|d| d.get_temp(scope_id)).unwrap_or(default_scope);
            if scope == 3 && !two {
                scope = 2;
            }
            icon_caps(ui, Icon::TpPocket, crate::i18n::tr("Carve"));
            let mut options = vec![(Some(Pic::Icon(Icon::TpProfile)), crate::i18n::tr("Selected")), (Some(Pic::Icon(Icon::Eye)), crate::i18n::tr("Visible")), (Some(Pic::Icon(Icon::SelectAll)), crate::i18n::tr("All"))];
            if two {
                options.push((Some(Pic::TwoFaces), crate::i18n::tr("Both faces")));
            }
            if let Some(k) = pictured_pills(ui, &options, scope) {
                scope = k;
            }
            ui.data_mut(|d| d.insert_temp(scope_id, scope));
            let can = match scope {
                0 => self.selected_tp.is_some(),
                1 => !visible.is_empty(),
                2 => !all.is_empty(),
                _ => two,
            };
            let carve = ui.add_enabled(can, egui::Button::new(RichText::new(format!("        {}", crate::i18n::tr("Carve it"))).color(theme::ON_ACCENT).strong()).fill(ACCENT).min_size(vec2(ui.available_width(), 28.0)));
            label_widget(&carve, crate::i18n::tr("Carve it"));
            // A play mark before the words, drawn as the font has none.
            {
                let tip = pos2(carve.rect.left() + 20.0, carve.rect.center().y);
                let colour = if can { theme::ON_ACCENT } else { MUTED() };
                ui.painter().add(Shape::convex_polygon(vec![tip + vec2(-9.0, -6.0), tip + vec2(0.0, 0.0), tip + vec2(-9.0, 6.0)], colour, Stroke::NONE));
            }
            if carve.clicked() {
                if scope < 3 {
                    // One side at a time again: a carve that joined both is started afresh.
                    if scope > 0 && self.sim_floor.take().is_some() {
                        self.sim = None;
                    }
                }
                match scope {
                    0 => {
                        if let Some(i) = self.selected_tp {
                            self.preview(vec![i]);
                        }
                    }
                    1 => self.preview(visible.clone()),
                    2 => self.preview(all.clone()),
                    _ => self.preview_all_sides(),
                }
            }
            // The player: one rounded bar, its speed beside it.
            let running = !self.anim.queue.is_empty();
            Frame::new().fill(theme::INPUT()).corner_radius(14).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (icon, tip) in [(Icon::Rewind, crate::i18n::tr("Back to start")), (Icon::Play, crate::i18n::tr("Play")), (Icon::Pause, crate::i18n::tr("Pause")), (Icon::Step, crate::i18n::tr("Step")), (Icon::End, crate::i18n::tr("Go to end"))] {
                        let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), if running { Sense::click() } else { Sense::hover() });
                        if resp.hovered() && running {
                            ui.painter().circle_filled(rect.center(), 11.0, theme::BUTTON());
                        }
                        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), icon, running);
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, running, tip));
                        if resp.on_hover_text(tip).clicked() {
                            match icon {
                                Icon::Play => {
                                    if self.anim.at >= self.anim.queue.len() {
                                        self.rewind_preview();
                                    }
                                    self.anim.playing = true;
                                }
                                Icon::Pause => self.anim.playing = false,
                                Icon::Step => {
                                    self.anim.playing = false;
                                    self.advance_preview(1);
                                    self.bump_surface();
                                }
                                Icon::End => self.rush_preview(),
                                _ => self.rewind_preview(),
                            }
                        }
                    }
                    ui.add_space(6.0);
                    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(30.0);
                    ui.spacing_mut().slider_rail_height = 3.0;
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.bg_fill = LINE();
                    w.hovered.bg_fill = LINE();
                    w.active.bg_fill = LINE();
                    ui.add(egui::Slider::new(&mut self.speed, 0.0..=1.0).show_value(false)).on_hover_text(crate::i18n::tr("Speed"));
                });
            });
            let has_sim = self.sim.is_some();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if worded_button(ui, has_sim, Icon::JobSetup, crate::i18n::tr("Start over")).on_hover_text(crate::i18n::tr("Back to the uncut board")).clicked() {
                    self.reset_preview();
                }
                if worded_button(ui, has_sim, Icon::Undo, crate::i18n::tr("Undo last")).clicked() {
                    self.undo_preview();
                }
                if worded_button(ui, true, Icon::Save, crate::i18n::tr("Save picture...")).clicked() {
                    self.save_preview = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Picture"), &["png"]).set_file_name("preview.png").save_file();
                }
            });
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Select, true);
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Double-click a loose offcut in the 3D view to take it away.")).size(11.0).color(MUTED())).wrap());
            });
            if let Some(picture) = self.picture_seconds {
                let whole = self.preview_seconds.map_or(String::new(), |s| tf!(", whole preview {s:.1} s"));
                ui.label(RichText::new(tf!("Last picture {picture:.1} s{whole}")).size(11.0).color(MUTED()));
            }
            // How the carving looks.
            ui.add_space(4.0);
            icon_caps(ui, Icon::Eye, crate::i18n::tr("Look"));
            ui.horizontal(|ui| {
                let name = self.doc.job.preview_material.map_or(crate::i18n::tr("Solid Material"), |m| tr(MATERIALS[m.min(4)].0));
                egui::ComboBox::from_id_salt("preview_material").selected_text(name).width(130.0).show_ui(ui, |ui| {
                    for (i, m) in MATERIALS.iter().enumerate() {
                        ui.selectable_value(&mut self.doc.job.preview_material, Some(i), tr(m.0));
                    }
                    ui.selectable_value(&mut self.doc.job.preview_material, None, crate::i18n::tr("Solid Material"));
                });
                if self.doc.job.preview_material.is_none() {
                    ui.color_edit_button_srgb(&mut self.doc.job.solid);
                }
            });
            ui.label(RichText::new(crate::i18n::tr("Cut areas")).size(12.0).color(MUTED()));
            let chosen = match self.machined {
                Machined::Material => 0,
                Machined::Global => 1,
                Machined::Toolpath => 2,
            };
            if let Some(k) = pictured_pills(ui, &[(Some(Pic::CutBoard), crate::i18n::tr("as the board")), (Some(Pic::CutOneColour), crate::i18n::tr("one colour")), (Some(Pic::CutByToolpath), crate::i18n::tr("by toolpath"))], chosen) {
                self.machined = [Machined::Material, Machined::Global, Machined::Toolpath][k];
            }
            match self.machined {
                Machined::Global => {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut self.global_fill);
                        if ui.small_button(crate::i18n::tr("Give it to every toolpath")).clicked() {
                            for t in &mut self.doc.toolpaths {
                                t.color = self.global_fill;
                            }
                        }
                    });
                }
                Machined::Toolpath => {
                    ui.horizontal(|ui| {
                        let mut chosen = self.selected_tp.and_then(|i| self.doc.toolpaths.get(i)).map(|t| t.color).unwrap_or(self.global_fill);
                        if ui.color_edit_button_srgb(&mut chosen).changed() {
                            if let Some(tp) = self.selected_tp.and_then(|i| self.doc.toolpaths.get_mut(i)) {
                                tp.color = chosen;
                            }
                        }
                        ui.label(RichText::new(crate::i18n::tr("the selected toolpath's colour")).size(11.0).color(MUTED()));
                    });
                }
                Machined::Material => {}
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.animate, crate::i18n::tr("Animate"));
                ui.checkbox(&mut self.draw_tool, crate::i18n::tr("Show the tool"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("3D quality"));
                egui::ComboBox::from_id_salt("quality3d").selected_text(tr(self.quality3d.name())).width(100.0).show_ui(ui, |ui| self.quality_list(ui));
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 80.0).max(0.0));
                if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                    self.right = RightMode::List;
                }
            });
        });
    }

    fn centre(&mut self, ui: &mut Ui) {
        let whole = ui.max_rect();
        // In narrow windows the view tools get a row of their own.
        let two_rows = whole.width() < 700.0;
        let top = Rect::from_min_size(whole.min, vec2(whole.width(), if two_rows { 60.0 } else { 32.0 }));
        let view = Rect::from_min_max(pos2(whole.left() + 5.0, top.bottom()), whole.max);
        self.top_row(ui, top, view, two_rows);
        let Some(tiling) = self.tiled else {
            if self.view3d {
                self.view_3d(ui, view);
                self.view_rect = view;
            } else {
                self.view_2d(ui, view);
            }
            return;
        };
        let (first, second) = tile_rects(view, tiling);
        let (r2, r3) = if self.view3d { (second, first) } else { (first, second) };
        // A press in the other view makes it the one in use, as clicking a window does.
        if let Some(p) = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) {
            if r3.contains(p) {
                self.view3d = true;
            } else if r2.contains(p) {
                self.view3d = false;
            }
        }
        self.view_2d(ui, r2);
        self.view_3d(ui, r3);
        if self.view3d {
            self.view_rect = r3;
        }
        // The view in use has its edge lit.
        let (on, off) = if self.view3d { (r3, r2) } else { (r2, r3) };
        ui.painter().rect_stroke(off, 0.0, Stroke::new(1.0, LINE()), egui::StrokeKind::Inside);
        ui.painter().rect_stroke(on, 0.0, Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
    }

    /// Makes one side of a two-sided job the one worked on (the original's `SetActiveSide`): only
    /// its shapes and toolpaths show, and what's drawn or made goes on it.
    pub(crate) fn set_active_face(&mut self, face: u8) {
        if !self.doc.job.two_sided || self.doc.active_face == face {
            return;
        }
        self.doc.active_face = face;
        self.selected.clear();
        self.handles_on = None;
        self.selected_tp = None;
        self.sim = None;
        self.sim_floor = None;
        self.status = if face == 0 { crate::i18n::tr("Working on the top side.") } else { crate::i18n::tr("Working on the bottom side.") }.into();
    }

    /// Change flip direction: the bottom side's shapes are drawn as seen once the block is turned
    /// over, so turning it the other way puts each of them half a turn round the job's middle,
    /// keeping it over the same spot of the material.
    pub(crate) fn change_flip_direction(&mut self) {
        self.checkpoint();
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        for v in self.doc.vectors.iter_mut().filter(|v| v.face == 1) {
            v.each_mut(&mut |v| {
                for q in v.paths.iter_mut().flat_map(|q| q.pts.iter_mut()) {
                    *q = [w - q[0], h - q[1]];
                }
                v.prim = Prim::Free;
            });
        }
        self.doc.job.flip_around_y = !self.doc.job.flip_around_y;
        self.dirty = true;
    }

    /// The same key again goes back to one view, like a maximised window.
    pub(crate) fn tile(&mut self, tiling: crate::app::Tiling) {
        self.tiled = if self.tiled == Some(tiling) { None } else { Some(tiling) };
        self.auto_fit = true;
    }

    fn top_row(&mut self, ui: &mut Ui, top: Rect, view: Rect, two_rows: bool) {
        ui.painter().rect_filled(top, 0.0, CHROME());
        let tab_y = top.top() + 8.0;
        let two = Rect::from_min_size(pos2(top.left() + 12.0, tab_y), vec2(76.0, 22.0));
        let three = Rect::from_min_size(pos2(two.right() - 4.0, tab_y), vec2(66.0, 22.0));
        if side_tab(ui, two, crate::i18n::tr("2D View"), !self.view3d).clicked() {
            self.view3d = false;
        }
        if side_tab(ui, three, crate::i18n::tr("3D View"), self.view3d).clicked() {
            self.view3d = true;
        }
        let layer_at = pos2(three.right() + 42.0, top.top() + 6.0);
        ui.painter().line_segment([layer_at - vec2(5.0, -2.0), layer_at + vec2(-5.0, 22.0)], Stroke::new(1.0, LINE()));
        icons::paint(ui.painter(), Rect::from_min_size(layer_at, vec2(22.0, 22.0)), Icon::Layer, true);
        let mut layer_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(layer_at + vec2(22.0, 1.0), vec2(120.0, 24.0))));
        let name = self.doc.layers.get(self.doc.active_layer).map_or(String::new(), |l| l.name.clone());
        // One line, however long the name: the whole of it shows when asked for, and in the list.
        let shown = egui::ComboBox::from_id_salt("layer").selected_text(name.clone()).truncate().width(78.0).show_ui(&mut layer_ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for (i, l) in self.doc.layers.iter().enumerate() {
                ui.selectable_value(&mut self.doc.active_layer, i, l.name.as_str());
            }
        });
        shown.response.on_hover_text(name);
        // View tools, placed as in the pictures.
        let (start, tools_y) = if two_rows {
            (top.left() + 12.0, top.top() + 33.0)
        } else {
            ((top.left() + top.width() * 0.482).max(layer_at.x + 138.0), top.top() + 4.0)
        };
        let mut x = start;
        let view3d = self.view3d;
        let items: [(Icon, &str, bool, bool, f32); 13] = [
            (Icon::Snap, crate::i18n::tr("Snap to grid"), true, self.snap, 27.0),
            (Icon::SnapShapes, crate::i18n::tr("Snap to shapes"), !view3d, self.snap_shapes, 27.0),
            (Icon::SnapObjects, crate::i18n::tr("Show model outlines in 2D"), true, self.show_model2d, 27.0),
            (Icon::Grid, crate::i18n::tr("Show grid"), true, self.grid, 34.0),
            (Icon::Pan, crate::i18n::tr("Pan"), !view3d, self.tool == Tool2::Pan, 27.0),
            (Icon::ZoomBox, crate::i18n::tr("Zoom to box"), !view3d, self.tool == Tool2::ZoomBox, 27.0),
            (Icon::ZoomFit, crate::i18n::tr("Zoom to job"), true, false, 27.0),
            (Icon::ZoomSel, crate::i18n::tr("Zoom to selection"), !view3d, false, 34.0),
            (Icon::ShowVectors, crate::i18n::tr("Show shapes"), true, self.show_vectors, 27.0),
            (Icon::ShowPaths, crate::i18n::tr("Show toolpaths"), true, self.show_toolpaths, 34.0),
            (Icon::ShowModel, crate::i18n::tr("Show the 3D model"), true, false, 36.0),
            // The original's view toolbar (resource 140) ends with Tile Vertical, then Tile Horizontal.
            (Icon::TileSide, crate::i18n::tr("Tile Windows Vertical"), true, self.tiled == Some(crate::app::Tiling::SideBySide), 27.0),
            (Icon::TileStacked, crate::i18n::tr("Tile Windows Horizontal"), true, self.tiled == Some(crate::app::Tiling::Stacked), 27.0),
        ];
        // In a narrow window the gaps between groups close up first, so every tool stays clear of
        // the panel on the right; each keeps its own 24 points.
        let wanted: f32 = items.iter().map(|i| i.4).sum();
        let room = top.right() - 8.0 - start;
        let squeeze = if two_rows && wanted > room { room / wanted } else { 1.0 };
        // The view's own tools in blue, apart from the drawing and machining ones.
        icons::set_tint(Some(theme::HUE_BLUE()));
        for (i, (icon, tip, enabled, active, step)) in items.into_iter().enumerate() {
            let step = (step * squeeze).max(24.5);
            let rect = Rect::from_min_size(pos2(x, tools_y), vec2(24.0, 24.0));
            let resp = ui.interact(rect, ui.scope_id().with(("toolbar", i)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
            if active {
                ui.painter().rect_filled(rect, 6.0, theme::ACCENT_SOFT());
            } else if enabled && resp.hovered() {
                ui.painter().rect_filled(rect, 6.0, theme::BUTTON());
            }
            icons::paint(ui.painter(), rect.shrink(1.0), icon, enabled);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
            if resp.clicked() {
                match icon {
                    Icon::Snap => self.snap = !self.snap,
                    Icon::SnapShapes => self.snap_shapes = !self.snap_shapes,
                    Icon::SnapObjects => self.show_model2d = !self.show_model2d,
                    Icon::Grid => self.grid = !self.grid,
                    Icon::Pan => self.tool = if self.tool == Tool2::Pan { Tool2::Select } else { Tool2::Pan },
                    Icon::ZoomBox => self.tool = if self.tool == Tool2::ZoomBox { Tool2::Select } else { Tool2::ZoomBox },
                    Icon::ZoomFit => self.zoom_to_job(),
                    Icon::ZoomSel => {
                        let inner = Rect::from_min_max(view.min + vec2(25.0, 25.0), view.max - vec2(16.0, 16.0));
                        self.fit_selection(inner);
                    }
                    Icon::ShowVectors => self.show_vectors = !self.show_vectors,
                    Icon::ShowPaths => self.show_toolpaths = !self.show_toolpaths,
                    Icon::ShowModel => {
                        self.view3d = true;
                        self.sim = None;
                        self.anim = Anim::default();
                        self.bump_surface();
                    }
                    Icon::TileSide => self.tile(crate::app::Tiling::SideBySide),
                    Icon::TileStacked => self.tile(crate::app::Tiling::Stacked),
                    _ => {}
                }
            }
            x += step;
        }
        icons::set_tint(None);
        // A two-sided job adds two buttons: which way the block turns over, and which side is in use
        // (the original's "Change flip direction" `0x8459` and "Toggle Top / Bottom Side" `0x84bd`).
        if self.doc.job.two_sided {
            // When they don't fit after the tools, they go at the end of the row above.
            let (mut x, tools_y) = if x + 7.0 + 2.0 * 27.0 > top.right() - 4.0 && two_rows {
                (top.right() - 7.0 - 2.0 * 27.0 - 4.0, top.top() + 4.0)
            } else {
                (x, tools_y)
            };
            let flip = if self.doc.job.flip_around_y { Icon::FlipAroundY } else { Icon::FlipAroundX };
            let side = if self.doc.active_face == 0 { Icon::SideTop } else { Icon::SideBottom };
            for (i, (icon, tip)) in [(flip, crate::i18n::tr("Change flip direction")), (side, crate::i18n::tr("Toggle Top / Bottom Side"))].into_iter().enumerate() {
                let rect = Rect::from_min_size(pos2(x + 7.0, tools_y), vec2(24.0, 24.0));
                let resp = ui.interact(rect, ui.scope_id().with(("sides", i)), Sense::click()).on_hover_text(tip);
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, theme::BUTTON());
                }
                icons::paint(ui.painter(), rect.shrink(1.0), icon, true);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                if resp.clicked() {
                    if i == 0 {
                        self.change_flip_direction();
                    } else {
                        self.set_active_face(1 - self.doc.active_face);
                    }
                }
                x += 27.0;
            }
        }
    }

    // ----- Floating windows -----

    fn windows(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.dim_editor(&ctx);
        self.op_windows(&ctx);
        self.recalc_window(&ctx);
        if self.tools_open {
            self.tool_list(&ctx);
        }
        if self.time_open {
            let mut open = true;
            egui::Window::new(crate::i18n::tr("Machining Time")).open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                let mut total = 0.0;
                let rapid = if self.doc.job.units == Units::Inches { toolpath::RAPID_RATE_MM / 25.4 } else { toolpath::RAPID_RATE_MM };
                for t in self.doc.toolpaths.iter().filter(|t| t.visible && self.doc.toolpath_on_side(t)) {
                    let s = self.toolpath_seconds(t, rapid);
                    total += s;
                    ui.label(format!("{}: {}", t.op.name, duration(s)));
                }
                ui.separator();
                bold(ui, &tf!("Visible toolpaths: {}", duration(total)));
                ui.label(RichText::new(crate::i18n::tr("An estimate: cutting and lifting count double, and fast moves go at 100 mm/s. The real time depends on your machine.")).size(11.5));
            });
            self.time_open = open;
        }
        if self.about_open {
            let mut open = true;
            egui::Window::new(crate::i18n::tr("About Burin")).open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                // As the original's (`0x140726810`): the version, then how the 3D view is drawn.
                let drawing = crate::startup::drawing_name(&crate::startup::graphics_line(), crate::platform::WEB);
                ui.label(RichText::new(tf!("Burin {} ({})", env!("CARGO_PKG_VERSION"), drawing)).strong().size(16.0));
                ui.label(crate::i18n::tr("Design, model and cut for CNC carving."));
                ui.label(RichText::new(crate::i18n::tr("Free software under the GNU GPL, version 3 or later.")).color(theme::MUTED()));
                ui.hyperlink_to("burin.cc", "https://burin.cc");
                ui.add_space(4.0);
                ui.label(crate::i18n::tr("Check every machine file in a simulator or above the material before cutting."));
            });
            self.about_open = open;
        }
        if let Some(text) = self.message.clone() {
            egui::Window::new("Burin").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(&ctx, |ui| {
                ui.label(text);
                ui.vertical_centered(|ui| {
                    if primary(ui, "OK", 74.0).clicked() {
                        self.message = None;
                    }
                });
            });
        }
    }

}

pub(crate) fn duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// A small slab of wood, used as the picture in several panels.
/// A point on the top of the wood block: 0,0 is the job's bottom left corner, 1,1 its top right.
pub(crate) fn wood_top_point(r: Rect, u: f32, v: f32) -> egui::Pos2 {
    let c = r.center();
    let (w, h) = (r.width() * 0.46, r.height() * 0.36);
    let (bottom_left, bottom_right) = (c + vec2(-w, h * 0.1), c + vec2(w * 0.55, h * 0.1));
    let (top_left, top_right) = (c + vec2(-w * 0.55, -h), c + vec2(w, -h));
    let low = bottom_left.lerp(bottom_right, u);
    let high = top_left.lerp(top_right, u);
    low.lerp(high, v)
}

/// The material as a slab, coloured like the logo's block: a gold top face on orange sides, with a
/// dark brown outline. Burin's own picture of the job, wherever the material is shown small.
pub(crate) fn wood_block(p: &egui::Painter, r: Rect) {
    let c = r.center();
    let (w, h) = (r.width() * 0.46, r.height() * 0.36);
    let top = vec![c + vec2(-w * 0.55, -h), c + vec2(w, -h), c + vec2(w * 0.55, h * 0.1), c + vec2(-w, h * 0.1)];
    let side = vec![c + vec2(-w, h * 0.1), c + vec2(w * 0.55, h * 0.1), c + vec2(w * 0.55, h), c + vec2(-w, h)];
    let end = vec![c + vec2(w * 0.55, h * 0.1), c + vec2(w, -h), c + vec2(w, h * 0.1), c + vec2(w * 0.55, h)];
    let edge = Stroke::new(1.0, Color32::from_rgb(90, 28, 0));
    p.add(Shape::convex_polygon(side, Color32::from_rgb(255, 122, 0), edge));
    p.add(Shape::convex_polygon(end, Color32::from_rgb(224, 74, 0), edge));
    p.add(Shape::convex_polygon(top, Color32::from_rgb(255, 180, 0), edge));
}

/// Where the machine's zero is, on the slab: a white crosshair in a ring.
pub(crate) fn zero_marker(p: &egui::Painter, at: egui::Pos2) {
    let white = Stroke::new(1.4, Color32::WHITE);
    p.circle_stroke(at, 4.0, white);
    p.line_segment([at - vec2(7.0, 0.0), at + vec2(7.0, 0.0)], white);
    p.line_segment([at - vec2(0.0, 7.0), at + vec2(0.0, 7.0)], white);
}

/// The two halves of the view area, the first one for the view in use, with a small gap between.
pub(crate) fn tile_rects(view: Rect, tiling: crate::app::Tiling) -> (Rect, Rect) {
    let gap = TILE_GAP / 2.0;
    match tiling {
        crate::app::Tiling::SideBySide => {
            let mid = view.center().x;
            (Rect::from_min_max(view.min, pos2(mid - gap, view.bottom())), Rect::from_min_max(pos2(mid + gap, view.top()), view.max))
        }
        crate::app::Tiling::Stacked => {
            let mid = view.center().y;
            (Rect::from_min_max(view.min, pos2(view.right(), mid - gap)), Rect::from_min_max(pos2(view.left(), mid + gap), view.max))
        }
    }
}

/// Space between two tiled views, in points.
const TILE_GAP: f32 = 4.0;
