//! Model menu tools that work on the 3D shapes' heights: a component made from the visible model, the outline
//! around a component, emptying a component inside or outside closed shapes, and scaling the model's height.
//!
//! Read from the original:
//! - Create Component from Visible Model (33743, `0x1402c6650`): refused when the model is empty or flat (its
//!   lowest point is "nothing", or as high as its highest, message `0x431`); otherwise a new component named
//!   "Copy of Visible Model" (`0x432`) is made from it, and the components it came from stay.
//! - Create Vector Boundary Around Selected Components (33355, `0x1402b3450`): needs components selected
//!   (message `0x325`); makes the outline of where they stand as a vector.
//! - Reset Inside / Outside Selected Vectors (33196, 33197, `VectricCadDoc::DeleteModelInsideOutsideVectors`,
//!   `0x1402c09d0`): needs a component and at least one other object, closed (message `0x277`); every cell of the
//!   component inside (or outside) the shapes is set to 9999999.0 (`0x1420fbc48`), its mark for "nothing here".
//! - Scale Model Z Height (33228, `0x1402c25c0`): needs shapes in the model (message `0x28a`), then opens its form.
//! - Create an Offset Model (33692, `0x1402c5ec0`): refused when the model is empty or flat (message `0x3f3`);
//!   otherwise its form (`uiOffsetModelDialog`, `0x140b08a00`, `0x140b08f20`): Offset Distance, 0.125 inch or 2.5 mm
//!   at first (`OffsetDistanceInch`, `OffsetDistanceMM`), at most 10% of the model's smaller side (`0x1420e8d18`,
//!   message `0x3f5`), and Clip Z values to Zero Plane (`ClipToZeroPlane`, off at first). The offset itself
//!   (`0x1402ff780`) is a ball's: each point takes the highest of `h + sqrt(d² - r²)` over the heights within `d`,
//!   the ball spanning `int(d / cell + 0.5) + 1` cells. Inward, the heights are turned upside down (`× -1`,
//!   `0x1420fbc58`), offset outward and turned back, and places that had nothing keep nothing. The new component
//!   is called "Model With %2.3f Offset" (`0x3f4`).
//! - Slice Model (33237, `0x140742ae0`): refused with nothing in the model (`0x2a0`) or a model no taller than the
//!   material (`0x2a1`). Its form takes a slice thickness, or a number of slices (the model's height divided by it);
//!   there are `int(height / thickness)` slices, one more if over 0.00001 is left. Slice `i` keeps every place at or
//!   above its bottom, as `min(h, top) - bottom`, in a component "Slice i" (`0x2a2`) on a level of that name; with
//!   Create Boundary Vectors (`0x140742200`) each gets the outline where the model reaches its bottom, and where it
//!   reaches its top when it isn't the last, on a layer of the same name. One step to undo, "Slice Model" (`0x2c4`).
//! - Create Component from Toolpath Preview (33247, `0x1402c3830`): needs a carve preview. The carved surface is
//!   moved by where the model sits in the material (`0x140302730`) and made a component, "Toolpath Preview" (`0x2c5`);
//!   places still at the untouched surface where the model has nothing become nothing. Holding Shift makes every
//!   place outside the model nothing; holding Ctrl takes the model away from the preview.
//! - Create Component with Draft from Visible Model (33608, "Add Draft to Model", `0x1402c5d90`): refused when the
//!   model is empty or flat (`0x3a2`); its form (`0x140358860`) has a Draft Angle slider from 0 to 45 degrees, 10 at
//!   first (`DraftAngle`, `0x142736370`). The draft (`0x1402ff480`, applied by `0x140309040`): with `t = tan(angle)`,
//!   angles over 0 and up to 65 (`0x142103340`), each point takes the highest `h - r / t` of the heights within
//!   `t × (highest - lowest) × 1.01` (`0x142103338`), its own included, kept only between the model's lowest and
//!   highest points. The new component is called "Model With %2.1f° Draft" (`0x3a3`).
//! - Turn and Spin (33708, `uiTurnAndSpinForm`, Apply `0x140b287e0`): every selected object must be a vector; the
//!   first is made into lines at a cell's third (`0x142125a58`) and read as a profile: its offset from the line
//!   between its ends, sampled every third of a cell and blended between (`0x140613110`, `0x1406137c0`). Turn
//!   (`DoTurn`, `0x1405a0530`, ends at least two cells apart) rotates it about that line: a point at distance `d`
//!   from it, level with `t` along it, gets `sqrt(r(t)² - d²)` with `r = |profile(t)|` (`0x1405a1450`). Spin
//!   (`0x1405a0950`, the vector turned to start at its left end) spins it about that end: a point `ρ` from it gets
//!   `profile(ρ / length)` (`0x1405a11f0`). Both keep the higher of what's there when combining.
//! - Split Selected Component (33341, "Split the selected component using a vector", `0x1402c4430`): the
//!   selection must be exactly two objects, one component and one vector (message `0x364`); the component must be
//!   visible (`0x318`, checked while baking it, `0x1402b1960`); the vector must cut it into two separate areas that
//!   both hold some of its heights (`0x30f`). It is one step to undo, "Split Component" (`0x310`).

use crate::app::App;
use crate::geom::{Field, Path, P};
use crate::i18n::tr;
use crate::relief::{self, Combine, Component, Grid, HeightMap, Profile};
use egui::Align2;
use std::sync::Arc;

pub const VISIBLE_MODEL_COMMAND: u32 = 33743;
pub const ZERO_PLANE_COMMAND: u32 = 33643;
/// Emboss Component (33689, `0x1402c2730`; its form `0x140733530`, one undo step `0x386`).
pub const EMBOSS_COMMAND: u32 = 33689;
/// The form starts at a scale of 0.1 and no smoothing (`0x140733847`, `+0x548` and `+0x550`); the
/// scale goes from 1% to 50% (`slider / 100 × 0.49 + 0.01`, `0x140734d60`).
pub const EMBOSS_SCALE: f64 = 0.1;
pub const EMBOSS_SCALE_LEAST: f64 = 0.01;
pub const EMBOSS_SCALE_MOST: f64 = 0.5;
/// The smoothness picks between four grids a third apart (`0x1421b9aa8`).
pub const EMBOSS_SMOOTH_STEP: f64 = 1.0 / 3.0;
/// Each smoother grid is the one before averaged over 7 by 7 cells, all weighing the same
/// (`0x140734910`, `0x140999c50`, `0x140999d20`).
pub const EMBOSS_BOX: usize = 7;
/// A surface's slope is taken across 2.001 cells, one less for each missing neighbour
/// (`0x1409df340`, `0x142211814`).
pub const EMBOSS_SPAN: f64 = 2.001;

/// The Emboss form: the height as a share of the model's, and how much the detail is smoothed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmbossForm {
    pub scale: f64,
    pub smoothness: f64,
}

impl Default for EmbossForm {
    fn default() -> Self {
        EmbossForm { scale: EMBOSS_SCALE, smoothness: 0.0 }
    }
}

/// The four grids the original prepares (`0x140734830`): at each place holding something, how
/// upright the surface is there, the up part of its normal (`0x1409e2130`, the dot product with
/// (0, 0, 1)), then that averaged three times over 7 by 7 cells. `None` where there's nothing, and
/// along the grid's edge, where the slope can't be taken.
pub fn emboss_grids(h: &Grid) -> [Vec<Option<f32>>; 4] {
    let (nx, ny, cell) = (h.nx, h.ny, h.cell);
    let held = |ix: usize, iy: usize| h.data[iy * nx + ix] != 0.0;
    let mut g0 = vec![None; nx * ny];
    for iy in 1..ny.saturating_sub(1) {
        for ix in 1..nx.saturating_sub(1) {
            if !held(ix, iy) {
                continue;
            }
            let here = h.data[iy * nx + ix] as f64;
            let side = |jx: usize, jy: usize, span: &mut f64| -> f64 {
                if held(jx, jy) {
                    h.data[jy * nx + jx] as f64
                } else {
                    *span -= cell;
                    here
                }
            };
            let mut sx = cell * EMBOSS_SPAN;
            let (l, r) = (side(ix - 1, iy, &mut sx), side(ix + 1, iy, &mut sx));
            let mut sy = cell * EMBOSS_SPAN;
            let (d, u) = (side(ix, iy - 1, &mut sy), side(ix, iy + 1, &mut sy));
            // The normal is the cross product of (sx, 0, r - l) and (0, sy, u - d).
            let n = [-(r - l) * sy, -sx * (u - d), sx * sy];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 0.0 {
                g0[iy * nx + ix] = Some((n[2] / len) as f32);
            }
        }
    }
    let smooth = |g: &[Option<f32>]| -> Vec<Option<f32>> {
        let reach = (EMBOSS_BOX / 2) as isize;
        let mut out = vec![None; nx * ny];
        for iy in 0..ny {
            for ix in 0..nx {
                if g0[iy * nx + ix].is_none() {
                    continue;
                }
                let (mut sum, mut n) = (0.0f32, 0.0f32);
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        let (jx, jy) = (ix as isize + dx, iy as isize + dy);
                        if jx < 0 || jy < 0 || jx >= nx as isize || jy >= ny as isize {
                            continue;
                        }
                        // Inside the grid, a place with nothing counts as 0.
                        sum += g[jy as usize * nx + jx as usize].unwrap_or(0.0);
                        n += 1.0;
                    }
                }
                out[iy * nx + ix] = Some(sum / n);
            }
        }
        out
    };
    let g1 = smooth(&g0);
    let g2 = smooth(&g1);
    let g3 = smooth(&g2);
    [g0, g1, g2, g3]
}

/// The embossed heights (`0x1407344f0`): the two grids either side of the smoothness, mixed,
/// times the scale and the model's height from its lowest to its highest point.
pub fn emboss_heights(grids: &[Vec<Option<f32>>; 4], like: &Grid, form: EmbossForm, lowest: f64, highest: f64) -> Grid {
    let s = form.smoothness.clamp(0.0, 1.0);
    let k = ((s / EMBOSS_SMOOTH_STEP) as usize).min(2);
    let frac = ((s - k as f64 * EMBOSS_SMOOTH_STEP) / EMBOSS_SMOOTH_STEP).clamp(0.0, 1.0) as f32;
    let scale = form.scale.clamp(EMBOSS_SCALE_LEAST, EMBOSS_SCALE_MOST) as f32;
    let range = (highest - lowest) as f32;
    let data = (0..like.data.len())
        .map(|i| match (grids[k][i], grids[k + 1][i]) {
            (Some(a), Some(b)) => (a + (b - a) * frac) * scale * range,
            _ => 0.0,
        })
        .collect();
    Grid { data, ..like.clone() }
}
/// Create a textured area component (33697, string `0x83a1`; its form `0x140b1c540`).
pub const TEXTURE_AREA_COMMAND: u32 = 33697;
/// The texture's settings are percentages of the tile (`0x1420fbc10`: 100). Spacing may go down to
/// -99.9% (`0x14213ad88`), the tiles then overlapping, and up to 100%; the shift of every other row
/// or column goes from 0 to 100% (`0x1403f6c70`, the top at `0x142736360`). All start at 0.
pub const TEXTURE_SPACING_LEAST: f64 = -99.9;
pub const TEXTURE_MOST: f64 = 100.0;
/// Burin's own limit: how many overlapping tiles one place looks at each way, so a spacing near
/// -99.9% (a thousand tiles over each other) stays quick.
pub const TEXTURE_MOST_OVERLAP: i64 = 16;

/// The Create Texture Area form: spacing and shift each way in percent of the tile, and how each
/// of the four tiles in a group of two by two is mirrored (0 none, 1 left to right, 2 top to
/// bottom, 3 both), from the lower left one, the tile itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureForm {
    pub spacing: [f64; 2],
    pub shift: [f64; 2],
    pub mirror: [u8; 4],
}

impl Default for TextureForm {
    fn default() -> Self {
        TextureForm { spacing: [0.0; 2], shift: [0.0; 2], mirror: [0; 4] }
    }
}

impl TextureForm {
    /// Kept within the original's limits (`0x1403f6c70`).
    pub fn clamped(self) -> TextureForm {
        TextureForm {
            spacing: self.spacing.map(|v| v.clamp(TEXTURE_SPACING_LEAST, TEXTURE_MOST)),
            shift: self.shift.map(|v| v.clamp(0.0, TEXTURE_MOST)),
            mirror: self.mirror.map(|m| m % 4),
        }
    }
}

/// A tile repeated over a grid like `tile`, inside `mask` (one flag per cell). The tile is the
/// part of `tile` holding something, cut to its box first (`0x1403f0b10`). Rows go up from `lo`,
/// each tile `1 + spacing` of its size from the last; every other row moves along by the X shift,
/// and every other column up by the Y shift. Where tiles overlap the highest wins.
pub fn texture_grid(tile: &Grid, form: TextureForm, lo: P, mask: &[bool]) -> Option<Grid> {
    let f = form.clamped();
    let (nx, ny, cell) = (tile.nx, tile.ny, tile.cell);
    let held = |ix: usize, iy: usize| tile.data[iy * nx + ix] != 0.0;
    let cols: Vec<usize> = (0..nx).filter(|&ix| (0..ny).any(|iy| held(ix, iy))).collect();
    let rows: Vec<usize> = (0..ny).filter(|&iy| (0..nx).any(|ix| held(ix, iy))).collect();
    let (&x0, &x1, &y0, &y1) = (cols.first()?, cols.last()?, rows.first()?, rows.last()?);
    let (tw, th) = ((x1 - x0 + 1) as f64 * cell, (y1 - y0 + 1) as f64 * cell);
    let (sx, sy) = (tw * (1.0 + f.spacing[0] / 100.0), th * (1.0 + f.spacing[1] / 100.0));
    let (shx, shy) = (tw * f.shift[0] / 100.0, th * f.shift[1] / 100.0);
    // The tile's own height at a place inside it, mirrored as asked.
    let at = |u: f64, v: f64, mirror: u8| -> f32 {
        let u = if mirror & 1 == 1 { tw - u } else { u };
        let v = if mirror & 2 == 2 { th - v } else { v };
        let ix = x0 + ((u / cell).floor().max(0.0) as usize).min(x1 - x0);
        let iy = y0 + ((v / cell).floor().max(0.0) as usize).min(y1 - y0);
        tile.data[iy * nx + ix]
    };
    let mut out = Grid { nx, ny, cell, data: vec![0.0; nx * ny] };
    for iy in 0..ny {
        for ix in 0..nx {
            if !mask[iy * nx + ix] {
                continue;
            }
            let (x, y) = (ix as f64 * cell - lo[0], iy as f64 * cell - lo[1]);
            let mut best: Option<f32> = None;
            // Rows that may reach this place; the X shift moves the odd ones along.
            let r_top = (y / sy).floor() as i64;
            let reach_r = ((th / sy).ceil() as i64).clamp(1, TEXTURE_MOST_OVERLAP);
            for r in (r_top - reach_r..=r_top + 1).rev() {
                let row_x = if r.rem_euclid(2) == 1 { shx } else { 0.0 };
                let c_top = ((x - row_x) / sx).floor() as i64;
                let reach_c = ((tw / sx).ceil() as i64).clamp(1, TEXTURE_MOST_OVERLAP);
                for c in (c_top - reach_c..=c_top + 1).rev() {
                    let col_y = if c.rem_euclid(2) == 1 { shy } else { 0.0 };
                    let (u, v) = (x - row_x - c as f64 * sx, y - col_y - r as f64 * sy);
                    if !(0.0..tw).contains(&u) || !(0.0..th).contains(&v) {
                        continue;
                    }
                    let k = (c.rem_euclid(2) + 2 * r.rem_euclid(2)) as usize;
                    let h = at(u, v, f.mirror[k]);
                    if h != 0.0 {
                        best = Some(best.map_or(h, |b| b.max(h)));
                    }
                }
            }
            out.data[iy * nx + ix] = best.unwrap_or(0.0);
        }
    }
    Some(out)
}

/// Export Component as a 3d Clipart File (33605, string `0x8345`; the files' filter, `0x36c`).
pub const EXPORT_CLIP3D_COMMAND: u32 = 33605;
/// Burin's own 3D clipart file: one component as it is, in Burin's text form (not Vectric's).
pub const CLIP3D_EXTENSION: &str = "3dclip";
pub const CLIP3D_VERSION: u32 = 2;

/// Version 1 held one component; version 2 holds the whole model, as the original's does.
#[derive(serde::Serialize, serde::Deserialize)]
struct Clip3d {
    burin_3dclip: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    component: Option<Component>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    components: Vec<Component>,
}

/// Where a set of components lies, from all their outlines.
fn components_bbox(cs: &[Component]) -> Option<(P, P)> {
    let all: Vec<Path> = cs.iter().flat_map(|c| c.paths.iter().cloned()).collect();
    crate::geom::bbox(&all)
}

/// Moves a component by `d`: its outline and, if it has one, its height grid.
pub fn shift_component(c: &mut Component, d: [f64; 2]) {
    for q in c.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += d[0];
        q[1] += d[1];
    }
    if let Some(s) = c.surface.as_mut() {
        s.at = [s.at[0] + d[0], s.at[1] + d[1]];
    }
}

/// The text of a 3D clipart file for the model's components, in their order, kept with the lower
/// left corner of them all at 0, 0.
pub fn clip3d_text(cs: &[Component]) -> String {
    let mut cs = cs.to_vec();
    if let Some((lo, _)) = components_bbox(&cs) {
        for c in &mut cs {
            shift_component(c, [-lo[0], -lo[1]]);
        }
    }
    serde_json::to_string(&Clip3d { burin_3dclip: CLIP3D_VERSION, component: None, components: cs }).unwrap_or_default()
}

/// The components in a 3D clipart file, or why it can't be read.
pub fn read_clip3d(text: &str) -> Result<Vec<Component>, String> {
    match serde_json::from_str::<Clip3d>(text) {
        Ok(f) if f.burin_3dclip > CLIP3D_VERSION => Err(tr("This 3D clipart was made by a newer Burin.").into()),
        Ok(f) => {
            let cs: Vec<Component> = f.component.into_iter().chain(f.components).collect();
            if cs.is_empty() {
                Err(tr("This 3D clipart file has nothing in it.").into())
            } else {
                Ok(cs)
            }
        }
        Err(_) => Err(tr("This is not a Burin 3D clipart file.").into()),
    }
}
pub const BOUNDARY_COMMAND: u32 = 33355;
pub const RESET_INSIDE_COMMAND: u32 = 33196;
pub const RESET_OUTSIDE_COMMAND: u32 = 33197;
pub const SCALE_Z_COMMAND: u32 = 33228;
pub const SMOOTH_COMMAND: u32 = 33227;
pub const BAKE_COMMAND: u32 = 33332;
pub const SPLIT_COMMAND: u32 = 33341;
pub const OFFSET_MODEL_COMMAND: u32 = 33692;
pub const SLICE_COMMAND: u32 = 33237;
pub const PREVIEW_COMPONENT_COMMAND: u32 = 33247;
pub const DRAFT_COMMAND: u32 = 33608;
pub const TURN_SPIN_COMMAND: u32 = 33708;
/// The profile is sampled every cell divided by this (`0x142125a58`).
pub const TURN_SAMPLES_PER_CELL: f64 = 3.0;

/// The Turn and Spin window's choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnForm {
    pub turn: bool,
    pub scale_to_height: bool,
    pub height: f64,
}

/// A vector read as a profile along the line between its ends: its offset from that line every `step`.
fn profile_of(path: &Path, start: P, end: P, step: f64) -> (Vec<f64>, f64) {
    let length = (end[0] - start[0]).hypot(end[1] - start[1]);
    let (ax, ay) = ((end[0] - start[0]) / length.max(1e-12), (end[1] - start[1]) / length.max(1e-12));
    let local = |p: P| ((p[0] - start[0]) * ax + (p[1] - start[1]) * ay, (p[1] - start[1]) * ax - (p[0] - start[0]) * ay);
    let pts: Vec<(f64, f64)> = path.pts.iter().map(|p| local(*p)).collect();
    let n = ((length / step).ceil() as usize).max(1);
    let table = (0..=n)
        .map(|i| {
            let x = length * i as f64 / n as f64;
            let mut best: Option<f64> = None;
            for w in pts.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if (x0 - x) * (x1 - x) <= 0.0 && (x1 - x0).abs() > 1e-12 {
                    let y = y0 + (y1 - y0) * (x - x0) / (x1 - x0);
                    if best.is_none_or(|b| y.abs() > b.abs()) {
                        best = Some(y);
                    }
                }
            }
            best.unwrap_or(0.0)
        })
        .collect();
    (table, length)
}

/// The profile at a share of its length, blended between samples (`0x1406137c0`).
fn profile_at(table: &[f64], t: f64) -> f64 {
    let n = table.len() - 1;
    let f = t.clamp(0.0, 1.0) * n as f64;
    let i = (f as usize).min(n.saturating_sub(1));
    let k = f - i as f64;
    table[i] + (table[(i + 1).min(n)] - table[i]) * k
}

/// The shape made by turning a vector about the line between its ends, or spinning it about its left end.
pub fn turned_grid(path: &Path, turn: bool, width: f64, height: f64, points: usize) -> Result<Grid, String> {
    let mut g = Grid::new(width, height, points, 0.0);
    let (Some(&first), Some(&last)) = (path.pts.first(), path.pts.last()) else { return Err(tr("Select a vector to turn or spin.").into()) };
    let (mut start, mut end) = (first, last);
    let mut route = path.clone();
    if !turn && start[0] > end[0] {
        std::mem::swap(&mut start, &mut end);
        route.pts.reverse();
    }
    let length = (end[0] - start[0]).hypot(end[1] - start[1]);
    if length <= 2.0 * g.cell {
        return Err(tr("The vector's ends are too close together to turn or spin it.").into());
    }
    let (table, length) = profile_of(&route, start, end, g.cell / TURN_SAMPLES_PER_CELL);
    let (ax, ay) = ((end[0] - start[0]) / length, (end[1] - start[1]) / length);
    for iy in 0..g.ny {
        for ix in 0..g.nx {
            let p = [ix as f64 * g.cell, iy as f64 * g.cell];
            let (dx, dy) = (p[0] - start[0], p[1] - start[1]);
            let h = if turn {
                let along = dx * ax + dy * ay;
                if !(0.0..=length).contains(&along) {
                    continue;
                }
                let away = dy * ax - dx * ay;
                let r = profile_at(&table, along / length).abs();
                if r * r > away * away { (r * r - away * away).sqrt() } else { continue }
            } else {
                let rho = dx.hypot(dy);
                if rho > length {
                    continue;
                }
                profile_at(&table, rho / length)
            };
            if h > 0.0 {
                g.data[iy * g.nx + ix] = h as f32;
            }
        }
    }
    Ok(g)
}
/// The draft angle the form starts at, the slider's largest, and the largest worked out.
pub const DRAFT_ANGLE: f64 = 10.0;
pub const DRAFT_SLIDER_MOST: f64 = 45.0;
pub const DRAFT_MOST: f64 = 65.0;
/// How far past the model's height the draft reaches.
pub const DRAFT_REACH: f64 = 1.01;

/// The model with draft added: walls sloped `angle` degrees from upright (`0x1402ff480`).
pub fn draft_grid(grid: &Grid, angle: f64) -> Grid {
    let (lo, hi) = grid.data.iter().filter(|h| **h > 0.0).fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
    if angle <= 0.0 || angle > DRAFT_MOST || hi <= lo {
        return grid.clone();
    }
    let t = angle.to_radians().tan();
    let reach = t * (hi - lo) as f64 * DRAFT_REACH;
    let (nx, ny, cell) = (grid.nx, grid.ny, grid.cell);
    let span = (reach / cell + 0.5) as i32 + 1;
    let mut cone = Vec::new();
    for dy in -span..=span {
        for dx in -span..=span {
            let r = (dx as f64).hypot(dy as f64) * cell;
            if r <= reach {
                cone.push((dx, dy, (r / t) as f32));
            }
        }
    }
    let mut out = grid.clone();
    let threads = crate::platform::threads(false);
    let rows_per = ny.div_ceil(threads).max(1);
    crate::platform::scope(|s| {
        for (ci, chunk) in out.data.chunks_mut(nx * rows_per).enumerate() {
            let cone = &cone;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(nx).enumerate() {
                    let y = (ci * rows_per + r) as i32;
                    for (x, v) in row.iter_mut().enumerate() {
                        let mut best = f32::MIN;
                        for (dx, dy, drop) in cone {
                            let (px, py) = (x as i32 + dx, y + dy);
                            if px < 0 || py < 0 || px as usize >= nx || py as usize >= ny {
                                continue;
                            }
                            let h = grid.data[py as usize * nx + px as usize];
                            if h > 0.0 {
                                best = best.max(h - drop);
                            }
                        }
                        if best >= lo && best <= hi {
                            *v = best;
                        }
                    }
                }
            });
        }
    });
    out
}
/// What's left over must be more than this for one more slice (`0x142101020`).
pub const SLICE_LEFT_OVER: f64 = 1e-5;

/// The Slice Model window's choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceForm {
    pub by_count: bool,
    pub thickness: f64,
    pub count: usize,
    pub boundaries: bool,
}

/// Where each slice starts and ends, from the model's lowest and highest points.
pub fn slice_bands(low: f64, high: f64, thickness: f64) -> Vec<(f64, f64)> {
    if thickness <= 0.0 || high <= low {
        return Vec::new();
    }
    let mut n = ((high - low) / thickness) as usize;
    if (high - low) - n as f64 * thickness > SLICE_LEFT_OVER {
        n += 1;
    }
    (0..n).map(|i| (low + i as f64 * thickness, low + (i + 1) as f64 * thickness)).collect()
}

/// One slice of a height grid: every place at or above `bottom`, as `min(h, top) - bottom`.
pub fn slice_of(grid: &Grid, bottom: f64, top: f64) -> Grid {
    let (b, t) = (bottom as f32, top as f32);
    Grid { data: grid.data.iter().map(|h| if *h > 0.0 && *h >= b { h.min(t) - b } else { 0.0 }).collect(), ..grid.clone() }
}
/// The Offset Model form's first distance, in inches and in millimetres.
pub const OFFSET_DISTANCE_INCH: f64 = 0.125;
pub const OFFSET_DISTANCE_MM: f64 = 2.5;
/// The longest offset, as a share of the model's smaller side.
pub const OFFSET_MOST_SHARE: f64 = 0.1;
/// Burin works the offset out on a grid where the ball spans at most this many cells, so a large offset
/// doesn't take minutes. **Chosen for Burin.**
pub const OFFSET_MOST_CELLS: f64 = 32.0;

/// The Offset Model window's choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetForm {
    pub distance: f64,
}

/// A height grid offset by `d` the way a ball of that radius would see it: outward (`d > 0`) every place gets the
/// highest `h + sqrt(d² - r²)` of the heights within `d`, and grows by `d` round its edge; inward (`d < 0`) every
/// place with heights gets the lowest `h - sqrt(d² - r²)`, nothing counting as 0, and what goes down to 0 or under
/// becomes nothing (`0x1402ff780`).
pub fn offset_grid(grid: &Grid, d: f64) -> Grid {
    let reach = d.abs();
    let (nx, ny, cell) = (grid.nx, grid.ny, grid.cell);
    let span = (reach / cell + 0.5) as i32 + 1;
    let mut ball = Vec::new();
    for dy in -span..=span {
        for dx in -span..=span {
            let r = (dx as f64).hypot(dy as f64) * cell;
            if r <= reach {
                ball.push((dx, dy, (reach * reach - r * r).max(0.0).sqrt() as f32));
            }
        }
    }
    let outward = d > 0.0;
    let mut out = grid.clone();
    let threads = crate::platform::threads(false);
    let rows_per = ny.div_ceil(threads).max(1);
    crate::platform::scope(|s| {
        for (ci, chunk) in out.data.chunks_mut(nx * rows_per).enumerate() {
            let ball = &ball;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(nx).enumerate() {
                    let y = (ci * rows_per + r) as i32;
                    for (x, v) in row.iter_mut().enumerate() {
                        let here = grid.data[y as usize * nx + x];
                        if !outward && here <= 0.0 {
                            continue;
                        }
                        let mut best = if outward { 0.0f32 } else { f32::MAX };
                        for (dx, dy, lift) in ball {
                            let (px, py) = (x as i32 + dx, y + dy);
                            if px < 0 || py < 0 || px as usize >= nx || py as usize >= ny {
                                continue;
                            }
                            let h = grid.data[py as usize * nx + px as usize];
                            if outward {
                                if h > 0.0 {
                                    best = best.max(h + lift);
                                }
                            } else {
                                best = best.min(h - lift);
                            }
                        }
                        *v = best.max(0.0);
                    }
                }
            });
        }
    });
    out
}

/// How much a smoothed component takes from its softened self, at first: the original's
/// `SmoothingBlendFactor` starts at 0.5 (`0x140743cef`) and shows as a percentage (`0x1407436ad`).
pub const SMOOTH_BLEND: f64 = 0.5;
/// How far the softening reaches, in model cells. **Estimated:** the original's own softening wasn't read.
pub const SMOOTH_REACH: usize = 3;

/// The Smooth window's choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothForm {
    pub blend: f64,
    /// Leaves places with nothing as they are, rather than letting the edges spread into them
    /// (`PreserveTransparency`, `0x140744ec0`).
    pub keep_empty: bool,
}

impl Default for SmoothForm {
    fn default() -> Self {
        SmoothForm { blend: SMOOTH_BLEND, keep_empty: true }
    }
}

/// Softens a height grid: each cell mixed with the average round it, by `blend` from 0 to 1.
pub fn smoothed(grid: &Grid, blend: f64, keep_empty: bool) -> Grid {
    let (nx, ny, r) = (grid.nx, grid.ny, SMOOTH_REACH as isize);
    let mut out = grid.clone();
    let blend = blend.clamp(0.0, 1.0) as f32;
    for iy in 0..ny {
        for ix in 0..nx {
            let here = grid.data[iy * nx + ix];
            if keep_empty && here <= 0.0 {
                continue;
            }
            let (mut sum, mut n) = (0.0f32, 0.0f32);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (ix as isize + dx, iy as isize + dy);
                    if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny {
                        sum += grid.data[y as usize * nx + x as usize];
                        n += 1.0;
                    }
                }
            }
            out.data[iy * nx + ix] = here * (1.0 - blend) + sum / n * blend;
        }
    }
    out
}

/// A height grid as a component's own heights: every cell kept, nothing where it is 0.
pub fn surface_from(grid: &Grid) -> Option<(HeightMap, f64)> {
    let top = grid.data.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return None;
    }
    let data = grid.data.iter().map(|&h| if h > 0.0 { 1 + ((h / top).clamp(0.0, 1.0) * 65534.0).round() as u16 } else { 0 }).collect();
    // The grid's points are cell corners; a height map's values sit in the middle of its cells.
    let half = grid.cell / 2.0;
    Some((HeightMap { w: grid.nx, h: grid.ny, data: Arc::new(data), at: [-half, -half], width: grid.nx as f64 * grid.cell }, top as f64))
}

/// The outlines of where a height grid stands above nothing, as closed shapes.
pub fn outline_of(grid: &Grid) -> Vec<Path> {
    // A ring of nothing round the edge, so every outline closes.
    let (nx, ny) = (grid.nx + 2, grid.ny + 2);
    let mut d = vec![-1.0f32; nx * ny];
    for iy in 0..grid.ny {
        for ix in 0..grid.nx {
            if grid.data[iy * grid.nx + ix] > 0.0 {
                d[(iy + 1) * nx + ix + 1] = 1.0;
            }
        }
    }
    let field = Field { x0: -grid.cell, y0: -grid.cell, cell: grid.cell, nx, ny, d };
    field.contours(0.0).into_iter().filter(|l| l.len() >= 3).map(Path::closed).collect()
}

/// Which part of a component's grid each cell goes to when a vector splits it: inside and outside a closed
/// vector, or the two sides of an open one, which counts as a wall. `None` unless both parts hold heights.
pub fn split_parts(grid: &Grid, vector: &[Path]) -> Option<Vec<Option<bool>>> {
    let (nx, ny) = (grid.nx, grid.ny);
    let full = |k: usize| grid.data[k] > 0.0;
    let parts: Vec<Option<bool>> = if vector.iter().all(|p| p.closed) {
        let inside = relief::raster_mask(vector, nx, ny, grid.cell);
        (0..nx * ny).map(|k| full(k).then_some(inside[k])).collect()
    } else {
        // The vector's cells, close enough together that nothing gets past on a side.
        let mut wall = vec![false; nx * ny];
        for (a, b) in vector.iter().flat_map(|p| p.segments()) {
            let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / (grid.cell / 4.0)).ceil() as usize).max(1);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let (x, y) = ((a[0] + (b[0] - a[0]) * t) / grid.cell, (a[1] + (b[1] - a[1]) * t) / grid.cell);
                let (ix, iy) = (x.round(), y.round());
                if ix >= 0.0 && iy >= 0.0 && (ix as usize) < nx && (iy as usize) < ny {
                    wall[iy as usize * nx + ix as usize] = true;
                }
            }
        }
        // Areas of heights the wall keeps apart, side by side only.
        let mut label = vec![usize::MAX; nx * ny];
        let mut areas = 0;
        for start in 0..nx * ny {
            if !full(start) || wall[start] || label[start] != usize::MAX {
                continue;
            }
            let mut todo = vec![start];
            label[start] = areas;
            while let Some(k) = todo.pop() {
                let (x, y) = (k % nx, k / nx);
                let near = [(x > 0).then(|| k - 1), (x + 1 < nx).then(|| k + 1), (y > 0).then(|| k - nx), (y + 1 < ny).then(|| k + nx)];
                for n in near.into_iter().flatten() {
                    if full(n) && !wall[n] && label[n] == usize::MAX {
                        label[n] = areas;
                        todo.push(n);
                    }
                }
            }
            areas += 1;
        }
        if areas != 2 {
            return None;
        }
        // The wall's own cells go with an area beside them.
        loop {
            let mut changed = false;
            for k in 0..nx * ny {
                if full(k) && label[k] == usize::MAX {
                    let (x, y) = (k % nx, k / nx);
                    let near = [(x > 0).then(|| k - 1), (x + 1 < nx).then(|| k + 1), (y > 0).then(|| k - nx), (y + 1 < ny).then(|| k + nx)];
                    if let Some(l) = near.into_iter().flatten().map(|n| label[n]).find(|l| *l != usize::MAX) {
                        label[k] = l;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        label.iter().map(|l| (*l != usize::MAX).then_some(*l == 0)).collect()
    };
    let has = |side: bool| parts.iter().any(|p| *p == Some(side));
    (has(true) && has(false)).then_some(parts)
}

impl App {
    /// Splits the selected component in two with the one selected vector (`0x1402c4430`).
    pub fn split_component(&mut self) {
        let vectors: Vec<&crate::app::Vector> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).collect();
        let Some(i) = self.selected_comp.filter(|i| *i < self.doc.components.len() && vectors.len() == 1) else {
            self.message = Some(tr("To split a component, select a single component and a single vector to split with. To split several components, bake them into one first.").into());
            return;
        };
        if !self.doc.components[i].visible {
            self.message = Some(tr("The selected component is not visible. To work with a selected component it must be visible.").into());
            return;
        }
        let paths = vectors[0].paths.clone();
        let grid = self.component_grid(i);
        let Some(parts) = split_parts(&grid, &paths) else {
            self.message = Some(tr("The selected vector did not make two components. It must split the component into two separate areas, both holding some of its heights.").into());
            return;
        };
        let half = |side: bool| Grid { data: grid.data.iter().zip(&parts).map(|(h, p)| if *p == Some(side) { *h } else { 0.0 }).collect(), ..grid.clone() };
        let (first, second) = (half(true), half(false));
        self.checkpoint();
        let mut other = self.doc.components[i].clone();
        other.name = tf!("{} (2)", other.name);
        self.replace_heights(i, &first);
        if let Some((surface, top)) = surface_from(&second) {
            other.paths = outline_of(&second);
            other.surface = Some(surface);
            other.height = top;
            other.base = 0.0;
            other.profile = Profile::Flat;
            self.doc.components.insert(i + 1, other);
        }
        self.selected_comp = Some(i);
        self.status = tr("Split the component in two along the vector.").into();
    }

    pub(crate) fn component_grid(&self, i: usize) -> Grid {
        let j = &self.doc.job;
        relief::component_height(&self.doc.components[i], j.width, j.height, j.points())
    }

    pub fn component_from_visible_model(&mut self) {
        self.refresh_model();
        let Some(model) = self.model.clone() else {
            self.message = Some(tr("There must be one or more components visible to be able to create a copy.").into());
            return;
        };
        let (lo, hi) = model.data.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        let Some((surface, top)) = surface_from(&model).filter(|_| lo < hi) else {
            self.message = Some(tr("There must be one or more components visible to be able to create a copy.").into());
            return;
        };
        self.checkpoint();
        let paths = outline_of(&model);
        self.doc.components.push(Component { name: tr("Copy of Visible Model").into(), paths, profile: Profile::Flat, height: top, base: 0.0, combine: Combine::Add, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.model_dirty = true;
        self.dirty = true;
        self.status = tr("Made a component from the visible model; the components it came from are still there.").into();
    }

    pub fn boundary_around_component(&mut self) {
        let Some(i) = self.selected_comp.filter(|i| *i < self.doc.components.len()) else {
            self.message = Some(tr("There are no selected components. Select the component to outline first.").into());
            return;
        };
        let paths = outline_of(&self.component_grid(i));
        if paths.is_empty() {
            self.status = tr("This component has nothing to outline.").into();
            return;
        }
        self.checkpoint();
        let id = self.doc.add_vector(paths, crate::app::Prim::Free);
        self.selected = vec![id];
        self.dirty = true;
        self.status = tr("Added the outline of the component.").into();
    }

    /// Empties the selected component inside, or outside, the selected closed shapes.
    pub fn reset_component(&mut self, inside: bool) {
        let shapes: Vec<Path> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).flat_map(|v| v.paths.iter().cloned()).filter(|p| p.closed).collect();
        let Some(i) = self.selected_comp.filter(|i| *i < self.doc.components.len() && !shapes.is_empty()) else {
            self.message = Some(tr("Select a component and at least one closed shape to trim it with.").into());
            return;
        };
        let mut grid = self.component_grid(i);
        let mask = relief::raster_mask(&shapes, grid.nx, grid.ny, grid.cell);
        for (h, m) in grid.data.iter_mut().zip(mask) {
            if m == inside {
                *h = 0.0;
            }
        }
        self.checkpoint();
        self.replace_heights(i, &grid);
    }

    /// Puts a component's new heights back, as a grid of its own.
    pub(crate) fn replace_heights(&mut self, i: usize, grid: &Grid) {
        let c = &mut self.doc.components[i];
        match surface_from(grid) {
            Some((surface, top)) => {
                c.surface = Some(surface);
                c.height = top;
                c.base = 0.0;
                c.profile = Profile::Flat;
                c.paths = outline_of(grid);
            }
            None => {
                self.doc.components.remove(i);
                self.selected_comp = None;
            }
        }
        self.model_dirty = true;
        self.dirty = true;
    }

    /// Smooths the selected component (`0x1402c2300`: refused with no component visible, message `0x287`,
    /// or none selected, `0x286`).
    pub fn smooth_component(&mut self, form: SmoothForm) -> bool {
        if !self.doc.components.iter().any(|c| c.visible) {
            self.message = Some(tr("There must be one or more components visible to be able to smooth them.").into());
            return false;
        }
        let Some(i) = self.selected_comp.filter(|i| *i < self.doc.components.len()) else {
            self.message = Some(tr("To smooth a component, select it first.").into());
            return false;
        };
        let grid = smoothed(&self.component_grid(i), form.blend, form.keep_empty);
        self.checkpoint();
        self.replace_heights(i, &grid);
        true
    }

    /// Bakes the selected component: its shape, base and height become heights of its own, which
    /// later changes then work on (`0x1402b36b0`; this version bakes one component, message `0x43d`).
    pub fn bake_component(&mut self) {
        let Some(i) = self.selected_comp.filter(|i| *i < self.doc.components.len()) else {
            self.message = Some(tr("Select the component to bake first.").into());
            return;
        };
        let grid = self.component_grid(i);
        self.checkpoint();
        self.replace_heights(i, &grid);
        self.status = tr("The component's shape and heights are baked into it.").into();
    }

    pub(crate) fn smooth_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.smooth_form else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        egui::Window::new(tr("Smooth Components")).id(egui::Id::unique("smooth")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Smoothing:"));
                let mut percent = form.blend * 100.0;
                if crate::ui::number(ui, &mut percent, 50.0) {
                    form.blend = (percent / 100.0).clamp(0.0, 1.0);
                }
                ui.label("%");
            });
            ui.checkbox(&mut form.keep_empty, tr("Keep empty places empty"));
            ui.horizontal(|ui| {
                apply = ui.button(tr("OK")).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        self.smooth_form = if apply || cancel || !open { None } else { Some(form) };
        if apply {
            self.smooth_component(form);
        }
    }

    /// Add Zero Plane (33643, `0x1402b8770`): a flat component at height 0 over the whole job,
    /// called "Zero Plane", combining by Add, so dishes subtracted after it go below zero.
    /// It goes first in the combine order, under everything, which is where it does its work.
    pub fn add_zero_plane(&mut self) {
        self.checkpoint();
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let paths = vec![Path::closed(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]])];
        let plane = Component { name: tr("Zero Plane").into(), paths, profile: Profile::Flat, height: 0.0, base: 0.0, combine: Combine::Add, visible: true, surface: None, whole: true };
        self.doc.components.insert(0, plane);
        self.selected_comp = Some(0);
        self.model_dirty = true;
        self.sim = None;
        self.status = tr("Added a zero plane under the model: shapes subtracted from it now go below zero.").into();
    }

    /// Create Texture Area: needs one component, the tile; closed shapes chosen as well bound it,
    /// or else it fills the job (`0x140b1c540`).
    pub fn open_texture_area(&mut self) {
        if self.selected_comp.is_none_or(|i| i >= self.doc.components.len()) {
            self.message = Some(tr("Choose the component to repeat in the Modeling tab, and closed shapes to bound it if you like.").into());
            return;
        }
        self.texture_form = Some(self.last_texture.unwrap_or_default());
    }

    /// Makes the textured area: a new component of the tile repeated, inside the chosen closed
    /// shapes or over the whole job. The tile is hidden then, as in the original.
    pub fn texture_area(&mut self, form: TextureForm) -> bool {
        let Some(i) = self.selected_comp.filter(|&i| i < self.doc.components.len()) else { return false };
        let j = self.doc.job.clone();
        let tile = self.component_grid(i);
        let bounds: Vec<Path> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).flat_map(|v| v.paths.iter().filter(|p| p.closed).cloned()).collect();
        let (bounds, lo) = match crate::geom::bbox(&bounds) {
            Some((lo, _)) if !bounds.is_empty() => (bounds, lo),
            _ => (vec![Path::closed(vec![[0.0, 0.0], [j.width, 0.0], [j.width, j.height], [0.0, j.height]])], [0.0, 0.0]),
        };
        let mask = relief::raster_mask(&bounds, tile.nx, tile.ny, tile.cell);
        let Some(grid) = texture_grid(&tile, form, lo, &mask) else {
            self.message = Some(tr("The chosen component has nothing in it to repeat.").into());
            return false;
        };
        let Some((surface, top)) = surface_from(&grid) else {
            self.message = Some(tr("After fitting the texture inside its boundary, no areas remained to texture.").into());
            return false;
        };
        self.checkpoint();
        let combine = self.doc.components[i].combine;
        self.doc.components[i].visible = false;
        let n = self.doc.components.iter().filter(|c| c.name.starts_with(tr("Texture Area"))).count() + 1;
        let name = tf!("Texture Area {n}");
        self.doc.components.push(Component { name, paths: bounds, profile: Profile::Flat, height: top, base: 0.0, combine, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.last_texture = Some(form.clamped());
        self.model_dirty = true;
        self.sim = None;
        self.dirty = true;
        true
    }

    pub(crate) fn texture_area_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.texture_form else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        egui::Window::new(tr("Create Texture Area")).id(egui::Id::unique("texture-area")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(egui::RichText::new(tr("The chosen component is repeated inside the chosen closed shapes, or over the whole job. Values are percent of its size.")).size(12.0).color(crate::theme::MUTED()));
            for (label, values, least) in [(tr("Spacing"), &mut form.spacing, TEXTURE_SPACING_LEAST), (tr("Shift every other row or column"), &mut form.shift, 0.0)] {
                ui.label(egui::RichText::new(label).strong());
                for (k, axis) in ["X", "Y"].into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(axis);
                        ui.add(egui::Slider::new(&mut values[k], least..=TEXTURE_MOST).suffix(" %").fixed_decimals(1));
                    });
                }
            }
            ui.label(egui::RichText::new(tr("Mirror")).strong());
            // Two by two, as the tiles lie: the top pair first on screen, the tile itself bottom left.
            let names = [tr("None"), tr("Left to right"), tr("Top to bottom"), tr("Both")];
            for row in [[2usize, 3], [0, 1]] {
                ui.horizontal(|ui| {
                    for k in row {
                        let m = form.mirror[k] as usize;
                        if ui.add(egui::Button::new(names[m]).min_size(egui::vec2(110.0, 22.0))).on_hover_text(tr("Click to change how this tile of each group of four is mirrored")).clicked() {
                            form.mirror[k] = ((m + 1) % 4) as u8;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                apply = ui.button(tr("Apply")).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        let done = apply && self.texture_area(form);
        self.texture_form = if done || cancel || !open { None } else { Some(form) };
    }

    /// Emboss Component: the chosen component, flattened to the shape of its slopes.
    pub fn open_emboss(&mut self) {
        if self.selected_comp.is_none_or(|i| i >= self.doc.components.len()) {
            self.message = Some(tr("Choose the component to emboss in the Modeling tab.").into());
            return;
        }
        self.emboss_form = Some(EmbossForm::default());
    }

    pub fn emboss_component(&mut self, form: EmbossForm) -> bool {
        let Some(i) = self.selected_comp.filter(|&i| i < self.doc.components.len()) else { return false };
        let grid = self.component_grid(i);
        let (lo, hi) = grid.data.iter().filter(|h| **h != 0.0).fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        if lo > hi {
            self.message = Some(tr("The chosen component has nothing in it to emboss.").into());
            return false;
        }
        let grids = emboss_grids(&grid);
        let out = emboss_heights(&grids, &grid, form, lo.min(0.0) as f64, hi as f64);
        let Some((surface, top)) = surface_from(&out) else {
            self.message = Some(tr("The chosen component has nothing in it to emboss.").into());
            return false;
        };
        self.checkpoint();
        let c = &mut self.doc.components[i];
        c.surface = Some(surface);
        c.height = top;
        c.profile = Profile::Flat;
        c.base = 0.0;
        self.model_dirty = true;
        self.sim = None;
        self.dirty = true;
        true
    }

    pub(crate) fn emboss_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.emboss_form else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        egui::Window::new(tr("Emboss Component")).id(egui::Id::unique("emboss")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_max_width(320.0);
            // The original's warning (`0x3f8`), shown in the form rather than before it.
            ui.label(egui::RichText::new(tr("Embossing is meant for imported components or 3D models, and works best with high relief.")).size(12.0).color(crate::theme::MUTED()));
            let mut scale = form.scale * 100.0;
            ui.horizontal(|ui| {
                ui.label(tr("Scale Height"));
                ui.add(egui::Slider::new(&mut scale, EMBOSS_SCALE_LEAST * 100.0..=EMBOSS_SCALE_MOST * 100.0).suffix(" %").fixed_decimals(0));
            });
            form.scale = scale / 100.0;
            let mut smooth = form.smoothness * 100.0;
            ui.horizontal(|ui| {
                ui.label(tr("Detail Smoothness"));
                ui.add(egui::Slider::new(&mut smooth, 0.0..=100.0).fixed_decimals(0));
            });
            form.smoothness = smooth / 100.0;
            ui.horizontal(|ui| {
                apply = ui.button(tr("Apply")).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        let done = apply && self.emboss_component(form);
        self.emboss_form = if done || cancel || !open { None } else { Some(form) };
    }

    /// Export as 3D Clipart (`0x1402c2d90`): the whole model, every component in its order, to a
    /// file that File > Import, dropping it on the window or the Clipart tab bring into any job.
    /// The original walks the component tree from its top (`0x1409d5310`), and refuses when the
    /// composite model is empty (`0x1409e0050`, message `0x28e`). The name offered is the job's.
    pub fn export_3d_clipart(&mut self) {
        self.refresh_model();
        let empty = self.model.as_ref().is_none_or(|m| m.data.iter().all(|v| *v == 0.0)) && !self.doc.components.iter().any(|c| c.whole);
        if self.doc.components.is_empty() || empty {
            self.message = Some(tr("There are no visible components. There must be shapes in the model to be able to export it as 3D clipart.").into());
            return;
        }
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| tr("model").to_string(), |s| s.to_string_lossy().to_string());
        let name = format!("{stem}.{CLIP3D_EXTENSION}");
        let Some(path) = crate::platform::FileDialog::new().add_filter(tr("3D Clipart"), &[CLIP3D_EXTENSION]).set_file_name(&name).save_file() else { return };
        match crate::platform::write(&path, clip3d_text(&self.doc.components)) {
            Ok(()) => self.status = tf!("Exported {}", path.display()),
            Err(e) => self.message = Some(tr("Failed to write clipart data").to_string() + &format!(" ({}: {e})", path.display())),
        }
    }

    /// Adds a piece of 3D clipart to the job: its components, together in the middle of the job,
    /// after the ones already there; the last one is chosen.
    pub fn insert_3d_clipart(&mut self, mut cs: Vec<Component>) {
        if cs.is_empty() {
            return;
        }
        if let Some((lo, hi)) = components_bbox(&cs) {
            let (j, mid) = (&self.doc.job, [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
            let d = [j.width / 2.0 - mid[0], j.height / 2.0 - mid[1]];
            for c in &mut cs {
                shift_component(c, d);
            }
        }
        self.checkpoint();
        self.doc.components.extend(cs);
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.model_dirty = true;
        self.sim = None;
    }

    pub fn import_3d_clipart(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = crate::platform::read(path).map_err(|e| tf!("Could not read {}: {e}", path.display()))?;
        let cs = read_clip3d(&String::from_utf8_lossy(&bytes))?;
        self.insert_3d_clipart(cs);
        Ok(())
    }

    /// The highest point of the model now.
    pub fn model_top(&mut self) -> Option<f64> {
        self.refresh_model();
        self.model.as_ref().map(|m| m.max() as f64).filter(|t| *t > 0.0)
    }

    /// Scales every visible component's height so the model's highest point is `wanted`.
    pub fn scale_model_height(&mut self, wanted: f64) -> bool {
        let Some(top) = self.model_top() else {
            self.message = Some(tr("There must be shapes in the model to be able to scale it.").into());
            return false;
        };
        if wanted <= 0.0 {
            return false;
        }
        let k = wanted / top;
        self.checkpoint();
        for c in self.doc.components.iter_mut().filter(|c| c.visible) {
            c.height *= k;
            c.base *= k;
        }
        self.model_dirty = true;
        self.dirty = true;
        true
    }

    pub(crate) fn scale_z_window(&mut self, ctx: &egui::Context) {
        let Some(mut wanted) = self.scale_z else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        let top = self.model.as_ref().map_or(0.0, |m| m.max() as f64);
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Scale Model Z Height")).id(egui::Id::unique("scale-z")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(tf!("The model's highest point is {} {} now.", crate::toolpath::trim(top), unit));
            ui.horizontal(|ui| {
                ui.label(tr("New height:"));
                crate::ui::number(ui, &mut wanted, 60.0);
                ui.label(unit);
            });
            ui.label(egui::RichText::new(tr("Every visible component is scaled up or down by the same amount.")).size(12.0).color(crate::theme::MUTED()));
            ui.horizontal(|ui| {
                apply = ui.add_enabled(wanted > 0.0, egui::Button::new(tr("OK"))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        self.scale_z = if apply || cancel || !open { None } else { Some(wanted) };
        if apply {
            self.scale_model_height(wanted);
        }
    }

    /// The visible model's lowest and highest points, as a grid at the job's resolution.
    fn model_range(&self) -> Option<(Grid, f64, f64)> {
        let j = &self.doc.job;
        let model = relief::compose(&self.doc.components, j.width, j.height, j.points())?;
        let (lo, hi) = model.data.iter().filter(|h| **h > 0.0).fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
        (hi > 0.0).then(|| (model, lo as f64, hi as f64))
    }

    /// Opens the Turn and Spin form.
    pub fn open_turn_spin(&mut self) {
        let height = self.turn_height.unwrap_or(self.doc.job.thickness / 2.0);
        self.turn_form = Some(TurnForm { turn: self.turn_mode.unwrap_or(true), scale_to_height: false, height });
    }

    /// Makes a component by turning or spinning the first selected vector (`0x140b287e0`).
    pub fn turn_spin(&mut self, form: TurnForm) -> bool {
        let vectors: Vec<&crate::app::Vector> = self.selected.iter().filter_map(|id| self.doc.vectors.iter().find(|v| v.id == *id)).collect();
        let Some(path) = vectors.first().and_then(|v| v.paths.first()).cloned() else {
            self.message = Some(tr("Select a vector to turn or spin.").into());
            return false;
        };
        let j = &self.doc.job;
        let mut grid = match turned_grid(&path, form.turn, j.width, j.height, j.points()) {
            Ok(g) => g,
            Err(why) => {
                self.message = Some(why);
                return false;
            }
        };
        let top = grid.max();
        if form.scale_to_height && top > 0.0 && form.height > 0.0 {
            let k = (form.height / top as f64) as f32;
            grid.data.iter_mut().for_each(|h| *h *= k);
        }
        let Some((surface, height)) = surface_from(&grid) else {
            self.message = Some(tr("The vector makes no shape: draw it on the side above the line between its ends.").into());
            return false;
        };
        self.checkpoint();
        self.doc.components.push(Component { name: tr("Turned Component").into(), paths: outline_of(&grid), profile: Profile::Flat, height, base: 0.0, combine: Combine::Merge, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.turn_mode = Some(form.turn);
        self.turn_height = Some(form.height);
        self.model_dirty = true;
        self.dirty = true;
        true
    }

    pub(crate) fn turn_spin_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.turn_form else { return };
        let mut open = true;
        let (mut apply, mut close) = (false, false);
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Turn and Spin")).id(egui::Id::unique("turn-spin")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.radio_value(&mut form.turn, true, tr("Turn (rotate) around line from start point to end point"));
            ui.radio_value(&mut form.turn, false, tr("Spin around left end point"));
            ui.checkbox(&mut form.scale_to_height, tr("Scale To Exact Height"));
            ui.add_enabled_ui(form.scale_to_height, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Height:"));
                    crate::ui::number(ui, &mut form.height, 60.0);
                    ui.label(unit);
                });
            });
            ui.horizontal(|ui| {
                apply = ui.button(tr("Apply")).clicked();
                close = ui.button(tr("Close")).clicked();
            });
        });
        if apply {
            self.turn_spin(form);
        }
        self.turn_form = if close || !open { None } else { Some(form) };
    }

    /// Opens the Add Draft form, or says why it can't (`0x1402c5d90`).
    pub fn open_draft_model(&mut self) {
        if self.model_top().is_none() {
            self.message = Some(tr("There are no components. There must be one or more components visible to be able to add draft.").into());
            return;
        }
        self.draft_angle = Some(self.last_draft_angle.unwrap_or(DRAFT_ANGLE));
    }

    /// Adds a component that is the visible model with draft (`0x140358cd0`).
    pub fn draft_model(&mut self, angle: f64) -> bool {
        let j = &self.doc.job;
        let Some(model) = relief::compose(&self.doc.components, j.width, j.height, j.points()).filter(|g| g.data.iter().any(|v| *v > 0.0)) else {
            self.message = Some(tr("There are no components. There must be one or more components visible to be able to add draft.").into());
            return false;
        };
        let (lo, hi) = model.data.iter().filter(|h| **h > 0.0).fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
        // A grid on which the cone spans at most 32 cells, so a steep model doesn't take minutes.
        let reach = angle.clamp(0.0, DRAFT_MOST).to_radians().tan() * (hi - lo) as f64 * DRAFT_REACH;
        let points = if reach > 0.0 { j.points().min((j.width * j.height / (reach / OFFSET_MOST_CELLS).powi(2)) as usize).max(10_000) } else { j.points() };
        let model = if points < j.points() { relief::compose(&self.doc.components, j.width, j.height, points).unwrap_or(model) } else { model };
        let drafted = draft_grid(&model, angle);
        let Some((surface, top)) = surface_from(&drafted) else { return false };
        self.checkpoint();
        let name = tf!("Model With {}° Draft", format!("{angle:.1}"));
        self.doc.components.push(Component { name, paths: outline_of(&drafted), profile: Profile::Flat, height: top, base: 0.0, combine: Combine::Merge, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.last_draft_angle = Some(angle);
        self.model_dirty = true;
        self.dirty = true;
        true
    }

    pub(crate) fn draft_model_window(&mut self, ctx: &egui::Context) {
        let Some(mut angle) = self.draft_angle else { return };
        let mut open = true;
        let (mut apply, mut close) = (false, false);
        egui::Window::new(tr("Add Draft to Model")).id(egui::Id::unique("draft-model")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(egui::RichText::new(tr("Adjust the angle and press Apply to create a new component: the visible model with the draft angle added.")).size(12.0).color(crate::theme::MUTED()));
            ui.horizontal(|ui| {
                ui.label(tr("Draft Angle"));
                ui.add(egui::Slider::new(&mut angle, 0.0..=DRAFT_SLIDER_MOST).step_by(1.0).suffix("°"));
            });
            ui.horizontal(|ui| {
                apply = ui.add_enabled(angle > 0.0, egui::Button::new(tr("Apply"))).clicked();
                close = ui.button(tr("Close")).clicked();
            });
        });
        if apply {
            self.draft_model(angle);
        }
        self.draft_angle = if close || !open { None } else { Some(angle) };
    }

    /// Makes a component from the carve preview (`0x1402c3830`).
    pub fn component_from_preview(&mut self) -> bool {
        let Some(sim) = self.sim.as_ref() else {
            self.message = Some(tr("There's no toolpath preview yet. Preview a toolpath first.").into());
            return false;
        };
        let j = &self.doc.job;
        let model = relief::compose(&self.doc.components, j.width, j.height, j.points());
        let highest = model.as_ref().map_or(0.0, |m| m.max() as f64);
        // The model's height 0, as a depth below the material's top.
        let zero = self.doc.machine.model_top(j.thickness, highest) as f32;
        let g = &sim.grid;
        let mut out = g.clone();
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let k = iy * g.nx + ix;
                let surface = g.data[k];
                let empty_model = model.as_ref().is_none_or(|m| m.sample(ix as f64 * g.cell, iy as f64 * g.cell) <= 0.0);
                out.data[k] = if surface >= 0.0 && empty_model { 0.0 } else { (surface + zero).max(0.0) };
            }
        }
        let Some((surface, top)) = surface_from(&out) else {
            self.message = Some(tr("The toolpath preview has nothing above the model's zero to make a component from.").into());
            return false;
        };
        self.checkpoint();
        self.doc.components.push(Component { name: tr("Toolpath Preview").into(), paths: outline_of(&out), profile: Profile::Flat, height: top, base: 0.0, combine: Combine::Add, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.model_dirty = true;
        self.dirty = true;
        self.status = tr("Made a component from the toolpath preview.").into();
        true
    }

    /// Opens the Slice Model form, or says why there's nothing to slice.
    pub fn open_slice_model(&mut self) {
        let Some((_, lo, hi)) = self.model_range() else {
            self.message = Some(tr("The model is empty. There is nothing to slice.").into());
            return;
        };
        let thickness = self.doc.job.thickness;
        if hi - lo <= thickness {
            self.message = Some(tr("The model fits into the material thickness. When the model is no taller than the material, there is no need to slice it.").into());
            return;
        }
        let count = slice_bands(lo, hi, thickness).len().max(1);
        self.slice_form = Some(SliceForm { by_count: false, thickness, count, boundaries: true });
    }

    /// Cuts the visible model into slices, each a component of its own (`0x140742ae0`).
    pub fn slice_model(&mut self, form: SliceForm) -> bool {
        let Some((model, lo, hi)) = self.model_range() else {
            self.message = Some(tr("The model is empty. There is nothing to slice.").into());
            return false;
        };
        let thickness = if form.by_count && form.count > 0 { (hi - lo) / form.count as f64 } else { form.thickness };
        let bands = slice_bands(lo, hi, thickness);
        if bands.is_empty() {
            return false;
        }
        self.checkpoint();
        let last = bands.len() - 1;
        for (i, (bottom, top)) in bands.into_iter().enumerate() {
            let name = tf!("Slice {}", i + 1);
            let slice = slice_of(&model, bottom, top);
            if let Some((surface, height)) = surface_from(&slice) {
                // Burin has no levels: the slices start hidden, so the model looks as it did.
                self.doc.components.push(Component { name: name.clone(), paths: outline_of(&slice), profile: Profile::Flat, height, base: 0.0, combine: Combine::Add, visible: false, surface: Some(surface), whole: false });
            }
            if form.boundaries {
                let above = |z: f64| Grid { data: model.data.iter().map(|h| if *h > 0.0 && *h as f64 >= z { 1.0 } else { 0.0 }).collect(), ..model.clone() };
                let mut paths = outline_of(&above(bottom));
                if i < last {
                    paths.extend(outline_of(&above(top)));
                }
                if !paths.is_empty() {
                    let layer = match self.doc.layers.iter().position(|l| l.name == name) {
                        Some(k) => k,
                        None => {
                            self.doc.layers.push(crate::app::Layer::named(&name));
                            self.doc.layers.len() - 1
                        }
                    };
                    let id = self.doc.new_id();
                    self.doc.vectors.push(crate::app::Vector { id, paths, layer, prim: crate::app::Prim::Free, construction: false, members: Vec::new(), piece: None, face: self.doc.active_face, sheet: self.doc.active_sheet });
                }
            }
        }
        self.model_dirty = true;
        self.dirty = true;
        self.status = tr("Sliced the model; the slices are hidden components, one per slice.").into();
        true
    }

    pub(crate) fn slice_model_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.slice_form else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        let unit = self.doc.job.unit();
        let range = self.model_range().map(|(_, lo, hi)| (lo, hi));
        let height = range.map_or(0.0, |(lo, hi)| hi - lo);
        let material = self.doc.job.thickness;
        egui::Window::new(tr("Slice Model")).id(egui::Id::unique("slice-model")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(tf!("Model Thickness: {} {}", crate::toolpath::trim(height), unit));
            ui.label(tf!("Material Thickness: {} {}", crate::toolpath::trim(material), unit));
            ui.radio_value(&mut form.by_count, false, tr("Slice Thickness"));
            ui.add_enabled_ui(!form.by_count, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    crate::ui::number(ui, &mut form.thickness, 60.0);
                    ui.label(unit);
                });
            });
            ui.radio_value(&mut form.by_count, true, tr("Number of Slices"));
            ui.add_enabled_ui(form.by_count, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.add(egui::DragValue::new(&mut form.count).range(1..=200));
                });
            });
            let thickness = if form.by_count && form.count > 0 { height / form.count as f64 } else { form.thickness };
            let bands = range.map(|(lo, hi)| slice_bands(lo, hi, thickness)).unwrap_or_default();
            if let (Some(first), Some(top)) = (bands.first(), bands.last()) {
                let (_, hi) = range.unwrap_or((0.0, 0.0));
                ui.label(egui::RichText::new(tf!("Number of Slices: {}", bands.len())).color(crate::theme::MUTED()));
                ui.label(egui::RichText::new(tf!("Slice Thickness: {} {}", crate::toolpath::trim(first.1 - first.0), unit)).color(crate::theme::MUTED()));
                ui.label(egui::RichText::new(tf!("Top Slice Thickness: {} {}", crate::toolpath::trim(hi - top.0), unit)).color(crate::theme::MUTED()));
            }
            ui.checkbox(&mut form.boundaries, tr("Create Boundary Vectors"));
            ui.horizontal(|ui| {
                apply = ui.add_enabled(!bands.is_empty(), egui::Button::new(tr("Slice Model"))).clicked();
                cancel = ui.button(tr("Close")).clicked();
            });
        });
        let done = apply && self.slice_model(form);
        self.slice_form = if done || cancel || !open { None } else { Some(form) };
    }

    /// Opens the Offset Model form, or says why it can't (`0x1402c5ec0`).
    pub fn open_offset_model(&mut self) {
        let flat = self.model_top().is_none();
        if flat {
            self.message = Some(tr("There are no components visible. There must be one or more components visible to be able to offset.").into());
            return;
        }
        let distance = if self.doc.job.units == crate::app::Units::Mm { OFFSET_DISTANCE_MM } else { OFFSET_DISTANCE_INCH };
        self.offset_form = Some(OffsetForm { distance: self.offset_distance.unwrap_or(distance) });
    }

    /// Adds a component that is the visible model offset by `d` (`0x140b08f20`).
    pub fn offset_model(&mut self, d: f64) -> bool {
        let j = &self.doc.job;
        let (w, h) = (j.width, j.height);
        let most = w.min(h) * OFFSET_MOST_SHARE;
        if d.abs() > most {
            self.message = Some(tf!("The largest offset is 10% of the model's size ({}).", crate::toolpath::trim(most)));
            return false;
        }
        if d == 0.0 {
            return false;
        }
        let fine = (w * h / (d.abs() / OFFSET_MOST_CELLS).powi(2)) as usize;
        let points = j.points().min(fine).max(10_000);
        let Some(model) = relief::compose(&self.doc.components, w, h, points).filter(|g| g.data.iter().any(|v| *v > 0.0)) else {
            self.message = Some(tr("There are no components visible. There must be one or more components visible to be able to offset.").into());
            return false;
        };
        let moved = offset_grid(&model, d);
        let Some((surface, top)) = surface_from(&moved) else {
            self.message = Some(tr("Nothing of the model is left at this offset.").into());
            return false;
        };
        self.checkpoint();
        let combine = if d > 0.0 { Combine::Merge } else { Combine::Low };
        let name = tf!("Model With {} Offset", format!("{d:.3}"));
        self.doc.components.push(Component { name, paths: outline_of(&moved), profile: Profile::Flat, height: top, base: 0.0, combine, visible: true, surface: Some(surface), whole: false });
        self.selected_comp = Some(self.doc.components.len() - 1);
        self.offset_distance = Some(d);
        self.model_dirty = true;
        self.dirty = true;
        true
    }

    pub(crate) fn offset_model_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.offset_form else { return };
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        let unit = self.doc.job.unit();
        egui::Window::new(tr("Offset Model")).id(egui::Id::unique("offset-model")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(egui::RichText::new(tr("Give the offset distance, then OK makes a new component: the visible model with that offset added. Below 0 it goes inward.")).size(12.0).color(crate::theme::MUTED()));
            ui.horizontal(|ui| {
                ui.label(tr("Offset Distance"));
                crate::ui::number(ui, &mut form.distance, 60.0);
                ui.label(unit);
            });
            let mut clip = true;
            ui.add_enabled(false, egui::Checkbox::new(&mut clip, tr("Clip Z values to Zero Plane"))).on_disabled_hover_text(tr("Burin's offset keeps only what is above zero, so it is always clipped there."));
            ui.horizontal(|ui| {
                apply = ui.add_enabled(form.distance != 0.0, egui::Button::new(tr("OK"))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        let done = apply && self.offset_model(form.distance);
        self.offset_form = if done || cancel || !open { None } else { Some(form) };
    }

    pub fn open_scale_z(&mut self) {
        match self.model_top() {
            Some(top) => self.scale_z = Some(top),
            None => self.message = Some(tr("There must be shapes in the model to be able to scale it.").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Emboss: flat tops come out at the full scaled height, slopes lower, nothing stays nothing,
    /// and smoothing blurs the detail by averaging over 7 by 7 cells.
    #[test]
    fn emboss_follows_the_slopes() {
        let mut g = Grid::new(10.0, 10.0, 10_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
                // A flat-topped plateau 1 high from x = 3 to 7, with ramps on both sides down to 0.2.
                if (2.0..8.0).contains(&x) && (2.0..8.0).contains(&y) {
                    let ramp = ((x - 2.0).min(8.0 - x)).min(1.0);
                    g.data[iy * g.nx + ix] = (0.2 + 0.8 * ramp) as f32;
                }
            }
        }
        let grids = emboss_grids(&g);
        let flat = grids[0][(50.0 / g.cell * 0.1) as usize * g.nx + (5.0 / g.cell) as usize];
        assert!((flat.unwrap() - 1.0).abs() < 1e-5, "flat: upright normal");
        let out = emboss_heights(&grids, &g, EmbossForm::default(), 0.0, 1.0);
        let top = out.sample(5.0, 5.0);
        assert!((top - EMBOSS_SCALE as f32).abs() < 1e-4, "{top}");
        assert!(out.sample(2.5, 5.0) < top, "a slope comes out lower");
        assert_eq!(out.sample(0.5, 0.5), 0.0, "nothing stays nothing");
        // Fully smoothed, the sharp change at the ramp's foot is spread out.
        let soft = emboss_heights(&grids, &g, EmbossForm { scale: EMBOSS_SCALE, smoothness: 1.0 }, 0.0, 1.0);
        let step = |o: &Grid| (o.sample(2.95, 5.0) - o.sample(3.05, 5.0)).abs();
        assert!(step(&soft) < step(&out));
        // Halfway between two grids mixes them evenly.
        let half = emboss_heights(&grids, &g, EmbossForm { scale: 0.5, smoothness: 0.5 }, 0.0, 1.0);
        let i = (5.0 / g.cell) as usize * g.nx + (2.6 / g.cell) as usize;
        let want = (grids[1][i].unwrap() + grids[2][i].unwrap()) / 2.0 * 0.5;
        assert!((half.data[i] - want).abs() < 1e-5);
    }

    /// A tile 1 wide and 2 tall at the job's corner, climbing from left to right, on a 10 by 10 job.
    fn ramp_tile() -> Grid {
        let mut g = Grid::new(10.0, 10.0, 40_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
                if x < 1.0 && y < 2.0 {
                    g.data[iy * g.nx + ix] = (0.1 + x) as f32;
                }
            }
        }
        g
    }

    #[test]
    fn a_texture_repeats_the_tile_with_spacing_shift_and_mirrors() {
        let tile = ramp_tile();
        let all = vec![true; tile.nx * tile.ny];
        let plain = texture_grid(&tile, TextureForm::default(), [0.0, 0.0], &all).unwrap();
        // Touching tiles: the same climb every 1 across and every 2 up.
        assert!((plain.sample(3.5, 5.0) - plain.sample(0.5, 1.0)).abs() < 0.03);
        assert!(plain.sample(3.95, 0.5) > plain.sample(3.05, 0.5) + 0.5, "each tile climbs left to right");
        // 100% spacing leaves a gap as wide as the tile.
        let spaced = texture_grid(&tile, TextureForm { spacing: [100.0, 0.0], ..Default::default() }, [0.0, 0.0], &all).unwrap();
        assert_eq!(spaced.sample(1.5, 1.0), 0.0, "a gap");
        assert!(spaced.sample(2.5, 1.0) > 0.0, "the next tile");
        // A 50% X shift moves every other row along by half the tile.
        let shifted = texture_grid(&tile, TextureForm { shift: [50.0, 0.0], ..Default::default() }, [0.0, 0.0], &all).unwrap();
        assert!((shifted.sample(0.75, 3.0) - plain.sample(0.25, 3.0)).abs() < 0.03, "{} {}", shifted.sample(0.75, 3.0), plain.sample(0.25, 3.0));
        // The lower right tile of each group mirrored left to right climbs the other way.
        let mirrored = texture_grid(&tile, TextureForm { mirror: [0, 1, 0, 0], ..Default::default() }, [0.0, 0.0], &all).unwrap();
        assert!(mirrored.sample(1.05, 0.5) > mirrored.sample(1.95, 0.5) + 0.5);
        assert!(mirrored.sample(2.95, 0.5) > mirrored.sample(2.05, 0.5) + 0.5, "the next group is as the tile");
        // Overlapping tiles keep the highest.
        let packed = texture_grid(&tile, TextureForm { spacing: [-50.0, 0.0], ..Default::default() }, [0.0, 0.0], &all).unwrap();
        assert!(packed.sample(0.9, 0.5) >= plain.sample(0.9, 0.5) - 0.03);
        // Out of range values are kept within the original's limits.
        let f = TextureForm { spacing: [-500.0, 300.0], shift: [-5.0, 140.0], mirror: [4, 5, 6, 7] }.clamped();
        assert_eq!(f.spacing, [TEXTURE_SPACING_LEAST, TEXTURE_MOST]);
        assert_eq!(f.shift, [0.0, TEXTURE_MOST]);
        assert_eq!(f.mirror, [0, 1, 2, 3]);
    }

    #[test]
    fn spinning_and_turning_a_triangle() {
        let tri = Path { pts: vec![[5.0, 5.0], [6.0, 5.5], [7.0, 5.0]], closed: false };
        let spun = turned_grid(&tri, false, 10.0, 10.0, 160_000).unwrap();
        assert!((spun.sample(6.0, 5.0) - 0.5).abs() < 0.01, "1 from the left end: the top");
        assert!((spun.sample(5.0, 6.0) - 0.5).abs() < 0.01, "any way round");
        assert!((spun.sample(5.5, 5.0) - 0.25).abs() < 0.01);
        assert_eq!(spun.sample(7.6, 5.0), 0.0, "nothing past its length");
        // Drawn right to left, it still spins about its left end.
        let back = Path { pts: tri.pts.iter().rev().cloned().collect(), closed: false };
        let again = turned_grid(&back, false, 10.0, 10.0, 160_000).unwrap();
        assert!((again.sample(6.0, 5.0) - spun.sample(6.0, 5.0)).abs() < 0.01 || again.sample(6.0, 5.0) > 0.0);
        let turned = turned_grid(&tri, true, 10.0, 10.0, 160_000).unwrap();
        assert!((turned.sample(6.0, 5.0) - 0.5).abs() < 0.01, "on the line: the radius");
        assert!((turned.sample(6.0, 5.3) - 0.4).abs() < 0.02, "a round cross-section: {}", turned.sample(6.0, 5.3));
        assert_eq!(turned.sample(6.0, 5.6), 0.0);
        assert_eq!(turned.sample(4.5, 5.0), 0.0, "nothing past the ends");
        let short = Path { pts: vec![[5.0, 5.0], [5.01, 5.0]], closed: false };
        assert!(turned_grid(&short, true, 10.0, 10.0, 160_000).is_err());
    }

    #[test]
    fn draft_slopes_the_walls_down_to_the_lowest_point() {
        // A block 0.5 high on a base 0.1 high.
        let mut g = Grid::new(10.0, 10.0, 40_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
                let base = if (1.0..=9.0).contains(&x) && (1.0..=9.0).contains(&y) { 0.1 } else { 0.0 };
                let block = if (4.0..=6.0).contains(&x) && (4.0..=6.0).contains(&y) { 0.5 } else { base };
                g.data[iy * g.nx + ix] = block;
            }
        }
        let d = draft_grid(&g, 45.0);
        assert!((d.sample(5.0, 5.0) - 0.5).abs() < 1e-3, "the top stays");
        // 45 degrees: 0.2 out from the wall it has come down 0.2.
        assert!((d.sample(6.2, 5.0) - 0.3).abs() < 0.03, "{}", d.sample(6.2, 5.0));
        assert!((d.sample(7.0, 5.0) - 0.1).abs() < 1e-3, "down to the base");
        assert_eq!(d.sample(0.5, 0.5), 0.0, "nothing below the lowest point is made");
        assert_eq!(draft_grid(&g, 0.0).data, g.data);
        assert_eq!(draft_grid(&g, 70.0).data, g.data, "over 65 degrees isn't worked out");
    }

    #[test]
    fn slices_cover_the_model_from_its_lowest_point() {
        assert_eq!(slice_bands(0.0, 1.0, 0.5), vec![(0.0, 0.5), (0.5, 1.0)], "exactly two");
        let b = slice_bands(0.2, 1.0, 0.3);
        assert_eq!(b.len(), 3, "one more for what's left: {b:?}");
        assert!((b[2].0 - 0.8).abs() < 1e-9);
        assert!(slice_bands(0.0, 1.0, 0.0).is_empty());
        let mut g = Grid::new(2.0, 2.0, 400, 0.0);
        g.data[0] = 0.9;
        g.data[1] = 0.3;
        let s = slice_of(&g, 0.5, 1.0);
        assert!((s.data[0] - 0.4).abs() < 1e-6 && s.data[1] == 0.0, "below the slice: nothing");
        let s = slice_of(&g, 0.0, 0.5);
        assert!((s.data[0] - 0.5).abs() < 1e-6 && (s.data[1] - 0.3).abs() < 1e-6, "cut at the top");
    }

    #[test]
    fn an_offset_is_what_a_ball_would_see() {
        // A flat square 0.5 high in the middle of a 10 x 10 grid.
        let mut g = Grid::new(10.0, 10.0, 40_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
                if (3.0..=7.0).contains(&x) && (3.0..=7.0).contains(&y) {
                    g.data[iy * g.nx + ix] = 0.5;
                }
            }
        }
        let up = offset_grid(&g, 0.4);
        assert!((up.sample(5.0, 5.0) - 0.9).abs() < 1e-3, "the top goes up by the distance: {}", up.sample(5.0, 5.0));
        // Beside the edge, as high as the ball's side: 0.5 + sqrt(0.4² - 0.3²).
        let side = 0.5 + (0.16f64 - 0.09).sqrt();
        assert!((up.sample(7.3, 5.0) as f64 - side).abs() < 0.03, "{}", up.sample(7.3, 5.0));
        assert_eq!(up.sample(7.6, 5.0), 0.0, "nothing past the distance");
        let down = offset_grid(&g, -0.2);
        assert!((down.sample(5.0, 5.0) - 0.3).abs() < 1e-3, "inward the top comes down: {}", down.sample(5.0, 5.0));
        assert_eq!(down.sample(7.5, 5.0), 0.0, "and nothing grows");
        assert!(down.sample(6.98, 5.0) < 0.3, "the edge drops away");
    }

    #[test]
    fn smoothing_softens_a_step_by_the_share_asked() {
        let mut g = Grid::new(10.0, 10.0, 10_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                g.data[iy * g.nx + ix] = if (ix as f64 * g.cell) < 5.0 { 1.0 } else { 0.2 };
            }
        }
        let edge = (5.0 / g.cell) as usize;
        let row = g.ny / 2 * g.nx;
        let before = g.data[row + edge - 1];
        let half = smoothed(&g, 0.5, true);
        let full = smoothed(&g, 1.0, true);
        let next_to = half.data[row + edge - 1];
        assert!(next_to < before && next_to > full.data[row + edge - 1], "{before} {next_to}");
        assert_eq!(smoothed(&g, 0.0, true).data, g.data, "nothing at 0%");
        let mut holed = g.clone();
        holed.data[row + 2] = 0.0;
        assert_eq!(smoothed(&holed, 1.0, true).data[row + 2], 0.0, "empty stays empty");
    }

    #[test]
    fn a_grid_becomes_a_surface_and_an_outline() {
        let mut g = Grid::new(10.0, 10.0, 10_000, 0.0);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
                if (2.0..6.0).contains(&x) && (3.0..7.0).contains(&y) {
                    g.data[iy * g.nx + ix] = 0.5;
                }
            }
        }
        let (map, top) = surface_from(&g).unwrap();
        assert!((top - 0.5).abs() < 1e-6);
        assert_eq!(map.value(4.0, 5.0), Some(1.0));
        assert_eq!(map.value(8.0, 8.0), None, "nothing outside");
        let outline = outline_of(&g);
        assert_eq!(outline.len(), 1);
        let (lo, hi) = crate::geom::bbox(outline.iter()).unwrap();
        assert!((lo[0] - 2.0).abs() < 0.2 && (hi[0] - 6.0).abs() < 0.2 && (lo[1] - 3.0).abs() < 0.2 && (hi[1] - 7.0).abs() < 0.2, "{lo:?} {hi:?}");
        assert!(surface_from(&Grid::new(10.0, 10.0, 100, 0.0)).is_none());
    }
}
