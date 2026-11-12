//! Height grids: 3D components built from shapes, the combined model, and the carve simulation.

use crate::geom::{dist2_seg, Path};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Profile {
    Round,
    Angular,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Combine {
    Add,
    Subtract,
    Merge,
    Low,
}

/// Heights brought in from a picture or a 3D model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeightMap {
    pub w: usize,
    pub h: usize,
    /// Rows from the low Y upward. 0 is nothing there; 1 to 65535 go from lowest to highest.
    pub data: Arc<Vec<u16>>,
    /// Bottom left corner on the job.
    pub at: [f64; 2],
    pub width: f64,
}

impl HeightMap {
    /// From 0 to 1 at a place on the job; `None` where there is nothing.
    pub fn value(&self, x: f64, y: f64) -> Option<f32> {
        let per_cell = self.width / self.w as f64;
        let (u, v) = ((x - self.at[0]) / per_cell, (y - self.at[1]) / per_cell);
        if u < 0.0 || v < 0.0 || u >= self.w as f64 || v >= self.h as f64 {
            return None;
        }
        let at = |ix: usize, iy: usize| self.data[iy * self.w + ix];
        let (cx, cy) = (u as usize, v as usize);
        if at(cx, cy) == 0 {
            return None;
        }
        // Between the four nearest cell centres.
        let (fx, fy) = ((u - 0.5).max(0.0), (v - 0.5).max(0.0));
        let (ix, iy) = ((fx as usize).min(self.w - 1), (fy as usize).min(self.h - 1));
        let (nx, ny) = ((ix + 1).min(self.w - 1), (iy + 1).min(self.h - 1));
        let corners = [at(ix, iy), at(nx, iy), at(ix, ny), at(nx, ny)];
        let level = |raw: u16| (raw - 1) as f32 / 65534.0;
        // Beside an empty cell there is nothing to blend with.
        if corners.contains(&0) {
            return Some(level(at(cx, cy)));
        }
        let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
        let low = level(corners[0]) * (1.0 - tx) + level(corners[1]) * tx;
        let high = level(corners[2]) * (1.0 - tx) + level(corners[3]) * tx;
        Some(low * (1.0 - ty) + high * ty)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub paths: Vec<Path>,
    pub profile: Profile,
    pub height: f64,
    pub base: f64,
    pub combine: Combine,
    pub visible: bool,
    /// When set, the height comes from here rather than from the outline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<HeightMap>,
    /// Every cell of the job is part of it, even where its height is 0: the zero plane
    /// (`0x1402b8770` sets every cell of its grid, at 0, as holding something).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole: bool,
}

/// `data` is laid out by rows from y = 0 upward; `cell` is the spacing in job units.
#[derive(Clone, Debug)]
pub struct Grid {
    pub nx: usize,
    pub ny: usize,
    pub cell: f64,
    pub data: Vec<f32>,
}

impl Grid {
    pub fn new(width: f64, height: f64, max_points: usize, fill: f32) -> Grid {
        let cell = (width * height / max_points as f64).sqrt().max(1e-6);
        let nx = (width / cell).ceil() as usize + 1;
        let ny = (height / cell).ceil() as usize + 1;
        Grid { nx, ny, cell, data: vec![fill; nx * ny] }
    }

    /// Interpolated value; positions outside the grid use the nearest edge.
    pub fn sample(&self, x: f64, y: f64) -> f32 {
        let fx = (x / self.cell).clamp(0.0, (self.nx - 1) as f64);
        let fy = (y / self.cell).clamp(0.0, (self.ny - 1) as f64);
        let (ix, iy) = ((fx as usize).min(self.nx - 2), (fy as usize).min(self.ny - 2));
        let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
        let at = |x: usize, y: usize| self.data[y * self.nx + x];
        let a = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
        let b = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
        a * (1.0 - ty) + b * ty
    }

    pub fn max(&self) -> f32 {
        self.data.iter().copied().fold(f32::MIN, f32::max)
    }
}

/// Distance from each inside cell to the nearest outside cell, in job units.
fn inside_distance(mask: &[bool], nx: usize, ny: usize, cell: f64) -> Vec<f32> {
    const FAR: f64 = 1e18;
    let mut d2: Vec<f64> = mask.iter().map(|m| if *m { FAR } else { 0.0 }).collect();
    let mut column = vec![0.0; ny];
    let mut out = vec![0.0; ny.max(nx)];
    for x in 0..nx {
        for y in 0..ny {
            column[y] = d2[y * nx + x];
        }
        parabola_pass(&column, &mut out);
        for y in 0..ny {
            d2[y * nx + x] = out[y];
        }
    }
    let mut row = vec![0.0; nx];
    for y in 0..ny {
        row.copy_from_slice(&d2[y * nx..(y + 1) * nx]);
        parabola_pass(&row, &mut out);
        d2[y * nx..(y + 1) * nx].copy_from_slice(&out[..nx]);
    }
    d2.iter().map(|v| (v.sqrt() * cell) as f32).collect()
}

/// One pass of the lower-envelope-of-parabolas distance transform.
fn parabola_pass(f: &[f64], out: &mut [f64]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0f64; n + 1];
    let mut k = 0;
    z[0] = f64::MIN;
    z[1] = f64::MAX;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                v[0] = q;
                z[1] = f64::MAX;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = f64::MAX;
            }
            break;
        }
    }
    let mut k = 0;
    for (q, o) in out.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k] as f64;
        *o = (q as f64 - p).powi(2) + f[v[k]];
    }
}

/// Which cells of a grid lie inside the shape, using exact row crossings.
pub fn raster_mask(paths: &[Path], nx: usize, ny: usize, cell: f64) -> Vec<bool> {
    let mut mask = vec![false; nx * ny];
    let segs: Vec<_> = paths.iter().filter(|p| p.closed).flat_map(|p| p.segments()).collect();
    for iy in 0..ny {
        let y = iy as f64 * cell;
        let mut xs: Vec<f64> = segs
            .iter()
            .filter(|(a, b)| (a[1] > y) != (b[1] > y))
            .map(|(a, b)| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
            .collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            let from = (pair[0] / cell).ceil().max(0.0) as usize;
            let to = (pair[1] / cell).floor().min(nx as f64 - 1.0);
            if to < 0.0 {
                continue;
            }
            for ix in from..=to as usize {
                mask[iy * nx + ix] = true;
            }
        }
    }
    mask
}

/// The exact distance to the outline for cells within `reach` of it; `f32::MAX` for the others.
fn edge_distance(paths: &[Path], nx: usize, ny: usize, cell: f64, reach: f64) -> Vec<f32> {
    let mut out = vec![f32::MAX; nx * ny];
    for (a, b) in paths.iter().filter(|p| p.closed).flat_map(|p| p.segments()) {
        let span = |p: f64, q: f64, n: usize| {
            let from = ((p.min(q) - reach) / cell).floor().max(0.0) as usize;
            let to = (((p.max(q) + reach) / cell).ceil().max(0.0) as usize).min(n.saturating_sub(1));
            (from, to)
        };
        let (x0, x1) = span(a[0], b[0], nx);
        let (y0, y1) = span(a[1], b[1], ny);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let d2 = dist2_seg([ix as f64 * cell, iy as f64 * cell], a, b);
                if d2 <= reach * reach {
                    let slot = &mut out[iy * nx + ix];
                    *slot = slot.min(d2.sqrt() as f32);
                }
            }
        }
    }
    out
}

/// Evens out the grid-sized ripples of a distance worked out cell by cell.
fn soften(values: &[f32], nx: usize, ny: usize) -> Vec<f32> {
    let mut out = values.to_vec();
    for y in 1..ny.saturating_sub(1) {
        for x in 1..nx.saturating_sub(1) {
            let mut sum = 0.0;
            for row in [y - 1, y, y + 1] {
                sum += values[row * nx + x - 1] + values[row * nx + x] + values[row * nx + x + 1];
            }
            out[y * nx + x] = sum / 9.0;
        }
    }
    out
}

/// The height the component adds at each cell (0 outside its shape).
pub fn component_height(c: &Component, width: f64, height: f64, points: usize) -> Grid {
    let mut grid = Grid::new(width, height, points, 0.0);
    if let Some(map) = &c.surface {
        for iy in 0..grid.ny {
            for ix in 0..grid.nx {
                if let Some(v) = map.value(ix as f64 * grid.cell, iy as f64 * grid.cell) {
                    grid.data[iy * grid.nx + ix] = c.base as f32 + c.height as f32 * v;
                }
            }
        }
        return grid;
    }
    let cell = grid.cell as f32;
    let mask = raster_mask(&c.paths, grid.nx, grid.ny, grid.cell);
    let raw = inside_distance(&mask, grid.nx, grid.ny, grid.cell);
    let middle: Vec<f32> = raw.iter().map(|d| (d - cell * 0.5).max(0.0)).collect();
    let middle = soften(&middle, grid.nx, grid.ny);
    // Near the outline the true distance is known, so the edge falls between cells where it should.
    let edge = edge_distance(&c.paths, grid.nx, grid.ny, grid.cell, grid.cell * 3.0);
    let widest = middle.iter().copied().fold(0.0f32, f32::max).max(1e-6);
    for (i, h) in grid.data.iter_mut().enumerate() {
        let signed = match (edge[i] < f32::MAX, mask[i]) {
            (true, true) => edge[i],
            (true, false) => -edge[i],
            (false, true) => middle[i],
            (false, false) => continue,
        };
        // A sloped rim two cells wide, centred on the outline, instead of a jump from cell to cell.
        let cover = ((signed + cell) / (2.0 * cell)).clamp(0.0, 1.0);
        if cover <= 0.0 {
            continue;
        }
        let t = (signed / widest).clamp(0.0, 1.0);
        let shape = match c.profile {
            Profile::Round => (1.0 - (1.0 - t) * (1.0 - t)).max(0.0).sqrt(),
            Profile::Angular => t,
            Profile::Flat => 1.0,
        };
        *h = (c.base as f32 + c.height as f32 * shape) * cover;
    }
    grid
}

/// Combines visible components, in order, into one height grid.
///
/// A place holds something once a component has covered it. Subtracting where something is
/// goes on below zero, as in the original, whose cells left empty (9999999) stay empty when
/// subtracted from; where nothing is yet it stops at 0. That is what the zero plane is for: it
/// covers the whole job at 0, so dishes cut into it go below.
pub fn compose(components: &[Component], width: f64, height: f64, points: usize) -> Option<Grid> {
    let mut out = Grid::new(width, height, points, 0.0);
    let mut holds = vec![false; out.data.len()];
    let mut any = false;
    for c in components.iter().filter(|c| c.visible) {
        let g = component_height(c, width, height, points);
        any = true;
        for ((o, v), held) in out.data.iter_mut().zip(&g.data).zip(holds.iter_mut()) {
            if *v == 0.0 && !c.whole {
                continue;
            }
            *o = match (c.combine, *held) {
                (Combine::Add, _) => *o + v,
                (Combine::Subtract, true) => *o - v,
                (Combine::Subtract, false) => (*o - v).max(0.0),
                (Combine::Merge, true) => o.max(*v),
                (Combine::Low, true) => o.min(*v),
                (Combine::Merge | Combine::Low, false) => *v,
            };
            *held |= c.combine != Combine::Subtract;
        }
    }
    any.then_some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolKind {
    EndMill,
    BallNose,
    VBit,
    Drill,
    Engraving,
}

/// How far above the tool's tip its cutting surface is, `r` from its centre.
pub fn tool_lift(kind: ToolKind, dia: f64, angle: f64, r: f64) -> f64 {
    let radius = dia / 2.0;
    match kind {
        ToolKind::EndMill | ToolKind::Drill => 0.0,
        ToolKind::BallNose => radius - (radius * radius - r * r).max(0.0).sqrt(),
        ToolKind::VBit | ToolKind::Engraving => r / (angle.to_radians() / 2.0).tan().max(1e-3),
    }
}

/// Cells around the tool centre as (dx, dy, lift).
pub fn tool_kernel(kind: ToolKind, dia: f64, angle: f64, cell: f64) -> Vec<(i32, i32, f32)> {
    let radius = dia / 2.0;
    let reach = (radius / cell).ceil() as i32;
    let mut out = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let r = ((dx * dx + dy * dy) as f64).sqrt() * cell;
            if r <= radius {
                out.push((dx, dy, tool_lift(kind, dia, angle, r) as f32));
            }
        }
    }
    if out.is_empty() {
        out.push((0, 0, 0.0));
    }
    out
}

/// The lowest height the tool tip may reach over each cell without gouging `surface`.
pub fn tool_surface(surface: &Grid, kind: ToolKind, dia: f64, angle: f64) -> Grid {
    let kernel = tool_kernel(kind, dia, angle, surface.cell);
    let mut out = surface.clone();
    let (nx, ny) = (surface.nx, surface.ny);
    let threads = crate::platform::threads(false);
    let rows_per = ny.div_ceil(threads).max(1);
    crate::platform::scope(|s| {
        for (ci, chunk) in out.data.chunks_mut(nx * rows_per).enumerate() {
            let kernel = &kernel;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(nx).enumerate() {
                    let y = (ci * rows_per + r) as i32;
                    for (x, v) in row.iter_mut().enumerate() {
                        let mut top = f32::MIN;
                        for (dx, dy, lift) in kernel {
                            let (px, py) = (x as i32 + dx, y + dy);
                            if px >= 0 && py >= 0 && (px as usize) < nx && (py as usize) < ny {
                                top = top.max(surface.data[py as usize * nx + px as usize] - lift);
                            }
                        }
                        *v = top;
                    }
                }
            });
        }
    });
    out
}

/// What a cell of the carved surface is, for the picture: a level the grid describes as it is,
pub const LEVEL: u8 = 0;
/// the slope standing in for an upright wall,
pub const WALL: u8 = 1;
/// or the curved or pointed shape of a tool, which the grid can only approach.
pub const SHAPED: u8 = 2;

/// The shape a tool leaves, worked out from where it really is rather than the nearest cell.
#[derive(Clone, Debug)]
pub struct Cutter {
    cell: f32,
    /// Inside this radius the surface is the tool's own shape.
    inner: f32,
    /// Width of the slope that carries the wall between cells of the grid.
    band: f32,
    radius: f32,
    /// How far the tool's surface is above its tip, every `lift_step` out from the centre.
    lifts: Vec<f32>,
    lift_step: f32,
    /// What it was made from, to tell one tool from another.
    made_from: (ToolKind, f32, f32),
    /// A flat bottom, so everything it cuts is level.
    flat: bool,
}

impl Cutter {
    pub fn new(kind: ToolKind, dia: f64, angle: f64, cell: f64) -> Cutter {
        let band = cell * 2.0;
        // A very thin tool still reaches its depth at the nearest cell.
        let inner = (dia / 2.0 - band / 2.0).max(cell * 0.5);
        let radius = dia / 2.0;
        // Worked out once: this runs millions of times for one carve.
        let lift_step = (cell / 16.0).max(radius / 4096.0);
        let count = (radius / lift_step).ceil() as usize + 2;
        let lifts: Vec<f32> = (0..count).map(|i| tool_lift(kind, dia, angle, (i as f64 * lift_step).min(radius)) as f32).collect();
        let flat = lifts.iter().all(|l| l.abs() < 1e-7);
        Cutter { cell: cell as f32, inner: inner as f32, band: band as f32, radius: radius as f32, lifts, lift_step: lift_step as f32, made_from: (kind, dia as f32, angle as f32), flat }
    }

    /// How far from its centre this tool cuts anything, with its tip at `z`: a pointed tool only
    /// reaches as far as it is deep.
    fn reach_at(&self, z: f32) -> f32 {
        if self.flat {
            return self.radius;
        }
        let i = self.lifts.partition_point(|l| z + *l < 0.0);
        (i as f32 * self.lift_step).min(self.radius)
    }

    fn lift(&self, r: f32) -> f32 {
        let at = r.min(self.radius) / self.lift_step;
        let i = (at as usize).min(self.lifts.len() - 2);
        let t = at - i as f32;
        self.lifts[i] * (1.0 - t) + self.lifts[i + 1] * t
    }

    /// The height the tool leaves `r` from its centre, with its tip at `z`, and what kind of place
    /// that makes it for the picture.
    fn height(&self, r: f32, z: f32) -> (f32, u8) {
        let shaped = if self.flat { LEVEL } else { SHAPED };
        if r <= self.inner {
            return (z + self.lift(r), shaped);
        }
        let rim = z + self.lift(self.inner);
        if rim >= 0.0 {
            // The sloped side of the tool already comes out of the material.
            return (z + self.lift(r), shaped);
        }
        (rim + (-rim) * ((r - self.inner) / self.band).min(1.0), WALL)
    }

    fn outer(&self) -> f32 {
        self.inner + self.band
    }
}

/// One cutting move of a tool.
#[derive(Clone, Copy, Debug)]
pub struct CutMove {
    a: [f32; 3],
    b: [f32; 3],
    tool: u16,
}

/// The moves sorted by where they are, to work out the true surface at any point. Unlike a grid it
/// has no size of its own: a wall is exactly where the tool's edge passed, at any zoom.
pub struct CutIndex {
    moves: Vec<CutMove>,
    tools: Vec<Cutter>,
    step: f32,
    nx: usize,
    ny: usize,
    /// For each square of `step`: where its list starts in `lists`.
    starts: Vec<u32>,
    lists: Vec<u32>,
    /// How many moves a look-up goes through, on average.
    pub crowding: f32,
}

/// More moves than this would take too much memory to sort: the grid alone has to do.
const MOST_INDEXED_MOVES: usize = 6_000_000;

/// Where a tool reaches at one point.
struct Reach {
    /// The height it leaves there, once it is within its radius.
    z: f32,
    distance: f32,
    closest: [f32; 2],
}

impl CutIndex {
    pub fn build(sim: &Sim) -> Option<CutIndex> {
        if sim.moves.is_empty() || sim.moves.len() > MOST_INDEXED_MOVES {
            return None;
        }
        let (w, h) = ((sim.grid.nx - 1) as f32 * sim.grid.cell as f32, (sim.grid.ny - 1) as f32 * sim.grid.cell as f32);
        let smallest = sim.tools.iter().map(|t| t.radius).fold(f32::MAX, f32::min);
        let step = smallest.max((w * h / 2_000_000.0).sqrt()).max(1e-5);
        let (nx, ny) = ((w / step) as usize + 2, (h / step) as usize + 2);
        // Every square of the grid that a move's tool can reach, row by row.
        let squares = |m: &CutMove, at: &mut dyn FnMut(usize)| {
            let reach = sim.tools[m.tool as usize].reach_at(m.a[2].min(m.b[2])) + step * 0.1;
            let (y0, y1) = (m.a[1].min(m.b[1]) - reach, m.a[1].max(m.b[1]) + reach);
            let (row0, row1) = (((y0 / step).floor().max(0.0)) as usize, ((y1 / step).floor().max(0.0) as usize).min(ny - 1));
            for row in row0..=row1 {
                // Only the part of the move that comes near this row.
                let (low, high) = (row as f32 * step - reach, (row + 1) as f32 * step + reach);
                let dy = m.b[1] - m.a[1];
                let (t0, t1) = if dy.abs() < 1e-6 {
                    (0.0, 1.0)
                } else {
                    let (p, q) = ((low - m.a[1]) / dy, (high - m.a[1]) / dy);
                    (p.min(q).clamp(0.0, 1.0), p.max(q).clamp(0.0, 1.0))
                };
                let (xa, xb) = (m.a[0] + (m.b[0] - m.a[0]) * t0, m.a[0] + (m.b[0] - m.a[0]) * t1);
                let (x0, x1) = (xa.min(xb) - reach, xa.max(xb) + reach);
                let (col0, col1) = (((x0 / step).floor().max(0.0)) as usize, ((x1 / step).floor().max(0.0) as usize).min(nx - 1));
                for col in col0..=col1 {
                    at(row * nx + col);
                }
            }
        };
        let mut counts = vec![0u32; nx * ny + 1];
        for m in &sim.moves {
            squares(m, &mut |square| counts[square + 1] += 1);
        }
        for i in 1..counts.len() {
            counts[i] += counts[i - 1];
        }
        let mut lists = vec![0u32; *counts.last()? as usize];
        let mut next = counts.clone();
        for (index, m) in sim.moves.iter().enumerate() {
            squares(m, &mut |square| {
                lists[next[square] as usize] = index as u32;
                next[square] += 1;
            });
        }
        // How long a look-up takes: where the moves are very dense, only the walls can afford it.
        let used = counts.windows(2).filter(|w| w[1] > w[0]).count().max(1);
        let crowding = lists.len() as f32 / used as f32;
        Some(CutIndex { moves: sim.moves.clone(), tools: sim.tools.clone(), step, nx, ny, starts: counts, lists, crowding })
    }

    fn nearby(&self, x: f32, y: f32) -> impl Iterator<Item = &CutMove> {
        let (col, row) = (((x / self.step).max(0.0) as usize).min(self.nx - 1), ((y / self.step).max(0.0) as usize).min(self.ny - 1));
        let square = row * self.nx + col;
        self.lists[self.starts[square] as usize..self.starts[square + 1] as usize].iter().map(|i| &self.moves[*i as usize])
    }

    /// How a move's tool stands to a point: what it left there, if within its radius.
    fn reach(&self, m: &CutMove, x: f32, y: f32) -> Reach {
        let tool = &self.tools[m.tool as usize];
        let (dx, dy) = (m.b[0] - m.a[0], m.b[1] - m.a[1]);
        let length2 = dx * dx + dy * dy;
        let (px, py) = (x - m.a[0], y - m.a[1]);
        let along = if length2 > 1e-10 { (px * dx + py * dy) / length2 } else { 0.0 };
        let t = along.clamp(0.0, 1.0);
        let closest = [m.a[0] + dx * t, m.a[1] + dy * t];
        let distance = (x - closest[0]).hypot(y - closest[1]);
        let height = |t: f32| m.a[2] + (m.b[2] - m.a[2]) * t;
        let radius = tool.radius;
        let z = if length2 <= 1e-10 {
            // Straight down: it got as low as its lower end.
            m.a[2].min(m.b[2]) + if tool.flat { 0.0 } else { tool.lift(distance) }
        } else if tool.flat && m.a[2] != m.b[2] && distance <= radius {
            // On a slope a flat tool leaves the height of its lowest place over this point.
            let off = (px * dy - py * dx) / length2.sqrt();
            let half = ((radius * radius - off * off).max(0.0) / length2).sqrt();
            height((along - half).clamp(0.0, 1.0)).min(height((along + half).clamp(0.0, 1.0)))
        } else {
            height(t) + if tool.flat { 0.0 } else { tool.lift(distance) }
        };
        Reach { z, distance, closest }
    }

    /// The true surface at a point, and whether it is level there.
    pub fn surface(&self, x: f32, y: f32) -> (f32, bool) {
        let (mut lowest, mut level) = (0f32, true);
        for m in self.nearby(x, y) {
            let tool = &self.tools[m.tool as usize];
            let r = self.reach(m, x, y);
            if r.distance <= tool.radius && r.z < lowest {
                lowest = r.z;
                level = tool.flat;
            }
        }
        (lowest, level)
    }

    /// Which way the wall at this point faces: towards the tool that made it, from the nearest
    /// edge of a tool that went below the height `z`.
    pub fn wall_facing(&self, x: f32, y: f32, z: f32) -> Option<[f32; 2]> {
        let mut best: Option<(f32, [f32; 2])> = None;
        for m in self.nearby(x, y) {
            let tool = &self.tools[m.tool as usize];
            let r = self.reach(m, x, y);
            if r.z >= z || r.distance < 1e-6 {
                continue;
            }
            let gap = (r.distance - tool.radius).abs();
            if best.is_none_or(|(g, _)| gap < g) {
                best = Some((gap, [(r.closest[0] - x) / r.distance, (r.closest[1] - y) / r.distance]));
            }
        }
        best.map(|(_, facing)| facing)
    }
}

/// How far, in cells, the slope standing for a wall can reach.
const WALL_CELLS: usize = 3;

/// The material surface while carving: 0 at the top, negative where cut.
#[derive(Clone, Debug)]
pub struct Sim {
    pub grid: Grid,
    pub thickness: f32,
    /// What each cell is: see `LEVEL`, `WALL` and `SHAPED`. The picture works out the true surface
    /// wherever a cell isn't a plain level, so it stays sharp and without stairs.
    pub walls: Vec<u8>,
    /// Every cutting move so far, from which the walls are worked out exactly when drawn.
    pub moves: Vec<CutMove>,
    pub tools: Vec<Cutter>,
    /// Where the tool last marked the surface, to save marking the same spot again.
    last_mark: Option<[f64; 3]>,
}

impl Sim {
    pub fn new(width: f64, height: f64, thickness: f64, points: usize) -> Sim {
        let grid = Grid::new(width, height, points, 0.0);
        let walls = vec![LEVEL; grid.data.len()];
        Sim { grid, thickness: thickness as f32, walls, moves: Vec::new(), tools: Vec::new(), last_mark: None }
    }

    /// Cuts along a straight move and records which toolpath touched each cell.
    pub fn cut_move(&mut self, cutter: &Cutter, tags: &mut [u8], a: [f64; 3], b: [f64; 3], tag: u8) {
        if a[2] < 0.0 || b[2] < 0.0 {
            let tool = match self.tools.iter().rposition(|t| t.made_from == cutter.made_from) {
                Some(i) => i,
                None => {
                    self.tools.push(cutter.clone());
                    self.tools.len() - 1
                }
            };
            self.moves.push(CutMove { a: a.map(|v| v as f32), b: b.map(|v| v as f32), tool: tool as u16 });
        }
        let g = &mut self.grid;
        let cell = g.cell;
        let floor = -self.thickness;
        let outer = cutter.outer() as f64;
        let outer2 = cutter.outer() * cutter.outer();
        let cell32 = cutter.cell;
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        // Under a cell at a time: the small scallops between stops are far finer than the grid.
        let steps = ((len / (cell * 0.75)).ceil() as usize).max(1);
        // Long paths are often many tiny moves: spots a fraction of a cell apart add nothing.
        let close = |p: [f64; 3], q: [f64; 3]| (p[0] - q[0]).hypot(p[1] - q[1]) < cell * 0.4 && (p[2] - q[2]).abs() < cell * 0.05;
        if self.last_mark.is_some_and(|m| close(m, b) && close(m, a)) {
            return;
        }
        let continues = self.last_mark == Some(a);
        self.last_mark = Some(b);
        for i in usize::from(continues)..=steps {
            let t = i as f64 / steps as f64;
            let (x, y, z) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t);
            if z >= 0.0 {
                continue;
            }
            let z = z as f32;
            let lowest = z.max(floor);
            let range = |v: f64, n: usize| {
                let from = ((v - outer) / cell).ceil().max(0.0) as usize;
                let to = ((v + outer) / cell).floor();
                (from, if to < 0.0 { None } else { Some((to as usize).min(n - 1)) })
            };
            let ((x0, x1), (y0, y1)) = (range(x, g.nx), range(y, g.ny));
            let (Some(x1), Some(y1)) = (x1, y1) else { continue };
            // Where the tool is, in cells; distances stay small and exact enough for 32 bits.
            let (fx, fy) = ((x / cell) as f32, (y / cell) as f32);
            for iy in y0..=y1 {
                let dy = (iy as f32 - fy) * cell32;
                for ix in x0..=x1 {
                    let at = iy * g.nx + ix;
                    // Already as low as this tool can go: the common case, and the quickest.
                    if g.data[at] <= lowest {
                        continue;
                    }
                    let dx = (ix as f32 - fx) * cell32;
                    let r2 = dx * dx + dy * dy;
                    if r2 > outer2 {
                        continue;
                    }
                    let (wanted, wall) = cutter.height(r2.sqrt(), z);
                    let wanted = wanted.max(floor);
                    // A wall can pass here without lowering this cell, beside a level almost as deep.
                    if wall == WALL && cutter.flat {
                        self.walls[at] = WALL;
                    }
                    if wanted < g.data[at] {
                        g.data[at] = wanted;
                        self.walls[at] = wall;
                        if let Some(t) = tags.get_mut(at) {
                            *t = tag;
                        }
                    }
                }
            }
        }
    }

    pub fn is_through(&self, ix: usize, iy: usize) -> bool {
        self.grid.data[iy * self.grid.nx + ix] <= -self.thickness + 1e-4
    }

    /// Removes the loose piece at `(x, y)`; refuses when nothing has been cut free.
    pub fn remove_waste(&mut self, x: f64, y: f64) -> bool {
        let g = &self.grid;
        let (nx, ny) = (g.nx, g.ny);
        let (ix, iy) = ((x / g.cell).round() as isize, (y / g.cell).round() as isize);
        if ix < 0 || iy < 0 || ix >= nx as isize || iy >= ny as isize {
            return false;
        }
        let start = iy as usize * nx + ix as usize;
        if self.is_through(ix as usize, iy as usize) {
            return false;
        }
        let mut seen = vec![false; nx * ny];
        seen[start] = true;
        let mut stack = vec![start];
        let mut piece = Vec::new();
        while let Some(i) = stack.pop() {
            piece.push(i);
            let (cx, cy) = (i % nx, i / nx);
            let mut visit = |j: usize, seen: &mut Vec<bool>| {
                if !seen[j] && self.grid.data[j] > -self.thickness + 1e-4 {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if cx > 0 {
                visit(i - 1, &mut seen);
            }
            if cx + 1 < nx {
                visit(i + 1, &mut seen);
            }
            if cy > 0 {
                visit(i - nx, &mut seen);
            }
            if cy + 1 < ny {
                visit(i + nx, &mut seen);
            }
        }
        let solid = self.grid.data.iter().filter(|v| **v > -self.thickness + 1e-4).count();
        if piece.len() == solid {
            return false;
        }
        for i in piece {
            self.grid.data[i] = -self.thickness;
            self.walls[i] = LEVEL;
            // The wall this piece stood behind is gone too: left marked, the picture would work out
            // the surface there from the tool's moves and put back slivers of the piece.
            let (cx, cy) = (i % nx, i / nx);
            for y in cy.saturating_sub(WALL_CELLS)..=(cy + WALL_CELLS).min(ny - 1) {
                for x in cx.saturating_sub(WALL_CELLS)..=(cx + WALL_CELLS).min(nx - 1) {
                    let j = y * nx + x;
                    if self.grid.data[j] <= -self.thickness + 1e-4 {
                        self.walls[j] = LEVEL;
                    }
                }
            }
        }
        true
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{circle, rect};

    fn comp(profile: Profile, combine: Combine, r: f64) -> Component {
        Component {
            surface: None,
            name: "c".into(),
            paths: vec![Path::closed(circle([5.0, 5.0], r))],
            profile,
            height: 0.5,
            base: 0.0,
            combine,
            visible: true,
            whole: false,
        }
    }

    #[test]
    fn dome_peaks_in_the_middle_and_is_zero_outside() {
        let g = compose(&[comp(Profile::Round, Combine::Add, 2.0)], 10.0, 10.0, 40_000).unwrap();
        assert!((g.sample(5.0, 5.0) - 0.5).abs() < 0.02);
        assert_eq!(g.sample(0.5, 0.5), 0.0);
        let mid = g.sample(6.0, 5.0);
        assert!(mid > 0.3 && mid < 0.5, "shoulder of the dome is {mid}");
    }

    #[test]
    fn subtract_lowers_and_merge_keeps_the_higher() {
        let base = Component {
            paths: vec![Path::closed(rect([5.0, 5.0], 8.0, 8.0, 0.0))],
            ..comp(Profile::Flat, Combine::Add, 1.0)
        };
        let cut = Component { height: 0.2, ..comp(Profile::Flat, Combine::Subtract, 1.0) };
        let g = compose(&[base.clone(), cut], 10.0, 10.0, 40_000).unwrap();
        assert!((g.sample(5.0, 5.0) - 0.3).abs() < 1e-3);
        assert!((g.sample(2.0, 2.0) - 0.5).abs() < 1e-3);
        let tall = Component { height: 0.9, ..comp(Profile::Flat, Combine::Merge, 1.0) };
        let g = compose(&[base, tall], 10.0, 10.0, 40_000).unwrap();
        assert!((g.sample(5.0, 5.0) - 0.9).abs() < 1e-3);
    }

    #[test]
    fn a_dish_goes_below_zero_only_into_something() {
        let dish = Component { height: 0.3, ..comp(Profile::Round, Combine::Subtract, 2.0) };
        let alone = compose(&[dish.clone()], 10.0, 10.0, 40_000).unwrap();
        assert_eq!(alone.sample(5.0, 5.0), 0.0, "with nothing under it, nothing is cut");
        let plane = Component { name: "Zero Plane".into(), paths: vec![Path::closed(rect([5.0, 5.0], 10.0, 10.0, 0.0))], height: 0.0, whole: true, ..comp(Profile::Flat, Combine::Add, 1.0) };
        let g = compose(&[plane, dish], 10.0, 10.0, 40_000).unwrap();
        assert!((g.sample(5.0, 5.0) + 0.3).abs() < 1e-2, "the dish's bottom is at -0.3: {}", g.sample(5.0, 5.0));
        assert_eq!(g.sample(1.0, 1.0), 0.0);
        assert_eq!(g.max(), 0.0);
        // Cutting deeper than a raised shape goes on through it too.
        let base = Component { paths: vec![Path::closed(rect([5.0, 5.0], 8.0, 8.0, 0.0))], height: 0.1, ..comp(Profile::Flat, Combine::Add, 1.0) };
        let deep = Component { height: 0.4, ..comp(Profile::Flat, Combine::Subtract, 1.0) };
        let g = compose(&[base, deep], 10.0, 10.0, 40_000).unwrap();
        assert!((g.sample(5.0, 5.0) + 0.3).abs() < 1e-3);
    }

    /// Follows a closed loop with an end mill.
    fn cut_loop(sim: &mut Sim, dia: f64, pts: &[[f64; 2]], z: f64) {
        let cutter = Cutter::new(ToolKind::EndMill, dia, 0.0, sim.grid.cell);
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            sim.cut_move(&cutter, &mut [], [a[0], a[1], z], [b[0], b[1], z], 0);
        }
    }

    #[test]
    fn a_cut_ring_frees_the_middle_and_only_that_piece_is_removed() {
        let mut sim = Sim::new(10.0, 10.0, 0.5, 250_000);
        cut_loop(&mut sim, 0.25, &circle([5.0, 5.0], 2.0), -0.5);
        assert!(sim.is_through((7.0 / sim.grid.cell).round() as usize, (5.0 / sim.grid.cell).round() as usize));
        assert!(sim.remove_waste(5.0, 5.0), "the disc is loose");
        assert!(sim.grid.sample(5.0, 5.0) <= -0.49);
        assert_eq!(sim.grid.sample(1.0, 1.0), 0.0, "the outside stays");
        assert!(!sim.remove_waste(1.0, 1.0), "the last piece cannot be removed");
    }

    /// Going out from a point, how far until the surface comes up through `level`.
    fn crossing(g: &Grid, c: [f64; 2], angle: f64, from: f64, to: f64, level: f32) -> f64 {
        let at = |r: f64| g.sample(c[0] + r * angle.cos(), c[1] + r * angle.sin());
        let (mut lo, mut hi) = (from, to);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if at(mid) < level {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// How far a round edge strays from a true circle, measured all the way around.
    fn roundness(g: &Grid, c: [f64; 2], from: f64, to: f64, level: f32) -> (f64, f64) {
        let radii: Vec<f64> = (0..720).map(|i| crossing(g, c, i as f64 / 720.0 * std::f64::consts::TAU, from, to, level)).collect();
        let (low, high) = radii.iter().fold((f64::MAX, f64::MIN), |(l, h), r| (l.min(*r), h.max(*r)));
        ((low + high) / 2.0, high - low)
    }

    #[test]
    fn a_round_cut_has_no_stair_steps() {
        let mut sim = Sim::new(10.0, 10.0, 0.5, 1_000_000);
        // A small tool following a many-sided circle: the wall sits at radius 2.125.
        let pts: Vec<[f64; 2]> = (0..2880).map(|i| {
            let a = i as f64 / 2880.0 * std::f64::consts::TAU;
            [5.03 + 2.0 * a.cos(), 5.017 + 2.0 * a.sin()]
        }).collect();
        cut_loop(&mut sim, 0.25, &pts, -0.5);
        let (radius, spread) = roundness(&sim.grid, [5.03, 5.017], 2.0, 2.3, -0.25);
        assert!((radius - 2.125).abs() < 0.005, "radius {radius}");
        // Whole cells would make this at least one cell (0.01); it must be a small part of one.
        assert!(spread < sim.grid.cell * 0.25, "the edge wanders by {spread}, the cell is {}", sim.grid.cell);
    }

    #[test]
    fn a_cut_lands_between_the_cells_where_the_tool_really_is() {
        let edge_for = |y: f64| {
            let mut sim = Sim::new(10.0, 10.0, 0.5, 1_000_000);
            let c = Cutter::new(ToolKind::EndMill, 0.25, 0.0, sim.grid.cell);
            sim.cut_move(&c, &mut [], [1.0, y, -0.2], [9.0, y, -0.2], 0);
            crossing(&sim.grid, [5.0, y], std::f64::consts::FRAC_PI_2, 0.0, 0.3, -0.1) + y
        };
        // Moves of a third of a cell show up as moves of the wall.
        let cell = Sim::new(10.0, 10.0, 0.5, 1_000_000).grid.cell;
        let (a, b) = (edge_for(5.0), edge_for(5.0 + cell / 3.0));
        assert!(((b - a) - cell / 3.0).abs() < cell * 0.1, "{a} {b} cell {cell}");
        assert!((a - 5.125).abs() < cell * 0.1, "{a}");
    }

    #[test]
    fn many_tiny_moves_cut_the_same_as_one_long_one() {
        let mut one = Sim::new(10.0, 10.0, 0.5, 250_000);
        let c = Cutter::new(ToolKind::BallNose, 0.25, 0.0, one.grid.cell);
        one.cut_move(&c, &mut [], [2.0, 5.0, -0.1], [8.0, 5.0, -0.1], 0);
        let mut tiny = Sim::new(10.0, 10.0, 0.5, 250_000);
        let n = 3000;
        for i in 0..n {
            let x = |k: usize| 2.0 + 6.0 * k as f64 / n as f64;
            tiny.cut_move(&c, &mut [], [x(i), 5.0, -0.1], [x(i + 1), 5.0, -0.1], 0);
        }
        let worst = one.grid.data.iter().zip(&tiny.grid.data).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        // Only the very end of the path may stop a fraction of a cell short.
        let mid = (0..one.grid.ny).map(|y| y * one.grid.nx + (5.0 / one.grid.cell) as usize);
        let worst_mid = mid.map(|i| (one.grid.data[i] - tiny.grid.data[i]).abs()).fold(0.0, f32::max);
        assert!(worst_mid < 0.002, "{worst_mid}");
        assert!(worst < 0.05, "{worst}");
    }

    #[test]
    fn cutting_the_same_path_again_changes_nothing() {
        let mut sim = Sim::new(10.0, 10.0, 0.5, 250_000);
        cut_loop(&mut sim, 0.25, &circle([5.0, 5.0], 2.0), -0.2);
        let once = sim.grid.data.clone();
        cut_loop(&mut sim, 0.25, &circle([5.0, 5.0], 2.0), -0.2);
        assert_eq!(once, sim.grid.data);
    }

    #[test]
    fn a_v_bit_leaves_its_true_slope_and_a_thin_tool_reaches_its_depth() {
        let mut sim = Sim::new(10.0, 10.0, 0.5, 1_000_000);
        // A 90 degree bit: the surface rises as much as it moves sideways.
        let c = Cutter::new(ToolKind::VBit, 1.0, 90.0, sim.grid.cell);
        sim.cut_move(&c, &mut [], [1.0, 5.0, -0.2], [9.0, 5.0, -0.2], 0);
        for (off, want) in [(0.0, -0.2), (0.05, -0.15), (0.1, -0.1), (0.15, -0.05), (0.3, 0.0)] {
            let got = sim.grid.sample(5.0, 5.0 + off) as f64;
            assert!((got - want).abs() < 0.012, "{off}: {got} vs {want}");
        }
        let mut sim = Sim::new(10.0, 10.0, 0.5, 1_000_000);
        let thin = Cutter::new(ToolKind::Engraving, 0.005, 30.0, sim.grid.cell);
        sim.cut_move(&thin, &mut [], [1.0, 5.003, -0.1], [9.0, 5.003, -0.1], 0);
        let lowest = (450..550).map(|i| sim.grid.sample(5.0, i as f64 * 0.01)).fold(0.0, f32::min);
        assert!(lowest < -0.05, "{lowest}");
    }

    #[test]
    fn a_cut_never_goes_below_the_material_and_tags_what_it_touched() {
        let mut sim = Sim::new(10.0, 10.0, 0.5, 40_000);
        let mut tags = vec![255u8; sim.grid.data.len()];
        let c = Cutter::new(ToolKind::EndMill, 0.5, 0.0, sim.grid.cell);
        sim.cut_move(&c, &mut tags, [5.0, 5.0, -3.0], [6.0, 5.0, -3.0], 7);
        assert!(sim.grid.data.iter().all(|v| *v >= -0.5));
        let i = (5.5 / sim.grid.cell).round() as usize;
        let j = (5.0 / sim.grid.cell).round() as usize;
        assert_eq!(tags[j * sim.grid.nx + i], 7);
        assert_eq!(tags[0], 255);
    }

    #[test]
    fn a_flat_shape_has_a_smooth_edge_too() {
        let comp = Component {
            name: "disc".into(),
            paths: vec![Path::closed(circle([5.03, 5.017], 2.0))],
            profile: Profile::Flat,
            height: 0.5,
            base: 0.0,
            combine: Combine::Add,
            visible: true,
            surface: None,
            whole: false,
        };
        let g = component_height(&comp, 10.0, 10.0, 1_000_000);
        // Turned upside down, so going outward climbs through the level as with a hole.
        let flipped = Grid { data: g.data.iter().map(|v| -v).collect(), ..g.clone() };
        let (radius, spread) = roundness(&flipped, [5.03, 5.017], 1.8, 2.2, -0.25);
        // The outline itself has 128 sides, so it strays by about 2r(1 - cos(pi/128)).
        assert!((radius - 2.0).abs() < 0.005, "radius {radius}");
        assert!(spread < 0.0012 + g.cell * 0.2, "the edge wanders by {spread}, the cell is {}", g.cell);
    }

    #[test]
    fn ball_tool_surface_never_gouges() {
        let g = compose(&[comp(Profile::Round, Combine::Add, 2.0)], 10.0, 10.0, 40_000).unwrap();
        let t = tool_surface(&g, ToolKind::BallNose, 0.5, 0.0);
        for (a, b) in t.data.iter().zip(&g.data) {
            assert!(a >= b, "tool tip must stay on or above the surface");
        }
    }
}
