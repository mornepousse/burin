//! Toolpath settings kept in a file of their own, to use on other jobs (`0x140882140`, `0x140882df0`
//! in the original). Burin writes its own format, not the original's.

use crate::app::*;
use crate::i18n::tr;
use crate::toolpath::{Op, VectorFilter};
use egui::{Align2, RichText};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const EXTENSION: &str = "burintemplate";
const VERSION: u32 = 1;

/// One toolpath's settings, and the layers that say which shapes it is for.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub op: Op,
    /// The one layer its shapes were all on.
    pub layer: Option<String>,
    /// The layers its shape filter was choosing from.
    #[serde(default)]
    pub selector_layers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateFile {
    pub version: u32,
    pub metric: bool,
    pub toolpaths: Vec<Item>,
}

/// A template needing an answer before its toolpaths are added.
#[derive(Clone, Debug)]
pub struct Waiting {
    pub items: Vec<Item>,
    pub missing: Vec<String>,
}

fn layers_wanted(item: &Item) -> impl Iterator<Item = &String> {
    item.layer.iter().chain(item.selector_layers.iter())
}

impl App {
    /// What a toolpath leaves in a template: no shapes and no moves.
    fn template_item(&self, tp: &Toolpath) -> Item {
        let mut op = tp.op.clone();
        let name_of = |i: usize| self.doc.layers.get(i).map(|l| l.name.clone());
        let selector_layers: Vec<String> = op.selector.as_ref().filter(|f| !f.visible_layers).map_or(Vec::new(), |f| f.layers.iter().filter_map(|i| name_of(*i)).collect());
        // With no filter, the layer its shapes are on says which shapes it is for.
        let mut on: Vec<usize> = self.doc.vectors.iter().filter(|v| op.source.contains(&v.id)).map(|v| v.layer).collect();
        on.dedup();
        on.sort_unstable();
        on.dedup();
        let layer = match (op.selector.is_none(), on.as_slice()) {
            (true, [one]) => name_of(*one),
            _ => None,
        };
        op.paths.clear();
        op.source.clear();
        Item { op, layer, selector_layers }
    }

    /// The template for the selected toolpath, or for all that are visible. Merged toolpaths have no
    /// settings that could make them again.
    pub fn template_of(&self, only_selected: bool) -> Result<TemplateFile, String> {
        let chosen: Vec<&Toolpath> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| if only_selected { self.selected_tp == Some(*i) } else { t.visible }).map(|(_, t)| t).collect();
        if only_selected && chosen.iter().any(|t| t.op.merged) {
            return Err(tr("A merged toolpath can't be saved as a template: its settings can't make it again.").into());
        }
        let toolpaths: Vec<Item> = chosen.into_iter().filter(|t| !t.op.merged).map(|t| self.template_item(t)).collect();
        if toolpaths.is_empty() {
            return Err(tr("No Toolpaths").into());
        }
        Ok(TemplateFile { version: VERSION, metric: self.doc.job.units == Units::Mm, toolpaths })
    }

    pub fn save_template_dialog(&mut self, only_selected: bool) {
        let template = match self.template_of(only_selected) {
            Ok(t) => t,
            Err(why) => {
                self.message = Some(why);
                return;
            }
        };
        // Named after the toolpath, or after the job, beside the job's file.
        let named = if only_selected { self.selected_tp.and_then(|i| self.doc.toolpaths.get(i)).map(|t| t.op.name.clone()) } else { self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()) };
        let name = format!("{}.{EXTENSION}", crate::pieces::file_stem_of(named.as_deref().unwrap_or("")));
        let mut dialog = rfd::FileDialog::new().add_filter(tr("Toolpath template"), &[EXTENSION]).set_file_name(name);
        if let Some(folder) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(folder);
        }
        let Some(path) = dialog.save_file() else { return };
        let path = crate::pieces::with_extension(path, EXTENSION);
        let written = serde_json::to_string_pretty(&template).map_err(|e| e.to_string()).and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
        match written {
            Ok(()) => self.status = tf!("{} toolpaths saved as a template.", template.toolpaths.len()),
            Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
        }
    }

    pub fn load_template_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter(tr("Toolpath template"), &[EXTENSION]).pick_file() else { return };
        let read = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        if let Err(why) = read.and_then(|text| self.load_template(&text)) {
            self.message = Some(tf!("Could not read this template:\n\n{}", why));
        }
    }

    /// Reads a template's text. If it wants layers this job lacks, that gets asked about first.
    pub fn load_template(&mut self, text: &str) -> Result<(), String> {
        let mut template: TemplateFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if template.version > VERSION {
            return Err(tr("This template was made by a newer version of Burin.").into());
        }
        let metric = self.doc.job.units == Units::Mm;
        if template.metric != metric {
            let k = if metric { 25.4 } else { 1.0 / 25.4 };
            for item in &mut template.toolpaths {
                op_in_units(&mut item.op, k, metric);
            }
        }
        let mut missing: Vec<String> = Vec::new();
        for name in template.toolpaths.iter().flat_map(layers_wanted) {
            if !self.doc.layers.iter().any(|l| l.name == *name) && !missing.contains(name) {
                missing.push(name.clone());
            }
        }
        if missing.is_empty() {
            self.add_template_items(template.toolpaths);
        } else {
            self.template_waiting = Some(Waiting { items: template.toolpaths, missing });
        }
        Ok(())
    }

    /// Adds the toolpaths, each with the shapes its layers now hold. None is calculated yet: the user
    /// may still be moving shapes onto those layers.
    pub fn add_template_items(&mut self, items: Vec<Item>) {
        if items.is_empty() {
            self.message = Some(tr("No toolpaths were left once those for the missing layers were removed.").into());
            return;
        }
        self.checkpoint();
        let count = items.len();
        let lines = self.doc.current_paths();
        for item in items {
            let layer_at = |name: &String| self.doc.layers.iter().position(|l| l.name == *name);
            let mut op = item.op;
            if let Some(i) = item.layer.as_ref().and_then(layer_at) {
                op.selector = Some(VectorFilter { open: true, closed: true, visible_layers: false, layers: vec![i], associate: true, ..Default::default() });
            } else if let Some(f) = &mut op.selector {
                if !f.visible_layers {
                    f.layers = item.selector_layers.iter().filter_map(layer_at).collect();
                }
            }
            op.source = op.selector.as_ref().map_or(Vec::new(), |f| self.filter_ids(f, op.tool.dia));
            op.paths = op.source.iter().filter_map(|id| lines.get(id)).flatten().cloned().collect();
            let color = TOOLPATH_COLOURS[self.doc.toolpaths.len() % TOOLPATH_COLOURS.len()];
            self.doc.toolpaths.push(Toolpath { op, visible: true, color, moves: Arc::new(Vec::new()), kept_moves: None, kept_steps: None });
        }
        self.selected_tp = Some(self.doc.toolpaths.len() - 1);
        self.right_open = true;
        self.right = RightMode::List;
        self.status = tf!("{} toolpaths loaded from the template. Edit them to choose their shapes, or Recalculate.", count);
    }

    pub fn create_template_layers(&mut self, waiting: Waiting) {
        self.checkpoint();
        for name in &waiting.missing {
            self.doc.layers.push(Layer::named(name));
        }
        self.add_template_items(waiting.items);
    }

    pub fn drop_template_items_without_layers(&mut self, waiting: Waiting) {
        let items = waiting.items.into_iter().filter(|item| !layers_wanted(item).any(|name| waiting.missing.contains(name))).collect();
        self.add_template_items(items);
    }

    pub(crate) fn template_window(&mut self, ctx: &egui::Context) {
        let Some(waiting) = self.template_waiting.take() else { return };
        let mut open = true;
        let mut answer = 0;
        egui::Window::new(tr("Missing Layers")).id(egui::Id::unique("template-layers")).open(&mut open).collapsible(false).resizable(false).default_width(380.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(tr("This template has toolpaths for layers that this job doesn't have:"));
            for name in &waiting.missing {
                ui.label(RichText::new(name).strong());
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Create these layers")).clicked() {
                    answer = 1;
                }
                if ui.button(tr("Remove their toolpaths")).clicked() {
                    answer = 2;
                }
                if ui.button(tr("Load them as they are")).clicked() {
                    answer = 3;
                }
            });
        });
        match answer {
            1 => self.create_template_layers(waiting),
            2 => self.drop_template_items_without_layers(waiting),
            3 => self.add_template_items(waiting.items),
            _ if open => self.template_waiting = Some(waiting),
            _ => {}
        }
    }
}
