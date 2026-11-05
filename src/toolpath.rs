//! Cutting paths: turns shapes and the 3D model into machine moves, and moves into G-code.

use crate::arcs::{fit_arcs, Step};
use crate::geom::{bbox, inside_even_odd, signed_area, Field, Path, P};
use crate::relief::{tool_surface, Grid, ToolKind};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub kind: ToolKind,
    pub dia: f64,
    pub angle: f64,
    pub pass_depth: f64,
    pub stepover: f64,
    pub feed: f64,
    pub plunge: f64,
    pub rpm: f64,
    /// Show the rates per second. They are kept per minute whichever is shown.
    #[serde(default)]
    pub per_second: bool,
    /// The unit this tool's numbers are in.
    #[serde(default)]
    pub metric: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub number: u32,
}

impl Tool {
    pub fn label(&self) -> String {
        let unit = if self.metric { "mm" } else { "inch" };
        let kind = match self.kind {
            ToolKind::EndMill => "End Mill",
            ToolKind::BallNose => "Ball Nose",
            ToolKind::VBit => "V-Bit",
            ToolKind::Drill => "Drill",
            ToolKind::Engraving => "Engraving",
        };
        match self.kind {
            ToolKind::VBit | ToolKind::Engraving => format!("{kind} ({:.0} deg {} {unit})", self.angle, trim(self.dia)),
            _ => format!("{kind} ({} {unit})", trim(self.dia)),
        }
    }

    /// The same tool with its numbers in millimetres or in inches.
    pub fn in_units(&self, metric: bool) -> Tool {
        let mut t = self.clone();
        if self.metric != metric {
            let k = if metric { 25.4 } else { 1.0 / 25.4 };
            t.dia *= k;
            t.pass_depth *= k;
            t.feed *= k;
            t.plunge *= k;
            t.metric = metric;
        }
        t
    }
}

pub fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

pub fn default_tools() -> Vec<Tool> {
    let t = |kind, dia: f64, angle, pass_depth, stepover, feed, plunge, metric: bool, number| {
        let mut tool = Tool {
            per_second: false,
            name: String::new(),
            kind,
            dia,
            angle,
            pass_depth,
            stepover,
            feed,
            plunge,
            rpm: 18000.0,
            metric,
            notes: String::new(),
            number,
        };
        tool.name = tool.label();
        tool
    };
    use ToolKind::*;
    vec![
        t(EndMill, 0.125, 0.0, 0.0625, 0.4, 60.0, 20.0, false, 1),
        t(EndMill, 0.25, 0.0, 0.125, 0.4, 100.0, 30.0, false, 2),
        t(EndMill, 0.5, 0.0, 0.25, 0.4, 120.0, 30.0, false, 3),
        t(BallNose, 0.0625, 0.0, 0.0625, 0.1, 60.0, 20.0, false, 4),
        t(BallNose, 0.125, 0.0, 0.125, 0.1, 100.0, 30.0, false, 5),
        t(BallNose, 0.25, 0.0, 0.25, 0.15, 120.0, 30.0, false, 6),
        t(VBit, 0.5, 60.0, 0.125, 0.1, 80.0, 30.0, false, 7),
        t(VBit, 1.25, 90.0, 0.25, 0.04, 80.0, 30.0, false, 8),
        t(Engraving, 0.02, 90.0, 0.05, 0.5, 60.0, 20.0, false, 9),
        t(Drill, 0.25, 0.0, 0.25, 1.0, 30.0, 15.0, false, 10),
        t(EndMill, 2.0, 0.0, 2.0, 0.4, 1500.0, 500.0, true, 11),
        t(EndMill, 3.0, 0.0, 3.0, 0.4, 2000.0, 600.0, true, 12),
        t(EndMill, 6.0, 0.0, 6.0, 0.4, 2500.0, 800.0, true, 13),
        t(EndMill, 12.0, 0.0, 6.0, 0.4, 3000.0, 800.0, true, 14),
        t(BallNose, 3.0, 0.0, 3.0, 0.1, 2000.0, 600.0, true, 15),
        t(BallNose, 6.0, 0.0, 6.0, 0.15, 2500.0, 800.0, true, 16),
        t(VBit, 12.0, 60.0, 3.0, 0.1, 2000.0, 600.0, true, 17),
        t(VBit, 25.0, 90.0, 6.0, 0.04, 2000.0, 600.0, true, 18),
        t(Drill, 6.0, 0.0, 6.0, 1.0, 800.0, 400.0, true, 19),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpKind {
    Profile,
    Pocket,
    Drill,
    Engrave,
    VCarve,
    Rough3D,
    Finish3D,
    Prism,
    Texture,
    Moulding,
    Flute,
    Chamfer,
}

impl OpKind {
    pub fn title(self) -> &'static str {
        match self {
            OpKind::Profile => "2D Profile Toolpath",
            OpKind::Pocket => "Pocket Toolpath",
            OpKind::Drill => "Drilling Toolpath",
            OpKind::Engrave => "Quick Engrave",
            OpKind::VCarve => "V-Carve / Engraving Toolpath",
            OpKind::Rough3D => "Rough Machining Toolpath",
            OpKind::Finish3D => "Finish Machining Toolpath",
            OpKind::Prism => "Prism Carving Toolpath",
            OpKind::Texture => "Texture Toolpath",
            OpKind::Moulding => "Moulding Toolpath",
            OpKind::Flute => "Fluting Toolpath",
            OpKind::Chamfer => "Chamfer Toolpath",
        }
    }
    pub fn base_name(self) -> &'static str {
        match self {
            OpKind::Profile => "Profile",
            OpKind::Pocket => "Pocket",
            OpKind::Drill => "Drill",
            OpKind::Engrave => "Quick Engrave",
            OpKind::VCarve => "V-Carve",
            OpKind::Rough3D => "3D Roughing",
            OpKind::Finish3D => "3D Finish",
            OpKind::Prism => "Prism Carve",
            OpKind::Texture => "Texture",
            OpKind::Moulding => "Swept Profile",
            OpKind::Flute => "Flute",
            OpKind::Chamfer => "Chamfer",
        }
    }
    pub fn uses_model(self) -> bool {
        matches!(self, OpKind::Rough3D | OpKind::Finish3D)
    }
}

/// How the tool gets down into the cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RampKind {
    /// In one long slope along the path.
    #[default]
    Smooth,
    /// Forward and back along the path until down.
    ZigZag,
    /// Sloping all the way around a closed shape.
    Spiral,
}

/// Where a closed shape's cut begins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum StartAt {
    #[default]
    Keep,
    Optimize,
    /// The point nearest one of the eight places around the shape's box: 0 is top left, then clockwise.
    Near(u8),
}

/// The ways of ordering shapes to try. With several, the one with the shortest travel wins.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderChoices {
    pub selection: bool,
    pub left_to_right: bool,
    pub bottom_to_top: bool,
    pub grid: bool,
    pub shortest: bool,
}

impl Default for OrderChoices {
    fn default() -> Self {
        OrderChoices { selection: false, left_to_right: true, bottom_to_top: true, grid: true, shortest: false }
    }
}

/// Choosing shapes by what they are, rather than by clicking them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorFilter {
    pub open: bool,
    pub closed: bool,
    pub only_circles: bool,
    /// 0 every circle, 1 those matching the tool, 2 those matching `diameter`.
    pub circle_match: u8,
    pub diameter: f64,
    pub tolerance: f64,
    pub visible_layers: bool,
    pub layers: Vec<usize>,
    /// Applied again each time the toolpath is calculated.
    pub associate: bool,
}

impl Default for VectorFilter {
    fn default() -> Self {
        VectorFilter { open: false, closed: false, only_circles: false, circle_match: 0, diameter: 0.0, tolerance: 0.004, visible_layers: true, layers: Vec::new(), associate: false }
    }
}

/// Which ends of a flute taper in depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum FluteEnds {
    #[default]
    Both,
    Start,
    End,
}

fn default_flute_ramp() -> f64 {
    25.0
}

fn default_chamfer_width() -> f64 {
    0.1
}

fn default_ramp_angle() -> f64 {
    20.0
}

fn default_lead_angle() -> f64 {
    90.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Side {
    Outside,
    Inside,
    On,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Boundary {
    #[default]
    Model,
    Material,
    Selected,
}

/// When a clean-up pass around the shape runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ProfilePass {
    #[default]
    Last,
    First,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Order {
    AsDrawn,
    LeftToRight,
    Nearest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Op {
    /// Depth of each pass when set by hand, deepest last. Empty uses the tool's pass depth.
    #[serde(default)]
    pub passes: Vec<f64>,
    #[serde(default)]
    pub ramp_kind: RampKind,
    /// Going down along the lead in, rather than along the cut.
    #[serde(default)]
    pub ramp_on_lead: bool,
    /// The ramp's length comes from its angle and the depth of the pass.
    #[serde(default)]
    pub ramp_by_angle: bool,
    #[serde(default = "default_ramp_angle")]
    pub ramp_angle: f64,
    /// The longest a ramp set by its angle may be; 0 for no limit.
    #[serde(default)]
    pub ramp_angle_limit: f64,
    /// A lead along an arc rather than a straight line.
    #[serde(default)]
    pub lead_circular: bool,
    /// A straight lead's angle to the cut: 90 comes in square.
    #[serde(default = "default_lead_angle")]
    pub lead_angle: f64,
    #[serde(default)]
    pub lead_radius: f64,
    #[serde(default)]
    pub lead_out: bool,
    /// How far a closed cut runs on past where it began.
    #[serde(default)]
    pub overcut: f64,
    /// When set, these decide the order of the shapes instead of `order`.
    #[serde(default)]
    pub order_choices: Option<OrderChoices>,
    #[serde(default)]
    pub start_at: StartAt,
    /// Outside corners stay square rather than following the tool's roundness.
    #[serde(default)]
    pub sharp_corners: bool,
    /// Filters that choose this toolpath's shapes again each time it is calculated.
    #[serde(default)]
    pub selector: Option<VectorFilter>,
    /// How much of a flute's length is spent ramping, in percent.
    #[serde(default = "default_flute_ramp")]
    pub flute_ramp: f64,
    #[serde(default)]
    pub flute_ends: FluteEnds,
    #[serde(default = "default_chamfer_width")]
    pub chamfer_width: f64,
    pub kind: OpKind,
    pub name: String,
    pub tool: Tool,
    pub start_depth: f64,
    pub cut_depth: f64,
    pub side: Side,
    pub climb: bool,
    pub allowance: f64,
    pub last_pass: bool,
    pub last_allowance: f64,
    pub last_reverse: bool,
    pub tabs: bool,
    pub tab_length: f64,
    pub tab_thickness: f64,
    pub tab_count: usize,
    pub ramp: bool,
    pub ramp_distance: f64,
    pub lead: bool,
    pub lead_distance: f64,
    pub order: Order,
    pub start_mid_span: bool,
    pub project: bool,
    pub raster: bool,
    pub flat_depth: f64,
    pub peck: bool,
    pub raster_along_x: bool,
    pub paths: Vec<Path>,
    /// A larger tool that clears the area first.
    #[serde(default)]
    pub clearance_tool: Option<Tool>,
    /// Only cut around the wall: a larger tool has already cleared the rest.
    #[serde(default)]
    pub rest_only: bool,
    #[serde(default)]
    pub raster_angle: f64,
    #[serde(default)]
    pub profile_pass: ProfilePass,
    /// What's left for the last pass, when passes are worked out for a new depth.
    #[serde(default)]
    pub last_thickness: f64,
    /// Passes keep to the tool's pass depth, with a shorter last one, rather than being shared evenly.
    #[serde(default)]
    pub keep_exact: bool,
    /// How much further from the walls raster rows stop, when an outline pass cleans up after them.
    #[serde(default)]
    pub raster_allowance: f64,
    /// Absolute stepover; 0 uses the tool's own.
    #[serde(default)]
    pub stepover: f64,
    #[serde(default)]
    pub retract_gap: f64,
    #[serde(default)]
    pub retract_to_start: bool,
    #[serde(default)]
    pub dwell: f64,
    #[serde(default)]
    pub fill: bool,
    #[serde(default)]
    pub hatch: bool,
    #[serde(default)]
    pub cross_hatch: bool,
    #[serde(default)]
    pub boundary: Boundary,
    #[serde(default)]
    pub boundary_offset: f64,
    /// Roughing: slice in levels rather than one pass along the surface.
    #[serde(default)]
    pub z_level: bool,
    #[serde(default)]
    pub tex_cut: [f64; 2],
    #[serde(default)]
    pub tex_length: [f64; 2],
    #[serde(default)]
    pub tex_variation: f64,
    #[serde(default)]
    pub gap: f64,
    /// The shapes this path was made from.
    #[serde(default)]
    pub source: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub p: [f64; 3],
    pub rapid: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Machine {
    pub safe_z: f64,
    pub home: [f64; 3],
    /// Fast moves come down to this height above the material before cutting.
    #[serde(default = "default_plunge_gap")]
    pub plunge_gap: f64,
    /// Space left above the model's highest point.
    #[serde(default)]
    pub gap_above: f64,
    /// When set, the model sits this far above the bottom of the material instead.
    #[serde(default)]
    pub gap_below: Option<f64>,
}

fn default_plunge_gap() -> f64 {
    0.2
}

impl Machine {
    /// Where the model's height 0 sits, as a distance below the material's top.
    pub fn model_top(&self, thickness: f64, highest: f64) -> f64 {
        match self.gap_below {
            Some(gap) => thickness - gap,
            None => highest + self.gap_above,
        }
    }
}

/// How many depth passes a cut takes.
pub fn pass_count(start: f64, cut: f64, pass_depth: f64) -> usize {
    passes_for(cut - start, pass_depth, false).len()
}

/// A single pass does if the tool's pass depth reaches this far past the depth (MECHANICS.md).
const ONE_PASS_MARGIN: f64 = 1.01;
/// Keeping exactly to the pass depth only adds a shorter pass if what's left is over this share of it.
const EXACT_LEFT_OVER: f64 = 0.01;
/// Sharing evenly only adds a pass if what's left is over this share of the pass depth.
const EVEN_LEFT_OVER: f64 = 0.1;
/// Two passes this close are the same one.
const SAME_PASS: f64 = 1e-5;
/// Below this a pass depth means nothing, and counting by it would never end.
const SMALLEST_PASS: f64 = 1e-6;

/// The depth of each pass, shallowest first, for a total depth and the tool's pass depth.
pub fn passes_for(total: f64, step: f64, keep_exact: bool) -> Vec<f64> {
    if step < SMALLEST_PASS || step * ONE_PASS_MARGIN >= total {
        return vec![total];
    }
    let mut count = (total / step) as usize;
    let left = total - count as f64 * step;
    if keep_exact && left > step * EXACT_LEFT_OVER {
        let mut out = Vec::with_capacity(count + 1);
        let mut at = step;
        while total > at {
            out.push(at);
            at += step;
        }
        out.push(total);
        return out;
    }
    if left > step * EVEN_LEFT_OVER {
        count += 1;
    }
    let count = count.max(1);
    let each = total / count as f64;
    (1..=count).map(|i| i as f64 * each).collect()
}

/// As `passes_for`, with a set thickness left for the last pass. What comes before it is shared evenly.
pub fn passes_with_last(total: f64, step: f64, last: f64) -> Vec<f64> {
    if last <= 0.0 {
        return passes_for(total, step, false);
    }
    let rest = total - last;
    if rest < 0.0 {
        return vec![total];
    }
    let mut out = passes_for(rest, step, false);
    if out.last().is_none_or(|d| (d - total).abs() >= SAME_PASS) {
        out.push(total);
    }
    out
}

/// How far a pass may be off the cut depth and still count as reaching it.
const PASS_CHECK: f64 = 1e-4;

/// The depth of each pass, shallowest first, as positive numbers.
pub fn pass_list(op: &Op) -> Vec<f64> {
    // Hand-set passes only hold for the depth they were set for.
    if op.passes.last().is_some_and(|d| (d - op.cut_depth).abs() < PASS_CHECK) {
        return op.passes.clone();
    }
    let total = op.cut_depth - op.start_depth;
    let passes = if op.last_thickness > 0.0 { passes_with_last(total, op.tool.pass_depth, op.last_thickness) } else { passes_for(total, op.tool.pass_depth, op.keep_exact) };
    passes.into_iter().map(|d| op.start_depth + d).collect()
}

fn depths(op: &Op) -> Vec<f64> {
    pass_list(op).into_iter().map(|d| -d).collect()
}

/// How far below the material's top these moves really go.
pub fn deepest_move(moves: &[Move]) -> f64 {
    moves.iter().filter(|m| !m.rapid).map(|m| -m.p[2]).fold(0.0, f64::max)
}

/// How far below the material's top this toolpath goes; 0 when that depends on a 3D model.
pub fn deepest_cut(op: &Op) -> f64 {
    match op.kind {
        OpKind::Rough3D | OpKind::Finish3D | OpKind::Moulding => 0.0,
        OpKind::VCarve => op.start_depth + op.flat_depth,
        OpKind::Chamfer => op.start_depth + chamfer_depth(op),
        OpKind::Texture => op.start_depth + op.tex_cut[1],
        _ => op.cut_depth.max(op.passes.iter().copied().fold(0.0, f64::max)),
    }
}

struct Emit<'a> {
    moves: Vec<Move>,
    m: Machine,
    /// Added to the cutting height when the path follows the 3D model.
    project: Option<&'a Grid>,
    top: f64,
}

impl Emit<'_> {
    fn z_at(&self, x: f64, y: f64, z: f64) -> f64 {
        match self.project {
            Some(g) => z + g.sample(x, y) as f64 - self.top,
            None => z,
        }
    }
    fn last(&self) -> [f64; 3] {
        self.moves.last().map_or(self.m.home, |m| m.p)
    }
    fn rapid(&mut self, p: [f64; 3]) {
        self.moves.push(Move { p, rapid: true });
    }
    fn feed(&mut self, x: f64, y: f64, z: f64) {
        let z = self.z_at(x, y, z);
        self.moves.push(Move { p: [x, y, z], rapid: false });
    }
    fn retract(&mut self) {
        let l = self.last();
        if l[2] < self.m.safe_z {
            self.rapid([l[0], l[1], self.m.safe_z]);
        }
    }
    /// Lifts, travels, and comes down to the start of a cut.
    fn go_to(&mut self, x: f64, y: f64, z: f64) {
        self.retract();
        let safe = self.m.safe_z;
        self.rapid([x, y, safe]);
        let near = self.z_at(x, y, 0.0) + self.m.plunge_gap;
        if near < safe && near > self.z_at(x, y, z) {
            self.rapid([x, y, near]);
        }
        self.feed(x, y, z);
    }
}

fn segments_meet(a: P, b: P, c: P, d: P) -> bool {
    let side = |p: P, q: P, r: P| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    (side(a, b, c) > 0.0) != (side(a, b, d) > 0.0) && (side(c, d, a) > 0.0) != (side(c, d, b) > 0.0)
}

fn curves_within(a: &Path, b: &Path, limit: f64) -> bool {
    let spans = |p: &Path| -> Vec<(P, P)> { (0..p.pts.len()).map(|i| (p.pts[i], p.pts[(i + 1) % p.pts.len()])).collect() };
    let (sa, sb) = (spans(a), spans(b));
    a.pts.iter().any(|q| distance_to_path(*q, b) < limit)
        || b.pts.iter().any(|q| distance_to_path(*q, a) < limit)
        || sa.iter().any(|s| sb.iter().any(|r| segments_meet(s.0, s.1, r.0, r.1)))
}

/// Sorts closed shapes into sets that can be worked out on their own, each on a grid fitting just
/// that set, so a small hole on a big sheet isn't lost between the grid's points. Shapes go together
/// when their lines come within `reach`; with `nested`, also when one lies inside the other.
fn regions(paths: &[Path], reach: f64, nested: bool) -> Vec<Vec<usize>> {
    let closed: Vec<usize> = (0..paths.len()).filter(|i| paths[*i].closed && paths[*i].pts.len() >= 3).collect();
    let boxes: Vec<Option<(P, P)>> = closed.iter().map(|i| bbox([&paths[*i]])).collect();
    let mut set: Vec<usize> = (0..closed.len()).collect();
    fn root(set: &mut [usize], mut k: usize) -> usize {
        while set[k] != k {
            set[k] = set[set[k]];
            k = set[k];
        }
        k
    }
    for i in 0..closed.len() {
        for j in i + 1..closed.len() {
            let (Some(a), Some(b)) = (boxes[i], boxes[j]) else { continue };
            let apart = a.0[0] > b.1[0] + reach || b.0[0] > a.1[0] + reach || a.0[1] > b.1[1] + reach || b.0[1] > a.1[1] + reach;
            if apart {
                continue;
            }
            let (ri, rj) = (root(&mut set, i), root(&mut set, j));
            if ri == rj {
                continue;
            }
            let (a_path, b_path) = (&paths[closed[i]], &paths[closed[j]]);
            let inside = nested && (inside_even_odd(std::slice::from_ref(a_path), b_path.pts[0]) || inside_even_odd(std::slice::from_ref(b_path), a_path.pts[0]));
            if inside || curves_within(a_path, b_path, reach) {
                set[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for k in 0..closed.len() {
        let r = root(&mut set, k);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some(g) => g.1.push(closed[k]),
            None => groups.push((r, vec![closed[k]])),
        }
    }
    groups.into_iter().map(|g| g.1).collect()
}

/// How long a ramp is for one pass going from `from` down to `z`.
fn ramp_distance(op: &Op, from: f64, z: f64) -> f64 {
    match op.ramp_kind {
        RampKind::Spiral => f64::MAX,
        _ if op.ramp_by_angle => (from - z).abs() / ramp_slope(op),
        // No distance at all means the whole way round.
        _ if op.ramp_distance <= 0.0 => f64::MAX,
        _ => op.ramp_distance,
    }
}

/// How steep a ramp by angle goes. An angle that makes no sense is taken as 45 degrees.
fn ramp_slope(op: &Op) -> f64 {
    let angle = if (0.0..=MOST_RAMP_ANGLE).contains(&op.ramp_angle) { op.ramp_angle } else { FALLBACK_RAMP_ANGLE };
    angle.to_radians().tan()
}

/// How far each leg of a zig-zag goes, and how many times it goes there and back.
fn zig_zag(op: &Op, depth: f64) -> (f64, usize) {
    let by_distance = if op.ramp_distance > 0.0 { op.ramp_distance } else { f64::MAX };
    if op.ramp_kind == RampKind::Spiral {
        return (f64::MAX, 1);
    }
    if !op.ramp_by_angle {
        return (by_distance, 1);
    }
    let slope = ramp_slope(op);
    let wanted = depth / 2.0 / slope;
    let limit = op.ramp_angle_limit;
    if limit <= 0.0 || wanted <= limit {
        return (wanted, 1);
    }
    // Too long a leg: more of them, at the longest allowed.
    let each = slope * limit * 2.0;
    let mut times = (depth / each) as usize;
    if times < 1 {
        times = 1;
    } else if depth - times as f64 * each > each * ZIG_LEFT_OVER {
        times += 1;
    }
    (limit, times)
}

/// The height a peck drill stops at each time, shallowest first.
fn peck_depths(start: f64, cut: f64, step: f64) -> Vec<f64> {
    let depth = cut - start;
    let mut times = ((depth / step + 0.5) as usize).max(1);
    if depth - times as f64 * step > step * PECK_LEFT_OVER {
        times += 1;
    }
    (1..=times).map(|i| -(start + depth * i as f64 / times as f64)).collect()
}

const MOST_RAMP_ANGLE: f64 = 89.0;
const FALLBACK_RAMP_ANGLE: f64 = 45.0;
/// A leg more of zig-zag only if what's left is over this share of one there and back.
const ZIG_LEFT_OVER: f64 = 0.1;
/// A peck more only if what's left is over this share of the peck depth.
const PECK_LEFT_OVER: f64 = 0.01;
/// Shapes shorter than this many millimetres aren't ramped, nor passes under 1% of it.
const SHORTEST_RAMP: f64 = 1.0;

/// The point a distance along a route.
fn along(route: &[P], mut s: f64) -> P {
    for w in route.windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if s <= len && len > 0.0 {
            let t = s / len;
            return [w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t];
        }
        s -= len;
    }
    *route.last().unwrap_or(&[0.0, 0.0])
}

/// Which way is away from the part, seen from a loop: outside to the right of a line.
fn away_from_part(pts: &[P], closed: bool, side: Side) -> f64 {
    // Going anticlockwise around a shape, its outside is on the right.
    let right_is_outside = !closed || signed_area(pts) > 0.0;
    match (side, right_is_outside) {
        (Side::Inside, true) | (Side::Outside | Side::On, false) => -1.0,
        _ => 1.0,
    }
}

/// The points of a lead reaching `p`, whose direction of travel there is `t`. Coming in, they run
/// from far to near; going out (`out`), they run from `p` away.
fn lead_points(p: P, t: P, side_sign: f64, op: &Op, out: bool) -> Vec<P> {
    // The right of travel, turned towards the side away from the part.
    let n = [t[1] * side_sign, -t[0] * side_sign];
    let along = if out { 1.0 } else { -1.0 };
    let mut points: Vec<P> = if op.lead_circular && op.lead_radius > 0.0 {
        let r = op.lead_radius;
        // Never more than a quarter of the way round.
        let sweep = (op.lead_distance / r).min(std::f64::consts::FRAC_PI_2);
        let steps = ((sweep / 0.15).ceil() as usize).max(3);
        (1..=steps)
            .map(|i| {
                let a = sweep * i as f64 / steps as f64;
                let (forward, sideways) = (r * a.sin() * along, r * (1.0 - a.cos()));
                [p[0] + t[0] * forward + n[0] * sideways, p[1] + t[1] * forward + n[1] * sideways]
            })
            .collect()
    } else {
        // An angle outside this range comes in square to the cut.
        let a = if (0.0..90.0).contains(&op.lead_angle) { op.lead_angle } else { 90.0 }.to_radians();
        let (forward, sideways) = (op.lead_distance * a.cos() * along, op.lead_distance * a.sin());
        vec![[p[0] + t[0] * forward + n[0] * sideways, p[1] + t[1] * forward + n[1] * sideways]]
    };
    if !out {
        points.reverse();
    }
    points
}

/// Starts a closed loop at the one of its points nearest to where the tool is.
fn start_near(pts: &mut [P], at: P) {
    let far = |p: P| (p[0] - at[0]).hypot(p[1] - at[1]);
    let best = (0..pts.len()).min_by(|a, b| far(pts[*a]).total_cmp(&far(pts[*b])));
    if let Some(best) = best {
        pts.rotate_left(best);
    }
}

/// Starts a closed loop at its point nearest to `target`, adding one there if need be.
fn start_loop_near_point(pts: &mut Vec<P>, target: P) {
    let n = pts.len();
    let mut best = (f64::MAX, 0, target);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((target[0] - a[0]) * dx + (target[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
        let q = [a[0] + dx * t, a[1] + dy * t];
        let d = (q[0] - target[0]).hypot(q[1] - target[1]);
        if d < best.0 {
            best = (d, i, q);
        }
    }
    pts.rotate_left((best.1 + 1) % n);
    let first = pts[0];
    let last = pts[n - 1];
    if (first[0] - best.2[0]).hypot(first[1] - best.2[1]) > 1e-9 && (last[0] - best.2[0]).hypot(last[1] - best.2[1]) > 1e-9 {
        pts.insert(0, best.2);
    } else if (last[0] - best.2[0]).hypot(last[1] - best.2[1]) <= 1e-9 {
        pts.rotate_right(1);
    }
}

/// Starts each worked-out loop level with the start of the shape it came from.
fn start_from_shapes(items: &mut [(Vec<P>, bool)], paths: &[Path], reach: f64) {
    let starts: Vec<P> = paths.iter().filter(|p| p.closed && !p.pts.is_empty()).map(|p| p.pts[0]).collect();
    for (pts, closed) in items.iter_mut() {
        if !*closed || pts.len() < 2 {
            continue;
        }
        let Some((lo, hi)) = bbox([&Path::closed(pts.clone())]) else { continue };
        let line = Path::closed(pts.clone());
        let near = starts.iter().filter(|s| s[0] >= lo[0] - reach && s[0] <= hi[0] + reach && s[1] >= lo[1] - reach && s[1] <= hi[1] + reach);
        if let Some(start) = near.min_by(|a, b| distance_to_path(**a, &line).total_cmp(&distance_to_path(**b, &line))) {
            start_loop_near_point(pts, *start);
        }
    }
}

/// A point on the box around a shape, counted round from the top left.
fn spot_on_box(pts: &[P], place: u8) -> Option<P> {
    let (lo, hi) = bbox([&Path::closed(pts.to_vec())])?;
    let (mx, my) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    Some([[lo[0], hi[1]], [mx, hi[1]], [hi[0], hi[1]], [hi[0], my], [hi[0], lo[1]], [mx, lo[1]], [lo[0], lo[1]], [lo[0], my]][place.min(7) as usize])
}

/// An open line starts from whichever end is nearer the chosen point.
fn start_line_at(pts: &mut [P], place: u8) {
    let Some(target) = spot_on_box(pts, place) else { return };
    let far = |p: P| (p[0] - target[0]).hypot(p[1] - target[1]);
    if far(pts[pts.len() - 1]) < far(pts[0]) {
        pts.reverse();
    }
}

fn distance_to_path(q: P, path: &Path) -> f64 {
    let n = path.pts.len();
    let spans = if path.closed { n } else { n.saturating_sub(1) };
    let mut best = path.pts.first().map_or(f64::MAX, |p| (p[0] - q[0]).hypot(p[1] - q[1]));
    for i in 0..spans {
        let (a, b) = (path.pts[i], path.pts[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
        best = best.min((a[0] + dx * t - q[0]).hypot(a[1] + dy * t - q[1]));
    }
    best
}

/// Whether a tool of radius `radius` following a loop stays off every shape.
fn stays_clear(route: &[P], paths: &[Path], radius: f64) -> bool {
    let n = route.len();
    (0..n).all(|i| {
        let (a, b) = (route[i], route[(i + 1) % n]);
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / (radius / 4.0).max(1e-6)).ceil() as usize).clamp(1, 200);
        (0..steps).all(|k| {
            let t = k as f64 / steps as f64;
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            paths.iter().all(|p| distance_to_path(q, p) >= radius * 0.999) && !inside_even_odd(paths, q)
        })
    })
}

/// An outline `d` outside a closed shape whose corners stay square, not rounded by the tool.
fn sharp_offset(path: &Path, d: f64) -> Vec<P> {
    let mut pts = path.pts.clone();
    if signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (a, p, b) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (la, lb) = ((p[0] - a[0]).hypot(p[1] - a[1]).max(1e-12), (b[0] - p[0]).hypot(b[1] - p[1]).max(1e-12));
        // Outward is to the right when going anticlockwise.
        let (n1, n2) = ([(p[1] - a[1]) / la, -(p[0] - a[0]) / la], [(b[1] - p[1]) / lb, -(b[0] - p[0]) / lb]);
        let (mx, my) = (n1[0] + n2[0], n1[1] + n2[1]);
        let len = mx.hypot(my).max(1e-12);
        let cos_half = (len / 2.0).max(1e-3);
        // A very sharp point would run far off: past that it is cut across.
        if 1.0 / cos_half > 4.0 {
            out.push([p[0] + n1[0] * d, p[1] + n1[1] * d]);
            out.push([p[0] + n2[0] * d, p[1] + n2[1] * d]);
        } else {
            let k = d / cos_half / len;
            out.push([p[0] + mx * k, p[1] + my * k]);
        }
    }
    out
}

/// Cuts along a loop at depth `z`, optionally ramping in and leaving tabs.
fn walk(e: &mut Emit, pts: &[P], closed: bool, z: f64, op: &Op, ramp_from: Option<f64>, tab_z: Option<f64>) {
    let ramp_length = ramp_from.map_or(0.0, |from| ramp_distance(op, from, z));
    if pts.len() < 2 {
        return;
    }
    let mut route: Vec<P> = pts.to_vec();
    if closed {
        route.push(pts[0]);
    }
    let mut stations = vec![0.0];
    for w in route.windows(2) {
        stations.push(stations.last().unwrap() + ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt());
    }
    let total = *stations.last().unwrap();
    if total <= 0.0 {
        return;
    }
    let tabs: Vec<(f64, f64)> = match tab_z {
        Some(_) if op.tab_count > 0 => (0..op.tab_count)
            .map(|i| {
                let mid = total * (i as f64 + 0.5) / op.tab_count as f64;
                (mid - op.tab_length / 2.0, mid + op.tab_length / 2.0)
            })
            .collect(),
        _ => Vec::new(),
    };
    let ramp_len = ramp_length.min(total);
    let height = |s: f64, after_tab_edge: bool| -> f64 {
        let mut h = z;
        if let Some(from) = ramp_from {
            if s < ramp_len {
                h = from + (z - from) * s / ramp_len.max(1e-9);
            }
        }
        if let Some(top) = tab_z {
            let eps = if after_tab_edge { 1e-9 } else { -1e-9 };
            if tabs.iter().any(|(a, b)| s + eps > *a && s + eps < *b) {
                h = h.max(top);
            }
        }
        h
    };
    let mut breaks: Vec<f64> = stations.clone();
    for (a, b) in &tabs {
        breaks.push(*a);
        breaks.push(*b);
    }
    if ramp_len > 0.0 {
        breaks.push(ramp_len);
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let position = |s: f64| -> P {
        let i = stations.partition_point(|v| *v < s).clamp(1, route.len() - 1);
        let (a, b) = (route[i - 1], route[i]);
        let span = (stations[i] - stations[i - 1]).max(1e-12);
        let t = ((s - stations[i - 1]) / span).clamp(0.0, 1.0);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    };
    // When following a model, keep steps short enough to trace its surface.
    let step = e.project.map(|g| g.cell);
    let mut prev: Option<f64> = None;
    for s in breaks.iter().copied() {
        let p = position(s);
        if let (Some(cell), Some(prev)) = (step, prev) {
            let n = ((s - prev) / cell).floor() as usize;
            for k in 1..=n {
                let si = prev + (s - prev) * k as f64 / (n + 1) as f64;
                let q = position(si);
                e.feed(q[0], q[1], height(si, true));
            }
        }
        let (before, after) = (height(s, false), height(s, true));
        if prev.is_some() {
            e.feed(p[0], p[1], before);
        }
        if prev.is_none() || (after - before).abs() > 1e-12 {
            e.feed(p[0], p[1], after);
        }
        prev = Some(s);
    }
    // A ramp on a closed loop leaves its first stretch shallow, so cut it again at depth, tabs left standing.
    if closed && ramp_len > 0.0 {
        let flat = |s: f64, after_tab_edge: bool| -> f64 {
            let eps = if after_tab_edge { 1e-9 } else { -1e-9 };
            match tab_z {
                Some(top) if tabs.iter().any(|(a, b)| s + eps > *a && s + eps < *b) => z.max(top),
                _ => z,
            }
        };
        for s in breaks.into_iter().filter(|s| *s > 0.0 && *s <= ramp_len + 1e-12) {
            let p = position(s);
            let (before, after) = (flat(s, false), flat(s, true));
            e.feed(p[0], p[1], before);
            if (after - before).abs() > 1e-12 {
                e.feed(p[0], p[1], after);
            }
        }
    }
}

fn oriented(mut pts: Vec<P>, ccw: bool) -> Vec<P> {
    if (signed_area(&pts) > 0.0) != ccw {
        pts.reverse();
    }
    pts
}

/// Where a shape's cut ends: back at its start if it is closed.
fn end_of(item: &(Vec<P>, bool)) -> P {
    if item.1 {
        item.0[0]
    } else {
        item.0[item.0.len() - 1]
    }
}

/// A short way round: always on to the nearest, then undoing wherever the way crosses itself.
fn shortest_way(items: Vec<(Vec<P>, bool)>, from: P) -> Vec<(Vec<P>, bool)> {
    let ends: Vec<(P, P)> = items.iter().map(|item| (item.0[0], end_of(item))).collect();
    let order = shortest_order(&ends, from);
    let mut slots: Vec<Option<(Vec<P>, bool)>> = items.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// The order for `shortest_way`, given where each shape starts and ends.
fn shortest_order(ends: &[(P, P)], from: P) -> Vec<usize> {
    let gap = |a: P, b: P| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut left: Vec<usize> = (0..ends.len()).collect();
    let mut out: Vec<usize> = Vec::with_capacity(ends.len());
    let mut at = from;
    while !left.is_empty() {
        let i = (0..left.len()).min_by(|a, b| gap(ends[left[*a]].0, at).total_cmp(&gap(ends[left[*b]].0, at))).unwrap_or(0);
        let item = left.swap_remove(i);
        at = ends[item].1;
        out.push(item);
    }
    // Turning a stretch round is only as simple as this if each shape ends where it starts.
    let same_ends = ends.iter().all(|e| e.0 == e.1);
    if !same_ends || out.len() < 3 || out.len() > MOST_SHAPES_TO_UNCROSS {
        return out;
    }
    let spot = |list: &[usize], k: usize| ends[list[k]].0;
    for _ in 0..12 {
        let mut better = false;
        for i in 0..out.len() - 1 {
            let before = if i == 0 { from } else { spot(&out, i - 1) };
            for j in i + 1..out.len() {
                let (first, last) = (spot(&out, i), spot(&out, j));
                let now = gap(before, first) + if j + 1 < out.len() { gap(last, spot(&out, j + 1)) } else { 0.0 };
                let then = gap(before, last) + if j + 1 < out.len() { gap(first, spot(&out, j + 1)) } else { 0.0 };
                if then < now - 1e-9 {
                    out[i..=j].reverse();
                    better = true;
                }
            }
        }
        if !better {
            break;
        }
    }
    out
}

/// Past this many, working out a better way would take longer than it is worth.
const MOST_SHAPES_TO_UNCROSS: usize = 3000;

/// How many of the nearest spots each one tries joining up with.
const TOUR_NEIGHBOURS: usize = 8;
/// The most any stretch moves when the way round is shaken up.
const TOUR_SHAKE: usize = 50;

/// Turns the stretch between two places in a loop round, or the rest of the loop if that's shorter.
fn turn_round(order: &mut [usize], place: &mut [usize], from: usize, to: usize) {
    let m = order.len();
    let inside = (to + m - from) % m + 1;
    let (mut a, mut b, swaps) = if inside * 2 <= m { (from, to, inside / 2) } else { ((to + 1) % m, (from + m - 1) % m, (m - inside) / 2) };
    for _ in 0..swaps {
        order.swap(a, b);
        place[order[a]] = a;
        place[order[b]] = b;
        a = (a + 1) % m;
        b = (b + m - 1) % m;
    }
}

/// Undoes crossings in a loop for as long as that shortens it, and says by how much it did.
fn uncross(order: &mut [usize], place: &mut [usize], near: &[Vec<usize>], gap: &dyn Fn(usize, usize) -> f64, mut waiting: Vec<usize>) -> f64 {
    let m = order.len();
    let mut saved = 0.0;
    while let Some(a) = waiting.pop() {
        'tries: for ahead in [true, false] {
            let step = |at: usize| if ahead { (at + 1) % m } else { (at + m - 1) % m };
            let b = order[step(place[a])];
            let ab = gap(a, b);
            for c in near[a].iter().copied() {
                let ac = gap(a, c);
                if ac >= ab {
                    break;
                }
                let d = order[step(place[c])];
                if c == b || d == a {
                    continue;
                }
                let better = ab + gap(c, d) - ac - gap(b, d);
                if better > 1e-9 {
                    if ahead {
                        turn_round(order, place, place[b], place[c]);
                    } else {
                        turn_round(order, place, place[a], place[d]);
                    }
                    saved += better;
                    waiting.extend([a, b, c, d]);
                    break 'tries;
                }
            }
        }
    }
    saved
}

/// The Shortest Path way: a closed loop through the tool's position and every spot, shortened as far
/// as it will go, then started from the tool.
fn shortest_tour(spots: &[P], from: P) -> Vec<usize> {
    let n = spots.len();
    if n < 3 {
        return (0..n).collect();
    }
    let at = |k: usize| if k == 0 { from } else { spots[k - 1] };
    let gap = |a: usize, b: usize| (at(a)[0] - at(b)[0]).hypot(at(a)[1] - at(b)[1]);
    let m = n + 1;
    let mut order: Vec<usize> = Vec::with_capacity(m);
    order.push(0);
    let mut left: Vec<usize> = (1..m).collect();
    while !left.is_empty() {
        let last = order[order.len() - 1];
        let k = (0..left.len()).min_by(|a, b| gap(last, left[*a]).total_cmp(&gap(last, left[*b]))).unwrap_or(0);
        order.push(left.swap_remove(k));
    }
    if n <= MOST_SHAPES_TO_UNCROSS {
        let mut place = vec![0; m];
        for (k, o) in order.iter().enumerate() {
            place[*o] = k;
        }
        let near: Vec<Vec<usize>> = (0..m)
            .map(|a| {
                let mut others: Vec<usize> = (0..m).filter(|o| *o != a).collect();
                let keep = TOUR_NEIGHBOURS.min(others.len());
                if keep < others.len() {
                    others.select_nth_unstable_by(keep, |x, y| gap(a, *x).total_cmp(&gap(a, *y)));
                    others.truncate(keep);
                }
                others.sort_by(|x, y| gap(a, *x).total_cmp(&gap(a, *y)));
                others
            })
            .collect();
        uncross(&mut order, &mut place, &near, &gap, (0..m).collect());
        // Shake up a stretch by swapping two runs of spots, and keep it if that ends up shorter.
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut roll = |below: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % below
        };
        if m >= 8 {
            let (mut kept_order, mut kept_place) = (order.clone(), place.clone());
            for _ in 0..m / 10 {
                let most = TOUR_SHAKE.min((m - 2) / 2);
                let (p, one, two) = (roll(m), 1 + roll(most), 1 + roll(most));
                let cell = |k: usize| (p + k) % m;
                let (before, a0, a1, b0, b1, after) = (order[cell(0)], order[cell(1)], order[cell(one)], order[cell(one + 1)], order[cell(one + two)], order[cell(one + two + 1)]);
                let added = gap(before, b0) + gap(b1, a0) + gap(a1, after) - gap(before, a0) - gap(a1, b0) - gap(b1, after);
                let first: Vec<usize> = (1..=one).map(|k| order[cell(k)]).collect();
                let second: Vec<usize> = (one + 1..=one + two).map(|k| order[cell(k)]).collect();
                for (k, o) in second.iter().chain(&first).enumerate() {
                    order[cell(k + 1)] = *o;
                    place[*o] = cell(k + 1);
                }
                let saved = uncross(&mut order, &mut place, &near, &gap, vec![before, a0, a1, b0, b1, after]);
                if added - saved < -1e-9 {
                    kept_order.copy_from_slice(&order);
                    kept_place.copy_from_slice(&place);
                } else {
                    order.copy_from_slice(&kept_order);
                    place.copy_from_slice(&kept_place);
                }
            }
        }
    }
    let start = order.iter().position(|o| *o == 0).unwrap_or(0);
    (1..m).map(|k| order[(start + k) % m] - 1).collect()
}

/// What the way of ordering needs to know about a shape.
#[derive(Clone, Copy)]
struct Placed {
    start: P,
    end: P,
    lo: P,
    hi: P,
}

fn placed(item: &(Vec<P>, bool)) -> Placed {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &item.0 {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    Placed { start: item.0[0], end: end_of(item), lo, hi }
}

/// Grid cells are this many times the size of a typical shape.
const CELL_SIZE: f64 = 1.5;
/// Shapes left out at each end when working out that typical size, so odd ones don't count.
const SIZE_TRIM: f64 = 0.1;
const MOST_CELLS: usize = 200;
/// With shapes of no size at all, cells can't be sized from them.
const SMALLEST_SIZE: f64 = 1e-8;
const CELLS_WITHOUT_SIZE: usize = 20;
/// Fewer shapes than this all go in one cell.
const FEWEST_FOR_CELLS: usize = 4;
/// How far the cells reach past the shapes all round.
const CELL_MARGIN: f64 = 0.01;

fn typical_size(mut sizes: Vec<f64>) -> f64 {
    sizes.sort_by(f64::total_cmp);
    let n = sizes.len();
    let (from, to) = (((n as f64 * SIZE_TRIM) as usize), n - (n as f64 * SIZE_TRIM) as usize);
    let kept = &sizes[from..to.max(from + 1).min(n)];
    kept.iter().sum::<f64>() / kept.len().max(1) as f64
}

/// How many columns and rows of cells to sort shapes into.
fn cell_counts(items: &[Placed], level: &[usize], width: f64, height: f64) -> (usize, usize) {
    if level.len() < FEWEST_FOR_CELLS {
        return (1, 1);
    }
    let w = typical_size(level.iter().map(|i| items[*i].hi[0] - items[*i].lo[0]).collect());
    let h = typical_size(level.iter().map(|i| items[*i].hi[1] - items[*i].lo[1]).collect());
    if w < SMALLEST_SIZE || h < SMALLEST_SIZE {
        return (CELLS_WITHOUT_SIZE, CELLS_WITHOUT_SIZE);
    }
    let count = |whole: f64, one: f64| ((whole / (CELL_SIZE * one) + 0.5) as usize).clamp(1, MOST_CELLS);
    (count(width, w), count(height, h))
}

/// The three ways that sweep over cells of shapes.
#[derive(Clone, Copy, PartialEq)]
enum Sweep {
    /// Column by column.
    LeftToRight,
    /// Row by row.
    BottomToTop,
    /// Always on to the nearest cell with something in it.
    Grid,
}

fn swept(items: &[Placed], level: &[usize], sweep: Sweep, from: P) -> Vec<usize> {
    let (mut lo, mut hi) = (from, from);
    for i in level {
        for k in 0..2 {
            lo[k] = lo[k].min(items[*i].lo[k]);
            hi[k] = hi[k].max(items[*i].hi[k]);
        }
    }
    let (width, height) = (hi[0] - lo[0], hi[1] - lo[1]);
    let (nx, ny) = cell_counts(items, level, width, height);
    // The cells in the order they're gone through. Each column or row goes the opposite way to the last.
    let mut route: Vec<usize> = Vec::with_capacity(nx * ny);
    if sweep == Sweep::BottomToTop {
        for row in 0..ny {
            for c in 0..nx {
                route.push(row * nx + if row % 2 == 0 { c } else { nx - 1 - c });
            }
        }
    } else {
        for col in 0..nx {
            for r in 0..ny {
                route.push(if col % 2 == 0 { r } else { ny - 1 - r } * nx + col);
            }
        }
    }
    let mut rank = vec![0; nx * ny];
    for (k, c) in route.iter().enumerate() {
        rank[*c] = k;
    }
    let (x0, y0) = (lo[0] - CELL_MARGIN, lo[1] - CELL_MARGIN);
    let (cw, ch) = ((width + 2.0 * CELL_MARGIN) / nx as f64, (height + 2.0 * CELL_MARGIN) / ny as f64);
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); nx * ny];
    for i in level {
        let s = items[*i].start;
        let col = (((s[0] - x0) / cw) as usize).min(nx - 1);
        let row = (((s[1] - y0) / ch) as usize).min(ny - 1);
        // On the line between cells it goes in the first of them along the route.
        let mut cell = route[0];
        let mut best = usize::MAX;
        for r in row.saturating_sub(1)..=(row + 1).min(ny - 1) {
            for c in col.saturating_sub(1)..=(col + 1).min(nx - 1) {
                let holds = x0 + c as f64 * cw <= s[0] && s[0] <= x0 + (c + 1) as f64 * cw && y0 + r as f64 * ch <= s[1] && s[1] <= y0 + (r + 1) as f64 * ch;
                if holds && rank[r * nx + c] < best {
                    best = rank[r * nx + c];
                    cell = r * nx + c;
                }
            }
        }
        cells[cell].push(*i);
    }
    let gap = |a: P, b: P| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut at = from;
    let mut out = Vec::with_capacity(level.len());
    // Within a cell, on to the nearest each time.
    let empty_cell = |cell: &mut Vec<usize>, at: &mut P, out: &mut Vec<usize>| {
        for done in 0..cell.len() {
            let k = (done..cell.len()).min_by(|a, b| gap(items[cell[*a]].start, *at).total_cmp(&gap(items[cell[*b]].start, *at))).unwrap_or(done);
            cell.swap(done, k);
            *at = items[cell[done]].end;
            out.push(cell[done]);
        }
        cell.clear();
    };
    if sweep == Sweep::Grid {
        // Always on to the cell with the nearest shape in it, the earlier one if two are as near.
        route.retain(|c| !cells[*c].is_empty());
        while !route.is_empty() {
            let nearest = |cell: &Vec<usize>| cell.iter().map(|i| gap(items[*i].start, at)).fold(f64::MAX, f64::min);
            let k = (0..route.len()).min_by(|a, b| nearest(&cells[route[*a]]).total_cmp(&nearest(&cells[route[*b]]))).unwrap_or(0);
            let next = route.remove(k);
            empty_cell(&mut cells[next], &mut at, &mut out);
        }
    } else {
        for c in route {
            empty_cell(&mut cells[c], &mut at, &mut out);
        }
    }
    out
}

/// Tries the ticked ways of ordering these shapes and keeps the one with the least travel, there and
/// back. With none ticked, left to right, bottom to top and grid are what's tried.
fn order_level(items: &[Placed], level: &[usize], choices: OrderChoices, from: P) -> Vec<usize> {
    let none = !(choices.selection || choices.left_to_right || choices.bottom_to_top || choices.grid || choices.shortest);
    let mut tries: Vec<Vec<usize>> = Vec::new();
    if choices.selection {
        tries.push(level.to_vec());
    }
    if choices.left_to_right || none {
        tries.push(swept(items, level, Sweep::LeftToRight, from));
    }
    if choices.bottom_to_top || none {
        tries.push(swept(items, level, Sweep::BottomToTop, from));
    }
    if choices.grid || none {
        tries.push(swept(items, level, Sweep::Grid, from));
    }
    if choices.shortest {
        let spots: Vec<P> = level.iter().map(|i| [(items[*i].start[0] + items[*i].end[0]) / 2.0, (items[*i].start[1] + items[*i].end[1]) / 2.0]).collect();
        tries.push(shortest_tour(&spots, from).into_iter().map(|k| level[k]).collect());
    }
    let cost = |list: &[usize]| {
        let (mut at, mut sum) = (from, 0.0);
        for i in list {
            sum += (items[*i].start[0] - at[0]).hypot(items[*i].start[1] - at[1]);
            at = items[*i].end;
        }
        // The way back to the start counts as well.
        sum + (from[0] - at[0]).hypot(from[1] - at[1])
    };
    let mut best = tries.remove(0);
    for t in tries {
        if cost(&t) < cost(&best) {
            best = t;
        }
    }
    best
}

/// The order to cut shapes in. Each is sequenced among its neighbours, with what lies inside it cut
/// just before it: a part cut free before its holes would shift under the tool.
fn sequence_order(all: &[(Vec<P>, bool)], choices: OrderChoices, from: [f64; 3]) -> Vec<usize> {
    let items: Vec<Placed> = all.iter().map(placed).collect();
    let area = |i: usize| (items[i].hi[0] - items[i].lo[0]) * (items[i].hi[1] - items[i].lo[1]);
    // The smallest closed shape around each is what holds it.
    let closed: Vec<usize> = (0..all.len()).filter(|j| all[*j].1).collect();
    let mut parent: Vec<Option<usize>> = vec![None; all.len()];
    for i in 0..all.len() {
        let s = items[i].start;
        for j in closed.iter().copied().filter(|j| *j != i) {
            let fits = items[j].lo[0] <= s[0] && s[0] <= items[j].hi[0] && items[j].lo[1] <= s[1] && s[1] <= items[j].hi[1];
            if fits && area(j) > area(i) && parent[i].is_none_or(|p| area(j) < area(p)) && inside_even_odd(&[Path::closed(all[j].0.clone())], s) {
                parent[i] = Some(j);
            }
        }
    }
    let mut held: Vec<Vec<usize>> = vec![Vec::new(); all.len()];
    for (k, p) in parent.iter().enumerate() {
        if let Some(p) = p {
            held[*p].push(k);
        }
    }
    // Each level is ordered from one fixed point, not from wherever the tool will have got to: from
    // the start of the shape around the level above, or from the tool's position at the top.
    fn walk_level(level: &[usize], items: &[Placed], held: &[Vec<usize>], choices: OrderChoices, from: P, owner: Option<usize>, out: &mut Vec<usize>) {
        let next_from = owner.map_or(from, |o| items[o].start);
        for i in order_level(items, level, choices, from) {
            if !held[i].is_empty() {
                walk_level(&held[i], items, held, choices, next_from, Some(i), out);
            }
            out.push(i);
        }
    }
    let top: Vec<usize> = (0..all.len()).filter(|k| parent[*k].is_none()).collect();
    let mut out = Vec::with_capacity(all.len());
    if !top.is_empty() {
        walk_level(&top, &items, &held, choices, [from[0], from[1]], None, &mut out);
    }
    out
}

fn best_order(items: Vec<(Vec<P>, bool)>, choices: OrderChoices, from: [f64; 3]) -> Vec<(Vec<P>, bool)> {
    if items.is_empty() {
        return items;
    }
    let order = sequence_order(&items, choices, from);
    let mut slots: Vec<Option<(Vec<P>, bool)>> = items.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

fn sort_paths(mut items: Vec<(Vec<P>, bool)>, order: Order, from: [f64; 3]) -> Vec<(Vec<P>, bool)> {
    let left = |pts: &[P]| pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    match order {
        Order::AsDrawn => items,
        Order::LeftToRight => {
            items.sort_by(|a, b| left(&a.0).total_cmp(&left(&b.0)));
            items
        }
        Order::Nearest => shortest_way(items, [from[0], from[1]]),
    }
}

fn profile_like(e: &mut Emit, op: &Op, side: Side) {
    let radius = op.tool.dia / 2.0 + op.allowance;
    let extra = if op.last_pass { op.last_allowance } else { 0.0 };
    let zs = depths(op);
    let margin = radius + extra + op.tool.dia;
    // Each set of shapes on its own grid; a set inside another shape counts its inside the other way round.
    let fields: Vec<(Field, bool)> = if side == Side::On {
        Vec::new()
    } else {
        regions(&op.paths, 2.0 * margin, false)
            .into_iter()
            .filter_map(|set| {
                let shapes: Vec<Path> = set.iter().map(|i| op.paths[*i].clone()).collect();
                let others = op.paths.iter().enumerate().filter(|(i, p)| p.closed && !set.contains(i)).map(|(_, p)| p);
                let flipped = inside_even_odd(others, shapes[0].pts[0]);
                Some((Field::build_to(&shapes, margin, 900, op.tool.dia / 64.0)?, flipped))
            })
            .collect()
    };
    // Square corners are worked out shape by shape, which only suits shapes not inside each other.
    let closed: Vec<&Path> = op.paths.iter().filter(|p| p.closed).collect();
    let separate = closed.iter().enumerate().all(|(i, p)| closed.iter().enumerate().all(|(j, q)| i == j || !inside_even_odd(std::slice::from_ref(*q), p.pts[0])));
    let sharp = op.sharp_corners && side == Side::Outside && separate;
    let keep_click_order = op.order == Order::AsDrawn || op.order_choices.is_some_and(|c| c.selection);
    let loops_at = |off: f64| -> Vec<(Vec<P>, bool)> {
        let mut out = Vec::new();
        // A square corner in a notch narrower than the tool, or by a close neighbour, would cut into the part.
        let square: Option<Vec<Vec<P>>> = sharp.then(|| closed.iter().map(|p| sharp_offset(p, off)).collect::<Vec<_>>()).filter(|loops| loops.iter().all(|l| stays_clear(l, &op.paths, off)));
        match side {
            Side::Outside if square.is_some() => {
                out.extend(square.unwrap_or_default().into_iter().map(|l| (oriented(l, !op.climb), true)));
            }
            Side::Outside => {
                for (f, flipped) in &fields {
                    out.extend(f.contours(if *flipped { off } else { -off }).into_iter().map(|l| (oriented(l, !op.climb), true)));
                }
            }
            Side::Inside => {
                for (f, flipped) in &fields {
                    out.extend(f.contours(if *flipped { -off } else { off }).into_iter().map(|l| (oriented(l, op.climb), true)));
                }
            }
            _ => {}
        }
        // Worked-out outlines come back in no special order, so put them in the order of their shapes.
        if keep_click_order {
            let source = |l: &[P]| (0..op.paths.len()).min_by(|a, b| distance_to_path(l[0], &op.paths[*a]).total_cmp(&distance_to_path(l[0], &op.paths[*b]))).unwrap_or(0);
            out.sort_by_key(|item: &(Vec<P>, bool)| source(&item.0));
        }
        // Open lines can only be followed.
        for p in &op.paths {
            if !p.closed {
                out.push((p.pts.clone(), false));
            } else if side == Side::On {
                // Around a closed shape, on its line turns the same way as outside it.
                out.push((oriented(p.pts.clone(), !op.climb), true));
            }
        }
        out
    };
    let tab_top = -(op.cut_depth - op.tab_thickness);
    let start = if op.start_at == StartAt::Keep && op.start_mid_span { StartAt::Optimize } else { op.start_at };
    // With leads or ramps, the start isn't moved to suit the tool: only a chosen point moves it.
    let start = if start == StartAt::Optimize && (op.lead || op.ramp) { StartAt::Keep } else { start };
    let run = |e: &mut Emit, mut items: Vec<(Vec<P>, bool)>, pass_zs: &[f64]| {
        let from = e.last();
        // The order is settled with each shape starting where it was drawn to, and only then is
        // the start moved.
        // Twice the offset, as a worked-out loop on a curve can sit a little further off than that.
        start_from_shapes(&mut items, &op.paths, 2.0 * (radius.abs() + extra.abs()) + 1e-3);
        let mut ordered = match op.order_choices {
            Some(choices) => best_order(items, choices, from),
            None => sort_paths(items, op.order, from),
        };
        match start {
            StartAt::Keep => {}
            StartAt::Near(place) => {
                for (pts, closed) in &mut ordered {
                    if *closed {
                        if let Some(target) = spot_on_box(pts, place) {
                            start_loop_near_point(pts, target);
                        }
                    } else {
                        start_line_at(pts, place);
                    }
                }
            }
            StartAt::Optimize => {
                let mut at = [from[0], from[1]];
                for (pts, closed) in &mut ordered {
                    if *closed {
                        start_near(pts, at);
                    }
                    at = if *closed { pts[0] } else { pts[pts.len() - 1] };
                }
            }
        }
        for (pts, closed) in ordered {
            let mut route = pts.clone();
            if closed {
                route.push(pts[0]);
            }
            let unit_dir = |a: P, b: P| {
                let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
                [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
            };
            let total: f64 = route.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
            let side_sign = away_from_part(&pts, closed, side);
            let has_lead = op.lead && op.lead_distance > 0.0 && pts.len() >= 2;
            let lead_in = if has_lead { lead_points(pts[0], unit_dir(route[0], route[1]), side_sign, op, false) } else { Vec::new() };
            let mut previous = -op.start_depth;
            for (i, z) in pass_zs.iter().enumerate() {
                let tab_z = (op.tabs && closed && *z < tab_top).then_some(tab_top);
                // Moves added at full depth near the start must stop short of the first tab.
                let before_tab = match tab_z {
                    Some(_) if op.tab_count > 0 => (total / (2.0 * op.tab_count as f64) - op.tab_length / 2.0).max(0.0),
                    _ => f64::MAX,
                };
                let depth = (previous - *z).abs();
                let smallest = if op.tool.metric { SHORTEST_RAMP } else { SHORTEST_RAMP / 25.4 };
                let wants_ramp = op.ramp && total >= smallest && depth >= smallest * 0.01;
                let ramp_on_lead = wants_ramp && has_lead && op.ramp_on_lead;
                // A line that doesn't come back round, or a cut led into, can only go there and back.
                let zigzag = wants_ramp && !ramp_on_lead && (op.ramp_kind == RampKind::ZigZag || !closed || has_lead);
                let ramp_from = (wants_ramp && !ramp_on_lead && !zigzag).then_some(previous);
                let lead_z = if ramp_on_lead || (wants_ramp && has_lead) { previous } else { *z };
                let plunge_z = if zigzag { previous } else { ramp_from.unwrap_or(lead_z) };
                if has_lead {
                    e.go_to(lead_in[0][0], lead_in[0][1], plunge_z);
                    let count = lead_in.len();
                    for (k, q) in lead_in.iter().enumerate().skip(1) {
                        let h = if ramp_on_lead { previous + (*z - previous) * k as f64 / count as f64 } else { plunge_z };
                        e.feed(q[0], q[1], h);
                    }
                    e.feed(pts[0][0], pts[0][1], if ramp_on_lead { *z } else { plunge_z });
                } else if i == 0 || !closed {
                    // Stay down between passes on a closed loop.
                    e.go_to(pts[0][0], pts[0][1], plunge_z);
                } else {
                    e.feed(pts[0][0], pts[0][1], plunge_z);
                }
                if zigzag {
                    // Forward going part of the way down, and back for the next part.
                    let (leg, times) = zig_zag(op, depth);
                    let turn = along(&route, leg.min(total).min(before_tab));
                    let drop = (previous - *z) / (2 * times) as f64;
                    for k in 0..times {
                        e.feed(turn[0], turn[1], previous - drop * (2 * k + 1) as f64);
                        e.feed(pts[0][0], pts[0][1], previous - drop * (2 * k + 2) as f64);
                    }
                }
                walk(e, &pts, closed, *z, op, ramp_from, tab_z);
                previous = *z;
                if has_lead || !closed || i + 1 == pass_zs.len() {
                    // Running on past the start leaves no mark where the cut closes.
                    if closed && op.overcut > 0.0 {
                        let n = (route.len() * 2).max(2);
                        let reach = op.overcut.min(total).min(before_tab);
                        for k in 1..=n {
                            let q = along(&route, reach * k as f64 / n as f64);
                            e.feed(q[0], q[1], *z);
                        }
                    }
                    if has_lead && op.lead_out {
                        let (end, dir) = if closed && op.overcut > 0.0 {
                            let s = op.overcut.min(total).min(before_tab);
                            (along(&route, s), unit_dir(along(&route, s), along(&route, (s + total / 1000.0).min(total))))
                        } else {
                            (route[route.len() - 1], unit_dir(route[route.len() - 2], route[route.len() - 1]))
                        };
                        for q in lead_points(end, dir, side_sign, op, true) {
                            e.feed(q[0], q[1], *z);
                        }
                    }
                    if has_lead {
                        e.retract();
                    }
                }
            }
            e.retract();
        }
    };
    run(e, loops_at(radius + extra), &zs);
    if op.last_pass && side != Side::On {
        let mut last = loops_at(radius);
        if op.last_reverse {
            for (pts, _) in &mut last {
                pts.reverse();
            }
        }
        run(e, last, &zs[zs.len() - 1..]);
    }
}

/// One straight line of a raster: start, direction and length.
fn scan_rows(lo: P, hi: P, angle: f64, step: f64) -> Vec<(P, P, f64)> {
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let reach = (hi[0] - lo[0]).hypot(hi[1] - lo[1]) / 2.0;
    let (s, k) = angle.to_radians().sin_cos();
    let (dir, normal) = ([k, s], [-s, k]);
    let mut rows = Vec::new();
    let mut v = -reach;
    while v <= reach + 1e-9 {
        let start = [c[0] + normal[0] * v - dir[0] * reach, c[1] + normal[1] * v - dir[1] * reach];
        rows.push((start, dir, reach * 2.0));
        v += step.max(1e-5);
    }
    rows
}

/// A pocket's rows: a grid over everything the toolpath clears, reaching a little past its corners
/// whatever the angle, with the first row beyond them.
fn pocket_rows(lo: P, hi: P, angle: f64, step: f64) -> Vec<(P, P, f64)> {
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let reach = (hi[0] - c[0]).hypot(hi[1] - c[1]) * POCKET_ROW_REACH;
    let (s, k) = angle.to_radians().sin_cos();
    let (dir, normal) = ([k, s], [-s, k]);
    let step = step.max(1e-5);
    let start = [c[0] - reach * normal[0] - reach * dir[0], c[1] - reach * normal[1] - reach * dir[1]];
    (0..=((reach * 2.0 / step) as usize)).map(|i| ([start[0] + normal[0] * step * i as f64, start[1] + normal[1] * step * i as f64], dir, reach * 2.0)).collect()
}

/// How far past the corners of what's being cleared a pocket's rows reach.
const POCKET_ROW_REACH: f64 = 1.05;

/// The stretches of a raster row for which `inside` holds.
fn row_spans(row: (P, P, f64), sample: f64, inside: &dyn Fn(f64, f64) -> bool) -> Vec<(P, P)> {
    let (start, dir, len) = row;
    let at = |u: f64| [start[0] + dir[0] * u, start[1] + dir[1] * u];
    let mut spans = Vec::new();
    let mut from: Option<f64> = None;
    let mut u = 0.0;
    while u <= len {
        let p = at(u);
        match (inside(p[0], p[1]), from) {
            (true, None) => from = Some(u),
            (false, Some(a)) => {
                spans.push((at(a), at(u - sample)));
                from = None;
            }
            _ => {}
        }
        u += sample;
    }
    if let Some(a) = from {
        spans.push((at(a), at(len)));
    }
    spans
}

/// Cuts every row back and forth at height `z`.
fn cut_rows(e: &mut Emit, rows: Vec<(P, P, f64)>, sample: f64, z: f64, reach: f64, inside: &dyn Fn(f64, f64) -> bool) {
    let mut forward = true;
    let mut down = false;
    for row in rows {
        // Each row goes the other way to the one before, whether or not there was anything to cut.
        let this_way = forward;
        forward = !forward;
        let forward = this_way;
        let mut spans = row_spans(row, sample, inside);
        if spans.is_empty() {
            continue;
        }
        if !forward {
            spans.reverse();
        }
        for (a, b) in spans {
            let (from, to) = if forward { (a, b) } else { (b, a) };
            // On to the next row without lifting, as long as the way there stays where the tool may go.
            let last = e.last();
            let across = (from[0] - last[0]).hypot(from[1] - last[1]);
            let clear = down && across <= reach && {
                let steps = ((across / sample).ceil() as usize).max(1);
                (0..=steps).all(|k| {
                    let t = k as f64 / steps as f64;
                    inside(last[0] + (from[0] - last[0]) * t, last[1] + (from[1] - last[1]) * t)
                })
            };
            if clear {
                e.feed(from[0], from[1], z);
            } else {
                e.go_to(from[0], from[1], z);
            }
            e.feed(to[0], to[1], z);
            down = true;
        }
    }
    e.retract();
}

fn step_of(op: &Op) -> f64 {
    if op.stepover > 0.0 {
        op.stepover
    } else {
        (op.tool.dia * op.tool.stepover).max(1e-4)
    }
}

/// Clears each separate pocket in turn, every one on a grid fitting just that pocket.
fn pocket(e: &mut Emit, op: &Op) {
    // A pocket's rings grow from its walls and around its islands until they meet, so shapes one
    // inside the other stay together, however far apart.
    let mut sets = regions(&op.paths, 2.0 * op.tool.dia, true);
    if let Some(choices) = op.order_choices {
        let shapes: Vec<(Vec<P>, bool)> = sets.iter().map(|set| (op.paths[set[0]].pts.clone(), true)).collect();
        let mut ordered: Vec<Vec<usize>> = Vec::with_capacity(sets.len());
        for i in sequence_order(&shapes, wanted_order(op, choices), e.last()) {
            ordered.push(std::mem::take(&mut sets[i]));
        }
        sets = ordered;
    } else {
        let at = e.last();
        let far = |set: &Vec<usize>| {
            let shapes: Vec<&Path> = set.iter().map(|i| &op.paths[*i]).collect();
            bbox(shapes).map_or(f64::MAX, |(lo, hi)| ((lo[0] + hi[0]) / 2.0 - at[0]).hypot((lo[1] + hi[1]) / 2.0 - at[1]))
        };
        if op.order != Order::AsDrawn {
            sets.sort_by(|a, b| far(a).total_cmp(&far(b)));
        }
    }
    let whole = bbox(&op.paths);
    for set in sets {
        let mut one = op.clone();
        one.paths = set.iter().map(|i| op.paths[*i].clone()).collect();
        pocket_set(e, &one, whole);
    }
}

/// What's ticked, or only the order of selection when that's what the form asks for.
fn wanted_order(op: &Op, choices: OrderChoices) -> OrderChoices {
    if op.order == Order::AsDrawn && !choices.selection {
        return OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: false };
    }
    choices
}

fn pocket_set(e: &mut Emit, op: &Op, whole: Option<(P, P)>) {
    let radius = op.tool.dia / 2.0 + op.allowance;
    let step = step_of(op);
    let Some(field) = Field::build_to(&op.paths, op.tool.dia, 900, op.tool.dia / 64.0) else { return };
    let zs = depths(op);
    let (lo, hi) = bbox(&op.paths).unwrap();
    let ring = |e: &mut Emit, level: f64, z: f64, from: f64| {
        let ramp = (op.ramp && op.ramp_distance > 0.0).then_some(from);
        let mut rings = field.contours(level);
        while !rings.is_empty() {
            // On to the nearest ring, at its nearest point: the tool needn't lift to get there.
            let last = e.last();
            let far = |q: &P| (q[0] - last[0]).hypot(q[1] - last[1]);
            let nearest = |pts: &Vec<P>| pts.iter().map(far).fold(f64::MAX, f64::min);
            let i = (0..rings.len()).min_by(|a, b| nearest(&rings[*a]).total_cmp(&nearest(&rings[*b]))).unwrap_or(0);
            let mut pts = oriented(rings.swap_remove(i), op.climb);
            let start = (0..pts.len()).min_by(|a, b| far(&pts[*a]).total_cmp(&far(&pts[*b]))).unwrap_or(0);
            pts.rotate_left(start);
            let across = far(&pts[0]);
            let stays_down = ramp.is_none() && (last[2] - z).abs() < 1e-9 && across <= op.tool.dia * 1.5 && {
                // Only through what the tool may cut: never closer to a wall than its radius.
                let steps = ((across / field.cell).ceil() as usize).max(1);
                (0..=steps).all(|k| {
                    let t = k as f64 / steps as f64;
                    field.sample(last[0] + (pts[0][0] - last[0]) * t, last[1] + (pts[0][1] - last[1]) * t) >= radius - field.cell * 0.1
                })
            };
            if stays_down {
                e.feed(pts[0][0], pts[0][1], z);
            } else {
                e.go_to(pts[0][0], pts[0][1], ramp.unwrap_or(z));
            }
            walk(e, &pts, true, z, op, ramp, None);
        }
    };
    let mut previous = -op.start_depth;
    let cut_from = e.moves.len();
    let mut spots: Option<Vec<P>> = None;
    for z in &zs {
        if op.rest_only {
            ring(e, radius, *z, previous);
        } else if op.raster {
            if op.profile_pass == ProfilePass::First {
                ring(e, radius, *z, previous);
            }
            // An outline pass cleans up after the rows, so they can stop short of it.
            let extra = if op.profile_pass == ProfilePass::None { 0.0 } else { op.raster_allowance.abs() };
            let clear = radius + extra;
            let (box_lo, box_hi) = whole.unwrap_or((lo, hi));
            let rows = pocket_rows([box_lo[0] + clear, box_lo[1] + clear], [box_hi[0] - clear, box_hi[1] - clear], op.raster_angle, step);
            cut_rows(e, rows, field.cell, *z, op.tool.dia * 1.5, &|x, y| field.sample(x, y) >= clear);
            if op.profile_pass == ProfilePass::Last {
                ring(e, radius, *z, *z);
            }
        } else {
            // Clear from the middle outward, so the last ring cleans the wall.
            let mut levels = Vec::new();
            let mut level = radius;
            while !field.contours(level).is_empty() {
                levels.push(level);
                level += step;
            }
            for l in levels.iter().rev() {
                ring(e, *l, *z, previous);
            }
        }
        e.retract();
        // What the passes above didn't reach, worked out once and cut at every depth. A pocket that
        // only clears the rest of a larger tool leaves the other tool's share on purpose.
        if !op.rest_only {
            let spots = spots.get_or_insert_with(|| leftovers(&field, op.tool.dia / 2.0, radius, op.allowance, &e.moves[cut_from..]));
            cut_spots(e, spots, *z, &field, radius);
        }
        previous = *z;
    }
}

/// Where the tool has to go to clear what the given moves left in a pocket: the middle of a bend or
/// a point, say, too far from any pass. A spot the tool can't reach without cutting into a wall is
/// left: a round tool can't get into a sharp corner.
fn leftovers(field: &Field, tool: f64, centre_at: f64, allowance: f64, moves: &[Move]) -> Vec<P> {
    let cell = field.cell.max(tool / 24.0);
    let (width, height) = ((field.nx - 1) as f64 * field.cell, (field.ny - 1) as f64 * field.cell);
    let (nx, ny) = ((width / cell) as usize + 1, (height / cell) as usize + 1);
    let at = |ix: usize, iy: usize| [field.x0 + ix as f64 * cell, field.y0 + iy as f64 * cell];
    // A touch of leeway, or the very edge of every pass would count as left behind.
    let reach = tool + cell * 0.15;
    let cells = (reach / cell).ceil() as i64 + 1;
    let disc: Vec<(i64, i64)> = (-cells..=cells).flat_map(|y| (-cells..=cells).map(move |x| (x, y))).filter(|(x, y)| ((x * x + y * y) as f64).sqrt() * cell <= reach + cell * 1.5).collect();
    let mut covered = vec![false; nx * ny];
    let stamp = |covered: &mut Vec<bool>, p: P| {
        // Measured from where the tool really is, not the nearest cell: slivers are smaller than cells.
        let (ox, oy) = ((p[0] - field.x0) / cell, (p[1] - field.y0) / cell);
        let (fx, fy) = (ox.round() as i64, oy.round() as i64);
        let limit = (reach / cell) * (reach / cell);
        for (dx, dy) in &disc {
            let (x, y) = (fx + dx, fy + dy);
            let (ax, ay) = (x as f64 - ox, y as f64 - oy);
            if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny && ax * ax + ay * ay <= limit {
                covered[y as usize * nx + x as usize] = true;
            }
        }
    };
    let spacing = (tool / 8.0).max(1e-6);
    for w in moves.windows(2) {
        if w[1].rapid {
            continue;
        }
        let (a, b) = ([w[0].p[0], w[0].p[1]], [w[1].p[0], w[1].p[1]]);
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / spacing).ceil() as usize).max(1);
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            stamp(&mut covered, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    // Deepest in the pocket first, so that each spot clears as much as it can.
    let wall = allowance + cell;
    let mut left: Vec<(usize, usize, f64)> = Vec::new();
    for iy in 0..ny {
        for ix in 0..nx {
            if !covered[iy * nx + ix] {
                let p = at(ix, iy);
                let inside = field.sample(p[0], p[1]);
                if inside > wall {
                    left.push((ix, iy, inside));
                }
            }
        }
    }
    left.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut spots = Vec::new();
    for (ix, iy, inside) in left {
        if covered[iy * nx + ix] {
            continue;
        }
        let p = at(ix, iy);
        // The nearest place the tool fits, if any is close enough to reach this.
        let spot = if inside >= centre_at {
            Some(p)
        } else {
            let tool_cells = (tool - cell * 0.5) / cell;
            disc.iter()
                .filter(|(dx, dy)| (((dx * dx + dy * dy) as f64).sqrt()) <= tool_cells)
                .map(|(dx, dy)| ([p[0] + *dx as f64 * cell, p[1] + *dy as f64 * cell], dx * dx + dy * dy))
                .filter(|(q, _)| field.sample(q[0], q[1]) >= centre_at)
                .min_by_key(|(_, far)| *far)
                .map(|(q, _)| q)
        };
        if let Some(q) = spot {
            stamp(&mut covered, q);
            spots.push(q);
            if spots.len() >= 5000 {
                break;
            }
        }
    }
    spots
}

/// Cuts at the spots, staying down between those with a clear way from one to the next.
fn cut_spots(e: &mut Emit, spots: &[P], z: f64, field: &Field, centre_at: f64) {
    if spots.is_empty() {
        return;
    }
    let mut left = spots.to_vec();
    let last = e.last();
    let mut at = [last[0], last[1]];
    let mut down = false;
    while !left.is_empty() {
        let far = |p: &P| (p[0] - at[0]).hypot(p[1] - at[1]);
        let i = (0..left.len()).min_by(|a, b| far(&left[*a]).total_cmp(&far(&left[*b]))).unwrap_or(0);
        let next = left.swap_remove(i);
        let steps = ((far(&next) / field.cell).ceil() as usize).max(1);
        let clear = down && (0..=steps).all(|k| {
            let t = k as f64 / steps as f64;
            field.sample(at[0] + (next[0] - at[0]) * t, at[1] + (next[1] - at[1]) * t) >= centre_at - field.cell * 0.1
        });
        if clear {
            e.feed(next[0], next[1], z);
        } else {
            e.go_to(next[0], next[1], z);
        }
        at = next;
        down = true;
    }
    e.retract();
}

fn drill(e: &mut Emit, op: &Op) {
    let mut centres: Vec<(Vec<P>, bool)> = op
        .paths
        .iter()
        .filter_map(|path| bbox([path]))
        .map(|(lo, hi)| (vec![[(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]], false))
        .collect();
    let from = e.last();
    centres = match op.order_choices {
        Some(choices) => best_order(centres, wanted_order(op, choices), from),
        None => sort_paths(centres, op.order, from),
    };
    for (c, _) in centres {
        let (x, y) = (c[0][0], c[0][1]);
        e.retract();
        let safe = e.m.safe_z;
        e.rapid([x, y, safe]);
        if op.peck && op.tool.pass_depth > 0.0 {
            let stops = peck_depths(op.start_depth, op.cut_depth, op.tool.pass_depth);
            for (i, z) in stops.iter().enumerate() {
                e.feed(x, y, *z);
                if i + 1 < stops.len() {
                    let lift = if op.retract_to_start { -op.start_depth + op.retract_gap } else { *z + op.retract_gap };
                    e.rapid([x, y, if op.retract_gap > 0.0 { lift } else { safe }]);
                }
            }
            e.rapid([x, y, safe]);
        } else {
            e.feed(x, y, -op.cut_depth);
            e.rapid([x, y, safe]);
        }
    }
}

/// Outline and optional fill at one depth, for marking and drag tools.
fn engrave(e: &mut Emit, op: &Op) {
    let z = -op.cut_depth;
    let step = step_of(op);
    let lines: Vec<Path> = op.paths.clone();
    if op.fill {
        if let Some(field) = Field::build(&op.paths, op.tool.dia.max(step), 900) {
            let (lo, hi) = bbox(&op.paths).unwrap();
            if op.hatch {
                let mut angles = vec![op.raster_angle];
                if op.cross_hatch {
                    angles.push(op.raster_angle + 90.0);
                }
                for a in angles {
                    cut_rows(e, scan_rows(lo, hi, a, step), field.cell, z, 0.0, &|x, y| field.sample(x, y) >= 0.0);
                }
            } else {
                let mut level = step;
                loop {
                    let rings = field.contours(level);
                    if rings.is_empty() {
                        break;
                    }
                    for pts in rings {
                        e.go_to(pts[0][0], pts[0][1], z);
                        walk(e, &pts, true, z, op, None, None);
                        e.retract();
                    }
                    level += step;
                }
            }
        }
    }
    for p in lines.iter().filter(|p| p.pts.len() >= 2) {
        e.go_to(p.pts[0][0], p.pts[0][1], z);
        walk(e, &p.pts, p.closed, z, op, None, None);
        e.retract();
    }
}

/// Follows each line with the depth easing in and out along its length.
fn flute(e: &mut Emit, op: &Op) {
    let start_z = -op.start_depth;
    for p in op.paths.iter().filter(|p| p.pts.len() >= 2) {
        let mut pts = p.pts.clone();
        if p.closed {
            pts.push(pts[0]);
        }
        let total: f64 = pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        if total <= 0.0 {
            continue;
        }
        let share = (op.flute_ramp / 100.0).clamp(0.01, 1.0);
        let ramp = match op.flute_ends {
            // Two ramps meet in the middle at most.
            FluteEnds::Both => total * share / 2.0,
            _ => total * share,
        };
        let eased = |s: f64| -> f64 {
            let from_start = s / ramp;
            let from_end = (total - s) / ramp;
            match op.flute_ends {
                FluteEnds::Both => from_start.min(from_end).clamp(0.0, 1.0),
                FluteEnds::Start => from_start.clamp(0.0, 1.0),
                FluteEnds::End => from_end.clamp(0.0, 1.0),
            }
        };
        // Short steps, so the slope of the ramp is followed closely.
        let step = (ramp / 24.0).max(total / 2000.0);
        for z in depths(op) {
            let depth_at = |s: f64| start_z + (z - start_z) * eased(s);
            e.go_to(pts[0][0], pts[0][1], depth_at(0.0));
            let mut run = 0.0;
            for w in pts.windows(2) {
                let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                let n = ((len / step).ceil() as usize).max(1);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    e.feed(w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t, depth_at(run + len * t));
                }
                run += len;
            }
            e.retract();
        }
    }
}

/// How deep a V-bit goes to cut a chamfer of the chosen width.
pub fn chamfer_depth(op: &Op) -> f64 {
    op.chamfer_width.max(0.0) / (op.tool.angle.to_radians() / 2.0).tan().max(1e-3)
}

/// Runs the tip of a V-bit along each line, deep enough for the chamfer's width.
fn chamfer(e: &mut Emit, op: &Op) {
    let mut deep = op.clone();
    deep.cut_depth = op.start_depth + chamfer_depth(op);
    for z in depths(&deep) {
        for p in op.paths.iter().filter(|p| p.pts.len() >= 2) {
            e.go_to(p.pts[0][0], p.pts[0][1], z);
            walk(e, &p.pts, p.closed, z, op, None, None);
            e.retract();
        }
    }
}

/// The widest distance from the edge of a shape to its middle.
pub fn widest_inside(paths: &[Path]) -> f64 {
    Field::build(paths, 0.0, 500).map_or(0.0, |f| f.d.iter().copied().fold(0.0f32, f32::max) as f64)
}

/// Short tool marks in rows, of varying depth and length, for a hand-carved surface.
fn texture(e: &mut Emit, op: &Op, job: &JobSize) {
    let field = Field::build(&op.paths, op.tool.dia, 700);
    let (lo, hi) = bbox(&op.paths).unwrap_or(([0.0, 0.0], [job.width, job.height]));
    let inset = op.boundary_offset;
    let inside = |x: f64, y: f64| match &field {
        Some(f) => f.sample(x, y) >= inset,
        None => x >= inset && y >= inset && x <= job.width - inset && y <= job.height - inset,
    };
    let mut seed = 0x2545_F491u64;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 10_000) as f64 / 10_000.0
    };
    let step = step_of(op).max(1e-3);
    let (cut_lo, cut_hi) = (op.tex_cut[0].min(op.tex_cut[1]), op.tex_cut[0].max(op.tex_cut[1]));
    let (len_lo, len_hi) = (op.tex_length[0].min(op.tex_length[1]).max(step), op.tex_length[0].max(op.tex_length[1]).max(step));
    let sample = (len_lo / 8.0).max(1e-3);
    for (start, dir, len) in scan_rows(lo, hi, op.raster_angle, step) {
        let mut u = random() * len_lo;
        while u < len {
            let stroke = len_lo + (len_hi - len_lo) * random();
            let deep = cut_lo + (cut_hi - cut_lo) * random();
            let n = (stroke / sample).ceil().max(2.0) as usize;
            let point = |t: f64| {
                let v = u + stroke * t;
                [start[0] + dir[0] * v, start[1] + dir[1] * v]
            };
            if (0..=n).all(|i| {
                let p = point(i as f64 / n as f64);
                inside(p[0], p[1])
            }) {
                let first = point(0.0);
                e.go_to(first[0], first[1], -op.start_depth);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let p = point(t);
                    e.feed(p[0], p[1], -op.start_depth - deep * (std::f64::consts::PI * t).sin());
                }
                e.retract();
            }
            u += stroke * (1.0 - op.tex_variation.clamp(0.0, 0.9) * random());
        }
    }
}

/// Sweeps the last selected shape, as a cross-section, along the others.
fn moulding(e: &mut Emit, op: &Op) {
    let Some((profile, rails)) = op.paths.split_last() else { return };
    let (lo, hi) = bbox([profile]).unwrap();
    let width = (hi[0] - lo[0]).max(1e-6);
    let step = step_of(op);
    let across = (width / step).ceil().max(1.0) as usize;
    let height_at = |u: f64| -> f64 {
        // The top of the cross-section at this position across it.
        let x = lo[0] + u;
        let mut top: Option<f64> = None;
        for (a, b) in profile.segments() {
            if (a[0] <= x) != (b[0] <= x) {
                let y = a[1] + (x - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
                top = Some(top.map_or(y, |t: f64| t.max(y)));
            }
        }
        top.unwrap_or(hi[1])
    };
    let surface = |u: f64| -(op.gap + hi[1] - height_at(u.clamp(1e-6, width - 1e-6)));
    for rail in rails.iter().filter(|r| r.pts.len() >= 2) {
        let mut route = rail.pts.clone();
        if rail.closed {
            route.push(route[0]);
        }
        let normal_at = |i: usize| {
            let (a, b) = (route[i.min(route.len() - 2)], route[(i + 1).min(route.len() - 1)]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
            [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]
        };
        for row in 0..=across {
            let u = width * row as f64 / across as f64;
            let shift = u - width / 2.0;
            let z = surface(u);
            let forward = row % 2 == 0;
            let order: Vec<usize> = if forward { (0..route.len()).collect() } else { (0..route.len()).rev().collect() };
            for (k, i) in order.into_iter().enumerate() {
                let n = normal_at(i);
                let (x, y) = (route[i][0] + n[0] * shift, route[i][1] + n[1] * shift);
                if k == 0 {
                    e.go_to(x, y, z);
                } else {
                    e.feed(x, y, z);
                }
            }
            e.retract();
        }
    }
}

fn vcarve(e: &mut Emit, op: &Op) {
    let half = (op.tool.angle.to_radians() / 2.0).tan().max(1e-3);
    let Some(field) = Field::build(&op.paths, op.tool.dia, 900) else { return };
    let reach = op.tool.dia / 2.0;
    // Fine enough to trace the middle of thin strokes, which set how deep the V goes.
    let step = (op.tool.dia * op.tool.stepover).min(op.tool.dia * 0.02).max(field.cell * 0.5);
    let limit = if op.kind == OpKind::Prism {
        op.cut_depth
    } else if op.flat_depth > 0.0 {
        op.flat_depth
    } else {
        f64::MAX
    };
    let mut level = field.cell * 0.25;
    loop {
        let found = field.contours(level);
        if found.is_empty() {
            break;
        }
        let z = -(op.start_depth + (level.min(reach) / half).min(limit));
        for pts in found {
            let pts = oriented(pts, op.climb);
            e.go_to(pts[0][0], pts[0][1], z);
            walk(e, &pts, true, z, op, None, None);
            e.retract();
        }
        level += step;
    }
}

/// Straight cutting lines across the model, back and forth.
/// Machines the 3D model: raster rows or rings, in levels when roughing.
fn machine_model(e: &mut Emit, op: &Op, surface: &Grid, top: f64, job: &JobSize) {
    let tip = tool_surface(surface, op.tool.kind, op.tool.dia, op.tool.angle);
    let step = step_of(op).max(surface.cell);
    let allow = op.allowance;
    let along = surface.cell;
    let at = |x: f64, y: f64| tip.sample(x, y) as f64 - top + allow;
    let rect = [Path::closed(crate::geom::rect([job.width / 2.0, job.height / 2.0], job.width, job.height, 0.0))];
    let limits: &[Path] = if op.paths.is_empty() || op.boundary == Boundary::Material { &rect } else { &op.paths };
    let margin = op.boundary_offset.max(0.0) + op.tool.dia;
    let Some(field) = Field::build(limits, margin, 800) else { return };
    let offset = op.boundary_offset;
    let inside = |x: f64, y: f64| field.sample(x, y) >= -offset && (0.0..=job.width).contains(&x) && (0.0..=job.height).contains(&y);
    let (lo, hi) = bbox(limits).unwrap();
    let lo = [lo[0] - offset, lo[1] - offset];
    let hi = [hi[0] + offset, hi[1] + offset];
    let mut levels: Vec<Option<f64>> = Vec::new();
    if op.kind == OpKind::Rough3D && op.z_level {
        let lowest = tip.data.iter().copied().fold(f32::MAX, f32::min) as f64 - top + allow;
        let n = pass_count(0.0, -lowest, op.tool.pass_depth);
        levels.extend((1..=n).map(|i| Some(lowest * i as f64 / n as f64)));
    } else {
        levels.push(None);
    }
    let boundary_ring = |e: &mut Emit| {
        for pts in field.contours(-offset) {
            let (x0, y0) = (pts[0][0], pts[0][1]);
            e.go_to(x0, y0, at(x0, y0));
            for p in pts.iter().chain(std::iter::once(&pts[0])) {
                e.feed(p[0], p[1], at(p[0], p[1]));
            }
            e.retract();
        }
    };
    let angle = if op.raster_along_x { op.raster_angle } else { op.raster_angle + 90.0 };
    let offset_strategy = op.kind == OpKind::Finish3D && !op.raster;
    if op.kind == OpKind::Rough3D && op.profile_pass == ProfilePass::First {
        boundary_ring(e);
    }
    for (li, level) in levels.iter().enumerate() {
        let above = if li == 0 { top.max(0.0) } else { levels[li - 1].unwrap() };
        if offset_strategy {
            let mut ring_level = -offset;
            loop {
                let rings = field.contours(ring_level);
                if rings.is_empty() {
                    break;
                }
                for pts in rings {
                    let pts = oriented(pts, op.climb);
                    let (x0, y0) = (pts[0][0], pts[0][1]);
                    e.go_to(x0, y0, at(x0, y0));
                    let n = pts.len();
                    for i in 1..=n {
                        let (a, b) = (pts[i - 1], pts[i % n]);
                        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
                        let parts = ((d / along).ceil() as usize).max(1);
                        for k in 1..=parts {
                            let t = k as f64 / parts as f64;
                            let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
                            e.feed(x, y, at(x, y));
                        }
                    }
                    e.retract();
                }
                ring_level += step;
            }
            continue;
        }
        let mut forward = true;
        for row in scan_rows(lo, hi, angle, step) {
            let needed = |x: f64, y: f64| inside(x, y) && (level.is_none() || at(x, y) < above - 1e-6);
            let mut spans = row_spans(row, along, &needed);
            if spans.is_empty() {
                continue;
            }
            if !forward {
                spans.reverse();
            }
            for (a, b) in spans {
                let (from, to) = if forward { (a, b) } else { (b, a) };
                let z_of = |x: f64, y: f64| level.map_or(at(x, y), |l| at(x, y).max(l));
                e.go_to(from[0], from[1], z_of(from[0], from[1]));
                let d = (to[0] - from[0]).hypot(to[1] - from[1]);
                let parts = ((d / along).ceil() as usize).max(1);
                for k in 1..=parts {
                    let t = k as f64 / parts as f64;
                    let (x, y) = (from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t);
                    e.feed(x, y, z_of(x, y));
                }
                e.retract();
            }
            forward = !forward;
        }
    }
    if op.kind == OpKind::Rough3D && op.profile_pass == ProfilePass::Last {
        boundary_ring(e);
    }
}

pub struct JobSize {
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
}

/// Builds the moves of a cutting path; returns an explanation when it can't be made.
pub fn generate(op: &Op, machine: Machine, job: &JobSize, model: Option<&Grid>) -> Result<Vec<Move>, String> {
    if op.tool.dia <= 0.0 {
        return Err("The tool's diameter must be more than zero.".into());
    }
    if op.tool.feed <= 0.0 || op.tool.plunge <= 0.0 {
        return Err("The tool's feed and plunge rates must be more than zero.".into());
    }
    if op.tool.rpm < 0.0 {
        return Err("The tool's spindle speed can't be below zero.".into());
    }
    if op.kind == OpKind::Pocket && (step_of(op) <= 0.0 || step_of(op) > op.tool.dia) {
        return Err("The stepover must be more than zero and no more than the tool's diameter.".into());
    }
    if op.kind == OpKind::Profile && op.tabs && op.tab_length <= 0.0 {
        return Err("The tabs need a length of more than zero.".into());
    }
    if op.kind.uses_model() {
        if model.is_none() {
            return Err("Create a 3D shape on the Modeling tab first.".into());
        }
    } else if op.paths.is_empty() && op.kind != OpKind::Texture {
        return Err("Select the shapes to cut in the 2D view first.".into());
    }
    let needs_closed = matches!(op.kind, OpKind::Pocket | OpKind::VCarve | OpKind::Prism);
    if needs_closed && !op.paths.iter().any(|p| p.closed) {
        return Err("This cut needs closed shapes. The selection only has open lines.".into());
    }
    if op.kind == OpKind::Moulding && op.paths.len() < 2 {
        return Err("Select the drive rail first, then the profile shape last.".into());
    }
    if op.tool.dia <= 0.0 {
        return Err("The tool needs a diameter above zero.".into());
    }
    if op.kind == OpKind::Chamfer {
        if !matches!(op.tool.kind, ToolKind::VBit | ToolKind::Engraving) {
            return Err("A chamfer is cut with a V-bit. Select one for this toolpath.".into());
        }
        if op.chamfer_width <= 0.0 {
            return Err("The chamfer needs a width above zero.".into());
        }
        if op.chamfer_width > op.tool.dia / 2.0 {
            return Err(format!("This V-bit can cut a chamfer {} wide at most.", trim(op.tool.dia / 2.0)));
        }
    }
    if op.kind == OpKind::Flute && op.cut_depth <= op.start_depth {
        return Err("The flute depth must be deeper than the start depth.".into());
    }
    if op.kind == OpKind::Drill && op.cut_depth <= op.start_depth {
        return Err("Cut depth must be deeper than start depth.".into());
    }
    let top = model.map_or(0.0, |g| machine.model_top(job.thickness, g.max() as f64));
    let project = if op.project && !op.kind.uses_model() { model } else { None };
    let mut e = Emit { moves: Vec::new(), m: machine, project, top };
    let home = machine.home;
    e.rapid(home);
    match op.kind {
        OpKind::Profile => profile_like(&mut e, op, op.side),
        OpKind::Engrave => engrave(&mut e, op),
        OpKind::Pocket => pocket(&mut e, op),
        OpKind::Drill => drill(&mut e, op),
        OpKind::VCarve | OpKind::Prism => vcarve(&mut e, op),
        OpKind::Texture => texture(&mut e, op, job),
        OpKind::Moulding => moulding(&mut e, op),
        OpKind::Flute => flute(&mut e, op),
        OpKind::Chamfer => chamfer(&mut e, op),
        OpKind::Rough3D | OpKind::Finish3D => machine_model(&mut e, op, model.unwrap(), top, job),
    }
    e.retract();
    e.rapid(home);
    if !e.moves.iter().any(|m| !m.rapid) {
        return Err(format!("No path fits: the {} is too wide for these shapes.", op.tool.label().to_lowercase()));
    }
    Ok(e.moves)
}

/// The moves that cut and travel, without the trip out from the home position and back to it.
pub fn working_moves(moves: &[Move]) -> &[Move] {
    let Some(home) = moves.first().filter(|m| m.rapid) else { return moves };
    let mut end = moves.len();
    if end > 1 && moves[end - 1].rapid && moves[end - 1].p == home.p {
        end -= 1;
    }
    &moves[1..end]
}

/// The same, for a path whose exact arcs are known.
pub fn working_steps(steps: &[Step]) -> &[Step] {
    let home = match steps.first() {
        Some(Step::Straight(m)) if m.rapid => m.p,
        _ => return steps,
    };
    let mut end = steps.len();
    if end > 1 && matches!(steps[end - 1], Step::Straight(m) if m.rapid && m.p == home) {
        end -= 1;
    }
    &steps[1..end]
}

/// A toolpath as a machine file runs it: from the home position only if it's the first in the file,
/// and back to it only if it's the last. Between them the tool goes straight on to the next cut.
pub fn route_in_file(moves: &[Move], steps: Option<&[Step]>, home: [f64; 3], first: bool, last: bool) -> (Vec<Move>, Option<Vec<Step>>) {
    let stop = Move { p: home, rapid: true };
    let mut route = Vec::with_capacity(moves.len() + 2);
    if first {
        route.push(stop);
    }
    route.extend_from_slice(working_moves(moves));
    if last {
        route.push(stop);
    }
    let exact = steps.map(|steps| {
        let mut exact = Vec::with_capacity(steps.len() + 2);
        if first {
            exact.push(Step::Straight(stop));
        }
        exact.extend_from_slice(working_steps(steps));
        if last {
            exact.push(Step::Straight(stop));
        }
        exact
    });
    (route, exact)
}

/// The original's usual allowance: time spent cutting and lifting counts this many times over.
pub const TIME_ALLOWANCE: f64 = 2.0;
/// The speed of fast moves it assumes unless told otherwise, in millimetres a minute (100 a second).
pub const RAPID_RATE_MM: f64 = 6000.0;

/// Estimated machining time, in seconds: cuts, plunges and lifts with the allowance, then fast moves
/// and pauses as they are. Lifts go at the fast rate.
pub fn estimate_seconds(moves: &[Move], tool: &Tool, rapid_rate: f64, dwell: f64) -> f64 {
    let (mut working, mut fast, mut paused) = (0.0, 0.0, 0.0);
    for (i, w) in moves.windows(2).enumerate() {
        let (a, b) = (w[0].p, w[1].p);
        let flat = (b[0] - a[0]).hypot(b[1] - a[1]);
        let rise = b[2] - a[2];
        let length = flat.hypot(rise);
        let minutes = |rate: f64| length / rate.max(1e-4);
        if w[1].rapid {
            if flat < 1e-9 && rise > 0.0 {
                working += minutes(rapid_rate);
            } else {
                fast += minutes(rapid_rate);
            }
        } else {
            working += minutes(if flat < 1e-9 && rise < 0.0 { tool.plunge } else { tool.feed });
            if flat < 1e-9 && moves.get(i + 2).is_some_and(|n| n.rapid) {
                paused += dwell;
            }
        }
    }
    (working * 60.0 * TIME_ALLOWANCE).trunc() + fast * 60.0 + paused
}

pub struct PostSettings {
    pub inches: bool,
    /// Where the machine's origin sits, in job coordinates.
    pub origin: [f64; 2],
    /// Added to Z: the thickness when zero is on the machine bed.
    pub z_shift: f64,
    /// Multiplies every length, when the output unit differs from the job's.
    pub scale: f64,
    /// When set, curves are written as true arcs, following them within this distance.
    pub arcs: Option<f64>,
}

/// One cutting path ready for output.
pub struct PostPath<'a> {
    pub name: &'a str,
    pub tool: &'a Tool,
    pub moves: &'a [Move],
    /// Seconds to pause at the bottom of each drill pass.
    pub dwell: f64,
    /// The same path with its exact arcs, when another program's file gave them.
    pub steps: Option<&'a [Step]>,
}

/// The machine file formats on offer: name, file extension, and whether it is in millimetres.
pub const POSTS: [(&str, &str, bool); 7] = [
    ("Generic G-code (inch) (*.nc)", "nc", false),
    ("Generic G-code (mm) (*.nc)", "nc", true),
    ("GRBL (mm) (*.gcode)", "gcode", true),
    ("GRBL (inch) (*.gcode)", "gcode", false),
    ("Mach3 (inch) (*.tap)", "tap", false),
    ("Mach3 (mm) (*.tap)", "tap", true),
    (crate::planetcnc::NAME, "gcode", true),
];

/// Plain G-code for a list of cutting paths.
pub fn gcode(paths: &[PostPath], post: &PostSettings) -> String {
    let mut s = String::new();
    let k = post.scale;
    let _ = writeln!(s, "(Written by Burin)");
    let _ = writeln!(s, "{} G90 G17", if post.inches { "G20" } else { "G21" });
    let mut current: Option<&Tool> = None;
    for path in paths {
        let (tool, moves) = (path.tool, path.moves);
        let _ = writeln!(s, "(Toolpath: {})", path.name);
        if current != Some(tool) {
            let _ = writeln!(s, "(Tool: {})", tool.name);
            let _ = writeln!(s, "M5\nM0 (Fit tool: {})\nS{:.0} M3", tool.name, tool.rpm);
            current = Some(tool);
        }
        let mut was_rapid = None;
        let mut feed = 0.0;
        let mut prev = moves.first().map_or([0.0; 3], |m| m.p);
        let steps: Vec<Step> = match (post.arcs, path.steps) {
            (Some(_), Some(exact)) => exact.to_vec(),
            (Some(tol), None) => fit_arcs(moves, tol),
            _ => moves.iter().map(|m| Step::Straight(*m)).collect(),
        };
        for (i, step) in steps.iter().enumerate() {
            let m = match step {
                Step::Straight(m) => m,
                Step::Arc { end, centre, counter_clockwise } => {
                    let (x, y, z) = ((end[0] - post.origin[0]) * k, (end[1] - post.origin[1]) * k, (end[2] + post.z_shift) * k);
                    let (ci, cj) = ((centre[0] - prev[0]) * k, (centre[1] - prev[1]) * k);
                    let wanted = tool.feed * k;
                    let _ = write!(s, "{} X{x:.4} Y{y:.4} Z{z:.4} I{ci:.4} J{cj:.4}", if *counter_clockwise { "G3" } else { "G2" });
                    if (wanted - feed).abs() > 1e-9 {
                        let _ = write!(s, " F{wanted:.1}");
                        feed = wanted;
                    }
                    s.push('\n');
                    // The next straight move must name its own kind again.
                    was_rapid = None;
                    prev = *end;
                    continue;
                }
            };
            let (x, y, z) = ((m.p[0] - post.origin[0]) * k, (m.p[1] - post.origin[1]) * k, (m.p[2] + post.z_shift) * k);
            if m.rapid {
                if was_rapid != Some(true) {
                    s.push_str("G0 ");
                }
                let _ = writeln!(s, "X{x:.4} Y{y:.4} Z{z:.4}");
            } else {
                if was_rapid != Some(false) {
                    s.push_str("G1 ");
                }
                let flat = (m.p[0] - prev[0]).hypot(m.p[1] - prev[1]);
                let wanted = if flat < (m.p[2] - prev[2]).abs() { tool.plunge } else { tool.feed } * k;
                let _ = write!(s, "X{x:.4} Y{y:.4} Z{z:.4}");
                if (wanted - feed).abs() > 1e-9 {
                    let _ = write!(s, " F{wanted:.1}");
                    feed = wanted;
                }
                s.push('\n');
                if path.dwell > 0.0 && flat < 1e-9 && matches!(steps.get(i + 1), Some(Step::Straight(n)) if n.rapid) {
                    let _ = writeln!(s, "G4 P{:.2}", path.dwell);
                    was_rapid = None;
                    prev = m.p;
                    continue;
                }
            }
            was_rapid = Some(m.rapid);
            prev = m.p;
        }
    }
    s.push_str("M5\nM30\n");
    s
}

pub fn blank_op(kind: OpKind, name: String, tool: Tool, depth: f64, paths: Vec<Path>) -> Op {
    let k = tool.dia / 0.25;
    let unit = if tool.metric { 25.4 } else { 1.0 };
    Op {
        passes: Vec::new(),
        ramp_kind: RampKind::Smooth,
        ramp_on_lead: false,
        ramp_by_angle: false,
        ramp_angle: default_ramp_angle(),
        ramp_angle_limit: 0.0,
        lead_circular: false,
        lead_angle: default_lead_angle(),
        lead_radius: 0.5 * unit,
        lead_out: false,
        overcut: 0.0,
        order_choices: matches!(kind, OpKind::Profile | OpKind::Pocket | OpKind::Drill).then(OrderChoices::default),
        start_at: if kind == OpKind::Profile { StartAt::Optimize } else { StartAt::Keep },
        sharp_corners: false,
        selector: None,
        flute_ramp: default_flute_ramp(),
        flute_ends: FluteEnds::Both,
        chamfer_width: 0.125 * unit,
        kind,
        name,
        start_depth: 0.0,
        cut_depth: depth,
        side: Side::Outside,
        climb: true,
        allowance: 0.0,
        last_pass: false,
        last_allowance: 0.0,
        last_reverse: false,
        tabs: false,
        tab_length: 0.5 * unit,
        tab_thickness: 0.125 * unit,
        tab_count: 4,
        ramp: false,
        ramp_distance: 1.0 * k,
        lead: false,
        lead_distance: 0.25 * k,
        order: Order::Nearest,
        start_mid_span: false,
        project: false,
        raster: false,
        flat_depth: 0.0,
        peck: false,
        raster_along_x: true,
        paths,
        clearance_tool: None,
        rest_only: false,
        raster_angle: 0.0,
        profile_pass: ProfilePass::Last,
        raster_allowance: 0.0,
        last_thickness: 0.0,
        keep_exact: false,
        stepover: if matches!(kind, OpKind::Finish3D | OpKind::Texture | OpKind::Engrave) { (tool.dia * tool.stepover).max(0.002 * unit) } else { 0.0 },
        retract_gap: 0.0,
        retract_to_start: true,
        dwell: 0.0,
        fill: false,
        hatch: false,
        cross_hatch: false,
        boundary: Boundary::Model,
        boundary_offset: 0.0,
        z_level: true,
        tex_cut: [0.08 * unit, 0.16 * unit],
        tex_length: [0.2 * unit, 0.4 * unit],
        tex_variation: 0.3,
        gap: 0.0,
        source: Vec::new(),
        tool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{circle, rect};

    const M: Machine = Machine { safe_z: 0.2, home: [0.0, 0.0, 0.8], plunge_gap: 0.2, gap_above: 0.0, gap_below: None };
    const J: JobSize = JobSize { width: 10.0, height: 10.0, thickness: 0.5 };

    fn op(kind: OpKind, paths: Vec<Path>) -> Op {
        blank_op(kind, "t".into(), tool("End Mill (0.25 inch)"), 0.5, paths)
    }

    fn tool(label: &str) -> Tool {
        default_tools().into_iter().find(|t| t.label() == label).unwrap()
    }

    fn cuts(moves: &[Move]) -> Vec<[f64; 3]> {
        moves.iter().filter(|m| !m.rapid).map(|m| m.p).collect()
    }

    #[test]
    fn outside_profile_runs_at_tool_radius_and_reaches_full_depth() {
        let o = op(OpKind::Profile, vec![Path::closed(circle([5.0, 5.0], 1.2))]);
        let moves = generate(&o, M, &J, None).unwrap();
        let cut = cuts(&moves);
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((lowest + 0.5).abs() < 1e-9, "reaches the depth, got {lowest}");
        for p in cut.iter().filter(|p| p[2] < 0.0) {
            let r = ((p[0] - 5.0).powi(2) + (p[1] - 5.0).powi(2)).sqrt();
            assert!((r - 1.325).abs() < 0.015, "radius {r}");
        }
        assert_eq!(pass_count(0.0, 0.5, 0.125), 4);
        assert_eq!(pass_count(0.0, 1.0, 0.125), 8);
    }

    #[test]
    fn inside_profile_stays_inside_and_directions_follow_climb() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Inside;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        for p in cut.iter().filter(|p| p[2] < 0.0) {
            assert!(p[0] > 3.1 && p[0] < 6.9 && p[1] > 3.1 && p[1] < 6.9, "{p:?}");
        }
        // Climb inside runs counter-clockwise; outside runs clockwise.
        let area_of = |o: &Op| {
            let c: Vec<P> = cuts(&generate(o, M, &J, None).unwrap())
                .iter()
                .filter(|p| (p[2] + 0.125).abs() < 1e-9)
                .map(|p| [p[0], p[1]])
                .collect();
            signed_area(&c)
        };
        assert!(area_of(&o) > 0.0);
        o.side = Side::Outside;
        assert!(area_of(&o) < 0.0);
        o.climb = false;
        assert!(area_of(&o) > 0.0);
    }

    #[test]
    fn tabs_leave_material_at_the_right_height() {
        let mut o = op(OpKind::Profile, vec![Path::closed(circle([5.0, 5.0], 2.0))]);
        o.tabs = true;
        o.tab_thickness = 0.125;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let at_tab = cut.iter().filter(|p| (p[2] + 0.375).abs() < 1e-9).count();
        let at_full = cut.iter().filter(|p| (p[2] + 0.5).abs() < 1e-9).count();
        assert!(at_tab > 4, "tab tops cut at depth minus thickness ({at_tab})");
        assert!(at_full > 4, "the rest reaches full depth ({at_full})");
    }

    #[test]
    fn ramp_descends_gradually() {
        let mut o = op(OpKind::Profile, vec![Path::closed(circle([5.0, 5.0], 2.0))]);
        o.ramp = true;
        o.ramp_distance = 2.0;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let between = cut.iter().filter(|p| p[2] < -0.01 && p[2] > -0.12).count();
        assert!(between > 3, "the first pass goes down along the way ({between})");
    }

    #[test]
    fn pocket_covers_the_middle_and_keeps_off_the_wall() {
        let o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 3.0, 0.0))]);
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let d_centre = cut.iter().filter(|p| p[2] < 0.0).map(|p| (p[0] - 5.0).abs().max((p[1] - 5.0).abs())).fold(9.0, f64::min);
        assert!(d_centre < 0.2, "gets near the centre ({d_centre})");
        for p in cut.iter().filter(|p| p[2] < 0.0) {
            assert!((p[0] - 5.0).abs() < 1.38 && (p[1] - 5.0).abs() < 1.38, "{p:?}");
        }
    }

    fn flute_op(ends: FluteEnds) -> Op {
        let mut o = blank_op(OpKind::Flute, "t".into(), tool("Ball Nose (0.25 inch)"), 0.2, vec![Path::open(vec![[1.0, 5.0], [9.0, 5.0]])]);
        o.tool.pass_depth = 1.0;
        o.flute_ends = ends;
        o.flute_ramp = 50.0;
        o
    }

    #[test]
    fn a_flute_eases_in_and_out_and_reaches_depth_in_the_middle() {
        let cut = cuts(&generate(&flute_op(FluteEnds::Both), M, &J, None).unwrap());
        let depth_near = |x: f64| cut.iter().min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs())).unwrap()[2];
        assert!(depth_near(1.0).abs() < 1e-9, "starts at the surface: {}", depth_near(1.0));
        assert!(depth_near(9.0).abs() < 1e-9, "ends at the surface: {}", depth_near(9.0));
        assert!((depth_near(5.0) + 0.2).abs() < 1e-9, "full depth in the middle: {}", depth_near(5.0));
        // A ramp share of 50% is a quarter of the length at each end: x = 3 is full depth.
        assert!((depth_near(2.0) + 0.1).abs() < 0.01, "halfway down the ramp: {}", depth_near(2.0));
        assert!((depth_near(3.0) + 0.2).abs() < 0.01);
    }

    #[test]
    fn a_flute_can_ramp_at_one_end_only() {
        let start = cuts(&generate(&flute_op(FluteEnds::Start), M, &J, None).unwrap());
        assert!(start.first().unwrap()[2].abs() < 1e-9, "shallow at the start");
        assert!((start.last().unwrap()[2] + 0.2).abs() < 1e-9, "full depth at the end");
        let end = cuts(&generate(&flute_op(FluteEnds::End), M, &J, None).unwrap());
        assert!((end.first().unwrap()[2] + 0.2).abs() < 1e-9, "full depth at the start");
        assert!(end.last().unwrap()[2].abs() < 1e-9, "shallow at the end");
    }

    #[test]
    fn a_deep_flute_takes_several_passes() {
        let mut o = flute_op(FluteEnds::Both);
        o.tool.pass_depth = 0.1;
        let moves = generate(&o, M, &J, None).unwrap();
        let lowest_per_pass: Vec<f64> = moves
            .split(|m| m.rapid)
            .filter(|run| !run.is_empty())
            .map(|run| run.iter().map(|m| m.p[2]).fold(0.0, f64::min))
            .collect();
        assert_eq!(lowest_per_pass.len(), 2, "{lowest_per_pass:?}");
        assert!((lowest_per_pass[0] + 0.1).abs() < 1e-9 && (lowest_per_pass[1] + 0.2).abs() < 1e-9, "{lowest_per_pass:?}");
    }

    #[test]
    fn flute_depth_must_go_down() {
        let mut o = flute_op(FluteEnds::Both);
        o.cut_depth = 0.0;
        assert!(generate(&o, M, &J, None).unwrap_err().contains("deeper"));
    }

    fn chamfer_op() -> Op {
        let mut t = tool("End Mill (0.25 inch)");
        t.kind = ToolKind::VBit;
        t.angle = 90.0;
        t.dia = 1.0;
        t.pass_depth = 10.0;
        let mut o = blank_op(OpKind::Chamfer, "t".into(), t, 0.5, vec![Path::closed(rect([5.0, 5.0], 2.0, 2.0, 0.0))]);
        o.chamfer_width = 0.2;
        o
    }

    #[test]
    fn a_chamfer_goes_as_deep_as_the_v_bit_needs_for_its_width() {
        let o = chamfer_op();
        // A 90 degree bit is as deep as it is wide on each side.
        assert!((chamfer_depth(&o) - 0.2).abs() < 1e-9);
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let low = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((low + 0.2).abs() < 1e-9, "{low}");
        // The tip stays on the line.
        for p in cut.iter().filter(|p| p[2] < 0.0) {
            let on_side = ((p[0] - 5.0).abs() - 1.0).abs() < 1e-6 || ((p[1] - 5.0).abs() - 1.0).abs() < 1e-6;
            assert!(on_side, "{p:?}");
        }
        let mut sixty = chamfer_op();
        sixty.tool.angle = 60.0;
        assert!((chamfer_depth(&sixty) - 0.2 / (30.0_f64).to_radians().tan()).abs() < 1e-9);
    }

    #[test]
    fn a_chamfer_refuses_what_it_cannot_cut() {
        let mut flat = chamfer_op();
        flat.tool.kind = ToolKind::EndMill;
        assert!(generate(&flat, M, &J, None).unwrap_err().contains("V-bit"));
        let mut wide = chamfer_op();
        wide.chamfer_width = 0.6;
        assert!(generate(&wide, M, &J, None).unwrap_err().contains("at most"));
        let mut none = chamfer_op();
        none.chamfer_width = 0.0;
        assert!(generate(&none, M, &J, None).unwrap_err().contains("above zero"));
    }

    #[test]
    fn a_chamfer_deeper_than_one_pass_steps_down() {
        let mut o = chamfer_op();
        o.tool.pass_depth = 0.1;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(cut.iter().any(|p| (p[2] + 0.1).abs() < 1e-9));
        assert!(cut.iter().any(|p| (p[2] + 0.2).abs() < 1e-9));
    }

    #[test]
    fn drill_goes_to_centres_at_depth() {
        let o = op(OpKind::Drill, vec![Path::closed(circle([2.0, 3.0], 0.2)), Path::closed(circle([7.0, 8.0], 0.2))]);
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        assert_eq!(cut.len(), 2);
        assert!((cut[0][0] - 2.0).abs() < 1e-6 && (cut[0][2] + 0.5).abs() < 1e-9);
        assert!((cut[1][1] - 8.0).abs() < 1e-6);
    }

    #[test]
    fn vcarve_depth_follows_the_width_of_the_shape() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(rect([5.0, 5.0], 0.4, 4.0, 0.0))]);
        o.tool = tool("V-Bit (90 deg 1.25 inch)");
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        // A 90 degree bit reaches half the 0.4 wide shape: 0.2 deep.
        assert!(lowest < -0.15 && lowest > -0.21, "{lowest}");
    }

    #[test]
    fn vcarve_reaches_the_middle_of_a_thin_stroke() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(rect([5.0, 5.0], 0.07, 3.0, 0.0))]);
        o.tool = tool("V-Bit (60 deg 0.5 inch)");
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        // A 60 degree bit in a 0.07 wide stroke: 0.035 / tan(30 degrees) = 0.06 deep.
        assert!(lowest < -0.045 && lowest > -0.0615, "{lowest}");
    }

    #[test]
    fn three_d_finish_follows_the_model() {
        let mut model = Grid::new(10.0, 10.0, 10_000, 0.0);
        for iy in 0..model.ny {
            for ix in 0..model.nx {
                let (x, y) = (ix as f64 * model.cell - 5.0, iy as f64 * model.cell - 5.0);
                model.data[iy * model.nx + ix] = (0.4 - (x * x + y * y) * 0.05).max(0.0) as f32;
            }
        }
        let mut o = op(OpKind::Finish3D, vec![]);
        o.tool = tool("Ball Nose (0.125 inch)");
        let cut = cuts(&generate(&o, M, &J, Some(&model)).unwrap());
        assert!(cut.iter().all(|p| p[2] <= 1e-6));
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((lowest + 0.4).abs() < 0.03, "{lowest}");
        let mut rough = op(OpKind::Rough3D, vec![]);
        rough.allowance = 0.02;
        assert!(!cuts(&generate(&rough, M, &J, Some(&model)).unwrap()).is_empty());
        assert!(generate(&rough, M, &J, None).is_err());
    }

    #[test]
    fn empty_selection_is_explained() {
        assert!(generate(&op(OpKind::Profile, vec![]), M, &J, None).unwrap_err().contains("Select"));
        let open = op(OpKind::Pocket, vec![Path::open(vec![[0.0, 0.0], [1.0, 1.0]])]);
        assert!(generate(&open, M, &J, None).unwrap_err().contains("closed"));
    }

    #[test]
    fn gcode_uses_units_origin_and_z_shift() {
        let o = op(OpKind::Drill, vec![Path::closed(circle([2.0, 3.0], 0.2))]);
        let moves = generate(&o, M, &J, None).unwrap();
        let t = o.tool.clone();
        let text = gcode(
            &[PostPath { name: "Drill 1", tool: &t, moves: &moves, dwell: 0.0, steps: None }],
            &PostSettings { inches: true, origin: [1.0, 1.0], z_shift: 0.5, scale: 1.0, arcs: None },
        );
        assert!(text.starts_with("(Written by Burin)\nG20"));
        assert!(text.contains("X1.0000 Y2.0000 Z0.0000"), "{text}");
        assert!(text.trim_end().ends_with("M30"));
        let mm = gcode(&[], &PostSettings { inches: false, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None });
        assert!(mm.contains("G21"));
    }

    #[test]
    fn estimate_counts_feed_and_rapid() {
        let t = tool("End Mill (0.25 inch)");
        let m = [
            Move { p: [0.0, 0.0, 0.0], rapid: true },
            Move { p: [100.0, 0.0, 0.0], rapid: false },
        ];
        // One minute of cutting counts double.
        assert!((estimate_seconds(&m, &t, 200.0, 0.0) - 120.0).abs() < 1e-6);
        let feed = t.feed;
        let plunge = t.plunge;
        let m = [
            Move { p: [0.0, 0.0, 1.0], rapid: true },
            // A fast move, then down, a pause, and up again at the fast rate.
            Move { p: [200.0, 0.0, 1.0], rapid: true },
            Move { p: [200.0, 0.0, -plunge], rapid: false },
            Move { p: [200.0, 0.0, 1.0], rapid: true },
        ];
        let lift = (plunge + 1.0) / 200.0;
        let down = (plunge + 1.0) / plunge;
        let expected = ((down + lift) * 60.0 * 2.0).trunc() + 60.0 + 3.0;
        assert!((estimate_seconds(&m, &t, 200.0, 3.0) - expected).abs() < 1e-6, "{feed}");
    }

    #[test]
    fn pocket_raster_at_an_angle_stays_in_the_shape() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 3.0, 0.0))]);
        o.raster = true;
        o.raster_angle = 45.0;
        o.profile_pass = ProfilePass::None;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(cut.iter().filter(|p| p[2] < 0.0).all(|p| (p[0] - 5.0).abs() < 1.38 && (p[1] - 5.0).abs() < 1.38));
        let near_middle = cut.windows(2).any(|w| {
            w[0][2] < 0.0 && w[1][2] < 0.0 && crate::geom::dist2_seg([5.0, 5.0], [w[0][0], w[0][1]], [w[1][0], w[1][1]]) < 0.04
        });
        assert!(near_middle, "a raster row crosses the middle");
    }

    #[test]
    fn rest_only_pocket_only_cuts_the_wall() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 3.0, 0.0))]);
        o.rest_only = true;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(cut.iter().filter(|p| p[2] < 0.0).all(|p| (p[0] - 5.0).abs().max((p[1] - 5.0).abs()) > 1.3));
    }

    #[test]
    fn peck_drilling_goes_down_in_steps_with_dwell_in_the_file() {
        let mut o = op(OpKind::Drill, vec![Path::closed(circle([2.0, 3.0], 0.2))]);
        o.peck = true;
        o.retract_gap = 0.05;
        o.dwell = 0.5;
        let moves = generate(&o, M, &J, None).unwrap();
        let zs: Vec<f64> = cuts(&moves).iter().map(|p| p[2]).collect();
        assert_eq!(zs, vec![-0.125, -0.25, -0.375, -0.5]);
        let text = gcode(&[PostPath { name: "d", tool: &o.tool, moves: &moves, dwell: o.dwell, steps: None }], &PostSettings { inches: true, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None });
        assert_eq!(text.matches("G4 P0.50").count(), 4, "{text}");
    }

    #[test]
    fn engrave_fill_hatches_inside_the_shape() {
        let mut o = op(OpKind::Engrave, vec![Path::closed(rect([5.0, 5.0], 2.0, 2.0, 0.0))]);
        o.cut_depth = 0.01;
        o.stepover = 0.1;
        o.fill = true;
        o.hatch = true;
        o.cross_hatch = true;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(cut.len() > 40);
        assert!(cut.iter().all(|p| (p[0] - 5.0).abs() <= 1.01 && (p[1] - 5.0).abs() <= 1.01));
        assert!(cut.iter().all(|p| (p[2] + 0.01).abs() < 1e-9));
    }

    #[test]
    fn prism_reaches_its_depth_limit_and_full_depth_is_computed() {
        let shape = vec![Path::closed(rect([5.0, 5.0], 2.0, 2.0, 0.0))];
        let mut o = op(OpKind::Prism, shape.clone());
        o.tool = tool("V-Bit (90 deg 1.25 inch)");
        o.cut_depth = 0.3;
        let lowest = cuts(&generate(&o, M, &J, None).unwrap()).iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((lowest + 0.3).abs() < 1e-6, "{lowest}");
        assert!((widest_inside(&shape) - 1.0).abs() < 0.02);
    }

    #[test]
    fn texture_makes_shallow_strokes_inside_its_depth_range() {
        let mut o = op(OpKind::Texture, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.tool = tool("Ball Nose (0.25 inch)");
        o.stepover = 0.1;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!(lowest < -0.05 && lowest >= -o.tex_cut[1] - 1e-9, "{lowest}");
        assert!(cut.iter().all(|p| (p[0] - 5.0).abs() <= 2.0 && (p[1] - 5.0).abs() <= 2.0));
    }

    #[test]
    fn moulding_follows_the_cross_section_along_the_rail() {
        let rail = Path::open(vec![[1.0, 5.0], [9.0, 5.0]]);
        let profile = Path::open(vec![[0.0, 0.0], [0.5, 0.25], [1.0, 0.0]]);
        let mut o = op(OpKind::Moulding, vec![rail, profile]);
        o.tool = tool("Ball Nose (0.125 inch)");
        o.stepover = 0.1;
        let cut = cuts(&generate(&o, M, &J, None).unwrap());
        let lowest = cut.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((lowest + 0.25).abs() < 0.03, "edges are lowest: {lowest}");
        let middle = cut.iter().filter(|p| (p[1] - 5.0).abs() < 0.06).map(|p| p[2]).fold(-9.0, f64::max);
        assert!(middle > -0.03, "the crown is at the top: {middle}");
        let alone = op(OpKind::Moulding, vec![Path::open(vec![[1.0, 5.0], [9.0, 5.0]])]);
        assert!(generate(&alone, M, &J, None).unwrap_err().contains("rail"));
    }

    #[test]
    fn model_boundary_limits_the_finish_pass() {
        let mut model = Grid::new(10.0, 10.0, 10_000, 0.0);
        let shape = vec![Path::closed(circle([5.0, 5.0], 2.0))];
        for iy in 0..model.ny {
            for ix in 0..model.nx {
                let (x, y) = (ix as f64 * model.cell - 5.0, iy as f64 * model.cell - 5.0);
                model.data[iy * model.nx + ix] = (0.2 - (x * x + y * y) * 0.05).max(0.0) as f32;
            }
        }
        let mut o = op(OpKind::Finish3D, shape);
        o.tool = tool("Ball Nose (0.125 inch)");
        o.raster = true;
        let cut = cuts(&generate(&o, M, &J, Some(&model)).unwrap());
        assert!(!cut.is_empty());
        assert!(cut.iter().all(|p| (p[0] - 5.0).hypot(p[1] - 5.0) <= 2.05), "stays on the model");
    }

    #[test]
    fn gap_below_seats_the_model_on_the_bottom() {
        let m = Machine { gap_below: Some(0.1), ..M };
        assert!((m.model_top(0.5, 0.2) - 0.4).abs() < 1e-9);
        let above = Machine { gap_above: 0.05, ..M };
        assert!((above.model_top(0.5, 0.2) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn tools_convert_between_units() {
        let inch = tool("End Mill (0.25 inch)");
        let mm = inch.in_units(true);
        assert!((mm.dia - 6.35).abs() < 1e-9 && mm.metric);
        assert_eq!(mm.label(), "End Mill (6.35 mm)");
        assert!((mm.in_units(false).dia - 0.25).abs() < 1e-9);
    }


    /// Reads G-code back the way a machine would, returning points along the path it follows.
    fn run_gcode(text: &str) -> Vec<[f64; 3]> {
        let mut at = [0.0f64; 3];
        let mut mode = 0;
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.starts_with('(') && !l.starts_with('M')) {
            let mut end = at;
            let (mut i, mut j) = (None, None);
            for word in line.split_whitespace() {
                let (letter, value) = word.split_at(1);
                let Ok(v) = value.parse::<f64>() else { continue };
                match letter {
                    "G" if (0..=3).contains(&(v as i32)) => mode = v as i32,
                    "X" => end[0] = v,
                    "Y" => end[1] = v,
                    "Z" => end[2] = v,
                    "I" => i = Some(v),
                    "J" => j = Some(v),
                    _ => {}
                }
            }
            if mode >= 2 {
                let (ci, cj) = (i.expect("every arc names its centre"), j.expect("every arc names its centre"));
                let c = [at[0] + ci, at[1] + cj];
                let r = ci.hypot(cj);
                assert!(((end[0] - c[0]).hypot(end[1] - c[1]) - r).abs() < 0.002, "{line}: the arc ends on its circle");
                let (a0, a1) = ((at[1] - c[1]).atan2(at[0] - c[0]), (end[1] - c[1]).atan2(end[0] - c[0]));
                let mut sweep = a1 - a0;
                if mode == 3 && sweep < 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                if mode == 2 && sweep > 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                for s in 1..=32 {
                    let t = a0 + sweep * s as f64 / 32.0;
                    out.push([c[0] + r * t.cos(), c[1] + r * t.sin(), end[2]]);
                }
            } else {
                out.push(end);
            }
            at = end;
        }
        out
    }

    fn profile_output(arcs: Option<f64>, origin: [f64; 2]) -> (String, Vec<Move>, Op) {
        let o = op(OpKind::Profile, vec![Path::closed(circle([5.0, 5.0], 1.2)), Path::closed(rect([2.0, 2.0], 1.0, 1.0, 0.0))]);
        let moves = generate(&o, M, &J, None).unwrap();
        let text = gcode(&[PostPath { name: "p", tool: &o.tool, moves: &moves, dwell: 0.0, steps: None }], &PostSettings { inches: true, origin, z_shift: 0.0, scale: 1.0, arcs });
        (text, moves, o)
    }

    #[test]
    fn a_file_with_arcs_follows_the_same_path_as_one_without() {
        let (text, moves, _) = profile_output(Some(0.0005), [0.0, 0.0]);
        let (plain, _, _) = profile_output(None, [0.0, 0.0]);
        assert!(text.lines().any(|l| l.starts_with("G2 ") || l.starts_with("G3 ")), "{text}");
        assert!(text.lines().count() * 3 < plain.lines().count(), "{} vs {}", text.lines().count(), plain.lines().count());
        // Every point the arcs pass through lies on the original path, at its height.
        let run = run_gcode(&text);
        let segs: Vec<_> = moves.windows(2).filter(|w| !w[1].rapid).map(|w| (w[0].p, w[1].p)).collect();
        for p in run.iter().step_by(3).filter(|p| p[2] < 0.0) {
            let best = segs
                .iter()
                .filter(|(a, b)| (a[2] - p[2]).abs() < 1e-3 && (b[2] - p[2]).abs() < 1e-3)
                .map(|(a, b)| crate::geom::dist2_seg([p[0], p[1]], [a[0], a[1]], [b[0], b[1]]))
                .fold(f64::MAX, f64::min)
                .sqrt();
            assert!(best < 0.002, "{p:?} strays by {best}");
        }
        // And it ends where the original ends.
        let (last, want) = (run[run.len() - 1], moves[moves.len() - 1].p);
        assert!((last[0] - want[0]).abs() < 1e-3 && (last[1] - want[1]).abs() < 1e-3 && (last[2] - want[2]).abs() < 1e-3);
    }

    #[test]
    fn arcs_follow_the_chosen_origin_and_their_centres_stay_relative() {
        let (home, _, _) = profile_output(Some(0.0005), [0.0, 0.0]);
        let (moved, _, _) = profile_output(Some(0.0005), [1.0, 1.0]);
        let a = run_gcode(&home);
        let b = run_gcode(&moved);
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b).filter(|(p, _)| p[2] < 0.0) {
            assert!((p[0] - q[0] - 1.0).abs() < 2e-3 && (p[1] - q[1] - 1.0).abs() < 2e-3, "{p:?} {q:?}");
        }
    }

    #[test]
    fn after_an_arc_the_next_straight_move_names_its_kind() {
        let (text, _, _) = profile_output(Some(0.0005), [0.0, 0.0]);
        let lines: Vec<&str> = text.lines().collect();
        for w in lines.windows(2) {
            if (w[0].starts_with("G2") || w[0].starts_with("G3")) && w[1].starts_with('X') {
                panic!("a bare line after an arc would be read as another arc: {} / {}", w[0], w[1]);
            }
        }
    }

    fn ring_op() -> Op {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.side = Side::On;
        o
    }

    fn first_cut(moves: &[Move]) -> Vec<[f64; 3]> {
        cuts(moves)
    }

    #[test]
    fn hand_set_passes_are_used_only_for_the_depth_they_were_set_for() {
        let mut o = ring_op();
        o.cut_depth = 0.5;
        o.passes = vec![0.1, 0.3, 0.5];
        let z: std::collections::BTreeSet<i64> = cuts(&generate(&o, M, &J, None).unwrap()).iter().map(|p| (p[2] * 1000.0).round() as i64).collect();
        assert!(z.contains(&-100) && z.contains(&-300) && z.contains(&-500), "{z:?}");
        assert_eq!(pass_list(&o), [0.1, 0.3, 0.5]);
        // Changing the depth afterwards goes back to the tool's own passes.
        o.cut_depth = 0.4;
        assert_eq!(pass_list(&o), [0.4]);
    }

    #[test]
    fn the_deepest_cut_is_known_for_each_kind() {
        let mut o = ring_op();
        o.cut_depth = 0.7;
        assert_eq!(deepest_cut(&o), 0.7);
        o.kind = OpKind::Finish3D;
        assert_eq!(deepest_cut(&o), 0.0, "a 3D finish depends on its model");
        o.kind = OpKind::VCarve;
        o.flat_depth = 0.2;
        o.start_depth = 0.1;
        assert!((deepest_cut(&o) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_ramp_can_be_set_by_angle_and_a_spiral_goes_all_the_way_round() {
        let mut o = ring_op();
        o.cut_depth = 0.5;
        o.ramp = true;
        o.ramp_by_angle = true;
        o.ramp_angle = 45.0;
        // At 45 degrees, half an inch down takes half an inch along.
        let c = first_cut(&generate(&o, M, &J, None).unwrap());
        let at = |x: f64, c: &[[f64; 3]]| c.iter().find(|p| (p[0] - x).abs() < 1e-9 && p[2] > -0.5 + 1e-9).is_some();
        assert!(c.iter().any(|p| (p[2] + 0.5).abs() < 1e-9));
        assert!(!at(9.9, &c));
        let down: Vec<_> = c.iter().filter(|p| p[2] > -0.499).collect();
        assert!(down.iter().all(|p| p[2] <= 0.0));
        // Spiral: only at the end of the whole way round does it reach full depth.
        o.ramp_by_angle = false;
        o.ramp_kind = RampKind::Spiral;
        let c = first_cut(&generate(&o, M, &J, None).unwrap());
        let first_full = c.iter().position(|p| (p[2] + 0.5).abs() < 1e-9).unwrap();
        // The corners passed on the way down are all still shallower.
        let corners_on_the_way = c[..first_full].iter().filter(|p| ((p[0] - 5.0).abs() - 2.0).abs() < 1e-9 && ((p[1] - 5.0).abs() - 2.0).abs() < 1e-9).count();
        assert!(corners_on_the_way >= 4, "{corners_on_the_way} in {:?}", &c[..first_full]);
    }

    #[test]
    fn a_zig_zag_goes_down_forward_and_back_before_cutting() {
        let mut o = ring_op();
        o.cut_depth = 0.5;
        o.ramp = true;
        o.ramp_distance = 1.0;
        o.ramp_kind = RampKind::ZigZag;
        o.start_at = StartAt::Keep;
        let c = first_cut(&generate(&o, M, &J, None).unwrap());
        // Start at the first point at the top; 1 along at half depth; back at full depth.
        let start = c[0];
        let turn = c.iter().position(|p| (p[2] + 0.25).abs() < 1e-9).expect("half way down");
        assert!(((c[turn][0] - start[0]).hypot(c[turn][1] - start[1]) - 1.0).abs() < 1e-9, "{:?}", c[turn]);
        let back = c[turn + 1];
        assert!((back[0] - start[0]).abs() < 1e-9 && (back[1] - start[1]).abs() < 1e-9 && (back[2] + 0.5).abs() < 1e-9, "{back:?}");
    }

    fn lead_op(side: Side) -> Op {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = side;
        o.lead = true;
        o.lead_distance = 0.5;
        o.start_at = StartAt::Near(1);
        o
    }

    #[test]
    fn a_lead_comes_in_from_the_waste_side() {
        // Starting at the middle of the top edge.
        let moves = generate(&lead_op(Side::Outside), M, &J, None).unwrap();
        let c = cuts(&moves);
        let lead_start = c[0];
        assert!(lead_start[1] > 7.125 + 0.25, "from above the top, outside the part: {lead_start:?}");
        let moves = generate(&lead_op(Side::Inside), M, &J, None).unwrap();
        let lead_start = cuts(&moves)[0];
        assert!(lead_start[1] < 6.875 - 0.25, "from below the top edge, inside it: {lead_start:?}");
    }

    #[test]
    fn a_straight_lead_at_90_is_square_and_at_0_runs_along_the_cut() {
        let mut o = lead_op(Side::Outside);
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let (a, b) = (c[0], c[1]);
        assert!((a[0] - b[0]).abs() < 1e-3, "straight down onto the edge: {a:?} {b:?}");
        assert!(((a[1] - b[1]).abs() - 0.5).abs() < 1e-3);
        o.lead_angle = 0.0;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        assert!((c[0][1] - c[1][1]).abs() < 1e-3, "in line with the edge: {:?} {:?}", c[0], c[1]);
    }

    #[test]
    fn a_circular_lead_is_tangent_to_the_cut() {
        let mut o = lead_op(Side::Outside);
        o.lead_circular = true;
        o.lead_radius = 0.5;
        o.lead_distance = 0.75;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        // Several points on the arc, all the radius from one centre above the edge.
        let start_of_cut = c.iter().position(|p| (p[1] - 7.125).abs() < 1e-3 && (p[0] - 5.0).abs() < 1e-3).unwrap();
        assert!(start_of_cut >= 3, "{start_of_cut}");
        let centre = [5.0, 7.625];
        for p in &c[..start_of_cut] {
            assert!(((p[0] - centre[0]).hypot(p[1] - centre[1]) - 0.5).abs() < 1e-3, "{p:?}");
        }
    }

    #[test]
    fn a_lead_out_and_an_overcut_carry_on_past_the_start() {
        let mut o = lead_op(Side::Outside);
        o.overcut = 0.3;
        o.lead_out = true;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let start = c[1];
        let last_visit = c.iter().rposition(|p| (p[0] - start[0]).abs() < 1e-3 && (p[1] - start[1]).abs() < 1e-3).unwrap();
        let after = &c[last_visit + 1..];
        assert!(after.len() >= 2, "{after:?}");
        let on_edge = after.iter().filter(|p| (p[1] - start[1]).abs() < 1e-3).map(|p| (p[0] - start[0]).abs()).fold(0.0, f64::max);
        assert!((on_edge - 0.3).abs() < 1e-3, "runs on by the overcut: {on_edge}");
        let (end, away) = (after[after.len() - 2], after[after.len() - 1]);
        assert!(away[1] > end[1] + 0.4, "and leaves on the waste side: {end:?} {away:?}");
    }

    #[test]
    fn a_ramp_can_run_down_the_lead() {
        let mut o = lead_op(Side::Outside);
        o.ramp = true;
        o.ramp_on_lead = true;
        o.lead_circular = true;
        o.lead_radius = 0.5;
        o.lead_distance = 0.75;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(c[0][2].abs() < 1e-9, "starts at the surface: {:?}", c[0]);
        let start = c.iter().position(|p| (p[1] - 7.125).abs() < 1e-3 && (p[0] - 5.0).abs() < 1e-3).unwrap();
        assert!((c[start][2] + 0.25).abs() < 1e-9, "at depth when it reaches the cut");
        assert!(c[1..start].windows(2).all(|w| w[1][2] <= w[0][2]), "going down all the way");
    }

    fn squares(centres: &[[f64; 2]]) -> Vec<Path> {
        centres.iter().map(|c| Path::closed(rect(*c, 0.5, 0.5, 0.0))).collect()
    }

    #[test]
    fn the_order_with_the_least_travel_of_those_ticked_is_used() {
        // A column of shapes: bottom to top wins over left to right, whatever order they came in.
        let mut o = op(OpKind::Profile, squares(&[[2.0, 8.0], [2.02, 2.0], [1.98, 5.0]]));
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.order_choices = Some(OrderChoices { selection: false, left_to_right: true, bottom_to_top: true, grid: false, shortest: false });
        let order = |o: &Op| -> Vec<i64> {
            let c = cuts(&generate(o, M, &J, None).unwrap());
            let mut out: Vec<i64> = Vec::new();
            for p in c {
                let row = (p[1] / 3.0).round() as i64;
                if out.last() != Some(&row) {
                    out.push(row);
                }
            }
            out
        };
        assert_eq!(order(&o), [1, 2, 3], "bottom to top");
        o.order_choices = Some(OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: false });
        assert_eq!(order(&o), [3, 1, 2], "as they were selected");
    }

    #[test]
    fn nothing_ticked_tries_the_same_three_as_the_default() {
        let shapes = squares(&[[2.0, 8.0], [8.0, 2.0], [5.0, 5.0], [2.0, 2.0], [8.0, 8.0]]);
        let mut o = op(OpKind::Profile, shapes);
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.order_choices = Some(OrderChoices::default());
        let usual = cuts(&generate(&o, M, &J, None).unwrap());
        o.order_choices = Some(OrderChoices { selection: false, left_to_right: false, bottom_to_top: false, grid: false, shortest: false });
        assert_eq!(cuts(&generate(&o, M, &J, None).unwrap()), usual);
    }

    #[test]
    fn cells_are_a_size_worked_out_from_the_typical_shape() {
        let box_at = |x: f64, y: f64, size: f64| Placed { start: [x, y], end: [x, y], lo: [x, y], hi: [x + size, y + size] };
        // Fewer than four: all in one.
        let few = [box_at(0.0, 0.0, 1.0), box_at(9.0, 9.0, 1.0)];
        assert_eq!(cell_counts(&few, &[0, 1], 30.0, 30.0), (1, 1));
        // Twenty shapes of size 2, with one huge and one tiny that don't count: cells of 1.5 x 2.
        let mut many: Vec<Placed> = (0..18).map(|i| box_at(i as f64, 0.0, 2.0)).collect();
        many.push(box_at(0.0, 0.0, 500.0));
        many.push(box_at(0.0, 0.0, 0.001));
        let all: Vec<usize> = (0..many.len()).collect();
        assert_eq!(cell_counts(&many, &all, 30.0, 15.0), (10, 5));
        // No size to go by, and no more than the most there can be.
        let points: Vec<Placed> = (0..5).map(|i| box_at(i as f64, 0.0, 0.0)).collect();
        assert_eq!(cell_counts(&points, &[0, 1, 2, 3, 4], 30.0, 30.0), (20, 20));
        assert_eq!(cell_counts(&many, &all, 3000.0, 15.0), (200, 5));
    }

    #[test]
    fn left_to_right_goes_up_one_column_and_down_the_next() {
        let spots: Vec<Placed> = [[1.0, 1.0], [1.0, 9.0], [9.0, 9.0], [9.0, 1.0], [5.0, 1.0], [5.0, 9.0]].iter().map(|s: &P| Placed { start: *s, end: *s, lo: *s, hi: [s[0] + 2.5, s[1] + 2.5] }).collect();
        let level: Vec<usize> = (0..spots.len()).collect();
        assert_eq!(swept(&spots, &level, Sweep::LeftToRight, [0.0, 0.0]), [0, 1, 5, 4, 3, 2]);
        // The row between them has nothing in it, so the top row goes the same way as the bottom.
        assert_eq!(swept(&spots, &level, Sweep::BottomToTop, [0.0, 0.0]), [0, 4, 3, 1, 5, 2]);
    }

    #[test]
    fn passes_follow_the_tools_pass_depth() {
        // Near enough to one pass is one pass, up to 1% over.
        assert_eq!(passes_for(10.0, 10.0, false), [10.0]);
        assert_eq!(passes_for(10.05, 10.0, false), [10.05]);
        assert_eq!(passes_for(10.2, 10.0, false), [10.2], "too little left over to be worth a second pass");
        // Up to a tenth of a pass left over is shared out, not given a pass of its own.
        assert_eq!(passes_for(20.5, 10.0, false).len(), 2);
        assert_eq!(passes_for(21.5, 10.0, false).len(), 3);
        // Keeping exactly to the pass depth leaves the rest for a short last pass.
        assert_eq!(passes_for(21.5, 10.0, true), [10.0, 20.0, 21.5]);
        assert_eq!(passes_for(20.05, 10.0, true).len(), 2, "under 1% left over isn't worth a pass");
        assert_eq!(passes_for(5.0, 0.0, false), [5.0]);
    }

    #[test]
    fn a_last_pass_thickness_is_left_for_the_end() {
        assert_eq!(passes_with_last(21.0, 10.0, 1.0), [10.0, 20.0, 21.0]);
        assert_eq!(passes_with_last(5.0, 10.0, 8.0), [5.0], "more than the whole depth");
        assert_eq!(passes_with_last(21.5, 10.0, 0.0).len(), 3);
    }

    #[test]
    fn a_grid_goes_on_to_the_nearest_cell_each_time() {
        // The rows are further apart than the columns, so it goes along one and back along the next.
        let mut o = op(OpKind::Profile, squares(&[[2.0, 2.0], [4.0, 2.0], [6.0, 2.0], [2.0, 5.0], [4.0, 5.0], [6.0, 5.0]]));
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.order_choices = Some(OrderChoices { selection: false, left_to_right: false, bottom_to_top: false, grid: true, shortest: false });
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let mut seq: Vec<(i64, i64)> = Vec::new();
        for p in c {
            let k = ((p[0] / 2.0).round() as i64, (p[1] / 2.5).round() as i64);
            if seq.last() != Some(&k) {
                seq.push(k);
            }
        }
        assert_eq!(seq, [(1, 1), (2, 1), (3, 1), (3, 2), (2, 2), (1, 2)]);
    }

    #[test]
    fn the_start_can_go_to_any_point_on_a_loop() {
        let path = rect([5.0, 5.0], 4.0, 2.0, 0.0);
        let mut near = path.clone();
        start_loop_near_point(&mut near, spot_on_box(&path, 0).unwrap());
        assert_eq!(near[0], [3.0, 6.0], "top left");
        let mut mid = path.clone();
        start_loop_near_point(&mut mid, spot_on_box(&path, 5).unwrap());
        assert_eq!(mid[0], [5.0, 4.0], "middle of the bottom, a new point on the side");
        assert_eq!(mid.len(), 5);
    }

    #[test]
    fn a_worked_out_loop_starts_level_with_the_start_of_its_shape() {
        // A shape drawn from the middle of its right side, cut outside it.
        let mut shape = rect([50.0, 50.0], 40.0, 40.0, 0.0);
        start_loop_near_point(&mut shape, [70.0, 50.0]);
        assert_eq!(shape[0], [70.0, 50.0]);
        let mut o = op(OpKind::Profile, vec![Path::closed(shape)]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.1;
        o.side = Side::Outside;
        o.start_at = StartAt::Keep;
        let first = cuts(&generate(&o, M, &J, None).unwrap())[0];
        assert!((first[0] - 70.125).abs() < 0.01 && (first[1] - 50.0).abs() < 1e-6, "{first:?}");
    }

    #[test]
    fn sharp_corners_reach_the_true_corner_of_the_offset() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = Side::Outside;
        let far = |o: &Op| cuts(&generate(o, M, &J, None).unwrap()).iter().map(|p| (p[0] - 5.0).hypot(p[1] - 5.0)).fold(0.0, f64::max);
        let round = far(&o);
        o.sharp_corners = true;
        let sharp = far(&o);
        let corner = 2.125f64.hypot(2.125);
        assert!((sharp - corner).abs() < 1e-6, "{sharp} vs {corner}");
        assert!(round < corner - 0.02, "the tool's roundness otherwise: {round}");
        // Shapes inside each other keep the safe rounded way.
        o.paths.push(Path::closed(rect([5.0, 5.0], 1.0, 1.0, 0.0)));
        assert!(far(&o) < corner - 0.02);
    }

    fn in_a_tab(p: &[f64; 3], o: &Op) -> bool {
        // Below the tab's top at a place that is meant to stay standing.
        p[2] < -(o.cut_depth - o.tab_thickness) - 1e-9
    }

    #[test]
    fn ramps_and_overcuts_leave_the_tabs_standing() {
        for kind in [RampKind::Smooth, RampKind::ZigZag, RampKind::Spiral] {
            let mut o = ring_op();
            o.start_at = StartAt::Keep;
            o.cut_depth = 0.5;
            o.tabs = true;
            o.tab_count = 4;
            o.tab_length = 0.5;
            o.tab_thickness = 0.1;
            o.ramp = true;
            o.ramp_kind = kind;
            o.ramp_distance = 100.0;
            o.overcut = 100.0;
            let c = cuts(&generate(&o, M, &J, None).unwrap());
            // A square of 4 from its corner, with a tab in the middle of each side.
            let spot = |p: &[f64; 3]| {
                let (dx, dy) = ((p[0] - 5.0).abs(), (p[1] - 5.0).abs());
                (dx.max(dy) - 2.0).abs() < 1e-6 && dx.min(dy) < 0.25 - 1e-6
            };
            let bad: Vec<_> = c.iter().filter(|p| spot(p) && in_a_tab(p, &o)).collect();
            assert!(bad.is_empty(), "{kind:?} went through a tab: {bad:?}");
            assert!(c.iter().any(|p| (p[2] + 0.5).abs() < 1e-9), "{kind:?} still reaches full depth");
        }
    }

    #[test]
    fn square_corners_give_way_where_they_would_cut_into_the_part() {
        // A notch 0.1 wide, in a shape cut with a 0.25 tool.
        let notched = vec![[3.0, 3.0], [7.0, 3.0], [7.0, 7.0], [5.05, 7.0], [5.05, 5.0], [4.95, 5.0], [4.95, 7.0], [3.0, 7.0]];
        let mut o = op(OpKind::Profile, vec![Path::closed(notched)]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = Side::Outside;
        o.sharp_corners = true;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        for p in &c {
            assert!(distance_to_path([p[0], p[1]], &o.paths[0]) > 0.125 - 0.01, "into the part at {p:?}");
        }
        // A neighbour too close is the same.
        let mut o = op(OpKind::Profile, squares(&[[3.0, 3.0], [3.7, 3.0]]));
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = Side::Outside;
        o.sharp_corners = true;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        for p in &c {
            for s in &o.paths {
                assert!(distance_to_path([p[0], p[1]], s) > 0.125 - 0.01, "into a neighbour at {p:?}");
            }
        }
    }

    #[test]
    fn holes_are_cut_before_the_part_around_them() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 6.0, 6.0, 0.0)), Path::closed(rect([5.0, 5.0], 1.0, 1.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = Side::On;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let near_middle = |p: &[f64; 3]| (p[0] - 5.0).abs() < 1.0 && (p[1] - 5.0).abs() < 1.0;
        assert!(near_middle(&c[0]), "the hole first: {:?}", c[0]);
        assert!(!near_middle(c.last().unwrap()));
        // Whatever order the shapes are chosen in.
        o.order_choices = Some(OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: false });
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        assert!(near_middle(&c[0]));
    }

    #[test]
    fn worked_out_outlines_follow_the_order_shapes_were_clicked() {
        let mut o = op(OpKind::Profile, squares(&[[8.0, 5.0], [2.0, 5.0], [5.0, 5.0]]));
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.side = Side::Outside;
        o.order_choices = Some(OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: false });
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let mut seq: Vec<i64> = Vec::new();
        for p in c {
            let k = (p[0] / 3.0).round() as i64;
            if seq.last() != Some(&k) {
                seq.push(k);
            }
        }
        assert_eq!(seq, [3, 1, 2]);
    }

    fn circle_at(c: P, r: f64) -> Path {
        Path::closed(circle(c, r))
    }

    #[test]
    fn small_holes_on_a_big_sheet_are_all_cut() {
        // A 4 wide hole with a 3.125 tool leaves it very little room, 1100 away from the others.
        let holes: Vec<Path> = [[40.0, 140.0], [190.0, 140.0], [40.0, 670.0], [120.0, 700.0], [40.0, 1185.0], [73.0, 1185.0]].iter().map(|c| circle_at(*c, 2.0)).collect();
        let mut o = op(OpKind::Profile, holes.clone());
        o.tool.dia = 3.125;
        o.tool.pass_depth = 100.0;
        o.cut_depth = 3.0;
        o.side = Side::Inside;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        for h in &holes {
            let b = bbox([h]).unwrap();
            let (x, y) = ((b.0[0] + b.1[0]) / 2.0, (b.0[1] + b.1[1]) / 2.0);
            let near: Vec<_> = c.iter().filter(|p| (p[0] - x).hypot(p[1] - y) < 3.0).collect();
            assert!(!near.is_empty(), "the hole at {x},{y} was lost");
            for p in near {
                assert!(((p[0] - x).hypot(p[1] - y) - (2.0 - 3.125 / 2.0)).abs() < 0.03, "{p:?}");
            }
        }
    }

    #[test]
    fn a_long_slot_barely_wider_than_the_tool_is_still_cut() {
        let slot = Path::closed(rect([50.0, 400.0], 3.3, 513.0, 0.0));
        let far = Path::closed(rect([190.0, 900.0], 3.3, 513.0, 0.0));
        let mut o = op(OpKind::Pocket, vec![slot, far]);
        o.tool.dia = 3.125;
        o.tool.pass_depth = 100.0;
        o.cut_depth = 3.0;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        for (x, y) in [(50.0, 400.0), (190.0, 900.0)] {
            let along: Vec<_> = c.iter().filter(|p| (p[0] - x).abs() < 0.2).collect();
            let top = along.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
            let bottom = along.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
            assert!((top - (y + 256.5 - 3.125 / 2.0)).abs() < 0.2, "{top} for the slot at {x}");
            assert!((bottom - (y - 256.5 + 3.125 / 2.0)).abs() < 0.2, "{bottom} for the slot at {x}");
        }
    }

    #[test]
    fn inside_means_the_material_between_a_shape_and_a_far_hole_within_it() {
        // A plate with a hole far from its edge. Inside the plate means outside the hole.
        let (plate, hole) = (Path::closed(rect([500.0, 500.0], 900.0, 900.0, 0.0)), circle_at([500.0, 500.0], 5.0));
        let mut o = op(OpKind::Profile, vec![plate, hole]);
        o.tool.dia = 2.0;
        o.tool.pass_depth = 100.0;
        o.cut_depth = 1.0;
        o.side = Side::Inside;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let around: Vec<_> = c.iter().filter(|p| (p[0] - 500.0).hypot(p[1] - 500.0) < 50.0).collect();
        assert!(!around.is_empty());
        for p in around {
            assert!(((p[0] - 500.0).hypot(p[1] - 500.0) - 6.0).abs() < 0.03, "{p:?}");
        }
        assert!(c.iter().any(|p| (p[0] - 949.0).abs() < 0.02), "and inside the plate's edge");
        o.side = Side::Outside;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let around: Vec<_> = c.iter().filter(|p| (p[0] - 500.0).hypot(p[1] - 500.0) < 50.0).collect();
        for p in around {
            assert!(((p[0] - 500.0).hypot(p[1] - 500.0) - 4.0).abs() < 0.03, "{p:?}");
        }
    }

    #[test]
    fn close_neighbours_still_share_their_outline() {
        let (a, b) = (Path::closed(rect([100.0, 100.0], 10.0, 10.0, 0.0)), Path::closed(rect([110.0, 100.0], 10.0, 10.0, 0.0)));
        let mut o = op(OpKind::Profile, vec![a, b]);
        o.tool.dia = 2.0;
        o.tool.pass_depth = 100.0;
        o.cut_depth = 1.0;
        o.side = Side::Outside;
        // Only one run around both, not one around each.
        let moves = generate(&o, M, &J, None).unwrap();
        let plunges = moves.windows(2).filter(|w| w[0].rapid && !w[1].rapid).count();
        assert_eq!(plunges, 1);
    }

    #[test]
    fn on_a_closed_line_turns_with_the_cut_direction() {
        for drawn_reversed in [false, true] {
            let mut pts = rect([5.0, 5.0], 4.0, 4.0, 0.0);
            if drawn_reversed {
                pts.reverse();
            }
            let mut o = op(OpKind::Profile, vec![Path::closed(pts)]);
            o.tool.pass_depth = 10.0;
            o.cut_depth = 0.1;
            o.side = Side::On;
            let turning = |o: &Op| {
                let c = cuts(&generate(o, M, &J, None).unwrap());
                signed_area(&c.iter().map(|p| [p[0], p[1]]).collect::<Vec<_>>()) > 0.0
            };
            o.climb = true;
            assert!(!turning(&o), "climb goes clockwise as on the outside");
            o.climb = false;
            assert!(turning(&o));
        }
    }

    /// The spots a pocket's cut leaves behind, that a tool of this size could have reached.
    fn left_in(o: &Op, moves: &[Move]) -> Vec<P> {
        let field = Field::build_to(&o.paths, o.tool.dia, 900, o.tool.dia / 64.0).unwrap();
        let (lo, hi) = bbox(&o.paths).unwrap();
        let r = o.tool.dia / 2.0;
        let cuts: Vec<P> = {
            let mut out = Vec::new();
            for w in moves.windows(2).filter(|w| !w[1].rapid) {
                let (a, b) = (w[0].p, w[1].p);
                let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / (r / 8.0)).ceil() as usize).max(1);
                out.extend((0..=n).map(|k| {
                    let t = k as f64 / n as f64;
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                }));
            }
            out
        };
        let mut left = Vec::new();
        let step = r / 5.0;
        let mut y = lo[1];
        while y <= hi[1] {
            let mut x = lo[0];
            while x <= hi[0] {
                let inside = field.sample(x, y);
                // Reachable: a place the tool fits is within its radius.
                let fits = inside >= r || (0..24).any(|k| {
                    let a = k as f64 / 24.0 * std::f64::consts::TAU;
                    [0.5, 0.9].iter().any(|f: &f64| field.sample(x + a.cos() * r * f, y + a.sin() * r * f) >= r)
                });
                if inside > step && fits && !cuts.iter().any(|c| (c[0] - x).hypot(c[1] - y) <= r + step) {
                    left.push([x, y]);
                }
                x += step;
            }
            y += step;
        }
        left
    }

    fn pocket_of(paths: Vec<Path>, dia: f64, stepover: f64) -> Op {
        let mut o = op(OpKind::Pocket, paths);
        o.tool.dia = dia;
        o.tool.stepover = stepover;
        o.tool.pass_depth = 100.0;
        o.cut_depth = 1.0;
        o
    }

    /// A band bent round a corner, as a slot for a strip of lights is, wider at the bend.
    fn bent_band(width: f64) -> Path {
        let w = width;
        Path::closed(vec![[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [40.0 - w, 30.0], [40.0 - w, w], [0.0, w]])
    }

    #[test]
    fn a_pocket_leaves_nothing_a_tool_could_reach() {
        let star: Vec<P> = (0..10).map(|k| {
            let a = k as f64 / 10.0 * std::f64::consts::TAU;
            let r = if k % 2 == 0 { 30.0 } else { 12.0 };
            [50.0 + r * a.cos(), 50.0 + r * a.sin()]
        }).collect();
        let triangle = vec![[0.0, 0.0], [60.0, 0.0], [5.0, 40.0]];
        for (name, path) in [("bend", bent_band(5.5)), ("wide bend", bent_band(9.0)), ("star", Path::closed(star)), ("triangle", Path::closed(triangle))] {
            for stepover in [0.4, 0.6, 0.9] {
                let o = pocket_of(vec![path.clone()], 3.175, stepover);
                let moves = generate(&o, M, &J, None).unwrap();
                let left = left_in(&o, &moves);
                assert!(left.is_empty(), "{name} at {stepover}: {} spots such as {:?}", left.len(), left.first());
            }
        }
    }

    #[test]
    fn the_spots_cut_stay_clear_of_the_walls_at_every_depth() {
        let mut o = pocket_of(vec![bent_band(9.0)], 3.175, 0.9);
        o.cut_depth = 3.0;
        o.tool.pass_depth = 1.0;
        let moves = generate(&o, M, &J, None).unwrap();
        for m in moves.iter().filter(|m| !m.rapid) {
            let d = distance_to_path([m.p[0], m.p[1]], &o.paths[0]);
            assert!(d >= 3.175 / 2.0 - 0.03, "into the wall at {:?}: {d}", m.p);
            assert!(inside_even_odd(&o.paths, [m.p[0], m.p[1]]));
        }
        assert!(left_in(&o, &moves).is_empty());
        let levels: std::collections::BTreeSet<i64> = moves.iter().filter(|m| !m.rapid).map(|m| (m.p[2] * 100.0).round() as i64).collect();
        assert_eq!(levels.len(), 3, "{levels:?}");
    }

    #[test]
    fn nothing_extra_is_cut_where_the_passes_already_cleared_everything() {
        // A plain square with an ordinary stepover: rings alone, and no plunges added.
        let o = pocket_of(vec![Path::closed(rect([30.0, 30.0], 40.0, 40.0, 0.0))], 3.175, 0.4);
        let field = Field::build_to(&o.paths, o.tool.dia, 900, o.tool.dia / 64.0).unwrap();
        let mut e = Emit { moves: Vec::new(), m: M, project: None, top: 0.0 };
        let mut level = o.tool.dia / 2.0;
        while !field.contours(level).is_empty() {
            for pts in field.contours(level) {
                e.go_to(pts[0][0], pts[0][1], -1.0);
                walk(&mut e, &pts, true, -1.0, &o, None, None);
            }
            level += step_of(&o);
        }
        let spots = leftovers(&field, o.tool.dia / 2.0, o.tool.dia / 2.0, 0.0, &e.moves);
        assert!(spots.is_empty(), "{spots:?}");
    }

    #[test]
    fn a_pocket_for_the_rest_of_a_larger_tool_adds_nothing() {
        let mut o = pocket_of(vec![bent_band(9.0)], 3.175, 0.9);
        o.rest_only = true;
        let moves = generate(&o, M, &J, None).unwrap();
        let plunges = moves.windows(2).filter(|w| w[0].rapid && !w[1].rapid).count();
        assert_eq!(plunges, 1, "only the wall");
    }

    #[test]
    fn long_slanted_slots_beside_each_other_each_get_a_fine_grid() {
        // Their boxes all overlap, but their lines are far apart.
        let slot = |x: f64| Path::closed(vec![[x, 0.0], [x + 400.0, 300.0], [x + 396.0, 305.0], [x - 4.0, 5.0]]);
        let paths = vec![slot(0.0), slot(40.0), slot(80.0)];
        assert_eq!(regions(&paths, 7.0, true).len(), 3);
        let o = pocket_of(paths, 3.175, 0.9);
        let moves = generate(&o, M, &J, None).unwrap();
        assert!(left_in(&o, &moves).is_empty());
        // An island in the middle of a pocket stays with it, whatever the distance.
        let plate = Path::closed(rect([100.0, 100.0], 180.0, 180.0, 0.0));
        let island = Path::closed(circle([100.0, 100.0], 5.0));
        assert_eq!(regions(&[plate.clone(), island.clone()], 7.0, true).len(), 1);
        let o = pocket_of(vec![plate, island], 3.175, 0.4);
        let moves = generate(&o, M, &J, None).unwrap();
        for m in moves.iter().filter(|m| !m.rapid) {
            assert!((m.p[0] - 100.0).hypot(m.p[1] - 100.0) >= 5.0 + 3.175 / 2.0 - 0.05, "into the island at {:?}", m.p);
        }
    }

    #[test]
    fn even_a_thin_sliver_in_a_bend_is_cleared() {
        // A curved band a little wider than two widths of the tool, passes far apart: what is left
        // between them in the bend is thin, and shows on the real part.
        let curve = |r: f64, a: f64| [50.0 + r * a.cos(), 50.0 + r * a.sin()];
        let mut outline: Vec<P> = (0..=90).map(|k| curve(30.0, k as f64 / 90.0 * std::f64::consts::FRAC_PI_2)).collect();
        outline.extend((0..=90).rev().map(|k| curve(30.0 - 6.6, k as f64 / 90.0 * std::f64::consts::FRAC_PI_2)));
        let o = pocket_of(vec![Path::closed(outline)], 3.175, 0.94);
        let moves = generate(&o, M, &J, None).unwrap();
        let field = Field::build_to(&o.paths, o.tool.dia, 900, o.tool.dia / 64.0).unwrap();
        let cuts: Vec<P> = moves.windows(2).filter(|w| !w[1].rapid).flat_map(|w| {
            let (a, b) = (w[0].p, w[1].p);
            let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / 0.1).ceil() as usize).max(1);
            (0..=n).map(move |k| {
                let t = k as f64 / n as f64;
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            })
        }).collect();
        // The middle of the band, all along it: the tool's edge passed over each point.
        for k in 12..78 {
            let p = curve(30.0 - 3.3, k as f64 / 90.0 * std::f64::consts::FRAC_PI_2);
            assert!(field.sample(p[0], p[1]) > 3.0);
            let nearest = cuts.iter().map(|c| (c[0] - p[0]).hypot(c[1] - p[1])).fold(f64::MAX, f64::min);
            assert!(nearest <= 3.175 / 2.0 + 0.03, "{p:?} is {nearest} from any pass");
        }
    }

    #[test]
    fn a_ramp_by_angle_stops_at_its_longest() {
        let mut o = ring_op();
        o.cut_depth = 0.5;
        o.ramp = true;
        o.ramp_by_angle = true;
        o.ramp_angle = 5.0;
        // At 5 degrees, half an inch down takes over five inches. A straight ramp isn't limited.
        let whole = 0.5 / 5f64.to_radians().tan();
        o.ramp_angle_limit = 2.0;
        assert!((ramp_distance(&o, 0.0, -0.5) - whole).abs() < 1e-9);
        // A zig-zag stays within the limit by going there and back more times.
        o.ramp_kind = RampKind::ZigZag;
        let each = 2.0 * 2.0 * 5f64.to_radians().tan();
        let times = (0.5 / each) as usize;
        let times = if 0.5 - times as f64 * each > each * 0.1 { times + 1 } else { times };
        assert_eq!(zig_zag(&o, 0.5), (2.0, times));
        assert!(times > 1);
        // A steeper one, within the limit, drops half the depth on each leg.
        o.ramp_angle = 45.0;
        let (leg, times) = zig_zag(&o, 0.5);
        assert!((leg - 0.25).abs() < 1e-9 && times == 1, "{leg} {times}");
        // An angle that makes no sense counts as 45 degrees.
        o.ramp_angle = 120.0;
        assert_eq!(zig_zag(&o, 0.5), (leg, 1));
    }

    #[test]
    fn peck_depths_round_to_the_nearest_count_and_share_evenly() {
        assert_eq!(peck_depths(0.0, 10.0, 5.0), [-5.0, -10.0]);
        // 10.4 / 5 rounds to 2, but what's left is over 1% of a peck, so one more.
        assert_eq!(peck_depths(0.0, 10.4, 5.0).len(), 3);
        // 12.6 / 5 rounds up to 3, so nothing is left.
        assert_eq!(peck_depths(0.0, 12.6, 5.0).len(), 3);
        // Measured from the start depth.
        assert_eq!(peck_depths(2.0, 6.0, 2.0), [-4.0, -6.0]);
    }

    #[test]
    fn a_circular_lead_turns_at_most_a_quarter_of_the_way_round() {
        let mut o = lead_op(Side::Outside);
        o.lead_circular = true;
        o.lead_radius = 1.0;
        o.lead_distance = 100.0;
        let far = |points: Vec<P>| points.iter().map(|q| q[0].hypot(q[1])).fold(0.0, f64::max);
        // A quarter turn of radius 1 ends the square root of 2 away.
        assert!((far(lead_points([0.0, 0.0], [1.0, 0.0], 1.0, &o, false)) - 2f64.sqrt()).abs() < 1e-9);
        // A straight one at a meaningless angle comes in square.
        o.lead_circular = false;
        o.lead_distance = 2.0;
        o.lead_angle = 120.0;
        let start = lead_points([0.0, 0.0], [1.0, 0.0], 1.0, &o, false)[0];
        assert!(start[0].abs() < 1e-9 && (start[1].abs() - 2.0).abs() < 1e-9, "{start:?}");
    }

    #[test]
    fn the_real_depth_is_measured_from_the_cutting_moves() {
        let m = |z: f64, rapid: bool| Move { p: [0.0, 0.0, z], rapid };
        assert_eq!(deepest_move(&[m(5.0, true), m(-3.0, false), m(-1.0, false), m(-9.0, true)]), 3.0, "a rapid is no cut");
        assert_eq!(deepest_move(&[m(5.0, true)]), 0.0);
    }

    /// Small shapes scattered as drilled parts are: a block of them, then another far away.
    fn scattered() -> Vec<Path> {
        let mut paths = Vec::new();
        for (cx, cy) in [(20.0, 20.0), (300.0, 40.0), (40.0, 300.0), (320.0, 310.0)] {
            for row in 0..4 {
                for col in 0..4 {
                    paths.push(Path::closed(rect([cx + col as f64 * 12.0, cy + row as f64 * 12.0 + col as f64], 6.0, 6.0, 0.0)));
                }
            }
        }
        paths
    }

    fn rapid_travel(moves: &[Move]) -> f64 {
        moves.windows(2).filter(|w| w[1].rapid).map(|w| (w[1].p[0] - w[0].p[0]).hypot(w[1].p[1] - w[0].p[1])).sum()
    }

    #[test]
    fn with_several_orders_ticked_the_travel_is_short() {
        let mut o = op(OpKind::Profile, scattered());
        o.tool.dia = 2.0;
        o.tool.pass_depth = 10.0;
        o.cut_depth = 1.0;
        o.side = Side::On;
        o.order_choices = Some(OrderChoices { selection: false, left_to_right: true, bottom_to_top: false, grid: false, shortest: false });
        let left_to_right = rapid_travel(&generate(&o, M, &J, None).unwrap());
        o.order_choices = Some(OrderChoices::default());
        let several = rapid_travel(&generate(&o, M, &J, None).unwrap());
        assert!(several < left_to_right * 0.6, "{several} against {left_to_right}");
        // Each block is finished before going on to the next, near enough a tour of the four.
        assert!(several < 1900.0, "{several}");
    }

    #[test]
    fn a_single_order_ticked_is_kept_to() {
        let mut o = op(OpKind::Profile, scattered());
        o.tool.dia = 2.0;
        o.tool.pass_depth = 10.0;
        o.cut_depth = 1.0;
        o.side = Side::On;
        o.order_choices = Some(OrderChoices { selection: false, left_to_right: true, bottom_to_top: false, grid: false, shortest: false });
        let starts: Vec<f64> = generate(&o, M, &J, None).unwrap().windows(2).filter(|w| w[0].rapid && !w[1].rapid).map(|w| w[1].p[0]).collect();
        assert!(starts.windows(2).all(|w| w[1] >= w[0] - 6.1), "left to right, whatever the travel: {starts:?}");
    }

    #[test]
    fn the_shortest_path_is_the_best_loop_back_to_the_start() {
        let length = |list: &[usize], spots: &[P], from: P| {
            let mut at = from;
            let mut sum = 0.0;
            for k in list {
                sum += (spots[*k][0] - at[0]).hypot(spots[*k][1] - at[1]);
                at = spots[*k];
            }
            sum + (from[0] - at[0]).hypot(from[1] - at[1])
        };
        // Along a line and back.
        let line: Vec<P> = [10.0, -11.0, 22.0, -24.0, 40.0, -44.0, 70.0, -80.0].iter().map(|x: &f64| [*x, 0.0]).collect();
        let way = shortest_tour(&line, [0.0, 0.0]);
        assert!((length(&way, &line, [0.0, 0.0]) - 300.0).abs() < 1e-6, "{way:?}");
        // Round a ring, however the spots were listed.
        let ring: Vec<P> = (0..60).map(|i| (i * 37 % 60) as f64 * std::f64::consts::TAU / 60.0).map(|a: f64| [100.0 * a.cos(), 100.0 * a.sin()]).collect();
        let way = shortest_tour(&ring, [100.0, 0.0]);
        let mut seen = way.clone();
        seen.sort();
        assert_eq!(seen, (0..60).collect::<Vec<_>>());
        assert!(length(&way, &ring, [100.0, 0.0]) < std::f64::consts::TAU * 100.0 * 1.001);
        // Fewer than three are left as they came.
        assert_eq!(shortest_tour(&[[9.0, 9.0], [1.0, 1.0]], [0.0, 0.0]), [0, 1]);
    }

    #[test]
    fn the_way_back_counts_when_choosing_between_orders() {
        // Four on a line, and the tool in line with them.
        let spots: Vec<Placed> = [[5.0, 0.0], [1.0, 0.0], [3.0, 0.0], [7.0, 0.0]].iter().map(|s: &P| Placed { start: *s, end: *s, lo: *s, hi: *s }).collect();
        let level = [0, 1, 2, 3];
        let selection = OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: true };
        // As chosen: 5 + 4 + 2 + 4 + 7 = 22; the best loop there and back is 14.
        assert_eq!(order_level(&spots, &level, selection, [0.0, 0.0]).len(), 4);
        assert_ne!(order_level(&spots, &level, selection, [0.0, 0.0]), level);
    }

    #[test]
    fn deeper_shapes_are_ordered_from_a_fixed_point_with_the_inside_cut_first() {
        // A plate holding a frame holding two holes.
        let shape = |c: P, side: f64| (rect(c, side, side, 0.0), true);
        let items = vec![shape([50.0, 50.0], 90.0), shape([50.0, 50.0], 40.0), shape([45.0, 50.0], 4.0), shape([55.0, 50.0], 4.0)];
        let order = sequence_order(&items, OrderChoices::default(), [0.0, 0.0, 0.0]);
        assert_eq!(order[2..], [1, 0], "each after what it holds");
        assert!(order[..2].contains(&2) && order[..2].contains(&3));
    }

    #[test]
    fn crossings_are_undone_in_the_way_round() {
        // Nearest-first alone goes along a line and has to come all the way back.
        let spots: Vec<(Vec<P>, bool)> = [10.0, -11.0, 22.0, -24.0, 40.0, -44.0, 70.0, -80.0].iter().map(|x: &f64| (vec![[*x, 0.0]], false)).collect();
        let way = shortest_way(spots, [0.0, 0.0]);
        let travel = way.windows(2).map(|w| (w[1].0[0][0] - end_of(&w[0])[0]).hypot(w[1].0[0][1] - end_of(&w[0])[1])).sum::<f64>();
        // The best goes all the way to one end and back to the other.
        assert!(travel <= 70.0 * 2.0 + 80.0 + 1e-6, "{travel}");
    }

    #[test]
    fn what_is_inside_another_stays_first_however_short_the_way() {
        let mut paths = scattered();
        let plate = Path::closed(rect([35.0, 35.0], 70.0, 70.0, 0.0));
        paths.push(plate);
        let mut o = op(OpKind::Profile, paths);
        o.tool.dia = 2.0;
        o.tool.pass_depth = 10.0;
        o.cut_depth = 1.0;
        o.side = Side::On;
        let moves = generate(&o, M, &J, None).unwrap();
        let starts: Vec<[f64; 3]> = moves.windows(2).filter(|w| w[0].rapid && !w[1].rapid).map(|w| w[1].p).collect();
        // The plate's outline is cut after all sixteen squares it holds.
        let plate_at = starts.iter().position(|p| (p[0] - 35.0).abs() > 34.9 || (p[1] - 35.0).abs() > 34.9).unwrap();
        let held_before = starts[..plate_at].iter().filter(|p| p[0] < 70.0 && p[1] < 70.0).count();
        assert_eq!(held_before, 16);
    }

    #[test]
    fn a_pocket_stays_down_from_ring_to_ring_and_within_its_walls() {
        let mut o = pocket_of(vec![Path::closed(rect([50.0, 50.0], 60.0, 40.0, 0.0))], 3.175, 0.4);
        o.cut_depth = 2.0;
        o.tool.pass_depth = 1.0;
        let moves = generate(&o, M, &J, None).unwrap();
        // One way down for each of the two depths, not one for each ring.
        let plunges = moves.windows(2).filter(|w| w[0].rapid && !w[1].rapid).count();
        assert_eq!(plunges, 2);
        for m in moves.iter().filter(|m| !m.rapid) {
            let d = distance_to_path([m.p[0], m.p[1]], &o.paths[0]);
            assert!(d >= 3.175 / 2.0 - 0.03, "into the wall at {:?}: {d}", m.p);
        }
        assert!(left_in(&o, &moves).is_empty());
    }

    #[test]
    fn a_pocket_lifts_rather_than_cross_an_island() {
        // Two lobes joined by a neck narrower than the tool: nothing can pass between them.
        let shape = vec![[0.0, 0.0], [40.0, 0.0], [40.0, 18.5], [60.0, 18.5], [60.0, 0.0], [100.0, 0.0], [100.0, 40.0], [60.0, 40.0], [60.0, 21.5], [40.0, 21.5], [40.0, 40.0], [0.0, 40.0]];
        let o = pocket_of(vec![Path::closed(shape)], 6.0, 0.4);
        let moves = generate(&o, M, &J, None).unwrap();
        for w in moves.windows(2).filter(|w| !w[1].rapid) {
            let mid = [(w[0].p[0] + w[1].p[0]) / 2.0, (w[0].p[1] + w[1].p[1]) / 2.0];
            assert!(distance_to_path(mid, &o.paths[0]) >= 3.0 - 0.05, "cutting into the neck at {mid:?}");
        }
    }

    #[test]
    fn an_optimised_start_is_the_point_of_the_loop_nearest_the_tool() {
        // The corners, not a point part way along a side.
        let mut pts = rect([50.0, 50.0], 80.0, 20.0, 0.0);
        start_near(&mut pts, [100.0, 0.0]);
        assert_eq!(pts[0], [90.0, 40.0]);
        let mut pts = rect([50.0, 50.0], 80.0, 20.0, 0.0);
        start_near(&mut pts, [0.0, 100.0]);
        assert_eq!(pts[0], [10.0, 60.0]);
        // On a round shape, whichever point is nearest.
        let mut round = circle([50.0, 50.0], 10.0);
        start_near(&mut round, [200.0, 50.0]);
        assert!((round[0][0] - 60.0).abs() < 0.1 && (round[0][1] - 50.0).abs() < 2.0, "{:?}", round[0]);
    }

    #[test]
    fn each_chosen_point_moves_the_start_of_a_shape() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([50.0, 50.0], 80.0, 20.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.1;
        o.side = Side::On;
        let spots = [[10.0, 60.0], [50.0, 60.0], [90.0, 60.0], [90.0, 50.0], [90.0, 40.0], [50.0, 40.0], [10.0, 40.0], [10.0, 50.0]];
        for (i, spot) in spots.iter().enumerate() {
            o.start_at = StartAt::Near(i as u8);
            let first = cuts(&generate(&o, M, &J, None).unwrap())[0];
            assert!((first[0] - spot[0]).abs() < 1e-6 && (first[1] - spot[1]).abs() < 1e-6, "{i}: {first:?}");
        }
    }

    #[test]
    fn an_open_line_starts_from_the_end_nearer_the_chosen_point() {
        let mut o = op(OpKind::Profile, vec![Path { pts: vec![[10.0, 10.0], [90.0, 10.0]], closed: false }]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.start_at = StartAt::Near(7);
        assert!((cuts(&generate(&o, M, &J, None).unwrap())[0][0] - 10.0).abs() < 1e-6, "left");
        o.start_at = StartAt::Near(3);
        assert!((cuts(&generate(&o, M, &J, None).unwrap())[0][0] - 90.0).abs() < 1e-6, "right");
    }

    #[test]
    fn leads_leave_the_start_where_it_is_unless_a_point_is_chosen() {
        let path = rect([50.0, 50.0], 80.0, 20.0, 0.0);
        let mut o = op(OpKind::Profile, vec![Path::closed(path.clone())]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.lead = true;
        o.lead_distance = 1.0;
        o.start_at = StartAt::Optimize;
        let optimised = generate(&o, M, &J, None).unwrap();
        o.start_at = StartAt::Keep;
        assert_eq!(cuts(&optimised), cuts(&generate(&o, M, &J, None).unwrap()));
        o.start_at = StartAt::Near(5);
        assert_ne!(cuts(&optimised), cuts(&generate(&o, M, &J, None).unwrap()));
    }

    #[test]
    fn a_file_only_goes_home_at_its_very_start_and_end() {
        let o = |at: P| {
            let mut o = op(OpKind::Profile, vec![Path::closed(rect(at, 1.0, 1.0, 0.0))]);
            o.tool.pass_depth = 10.0;
            o.cut_depth = 0.1;
            o.side = Side::On;
            o
        };
        let moves = generate(&o([5.0, 5.0]), M, &J, None).unwrap();
        assert_eq!(moves[0].p, M.home, "a toolpath on its own starts at home");
        assert_eq!(moves[moves.len() - 1].p, M.home, "and ends there");
        let working = working_moves(&moves);
        assert!(working.iter().all(|m| m.p != M.home), "the trip is left out");
        let new_home = [40.0, 30.0, 0.8];
        let (first, _) = route_in_file(&moves, None, new_home, true, false);
        assert_eq!(first[0].p, new_home, "the home position as it is now, not when calculated");
        assert_ne!(first[first.len() - 1].p, new_home);
        let (middle, _) = route_in_file(&moves, None, new_home, false, false);
        assert_eq!(middle.as_slice(), working);
        let (last, _) = route_in_file(&moves, None, new_home, false, true);
        assert_ne!(last[0].p, new_home);
        assert_eq!(last[last.len() - 1].p, new_home);
    }

    #[test]
    fn a_raster_pocket_goes_row_to_row_without_lifting() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([20.0, 20.0], 30.0, 30.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.raster = true;
        o.raster_angle = 0.0;
        o.profile_pass = ProfilePass::None;
        let moves = generate(&o, M, &J, None).unwrap();
        let cut_rows: Vec<&Move> = moves.iter().filter(|m| !m.rapid).collect();
        let first_cut = moves.iter().position(|m| !m.rapid).unwrap();
        let first_lift = moves[first_cut..].iter().position(|m| m.rapid).unwrap() + first_cut;
        // Every row on the way, in one go down.
        let ys: std::collections::BTreeSet<i64> = moves[first_cut..first_lift].iter().map(|m| (m.p[1] * 1000.0).round() as i64).collect();
        assert!(ys.len() > 10, "{} rows before lifting, of {} cuts", ys.len(), cut_rows.len());
        // Nothing cut is closer to the wall than the tool's radius.
        let r = o.tool.dia / 2.0;
        assert!(moves[first_cut..first_lift].iter().all(|m| m.p[0] >= 5.0 + r - 0.05 && m.p[0] <= 35.0 - r + 0.05 && m.p[1] >= 5.0 + r - 0.05 && m.p[1] <= 35.0 - r + 0.05));
    }

    #[test]
    fn pocket_rows_cover_the_box_from_below_its_corner() {
        let rows = pocket_rows([0.0, 0.0], [30.0, 40.0], 0.0, 2.0);
        // Half the diagonal is 25, and they reach 5% past that all round.
        let reach = 25.0 * 1.05;
        assert!((rows[0].0[0] - (15.0 - reach)).abs() < 1e-9 && (rows[0].0[1] - (20.0 - reach)).abs() < 1e-9);
        assert_eq!(rows[0].1, [1.0, 0.0]);
        assert_eq!(rows.len(), 27);
        assert!((rows[1].0[1] - rows[0].0[1] - 2.0).abs() < 1e-9);
        assert!(rows.last().unwrap().0[1] >= 40.0, "up past the top");
    }

    #[test]
    fn raster_rows_stop_short_of_the_outline_pass_by_the_allowance() {
        let extent = |allowance: f64, pass: ProfilePass| {
            let mut o = op(OpKind::Pocket, vec![Path::closed(rect([20.0, 20.0], 30.0, 30.0, 0.0))]);
            o.tool.pass_depth = 10.0;
            o.cut_depth = 0.25;
            o.raster = true;
            o.raster_angle = 0.0;
            o.profile_pass = pass;
            o.raster_allowance = allowance;
            let moves = generate(&o, M, &J, None).unwrap();
            // The first cut along a row, before any outline.
            let w = moves.windows(2).find(|w| !w[1].rapid && (w[1].p[0] - w[0].p[0]).abs() > 1.0 && (w[1].p[1] - w[0].p[1]).abs() < 1e-9).unwrap();
            w[0].p[0].min(w[1].p[0])
        };
        let none = extent(0.0, ProfilePass::None);
        assert!((extent(1.0, ProfilePass::Last) - none - 1.0).abs() < 0.1, "stops 1 further in");
        assert!((extent(1.0, ProfilePass::None) - none).abs() < 1e-6, "with no outline to finish, it goes to the wall");
    }

    #[test]
    fn a_raster_pocket_lifts_rather_than_cross_an_island() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([20.0, 20.0], 30.0, 30.0, 0.0)), Path::closed(rect([20.0, 20.0], 10.0, 10.0, 0.0))]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.raster = true;
        o.raster_angle = 0.0;
        o.profile_pass = ProfilePass::None;
        let moves = generate(&o, M, &J, None).unwrap();
        let r = o.tool.dia / 2.0;
        let in_island = |p: [f64; 3]| (p[0] - 20.0).abs() < 5.0 + r - 0.05 && (p[1] - 20.0).abs() < 5.0 + r - 0.05;
        for w in moves.windows(2).filter(|w| !w[1].rapid && w[1].p[2] < 0.0) {
            for k in 0..=20 {
                let t = k as f64 / 20.0;
                let q = [w[0].p[0] + (w[1].p[0] - w[0].p[0]) * t, w[0].p[1] + (w[1].p[1] - w[0].p[1]) * t, 0.0];
                assert!(!in_island(q), "{:?} to {:?}", w[0].p, w[1].p);
            }
        }
    }

    #[test]
    fn a_new_depth_keeps_the_way_passes_were_set_up() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([20.0, 20.0], 30.0, 30.0, 0.0))]);
        o.tool.pass_depth = 2.0;
        o.cut_depth = 5.0;
        // A list for another depth doesn't count, but what was asked for does.
        o.passes = vec![2.0, 3.0];
        o.last_thickness = 1.0;
        assert_eq!(pass_list(&o), [2.0, 4.0, 5.0]);
        o.last_thickness = 0.0;
        o.keep_exact = true;
        o.cut_depth = 5.0;
        assert_eq!(pass_list(&o), [2.0, 4.0, 5.0]);
        o.keep_exact = false;
        assert_eq!(pass_list(&o).len(), 3);
        // A saved list for this very depth is used as it stands.
        o.last_thickness = 1.0;
        o.passes = vec![1.5, 5.0];
        assert_eq!(pass_list(&o), [1.5, 5.0]);
    }

    #[test]
    fn a_kept_start_point_stays_kept() {
        let path = rect([50.0, 50.0], 80.0, 20.0, 0.0);
        let mut o = op(OpKind::Profile, vec![Path::closed(path.clone())]);
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.1;
        o.side = Side::On;
        o.start_at = StartAt::Keep;
        let first = cuts(&generate(&o, M, &J, None).unwrap())[0];
        assert!(path.iter().any(|p| (p[0] - first[0]).abs() < 1e-9 && (p[1] - first[1]).abs() < 1e-9), "{first:?}");
    }
}

