//! What's selected, at a glance and to the number: where it is, how big, on which layer, in a small
//! card at the top right of the 2D view, as drawing programs such as Affinity show it. Typing a new
//! value moves or resizes the selection at once. Burin's own; the original does this through its
//! Move and Size forms.

use crate::app::*;
use crate::i18n::tr;
use crate::theme;
use egui::{pos2, RichText};

/// The card's width, in points.
const CARD_W: f32 = 214.0;

impl App {
    pub(crate) fn inspector(&mut self, ctx: &egui::Context) {
        if self.screen != Screen::Work || self.view3d || self.persona == Persona::Machining || self.selected.is_empty() || self.view_rect.width() < 2.0 * CARD_W {
            self.inspector_editing = false;
            return;
        }
        let Some((lo, hi)) = self.moving_bbox() else { return };
        let unit = self.doc.job.unit();
        let at = pos2(self.view_rect.right() - CARD_W - 10.0, self.view_rect.top() + 10.0);
        let (mut x, mut y, mut w, mut h) = (lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]);
        let layers: Vec<String> = self.doc.layers.iter().map(|l| l.name.clone()).collect();
        let shared_layer = {
            let mut found = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).map(|v| v.layer);
            let first = found.next();
            first.filter(|f| found.all(|l| l == *f))
        };
        let mut layer = shared_layer;
        let mut keep_ratio = self.inspector_keep_ratio;
        let (mut moved, mut sized) = (false, false);
        egui::Area::new(egui::Id::unique("inspector")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(theme::CARD).stroke(egui::Stroke::new(1.0, theme::LINE)).corner_radius(10.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(CARD_W - 20.0);
                let count = self.selected.len();
                let title = if count == 1 { tr("Selected shape").to_string() } else { tf!("{} shapes selected", count) };
                ui.label(RichText::new(title).strong().color(theme::HUE_BLUE));
                ui.add_space(2.0);
                egui::Grid::new("inspector-grid").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                    ui.label("X");
                    moved |= crate::ui::number(ui, &mut x, 56.0);
                    ui.label("Y");
                    moved |= crate::ui::number(ui, &mut y, 56.0);
                    ui.end_row();
                    ui.label(tr("W"));
                    sized |= crate::ui::number(ui, &mut w, 56.0);
                    ui.label(tr("H"));
                    let h_before = h;
                    if crate::ui::number(ui, &mut h, 56.0) {
                        sized = true;
                        if keep_ratio && h_before > 1e-9 {
                            w = (hi[0] - lo[0]) * h / h_before;
                        }
                    } else if sized && keep_ratio && (hi[0] - lo[0]) > 1e-9 {
                        h = (hi[1] - lo[1]) * w / (hi[0] - lo[0]);
                    }
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut keep_ratio, tr("Keep proportions"));
                    ui.label(RichText::new(unit).size(11.0).color(theme::MUTED));
                });
                if layers.len() > 1 {
                    let shown = layer.and_then(|l| layers.get(l).cloned()).unwrap_or_else(|| tr("Several layers").to_string());
                    egui::ComboBox::from_id_salt("inspector-layer").selected_text(shown).width(CARD_W - 40.0).show_ui(ui, |ui| {
                        for (i, name) in layers.iter().enumerate() {
                            ui.selectable_value(&mut layer, Some(i), name);
                        }
                    });
                }
            });
        });
        self.inspector_keep_ratio = keep_ratio;
        let changed = moved || sized || layer != shared_layer;
        if changed && !self.inspector_editing {
            // One step to undo for the whole edit, however many keys it took.
            self.checkpoint();
            self.inspector_editing = true;
        } else if !changed && !ctx.egui_wants_keyboard_input() {
            self.inspector_editing = false;
        }
        if moved {
            self.translate_selected(x - lo[0], y - lo[1]);
            self.dirty = true;
        }
        if sized {
            let sx = if hi[0] - lo[0] > 1e-9 && w > 1e-9 { w / (hi[0] - lo[0]) } else { 1.0 };
            let sy = if hi[1] - lo[1] > 1e-9 && h > 1e-9 { h / (hi[1] - lo[1]) } else { 1.0 };
            self.map_moving(|p| [lo[0] + (p[0] - lo[0]) * sx, lo[1] + (p[1] - lo[1]) * sy]);
            self.dirty = true;
        }
        if let Some(l) = layer.filter(|l| Some(*l) != shared_layer) {
            let ids = self.selected.clone();
            for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
                v.layer = l;
            }
            self.dirty = true;
        }
    }
}
