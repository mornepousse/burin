//! The left panel's tabs and forms, and the right panel's toolpath forms and tool list.

use crate::app::*;
use crate::edit::{DimKind, Edge, FilletKind};
use crate::geom::{self, BoolOp};
use crate::icons::Icon;
use crate::relief::ToolKind;
use crate::theme;
use crate::toolpath::{self, Boundary, Op, OpKind, ProfilePass, Tool, POSTS};
use crate::ui::{bold, card, primary, count_box, icon_button, icon_row, label_widget, labelled, labelled_from, number, number_from, title_card, wide, wood_block};
use egui::{vec2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui};

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::MUTED));
}

/// Picks one of the four corners or the middle of a box.
pub fn anchor_grid(ui: &mut Ui, value: &mut Datum, name: &str) {
    let (square, _) = ui.allocate_exact_size(vec2(58.0, 54.0), Sense::hover());
    let inner = square.shrink(7.0);
    ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    for (spot, datum) in [
        (inner.left_top(), Datum::TopLeft),
        (inner.right_top(), Datum::TopRight),
        (inner.center(), Datum::Centre),
        (inner.left_bottom(), Datum::BottomLeft),
        (inner.right_bottom(), Datum::BottomRight),
    ] {
        let hit = Rect::from_center_size(spot, vec2(14.0, 14.0));
        let resp = ui.interact(hit, ui.make_persistent_id((name, format!("{datum:?}"))), Sense::click());
        ui.painter().circle(spot, 5.5, theme::INPUT, Stroke::new(1.0, theme::MUTED));
        if *value == datum {
            ui.painter().circle_filled(spot, 3.0, theme::ACCENT);
        }
        label_widget(&resp, &format!("{name} {datum:?}"));
        if resp.clicked() {
            *value = datum;
        }
    }
}

fn row_with(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        add(ui);
    });
}

fn tool_card(ui: &mut Ui, tool: &Tool) -> (bool, bool) {
    let mut result = (false, false);
    ui.horizontal(|ui| {
        bold(ui, "Tool:");
        ui.add(egui::Label::new(tool.label()).truncate());
    });
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - 136.0).max(0.0));
        result.0 = wide(ui, "Select ...", 64.0).clicked();
        result.1 = wide(ui, "Edit ...", 64.0).clicked();
    });
    result
}

impl App {
    pub fn start_xform(&mut self, kind: Xform) {
        let mut form = XformForm::new(kind, &self.doc.job);
        if let Some((lo, hi)) = self.selected_bbox() {
            let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            match kind {
                Xform::Move => (form.dx, form.dy) = (middle[0], middle[1]),
                Xform::Rotate => form.pivot = middle,
                Xform::CircularCopy => (form.dx, form.dy) = (self.doc.job.width / 2.0, self.doc.job.height / 2.0),
                Xform::BlockCopy => {
                    form.dx = (hi[0] - lo[0]) * 0.25;
                    form.dy = (hi[1] - lo[1]) * 0.25;
                }
                Xform::Bulk => self.prefill_bulk(&mut form),
                _ => {}
            }
        } else if kind == Xform::CircularCopy {
            (form.dx, form.dy) = (self.doc.job.width / 2.0, self.doc.job.height / 2.0);
        }
        self.tool = Tool2::Select;
        self.left = LeftForm::Xform(form);
        self.view3d = false;
    }

    /// Fills the bulk form with the size of the first selected shape of each kind.
    pub fn prefill_bulk(&self, f: &mut XformForm) {
        use crate::bulk::{classify, Class};
        for v in self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)) {
            match classify(v) {
                Some(Class::Circle { d }) if f.bulk_d == 0.0 => f.bulk_d = d,
                Some(Class::Rectangle { w, h } | Class::Other { w, h }) if f.bulk_w == 0.0 => (f.bulk_w, f.bulk_h) = (w, h),
                _ => {}
            }
        }
    }

    pub fn drawing_tab(&mut self, ui: &mut Ui) {
        let some = !self.selected.is_empty();
        let many = self.selected.len() >= 2;
        if self.section_open(ui, "Create Vectors") {
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Circle, "Draw circle", DrawKind::Circle),
                    (Icon::Ellipse, "Draw ellipse", DrawKind::Ellipse),
                    (Icon::Rect, "Draw rectangle", DrawKind::Rect),
                    (Icon::Polygon, "Draw polygon", DrawKind::Polygon),
                    (Icon::Star, "Draw star", DrawKind::Star),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_draw(kind);
                    }
                }
            });
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Lines, "Draw lines", DrawKind::Polyline),
                    (Icon::Arc, "Draw arc", DrawKind::Arc),
                    (Icon::Curve, "Draw curve", DrawKind::Curve),
                    (Icon::Spiral, "Draw spiral", DrawKind::Spiral),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_draw(kind);
                    }
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Text, "Draw text", true, false).clicked() {
                    self.start_draw(DrawKind::Text);
                }
                if icon_button(ui, Icon::TextCurve, "Text on a curve", true, false).clicked() {
                    self.start_xform(Xform::TextOnCurve);
                }
                if icon_button(ui, Icon::Measure, "Measure distance", true, self.tool == Tool2::Measure).clicked() {
                    self.tool = Tool2::Measure;
                    self.left = LeftForm::None;
                    self.view3d = false;
                }
                if icon_button(ui, Icon::Dimension, "Dimensions", true, false).clicked() {
                    self.start_xform(Xform::Dimension);
                }
            });
        }
        ui.add_space(6.0);
        self.transform_section(ui, some);
        ui.add_space(6.0);
        if self.section_open(ui, "Edit Objects") {
            self.edit_rows(ui, some, many);
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Weld, "Weld shapes", many, false).clicked() {
                    self.boolean_selected(BoolOp::Weld);
                }
                if icon_button(ui, Icon::Subtract, "Subtract shapes", many, false).clicked() {
                    self.boolean_selected(BoolOp::Subtract);
                }
                if icon_button(ui, Icon::Intersect, "Keep the overlap", many, false).clicked() {
                    self.boolean_selected(BoolOp::Intersect);
                }
                if icon_button(ui, Icon::Trim, "Trim lines", true, false).clicked() {
                    self.start_xform(Xform::Trim);
                }
                if icon_button(ui, Icon::Join, "Join vectors", true, false).clicked() {
                    self.start_xform(Xform::Join);
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Fillet, "Create fillets", true, false).clicked() {
                    self.start_xform(Xform::Fillet);
                }
                if icon_button(ui, Icon::Close, "Close open lines", some, false).clicked() {
                    self.close_selected();
                }
                if icon_button(ui, Icon::Validate, "Vector validator", true, false).clicked() {
                    self.start_xform(Xform::Validate);
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Similar, "Select similar shapes", some, false).clicked() {
                    let tol = self.default_similar_tolerance();
                    self.select_similar(tol);
                }
                if icon_button(ui, Icon::Bulk, "Modify several shapes", true, false).clicked() {
                    self.start_xform(Xform::Bulk);
                }
                if icon_button(ui, Icon::Trace, "Trace bitmap", true, false).clicked() {
                    self.start_trace();
                }
            });
        }
        ui.add_space(6.0);
        if self.section_open(ui, "Offset and Layout") {
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Offset, "Offset", some, false).clicked() {
                    self.start_xform(Xform::Offset);
                }
                self.array_buttons(ui, some);
                if icon_button(ui, Icon::Nest, "Nest parts", some, false).clicked() {
                    self.start_xform(Xform::Nest);
                }
            });
        }
        ui.add_space(6.0);
        if self.section_open(ui, "Sketch") {
            use crate::constraints::Kind;
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::RDimension, "Smart dimension", true, false).clicked() {
                    self.start_sketch_tool(None, true);
                }
                for (icon, tip, kind) in [
                    (Icon::RCoincident, "Coincident rule", Kind::Coincident),
                    (Icon::RHorizontal, "Horizontal rule", Kind::Horizontal),
                    (Icon::RVertical, "Vertical rule", Kind::Vertical),
                    (Icon::RParallel, "Parallel rule", Kind::Parallel),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_sketch_tool(Some(kind), false);
                    }
                }
            });
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::RPerpendicular, "Perpendicular rule", Kind::Perpendicular),
                    (Icon::REqual, "Equal rule", Kind::Equal),
                    (Icon::RTangent, "Tangent rule", Kind::Tangent),
                    (Icon::RConcentric, "Concentric rule", Kind::Concentric),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_sketch_tool(Some(kind), false);
                    }
                }
                let all_guides = some && self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).all(|v| v.construction);
                if icon_button(ui, Icon::RConstruction, "Normal / construction", some, all_guides).clicked() {
                    self.set_construction(!all_guides);
                }
            });
            icon_row(ui, |ui| {
                let on = matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Constraints);
                if icon_button(ui, Icon::Constraint, "Drawing constraints", true, on).clicked() {
                    self.start_constraint_form();
                }
                if !self.doc.constraints.is_empty() {
                    ui.label(RichText::new(format!("{} rules", self.doc.constraints.len())).color(theme::MUTED));
                }
            });
        }
    }

    fn transform_section(&mut self, ui: &mut Ui, some: bool) {
        if self.section_open(ui, "Transform Objects") {
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Move, "Move", Xform::Move),
                    (Icon::Scale, "Set size", Xform::Scale),
                    (Icon::Rotate, "Rotate", Xform::Rotate),
                    (Icon::Mirror, "Mirror", Xform::Mirror),
                    (Icon::Align, "Alignment tools", Xform::Align),
                    (Icon::Distort, "Distort object", Xform::Distort),
                ] {
                    if icon_button(ui, icon, tip, some, false).clicked() {
                        self.start_xform(kind);
                    }
                }
            });
        }
    }

    fn edit_rows(&mut self, ui: &mut Ui, some: bool, many: bool) {
        icon_row(ui, |ui| {
            if icon_button(ui, Icon::Select, "Select", true, self.tool == Tool2::Select).clicked() {
                self.tool = Tool2::Select;
                self.left = LeftForm::None;
            }
            if icon_button(ui, Icon::Nodes, "Edit points", true, self.tool == Tool2::Nodes).clicked() {
                self.tool = Tool2::Nodes;
                self.left = LeftForm::None;
                self.view3d = false;
            }
            if icon_button(ui, Icon::Group, "Group", many, false).clicked() {
                self.group_selected();
            }
            if icon_button(ui, Icon::Ungroup, "Ungroup", some, false).clicked() {
                self.ungroup_selected();
            }
            if icon_button(ui, Icon::Delete, "Delete", some, false).clicked() {
                self.delete_selected();
            }
        });
    }

    fn array_buttons(&mut self, ui: &mut Ui, some: bool) {
        for (icon, tip, kind) in [
            (Icon::BlockCopy, "Array copy", Xform::BlockCopy),
            (Icon::CircularCopy, "Circular copy", Xform::CircularCopy),
            (Icon::CopyAlong, "Copy along vectors", Xform::CopyAlong),
        ] {
            if icon_button(ui, icon, tip, some, false).clicked() {
                self.start_xform(kind);
            }
        }
    }

    pub fn open_shape_form(&mut self, editing: Option<usize>) {
        let (profile, height, base, combine) = match editing.and_then(|i| self.doc.components.get(i)) {
            Some(c) => (c.profile, c.height, c.base, c.combine),
            None => (crate::relief::Profile::Round, self.doc.job.thickness * 0.5, 0.0, crate::relief::Combine::Add),
        };
        self.left_tab = LeftTab::Modeling;
        self.left = LeftForm::Shape(ShapeForm { profile, height, base, combine, editing });
    }

    pub fn modeling_tab(&mut self, ui: &mut Ui) {
        let some = !self.selected.is_empty();
        let many = self.selected.len() >= 2;
        let comp = self.selected_comp.is_some();
        if self.section_open(ui, "Modeling Tools") {
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Dome, "Create shape from selection", true, false).clicked() {
                    self.open_shape_form(None);
                }
                if icon_button(ui, Icon::Edit, "Edit shape", comp, false).clicked() {
                    self.open_shape_form(self.selected_comp);
                }
                if icon_button(ui, Icon::Duplicate, "Copy shape", comp, false).clicked() {
                    if let Some(i) = self.selected_comp {
                        self.checkpoint();
                        let mut c = self.doc.components[i].clone();
                        c.name += " copy";
                        self.doc.components.insert(i + 1, c);
                        self.selected_comp = Some(i + 1);
                        self.model_dirty = true;
                        self.sim = None;
                    }
                }
                if icon_button(ui, Icon::ReliefPicture, "3D relief from picture", true, false).clicked() {
                    self.dialog_relief_picture();
                }
                if icon_button(ui, Icon::ImportModel, "Import 3D model", true, false).clicked() {
                    self.dialog_import_model();
                }
                if icon_button(ui, Icon::Delete, "Delete shape", comp, false).clicked() {
                    if let Some(i) = self.selected_comp {
                        self.checkpoint();
                        self.doc.components.remove(i);
                        self.selected_comp = None;
                        self.model_dirty = true;
                        self.sim = None;
                    }
                }
            });
        }
        ui.add_space(6.0);
        self.transform_section(ui, some);
        ui.add_space(6.0);
        if self.section_open(ui, "Edit Objects") {
            self.edit_rows(ui, some, many);
        }
        ui.add_space(6.0);
        if self.section_open(ui, "Array Objects") {
            icon_row(ui, |ui| self.array_buttons(ui, some));
        }
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            let mut all = !self.doc.components.is_empty() && self.doc.components.iter().all(|c| c.visible);
            if ui.checkbox(&mut all, "").on_hover_text("Show or hide all components").changed() {
                for c in &mut self.doc.components {
                    c.visible = all;
                }
                self.model_dirty = true;
                self.sim = None;
            }
            bold(ui, "Components");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Down, "Move component down", comp, false).clicked() {
                    self.move_component(1);
                }
                if icon_button(ui, Icon::Up, "Move component up", comp, false).clicked() {
                    self.move_component(-1);
                }
            });
        });
        ui.separator();
        if self.doc.components.is_empty() {
            note(ui, "Draw a closed shape, select it, then use the first tool above to make it 3D.");
        }
        let mut toggled = false;
        for (i, c) in self.doc.components.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                toggled |= ui.checkbox(&mut c.visible, "").changed();
                if ui.selectable_label(self.selected_comp == Some(i), c.name.as_str()).clicked() {
                    self.selected_comp = Some(i);
                }
            });
        }
        if toggled {
            self.model_dirty = true;
            self.sim = None;
        }
    }

    pub fn move_component(&mut self, by: i32) {
        let Some(i) = self.selected_comp else { return };
        let j = i as i32 + by;
        if j < 0 || j as usize >= self.doc.components.len() {
            return;
        }
        self.checkpoint();
        self.doc.components.swap(i, j as usize);
        self.selected_comp = Some(j as usize);
        self.model_dirty = true;
        self.sim = None;
    }

    fn insert_paths_centred(&mut self, mut paths: Vec<geom::Path>, scale_to: Option<f64>) {
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let Some((lo, hi)) = geom::bbox(&paths) else { return };
        let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let k = scale_to.map_or(1.0, |size| size / (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9));
        for p in paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
            *p = [w / 2.0 + (p[0] - middle[0]) * k, h / 2.0 + (p[1] - middle[1]) * k];
        }
        self.checkpoint();
        let id = self.doc.add_vector(paths, Prim::Free);
        self.selected = vec![id];
        self.tool = Tool2::Select;
        self.view3d = false;
    }

    fn reload_clip_folder(&mut self) {
        // Folder 0 is the built-in set.
        let folder = self.clip_folder.checked_sub(1).and_then(|i| self.clip_folders.get(i)).cloned();
        if folder == self.clip_loaded {
            return;
        }
        self.clip_files.clear();
        if let Some(dir) = &folder {
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut files: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dxf")))
                    .collect();
                files.sort();
                for f in files {
                    let paths = std::fs::read(&f).map(|b| geom::dxf_read(&String::from_utf8_lossy(&b))).unwrap_or_default();
                    if !paths.is_empty() {
                        self.clip_files.push((f.file_stem().unwrap_or_default().to_string_lossy().to_string(), paths));
                    }
                }
            }
        }
        self.clip_loaded = folder;
    }

    fn clip_tile(&mut self, ui: &mut Ui, name: &str, paths: &[geom::Path]) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(66.0, 66.0), Sense::click());
        let line = if resp.hovered() { theme::ACCENT } else { theme::LINE };
        ui.painter().rect(rect, 5.0, theme::INPUT, Stroke::new(1.0, line), StrokeKind::Inside);
        if let Some((lo, hi)) = geom::bbox(paths) {
            let k = 50.0 / ((hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9)) as f32;
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            for p in paths {
                let mut pts: Vec<egui::Pos2> = p.pts.iter().map(|q| rect.center() + vec2((q[0] - c[0]) as f32, -(q[1] - c[1]) as f32) * k).collect();
                if p.closed && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                ui.painter().add(egui::Shape::line(pts, Stroke::new(1.2, theme::TEXT)));
            }
        }
        label_widget(&resp, name);
        resp.on_hover_text(name).clicked()
    }

    pub fn clipart_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.clip_tab, 0, "Library Browser");
            ui.selectable_value(&mut self.clip_tab, 1, "Local Files");
        });
        ui.separator();
        if self.clip_tab == 1 {
            self.local_files(ui);
            return;
        }
        ui.horizontal(|ui| {
            bold(ui, "Library Folders");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_down = self.clip_folder >= 1 && self.clip_folder < self.clip_folders.len();
                if icon_button(ui, Icon::Down, "Move folder down", can_down, false).clicked() {
                    self.clip_folders.swap(self.clip_folder - 1, self.clip_folder);
                    self.clip_folder += 1;
                    save_clip_folders(&self.clip_folders);
                }
                let can_up = self.clip_folder >= 2;
                if icon_button(ui, Icon::Up, "Move folder up", can_up, false).clicked() {
                    self.clip_folders.swap(self.clip_folder - 1, self.clip_folder - 2);
                    self.clip_folder -= 1;
                    save_clip_folders(&self.clip_folders);
                }
            });
        });
        if ui.selectable_label(self.clip_folder == 0, "Sample Shapes").clicked() {
            self.clip_folder = 0;
        }
        let names: Vec<String> = self.clip_folders.iter().map(|p| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().to_string())).collect();
        for (i, n) in names.iter().enumerate() {
            if ui.selectable_label(self.clip_folder == i + 1, n).clicked() {
                self.clip_folder = i + 1;
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(self.clip_folder > 0, egui::Button::new("Remove Folder")).clicked() {
                self.clip_folders.remove(self.clip_folder - 1);
                self.clip_folder = 0;
                save_clip_folders(&self.clip_folders);
            }
            if ui.button("Add Folder ...").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.clip_folders.push(dir);
                    self.clip_folder = self.clip_folders.len();
                    save_clip_folders(&self.clip_folders);
                }
            }
        });
        ui.separator();
        self.reload_clip_folder();
        if self.clip_folder == 0 {
            for row in CLIPART.chunks(3) {
                ui.horizontal(|ui| {
                    for name in row {
                        let shape = clipart_shape(name);
                        if self.clip_tile(ui, name, &shape) {
                            let size = self.doc.job.width.min(self.doc.job.height) * 0.3;
                            self.insert_paths_centred(shape, Some(size));
                        }
                    }
                });
            }
        } else if self.clip_files.is_empty() {
            note(ui, "No DXF files in this folder.");
        } else {
            let files = self.clip_files.clone();
            for row in files.chunks(3) {
                ui.horizontal(|ui| {
                    for (name, paths) in row {
                        if self.clip_tile(ui, name, paths) {
                            self.insert_paths_centred(paths.clone(), None);
                        }
                    }
                });
            }
        }
    }

    fn local_files(&mut self, ui: &mut Ui) {
        bold(ui, "Files on this computer");
        note(ui, "Bring in a drawing from a DXF file. It lands in the middle of the job.");
        ui.add_space(4.0);
        if wide(ui, "Import a File ...", 130.0).clicked() {
            self.dialog_import();
        }
    }

    pub fn layers_tab(&mut self, ui: &mut Ui) {
        let active = self.doc.active_layer;
        let count = self.doc.layers.len();
        ui.horizontal(|ui| {
            bold(ui, "Layers List");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Down, "Move layer down", active + 1 < count, false).clicked() {
                    self.swap_layers(active, active + 1);
                }
                if icon_button(ui, Icon::Up, "Move layer up", active > 0, false).clicked() {
                    self.swap_layers(active, active - 1);
                }
            });
        });
        ui.separator();
        let mut remove = None;
        for i in 0..count {
            ui.horizontal(|ui| {
                let on = self.doc.layers[i].visible;
                let tip = format!("Show or hide {}", self.doc.layers[i].name);
                if icon_button(ui, if on { Icon::Eye } else { Icon::EyeOff }, &tip, true, false).clicked() {
                    self.doc.layers[i].visible = !on;
                }
                ui.color_edit_button_srgb(&mut self.doc.layers[i].color).on_hover_text("Line colour");
                let locked = self.doc.layers[i].locked;
                let tip = format!("Lock {}", self.doc.layers[i].name);
                if icon_button(ui, if locked { Icon::Locked } else { Icon::Unlocked }, &tip, true, locked).clicked() {
                    self.doc.layers[i].locked = !locked;
                    if !locked {
                        let in_layer: Vec<u64> = self.doc.vectors.iter().filter(|v| v.layer == i).map(|v| v.id).collect();
                        self.selected.retain(|s| !in_layer.contains(s));
                    }
                }
                ui.radio_value(&mut self.doc.active_layer, i, "").on_hover_text("Draw on this layer");
                ui.add(TextEdit::singleline(&mut self.doc.layers[i].name).desired_width(62.0));
                if count > 1 && icon_button(ui, Icon::Delete, &format!("Delete layer {}", i + 1), true, false).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.checkpoint();
            self.doc.layers.remove(i);
            for v in &mut self.doc.vectors {
                if v.layer == i {
                    v.layer = 0;
                } else if v.layer > i {
                    v.layer -= 1;
                }
            }
            self.doc.active_layer = self.doc.active_layer.min(self.doc.layers.len() - 1);
        }
        ui.add_space(6.0);
        if wide(ui, "Add New Layer ...", ui.available_width() - 16.0).clicked() {
            self.checkpoint();
            let n = self.doc.layers.len() + 1;
            self.doc.layers.push(Layer::named(&format!("Layer {n}")));
            self.doc.active_layer = n - 1;
        }
        let some = !self.selected.is_empty();
        if ui.add_enabled(some, egui::Button::new("Move Selection to Active Layer")).clicked() {
            self.checkpoint();
            let layer = self.doc.active_layer;
            for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
                v.layer = layer;
            }
        }
    }

    fn swap_layers(&mut self, a: usize, b: usize) {
        self.checkpoint();
        self.doc.layers.swap(a, b);
        for v in &mut self.doc.vectors {
            if v.layer == a {
                v.layer = b;
            } else if v.layer == b {
                v.layer = a;
            }
        }
        self.doc.active_layer = b;
    }

    /// The form of the Lines and Curve tools.
    pub fn line_form(&mut self, ui: &mut Ui, mut f: DrawForm) {
        let icon = if f.kind == DrawKind::Curve { Icon::Curve } else { Icon::Lines };
        title_card(ui, icon, if f.kind == DrawKind::Curve { "Create Curve" } else { "Create Line / Polyline" });
        let last = f.points.last().copied();
        card(ui, |ui| {
            bold(ui, "Next Point");
            let zero = self.doc.job.origin();
            let mut changed_abs = false;
            let mut changed_delta = false;
            let mut changed_polar = false;
            egui::Grid::new("next_point").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("X");
                changed_abs |= number_from(ui, &mut f.next[0], zero[0], 54.0);
                ui.label("Y");
                changed_abs |= number_from(ui, &mut f.next[1], zero[1], 54.0);
                ui.end_row();
                ui.label("Angle");
                changed_polar |= number(ui, &mut f.step[0], 54.0);
                ui.label("L");
                changed_polar |= number(ui, &mut f.step[1], 54.0);
                ui.end_row();
                ui.label("dx");
                changed_delta |= number(ui, &mut f.delta[0], 54.0);
                ui.label("dy");
                changed_delta |= number(ui, &mut f.delta[1], 54.0);
                ui.end_row();
            });
            let from = last.unwrap_or([0.0, 0.0]);
            if changed_polar {
                let a = f.step[0].to_radians();
                f.delta = [f.step[1] * a.cos(), f.step[1] * a.sin()];
                changed_delta = true;
            }
            if changed_delta {
                f.next = [from[0] + f.delta[0], from[1] + f.delta[1]];
            }
            if changed_abs {
                f.delta = [f.next[0] - from[0], f.next[1] - from[1]];
            }
            if changed_abs || changed_delta {
                f.step = [f.delta[1].atan2(f.delta[0]).to_degrees(), f.delta[0].hypot(f.delta[1])];
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if wide(ui, "Add", 56.0).clicked() {
                    f.points.push(f.next);
                }
                if ui.add_enabled(!f.points.is_empty(), egui::Button::new("Undo").min_size(vec2(56.0, 20.0))).clicked() {
                    f.points.pop();
                }
                if ui.add_enabled(f.points.len() >= 2, egui::Button::new("Finish").min_size(vec2(56.0, 20.0))).clicked() {
                    self.left = LeftForm::Draw(f.clone());
                    self.finish_line(false);
                    f.points.clear();
                }
            });
            ui.separator();
            bold(ui, "Cursor Position");
            if let Some(c) = self.cursor {
                let from = f.points.last().copied().unwrap_or([0.0, 0.0]);
                let (dx, dy) = (c[0] - from[0], c[1] - from[1]);
                let zero = self.doc.job.origin();
                ui.label(format!("X: {:.4}   Y: {:.4}", c[0] - zero[0], c[1] - zero[1]));
                ui.label(format!("Angle: {:.2}   L: {:.4}", dy.atan2(dx).to_degrees(), dx.hypot(dy)));
                ui.label(format!("dx: {dx:.4}   dy: {dy:.4}"));
            } else {
                note(ui, "Move over the sheet to see it here.");
            }
            ui.separator();
            note(ui, "Click on the sheet to add points. Press the Space bar to finish this line and start a new one. Right click to finish and leave. Click the first point to close the shape.");
        });
        if matches!(self.left, LeftForm::Draw(_)) {
            self.left = LeftForm::Draw(f);
        }
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if wide(ui, "Close", 74.0).clicked() {
                self.left = LeftForm::None;
            }
        });
    }

    fn apply_close(&mut self, ui: &mut Ui, apply: &str, enabled: bool) -> bool {
        let mut pressed = false;
        ui.horizontal(|ui| {
            pressed = ui.add_enabled(enabled, egui::Button::new(RichText::new(apply).color(theme::ON_ACCENT).strong()).fill(theme::ACCENT).min_size(vec2(74.0, 22.0))).clicked();
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if wide(ui, "Close", 74.0).clicked() {
                self.left = LeftForm::None;
            }
        });
        pressed
    }

    pub fn xform_form(&mut self, ui: &mut Ui, mut f: XformForm) {
        let some = !self.selected.is_empty();
        let unit = self.doc.job.unit();
        let (icon, title) = match f.kind {
            Xform::Move => (Icon::Move, "Move Selection"),
            Xform::Scale => (Icon::Scale, "Set Size"),
            Xform::Rotate => (Icon::Rotate, "Rotate"),
            Xform::Mirror => (Icon::Mirror, "Mirror"),
            Xform::Align => (Icon::Align, "Alignment Tools"),
            Xform::Offset => (Icon::Offset, "Offset"),
            Xform::BlockCopy => (Icon::BlockCopy, "Array Copy"),
            Xform::CircularCopy => (Icon::CircularCopy, "Circular Copy"),
            Xform::CopyAlong => (Icon::CopyAlong, "Copy Along Vectors"),
            Xform::Join => (Icon::Join, "Join Vectors"),
            Xform::Fillet => (Icon::Fillet, "Create Fillets"),
            Xform::Trim => (Icon::Trim, "Interactive Vector Trim"),
            Xform::Validate => (Icon::Validate, "Vector Validator"),
            Xform::Dimension => (Icon::Dimension, "Dimensions"),
            Xform::Nest => (Icon::Nest, "Nest Parts"),
            Xform::TextOnCurve => (Icon::TextCurve, "Text On A Curve"),
            Xform::Bulk => (Icon::Bulk, "Modify Several Shapes"),
            Xform::Constraints => (Icon::Constraint, "Dimensions and Rules"),
            Xform::Distort => (Icon::Distort, "Distort Object"),
        };
        title_card(ui, icon, title);
        let mut apply_label = Some("Apply");
        match f.kind {
            Xform::Move => {
                self.zero_card(ui, f.anchor, unit);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        bold(ui, "Anchor");
                        ui.add_space(30.0);
                        anchor_grid(ui, &mut f.anchor, "Anchor");
                    });
                    bold(ui, "Type of Move");
                    ui.horizontal(|ui| {
                        let before = f.absolute;
                        ui.radio_value(&mut f.absolute, true, "Absolute");
                        ui.radio_value(&mut f.absolute, false, "Relative");
                        if before != f.absolute {
                            if f.absolute {
                                if let Some((lo, hi)) = self.selected_bbox() {
                                    let at = crate::edit::anchor_point(lo, hi, f.anchor);
                                    (f.dx, f.dy) = (at[0], at[1]);
                                }
                            } else {
                                (f.dx, f.dy) = (0.0, 0.0);
                            }
                        }
                    });
                    // An absolute position counts from the job's origin; a relative one is only a distance.
                    let zero = if f.absolute { self.doc.job.origin() } else { [0.0, 0.0] };
                    labelled_from(ui, "X Position", 66.0, &mut f.dx, zero[0], 60.0, unit);
                    labelled_from(ui, "Y Position", 66.0, &mut f.dy, zero[1], 60.0, unit);
                });
            }
            Xform::Scale => {
                let size = self.selected_bbox().map_or([0.0, 0.0], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1]]);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        bold(ui, "Anchor");
                        ui.add_space(30.0);
                        anchor_grid(ui, &mut f.anchor, "Anchor");
                    });
                    let mut w = size[0] * f.sx / 100.0;
                    if labelled(ui, "Width (X)", 62.0, &mut w, 60.0, unit) && size[0] > 0.0 {
                        f.sx = w / size[0] * 100.0;
                    }
                    labelled(ui, "", 62.0, &mut f.sx, 60.0, "%");
                    ui.checkbox(&mut f.keep_ratio, "Link XY");
                    ui.checkbox(&mut f.each, "Each shape on its own");
                    if f.keep_ratio {
                        f.sy = f.sx;
                    }
                    let mut h = size[1] * f.sy / 100.0;
                    if labelled(ui, "Height", 62.0, &mut h, 60.0, unit) && size[1] > 0.0 {
                        f.sy = h / size[1] * 100.0;
                        if f.keep_ratio {
                            f.sx = f.sy;
                        }
                    }
                    labelled(ui, "", 62.0, &mut f.sy, 60.0, "%");
                });
            }
            Xform::Rotate => {
                card(ui, |ui| {
                    bold(ui, "Pivot Point");
                    ui.horizontal(|ui| {
                        let before = f.anchor;
                        ui.vertical(|ui| anchor_grid(ui, &mut f.anchor, "Pivot"));
                        if f.anchor != before {
                            if let Some((lo, hi)) = self.selected_bbox() {
                                f.pivot = crate::edit::anchor_point(lo, hi, f.anchor);
                            }
                        }
                        ui.vertical(|ui| {
                            let zero = self.doc.job.origin();
                            labelled_from(ui, "X:", 18.0, &mut f.pivot[0], zero[0], 62.0, "");
                            labelled_from(ui, "Y:", 18.0, &mut f.pivot[1], zero[1], 62.0, "");
                        });
                    });
                    ui.add_space(4.0);
                    labelled(ui, "Angle", 40.0, &mut f.angle, 60.0, "degrees");
                    ui.checkbox(&mut f.each, "Each shape on its own, around the anchor");
                });
            }
            Xform::Mirror => {
                apply_label = None;
                let mut go: Option<(bool, Option<Edge>, bool)> = None;
                card(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        if wide(ui, "Top", 60.0).clicked() {
                            go = Some((false, Some(Edge::Top), false));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 130.0).max(0.0) / 2.0);
                        if wide(ui, "Left", 60.0).clicked() {
                            go = Some((true, Some(Edge::Left), false));
                        }
                        if wide(ui, "Right", 60.0).clicked() {
                            go = Some((true, Some(Edge::Right), false));
                        }
                    });
                    ui.vertical_centered(|ui| {
                        if wide(ui, "Bottom", 60.0).clicked() {
                            go = Some((false, Some(Edge::Bottom), false));
                        }
                    });
                    ui.separator();
                    if wide(ui, "Flip Horizontal", 130.0).clicked() {
                        go = Some((true, None, f.about_job));
                    }
                    if wide(ui, "Flip Vertical", 130.0).clicked() {
                        go = Some((false, None, f.about_job));
                    }
                    ui.checkbox(&mut f.about_job, "Flip about job center");
                    ui.separator();
                    ui.checkbox(&mut f.copy, "Create a mirrored copy");
                });
                if let Some((h, edge, job)) = go {
                    self.mirror(h, edge, job, f.copy);
                }
            }
            Xform::Align => {
                apply_label = None;
                card(ui, |ui| {
                    bold(ui, "Align to Material");
                    ui.horizontal(|ui| {
                        if ui.button("Center X").clicked() {
                            self.align_selected(Some(0), None);
                        }
                        if ui.button("Center").clicked() {
                            self.align_selected(Some(0), Some(0));
                        }
                        if ui.button("Center Y").clicked() {
                            self.align_selected(None, Some(0));
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (label, x, y) in [("Left", Some(-1), None), ("Right", Some(1), None), ("Top", None, Some(1)), ("Bottom", None, Some(-1))] {
                            if ui.button(label).clicked() {
                                self.align_selected(x, y);
                            }
                        }
                    });
                });
                card(ui, |ui| {
                    bold(ui, "Align to Selection");
                    note(ui, "Aligns selected objects to the LAST item in the selection.");
                    ui.label("Center");
                    ui.horizontal(|ui| {
                        if ui.button("X").clicked() {
                            self.align_to_last_center(true, false);
                        }
                        if ui.button("Both").clicked() {
                            self.align_to_last_center(true, true);
                        }
                        if ui.button("Y").clicked() {
                            self.align_to_last_center(false, true);
                        }
                    });
                    for (label, inside) in [("Inside Edge", true), ("Outside Edge", false)] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            for (name, edge) in [("Left", Edge::Left), ("Right", Edge::Right), ("Top", Edge::Top), ("Bottom", Edge::Bottom)] {
                                let response = ui.button(name);
                                label_widget(&response, &format!("{label} {name}"));
                                if response.clicked() {
                                    self.align_to_last_edge(edge, inside);
                                }
                            }
                        });
                    }
                });
                card(ui, |ui| {
                    bold(ui, "Space Selection");
                    ui.horizontal(|ui| {
                        if ui.button("Across").clicked() {
                            self.space_selection(true);
                        }
                        if ui.button("Up and Down").clicked() {
                            self.space_selection(false);
                        }
                    });
                });
            }
            Xform::Offset => {
                card(ui, |ui| {
                    labelled(ui, "Distance:", 56.0, &mut f.distance, 56.0, unit);
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.outward, true, "Outward");
                        ui.radio_value(&mut f.outward, false, "Inward");
                    });
                });
            }
            Xform::BlockCopy => {
                apply_label = Some("Copy");
                let size = self.selected_bbox().map_or([0.0, 0.0], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1]]);
                card(ui, |ui| {
                    bold(ui, "Selected Objects Size");
                    ui.label(format!("X: {:.4}   Y: {:.4}", size[0], size[1]));
                    ui.add_space(4.0);
                    row_with(ui, "Rows (Y)", |ui| {
                        count_box(ui, &mut f.rows, 54.0);
                    });
                    row_with(ui, "Columns", |ui| {
                        count_box(ui, &mut f.cols, 54.0);
                    });
                    bold(ui, "Spacing ...");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.use_offset, false, "Gap");
                        ui.radio_value(&mut f.use_offset, true, "Offset");
                    });
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.dx, 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.dy, 54.0, "");
                    });
                    bold(ui, "Row/Column Displacement ...");
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.shift[0], 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.shift[1], 54.0, "");
                    });
                    ui.checkbox(&mut f.group, "Group Copies");
                });
            }
            Xform::CircularCopy => {
                apply_label = Some("Copy");
                card(ui, |ui| {
                    bold(ui, "Rotation Center");
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.dx, 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.dy, 54.0, "");
                    });
                    ui.checkbox(&mut f.rotate_copies, "Rotate Copies");
                    row_with(ui, "Number of items", |ui| {
                        count_box(ui, &mut f.count, 54.0);
                    });
                    bold(ui, "Angle ...");
                    ui.horizontal(|ui| {
                        let before = f.total_angle;
                        ui.radio_value(&mut f.total_angle, true, "Total Angle");
                        ui.radio_value(&mut f.total_angle, false, "Step Angle");
                        if before != f.total_angle {
                            let n = f.count.max(1) as f64;
                            f.angle = if f.total_angle { f.angle * n } else { f.angle / n };
                        }
                    });
                    labelled(ui, "", 2.0, &mut f.angle, 60.0, "degrees");
                    ui.checkbox(&mut f.group, "Group Copies");
                });
            }
            Xform::CopyAlong => {
                apply_label = Some("Copy");
                card(ui, |ui| {
                    ui.radio_value(&mut f.copy_circles, false, "Copy object");
                    note(ui, "Select the object to copy, then one vector to copy it along.");
                    ui.radio_value(&mut f.copy_circles, true, "Copy Circles");
                    ui.add_enabled_ui(f.copy_circles, |ui| {
                        labelled(ui, "Diameter", 60.0, &mut f.size, 54.0, unit);
                    });
                    bold(ui, "Specify ...");
                    ui.radio_value(&mut f.by_number, false, "Distance between copies");
                    ui.add_enabled_ui(!f.by_number, |ui| {
                        labelled(ui, "", 14.0, &mut f.distance, 54.0, unit);
                        ui.checkbox(&mut f.even, "Force even spacing");
                    });
                    ui.radio_value(&mut f.by_number, true, "Number of copies");
                    ui.add_enabled_ui(f.by_number, |ui| {
                        count_box(ui, &mut f.count, 54.0);
                    });
                    ui.separator();
                    ui.checkbox(&mut f.align, "Align objects to curve");
                    ui.checkbox(&mut f.new_layer, "Create copies on new layer");
                    ui.checkbox(&mut f.reverse, "Reverse direction");
                });
            }
            Xform::Join => {
                apply_label = Some("Join");
                let (before, after) = self.join_counts(f.distance);
                card(ui, |ui| {
                    bold(ui, "Selected vectors:");
                    ui.label(format!("Closed: {}", before[0]));
                    ui.label(format!("Open: {}", before[1]));
                    ui.add_space(4.0);
                    bold(ui, "Tolerance:");
                    labelled(ui, "", 2.0, &mut f.distance, 60.0, unit);
                    ui.add_space(4.0);
                    bold(ui, "Vectors after");
                    ui.label(format!("Closed: {}", after[0]));
                    ui.label(format!("Open: {}", after[1]));
                });
            }
            Xform::Fillet => {
                apply_label = None;
                card(ui, |ui| {
                    labelled(ui, "Fillet / Tool", 62.0, &mut f.distance, 54.0, unit);
                    note(ui, "Click on an angled corner to change it into a circular fillet. Undo removes a fillet.");
                    bold(ui, "Fillet Type");
                    ui.radio_value(&mut f.fillet, FilletKind::Normal, "Normal fillet");
                    ui.radio_value(&mut f.fillet, FilletKind::DogBone, "'Dog-Bone' Fillet");
                    note(ui, "Clears inside corners so slotted pieces fit together.");
                    ui.radio_value(&mut f.fillet, FilletKind::TBone, "'T-Bone' Fillet");
                    note(ui, "The same clearance, for when the slot is the same size as the tool.");
                });
            }
            Xform::Trim => {
                apply_label = None;
                card(ui, |ui| {
                    ui.label("To trim overlapping vectors click on sections of vectors you want to delete, or hold the button and sweep across them.");
                    ui.add_space(4.0);
                    ui.label("To delete closed non-overlapping vectors hold down the Shift key when clicking.");
                    ui.add_space(4.0);
                    ui.checkbox(&mut f.rejoin, "Rejoin trimmed sections automatically when form is closed.");
                    if !self.doc.constraints.is_empty() {
                        ui.add_space(4.0);
                        note(ui, "Rules stay on the edges they were on. A dimension on a shortened line takes its new length, and rules on a removed part are removed.");
                    }
                });
            }
            Xform::Validate => {
                apply_label = None;
                let zero = self.issues.iter().filter(|i| i.kind == crate::edit::IssueKind::ZeroLength).count();
                card(ui, |ui| {
                    ui.label("If there is no selection, all visible vectors on visible layers will be validated.");
                    ui.add_space(4.0);
                    ui.label("The following issues will be marked:");
                    for (name, colour, kind) in [
                        ("Zero-length", theme::RAPID, crate::edit::IssueKind::ZeroLength),
                        ("Overlaps", theme::ACCENT, crate::edit::IssueKind::Overlap),
                        ("Intersections", theme::CUT, crate::edit::IssueKind::Intersection),
                    ] {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            ui.painter().rect_filled(r.shrink(2.0), 2.0, colour);
                            let n = self.issues.iter().filter(|i| i.kind == kind).count();
                            ui.label(format!("{name}: {n}"));
                        });
                    }
                    ui.add_space(4.0);
                    if wide(ui, "Search All", ui.available_width() - 2.0).clicked() {
                        self.validate();
                    }
                });
                if ui.add_enabled(zero > 0, egui::Button::new("Fix Zero-length Spans")).clicked() {
                    self.fix_zero_length();
                }
            }
            Xform::Dimension => {
                apply_label = None;
                card(ui, |ui| {
                    ui.radio_value(&mut f.dim, DimKind::Length, "Length Dimension");
                    ui.radio_value(&mut f.dim, DimKind::Vertical, "Vertical Dimension");
                    ui.radio_value(&mut f.dim, DimKind::Horizontal, "Horizontal Dimension");
                });
                card(ui, |ui| {
                    labelled(ui, "Text", 46.0, &mut f.size, 54.0, unit);
                    row_with(ui, "Decimal", |ui| {
                        count_box(ui, &mut f.decimals, 54.0);
                    });
                    labelled(ui, "Offset:", 46.0, &mut f.distance, 54.0, unit);
                    row_with(ui, "Custom Text:", |ui| {
                        ui.add(TextEdit::singleline(&mut f.text).desired_width(100.0));
                    });
                });
                card(ui, |ui| {
                    ui.label("Drag on the sheet from the first point to the second.");
                    note(ui, "Dimensions go on a layer named Dimensions.");
                });
            }
            Xform::Nest => {
                apply_label = Some("Nest");
                card(ui, |ui| {
                    bold(ui, "Tool and Clearance Settings");
                    labelled(ui, "Tool Dia. (D)", 74.0, &mut f.tool_dia, 54.0, unit);
                    labelled(ui, "Clearance", 74.0, &mut f.clearance, 54.0, unit);
                    labelled(ui, "Border Gap", 74.0, &mut f.border, 54.0, unit);
                    ui.separator();
                    bold(ui, "Sheet Options");
                    ui.horizontal(|ui| {
                        ui.label("Nest from");
                        ui.add_space(14.0);
                        anchor_grid(ui, &mut f.anchor, "Nest from");
                    });
                    ui.label("Nest Direction");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.along_x, true, "Along X");
                        ui.radio_value(&mut f.along_x, false, "Along Y");
                    });
                    note(ui, "Parts are packed in rows by their outer box.");
                });
            }
            Xform::Bulk => {
                apply_label = None;
                self.bulk_form(ui, &mut f, unit);
            }
            Xform::Constraints => {
                apply_label = None;
                self.constraint_form(ui, &mut f);
            }
            Xform::Distort => {
                apply_label = None;
                use crate::distort::EnvelopeKind;
                card(ui, |ui| {
                    bold(ui, "Create Envelope");
                    ui.label("Select the object to distort and (optionally) one or two open vectors to define the bottom and top of the initial distortion envelope.");
                    ui.add_space(4.0);
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::BoundingBox, "Bounding Box");
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::OneCurve, "Above a Single Curve");
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::TwoCurves, "Between Two Curves");
                    ui.add_space(4.0);
                    let check = self.envelope_check(f.envelope_kind);
                    match &check {
                        Ok(()) => {
                            ui.colored_label(egui::Color32::from_rgb(120, 214, 140), "Selected vectors are valid.");
                        }
                        Err(why) => {
                            ui.colored_label(theme::ERROR, why);
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 76.0).max(0.0));
                        let active = self.envelope.is_some();
                        if ui.add_enabled(check.is_ok() && !active, egui::Button::new("Apply").min_size(vec2(74.0, 20.0))).clicked() {
                            self.apply_envelope(f.envelope_kind);
                        }
                    });
                });
                card(ui, |ui| {
                    bold(ui, "Edit Envelope");
                    let active = self.envelope.is_some();
                    let handles = self.envelope.as_ref().is_some_and(|e| e.has_handles());
                    ui.label(if handles {
                        "Drag the four corners on the sheet to reshape the object."
                    } else if active {
                        "The object follows the curves you chose."
                    } else {
                        "Apply an envelope to edit it here."
                    });
                    if ui.add_enabled(active, egui::Button::new("Bake Distortion").min_size(vec2(ui.available_width() - 2.0, 20.0))).clicked() {
                        self.bake_envelope();
                    }
                });
            }
            Xform::TextOnCurve => {
                card(ui, |ui| {
                    note(ui, "Select a SINGLE LINE of text and the curve to wrap it onto.");
                    bold(ui, "Text Spacing");
                    ui.add(egui::Slider::new(&mut f.spacing, 0.5..=3.0).show_value(true));
                    bold(ui, "Text Position");
                    labelled(ui, "Offset Distance", 86.0, &mut f.distance, 50.0, unit);
                    note(ui, "A positive offset sits above the curve, a negative one below.");
                    bold(ui, "Text Alignment");
                    ui.radio_value(&mut f.anchor, Datum::BottomLeft, "Left");
                    ui.radio_value(&mut f.anchor, Datum::Centre, "Middle");
                    ui.radio_value(&mut f.anchor, Datum::BottomRight, "Right");
                    ui.separator();
                    ui.radio_value(&mut f.align, true, "Align to Curve");
                    ui.radio_value(&mut f.align, false, "Keep Vertical");
                });
            }
        }
        if matches!(self.left, LeftForm::Xform(_)) {
            self.left = LeftForm::Xform(f.clone());
        }
        match apply_label {
            Some(label) => {
                let enabled = some;
                if self.apply_close(ui, label, enabled) {
                    self.apply_xform();
                }
            }
            None => {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 80.0).max(0.0));
                    if wide(ui, "Close", 74.0).clicked() {
                        if f.kind == Xform::Trim && f.rejoin {
                            self.rejoin_all(f.distance.min(0.01));
                        }
                        self.left = LeftForm::None;
                        self.issues.clear();
                        self.bake_envelope();
                        self.picks.clear();
                        self.hover_elem = None;
                    }
                });
            }
        }
    }

    /// Lists the alike shapes on the sheet, and changes the selected ones together.
    fn bulk_form(&mut self, ui: &mut Ui, f: &mut XformForm, unit: &str) {
        use crate::bulk::{classify, Class};
        card(ui, |ui| {
            bold(ui, "Shapes on the sheet");
            note(ui, "Click a line to select all the shapes in it.");
            let groups = self.similar_groups(f.tolerance, f.layer_only);
            if groups.is_empty() {
                note(ui, "Nothing drawn yet.");
            }
            egui::ScrollArea::vertical().max_height(190.0).id_salt("similar_groups").show(ui, |ui| {
                for g in &groups {
                    let chosen = g.ids.iter().all(|id| self.selected.contains(id)) && g.ids.len() == self.selected.len();
                    let text = format!("{} x  {}", g.ids.len(), g.class.describe(unit));
                    if ui.selectable_label(chosen, text).clicked() {
                        self.selected = g.ids.clone();
                        f.bulk_d = 0.0;
                        f.bulk_w = 0.0;
                        f.bulk_h = 0.0;
                        self.prefill_bulk(f);
                        self.tool = Tool2::Select;
                    }
                }
            });
            ui.separator();
            labelled(ui, "Alike within", 66.0, &mut f.tolerance, 54.0, unit);
            f.tolerance = f.tolerance.max(0.0);
            ui.checkbox(&mut f.layer_only, "Only the active layer");
        });
        let selected: Vec<Class> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).filter_map(classify).collect();
        let circles = selected.iter().filter(|c| matches!(c, Class::Circle { .. })).count();
        let others = selected.len() - circles;
        card(ui, |ui| {
            bold(ui, &format!("Change the {} selected", selected.len()));
            if selected.is_empty() {
                note(ui, "Select shapes on the sheet, or pick a line above.");
                return;
            }
            if circles > 0 {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_d, "");
                    labelled(ui, "Diameter", 52.0, &mut f.bulk_d, 60.0, unit);
                });
            }
            if others > 0 {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_w, "");
                    labelled(ui, "Width", 52.0, &mut f.bulk_w, 60.0, unit);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_h, "");
                    labelled(ui, "Height", 52.0, &mut f.bulk_h, 60.0, unit);
                });
            }
            ui.horizontal(|ui| {
                ui.label("Keep in place");
                ui.add_space(8.0);
                anchor_grid(ui, &mut f.anchor, "Keep in place");
            });
            note(ui, "Each shape changes around its own point, so holes stay where they are.");
            ui.add_space(4.0);
            if wide(ui, "Apply to All Selected", ui.available_width() - 2.0).clicked() {
                let d = (circles > 0 && f.set_d).then_some(f.bulk_d);
                let w = (others > 0 && f.set_w).then_some(f.bulk_w);
                let h = (others > 0 && f.set_h).then_some(f.bulk_h);
                self.resize_each(d, w, h, f.anchor);
            }
        });
    }

    /// After trimming, connects the pieces of each shape that still touch.
    pub fn rejoin_all(&mut self, tolerance: f64) {
        self.keep_rules(|a| a.rejoin_inner(tolerance));
    }

    fn rejoin_inner(&mut self, tolerance: f64) {
        for v in &mut self.doc.vectors {
            if v.paths.iter().any(|p| !p.closed) && v.paths.len() > 1 {
                v.paths = crate::edit::join_paths(std::mem::take(&mut v.paths), tolerance);
            }
        }
    }

    // ----- Right panel -----

    /// The panel's material summary and toolpath tools.
    pub fn toolpath_ops(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 8, top: 10, bottom: 8 }).show(ui, |ui| {
            ui.set_width(width - 22.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    bold(ui, "Material");
                    ui.horizontal(|ui| {
                        if wide(ui, "Set ...", 60.0).clicked() {
                            self.right = RightMode::Material(Box::new(self.doc.clone()));
                        }
                        let (r, _) = ui.allocate_exact_size(vec2(50.0, 40.0), Sense::hover());
                        wood_block(ui.painter(), r);
                    });
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("XY").color(theme::MUTED));
                    let (r, resp) = ui.allocate_exact_size(vec2(46.0, 30.0), Sense::click());
                    wood_block(ui.painter(), r);
                    let (job, zero) = (&self.doc.job, self.doc.job.origin());
                    let (u, v) = (((zero[0] / job.width) as f32).clamp(0.0, 1.0), ((zero[1] / job.height) as f32).clamp(0.0, 1.0));
                    ui.painter().circle_filled(crate::ui::wood_top_point(r, u, v), 3.0, egui::Color32::from_rgb(220, 30, 30));
                    let text = format!("X: {}  Y: {}", toolpath::trim(zero[0]), toolpath::trim(zero[1]));
                    label_widget(&resp, "X/Y zero");
                    if resp.on_hover_text("Where the machine's X0 Y0 is. Click to change it.").clicked() {
                        self.right = RightMode::Material(Box::new(self.doc.clone()));
                    }
                    ui.label(RichText::new(text).size(11.0).color(theme::MUTED));
                });
            });
            let (safe, home) = (self.doc.machine.safe_z, self.doc.machine.home);
            note(ui, &format!("Safe Z: {}   Home: X{} Y{} Z{}", toolpath::trim(safe), toolpath::trim(home[0]), toolpath::trim(home[1]), toolpath::trim(home[2])));
            ui.add_space(6.0);
            if self.section_open(ui, "Toolpath Operations") {
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [
                        (Icon::TpProfile, "Profile toolpath", OpKind::Profile),
                        (Icon::TpPocket, "Pocket toolpath", OpKind::Pocket),
                        (Icon::TpDrill, "Drill toolpath", OpKind::Drill),
                        (Icon::TpEngrave, "Quick engrave toolpath", OpKind::Engrave),
                        (Icon::TpVCarve, "V-Carve toolpath", OpKind::VCarve),
                    ] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [
                        (Icon::TpPrism, "Prism carve toolpath", OpKind::Prism),
                        (Icon::TpTexture, "Texture toolpath", OpKind::Texture),
                        (Icon::TpMoulding, "Moulding toolpath", OpKind::Moulding),
                        (Icon::TpRough, "3D roughing toolpath", OpKind::Rough3D),
                        (Icon::TpFinish, "3D finishing toolpath", OpKind::Finish3D),
                    ] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [(Icon::TpFlute, "Fluting toolpath", OpKind::Flute), (Icon::TpChamfer, "Chamfer toolpath", OpKind::Chamfer)] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
            }
            let some = self.selected_tp.is_some();
            let any = !self.doc.toolpaths.is_empty();
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Tools, "Tool list", true, false).clicked() {
                    self.open_tool_list(None);
                }
                if icon_button(ui, Icon::Edit, "Edit toolpath", some, false).clicked() {
                    self.edit_toolpath();
                }
                if icon_button(ui, Icon::Duplicate, "Copy toolpath", some, false).clicked() {
                    self.duplicate_toolpath();
                }
                if icon_button(ui, Icon::Trash, "Delete toolpath", some, false).clicked() {
                    self.delete_toolpath();
                }
                if icon_button(ui, Icon::Clock, "Machining time", any, false).clicked() {
                    self.time_open = true;
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Preview, "Preview toolpaths", true, false).clicked() {
                    self.right = RightMode::Preview;
                    self.view3d = true;
                }
                if icon_button(ui, Icon::Recalc, "Recalculate all toolpaths", any, false).clicked() {
                    self.recalculate_all();
                }
                if icon_button(ui, Icon::SaveTp, "Save toolpaths", true, false).clicked() {
                    self.right = RightMode::Save;
                }
            });
        });
    }

    pub fn material_form(&mut self, ui: &mut Ui, backup: Box<Doc>) {
        let width = ui.available_width();
        let unit = self.doc.job.unit();
        let mut close: Option<bool> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                ui.set_width(width - 30.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                title_card(ui, Icon::TpProfile, "Material Setup");
                card(ui, |ui| {
                    labelled(ui, "Thickness", 60.0, &mut self.doc.job.thickness, 60.0, unit);
                    if labelled(ui, "Sacrificial board", 96.0, &mut self.doc.job.spoil_board, 60.0, unit) {
                        self.doc.job.spoil_board = self.doc.job.spoil_board.max(0.0);
                    }
                    note(ui, "Cutting into it brings no warning; going past it does.");
                });
                card(ui, |ui| {
                    bold(ui, "XY");
                    ui.horizontal(|ui| {
                        anchor_grid(ui, &mut self.doc.job.datum, "XY datum");
                        ui.vertical(|ui| {
                            ui.checkbox(&mut self.doc.job.use_offset, "Use Offset");
                            ui.add_enabled_ui(self.doc.job.use_offset, |ui| {
                                labelled(ui, "X:", 14.0, &mut self.doc.job.offset[0], 54.0, "");
                                labelled(ui, "Y:", 14.0, &mut self.doc.job.offset[1], 54.0, "");
                            });
                        });
                    });
                });
                card(ui, |ui| {
                    bold(ui, "Z-Zero");
                    if ui.radio(!self.doc.job.z_on_bed, "Material Surface").clicked() {
                        self.doc.job.z_on_bed = false;
                    }
                    if ui.radio(self.doc.job.z_on_bed, "Machine Bed").clicked() {
                        self.doc.job.z_on_bed = true;
                    }
                });
                card(ui, |ui| {
                    bold(ui, "Model Position in Material");
                    let mut changed = false;
                    let above = self.doc.machine.gap_below.is_none();
                    if ui.radio(above, "Gap Above Model").clicked() {
                        self.doc.machine.gap_below = None;
                        changed = true;
                    }
                    ui.add_enabled_ui(above, |ui| {
                        changed |= labelled(ui, "", 14.0, &mut self.doc.machine.gap_above, 54.0, unit);
                    });
                    if ui.radio(!above, "Gap Below Model").clicked() {
                        self.doc.machine.gap_below = Some(0.0);
                        changed = true;
                    }
                    if let Some(g) = &mut self.doc.machine.gap_below {
                        changed |= labelled(ui, "", 14.0, g, 54.0, unit);
                    }
                    let high = self.model.as_ref().map_or(0.0, |m| m.max() as f64);
                    note(ui, &format!("Model Thickness  {high:.3} {unit}"));
                    if changed {
                        self.bump_surface();
                    }
                });
                card(ui, |ui| {
                    bold(ui, "Rapid Z Gaps above Material");
                    labelled(ui, "Clearance (Z1)", 84.0, &mut self.doc.machine.safe_z, 54.0, unit);
                    labelled(ui, "Plunge (Z2)", 84.0, &mut self.doc.machine.plunge_gap, 54.0, unit);
                });
                card(ui, |ui| {
                    bold(ui, "Home / Start Position");
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut self.doc.machine.home[0], 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut self.doc.machine.home[1], 54.0, "");
                    });
                    labelled(ui, "Z Gap above Material", 118.0, &mut self.doc.machine.home[2], 54.0, "");
                });
                ui.horizontal(|ui| {
                    if primary(ui, "OK", 74.0).clicked() {
                        close = Some(true);
                    }
                    ui.add_space((ui.available_width() - 80.0).max(0.0));
                    if wide(ui, "Cancel", 74.0).clicked() {
                        close = Some(false);
                    }
                });
            });
        });
        match close {
            Some(true) => {
                self.doc.job.thickness = self.doc.job.thickness.max(0.001);
                self.right = RightMode::List;
                self.sim = None;
                self.recalculate_all();
                self.bump_surface();
            }
            Some(false) => {
                self.doc = *backup;
                self.right = RightMode::List;
                self.bump_surface();
            }
            None => {}
        }
    }

    pub fn save_form(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            title_card(ui, Icon::SaveTp, "Save Toolpaths");
            card(ui, |ui| {
                ui.checkbox(&mut self.one_file, "Output all visible toolpaths to one file");
                ui.checkbox(&mut self.output_arcs, "Write curves as arcs (G2 / G3)").on_hover_text("Smaller files and smoother curves. Try it in the air on your machine first.");
                ui.add_space(6.0);
                bold(ui, "Toolpaths to be saved ...");
                let names: Vec<&str> = self
                    .doc
                    .toolpaths
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| if self.one_file { t.visible } else { self.selected_tp == Some(*i) })
                    .map(|(_, t)| t.op.name.as_str())
                    .collect();
                if names.is_empty() {
                    ui.label(RichText::new("No Toolpaths").color(theme::MUTED));
                }
                for n in &names {
                    ui.label(*n);
                }
                ui.add_space(6.0);
                ui.separator();
                bold(ui, "Post Processor");
                let before = self.post_index;
                let total = POSTS.len() + self.loaded_posts.len();
                let names: Vec<String> = (0..total).map(|i| self.post_name(i)).collect();
                self.post_index = self.post_index.min(total - 1);
                egui::ComboBox::from_id_salt("post").selected_text(names[self.post_index].clone()).width(ui.available_width() - 8.0).truncate().show_ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        if i == POSTS.len() {
                            ui.separator();
                        }
                        ui.selectable_value(&mut self.post_index, i, name.as_str());
                    }
                });
                if self.post_index != before {
                    self.remember_posts();
                    // Files for this machine use arcs; it can still be turned off above.
                    if self.is_planetcnc() {
                        self.output_arcs = true;
                    }
                }
                if self.is_planetcnc() {
                    note(ui, "Laid out like your own PlanetCNC file, in millimetres.");
                }
                ui.horizontal(|ui| {
                    if ui.button("Load .pp Files ...").clicked() {
                        if let Some(files) = rfd::FileDialog::new().add_filter("Post processor", &["pp", "PP"]).pick_files() {
                            self.report_loaded_posts(files);
                        }
                    }
                    if ui.button("Load a Folder ...").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            let files = App::post_files_in(&dir);
                            self.report_loaded_posts(files);
                        }
                    }
                });
                if let Some(def) = self.post_index.checked_sub(POSTS.len()).and_then(|k| self.loaded_posts.get(k)) {
                    let unit = match def.metric {
                        Some(true) => "millimetres".to_string(),
                        Some(false) => "inches".to_string(),
                        None => format!("the job's unit ({})", self.doc.job.unit()),
                    };
                    let mut lines = vec![format!("Writes .{} files in {unit}.", def.extension), if !self.output_arcs {
                        "Arcs are written as short straight lines."
                    } else if def.can_write_arcs() {
                        "Curves are written as arcs (G2 / G3)."
                    } else {
                        "This file doesn't say how to write arcs, so curves stay short straight lines."
                    }.to_string()];
                    lines.extend(def.notes.iter().cloned());
                    for line in &lines {
                        note(ui, line);
                    }
                    if ui.button("Remove This One").clicked() {
                        self.remove_selected_post();
                    }
                }
                ui.add_space(4.0);
                let can = !names.is_empty();
                if ui.add_enabled(can, egui::Button::new(RichText::new("Save Toolpath(s) ...").color(theme::ON_ACCENT).strong()).fill(theme::ACCENT).min_size(vec2(ui.available_width() - 2.0, 24.0))).clicked() {
                    self.save_gcode_dialog();
                }
            });
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 80.0).max(0.0));
                if wide(ui, "Close", 74.0).clicked() {
                    self.right = RightMode::List;
                }
            });
        });
    }

    /// Loads post processor files and says plainly how it went.
    fn report_loaded_posts(&mut self, files: Vec<std::path::PathBuf>) {
        if files.is_empty() {
            self.message = Some("That folder has no .pp files.".into());
            return;
        }
        let first_new = POSTS.len() + self.loaded_posts.len();
        let (added, failed) = self.load_posts(files);
        if added > 0 {
            self.post_index = first_new;
            self.remember_posts();
        }
        self.status = format!("Loaded {added} post processor(s).");
        if !failed.is_empty() {
            self.message = Some(format!("Loaded {added}. These couldn't be read:\n{}", failed.join("\n")));
        }
    }

    pub fn save_gcode_dialog(&mut self) {
        let only = if self.one_file { None } else { self.selected_tp };
        if self.gcode_text(only).is_none() {
            self.message = Some("Tick at least one toolpath in the list to save it.".into());
            return;
        }
        let ext = self.post_extension(self.post_index);
        let name = format!("toolpaths.{ext}");
        if let Some(path) = rfd::FileDialog::new().add_filter("Machine file", &[ext.as_str()]).set_file_name(name).save_file() {
            let stem = path.file_stem().map_or_else(|| "toolpaths".to_string(), |s| s.to_string_lossy().to_string());
            let text = self.gcode_text_named(only, &stem).unwrap_or_default();
            let bytes = if self.is_planetcnc() { crate::planetcnc::file_bytes(&text) } else { text.into_bytes() };
            match std::fs::write(&path, bytes) {
                Ok(()) => self.status = format!("Saved toolpaths to {}", path.display()),
                Err(e) => self.message = Some(format!("Could not write {}: {e}", path.display())),
            }
        }
    }

    pub fn op_form(&mut self, ui: &mut Ui, mut op: Op, editing: Option<usize>, mut tab: usize, mut error: Option<String>) {
        let full = ui.available_rect_before_wrap();
        let unit = self.doc.job.unit();
        if let Some((target, tool)) = self.picked_tool.take() {
            let tool = tool.in_units(self.doc.job.units == Units::Mm);
            if target == 0 {
                if op.stepover > 0.0 {
                    op.stepover = (tool.dia * tool.stepover).max(1e-4);
                }
                op.tool = tool;
            } else {
                op.clearance_tool = Some(tool);
            }
        }
        let mut action = 0;
        let bar_height = if error.is_some() { 76.0 } else { 44.0 };
        let (form, bar) = full.split_top_bottom_at_y(full.bottom() - bar_height);
        ui.scope_builder(egui::UiBuilder::new().max_rect(form), |ui| {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 6, bottom: 6 }).show(ui, |ui| {
                ui.set_width(full.width() - 30.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                let icon = op_icon(op.kind);
                title_card(ui, icon, op.kind.title());
                self.op_depths(ui, &mut op, unit);
                self.op_tool(ui, &mut op, unit);
                match op.kind {
                    OpKind::Profile => self.profile_cards(ui, &mut op, unit, &mut tab),
                    OpKind::Pocket => self.pocket_cards(ui, &mut op, unit),
                    OpKind::Drill => drill_cards(ui, &mut op, unit),
                    OpKind::Engrave => engrave_cards(ui, &mut op, unit),
                    OpKind::VCarve => self.vcarve_cards(ui, &mut op, unit),
                    OpKind::Prism => {
                        card(ui, |ui| {
                            ui.horizontal(|ui| {
                                bold(ui, "Cut");
                                ui.radio_value(&mut op.climb, true, "Climb");
                                ui.radio_value(&mut op.climb, false, "Conventional");
                            });
                        });
                    }
                    OpKind::Texture => texture_cards(ui, &mut op, unit),
                    OpKind::Moulding => moulding_cards(ui, &mut op, unit),
                    OpKind::Flute => flute_cards(ui, &mut op),
                    OpKind::Chamfer => {}
                    OpKind::Rough3D | OpKind::Finish3D => model_cards(ui, &mut op, unit),
                }
                if matches!(op.kind, OpKind::Pocket | OpKind::Drill | OpKind::VCarve | OpKind::Prism) {
                    card(ui, |ui| {
                        let mut by_selection = op.order == toolpath::Order::AsDrawn;
                        if ui.checkbox(&mut by_selection, "Use Vector Selection Order").changed() {
                            op.order = if by_selection { toolpath::Order::AsDrawn } else { toolpath::Order::Nearest };
                        }
                    });
                }
                self.op_footer(ui, &mut op, unit);
            });
        });
        });
        // Always in view, however long the form above it.
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.painter().hline(bar.x_range(), bar.top(), egui::Stroke::new(1.0, theme::LINE));
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 8 }).show(ui, |ui| {
                ui.set_width(bar.width() - 30.0);
                if let Some(e) = &error {
                    ui.colored_label(theme::ERROR, e);
                }
                ui.horizontal(|ui| {
                    if primary(ui, "Calculate", 92.0).clicked() {
                        action = 1;
                    }
                    ui.add_space((ui.available_width() - 80.0).max(0.0));
                    if wide(ui, "Close", 74.0).clicked() {
                        action = 2;
                    }
                });
            });
        });
        if matches!(self.right, RightMode::Op { .. }) {
            error = if action == 0 { error } else { None };
            self.right = RightMode::Op { op: Box::new(op), editing, tab, error };
        }
        match action {
            1 => self.confirm_depth_then_calculate(),
            2 => self.right = RightMode::List,
            _ => {}
        }
    }

    fn op_depths(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        match op.kind {
            OpKind::Profile | OpKind::Pocket | OpKind::Drill => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, "Cutting Depths");
                    labelled(ui, "Start Depth (D)", 106.0, &mut op.start_depth, 50.0, unit);
                    labelled(ui, "Cut Depth (C)", 106.0, &mut op.cut_depth, 50.0, unit);
                    if op.kind != OpKind::Drill {
                        ui.checkbox(&mut self.show_advanced, "Show advanced toolpath options");
                    }
                });
            }
            OpKind::VCarve => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, "Cutting Depths");
                    labelled(ui, "Start Depth (D)", 106.0, &mut op.start_depth, 50.0, unit);
                    let mut flat = op.flat_depth > 0.0;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut flat, "Flat Depth (F)").changed() {
                            op.flat_depth = if flat { self.doc.job.thickness * 0.2 } else { 0.0 };
                            if !flat {
                                op.clearance_tool = None;
                            }
                        }
                        ui.add_space(4.0);
                        ui.add_enabled_ui(flat, |ui| {
                            if flat {
                                number(ui, &mut op.flat_depth, 50.0);
                            } else {
                                ui.label(RichText::new("None").color(theme::FAINT));
                            }
                            ui.label(unit);
                        });
                    });
                });
            }
            OpKind::Prism => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, "Cutting Depths");
                    labelled(ui, "Start Depth (D)", 106.0, &mut op.start_depth, 50.0, unit);
                    labelled(ui, "Prism Depth", 106.0, &mut op.cut_depth, 50.0, unit);
                    ui.vertical_centered(|ui| {
                        if wide(ui, "Set Depth for Full Prism", 170.0).clicked() {
                            let half = (op.tool.angle.to_radians() / 2.0).tan().max(1e-3);
                            let widest = toolpath::widest_inside(&self.selected_paths());
                            if widest > 0.0 {
                                op.cut_depth = widest / half;
                            } else {
                                self.status = "Select the closed shapes first.".into();
                            }
                        }
                    });
                });
            }
            OpKind::Texture => {
                card(ui, |ui| {
                    labelled(ui, "Start Depth", 76.0, &mut op.start_depth, 56.0, unit);
                });
            }
            OpKind::Flute => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, "Cutting Depths");
                    labelled(ui, "Start Depth (D)", 106.0, &mut op.start_depth, 50.0, unit);
                    labelled(ui, "Flute Depth (F)", 106.0, &mut op.cut_depth, 50.0, unit);
                });
            }
            OpKind::Chamfer => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, "Chamfer");
                    labelled(ui, "Start Depth (D)", 106.0, &mut op.start_depth, 50.0, unit);
                    labelled(ui, "Chamfer Width (W)", 106.0, &mut op.chamfer_width, 50.0, unit);
                    note(ui, &format!("Cut depth: {} {unit}", toolpath::trim(toolpath::chamfer_depth(op))));
                    note(ui, "The tip of the V-bit follows the line.");
                });
            }
            _ => {}
        }
    }

    fn op_tool(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let (select, edit) = tool_card(ui, &op.tool);
            if select {
                self.open_tool_list(Some(0));
            }
            if edit {
                self.edit_passes = !self.edit_passes;
            }
            if matches!(op.kind, OpKind::Profile | OpKind::Pocket | OpKind::Rough3D) {
                let passes = toolpath::pass_list(op).len();
                ui.horizontal(|ui| {
                    bold(ui, "Passes:");
                    ui.label(if op.kind == OpKind::Rough3D { "Auto".to_string() } else { passes.to_string() });
                    ui.add_space((ui.available_width() - 112.0).max(0.0));
                    if wide(ui, "Edit Passes ...", 110.0).clicked() {
                        self.open_passes(op);
                    }
                });
            }
            if self.edit_passes {
                ui.separator();
                labelled(ui, "Pass depth", 66.0, &mut op.tool.pass_depth, 50.0, unit);
                labelled(ui, "Diameter", 66.0, &mut op.tool.dia, 50.0, unit);
                if matches!(op.tool.kind, ToolKind::VBit | ToolKind::Engraving) {
                    labelled(ui, "Angle", 66.0, &mut op.tool.angle, 50.0, "degrees");
                }
                let mut percent = op.tool.stepover * 100.0;
                if labelled(ui, "Stepover", 66.0, &mut percent, 50.0, "%") {
                    op.tool.stepover = (percent / 100.0).clamp(0.01, 1.0);
                }
                rate_fields(ui, &mut op.tool, unit, 66.0, 50.0);
                labelled(ui, "Spindle", 66.0, &mut op.tool.rpm, 50.0, "r.p.m");
            }
            if op.kind == OpKind::Engrave {
                ui.separator();
                labelled(ui, "Depth / Pressure", 94.0, &mut op.cut_depth, 50.0, unit);
            }
        });
    }

    /// The larger tool used to clear a pocket or the flats of a V-carve.
    fn clearance_card(&mut self, ui: &mut Ui, op: &mut Op, title: &str, enabled: bool) {
        card(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let mut on = op.clearance_tool.is_some();
                if ui.checkbox(&mut on, title).changed() {
                    if on {
                        self.open_tool_list(Some(1));
                    } else {
                        op.clearance_tool = None;
                    }
                }
                match &op.clearance_tool {
                    Some(t) => {
                        ui.label(t.label());
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 66.0).max(0.0));
                            if wide(ui, "Select ...", 64.0).clicked() {
                                self.open_tool_list(Some(1));
                            }
                        });
                    }
                    None => note(ui, "Not using area clear tool"),
                }
            });
        });
    }

    fn pocket_cards(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        if self.show_advanced {
            self.clearance_card(ui, op, "Use Larger Area Clearance Tool", true);
        }
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, "Clear Pocket ...");
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.raster, false, "Offset");
                ui.radio_value(&mut op.raster, true, "Raster");
            });
            ui.horizontal(|ui| {
                ui.label("Cut Direction");
                ui.radio_value(&mut op.climb, true, "Climb");
                ui.radio_value(&mut op.climb, false, "Conventional");
            });
            ui.add_enabled_ui(op.raster, |ui| {
                labelled(ui, "Raster", 40.0, &mut op.raster_angle, 50.0, "degrees");
                ui.horizontal(|ui| {
                    ui.label("Profile");
                    profile_combo(ui, &mut op.profile_pass, "pocket_profile");
                });
                ui.add_enabled_ui(op.profile_pass != ProfilePass::None, |ui| {
                    labelled(ui, "Raster Allowance", 96.0, &mut op.raster_allowance, 50.0, unit);
                });
            });
            ui.separator();
            labelled(ui, "Pocket Allowance", 96.0, &mut op.allowance, 50.0, unit);
        });
        card(ui, |ui| {
            ui.checkbox(&mut op.ramp, "Ramp Plunge Moves");
            ui.add_enabled_ui(op.ramp, |ui| {
                labelled(ui, "Distance", 56.0, &mut op.ramp_distance, 50.0, unit);
            });
        });
    }

    fn vcarve_cards(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        let flat = op.flat_depth > 0.0;
        self.clearance_card(ui, op, "Use Flat Area Clearance Tool", flat);
        card(ui, |ui| {
            ui.add_enabled_ui(flat && op.clearance_tool.is_some(), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                bold(ui, "Flat Area Clearance ...");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut op.raster, false, "Offset");
                    ui.radio_value(&mut op.raster, true, "Raster");
                });
                labelled(ui, "Raster Angle", 70.0, &mut op.raster_angle, 50.0, "degrees");
            });
            let _ = unit;
        });
    }

    fn op_footer(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let m = self.doc.machine;
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 14.0], egui::Label::new("Safe"));
                ui.label(format!("{} {unit}", toolpath::trim(m.safe_z)));
            });
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 14.0], egui::Label::new("Home"));
                ui.label(format!("X:{:.2} Y:{:.2} Z:{:.2}", m.home[0], m.home[1], m.home[2]));
            });
            let is_3d = op.kind.uses_model();
            if !is_3d && op.kind != OpKind::Moulding && op.kind != OpKind::Engrave {
                if self.doc.components.is_empty() {
                    ui.add_enabled(false, egui::Checkbox::new(&mut op.project, "Project toolpath onto 3D model")).on_disabled_hover_text("Create a 3D shape first");
                } else {
                    ui.checkbox(&mut op.project, "Project toolpath onto 3D model");
                }
            }
            if op.kind != OpKind::Moulding {
                ui.horizontal(|ui| {
                    ui.label("Vector Selection:");
                    ui.label(format!("{} selected", self.selected.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if wide(ui, "Selector ...", 74.0).on_hover_text("Selects shapes by kind, size and layer").clicked() {
                            self.open_selector(op);
                        }
                    });
                });
            } else {
                note(ui, &format!("Selected vectors: {}", self.selected.len()));
            }
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(TextEdit::singleline(&mut op.name).desired_width(f32::INFINITY));
            });
        });
    }

    // ----- Tool list -----

    pub fn open_tool_list(&mut self, target: Option<u8>) {
        self.tool_edit = Some(self.doc.tools.clone());
        self.tool_target = target;
        self.tools_open = true;
    }

    pub fn tool_list(&mut self, ctx: &egui::Context) {
        let Some(mut tools) = self.tool_edit.clone() else { return };
        let mut close: Option<bool> = None;
        let mut open = true;
        self.tool_index = self.tool_index.min(tools.len().saturating_sub(1));
        egui::Window::new("Tool List").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.allocate_ui(vec2(600.0, 470.0), |ui| ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(226.0);
                    egui::Frame::new().fill(theme::INPUT).corner_radius(5).inner_margin(6).show(ui, |ui| {
                        ui.set_width(214.0);
                        egui::ScrollArea::vertical().max_height(380.0).id_salt("tools_tree").show(ui, |ui| {
                            for (metric, group) in [(false, "Imperial Tools"), (true, "Metric Tools")] {
                                egui::CollapsingHeader::new(group).default_open(true).show(ui, |ui| {
                                    for (kind, name) in [
                                        (ToolKind::EndMill, "End Mills"),
                                        (ToolKind::BallNose, "Ball Nose"),
                                        (ToolKind::VBit, "V-Bits"),
                                        (ToolKind::Engraving, "Engraving"),
                                        (ToolKind::Drill, "Drills"),
                                    ] {
                                        let ids: Vec<usize> = (0..tools.len()).filter(|i| tools[*i].metric == metric && tools[*i].kind == kind).collect();
                                        if ids.is_empty() {
                                            continue;
                                        }
                                        egui::CollapsingHeader::new(name).id_salt((metric, name)).default_open(kind == ToolKind::EndMill || kind == ToolKind::BallNose).show(ui, |ui| {
                                            for i in ids {
                                                if ui.selectable_label(self.tool_index == i, tools[i].name.as_str()).clicked() {
                                                    self.tool_index = i;
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if wide(ui, "New ...", 68.0).clicked() {
                            let mut t = toolpath::default_tools().remove(1);
                            t.name = "New Tool".into();
                            t.number = tools.iter().map(|t| t.number).max().unwrap_or(0) + 1;
                            tools.push(t);
                            self.tool_index = tools.len() - 1;
                        }
                        if wide(ui, "Copy ...", 68.0).clicked() {
                            if let Some(mut t) = tools.get(self.tool_index).cloned() {
                                t.name += " copy";
                                t.number = tools.iter().map(|t| t.number).max().unwrap_or(0) + 1;
                                tools.push(t);
                                self.tool_index = tools.len() - 1;
                            }
                        }
                        if ui.add_enabled(tools.len() > 1, egui::Button::new("Delete").min_size(vec2(68.0, 20.0))).clicked() {
                            tools.remove(self.tool_index);
                            self.tool_index = self.tool_index.min(tools.len() - 1);
                        }
                    });
                    ui.horizontal(|ui| {
                        if wide(ui, "Import ...", 106.0).clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Tool list", &["json"]).pick_file() {
                                match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Vec<Tool>>(&t).ok()) {
                                    Some(more) => tools.extend(more),
                                    None => self.message = Some(format!("{} is not a tool list.", path.display())),
                                }
                            }
                        }
                        if wide(ui, "Export ...", 106.0).clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Tool list", &["json"]).set_file_name("tools.json").save_file() {
                                if let Ok(text) = serde_json::to_string_pretty(&tools) {
                                    if let Err(e) = std::fs::write(&path, text) {
                                        self.message = Some(format!("Could not write {}: {e}", path.display()));
                                    }
                                }
                            }
                        }
                    });
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(350.0);
                    let Some(t) = tools.get_mut(self.tool_index) else { return };
                    let unit = if t.metric { "mm" } else { "inches" };
                    bold(ui, "Tool Info");
                    row_with(ui, "Name", |ui| {
                        ui.add(TextEdit::singleline(&mut t.name).desired_width(280.0));
                    });
                    row_with(ui, "Tool Type", |ui| {
                        egui::ComboBox::from_id_salt("tool_type").selected_text(kind_name(t.kind)).width(170.0).show_ui(ui, |ui| {
                            for k in [ToolKind::EndMill, ToolKind::BallNose, ToolKind::VBit, ToolKind::Engraving, ToolKind::Drill] {
                                ui.selectable_value(&mut t.kind, k, kind_name(k));
                            }
                        });
                    });
                    ui.label("Notes");
                    ui.add(TextEdit::multiline(&mut t.notes).desired_rows(3).desired_width(340.0));
                    ui.add_space(4.0);
                    bold(ui, "Geometry");
                    ui.horizontal(|ui| {
                        labelled(ui, "Diameter (D)", 86.0, &mut t.dia, 64.0, "");
                        let before = t.metric;
                        egui::ComboBox::from_id_salt("tool_unit").selected_text(if t.metric { "mm" } else { "inches" }).width(70.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.metric, false, "inches");
                            ui.selectable_value(&mut t.metric, true, "mm");
                        });
                        if before != t.metric {
                            let wanted = t.metric;
                            t.metric = before;
                            *t = t.in_units(wanted);
                        }
                    });
                    if matches!(t.kind, ToolKind::VBit | ToolKind::Engraving) {
                        labelled(ui, "Included Angle", 86.0, &mut t.angle, 64.0, "degrees");
                    }
                    ui.add_space(4.0);
                    bold(ui, "Cutting Parameters");
                    labelled(ui, "Pass Depth", 86.0, &mut t.pass_depth, 64.0, unit);
                    ui.horizontal(|ui| {
                        let mut across = t.dia * t.stepover;
                        if labelled(ui, "Stepover", 86.0, &mut across, 64.0, unit) && t.dia > 0.0 {
                            t.stepover = (across / t.dia).clamp(0.005, 1.0);
                        }
                        let mut percent = t.stepover * 100.0;
                        if number(ui, &mut percent, 50.0) {
                            t.stepover = (percent / 100.0).clamp(0.005, 1.0);
                        }
                        ui.label("%");
                    });
                    ui.add_space(4.0);
                    bold(ui, "Feeds and Speeds");
                    labelled(ui, "Spindle Speed", 86.0, &mut t.rpm, 64.0, "r.p.m");
                    rate_fields(ui, t, unit, 86.0, 64.0);
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        bold(ui, "Tool Number");
                        let mut n = t.number as usize;
                        if count_box(ui, &mut n, 50.0) {
                            t.number = n as u32;
                        }
                    });
                });
            }));
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if wide(ui, "Cancel", 74.0).clicked() {
                    close = Some(false);
                }
                let label = if self.tool_target.is_some() { "Use This Tool" } else { "OK" };
                if wide(ui, label, 92.0).clicked() {
                    close = Some(true);
                }
            });
        });
        if !open {
            close = Some(false);
        }
        match close {
            Some(true) => {
                if let (Some(target), Some(t)) = (self.tool_target, tools.get(self.tool_index)) {
                    self.picked_tool = Some((target, t.clone()));
                }
                self.doc.tools = tools;
                self.tool_edit = None;
                self.tools_open = false;
            }
            Some(false) => {
                self.tool_edit = None;
                self.tools_open = false;
            }
            None => self.tool_edit = Some(tools),
        }
    }
}

fn kind_name(k: ToolKind) -> &'static str {
    match k {
        ToolKind::EndMill => "End Mill",
        ToolKind::BallNose => "Ball Nose",
        ToolKind::VBit => "V-Bit",
        ToolKind::Engraving => "Engraving",
        ToolKind::Drill => "Drill",
    }
}

fn profile_combo(ui: &mut Ui, value: &mut ProfilePass, salt: &str) {
    let name = |p: ProfilePass| match p {
        ProfilePass::Last => "Last",
        ProfilePass::First => "First",
        ProfilePass::None => "No Profile Pass",
    };
    egui::ComboBox::from_id_salt(salt).selected_text(name(*value)).width(120.0).show_ui(ui, |ui| {
        for p in [ProfilePass::Last, ProfilePass::First, ProfilePass::None] {
            ui.selectable_value(value, p, name(p));
        }
    });
}

fn drill_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.checkbox(&mut op.peck, "Use Peck Drilling");
        ui.add_enabled_ui(op.peck, |ui| {
            ui.radio_value(&mut op.retract_to_start, true, "Retract above the cutting start depth");
            ui.radio_value(&mut op.retract_to_start, false, "Retract above the height of the previous pass");
            labelled(ui, "Retract Gap", 76.0, &mut op.retract_gap, 50.0, unit);
            ui.label(format!("Peck Depth (P)   {} {unit}", toolpath::trim(op.tool.pass_depth)));
            note(ui, "Peck depth is the tool's Pass Depth.");
        });
    });
    card(ui, |ui| {
        let mut on = op.dwell > 0.0;
        if ui.checkbox(&mut on, "Dwell at the bottom of each drill pass").changed() {
            op.dwell = if on { 0.5 } else { 0.0 };
        }
        ui.add_enabled_ui(on, |ui| {
            labelled(ui, "Dwell Time", 70.0, &mut op.dwell, 50.0, "seconds");
        });
    });
}

fn engrave_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.radio_value(&mut op.fill, false, "Outline");
            ui.radio_value(&mut op.fill, true, "Fill");
        });
        ui.add_enabled_ui(op.fill, |ui| {
            labelled(ui, "Stepover", 60.0, &mut op.stepover, 50.0, unit);
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.hatch, false, "Offset");
                ui.radio_value(&mut op.hatch, true, "Hatch");
            });
            ui.add_enabled_ui(op.hatch, |ui| {
                labelled(ui, "Hatch", 60.0, &mut op.raster_angle, 50.0, "degrees");
                ui.checkbox(&mut op.cross_hatch, "Cross Hatch");
            });
        });
    });
}

fn flute_cards(ui: &mut Ui, op: &mut Op) {
    use crate::toolpath::FluteEnds;
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, "Flute Type");
        ui.radio_value(&mut op.flute_ends, FluteEnds::Both, "Ramp at both ends");
        ui.radio_value(&mut op.flute_ends, FluteEnds::Start, "Ramp at the start only");
        ui.radio_value(&mut op.flute_ends, FluteEnds::End, "Ramp at the end only");
        let mut share = op.flute_ramp as f32;
        ui.add_space(3.0);
        ui.label("Ramp length");
        ui.spacing_mut().slider_width = 120.0;
        if ui.add(egui::Slider::new(&mut share, 1.0..=100.0).step_by(1.0).custom_formatter(|v, _| format!("{v:.0}%"))).changed() {
            op.flute_ramp = share as f64;
        }
    });
}

fn texture_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, "Texture");
        labelled(ui, "Max. Cut", 76.0, &mut op.tex_cut[1], 56.0, unit);
        labelled(ui, "Min. Cut", 76.0, &mut op.tex_cut[0], 56.0, unit);
        labelled(ui, "Max. Length", 76.0, &mut op.tex_length[1], 56.0, unit);
        labelled(ui, "Min. Length", 76.0, &mut op.tex_length[0], 56.0, unit);
        ui.horizontal(|ui| {
            ui.label("Max Overlap");
            ui.add(egui::Slider::new(&mut op.tex_variation, 0.0..=0.9).show_value(false));
            ui.label(format!("{:.0} %", op.tex_variation * 100.0));
        });
        labelled(ui, "Stepover", 76.0, &mut op.stepover, 56.0, unit);
        labelled(ui, "Angle", 76.0, &mut op.raster_angle, 56.0, "degrees");
    });
    card(ui, |ui| {
        bold(ui, "Boundary Vector Offset");
        labelled(ui, "", 4.0, &mut op.boundary_offset, 56.0, unit);
        note(ui, "With no selection, the texture covers the whole job.");
    });
}

fn moulding_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        bold(ui, "Drive Rail");
        note(ui, "Select the drive rails for the toolpath followed by the profile you wish to extrude. You may select multiple rails. The last selected vector is the profile.");
    });
    card(ui, |ui| {
        bold(ui, "Toolpath");
        labelled(ui, "Gap Above Toolpath", 110.0, &mut op.gap, 50.0, unit);
        labelled(ui, "Stepover", 110.0, &mut op.stepover, 50.0, unit);
    });
}

fn model_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, "Machining Limit Boundary");
        ui.radio_value(&mut op.boundary, Boundary::Model, "Model Boundary");
        ui.radio_value(&mut op.boundary, Boundary::Material, "Material Boundary");
        ui.radio_value(&mut op.boundary, Boundary::Selected, "Selected Vector(s)");
        labelled(ui, "Boundary", 60.0, &mut op.boundary_offset, 56.0, unit);
    });
    if op.kind == OpKind::Rough3D {
        card(ui, |ui| {
            bold(ui, "Machining Allowance");
            labelled(ui, "", 4.0, &mut op.allowance, 56.0, unit);
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, "Roughing Strategy");
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.z_level, true, "Z Level");
                ui.radio_value(&mut op.z_level, false, "3D Raster");
            });
            ui.horizontal(|ui| {
                ui.label("Raster");
                ui.radio_value(&mut op.raster_along_x, true, "Along X");
                ui.radio_value(&mut op.raster_along_x, false, "Along Y");
            });
            ui.horizontal(|ui| {
                ui.label("Profile ...");
                profile_combo(ui, &mut op.profile_pass, "rough_profile");
            });
        });
    } else {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, "Area Machine Strategy");
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.raster, false, "Offset");
                ui.radio_value(&mut op.raster, true, "Raster");
            });
            ui.horizontal(|ui| {
                ui.label("Cut Direction");
                ui.radio_value(&mut op.climb, true, "Climb");
                ui.radio_value(&mut op.climb, false, "Conventional");
            });
            ui.add_enabled_ui(op.raster, |ui| {
                labelled(ui, "Raster", 54.0, &mut op.raster_angle, 56.0, "degrees");
            });
            labelled(ui, "Stepover", 54.0, &mut op.stepover, 56.0, unit);
        });
    }
}

/// The picture of each kind of toolpath, in the forms and in the list.
pub(crate) fn op_icon(kind: OpKind) -> Icon {
    match kind {
        OpKind::Profile => Icon::TpProfile,
        OpKind::Pocket => Icon::TpPocket,
        OpKind::Drill => Icon::TpDrill,
        OpKind::Engrave => Icon::TpEngrave,
        OpKind::VCarve => Icon::TpVCarve,
        OpKind::Rough3D => Icon::TpRough,
        OpKind::Finish3D => Icon::TpFinish,
        OpKind::Prism => Icon::TpPrism,
        OpKind::Texture => Icon::TpTexture,
        OpKind::Moulding => Icon::TpMoulding,
        OpKind::Flute => Icon::TpFlute,
        OpKind::Chamfer => Icon::TpChamfer,
    }
}

/// The feed and plunge rates of a tool, per minute or per second as chosen.
pub(crate) fn rate_fields(ui: &mut Ui, tool: &mut crate::toolpath::Tool, unit: &str, label_width: f32, field_width: f32) {
    ui.horizontal(|ui| {
        ui.label("Rates in");
        ui.radio_value(&mut tool.per_second, false, format!("{unit}/min"));
        ui.radio_value(&mut tool.per_second, true, format!("{unit}/s"));
    });
    let (k, shown_unit) = if tool.per_second { (60.0, format!("{unit}/s")) } else { (1.0, format!("{unit}/min")) };
    for (name, value) in [("Feed Rate", &mut tool.feed), ("Plunge Rate", &mut tool.plunge)] {
        let mut shown = *value / k;
        if labelled(ui, name, label_width, &mut shown, field_width, &shown_unit) {
            *value = shown * k;
        }
    }
}

impl App {
    /// Where the machine's X0 Y0 is on the job, and the ways of moving it.
    fn zero_card(&mut self, ui: &mut Ui, anchor: Datum, unit: &str) {
        card(ui, |ui| {
            bold(ui, "X/Y Zero");
            ui.spacing_mut().item_spacing.y = 3.0;
            note(ui, "The machine's X0 Y0, measured from the job's bottom left corner.");
            let mut at = self.doc.job.origin();
            let changed = labelled(ui, "Zero X", 66.0, &mut at[0], 60.0, unit) | labelled(ui, "Zero Y", 66.0, &mut at[1], 60.0, unit);
            if changed {
                self.checkpoint();
                self.doc.job.set_origin(at);
            }
            ui.horizontal(|ui| {
                let spot = self.selected_bbox().map(|(lo, hi)| crate::edit::anchor_point(lo, hi, anchor));
                if ui.add_enabled(spot.is_some(), egui::Button::new("Zero to Anchor")).on_hover_text("Puts the zero on the selection's anchor point").clicked() {
                    if let Some(spot) = spot {
                        self.checkpoint();
                        self.doc.job.set_origin(spot);
                        self.status = "Moved the X/Y zero to the selection's anchor.".into();
                    }
                }
                if ui.button("Back to Corner").clicked() {
                    self.checkpoint();
                    self.doc.job.datum = Datum::BottomLeft;
                    self.doc.job.use_offset = false;
                    self.doc.job.offset = [0.0, 0.0];
                }
            });
        });
    }
}
