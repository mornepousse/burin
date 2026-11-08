//! Pieces as things of their own: named and listed, with what cuts them, and cut one at a time.

use crate::app::*;
use crate::i18n::tr;
use crate::icons::Icon;
use crate::toolpath::{self, JobSize};
use crate::ui::icon_button;
use egui::{RichText, TextEdit, Ui};
use std::sync::Arc;

/// A piece as the list shows it.
pub struct PieceRow {
    pub id: u64,
    pub name: String,
    pub shapes: Vec<u64>,
}

/// A name made fit for a file: the characters that can't be in one become underscores.
pub fn file_stem_of(name: &str) -> String {
    let stem: String = name.chars().map(|c| if ":\"/'<>?*|^\\".contains(c) { '_' } else { c }).collect();
    let stem = stem.trim();
    if stem.is_empty() { "toolpaths".to_string() } else { stem.to_string() }
}

/// A file chosen with no extension gets the one the post processor writes.
pub fn with_extension(path: std::path::PathBuf, extension: &str) -> std::path::PathBuf {
    if path.extension().is_some() { path } else { path.with_extension(extension) }
}

/// Says how writing the machine file went.
pub(crate) fn report_written(app: &mut App, path: &std::path::Path, written: std::io::Result<()>) {
    match written {
        Ok(()) => app.status = tf!("Saved toolpaths to {}", path.display()),
        Err(e) => app.message = Some(tf!("Could not write {}. Check that it isn't read only or open in another program, and that the disk has room.\n\n{e}", path.display())),
    }
}

/// Whether all the visible toolpaths go to one file, as it was left last time.
pub fn load_one_file() -> bool {
    if cfg!(test) {
        return true;
    }
    settings_file("output-visible.txt").and_then(|f| crate::platform::read_to_string(f).ok()).is_none_or(|s| s.trim() != "0")
}

pub fn save_one_file(one_file: bool) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = settings_file("output-visible.txt") {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = crate::platform::write(file, if one_file { "1" } else { "0" });
    }
}

impl Doc {
    /// The pieces, in the order they first come in the drawing.
    pub fn pieces(&self) -> Vec<PieceRow> {
        let mut rows: Vec<PieceRow> = Vec::new();
        for v in &self.vectors {
            let Some(id) = v.piece else { continue };
            match rows.iter_mut().find(|r| r.id == id) {
                Some(row) => row.shapes.push(v.id),
                None => {
                    let name = self.piece_names.get(&id).cloned().unwrap_or_else(|| tf!("Piece {n}", n = rows.len() + 1));
                    rows.push(PieceRow { id, name, shapes: vec![v.id] });
                }
            }
        }
        rows
    }

    pub fn piece_name(&self, id: u64) -> String {
        self.pieces().into_iter().find(|r| r.id == id).map_or_else(|| tr("Piece").to_string(), |r| r.name)
    }

    /// The first number that no piece is called by.
    pub fn next_piece_name(&self) -> String {
        let rows = self.pieces();
        let mut n = rows.len() + 1;
        loop {
            let name = tf!("Piece {n}");
            if !rows.iter().any(|r| r.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// A copy in a piece is cut by the same toolpaths as what it was copied from. They then count
    /// as out of date, as for any change of shape, until Recalculate.
    pub fn join_copies_to_toolpaths(&mut self, pairs: &[(u64, u64)]) {
        let in_piece: Vec<(u64, u64)> = pairs.iter().copied().filter(|(_, copy)| self.vectors.iter().any(|v| v.id == *copy && v.piece.is_some())).collect();
        for t in &mut self.toolpaths {
            for (original, copy) in &in_piece {
                if t.op.source.contains(original) && !t.op.source.contains(copy) {
                    t.op.source.push(*copy);
                }
            }
        }
    }
}

impl App {
    /// The toolpaths that cut any of these shapes.
    fn toolpaths_cutting(&self, shapes: &[u64]) -> Vec<usize> {
        self.doc.toolpaths.iter().enumerate().filter(|(_, t)| t.op.source.iter().any(|id| shapes.contains(id))).map(|(i, _)| i).collect()
    }

    /// Recalculates what cuts this piece and is out of date. A toolpath shared with other pieces
    /// is worked out for all of them, as it's one toolpath.
    pub fn recalculate_piece(&mut self, shapes: &[u64]) {
        if self.recalc.is_some() {
            return;
        }
        self.refresh_model();
        let wanted = self.toolpaths_cutting(shapes);
        let tasks: Vec<_> = self.follow_tasks().into_iter().filter(|(i, _)| wanted.contains(i)).collect();
        let background = self.background_recalc;
        self.run_tasks(tasks, crate::recalc::Finish::All { unchanged: 0, no_shapes: 0, warn: true }, background);
    }

    /// These toolpaths worked out again for only this piece's shapes. Nothing in the project changes.
    pub fn piece_toolpaths(&mut self, piece: u64, only: Option<usize>) -> Result<Vec<Toolpath>, String> {
        let shapes: Vec<u64> = self.doc.vectors.iter().filter(|v| v.piece == Some(piece)).map(|v| v.id).collect();
        let lines = self.doc.current_paths();
        let chosen: Vec<&Toolpath> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| only.map_or(t.visible && self.doc.toolpath_on_side(t), |o| o == *i) && !t.op.merged).map(|(_, t)| t).collect();
        let mut ops = Vec::new();
        for t in chosen {
            let mine: Vec<u64> = t.op.source.iter().copied().filter(|id| shapes.contains(id)).collect();
            if mine.is_empty() {
                continue;
            }
            let mut op = t.op.clone();
            op.paths = mine.iter().filter_map(|id| lines.get(id)).flatten().cloned().collect();
            op.source = mine;
            ops.push((op, t.color));
        }
        if ops.is_empty() {
            return Err(tr("No toolpath to be saved cuts this piece.").into());
        }
        if ops.iter().any(|(op, _)| op.kind.uses_model() || op.project) {
            self.refresh_model();
        }
        let machine = self.doc.machine_on_job();
        let job = JobSize { width: self.doc.job.width, height: self.doc.job.height, thickness: self.doc.job.thickness };
        let mut made = Vec::with_capacity(ops.len());
        for (op, color) in ops {
            match toolpath::generate(&op, machine, &job, self.model.as_ref()) {
                Ok(moves) if moves.is_empty() => return Err(tf!("These toolpaths have no moves, so there's nothing to write for them:\n\n{}\n\nUse Recalculate, or edit them to choose their shapes again, then save.", op.name)),
                Ok(moves) => made.push(Toolpath { op, visible: true, color, moves: Arc::new(moves), kept_moves: None, kept_steps: None, face: self.doc.active_face, sheet: self.doc.active_sheet }),
                Err(why) => return Err(format!("{}: {why}", op.name)),
            }
        }
        if let Some(problem) = self.tool_change_problem(&made.iter().map(|t| &t.op.tool).collect::<Vec<_>>()) {
            return Err(problem);
        }
        Ok(made)
    }

    /// The machine file for one piece.
    #[cfg(test)]
    pub fn piece_gcode(&mut self, piece: u64, only: Option<usize>, file_name: &str) -> Result<String, String> {
        let made = self.piece_toolpaths(piece, only)?;
        let refs: Vec<&Toolpath> = made.iter().collect();
        self.gcode_from(&refs, file_name).ok_or_else(|| tr("No toolpath to be saved cuts this piece.").to_string())
    }

    pub(crate) fn save_piece_dialog(&mut self, piece: u64, only: Option<usize>) {
        let made = match self.piece_toolpaths(piece, only) {
            Ok(made) => made,
            Err(why) => {
                self.message = Some(why);
                return;
            }
        };
        let ext = self.post_extension(self.post_index);
        let name = format!("{}.{ext}", file_stem_of(&self.doc.piece_name(piece)));
        let Some(path) = crate::platform::FileDialog::new().add_filter(tr("Machine file"), &[ext.as_str()]).set_file_name(name).save_file() else { return };
        let path = with_extension(path, &ext);
        let stem = path.file_stem().map_or_else(|| "piece".to_string(), |s| s.to_string_lossy().to_string());
        let refs: Vec<&Toolpath> = made.iter().collect();
        let text = self.gcode_from(&refs, &stem).unwrap_or_default();
        let bytes = if self.is_planetcnc() { crate::planetcnc::file_bytes(&text) } else { text.into_bytes() };
        let written = crate::platform::write(&path, bytes);
        report_written(self, &path, written);
    }

    /// Below the layers: each piece, with what cuts it.
    pub(crate) fn pieces_section(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        ui.separator();
        crate::ui::bold(ui, tr("Pieces"));
        let rows = self.doc.pieces();
        if rows.is_empty() {
            ui.label(RichText::new(tr("None yet. Select shapes, then use Make Piece in Edit Objects.")).size(11.5).color(crate::theme::MUTED));
            return;
        }
        for row in rows {
            let whole = row.shapes.iter().all(|id| self.selected.contains(id));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icon_button(ui, Icon::SelectPiece, &tf!("Select {}", row.name), true, whole).clicked() {
                    self.selected = row.shapes.clone();
                    self.handles_on = None;
                }
                let mut name = row.name.clone();
                let room = (ui.available_width() - 62.0).max(50.0);
                if ui.add(TextEdit::singleline(&mut name).desired_width(room)).changed() {
                    self.doc.piece_names.insert(row.id, name);
                }
                if icon_button(ui, Icon::SaveTp, &tf!("Save the toolpaths of {}", row.name), true, false).clicked() {
                    self.save_piece = Some(row.id);
                    self.right = RightMode::Save;
                    self.right_open = true;
                }
                if icon_button(ui, Icon::ReleasePiece, &tf!("Release {}", row.name), true, false).clicked() {
                    self.selected = row.shapes.clone();
                    self.release_piece();
                }
            });
            let cutting = self.toolpaths_cutting(&row.shapes);
            let names: Vec<&str> = cutting.iter().map(|i| self.doc.toolpaths[*i].op.name.as_str()).collect();
            let text = if names.is_empty() { tf!("{} shapes. No toolpath yet.", row.shapes.len()) } else { tf!("{} shapes. Toolpaths: {}", row.shapes.len(), names.join(", ")) };
            ui.add(egui::Label::new(RichText::new(text).size(11.5).color(crate::theme::MUTED)).wrap());
            if !self.all_behind && cutting.iter().any(|i| self.behind.contains(i)) {
                let response = ui.button(tf!("Recalculate for {}", row.name)).on_hover_text(tr("Only the toolpaths that cut this piece and are out of date. A toolpath shared with other pieces is worked out for all of them."));
                if response.clicked() {
                    self.recalculate_piece(&row.shapes);
                }
            }
        }
    }

    /// In the save form: all the pieces, or only one.
    pub(crate) fn piece_chooser(&mut self, ui: &mut Ui) {
        let rows = self.doc.pieces();
        self.save_piece = self.save_piece.filter(|p| rows.iter().any(|r| r.id == *p));
        if rows.is_empty() {
            return;
        }
        crate::ui::bold(ui, tr("Pieces to cut"));
        let shown = self.save_piece.and_then(|p| rows.iter().find(|r| r.id == p)).map_or_else(|| tr("All pieces").to_string(), |r| r.name.clone());
        egui::ComboBox::from_id_salt("save_piece").selected_text(shown).width(ui.available_width() - 16.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut self.save_piece, None, tr("All pieces"));
            for row in &rows {
                ui.selectable_value(&mut self.save_piece, Some(row.id), row.name.clone());
            }
        });
        if self.save_piece.is_some() {
            ui.label(RichText::new(tr("Worked out again for this piece alone as the file is saved. The toolpaths in the project stay as they are.")).size(11.5).color(crate::theme::MUTED));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(file_stem_of("Roue avant 2"), "Roue avant 2");
        assert_eq!(file_stem_of("a/b:c"), "a_b_c");
        assert_eq!(file_stem_of(r#"1:2"3/4'5<6>7?8*9|0^1\2"#), "1_2_3_4_5_6_7_8_9_0_1_2");
        assert_eq!(file_stem_of("Découpe (6 mm) 1.5"), "Découpe (6 mm) 1.5", "only those");
        assert_eq!(file_stem_of("  "), "toolpaths");
    }

    #[test]
    fn a_file_with_no_extension_gets_the_post_processors() {
        use std::path::PathBuf;
        assert_eq!(with_extension(PathBuf::from("/a/plate"), "tap"), PathBuf::from("/a/plate.tap"));
        assert_eq!(with_extension(PathBuf::from("/a/plate.nc"), "tap"), PathBuf::from("/a/plate.nc"));
    }
}
