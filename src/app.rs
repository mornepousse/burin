//! The project document and everything that can be done to it, without any drawing code.

use crate::geom::{self, BoolOp, Field, Path, P};
use crate::relief::{self, Combine, Component, Grid, Profile, Sim};
use crate::toolpath::{self, blank_op, default_tools, Machine, Move, Op, OpKind, Tool};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Units {
    Inches,
    Mm,
}

impl Units {
    pub fn word(self) -> &'static str {
        match self {
            Units::Inches => "inches",
            Units::Mm => "mm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Datum {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
    Centre,
}

impl Datum {
    pub fn label(self) -> &'static str {
        match self {
            Datum::BottomLeft => "Bottom Left",
            Datum::BottomRight => "Bottom Right",
            Datum::TopLeft => "Top Left",
            Datum::TopRight => "Top Right",
            Datum::Centre => "Center",
        }
    }
}

pub const MATERIALS: [(&str, [u8; 3], [u8; 3]); 5] = [
    ("Beech Dark", [247, 201, 142], [232, 170, 112]),
    ("Maple", [248, 226, 186], [236, 208, 160]),
    ("Oak", [214, 170, 112], [186, 138, 84]),
    ("Cherry", [190, 112, 76], [160, 84, 54]),
    ("Walnut", [128, 88, 62], [96, 62, 42]),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    /// The board under the material, which the tool may cut into on the way through.
    #[serde(default)]
    pub spoil_board: f64,
    pub units: Units,
    pub z_on_bed: bool,
    pub datum: Datum,
    pub use_offset: bool,
    pub offset: P,
    pub resolution: usize,
    /// How the model looks: `None` means a solid colour.
    pub material: Option<usize>,
    pub solid: [u8; 3],
    /// How the wood looks in the carve preview: `None` means the solid colour.
    pub preview_material: Option<usize>,
}

impl Default for Job {
    fn default() -> Self {
        Job {
            width: 10.0,
            height: 10.0,
            thickness: 0.5,
            spoil_board: 0.0,
            units: Units::Inches,
            z_on_bed: false,
            datum: Datum::BottomLeft,
            use_offset: false,
            offset: [0.0, 0.0],
            resolution: 0,
            material: None,
            solid: [122, 122, 62],
            preview_material: Some(0),
        }
    }
}

pub const RESOLUTIONS: [(&str, &str, usize); 4] = [
    ("Standard (fastest)", "1 million points", 1_000_000),
    ("High", "2 million points", 2_000_000),
    ("Very High", "4 million points", 4_000_000),
    ("Highest", "8 million points", 8_000_000),
];

impl Job {
    pub fn points(&self) -> usize {
        RESOLUTIONS[self.resolution.min(3)].2
    }
    /// How deep the tool may go before anything is wrong: through the material and into its board.
    pub fn deepest_allowed(&self) -> f64 {
        self.thickness + self.spoil_board.max(0.0)
    }
    pub fn unit(&self) -> &'static str {
        self.units.word()
    }
    fn datum_point(&self) -> P {
        match self.datum {
            Datum::BottomLeft => [0.0, 0.0],
            Datum::BottomRight => [self.width, 0.0],
            Datum::TopLeft => [0.0, self.height],
            Datum::TopRight => [self.width, self.height],
            Datum::Centre => [self.width / 2.0, self.height / 2.0],
        }
    }
    /// Makes this job position the machine's X0 Y0.
    pub fn set_origin(&mut self, at: P) {
        let d = self.datum_point();
        self.offset = [d[0] - at[0], d[1] - at[1]];
        self.use_offset = self.offset != [0.0, 0.0];
    }
    /// The job position that becomes the machine's X0 Y0.
    pub fn origin(&self) -> P {
        let mut o = self.datum_point();
        if self.use_offset {
            o[0] -= self.offset[0];
            o[1] -= self.offset[1];
        }
        o
    }
}

/// How a shape was made, so it can still be edited as that shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Prim {
    Free,
    Circle { c: P, r: f64 },
    Ellipse { c: P, rx: f64, ry: f64 },
    Rect { c: P, w: f64, h: f64, corner: f64 },
    Polygon { c: P, r: f64, sides: usize },
    Star { c: P, outer: f64, inner: f64, points: usize },
    Spiral { c: P, r: f64, turns: f64 },
    Arc { c: P, r: f64, start: f64, sweep: f64 },
    Text { at: P, height: f64, text: String },
}

impl Prim {
    pub fn paths(&self) -> Vec<Path> {
        match self {
            Prim::Free => Vec::new(),
            Prim::Circle { c, r } => vec![Path::closed(geom::circle(*c, *r))],
            Prim::Ellipse { c, rx, ry } => vec![Path::closed(geom::ellipse(*c, *rx, *ry))],
            Prim::Rect { c, w, h, corner } => vec![Path::closed(geom::rect(*c, *w, *h, *corner))],
            Prim::Polygon { c, r, sides } => vec![Path::closed(geom::polygon(*c, *r, *sides))],
            Prim::Star { c, outer, inner, points } => vec![Path::closed(geom::star(*c, *outer, *inner, *points))],
            Prim::Spiral { c, r, turns } => vec![Path::open(geom::spiral(*c, *r, *turns))],
            Prim::Arc { c, r, start, sweep } => vec![Path::open(geom::arc(*c, *r, *start, *sweep))],
            Prim::Text { at, height, text } => geom::text_paths(text, *height, *at),
        }
    }

    pub(crate) fn translate(&mut self, dx: f64, dy: f64) {
        match self {
            Prim::Free => {}
            Prim::Circle { c, .. }
            | Prim::Ellipse { c, .. }
            | Prim::Rect { c, .. }
            | Prim::Polygon { c, .. }
            | Prim::Star { c, .. }
            | Prim::Spiral { c, .. }
            | Prim::Arc { c, .. } => {
                c[0] += dx;
                c[1] += dy;
            }
            Prim::Text { at, .. } => {
                at[0] += dx;
                at[1] += dy;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vector {
    pub id: u64,
    pub paths: Vec<Path>,
    pub layer: usize,
    pub prim: Prim,
    /// Guides the drawing, but is never cut.
    #[serde(default)]
    pub construction: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    #[serde(default = "default_layer_colour")]
    pub color: [u8; 3],
    #[serde(default)]
    pub locked: bool,
}

fn default_layer_colour() -> [u8; 3] {
    [24, 26, 30]
}

impl Layer {
    pub fn named(name: &str) -> Layer {
        Layer { name: name.into(), visible: true, color: default_layer_colour(), locked: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Toolpath {
    pub op: Op,
    pub visible: bool,
    pub color: [u8; 3],
    #[serde(skip)]
    pub moves: Arc<Vec<Move>>,
    /// Moves that came from another program's file: kept in the project, not worked out again,
    /// until the toolpath is calculated here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_moves: Option<Arc<Vec<Move>>>,
    /// The same moves with their exact arcs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_steps: Option<Arc<Vec<crate::arcs::Step>>>,
}

/// Everything saved in a project file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Doc {
    pub job: Job,
    pub vectors: Vec<Vector>,
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    pub components: Vec<Component>,
    pub tools: Vec<Tool>,
    pub toolpaths: Vec<Toolpath>,
    pub machine: Machine,
    pub next_id: u64,
    #[serde(default)]
    pub constraints: Vec<crate::constraints::Constraint>,
    /// Lines across the job to draw against.
    #[serde(default)]
    pub guides: Vec<crate::snap::Guide>,
    /// Pictures placed on the job.
    #[serde(default)]
    pub bitmaps: Vec<crate::bitmap::BitmapRef>,
    /// New shapes are drawn as construction geometry.
    #[serde(default)]
    pub new_construction: bool,
}

impl Default for Doc {
    fn default() -> Self {
        Doc {
            job: Job::default(),
            vectors: Vec::new(),
            layers: vec![Layer::named("Layer 1")],
            active_layer: 0,
            components: Vec::new(),
            tools: default_tools(),
            toolpaths: Vec::new(),
            machine: Machine { safe_z: 0.2, home: [0.0, 0.0, 0.8], plunge_gap: 0.2, gap_above: 0.0, gap_below: None },
            next_id: 1,
            constraints: Vec::new(),
            new_construction: false,
            bitmaps: Vec::new(),
            guides: Vec::new(),
        }
    }
}

impl Doc {
    /// The machine settings with the home position as a place on the job. It's held measured from
    /// the X/Y zero, so that it goes along when the zero moves.
    pub fn machine_on_job(&self) -> Machine {
        let zero = self.job.origin();
        let home = self.machine.home;
        Machine { home: [home[0] + zero[0], home[1] + zero[1], home[2]], ..self.machine }
    }

    /// Changes the unit of the job, converting every stored length.
    pub fn set_units(&mut self, units: Units) {
        if units == self.job.units {
            return;
        }
        let k = if units == Units::Mm { 25.4 } else { 1.0 / 25.4 };
        let j = &mut self.job;
        j.units = units;
        j.width *= k;
        j.height *= k;
        j.thickness *= k;
        j.spoil_board *= k;
        j.offset = [j.offset[0] * k, j.offset[1] * k];
        for g in &mut self.guides {
            g.at *= k;
        }
        for b in &mut self.bitmaps {
            b.at = [b.at[0] * k, b.at[1] * k];
            b.width *= k;
        }
        let scale_paths = |paths: &mut Vec<Path>| {
            for p in paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                p[0] *= k;
                p[1] *= k;
            }
        };
        for v in &mut self.vectors {
            scale_paths(&mut v.paths);
            v.prim = Prim::Free;
        }
        for c in &mut self.components {
            scale_paths(&mut c.paths);
            c.height *= k;
            c.base *= k;
            if let Some(map) = &mut c.surface {
                map.at = [map.at[0] * k, map.at[1] * k];
                map.width *= k;
            }
        }
        let metric = units == Units::Mm;
        for tp in &mut self.toolpaths {
            let o = &mut tp.op;
            o.tool = o.tool.in_units(metric);
            o.clearance_tool = o.clearance_tool.as_ref().map(|t| t.in_units(metric));
            o.stepover *= k;
            o.retract_gap *= k;
            o.boundary_offset *= k;
            o.gap *= k;
            o.tex_cut = [o.tex_cut[0] * k, o.tex_cut[1] * k];
            o.tex_length = [o.tex_length[0] * k, o.tex_length[1] * k];
            scale_paths(&mut o.paths);
            o.start_depth *= k;
            o.cut_depth *= k;
            o.allowance *= k;
            o.raster_allowance *= k;
            o.last_thickness *= k;
            o.last_allowance *= k;
            o.tab_length *= k;
            o.tab_thickness *= k;
            o.ramp_distance *= k;
            o.lead_distance *= k;
            o.flat_depth *= k;
            o.chamfer_width *= k;
            o.lead_radius *= k;
            o.ramp_angle_limit *= k;
            o.overcut *= k;
            for p in &mut o.passes {
                *p *= k;
            }
            if let Some(f) = &mut o.selector {
                f.diameter *= k;
                f.tolerance *= k;
            }
            tp.moves = Arc::new(tp.moves.iter().map(|m| Move { p: [m.p[0] * k, m.p[1] * k, m.p[2] * k], rapid: m.rapid }).collect());
            if tp.kept_moves.is_some() {
                tp.kept_moves = Some(tp.moves.clone());
            }
            if let Some(steps) = &tp.kept_steps {
                use crate::arcs::Step;
                tp.kept_steps = Some(Arc::new(steps.iter().map(|s| match s {
                    Step::Straight(m) => Step::Straight(Move { p: [m.p[0] * k, m.p[1] * k, m.p[2] * k], rapid: m.rapid }),
                    Step::Arc { end, centre, counter_clockwise } => Step::Arc { end: [end[0] * k, end[1] * k, end[2] * k], centre: [centre[0] * k, centre[1] * k], counter_clockwise: *counter_clockwise },
                }).collect()));
            }
        }
        let m = &mut self.machine;
        m.safe_z *= k;
        m.plunge_gap *= k;
        m.gap_above *= k;
        m.gap_below = m.gap_below.map(|g| g * k);
        m.home = [m.home[0] * k, m.home[1] * k, m.home[2] * k];
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn add_vector(&mut self, paths: Vec<Path>, prim: Prim) -> u64 {
        let id = self.new_id();
        self.vectors.push(Vector { id, paths, layer: self.active_layer, prim, construction: self.new_construction });
        id
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Start,
    Work,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LeftTab {
    Drawing,
    Modeling,
    Clipart,
    Layers,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawKind {
    Circle,
    Ellipse,
    Rect,
    Polygon,
    Star,
    Polyline,
    Arc,
    Curve,
    Spiral,
    Text,
}

impl DrawKind {
    pub fn title(self) -> &'static str {
        match self {
            DrawKind::Circle => "Draw Circle",
            DrawKind::Ellipse => "Draw Ellipse",
            DrawKind::Rect => "Draw Rectangle",
            DrawKind::Polygon => "Draw Polygon",
            DrawKind::Star => "Draw Star",
            DrawKind::Polyline => "Draw Lines",
            DrawKind::Arc => "Draw Arc",
            DrawKind::Curve => "Draw Curve",
            DrawKind::Spiral => "Draw Spiral",
            DrawKind::Text => "Draw Text",
        }
    }
}

/// The values of the open "Draw ..." form.
#[derive(Clone, Debug)]
pub struct DrawForm {
    pub kind: DrawKind,
    pub c: P,
    pub r: f64,
    pub use_dia: bool,
    pub rx: f64,
    pub ry: f64,
    pub w: f64,
    pub h: f64,
    pub corner: f64,
    pub sides: usize,
    pub inner: f64,
    pub start: f64,
    pub sweep: f64,
    pub turns: f64,
    pub text: String,
    pub text_height: f64,
    pub editing: Option<u64>,
    pub points: Vec<P>,
    /// The Next Point fields of the line tool, relative to the last point where it applies.
    pub next: P,
    pub delta: P,
    pub step: [f64; 2],
}

impl DrawForm {
    pub fn new(kind: DrawKind, job: &Job) -> Self {
        let size = job.width.min(job.height);
        DrawForm {
            kind,
            c: [job.width / 2.0, job.height / 2.0],
            r: size * 0.12,
            use_dia: false,
            rx: size * 0.2,
            ry: size * 0.12,
            w: size * 0.4,
            h: size * 0.25,
            corner: 0.0,
            sides: 6,
            inner: size * 0.05,
            start: 0.0,
            sweep: 180.0,
            turns: 3.0,
            text: "Text".into(),
            text_height: size * 0.1,
            editing: None,
            points: Vec::new(),
            next: [0.0, 0.0],
            delta: [0.0, 0.0],
            step: [0.0, 0.0],
        }
    }

    pub fn prim(&self) -> Option<Prim> {
        let c = self.c;
        Some(match self.kind {
            DrawKind::Circle => Prim::Circle { c, r: self.r },
            DrawKind::Ellipse => Prim::Ellipse { c, rx: self.rx, ry: self.ry },
            DrawKind::Rect => Prim::Rect { c, w: self.w, h: self.h, corner: self.corner },
            DrawKind::Polygon => Prim::Polygon { c, r: self.r, sides: self.sides },
            DrawKind::Star => Prim::Star { c, outer: self.r, inner: self.inner, points: self.sides },
            DrawKind::Spiral => Prim::Spiral { c, r: self.r, turns: self.turns },
            DrawKind::Arc => Prim::Arc { c, r: self.r, start: self.start, sweep: self.sweep },
            DrawKind::Text => Prim::Text { at: c, height: self.text_height, text: self.text.clone() },
            DrawKind::Polyline | DrawKind::Curve => return None,
        })
    }

    /// Fills the form from a shape as it is now, including one that no longer remembers what
    /// it was drawn as (imported, say) but still has the form. False when it is another kind.
    pub fn load_shape(&mut self, v: &Vector) -> bool {
        // What it was drawn as only counts if it still looks like that.
        let still_true = v.prim.paths().len() == v.paths.len()
            && v.prim.paths().iter().zip(&v.paths).all(|(p, q)| {
                p.closed == q.closed && p.pts.len() == q.pts.len() && p.pts.iter().zip(&q.pts).all(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() < 1e-6)
            });
        if still_true && self.load(v.id, &v.prim) {
            return true;
        }
        let [only] = v.paths.as_slice() else { return false };
        match self.kind {
            DrawKind::Circle => {
                let Some((c, r)) = crate::bulk::as_circle(only) else { return false };
                self.c = c;
                self.r = r;
            }
            DrawKind::Ellipse => {
                let Some((c, rx, ry)) = crate::bulk::as_ellipse(only) else { return false };
                self.c = c;
                self.rx = rx;
                self.ry = ry;
            }
            DrawKind::Rect => {
                if !crate::bulk::is_rectangle(only) {
                    return false;
                }
                let Some((lo, hi)) = geom::bbox(&v.paths) else { return false };
                self.c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                self.w = hi[0] - lo[0];
                self.h = hi[1] - lo[1];
                self.corner = 0.0;
            }
            _ => return false,
        }
        self.editing = Some(v.id);
        true
    }

    /// Fills the form from what a shape was drawn as; false when that is another kind.
    pub fn load(&mut self, id: u64, prim: &Prim) -> bool {
        match (self.kind, prim) {
            (DrawKind::Circle, Prim::Circle { c, r }) => {
                self.c = *c;
                self.r = *r;
            }
            (DrawKind::Ellipse, Prim::Ellipse { c, rx, ry }) => {
                self.c = *c;
                self.rx = *rx;
                self.ry = *ry;
            }
            (DrawKind::Rect, Prim::Rect { c, w, h, corner }) => {
                self.c = *c;
                self.w = *w;
                self.h = *h;
                self.corner = *corner;
            }
            (DrawKind::Polygon, Prim::Polygon { c, r, sides }) => {
                self.c = *c;
                self.r = *r;
                self.sides = *sides;
            }
            (DrawKind::Star, Prim::Star { c, outer, inner, points }) => {
                self.c = *c;
                self.r = *outer;
                self.inner = *inner;
                self.sides = *points;
            }
            (DrawKind::Spiral, Prim::Spiral { c, r, turns }) => {
                self.c = *c;
                self.r = *r;
                self.turns = *turns;
            }
            (DrawKind::Arc, Prim::Arc { c, r, start, sweep }) => {
                self.c = *c;
                self.r = *r;
                self.start = *start;
                self.sweep = *sweep;
            }
            (DrawKind::Text, Prim::Text { at, height, text }) => {
                self.c = *at;
                self.text_height = *height;
                self.text = text.clone();
            }
            _ => return false,
        }
        self.editing = Some(id);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Xform {
    Move,
    Scale,
    Rotate,
    Mirror,
    Align,
    Offset,
    BlockCopy,
    CircularCopy,
    CopyAlong,
    Join,
    Fillet,
    Trim,
    Validate,
    Dimension,
    Nest,
    TextOnCurve,
    Bulk,
    Constraints,
    Distort,
}

#[derive(Clone, Debug)]
pub struct XformForm {
    pub kind: Xform,
    pub anchor: Datum,
    pub dx: f64,
    pub dy: f64,
    pub absolute: bool,
    pub sx: f64,
    pub sy: f64,
    pub keep_ratio: bool,
    pub angle: f64,
    pub pivot: P,
    pub distance: f64,
    pub outward: bool,
    pub cols: usize,
    pub rows: usize,
    pub count: usize,
    pub use_offset: bool,
    pub shift: P,
    pub group: bool,
    pub rotate_copies: bool,
    pub total_angle: bool,
    pub copy: bool,
    pub about_job: bool,
    pub copy_circles: bool,
    pub by_number: bool,
    pub even: bool,
    pub align: bool,
    pub new_layer: bool,
    pub reverse: bool,
    pub size: f64,
    pub spacing: f64,
    pub fillet: crate::edit::FilletKind,
    pub dim: crate::edit::DimKind,
    pub decimals: usize,
    pub text: String,
    pub rejoin: bool,
    pub tool_dia: f64,
    pub clearance: f64,
    pub border: f64,
    pub along_x: bool,
    /// Set Size and Rotate: treat each selected shape on its own.
    pub each: bool,
    pub constraint: Option<crate::constraints::Kind>,
    pub smart: bool,
    pub dim_mode: crate::constraints::DimMode,
    pub envelope_kind: crate::distort::EnvelopeKind,
    /// Bulk edit: the new sizes, and which ones to apply.
    pub bulk_d: f64,
    pub bulk_w: f64,
    pub bulk_h: f64,
    pub set_d: bool,
    pub set_w: bool,
    pub set_h: bool,
    pub tolerance: f64,
    pub layer_only: bool,
}

impl XformForm {
    pub fn new(kind: Xform, job: &Job) -> Self {
        let unit = if job.units == Units::Mm { 25.4 } else { 1.0 };
        XformForm {
            kind,
            anchor: if kind == Xform::Nest { Datum::BottomLeft } else { Datum::Centre },
            dx: 0.0,
            dy: 0.0,
            absolute: true,
            sx: 100.0,
            sy: 100.0,
            keep_ratio: true,
            angle: if kind == Xform::CircularCopy { 360.0 } else { 0.0 },
            pivot: [job.width / 2.0, job.height / 2.0],
            distance: match kind {
                Xform::Join => 0.0004 * unit,
                Xform::Fillet => 0.2 * unit,
                Xform::Dimension => 0.1 * unit,
                Xform::CopyAlong => 1.0 * unit,
                Xform::TextOnCurve => 0.0,
                _ => job.width.min(job.height) * 0.025,
            },
            outward: true,
            cols: 2,
            rows: 2,
            count: if kind == Xform::CopyAlong { 6 } else { 12 },
            use_offset: false,
            shift: [0.0, 0.0],
            group: false,
            rotate_copies: true,
            total_angle: true,
            copy: false,
            about_job: false,
            copy_circles: false,
            by_number: false,
            even: true,
            align: true,
            new_layer: kind == Xform::CopyAlong,
            reverse: false,
            size: if kind == Xform::CopyAlong { 0.1 * unit } else { 0.2 * unit },
            spacing: 1.0,
            fillet: crate::edit::FilletKind::Normal,
            dim: crate::edit::DimKind::Length,
            decimals: 4,
            text: String::new(),
            rejoin: true,
            tool_dia: 0.25 * unit,
            clearance: 0.0,
            border: 0.25 * unit,
            along_x: true,
            each: false,
            constraint: None,
            smart: false,
            dim_mode: crate::constraints::DimMode::Smart,
            envelope_kind: crate::distort::EnvelopeKind::BoundingBox,
            bulk_d: 0.0,
            bulk_w: 0.0,
            bulk_h: 0.0,
            set_d: true,
            set_w: true,
            set_h: true,
            tolerance: job.width.max(job.height) * 1e-3,
            layer_only: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShapeForm {
    pub profile: Profile,
    pub height: f64,
    pub base: f64,
    pub combine: Combine,
    pub editing: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum LeftForm {
    None,
    Job { backup: Option<Box<Doc>>, first: bool },
    Draw(DrawForm),
    Xform(XformForm),
    Shape(ShapeForm),
    Trace(Box<crate::bitmap::TraceForm>),
    Relief(Box<crate::relief_import::ReliefForm>),
}

#[derive(Clone, Debug)]
pub enum RightMode {
    List,
    Op { op: Box<Op>, editing: Option<usize>, tab: usize, error: Option<String> },
    Preview,
    Material(Box<Doc>),
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool2 {
    Select,
    Nodes,
    Measure,
    ZoomBox,
    Pan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Machined {
    Material,
    Global,
    Toolpath,
}

/// The animated carve preview.
#[derive(Clone, Debug, Default)]
pub struct Anim {
    pub queue: Vec<usize>,
    pub at: usize,
    pub step: usize,
    pub playing: bool,
    pub tool_at: Option<([f64; 3], usize)>,
    /// Straight to the end, as fast as can be without holding up the window.
    pub rush: bool,
}

pub struct App {
    pub doc: Doc,
    pub undo: Vec<Doc>,
    pub redo: Vec<Doc>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub recent: Vec<PathBuf>,
    /// The panel sections folded away, by title.
    pub folded: Vec<String>,
    pub screen: Screen,
    pub left_tab: LeftTab,
    pub left: LeftForm,
    pub right: RightMode,
    pub left_open: bool,
    pub right_open: bool,
    pub view3d: bool,
    pub tool: Tool2,
    pub selected: Vec<u64>,
    pub clipboard: Vec<Vector>,
    pub selected_tp: Option<usize>,
    pub selected_comp: Option<usize>,
    pub status: String,
    pub tool_index: usize,
    /// Working copy while the tool list is open.
    pub tool_edit: Option<Vec<Tool>>,
    /// What the tool list is choosing for: 0 main tool, 1 area clearance tool.
    pub tool_target: Option<u8>,
    pub picked_tool: Option<(u8, Tool)>,
    pub post_index: usize,
    /// Post processor files the user has loaded; they follow the built-in formats in the list.
    pub loaded_posts: Vec<crate::postp::PostDef>,
    pub one_file: bool,
    /// Writes curves as true arcs in machine files.
    pub output_arcs: bool,
    /// The pointer catches on what is drawn.
    pub snap_shapes: bool,
    pub show_guides: bool,
    /// Alt is held: nothing catches for now.
    pub snap_held_off: bool,
    pub snap_hit: std::cell::Cell<Option<crate::snap::SnapHit>>,
    pub issues: Vec<crate::edit::Issue>,
    pub clip_tab: usize,
    pub clip_folders: Vec<PathBuf>,
    pub clip_folder: usize,
    pub clip_files: Vec<(String, Vec<Path>)>,
    pub clip_loaded: Option<PathBuf>,
    pub picks: Vec<crate::constraints::Elem>,
    pub hover_elem: Option<crate::constraints::Elem>,
    /// The deepest cut, while the warning about going through the material is up.
    pub through_warning: Option<f64>,
    pub passes_dialog: Option<crate::opdialogs::PassesDialog>,
    pub selector_dialog: Option<crate::toolpath::VectorFilter>,
    /// The rule under the pointer, on the sheet or in the list.
    pub hover_rule: Option<usize>,
    pub selected_rule: Option<usize>,
    /// Why the last rule wasn't added.
    pub sketch_note: Option<String>,
    pub show_constraints: bool,
    pub constraint_conflict: bool,
    pub failed_print: Option<u64>,
    pub relink_due: bool,
    /// The toolpaths being worked out, while the progress bar is up.
    pub recalc: Option<crate::recalc::Running>,
    /// Work toolpaths out away from the window. The tests want each result at once.
    pub background_recalc: bool,
    /// New jobs start in millimetres. The tests' numbers are written in inches, so they ask for that.
    pub metric_by_default: bool,
    /// An arrow key is moving the selection: one step of undo, and toolpaths wait for the end.
    pub nudging: bool,
    /// What the drawing looked like when the toolpaths last followed it.
    pub shapes_print: Option<u64>,
    pub defined: std::collections::HashMap<u64, bool>,
    pub defined_print: Option<u64>,
    pub dim_edit: Option<usize>,
    pub dim_text: String,
    pub dim_focus: bool,
    pub dim_error: Option<String>,
    pub dim_anchor: Option<crate::constraints::Elem>,
    pub new_fillet: Option<(u64, P, P)>,
    /// The ends the last trim made: which shape, where, and what cut them.
    pub trim_ends: Vec<(u64, P, crate::constraints::Elem)>,
    pub trim_stroke: Option<crate::canvas::TrimStroke>,
    pub bitmap_pixels: std::collections::HashMap<String, crate::bitmap::Image>,
    pub bitmap_textures: std::collections::HashMap<String, egui::TextureHandle>,
    pub bitmap_missing: std::collections::HashSet<String>,
    pub active_bitmap: usize,
    pub trace_preview: Arc<Vec<Path>>,
    pub envelope: Option<crate::distort::Envelope>,
    pub tools_open: bool,
    pub about_open: bool,
    pub time_open: bool,
    pub edit_passes: bool,
    pub show_advanced: bool,
    pub message: Option<String>,

    // 2D view
    pub scale: f32,
    pub origin: egui::Pos2,
    pub auto_fit: bool,
    pub last_rect: egui::Rect,
    pub snap: bool,
    pub grid: bool,
    pub show_vectors: bool,
    pub show_toolpaths: bool,
    pub show_model2d: bool,
    pub drag: crate::canvas::Drag,
    pub measure: Option<(P, P)>,
    pub cursor: Option<P>,

    // Model and preview
    pub model: Option<Grid>,
    pub model_dirty: bool,
    pub sim: Option<Sim>,
    pub sim_undo: Option<Sim>,
    pub sim_tag: Vec<u8>,
    pub anim: Anim,
    pub speed: f32,
    pub animate: bool,
    pub draw_tool: bool,
    pub machined: Machined,
    pub global_fill: [u8; 3],
    pub save_preview: Option<PathBuf>,
    pub cam: crate::view3d::Camera,
    pub mesh: Option<crate::view3d::SurfaceCache>,
    /// The 3D picture being made away from the window.
    pub surface_job: Option<crate::view3d::SurfaceJob>,
    /// The tests and the screenshot option want each picture at once.
    pub surface_in_background: bool,
    /// How big the quick picture is, to suit how fast this computer is.
    pub quick_pixels: f32,
    pub surface_rev: u64,
    pub ornament: Option<egui::TextureHandle>,
    pub view_rect: egui::Rect,
}

impl Default for App {
    fn default() -> Self {
        App {
            doc: Doc::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            path: None,
            dirty: false,
            recent: load_recent(),
            folded: load_folded(),
            screen: Screen::Start,
            left_tab: LeftTab::Drawing,
            left: LeftForm::None,
            right: RightMode::List,
            left_open: true,
            right_open: false,
            view3d: false,
            tool: Tool2::Select,
            selected: Vec::new(),
            clipboard: Vec::new(),
            selected_tp: None,
            selected_comp: None,
            status: "Ready".into(),
            tool_index: 0,
            tool_edit: None,
            tool_target: None,
            picked_tool: None,
            // The first of the list that writes millimetres.
            post_index: usize::from(!cfg!(test)),
            loaded_posts: Vec::new(),
            one_file: true,
            output_arcs: false,
            snap_shapes: true,
            show_guides: true,
            snap_held_off: false,
            snap_hit: std::cell::Cell::new(None),
            issues: Vec::new(),
            clip_tab: 0,
            clip_folders: load_clip_folders(),
            clip_folder: 0,
            clip_files: Vec::new(),
            clip_loaded: None,
            picks: Vec::new(),
            hover_elem: None,
            through_warning: None,
            passes_dialog: None,
            selector_dialog: None,
            hover_rule: None,
            selected_rule: None,
            sketch_note: None,
            show_constraints: true,
            constraint_conflict: false,
            failed_print: None,
            relink_due: false,
            recalc: None,
            background_recalc: !cfg!(test),
            metric_by_default: !cfg!(test),
            nudging: false,
            shapes_print: None,
            defined: std::collections::HashMap::new(),
            defined_print: None,
            dim_edit: None,
            dim_text: String::new(),
            dim_focus: false,
            dim_error: None,
            dim_anchor: None,
            new_fillet: None,
            trim_ends: Vec::new(),
            trim_stroke: None,
            bitmap_pixels: std::collections::HashMap::new(),
            bitmap_textures: std::collections::HashMap::new(),
            bitmap_missing: std::collections::HashSet::new(),
            active_bitmap: 0,
            trace_preview: Arc::new(Vec::new()),
            envelope: None,
            tools_open: false,
            about_open: false,
            time_open: false,
            edit_passes: false,
            show_advanced: true,
            message: None,
            scale: 80.0,
            origin: egui::pos2(0.0, 0.0),
            auto_fit: true,
            last_rect: egui::Rect::NOTHING,
            snap: false,
            grid: false,
            show_vectors: true,
            show_toolpaths: true,
            show_model2d: false,
            drag: crate::canvas::Drag::None,
            measure: None,
            cursor: None,
            model: None,
            model_dirty: false,
            sim: None,
            sim_undo: None,
            sim_tag: Vec::new(),
            anim: Anim::default(),
            speed: 0.9,
            animate: true,
            draw_tool: true,
            machined: Machined::Material,
            global_fill: [230, 230, 230],
            save_preview: None,
            cam: crate::view3d::Camera::default(),
            mesh: None,
            surface_job: None,
            surface_in_background: !cfg!(test),
            quick_pixels: if cfg!(test) { crate::view3d::MOST_QUICK_PIXELS } else { 400_000.0 },
            surface_rev: 1,
            ornament: None,
            view_rect: egui::Rect::NOTHING,
        }
    }
}

fn recent_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("burin").join("recent.txt"))
}

fn clip_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("clipart-folders.txt"))
}

fn load_clip_folders() -> Vec<PathBuf> {
    if cfg!(test) {
        return Vec::new();
    }
    clip_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| s.lines().map(PathBuf::from).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

pub fn save_clip_folders(folders: &[PathBuf]) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = clip_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let lines: Vec<String> = folders.iter().map(|p| p.display().to_string()).collect();
        let _ = std::fs::write(file, lines.join("\n"));
    }
}

fn post_list_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("post-processors.txt"))
}

impl App {
    /// The name of an entry in the machine file list.
    pub fn post_name(&self, i: usize) -> String {
        match i.checked_sub(toolpath::POSTS.len()) {
            None => toolpath::POSTS[i].0.to_string(),
            Some(k) => self.loaded_posts.get(k).map_or_else(|| toolpath::POSTS[0].0.to_string(), |p| p.name.clone()),
        }
    }

    pub fn post_extension(&self, i: usize) -> String {
        match i.checked_sub(toolpath::POSTS.len()) {
            None => toolpath::POSTS[i].1.to_string(),
            Some(k) => self.loaded_posts.get(k).map_or_else(|| toolpath::POSTS[0].1.to_string(), |p| p.extension.clone()),
        }
    }

    /// Adds post processor files. Returns how many loaded, and why the others didn't.
    pub fn load_posts(&mut self, files: Vec<PathBuf>) -> (usize, Vec<String>) {
        let mut added = 0;
        let mut failed = Vec::new();
        for file in files {
            if self.loaded_posts.iter().any(|p| p.source.as_ref() == Some(&file)) {
                continue;
            }
            match crate::postp::PostDef::load(&file) {
                Ok(def) => {
                    self.loaded_posts.push(def);
                    added += 1;
                }
                Err(e) => failed.push(format!("{}: {e}", file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().to_string()))),
            }
        }
        self.remember_posts();
        (added, failed)
    }

    /// Every .pp file in a folder.
    /// The post processors in a folder and the folders inside it.
    pub fn post_files_in(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut inner: Vec<PathBuf> = std::fs::read_dir(dir).map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        inner.sort();
        let mut below: Vec<PathBuf> = inner.iter().flat_map(|d| App::post_files_in(d)).collect();
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pp"))).collect())
            .unwrap_or_default();
        files.sort();
        files.append(&mut below);
        files
    }

    pub fn remove_selected_post(&mut self) {
        if let Some(k) = self.post_index.checked_sub(toolpath::POSTS.len()) {
            if k < self.loaded_posts.len() {
                self.loaded_posts.remove(k);
                self.post_index = 0;
                self.remember_posts();
            }
        }
    }

    /// Keeps the list for next time: the chosen name first, then the files.
    pub fn remember_posts(&self) {
        if cfg!(test) {
            return;
        }
        if let Some(file) = post_list_file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut lines = vec![self.post_name(self.post_index)];
            lines.extend(self.loaded_posts.iter().filter_map(|p| p.source.as_ref()).map(|p| p.display().to_string()));
            let _ = std::fs::write(file, lines.join("\n"));
        }
    }

    /// Brings back the files loaded last time. Ones that have gone are skipped quietly.
    pub fn restore_posts(&mut self) {
        if cfg!(test) {
            return;
        }
        let Some(text) = post_list_file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
        let mut lines = text.lines();
        let chosen = lines.next().unwrap_or_default().to_string();
        for line in lines {
            if let Ok(def) = crate::postp::PostDef::load(std::path::Path::new(line)) {
                self.loaded_posts.push(def);
            }
        }
        let total = toolpath::POSTS.len() + self.loaded_posts.len();
        if let Some(i) = (0..total).find(|i| self.post_name(*i) == chosen) {
            self.post_index = i;
            self.output_arcs = self.is_planetcnc();
        }
    }
}

fn folded_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("folded-sections.txt"))
}

fn load_folded() -> Vec<String> {
    if cfg!(test) {
        return Vec::new();
    }
    let text = folded_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Remembers which sections are folded away for next time.
pub fn save_folded(folded: &[String]) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = folded_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, folded.join("\n"));
    }
}

fn load_recent() -> Vec<PathBuf> {
    if cfg!(test) {
        return Vec::new();
    }
    recent_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).take(5).collect())
        .unwrap_or_default()
}

impl App {
    pub fn checkpoint(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn do_undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            let was = self.doc.job.clone();
            self.redo.push(std::mem::replace(&mut self.doc, prev));
            self.after_history();
            self.after_job_history(&was);
            self.status = "Undid the last change".into();
        }
    }

    pub fn do_redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let was = self.doc.job.clone();
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.after_history();
            self.after_job_history(&was);
            self.status = "Redid the change".into();
        }
    }

    /// Undo or redo may have changed the sheet itself: fit the view to it again.
    fn after_job_history(&mut self, was: &Job) {
        let j = &self.doc.job;
        if (was.width, was.height, was.thickness, was.units) != (j.width, j.height, j.thickness, j.units) {
            self.auto_fit = true;
            self.last_rect = egui::Rect::NOTHING;
            self.sim = None;
            self.bump_surface();
        }
    }

    fn after_history(&mut self) {
        self.selected_rule = None;
        self.hover_rule = None;
        self.sketch_note = None;
        self.envelope = None;
        self.trace_preview = Arc::new(Vec::new());
        let ids: Vec<u64> = self.doc.vectors.iter().map(|v| v.id).collect();
        self.selected.retain(|s| ids.contains(s));
        self.model_dirty = true;
        self.selected_tp = self.selected_tp.filter(|i| *i < self.doc.toolpaths.len());
        self.selected_comp = self.selected_comp.filter(|i| *i < self.doc.components.len());
        self.dirty = true;
    }

    pub fn new_file(&mut self) {
        self.doc = Doc::default();
        if self.metric_by_default {
            self.doc.set_units(Units::Mm);
            (self.doc.job.width, self.doc.job.height, self.doc.job.thickness) = (600.0, 400.0, 18.0);
        }
        self.reset_state();
        self.screen = Screen::Work;
        self.left = LeftForm::Job { backup: None, first: true };
        self.status = "Ready".into();
    }

    pub fn reset_state(&mut self) {
        self.recalc = None;
        self.surface_job = None;
        self.undo.clear();
        self.redo.clear();
        self.selected.clear();
        self.selected_tp = None;
        self.selected_comp = None;
        self.sim = None;
        self.sim_undo = None;
        self.anim = Anim::default();
        self.model = None;
        self.model_dirty = true;
        self.view3d = false;
        self.right = RightMode::List;
        self.left_tab = LeftTab::Drawing;
        self.left = LeftForm::None;
        self.tool = Tool2::Select;
        self.auto_fit = true;
        self.path = None;
        self.dirty = false;
        self.measure = None;
        self.envelope = None;
        self.trace_preview = Arc::new(Vec::new());
        self.active_bitmap = 0;
        self.bump_surface();
    }

    pub fn bump_surface(&mut self) {
        self.surface_rev += 1;
    }

    pub fn save_to(&mut self, path: PathBuf) -> Result<(), String> {
        let text = serde_json::to_string(&self.doc).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("Could not save {}: {e}", path.display()))?;
        self.status = format!("Saved {}", path.display());
        self.remember(path.clone());
        self.path = Some(path);
        self.dirty = false;
        Ok(())
    }

    /// Starts a new job from a .crv3d project file: its material, layers, shapes and toolpaths.
    pub fn open_project(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        let p = crate::crv3d::read(&bytes)?;
        let mut doc = Doc::default();
        let (w, h) = (p.max[0] - p.min[0], p.max[1] - p.min[1]);
        // Everything here counts from the sheet's bottom left corner.
        let shift = [-p.min[0], -p.min[1]];
        let j = &mut doc.job;
        j.units = Units::Mm;
        j.width = w;
        j.height = h;
        j.thickness = p.max[2] - p.min[2];
        // Zero at the bottom of the material means the machine bed.
        j.z_on_bed = p.max[2] > 0.0 && p.min[2] >= 0.0;
        j.datum = match p.datum {
            1 => Datum::BottomRight,
            2 => Datum::TopRight,
            3 => Datum::TopLeft,
            4 => Datum::Centre,
            _ => Datum::BottomLeft,
        };
        // The machine's zero stays exactly where the file has it, even off the named spot.
        j.use_offset = false;
        let named = j.origin();
        let gap = [named[0] - shift[0], named[1] - shift[1]];
        if gap[0].abs() > 1e-6 || gap[1].abs() > 1e-6 {
            j.use_offset = true;
            j.offset = gap;
        }
        if !p.layers.is_empty() {
            doc.layers = p.layers.iter().map(|l| {
                let mut layer = Layer::named(&l.name);
                // Black reads as the usual ink on the sheet.
                if l.color != [0, 0, 0] {
                    layer.color = l.color;
                }
                layer
            }).collect();
        }
        let mut by_mark: std::collections::HashMap<[u8; 16], u64> = std::collections::HashMap::new();
        for (i, (layer, outline)) in p.shapes.iter().enumerate() {
            let mut outline = outline.clone();
            for q in &mut outline.pts {
                *q = [q[0] + shift[0], q[1] + shift[1]];
            }
            doc.active_layer = *layer;
            let id = doc.add_vector(vec![outline], Prim::Free);
            if let Some(mark) = p.marks.get(i) {
                by_mark.insert(*mark, id);
            }
        }
        doc.active_layer = 0;
        // Heights in the file count from where its Z zero is; here they count from the top.
        let down = if doc.job.z_on_bed { doc.job.thickness } else { 0.0 };
        let palette = [[80, 150, 230], [230, 120, 70], [90, 190, 120], [200, 110, 200], [220, 190, 70], [90, 190, 200]];
        for (k, saved) in p.toolpaths.iter().enumerate() {
            // The file holds these at single precision: more decimals would only be noise.
            let tidy = |v: f64| (v * 10_000.0).round() / 10_000.0;
            let place = |q: [f64; 3]| [tidy(q[0] + shift[0]), tidy(q[1] + shift[1]), tidy(q[2] - down)];
            if k == 0 {
                doc.machine.safe_z = saved.safe_z - down;
                doc.machine.plunge_gap = saved.plunge_gap - down;
                // Kept from the zero, as the file's own moves are in the job's coordinates.
                let (home, zero) = (place(saved.home), doc.job.origin());
                doc.machine.home = [home[0] - zero[0], home[1] - zero[1], home[2]];
            }
            let tool = saved_tool(saved);
            let op_kind = saved_kind(&saved.kind);
            let sources: Vec<u64> = saved.sources.iter().filter_map(|m| by_mark.get(m).copied()).collect();
            let paths: Vec<Path> = sources.iter().filter_map(|id| doc.vectors.iter().find(|v| v.id == *id)).flat_map(|v| v.paths.iter().cloned()).collect();
            // The file counts its cut depth and passes from the start depth; here they count from the top.
            let mut op = toolpath::blank_op(op_kind, saved.name.clone(), tool, saved.start_depth + saved.cut_depth, paths);
            op.start_depth = saved.start_depth;
            op.passes = saved.passes.iter().map(|d| d + saved.start_depth).collect();
            op.allowance = saved.allowance;
            op.dwell = saved.dwell;
            // Only a toolpath that can be made again the same way follows its shapes. Others keep
            // their moves as saved: V-carves, and those made partly from shapes since removed.
            let can_follow = (saved.kind.starts_with("Profile") || saved.kind == "Pocket" || saved.kind == "Drill") && !saved.options.area_clear && !saved.options.special;
            let whole = !sources.is_empty() && sources.len() == saved.sources.len();
            op.source = if can_follow && whole { sources } else { Vec::new() };
            saved_options(&mut op, &saved.options);
            if saved.kind.ends_with("Inside") {
                op.side = toolpath::Side::Inside;
            } else if saved.kind.ends_with("On") {
                op.side = toolpath::Side::On;
            }
            let moves: Vec<Move> = fast_descents(&saved.moves, saved.plunge_gap).into_iter().map(|(q, rapid)| Move { p: place(q), rapid }).collect();
            let moves = Arc::new(moves);
            let steps: Vec<crate::arcs::Step> = fast_descent_steps(&saved.steps, saved.plunge_gap)
                .into_iter()
                .map(|s| match s {
                    crate::arcs::Step::Straight(m) => crate::arcs::Step::Straight(Move { p: place(m.p), rapid: m.rapid }),
                    crate::arcs::Step::Arc { end, centre, counter_clockwise } => {
                        let c = place([centre[0], centre[1], 0.0]);
                        crate::arcs::Step::Arc { end: place(end), centre: [c[0], c[1]], counter_clockwise }
                    }
                })
                .collect();
            doc.toolpaths.push(Toolpath { op, visible: true, kept_moves: Some(moves.clone()), kept_steps: Some(Arc::new(steps)), moves, color: palette[k % palette.len()] });
        }
        let (shapes, layers, toolpaths) = (p.shapes.len(), p.layers.len().max(1), p.toolpaths.len());
        self.doc = doc;
        self.reset_state();
        self.screen = Screen::Work;
        // A separate file from the one it came from: saving asks where.
        self.path = None;
        self.dirty = true;
        self.status = format!("{shapes} shapes on {layers} layers, {toolpaths} toolpaths.");
        if toolpaths > 0 {
            self.right_open = true;
            self.right = RightMode::List;
        }
        self.remember(path.to_path_buf());
        Ok(())
    }

    pub fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("crv3d") || e.eq_ignore_ascii_case("crv")) {
            return self.open_project(&path);
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        let doc: Doc = serde_json::from_str(&text)
            .map_err(|_| format!("{} is not a project made by this program.", path.display()))?;
        self.doc = doc;
        self.reset_state();
        self.screen = Screen::Work;
        self.recalculate(false);
        self.status = format!("Opened {}", path.display());
        self.remember(path.clone());
        self.path = Some(path);
        Ok(())
    }

    fn remember(&mut self, path: PathBuf) {
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(5);
        if cfg!(test) {
            return;
        }
        if let Some(file) = recent_file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let lines: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
            let _ = std::fs::write(file, lines.join("\n"));
        }
    }

    /// A small finished project: a plaque with raised shapes, ready to preview.
    pub fn load_sample(&mut self) {
        self.doc = Doc::default();
        self.reset_state();
        self.screen = Screen::Work;
        let border = self.doc.add_vector(vec![Path::closed(geom::rect([5.0, 5.0], 9.0, 9.0, 0.6))], Prim::Rect { c: [5.0, 5.0], w: 9.0, h: 9.0, corner: 0.6 });
        let disc = self.doc.add_vector(vec![Path::closed(geom::circle([5.0, 6.0], 2.2))], Prim::Circle { c: [5.0, 6.0], r: 2.2 });
        let text = Prim::Text { at: [2.2, 1.6], height: 1.1, text: "Burin".into() };
        let word = self.doc.add_vector(text.paths(), text);
        let flat = ShapeForm { profile: Profile::Flat, height: 0.1, base: 0.0, combine: Combine::Add, editing: None };
        self.selected = vec![border];
        self.add_component(flat);
        self.selected = vec![disc];
        self.add_component(ShapeForm { profile: Profile::Round, height: 0.28, base: 0.0, combine: Combine::Add, editing: None });
        self.refresh_model();
        self.start_op(OpKind::Finish3D);
        if let RightMode::Op { op, .. } = &mut self.right {
            op.boundary = toolpath::Boundary::Material;
            op.raster = true;
        }
        self.calculate_op();
        self.selected = vec![word];
        self.start_op(OpKind::VCarve);
        if let RightMode::Op { op, .. } = &mut self.right {
            op.project = true;
        }
        self.calculate_op();
        self.selected = vec![border];
        self.start_op(OpKind::Profile);
        if let RightMode::Op { op, .. } = &mut self.right {
            op.tabs = true;
        }
        self.calculate_op();
        self.selected.clear();
        self.selected_tp = Some(0);
        self.undo.clear();
        self.dirty = false;
        if self.metric_by_default {
            self.doc.set_units(Units::Mm);
            self.refresh_model();
            self.bump_surface();
        }
    }

    pub fn selected_paths(&self) -> Vec<Path> {
        self.doc
            .vectors
            .iter()
            .filter(|v| self.selected.contains(&v.id))
            .flat_map(|v| v.paths.iter().cloned())
            .collect()
    }

    pub fn selected_bbox(&self) -> Option<(P, P)> {
        geom::bbox(self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).flat_map(|v| v.paths.iter()))
    }

    pub fn apply_draw(&mut self) {
        let LeftForm::Draw(form) = &self.left else { return };
        let form = form.clone();
        if matches!(form.kind, DrawKind::Polyline | DrawKind::Curve) {
            self.finish_line(false);
            return;
        }
        let Some(prim) = form.prim() else { return };
        let paths = prim.paths();
        if paths.is_empty() {
            self.status = "Type some text first.".into();
            return;
        }
        self.checkpoint();
        let existing = form.editing.filter(|id| self.doc.vectors.iter().any(|v| v.id == *id));
        let id = match existing {
            Some(id) => {
                let v = self.doc.vectors.iter_mut().find(|v| v.id == id).unwrap();
                v.paths = paths.clone();
                v.prim = prim;
                id
            }
            None => self.doc.add_vector(paths.clone(), prim),
        };
        self.selected = vec![id];
        if let LeftForm::Draw(f) = &mut self.left {
            f.editing = Some(id);
        }
        if existing.is_none() {
            return;
        }
        // A rule on the shape has the last word; say so rather than quietly undoing the typing.
        let ruled = self.doc.constraints.iter().any(|c| c.items.iter().any(|e| e.vector_id() == id));
        if ruled {
            self.maintain_constraints();
        }
        let now = self.doc.vectors.iter().find(|v| v.id == id).cloned();
        if let Some(v) = now.filter(|v| ruled && v.paths != paths) {
            if let LeftForm::Draw(f) = &mut self.left {
                f.load_shape(&v);
            }
            self.status = "A rule on this shape kept its size. Change the rule's dimension, or remove the rule.".into();
        }
        // Toolpaths made from this shape follow it.
        let redone = self.relink_toolpaths();
        if redone > 0 {
            self.status = format!("Changed the shape and recalculated {redone} toolpaths.");
        }
    }

    /// Turns the clicked points of the line tools into a shape.
    pub fn finish_line(&mut self, close: bool) {
        let LeftForm::Draw(form) = &mut self.left else { return };
        let pts = std::mem::take(&mut form.points);
        let curve = form.kind == DrawKind::Curve;
        if pts.len() < 2 {
            return;
        }
        let path = if curve {
            if close {
                let mut loop_pts = pts.clone();
                loop_pts.push(pts[0]);
                let mut smooth = geom::smooth_curve(&loop_pts);
                smooth.pop();
                Path::closed(smooth)
            } else {
                Path::open(geom::smooth_curve(&pts))
            }
        } else {
            Path { pts, closed: close }
        };
        self.checkpoint();
        let id = self.doc.add_vector(vec![path], Prim::Free);
        self.selected = vec![id];
    }

    pub fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.checkpoint();
        let sel = std::mem::take(&mut self.selected);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
    }

    pub fn copy_selected(&mut self) {
        self.clipboard = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).cloned().collect();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.checkpoint();
        let step = self.doc.job.width.min(self.doc.job.height) * 0.02;
        let items = self.clipboard.clone();
        self.selected.clear();
        for mut v in items {
            v.id = self.doc.new_id();
            v.layer = self.doc.active_layer;
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                p[0] += step;
                p[1] -= step;
            }
            v.prim.translate(step, -step);
            self.selected.push(v.id);
            self.doc.vectors.push(v);
        }
        self.copy_selected();
    }

    pub fn map_selected(&mut self, f: impl Fn(P) -> P) {
        for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                *p = f(*p);
            }
            v.prim = Prim::Free;
        }
    }

    pub fn translate_selected(&mut self, dx: f64, dy: f64) {
        for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                p[0] += dx;
                p[1] += dy;
            }
            v.prim.translate(dx, dy);
        }
    }



    /// `ax`/`ay`: -1 left/bottom, 0 centre, 1 right/top of the job; `None` leaves that axis alone.
    pub fn align_selected(&mut self, ax: Option<i32>, ay: Option<i32>) {
        let Some((lo, hi)) = self.selected_bbox() else { return };
        self.checkpoint();
        let (w, h) = (self.doc.job.width, self.doc.job.height);
        let dx = match ax {
            Some(-1) => -lo[0],
            Some(1) => w - hi[0],
            Some(_) => w / 2.0 - (lo[0] + hi[0]) / 2.0,
            None => 0.0,
        };
        let dy = match ay {
            Some(-1) => -lo[1],
            Some(1) => h - hi[1],
            Some(_) => h / 2.0 - (lo[1] + hi[1]) / 2.0,
            None => 0.0,
        };
        self.translate_selected(dx, dy);
    }

    pub fn offset_selected(&mut self, distance: f64, outward: bool) {
        let paths = self.selected_paths();
        let Some(field) = Field::build(&paths, distance.abs() + self.doc.job.width.min(self.doc.job.height) * 0.01, 900) else {
            self.status = "Offset needs a closed shape.".into();
            return;
        };
        let level = if outward { -distance.abs() } else { distance.abs() };
        let loops = field.contours(level);
        if loops.is_empty() {
            self.status = "That offset leaves nothing: try a smaller distance.".into();
            return;
        }
        self.checkpoint();
        let id = self.doc.add_vector(loops.into_iter().map(Path::closed).collect(), Prim::Free);
        self.selected = vec![id];
    }

    pub fn group_selected(&mut self) {
        self.keep_rules(App::group_inner);
    }

    fn group_inner(&mut self) {
        if self.selected.len() < 2 {
            self.status = "Select two or more shapes to group.".into();
            return;
        }
        self.checkpoint();
        let paths = self.selected_paths();
        let sel = std::mem::take(&mut self.selected);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
        self.selected = vec![self.doc.add_vector(paths, Prim::Free)];
    }

    pub fn ungroup_selected(&mut self) {
        self.keep_rules(App::ungroup_inner);
    }

    fn ungroup_inner(&mut self) {
        if !self.doc.vectors.iter().any(|v| self.selected.contains(&v.id) && v.paths.len() > 1) {
            self.status = "Select a grouped shape to ungroup.".into();
            return;
        }
        self.checkpoint();
        let paths = self.selected_paths();
        let sel = std::mem::take(&mut self.selected);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
        for p in paths {
            let id = self.doc.add_vector(vec![p], Prim::Free);
            self.selected.push(id);
        }
    }


    pub fn close_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.checkpoint();
        for v in self.doc.vectors.iter_mut().filter(|v| self.selected.contains(&v.id)) {
            for p in &mut v.paths {
                if p.pts.len() >= 3 {
                    p.closed = true;
                }
            }
            v.prim = Prim::Free;
        }
    }

    pub fn boolean_selected(&mut self, op: BoolOp) {
        self.keep_rules(|a| a.boolean_inner(op));
    }

    fn boolean_inner(&mut self, op: BoolOp) {
        let items: Vec<Vector> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).cloned().collect();
        if items.len() < 2 || items.iter().any(|v| v.paths.iter().all(|p| !p.closed)) {
            self.status = "Select two or more closed shapes.".into();
            return;
        }
        let mut result = items[0].paths.clone();
        for v in &items[1..] {
            result = geom::boolean(&result, &v.paths, op);
        }
        if result.is_empty() {
            self.status = "Those shapes don't overlap, so nothing is left.".into();
            return;
        }
        self.checkpoint();
        let sel = std::mem::take(&mut self.selected);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
        self.selected = vec![self.doc.add_vector(result, Prim::Free)];
    }

    pub fn add_component(&mut self, form: ShapeForm) {
        if let Some(i) = form.editing.filter(|i| *i < self.doc.components.len()) {
            self.checkpoint();
            let c = &mut self.doc.components[i];
            c.profile = form.profile;
            c.height = form.height;
            c.base = form.base;
            c.combine = form.combine;
        } else {
            let cut = self.cut_ids();
            let paths: Vec<Path> = self.doc.vectors.iter().filter(|v| cut.contains(&v.id)).flat_map(|v| v.paths.iter().cloned()).filter(|p| p.closed).collect();
            if paths.is_empty() {
                self.status = "Select a closed shape in the 2D view first.".into();
                return;
            }
            self.checkpoint();
            let n = self.doc.components.len() + 1;
            self.doc.components.push(Component {
                name: format!("Shape {n}"),
                paths,
                profile: form.profile,
                height: form.height,
                base: form.base,
                combine: form.combine,
                visible: true,
                surface: None,
            });
            self.selected_comp = Some(n - 1);
        }
        self.model_dirty = true;
        self.sim = None;
        self.view3d = true;
    }

    pub fn refresh_model(&mut self) {
        if !self.model_dirty {
            return;
        }
        self.model_dirty = false;
        let j = &self.doc.job;
        self.model = relief::compose(&self.doc.components, j.width, j.height, j.points());
        self.bump_surface();
    }

    pub fn start_op(&mut self, kind: OpKind) {
        let n = self.doc.toolpaths.iter().filter(|t| t.op.kind == kind).count() + 1;
        let name = format!("{} {n}", kind.base_name());
        let metric = self.doc.job.units == Units::Mm;
        let wanted = match kind {
            OpKind::VCarve | OpKind::Prism | OpKind::Chamfer => relief::ToolKind::VBit,
            OpKind::Engrave => relief::ToolKind::Engraving,
            OpKind::Finish3D | OpKind::Texture | OpKind::Moulding | OpKind::Flute => relief::ToolKind::BallNose,
            OpKind::Drill => relief::ToolKind::Drill,
            _ => relief::ToolKind::EndMill,
        };
        let size = |t: &Tool| {
            let inch = t.in_units(false).dia;
            match kind {
                OpKind::Profile | OpKind::Rough3D | OpKind::Drill | OpKind::VCarve | OpKind::Prism => (inch - 0.25).abs().min((inch - 1.25).abs() * if kind == OpKind::VCarve || kind == OpKind::Prism { 1.0 } else { 9.0 }),
                OpKind::Texture => (inch - 0.25).abs(),
                _ => (inch - 0.125).abs(),
            }
        };
        let tool = self
            .doc
            .tools
            .iter()
            .filter(|t| t.kind == wanted)
            .min_by(|a, b| {
                let score = |t: &Tool| size(t) + if t.metric == metric { 0.0 } else { 100.0 };
                score(a).total_cmp(&score(b))
            })
            .or(self.doc.tools.first())
            .map(|t| t.in_units(metric))
            .unwrap_or_else(|| default_tools()[0].in_units(metric));
        let depth = match kind {
            OpKind::VCarve | OpKind::Flute => self.doc.job.thickness * 0.2,
            OpKind::Engrave => 0.0,
            OpKind::Pocket => self.doc.job.thickness * 0.5,
            _ => self.doc.job.thickness,
        };
        let op = blank_op(kind, name, tool, depth, Vec::new());
        self.right = RightMode::Op { op: Box::new(op), editing: None, tab: 0, error: None };
        self.right_open = true;
    }

    pub fn edit_toolpath(&mut self) {
        let Some(i) = self.selected_tp else {
            self.status = "Select a toolpath to edit.".into();
            return;
        };
        let op = self.doc.toolpaths[i].op.clone();
        // Its shapes are selected again, unless it works from the 3D model.
        if !op.kind.uses_model() {
            self.selected = op.source.iter().copied().filter(|id| self.doc.vectors.iter().any(|v| v.id == *id)).collect();
        }
        self.right = RightMode::Op { op: Box::new(op), editing: Some(i), tab: 0, error: None };
        self.right_open = true;
    }

    /// The shapes a path should use, in the order that matters to it.
    fn op_paths(&self, op: &Op) -> Vec<Path> {
        let cut_ids: Vec<u64> = self.cut_ids();
        let by_click = || -> Vec<Path> { cut_ids.iter().filter_map(|id| self.doc.vectors.iter().find(|v| v.id == *id)).flat_map(|v| v.paths.iter().cloned()).collect() };
        let by_drawing = || -> Vec<Path> { self.doc.vectors.iter().filter(|v| cut_ids.contains(&v.id)).flat_map(|v| v.paths.iter().cloned()).collect() };
        match op.kind {
            OpKind::Rough3D | OpKind::Finish3D => match op.boundary {
                toolpath::Boundary::Model => self.doc.components.iter().filter(|c| c.visible).flat_map(|c| c.paths.iter().cloned()).collect(),
                toolpath::Boundary::Selected => by_click(),
                toolpath::Boundary::Material => Vec::new(),
            },
            _ if op.kind == OpKind::Moulding || op.order == toolpath::Order::AsDrawn => by_click(),
            _ => by_drawing(),
        }
    }

    /// The selected shapes that can be cut: construction geometry is left out.
    pub fn cut_ids(&self) -> Vec<u64> {
        self.selected.iter().copied().filter(|id| self.doc.vectors.iter().any(|v| v.id == *id && !v.construction)).collect()
    }

    /// Calculates at once, for what has more to do right after it.
    pub fn calculate_op(&mut self) {
        self.start_calculate(false);
    }

    /// The form's Calculate button, with the progress bar when it takes a while.
    pub fn calculate_from_form(&mut self) {
        self.start_calculate(self.background_recalc);
    }

    fn start_calculate(&mut self, background: bool) {
        if self.recalc.is_some() {
            return;
        }
        self.refresh_model();
        let RightMode::Op { op, editing, tab, .. } = &self.right else { return };
        let (mut op, editing, tab) = (op.clone(), *editing, *tab);
        // Shapes chosen by filter are chosen afresh.
        if let Some(filter) = op.selector.clone() {
            self.selected = self.filter_ids(&filter, op.tool.dia);
        }
        let paths = self.op_paths(&op);
        let model_based = op.kind.uses_model();
        if !paths.is_empty() || editing.is_none() || model_based {
            op.paths = paths;
            // Remember which shapes these were, so the path can follow them when they change.
            op.source = if model_based { Vec::new() } else { self.cut_ids() };
        }
        // A larger tool clears the area first, in a path of its own.
        let clearance = op.clearance_tool.clone().filter(|_| matches!(op.kind, OpKind::Pocket | OpKind::VCarve));
        let clear_op = clearance.map(|tool| {
            let mut first = (*op).clone();
            first.clearance_tool = None;
            first.name = format!("{} [Clear]", op.name);
            if op.kind == OpKind::VCarve {
                let half = (op.tool.angle.to_radians() / 2.0).tan();
                first.kind = OpKind::Pocket;
                first.start_depth = op.start_depth;
                first.cut_depth = op.start_depth + op.flat_depth;
                first.allowance = op.flat_depth * half;
            }
            first.tool = tool;
            first
        });
        let mut main = (*op).clone();
        if clear_op.is_some() && op.kind == OpKind::Pocket {
            main.rest_only = true;
        }
        let tasks: Vec<crate::recalc::Task> = clear_op.into_iter().chain(std::iter::once(main)).enumerate().collect();
        self.run_tasks(tasks, crate::recalc::Finish::Calculate { form: op, editing, tab }, background);
    }

    /// Puts newly calculated toolpaths in the list and shows them.
    pub(crate) fn keep_calculated(&mut self, results: Vec<(Op, Arc<Vec<Move>>)>, editing: Option<usize>) {
        self.checkpoint();
        let mut index = editing.unwrap_or(self.doc.toolpaths.len());
        if let Some(i) = editing {
            self.doc.toolpaths.remove(i);
        }
        for (o, moves) in results {
            let n = self.doc.toolpaths.len();
            let color = TOOLPATH_COLOURS[n % TOOLPATH_COLOURS.len()];
            self.doc.toolpaths.insert(index, Toolpath { op: o, visible: true, color, moves, kept_moves: None, kept_steps: None });
            index += 1;
        }
        self.selected_tp = Some(index - 1);
        self.right = RightMode::Preview;
        self.view3d = true;
        self.status = "Ready".into();
    }

    pub fn recalculate_all(&mut self) {
        self.recalculate(true);
    }

    /// `warn` tells of toolpaths that go through the material; opening a file doesn't need that yet.
    fn recalculate(&mut self, warn: bool) {
        if self.recalc.is_some() {
            return;
        }
        self.refresh_model();
        let mut tasks = self.follow_tasks();
        let (mut unchanged, mut no_shapes) = (0, 0);
        for (i, tp) in self.doc.toolpaths.iter_mut().enumerate() {
            if tasks.iter().any(|(at, _)| *at == i) {
                continue;
            }
            // Moves from another program's file stay exactly as they came.
            match &tp.kept_moves {
                Some(kept) => {
                    tp.moves = kept.clone();
                    if tp.op.source.is_empty() {
                        no_shapes += 1;
                    } else {
                        unchanged += 1;
                    }
                }
                None => tasks.push((i, tp.op.clone())),
            }
        }
        tasks.sort_by_key(|(at, _)| *at);
        self.run_tasks(tasks, crate::recalc::Finish::All { unchanged, no_shapes, warn }, self.background_recalc);
    }

    pub fn duplicate_toolpath(&mut self) {
        let Some(i) = self.selected_tp else { return };
        self.checkpoint();
        let mut copy = self.doc.toolpaths[i].clone();
        copy.op.name += " copy";
        self.doc.toolpaths.insert(i + 1, copy);
        self.selected_tp = Some(i + 1);
    }

    pub fn delete_toolpath(&mut self) {
        let Some(i) = self.selected_tp else { return };
        self.checkpoint();
        self.doc.toolpaths.remove(i);
        self.selected_tp = (!self.doc.toolpaths.is_empty()).then(|| i.min(self.doc.toolpaths.len() - 1));
    }

    pub fn is_planetcnc(&self) -> bool {
        toolpath::POSTS.get(self.post_index).is_some_and(|p| p.0 == crate::planetcnc::NAME)
    }

    pub fn gcode_text(&self, only: Option<usize>) -> Option<String> {
        self.gcode_text_named(only, "toolpaths")
    }

    /// The machine file's text. `file_name` is what a post processor file can print in it.
    pub fn gcode_text_named(&self, only: Option<usize>, file_name: &str) -> Option<String> {
        let chosen: Vec<&Toolpath> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| only.map_or(t.visible, |o| o == *i)).map(|(_, t)| t).collect();
        if chosen.is_empty() {
            return None;
        }
        let machine = self.doc.machine_on_job();
        // A post processor file sends the machine home itself, in its header and footer if it wants to.
        let own_format = self.post_index < toolpath::POSTS.len();
        let routes: Vec<(Vec<Move>, Option<Vec<crate::arcs::Step>>)> = chosen
            .iter()
            .enumerate()
            .map(|(k, t)| toolpath::route_in_file(&t.moves, t.kept_steps.as_ref().map(|s| s.as_slice()), machine.home, own_format && k == 0, own_format && k + 1 == chosen.len()))
            .collect();
        let items: Vec<toolpath::PostPath> = chosen
            .iter()
            .zip(&routes)
            .map(|(t, (moves, steps))| toolpath::PostPath { name: t.op.name.as_str(), tool: &t.op.tool, moves: moves.as_slice(), dwell: t.op.dwell, steps: steps.as_deref() })
            .collect();
        let j = &self.doc.job;
        if let Some(def) = self.post_index.checked_sub(toolpath::POSTS.len()).and_then(|i| self.loaded_posts.get(i)) {
            let job_metric = j.units == Units::Mm;
            let scale = match (job_metric, def.metric.unwrap_or(job_metric)) {
                (true, false) => 1.0 / 25.4,
                (false, true) => 25.4,
                _ => 1.0,
            };
            let (date, time) = crate::postp::now_strings();
            let info = crate::postp::PostJob {
                origin: j.origin(),
                z_shift: if j.z_on_bed { j.thickness } else { 0.0 },
                scale,
                size: [j.width, j.height, j.thickness],
                home: machine.home,
                safe_z: machine.safe_z,
                file_name,
                date,
                time,
            };
            let tol = if job_metric { 0.012 } else { 0.0005 };
            return Some(crate::postp::render(def, &items, &info, self.output_arcs.then_some(tol)));
        }
        if self.is_planetcnc() {
            let scale = if j.units == Units::Mm { 1.0 } else { 25.4 };
            let (date, time) = crate::postp::now_strings();
            // This file never visits the home position, not even at its start and end.
            let items: Vec<toolpath::PostPath> = chosen
                .iter()
                .map(|t| toolpath::PostPath { name: t.op.name.as_str(), tool: &t.op.tool, moves: t.moves.as_slice(), dwell: t.op.dwell, steps: t.kept_steps.as_ref().map(|s| s.as_slice()) })
                .collect();
            let info = crate::planetcnc::PlanetJob {
                name: file_name,
                created: format!("{date} {time}"),
                material: [j.width * scale, j.height * scale, j.thickness * scale],
                origin: j.origin(),
                z_shift: if j.z_on_bed { j.thickness } else { 0.0 },
                scale,
                arcs: self.output_arcs.then_some(if j.units == Units::Mm { 0.012 } else { 0.0005 }),
            };
            return Some(crate::planetcnc::gcode(&items, &info));
        }
        let (_, _, metric) = toolpath::POSTS[self.post_index.min(toolpath::POSTS.len() - 1)];
        let scale = match (j.units == Units::Mm, metric) {
            (true, false) => 1.0 / 25.4,
            (false, true) => 25.4,
            _ => 1.0,
        };
        Some(toolpath::gcode(
            &items,
            &toolpath::PostSettings { inches: !metric, origin: j.origin(), z_shift: if j.z_on_bed { j.thickness } else { 0.0 }, scale, arcs: self.output_arcs.then_some(if j.units == Units::Mm { 0.012 } else { 0.0005 }) },
        ))
    }

    // ----- Carve preview -----

    pub fn preview(&mut self, queue: Vec<usize>) {
        if queue.is_empty() {
            self.status = "Create a toolpath first.".into();
            return;
        }
        // A toolpath made before the material got thinner would go through it now.
        self.warn_if_too_deep(&queue);
        self.sim_undo = self.sim.clone();
        self.ensure_sim();
        self.anim = Anim { queue, at: 0, step: 0, playing: true, tool_at: None, rush: false };
        self.view3d = true;
        if !self.animate {
            self.rush_preview();
        }
    }

    /// How many points the carve preview uses: fine enough for the job's smallest tool to look like
    /// itself, however big the sheet, up to what a modest computer can hold.
    pub(crate) fn sim_points(&self) -> usize {
        use crate::relief::ToolKind;
        let j = &self.doc.job;
        let finest = self.doc.toolpaths.iter().map(|t| {
            let tool = &t.op.tool;
            // A V-bit's point is far finer than its width.
            let per_tool = if matches!(tool.kind, ToolKind::VBit | ToolKind::Engraving) { 32.0 } else { 8.0 };
            tool.dia / per_tool
        }).filter(|c| *c > 0.0).fold(f64::MAX, f64::min);
        let wanted = if finest == f64::MAX { 0.0 } else { j.width * j.height / (finest * finest) };
        (wanted.ceil() as usize).clamp(1_000_000, 12_000_000)
    }

    fn ensure_sim(&mut self) {
        if self.sim.is_none() {
            let points = self.sim_points();
            let j = &self.doc.job;
            let sim = Sim::new(j.width, j.height, j.thickness, points);
            self.sim_tag = vec![255; sim.grid.data.len()];
            self.sim = Some(sim);
        }
    }

    pub fn reset_preview(&mut self) {
        self.sim = None;
        self.sim_undo = None;
        self.anim = Anim::default();
        self.bump_surface();
    }

    pub fn undo_preview(&mut self) {
        self.sim = self.sim_undo.take();
        self.anim = Anim::default();
        self.bump_surface();
    }

    pub fn rewind_preview(&mut self) {
        let queue = std::mem::take(&mut self.anim.queue);
        self.sim = self.sim_undo.clone();
        self.sim_tag.clear();
        if let Some(s) = &self.sim {
            self.sim_tag = vec![255; s.grid.data.len()];
        }
        self.ensure_sim();
        self.anim = Anim { queue, at: 0, step: 0, playing: false, tool_at: None, rush: false };
        self.bump_surface();
    }

    /// Goes to the end of the carve: at once for the tests, otherwise a little at each frame.
    pub fn rush_preview(&mut self) {
        if self.surface_in_background {
            self.anim.playing = true;
            self.anim.rush = true;
        } else {
            self.finish_preview();
        }
    }

    pub fn finish_preview(&mut self) {
        self.anim.playing = true;
        while self.advance_preview(usize::MAX) {}
        self.bump_surface();
    }

    /// Cuts up to `budget` moves; returns true while more remain.
    pub fn advance_preview(&mut self, budget: usize) -> bool {
        let Some(sim) = self.sim.as_mut() else { return false };
        let mut left = budget;
        while left > 0 {
            let Some(&index) = self.anim.queue.get(self.anim.at) else {
                self.anim.playing = false;
                self.anim.tool_at = None;
                return false;
            };
            let Some(tp) = self.doc.toolpaths.get(index) else {
                self.anim.at += 1;
                continue;
            };
            let all_moves = tp.moves.clone();
            let moves = toolpath::working_moves(&all_moves);
            let tool = &tp.op.tool;
            let cutter = relief::Cutter::new(tool.kind, tool.dia, tool.angle, sim.grid.cell);
            let tag = index.min(254) as u8;
            let end = self.anim.step.saturating_add(left).min(moves.len());
            let from = self.anim.step.max(1);
            for i in from..end {
                let (a, b) = (moves[i - 1], moves[i]);
                if !b.rapid && (a.p[2] < 0.0 || b.p[2] < 0.0) {
                    sim.cut_move(&cutter, &mut self.sim_tag, a.p, b.p, tag);
                }
            }
            left -= (end - self.anim.step.min(end)).max(1).min(left);
            self.anim.step = end;
            if end >= moves.len() {
                self.anim.at += 1;
                self.anim.step = 0;
                self.anim.tool_at = None;
            } else {
                self.anim.tool_at = Some((moves[end.saturating_sub(1)].p, index));
                return true;
            }
        }
        self.anim.at < self.anim.queue.len()
    }
}


pub const TOOLPATH_COLOURS: [[u8; 3]; 6] = [
    [226, 92, 72],
    [72, 134, 226],
    [86, 176, 96],
    [226, 176, 62],
    [160, 96, 206],
    [62, 184, 190],
];

pub const CLIPART: [&str; 9] = ["Heart", "Arrow", "Leaf", "Flower", "Cross", "Moon", "Shield", "Gear", "Drop"];

/// Sample shapes for the Clipart tab, in a unit box around the origin.
pub fn clipart_shape(name: &str) -> Vec<Path> {
    let polar = |f: &dyn Fn(f64) -> f64, n: usize| -> Vec<P> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                let r = f(a);
                [r * a.cos(), r * a.sin()]
            })
            .collect()
    };
    let pts: Vec<P> = match name {
        "Heart" => (0..120)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 120.0;
                let s = t.sin();
                [s * s * s, 0.0625 * (13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos())]
            })
            .collect(),
        "Arrow" => vec![[-1.0, -0.3], [0.2, -0.3], [0.2, -0.7], [1.0, 0.0], [0.2, 0.7], [0.2, 0.3], [-1.0, 0.3]],
        "Leaf" => {
            let upper = (0..=40).map(|i| {
                let t = i as f64 / 40.0;
                [-1.0 + 2.0 * t, 0.6 * (std::f64::consts::PI * t).sin()]
            });
            let lower = (1..40).rev().map(|i| {
                let t = i as f64 / 40.0;
                [-1.0 + 2.0 * t, -0.6 * (std::f64::consts::PI * t).sin() * (1.0 - 0.3 * t)]
            });
            upper.chain(lower).collect()
        }
        "Flower" => polar(&|a| 0.65 + 0.35 * (5.0 * a).cos(), 180),
        "Cross" => vec![
            [-0.3, -1.0], [0.3, -1.0], [0.3, -0.3], [1.0, -0.3], [1.0, 0.3], [0.3, 0.3],
            [0.3, 1.0], [-0.3, 1.0], [-0.3, 0.3], [-1.0, 0.3], [-1.0, -0.3], [-0.3, -0.3],
        ],
        "Moon" => {
            let outer = (0..=60).map(|i| {
                let a = -2.0 + 4.0 * i as f64 / 60.0;
                [a.cos(), a.sin()]
            });
            let inner = (1..60).rev().map(|i| {
                let a = -2.0 + 4.0 * i as f64 / 60.0;
                let x0 = (2.0f64).cos();
                [x0 + (a.cos() - x0) * 0.45, a.sin()]
            });
            outer.chain(inner).collect()
        }
        "Shield" => {
            let mut p = vec![[-0.8, 0.9], [0.8, 0.9]];
            p.extend((0..=40).map(|i| {
                let t = i as f64 / 40.0;
                [0.8 * (1.0 - t * t), 0.9 - 1.9 * t]
            }));
            p.extend((1..40).rev().map(|i| {
                let t = i as f64 / 40.0;
                [-0.8 * (1.0 - t * t), 0.9 - 1.9 * t]
            }));
            p
        }
        "Gear" => polar(&|a| if (a * 12.0 / std::f64::consts::TAU).fract() < 0.5 { 1.0 } else { 0.78 }, 240),
        _ => (0..80)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 80.0;
                [t.sin() * (0.5 * t).sin().powi(2), t.cos()]
            })
            .collect(),
    };
    vec![Path::closed(pts)]
}

/// The tool a saved toolpath used, in a form this program knows.
fn saved_tool(saved: &crate::crv3d::SavedToolpath) -> Tool {
    let mut tool = default_tools().into_iter().find(|t| t.metric && t.kind == relief::ToolKind::EndMill).unwrap_or_else(|| default_tools().remove(0));
    if let Some(t) = &saved.tool {
        tool.name = t.name.clone();
        tool.number = t.number;
        tool.dia = t.dia;
        tool.pass_depth = t.pass_depth;
        tool.feed = t.feed;
        tool.plunge = t.plunge;
        tool.rpm = t.rpm as f64;
        tool.stepover = if t.dia > 0.0 { (t.stepover / t.dia).clamp(0.01, 1.0) } else { tool.stepover };
        let name = t.name.to_lowercase();
        if name.contains("v-bit") || name.contains("v bit") || name.contains("grav") {
            tool.kind = relief::ToolKind::VBit;
            // Named like "V-Bit (60 deg 12 mm)".
            let words: Vec<&str> = name.split_whitespace().collect();
            tool.angle = words.windows(2).find(|w| w[1].starts_with("deg")).and_then(|w| w[0].trim_start_matches('(').parse().ok()).unwrap_or(90.0);
        } else if name.contains("ball") || name.contains("boule") {
            tool.kind = relief::ToolKind::BallNose;
        }
    }
    tool
}

/// The same as `fast_descents`, for the list with true arcs.
fn fast_descent_steps(steps: &[crate::arcs::Step], gap: f64) -> Vec<crate::arcs::Step> {
    use crate::arcs::Step;
    let mut out = Vec::with_capacity(steps.len() + 8);
    let mut at: Option<[f64; 3]> = None;
    for s in steps {
        match s {
            Step::Straight(m) => {
                if let (Some(p), false) = (at, m.rapid) {
                    let straight_down = (p[0] - m.p[0]).abs() < 1e-6 && (p[1] - m.p[1]).abs() < 1e-6 && m.p[2] < p[2];
                    if straight_down && p[2] > gap && m.p[2] < gap {
                        out.push(Step::Straight(Move { p: [m.p[0], m.p[1], gap], rapid: true }));
                    }
                }
                at = Some(m.p);
            }
            Step::Arc { end, .. } => at = Some(*end),
        }
        out.push(*s);
    }
    out
}

/// Sets a toolpath up the way the file says, so that working it out again gives the same cut.
fn saved_options(op: &mut Op, o: &crate::crv3d::SavedOptions) {
    op.climb = o.climb.unwrap_or(op.climb);
    op.tabs = o.tabs;
    op.tab_length = o.tab_length.unwrap_or(op.tab_length);
    op.tab_thickness = o.tab_thickness.unwrap_or(op.tab_thickness);
    op.tab_count = o.tab_count.unwrap_or(op.tab_count);
    op.ramp = o.ramp;
    op.ramp_kind = match o.ramp_kind {
        Some(1) => toolpath::RampKind::ZigZag,
        Some(2) => toolpath::RampKind::Spiral,
        _ => toolpath::RampKind::Smooth,
    };
    op.ramp_angle_limit = o.ramp_angle_limit.unwrap_or(op.ramp_angle_limit);
    op.ramp_distance = o.ramp_distance.unwrap_or(op.ramp_distance);
    op.ramp_by_angle = o.ramp_by_angle;
    op.ramp_angle = o.ramp_angle.unwrap_or(op.ramp_angle);
    op.ramp_on_lead = o.ramp_on_lead;
    op.lead = o.lead;
    op.lead_circular = o.lead_circular;
    op.lead_distance = o.lead_length.unwrap_or(op.lead_distance);
    op.lead_angle = o.lead_angle.unwrap_or(op.lead_angle);
    op.lead_radius = o.lead_radius.unwrap_or(op.lead_radius);
    op.lead_out = o.lead_out;
    op.overcut = o.overcut.unwrap_or(op.overcut);
    op.last_pass = o.last_pass;
    op.last_allowance = o.last_allowance.unwrap_or(op.last_allowance);
    op.last_reverse = o.last_reverse;
    op.sharp_corners = o.sharp_corners;
    if o.keep_starts {
        op.start_at = toolpath::StartAt::Keep;
    } else if let Some(place) = o.nearest_start {
        // The file counts along the rows, top to bottom; the form goes round the box from the top left.
        const ROUND_THE_BOX: [u8; 8] = [0, 1, 2, 7, 3, 6, 5, 4];
        if let Some(spot) = ROUND_THE_BOX.get(place as usize) {
            op.start_at = toolpath::StartAt::Near(*spot);
        }
    }
    op.flat_depth = o.flat_depth.unwrap_or(op.flat_depth);
    if !o.sequence.is_empty() || o.selection_order {
        let has = |code: u32| o.sequence.contains(&code);
        let selection = o.selection_order || has(10);
        let choices = toolpath::OrderChoices { selection, left_to_right: has(0), bottom_to_top: has(2), grid: has(6), shortest: has(8) };
        if op.order_choices.is_some() {
            op.order_choices = Some(choices);
        }
        op.order = if selection { toolpath::Order::AsDrawn } else { toolpath::Order::Nearest };
    }
    op.peck = o.peck;
    op.retract_gap = o.retract_gap.unwrap_or(op.retract_gap);
    op.raster = o.raster;
    op.raster_angle = o.raster_angle.unwrap_or(op.raster_angle);
    op.raster_allowance = o.raster_allowance.unwrap_or(op.raster_allowance);
    op.last_thickness = o.last_thickness.unwrap_or(0.0);
    op.keep_exact = o.keep_exact;
    op.profile_pass = match o.profile_pass {
        Some(1) => toolpath::ProfilePass::First,
        Some(2) => toolpath::ProfilePass::Last,
        Some(0) => toolpath::ProfilePass::None,
        _ => op.profile_pass,
    };
}

fn saved_kind(kind: &str) -> OpKind {
    let kind = kind.to_lowercase();
    if kind.contains("drill") {
        OpKind::Drill
    } else if kind.contains("pocket") || kind.contains("areaclear") {
        OpKind::Pocket
    } else if kind.contains("carve") {
        OpKind::VCarve
    } else if kind.contains("engrav") {
        OpKind::Engrave
    } else {
        OpKind::Profile
    }
}

/// A file keeps each plunge as one move from the travel height. The tool in fact comes down
/// fast to just above the material first, which saves a long slow descent through the air.
fn fast_descents(moves: &[([f64; 3], bool)], gap: f64) -> Vec<([f64; 3], bool)> {
    let mut out = Vec::with_capacity(moves.len() + 8);
    let mut at: Option<[f64; 3]> = None;
    for (q, rapid) in moves {
        if let (Some(p), false) = (at, *rapid) {
            let straight_down = (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6 && q[2] < p[2];
            if straight_down && p[2] > gap && q[2] < gap {
                out.push(([q[0], q[1], gap], true));
            }
        }
        out.push((*q, *rapid));
        at = Some(*q);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working() -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        a
    }

    fn add_circle(a: &mut App, c: P, r: f64) -> u64 {
        let id = a.doc.add_vector(vec![Path::closed(geom::circle(c, r))], Prim::Circle { c, r });
        a.selected = vec![id];
        id
    }

    #[test]
    fn undo_and_redo_restore_shapes() {
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 1.0);
        a.delete_selected();
        assert!(a.doc.vectors.is_empty());
        a.do_undo();
        assert_eq!(a.doc.vectors.len(), 1);
        a.do_redo();
        assert!(a.doc.vectors.is_empty());
    }

    #[test]
    fn project_files_round_trip_and_recalculate() {
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 1.0);
        a.start_op(OpKind::Profile);
        a.calculate_op();
        assert_eq!(a.doc.toolpaths.len(), 1);
        let file = std::env::temp_dir().join(format!("burin-test-{}.burin", std::process::id()));
        a.save_to(file.clone()).unwrap();
        let mut b = App::default();
        b.open_path(file.clone()).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!(b.doc.vectors.len(), 1);
        assert_eq!(b.doc.toolpaths.len(), 1);
        assert!(!b.doc.toolpaths[0].moves.is_empty(), "paths come back after opening");
        assert!(b.open_path(PathBuf::from("/nonexistent/file.burin")).is_err());
    }

    #[test]
    fn changing_units_converts_lengths() {
        let mut d = Doc::default();
        d.set_units(Units::Mm);
        assert!((d.job.width - 254.0).abs() < 1e-9);
        d.set_units(Units::Inches);
        assert!((d.job.width - 10.0).abs() < 1e-9);
    }

    #[test]
    fn datum_and_bed_zero_reach_the_gcode() {
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 1.0);
        a.start_op(OpKind::Drill);
        a.calculate_op();
        a.doc.job.datum = Datum::Centre;
        a.doc.job.z_on_bed = true;
        let text = a.gcode_text(None).unwrap();
        assert!(text.contains("X0.0000 Y0.0000 Z0.0000"), "drill bottom at centre, on the bed:\n{text}");
    }

    #[test]
    fn scale_rotate_and_align_move_the_shape() {
        let mut a = working();
        add_circle(&mut a, [2.0, 3.0], 1.0);
        a.left = LeftForm::Xform(XformForm { sx: 200.0, ..XformForm::new(Xform::Scale, &a.doc.job) });
        a.apply_xform();
        let (lo, hi) = a.selected_bbox().unwrap();
        assert!((hi[0] - lo[0] - 4.0).abs() < 1e-6);
        a.align_selected(Some(0), Some(0));
        let (lo, hi) = a.selected_bbox().unwrap();
        assert!(((lo[0] + hi[0]) / 2.0 - 5.0).abs() < 1e-6);
        assert!(((lo[1] + hi[1]) / 2.0 - 5.0).abs() < 1e-6);
    }

    #[test]
    fn moving_keeps_a_shape_editable_but_scaling_does_not() {
        let mut a = working();
        let id = add_circle(&mut a, [2.0, 3.0], 1.0);
        a.translate_selected(1.0, 1.0);
        let v = a.doc.vectors.iter().find(|v| v.id == id).unwrap();
        assert_eq!(v.prim, Prim::Circle { c: [3.0, 4.0], r: 1.0 });
        a.map_selected(|p| [p[0] * 2.0, p[1]]);
        assert_eq!(a.doc.vectors[0].prim, Prim::Free);
    }

    #[test]
    fn block_and_circular_copies_make_the_right_counts() {
        let mut a = working();
        add_circle(&mut a, [1.0, 1.0], 0.5);
        a.left = LeftForm::Xform(XformForm { cols: 3, rows: 2, ..XformForm::new(Xform::BlockCopy, &a.doc.job) });
        a.apply_xform();
        assert_eq!(a.doc.vectors.len(), 6);
        let mut b = working();
        add_circle(&mut b, [8.0, 5.0], 0.5);
        b.left = LeftForm::Xform(XformForm { count: 4, dx: 5.0, dy: 5.0, ..XformForm::new(Xform::CircularCopy, &b.doc.job) });
        b.apply_xform();
        assert_eq!(b.doc.vectors.len(), 4);
    }

    #[test]
    fn group_ungroup_join_and_weld() {
        let mut a = working();
        let one = add_circle(&mut a, [3.0, 3.0], 1.0);
        let two = add_circle(&mut a, [4.0, 3.0], 1.0);
        a.selected = vec![one, two];
        a.group_selected();
        assert_eq!(a.doc.vectors.len(), 1);
        a.ungroup_selected();
        assert_eq!(a.doc.vectors.len(), 2);
        a.boolean_selected(BoolOp::Weld);
        assert_eq!(a.doc.vectors.len(), 1);
        assert_eq!(a.doc.vectors[0].paths.len(), 1);

        let mut b = working();
        let l1 = b.doc.add_vector(vec![Path::open(vec![[0.0, 0.0], [1.0, 0.0]])], Prim::Free);
        let l2 = b.doc.add_vector(vec![Path::open(vec![[1.0, 0.0], [1.0, 1.0]])], Prim::Free);
        let l3 = b.doc.add_vector(vec![Path::open(vec![[1.0, 1.0], [0.0, 0.0]])], Prim::Free);
        b.selected = vec![l1, l2, l3];
        b.join_with(0.05);
        assert_eq!(b.doc.vectors.len(), 1);
        assert!(b.doc.vectors[0].paths[0].closed, "a loop of lines closes");
    }

    #[test]
    fn profile_flow_ends_in_preview_with_a_cut_through_ring() {
        let mut a = working();
        add_circle(&mut a, [4.3, 6.0], 1.2);
        a.start_op(OpKind::Profile);
        a.calculate_op();
        assert!(matches!(a.right, RightMode::Preview));
        assert!(a.view3d);
        a.preview(vec![0]);
        a.finish_preview();
        let sim = a.sim.as_ref().unwrap();
        assert!(sim.grid.sample(4.3 + 1.325, 6.0) <= -0.49, "the ring goes through");
        assert_eq!(sim.grid.sample(4.3, 6.0), 0.0, "the disc is untouched");
        assert_eq!(sim.grid.sample(1.0, 1.0), 0.0, "the sheet is untouched");
        a.undo_preview();
        assert!(a.sim.is_none());
    }

    #[test]
    fn calculate_without_a_selection_explains_itself() {
        let mut a = working();
        a.start_op(OpKind::Profile);
        a.calculate_op();
        match &a.right {
            RightMode::Op { error: Some(e), .. } => assert!(e.contains("Select")),
            other => panic!("expected an explanation, got {other:?}"),
        }
        assert!(a.doc.toolpaths.is_empty());
    }

    #[test]
    fn model_shapes_feed_3d_cuts() {
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 3.0);
        let form = ShapeForm { profile: Profile::Round, height: 0.4, base: 0.0, combine: Combine::Add, editing: None };
        a.add_component(form);
        a.refresh_model();
        assert!((a.model.as_ref().unwrap().max() - 0.4).abs() < 0.01);
        a.start_op(OpKind::Finish3D);
        if let RightMode::Op { op, .. } = &mut a.right {
            op.boundary = toolpath::Boundary::Material;
            op.raster = true;
        }
        a.calculate_op();
        assert_eq!(a.doc.toolpaths.len(), 1);
        a.preview(vec![0]);
        a.finish_preview();
        let sim = a.sim.as_ref().unwrap();
        assert!(sim.grid.sample(5.0, 5.0) > -0.03, "peak is near the top");
        assert!(sim.grid.sample(0.5, 0.5) < -0.35, "around the shape is cut down");
    }

    #[test]
    fn a_clearance_tool_makes_a_second_path_for_the_wall() {
        let mut a = working();
        a.doc.add_vector(vec![Path::closed(geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
        a.select_all();
        a.start_op(OpKind::Pocket);
        let big = a.doc.tools.iter().find(|t| t.label() == "End Mill (0.5 inch)").unwrap().clone();
        if let RightMode::Op { op, .. } = &mut a.right {
            op.clearance_tool = Some(big);
        }
        a.calculate_op();
        assert_eq!(a.doc.toolpaths.len(), 2);
        assert_eq!(a.doc.toolpaths[0].op.name, "Pocket 1 [Clear]");
        assert_eq!(a.doc.toolpaths[1].op.name, "Pocket 1");
        assert!(a.doc.toolpaths[1].op.rest_only);
    }

    #[test]
    fn output_scales_when_the_machine_file_uses_another_unit() {
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 1.0);
        a.start_op(OpKind::Drill);
        a.calculate_op();
        a.post_index = 1;
        let text = a.gcode_text(None).unwrap();
        assert!(text.contains("G21"), "{text}");
        assert!(text.contains("X127.0000 Y127.0000 Z-12.7000"), "{text}");
    }

    #[test]
    fn a_loaded_post_processor_writes_the_machine_file() {
        let dir = std::env::temp_dir().join(format!("burin-posts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("router.pp");
        std::fs::write(&good, "POST_NAME = \"Router (mm) (*.tap)\"\nFILE_EXTENSION = \"tap\"\nUNITS = \"MM\"\nVAR X_POSITION = [X|C|X|1.1]\nVAR Y_POSITION = [Y|C|Y|1.1]\nVAR Z_POSITION = [Z|C|Z|1.1]\nbegin HEADER\n\"(file [TP_FILENAME])\"\nbegin RAPID_MOVE\n\"G0 [X][Y][Z]\"\nbegin FEED_MOVE\n\"G1 [X][Y][Z]\"\nbegin FOOTER\n\"END\"\n").unwrap();
        std::fs::write(dir.join("notes.pp"), "this is not a post processor").unwrap();
        std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
        let mut a = working();
        add_circle(&mut a, [5.0, 5.0], 1.0);
        a.start_op(OpKind::Drill);
        a.calculate_op();
        let files = App::post_files_in(&dir);
        assert_eq!(files.len(), 2, "only .pp files: {files:?}");
        let (added, failed) = a.load_posts(files.clone());
        assert_eq!(added, 1);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("notes.pp") && failed[0].contains("POST_NAME"), "{failed:?}");
        assert_eq!(a.load_posts(vec![good]).0, 0, "the same file isn't added twice");
        a.post_index = toolpath::POSTS.len();
        assert_eq!(a.post_name(a.post_index), "Router (mm) (*.tap)");
        assert_eq!(a.post_extension(a.post_index), "tap");
        let text = a.gcode_text_named(None, "plate").unwrap();
        assert!(text.starts_with("(file plate)\n"), "{text}");
        assert!(text.contains("G0 X127.0Y127.0Z5.1\nG1 Z-12.7\n"), "the job's inches become the file's mm, and only what changed is written: {text}");
        assert!(text.trim_end().ends_with("END"));
        a.remove_selected_post();
        assert!(a.loaded_posts.is_empty());
        assert_eq!(a.post_index, 0, "falls back to a built-in format");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_sample_project_is_complete() {
        let mut a = App::default();
        a.load_sample();
        assert_eq!(a.doc.vectors.len(), 3);
        assert_eq!(a.doc.components.len(), 2);
        assert_eq!(a.doc.toolpaths.len(), 3);
        assert!(a.gcode_text(None).unwrap().contains("Toolpath: Profile 1"));
    }

    #[test]
    fn every_clipart_shape_is_closed_and_has_area() {
        for name in CLIPART {
            let p = clipart_shape(name);
            assert!(p[0].closed && geom::signed_area(&p[0].pts).abs() > 0.2, "{name}");
        }
    }
}
