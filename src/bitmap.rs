//! Pictures behind the drawing, and turning them into vectors.

use crate::app::*;
use crate::geom::{signed_area, Field, Path, P};
use crate::icons::Icon;
use crate::theme;
use crate::ui::{bold, card, labelled, labelled_from, title_card, wide};
use egui::{vec2, Color32, Rect, RichText, Sense, Ui};
use serde::{Deserialize, Serialize};
use std::path::Path as FilePath;
use std::sync::Arc;

/// A picture placed on the job. The pixels stay in the file it came from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BitmapRef {
    pub name: String,
    pub file: String,
    /// Bottom left corner, in job units.
    pub at: P,
    pub width: f64,
    /// 0 shows the picture fully, 1 hides it.
    pub fade: f32,
    pub visible: bool,
}

/// Decoded pixels: rows from the top, four bytes each.
#[derive(Clone, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgba: Arc<Vec<u8>>,
}

impl Image {
    pub fn load(file: &FilePath) -> Result<Image, String> {
        let bytes = crate::platform::read(file).map_err(|e| tf!("Could not read this picture: {e}"))?;
        let decoded = image::load_from_memory(&bytes).map_err(|e| tf!("Could not read this picture: {e}"))?.to_rgba8();
        let (w, h) = (decoded.width() as usize, decoded.height() as usize);
        if w == 0 || h == 0 {
            return Err(crate::i18n::tr("This picture is empty.").into());
        }
        Ok(Image { w, h, rgba: Arc::new(decoded.into_raw()) })
    }

    /// The colour of a pixel over a white background.
    pub fn pixel(&self, x: usize, y: usize) -> [f32; 3] {
        let i = (y * self.w + x) * 4;
        let p = &self.rgba[i..i + 4];
        let a = p[3] as f32 / 255.0;
        [p[0] as f32 * a + 255.0 * (1.0 - a), p[1] as f32 * a + 255.0 * (1.0 - a), p[2] as f32 * a + 255.0 * (1.0 - a)]
    }
}

/// The picture sorted into a few colours, ready to trace.
#[derive(Clone, Debug)]
pub struct Sorted {
    pub w: usize,
    pub h: usize,
    /// Which colour each cell belongs to, rows from the top.
    pub labels: Arc<Vec<u8>>,
    pub palette: Vec<[u8; 3]>,
}

fn light(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Black and white: everything darker than `threshold` (0 to 1) is one colour.
pub fn sort_black_white(img: &Image, threshold: f32) -> Sorted {
    let (stride, w, h) = work_size(img);
    let mut labels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            labels.push(u8::from(light(img.pixel(x * stride, y * stride)) >= threshold * 255.0));
        }
    }
    Sorted { w, h, labels: Arc::new(labels), palette: vec![[0, 0, 0], [255, 255, 255]] }
}

/// Reduces a large picture for tracing: one in every `stride` pixels.
fn work_size(img: &Image) -> (usize, usize, usize) {
    let stride = img.w.max(img.h).div_ceil(1000).max(1);
    (stride, (img.w / stride).max(1), (img.h / stride).max(1))
}

/// Sorts the picture into up to `count` colours by repeated averaging.
pub fn sort_colors(img: &Image, count: usize) -> Sorted {
    let count = count.clamp(2, 16);
    let (stride, w, h) = work_size(img);
    let sample_step = ((w * h) / 20_000).max(1);
    let mut samples: Vec<[f32; 3]> = Vec::new();
    for i in (0..w * h).step_by(sample_step) {
        samples.push(img.pixel((i % w) * stride, (i / w) * stride));
    }
    // Start from the darkest colour, then keep adding whichever colour is least like
    // those chosen: a small patch still gets its own colour when one colour fills most of the picture.
    let far = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    let darkest = samples.iter().copied().min_by(|a, b| light(*a).total_cmp(&light(*b))).unwrap_or([0.0; 3]);
    let mut centres = vec![darkest];
    while centres.len() < count {
        let gap = |s: &[f32; 3]| centres.iter().map(|c| far(s, c)).fold(f32::MAX, f32::min);
        let next = samples.iter().copied().max_by(|a, b| gap(a).total_cmp(&gap(b))).unwrap_or(darkest);
        centres.push(next);
    }
    let nearest = |c: [f32; 3], centres: &[[f32; 3]]| -> usize {
        let gap = |p: &[f32; 3]| (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
        (0..centres.len()).min_by(|a, b| gap(&centres[*a]).total_cmp(&gap(&centres[*b]))).unwrap_or(0)
    };
    for _ in 0..10 {
        let mut sums = vec![[0f32; 4]; count];
        for s in &samples {
            let k = nearest(*s, &centres);
            sums[k][0] += s[0];
            sums[k][1] += s[1];
            sums[k][2] += s[2];
            sums[k][3] += 1.0;
        }
        for (centre, sum) in centres.iter_mut().zip(&sums) {
            if sum[3] > 0.0 {
                *centre = [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]];
            }
        }
    }
    // Colours that ended up the same count as one.
    let mut unique: Vec<[f32; 3]> = Vec::new();
    for c in centres {
        if !unique.iter().any(|u| (u[0] - c[0]).abs() + (u[1] - c[1]).abs() + (u[2] - c[2]).abs() < 12.0) {
            unique.push(c);
        }
    }
    unique.sort_by(|a, b| light(*a).total_cmp(&light(*b)));
    let mut labels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            labels.push(nearest(img.pixel(x * stride, y * stride), &unique) as u8);
        }
    }
    let palette = unique.iter().map(|c| [c[0] as u8, c[1] as u8, c[2] as u8]).collect();
    Sorted { w, h, labels: Arc::new(labels), palette }
}

/// Fewer points on a loop: drops those within `tol` of the line through their neighbours.
fn thin_loop(pts: &[P], tol: f64) -> Vec<P> {
    if pts.len() < 4 || tol <= 0.0 {
        return pts.to_vec();
    }
    // Cut the loop at its two farthest points, then simplify each half.
    let far = (1..pts.len())
        .max_by(|a, b| {
            let d = |i: usize| (pts[i][0] - pts[0][0]).hypot(pts[i][1] - pts[0][1]);
            d(*a).total_cmp(&d(*b))
        })
        .unwrap_or(1);
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[far] = true;
    let mut stack = vec![(0, far), (far, pts.len())];
    while let Some((a, b)) = stack.pop() {
        if b - a < 2 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b % pts.len()]);
        let mut worst = (0.0, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = crate::geom::dist2_seg(*p, pa, pb);
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tol * tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    let out: Vec<P> = pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
    if out.len() >= 3 {
        out
    } else {
        pts.to_vec()
    }
}

/// Where to put the traced loops, and how closely to follow the picture.
#[derive(Clone, Copy, Debug)]
pub struct TraceFit {
    pub at: P,
    pub width: f64,
    /// 0 (loose) to 1 (tight).
    pub corner: f64,
    /// Patches smaller than this many pixels across are ignored.
    pub noise: f64,
}

/// The outlines of one colour of a sorted picture, as closed loops in job units.
pub fn trace_color(sorted: &Sorted, label: u8, fit: TraceFit) -> Vec<Path> {
    let (w, h) = (sorted.w, sorted.h);
    let cell = fit.width / w as f64;
    // A margin of two empty cells, so shapes touching the picture's edge still close.
    let (nx, ny) = (w + 4, h + 4);
    let mut raw = vec![0f32; nx * ny];
    for y in 0..h {
        for x in 0..w {
            if sorted.labels[y * w + x] == label {
                raw[(h - 1 - y + 2) * nx + x + 2] = 1.0;
            }
        }
    }
    // A slight blur turns pixel steps into smooth edges.
    let mut d = vec![0f32; nx * ny];
    for y in 1..ny - 1 {
        for x in 1..nx - 1 {
            let mut sum = 0.0;
            for (dy, dx, weight) in [(0, 0, 4.0f32), (0, 1, 2.0), (0, -1, 2.0), (1, 0, 2.0), (-1, 0, 2.0), (1, 1, 1.0), (1, -1, 1.0), (-1, 1, 1.0), (-1, -1, 1.0)] {
                sum += raw[((y as i32 + dy) as usize) * nx + (x as i32 + dx) as usize] * weight;
            }
            d[y * nx + x] = sum / 16.0;
        }
    }
    let field = Field { x0: fit.at[0] + (0.5 - 2.0) * cell, y0: fit.at[1] + (0.5 - 2.0) * cell, cell, nx, ny, d };
    let tol = cell * (1.6 - 1.5 * fit.corner.clamp(0.0, 1.0));
    let smallest = (fit.noise.max(1.0) * cell).powi(2);
    field
        .contours(0.5)
        .into_iter()
        .filter(|l| signed_area(l).abs() >= smallest)
        .map(|l| Path::closed(thin_loop(&l, tol)))
        .collect()
}

/// The values of the open Trace Bitmap form.
#[derive(Clone, Debug)]
pub struct TraceForm {
    pub color: bool,
    pub colors: usize,
    pub threshold: f32,
    pub corner: f32,
    pub noise: f32,
    pub group: bool,
    pub sorted: Option<Sorted>,
    pub sorted_for: (usize, bool, usize, u32),
    pub ticked: Vec<bool>,
    pub preview: Arc<Vec<Path>>,
}

pub const DEFAULT_CORNER: f32 = 0.82;
pub const DEFAULT_NOISE: f32 = 2.0;
pub const DEFAULT_FADE: f32 = 0.52;

impl TraceForm {
    pub fn new() -> Self {
        TraceForm {
            color: false,
            colors: 4,
            threshold: 0.5,
            corner: DEFAULT_CORNER,
            noise: DEFAULT_NOISE,
            group: false,
            sorted: None,
            sorted_for: (usize::MAX, false, 0, 0),
            ticked: Vec::new(),
            preview: Arc::new(Vec::new()),
        }
    }
}

/// The small words at each end of a slider.
fn slider_ends(ui: &mut Ui, low: &str, high: &str) {
    let width = ui.spacing().slider_width;
    ui.allocate_ui_with_layout(vec2(width, 12.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(width);
        ui.label(RichText::new(low).size(10.5).color(theme::MUTED()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(high).size(10.5).color(theme::MUTED()));
        });
    });
}

/// The largest side a picture is shown at; graphics cards refuse very large ones.
const SHOWN_SIDE: usize = 4096;

/// The pixels to show on screen, reduced if the picture is very large.
fn shown_pixels(img: &Image) -> ([usize; 2], Vec<u8>) {
    let stride = img.w.max(img.h).div_ceil(SHOWN_SIDE).max(1);
    let (w, h) = ((img.w / stride).max(1), (img.h / stride).max(1));
    if stride == 1 {
        return ([w, h], img.rgba.to_vec());
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let i = (y * stride * img.w + x * stride) * 4;
            out.extend_from_slice(&img.rgba[i..i + 4]);
        }
    }
    ([w, h], out)
}

impl App {
    /// Adds a picture from a file, fitted to the job.
    pub fn import_bitmap(&mut self, file: &FilePath) -> Result<(), String> {
        let img = Image::load(file)?;
        let key = file.display().to_string();
        let name = file.file_name().map_or_else(|| key.clone(), |n| n.to_string_lossy().to_string());
        self.place_bitmap(name, key, img);
        Ok(())
    }

    /// Puts decoded pixels on the job, as large as fits and centred.
    pub fn place_bitmap(&mut self, name: String, file: String, img: Image) {
        let (jw, jh) = (self.doc.job.width, self.doc.job.height);
        let ratio = img.h as f64 / img.w as f64;
        let width = jw.min(jh / ratio);
        let at = [(jw - width) / 2.0, (jh - width * ratio) / 2.0];
        self.checkpoint();
        self.bitmap_missing.remove(&file);
        self.bitmap_textures.remove(&file);
        self.bitmap_pixels.insert(file.clone(), img);
        self.doc.bitmaps.push(BitmapRef { name, file, at, width, fade: DEFAULT_FADE, visible: true });
        self.active_bitmap = self.doc.bitmaps.len() - 1;
        self.view3d = false;
    }

    /// Pixels for a placed picture, read again from its file after a project is opened.
    pub fn bitmap_image(&mut self, index: usize) -> Option<Image> {
        let file = self.doc.bitmaps.get(index)?.file.clone();
        if !self.bitmap_pixels.contains_key(&file) {
            // Looked for once: the drawing is repainted far too often to search each time.
            if self.bitmap_missing.contains(&file) {
                return None;
            }
            match Image::load(FilePath::new(&file)) {
                Ok(img) => {
                    self.bitmap_pixels.insert(file.clone(), img);
                }
                Err(_) => {
                    self.status = tf!("The picture {file} can't be found.");
                    self.bitmap_missing.insert(file);
                    return None;
                }
            }
        }
        self.bitmap_pixels.get(&file).cloned()
    }

    pub fn start_trace(&mut self) {
        self.tool = Tool2::Select;
        self.left_tab = LeftTab::Drawing;
        self.left = LeftForm::Trace(Box::new(TraceForm::new()));
        self.view3d = false;
    }

    /// Sorts the picture's colours again if the settings that decide them changed.
    fn refresh_sorting(&mut self, f: &mut TraceForm) {
        let key = (self.active_bitmap, f.color, f.colors, f.threshold.to_bits());
        if f.sorted_for == key && f.sorted.is_some() {
            return;
        }
        let Some(img) = self.bitmap_image(self.active_bitmap) else {
            f.sorted = None;
            return;
        };
        let sorted = if f.color { sort_colors(&img, f.colors) } else { sort_black_white(&img, f.threshold) };
        // Everything but the lightest colour: that one is usually the background.
        let lightest = (0..sorted.palette.len()).max_by_key(|k| sorted.palette[*k].iter().map(|v| *v as u32).sum::<u32>()).unwrap_or(0);
        f.ticked = (0..sorted.palette.len()).map(|k| k != lightest).collect();
        f.sorted = Some(sorted);
        f.sorted_for = key;
        f.preview = Arc::new(Vec::new());
    }

    /// The loops the current settings give, one list per ticked colour.
    pub fn traced_loops(&self, f: &TraceForm) -> Vec<Vec<Path>> {
        let (Some(sorted), Some(b)) = (&f.sorted, self.doc.bitmaps.get(self.active_bitmap)) else { return Vec::new() };
        let fit = TraceFit { at: b.at, width: b.width, corner: f.corner as f64, noise: f.noise as f64 };
        (0..sorted.palette.len()).filter(|k| f.ticked.get(*k).copied().unwrap_or(false)).map(|k| trace_color(sorted, k as u8, fit)).collect()
    }

    /// Makes vectors from the picture. Returns how many outlines were added.
    pub fn apply_trace(&mut self, f: &TraceForm) -> usize {
        let colors = self.traced_loops(f);
        let total: usize = colors.iter().map(Vec::len).sum();
        if total == 0 {
            self.status = crate::i18n::tr("Nothing to trace with these settings. Tick a colour, or lower the noise filter.").into();
            return 0;
        }
        self.checkpoint();
        self.selected.clear();
        for loops in colors.into_iter().filter(|l| !l.is_empty()) {
            if f.group {
                let id = self.doc.add_vector(loops, Prim::Free);
                self.selected.push(id);
            } else {
                for l in loops {
                    let id = self.doc.add_vector(vec![l], Prim::Free);
                    self.selected.push(id);
                }
            }
        }
        self.status = tf!("Traced {total} outlines.");
        total
    }

    pub fn trace_form(&mut self, ui: &mut Ui, mut f: TraceForm) {
        self.refresh_sorting(&mut f);
        let has_bitmap = self.doc.bitmaps.get(self.active_bitmap).is_some();
        title_card(ui, Icon::Trace, crate::i18n::tr("Trace Bitmap"));
        let unit = self.doc.job.unit();
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Bitmap"));
            if self.doc.bitmaps.len() > 1 {
                let names: Vec<String> = self.doc.bitmaps.iter().map(|b| b.name.clone()).collect();
                let shown = names.get(self.active_bitmap).cloned().unwrap_or_default();
                egui::ComboBox::from_id_salt("bitmap_choice").selected_text(shown).width(ui.available_width() - 8.0).truncate().show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.active_bitmap, i, n.as_str());
                    }
                });
            } else if let Some(b) = self.doc.bitmaps.first() {
                ui.add(egui::Label::new(b.name.as_str()).truncate());
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Import Bitmap ...")).clicked() {
                    if let Some(path) = crate::platform::FileDialog::new().add_filter(crate::i18n::tr("Picture"), &["png", "jpg", "jpeg", "bmp"]).pick_file() {
                        if let Err(e) = self.import_bitmap(&path) {
                            self.message = Some(e);
                        }
                    }
                }
                if ui.add_enabled(has_bitmap, egui::Button::new(crate::i18n::tr("Remove"))).clicked() {
                    self.checkpoint();
                    self.doc.bitmaps.remove(self.active_bitmap);
                    self.active_bitmap = 0;
                    f.sorted = None;
                }
            });
            let i = self.active_bitmap;
            let zero = self.doc.job.origin();
            if let Some(b) = self.doc.bitmaps.get_mut(i) {
                let was = (b.at, b.width);
                ui.horizontal(|ui| {
                    labelled_from(ui, "X", 10.0, &mut b.at[0], zero[0], 52.0, "");
                    labelled_from(ui, "Y", 10.0, &mut b.at[1], zero[1], 52.0, "");
                });
                labelled(ui, crate::i18n::tr("Width"), 34.0, &mut b.width, 60.0, unit);
                b.width = b.width.max(1e-3);
                if was != (b.at, b.width) {
                    f.preview = Arc::new(Vec::new());
                }
            }
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Type of tracing to use"));
            ui.radio_value(&mut f.color, true, crate::i18n::tr("Color"));
            ui.radio_value(&mut f.color, false, crate::i18n::tr("Black / White"));
            if f.color {
                bold(ui, crate::i18n::tr("Number of Colors"));
                let mut n = f.colors as f32;
                ui.spacing_mut().slider_width = 150.0;
                slider_ends(ui, "Min", "Max");
                if ui.add(egui::Slider::new(&mut n, 2.0..=16.0).step_by(1.0).fixed_decimals(0)).changed() {
                    f.colors = n as usize;
                }
            } else {
                bold(ui, crate::i18n::tr("Threshold"));
                ui.spacing_mut().slider_width = 150.0;
                slider_ends(ui, crate::i18n::tr("Dark"), crate::i18n::tr("Light"));
                ui.add(egui::Slider::new(&mut f.threshold, 0.02..=0.98).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            }
            bold(ui, crate::i18n::tr("Colors to fit vectors to ..."));
            match &f.sorted {
                Some(s) => {
                    let palette = s.palette.clone();
                    ui.horizontal_wrapped(|ui| {
                        for (k, c) in palette.iter().enumerate() {
                            let mut on = f.ticked.get(k).copied().unwrap_or(false);
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                            ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
                            ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.0, theme::LINE()), egui::StrokeKind::Inside);
                            if ui.checkbox(&mut on, "").changed() {
                                if let Some(t) = f.ticked.get_mut(k) {
                                    *t = on;
                                }
                                f.preview = Arc::new(Vec::new());
                            }
                        }
                    });
                }
                None => {
                    bold(ui, crate::i18n::tr("No bitmap selected"));
                    ui.label(RichText::new(crate::i18n::tr("Import a picture to fit vectors to.")).color(theme::MUTED()));
                }
            }
        });
        let before = (f.corner, f.noise, f.group);
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.spacing_mut().slider_width = 130.0;
            ui.horizontal(|ui| {
                bold(ui, crate::i18n::tr("Corner Fit"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Default Corner Fit")).clicked() {
                        f.corner = DEFAULT_CORNER;
                    }
                });
            });
            slider_ends(ui, crate::i18n::tr("Loose"), crate::i18n::tr("Tight"));
            ui.add(egui::Slider::new(&mut f.corner, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            ui.separator();
            ui.horizontal(|ui| {
                bold(ui, crate::i18n::tr("Noise Filter"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Default Noise")).clicked() {
                        f.noise = DEFAULT_NOISE;
                    }
                });
            });
            slider_ends(ui, crate::i18n::tr("1 Pixel"), crate::i18n::tr("10 Pixels"));
            ui.add(egui::Slider::new(&mut f.noise, 1.0..=10.0).step_by(1.0).fixed_decimals(0));
            ui.separator();
            let i = self.active_bitmap;
            ui.horizontal(|ui| {
                bold(ui, crate::i18n::tr("Bitmap"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(has_bitmap, egui::Button::new(crate::i18n::tr("Default Fading"))).clicked() {
                        if let Some(b) = self.doc.bitmaps.get_mut(i) {
                            b.fade = DEFAULT_FADE;
                        }
                    }
                });
            });
            slider_ends(ui, crate::i18n::tr("None"), crate::i18n::tr("Full"));
            let mut fade = self.doc.bitmaps.get(i).map_or(DEFAULT_FADE, |b| b.fade);
            if ui.add_enabled(has_bitmap, egui::Slider::new(&mut fade, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
                if let Some(b) = self.doc.bitmaps.get_mut(i) {
                    b.fade = fade;
                }
            }
            ui.separator();
            ui.checkbox(&mut f.group, crate::i18n::tr("Group vectors"));
        });
        if before.0 != f.corner || before.1 != f.noise {
            f.preview = Arc::new(Vec::new());
        }
        let ready = has_bitmap && f.sorted.is_some();
        if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Preview")).min_size(vec2(ui.available_width() - 4.0, 20.0))).clicked() {
            f.preview = Arc::new(self.traced_loops(&f).into_iter().flatten().collect());
            self.status = tf!("{} outlines would be made.", f.preview.len());
        }
        let mut close = false;
        let mut apply = false;
        ui.horizontal(|ui| {
            close = wide(ui, crate::i18n::tr("Close"), 74.0).clicked();
            ui.add_space((ui.available_width() - 80.0).max(0.0));
            apply = ui.add_enabled(ready, egui::Button::new(RichText::new(crate::i18n::tr("Apply")).color(theme::ON_ACCENT).strong()).fill(theme::ACCENT).min_size(vec2(74.0, 22.0))).clicked();
        });
        if apply && self.apply_trace(&f) > 0 {
            f.preview = Arc::new(Vec::new());
        }
        self.trace_preview = f.preview.clone();
        if close {
            self.left = LeftForm::None;
            self.trace_preview = Arc::new(Vec::new());
        } else if matches!(self.left, LeftForm::Trace(_)) {
            self.left = LeftForm::Trace(Box::new(f));
        }
    }

    /// Draws the pictures under everything else on the sheet.
    pub fn paint_bitmaps(&mut self, ui: &Ui, painter: &egui::Painter) {
        for i in 0..self.doc.bitmaps.len() {
            let b = self.doc.bitmaps[i].clone();
            if !b.visible {
                continue;
            }
            if !self.bitmap_textures.contains_key(&b.file) {
                let Some(img) = self.bitmap_image(i) else { continue };
                let (size, pixels) = shown_pixels(&img);
                let colour = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let handle = ui.ctx().load_texture(format!("bitmap {}", b.file), colour, egui::TextureOptions::LINEAR);
                self.bitmap_textures.insert(b.file.clone(), handle);
            }
            let Some(img) = self.bitmap_pixels.get(&b.file) else { continue };
            let height = b.width * img.h as f64 / img.w as f64;
            let rect = Rect::from_two_pos(self.to_screen(b.at), self.to_screen([b.at[0] + b.width, b.at[1] + height]));
            if let Some(tex) = self.bitmap_textures.get(&b.file) {
                let tint = Color32::WHITE.gamma_multiply(1.0 - b.fade);
                painter.image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::bbox;

    /// White picture with a black disc, and optionally a red square.
    fn picture(w: usize, h: usize, red_square: bool) -> Image {
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - 50.0, y as f64 - 50.0);
                let colour = if dx.hypot(dy) < 30.0 {
                    [0, 0, 0]
                } else if red_square && (120..170).contains(&x) && (30..80).contains(&y) {
                    [220, 20, 20]
                } else {
                    [255, 255, 255]
                };
                rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
            }
        }
        Image { w, h, rgba: Arc::new(rgba) }
    }

    fn fit() -> TraceFit {
        TraceFit { at: [0.0, 0.0], width: 10.0, corner: 0.82, noise: 2.0 }
    }

    #[test]
    fn a_black_disc_traces_to_one_round_outline_in_the_right_place() {
        let img = picture(100, 100, false);
        let sorted = sort_black_white(&img, 0.5);
        let loops = trace_color(&sorted, 0, fit());
        assert_eq!(loops.len(), 1);
        let (lo, hi) = bbox(&loops).unwrap();
        // Pixel 50,50 of 100 is the middle of a 10 wide job; row 50 from the top too.
        assert!(((lo[0] + hi[0]) / 2.0 - 5.05).abs() < 0.1, "{lo:?} {hi:?}");
        assert!(((lo[1] + hi[1]) / 2.0 - 4.95).abs() < 0.1, "{lo:?} {hi:?}");
        assert!((hi[0] - lo[0] - 6.0).abs() < 0.2, "diameter {}", hi[0] - lo[0]);
        let area = signed_area(&loops[0].pts).abs();
        assert!((area - std::f64::consts::PI * 9.0).abs() < 1.0, "{area}");
        assert!(loops[0].closed);
    }

    #[test]
    fn up_in_the_picture_is_up_on_the_job() {
        // A black bar along the top rows only.
        let (w, h) = (40, 40);
        let mut rgba = vec![255u8; w * h * 4];
        for y in 0..5 {
            for x in 0..w {
                let i = (y * w + x) * 4;
                rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let img = Image { w, h, rgba: Arc::new(rgba) };
        let loops = trace_color(&sort_black_white(&img, 0.5), 0, fit());
        let (lo, _) = bbox(&loops).unwrap();
        assert!(lo[1] > 8.0, "the bar is at the top: {lo:?}");
    }

    #[test]
    fn specks_are_filtered_out_by_the_noise_setting() {
        let (w, h) = (100, 100);
        let mut rgba = vec![255u8; w * h * 4];
        for (x, y) in [(10, 10), (30, 40), (70, 20)] {
            let i = (y * w + x) * 4;
            rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
        }
        for y in 60..90 {
            for x in 60..90 {
                let i = (y * w + x) * 4;
                rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let img = Image { w, h, rgba: Arc::new(rgba) };
        let sorted = sort_black_white(&img, 0.9);
        assert_eq!(trace_color(&sorted, 0, TraceFit { noise: 5.0, ..fit() }).len(), 1, "only the big square");
    }

    #[test]
    fn a_tighter_corner_fit_keeps_more_points() {
        let sorted = sort_black_white(&picture(100, 100, false), 0.5);
        let tight = trace_color(&sorted, 0, TraceFit { corner: 1.0, ..fit() })[0].pts.len();
        let loose = trace_color(&sorted, 0, TraceFit { corner: 0.0, ..fit() })[0].pts.len();
        assert!(loose < tight && loose >= 6, "loose {loose}, tight {tight}");
    }

    #[test]
    fn colors_are_told_apart_and_traced_separately() {
        let img = picture(200, 100, true);
        let sorted = sort_colors(&img, 3);
        assert_eq!(sorted.palette.len(), 3, "{:?}", sorted.palette);
        let red = sorted.palette.iter().position(|c| c[0] > 150 && c[1] < 100).expect("red is found") as u8;
        let black = sorted.palette.iter().position(|c| c[0] < 60).expect("black is found") as u8;
        assert_eq!(trace_color(&sorted, red, TraceFit { width: 20.0, ..fit() }).len(), 1);
        let disc = trace_color(&sorted, black, TraceFit { width: 20.0, ..fit() });
        assert_eq!(disc.len(), 1);
        assert!(bbox(&disc).unwrap().1[0] < 9.0, "the disc is on the left");
    }

    #[test]
    fn asking_for_more_colors_than_exist_gives_only_real_ones() {
        let sorted = sort_colors(&picture(100, 100, false), 8);
        assert_eq!(sorted.palette.len(), 2, "{:?}", sorted.palette);
    }

    #[test]
    fn very_large_pictures_are_reduced_for_the_screen_only() {
        let img = Image { w: 9000, h: 2, rgba: Arc::new(vec![255; 9000 * 2 * 4]) };
        let (size, pixels) = shown_pixels(&img);
        assert!(size[0] <= SHOWN_SIDE && size[1] >= 1, "{size:?}");
        assert_eq!(pixels.len(), size[0] * size[1] * 4);
        let small = picture(100, 100, false);
        assert_eq!(shown_pixels(&small).0, [100, 100]);
        // A long thin picture still has something to trace.
        assert_eq!(sort_black_white(&img, 0.5).h, 1);
    }

    #[test]
    fn a_missing_picture_is_only_searched_for_once() {
        let mut a = App::default();
        a.new_file();
        a.doc.bitmaps.push(BitmapRef { name: "gone".into(), file: "/nowhere/gone.png".into(), at: [0.0, 0.0], width: 1.0, fade: 0.5, visible: true });
        assert!(a.bitmap_image(0).is_none());
        assert!(a.status.contains("can't be found"));
        a.status.clear();
        assert!(a.bitmap_image(0).is_none());
        assert!(a.status.is_empty(), "not searched again");
    }

    #[test]
    fn transparent_pixels_count_as_white() {
        let img = Image { w: 1, h: 1, rgba: Arc::new(vec![0, 0, 0, 0]) };
        assert_eq!(img.pixel(0, 0), [255.0, 255.0, 255.0]);
    }

    #[test]
    fn a_shape_with_a_hole_gives_two_outlines() {
        let (w, h) = (100, 100);
        let mut rgba = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let r = (x as f64 - 50.0).hypot(y as f64 - 50.0);
                if (15.0..35.0).contains(&r) {
                    let i = (y * w + x) * 4;
                    rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
        let img = Image { w, h, rgba: Arc::new(rgba) };
        assert_eq!(trace_color(&sort_black_white(&img, 0.5), 0, fit()).len(), 2);
    }

    #[test]
    fn apply_adds_vectors_and_undo_takes_them_away() {
        let mut a = App::default();
        a.new_file();
        a.place_bitmap("disc".into(), "memory".into(), picture(100, 100, false));
        assert!((a.doc.bitmaps[0].width - 10.0).abs() < 1e-9, "fills the job");
        let mut f = TraceForm::new();
        a.refresh_sorting(&mut f);
        assert_eq!(f.ticked, vec![true, false], "black is traced, the white background isn't");
        assert_eq!(a.apply_trace(&f), 1);
        assert_eq!(a.doc.vectors.len(), 1);
        assert_eq!(a.selected.len(), 1);
        a.do_undo();
        assert!(a.doc.vectors.is_empty());
        f.ticked = vec![false, false];
        assert_eq!(a.apply_trace(&f), 0);
        assert!(a.status.contains("Nothing to trace"));
    }

    #[test]
    fn grouping_makes_one_shape_from_many_outlines() {
        let mut a = App::default();
        a.new_file();
        a.place_bitmap("disc".into(), "memory".into(), picture(200, 100, true));
        let mut f = TraceForm { color: true, colors: 3, group: true, ..TraceForm::new() };
        a.refresh_sorting(&mut f);
        assert_eq!(a.apply_trace(&f), 2, "black disc and red square");
        assert_eq!(a.doc.vectors.len(), 2, "one group for each colour");
    }

    #[test]
    fn pictures_load_from_files_and_bad_files_are_explained() {
        let dir = std::env::temp_dir().join(format!("burin-bitmap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("shape.png");
        let out = std::fs::File::create(&file).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(out), 4, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&[0u8; 32]).unwrap();
        let mut a = App::default();
        a.new_file();
        a.import_bitmap(&file).unwrap();
        assert_eq!(a.doc.bitmaps[0].name, "shape.png");
        assert!((a.doc.bitmaps[0].width - 10.0).abs() < 1e-9);
        assert!((a.doc.bitmaps[0].at[1] - 2.5).abs() < 1e-9, "centred: {:?}", a.doc.bitmaps[0].at);
        let bad = dir.join("broken.png");
        crate::platform::write(&bad, "not a picture").unwrap();
        assert!(a.import_bitmap(&bad).unwrap_err().contains("Could not read"));
        // The pixels come back from the file after a project is reopened.
        a.bitmap_pixels.clear();
        assert_eq!(a.bitmap_image(0).unwrap().w, 4);
        std::fs::remove_dir_all(&dir).unwrap();
        a.bitmap_pixels.clear();
        assert!(a.bitmap_image(0).is_none());
        assert!(a.status.contains("can't be found"));
    }
}
