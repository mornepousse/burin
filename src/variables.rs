//! The Document Variables window (Edit > Document Variables, the original's command 33702): named values,
//! saved with the job, that sums in number boxes use as `{name}`. Changing one works the toolpaths' depths out
//! again; the original then warns which toolpaths use the variables edited and need recalculating
//! (`0x14062df6a`), and Burin offers to recalculate them.
//!
//! Export Variables and Import Variables use the original's file, "Document Variables (*.DocVars)": an INI
//! file with `Version=1` under `[DocumentVariables]`, then one `name=value` line per variable under
//! `[Variables]` (written by `0x14061e960`). Reading it (`0x14061ef30`) refuses a version above 1, then sets
//! each variable by name: one the document already has takes the file's value, the others are added.

use crate::app::App;
use crate::formula::{self, Variable};
use crate::i18n::tr;
use egui::Align2;

/// What the window is editing: a copy, put back when it's applied.
#[derive(Clone, Debug, Default)]
pub struct VariablesForm {
    pub rows: Vec<(String, f64)>,
}

/// The original's extension for a file of document variables.
pub const DOCVARS_EXTENSION: &str = "DocVars";
/// The newest version of that file the original reads.
pub const DOCVARS_VERSION: u32 = 1;

/// The variables as a `.DocVars` file.
pub fn docvars_text(rows: &[(String, f64)]) -> String {
    let mut out = format!("[DocumentVariables]\nVersion={DOCVARS_VERSION}\n\n[Variables]\n");
    for (name, value) in rows {
        out.push_str(&format!("{}={value}\n", name.trim()));
    }
    out
}

/// Reads a `.DocVars` file into the rows, as the original does: same name, new value; new names added at the
/// end. Returns how many were left out for a name that can't be used.
pub fn read_docvars(text: &str, rows: &mut Vec<(String, f64)>) -> Result<usize, String> {
    let mut section = String::new();
    let mut version = None;
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { return Err(crate::i18n::tr("This isn't a file of document variables.").into()) };
        let (key, value) = (key.trim(), value.trim());
        match section.as_str() {
            "DocumentVariables" if key == "Version" => version = value.parse::<u32>().ok(),
            "Variables" => {
                let v = value.parse::<f64>().map_err(|_| tf!("{} has no number: {}", key, value))?;
                found.push((key.to_string(), v));
            }
            _ => {}
        }
    }
    match version {
        Some(v) if v <= DOCVARS_VERSION => {}
        Some(_) => return Err(crate::i18n::tr("This file of document variables is from a newer version.").into()),
        None => return Err(crate::i18n::tr("This isn't a file of document variables.").into()),
    }
    let mut skipped = 0;
    for (name, value) in found {
        // As `0x14061de10`: a name that can't be one (`0x14061d330`), or the largest number, the original's
        // mark for "no value" (`0x142172598`), is left out.
        if !formula::valid_name(&name) || value == NO_VALUE {
            skipped += 1;
            continue;
        }
        match rows.iter_mut().find(|(n, _)| n.trim() == name) {
            Some(row) => row.1 = value,
            None => rows.push((name, value)),
        }
    }
    Ok(skipped)
}

/// The value the original takes for "none", never kept as a variable.
pub const NO_VALUE: f64 = 1.7976931348623157e308;

impl App {
    /// What sums can use now.
    pub fn formula_context(&self) -> formula::Context {
        let job = &self.doc.job;
        formula::Context { variables: self.doc.variables.clone(), width: job.width, height: job.height, thickness: job.thickness, metric: job.units == crate::app::Units::Mm }
    }

    pub fn open_variables(&mut self) {
        self.variables_form = Some(VariablesForm { rows: self.doc.variables.iter().map(|v| (v.name.clone(), v.value)).collect() });
    }

    /// What's wrong with the names in the form, if anything.
    pub fn variables_problem(rows: &[(String, f64)]) -> Option<String> {
        for (i, (name, _)) in rows.iter().enumerate() {
            let name = name.trim();
            if !formula::valid_name(name) {
                return Some(tf!("\"{}\" can't be a name: start with a letter, then use letters, digits or _.", name));
            }
            if rows[..i].iter().any(|(n, _)| n.trim() == name) {
                return Some(tf!("There are two variables called {}.", name));
            }
        }
        None
    }

    /// Keeps the variables, and works out again the depths that use them. Returns the names of the
    /// toolpaths whose depths changed, which need recalculating.
    pub fn set_variables(&mut self, variables: Vec<Variable>) -> Result<Vec<String>, String> {
        let changed: Vec<String> = variables
            .iter()
            .filter(|v| self.doc.variables.iter().find(|o| o.name == v.name).is_none_or(|o| o.value != v.value))
            .map(|v| v.name.clone())
            .chain(self.doc.variables.iter().filter(|o| !variables.iter().any(|v| v.name == o.name)).map(|o| o.name.clone()))
            .collect();
        let mut context = self.formula_context();
        context.variables = variables.clone();
        // All or nothing: a toolpath whose sum would break keeps the variables as they were.
        let mut ops = Vec::new();
        for t in &self.doc.toolpaths {
            let mut op = t.op.clone();
            let moved = t.op.formulas.apply(&mut op, &context).map_err(|e| tf!("{}: {}", t.op.name, e.text()))?;
            ops.push((op, moved));
        }
        self.checkpoint();
        self.doc.variables = variables;
        let mut behind = Vec::new();
        for (t, (op, moved)) in self.doc.toolpaths.iter_mut().zip(ops) {
            let uses = changed.iter().any(|name| t.op.formulas.uses(name));
            t.op = op;
            if moved || uses {
                behind.push(t.op.name.clone());
            }
        }
        self.dirty = true;
        Ok(behind)
    }

    pub(crate) fn variables_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.variables_form.take() else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        egui::Window::new(tr("Document Variables")).id(egui::Id::unique("document-variables")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(egui::RichText::new(tr("Type {name} in any number box to use a value. Sums can also use W, H and T for the job's width, height and thickness.")).color(crate::theme::MUTED()));
            ui.add_space(6.0);
            let mut remove = None;
            egui::Grid::new("variables-grid").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(egui::RichText::new(tr("Name")).strong());
                ui.label(egui::RichText::new(tr("Value")).strong());
                ui.end_row();
                for (i, (name, value)) in form.rows.iter_mut().enumerate() {
                    ui.push_id(i, |ui| ui.add_sized([140.0, 20.0], egui::TextEdit::singleline(name)));
                    ui.push_id(("value", i), |ui| ui.add(egui::DragValue::new(value).speed(0.01).max_decimals(4)));
                    if ui.push_id(("remove", i), |ui| crate::ui::icon_button(ui, crate::icons::Icon::Delete, tr("Delete"), true, false)).inner.clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                form.rows.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button(tr("Add a Variable")).clicked() {
                    let mut n = form.rows.len() + 1;
                    while form.rows.iter().any(|(name, _)| *name == format!("v{n}")) {
                        n += 1;
                    }
                    form.rows.push((format!("v{n}"), 0.0));
                }
                if ui.button(tr("Import Variables ...")).clicked() {
                    if let Some(path) = crate::platform::FileDialog::new().add_filter(tr("Document Variables"), &[DOCVARS_EXTENSION]).pick_file() {
                        match crate::platform::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| read_docvars(&t, &mut form.rows)) {
                            Ok(0) => {}
                            Ok(skipped) => self.message = Some(tf!("{} variables in the file were left out: a name must start with a letter, then use letters, digits or _.", skipped)),
                            Err(why) => self.message = Some(tf!("Could not read {}: {}", path.display(), why)),
                        }
                    }
                }
                if ui.add_enabled(!form.rows.is_empty(), egui::Button::new(tr("Export Variables ..."))).clicked() {
                    let name = format!("{}.{DOCVARS_EXTENSION}", tr("Variables"));
                    if let Some(path) = crate::platform::FileDialog::new().add_filter(tr("Document Variables"), &[DOCVARS_EXTENSION]).set_file_name(name).save_file() {
                        if let Err(e) = crate::platform::write(&path, docvars_text(&form.rows)) {
                            self.message = Some(tf!("Could not write {}: {e}", path.display()));
                        }
                    }
                }
            });
            ui.add_space(6.0);
            let problem = App::variables_problem(&form.rows);
            if let Some(p) = &problem {
                ui.label(egui::RichText::new(p).color(crate::theme::WARNING));
            }
            ui.horizontal(|ui| {
                apply = ui.add_enabled(problem.is_none(), egui::Button::new(tr("OK"))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if apply {
            let variables = form.rows.iter().map(|(n, v)| Variable { name: n.trim().to_string(), value: *v }).collect();
            match self.set_variables(variables) {
                Ok(behind) if !behind.is_empty() => {
                    self.recalc_note = Some(tf!("These toolpaths use document variables that have changed:\n{}", behind.join(", ")));
                    self.ask_recalc = true;
                }
                Ok(_) => {}
                Err(why) => {
                    self.message = Some(why);
                    self.variables_form = Some(form);
                }
            }
        } else if open && !cancel {
            self.variables_form = Some(form);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_docvars_file_goes_out_and_back_in() {
        let rows = vec![("thick".to_string(), 0.75), ("tab_len".to_string(), 0.1)];
        let text = docvars_text(&rows);
        assert_eq!(text, "[DocumentVariables]\nVersion=1\n\n[Variables]\nthick=0.75\ntab_len=0.1\n");
        let mut mine = vec![("thick".to_string(), 1.0), ("width".to_string(), 3.0)];
        assert_eq!(read_docvars(&text, &mut mine), Ok(0));
        assert_eq!(mine, vec![("thick".to_string(), 0.75), ("width".to_string(), 3.0), ("tab_len".to_string(), 0.1)], "same name takes the file's value");
    }

    #[test]
    fn a_docvars_file_must_say_its_version() {
        let mut rows = Vec::new();
        assert!(read_docvars("[Variables]\na=1\n", &mut rows).is_err());
        assert!(read_docvars("[DocumentVariables]\nVersion=2\n[Variables]\na=1\n", &mut rows).is_err());
        assert!(read_docvars("[DocumentVariables]\nVersion=1\n[Variables]\na=x\n", &mut rows).is_err());
        assert!(rows.is_empty(), "nothing taken from a file that's refused");
        assert_eq!(read_docvars("\u{feff}[DocumentVariables]\r\nVersion=1\r\n[Variables]\r\na = 2.5\r\n", &mut rows), Ok(0));
        assert_eq!(rows, vec![("a".to_string(), 2.5)]);
        let odd = format!("[DocumentVariables]\nVersion=1\n[Variables]\n2nd=1\nb_1=2\nc={}\n", f64::MAX);
        assert_eq!(read_docvars(&odd, &mut rows), Ok(2), "a bad name and the no-value mark are left out");
        assert_eq!(rows, vec![("a".to_string(), 2.5), ("b_1".to_string(), 2.0)]);
    }

    #[test]
    fn names_must_be_good_and_different() {
        assert!(App::variables_problem(&[("depth".into(), 1.0), ("width2".into(), 2.0)]).is_none());
        assert!(App::variables_problem(&[("2nd".into(), 1.0)]).is_some());
        assert!(App::variables_problem(&[("a".into(), 1.0), ("a".into(), 2.0)]).is_some());
    }
}
