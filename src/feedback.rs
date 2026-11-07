//! Telling the author something: a fault, an idea, a question, or a crash report. Burin sends nothing
//! itself. It opens a draft on the project's page in the browser, for the person to read and submit.

use crate::app::*;
use crate::crash::{self, Report};
use crate::i18n::tr;
use crate::theme;
use egui::{Align2, RichText};
use std::sync::{Arc, Mutex};

/// The page for new reports, written in at build time from where the code is kept. A copy built by
/// hand has none, and offers to copy the text instead.
pub const ISSUES_PAGE: Option<&str> = option_env!("BURIN_ISSUES_URL");

/// Past this an address gets refused, so the text goes by the clipboard instead.
const LONGEST_ADDRESS: usize = 6000;
const LONGEST_TEXT: usize = 24000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Problem,
    Idea,
    Question,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Problem => tr("A problem"),
            Kind::Idea => tr("An idea"),
            Kind::Question => tr("A question"),
        }
    }
    /// How a draft's title starts. In English, for the list of issues.
    fn tag(self) -> &'static str {
        match self {
            Kind::Problem => "Problem",
            Kind::Idea => "Idea",
            Kind::Question => "Question",
        }
    }
}

pub struct FeedbackForm {
    pub kind: Kind,
    pub title: String,
    pub text: String,
    pub with_details: bool,
    /// The crash report it's about.
    pub crash: Option<Report>,
    /// The window as it was before this form opened.
    pub picture: Arc<Mutex<Option<egui::ColorImage>>>,
    pub said: Option<String>,
}

/// Web address text, with everything that could end a value made safe.
pub fn url_encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The address of a draft, and whether the text was too long to go in it.
pub fn draft_address(page: &str, title: &str, body: &str) -> (String, bool) {
    let base = format!("{page}?title={}", url_encoded(title));
    let whole = format!("{base}&body={}", url_encoded(body));
    if whole.len() <= LONGEST_ADDRESS {
        (whole, false)
    } else {
        (format!("{base}&body={}", url_encoded(tr("Paste the text from Burin here: it has been copied."))), true)
    }
}

impl FeedbackForm {
    pub fn title_of_draft(&self) -> String {
        match &self.crash {
            Some(_) => format!("[Crash] {}", self.title.trim()),
            None => format!("[{}] {}", self.kind.tag(), self.title.trim()),
        }
    }

    /// Everything that would go in the draft: all of it shows in the form before anything leaves.
    pub fn body(&self, details: &str) -> String {
        let mut out = self.text.trim().to_string();
        if let Some(report) = &self.crash {
            out.push_str("\n\n### Crash report\n\n```text\n");
            out.push_str(&report.text().replace("```", "'''"));
            out.push_str("```\n");
        } else if self.with_details {
            out.push_str("\n\n### Details\n\n```text\n");
            out.push_str(&details.replace("```", "'''"));
            out.push_str("```\n");
        }
        if out.len() > LONGEST_TEXT {
            let mut end = LONGEST_TEXT;
            while !out.is_char_boundary(end) {
                end -= 1;
            }
            out.truncate(end);
            out.push_str("\n... (cut short)\n");
        }
        out
    }
}

impl App {
    pub fn open_feedback(&mut self, ctx: &egui::Context, crash: Option<Report>) {
        let picture = Arc::new(Mutex::new(None));
        let title = crash.as_ref().map_or(String::new(), Report::headline);
        let text = if crash.is_some() { tr("What were you doing when it stopped?").to_string() } else { String::new() };
        self.feedback = Some(FeedbackForm { kind: Kind::Problem, title, text, with_details: true, crash, picture: picture.clone(), said: None });
        // Taken at once, so the picture shows the work rather than this form.
        self.want_picture = Some(picture);
        ctx.request_repaint();
    }

    pub(crate) fn feedback_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.feedback.take() else { return };
        let details = self.support_text();
        let mut open = true;
        egui::Window::new(tr("Send Feedback")).id(egui::Id::unique("feedback")).open(&mut open).collapsible(false).default_width(560.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            if form.crash.is_none() {
                ui.horizontal(|ui| {
                    for kind in [Kind::Problem, Kind::Idea, Kind::Question] {
                        ui.radio_value(&mut form.kind, kind, kind.name());
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(tr("In a few words"));
            ui.add(egui::TextEdit::singleline(&mut form.title).desired_width(f32::INFINITY));
            ui.add_space(4.0);
            ui.label(tr("Tell us more"));
            ui.add(egui::TextEdit::multiline(&mut form.text).desired_width(f32::INFINITY).desired_rows(5));
            ui.add_space(4.0);
            if form.crash.is_none() {
                ui.checkbox(&mut form.with_details, tr("Add the technical details below"));
            }
            let body = form.body(&details);
            egui::CollapsingHeader::new(tr("Exactly what would be in the draft")).id_salt("draft-text").show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let mut shown = body.as_str();
                    ui.add(egui::TextEdit::multiline(&mut shown).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                });
            });
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Burin sends nothing by itself. The draft opens in your browser and goes nowhere until you submit it. Your job and its file names are never in it.")).size(12.0).color(theme::MUTED));
            ui.add_space(6.0);
            let ready = !form.title.trim().is_empty();
            ui.horizontal_wrapped(|ui| {
                if let Some(page) = self.issues_page.clone() {
                    if ui.add_enabled(ready, egui::Button::new(tr("Open a Draft in the Browser"))).clicked() {
                        crash::note("Open a Draft in the Browser");
                        let (address, too_long) = draft_address(&page, &form.title_of_draft(), &body);
                        if too_long {
                            ui.ctx().copy_text(body.clone());
                            form.said = Some(tr("The text is too long for the address, so it has been copied. Paste it into the draft.").to_string());
                        } else {
                            form.said = Some(tr("The draft is open in your browser. Submit it there when you're happy with it.").to_string());
                        }
                        self.last_draft = Some(address.clone());
                        ui.ctx().open_url(egui::OpenUrl::new_tab(address));
                    }
                }
                if ui.button(tr("Copy the Text")).clicked() {
                    ui.ctx().copy_text(format!("{}\n\n{body}", form.title_of_draft()));
                    form.said = Some(tr("Copied.").to_string());
                }
                let has_picture = form.picture.lock().is_ok_and(|p| p.is_some());
                if ui.add_enabled(has_picture, egui::Button::new(tr("Save a Picture of the Window..."))).on_hover_text(tr("Pictures can show your work, so none goes in the draft. Add this one yourself if you want to.")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("burin.png").save_file() {
                        let image = form.picture.lock().ok().and_then(|p| p.clone());
                        form.said = image.map(|image| match crate::write_png(&crate::pieces::with_extension(path, "png"), &image) {
                            Ok(()) => tr("Saved.").to_string(),
                            Err(e) => e,
                        });
                    }
                }
            });
            if self.issues_page.is_none() {
                ui.label(RichText::new(tr("This copy of Burin doesn't know where to send drafts. Copy the text instead.")).size(12.0).color(theme::MUTED));
            }
            if let Some(said) = &form.said {
                ui.label(RichText::new(said).color(theme::ACCENT));
            }
        });
        if open {
            self.feedback = Some(form);
        }
    }

    /// The list of crash reports, and the one chosen.
    pub(crate) fn crash_window(&mut self, ctx: &egui::Context) {
        if !self.crashes_open {
            return;
        }
        let dir = self.data_dir.as_ref().map(|d| d.join("crashes"));
        let mut reports = dir.as_ref().map_or(Vec::new(), |d| crash::list_in(d));
        crash::mark_seen(&mut reports);
        let mut open = true;
        let mut report_it = None;
        let mut remove = None;
        let mut remove_all = false;
        egui::Window::new(tr("Crash Reports")).id(egui::Id::unique("crash-reports")).open(&mut open).collapsible(false).default_width(640.0).default_height(420.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            if reports.is_empty() {
                ui.label(RichText::new(tr("No crash reports.")).color(theme::MUTED));
                return;
            }
            ui.label(RichText::new(tr("These stay on this computer. Nothing is sent unless you choose to report one.")).size(12.0).color(theme::MUTED));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("crash-list").max_height(130.0).show(ui, |ui| {
                for (i, r) in reports.iter().enumerate() {
                    let times = if r.count > 1 { format!(" (x{})", r.count) } else { String::new() };
                    let line = format!("{}{times}  {}", r.last_seen, r.headline());
                    if ui.selectable_label(self.crash_chosen == i, line).clicked() {
                        self.crash_chosen = i;
                    }
                }
            });
            let chosen = self.crash_chosen.min(reports.len() - 1);
            ui.separator();
            let text = reports[chosen].text();
            egui::ScrollArea::vertical().id_salt("crash-text").max_height(220.0).show(ui, |ui| {
                let mut shown = text.as_str();
                ui.add(egui::TextEdit::multiline(&mut shown).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Report This...")).clicked() {
                    report_it = Some(reports[chosen].clone());
                }
                if ui.button(tr("Copy the Text")).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
                if ui.button(tr("Delete")).clicked() {
                    remove = Some(reports[chosen].file.clone());
                }
                if ui.button(tr("Delete All")).clicked() {
                    remove_all = true;
                }
            });
        });
        if let Some(file) = remove {
            let _ = std::fs::remove_file(file);
        }
        if remove_all {
            for r in &reports {
                let _ = std::fs::remove_file(&r.file);
            }
        }
        self.crashes_open = open;
        if let Some(report) = report_it {
            self.crashes_open = false;
            self.open_feedback(ctx, Some(report));
        }
    }

    /// Said once at the start, after a crash: what was kept, and what can be got back.
    pub(crate) fn after_crash_window(&mut self, ctx: &egui::Context) {
        if !self.after_crash_checked {
            self.after_crash_checked = true;
            if let Some(data) = self.data_dir.clone() {
                self.new_crashes = crash::list_in(&data.join("crashes")).into_iter().filter(|r| !r.seen).count();
                self.recovery = crate::recovery::find(&data);
            }
        }
        if self.new_crashes == 0 && self.recovery.is_none() {
            return;
        }
        let mut open = true;
        let (mut see, mut restore, mut forget) = (false, false, false);
        egui::Window::new(tr("Last Time")).id(egui::Id::unique("after-crash")).open(&mut open).collapsible(false).resizable(false).default_width(420.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            if self.new_crashes > 0 {
                ui.label(tr("Burin didn't close normally last time. A report is on this computer, and nothing has been sent."));
                ui.add_space(4.0);
                if ui.button(tr("See the Report...")).clicked() {
                    see = true;
                }
                ui.add_space(8.0);
            }
            if let Some(found) = &self.recovery {
                ui.label(tf!("Work that wasn't saved was kept automatically at {} (universal time).", found.saved_at));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Get It Back")).clicked() {
                        restore = true;
                    }
                    if ui.button(tr("Delete It")).clicked() {
                        forget = true;
                    }
                });
            }
        });
        if see {
            self.crashes_open = true;
            self.new_crashes = 0;
        }
        if restore {
            if let Some(found) = self.recovery.take() {
                if let Err(why) = self.restore_recovery(&found) {
                    self.message = Some(why);
                }
            }
        }
        if forget {
            if let (Some(data), Some(_)) = (self.data_dir.clone(), self.recovery.take()) {
                crate::recovery::remove(&data);
            }
        }
        if !open {
            // Closed unanswered: the copy stays for next time, and the report in its list.
            self.new_crashes = 0;
            self.recovery_left = self.recovery.take().is_some();
            if let Some(data) = &self.data_dir {
                crash::mark_seen(&mut crash::list_in(&data.join("crashes")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_cannot_be_made_to_say_more_than_it_should() {
        let (address, long) = draft_address("https://example.org/issues/new", "Cut &labels=secret #? é", "body\n&assignees=someone");
        assert!(!long);
        assert!(address.starts_with("https://example.org/issues/new?title=Cut%20%26labels%3Dsecret%20%23%3F%20%C3%A9&body="), "{address}");
        assert!(!address.contains("&labels") && !address.contains("&assignees"));
        let (short, long) = draft_address("https://example.org/issues/new", "t", &"é".repeat(5000));
        assert!(long && short.len() < LONGEST_ADDRESS);
    }

    #[test]
    fn the_text_is_kept_to_a_size_on_a_character_boundary() {
        let form = FeedbackForm { kind: Kind::Idea, title: " Hello ".into(), text: "é".repeat(LONGEST_TEXT), with_details: false, crash: None, picture: Default::default(), said: None };
        assert_eq!(form.title_of_draft(), "[Idea] Hello");
        let body = form.body("");
        assert!(body.len() < LONGEST_TEXT + 40 && body.ends_with("(cut short)\n"));
    }

    #[test]
    fn details_are_only_there_when_asked_for() {
        let mut form = FeedbackForm { kind: Kind::Problem, title: "x".into(), text: "It broke".into(), with_details: true, crash: None, picture: Default::default(), said: None };
        assert!(form.body("the details").contains("the details"));
        form.with_details = false;
        assert!(!form.body("the details").contains("the details"));
        form.crash = Some(Report { message: "boom".into(), ..Default::default() });
        assert_eq!(form.title_of_draft(), "[Crash] x");
        assert!(form.body("the details").contains("boom"));
    }
}
