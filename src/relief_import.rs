//! 3D reliefs from outside: pictures turned into height, and STL models seen from above.

use crate::app::*;
use crate::bitmap::Image;
use crate::geom::{rect, Path as Outline};
use crate::icons::Icon;
use crate::relief::{Combine, Component, HeightMap, Profile};
use crate::theme;
use crate::ui::{bold, card, labelled, labelled_from, title_card};
use egui::{RichText, Ui};
use std::path::Path;
use std::sync::Arc;

/// The longest side of an imported height grid.
const MAX_SIDE: usize = 800;

type Triangle = [[f32; 3]; 3];

/// The triangles of an STL file, in either of its two forms.
pub fn parse_stl(bytes: &[u8]) -> Result<Vec<Triangle>, String> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if count > 0 && bytes.len() == 84 + count * 50 {
            let tris = bytes[84..]
                .chunks_exact(50)
                .map(|t| {
                    let at = |i: usize| f32::from_le_bytes([t[i], t[i + 1], t[i + 2], t[i + 3]]);
                    [[at(12), at(16), at(20)], [at(24), at(28), at(32)], [at(36), at(40), at(44)]]
                })
                .collect();
            return Ok(tris);
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let mut corners: Vec<[f32; 3]> = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next().is_some_and(|w| w.eq_ignore_ascii_case("vertex")) {
            let v: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
            if v.len() == 3 {
                corners.push([v[0], v[1], v[2]]);
            }
        }
    }
    let tris: Vec<Triangle> = corners.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    if tris.is_empty() {
        return Err(crate::i18n::tr("This file has no 3D surface this program can read.").into());
    }
    Ok(tris)
}

/// A model seen from straight above: heights, and its size in the file's own units.
#[derive(Clone, Debug)]
pub struct TopView {
    pub w: usize,
    pub h: usize,
    /// Rows from the low Y upward. 0 is nothing there; 1 to 65535 go from lowest to highest.
    pub heights: Vec<u16>,
    pub size: [f64; 3],
}

pub fn top_view(tris: &[Triangle], max_side: usize) -> Result<TopView, String> {
    let finite = |t: &&Triangle| t.iter().flatten().all(|v| v.is_finite());
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for t in tris.iter().filter(finite) {
        for p in t {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    let size = [(hi[0] - lo[0]) as f64, (hi[1] - lo[1]) as f64, (hi[2] - lo[2]) as f64];
    if !(size[0] > 0.0 && size[1] > 0.0) {
        return Err(crate::i18n::tr("This model has no width seen from above.").into());
    }
    let cell = size[0].max(size[1]) / max_side as f64;
    let w = ((size[0] / cell).ceil() as usize).max(1);
    let h = ((size[1] / cell).ceil() as usize).max(1);
    let mut top = vec![f32::NEG_INFINITY; w * h];
    let cell32 = cell as f32;
    for t in tris.iter().filter(finite) {
        // Pixel centres sit half a cell in.
        let sx = |p: &[f32; 3]| (p[0] - lo[0]) / cell32 - 0.5;
        let sy = |p: &[f32; 3]| (p[1] - lo[1]) / cell32 - 0.5;
        let (ax, ay, bx, by, cx, cy) = (sx(&t[0]), sy(&t[0]), sx(&t[1]), sy(&t[1]), sx(&t[2]), sy(&t[2]));
        let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if area.abs() < 1e-9 {
            continue; // Upright walls have no area from above.
        }
        let (x0, x1) = (ax.min(bx).min(cx).floor().max(0.0) as usize, (ax.max(bx).max(cx).ceil() as usize).min(w - 1));
        let (y0, y1) = (ay.min(by).min(cy).floor().max(0.0) as usize, (ay.max(by).max(cy).ceil() as usize).min(h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32, y as f32);
                let w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) / area;
                let w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) / area;
                let w2 = 1.0 - w0 - w1;
                // A touch of slack, so no pinholes open between neighbouring triangles.
                if w0 >= -1e-3 && w1 >= -1e-3 && w2 >= -1e-3 {
                    let z = w0 * t[0][2] + w1 * t[1][2] + w2 * t[2][2];
                    let cellz = &mut top[y * w + x];
                    *cellz = cellz.max(z);
                }
            }
        }
    }
    let range = (hi[2] - lo[2]).max(1e-9);
    let heights = top
        .iter()
        .map(|z| if z.is_finite() { 1 + (((z - lo[2]) / range).clamp(0.0, 1.0) * 65534.0) as u16 } else { 0 })
        .collect();
    Ok(TopView { w, h, heights, size })
}

/// A picture as heights: rows from the bottom, light is high.
pub fn picture_heights(img: &Image, max_side: usize) -> (usize, usize, Vec<u16>) {
    let stride = img.w.max(img.h).div_ceil(max_side).max(1);
    let (w, h) = ((img.w / stride).max(1), (img.h / stride).max(1));
    let mut out = Vec::with_capacity(w * h);
    for y in (0..h).rev() {
        for x in 0..w {
            let c = img.pixel(x * stride, y * stride);
            let light = (0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]) / 255.0;
            out.push(1 + (light.clamp(0.0, 1.0) * 65534.0) as u16);
        }
    }
    (w, h, out)
}

/// Dark becomes high and high becomes low; empty cells stay empty.
pub fn inverted(heights: &[u16]) -> Vec<u16> {
    heights.iter().map(|v| if *v == 0 { 0 } else { 65536 - *v as u32 } as u16).collect()
}

/// Softens a height grid, `passes` times over; empty cells stay empty.
pub fn smoothed(heights: &[u16], w: usize, h: usize, passes: usize) -> Vec<u16> {
    let mut now = heights.to_vec();
    for _ in 0..passes {
        let before = now.clone();
        for y in 0..h {
            for x in 0..w {
                if before[y * w + x] == 0 {
                    continue;
                }
                let (mut sum, mut count) = (0u32, 0u32);
                for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let v = before[ny * w + nx];
                        if v != 0 {
                            sum += v as u32;
                            count += 1;
                        }
                    }
                }
                now[y * w + x] = (sum / count) as u16;
            }
        }
    }
    now
}

/// The values of the open relief form.
#[derive(Clone, Debug)]
pub struct ReliefForm {
    pub name: String,
    pub is_picture: bool,
    pub w: usize,
    pub h: usize,
    pub heights: Arc<Vec<u16>>,
    /// The height that keeps a model's true proportions, as a share of its width.
    pub true_ratio: Option<f64>,
    pub at: [f64; 2],
    pub width: f64,
    pub height: f64,
    pub base: f64,
    pub combine: Combine,
    pub invert: bool,
    pub smooth: usize,
}

impl App {
    fn relief_form(&self, name: String, is_picture: bool, w: usize, h: usize, heights: Vec<u16>, true_ratio: Option<f64>) -> ReliefForm {
        let j = &self.doc.job;
        let ratio = h as f64 / w as f64;
        let width = j.width.min(j.height / ratio);
        let height = match true_ratio {
            Some(r) => (r * width).min(j.thickness),
            None => j.thickness * 0.5,
        };
        ReliefForm {
            name,
            is_picture,
            w,
            h,
            heights: Arc::new(heights),
            true_ratio,
            at: [(j.width - width) / 2.0, (j.height - width * ratio) / 2.0],
            width,
            height,
            base: 0.0,
            combine: Combine::Add,
            invert: false,
            smooth: if is_picture { 1 } else { 0 },
        }
    }

    pub fn open_relief_picture(&mut self, name: String, img: &Image) {
        let (w, h, heights) = picture_heights(img, MAX_SIDE);
        let form = self.relief_form(name, true, w, h, heights, None);
        self.show_relief_form(form);
    }

    pub fn open_model_bytes(&mut self, name: String, bytes: &[u8]) -> Result<(), String> {
        let view = top_view(&parse_stl(bytes)?, MAX_SIDE)?;
        let form = self.relief_form(name, false, view.w, view.h, view.heights, Some(view.size[2] / view.size[0]));
        self.show_relief_form(form);
        Ok(())
    }

    fn show_relief_form(&mut self, form: ReliefForm) {
        self.left_open = true;
        self.left_tab = LeftTab::Modeling;
        self.left = LeftForm::Relief(Box::new(form));
    }

    pub fn dialog_import_model(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter(crate::i18n::tr("3D model"), &["stl"]).pick_file() else { return };
        if let Err(why) = self.open_model_file(&file) {
            self.message = Some(why);
        }
    }

    pub fn open_model_file(&mut self, file: &Path) -> Result<(), String> {
        let bytes = std::fs::read(file).map_err(|e| tf!("Could not read {}: {e}", file.display()))?;
        self.open_model_bytes(short_name(file), &bytes)
    }

    pub fn dialog_relief_picture(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter(crate::i18n::tr("Picture"), &["png", "jpg", "jpeg", "bmp"]).pick_file() else { return };
        match Image::load(&file) {
            Ok(img) => self.open_relief_picture(short_name(&file), &img),
            Err(why) => self.message = Some(why),
        }
    }

    /// Adds the relief as a 3D shape, shown in the 3D view.
    pub fn apply_relief(&mut self, f: &ReliefForm) {
        let flipped = if f.invert { inverted(&f.heights) } else { f.heights.as_ref().clone() };
        let heights = smoothed(&flipped, f.w, f.h, f.smooth);
        let tall = f.width * f.h as f64 / f.w as f64;
        let centre = [f.at[0] + f.width / 2.0, f.at[1] + tall / 2.0];
        self.checkpoint();
        self.doc.components.push(Component {
            name: f.name.clone(),
            paths: vec![Outline::closed(rect(centre, f.width, tall, 0.0))],
            profile: Profile::Flat,
            height: f.height,
            base: f.base,
            combine: f.combine,
            visible: true,
            surface: Some(HeightMap { w: f.w, h: f.h, data: Arc::new(heights), at: f.at, width: f.width }),
        });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.model_dirty = true;
        self.sim = None;
        self.view3d = true;
        self.left = LeftForm::None;
        self.status = tf!("Added {} as a 3D shape.", f.name);
    }

    pub fn relief_form_ui(&mut self, ui: &mut Ui, mut f: ReliefForm) {
        let title = if f.is_picture { crate::i18n::tr("3D Relief from Picture") } else { crate::i18n::tr("Import 3D Model") };
        title_card(ui, if f.is_picture { Icon::ReliefPicture } else { Icon::ImportModel }, title);
        let unit = self.doc.job.unit();
        let zero = self.doc.job.origin();
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(egui::Label::new(f.name.as_str()).truncate());
            ui.add_space(2.0);
            bold(ui, crate::i18n::tr("Position and Size"));
            ui.horizontal(|ui| {
                labelled_from(ui, "X", 10.0, &mut f.at[0], zero[0], 52.0, "");
                labelled_from(ui, "Y", 10.0, &mut f.at[1], zero[1], 52.0, "");
            });
            labelled(ui, crate::i18n::tr("Width"), 44.0, &mut f.width, 60.0, unit);
            f.width = f.width.max(1e-3);
            ui.label(RichText::new(tf!("Depth (Y): {} {unit}", crate::toolpath::trim(f.width * f.h as f64 / f.w as f64))).color(theme::MUTED));
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Height"));
            labelled(ui, crate::i18n::tr("Height"), 44.0, &mut f.height, 60.0, unit);
            labelled(ui, crate::i18n::tr("Base"), 44.0, &mut f.base, 60.0, unit);
            f.height = f.height.max(0.0);
            f.base = f.base.max(0.0);
            if let Some(r) = f.true_ratio {
                let natural = r * f.width;
                ui.label(RichText::new(tf!("True proportions: {} {unit}", crate::toolpath::trim(natural))).color(theme::MUTED));
                if ui.button(crate::i18n::tr("Use True Proportions")).clicked() {
                    f.height = natural;
                }
            }
            if f.is_picture {
                ui.add_space(3.0);
                ui.radio_value(&mut f.invert, false, crate::i18n::tr("Light is high"));
                ui.radio_value(&mut f.invert, true, crate::i18n::tr("Dark is high (lithophane)"));
            }
            let mut smooth = f.smooth as f32;
            ui.add_space(3.0);
            ui.label(crate::i18n::tr("Smoothing"));
            ui.spacing_mut().slider_width = 130.0;
            if ui.add(egui::Slider::new(&mut smooth, 0.0..=8.0).step_by(1.0).fixed_decimals(0)).changed() {
                f.smooth = smooth as usize;
            }
        });
        card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            bold(ui, crate::i18n::tr("Combine"));
            for (mode, label) in [(Combine::Add, crate::i18n::tr("Add")), (Combine::Subtract, crate::i18n::tr("Subtract")), (Combine::Merge, crate::i18n::tr("Merge")), (Combine::Low, crate::i18n::tr("Low"))] {
                ui.radio_value(&mut f.combine, mode, label);
            }
        });
        if !f.is_picture {
            card(ui, |ui| {
                ui.label(RichText::new(crate::i18n::tr("The model is read from straight above: what is hidden underneath is filled in.")).color(theme::MUTED));
            });
        }
        if self.form_buttons(ui, crate::i18n::tr("Apply")) {
            self.apply_relief(&f);
        } else if matches!(self.left, LeftForm::Relief(_)) {
            self.left = LeftForm::Relief(Box::new(f));
        }
    }
}

fn short_name(file: &Path) -> String {
    file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relief::compose;

    /// A square pyramid: base 10 by 10 at z = 0, top at z = 5.
    fn pyramid() -> Vec<Triangle> {
        let (a, b, c, d) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        let tip = [5.0, 5.0, 5.0];
        vec![[a, b, tip], [b, c, tip], [c, d, tip], [d, a, tip], [a, c, b], [a, d, c]]
    }

    fn binary(tris: &[Triangle]) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend_from_slice(&[0u8; 12]);
            for p in t {
                for v in p {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    fn ascii(tris: &[Triangle]) -> String {
        let mut s = String::from("solid demo\n");
        for t in tris {
            s += " facet normal 0 0 1\n  outer loop\n";
            for p in t {
                s += &format!("   vertex {} {} {}\n", p[0], p[1], p[2]);
            }
            s += "  endloop\n endfacet\n";
        }
        s + "endsolid demo\n"
    }

    fn app() -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        a
    }

    #[test]
    fn both_stl_forms_read_the_same_triangles() {
        let bin = parse_stl(&binary(&pyramid())).unwrap();
        let txt = parse_stl(ascii(&pyramid()).as_bytes()).unwrap();
        assert_eq!(bin.len(), 6);
        assert_eq!(bin, txt);
        assert!(parse_stl(b"nothing useful here").unwrap_err().contains("no 3D surface"));
    }

    #[test]
    fn a_pyramid_seen_from_above_peaks_in_the_middle() {
        let v = top_view(&pyramid(), 100).unwrap();
        assert_eq!((v.w, v.h), (100, 100));
        assert_eq!(v.size, [10.0, 10.0, 5.0]);
        let at = |x: usize, y: usize| v.heights[y * v.w + x] as f64 / 65535.0;
        assert!(at(50, 50) > 0.97, "{}", at(50, 50));
        assert!(at(25, 50) > 0.45 && at(25, 50) < 0.55, "halfway up: {}", at(25, 50));
        assert!(at(1, 1) < 0.06, "{}", at(1, 1));
        assert!(v.heights.iter().all(|h| *h > 0), "no pinholes between the triangles");
    }

    #[test]
    fn parts_the_model_does_not_cover_stay_empty() {
        // One triangle covers only half of its box.
        let v = top_view(&[[[0.0, 0.0, 0.0], [10.0, 0.0, 1.0], [0.0, 10.0, 2.0]]], 50).unwrap();
        assert_eq!(v.heights[v.w * v.h - 1], 0);
        assert!(v.heights[0] > 0);
    }

    #[test]
    fn a_model_with_no_width_is_explained() {
        let wall = [[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 5.0]]];
        assert!(top_view(&wall, 50).unwrap_err().contains("no width"));
    }

    #[test]
    fn broken_numbers_in_a_model_do_not_spoil_it() {
        let mut tris = pyramid();
        tris.push([[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        assert_eq!(top_view(&tris, 20).unwrap().size, [10.0, 10.0, 5.0]);
    }

    #[test]
    fn a_picture_keeps_top_at_the_top_and_light_high() {
        // Two rows: white on top, black underneath.
        let img = Image { w: 1, h: 2, rgba: Arc::new(vec![255, 255, 255, 255, 0, 0, 0, 255]) };
        let (w, h, heights) = picture_heights(&img, 100);
        assert_eq!((w, h), (1, 2));
        assert!(heights[0] < 10, "row 0 of the result is the bottom of the picture");
        assert!(heights[1] > 65000);
        let flipped = inverted(&heights);
        assert!(flipped[0] > 65000 && flipped[1] < 10);
    }

    #[test]
    fn large_pictures_are_reduced() {
        let img = Image { w: 3000, h: 1500, rgba: Arc::new(vec![128; 3000 * 1500 * 4]) };
        let (w, h, heights) = picture_heights(&img, 800);
        assert!(w <= 800 && h <= 800 && heights.len() == w * h, "{w} {h}");
    }

    #[test]
    fn smoothing_softens_steps_and_leaves_empty_cells_alone() {
        let heights = vec![1000, 1000, 0, 3000, 3000, 0];
        let soft = smoothed(&heights, 3, 2, 1);
        assert!(soft[0] > 1000 && soft[0] < 3000, "{soft:?}");
        assert_eq!(soft[2], 0);
        assert_eq!(smoothed(&heights, 3, 2, 0), heights);
    }

    #[test]
    fn a_model_lands_on_the_job_with_its_proportions_and_undo_removes_it() {
        let mut a = app();
        a.doc.job.thickness = 10.0;
        a.open_model_bytes("demo.stl".into(), &binary(&pyramid())).unwrap();
        let LeftForm::Relief(f) = a.left.clone() else { panic!("the form is open") };
        assert!((f.width - 10.0).abs() < 1e-9, "fills the 10 wide job");
        assert!((f.height - 5.0).abs() < 1e-9, "true proportions: {}", f.height);
        a.apply_relief(&f);
        assert!(matches!(a.left, LeftForm::None));
        assert!(a.view3d);
        assert_eq!(a.doc.components.len(), 1);
        let j = &a.doc.job;
        let grid = compose(&a.doc.components, j.width, j.height, 40_000).unwrap();
        assert!((grid.sample(5.0, 5.0) - 5.0).abs() < 0.1, "peak {}", grid.sample(5.0, 5.0));
        assert!((grid.sample(2.5, 5.0) - 2.5).abs() < 0.1, "slope {}", grid.sample(2.5, 5.0));
        a.do_undo();
        assert!(a.doc.components.is_empty());
    }

    #[test]
    fn a_model_taller_than_the_material_starts_at_the_material_thickness() {
        let mut a = app();
        a.open_model_bytes("demo.stl".into(), &binary(&pyramid())).unwrap();
        let LeftForm::Relief(f) = a.left.clone() else { panic!("the form is open") };
        assert!((f.height - a.doc.job.thickness).abs() < 1e-9);
        assert!((f.true_ratio.unwrap() * f.width - 5.0).abs() < 1e-9);
    }

    #[test]
    fn placing_a_picture_puts_its_height_where_it_was_asked() {
        let mut a = app();
        let img = Image { w: 2, h: 2, rgba: Arc::new(vec![255; 16]) };
        a.open_relief_picture("white".into(), &img);
        let LeftForm::Relief(mut f) = a.left.clone() else { panic!("the form is open") };
        f.at = [2.0, 3.0];
        f.width = 2.0;
        f.height = 0.4;
        f.smooth = 0;
        a.apply_relief(&f);
        let j = &a.doc.job;
        let grid = compose(&a.doc.components, j.width, j.height, 90_000).unwrap();
        assert!((grid.sample(3.0, 4.0) - 0.4).abs() < 0.01, "inside: {}", grid.sample(3.0, 4.0));
        assert_eq!(grid.sample(6.0, 8.0), 0.0, "nothing outside");
        assert_eq!(grid.sample(1.5, 4.0), 0.0);
    }

    #[test]
    fn imported_reliefs_survive_saving_and_units() {
        let mut a = app();
        let img = Image { w: 2, h: 2, rgba: Arc::new(vec![200; 16]) };
        a.open_relief_picture("grey".into(), &img);
        let LeftForm::Relief(f) = a.left.clone() else { panic!("the form is open") };
        a.apply_relief(&f);
        let text = serde_json::to_string(&a.doc).unwrap();
        let back: Doc = serde_json::from_str(&text).unwrap();
        let s = back.components[0].surface.as_ref().unwrap();
        assert_eq!((s.w, s.h), (f.w, f.h));
        let mut mm = back.clone();
        mm.set_units(Units::Mm);
        let s = mm.components[0].surface.as_ref().unwrap();
        assert!((s.width - f.width * 25.4).abs() < 1e-6);
    }

    #[test]
    fn older_projects_without_reliefs_still_open() {
        let mut a = app();
        a.doc.components.push(Component { name: "old".into(), paths: Vec::new(), profile: Profile::Flat, height: 1.0, base: 0.0, combine: Combine::Add, visible: true, surface: None });
        let text = serde_json::to_string(&a.doc).unwrap();
        assert!(!text.contains("surface"), "nothing extra is written for ordinary shapes");
        let back: Doc = serde_json::from_str(&text).unwrap();
        assert!(back.components[0].surface.is_none());
    }
}
