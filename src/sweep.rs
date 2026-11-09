//! Two Rail Sweep: a component made by hanging cross sections between two rails and sweeping
//! them along.
//!
//! From the original (Model > Create Component > Two Rail Sweep, `uiTwoRailSweepForm` over
//! `uiSweepFormBase`, the manual's Two Rail Sweep page): two rails, open or closed, the first red
//! and the second green; one or more open cross sections; points at the same share of each rail's
//! length are joined. Its choices and their starting values: Scale cross sections with width, on,
//! and Fill centre of inner closed vector rails, off (`ScaleWithWidth`, `FillCentreVector`,
//! `0x140773820`, both set at `0x140773760`); Sweep between spans, on, and Scale to exact height,
//! off, at a height of 1 (`SweepBetweenSpans`, `ScaleExactHeight`, `ScaleToHeightMM` /
//! `ScaleToHeightInch`, `0x140749ac0`, set at `0x14074822d` to `0x140748250`).

use crate::app::App;
use crate::geom::{Path, P};
use crate::i18n::tr;
use crate::relief::{Combine, Component, Grid, Profile};

/// Model > Create Component > Two Rail Sweep.
pub const TWO_RAIL_SWEEP_COMMAND: u32 = 33179;

#[derive(Clone, Debug, PartialEq)]
pub struct SweepForm {
    pub rails: Vec<u64>,
    pub sections: Vec<u64>,
    pub scale_with_width: bool,
    pub fill_centre: bool,
    pub between_spans: bool,
    pub exact_height: bool,
    pub height: f64,
}

impl Default for SweepForm {
    fn default() -> Self {
        SweepForm { rails: Vec::new(), sections: Vec::new(), scale_with_width: true, fill_centre: false, between_spans: true, exact_height: false, height: 1.0 }
    }
}

/// A rail's points, closed rails ending where they began, and the distance along it to each.
struct Rail {
    pts: Vec<P>,
    along: Vec<f64>,
    closed: bool,
}

impl Rail {
    fn new(path: &Path) -> Rail {
        let mut pts = path.pts.clone();
        if path.closed && pts.first() != pts.last() {
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
        }
        let mut along = vec![0.0];
        for w in pts.windows(2) {
            along.push(along.last().copied().unwrap_or(0.0) + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
        }
        Rail { pts, along, closed: path.closed }
    }

    fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point a share `t` of the way along.
    fn at(&self, t: f64) -> P {
        let d = t.clamp(0.0, 1.0) * self.length();
        let i = self.along.partition_point(|a| *a < d).clamp(1, self.pts.len().max(2) - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let span = self.along[i] - self.along[i - 1];
        let k = if span > 0.0 { (d - self.along[i - 1]) / span } else { 0.0 };
        [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]
    }
}

/// A cross section as heights across its own chord: `(share across, height)` pairs in order,
/// and the chord's length. It hangs on the side most of it stands on.
pub fn section_profile(path: &Path) -> Option<(Vec<(f64, f64)>, f64)> {
    let (&a, &b) = (path.pts.first()?, path.pts.last()?);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = dx.hypot(dy);
    if chord <= 1e-9 {
        return None;
    }
    let (ux, uy) = (dx / chord, dy / chord);
    let mut pairs: Vec<(f64, f64)> = path.pts.iter().map(|p| {
        let (px, py) = (p[0] - a[0], p[1] - a[1]);
        ((px * ux + py * uy) / chord, -px * uy + py * ux)
    }).collect();
    let up: f64 = pairs.iter().map(|q| q.1).sum();
    if up < 0.0 {
        for q in &mut pairs {
            q.1 = -q.1;
        }
    }
    Some((pairs, chord))
}

/// The height of a profile a share `u` across, following its points in order.
fn profile_height(profile: &[(f64, f64)], u: f64) -> f64 {
    for w in profile.windows(2) {
        let (lo, hi) = if w[0].0 <= w[1].0 { (w[0], w[1]) } else { (w[1], w[0]) };
        if u >= lo.0 && u <= hi.0 {
            let span = hi.0 - lo.0;
            return if span > 1e-12 { lo.1 + (hi.1 - lo.1) * (u - lo.0) / span } else { lo.1.max(hi.1) };
        }
    }
    0.0
}

/// The swept heights over a grid like `like`: the sections spread evenly along the rails in
/// their order (one alone is used all along), blended from one to the next, hung between the
/// points at the same share of each rail's length.
pub fn sweep_grid(rail_a: &Path, rail_b: &Path, sections: &[Path], form: &SweepForm, like: &Grid) -> Option<Grid> {
    let (ra, rb) = (Rail::new(rail_a), Rail::new(rail_b));
    if ra.pts.len() < 2 || rb.pts.len() < 2 {
        return None;
    }
    let profiles: Vec<(Vec<(f64, f64)>, f64)> = sections.iter().filter_map(section_profile).collect();
    if profiles.is_empty() {
        return None;
    }
    let mut g = Grid { data: vec![0.0; like.data.len()], ..like.clone() };
    let (nx, ny, cell) = (g.nx, g.ny, g.cell);
    let stations = ((ra.length().max(rb.length()) / (cell * 0.5)).ceil() as usize).max(2);
    let m = profiles.len();
    // Where on the inner closed rail the sweep ends up, for filling the middle.
    let mut edge: Vec<f64> = Vec::new();
    for s in 0..=stations {
        let t = s as f64 / stations as f64;
        let (a, b) = (ra.at(t), rb.at(t));
        let width = (b[0] - a[0]).hypot(b[1] - a[1]);
        let (k, frac) = if m == 1 {
            (0, 0.0)
        } else {
            let at = t * (m - 1) as f64;
            let k = (at.floor() as usize).min(m - 2);
            (k, at - k as f64)
        };
        let height_at = |u: f64| -> f64 {
            let one = |p: &(Vec<(f64, f64)>, f64)| {
                let h = profile_height(&p.0, u);
                if form.scale_with_width { h * width / p.1 } else { h }
            };
            if m == 1 { one(&profiles[0]) } else { one(&profiles[k]) * (1.0 - frac) + one(&profiles[k + 1]) * frac }
        };
        let across = ((width / (cell * 0.5)).ceil() as usize).max(1);
        for j in 0..=across {
            let u = j as f64 / across as f64;
            let p = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
            let (ix, iy) = ((p[0] / cell).round(), (p[1] / cell).round());
            if ix < 0.0 || iy < 0.0 || ix >= nx as f64 || iy >= ny as f64 {
                continue;
            }
            let i = iy as usize * nx + ix as usize;
            g.data[i] = g.data[i].max(height_at(u).max(0.0) as f32);
        }
        edge.push(height_at(1.0));
        edge.push(height_at(0.0));
    }
    // Places between samples that were missed, closed in by the sweep on all four sides.
    let snapshot = g.data.clone();
    for iy in 1..ny.saturating_sub(1) {
        for ix in 1..nx.saturating_sub(1) {
            let i = iy * nx + ix;
            if snapshot[i] == 0.0 {
                let around = [snapshot[i - 1], snapshot[i + 1], snapshot[i - nx], snapshot[i + nx]];
                if around.iter().all(|h| *h > 0.0) {
                    g.data[i] = around.iter().sum::<f32>() / 4.0;
                }
            }
        }
    }
    // Fill centre: with two closed rails, the inside of the inner one is raised to the height the
    // sweep reaches along it.
    if form.fill_centre && ra.closed && rb.closed {
        let area = |r: &Rail| crate::geom::signed_area(&r.pts).abs();
        let inner_is_a = area(&ra) < area(&rb);
        let inner = if inner_is_a { rail_a } else { rail_b };
        let heights: Vec<f64> = edge.chunks(2).map(|c| if inner_is_a { c[1] } else { c[0] }).collect();
        let level = (heights.iter().sum::<f64>() / heights.len().max(1) as f64).max(0.0) as f32;
        let mask = crate::relief::raster_mask(std::slice::from_ref(inner), nx, ny, cell);
        for (h, inside) in g.data.iter_mut().zip(mask) {
            if inside && *h < level {
                *h = level;
            }
        }
    }
    if form.exact_height {
        let top = g.data.iter().copied().fold(0.0f32, f32::max);
        if top > 0.0 {
            let k = form.height as f32 / top;
            for h in &mut g.data {
                *h *= k;
            }
        }
    }
    Some(g)
}

impl App {
    pub fn open_two_rail_sweep(&mut self) {
        let mut form = SweepForm::default();
        self.sweep_use_selection(&mut form);
        self.sweep_form = Some(form);
    }

    /// The first two chosen shapes are the rails, in the order they were chosen; the other open
    /// ones are the cross sections.
    fn sweep_use_selection(&mut self, form: &mut SweepForm) {
        let chosen: Vec<u64> = self.selected.clone();
        form.rails = chosen.iter().take(2).copied().collect();
        form.sections = chosen.iter().skip(2).copied().filter(|id| self.doc.vectors.iter().any(|v| v.id == *id && v.paths.first().is_some_and(|p| !p.closed))).collect();
    }

    fn first_path(&self, id: u64) -> Option<Path> {
        self.doc.vectors.iter().find(|v| v.id == id).and_then(|v| v.paths.first().cloned())
    }

    /// Makes the swept component, combining by Add. Rails come in left and right pairs, in the
    /// order chosen, as in the original (`0x14074a880`: an odd number is its error 3; each pair is
    /// marked left 1 and right 2 and pointed at each other). Every pair is swept with the same cross
    /// sections, and where two sweeps meet the higher wins.
    pub fn two_rail_sweep(&mut self, form: &SweepForm) -> bool {
        if form.rails.len() < 2 {
            self.message = Some(tr("Choose two or more vectors as rails, in left and right pairs, in order.").into());
            return false;
        }
        if form.rails.len() % 2 == 1 {
            self.message = Some(tr("Rails go in left and right pairs, in order: choose an even number of them.").into());
            return false;
        }
        let pairs: Vec<(Path, Path)> = form.rails.chunks(2).filter_map(|c| Some((self.first_path(c[0])?, self.first_path(c[1])?))).collect();
        if pairs.is_empty() {
            self.message = Some(tr("Choose two or more vectors as rails, in left and right pairs, in order.").into());
            return false;
        }
        let sections: Vec<Path> = form.sections.iter().filter_map(|id| self.first_path(*id)).collect();
        if sections.is_empty() {
            self.message = Some(tr("No cross section is attached to the rails: choose an open vector as the cross section, last.").into());
            return false;
        }
        let j = &self.doc.job;
        let like = Grid::new(j.width, j.height, j.points(), 0.0);
        let mut grid: Option<Grid> = None;
        for (a, b) in &pairs {
            let Some(g) = sweep_grid(a, b, &sections, form, &like) else { continue };
            match &mut grid {
                Some(all) => {
                    for (h, n) in all.data.iter_mut().zip(&g.data) {
                        *h = h.max(*n);
                    }
                }
                None => grid = Some(g),
            }
        }
        let Some(grid) = grid else { return false };
        let Some((surface, top)) = crate::modeltools::surface_from(&grid) else {
            self.message = Some(tr("Failed to create sweep shape data").into());
            return false;
        };
        self.checkpoint();
        let paths = crate::modeltools::outline_of(&grid);
        self.doc.components.push(Component { name: tr("Two Rail Sweep").into(), paths, profile: Profile::Flat, height: top, base: 0.0, combine: Combine::Add, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.model_dirty = true;
        self.sim = None;
        self.dirty = true;
        true
    }

    pub(crate) fn sweep_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.sweep_form.take() else { return };
        let mut open = true;
        let (mut apply, mut close, mut use_selection, mut add_pair) = (false, false, false, false);
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Two Rail Sweep")).id(egui::Id::unique("two-rail-sweep")).open(&mut open).collapsible(false).resizable(false).default_pos(ctx.content_rect().left_top() + egui::vec2(260.0, 90.0)).show(ctx, |ui| {
            ui.set_max_width(300.0);
            ui.label(egui::RichText::new(tr("Choose the two rails first, then the open cross sections, and click Use Selection.")).size(12.0).color(crate::theme::MUTED()));
            ui.label(tf!("Rail pairs: {}, cross sections: {}", form.rails.len() / 2, form.sections.len()));
            ui.horizontal(|ui| {
                use_selection = ui.button(tr("Use Selection")).clicked();
                add_pair = ui.button(tr("Add Pair")).on_hover_text(tr("Adds the two chosen shapes as another pair of rails, left then right.")).clicked();
            });
            ui.horizontal(|ui| {
                // Left and right swap within every pair.
                if ui.add_enabled(form.rails.len() >= 2, egui::Button::new(tr("Swap Rail Order"))).clicked() {
                    for pair in form.rails.chunks_mut(2) {
                        if pair.len() == 2 {
                            pair.swap(0, 1);
                        }
                    }
                }
                if ui.button(tr("Clear Rails")).clicked() {
                    form.rails.clear();
                    form.sections.clear();
                }
            });
            ui.checkbox(&mut form.scale_with_width, tr("Scale cross sections with width"));
            ui.checkbox(&mut form.between_spans, tr("Sweep between spans"));
            ui.checkbox(&mut form.fill_centre, tr("Fill centre of inner closed vector rails"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut form.exact_height, tr("Scale to exact height"));
                ui.add_enabled_ui(form.exact_height, |ui| {
                    crate::ui::number(ui, &mut form.height, 50.0);
                    ui.label(unit);
                });
            });
            ui.horizontal(|ui| {
                apply = ui.button(tr("Apply")).clicked();
                close = ui.button(tr("Close")).clicked();
            });
        });
        if use_selection {
            self.sweep_use_selection(&mut form);
        }
        if add_pair {
            if self.selected.len() == 2 && form.rails.len() % 2 == 0 {
                form.rails.extend_from_slice(&self.selected);
            } else {
                self.message = Some(tr("Choose exactly two shapes: the left rail, then the right one.").into());
            }
        }
        if apply {
            self.two_rail_sweep(&form);
        }
        self.sweep_form = if close || !open { None } else { Some(form) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: P, b: P) -> Path {
        Path::open(vec![a, b])
    }

    /// An arch over 1 wide and 0.5 high.
    fn arch() -> Path {
        Path::open((0..=20).map(|k| {
            let u = k as f64 / 20.0;
            [u, 0.5 * (std::f64::consts::PI * u).sin()]
        }).collect())
    }

    /// A section 1 wide and 0.5 high, rounded.
    fn bump() -> Path {
        arch()
    }

    /// A bump 1 long and 0.6 wide, highest at its middle, standing at (3, 3).
    fn bead() -> Piece {
        let mut g = Grid::new(10.0, 10.0, 40_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell - 3.5, iy as f64 * g.cell - 3.3);
                let r = (x / 0.5).powi(2) + (y / 0.3).powi(2);
                if r < 1.0 {
                    g.data[iy * g.nx + ix] = (0.4 * (1.0 - r).sqrt()) as f32;
                }
            }
        }
        Piece::from_grid(g).unwrap()
    }

    #[test]
    fn a_component_is_repeated_along_a_rail() {
        let p = bead();
        assert!((p.length - 1.0).abs() < 0.12 && (p.width - 0.6).abs() < 0.12, "{} × {}", p.length, p.width);
        let like = Grid::new(10.0, 10.0, 40_000, 0.0);
        let rail = line([1.0, 5.0], [7.0, 5.0]);
        let g = extrude_with(&[rail], &[], Some(&p), &ExtrudeForm::default(), &like).unwrap();
        // Six copies exactly: high in the middle of each, low where two meet.
        let step = 6.0 / (6.0 / p.length).round();
        assert!(g.sample(1.0 + step * 0.5, 5.0) > 0.3, "{}", g.sample(1.0 + step * 0.5, 5.0));
        assert!(g.sample(1.0 + step * 2.5, 5.0) > 0.3);
        assert!(g.sample(1.0 + step * 2.0, 5.0) < 0.25, "{}", g.sample(1.0 + step * 2.0, 5.0));
        assert_eq!(g.sample(4.0, 5.6), 0.0, "its own width across");
        // Not tiled: one copy stretched over the whole rail, highest at the middle only.
        let one = extrude_with(&[line([1.0, 5.0], [7.0, 5.0])], &[], Some(&p), &ExtrudeForm { tile: false, ..ExtrudeForm::default() }, &like).unwrap();
        assert!(one.sample(4.0, 5.0) > 0.3 && one.sample(1.0 + step * 2.0, 5.0) > 0.25);
    }

    #[test]
    fn a_section_runs_along_a_rail_at_its_own_size() {
        let like = Grid::new(10.0, 10.0, 40_000, 0.0);
        let rail = line([2.0, 5.0], [8.0, 5.0]);
        let g = extrude_grid(&[rail], &[bump()], &ExtrudeForm::default(), &like).unwrap();
        assert!((g.sample(5.0, 5.0) - 0.5).abs() < 0.03, "its own height on the rail: {}", g.sample(5.0, 5.0));
        assert!(g.sample(5.0, 5.4) > 0.0 && g.sample(5.0, 5.6) == 0.0, "its own width, 1 across");
        assert_eq!(g.sample(9.0, 5.0), 0.0, "an open rail ends square");
    }

    #[test]
    fn sharp_corners_fill_the_outside_of_a_corner() {
        let like = Grid::new(10.0, 10.0, 90_000, 0.0);
        let square = Path::closed(vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]]);
        let sharp = extrude_grid(std::slice::from_ref(&square), &[bump()], &ExtrudeForm::default(), &like).unwrap();
        let round = extrude_grid(&[square], &[bump()], &ExtrudeForm { sharp_corners: false, ..ExtrudeForm::default() }, &like).unwrap();
        // Just outside the corner, along the diagonal.
        let (x, y) = (8.3, 8.3);
        assert!(sharp.sample(x, y) > round.sample(x, y) + 0.05, "{} vs {}", sharp.sample(x, y), round.sample(x, y));
    }

    #[test]
    fn weaving_alternates_over_and_under() {
        let like = Grid::new(10.0, 10.0, 90_000, 0.0);
        let a = Path::closed(crate::geom::circle([4.0, 5.0], 2.5));
        let b = Path::closed(crate::geom::circle([6.0, 5.0], 2.5));
        let crossings = weave_crossings(&[a.clone(), b.clone()]);
        assert_eq!(crossings[0].len(), 2);
        assert_ne!(crossings[0][0].1, crossings[0][1].1, "over then under along one rail");
        // Where they cross, the other rail does the opposite.
        assert!(crossings[0].iter().all(|c| crossings[1].iter().filter(|d| d.1 == c.1).count() == 1));
        let woven = extrude_grid(&[a.clone(), b.clone()], &[bump()], &ExtrudeForm { weave: true, ..ExtrudeForm::default() }, &like).unwrap();
        let flat = extrude_grid(&[a, b], &[bump()], &ExtrudeForm::default(), &like).unwrap();
        let top = |g: &Grid| g.data.iter().copied().fold(0.0f32, f32::max);
        assert!((top(&woven) - 0.5 * WEAVE_OVER as f32).abs() < 0.03, "raised to 130% where it goes over: {}", top(&woven));
        assert!((top(&flat) - 0.5).abs() < 0.03);
    }

    #[test]
    fn a_section_hangs_between_two_rails() {
        let like = Grid::new(10.0, 10.0, 40_000, 0.0);
        // Rails 2 apart at the start, 4 apart at the end.
        let (a, b) = (line([1.0, 4.0], [9.0, 3.0]), line([1.0, 6.0], [9.0, 7.0]));
        let form = SweepForm::default();
        let g = sweep_grid(&a, &b, &[arch()], &form, &like).unwrap();
        let h_start = g.sample(1.2, 5.0);
        let h_end = g.sample(8.8, 5.0);
        assert!((h_start - 0.5 * 2.0).abs() < 0.08, "scaled with a width of 2: {h_start}");
        assert!((h_end - 0.5 * 4.0).abs() < 0.12, "and of 4: {h_end}");
        assert!(g.sample(5.0, 4.1) < g.sample(5.0, 5.0), "lower at the rails than between");
        assert_eq!(g.sample(5.0, 9.0), 0.0, "nothing outside the rails");
        // Without scaling, the arch keeps its own height.
        let plain = sweep_grid(&a, &b, &[arch()], &SweepForm { scale_with_width: false, ..SweepForm::default() }, &like).unwrap();
        assert!((plain.sample(8.8, 5.0) - 0.5).abs() < 0.05);
        // Scale to exact height.
        let exact = sweep_grid(&a, &b, &[arch()], &SweepForm { exact_height: true, height: 0.3, ..SweepForm::default() }, &like).unwrap();
        assert!((exact.data.iter().copied().fold(0.0f32, f32::max) - 0.3).abs() < 1e-5);
    }

    #[test]
    fn sections_blend_along_the_rails_and_the_centre_fills() {
        let like = Grid::new(10.0, 10.0, 40_000, 0.0);
        let (a, b) = (line([1.0, 4.0], [9.0, 4.0]), line([1.0, 6.0], [9.0, 6.0]));
        let flat_low = Path::open(vec![[0.0, 0.0], [0.0, 0.2], [1.0, 0.2], [1.0, 0.0]]);
        let flat_high = Path::open(vec![[0.0, 0.0], [0.0, 0.8], [1.0, 0.8], [1.0, 0.0]]);
        let form = SweepForm { scale_with_width: false, ..SweepForm::default() };
        let g = sweep_grid(&a, &b, &[flat_low, flat_high], &form, &like).unwrap();
        assert!((g.sample(1.1, 5.0) - 0.2).abs() < 0.03 && (g.sample(8.9, 5.0) - 0.8).abs() < 0.03 && (g.sample(5.0, 5.0) - 0.5).abs() < 0.03);
        // A ring between two closed squares, filled in the middle at the height along the inner one.
        let outer = Path::closed(vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0]]);
        let inner = Path::closed(vec![[3.0, 3.0], [7.0, 3.0], [7.0, 7.0], [3.0, 7.0]]);
        // Heights are measured from the line between a section's two ends: a plateau 0.4 high.
        let ramp = Path::open(vec![[0.0, 0.0], [0.0, 0.4], [1.0, 0.4], [1.0, 0.0]]);
        let ring = SweepForm { scale_with_width: false, fill_centre: true, ..SweepForm::default() };
        let g = sweep_grid(&outer, &inner, &[ramp], &ring, &like).unwrap();
        assert!((g.sample(5.0, 5.0) - 0.4).abs() < 0.05, "the middle filled to the inner edge: {}", g.sample(5.0, 5.0));
    }
}

// ----- Extrude and Weave -----

/// Model > Create Component > Extrude and Weave (33607). Its form shares `uiSweepFormBase` with the
/// two-rail sweep and keeps, with their starting values (`0x14037ba3c` to `0x14037baee`, loaded by
/// `0x14037bd20`): `SquareCorners` on, `TileComponents` on, `TileComponentsExactly` on,
/// `TileComponentsMirror` off, `ComponentOverlap` 0, `DoWeave` off, `WeaveAddBase` off, and the
/// weave's under and over scales 0.7 and 1.3 (`WeaveUnderScale`, `WeaveOverScale`, and the same for
/// `WeaveUnderAddBase`, `WeaveOverAddBase`).
pub const EXTRUDE_COMMAND: u32 = 33607;
pub const WEAVE_UNDER: f64 = 0.7;
pub const WEAVE_OVER: f64 = 1.3;

#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeForm {
    pub rails: Vec<u64>,
    pub sections: Vec<u64>,
    pub sharp_corners: bool,
    pub weave: bool,
    pub under: f64,
    pub over: f64,
    pub exact_height: bool,
    pub height: f64,
    /// With a component as the cross section: copies of it along the rail (`TileComponents`), as
    /// many whole copies as fit exactly (`TileComponentsExactly`), every other one mirrored
    /// (`TileComponentsMirror`), overlapping by this distance (`ComponentOverlap`).
    pub tile: bool,
    pub tile_exactly: bool,
    pub tile_mirror: bool,
    pub overlap: f64,
}

impl Default for ExtrudeForm {
    fn default() -> Self {
        ExtrudeForm { rails: Vec::new(), sections: Vec::new(), sharp_corners: true, weave: false, under: WEAVE_UNDER, over: WEAVE_OVER, exact_height: false, height: 1.0, tile: true, tile_exactly: true, tile_mirror: false, overlap: 0.0 }
    }
}

/// The original samples a component used as a cross section at half the model's resolution
/// (`0x14074b4d0`: resolution × 0.5 at `0x14210331c`).
pub const COMPONENT_SECTION_SAMPLING: f64 = 0.5;

/// A component used as the cross section: its heights, and the box they stand in. Its length runs
/// along the rail and its width across it.
#[derive(Clone, Debug)]
pub struct Piece {
    pub grid: Grid,
    pub x0: f64,
    pub y0: f64,
    pub length: f64,
    pub width: f64,
}

impl Piece {
    /// The box of the cells a component raises, or nothing when it raises none.
    pub fn from_grid(grid: Grid) -> Option<Piece> {
        let (mut lo, mut hi) = ([usize::MAX; 2], [0usize; 2]);
        for iy in 0..grid.ny {
            for ix in 0..grid.nx {
                if grid.data[iy * grid.nx + ix] > 0.0 {
                    lo = [lo[0].min(ix), lo[1].min(iy)];
                    hi = [hi[0].max(ix), hi[1].max(iy)];
                }
            }
        }
        if lo[0] == usize::MAX || hi[0] <= lo[0] || hi[1] <= lo[1] {
            return None;
        }
        let c = grid.cell;
        Some(Piece { x0: lo[0] as f64 * c, y0: lo[1] as f64 * c, length: (hi[0] - lo[0]) as f64 * c, width: (hi[1] - lo[1]) as f64 * c, grid })
    }

    /// The height at a distance along the rail (of `total`) and an offset across it.
    pub fn height_at(&self, at: f64, off: f64, total: f64, form: &ExtrudeForm) -> f64 {
        let v = off + self.width / 2.0;
        if !(0.0..=self.width).contains(&v) {
            return 0.0;
        }
        let l = self.length;
        let sample = |u: f64, mirrored: bool| {
            if !(0.0..=l).contains(&u) {
                return 0.0;
            }
            let u = if mirrored { l - u } else { u };
            self.grid.sample(self.x0 + u, self.y0 + v) as f64
        };
        if !form.tile {
            // One copy stretched over the whole rail.
            return sample(at / total.max(1e-12) * l, false);
        }
        let step = (l - form.overlap).max(l * 0.05);
        // Exactly: stretch the copies so a whole number of them fills the rail.
        let (step, k) = if form.tile_exactly {
            let n = (total / step).round().max(1.0);
            (total / n, total / n / step)
        } else {
            (step, 1.0)
        };
        let i = (at / step).floor();
        let mut h = 0.0f64;
        for j in [i - 1.0, i] {
            if j < 0.0 {
                continue;
            }
            let u = (at - j * step) / k;
            h = h.max(sample(u, form.tile_mirror && (j as i64) % 2 == 1));
        }
        h
    }
}

/// Where two segments cross, as shares along each.
fn segments_cross(a: P, b: P, c: P, d: P) -> Option<(f64, f64)> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    (t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9).then_some((t, u))
}

/// For each rail, the places along it (as distances) where it crosses a rail, and whether it goes
/// over there. Walking the rails in order, the crossings met alternate over and under; a crossing
/// met the second time takes the other.
pub fn weave_crossings(rails: &[Path]) -> Vec<Vec<(f64, bool)>> {
    let rs: Vec<Rail> = rails.iter().map(Rail::new).collect();
    // Every crossing: (rail, distance along it, other rail, distance along that).
    let mut found: Vec<(usize, f64, usize, f64)> = Vec::new();
    for i in 0..rs.len() {
        for j in i..rs.len() {
            for si in 0..rs[i].pts.len() - 1 {
                for sj in 0..rs[j].pts.len() - 1 {
                    if i == j && sj <= si + 1 {
                        continue;
                    }
                    if i == j && rs[i].closed && si == 0 && sj == rs[i].pts.len() - 2 {
                        continue;
                    }
                    let (a, b, c, d) = (rs[i].pts[si], rs[i].pts[si + 1], rs[j].pts[sj], rs[j].pts[sj + 1]);
                    if let Some((t, u)) = segments_cross(a, b, c, d) {
                        let da = rs[i].along[si] + t * (rs[i].along[si + 1] - rs[i].along[si]);
                        let db = rs[j].along[sj] + u * (rs[j].along[sj + 1] - rs[j].along[sj]);
                        found.push((i, da, j, db));
                    }
                }
            }
        }
    }
    let mut out: Vec<Vec<(f64, bool)>> = vec![Vec::new(); rs.len()];
    let mut decided: Vec<Option<bool>> = vec![None; found.len()];
    for rail in 0..rs.len() {
        let mut meets: Vec<(f64, usize, bool)> = Vec::new();
        for (k, f) in found.iter().enumerate() {
            if f.0 == rail {
                meets.push((f.1, k, true));
            }
            if f.2 == rail {
                meets.push((f.3, k, false));
            }
        }
        meets.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut next_over = true;
        for (at, k, first_side) in meets {
            let over = match decided[k] {
                // The other strand already passed here: this one goes the other way.
                Some(other_over) => !other_over,
                None => {
                    decided[k] = Some(next_over);
                    next_over
                }
            };
            let _ = first_side;
            out[rail].push((at, over));
            next_over = !over;
        }
    }
    out
}

/// The heights of cross sections carried along each rail at their own size, centred on it. With
/// sharp corners each straight piece of a rail reaches out to the lines halving its corners;
/// otherwise the shape rounds the outside of a corner. With weaving, each rail is raised where it
/// goes over a crossing and lowered where it goes under, easing in over the section's width.
#[cfg(test)]
pub fn extrude_grid(rails: &[Path], sections: &[Path], form: &ExtrudeForm, like: &Grid) -> Option<Grid> {
    extrude_with(rails, sections, None, form, like)
}

/// The same, with a component as the cross section when `piece` is given and no section is.
pub fn extrude_with(rails: &[Path], sections: &[Path], piece: Option<&Piece>, form: &ExtrudeForm, like: &Grid) -> Option<Grid> {
    let profiles: Vec<(Vec<(f64, f64)>, f64)> = sections.iter().filter_map(section_profile).collect();
    let piece = if profiles.is_empty() { piece } else { None };
    if (profiles.is_empty() && piece.is_none()) || rails.is_empty() {
        return None;
    }
    let crossings = if form.weave { weave_crossings(rails) } else { vec![Vec::new(); rails.len()] };
    let mut g = Grid { data: vec![0.0; like.data.len()], ..like.clone() };
    let (nx, ny, cell) = (g.nx, g.ny, g.cell);
    let m = profiles.len();
    let widest = match piece {
        Some(p) => p.width,
        None => profiles.iter().map(|p| p.1).fold(0.0, f64::max),
    };
    for (ri, path) in rails.iter().enumerate() {
        let rail = Rail::new(path);
        let total = rail.length();
        if total <= 0.0 {
            continue;
        }
        let n = rail.pts.len();
        let segs = n - 1;
        let dir = |k: usize| {
            let (a, b) = (rail.pts[k], rail.pts[k + 1]);
            let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
            [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
        };
        // The best place on the rail for each cell: |offset| and (offset, distance along).
        let mut best: Vec<Option<(f64, f64, f64)>> = vec![None; nx * ny];
        for k in 0..segs {
            let (a, b) = (rail.pts[k], rail.pts[k + 1]);
            let t = dir(k);
            let len = rail.along[k + 1] - rail.along[k];
            if len <= 1e-12 {
                continue;
            }
            let nrm = [-t[1], t[0]];
            // The lines halving the corners at each end, for sharp corners.
            let prev = if k > 0 { Some(dir(k - 1)) } else if rail.closed { Some(dir(segs - 1)) } else { None };
            let next = if k + 1 < segs { Some(dir(k + 1)) } else if rail.closed { Some(dir(0)) } else { None };
            let reach = widest / 2.0 + cell;
            let lo = [a[0].min(b[0]) - reach * 3.0, a[1].min(b[1]) - reach * 3.0];
            let hi = [a[0].max(b[0]) + reach * 3.0, a[1].max(b[1]) + reach * 3.0];
            let (x0, x1) = ((lo[0] / cell).floor().max(0.0) as usize, ((hi[0] / cell).ceil() as usize).min(nx - 1));
            let (y0, y1) = ((lo[1] / cell).floor().max(0.0) as usize, ((hi[1] / cell).ceil() as usize).min(ny - 1));
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    let p = [ix as f64 * cell - a[0], iy as f64 * cell - a[1]];
                    let along = p[0] * t[0] + p[1] * t[1];
                    let off = p[0] * nrm[0] + p[1] * nrm[1];
                    if off.abs() > widest / 2.0 {
                        continue;
                    }
                    let inside = if form.sharp_corners {
                        // Ahead of the halving line at the start, behind the one at the end.
                        let start_ok = match prev {
                            Some(pd) => {
                                let h = [t[0] + pd[0], t[1] + pd[1]];
                                p[0] * h[0] + p[1] * h[1] >= 0.0
                            }
                            None => along >= 0.0,
                        };
                        let pe = [p[0] - (b[0] - a[0]), p[1] - (b[1] - a[1])];
                        let end_ok = match next {
                            Some(nd) => {
                                let h = [t[0] + nd[0], t[1] + nd[1]];
                                pe[0] * h[0] + pe[1] * h[1] <= 0.0
                            }
                            None => along <= len,
                        };
                        start_ok && end_ok
                    } else {
                        along >= 0.0 && along <= len
                    };
                    let (off, along) = if inside {
                        (off, along.clamp(0.0, len))
                    } else if !form.sharp_corners {
                        // Round corners: the distance to the corner point itself, on joints only.
                        let joint = if along < 0.0 && (k > 0 || rail.closed) { Some((a, 0.0)) } else if along > len && (k + 1 < segs || rail.closed) { Some((b, len)) } else { None };
                        match joint {
                            Some((q, at)) => {
                                let d = (ix as f64 * cell - q[0]).hypot(iy as f64 * cell - q[1]);
                                (d * off.signum(), at)
                            }
                            None => continue,
                        }
                    } else {
                        continue;
                    };
                    let i = iy * nx + ix;
                    if best[i].is_none_or(|b| off.abs() < b.0) {
                        best[i] = Some((off.abs(), off, rail.along[k] + along));
                    }
                }
            }
        }
        for (i, b) in best.iter().enumerate() {
            let Some((_, off, at)) = *b else { continue };
            let t = at / total;
            let (k, frac) = if m <= 1 { (0, 0.0) } else {
                let s = t * (m - 1) as f64;
                let k = (s.floor() as usize).min(m - 2);
                (k, s - k as f64)
            };
            let one = |p: &(Vec<(f64, f64)>, f64)| if off.abs() <= p.1 / 2.0 { profile_height(&p.0, 0.5 + off / p.1) } else { 0.0 };
            let mut h = if let Some(p) = piece {
                p.height_at(at, off, total, form)
            } else if m == 1 {
                one(&profiles[0])
            } else {
                one(&profiles[k]) * (1.0 - frac) + one(&profiles[k + 1]) * frac
            };
            // Weaving: up or down near each crossing, easing over the section's width.
            for &(cross, over) in &crossings[ri] {
                let d = (at - cross).abs();
                let ease = widest * 1.5;
                if d < ease {
                    let w = 0.5 * (1.0 + (std::f64::consts::PI * d / ease).cos());
                    let k = if over { form.over } else { form.under };
                    h *= 1.0 + (k - 1.0) * w;
                }
            }
            g.data[i] = g.data[i].max(h.max(0.0) as f32);
        }
    }
    if form.exact_height {
        let top = g.data.iter().copied().fold(0.0f32, f32::max);
        if top > 0.0 {
            let k = form.height as f32 / top;
            for h in &mut g.data {
                *h *= k;
            }
        }
    }
    Some(g)
}

impl App {
    pub fn open_extrude(&mut self) {
        let mut form = ExtrudeForm::default();
        self.extrude_use_selection(&mut form);
        self.extrude_form = Some(form);
    }

    /// The last chosen shape is the cross section, when it's open; every one chosen before it is a rail.
    fn extrude_use_selection(&mut self, form: &mut ExtrudeForm) {
        let chosen = self.selected.clone();
        let open = |id: &u64| self.doc.vectors.iter().any(|v| v.id == *id && v.paths.first().is_some_and(|p| !p.closed));
        match chosen.split_last() {
            Some((last, rails)) if open(last) && !rails.is_empty() => {
                form.rails = rails.to_vec();
                form.sections = vec![*last];
            }
            _ => {
                form.rails = chosen;
                form.sections.clear();
            }
        }
    }

    /// Every path of a shape, each one a rail of its own, as the original makes one rail of each
    /// span of each chosen vector (`0x14074a880`). Rails of no length are left out.
    fn rail_paths(&self, id: u64) -> Vec<Path> {
        let Some(v) = self.doc.vectors.iter().find(|v| v.id == id) else { return Vec::new() };
        v.paths.iter().filter(|p| Rail::new(p).length() > 0.0).cloned().collect()
    }

    pub fn extrude(&mut self, form: &ExtrudeForm) -> bool {
        let rails: Vec<Path> = form.rails.iter().flat_map(|id| self.rail_paths(*id)).collect();
        let sections: Vec<Path> = form.sections.iter().filter_map(|id| self.first_path(*id)).collect();
        // The original's checks, in its order: rails first (`0x14074a880`, code 2), then a cross
        // section (`0x14074b4d0`).
        if rails.is_empty() {
            self.message = Some(tr("Choose one or more vectors as rails.").into());
            return false;
        }
        // With no cross section the original uses the one chosen component instead (`0x14074b4d0`).
        let piece = if sections.is_empty() {
            let j = &self.doc.job;
            // Half the model's spacing: four times the points.
            let points = (j.points() as f64 / (COMPONENT_SECTION_SAMPLING * COMPONENT_SECTION_SAMPLING)) as usize;
            let sampled = |c: usize| crate::relief::component_height(&self.doc.components[c], j.width, j.height, points);
            match self.selected_comp.filter(|c| *c < self.doc.components.len()).and_then(|c| Piece::from_grid(sampled(c))) {
                Some(p) => Some(p),
                None => {
                    self.message = Some(tr("No cross section is attached to the rails: choose an open vector as the cross section, last.").into());
                    return false;
                }
            }
        } else {
            None
        };
        let j = &self.doc.job;
        let like = Grid::new(j.width, j.height, j.points(), 0.0);
        let Some(grid) = extrude_with(&rails, &sections, piece.as_ref(), form, &like) else { return false };
        let Some((surface, top)) = crate::modeltools::surface_from(&grid) else {
            self.message = Some(tr("Failed to create sweep shape data").into());
            return false;
        };
        self.checkpoint();
        let paths = crate::modeltools::outline_of(&grid);
        self.doc.components.push(Component { name: tr("Extrude and Weave").into(), paths, profile: Profile::Flat, height: top, base: 0.0, combine: Combine::Add, visible: true, surface: Some(surface), whole: false });
        // With a component as the cross section it stays chosen, so Apply can be clicked again.
        if piece.is_none() {
            self.selected_comp = Some(self.doc.components.len() - 1);
        }
        self.model_dirty = true;
        self.sim = None;
        self.dirty = true;
        true
    }

    pub(crate) fn extrude_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.extrude_form.take() else { return };
        let mut open = true;
        let (mut apply, mut close, mut use_selection) = (false, false, false);
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Extrude and Weave")).id(egui::Id::unique("extrude-weave")).open(&mut open).collapsible(false).resizable(false).default_pos(ctx.content_rect().left_top() + egui::vec2(260.0, 90.0)).show(ctx, |ui| {
            ui.set_max_width(300.0);
            ui.label(egui::RichText::new(tr("Choose the rails first, then the open cross section last, and click Use Selection.")).size(12.0).color(crate::theme::MUTED()));
            ui.label(tf!("Rails: {}, cross sections: {}", form.rails.len(), form.sections.len()));
            ui.horizontal(|ui| {
                use_selection = ui.button(tr("Use Selection")).clicked();
                if ui.button(tr("Clear Rails")).clicked() {
                    form.rails.clear();
                    form.sections.clear();
                }
            });
            if form.sections.is_empty() {
                let name = self.selected_comp.and_then(|c| self.doc.components.get(c)).map(|c| c.name.clone());
                ui.label(egui::RichText::new(match name {
                    Some(n) => tf!("Cross section: the component {}", n),
                    None => tr("Without an open cross section, the chosen component is used.").to_string(),
                }).size(12.0).color(crate::theme::MUTED()));
            }
            ui.checkbox(&mut form.sharp_corners, tr("Create sharp corners"));
            ui.add_enabled_ui(form.sections.is_empty(), |ui| {
                ui.checkbox(&mut form.tile, tr("Tile components along the rail"));
                ui.add_enabled_ui(form.tile, |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut form.tile_exactly, tr("Exactly"));
                        ui.checkbox(&mut form.tile_mirror, tr("Mirror every other copy"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Overlap"));
                        crate::ui::number(ui, &mut form.overlap, 50.0);
                        ui.label(unit);
                    });
                });
            });
            ui.checkbox(&mut form.weave, tr("Weave under / over at crossings"));
            ui.add_enabled_ui(form.weave, |ui| {
                let (mut under, mut over) = (form.under * 100.0, form.over * 100.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Z Under"));
                    crate::ui::number(ui, &mut under, 44.0);
                    ui.label("%");
                    ui.label(tr("Z Over"));
                    crate::ui::number(ui, &mut over, 44.0);
                    ui.label("%");
                });
                form.under = under / 100.0;
                form.over = over / 100.0;
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut form.exact_height, tr("Scale to exact height"));
                ui.add_enabled_ui(form.exact_height, |ui| {
                    crate::ui::number(ui, &mut form.height, 50.0);
                    ui.label(unit);
                });
            });
            ui.horizontal(|ui| {
                apply = ui.button(tr("Apply")).clicked();
                close = ui.button(tr("Close")).clicked();
            });
        });
        if use_selection {
            self.extrude_use_selection(&mut form);
        }
        if apply {
            self.extrude(&form);
        }
        self.extrude_form = if close || !open { None } else { Some(form) };
    }
}
