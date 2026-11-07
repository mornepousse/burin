//! Reading a machine file written by another program back in as a toolpath, to see it, simulate it
//! and send it out again (File > Import > Import Toolpaths from Other Programs). Burin's own reading.

use crate::app::*;
use crate::i18n::tr;
use crate::toolpath::{self, Move, OpKind};
use std::sync::Arc;

/// Arcs become straight pieces no further than this from the true curve, in millimetres.
const ARC_TOLERANCE_MM: f64 = 0.01;

#[derive(Debug, PartialEq)]
pub struct Read {
    pub moves: Vec<Move>,
    /// In millimetres, as the file said with G21, or inches with G20.
    pub metric: Option<bool>,
    /// Lines with something Burin doesn't follow, such as a work offset or a canned cycle.
    pub skipped: Vec<String>,
}

/// The words of a line, comments left out: `(...)` and everything after `;`.
fn words(line: &str) -> Vec<(char, f64)> {
    let mut clean = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ';' if depth == 0 => break,
            c if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    let mut out = Vec::new();
    let chars: Vec<char> = clean.chars().filter(|c| !c.is_whitespace()).collect();
    let mut i = 0;
    while i < chars.len() {
        let letter = chars[i].to_ascii_uppercase();
        i += 1;
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | '-' | '+')) {
            i += 1;
        }
        if letter.is_ascii_alphabetic() {
            if let Ok(v) = chars[start..i].iter().collect::<String>().parse::<f64>() {
                out.push((letter, v));
            }
        }
    }
    out
}

/// The moves a G-code file makes, in its own unit.
pub fn read(text: &str) -> Read {
    let mut at = [0.0f64; 3];
    let mut motion = 0u8;
    let mut absolute = true;
    let mut metric = None;
    let mut moves = Vec::new();
    let mut skipped = Vec::new();
    for raw in text.lines() {
        let w = words(raw);
        if w.is_empty() {
            continue;
        }
        let mut target = at;
        let mut has_axis = false;
        let (mut i, mut j, mut r) = (None, None, None);
        for (letter, v) in &w {
            match (letter, *v) {
                ('G', g) if g == 0.0 || g == 1.0 || g == 2.0 || g == 3.0 => motion = g as u8,
                ('G', g) if g == 20.0 => metric = Some(false),
                ('G', g) if g == 21.0 => metric = Some(true),
                ('G', g) if g == 90.0 => absolute = true,
                ('G', g) if g == 91.0 => absolute = false,
                ('G', g) if [17.0, 40.0, 43.0, 49.0, 54.0, 64.0, 80.0, 94.0, 90.1, 91.1, 61.0].contains(&g) => {}
                ('G', _) => skipped.push(raw.trim().to_string()),
                ('X' | 'Y' | 'Z', v) => {
                    let k = match letter {
                        'X' => 0,
                        'Y' => 1,
                        _ => 2,
                    };
                    target[k] = if absolute { v } else { at[k] + v };
                    has_axis = true;
                }
                ('I', v) => i = Some(v),
                ('J', v) => j = Some(v),
                ('R', v) => r = Some(v),
                _ => {}
            }
        }
        if !has_axis {
            continue;
        }
        match motion {
            0 => moves.push(Move { p: target, rapid: true }),
            1 => moves.push(Move { p: target, rapid: false }),
            _ => {
                let clockwise = motion == 2;
                let centre = match (i, j, r) {
                    (_, _, Some(r)) => centre_from_radius(at, target, r, clockwise),
                    (i, j, _) => Some([at[0] + i.unwrap_or(0.0), at[1] + j.unwrap_or(0.0)]),
                };
                let tolerance = if metric == Some(false) { ARC_TOLERANCE_MM / 25.4 } else { ARC_TOLERANCE_MM };
                match centre {
                    Some(c) => moves.extend(arc_points(at, target, c, clockwise, tolerance).into_iter().map(|p| Move { p, rapid: false })),
                    None => moves.push(Move { p: target, rapid: false }),
                }
            }
        }
        at = target;
    }
    Read { moves, metric, skipped }
}

/// The centre of an arc given by its radius: a negative radius takes the long way round.
fn centre_from_radius(from: [f64; 3], to: [f64; 3], r: f64, clockwise: bool) -> Option<[f64; 2]> {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let chord = dx.hypot(dy);
    if chord < 1e-12 || r.abs() * 2.0 < chord - 1e-9 {
        return None;
    }
    let h = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    let (mx, my) = (from[0] + dx / 2.0, from[1] + dy / 2.0);
    let side = if clockwise == (r > 0.0) { -1.0 } else { 1.0 };
    Some([mx - side * h * dy / chord, my + side * h * dx / chord])
}

/// Points along an arc, Z changing evenly for a helix, the last one exactly the end.
fn arc_points(from: [f64; 3], to: [f64; 3], c: [f64; 2], clockwise: bool, tolerance: f64) -> Vec<[f64; 3]> {
    let r = (from[0] - c[0]).hypot(from[1] - c[1]);
    let a0 = (from[1] - c[1]).atan2(from[0] - c[0]);
    let a1 = (to[1] - c[1]).atan2(to[0] - c[0]);
    let mut sweep = a1 - a0;
    let tau = std::f64::consts::TAU;
    if clockwise {
        while sweep >= -1e-12 {
            sweep -= tau;
        }
    } else {
        while sweep <= 1e-12 {
            sweep += tau;
        }
    }
    // A whole turn when it ends where it began.
    if (from[0] - to[0]).hypot(from[1] - to[1]) > 1e-9 && sweep.abs() > tau - 1e-9 {
        sweep -= tau * sweep.signum();
    }
    let step = if r > tolerance { 2.0 * (1.0 - tolerance / r).acos() } else { sweep.abs() };
    let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 10000);
    (1..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            if k == n {
                to
            } else {
                let a = a0 + sweep * t;
                [c[0] + r * a.cos(), c[1] + r * a.sin(), from[2] + (to[2] - from[2]) * t]
            }
        })
        .collect()
}

impl App {
    pub fn import_toolpath_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter(tr("Machine file"), &["nc", "gcode", "ngc", "tap", "txt", "cnc"]).pick_file() else { return };
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().to_string());
        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                if let Err(why) = self.import_toolpath(&name, &text) {
                    self.message = Some(why);
                }
            }
            Err(e) => self.message = Some(tf!("Could not open {}: {e}", path.display())),
        }
    }

    /// Adds the file's moves as a toolpath of their own, in the job's unit, from the job's zero.
    pub fn import_toolpath(&mut self, name: &str, text: &str) -> Result<(), String> {
        let read = read(text);
        if !read.moves.iter().any(|m| !m.rapid) {
            return Err(tr("There are no cutting moves in this file.").into());
        }
        let job_metric = self.doc.job.units == Units::Mm;
        let k = match (read.metric.unwrap_or(job_metric), job_metric) {
            (true, false) => 1.0 / 25.4,
            (false, true) => 25.4,
            _ => 1.0,
        };
        // The file counts from the machine's zero; the job from its own corner.
        let zero = self.doc.job.origin();
        let z_shift = if self.doc.job.z_on_bed { -self.doc.job.thickness } else { 0.0 };
        let machine = self.doc.machine_on_job();
        let mut moves = vec![Move { p: machine.home, rapid: true }];
        moves.extend(read.moves.iter().map(|m| Move { p: [m.p[0] * k + zero[0], m.p[1] * k + zero[1], m.p[2] * k + z_shift], rapid: m.rapid }));
        moves.push(Move { p: machine.home, rapid: true });
        let tool = self.doc.tools.first().cloned().unwrap_or_else(|| toolpath::default_tools().remove(0)).in_units(job_metric);
        let mut op = toolpath::blank_op(OpKind::Profile, name.to_string(), tool, toolpath::deepest_move(&moves), Vec::new());
        op.imported = true;
        let moves = Arc::new(moves);
        self.checkpoint();
        let color = TOOLPATH_COLOURS[self.doc.toolpaths.len() % TOOLPATH_COLOURS.len()];
        self.doc.toolpaths.push(Toolpath { op, visible: true, color, moves: moves.clone(), kept_moves: Some(moves), kept_steps: None });
        self.selected_tp = Some(self.doc.toolpaths.len() - 1);
        self.screen = Screen::Work;
        self.right_open = true;
        self.right = RightMode::Preview;
        self.status = if read.skipped.is_empty() {
            tf!("{} imported. Check its tool: Burin can't read that from the file.", name)
        } else {
            tf!("{} imported, but {} lines with commands Burin doesn't follow were left out. Check its tool too.", name, read.skipped.len())
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_moves_and_units_are_read() {
        let r = read("%\n(a comment G0 X99)\nG21 G90\nG0 X10 Y5 Z5 ; going there\nG1 Z-2 F300\nX20\nG91 G1 X5\nM30\n%");
        assert_eq!(r.metric, Some(true));
        let points: Vec<([f64; 3], bool)> = r.moves.iter().map(|m| (m.p, m.rapid)).collect();
        assert_eq!(points, [([10.0, 5.0, 5.0], true), ([10.0, 5.0, -2.0], false), ([20.0, 5.0, -2.0], false), ([25.0, 5.0, -2.0], false)]);
        assert!(r.skipped.is_empty());
        assert_eq!(read("G20\nG0X1Y2").metric, Some(false));
        assert_eq!(read("G28 X0\nG0 X1").skipped, ["G28 X0"]);
    }

    #[test]
    fn arcs_come_out_as_close_points_both_ways() {
        // A half circle of radius 10 round the origin, each way, and by its radius.
        for line in ["G2 X10 Y0 I10 J0", "G2 X10 Y0 R10"] {
            let r = read(&format!("G0 X-10 Y0 Z0\n{line}"));
            assert!(r.moves.len() > 10);
            let last = r.moves.last().unwrap().p;
            assert_eq!([last[0], last[1]], [10.0, 0.0]);
            for m in &r.moves[1..] {
                assert!((m.p[0].hypot(m.p[1]) - 10.0).abs() < 0.011, "{:?}", m.p);
            }
            assert!(r.moves[r.moves.len() / 2].p[1] > 9.0, "clockwise from the left goes over the top: {line}");
        }
        let ccw = read("G0 X-10 Y0\nG3 X10 Y0 I10 J0");
        assert!(ccw.moves[ccw.moves.len() / 2].p[1] < -9.0);
        // A full circle, and a helix going down as it turns.
        let full = read("G0 X10 Y0 Z0\nG3 X10 Y0 Z-3 I-10 J0");
        assert!(full.moves.len() > 40);
        assert!((full.moves.last().unwrap().p[2] + 3.0).abs() < 1e-9);
    }
}
