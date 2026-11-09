//! The left panel's tabs and forms, and the right panel's toolpath forms and tool list.

use crate::app::*;
use crate::edit::{DimKind, Edge, FilletKind};
use crate::geom::{self, BoolOp};
use crate::icons::Icon;
use crate::relief::ToolKind;
use crate::theme;
use crate::toolpath::{self, Boundary, Op, OpKind, ProfilePass, Tool, POSTS};
use crate::keys::Action;
use crate::ui::{bold, card, primary, count_box, icon_button, icon_button_saying, icon_row, label_widget, labelled, labelled_formula, labelled_from, number, number_from, title_card, wide, wood_block};
use egui::{vec2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui};

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::MUTED));
}

/// Picks one of the four corners or the middle of a box.
pub fn anchor_grid(ui: &mut Ui, value: &mut Datum, name: &str) {
    anchor_grid_with(ui, value, name, true);
}

/// The same, with or without the middle: nesting starts from one of the four corners only, as
/// the original's form has four buttons for it (`0x5aa` to `0x5ad`, `CornerToNestFrom`).
pub fn anchor_grid_with(ui: &mut Ui, value: &mut Datum, name: &str, centre: bool) {
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
        if datum == Datum::Centre && !centre {
            continue;
        }
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
        bold(ui, crate::i18n::tr("Tool:"));
        ui.add(egui::Label::new(tool.label()).truncate());
    });
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - 136.0).max(0.0));
        result.0 = wide(ui, crate::i18n::tr("Select ..."), 64.0).clicked();
        result.1 = wide(ui, crate::i18n::tr("Edit ..."), 64.0).clicked();
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
                Xform::Rotate => {
                    let (a, b) = self.moving_bbox().unwrap_or((lo, hi));
                    form.pivot = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                }
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
        if self.section_open(ui, crate::i18n::tr("Shapes")) {
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Circle, crate::i18n::tr("Draw circle"), DrawKind::Circle),
                    (Icon::Ellipse, crate::i18n::tr("Draw ellipse"), DrawKind::Ellipse),
                    (Icon::Rect, crate::i18n::tr("Draw rectangle"), DrawKind::Rect),
                    (Icon::Polygon, crate::i18n::tr("Draw polygon"), DrawKind::Polygon),
                    (Icon::Star, crate::i18n::tr("Draw star"), DrawKind::Star),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_draw(kind);
                    }
                }
            });
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Lines, crate::i18n::tr("Draw lines"), DrawKind::Polyline),
                    (Icon::Arc, crate::i18n::tr("Draw arc"), DrawKind::Arc),
                    (Icon::Curve, crate::i18n::tr("Draw curve"), DrawKind::Curve),
                    (Icon::Spiral, crate::i18n::tr("Draw spiral"), DrawKind::Spiral),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_draw(kind);
                    }
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Text, crate::i18n::tr("Draw text"), true, false).clicked() {
                    self.start_draw(DrawKind::Text);
                }
                if icon_button(ui, Icon::TextCurve, crate::i18n::tr("Text on a curve"), true, false).clicked() {
                    self.start_xform(Xform::TextOnCurve);
                }
                if icon_button(ui, Icon::Measure, crate::i18n::tr("Measure distance"), true, self.tool == Tool2::Measure).clicked() {
                    self.tool = Tool2::Measure;
                    self.left = LeftForm::None;
                    self.view3d = false;
                }
                if icon_button(ui, Icon::Dimension, crate::i18n::tr("Dimensions"), true, false).clicked() {
                    self.start_xform(Xform::Dimension);
                }
            });
        }
        ui.add_space(6.0);
        self.transform_section(ui, some);
        ui.add_space(6.0);
        if self.section_open(ui, crate::i18n::tr("Edit Shapes")) {
            self.edit_rows(ui, some, many);
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Weld, crate::i18n::tr("Weld shapes"), many, false).clicked() {
                    self.boolean_selected(BoolOp::Weld);
                }
                if icon_button(ui, Icon::Subtract, crate::i18n::tr("Subtract shapes"), many, false).clicked() {
                    self.boolean_selected(BoolOp::Subtract);
                }
                if icon_button(ui, Icon::Intersect, crate::i18n::tr("Keep the overlap"), many, false).clicked() {
                    self.boolean_selected(BoolOp::Intersect);
                }
                if icon_button(ui, Icon::Trim, crate::i18n::tr("Trim lines"), true, false).clicked() {
                    self.start_xform(Xform::Trim);
                }
                if icon_button(ui, Icon::Join, crate::i18n::tr("Join vectors"), true, false).clicked() {
                    self.start_xform(Xform::Join);
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Fillet, crate::i18n::tr("Create fillets"), true, false).clicked() {
                    self.start_xform(Xform::Fillet);
                }
                if icon_button(ui, Icon::Close, crate::i18n::tr("Close open lines"), some, false).clicked() {
                    self.close_selected();
                }
                if icon_button(ui, Icon::Validate, crate::i18n::tr("Vector validator"), true, false).clicked() {
                    self.start_xform(Xform::Validate);
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Similar, crate::i18n::tr("Select similar shapes"), some, false).clicked() {
                    let tol = self.default_similar_tolerance();
                    self.select_similar(tol);
                }
                if icon_button(ui, Icon::Bulk, crate::i18n::tr("Modify several shapes"), true, false).clicked() {
                    self.start_xform(Xform::Bulk);
                }
                if icon_button(ui, Icon::Trace, crate::i18n::tr("Trace bitmap"), true, false).clicked() {
                    self.start_trace();
                }
            });
        }
        ui.add_space(6.0);
        if self.section_open(ui, crate::i18n::tr("Offset and Repeat")) {
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Offset, crate::i18n::tr("Offset"), some, false).clicked() {
                    self.start_xform(Xform::Offset);
                }
                self.array_buttons(ui, some);
                if icon_button(ui, Icon::Nest, crate::i18n::tr("Nest parts"), some, false).clicked() {
                    self.start_xform(Xform::Nest);
                }
            });
        }
        ui.add_space(6.0);
        if self.section_open(ui, crate::i18n::tr("Sketch")) {
            use crate::constraints::Kind;
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::RDimension, crate::i18n::tr("Smart dimension"), true, false).clicked() {
                    self.start_sketch_tool(None, true);
                }
                for (icon, tip, kind) in [
                    (Icon::RCoincident, crate::i18n::tr("Coincident rule"), Kind::Coincident),
                    (Icon::RHorizontal, crate::i18n::tr("Horizontal rule"), Kind::Horizontal),
                    (Icon::RVertical, crate::i18n::tr("Vertical rule"), Kind::Vertical),
                    (Icon::RParallel, crate::i18n::tr("Parallel rule"), Kind::Parallel),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_sketch_tool(Some(kind), false);
                    }
                }
            });
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::RPerpendicular, crate::i18n::tr("Perpendicular rule"), Kind::Perpendicular),
                    (Icon::REqual, crate::i18n::tr("Equal rule"), Kind::Equal),
                    (Icon::RTangent, crate::i18n::tr("Tangent rule"), Kind::Tangent),
                    (Icon::RConcentric, crate::i18n::tr("Concentric rule"), Kind::Concentric),
                ] {
                    if icon_button(ui, icon, tip, true, false).clicked() {
                        self.start_sketch_tool(Some(kind), false);
                    }
                }
                let all_guides = some && self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).all(|v| v.construction);
                if icon_button(ui, Icon::RConstruction, crate::i18n::tr("Normal / construction"), some, all_guides).clicked() {
                    self.set_construction(!all_guides);
                }
            });
            icon_row(ui, |ui| {
                let on = matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Constraints);
                if icon_button(ui, Icon::Constraint, crate::i18n::tr("Drawing constraints"), true, on).clicked() {
                    self.start_constraint_form();
                }
                if !self.doc.constraints.is_empty() {
                    ui.label(RichText::new(tf!("{} rules", self.doc.constraints.len())).color(theme::MUTED));
                }
            });
        }
    }

    fn transform_section(&mut self, ui: &mut Ui, some: bool) {
        if self.section_open(ui, crate::i18n::tr("Move and Size")) {
            icon_row(ui, |ui| {
                for (icon, tip, kind) in [
                    (Icon::Move, crate::i18n::tr("Move"), Xform::Move),
                    (Icon::Scale, crate::i18n::tr("Set size"), Xform::Scale),
                    (Icon::Rotate, crate::i18n::tr("Rotate"), Xform::Rotate),
                    (Icon::Mirror, crate::i18n::tr("Mirror"), Xform::Mirror),
                    (Icon::Align, crate::i18n::tr("Alignment tools"), Xform::Align),
                    (Icon::Distort, crate::i18n::tr("Distort object"), Xform::Distort),
                ] {
                    if icon_button(ui, icon, tip, some, false).clicked() {
                        self.start_xform(kind);
                    }
                }
            });
        }
    }

    fn edit_rows(&mut self, ui: &mut Ui, some: bool, many: bool) {
        let apart = self.can_ungroup();
        let (can_make, in_piece) = (self.can_make_piece(), self.selection_in_piece());
        icon_row(ui, |ui| {
            if icon_button(ui, Icon::Select, crate::i18n::tr("Select"), true, self.tool == Tool2::Select).clicked() {
                self.tool = Tool2::Select;
                self.left = LeftForm::None;
            }
            if icon_button(ui, Icon::Nodes, crate::i18n::tr("Edit points"), true, self.tool == Tool2::Nodes).clicked() {
                self.tool = Tool2::Nodes;
                self.left = LeftForm::None;
                self.view3d = false;
            }
            if icon_button_saying(ui, Icon::Group, crate::i18n::tr("Group"), &tf!("Group ({})\nHold the selected shapes together. Select two or more.", self.keys.text(Action::Group)), many, false).clicked() {
                self.group_selected();
            }
            if icon_button_saying(ui, Icon::Ungroup, crate::i18n::tr("Ungroup"), &tf!("Ungroup ({})\nTake the selected groups apart by one level, each shape back on its own layer.\n{}: they stay on the group's layer.", self.keys.text(Action::Ungroup), self.keys.text(Action::UngroupStay)), apart, false).clicked() {
                self.ungroup_selected();
            }
            if icon_button_saying(ui, Icon::UngroupAll, crate::i18n::tr("Ungroup All"), &tf!("Ungroup All ({})\nTake the selected groups apart, and the groups inside them.", self.keys.text(Action::UngroupAll)), apart, false).clicked() {
                self.ungroup_with(true, true);
            }
        });
        icon_row(ui, |ui| {
            if icon_button_saying(ui, Icon::MakePiece, crate::i18n::tr("Make Piece"), &tf!("Make Piece ({})\nEach shape can still be selected alone, for its toolpaths, but moving one moves them all. A group in the selection comes apart. Select two or more shapes, or a group.", self.keys.text(Action::MakePiece)), can_make, false).clicked() {
                self.make_piece();
            }
            if icon_button_saying(ui, Icon::SelectPiece, crate::i18n::tr("Select Piece"), &tf!("Select Piece ({})\nSelect all the shapes of this piece.", self.keys.text(Action::SelectPiece)), in_piece, false).clicked() {
                self.select_piece();
            }
            if icon_button_saying(ui, Icon::AdjustPiece, crate::i18n::tr("Adjust Alone"), &tf!("Adjust Alone ({})\nWhile this is on, moving or turning a shape of a piece leaves the rest where it is, to adjust its place in the piece. Its size can always be changed alone.", self.keys.text(Action::AdjustPiece)), in_piece || self.adjust_piece, self.adjust_piece).clicked() {
                self.adjust_piece = !self.adjust_piece;
            }
            if icon_button_saying(ui, Icon::ReleasePiece, crate::i18n::tr("Release Piece"), &tf!("Release Piece ({})\nThe shapes stay as they are, but no longer move together.", self.keys.text(Action::ReleasePiece)), in_piece, false).clicked() {
                self.release_piece();
            }
            if icon_button(ui, Icon::Delete, crate::i18n::tr("Delete"), some, false).clicked() {
                self.delete_selected();
            }
        });
    }

    fn array_buttons(&mut self, ui: &mut Ui, some: bool) {
        for (icon, tip, kind) in [
            (Icon::BlockCopy, crate::i18n::tr("Array copy"), Xform::BlockCopy),
            (Icon::CircularCopy, crate::i18n::tr("Circular copy"), Xform::CircularCopy),
            (Icon::CopyAlong, crate::i18n::tr("Copy along vectors"), Xform::CopyAlong),
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
        if self.section_open(ui, crate::i18n::tr("3D Shapes")) {
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Dome, crate::i18n::tr("Create shape from selection"), true, false).clicked() {
                    self.open_shape_form(None);
                }
                if icon_button(ui, Icon::Edit, crate::i18n::tr("Edit shape"), comp, false).clicked() {
                    self.open_shape_form(self.selected_comp);
                }
                if icon_button(ui, Icon::Duplicate, crate::i18n::tr("Copy shape"), comp, false).clicked() {
                    if let Some(i) = self.selected_comp {
                        self.checkpoint();
                        let mut c = self.doc.components[i].clone();
                        c.name += crate::i18n::tr(" copy");
                        self.doc.components.insert(i + 1, c);
                        self.selected_comp = Some(i + 1);
                        self.model_dirty = true;
                        self.sim = None;
                    }
                }
                if icon_button(ui, Icon::ReliefPicture, crate::i18n::tr("3D relief from picture"), true, false).clicked() {
                    self.dialog_relief_picture();
                }
                if icon_button(ui, Icon::ImportModel, crate::i18n::tr("Import 3D model"), true, false).clicked() {
                    self.dialog_import_model();
                }
                if icon_button(ui, Icon::ZeroPlane, crate::i18n::tr("Add Zero Plane"), true, false).clicked() {
                    self.add_zero_plane();
                }
                if icon_button(ui, Icon::Delete, crate::i18n::tr("Delete shape"), comp, false).clicked() {
                    self.delete_component();
                }
            });
        }
        ui.add_space(6.0);
        self.transform_section(ui, some);
        ui.add_space(6.0);
        if self.section_open(ui, crate::i18n::tr("Edit Shapes")) {
            self.edit_rows(ui, some, many);
        }
        ui.add_space(6.0);
        if self.section_open(ui, crate::i18n::tr("Repeat")) {
            icon_row(ui, |ui| self.array_buttons(ui, some));
        }
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            let mut all = !self.doc.components.is_empty() && self.doc.components.iter().all(|c| c.visible);
            if ui.checkbox(&mut all, "").on_hover_text(crate::i18n::tr("Show or hide all components")).changed() {
                for c in &mut self.doc.components {
                    c.visible = all;
                }
                self.model_dirty = true;
                self.sim = None;
            }
            bold(ui, crate::i18n::tr("Components"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Down, crate::i18n::tr("Move component down"), comp, false).clicked() {
                    self.move_component(1);
                }
                if icon_button(ui, Icon::Up, crate::i18n::tr("Move component up"), comp, false).clicked() {
                    self.move_component(-1);
                }
            });
        });
        ui.separator();
        if self.doc.components.is_empty() {
            note(ui, crate::i18n::tr("Draw a closed shape, select it, then use the first tool above to make it 3D."));
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

    /// Takes the chosen component away, the Delete key's job in the component list (`0x1403ba190`).
    pub fn delete_component(&mut self) {
        if let Some(i) = self.selected_comp.filter(|&i| i < self.doc.components.len()) {
            self.checkpoint();
            self.doc.components.remove(i);
            self.selected_comp = None;
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
        self.clip_models.clear();
        if let Some(dir) = &folder {
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(crate::modeltools::CLIP3D_EXTENSION))).collect();
                files.sort();
                for f in files {
                    if let Some(c) = crate::platform::read(&f).ok().and_then(|b| crate::modeltools::read_clip3d(&String::from_utf8_lossy(&b)).ok()) {
                        self.clip_models.push((f.file_stem().unwrap_or_default().to_string_lossy().to_string(), c));
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut files: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dxf")))
                    .collect();
                files.sort();
                for f in files {
                    let paths = crate::platform::read(&f).map(|b| geom::dxf_read(&String::from_utf8_lossy(&b))).unwrap_or_default();
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
            ui.selectable_value(&mut self.clip_tab, 0, crate::i18n::tr("Library Browser"));
            ui.selectable_value(&mut self.clip_tab, 1, crate::i18n::tr("Local Files"));
        });
        ui.separator();
        if self.clip_tab == 1 {
            self.local_files(ui);
            return;
        }
        ui.horizontal(|ui| {
            bold(ui, crate::i18n::tr("Library Folders"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_down = self.clip_folder >= 1 && self.clip_folder < self.clip_folders.len();
                if icon_button(ui, Icon::Down, crate::i18n::tr("Move folder down"), can_down, false).clicked() {
                    self.clip_folders.swap(self.clip_folder - 1, self.clip_folder);
                    self.clip_folder += 1;
                    save_clip_folders(&self.clip_folders);
                }
                let can_up = self.clip_folder >= 2;
                if icon_button(ui, Icon::Up, crate::i18n::tr("Move folder up"), can_up, false).clicked() {
                    self.clip_folders.swap(self.clip_folder - 1, self.clip_folder - 2);
                    self.clip_folder -= 1;
                    save_clip_folders(&self.clip_folders);
                }
            });
        });
        if ui.selectable_label(self.clip_folder == 0, crate::i18n::tr("Sample Shapes")).clicked() {
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
            if ui.add_enabled(self.clip_folder > 0, egui::Button::new(crate::i18n::tr("Remove Folder"))).clicked() {
                self.clip_folders.remove(self.clip_folder - 1);
                self.clip_folder = 0;
                save_clip_folders(&self.clip_folders);
            }
            if ui.button(crate::i18n::tr("Add Folder ...")).clicked() {
                if let Some(dir) = crate::platform::FileDialog::new().pick_folder() {
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
                        if self.clip_tile(ui, crate::i18n::tr(name), &shape) {
                            let size = self.doc.job.width.min(self.doc.job.height) * 0.3;
                            self.insert_paths_centred(shape, Some(size));
                        }
                    }
                });
            }
        } else if self.clip_files.is_empty() && self.clip_models.is_empty() {
            note(ui, crate::i18n::tr("No DXF or 3D clipart files in this folder."));
        } else {
            // 3D clipart first: each becomes a component in the middle of the job.
            let models = self.clip_models.clone();
            for row in models.chunks(3) {
                ui.horizontal(|ui| {
                    for (name, cs) in row {
                        let outline: Vec<geom::Path> = cs.iter().flat_map(|c| c.paths.iter().cloned()).collect();
                        if self.clip_tile(ui, name, &outline) {
                            self.insert_3d_clipart(cs.clone());
                        }
                    }
                });
            }
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
        bold(ui, crate::i18n::tr("Files on this computer"));
        note(ui, crate::i18n::tr("Bring in a drawing from a DXF file. It lands in the middle of the job."));
        ui.add_space(4.0);
        if wide(ui, crate::i18n::tr("Import a File ..."), 130.0).clicked() {
            self.dialog_import();
        }
    }

    /// The sheets: which one shows, adding and removing them, and sending the chosen shapes to another.
    fn sheets_bar(&mut self, ui: &mut Ui) {
        let count = self.doc.sheet_count();
        let name = |i: usize| tf!("Sheet {}", i + 1);
        ui.horizontal(|ui| {
            bold(ui, crate::i18n::tr("Sheet"));
            let mut showing = self.doc.active_sheet as usize;
            egui::ComboBox::from_id_salt("sheet").selected_text(name(showing)).width(90.0).show_ui(ui, |ui| {
                for i in 0..count {
                    ui.selectable_value(&mut showing, i, name(i));
                }
            });
            if showing != self.doc.active_sheet as usize {
                self.show_sheet(showing);
            }
            if ui.small_button("+").on_hover_text(crate::i18n::tr("Add a sheet of the job's size")).clicked() {
                self.add_sheet();
            }
            if ui.add_enabled(count > 1, egui::Button::new("−").small()).on_hover_text(crate::i18n::tr("Delete this sheet with its shapes and toolpaths")).clicked() {
                self.delete_sheet();
            }
        });
        if count > 1 && !self.selected.is_empty() {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Move selection to"));
                let mut target = self.doc.active_sheet as usize;
                egui::ComboBox::from_id_salt("sheet-move").selected_text(name(target)).width(90.0).show_ui(ui, |ui| {
                    for i in 0..count {
                        ui.selectable_value(&mut target, i, name(i));
                    }
                });
                if target != self.doc.active_sheet as usize {
                    self.move_selected_to_sheet(target);
                }
            });
        }
        ui.separator();
    }

    pub fn layers_tab(&mut self, ui: &mut Ui) {
        self.sheets_bar(ui);
        let active = self.doc.active_layer;
        let count = self.doc.layers.len();
        ui.horizontal(|ui| {
            bold(ui, crate::i18n::tr("Layers List"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Down, crate::i18n::tr("Move layer down"), active + 1 < count, false).clicked() {
                    self.swap_layers(active, active + 1);
                }
                if icon_button(ui, Icon::Up, crate::i18n::tr("Move layer up"), active > 0, false).clicked() {
                    self.swap_layers(active, active - 1);
                }
            });
        });
        ui.separator();
        let mut remove = None;
        for i in 0..count {
            ui.horizontal(|ui| {
                let on = self.doc.layers[i].visible;
                let tip = tf!("Show or hide {}", self.doc.layers[i].name);
                if icon_button(ui, if on { Icon::Eye } else { Icon::EyeOff }, &tip, true, false).clicked() {
                    self.doc.layers[i].visible = !on;
                }
                ui.color_edit_button_srgb(&mut self.doc.layers[i].color).on_hover_text(crate::i18n::tr("Line colour"));
                let locked = self.doc.layers[i].locked;
                let tip = tf!("Lock {}", self.doc.layers[i].name);
                if icon_button(ui, if locked { Icon::Locked } else { Icon::Unlocked }, &tip, true, locked).clicked() {
                    self.doc.layers[i].locked = !locked;
                    if !locked {
                        let in_layer: Vec<u64> = self.doc.vectors.iter().filter(|v| v.layer == i).map(|v| v.id).collect();
                        self.selected.retain(|s| !in_layer.contains(s));
                    }
                }
                ui.radio_value(&mut self.doc.active_layer, i, "").on_hover_text(crate::i18n::tr("Draw on this layer"));
                // All the room left in the row, keeping some for the delete button. A longer name shows when pointed at.
                let room = (ui.available_width() - if count > 1 { 40.0 } else { 8.0 }).max(62.0);
                let name = self.doc.layers[i].name.clone();
                ui.add(TextEdit::singleline(&mut self.doc.layers[i].name).desired_width(room)).on_hover_text(name);
                if count > 1 && icon_button(ui, Icon::Delete, &tf!("Delete layer {}", i + 1), true, false).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.checkpoint();
            self.doc.layers.remove(i);
            for v in &mut self.doc.vectors {
                v.each_mut(&mut |v| {
                    if v.layer == i {
                        v.layer = 0;
                    } else if v.layer > i {
                        v.layer -= 1;
                    }
                });
            }
            self.doc.active_layer = self.doc.active_layer.min(self.doc.layers.len() - 1);
        }
        ui.add_space(6.0);
        if wide(ui, crate::i18n::tr("Add New Layer ..."), ui.available_width() - 16.0).clicked() {
            self.checkpoint();
            let n = self.doc.layers.len() + 1;
            self.doc.layers.push(Layer::named(&tf!("Layer {n}")));
            self.doc.active_layer = n - 1;
        }
        let some = !self.selected.is_empty();
        if ui.add_enabled(some, egui::Button::new(crate::i18n::tr("Move Selection to Active Layer"))).clicked() {
            self.checkpoint();
            let layer = self.doc.active_layer;
            for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
                v.layer = layer;
            }
        }
        self.pieces_section(ui);
    }

    fn swap_layers(&mut self, a: usize, b: usize) {
        self.checkpoint();
        self.doc.layers.swap(a, b);
        for v in &mut self.doc.vectors {
            v.each_mut(&mut |v| {
                if v.layer == a {
                    v.layer = b;
                } else if v.layer == b {
                    v.layer = a;
                }
            });
        }
        self.doc.active_layer = b;
    }

    /// The form of the Lines and Curve tools.
    pub fn line_form(&mut self, ui: &mut Ui, mut f: DrawForm) {
        let icon = if f.kind == DrawKind::Curve { Icon::Curve } else { Icon::Lines };
        title_card(ui, icon, if f.kind == DrawKind::Curve { crate::i18n::tr("Create Curve") } else { crate::i18n::tr("Create Line / Polyline") });
        let last = f.points.last().copied();
        card(ui, |ui| {
            bold(ui, crate::i18n::tr("Next Point"));
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
                ui.label(crate::i18n::tr("Angle"));
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
                if wide(ui, crate::i18n::tr("Add"), 56.0).clicked() {
                    f.points.push(f.next);
                }
                if ui.add_enabled(!f.points.is_empty(), egui::Button::new(crate::i18n::tr("Undo")).min_size(vec2(56.0, 20.0))).clicked() {
                    f.points.pop();
                }
                if ui.add_enabled(f.points.len() >= 2, egui::Button::new(crate::i18n::tr("Finish")).min_size(vec2(56.0, 20.0))).clicked() {
                    self.left = LeftForm::Draw(f.clone());
                    self.finish_line(false);
                    f.points.clear();
                }
            });
            ui.separator();
            bold(ui, crate::i18n::tr("Cursor Position"));
            if let Some(c) = self.cursor {
                let from = f.points.last().copied().unwrap_or([0.0, 0.0]);
                let (dx, dy) = (c[0] - from[0], c[1] - from[1]);
                let zero = self.doc.job.origin();
                ui.label(format!("X: {:.4}   Y: {:.4}", c[0] - zero[0], c[1] - zero[1]));
                ui.label(tf!("Angle: {:.2}   L: {:.4}", dy.atan2(dx).to_degrees(), dx.hypot(dy)));
                ui.label(format!("dx: {dx:.4}   dy: {dy:.4}"));
            } else {
                note(ui, crate::i18n::tr("Move over the sheet to see it here."));
            }
            ui.separator();
            note(ui, crate::i18n::tr("Click on the sheet to add points. Press the Space bar to finish this line and start a new one. Right click to finish and leave. Click the first point to close the shape."));
        });
        if matches!(self.left, LeftForm::Draw(_)) {
            self.left = LeftForm::Draw(f);
        }
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                self.left = LeftForm::None;
            }
        });
    }

    fn apply_close(&mut self, ui: &mut Ui, apply: &str, enabled: bool) -> bool {
        let mut pressed = false;
        ui.horizontal(|ui| {
            pressed = ui.add_enabled(enabled, egui::Button::new(RichText::new(apply).color(theme::ON_ACCENT).strong()).fill(theme::ACCENT).min_size(vec2(74.0, 22.0))).clicked();
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                self.left = LeftForm::None;
            }
        });
        pressed
    }

    pub fn xform_form(&mut self, ui: &mut Ui, mut f: XformForm) {
        let some = !self.selected.is_empty();
        let unit = self.doc.job.unit();
        let (icon, title) = match f.kind {
            Xform::Move => (Icon::Move, crate::i18n::tr("Move Selection")),
            Xform::Scale => (Icon::Scale, crate::i18n::tr("Set Size")),
            Xform::Rotate => (Icon::Rotate, crate::i18n::tr("Rotate")),
            Xform::Mirror => (Icon::Mirror, crate::i18n::tr("Mirror")),
            Xform::Align => (Icon::Align, crate::i18n::tr("Alignment Tools")),
            Xform::Offset => (Icon::Offset, crate::i18n::tr("Offset")),
            Xform::BlockCopy => (Icon::BlockCopy, crate::i18n::tr("Array Copy")),
            Xform::CircularCopy => (Icon::CircularCopy, crate::i18n::tr("Circular Copy")),
            Xform::CopyAlong => (Icon::CopyAlong, crate::i18n::tr("Copy Along Vectors")),
            Xform::Join => (Icon::Join, crate::i18n::tr("Join Vectors")),
            Xform::Fillet => (Icon::Fillet, crate::i18n::tr("Create Fillets")),
            Xform::Trim => (Icon::Trim, crate::i18n::tr("Interactive Vector Trim")),
            Xform::Validate => (Icon::Validate, crate::i18n::tr("Vector Validator")),
            Xform::Dimension => (Icon::Dimension, crate::i18n::tr("Dimensions")),
            Xform::Nest => (Icon::Nest, crate::i18n::tr("Nest Parts")),
            Xform::TextOnCurve => (Icon::TextCurve, crate::i18n::tr("Text On A Curve")),
            Xform::Bulk => (Icon::Bulk, crate::i18n::tr("Modify Several Shapes")),
            Xform::Constraints => (Icon::Constraint, crate::i18n::tr("Dimensions and Rules")),
            Xform::Distort => (Icon::Distort, crate::i18n::tr("Distort Object")),
        };
        title_card(ui, icon, title);
        let mut apply_label = Some(crate::i18n::tr("Apply"));
        match f.kind {
            Xform::Move => {
                self.zero_card(ui, f.anchor, unit);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        bold(ui, crate::i18n::tr("Anchor"));
                        ui.add_space(30.0);
                        anchor_grid(ui, &mut f.anchor, crate::i18n::tr("Anchor"));
                    });
                    bold(ui, crate::i18n::tr("Type of Move"));
                    ui.horizontal(|ui| {
                        let before = f.absolute;
                        ui.radio_value(&mut f.absolute, true, crate::i18n::tr("Absolute"));
                        ui.radio_value(&mut f.absolute, false, crate::i18n::tr("Relative"));
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
                    labelled_from(ui, crate::i18n::tr("X Position"), 66.0, &mut f.dx, zero[0], 60.0, unit);
                    labelled_from(ui, crate::i18n::tr("Y Position"), 66.0, &mut f.dy, zero[1], 60.0, unit);
                });
            }
            Xform::Scale => {
                let size = self.selected_bbox().map_or([0.0, 0.0], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1]]);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        bold(ui, crate::i18n::tr("Anchor"));
                        ui.add_space(30.0);
                        anchor_grid(ui, &mut f.anchor, crate::i18n::tr("Anchor"));
                    });
                    let mut w = size[0] * f.sx / 100.0;
                    if labelled(ui, crate::i18n::tr("Width (X)"), 62.0, &mut w, 60.0, unit) && size[0] > 0.0 {
                        f.sx = w / size[0] * 100.0;
                    }
                    labelled(ui, "", 62.0, &mut f.sx, 60.0, "%");
                    ui.checkbox(&mut f.keep_ratio, crate::i18n::tr("Link XY"));
                    ui.checkbox(&mut f.each, crate::i18n::tr("Each shape on its own"));
                    if f.keep_ratio {
                        f.sy = f.sx;
                    }
                    let mut h = size[1] * f.sy / 100.0;
                    if labelled(ui, crate::i18n::tr("Height"), 62.0, &mut h, 60.0, unit) && size[1] > 0.0 {
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
                    bold(ui, crate::i18n::tr("Pivot Point"));
                    ui.horizontal(|ui| {
                        let before = f.anchor;
                        ui.vertical(|ui| anchor_grid(ui, &mut f.anchor, crate::i18n::tr("Pivot")));
                        if f.anchor != before {
                            if let Some((lo, hi)) = self.moving_bbox() {
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
                    labelled(ui, crate::i18n::tr("Angle"), 40.0, &mut f.angle, 60.0, crate::i18n::tr("degrees"));
                    ui.checkbox(&mut f.each, crate::i18n::tr("Each shape on its own, around the anchor"));
                });
            }
            Xform::Mirror => {
                apply_label = None;
                let mut go: Option<(bool, Option<Edge>, bool)> = None;
                card(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        if wide(ui, crate::i18n::tr("Top"), 60.0).clicked() {
                            go = Some((false, Some(Edge::Top), false));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 130.0).max(0.0) / 2.0);
                        if wide(ui, crate::i18n::tr("Left"), 60.0).clicked() {
                            go = Some((true, Some(Edge::Left), false));
                        }
                        if wide(ui, crate::i18n::tr("Right"), 60.0).clicked() {
                            go = Some((true, Some(Edge::Right), false));
                        }
                    });
                    ui.vertical_centered(|ui| {
                        if wide(ui, crate::i18n::tr("Bottom"), 60.0).clicked() {
                            go = Some((false, Some(Edge::Bottom), false));
                        }
                    });
                    ui.separator();
                    if wide(ui, crate::i18n::tr("Flip Horizontal"), 130.0).clicked() {
                        go = Some((true, None, f.about_job));
                    }
                    if wide(ui, crate::i18n::tr("Flip Vertical"), 130.0).clicked() {
                        go = Some((false, None, f.about_job));
                    }
                    ui.checkbox(&mut f.about_job, crate::i18n::tr("Flip about job center"));
                    ui.separator();
                    ui.checkbox(&mut f.copy, crate::i18n::tr("Create a mirrored copy"));
                });
                if let Some((h, edge, job)) = go {
                    self.mirror(h, edge, job, f.copy);
                }
            }
            Xform::Align => {
                apply_label = None;
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Align to Material"));
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Center X")).clicked() {
                            self.align_selected(Some(0), None);
                        }
                        if ui.button(crate::i18n::tr("Center")).clicked() {
                            self.align_selected(Some(0), Some(0));
                        }
                        if ui.button(crate::i18n::tr("Center Y")).clicked() {
                            self.align_selected(None, Some(0));
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (label, x, y) in [(crate::i18n::tr("Left"), Some(-1), None), (crate::i18n::tr("Right"), Some(1), None), (crate::i18n::tr("Top"), None, Some(1)), (crate::i18n::tr("Bottom"), None, Some(-1))] {
                            if ui.button(label).clicked() {
                                self.align_selected(x, y);
                            }
                        }
                    });
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Align to Selection"));
                    note(ui, crate::i18n::tr("Aligns selected objects to the LAST item in the selection."));
                    ui.label(crate::i18n::tr("Center"));
                    ui.horizontal(|ui| {
                        if ui.button("X").clicked() {
                            self.align_to_last_center(true, false);
                        }
                        if ui.button(crate::i18n::tr("Both")).clicked() {
                            self.align_to_last_center(true, true);
                        }
                        if ui.button("Y").clicked() {
                            self.align_to_last_center(false, true);
                        }
                    });
                    for (label, inside) in [(crate::i18n::tr("Inside Edge"), true), (crate::i18n::tr("Outside Edge"), false)] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            for (name, edge) in [(crate::i18n::tr("Left"), Edge::Left), (crate::i18n::tr("Right"), Edge::Right), (crate::i18n::tr("Top"), Edge::Top), (crate::i18n::tr("Bottom"), Edge::Bottom)] {
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
                    bold(ui, crate::i18n::tr("Space Selection"));
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Across")).clicked() {
                            self.space_selection(true);
                        }
                        if ui.button(crate::i18n::tr("Up and Down")).clicked() {
                            self.space_selection(false);
                        }
                    });
                });
            }
            Xform::Offset => {
                card(ui, |ui| {
                    labelled(ui, crate::i18n::tr("Distance:"), 56.0, &mut f.distance, 56.0, unit);
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.outward, true, crate::i18n::tr("Outward"));
                        ui.radio_value(&mut f.outward, false, crate::i18n::tr("Inward"));
                    });
                });
            }
            Xform::BlockCopy => {
                apply_label = Some(crate::i18n::tr("Copy"));
                let size = self.selected_bbox().map_or([0.0, 0.0], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1]]);
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Selected Objects Size"));
                    ui.label(format!("X: {:.4}   Y: {:.4}", size[0], size[1]));
                    ui.add_space(4.0);
                    row_with(ui, crate::i18n::tr("Rows (Y)"), |ui| {
                        count_box(ui, &mut f.rows, 54.0);
                    });
                    row_with(ui, crate::i18n::tr("Columns"), |ui| {
                        count_box(ui, &mut f.cols, 54.0);
                    });
                    bold(ui, crate::i18n::tr("Spacing ..."));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.use_offset, false, crate::i18n::tr("Gap"));
                        ui.radio_value(&mut f.use_offset, true, crate::i18n::tr("Offset"));
                    });
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.dx, 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.dy, 54.0, "");
                    });
                    bold(ui, crate::i18n::tr("Row/Column Displacement ..."));
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.shift[0], 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.shift[1], 54.0, "");
                    });
                    ui.checkbox(&mut f.group, crate::i18n::tr("Group Copies"));
                });
            }
            Xform::CircularCopy => {
                apply_label = Some(crate::i18n::tr("Copy"));
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Rotation Center"));
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut f.dx, 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut f.dy, 54.0, "");
                    });
                    ui.checkbox(&mut f.rotate_copies, crate::i18n::tr("Rotate Copies"));
                    row_with(ui, crate::i18n::tr("Number of items"), |ui| {
                        count_box(ui, &mut f.count, 54.0);
                    });
                    bold(ui, crate::i18n::tr("Angle ..."));
                    ui.horizontal(|ui| {
                        let before = f.total_angle;
                        ui.radio_value(&mut f.total_angle, true, crate::i18n::tr("Total Angle"));
                        ui.radio_value(&mut f.total_angle, false, crate::i18n::tr("Step Angle"));
                        if before != f.total_angle {
                            let n = f.count.max(1) as f64;
                            f.angle = if f.total_angle { f.angle * n } else { f.angle / n };
                        }
                    });
                    labelled(ui, "", 2.0, &mut f.angle, 60.0, crate::i18n::tr("degrees"));
                    ui.checkbox(&mut f.group, crate::i18n::tr("Group Copies"));
                });
            }
            Xform::CopyAlong => {
                apply_label = Some(crate::i18n::tr("Copy"));
                card(ui, |ui| {
                    ui.radio_value(&mut f.copy_circles, false, crate::i18n::tr("Copy object"));
                    note(ui, crate::i18n::tr("Select the object to copy, then one vector to copy it along."));
                    ui.radio_value(&mut f.copy_circles, true, crate::i18n::tr("Copy Circles"));
                    ui.add_enabled_ui(f.copy_circles, |ui| {
                        labelled(ui, crate::i18n::tr("Diameter"), 60.0, &mut f.size, 54.0, unit);
                    });
                    bold(ui, crate::i18n::tr("Specify ..."));
                    ui.radio_value(&mut f.by_number, false, crate::i18n::tr("Distance between copies"));
                    ui.add_enabled_ui(!f.by_number, |ui| {
                        labelled(ui, "", 14.0, &mut f.distance, 54.0, unit);
                        ui.checkbox(&mut f.even, crate::i18n::tr("Force even spacing"));
                    });
                    ui.radio_value(&mut f.by_number, true, crate::i18n::tr("Number of copies"));
                    ui.add_enabled_ui(f.by_number, |ui| {
                        count_box(ui, &mut f.count, 54.0);
                    });
                    ui.separator();
                    ui.checkbox(&mut f.align, crate::i18n::tr("Align objects to curve"));
                    ui.checkbox(&mut f.new_layer, crate::i18n::tr("Create copies on new layer"));
                    ui.checkbox(&mut f.reverse, crate::i18n::tr("Reverse direction"));
                });
            }
            Xform::Join => {
                apply_label = Some(crate::i18n::tr("Join"));
                let (before, after) = self.join_counts(f.distance);
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Selected vectors:"));
                    ui.label(tf!("Closed: {}", before[0]));
                    ui.label(tf!("Open: {}", before[1]));
                    ui.add_space(4.0);
                    bold(ui, crate::i18n::tr("Tolerance:"));
                    labelled(ui, "", 2.0, &mut f.distance, 60.0, unit);
                    ui.add_space(4.0);
                    bold(ui, crate::i18n::tr("Vectors after"));
                    ui.label(tf!("Closed: {}", after[0]));
                    ui.label(tf!("Open: {}", after[1]));
                });
            }
            Xform::Fillet => {
                apply_label = None;
                card(ui, |ui| {
                    labelled(ui, crate::i18n::tr("Fillet / Tool"), 62.0, &mut f.distance, 54.0, unit);
                    note(ui, crate::i18n::tr("Click on an angled corner to change it into a circular fillet. Undo removes a fillet."));
                    bold(ui, crate::i18n::tr("Fillet Type"));
                    ui.radio_value(&mut f.fillet, FilletKind::Normal, crate::i18n::tr("Normal fillet"));
                    ui.radio_value(&mut f.fillet, FilletKind::DogBone, crate::i18n::tr("'Dog-Bone' Fillet"));
                    note(ui, crate::i18n::tr("Clears inside corners so slotted pieces fit together."));
                    ui.radio_value(&mut f.fillet, FilletKind::TBone, crate::i18n::tr("'T-Bone' Fillet"));
                    note(ui, crate::i18n::tr("The same clearance, for when the slot is the same size as the tool."));
                });
            }
            Xform::Trim => {
                apply_label = None;
                card(ui, |ui| {
                    ui.label(crate::i18n::tr("To trim overlapping vectors click on sections of vectors you want to delete, or hold the button and sweep across them."));
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("To delete closed non-overlapping vectors hold down the Shift key when clicking."));
                    ui.add_space(4.0);
                    ui.checkbox(&mut f.rejoin, crate::i18n::tr("Rejoin trimmed sections automatically when form is closed."));
                    if !self.doc.constraints.is_empty() {
                        ui.add_space(4.0);
                        note(ui, crate::i18n::tr("Rules stay on the edges they were on. A dimension on a shortened line takes its new length, and rules on a removed part are removed."));
                    }
                });
            }
            Xform::Validate => {
                apply_label = None;
                let zero = self.issues.iter().filter(|i| i.kind == crate::edit::IssueKind::ZeroLength).count();
                card(ui, |ui| {
                    ui.label(crate::i18n::tr("If there is no selection, all visible vectors on visible layers will be validated."));
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("The following issues will be marked:"));
                    for (name, colour, kind) in [
                        (crate::i18n::tr("Zero-length"), theme::RAPID, crate::edit::IssueKind::ZeroLength),
                        (crate::i18n::tr("Overlaps"), theme::WARNING, crate::edit::IssueKind::Overlap),
                        (crate::i18n::tr("Intersections"), theme::CUT, crate::edit::IssueKind::Intersection),
                    ] {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            ui.painter().rect_filled(r.shrink(2.0), 2.0, colour);
                            let n = self.issues.iter().filter(|i| i.kind == kind).count();
                            ui.label(format!("{name}: {n}"));
                        });
                    }
                    ui.add_space(4.0);
                    if wide(ui, crate::i18n::tr("Search All"), ui.available_width() - 2.0).clicked() {
                        self.validate();
                    }
                });
                if ui.add_enabled(zero > 0, egui::Button::new(crate::i18n::tr("Fix Zero-length Spans"))).clicked() {
                    self.fix_zero_length();
                }
            }
            Xform::Dimension => {
                apply_label = None;
                card(ui, |ui| {
                    ui.radio_value(&mut f.dim, DimKind::Length, crate::i18n::tr("Length Dimension"));
                    ui.radio_value(&mut f.dim, DimKind::Vertical, crate::i18n::tr("Vertical Dimension"));
                    ui.radio_value(&mut f.dim, DimKind::Horizontal, crate::i18n::tr("Horizontal Dimension"));
                });
                card(ui, |ui| {
                    labelled(ui, crate::i18n::tr("Text"), 46.0, &mut f.size, 54.0, unit);
                    row_with(ui, crate::i18n::tr("Decimal"), |ui| {
                        count_box(ui, &mut f.decimals, 54.0);
                    });
                    labelled(ui, crate::i18n::tr("Offset:"), 46.0, &mut f.distance, 54.0, unit);
                    row_with(ui, crate::i18n::tr("Custom Text:"), |ui| {
                        ui.add(TextEdit::singleline(&mut f.text).desired_width(100.0));
                    });
                });
                card(ui, |ui| {
                    ui.label(crate::i18n::tr("Drag on the sheet from the first point to the second."));
                    note(ui, crate::i18n::tr("Dimensions go on a layer named Dimensions."));
                });
            }
            Xform::Nest => {
                apply_label = Some(crate::i18n::tr("Nest"));
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Tool and Clearance Settings"));
                    labelled(ui, crate::i18n::tr("Tool Dia. (D)"), 74.0, &mut f.tool_dia, 54.0, unit);
                    labelled(ui, crate::i18n::tr("Clearance"), 74.0, &mut f.clearance, 54.0, unit);
                    labelled(ui, crate::i18n::tr("Border Gap"), 74.0, &mut f.border, 54.0, unit);
                    ui.separator();
                    bold(ui, crate::i18n::tr("Sheet Options"));
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Nest from"));
                        ui.add_space(14.0);
                        anchor_grid_with(ui, &mut f.anchor, crate::i18n::tr("Nest from"), false);
                        if f.anchor == Datum::Centre {
                            f.anchor = Datum::BottomLeft;
                        }
                    });
                    ui.label(crate::i18n::tr("Nest Direction"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut f.along_x, true, crate::i18n::tr("Along X"));
                        ui.radio_value(&mut f.along_x, false, crate::i18n::tr("Along Y"));
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut f.nest_turn, crate::i18n::tr("Allow rotation"));
                        ui.add_enabled_ui(f.nest_turn, |ui| {
                            ui.label(crate::i18n::tr("Angle step"));
                            crate::ui::number(ui, &mut f.nest_step, 44.0);
                            ui.label("°");
                        });
                    });
                    // Copies of the chosen parts, kept with each shape (`nestNumCopies`).
                    // As the original fills the box (`0x1403bdebd`): each chosen shape's count, 1 when it
                    // has none; the box shows it when they all agree and stays empty when they don't.
                    let first = self.selected.first().copied();
                    let counts: Vec<u16> = self.selected.iter().map(|id| self.doc.nest_copies.get(id).copied().unwrap_or(1)).collect();
                    let agreed = counts.windows(2).all(|w| w[0] == w[1]);
                    let shown = counts.first().copied().unwrap_or(1);
                    let mut text = if agreed { shown.to_string() } else { String::new() };
                    let before = text.clone();
                    ui.add_enabled_ui(first.is_some(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Copies of the selected parts"));
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(44.0).hint_text(if agreed { "" } else { "–" }));
                        });
                    });
                    let wanted = if text != before { text.trim().parse::<f64>().ok().map(|v| v.round().clamp(1.0, 999.0) as u16) } else { None };
                    if let (Some(copies), Some(_)) = (wanted, first) {
                        for id in self.selected.clone() {
                            if copies > 1 {
                                self.doc.nest_copies.insert(id, copies);
                            } else {
                                self.doc.nest_copies.remove(&id);
                            }
                        }
                        self.dirty = true;
                    }
                    ui.checkbox(&mut f.part_in_part, crate::i18n::tr("Part in part")).on_hover_text(crate::i18n::tr("Small parts go in the holes of bigger ones."));
                    ui.checkbox(&mut f.by_shape, crate::i18n::tr("Nest by true shape")).on_hover_text(crate::i18n::tr("Parts fit together by their real outline, not their box. Slower."));
                    ui.add_enabled_ui(f.by_shape, |ui| {
                        ui.checkbox(&mut f.nest_mirror, crate::i18n::tr("Mirror parts to find best fit"));
                    });
                    ui.checkbox(&mut f.into_boundary, crate::i18n::tr("Last vector is nest boundary")).on_hover_text(crate::i18n::tr("Select the parts, then the closed outline to nest them in, last."));
                    note(ui, crate::i18n::tr("Parts are packed in rows by their outer box. Those that don't fit go on new sheets."));
                    // As the original's form takes them (`0x1403bd2c0`): none of these below 0.
                    for v in [&mut f.tool_dia, &mut f.clearance, &mut f.border, &mut f.nest_step] {
                        *v = crate::edit::nest_value(*v);
                    }
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
                    bold(ui, crate::i18n::tr("Create Envelope"));
                    ui.label(crate::i18n::tr("Select the object to distort and (optionally) one or two open vectors to define the bottom and top of the initial distortion envelope."));
                    ui.add_space(4.0);
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::BoundingBox, crate::i18n::tr("Bounding Box"));
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::OneCurve, crate::i18n::tr("Above a Single Curve"));
                    ui.radio_value(&mut f.envelope_kind, EnvelopeKind::TwoCurves, crate::i18n::tr("Between Two Curves"));
                    ui.add_space(4.0);
                    let check = self.envelope_check(f.envelope_kind);
                    match &check {
                        Ok(()) => {
                            ui.colored_label(egui::Color32::from_rgb(120, 214, 140), crate::i18n::tr("Selected vectors are valid."));
                        }
                        Err(why) => {
                            ui.colored_label(theme::ERROR, why);
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 76.0).max(0.0));
                        let active = self.envelope.is_some();
                        if ui.add_enabled(check.is_ok() && !active, egui::Button::new(crate::i18n::tr("Apply")).min_size(vec2(74.0, 20.0))).clicked() {
                            self.apply_envelope(f.envelope_kind);
                        }
                    });
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Edit Envelope"));
                    let active = self.envelope.is_some();
                    let handles = self.envelope.as_ref().is_some_and(|e| e.has_handles());
                    ui.label(if handles {
                        crate::i18n::tr("Drag the four corners on the sheet to reshape the object.")
                    } else if active {
                        crate::i18n::tr("The object follows the curves you chose.")
                    } else {
                        crate::i18n::tr("Apply an envelope to edit it here.")
                    });
                    if ui.add_enabled(active, egui::Button::new(crate::i18n::tr("Bake Distortion")).min_size(vec2(ui.available_width() - 2.0, 20.0))).clicked() {
                        self.bake_envelope();
                    }
                });
            }
            Xform::TextOnCurve => {
                card(ui, |ui| {
                    note(ui, crate::i18n::tr("Select a SINGLE LINE of text and the curve to wrap it onto."));
                    bold(ui, crate::i18n::tr("Text Spacing"));
                    ui.add(egui::Slider::new(&mut f.spacing, 0.5..=3.0).show_value(true));
                    bold(ui, crate::i18n::tr("Text Position"));
                    labelled(ui, crate::i18n::tr("Offset Distance"), 86.0, &mut f.distance, 50.0, unit);
                    note(ui, crate::i18n::tr("A positive offset sits above the curve, a negative one below."));
                    bold(ui, crate::i18n::tr("Text Alignment"));
                    ui.radio_value(&mut f.anchor, Datum::BottomLeft, crate::i18n::tr("Left"));
                    ui.radio_value(&mut f.anchor, Datum::Centre, crate::i18n::tr("Middle"));
                    ui.radio_value(&mut f.anchor, Datum::BottomRight, crate::i18n::tr("Right"));
                    ui.separator();
                    ui.radio_value(&mut f.align, true, crate::i18n::tr("Align to Curve"));
                    ui.radio_value(&mut f.align, false, crate::i18n::tr("Keep Vertical"));
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
                    if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
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
            bold(ui, crate::i18n::tr("Shapes on the sheet"));
            note(ui, crate::i18n::tr("Click a line to select all the shapes in it."));
            let groups = self.similar_groups(f.tolerance, f.layer_only);
            if groups.is_empty() {
                note(ui, crate::i18n::tr("Nothing drawn yet."));
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
            labelled(ui, crate::i18n::tr("Alike within"), 66.0, &mut f.tolerance, 54.0, unit);
            f.tolerance = f.tolerance.max(0.0);
            ui.checkbox(&mut f.layer_only, crate::i18n::tr("Only the active layer"));
        });
        let selected: Vec<Class> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).filter_map(classify).collect();
        let circles = selected.iter().filter(|c| matches!(c, Class::Circle { .. })).count();
        let others = selected.len() - circles;
        card(ui, |ui| {
            bold(ui, &tf!("Change the {} selected", selected.len()));
            if selected.is_empty() {
                note(ui, crate::i18n::tr("Select shapes on the sheet, or pick a line above."));
                return;
            }
            if circles > 0 {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_d, "");
                    labelled(ui, crate::i18n::tr("Diameter"), 52.0, &mut f.bulk_d, 60.0, unit);
                });
            }
            if others > 0 {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_w, "");
                    labelled(ui, crate::i18n::tr("Width"), 52.0, &mut f.bulk_w, 60.0, unit);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.set_h, "");
                    labelled(ui, crate::i18n::tr("Height"), 52.0, &mut f.bulk_h, 60.0, unit);
                });
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Keep in place"));
                ui.add_space(8.0);
                anchor_grid(ui, &mut f.anchor, crate::i18n::tr("Keep in place"));
            });
            note(ui, crate::i18n::tr("Each shape changes around its own point, so holes stay where they are."));
            ui.add_space(4.0);
            if wide(ui, crate::i18n::tr("Apply to All Selected"), ui.available_width() - 2.0).clicked() {
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
            // One card for the material: the slab with its zero, then its size and the zero's place.
            egui::Frame::new().fill(theme::CARD).stroke(egui::Stroke::new(1.0, theme::LINE)).corner_radius(8.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(width - 40.0);
                ui.horizontal(|ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(72.0, 50.0), Sense::click());
                    wood_block(ui.painter(), r);
                    let (job, zero) = (&self.doc.job, self.doc.job.origin());
                    let (u, v) = (((zero[0] / job.width) as f32).clamp(0.0, 1.0), ((zero[1] / job.height) as f32).clamp(0.0, 1.0));
                    crate::ui::zero_marker(ui.painter(), crate::ui::wood_top_point(r, u, v));
                    label_widget(&resp, crate::i18n::tr("X/Y zero"));
                    if resp.on_hover_text(crate::i18n::tr("Where the machine's X0 Y0 is. Click to change it.")).clicked() {
                        self.right = RightMode::Material(Box::new(self.doc.clone()));
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        bold(ui, crate::i18n::tr("Material"));
                        let size = format!("{} \u{d7} {} \u{d7} {} {}", toolpath::trim(job.width), toolpath::trim(job.height), toolpath::trim(job.thickness), job.unit());
                        ui.label(RichText::new(size).size(12.0).color(theme::TEXT));
                        let text = format!("X0 Y0: {}, {}", toolpath::trim(zero[0]), toolpath::trim(zero[1]));
                        ui.label(RichText::new(text).size(11.0).color(theme::MUTED));
                        if wide(ui, crate::i18n::tr("Set ..."), 60.0).clicked() {
                            self.right = RightMode::Material(Box::new(self.doc.clone()));
                        }
                    });
                });
            });
            ui.add_space(2.0);
            let (safe, home) = (self.doc.machine.safe_z, self.doc.machine.home);
            note(ui, &tf!("Safe Z: {}   Home: X{} Y{} Z{}", toolpath::trim(safe), toolpath::trim(home[0]), toolpath::trim(home[1]), toolpath::trim(home[2])));
            ui.add_space(6.0);
            if self.section_open(ui, crate::i18n::tr("New Toolpath")) {
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [
                        (Icon::TpProfile, crate::i18n::tr("Profile toolpath"), OpKind::Profile),
                        (Icon::TpPocket, crate::i18n::tr("Pocket toolpath"), OpKind::Pocket),
                        (Icon::TpDrill, crate::i18n::tr("Drill toolpath"), OpKind::Drill),
                        (Icon::TpEngrave, crate::i18n::tr("Quick engrave toolpath"), OpKind::Engrave),
                        (Icon::TpVCarve, crate::i18n::tr("V-Carve toolpath"), OpKind::VCarve),
                    ] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [
                        (Icon::TpPrism, crate::i18n::tr("Prism carve toolpath"), OpKind::Prism),
                        (Icon::TpTexture, crate::i18n::tr("Texture toolpath"), OpKind::Texture),
                        (Icon::TpMoulding, crate::i18n::tr("Moulding toolpath"), OpKind::Moulding),
                        (Icon::TpRough, crate::i18n::tr("3D roughing toolpath"), OpKind::Rough3D),
                        (Icon::TpFinish, crate::i18n::tr("3D finishing toolpath"), OpKind::Finish3D),
                    ] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
                icon_row(ui, |ui| {
                    for (icon, tip, kind) in [(Icon::TpFlute, crate::i18n::tr("Fluting toolpath"), OpKind::Flute), (Icon::TpChamfer, crate::i18n::tr("Chamfer toolpath"), OpKind::Chamfer)] {
                        if icon_button(ui, icon, tip, true, false).clicked() {
                            self.start_op(kind);
                        }
                    }
                });
            }
            let some = self.selected_tp.is_some();
            let any = !self.doc.toolpaths.is_empty();
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Tools, crate::i18n::tr("Tool list"), true, false).clicked() {
                    self.open_tool_list(None);
                }
                if icon_button(ui, Icon::Edit, crate::i18n::tr("Edit toolpath"), some, false).clicked() {
                    self.edit_toolpath();
                }
                if icon_button(ui, Icon::Duplicate, crate::i18n::tr("Copy toolpath"), some, false).clicked() {
                    self.duplicate_toolpath();
                }
                if icon_button(ui, Icon::Trash, crate::i18n::tr("Delete toolpath"), some, false).clicked() {
                    self.delete_toolpath();
                }
                if icon_button(ui, Icon::Clock, crate::i18n::tr("Machining time"), any, false).clicked() {
                    self.time_open = true;
                }
            });
            icon_row(ui, |ui| {
                if icon_button(ui, Icon::Preview, crate::i18n::tr("Preview toolpaths"), true, false).clicked() {
                    self.right = RightMode::Preview;
                    self.view3d = true;
                }
                if icon_button(ui, Icon::Recalc, crate::i18n::tr("Recalculate all toolpaths"), any, false).clicked() {
                    self.recalculate_all();
                }
                if icon_button(ui, Icon::SaveTp, crate::i18n::tr("Save toolpaths"), true, false).clicked() {
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
                title_card(ui, Icon::TpProfile, crate::i18n::tr("Material Setup"));
                card(ui, |ui| {
                    labelled(ui, crate::i18n::tr("Thickness"), 60.0, &mut self.doc.job.thickness, 60.0, unit);
                    if labelled(ui, crate::i18n::tr("Sacrificial board"), 96.0, &mut self.doc.job.spoil_board, 60.0, unit) {
                        self.doc.job.spoil_board = self.doc.job.spoil_board.max(0.0);
                    }
                    note(ui, crate::i18n::tr("Cutting into it brings no warning; going past it does."));
                });
                card(ui, |ui| {
                    bold(ui, "XY");
                    ui.horizontal(|ui| {
                        anchor_grid(ui, &mut self.doc.job.datum, crate::i18n::tr("XY datum"));
                        ui.vertical(|ui| {
                            ui.checkbox(&mut self.doc.job.use_offset, crate::i18n::tr("Use Offset"));
                            ui.add_enabled_ui(self.doc.job.use_offset, |ui| {
                                labelled(ui, "X:", 14.0, &mut self.doc.job.offset[0], 54.0, "");
                                labelled(ui, "Y:", 14.0, &mut self.doc.job.offset[1], 54.0, "");
                            });
                        });
                    });
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Z-Zero"));
                    if ui.radio(!self.doc.job.z_on_bed, crate::i18n::tr("Material Surface")).clicked() {
                        self.doc.job.z_on_bed = false;
                    }
                    if ui.radio(self.doc.job.z_on_bed, crate::i18n::tr("Machine Bed")).clicked() {
                        self.doc.job.z_on_bed = true;
                    }
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Model Position in Material"));
                    let mut changed = false;
                    let above = self.doc.machine.gap_below.is_none();
                    if ui.radio(above, crate::i18n::tr("Gap Above Model")).clicked() {
                        self.doc.machine.gap_below = None;
                        changed = true;
                    }
                    ui.add_enabled_ui(above, |ui| {
                        changed |= labelled(ui, "", 14.0, &mut self.doc.machine.gap_above, 54.0, unit);
                    });
                    if ui.radio(!above, crate::i18n::tr("Gap Below Model")).clicked() {
                        self.doc.machine.gap_below = Some(0.0);
                        changed = true;
                    }
                    if let Some(g) = &mut self.doc.machine.gap_below {
                        changed |= labelled(ui, "", 14.0, g, 54.0, unit);
                    }
                    let high = self.model.as_ref().map_or(0.0, |m| m.max() as f64);
                    note(ui, &tf!("Model Thickness  {high:.3} {unit}"));
                    if changed {
                        self.bump_surface();
                    }
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Rapid Z Gaps above Material"));
                    labelled(ui, crate::i18n::tr("Clearance (Z1)"), 84.0, &mut self.doc.machine.safe_z, 54.0, unit);
                    labelled(ui, crate::i18n::tr("Plunge (Z2)"), 84.0, &mut self.doc.machine.plunge_gap, 54.0, unit);
                });
                card(ui, |ui| {
                    bold(ui, crate::i18n::tr("Home / Start Position"));
                    ui.horizontal(|ui| {
                        labelled(ui, "X:", 14.0, &mut self.doc.machine.home[0], 54.0, "");
                        labelled(ui, "Y:", 14.0, &mut self.doc.machine.home[1], 54.0, "");
                    });
                    labelled(ui, crate::i18n::tr("Z Gap above Material"), 118.0, &mut self.doc.machine.home[2], 54.0, "");
                });
                ui.horizontal(|ui| {
                    if primary(ui, "OK", 74.0).clicked() {
                        close = Some(true);
                    }
                    ui.add_space((ui.available_width() - 80.0).max(0.0));
                    if wide(ui, crate::i18n::tr("Cancel"), 74.0).clicked() {
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
                self.material_accepted(&backup);
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
        let before = self.output_choices();
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            title_card(ui, Icon::SaveTp, crate::i18n::tr("Save Toolpaths"));
            card(ui, |ui| {
                if ui.checkbox(&mut self.one_file, crate::i18n::tr("Output all visible toolpaths to one file")).changed() {
                    crate::pieces::save_one_file(self.one_file);
                }
                self.piece_chooser(ui);
                ui.checkbox(&mut self.output_arcs, crate::i18n::tr("Write curves as arcs (G2 / G3)")).on_hover_text(crate::i18n::tr("Smaller files and smoother curves. Try it in the air on your machine first."));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.corner_slow, crate::i18n::tr("Slow down in tight corners")).on_hover_text(crate::i18n::tr("Near a sharp turn the cutter takes a wider bite; other CAM programs lower the rate there. Not in the original."));
                    ui.add_enabled_ui(self.corner_slow, |ui| {
                        let mut pct = self.corner_share * 100.0;
                        crate::ui::number(ui, &mut pct, 40.0);
                        self.corner_share = (pct / 100.0).clamp(0.05, 1.0);
                        ui.label("%");
                    });
                });
                if crate::toolpath::post_takes_blend(self.post_index) {
                    ui.checkbox(&mut self.blend, crate::i18n::tr("Corner tolerance for LinuxCNC (G64 P)")).on_hover_text(crate::i18n::tr("Without it, LinuxCNC may round corners as far as it likes to keep its speed. Not in the original."));
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.add_enabled_ui(self.blend, |ui| {
                            crate::ui::number(ui, &mut self.blend_mm, 50.0);
                            self.blend_mm = self.blend_mm.clamp(0.001, 1.0);
                            ui.label("mm");
                        });
                    });
                }
                if self.output_choices() != before {
                    self.output_choices().save();
                }
                ui.add_space(6.0);
                bold(ui, crate::i18n::tr("Toolpaths to be saved ..."));
                let names: Vec<&str> = self
                    .doc
                    .toolpaths
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| if self.one_file { t.visible } else { self.selected_tp == Some(*i) })
                    .filter(|(_, t)| self.save_piece.is_none_or(|piece| self.doc.vectors.iter().any(|v| v.piece == Some(piece) && t.op.source.contains(&v.id))))
                    .map(|(_, t)| t.op.name.as_str())
                    .collect();
                if names.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("No Toolpaths")).color(theme::MUTED));
                }
                for n in &names {
                    ui.label(*n);
                }
                ui.add_space(6.0);
                ui.separator();
                bold(ui, crate::i18n::tr("Post Processor"));
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
                    note(ui, crate::i18n::tr("Laid out like your own PlanetCNC file, in millimetres."));
                }
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Load .pp Files ...")).clicked() {
                        if let Some(files) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Post processor"), &["pp", "PP"]).pick_files() {
                            self.report_loaded_posts(files);
                        }
                    }
                    if ui.button(crate::i18n::tr("Load a Folder ...")).clicked() {
                        if let Some(dir) = crate::platform::FileDialog::new().pick_folder() {
                            let files = App::post_files_in(&dir);
                            self.report_loaded_posts(files);
                        }
                    }
                });
                if let Some(def) = self.post_index.checked_sub(POSTS.len()).and_then(|k| self.loaded_posts.get(k)) {
                    let unit = match def.metric {
                        Some(true) => crate::i18n::tr("millimetres").to_string(),
                        Some(false) => crate::i18n::tr("inches").to_string(),
                        None => tf!("the job's unit ({})", self.doc.job.unit()),
                    };
                    let mut lines = vec![tf!("Writes .{} files in {unit}.", def.extension), if !self.output_arcs {
                        crate::i18n::tr("Arcs are written as short straight lines.")
                    } else if def.can_write_arcs() {
                        crate::i18n::tr("Curves are written as arcs (G2 / G3).")
                    } else {
                        crate::i18n::tr("This file doesn't say how to write arcs, so curves stay short straight lines.")
                    }.to_string()];
                    lines.extend(def.notes.iter().cloned());
                    for line in &lines {
                        note(ui, line);
                    }
                    if ui.button(crate::i18n::tr("Remove This One")).clicked() {
                        self.remove_selected_post();
                    }
                }
                ui.add_space(4.0);
                let can = !names.is_empty();
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Save Toolpath(s) ...")).color(theme::ON_ACCENT).strong()).fill(theme::ACCENT).min_size(vec2(ui.available_width() - 2.0, 24.0))).clicked() {
                    self.save_gcode_dialog();
                }
            });
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 80.0).max(0.0));
                if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                    self.right = RightMode::List;
                }
            });
        });
    }

    /// Loads post processor files and says plainly how it went.
    pub(crate) fn report_loaded_posts(&mut self, files: Vec<std::path::PathBuf>) {
        if files.is_empty() {
            self.message = Some(crate::i18n::tr("That folder has no .pp files.").into());
            return;
        }
        let first_new = POSTS.len() + self.loaded_posts.len();
        let (added, failed) = self.load_posts(files);
        if added > 0 {
            self.post_index = first_new;
            self.remember_posts();
        }
        self.status = tf!("Loaded {added} post processor(s).");
        if !failed.is_empty() {
            self.message = Some(tf!("Loaded {added}. These couldn't be read:\n{}", failed.join("\n")));
        }
    }

    pub fn save_gcode_dialog(&mut self) {
        let only = if self.one_file { None } else { self.selected_tp };
        if let Some(piece) = self.save_piece {
            self.save_piece_dialog(piece, only);
            return;
        }
        if self.gcode_text(only).is_none() {
            self.message = Some(crate::i18n::tr("Tick at least one toolpath in the list to save it.").into());
            return;
        }
        if let Some(problem) = self.output_problem(only) {
            self.message = Some(problem);
            return;
        }
        if !self.checks_accepted && crate::checks::check_all(self, only).iter().any(|f| f.level == crate::checks::Level::Danger) {
            self.checks_for_save = Some(only);
            self.checks_open = true;
            return;
        }
        let ext = self.post_extension(self.post_index);
        // Called after the first toolpath going in the file.
        let first = self.doc.toolpaths.iter().enumerate().find(|(i, t)| only.map_or(t.visible && self.doc.toolpath_on_side(t), |o| o == *i)).map_or("", |(_, t)| t.op.name.as_str());
        let name = format!("{}.{ext}", crate::pieces::file_stem_of(first));
        if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Machine file"), &[ext.as_str()]).set_file_name(name).save_file() {
            let path = crate::pieces::with_extension(path, &ext);
            let stem = path.file_stem().map_or_else(|| "toolpaths".to_string(), |s| s.to_string_lossy().to_string());
            let text = self.gcode_text_named(only, &stem).unwrap_or_default();
            let bytes = if self.is_planetcnc() { crate::planetcnc::file_bytes(&text) } else { text.into_bytes() };
            let written = crate::platform::write(&path, bytes);
            crate::pieces::report_written(self, &path, written);
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
                                bold(ui, crate::i18n::tr("Cut"));
                                ui.radio_value(&mut op.climb, true, crate::i18n::tr("Climb"));
                                ui.radio_value(&mut op.climb, false, crate::i18n::tr("Conventional"));
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
                        if ui.checkbox(&mut by_selection, crate::i18n::tr("Use Vector Selection Order")).changed() {
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
                    if primary(ui, crate::i18n::tr("Calculate"), 92.0).clicked() {
                        action = 1;
                    }
                    ui.add_space((ui.available_width() - 80.0).max(0.0));
                    if wide(ui, crate::i18n::tr("Close"), 74.0).clicked() {
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
                    bold(ui, crate::i18n::tr("Cutting Depths"));
                    labelled_formula(ui, crate::i18n::tr("Start Depth (D)"), 106.0, &mut op.start_depth, &mut op.formulas.start_depth, 50.0, unit);
                    labelled_formula(ui, crate::i18n::tr("Cut Depth (C)"), 106.0, &mut op.cut_depth, &mut op.formulas.cut_depth, 50.0, unit);
                    if op.kind != OpKind::Drill {
                        ui.checkbox(&mut self.show_advanced, crate::i18n::tr("Show advanced toolpath options"));
                    }
                });
            }
            OpKind::VCarve => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, crate::i18n::tr("Cutting Depths"));
                    labelled_formula(ui, crate::i18n::tr("Start Depth (D)"), 106.0, &mut op.start_depth, &mut op.formulas.start_depth, 50.0, unit);
                    let mut flat = op.flat_depth > 0.0;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut flat, crate::i18n::tr("Flat Depth (F)")).changed() {
                            op.flat_depth = if flat { self.doc.job.thickness * 0.2 } else { 0.0 };
                            op.formulas.flat_depth.clear();
                            if !flat {
                                op.clearance_tool = None;
                            }
                        }
                        ui.add_space(4.0);
                        ui.add_enabled_ui(flat, |ui| {
                            if flat {
                                crate::ui::number_formula(ui, &mut op.flat_depth, &mut op.formulas.flat_depth, 50.0);
                            } else {
                                ui.label(RichText::new(crate::i18n::tr("None")).color(theme::FAINT));
                            }
                            ui.label(unit);
                        });
                    });
                });
            }
            OpKind::Prism => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, crate::i18n::tr("Cutting Depths"));
                    labelled_formula(ui, crate::i18n::tr("Start Depth (D)"), 106.0, &mut op.start_depth, &mut op.formulas.start_depth, 50.0, unit);
                    labelled_formula(ui, crate::i18n::tr("Prism Depth"), 106.0, &mut op.cut_depth, &mut op.formulas.cut_depth, 50.0, unit);
                    ui.vertical_centered(|ui| {
                        if wide(ui, crate::i18n::tr("Set Depth for Full Prism"), 170.0).clicked() {
                            let half = (op.tool.angle.to_radians() / 2.0).tan().max(1e-3);
                            let widest = toolpath::widest_inside(&self.selected_paths());
                            if widest > 0.0 {
                                op.cut_depth = widest / half;
                            } else {
                                self.status = crate::i18n::tr("Select the closed shapes first.").into();
                            }
                        }
                    });
                });
            }
            OpKind::Texture => {
                card(ui, |ui| {
                    labelled_formula(ui, crate::i18n::tr("Start Depth"), 76.0, &mut op.start_depth, &mut op.formulas.start_depth, 56.0, unit);
                });
            }
            OpKind::Flute => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, crate::i18n::tr("Cutting Depths"));
                    labelled_formula(ui, crate::i18n::tr("Start Depth (D)"), 106.0, &mut op.start_depth, &mut op.formulas.start_depth, 50.0, unit);
                    labelled_formula(ui, crate::i18n::tr("Flute Depth (F)"), 106.0, &mut op.cut_depth, &mut op.formulas.cut_depth, 50.0, unit);
                });
            }
            OpKind::Chamfer => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    bold(ui, crate::i18n::tr("Chamfer"));
                    labelled_formula(ui, crate::i18n::tr("Start Depth (D)"), 106.0, &mut op.start_depth, &mut op.formulas.start_depth, 50.0, unit);
                    labelled(ui, crate::i18n::tr("Chamfer Width (W)"), 106.0, &mut op.chamfer_width, 50.0, unit);
                    note(ui, &tf!("Cut depth: {} {unit}", toolpath::trim(toolpath::chamfer_depth(op))));
                    note(ui, crate::i18n::tr("The tip of the V-bit follows the line."));
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
                    bold(ui, crate::i18n::tr("Passes:"));
                    ui.label(if op.kind == OpKind::Rough3D { crate::i18n::tr("Auto").to_string() } else { passes.to_string() });
                    ui.add_space((ui.available_width() - 112.0).max(0.0));
                    if wide(ui, crate::i18n::tr("Edit Passes ..."), 110.0).clicked() {
                        self.open_passes(op);
                    }
                });
            }
            if self.edit_passes {
                ui.separator();
                labelled(ui, crate::i18n::tr("Pass depth"), 66.0, &mut op.tool.pass_depth, 50.0, unit);
                labelled(ui, crate::i18n::tr("Diameter"), 66.0, &mut op.tool.dia, 50.0, unit);
                if matches!(op.tool.kind, ToolKind::VBit | ToolKind::Engraving) {
                    labelled(ui, crate::i18n::tr("Angle"), 66.0, &mut op.tool.angle, 50.0, crate::i18n::tr("degrees"));
                }
                let mut percent = op.tool.stepover * 100.0;
                if labelled(ui, crate::i18n::tr("Stepover"), 66.0, &mut percent, 50.0, "%") {
                    op.tool.stepover = (percent / 100.0).clamp(0.01, 1.0);
                }
                rate_fields(ui, &mut op.tool, unit, 66.0, 50.0);
                labelled(ui, crate::i18n::tr("Spindle"), 66.0, &mut op.tool.rpm, 50.0, "r.p.m");
            }
            if op.kind == OpKind::Engrave {
                ui.separator();
                labelled_formula(ui, crate::i18n::tr("Depth / Pressure"), 94.0, &mut op.cut_depth, &mut op.formulas.cut_depth, 50.0, unit);
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
                            if wide(ui, crate::i18n::tr("Select ..."), 64.0).clicked() {
                                self.open_tool_list(Some(1));
                            }
                        });
                    }
                    None => note(ui, crate::i18n::tr("Not using area clear tool")),
                }
            });
        });
    }

    fn pocket_cards(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        if self.show_advanced {
            self.clearance_card(ui, op, crate::i18n::tr("Use Larger Area Clearance Tool"), true);
        }
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Clear Pocket ..."));
            ui.horizontal(|ui| {
                let mut way = if op.adaptive { 2 } else if op.raster { 1 } else { 0 };
                ui.radio_value(&mut way, 0, crate::i18n::tr("Offset"));
                ui.radio_value(&mut way, 1, crate::i18n::tr("Raster"));
                ui.radio_value(&mut way, 2, crate::i18n::tr("Adaptive")).on_hover_text(crate::i18n::tr("The cutter never bites more than its stepover: gentler on small cutters. From other CAM programs, not in the original."));
                op.raster = way == 1;
                op.adaptive = way == 2;
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Cut Direction"));
                ui.radio_value(&mut op.climb, true, crate::i18n::tr("Climb"));
                ui.radio_value(&mut op.climb, false, crate::i18n::tr("Conventional"));
            });
            ui.add_enabled_ui(op.raster, |ui| {
                labelled(ui, crate::i18n::tr("Raster"), 40.0, &mut op.raster_angle, 50.0, crate::i18n::tr("degrees"));
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Profile"));
                    profile_combo(ui, &mut op.profile_pass, "pocket_profile");
                });
                ui.add_enabled_ui(op.profile_pass != ProfilePass::None, |ui| {
                    labelled(ui, crate::i18n::tr("Raster Allowance"), 96.0, &mut op.raster_allowance, 50.0, unit);
                });
            });
            ui.separator();
            labelled_formula(ui, crate::i18n::tr("Pocket Allowance"), 96.0, &mut op.allowance, &mut op.formulas.allowance, 50.0, unit);
        });
        card(ui, |ui| {
            ui.checkbox(&mut op.ramp, crate::i18n::tr("Ramp Plunge Moves"));
            ui.add_enabled_ui(op.ramp, |ui| {
                labelled(ui, crate::i18n::tr("Distance"), 56.0, &mut op.ramp_distance, 50.0, unit);
            });
        });
    }

    fn vcarve_cards(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        let flat = op.flat_depth > 0.0;
        self.clearance_card(ui, op, crate::i18n::tr("Use Flat Area Clearance Tool"), flat);
        card(ui, |ui| {
            ui.add_enabled_ui(flat && op.clearance_tool.is_some(), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                bold(ui, crate::i18n::tr("Flat Area Clearance ..."));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut op.raster, false, crate::i18n::tr("Offset"));
                    ui.radio_value(&mut op.raster, true, crate::i18n::tr("Raster"));
                });
                labelled(ui, crate::i18n::tr("Raster Angle"), 70.0, &mut op.raster_angle, 50.0, crate::i18n::tr("degrees"));
            });
            let _ = unit;
        });
    }

    fn op_footer(&mut self, ui: &mut Ui, op: &mut Op, unit: &str) {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let m = self.doc.machine;
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 14.0], egui::Label::new(crate::i18n::tr("Safe")));
                ui.label(format!("{} {unit}", toolpath::trim(m.safe_z)));
            });
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 14.0], egui::Label::new(crate::i18n::tr("Home")));
                ui.label(format!("X:{:.2} Y:{:.2} Z:{:.2}", m.home[0], m.home[1], m.home[2]));
            });
            let is_3d = op.kind.uses_model();
            if !is_3d && op.kind != OpKind::Moulding && op.kind != OpKind::Engrave {
                if self.doc.components.is_empty() {
                    ui.add_enabled(false, egui::Checkbox::new(&mut op.project, crate::i18n::tr("Project toolpath onto 3D model"))).on_disabled_hover_text(crate::i18n::tr("Create a 3D shape first"));
                } else {
                    ui.checkbox(&mut op.project, crate::i18n::tr("Project toolpath onto 3D model"));
                }
            }
            if op.kind != OpKind::Moulding {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Vector Selection:"));
                    ui.label(tf!("{} selected", self.selected.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if wide(ui, crate::i18n::tr("Selector ..."), 74.0).on_hover_text(crate::i18n::tr("Selects shapes by kind, size and layer")).clicked() {
                            self.open_selector(op);
                        }
                    });
                });
            } else {
                note(ui, &tf!("Selected vectors: {}", self.selected.len()));
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Name:"));
                ui.add(TextEdit::singleline(&mut op.name).desired_width(f32::INFINITY));
            });
            egui::CollapsingHeader::new(crate::i18n::tr("Notes")).id_salt("toolpath-notes").default_open(!op.notes.is_empty()).show(ui, |ui| {
                ui.add(TextEdit::multiline(&mut op.notes).desired_width(f32::INFINITY).desired_rows(2));
            });
        });
    }

    // ----- Tool list -----

    pub fn open_tool_list(&mut self, target: Option<u8>) {
        self.tool_edit = Some(self.doc.tools.clone());
        self.tool_groups_edit = self.doc.tool_groups.clone();
        self.tool_group_picked = None;
        self.tool_target = target;
        self.tools_open = true;
    }

    pub fn tool_list(&mut self, ctx: &egui::Context) {
        let Some(mut tools) = self.tool_edit.clone() else { return };
        let mut close: Option<bool> = None;
        let mut open = true;
        self.tool_index = self.tool_index.min(tools.len().saturating_sub(1));
        egui::Window::new(crate::i18n::tr("Tool List")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.allocate_ui(vec2(600.0, 470.0), |ui| ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(226.0);
                    egui::Frame::new().fill(theme::INPUT).corner_radius(5).inner_margin(6).show(ui, |ui| {
                        ui.set_width(214.0);
                        let mut dropped: Option<(usize, String)> = None;
                        let mut moved: Option<(String, String)> = None;
                        egui::ScrollArea::vertical().max_height(380.0).id_salt("tools_tree").show(ui, |ui| {
                            // The user's own groups first, each a folder tools and groups can be dragged onto,
                            // with groups inside groups as in the original's tool database (types 11 and 12).
                            let groups = group_paths(&self.tool_groups_edit, &tools);
                            let mut tree = GroupTree { tools: &tools, groups: &groups, index: &mut self.tool_index, picked: &mut self.tool_group_picked, dropped: &mut dropped, moved: &mut moved };
                            for top in groups.iter().filter(|g| !g.contains(GROUP_SEP)) {
                                tree.show(ui, top);
                            }
                            for (metric, group) in [(false, crate::i18n::tr("Imperial Tools")), (true, crate::i18n::tr("Metric Tools"))] {
                                let shown = egui::CollapsingHeader::new(group).default_open(true).show(ui, |ui| {
                                    for (kind, name) in [
                                        (ToolKind::EndMill, crate::i18n::tr("End Mills")),
                                        (ToolKind::BallNose, crate::i18n::tr("Ball Nose")),
                                        (ToolKind::VBit, crate::i18n::tr("V-Bits")),
                                        (ToolKind::Engraving, crate::i18n::tr("Engraving")),
                                        (ToolKind::Drill, crate::i18n::tr("Drills")),
                                    ] {
                                        let ids: Vec<usize> = (0..tools.len()).filter(|i| tools[*i].metric == metric && tools[*i].kind == kind && tools[*i].group.is_empty()).collect();
                                        if ids.is_empty() {
                                            continue;
                                        }
                                        egui::CollapsingHeader::new(name).id_salt((metric, name)).default_open(kind == ToolKind::EndMill || kind == ToolKind::BallNose).show(ui, |ui| {
                                            for i in ids {
                                                let r = ui.selectable_label(self.tool_index == i && self.tool_group_picked.is_none(), tools[i].name.as_str()).interact(egui::Sense::click_and_drag());
                                                r.dnd_set_drag_payload(i);
                                                if r.clicked() {
                                                    self.tool_index = i;
                                                    self.tool_group_picked = None;
                                                }
                                            }
                                        });
                                    }
                                });
                                // Dropped back on the units' folder, a tool leaves its group, and a group goes to the top.
                                if let Some(i) = shown.header_response.dnd_release_payload::<usize>() {
                                    dropped = Some((*i, String::new()));
                                }
                                if let Some(g) = shown.header_response.dnd_release_payload::<GroupDrag>() {
                                    moved = Some((g.0.clone(), String::new()));
                                }
                            }
                        });
                        if let Some((i, group)) = dropped {
                            if let Some(t) = tools.get_mut(i) {
                                t.group = group;
                                self.tool_index = i;
                                self.tool_group_picked = None;
                            }
                        }
                        if let Some((group, into)) = moved {
                            if let Some(now) = move_group(&mut self.tool_groups_edit, &mut tools, &group, &into) {
                                self.tool_group_picked = Some(now);
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if wide(ui, crate::i18n::tr("New ..."), 68.0).clicked() {
                            let mut t = toolpath::default_tools().remove(1);
                            t.name = crate::i18n::tr("New Tool").into();
                            t.number = tools.iter().map(|t| t.number).max().unwrap_or(0) + 1;
                            // With a group chosen, the new tool goes straight into it (the manual's Tool Database page).
                            t.group = self.tool_group_picked.clone().unwrap_or_default();
                            tools.push(t);
                            self.tool_index = tools.len() - 1;
                        }
                        if wide(ui, crate::i18n::tr("Copy ..."), 68.0).clicked() {
                            if let Some(mut t) = tools.get(self.tool_index).cloned() {
                                t.name += crate::i18n::tr(" copy");
                                t.number = tools.iter().map(|t| t.number).max().unwrap_or(0) + 1;
                                tools.push(t);
                                self.tool_index = tools.len() - 1;
                            }
                        }
                        if ui.add_enabled(tools.len() > 1, egui::Button::new(crate::i18n::tr("Delete")).min_size(vec2(68.0, 20.0))).clicked() {
                            tools.remove(self.tool_index);
                            self.tool_index = self.tool_index.min(tools.len() - 1);
                        }
                    });
                    // New Group: a name, then Apply makes the group, which tools are dragged onto.
                    match self.new_tool_group.take() {
                        None => {
                            if wide(ui, crate::i18n::tr("New Group"), 216.0).clicked() {
                                self.new_tool_group = Some(crate::i18n::tr("New Group").into());
                            }
                        }
                        Some(mut name) => {
                            let mut keep = true;
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
                                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(crate::i18n::tr("Apply"))).clicked() {
                                    // An empty group; tools are dragged onto it. With a group chosen, it goes inside that one.
                                    let name = name.trim().replace(GROUP_SEP, "-");
                                    let name = match &self.tool_group_picked {
                                        Some(parent) => format!("{parent}{GROUP_SEP}{name}"),
                                        None => name,
                                    };
                                    if !self.tool_groups_edit.contains(&name) {
                                        self.tool_groups_edit.push(name);
                                    }
                                    keep = false;
                                }
                                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                                    keep = false;
                                }
                            });
                            if keep {
                                self.new_tool_group = Some(name);
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        if wide(ui, crate::i18n::tr("Import ..."), 106.0).clicked() {
                            if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Tool list"), &["json"]).pick_file() {
                                match crate::platform::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Vec<Tool>>(&t).ok()) {
                                    Some(more) => tools.extend(more),
                                    None => self.message = Some(tf!("{} is not a tool list.", path.display())),
                                }
                            }
                        }
                        if wide(ui, crate::i18n::tr("Export ..."), 106.0).clicked() {
                            if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Tool list"), &["json"]).set_file_name("tools.json").save_file() {
                                if let Ok(text) = serde_json::to_string_pretty(&tools) {
                                    if let Err(e) = crate::platform::write(&path, text) {
                                        self.message = Some(tf!("Could not write {}: {e}", path.display()));
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
                    let unit = if t.metric { "mm" } else { crate::i18n::tr("inches") };
                    bold(ui, crate::i18n::tr("Tool Info"));
                    row_with(ui, crate::i18n::tr("Name"), |ui| {
                        ui.add(TextEdit::singleline(&mut t.name).desired_width(280.0));
                    });
                    row_with(ui, crate::i18n::tr("Tool Type"), |ui| {
                        egui::ComboBox::from_id_salt("tool_type").selected_text(kind_name(t.kind)).width(170.0).show_ui(ui, |ui| {
                            for k in [ToolKind::EndMill, ToolKind::BallNose, ToolKind::VBit, ToolKind::Engraving, ToolKind::Drill] {
                                ui.selectable_value(&mut t.kind, k, kind_name(k));
                            }
                        });
                    });
                    ui.label(crate::i18n::tr("Notes"));
                    ui.add(TextEdit::multiline(&mut t.notes).desired_rows(3).desired_width(340.0));
                    ui.add_space(4.0);
                    bold(ui, crate::i18n::tr("Geometry"));
                    ui.horizontal(|ui| {
                        labelled(ui, crate::i18n::tr("Diameter (D)"), 86.0, &mut t.dia, 64.0, "");
                        let before = t.metric;
                        egui::ComboBox::from_id_salt("tool_unit").selected_text(if t.metric { "mm" } else { crate::i18n::tr("inches") }).width(70.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.metric, false, crate::i18n::tr("inches"));
                            ui.selectable_value(&mut t.metric, true, "mm");
                        });
                        if before != t.metric {
                            let wanted = t.metric;
                            t.metric = before;
                            *t = t.in_units(wanted);
                        }
                    });
                    if matches!(t.kind, ToolKind::VBit | ToolKind::Engraving) {
                        labelled(ui, crate::i18n::tr("Included Angle"), 86.0, &mut t.angle, 64.0, crate::i18n::tr("degrees"));
                    }
                    ui.add_space(4.0);
                    bold(ui, crate::i18n::tr("Cutting Parameters"));
                    labelled(ui, crate::i18n::tr("Pass Depth"), 86.0, &mut t.pass_depth, 64.0, unit);
                    labelled(ui, crate::i18n::tr("Flute Length"), 86.0, &mut t.flute_length, 64.0, unit).then_some(()).unwrap_or(());
                    ui.label(egui::RichText::new(crate::i18n::tr("How deep it can cut. 0 if not known.")).size(11.5).color(crate::theme::MUTED));
                    ui.horizontal(|ui| {
                        let mut across = t.dia * t.stepover;
                        if labelled(ui, crate::i18n::tr("Stepover"), 86.0, &mut across, 64.0, unit) && t.dia > 0.0 {
                            t.stepover = (across / t.dia).clamp(0.005, 1.0);
                        }
                        let mut percent = t.stepover * 100.0;
                        if number(ui, &mut percent, 50.0) {
                            t.stepover = (percent / 100.0).clamp(0.005, 1.0);
                        }
                        ui.label("%");
                    });
                    ui.add_space(4.0);
                    bold(ui, crate::i18n::tr("Feeds and Speeds"));
                    labelled(ui, crate::i18n::tr("Spindle Speed"), 86.0, &mut t.rpm, 64.0, "r.p.m");
                    rate_fields(ui, t, unit, 86.0, 64.0);
                    // Starting points from the material and the tool's size.
                    let memory = egui::Id::unique("rates-material");
                    let mut chosen: usize = ui.data(|d| d.get_temp(memory)).unwrap_or(0);
                    let mut flutes: u32 = ui.data(|d| d.get_temp(memory.with("flutes"))).unwrap_or(2);
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("rates-material").selected_text(crate::rates::MATERIALS[chosen].name()).width(130.0).show_ui(ui, |ui| {
                            for (i, m) in crate::rates::MATERIALS.iter().enumerate() {
                                ui.selectable_value(&mut chosen, i, m.name());
                            }
                        });
                        ui.add(egui::DragValue::new(&mut flutes).range(1..=6).suffix(crate::i18n::tr(" flutes")));
                        if ui.button(crate::i18n::tr("Suggest")).on_hover_text(crate::i18n::tr("Feed, plunge and pass depth from the usual chip load for this material and size. Starting points: your tool maker's figures come first.")).clicked() {
                            *t = crate::rates::suggest(t, crate::rates::MATERIALS[chosen], flutes);
                        }
                    });
                    ui.data_mut(|d| {
                        d.insert_temp(memory, chosen);
                        d.insert_temp(memory.with("flutes"), flutes);
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        bold(ui, crate::i18n::tr("Tool Number"));
                        let mut n = t.number as usize;
                        if count_box(ui, &mut n, 50.0) {
                            t.number = n as u32;
                        }
                    });
                });
            }));
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if wide(ui, crate::i18n::tr("Cancel"), 74.0).clicked() {
                    close = Some(false);
                }
                let label = if self.tool_target.is_some() { crate::i18n::tr("Use This Tool") } else { "OK" };
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
                self.doc.tool_groups = std::mem::take(&mut self.tool_groups_edit);
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
        ToolKind::EndMill => crate::i18n::tr("End Mill"),
        ToolKind::BallNose => crate::i18n::tr("Ball Nose"),
        ToolKind::VBit => crate::i18n::tr("V-Bit"),
        ToolKind::Engraving => crate::i18n::tr("Engraving"),
        ToolKind::Drill => crate::i18n::tr("Drill"),
    }
}

fn profile_combo(ui: &mut Ui, value: &mut ProfilePass, salt: &str) {
    let name = |p: ProfilePass| match p {
        ProfilePass::Last => crate::i18n::tr("Last"),
        ProfilePass::First => crate::i18n::tr("First"),
        ProfilePass::None => crate::i18n::tr("No Profile Pass"),
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
        ui.checkbox(&mut op.peck, crate::i18n::tr("Use Peck Drilling"));
        ui.add_enabled_ui(op.peck, |ui| {
            ui.radio_value(&mut op.retract_to_start, true, crate::i18n::tr("Retract above the cutting start depth"));
            ui.radio_value(&mut op.retract_to_start, false, crate::i18n::tr("Retract above the height of the previous pass"));
            labelled(ui, crate::i18n::tr("Retract Gap"), 76.0, &mut op.retract_gap, 50.0, unit);
            ui.label(tf!("Peck Depth (P)   {} {unit}", toolpath::trim(op.tool.pass_depth)));
            note(ui, crate::i18n::tr("Peck depth is the tool's Pass Depth."));
        });
    });
    card(ui, |ui| {
        let mut on = op.dwell > 0.0;
        if ui.checkbox(&mut on, crate::i18n::tr("Dwell at the bottom of each drill pass")).changed() {
            op.dwell = if on { 0.5 } else { 0.0 };
        }
        ui.add_enabled_ui(on, |ui| {
            labelled(ui, crate::i18n::tr("Dwell Time"), 70.0, &mut op.dwell, 50.0, crate::i18n::tr("seconds"));
        });
    });
}

fn engrave_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.radio_value(&mut op.fill, false, crate::i18n::tr("Outline"));
            ui.radio_value(&mut op.fill, true, crate::i18n::tr("Fill"));
        });
        ui.add_enabled_ui(op.fill, |ui| {
            labelled(ui, crate::i18n::tr("Stepover"), 60.0, &mut op.stepover, 50.0, unit);
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.hatch, false, crate::i18n::tr("Offset"));
                ui.radio_value(&mut op.hatch, true, crate::i18n::tr("Hatch"));
            });
            ui.add_enabled_ui(op.hatch, |ui| {
                labelled(ui, crate::i18n::tr("Hatch"), 60.0, &mut op.raster_angle, 50.0, crate::i18n::tr("degrees"));
                ui.checkbox(&mut op.cross_hatch, crate::i18n::tr("Cross Hatch"));
            });
        });
    });
}

fn flute_cards(ui: &mut Ui, op: &mut Op) {
    use crate::toolpath::FluteEnds;
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, crate::i18n::tr("Flute Type"));
        ui.radio_value(&mut op.flute_ends, FluteEnds::Both, crate::i18n::tr("Ramp at both ends"));
        ui.radio_value(&mut op.flute_ends, FluteEnds::Start, crate::i18n::tr("Ramp at the start only"));
        ui.radio_value(&mut op.flute_ends, FluteEnds::End, crate::i18n::tr("Ramp at the end only"));
        let mut share = op.flute_ramp as f32;
        ui.add_space(3.0);
        ui.label(crate::i18n::tr("Ramp length"));
        ui.spacing_mut().slider_width = 120.0;
        if ui.add(egui::Slider::new(&mut share, 1.0..=100.0).step_by(1.0).custom_formatter(|v, _| format!("{v:.0}%"))).changed() {
            op.flute_ramp = share as f64;
        }
    });
}

fn texture_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, crate::i18n::tr("Texture"));
        labelled_formula(ui, crate::i18n::tr("Max. Cut"), 76.0, &mut op.tex_cut[1], &mut op.formulas.texture_depth, 56.0, unit);
        labelled(ui, crate::i18n::tr("Min. Cut"), 76.0, &mut op.tex_cut[0], 56.0, unit);
        labelled_formula(ui, crate::i18n::tr("Max. Length"), 76.0, &mut op.tex_length[1], &mut op.formulas.texture_length, 56.0, unit);
        labelled(ui, crate::i18n::tr("Min. Length"), 76.0, &mut op.tex_length[0], 56.0, unit);
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Max Overlap"));
            ui.add(egui::Slider::new(&mut op.tex_variation, 0.0..=0.9).show_value(false));
            ui.label(format!("{:.0} %", op.tex_variation * 100.0));
        });
        labelled_formula(ui, crate::i18n::tr("Stepover"), 76.0, &mut op.stepover, &mut op.formulas.stepover, 56.0, unit);
        labelled(ui, crate::i18n::tr("Angle"), 76.0, &mut op.raster_angle, 56.0, crate::i18n::tr("degrees"));
    });
    card(ui, |ui| {
        bold(ui, crate::i18n::tr("Boundary Vector Offset"));
        labelled(ui, "", 4.0, &mut op.boundary_offset, 56.0, unit);
        note(ui, crate::i18n::tr("With no selection, the texture covers the whole job."));
    });
}

fn moulding_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        bold(ui, crate::i18n::tr("Drive Rail"));
        note(ui, crate::i18n::tr("Select the drive rails for the toolpath followed by the profile you wish to extrude. You may select multiple rails. The last selected vector is the profile."));
    });
    card(ui, |ui| {
        bold(ui, crate::i18n::tr("Toolpath"));
        labelled(ui, crate::i18n::tr("Gap Above Toolpath"), 110.0, &mut op.gap, 50.0, unit);
        labelled(ui, crate::i18n::tr("Stepover"), 110.0, &mut op.stepover, 50.0, unit);
    });
}

fn model_cards(ui: &mut Ui, op: &mut Op, unit: &str) {
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        bold(ui, crate::i18n::tr("Machining Limit Boundary"));
        ui.radio_value(&mut op.boundary, Boundary::Model, crate::i18n::tr("Model Boundary"));
        ui.radio_value(&mut op.boundary, Boundary::Material, crate::i18n::tr("Material Boundary"));
        ui.radio_value(&mut op.boundary, Boundary::Selected, crate::i18n::tr("Selected Vector(s)"));
        labelled(ui, crate::i18n::tr("Boundary"), 60.0, &mut op.boundary_offset, 56.0, unit);
    });
    if op.kind == OpKind::Rough3D {
        card(ui, |ui| {
            bold(ui, crate::i18n::tr("Machining Allowance"));
            labelled_formula(ui, "", 4.0, &mut op.allowance, &mut op.formulas.allowance, 56.0, unit);
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Roughing Strategy"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.z_level, true, crate::i18n::tr("Z Level"));
                ui.radio_value(&mut op.z_level, false, crate::i18n::tr("3D Raster"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Raster"));
                ui.radio_value(&mut op.raster_along_x, true, crate::i18n::tr("Along X"));
                ui.radio_value(&mut op.raster_along_x, false, crate::i18n::tr("Along Y"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Profile ..."));
                profile_combo(ui, &mut op.profile_pass, "rough_profile");
            });
        });
    } else {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Area Machine Strategy"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut op.raster, false, crate::i18n::tr("Offset"));
                ui.radio_value(&mut op.raster, true, crate::i18n::tr("Raster"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Cut Direction"));
                ui.radio_value(&mut op.climb, true, crate::i18n::tr("Climb"));
                ui.radio_value(&mut op.climb, false, crate::i18n::tr("Conventional"));
            });
            ui.add_enabled_ui(op.raster, |ui| {
                labelled(ui, crate::i18n::tr("Raster"), 54.0, &mut op.raster_angle, 56.0, crate::i18n::tr("degrees"));
            });
            labelled(ui, crate::i18n::tr("Stepover"), 54.0, &mut op.stepover, 56.0, unit);
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
        ui.label(crate::i18n::tr("Rates in"));
        ui.radio_value(&mut tool.per_second, false, format!("{unit}/min"));
        ui.radio_value(&mut tool.per_second, true, format!("{unit}/s"));
    });
    let (k, shown_unit) = if tool.per_second { (60.0, format!("{unit}/s")) } else { (1.0, format!("{unit}/min")) };
    for (name, value) in [(crate::i18n::tr("Feed Rate"), &mut tool.feed), (crate::i18n::tr("Plunge Rate"), &mut tool.plunge)] {
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
            bold(ui, crate::i18n::tr("X/Y Zero"));
            ui.spacing_mut().item_spacing.y = 3.0;
            note(ui, crate::i18n::tr("The machine's X0 Y0, measured from the job's bottom left corner."));
            let mut at = self.doc.job.origin();
            let changed = labelled(ui, crate::i18n::tr("Zero X"), 66.0, &mut at[0], 60.0, unit) | labelled(ui, crate::i18n::tr("Zero Y"), 66.0, &mut at[1], 60.0, unit);
            if changed {
                self.checkpoint();
                self.doc.job.set_origin(at);
            }
            ui.horizontal(|ui| {
                let spot = self.selected_bbox().map(|(lo, hi)| crate::edit::anchor_point(lo, hi, anchor));
                if ui.add_enabled(spot.is_some(), egui::Button::new(crate::i18n::tr("Zero to Anchor"))).on_hover_text(crate::i18n::tr("Puts the zero on the selection's anchor point")).clicked() {
                    if let Some(spot) = spot {
                        self.checkpoint();
                        self.doc.job.set_origin(spot);
                        self.status = crate::i18n::tr("Moved the X/Y zero to the selection's anchor.").into();
                    }
                }
                if ui.button(crate::i18n::tr("Back to Corner")).clicked() {
                    self.checkpoint();
                    self.doc.job.datum = Datum::BottomLeft;
                    self.doc.job.use_offset = false;
                    self.doc.job.offset = [0.0, 0.0];
                }
            });
        });
    }
}

/// Between a group and the group it's in, in a tool's group: `Roughing/Big` is Big inside Roughing.
pub const GROUP_SEP: char = '/';

/// A group being dragged in the tool list.
#[derive(Clone)]
pub struct GroupDrag(pub String);

/// Every group, with the groups holding them, sorted so each comes before what's inside it.
pub fn group_paths(empty: &[String], tools: &[Tool]) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for g in empty.iter().chain(tools.iter().map(|t| &t.group)).filter(|g| !g.is_empty()) {
        let mut at = 0;
        while let Some(k) = g[at..].find(GROUP_SEP) {
            all.push(g[..at + k].to_string());
            at += k + 1;
        }
        all.push(g.clone());
    }
    all.sort();
    all.dedup();
    all
}

/// Puts `group`, with everything inside it, into `into` (the top when empty). Refused into itself
/// or into a group inside it. Gives the group's new name.
pub fn move_group(groups: &mut Vec<String>, tools: &mut [Tool], group: &str, into: &str) -> Option<String> {
    let inside = |g: &str, of: &str| g == of || g.strip_prefix(of).is_some_and(|rest| rest.starts_with(GROUP_SEP));
    if inside(into, group) {
        return None;
    }
    let leaf = group.rsplit(GROUP_SEP).next().unwrap_or(group);
    let now = if into.is_empty() { leaf.to_string() } else { format!("{into}{GROUP_SEP}{leaf}") };
    if now == group {
        return None;
    }
    let rename = |g: &mut String| {
        if inside(g, group) {
            *g = format!("{now}{}", &g[group.len()..]);
        }
    };
    // Groups that only held others keep existing through what's inside them; empty ones are listed.
    for g in groups.iter_mut() {
        rename(g);
    }
    if !groups.iter().any(|g| inside(g, &now)) {
        groups.push(now.clone());
    }
    for t in tools.iter_mut() {
        rename(&mut t.group);
    }
    groups.sort();
    groups.dedup();
    Some(now)
}

/// Draws the user's groups in the tool list, each holding its own groups and tools.
struct GroupTree<'a> {
    tools: &'a [Tool],
    groups: &'a [String],
    index: &'a mut usize,
    picked: &'a mut Option<String>,
    dropped: &'a mut Option<(usize, String)>,
    moved: &'a mut Option<(String, String)>,
}

impl GroupTree<'_> {
    fn show(&mut self, ui: &mut Ui, group: &str) {
        let name = group.rsplit(GROUP_SEP).next().unwrap_or(group);
        let id = ui.make_persistent_id(("tool-group", group));
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
        let (_, header, _) = state
            .show_header(ui, |ui| {
                let r = ui.selectable_label(self.picked.as_deref() == Some(group), name).interact(egui::Sense::click_and_drag());
                r.dnd_set_drag_payload(GroupDrag(group.to_string()));
                if r.clicked() {
                    *self.picked = Some(group.to_string());
                }
                if let Some(i) = r.dnd_release_payload::<usize>() {
                    *self.dropped = Some((*i, group.to_string()));
                }
                if let Some(g) = r.dnd_release_payload::<GroupDrag>() {
                    *self.moved = Some((g.0.clone(), group.to_string()));
                }
            })
            .body(|ui| {
                let prefix = format!("{group}{GROUP_SEP}");
                for inner in self.groups.iter().filter(|g| g.strip_prefix(&prefix).is_some_and(|rest| !rest.contains(GROUP_SEP))) {
                    self.show(ui, inner);
                }
                for i in (0..self.tools.len()).filter(|i| self.tools[*i].group == group) {
                    let r = ui.selectable_label(*self.index == i && self.picked.is_none(), self.tools[i].name.as_str()).interact(egui::Sense::click_and_drag());
                    // Where the tool sits among the groups, as the original's tooltip says.
                    let r = r.on_hover_text(group.replace(GROUP_SEP, " > "));
                    r.dnd_set_drag_payload(i);
                    if r.clicked() {
                        *self.index = i;
                        *self.picked = None;
                    }
                }
            });
        let _ = header;
    }
}
