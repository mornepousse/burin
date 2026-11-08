//! What's useful to know when something is wrong on a computer the author can't see, in one place to
//! copy: never the user's files or folders.

use crate::app::*;
use crate::i18n::tr;
use egui::Align2;

impl App {
    /// The details, plain text. The start-up log says how the window was opened.
    pub fn support_text(&self) -> String {
        let moves: usize = self.doc.toolpaths.iter().map(|t| t.moves.len()).sum();
        let seconds = |s: Option<f32>| s.map_or("-".to_string(), |s| format!("{s:.1} s"));
        let log: Vec<String> = crate::startup::previous_log().lines().filter(|l| !l.starts_with("Message box")).map(str::to_string).collect();
        let mut out = format!(
            "Burin {}\n{}\n3D quality: {}\nLast picture: {}\nWhole preview: {}\nJob: {} x {} x {} {:?}, {} shapes, {} toolpaths, {} moves, resolution {}\n",
            env!("CARGO_PKG_VERSION"),
            crate::startup::system_line(),
            self.quality3d.name(),
            seconds(self.picture_seconds),
            seconds(self.preview_seconds),
            self.doc.job.width,
            self.doc.job.height,
            self.doc.job.thickness,
            self.doc.job.units,
            self.doc.vectors.len(),
            self.doc.toolpaths.len(),
            moves,
            self.doc.job.resolution,
        );
        out.push_str("Last start:\n");
        for line in log {
            out.push_str("  ");
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    /// Notes about the job, kept in its file.
    pub(crate) fn notes_window(&mut self, ctx: &egui::Context) {
        if !self.notes_open {
            return;
        }
        let mut open = true;
        let before = self.doc.notes.clone();
        egui::Window::new(tr("Notes")).id(egui::Id::unique("notes")).open(&mut open).collapsible(false).default_width(440.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.add(egui::TextEdit::multiline(&mut self.doc.notes).desired_width(f32::INFINITY).desired_rows(10));
            ui.label(egui::RichText::new(tr("Saved with the job, printed on its job sheet, and written where a post processor asks for them. Start with a dot to see them each time the job opens.")).size(12.0).color(crate::theme::MUTED));
        });
        if self.doc.notes != before {
            self.dirty = true;
        }
        self.notes_open = open;
    }

    /// A project's file dropped on the window opens as if chosen from the menu. In the web version a
    /// dropped file has no place on disk: its bytes are kept, and any kind Burin reads is opened.
    pub(crate) fn files_dropped(&mut self, ctx: &egui::Context) {
        for path in crate::platform::picked() {
            self.open_picked(path);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().cloned());
        let Some(file) = dropped else { return };
        if crate::platform::WEB {
            // Read by the browser in a moment; it comes back through `picked`.
            crate::platform::keep_dropped(file);
            return;
        }
        let path = file.path().to_path_buf();
        // 3D clipart goes into the job open now, as in the original.
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(crate::modeltools::CLIP3D_EXTENSION)) {
            if let Err(why) = self.import_3d_clipart(&path) {
                self.message = Some(why);
            }
            return;
        }
        if !path.extension().is_some_and(|e| ["burin", "crv3d", "crv"].iter().any(|k| e.eq_ignore_ascii_case(k))) {
            self.message = Some(tf!("Burin can open .burin, .crv3d and .crv files, not {}.", path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string())));
            return;
        }
        self.recent_wanted = Some(path);
        self.leave_for(Leave::Recent);
    }

    pub(crate) fn support_window(&mut self, ctx: &egui::Context) {
        if !self.support_open {
            return;
        }
        let mut open = true;
        let text = self.support_text();
        egui::Window::new(tr("Details for Support")).id(egui::Id::unique("support")).open(&mut open).collapsible(false).default_width(520.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            let mut shown = text.as_str();
            ui.add(egui::TextEdit::multiline(&mut shown).desired_width(f32::INFINITY).desired_rows(14).font(egui::TextStyle::Monospace));
            ui.add_space(6.0);
            if ui.button(tr("Copy These Details")).clicked() {
                ui.ctx().copy_text(text.clone());
                self.status = tr("Copied. Paste this where you were asked for it.").into();
            }
        });
        self.support_open = open;
    }

    /// A file chosen in the web version's picker, or dropped on its page: opened by what kind it is,
    /// as the menu that asked for it would have.
    pub(crate) fn open_picked(&mut self, path: std::path::PathBuf) {
        let ext = path.extension().map_or(String::new(), |e| e.to_string_lossy().to_ascii_lowercase());
        let result = match ext.as_str() {
            "burin" | "crv3d" | "crv" => {
                self.recent_wanted = Some(path);
                self.leave_for(Leave::Recent);
                Ok(())
            }
            "svg" | "dxf" => self.import_drawing(&path),
            crate::modeltools::CLIP3D_EXTENSION => self.import_3d_clipart(&path),
            "stl" => self.open_model_file(&path),
            "png" | "jpg" | "jpeg" | "bmp" => match crate::bitmap::Image::load(&path) {
                // Tracing has its bitmap in the drawing; otherwise a picture becomes a relief.
                Ok(_) if matches!(self.left, LeftForm::Trace(_)) => self.import_bitmap(&path),
                Ok(img) => {
                    self.open_relief_picture(crate::relief_import::short_name(&path), &img);
                    Ok(())
                }
                Err(why) => Err(why),
            },
            "pp" => {
                self.report_loaded_posts(vec![path]);
                Ok(())
            }
            "nc" | "gcode" | "ngc" | "tap" | "txt" | "cnc" => {
                let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().to_string());
                crate::platform::read(&path)
                    .map_err(|e| tf!("Could not open {}: {e}", path.display()))
                    .and_then(|bytes| self.import_toolpath(&name, &String::from_utf8_lossy(&bytes)))
            }
            _ => Err(tf!("Burin can open .burin, .crv3d and .crv files, not {}.", path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string()))),
        };
        if let Err(why) = result {
            self.message = Some(why);
        }
    }
}
