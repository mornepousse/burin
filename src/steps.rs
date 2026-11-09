//! Machining as a sequence: the toolpaths as cards along the bottom of the view, in the order they
//! cut, each with its tool, depth and time. Click to choose one, double-click to change it, and
//! move it earlier or later with its arrows. Burin's own; the original lists them in a tree.

use crate::app::*;
use crate::i18n::tr;
use crate::theme;
use crate::toolpath::{self, OpKind};
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, StrokeKind};

/// The strip's height, and one card's width, in points.
pub const STRIP_H: f32 = 112.0;
const CARD_W: f32 = 196.0;

/// The colour of a family of toolpaths, as on their buttons: 2D teal, 3D amber, edges violet.
pub fn family_hue(kind: OpKind) -> egui::Color32 {
    match kind {
        OpKind::Rough3D | OpKind::Finish3D | OpKind::Prism | OpKind::Texture | OpKind::Moulding => theme::HUE_AMBER(),
        OpKind::Flute | OpKind::Chamfer => theme::HUE_VIOLET(),
        _ => theme::HUE_TEAL(),
    }
}

/// Text cut to fit a width, with an ellipsis.
fn fitted(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let fits = |t: &str| ui.painter().layout_no_wrap(t.to_string(), font.clone(), theme::TEXT()).size().x <= width;
    if fits(text) {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while kept.len() > 1 {
        kept.pop();
        let shown = format!("{}…", kept.iter().collect::<String>().trim_end());
        if fits(&shown) {
            return shown;
        }
    }
    "…".into()
}

impl App {
    /// A toolpath's time, worked out once for its moves.
    fn step_seconds(&mut self, i: usize) -> f64 {
        let key = std::sync::Arc::as_ptr(&self.doc.toolpaths[i].moves) as usize;
        if let Some(s) = self.step_times.get(&key) {
            return *s;
        }
        let rapid = if self.doc.job.units == Units::Inches { toolpath::RAPID_RATE_MM / 25.4 } else { toolpath::RAPID_RATE_MM };
        let s = self.toolpath_seconds(&self.doc.toolpaths[i], rapid);
        if self.step_times.len() > 500 {
            self.step_times.clear();
        }
        self.step_times.insert(key, s);
        s
    }

    pub(crate) fn steps_strip(&mut self, ui: &mut egui::Ui) {
        let shown: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|i| self.doc.toolpath_on_side(&self.doc.toolpaths[*i])).collect();
        egui::Panel::bottom("steps").exact_size(STRIP_H).resizable(false).frame(egui::Frame::new().fill(theme::CHROME())).show(ui, |ui| {
            let whole = ui.max_rect();
            ui.painter().line_segment([whole.left_top(), whole.right_top()], Stroke::new(1.0, theme::LINE()));
            let title = tr("Cutting order");
            let said = ui.painter().text(whole.left_top() + vec2(12.0, 12.0), Align2::LEFT_CENTER, title, FontId::proportional(12.0), theme::MUTED());
            let heading = ui.interact(said, ui.make_persistent_id("steps-title"), Sense::hover());
            crate::ui::label_widget(&heading, title);
            if shown.is_empty() {
                ui.painter().text(whole.center(), Align2::CENTER_CENTER, tr("No Toolpaths Created Yet"), FontId::proportional(13.0), theme::MUTED());
                return;
            }
            let visible: Vec<usize> = shown.iter().copied().filter(|i| self.doc.toolpaths[*i].visible).collect();
            let total: f64 = visible.iter().map(|i| self.step_seconds(*i)).sum();
            let sum = tf!("{} visible, {} in all", visible.len(), crate::ui::duration(total));
            ui.painter().text(whole.right_top() + vec2(-12.0, 12.0), Align2::RIGHT_CENTER, sum, FontId::proportional(12.0), theme::MUTED());
            let area = Rect::from_min_max(pos2(whole.left(), whole.top() + 22.0), whole.max);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
            let mut swap: Option<(usize, usize)> = None;
            let mut edit = false;
            egui::ScrollArea::horizontal().auto_shrink(false).show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (n, &i) in shown.iter().enumerate() {
                        let seconds = self.step_seconds(i);
                        let t = &self.doc.toolpaths[i];
                        let hue = family_hue(t.op.kind);
                        let picked = self.selected_tp == Some(i);
                        let (rect, resp) = ui.allocate_exact_size(vec2(CARD_W, STRIP_H - 32.0), Sense::click());
                        let p = ui.painter();
                        let fill = if picked { theme::hue_soft(hue) } else if resp.hovered() { theme::BUTTON() } else { theme::CARD() };
                        p.rect(rect, 8.0, fill, Stroke::new(if picked { 1.5 } else { 1.0 }, if picked { hue } else { theme::LINE() }), StrokeKind::Inside);
                        // Its place in the order, in its family's colour.
                        let badge = rect.left_top() + vec2(16.0, 16.0);
                        p.circle_filled(badge, 10.0, hue);
                        p.text(badge, Align2::CENTER_CENTER, format!("{}", n + 1), FontId::proportional(11.5), theme::CARD());
                        let text_left = rect.left() + 32.0;
                        let name_font = FontId::proportional(12.5);
                        // Room left for the arrows on the chosen card.
                        let name = fitted(ui, &t.op.name, &name_font, CARD_W - if picked { 92.0 } else { 40.0 });
                        let ink = if t.visible { theme::TEXT() } else { theme::FAINT() };
                        p.text(pos2(text_left, rect.top() + 16.0), Align2::LEFT_CENTER, name, name_font, ink);
                        let tool = fitted(ui, &t.op.tool.name, &FontId::proportional(11.0), CARD_W - 20.0);
                        p.text(pos2(rect.left() + 10.0, rect.top() + 36.0), Align2::LEFT_CENTER, tool, FontId::proportional(11.0), theme::MUTED());
                        let unit = self.doc.job.unit();
                        let line = format!("{} {}  ·  {}", toolpath::trim(t.op.cut_depth), unit, crate::ui::duration(seconds));
                        p.text(pos2(rect.left() + 10.0, rect.top() + 54.0), Align2::LEFT_CENTER, line, FontId::proportional(11.0), theme::MUTED());
                        if !t.visible {
                            p.text(pos2(rect.right() - 10.0, rect.top() + 54.0), Align2::RIGHT_CENTER, tr("hidden"), FontId::proportional(10.5), theme::FAINT());
                        }
                        let name_for_tests = t.op.name.clone();
                        crate::ui::label_widget(&resp, &name_for_tests);
                        if resp.clicked() {
                            self.selected_tp = Some(i);
                        }
                        if resp.double_clicked() {
                            self.selected_tp = Some(i);
                            edit = true;
                        }
                        // Earlier or later, on the chosen card.
                        if picked {
                            for (k, tip) in [tr("Cut it earlier"), tr("Cut it later")].into_iter().enumerate() {
                                let r = Rect::from_min_size(pos2(rect.right() - 50.0 + k as f32 * 24.0, rect.top() + 4.0), vec2(22.0, 22.0));
                                let other = if k == 0 { n.checked_sub(1) } else { (n + 1 < shown.len()).then_some(n + 1) };
                                let b = ui.interact(r, ui.make_persistent_id(("step-move", i, k)), if other.is_some() { Sense::click() } else { Sense::hover() });
                                if b.hovered() && other.is_some() {
                                    ui.painter().rect_filled(r, 5.0, theme::BUTTON_HOVER());
                                }
                                // Turned a quarter, the arrows point left and right along the strip.
                                let shape = if k == 0 { [pos2(r.left() + 6.0, r.center().y), pos2(r.right() - 7.0, r.top() + 6.0), pos2(r.right() - 7.0, r.bottom() - 6.0)] } else { [pos2(r.right() - 6.0, r.center().y), pos2(r.left() + 7.0, r.top() + 6.0), pos2(r.left() + 7.0, r.bottom() - 6.0)] };
                                let colour = if other.is_some() { theme::TEXT() } else { theme::FAINT() };
                                ui.painter().add(egui::Shape::convex_polygon(shape.to_vec(), colour, Stroke::NONE));
                                crate::ui::label_widget(&b, tip);
                                if b.on_hover_text(tip).clicked() {
                                    if let Some(o) = other {
                                        swap = Some((i, shown[o]));
                                    }
                                }
                            }
                        }
                    }
                    ui.add_space(10.0);
                });
            });
            if let Some((a, b)) = swap {
                self.checkpoint();
                self.doc.toolpaths.swap(a, b);
                self.selected_tp = Some(b);
            }
            if edit {
                self.edit_toolpath();
            }
        });
    }
}
