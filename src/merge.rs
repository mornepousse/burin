//! Merging the visible toolpaths into one (`0x140b211c0`, `0x14055a0a0`, `0x14055b5c0` in the original).

use crate::app::*;
use crate::i18n::tr;
use crate::theme;
use crate::toolpath::{self, OrderChoices};
use egui::{Align2, RichText};
use std::sync::Arc;

/// How the cuts are put in order. Kept from one merge to the next.
#[derive(Clone, Copy, Debug)]
pub struct MergeChoices {
    pub left_to_right: bool,
    pub bottom_to_top: bool,
    pub grid: bool,
    pub shortest: bool,
    pub by_part: bool,
}

impl Default for MergeChoices {
    fn default() -> Self {
        MergeChoices { left_to_right: false, bottom_to_top: false, grid: false, shortest: false, by_part: true }
    }
}

#[derive(Clone, Debug)]
pub struct MergeForm {
    pub name: String,
    pub problem: Option<String>,
}

impl App {
    /// The first free name, stopping at 99.
    fn next_merged_name(&self) -> String {
        let named = |n: u32| tf!("Merged Toolpath {}", n);
        (1..100).map(named).find(|name| !self.doc.toolpaths.iter().any(|t| t.op.name == *name)).unwrap_or_else(|| named(99))
    }

    pub fn open_merge_form(&mut self) {
        self.merge_form = Some(MergeForm { name: self.next_merged_name(), problem: None });
    }

    /// The toolpaths that would be merged have different rates: only the first's are kept.
    fn merge_rates_differ(&self) -> Option<String> {
        let mut seen = self.doc.toolpaths.iter().filter(|t| t.visible && self.doc.toolpath_on_side(t)).map(|t| &t.op.tool);
        let first = seen.next()?;
        seen.any(|t| t.feed != first.feed || t.plunge != first.plunge || t.rpm != first.rpm).then(|| first.name.clone())
    }

    /// Makes the merged toolpath, or says why it can't.
    pub fn merge_visible(&mut self, name: &str) -> Result<(), String> {
        if self.doc.toolpaths.is_empty() {
            return Err(tr("No Toolpaths").into());
        }
        let chosen: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|i| self.doc.toolpaths[*i].visible).collect();
        if chosen.len() < 2 {
            return Err(tr("Two visible toolpaths at least are needed to merge.").into());
        }
        let first = self.doc.toolpaths[chosen[0]].clone();
        let tool = &first.op.tool;
        let same = |t: &toolpath::Tool| t.kind == tool.kind && t.metric == tool.metric && (t.dia - tool.dia).abs() < 1e-6 && (t.angle - tool.angle).abs() < 1e-6;
        if !chosen.iter().all(|i| same(&self.doc.toolpaths[*i].op.tool)) {
            return Err(tr("All the toolpaths to merge must use the same tool. The visible ones don't.").into());
        }
        let empty: Vec<&str> = chosen.iter().map(|i| &self.doc.toolpaths[*i]).filter(|t| toolpath::cuts_of(&t.moves, 0).is_empty()).map(|t| t.op.name.as_str()).collect();
        if !empty.is_empty() {
            return Err(tf!("These toolpaths have nothing to cut, so they can't be merged:\n\n{}\n\nRecalculate them and try again.", empty.join("\n")));
        }
        let c = self.merge_choices;
        let choices = OrderChoices { selection: false, left_to_right: c.left_to_right, bottom_to_top: c.bottom_to_top, grid: c.grid, shortest: c.shortest };
        let lists: Vec<&[toolpath::Move]> = chosen.iter().map(|i| self.doc.toolpaths[*i].moves.as_slice()).collect();
        let moves = Arc::new(toolpath::merged_moves(&lists, choices, c.by_part, self.doc.machine_on_job(), tool.metric));
        let mut source: Vec<u64> = Vec::new();
        for id in chosen.iter().flat_map(|i| self.doc.toolpaths[*i].op.source.iter().copied()) {
            if !source.contains(&id) {
                source.push(id);
            }
        }
        let mut op = first.op;
        op.name = name.trim().to_string();
        if op.name.is_empty() {
            op.name = self.next_merged_name();
        }
        op.merged = true;
        op.selector = None;
        op.source = source;
        self.checkpoint();
        // The original leaves them showing. Saving all that is visible would then cut everything twice.
        for i in &chosen {
            self.doc.toolpaths[*i].visible = false;
        }
        let color = TOOLPATH_COLOURS[self.doc.toolpaths.len() % TOOLPATH_COLOURS.len()];
        self.status = tf!("{} toolpaths merged into {}. They are now hidden, so the file won't cut them twice.", chosen.len(), op.name);
        self.doc.toolpaths.push(Toolpath { op, visible: true, color, kept_moves: Some(moves.clone()), kept_steps: None, moves, face: self.doc.active_face, sheet: self.doc.active_sheet });
        self.selected_tp = Some(self.doc.toolpaths.len() - 1);
        self.reset_preview();
        Ok(())
    }

    pub(crate) fn merge_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.merge_form.take() else { return };
        let mut open = true;
        let mut merge = false;
        let rates = self.merge_rates_differ();
        egui::Window::new(tr("Merge Visible Toolpaths")).id(egui::Id::unique("merge-toolpaths")).open(&mut open).collapsible(false).resizable(false).default_width(380.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(280.0));
            });
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Cutting order")).strong());
            let c = &mut self.merge_choices;
            ui.checkbox(&mut c.left_to_right, tr("Left to Right"));
            ui.checkbox(&mut c.bottom_to_top, tr("Bottom to Top"));
            ui.checkbox(&mut c.grid, tr("Grid"));
            ui.checkbox(&mut c.shortest, tr("Shortest Path"));
            let none = !(c.left_to_right || c.bottom_to_top || c.grid || c.shortest);
            ui.add_space(6.0);
            ui.checkbox(&mut c.by_part, tr("Keep each part's cuts together"));
            if none {
                let says = if c.by_part { "With no order ticked, parts follow the order of the toolpaths." } else { "With no order ticked, the order with the least travel is used." };
                ui.label(RichText::new(tr(says)).color(theme::MUTED));
            }
            if !c.by_part {
                ui.label(RichText::new(tr("Warning: cuts can change order, within a toolpath and between toolpaths.")).color(theme::WARNING));
            }
            if let Some(first) = &rates {
                ui.label(RichText::new(tf!("These toolpaths have different feed rates or spindle speeds. The merged toolpath uses those of the first: {}", first)).color(theme::WARNING));
            }
            if let Some(problem) = &form.problem {
                ui.label(RichText::new(problem).color(theme::ERROR));
            }
            ui.add_space(6.0);
            if ui.button(tr("Merge")).clicked() {
                merge = true;
            }
        });
        if merge {
            match self.merge_visible(&form.name.clone()) {
                Ok(()) => return,
                Err(why) => form.problem = Some(why),
            }
        }
        if open {
            self.merge_form = Some(form);
        }
    }
}
