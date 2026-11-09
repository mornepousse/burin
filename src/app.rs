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
            Units::Inches => crate::i18n::tr("inches"),
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
            Datum::BottomLeft => crate::i18n::tr("Bottom Left"),
            Datum::BottomRight => crate::i18n::tr("Bottom Right"),
            Datum::TopLeft => crate::i18n::tr("Top Left"),
            Datum::TopRight => crate::i18n::tr("Top Right"),
            Datum::Centre => crate::i18n::tr("Center"),
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
    /// Both sides of the material are cut (the original's Double Sided job type).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub two_sided: bool,
    /// How the material is turned over between the sides: about the Y axis (left to right) when
    /// set, else about the X axis (top to bottom), the original's `ppSideFlipDirection` "Around Y"
    /// and "Around X" (value 0, `0x140602519`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip_around_y: bool,
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
            two_sided: false,
            flip_around_y: false,
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
    Text {
        at: P,
        height: f64,
        text: String,
        /// The font, bold, italic and alignment; Burin's own font, left aligned, when left out.
        #[serde(default, skip_serializing_if = "crate::fonts::TextStyle::is_default")]
        style: crate::fonts::TextStyle,
    },
}

impl Prim {
    /// The same kind of shape, for lines these have become. Only if it really fits them: a shape
    /// that was turned or stretched out of its kind is just lines now.
    pub fn following(&self, old: &[Path], new: &[Path]) -> Prim {
        let (Some((lo, hi)), Some((to_lo, to_hi))) = (geom::bbox(old), geom::bbox(new)) else { return Prim::Free };
        let ratio = |a: f64, b: f64| if a.abs() > 1e-12 { b / a } else { 1.0 };
        let (sx, sy) = (ratio(hi[0] - lo[0], to_hi[0] - to_lo[0]), ratio(hi[1] - lo[1], to_hi[1] - to_lo[1]));
        let place = |p: &P| [to_lo[0] + (p[0] - lo[0]) * sx, to_lo[1] + (p[1] - lo[1]) * sy];
        let fitted = match self {
            Prim::Free => return Prim::Free,
            Prim::Circle { c, r } => Prim::Circle { c: place(c), r: r * sx },
            Prim::Ellipse { c, rx, ry } => Prim::Ellipse { c: place(c), rx: rx * sx, ry: ry * sy },
            Prim::Rect { c, w, h, corner } => Prim::Rect { c: place(c), w: w * sx, h: h * sy, corner: corner * sx },
            Prim::Polygon { c, r, sides } => Prim::Polygon { c: place(c), r: r * sx, sides: *sides },
            Prim::Star { c, outer, inner, points } => Prim::Star { c: place(c), outer: outer * sx, inner: inner * sx, points: *points },
            Prim::Spiral { c, r, turns } => Prim::Spiral { c: place(c), r: r * sx, turns: *turns },
            Prim::Arc { c, r, start, sweep } => Prim::Arc { c: place(c), r: r * sx, start: *start, sweep: *sweep },
            Prim::Text { at, height, text, style } => Prim::Text { at: place(at), height: height * sy, text: text.clone(), style: style.clone() },
        };
        let tolerance = (to_hi[0] - to_lo[0]).max(to_hi[1] - to_lo[1]).max(1e-6) * 1e-6;
        let made = fitted.paths();
        let fits = made.len() == new.len()
            && made.iter().zip(new).all(|(m, n)| m.closed == n.closed && m.pts.len() == n.pts.len() && m.pts.iter().zip(&n.pts).all(|(p, q)| (p[0] - q[0]).abs() <= tolerance && (p[1] - q[1]).abs() <= tolerance));
        if fits {
            fitted
        } else {
            Prim::Free
        }
    }

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
            Prim::Text { at, height, text, style } => crate::fonts::text_paths(text, *height, *at, style),
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
    /// When this is a group: what it holds, each as it was put in. `paths` is theirs together, in
    /// this order, so a group moves, draws and cuts like one shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Vector>,
    /// The piece this belongs to: its shapes stay separate and can each be selected, but move together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece: Option<u64>,
    /// The side of a two-sided job it is on: 0 the top, 1 the bottom (`SetActiveSide`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub face: u8,
    /// The sheet it is on, counted from 0 ("Sheet 1"), as the original keeps each object's sheet.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sheet: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl Vector {
    pub fn is_group(&self) -> bool {
        !self.members.is_empty()
    }

    /// How many of the group's lines belong to this.
    fn line_count(&self) -> usize {
        if self.is_group() {
            self.members.iter().map(Vector::line_count).sum()
        } else {
            self.paths.len()
        }
    }

    /// What the group holds, each with the lines it has now that the group may have been moved or
    /// resized. None if the group's lines no longer add up to what was put in.
    pub fn members_now(&self) -> Option<Vec<Vector>> {
        if !self.is_group() || self.members.iter().map(Vector::line_count).sum::<usize>() != self.paths.len() {
            return None;
        }
        let mut at = 0;
        let mut out = Vec::with_capacity(self.members.len());
        for member in &self.members {
            let lines = &self.paths[at..at + member.line_count()];
            at += lines.len();
            let mut member = member.clone();
            if member.paths != lines {
                member.prim = member.prim.following(&member.paths, lines);
                member.paths = lines.to_vec();
            }
            out.push(member);
        }
        Some(out)
    }

    pub fn holds(&self, id: u64) -> bool {
        self.id == id || self.members.iter().any(|m| m.holds(id))
    }

    /// This and everything in it.
    pub fn each_mut(&mut self, f: &mut impl FnMut(&mut Vector)) {
        f(self);
        for member in &mut self.members {
            member.each_mut(f);
        }
    }

    /// The lines of this and of everything in it that can be cut, by what each goes by.
    fn gather(&self, lines: &[Path], out: &mut std::collections::HashMap<u64, Vec<Path>>) {
        if !self.construction {
            out.insert(self.id, lines.to_vec());
        }
        if self.is_group() && self.members.iter().map(Vector::line_count).sum::<usize>() == lines.len() {
            let mut at = 0;
            for member in &self.members {
                let n = member.line_count();
                member.gather(&lines[at..at + n], out);
                at += n;
            }
        }
    }
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
    /// The side of a two-sided job it cuts: 0 the top, 1 the bottom.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub face: u8,
    /// The sheet that was showing when it was calculated (the original's
    /// `_ActiveSheetWhenCalculated`, 0 in every toolpath of the user's files).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sheet: u8,
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
    /// The tool list's groups, kept even while empty: the original's Tool Database holds them as start and end
    /// markers in the list (types 11 and 12, `0x14086ce30`), made by New Group with tools dragged in after.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_groups: Vec<String>,
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
    /// The side being worked on in a two-sided job: 0 the top, 1 the bottom.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub active_face: u8,
    /// Sheets beyond the first: every sheet is the job's size, each with its own shapes and
    /// toolpaths, and only the active one shows (`vcCadSheetManager`, `ActiveSheetIndex`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub extra_sheets: u8,
    /// How many of a shape nesting lays out, when more than one (the original's per-object
    /// `nestNumCopies`, set from the nesting form by `0x1403c17f0`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub nest_copies: std::collections::BTreeMap<u64, u16>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub active_sheet: u8,
    /// The person's notes about the job, which go on the job sheet.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Named values that sums in number boxes can use as `{name}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<crate::formula::Variable>,
    /// What each piece is called, by the mark on its shapes.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub piece_names: std::collections::BTreeMap<u64, String>,
}

impl Default for Doc {
    fn default() -> Self {
        Doc {
            job: Job::default(),
            vectors: Vec::new(),
            piece_names: Default::default(),
            notes: String::new(),
            variables: Vec::new(),
            layers: vec![Layer::named(crate::i18n::tr("Layer 1"))],
            active_layer: 0,
            components: Vec::new(),
            tools: default_tools(),
            tool_groups: Vec::new(),
            toolpaths: Vec::new(),
            machine: Machine { safe_z: 0.2, home: [0.0, 0.0, 0.8], plunge_gap: 0.2, gap_above: 0.0, gap_below: None },
            next_id: 1,
            constraints: Vec::new(),
            new_construction: false,
            active_face: 0,
            extra_sheets: 0,
            nest_copies: Default::default(),
            active_sheet: 0,
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
            op_in_units(&mut tp.op, k, metric);
            tp.moves = Arc::new(tp.moves.iter().map(|m| Move { p: [m.p[0] * k, m.p[1] * k, m.p[2] * k], rapid: m.rapid, join: m.join }).collect());
            if tp.kept_moves.is_some() {
                tp.kept_moves = Some(tp.moves.clone());
            }
            if let Some(steps) = &tp.kept_steps {
                use crate::arcs::Step;
                tp.kept_steps = Some(Arc::new(steps.iter().map(|s| match s {
                    Step::Straight(m) => Step::Straight(Move { p: [m.p[0] * k, m.p[1] * k, m.p[2] * k], rapid: m.rapid, join: m.join }),
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

    /// Whether a shape is on the side being worked on: always, unless the job is two-sided.
    pub fn on_active_side(&self, v: &Vector) -> bool {
        v.sheet == self.active_sheet && (!self.job.two_sided || v.face == self.active_face)
    }

    /// How many sheets the job has.
    pub fn sheet_count(&self) -> usize {
        1 + self.extra_sheets as usize
    }

    /// Whether a shape shows: on a visible layer and on the side being worked on.
    pub fn shows(&self, v: &Vector) -> bool {
        self.on_active_side(v) && self.layers.get(v.layer).is_none_or(|l| l.visible)
    }

    /// Whether a shape can be chosen: showing, and its layer not locked.
    pub fn can_pick(&self, v: &Vector) -> bool {
        self.on_active_side(v) && self.layers.get(v.layer).is_none_or(|l| l.visible && !l.locked)
    }

    /// Whether a toolpath belongs to the side being worked on.
    pub fn toolpath_on_side(&self, t: &Toolpath) -> bool {
        t.sheet == self.active_sheet && (!self.job.two_sided || t.face == self.active_face)
    }

    pub fn add_vector(&mut self, paths: Vec<Path>, prim: Prim) -> u64 {
        let id = self.new_id();
        self.vectors.push(Vector { id, paths, layer: self.active_layer, prim, construction: self.new_construction, members: Vec::new(), piece: None, face: self.active_face, sheet: self.active_sheet });
        id
    }

    /// The lines of every shape that can be cut, grouped or not, by what it goes by.
    pub fn current_paths(&self) -> std::collections::HashMap<u64, Vec<Path>> {
        let mut out = std::collections::HashMap::new();
        for v in &self.vectors {
            v.gather(&v.paths, &mut out);
        }
        out
    }

    /// What's in the same piece as these shapes, these included.
    pub fn with_their_pieces(&self, ids: &[u64]) -> Vec<u64> {
        let pieces: Vec<u64> = self.vectors.iter().filter(|v| ids.contains(&v.id)).filter_map(|v| v.piece).collect();
        self.vectors.iter().filter(|v| ids.contains(&v.id) || v.piece.is_some_and(|p| pieces.contains(&p))).map(|v| v.id).collect()
    }

    pub fn put_in_piece(&mut self, ids: &[u64], piece: Option<u64>) {
        for v in self.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
            v.piece = piece;
        }
    }

    /// The piece all of these shapes are in, if they share one.
    pub fn shared_piece(&self, ids: &[u64]) -> Option<u64> {
        let mut found = self.vectors.iter().filter(|v| ids.contains(&v.id)).map(|v| v.piece);
        let first = found.next()??;
        found.all(|p| p == Some(first)).then_some(first)
    }

    /// One shape left alone isn't a piece any more.
    pub fn tidy_pieces(&mut self) {
        fn count(v: &Vector, seen: &mut std::collections::HashMap<u64, usize>) {
            if let Some(piece) = v.piece {
                *seen.entry(piece).or_default() += 1;
            }
            for member in &v.members {
                count(member, seen);
            }
        }
        if self.piece_names.is_empty() && !self.vectors.iter().any(|v| v.piece.is_some()) {
            return;
        }
        let mut seen = std::collections::HashMap::new();
        for v in &self.vectors {
            count(v, &mut seen);
        }
        self.piece_names.retain(|p, _| seen.get(p).copied().unwrap_or(0) >= 2);
        for v in &mut self.vectors {
            v.each_mut(&mut |v| {
                if v.piece.is_some_and(|p| seen.get(&p).copied().unwrap_or(0) < 2) {
                    v.piece = None;
                }
            });
        }
    }

    /// Copies made together: a whole piece copied is a new piece, but part of one stays in it.
    pub fn sort_out_copied_pieces(&mut self, copies: &[u64]) {
        let copied: Vec<u64> = self.vectors.iter().filter(|v| copies.contains(&v.id)).filter_map(|v| v.piece).collect();
        let mut done: Vec<u64> = Vec::new();
        for old in copied {
            if done.contains(&old) {
                continue;
            }
            done.push(old);
            let here = |v: &Vector| v.piece == Some(old);
            let among = self.vectors.iter().filter(|v| here(v) && copies.contains(&v.id)).count();
            let others = self.vectors.iter().filter(|v| here(v) && !copies.contains(&v.id)).count();
            if among == others {
                let name = self.next_piece_name();
                let new = self.new_id();
                for v in self.vectors.iter_mut().filter(|v| v.piece == Some(old) && copies.contains(&v.id)) {
                    v.piece = Some(new);
                }
                self.piece_names.insert(new, name);
            }
        }
    }

    /// The shape on the sheet that this is, or that holds it.
    pub fn top_of(&self, id: u64) -> Option<u64> {
        self.vectors.iter().find(|v| v.holds(id)).map(|v| v.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Start,
    Work,
}

/// Both views at once, as the original's Tile Windows commands lay out its two view windows.
/// The view in use goes first: on the left side by side, at the top one above the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tiling {
    /// Tile Windows Vertical (57652, `Page Up`).
    SideBySide,
    /// Tile Windows Horizontal (57651, `Page Down`).
    Stacked,
}

pub const TILE_HORIZONTAL_COMMAND: u32 = 57651;
pub const TILE_VERTICAL_COMMAND: u32 = 57652;
pub const CASCADE_COMMAND: u32 = 57650;

/// Which workspace shows, as Affinity's personas: drawing and modelling, or machining. Each has
/// its own panels; the other's are put away until it's chosen. `Both` shows the two side by side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Persona {
    Both,
    Design,
    Machining,
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
            DrawKind::Circle => crate::i18n::tr("Draw Circle"),
            DrawKind::Ellipse => crate::i18n::tr("Draw Ellipse"),
            DrawKind::Rect => crate::i18n::tr("Draw Rectangle"),
            DrawKind::Polygon => crate::i18n::tr("Draw Polygon"),
            DrawKind::Star => crate::i18n::tr("Draw Star"),
            DrawKind::Polyline => crate::i18n::tr("Draw Lines"),
            DrawKind::Arc => crate::i18n::tr("Draw Arc"),
            DrawKind::Curve => crate::i18n::tr("Draw Curve"),
            DrawKind::Spiral => crate::i18n::tr("Draw Spiral"),
            DrawKind::Text => crate::i18n::tr("Draw Text"),
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
    pub text_style: crate::fonts::TextStyle,
    /// How much of a shape the text fills when fitted to it, in percent.
    pub fill_percent: f64,
    /// Auto layout in the selected shape: the margin, and whether the letters spread to the width.
    pub text_margin: crate::fonts::Margin,
    pub spread_letters: bool,
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
            text: crate::i18n::tr("Text").into(),
            text_height: size * 0.1,
            text_style: Default::default(),
            fill_percent: crate::fonts::FILL_PERCENT,
            text_margin: crate::fonts::Margin::None,
            spread_letters: false,
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
            DrawKind::Text => Prim::Text { at: c, height: self.text_height, text: self.text.clone(), style: self.text_style.clone() },
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
            (DrawKind::Text, Prim::Text { at, height, text, style }) => {
                self.c = *at;
                self.text_height = *height;
                self.text = text.clone();
                self.text_style = style.clone();
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
    /// Nesting may turn parts (the original's `CanRotate`), in steps of this many degrees
    /// (`RotationStepAngle`).
    pub nest_turn: bool,
    pub nest_step: f64,
    /// Small parts may go in the holes of bigger ones (the original's `PartInPart`).
    pub part_in_part: bool,
    /// The last shape chosen is the outline to nest in, instead of the sheet ("Last vector is
    /// nest boundary").
    pub into_boundary: bool,
    /// Nest by each part's true shape rather than its box, and let parts be mirrored to fit
    /// (the original's `CanMirror`, "Mirror parts to find best fit").
    pub by_shape: bool,
    pub nest_mirror: bool,
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
            nest_turn: true,
            part_in_part: true,
            into_boundary: false,
            by_shape: false,
            nest_mirror: false,
            nest_step: crate::edit::NEST_FORM_STEP,
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
    /// The workspace showing.
    pub persona: Persona,
    /// What the two panels held last frame, to switch workspace when that changes.
    pub persona_seen: (u8, u8),
    /// The command search (Ctrl+K): open, what's typed, which line is chosen.
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_pick: usize,
    /// An action asked for by the command search, done as its key would do it.
    pub forced_action: Option<crate::keys::Action>,
    /// The selection's card: an edit under way (one step to undo), and whether width and height go together.
    pub inspector_editing: bool,
    pub inspector_keep_ratio: bool,
    pub view3d: bool,
    /// Both views shown at once, and how.
    pub tiled: Option<Tiling>,
    /// The last drawing form opened, which Space opens again.
    pub last_draw: Option<DrawKind>,
    pub tool: Tool2,
    pub selected: Vec<u64>,
    pub clipboard: Vec<Vector>,
    pub selected_tp: Option<usize>,
    pub selected_comp: Option<usize>,
    pub status: String,
    pub tool_index: usize,
    /// The name being typed for a new group in the tool list, and the groups while the list is open.
    pub new_tool_group: Option<String>,
    /// The group chosen in the tool list, where New makes its tool and New Group its group.
    pub tool_group_picked: Option<String>,
    pub tool_groups_edit: Vec<String>,
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
    /// Cut more slowly near tight corners (`toolpath::slow_corners`), at this share of the rate.
    pub corner_slow: bool,
    pub corner_share: f64,
    /// Write `G64 P` with this tolerance, in millimetres, for LinuxCNC. Not from the original.
    pub blend: bool,
    pub blend_mm: f64,
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
    /// The 3D clipart in the chosen library folder, by name.
    pub clip_models: Vec<(String, Vec<crate::relief::Component>)>,
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
    /// The toolpaths whose shapes have changed since they were calculated.
    pub behind: Vec<usize>,
    /// The material has changed and the toolpaths exist: asking whether to work them out again.
    pub ask_recalc: bool,
    /// The answer was not yet, so all of them are out of date.
    pub all_behind: bool,
    /// The selection whose resizing handles are showing, after a click on what was already selected.
    pub handles_on: Option<Vec<u64>>,
    /// Moving a shape of a piece leaves the rest of the piece where it is.
    pub adjust_piece: bool,
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
    /// Waiting for an answer about unsaved work before doing this.
    pub leave: Option<Leave>,

    // 2D view
    pub scale: f32,
    pub origin: egui::Pos2,
    pub auto_fit: bool,
    pub last_rect: egui::Rect,
    pub snap: bool,
    /// The grid's spacing in job units, as a project file can set it (`vcSnapGrid`); 0 follows the zoom.
    pub grid_spacing: f64,
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
    /// For Preview All Sides: the bottom side carved on its own, as seen from underneath, which
    /// is joined to the top side's carve when that is done.
    pub sim_floor: Option<Sim>,
    /// The original's Multi-Sided View (33839): both sides' toolpaths in the 3D view at once.
    pub multi_sided: bool,
    /// Sculpting a component, while its form is open.
    pub sculpt: Option<crate::sculpt::Sculpt>,
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
    /// Pictures are made a slice at a time between frames instead of on a thread: in a browser,
    /// which has no threads.
    pub surface_sliced: bool,
    pub quality3d: Quality3d,
    /// The keyboard shortcuts in use, and the window that changes them.
    pub keys: crate::keys::Keys,
    pub prefs_open: bool,
    /// The form for merging the visible toolpaths, and what it was last set to.
    pub merge_form: Option<crate::merge::MergeForm>,
    pub merge_choices: crate::merge::MergeChoices,
    /// Where the selection turns about, once moved away from its middle.
    pub rotate_centre: Option<(Vec<u64>, P)>,
    /// A template waiting for a decision on the layers it needs.
    pub template_waiting: Option<crate::templates::Waiting>,
    pub support_open: bool,
    pub help_open: bool,
    /// The Scale Model Z Height window, with the height asked for.
    pub scale_z: Option<f64>,
    pub smooth_form: Option<crate::modeltools::SmoothForm>,
    pub offset_form: Option<crate::modeltools::OffsetForm>,
    pub texture_form: Option<crate::modeltools::TextureForm>,
    pub emboss_form: Option<crate::modeltools::EmbossForm>,
    pub sweep_form: Option<crate::sweep::SweepForm>,
    pub extrude_form: Option<crate::sweep::ExtrudeForm>,
    /// The texture settings last used, offered again.
    pub last_texture: Option<crate::modeltools::TextureForm>,
    pub slice_form: Option<crate::modeltools::SliceForm>,
    /// The Add Draft form's angle while it is open, and the last angle used.
    pub draft_angle: Option<f64>,
    pub last_draft_angle: Option<f64>,
    pub turn_form: Option<crate::modeltools::TurnForm>,
    /// Turn or Spin, and the height, as last used.
    pub turn_mode: Option<bool>,
    pub turn_height: Option<f64>,
    /// The last offset used, which the form starts from next time.
    pub offset_distance: Option<f64>,
    /// For the screen of known states: the text form's font list opens at once.
    pub open_font_list: Option<String>,
    pub variables_form: Option<crate::variables::VariablesForm>,
    /// Why the toolpaths should be recalculated, when it isn't the material.
    pub recalc_note: Option<String>,
    pub print_setup_open: bool,
    /// Kept for the session, as the original's printer settings are.
    pub print_setup: crate::printing::PrintSetup,
    pub help_page: usize,
    /// The form for telling the author something, and where drafts go.
    pub feedback: Option<crate::feedback::FeedbackForm>,
    pub issues_page: Option<String>,
    /// The last draft's address, for checking what it would say.
    pub last_draft: Option<String>,
    pub want_picture: Option<std::sync::Arc<std::sync::Mutex<Option<egui::ColorImage>>>>,
    pub crashes_open: bool,
    pub crash_chosen: usize,
    pub new_crashes: usize,
    pub after_crash_checked: bool,
    /// Where reports and the copy of unsaved work are kept. None in tests and pictures.
    pub data_dir: Option<PathBuf>,
    pub recovery: Option<crate::recovery::Found>,
    pub recovery_at: Option<crate::platform::Instant>,
    pub recovery_every: Option<u64>,
    pub recovery_written: bool,
    /// The person put off deciding about the last copy: it stays as it is for next time.
    pub recovery_left: bool,
    pub notes_open: bool,
    /// Earlier versions kept beside each project, and the window that lists them.
    pub backup_settings: crate::backups::Settings,
    pub backups_open: bool,
    /// The look over the toolpaths before cutting: open, asked for by Save (with which toolpath),
    /// and passed for this one save.
    pub checks_open: bool,
    pub checks_for_save: Option<Option<usize>>,
    pub checks_accepted: bool,
    /// How toolpaths are drawn: each kind of move can be shown or not, and 2D previews filled.
    pub draw_plunges: bool,
    pub draw_retracts: bool,
    pub draw_joins: bool,
    /// The main light turns with the view. Off at the start, as in the original.
    pub light_follows_viewer: bool,
    /// Higher parts cast shadows in the 3D view. Off at the start, as in the original.
    pub shadow_shading: bool,
    /// Draw Modeling Plane, on at first and not kept (`0x142e62668`).
    pub draw_modeling_plane: bool,
    pub solid_previews: bool,
    pub shaded_background: bool,
    pub draw_origin: bool,
    pub curve_fit_open: bool,
    pub curve_fit_tolerance: f64,
    /// Writing the machine file from a toolpath's cut on, after a stop: toolpath, cut from 0.
    pub resume: Option<(usize, usize)>,
    pub resume_open: bool,
    pub resume_choice: (usize, usize),
    /// A value typed while dragging, made exact with Enter.
    pub typed: String,
    pub draw_block: bool,
    pub recent_wanted: Option<PathBuf>,
    /// The picture on the first screen, once it has been shown.
    pub start_logo: Option<egui::TextureHandle>,
    /// The one piece the save form is for, if not all of them.
    pub save_piece: Option<u64>,
    pub guides_locked: bool,
    /// Whether calculating a toolpath shows it in the 3D view at once.
    pub auto_open_3d: bool,
    pub draw_rapids: bool,
    pub prefs_waiting: Option<crate::keys::Action>,
    pub prefs_note: String,
    /// How long the last sharp picture took, and the last preview from its start to that picture.
    pub picture_seconds: Option<f32>,
    pub preview_seconds: Option<f32>,
    pub preview_started: Option<crate::platform::Instant>,
    /// While the tool cuts: not before this, so copying the carve can't take up the window's time.
    pub next_cut_picture: Option<crate::platform::Instant>,
    /// How big the quick picture is, to suit how fast this computer is.
    pub quick_pixels: f32,
    pub surface_rev: u64,
    pub ornament: Option<egui::TextureHandle>,
    pub view_rect: egui::Rect,
}

/// How much of the computer the 3D view may use for a picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality3d {
    Light,
    Balanced,
    Sharp,
}

impl Quality3d {
    pub const ALL: [Quality3d; 3] = [Quality3d::Light, Quality3d::Balanced, Quality3d::Sharp];

    pub fn name(self) -> &'static str {
        match self {
            Quality3d::Light => "Light",
            Quality3d::Balanced => "Balanced",
            Quality3d::Sharp => "Sharp",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Quality3d::Light => crate::i18n::tr("Fastest: the carve's grid as it is. Fine detail shows less sharply."),
            Quality3d::Balanced => crate::i18n::tr("A good picture at once, then cleaner cut walls."),
            Quality3d::Sharp => crate::i18n::tr("The finest picture. It can take a long time on a modest computer."),
        }
    }

    /// The most points the carve is worked out on, from the original's own steps.
    pub fn most_points(self) -> usize {
        match self {
            Quality3d::Light => 4_000_000,
            Quality3d::Balanced => 8_000_000,
            Quality3d::Sharp => 16_000_000,
        }
    }

    /// The most spots looked at per side of a pixel: everywhere, and on the edges.
    pub fn samples(self) -> (usize, usize) {
        match self {
            Quality3d::Light => (1, 3),
            Quality3d::Balanced => (2, 4),
            Quality3d::Sharp => (3, 6),
        }
    }
}

/// How many of the 3D detail choices, from the first, a new job remembers for the next one; the
/// highest isn't kept (`0x140332cc0`: saved when the index is below 3).
pub const NEW_JOB_REMEMBERED_RESOLUTIONS: usize = 3;

fn new_job_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("new-job.json"))
}

/// What a new job starts from: the material of the last new job confirmed, as the original keeps
/// it (`0x140332cc0` saves "NewFileMaterialBlock"; `0x14084b7f0` reads back the units, the Z and
/// X/Y zeros, the size, the job type and its flip). The 3D detail comes along only when it isn't
/// the highest (`0x140332cc0` saves "Resolution" below 3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewJob {
    pub units: Units,
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    pub z_on_bed: bool,
    pub datum: Datum,
    pub two_sided: bool,
    pub flip_around_y: bool,
    pub resolution: Option<usize>,
}

impl NewJob {
    pub fn of(job: &Job) -> NewJob {
        NewJob {
            units: job.units,
            width: job.width,
            height: job.height,
            thickness: job.thickness,
            z_on_bed: job.z_on_bed,
            datum: job.datum,
            two_sided: job.two_sided,
            flip_around_y: job.flip_around_y,
            resolution: (job.resolution < NEW_JOB_REMEMBERED_RESOLUTIONS).then_some(job.resolution),
        }
    }

    /// Puts these on a job, keeping what isn't saved (its zero offset, colours and the rest).
    pub fn apply(&self, job: &mut Job) {
        job.units = self.units;
        if self.width > 0.0 && self.height > 0.0 && self.thickness > 0.0 {
            (job.width, job.height, job.thickness) = (self.width, self.height, self.thickness);
        }
        job.z_on_bed = self.z_on_bed;
        job.datum = self.datum;
        job.two_sided = self.two_sided;
        job.flip_around_y = self.flip_around_y;
        if let Some(r) = self.resolution.filter(|r| *r < NEW_JOB_REMEMBERED_RESOLUTIONS) {
            job.resolution = r;
        }
    }
}

fn saved_new_job() -> Option<NewJob> {
    if cfg!(test) {
        return None;
    }
    let text = crate::platform::read_to_string(new_job_file()?).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remember_new_job(job: &Job) {
    if cfg!(test) {
        return;
    }
    if let (Some(file), Ok(text)) = (new_job_file(), serde_json::to_string_pretty(&NewJob::of(job))) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::platform::write(file, text);
    }
}

fn quality_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("3d-quality.txt"))
}

fn load_quality() -> Quality3d {
    let saved = if cfg!(test) { None } else { quality_file().and_then(|f| crate::platform::read_to_string(f).ok()) };
    Quality3d::ALL.into_iter().find(|q| saved.as_deref().is_some_and(|s| s.trim() == q.name())).unwrap_or(Quality3d::Balanced)
}

/// What would drop the project on screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Leave {
    Close,
    New,
    Open,
    /// Back to the first screen, with the window still open.
    ToStart,
    /// To the project in `recent_wanted`.
    Recent,
}

impl Default for App {
    fn default() -> Self {
        let kept_output = crate::pieces::OutputChoices::load();
        App {
            doc: Doc::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            path: None,
            dirty: false,
            leave: None,
            recent: load_recent(),
            folded: load_folded(),
            screen: Screen::Start,
            left_tab: LeftTab::Drawing,
            left: LeftForm::None,
            right: RightMode::List,
            left_open: true,
            right_open: false,
            // The tests see both sides at once, as most were written that way.
            persona: if cfg!(test) { Persona::Both } else { Persona::Design },
            persona_seen: (0, 0),
            palette_open: false,
            palette_query: String::new(),
            palette_pick: 0,
            forced_action: None,
            inspector_editing: false,
            inspector_keep_ratio: true,
            view3d: false,
            tiled: None,
            last_draw: None,
            tool: Tool2::Select,
            selected: Vec::new(),
            clipboard: Vec::new(),
            selected_tp: None,
            selected_comp: None,
            status: crate::i18n::tr("Ready").into(),
            tool_index: 0,
            new_tool_group: None,
            tool_group_picked: None,
            tool_groups_edit: Vec::new(),
            tool_edit: None,
            tool_target: None,
            picked_tool: None,
            // The first of the list that writes millimetres.
            post_index: usize::from(!cfg!(test)),
            loaded_posts: Vec::new(),
            one_file: crate::pieces::load_one_file(),
            output_arcs: false,
            corner_slow: kept_output.corner_slow,
            corner_share: kept_output.corner_share,
            blend: kept_output.blend,
            blend_mm: kept_output.blend_mm,
            snap_shapes: true,
            show_guides: true,
            snap_held_off: false,
            snap_hit: std::cell::Cell::new(None),
            issues: Vec::new(),
            clip_tab: 0,
            clip_folders: load_clip_folders(),
            clip_folder: 0,
            clip_files: Vec::new(),
            clip_models: Vec::new(),
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
            recalc: None,
            background_recalc: !cfg!(test),
            metric_by_default: !cfg!(test),
            nudging: false,
            shapes_print: None,
            behind: Vec::new(),
            ask_recalc: false,
            all_behind: false,
            handles_on: None,
            adjust_piece: false,
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
            grid_spacing: 0.0,
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
            sim_floor: None,
            multi_sided: false,
            sculpt: None,
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
            surface_sliced: crate::platform::WEB,
            quality3d: load_quality(),
            keys: crate::keys::load(),
            prefs_open: false,
            merge_form: None,
            merge_choices: Default::default(),
            rotate_centre: None,
            template_waiting: None,
            support_open: false,
            help_open: false,
            scale_z: None,
            smooth_form: None,
            offset_form: None,
            texture_form: None,
            emboss_form: None,
            sweep_form: None,
            extrude_form: None,
            last_texture: None,
            slice_form: None,
            draft_angle: None,
            last_draft_angle: None,
            turn_form: None,
            turn_mode: None,
            turn_height: None,
            offset_distance: None,
            open_font_list: None,
            variables_form: None,
            recalc_note: None,
            print_setup_open: false,
            print_setup: Default::default(),
            help_page: 0,
            feedback: None,
            issues_page: crate::feedback::ISSUES_PAGE.map(str::to_string),
            last_draft: None,
            want_picture: None,
            crashes_open: false,
            crash_chosen: 0,
            new_crashes: 0,
            after_crash_checked: false,
            data_dir: None,
            recovery: None,
            recovery_at: None,
            recovery_every: None,
            recovery_written: false,
            recovery_left: false,
            notes_open: false,
            backup_settings: crate::backups::load_settings(),
            backups_open: false,
            checks_open: false,
            checks_for_save: None,
            checks_accepted: false,
            draw_plunges: true,
            draw_retracts: true,
            draw_joins: true,
            light_follows_viewer: false,
            draw_modeling_plane: true,
            shadow_shading: false,
            solid_previews: false,
            shaded_background: crate::viewprefs::load().shaded_background,
            draw_origin: crate::viewprefs::load().draw_origin,
            curve_fit_open: false,
            curve_fit_tolerance: 0.01,
            resume: None,
            resume_open: false,
            resume_choice: (0, 1),
            typed: String::new(),
            draw_block: crate::viewprefs::load().draw_block,
            recent_wanted: None,
            start_logo: None,
            save_piece: None,
            guides_locked: false,
            auto_open_3d: true,
            draw_rapids: true,
            prefs_waiting: None,
            prefs_note: String::new(),
            picture_seconds: None,
            preview_seconds: None,
            preview_started: None,
            next_cut_picture: None,
            // A browser has one thread for everything, so it starts smaller and grows if it keeps up.
            quick_pixels: if cfg!(test) { crate::view3d::MOST_QUICK_PIXELS } else if crate::platform::WEB { 100_000.0 } else { 400_000.0 },
            surface_rev: 1,
            ornament: None,
            view_rect: egui::Rect::NOTHING,
        }
    }
}

fn recent_file() -> Option<PathBuf> {
    // Windows keeps a program's settings in a place of its own.
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("burin").join("recent.txt"))
}

/// A file in the folder where the program keeps its settings.
pub fn settings_file(name: &str) -> Option<PathBuf> {
    if crate::platform::WEB {
        // Kept by the browser for this site (`platform::SETTINGS`).
        return Some(PathBuf::from(crate::platform::SETTINGS).join(name));
    }
    recent_file().map(|f| f.with_file_name(name))
}

fn clip_file() -> Option<PathBuf> {
    recent_file().map(|f| f.with_file_name("clipart-folders.txt"))
}

fn load_clip_folders() -> Vec<PathBuf> {
    if cfg!(test) {
        return Vec::new();
    }
    clip_file()
        .and_then(|f| crate::platform::read_to_string(f).ok())
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
        let _ = crate::platform::write(file, lines.join("\n"));
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
            let _ = crate::platform::write(file, lines.join("\n"));
        }
    }

    /// Brings back the files loaded last time. Ones that have gone are skipped quietly.
    pub fn restore_posts(&mut self) {
        if cfg!(test) {
            return;
        }
        let Some(text) = post_list_file().and_then(|f| crate::platform::read_to_string(f).ok()) else { return };
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
    let text = folded_file().and_then(|f| crate::platform::read_to_string(f).ok()).unwrap_or_default();
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
        let _ = crate::platform::write(file, folded.join("\n"));
    }
}

/// How many recently opened files are kept.
const RECENT_FILES: usize = 4;

fn load_recent() -> Vec<PathBuf> {
    if cfg!(test) {
        return Vec::new();
    }
    recent_file()
        .and_then(|f| crate::platform::read_to_string(f).ok())
        .map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).take(RECENT_FILES).collect())
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
            self.status = crate::i18n::tr("Undid the last change").into();
        }
    }

    pub fn do_redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let was = self.doc.job.clone();
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.after_history();
            self.after_job_history(&was);
            self.status = crate::i18n::tr("Redid the change").into();
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
        // The material of the last new job, as the original keeps it.
        if let Some(saved) = saved_new_job() {
            if saved.units != self.doc.job.units {
                self.doc.set_units(saved.units);
            }
            saved.apply(&mut self.doc.job);
        }
        self.reset_state();
        self.screen = Screen::Work;
        self.left = LeftForm::Job { backup: None, first: true };
        self.status = crate::i18n::tr("Ready").into();
    }

    pub fn reset_state(&mut self) {
        self.handles_on = None;
        self.adjust_piece = false;
        self.ask_recalc = false;
        self.all_behind = false;
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
        // Fitted again even when the window hasn't changed size: another job opened in it.
        self.auto_fit = true;
        self.last_rect = egui::Rect::NOTHING;
        self.path = None;
        self.dirty = false;
        self.measure = None;
        self.envelope = None;
        self.trace_preview = Arc::new(Vec::new());
        self.active_bitmap = 0;
        self.bump_surface();
    }

    /// The 3D view's choices that are kept between sessions.
    pub fn view_prefs(&self) -> crate::viewprefs::ViewPrefs {
        crate::viewprefs::ViewPrefs { draw_origin: self.draw_origin, draw_block: self.draw_block, shaded_background: self.shaded_background }
    }

    pub fn bump_surface(&mut self) {
        self.surface_rev += 1;
    }

    pub fn save_to(&mut self, path: PathBuf) -> Result<(), String> {
        let text = serde_json::to_string(&self.doc).map_err(|e| e.to_string())?;
        if crate::platform::WEB {
            // A download: the browser keeps it, and there's nothing on disk to back up or replace.
            crate::platform::write(&path, text).map_err(|e| tf!("Could not save {}: {e}", path.display()))?;
            self.status = tf!("Saved {}", path.display());
            self.path = Some(path);
            self.dirty = false;
            return Ok(());
        }
        // The version being replaced is kept first. If that fails, the save still goes ahead.
        let no_backup = self.backup_before_saving(&path, text.as_bytes());
        // Written beside it first: a full disk or a crash must not cost the copy already there.
        let beside = path.with_extension("burin-saving");
        let written = crate::platform::write(&beside, text).and_then(|()| std::fs::rename(&beside, &path));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&beside);
            return Err(tf!("Could not save {}: {e}", path.display()));
        }
        self.status = tf!("Saved {}", path.display());
        if let Some(why) = no_backup {
            self.status = format!("{}. {why}", self.status);
        }
        self.remember(path.clone());
        self.path = Some(path);
        self.dirty = false;
        Ok(())
    }

    /// Starts a new job from a .crv3d project file: its material, layers, shapes and toolpaths.
    pub fn open_project(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = crate::platform::read(path).map_err(|e| tf!("Could not open {}: {e}", path.display()))?;
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
        // Cut from two sides, and turned over around which axis (`0x14084a480`). A rotary job
        // opens as one side: Burin has no rotary jobs.
        j.two_sided = p.job_type == 1;
        j.flip_around_y = p.side_flip != 0;
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
            op.notes = saved.notes.clone();
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
            // Joins are marked on the file's own straight moves; the same start and end mark them here.
            let mut joins: std::collections::HashSet<([u64; 3], [u64; 3])> = std::collections::HashSet::new();
            let mut was: Option<[f64; 3]> = None;
            for step in &saved.steps {
                let end = match step {
                    crate::arcs::Step::Straight(m) => {
                        if let (true, Some(from)) = (m.join, was) {
                            joins.insert((from.map(f64::to_bits), m.p.map(f64::to_bits)));
                        }
                        m.p
                    }
                    crate::arcs::Step::Arc { end, .. } => *end,
                };
                was = Some(end);
            }
            let descents = fast_descents(&saved.moves, saved.plunge_gap);
            let moves: Vec<Move> = descents
                .iter()
                .enumerate()
                .map(|(i, &(q, rapid))| {
                    let join = !rapid && i > 0 && joins.contains(&(descents[i - 1].0.map(f64::to_bits), q.map(f64::to_bits)));
                    Move { p: place(q), rapid, join }
                })
                .collect();
            let moves = Arc::new(moves);
            let steps: Vec<crate::arcs::Step> = fast_descent_steps(&saved.steps, saved.plunge_gap)
                .into_iter()
                .map(|s| match s {
                    crate::arcs::Step::Straight(m) => crate::arcs::Step::Straight(Move { p: place(m.p), rapid: m.rapid, join: m.join }),
                    crate::arcs::Step::Arc { end, centre, counter_clockwise } => {
                        let c = place([centre[0], centre[1], 0.0]);
                        crate::arcs::Step::Arc { end: place(end), centre: [c[0], c[1]], counter_clockwise }
                    }
                })
                .collect();
            doc.toolpaths.push(Toolpath { op, visible: true, kept_moves: Some(moves.clone()), kept_steps: Some(Arc::new(steps)), moves, color: palette[k % palette.len()], face: doc.active_face, sheet: doc.active_sheet });
        }
        // Last: toolpaths found their shapes by what each goes by, and still can inside a group.
        for group in &p.groups {
            if let Some(v) = saved_group(&mut doc, &by_mark, group) {
                doc.vectors.push(v);
            }
        }
        // Guide lines: Burin's go straight up and down or across; any at another angle stays behind.
        for g in &p.guides {
            let vertical = g.kind == 1 || g.direction[0].abs() < 1e-9;
            let across = g.kind == 2 || g.direction[1].abs() < 1e-9;
            if vertical {
                doc.guides.push(crate::snap::Guide { vertical: true, at: g.point[0] + shift[0] });
            } else if across {
                doc.guides.push(crate::snap::Guide { vertical: false, at: g.point[1] + shift[1] });
            }
        }
        if !p.notes.is_empty() {
            doc.notes = p.notes.clone();
        }
        // The model's look: the nearest wood Burin has, or a solid colour close to the original's
        // material (colours chosen for Burin).
        if let Some(look) = &p.look {
            let lower = look.to_lowercase();
            let wood = MATERIALS.iter().position(|m| lower.contains(&m.0.to_lowercase().split(' ').next().unwrap_or("").to_string()));
            let solid = if lower.contains("mdf") {
                Some([196, 160, 118])
            } else if lower.contains("granite") || lower.contains("black") {
                Some([46, 46, 50])
            } else if lower.contains("red") {
                Some([190, 44, 38])
            } else if lower.contains("white") {
                Some([236, 236, 232])
            } else {
                None
            };
            match (wood, solid) {
                (Some(w), _) => doc.job.material = Some(w),
                (None, Some(c)) => {
                    doc.job.material = None;
                    doc.job.solid = c;
                }
                _ => {}
            }
        }
        let (shapes, layers, toolpaths) = (p.shapes.len(), p.layers.len().max(1), p.toolpaths.len());
        self.doc = doc;
        self.reset_state();
        self.show_guides = p.guides_shown;
        // The file's snap grid: shown or not, snapping or not, and its spacing.
        if let Some(g) = p.snap_grid {
            self.grid = g.visible;
            self.snap = g.snap;
            if g.spacing > 0.0 {
                self.grid_spacing = g.spacing;
            }
        }
        self.screen = Screen::Work;
        // A separate file from the one it came from: saving asks where. The original is still
        // there, so nothing is lost by leaving until something here changes.
        self.path = None;
        self.dirty = false;
        self.status = tf!("{shapes} shapes on {layers} layers, {toolpaths} toolpaths.");
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
        let text = crate::platform::read_to_string(&path).map_err(|e| tf!("Could not open {}: {e}", path.display()))?;
        let doc: Doc = serde_json::from_str(&text)
            .map_err(|_| tf!("{} is not a project made by this program.", path.display()))?;
        // Notes starting with a dot ask to be read each time the job is opened (`0x1402923d0`).
        self.notes_open = doc.notes.starts_with('.');
        self.doc = doc;
        self.reset_state();
        self.screen = Screen::Work;
        self.recalculate(false);
        self.status = tf!("Opened {}", path.display());
        self.remember(path.clone());
        self.path = Some(path);
        Ok(())
    }

    fn remember(&mut self, path: PathBuf) {
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_FILES);
        if cfg!(test) {
            return;
        }
        if let Some(file) = recent_file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let lines: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
            let _ = crate::platform::write(file, lines.join("\n"));
        }
    }

    /// A small finished project: a plaque with raised shapes, ready to preview.
    pub fn load_sample(&mut self) {
        self.doc = Doc::default();
        self.reset_state();
        self.screen = Screen::Work;
        let border = self.doc.add_vector(vec![Path::closed(geom::rect([5.0, 5.0], 9.0, 9.0, 0.6))], Prim::Rect { c: [5.0, 5.0], w: 9.0, h: 9.0, corner: 0.6 });
        let disc = self.doc.add_vector(vec![Path::closed(geom::circle([5.0, 6.0], 2.2))], Prim::Circle { c: [5.0, 6.0], r: 2.2 });
        let text = Prim::Text { at: [2.2, 1.6], height: 1.1, text: "Burin".into(), style: Default::default() };
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
            self.status = crate::i18n::tr("Type some text first.").into();
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
            self.status = crate::i18n::tr("A rule on this shape kept its size. Change the rule's dimension, or remove the rule.").into();
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
        let mut pairs = Vec::new();
        for mut v in items {
            let original = v.id;
            v.id = self.doc.new_id();
            pairs.push((original, v.id));
            v.layer = self.doc.active_layer;
            // Pasted onto the sheet and side showing, wherever it was copied from.
            v.sheet = self.doc.active_sheet;
            v.face = self.doc.active_face;
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                p[0] += step;
                p[1] -= step;
            }
            v.prim.translate(step, -step);
            self.selected.push(v.id);
            self.doc.vectors.push(v);
        }
        let pasted = self.selected.clone();
        self.doc.sort_out_copied_pieces(&pasted);
        self.doc.join_copies_to_toolpaths(&pairs);
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

    /// What moves when the selection does: the rest of its pieces too, unless adjusting within one.
    pub fn moving_ids(&self) -> Vec<u64> {
        if self.adjust_piece {
            self.selected.clone()
        } else {
            self.doc.with_their_pieces(&self.selected)
        }
    }

    /// The box of what moves when the selection does.
    pub fn moving_bbox(&self) -> Option<(P, P)> {
        let ids = self.moving_ids();
        geom::bbox(self.doc.vectors.iter().filter(|v| ids.contains(&v.id)).flat_map(|v| v.paths.iter()))
    }

    /// Reshapes what moves when the selection does: for what turns or flips a piece as one.
    pub fn map_moving(&mut self, f: impl Fn(P) -> P) {
        let moving = self.moving_ids();
        let keep = std::mem::replace(&mut self.selected, moving);
        self.map_selected(f);
        self.selected = keep;
    }

    pub fn translate_selected(&mut self, dx: f64, dy: f64) {
        let ids = self.moving_ids();
        self.translate_ids(&ids, dx, dy);
    }

    pub fn translate_ids(&mut self, ids: &[u64], dx: f64, dy: f64) {
        for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                p[0] += dx;
                p[1] += dy;
            }
            v.prim.translate(dx, dy);
        }
    }



    /// `ax`/`ay`: -1 left/bottom, 0 centre, 1 right/top of the job; `None` leaves that axis alone.
    pub fn align_selected(&mut self, ax: Option<i32>, ay: Option<i32>) {
        let Some((lo, hi)) = self.moving_bbox() else { return };
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
            self.status = crate::i18n::tr("Offset needs a closed shape.").into();
            return;
        };
        let level = if outward { -distance.abs() } else { distance.abs() };
        let loops = field.contours(level);
        if loops.is_empty() {
            self.status = crate::i18n::tr("That offset leaves nothing: try a smaller distance.").into();
            return;
        }
        self.checkpoint();
        let piece = self.doc.shared_piece(&self.selected);
        let id = self.doc.add_vector(loops.into_iter().map(Path::closed).collect(), Prim::Free);
        self.doc.put_in_piece(&[id], piece);
        self.selected = vec![id];
    }

    pub fn group_selected(&mut self) {
        self.keep_rules(App::group_inner);
    }

    fn group_inner(&mut self) {
        let chosen: Vec<u64> = self.selected.iter().copied().filter(|id| self.doc.vectors.iter().any(|v| v.id == *id)).collect();
        if chosen.len() < 2 {
            self.status = crate::i18n::tr("Select two or more shapes to group.").into();
            return;
        }
        self.checkpoint();
        self.group_now(chosen);
    }

    /// Puts these shapes in a new group, which ends up selected.
    pub(crate) fn group_now(&mut self, chosen: Vec<u64>) {
        self.selected.clear();
        let mut members = Vec::with_capacity(chosen.len());
        for id in chosen {
            if let Some(at) = self.doc.vectors.iter().position(|v| v.id == id) {
                members.push(self.doc.vectors.remove(at));
            }
        }
        // On the layer they share, or else on the active one; either way one that can be seen.
        let layer = if members.iter().all(|m| m.layer == members[0].layer) { members[0].layer } else { self.doc.active_layer };
        self.show_layer(layer);
        let id = self.doc.new_id();
        let paths = members.iter().flat_map(|m| m.paths.iter().cloned()).collect();
        let construction = members.iter().all(|m| m.construction);
        let piece = members[0].piece.filter(|p| members.iter().all(|m| m.piece == Some(*p)));
        self.doc.vectors.push(Vector { id, paths, layer, prim: Prim::Free, construction, members, piece, face: self.doc.active_face, sheet: self.doc.active_sheet });
        self.selected = vec![id];
    }

    fn show_layer(&mut self, layer: usize) {
        if let Some(l) = self.doc.layers.get_mut(layer) {
            l.visible = true;
        }
    }

    /// Makes a piece of the selection. Groups in it come apart first, so each shape can be chosen.
    pub fn make_piece(&mut self) {
        if !self.can_make_piece() {
            self.status = crate::i18n::tr("Select two or more shapes, or a group, to make a piece.").into();
            return;
        }
        self.keep_rules(|a| {
            a.checkpoint();
            let groups: Vec<u64> = a.doc.vectors.iter().filter(|v| a.selected.contains(&v.id) && v.is_group()).map(|v| v.id).collect();
            a.selected.retain(|id| !groups.contains(id));
            for id in groups {
                if let Some(at) = a.doc.vectors.iter().position(|v| v.id == id) {
                    let group = a.doc.vectors.remove(at);
                    let released = a.release(group, true, true);
                    a.selected.extend(released);
                }
            }
            let name = a.doc.next_piece_name();
            let piece = a.doc.new_id();
            a.doc.piece_names.insert(piece, name);
            let count = a.selected.len();
            for v in a.doc.vectors.iter_mut().filter(|v| a.selected.contains(&v.id)) {
                v.piece = Some(piece);
            }
            a.status = tf!("Made a piece of {count} shapes: each can be selected alone, and they move together.");
        });
    }

    pub fn can_make_piece(&self) -> bool {
        self.selected.len() >= 2 || self.doc.vectors.iter().any(|v| self.selected.contains(&v.id) && v.is_group())
    }

    pub fn selection_in_piece(&self) -> bool {
        self.doc.vectors.iter().any(|v| self.selected.contains(&v.id) && v.piece.is_some())
    }

    /// The shapes stay as they are, but no longer move together.
    pub fn release_piece(&mut self) {
        if !self.selection_in_piece() {
            return;
        }
        self.checkpoint();
        let ids = self.doc.with_their_pieces(&self.selected);
        for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
            v.piece = None;
        }
        self.adjust_piece = false;
    }

    pub fn select_piece(&mut self) {
        self.selected = self.doc.with_their_pieces(&self.selected);
    }

    /// Whether something selected can be taken apart.
    pub fn can_ungroup(&self) -> bool {
        self.doc.vectors.iter().any(|v| self.selected.contains(&v.id) && (v.is_group() || v.paths.len() > 1))
    }

    /// Takes the selected groups apart by one level, each shape back on its own layer.
    pub fn ungroup_selected(&mut self) {
        self.ungroup_with(false, true);
    }

    /// `deep` takes apart the groups inside as well. Without `own_layers` everything stays on the
    /// layer the group was on.
    pub fn ungroup_with(&mut self, deep: bool, own_layers: bool) {
        self.keep_rules(|a| a.ungroup_inner(deep, own_layers));
    }

    fn ungroup_inner(&mut self, deep: bool, own_layers: bool) {
        let chosen: Vec<u64> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id) && (v.is_group() || v.paths.len() > 1)).map(|v| v.id).collect();
        if chosen.is_empty() {
            self.status = crate::i18n::tr("Select a group to ungroup.").into();
            return;
        }
        self.checkpoint();
        self.selected.retain(|id| !chosen.contains(id));
        for id in chosen {
            if let Some(at) = self.doc.vectors.iter().position(|v| v.id == id) {
                let group = self.doc.vectors.remove(at);
                let released = self.release(group, deep, own_layers);
                self.selected.extend(released);
            }
        }
    }

    /// Puts what a group holds back on the sheet, and says what's there now.
    fn release(&mut self, group: Vector, deep: bool, own_layers: bool) -> Vec<u64> {
        let members = match group.members_now() {
            Some(members) => members,
            // Lines that were never separate shapes, or a group whose lines have since been changed.
            None => group
                .paths
                .iter()
                .map(|p| Vector { id: self.doc.new_id(), paths: vec![p.clone()], layer: group.layer, prim: Prim::Free, construction: group.construction, members: Vec::new(), piece: None, face: group.face, sheet: group.sheet })
                .collect(),
        };
        let mut ids = Vec::with_capacity(members.len());
        for mut member in members {
            if deep && member.is_group() {
                ids.extend(self.release(member, deep, own_layers));
                continue;
            }
            // A copy of a group holds shapes that go by the same names as the first one's.
            if self.doc.top_of(member.id).is_some() {
                member.id = self.doc.new_id();
            }
            if !own_layers || member.layer >= self.doc.layers.len() {
                member.layer = group.layer;
            }
            self.show_layer(member.layer);
            ids.push(member.id);
            self.doc.vectors.push(member);
        }
        // A toolpath made from the group goes on with what was in it.
        for t in &mut self.doc.toolpaths {
            if let Some(at) = t.op.source.iter().position(|s| *s == group.id) {
                t.op.source.splice(at..=at, ids.iter().copied());
            }
        }
        ids
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
            self.status = crate::i18n::tr("Select two or more closed shapes.").into();
            return;
        }
        let mut result = items[0].paths.clone();
        for v in &items[1..] {
            result = geom::boolean(&result, &v.paths, op);
        }
        if result.is_empty() {
            self.status = crate::i18n::tr("Those shapes don't overlap, so nothing is left.").into();
            return;
        }
        self.checkpoint();
        let sel = std::mem::take(&mut self.selected);
        let piece = self.doc.shared_piece(&sel);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
        self.selected = vec![self.doc.add_vector(result, Prim::Free)];
        self.doc.put_in_piece(&self.selected.clone(), piece);
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
                self.status = crate::i18n::tr("Select a closed shape in the 2D view first.").into();
                return;
            }
            self.checkpoint();
            let n = self.doc.components.len() + 1;
            self.doc.components.push(Component {
                name: tf!("Shape {n}"),
                paths,
                profile: form.profile,
                height: form.height,
                base: form.base,
                combine: form.combine,
                visible: true,
                surface: None,
                whole: false,
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
            self.status = crate::i18n::tr("Select a toolpath to edit.").into();
            return;
        };
        if self.doc.toolpaths[i].op.imported {
            self.message = Some(crate::i18n::tr("This toolpath was read from another program's file: it can't be edited or calculated again here.").into());
            return;
        }
        if self.doc.toolpaths[i].op.merged {
            self.message = Some(crate::i18n::tr("A merged toolpath can't be edited or calculated again. Change the toolpaths it was made from and merge them again.").into());
            return;
        }
        let op = self.doc.toolpaths[i].op.clone();
        // Its shapes are selected again, unless it works from the 3D model.
        if !op.kind.uses_model() {
            let mut chosen: Vec<u64> = Vec::new();
            for top in op.source.iter().filter_map(|id| self.doc.top_of(*id)) {
                if !chosen.contains(&top) {
                    chosen.push(top);
                }
            }
            self.selected = chosen;
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
            first.name = tf!("{} [Clear]", op.name);
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
            self.doc.toolpaths.insert(index, Toolpath { op: o, visible: true, color, moves, kept_moves: None, kept_steps: None, face: self.doc.active_face, sheet: self.doc.active_sheet });
            index += 1;
        }
        self.selected_tp = Some(index - 1);
        self.right = RightMode::Preview;
        self.view3d = self.auto_open_3d;
        self.status = crate::i18n::tr("Ready").into();
    }

    pub fn recalculate_all(&mut self) {
        self.recalculate(true);
    }

    /// `warn` tells of toolpaths that go through the material; opening a file doesn't need that yet.
    fn recalculate(&mut self, warn: bool) {
        if self.recalc.is_some() {
            return;
        }
        self.all_behind = false;
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
                    if tp.op.merged || tp.op.imported {
                        continue;
                    }
                    if tp.op.source.is_empty() {
                        no_shapes += 1;
                    } else {
                        unchanged += 1;
                    }
                }
                // A template's toolpath with no shapes yet waits for them to be chosen.
                None if tp.op.paths.is_empty() && !tp.op.kind.uses_model() => no_shapes += 1,
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
        copy.op.name += crate::i18n::tr(" copy");
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

    /// After the material's settings were accepted: whether what they changed affects the toolpaths.
    /// If so asks about working them out again; otherwise only looks at how deep they go.
    pub fn material_accepted(&mut self, before: &Doc) {
        let sizes = |d: &Doc| (d.job.width, d.job.height, d.job.thickness, d.job.z_on_bed);
        let heights = |d: &Doc| serde_json::to_string(&d.machine).ok();
        let changed = sizes(before) != sizes(&self.doc) || heights(before) != heights(&self.doc);
        if self.doc.toolpaths.is_empty() {
            return;
        }
        if changed {
            self.ask_recalc = true;
        } else {
            let all: Vec<usize> = (0..self.doc.toolpaths.len()).collect();
            self.warn_if_too_deep(&all);
        }
    }

    /// Why this machine file must not be written: it would cut something other than what's on screen.
    pub fn output_problem(&mut self, only: Option<usize>) -> Option<String> {
        if self.all_behind {
            return Some(crate::i18n::tr("The material's settings have changed since the toolpaths were calculated.\n\nUse Recalculate, then save.").into());
        }
        let behind: Vec<usize> = self.follow_tasks().into_iter().map(|(i, _)| i).collect();
        let (two, face) = (self.doc.job.two_sided, self.doc.active_face);
        let chosen = |i: usize, t: &Toolpath| only.map_or(t.visible && (!two || t.face == face), |o| o == i);
        let shapes = self.doc.current_paths();
        let mut old: Vec<&str> = Vec::new();
        for (i, t) in self.doc.toolpaths.iter().enumerate().filter(|(i, t)| chosen(*i, t)) {
            let gone = !t.op.source.is_empty() && !t.op.source.iter().any(|id| shapes.contains_key(id));
            if gone || behind.contains(&i) {
                old.push(&t.op.name);
            }
        }
        if !old.is_empty() {
            return Some(tf!(
                "These toolpaths no longer match their shapes, so the file would cut the old ones:\n\n{}\n\nUse Recalculate, or edit them to choose their shapes again, then save.",
                old.join("\n")
            ));
        }
        let empty: Vec<&str> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| chosen(*i, t) && t.moves.is_empty()).map(|(_, t)| t.op.name.as_str()).collect();
        if !empty.is_empty() {
            return Some(tf!("These toolpaths have no moves, so there's nothing to write for them:\n\n{}\n\nUse Recalculate, or edit them to choose their shapes again, then save.", empty.join("\n")));
        }
        let tools: Vec<&toolpath::Tool> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| chosen(*i, t)).map(|(_, t)| &t.op.tool).collect();
        self.tool_change_problem(&tools)
    }

    /// When the post processor can't change tool and these need more than one.
    pub(crate) fn tool_change_problem(&self, tools: &[&toolpath::Tool]) -> Option<String> {
        let def = self.post_index.checked_sub(toolpath::POSTS.len()).and_then(|i| self.loaded_posts.get(i))?;
        if def.can_change_tool() {
            return None;
        }
        let first = tools.first()?;
        let same = |t: &toolpath::Tool| t.kind == first.kind && t.metric == first.metric && (t.dia - first.dia).abs() < 1e-6 && (t.angle - first.angle).abs() < 1e-6;
        let other = tools.iter().find(|t| !same(t))?;
        Some(tf!(
            "This post processor can't change tool, and these toolpaths use more than one:\n\n{}\n{}\n\nThe machine would cut them all with the first. Untick \"Output all visible toolpaths to one file\" and save each tool's toolpaths separately.",
            first.name, other.name
        ))
    }

    #[cfg(test)]
    pub(crate) fn keep_for_test(&mut self, op: toolpath::Op, moves: Vec<Move>) {
        self.keep_calculated(vec![(op, Arc::new(moves))], None);
    }

    pub fn gcode_text(&self, only: Option<usize>) -> Option<String> {
        self.gcode_text_named(only, "toolpaths")
    }

    /// The machine file's text. `file_name` is what a post processor file can print in it.
    pub fn gcode_text_named(&self, only: Option<usize>, file_name: &str) -> Option<String> {
        let chosen: Vec<&Toolpath> = self.doc.toolpaths.iter().enumerate().filter(|(i, t)| only.map_or(t.visible && self.doc.toolpath_on_side(t), |o| o == *i)).map(|(_, t)| t).collect();
        // Starting again after a stop: the toolpaths before go, and that one starts from its cut.
        if let Some((from, cut)) = self.resume {
            let started = self.doc.toolpaths.get(from).map(|t| {
                let mut t = t.clone();
                t.moves = std::sync::Arc::new(toolpath::moves_from_cut(&t.moves, cut, self.doc.machine_on_job(), t.op.tool.metric));
                t.kept_steps = None;
                t
            });
            let mut list: Vec<&Toolpath> = Vec::new();
            for (i, t) in self.doc.toolpaths.iter().enumerate().filter(|(i, t)| (only.map_or(t.visible && self.doc.toolpath_on_side(t), |o| o == *i) || *i == from) && *i >= from) {
                if i == from {
                    list.push(started.as_ref()?);
                } else {
                    list.push(t);
                }
            }
            return self.gcode_from(&list, file_name);
        }
        self.gcode_from(&chosen, file_name)
    }

    /// Shows another sheet; what was chosen on the one before is let go.
    pub fn show_sheet(&mut self, sheet: usize) {
        let sheet = sheet.min(self.doc.sheet_count() - 1) as u8;
        if sheet != self.doc.active_sheet {
            self.doc.active_sheet = sheet;
            self.selected.clear();
            self.selected_comp = None;
        }
    }

    /// Adds a sheet of the job's size after the others and shows it, as the original names its
    /// sheets "Sheet %d" (`0x14217bfb8`).
    pub fn add_sheet(&mut self) {
        if self.doc.sheet_count() >= u8::MAX as usize {
            return;
        }
        self.checkpoint();
        self.doc.extra_sheets += 1;
        self.show_sheet(self.doc.sheet_count() - 1);
        self.dirty = true;
    }

    /// Removes the sheet showing, with its shapes and toolpaths; the sheets after it move up one.
    pub fn delete_sheet(&mut self) {
        if self.doc.sheet_count() < 2 {
            return;
        }
        self.checkpoint();
        let gone = self.doc.active_sheet;
        self.doc.vectors.retain(|v| v.sheet != gone);
        self.doc.toolpaths.retain(|t| t.sheet != gone);
        for v in &mut self.doc.vectors {
            if v.sheet > gone {
                v.sheet -= 1;
            }
        }
        for t in &mut self.doc.toolpaths {
            if t.sheet > gone {
                t.sheet -= 1;
            }
        }
        self.doc.extra_sheets -= 1;
        self.doc.active_sheet = gone.min(self.doc.extra_sheets);
        self.selected.clear();
        self.dirty = true;
    }

    /// Moves the chosen shapes to another sheet, keeping where they are on it.
    pub fn move_selected_to_sheet(&mut self, sheet: usize) {
        if self.selected.is_empty() || sheet >= self.doc.sheet_count() || sheet == self.doc.active_sheet as usize {
            return;
        }
        self.checkpoint();
        let chosen = self.selected.clone();
        for v in self.doc.vectors.iter_mut().filter(|v| chosen.contains(&v.id)) {
            v.sheet = sheet as u8;
        }
        self.selected.clear();
        self.dirty = true;
    }

    /// A toolpath's machining time, counting the slower corners when they're asked for.
    pub(crate) fn toolpath_seconds(&self, t: &Toolpath, rapid: f64) -> f64 {
        let moves = toolpath::working_moves(&t.moves);
        if self.corner_slow && t.kept_steps.is_none() {
            let (split, slow) = toolpath::slow_corners(moves, t.op.tool.dia, self.corner_share, toolpath::CORNER_REACH);
            return toolpath::estimate_with(&split, &t.op.tool, rapid, t.op.dwell, Some(&slow));
        }
        toolpath::estimate_seconds(moves, &t.op.tool, rapid, t.op.dwell)
    }

    /// How the machine file places a point of the job: less the origin, raised by `z_shift`, times
    /// the scale, as every writer does.
    pub(crate) fn output_space(&self) -> (P, f64, f64) {
        let j = &self.doc.job;
        let job_metric = j.units == Units::Mm;
        let scale = if let Some(def) = self.post_index.checked_sub(toolpath::POSTS.len()).and_then(|i| self.loaded_posts.get(i)) {
            match (job_metric, def.metric.unwrap_or(job_metric)) {
                (true, false) => 1.0 / 25.4,
                (false, true) => 25.4,
                _ => 1.0,
            }
        } else if self.is_planetcnc() {
            if job_metric { 1.0 } else { 25.4 }
        } else {
            let (_, _, metric) = toolpath::POSTS[self.post_index.min(toolpath::POSTS.len() - 1)];
            match (job_metric, metric) {
                (true, false) => 1.0 / 25.4,
                (false, true) => 25.4,
                _ => 1.0,
            }
        };
        (j.origin(), if j.z_on_bed { j.thickness } else { 0.0 }, scale)
    }

    /// The machine file's text for these toolpaths.
    pub(crate) fn gcode_from(&self, chosen: &[&Toolpath], file_name: &str) -> Option<String> {
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
        // Slower near tight corners, when asked: the moves there are split off first. Paths that
        // keep exact arcs from another program's file stay as they are.
        let routes: Vec<(Vec<Move>, Option<Vec<crate::arcs::Step>>, Option<toolpath::SlowMoves>)> = chosen
            .iter()
            .zip(routes)
            .map(|(t, (moves, steps))| match (self.corner_slow, &steps) {
                (true, None) => {
                    let (moves, slow) = toolpath::slow_corners(&moves, t.op.tool.dia, self.corner_share, toolpath::CORNER_REACH);
                    (moves, None, Some(slow))
                }
                _ => (moves, steps, None),
            })
            .collect();
        let items: Vec<toolpath::PostPath> = chosen
            .iter()
            .zip(&routes)
            .map(|(t, (moves, steps, slow))| toolpath::PostPath { name: t.op.name.as_str(), tool: &t.op.tool, moves: moves.as_slice(), dwell: t.op.dwell, steps: steps.as_deref(), notes: t.op.notes.as_str(), slow: slow.as_ref() })
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
                notes: &self.doc.notes,
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
            let planet: Vec<(Vec<Move>, Option<toolpath::SlowMoves>)> = chosen
                .iter()
                .map(|t| match (self.corner_slow, &t.kept_steps) {
                    (true, None) => {
                        let (moves, slow) = toolpath::slow_corners(&t.moves, t.op.tool.dia, self.corner_share, toolpath::CORNER_REACH);
                        (moves, Some(slow))
                    }
                    _ => (t.moves.to_vec(), None),
                })
                .collect();
            let items: Vec<toolpath::PostPath> = chosen
                .iter()
                .zip(&planet)
                .map(|(t, (moves, slow))| toolpath::PostPath { name: t.op.name.as_str(), tool: &t.op.tool, moves: moves.as_slice(), dwell: t.op.dwell, steps: t.kept_steps.as_ref().map(|s| s.as_slice()), notes: t.op.notes.as_str(), slow: slow.as_ref() })
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
            &toolpath::PostSettings { inches: !metric, origin: j.origin(), z_shift: if j.z_on_bed { j.thickness } else { 0.0 }, scale, arcs: self.output_arcs.then_some(if j.units == Units::Mm { 0.012 } else { 0.0005 }), blend: (self.blend && toolpath::post_takes_blend(self.post_index)).then_some(if metric { self.blend_mm } else { self.blend_mm / 25.4 }) },
        ))
    }

    // ----- Carve preview -----

    pub fn preview(&mut self, queue: Vec<usize>) {
        if queue.is_empty() {
            self.status = crate::i18n::tr("Create a toolpath first.").into();
            return;
        }
        // A toolpath made before the material got thinner would go through it now.
        self.warn_if_too_deep(&queue);
        self.preview_started = Some(crate::platform::Instant::now());
        self.preview_seconds = None;
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
        (wanted.ceil() as usize).clamp(1_000_000, self.quality3d.most_points())
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

    /// Chooses how much the 3D view may use, and remembers it for next time. A carve already made
    /// keeps its points until it's reset; only its picture changes.
    pub fn set_quality3d(&mut self, quality: Quality3d) {
        self.quality3d = quality;
        if cfg!(test) {
            return;
        }
        if let Some(file) = quality_file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = crate::platform::write(file, quality.name());
        }
    }

    pub fn reset_preview(&mut self) {
        self.sim = None;
        self.sim_undo = None;
        self.sim_floor = None;
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

    /// Preview All Sides (`0x14033aa10`): every toolpath, shown or not, the active side's first and
    /// then the other side's, and the active side is the one looked at afterwards. Here the active
    /// side's toolpaths carve as usual and the other side's carve a block of their own, as seen from
    /// that side. Turned over the way the job flips, wherever the two meet the material is gone, so
    /// what shows is the active side with every hole that goes right through.
    pub fn preview_all_sides(&mut self) {
        let face = self.doc.active_face;
        let top: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|&i| self.doc.toolpaths[i].face == face).collect();
        let bottom: Vec<usize> = (0..self.doc.toolpaths.len()).filter(|&i| self.doc.toolpaths[i].face != face).collect();
        if top.is_empty() && bottom.is_empty() {
            self.status = crate::i18n::tr("Create a toolpath first.").into();
            return;
        }
        let points = self.sim_points();
        let j = self.doc.job.clone();
        let mut under = Sim::new(j.width, j.height, j.thickness, points);
        for &i in &bottom {
            let tp = &self.doc.toolpaths[i];
            let moves = toolpath::working_moves(&tp.moves);
            let tool = &tp.op.tool;
            let cutter = relief::Cutter::new(tool.kind, tool.dia, tool.angle, under.grid.cell);
            for w in moves.windows(2) {
                if !w[1].rapid && (w[0].p[2] < 0.0 || w[1].p[2] < 0.0) {
                    under.cut_move(&cutter, &mut [], w[0].p, w[1].p, 0);
                }
            }
        }
        self.sim = None;
        self.sim_floor = Some(under);
        // The original turns its Multi-Sided View on when this is done (`0x14033aa10`, `0x140593b70`).
        self.multi_sided = true;
        if top.is_empty() {
            self.ensure_sim();
            self.join_sides();
            self.view3d = true;
            self.bump_surface();
        } else {
            self.preview(top);
        }
    }

    /// Takes away the material wherever the bottom side's carve reaches up to the top side's.
    fn join_sides(&mut self) {
        let (Some(sim), Some(under)) = (self.sim.as_mut(), self.sim_floor.as_ref()) else { return };
        let (w, h, around_y) = (self.doc.job.width, self.doc.job.height, self.doc.job.flip_around_y);
        let t = sim.thickness;
        let (nx, ny, cell) = (sim.grid.nx, sim.grid.ny, sim.grid.cell);
        for iy in 0..ny {
            for ix in 0..nx {
                let (x, y) = (ix as f64 * cell, iy as f64 * cell);
                // Where this spot is on the other side once the block is turned over (the same
                // either way: turning over twice puts everything back).
                let (bx, by) = if around_y { (w - x, y) } else { (x, h - y) };
                let floor = -t - under.grid.sample(bx, by);
                let i = iy * nx + ix;
                if under.grid.sample(bx, by) < -1e-4 && sim.grid.data[i] <= floor + 1e-4 {
                    sim.grid.data[i] = -t;
                    sim.walls[i] = relief::LEVEL;
                }
            }
        }
    }

    /// Cuts up to `budget` moves; returns true while more remain.
    pub fn advance_preview(&mut self, budget: usize) -> bool {
        if self.sim.is_none() {
            return false;
        }
        let more = self.advance_cuts(budget);
        if !more && self.sim_floor.is_some() {
            self.join_sides();
        }
        more
    }

    fn advance_cuts(&mut self, budget: usize) -> bool {
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


/// A toolpath's settings in the other unit: `k` is what lengths are multiplied by.
pub(crate) fn op_in_units(o: &mut toolpath::Op, k: f64, metric: bool) {
    o.tool = o.tool.in_units(metric);
    o.clearance_tool = o.clearance_tool.as_ref().map(|t| t.in_units(metric));
    o.stepover *= k;
    o.retract_gap *= k;
    o.boundary_offset *= k;
    o.gap *= k;
    o.tex_cut = [o.tex_cut[0] * k, o.tex_cut[1] * k];
    o.tex_length = [o.tex_length[0] * k, o.tex_length[1] * k];
    for p in o.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        p[0] *= k;
        p[1] *= k;
    }
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
                        out.push(Step::Straight(Move { p: [m.p[0], m.p[1], gap], rapid: true, join: false }));
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

/// Takes a saved group's shapes off the sheet and makes the group of them. With only one shape
/// found there's nothing to group.
fn saved_group(doc: &mut Doc, by_mark: &std::collections::HashMap<[u8; 16], u64>, group: &crate::crv3d::SavedGroup) -> Option<Vector> {
    let mut members = Vec::new();
    for item in &group.items {
        match item {
            crate::crv3d::SavedItem::Shape(mark) => {
                let at = by_mark.get(mark).and_then(|id| doc.vectors.iter().position(|v| v.id == *id));
                members.extend(at.map(|at| doc.vectors.remove(at)));
            }
            crate::crv3d::SavedItem::Group(inner) => members.extend(saved_group(doc, by_mark, inner)),
        }
    }
    if members.len() < 2 {
        return members.pop();
    }
    let layer = group.layer.unwrap_or(members[0].layer);
    let paths = members.iter().flat_map(|m| m.paths.iter().cloned()).collect();
    Some(Vector { id: doc.new_id(), paths, layer, prim: Prim::Free, construction: false, members, piece: None, face: 0, sheet: 0 })
}

/// Sets a toolpath up the way the file says, so that working it out again gives the same cut.
fn saved_options(op: &mut Op, o: &crate::crv3d::SavedOptions) {
    op.climb = o.climb.unwrap_or(op.climb);
    op.tabs = o.tabs;
    op.zigzag_open = o.zigzag_open;
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

    /// A new job starts from the last new job's material, as the original keeps it, but not
    /// from its highest 3D detail, and never from another job's zero offset.
    #[test]
    fn a_new_job_takes_the_last_new_jobs_material() {
        let mut last = Job::default();
        (last.units, last.width, last.height, last.thickness) = (Units::Mm, 300.0, 200.0, 12.0);
        last.z_on_bed = true;
        last.datum = Datum::Centre;
        last.resolution = 3;
        let saved = NewJob::of(&last);
        assert_eq!(saved.resolution, None, "the highest detail isn't kept");
        let mut next = Job::default();
        next.offset = [1.0, 2.0];
        saved.apply(&mut next);
        assert_eq!((next.units, next.width, next.height, next.thickness, next.z_on_bed, next.datum), (Units::Mm, 300.0, 200.0, 12.0, true, Datum::Centre));
        assert_eq!(next.resolution, 0);
        assert_eq!(next.offset, [1.0, 2.0], "its own offset stays");
        last.resolution = 2;
        NewJob::of(&last).apply(&mut next);
        assert_eq!(next.resolution, 2);
        // It goes through its file and back unchanged.
        let text = serde_json::to_string(&NewJob::of(&last)).unwrap();
        assert_eq!(serde_json::from_str::<NewJob>(&text).unwrap(), NewJob::of(&last));
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
    fn a_circle_comes_out_of_a_group_still_a_circle_where_the_group_went() {
        let mut a = working();
        let one = add_circle(&mut a, [3.0, 3.0], 1.0);
        let two = add_circle(&mut a, [7.0, 3.0], 0.5);
        a.selected = vec![two, one];
        a.group_selected();
        assert_eq!(a.doc.vectors.len(), 1);
        assert!(a.doc.vectors[0].is_group());
        assert_eq!(a.doc.vectors[0].paths.len(), 2);
        a.translate_selected(1.0, 2.0);
        a.ungroup_selected();
        assert_eq!(a.selected, [two, one], "the shapes come back as themselves, and selected");
        let circle = a.doc.vectors.iter().find(|v| v.id == one).unwrap();
        let Prim::Circle { c, r } = circle.prim else { panic!("{:?}", circle.prim) };
        assert!((c[0] - 4.0).abs() < 1e-9 && (c[1] - 5.0).abs() < 1e-9 && (r - 1.0).abs() < 1e-9, "{c:?} {r}");
        let (lo, _) = geom::bbox(&circle.paths).unwrap();
        assert!((lo[0] - 3.0).abs() < 1e-6, "its lines moved too");
    }

    #[test]
    fn a_shape_changed_out_of_its_kind_in_a_group_comes_back_as_lines() {
        let mut a = working();
        let one = add_circle(&mut a, [3.0, 3.0], 1.0);
        let two = add_circle(&mut a, [7.0, 3.0], 1.0);
        a.selected = vec![one, two];
        a.group_selected();
        for p in &mut a.doc.vectors[0].paths {
            for q in &mut p.pts {
                q[0] *= 2.0;
            }
        }
        a.ungroup_selected();
        assert_eq!(a.doc.vectors.len(), 2);
        assert!(a.doc.vectors.iter().all(|v| v.prim == Prim::Free), "a stretched circle isn't one");
    }

    #[test]
    fn groups_inside_groups_come_apart_one_level_or_all_at_once() {
        let mut a = working();
        let (one, two, three) = (add_circle(&mut a, [1.0, 1.0], 0.2), add_circle(&mut a, [2.0, 1.0], 0.2), add_circle(&mut a, [3.0, 1.0], 0.2));
        a.selected = vec![one, two];
        a.group_selected();
        let inner = a.selected[0];
        a.selected = vec![inner, three];
        a.group_selected();
        assert_eq!(a.doc.vectors.len(), 1);
        let outer = a.selected[0];
        a.ungroup_selected();
        assert_eq!(a.selected, [inner, three], "one level");
        a.selected = vec![inner, three];
        a.group_selected();
        assert_ne!(a.selected, [outer]);
        a.ungroup_with(true, true);
        assert_eq!(a.selected, [one, two, three], "all of them");
        assert!(a.doc.vectors.iter().all(|v| !v.is_group()));
    }

    #[test]
    fn a_group_goes_on_the_layer_its_shapes_share_and_gives_them_back_theirs() {
        let mut a = working();
        a.doc.layers.push(Layer::named("Other"));
        a.doc.layers.push(Layer::named("Third"));
        a.doc.active_layer = 1;
        let (one, two) = (add_circle(&mut a, [1.0, 1.0], 0.2), add_circle(&mut a, [2.0, 1.0], 0.2));
        a.doc.active_layer = 2;
        a.doc.layers[1].visible = false;
        a.selected = vec![one, two];
        a.group_selected();
        assert_eq!(a.doc.vectors[0].layer, 1, "not the active layer");
        assert!(a.doc.layers[1].visible, "its layer is shown");
        a.ungroup_selected();

        // On different layers, the group goes on the active one; each shape then goes home.
        a.doc.vectors.iter_mut().find(|v| v.id == two).unwrap().layer = 0;
        a.selected = vec![one, two];
        a.group_selected();
        assert_eq!(a.doc.vectors[0].layer, 2);
        a.ungroup_selected();
        let layer = |a: &App, id| a.doc.vectors.iter().find(|v| v.id == id).unwrap().layer;
        assert_eq!((layer(&a, one), layer(&a, two)), (1, 0));

        // Or all stay with the group.
        a.selected = vec![one, two];
        a.group_selected();
        a.ungroup_with(false, false);
        assert_eq!((layer(&a, one), layer(&a, two)), (2, 2));
    }

    #[test]
    fn toolpaths_go_on_following_shapes_put_in_a_group() {
        use crate::toolpath::OpKind;
        let mut a = working();
        let (one, two) = (add_circle(&mut a, [3.0, 3.0], 1.0), add_circle(&mut a, [7.0, 3.0], 1.0));
        let tool = default_tools()[0].clone();
        let paths = a.doc.vectors[0].paths.clone();
        let mut op = toolpath::blank_op(OpKind::Profile, "One".into(), tool, 0.1, paths);
        op.source = vec![one];
        a.keep_calculated(vec![(op, Arc::new(vec![Move { p: [3.0, 3.0, 0.0], rapid: false, join: false }]))], None);
        a.selected = vec![one, two];
        a.group_selected();
        assert!(a.follow_tasks().is_empty(), "nothing changed yet");
        assert_eq!(a.output_problem(None), None);
        a.translate_selected(1.0, 0.0);
        let tasks = a.follow_tasks();
        assert_eq!(tasks.len(), 1, "it follows the shape inside");
        assert_eq!(tasks[0].1.paths.len(), 1, "that one only");
        // Going back into it selects the group that holds it.
        assert_eq!(a.doc.top_of(one), Some(a.selected[0]));

        // A toolpath made from a whole group goes on with what was in it.
        let group = a.selected[0];
        a.doc.toolpaths[0].op.source = vec![group];
        a.ungroup_selected();
        assert_eq!(a.doc.toolpaths[0].op.source, [one, two]);
    }

    #[test]
    fn a_copied_group_comes_apart_into_shapes_of_their_own() {
        let mut a = working();
        let (one, two) = (add_circle(&mut a, [3.0, 3.0], 1.0), add_circle(&mut a, [7.0, 3.0], 1.0));
        a.selected = vec![one, two];
        a.group_selected();
        let first = a.selected[0];
        a.copy_selected();
        a.paste();
        let second = a.selected[0];
        assert_ne!(first, second);
        a.selected = vec![first, second];
        a.ungroup_selected();
        let mut ids: Vec<u64> = a.doc.vectors.iter().map(|v| v.id).collect();
        assert_eq!(ids.len(), 4);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4, "no two the same");
    }

    #[test]
    fn a_group_is_saved_with_what_it_holds() {
        let mut a = working();
        let (one, two) = (add_circle(&mut a, [3.0, 3.0], 1.0), add_circle(&mut a, [7.0, 3.0], 1.0));
        a.selected = vec![one, two];
        a.group_selected();
        let text = serde_json::to_string(&a.doc).unwrap();
        let back: Doc = serde_json::from_str(&text).unwrap();
        assert_eq!(back.vectors[0].members.len(), 2);
        assert!(matches!(back.vectors[0].members[0].prim, Prim::Circle { .. }));
        // A project saved before groups were kept like this still opens.
        let plain = serde_json::to_string(&Doc::default()).unwrap();
        assert!(serde_json::from_str::<Doc>(&plain).is_ok());
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
        crate::platform::write(&good, "POST_NAME = \"Router (mm) (*.tap)\"\nFILE_EXTENSION = \"tap\"\nUNITS = \"MM\"\nVAR X_POSITION = [X|C|X|1.1]\nVAR Y_POSITION = [Y|C|Y|1.1]\nVAR Z_POSITION = [Z|C|Z|1.1]\nbegin HEADER\n\"(file [TP_FILENAME])\"\nbegin RAPID_MOVE\n\"G0 [X][Y][Z]\"\nbegin FEED_MOVE\n\"G1 [X][Y][Z]\"\nbegin FOOTER\n\"END\"\n").unwrap();
        crate::platform::write(dir.join("notes.pp"), "this is not a post processor").unwrap();
        crate::platform::write(dir.join("readme.txt"), "ignored").unwrap();
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
