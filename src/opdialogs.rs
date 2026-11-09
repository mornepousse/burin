//! The windows a toolpath form opens: pass depths, choosing shapes by filter, and the
//! warning before a cut goes through the material.

use crate::app::*;
use crate::theme;
use crate::toolpath::{self, VectorFilter};
use crate::ui::{bold, number, primary};
use egui::{pos2, vec2, Color32, RichText, Sense, Stroke, Ui};

/// The Edit Passes window while it is open.
#[derive(Clone, Debug)]
pub struct PassesDialog {
    /// Depth of each pass, shallowest first; the last is always the full depth.
    pub list: Vec<f64>,
    pub selected: usize,
    pub depth: f64,
    pub last_on: bool,
    pub last_thickness: f64,
    pub exact: bool,
    pub count: usize,
}

/// What a list of passes looks like after a change: in order, none repeated, ending at full depth.
pub fn tidy_passes(list: &mut Vec<f64>, start: f64, total: f64) {
    list.retain(|d| *d > start + 1e-9 && *d < total - 1e-9);
    list.push(total);
    list.sort_by(f64::total_cmp);
    list.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
}

/// Passes from the tool's pass depth: evenly shared, or exact with a shorter last one.
pub fn passes_by_depth(start: f64, total: f64, pass: f64, exact: bool) -> Vec<f64> {
    toolpath::passes_for(total - start, pass, exact).into_iter().map(|d| start + d).collect()
}

pub fn passes_by_count(start: f64, total: f64, count: usize) -> Vec<f64> {
    let count = count.max(1);
    (1..=count).map(|i| start + (total - start) * i as f64 / count as f64).collect()
}

impl App {
    pub fn open_passes(&mut self, op: &toolpath::Op) {
        let list = toolpath::pass_list(op);
        let count = list.len();
        let depth = list.first().copied().unwrap_or(op.cut_depth);
        self.passes_dialog = Some(PassesDialog { list, selected: 0, depth, last_on: op.last_thickness > 0.0, last_thickness: op.last_thickness, exact: op.keep_exact, count });
    }

    /// Shapes that pass a set of filters, in drawing order.
    pub fn filter_ids(&self, f: &VectorFilter, tool_dia: f64) -> Vec<u64> {
        self.doc
            .vectors
            .iter()
            .filter(|v| !v.construction)
            .filter(|v| {
                let layer_ok = if f.visible_layers { self.doc.shows(v) } else { f.layers.contains(&v.layer) };
                if !layer_ok {
                    return false;
                }
                let closed = v.paths.iter().all(|p| p.closed);
                if !closed {
                    return f.open;
                }
                if !f.closed {
                    return false;
                }
                if !f.only_circles {
                    return true;
                }
                let [only] = v.paths.as_slice() else { return false };
                let Some((_, r)) = crate::bulk::as_circle(only) else { return false };
                let d = r * 2.0;
                match f.circle_match {
                    1 => (d - tool_dia).abs() <= f.tolerance,
                    2 => (d - f.diameter).abs() <= f.tolerance,
                    _ => true,
                }
            })
            .map(|v| v.id)
            .collect()
    }

    /// Asks before calculating a cut deeper than the material.
    pub fn confirm_depth_then_calculate(&mut self) {
        let RightMode::Op { op, .. } = &self.right else { return };
        let deepest = toolpath::deepest_cut(op);
        // With nothing to cut there is only the form's own message to give.
        let has_shapes = op.selector.is_some() || !self.selected.is_empty() || !op.paths.is_empty() || op.kind.uses_model();
        if has_shapes && deepest > self.doc.job.deepest_allowed() + 1e-9 {
            self.through_warning = Some(deepest);
        } else {
            self.calculate_from_form();
        }
    }

    /// The toolpaths that would go through the material as it is now.
    pub fn too_deep_toolpaths(&self, queue: &[usize]) -> Vec<(String, f64)> {
        let allowed = self.doc.job.deepest_allowed();
        queue
            .iter()
            .filter_map(|i| self.doc.toolpaths.get(*i))
            // What its settings say, or what its moves really do where the depth comes from the shape.
            .map(|t| (t.op.name.clone(), toolpath::deepest_cut(&t.op).max(toolpath::deepest_move(&t.moves))))
            .filter(|(_, d)| *d > allowed + 1e-9)
            .collect()
    }

    /// Says so when any of these toolpaths goes through the material. Often that is wanted, to be
    /// sure a part comes free, so it only warns: what is under the material has to be able to take it.
    pub fn warn_if_too_deep(&mut self, queue: &[usize]) {
        let deep = self.too_deep_toolpaths(queue);
        if deep.is_empty() {
            return;
        }
        let (unit, thick, board, allowed) = (self.doc.job.unit(), self.doc.job.thickness, self.doc.job.spoil_board, self.doc.job.deepest_allowed());
        let what = if board > 0.0 { crate::i18n::tr("the sacrificial board") } else { crate::i18n::tr("the material") };
        const SHOWN: usize = 12;
        let mut list: Vec<String> = deep.iter().take(SHOWN).map(|(name, d)| tf!("{name}: cuts to {} {unit}, {} {unit} past {what}", toolpath::trim(*d), toolpath::trim(*d - allowed))).collect();
        if deep.len() > SHOWN {
            list.push(tf!("and {} more", deep.len() - SHOWN));
        }
        let (heading, sizes, advice) = if board > 0.0 {
            (
                crate::i18n::tr("Tool will cut through the sacrificial board"),
                tf!("Material: {} {unit} + board: {} {unit} = {} {unit}", toolpath::trim(thick), toolpath::trim(board), toolpath::trim(allowed)),
                crate::i18n::tr("Nothing is left under the tool here: it can damage the machine bed."),
            )
        } else {
            (
                crate::i18n::tr("Tool will cut through the material"),
                tf!("Material thickness: {} {unit}", toolpath::trim(thick)),
                crate::i18n::tr("Check that a sacrificial board is under the material and can take this depth. Without one, the tool can damage the machine bed. If there is one, enter its thickness in the job's settings."),
            )
        };
        self.message = Some(format!("{heading}\n\n{sizes}\n\n{}\n\n{advice}", list.join("\n")));
    }

    pub(crate) fn op_windows(&mut self, ctx: &egui::Context) {
        self.through_window(ctx);
        self.passes_window(ctx);
        self.selector_window(ctx);
    }

    fn through_window(&mut self, ctx: &egui::Context) {
        let Some(deepest) = self.through_warning else { return };
        let (unit, thick, board, allowed) = (self.doc.job.unit(), self.doc.job.thickness, self.doc.job.spoil_board, self.doc.job.deepest_allowed());
        let mut answer = None;
        let title = if board > 0.0 { crate::i18n::tr("Tool will cut through the sacrificial board") } else { crate::i18n::tr("Tool will cut through the material") };
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_width(330.0);
            let headline = if board > 0.0 { crate::i18n::tr("This toolpath goes deeper than the material and its board.") } else { crate::i18n::tr("This toolpath goes deeper than the material is thick.") };
            ui.label(RichText::new(headline).strong().color(theme::ERROR));
            ui.add_space(6.0);
            ui.label(tf!("Material thickness: {} {unit}", toolpath::trim(thick)));
            if board > 0.0 {
                ui.label(tf!("Sacrificial board: {} {unit}, {} {unit} in all", toolpath::trim(board), toolpath::trim(allowed)));
            }
            ui.label(tf!("Deepest cut: {} {unit}", toolpath::trim(deepest)));
            ui.add_space(6.0);
            ui.label(if board > 0.0 {
                crate::i18n::tr("Nothing is left under the tool here: it can damage the machine bed.")
            } else {
                crate::i18n::tr("Without a sacrificial layer under the material, the tool can damage the machine bed. If there is one, enter its thickness in the job's settings.")
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Calculate Anyway")).clicked() {
                    answer = Some(true);
                }
                ui.add_space((ui.available_width() - 74.0).max(0.0));
                if primary(ui, crate::i18n::tr("Cancel"), 74.0).clicked() {
                    answer = Some(false);
                }
            });
        });
        match answer {
            Some(true) => {
                self.through_warning = None;
                self.calculate_from_form();
            }
            Some(false) => self.through_warning = None,
            None => {}
        }
    }

    fn passes_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.passes_dialog.clone() else { return };
        let RightMode::Op { op, .. } = &self.right else {
            self.passes_dialog = None;
            return;
        };
        let (start, total, tool_pass) = (op.start_depth, op.cut_depth, op.tool.pass_depth);
        let unit = self.doc.job.unit();
        let mut close = None;
        egui::Window::new(crate::i18n::tr("Edit Passes")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                bold(ui, crate::i18n::tr("Total depth"));
                ui.label(format!("{} {unit}", toolpath::trim(total)));
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                pass_bar(ui, &mut d, start, total);
                egui::ScrollArea::vertical().id_salt("pass_list").max_height(170.0).min_scrolled_width(62.0).show(ui, |ui| {
                    ui.set_width(62.0);
                    ui.vertical(|ui| {
                        for i in 0..d.list.len() {
                            if ui.selectable_label(d.selected == i, toolpath::trim(d.list[i])).clicked() {
                                d.selected = i;
                                d.depth = d.list[i];
                            }
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Double click the bar to add or remove a pass. Click it, or the list, to choose one.")).size(11.5).color(theme::MUTED()));
                    ui.horizontal(|ui| {
                        ui.label(tf!("Pass {}", d.selected + 1));
                        ui.label(crate::i18n::tr("Depth:"));
                        number(ui, &mut d.depth, 54.0);
                        ui.label(unit);
                    });
                    let before = if d.selected == 0 { start } else { d.list[d.selected - 1] };
                    ui.label(tf!("From previous: {} {unit}", toolpath::trim((d.depth - before).max(0.0))));
                    ui.horizontal(|ui| {
                        // The full depth is the job of the form, so the last pass stays.
                        if ui.add_enabled(d.selected + 1 < d.list.len(), egui::Button::new(crate::i18n::tr("Delete"))).clicked() {
                            d.list.remove(d.selected);
                            d.selected = d.selected.min(d.list.len() - 1);
                            d.depth = d.list[d.selected];
                        }
                        if ui.add_enabled(d.selected + 1 < d.list.len(), egui::Button::new(crate::i18n::tr("Apply Depth"))).clicked() {
                            let value = d.depth;
                            d.list[d.selected] = value;
                            tidy_passes(&mut d.list, start, total);
                            d.selected = d.list.iter().position(|v| (*v - value).abs() < 1e-9).unwrap_or(0);
                            d.depth = d.list[d.selected];
                        }
                    });
                    if ui.button(crate::i18n::tr("Clear All Passes")).clicked() {
                        d.list = vec![total];
                        d.selected = 0;
                        d.depth = total;
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.last_on, crate::i18n::tr("Set Last Pass Thickness"));
                ui.add_enabled_ui(d.last_on, |ui| {
                    number(ui, &mut d.last_thickness, 50.0);
                    ui.label(unit);
                    if ui.button(crate::i18n::tr("Apply Thickness")).clicked() && d.last_thickness > 0.0 && d.last_thickness < total - start {
                        d.list = toolpath::passes_with_last(total - start, tool_pass, d.last_thickness).into_iter().map(|v| start + v).collect();
                        d.selected = 0;
                        d.depth = d.list[0];
                    }
                });
            });
            ui.separator();
            bold(ui, crate::i18n::tr("Fill the list"));
            let by_depth = passes_by_depth(start, total, tool_pass, d.exact);
            ui.label(RichText::new(tf!("From the tool's pass depth of {} {unit}: {} passes", toolpath::trim(tool_pass), by_depth.len())).size(12.0));
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.exact, crate::i18n::tr("Keep exactly to the tool's pass depth"));
                ui.add_space((ui.available_width() - 76.0).max(0.0));
                if ui.button(crate::i18n::tr("Use Tool Depth")).clicked() {
                    d.list = by_depth.clone();
                    d.selected = 0;
                    d.depth = d.list[0];
                }
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Number of passes"));
                crate::ui::count_box(ui, &mut d.count, 44.0);
                ui.label(RichText::new(tf!("{} {unit} each", toolpath::trim((total - start) / d.count.max(1) as f64))).size(12.0).color(theme::MUTED()));
                ui.add_space((ui.available_width() - 76.0).max(0.0));
                if ui.button(crate::i18n::tr("Use Number")).clicked() {
                    d.list = passes_by_count(start, total, d.count);
                    d.selected = 0;
                    d.depth = d.list[0];
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if primary(ui, "OK", 74.0).clicked() {
                    close = Some(true);
                }
                ui.add_space((ui.available_width() - 76.0).max(0.0));
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    close = Some(false);
                }
            });
        });
        match close {
            Some(keep) => {
                if keep {
                    if let RightMode::Op { op, .. } = &mut self.right {
                        // A list the tool's pass depth gives anyway need not be kept.
                        let auto = passes_by_depth(start, total, tool_pass, false);
                        op.passes = if d.list == auto { Vec::new() } else { d.list.clone() };
                        // So that a later change of depth works out passes the same way.
                        op.last_thickness = if d.last_on { d.last_thickness } else { 0.0 };
                        op.keep_exact = d.exact;
                    }
                }
                self.passes_dialog = None;
            }
            None => self.passes_dialog = Some(d),
        }
    }

    pub fn open_selector(&mut self, op: &toolpath::Op) {
        let metric = self.doc.job.unit() == "mm";
        let mut f = op.selector.clone().unwrap_or_else(|| VectorFilter { diameter: op.tool.dia, tolerance: if metric { 0.1 } else { 0.004 }, ..VectorFilter::default() });
        if f.layers.is_empty() {
            f.layers = (0..self.doc.layers.len()).collect();
        }
        self.selector_dialog = Some(f);
    }

    fn selector_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.selector_dialog.clone() else { return };
        let RightMode::Op { op, .. } = &self.right else {
            self.selector_dialog = None;
            return;
        };
        let tool_dia = op.tool.dia;
        let unit = self.doc.job.unit();
        let names: Vec<String> = self.doc.layers.iter().map(|l| l.name.clone()).collect();
        let before = f.clone();
        let mut close = false;
        egui::Window::new(crate::i18n::tr("Vector Selector")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_width(470.0);
            ui.label(RichText::new(crate::i18n::tr("Selects shapes by filter. With Associate, the filter is applied again each time the toolpath is calculated.")).size(11.5).color(theme::MUTED()));
            ui.horizontal(|ui| {
                bold(ui, crate::i18n::tr("Selection:"));
                ui.label(match self.selected.len() {
                    0 => crate::i18n::tr("No objects selected").to_string(),
                    1 => crate::i18n::tr("1 object selected").to_string(),
                    n => tf!("{n} objects selected"),
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    bold(ui, crate::i18n::tr("Type of vectors to select ..."));
                    ui.checkbox(&mut f.open, crate::i18n::tr("Open Vectors"));
                    ui.checkbox(&mut f.closed, crate::i18n::tr("Closed Vectors ..."));
                    ui.add_enabled_ui(f.closed, |ui| {
                        ui.indent("closed", |ui| {
                            ui.radio_value(&mut f.only_circles, false, crate::i18n::tr("All Closed Vectors"));
                            ui.radio_value(&mut f.only_circles, true, crate::i18n::tr("Only Circles"));
                            ui.add_enabled_ui(f.only_circles, |ui| {
                                ui.indent("circles", |ui| {
                                    ui.radio_value(&mut f.circle_match, 0, crate::i18n::tr("Select ALL Circles"));
                                    ui.radio_value(&mut f.circle_match, 1, crate::i18n::tr("Select Circles Matching Tool Diameter"));
                                    ui.radio_value(&mut f.circle_match, 2, crate::i18n::tr("Select Circles Matching ..."));
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::tr("Circle Diameter"));
                                        number(ui, &mut f.diameter, 54.0);
                                        ui.label(unit);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::tr("Tolerance"));
                                        number(ui, &mut f.tolerance, 54.0);
                                        ui.label(unit);
                                    });
                                });
                            });
                        });
                    });
                    ui.add_space(4.0);
                    ui.checkbox(&mut f.associate, crate::i18n::tr("Associate with toolpath"));
                });
                // A separator here would stretch to the window's height and make it grow every frame.
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    bold(ui, crate::i18n::tr("Select vectors on ..."));
                    ui.radio_value(&mut f.visible_layers, true, crate::i18n::tr("All visible layers"));
                    ui.radio_value(&mut f.visible_layers, false, crate::i18n::tr("Selected layers only"));
                    ui.add_enabled_ui(!f.visible_layers, |ui| {
                        egui::ScrollArea::vertical().id_salt("selector_layers").max_height(180.0).show(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                let mut on = f.layers.contains(&i);
                                if ui.checkbox(&mut on, name.as_str()).changed() {
                                    if on {
                                        f.layers.push(i);
                                    } else {
                                        f.layers.retain(|l| *l != i);
                                    }
                                }
                            }
                        });
                    });
                });
            });
            ui.separator();
            ui.vertical_centered(|ui| {
                if primary(ui, crate::i18n::tr("Close"), 74.0).clicked() {
                    close = true;
                }
            });
        });
        // The selection follows the filters as they change, once there is one to follow.
        if f != before && (f.open || f.closed) {
            self.selected = self.filter_ids(&f, tool_dia);
        }
        if close {
            if let RightMode::Op { op, .. } = &mut self.right {
                op.selector = f.associate.then(|| f.clone());
            }
            self.selector_dialog = None;
        } else {
            self.selector_dialog = Some(f);
        }
    }
}

/// The piece of wood seen from the side, with a line at each pass.
fn pass_bar(ui: &mut Ui, d: &mut PassesDialog, start: f64, total: f64) {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 170.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgb(230, 178, 110));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
    let y_of = |v: f64| rect.top() + ((v - start) / (total - start).max(1e-9)) as f32 * rect.height();
    for (i, v) in d.list.iter().enumerate() {
        let y = y_of(*v);
        let colour = if i == d.selected { Color32::from_rgb(210, 30, 30) } else { Color32::BLACK };
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(if i == d.selected { 2.0 } else { 1.0 }, colour));
    }
    let Some(at) = resp.interact_pointer_pos() else { return };
    let depth = start + (((at.y - rect.top()) / rect.height()).clamp(0.0, 1.0) as f64) * (total - start);
    let nearest = (0..d.list.len()).min_by(|a, b| (d.list[*a] - depth).abs().total_cmp(&(d.list[*b] - depth).abs())).unwrap_or(0);
    let near = (y_of(d.list[nearest]) - at.y).abs() < 5.0;
    if resp.double_clicked() {
        if near && nearest + 1 < d.list.len() {
            d.list.remove(nearest);
        } else if !near {
            d.list.push((depth * 1000.0).round() / 1000.0);
            tidy_passes(&mut d.list, start, total);
        }
        d.selected = d.selected.min(d.list.len() - 1);
        d.depth = d.list[d.selected];
    } else if resp.clicked() {
        d.selected = nearest;
        d.depth = d.list[nearest];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{circle, rect, Path};

    #[test]
    fn a_list_of_passes_stays_ordered_and_ends_at_full_depth() {
        let mut l = vec![0.6, 0.2, 0.2, 1.4, 0.0, 1.0];
        tidy_passes(&mut l, 0.0, 1.0);
        assert_eq!(l, [0.2, 0.6, 1.0]);
    }

    #[test]
    fn passes_fill_by_the_tools_depth_or_by_number() {
        assert_eq!(passes_by_count(0.0, 1.0, 4), [0.25, 0.5, 0.75, 1.0]);
        let even = passes_by_depth(0.0, 1.0, 0.3, false);
        assert_eq!(even.len(), 4);
        assert!((even[0] - 0.25).abs() < 1e-9, "shared evenly: {even:?}");
        let exact = passes_by_depth(0.0, 1.0, 0.3, true);
        assert!((exact[0] - 0.3).abs() < 1e-9 && (exact[2] - 0.9).abs() < 1e-9 && exact[3] == 1.0, "{exact:?}");
        // From a start depth too.
        assert_eq!(passes_by_count(1.0, 3.0, 2), [2.0, 3.0]);
    }

    #[test]
    fn filters_choose_shapes_by_kind_size_and_layer() {
        let mut a = App::default();
        a.new_file();
        a.doc.layers.push(Layer::named("deux"));
        let hole = a.doc.add_vector(vec![Path::closed(circle([2.0, 2.0], 0.125))], Prim::Free);
        let big = a.doc.add_vector(vec![Path::closed(circle([5.0, 5.0], 1.0))], Prim::Free);
        let plate = a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 8.0, 8.0, 0.0))], Prim::Free);
        a.doc.active_layer = 1;
        let line = a.doc.add_vector(vec![Path::open(vec![[0.0, 0.0], [1.0, 1.0]])], Prim::Free);
        a.doc.active_layer = 0;
        let base = VectorFilter { closed: false, open: false, ..VectorFilter::default() };
        let check = |a: &App, f: &VectorFilter, want: &[u64]| assert_eq!(a.filter_ids(f, 0.25), want, "{f:?}");
        check(&a, &base, &[]);
        check(&a, &VectorFilter { open: true, ..base.clone() }, &[line]);
        check(&a, &VectorFilter { closed: true, ..base.clone() }, &[hole, big, plate]);
        let circles = VectorFilter { closed: true, only_circles: true, ..base.clone() };
        check(&a, &circles, &[hole, big]);
        check(&a, &VectorFilter { circle_match: 1, tolerance: 0.01, ..circles.clone() }, &[hole]);
        check(&a, &VectorFilter { circle_match: 2, diameter: 2.0, tolerance: 0.01, ..circles.clone() }, &[big]);
        // Only some layers.
        let on_second = VectorFilter { open: true, closed: true, visible_layers: false, layers: vec![1], ..base.clone() };
        check(&a, &on_second, &[line]);
        // Hidden layers are left out of "all visible", construction lines always.
        a.doc.layers[1].visible = false;
        check(&a, &VectorFilter { open: true, closed: true, ..base.clone() }, &[hole, big, plate]);
        a.doc.vectors.iter_mut().find(|v| v.id == big).unwrap().construction = true;
        check(&a, &VectorFilter { open: true, closed: true, ..base }, &[hole, plate]);
    }
}
