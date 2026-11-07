//! The menu bar, laid out as in the original, and the small things only it starts.

use crate::app::*;
use crate::edit::Edge;
use crate::geom::{self, Path};
use crate::i18n::tr;
use crate::keys::Action;
use crate::toolpath::OpKind;
use egui::Ui;
use std::path::{Path as FilePath, PathBuf};

/// An entry that does something. Its key, if any, shows when the pointer rests on it.
fn entry(ui: &mut Ui, label: &'static str, enabled: bool, key: Option<String>) -> bool {
    let response = ui.add_enabled(enabled, egui::Button::new(tr(label)));
    let response = match key {
        Some(key) => response.on_hover_text(key),
        None => response,
    };
    if response.clicked() {
        crate::crash::note(label);
        ui.close();
        true
    } else {
        false
    }
}

/// An entry of the original that Burin doesn't do yet. It stays in its place, greyed.
fn later(ui: &mut Ui, label: &'static str) {
    ui.add_enabled(false, egui::Button::new(tr(label))).on_disabled_hover_text(tr("Not in this version"));
}

/// The next name of a numbered project: a number after a hyphen, an underscore or a space goes up by
/// one, with no zeros kept in front, and any other name gets `_1`. Taken names are passed over, up to
/// a hundred of them. After that it gives up, and the project is named by hand.
pub fn incremented(path: &FilePath) -> Option<PathBuf> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let digits = stem.chars().rev().take_while(char::is_ascii_digit).count();
    let (head, number) = stem.split_at(stem.len() - digits);
    let numbered = digits > 0 && head.chars().count() >= 2 && head.ends_with(|c: char| c == '-' || c == '_' || c.is_whitespace());
    let (head, mut n) = if numbered { (head.to_string(), number.parse::<u64>().unwrap_or(0) + 1) } else { (format!("{stem}_"), 1) };
    let extension = path.extension().map_or_else(|| "burin".to_string(), |e| e.to_string_lossy().into_owned());
    for _ in 0..=100 {
        let next = path.with_file_name(format!("{head}{n}.{extension}"));
        if !next.exists() {
            return Some(next);
        }
        n += 1;
    }
    None
}

/// The middle of a line's area, or of its box if it has none.
fn centroid(path: &Path) -> Option<geom::P> {
    let (lo, hi) = geom::bbox(std::slice::from_ref(path))?;
    let n = path.pts.len();
    let (mut area, mut x, mut y) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (path.pts[i], path.pts[(i + 1) % n]);
        let cross = a[0] * b[1] - b[0] * a[1];
        area += cross;
        x += (a[0] + b[0]) * cross;
        y += (a[1] + b[1]) * cross;
    }
    if area.abs() < 1e-12 {
        return Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
    }
    Some([x / (3.0 * area), y / (3.0 * area)])
}

fn area_of(path: &Path) -> f64 {
    let n = path.pts.len();
    (0..n).map(|i| path.pts[i][0] * path.pts[(i + 1) % n][1] - path.pts[(i + 1) % n][0] * path.pts[i][1]).sum::<f64>().abs() / 2.0
}

/// Whether two lines lie on each other: their starts and boxes within `tol`, and their middles within twice that.
fn same_line(a: &Path, b: &Path, tol: f64) -> bool {
    let near = |p: geom::P, q: geom::P, within: f64| (p[0] - q[0]).hypot(p[1] - q[1]) < within;
    let (Some(first_a), Some(first_b)) = (a.pts.first(), b.pts.first()) else { return false };
    if !near(*first_a, *first_b, tol) {
        return false;
    }
    let (Some((lo_a, hi_a)), Some((lo_b, hi_b))) = (geom::bbox(std::slice::from_ref(a)), geom::bbox(std::slice::from_ref(b))) else { return false };
    if !near(lo_a, lo_b, tol) || !near(hi_a, hi_b, tol) {
        return false;
    }
    let (Some(middle_a), Some(middle_b)) = (centroid(a), centroid(b)) else { return false };
    if !near(middle_a, middle_b, 2.0 * tol) {
        return false;
    }
    // One straight span against something with more of them is only the same if neither encloses anything.
    let spans = |p: &Path| p.pts.len() - usize::from(!p.closed);
    if (spans(a) == 1) != (spans(b) == 1) {
        return area_of(a) < 1e-4 && area_of(b) < 1e-4;
    }
    true
}

/// Number text for drawing files, with no more digits than it needs.
fn num(v: f64) -> String {
    let text = format!("{v:.5}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The shapes as an SVG file, at their real size. SVG's Y goes down, and the job's goes up.
pub fn svg_text(paths: &[Path], width: f64, height: f64, unit: &str) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}{unit}\" height=\"{h}{unit}\" viewBox=\"0 0 {w} {h}\">\n",
        w = num(width),
        h = num(height),
    );
    for path in paths {
        let mut d = String::new();
        for (i, p) in path.pts.iter().enumerate() {
            d.push_str(&format!("{}{} {} ", if i == 0 { "M" } else { "L" }, num(p[0]), num(height - p[1])));
        }
        if path.closed {
            d.push('Z');
        }
        s.push_str(&format!("<path d=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"{}\"/>\n", d.trim_end(), num(width.max(height) / 1000.0)));
    }
    s.push_str("</svg>\n");
    s
}

/// The drawing operators both EPS and PDF use, in points from the job's bottom left corner.
fn line_art(paths: &[Path], points_per_unit: f64, moveto: &str, lineto: &str, close: &str, stroke: &str) -> String {
    let mut s = String::new();
    for path in paths {
        for (i, p) in path.pts.iter().enumerate() {
            s.push_str(&format!("{} {} {}\n", num(p[0] * points_per_unit), num(p[1] * points_per_unit), if i == 0 { moveto } else { lineto }));
        }
        if path.closed {
            s.push_str(close);
            s.push('\n');
        }
        s.push_str(stroke);
        s.push('\n');
    }
    s
}

pub fn eps_text(paths: &[Path], width: f64, height: f64, points_per_unit: f64) -> String {
    format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {} {}\n%%EndComments\n0.5 setlinewidth\n{}showpage\n%%EOF\n",
        (width * points_per_unit).ceil() as i64,
        (height * points_per_unit).ceil() as i64,
        line_art(paths, points_per_unit, "newpath moveto", "lineto", "closepath", "stroke"),
    )
}

pub fn pdf_bytes(paths: &[Path], width: f64, height: f64, points_per_unit: f64) -> Vec<u8> {
    let content = format!("0.5 w\n{}", line_art(paths, points_per_unit, "m", "l", "h", "S"));
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R >>", num(width * points_per_unit), num(height * points_per_unit)),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let table = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1));
    out.into_bytes()
}

/// A model's surface as a closed binary STL, with no more than about `most` squares of it.
pub fn stl_bytes(model: &crate::relief::Grid, most: usize) -> Vec<u8> {
    let stride = (((model.nx * model.ny) as f64 / most.max(1) as f64).sqrt().ceil() as usize).max(1);
    let columns: Vec<usize> = (0..model.nx).step_by(stride).collect();
    let rows: Vec<usize> = (0..model.ny).step_by(stride).collect();
    let height = |ix: usize, iy: usize| {
        let z = model.data[ix + iy * model.nx];
        if z.is_finite() { z } else { 0.0 }
    };
    let bottom = model.data.iter().copied().filter(|z| z.is_finite()).fold(0.0f32, f32::min) - model.cell as f32;
    let corner = |ix: usize, iy: usize, z: f32| [(ix as f64 * model.cell) as f32, (iy as f64 * model.cell) as f32, z];
    let mut triangles: Vec<[[f32; 3]; 3]> = Vec::new();
    let mut square = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    };
    for j in 1..rows.len() {
        for i in 1..columns.len() {
            let (x0, x1, y0, y1) = (columns[i - 1], columns[i], rows[j - 1], rows[j]);
            square(corner(x0, y0, height(x0, y0)), corner(x1, y0, height(x1, y0)), corner(x1, y1, height(x1, y1)), corner(x0, y1, height(x0, y1)));
            square(corner(x0, y0, bottom), corner(x0, y1, bottom), corner(x1, y1, bottom), corner(x1, y0, bottom));
        }
    }
    let (first_x, last_x, first_y, last_y) = (columns[0], *columns.last().unwrap_or(&0), rows[0], *rows.last().unwrap_or(&0));
    for i in 1..columns.len() {
        let (x0, x1) = (columns[i - 1], columns[i]);
        square(corner(x0, first_y, bottom), corner(x1, first_y, bottom), corner(x1, first_y, height(x1, first_y)), corner(x0, first_y, height(x0, first_y)));
        square(corner(x1, last_y, bottom), corner(x0, last_y, bottom), corner(x0, last_y, height(x0, last_y)), corner(x1, last_y, height(x1, last_y)));
    }
    for j in 1..rows.len() {
        let (y0, y1) = (rows[j - 1], rows[j]);
        square(corner(last_x, y0, bottom), corner(last_x, y1, bottom), corner(last_x, y1, height(last_x, y1)), corner(last_x, y0, height(last_x, y0)));
        square(corner(first_x, y1, bottom), corner(first_x, y0, bottom), corner(first_x, y0, height(first_x, y0)), corner(first_x, y1, height(first_x, y1)));
    }
    let mut out = Vec::with_capacity(84 + triangles.len() * 50);
    out.extend_from_slice(&[0u8; 80]);
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for triangle in triangles {
        out.extend_from_slice(&[0u8; 12]);
        for point in triangle {
            for v in point {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0u8; 2]);
    }
    out
}

/// A model's heights as a 16 bit grey picture: white is the highest. The first row of a picture is its top.
pub fn grey_picture(model: &crate::relief::Grid) -> image::ImageBuffer<image::Luma<u16>, Vec<u16>> {
    let finite = || model.data.iter().copied().filter(|z| z.is_finite());
    let low = finite().fold(f32::INFINITY, f32::min);
    let high = finite().fold(f32::NEG_INFINITY, f32::max);
    let range = (high - low).max(1e-9);
    image::ImageBuffer::from_fn(model.nx as u32, model.ny as u32, |x, y| {
        let z = model.data[x as usize + (model.ny - 1 - y as usize) * model.nx];
        let level = if z.is_finite() { (z - low) / range } else { 0.0 };
        image::Luma([(level.clamp(0.0, 1.0) * 65535.0) as u16])
    })
}

impl App {
    /// Back to the first screen, with no project open.
    pub fn close_project(&mut self) {
        self.new_file();
        self.left = LeftForm::None;
        self.screen = Screen::Start;
        self.path = None;
        self.dirty = false;
    }

    /// Asks first if there's work to lose, as opening any other project does.
    pub fn open_recent(&mut self, path: PathBuf) {
        self.recent_wanted = Some(path);
        self.leave_for(Leave::Recent);
    }

    pub fn open_recent_wanted(&mut self) {
        if let Some(path) = self.recent_wanted.take() {
            if let Err(why) = self.open_path(path) {
                self.message = Some(why);
            }
        }
    }

    /// Saves under the next number. A project with no file yet is named first.
    pub fn increment_and_save(&mut self) {
        match self.path.clone() {
            Some(path) => match incremented(&path) {
                Some(next) => {
                    if let Err(why) = self.save_to(next) {
                        self.message = Some(why);
                    }
                }
                None => self.dialog_save(true),
            },
            None => self.dialog_save(true),
        }
    }

    fn can_be_selected(&self, v: &Vector) -> bool {
        self.doc.layers.get(v.layer).is_none_or(|l| l.visible && !l.locked)
    }

    /// Open lines, and closed ones with next to no area, which have nothing inside to cut. Groups
    /// and text don't count, nor layers that are hidden or locked.
    pub fn select_open(&mut self) {
        let is_open = |p: &Path| !p.closed || area_of(p) < if p.pts.len() == 2 { 1e-6 } else { 1e-9 };
        self.selected = self
            .doc
            .vectors
            .iter()
            .filter(|v| self.can_be_selected(v) && !v.is_group() && !matches!(v.prim, Prim::Text { .. }))
            .filter(|v| v.paths.iter().any(is_open))
            .map(|v| v.id)
            .collect();
        self.handles_on = None;
        if self.selected.is_empty() {
            self.status = tr("No open vectors in design").into();
        }
    }

    /// What's on the active layer, unless that's hidden or locked. With Shift, it adds to the selection.
    pub fn select_on_active_layer(&mut self, adding: bool) {
        if !adding {
            self.selected.clear();
        }
        let layer = self.doc.active_layer;
        let found: Vec<u64> = self.doc.vectors.iter().filter(|v| v.layer == layer && self.can_be_selected(v) && !self.selected.contains(&v.id)).map(|v| v.id).collect();
        self.selected.extend(found);
        self.handles_on = None;
    }

    /// Shapes that lie on another one. Groups and text don't count, nor layers that are hidden or locked.
    /// The first of each stays out, so deleting the selection leaves it.
    pub fn select_duplicates(&mut self) {
        let tol = if self.doc.job.units == Units::Mm { 0.05 } else { 0.0019685 };
        let shapes: Vec<&Vector> = self
            .doc
            .vectors
            .iter()
            .filter(|v| self.doc.layers.get(v.layer).is_none_or(|l| l.visible && !l.locked))
            .filter(|v| !v.is_group() && !matches!(v.prim, Prim::Text { .. }))
            .collect();
        let mut found = Vec::new();
        for (i, v) in shapes.iter().enumerate() {
            let same = shapes[..i].iter().any(|w| !found.contains(&w.id) && v.paths.len() == w.paths.len() && !v.paths.is_empty() && v.paths.iter().zip(&w.paths).all(|(a, b)| same_line(a, b, tol)));
            if same {
                found.push(v.id);
            }
        }
        if found.is_empty() {
            self.status = tr("No duplicate vectors in design").into();
        }
        self.selected = found;
        self.handles_on = None;
    }

    fn file_units(&self) -> (&'static str, f64) {
        if self.doc.job.units == Units::Mm { ("mm", 72.0 / 25.4) } else { ("in", 72.0) }
    }

    /// The selection, or everything that gets cut if nothing is selected.
    fn paths_to_export(&self) -> Vec<Path> {
        if self.selected.is_empty() {
            self.doc.vectors.iter().filter(|v| !v.construction).flat_map(|v| v.paths.iter().cloned()).collect()
        } else {
            self.selected_paths()
        }
    }

    fn export_drawing_as(&mut self, extension: &str) {
        let paths = self.paths_to_export();
        if paths.is_empty() {
            self.status = tr("There are no shapes to export.").into();
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter(tr("Drawing"), &[extension]).set_file_name(format!("{}.{extension}", tr("Drawing"))).save_file() else { return };
        let (width, height) = (self.doc.job.width, self.doc.job.height);
        let (unit, points) = self.file_units();
        let bytes = match extension {
            "svg" => svg_text(&paths, width, height, unit).into_bytes(),
            "eps" => eps_text(&paths, width, height, points).into_bytes(),
            "pdf" => pdf_bytes(&paths, width, height, points),
            _ => geom::dxf_write(&paths).into_bytes(),
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => self.status = tf!("Exported {}", path.display()),
            Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
        }
    }

    fn model_for_export(&mut self) -> Option<crate::relief::Grid> {
        if self.model_dirty {
            self.refresh_model();
        }
        self.model.clone()
    }

    fn export_stl(&mut self) {
        let Some(model) = self.model_for_export() else { return };
        let Some(path) = rfd::FileDialog::new().add_filter(tr("3D model"), &["stl"]).set_file_name("model.stl").save_file() else { return };
        match std::fs::write(&path, stl_bytes(&model, 1_000_000)) {
            Ok(()) => self.status = tf!("Exported {}", path.display()),
            Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
        }
    }

    fn export_grey(&mut self) {
        let Some(model) = self.model_for_export() else { return };
        let Some(path) = rfd::FileDialog::new().add_filter(tr("Picture"), &["png"]).set_file_name("model.png").save_file() else { return };
        match grey_picture(&model).save(&path) {
            Ok(()) => self.status = tf!("Exported {}", path.display()),
            Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
        }
    }

    /// Shows the folder where the program keeps its settings.
    fn open_settings_folder(&mut self) {
        let Some(folder) = settings_file("x").and_then(|f| f.parent().map(FilePath::to_path_buf)) else { return };
        let _ = std::fs::create_dir_all(&folder);
        let shower = if cfg!(windows) { "explorer" } else { "xdg-open" };
        if let Err(e) = std::process::Command::new(shower).arg(&folder).spawn() {
            self.message = Some(tf!("Could not open {}: {e}", folder.display()));
        }
    }

    fn file_menu(&mut self, ui: &mut Ui, work: bool) {
        let keys = self.keys;
        if entry(ui, "New", true, Some(keys.text(Action::New))) {
            self.leave_for(Leave::New);
        }
        if entry(ui, "Open...", true, Some(keys.text(Action::Open))) {
            self.leave_for(Leave::Open);
        }
        if entry(ui, "Close", work, None) {
            self.leave_for(Leave::ToStart);
        }
        if entry(ui, "Save", work, Some(keys.text(Action::Save))) {
            self.dialog_save(false);
        }
        if entry(ui, "Save As...", work, Some(keys.text(Action::SaveAs))) {
            self.dialog_save(true);
        }
        if entry(ui, "Project Backups...", work, None) {
            self.backups_open = true;
        }
        if entry(ui, "Increment and Save", work, Some(keys.text(Action::IncrementSave))) {
            self.increment_and_save();
        }
        ui.separator();
        later(ui, "Print...");
        later(ui, "Print Preview");
        later(ui, "Print Setup...");
        ui.separator();
        ui.add_enabled_ui(work, |ui| {
            ui.menu_button(tr("Import"), |ui| {
                if entry(ui, "Import Shapes (SVG, DXF)...", true, Some(keys.text(Action::Import))) {
                    self.dialog_import();
                }
                if entry(ui, "Import Bitmap / Trace...", true, None) {
                    self.start_trace();
                }
                if entry(ui, "Import 3D Model (STL)...", true, None) {
                    self.dialog_import_model();
                }
                if entry(ui, "3D Relief from Picture...", true, None) {
                    self.dialog_relief_picture();
                }
                later(ui, "Import Toolpaths from Other Programs...");
            });
            ui.menu_button(tr("Export"), |ui| {
                for (label, extension) in [("DXF", "dxf"), ("SVG", "svg"), ("EPS", "eps"), ("PDF", "pdf")] {
                    if entry(ui, label, true, None) {
                        self.export_drawing_as(extension);
                    }
                }
                later(ui, "AI");
            });
        });
        if entry(ui, "Open Settings Folder...", true, None) {
            self.open_settings_folder();
        }
        ui.separator();
        ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
            ui.menu_button(tr("Recent Files"), |ui| {
                for path in self.recent.clone() {
                    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        ui.close();
                        self.open_recent(path);
                    }
                }
            });
        });
        ui.separator();
        // The Edit menu has this once there's a project.
        if !work && entry(ui, "Preferences...", true, None) {
            self.prefs_open = true;
        }
        if ui.button(tr("Exit")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn edit_menu(&mut self, ui: &mut Ui) {
        let keys = self.keys;
        let some = !self.selected.is_empty();
        if entry(ui, "Undo", !self.undo.is_empty(), Some(keys.text(Action::Undo))) {
            self.do_undo();
        }
        if entry(ui, "Redo", !self.redo.is_empty(), Some(keys.text(Action::Redo))) {
            self.do_redo();
        }
        ui.separator();
        if entry(ui, "Cut", some, Some(keys.text(Action::Cut))) {
            self.copy_selected();
            self.delete_selected();
        }
        if entry(ui, "Copy", some, Some(keys.text(Action::Copy))) {
            self.copy_selected();
        }
        if entry(ui, "Paste", !self.clipboard.is_empty(), Some(keys.text(Action::Paste))) {
            self.paste();
        }
        if entry(ui, "Delete", some, Some(keys.text(Action::Delete))) {
            self.delete_selected();
        }
        ui.separator();
        ui.menu_button(tr("Selection"), |ui| {
            if entry(ui, "Select All", true, Some(keys.text(Action::SelectAll))) {
                self.select_all();
            }
            if entry(ui, "Select All Open Vectors", true, None) {
                self.select_open();
            }
            if entry(ui, "Select All Duplicate Vectors", true, None) {
                self.select_duplicates();
            }
            let adding = ui.input(|i| i.modifiers.shift);
            if entry(ui, "Select All Vectors On Current Layer", true, None) {
                self.select_on_active_layer(adding);
            }
            if entry(ui, "Select Similar", some, Some(keys.text(Action::SelectSimilar))) {
                let tol = self.default_similar_tolerance();
                self.select_similar(tol);
            }
            if entry(ui, "Unselect All", some, None) {
                self.selected.clear();
                self.handles_on = None;
            }
            ui.add_enabled(false, egui::Button::new(tr("Vector Selector..."))).on_disabled_hover_text(tr("It opens from the Selector button of a toolpath's form."));
        });
        ui.menu_button(tr("Align Selected Objects"), |ui| {
            if entry(ui, "Center In Material", some, Some(keys.text(Action::CentreOnMaterial))) {
                self.align_selected(Some(0), Some(0));
            }
            if entry(ui, "Center Horizontally In Material", some, None) {
                self.align_selected(Some(0), None);
            }
            if entry(ui, "Center Vertically In Material", some, None) {
                self.align_selected(None, Some(0));
            }
            ui.separator();
            // With Shift, the others move as one block instead of each on its own.
            let together = ui.input(|i| i.modifiers.shift);
            if entry(ui, "Center", some, None) {
                self.align_to_last_center_as(true, true, together);
            }
            for (label, edge) in [("Left", Edge::Left), ("Right", Edge::Right), ("Top", Edge::Top), ("Bottom", Edge::Bottom)] {
                if entry(ui, label, some, None) {
                    self.align_to_last_edge_as(edge, true, together);
                }
            }
            if entry(ui, "Center Horizontally", some, None) {
                self.align_to_last_center_as(true, false, together);
            }
            if entry(ui, "Center Vertically", some, None) {
                self.align_to_last_center_as(false, true, together);
            }
            for (label, edge) in [("Left To Right", Edge::Right), ("Right To Left", Edge::Left), ("Top To Bottom", Edge::Bottom), ("Bottom To Top", Edge::Top)] {
                if entry(ui, label, some, None) {
                    self.align_to_last_edge_as(edge, false, together);
                }
            }
            ui.separator();
            if entry(ui, "Alignment Tools", true, Some(keys.text(Action::AlignForm))) {
                self.start_xform(Xform::Align);
            }
        });
        ui.separator();
        if entry(ui, "Group", self.selected.len() > 1, Some(keys.text(Action::Group))) {
            self.group_selected();
        }
        let apart = self.can_ungroup();
        let ungroup_tip = format!("{}\n{}", keys.text(Action::Ungroup), tf!("Each shape goes back on its own layer. {}: they stay on the group's layer.", keys.text(Action::UngroupStay)));
        if entry(ui, "Ungroup", apart, Some(ungroup_tip)) {
            self.ungroup_selected();
        }
        if entry(ui, "Ungroup All Levels", apart, Some(keys.text(Action::UngroupAll))) {
            self.ungroup_with(true, true);
        }
        ui.menu_button(tr("Pieces"), |ui| {
            let in_piece = self.selection_in_piece();
            if entry(ui, "Make Piece", self.can_make_piece(), Some(keys.text(Action::MakePiece))) {
                self.make_piece();
            }
            if entry(ui, "Select Piece", in_piece, Some(keys.text(Action::SelectPiece))) {
                self.select_piece();
            }
            ui.checkbox(&mut self.adjust_piece, tr("Adjust Alone")).on_hover_text(keys.text(Action::AdjustPiece));
            if entry(ui, "Release Piece", in_piece, Some(keys.text(Action::ReleasePiece))) {
                self.release_piece();
            }
        });
        ui.separator();
        if entry(ui, "Join Vectors", true, Some(keys.text(Action::JoinForm))) {
            self.start_xform(Xform::Join);
        }
        later(ui, "Curve Fit Vectors");
        if entry(ui, "Nest Selected Objects", true, None) {
            self.start_xform(Xform::Nest);
        }
        if entry(ui, "Modify Several Shapes...", true, None) {
            self.start_xform(Xform::Bulk);
        }
        if entry(ui, "Drawing Constraints...", true, None) {
            self.left_tab = LeftTab::Drawing;
            self.start_constraint_form();
        }
        ui.separator();
        if entry(ui, "Job Setup...", true, None) {
            self.open_job_setup();
        }
        if entry(ui, "Move X/Y Zero...", true, None) {
            self.start_xform(Xform::Move);
        }
        ui.separator();
        if entry(ui, "Notes", true, None) {
            self.notes_open = true;
        }
        later(ui, "Document Variables");
        ui.menu_button(tr("Snap Options"), |ui| {
            ui.checkbox(&mut self.snap, tr("Snap to Grid"));
            ui.checkbox(&mut self.snap_shapes, tr("Snap to Shapes")).on_hover_text(tr("Hold Alt to turn it off for a moment."));
        });
        if entry(ui, "Preferences...", true, None) {
            self.prefs_open = true;
        }
    }

    fn model_menu(&mut self, ui: &mut Ui) {
        let has_model = !self.doc.components.is_empty();
        ui.menu_button(tr("Create Component"), |ui| {
            if entry(ui, "Create Shape from Selection...", true, None) {
                self.open_shape_form(None);
            }
            later(ui, "Two Rail Sweep");
            later(ui, "Extrude and Weave");
            later(ui, "Turn and Spin");
            later(ui, "Create Textured Area Component");
            later(ui, "Add Zero Plane");
        });
        ui.menu_button(tr("Create Component from..."), |ui| {
            later(ui, "Bake Components");
            later(ui, "Create Component from Visible Model");
            later(ui, "Create Component with Draft from Visible Model");
            later(ui, "Create an Offset Model");
            later(ui, "Slice Model");
            later(ui, "Create Component from Toolpath Preview");
        });
        ui.separator();
        if entry(ui, "3D Relief from Picture...", true, None) {
            self.dialog_relief_picture();
        }
        if entry(ui, "Import 3D Model (STL)...", true, None) {
            self.dialog_import_model();
        }
        if entry(ui, "Export as STL", has_model, None) {
            self.export_stl();
        }
        if entry(ui, "Export as Grayscale Bitmap", has_model, None) {
            self.export_grey();
        }
        later(ui, "Export as 3D Clipart");
        ui.separator();
        later(ui, "Sculpt Visible Model");
        later(ui, "Smooth Selected Components");
        later(ui, "Reset Inside Selected Vectors");
        later(ui, "Reset Outside Selected Vectors");
        later(ui, "Split Selected Component");
        later(ui, "Create Vector Boundary Around Selected Components");
        ui.separator();
        later(ui, "Emboss Component");
        later(ui, "Scale Model Z Height");
        later(ui, "Distort Selected Component");
        ui.separator();
        if entry(ui, "Selected Component Properties", self.selected_comp.is_some(), None) {
            self.open_shape_form(self.selected_comp);
        }
        if entry(ui, "Delete All Shapes", has_model, None) {
            self.checkpoint();
            self.doc.components.clear();
            self.selected_comp = None;
            self.model_dirty = true;
        }
    }

    fn toolpaths_menu(&mut self, ui: &mut Ui) {
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
            if entry(ui, label, true, None) {
                self.start_op(kind);
            }
        }
        ui.separator();
        ui.checkbox(&mut self.right_open, tr("Show Toolpaths Tab"));
        if entry(ui, "Tool Database...", true, None) {
            self.open_tool_list(None);
        }
        if entry(ui, "Recalculate All", true, None) {
            self.recalculate_all();
        }
        ui.menu_button(tr("Toolpath Drawing"), |ui| {
            ui.checkbox(&mut self.show_toolpaths, tr("Draw 2D Previews"));
            later(ui, "Make 2D Previews Solid");
            ui.checkbox(&mut self.auto_open_3d, tr("Auto Open 3D View"));
            ui.checkbox(&mut self.draw_rapids, tr("Draw rapid moves"));
            later(ui, "Draw plunge moves");
            later(ui, "Draw retract moves");
            later(ui, "Draw join moves");
        });
        ui.menu_button(tr("Preview Simulation Quality"), |ui| self.quality_list(ui));
        ui.separator();
        ui.menu_button(tr("Templates"), |ui| {
            let has_selected = self.selected_tp.is_some_and(|i| i < self.doc.toolpaths.len());
            if entry(ui, "Save Selected Toolpath as Template...", has_selected, None) {
                self.save_template_dialog(true);
            }
            if entry(ui, "Save All Visible Toolpaths as Template...", self.doc.toolpaths.iter().any(|t| t.visible), None) {
                self.save_template_dialog(false);
            }
            if entry(ui, "Load Template...", true, None) {
                self.load_template_dialog();
            }
        });
        if entry(ui, "Merge Visible Toolpaths", true, None) {
            self.open_merge_form();
        }
        if entry(ui, "Create Job Sheet", true, None) {
            let open_after = ui.input(|i| i.modifiers.ctrl);
            self.save_job_sheet_dialog(ui.ctx(), open_after);
        }
        ui.separator();
        if entry(ui, "Install Post Processor...", true, None) {
            if let Some(files) = rfd::FileDialog::new().add_filter(tr("Post processor"), &["pp"]).pick_files() {
                self.report_loaded_posts(files);
            }
        }
        if entry(ui, "Save Toolpaths...", true, None) {
            self.right = RightMode::Save;
            self.right_open = true;
        }
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        let keys = self.keys;
        if ui.selectable_label(!self.view3d, tr("2D View")).on_hover_text(keys.text(Action::View2d)).clicked() {
            self.view3d = false;
            ui.close();
        }
        if ui.selectable_label(self.view3d, tr("3D View")).on_hover_text(keys.text(Action::View3d)).clicked() {
            self.view3d = true;
            ui.close();
        }
        ui.separator();
        if entry(ui, "Zoom to job", true, Some(keys.text(Action::ZoomFit))) {
            self.zoom_to_job();
        }
        if entry(ui, "Refresh 2D View", true, Some(keys.text(Action::Refresh))) {
            ui.ctx().request_repaint();
        }
        ui.checkbox(&mut self.grid, tr("Show Grid"));
        ui.checkbox(&mut self.show_vectors, tr("Show Shapes"));
        ui.checkbox(&mut self.show_toolpaths, tr("Show Toolpaths"));
        ui.menu_button(tr("Guide Lines"), |ui| {
            ui.checkbox(&mut self.show_guides, tr("Show Guides")).on_hover_text(tr("Drag from a ruler onto the sheet to add one. Drag it back to remove it."));
            if entry(ui, "Remove All Guides", !self.doc.guides.is_empty(), None) {
                self.checkpoint();
                self.doc.guides.clear();
            }
            if entry(ui, "Lock All Existing Guides", !self.guides_locked, None) {
                self.guides_locked = true;
            }
            if entry(ui, "Unlock All Existing Guides", self.guides_locked, None) {
                self.guides_locked = false;
            }
        });
        ui.separator();
        later(ui, "Draw Origin");
        later(ui, "Draw Modeling Plane");
        later(ui, "Draw Material Block");
        ui.separator();
        later(ui, "Multi-Sided View");
        later(ui, "Auto-Wrapping View");
        later(ui, "Color Shaded View");
        later(ui, "Use Shaded Background");
        later(ui, "Light Follows Viewer");
        later(ui, "Use Shadow Shading");
        ui.menu_button(tr("3D Quality"), |ui| self.quality_list(ui));
        ui.separator();
        if entry(ui, "Save Shaded Image", self.view3d, None) {
            self.save_preview = rfd::FileDialog::new().add_filter(tr("Picture"), &["png"]).set_file_name("preview.png").save_file();
        }
    }

    fn help_menu(&mut self, ui: &mut Ui) {
        later(ui, "Help Contents");
        if entry(ui, "Keyboard Shortcuts", true, None) {
            self.prefs_open = true;
        }
        if entry(ui, "Send Feedback...", true, None) {
            self.open_feedback(ui.ctx(), None);
        }
        if entry(ui, "Crash Reports...", true, None) {
            self.crashes_open = true;
        }
        if entry(ui, "Details for Support...", true, None) {
            self.support_open = true;
        }
        if cfg!(target_os = "linux") {
            if let Some(data) = crate::linuxmenu::data_home() {
                let there = crate::linuxmenu::installed_in(&data);
                let label = if there { "Remove from Applications Menu" } else { "Add to Applications Menu" };
                if entry(ui, label, true, None) {
                    self.applications_menu(&data, there);
                }
            }
        }
        ui.separator();
        if entry(ui, "About", true, None) {
            self.about_open = true;
        }
    }

    /// Puts Burin in the desktop's menu, or takes it out.
    fn applications_menu(&mut self, data: &FilePath, there: bool) {
        if there {
            crate::linuxmenu::remove_from(data);
            crate::linuxmenu::refresh(data);
            self.status = tr("Burin is no longer in the applications menu.").into();
            return;
        }
        let Some(program) = crate::linuxmenu::program_file() else { return };
        match crate::linuxmenu::install_in(data, &program, include_bytes!("../release/burin.png")) {
            Ok(()) => {
                crate::linuxmenu::refresh(data);
                self.status = tr("Burin is in the applications menu, and opens .burin files. If you move the program, add it again.").into();
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// The menus, in the original's order.
    pub(crate) fn main_menus(&mut self, ui: &mut Ui, work: bool) {
        ui.menu_button(tr("File"), |ui| self.file_menu(ui, work));
        if work {
            ui.menu_button(tr("Edit"), |ui| self.edit_menu(ui));
            ui.menu_button(tr("Model"), |ui| self.model_menu(ui));
            ui.menu_button(tr("Toolpaths"), |ui| self.toolpaths_menu(ui));
            ui.menu_button(tr("View"), |ui| self.view_menu(ui));
        }
        ui.menu_button(tr("Gadgets"), |ui| {
            ui.add_enabled(false, egui::Label::new(tr("No gadgets installed")));
        });
        ui.menu_button(tr("Help"), |ui| self.help_menu(ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::rect;

    fn app_with(paths: Vec<Vec<Path>>) -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        for p in paths {
            a.doc.add_vector(p, Prim::Free);
        }
        a
    }

    #[test]
    fn the_next_number_follows_a_hyphen_an_underscore_or_a_space() {
        let name = |p: &str| incremented(FilePath::new(p)).unwrap().to_string_lossy().into_owned();
        assert_eq!(name("/nowhere/plate_1.burin"), "/nowhere/plate_2.burin");
        assert_eq!(name("/nowhere/plate-9.burin"), "/nowhere/plate-10.burin");
        assert_eq!(name("/nowhere/plate 3.burin"), "/nowhere/plate 4.burin");
        assert_eq!(name("/nowhere/plate_007.burin"), "/nowhere/plate_8.burin", "no zeros are kept");
        assert_eq!(name("/nowhere/plate.burin"), "/nowhere/plate_1.burin");
        assert_eq!(name("/nowhere/plate2.burin"), "/nowhere/plate2_1.burin");
        assert_eq!(name("/nowhere/_3.burin"), "/nowhere/_3_1.burin", "there has to be a name before it");
    }

    #[test]
    fn taken_numbers_are_passed_over_a_hundred_times_at_most() {
        let folder = std::env::temp_dir().join(format!("burin-increment-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for n in 2..=102 {
            std::fs::write(folder.join(format!("plate_{n}.burin")), "taken").unwrap();
        }
        assert_eq!(incremented(&folder.join("plate_1.burin")), None);
        std::fs::remove_file(folder.join("plate_102.burin")).unwrap();
        assert_eq!(incremented(&folder.join("plate_1.burin")).unwrap().file_name().unwrap(), "plate_102.burin");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn duplicates_are_within_the_tolerance_of_a_twentieth_of_a_millimetre() {
        let square = |x: f64| vec![Path::closed(rect([5.0 + x, 5.0], 2.0, 2.0, 0.0))];
        let mut a = app_with(vec![square(0.0)]);
        a.doc.job.units = Units::Mm;
        a.select_duplicates();
        assert!(a.selected.is_empty());
        assert_eq!(a.status, "No duplicate vectors in design");
        let close = a.doc.add_vector(square(0.03), Prim::Free);
        let far = a.doc.add_vector(square(0.2), Prim::Free);
        a.select_duplicates();
        assert_eq!(a.selected, [close]);
        assert!(!a.selected.contains(&far));
        // In inches the same distance is much wider than the tolerance.
        a.doc.job.units = Units::Inches;
        a.select_duplicates();
        assert!(a.selected.is_empty());
        // The same square, started from another corner, isn't taken for one.
        a.doc.job.units = Units::Mm;
        let mut turned = square(0.0);
        turned[0].pts.rotate_left(1);
        a.doc.add_vector(turned, Prim::Free);
        a.select_duplicates();
        assert_eq!(a.selected, [close]);
        // Nor are locked layers looked at.
        a.doc.layers[0].locked = true;
        a.select_duplicates();
        assert!(a.selected.is_empty());
    }

    #[test]
    fn with_shift_the_others_move_as_one_block() {
        let square = |x: f64, y: f64| vec![Path::closed(rect([x, y], 2.0, 2.0, 0.0))];
        let mut a = app_with(vec![square(1.0, 2.0), square(1.0, 6.0), square(8.0, 4.0)]);
        // The others' sides: one at 0 to 2, the other at 2 to 4 after this move.
        let ids: Vec<u64> = a.doc.vectors.iter().map(|v| v.id).collect();
        a.translate_ids(&[ids[1]], 2.0, 0.0);
        a.selected = ids.clone();
        a.align_to_last_edge_as(Edge::Right, false, true);
        let left = |a: &App, i: usize| geom::bbox(&a.doc.vectors[i].paths).unwrap().0[0];
        assert!((left(&a, 0) - 9.0).abs() < 1e-9, "the block's left side reaches the target's right side");
        assert!((left(&a, 1) - 11.0).abs() < 1e-9, "and they keep their places in it");
        let mut b = app_with(vec![square(1.0, 2.0), square(3.0, 6.0), square(8.0, 4.0)]);
        b.selected = b.doc.vectors.iter().map(|v| v.id).collect();
        b.align_to_last_edge_as(Edge::Right, false, false);
        assert!((left(&b, 0) - 9.0).abs() < 1e-9 && (left(&b, 1) - 9.0).abs() < 1e-9, "each on its own");
    }

    #[test]
    fn selections_of_the_edit_menu() {
        let square = || vec![Path::closed(rect([1.0, 1.0], 2.0, 2.0, 0.0))];
        let mut a = app_with(vec![square(), square(), vec![Path::open(vec![[0.0, 0.0], [1.0, 1.0]])]]);
        let ids: Vec<u64> = a.doc.vectors.iter().map(|v| v.id).collect();
        a.select_open();
        assert_eq!(a.selected, [ids[2]]);
        a.select_duplicates();
        assert_eq!(a.selected, [ids[1]], "the first copy stays out");
        a.select_on_active_layer(false);
        assert_eq!(a.selected.len(), 3);
        // With Shift, what's on another layer stays selected.
        a.doc.layers.push(Layer::named("Other"));
        a.doc.active_layer = 1;
        let other = a.doc.add_vector(square(), Prim::Free);
        a.select_on_active_layer(true);
        assert_eq!(a.selected.len(), 4);
        a.select_on_active_layer(false);
        assert_eq!(a.selected, [other]);
        // A hidden or locked layer gives nothing, and says so for open lines.
        a.doc.layers[0].locked = true;
        a.select_open();
        assert!(a.selected.is_empty());
        assert_eq!(a.status, "No open vectors in design");
        a.doc.layers[0].locked = false;
        a.doc.layers[1].visible = false;
        a.selected = vec![ids[0]];
        a.select_on_active_layer(false);
        assert!(a.selected.is_empty());
    }

    #[test]
    fn select_all_leaves_out_hidden_and_locked_layers() {
        let square = || vec![Path::closed(rect([1.0, 1.0], 2.0, 2.0, 0.0))];
        let mut a = app_with(vec![square()]);
        let first = a.doc.vectors[0].id;
        a.doc.layers.push(Layer::named("Locked"));
        a.doc.layers.push(Layer::named("Hidden"));
        a.doc.active_layer = 1;
        a.doc.add_vector(square(), Prim::Free);
        a.doc.active_layer = 2;
        a.doc.add_vector(square(), Prim::Free);
        a.doc.layers[1].locked = true;
        a.doc.layers[2].visible = false;
        a.select_all();
        assert_eq!(a.selected, [first]);
    }

    #[test]
    fn a_closed_line_with_next_to_no_area_counts_as_open() {
        let there_and_back = vec![Path::closed(vec![[0.0, 0.0], [1.0, 0.0]])];
        let sliver = vec![Path::closed(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 2e-10]])];
        let small = vec![Path::closed(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 2e-8]])];
        let mut a = app_with(vec![there_and_back, sliver, small]);
        let ids: Vec<u64> = a.doc.vectors.iter().map(|v| v.id).collect();
        a.select_open();
        assert_eq!(a.selected, [ids[0], ids[1]]);
    }

    #[test]
    fn closing_goes_back_to_the_first_screen() {
        let mut a = app_with(vec![vec![Path::closed(rect([1.0, 1.0], 2.0, 2.0, 0.0))]]);
        a.screen = Screen::Work;
        a.close_project();
        assert!(a.screen == Screen::Start);
        assert!(a.doc.vectors.is_empty() && a.path.is_none() && !a.has_unsaved_work());
    }

    #[test]
    fn an_svg_can_be_read_back_at_the_same_place() {
        let paths = vec![Path::closed(rect([2.5, 2.5], 3.0, 1.0, 0.0))];
        let text = svg_text(&paths, 10.0, 8.0, "in");
        assert!(text.contains("width=\"10in\"") && text.contains("viewBox=\"0 0 10 8\""));
        // Y is measured from the top in the file: 8 - 2 = 6 for the lowest corner.
        assert!(text.contains(" 6 "), "{text}");
        assert!(crate::svg::read(&text, 1.0).is_ok());
    }

    #[test]
    fn eps_and_pdf_are_in_points_at_the_real_size() {
        let paths = vec![Path::closed(rect([1.0, 1.0], 1.0, 1.0, 0.0))];
        let eps = eps_text(&paths, 10.0, 5.0, 72.0);
        assert!(eps.starts_with("%!PS-Adobe") && eps.contains("%%BoundingBox: 0 0 720 360") && eps.contains("closepath"));
        let pdf = pdf_bytes(&paths, 10.0, 5.0, 72.0);
        let text = String::from_utf8(pdf).unwrap();
        assert!(text.starts_with("%PDF-1.4") && text.contains("/MediaBox [0 0 720 360]"));
        // The table at the end says where it starts, and each object is where it says.
        let table: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(text[table..].starts_with("xref"));
        for (i, line) in text[table..].lines().skip(3).take(4).enumerate() {
            let at: usize = line[..10].parse().unwrap();
            assert!(text[at..].starts_with(&format!("{} 0 obj", i + 1)));
        }
    }

    #[test]
    fn an_stl_is_closed_and_has_as_many_triangles_as_it_says() {
        let mut model = crate::relief::Grid::new(4.0, 2.0, 64, 0.0);
        let (nx, ny) = (model.nx, model.ny);
        model.data[nx + 1] = 0.5;
        let bytes = stl_bytes(&model, 1_000_000);
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 84 + count * 50);
        // Top and bottom, two triangles a square, and two a square of each of the four walls.
        assert_eq!(count, 4 * (nx - 1) * (ny - 1) + 4 * (nx - 1) + 4 * (ny - 1));
        // Every edge of a closed surface belongs to two triangles.
        let mut edges = std::collections::HashMap::<[[u32; 3]; 2], i32>::new();
        for t in bytes[84..].chunks(50) {
            let p: Vec<[u32; 3]> = (0..3).map(|k| { let at = 12 + k * 12; [0, 4, 8].map(|o| u32::from_le_bytes(t[at + o..at + o + 4].try_into().unwrap())) }).collect();
            for k in 0..3 {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                *edges.entry(if a < b { [a, b] } else { [b, a] }).or_default() += 1;
            }
        }
        assert!(edges.values().all(|n| *n == 2), "not closed");
        assert!(stl_bytes(&model, 4).len() < bytes.len(), "fewer squares when asked");
    }

    #[test]
    fn the_grey_picture_is_white_at_the_highest_with_its_top_at_the_top() {
        let mut model = crate::relief::Grid::new(4.0, 2.0, 64, 0.0);
        let at = model.nx * (model.ny - 1);
        model.data[at] = 2.0;
        let picture = grey_picture(&model);
        assert_eq!(picture.get_pixel(0, 0).0[0], 65535);
        assert_eq!(picture.get_pixel(1, 1).0[0], 0);
    }
}
