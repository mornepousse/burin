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
            ui.label(egui::RichText::new(tr("Saved with the job, and printed on its job sheet.")).size(12.0).color(crate::theme::MUTED));
        });
        if self.doc.notes != before {
            self.dirty = true;
        }
        self.notes_open = open;
    }

    /// A project's file dropped on the window opens as if chosen from the menu.
    pub(crate) fn files_dropped(&mut self, ctx: &egui::Context) {
        let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) else { return };
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
}
