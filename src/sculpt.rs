//! Sculpting: changing a component's heights with the mouse in the 3D view, brush by brush.
//!
//! From the original (Model > Sculpt Visible Model, its form `uiSculptingForm`, the manual's
//! Sculpting page): six tools on the keys 1 to 6, Smooth, Smudge, Deposit, Remove, the Undo Brush
//! (Shift erases instead) and Twiddle, which turns the view; a diameter, a strength and a
//! smoothness; Preserve Transparency, on at first (`0x140743cd0`), which keeps the changes inside
//! the component's own edge, and 0 turns it on or off; the mode Normal, Raise or Lower, for
//! smoothing and smudging; and Keep, Discard and Close for the changes. Each tool keeps its own
//! settings (`SmoothingTool`, `EraserTool`, `SmudgeTool`, `DepositTool`, `SubtractTool` in the
//! section "Sculpting", read by `0x140735440`), whose numbers weren't found: Burin's are its own.

use crate::app::App;
use crate::geom::P;
use crate::i18n::tr;
use crate::relief::{Grid, Profile};

/// Model > Sculpt Visible Model (string `0x819e`).
pub const SCULPT_COMMAND: u32 = 33182;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SculptTool {
    Smooth,
    Smudge,
    Deposit,
    Remove,
    Undo,
    Twiddle,
}

impl SculptTool {
    pub const ALL: [SculptTool; 6] = [SculptTool::Smooth, SculptTool::Smudge, SculptTool::Deposit, SculptTool::Remove, SculptTool::Undo, SculptTool::Twiddle];

    pub fn name(self) -> &'static str {
        match self {
            SculptTool::Smooth => tr("Smooth"),
            SculptTool::Smudge => tr("Smudge"),
            SculptTool::Deposit => tr("Deposit"),
            SculptTool::Remove => tr("Remove"),
            SculptTool::Undo => tr("Undo Brush"),
            SculptTool::Twiddle => tr("Twiddle"),
        }
    }
}

/// For smoothing and smudging: either way, only up, or only down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SculptMode {
    Normal,
    Raise,
    Lower,
}

/// Burin's own starting numbers: the brush a tenth of the job's smaller side, at half strength
/// and half smoothness, and each dab of Deposit or Remove a fiftieth of the brush's width at full
/// strength.
pub const SCULPT_DIAMETER_SHARE: f64 = 0.1;
pub const SCULPT_STRENGTH: f64 = 0.5;
pub const SCULPT_SMOOTHNESS: f64 = 0.5;
pub const SCULPT_DAB_SHARE: f64 = 0.02;

#[derive(Clone, Debug)]
pub struct Sculpt {
    /// The component being sculpted.
    pub comp: usize,
    pub tool: SculptTool,
    pub diameter: f64,
    pub strength: f64,
    pub smoothness: f64,
    pub preserve: bool,
    pub mode: SculptMode,
    /// The heights now, over the whole job.
    pub grid: Grid,
    /// As they were when sculpting began: for the Undo Brush and the component's own edge.
    pub original: Grid,
    /// As they were at the last Keep: what Discard goes back to.
    pub kept: Grid,
    /// Where the last dab of this stroke was, for smudging.
    pub last: Option<P>,
}

/// How much of the brush's effect reaches `t` (0 in the middle, 1 at the edge): full inside, then
/// easing off over the outer part, which grows with the smoothness.
pub fn falloff(t: f64, smoothness: f64) -> f64 {
    if t >= 1.0 {
        return 0.0;
    }
    let inner = (1.0 - smoothness).clamp(0.0, 1.0);
    if t <= inner {
        1.0
    } else {
        0.5 * (1.0 + (std::f64::consts::PI * (t - inner) / (1.0 - inner).max(1e-9)).cos())
    }
}

impl Sculpt {
    pub fn new(comp: usize, grid: Grid, job_side: f64) -> Sculpt {
        Sculpt {
            comp,
            tool: SculptTool::Smooth,
            diameter: job_side * SCULPT_DIAMETER_SHARE,
            strength: SCULPT_STRENGTH,
            smoothness: SCULPT_SMOOTHNESS,
            preserve: true,
            mode: SculptMode::Normal,
            original: grid.clone(),
            kept: grid.clone(),
            grid,
            last: None,
        }
    }

    /// One dab of the brush at `p`; `erase` is Shift with the Undo Brush. True if anything changed.
    pub fn dab(&mut self, p: P, erase: bool) -> bool {
        let g = &self.grid;
        let (nx, ny, cell) = (g.nx, g.ny, g.cell);
        let r = (self.diameter / 2.0).max(cell);
        let (x0, x1) = (((p[0] - r) / cell).floor().max(0.0) as usize, (((p[0] + r) / cell).ceil() as usize).min(nx - 1));
        let (y0, y1) = (((p[1] - r) / cell).floor().max(0.0) as usize, (((p[1] + r) / cell).ceil() as usize).min(ny - 1));
        if x0 > x1 || y0 > y1 {
            return false;
        }
        let weight = |ix: usize, iy: usize| -> f64 {
            let (dx, dy) = (ix as f64 * cell - p[0], iy as f64 * cell - p[1]);
            falloff(dx.hypot(dy) / r, self.smoothness)
        };
        // The weighted average under the brush, for smoothing.
        let (mut sum, mut total) = (0.0, 0.0);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let w = weight(ix, iy);
                sum += self.grid.data[iy * nx + ix] as f64 * w;
                total += w;
            }
        }
        let average = if total > 0.0 { sum / total } else { 0.0 };
        let dab = self.diameter * SCULPT_DAB_SHARE * self.strength;
        let shift = self.last.map(|q| [p[0] - q[0], p[1] - q[1]]);
        let before = self.grid.clone();
        let mut changed = false;
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let w = weight(ix, iy);
                if w <= 0.0 {
                    continue;
                }
                let k = iy * nx + ix;
                if self.preserve && self.original.data[k] == 0.0 && !(self.tool == SculptTool::Undo && erase) {
                    continue;
                }
                let h = before.data[k] as f64;
                let mut new = match self.tool {
                    SculptTool::Deposit => h + dab * w,
                    SculptTool::Remove => (h - dab * w).max(0.0),
                    SculptTool::Smooth => h + (average - h) * self.strength * w,
                    SculptTool::Smudge => match shift {
                        Some(d) => {
                            let from = before.sample(ix as f64 * cell - d[0], iy as f64 * cell - d[1]) as f64;
                            h + (from - h) * self.strength * w
                        }
                        None => h,
                    },
                    SculptTool::Undo if erase => {
                        if w > 0.5 {
                            0.0
                        } else {
                            h
                        }
                    }
                    SculptTool::Undo => h + (self.original.data[k] as f64 - h) * self.strength * w,
                    SculptTool::Twiddle => h,
                };
                if matches!(self.tool, SculptTool::Smooth | SculptTool::Smudge) {
                    new = match self.mode {
                        SculptMode::Normal => new,
                        SculptMode::Raise => new.max(h),
                        SculptMode::Lower => new.min(h),
                    };
                }
                if (new - h).abs() > 1e-9 {
                    self.grid.data[k] = new as f32;
                    changed = true;
                }
            }
        }
        self.last = Some(p);
        changed
    }
}

impl App {
    /// Model > Sculpt Visible Model: the chosen component (or the only one) is sculpted in the 3D
    /// view. A component drawn from an outline becomes heights first, as the original bakes it.
    pub fn open_sculpt(&mut self) {
        let comp = match (self.selected_comp, self.doc.components.len()) {
            (Some(i), n) if i < n => i,
            (_, 1) => 0,
            _ => {
                self.message = Some(tr("Choose the component to sculpt in the Modeling tab.").into());
                return;
            }
        };
        let grid = self.component_grid(comp);
        if grid.data.iter().all(|h| *h == 0.0) {
            self.message = Some(tr("The chosen component has nothing in it to sculpt.").into());
            return;
        }
        let side = self.doc.job.width.min(self.doc.job.height);
        self.sculpt = Some(Sculpt::new(comp, grid, side));
        self.selected_comp = Some(comp);
        self.view3d = true;
        self.status = tr("Sculpting: drag over the model in the 3D view. Keys 1 to 6 choose the tool.").into();
    }

    /// Puts the sculpted heights into the component.
    pub(crate) fn write_sculpt(&mut self) {
        let Some(s) = &self.sculpt else { return };
        let Some(c) = self.doc.components.get_mut(s.comp) else { return };
        match crate::modeltools::surface_from(&s.grid) {
            Some((surface, top)) => {
                c.surface = Some(surface);
                c.height = top;
                c.profile = Profile::Flat;
                c.base = 0.0;
                if !s.preserve {
                    c.paths = crate::modeltools::outline_of(&s.grid);
                }
            }
            None => c.height = 0.0,
        }
        self.model_dirty = true;
        self.sim = None;
        self.dirty = true;
    }

    /// One dab at a point of the job, from the 3D view. A stroke's first dab is an undo step.
    pub fn sculpt_at(&mut self, p: P, first: bool, erase: bool) {
        if first {
            self.checkpoint();
            if let Some(s) = self.sculpt.as_mut() {
                s.last = None;
            }
        }
        let changed = self.sculpt.as_mut().is_some_and(|s| s.dab(p, erase));
        if changed {
            self.write_sculpt();
        }
    }

    /// Keep: what's been done so far is what Discard goes back to from now on.
    pub fn keep_sculpt(&mut self) {
        if let Some(s) = self.sculpt.as_mut() {
            s.kept = s.grid.clone();
        }
    }

    /// Discard: back to the last Keep, or to the start.
    pub fn discard_sculpt(&mut self) {
        if let Some(s) = self.sculpt.as_mut() {
            s.grid = s.kept.clone();
        }
        self.checkpoint();
        self.write_sculpt();
    }

    pub(crate) fn sculpt_keys(&mut self, ctx: &egui::Context) {
        if self.sculpt.is_none() || ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
        for (key, tool) in keys.into_iter().zip(SculptTool::ALL) {
            if ctx.input(|i| i.key_pressed(key)) {
                if let Some(s) = self.sculpt.as_mut() {
                    s.tool = tool;
                }
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Num0)) {
            if let Some(s) = self.sculpt.as_mut() {
                s.preserve = !s.preserve;
            }
        }
    }

    pub(crate) fn sculpt_window(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.sculpt.take() else { return };
        let mut open = true;
        let (mut keep, mut discard, mut close) = (false, false, false);
        let unit = self.doc.job.unit();
        let side = self.doc.job.width.min(self.doc.job.height);
        egui::Window::new(tr("Sculpting")).id(egui::Id::unique("sculpting")).open(&mut open).collapsible(false).resizable(false).default_pos(ctx.content_rect().right_top() + egui::vec2(-300.0, 80.0)).show(ctx, |ui| {
            ui.set_max_width(260.0);
            ui.label(egui::RichText::new(tr("Tool")).strong());
            for (n, tool) in SculptTool::ALL.into_iter().enumerate() {
                ui.radio_value(&mut s.tool, tool, format!("{} {}", n + 1, tool.name()));
            }
            ui.label(egui::RichText::new(tr("Hold Shift with the Undo Brush to erase.")).size(11.0).color(crate::theme::MUTED));
            ui.horizontal(|ui| {
                ui.label(tr("Diameter"));
                ui.add(egui::Slider::new(&mut s.diameter, side * 0.005..=side * 0.5).suffix(format!(" {unit}")));
            });
            ui.horizontal(|ui| {
                ui.label(tr("Strength"));
                ui.add(egui::Slider::new(&mut s.strength, 0.01..=1.0).show_value(false));
            });
            ui.horizontal(|ui| {
                ui.label(tr("Smoothness"));
                ui.add(egui::Slider::new(&mut s.smoothness, 0.0..=1.0).show_value(false));
            });
            ui.checkbox(&mut s.preserve, tr("Preserve Transparency (0)"));
            ui.label(egui::RichText::new(tr("Mode")).strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.mode, SculptMode::Normal, tr("Normal"));
                ui.radio_value(&mut s.mode, SculptMode::Raise, tr("Raise"));
                ui.radio_value(&mut s.mode, SculptMode::Lower, tr("Lower"));
            });
            ui.separator();
            ui.horizontal(|ui| {
                keep = ui.button(tr("Keep")).clicked();
                discard = ui.button(tr("Discard")).clicked();
                close = ui.button(tr("Close")).clicked();
            });
        });
        self.sculpt = Some(s);
        if keep {
            self.keep_sculpt();
        }
        if discard {
            self.discard_sculpt();
        }
        if close || !open {
            self.sculpt = None;
            self.status = tr("Sculpting finished.").into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dome() -> Grid {
        let mut g = Grid::new(10.0, 10.0, 40_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell - 5.0, iy as f64 * g.cell - 5.0);
                let d = x.hypot(y);
                if d < 3.0 {
                    g.data[iy * g.nx + ix] = (1.0 - (d / 3.0).powi(2)) as f32;
                }
            }
        }
        g
    }

    #[test]
    fn each_tool_changes_the_heights_its_own_way() {
        let mut s = Sculpt::new(0, dome(), 10.0);
        let middle = |s: &Sculpt| s.grid.sample(5.0, 5.0);
        let h = middle(&s);
        s.tool = SculptTool::Deposit;
        assert!(s.dab([5.0, 5.0], false));
        assert!(middle(&s) > h, "deposit raises");
        s.tool = SculptTool::Remove;
        s.dab([5.0, 5.0], false);
        s.dab([5.0, 5.0], false);
        assert!(middle(&s) < h, "remove lowers");
        s.tool = SculptTool::Undo;
        s.strength = 1.0;
        s.dab([5.0, 5.0], false);
        assert!((middle(&s) - h).abs() < 1e-4, "the undo brush puts it back");
        // Smoothing brings the top down towards the average; Raise keeps it.
        s.tool = SculptTool::Smooth;
        s.diameter = 4.0;
        s.mode = SculptMode::Raise;
        s.dab([5.0, 5.0], false);
        assert!((middle(&s) - h).abs() < 1e-4, "raise only adds");
        s.mode = SculptMode::Normal;
        s.dab([5.0, 5.0], false);
        assert!(middle(&s) < h, "smooth lowers the peak");
        // Preserve Transparency leaves the background alone; off, the brush reaches it.
        s.tool = SculptTool::Deposit;
        let outside = |s: &Sculpt| s.grid.sample(1.0, 1.0);
        s.dab([1.0, 1.0], false);
        assert_eq!(outside(&s), 0.0);
        s.preserve = false;
        s.dab([1.0, 1.0], false);
        assert!(outside(&s) > 0.0);
        // Shift with the undo brush erases.
        s.tool = SculptTool::Undo;
        s.dab([5.0, 5.0], true);
        assert_eq!(middle(&s), 0.0);
    }

    #[test]
    fn smudging_drags_the_heights_along() {
        let mut s = Sculpt::new(0, dome(), 10.0);
        s.tool = SculptTool::Smudge;
        s.strength = 1.0;
        s.diameter = 3.0;
        let before = s.grid.sample(7.5, 5.0);
        s.dab([5.0, 5.0], false);
        s.dab([7.0, 5.0], false);
        assert!(s.grid.sample(7.5, 5.0) > before, "the high middle is pulled outwards");
    }

    #[test]
    fn the_brush_eases_off_by_its_smoothness() {
        assert_eq!(falloff(0.3, 0.5), 1.0);
        assert!(falloff(0.75, 0.5) > 0.0 && falloff(0.75, 0.5) < 1.0);
        assert_eq!(falloff(1.0, 0.5), 0.0);
        assert_eq!(falloff(0.99, 0.0), 1.0, "no smoothness: a hard edge");
    }
}
