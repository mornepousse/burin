//! Machine files for PlanetCNC, laid out like the jobs that machine already runs.
//! All of those use one tool to a file, so what happens between two tools is a careful guess.

use crate::arcs::{fit_arcs, Step};
use crate::toolpath::{PostPath, Tool};
use std::fmt::Write;

/// The name it has in the list of machine file formats.
pub const NAME: &str = "PlanetCNC (mm) (*.gcode)";

const LINE: &str = "\r\n";

pub struct PlanetJob<'a> {
    pub name: &'a str,
    pub created: String,
    /// Width, height and thickness of the material, in millimetres.
    pub material: [f64; 3],
    /// Where the machine's origin sits, in job coordinates.
    pub origin: [f64; 2],
    /// Added to Z: the thickness when zero is on the machine bed.
    pub z_shift: f64,
    /// Job units to millimetres.
    pub scale: f64,
    /// When set, curves are written as true arcs, following them within this distance.
    pub arcs: Option<f64>,
}

/// A number with three decimals, never "-0.000".
fn mm(v: f64) -> String {
    let text = format!("{v:.3}");
    if text == "-0.000" {
        "0.000".to_string()
    } else {
        text
    }
}

/// Keeps what the machine already knows, so that only what changes is written.
#[derive(Default)]
struct Written {
    axis: [Option<String>; 3],
    feed: Option<String>,
}

impl Written {
    /// " X1.000" when that axis changes, nothing otherwise.
    fn axis(&mut self, index: usize, value: f64) -> String {
        let text = mm(value);
        if self.axis[index].as_deref() == Some(text.as_str()) {
            return String::new();
        }
        let word = format!(" {}{text}", ['X', 'Y', 'Z'][index]);
        self.axis[index] = Some(text);
        word
    }

    fn feed(&mut self, value: f64) -> String {
        let text = format!("{value:.1}");
        if self.feed.as_deref() == Some(text.as_str()) {
            return String::new();
        }
        let word = format!(" F{text}");
        self.feed = Some(text);
        word
    }
}

/// The number a tool goes by: its own, or its place in the list when it has none.
fn tool_number(tools: &[&Tool], tool: &Tool) -> u32 {
    if tool.number > 0 {
        return tool.number;
    }
    tools.iter().position(|t| *t == tool).map_or(1, |i| i as u32 + 1)
}

pub fn gcode(paths: &[PostPath], job: &PlanetJob) -> String {
    let k = job.scale;
    let mut s = String::new();
    let mut tools: Vec<&Tool> = Vec::new();
    for p in paths {
        if !tools.contains(&p.tool) {
            tools.push(p.tool);
        }
    }
    let _ = write!(s, "({}){LINE}", job.name);
    let _ = write!(s, "(Created: {}){LINE}", job.created);
    let _ = write!(s, "(Material Size: X={}, Y={}, Z={}){LINE}", mm(job.material[0]), mm(job.material[1]), mm(job.material[2]));
    let _ = write!(s, "(Tools:){LINE}");
    for t in &tools {
        let _ = write!(s, "({} = {}){LINE}", tool_number(&tools, t), t.name);
    }
    let _ = write!(s, "{LINE}G21 G17 G40 G49 G90 G91.1 G94{LINE}{LINE}");
    let mut current: Option<&Tool> = None;
    let mut written = Written::default();
    for path in paths {
        let tool = path.tool;
        if current != Some(tool) {
            if current.is_some() {
                let _ = write!(s, "M5{LINE}G49{LINE}");
            }
            let n = tool_number(&tools, tool);
            let _ = write!(s, "T{n} M6 ({}){LINE}G43 H{n}{LINE}S{:.2} M3{LINE}", tool.name, tool.rpm);
            current = Some(tool);
            written = Written::default();
        }
        let _ = write!(s, "({}){LINE}", path.name);
        let moves = crate::toolpath::working_moves(path.moves);
        let steps: Vec<Step> = match (job.arcs, path.steps) {
            (Some(_), Some(exact)) => crate::toolpath::working_steps(exact).to_vec(),
            (Some(tol), None) => fit_arcs(moves, tol),
            _ => moves.iter().map(|m| Step::Straight(*m)).collect(),
        };
        let mut prev = path.moves.first().map_or([0.0; 3], |m| m.p);
        for (i, step) in steps.iter().enumerate() {
            let m = match step {
                Step::Straight(m) => m,
                Step::Arc { end, centre, counter_clockwise } => {
                    let (x, y) = ((end[0] - job.origin[0]) * k, (end[1] - job.origin[1]) * k);
                    // Arcs always name both ends of their move, and their centre from where they start.
                    let (ci, cj) = ((centre[0] - prev[0]) * k, (centre[1] - prev[1]) * k);
                    written.axis[0] = Some(mm(x));
                    written.axis[1] = Some(mm(y));
                    // Only an arc that also goes down says so; the flat ones stay as in their files.
                    let height = written.axis(2, (end[2] + job.z_shift) * k);
                    let feed = written.feed(tool_feed(tool, k));
                    let _ = write!(s, "{} X{} Y{}{height} I{} J{}{feed}{LINE}", if *counter_clockwise { "G03" } else { "G02" }, mm(x), mm(y), mm(ci), mm(cj));
                    prev = *end;
                    continue;
                }
            };
            let (x, y, z) = ((m.p[0] - job.origin[0]) * k, (m.p[1] - job.origin[1]) * k, (m.p[2] + job.z_shift) * k);
            let words = format!("{}{}{}", written.axis(0, x), written.axis(1, y), written.axis(2, z));
            if m.rapid {
                if !words.is_empty() {
                    let _ = write!(s, "G00{words}{LINE}");
                }
            } else {
                let flat = (m.p[0] - prev[0]).hypot(m.p[1] - prev[1]);
                let rate = if flat < (m.p[2] - prev[2]).abs() { tool.plunge } else { tool.feed } * k;
                if !words.is_empty() {
                    // Going straight down always states its speed, as every plunge in their files does.
                    if flat < 1e-9 {
                        written.feed = None;
                    }
                    let feed = written.feed(rate);
                    let _ = write!(s, "G01{words}{feed}{LINE}");
                }
                if path.dwell > 0.0 && flat < 1e-9 && matches!(steps.get(i + 1), Some(Step::Straight(n)) if n.rapid) {
                    let _ = write!(s, "G04 P{:.2}{LINE}", path.dwell);
                }
            }
            prev = m.p;
        }
    }
    let _ = write!(s, "M5{LINE}G49{LINE}G53 G00 Z#<_tc_safeheight>{LINE}M2{LINE}");
    s
}

fn tool_feed(tool: &Tool, k: f64) -> f64 {
    tool.feed * k
}

/// The bytes to save: one byte per accented letter, as in the file this format comes from.
pub fn file_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect()
}

#[cfg(test)]
mod tests {
    use crate::toolpath::Move;
    use super::*;
    use crate::geom::{circle, rect, Path};
    use crate::toolpath::{blank_op, default_tools, generate, JobSize, Machine, OpKind};

    /// The start and end of a real job for this machine.
    const THEIR_FILE: &str = "(sample_job)\r\n(Created: Wednesday June 03 2026 09:47 AM)\r\n(Material Size: X=940.000, Y=1180.000, Z=13.000)\r\n(Tools:)\r\n(1 = End Mill (0.125 inch))\r\n\r\nG21 G17 G40 G49 G90 G91.1 G94\r\n\r\nT1 M6 (End Mill (0.125 inch))\r\nG43 H1\r\nS17000.00 M3\r\n(Trous travessants)\r\nG00 X39.924 Y-14.510 Z15.000\r\nG00 Z2.000\r\nG01 Z-3.450 F300.0\r\nG02 X40.337 Y-14.922 I0.000 J-0.412 F800.0\r\nG02 X39.924 Y-15.335 I-0.412 J0.000\r\nG01 Y-9.607 F800.0\r\nG01 X87.321\r\nG00 Z15.000\r\nG00 X73.012 Y-14.922\r\nM5\r\nG49\r\nG53 G00 Z#<_tc_safeheight>\r\nM2\r\n";

    fn tool() -> Tool {
        let mut t = default_tools().into_iter().find(|t| t.metric).unwrap();
        t.name = "End Mill (3 mm)".into();
        t.number = 1;
        t.dia = 3.0;
        t.pass_depth = 3.45;
        t.feed = 800.0;
        t.plunge = 300.0;
        t.rpm = 17000.0;
        t
    }

    fn machine() -> Machine {
        Machine { safe_z: 15.0, home: [0.0, 0.0, 20.0], plunge_gap: 2.0, gap_above: 0.0, gap_below: None }
    }

    fn job(arcs: Option<f64>) -> PlanetJob<'static> {
        PlanetJob { name: "sample", created: "2026-11-05 10:00".into(), material: [300.0, 200.0, 13.0], origin: [0.0, 0.0], z_shift: 0.0, scale: 1.0, arcs }
    }

    #[test]
    fn an_arc_that_goes_down_says_how_far() {
        let t = tool();
        let start = Move { p: [10.0, 0.0, -1.0], rapid: false };
        let steps = [
            Step::Straight(Move { p: [10.0, 0.0, 15.0], rapid: true }),
            Step::Straight(start),
            Step::Arc { end: [-10.0, 0.0, -2.0], centre: [0.0, 0.0], counter_clockwise: true },
            Step::Arc { end: [10.0, 0.0, -2.0], centre: [0.0, 0.0], counter_clockwise: true },
        ];
        let moves = [start];
        let path = PostPath { name: "Down", tool: &t, moves: &moves, dwell: 0.0, steps: Some(&steps), notes: "" };
        let text = gcode(&[path], &job(Some(0.012)));
        let arcs: Vec<&str> = text.lines().filter(|l| l.starts_with("G03")).collect();
        assert!(arcs[0].contains(" Z-2.000"), "{}", arcs[0]);
        assert!(!arcs[1].contains('Z'), "{}", arcs[1]);
    }

    fn profile() -> (Vec<Move>, Tool) {
        let t = tool();
        let o = blank_op(OpKind::Profile, "Contour externe".into(), t.clone(), 6.9, vec![Path::closed(circle([100.0, 100.0], 40.0)), Path::closed(rect([220.0, 60.0], 50.0, 30.0, 0.0))]);
        let moves = generate(&o, machine(), &JobSize { width: 300.0, height: 200.0, thickness: 13.0 }, None).unwrap();
        (moves, t)
    }

    /// Each motion line reduced to its letters: "G02 X1 Y2 I3 J4" is "G02 X Y I J".
    fn shape(line: &str) -> String {
        line.split_whitespace().map(|w| if w.starts_with('G') { w.to_string() } else { w[..1].to_string() }).collect::<Vec<_>>().join(" ")
    }

    fn motion_lines(text: &str) -> Vec<&str> {
        text.lines().filter(|l| ["G00 ", "G01 ", "G02 ", "G03 "].iter().any(|g| l.starts_with(g))).collect()
    }

    #[test]
    fn it_opens_and_closes_exactly_like_their_file() {
        let (moves, t) = profile();
        let text = gcode(&[PostPath { name: "Contour externe", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &job(Some(0.012)));
        let head = "(sample)\r\n(Created: 2026-11-05 10:00)\r\n(Material Size: X=300.000, Y=200.000, Z=13.000)\r\n(Tools:)\r\n(1 = End Mill (3 mm))\r\n\r\nG21 G17 G40 G49 G90 G91.1 G94\r\n\r\nT1 M6 (End Mill (3 mm))\r\nG43 H1\r\nS17000.00 M3\r\n(Contour externe)\r\n";
        assert!(text.starts_with(head), "{}", &text[..head.len().min(text.len())]);
        let tail = "\r\nM5\r\nG49\r\nG53 G00 Z#<_tc_safeheight>\r\nM2\r\n";
        assert!(text.ends_with(tail), "{}", &text[text.len() - 80..]);
        assert!(THEIR_FILE.ends_with(&tail[2..]), "the test's excerpt says the same");
        assert!(THEIR_FILE.contains("G21 G17 G40 G49 G90 G91.1 G94\r\n\r\nT1 M6"));
    }

    #[test]
    fn every_line_has_a_shape_their_file_has() {
        // Every kind of line in their 40 PlanetCNC files, about 290000 lines.
        const THEIRS: [&str; 17] = ["G00 X", "G00 Y", "G00 X Y", "G00 X Z", "G00 Z", "G00 X Y Z", "G01 Z F", "G01 X Y", "G01 X", "G01 Y", "G01 X Y F", "G01 X F", "G01 Y F", "G02 X Y I J", "G02 X Y I J F", "G03 X Y I J", "G03 X Y I J F"];
        let (moves, t) = profile();
        for arcs in [Some(0.012), None] {
            let text = gcode(&[PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &job(arcs));
            for line in motion_lines(&text) {
                assert!(THEIRS.contains(&shape(line).as_str()), "{line} has the shape {}", shape(line));
            }
        }
        for line in motion_lines(THEIR_FILE) {
            assert!(THEIRS.contains(&shape(line).as_str()), "{line}");
        }
    }

    #[test]
    fn it_starts_over_the_first_cut_with_no_trip_to_home() {
        let (moves, t) = profile();
        let text = gcode(&[PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &job(None));
        let lines = motion_lines(&text);
        assert_eq!(shape(lines[0]), "G00 X Y Z", "{}", lines[0]);
        assert!(!lines[0].contains("X0.000 Y0.000"), "{}", lines[0]);
        assert!(lines[0].ends_with("Z15.000"), "{}", lines[0]);
        assert_eq!(lines[1], "G00 Z2.000");
        assert!(lines[2].starts_with("G01 Z"));
        assert!(lines[2].ends_with("F300.0"), "plunging: {}", lines[2]);
        assert_eq!(lines[lines.len() - 1], "G00 Z15.000", "it ends lifted, not back at home");
    }

    #[test]
    fn the_feed_is_only_written_when_it_changes() {
        let (moves, t) = profile();
        let text = gcode(&[PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &job(Some(0.012)));
        let mut last = "";
        for line in motion_lines(&text) {
            if let Some(i) = line.find(" F") {
                let plunge = line.starts_with("G01 Z");
                assert!(plunge || &line[i..] != last, "{line}");
                last = &line[i..];
            }
        }
        assert!(text.contains("F800.0") && text.contains("F300.0"));
    }

    /// Follows the file as the machine would, and returns points along the way.
    fn replay(text: &str) -> Vec<[f64; 3]> {
        let mut at = [0.0f64; 3];
        let mut out = Vec::new();
        for line in motion_lines(text) {
            let get = |c: char| line.split_whitespace().find_map(|w| w.strip_prefix(c).and_then(|v| v.parse::<f64>().ok()));
            let end = [get('X').unwrap_or(at[0]), get('Y').unwrap_or(at[1]), get('Z').unwrap_or(at[2])];
            if line.starts_with("G02") || line.starts_with("G03") {
                let (i, j) = (get('I').expect("a centre"), get('J').expect("a centre"));
                let c = [at[0] + i, at[1] + j];
                let r = i.hypot(j);
                assert!(((end[0] - c[0]).hypot(end[1] - c[1]) - r).abs() < 0.005, "{line}: both ends on the circle");
                let (a0, a1) = ((at[1] - c[1]).atan2(at[0] - c[0]), (end[1] - c[1]).atan2(end[0] - c[0]));
                let mut sweep = a1 - a0;
                if line.starts_with("G03") && sweep < 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                if line.starts_with("G02") && sweep > 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                for s in 1..=24 {
                    let t = a0 + sweep * s as f64 / 24.0;
                    out.push([c[0] + r * t.cos(), c[1] + r * t.sin(), end[2]]);
                }
            } else {
                out.push(end);
            }
            at = end;
        }
        out
    }

    #[test]
    fn a_file_with_arcs_cuts_the_same_path_as_one_without() {
        let (moves, t) = profile();
        let path = [PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }];
        let (with, plain) = (gcode(&path, &job(Some(0.012))), gcode(&path, &job(None)));
        assert!(with.lines().count() * 2 < plain.lines().count(), "{} vs {}", with.lines().count(), plain.lines().count());
        let segs: Vec<_> = moves.windows(2).filter(|w| !w[1].rapid).map(|w| (w[0].p, w[1].p)).collect();
        for p in replay(&with).iter().filter(|p| p[2] < 0.0).step_by(2) {
            let best = segs
                .iter()
                .filter(|(a, b)| (a[2] - p[2]).abs() < 0.01 && (b[2] - p[2]).abs() < 0.01)
                .map(|(a, b)| crate::geom::dist2_seg([p[0], p[1]], [a[0], a[1]], [b[0], b[1]]))
                .fold(f64::MAX, f64::min)
                .sqrt();
            assert!(best < 0.1, "{p:?} strays by {best}");
        }
    }

    #[test]
    fn a_change_of_tool_stops_and_restates_everything() {
        let (moves, t) = profile();
        let mut second = t.clone();
        second.name = "V-Bit".into();
        second.number = 4;
        second.rpm = 12000.0;
        let text = gcode(
            &[PostPath { name: "a", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }, PostPath { name: "b", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }, PostPath { name: "c", tool: &second, moves: &moves, dwell: 0.0, steps: None, notes: "" }],
            &job(None),
        );
        assert!(text.contains("(1 = End Mill (3 mm))\r\n(4 = V-Bit)\r\n"), "both in the list");
        assert_eq!(text.matches(" M6 ").count(), 2, "only when the tool really changes");
        assert!(text.contains("M5\r\nG49\r\nT4 M6 (V-Bit)\r\nG43 H4\r\nS12000.00 M3\r\n(c)\r\n"), "{text}");
        let after = &text[text.find("T4 M6").unwrap()..];
        let first = motion_lines(after)[0];
        assert_eq!(shape(first), "G00 X Y Z", "nothing is assumed after a change: {first}");
    }

    #[test]
    fn tools_without_a_number_take_their_place_in_the_list() {
        let (moves, mut t) = profile();
        t.number = 0;
        let text = gcode(&[PostPath { name: "a", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &job(None));
        assert!(text.contains("T1 M6 "), "{text}");
    }

    #[test]
    fn inches_become_millimetres_and_the_origin_is_respected() {
        let (moves, t) = profile();
        let inch: Vec<Move> = moves.iter().map(|m| Move { p: [m.p[0] / 25.4, m.p[1] / 25.4, m.p[2] / 25.4], rapid: m.rapid }).collect();
        let mut tool_in = t.clone();
        tool_in.feed /= 25.4;
        tool_in.plunge /= 25.4;
        let inches = PlanetJob { scale: 25.4, ..job(Some(0.0005)) };
        let a = replay(&gcode(&[PostPath { name: "p", tool: &tool_in, moves: &inch, dwell: 0.0, steps: None, notes: "" }], &inches));
        let b = gcode(&[PostPath { name: "p", tool: &tool_in, moves: &inch, dwell: 0.0, steps: None, notes: "" }], &inches);
        assert!(b.contains("F800.0") && b.contains("F300.0"), "{}", &b[..400]);
        assert!(a.iter().map(|p| p[0]).fold(0.0, f64::max) > 200.0, "millimetres");
        let moved = PlanetJob { origin: [100.0, 100.0], ..job(None) };
        let text = gcode(&[PostPath { name: "p", tool: &t, moves: &moves, dwell: 0.0, steps: None, notes: "" }], &moved);
        let lowest = replay(&text).iter().map(|p| p[0]).fold(f64::MAX, f64::min);
        assert!(lowest < 0.0, "X now counts from the origin: {lowest}");
    }

    #[test]
    fn numbers_never_read_minus_zero_and_accents_become_one_byte() {
        assert_eq!(mm(-0.0001), "0.000");
        assert_eq!(mm(-1.5), "-1.500");
        assert_eq!(file_bytes("poign\u{e9}es \u{20ac}"), [b'p', b'o', b'i', b'g', b'n', 0xE9, b'e', b's', b' ', b'?']);
    }
}
