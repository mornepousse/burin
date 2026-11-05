//! The window: menus, side panels, forms and the status bar.

use crate::app::*;
use crate::geom::{self, Path};
use crate::icons::{self, Icon};
use crate::relief::{Combine, Profile};
use crate::toolpath::{self, OpKind, Order, Side};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui};

use crate::theme::{self, ACCENT, CARD, CHROME, INPUT, LINE, MUTED, PANEL, TEXT, WORKSPACE};
pub use crate::theme::setup_style;
const SIDE_W: f32 = 248.0;
const RIGHT_W: f32 = 282.0;


/// Gives a painted control a name for screen readers and tests.
pub fn label_widget(resp: &egui::Response, name: &str) {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, resp.enabled(), name));
}

/// Dark, heavier text for headings. The built-in font has no bold, so it is drawn twice.
pub fn bold(ui: &mut Ui, text: &str) -> egui::Response {
    bold_sized(ui, text, 12.5, TEXT)
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
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, LINE));
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

pub fn number(ui: &mut Ui, value: &mut f64, width: f32) -> bool {
    let id = ui.next_auto_id();
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| toolpath::trim(*value));
    let resp = ui.add(TextEdit::singleline(&mut text).id(id).desired_width(width).margin(vec2(3.0, 1.0)));
    let mut changed = false;
    if resp.has_focus() {
        if let Ok(v) = text.trim().parse::<f64>() {
            if v.is_finite() && v != *value {
                *value = v;
                changed = true;
            }
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
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

pub(crate) fn labelled(ui: &mut Ui, label: &str, label_w: f32, value: &mut f64, width: f32, unit: &str) -> bool {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(label_w, 18.0), Sense::hover());
        ui.painter().text(r.right_center() - vec2(1.0, 0.0), Align2::RIGHT_CENTER, label, FontId::proportional(12.5), TEXT);
        let changed = number(ui, value, width);
        if !unit.is_empty() {
            ui.add(egui::Label::new(unit).wrap_mode(egui::TextWrapMode::Extend));
        }
        changed
    })
    .inner
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    if active {
        ui.painter().rect_filled(rect, 7.0, theme::ACCENT_SOFT);
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, theme::BUTTON);
    }
    icons::paint(ui.painter(), rect.shrink(2.0), icon, enabled);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    resp.on_hover_text(tip)
}

impl App {
    /// A section's title, which folds it away when clicked. Says whether what's under it is showing.
    pub(crate) fn section_open(&mut self, ui: &mut Ui, title: &str) -> bool {
        let mut open = !self.folded.iter().any(|f| f == title);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, theme::CARD);
        }
        let colour = if resp.hovered() { TEXT } else { theme::MUTED };
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
    ui.painter().rect_filled(rect, 12.0, if resp.hovered() { theme::BUTTON } else { CARD });
    icons::paint(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 16.0), vec2(30.0, 30.0)), icon, true);
    ui.painter().text(rect.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, label, FontId::proportional(13.0), TEXT);
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
        ui.add(egui::Label::new(RichText::new(title).size(15.0).strong().color(TEXT)).truncate());
    });
}


/// A flat tab: the chosen one is lifted onto a card with an accent underline.
fn side_tab(ui: &Ui, rect: Rect, label: &str, active: bool) -> egui::Response {
    let resp = ui.interact(rect, ui.make_persistent_id(("tab", label)), Sense::click());
    if active {
        ui.painter().rect_filled(rect, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, CARD);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(2.0, ACCENT));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, theme::BUTTON);
    }
    let colour = if active || resp.hovered() { TEXT } else { MUTED };
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

impl App {
    // ----- File dialogs -----

    pub fn dialog_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Projects", &["burin", "crv3d", "crv"]).add_filter("Burin project", &["burin"]).add_filter(".crv3d project", &["crv3d", "crv"]).pick_file() {
            if let Err(e) = self.open_path(path) {
                self.message = Some(e);
            }
        }
    }

    pub fn dialog_save(&mut self, save_as: bool) {
        let target = match (&self.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter("Burin project", &["burin"]).set_file_name("project.burin").save_file(),
        };
        if let Some(path) = target {
            if let Err(e) = self.save_to(path) {
                self.message = Some(e);
            }
        }
    }

    pub fn dialog_import(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Drawing", &["svg", "dxf"]).pick_file() else { return };
        if let Err(why) = self.import_drawing(&path) {
            self.message = Some(why);
        }
    }

    /// Adds the shapes of a DXF or SVG file to the job, selected.
    pub fn import_drawing(&mut self, path: &std::path::Path) -> Result<(), String> {
        // Older drawing files are often in other character sets; only the numbers matter here.
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
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
            return Err(format!("{} has no lines, curves or shapes this program can read.", path.display()));
        }
        self.checkpoint();
        self.selected.clear();
        for paths in objects {
            let id = self.doc.add_vector(paths, Prim::Free);
            self.selected.push(id);
        }
        self.view3d = false;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let mut note = format!("Imported {name}.");
        if skipped > 0 {
            note += &format!(" {skipped} objects were left out: text must be converted to outlines first.");
        }
        if masked > 0 {
            note += &format!(" {masked} objects are hidden in part by a mask in the file: they come in whole.");
        }
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let picked = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id));
        if geom::bbox(picked.flat_map(|v| v.paths.iter())).is_some_and(|(lo, hi)| lo[0] < -1e-6 || lo[1] < -1e-6 || hi[0] > w + 1e-6 || hi[1] > h + 1e-6) {
            note += " Part of it lies outside the job.";
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
            self.status = "There are no shapes to export.".into();
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("Drawing", &["dxf"]).set_file_name("shapes.dxf").save_file() {
            match std::fs::write(&path, geom::dxf_write(&paths)) {
                Ok(()) => self.status = format!("Exported {}", path.display()),
                Err(e) => self.message = Some(format!("Could not write {}: {e}", path.display())),
            }
        }
    }


    // ----- The whole window -----

    pub fn frame(&mut self, ui: &mut Ui) {
        self.poll_recalc();
        self.keyboard(ui);
        self.maintain_constraints();
        if self.anim.playing {
            let total: usize = self.anim.queue.iter().filter_map(|i| self.doc.toolpaths.get(*i)).map(|t| t.moves.len()).sum();
            let mut left = if self.anim.rush { usize::MAX } else { 1 + (self.speed.powi(4) * total as f32 / 45.0) as usize };
            // A little at a time, and never for long: the window must go on answering.
            let started = std::time::Instant::now();
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

        egui::Panel::top("menu").exact_size(34.0).frame(Frame::new().fill(CHROME).inner_margin(Margin::symmetric(8, 3))).show(ui, |ui| self.menu_bar(ui));
        if self.screen == Screen::Start {
            self.start_screen(ui);
        } else {
            egui::Panel::bottom("status").exact_size(22.0).frame(Frame::new().fill(CHROME).inner_margin(Margin::symmetric(4, 2))).show(ui, |ui| self.status_bar(ui));
            self.left_panel(ui);
            self.right_panel(ui);
            egui::CentralPanel::default().frame(Frame::new().fill(WORKSPACE)).show(ui, |ui| self.centre(ui));
        }
        self.windows(ui);
        // Last, so that a shape let go this frame is followed at once, whatever is on the screen.
        if self.follow_shapes() > 0 {
            ui.ctx().request_repaint();
        }
        // Started just now, by the shape let go or a button above.
        if self.recalc.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// The arrow keys move the selection: Shift ten times as far, Ctrl a hundred, Alt a tenth.
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
        let step = if metric { 1.0 } else { 0.05 } * times;
        if !self.nudging {
            self.checkpoint();
            self.nudging = true;
        }
        self.translate_selected(dx * step, dy * step);
        self.status = format!("Moved the selection by {} {}. Shift: x10, Ctrl: x100, Alt: x0.1.", toolpath::trim(step), self.doc.job.unit());
    }

    fn keyboard(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.nudge(&ctx);
        if self.recalc.is_some() {
            return;
        }
        if ctx.egui_wants_keyboard_input() || self.screen != Screen::Work {
            return;
        }
        use egui::Key;
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            // A chosen rule goes first, and the shapes stay.
            match self.selected_rule.filter(|_| matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Constraints)) {
                Some(i) => self.remove_rule(i),
                None => self.delete_selected(),
            }
        }
        if pressed(Key::Escape) {
            if let LeftForm::Draw(f) = &mut self.left {
                f.points.clear();
            }
            self.picks.clear();
            self.close_left_form();
            self.tool = Tool2::Select;
        }
        if pressed(Key::Enter) && matches!(&self.left, LeftForm::Draw(f) if !f.points.is_empty()) {
            self.finish_line(false);
        }
        if cmd {
            if pressed(Key::Z) {
                if shift {
                    self.do_redo();
                } else {
                    self.do_undo();
                }
            }
            if pressed(Key::Y) {
                self.do_redo();
            }
            if pressed(Key::A) {
                if shift {
                    let tol = self.default_similar_tolerance();
                    self.select_similar(tol);
                } else {
                    self.select_all();
                }
            }
            if pressed(Key::S) {
                self.dialog_save(shift);
            }
            if pressed(Key::O) {
                self.dialog_open();
            }
        }
        let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        let cut = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Cut)));
        let paste = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))));
        if copy {
            self.copy_selected();
        }
        if cut {
            self.copy_selected();
            self.delete_selected();
        }
        if paste {
            self.paste();
        }
    }

    pub fn select_all(&mut self) {
        self.selected = self.doc.vectors.iter().filter(|v| self.doc.layers.get(v.layer).is_none_or(|l| l.visible)).map(|v| v.id).collect();
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
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_file();
                    ui.close();
                }
                if ui.button("Open...").clicked() {
                    ui.close();
                    self.dialog_open();
                }
                if ui.add_enabled(work, egui::Button::new("Save")).clicked() {
                    ui.close();
                    self.dialog_save(false);
                }
                if ui.add_enabled(work, egui::Button::new("Save As...")).clicked() {
                    ui.close();
                    self.dialog_save(true);
                }
                ui.separator();
                if ui.add_enabled(work, egui::Button::new("Import Shapes (SVG, DXF)...")).clicked() {
                    ui.close();
                    self.dialog_import();
                }
                if ui.add_enabled(work, egui::Button::new("Import 3D Model (STL)...")).clicked() {
                    ui.close();
                    self.dialog_import_model();
                }
                if ui.add_enabled(work, egui::Button::new("3D Relief from Picture...")).clicked() {
                    ui.close();
                    self.dialog_relief_picture();
                }
                if ui.add_enabled(work, egui::Button::new("Import Bitmap / Trace...")).clicked() {
                    ui.close();
                    self.start_trace();
                }
                if ui.add_enabled(work, egui::Button::new("Export Shapes (DXF)...")).clicked() {
                    ui.close();
                    self.dialog_export();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            if work {
                ui.menu_button("Edit", |ui| {
                    if ui.button("Move X/Y Zero...").clicked() {
                        self.start_xform(Xform::Move);
                        ui.close();
                    }
                    ui.separator();
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked() {
                        self.do_undo();
                        ui.close();
                    }
                    if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).clicked() {
                        self.do_redo();
                        ui.close();
                    }
                    ui.separator();
                    let some = !self.selected.is_empty();
                    if ui.add_enabled(some, egui::Button::new("Cut")).clicked() {
                        self.copy_selected();
                        self.delete_selected();
                        ui.close();
                    }
                    if ui.add_enabled(some, egui::Button::new("Copy")).clicked() {
                        self.copy_selected();
                        ui.close();
                    }
                    if ui.add_enabled(!self.clipboard.is_empty(), egui::Button::new("Paste")).clicked() {
                        self.paste();
                        ui.close();
                    }
                    if ui.add_enabled(some, egui::Button::new("Delete")).clicked() {
                        self.delete_selected();
                        ui.close();
                    }
                    if ui.button("Select All").clicked() {
                        self.select_all();
                        ui.close();
                    }
                    if ui.add_enabled(some, egui::Button::new("Select Similar")).clicked() {
                        let tol = self.default_similar_tolerance();
                        self.select_similar(tol);
                        ui.close();
                    }
                    if ui.button("Modify Several Shapes...").clicked() {
                        self.start_xform(Xform::Bulk);
                        ui.close();
                    }
                    if ui.button("Drawing Constraints...").clicked() {
                        self.left_tab = LeftTab::Drawing;
                        self.start_constraint_form();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Job Setup...").clicked() {
                        self.open_job_setup();
                        ui.close();
                    }
                });
                ui.menu_button("Model", |ui| {
                    if ui.button("Create Shape from Selection...").clicked() {
                        self.open_shape_form(None);
                        ui.close();
                    }
                    if ui.add_enabled(!self.doc.components.is_empty(), egui::Button::new("Delete All Shapes")).clicked() {
                        self.checkpoint();
                        self.doc.components.clear();
                        self.selected_comp = None;
                        self.model_dirty = true;
                        ui.close();
                    }
                });
                ui.menu_button("Toolpaths", |ui| {
                    for (kind, label) in [
                        (OpKind::Profile, "Profile..."),
                        (OpKind::Pocket, "Pocket..."),
                        (OpKind::Drill, "Drill..."),
                        (OpKind::Engrave, "Quick Engrave..."),
                        (OpKind::VCarve, "V-Carve..."),
                        (OpKind::Rough3D, "3D Roughing..."),
                        (OpKind::Finish3D, "3D Finishing..."),
                        (OpKind::Prism, "Prism Carve..."),
                        (OpKind::Texture, "Texture..."),
                        (OpKind::Moulding, "Moulding..."),
                        (OpKind::Flute, "Fluting..."),
                        (OpKind::Chamfer, "Chamfer..."),
                    ] {
                        if ui.button(label).clicked() {
                            self.start_op(kind);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Tool Database...").clicked() {
                        self.open_tool_list(None);
                        ui.close();
                    }
                    if ui.button("Recalculate All").clicked() {
                        self.recalculate_all();
                        ui.close();
                    }
                    if ui.button("Save Toolpaths...").clicked() {
                        ui.close();
                        self.right = RightMode::Save;
                        self.right_open = true;
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.selectable_label(!self.view3d, "2D View").clicked() {
                        self.view3d = false;
                        ui.close();
                    }
                    if ui.selectable_label(self.view3d, "3D View").clicked() {
                        self.view3d = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.grid, "Show Grid");
                    ui.checkbox(&mut self.snap, "Snap to Grid");
                    ui.checkbox(&mut self.snap_shapes, "Snap to Shapes").on_hover_text("Hold Alt to turn it off for a moment.");
                    ui.checkbox(&mut self.show_guides, "Show Guides").on_hover_text("Drag from a ruler onto the sheet to add one. Drag it back to remove it.");
                    if ui.add_enabled(!self.doc.guides.is_empty(), egui::Button::new("Remove All Guides")).clicked() {
                        self.checkpoint();
                        self.doc.guides.clear();
                        ui.close();
                    }
                    ui.checkbox(&mut self.show_vectors, "Show Shapes");
                    ui.checkbox(&mut self.show_toolpaths, "Show Toolpaths");
                });
            }
            ui.menu_button("Gadgets", |ui| {
                ui.add_enabled(false, egui::Label::new("No gadgets installed"));
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });
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
            ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, LINE));
        };
        // The job's size stays on the far right, however little room is left.
        let job = self.doc.job.clone();
        let text = format!("{} \u{d7} {} \u{d7} {} {}", toolpath::trim(job.width), toolpath::trim(job.height), toolpath::trim(job.thickness), job.unit());
        let galley = ui.painter().layout_no_wrap(text, FontId::proportional(12.5), TEXT);
        let chip_width = galley.size().x + 44.0;
        ui.add_space(6.0);
        divider(ui);
        ui.spacing_mut().item_spacing.x = 1.0;
        let actions: [(Icon, &str, bool); 3] = [(Icon::New, "New", true), (Icon::Open, "Open", true), (Icon::Save, "Save", true)];
        for (icon, tip, on) in actions {
            if icon_button(ui, icon, tip, on, false).clicked() {
                match icon {
                    Icon::New => self.new_file(),
                    Icon::Open => self.dialog_open(),
                    _ => self.dialog_save(false),
                }
            }
        }
        divider(ui);
        if icon_button(ui, Icon::Undo, "Undo", !self.undo.is_empty(), false).clicked() {
            self.do_undo();
        }
        if icon_button(ui, Icon::Redo, "Redo", !self.redo.is_empty(), false).clicked() {
            self.do_redo();
        }
        // Everything from here on is also in the menus, so it can give way on a narrow window.
        const ICON: f32 = 29.0;
        const DIVIDER: f32 = 15.0;
        let room = ui.available_width() - chip_width - 12.0;
        if room >= 4.0 * ICON + DIVIDER {
            divider(ui);
            if icon_button(ui, Icon::Cut, "Cut", some, false).clicked() {
                self.copy_selected();
                self.delete_selected();
            }
            if icon_button(ui, Icon::Copy, "Copy", some, false).clicked() {
                self.copy_selected();
            }
            if icon_button(ui, Icon::Paste, "Paste", !self.clipboard.is_empty(), false).clicked() {
                self.paste();
            }
            if icon_button(ui, Icon::SelectAll, "Select all", true, false).clicked() {
                self.select_all();
            }
        }
        if room >= 6.0 * ICON + 2.0 * DIVIDER {
            divider(ui);
            if icon_button(ui, Icon::Import, "Import shapes", true, false).clicked() {
                self.dialog_import();
            }
            if icon_button(ui, Icon::Export, "Export shapes", true, false).clicked() {
                self.dialog_export();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(chip_width, 24.0), Sense::click());
            let lit = resp.hovered();
            ui.painter().rect(rect, 12.0, if lit { theme::BUTTON_HOVER } else { theme::CARD }, Stroke::NONE, StrokeKind::Inside);
            icons::paint(ui.painter(), Rect::from_center_size(rect.left_center() + vec2(15.0, 0.0), vec2(20.0, 20.0)), Icon::JobSetup, true);
            ui.painter().galley(pos2(rect.left() + 29.0, rect.center().y - galley.size().y / 2.0), galley.clone(), TEXT);
            label_widget(&resp, "Job Setup...");
            let tip = format!("Job setup: change the size, thickness, units and origin.\nOrigin: {}", job.datum.label());
            if resp.on_hover_text(tip).clicked() {
                self.open_job_setup();
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        let font = FontId::proportional(12.0);
        let c = TEXT;
        // The status message gives way to the sketch state when the window is narrow.
        let state_width = self.definition_text().map_or(0.0, |(text, colour)| p.layout_no_wrap(text, font.clone(), colour).size().x + 18.0);
        let status_end = (rect.right() - 546.0 - state_width).max(rect.left() + 40.0);
        let status_area = Rect::from_min_max(rect.min, pos2(status_end, rect.bottom()));
        p.with_clip_rect(status_area).text(rect.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, &self.status, font.clone(), c);
        let x = rect.right() - 540.0;
        let y = rect.right() - 358.0;
        if self.grid && !self.view3d {
            let unit = self.doc.job.unit();
            p.text(pos2(y + 186.0, rect.center().y), Align2::LEFT_CENTER, format!("Grid {} {unit}", toolpath::trim(self.grid_steps().0)), font.clone(), MUTED);
        }
        if let Some((text, colour)) = self.definition_text() {
            p.text(pos2(x - 12.0, rect.center().y), Align2::RIGHT_CENTER, text, font.clone(), colour);
        }
        let line = Stroke::new(1.0, LINE);
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
                        pixels.push(Color32::from_rgb((PANEL.r() as f32 + v) as u8, (PANEL.g() as f32 + v) as u8, (PANEL.b() as f32 + v) as u8));
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
        egui::CentralPanel::default().frame(Frame::new().fill(theme::WORKSPACE)).show(ui, |ui| {
            let whole = ui.max_rect();
            let width = 460.0_f32.min(whole.width() - 32.0);
            let recent = self.recent.clone();
            let height = 300.0 + if recent.is_empty() { 0.0 } else { 34.0 + recent.len() as f32 * 26.0 };
            let top = (whole.center().y - height / 2.0).max(whole.top() + 16.0);
            let column = Rect::from_min_max(pos2(whole.center().x - width / 2.0, top), pos2(whole.center().x + width / 2.0, whole.bottom()));
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(column));
            let ui = &mut body;
            let (logo, _) = ui.allocate_exact_size(vec2(width, 76.0), Sense::hover());
            let p = ui.painter();
            p.text(logo.center(), Align2::CENTER_CENTER, "Burin", FontId::proportional(64.0), TEXT);
            let tip = logo.center() + vec2(78.0, 0.0);
            p.add(Shape::convex_polygon(vec![tip + vec2(0.0, 24.0), tip + vec2(30.0, -24.0), tip + vec2(22.0, 22.0)], ACCENT, Stroke::NONE));
            let (line, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
            ui.painter().text(line.center(), Align2::CENTER_CENTER, "Design it, then carve it", FontId::proportional(14.0), theme::MUTED);
            ui.add_space(26.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let tile = (width - 12.0) / 2.0;
                if start_tile(ui, Icon::Doc, "Create a new file", tile) {
                    self.new_file();
                }
                if start_tile(ui, Icon::Folder, "Open an existing file", tile) {
                    self.dialog_open();
                }
            });
            ui.add_space(24.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            if !recent.is_empty() {
                bold_sized(ui, "Recently opened files", 12.5, theme::MUTED);
                for path in recent {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if self.start_link(ui, Icon::Doc, &name) {
                        if let Err(e) = self.open_path(path) {
                            self.message = Some(e);
                        }
                    }
                }
                ui.add_space(10.0);
            }
            if self.start_link(ui, Icon::Sample, "Open the sample project") {
                self.load_sample();
            }
        });
    }

    pub(crate) fn start_link(&mut self, ui: &mut Ui, icon: Icon, label: &str) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, CARD);
        }
        icons::paint(ui.painter(), Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(20.0, 20.0)), icon, true);
        ui.painter().text(rect.left_center() + vec2(32.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), TEXT);
        label_widget(&resp, label);
        resp.clicked()
    }

    // ----- Left panel -----

    fn left_panel(&mut self, ui: &mut Ui) {
        if !self.left_open {
            self.collapsed_strip(ui, true);
            return;
        }
        let job_open = matches!(self.left, LeftForm::Job { .. });
        egui::Panel::left("left").exact_size(SIDE_W).resizable(false).frame(Frame::new().fill(PANEL)).show(ui, |ui| {
            let whole = ui.max_rect();
            if job_open {
                self.paint_ornament(ui, whole, whole.top() + 660.0);
                let inner = whole.shrink2(vec2(10.0, 8.0));
                let mut body = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                self.job_setup(&mut body);
                return;
            }
            let header = Rect::from_min_size(whole.min, vec2(whole.width(), 22.0));
            let tabs_h = 26.0;
            let tabs = Rect::from_min_max(pos2(whole.left(), whole.bottom() - tabs_h), whole.max);
            let body = Rect::from_min_max(pos2(whole.left(), header.bottom()), pos2(whole.right(), tabs.top()));
            let title = match self.left_tab {
                LeftTab::Drawing => "Drawing",
                LeftTab::Modeling => "Modeling",
                LeftTab::Clipart => "Clipart",
                LeftTab::Layers => "Layers",
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
            // Tabs along the bottom.
            let strip = ui.painter();
            strip.rect_filled(tabs, 0.0, CHROME);
            let specs = [(LeftTab::Drawing, "Drawing", 58.0), (LeftTab::Modeling, "Modeling", 64.0), (LeftTab::Clipart, "Clipart", 52.0), (LeftTab::Layers, "Layers", 56.0)];
            let mut x = tabs.left() + 2.0;
            for (tab, label, w) in specs {
                let active = self.left_tab == tab;
                let r = Rect::from_min_size(pos2(x, tabs.top() + 2.0), vec2(w, tabs_h - 4.0));
                if side_tab(ui, r, label, active).clicked() {
                    self.left_tab = tab;
                    self.close_left_form();
                }
                x += w;
            }
        });
    }

    fn collapsed_strip(&mut self, ui: &mut Ui, left: bool) {
        let panel = if left { egui::Panel::left("left_strip") } else { egui::Panel::right("right_strip") };
        let label = if left { "Drawing" } else { "Toolpaths" };
        panel.exact_size(22.0).resizable(false).frame(Frame::new().fill(CHROME)).show(ui, |ui| {
            let r = ui.max_rect();
            let tab = Rect::from_min_size(r.min + vec2(1.0, 6.0), vec2(20.0, 70.0));
            let resp = ui.interact(tab, ui.scope_id().with("open"), Sense::click());
            ui.painter().rect(tab, 4.0, CARD, Stroke::NONE, StrokeKind::Inside);
            let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.0), TEXT);
            let at = tab.left_top() + vec2(16.0, 6.0);
            ui.painter().add(egui::epaint::TextShape::new(at, galley, TEXT).with_angle(std::f32::consts::FRAC_PI_2));
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
        ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(12.5), TEXT);
        let b = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(16.0, 16.0));
        let name = if left { "Hide left panel" } else { "Hide right panel" };
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
        Frame::new().fill(PANEL).corner_radius(6).inner_margin(Margin::same(4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                wood_block(ui.painter(), r);
                ui.add_space(8.0);
                bold(ui, "Job Setup");
            });
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        bold(ui, "Job Type");
                        let (r, _) = ui.allocate_exact_size(vec2(58.0, 32.0), Sense::hover());
                        wood_block(ui.painter(), r);
                    });
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        let _ = ui.radio(true, "Single Sided");
                        ui.add_enabled(false, egui::RadioButton::new(false, "Double Sided")).on_disabled_hover_text("Not in this version");
                        ui.add_enabled(false, egui::RadioButton::new(false, "Rotary")).on_disabled_hover_text("Not in this version");
                    });
                });
            });
            let mut size_changed = false;
            let unit = self.doc.job.unit();
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(56.0);
                        bold(ui, "Job Size");
                        let (r, _) = ui.allocate_exact_size(vec2(58.0, 44.0), Sense::hover());
                        wood_block(ui.painter(), r);
                    });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        size_changed |= labelled(ui, "Width (X):", 54.0, &mut self.doc.job.width, 40.0, unit);
                        size_changed |= labelled(ui, "Height (Y):", 54.0, &mut self.doc.job.height, 40.0, unit);
                        size_changed |= labelled(ui, "Thickness", 54.0, &mut self.doc.job.thickness, 40.0, unit);
                    });
                });
                if labelled(ui, "Sacrificial board", 96.0, &mut self.doc.job.spoil_board, 40.0, unit) {
                    self.doc.job.spoil_board = self.doc.job.spoil_board.max(0.0);
                }
                ui.label(RichText::new("Cutting into it brings no warning; going past it does.").size(11.5).color(MUTED));
                ui.horizontal(|ui| {
                    bold(ui, "Units");
                    ui.add_space(14.0);
                    let inches = self.doc.job.units == Units::Inches;
                    if ui.radio(inches, "inches").clicked() {
                        self.doc.set_units(Units::Inches);
                        size_changed = true;
                    }
                    ui.add_space(12.0);
                    if ui.radio(!inches, "mm").clicked() {
                        self.doc.set_units(Units::Mm);
                        size_changed = true;
                    }
                });
            });
            card(ui, |ui| {
                bold(ui, "Z Zero Position");
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(60.0, 40.0), Sense::hover());
                    wood_block(ui.painter(), r);
                    let datum_y = if self.doc.job.z_on_bed { r.bottom() - 4.0 } else { r.top() + 16.0 };
                    ui.painter().circle_filled(pos2(r.left() + 4.0, datum_y), 4.0, Color32::from_rgb(200, 30, 30));
                    ui.add_space(26.0);
                    ui.vertical(|ui| {
                        if ui.radio(!self.doc.job.z_on_bed, "Material Surface").clicked() {
                            self.doc.job.z_on_bed = false;
                        }
                        if ui.radio(self.doc.job.z_on_bed, "Machine Bed").clicked() {
                            self.doc.job.z_on_bed = true;
                        }
                    });
                });
            });
            card(ui, |ui| {
                bold(ui, "XY Datum Position");
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(52.0, 56.0), Sense::hover());
                    wood_block(ui.painter(), Rect::from_center_size(r.center(), vec2(52.0, 30.0)));
                    let (square, _) = ui.allocate_exact_size(vec2(58.0, 58.0), Sense::hover());
                    let inner = square.shrink(7.0);
                    ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
                    for (spot, datum) in [
                        (inner.left_top(), Datum::TopLeft),
                        (inner.right_top(), Datum::TopRight),
                        (inner.center(), Datum::Centre),
                        (inner.left_bottom(), Datum::BottomLeft),
                        (inner.right_bottom(), Datum::BottomRight),
                    ] {
                        let hit = Rect::from_center_size(spot, vec2(14.0, 14.0));
                        let resp = ui.interact(hit, ui.scope_id().with(("datum", format!("{datum:?}"))), Sense::click());
                        ui.painter().circle(spot, 5.5, INPUT, Stroke::new(1.0, MUTED));
                        if self.doc.job.datum == datum {
                            ui.painter().circle_filled(spot, 3.0, ACCENT);
                        }
                        label_widget(&resp, &format!("Datum {datum:?}"));
                        if resp.clicked() {
                            self.doc.job.datum = datum;
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.checkbox(&mut self.doc.job.use_offset, "Use Offset");
                        let on = self.doc.job.use_offset;
                        ui.add_enabled_ui(on, |ui| {
                            labelled(ui, "X:", 14.0, &mut self.doc.job.offset[0], 46.0, "");
                            labelled(ui, "Y:", 14.0, &mut self.doc.job.offset[1], 46.0, "");
                        });
                    });
                });
            });
            card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                bold(ui, "Modeling Resolution");
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::BlockCopy, true);
                    let before = self.doc.job.resolution;
                    egui::ComboBox::from_id_salt("res").selected_text(RESOLUTIONS[before.min(3)].0).width(160.0).show_ui(ui, |ui| {
                        for (i, r) in RESOLUTIONS.iter().enumerate() {
                            ui.selectable_value(&mut self.doc.job.resolution, i, r.0);
                        }
                    });
                    if self.doc.job.resolution != before {
                        self.model_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(32.0);
                    ui.label(RichText::new(RESOLUTIONS[self.doc.job.resolution.min(3)].1).size(11.5).color(MUTED));
                });
                bold(ui, "Appearance");
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 10.0, Color32::from_rgb(240, 236, 220));
                    for (o, c) in [(vec2(-5.0, -2.0), Color32::RED), (vec2(0.0, -5.0), Color32::from_rgb(240, 190, 30)), (vec2(5.0, -2.0), Color32::from_rgb(40, 150, 60)), (vec2(3.0, 4.0), Color32::from_rgb(40, 90, 220))] {
                        ui.painter().circle_filled(r.center() + o, 2.0, c);
                    }
                    let name = self.doc.job.material.map_or("Solid Color", |m| MATERIALS[m.min(4)].0);
                    egui::ComboBox::from_id_salt("appearance").selected_text(name).width(160.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.doc.job.material, None, "Solid Color");
                        for (i, m) in MATERIALS.iter().enumerate() {
                            ui.selectable_value(&mut self.doc.job.material, Some(i), m.0);
                        }
                    });
                });
                if self.doc.job.material.is_none() {
                    ui.horizontal(|ui| {
                        ui.add_space(52.0);
                        ui.label("Solid Color:");
                        ui.color_edit_button_srgb(&mut self.doc.job.solid);
                    });
                }
            });
            ui.horizontal(|ui| {
                if primary(ui, "OK", 74.0).clicked() {
                    self.confirm_job();
                }
                ui.add_space(ui.available_width() - 76.0);
                if wide(ui, "Cancel", 74.0).clicked() {
                    self.cancel_job();
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

    pub fn confirm_job(&mut self) {
        let j = &mut self.doc.job;
        j.width = j.width.max(0.01);
        j.height = j.height.max(0.01);
        j.thickness = j.thickness.max(0.001);
        // Editing an existing job is one step of undo, and what it affects gets pointed out.
        if let LeftForm::Job { backup: Some(before), .. } = &self.left {
            let (old, new) = (&before.job, &self.doc.job);
            if serde_json::to_string(old).ok() != serde_json::to_string(new).ok() {
                let reshaped = (old.width, old.height, old.thickness, old.units) != (new.width, new.height, new.thickness, new.units);
                let (w, h) = (new.width, new.height);
                let outside = crate::geom::bbox(self.doc.vectors.iter().flat_map(|v| v.paths.iter())).is_some_and(|(lo, hi)| lo[0] < -1e-6 || lo[1] < -1e-6 || hi[0] > w + 1e-6 || hi[1] > h + 1e-6);
                self.status = match (outside, reshaped && !self.doc.toolpaths.is_empty()) {
                    (true, true) => "Job changed: some shapes are now outside the job, and the toolpaths need calculating again.".into(),
                    (true, false) => "Job changed: some shapes are now outside the job.".into(),
                    (false, true) => "Job changed: the toolpaths need calculating again.".into(),
                    (false, false) => "Job changed.".into(),
                };
                let LeftForm::Job { backup: Some(before), .. } = std::mem::replace(&mut self.left, LeftForm::None) else { unreachable!() };
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
        if !chosen.is_some_and(|v| form.load_shape(v)) {
            self.selected.clear();
        }
        self.left = LeftForm::Draw(form);
        self.view3d = false;
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
            if wide(ui, "Close", 74.0).clicked() {
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
        if editing {
            title_card(ui, icon, &f.kind.title().replacen("Draw", "Edit", 1));
        } else {
            title_card(ui, icon, f.kind.title());
        }
        let unit = self.doc.job.unit();
        let line_tool = matches!(f.kind, DrawKind::Polyline | DrawKind::Curve);
        if line_tool {
            card(ui, |ui| {
                ui.label("Click on the sheet to add points.");
                ui.label("Double click or press Enter to finish.");
                ui.label("Click the first point to close the shape.");
                ui.add_space(4.0);
                ui.label(format!("Points: {}", f.points.len()));
            });
        } else {
            card(ui, |ui| {
                bold(ui, if f.kind == DrawKind::Text { "Start Point" } else { "Center Point" });
                let zero = self.doc.job.origin();
                ui.horizontal(|ui| {
                    ui.label("X:");
                    number_from(ui, &mut f.c[0], zero[0], 54.0);
                    ui.add_space(4.0);
                    ui.label("Y:");
                    number_from(ui, &mut f.c[1], zero[1], 54.0);
                });
                ui.add_space(6.0);
                match f.kind {
                    DrawKind::Circle => {
                        ui.horizontal(|ui| {
                            if ui.radio(!f.use_dia, "Radius").clicked() {
                                f.use_dia = false;
                            }
                            ui.add_space(12.0);
                            if ui.radio(f.use_dia, "Diameter").clicked() {
                                f.use_dia = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(if f.use_dia { "D:" } else { "R:" });
                            let mut shown = if f.use_dia { f.r * 2.0 } else { f.r };
                            if number(ui, &mut shown, 54.0) {
                                f.r = if f.use_dia { shown / 2.0 } else { shown };
                            }
                            ui.label(unit);
                        });
                    }
                    DrawKind::Ellipse => {
                        labelled(ui, "Radius X:", 54.0, &mut f.rx, 54.0, unit);
                        labelled(ui, "Radius Y:", 54.0, &mut f.ry, 54.0, unit);
                    }
                    DrawKind::Rect => {
                        labelled(ui, "Width:", 54.0, &mut f.w, 54.0, unit);
                        labelled(ui, "Height:", 54.0, &mut f.h, 54.0, unit);
                        labelled(ui, "Corners:", 54.0, &mut f.corner, 54.0, unit);
                    }
                    DrawKind::Polygon => {
                        labelled(ui, "Radius:", 54.0, &mut f.r, 54.0, unit);
                        ui.horizontal(|ui| {
                            ui.label("Sides:");
                            count_box(ui, &mut f.sides, 54.0);
                        });
                    }
                    DrawKind::Star => {
                        labelled(ui, "Outer:", 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, "Inner:", 54.0, &mut f.inner, 54.0, unit);
                        ui.horizontal(|ui| {
                            ui.label("Points:");
                            count_box(ui, &mut f.sides, 54.0);
                        });
                    }
                    DrawKind::Spiral => {
                        labelled(ui, "Radius:", 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, "Turns:", 54.0, &mut f.turns, 54.0, "");
                    }
                    DrawKind::Arc => {
                        labelled(ui, "Radius:", 54.0, &mut f.r, 54.0, unit);
                        labelled(ui, "Start:", 54.0, &mut f.start, 54.0, "degrees");
                        labelled(ui, "Sweep:", 54.0, &mut f.sweep, 54.0, "degrees");
                    }
                    DrawKind::Text => {
                        ui.add(TextEdit::multiline(&mut f.text).desired_rows(2).desired_width(f32::INFINITY));
                        labelled(ui, "Height:", 54.0, &mut f.text_height, 54.0, unit);
                    }
                    _ => {}
                }
            });
            card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if editing {
                        ui.label("Apply changes this shape.");
                        ui.label("Draw on the sheet to make a new one.");
                    } else {
                        ui.label("To edit an existing shape");
                        ui.label("hold shift while selecting");
                    }
                });
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
        if self.form_buttons(ui, "Apply") {
            self.apply_draw();
        }
    }


    fn shape_form(&mut self, ui: &mut Ui, mut f: ShapeForm) {
        title_card(ui, Icon::Dome, if f.editing.is_some() { "Edit 3D Shape" } else { "Create 3D Shape" });
        let unit = self.doc.job.unit();
        card(ui, |ui| {
            bold(ui, "Profile");
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.profile, Profile::Round, "Round");
                ui.radio_value(&mut f.profile, Profile::Angular, "Angular");
                ui.radio_value(&mut f.profile, Profile::Flat, "Flat");
            });
            ui.add_space(4.0);
            labelled(ui, "Height:", 66.0, &mut f.height, 54.0, unit);
            labelled(ui, "Base Height:", 66.0, &mut f.base, 54.0, unit);
        });
        card(ui, |ui| {
            bold(ui, "Combine with the model");
            ui.radio_value(&mut f.combine, Combine::Add, "Add");
            ui.radio_value(&mut f.combine, Combine::Subtract, "Subtract");
            ui.radio_value(&mut f.combine, Combine::Merge, "Merge (keep highest)");
            ui.radio_value(&mut f.combine, Combine::Low, "Low (keep lowest)");
        });
        if f.editing.is_none() {
            card(ui, |ui| {
                ui.label(format!("Selected shapes: {}", self.selected.len()));
            });
        }
        self.left = LeftForm::Shape(f.clone());
        if self.form_buttons(ui, "Apply") {
            self.add_component(f);
        }
    }

    // ----- Right panel -----

    fn right_panel(&mut self, ui: &mut Ui) {
        if !self.right_open {
            self.collapsed_strip(ui, false);
            return;
        }
        egui::Panel::right("right").exact_size(RIGHT_W).resizable(false).frame(Frame::new().fill(PANEL)).show(ui, |ui| {
            let whole = ui.max_rect();
            let header = Rect::from_min_size(whole.min, vec2(whole.width(), 22.0));
            self.panel_header(ui, header, "Toolpaths", false);
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
        });
    }


    fn toolpath_list(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(3.0, LINE));
        let width = rect.width();
        ui.add_space(6.0);
        Frame::new().inner_margin(Margin { left: 12, right: 2, top: 2, bottom: 2 }).show(ui, |ui| {
            ui.set_width(width - 14.0);
            ui.horizontal(|ui| {
                let all = !self.doc.toolpaths.is_empty() && self.doc.toolpaths.iter().all(|t| t.visible);
                let mut toggle = all;
                if ui.checkbox(&mut toggle, "").on_hover_text("Show or hide all toolpaths").changed() {
                    for t in &mut self.doc.toolpaths {
                        t.visible = toggle;
                    }
                }
                bold_sized(ui, "Toolpaths", 13.5, TEXT);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let some = self.selected_tp.is_some_and(|i| i > 0);
                    if icon_button(ui, Icon::Up, "Move toolpath up", some, false).clicked() {
                        if let Some(i) = self.selected_tp {
                            self.checkpoint();
                            self.doc.toolpaths.swap(i, i - 1);
                            self.selected_tp = Some(i - 1);
                        }
                    }
                });
            });
            let r = ui.available_rect_before_wrap();
            ui.painter().line_segment([r.left_top(), r.right_top() - vec2(0.0, 0.0)], Stroke::new(1.0, LINE));
            ui.add_space(4.0);
            if self.doc.toolpaths.is_empty() {
                let rest = ui.available_rect_before_wrap();
                ui.painter().text(rest.center(), Align2::CENTER_CENTER, "No Toolpaths Created Yet", FontId::proportional(13.5), MUTED);
                return;
            }
            let mut clicked_edit = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for i in 0..self.doc.toolpaths.len() {
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
                        let galley = ui.painter().layout_no_wrap(name.clone(), FontId::proportional(13.5), if selected { ACCENT } else { TEXT });
                        let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(6.0, 2.0), Sense::click());
                        if selected {
                            ui.painter().rect_filled(rect, 3.0, theme::ACCENT_SOFT);
                        }
                        ui.painter().galley(rect.min + vec2(3.0, 1.0), galley, TEXT);
                        label_widget(&resp, &name);
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
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    bold(ui, "Machine Vectors ...");
                    ui.radio_value(&mut op.side, Side::Outside, "Outside / Right");
                    ui.radio_value(&mut op.side, Side::Inside, "Inside / Left");
                    ui.radio_value(&mut op.side, Side::On, "On");
                });
                let (r, _) = ui.allocate_exact_size(vec2(54.0, 62.0), Sense::hover());
                let square = Rect::from_center_size(r.center() + vec2(2.0, 4.0), vec2(42.0, 42.0));
                ui.painter().rect_stroke(square, 0.0, Stroke::new(1.0, MUTED), StrokeKind::Inside);
                let offset = match op.side {
                    Side::Outside => -9.0,
                    Side::Inside => 9.0,
                    Side::On => 0.0,
                };
                ui.painter().line_segment([square.left_bottom() + vec2(offset, 0.0), square.left_top() + vec2(offset, 0.0)], Stroke::new(1.5, Color32::from_rgb(210, 30, 30)));
                ui.painter().circle(square.left_top() + vec2(offset, -6.0), 5.0, Color32::from_rgb(240, 220, 110), Stroke::new(1.0, MUTED));
            });
            ui.horizontal(|ui| {
                bold(ui, "Direction");
                ui.add_space(6.0);
                ui.radio_value(&mut op.climb, true, "Climb");
                ui.radio_value(&mut op.climb, false, "Conventional");
            });
            labelled(ui, "Allowance offset", 96.0, &mut op.allowance, 50.0, unit);
        });
        if !self.show_advanced {
            return;
        }
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.checkbox(&mut op.last_pass, "Do Separate Last Pass");
            ui.add_enabled_ui(op.last_pass, |ui| {
                labelled(ui, "Allowance (A)", 96.0, &mut op.last_allowance, 50.0, unit);
                ui.horizontal(|ui| {
                    ui.add_space(50.0);
                    ui.checkbox(&mut op.last_reverse, "Reverse direction");
                });
            });
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.checkbox(&mut op.tabs, "Add tabs to toolpath");
            ui.add_enabled_ui(op.tabs, |ui| {
                labelled(ui, "Length", 96.0, &mut op.tab_length, 50.0, unit);
                labelled(ui, "Thickness", 96.0, &mut op.tab_thickness, 50.0, unit);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(96.0, 18.0), Sense::hover());
                    ui.painter().text(r.right_center() - vec2(1.0, 0.0), Align2::RIGHT_CENTER, "Tabs per shape", FontId::proportional(12.5), TEXT);
                    count_box(ui, &mut op.tab_count, 50.0);
                });
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for (i, name) in ["Ramp", "Leads", "Order", "Start At", "Corners"].iter().enumerate() {
                let active = *tab == i;
                let fill = if active { CARD } else { PANEL };
                let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 19.0), Sense::click());
                ui.painter().rect(rect, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, fill, Stroke::new(1.0, if active { ACCENT } else { LINE }), StrokeKind::Inside);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, *name, FontId::proportional(11.5), TEXT);
                label_widget(&resp, name);
                if resp.clicked() {
                    *tab = i;
                }
            }
        });
        ui.add_space(-6.0);
        card(ui, |ui| match *tab {
            0 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                use crate::toolpath::RampKind;
                ui.checkbox(&mut op.ramp, "Add ramps to toolpath");
                if op.ramp {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut op.ramp_kind, RampKind::Smooth, "Smooth");
                        ui.radio_value(&mut op.ramp_kind, RampKind::ZigZag, "Zig Zag");
                        ui.radio_value(&mut op.ramp_kind, RampKind::Spiral, "Spiral");
                    });
                    if op.ramp_kind == RampKind::Spiral {
                        ui.label(RichText::new("Slopes all the way around each closed shape.").size(11.5).color(MUTED));
                    } else {
                        ui.add_enabled_ui(op.lead, |ui| {
                            ui.checkbox(&mut op.ramp_on_lead, "Ramp on Lead In");
                        });
                        ui.add_enabled_ui(!(op.lead && op.ramp_on_lead), |ui| {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut op.ramp_by_angle, false, "Distance");
                                ui.radio_value(&mut op.ramp_by_angle, true, "Angle");
                            });
                            if op.ramp_by_angle {
                                labelled(ui, "Angle", 96.0, &mut op.ramp_angle, 50.0, "degrees");
                                labelled(ui, "Longest", 96.0, &mut op.ramp_angle_limit, 50.0, unit);
                            } else {
                                labelled(ui, "Distance", 96.0, &mut op.ramp_distance, 50.0, unit);
                            }
                        });
                    }
                }
            }
            1 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.checkbox(&mut op.lead, "Add leads");
                if op.lead {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut op.lead_circular, false, "Straight Line");
                        ui.radio_value(&mut op.lead_circular, true, "Circular");
                    });
                    if op.lead_circular {
                        labelled(ui, "Radius", 96.0, &mut op.lead_radius, 50.0, unit);
                    } else {
                        labelled(ui, "Angle", 96.0, &mut op.lead_angle, 50.0, "degrees");
                    }
                    labelled(ui, "Lead", 96.0, &mut op.lead_distance, 50.0, unit);
                    ui.checkbox(&mut op.lead_out, "Do lead out");
                    labelled(ui, "Overcut", 96.0, &mut op.overcut, 50.0, unit);
                }
            }
            2 => {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Select one or more ways to try. With several, the one with the shortest travel between shapes is used.").size(11.5).color(MUTED));
                let mut c = op.order_choices.unwrap_or_default();
                ui.checkbox(&mut c.selection, "Vector Selection Order");
                ui.checkbox(&mut c.left_to_right, "Left to Right");
                ui.checkbox(&mut c.bottom_to_top, "Bottom to Top");
                ui.checkbox(&mut c.grid, "Grid");
                ui.checkbox(&mut c.shortest, "Shortest Path");
                op.order_choices = Some(c);
                // Keeping the order of clicks needs the shapes gathered that way.
                op.order = if c.selection { Order::AsDrawn } else { Order::Nearest };
            }
            3 => {
                use crate::toolpath::StartAt;
                ui.spacing_mut().item_spacing.y = 2.0;
                if ui.radio(op.start_at == StartAt::Keep, "Keep Current Start Points").clicked() {
                    op.start_at = StartAt::Keep;
                    op.start_mid_span = false;
                }
                if ui.radio(op.start_at == StartAt::Optimize, "Optimize Start Points").clicked() {
                    op.start_at = StartAt::Optimize;
                }
                let placed = matches!(op.start_at, StartAt::Near(_));
                if ui.radio(placed, "Move Start Points to be closest to selected point on vector bounding box").clicked() {
                    op.start_at = StartAt::Near(0);
                }
                ui.add_enabled_ui(placed, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 70.0), Sense::hover());
                    let inner = rect.shrink(9.0);
                    ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
                    let (l, r, t, b, cx, cy) = (inner.left(), inner.right(), inner.top(), inner.bottom(), inner.center().x, inner.center().y);
                    let spots = [pos2(l, t), pos2(cx, t), pos2(r, t), pos2(r, cy), pos2(r, b), pos2(cx, b), pos2(l, b), pos2(l, cy)];
                    for (i, spot) in spots.iter().enumerate() {
                        let hit = Rect::from_center_size(*spot, vec2(14.0, 14.0));
                        let resp = ui.interact(hit, ui.scope_id().with(("start_spot", i)), if placed { Sense::click() } else { Sense::hover() });
                        ui.painter().circle(*spot, 5.0, INPUT, Stroke::new(1.0, MUTED));
                        if op.start_at == StartAt::Near(i as u8) {
                            ui.painter().circle_filled(*spot, 3.0, ACCENT);
                        }
                        label_widget(&resp, &format!("Start point {}", i + 1));
                        if resp.clicked() {
                            op.start_at = StartAt::Near(i as u8);
                        }
                    }
                });
            }
            _ => {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add_enabled_ui(op.side == Side::Outside, |ui| {
                    ui.checkbox(&mut op.sharp_corners, "Sharp external corners");
                });
                if op.side != Side::Outside {
                    ui.label(RichText::new("Only for cuts on the outside of a shape.").size(11.5).color(MUTED));
                } else {
                    ui.label(RichText::new("The tool goes all the way into each outside corner, instead of rounding it. Shapes inside each other stay rounded.").size(11.5).color(MUTED));
                }
            }
        });
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        Frame::new().inner_margin(Margin { left: 14, right: 14, top: 8, bottom: 6 }).show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            title_card(ui, Icon::Preview, "Preview Toolpaths");
            card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::hover());
                    wood_block(ui.painter(), r);
                    let name = self.doc.job.preview_material.map_or("Solid Material", |m| MATERIALS[m.min(4)].0);
                    egui::ComboBox::from_id_salt("preview_material").selected_text(name).width(170.0).show_ui(ui, |ui| {
                        for (i, m) in MATERIALS.iter().enumerate() {
                            ui.selectable_value(&mut self.doc.job.preview_material, Some(i), m.0);
                        }
                        ui.selectable_value(&mut self.doc.job.preview_material, None, "Solid Material");
                    });
                });
                ui.add_enabled_ui(self.doc.job.preview_material.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(38.0);
                        ui.label("Solid Material");
                        ui.add_space(40.0);
                        ui.color_edit_button_srgb(&mut self.doc.job.solid);
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    bold(ui, "Machined Area Color ....");
                });
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.radio_value(&mut self.machined, Machined::Material, "Material Color");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.machined, Machined::Global, "Global Fill Color");
                            ui.color_edit_button_srgb(&mut self.global_fill);
                        });
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.machined, Machined::Toolpath, "Toolpath Color");
                            let mut chosen = self.selected_tp.and_then(|i| self.doc.toolpaths.get(i)).map(|t| t.color).unwrap_or(self.global_fill);
                            if ui.color_edit_button_srgb(&mut chosen).changed() {
                                if let Some(tp) = self.selected_tp.and_then(|i| self.doc.toolpaths.get_mut(i)) {
                                    tp.color = chosen;
                                }
                            }
                            if ui.button("Set All").on_hover_text("Give every toolpath the global fill color").clicked() {
                                for t in &mut self.doc.toolpaths {
                                    t.color = self.global_fill;
                                }
                            }
                        });
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.checkbox(&mut self.animate, "Animate preview");
                    ui.checkbox(&mut self.draw_tool, "Draw tool");
                });
            });
            card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                let w = ui.available_width() - 40.0;
                let row = |ui: &mut Ui, icon: Icon, add: &mut dyn FnMut(&mut Ui)| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(32.0, 22.0), Sense::hover());
                        icons::paint(ui.painter(), r, icon, true);
                        add(ui);
                    });
                };
                let has = self.selected_tp.is_some();
                let mut do_selected = false;
                row(ui, Icon::TpProfile, &mut |ui| {
                    do_selected = ui.add_enabled(has, egui::Button::new("Preview Selected Toolpath").min_size(vec2(w, 20.0))).clicked();
                });
                if do_selected {
                    if let Some(i) = self.selected_tp {
                        self.preview(vec![i]);
                    }
                }
                let running = !self.anim.queue.is_empty();
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (icon, tip) in [(Icon::Play, "Play"), (Icon::Pause, "Pause"), (Icon::Step, "Step"), (Icon::End, "Go to end"), (Icon::Rewind, "Back to start")] {
                        let (rect, resp) = ui.allocate_exact_size(vec2(35.0, 20.0), if running { Sense::click() } else { Sense::hover() });
                        ui.painter().rect_filled(rect, 4.0, theme::BUTTON);
                        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, running);
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
                });
                ui.horizontal(|ui| {
                    ui.add_space(52.0);
                    ui.label("Speed");
                    ui.add_space(8.0);
                    ui.spacing_mut().slider_width = 138.0;
                    ui.spacing_mut().slider_rail_height = 3.0;
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.bg_fill = LINE;
                    w.hovered.bg_fill = LINE;
                    w.active.bg_fill = LINE;
                    ui.add(egui::Slider::new(&mut self.speed, 0.0..=1.0).show_value(false).handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }));
                });
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.add_enabled(false, egui::Button::new("Preview All Sides").min_size(vec2(w, 20.0))).on_disabled_hover_text("Only single sided jobs in this version");
                });
                let all: Vec<usize> = (0..self.doc.toolpaths.len()).collect();
                let visible: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|i| self.doc.toolpaths[*i].visible).collect();
                let mut do_all = false;
                let mut do_visible = false;
                row(ui, Icon::TpPocket, &mut |ui| {
                    do_all = ui.add_enabled(!all.is_empty(), egui::Button::new("Preview All Toolpaths").min_size(vec2(w, 20.0))).clicked();
                });
                row(ui, Icon::TpDrill, &mut |ui| {
                    do_visible = ui.add_enabled(!visible.is_empty(), egui::Button::new("Preview Visible Toolpaths").min_size(vec2(w, 20.0))).clicked();
                });
                if do_all {
                    self.preview(all);
                }
                if do_visible {
                    self.preview(visible);
                }
                let has_sim = self.sim.is_some();
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 22.0), Sense::hover());
                    wood_block(ui.painter(), r);
                    if ui.add_enabled(has_sim, egui::Button::new("Reset Preview").min_size(vec2(w / 2.0 + 2.0, 20.0))).clicked() {
                        self.reset_preview();
                    }
                    if ui.add_enabled(has_sim, egui::Button::new("Undo Last").min_size(vec2(w / 2.0 - 8.0, 20.0))).clicked() {
                        self.undo_preview();
                    }
                });
                let mut do_save = false;
                row(ui, Icon::Save, &mut |ui| {
                    do_save = ui.add_sized([w, 20.0], egui::Button::new("Save Preview Image")).clicked();
                });
                if do_save {
                    self.save_preview = rfd::FileDialog::new().add_filter("Picture", &["png"]).set_file_name("preview.png").save_file();
                }
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 26.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Select, true);
                    ui.label("Double click on waste areas in 3D\nview to remove them.");
                });
            });
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 80.0).max(0.0));
                if wide(ui, "Close", 74.0).clicked() {
                    self.right = RightMode::List;
                }
            });
        });
    }

    // ----- Centre -----

    fn centre(&mut self, ui: &mut Ui) {
        let whole = ui.max_rect();
        // In narrow windows the view tools get a row of their own.
        let two_rows = whole.width() < 700.0;
        let top = Rect::from_min_size(whole.min, vec2(whole.width(), if two_rows { 60.0 } else { 32.0 }));
        let view = Rect::from_min_max(pos2(whole.left() + 5.0, top.bottom()), whole.max);
        self.top_row(ui, top, view, two_rows);
        if self.view3d {
            self.view_3d(ui, view);
            self.view_rect = view;
        } else {
            self.view_2d(ui, view);
        }
    }

    fn top_row(&mut self, ui: &mut Ui, top: Rect, view: Rect, two_rows: bool) {
        ui.painter().rect_filled(top, 0.0, CHROME);
        let tab_y = top.top() + 8.0;
        let two = Rect::from_min_size(pos2(top.left() + 12.0, tab_y), vec2(76.0, 22.0));
        let three = Rect::from_min_size(pos2(two.right() - 4.0, tab_y), vec2(66.0, 22.0));
        if side_tab(ui, two, "2D View", !self.view3d).clicked() {
            self.view3d = false;
        }
        if side_tab(ui, three, "3D View", self.view3d).clicked() {
            self.view3d = true;
        }
        let layer_at = pos2(three.right() + 42.0, top.top() + 6.0);
        ui.painter().line_segment([layer_at - vec2(5.0, -2.0), layer_at + vec2(-5.0, 22.0)], Stroke::new(1.0, LINE));
        icons::paint(ui.painter(), Rect::from_min_size(layer_at, vec2(22.0, 22.0)), Icon::Layer, true);
        let mut layer_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(layer_at + vec2(22.0, 1.0), vec2(120.0, 24.0))));
        let name = self.doc.layers.get(self.doc.active_layer).map_or(String::new(), |l| l.name.clone());
        egui::ComboBox::from_id_salt("layer").selected_text(name).width(78.0).show_ui(&mut layer_ui, |ui| {
            for (i, l) in self.doc.layers.iter().enumerate() {
                ui.selectable_value(&mut self.doc.active_layer, i, l.name.as_str());
            }
        });
        // View tools, placed as in the pictures.
        let (start, tools_y) = if two_rows {
            (top.left() + 12.0, top.top() + 33.0)
        } else {
            ((top.left() + top.width() * 0.482).max(layer_at.x + 138.0), top.top() + 4.0)
        };
        let mut x = start;
        let view3d = self.view3d;
        let items: [(Icon, &str, bool, bool, f32); 13] = [
            (Icon::Snap, "Snap to grid", true, self.snap, 27.0),
            (Icon::SnapShapes, "Snap to shapes", !view3d, self.snap_shapes, 27.0),
            (Icon::SnapObjects, "Show model outlines in 2D", true, self.show_model2d, 27.0),
            (Icon::Grid, "Show grid", true, self.grid, 34.0),
            (Icon::Pan, "Pan", !view3d, self.tool == Tool2::Pan, 27.0),
            (Icon::ZoomBox, "Zoom to box", !view3d, self.tool == Tool2::ZoomBox, 27.0),
            (Icon::ZoomFit, "Zoom to job", true, false, 27.0),
            (Icon::ZoomSel, "Zoom to selection", !view3d, false, 34.0),
            (Icon::ShowVectors, "Show shapes", true, self.show_vectors, 27.0),
            (Icon::ShowPaths, "Show toolpaths", true, self.show_toolpaths, 34.0),
            (Icon::ShowModel, "Show the 3D model", true, false, 36.0),
            (Icon::PanelLeft, "Show or hide the left panel", true, false, 27.0),
            (Icon::PanelRight, "Show or hide the right panel", true, false, 27.0),
        ];
        for (i, (icon, tip, enabled, active, step)) in items.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(x, tools_y), vec2(24.0, 24.0));
            let resp = ui.interact(rect, ui.scope_id().with(("toolbar", i)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
            if active {
                ui.painter().rect_filled(rect, 6.0, theme::ACCENT_SOFT);
            } else if enabled && resp.hovered() {
                ui.painter().rect_filled(rect, 6.0, theme::BUTTON);
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
                    Icon::ZoomFit => {
                        if self.view3d {
                            self.cam.zoom = 1.0;
                            self.cam.pan = egui::Vec2::ZERO;
                        } else {
                            self.auto_fit = true;
                            self.last_rect = Rect::NOTHING;
                        }
                    }
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
                    Icon::PanelLeft => {
                        self.left_open = !self.left_open;
                        self.auto_fit = true;
                    }
                    Icon::PanelRight => {
                        self.right_open = !self.right_open;
                        self.auto_fit = true;
                    }
                    _ => {}
                }
            }
            x += step;
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
            egui::Window::new("Machining Time").open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                let mut total = 0.0;
                let rapid = if self.doc.job.units == Units::Inches { toolpath::RAPID_RATE_MM / 25.4 } else { toolpath::RAPID_RATE_MM };
                for t in self.doc.toolpaths.iter().filter(|t| t.visible) {
                    let s = toolpath::estimate_seconds(toolpath::working_moves(&t.moves), &t.op.tool, rapid, t.op.dwell);
                    total += s;
                    ui.label(format!("{}: {}", t.op.name, duration(s)));
                }
                ui.separator();
                bold(ui, &format!("Visible toolpaths: {}", duration(total)));
                ui.label(RichText::new("An estimate: cutting and lifting count double, and fast moves go at 100 mm/s. The real time depends on your machine.").size(11.5));
            });
            self.time_open = open;
        }
        if self.about_open {
            let mut open = true;
            egui::Window::new("About Burin").open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                ui.label("Burin 0.1: design, model and cut for CNC carving.");
                ui.label("Check every machine file in a simulator or above the material before cutting.");
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

fn duration(seconds: f64) -> String {
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

pub(crate) fn wood_block(p: &egui::Painter, r: Rect) {
    let c = r.center();
    let (w, h) = (r.width() * 0.46, r.height() * 0.36);
    let top = vec![c + vec2(-w * 0.55, -h), c + vec2(w, -h), c + vec2(w * 0.55, h * 0.1), c + vec2(-w, h * 0.1)];
    let side = vec![c + vec2(-w, h * 0.1), c + vec2(w * 0.55, h * 0.1), c + vec2(w * 0.55, h), c + vec2(-w, h)];
    let end = vec![c + vec2(w * 0.55, h * 0.1), c + vec2(w, -h), c + vec2(w, h * 0.1), c + vec2(w * 0.55, h)];
    p.add(Shape::convex_polygon(side, Color32::from_rgb(196, 136, 84), Stroke::NONE));
    p.add(Shape::convex_polygon(end, Color32::from_rgb(158, 104, 62), Stroke::NONE));
    p.add(Shape::convex_polygon(top, Color32::from_rgb(226, 172, 120), Stroke::NONE));
}
