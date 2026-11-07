//! Reads the drawing out of .crv3d project files, so a user's own jobs can come across.
//!
//! The format was worked out only from a user's own project files. Such a file is a
//! compound document holding named streams: the material in one, the vectors in another.

use crate::arcs::Step;
use crate::geom::{bow_points, Path, P};
use crate::toolpath::Move;
use std::collections::HashSet;

/// A layer as the file has it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    pub name: String,
    pub color: [u8; 3],
}

/// What could be read from a project file.
#[derive(Clone, Debug)]
pub struct Project {
    /// The material's lowest and highest corners, in the file's coordinates.
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// Where the origin is: 0 bottom left, 1 bottom right, 2 top right, 3 top left, 4 centre.
    pub datum: u32,
    pub layers: Vec<LayerInfo>,
    /// Each outline, and the layer it is on.
    pub shapes: Vec<(usize, Path)>,
    /// The mark each shape goes by, which toolpaths refer to it with.
    pub marks: Vec<[u8; 16]>,
    /// The groups on the sheet, with what is inside each.
    pub groups: Vec<SavedGroup>,
    pub toolpaths: Vec<SavedToolpath>,
}

/// A group of shapes in the drawing, on its layer if the file says which.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedGroup {
    pub layer: Option<usize>,
    pub items: Vec<SavedItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SavedItem {
    Shape([u8; 16]),
    Group(SavedGroup),
}

/// How a saved toolpath was cut with.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedTool {
    pub name: String,
    pub number: u32,
    pub dia: f64,
    pub pass_depth: f64,
    pub stepover: f64,
    pub feed: f64,
    pub plunge: f64,
    pub rpm: u32,
}

/// A toolpath as the project saved it, moves included.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedToolpath {
    pub name: String,
    /// "Profile Inside", "Drill", "Pocket" and so on.
    pub kind: String,
    pub tool: Option<SavedTool>,
    pub start_depth: f64,
    pub cut_depth: f64,
    pub allowance: f64,
    pub dwell: f64,
    /// The depth of each pass when the file lists them; empty otherwise.
    pub passes: Vec<f64>,
    pub options: SavedOptions,
    /// Which shapes it was made from.
    pub sources: Vec<[u8; 16]>,
    /// Each move's end, and whether it is a fast one. The first is the home position.
    pub moves: Vec<([f64; 3], bool)>,
    /// The same path with its curves as the true arcs the file records.
    pub steps: Vec<Step>,
    /// How high it travels and how close to the material it comes down fast.
    pub safe_z: f64,
    pub plunge_gap: f64,
    pub home: [f64; 3],
}

/// How a toolpath was set up, as far as this program can do the same. What the file leaves out is `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedOptions {
    pub climb: Option<bool>,
    pub tabs: bool,
    pub tab_length: Option<f64>,
    pub tab_thickness: Option<f64>,
    pub tab_count: Option<usize>,
    pub ramp: bool,
    /// 0 a straight slope, 1 zig-zag, 2 spiral.
    pub ramp_kind: Option<u32>,
    pub ramp_distance: Option<f64>,
    pub ramp_by_angle: bool,
    pub ramp_angle: Option<f64>,
    /// The longest a ramp set by its angle may be.
    pub ramp_angle_limit: Option<f64>,
    pub ramp_on_lead: bool,
    pub lead: bool,
    pub lead_circular: bool,
    pub lead_length: Option<f64>,
    pub lead_angle: Option<f64>,
    pub lead_radius: Option<f64>,
    pub lead_out: bool,
    pub overcut: Option<f64>,
    pub last_pass: bool,
    pub last_allowance: Option<f64>,
    pub last_reverse: bool,
    pub sharp_corners: bool,
    pub keep_starts: bool,
    /// Which point of a shape's box the start was moved nearest to, if one was chosen.
    pub nearest_start: Option<u32>,
    pub peck: bool,
    pub retract_gap: Option<f64>,
    pub raster: bool,
    pub raster_angle: Option<f64>,
    /// 0 no outline pass, 1 first, 2 last.
    pub profile_pass: Option<u32>,
    pub raster_allowance: Option<f64>,
    /// What's left for the last pass, if it was asked for.
    pub last_thickness: Option<f64>,
    pub keep_exact: bool,
    pub zigzag_open: bool,
    /// A pocket cleared with a second tool, which makes a toolpath of its own.
    pub area_clear: bool,
    /// The ways of ordering the shapes that were ticked: 10 order of selection, 0 left to right,
    /// 2 bottom to top, 6 grid, 8 shortest path.
    pub sequence: Vec<u32>,
    pub selection_order: bool,
    /// The flat bottom of a V-carve, when it has one.
    pub flat_depth: Option<f64>,
    /// Made in a way this program can't do again: an inlay, a nose cone, engraved or fluted.
    pub special: bool,
}

/// The ways of ordering shapes in a setting such as "V1|3|0|2|6|": a version, how many, then each.
fn sequence_list(text: &str) -> Vec<u32> {
    let mut parts = text.split('|');
    if parts.next() != Some("V1") {
        return Vec::new();
    }
    let Some(count) = parts.next().and_then(|n| n.parse::<usize>().ok()) else { return Vec::new() };
    // Codes above the last known one are ignored.
    parts.take(count).filter_map(|p| p.parse().ok()).filter(|code| *code <= 10).collect()
}

fn read_options(settings: &[(String, Setting)], kind: &str) -> SavedOptions {
    let get = |name: &str| settings.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let flag = |name: &str| matches!(get(name), Some(Setting::Flag(true)));
    let num = |name: &str| match get(name) {
        Some(Setting::Number(v)) => Some(*v),
        _ => None,
    };
    let whole = |name: &str| match get(name) {
        Some(Setting::Whole(v)) => Some(*v),
        _ => None,
    };
    // The settings of a neighbouring toolpath can be in reach, so only its own kind's names count.
    let pocket = kind == "Pocket";
    let drill = kind == "Drill";
    let text = |name: &str| match get(name) {
        Some(Setting::Text(t)) => Some(t.as_str()),
        _ => None,
    };
    SavedOptions {
        // Checked on the jobs' own moves: 0 turns the way a climb cut does, 1 the other way.
        climb: whole(if pocket { "_pkpdCutDirection" } else { "_ppdCutDirection" }).map(|d| d == 0),
        tabs: flag("_ppdUseTabs"),
        tab_length: num("_ppdTabLength"),
        tab_thickness: num("_ppdTabThickness"),
        tab_count: whole("tabsNumTabs").map(|n| n as usize),
        ramp: if pocket { flag("_pkpdDoRamping") } else { flag("_mcrdDoRamping") },
        ramp_kind: if pocket { None } else { whole("_mcrdRampingType") },
        ramp_distance: if pocket { num("_pkpdRampDistance") } else { num("_mcrdRampingDistance") },
        ramp_by_angle: !pocket && whole("_mcrdRampingConstraint") == Some(1),
        ramp_angle: num("_mcrdRampingAngle"),
        ramp_angle_limit: num("_mcrdRampingAngleMaxDistance"),
        ramp_on_lead: flag("_mcrdRampOnLeadIn"),
        // The file calls the "add leads" tick by this name.
        lead: flag("_mcldDoRamping"),
        lead_circular: whole("_mcldLeadType") == Some(1),
        lead_length: num("_mcldLeadLength"),
        lead_angle: num("_mcldLinearAngle"),
        lead_radius: num("_mcldCircularRadius"),
        lead_out: flag("_mcldDoLeadOut"),
        overcut: num("_mcldOvercutDistance"),
        last_pass: flag("lpaDoFullPass"),
        last_allowance: num("lpaAllowance"),
        last_reverse: flag("lpaReverse"),
        sharp_corners: flag("_ppdSquareCorners"),
        // Older files spell this one differently; where both are there, the newer spelling counts.
        keep_starts: match get("_ppdKeepStartPoints") {
            Some(Setting::Flag(v)) => *v,
            _ => flag("_ppdKeepStatrPoints"),
        },
        nearest_start: if flag("_ppdUseNearestPoint") { whole("_ppdNearestPointPosition") } else { None },
        peck: flag("_dpdPeckDrill"),
        retract_gap: num("_dpdRetractGap"),
        raster: flag("_pkpdDoRaster"),
        raster_angle: num("_pkpdRasterAngle"),
        // Newer files use the longer name; where there's both, it counts.
        profile_pass: whole("_pkpdProfilePassType").or(whole("_pkpdProfileType")),
        raster_allowance: num("_pkpdRasterAllowance"),
        last_thickness: if flag("_mctddUseLastPassThickness") { num("_mctddLastPassThickness") } else { None },
        keep_exact: flag("_mctddKeepToolStepdown"),
        zigzag_open: flag("_ppdZigZagOpen"),
        area_clear: flag("_pkpdUseAreaClearTool"),
        sequence: text("_mcsdSequenceOptions").map_or_else(Vec::new, sequence_list),
        selection_order: flag(if pocket { "_pkpdUseSelectionOrder" } else if drill { "_dpdUseSelectionOrder" } else { "_ppdUseSelectionOrder" }),
        flat_depth: if kind == "VCarveToolpath" && flag("_vcpdDoFlatBottom") { num("_vcpdFlatDepth") } else { None },
        special: if pocket {
            whole("_pkpdPocketMode").is_some_and(|m| m != 0) || flag("_pkpdDoEngraving") || flag("_pkpdUseNoseCone")
        } else if kind.starts_with("Profile") {
            whole("_ppdProfileMode").is_some_and(|m| m != 0) || flag("_ppdDoEngraving") || flag("_ppdUseNoseCone") || flag("_ppdDoFluting")
        } else {
            false
        },
    }
}

/// What a run of stored moves holds: small straight moves, the same with true arcs, the longest pause.
struct Stored {
    moves: Vec<([f64; 3], bool)>,
    steps: Vec<Step>,
    dwell: f64,
}

/// Follows the moves stored from `at` on, the first without its size in front of it.
/// Returns them and where they end.
fn read_moves(d: &[u8], at: usize, total: usize) -> Option<(Stored, usize)> {
    let end = at.checked_add(total)?;
    let mut out = Stored { moves: Vec::new(), steps: Vec::new(), dwell: 0.0 };
    let f32_at = |q: usize| -> Option<f64> {
        let v = f32::from_le_bytes(d.get(q..q + 4)?.try_into().ok()?) as f64;
        v.is_finite().then_some(v)
    };
    let corner = |q: usize| -> Option<[f64; 3]> { Some([f32_at(q)?, f32_at(q + 4)?, f32_at(q + 8)?]) };
    // A curve is a marker (kind 4) holding its end, then a head, then its many small sides.
    let mut arc_end: Option<[f64; 3]> = None;
    let mut sides_left = 0u32;
    // What each kind means: 2 fast travel, 8 going up, 9 and 10 at home are fast; 3 cuts, 7 goes down.
    // Others mark something without moving, or are the leftovers after the end.
    let apply = |out: &mut Stored, kind: u32, q: usize, arc_end: &mut Option<[f64; 3]>, sides_left: &mut u32| -> Option<()> {
        if kind >= 100 {
            return Some(());
        }
        let (from, to) = (corner(q)?, corner(q + 12)?);
        if kind == 4 {
            *arc_end = Some(to);
            return Some(());
        }
        let fast = matches!(kind, 2 | 8 | 9 | 10);
        let moved = (from[0] - to[0]).abs() + (from[1] - to[1]).abs() + (from[2] - to[2]).abs() > 1e-9;
        if !(fast || moved) {
            return Some(());
        }
        out.moves.push((to, fast));
        if *sides_left > 0 {
            *sides_left -= 1;
        } else {
            out.steps.push(Step::Straight(Move { p: to, rapid: fast }));
        }
        Some(())
    };
    let first = u32_at(d, at)?;
    if first != 9 {
        return None;
    }
    apply(&mut out, first, at + 4, &mut arc_end, &mut sides_left)?;
    let mut q = at + 28;
    while q < end {
        let size = u32_at(d, q)?;
        // A curve's marker with no head after it is a straight cut after all.
        if size != 0x58 {
            if let Some(to) = arc_end.take() {
                out.moves.push((to, false));
                out.steps.push(Step::Straight(Move { p: to, rapid: false }));
            }
        }
        match size {
            0x20 => {
                apply(&mut out, u32_at(d, q + 4)?, q + 8, &mut arc_end, &mut sides_left)?;
                q += 32;
            }
            0x58 => {
                let bulge = f64_at(d, q + 12)?;
                let centre = [f32_at(q + 20)?, f32_at(q + 24)?];
                let count = u32_at(d, q + 52)?;
                match arc_end.take() {
                    Some(end_at) if count > 0 && count < 1_000_000 => {
                        out.steps.push(Step::Arc { end: end_at, centre, counter_clockwise: bulge > 0.0 });
                        // This head holds the first side; the rest follow as usual.
                        sides_left = count;
                    }
                    _ => return None,
                }
                apply(&mut out, u32_at(d, q + 60)?, q + 64, &mut arc_end, &mut sides_left)?;
                q += 88;
            }
            0x28 => {
                // A pause, then the move after it.
                out.dwell = out.dwell.max(f64_at(d, q + 4)?);
                apply(&mut out, u32_at(d, q + 12)?, q + 16, &mut arc_end, &mut sides_left)?;
                q += 40;
            }
            _ => return None,
        }
    }
    (q == end + 28).then_some((out, q))
}

/// A named value among a toolpath's settings.
#[derive(Clone, Debug, PartialEq)]
enum Setting {
    Number(f64),
    Whole(u32),
    Flag(bool),
    Text(String),
}

fn text_at(d: &[u8], at: usize) -> Option<(String, usize)> {
    if d.get(at..at + 3)? != [0xFF, 0xFE, 0xFF] {
        return None;
    }
    let len = *d.get(at + 3)? as usize;
    let raw = d.get(at + 4..at + 4 + len * 2)?;
    let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&units), at + 4 + len * 2))
}

/// The settings written after a toolpath's moves: a name, a type, then a value.
fn read_settings(d: &[u8]) -> Vec<(String, Setting)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 < d.len() {
        if let Some((name, after)) = text_at(d, i) {
            let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if plain {
                let value = match u32_at(d, after) {
                    Some(0) => f64_at(d, after + 4).map(|v| (Setting::Number(v), after + 12)),
                    Some(1) => u32_at(d, after + 4).map(|v| (Setting::Whole(v), after + 8)),
                    Some(2) => d.get(after + 4).map(|v| (Setting::Flag(*v != 0), after + 5)),
                    Some(3) => text_at(d, after + 4).map(|(t, e)| (Setting::Text(t), e)),
                    _ => None,
                };
                if let Some((v, e)) = value {
                    out.push((name, v));
                    i = e;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

const SMALLEST_TOOL: f64 = 1e-3;

/// The tool's record just before a toolpath's moves, the last if there are several.
fn read_tool(d: &[u8]) -> Option<SavedTool> {
    let mut found = None;
    let mut i = 0;
    while i + 80 < d.len() {
        if d[i..i + 8] == [2, 0, 0, 0, 1, 0, 0, 0] && d[i + 12..i + 16] == [0, 0, 0, 0] && d[i + 17..i + 20] == [0, 0, 0] {
            let at = i + 21;
            let (dia, pass, step, feed, plunge) = (f64_at(d, at)?, f64_at(d, at + 8)?, f64_at(d, at + 16)?, f64_at(d, at + 24)?, f64_at(d, at + 32)?);
            let number = u32_at(d, at + 40)?;
            let rpm = u32_at(d, at + 44)?;
            let len = u32_at(d, at + 52)? as usize;
            // The same bytes turn up by chance elsewhere, with numbers no tool would have.
            if (SMALLEST_TOOL..1000.0).contains(&dia) && pass >= 0.0 && len < 200 {
                if let Some(raw) = d.get(at + 56..at + 56 + len) {
                    // The file's own text ends in a zero, which must not reach a machine file.
                    let name: String = raw.iter().take_while(|b| **b != 0).map(|b| *b as char).collect();
                    found = Some(SavedTool { name, number, dia, pass_depth: pass, stepover: step, feed, plunge, rpm });
                    i = at + 56 + len;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

/// Height, gap and home. They sit just before a group of moves, ahead of `at`,
/// and the home position is the one the moves start from.
fn read_heights(d: &[u8], at: usize, home: [f64; 3]) -> Option<(f64, f64)> {
    let mut wanted = [0u8; 12];
    for (i, v) in home.iter().enumerate() {
        wanted[i * 4..i * 4 + 4].copy_from_slice(&(*v as f32).to_le_bytes());
    }
    let from = at.saturating_sub(64);
    let window = d.get(from..at)?;
    let found = window.windows(12).rposition(|w| w == wanted)?;
    let place = (from + found).checked_sub(16)?;
    let (safe, gap) = (f64_at(d, place)?, f64_at(d, place + 8)?);
    (safe.is_finite() && gap.is_finite() && safe.abs() < 1e6 && gap.abs() < 1e6).then_some((safe, gap))
}

/// One group of stored moves found in the stream.
struct Group {
    start: usize,
    sources: Vec<[u8; 16]>,
    stored: Stored,
    end: usize,
}

/// Pockets list their shapes just ahead of their settings instead. Takes the nearest list there.
fn sources_ahead_of(d: &[u8], from: usize, head: usize) -> Vec<[u8; 16]> {
    for start in (from..head.saturating_sub(24)).rev() {
        if d[start..start + 5] != [2, 0, 0, 0, 0] {
            continue;
        }
        let Some(n) = u32_at(d, start + 5).map(|n| n as usize).filter(|n| *n >= 1) else { continue };
        let end = start + 9 + n.saturating_mul(16);
        if end <= head {
            return d[start + 9..end].chunks_exact(16).map(|c| c.try_into().unwrap_or([0; 16])).collect();
        }
    }
    Vec::new()
}

/// Finds the shapes a group says it came from, written just ahead of its size.
fn group_sources(d: &[u8], size_at: usize) -> (usize, Vec<[u8; 16]>) {
    for n in 0..20_000usize {
        let Some(start) = size_at.checked_sub(9 + n * 16) else { break };
        if d[start..start + 5] == [2, 0, 0, 0, 0] && u32_at(d, start + 5) == Some(n as u32) {
            let marks = d[start + 9..size_at].chunks_exact(16).map(|c| c.try_into().unwrap_or([0; 16])).collect();
            return (start, marks);
        }
    }
    (size_at, Vec::new())
}

fn find_groups(d: &[u8]) -> Vec<Group> {
    let mut groups = Vec::new();
    let mut i = 0;
    while i + 40 < d.len() {
        // A size, then the first record, always "at home", then the size of the next.
        if d[i + 4..i + 8] == [9, 0, 0, 0] && matches!(u32_at(d, i + 32), Some(0x20 | 0x58 | 0x28)) {
            if let Some(total) = u32_at(d, i).map(|t| t as usize).filter(|t| *t >= 28 && i + 8 + *t <= d.len() + 28) {
                if let Some((stored, end)) = read_moves(d, i + 4, total) {
                    let (start, sources) = group_sources(d, i);
                    groups.push(Group { start, sources, stored, end });
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    groups
}

/// Every toolpath in a project's toolpath stream. Most are one group of moves with their
/// settings after them. A pocket or V-carve has its settings first, then several groups.
fn read_toolpaths(d: &[u8]) -> Vec<SavedToolpath> {
    let groups = find_groups(d);
    let label: Vec<u8> = "ToolpathType".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let heads: Vec<usize> = d.windows(label.len()).enumerate().filter(|(_, w)| *w == label.as_slice()).map(|(i, _)| i.saturating_sub(4)).collect();
    // Which settings go with each group. A group's own settings follow it directly. Settings
    // with a tool between them and the group belong to the toolpath that starts there.
    let mut owner: Vec<Option<usize>> = Vec::with_capacity(groups.len());
    let mut follows: Vec<bool> = Vec::with_capacity(groups.len());
    for (k, g) in groups.iter().enumerate() {
        let next = groups.get(k + 1).map_or(d.len(), |n| n.start);
        // Parts of a pocket say nothing about their shapes, and never have settings of their own.
        let whole = !g.sources.is_empty();
        let after = heads.iter().position(|h| *h >= g.end && *h < next).filter(|i| whole && read_tool(&d[g.end..heads[*i]]).is_none());
        follows.push(after.is_some());
        let claimed = |i: usize| owner.iter().zip(&follows).any(|(o, f): (&Option<usize>, &bool)| *o == Some(i) && *f);
        let earlier = heads.iter().rposition(|h| *h < g.start).filter(|i| !claimed(*i));
        owner.push(after.or(earlier));
    }
    let mut out: Vec<SavedToolpath> = Vec::new();
    let mut seen: Vec<Option<usize>> = Vec::new();
    let mut before = 0;
    for (k, g) in groups.iter().enumerate() {
        let tool = read_tool(&d[before..g.start]);
        before = g.end;
        // Further groups of the same toolpath carry on from the one before.
        if k > 0 && owner[k].is_some() && seen.last() == Some(&owner[k]) {
            if let Some(last) = out.last_mut() {
                let trim = |moves: &[([f64; 3], bool)]| moves[1..moves.len().saturating_sub(1).max(1)].to_vec();
                last.moves.pop();
                last.steps.pop();
                last.moves.extend(trim(&g.stored.moves));
                last.moves.extend(g.stored.moves.last().copied());
                if g.stored.steps.len() > 1 {
                    last.steps.extend_from_slice(&g.stored.steps[1..]);
                }
                last.sources.extend_from_slice(&g.sources);
                last.dwell = last.dwell.max(g.stored.dwell);
                continue;
            }
        }
        seen.push(owner[k]);
        let next = groups.get(k + 1).map_or(d.len(), |n| n.start);
        let settings = owner[k].map_or_else(Vec::new, |index| {
            // Some settings, the name among them, come just ahead of the type.
            if follows[k] {
                return read_settings(&d[g.end..next]);
            }
            let head = heads[index];
            let mut found = read_settings(&d[head..next.min(head + 40_000)]);
            // Nearest first, and only back to where the toolpath before it ended.
            let back = if k > 0 { groups[k - 1].end.min(head) } else { head.saturating_sub(40_000) };
            let mut earlier = read_settings(&d[back.max(head.saturating_sub(40_000))..head]);
            earlier.reverse();
            found.extend(earlier);
            found
        });
        let find = |tail: &str| settings.iter().find(|(n, _)| n.ends_with(tail)).map(|(_, v)| v);
        let number = |tail: &str| match find(tail) {
            Some(Setting::Number(v)) => *v,
            _ => 0.0,
        };
        let name = match find("ToolpathName") {
            Some(Setting::Text(t)) => t.clone(),
            _ => format!("Toolpath {}", out.len() + 1),
        };
        let kind = match find("ToolpathType") {
            Some(Setting::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let uses_dwell = !matches!(find("UseDwell"), Some(Setting::Flag(false)));
        let home = g.stored.moves.first().map_or([0.0; 3], |m| m.0);
        let (safe_z, plunge_gap) = read_heights(d, g.start, home).unwrap_or((5.0, 1.0));
        let options = read_options(&settings, &kind);
        out.push(SavedToolpath {
            name,
            kind,
            options,
            tool,
            start_depth: number("StartDepth"),
            cut_depth: number("CutDepth"),
            // The last pass has an allowance of its own, written first.
            allowance: settings.iter().find_map(|(n, v)| match v {
                Setting::Number(a) if n.ends_with("Allowance") && !n.starts_with("lpa") && !n.contains("Raster") => Some(*a),
                _ => None,
            }).unwrap_or(0.0),
            dwell: if uses_dwell { g.stored.dwell } else { 0.0 },
            passes: settings.iter().find_map(|(_, v)| match v {
                Setting::Text(t) => pass_list(t, number("CutDepth")),
                _ => None,
            }).unwrap_or_default(),
            sources: if g.sources.is_empty() && !follows[k] {
                let back = if k > 0 { groups[k - 1].end } else { 0 };
                owner[k].map_or_else(Vec::new, |index| sources_ahead_of(d, back.min(heads[index]), heads[index]))
            } else {
                g.sources.clone()
            },
            moves: g.stored.moves.clone(),
            steps: g.stored.steps.clone(),
            safe_z,
            plunge_gap,
            home,
        });
    }
    out
}

/// Reads a list of pass depths such as "3.45;6.9;10.35;13.8;". Only one that goes down in steps
/// and ends exactly at the cut depth counts as one. Both count from the start depth.
fn pass_list(text: &str, cut: f64) -> Option<Vec<f64>> {
    if !text.contains(';') {
        return None;
    }
    let list: Vec<f64> = text.split(';').filter(|s| !s.trim().is_empty()).map(|s| s.trim().parse().ok()).collect::<Option<_>>()?;
    let (first, last) = (*list.first()?, *list.last()?);
    let rising = list.windows(2).all(|w| w[1] > w[0]);
    (rising && first > 0.0 && (last - cut).abs() < 1e-6).then_some(list)
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn f64_at(d: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

const END: u32 = 0xFFFF_FFFC;

/// A compound document: a small file system inside one file.
struct Compound<'a> {
    bytes: &'a [u8],
    sector: usize,
    mini_sector: usize,
    cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini: Vec<u8>,
    entries: Vec<Option<Entry>>,
}

#[derive(Clone)]
struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

impl<'a> Compound<'a> {
    fn sector_data(bytes: &'a [u8], size: usize, index: u32) -> Option<&'a [u8]> {
        // A sector past the end of a cut-short file reads as empty: what remains may still be whole.
        let from = (index as usize + 1).checked_mul(size)?;
        Some(bytes.get(from..(from + size).min(bytes.len())).unwrap_or(&[]))
    }

    /// Follows a chain of sectors, stopping at the end or at a loop in a damaged file.
    fn chain(fat: &[u32], start: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut at = start;
        while at < END && out.len() <= fat.len() {
            out.push(at);
            match fat.get(at as usize) {
                Some(next) => at = *next,
                None => break,
            }
        }
        out
    }

    fn open(bytes: &'a [u8]) -> Option<Compound<'a>> {
        if bytes.get(..8)? != [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
            return None;
        }
        let sector = 1usize << u16_at(bytes, 30)?.min(16);
        let mini_sector = 1usize << u16_at(bytes, 32)?.min(16);
        let dir_start = u32_at(bytes, 48)?;
        let cutoff = u32_at(bytes, 56)? as u64;
        let mini_fat_start = u32_at(bytes, 60)?;
        let mut difat_at = u32_at(bytes, 68)?;
        let difat_count = u32_at(bytes, 72)?;
        let mut fat_sectors: Vec<u32> = (0..109).filter_map(|i| u32_at(bytes, 76 + i * 4)).collect();
        for _ in 0..difat_count {
            let s = Self::sector_data(bytes, sector, difat_at)?;
            let words: Vec<u32> = s.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            let (next, rest) = words.split_last()?;
            fat_sectors.extend_from_slice(rest);
            difat_at = *next;
        }
        let words = |s: &[u8]| s.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<u32>>();
        let mut fat = Vec::new();
        for s in fat_sectors.into_iter().filter(|s| *s < END) {
            fat.extend(words(Self::sector_data(bytes, sector, s)?));
        }
        let mut directory = Vec::new();
        for s in Self::chain(&fat, dir_start) {
            directory.extend_from_slice(Self::sector_data(bytes, sector, s)?);
        }
        let mut entries = Vec::new();
        for e in directory.chunks_exact(128) {
            let len = u16_at(e, 64)? as usize;
            if len < 2 || len > 64 {
                entries.push(None);
                continue;
            }
            let name: Vec<u16> = e[..len - 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            entries.push(Some(Entry {
                name: String::from_utf16_lossy(&name),
                kind: e[66],
                left: u32_at(e, 68)?,
                right: u32_at(e, 72)?,
                child: u32_at(e, 76)?,
                start: u32_at(e, 116)?,
                size: u32_at(e, 120)? as u64,
            }));
        }
        let mut mini = Vec::new();
        if let Some(Some(root)) = entries.first() {
            for s in Self::chain(&fat, root.start) {
                mini.extend_from_slice(Self::sector_data(bytes, sector, s)?);
            }
        }
        let mut mini_fat = Vec::new();
        for s in Self::chain(&fat, mini_fat_start) {
            mini_fat.extend(words(Self::sector_data(bytes, sector, s)?));
        }
        Some(Compound { bytes, sector, mini_sector, cutoff, fat, mini_fat, mini, entries })
    }

    /// Looks for a name among the siblings under `node`, which are kept as a tree.
    fn find_among(&self, node: u32, name: &str, seen: &mut HashSet<u32>) -> Option<&Entry> {
        if !seen.insert(node) {
            return None;
        }
        let e = self.entries.get(node as usize)?.as_ref()?;
        if e.name == name {
            return Some(e);
        }
        self.find_among(e.left, name, seen).or_else(|| self.find_among(e.right, name, seen))
    }

    /// The contents of the stream at `folder/name`.
    fn stream(&self, folder: &str, name: &str) -> Option<Vec<u8>> {
        let root = self.entries.first()?.as_ref()?;
        let dir = self.find_among(root.child, folder, &mut HashSet::new())?;
        if !matches!(dir.kind, 1 | 5) {
            return None;
        }
        let e = self.find_among(dir.child, name, &mut HashSet::new())?;
        let size = e.size as usize;
        let mut out = Vec::with_capacity(size);
        if e.size < self.cutoff {
            for s in Self::chain(&self.mini_fat, e.start) {
                let from = s as usize * self.mini_sector;
                out.extend_from_slice(self.mini.get(from..(from + self.mini_sector).min(self.mini.len())).unwrap_or(&[]));
            }
        } else {
            for s in Self::chain(&self.fat, e.start) {
                out.extend_from_slice(Self::sector_data(self.bytes, self.sector, s)?);
            }
        }
        out.truncate(size);
        Some(out)
    }
}

/// Reads one outline at `at`, returning it and where it ends.
/// Everything is checked, so that bytes that only look like an outline are turned down.
fn outline_at(d: &[u8], at: usize) -> Option<(Path, usize)> {
    let version = u32_at(d, at)?;
    if !(4..=12).contains(&version) {
        return None;
    }
    if !(f64_at(d, at + 8)? > 0.0 && f64_at(d, at + 8)? < 10.0) {
        return None;
    }
    let count = u32_at(d, at + 26)? as usize;
    if count == 0 || count > 5_000_000 {
        return None;
    }
    let mut q = at + 30;
    let mut pts: Vec<P> = Vec::new();
    let point = |q: usize| -> Option<P> {
        let (x, y) = (f64_at(d, q)?, f64_at(d, q + 8)?);
        (x.is_finite() && y.is_finite() && x.abs() < 1e8 && y.abs() < 1e8).then_some([x, y])
    };
    for _ in 0..count {
        // Older files name each piece by its class, newer ones by one byte.
        let kind = if version >= 5 {
            q += 1;
            *d.get(q - 1)?
        } else {
            let tag = u16_at(d, q)?;
            q += if tag == 0xFFFF { 6 + u16_at(d, q + 4)? as usize } else { 2 };
            2
        };
        if u32_at(d, q)? != 2 {
            return None;
        }
        let flag = u32_at(d, q + 4)?;
        let (a, b) = (point(q + 8)?, point(q + 32)?);
        q += 56;
        if u32_at(d, q)? != 1 {
            return None;
        }
        q += 4;
        match pts.last() {
            None => pts.push(a),
            Some(l) if (l[0] - a[0]).hypot(l[1] - a[1]) > 0.05 => return None,
            _ => {}
        }
        // Old files say straight or curved by the flag instead.
        let arc = if version >= 5 { kind == 1 } else { flag == 0 };
        match (kind, arc) {
            (_, true) => {
                let bulge = f64_at(d, q)?;
                q += 8;
                if !bulge.is_finite() {
                    return None;
                }
                pts.extend(bow_points(a, b, bulge));
            }
            (2, false) if version >= 5 => {
                let (c1, c2) = (point(q)?, point(q + 16)?);
                q += 32;
                let span = (a[0] - c1[0]).hypot(a[1] - c1[1]) + (c1[0] - c2[0]).hypot(c1[1] - c2[1]) + (c2[0] - b[0]).hypot(c2[1] - b[1]);
                let chord = (a[0] - b[0]).hypot(a[1] - b[1]);
                // More steps the more it bends.
                let steps = (((span - chord).max(0.0) * 8.0).sqrt().ceil() as usize + 6).min(64);
                for i in 1..steps {
                    let t = i as f64 / steps as f64;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    pts.push([w[0] * a[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * b[0], w[0] * a[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * b[1]]);
                }
            }
            (0, false) | (2, false) => {}
            _ => return None,
        }
        pts.push(b);
    }
    let meets = (pts[0][0] - pts[pts.len() - 1][0]).hypot(pts[0][1] - pts[pts.len() - 1][1]) < 1e-3;
    if meets && pts.len() > 3 {
        pts.pop();
        return Some((Path::closed(pts), q));
    }
    Some((Path::open(pts), q))
}

/// Finds the layers: each one's place in the stream, its mark, its name and colour.
fn read_layers(d: &[u8]) -> Vec<(usize, [u8; 16], LayerInfo)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 24 < d.len() {
        // A small version number, 16 bytes of mark, then a name.
        if d[i] != 0 && d[i] < 10 && d[i + 1..i + 4] == [0, 0, 0] && d[i + 20..i + 23] == [0xFF, 0xFE, 0xFF] {
            let len = d[i + 23] as usize;
            let from = i + 24;
            if let Some(raw) = d.get(from..from + len * 2) {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let name = String::from_utf16_lossy(&units);
                let after = from + len * 2;
                if let Some(c) = d.get(after..after + 3) {
                    let mut mark = [0u8; 16];
                    mark.copy_from_slice(&d[i + 4..i + 20]);
                    out.push((i, mark, LayerInfo { name, color: [c[0], c[1], c[2]] }));
                    i = after;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn window_has(window: &[u8], mark: &[u8; 16]) -> bool {
    window.windows(16).any(|w| w == mark)
}

/// Reads what it can from a project file. An error says why nothing could be read.
/// A drawing object as the file records it: its own mark, then its layer's, then eight zeros.
struct Object {
    at: usize,
    mark: [u8; 16],
    layer: [u8; 16],
    /// How many objects follow that belong to this one, when it is a group.
    children: Option<usize>,
}

/// The place an object's record starts, given where its class is named or referred to.
fn record_after(d: &[u8], at: usize) -> Option<usize> {
    if d.get(at..at + 4)? == [0xFF, 0xFF, 1, 0] {
        let len = u16_at(d, at + 4)? as usize;
        return Some(at + 6 + len);
    }
    (u16_at(d, at)? & 0x8000 != 0).then_some(at + 2)
}

fn is_object(d: &[u8], at: usize, layers: &[[u8; 16]]) -> bool {
    matches!(u32_at(d, at), Some(7 | 8))
        && d.get(at + 36..at + 44) == Some(&[0u8; 8][..])
        && d.get(at + 20..at + 36).and_then(|m| <[u8; 16]>::try_from(m).ok()).is_some_and(|m| layers.contains(&m))
}

/// Every drawing object in the stream. A group is told by another object starting right after its head.
fn find_objects(d: &[u8], layers: &[[u8; 16]]) -> Vec<Object> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 44 <= d.len() {
        if !is_object(d, at, layers) {
            at += 1;
            continue;
        }
        let mark: [u8; 16] = d[at + 4..at + 20].try_into().unwrap_or([0; 16]);
        // Newer records have a version string with an identifier before the count.
        let after_head = if u32_at(d, at) == Some(8) {
            let marker = b"Vectric__Version";
            let window = &d[at + 44..(at + 340).min(d.len())];
            window.windows(marker.len()).position(|w| w == marker).map(|p| at + 44 + p + marker.len() + 8 + 36 + 4)
        } else {
            Some(at + 90 + 4)
        };
        let count = after_head.and_then(|p| Some((u32_at(d, p)? as usize, p + 4))).filter(|(n, _)| (1..1_000_000).contains(n));
        let children = count.filter(|(_, next)| record_after(d, *next).is_some_and(|r| is_object(d, r, layers))).map(|(n, _)| n);
        let layer: [u8; 16] = d[at + 20..at + 36].try_into().unwrap_or([0; 16]);
        out.push(Object { at, mark, layer, children });
        at += 44;
    }
    out
}

/// The shapes under each group, however deep. The objects a group owns follow it in order.
fn group_members(objects: &[Object]) -> std::collections::HashMap<[u8; 16], Vec<[u8; 16]>> {
    fn take(objects: &[Object], k: usize, into: &mut Vec<[u8; 16]>) -> usize {
        let Some(object) = objects.get(k) else { return k };
        let Some(count) = object.children else {
            into.push(object.mark);
            return k + 1;
        };
        let mut next = k + 1;
        for _ in 0..count {
            next = take(objects, next, into);
        }
        next
    }
    let mut members = std::collections::HashMap::new();
    for (k, object) in objects.iter().enumerate() {
        if object.children.is_some() {
            let mut list = Vec::new();
            take(objects, k, &mut list);
            members.insert(object.mark, list);
        }
    }
    members
}

/// The groups on the sheet: those not inside another. The objects a group owns follow it in order.
fn group_tree(objects: &[Object], layer_of: &dyn Fn(&[u8; 16]) -> Option<usize>) -> Vec<SavedGroup> {
    fn item(objects: &[Object], k: usize, layer_of: &dyn Fn(&[u8; 16]) -> Option<usize>) -> Option<(SavedItem, usize)> {
        let object = objects.get(k)?;
        let Some(count) = object.children else {
            return Some((SavedItem::Shape(object.mark), k + 1));
        };
        let mut items = Vec::new();
        let mut next = k + 1;
        for _ in 0..count {
            let Some((inner, after)) = item(objects, next, layer_of) else { break };
            items.push(inner);
            next = after;
        }
        Some((SavedItem::Group(SavedGroup { layer: layer_of(&object.layer), items }), next))
    }
    let mut groups = Vec::new();
    let mut k = 0;
    while let Some((found, next)) = item(objects, k, layer_of) {
        if let SavedItem::Group(group) = found {
            groups.push(group);
        }
        k = next;
    }
    groups
}

pub fn read(bytes: &[u8]) -> Result<Project, String> {
    let file = Compound::open(bytes).ok_or(crate::i18n::tr("This is not a project file this program can read."))?;
    let size = file.stream("VectorData", "MaterialSize").ok_or(crate::i18n::tr("This project has no material size."))?;
    let d = file.stream("VectorData", "2dDataV2").ok_or(crate::i18n::tr("This project is in a version this program can't read: it has no drawing where expected."))?;
    let number = |i: usize| f64_at(&size, 6 + i * 8);
    let (Some(a), Some(b), Some(c), Some(e), Some(f), Some(g)) = (number(0), number(1), number(2), number(3), number(4), number(5)) else {
        return Err(crate::i18n::tr("The material size in this project can't be read.").into());
    };
    let datum = u32_at(&size, 54).unwrap_or(0);
    if !(e > a && f > b && g > c) {
        return Err(crate::i18n::tr("The material size in this project can't be read.").into());
    }
    let found = read_layers(&d);
    let mut shapes: Vec<(usize, Path)> = Vec::new();
    let mut marks: Vec<[u8; 16]> = Vec::new();
    // Toolpath previews keep their own outlines in the same stream. Newer files mark each real
    // shape with a version string just before it; older ones have no previews to confuse it with.
    let marker = b"Vectric__Version";
    let marked = d.windows(marker.len()).any(|w| w == marker);
    let layer_for = |at: usize| -> usize {
        let window = &d[at.saturating_sub(220)..at];
        found
            .iter()
            .position(|(_, mark, _)| window_has(window, mark))
            .or_else(|| found.iter().rposition(|(place, _, _)| *place < at))
            .unwrap_or(0)
    };
    let layer_marks: Vec<[u8; 16]> = found.iter().map(|(_, m, _)| *m).collect();
    let objects = find_objects(&d, &layer_marks);
    let mut i = 0;
    while i + 8 < d.len() {
        let starts = d[i..i + 4] == [2, 0, 0, 0];
        if !starts {
            i += 1;
            continue;
        }
        if marked {
            // ...the marker, a number, 36 characters of identifier, then this.
            let good = i >= 44 + marker.len() && &d[i - 44 - marker.len()..i - 44] == marker;
            if !good {
                i += 1;
                continue;
            }
        }
        let tag = u16_at(&d, i + 4).unwrap_or(0);
        let body = if tag == 0xFFFF {
            let len = u16_at(&d, i + 8).unwrap_or(0) as usize;
            if d.get(i + 10..i + 10 + len) != Some(b"vdContour".as_slice()) {
                i += 1;
                continue;
            }
            i + 10 + len
        } else if tag & 0x8000 != 0 {
            i + 6
        } else {
            i += 1;
            continue;
        };
        match outline_at(&d, body) {
            Some((path, end)) => {
                let layer = layer_for(i);
                // A shape's own mark comes just before the mark of its layer.
                let window = &d[i.saturating_sub(220)..i];
                let by_layer = found.get(layer).and_then(|(_, m, _)| window.windows(16).rposition(|w| w == m)).and_then(|p| p.checked_sub(16)).and_then(|p| window.get(p..p + 16)).and_then(|m| m.try_into().ok()).unwrap_or([0; 16]);
                // The object this outline belongs to says so more surely, whatever its kind.
                let holder = objects.partition_point(|o| o.at < i).checked_sub(1).map(|k| &objects[k]).filter(|o| i - o.at < 220 && o.children.is_none());
                let mark = holder.map_or(by_layer, |o| o.mark);
                shapes.push((layer, path));
                marks.push(mark);
                i = end;
            }
            None => i += 1,
        }
    }
    // Only the layers that hold something; the file always has one for toolpath previews.
    let mut layers = Vec::new();
    let mut map = vec![usize::MAX; found.len().max(1)];
    for (layer, _) in &mut shapes {
        if map[*layer] == usize::MAX {
            map[*layer] = layers.len();
            layers.push(found.get(*layer).map_or_else(|| LayerInfo { name: "Layer 1".into(), color: [0, 0, 0] }, |f| f.2.clone()));
        }
        *layer = map[*layer];
    }
    let mut toolpaths = file.stream("Toolpaths", "ToolpathData").map_or_else(Vec::new, |t| read_toolpaths(&t));
    // A toolpath can name a whole group: it means the shapes in it.
    let layer_of = |mark: &[u8; 16]| found.iter().position(|(_, m, _)| m == mark).map(|p| map[p]).filter(|l| *l != usize::MAX);
    let tree = group_tree(&objects, &layer_of);
    let groups = group_members(&objects);
    for t in &mut toolpaths {
        t.sources = t.sources.iter().flat_map(|s| groups.get(s).cloned().unwrap_or_else(|| vec![*s])).collect();
    }
    Ok(Project { min: [a, b, c], max: [e, f, g], datum, layers, shapes, marks, groups: tree, toolpaths })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn dir_entry(name: &str, kind: u8, left: u32, right: u32, child: u32, start: u32, size: u32) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        let units: Vec<u16> = name.encode_utf16().collect();
        for (i, u) in units.iter().enumerate() {
            e[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        e[64..66].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
        e[66] = kind;
        e[68..72].copy_from_slice(&left.to_le_bytes());
        e[72..76].copy_from_slice(&right.to_le_bytes());
        e[76..80].copy_from_slice(&child.to_le_bytes());
        e[116..120].copy_from_slice(&start.to_le_bytes());
        e[120..124].copy_from_slice(&size.to_le_bytes());
        e
    }

    /// A compound file with its streams in two folders, written the plain way: no small-stream area.
    fn compound_with(material: &[u8], vectors: &[u8], toolpaths: &[u8]) -> Vec<u8> {
        const NONE: u32 = 0xFFFF_FFFF;
        let sectors = |n: usize| n.div_ceil(512).max(1);
        let (dir, mat, vec_, tps) = (2usize, sectors(material.len()), sectors(vectors.len()), sectors(toolpaths.len()));
        let fat_sectors = 1 + (dir + mat + vec_ + tps) / 128;
        let (dir_at, mat_at) = (fat_sectors, fat_sectors + dir);
        let vec_at = mat_at + mat;
        let tp_at = vec_at + vec_;
        let mut fat = vec![0xFFFF_FFFDu32; fat_sectors];
        for (start, n) in [(dir_at, dir), (mat_at, mat), (vec_at, vec_), (tp_at, tps)] {
            for k in 0..n {
                fat.push(if k + 1 == n { 0xFFFF_FFFE } else { (start + k + 1) as u32 });
            }
        }
        fat.resize(fat_sectors * 128, 0xFFFF_FFFF);
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        h[30..32].copy_from_slice(&9u16.to_le_bytes());
        h[32..34].copy_from_slice(&6u16.to_le_bytes());
        h[44..48].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
        h[48..52].copy_from_slice(&(dir_at as u32).to_le_bytes());
        h[60..64].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        h[68..72].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        for i in 0..109 {
            let v = if i < fat_sectors { i as u32 } else { NONE };
            h[76 + i * 4..80 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut out = h;
        for v in &fat {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let mut directory = Vec::new();
        directory.extend(dir_entry("Root Entry", 5, NONE, NONE, 1, 0xFFFF_FFFE, 0));
        directory.extend(dir_entry("VectorData", 1, 4, NONE, 2, 0, 0));
        directory.extend(dir_entry("MaterialSize", 2, 3, NONE, NONE, mat_at as u32, material.len() as u32));
        directory.extend(dir_entry("2dDataV2", 2, NONE, NONE, NONE, vec_at as u32, vectors.len() as u32));
        directory.extend(dir_entry("Toolpaths", 1, NONE, NONE, 5, 0, 0));
        directory.extend(dir_entry("ToolpathData", 2, NONE, NONE, NONE, tp_at as u32, toolpaths.len() as u32));
        directory.resize(dir * 512, 0);
        out.extend(directory);
        for s in [material, vectors, toolpaths] {
            let mut b = s.to_vec();
            b.resize(sectors(s.len()) * 512, 0);
            out.extend(b);
        }
        out
    }

    fn compound(material: &[u8], vectors: &[u8]) -> Vec<u8> {
        compound_with(material, vectors, &[])
    }

    pub(crate) fn sample_with_toolpath(material: &[u8]) -> Vec<u8> {
        compound_with(material, &sample(), &toolpath_stream(&SHAPE_MARK))
    }

    pub(crate) fn material(min: [f64; 3], max: [f64; 3], datum: u32) -> Vec<u8> {
        let mut m = vec![8, 0, 0, 0, 1, 1];
        for v in min.iter().chain(&max) {
            m.extend_from_slice(&v.to_le_bytes());
        }
        m.extend_from_slice(&datum.to_le_bytes());
        m
    }

    fn line(a: P, b: P) -> Vec<u8> {
        let mut s = vec![0u8];
        s.extend_from_slice(&2u32.to_le_bytes());
        s.extend_from_slice(&1u32.to_le_bytes());
        for v in [a[0], a[1], 0.0, b[0], b[1], 0.0] {
            s.extend_from_slice(&f64::to_le_bytes(v));
        }
        s.extend_from_slice(&1u32.to_le_bytes());
        s
    }

    fn arc(a: P, b: P, bulge: f64) -> Vec<u8> {
        let mut s = line(a, b);
        s[0] = 1;
        s[5..9].copy_from_slice(&0u32.to_le_bytes());
        s.extend_from_slice(&bulge.to_le_bytes());
        s
    }

    fn bezier(a: P, c1: P, c2: P, b: P) -> Vec<u8> {
        let mut s = line(a, b);
        s[0] = 2;
        for v in [c1[0], c1[1], c2[0], c2[1]] {
            s.extend_from_slice(&v.to_le_bytes());
        }
        s
    }

    /// A shape as the newer files keep it, with `mark` saying which layer it is on.
    fn shape(mark: &[u8; 16], spans: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; 4];
        r.extend_from_slice(&SHAPE_MARK);
        r.extend_from_slice(mark);
        r.extend_from_slice(&[0u8; 30]);
        r.extend_from_slice(&16u32.to_le_bytes());
        r.extend_from_slice(b"Vectric__Version");
        r.extend_from_slice(&1u32.to_le_bytes());
        r.extend_from_slice(&36u32.to_le_bytes());
        r.extend_from_slice(&[b'a'; 36]);
        r.extend_from_slice(&2u32.to_le_bytes());
        r.extend_from_slice(&0x8007u16.to_le_bytes());
        r.extend_from_slice(&7u32.to_le_bytes());
        r.extend_from_slice(&0u32.to_le_bytes());
        r.extend_from_slice(&0.001f64.to_le_bytes());
        r.push(0);
        r.extend_from_slice(&0f64.to_le_bytes());
        r.push(0);
        r.extend_from_slice(&(spans.len() as u32).to_le_bytes());
        for s in spans {
            r.extend_from_slice(s);
        }
        r.extend_from_slice(&[0x11; 16]);
        r
    }

    fn layer(mark: &[u8; 16], name: &str, color: [u8; 3]) -> Vec<u8> {
        let mut r = 4u32.to_le_bytes().to_vec();
        r.extend_from_slice(mark);
        r.extend_from_slice(&[0xFF, 0xFE, 0xFF, name.encode_utf16().count() as u8]);
        for u in name.encode_utf16() {
            r.extend_from_slice(&u.to_le_bytes());
        }
        r.extend_from_slice(&color);
        r.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0]);
        r
    }

    /// The mark of the first shape in `sample`, for a toolpath to refer to.
    pub(crate) const SHAPE_MARK: [u8; 16] = [0x5A; 16];

    pub(crate) fn sample() -> Vec<u8> {
        let (red, blue) = ([7u8; 16], [9u8; 16]);
        let mut v = 1u32.to_le_bytes().to_vec();
        v.extend(layer(&[3u8; 16], "Pr\u{e9}visualisation", [0, 0, 0]));
        // A toolpath preview: same outline format, but not a shape to import.
        let mut preview = 2u32.to_le_bytes().to_vec();
        preview.extend_from_slice(&0x8007u16.to_le_bytes());
        preview.extend(shape(&red, &[line([0.0, 0.0], [1.0, 1.0])])[80 + 44 + 16..].to_vec());
        v.extend(preview);
        v.extend(layer(&red, "per\u{e7}age", [255, 0, 0]));
        // A rectangle, 100 by 50, and a circle of four arcs around (50, -25), radius 10.
        v.extend(shape(&red, &[line([10.0, -10.0], [110.0, -10.0]), line([110.0, -10.0], [110.0, -60.0]), line([110.0, -60.0], [10.0, -60.0]), line([10.0, -60.0], [10.0, -10.0])]));
        let k = std::f64::consts::FRAC_PI_8.tan();
        v.extend(shape(&red, &[arc([60.0, -25.0], [50.0, -15.0], k), arc([50.0, -15.0], [40.0, -25.0], k), arc([40.0, -25.0], [50.0, -35.0], k), arc([50.0, -35.0], [60.0, -25.0], k)]));
        v.extend(layer(&blue, "courbes", [0, 0, 255]));
        // An open curve: a line then a Bezier.
        v.extend(shape(&blue, &[line([120.0, -10.0], [130.0, -10.0]), bezier([130.0, -10.0], [130.0, -30.0], [150.0, -30.0], [150.0, -10.0])]));
        v
    }

    /// The records of a toolpath: start at home, go over, down, cut, a curve, up, home again.
    pub(crate) fn toolpath_stream(source: &[u8; 16]) -> Vec<u8> {
        let rec = |kind: u32, a: [f32; 3], b: [f32; 3]| -> Vec<u8> {
            let mut r = 0x20u32.to_le_bytes().to_vec();
            r.extend_from_slice(&kind.to_le_bytes());
            for v in a.iter().chain(&b) {
                r.extend_from_slice(&v.to_le_bytes());
            }
            r
        };
        let mut body = rec(2, [0.0, 0.0, 20.0], [10.0, -10.0, 15.0]);
        body.extend(rec(15, [10.0, -10.0, 15.0], [10.0, -10.0, 15.0]));
        body.extend(rec(0, [10.0, -10.0, 15.0], [10.0, -10.0, 15.0]));
        body.extend(rec(7, [10.0, -10.0, 15.0], [10.0, -10.0, -6.0]));
        body.extend(rec(3, [10.0, -10.0, -6.0], [110.0, -10.0, -6.0]));
        // A curve: its marker, a head holding the first side, then one more side.
        body.extend(rec(4, [110.0, -10.0, -6.0], [120.0, -20.0, -6.0]));
        body.extend_from_slice(&0x58u32.to_le_bytes());
        body.extend_from_slice(&10.0f64.to_le_bytes());
        body.extend_from_slice(&0.4142f64.to_le_bytes());
        for v in [110.0f32, -20.0, -6.0, 117.0, -13.0, -6.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(&90.0f64.to_le_bytes());
        body.extend_from_slice(&2u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&3u32.to_le_bytes());
        for v in [110.0f32, -10.0, -6.0, 117.0, -13.0, -6.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend(rec(3, [117.0, -13.0, -6.0], [120.0, -20.0, -6.0]));
        // A pause, then going up.
        body.extend_from_slice(&0x28u32.to_le_bytes());
        body.extend_from_slice(&1.5f64.to_le_bytes());
        body.extend_from_slice(&8u32.to_le_bytes());
        for v in [120.0f32, -20.0, -6.0, 120.0, -20.0, 15.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend(rec(1, [120.0, -20.0, 15.0], [120.0, -20.0, 15.0]));
        body.extend(rec(16, [120.0, -20.0, 15.0], [120.0, -20.0, 15.0]));
        body.extend(rec(10, [0.0, 0.0, 20.0], [0.0, 0.0, 20.0]));
        // The tool, then height, gap and home, then the group of moves.
        let mut out = vec![5, 0, 0, 0, 1, 0, 0, 0];
        out.extend_from_slice(&[2, 0, 0, 0, 1, 0, 0, 0]);
        out.extend_from_slice(&1.5f32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 6, 0, 0, 0, 1]);
        for v in [3.0f64, 2.5, 1.2, 800.0, 200.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&18000u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(b"Fraise 3");
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&15.0f64.to_le_bytes());
        out.extend_from_slice(&2.0f64.to_le_bytes());
        for v in [0.0f32, 0.0, 20.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 0, 0, 1]);
        out.extend_from_slice(&0x8010u16.to_le_bytes());
        out.extend_from_slice(&[2, 0, 0, 0, 0]);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(source);
        // The size leaves out the first record, and counts through the last.
        let first = {
            let mut r = 9u32.to_le_bytes().to_vec();
            for v in [0.0f32, 0.0, 20.0, 0.0, 0.0, 20.0] {
                r.extend_from_slice(&v.to_le_bytes());
            }
            r
        };
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(first);
        out.extend(body);
        // Its settings.
        let text = |s: &str| -> Vec<u8> {
            let mut r = vec![0xFF, 0xFE, 0xFF, s.encode_utf16().count() as u8];
            for u in s.encode_utf16() {
                r.extend_from_slice(&u.to_le_bytes());
            }
            r
        };
        let mut setting = |name: &str, tail: Vec<u8>| {
            out.extend_from_slice(&0x800Bu16.to_le_bytes());
            out.extend_from_slice(&2u32.to_le_bytes());
            out.extend(text(name));
            out.extend(tail);
        };
        let number = |v: f64| { let mut r = 0u32.to_le_bytes().to_vec(); r.extend_from_slice(&v.to_le_bytes()); r };
        let words = |t: &str| { let mut r = 3u32.to_le_bytes().to_vec(); r.extend(text(t)); r };
        setting("ToolpathType", words("Profile Outside"));
        setting("_ppdToolpathName", words("Contour \u{e9}"));
        setting("_ppdStartDepth", number(0.0));
        setting("_ppdCutDepth", number(6.0));
        setting("_ppdAllowance", number(0.25));
        setting("_ppdUseTabs", vec![2, 0, 0, 0, 1]);
        setting("_ppdVersion", vec![1, 0, 0, 0, 12, 0, 0, 0]);
        out
    }

    fn object(version: u32, mark: u8, layer: u8, count: Option<u32>, next_is_object: bool) -> Vec<u8> {
        let mut r = vec![0x34, 0x80];
        r.extend_from_slice(&version.to_le_bytes());
        r.extend_from_slice(&[mark; 16]);
        r.extend_from_slice(&[layer; 16]);
        r.extend_from_slice(&[0; 8]);
        r.extend_from_slice(&[0x82, 0, 0, 0, 0x2A, 0, 0, 0, 9, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
        r.extend_from_slice(&[0; 16]);
        if version == 8 {
            r.extend_from_slice(&16u32.to_le_bytes());
            r.extend_from_slice(b"Vectric__Version");
            r.extend_from_slice(&[1, 0, 0, 0, 36, 0, 0, 0]);
            r.extend_from_slice(&[b'a'; 36]);
        } else {
            r.extend_from_slice(&[0; 4]);
        }
        // What kind it is, then how many it holds; a plain shape has its outline here instead.
        r.extend_from_slice(&1u32.to_le_bytes());
        r.extend_from_slice(&count.unwrap_or(0).to_le_bytes());
        if !next_is_object {
            r.extend_from_slice(&[0, 0]);
        }
        r
    }

    #[test]
    fn a_group_counts_as_the_shapes_inside_it_in_old_and_new_records() {
        for version in [7, 8] {
            let mut d = Vec::new();
            d.extend(object(version, 0xA0, 1, Some(2), true));
            d.extend(object(version, 0xB0, 1, None, false));
            d.extend(object(version, 0xB1, 1, None, false));
            d.extend(object(version, 0xB2, 1, None, false));
            let objects = find_objects(&d, &[[1; 16]]);
            let kinds: Vec<_> = objects.iter().map(|o| (o.mark[0], o.children)).collect();
            assert_eq!(kinds, [(0xA0, Some(2)), (0xB0, None), (0xB1, None), (0xB2, None)], "version {version}");
            let members = group_members(&objects);
            assert_eq!(members[&[0xA0; 16]], [[0xB0; 16], [0xB1; 16]], "the third is outside it");
            // Another layer's mark is not a layer of this drawing, so nothing here is an object.
            assert!(find_objects(&d, &[[2; 16]]).is_empty());
        }
    }

    #[test]
    fn the_groups_on_the_sheet_are_read_with_what_is_inside_them() {
        let mut d = Vec::new();
        d.extend(object(7, 0xB9, 1, None, false));
        d.extend(object(7, 0xA0, 1, Some(2), true));
        d.extend(object(7, 0xA1, 1, Some(2), true));
        d.extend(object(7, 0xB0, 1, None, false));
        d.extend(object(7, 0xB1, 1, None, false));
        d.extend(object(7, 0xB2, 1, None, false));
        d.extend(object(7, 0xB3, 1, None, false));
        let objects = find_objects(&d, &[[1; 16]]);
        let tree = group_tree(&objects, &|m| (*m == [1; 16]).then_some(4));
        let shape = |n: u8| SavedItem::Shape([n; 16]);
        let inner = SavedGroup { layer: Some(4), items: vec![shape(0xB0), shape(0xB1)] };
        assert_eq!(tree, [SavedGroup { layer: Some(4), items: vec![SavedItem::Group(inner), shape(0xB2)] }], "the shapes either side are outside it");
    }

    #[test]
    fn a_group_inside_a_group_gives_all_its_shapes() {
        let mut d = Vec::new();
        d.extend(object(7, 0xA0, 1, Some(2), true));
        d.extend(object(7, 0xA1, 1, Some(2), true));
        d.extend(object(7, 0xB0, 1, None, false));
        d.extend(object(7, 0xB1, 1, None, false));
        d.extend(object(7, 0xB2, 1, None, false));
        let members = group_members(&find_objects(&d, &[[1; 16]]));
        assert_eq!(members[&[0xA0; 16]], [[0xB0; 16], [0xB1; 16], [0xB2; 16]]);
        assert_eq!(members[&[0xA1; 16]], [[0xB0; 16], [0xB1; 16]]);
    }

    #[test]
    fn a_pocket_finds_its_shapes_ahead_of_its_settings() {
        let mut d = vec![9u8; 40];
        d.extend_from_slice(&[2, 0, 0, 0, 0, 2, 0, 0, 0]);
        d.extend_from_slice(&[0xC0; 16]);
        d.extend_from_slice(&[0xC1; 16]);
        d.extend_from_slice(&[1; 121]);
        let head = d.len();
        assert_eq!(sources_ahead_of(&d, 0, head), [[0xC0; 16], [0xC1; 16]]);
        assert!(sources_ahead_of(&d, 60, head).is_empty(), "not back past the toolpath before");
        // A count too large to fit before the settings isn't a list.
        let mut bad = vec![2, 0, 0, 0, 0, 200, 0, 0, 0];
        bad.extend_from_slice(&[0; 60]);
        let head = bad.len();
        assert!(sources_ahead_of(&bad, 0, head).is_empty());
    }

    #[test]
    fn the_ways_of_ordering_shapes_are_read_from_their_list() {
        assert_eq!(sequence_list("V1|3|0|2|6|"), [0, 2, 6]);
        assert_eq!(sequence_list("V1|4|10|0|2|6|"), [10, 0, 2, 6]);
        assert_eq!(sequence_list("V1|1|10|"), [10]);
        assert_eq!(sequence_list("V1|2|11|8|"), [8], "a code it doesn't know is left out");
        assert!(sequence_list("V2|1|10|").is_empty(), "another version isn't guessed at");
        assert!(sequence_list("").is_empty());
    }

    #[test]
    fn each_kind_of_ramp_and_special_toolpath_is_told_apart() {
        let list = |items: Vec<(&str, Setting)>| -> Vec<(String, Setting)> { items.into_iter().map(|(n, v)| (n.to_string(), v)).collect() };
        let o = read_options(&list(vec![
            ("_mcrdDoRamping", Setting::Flag(true)),
            ("_mcrdRampingType", Setting::Whole(2)),
            ("_mcrdRampingConstraint", Setting::Whole(1)),
            ("_mcrdRampingAngleMaxDistance", Setting::Number(100.0)),
            ("_mcsdSequenceOptions", Setting::Text("V1|3|0|2|6|".into())),
        ]), "Profile On");
        assert_eq!((o.ramp_kind, o.ramp_by_angle, o.ramp_angle_limit), (Some(2), true, Some(100.0)));
        assert_eq!(o.sequence, [0, 2, 6]);
        assert!(!o.special);
        assert!(read_options(&list(vec![("_ppdProfileMode", Setting::Whole(1))]), "Profile On").special, "an inlay");
        // Both spellings of keeping start points, the newer one counting where there are both.
        assert!(read_options(&list(vec![("_ppdKeepStatrPoints", Setting::Flag(true))]), "Profile On").keep_starts);
        assert!(read_options(&list(vec![("_ppdKeepStartPoints", Setting::Flag(true))]), "Profile On").keep_starts);
        assert!(!read_options(&list(vec![("_ppdKeepStatrPoints", Setting::Flag(true)), ("_ppdKeepStartPoints", Setting::Flag(false))]), "Profile On").keep_starts);
        // A chosen point only counts when it's asked for.
        let chosen = vec![("_ppdUseNearestPoint", Setting::Flag(true)), ("_ppdNearestPointPosition", Setting::Whole(3))];
        assert_eq!(read_options(&list(chosen), "Profile On").nearest_start, Some(3));
        let not_chosen = vec![("_ppdUseNearestPoint", Setting::Flag(false)), ("_ppdNearestPointPosition", Setting::Whole(3))];
        assert_eq!(read_options(&list(not_chosen), "Profile On").nearest_start, None);
        assert!(read_options(&list(vec![("_pkpdPocketMode", Setting::Whole(1))]), "Pocket").special);
        // Both names for the outline pass, the longer one counting where there are both.
        assert_eq!(read_options(&list(vec![("_pkpdProfileType", Setting::Whole(1))]), "Pocket").profile_pass, Some(1));
        let thick = vec![("_mctddUseLastPassThickness", Setting::Flag(true)), ("_mctddLastPassThickness", Setting::Number(0.2))];
        assert_eq!(read_options(&list(thick), "Profile On").last_thickness, Some(0.2));
        let unused = vec![("_mctddUseLastPassThickness", Setting::Flag(false)), ("_mctddLastPassThickness", Setting::Number(0.2))];
        assert_eq!(read_options(&list(unused), "Profile On").last_thickness, None);
        assert_eq!(read_options(&list(vec![("_pkpdProfileType", Setting::Whole(0)), ("_pkpdProfilePassType", Setting::Whole(2))]), "Pocket").profile_pass, Some(2));
        assert!(read_options(&list(vec![("_ppdDoFluting", Setting::Flag(true))]), "Profile Outside").special);
        // Settings of a neighbour are not this toolpath's.
        let v = list(vec![("_vcpdDoFlatBottom", Setting::Flag(true)), ("_vcpdFlatDepth", Setting::Number(2.0))]);
        assert_eq!(read_options(&v, "VCarveToolpath").flat_depth, Some(2.0));
        assert_eq!(read_options(&v, "Profile On").flat_depth, None);
    }

    #[test]
    fn options_are_read_by_their_exact_names() {
        let list = |items: Vec<(&str, Setting)>| -> Vec<(String, Setting)> { items.into_iter().map(|(n, v)| (n.to_string(), v)).collect() };
        let o = read_options(&list(vec![
            ("_ppdCutDirection", Setting::Whole(1)),
            ("_ppdUseTabs", Setting::Flag(true)),
            ("_ppdZigZagOpen", Setting::Flag(true)),
            ("_ppdTabLength", Setting::Number(12.0)),
            ("_mcrdDoRamping", Setting::Flag(false)),
            ("_mcldDoRamping", Setting::Flag(true)),
            ("_mcldLeadType", Setting::Whole(1)),
            ("_mcldLeadLength", Setting::Number(25.0)),
            ("_pkpdCutDirection", Setting::Whole(0)),
        ]), "Profile Outside");
        assert_eq!(o.climb, Some(false), "1 is the conventional way round");
        assert!(o.tabs && o.lead && o.lead_circular && !o.ramp, "{o:?}");
        assert_eq!((o.tab_length, o.lead_length, o.overcut), (Some(12.0), Some(25.0), None));
        assert!(o.zigzag_open);
        // A pocket reads its own names, even with a neighbouring profile's in reach.
        let climb = read_options(&list(vec![("_ppdCutDirection", Setting::Whole(1)), ("_pkpdCutDirection", Setting::Whole(0)), ("_mcrdDoRamping", Setting::Flag(false)), ("_pkpdDoRamping", Setting::Flag(true)), ("_pkpdRampDistance", Setting::Number(3.0))]), "Pocket");
        assert_eq!((climb.climb, climb.ramp, climb.ramp_distance), (Some(true), true, Some(3.0)));
    }

    #[test]
    fn a_list_of_passes_is_only_taken_when_it_fits_the_depths() {
        assert_eq!(pass_list("3.45;6.9;10.35;13.8;", 13.8), Some(vec![3.45, 6.9, 10.35, 13.8]));
        assert_eq!(pass_list("3.45;6.9;", 13.8), None, "ends somewhere else");
        assert_eq!(pass_list("6.9;3.45;13.8;", 13.8), None, "out of order");
        assert_eq!(pass_list("Profile;Inside;", 13.8), None);
        assert_eq!(pass_list("13.8", 13.8), None);
    }

    #[test]
    fn a_saved_toolpath_comes_with_its_moves_tool_and_settings() {
        let list = read_toolpaths(&toolpath_stream(&[0x42; 16]));
        assert_eq!(list.len(), 1);
        let t = &list[0];
        assert_eq!(t.name, "Contour \u{e9}");
        assert_eq!(t.kind, "Profile Outside");
        assert_eq!((t.start_depth, t.cut_depth, t.allowance), (0.0, 6.0, 0.25));
        assert_eq!(t.dwell, 1.5);
        assert_eq!(t.sources, vec![[0x42; 16]]);
        let tool = t.tool.as_ref().expect("a tool");
        assert_eq!(tool.name, "Fraise 3");
        assert_eq!((tool.dia, tool.pass_depth, tool.stepover, tool.feed, tool.plunge, tool.number, tool.rpm), (3.0, 2.5, 1.2, 800.0, 200.0, 3, 18000));
        assert_eq!((t.safe_z, t.plunge_gap, t.home), (15.0, 2.0, [0.0, 0.0, 20.0]));
        let m = |x: f64, y: f64, z: f64, fast: bool| ([x, y, z], fast);
        assert_eq!(t.moves, vec![
            m(0.0, 0.0, 20.0, true),
            m(10.0, -10.0, 15.0, true),
            m(10.0, -10.0, -6.0, false),
            m(110.0, -10.0, -6.0, false),
            m(117.0, -13.0, -6.0, false),
            m(120.0, -20.0, -6.0, false),
            m(120.0, -20.0, 15.0, true),
            m(0.0, 0.0, 20.0, true),
        ]);
        // The curve stays one true arc in the other list.
        let arcs: Vec<_> = t.steps.iter().filter(|s| matches!(s, Step::Arc { .. })).collect();
        assert_eq!(arcs.len(), 1);
        assert!(matches!(arcs[0], Step::Arc { end, .. } if *end == [120.0, -20.0, -6.0]));
        assert_eq!(t.steps.len(), t.moves.len() - 1, "two small sides became one arc");
    }

    #[test]
    fn a_pocket_is_one_toolpath_made_of_several_groups() {
        // Its settings first, then two groups with nothing after them but the next toolpath.
        let group = |x: f32| -> Vec<u8> {
            let mut out = Vec::new();
            for v in [15.0f64, 2.0] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in [0.0f32, 0.0, 20.0] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            let rec = |kind: u32, a: [f32; 3], b: [f32; 3]| -> Vec<u8> {
                let mut r = 0x20u32.to_le_bytes().to_vec();
                r.extend_from_slice(&kind.to_le_bytes());
                for v in a.iter().chain(&b) {
                    r.extend_from_slice(&v.to_le_bytes());
                }
                r
            };
            let mut body = rec(2, [0.0, 0.0, 20.0], [x, 0.0, 15.0]);
            body.extend(rec(7, [x, 0.0, 15.0], [x, 0.0, -3.0]));
            body.extend(rec(3, [x, 0.0, -3.0], [x + 5.0, 0.0, -3.0]));
            body.extend(rec(8, [x + 5.0, 0.0, -3.0], [x + 5.0, 0.0, 15.0]));
            body.extend(rec(10, [0.0, 0.0, 20.0], [0.0, 0.0, 20.0]));
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&9u32.to_le_bytes());
            for v in [0.0f32, 0.0, 20.0, 0.0, 0.0, 20.0] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend(body);
            out.extend_from_slice(&[0u8; 2000]);
            out
        };
        let text = |s: &str| -> Vec<u8> {
            let mut r = vec![0xFF, 0xFE, 0xFF, s.encode_utf16().count() as u8];
            for u in s.encode_utf16() {
                r.extend_from_slice(&u.to_le_bytes());
            }
            r
        };
        let mut d = Vec::new();
        for (name, tail) in [("ToolpathType", "Pocket"), ("_pkpdToolpathName", "Poche 1")] {
            d.extend(text(name));
            d.extend_from_slice(&3u32.to_le_bytes());
            d.extend(text(tail));
        }
        d.extend_from_slice(&[0u8; 100]);
        d.extend(group(10.0));
        d.extend(group(30.0));
        let list = read_toolpaths(&d);
        assert_eq!(list.len(), 1, "{:?}", list.iter().map(|t| &t.name).collect::<Vec<_>>());
        let t = &list[0];
        assert_eq!((t.name.as_str(), t.kind.as_str()), ("Poche 1", "Pocket"));
        assert_eq!((t.safe_z, t.plunge_gap), (15.0, 2.0));
        let xs: Vec<f64> = t.moves.iter().map(|m| m.0[0]).collect();
        assert_eq!(xs, [0.0, 10.0, 10.0, 15.0, 15.0, 30.0, 30.0, 35.0, 35.0, 0.0], "no trip home between the two");
    }

    #[test]
    fn damaged_toolpath_records_are_left_out_without_a_crash() {
        let good = toolpath_stream(&[1; 16]);
        for keep in (0..good.len()).step_by(37) {
            let _ = read_toolpaths(&good[..keep]);
        }
        let mut bad = good.clone();
        for b in bad.iter_mut().skip(150).step_by(11) {
            *b = 0xEE;
        }
        let _ = read_toolpaths(&bad);
        assert!(read_toolpaths(b"nothing here at all").is_empty());
    }

    pub(crate) fn sample_compound(material: &[u8], vectors: &[u8]) -> Vec<u8> {
        compound(material, vectors)
    }

    fn sample_file() -> Vec<u8> {
        compound(&material([0.0, -200.0, -13.0], [300.0, 0.0, 0.0], 3), &sample())
    }

    #[test]
    fn the_material_and_where_its_origin_is_are_read() {
        let p = read(&sample_file()).unwrap();
        assert_eq!(p.min, [0.0, -200.0, -13.0]);
        assert_eq!(p.max, [300.0, 0.0, 0.0]);
        assert_eq!(p.datum, 3);
    }

    #[test]
    fn shapes_come_in_with_their_kind_and_size() {
        let p = read(&sample_file()).unwrap();
        assert_eq!(p.shapes.len(), 3, "the preview's outline is left out");
        let rect = &p.shapes[0].1;
        assert!(rect.closed);
        assert_eq!(rect.pts.len(), 4);
        assert_eq!(rect.pts[0], [10.0, -10.0]);
        // Positive bulge turns anticlockwise, so this is a true circle around (50, -25).
        let circle = &p.shapes[1].1;
        assert!(circle.closed);
        let (c, r) = crate::bulk::as_circle(circle).expect("seen as a circle");
        assert!((c[0] - 50.0).abs() < 1e-6 && (c[1] + 25.0).abs() < 1e-6, "{c:?}");
        assert!((r - 10.0).abs() < 1e-6, "{r}");
        for q in &circle.pts {
            assert!(((q[0] - 50.0).hypot(q[1] + 25.0) - 10.0).abs() < 1e-6, "{q:?}");
        }
        let open = &p.shapes[2].1;
        assert!(!open.closed);
        assert_eq!(open.pts[0], [120.0, -10.0]);
        assert_eq!(*open.pts.last().unwrap(), [150.0, -10.0]);
        // A Bezier with these controls dips three quarters of the way to them.
        let low = open.pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min);
        assert!((low + 25.0).abs() < 0.1, "{low}");
    }

    #[test]
    fn layers_keep_their_names_and_colours_and_empty_ones_stay_behind() {
        let p = read(&sample_file()).unwrap();
        let names: Vec<&str> = p.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["per\u{e7}age", "courbes"]);
        assert_eq!(p.layers[0].color, [255, 0, 0]);
        assert_eq!(p.layers[1].color, [0, 0, 255]);
        assert_eq!(p.shapes.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 0, 1]);
    }

    #[test]
    fn older_files_with_named_pieces_are_read_too() {
        // Version 4: each piece begins with its class, new the first time and a reference after.
        let mut v = layer(&[5u8; 16], "Couche 1", [0, 0, 0]);
        v.extend_from_slice(&[0u8; 24]);
        v.extend_from_slice(&2u32.to_le_bytes());
        v.extend_from_slice(&0xFFFFu16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&9u16.to_le_bytes());
        v.extend_from_slice(b"vdContour");
        v.extend_from_slice(&4u32.to_le_bytes());
        v.extend_from_slice(&u32::MAX.to_le_bytes());
        v.extend_from_slice(&0.001f64.to_le_bytes());
        v.push(0);
        v.extend_from_slice(&0f64.to_le_bytes());
        v.push(0);
        v.extend_from_slice(&3u32.to_le_bytes());
        for (i, (a, b)) in [([0.0, 0.0], [4.0, 0.0]), ([4.0, 0.0], [4.0, 3.0]), ([4.0, 3.0], [0.0, 0.0])].into_iter().enumerate() {
            if i == 0 {
                v.extend_from_slice(&0xFFFFu16.to_le_bytes());
                v.extend_from_slice(&1u16.to_le_bytes());
                v.extend_from_slice(&10u16.to_le_bytes());
                v.extend_from_slice(b"vdLineSpan");
            } else {
                v.extend_from_slice(&0x8008u16.to_le_bytes());
            }
            v.extend_from_slice(&line(a, b)[1..]);
        }
        let p = read(&compound(&material([0.0, 0.0, -3.0], [10.0, 10.0, 0.0], 0), &v)).unwrap();
        assert_eq!(p.shapes.len(), 1);
        assert!(p.shapes[0].1.closed);
        assert_eq!(p.shapes[0].1.pts.len(), 3);
    }

    #[test]
    fn damaged_and_foreign_files_are_explained_not_crashed_on() {
        assert!(read(b"not a project").unwrap_err().contains("not a project file"));
        assert!(read(&[]).is_err());
        let good = sample_file();
        // Cut short anywhere, it either reads what it can or explains.
        for keep in [100, 511, 600, 1024, 1536, 2048, good.len() - 300] {
            let _ = read(&good[..keep]);
        }
        // Scrambled bytes in the vectors stop that shape, without a panic.
        let mut bad = sample();
        for b in bad.iter_mut().skip(200).step_by(7) {
            *b = 0xFF;
        }
        let _ = read(&compound(&material([0.0, 0.0, -3.0], [10.0, 10.0, 0.0], 0), &bad));
        // A material with no size is refused.
        assert!(read(&compound(&material([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0), &sample())).unwrap_err().contains("material"));
    }

    #[test]
    fn a_circle_going_the_other_way_is_still_a_circle() {
        let k = -std::f64::consts::FRAC_PI_8.tan();
        let mark = [7u8; 16];
        let mut v = layer(&mark, "a", [0, 0, 0]);
        v.extend(shape(&mark, &[arc([10.0, 0.0], [0.0, -10.0], k), arc([0.0, -10.0], [-10.0, 0.0], k), arc([-10.0, 0.0], [0.0, 10.0], k), arc([0.0, 10.0], [10.0, 0.0], k)]));
        let p = read(&compound(&material([-20.0, -20.0, -1.0], [20.0, 20.0, 0.0], 4), &v)).unwrap();
        let (c, r) = crate::bulk::as_circle(&p.shapes[0].1).unwrap();
        assert!(c[0].abs() < 1e-6 && c[1].abs() < 1e-6 && (r - 10.0).abs() < 1e-6);
    }
}
