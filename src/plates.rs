//! Plates from data: the selected design laid out again and again on the material, in rows and
//! columns within margins, each plate's texts filled from a line of data (typed in or imported from
//! a file) or numbered in turn. For name plates, labels, tags: one design, many plates.
//! Plates that don't fit on the sheet go on more sheets.

use crate::app::*;
use crate::geom::{self, P};
use crate::i18n::tr;
use egui::{Align2, RichText};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Separator {
    Comma,
    Tab,
    Semicolon,
    Space,
}

impl Separator {
    fn split(self, line: &str) -> Vec<String> {
        let parts: Vec<&str> = match self {
            Separator::Comma => line.split(',').collect(),
            Separator::Tab => line.split('\t').collect(),
            Separator::Semicolon => line.split(';').collect(),
            Separator::Space => line.split_whitespace().collect(),
        };
        parts.iter().map(|p| p.trim().trim_matches('"').to_string()).collect()
    }
}

/// What one of the design's texts becomes on each plate.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// The same on every plate.
    Same,
    /// The value of this column of the plate's line of data.
    Column(usize),
    /// A number counting up from plate to plate.
    Counter { start: f64, step: f64, digits: usize, decimals: usize },
}

#[derive(Clone, Debug)]
pub struct PlatesForm {
    /// Top, left, right, bottom.
    pub margins: [f64; 4],
    pub equal_margins: bool,
    /// Across, up and down.
    pub gaps: [f64; 2],
    pub across: usize,
    pub down: usize,
    /// As many as fit.
    pub auto_count: bool,
    pub data: String,
    pub separator: Separator,
    pub titles: bool,
    /// The design's texts (by shape id, with what they say now) and what each becomes.
    pub texts: Vec<(u64, String, Fill)>,
    pub chosen_text: usize,
}

impl PlatesForm {
    pub fn new(app: &App) -> Self {
        let texts = app
            .doc
            .vectors
            .iter()
            .filter(|v| app.selected.contains(&v.id))
            .filter_map(|v| match &v.prim {
                Prim::Text { text, .. } => Some((v.id, text.clone(), Fill::Same)),
                _ => None,
            })
            .collect();
        PlatesForm { margins: [0.0; 4], equal_margins: true, gaps: [0.0; 2], across: 1, down: 1, auto_count: true, data: String::new(), separator: Separator::Comma, titles: false, texts, chosen_text: 0 }
    }

    /// The lines of data, and the column names (the first line's, or 1, 2, 3...).
    pub fn rows(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let mut lines: Vec<Vec<String>> = self.data.lines().filter(|l| !l.trim().is_empty()).map(|l| self.separator.split(l)).collect();
        let width = lines.iter().map(Vec::len).max().unwrap_or(0);
        let names = if self.titles && !lines.is_empty() {
            let mut first = lines.remove(0);
            first.resize(width, String::new());
            first
        } else {
            (1..=width).map(|k| k.to_string()).collect()
        };
        (names, lines)
    }
}

/// The counter's text for plate `k`.
pub fn counted(start: f64, step: f64, digits: usize, decimals: usize, k: usize) -> String {
    let v = start + step * k as f64;
    let text = format!("{:.*}", decimals, v.abs());
    let (whole, rest) = text.split_once('.').map_or((text.as_str(), None), |(a, b)| (a, Some(b)));
    let padded = format!("{}{:0>width$}", if v < 0.0 { "-" } else { "" }, whole, width = digits.max(1));
    match rest {
        Some(r) => format!("{padded}.{r}"),
        None => padded,
    }
}

/// Where the plates go: how many across and down fit, and each plate's bottom-left corner and sheet.
pub fn layout(sheet: [f64; 2], plate: [f64; 2], f: &PlatesForm, count: usize) -> (usize, usize, Vec<(P, usize)>) {
    let [top, left, right, bottom] = f.margins;
    let fit = |room: f64, size: f64, gap: f64| if size <= 0.0 { 1 } else { (((room + gap) / (size + gap)).floor().max(0.0)) as usize };
    let (across, down) = if f.auto_count {
        (fit(sheet[0] - left - right, plate[0], f.gaps[0]).max(1), fit(sheet[1] - top - bottom, plate[1], f.gaps[1]).max(1))
    } else {
        (f.across.max(1), f.down.max(1))
    };
    let per_sheet = across * down;
    let spots = (0..count)
        .map(|k| {
            let (i, j) = ((k % per_sheet) % across, (k % per_sheet) / across);
            let x = left + i as f64 * (plate[0] + f.gaps[0]);
            let y = sheet[1] - top - plate[1] - j as f64 * (plate[1] + f.gaps[1]);
            ([x, y], k / per_sheet)
        })
        .collect();
    (across, down, spots)
}

impl App {
    pub(crate) fn open_plates(&mut self) {
        if self.selected.is_empty() {
            self.status = tr("Select the design of one plate first.").into();
            return;
        }
        self.plates = Some(PlatesForm::new(self));
    }

    /// How many plates the form makes: one per line of data, or as many as fit.
    fn plate_count(&self, f: &PlatesForm, plate: [f64; 2]) -> usize {
        let (_, rows) = f.rows();
        if !rows.is_empty() {
            return rows.len();
        }
        let (across, down, _) = layout([self.doc.job.width, self.doc.job.height], plate, f, 0);
        across * down
    }

    /// Lays the plates out and fills their texts.
    pub(crate) fn make_plates(&mut self, f: &PlatesForm) {
        let template: Vec<Vector> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).cloned().collect();
        let Some((lo, hi)) = geom::bbox(template.iter().flat_map(|v| v.paths.iter())) else { return };
        let plate = [hi[0] - lo[0], hi[1] - lo[1]];
        let count = self.plate_count(f, plate).min(2000);
        let (_, rows) = f.rows();
        let (_, _, spots) = layout([self.doc.job.width, self.doc.job.height], plate, f, count);
        let sheets_needed = spots.iter().map(|s| s.1).max().unwrap_or(0) + self.doc.active_sheet as usize + 1;
        if sheets_needed > u8::MAX as usize {
            self.status = tr("Too many plates for the sheets Burin can hold.").into();
            return;
        }
        self.checkpoint();
        while self.doc.sheet_count() < sheets_needed {
            self.doc.extra_sheets += 1;
        }
        let text_of = |id: u64, k: usize| -> Option<String> {
            let (_, now, fill) = f.texts.iter().find(|t| t.0 == id)?;
            Some(match fill {
                Fill::Same => now.clone(),
                Fill::Column(c) => rows.get(k).and_then(|r| r.get(*c)).cloned().unwrap_or_default(),
                Fill::Counter { start, step, digits, decimals } => counted(*start, *step, *digits, *decimals, k),
            })
        };
        let mut made = Vec::new();
        for (k, (at, sheet)) in spots.iter().enumerate() {
            let (dx, dy) = (at[0] - lo[0], at[1] - lo[1]);
            for t in &template {
                let mut v = t.clone();
                if k > 0 {
                    v.id = self.doc.new_id();
                }
                for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                    *p = [p[0] + dx, p[1] + dy];
                }
                v.prim.translate(dx, dy);
                v.sheet = self.doc.active_sheet + *sheet as u8;
                if let (Some(new), Prim::Text { text, .. }) = (text_of(t.id, k), &mut v.prim) {
                    *text = new;
                    v.paths = v.prim.paths();
                }
                made.push(v.id);
                match self.doc.vectors.iter_mut().find(|x| x.id == v.id) {
                    Some(slot) if k == 0 => *slot = v,
                    _ => self.doc.vectors.push(v),
                }
            }
        }
        self.doc.sort_out_copied_pieces(&made);
        self.selected = made;
        self.status = tf!("{} plates made.", count);
        self.note_changed_shapes();
    }

    pub(crate) fn plates_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.plates.take() else { return };
        let unit = self.doc.job.unit();
        let template: Vec<&Vector> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).collect();
        let plate = geom::bbox(template.iter().flat_map(|v| v.paths.iter())).map_or([0.0; 2], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1]]);
        let mut open = true;
        let mut make = false;
        egui::Window::new(tr("Plates from Data")).id(egui::Id::unique("plates")).open(&mut open).collapsible(false).resizable(false).default_width(720.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.label(RichText::new(tr("Material size")).strong());
                ui.label(format!("{} × {} {unit}", crate::toolpath::trim(self.doc.job.width), crate::toolpath::trim(self.doc.job.height)));
                ui.label(RichText::new(tr("Plate size (the selected design)")).strong());
                ui.label(format!("{} × {} {unit}", crate::toolpath::trim(plate[0]), crate::toolpath::trim(plate[1])));
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Margins")).strong());
                ui.checkbox(&mut f.equal_margins, tr("Equal margins"));
                let names = [tr("Top"), tr("Left"), tr("Right"), tr("Bottom")];
                for (k, name) in names.iter().enumerate() {
                    if f.equal_margins && k > 0 {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(if f.equal_margins { tr("All sides") } else { name });
                        crate::ui::number(ui, &mut f.margins[k], 60.0);
                        ui.label(unit);
                    });
                }
                if f.equal_margins {
                    f.margins = [f.margins[0]; 4];
                }
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Gap between plates")).strong());
                ui.horizontal(|ui| {
                    ui.label(tr("Across"));
                    crate::ui::number(ui, &mut f.gaps[0], 54.0);
                    ui.label(tr("Up and down"));
                    crate::ui::number(ui, &mut f.gaps[1], 54.0);
                    ui.label(unit);
                });
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Number of plates")).strong());
                ui.checkbox(&mut f.auto_count, tr("As many as fit"));
                let (across, down, _) = layout([self.doc.job.width, self.doc.job.height], plate, &f, 0);
                if f.auto_count {
                    (f.across, f.down) = (across, down);
                }
                ui.add_enabled_ui(!f.auto_count, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr("Across"));
                        crate::ui::count_box(ui, &mut f.across, 40.0);
                        ui.label(tr("Down"));
                        crate::ui::count_box(ui, &mut f.down, 40.0);
                    });
                });
                let count = self.plate_count(&f, plate);
                let per_sheet = (f.across * f.down).max(1);
                ui.label(RichText::new(tf!("{} plates, on {} sheet(s).", count, count.div_ceil(per_sheet).max(1))).color(crate::theme::MUTED()));

                let ui = &mut cols[1];
                ui.label(RichText::new(tr("Data to engrave")).strong());
                ui.add(egui::TextEdit::multiline(&mut f.data).desired_rows(6).desired_width(f32::INFINITY).hint_text(tr("One plate per line, values separated as chosen below.")));
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Separator"));
                    for (s, name) in [(Separator::Comma, tr("Comma")), (Separator::Tab, tr("Tab")), (Separator::Semicolon, tr("Semicolon")), (Separator::Space, tr("Space"))] {
                        ui.radio_value(&mut f.separator, s, name);
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.titles, tr("First line = column names"));
                    if ui.button(tr("Import from a file...")).clicked() {
                        if let Some(path) = crate::platform::FileDialog::new().add_filter(tr("Data"), &["csv", "txt", "tsv"]).pick_file() {
                            match crate::platform::read(&path) {
                                Ok(bytes) => {
                                    f.data = String::from_utf8_lossy(&bytes).to_string();
                                    if f.data.lines().next().is_some_and(|l| l.contains('\t')) {
                                        f.separator = Separator::Tab;
                                    } else if f.data.lines().next().is_some_and(|l| l.contains(';') && !l.contains(',')) {
                                        f.separator = Separator::Semicolon;
                                    }
                                }
                                Err(e) => self.message = Some(tf!("Could not read {}: {e}", path.display())),
                            }
                        }
                    }
                });
                let (names, rows) = f.rows();
                ui.label(RichText::new(tf!("Lines of data: {}", rows.len())).color(crate::theme::MUTED()));
                ui.add_space(6.0);
                ui.label(RichText::new(tr("What each text becomes")).strong());
                if f.texts.is_empty() {
                    ui.label(RichText::new(tr("The selected design has no text: every plate is the same.")).color(crate::theme::MUTED()));
                }
                let chosen = f.chosen_text.min(f.texts.len().saturating_sub(1));
                for (k, (_, now, _)) in f.texts.iter().enumerate() {
                    if ui.selectable_label(k == chosen, format!("“{now}”")).clicked() {
                        f.chosen_text = k;
                    }
                }
                if let Some((_, _, fill)) = f.texts.get_mut(chosen) {
                    let label = match fill {
                        Fill::Same => tr("Unchanged").to_string(),
                        Fill::Column(c) => tf!("Column {}", names.get(*c).cloned().unwrap_or_else(|| (*c + 1).to_string())),
                        Fill::Counter { .. } => tr("Counter").to_string(),
                    };
                    egui::ComboBox::from_id_salt("plate-fill").selected_text(label).show_ui(ui, |ui| {
                        ui.selectable_value(fill, Fill::Same, tr("Unchanged"));
                        for (c, name) in names.iter().enumerate() {
                            ui.selectable_value(fill, Fill::Column(c), tf!("Column {}", name));
                        }
                        if !matches!(fill, Fill::Counter { .. }) && ui.selectable_label(false, tr("Counter")).clicked() {
                            *fill = Fill::Counter { start: 1.0, step: 1.0, digits: 1, decimals: 0 };
                        }
                    });
                    if let Fill::Counter { start, step, digits, decimals } = fill {
                        ui.horizontal(|ui| {
                            ui.label(tr("Start"));
                            crate::ui::number(ui, start, 50.0);
                            ui.label(tr("Step"));
                            crate::ui::number(ui, step, 50.0);
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("Digits"));
                            crate::ui::count_box(ui, digits, 30.0);
                            ui.label(tr("Decimals"));
                            crate::ui::count_box(ui, decimals, 30.0);
                            *digits = (*digits).clamp(1, 12);
                            *decimals = (*decimals).min(6);
                        });
                        ui.label(RichText::new(tf!("First plates: {}, {}, {}", counted(*start, *step, *digits, *decimals, 0), counted(*start, *step, *digits, *decimals, 1), counted(*start, *step, *digits, *decimals, 2))).color(crate::theme::MUTED()));
                    }
                }
                ui.add_space(8.0);
                if ui.add_enabled(!self.selected.is_empty(), egui::Button::new(RichText::new(tr("Make the Plates")).strong().color(crate::theme::ON_ACCENT)).fill(crate::theme::ACCENT)).clicked() {
                    make = true;
                }
            });
        });
        if make {
            self.make_plates(&f);
        }
        if open && !make {
            self.plates = Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_and_rows_read_as_asked() {
        assert_eq!(counted(1.0, 1.0, 3, 0, 0), "001");
        assert_eq!(counted(1.0, 1.0, 3, 0, 41), "042");
        assert_eq!(counted(0.5, 0.25, 1, 2, 2), "1.00");
        let mut f = PlatesForm { margins: [0.0; 4], equal_margins: true, gaps: [0.0; 2], across: 1, down: 1, auto_count: true, data: "Name;Room\nAnna;12\nBob;7\n".into(), separator: Separator::Semicolon, titles: true, texts: Vec::new(), chosen_text: 0 };
        let (names, rows) = f.rows();
        assert_eq!(names, ["Name", "Room"]);
        assert_eq!(rows, [vec!["Anna".to_string(), "12".to_string()], vec!["Bob".to_string(), "7".to_string()]]);
        f.titles = false;
        assert_eq!(f.rows().1.len(), 3);
    }

    #[test]
    fn plates_fill_the_sheet_from_the_top_left_within_the_margins() {
        let f = PlatesForm { margins: [10.0, 10.0, 10.0, 10.0], equal_margins: true, gaps: [5.0, 5.0], across: 1, down: 1, auto_count: true, data: String::new(), separator: Separator::Comma, titles: false, texts: Vec::new(), chosen_text: 0 };
        let (across, down, spots) = layout([400.0, 400.0], [114.0, 98.0], &f, 13);
        assert_eq!((across, down), (3, 3), "as many as fit");
        assert_eq!(spots[0], ([10.0, 400.0 - 10.0 - 98.0], 0));
        assert_eq!(spots[1].0, [10.0 + 119.0, 292.0]);
        assert_eq!(spots[3].0, [10.0, 292.0 - 103.0]);
        assert_eq!(spots[9].1, 1, "the tenth on the next sheet");
    }
}
