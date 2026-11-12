//! The 3D view: draws the material, the model or the carve result as a shaded surface.

use crate::app::{App, Machined, Quality3d, MATERIALS};
use crate::relief::Grid;
use egui::epaint::{Mesh, Vertex};
use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    /// 0 looks straight down.
    pub tilt: f32,
    pub zoom: f32,
    pub pan: egui::Vec2,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::iso()
    }
}

impl Camera {
    pub fn iso() -> Self {
        Camera { yaw: (-26f32).to_radians(), tilt: 30f32.to_radians(), zoom: 1.0, pan: vec2(0.0, 0.0) }
    }
    pub fn look(yaw: f32, tilt: f32) -> Self {
        Camera { yaw: yaw.to_radians(), tilt: tilt.to_radians(), zoom: 1.0, pan: egui::Vec2::ZERO }
    }
}

/// How much of the 3D view the board takes when the view is fitted, along the side that limits it
/// (the original's 3D view: 642 of 953 pixels; estimated from that one picture).
const VIEW_FILL: f32 = 0.67;

/// Turns job positions into screen positions.
#[derive(Clone, Copy)]
pub struct Projection {
    centre: Pos2,
    mid: [f32; 2],
    sin: f32,
    cos: f32,
    tilt_sin: f32,
    tilt_cos: f32,
    scale: f32,
    distance: f32,
}

impl Projection {
    pub fn new(cam: Camera, rect: Rect, width: f64, height: f64) -> Self {
        let size = width.max(height) as f32;
        let mut p = Projection {
            centre: pos2(0.0, 0.0),
            mid: [width as f32 / 2.0, height as f32 / 2.0],
            sin: cam.yaw.sin(),
            cos: cam.yaw.cos(),
            tilt_sin: cam.tilt.sin(),
            tilt_cos: cam.tilt.cos(),
            scale: 1.0,
            distance: size * 3.5,
        };
        // The board as it is turned, fitted to the view and centred in it, as the original frames
        // its 3D view: the board's outline takes about two thirds of the view (642 of 953 pixels
        // high in its 3D view of a 10 x 10 inch job).
        let (w, h) = (width as f32, height as f32);
        let corners = [p.project(0.0, 0.0, 0.0), p.project(w, 0.0, 0.0), p.project(0.0, h, 0.0), p.project(w, h, 0.0)];
        let (lo, hi) = corners.iter().fold((pos2(f32::MAX, f32::MAX), pos2(f32::MIN, f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
        let (bw, bh) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
        p.scale = VIEW_FILL * (rect.width() / bw).min(rect.height() / bh) * cam.zoom;
        let middle = vec2((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0) * p.scale;
        p.centre = rect.center() + cam.pan - middle;
        p
    }

    pub fn project(&self, x: f32, y: f32, z: f32) -> Pos2 {
        let (dx, dy) = (x - self.mid[0], y - self.mid[1]);
        let vx = dx * self.cos - dy * self.sin;
        let vy = dx * self.sin + dy * self.cos;
        let up = vy * self.tilt_cos + z * self.tilt_sin;
        let depth = -vy * self.tilt_sin + z * self.tilt_cos;
        let k = self.distance / (self.distance - depth).max(self.distance * 0.1) * self.scale;
        pos2(self.centre.x + vx * k, self.centre.y - up * k)
    }

    /// The eye and the direction of the line of sight through a screen point, in job space.
    fn ray(&self, at: Pos2) -> ([f32; 3], [f32; 3]) {
        let (px, py) = ((at.x - self.centre.x) / self.scale, (self.centre.y - at.y) / self.scale);
        let world = |vx: f32, up: f32, depth: f32| {
            let vy = up * self.tilt_cos - depth * self.tilt_sin;
            let z = up * self.tilt_sin + depth * self.tilt_cos;
            [vx * self.cos + vy * self.sin, -vx * self.sin + vy * self.cos, z]
        };
        let eye = world(0.0, 0.0, self.distance);
        let dir = world(px, py, -self.distance);
        ([eye[0] + self.mid[0], eye[1] + self.mid[1], eye[2]], dir)
    }

    /// The job position on the material's top under a screen point.
    pub fn unproject(&self, p: Pos2) -> [f64; 2] {
        let mut k = self.scale;
        let (mut vx, mut vy) = (0.0, 0.0);
        for _ in 0..4 {
            vx = (p.x - self.centre.x) / k;
            vy = -(p.y - self.centre.y) / k / self.tilt_cos.max(0.05);
            let depth = -vy * self.tilt_sin;
            k = self.distance / (self.distance - depth).max(self.distance * 0.1) * self.scale;
        }
        let dx = vx * self.cos + vy * self.sin;
        let dy = -vx * self.sin + vy * self.cos;
        [(dx + self.mid[0]) as f64, (dy + self.mid[1]) as f64]
    }
}

/// What the finished picture depends on.
#[derive(Clone, PartialEq)]
struct ImageKey {
    rev: u64,
    cam: Camera,
    rect: Rect,
    pixels_per_point: f32,
    size: [f64; 3],
    material: Option<usize>,
    solid: [u8; 3],
    machined: Machined,
    fill: [u8; 3],
    colours: Vec<[u8; 3]>,
    fine: bool,
    quality: Quality3d,
    shadows: bool,
    light_follows: bool,
    plane: bool,
}

/// The last picture of the surface, and what was prepared to make it.
/// What a surface was made from, whether it is only the small copy, and whether the true surface
/// was left out for the lightest pictures.
type BakedFor = (u64, [f64; 3], bool, u32, bool, bool, Option<[u32; 3]>);
type WoodFor = (Option<usize>, [f64; 3]);

pub struct SurfaceCache {
    key: ImageKey,
    baked_for: BakedFor,
    baked: Arc<Baked>,
    wood_for: WoodFor,
    wood: Option<Arc<WoodTable>>,
    texture: egui::TextureHandle,
}

/// A picture being made away from the window, which keeps showing the last one until it is ready.
pub struct SurfaceJob {
    started: crate::platform::Instant,
    key: ImageKey,
    stop: Arc<AtomicBool>,
    pictures: Receiver<Finished>,
    /// In a browser, which has no threads: the picture made a slice at a time between frames.
    steps: Option<Box<Steps>>,
}

impl Drop for SurfaceJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Finished {
    key: ImageKey,
    baked_for: BakedFor,
    baked: Arc<Baked>,
    wood_for: WoodFor,
    wood: Option<Arc<WoodTable>>,
    picture: egui::ColorImage,
    seconds: f32,
}

/// Where the surface comes from.
enum Source {
    /// Already made.
    Ready(Arc<Baked>),
    /// The small copy of a carve, while the tool moves.
    Small(Grid),
    /// All of it.
    Whole(Box<WholeSource>),
}

enum WholeSource {
    Carved(crate::relief::Sim),
    Grid(Grid),
}

/// Everything needed to make a picture, owned, so it can go to another thread.
struct Order {
    key: ImageKey,
    baked_for: BakedFor,
    source: Source,
    top: f32,
    thick: f32,
    w: f32,
    h: f32,
    wood_for: WoodFor,
    wood: Option<Arc<WoodTable>>,
    /// Which toolpath cut each cell, only when the machined area is coloured.
    tags: Option<Vec<u8>>,
    proj: Projection,
    rect: Rect,
    solid: [f32; 3],
    fill: [f32; 3],
    palette: Vec<[f32; 3]>,
    machined: Machined,
    quick_pixels: f32,
    light: [f32; 3],
    /// The light that casts shadows, when they are on.
    shadow_light: Option<[f32; 3]>,
    /// The modeling plane's height, when it's drawn.
    plane: Option<f32>,
}

/// How far along a picture is.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// Small, while the view moves.
    Moving,
    /// Every pixel, from the grid alone: much quicker, and already free of the small copy's blur.
    First,
    /// As good as the chosen quality asks.
    Last,
}

/// Spots per side of a pixel on edges, in the first full-sized picture.
const FIRST_EDGE_SAMPLES: usize = 3;

impl Order {
    fn scene<'a>(&'a self, baked: &'a Baked, wood: Option<&'a WoodTable>, stage: Stage, stop: Option<&'a AtomicBool>) -> Scene<'a> {
        Scene {
            proj: self.proj,
            baked,
            wood,
            solid: self.solid,
            tags: self.tags.as_deref(),
            machined: self.machined,
            fill: self.fill,
            palette: self.palette.clone(),
            light: self.light,
            quick: stage != Stage::Last,
            stop,
            plane: self.plane,
        }
    }

    /// The picture's width and height, and the spots per side of a pixel, then on its edges.
    fn sizes(&self, baked: &Baked, stage: Stage) -> (usize, usize, usize, usize) {
        let (width, height, samples) = picture_size(self.rect, self.key.pixels_per_point, self.proj.scale, baked.cell, stage != Stage::Moving, self.quick_pixels);
        let (most, edges) = self.key.quality.samples();
        let (samples, refine) = match stage {
            Stage::Moving => (1, 1),
            Stage::First => (1, FIRST_EDGE_SAMPLES),
            Stage::Last => (samples.min(most), edges),
        };
        (width, height, samples, refine)
    }

    fn draw(&self, baked: &Baked, wood: Option<&WoodTable>, stage: Stage, stop: &AtomicBool) -> egui::ColorImage {
        let scene = self.scene(baked, wood, stage, Some(stop));
        let (width, height, samples, refine) = self.sizes(baked, stage);
        egui::ColorImage::new([width, height], scene.render(self.rect, width, height, samples, refine))
    }

    /// The stages a picture goes through: one small one while the view moves; otherwise something
    /// good to look at right away, then as good as the chosen quality asks.
    fn stages(&self) -> Vec<Stage> {
        if !self.key.fine {
            vec![Stage::Moving]
        } else if self.key.quality != Quality3d::Light {
            vec![Stage::First, Stage::Last]
        } else {
            vec![Stage::Last]
        }
    }

    /// The surface and the wood, made if they aren't already.
    fn prepare(&self) -> (Arc<Baked>, Option<Arc<WoodTable>>) {
        let wood = self.wood.clone().or_else(|| {
            self.wood_for.0.map(|m| {
                let (_, a, b) = MATERIALS[m.min(MATERIALS.len() - 1)];
                Arc::new(WoodTable::new(a, b, self.w, self.h, self.thick))
            })
        });
        let (top, thick, w, h) = (self.top, self.thick, self.w, self.h);
        let baked = match &self.source {
            Source::Ready(b) => b.clone(),
            Source::Small(g) => Arc::new(Baked::quick_lit(g, top, thick, w, h, self.shadow_light)),
            Source::Whole(whole) => {
                let baked = match whole.as_ref() {
                    WholeSource::Carved(s) => Baked::lit_by(&s.grid, Some(s), top, thick, w, h, self.shadow_light),
                    WholeSource::Grid(g) => Baked::lit_by(g, None, top, thick, w, h, self.shadow_light),
                };
                Arc::new(baked)
            }
        };
        (baked, wood)
    }

    /// Makes the picture, handing over each one as it is ready, unless it is no longer wanted.
    fn run(self, stop: &AtomicBool, hand_over: &mut dyn FnMut(Finished)) {
        let started = crate::platform::Instant::now();
        let (baked, wood) = self.prepare();
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if !self.key.fine {
            let picture = self.draw(&baked, wood.as_deref(), Stage::Moving, stop);
            if !stop.load(Ordering::Relaxed) {
                hand_over(Finished { key: self.key.clone(), baked_for: self.baked_for, baked, wood_for: self.wood_for, wood, picture, seconds: started.elapsed().as_secs_f32() });
            }
            return;
        }
        // Something good to look at right away; the rest of the time only cleans it up.
        if self.key.quality != Quality3d::Light {
            let picture = self.draw(&baked, wood.as_deref(), Stage::First, stop);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let key = ImageKey { fine: false, ..self.key.clone() };
            hand_over(Finished { key, baked_for: self.baked_for, baked: baked.clone(), wood_for: self.wood_for, wood: wood.clone(), picture, seconds: started.elapsed().as_secs_f32() });
        }
        let picture = self.draw(&baked, wood.as_deref(), Stage::Last, stop);
        if !stop.load(Ordering::Relaxed) {
            hand_over(Finished { key: self.key, baked_for: self.baked_for, baked, wood_for: self.wood_for, wood, picture, seconds: started.elapsed().as_secs_f32() });
        }
    }
}

/// How long a slice of a picture made between frames may take, so a page with no threads keeps
/// answering while it sharpens: 12 ms leaves room in a 60-frame second for drawing the rest.
/// Burin's choice.
pub const PICTURE_SLICE_MS: f32 = 12.0;

/// A picture made a few rows at a time, for a browser: there, nothing runs beside the page, so a
/// whole sharp picture at once would freeze it for seconds after every move.
pub struct Steps {
    order: Order,
    baked: Arc<Baked>,
    wood: Option<Arc<WoodTable>>,
    /// The stages still to make, first first.
    stages: Vec<Stage>,
    pass: Option<Pass>,
    send: Sender<Finished>,
    started: crate::platform::Instant,
}

/// One stage of a picture, part made.
struct Pass {
    stage: Stage,
    width: usize,
    height: usize,
    samples: usize,
    refine: usize,
    out: Vec<Color32>,
    /// The picture before its edges were looked at again, once that has begun.
    first: Option<Vec<Color32>>,
    row: usize,
}

impl Steps {
    fn new(order: Order, send: Sender<Finished>) -> Steps {
        let started = crate::platform::Instant::now();
        let (baked, wood) = order.prepare();
        let mut stages = order.stages();
        stages.reverse();
        Steps { order, baked, wood, stages, pass: None, send, started }
    }

    /// Goes on for about `ms` milliseconds. True once every stage is handed over.
    fn step(&mut self, ms: f32) -> bool {
        let began = crate::platform::Instant::now();
        loop {
            if self.pass.is_none() {
                let Some(stage) = self.stages.pop() else { return true };
                let (width, height, samples, refine) = self.order.sizes(&self.baked, stage);
                self.pass = Some(Pass { stage, width, height, samples, refine, out: vec![Color32::TRANSPARENT; width * height], first: None, row: 0 });
            }
            let pass = self.pass.as_mut().unwrap();
            let scene = self.order.scene(&self.baked, self.wood.as_deref(), pass.stage, None);
            let rect = self.order.rect;
            let (pw, ph) = (rect.width() / pass.width as f32, rect.height() / pass.height as f32);
            while pass.row < pass.height {
                let y = pass.row;
                let line = &mut pass.out[y * pass.width..(y + 1) * pass.width];
                match &pass.first {
                    None => scene.base_row(rect, pw, ph, y, line, pass.samples),
                    Some(first) => scene.refine_row(rect, pw, ph, pass.width, pass.height, first, y, line, pass.refine),
                }
                pass.row += 1;
                if began.elapsed().as_secs_f32() * 1000.0 >= ms {
                    break;
                }
            }
            if pass.row < pass.height {
                return false;
            }
            if pass.first.is_none() && pass.refine > pass.samples {
                pass.first = Some(pass.out.clone());
                pass.row = 0;
                continue;
            }
            let pass = self.pass.take().unwrap();
            // A first picture is handed over as not yet the sharp one, as when made on a thread.
            let fine = self.order.key.fine && pass.stage != Stage::First;
            let _ = self.send.send(Finished {
                key: ImageKey { fine, ..self.order.key.clone() },
                baked_for: self.order.baked_for,
                baked: self.baked.clone(),
                wood_for: self.order.wood_for,
                wood: self.wood.clone(),
                picture: egui::ColorImage::new([pass.width, pass.height], pass.out),
                seconds: self.started.elapsed().as_secs_f32(),
            });
            if began.elapsed().as_secs_f32() * 1000.0 >= ms {
                return self.stages.is_empty();
            }
        }
    }
}

/// The most pixels of the quick picture; fewer on a computer that can't keep up.
pub const MOST_QUICK_PIXELS: f32 = 1_200_000.0;
const FEWEST_QUICK_PIXELS: f32 = 60_000.0;
/// How long a quick picture may take, so that moving the view stays smooth.
const QUICK_PICTURE_MS: f32 = 22.0;

/// A soft, repeatable wobble for wood grain.
fn noise(v: f32) -> f32 {
    let (i, t) = (v.floor(), v.fract());
    let hash = |n: f32| ((n * 127.1).sin() * 43758.547).fract().abs();
    let s = t * t * (3.0 - 2.0 * t);
    hash(i) * (1.0 - s) + hash(i + 1.0) * s
}

fn wood(a: [u8; 3], b: [u8; 3], x: f32, y: f32) -> [f32; 3] {
    let drift = y + 0.08 * noise(x * 0.45) + 0.03 * noise(x * 1.7 + 9.0);
    let grain = 0.5 * noise(drift * 2.3) + 0.3 * noise(drift * 6.1 + 4.0) + 0.2 * noise(drift * 17.0 + 8.0);
    let t = grain.clamp(0.0, 1.0);
    std::array::from_fn(|i| a[i] as f32 * (1.0 - t) + b[i] as f32 * t)
}

const WHITE_UV: Pos2 = egui::epaint::WHITE_UV;

/// Grain runs through the wood: each level down shows a slightly different layer.
const GRAIN_PER_DEPTH: f32 = 1.2;

/// Main light, from the viewer's side so the walls they look at are lit, and a weak one
/// from the other side so nothing goes black.
const KEY_LIGHT: [f32; 3] = [-0.46, -0.38, 0.8];
const FILL_LIGHT: [f32; 3] = [0.5, 0.25, 0.45];

/// The original's Light Follows Viewer and Use Shadow Shading commands (`0x1408ac380`, `0x1408ad440`).
pub const LIGHT_FOLLOWS_VIEWER_COMMAND: u32 = 32889;
pub const SHADOW_SHADING_COMMAND: u32 = 33698;
/// How close to the modeling plane, in cells, a point of the surface counts as lying on it. The
/// rays find a flat surface only to about this much, so a whole job at 0 (a zero plane) would
/// otherwise come out striped.
const PLANE_REACH: f32 = 0.05;

/// Multi-Sided View (33839, `0x1402c6e90`): flips a flag of the job's sides (`0x140593b70`), offered
/// only when there are two sides (`0x1402c6f40`), and turned on by Preview All Sides at its end.
pub const MULTI_SIDED_COMMAND: u32 = 33839;

/// The original's Draw Modeling Plane command (`0x1402c6170`), and how see-through the plane is.
pub const MODELING_PLANE_COMMAND: u32 = 33188;
pub const MODELING_PLANE_OPACITY: f32 = 0.4;

/// Where the main light comes from. Fixed on the job, as the original starts; when it follows
/// the viewer, it turns with the view so it stays where it is at the starting view.
pub fn key_light(cam: &Camera, follows_viewer: bool) -> [f32; 3] {
    if !follows_viewer {
        return KEY_LIGHT;
    }
    let a = Camera::iso().yaw - cam.yaw;
    let (s, c) = a.sin_cos();
    [KEY_LIGHT[0] * c - KEY_LIGHT[1] * s, KEY_LIGHT[0] * s + KEY_LIGHT[1] * c, KEY_LIGHT[2]]
}

/// Brightness from a surface direction, how open the spot is and how much it is in shadow.
fn lit(n: [f32; 3], open: f32, shadow: f32, key: [f32; 3]) -> f32 {
    let dot = |l: [f32; 3]| (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0);
    let raw = |d: f32, f: f32, o: f32, s: f32| o * (0.34 + 0.22 * f) + 0.62 * d * (1.0 - 0.8 * s) * o.sqrt();
    // A flat, open top is the wood's own colour.
    let flat = raw(key[2], FILL_LIGHT[2], 1.0, 0.0);
    raw(dot(key), dot(FILL_LIGHT), open, shadow) / flat
}

/// Runs `f` over rows of `data` on all processor cores.
fn par_rows<T: Send>(data: &mut [T], row: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    // One core is left for the window, so it stays alive while a picture is being made.
    let threads = crate::platform::threads(true);
    let rows = (data.len() / row.max(1)).max(1);
    let per = rows.div_ceil(threads).max(1);
    crate::platform::scope(|s| {
        for (c, chunk) in data.chunks_mut(row * per).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (r, line) in chunk.chunks_mut(row).enumerate() {
                    f(c * per + r, line);
                }
            });
        }
    });
}

/// The average height within `r` cells of each cell.
fn box_blur(z: &[f32], nx: usize, ny: usize, r: usize) -> Vec<f32> {
    let line = |get: &dyn Fn(usize) -> f32, n: usize, out: &mut dyn FnMut(usize, f32)| {
        let mut sum = 0.0f64;
        for i in 0..=r.min(n - 1) {
            sum += get(i) as f64;
        }
        let mut count = r.min(n - 1) + 1;
        for i in 0..n {
            out(i, (sum / count as f64) as f32);
            if i + r + 1 < n {
                sum += get(i + r + 1) as f64;
                count += 1;
            }
            if i >= r {
                sum -= get(i - r) as f64;
                count -= 1;
            }
        }
    };
    let mut across = vec![0f32; z.len()];
    par_rows(&mut across, nx, |y, row| {
        line(&|i| z[y * nx + i], nx, &mut |i, v| row[i] = v);
    });
    let mut out = vec![0f32; z.len()];
    for x in 0..nx {
        line(&|i| across[i * nx + x], ny, &mut |i, v| out[i * nx + x] = v);
    }
    out
}

/// How wide, in cells, the two rings are that tell how hollow a spot is.
const NEAR_RING: usize = 6;
const FAR_RING: usize = 30;

/// How much the true surface may cost: the share of the surface it is needed on times the moves
/// each look-up goes through. Past this a picture would take many seconds, as for a whole 3D finish,
/// and the grid draws those parts. Upright walls show the most, so they get more.
const MOST_SHAPE_WORK: f32 = 8.0;
const MOST_WALL_WORK: f32 = 30.0;
/// Even a little of it is too slow when each look-up goes through this many moves.
const MOST_CROWDING: f32 = 150.0;

/// How many cells the soft part of the light is worked out on, however fine the surface is.
const SHADE_CELLS: usize = 1_000_000;

/// A smaller copy of a grid, each cell the average of those around its place.
fn shrunk(z: &[f32], nx: usize, ny: usize, k: usize, sx: usize, sy: usize) -> Vec<f32> {
    if k == 1 {
        return z.to_vec();
    }
    let mut out = vec![0f32; sx * sy];
    par_rows(&mut out, sx, |y, row| {
        let (y0, y1) = ((y * k).saturating_sub(k / 2), (y * k + k / 2).min(ny - 1));
        for (x, value) in row.iter_mut().enumerate() {
            let (x0, x1) = ((x * k).saturating_sub(k / 2), (x * k + k / 2).min(nx - 1));
            let mut sum = 0f64;
            for j in y0..=y1 {
                for v in &z[j * nx + x0..=j * nx + x1] {
                    sum += *v as f64;
                }
            }
            *value = (sum / ((y1 - y0 + 1) * (x1 - x0 + 1)) as f64) as f32;
        }
    });
    out
}

/// The surface ready for drawing: its heights, and what its lighting is worked out from.
pub struct Baked {
    nx: usize,
    ny: usize,
    cell: f32,
    w: f32,
    h: f32,
    z: Vec<f32>,
    /// Average height nearby and further out, and shadow, on a grid of its own that is never very
    /// large. Empty for a flat surface. All of these vary smoothly, so walls are shaded without stripes.
    shade: Vec<[f32; 3]>,
    shade_nx: usize,
    shade_ny: usize,
    shade_cell: f32,
    /// The cells where the true surface is worked out from the tool's moves, not read from the grid.
    walls: Vec<bool>,
    cuts: Option<crate::relief::CutIndex>,
    z_max: f32,
    floor: f32,
}

impl Baked {
    /// `top` is the height of the material's top in the grid; heights end up 0 there. `walls` marks
    /// the cells whose slope only stands for an upright wall.
    #[cfg(test)]
    pub fn new(g: &Grid, carved: Option<&crate::relief::Sim>, top: f32, thick: f32, w: f32, h: f32) -> Baked {
        Baked::lit_by(g, carved, top, thick, w, h, Some(KEY_LIGHT))
    }

    /// `shadow_light` is the light that casts shadows; none when shadows are off.
    pub fn lit_by(g: &Grid, carved: Option<&crate::relief::Sim>, top: f32, thick: f32, w: f32, h: f32, shadow_light: Option<[f32; 3]>) -> Baked {
        let (nx, ny, cell) = (g.nx, g.ny, g.cell as f32);
        let floor = -thick;
        let z: Vec<f32> = g.data.iter().map(|v| (v - top).max(floor)).collect();
        let cuts = carved.and_then(crate::relief::CutIndex::build);
        let walls = match (&cuts, carved) {
            (Some(c), Some(s)) => {
                // Worked out only where a picture stays quick; the grid does for the rest.
                let share = |kind: u8| s.walls.iter().filter(|w| **w == kind).count() as f32 / s.walls.len().max(1) as f32;
                let (walls_work, shapes_work) = (share(crate::relief::WALL) * c.crowding, share(crate::relief::SHAPED) * c.crowding);
                let walls = walls_work <= MOST_WALL_WORK && c.crowding <= MOST_CROWDING;
                let shapes = walls && walls_work + shapes_work <= MOST_SHAPE_WORK;
                s.walls.iter().map(|w| (walls && *w == crate::relief::WALL) || (shapes && *w == crate::relief::SHAPED)).collect()
            }
            _ => Vec::new(),
        };
        let z_max = z.iter().copied().fold(f32::MIN, f32::max);
        let z_min = z.iter().copied().fold(f32::MAX, f32::min);
        // Soft light doesn't need every cell: on a fine surface it comes from a smaller copy.
        let shrink = (((nx * ny) as f32 / SHADE_CELLS as f32).sqrt().ceil() as usize).max(1);
        let (sx, sy) = ((nx - 1) / shrink + 1, (ny - 1) / shrink + 1);
        let shade_cell = cell * shrink as f32;
        let mut shade = Vec::new();
        if z_max - z_min > 1e-6 {
            let small = shrunk(&z, nx, ny, shrink, sx, sy);
            let near = box_blur(&small, sx, sy, NEAR_RING);
            let far = box_blur(&small, sx, sy, FAR_RING);
            let light = shadow_light.unwrap_or(KEY_LIGHT);
            let flat_xy = light[0].hypot(light[1]);
            let (sun_x, sun_y, rise) = (light[0] / flat_xy, light[1] / flat_xy, light[2] / flat_xy);
            const REACH: [f32; 16] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 11.0, 15.0, 20.0, 27.0, 36.0, 48.0, 64.0, 85.0, 113.0, 150.0];
            let z_ref = &small;
            // Follow the light from each spot; anything higher than its beam casts a shadow.
            let mut shadows = vec![0f32; small.len()];
            par_rows(&mut shadows, sx, |y, row| {
                if shadow_light.is_none() {
                    return;
                }
                let at = |ix: i64, iy: i64| z_ref[iy.clamp(0, sy as i64 - 1) as usize * sx + ix.clamp(0, sx as i64 - 1) as usize];
                for (x, out) in row.iter_mut().enumerate() {
                    let here = z_ref[y * sx + x];
                    if here >= z_max - 1e-6 {
                        continue;
                    }
                    let mut shadow = 0f32;
                    for s in REACH {
                        let climb = rise * s * shade_cell;
                        if here + climb > z_max {
                            break;
                        }
                        let block = at(x as i64 + (sun_x * s).round() as i64, y as i64 + (sun_y * s).round() as i64);
                        let over = block - (here + climb);
                        if over > 0.0 {
                            shadow = shadow.max(over / (0.2 * s * shade_cell + shade_cell));
                        }
                    }
                    *out = shadow.min(1.0);
                }
            });
            let shadows = box_blur(&shadows, sx, sy, 2);
            shade = (0..small.len()).map(|i| [near[i], far[i], shadows[i]]).collect();
        }
        Baked { nx, ny, cell, w, h, z, shade, shade_nx: sx, shade_ny: sy, shade_cell, walls, cuts, z_max, floor }
    }

    /// A much smaller copy, quick enough to make afresh at every frame while the tool is moving.
    #[cfg(test)]
    pub fn quick(g: &Grid, top: f32, thick: f32, w: f32, h: f32) -> Baked {
        Baked::quick_lit(g, top, thick, w, h, Some(KEY_LIGHT))
    }

    pub fn quick_lit(g: &Grid, top: f32, thick: f32, w: f32, h: f32, shadow_light: Option<[f32; 3]>) -> Baked {
        let shrink = (((g.nx * g.ny) as f32 / SHADE_CELLS as f32).sqrt().ceil() as usize).max(1);
        if shrink == 1 {
            return Baked::lit_by(g, None, top, thick, w, h, shadow_light);
        }
        let (sx, sy) = ((g.nx - 1) / shrink + 1, (g.ny - 1) / shrink + 1);
        let small = Grid { nx: sx, ny: sy, cell: g.cell * shrink as f64, data: shrunk(&g.data, g.nx, g.ny, shrink, sx, sy) };
        Baked::lit_by(&small, None, top, thick, w, h, shadow_light)
    }

    /// Whether a wall runs through this square of the grid.
    fn near_wall(&self, i: usize) -> bool {
        !self.walls.is_empty() && (self.walls[i] || self.walls[i + 1] || self.walls[i + self.nx] || self.walls[i + self.nx + 1])
    }

    /// Which cell a position falls in, and how far across it.
    fn locate(&self, x: f32, y: f32) -> (usize, f32, f32) {
        let fx = (x / self.cell).clamp(0.0, (self.nx - 1) as f32);
        let fy = (y / self.cell).clamp(0.0, (self.ny - 1) as f32);
        let (ix, iy) = ((fx as usize).min(self.nx - 2), (fy as usize).min(self.ny - 2));
        (iy * self.nx + ix, fx - ix as f32, fy - iy as f32)
    }

    fn height(&self, x: f32, y: f32) -> f32 {
        let (i, ..) = self.locate(x, y);
        if let (Some(cuts), true) = (&self.cuts, self.near_wall(i)) {
            return cuts.surface(x, y).0.max(self.floor);
        }
        self.raw_height(x, y)
    }

    /// The height from the grid alone, as the slopes standing for walls really are.
    fn raw_height(&self, x: f32, y: f32) -> f32 {
        let (i, tx, ty) = self.locate(x, y);
        let v = &self.z;
        let low = v[i] * (1.0 - tx) + v[i + 1] * tx;
        let high = v[i + self.nx] * (1.0 - tx) + v[i + self.nx + 1] * tx;
        low * (1.0 - ty) + high * ty
    }

    /// The light at a point on the surface, from its true height rather than its cell's.
    #[cfg(test)]
    fn brightness(&self, x: f32, y: f32, z: f32) -> f32 {
        self.brightness_of(x, y, z, true, KEY_LIGHT)
    }

    /// `exact` is the true surface; without it, what the grid says, much quicker while things move.
    fn brightness_of(&self, x: f32, y: f32, z: f32, exact: bool, key: [f32; 3]) -> f32 {
        if self.shade.is_empty() {
            return 1.0;
        }
        let fx = (x / self.shade_cell).clamp(0.0, (self.shade_nx - 1) as f32);
        let fy = (y / self.shade_cell).clamp(0.0, (self.shade_ny - 1) as f32);
        let (ix, iy) = ((fx as usize).min(self.shade_nx.saturating_sub(2)), (fy as usize).min(self.shade_ny.saturating_sub(2)));
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let s = iy * self.shade_nx + ix;
        let (a, b, c, e) = (self.shade[s], self.shade[s + 1], self.shade[s + self.shade_nx], self.shade[s + self.shade_nx + 1]);
        let mix: [f32; 3] = std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + e[k] * tx) * ty);
        let [near, far, mut shadow] = mix;
        // Nothing can shade the very top: this only comes from the softening of what is beside it.
        if z >= self.z_max - 1e-4 {
            shadow = 0.0;
        }
        // The slope here, over a little more than a cell so that walls are shaded without stripes.
        let d = self.cell * 1.5;
        let (i, ..) = self.locate(x, y);
        let wall_cuts = self.cuts.as_ref().filter(|_| exact && self.near_wall(i));
        let (gx, gy) = match wall_cuts {
            Some(cuts) => {
                let (here, level) = cuts.surface(x, y);
                let close = (self.cell * 0.05).max((self.z_max - self.floor) / 400.0);
                if (z - here.max(self.floor)).abs() > close {
                    // On the wall itself, facing the tool that made it.
                    let facing = cuts.wall_facing(x, y, z).unwrap_or([0.0, 0.0]);
                    (-facing[0] * 30.0, -facing[1] * 30.0)
                } else if level {
                    (0.0, 0.0)
                } else {
                    // The shape of a pointed or round tool.
                    let e = self.cell * 0.25;
                    let slope = |after: f32, before: f32| ((after - before) / (2.0 * e)).clamp(-6.0, 6.0);
                    (slope(cuts.surface(x + e, y).0, cuts.surface(x - e, y).0), slope(cuts.surface(x, y + e).0, cuts.surface(x, y - e).0))
                }
            }
            None => {
                // Away from a wall, but perhaps within reach of one: its slope is no part of this surface.
                let beside = |px: f32, py: f32| {
                    let (q, ..) = self.locate(px, py);
                    (!exact || !self.near_wall(q)).then(|| self.raw_height(px, py))
                };
                let slope = |after: Option<f32>, before: Option<f32>| match (after, before) {
                    (Some(a), Some(b)) => (a - b) / (2.0 * d),
                    (Some(a), None) => (a - z) / d,
                    (None, Some(b)) => (z - b) / d,
                    (None, None) => 0.0,
                };
                (slope(beside(x + d, y), beside(x - d, y)), slope(beside(x, y + d), beside(x, y - d)))
            }
        };
        let len = (gx * gx + gy * gy + 1.0).sqrt();
        let hollow_near = (near - z).max(0.0) / (NEAR_RING as f32 * self.shade_cell);
        let hollow_far = (far - z).max(0.0) / (FAR_RING as f32 * self.shade_cell);
        let open = (1.0 / (1.0 + 0.9 * hollow_near + 1.4 * hollow_far)).max(0.35);
        lit([-gx / len, -gy / len, 1.0 / len], open, shadow, key).min(1.3)
    }

    /// Nothing is left here: the tool went right through.
    fn is_air(&self, height: f32) -> bool {
        height <= self.floor + (-self.floor * 1e-3).max(1e-5)
    }
}

/// Wood colours worked out ahead, since the grain is slow to compute at every pixel.
pub struct WoodTable {
    nx: usize,
    ny: usize,
    /// Job units per table step.
    step: f32,
    /// The Y of the table's first row; below 0 for the layers lower in the wood.
    y0: f32,
    rgb: Vec<[f32; 3]>,
}

impl WoodTable {
    fn new(a: [u8; 3], b: [u8; 3], w: f32, h: f32, thick: f32) -> WoodTable {
        let size = w.max(h);
        let step = size / 1024.0;
        let y0 = -thick * GRAIN_PER_DEPTH - step;
        let (nx, ny) = ((w / step).ceil() as usize + 2, (((h - y0) / step).ceil() as usize + 2).min(4096));
        let mut rgb = vec![[0f32; 3]; nx * ny];
        par_rows(&mut rgb, nx, |y, row| {
            for (x, out) in row.iter_mut().enumerate() {
                *out = wood(a, b, x as f32 * step / size * 10.0, (y0 + y as f32 * step) / size * 10.0);
            }
        });
        WoodTable { nx, ny, step, y0, rgb }
    }

    fn at(&self, x: f32, y: f32) -> [f32; 3] {
        let fx = (x / self.step).clamp(0.0, (self.nx - 1) as f32);
        let fy = ((y - self.y0) / self.step).clamp(0.0, (self.ny - 1) as f32);
        let (ix, iy) = ((fx as usize).min(self.nx - 2), (fy as usize).min(self.ny - 2));
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let i = iy * self.nx + ix;
        let (a, b, c, d) = (self.rgb[i], self.rgb[i + 1], self.rgb[i + self.nx], self.rgb[i + self.nx + 1]);
        std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty)
    }
}

/// Where a line of sight lands on the material.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    p: [f32; 3],
    /// Set on the outer sides of the material, which the height grid doesn't describe.
    side: Option<[f32; 3]>,
}

/// Everything needed to work out the colour of one spot.
struct Scene<'a> {
    proj: Projection,
    baked: &'a Baked,
    wood: Option<&'a WoodTable>,
    solid: [f32; 3],
    /// Which toolpath cut each cell, when the machined area is coloured.
    tags: Option<&'a [u8]>,
    machined: Machined,
    fill: [f32; 3],
    palette: Vec<[f32; 3]>,
    /// Where the main light comes from.
    light: [f32; 3],
    /// The quick picture: the grid alone, not the true surface.
    quick: bool,
    /// Set when the picture is no longer wanted.
    stop: Option<&'a AtomicBool>,
    /// The modeling plane's height, when it's drawn.
    plane: Option<f32>,
}

impl Scene<'_> {
    fn surface_height(&self, x: f32, y: f32) -> f32 {
        if self.quick {
            self.baked.raw_height(x, y)
        } else {
            self.baked.height(x, y)
        }
    }

    fn stopped(&self) -> bool {
        self.stop.is_some_and(|s| s.load(Ordering::Relaxed))
    }

    /// Follows the line of sight through a screen point until it meets the material.
    fn trace(&self, at: Pos2) -> Option<Hit> {
        let b = self.baked;
        let (o, d) = self.proj.ray(at);
        let (mut t0, mut t1) = (0f32, f32::MAX);
        let mut entry = 2;
        for (axis, lo, hi) in [(0, 0.0, b.w), (1, 0.0, b.h), (2, b.floor, b.z_max)] {
            if d[axis].abs() < 1e-9 {
                if o[axis] < lo || o[axis] > hi {
                    return None;
                }
                continue;
            }
            let (p, q) = ((lo - o[axis]) / d[axis], (hi - o[axis]) / d[axis]);
            let (near, far) = if p < q { (p, q) } else { (q, p) };
            if near > t0 {
                t0 = near;
                entry = axis;
            }
            t1 = t1.min(far);
            if t0 > t1 {
                return None;
            }
        }
        let point = |t: f32| [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        let solid_at = |p: [f32; 3]| {
            let h = self.surface_height(p[0], p[1]);
            p[2] <= h && !b.is_air(h)
        };
        let first = point(t0);
        if entry != 2 && solid_at(first) {
            let mut n = [0.0; 3];
            n[entry] = -d[entry].signum();
            return Some(Hit { p: first, side: Some(n) });
        }
        if solid_at(first) || (entry == 2 && first[2] <= self.surface_height(first[0], first[1]) + 1e-6 && !b.is_air(self.surface_height(first[0], first[1]))) {
            return Some(Hit { p: first, side: None });
        }
        // Steps of under a cell across, and never skipping the whole depth when looking straight down.
        let across = d[0].hypot(d[1]).max(1e-9);
        let range = (b.z_max - b.floor).max(b.cell);
        let step = (0.7 * b.cell / across).min(range / (3.0 * d[2].abs().max(1e-9)));
        let mut before = t0;
        let mut t = t0;
        while t < t1 {
            t = (t + step).min(t1);
            if solid_at(point(t)) {
                let (mut lo, mut hi) = (before, t);
                for _ in 0..9 {
                    let mid = (lo + hi) / 2.0;
                    if solid_at(point(mid)) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(Hit { p: point(hi), side: None });
            }
            before = t;
        }
        None
    }

    fn colour(&self, hit: Hit) -> [f32; 3] {
        let [x, y, z] = hit.p;
        let mut base = match self.wood {
            Some(table) => table.at(x, y + z * GRAIN_PER_DEPTH),
            None => self.solid,
        };
        if let (Some(tags), true) = (self.tags, z < -1e-3 && self.machined != Machined::Material && hit.side.is_none()) {
            let b = self.baked;
            let i = ((x / b.cell).round() as usize).min(b.nx - 1);
            let j = ((y / b.cell).round() as usize).min(b.ny - 1);
            let tag = tags.get(j * b.nx + i).copied().unwrap_or(255) as usize;
            base = match (self.machined, self.palette.get(tag)) {
                (Machined::Toolpath, Some(c)) => *c,
                _ => self.fill,
            };
        }
        let light = match hit.side {
            Some(n) => lit(n, 0.85, 0.0, self.light),
            None => self.baked.brightness_of(x, y, z, !self.quick, self.light),
        };
        let shaded = base.map(|v| (v * light).min(255.0));
        // The modeling plane, white and see-through (`0x140301fe6`: 0.4, colour `0xffffff`), shows where
        // nothing of the model stands above it.
        match self.plane {
            Some(at) if hit.side.is_none() && (z - at).abs() <= self.baked.cell * PLANE_REACH + 1e-5 => shaded.map(|v| v * (1.0 - MODELING_PLANE_OPACITY) + 255.0 * MODELING_PLANE_OPACITY),
            _ => shaded,
        }
    }

    /// The colour of one pixel, averaged over `samples` x `samples` spots.
    fn pixel(&self, rect: Rect, pw: f32, ph: f32, x: usize, y: usize, samples: usize) -> Color32 {
        let share = 1.0 / (samples * samples) as f32;
        let mut sum = [0f32; 3];
        let mut cover = 0f32;
        for j in 0..samples {
            for i in 0..samples {
                let at = pos2(
                    rect.left() + (x as f32 + (i as f32 + 0.5) / samples as f32) * pw,
                    rect.top() + (y as f32 + (j as f32 + 0.5) / samples as f32) * ph,
                );
                if let Some(hit) = self.trace(at) {
                    let c = self.colour(hit);
                    sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
                    cover += 1.0;
                }
            }
        }
        if cover > 0.0 {
            Color32::from_rgba_premultiplied((sum[0] * share) as u8, (sum[1] * share) as u8, (sum[2] * share) as u8, (cover * share * 255.0) as u8)
        } else {
            Color32::TRANSPARENT
        }
    }

    /// Draws at `samples` spots a side, then looks again much closer wherever colours jump
    /// from one pixel to the next: that is where edges would look like stairs.
    fn render(&self, rect: Rect, width: usize, height: usize, samples: usize, refine: usize) -> Vec<Color32> {
        let mut out = vec![Color32::TRANSPARENT; width * height];
        let (pw, ph) = (rect.width() / width as f32, rect.height() / height as f32);
        par_rows(&mut out, width, |y, row| {
            if self.stopped() {
                return;
            }
            self.base_row(rect, pw, ph, y, row, samples);
        });
        if refine <= samples || self.stopped() {
            return out;
        }
        let first = out.clone();
        let first_ref = &first;
        par_rows(&mut out, width, |y, row| {
            if self.stopped() {
                return;
            }
            self.refine_row(rect, pw, ph, width, height, first_ref, y, row, refine);
        });
        out
    }

    /// One row of the picture at `samples` spots a side.
    fn base_row(&self, rect: Rect, pw: f32, ph: f32, y: usize, row: &mut [Color32], samples: usize) {
        for (x, pixel) in row.iter_mut().enumerate() {
            *pixel = self.pixel(rect, pw, ph, x, y, samples);
        }
    }

    /// One row looked at again, closer, wherever its colour jumps from a neighbour's in `first`.
    #[allow(clippy::too_many_arguments)]
    fn refine_row(&self, rect: Rect, pw: f32, ph: f32, width: usize, height: usize, first: &[Color32], y: usize, row: &mut [Color32], refine: usize) {
        let jump = |a: Color32, b: Color32| {
            let d = |p: u8, q: u8| (p as i32 - q as i32).abs();
            d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b()) + d(a.a(), b.a()) > 14
        };
        for (x, pixel) in row.iter_mut().enumerate() {
            let here = first[y * width + x];
            let mut edge = false;
            for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                    edge |= jump(here, first[ny * width + nx]);
                }
            }
            if edge {
                *pixel = self.pixel(rect, pw, ph, x, y, refine);
            }
        }
    }
}

/// Spots per side of a pixel on edges, in the sharpest picture.
#[cfg(test)]
const EDGE_SAMPLES: usize = 6;

/// The finished picture in a browser: at most this many pixels and samples. Burin's choice.
const WEB_FINE_PIXELS: f32 = 1_500_000.0;
const WEB_FINE_SAMPLES: usize = 2_000_000;

/// Picture size and samples per pixel: quick while things move, sharp once they stop.
fn picture_size(rect: Rect, pixels_per_point: f32, scale: f32, cell: f32, fine: bool, quick_pixels: f32) -> (usize, usize, usize) {
    // A browser works it out on one core: fewer samples, and no more pixels than this.
    let fine_samples: usize = if crate::platform::WEB { WEB_FINE_SAMPLES } else { 8_000_000 };
    let (mut w, mut h) = ((rect.width() * pixels_per_point).max(1.0), (rect.height() * pixels_per_point).max(1.0));
    if fine && crate::platform::WEB && w * h > WEB_FINE_PIXELS {
        let shrink = (WEB_FINE_PIXELS / (w * h)).sqrt();
        w *= shrink;
        h *= shrink;
    }
    if !fine {
        let shrink = (quick_pixels / (w * h)).sqrt().min(1.0);
        w *= shrink;
        h *= shrink;
        return ((w as usize).max(1), (h as usize).max(1), 1);
    }
    let (w, h) = (w.round() as usize, h.round() as usize);
    // Fine enough to land on every cell of the simulation, so thin grooves don't vanish.
    let cells_per_pixel = 1.0 / (pixels_per_point * scale * cell).max(1e-6);
    let mut samples = (cells_per_pixel.ceil() as usize).clamp(1, 3);
    while samples > 1 && w * h * samples * samples > fine_samples {
        samples -= 1;
    }
    (w, h, samples)
}

impl App {
    /// Wood in the carve preview; the job's own appearance while modelling.
    fn shown_material(&self) -> Option<usize> {
        if self.sim.is_some() || matches!(self.right, crate::app::RightMode::Preview) {
            self.doc.job.preview_material
        } else {
            self.doc.job.material
        }
    }

    /// Whether the picture on the screen is the sharp one.
    #[cfg(test)]
    pub fn mesh_is_sharp(&self) -> bool {
        self.mesh.as_ref().is_some_and(|m| m.key.fine)
    }

    /// What the surface should be made from now.
    /// The light that casts shadows, when shadow shading is on. While it follows the viewer, the
    /// shadows are worked out again for each view, so they come in a step behind the turning.
    fn shadow_light(&self) -> Option<[f32; 3]> {
        self.shadow_shading.then(|| key_light(&self.cam, self.light_follows_viewer))
    }

    fn wanted_baked(&self, key: &ImageKey) -> (BakedFor, f32) {
        let carved = self.sim.is_some();
        let model = if carved { None } else { self.model.as_ref() };
        let top = model.map_or(0.0, |g| self.doc.machine.model_top(self.doc.job.thickness, g.max() as f64) as f32);
        // While the tool is moving there's a new surface at every frame: a smaller one will do.
        let small = self.anim.playing && carved;
        let shadows = self.shadow_light().map(|l| l.map(f32::to_bits));
        ((self.surface_rev, key.size, carved, top.to_bits(), small, key.quality == Quality3d::Light, shadows), top)
    }

    /// Everything a picture needs, with the surface and wood kept if they haven't changed.
    fn order(&self, proj: &Projection, rect: Rect, key: ImageKey) -> Order {
        let job = &self.doc.job;
        let (baked_for, top) = self.wanted_baked(&key);
        let wood_for = (self.shown_material(), key.size);
        let old = self.mesh.as_ref();
        let source = match old.filter(|c| c.baked_for == baked_for) {
            Some(c) => Source::Ready(c.baked.clone()),
            None => match (&self.sim, &self.model) {
                (Some(s), _) if baked_for.4 => Source::Small(s.grid.clone()),
                (Some(s), _) if baked_for.5 => Source::Whole(Box::new(WholeSource::Grid(s.grid.clone()))),
                (Some(s), _) => Source::Whole(Box::new(WholeSource::Carved(s.clone()))),
                (None, Some(g)) => Source::Whole(Box::new(WholeSource::Grid(g.clone()))),
                (None, None) => Source::Whole(Box::new(WholeSource::Grid(Grid::new(job.width, job.height, 16, 0.0)))),
            },
        };
        Order {
            baked_for,
            source,
            top,
            thick: job.thickness as f32,
            w: job.width as f32,
            h: job.height as f32,
            wood_for,
            wood: old.filter(|c| c.wood_for == wood_for).and_then(|c| c.wood.clone()),
            // Only when it shows: on a fine carve this is many millions of cells.
            tags: (self.sim.is_some() && self.machined != Machined::Material).then(|| self.sim_tag.clone()),
            proj: *proj,
            rect,
            solid: job.solid.map(|v| v as f32),
            fill: self.global_fill.map(|v| v as f32),
            palette: self.doc.toolpaths.iter().map(|t| t.color.map(|v| v as f32)).collect(),
            machined: self.machined,
            quick_pixels: self.quick_pixels,
            light: key_light(&self.cam, self.light_follows_viewer),
            shadow_light: self.shadow_light(),
            // The plane lies at the model's zero; over a carving there's no model to show it against.
            plane: (self.draw_modeling_plane && self.sim.is_none() && self.model.is_some()).then_some(-top),
            key,
        }
    }

    fn keep_picture(&mut self, ctx: &egui::Context, done: Finished) {
        if done.key.fine {
            self.picture_seconds = Some(done.seconds);
            if let Some(started) = self.preview_started.filter(|_| !self.anim.playing) {
                self.preview_seconds = Some(started.elapsed().as_secs_f32());
                self.preview_started = None;
            }
        }
        let old = self.mesh.take();
        let texture = match old {
            Some(mut c) => {
                c.texture.set(done.picture, egui::TextureOptions::LINEAR);
                c.texture
            }
            None => ctx.load_texture("surface", done.picture, egui::TextureOptions::LINEAR),
        };
        self.mesh = Some(SurfaceCache { key: done.key, baked_for: done.baked_for, baked: done.baked, wood_for: done.wood_for, wood: done.wood, texture });
    }

    /// Takes in the picture made away from the window, if it's ready.
    fn poll_surface(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.surface_job else { return };
        if let Some(steps) = job.steps.as_mut() {
            if steps.step(PICTURE_SLICE_MS) {
                // Done: the last picture is in the channel, and nothing more will come.
                job.steps = None;
            } else {
                ctx.request_repaint();
            }
        }
        let job = &*job;
        let mut ended = false;
        let mut latest = None;
        loop {
            match job.pictures.try_recv() {
                Ok(done) => latest = Some(done),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.surface_job = None;
        }
        if let Some(done) = latest {
            self.keep_picture(ctx, done);
        }
    }

    /// Moving the view: a picture right now from the surface already made, small enough to keep up.
    fn quick_picture(&mut self, ctx: &egui::Context, proj: &Projection, rect: Rect, key: ImageKey) {
        self.surface_job = None;
        let started = crate::platform::Instant::now();
        let mut order = self.order(proj, rect, key);
        if let Some(c) = &self.mesh {
            // Even if it's still only the small copy: the sharp one comes when the view stops.
            order.source = Source::Ready(c.baked.clone());
            order.baked_for = c.baked_for;
        }
        let stop = AtomicBool::new(false);
        let mut got = None;
        order.run(&stop, &mut |done| got = Some(done));
        if let Some(done) = got {
            self.keep_picture(ctx, done);
        }
        // Next time as big as can be done in time.
        let took = started.elapsed().as_secs_f32() * 1000.0;
        if self.surface_in_background {
            self.quick_pixels = (self.quick_pixels * (QUICK_PICTURE_MS / took.max(1.0)).clamp(0.6, 1.25)).clamp(FEWEST_QUICK_PIXELS, MOST_QUICK_PIXELS);
        }
    }

    /// Orders a picture, away from the window unless the tests want it at once.
    fn order_surface(&mut self, ctx: &egui::Context, proj: &Projection, rect: Rect, key: ImageKey) {
        if let Some(job) = &self.surface_job {
            // Already on its way, or the tool is cutting: the latest wins when the last is done.
            if job.key == key || self.anim.playing {
                return;
            }
        }
        let cutting = self.anim.playing && self.surface_in_background;
        if cutting && self.next_cut_picture.is_some_and(|t| crate::platform::Instant::now() < t) {
            return;
        }
        self.surface_job = None;
        let before = crate::platform::Instant::now();
        let order = self.order(proj, rect, key.clone());
        // Copying a big carve is done on the window's time, so it may only take a share of it.
        self.next_cut_picture = cutting.then(|| crate::platform::Instant::now() + before.elapsed() * 4);
        let stop = Arc::new(AtomicBool::new(false));
        if !self.surface_in_background {
            let mut last = None;
            order.run(&stop, &mut |done| last = Some(done));
            if let Some(done) = last {
                self.keep_picture(ctx, done);
            }
            return;
        }
        let (send, pictures) = channel();
        if self.surface_sliced {
            let steps = Steps::new(order, send);
            self.surface_job = Some(SurfaceJob { started: crate::platform::Instant::now(), key, stop, pictures, steps: Some(Box::new(steps)) });
            ctx.request_repaint();
            return;
        }
        let (worker_stop, repaint) = (stop.clone(), ctx.clone());
        crate::platform::spawn(move || {
            order.run(&worker_stop, &mut |done| {
                let _ = send.send(done);
                repaint.request_repaint();
            });
            repaint.request_repaint();
        });
        self.surface_job = Some(SurfaceJob { started: crate::platform::Instant::now(), key, stop, pictures, steps: None });
    }

    fn gradient_background(painter: &egui::Painter, rect: Rect) {
        let mut mesh = Mesh::default();
        let (top, bottom) = (crate::theme::BACKDROP_TOP(), crate::theme::BACKDROP_BOTTOM());
        for (p, c) in [
            (rect.left_top(), top),
            (rect.right_top(), top),
            (rect.right_bottom(), bottom),
            (rect.left_bottom(), bottom),
        ] {
            mesh.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: c });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(Shape::mesh(Arc::new(mesh)));
    }

    /// What the picture must show now.
    fn image_key(&self, rect: Rect, pixels_per_point: f32, fine: bool) -> ImageKey {
        let job = &self.doc.job;
        ImageKey {
            rev: self.surface_rev,
            cam: self.cam,
            rect,
            pixels_per_point,
            size: [job.width, job.height, job.thickness],
            material: self.shown_material(),
            solid: job.solid,
            machined: self.machined,
            fill: self.global_fill,
            colours: self.doc.toolpaths.iter().map(|t| t.color).collect(),
            fine,
            quality: self.quality3d,
            shadows: self.shadow_shading,
            light_follows: self.light_follows_viewer,
            plane: self.draw_modeling_plane,
        }
    }

    pub fn view_3d(&mut self, ui: &mut egui::Ui, rect: Rect) {
        self.refresh_model();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if self.shaded_background {
            Self::gradient_background(&painter, rect);
        } else {
            // Plain, the original paints both ends with the top colour (`0x1408ad390`).
            painter.rect_filled(rect, 0.0, crate::theme::BACKDROP_TOP());
        }

        // While sculpting, the left button sculpts, unless the tool is Twiddle or Alt is held.
        let alt = ui.input(|i| i.modifiers.alt);
        let sculpting = self.sculpt.as_ref().is_some_and(|s| s.tool != crate::sculpt::SculptTool::Twiddle) && !alt;
        if sculpting {
            let proj = Projection::new(self.cam, rect, self.doc.job.width, self.doc.job.height);
            if response.dragged_by(egui::PointerButton::Primary) || response.drag_started_by(egui::PointerButton::Primary) || response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let first = response.drag_started_by(egui::PointerButton::Primary) || response.clicked();
                    let erase = ui.input(|i| i.modifiers.shift);
                    self.sculpt_at(proj.unproject(p), first, erase);
                }
            }
            // The brush, a red ring on the material's top under the pointer.
            if let (Some(p), Some(s)) = (response.hover_pos(), self.sculpt.as_ref()) {
                let c = proj.unproject(p);
                let r = s.diameter / 2.0;
                let ring: Vec<Pos2> = (0..=48).map(|k| {
                    let a = k as f64 / 48.0 * std::f64::consts::TAU;
                    proj.project((c[0] + r * a.cos()) as f32, (c[1] + r * a.sin()) as f32, 0.0)
                }).collect();
                ui.painter().add(Shape::line(ring, Stroke::new(1.5, crate::theme::ERROR)));
            }
            // The wheel changes the brush's size, finer with Ctrl; with Shift, its strength.
            if response.hovered() {
                let (scroll, ctrl, shift) = ui.input(|i| (i.smooth_scroll_delta.y + i.smooth_scroll_delta.x, i.modifiers.command, i.modifiers.shift));
                if scroll != 0.0 {
                    if let Some(s) = self.sculpt.as_mut() {
                        if shift {
                            s.strength = (s.strength * (1.0 + scroll as f64 * 0.002)).clamp(0.01, 1.0);
                        } else {
                            let k = if ctrl { 0.0005 } else { 0.002 };
                            s.diameter = (s.diameter * (1.0 + scroll as f64 * k)).max(1e-3);
                        }
                    }
                }
            }
        } else if response.dragged_by(egui::PointerButton::Primary) {
            let d = response.drag_delta();
            self.cam.yaw -= d.x * 0.008;
            self.cam.tilt = (self.cam.tilt - d.y * 0.008).clamp(0.0, 88f32.to_radians());
        }
        if response.dragged_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Middle) {
            self.cam.pan += response.drag_delta();
        }
        if response.hovered() && !sculpting {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.zoom = (self.cam.zoom * (scroll * 0.002).exp()).clamp(0.2, 20.0);
            }
        }

        let job = self.doc.job.clone();
        let proj = Projection::new(self.cam, rect, job.width, job.height);
        if response.double_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let at = proj.unproject(p);
                let removed = self.sim.as_mut().is_some_and(|sim| sim.remove_waste(at[0], at[1]));
                if removed {
                    self.bump_surface();
                    self.status = crate::i18n::tr("Removed the loose piece").into();
                } else {
                    self.status = crate::i18n::tr("Only pieces that are cut free can be removed.").into();
                }
            }
        }

        // Quick while the view moves or the tool cuts, then one sharp picture.
        let moving = response.dragged() || ui.input(|i| i.smooth_scroll_delta != egui::Vec2::ZERO) || self.anim.playing;
        let key = self.image_key(rect, ui.ctx().pixels_per_point(), !moving);
        self.poll_surface(ui.ctx());
        if self.mesh.as_ref().is_none_or(|m| m.key != key) {
            // Only the view moved, and there's a surface: a quick picture at once. All else takes
            // its time away from the window, which goes on showing the last picture meanwhile.
            let (wanted, _) = self.wanted_baked(&key);
            let same_surface = self.mesh.as_ref().is_some_and(|m| m.baked_for.0 == wanted.0 && m.baked_for.1 == wanted.1 && m.baked_for.2 == wanted.2 && m.baked_for.3 == wanted.3 && m.baked_for.5 == wanted.5);
            if moving && !self.anim.playing && same_surface && self.surface_in_background {
                self.quick_picture(ui.ctx(), &proj, rect, key);
            } else {
                self.order_surface(ui.ctx(), &proj, rect, key);
            }
        }
        if self.surface_job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        }
        if moving {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
        if let Some(cache) = &self.mesh {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(cache.texture.id(), rect, whole, Color32::WHITE);
        }
        // Saying so once it's been a moment, so a slow computer doesn't look stuck.
        if let Some(job) = self.surface_job.as_ref().filter(|j| j.key.fine && !self.anim.playing && j.started.elapsed().as_secs_f32() > 0.5) {
            let seconds = job.started.elapsed().as_secs_f32() as u32;
            painter.text(rect.left_bottom() + vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, tf!("Sharpening the picture... {seconds} s"), egui::FontId::proportional(12.0), crate::theme::MUTED());
        }

        // The grid lies on the top of the material.
        if self.grid {
            let (w, h) = (job.width, job.height);
            let step = crate::canvas::nice_step(w.max(h) / 12.0);
            let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(50, 34, 18, 110));
            let flat = |x: f64, y: f64| proj.project(x as f32, y as f32, 0.002);
            let mut x = 0.0;
            while x <= w + 1e-9 {
                painter.line_segment([flat(x, 0.0), flat(x, h)], stroke);
                x += step;
            }
            let mut y = 0.0;
            while y <= h + 1e-9 {
                painter.line_segment([flat(0.0, y), flat(w, y)], stroke);
                y += step;
            }
        }

        // The material's outline, and where the job's zero is (View menu).
        if self.draw_block {
            let (w, h, t) = (job.width as f32, job.height as f32, job.thickness as f32);
            let edge = Stroke::new(1.0, Color32::from_rgba_unmultiplied(230, 230, 235, 170));
            let corner = |x: f32, y: f32, z: f32| proj.project(x, y, z);
            for z in [0.0, -t] {
                for (a, b) in [((0.0, 0.0), (w, 0.0)), ((w, 0.0), (w, h)), ((w, h), (0.0, h)), ((0.0, h), (0.0, 0.0))] {
                    painter.line_segment([corner(a.0, a.1, z), corner(b.0, b.1, z)], edge);
                }
            }
            for (x, y) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
                painter.line_segment([corner(x, y, 0.0), corner(x, y, -t)], edge);
            }
        }
        if self.draw_origin {
            let o = self.doc.job.origin();
            let (ox, oy) = (o[0] as f32, o[1] as f32);
            let long = (job.width.max(job.height) * 0.12) as f32;
            let at = proj.project(ox, oy, 0.0);
            for (end, colour) in [((ox + long, oy, 0.0), Color32::from_rgb(230, 70, 60)), ((ox, oy + long, 0.0), Color32::from_rgb(80, 200, 90)), ((ox, oy, long), Color32::from_rgb(80, 140, 240))] {
                painter.line_segment([at, proj.project(end.0, end.1, end.2)], Stroke::new(2.0, colour));
            }
        }

        // Cutting paths: blue where cutting, red where travelling, cyan going down, green coming up, purple across.
        if self.show_toolpaths {
            let view = rect.expand(12.0);
            // With the Multi-Sided View on, the other side's toolpaths show too, where they really
            // cut: turned over the way the job flips, coming up from under the block.
            let both = self.multi_sided && self.doc.job.two_sided;
            let (w, h, t, around_y) = (self.doc.job.width, self.doc.job.height, self.doc.job.thickness, self.doc.job.flip_around_y);
            for tp in self.doc.toolpaths.iter().filter(|t| t.visible && (both || self.doc.toolpath_on_side(t))) {
                let turned = both && !self.doc.toolpath_on_side(tp);
                let place = |p: [f64; 3]| -> [f64; 3] {
                    if !turned {
                        p
                    } else if around_y {
                        [w - p[0], p[1], -t - p[2]]
                    } else {
                        [p[0], h - p[1], -t - p[2]]
                    }
                };
                let mut line: Vec<Pos2> = Vec::new();
                let mut fast = crate::toolpath::MoveKind::Cut;
                let mut last: Option<Pos2> = None;
                let mut last_at: Option<[f64; 3]> = None;
                let shown = [true, self.draw_rapids, self.draw_plunges, self.draw_retracts, self.draw_joins];
                let flush = |points: &mut Vec<Pos2>, kind: crate::toolpath::MoveKind| {
                    if points.len() >= 2 && shown[kind as usize] {
                        let colour = match kind {
                            crate::toolpath::MoveKind::Cut => crate::theme::CUT,
                            crate::toolpath::MoveKind::Rapid => crate::theme::RAPID,
                            crate::toolpath::MoveKind::Plunge => crate::theme::PLUNGE,
                            crate::toolpath::MoveKind::Retract => crate::theme::RETRACT,
                            crate::toolpath::MoveKind::Join => crate::theme::JOIN,
                        };
                        painter.add(Shape::line(std::mem::take(points), Stroke::new(1.0, colour)));
                    } else {
                        points.clear();
                    }
                };
                for m in crate::toolpath::working_moves(&tp.moves) {
                    let at = place(m.p);
                    let q = proj.project(at[0] as f32, at[1] as f32, at[2] as f32);
                    let kind = last_at.map_or(crate::toolpath::MoveKind::Cut, |from| crate::toolpath::move_kind(from, m));
                    last_at = Some(m.p);
                    let Some(p) = last else {
                        last = Some(q);
                        continue;
                    };
                    // What can't be seen isn't drawn: moves too small to show, and those off the screen.
                    if kind == fast && (q - p).length() < crate::canvas::MOST_UNSEEN_PIXELS {
                        continue;
                    }
                    last = Some(q);
                    if kind != fast {
                        flush(&mut line, fast);
                        fast = kind;
                    }
                    if crate::canvas::off_to_one_side(p, q, view) {
                        flush(&mut line, fast);
                        continue;
                    }
                    if line.is_empty() {
                        line.push(p);
                    }
                    line.push(q);
                }
                flush(&mut line, fast);
            }
        }

        if self.draw_tool {
            if let Some((p, index)) = self.anim.tool_at {
                if let Some(tp) = self.doc.toolpaths.get(index) {
                    let (x, y, z) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    let long = job.width.max(job.height) as f32 * 0.14;
                    let px = proj.scale * tp.op.tool.dia as f32;
                    let (tip, neck, top) = (proj.project(x, y, z), proj.project(x, y, z + long * 0.5), proj.project(x, y, z + long));
                    painter.line_segment([tip, neck], Stroke::new(px.max(3.0), Color32::from_rgb(214, 180, 70)));
                    painter.line_segment([neck, top], Stroke::new(px.max(3.0) * 1.3, Color32::from_rgb(150, 150, 160)));
                }
            }
        }

        // View direction buttons, top right.
        use crate::icons::Icon;
        let buttons = [
            (Icon::ViewFit, crate::i18n::tr("Fit view"), None),
            (Icon::ViewIso, crate::i18n::tr("Angled view"), Some(Camera::iso())),
            (Icon::ViewZ, crate::i18n::tr("View from above"), Some(Camera::look(0.0, 0.0))),
            (Icon::ViewX, crate::i18n::tr("View along X"), Some(Camera::look(-90.0, 85.0))),
            (Icon::ViewY, crate::i18n::tr("View along Y"), Some(Camera::look(0.0, 85.0))),
        ];
        for (i, (icon, tip, cam)) in buttons.into_iter().enumerate() {
            let r = Rect::from_min_size(rect.right_top() + vec2(-133.0 + i as f32 * 25.0, 6.0), vec2(24.0, 24.0));
            let id = ui.scope_id().with(("view3d", i));
            let resp = ui.interact(r, id, Sense::click()).on_hover_text(tip);
            crate::icons::paint(ui.painter(), r, icon, true);
            crate::ui::label_widget(&resp, tip);
            if resp.clicked() {
                match cam {
                    Some(c) => self.cam = c,
                    None => {
                        self.cam.zoom = 1.0;
                        self.cam.pan = egui::Vec2::ZERO;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unproject_undoes_project_on_the_top_surface() {
        let rect = Rect::from_min_size(pos2(100.0, 50.0), vec2(1300.0, 950.0));
        let proj = Projection::new(Camera::iso(), rect, 10.0, 10.0);
        for (x, y) in [(0.0, 0.0), (5.0, 5.0), (9.0, 2.0), (3.0, 8.0)] {
            let back = proj.unproject(proj.project(x, y, 0.0));
            assert!((back[0] - x as f64).abs() < 0.02 && (back[1] - y as f64).abs() < 0.02, "{x},{y} -> {back:?}");
        }
    }

    const VIEW: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(700.0, 700.0) };

    /// A 10 by 10 job, half an inch thick, with `cut` setting the height of each cell.
    fn surface(points: usize, cut: impl Fn(f64, f64) -> Option<f32>) -> Baked {
        let mut g = Grid::new(10.0, 10.0, points, 0.0);
        for y in 0..g.ny {
            for x in 0..g.nx {
                if let Some(z) = cut(x as f64 * g.cell, y as f64 * g.cell) {
                    g.data[y * g.nx + x] = z;
                }
            }
        }
        Baked::new(&g, None, 0.0, 0.5, 10.0, 10.0)
    }

    fn scene(baked: &Baked) -> Scene<'_> {
        Scene {
            proj: Projection::new(Camera::iso(), VIEW, 10.0, 10.0),
            baked,
            wood: None,
            solid: [200.0, 200.0, 200.0],
            tags: None,
            machined: Machined::Material,
            fill: [255.0, 255.0, 255.0],
            palette: Vec::new(),
            light: KEY_LIGHT,
            quick: false,
            stop: None,
            plane: None,
        }
    }

    fn inside(x: f64, y: f64, lo: f64, hi: f64) -> bool {
        (lo..hi).contains(&x) && (lo..hi).contains(&y)
    }

    #[test]
    fn the_modeling_plane_tints_only_where_the_model_is_empty() {
        // A raised square in the middle of an empty model.
        let baked = surface(40_000, |x, y| inside(x, y, 4.0, 6.0).then_some(0.2));
        let mut s = scene(&baked);
        let empty = Hit { p: [2.0, 2.0, 0.0], side: None };
        let raised = Hit { p: [5.0, 5.0, 0.2], side: None };
        let (plain_empty, plain_raised) = (s.colour(empty), s.colour(raised));
        s.plane = Some(0.0);
        let (tinted, still) = (s.colour(empty), s.colour(raised));
        let whiter = plain_empty[0] + (255.0 - plain_empty[0]) * MODELING_PLANE_OPACITY;
        assert!((tinted[0] - whiter).abs() < 0.5, "{plain_empty:?} -> {tinted:?}");
        assert_eq!(still, plain_raised, "the model above it isn't covered");
    }

    #[test]
    fn a_line_of_sight_lands_where_the_screen_point_was_projected_from() {
        let baked = surface(40_000, |_, _| None);
        let s = scene(&baked);
        for (x, y) in [(5.0, 5.0), (1.0, 9.0), (9.0, 1.0), (0.3, 0.3)] {
            let hit = s.trace(s.proj.project(x, y, 0.0)).expect("on the board");
            assert!((hit.p[0] - x).abs() < 0.02 && (hit.p[1] - y).abs() < 0.02, "{x},{y} -> {hit:?}");
            assert!(hit.p[2].abs() < 1e-3 && hit.side.is_none());
        }
    }

    #[test]
    fn beside_the_board_there_is_nothing() {
        let baked = surface(40_000, |_, _| None);
        assert!(scene(&baked).trace(pos2(5.0, 5.0)).is_none());
    }

    #[test]
    fn the_outer_sides_of_the_board_are_seen_and_are_darker_than_its_top() {
        let baked = surface(40_000, |_, _| None);
        let s = scene(&baked);
        let hit = s.trace(s.proj.project(5.0, 0.0, -0.25)).expect("the near side");
        assert!(hit.side.is_some(), "{hit:?}");
        assert!((hit.p[2] + 0.25).abs() < 0.01 && hit.p[1].abs() < 0.01, "{hit:?}");
        let top = s.colour(s.trace(s.proj.project(5.0, 5.0, 0.0)).unwrap());
        assert!(s.colour(hit)[0] < top[0], "side {:?} top {top:?}", s.colour(hit));
    }

    #[test]
    fn a_cut_right_through_shows_what_is_behind_the_board() {
        let baked = surface(250_000, |x, y| inside(x, y, 4.0, 6.0).then_some(-0.5));
        let s = scene(&baked);
        // Through the middle of the hole, the near wall doesn't reach this far up the sight line.
        assert!(s.trace(s.proj.project(5.0, 5.2, -0.5)).is_none());
        assert!(s.trace(s.proj.project(2.0, 2.0, 0.0)).is_some());
    }

    #[test]
    fn a_pocket_floor_is_found_at_its_depth() {
        let baked = surface(250_000, |x, y| inside(x, y, 3.0, 7.0).then_some(-0.2));
        let s = scene(&baked);
        let hit = s.trace(s.proj.project(5.0, 5.0, -0.2)).expect("the floor");
        assert!((hit.p[2] + 0.2).abs() < 1e-3 && (hit.p[0] - 5.0).abs() < 0.02, "{hit:?}");
    }

    #[test]
    fn a_nearer_wall_hides_what_is_behind_it() {
        // A pocket: its floor just behind the side closest to the viewer is hidden by that side.
        let baked = surface(250_000, |x, y| inside(x, y, 3.0, 7.0).then_some(-0.4));
        let s = scene(&baked);
        let behind = s.proj.project(5.0, 3.02, -0.4);
        let hit = s.trace(behind).expect("something is in the way");
        assert!(hit.p[2] > -0.39, "the wall or the rim comes first: {hit:?}");
        // And walls show: part of the sight lines stop between top and floor.
        let far_wall = s.trace(s.proj.project(5.0, 7.0, -0.2)).expect("the far wall");
        assert!(far_wall.p[2] < -0.02 && far_wall.p[2] > -0.38, "{far_wall:?}");
    }

    fn brightness_at(baked: &Baked, x: f32, y: f32) -> f32 {
        baked.brightness(x, y, baked.height(x, y))
    }

    #[test]
    fn the_open_flat_top_keeps_the_wood_colour() {
        let baked = surface(250_000, |x, y| inside(x, y, 4.0, 6.0).then_some(-0.2));
        assert!((brightness_at(&baked, 1.0, 1.0) - 1.0).abs() < 1e-3, "{}", brightness_at(&baked, 1.0, 1.0));
    }

    #[test]
    fn deep_corners_are_darker_than_open_floors() {
        let baked = surface(250_000, |x, y| inside(x, y, 2.0, 8.0).then_some(-0.4));
        let middle = brightness_at(&baked, 5.0, 5.0);
        let corner = brightness_at(&baked, 2.1, 7.9);
        assert!(corner < middle * 0.8, "corner {corner} middle {middle}");
    }

    #[test]
    fn the_wall_facing_the_light_is_brighter_than_the_one_turned_away() {
        let baked = surface(250_000, |x, y| inside(x, y, 3.0, 7.0).then_some(-0.2));
        // The light comes from low X, so it falls on the wall at high X, which faces it.
        let lit_wall = brightness_at(&baked, 7.0, 5.0);
        let dark_wall = brightness_at(&baked, 3.0, 5.0);
        assert!(lit_wall > dark_wall * 1.3, "lit {lit_wall} dark {dark_wall}");
    }

    #[test]
    fn a_wall_casts_its_shadow_away_from_the_light() {
        let baked = surface(250_000, |x, y| inside(x, y, 2.0, 8.0).then_some(-0.4));
        // Just inside the wall nearest the light, compared to the same distance from the far one.
        let shaded = brightness_at(&baked, 2.1, 5.0);
        let sunny = brightness_at(&baked, 7.9, 5.0);
        assert!(shaded < sunny * 0.8, "shaded {shaded} sunny {sunny}");
    }

    #[test]
    fn without_shadow_shading_a_wall_casts_no_shadow() {
        let mut g = Grid::new(10.0, 10.0, 250_000, 0.0);
        for y in 0..g.ny {
            for x in 0..g.nx {
                if inside(x as f64 * g.cell, y as f64 * g.cell, 2.0, 8.0) {
                    g.data[y * g.nx + x] = -0.4;
                }
            }
        }
        let plain = Baked::lit_by(&g, None, 0.0, 0.5, 10.0, 10.0, None);
        assert!(plain.shade.iter().all(|s| s[2] == 0.0), "no shadow at all");
        let shaded = brightness_at(&plain, 2.1, 5.0);
        let sunny = brightness_at(&plain, 7.9, 5.0);
        // With shadows, this side is under 0.8 times the other; without, it isn't darker at all.
        assert!(shaded >= sunny * 0.95, "shaded {shaded} sunny {sunny}");
    }

    #[test]
    fn the_light_stays_on_the_job_unless_it_follows_the_viewer() {
        let turned = Camera { yaw: Camera::iso().yaw + std::f32::consts::FRAC_PI_2, ..Camera::iso() };
        assert_eq!(key_light(&turned, false), KEY_LIGHT);
        let start = key_light(&Camera::iso(), true);
        assert!(start.iter().zip(KEY_LIGHT).all(|(a, b)| (a - b).abs() < 1e-6), "the same at the starting view");
        let follows = key_light(&turned, true);
        // A quarter turn of the view takes the light a quarter turn the other way around the job.
        assert!((follows[0] - KEY_LIGHT[1]).abs() < 1e-5 && (follows[1] + KEY_LIGHT[0]).abs() < 1e-5, "{follows:?}");
        assert_eq!(follows[2], KEY_LIGHT[2]);
    }

    #[test]
    fn a_groove_one_cell_wide_still_shows_in_the_sharp_picture() {
        let cell = Grid::new(10.0, 10.0, 1_000_000, 0.0).cell;
        let baked = surface(1_000_000, |x, _| ((x - 5.01).abs() < cell * 0.6).then_some(-0.1));
        let s = scene(&baked);
        let (w, h, samples) = picture_size(VIEW, 1.0, s.proj.scale, baked.cell, true, MOST_QUICK_PIXELS);
        assert!(samples >= 2, "{samples}");
        let picture = s.render(VIEW, w, h, samples, EDGE_SAMPLES);
        // Along the screen row through the middle of the job: the groove is the darkest spot.
        let middle = s.proj.project(5.0, 5.0, 0.0);
        let y = middle.y as usize;
        let row = &picture[y * w..(y + 1) * w];
        let plain = row[middle.x as usize + 60].r();
        let groove = row[middle.x as usize - 30..middle.x as usize + 30].iter().map(|p| p.r()).min().unwrap();
        assert!(groove + 6 < plain, "groove {groove} plain {plain}");
    }

    #[test]
    fn the_sharp_picture_softens_the_board_edge_and_the_quick_one_does_not() {
        let baked = surface(40_000, |_, _| None);
        let s = scene(&baked);
        let partial = |p: &Vec<Color32>| p.iter().filter(|c| c.a() > 0 && c.a() < 255).count();
        let sharp = s.render(VIEW, 200, 200, 1, EDGE_SAMPLES);
        assert!(partial(&sharp) > 20);
        let levels: std::collections::HashSet<u8> = sharp.iter().map(|c| c.a()).collect();
        assert!(levels.len() > 8, "edges fade in many steps, not a few: {}", levels.len());
        let quick = s.render(VIEW, 200, 200, 1, 1);
        assert_eq!(partial(&quick), 0);
        assert!(quick.iter().any(|c| c.a() == 0) && quick.iter().any(|c| c.a() == 255), "board and background");
    }

    #[test]
    fn picture_sizes_stay_within_their_limits() {
        let big = Rect::from_min_size(pos2(0.0, 0.0), vec2(2500.0, 1500.0));
        let (w, h, samples) = picture_size(big, 1.0, 50.0, 0.01, false, MOST_QUICK_PIXELS);
        assert_eq!(samples, 1);
        assert!(w * h <= 1_210_000, "{w} {h}");
        let (w, h, samples) = picture_size(big, 1.0, 50.0, 0.01, true, MOST_QUICK_PIXELS);
        assert_eq!((w, h), (2500, 1500));
        assert!(w * h * samples * samples <= 8_000_000, "{samples}");
        let (_, _, samples) = picture_size(VIEW, 1.0, 35.0, 0.01, true, MOST_QUICK_PIXELS);
        assert_eq!(samples, 3, "coarse views look closer so thin cuts are not skipped");
        let (_, _, samples) = picture_size(VIEW, 1.0, 200.0, 0.01, true, MOST_QUICK_PIXELS);
        assert_eq!(samples, 1, "zoomed in, one look per pixel is enough away from edges");
        let (w, h, _) = picture_size(VIEW, 2.0, 35.0, 0.01, true, MOST_QUICK_PIXELS);
        assert_eq!((w, h), (1400, 1400), "follows the screen's real pixels");
    }

    /// A carve of the sample project.
    fn carved_sample() -> App {
        let mut a = App::default();
        a.load_sample();
        let all: Vec<usize> = (0..a.doc.toolpaths.len()).collect();
        a.preview(all);
        a.message = None;
        a.finish_preview();
        a
    }

    #[test]
    fn a_good_picture_comes_first_unless_the_light_one_is_all_there_is() {
        let mut a = carved_sample();
        let job = a.doc.job.clone();
        let proj = Projection::new(a.cam, VIEW, job.width, job.height);
        let handed = |a: &App| {
            let mut stages = Vec::new();
            let mut surfaces = 0;
            a.order(&proj, VIEW, a.image_key(VIEW, 1.0, true)).run(&AtomicBool::new(false), &mut |done| {
                stages.push(done.key.fine);
                surfaces += usize::from(done.baked.cuts.is_some());
                assert_eq!((done.picture.size[0], done.picture.size[1]), (VIEW.width() as usize, VIEW.height() as usize), "never the small picture");
            });
            (stages, surfaces)
        };
        for q in [Quality3d::Balanced, Quality3d::Sharp] {
            a.quality3d = q;
            assert_eq!(handed(&a), (vec![false, true], 2), "{q:?}");
        }
        a.quality3d = Quality3d::Light;
        assert_eq!(handed(&a), (vec![true], 0), "the light picture needs nothing more than the grid");
    }

    #[test]
    fn the_quality_decides_how_much_each_picture_looks_at() {
        assert_eq!(Quality3d::Sharp.samples(), (3, EDGE_SAMPLES), "sharp is the picture there was before");
        assert!(Quality3d::Light.samples().0 == 1 && Quality3d::Balanced.samples().1 < EDGE_SAMPLES);
        let mut a = App::default();
        a.load_sample();
        a.doc.toolpaths[0].op.tool.dia = 0.001;
        for (q, most) in [(Quality3d::Light, 4_000_000), (Quality3d::Balanced, 8_000_000), (Quality3d::Sharp, 16_000_000)] {
            a.quality3d = q;
            assert_eq!(a.sim_points(), most);
        }
    }

    #[test]
    fn a_round_hole_has_a_smooth_edge_on_screen() {
        // Cut as the simulation does, then seen from straight above at a high zoom.
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 1_000_000);
        let cutter = crate::relief::Cutter::new(crate::relief::ToolKind::EndMill, 0.25, 0.0, sim.grid.cell);
        let pts: Vec<[f64; 2]> = (0..=1440).map(|i| {
            let a = i as f64 / 1440.0 * std::f64::consts::TAU;
            [5.0 + 0.5 * a.cos(), 5.0 + 0.5 * a.sin()]
        }).collect();
        for w in pts.windows(2) {
            sim.cut_move(&cutter, &mut [], [w[0][0], w[0][1], -0.5], [w[1][0], w[1][1], -0.5], 0);
        }
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        let mut s = scene(&baked);
        let cam = Camera { zoom: 7.0, ..Camera::look(0.0, 0.0) };
        s.proj = Projection::new(cam, VIEW, 10.0, 10.0);
        let picture = s.render(VIEW, 700, 700, 1, EDGE_SAMPLES);
        // Across the outer edge, along many directions: where the picture crosses half cover.
        let centre = s.proj.project(5.0, 5.0, 0.0);
        let px_per_unit = s.proj.project(6.0, 5.0, 0.0).x - centre.x;
        let spots: Vec<f32> = (0..360).map(|i| {
            let a = i as f32 / 360.0 * std::f32::consts::TAU;
            let mut last = 0.0;
            for k in 0..200 {
                let r = 0.56 * px_per_unit + k as f32 * 0.25;
                let (x, y) = (centre.x + r * a.cos(), centre.y + r * a.sin());
                let c = picture[y as usize * 700 + x as usize];
                last = r;
                // Past the dark hole and back on the bright top.
                if c.a() > 200 && c.r() > 150 {
                    break;
                }
            }
            last
        }).collect();
        // Lighting moves this line gently around the circle; whole cells would make it jump.
        let cell_px = baked.cell * px_per_unit;
        let worst = (0..360).map(|i| (spots[i] - spots[(i + 1) % 360]).abs()).fold(0.0, f32::max);
        assert!(worst < cell_px * 0.8, "the edge jumps by {worst} pixels, a cell is {cell_px}");
    }

    #[test]
    fn walls_are_shaded_without_stripes_around_a_curve() {
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 1_000_000);
        let cutter = crate::relief::Cutter::new(crate::relief::ToolKind::EndMill, 0.25, 0.0, sim.grid.cell);
        let pts: Vec<[f64; 2]> = (0..=2880).map(|i| {
            let a = i as f64 / 2880.0 * std::f64::consts::TAU;
            [5.0 + 2.0 * a.cos(), 5.0 + 2.0 * a.sin()]
        }).collect();
        for w in pts.windows(2) {
            sim.cut_move(&cutter, &mut [], [w[0][0], w[0][1], -0.5], [w[1][0], w[1][1], -0.5], 0);
        }
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        // Halfway down the outer wall, step by small steps around the part facing the light.
        let at = |deg: f64| {
            let a = deg.to_radians();
            let (x, y) = ((5.0 + 2.125 * a.cos()) as f32, (5.0 + 2.125 * a.sin()) as f32);
            baked.brightness(x, y, -0.25)
        };
        let mut worst = 0f32;
        let mut deg = 20.0;
        while deg < 70.0 {
            worst = worst.max((at(deg) - at(deg + 0.1)).abs());
            deg += 0.1;
        }
        assert!(worst < 0.02, "brightness jumps by {worst} between neighbouring spots");
    }

    #[test]
    fn the_machined_area_can_take_the_toolpath_colour() {
        let baked = surface(40_000, |x, y| inside(x, y, 3.0, 7.0).then_some(-0.2));
        let tags = vec![0u8; baked.nx * baked.ny];
        let mut s = scene(&baked);
        s.tags = Some(&tags);
        s.machined = Machined::Toolpath;
        s.palette = vec![[220.0, 40.0, 40.0]];
        let floor = s.colour(s.trace(s.proj.project(5.0, 5.0, -0.2)).unwrap());
        assert!(floor[0] > floor[1] * 2.0, "{floor:?}");
        let top = s.colour(s.trace(s.proj.project(1.0, 1.0, 0.0)).unwrap());
        assert!((top[0] - top[1]).abs() < 1.0, "the uncut top keeps its colour: {top:?}");
    }

    #[test]
    fn default_camera_matches_the_angle_in_the_pictures() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1360.0, 950.0));
        let p = Projection::new(Camera::iso(), rect, 10.0, 10.0);
        let (o, x, y) = (p.project(0.0, 0.0, 0.0), p.project(10.0, 0.0, 0.0), p.project(0.0, 10.0, 0.0));
        assert!(x.x > o.x && x.y > o.y, "the X edge runs right and down");
        assert!(y.x > o.x && y.y < o.y, "the Y edge runs right and up");
    }

    /// A square cut with its walls along the grid, at a place that falls between the cells.
    fn cut_pocket(kind: crate::relief::ToolKind, dia: f64, angle: f64, depth: f64) -> crate::relief::Sim {
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 250_000);
        let cutter = crate::relief::Cutter::new(kind, dia, angle, sim.grid.cell);
        // Rows that land exactly on y = 5, where the tests look, so that no scallop is in the way.
        let mut y = 3.0;
        while y < 7.0 {
            sim.cut_move(&cutter, &mut [], [3.037, y, -depth], [7.037, y, -depth], 0);
            y += 0.125;
        }
        sim
    }

    #[test]
    fn walls_stand_upright_and_stay_where_the_tool_left_them() {
        use crate::relief::ToolKind;
        let sim = cut_pocket(ToolKind::EndMill, 0.5, 0.0, 0.2);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        let cell = baked.cell;
        // The tool's edge along the left: it stopped at 3.037, its radius from there, between two cells.
        let wall = 3.037 - 0.25;
        let height = |x: f32| baked.height(x, 5.0);
        // Half a cell either side is already flat, not a slope reaching a cell and more each way.
        assert!(height(wall - 0.5 * cell).abs() < 1e-4, "the top: {}", height(wall - 0.5 * cell));
        assert!((height(wall + 0.5 * cell) + 0.2).abs() < 1e-4, "the floor: {}", height(wall + 0.5 * cell));
        assert!(height(wall - 0.05 * cell) > -0.1 && height(wall + 0.05 * cell) < -0.1, "the wall is at the tool's edge");
        // And the corner between them isn't rounded: at most a third of a cell is on the wall.
        let on_wall = (0..300).filter(|k| {
            let h = height(wall - cell + *k as f32 / 150.0 * cell);
            h < -0.002 && h > -0.198
        }).count();
        assert!(on_wall < 50, "{on_wall} of 150 spots a cell");
    }

    #[test]
    fn the_flat_beside_a_wall_is_lit_like_any_other_flat() {
        use crate::relief::ToolKind;
        let sim = cut_pocket(ToolKind::EndMill, 0.5, 0.0, 0.2);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        let wall = 3.037 - 0.25;
        let (beside, far) = (baked.brightness(wall - 0.6 * baked.cell, 5.0, 0.0), baked.brightness(1.0, 5.0, 0.0));
        assert!((beside - far).abs() < 0.03, "the top's edge isn't shaded round: {beside} {far}");
        let on_wall = baked.brightness(wall, 5.0, -0.1);
        assert!((on_wall - far).abs() > 0.05, "while the wall looks like a wall: {on_wall} {far}");
    }

    #[test]
    fn the_real_slope_of_a_tool_stays_a_slope() {
        use crate::relief::ToolKind;
        // A groove from a 90 degree V-bit, not as deep as its cone: its sides are true slopes, not walls.
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 250_000);
        let cutter = crate::relief::Cutter::new(ToolKind::VBit, 1.0, 90.0, sim.grid.cell);
        sim.cut_move(&cutter, &mut [], [2.0, 5.03, -0.2], [8.0, 5.03, -0.2], 0);
        assert!(!sim.walls.iter().any(|w| *w == crate::relief::WALL), "nothing here stands for a wall");
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        // Coming down its side the height is exactly the tool's, however finely it is looked at.
        for k in 0..2000 {
            let across = 0.0001 + k as f32 * 0.0000995;
            let h = baked.height(5.0, 5.03 + across);
            assert!((h + 0.2 - across).abs() < 0.0006, "{across}: {h}");
        }
        // And it ends sharply at the top, where the point no longer reaches.
        assert_eq!(baked.height(5.0, 5.03 + 0.201), 0.0);
        assert!((baked.height(5.0, 5.03 + 0.199) + 0.001).abs() < 0.0006);
    }

    #[test]
    fn a_ball_nose_groove_has_the_tools_true_curve() {
        use crate::relief::{Cutter, Sim, ToolKind};
        let mut sim = Sim::new(100.0, 100.0, 13.0, 250_000);
        let ball = Cutter::new(ToolKind::BallNose, 8.0, 0.0, sim.grid.cell);
        sim.cut_move(&ball, &mut [], [20.0, 50.03, -2.0], [80.0, 50.03, -2.0], 0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        // A circle of radius 4 with its lowest point at -2, in fine steps much smaller than a cell.
        for k in 0..600 {
            let off = k as f32 / 600.0 * 3.4;
            let want = (-2.0 + 4.0 - (16.0 - off * off).sqrt()).min(0.0);
            let h = baked.height(50.0, 50.03 + off);
            assert!((h - want).abs() < 0.01, "{off}: {h}, not {want}");
        }
    }

    #[test]
    fn a_pointed_tool_only_counts_as_far_as_it_reaches() {
        use crate::relief::{Cutter, CutIndex, Sim, ToolKind};
        // A wide V-bit barely touching: moves far from a point are not gone through for it.
        let mut sim = Sim::new(100.0, 100.0, 13.0, 250_000);
        let bit = Cutter::new(ToolKind::VBit, 60.0, 90.0, sim.grid.cell);
        for k in 0..50 {
            let y = 10.0 + k as f64 * 1.5;
            sim.cut_move(&bit, &mut [], [20.0, y, -0.2], [80.0, y, -0.2], 0);
        }
        let index = CutIndex::build(&sim).unwrap();
        assert!(index.crowding < 30.0, "{}", index.crowding);
    }

    #[test]
    fn a_surface_too_crowded_with_moves_is_drawn_from_the_grid_and_stays_quick() {
        use crate::relief::{Cutter, Sim, ToolKind, WALL};
        // A thick raster of a round tool over a small sheet, beside an upright cut.
        let mut sim = Sim::new(20.0, 20.0, 13.0, 250_000);
        let ball = Cutter::new(ToolKind::BallNose, 8.0, 0.0, sim.grid.cell);
        let mill = Cutter::new(ToolKind::EndMill, 2.0, 0.0, sim.grid.cell);
        for k in 0..6000 {
            let t = k as f64 / 6000.0;
            let (x, y) = (2.0 + 16.0 * t, 2.0 + 16.0 * (t * 60.0).sin().abs());
            sim.cut_move(&ball, &mut [], [x, y, -1.0], [x + 0.01, y, -1.0], 0);
        }
        sim.cut_move(&mill, &mut [], [16.03, 3.0, -5.0], [16.03, 18.0, -5.0], 1);
        assert!(sim.walls.iter().any(|w| *w == WALL));
        let index = crate::relief::CutIndex::build(&sim).unwrap();
        assert!(index.crowding > MOST_CROWDING, "{}", index.crowding);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 20.0, 20.0);
        assert!(!baked.walls.iter().any(|w| *w), "nothing is looked up");
        // The picture is still right, as far as the grid goes.
        assert!(baked.height(16.03, 12.0) < -4.9);
    }

    #[test]
    fn a_wall_beside_a_light_cut_is_still_exact_when_a_shape_costs_too_much() {
        use crate::relief::{Cutter, Sim, ToolKind};
        // Round cuts over much of the sheet, not very crowded, beside a wall.
        let mut sim = Sim::new(100.0, 100.0, 13.0, 250_000);
        let ball = Cutter::new(ToolKind::BallNose, 10.0, 0.0, sim.grid.cell);
        let mill = Cutter::new(ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        for row in 0..40 {
            let y = 5.0 + row as f64 * 2.0;
            for k in 0..80 {
                let x = 5.0 + k as f64;
                sim.cut_move(&ball, &mut [], [x, y, -2.0], [x + 1.0, y, -2.0], 0);
            }
        }
        sim.cut_move(&mill, &mut [], [50.03, 92.0, -5.0], [90.03, 92.0, -5.0], 1);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let shaped = (0..sim.walls.len()).filter(|i| sim.walls[*i] == crate::relief::SHAPED);
        assert!(shaped.clone().count() > 1000 && shaped.clone().all(|i| !baked.walls[i]), "these come from the grid");
        assert_eq!(baked.height(70.0, 93.53), 0.0);
        assert!((baked.height(70.0, 93.47) + 5.0).abs() < 1e-3, "{}", baked.height(70.0, 93.47));
    }

    #[test]
    fn a_model_keeps_its_gentle_slopes_exactly() {
        let baked = surface(40_000, |x, _| Some(-0.02 * x as f32));
        let at = baked.height(5.03, 5.0);
        assert!((at + 0.02 * 5.03).abs() < 1e-3, "{at}");
    }

    #[test]
    fn a_fine_surface_keeps_its_detail_but_a_small_lighting_grid() {
        use crate::relief::ToolKind;
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 4_000_000);
        let cutter = crate::relief::Cutter::new(ToolKind::EndMill, 0.25, 0.0, sim.grid.cell);
        sim.cut_move(&cutter, &mut [], [5.0, 5.0, -0.5], [5.0, 5.0, -0.5], 0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 0.5, 10.0, 10.0);
        assert_eq!(baked.nx * baked.ny, sim.grid.data.len());
        assert!(baked.shade.len() <= SHADE_CELLS + SHADE_CELLS / 10, "{}", baked.shade.len());
        assert!(baked.brightness(5.0, 5.0, -0.5) < baked.brightness(1.0, 1.0, 0.0), "the hole is still dark");
        // The quick copy is about the same picture, only coarser.
        let quick = Baked::quick(&sim.grid, 0.0, 0.5, 10.0, 10.0);
        assert!(quick.nx * quick.ny <= SHADE_CELLS + SHADE_CELLS / 10);
        assert!(quick.height(5.0, 5.0) < -0.3 && quick.height(1.0, 1.0) == 0.0);
    }

    #[test]
    fn shrinking_averages_and_keeps_the_places() {
        let z: Vec<f32> = (0..81).map(|i| (i % 9) as f32).collect();
        let small = shrunk(&z, 9, 9, 2, 5, 5);
        assert_eq!(small.len(), 25);
        // Along X it rises as before, a cell of the copy being two of the original.
        assert!((small[2] - 4.0).abs() < 1e-5, "{small:?}");
        assert!((small[3] - 6.0).abs() < 1e-5, "{small:?}");
    }

    #[test]
    fn the_flat_all_around_a_hole_is_lit_evenly_with_no_collar() {
        use crate::relief::ToolKind;
        // Drilled like a real hole: one plunge, deep, on a fine grid.
        let mut sim = crate::relief::Sim::new(100.0, 100.0, 13.0, 4_000_000);
        let cutter = crate::relief::Cutter::new(ToolKind::EndMill, 3.175, 0.0, sim.grid.cell);
        sim.cut_move(&cutter, &mut [], [50.013, 50.007, -10.0], [50.013, 50.007, -10.0], 0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let far = baked.brightness(20.0, 20.0, 0.0);
        // Just outside the wall, all the way around, at distances from a fraction of a cell to several.
        let cell = baked.cell;
        for k in 0..720 {
            let a = k as f32 / 720.0 * std::f32::consts::TAU;
            for away in [0.2, 0.5, 1.0, 1.5, 2.0, 3.0] {
                let r = 3.175 / 2.0 + away * cell;
                let (x, y) = (50.013 + r * a.cos(), 50.007 + r * a.sin());
                assert_eq!(baked.height(x, y), 0.0, "still the top at {away} cells");
                let b = baked.brightness(x, y, 0.0);
                assert!((b - far).abs() < 0.02, "{b} against {far}, {away} cells off at {k}");
            }
        }
    }

    /// A straight wall running at an angle, its true place known at every point.
    fn slanted_cut(depth: f32) -> (crate::relief::Sim, [f32; 2], [f32; 2]) {
        let mut sim = crate::relief::Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let cutter = crate::relief::Cutter::new(crate::relief::ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        let (a, b) = ([20.03f32, 31.07f32], [80.11f32, 57.41f32]);
        sim.cut_move(&cutter, &mut [], [a[0] as f64, a[1] as f64, depth as f64], [b[0] as f64, b[1] as f64, depth as f64], 0);
        (sim, a, b)
    }

    #[test]
    fn a_wall_is_exactly_where_the_tool_passed_at_any_zoom() {
        let (sim, a, b) = slanted_cut(-5.0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let (dir, side) = ([(b[0] - a[0]) / len, (b[1] - a[1]) / len], [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]);
        // A hundredth of a cell either side of the edge, all along it: no slope and no stairs.
        let gap = baked.cell * 0.01;
        for k in 0..2000 {
            let s = 5.0 + k as f32 / 2000.0 * (len - 10.0);
            let on = |off: f32| (a[0] + dir[0] * s + side[0] * off, a[1] + dir[1] * s + side[1] * off);
            for sign in [1.0, -1.0] {
                let inside = on(sign * (1.5 - gap));
                let outside = on(sign * (1.5 + gap));
                assert!((baked.height(inside.0, inside.1) + 5.0).abs() < 1e-4, "{s}: floor {}", baked.height(inside.0, inside.1));
                assert_eq!(baked.height(outside.0, outside.1), 0.0, "{s}");
            }
        }
    }

    #[test]
    fn a_wall_faces_the_tool_that_made_it_with_no_stripes() {
        let (sim, a, b) = slanted_cut(-5.0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let (dir, side) = ([(b[0] - a[0]) / len, (b[1] - a[1]) / len], [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]);
        // Halfway down the wall the light never changes along a straight wall.
        let first = baked.brightness(a[0] + dir[0] * 10.0 + side[0] * 1.5, a[1] + dir[1] * 10.0 + side[1] * 1.5, -2.5);
        for k in 0..1000 {
            let s = 10.0 + k as f32 / 1000.0 * (len - 20.0);
            let at = baked.brightness(a[0] + dir[0] * s + side[0] * 1.5, a[1] + dir[1] * s + side[1] * 1.5, -2.5);
            assert!((at - first).abs() < 0.01, "{at} {first} at {s}");
        }
    }

    #[test]
    fn a_step_between_two_depths_is_as_clean_as_a_wall_to_the_top() {
        use crate::relief::{Cutter, Sim, ToolKind};
        let mut sim = Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let wide = Cutter::new(ToolKind::EndMill, 12.0, 0.0, sim.grid.cell);
        let thin = Cutter::new(ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        sim.cut_move(&wide, &mut [], [20.0, 50.03, -2.0], [80.0, 50.03, -2.0], 0);
        sim.cut_move(&thin, &mut [], [20.0, 50.03, -9.0], [80.0, 50.03, -9.0], 1);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        // Across both walls on one side, sampled finely at many places along them.
        for k in 0..400 {
            let x = 25.0 + k as f32 * 0.123;
            let h = |off: f32| baked.height(x, 50.03 + off);
            assert!((h(1.49) + 9.0).abs() < 1e-3, "{x}: {}", h(1.49));
            assert!((h(1.51) + 2.0).abs() < 1e-3, "{x}: {}", h(1.51));
            assert!((h(5.99) + 2.0).abs() < 1e-3, "{x}: {}", h(5.99));
            assert_eq!(h(6.01), 0.0, "{x}");
        }
        // The hit on the ledge is level, the deep wall is a wall.
        let flat = baked.brightness(40.0, 53.0, -2.0);
        let wall = baked.brightness(40.0, 51.53, -5.0);
        assert!((flat - wall).abs() > 0.03 || wall < 1.0, "{flat} {wall}");
    }

    #[test]
    fn a_round_tool_keeps_its_round_shape_near_its_edge() {
        use crate::relief::{Cutter, Sim, ToolKind};
        let mut sim = Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let ball = Cutter::new(ToolKind::BallNose, 6.0, 0.0, sim.grid.cell);
        sim.cut_move(&ball, &mut [], [20.0, 50.0, -8.0], [80.0, 50.0, -8.0], 0);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        // Deeper than the ball, so its side is a wall and its floor the shape of the ball.
        let depth_at = |off: f32| baked.height(50.0, 50.0 + off);
        assert!((depth_at(0.0) + 8.0).abs() < 0.01, "{}", depth_at(0.0));
        assert!(depth_at(2.0) > depth_at(0.0), "rising towards its edge");
        assert_eq!(depth_at(3.2), 0.0, "and the top outside it");
    }

    #[test]
    fn with_too_many_moves_the_grid_alone_draws_the_surface() {
        let mut sim = crate::relief::Sim::new(10.0, 10.0, 0.5, 40_000);
        let cutter = crate::relief::Cutter::new(crate::relief::ToolKind::EndMill, 1.0, 0.0, sim.grid.cell);
        // Nothing has cut yet: no moves to look at.
        assert!(crate::relief::CutIndex::build(&sim).is_none());
        sim.cut_move(&cutter, &mut [], [5.0, 5.0, -0.2], [6.0, 5.0, -0.2], 0);
        assert!(crate::relief::CutIndex::build(&sim).is_some());
        // A move above the material is not a cut at all.
        let count = sim.moves.len();
        sim.cut_move(&cutter, &mut [], [1.0, 1.0, 0.5], [2.0, 2.0, 0.5], 0);
        assert_eq!(sim.moves.len(), count);
    }

    #[test]
    fn a_removed_piece_stays_removed_on_screen() {
        use crate::relief::{Cutter, Sim, ToolKind};
        let mut sim = Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let cutter = Cutter::new(ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        let corners = [[30.0, 30.0], [70.0, 30.0], [70.0, 70.0], [30.0, 70.0], [30.0, 30.0]];
        for w in corners.windows(2) {
            sim.cut_move(&cutter, &mut [], [w[0][0], w[0][1], -13.8], [w[1][0], w[1][1], -13.8], 0);
        }
        assert!(sim.remove_waste(50.0, 50.0));
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        // Everywhere inside the cut-out, right up to where the wall of the piece was, is air.
        for k in 0..300 {
            let at = 31.0 + k as f32 * 0.03;
            assert!(baked.is_air(baked.height(at, 50.0)), "{at}: {}", baked.height(at, 50.0));
        }
        // And the outer part keeps its wall where the tool's edge was.
        assert_eq!(baked.height(28.49, 50.0), 0.0);
        assert!(baked.is_air(baked.height(28.51, 50.0)));
    }

    #[test]
    fn a_small_step_at_an_angle_is_as_clean_as_a_big_one() {
        use crate::relief::{Cutter, Sim, ToolKind};
        let mut sim = Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let wide = Cutter::new(ToolKind::EndMill, 12.0, 0.0, sim.grid.cell);
        let thin = Cutter::new(ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        // Nearly the same depth, so most of the deeper tool's slope never gets below the first.
        let (a, b) = ([20.03f32, 31.07f32], [80.11f32, 57.41f32]);
        let at = |p: [f32; 2], z: f64| [p[0] as f64, p[1] as f64, z];
        sim.cut_move(&wide, &mut [], at(a, -8.0), at(b, -8.0), 0);
        sim.cut_move(&thin, &mut [], at(a, -8.2), at(b, -8.2), 1);
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let (dir, side) = ([(b[0] - a[0]) / len, (b[1] - a[1]) / len], [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]);
        for k in 0..2000 {
            let s = 8.0 + k as f32 / 2000.0 * (len - 16.0);
            let h = |off: f32| baked.height(a[0] + dir[0] * s + side[0] * off, a[1] + dir[1] * s + side[1] * off);
            assert!((h(1.48) + 8.2).abs() < 1e-3, "{s}: {}", h(1.48));
            assert!((h(1.52) + 8.0).abs() < 1e-3, "{s}: {}", h(1.52));
        }
    }

    #[test]
    fn the_quick_picture_reads_only_the_grid_and_can_be_stopped() {
        let baked = surface(40_000, |x, y| inside(x, y, 3.0, 7.0).then_some(-0.2));
        let mut s = scene(&baked);
        s.quick = true;
        let quick = s.render(VIEW, 100, 100, 1, 1);
        assert!(quick.iter().any(|c| c.a() == 255), "it draws");
        // Told to stop, nothing is drawn.
        let stop = AtomicBool::new(true);
        s.stop = Some(&stop);
        let stopped = s.render(VIEW, 100, 100, 1, EDGE_SAMPLES);
        assert!(stopped.iter().all(|c| c.a() == 0));
    }

    #[test]
    fn the_quick_picture_is_as_small_as_it_is_told() {
        let big = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 1000.0));
        let (w, h, samples) = picture_size(big, 1.0, 50.0, 0.01, false, 60_000.0);
        assert!(w * h <= 61_000 && samples == 1, "{w} {h}");
        assert!((w as f32 / h as f32 - 2.0).abs() < 0.05, "keeping its shape");
    }

    #[test]
    fn nothing_is_left_of_a_removed_piece_all_around_it() {
        use crate::relief::{Cutter, Sim, ToolKind};
        // A part cut out of the sheet with a rounded, slanted outline, nothing lining up with the grid.
        let mut sim = Sim::new(100.0, 100.0, 13.0, 1_000_000);
        let cutter = Cutter::new(ToolKind::EndMill, 3.0, 0.0, sim.grid.cell);
        let outline: Vec<[f64; 2]> = (0..=720).map(|k| {
            let a = k as f64 / 720.0 * std::f64::consts::TAU;
            let r = 30.0 + 6.0 * (3.0 * a).cos();
            [50.03 + r * a.cos(), 50.07 + r * 0.8 * a.sin()]
        }).collect();
        for w in outline.windows(2) {
            sim.cut_move(&cutter, &mut [], [w[0][0], w[0][1], -13.8], [w[1][0], w[1][1], -13.8], 0);
        }
        // The sheet around it comes away.
        assert!(sim.remove_waste(1.0, 1.0));
        let baked = Baked::new(&sim.grid, Some(&sim), 0.0, 13.0, 100.0, 100.0);
        let distance = |x: f32, y: f32| {
            outline.windows(2).map(|w| {
                let (a, b) = ([w[0][0] as f32, w[0][1] as f32], [w[1][0] as f32, w[1][1] as f32]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let t = (((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
                (a[0] + dx * t - x).hypot(a[1] + dy * t - y)
            }).fold(f32::MAX, f32::min)
        };
        // Everywhere the tool passed and all beyond it is air, however finely it's looked at.
        let mut kept = 0;
        for k in 0..720 {
            let a = k as f32 / 720.0 * std::f32::consts::TAU;
            for step in 0..200 {
                let r = 20.0 + step as f32 * 0.15;
                let (x, y) = (50.03 + r * a.cos(), 50.07 + r * 0.8 * a.sin());
                let inside = {
                    let rr = 30.0 + 6.0 * (3.0 * a).cos();
                    r < rr
                };
                if inside {
                    if distance(x, y) > 1.52 {
                        kept += 1;
                        assert!(!baked.is_air(baked.height(x, y)), "the part is still there at {x},{y}");
                    }
                } else {
                    assert!(baked.is_air(baked.height(x, y)), "a sliver at {x},{y}: {}", baked.height(x, y));
                }
            }
        }
        assert!(kept > 1000);
    }
}

