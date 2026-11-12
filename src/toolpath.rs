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
    /// How deep the tool can cut before its shank or holder would rub. 0 when not given.
    #[serde(default)]
    pub flute_length: f64,
    /// The group it's kept in in the tool list, if any (the original's Tool Database groups: "New Group", then tools
    /// dragged onto it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// The stepover a V-carve clears with between passes, in the tool's unit; 0 for none. The
    /// original's tools keep it beside the usual stepover (field +0x98, used by slot +0xa8 of
    /// `mcVBitTool` at `0x1408667f0`, falling back to the stepover at +0x90), and a saved tool
    /// writes it just after its name (0.794 for a 3.175 mm V-bit whose stepover is 0.133).
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub clear_step: f64,
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

impl ToolKind {
    pub fn name(self) -> &'static str {
        match self {
            ToolKind::EndMill => crate::i18n::tr("End Mill"),
            ToolKind::BallNose => crate::i18n::tr("Ball Nose"),
            ToolKind::VBit => crate::i18n::tr("V-Bit"),
            ToolKind::Drill => crate::i18n::tr("Drill"),
            ToolKind::Engraving => crate::i18n::tr("Engraving"),
        }
    }
}

impl Tool {
    pub fn label(&self) -> String {
        let unit = if self.metric { "mm" } else { crate::i18n::tr("inch") };
        let kind = self.kind.name();
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
            t.flute_length *= k;
            t.clear_step *= k;
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
            flute_length: 0.0,
            group: String::new(),
            clear_step: 0.0,
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
            OpKind::Profile => crate::i18n::tr("Profile cut"),
            OpKind::Pocket => crate::i18n::tr("Pocket"),
            OpKind::Drill => crate::i18n::tr("Drilling"),
            OpKind::Engrave => crate::i18n::tr("Quick engrave"),
            OpKind::VCarve => crate::i18n::tr("V-carve"),
            OpKind::Rough3D => crate::i18n::tr("3D roughing"),
            OpKind::Finish3D => crate::i18n::tr("3D finishing"),
            OpKind::Prism => crate::i18n::tr("Prism carve"),
            OpKind::Texture => crate::i18n::tr("Texture"),
            OpKind::Moulding => crate::i18n::tr("Moulding"),
            OpKind::Flute => crate::i18n::tr("Fluting"),
            OpKind::Chamfer => crate::i18n::tr("Chamfer"),
        }
    }
    pub fn base_name(self) -> &'static str {
        match self {
            OpKind::Profile => crate::i18n::tr("Profile"),
            OpKind::Pocket => crate::i18n::tr("Pocket"),
            OpKind::Drill => crate::i18n::tr("Drill"),
            OpKind::Engrave => crate::i18n::tr("Quick Engrave"),
            OpKind::VCarve => crate::i18n::tr("V-Carve"),
            OpKind::Rough3D => crate::i18n::tr("3D Roughing"),
            OpKind::Finish3D => crate::i18n::tr("3D Finish"),
            OpKind::Prism => crate::i18n::tr("Prism Carve"),
            OpKind::Texture => crate::i18n::tr("Texture"),
            OpKind::Moulding => crate::i18n::tr("Swept Profile"),
            OpKind::Flute => crate::i18n::tr("Flute"),
            OpKind::Chamfer => crate::i18n::tr("Chamfer"),
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
    /// Where the tabs were put on the drawing, as the original keeps them with each shape
    /// (`vcToolpathTab`): each tab is centred on the loop at the point nearest one of these.
    /// None: `tab_count` tabs evenly spaced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tab_spots: Vec<P>,
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
    /// Clear the pocket adaptively: the cutter never takes more than its stepover's width
    /// (`adaptive_paths`). Not in the original; from other CAM programs.
    #[serde(default)]
    pub adaptive: bool,
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
    /// Open lines are cut there and back, one pass each way, without lifting.
    #[serde(default)]
    pub zigzag_open: bool,
    /// How much further from the walls raster rows stop, when an outline pass cleans up after them.
    #[serde(default)]
    pub raster_allowance: f64,
    /// A second set of raster rows, at the angle + 90 degrees, after the first (`_pkpdDo2ndRaster`).
    #[serde(default)]
    pub raster_second: bool,
    /// V-Carve with an end mill: clear the flat inside the first ring as well, a ring every stepover
    /// until nothing is left, as F-Engrave's clean-up does. Not from the original, so off at first.
    #[serde(default)]
    pub clear_flat: bool,
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
    /// Made by merging other toolpaths: its moves are all there is, and it can't be worked out again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merged: bool,
    /// The person's notes about this toolpath.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Sums the depths were typed as, worked out again when the document's variables change, as the
    /// original keeps `StartDepthFormula`, `CutDepthFormula`, `AllowanceFormula` and `FlatDepthFormula`.
    #[serde(default, skip_serializing_if = "crate::toolpath::Formulas::is_empty")]
    pub formulas: Formulas,
    /// Read from another program's machine file: its moves are all there is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub imported: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub p: [f64; 3],
    pub rapid: bool,
    /// A join: cutting speed from one cut to the next without lifting, as the original marks
    /// them (its move type 14, from element kind 18; every one in the saved jobs goes across at
    /// one depth between two cuts).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub join: bool,
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

/// A single pass does if the tool's pass depth reaches this far past the depth.
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
    // A last pass as thick as the whole cut leaves nothing before it: one pass. The original's
    // list would start with a pass at no depth at all (`0x1408779b0` adds the 0 it's given), but
    // its saved jobs never cut there, and a pass at the start depth removes nothing.
    if rest < SAME_PASS {
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
    /// Closer than this counts as the same spot.
    same_spot: f64,
}

/// A thousandth of a millimetre.
const SAME_SPOT_MM: f64 = 0.001;

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
        self.moves.push(Move { p, rapid: true, join: false });
    }
    fn feed(&mut self, x: f64, y: f64, z: f64) {
        let z = self.z_at(x, y, z);
        self.moves.push(Move { p: [x, y, z], rapid: false, join: false });
    }
    /// From one cut to the next at cutting speed without lifting: a join, as the original marks it.
    fn join(&mut self, x: f64, y: f64, z: f64) {
        self.feed(x, y, z);
        if let Some(m) = self.moves.last_mut() {
            m.join = true;
        }
    }
    fn retract(&mut self) {
        let l = self.last();
        if l[2] < self.m.safe_z {
            self.rapid([l[0], l[1], self.m.safe_z]);
        }
    }
    /// Lifts, travels, and comes down to the start of a cut.
    fn go_to(&mut self, x: f64, y: f64, z: f64) {
        // Already in the material right there: straight on down, with no need to lift.
        let l = self.last();
        if l[2] < self.m.safe_z && (l[0] - x).hypot(l[1] - y) < self.same_spot {
            if (l[2] - z).abs() > self.same_spot {
                self.feed(x, y, z);
            }
            return;
        }
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
/// How much of a lead a zig-zag on it goes along before it comes back (`0x1421a0400`).
const ZIG_ON_LEAD: f64 = 0.95;
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

/// The stretches of a closed loop `total` long the tool stays up over for its tabs, measured from
/// a start `shift` further on than the one they were spaced from (each also a loop before and
/// after, for one across the start).
fn tab_spans(pts: &[P], total: f64, op: &Op, shift: f64) -> Vec<(f64, f64)> {
    let mids: Vec<f64> = if op.tab_spots.is_empty() {
        (0..op.tab_count).map(|i| (total * (i as f64 + 0.5) / op.tab_count as f64 - shift).rem_euclid(total.max(1e-12))).collect()
    } else {
        // The drawing's tabs on this loop: those no further from it than the cut is from its
        // shape, at the place on the loop nearest each.
        let reach = op.tool.dia / 2.0 + op.allowance.abs() + op.last_allowance.abs() + op.tool.dia * 0.01;
        op.tab_spots.iter().filter_map(|spot| nearest_station(pts, *spot).filter(|(_, d)| *d <= reach).map(|(s, _)| s)).collect()
    };
    let mut out = Vec::new();
    for mid in mids {
        for m in [mid - total, mid, mid + total] {
            out.push((m - tab_reach(op) / 2.0, m + tab_reach(op) / 2.0));
        }
    }
    out
}

/// `count` tabs on each closed shape, spread evenly along it from its start, as Burin spaces them
/// when none are put by hand: the places to put on the drawing to start from.
pub fn spread_tabs(paths: &[Path], count: usize) -> Vec<P> {
    let mut out = Vec::new();
    for path in paths.iter().filter(|p| p.closed && p.pts.len() >= 3) {
        let mut route = path.pts.clone();
        route.push(path.pts[0]);
        let total: f64 = route.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        out.extend((0..count).map(|i| along(&route, total * (i as f64 + 0.5) / count as f64)));
    }
    out
}

/// Tabs on shapes `before` carried onto the same shapes changed (`after`): each at the same
/// piece of its shape and the same share along it when the shape has as many points as before,
/// else at the same share of the way round it; when the shapes aren't the same number, at the
/// place on them nearest where it was.
pub fn carry_tabs(before: &[Path], after: &[Path], tabs: &[P]) -> Vec<P> {
    let length = |p: &Path| p.segments().map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1])).sum::<f64>();
    tabs.iter()
        .map(|spot| {
            // Where it is on the shapes before: which shape, which piece, how far along it.
            let mut best: Option<(f64, usize, usize, f64, f64)> = None;
            for (i, path) in before.iter().enumerate() {
                let mut gone = 0.0;
                for (k, (a, b)) in path.segments().enumerate() {
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let len2 = dx * dx + dy * dy;
                    let t = if len2 > 0.0 { (((spot[0] - a[0]) * dx + (spot[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let d = (a[0] + dx * t - spot[0]).hypot(a[1] + dy * t - spot[1]);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, i, k, t, gone + t * len2.sqrt()));
                    }
                    gone += len2.sqrt();
                }
            }
            let Some((_, i, k, t, gone)) = best else { return *spot };
            match (before.len() == after.len(), before.get(i), after.get(i)) {
                (true, Some(old), Some(new)) if old.pts.len() == new.pts.len() && old.closed == new.closed => {
                    let (a, b) = new.segments().nth(k).unwrap_or((new.pts[0], new.pts[0]));
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                }
                (true, Some(old), Some(new)) => {
                    let share = gone / length(old).max(1e-12);
                    let mut route = new.pts.clone();
                    if new.closed {
                        route.push(new.pts[0]);
                    }
                    along(&route, share * length(new))
                }
                _ => nearest_on_paths(after, *spot).map_or(*spot, |(q, _)| q),
            }
        })
        .collect()
}

/// The point of these shapes nearest `at`, and how far it is.
pub fn nearest_on_paths(paths: &[Path], at: P) -> Option<(P, f64)> {
    paths
        .iter()
        .flat_map(|p| p.segments())
        .map(|(a, b)| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let q = [a[0] + dx * t, a[1] + dy * t];
            (q, (q[0] - at[0]).hypot(q[1] - at[1]))
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
}

/// Whether a toolpath leaves tabs on its loops: some counted, or some put on the drawing.
fn tabs_wanted(op: &Op) -> bool {
    op.tab_count > 0 || !op.tab_spots.is_empty()
}

/// How far along a closed loop the place nearest `spot` is, and how far that place is from it.
fn nearest_station(pts: &[P], spot: P) -> Option<(f64, f64)> {
    let n = pts.len();
    let mut acc = 0.0;
    let mut best: Option<(f64, f64)> = None;
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((spot[0] - a[0]) * dx + (spot[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (a[0] + dx * t - spot[0]).hypot(a[1] + dy * t - spot[1]);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((acc + t * len2.sqrt(), d));
        }
        acc += len2.sqrt();
    }
    best
}

/// A closed loop started `s` along it from its first point (the point put in there).
fn restart_at(pts: &[P], s: f64) -> Vec<P> {
    let n = pts.len();
    let mut acc = 0.0;
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if acc + len >= s && len > 0.0 {
            let t = ((s - acc) / len).clamp(0.0, 1.0);
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let near = |q: P| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-9;
            let first = if near(a) { k } else { (k + 1) % n };
            let mut out: Vec<P> = Vec::with_capacity(n + 1);
            if !near(a) && !near(b) {
                out.push(p);
            }
            out.extend((0..n).map(|j| pts[(first + j) % n]));
            return out;
        }
        acc += len;
    }
    pts.to_vec()
}

/// The route's points from its start up to `s` along it, ending at that place.
fn route_to(route: &[P], s: f64) -> Vec<(P, f64)> {
    let mut out = vec![(route[0], 0.0)];
    let mut acc = 0.0;
    for w in route.windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if acc + len >= s {
            out.push((along(route, s), s));
            return out;
        }
        acc += len;
        out.push((w[1], acc));
    }
    out
}

/// A ramp that goes the whole way round each time (`0x14085e3f0`): a spiral, or no distance set.
/// On a closed loop it isn't followed by a lap at depth except at the last depth (`0x1408575e0`).
fn ramps_whole_way(op: &Op) -> bool {
    op.ramp_kind == RampKind::Spiral || (!op.ramp_by_angle && op.ramp_distance <= 0.0)
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
#[cfg(test)]
fn lead_points(p: P, t: P, side_sign: f64, op: &Op, out: bool) -> Vec<P> {
    lead_points_for(p, t, side_sign, op, out, op.lead_distance)
}

/// The same as `lead_points`, `distance` long instead of the toolpath's lead distance.
fn lead_points_for(p: P, t: P, side_sign: f64, op: &Op, out: bool, distance: f64) -> Vec<P> {
    // The right of travel, turned towards the side away from the part.
    let n = [t[1] * side_sign, -t[0] * side_sign];
    let along = if out { 1.0 } else { -1.0 };
    let mut points: Vec<P> = if op.lead_circular && op.lead_radius > 0.0 {
        let r = op.lead_radius;
        // Never more than a quarter of the way round.
        let sweep = (distance / r).min(std::f64::consts::FRAC_PI_2);
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
        let (forward, sideways) = (distance * a.cos() * along, distance * a.sin());
        vec![[p[0] + t[0] * forward + n[0] * sideways, p[1] + t[1] * forward + n[1] * sideways]]
    };
    if !out {
        points.reverse();
    }
    points
}

/// How long a lead into a closed loop can be (`0x1408437a0`, `0x140843ee0`): the whole lead is
/// tried at ten points from the loop outwards, at 0, 1/9, 2/9 ... 1 of its length
/// (`0x1421e2000`, up to `0x1421e2008` = 1.0001). From the second point on (the first lies on the
/// loop), the first point on the other side of the loop, the k-th, cuts the lead to (k - 1) tenths
/// of its length (`/ 10.0`, `0x142736370`), when that is under 1.01 (`0x142103338`); nothing left
/// means no lead. A lead whose middle point (the sixth) isn't on the waste side is left out. `None`
/// means no lead: the cut then starts on the loop. The toolpath's other loops (`others`) count
/// too, from the first point on: one across any of them at the k-th point also cuts the lead to
/// (k - 1) tenths, the smallest fraction winning (`0x140842ba0` passes the whole group).
fn fitted_lead(loop_pts: &[P], others: &[&[P]], side: Side, side_sign: f64, op: &Op) -> Option<f64> {
    if loop_pts.len() < 3 || side == Side::On {
        return Some(op.lead_distance);
    }
    let start = loop_pts[0];
    let next = loop_pts.iter().copied().find(|q| (q[0] - start[0]).hypot(q[1] - start[1]) > 1e-9)?;
    let len = (next[0] - start[0]).hypot(next[1] - start[1]);
    let t = [(next[0] - start[0]) / len, (next[1] - start[1]) / len];
    let shape = [Path::closed(loop_pts.to_vec())];
    let waste_inside = side == Side::Inside;
    // The lead from the loop outwards.
    let mut chain = vec![start];
    let mut far = lead_points_for(start, t, side_sign, op, false, op.lead_distance);
    far.reverse();
    chain.extend(far);
    let total: f64 = chain.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    if total <= 1e-12 {
        return Some(op.lead_distance);
    }
    let at = |s: f64| -> P {
        let mut left = s;
        for w in chain.windows(2) {
            let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            if left <= l && l > 0.0 {
                let f = left / l;
                return [w[0][0] + (w[1][0] - w[0][0]) * f, w[0][1] + (w[1][1] - w[0][1]) * f];
            }
            left -= l;
        }
        chain[chain.len() - 1]
    };
    let sides: Vec<bool> = (0..10).map(|k| crate::geom::inside_even_odd(&shape, at(total * (k as f64 / 9.0).min(1.0)))).collect();
    if sides[5] != waste_inside {
        return None;
    }
    let mut fraction = (2..10).find(|&k| sides[k] != sides[1]).map_or(999.0, |k| (k - 1) as f64 / 10.0);
    for other in others.iter().filter(|o| o.len() >= 3) {
        let shape = [Path::closed(other.to_vec())];
        let inside: Vec<bool> = (0..10).map(|k| crate::geom::inside_even_odd(&shape, at(total * (k as f64 / 9.0).min(1.0)))).collect();
        if let Some(k) = (1..10).find(|&k| inside[k] != inside[0]) {
            fraction = fraction.min((k - 1) as f64 / 10.0);
        }
    }
    if fraction < 1.01 {
        if fraction <= 0.0 {
            return None;
        }
        return Some(op.lead_distance * fraction);
    }
    Some(op.lead_distance)
}

/// Starts a closed loop at the one of its points nearest to where the tool is.
fn start_near(pts: &mut Vec<P>, at: P) {
    let far = |p: P| (p[0] - at[0]).hypot(p[1] - at[1]);
    // A round loop's nodes are its quarter points: the original keeps a circle, and its offset,
    // as four arcs, and picks among the spans' starts (`0x14079b600`, the first nearest).
    if let Some((c, r)) = round_loop(pts) {
        let quarters = [[c[0] + r, c[1]], [c[0], c[1] + r], [c[0] - r, c[1]], [c[0], c[1] - r]];
        if let Some(q) = quarters.into_iter().min_by(|a, b| far(*a).total_cmp(&far(*b))) {
            start_loop_near_point(pts, q);
        }
        return;
    }
    let best = (0..pts.len()).min_by(|a, b| far(pts[*a]).total_cmp(&far(pts[*b])));
    if let Some(best) = best {
        pts.rotate_left(best);
    }
}

/// A pocket's rings at `level`, as the original's offsets make them (`0x1407af400`): its outside
/// geometry engine (`0x140dcb3a0`) works to half the toolpath's tolerance (`× 0.5`, `0x1420e8d28`),
/// after each contour is tidied of pieces shorter than 1% of that (`0x14079cf50`, `0x1420fbbb0`:
/// a piece's ends merged into its neighbours, over and over while any is removed and two or more
/// are left; a contour that is one whole circle is left as it is). Where the shape is exactly twice
/// the level wide, the offset has no width and gives nothing. The distance grid's contour runs out
/// along such a ridge and back the same way; those runs are taken out, and a loop thinner than
/// the engine's tolerance (mean half-width, area over perimeter) is dropped. Seen in the person's
/// islands job: two bars 30 mm high joined by a 7 mm neck, the ring at 15 mm in is only the bump
/// at the neck (x 149 to 156), not the whole middle line of each bar.
fn area_contours(field: &Field, level: f64, tolerance: f64, exact: Option<&[Vec<P>]>) -> Vec<Vec<P>> {
    // Shapes all closed: offset them exactly, as the original's engine does, so the corners
    // where two offsets meet are sharp where the grid would round them off (the person's islands
    // job: a ring meeting the offset of a round island at 7.6 degrees, 0.65 mm along the line short
    // on the grid). The arcs are drawn within the engine's tolerance; points are put a quarter of
    // a tool apart along straight stretches, so a ring can be started near the tool anywhere.
    if let Some(loops) = exact {
        let engine = tolerance * OFFSET_TOLERANCE_SHARE;
        let arc_step = (8.0 * engine / level.max(field.cell)).sqrt().clamp(0.02, 0.5);
        return crate::solid::shrunk(loops, level, arc_step).into_iter().map(|r| densify_loop(&r, field.cell * EXACT_RING_SPACING_CELLS)).collect();
    }
    let engine = tolerance * OFFSET_TOLERANCE_SHARE;
    let tidy = engine * TIDY_SHARE;
    let tol = field.cell * BACK_TRACK_SHARE;
    let thinnest = tol.max(engine);
    field
        .contours(level)
        .into_iter()
        .map(|l| without_short_pieces(without_back_tracks(l, tol), tidy))
        .filter(|l| {
            let n = l.len();
            let perimeter: f64 = (0..n).map(|k| (l[(k + 1) % n][0] - l[k][0]).hypot(l[(k + 1) % n][1] - l[k][1])).sum();
            n >= 3 && signed_area(l).abs() > thinnest * perimeter
        })
        .collect()
}

/// The spacing of points along an exactly offset ring's straight stretches, in grid cells: 16, a
/// quarter of the tool (the grid is 1/64 of it). Closer gives the same cuts with more points;
/// none leaves a ring's start only at its corners, and rings stop linking.
const EXACT_RING_SPACING_CELLS: f64 = 16.0;

/// A closed loop without the points that lie on the straight line through their neighbours
/// (within a millionth of a millimetre), its first point kept.
fn without_straight_points(pts: Vec<P>) -> Vec<P> {
    let n = pts.len();
    if n < 4 {
        return pts;
    }
    let on_line = |a: P, p: P, b: P| {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        len > 0.0 && ((p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])).abs() / len < 1e-6 && (p[0] - a[0]) * (b[0] - p[0]) + (p[1] - a[1]) * (b[1] - p[1]) >= 0.0
    };
    let mut out = vec![pts[0]];
    for k in 1..n {
        if !on_line(*out.last().unwrap(), pts[k], pts[(k + 1) % n]) {
            out.push(pts[k]);
        }
    }
    out
}

/// A closed loop with points put in along its pieces, no further apart than `step`.
fn densify_loop(l: &[P], step: f64) -> Vec<P> {
    let n = l.len();
    let mut out = Vec::new();
    for k in 0..n {
        let (a, b) = (l[k], l[(k + 1) % n]);
        let m = (((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil() as usize).max(1);
        for i in 0..m {
            let t = i as f64 / m as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// A ring with an ear at each of its sharp corners: out along the middle line's branch into the
/// corner, away from the ring's inside, until the distance from the walls has dropped by `drop`,
/// and back. The branch is followed through the sharp corners of `between`, the exact offsets at
/// levels going down to the end of the ear, each the nearest to the point before; with none near,
/// the ear goes straight along the corner's bisector. The ring has the region on its left (outer
/// loops anticlockwise, holes clockwise), so a corner the region turns round is a left turn.
/// Corners turning less than `EAR_CORNER_DEGREES` are an offset arc's pieces and get none.
fn with_corner_ears(ring: Vec<P>, drop: f64, between: &[Vec<Vec<P>>], pieces: &mut Vec<(Vec<P>, Vec<P>)>, reach_piece: f64, cell: f64) -> Vec<P> {
    let n = ring.len();
    if drop <= 0.0 || n < 3 {
        return ring;
    }
    let sharp = |a: P, p: P, b: P| -> Option<([f64; 2], [f64; 2], f64)> {
        let (la, lb) = ((p[0] - a[0]).hypot(p[1] - a[1]), (b[0] - p[0]).hypot(b[1] - p[1]));
        if la < 1e-9 || lb < 1e-9 {
            return None;
        }
        let (u, v) = ([(p[0] - a[0]) / la, (p[1] - a[1]) / la], [(b[0] - p[0]) / lb, (b[1] - p[1]) / lb]);
        let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]);
        (turn >= EAR_CORNER_DEGREES.to_radians()).then_some((u, v, turn))
    };
    let mut out = Vec::with_capacity(n + 16);
    for k in 0..n {
        let (a, p, b) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
        out.push(p);
        let Some((u, v, turn)) = sharp(a, p, b) else { continue };

        let half = (std::f64::consts::PI - turn) / 2.0;
        let (bx, by) = (u[0] - v[0], u[1] - v[1]);
        let bl = bx.hypot(by);
        if bl < 1e-9 || half.sin() < 1e-6 {
            continue;
        }
        let reach = drop / half.sin();
        let straight = [p[0] + bx / bl * reach, p[1] + by / bl * reach];
        // Only at a real corner: a ring's way still turning by that much a tool's diameter before
        // and after. A step in a ragged side (a thin ring along a font's outline in the person's
        // logo in a saved job) goes on straight either side of it.
        let along = |from: usize, back: bool, length: f64| {
            let (mut gone, mut k) = (0.0, from);
            loop {
                let next = if back { (k + n - 1) % n } else { (k + 1) % n };
                gone += (ring[next][0] - ring[k][0]).hypot(ring[next][1] - ring[k][1]);
                k = next;
                if gone >= length || k == from {
                    return ring[k];
                }
            }
        };
        let span = 2.0 * (reach_piece - cell);
        let (before, after) = (along(k, true, span), along(k, false, span));
        let (wu, wv) = ([p[0] - before[0], p[1] - before[1]], [after[0] - p[0], after[1] - p[1]]);
        let wide = (wu[0] * wv[1] - wu[1] * wv[0]).atan2(wu[0] * wv[0] + wu[1] * wv[1]);
        if wide < EAR_CORNER_DEGREES.to_radians() {
            continue;
        }
        // Along the branch: each level's sharp corner nearest the point before, no further than
        // the straight way's step would take it, twice over.
        let mut way: Vec<P> = Vec::new();
        let mut at = p;
        let each = reach / between.len().max(1) as f64;
        for level in between {
            let found = level
                .iter()
                .flat_map(|l| {
                    let m = l.len();
                    (0..m).filter(move |&j| sharp(l[(j + m - 1) % m], l[j], l[(j + 1) % m]).is_some()).map(move |j| l[j])
                })
                .map(|q| ((q[0] - at[0]).hypot(q[1] - at[1]), q))
                .filter(|(d, _)| *d <= 2.0 * each)
                .min_by(|x, y| x.0.total_cmp(&y.0));
            match found {
                Some((_, q)) => {
                    way.push(q);
                    at = q;
                }
                None => {
                    way.clear();
                    break;
                }
            }
        }
        if way.len() != between.len() || between.is_empty() {
            // The branch bends too much to follow level by level: the leftover piece at this
            // corner, if there is one, is the ear, all along its middle (a corner meeting an arc
            // in the person's saved jobs: the saved ear runs 5 mm round the curve).
            let found = pieces
                .iter()
                .enumerate()
                .filter_map(|(k, (_, m))| m.iter().enumerate().map(|(j, q)| ((q[0] - p[0]).hypot(q[1] - p[1]), k, j)).min_by(|a, b| a.0.total_cmp(&b.0)))
                .filter(|(d, _, _)| *d <= reach_piece)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, k, _)) = found {
                let (_, middle) = pieces.swap_remove(k);
                // Not the piece's corners on the edge of what this ring clears: those are spurs
                // off its middle, within the ring's reach already.
                let reached = |q: &P| (0..n).any(|t| crate::geom::dist2_seg(*q, ring[t], ring[(t + 1) % n]) <= reach_piece * reach_piece);
                let middle: Vec<P> = middle.into_iter().filter(|q| !reached(q)).collect();
                let m = middle.len();
                if let Some(j) = (0..m).min_by(|a, b| (middle[*a][0] - p[0]).hypot(middle[*a][1] - p[1]).total_cmp(&(middle[*b][0] - p[0]).hypot(middle[*b][1] - p[1]))) {
                    out.extend((0..=m).map(|t| middle[(j + t) % m]));
                    out.push(p);
                    continue;
                }
            }
            way = vec![straight];
        }
        out.extend(way.iter().copied());
        out.extend(way.iter().rev().skip(1).copied());
        out.push(p);
    }
    out
}

/// How many levels the branch into a corner is followed through for an ear.
const EAR_LEVELS: usize = 4;

/// The least a ring turns at a corner for it to get an ear (degrees): offset arcs turn by under
/// 30 degrees a piece.
const EAR_CORNER_DEGREES: f64 = 35.0;

/// What lies past the reach of a ring at `level` (its level and the tool's `radius` in) and out
/// of reach of the next, at `next_level` (its region grown by the radius): each piece's outline
/// and its middle line, out and back. Slivers thinner than a grid cell are left out.
fn leftover_pieces(field: &Field, loops: &[Vec<P>], level: f64, next_level: f64, radius: f64, tolerance: f64) -> Vec<(Vec<P>, Vec<P>)> {
    let engine = tolerance * OFFSET_TOLERANCE_SHARE;
    let arc = |l: f64| (8.0 * engine / l.max(field.cell)).sqrt().clamp(0.02, 0.5);
    let beyond = crate::solid::shrunk(loops, level + radius, arc(level + radius));
    let inner = crate::solid::shrunk(loops, next_level, arc(next_level));
    if beyond.is_empty() {
        return Vec::new();
    }
    let reached = if inner.is_empty() { Vec::new() } else { crate::solid::shrunk(&inner, -radius, arc(radius)) };
    let pieces = crate::solid::overlay(&beyond, &reached, i_overlay::core::overlay_rule::OverlayRule::Difference);
    let mut out = Vec::new();
    for piece in pieces {
        let Some(outline) = piece.first() else { continue };
        let n = outline.len();
        let perimeter: f64 = (0..n).map(|k| (outline[(k + 1) % n][0] - outline[k][0]).hypot(outline[(k + 1) % n][1] - outline[k][1])).sum();
        if n < 3 || signed_area(outline).abs() <= field.cell * perimeter {
            continue;
        }
        // Climbing the field can leave a small piece on its high side: what is outside the piece
        // (by more than a grid cell) is not its middle.
        let around = [Path::closed(outline.clone())];
        let inside = |q: &P| inside_even_odd(around.iter(), *q) || (0..n).any(|k| crate::geom::dist2_seg(*q, outline[k], outline[(k + 1) % n]) <= field.cell * field.cell);
        let middle: Vec<P> = middle_line_of(field, outline, true).into_iter().filter(inside).collect();
        if middle.len() < 2 {
            continue;
        }
        out.push((outline.clone(), without_repeats(crate::geom::simplify_loop(&middle, field.cell * STROKE_THIN_CELLS), field.cell)));
    }
    out
}

/// Of the leftover pieces, the middles of those the next ring's path, with its ears, comes within
/// the tool's radius of somewhere but not all round.
fn leftover_strokes(field: &Field, pieces: &[(Vec<P>, Vec<P>)], radius: f64, next: &[Vec<P>], whole: &[Vec<P>]) -> Vec<Vec<P>> {
    let segments: Vec<(P, P)> = next.iter().flat_map(|r| (0..r.len()).map(move |k| (r[k], r[(k + 1) % r.len()]))).collect();
    let near_next = |q: P| segments.iter().any(|(a, b)| crate::geom::dist2_seg(q, *a, *b) <= (radius * 1.02 + field.cell).powi(2));
    // A whole loop the stroke along the middle before already cuts is the same piece again: the
    // same size, round the same place.
    let bounds = |l: &[P]| l.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), q| ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]));
    let same = |a: &[P], b: &[P]| {
        let ((la, ha), (lb, hb)) = (bounds(a), bounds(b));
        (0..2).all(|k| (la[k] - lb[k]).abs() <= field.cell * 2.0 && (ha[k] - hb[k]).abs() <= field.cell * 2.0)
    };
    pieces
        .iter()
        .filter(|(outline, _)| !outline.iter().all(|q| near_next(*q)) && !whole.iter().any(|w| same(w, outline)))
        .map(|(_, middle)| middle.clone())
        .collect()
}

/// A stroke with the trips it makes twice taken out: out to a point and back, then out to the
/// same point and back again (the loop's two sides of a corner both land on the branch into it),
/// is one trip. Points within `same` count as one place.
fn without_repeats(pts: Vec<P>, same: f64) -> Vec<P> {
    let near = |a: P, b: P| (a[0] - b[0]).hypot(a[1] - b[1]) <= same;
    let mut out: Vec<P> = Vec::with_capacity(pts.len());
    for p in pts {
        // Here again where the tool was two points ago, having just gone out and back: the trip
        // out and back is done again from here, so the last two points go.
        let n = out.len();
        if n >= 3 && near(p, out[n - 2]) && near(out[n - 1], out[n - 3]) {
            out.truncate(n - 1);
            continue;
        }
        if out.last().is_none_or(|q| !near(*q, p)) {
            out.push(p);
        }
    }
    out
}

/// How far a pocket's stroke along the middle may wander from a straight line and still be
/// drawn as one, in grid cells (the grid puts it a few hundredths of a millimetre either way).
const STROKE_THIN_CELLS: f64 = 0.3;

/// The offset engine's tolerance as a share of the toolpath's (`0x1407af400`, `0x1420e8d28`).
const OFFSET_TOLERANCE_SHARE: f64 = 0.5;
/// Pieces shorter than this share of the engine's tolerance are tidied away before offsetting
/// (`0x1407af400` passes it to `0x14079cf50`; `0x1420fbbb0`).
const TIDY_SHARE: f64 = 0.01;

/// A closed loop with its pieces shorter than `shortest` taken out, as `0x14079cf50` tidies a
/// contour: each such piece's ends are merged, again and again while any goes and at least three
/// points are left.
fn without_short_pieces(mut l: Vec<P>, shortest: f64) -> Vec<P> {
    loop {
        let n = l.len();
        if n <= 3 {
            return l;
        }
        let Some(k) = (0..n).find(|&k| (l[(k + 1) % n][0] - l[k][0]).hypot(l[(k + 1) % n][1] - l[k][1]) < shortest) else { return l };
        l.remove((k + 1) % n);
    }
}

/// How far off a line a contour may wander and still be going back along it, as a share of the
/// grid's cell.
const BACK_TRACK_SHARE: f64 = 0.05;

/// A closed loop with the stretches where it goes out and comes straight back taken out: a point
/// where the way turns right round (within `tol`) goes, again and again, all the way round.
fn without_back_tracks(l: Vec<P>, tol: f64) -> Vec<P> {
    let back = |a: P, p: P, b: P| {
        let (u, v) = ([p[0] - a[0], p[1] - a[1]], [b[0] - p[0], b[1] - p[1]]);
        let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
        if lu < 1e-12 || lv < 1e-12 {
            return true;
        }
        (u[0] * v[1] - u[1] * v[0]).abs() <= tol * lu.max(lv) && u[0] * v[0] + u[1] * v[1] < 0.0
    };
    let pass = |pts: Vec<P>| -> Vec<P> {
        let mut out: Vec<P> = Vec::with_capacity(pts.len());
        for p in pts {
            while out.len() >= 2 && back(out[out.len() - 2], out[out.len() - 1], p) {
                out.pop();
            }
            if out.last().is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-12) {
                out.push(p);
            }
        }
        out
    };
    // Twice, the second time from halfway round, so a run across the loop's start goes too.
    let mut once = pass(l);
    let half = once.len() / 2;
    once.rotate_left(half);
    let mut out = pass(once);
    // The joint where it closes.
    while out.len() >= 3 && back(out[out.len() - 1], out[0], out[1]) {
        out.remove(0);
    }
    while out.len() >= 3 && back(out[out.len() - 2], out[out.len() - 1], out[0]) {
        out.pop();
    }
    out
}

/// The circle a closed loop runs round, if it is one: a dozen points or more, all on it within a
/// thousandth of its radius.
fn round_loop(pts: &[P]) -> Option<(P, f64)> {
    let n = pts.len();
    if n < 12 {
        return None;
    }
    let (a, b, c) = (pts[0], pts[n / 3], pts[2 * n / 3]);
    let (bx, by, cx, cy) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    if d.abs() < 1e-12 {
        return None;
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let centre = [a[0] + (cy * b2 - by * c2) / d, a[1] + (bx * c2 - cx * b2) / d];
    let r = (a[0] - centre[0]).hypot(a[1] - centre[1]);
    let tol = (r * 1e-3).max(1e-6);
    pts.iter().all(|p| ((p[0] - centre[0]).hypot(p[1] - centre[1]) - r).abs() <= tol).then_some((centre, r))
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
    // Each closed shape's start, with the points before and after it.
    let starts: Vec<(P, P, P)> = paths
        .iter()
        .filter(|p| p.closed && p.pts.len() >= 3)
        .map(|p| (p.pts[0], p.pts[p.pts.len() - 1], p.pts[1]))
        .collect();
    for (pts, closed) in items.iter_mut() {
        if !*closed || pts.len() < 2 {
            continue;
        }
        let Some((lo, hi)) = bbox([&Path::closed(pts.clone())]) else { continue };
        let line = Path::closed(pts.clone());
        let near = starts.iter().filter(|(s, _, _)| s[0] >= lo[0] - reach && s[0] <= hi[0] + reach && s[1] >= lo[1] - reach && s[1] <= hi[1] + reach);
        if let Some((start, before, after)) = near.min_by(|a, b| distance_to_path(a.0, &line).total_cmp(&distance_to_path(b.0, &line))) {
            start_loop_near_point(pts, *start);
            start_at_corner_arc(pts, *start, *before, *after);
        }
    }
}

/// Where a shape starts at a corner, its offset loop goes round that corner on an arc, and
/// every point of the arc is as near the start. The original starts the loop where the arc
/// begins, in the direction of travel, as the saved jobs show on a keyhole whose start is the
/// corner where the round part meets the neck.
fn start_at_corner_arc(pts: &mut Vec<P>, start: P, before: P, after: P) {
    let unit = |a: P, b: P| {
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        (l > 1e-9).then(|| [(b[0] - a[0]) / l, (b[1] - a[1]) / l])
    };
    let (Some(din), Some(dout)) = (unit(before, start), unit(start, after)) else { return };
    let turn = (din[0] * dout[1] - din[1] * dout[0]).atan2(din[0] * dout[0] + din[1] * dout[1]);
    if turn.abs() < 5f64.to_radians() || pts.len() < 3 {
        return;
    }
    let d = (pts[0][0] - start[0]).hypot(pts[0][1] - start[1]);
    if d < 1e-6 {
        return;
    }
    let line = Path::closed(pts.clone());
    // The ends of the arc: square off each side of the corner, on the side the loop is.
    let end_of = |dir: P| {
        let a = [start[0] - dir[1] * d, start[1] + dir[0] * d];
        let b = [start[0] + dir[1] * d, start[1] - dir[0] * d];
        if distance_to_path(a, &line) <= distance_to_path(b, &line) { a } else { b }
    };
    let (e1, e2) = (end_of(din), end_of(dout));
    let arc = d * turn.abs();
    if (e1[0] - e2[0]).hypot(e1[1] - e2[1]) < 1e-6 || distance_to_path(e1, &line) > d * 0.05 + 1e-3 || distance_to_path(e2, &line) > d * 0.05 + 1e-3 {
        return;
    }
    // Along the loop from its start to the point nearest `q`.
    let along_to = |pts: &[P], q: P| {
        let n = pts.len();
        let (mut run, mut best) = (0.0, (f64::MAX, 0.0));
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.hypot(dy);
            let t = (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / (len * len).max(1e-18)).clamp(0.0, 1.0);
            let off = (a[0] + dx * t - q[0]).hypot(a[1] + dy * t - q[1]);
            if off < best.0 {
                best = (off, run + len * t);
            }
            run += len;
        }
        best.1
    };
    for (first, second) in [(e1, e2), (e2, e1)] {
        let mut trial = pts.clone();
        start_loop_near_point(&mut trial, first);
        if along_to(&trial, second) <= arc * 1.5 + d * 0.05 {
            *pts = trial;
            return;
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

/// The tolerance the original works to in a toolpath, Burin keeping none of its own: its default,
/// the same for 2D, 3D and V-Carve toolpaths, 0.0003937 inch, which is 0.01 mm (`0x1420fbb90`, set
/// by `0x140896880`, kept within 0.00004 and 0.02 inch when read back, `0x140896930`). The saved
/// test job has it (0.010 mm).
const DEFAULT_TOLERANCE_INCH: f64 = 0.00039370078740000005;

fn toolpath_tolerance(op: &Op) -> f64 {
    if op.tool.metric { DEFAULT_TOLERANCE_INCH * 25.4 } else { DEFAULT_TOLERANCE_INCH }
}

/// The size of the circle cut where a place is only just as wide as the tool, as a share of the
/// tool's radius (estimated from one saved job: 0.015 mm with a 3 mm tool).
const SNUG_CIRCLE_SHARE: f64 = 0.01;

/// The widest point of each part of the shape that is no wider than the tool but within
/// `tolerance` of it: there the first ring, a radius in, comes to nothing, yet the tool fits. The
/// grid's best point is refined on the shapes themselves.
fn snug_spots(field: &Field, paths: &[Path], radius: f64, tolerance: f64) -> Vec<P> {
    let (nx, ny, cell) = (field.nx, field.ny, field.cell);
    let exact = |q: P| -> f64 {
        if field.sample(q[0], q[1]) <= 0.0 {
            return -1.0;
        }
        paths.iter().filter(|p| p.closed).map(|p| distance_to_path(q, p)).fold(f64::MAX, f64::min)
    };
    let mut seen = vec![false; nx * ny];
    let mut out = Vec::new();
    for start in 0..nx * ny {
        if seen[start] || field.d[start] <= 0.0 {
            continue;
        }
        let (mut best, mut widest) = (start, field.d[start]);
        let mut todo = vec![start];
        seen[start] = true;
        while let Some(i) = todo.pop() {
            if field.d[i] > widest {
                (best, widest) = (i, field.d[i]);
            }
            let (ix, iy) = (i % nx, i / nx);
            let around = [(ix + 1 < nx).then(|| i + 1), (ix > 0).then(|| i - 1), (iy + 1 < ny).then(|| i + nx), (iy > 0).then(|| i - nx)];
            for j in around.into_iter().flatten() {
                if !seen[j] && field.d[j] > 0.0 {
                    seen[j] = true;
                    todo.push(j);
                }
            }
        }
        let widest = widest as f64;
        if widest >= radius || widest < radius - tolerance - 2.0 * cell {
            continue;
        }
        // Climb to the true widest point from the grid's.
        let mut p = [field.x0 + (best % nx) as f64 * cell, field.y0 + (best / nx) as f64 * cell];
        let mut d = exact(p);
        let mut stride = cell;
        while stride > cell * 1e-4 {
            let moved = (0..8).map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 8.0;
                let q = [p[0] + stride * a.cos(), p[1] + stride * a.sin()];
                (q, exact(q))
            }).filter(|(_, dq)| *dq > d).max_by(|a, b| a.1.total_cmp(&b.1));
            match moved {
                Some((q, dq)) => (p, d) = (q, dq),
                None => stride *= 0.5,
            }
        }
        if d < radius && d > radius - tolerance {
            out.push(p);
        }
    }
    out
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
/// How far along its path the tool stays up to leave a tab of the length asked for: its width takes
/// half of itself out of the material at each end.
fn tab_reach(op: &Op) -> f64 {
    op.tab_length + op.tool.dia
}

fn walk(e: &mut Emit, pts: &[P], closed: bool, z: f64, op: &Op, ramp_from: Option<f64>, tab_z: Option<f64>) {
    walk_from(e, pts, closed, z, op, ramp_from, tab_z, 0.0, true);
}

/// Cuts along a loop as `walk` does, its tabs where they stand on the loop started `shift` further
/// back, and with `recut` false leaving out the cut at depth along a ramp's stretch.
#[allow(clippy::too_many_arguments)]
fn walk_from(e: &mut Emit, pts: &[P], closed: bool, z: f64, op: &Op, ramp_from: Option<f64>, tab_z: Option<f64>, shift: f64, recut: bool) {
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
        Some(_) if tabs_wanted(op) => tab_spans(pts, total, op, shift),
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
        breaks.extend([*a, *b].into_iter().filter(|s| *s > 0.0 && *s < total));
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
    if recut && closed && ramp_len > 0.0 {
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
    let first_leg = |list: &[usize]| list.first().map_or(0.0, |i| (items[*i].start[0] - from[0]).hypot(items[*i].start[1] - from[1]));
    let mut best = tries.remove(0);
    for t in tries {
        let (a, b) = (cost(&t), cost(&best));
        // The original keeps the first on a tie (`0x140894a60`), but its sums come from points kept as
        // 32-bit floats, so ways that go round the same loop either way tie only within rounding, and
        // which one wins is then down to rounding. Saved jobs show it starting nearest the tool.
        let tied = (a - b).abs() <= TIE_SHARE * b.max(1.0);
        if (!tied && a < b) || (tied && first_leg(&t) < first_leg(&best) - TIE_SHARE * b.max(1.0)) {
            best = t;
        }
    }
    best
}

/// Travels this close, as a share, count as the same.
const TIE_SHARE: f64 = 1e-7;

/// The smallest closed shape around each one is what holds it.
fn holders(all: &[(Vec<P>, bool)], items: &[Placed]) -> Vec<Option<usize>> {
    let area = |i: usize| (items[i].hi[0] - items[i].lo[0]) * (items[i].hi[1] - items[i].lo[1]);
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
    parent
}

/// The order to cut shapes in. Each is sequenced among its neighbours, with what lies inside it cut
/// just before it: a part cut free before its holes would shift under the tool.
fn sequence_order(all: &[(Vec<P>, bool)], choices: OrderChoices, from: [f64; 3]) -> Vec<usize> {
    let items: Vec<Placed> = all.iter().map(placed).collect();
    let parent = holders(all, &items);
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

/// What a profile's cut is doing, when a separate last pass splits it in two.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Whole,
    /// Stays at depth at the end if the finish follows without a lift.
    Rough { stay_down: bool },
    /// Linked: comes straight across from the rough loop at depth.
    Finish { linked: bool },
}

fn profile_like(e: &mut Emit, op: &Op, side: Side) {
    let radius = op.tool.dia / 2.0 + op.allowance;
    let zs = depths(op);
    // No allowance, or just one pass, means no separate last pass at all.
    let separate_last = op.last_pass && op.last_allowance > 0.0 && side != Side::On && zs.len() >= 2;
    let extra = if separate_last { op.last_allowance } else { 0.0 };
    let margin = radius + extra + op.tool.dia;
    // Each set of shapes on its own grid; a set inside another shape counts its inside the other way round.
    let fields: Vec<(Field, bool, Vec<Vec<P>>)> = if side == Side::On {
        Vec::new()
    } else {
        regions(&op.paths, 2.0 * margin, false)
            .into_iter()
            .filter_map(|set| {
                let shapes: Vec<Path> = set.iter().map(|i| op.paths[*i].clone()).collect();
                let others = op.paths.iter().enumerate().filter(|(i, p)| p.closed && !set.contains(i)).map(|(_, p)| p);
                let flipped = inside_even_odd(others, shapes[0].pts[0]);
                let loops: Vec<Vec<P>> = shapes.iter().map(|p| p.pts.clone()).collect();
                Some((Field::build_to(&shapes, margin, 900, op.tool.dia / 64.0)?, flipped, loops))
            })
            .collect()
    };
    // The offsets worked out exactly, as the original's engine does (`0x1407af400`): straight
    // stretches stay single lines, so a loop's start falls on the end of one, as the original
    // starts on a span's start (`0x14079b600`). Grown or shrunk by `inset` (positive inward).
    let exact_offset = |loops: &[Vec<P>], inset: f64| -> Vec<Vec<P>> {
        let engine = toolpath_tolerance(op) * OFFSET_TOLERANCE_SHARE;
        crate::solid::shrunk(loops, inset, (8.0 * engine / inset.abs().max(1e-3)).sqrt().clamp(0.02, 0.5))
    };
    // Square corners are worked out shape by shape, which only suits shapes not inside each other.
    let closed: Vec<&Path> = op.paths.iter().filter(|p| p.closed).collect();
    let separate = closed.iter().enumerate().all(|(i, p)| closed.iter().enumerate().all(|(j, q)| i == j || !inside_even_odd(std::slice::from_ref(*q), p.pts[0])));
    let sharp = op.sharp_corners && side == Side::Outside && separate;
    let keep_click_order = op.order == Order::AsDrawn || op.order_choices.is_some_and(|c| c.selection);
    let source = |l: &[P]| (0..op.paths.len()).min_by(|a, b| distance_to_path(l[0], &op.paths[*a]).total_cmp(&distance_to_path(l[0], &op.paths[*b]))).unwrap_or(0);
    let loops_at = |off: f64| -> Vec<(Vec<P>, bool)> {
        let mut out = Vec::new();
        // A square corner in a notch narrower than the tool, or by a close neighbour, would cut into the part.
        let square: Option<Vec<Vec<P>>> = sharp.then(|| closed.iter().map(|p| sharp_offset(p, off)).collect::<Vec<_>>()).filter(|loops| loops.iter().all(|l| stays_clear(l, &op.paths, off)));
        match side {
            Side::Outside if square.is_some() => {
                out.extend(square.unwrap_or_default().into_iter().map(|l| (oriented(l, !op.climb), true)));
            }
            Side::Outside => {
                for (_, flipped, loops) in &fields {
                    let level = if *flipped { off } else { -off };
                    let found = exact_offset(loops, level);
                    out.extend(found.into_iter().map(|l| (oriented(l, !op.climb), true)));
                }
            }
            Side::Inside => {
                for (_, flipped, loops) in &fields {
                    let level = if *flipped { -off } else { off };
                    let found = exact_offset(loops, level);
                    out.extend(found.into_iter().map(|l| (oriented(l, op.climb), true)));
                }
            }
            _ => {}
        }
        // Worked-out outlines come back in no special order, so put them in the order of their shapes.
        if keep_click_order {
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
    // With leads, the start isn't moved to suit the tool: only a chosen point moves it
    // (`0x140855cf0`). Ramps don't stop it: there the starts are moved in a first run that only
    // counts the moves, on the loops themselves, and kept (`0x140857080`).
    let start = if start == StartAt::Optimize && op.lead { StartAt::Keep } else { start };
    // The order is settled with each shape starting where it was drawn to, and only then is
    // the start moved.
    let settle = |mut items: Vec<(Vec<P>, bool)>, from: [f64; 3]| {
        // Twice the offset, as a worked-out loop on a curve can sit a little further off than that.
        start_from_shapes(&mut items, &op.paths, 2.0 * (radius.abs() + extra.abs()) + 1e-3);
        match op.order_choices {
            Some(choices) => best_order(items, choices, from),
            None => sort_paths(items, op.order, from),
        }
    };
    let run = |e: &mut Emit, items: Vec<(Vec<P>, bool)>, pass_zs: &[f64], stage: Stage| {
        let from = e.last();
        let finish = matches!(stage, Stage::Finish { .. });
        // A finish loop always starts at its point nearest the tool, whatever start was chosen.
        let mut ordered = if finish { items } else { settle(items, from) };
        match if finish { StartAt::Optimize } else { start } {
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
        // Every closed loop of the pass, which a lead mustn't cross either (`0x140842ba0` hands
        // the whole group to each loop's lead fitting).
        let group: Vec<Vec<P>> = ordered.iter().filter(|(_, c)| *c).map(|(p, _)| p.clone()).collect();
        for (mut pts, closed) in ordered {
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
            // A lead that would reach into the part is shortened, or left out.
            let lead_length = if op.lead && op.lead_distance > 0.0 && pts.len() >= 2 {
                if closed {
                    let others: Vec<&[P]> = group.iter().filter(|g| **g != pts).map(|g| g.as_slice()).collect();
                    fitted_lead(&pts, &others, side, side_sign, op)
                } else {
                    Some(op.lead_distance)
                }
            } else {
                None
            };
            let has_lead = lead_length.is_some();
            let lead_length = lead_length.unwrap_or(0.0);
            // The way the loop sets off, over a tenth of the tool's diameter: a worked-out loop's
            // first step can be tiny and turned a little off its true direction.
            // On a closed loop that runs smoothly through its start, the way across the start from
            // a little before it to a little after, which is true on a small circle too.
            let heading = (op.tool.dia * 0.1).min(total * 0.1);
            let set_off = if heading > 1e-9 {
                let ahead = unit_dir(route[0], along(&route, heading));
                if closed {
                    let behind = unit_dir(along(&route, total - heading), route[0]);
                    let across = unit_dir(along(&route, total - heading), along(&route, heading));
                    // Not smooth through the start: the way its first piece sets off. On a circle
                    // drawn with few points (the 60 plate's grouped holes turn 20° a point), that
                    // is the first step turned back by its share of the turn: the original's lead
                    // then goes straight to the hole's middle, as off the true arc.
                    let signed_turn = |k: usize| -> Option<f64> {
                        let (a, b, c) = (pts.get(k.checked_sub(1)?)?, pts.get(k)?, pts.get(k + 1)?);
                        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
                        (u[0].hypot(u[1]) > 1e-9 && v[0].hypot(v[1]) > 1e-9).then(|| (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]))
                    };
                    let on_arc = match (signed_turn(1), signed_turn(2)) {
                        (Some(t1), Some(t2)) if t1.abs() > 0.5f64.to_radians() && t1.abs() < 45f64.to_radians() && (t1 - t2).abs() < 0.2 * t1.abs() + 0.5f64.to_radians() => Some(t1),
                        _ => None,
                    }
                    // Only on a whole circle: every point turns the same way, by about the same.
                    .filter(|t1| {
                        let n = pts.len();
                        let turns: Vec<f64> = (0..n)
                            .filter_map(|k| {
                                let (a, b, c) = (pts[(k + n - 1) % n], pts[k], pts[(k + 1) % n]);
                                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
                                (u[0].hypot(u[1]) > 1e-6 && v[0].hypot(v[1]) > 1e-6).then(|| (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]))
                            })
                            .collect();
                        let whole: f64 = turns.iter().sum();
                        n >= 6 && (whole.abs() - std::f64::consts::TAU).abs() < 0.05 && turns.iter().all(|t| t.signum() == t1.signum() && t.abs() < 2.0 * t1.abs() + 1f64.to_radians())
                    });
                    if ahead[0] * behind[0] + ahead[1] * behind[1] > 20f64.to_radians().cos() {
                        across
                    } else if let Some(t) = on_arc {
                        // On an arc of radius R, chords of lengths l0 and l1 turn (l0 + l1) / 2R
                        // from one to the next, and the first lies l0 / 2R off the tangent.
                        let d = unit_dir(pts[0], pts[1]);
                        let (l0, l1) = ((pts[1][0] - pts[0][0]).hypot(pts[1][1] - pts[0][1]), (pts[2][0] - pts[1][0]).hypot(pts[2][1] - pts[1][1]));
                        let back = t * l0 / (l0 + l1).max(1e-12);
                        let (c, s) = ((-back).cos(), (-back).sin());
                        [d[0] * c - d[1] * s, d[0] * s + d[1] * c]
                    } else {
                        ahead
                    }
                } else {
                    ahead
                }
            } else {
                unit_dir(route[0], route[1])
            };
            let lead_in = if has_lead { lead_points_for(pts[0], set_off, side_sign, op, false, lead_length) } else { Vec::new() };
            let mut previous = -op.start_depth;
            // How far on the loop now starts from where its tabs were spaced from.
            let mut shift = 0.0;
            for (i, z) in pass_zs.iter().enumerate() {
                // Back the way it came, from where the last pass ended.
                if i > 0 && op.zigzag_open && !closed && !has_lead {
                    pts.reverse();
                    route.reverse();
                }
                let tab_z = (op.tabs && closed && *z < tab_top).then_some(tab_top);
                // Moves added at full depth near the start must stop short of the first tab.
                let before_tab = match tab_z {
                    Some(_) if tabs_wanted(op) => tab_spans(&pts, total, op, shift).into_iter().filter(|(_, b)| *b > 0.0).map(|(a, _)| a.max(0.0)).fold(f64::MAX, f64::min),
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
                // A loop that starts on a tab comes down only to the tab's top there.
                let on_tab = closed && !has_lead && tab_z.is_some() && tabs_wanted(op) && tab_spans(&pts, total, op, shift).iter().any(|(a, b)| *a < 0.0 && *b > 0.0);
                let plunge_z = if on_tab { plunge_z.max(tab_top) } else { plunge_z };
                if has_lead {
                    e.go_to(lead_in[0][0], lead_in[0][1], plunge_z);
                    // A zig-zag on the lead: once along 95% of it and back, then the lead at full depth
                    // (`0x14085e10d`); other ramps go down the whole lead.
                    let zig_on_lead = ramp_on_lead && op.ramp_kind == RampKind::ZigZag;
                    if zig_on_lead {
                        let mut way = lead_in.clone();
                        way.push(pts[0]);
                        let length: f64 = way.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
                        let turn = along(&way, length * ZIG_ON_LEAD);
                        e.feed(turn[0], turn[1], (previous + *z) / 2.0);
                        e.feed(lead_in[0][0], lead_in[0][1], *z);
                    }
                    let count = lead_in.len();
                    for (k, q) in lead_in.iter().enumerate().skip(1) {
                        let h = if zig_on_lead { *z } else if ramp_on_lead { previous + (*z - previous) * k as f64 / count as f64 } else { plunge_z };
                        e.feed(q[0], q[1], h);
                    }
                    e.feed(pts[0][0], pts[0][1], if ramp_on_lead { *z } else { plunge_z });
                } else if i == 0 && stage == (Stage::Finish { linked: true }) {
                    // Across from the rough loop at the height it finished at, then down.
                    e.join(pts[0][0], pts[0][1], e.last()[2]);
                    e.feed(pts[0][0], pts[0][1], plunge_z);
                } else if i == 0 || !closed {
                    // Stay down between passes on a closed loop.
                    e.go_to(pts[0][0], pts[0][1], plunge_z);
                } else {
                    e.feed(pts[0][0], pts[0][1], plunge_z);
                }
                if zigzag {
                    // Forward along the cut going part of the way down, and back along it for the
                    // next part (`0x140857ce0`: the leg's pieces, then the same pieces reversed).
                    let (leg, times) = zig_zag(op, depth);
                    let leg = leg.min(total).min(before_tab);
                    let way = route_to(&route, leg);
                    let drop = (previous - *z) / (2 * times) as f64;
                    let share = |s: f64| if leg > 0.0 { s / leg } else { 1.0 };
                    for k in 0..times {
                        for (q, s) in way.iter().skip(1) {
                            e.feed(q[0], q[1], previous - drop * (2 * k) as f64 - drop * share(*s));
                        }
                        for (q, s) in way.iter().rev().skip(1) {
                            e.feed(q[0], q[1], previous - drop * (2 * k + 1) as f64 - drop * (1.0 - share(*s)));
                        }
                    }
                }
                let last_pass = i + 1 == pass_zs.len();
                // A ramp the whole way round goes on down with no lap between depths.
                let recut = !(closed && ramp_from.is_some() && ramps_whole_way(op) && !last_pass);
                walk_from(e, &pts, closed, *z, op, ramp_from, tab_z, shift, recut);
                // The loop goes on from where its ramp ended (`0x1408566f0` starts it again at
                // the point nearest there, `0x14079bc50`): the next ramp follows on, with no move
                // across back to the start.
                if let (Some(from), true, false) = (ramp_from, closed, last_pass) {
                    let s = ramp_distance(op, from, *z).min(total);
                    if s > 0.0 && s < total {
                        pts = restart_at(&pts, s);
                        route = pts.clone();
                        route.push(pts[0]);
                        shift = (shift + s).rem_euclid(total);
                    }
                }
                previous = *z;
                if has_lead || !closed || i + 1 == pass_zs.len() {
                    // Running on past the start leaves no mark where the cut closes.
                    // The finish takes away any mark left where a rough loop closes.
                    if closed && op.overcut > 0.0 && (has_lead || !matches!(stage, Stage::Rough { .. })) {
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
                            let last = route[route.len() - 1];
                            (last, if closed { set_off } else { unit_dir(route[route.len() - 2], last) })
                        };
                        for q in lead_points_for(end, dir, side_sign, op, true, lead_length) {
                            e.feed(q[0], q[1], *z);
                        }
                    }
                    if has_lead {
                        e.retract();
                    }
                }
            }
            if stage != (Stage::Rough { stay_down: true }) {
                e.retract();
            }
        }
    };
    if separate_last {
        // Shape by shape: rough loops down to the depth before the last, then the finish at the last.
        let rough = settle(loops_at(radius + extra), e.last());
        let mut last = loops_at(radius);
        if op.last_reverse {
            for (pts, _) in &mut last {
                pts.reverse();
            }
        }
        // A ramp the whole way round only cuts flat at the last depth, so the rough loop goes there too.
        let spiral = op.ramp && op.ramp_kind == RampKind::Spiral;
        let upper = if spiral { &zs[..] } else { &zs[..zs.len() - 1] };
        let bottom = &zs[zs.len() - 1..];
        let mut shapes: Vec<usize> = Vec::new();
        for item in &rough {
            let shape = source(&item.0);
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        for shape in shapes {
            let own: Vec<_> = rough.iter().filter(|item| source(&item.0) == shape).cloned().collect();
            let (finish, others): (Vec<_>, Vec<_>) = last.into_iter().partition(|item: &(Vec<P>, bool)| source(&item.0) == shape);
            last = others;
            let linked = !upper.is_empty() && !op.ramp && !op.lead && own.len() == 1 && finish.len() == 1 && own[0].1 && finish[0].1;
            if !upper.is_empty() {
                run(e, own, upper, Stage::Rough { stay_down: linked });
            }
            run(e, finish, bottom, Stage::Finish { linked });
        }
        // A finish with no rough loop of its own: the offset can close up before reaching the shape.
        run(e, last, bottom, Stage::Finish { linked: false });
    } else {
        run(e, loops_at(radius + extra), &zs, Stage::Whole);
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
    // Each row runs down the middle of a band one stepover wide, so the first is half a step in.
    let first = -reach + step / 2.0;
    let start = [c[0] + first * normal[0] - reach * dir[0], c[1] + first * normal[1] - reach * dir[1]];
    (0..=((reach * 2.0 / step) as usize)).map(|i| ([start[0] + normal[0] * step * i as f64, start[1] + normal[1] * step * i as f64], dir, reach * 2.0)).collect()
}

/// How much short of the distance from the walls raster rows may go, in cells of the pocket's distance
/// grid: half a cell (0.025 mm for a 3.175 mm tool), which matches saved jobs' rows and lifts.
const RASTER_TOLERANCE_CELLS: f64 = 0.5;

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
fn cut_rows(e: &mut Emit, rows: Vec<(P, P, f64)>, sample: f64, z: f64, edges: &[Vec<P>], inside: &dyn Fn(f64, f64) -> bool) {
    // The stretches of each row, in the row's own direction.
    let rows: Vec<Vec<(P, P)>> = rows.into_iter().map(|row| row_spans(row, sample, inside)).collect();
    let edge = Edge::new(edges, rows.iter().flatten().flat_map(|(a, b)| [*a, *b]), 2.0 * sample);
    let mut done: Vec<Vec<bool>> = rows.iter().map(|r| vec![false; r.len()]).collect();
    let mut forward = true;
    let mut down = false;
    // Region by region, as the original (`0x1408cf3a0` sorts the rows into regions, `0x1408cfc00`
    // cuts each one whole): start at the first stretch not cut yet, then keep going to the stretch of
    // the next row the edge leads to (`0x1408cef30`), and lift only to start the next region.
    while let Some((mut r, mut k)) = (0..rows.len()).find_map(|r| done[r].iter().position(|d| !d).map(|k| (r, k))) {
        let (a, b) = rows[r][k];
        let (mut from, mut to) = if forward { (a, b) } else { (b, a) };
        let mut walk: Option<Vec<P>> = None;
        loop {
            match walk.take() {
                Some(points) if down => {
                    for q in points {
                        e.join(q[0], q[1], z);
                    }
                    e.join(from[0], from[1], z);
                }
                _ => e.go_to(from[0], from[1], z),
            }
            e.feed(to[0], to[1], z);
            done[r][k] = true;
            down = true;
            forward = !forward;
            // The next row's stretch reached along the edge from here, with no other row end on the way.
            let Some(next) = rows.get(r + 1) else { break };
            let found = next.iter().enumerate().filter(|(j, _)| !done[r + 1][*j]).find_map(|(j, (a, b))| {
                [(*a, *b), (*b, *a)].into_iter().find_map(|(s, t)| edge.walk(to, s).map(|w| (j, s, t, w)))
            });
            let Some((j, s, t, w)) = found else { break };
            r += 1;
            k = j;
            from = s;
            to = t;
            walk = Some(w);
        }
    }
    e.retract();
}

/// The outlines raster rows end on, with where each row end sits along them, for going from one
/// row to the next along the edge.
struct Edge<'a> {
    loops: &'a [Vec<P>],
    /// Each loop's length, and its points' distance along it from its first point.
    lengths: Vec<(f64, Vec<f64>)>,
    /// Row ends that sit on an outline: which one, and how far along.
    ends: Vec<(usize, f64)>,
    near: f64,
}

impl<'a> Edge<'a> {
    fn new(loops: &'a [Vec<P>], ends: impl Iterator<Item = P>, near: f64) -> Self {
        let lengths = loops
            .iter()
            .map(|l| {
                let mut along = Vec::with_capacity(l.len());
                let mut total = 0.0;
                for (i, p) in l.iter().enumerate() {
                    along.push(total);
                    let q = l[(i + 1) % l.len()];
                    total += (q[0] - p[0]).hypot(q[1] - p[1]);
                }
                (total, along)
            })
            .collect();
        let mut edge = Self { loops, lengths, ends: Vec::new(), near };
        edge.ends = ends.filter_map(|p| edge.place(p)).collect();
        edge
    }

    /// Which outline a point is on, within reach, and how far along it.
    fn place(&self, p: P) -> Option<(usize, f64)> {
        let mut best: Option<(f64, usize, f64)> = None;
        for (k, l) in self.loops.iter().enumerate() {
            if l.len() < 2 {
                continue;
            }
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let d = (a[0] + dx * t - p[0]).hypot(a[1] + dy * t - p[1]);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, k, self.lengths[k].1[i] + t * len2.sqrt()));
                }
            }
        }
        best.filter(|(d, _, _)| *d <= self.near).map(|(_, k, s)| (k, s))
    }

    /// The corners of the outline met going from one row end to the next the shorter way that has
    /// no other row end on it, or None when both ways do or the two aren't on one outline.
    fn walk(&self, from: P, to: P) -> Option<Vec<P>> {
        let (k, a) = self.place(from)?;
        let (k2, b) = self.place(to)?;
        if k != k2 {
            return None;
        }
        let (total, along) = &self.lengths[k];
        let tiny = (self.near * 0.01).max(1e-9);
        // How far along from `a` going forward, round the loop.
        let ahead = |s: f64| (s - a).rem_euclid(*total);
        let forward_len = ahead(b);
        let backward_len = *total - forward_len;
        let clear = |forward: bool| {
            self.ends.iter().filter(|(kk, _)| *kk == k).all(|(_, s)| {
                let d = if forward { ahead(*s) } else { (*total - ahead(*s)).rem_euclid(*total) };
                let len = if forward { forward_len } else { backward_len };
                d <= tiny || d >= len - tiny
            })
        };
        let mut ways = [(forward_len, true), (backward_len, false)];
        ways.sort_by(|x, y| x.0.total_cmp(&y.0));
        let (_, forward) = *ways.iter().find(|(_, f)| clear(*f))?;
        let l = &self.loops[k];
        let n = l.len();
        let mut corners: Vec<(f64, P)> = (0..n)
            .map(|i| {
                let d = if forward { ahead(along[i]) } else { (*total - ahead(along[i])).rem_euclid(*total) };
                (d, l[i])
            })
            .filter(|(d, _)| *d > tiny && *d < if forward { forward_len } else { backward_len } - tiny)
            .collect();
        corners.sort_by(|x, y| x.0.total_cmp(&y.0));
        Some(corners.into_iter().map(|(_, p)| p).collect())
    }
}

/// How far, in stepovers, the next ring's start may be for the tool to go straight there
/// whatever lies between (`0x1407b5500`: the distance under the stepover times 1.001, `0x1421cf160`).
pub const RING_LINK_STEPS: f64 = 1.001;

/// An innermost pocket ring smaller than this share of the tool's diameter, across its widest, is
/// left to the passes around it. Burin's choice: its grid makes such specks where a stroke's middle
/// just touches a level, and the original's offsets don't.
pub const TINY_RING_SHARE: f64 = 0.25;


/// A loop just inside an outline drawn onto the shape's middle line: each point climbs straight
/// in, away from the outline, and stops where its way turns, on the middle line, so the points
/// near a corner land along the line into that corner instead of sliding on to the deepest part.
fn onto_middle_line(field: &Field, ring: &[P]) -> Vec<P> {
    middle_line_of(field, ring, false)
}

/// The same, with `keep_sharp`: the loop's sharp corners (turning `EAR_CORNER_DEGREES` or more)
/// stay where they are. An exact offset's sharp corners lie on the middle line already, at the
/// ends of its branches; climbing from there runs up the branch and leaves its end uncut.
fn middle_line_of(field: &Field, ring: &[P], keep_sharp: bool) -> Vec<P> {
    let h = field.cell * 0.5;
    let grad = |q: P| {
        let gx = field.sample(q[0] + h, q[1]) - field.sample(q[0] - h, q[1]);
        let gy = field.sample(q[0], q[1] + h) - field.sample(q[0], q[1] - h);
        let len = gx.hypot(gy);
        if len > 1e-12 { [gx / len, gy / len] } else { [0.0, 0.0] }
    };
    let pace = field.cell * 0.25;
    let turned = MIDDLE_TURN_DEGREES.to_radians().cos();
    // A point every grid cell along the loop: a long straight side has none between its ends.
    let m = ring.len();
    let sharp = |k: usize| {
        let (a, p, b) = (ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]);
        let (u, v) = ([p[0] - a[0], p[1] - a[1]], [b[0] - p[0], b[1] - p[1]]);
        keep_sharp && u[0].hypot(u[1]) > 1e-9 && v[0].hypot(v[1]) > 1e-9 && (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).abs() >= EAR_CORNER_DEGREES.to_radians()
    };
    let mut dense: Vec<(P, bool)> = Vec::with_capacity(ring.len());
    for k in 0..m {
        let (a, b) = (ring[k], ring[(k + 1) % m]);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / field.cell).ceil().max(1.0) as usize;
        let corner = sharp(k);
        dense.extend((0..n).map(|i| {
            let t = i as f64 / n as f64;
            ([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], i == 0 && corner)
        }));
    }
    let mut out: Vec<P> = Vec::with_capacity(dense.len());
    for &(p, stays) in &dense {
        if stays {
            out.push(p);
            continue;
        }
        let start = grad(p);
        let (mut q, mut v) = (p, field.sample(p[0], p[1]));
        for _ in 0..1_000_000 {
            let g = grad(q);
            if g[0] * start[0] + g[1] * start[1] < turned {
                break;
            }
            let next = [q[0] + g[0] * pace, q[1] + g[1] * pace];
            let w = field.sample(next[0], next[1]);
            if w <= v {
                break;
            }
            (q, v) = (next, w);
        }
        if out.last().is_none_or(|l: &P| (l[0] - q[0]).hypot(l[1] - q[1]) > field.cell * 0.5) {
            out.push(q);
        }
    }
    while out.len() < 2 {
        out.push(out.last().copied().unwrap_or(ring[0]));
    }
    out
}

/// How far the way in may turn before a point counts as on the middle line.
const MIDDLE_TURN_DEGREES: f64 = 25.0;


/// Whether the field is higher inside this level's loop than on it: a loop round a part of the
/// pocket, not round a hole in it.
fn encloses_higher(field: &Field, ring: &[P], level: f64) -> bool {
    let (a, b) = (ring[0], ring[1]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-12);
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let d = field.cell * 0.5;
    let around = [Path::closed(ring.to_vec())];
    [1.0, -1.0].iter().map(|s| [mid[0] - dy / len * d * s, mid[1] + dx / len * d * s]).find(|q| inside_even_odd(around.iter(), *q)).is_some_and(|q| field.sample(q[0], q[1]) > level)
}

/// How much of the way on, in stepovers, is left out at each end when looking for crossings
/// (`0x1407b5500`: the stepover times 0.01, `0x1420fbbb0`).
pub const RING_LINK_TRIM: f64 = 0.01;

/// When a pocket's next ring may go on from the one before without lifting.
#[derive(Clone, Copy, Debug)]
enum Link {
    No,
    /// Only when its start is within 1.001 stepovers. The original doesn't link rings of one level,
    /// but its offsets don't split where a ring merely pinches, as Burin's grid can: a start that
    /// close is that pinch, not another branch. Burin's choice.
    Near,
    /// Within 1.001 stepovers, or further when the straight way crosses no ring.
    Clear,
}

/// Whether the straight way from `a` to `b` crosses one of these loops anywhere but at its ends.
fn crosses_rings(a: P, b: P, loops: &[&[P]], trim: f64) -> bool {
    let r = [b[0] - a[0], b[1] - a[1]];
    let len = r[0].hypot(r[1]);
    if len < 1e-9 {
        return false;
    }
    // The ends lie on rings themselves: crossings this close to them don't count. The original
    // shortens the way by 1% of the stepover at each end (`0x1407b5500`, `0x1420fbbb0`).
    let end = (trim.max(1e-6) / len).max(1e-6);
    loops.iter().any(|l| {
        (0..l.len()).any(|i| {
            let (c, d) = (l[i], l[(i + 1) % l.len()]);
            let s = [d[0] - c[0], d[1] - c[1]];
            let den = r[0] * s[1] - r[1] * s[0];
            if den.abs() < 1e-12 {
                return false;
            }
            let q = [c[0] - a[0], c[1] - a[1]];
            let t = (q[0] * s[1] - q[1] * s[0]) / den;
            let u = (q[0] * r[1] - q[1] * r[0]) / den;
            t > end && t < 1.0 - end && (0.0..=1.0).contains(&u)
        })
    })
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
    for set in sets {
        let mut one = op.clone();
        one.paths = set.iter().map(|i| op.paths[*i].clone()).collect();
        pocket_set(e, &one);
    }
}

/// What's ticked, or only the order of selection when that's what the form asks for.
fn wanted_order(op: &Op, choices: OrderChoices) -> OrderChoices {
    if op.order == Order::AsDrawn && !choices.selection {
        return OrderChoices { selection: true, left_to_right: false, bottom_to_top: false, grid: false, shortest: false };
    }
    choices
}

fn pocket_set(e: &mut Emit, op: &Op) {
    let radius = op.tool.dia / 2.0 + op.allowance;
    let step = step_of(op);
    let Some(field) = Field::build_to(&op.paths, op.tool.dia, 900, op.tool.dia / 64.0) else { return };
    let exact_loops: Option<Vec<Vec<P>>> = op.paths.iter().all(|p| p.closed).then(|| op.paths.iter().map(|p| p.pts.clone()).collect());
    let exact = exact_loops.as_deref();
    let zs = depths(op);
    // Places only just as wide as the tool, a hole of its own diameter say, where the first ring
    // shrinks to nothing: the original still cuts them (its pocket, `0x14081fa20`, tests the widest
    // point against the radius less the tolerance, `0x1407c9a60`, `0x1407c98f0`).
    let snug = if op.rest_only { Vec::new() } else { snug_spots(&field, &op.paths, radius, toolpath_tolerance(op)) };
    let (lo, hi) = bbox(&op.paths).unwrap();
    // Cuts one ring from its point nearest the tool. With `may_link`, the tool goes on at depth from
    // the ring before, as the original links a ring to the next (`0x1407b5200`, `0x1407b5500`): when
    // the start is within 1.001 stepovers, or further when the straight way crosses none of `walls`.
    // Never closer to a wall than the tool's radius.
    // `at`: where the ring is to start, its point nearest there; None for its point nearest the tool.
    let cut_ring = |e: &mut Emit, ring: Vec<P>, z: f64, from: f64, link: Link, walls: &[&[P]], at: Option<P>| {
        let ramp = (op.ramp && op.ramp_distance > 0.0).then_some(from);
        let last = e.last();
        let far = |q: &P| (q[0] - last[0]).hypot(q[1] - last[1]);
        let mut pts = oriented(ring, op.climb);
        match at {
            Some(target) => start_loop_near_point(&mut pts, target),
            None => {
                let nearest = (0..pts.len()).min_by(|a, b| far(&pts[*a]).total_cmp(&far(&pts[*b]))).unwrap_or(0);
                // Of the points about as near as the nearest (within half a stepover more),
                // one the tool can go on to without crossing a ring: worked-out rings put their
                // corners a hair apart on either side of a straight slot, and the nearest by a
                // hair can be the one across the ring just cut.
                let tie = far(&pts[nearest]) + 0.5 * step;
                // Of those, a node if there is one: the original's rings are lines and arcs, and a
                // ring starts where one of them does (`0x14079b600` picks among span starts). The
                // points only put in along a straight stretch are passed over.
                let n = pts.len();
                let node = |k: usize| {
                    let (a, p, b) = (pts[(k + n - 1) % n], pts[k], pts[(k + 1) % n]);
                    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                    len <= 0.0 || ((p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])).abs() / len >= 1e-6
                };
                let candidates: Vec<usize> = (0..n)
                    .filter(|k| far(&pts[*k]) <= tie && !matches!(link, Link::No))
                    .filter(|k| !crosses_rings([last[0], last[1]], pts[*k], walls, step * RING_LINK_TRIM))
                    .collect();
                let closest = |ks: &mut dyn Iterator<Item = usize>| ks.min_by(|a, b| far(&pts[*a]).total_cmp(&far(&pts[*b])));
                // A node only where the tool still goes on to it without lifting.
                let goes_on = |k: usize| !matches!(link, Link::Near) || far(&pts[k]) <= step * RING_LINK_STEPS;
                let reachable = closest(&mut candidates.iter().copied().filter(|k| node(*k) && goes_on(*k))).or_else(|| closest(&mut candidates.iter().copied()));
                pts.rotate_left(reachable.unwrap_or(nearest));
            }
        }
        let across = far(&pts[0]);
        let to = [pts[0][0], pts[0][1]];
        let near = across <= step * RING_LINK_STEPS;
        let linked = match link {
            Link::No => false,
            Link::Near => near,
            Link::Clear => near || !crosses_rings([last[0], last[1]], to, walls, step * RING_LINK_TRIM),
        };
        let stays_down = ramp.is_none() && (last[2] - z).abs() < 1e-9 && linked && {
            let steps = ((across / field.cell).ceil() as usize).max(1);
            (0..=steps).all(|k| {
                let t = k as f64 / steps as f64;
                field.sample(last[0] + (pts[0][0] - last[0]) * t, last[1] + (pts[0][1] - last[1]) * t) >= radius - field.cell * 0.5
            })
        };
        if stays_down {
            e.feed(pts[0][0], pts[0][1], z);
        } else {
            e.go_to(pts[0][0], pts[0][1], ramp.unwrap_or(z));
        }
        // The points only put in to choose the start from go again, its start kept.
        let pts = without_straight_points(pts);
        walk(e, &pts, true, z, op, ramp, None);
    };
    // The nearest of these rings to where the tool is.
    let nearest_of = |e: &Emit, rings: &[&Vec<P>]| -> usize {
        let last = e.last();
        let near = |pts: &Vec<P>| pts.iter().map(|q| (q[0] - last[0]).hypot(q[1] - last[1])).fold(f64::MAX, f64::min);
        (0..rings.len()).min_by(|a, b| near(rings[*a]).total_cmp(&near(rings[*b]))).unwrap_or(0)
    };
    // One level's rings, nearest first; only the first may go on from what came before.
    let ring = |e: &mut Emit, level: f64, z: f64, from: f64| {
        let mut rings = area_contours(&field, level, toolpath_tolerance(op), exact);
        let inner = area_contours(&field, level + step, toolpath_tolerance(op), exact);
        // Kept apart from `rings`, which empties as they're cut.
        let owned: Vec<Vec<P>> = rings.iter().chain(&inner).cloned().collect();
        let walls: Vec<&[P]> = owned.iter().map(|w| w.as_slice()).collect();
        let mut first = true;
        while !rings.is_empty() {
            let i = nearest_of(e, &rings.iter().collect::<Vec<_>>());
            cut_ring(e, rings.swap_remove(i), z, from, if first { Link::Clear } else { Link::No }, &walls, None);
            first = false;
        }
    };
    // Every level at once, as a tree: each ring's parent is the ring of the level around it that
    // turns the same way and lies nearest. Each branch is cut from the inside out and a ring right
    // after one of its own, so the tool goes on from the last child to its parent and lifts only to
    // start another branch, as the original chains rings parent to child (`0x1407b99b0`).
    let ring_tree = |e: &mut Emit, levels: &[f64], z: f64, from: f64| {
        let mut rings: Vec<Vec<Vec<P>>> = levels.iter().map(|l| area_contours(&field, *l, toolpath_tolerance(op), exact)).collect();
        // Each ring after the first reaches into its sharp corners, out to where the ring before
        // stopped clearing (saved jobs: ears 1.96 mm long at the corners of a slot's second ring
        // with a 94% stepover, their tips 3.18 from the walls, the first ring's level and the
        // tool's radius).
        // The pieces each ring after the first leaves for strokes of their own.
        let mut leftovers: Vec<Vec<(Vec<P>, Vec<P>)>> = vec![Vec::new(); levels.len()];
        if let Some(loops) = exact {
            let tolerance = toolpath_tolerance(op);
            for i in 1..levels.len() {
                let target = levels[i - 1] + op.tool.dia / 2.0;
                if target >= levels[i] {
                    continue;
                }
                // The middle line's branch into a corner, where it bends (a corner meeting a round
                // end): the sharp corners of the exact offsets at levels between, outermost last.
                let between: Vec<Vec<Vec<P>>> = (1..=EAR_LEVELS).map(|j| {
                    let level = levels[i] - (levels[i] - target) * j as f64 / EAR_LEVELS as f64;
                    let engine = tolerance * OFFSET_TOLERANCE_SHARE;
                    crate::solid::shrunk(loops, level, (8.0 * engine / level.max(field.cell)).sqrt().clamp(0.02, 0.5))
                }).collect();
                let mut pieces = leftover_pieces(&field, loops, levels[i - 1], levels[i], op.tool.dia / 2.0, tolerance);
                for r in rings[i].iter_mut() {
                    *r = with_corner_ears(std::mem::take(r), levels[i] - target, &between, &mut pieces, op.tool.dia / 2.0 + field.cell, field.cell);
                }
                leftovers[i] = pieces;
            }
        }
        // Per level, where the strokes along the middle begin among its rings.
        let mut stroke_from: Vec<usize> = vec![usize::MAX; rings.len()];
        // Where a ring leaves material, further from the walls than its level and the tool's
        // radius, in a part too narrow for the next ring, the original runs along the middle of
        // what's left, out and back (saved jobs: a bend in a band, rings at 1.59 and 4.55 mm from
        // the walls, a 16 mm stroke where the middle is 3.18 to 3.44 mm in; the meeting of three
        // arms in a logo, a 0.8 mm stroke where they're 5.18 to 5.39 mm in; a single plunge in
        // the middle of a round pocket). That level's loops with no next ring inside give it,
        // drawn in onto the middle line; it is cut as one of the next level's rings, so the tree
        // chains it as it does them.
        for i in 0..levels.len() {
            let half = levels[i] + op.tool.dia / 2.0;
            // Exact when the shapes are closed: the loop's corners, where the strokes' ears end,
            // are sharp there and rounded off on the grid (the person's desk, "ruban leds": a
            // stroke round a corner of the slot 0.35 and 0.53 mm short at its two ends).
            let whole: Vec<Vec<P>> = area_contours(&field, half, toolpath_tolerance(op), exact)
                .into_iter()
                .filter(|l| l.len() >= 3 && encloses_higher(&field, l, half))
                .filter(|l| {
                    let around = [Path::closed(l.clone())];
                    // No next ring inside it, nor round it (with a stepover under the tool's
                    // radius the next ring lies between this one and the loop, and clears it).
                    rings.get(i + 1).is_none_or(|next| {
                        !next.iter().any(|n| inside_even_odd(around.iter(), n[0]) || inside_even_odd(std::iter::once(&Path::closed(n.clone())), l[0]))
                    })
                })
                .collect();
            let strokes: Vec<Vec<P>> = whole
                .iter()
                .map(|l| {
                    // Into the corners too, as the saved jobs' strokes do: the loop's corners lie on the
                    // middle line's branches into the corners, where the ring before stops
                    // clearing (its level and the tool's radius in), and stay there.
                    without_repeats(crate::geom::simplify_loop(&middle_line_of(&field, l, exact.is_some()), field.cell * STROKE_THIN_CELLS), field.cell)
                })
                .collect();
            // Where there is a next ring: the pieces the ring before leaves and the next ring's
            // tool doesn't reach either, each cut along its middle (the person's desk, "ruban
            // leds": a curved stroke 8 mm long in a slot's rounded corner, where the second
            // ring doesn't go, its ends 3.20 from the walls). The pieces at the next ring's
            // corners are left to its ears, which clear them.
            let mut strokes = strokes;
            if let (Some(next), Some(pieces)) = (rings.get(i + 1).filter(|_| exact.is_some()), leftovers.get(i + 1)) {
                strokes.extend(leftover_strokes(&field, pieces, op.tool.dia / 2.0, next, &whole));
            }
            if strokes.is_empty() {
                continue;
            }
            if i + 1 == rings.len() {
                rings.push(Vec::new());
            }
            stroke_from.resize(rings.len(), usize::MAX);
            stroke_from[i + 1] = stroke_from[i + 1].min(rings[i + 1].len());
            rings[i + 1].extend(strokes);
        }
        let is_stroke = |i: usize, j: usize| stroke_from.get(i).is_some_and(|from| j >= *from);
        let mut children: Vec<Vec<Vec<usize>>> = rings.iter().map(|r| vec![Vec::new(); r.len()]).collect();
        // The strokes along the middle that follow each ring, cut once it is: after the rings round
        // a round pocket's centre, after the ring round a band's bend, as in the saved jobs (the
        // original walks its tree of rings from the outside in, `0x1407b99b0`, a stroke starting
        // a chain of its own).
        let mut after: Vec<Vec<Vec<usize>>> = rings.iter().map(|r| vec![Vec::new(); r.len()]).collect();
        for i in 1..rings.len() {
            let turns: Vec<bool> = rings[i - 1].iter().map(|r| signed_area(r) > 0.0).collect();
            for (j, ring) in rings[i].iter().enumerate() {
                let p = ring[0];
                let gap = |k: &usize| rings[i - 1][*k].iter().map(|q| (q[0] - p[0]).hypot(q[1] - p[1])).fold(f64::MAX, f64::min);
                let turn = signed_area(ring) > 0.0;
                let all = (0..rings[i - 1].len()).filter(|k| !is_stroke(i - 1, *k));
                let parent = all.clone().filter(|k| turns[*k] == turn).min_by(|a, b| gap(a).total_cmp(&gap(b))).or_else(|| all.min_by(|a, b| gap(a).total_cmp(&gap(b))));
                if let Some(k) = parent {
                    if is_stroke(i, j) {
                        after[i - 1][k].push(j);
                    } else {
                        children[i - 1][k].push(j);
                    }
                }
            }
        }
        let Some(top) = rings.first() else { return };
        // Where a branch's first ring starts, as in the saved jobs: the start of the shape is carried
        // in from ring to ring (each at its point nearest the start of the one around it), so the
        // innermost ring starts on the side of the shape's start. The rings cut after it start where
        // the tool is, so the starts run in a line from the middle out to the shape's start.
        let shape_starts: Vec<P> = op.paths.iter().filter(|p| p.closed && !p.pts.is_empty()).map(|p| p.pts[0]).collect();
        let nearest_point = |ring: &[P], q: P| {
            let mut pts = ring.to_vec();
            if pts.len() >= 2 {
                start_loop_near_point(&mut pts, q);
            }
            pts.first().copied().unwrap_or(q)
        };
        let mut starts: Vec<Vec<P>> = rings.iter().map(|r| vec![[0.0, 0.0]; r.len()]).collect();
        for (j, ring) in top.iter().enumerate() {
            let line = Path::closed(ring.clone());
            let target = shape_starts.iter().copied().min_by(|a, b| distance_to_path(*a, &line).total_cmp(&distance_to_path(*b, &line))).unwrap_or(ring[0]);
            starts[0][j] = nearest_point(ring, target);
        }
        for i in 0..rings.len().saturating_sub(1) {
            for j in 0..rings[i].len() {
                for &c in &children[i][j] {
                    starts[i + 1][c] = nearest_point(&rings[i + 1][c], starts[i][j]);
                }
            }
        }
        let mut roots: Vec<usize> = (0..top.len()).collect();
        while !roots.is_empty() {
            let r = roots.swap_remove(nearest_of(e, &roots.iter().map(|k| &top[*k]).collect::<Vec<_>>()));
            let mut stack: Vec<(usize, usize, Vec<usize>)> = vec![(0, r, children[0][r].clone())];
            let mut first_ring = true;
            // The branch's strokes, cut once all its rings are.
            let mut waiting: Vec<(usize, usize)> = Vec::new();
            while let Some(at) = stack.len().checked_sub(1) {
                let (i, j) = (stack[at].0, stack[at].1);
                if stack[at].2.is_empty() {
                    stack.pop();
                    // What the way on mustn't cross: this ring and its own inner rings, the last of
                    // which was just cut (`0x1407b5500` looks at the two rings and the outer one's
                    // other inner rings).
                    let walls: Vec<&[P]> = std::iter::once(&rings[i][j]).chain(children[i][j].iter().map(|c| &rings[i + 1][*c])).map(|w| w.as_slice()).collect();
                    // Only a parent's last child goes on into it (`0x1407b5200`: no next sibling,
                    // the parent not taken yet, both closed and turning the same way). A ring
                    // with a stroke along its middle has that stroke as its last child, so its
                    // inner ring lifts before it, as in the saved band pockets.
                    // A stroke along the middle with some length is the ring's last child: it is
                    // cut now, after the ring's other children, and the ring goes on from it. A
                    // single plunge isn't a closed ring and never goes on into one: it waits for
                    // the end of the branch.
                    let (strokes, plunges): (Vec<usize>, Vec<usize>) = after[i][j].iter().partition(|s| {
                        let ring = &rings[i + 1][**s];
                        ring.iter().any(|q| (q[0] - ring[0][0]).hypot(q[1] - ring[0][1]) > field.cell)
                    });
                    waiting.extend(plunges.iter().map(|s| (i + 1, *s)));
                    let mut left = strokes.clone();
                    while !left.is_empty() {
                        let k = left.swap_remove(nearest_of(e, &left.iter().map(|s| &rings[i + 1][*s]).collect::<Vec<_>>()));
                        cut_ring(e, rings[i + 1][k].clone(), z, from, Link::No, &[rings[i + 1][k].as_slice()], None);
                    }
                    let link = if !strokes.is_empty() {
                        Link::Near
                    } else if children[i][j].is_empty() {
                        Link::Near
                    } else {
                        Link::Clear
                    };
                    // An innermost ring that is hardly more than a point, where the grid meets a level
                    // at the very middle of a stroke, isn't cut on its own: the passes around it
                    // reach it, and whatever they leave is cleaned from them afterwards (`tuck_spots`).
                    let tiny = children[i][j].is_empty()
                        && crate::geom::bbox(std::iter::once(&Path::closed(rings[i][j].clone()))).is_some_and(|(lo, hi)| (hi[0] - lo[0]).max(hi[1] - lo[1]) < TINY_RING_SHARE * op.tool.dia);
                    if !tiny {
                        // Only the first ring of a branch starts on the side of the shape's start;
                        // every ring after it starts where the tool is, as the original goes on.
                        let at = first_ring.then_some(starts[i][j]);
                        first_ring = false;
                        cut_ring(e, rings[i][j].clone(), z, from, link, &walls, at);
                    }
                } else {
                    let rest = &stack[at].2;
                    let pick = nearest_of(e, &rest.iter().map(|c| &rings[i + 1][*c]).collect::<Vec<_>>());
                    let c = stack[at].2.swap_remove(pick);
                    stack.push((i + 1, c, children[i + 1][c].clone()));
                }
            }
            while !waiting.is_empty() {
                let k = nearest_of(e, &waiting.iter().map(|(i, j)| &rings[*i][*j]).collect::<Vec<_>>());
                let (i, j) = waiting.swap_remove(k);
                cut_ring(e, rings[i][j].clone(), z, from, Link::Near, &[rings[i][j].as_slice()], None);
            }
        }
    };
    let mut previous = -op.start_depth;
    let cut_from = e.moves.len();
    let mut spots: Option<Vec<P>> = None;
    let mut adaptive: Option<Vec<(Vec<P>, bool)>> = None;
    for z in &zs {
        let depth_from = e.moves.len();
        if op.rest_only {
            ring(e, radius, *z, previous);
            // Then what the larger tool couldn't reach: corners and slots too narrow for it, as
            // other CAM programs' rest machining does. The original works both tools out in one go
            // (`0x1405467b0` hands both to `0x1405680a0`).
            if let Some(big) = &op.clearance_tool {
                let big_r = big.dia / 2.0;
                let rest = spots.get_or_insert_with(|| leftovers_after(&field, op.tool.dia / 2.0, radius, op.allowance, &e.moves[cut_from..], Some((big_r, big_r + op.allowance))));
                e.retract();
                cut_spots(e, rest, *z, &field, radius);
            }
        } else if op.adaptive {
            let helix = if op.ramp { adaptive_helix(&field, op.tool.dia / 2.0, radius) } else { 0.0 };
            let paths = adaptive.get_or_insert_with(|| adaptive_paths(&field, op.tool.dia / 2.0, radius, step, op.climb, helix));
            for (k, (pts, stay_down)) in paths.iter().enumerate() {
                let Some(first) = pts.first() else { continue };
                if k == 0 && helix > field.cell {
                    // With ramping on, the first cut comes down in a helix rather than straight.
                    helix_down(e, op, *first, helix, previous, *z);
                } else if *stay_down && k > 0 && (e.last()[2] - *z).abs() < 1e-9 {
                    e.join(first[0], first[1], *z);
                } else {
                    e.go_to(first[0], first[1], *z);
                }
                for p in &pts[1..] {
                    e.feed(p[0], p[1], *z);
                }
            }
            // A last pass round the wall leaves it clean.
            ring(e, radius, *z, *z);
        } else if op.raster {
            if op.profile_pass == ProfilePass::First {
                ring(e, radius, *z, previous);
            }
            // An outline pass cleans up after the rows, so they can stop short of it.
            let extra = if op.profile_pass == ProfilePass::None { 0.0 } else { op.raster_allowance.abs() };
            let clear = radius + extra;
            // The rows' box is this pocket's own outline offset by that much (`0x140820030` makes the
            // offset outline, `0x1408cf270` takes its box), checked against saved jobs pocket by pocket.
            let offset: Vec<Path> = field.contours(clear).into_iter().map(Path::closed).collect();
            let (box_lo, box_hi) = bbox(&offset).unwrap_or(([lo[0] + clear, lo[1] + clear], [hi[0] - clear, hi[1] - clear]));
            let rows = pocket_rows(box_lo, box_hi, op.raster_angle, step);
            // Rows may come short of the full distance by a hair, as the original offsets by the distance
            // less its tolerance (`0x140820030`): a row that just touches the edge is still cut, as in saved jobs.
            let reach = clear - field.cell * RASTER_TOLERANCE_CELLS;
            let edges: Vec<Vec<P>> = offset.iter().map(|p| p.pts.clone()).collect();
            cut_rows(e, rows, field.cell, *z, &edges, &|x, y| field.sample(x, y) >= reach);
            // A second set of rows across the first, in the same box, added after them (`0x140820030`
            // joins the rows at the angle + 90 degrees, `0x1420fbc08`, to the first set).
            if op.raster_second {
                let across = pocket_rows(box_lo, box_hi, op.raster_angle + 90.0, step);
                cut_rows(e, across, field.cell, *z, &edges, &|x, y| field.sample(x, y) >= reach);
            }
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
            ring_tree(e, &levels, *z, previous);
        }
        // There, a circle of a hundredth of the tool's radius, as the saved test job has (a 3 mm
        // hole with a 3 mm end mill: a circle 0.015 mm round its middle). Its size is estimated
        // from that one job; it is only a plunge with a hair of movement.
        for c in &snug {
            let r = op.tool.dia / 2.0 * SNUG_CIRCLE_SHARE;
            e.go_to(c[0] + r, c[1], *z);
            for k in 1..=16 {
                let a = std::f64::consts::TAU * k as f64 / 16.0 * if op.climb { 1.0 } else { -1.0 };
                e.feed(c[0] + r * a.cos(), c[1] + r * a.sin(), *z);
            }
        }
        // What the passes above didn't reach, worked out once and cut at every depth, going on from
        // the passes without lifting. A pocket that only clears the rest of a larger tool leaves the
        // other tool's share on purpose.
        if !op.rest_only {
            let spots = spots.get_or_insert_with(|| leftovers(&field, op.tool.dia / 2.0, radius, op.allowance, &e.moves[cut_from..]));
            let missed = tuck_spots(e, spots, depth_from, *z, &field, radius, SPOT_DETOUR_STEPS * step);
            cut_spots(e, &missed, *z, &field, radius);
        }
        e.retract();
        previous = *z;
    }
}

/// Where the tool has to go to clear what the given moves left in a pocket: the middle of a bend or
/// a point, say, too far from any pass. A spot the tool can't reach without cutting into a wall is
/// left: a round tool can't get into a sharp corner.
fn leftovers(field: &Field, tool: f64, centre_at: f64, allowance: f64, moves: &[Move]) -> Vec<P> {
    leftovers_after(field, tool, centre_at, allowance, moves, None)
}

/// The radius of the helix an adaptive pocket comes down in: half the cutter's radius, less where
/// the pocket is too tight round its deepest point. Burin's choice.
pub const ADAPTIVE_HELIX_SHARE: f64 = 0.5;

fn adaptive_helix(field: &Field, tool: f64, centre_at: f64) -> f64 {
    // The deepest point: the most room, and where the adaptive paths start.
    let deepest = field.d.iter().copied().fold(0.0f32, f32::max) as f64;
    (tool * ADAPTIVE_HELIX_SHARE).min(deepest - centre_at).max(0.0)
}

/// Comes down round a circle from `from` to `z`, over the ramp's length (once round when it has
/// none, as a spiral ramp goes the whole way round), then goes once round at depth and back to
/// the middle.
fn helix_down(e: &mut Emit, op: &Op, centre: P, r: f64, from: f64, z: f64) {
    let round = std::f64::consts::TAU * r;
    let length = ramp_distance(op, from, z).min(round * 50.0).max(round);
    let turns = length / round;
    let n = ((turns * 48.0).ceil() as usize).max(12);
    e.go_to(centre[0] + r, centre[1], from);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let a = std::f64::consts::TAU * turns * t;
        e.feed(centre[0] + r * a.cos(), centre[1] + r * a.sin(), from + (z - from) * t);
    }
    let a0 = std::f64::consts::TAU * turns;
    for k in 1..=48 {
        let a = a0 + std::f64::consts::TAU * k as f64 / 48.0;
        e.feed(centre[0] + r * a.cos(), centre[1] + r * a.sin(), z);
    }
    e.feed(centre[0], centre[1], z);
}

/// How much of the stepover's width an adaptive step may bite, at most: the angle search stops at
/// the first direction that would take more.
pub const ADAPTIVE_STEP_SHARE: f64 = 0.5;

/// Adaptive clearing, as other CAM programs do it (FreeCAD's Adaptive, Fusion 360's adaptive
/// clearing): the material left is kept on a grid, and the cutter moves on in small steps, each
/// time turning to the direction where it removes no more than `load` across its path, keeping the
/// material on its right for climb cutting (left for conventional). When nothing is left within
/// reach it goes on to the nearest place where material starts. Returns the paths in order, each
/// with whether the way to it from the end of the one before stays in cleared ground.
fn adaptive_paths(field: &Field, tool: f64, centre_at: f64, load: f64, climb: bool, helix: f64) -> Vec<(Vec<P>, bool)> {
    let (width, height) = ((field.nx - 1) as f64 * field.cell, (field.ny - 1) as f64 * field.cell);
    let mut cell = field.cell.max(tool / 6.0);
    while (width / cell) * (height / cell) > 3.0e6 {
        cell *= 1.25;
    }
    let (nx, ny) = ((width / cell) as usize + 1, (height / cell) as usize + 1);
    let at = |ix: usize, iy: usize| [field.x0 + ix as f64 * cell, field.y0 + iy as f64 * cell];
    let allowed = |p: P| field.sample(p[0], p[1]) >= centre_at;
    // Material: whatever a centre the cutter may reach can touch.
    let reach: Vec<bool> = (0..nx * ny).map(|i| allowed(at(i % nx, i / nx))).collect();
    if !reach.iter().any(|r| *r) {
        return Vec::new();
    }
    let near = squared_distances(&reach, nx, ny);
    let r_cells = tool / cell;
    let mut material: Vec<bool> = near.iter().map(|d| *d <= r_cells * r_cells).collect();
    let mut left = material.iter().filter(|m| **m).count();
    let disc: Vec<(i64, i64)> = {
        let c = r_cells.ceil() as i64;
        (-c..=c).flat_map(|y| (-c..=c).map(move |x| (x, y))).filter(|(x, y)| ((x * x + y * y) as f64) <= r_cells * r_cells).collect()
    };
    let index = |p: P| {
        let (fx, fy) = (((p[0] - field.x0) / cell).round() as i64, ((p[1] - field.y0) / cell).round() as i64);
        (fx, fy)
    };
    let bite = |material: &Vec<bool>, p: P| -> usize {
        let (fx, fy) = index(p);
        disc.iter().filter(|(dx, dy)| {
            let (x, y) = (fx + dx, fy + dy);
            x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny && material[y as usize * nx + x as usize]
        }).count()
    };
    let clear = |material: &mut Vec<bool>, left: &mut usize, p: P| {
        let (fx, fy) = index(p);
        for (dx, dy) in &disc {
            let (x, y) = (fx + dx, fy + dy);
            if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny && material[y as usize * nx + x as usize] {
                material[y as usize * nx + x as usize] = false;
                *left -= 1;
            }
        }
    };
    let step = (tool / 4.0).max(cell);
    // Cells a step may take: the load across the step's length, plus the front of the cutter.
    let target = ((load.min(tool * 2.0) * step) / (cell * cell)).max(1.0) * (1.0 + ADAPTIVE_STEP_SHARE);
    let side = if climb { -1.0 } else { 1.0 };
    let mut out: Vec<(Vec<P>, bool)> = Vec::new();
    // Start where the pocket is deepest: the most room around the first cut.
    let (mut best, mut start) = (f64::MIN, [0.0, 0.0]);
    for iy in 0..ny {
        for ix in 0..nx {
            let p = at(ix, iy);
            let f = field.sample(p[0], p[1]);
            if f > best {
                best = f;
                start = p;
            }
        }
    }
    let mut p = start;
    let mut heading = 0.0f64;
    clear(&mut material, &mut left, p);
    // A helix down has already cleared the ring round the start.
    if helix > 0.0 {
        let n = ((std::f64::consts::TAU * helix / cell).ceil() as usize).max(8);
        for k in 0..n {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            clear(&mut material, &mut left, [p[0] + helix * a.cos(), p[1] + helix * a.sin()]);
        }
    }
    let mut path = vec![p];
    let mut stay_down = false;
    let floor = (disc.len() as f64 * 0.01).max(1.0) as usize;
    let mut guard = 0usize;
    while guard < 400_000 {
        guard += 1;
        // Turn from the cleared side towards the material until the bite reaches the load.
        let mut chosen: Option<(f64, P, usize)> = None;
        let mut fallback: Option<(f64, P, usize)> = None;
        for k in 0..=48 {
            let a = heading - side * 2.4 + side * 4.8 * k as f64 / 48.0;
            let q = [p[0] + step * a.cos(), p[1] + step * a.sin()];
            if !allowed(q) {
                continue;
            }
            let b = bite(&material, q);
            if (b as f64) <= target {
                if b >= floor {
                    chosen = Some((a, q, b));
                }
            } else {
                if fallback.is_none() {
                    fallback = Some((a, q, b));
                }
                break;
            }
        }
        if chosen.is_none() {
            // Hemmed in (all bites too big) only right at the start: take the smallest bite.
            if path.len() < 3 {
                chosen = fallback;
            }
        }
        if let Some((a, q, _)) = chosen {
            // Clear along the step, not only at its end.
            let n = ((step / cell).ceil() as usize).max(1);
            for k in 1..=n {
                let t = k as f64 / n as f64;
                clear(&mut material, &mut left, [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
            p = q;
            heading = a;
            path.push(p);
            continue;
        }
        // Nothing to take here: on to the nearest place where material starts.
        if !path.is_empty() {
            out.push((std::mem::take(&mut path), stay_down));
        }
        path.clear();
        if left < floor {
            break;
        }
        // Places where material starts, nearest first; of the nearest few, the first the cutter can
        // reach without lifting, else the nearest.
        let mut starts: Vec<(f64, P)> = Vec::new();
        for iy in (0..ny).step_by(2) {
            for ix in (0..nx).step_by(2) {
                let q = at(ix, iy);
                if !allowed(q) || material[iy * nx + ix] {
                    continue;
                }
                let b = bite(&material, q);
                if b >= floor && (b as f64) <= target * 2.0 {
                    starts.push(((q[0] - p[0]).hypot(q[1] - p[1]), q));
                }
            }
        }
        if starts.is_empty() {
            break;
        }
        starts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let way_clear = |q: P| {
            let d = (q[0] - p[0]).hypot(q[1] - p[1]);
            let n = ((d / cell).ceil() as usize).max(1);
            (0..=n).all(|k| {
                let t = k as f64 / n as f64;
                let m = [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
                allowed(m) && (bite(&material, m) as f64) <= target
            })
        };
        let nearest = starts[0];
        let found = starts.iter().take(40).take_while(|s| s.0 <= nearest.0 * 4.0 + tool * 2.0).find(|s| way_clear(s.1)).copied();
        let (d, q) = found.unwrap_or(nearest);
        stay_down = found.is_some();
        let n = ((d / cell).ceil() as usize).max(1);
        if stay_down {
            for k in 0..=n {
                let t = k as f64 / n as f64;
                clear(&mut material, &mut left, [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        // Head along the edge of the material, with it on the cutting side.
        let (fx, fy) = index(q);
        let (mut sx, mut sy) = (0.0, 0.0);
        for (dx, dy) in &disc {
            let (x, y) = (fx + dx, fy + dy);
            if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny && material[y as usize * nx + x as usize] {
                sx += *dx as f64;
                sy += *dy as f64;
            }
        }
        heading = sy.atan2(sx) - side * std::f64::consts::FRAC_PI_2;
        p = q;
        // Coming down here takes what's in reach, so the search always moves on.
        clear(&mut material, &mut left, p);
        path = vec![p];
    }
    if !path.is_empty() {
        out.push((path, stay_down));
    }
    out
}

/// Squared distances to the nearest marked cell, in cells (an exact Euclidean distance transform,
/// one pass along rows then one along columns).
fn squared_distances(marked: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    const FAR: f64 = 1e20;
    // The lower envelope of parabolas along one line.
    fn line(f: &[f64], out: &mut [f64]) {
        let n = f.len();
        let (mut v, mut z) = (vec![0usize; n], vec![0f64; n + 1]);
        let mut k = 0usize;
        z[0] = f64::NEG_INFINITY;
        z[1] = f64::INFINITY;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
                if s <= z[k] && k > 0 {
                    k -= 1;
                    continue;
                }
                if s <= z[k] {
                    v[0] = q;
                    z[1] = f64::INFINITY;
                    break;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = f64::INFINITY;
                break;
            }
        }
        k = 0;
        for (q, o) in out.iter_mut().enumerate() {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let d = q as f64 - v[k] as f64;
            *o = d * d + f[v[k]];
        }
    }
    let mut d: Vec<f64> = marked.iter().map(|m| if *m { 0.0 } else { FAR }).collect();
    let mut buf = vec![0.0; nx.max(ny)];
    for y in 0..ny {
        let row = d[y * nx..(y + 1) * nx].to_vec();
        line(&row, &mut buf[..nx]);
        d[y * nx..(y + 1) * nx].copy_from_slice(&buf[..nx]);
    }
    let mut col = vec![0.0; ny];
    for x in 0..nx {
        for y in 0..ny {
            col[y] = d[y * nx + x];
        }
        line(&col, &mut buf[..ny]);
        for y in 0..ny {
            d[y * nx + x] = buf[y];
        }
    }
    d
}

/// The same, after a larger tool has cleared all it could: `cleared` is its radius and how far
/// from the walls its centre may go. Whatever lies within its radius of a place its centre reaches
/// is taken as gone; what's left (corners and slots too narrow for it) is what this tool clears.
fn leftovers_after(field: &Field, tool: f64, centre_at: f64, allowance: f64, moves: &[Move], cleared: Option<(f64, f64)>) -> Vec<P> {
    let cell = field.cell.max(tool / 24.0);
    let (width, height) = ((field.nx - 1) as f64 * field.cell, (field.ny - 1) as f64 * field.cell);
    let (nx, ny) = ((width / cell) as usize + 1, (height / cell) as usize + 1);
    let at = |ix: usize, iy: usize| [field.x0 + ix as f64 * cell, field.y0 + iy as f64 * cell];
    // A touch of leeway, or the very edge of every pass would count as left behind.
    let reach = tool + cell * 0.15;
    let cells = (reach / cell).ceil() as i64 + 1;
    let disc: Vec<(i64, i64)> = (-cells..=cells).flat_map(|y| (-cells..=cells).map(move |x| (x, y))).filter(|(x, y)| ((x * x + y * y) as f64).sqrt() * cell <= reach + cell * 1.5).collect();
    let mut covered = vec![false; nx * ny];
    if let Some((big, big_at)) = cleared {
        let reached: Vec<bool> = (0..nx * ny).map(|i| {
            let p = at(i % nx, i / nx);
            field.sample(p[0], p[1]) >= big_at
        }).collect();
        if reached.iter().any(|r| *r) {
            let d = squared_distances(&reached, nx, ny);
            let limit = (big / cell) * (big / cell);
            for (c, dist) in covered.iter_mut().zip(&d) {
                *c = *dist <= limit;
            }
        }
    }
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

/// How far from a pass, in stepovers, a spot it missed may be for the pass to reach out to it on
/// the way round. Burin's choice.
pub const SPOT_DETOUR_STEPS: f64 = 2.0;

/// Cuts each spot from the pass at this depth that comes nearest it, out and back on the way round,
/// when it's within `reach` and the way keeps clear of the walls: corners a ring left are cleaned as
/// the ring goes by, rather than by crossing the pocket to them afterwards. The original cuts its
/// corners as part of each ring too. Returns the spots left for `cut_spots`.
fn tuck_spots(e: &mut Emit, spots: &[P], from: usize, z: f64, field: &Field, centre_at: f64, reach: f64) -> Vec<P> {
    let clear = |a: P, b: P| {
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / field.cell).ceil() as usize).max(1);
        (0..=steps).all(|k| {
            let t = k as f64 / steps as f64;
            field.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) >= centre_at - field.cell * 0.1
        })
    };
    // Spots in a row, as along a strip the passes missed, are cut in one stroke: those a tool's
    // width apart or less, with a clear way between, go together, in order from one end.
    let link = 2.0 * centre_at.max(field.cell);
    let mut group: Vec<usize> = (0..spots.len()).collect();
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    for i in 0..spots.len() {
        for j in i + 1..spots.len() {
            let (a, b) = (spots[i], spots[j]);
            if (a[0] - b[0]).hypot(a[1] - b[1]) <= link && clear(a, b) {
                let (ri, rj) = (root(&mut group, i), root(&mut group, j));
                group[ri] = rj;
            }
        }
    }
    let mut rows: std::collections::BTreeMap<usize, Vec<P>> = std::collections::BTreeMap::new();
    for i in 0..spots.len() {
        let r = root(&mut group, i);
        rows.entry(r).or_default().push(spots[i]);
    }
    // Each detour: the move it goes before, how far along that move it leaves, from where, and the
    // spots out and back.
    let mut detours: Vec<(usize, f64, P, Vec<P>)> = Vec::new();
    let mut missed = Vec::new();
    for (_, mut row) in rows {
        // From one end: the spot furthest from the first, then on to the nearest each time.
        let far_from = |p: P, row: &[P]| (0..row.len()).max_by(|a, b| (row[*a][0] - p[0]).hypot(row[*a][1] - p[1]).total_cmp(&(row[*b][0] - p[0]).hypot(row[*b][1] - p[1]))).unwrap_or(0);
        let first = far_from(row[0], &row);
        let mut order = vec![row.swap_remove(first)];
        while !row.is_empty() {
            let at = order[order.len() - 1];
            let next = (0..row.len()).min_by(|a, b| (row[*a][0] - at[0]).hypot(row[*a][1] - at[1]).total_cmp(&(row[*b][0] - at[0]).hypot(row[*b][1] - at[1]))).unwrap_or(0);
            order.push(row.swap_remove(next));
        }
        let joined = order.windows(2).all(|w| clear(w[0], w[1]));
        // The nearest point of a move cutting at this depth from a point at this depth, to any spot.
        let mut best: Option<(f64, usize, f64, P, usize)> = None;
        for i in from + 1..e.moves.len() {
            let (a, b) = (e.moves[i - 1].p, e.moves[i].p);
            if e.moves[i].rapid || (a[2] - z).abs() > 1e-9 || (b[2] - z).abs() > 1e-9 {
                continue;
            }
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            for (k, s) in order.iter().enumerate() {
                let t = if len2 < 1e-18 { 1.0 } else { (((s[0] - a[0]) * d[0] + (s[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) };
                let q = [a[0] + d[0] * t, a[1] + d[1] * t];
                let gap = (q[0] - s[0]).hypot(q[1] - s[1]);
                if best.is_none_or(|b| gap < b.0) {
                    best = Some((gap, i, t, q, k));
                }
            }
        }
        match best {
            Some((gap, i, t, q, k)) if joined && gap <= reach && clear(q, order[k]) => {
                // Out to the nearest spot, to one end, along to the other, and back.
                let mut way: Vec<P> = order[..=k].iter().rev().copied().collect();
                way.extend(order[1..].iter().copied());
                way.extend(order[k..order.len().saturating_sub(1)].iter().rev().copied());
                detours.push((i, t, q, way));
            }
            _ => missed.extend(order),
        }
    }
    // From the last, so the places found stay where they are; along one move, the furthest first.
    detours.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (i, _, q, way) in detours.into_iter().rev() {
        let at = |p: P| Move { p: [p[0], p[1], z], rapid: false, join: false };
        let mut moves = vec![at(q)];
        moves.extend(way.into_iter().map(at));
        moves.push(at(q));
        moves.dedup_by(|a, b| (a.p[0] - b.p[0]).hypot(a.p[1] - b.p[1]) < 1e-9);
        e.moves.splice(i..i, moves);
    }
    missed
}

/// Cuts at the spots, staying down between those with a clear way from one to the next.
fn cut_spots(e: &mut Emit, spots: &[P], z: f64, field: &Field, centre_at: f64) {
    if spots.is_empty() {
        return;
    }
    let mut left = spots.to_vec();
    let last = e.last();
    let mut at = [last[0], last[1]];
    // Already down in the pocket, when the rings have just been cut at this depth.
    let mut down = (last[2] - z).abs() < 1e-9;
    let clear = |a: P, b: P| {
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / field.cell).ceil() as usize).max(1);
        (0..=steps).all(|k| {
            let t = k as f64 / steps as f64;
            field.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) >= centre_at - field.cell * 0.1
        })
    };
    while !left.is_empty() {
        let far = |p: &P| (p[0] - at[0]).hypot(p[1] - at[1]);
        let i = (0..left.len()).min_by(|a, b| far(&left[*a]).total_cmp(&far(&left[*b]))).unwrap_or(0);
        let next = left.swap_remove(i);
        // Across the pocket's floor, cleared at this depth by the rings, without lifting: straight
        // when that keeps clear of the walls, otherwise round by the cleared middle. The original
        // cuts its corners as part of each ring, so it doesn't lift for them either; this way of
        // getting there is Burin's.
        let route = if !down {
            None
        } else if clear(at, next) {
            Some(vec![next])
        } else {
            safe_route(field, at, next, centre_at).map(|r| straighten(at, &r, &clear))
        };
        match route {
            Some(points) => {
                for q in points {
                    e.join(q[0], q[1], z);
                }
            }
            None => e.go_to(next[0], next[1], z),
        }
        at = next;
        down = true;
    }
    e.retract();
}

/// A way from `from` to `to` through the cells of the pocket at least `min` from every wall, by the
/// shortest chain of neighbouring cells, or None when there is none.
fn safe_route(field: &Field, from: P, to: P, min: f64) -> Option<Vec<P>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (nx, ny) = (field.nx, field.ny);
    let cell_of = |p: P| {
        let ix = ((p[0] - field.x0) / field.cell).round().clamp(0.0, (nx - 1) as f64) as usize;
        let iy = ((p[1] - field.y0) / field.cell).round().clamp(0.0, (ny - 1) as f64) as usize;
        iy * nx + ix
    };
    let centre = |i: usize| [field.x0 + (i % nx) as f64 * field.cell, field.y0 + (i / nx) as f64 * field.cell];
    let (start, goal) = (cell_of(from), cell_of(to));
    let open = |i: usize| i == start || i == goal || field.d[i] as f64 >= min;
    let g_end = centre(goal);
    let guess = |i: usize| {
        let c = centre(i);
        ((c[0] - g_end[0]).hypot(c[1] - g_end[1]) / field.cell) as f32
    };
    let mut cost = vec![f32::INFINITY; nx * ny];
    let mut came = vec![u32::MAX; nx * ny];
    let mut heap = BinaryHeap::new();
    cost[start] = 0.0;
    heap.push(Reverse((guess(start).to_bits(), start)));
    while let Some(Reverse((_, i))) = heap.pop() {
        if i == goal {
            let mut chain = vec![to];
            let mut k = came[i];
            while k != u32::MAX && k as usize != start {
                chain.push(centre(k as usize));
                k = came[k as usize];
            }
            chain.reverse();
            return Some(chain);
        }
        let (x, y) = ((i % nx) as i64, (i / nx) as i64);
        for (dx, dy, step) in [(1, 0, 1.0), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (1, 1, std::f32::consts::SQRT_2), (1, -1, std::f32::consts::SQRT_2), (-1, 1, std::f32::consts::SQRT_2), (-1, -1, std::f32::consts::SQRT_2)] {
            let (a, b) = (x + dx, y + dy);
            if a < 0 || b < 0 || a >= nx as i64 || b >= ny as i64 {
                continue;
            }
            let j = b as usize * nx + a as usize;
            if !open(j) {
                continue;
            }
            let c = cost[i] + step;
            if c < cost[j] {
                cost[j] = c;
                came[j] = i as u32;
                heap.push(Reverse(((c + guess(j)).to_bits(), j)));
            }
        }
    }
    None
}

/// The fewest corners of a chain that still keep each straight stretch where `clear` allows.
fn straighten(from: P, chain: &[P], clear: &dyn Fn(P, P) -> bool) -> Vec<P> {
    let mut out = Vec::new();
    let mut anchor = from;
    let mut i = 0;
    while i < chain.len() {
        // The furthest point still in a straight line of sight; the next one at least.
        let mut j = i;
        let mut k = chain.len() - 1;
        while k > i {
            if clear(anchor, chain[k]) {
                j = k;
                break;
            }
            k -= 1;
        }
        out.push(chain[j]);
        anchor = chain[j];
        i = j + 1;
    }
    out
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
        // Down fast to the plunge gap above the material, then drill from there, as the saved jobs
        // do (a fast move to 0.2 or 1 mm above the top before every hole); as `Emit::go_to` does.
        let near = e.z_at(x, y, 0.0) + e.m.plunge_gap;
        let gap = near < safe && near >= e.z_at(x, y, -op.start_depth);
        if gap {
            e.rapid([x, y, near]);
        }
        if op.peck && op.tool.pass_depth > 0.0 {
            let stops = peck_depths(op.start_depth, op.cut_depth, op.tool.pass_depth);
            for (i, z) in stops.iter().enumerate() {
                e.feed(x, y, *z);
                if i + 1 < stops.len() {
                    if op.retract_gap > 0.0 {
                        let lift = if op.retract_to_start { -op.start_depth + op.retract_gap } else { *z + op.retract_gap };
                        e.rapid([x, y, lift]);
                    } else {
                        // Out of the hole altogether, then down fast again.
                        e.rapid([x, y, safe]);
                        if gap {
                            e.rapid([x, y, near]);
                        }
                    }
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
                    cut_rows(e, scan_rows(lo, hi, a, step), field.cell, z, &[], &|x, y| field.sample(x, y) >= 0.0);
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

/// Below this angle a tool has no V to carve with.
pub const FLAT_TOOL_ANGLE: f64 = 1.0;

fn vcarve(e: &mut Emit, op: &Op) {
    let half = (op.tool.angle.to_radians() / 2.0).tan().max(1e-3);
    let Some(field) = Field::build(&op.paths, op.tool.dia, 900) else { return };
    let reach = op.tool.dia / 2.0;
    // A ball nose carves by its own profile, as a V-bit does by its angle (`mcBallNoseTool`).
    if op.kind == OpKind::VCarve && op.tool.kind == ToolKind::BallNose && op.clearance_tool.is_none() {
        profile_carve(e, op);
        return;
    }
    if op.tool.angle < FLAT_TOOL_ANGLE && op.kind == OpKind::VCarve {
        // An end mill can't carve a slope: it follows each shape once, its radius inside, at the
        // flat depth, as the saved jobs show (a 2 mm end mill: every cut 1.0 mm inside the letters,
        // at the flat depth, one trace per outline). Passes by the tool's pass depth are Burin's.
        let depth = if op.flat_depth > 0.0 { op.flat_depth } else { op.cut_depth };
        if depth <= 0.0 {
            return;
        }
        // Only the ring a radius inside, as in the saved jobs. Read in the original but not
        // followed: with a flat bottom it may add rings one stepover further in
        // (`0x1407be050`, levels `L + k * step`, counted by `0x1407c1230`), yet the saved jobs
        // clear nothing in the middle of big letters, so what stops it isn't known yet. Its
        // Voronoi middle-line pieces (`0x1407c06f0`) are for V-bits; the end mill's flat-tool flag
        // skips that walk (`0x1405697d0`).
        let mut rings: Vec<Vec<P>> = field.contours(reach).into_iter().map(|pts| oriented(pts, op.climb)).collect();
        // Not from the original: F-Engrave clears the flat bottom with a clean-up after the
        // carving (`Clean_Path_Calc`: the edge of the flat, then cuts every half diameter by
        // default). Here the same end mill goes on inwards, a ring every stepover, until nothing
        // is left, the way the original's rings function (`0x1407be050`) steps.
        // With a clearing tool, the end mill also cuts where that tool can't get in: rings at
        // L + k * stepover (k from 1) below L + the clearing tool's radius, each only along the
        // parts of the shape too narrow for a disc of that size (`0x1405697d0` with `local_468`,
        // drawn by `0x1407bdec0` in mode 2, edges filtered by `0x1407c05c0`).
        if let (Some(ct), true) = (&op.clearance_tool, op.flat_depth > 0.0) {
            let limit = reach + ct.dia / 2.0;
            let step = step_of(op).max(field.cell);
            let reached = middle_reach(&field, reach + step, limit);
            let mut level = reach + step;
            while level < limit {
                let mut band = field.clone();
                for (i, v) in band.d.iter_mut().enumerate() {
                    *v = (*v - level as f32).min(limit as f32 - reached[i]);
                }
                rings.extend(band.contours(0.0).into_iter().map(|pts| oriented(pts, op.climb)));
                level += step;
            }
        }
        if op.clear_flat && op.flat_depth > 0.0 {
            let step = step_of(op).max(field.cell);
            let mut level = reach + step;
            loop {
                let found = field.contours(level);
                if found.is_empty() {
                    break;
                }
                rings.extend(found.into_iter().map(|pts| oriented(pts, op.climb)));
                level += step;
            }
        }
        for d in passes_for(depth, op.tool.pass_depth, false) {
            let z = -(op.start_depth + d);
            for pts in &rings {
                e.go_to(pts[0][0], pts[0][1], z);
                walk(e, pts, true, z, op, None, None);
                e.retract();
            }
        }
        return;
    }
    if op.kind == OpKind::VCarve {
        profile_carve(e, op);
        return;
    }
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

/// A V-carve by the tool's own profile (a V-bit, or a ball nose), as the original makes it (`0x1405697d0`, each pass's
/// numbers from `0x1407c1230`): the full depth is the middle's distance from the outline over the
/// tangent of half the angle, with no limit (a 3.8 mm hole, 60 degree bit: 6.58 mm). Where that is
/// deeper than the pass depth, every whole pass depth above it is cleared first: a ring where the
/// V touches the outline at that depth (depth × tan(half angle) in), then rings a clearing step
/// further in each while any are left (`0x1407be050`), cut from the inside out in one go. The
/// last pass walks the middle line at its own depth (`0x1407bdc20`): in a round hole, a plunge
/// in the middle.
/// How far round a closed ring is.
fn ring_length(r: &[P]) -> f64 {
    (0..r.len()).map(|k| (r[(k + 1) % r.len()][0] - r[k][0]).hypot(r[(k + 1) % r.len()][1] - r[k][1])).sum()
}

fn profile_carve(e: &mut Emit, op: &Op) {
    // The grid's cells no wider than half the stepover: a large V-bit with a fine stepover (31.75
    // mm and 0.41 mm in the person's flat V-Carve test) would otherwise have rings closer than a
    // cell.
    let finest = (op.tool.dia / 64.0).min(op.tool.dia * op.tool.stepover / 2.0).max(op.tool.dia / 400.0);
    let Some(field) = Field::build_to(&op.paths, op.tool.dia, 900, finest) else { return };
    // The tool's profile: how far in from the outline its side touches at a depth (slot +0x38:
    // `0x1408995b0` for a V-bit, depth × tan(half the angle); `0x1408665f0` for a ball nose, the
    // ball's radius at that height, then the tool's radius below it), and how deep it goes where
    // the middle is a distance in (slot +0x40: `0x140899590`, distance / tan; `0x1408666f0`, from
    // the ball's profile, and no deeper than the ball's radius beyond it).
    let radius = op.tool.dia / 2.0;
    let half = (op.tool.angle.to_radians() / 2.0).tan().max(1e-3);
    let ball = op.tool.kind == ToolKind::BallNose;
    let offset_at = |d: f64| if ball { if d < radius { (2.0 * radius * d - d * d).max(0.0).sqrt() } else { radius } } else { d * half };
    let depth_for = |f: f64| if ball { if f < radius { radius - (radius * radius - f * f).max(0.0).sqrt() } else { radius } } else { f / half };
    // How deep it may go at all: the flat depth when asked for, and a ball's own radius.
    let cap = if op.flat_depth > 0.0 { op.flat_depth } else { f64::MAX }.min(if ball { radius } else { f64::MAX });
    let deepest_middle = field.d.iter().copied().fold(0f32, f32::max) as f64;
    let full = depth_for((deepest_middle - op.allowance).max(0.0)).min(cap);
    // Capped where the middle lies further in than the tool's side reaches at its full depth.
    let capped = deepest_middle - op.allowance > offset_at(full) + 1e-6;
    // Between passes the tool's clearing stepover (`mcVBitTool` slot +0xa8), else its usual one;
    // at the full depth, its usual stepover (`0x1407c1230`: the last pass takes field +0x90).
    let usual = (op.tool.dia * op.tool.stepover).max(field.cell);
    let clearing = if op.tool.clear_step > 0.0 { op.tool.clear_step.max(field.cell) } else { usual };
    let roots = |level: f64| -> Vec<Vec<P>> { field.contours(level).into_iter().filter(|l| l.len() >= 3 && encloses_higher(&field, l, level)).collect() };
    // Every whole pass depth above the full depth, then, where the tool can't reach the middle
    // (a flat depth, or a ball's radius), the full depth itself, cleared by rings.
    let pass = op.tool.pass_depth;
    let mut depths: Vec<f64> = Vec::new();
    let mut k = 1;
    while pass > 0.0 && k as f64 * pass < full - 1e-9 {
        depths.push(k as f64 * pass);
        k += 1;
    }
    let passes = depths.len();
    if capped {
        depths.push(full);
    }
    // How deep the walk goes at a point, and how near two of its pieces' ends must be to be one cut
    // (`0x1407b5fe0`, the tolerance times 10.0 at 0x142736370).
    let walk_z = |q: &P| -(op.start_depth + depth_for((field.sample(q[0], q[1]) - op.allowance).max(0.0)).min(cap));
    let join = toolpath_tolerance(op) * 10.0 + field.cell * 1.5;
    // Per outline: its walk's pieces, or the one plunge of a round hole.
    let mut walks: Vec<(Vec<Vec<P>>, Option<P>)> = Vec::new();
    // The last pass's walk, worked out before the rings: along the middle line, each point as deep as the tool reaches there (no
    // deeper than it may go). The outline, just inside, drawn onto the middle: its points climb
    // straight in until the distance from the outline stops growing.
    // With a flat depth and a clearing tool, the walk leaves out the pieces that run into the
    // outline's corners and keeps those between two flat parts (a neck narrower than the ring):
    // on the person's 80 cm plates the saved cuts rise only in two necks, 0.6 to 0.8 mm from the
    // outline, and on the 100 plate nowhere; the person's test without a clearing tool climbs
    // into every corner up to the surface.
    let no_corners = op.clearance_tool.is_some() && op.flat_depth > 0.0 && capped;
    // The outlines, then the holes in them: a neck between two holes of a letter (an 8's waist)
    // is only seen from the holes. Pieces already walked from another outline aren't walked again.
    let mut outlines_and_holes = roots(field.cell * 0.5);
    for ring in field.contours(field.cell * 0.5).into_iter().filter(|l| l.len() >= 3) {
        if !outlines_and_holes.iter().any(|o| o.len() == ring.len() && o[0] == ring[0]) {
            outlines_and_holes.push(ring);
        }
    }
    let seen_cell = field.cell * 1.5;
    let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let key = |q: &P| ((q[0] / seen_cell).floor() as i64, (q[1] / seen_cell).floor() as i64);
    for outline in outlines_and_holes {
        let middle = onto_middle_line(&field, &outline);
        let (lo, hi) = middle.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), q| ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]));
        if (hi[0] - lo[0]).max(hi[1] - lo[1]) <= field.cell * 2.0 {
            // A round hole's middle is a point: a plunge there.
            walks.push((Vec::new(), Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])));
            continue;
        }
        // Only the middle line's parts no further in than the full depth's offset (`0x1407bdc20`
        // keeps the pass's offset, times 1.0 at 0x142e6d670, as the walk's limit): wider parts are
        // the rings'. Uncapped, that is the whole line.
        let limit = offset_at(full) + op.allowance + field.cell * 0.5;
        let walked = |q: &P| field.sample(q[0], q[1]) <= limit;
        let mut pts = middle.clone();
        if let Some(out) = pts.iter().position(|q| !walked(q)) {
            pts.rotate_left(out);
        } else {
            pts.push(pts[0]);
        }
        let mut runs: Vec<Vec<P>> = vec![Vec::new()];
        for q in pts {
            if walked(&q) {
                runs.last_mut().unwrap().push(q);
            } else if !runs.last().unwrap().is_empty() {
                runs.push(Vec::new());
            }
        }
        // A corner's piece goes out to the outline and comes back where it left; a neck's runs
        // from one flat part to the other.
        let into_corner = |r: &Vec<P>| r.iter().any(|q| field.sample(q[0], q[1]) < field.cell * 2.0) && (r[0][0] - r[r.len() - 1][0]).hypot(r[0][1] - r[r.len() - 1][1]) < limit;
        runs.retain(|r| r.len() >= 2 && !(no_corners && into_corner(r)));
        // Most of a piece where pieces already go: walked already.
        runs.retain(|r| {
            let near = |q: &P| { let (i, j) = key(q); (-1..=1).any(|a| (-1..=1).any(|b| seen.contains(&(i + a, j + b)))) };
            r.iter().filter(|q| near(q)).count() * 5 < r.len() * 4
        });
        for r in &runs {
            for q in r {
                seen.insert(key(q));
            }
        }
        // Pieces whose ends are within ten times the tolerance are one cut (`0x1407b5fe0`, the
        // tolerance times 10.0 at 0x142736370): the tool goes on, without lifting.
        let close = |a: &P, b: &P| (a[0] - b[0]).hypot(a[1] - b[1]) <= join;
        let mut chains: Vec<Vec<P>> = Vec::new();
        while let Some(mut chain) = runs.pop() {
            loop {
                let end = *chain.last().unwrap();
                let Some((k, flip)) = runs.iter().enumerate().find_map(|(k, r)| {
                    if close(&end, &r[0]) {
                        Some((k, false))
                    } else if close(&end, r.last().unwrap()) {
                        Some((k, true))
                    } else {
                        None
                    }
                }) else {
                    let start = chain[0];
                    match runs.iter().position(|r| close(&start, r.last().unwrap()) || close(&start, &r[0])) {
                        Some(k) => {
                            let mut r = runs.swap_remove(k);
                            if close(&start, &r[0]) {
                                r.reverse();
                            }
                            r.extend(chain);
                            chain = r;
                            continue;
                        }
                        None => break,
                    }
                };
                let mut r = runs.swap_remove(k);
                if flip {
                    r.reverse();
                }
                chain.extend(r);
            }
            chains.push(chain);
        }
        walks.push((chains, None));
    }
    let last_depth = depths.len().saturating_sub(1);
    for (n, depth) in depths.into_iter().enumerate() {
        let pass_no = n;
        let step = if n < passes { clearing } else { usual };
        let z = -(op.start_depth + depth);
        let first = offset_at(depth) + op.allowance;
        // Every ring of this pass, by level, then each kept with the outline ring it lies in.
        let mut levels: Vec<Vec<Vec<P>>> = Vec::new();
        // At the flat depth with a clearing tool, which clears the flat itself: past the first
        // ring, rings only below the first ring's level and the clearing tool's radius, and only
        // where that tool can't get in, the middle line nearer than that (`0x1407c1230` gives the
        // limit L + the clearing tool's value at its +0x48, its radius; `0x1405697d0` draws the
        // rings with `0x1407bdec0` in mode 2, edges kept by `0x1407c05c0`), as for an end mill.
        let clearing_limit = op.clearance_tool.as_ref().filter(|_| n >= passes && op.flat_depth > 0.0).map(|ct| first + ct.dia / 2.0);
        let reached = clearing_limit.map(|limit| middle_reach(&field, first + step, limit));
        let mut level = first;
        loop {
            let found: Vec<Vec<P>> = match (clearing_limit, &reached) {
                (Some(limit), Some(reached)) if level > first => {
                    if level >= limit {
                        break;
                    }
                    let mut band = field.clone();
                    for (i, v) in band.d.iter_mut().enumerate() {
                        *v = (*v - level as f32).min(limit as f32 - reached[i]);
                    }
                    band.contours(0.0).into_iter().filter(|l| l.len() >= 3).collect()
                }
                _ => field.contours(level).into_iter().filter(|l| l.len() >= 3).collect(),
            };
            if found.is_empty() && clearing_limit.is_none() {
                break;
            }
            // Specks the grid leaves where a level just touches the middle (a ring shorter than
            // two steps round): the ring a step further out covers them. Burin's choice, as for
            // pockets; the original's exact offsets don't make them.
            let found: Vec<Vec<P>> = if level > first { found.into_iter().filter(|r| ring_length(r) >= 2.0 * step).collect() } else { found };
            levels.push(found);
            level += step;
        }
        let mut outlines = roots(first);
        // The first level's other rings go round holes in the shapes (a letter's inside): cut
        // with the outline they lie in, last before it.
        if let Some(found) = levels.first_mut() {
            found.retain(|r| !outlines.iter().any(|o| o.len() == r.len() && o[0] == r[0]));
        }
        while !outlines.is_empty() {
            let last = e.last();
            let near = |r: &Vec<P>| r.iter().map(|q| (q[0] - last[0]).hypot(q[1] - last[1])).fold(f64::MAX, f64::min);
            let i = (0..outlines.len()).min_by(|a, b| near(&outlines[*a]).total_cmp(&near(&outlines[*b]))).unwrap();
            let outline = outlines.swap_remove(i);
            let around = [Path::closed(outline.clone())];
            // From the inside out, each ring from its point nearest the tool, going on at depth
            // from the ring before when it is within a step.
            let mut group: Vec<Vec<P>> = Vec::new();
            for found in levels.iter_mut().rev() {
                let (inside, rest): (Vec<Vec<P>>, Vec<Vec<P>>) = found.drain(..).partition(|r| inside_even_odd(around.iter(), r[0]));
                *found = rest;
                group.extend(inside);
            }
            let inner = group.len();
            group.push(outline);
            let mut started = false;
            for (g, ring) in group.into_iter().enumerate() {
                let is_outline = g == inner;
                let at_full = pass_no == last_depth;
                let mut pts = oriented(ring, op.climb);
                let last = e.last();
                let far = |q: &P| (q[0] - last[0]).hypot(q[1] - last[1]);
                // From the place on the ring nearest the tool, between its points if need be: its
                // points are a grid cell apart, so the nearest point can be further than the ring.
                let here = [last[0], last[1]];
                let (seg, foot) = (0..pts.len())
                    .map(|k| {
                        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                        let t = (((here[0] - a[0]) * dx + (here[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
                        (k, [a[0] + dx * t, a[1] + dy * t])
                    })
                    .min_by(|x, y| far(&x.1).total_cmp(&far(&y.1)))
                    .unwrap_or((0, pts[0]));
                let n = pts.len();
                pts.rotate_left((seg + 1) % n);
                if (pts[pts.len() - 1][0] - foot[0]).hypot(pts[pts.len() - 1][1] - foot[1]) > 1e-9 && (pts[0][0] - foot[0]).hypot(pts[0][1] - foot[1]) > 1e-9 {
                    pts.insert(0, foot);
                } else if (pts[0][0] - foot[0]).hypot(pts[0][1] - foot[1]) > 1e-9 {
                    pts.rotate_right(1);
                }
                // Also on, at depth, when the straight way stays in the flat, where the V's sides
                // can't reach an outline: the original's sorter goes on when the way crosses no
                // ring (switch 0x142e67651); in a flat everything the way crosses is cleared to
                // that depth anyway. Not with a clearing tool: the person's plates then lift between
                // their rings (26 traces saved).
                let in_flat = |a: P, b: P| {
                    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                    let n = (len / (field.cell * 0.5)).ceil().max(1.0) as usize;
                    (0..=n).all(|i| {
                        let t = i as f64 / n as f64;
                        field.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) >= first - field.cell * 0.5
                    })
                };
                if started && (last[2] - z).abs() < 1e-9 && (far(&pts[0]) <= step * RING_LINK_STEPS + field.cell * 0.1 || (capped && pass_no == last_depth && op.clearance_tool.is_none() && in_flat([last[0], last[1]], pts[0]))) {
                    e.feed(pts[0][0], pts[0][1], z);
                } else {
                    e.go_to(pts[0][0], pts[0][1], z);
                }
                // At the full depth, the outline's ring takes in the walk's pieces that reach it
                // (out along the middle line into a corner and back), going on without lifting:
                // the original's ring and corner pieces come as one cut.
                let spliced: Vec<(usize, f64, P, Vec<P>)> = if is_outline && at_full && capped {
                    // The ring's piece nearest a point, how far along it, the foot and the distance.
                    let near_ring = |q: &P| {
                        (0..pts.len())
                            .map(|k| {
                                let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                                let t = (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
                                let f = [a[0] + dx * t, a[1] + dy * t];
                                (k, t, f, (f[0] - q[0]).hypot(f[1] - q[1]))
                            })
                            .min_by(|a, b| a.3.total_cmp(&b.3))
                            .filter(|x| x.3 <= join)
                    };
                    let mut taken = Vec::new();
                    for (chains, _) in walks.iter_mut() {
                        let mut k = 0;
                        while k < chains.len() {
                            let c = &chains[k];
                            match (near_ring(&c[0]), near_ring(c.last().unwrap())) {
                                (Some((at, t, f, _)), Some(_)) if c.len() >= 2 => taken.push((at, t, f, chains.swap_remove(k))),
                                _ => k += 1,
                            }
                        }
                    }
                    taken.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                    taken
                } else {
                    Vec::new()
                };
                if spliced.is_empty() {
                    walk(e, &pts, true, z, op, None, None);
                } else {
                    let mut next = 0;
                    for k in 0..=pts.len() {
                        let q = pts[k % pts.len()];
                        e.feed(q[0], q[1], z);
                        if k == pts.len() {
                            break;
                        }
                        while next < spliced.len() && spliced[next].0 == k {
                            let (_, _, foot, run) = &spliced[next];
                            e.feed(foot[0], foot[1], z);
                            for r in run {
                                e.feed(r[0], r[1], walk_z(r));
                            }
                            e.feed(foot[0], foot[1], z);
                            next += 1;
                        }
                    }
                }
                started = true;
            }
            e.retract();
        }
    }
    // What is left of the walk, outline by outline.
    for (mut chains, plunge) in walks {
        if let Some(c) = plunge {
            e.go_to(c[0], c[1], walk_z(&c));
            e.retract();
            continue;
        }
        if chains.is_empty() {
            continue;
        }
        // From the piece nearest the tool, then each nearest the last.
        while !chains.is_empty() {
            let l = e.last();
            let far = |q: &P| (q[0] - l[0]).hypot(q[1] - l[1]);
            let (k, flip) = chains
                .iter()
                .enumerate()
                .flat_map(|(k, c)| [(k, false, far(&c[0])), (k, true, far(c.last().unwrap()))])
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(k, f, _)| (k, f))
                .unwrap();
            let mut run = chains.swap_remove(k);
            if flip {
                run.reverse();
            }
            if l[2] < 0.0 && far(&run[0]) <= join {
                e.join(run[0][0], run[0][1], walk_z(&run[0]));
            } else {
                e.go_to(run[0][0], run[0][1], walk_z(&run[0]));
            }
            for q in run.iter().skip(1) {
                e.feed(q[0], q[1], walk_z(q));
            }
        }
        e.retract();
    }
}

/// For each grid point between `from` and `limit` from the outline: how far from the outline the
/// middle line is, straight inwards from the outline through that point (the radius of the
/// middle-line edge it belongs to in the original's Voronoi diagram). The distance grows by as
/// much as one moves inwards until the middle line is reached, where it stops growing. Points
/// outside that band get `limit` or more when deeper in, `from` minus one when nearer the edge.
fn middle_reach(field: &Field, from: f64, limit: f64) -> Vec<f32> {
    let (nx, cell) = (field.nx, field.cell);
    let mut out = vec![0f32; field.d.len()];
    for (i, o) in out.iter_mut().enumerate() {
        let v = field.d[i] as f64;
        if v >= limit {
            *o = v as f32;
            continue;
        }
        if v < from - 2.0 * cell {
            *o = (from - 1.0) as f32;
            continue;
        }
        let (x, y) = (field.x0 + (i % nx) as f64 * cell, field.y0 + (i / nx) as f64 * cell);
        let gx = field.sample(x + cell, y) - field.sample(x - cell, y);
        let gy = field.sample(x, y + cell) - field.sample(x, y - cell);
        let len = gx.hypot(gy);
        if len < 1e-9 {
            *o = v as f32;
            continue;
        }
        let (ux, uy) = (gx / len, gy / len);
        let tol = 0.35 * cell;
        let mut t = 0.0;
        let step = cell * 0.5;
        while v + t < limit {
            let next = field.sample(x + ux * (t + step), y + uy * (t + step));
            if next < v + t + step - tol {
                break;
            }
            t += step;
        }
        *o = (v + t) as f32;
    }
    out
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
        return Err(crate::i18n::tr("The tool's diameter must be more than zero.").into());
    }
    if op.tool.feed <= 0.0 || op.tool.plunge <= 0.0 {
        return Err(crate::i18n::tr("The tool's feed and plunge rates must be more than zero.").into());
    }
    if op.tool.rpm < 0.0 {
        return Err(crate::i18n::tr("The tool's spindle speed can't be below zero.").into());
    }
    if op.kind == OpKind::Pocket && (step_of(op) <= 0.0 || step_of(op) > op.tool.dia) {
        return Err(crate::i18n::tr("The stepover must be more than zero and no more than the tool's diameter.").into());
    }
    if op.kind == OpKind::Profile && op.tabs && op.tab_length <= 0.0 {
        return Err(crate::i18n::tr("The tabs need a length of more than zero.").into());
    }
    if op.kind.uses_model() {
        if model.is_none() {
            return Err(crate::i18n::tr("Create a 3D shape on the Modeling tab first.").into());
        }
    } else if op.paths.is_empty() && op.kind != OpKind::Texture {
        return Err(crate::i18n::tr("Select the shapes to cut in the 2D view first.").into());
    }
    let needs_closed = matches!(op.kind, OpKind::Pocket | OpKind::VCarve | OpKind::Prism);
    if needs_closed && !op.paths.iter().any(|p| p.closed) {
        return Err(crate::i18n::tr("This cut needs closed shapes. The selection only has open lines.").into());
    }
    if op.kind == OpKind::Moulding && op.paths.len() < 2 {
        return Err(crate::i18n::tr("Select the drive rail first, then the profile shape last.").into());
    }
    if op.tool.dia <= 0.0 {
        return Err(crate::i18n::tr("The tool needs a diameter above zero.").into());
    }
    if op.kind == OpKind::Chamfer {
        if !matches!(op.tool.kind, ToolKind::VBit | ToolKind::Engraving) {
            return Err(crate::i18n::tr("A chamfer is cut with a V-bit. Select one for this toolpath.").into());
        }
        if op.chamfer_width <= 0.0 {
            return Err(crate::i18n::tr("The chamfer needs a width above zero.").into());
        }
        if op.chamfer_width > op.tool.dia / 2.0 {
            return Err(tf!("This V-bit can cut a chamfer {} wide at most.", trim(op.tool.dia / 2.0)));
        }
    }
    if op.kind == OpKind::Flute && op.cut_depth <= op.start_depth {
        return Err(crate::i18n::tr("The flute depth must be deeper than the start depth.").into());
    }
    if op.kind == OpKind::Drill && op.cut_depth <= op.start_depth {
        return Err(crate::i18n::tr("Cut depth must be deeper than start depth.").into());
    }
    let top = model.map_or(0.0, |g| machine.model_top(job.thickness, g.max() as f64));
    let project = if op.project && !op.kind.uses_model() { model } else { None };
    let same_spot = if op.tool.metric { SAME_SPOT_MM } else { SAME_SPOT_MM / 25.4 };
    let mut e = Emit { moves: Vec::new(), m: machine, project, top, same_spot };
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
        return Err(tf!("No path fits: the {} is too wide for these shapes.", op.tool.label().to_lowercase()));
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
    let stop = Move { p: home, rapid: true, join: false };
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

/// The original's menu commands that switch plunge and retract drawing on and off
/// (handlers `0x1408ad5c0` and `0x1408ad630`).
pub const DRAW_PLUNGES_COMMAND: u32 = 32907;
pub const DRAW_RETRACTS_COMMAND: u32 = 32908;
/// Draw join moves (33776), purple in the original (`0x14051faf0`: type 14, colour `0x800080`).
pub const DRAW_JOINS_COMMAND: u32 = 33776;

/// The sums a toolpath's depths were typed as; empty for a plain number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Formulas {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_depth: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cut_depth: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub allowance: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub flat_depth: String,
    /// A texture's Max. Cut Length, Max. Cut Depth and Stepover (`_tpdLengthFormula`, `_tpdDepthFormula`,
    /// `_tpdStepoverFormula`, read from version 4 on by `0x1408658e0`). The original keeps each of those as a
    /// value with its smallest and largest share (`_Value`, `_MinRatio`, `_MaxRatio`, `0x140633da0`), and its form
    /// shows the value as the maximum, so the sums set Burin's maximums.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub texture_length: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub texture_depth: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stepover: String,
}

impl Formulas {
    fn texts(&self) -> [&String; 7] {
        [&self.start_depth, &self.cut_depth, &self.allowance, &self.flat_depth, &self.texture_length, &self.texture_depth, &self.stepover]
    }

    pub fn is_empty(&self) -> bool {
        self.texts().iter().all(|t| t.is_empty())
    }

    /// Works each one out again, setting the value it stands for. True if any value changed; the first
    /// sum that can't be worked out is returned instead, and nothing is changed.
    pub fn apply(&self, op: &mut Op, context: &crate::formula::Context) -> Result<bool, crate::formula::FormulaError> {
        let old = [op.start_depth, op.cut_depth, op.allowance, op.flat_depth, op.tex_length[1], op.tex_cut[1], op.stepover];
        let mut values = old;
        for (text, value) in self.texts().into_iter().zip(values.iter_mut()) {
            if !text.is_empty() {
                *value = crate::formula::evaluate(text, context)?;
            }
        }
        [op.start_depth, op.cut_depth, op.allowance, op.flat_depth, op.tex_length[1], op.tex_cut[1], op.stepover] = values;
        Ok(values != old)
    }

    /// Whether any of them uses this variable.
    pub fn uses(&self, name: &str) -> bool {
        self.texts().iter().any(|t| crate::formula::uses(t, name))
    }
}

/// What a move does, for drawing: the original draws each kind in its own colour (`0x14051fb40`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Cut = 0,
    Rapid = 1,
    /// Cutting straight down.
    Plunge = 2,
    /// Fast, straight up.
    Retract = 3,
    /// At cutting speed from one cut to the next without lifting.
    Join = 4,
}

pub fn move_kind(from: [f64; 3], m: &Move) -> MoveKind {
    let still = (m.p[0] - from[0]).hypot(m.p[1] - from[1]) < 1e-9;
    if m.join && !m.rapid {
        return MoveKind::Join;
    }
    match (m.rapid, still) {
        (true, true) if m.p[2] > from[2] => MoveKind::Retract,
        (true, _) => MoveKind::Rapid,
        (false, true) if m.p[2] < from[2] => MoveKind::Plunge,
        (false, _) => MoveKind::Cut,
    }
}

/// One stretch of cutting: where the tool arrives by fast move, then everything until it next moves fast.
#[derive(Clone, Debug)]
pub struct Cut {
    pub toolpath: usize,
    pub number: usize,
    pub moves: Vec<Move>,
}

/// The stretches of cutting in a toolpath, in order.
pub fn cuts_of(moves: &[Move], toolpath: usize) -> Vec<Cut> {
    let mut cuts: Vec<Cut> = Vec::new();
    let mut open = false;
    let mut last: Option<[f64; 3]> = None;
    for m in working_moves(moves) {
        if m.rapid {
            open = false;
        } else {
            if !open {
                let number = cuts.len();
                cuts.push(Cut { toolpath, number, moves: vec![Move { p: last.unwrap_or(m.p), rapid: true, join: false }] });
                open = true;
            }
            if let Some(cut) = cuts.last_mut() {
                cut.moves.push(*m);
            }
        }
        last = Some(m.p);
    }
    cuts
}

/// A cut seen from above, for ordering: once round if it comes back to where it began.
fn seen_from_above(cut: &Cut, same_spot: f64) -> (Vec<P>, bool) {
    let mut pts: Vec<P> = Vec::with_capacity(cut.moves.len());
    for m in &cut.moves[1..] {
        let p = [m.p[0], m.p[1]];
        if pts.last().is_none_or(|l| (l[0] - p[0]).hypot(l[1] - p[1]) >= same_spot) {
            pts.push(p);
        }
    }
    if pts.is_empty() {
        pts.push([cut.moves[0].p[0], cut.moves[0].p[1]]);
    }
    let start = pts[0];
    match (3..pts.len()).find(|k| (pts[*k][0] - start[0]).hypot(pts[*k][1] - start[1]) < same_spot) {
        Some(k) => {
            pts.truncate(k);
            (pts, true)
        }
        None => (pts, false),
    }
}

/// Whether a part comes before another when no order was asked for: at the first toolpath they both
/// have cuts in, the earlier cut goes first; with none in common, the part in the earlier toolpath.
fn part_before(a: &[i64], b: &[i64]) -> bool {
    if let Some(k) = (0..a.len()).find(|k| a[*k] >= 0 && b[*k] >= 0) {
        return a[k] < b[k];
    }
    for k in 0..a.len() {
        if a[k] < 0 && b[k] >= 0 {
            return false;
        }
        if b[k] < 0 && a[k] >= 0 {
            return true;
        }
    }
    false
}

/// The order to make the cuts of several toolpaths in. With `by_part`, what lies inside a closed cut
/// stays with it, and within a part the toolpaths keep their order.
fn merge_order(cuts: &[Cut], toolpaths: usize, choices: OrderChoices, by_part: bool, from: [f64; 3], same_spot: f64) -> Vec<usize> {
    let shapes: Vec<(Vec<P>, bool)> = cuts.iter().map(|c| seen_from_above(c, same_spot)).collect();
    let items: Vec<Placed> = shapes.iter().map(placed).collect();
    let spot = [from[0], from[1]];
    if !by_part {
        let all: Vec<usize> = (0..cuts.len()).collect();
        return order_level(&items, &all, choices, spot);
    }
    let parent = holders(&shapes, &items);
    let top_of = |mut i: usize| {
        while let Some(p) = parent[i] {
            i = p;
        }
        i
    };
    let tops: Vec<usize> = (0..cuts.len()).filter(|i| parent[*i].is_none()).collect();
    let mut parts: Vec<Vec<usize>> = order_level(&items, &tops, choices, spot)
        .into_iter()
        .map(|t| {
            let mut part: Vec<usize> = (0..cuts.len()).filter(|i| top_of(*i) == t).collect();
            part.sort_by_key(|i| (cuts[*i].toolpath, cuts[*i].number));
            part
        })
        .collect();
    let none_chosen = !(choices.left_to_right || choices.bottom_to_top || choices.grid || choices.shortest);
    if none_chosen {
        let row = |part: &Vec<usize>| {
            let mut r = vec![-1i64; toolpaths];
            for i in part {
                let t = cuts[*i].toolpath;
                let n = cuts[*i].number as i64;
                if r[t] < 0 || r[t] > n {
                    r[t] = n;
                }
            }
            r
        };
        // One at a time: these comparisons don't make a strict ranking when parts share no toolpath.
        let mut sorted: Vec<(Vec<i64>, Vec<usize>)> = Vec::with_capacity(parts.len());
        for part in parts {
            let r = row(&part);
            let at = sorted.iter().position(|(other, _)| part_before(&r, other)).unwrap_or(sorted.len());
            sorted.insert(at, (r, part));
        }
        parts = sorted.into_iter().map(|(_, part)| part).collect();
    }
    parts.into_iter().flatten().collect()
}

/// The moves of several toolpaths as one, joined the way the tool gets to any other cut.
pub fn merged_moves(lists: &[&[Move]], choices: OrderChoices, by_part: bool, machine: Machine, metric: bool) -> Vec<Move> {
    let same_spot = if metric { SAME_SPOT_MM } else { SAME_SPOT_MM / 25.4 };
    let cuts: Vec<Cut> = lists.iter().enumerate().flat_map(|(t, moves)| cuts_of(moves, t)).collect();
    let order = merge_order(&cuts, lists.len(), choices, by_part, machine.home, same_spot);
    join_cuts(order.iter().map(|i| &cuts[*i]), machine, same_spot)
}

/// A toolpath's moves from one of its cuts on (counted from 0), reached the way any cut is.
pub fn moves_from_cut(moves: &[Move], first: usize, machine: Machine, metric: bool) -> Vec<Move> {
    let same_spot = if metric { SAME_SPOT_MM } else { SAME_SPOT_MM / 25.4 };
    let cuts = cuts_of(moves, 0);
    join_cuts(cuts.iter().skip(first), machine, same_spot)
}

/// Cuts one after another, from home and back, with the usual way between them.
pub fn join_cuts<'a>(cuts: impl IntoIterator<Item = &'a Cut>, machine: Machine, same_spot: f64) -> Vec<Move> {
    let safe = machine.safe_z;
    let mut out = vec![Move { p: machine.home, rapid: true, join: false }];
    let mut at = machine.home;
    for cut in cuts {
        let entry = cut.moves[0].p;
        let apart = (at[0] - entry[0]).powi(2) + (at[1] - entry[1]).powi(2) + (at[2] - entry[2]).powi(2);
        if apart >= 0.00000001 {
            if at[2] < safe && (at[0] - entry[0]).hypot(at[1] - entry[1]) < same_spot {
                out.push(Move { p: entry, rapid: false, join: false });
            } else {
                if at[2] < safe {
                    out.push(Move { p: [at[0], at[1], safe], rapid: true, join: false });
                }
                out.push(Move { p: [entry[0], entry[1], safe], rapid: true, join: false });
                if entry[2] != safe {
                    out.push(Move { p: entry, rapid: true, join: false });
                }
            }
        }
        out.extend_from_slice(&cut.moves[1..]);
        at = cut.moves[cut.moves.len() - 1].p;
    }
    if at[2] < safe {
        out.push(Move { p: [at[0], at[1], safe], rapid: true, join: false });
    }
    out.push(Move { p: machine.home, rapid: true, join: false });
    out
}

/// The original's usual allowance: time spent cutting and lifting counts this many times over.
pub const TIME_ALLOWANCE: f64 = 2.0;
/// The speed of fast moves it assumes unless told otherwise, in millimetres a minute (100 a second).
pub const RAPID_RATE_MM: f64 = 6000.0;

/// Estimated machining time, in seconds: cuts, plunges and lifts with the allowance, then fast moves
/// and pauses as they are. Lifts go at the fast rate.
pub fn estimate_seconds(moves: &[Move], tool: &Tool, rapid_rate: f64, dwell: f64) -> f64 {
    estimate_with(moves, tool, rapid_rate, dwell, None)
}

/// The same, with the moves near tight corners at their lower rate (`slow_corners`).
pub fn estimate_with(moves: &[Move], tool: &Tool, rapid_rate: f64, dwell: f64, slow: Option<&SlowMoves>) -> f64 {
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
            working += minutes(if flat < 1e-9 && rise < 0.0 { tool.plunge } else { tool.feed * SlowMoves::share_at(slow, b) });
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
    /// When set, written as `G64 P` at the top: how far LinuxCNC may round a corner to keep its
    /// speed. Without it, LinuxCNC rounds as far as it likes (its G-code reference, G64).
    pub blend: Option<f64>,
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
    /// The toolpath's own notes, for `[TOOLPATH_NOTES]`.
    pub notes: &'a str,
    /// Moves near a tight corner to cut more slowly, and the share of the cutting rate they get.
    pub slow: Option<&'a SlowMoves>,
}

/// Slowing down in tight corners, a practice of other CAM programs (feed optimisation in Fusion 360
/// and HSM, for one): where the path turns sharply the cutter takes a wider bite, so the rate
/// drops near the corner. Not something the original does; off unless asked for.
pub const CORNER_TURN_DEGREES: f64 = 45.0;
/// The share of the cutting rate near a tight corner, at first.
pub const CORNER_RATE_SHARE: f64 = 0.5;

/// The corner tolerance offered for LinuxCNC's `G64 P`, in millimetres: what the person's own
/// LinuxCNC files use (`G64 P0.03`). Not from the original.
pub const BLEND_TOLERANCE_MM: f64 = 0.03;

/// Whether the built-in output with this index is plain G-code that LinuxCNC reads: GRBL and
/// Mach3 don't take `G64 P`.
pub fn post_takes_blend(index: usize) -> bool {
    POSTS.get(index).is_some_and(|p| p.0.starts_with("Generic"))
}
/// How far before and after the corner the rate stays down, in tool diameters.
pub const CORNER_REACH: f64 = 1.0;

/// The end points of the moves that cut slowly, and their share of the cutting rate.
#[derive(Clone, Debug, Default)]
pub struct SlowMoves {
    pub share: f64,
    pub ends: std::collections::HashSet<[u64; 3]>,
}

impl SlowMoves {
    fn key(p: [f64; 3]) -> [u64; 3] {
        [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
    }

    /// The cutting rate's share for a move ending here.
    pub fn share_at(slow: Option<&SlowMoves>, p: [f64; 3]) -> f64 {
        slow.filter(|s| s.ends.contains(&Self::key(p))).map_or(1.0, |s| s.share)
    }
}

/// Splits the cutting moves near each tight corner so they can go slower. A corner is tight where
/// the way in over one tool radius and the way out over one tool radius differ by more than
/// `CORNER_TURN_DEGREES`, which also catches small curves drawn as many short moves. Only level
/// cuts count; ramps and plunges keep their own rate.
pub fn slow_corners(moves: &[Move], dia: f64, share: f64, reach: f64) -> (Vec<Move>, SlowMoves) {
    let mut out: Vec<Move> = Vec::with_capacity(moves.len());
    let mut slow = SlowMoves { share, ends: Default::default() };
    let level = |a: [f64; 3], b: &Move| !b.rapid && (b.p[0] - a[0]).hypot(b.p[1] - a[1]) >= (b.p[2] - a[2]).abs() * 4.0 && (b.p[0] - a[0]).hypot(b.p[1] - a[1]) > 1e-9;
    let mut i = 0;
    while i < moves.len() {
        // A run of level cuts from moves[i - 1] (its start point) through moves[j - 1].
        if i == 0 || !level(moves[i - 1].p, &moves[i]) {
            out.push(moves[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < moves.len() && level(moves[j - 1].p, &moves[j]) {
            j += 1;
        }
        let pts: Vec<[f64; 3]> = moves[i - 1..j].iter().map(|m| m.p).collect();
        let mut along = vec![0.0];
        for w in pts.windows(2) {
            along.push(along.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
        }
        let at = |d: f64| {
            let d = d.clamp(0.0, *along.last().unwrap());
            let k = along.partition_point(|a| *a < d).clamp(1, pts.len() - 1);
            let t = (d - along[k - 1]) / (along[k] - along[k - 1]).max(1e-12);
            [pts[k - 1][0] + (pts[k][0] - pts[k - 1][0]) * t, pts[k - 1][1] + (pts[k][1] - pts[k - 1][1]) * t]
        };
        let r = dia / 2.0;
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for k in 1..pts.len() - 1 {
            let (a, v, b) = (at(along[k] - r), pts[k], at(along[k] + r));
            let (u, w) = ([v[0] - a[0], v[1] - a[1]], [b[0] - v[0], b[1] - v[1]]);
            let (lu, lw) = (u[0].hypot(u[1]), w[0].hypot(w[1]));
            if lu < 1e-9 || lw < 1e-9 {
                continue;
            }
            let cos = ((u[0] * w[0] + u[1] * w[1]) / (lu * lw)).clamp(-1.0, 1.0);
            if cos.acos().to_degrees() > CORNER_TURN_DEGREES {
                let span = (along[k] - reach * dia, along[k] + reach * dia);
                match spans.last_mut() {
                    Some(last) if span.0 <= last.1 => last.1 = span.1,
                    _ => spans.push(span),
                }
            }
        }
        if spans.is_empty() {
            out.extend_from_slice(&moves[i..j]);
            i = j;
            continue;
        }
        let inside = |d: f64| spans.iter().any(|s| d > s.0 && d < s.1);
        for k in 1..pts.len() {
            let (d0, d1) = (along[k - 1], along[k]);
            // Cut this move where a slow span starts or ends inside it.
            let mut cuts: Vec<f64> = spans.iter().flat_map(|s| [s.0, s.1]).filter(|c| *c > d0 + 1e-9 && *c < d1 - 1e-9).collect();
            cuts.sort_by(f64::total_cmp);
            cuts.push(d1);
            let mut from = d0;
            let orig = moves[i - 1 + k];
            for c in cuts {
                let t = (c - d0) / (d1 - d0).max(1e-12);
                let p = if c >= d1 { orig.p } else { [pts[k - 1][0] + (pts[k][0] - pts[k - 1][0]) * t, pts[k - 1][1] + (pts[k][1] - pts[k - 1][1]) * t, pts[k - 1][2] + (pts[k][2] - pts[k - 1][2]) * t] };
                let m = Move { p, ..orig };
                if inside((from + c) / 2.0) {
                    slow.ends.insert(SlowMoves::key(p));
                }
                out.push(m);
                from = c;
            }
        }
        i = j;
    }
    (out, slow)
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

/// A name as it can stand inside a G-code comment: LinuxCNC refuses a file with one comment inside
/// another, and a tool's name often has brackets of its own.
fn in_comment(name: &str) -> String {
    name.chars().map(|c| match c { '(' => '[', ')' => ']', '\n' | '\r' => ' ', c => c }).collect()
}

/// Plain G-code for a list of cutting paths.
pub fn gcode(paths: &[PostPath], post: &PostSettings) -> String {
    let mut s = String::new();
    let k = post.scale;
    let _ = writeln!(s, "(Written by Burin)");
    let _ = writeln!(s, "{} G90 G17", if post.inches { "G20" } else { "G21" });
    if let Some(p) = post.blend {
        let _ = writeln!(s, "G64 P{p:.4}");
    }
    let mut current: Option<&Tool> = None;
    for path in paths {
        let (tool, moves) = (path.tool, path.moves);
        let _ = writeln!(s, "(Toolpath: {})", in_comment(path.name));
        if current != Some(tool) {
            let name = in_comment(&tool.name);
            let _ = writeln!(s, "(Tool: {name})");
            let _ = writeln!(s, "M5\nM0 (Fit tool: {name})\nS{:.0} M3", tool.rpm);
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
                let wanted = if flat < (m.p[2] - prev[2]).abs() { tool.plunge } else { tool.feed * SlowMoves::share_at(path.slow, m.p) } * k;
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
        tab_spots: Vec::new(),
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
        adaptive: false,
        raster_angle: 0.0,
        profile_pass: ProfilePass::Last,
        raster_allowance: 0.0,
        raster_second: false,
        clear_flat: false,
        last_thickness: 0.0,
        keep_exact: false,
        zigzag_open: false,
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
        merged: false,
        imported: false,
        notes: String::new(),
        formulas: Formulas::default(),
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

    /// Each hole is reached by a fast move down to the plunge gap above the top, as in the saved
    /// jobs; only from there does the tool drill at its plunge rate.
    #[test]
    fn drill_comes_down_fast_to_the_plunge_gap() {
        let m = Machine { safe_z: 1.0, plunge_gap: 0.05, ..M };
        // With a retract gap the tool lifts only that much between pecks; without one it leaves the hole.
        for (peck, retract) in [(false, 0.1), (true, 0.1), (true, 0.0)] {
            let mut o = op(OpKind::Drill, vec![Path::closed(circle([2.0, 3.0], 0.2)), Path::closed(circle([7.0, 8.0], 0.2))]);
            o.peck = peck;
            o.retract_gap = retract;
            o.tool.pass_depth = 0.2;
            let moves = generate(&o, m, &J, None).unwrap();
            // No hole is drilled at the plunge rate from the safe height.
            assert!(moves.windows(2).all(|w| w[1].rapid || w[0].p[2] < 1.0 - 1e-9), "peck {peck}: a slow move from the safe height");
            assert!(moves.windows(2).any(|w| w[0].rapid && !w[1].rapid && (w[0].p[2] - 0.05).abs() < 1e-9), "peck {peck}: drills from the plunge gap");
        }
    }

    /// An end mill in a V-Carve toolpath follows each shape once, its radius inside, at the flat
    /// depth, as in the saved jobs; it doesn't sweep the whole shape with rings.
    #[test]
    fn vcarve_with_an_end_mill_follows_the_outline_once_at_the_flat_depth() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(rect([5.0, 5.0], 2.0, 1.0, 0.0))]);
        assert!(o.tool.angle < FLAT_TOOL_ANGLE);
        o.flat_depth = 0.1;
        let m = generate(&o, M, &J, None).unwrap();
        let cut: Vec<[f64; 3]> = m.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect();
        assert!(!cut.is_empty());
        assert!(cut.iter().all(|p| (p[2] + 0.1).abs() < 1e-9), "all at the flat depth");
        let r = o.tool.dia / 2.0;
        // On the ring one radius inside the rectangle.
        assert!(cut.iter().all(|p| {
            let (dx, dy) = ((p[0] - 5.0).abs(), (p[1] - 5.0).abs());
            let on_side = ((dx - (1.0 - r)).abs() < 0.01 && dy <= 0.5 - r + 0.01) || ((dy - (0.5 - r)).abs() < 0.01 && dx <= 1.0 - r + 0.01);
            on_side
        }), "on the ring a radius inside");
        assert_eq!(m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count(), 1, "one trace, one lift");
    }

    /// With a clearing tool, the end mill adds a ring one stepover further in only where the shape
    /// is too narrow for that tool: none in a wide disc, one in a narrow strip.
    #[test]
    fn vcarve_end_mill_adds_rings_only_where_the_clearing_tool_cannot_go() {
        let lifts = |paths: Vec<Path>| {
            let mut o = op(OpKind::VCarve, paths);
            o.flat_depth = 0.1;
            let mut ct = o.tool.clone();
            ct.dia = o.tool.dia * 1.6;
            o.clearance_tool = Some(ct);
            generate(&o, M, &J, None).unwrap().windows(2).filter(|w| w[1].rapid && !w[0].rapid).count()
        };
        let o = op(OpKind::VCarve, vec![]);
        let (r, step) = (o.tool.dia / 2.0, o.tool.dia * o.tool.stepover);
        // Wide and round: the middle is far from every point, so the clearing tool reaches
        // everywhere past the first ring. (Corners are different: next to one the middle line is
        // near, so the original cuts a small loop there too, as its corner edges aren't skipped,
        // `0x1407c9b90` returning false.)
        assert_eq!(lifts(vec![Path::closed(crate::geom::circle([5.0, 5.0], 3.0))]), 1);
        // A strip whose half width lies between L + step and L + the clearing radius.
        let half = r + step + 0.3 * (o.tool.dia * 0.8 - step);
        assert_eq!(lifts(vec![Path::closed(rect([5.0, 5.0], 2.0 * half, 6.0, 0.0))]), 2);
    }

    /// Asked for, the end mill clears the whole flat inside its first ring, a ring every stepover;
    /// left off, it cuts the one ring as the original does.
    #[test]
    fn vcarve_with_an_end_mill_clears_the_flat_when_asked() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.flat_depth = 0.1;
        let lifts = |o: &Op| generate(o, M, &J, None).unwrap().windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        assert_eq!(lifts(&o), 1);
        o.clear_flat = true;
        let moves = generate(&o, M, &J, None).unwrap();
        let rings = moves.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        let step = o.tool.dia * o.tool.stepover;
        let expected = ((2.0 - o.tool.dia / 2.0) / step).ceil() as usize;
        assert!(rings >= expected.max(2) - 1 && rings <= expected + 1, "{rings} rings, about {expected}");
        // The innermost cut comes within a stepover of the middle.
        let nearest = moves.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| (m.p[0] - 5.0).abs().max((m.p[1] - 5.0).abs())).fold(f64::MAX, f64::min);
        assert!(nearest <= step + 1e-6, "{nearest}");
    }

    /// A lead also stops short of the toolpath's other loops (`0x140842ba0`): coming in from
    /// outside a square, it is cut where a neighbouring loop lies across its way.
    #[test]
    fn a_lead_stops_short_of_another_loop_of_the_toolpath() {
        let mut o = op(OpKind::Profile, vec![]);
        o.lead = true;
        o.lead_distance = 4.0;
        o.lead_circular = false;
        o.lead_angle = 90.0;
        let own = crate::geom::rect([0.0, 0.0], 4.0, 4.0, 0.0);
        let own = if signed_area(&own) > 0.0 { own } else { own.into_iter().rev().collect() };
        let side_sign = away_from_part(&own, true, Side::Outside);
        let alone = fitted_lead(&own, &[], Side::Outside, side_sign, &o);
        assert_eq!(alone, Some(4.0));
        // Where the lead goes: the far end of a lead at the loop's start.
        let start = own[0];
        let next = own[1];
        let len = (next[0] - start[0]).hypot(next[1] - start[1]);
        let t = [(next[0] - start[0]) / len, (next[1] - start[1]) / len];
        let far = lead_points_for(start, t, side_sign, &o, false, 4.0)[0];
        // A small loop around the point 2.5 mm out along the lead: the 6th point (2.22) is still
        // short of it, the 7th (2.67) inside, so 5 tenths are kept.
        let mid = [start[0] + (far[0] - start[0]) * 0.625, start[1] + (far[1] - start[1]) * 0.625];
        let neighbour = crate::geom::circle(mid, 0.2);
        assert_eq!(fitted_lead(&own, &[&neighbour], Side::Outside, side_sign, &o), Some(2.0));
    }

    /// A hole exactly as wide as the tool is still cut by a pocket, as the original does (a tiny
    /// circle round its middle, from a saved test job), though no ring a radius in fits.
    #[test]
    fn a_pocket_cuts_a_hole_just_as_wide_as_the_tool() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(crate::geom::circle([5.0, 5.0], 0.0))]);
        let r = o.tool.dia / 2.0;
        o.paths = vec![Path::closed(crate::geom::circle([5.0, 5.0], r))];
        let cuts: Vec<[f64; 3]> = generate(&o, M, &J, None).unwrap().iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect();
        assert!(!cuts.is_empty(), "the hole is cut");
        assert!(cuts.iter().all(|p| (p[0] - 5.0).hypot(p[1] - 5.0) < r * 0.05), "in its middle");
        // A hole a little narrower than the tool isn't: nothing fits.
        o.paths = vec![Path::closed(crate::geom::circle([5.0, 5.0], r * 0.9))];
        assert!(generate(&o, M, &J, None).is_err());
    }

    /// A round pocket whose last ring leaves its middle uncut, more than the tool's radius past the
    /// ring, is cleared by a plunge in the middle once all the rings are cut, as the saved
    /// connector pockets have (a 25 mm hole, a 3.175 mm tool, rings 2.96 mm apart: the rings
    /// from the inside out in one go, then a lift and a plunge in the middle).
    #[test]
    fn a_round_pocket_ends_with_a_plunge_in_its_middle() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(crate::geom::circle([20.0, 20.0], 12.5))]);
        o.tool.dia = 3.175;
        o.stepover = 2.96;
        o.cut_depth = 3.0;
        o.tool.pass_depth = 3.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let cuts: Vec<&Move> = moves.iter().filter(|m| m.p[2] < 0.0).collect();
        let last = cuts.last().unwrap();
        assert!((last.p[0] - 20.0).hypot(last.p[1] - 20.0) < 0.1, "it ends in the middle: {:?}", last.p);
        // Reached by a lift: the move down into the middle comes straight from above it.
        let down = moves.iter().position(|m| std::ptr::eq(m, *cuts.iter().rev().find(|m| (m.p[0] - 20.0).hypot(m.p[1] - 20.0) < 0.1 && !m.rapid).unwrap())).unwrap();
        assert!(moves[..down].iter().rev().take_while(|m| !m.rapid).all(|m| (m.p[0] - 20.0).hypot(m.p[1] - 20.0) < 0.1), "a plunge of its own");
        // The rings come first and cut nothing near the middle.
        let first = cuts.first().unwrap();
        assert!((first.p[0] - 20.0).hypot(first.p[1] - 20.0) > 1.0, "a ring first: {:?}", first.p);
    }

    /// Where a band widens in a bend, too narrow there for a second ring but wider than the first
    /// one clears, the original runs along its middle, out and back; a pocket that's only a
    /// ring's width everywhere gets nothing more.
    #[test]
    fn a_wider_bend_in_a_band_gets_a_stroke_along_its_middle() {
        let r = 3.175 / 2.0;
        let plain = |pts: Vec<P>| {
            let mut o = op(OpKind::Pocket, vec![Path::closed(pts)]);
            o.tool.dia = 3.175;
            o.stepover = 2.96;
            o.cut_depth = 3.0;
            o.tool.pass_depth = 3.0;
            generate(&o, M, &J, None).unwrap().iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect::<Vec<_>>()
        };
        // A strip 6 mm wide: one ring 1.59 mm in clears it all, and nothing runs along the middle.
        let strip = plain(vec![[0.0, 0.0], [40.0, 0.0], [40.0, 6.0], [0.0, 6.0]]);
        assert!(strip.iter().all(|p| (p[1] - 3.0).abs() > 1.0), "only the ring");
        // A square bulge 7 mm wide in its middle: 3.5 mm in, past the 3.18 the ring clears.
        let bulge = plain(vec![[0.0, 0.0], [40.0, 0.0], [40.0, 6.0], [23.5, 6.0], [23.5, 7.0], [16.5, 7.0], [16.5, 6.0], [0.0, 6.0]]);
        let middle: Vec<&[f64; 3]> = bulge.iter().filter(|p| (p[1] - 3.5).abs() < 0.3 && (p[0] - 20.0).abs() < 3.0).collect();
        assert!(!middle.is_empty(), "a stroke along the middle of the bulge");
        let _ = r;
    }

    /// A V-bit carving a round hole wider than it, in passes, as in the person's saved test job
    /// (a 3.8 mm hole, a 60 degree 3.175 mm V-bit, 2 mm passes, clearing stepover 0.794): rings at
    /// 2, 4 and 6 mm from the edge ring inwards, then a plunge in the middle to 3.8 / tan 30.
    #[test]
    fn a_vbit_carves_a_wide_hole_in_passes_then_plunges_in_its_middle() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(crate::geom::circle([10.0, 10.0], 3.8))]);
        o.tool.kind = ToolKind::VBit;
        o.tool.dia = 3.175;
        o.tool.angle = 60.0;
        o.tool.pass_depth = 2.0;
        o.tool.clear_step = 0.794;
        o.flat_depth = 0.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let cuts: Vec<[f64; 3]> = moves.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect();
        let r = |p: &[f64; 3]| (p[0] - 10.0).hypot(p[1] - 10.0);
        let at = |z: f64| cuts.iter().filter(|p| (p[2] + z).abs() < 1e-6).map(r).collect::<Vec<f64>>();
        let outer = |z: f64| at(z).into_iter().fold(0.0, f64::max);
        let half = 30f64.to_radians().tan();
        for z in [2.0, 4.0, 6.0] {
            assert!((outer(z) - (3.8 - z * half)).abs() < 0.1, "the edge ring at {z} mm: {}", outer(z));
        }
        // Inside the first pass's edge ring, rings a clearing step apart.
        let first = at(2.0);
        assert!(first.iter().any(|r| (r - (3.8 - 2.0 * half - 0.794)).abs() < 0.1), "a ring one step in");
        let deepest = cuts.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((deepest + 3.8 / half).abs() < 0.15, "the plunge: {deepest}");
        let last = cuts.last().unwrap();
        assert!(r(last) < 0.1, "in the middle");
    }

    /// A ball nose carves no deeper than its radius (the original's ball-nose profile,
    /// `0x1408666f0`): a shape wider than it is cleared at that depth by rings its stepover apart,
    /// and its corners are carved up the ball's curve, as in the person's saved job (2 mm ball,
    /// stepover 0.2: rings 0.2 apart at 1.0 mm).
    #[test]
    fn a_ball_nose_carves_down_to_its_radius_and_clears_the_rest_there() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(crate::geom::rect([15.0, 15.0], 6.0, 6.0, 0.0))]);
        o.tool.kind = ToolKind::BallNose;
        o.tool.dia = 2.0;
        o.tool.angle = 0.0;
        o.tool.stepover = 0.1;
        o.tool.clear_step = 0.4;
        o.tool.pass_depth = 1.5;
        o.flat_depth = 0.0;
        let cuts: Vec<[f64; 3]> = generate(&o, M, &J, None).unwrap().iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect();
        let deepest = cuts.iter().map(|p| p[2]).fold(0.0, f64::min);
        assert!((deepest + 1.0).abs() < 1e-6, "no deeper than the ball: {deepest}");
        // Rings 0.2 apart at the full depth: a square ring a step inside the first.
        let ring = |inset: f64| cuts.iter().any(|p| (p[2] + 1.0).abs() < 1e-6 && ((p[0] - 15.0).abs().max((p[1] - 15.0).abs()) - (3.0 - inset)).abs() < 0.05);
        assert!(ring(1.0) && ring(1.2) && ring(1.4), "rings at 1.0, 1.2, 1.4 in");
        // Into the corners, coming up the ball's curve.
        let corner = cuts.iter().map(|p| (p[0] - 12.0).hypot(p[1] - 12.0)).fold(f64::MAX, f64::min);
        assert!(corner < 0.6, "into the corner: {corner}");
    }

    /// The middle line of a square is its two diagonals: a V-bit goes deepest in the middle and
    /// into every corner, coming up as the corner closes in.
    #[test]
    fn a_vbit_follows_the_middle_line_into_every_corner() {
        let mut o = op(OpKind::VCarve, vec![Path::closed(crate::geom::rect([15.0, 15.0], 10.0, 10.0, 0.0))]);
        o.tool.kind = ToolKind::VBit;
        o.tool.dia = 12.0;
        o.tool.angle = 60.0;
        o.tool.pass_depth = 20.0;
        o.flat_depth = 0.0;
        let cuts: Vec<[f64; 3]> = generate(&o, M, &J, None).unwrap().iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p).collect();
        for c in [[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]] {
            let near = cuts.iter().map(|p| (p[0] - c[0]).hypot(p[1] - c[1])).fold(f64::MAX, f64::min);
            assert!(near < 0.6, "into the corner {c:?}: {near}");
        }
        // Every cut on a diagonal, as deep as its distance from the sides allows.
        let half = 30f64.to_radians().tan();
        for p in &cuts {
            let off = ((p[0] - 15.0).abs() - (p[1] - 15.0).abs()).abs();
            assert!(off < 0.3, "on a diagonal: {p:?}");
            let inside = (p[0] - 10.0).min(20.0 - p[0]).min(p[1] - 10.0).min(20.0 - p[1]);
            assert!((-p[2] - inside / half).abs() < 0.3, "its depth: {p:?}");
        }
    }

    /// The original's lead fitting (`0x1408437a0`): ten points at ninths of the lead, cut to
    /// (k - 1) tenths at the first one across the loop. The saved jobs' four cases, and a 4 mm lead,
    /// which comes in by tenths of 4 (0.4 mm), not by 0.25 mm steps, or not at all when its middle
    /// point is already across.
    #[test]
    fn a_lead_into_a_small_loop_is_cut_to_tenths_where_it_crosses() {
        let lead_in = |loop_r: f64, lead: f64| {
            let mut o = op(OpKind::Profile, vec![]);
            o.lead = true;
            o.lead_distance = lead;
            o.lead_circular = false;
            o.lead_angle = 90.0;
            let pts = crate::geom::circle([0.0, 0.0], loop_r);
            let pts = if signed_area(&pts) > 0.0 { pts } else { pts.into_iter().rev().collect() };
            let side_sign = away_from_part(&pts, true, Side::Inside);
            fitted_lead(&pts, &[], Side::Inside, side_sign, &o)
        };
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(close(lead_in(1.9125, 2.5), 2.5));
        assert!(close(lead_in(1.4625, 2.5), 2.5));
        assert!(close(lead_in(1.0125, 2.5), 1.75), "{:?}", lead_in(1.0125, 2.5));
        assert!(close(lead_in(0.9125, 2.5), 1.5), "{:?}", lead_in(0.9125, 2.5));
        // 2.6 across: the 7th point (4 x 6/9 = 2.67) is the first across, so 5 tenths of 4.
        assert!(close(lead_in(1.3, 4.0), 2.0), "{:?}", lead_in(1.3, 4.0));
        // 2.0 across: the middle point (4 x 5/9 = 2.22) is already across, so no lead at all.
        assert_eq!(lead_in(1.0, 4.0), None);
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
            &[PostPath { name: "Drill 1", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "", slow: None }],
            &PostSettings { inches: true, origin: [1.0, 1.0], z_shift: 0.5, scale: 1.0, arcs: None, blend: None },
        );
        assert!(text.starts_with("(Written by Burin)\nG20"));
        assert!(text.contains("X1.0000 Y2.0000 Z0.0000"), "{text}");
        assert!(text.trim_end().ends_with("M30"));
        let mm = gcode(&[], &PostSettings { inches: false, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None, blend: None });
        assert!(mm.contains("G21"));
    }

    #[test]
    fn estimate_counts_feed_and_rapid() {
        let t = tool("End Mill (0.25 inch)");
        let m = [
            Move { p: [0.0, 0.0, 0.0], rapid: true, join: false },
            Move { p: [100.0, 0.0, 0.0], rapid: false, join: false },
        ];
        // One minute of cutting counts double.
        assert!((estimate_seconds(&m, &t, 200.0, 0.0) - 120.0).abs() < 1e-6);
        let feed = t.feed;
        let plunge = t.plunge;
        let m = [
            Move { p: [0.0, 0.0, 1.0], rapid: true, join: false },
            // A fast move, then down, a pause, and up again at the fast rate.
            Move { p: [200.0, 0.0, 1.0], rapid: true, join: false },
            Move { p: [200.0, 0.0, -plunge], rapid: false, join: false },
            Move { p: [200.0, 0.0, 1.0], rapid: true, join: false },
        ];
        let lift = (plunge + 1.0) / 200.0;
        let down = (plunge + 1.0) / plunge;
        let expected = ((down + lift) * 60.0 * 2.0).trunc() + 60.0 + 3.0;
        assert!((estimate_seconds(&m, &t, 200.0, 3.0) - expected).abs() < 1e-6, "{feed}");
    }

    /// A long curved strip, like a groove for an LED ribbon: Burin goes on from ring to ring along
    /// the walls where the original sometimes lifts and comes back down, so it may lift less, but
    /// every move at depth keeps the tool's radius from the walls.
    #[test]
    fn a_curved_strip_pocket_keeps_off_the_walls_and_lifts_once_per_depth() {
        let (c, outer, inner) = ([5.0, 1.0], 4.0, 3.0);
        let mut pts: Vec<P> = (0..=72).map(|i| {
            let a = std::f64::consts::PI * i as f64 / 72.0;
            [c[0] + outer * a.cos(), c[1] + outer * a.sin()]
        }).collect();
        pts.extend((0..=72).rev().map(|i| {
            let a = std::f64::consts::PI * i as f64 / 72.0;
            [c[0] + inner * a.cos(), c[1] + inner * a.sin()]
        }));
        let strip = Path::closed(pts);
        let o = op(OpKind::Pocket, vec![strip.clone()]);
        let m = generate(&o, M, &J, None).unwrap();
        let r = o.tool.dia / 2.0;
        let walls: Vec<(P, P)> = (0..strip.pts.len()).map(|i| (strip.pts[i], strip.pts[(i + 1) % strip.pts.len()])).collect();
        let mut closest = f64::MAX;
        for w in m.windows(2).filter(|w| !w[1].rapid && w[1].p[2] < 0.0) {
            let n = ((w[0].p[0] - w[1].p[0]).hypot(w[0].p[1] - w[1].p[1]) / 0.01).ceil().max(1.0) as usize;
            for k in 0..=n {
                let f = k as f64 / n as f64;
                let q = [w[0].p[0] + (w[1].p[0] - w[0].p[0]) * f, w[0].p[1] + (w[1].p[1] - w[0].p[1]) * f];
                assert!(crate::geom::inside_even_odd([&strip], q), "{q:?} left the strip");
                closest = walls.iter().map(|(a, b)| crate::geom::dist2_seg(q, *a, *b)).fold(closest, f64::min);
            }
        }
        assert!(closest.sqrt() > r - 0.001, "came within {:.4} of a wall, radius {r}", closest.sqrt());
        let mut depths: Vec<f64> = m.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p[2]).collect();
        depths.sort_by(f64::total_cmp);
        depths.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let lifts = m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        assert!(lifts <= depths.len(), "{lifts} lifts over {} depths", depths.len());
    }

    /// The second raster (`_pkpdDo2ndRaster`) cuts the rows again across the first, at the angle
    /// + 90 degrees, after them, at every depth.
    #[test]
    fn a_second_raster_crosses_the_first() {
        let shape = Path::closed(rect([5.0, 5.0], 3.0, 2.0, 0.0));
        let mut o = op(OpKind::Pocket, vec![shape.clone()]);
        o.raster = true;
        o.raster_angle = 0.0;
        o.profile_pass = ProfilePass::None;
        let once = generate(&o, M, &J, None).unwrap();
        o.raster_second = true;
        let twice = generate(&o, M, &J, None).unwrap();
        let long = |m: &[Move], along_x: bool| m.windows(2).filter(|w| !w[1].rapid && w[1].p[2] < 0.0 && {
            let (dx, dy) = ((w[1].p[0] - w[0].p[0]).abs(), (w[1].p[1] - w[0].p[1]).abs());
            if along_x { dx > 1.0 && dy < 1e-9 } else { dy > 1.0 && dx < 1e-9 }
        }).count();
        assert_eq!(long(&once, false), 0, "one raster: rows along X only");
        assert!(long(&twice, false) >= 4, "the second raster runs along Y");
        assert_eq!(long(&twice, true), long(&once, true), "the first rows are unchanged");
        // Each depth: rows along X, then along Y.
        let z = twice.iter().filter(|m| !m.rapid).map(|m| m.p[2]).fold(0.0, f64::min);
        let at: Vec<&Move> = twice.iter().filter(|m| !m.rapid && (m.p[2] - z).abs() < 1e-9).collect();
        let last_x = at.windows(2).rposition(|w| (w[1].p[0] - w[0].p[0]).abs() > 1.0 && (w[1].p[1] - w[0].p[1]).abs() < 1e-9).unwrap();
        let first_y = at.windows(2).position(|w| (w[1].p[1] - w[0].p[1]).abs() > 1.0 && (w[1].p[0] - w[0].p[0]).abs() < 1e-9).unwrap();
        assert!(last_x < first_y, "the rows across come after the first rows");
        let r = o.tool.dia / 2.0;
        let over = |m: &[Move]| m.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| ((m.p[0] - 5.0).abs() - (1.5 - r)).max((m.p[1] - 5.0).abs() - (1.0 - r))).fold(f64::MIN, f64::max);
        assert!(over(&twice) <= over(&once).max(0.0) + 1e-6, "no closer to the wall than the first rows go");
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

    /// Going on to the next raster row without lifting is a join, as in the original's saved jobs:
    /// at cutting speed, at one depth, between two cuts.
    #[test]
    fn raster_rows_are_linked_by_joins() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 3.0, 0.0))]);
        o.raster = true;
        o.profile_pass = ProfilePass::None;
        let m = generate(&o, M, &J, None).unwrap();
        let joins: Vec<usize> = (1..m.len()).filter(|&i| m[i].join).collect();
        assert!(joins.len() >= 3, "{} joins", joins.len());
        for i in joins {
            assert!(!m[i].rapid && (m[i].p[2] - m[i - 1].p[2]).abs() < 1e-9, "across at one depth");
            assert_eq!(move_kind(m[i - 1].p, &m[i]), MoveKind::Join);
        }
        // Without them a fast move takes their place, or a cut: nothing else is marked.
        let plain = generate(&op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 3.0, 3.0, 0.0))]), M, &J, None).unwrap();
        assert!(plain.iter().all(|m| !m.join));
    }

    /// Raster rows go on to the next one along the pocket's edge, as the original does
    /// (`0x1408cef30`), however far that is, never coming closer to a wall than the tool's radius.
    /// In a U, each prong is cleared on its own, so the rows lift once per depth, into the second prong.
    #[test]
    fn raster_rows_follow_the_edge_to_the_next_row() {
        let u = vec![[0.0, 0.0], [6.0, 0.0], [6.0, 5.0], [4.0, 5.0], [4.0, 1.5], [2.0, 1.5], [2.0, 5.0], [0.0, 5.0]];
        let mut o = op(OpKind::Pocket, vec![Path::closed(u.clone())]);
        o.raster = true;
        o.raster_angle = 0.0;
        o.profile_pass = ProfilePass::None;
        let m = generate(&o, M, &J, None).unwrap();
        let radius = o.tool.dia / 2.0;
        let wall = |p: [f64; 3]| {
            (0..u.len())
                .map(|i| {
                    let (a, b) = (u[i], u[(i + 1) % u.len()]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                    (a[0] + dx * t - p[0]).hypot(a[1] + dy * t - p[1])
                })
                .fold(f64::MAX, f64::min)
        };
        let mut joins = 0;
        for i in 1..m.len() {
            if m[i].join {
                joins += 1;
                for k in 0..=10 {
                    let t = k as f64 / 10.0;
                    let q = [m[i - 1].p[0] + (m[i].p[0] - m[i - 1].p[0]) * t, m[i - 1].p[1] + (m[i].p[1] - m[i - 1].p[1]) * t, 0.0];
                    assert!(wall(q) >= radius - 0.02, "a link comes {} from a wall at {q:?}", wall(q));
                }
            }
        }
        let rows = (5.0 / (o.tool.dia * o.tool.stepover)).floor() as usize;
        let lifts = m.windows(2).filter(|w| w[1].rapid && w[1].p[2] > w[0].p[2] + 1e-9).count();
        assert!(joins >= rows, "{joins} links for about {rows} rows");
        let mut levels: Vec<f64> = m.iter().filter(|x| !x.rapid && x.p[2] < -1e-9).map(|x| x.p[2]).collect();
        levels.sort_by(|a, b| a.total_cmp(b));
        levels.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        // Per depth: one lift into the second prong, the corner clean-up after the rows, and the way
        // down to the next depth. Row by row across both prongs it was 145.
        assert!(lifts <= 5 * levels.len(), "{lifts} lifts over {} levels", levels.len());
    }

    /// After the rings, the corners they left are cut without lifting: across the cleared floor,
    /// never closer to a wall than the tool's radius, and only one lift per pocket and depth.
    #[test]
    fn ring_pockets_finish_a_branch_before_lifting_to_the_next() {
        // Two squares joined by a neck: inside, the rings split in two. Cut level by level the tool
        // would lift at every level to cross the neck; branch by branch, once per depth.
        let shape = Path::closed(vec![[0.0, 0.0], [30.0, 0.0], [30.0, 12.5], [40.0, 12.5], [40.0, 0.0], [70.0, 0.0], [70.0, 30.0], [40.0, 30.0], [40.0, 17.5], [30.0, 17.5], [30.0, 30.0], [0.0, 30.0]]);
        let o = op(OpKind::Pocket, vec![shape]);
        let m = generate(&o, M, &J, None).unwrap();
        let radius = o.tool.dia / 2.0;
        let field = Field::build(&o.paths, o.tool.dia, 400).unwrap();
        let mut levels: Vec<f64> = m.iter().filter(|x| !x.rapid && x.p[2] < -1e-9).map(|x| x.p[2]).collect();
        levels.sort_by(|a, b| a.total_cmp(b));
        levels.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for i in 1..m.len() {
            if !m[i].rapid && !m[i - 1].rapid && (m[i].p[2] - m[i - 1].p[2]).abs() < 1e-9 {
                let q = [(m[i - 1].p[0] + m[i].p[0]) / 2.0, (m[i - 1].p[1] + m[i].p[1]) / 2.0];
                assert!(field.sample(q[0], q[1]) >= radius - 2.0 * field.cell, "{q:?} is {} from a wall", field.sample(q[0], q[1]));
            }
        }
        let lifts = m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        assert!(lifts <= 2 * levels.len(), "{lifts} lifts for {} depths", levels.len());
    }

    #[test]
    fn ring_pockets_reach_their_corners_without_lifting() {
        // A long thin triangle and an L: their sharp corners are where rings leave something.
        let shapes = vec![Path::closed(vec![[0.0, 0.0], [5.0, 0.0], [0.0, 2.2]]), Path::closed(vec![[6.0, 0.0], [9.5, 0.0], [9.5, 1.1], [7.2, 1.1], [7.2, 4.0], [6.0, 4.0]])];
        let mut o = op(OpKind::Pocket, shapes);
        // A wide stepover leaves the corners between rings.
        o.stepover = o.tool.dia * 0.85;
        let m = generate(&o, M, &J, None).unwrap();
        let radius = o.tool.dia / 2.0;
        let field = Field::build(&o.paths, o.tool.dia, 400).unwrap();
        let mut levels: Vec<f64> = m.iter().filter(|x| !x.rapid && x.p[2] < -1e-9).map(|x| x.p[2]).collect();
        levels.sort_by(|a, b| a.total_cmp(b));
        levels.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        // Every cut at one depth, corners included, keeps the tool off the walls.
        for i in 1..m.len() {
            if !m[i].rapid && (m[i].p[2] - m[i - 1].p[2]).abs() < 1e-9 && m[i].p[2] < -1e-9 {
                for k in 0..=20 {
                    let t = k as f64 / 20.0;
                    let q = [m[i - 1].p[0] + (m[i].p[0] - m[i - 1].p[0]) * t, m[i - 1].p[1] + (m[i].p[1] - m[i - 1].p[1]) * t];
                    assert!(field.sample(q[0], q[1]) >= radius - 2.0 * field.cell, "{q:?} is {} from a wall", field.sample(q[0], q[1]));
                }
            }
        }
        let lifts = m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        // Per depth: the L lifts once, to go on; the triangle twice, as its innermost ring is a
        // single point the rings go on from with a lift. None is for the corners.
        assert!(lifts <= 3 * levels.len(), "{lifts} lifts");
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
        let text = gcode(&[PostPath { name: "d", tool: &o.tool, moves: &moves, dwell: o.dwell, steps: None, notes: "", slow: None }], &PostSettings { inches: true, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None, blend: None });
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
        let text = gcode(&[PostPath { name: "p", tool: &o.tool, moves: &moves, dwell: 0.0, steps: None, notes: "", slow: None }], &PostSettings { inches: true, origin, z_shift: 0.0, scale: 1.0, arcs, blend: None });
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

    /// In a hole hardly wider than the tool, a lead is shortened until the tool stays inside
    /// the hole, and left out when no length fits: the saved jobs' 5.2 mm hole with a 3.175 mm
    /// tool takes 1.75 of a 2.5 lead, and a circular lead of radius 5 into a 5 mm hole none.
    #[test]
    fn a_lead_into_a_small_hole_is_shortened_or_left_out() {
        let hole = |r: f64| {
            let mut o = op(OpKind::Profile, vec![Path::closed(crate::geom::circle([20.0, 20.0], r))]);
            o.tool.dia = 3.175;
            o.tool.pass_depth = 10.0;
            o.cut_depth = 1.0;
            o.side = Side::Inside;
            o.lead = true;
            o.lead_distance = 2.5;
            o.lead_angle = 90.0;
            o.lead_circular = false;
            o
        };
        let far_from_centre = |o: &Op| {
            let moves = generate(o, M, &J, None).unwrap();
            let c = cuts(&moves);
            (c[0][0] - 20.0).hypot(c[0][1] - 20.0)
        };
        // A wide hole keeps the whole lead: it starts 2.5 in from the loop.
        let wide = hole(6.0);
        assert!((far_from_centre(&wide) - (6.0 - 1.5875 - 2.5)).abs() < 0.05, "{}", far_from_centre(&wide));
        // In the 5.2 mm hole the loop has a radius of 1.0125, and the lead comes in 1.75 of it.
        let small = hole(2.6);
        assert!((far_from_centre(&small) - (1.75 - 1.0125)).abs() < 0.05, "{}", far_from_centre(&small));
        // A circular lead too big for a 5 mm hole is left out: the cut starts on the loop.
        let mut tiny = hole(2.5);
        tiny.allowance = -0.3;
        tiny.lead_circular = true;
        tiny.lead_radius = 5.0;
        tiny.lead_distance = 7.0;
        assert!((far_from_centre(&tiny) - (2.5 - 1.5875 + 0.3)).abs() < 0.05, "{}", far_from_centre(&tiny));
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

    #[test]
    fn a_zig_zag_on_the_lead_goes_most_of_the_way_along_it_and_back() {
        let mut o = lead_op(Side::Outside);
        o.ramp = true;
        o.ramp_on_lead = true;
        o.ramp_kind = RampKind::ZigZag;
        o.lead_circular = false;
        o.lead_distance = 0.8;
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let first = c[0];
        assert!(first[2].abs() < 1e-9, "starts at the surface: {first:?}");
        let turn = c[1];
        let gone = (turn[0] - first[0]).hypot(turn[1] - first[1]);
        assert!((gone - 0.8 * 0.95).abs() < 1e-6, "95% of the lead: {gone}");
        assert!((turn[2] + 0.125).abs() < 1e-9, "half way down there: {turn:?}");
        assert!((c[2][0] - first[0]).abs() < 1e-9 && (c[2][1] - first[1]).abs() < 1e-9 && (c[2][2] + 0.25).abs() < 1e-9, "back at the start, at depth: {:?}", c[2]);
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
        // As thick as the whole depth: no pass at the top that would cut nothing.
        assert_eq!(passes_with_last(1.0, 2.0, 1.0), [1.0], "the whole depth");
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
    fn an_open_line_can_be_cut_there_and_back_without_lifting() {
        let line = vec![Path::open(vec![[2.0, 2.0], [8.0, 2.0]])];
        let mut o = op(OpKind::Profile, line);
        o.side = Side::On;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.75;
        let start = |m: &[Move]| m.iter().filter(|m| !m.rapid && m.p[2] < -1e-9).map(|m| m.p).collect::<Vec<_>>();
        let one_way = generate(&o, M, &J, None).unwrap();
        let (first, last) = (start(&one_way)[0], *start(&one_way).last().unwrap());
        assert!((first[0] - last[0]).abs() > 5.0, "each pass goes the same way: {first:?} {last:?}");
        assert!(one_way.iter().filter(|m| m.rapid).count() > 4, "and lifts to go back");

        o.zigzag_open = true;
        let both = generate(&o, M, &J, None).unwrap();
        let cuts = start(&both);
        let at_depth = |z: f64| cuts.iter().filter(|p| (p[2] - z).abs() < 1e-9).map(|p| p[0]).collect::<Vec<_>>();
        assert_eq!(*at_depth(-0.25).last().unwrap(), 8.0);
        assert_eq!(*at_depth(-0.5).last().unwrap(), 2.0, "the second pass comes back");
        assert_eq!(*at_depth(-0.75).last().unwrap(), 8.0);
        // Nothing fast between the first cut and the last.
        let a = both.iter().position(|m| !m.rapid && m.p[2] < -1e-9).unwrap();
        let b = both.iter().rposition(|m| !m.rapid && m.p[2] < -1e-9).unwrap();
        assert!(both[a..=b].iter().all(|m| !m.rapid), "{:?}", &both[a..=b]);
    }

    #[test]
    fn a_closed_shape_is_not_cut_there_and_back() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.75;
        let plain = generate(&o, M, &J, None).unwrap();
        o.zigzag_open = true;
        assert_eq!(generate(&o, M, &J, None).unwrap(), plain);
    }

    #[test]
    fn a_tab_keeps_its_whole_length_after_the_width_of_the_tool() {
        let mut o = ring_op();
        o.start_at = StartAt::Keep;
        o.cut_depth = 0.5;
        o.tabs = true;
        o.tab_count = 4;
        o.tab_length = 0.5;
        o.tab_thickness = 0.1;
        let moves = generate(&o, M, &J, None).unwrap();
        let top = -(o.cut_depth - o.tab_thickness);
        // The lengths the tool travels at the top of a tab.
        let (mut spans, mut run) = (Vec::new(), 0.0);
        for w in moves.windows(2) {
            if !w[1].rapid && (w[0].p[2] - top).abs() < 1e-9 && (w[1].p[2] - top).abs() < 1e-9 {
                run += (w[0].p[0] - w[1].p[0]).hypot(w[0].p[1] - w[1].p[1]);
            } else {
                if run > 0.0 {
                    spans.push(run);
                }
                run = 0.0;
            }
        }
        assert_eq!(spans.len(), 4, "{spans:?}");
        for s in &spans {
            assert!((s - (o.tab_length + o.tool.dia)).abs() < 1e-6, "{spans:?}");
        }
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

    /// Ramps in passes as the original makes them (`0x1408575e0`, `0x1408566f0`, `0x140857ce0`):
    /// the tool never leaves the loop's line between passes; a straight ramp's next pass ramps on
    /// from where the last ended; a zig-zag goes along the loop and back; a spiral comes down with
    /// no lap between depths and one at the bottom.
    #[test]
    fn ramps_stay_on_the_loop_and_follow_on_from_pass_to_pass() {
        let on_square = |p: &[f64; 3]| {
            let (dx, dy) = ((p[0] - 5.0).abs(), (p[1] - 5.0).abs());
            (dx.max(dy) - 2.0).abs() < 1e-6
        };
        let length = |c: &[[f64; 3]]| c.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
        for kind in [RampKind::Smooth, RampKind::ZigZag, RampKind::Spiral] {
            let mut o = ring_op();
            o.start_at = StartAt::Keep;
            o.tool.pass_depth = 1.0;
            o.cut_depth = 3.0;
            o.ramp = true;
            o.ramp_kind = kind;
            o.ramp_distance = 5.0;
            o.overcut = 0.0;
            let c = cuts(&generate(&o, M, &J, None).unwrap());
            // Every point on the square, and every move along one of its sides.
            assert!(c.iter().all(on_square), "{kind:?} left the loop");
            assert!(c.windows(2).all(|w| (w[0][0] - w[1][0]).abs() < 1e-9 || (w[0][1] - w[1][1]).abs() < 1e-9), "{kind:?} cut across");
            let expected = match kind {
                // Each pass a ramp of 5 then a lap of 16.
                RampKind::Smooth => 3.0 * (5.0 + 16.0),
                // Each pass 5 there and 5 back, then a lap.
                RampKind::ZigZag => 3.0 * (10.0 + 16.0),
                // Three laps down, one at the bottom.
                RampKind::Spiral => 4.0 * 16.0,
            };
            assert!((length(&c) - expected).abs() < 1e-6, "{kind:?}: {} against {expected}", length(&c));
        }
    }

    /// Tabs put on the drawing stand where they were put, centred on the loop at the place
    /// nearest each, tab length plus the tool's width long; a loop starting on one comes down
    /// only to its top there.
    #[test]
    fn tabs_put_on_the_drawing_stand_where_they_were_put() {
        let mut o = ring_op();
        o.start_at = StartAt::Keep;
        o.cut_depth = 0.5;
        o.tabs = true;
        o.tab_length = 0.5;
        o.tab_thickness = 0.1;
        o.tab_spots = vec![[5.0, 3.0], [3.0, 3.2]];
        let c = cuts(&generate(&o, M, &J, None).unwrap());
        let up: Vec<&[f64; 3]> = c.iter().filter(|p| (p[2] + 0.4).abs() < 1e-9).collect();
        assert!(!up.is_empty());
        let tab_reach = o.tab_length + o.tool.dia;
        for p in &up {
            let near_first = (p[1] - 3.0).abs() < 1e-9 && (p[0] - 5.0).abs() <= tab_reach / 2.0 + 1e-9;
            let near_second = (p[0] - 3.0).abs() < 1e-9 && (p[1] - 3.2).abs() <= tab_reach / 2.0 + 1e-9 || (p[1] - 3.0).abs() < 1e-9 && p[0] <= 3.0 + tab_reach / 2.0;
            assert!(near_first || near_second, "up at {p:?}, away from the tabs put");
        }
        // Down to full depth elsewhere.
        assert!(c.iter().any(|p| (p[2] + 0.5).abs() < 1e-9 && (p[1] - 7.0).abs() < 1e-9));
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

    /// Where a ring along the outline meets one round an island, the corner is sharp, as the
    /// original's exact offsets make it: the person's islands job, a 90 by 70 pocket with a round
    /// island of radius 10 at (40, 50), the ring 12.6 in meets the island's at (37, 27.6).
    #[test]
    fn a_ring_meets_an_islands_ring_at_a_sharp_corner() {
        let outer = Path::closed(rect([60.0, 50.0], 90.0, 70.0, 0.0));
        let island = Path::closed(circle([40.0, 50.0], 10.0));
        let o = pocket_of(vec![outer, island], 6.0, 0.4);
        let moves = generate(&o, M, &J, None).unwrap();
        let near = moves.iter().filter(|m| !m.rapid).map(|m| (m.p[0] - 37.0).hypot(m.p[1] - 27.6)).fold(f64::MAX, f64::min);
        assert!(near < 0.05, "the corner is cut {near} short");
    }

    /// A wide stepover leaves the corners between rings: each ring after the first reaches into
    /// its corners, out to where the ring before stopped clearing, as the saved slot pockets do
    /// (a 94% stepover with a 3.175 mm tool: ears 1.96 mm long, their tips 3.18 from the walls).
    #[test]
    fn a_wide_stepover_reaches_into_the_corners() {
        let mut o = pocket_of(vec![Path::closed(rect([50.0, 50.0], 40.0, 20.0, 0.0))], 3.175, 0.94);
        o.cut_depth = 1.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let r = 3.175 / 2.0;
        let step = 3.175 * 0.94;
        // The corner of the second ring, and where its ear ends: 3.18 in from both walls.
        let tip = [30.0 + 2.0 * r, 40.0 + 2.0 * r];
        let near = moves.iter().filter(|m| !m.rapid).map(|m| (m.p[0] - tip[0]).hypot(m.p[1] - tip[1])).fold(f64::MAX, f64::min);
        assert!(near < 0.02, "the ear ends {near} from where it should");
        let corner = [30.0 + r + step, 40.0 + r + step];
        assert!(moves.iter().any(|m| !m.rapid && (m.p[0] - corner[0]).hypot(m.p[1] - corner[1]) < 0.02));
    }

    /// What the ring before leaves and the next ring's reach misses, in a lobe the next ring
    /// doesn't go into, is cut along its middle: a slot whose end narrows to a point beyond a
    /// round part, with a 94% stepover.
    #[test]
    fn a_lobe_the_next_ring_misses_is_cut_along_its_middle() {
        // A wide body with a narrow nose at its right: the second ring stays in the body.
        let shape = vec![[0.0, 0.0], [30.0, 0.0], [30.0, 2.0], [45.0, 6.0], [30.0, 10.0], [30.0, 12.0], [0.0, 12.0]];
        let mut o = pocket_of(vec![Path::closed(shape.clone())], 3.175, 0.94);
        o.cut_depth = 1.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let field = Field::build(&o.paths, o.tool.dia, 400).unwrap();
        let r = 3.175 / 2.0;
        // Somewhere in the nose further from the walls than the first ring's reach is reached.
        let reach = |q: P| moves.windows(2).filter(|w| !w[1].rapid).any(|w| crate::geom::dist2_seg(q, [w[0].p[0], w[0].p[1]], [w[1].p[0], w[1].p[1]]) <= r * r * 1.0001);
        let spots: Vec<P> = (0..300).map(|k| [30.0 + k as f64 * 0.05, 6.0]).filter(|q| field.sample(q[0], q[1]) > 2.0 * r + 0.05).collect();
        assert!(!spots.is_empty());
        assert!(spots.iter().all(|q| reach(*q)), "left in the nose: {:?}", spots.iter().find(|q| !reach(**q)));
    }

    /// A little step in a ring's side, which goes on straight either side of it, gets no ear.
    #[test]
    fn a_step_in_a_straight_side_gets_no_ear() {
        let mut ring = vec![[0.0, 0.0], [20.0, 0.0], [20.0, 0.3], [40.0, 0.3], [40.0, 10.0], [0.0, 10.0]];
        let mut pieces = Vec::new();
        let out = with_corner_ears(std::mem::take(&mut ring), 0.4, &[], &mut pieces, 1.6, 0.05);
        // The four corners of the box get ears (two points each); the step's two corners none.
        assert_eq!(out.len(), 6 + 4 * 2, "{out:?}");
    }

    /// Tabs go with their shapes: moved, a tab stays at the same place along its shape; with
    /// more points, at the same share of the way round.
    #[test]
    fn tabs_go_where_their_shapes_go() {
        let square = Path::closed(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let moved = Path::closed(square.pts.iter().map(|q| [q[0] + 5.0, q[1] * 2.0]).collect());
        let tabs = carry_tabs(&[square.clone()], &[moved], &[[10.0, 4.0], [3.0, 0.0]]);
        assert!((tabs[0][0] - 15.0).abs() < 1e-9 && (tabs[0][1] - 8.0).abs() < 1e-9, "{tabs:?}");
        assert!((tabs[1][0] - 8.0).abs() < 1e-9 && tabs[1][1].abs() < 1e-9, "{tabs:?}");
        // Drawn again with a point more: a quarter of the way round stays a quarter of the way.
        let more = Path::closed(vec![[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let tabs = carry_tabs(&[square], &[more], &[[10.0, 0.0]]);
        assert!((tabs[0][0] - 10.0).abs() < 1e-9 && tabs[0][1].abs() < 1e-9, "{tabs:?}");
    }

    /// A V-bit V-carve with a flat depth and a clearing tool: the ring where the V meets the flat
    /// goes round the shape and round its hole too, and the flat itself is left to the clearing
    /// tool, as on the person's plates (a 90 degree V-bit, 1 mm flat: 28 traces, 1,766 mm, against
    /// 31 and 1,765 saved, where Burin swept in levels before: 361 and 37.9 m).
    #[test]
    fn a_v_bit_with_a_clearing_tool_rings_the_flat_and_its_holes() {
        let outer = Path::closed(rect([50.0, 50.0], 40.0, 30.0, 0.0));
        let hole = Path::closed(rect([50.0, 50.0], 10.0, 8.0, 0.0));
        let mut o = op(OpKind::VCarve, vec![outer, hole]);
        o.tool.kind = ToolKind::VBit;
        o.tool.angle = 90.0;
        o.tool.dia = 22.0;
        o.tool.stepover = 0.04;
        o.tool.pass_depth = 1.0;
        o.flat_depth = 1.0;
        let mut clear = o.tool.clone();
        clear.kind = ToolKind::EndMill;
        clear.angle = 0.0;
        clear.dia = 3.175;
        o.clearance_tool = Some(clear);
        let moves = generate(&o, M, &J, None).unwrap();
        let cut: Vec<&Move> = moves.iter().filter(|m| !m.rapid).collect();
        // Round the hole, 1 mm out from it (the V's offset at the flat depth).
        let passes_by = |q: P| moves.windows(2).any(|w| !w[1].rapid && (w[1].p[2] + 1.0).abs() < 1e-6 && crate::geom::dist2_seg(q, [w[0].p[0], w[0].p[1]], [w[1].p[0], w[1].p[1]]) < 0.05 * 0.05);
        assert!(passes_by([44.0, 50.0]), "no ring round the hole");
        // Nothing in the middle of the flat, which is the clearing tool's.
        assert!(!cut.iter().any(|m| (m.p[0] - 37.0).abs() < 1.0 && (m.p[1] - 50.0).abs() < 1.0), "the flat was swept");
    }

    /// The person's flat V-Carve test (a 90° V-bit, a flat bottom, no clearing tool): the original
    /// cuts the outer ring at the flat and the trips up into the corners as one cut, the tool going
    /// out along the middle line and back onto the ring without lifting. The flat is still ringed
    /// all the way in.
    #[test]
    fn a_flat_v_carve_goes_into_the_corners_from_its_outer_ring() {
        let outer = Path::closed(rect([50.0, 50.0], 40.0, 30.0, 0.0));
        let mut o = op(OpKind::VCarve, vec![outer]);
        o.tool.kind = ToolKind::VBit;
        o.tool.angle = 90.0;
        o.tool.dia = 31.75;
        o.tool.stepover = 0.4 / 31.75;
        o.tool.pass_depth = 12.7;
        o.flat_depth = 5.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let mut traces: Vec<Vec<[f64; 3]>> = Vec::new();
        for w in moves.windows(2) {
            if w[1].rapid {
                continue;
            }
            if w[0].rapid || traces.is_empty() {
                traces.push(vec![w[0].p]);
            }
            traces.last_mut().unwrap().push(w[1].p);
        }
        let flat = |p: &[f64; 3]| (p[2] + 5.0).abs() < 1e-6;
        // Each corner is reached (nearly to the outline, so nearly to the top)...
        for c in [[30.0, 35.0], [70.0, 35.0], [70.0, 65.0], [30.0, 65.0]] {
            assert!(moves.iter().any(|m| !m.rapid && (m.p[0] - c[0]).hypot(m.p[1] - c[1]) < 1.0 && m.p[2] > -1.0), "corner {c:?} not reached");
        }
        // ...from a cut that also runs along the flat: no corner trip is a cut of its own.
        for t in &traces {
            if t.iter().any(|p| p[2] > -1.0) {
                assert!(t.iter().filter(|p| flat(p)).count() > 10, "a corner trip on its own");
            }
        }
        // The middle of the flat is cut.
        assert!(moves.windows(2).any(|w| !w[1].rapid && flat(&w[0].p) && flat(&w[1].p) && crate::geom::dist2_seg([50.0, 50.0], [w[0].p[0], w[0].p[1]], [w[1].p[0], w[1].p[1]]) < 1.0), "the middle of the flat was left");
    }

    /// Without a clearing tool the flat of a letter is ringed in one go: from ring to ring the tool
    /// stays down while the way stays in the flat (the person's test: 14 cuts for "test" in the
    /// original). An L shape's two arms meet at a corner where the rings split.
    #[test]
    fn a_flat_v_carve_rings_its_flat_without_lifting() {
        let l = Path::closed(vec![[20.0, 20.0], [100.0, 20.0], [100.0, 50.0], [50.0, 50.0], [50.0, 100.0], [20.0, 100.0]]);
        let mut o = op(OpKind::VCarve, vec![l]);
        o.tool.kind = ToolKind::VBit;
        o.tool.angle = 90.0;
        o.tool.dia = 31.75;
        o.tool.stepover = 1.0 / 31.75;
        o.tool.pass_depth = 12.7;
        o.flat_depth = 5.0;
        let moves = generate(&o, M, &J, None).unwrap();
        let lifts = moves.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
        let flat_cut: f64 = moves.windows(2).filter(|w| !w[1].rapid && (w[0].p[2] + 5.0).abs() < 1e-6 && (w[1].p[2] + 5.0).abs() < 1e-6).map(|w| (w[1].p[0] - w[0].p[0]).hypot(w[1].p[1] - w[0].p[1])).sum();
        assert!(flat_cut > 500.0, "{flat_cut}");
        assert!(lifts <= 3, "{lifts} lifts");
    }

    /// A waist between two holes of a letter, narrower than the flat's ring (the person's 80 cm plates'
    /// 8-like letters): the walk goes through it, above the flat, though it never reaches the
    /// letter's outer outline; with a clearing tool the corners still aren't walked.
    #[test]
    fn a_waist_between_two_holes_is_carved() {
        let outer = Path::closed(rect([50.0, 50.0], 30.0, 50.0, 0.0));
        let low = Path::closed(rect([50.0, 39.3], 14.0, 20.0, 0.0));
        let high = Path::closed(rect([50.0, 60.7], 14.0, 20.0, 0.0));
        let mut o = op(OpKind::VCarve, vec![outer, low, high]);
        o.tool.kind = ToolKind::VBit;
        o.tool.angle = 90.0;
        o.tool.dia = 22.0;
        o.tool.stepover = 0.04;
        o.tool.pass_depth = 3.0;
        o.flat_depth = 1.0;
        let mut clear = o.tool.clone();
        clear.kind = ToolKind::EndMill;
        clear.angle = 0.0;
        clear.dia = 3.175;
        o.clearance_tool = Some(clear);
        let moves = generate(&o, M, &J, None).unwrap();
        // The holes are 1.4 apart: the waist's middle is 0.7 from each, so cut 0.7 deep there.
        assert!(moves.iter().any(|m| !m.rapid && (m.p[1] - 50.0).abs() < 0.3 && (m.p[0] - 50.0).abs() < 5.0 && m.p[2] > -0.99), "the waist wasn't carved");
        // No trip into the outer corners.
        assert!(!moves.iter().any(|m| !m.rapid && (m.p[0] - 35.0).abs() < 0.8 && (m.p[1] - 25.0).abs() < 0.8), "a corner was walked");
    }

    /// A stroke's trip into a corner is made once, though both sides of the loop land on it.
    #[test]
    fn a_trip_made_twice_is_made_once() {
        let pts = vec![[0.0, 0.0], [0.0, 5.0], [1.0, 6.0], [0.0, 5.0], [1.0, 6.0], [0.0, 5.0], [-1.0, 6.0], [0.0, 5.0]];
        assert_eq!(without_repeats(pts, 0.01), vec![[0.0, 0.0], [0.0, 5.0], [1.0, 6.0], [0.0, 5.0], [-1.0, 6.0], [0.0, 5.0]]);
    }

    /// Where a pocket is exactly twice a ring's level wide, that ring has no width and isn't cut:
    /// the person's islands job, two bars 30 high joined by a 7 wide neck, cut with a 6 mm tool at
    /// 40%: the ring 15 in is only the bump at the neck, not the whole middle of each bar.
    #[test]
    fn a_ring_with_no_width_is_not_cut() {
        let shape = vec![[120.0, 15.0], [185.0, 15.0], [185.0, 45.0], [156.0, 45.0], [156.0, 55.0], [185.0, 55.0], [185.0, 85.0], [120.0, 85.0], [120.0, 55.0], [149.0, 55.0], [149.0, 45.0], [120.0, 45.0]];
        let o = pocket_of(vec![Path::closed(shape)], 6.0, 0.4);
        let moves = generate(&o, M, &J, None).unwrap();
        let along_middle = moves.windows(2).filter(|w| !w[1].rapid).filter(|w| {
            let mid = [(w[0].p[0] + w[1].p[0]) / 2.0, (w[0].p[1] + w[1].p[1]) / 2.0];
            (w[0].p[1] - w[1].p[1]).abs() < 0.01 && ((mid[1] - 30.0).abs() < 0.05 || (mid[1] - 70.0).abs() < 0.05) && (mid[0] < 145.0 || mid[0] > 160.0)
        });
        let found: Vec<_> = along_middle.map(|w| (w[0].p, w[1].p)).collect();
        assert!(found.is_empty(), "nothing along the bars' middle lines away from the neck: {found:?}");
        // The bump at the neck is still cut.
        assert!(moves.iter().any(|m| !m.rapid && (m.p[1] - 30.4).abs() < 0.1 && (m.p[0] - 152.5).abs() < 0.5));
    }

    /// A ring pocket starts on the side where its shape starts, as the saved jobs do: the innermost
    /// ring at its point nearest the shape's start carried in ring by ring, and the rings around it
    /// from where the tool comes out, so the plunge moves with the shape's first point.
    #[test]
    fn a_ring_pocket_starts_on_the_side_of_the_shape_start() {
        let first_cut = |pts: Vec<P>| {
            let o = pocket_of(vec![Path::closed(pts)], 3.0, 0.4);
            let moves = generate(&o, M, &J, None).unwrap();
            let c = cuts(&moves);
            c[0]
        };
        // From the bottom right corner, then the same rectangle from its top left.
        let a = first_cut(vec![[40.0, 0.0], [40.0, 20.0], [0.0, 20.0], [0.0, 0.0]]);
        let b = first_cut(vec![[0.0, 20.0], [0.0, 0.0], [40.0, 0.0], [40.0, 20.0]]);
        assert!(a[0] > 20.0 && a[1] <= 10.0 + 1e-6, "{a:?}");
        assert!(b[0] < 20.0 && b[1] >= 10.0 - 1e-6, "{b:?}");
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
        let mut e = Emit { moves: Vec::new(), m: M, project: None, top: 0.0, same_spot: SAME_SPOT_MM };
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
        let m = |z: f64, rapid: bool| Move { p: [0.0, 0.0, z], rapid, join: false };
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
        // On a round shape, whichever of its quarter points is nearest.
        let mut round = circle([50.0, 50.0], 10.0);
        start_near(&mut round, [200.0, 50.0]);
        assert!((round[0][0] - 60.0).abs() < 1e-9 && (round[0][1] - 50.0).abs() < 1e-9, "{:?}", round[0]);
        let mut round = circle([50.0, 50.0], 10.0);
        start_near(&mut round, [0.0, 70.0]);
        assert!((round[0][0] - 40.0).abs() < 1e-9 && (round[0][1] - 50.0).abs() < 1e-9, "the left one, not the nearest point: {:?}", round[0]);
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
    fn a_tie_between_ways_round_goes_to_the_one_starting_nearest_the_tool() {
        // Four shapes one above the other, the tool above them: the same loop either way round.
        let shapes: Vec<(Vec<P>, bool)> = (0..4).map(|k| (rect([700.0, 550.0 + 180.0 * k as f64], 400.0, 170.0, 0.0), true)).collect();
        let choices = OrderChoices { selection: false, left_to_right: true, bottom_to_top: true, grid: true, shortest: false };
        let order = sequence_order(&shapes, choices, [940.0, 1240.0, 50.0]);
        assert_eq!(order, vec![3, 2, 1, 0], "from the top, nearest the tool");
    }

    #[test]
    fn pocket_rows_cover_the_box_from_below_its_corner() {
        let rows = pocket_rows([0.0, 0.0], [30.0, 40.0], 0.0, 2.0);
        // Half the diagonal is 25, and they reach 5% past that all round.
        let reach = 25.0 * 1.05;
        // Half a step in: the middle of the first band.
        assert!((rows[0].0[0] - (15.0 - reach)).abs() < 1e-9 && (rows[0].0[1] - (20.0 - reach + 1.0)).abs() < 1e-9);
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
    fn going_to_where_the_tool_already_is_does_not_lift_it() {
        let mut e = Emit { moves: Vec::new(), m: M, project: None, top: 0.0, same_spot: SAME_SPOT_MM };
        e.feed(5.0, 5.0, -0.25);
        e.go_to(5.0, 5.0, -0.5);
        assert_eq!(e.moves.len(), 2);
        assert!(!e.moves[1].rapid && e.moves[1].p[2] == -0.5);
        e.go_to(5.0, 5.0, -0.5);
        assert_eq!(e.moves.len(), 2, "already there");
        // Anywhere else, up and across.
        e.go_to(9.0, 5.0, -0.5);
        assert!(e.moves[2].rapid && e.moves[2].p == [5.0, 5.0, M.safe_z]);
    }

    #[test]
    fn names_never_put_one_comment_inside_another() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.tool.name = "End Mill (6 mm)".into();
        o.name = "Outside (top)".into();
        let moves = generate(&o, M, &J, None).unwrap();
        let path = PostPath { name: &o.name, tool: &o.tool, moves: &moves, dwell: 0.0, steps: None, notes: "", slow: None };
        let text = gcode(&[path], &PostSettings { inches: false, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None, blend: None });
        for line in text.lines() {
            let mut open = 0;
            for c in line.chars() {
                match c {
                    '(' => open += 1,
                    ')' => open -= 1,
                    _ => {}
                }
                assert!((0..=1).contains(&open), "{line}");
            }
            assert_eq!(open, 0, "{line}");
        }
        assert!(text.contains("(Tool: End Mill [6 mm])"), "{text}");
    }

    #[test]
    fn a_separate_last_pass_finishes_each_shape_before_the_next() {
        let squares = vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0)), Path::closed(rect([30.0, 10.0], 4.0, 4.0, 0.0))];
        let mut o = op(OpKind::Profile, squares);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.75;
        o.last_pass = true;
        o.last_allowance = 0.05;
        let moves = generate(&o, M, &J, None).unwrap();
        let cuts: Vec<[f64; 3]> = moves.iter().filter(|m| !m.rapid && m.p[2] < -1e-9).map(|m| m.p).collect();
        let shape = |p: &[f64; 3]| if p[0] < 20.0 { 0 } else { 1 };
        // All of one shape, then all of the other.
        let turns = cuts.windows(2).filter(|w| shape(&w[0]) != shape(&w[1])).count();
        assert_eq!(turns, 1, "{cuts:?}");
        // The rough loop stops a depth short: the last depth is only cut on the finish loop.
        let r = o.tool.dia / 2.0;
        let mut links = 0;
        for p in &cuts {
            let c = if shape(p) == 0 { 10.0 } else { 30.0 };
            // Distance from the square itself, so the rounded corners count the same as the sides.
            let off = ((p[0] - c).abs() - 2.0).max(0.0).hypot(((p[1] - 10.0).abs() - 2.0).max(0.0));
            if (p[2] + 0.75).abs() < 1e-9 {
                assert!((off - r).abs() < 0.02, "bottom only at the finish: {p:?} {off}");
            } else if (off - r).abs() < 0.02 {
                // The one move across to the finish loop, still at the height above it.
                links += 1;
            } else {
                assert!((off - r - 0.05).abs() < 0.02, "above that with the allowance: {p:?} {off}");
            }
        }
        assert_eq!(links, 2);
    }

    #[test]
    fn the_finish_follows_the_rough_loop_without_a_lift() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.75;
        o.last_pass = true;
        o.last_allowance = 0.05;
        let moves = generate(&o, M, &J, None).unwrap();
        let first = moves.iter().position(|m| !m.rapid && m.p[2] < -1e-9).unwrap();
        let bottom = moves.iter().position(|m| (m.p[2] + 0.75).abs() < 1e-9).unwrap();
        assert!(moves[first..bottom].iter().all(|m| !m.rapid && m.p[2] < -1e-9), "stays in the cut: {:?}", &moves[first..=bottom]);
        // Across at the height it was at, and only then down.
        assert!((moves[bottom - 1].p[2] + 0.5).abs() < 1e-9);
        assert_eq!(moves[bottom - 1].p[..2], moves[bottom].p[..2]);
        assert!(moves[bottom..].iter().skip_while(|m| !m.rapid).all(|m| m.rapid || m.p[2] > -1e-9));

        // A ramp or a lead brings its own way in, so the tool lifts first.
        o.ramp = true;
        o.ramp_distance = 0.5;
        let moves = generate(&o, M, &J, None).unwrap();
        let bottom = moves.iter().position(|m| (m.p[2] + 0.75).abs() < 1e-9).unwrap();
        assert!(moves[..bottom].iter().rev().take_while(|m| m.p[2] < 1e-9).count() < bottom);
        assert!(moves[..bottom].iter().rev().any(|m| m.rapid));
    }

    #[test]
    fn the_finish_starts_where_the_tool_is_and_can_run_the_other_way() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.5;
        o.last_pass = true;
        o.last_allowance = 0.05;
        let area = |moves: &[Move], at_bottom: bool| {
            let pts: Vec<[f64; 3]> = moves.iter().filter(|m| !m.rapid && ((m.p[2] + 0.5).abs() < 1e-9) == at_bottom && m.p[2] < -1e-9).map(|m| m.p).collect();
            (0..pts.len()).map(|i| pts[i][0] * pts[(i + 1) % pts.len()][1] - pts[(i + 1) % pts.len()][0] * pts[i][1]).sum::<f64>()
        };
        let same = generate(&o, M, &J, None).unwrap();
        assert!(area(&same, false) * area(&same, true) > 0.0, "same way round");
        // The way across is no longer than the allowance plus the gap to the nearest corner point.
        let bottom = same.iter().position(|m| (m.p[2] + 0.5).abs() < 1e-9).unwrap();
        let (a, b) = (same[bottom - 2].p, same[bottom - 1].p);
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 0.2, "{a:?} {b:?}");
        o.last_reverse = true;
        let back = generate(&o, M, &J, None).unwrap();
        assert!(area(&back, false) * area(&back, true) < 0.0, "the other way round");
    }

    #[test]
    fn a_whole_way_ramp_takes_the_rough_loop_to_the_last_depth_too() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.5;
        o.last_pass = true;
        o.last_allowance = 0.05;
        o.ramp = true;
        o.ramp_kind = RampKind::Spiral;
        let moves = generate(&o, M, &J, None).unwrap();
        let r = o.tool.dia / 2.0;
        let gap = |p: &[f64; 3]| ((p[0] - 10.0).abs() - 2.0).max(0.0).hypot(((p[1] - 10.0).abs() - 2.0).max(0.0)) - r;
        let at_bottom: Vec<f64> = moves.iter().filter(|m| !m.rapid && (m.p[2] + 0.5).abs() < 1e-9).map(|m| gap(&m.p)).collect();
        assert!(at_bottom.iter().any(|g| (g - 0.05).abs() < 0.02), "rough at the last depth");
        assert!(at_bottom.iter().any(|g| g.abs() < 0.02), "finish at the last depth");
    }

    #[test]
    fn no_allowance_means_no_separate_last_pass() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 0.25;
        o.cut_depth = 0.5;
        let plain = generate(&o, M, &J, None).unwrap();
        o.last_pass = true;
        o.last_allowance = 0.0;
        assert_eq!(generate(&o, M, &J, None).unwrap().len(), plain.len());
    }

    #[test]
    fn with_one_pass_a_separate_last_pass_changes_nothing() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        let plain = generate(&o, M, &J, None).unwrap();
        o.last_pass = true;
        o.last_allowance = 0.05;
        assert_eq!(generate(&o, M, &J, None).unwrap(), plain);
    }

    #[test]
    fn a_single_pass_with_a_separate_last_pass_is_just_the_finish() {
        let mut o = op(OpKind::Profile, vec![Path::closed(rect([10.0, 10.0], 4.0, 4.0, 0.0))]);
        o.side = Side::Outside;
        o.tool.pass_depth = 10.0;
        o.cut_depth = 0.25;
        o.last_pass = true;
        o.last_allowance = 0.05;
        let moves = generate(&o, M, &J, None).unwrap();
        let r = o.tool.dia / 2.0;
        assert!(moves.iter().filter(|m| !m.rapid && m.p[2] < -1e-9).all(|m| (((m.p[0] - 10.0).abs() - 2.0).max(0.0).hypot(((m.p[1] - 10.0).abs() - 2.0).max(0.0)) - r).abs() < 0.02));
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

    fn cut(p: [f64; 3]) -> Move {
        Move { p, rapid: false, join: false }
    }

    #[test]
    fn tight_corners_are_cut_slower_and_long_sides_are_not() {
        // A 10 × 10 square at one depth, cut with a 0.25 tool.
        let mut moves = vec![Move { p: [0.0, 0.0, 0.2], rapid: true, join: false }, cut([0.0, 0.0, -0.1])];
        for p in [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]] {
            moves.push(cut([p[0], p[1], -0.1]));
        }
        let (out, slow) = slow_corners(&moves, 0.25, 0.5, 1.0);
        assert!(out.len() > moves.len(), "long sides split near the corners");
        let slowed: Vec<&Move> = out.iter().filter(|m| SlowMoves::share_at(Some(&slow), m.p) < 1.0).collect();
        assert!(slowed.iter().any(|m| m.p[0] == 10.0 && m.p[1] == 0.0), "the corner itself");
        // Each slow piece lies within one diameter of a corner.
        for w in out.windows(2) {
            if SlowMoves::share_at(Some(&slow), w[1].p) < 1.0 {
                let mid = [(w[0].p[0] + w[1].p[0]) / 2.0, (w[0].p[1] + w[1].p[1]) / 2.0];
                let near = [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].iter().any(|c: &[f64; 2]| (mid[0] - c[0]).hypot(mid[1] - c[1]) <= 0.25 + 1e-9);
                assert!(near, "{mid:?}");
            }
        }
        // The plunge keeps its own rate, and the path is the same shape.
        assert!(SlowMoves::share_at(Some(&slow), [0.0, 0.0, -0.1]) == 1.0 || out[1].p == [0.0, 0.0, -0.1]);
        assert_eq!(out.last().unwrap().p, [0.0, 0.0, -0.1]);
    }

    #[test]
    fn wide_curves_keep_their_rate_and_tight_ones_slow_down() {
        let ring = |r: f64| {
            let mut m = vec![Move { p: [r, 0.0, 0.2], rapid: true, join: false }, cut([r, 0.0, -0.1])];
            for k in 1..=72 {
                let a = std::f64::consts::TAU * k as f64 / 72.0;
                m.push(cut([r * a.cos(), r * a.sin(), -0.1]));
            }
            m
        };
        let (_, wide) = slow_corners(&ring(5.0), 0.25, 0.5, 1.0);
        assert!(wide.ends.is_empty(), "a 5 inch circle has no tight corner");
        let (_, tight) = slow_corners(&ring(0.15), 0.25, 0.5, 1.0);
        assert!(tight.ends.len() > 30, "a curve barely wider than the tool is slowed: {}", tight.ends.len());
    }

    #[test]
    fn the_machine_file_lowers_the_rate_near_corners() {
        let t = default_tools().into_iter().next().unwrap();
        let mut moves = vec![Move { p: [0.0, 0.0, 0.2], rapid: true, join: false }, cut([0.0, 0.0, -0.1])];
        for p in [[4.0, 0.0], [4.0, 4.0]] {
            moves.push(cut([p[0], p[1], -0.1]));
        }
        let (moves, slow) = slow_corners(&moves, t.dia, 0.5, 1.0);
        let text = gcode(&[PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "", slow: Some(&slow) }], &PostSettings { inches: true, origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs: None, blend: None });
        assert!(text.contains(&format!("F{:.1}", t.feed * 0.5)), "{text}");
        assert!(text.contains(&format!("F{:.1}", t.feed)));
    }

    #[test]
    fn the_estimate_counts_slower_corners() {
        let t = default_tools().into_iter().next().unwrap();
        let mut moves = vec![cut([0.0, 0.0, -0.1])];
        for p in [[4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]] {
            moves.push(cut([p[0], p[1], -0.1]));
        }
        let plain = estimate_seconds(&moves, &t, 200.0, 0.0);
        let (split, slow) = slow_corners(&moves, t.dia, 0.5, 1.0);
        assert!((estimate_with(&split, &t, 200.0, 0.0, None) - plain).abs() <= 1.0, "splitting alone changes nothing");
        assert!(estimate_with(&split, &t, 200.0, 0.0, Some(&slow)) > plain);
    }

    #[test]
    fn squared_distances_are_exact() {
        let (nx, ny) = (7, 5);
        let mut marked = vec![false; nx * ny];
        marked[2 * nx + 3] = true;
        let d = squared_distances(&marked, nx, ny);
        assert_eq!(d[2 * nx + 3], 0.0);
        assert_eq!(d[2 * nx + 6], 9.0);
        assert_eq!(d[4 * nx + 0], 13.0);
    }

    #[test]
    fn the_small_tool_clears_what_the_large_one_could_not_reach() {
        // A wide room with a narrow slot off it: 0.5 wide, too narrow for a 1 inch tool, wide
        // enough for a quarter inch tool to need more than its wall pass.
        let shape = Path::closed(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [2.25, 4.0], [2.25, 7.0], [1.75, 7.0], [1.75, 4.0], [0.0, 4.0]]);
        let mut o = op(OpKind::Pocket, vec![shape]);
        o.tool.dia = 0.125;
        o.rest_only = true;
        let mut big = o.tool.clone();
        big.dia = 1.0;
        o.clearance_tool = Some(big);
        let with_rest = generate(&o, M, &J, None).unwrap().to_vec();
        o.clearance_tool = None;
        let wall_only = generate(&o, M, &J, None).unwrap().to_vec();
        // The slot's middle line: the wall pass alone stays 0.0625 from the walls and can't reach it.
        let near_mid = |moves: &[Move]| moves.iter().any(|m| !m.rapid && (m.p[0] - 2.0).abs() < 0.12 && m.p[1] > 4.8 && m.p[1] < 6.5);
        assert!(!near_mid(&wall_only));
        assert!(near_mid(&with_rest), "the slot is cleared");
        // The open room, which the large tool reaches, isn't cut again in its middle.
        assert!(!with_rest.iter().any(|m| !m.rapid && (m.p[0] - 2.0).abs() < 0.5 && (m.p[1] - 2.0).abs() < 0.5));
    }

    #[test]
    fn adaptive_clearing_clears_the_pocket_without_heavy_bites() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 2.0, 0.0))]);
        o.tool.dia = 0.25;
        o.adaptive = true;
        let moves = generate(&o, M, &J, None).unwrap().to_vec();
        let cut = cuts(&moves);
        // It reaches the whole floor: every point well inside is passed near by the cutter.
        for (x, y) in [(5.0, 5.0), (4.0, 4.4), (6.2, 5.6), (3.8, 5.7), (6.3, 4.3)] {
            assert!(cut.iter().any(|p| p[2] < 0.0 && (p[0] - x).hypot(p[1] - y) < 0.13), "({x}, {y}) is left");
        }
        // Never closer to the wall than the cutter's radius.
        assert!(cut.iter().filter(|p| p[2] < 0.0).all(|p| (p[0] - 5.0).abs() <= 1.5 - 0.125 + 1e-3 && (p[1] - 5.0).abs() <= 1.0 - 0.125 + 1e-3));
        // Few lifts: most hops from one stretch to the next stay down.
        let lifts = moves.windows(2).filter(|w| w[1].rapid && w[1].p[2] > w[0].p[2] && w[0].p[2] < 0.0).count();
        assert!(lifts < 12, "{lifts} lifts");
        // Many small steps rather than a few rings.
        let plain = { let mut q = o.clone(); q.adaptive = false; cuts(&generate(&q, M, &J, None).unwrap()).len() };
        assert!(cut.len() > plain);
    }

    #[test]
    fn adaptive_clearing_comes_down_in_a_helix_with_ramping_on() {
        let mut o = op(OpKind::Pocket, vec![Path::closed(rect([5.0, 5.0], 3.0, 2.0, 0.0))]);
        o.tool.dia = 0.25;
        o.adaptive = true;
        o.ramp = true;
        o.ramp_kind = RampKind::Spiral;
        let moves = generate(&o, M, &J, None).unwrap().to_vec();
        // The first cut into the material, at each depth, goes down sideways round the helix.
        let downs: Vec<&[Move]> = moves.windows(2).filter(|w| !w[1].rapid && w[1].p[2] < w[0].p[2] - 1e-9 && w[1].p[2] < -1e-9).collect();
        let first = downs.first().unwrap();
        assert!((first[1].p[0] - first[0].p[0]).hypot(first[1].p[1] - first[0].p[1]) > 1e-6, "not a straight plunge");
        let sideways = downs.iter().filter(|w| (w[1].p[0] - w[0].p[0]).hypot(w[1].p[1] - w[0].p[1]) > 1e-6).count();
        assert!(sideways >= 48, "{sideways}");
    }
}

