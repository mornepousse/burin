//! Looking over the toolpaths before they're cut: what could break a tool, mark the work or hit the
//! machine. The original has nothing like this, and none of it stops a file being saved for good:
//! the person decides, once told.

use crate::app::*;
use crate::i18n::tr;
use crate::relief::ToolKind;
use crate::toolpath::{self, OpKind};
use egui::{Align2, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Likely to break a tool, ruin the work or hit something.
    Danger,
    /// Often meant, but worth a look.
    Check,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub level: Level,
    pub toolpath: String,
    pub text: String,
}

/// The thinnest tab that holds a part reliably, in millimetres. A rule of thumb, not the original's.
const THINNEST_TAB_MM: f64 = 1.0;
/// Cutting a little into the board under the material is how a part comes free: only more is said.
const THROUGH_MARGIN_MM: f64 = 1.0;
/// Passes deeper than this many times the tool's diameter.
const DEEPEST_PASS_IN_DIAMETERS: f64 = 2.0;

fn same_spot(metric: bool) -> f64 {
    if metric { 0.001 } else { 0.001 / 25.4 }
}

/// Everything worth saying about one toolpath. `top` is the material's top, where Z is 0.
pub fn check_toolpath(tp: &Toolpath, job: &Job, machine: &toolpath::Machine) -> Vec<Finding> {
    let mut out = Vec::new();
    let name = tp.op.name.clone();
    let mut say = |level: Level, text: String| out.push(Finding { level, toolpath: name.clone(), text });
    let tool = &tp.op.tool;
    let metric = tool.metric;
    let unit = if metric { "mm" } else { "in" };
    let tol = same_spot(metric) * 10.0;
    let moves = toolpath::working_moves(&tp.moves);

    if tool.feed <= 0.0 || tool.plunge <= 0.0 {
        say(Level::Danger, tr("The feed or plunge rate is 0: the machine may refuse the file, or cut at its own speed.").into());
    }
    if tool.rpm <= 0.0 && !matches!(tool.kind, ToolKind::Drill) {
        say(Level::Danger, tr("The spindle speed is 0.").into());
    }
    let deepest = toolpath::deepest_move(moves);
    if tool.flute_length > 0.0 && deepest > tool.flute_length + tol {
        say(Level::Danger, tf!("It cuts {} {unit} deep, but the tool only cuts {} {unit}: the shank or the holder would rub.", toolpath::trim(deepest), toolpath::trim(tool.flute_length)));
    }
    let margin = if metric { THROUGH_MARGIN_MM } else { THROUGH_MARGIN_MM / 25.4 };
    if deepest > job.deepest_allowed() + margin {
        say(Level::Check, tf!("It goes {} {unit} past the bottom of what's under it.", toolpath::trim(deepest - job.deepest_allowed())));
    }
    if matches!(tool.kind, ToolKind::EndMill | ToolKind::BallNose) && tool.dia > 0.0 && tool.pass_depth > tool.dia * DEEPEST_PASS_IN_DIAMETERS + tol {
        say(Level::Check, tf!("Each pass goes {} {unit} deep, more than twice the tool's diameter: a lot to ask of it.", toolpath::trim(tool.pass_depth)));
    }
    if tp.op.kind == OpKind::Profile && tp.op.tabs {
        let thinnest = if metric { THINNEST_TAB_MM } else { THINNEST_TAB_MM / 25.4 };
        if tp.op.tab_thickness < thinnest - tol {
            say(Level::Check, tf!("Its tabs are {} {unit} thick: thin enough to break before the cut ends.", toolpath::trim(tp.op.tab_thickness)));
        }
    }

    // Fast moves across the material, below its top.
    let mut low_rapids = 0;
    let mut lowest: f64 = 0.0;
    // Cutting outside the material, past the tool's own width.
    let reach = tool.dia / 2.0 + tol;
    let mut outside = 0;
    let mut last: Option<[f64; 3]> = None;
    for m in moves {
        if let Some(p) = last {
            let across = (m.p[0] - p[0]).hypot(m.p[1] - p[1]) > same_spot(metric);
            if m.rapid && across && m.p[2].min(p[2]) < -tol {
                low_rapids += 1;
                lowest = lowest.min(m.p[2].min(p[2]));
            }
        }
        if !m.rapid && (m.p[0] < -reach || m.p[1] < -reach || m.p[0] > job.width + reach || m.p[1] > job.height + reach) {
            outside += 1;
        }
        last = Some(m.p);
    }
    if low_rapids > 0 {
        say(Level::Danger, tf!("{} fast moves go across below the top of the material, down to {} {unit}: unless that part is already cut away, the tool would hit it.", low_rapids, toolpath::trim(-lowest)));
    }
    if outside > 0 {
        say(Level::Check, tf!("{} cutting moves are outside the material.", outside));
    }
    if machine.safe_z <= tol {
        say(Level::Danger, tr("The clearance height is at or below the top of the material.").into());
    }
    if machine.home[2] < machine.safe_z - tol {
        say(Level::Check, tr("The home position is lower than the clearance height: going there could drag across the material.").into());
    }
    out
}

/// The visible toolpaths, or the one chosen, the most serious first.
pub fn check_all(app: &App, only: Option<usize>) -> Vec<Finding> {
    let machine = app.doc.machine_on_job();
    let mut out: Vec<Finding> = app
        .doc
        .toolpaths
        .iter()
        .enumerate()
        .filter(|(i, t)| only.map_or(t.visible, |o| o == *i) && !t.moves.is_empty())
        .flat_map(|(_, t)| check_toolpath(t, &app.doc.job, &machine))
        .collect();
    out.sort_by_key(|f| f.level);
    out
}

impl App {
    /// After a stop: a file from one cut of a toolpath on, then the toolpaths after it.
    pub(crate) fn resume_window(&mut self, ctx: &egui::Context) {
        if !self.resume_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        let (mut index, mut cut) = self.resume_choice;
        let visible: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|i| self.doc.toolpaths[*i].visible && !self.doc.toolpaths[*i].moves.is_empty()).collect();
        if !visible.contains(&index) {
            index = visible.first().copied().unwrap_or(0);
        }
        egui::Window::new(tr("Start Again From a Cut")).id(egui::Id::unique("resume")).open(&mut open).collapsible(false).resizable(false).default_width(420.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(tr("After a stop, write a file that starts from one of a toolpath's cuts, then goes on with the toolpaths after it.")).color(crate::theme::MUTED));
            let Some(tp) = self.doc.toolpaths.get(index) else {
                ui.label(tr("No Toolpaths"));
                return;
            };
            egui::ComboBox::from_id_salt("resume-toolpath").selected_text(tp.op.name.clone()).width(260.0).show_ui(ui, |ui| {
                for i in &visible {
                    ui.selectable_value(&mut index, *i, self.doc.toolpaths[*i].op.name.clone());
                }
            });
            let cuts = toolpath::cuts_of(&self.doc.toolpaths[index].moves, 0);
            cut = cut.clamp(1, cuts.len().max(1));
            ui.horizontal(|ui| {
                ui.label(tr("Cut"));
                ui.add(egui::DragValue::new(&mut cut).range(1..=cuts.len().max(1)));
                ui.label(tf!("of {}", cuts.len()));
            });
            if let Some(c) = cuts.get(cut - 1) {
                let deepest = c.moves.iter().map(|m| m.p[2]).fold(f64::MAX, f64::min);
                ui.label(tf!("It starts at X {} Y {}, down to {}.", toolpath::trim(c.moves[0].p[0]), toolpath::trim(c.moves[0].p[1]), toolpath::trim(-deepest)));
            }
            ui.add_space(6.0);
            if ui.button(tr("Save From Here...")).clicked() {
                save = true;
            }
        });
        self.resume_choice = (index, cut);
        if save {
            self.resume = Some((index, cut - 1));
            self.save_gcode_dialog();
            if !self.checks_open {
                self.resume = None;
            }
        }
        self.resume_open = open;
    }

    pub(crate) fn checks_window(&mut self, ctx: &egui::Context) {
        if !self.checks_open {
            return;
        }
        let findings = check_all(self, self.checks_for_save.flatten());
        let mut open = true;
        let mut save_anyway = false;
        egui::Window::new(tr("Check Before Cutting")).id(egui::Id::unique("checks")).open(&mut open).collapsible(false).default_width(560.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            if findings.is_empty() {
                ui.label(tr("Nothing found. This doesn't replace a dry run above the material."));
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for f in &findings {
                    let (mark, colour) = match f.level {
                        Level::Danger => (tr("Danger"), crate::theme::ERROR),
                        Level::Check => (tr("Check"), crate::theme::WARNING),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(mark).strong().color(colour));
                        ui.label(RichText::new(&f.toolpath).strong());
                        ui.label(&f.text);
                    });
                    ui.add_space(2.0);
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Burin can't see clamps, the holder's shape or how the material is held: check those yourself.")).size(12.0).color(crate::theme::MUTED));
            if self.checks_for_save.is_some() {
                ui.add_space(6.0);
                if ui.button(tr("Save Anyway")).clicked() {
                    save_anyway = true;
                }
            }
        });
        if save_anyway {
            self.checks_open = false;
            self.checks_accepted = true;
            self.checks_for_save = None;
            self.save_gcode_dialog();
            self.checks_accepted = false;
            self.resume = None;
            return;
        }
        if !open {
            self.checks_for_save = None;
            self.resume = None;
        }
        self.checks_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolpath::Move;

    fn toolpath(points: &[([f64; 3], bool)]) -> Toolpath {
        let mut tool = toolpath::default_tools().into_iter().find(|t| t.metric).unwrap();
        tool.dia = 6.0;
        tool.pass_depth = 3.0;
        let op = toolpath::blank_op(OpKind::Profile, "Around".into(), tool, 5.0, Vec::new());
        let mut moves = vec![Move { p: [0.0, 0.0, 20.0], rapid: true }];
        moves.extend(points.iter().map(|(p, rapid)| Move { p: *p, rapid: *rapid }));
        moves.push(Move { p: [0.0, 0.0, 20.0], rapid: true });
        Toolpath { op, visible: true, color: [0, 0, 0], moves: std::sync::Arc::new(moves), kept_moves: None, kept_steps: None }
    }

    fn job() -> Job {
        Job { width: 100.0, height: 100.0, thickness: 18.0, units: Units::Mm, ..Default::default() }
    }

    fn machine() -> toolpath::Machine {
        toolpath::Machine { safe_z: 5.0, home: [0.0, 0.0, 20.0], plunge_gap: 2.0, gap_above: 0.0, gap_below: None }
    }

    fn texts(tp: &Toolpath) -> Vec<(Level, String)> {
        check_toolpath(tp, &job(), &machine()).into_iter().map(|f| (f.level, f.text)).collect()
    }

    #[test]
    fn an_ordinary_cut_says_nothing() {
        let tp = toolpath(&[([10.0, 10.0, 5.0], true), ([10.0, 10.0, -3.0], false), ([50.0, 10.0, -3.0], false), ([50.0, 10.0, 5.0], true), ([60.0, 60.0, 5.0], true)]);
        assert!(texts(&tp).is_empty(), "{:?}", texts(&tp));
    }

    #[test]
    fn a_fast_move_through_the_material_is_a_danger() {
        let tp = toolpath(&[([10.0, 10.0, -1.0], false), ([60.0, 10.0, -1.0], true)]);
        let found = texts(&tp);
        assert!(found.iter().any(|(l, t)| *l == Level::Danger && t.contains("fast moves go across")), "{found:?}");
    }

    #[test]
    fn rates_flutes_tabs_and_edges_are_looked_at() {
        let mut tp = toolpath(&[([10.0, 10.0, -12.0], false), ([130.0, 10.0, -12.0], false)]);
        tp.op.tool.feed = 0.0;
        tp.op.tool.flute_length = 10.0;
        tp.op.tabs = true;
        tp.op.tab_thickness = 0.5;
        tp.op.tool.pass_depth = 13.0;
        let found = texts(&tp);
        for part in ["rate is 0", "shank or the holder", "tabs are", "outside the material", "twice the tool's diameter"] {
            assert!(found.iter().any(|(_, t)| t.contains(part)), "{part}: {found:?}");
        }
        // A little into the board is how parts come free; more is said.
        let just = toolpath(&[([10.0, 10.0, -18.5], false), ([20.0, 10.0, -18.5], false)]);
        assert!(!texts(&just).iter().any(|(_, t)| t.contains("past the bottom")));
        let far = toolpath(&[([10.0, 10.0, -20.0], false), ([20.0, 10.0, -20.0], false)]);
        assert!(texts(&far).iter().any(|(_, t)| t.contains("past the bottom")));
    }

    #[test]
    fn the_clearance_and_home_heights_are_looked_at() {
        let tp = toolpath(&[([10.0, 10.0, -1.0], false), ([20.0, 10.0, -1.0], false)]);
        let low = toolpath::Machine { safe_z: 0.0, home: [0.0, 0.0, -1.0], ..machine() };
        let found: Vec<String> = check_toolpath(&tp, &job(), &low).into_iter().map(|f| f.text).collect();
        assert!(found.iter().any(|t| t.contains("clearance height is at or below")) && found.iter().any(|t| t.contains("home position is lower")), "{found:?}");
    }
}
