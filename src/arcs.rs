//! Turning runs of small straight moves back into true arcs, for machines that cut them smoothly.

use crate::toolpath::Move;

/// One thing the machine does.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Step {
    Straight(Move),
    Arc { end: [f64; 3], centre: [f64; 2], counter_clockwise: bool },
}

/// How far around the circle one straight side may go and still be part of a curve: 8 degrees.
const MOST_PER_SIDE: f64 = 8.0 * std::f64::consts::PI / 180.0;

/// How far a curve's sides may bulge from the arc that stands in for them, in tolerances; sides
/// any longer make a polygon. Burin's choice.
const SIDE_BULGE_MOST: f64 = 16.0;

/// The circle through three points.
fn circle_through(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<[f64; 2]> {
    let (bx, by, cx, cy) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
    let under = 2.0 * (bx * cy - by * cx);
    if under.abs() < 1e-12 {
        return None;
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    Some([a[0] + (cy * b2 - by * c2) / under, a[1] + (bx * c2 - cx * b2) / under])
}

/// The arc for moves `from..=to`, if they all follow one within `tol`.
fn fit(moves: &[Move], from: usize, to: usize, tol: f64) -> Option<Step> {
    let (a, m, b) = (moves[from].p, moves[(from + to) / 2].p, moves[to].p);
    let centre = circle_through(a, m, b)?;
    let radius = (a[0] - centre[0]).hypot(a[1] - centre[1]);
    // Nearly straight runs stay straight: such arcs are all rounding error on a machine.
    let chord = (a[0] - b[0]).hypot(a[1] - b[1]);
    if radius > chord.max(tol) * 500.0 {
        return None;
    }
    let angle = |p: [f64; 3]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    // A curve drawn as small sides strays from its true circle by the bulge of one side,
    // and offsetting it keeps those flats. That much is the drawing's own roughness: the bulge of
    // a typical side of the run, as a curve's sides are all alike. Not of the longest: one long
    // straight side among short ones (a pocket's straight wall between its corners) would let an
    // arc of hundreds of millimetres stand in for it, a millimetre and more off the cut.
    let mut sides: Vec<f64> = moves[from..to].iter().zip(&moves[from + 1..=to]).map(|(p, q)| (p.p[0] - q.p[0]).hypot(p.p[1] - q.p[1])).collect();
    sides.sort_by(|x, y| x.total_cmp(y));
    // The longer half's middle: an exact offset of a curve drawn as sides alternates the sides'
    // own offsets with tiny pieces where they turn, which would hide the sides' bulge.
    let typical = sides[(sides.len() - 1) * 3 / 4];
    let bulge = typical * typical / (8.0 * radius.max(1e-9));
    if bulge > SIDE_BULGE_MOST * tol {
        return None;
    }
    let allowed = tol + bulge;
    let mut swept = 0.0;
    let mut way = 0.0f64;
    let mut before = moves[from].p;
    for mv in &moves[from + 1..=to] {
        let p = mv.p;
        if mv.rapid || (p[2] - a[2]).abs() > 1e-9 {
            return None;
        }
        // Every point near the circle, and the middle of every straight side too.
        if ((p[0] - centre[0]).hypot(p[1] - centre[1]) - radius).abs() > allowed {
            return None;
        }
        let mid = [(p[0] + before[0]) / 2.0, (p[1] + before[1]) / 2.0];
        if ((mid[0] - centre[0]).hypot(mid[1] - centre[1]) - radius).abs() > allowed {
            return None;
        }
        let mut turn = angle(p) - angle(before);
        turn = (turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        // Big steps around the circle mean a real polygon, which must keep its corners.
        if turn.abs() < 1e-12 || turn.abs() > MOST_PER_SIDE || (way != 0.0 && way.signum() != turn.signum()) {
            return None;
        }
        way = turn;
        swept += turn;
        before = p;
    }
    // About a quarter turn each, the same as the files their machines already run.
    if swept.abs() > std::f64::consts::FRAC_PI_2 * 1.03 {
        return None;
    }
    Some(Step::Arc { end: b, centre, counter_clockwise: swept > 0.0 })
}

/// The moves with their curved runs as arcs. Arcs are only made at one height.
pub fn fit_arcs(moves: &[Move], tol: f64) -> Vec<Step> {
    let mut out = Vec::with_capacity(moves.len() / 2);
    let mut i = 0;
    if let Some(first) = moves.first() {
        out.push(Step::Straight(*first));
    }
    const SHORTEST: usize = 4;
    while i + 1 < moves.len() {
        let mut best = None;
        let mut reach = i + SHORTEST;
        // Grow while it still fits, then narrow down to the last move that does.
        let mut last_good = None;
        let mut grow = SHORTEST;
        while reach < moves.len() {
            match fit(moves, i, reach, tol) {
                Some(step) => {
                    best = Some(step);
                    last_good = Some(reach);
                    grow *= 2;
                    reach += grow;
                }
                None => break,
            }
        }
        if let Some(good) = last_good {
            let (mut lo, mut hi) = (good, reach.min(moves.len()));
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                match fit(moves, i, mid, tol) {
                    Some(step) => {
                        best = Some(step);
                        lo = mid;
                    }
                    None => hi = mid,
                }
            }
            out.push(best.expect("a fit was found"));
            i = lo;
        } else {
            out.push(Step::Straight(moves[i + 1]));
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(x: f64, y: f64, z: f64) -> Move {
        Move { p: [x, y, z], rapid: false, join: false }
    }

    #[test]
    fn a_long_straight_side_stays_straight() {
        // Points on a wide circle, short sides either side of a long one: as a pocket's ring with a
        // long straight wall. An arc through them all bulges far off the wall in its middle.
        let (c, r) = ([0.0, 0.0], 700.0);
        let mut angles = vec![0.0, 0.0015, 0.003, 0.0045];
        angles.extend([0.0045 + 0.13, 0.0045 + 0.13 + 0.0015, 0.0045 + 0.13 + 0.003, 0.0045 + 0.13 + 0.0045]);
        let moves: Vec<Move> = angles.iter().map(|a: &f64| feed(c[0] + r * a.cos(), c[1] + r * a.sin(), -2.0)).collect();
        let tol = 0.012;
        let off = |q: [f64; 3]| {
            moves.windows(2).map(|w| {
                let (a, b) = (w[0].p, w[1].p);
                let d = [b[0] - a[0], b[1] - a[1]];
                let t = (((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1])).clamp(0.0, 1.0);
                (a[0] + d[0] * t - q[0]).hypot(a[1] + d[1] * t - q[1])
            }).fold(f64::MAX, f64::min)
        };
        let mut steps = vec![Step::Straight(moves[0])];
        steps.extend(fit_arcs(&moves, tol).into_iter().skip(1));
        for q in replay(&steps) {
            assert!(off(q) <= 2.0 * tol, "{q:?} is {} off the moves", off(q));
        }
    }

    fn circle(c: [f64; 2], r: f64, n: usize, z: f64) -> Vec<Move> {
        (0..=n).map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            feed(c[0] + r * a.cos(), c[1] + r * a.sin(), z)
        }).collect()
    }

    /// Follows the steps as a machine would, and returns points along the way.
    fn replay(steps: &[Step]) -> Vec<[f64; 3]> {
        let mut at = [0.0; 3];
        let mut out = Vec::new();
        for s in steps {
            match s {
                Step::Straight(m) => {
                    at = m.p;
                    out.push(at);
                }
                Step::Arc { end, centre, counter_clockwise } => {
                    let r = (at[0] - centre[0]).hypot(at[1] - centre[1]);
                    assert!(((end[0] - centre[0]).hypot(end[1] - centre[1]) - r).abs() < 1e-6, "both ends on the circle");
                    let (a0, a1) = ((at[1] - centre[1]).atan2(at[0] - centre[0]), (end[1] - centre[1]).atan2(end[0] - centre[0]));
                    let mut sweep = a1 - a0;
                    if *counter_clockwise && sweep < 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    if !*counter_clockwise && sweep > 0.0 {
                        sweep -= std::f64::consts::TAU;
                    }
                    assert!(sweep.abs() < std::f64::consts::PI, "under half a turn: {sweep}");
                    for i in 1..=40 {
                        let t = a0 + sweep * i as f64 / 40.0;
                        out.push([centre[0] + r * t.cos(), centre[1] + r * t.sin(), end[2]]);
                    }
                    at = *end;
                }
            }
        }
        out
    }

    #[test]
    fn a_many_sided_circle_becomes_a_few_arcs_on_the_same_path() {
        let moves = circle([5.0, 5.0], 2.0, 256, -0.2);
        let steps = fit_arcs(&moves, 0.001);
        let arcs = steps.iter().filter(|s| matches!(s, Step::Arc { .. })).count();
        assert!((3..=6).contains(&arcs), "{arcs} arcs");
        assert!(steps.len() < 10, "{} steps", steps.len());
        for p in replay(&steps) {
            assert!(((p[0] - 5.0).hypot(p[1] - 5.0) - 2.0).abs() < 0.001, "{p:?}");
            assert_eq!(p[2], -0.2);
        }
        match steps.last() {
            Some(Step::Arc { end, .. }) => assert!((end[0] - 7.0).abs() < 1e-9 && (end[1] - 5.0).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_direction_is_kept() {
        let ccw = fit_arcs(&circle([0.0, 0.0], 1.0, 128, 0.0), 0.001);
        assert!(ccw.iter().any(|s| matches!(s, Step::Arc { counter_clockwise: true, .. })));
        assert!(!ccw.iter().any(|s| matches!(s, Step::Arc { counter_clockwise: false, .. })));
        let mut back = circle([0.0, 0.0], 1.0, 128, 0.0);
        back.reverse();
        let cw = fit_arcs(&back, 0.001);
        assert!(cw.iter().any(|s| matches!(s, Step::Arc { counter_clockwise: false, .. })));
        assert!(!cw.iter().any(|s| matches!(s, Step::Arc { counter_clockwise: true, .. })));
    }

    #[test]
    fn straight_lines_and_corners_stay_straight() {
        let line: Vec<Move> = (0..50).map(|i| feed(i as f64 * 0.1, 0.0, -0.1)).collect();
        assert!(fit_arcs(&line, 0.001).iter().all(|s| matches!(s, Step::Straight(_))));
        let square = [feed(0.0, 0.0, 0.0), feed(1.0, 0.0, 0.0), feed(1.0, 1.0, 0.0), feed(0.0, 1.0, 0.0), feed(0.0, 0.0, 0.0), feed(1.0, 0.0, 0.0)];
        assert!(fit_arcs(&square, 0.001).iter().all(|s| matches!(s, Step::Straight(_))));
    }

    #[test]
    fn a_change_of_height_or_a_fast_move_breaks_an_arc() {
        // A spiral down: stays as straight moves so the depth is never in doubt.
        let spiral: Vec<Move> = (0..64).map(|i| {
            let a = i as f64 / 64.0 * std::f64::consts::TAU;
            feed(a.cos(), a.sin(), -0.001 * i as f64)
        }).collect();
        assert!(fit_arcs(&spiral, 0.001).iter().all(|s| matches!(s, Step::Straight(_))));
        let mut moves = circle([0.0, 0.0], 1.0, 64, 0.0);
        moves[10].rapid = true;
        let steps = fit_arcs(&moves, 0.001);
        assert!(steps.iter().any(|s| matches!(s, Step::Straight(m) if m.rapid)), "the fast move is still there");
    }

    #[test]
    fn a_wavy_line_is_not_flattened_beyond_the_tolerance() {
        let wave: Vec<Move> = (0..400).map(|i| {
            let x = i as f64 * 0.02;
            feed(x, (x * 3.0).sin(), -0.1)
        }).collect();
        let steps = fit_arcs(&wave, 0.001);
        assert!(steps.len() < 200, "{}", steps.len());
        let out = replay(&steps);
        for p in out {
            // Measured square to the curve, which is steep in places.
            let slope = 3.0 * (p[0] * 3.0).cos();
            let off = (p[1] - (p[0] * 3.0).sin()).abs() / (1.0 + slope * slope).sqrt();
            assert!(off < 0.0015, "{p:?} is {off} from the curve");
        }
    }

    #[test]
    fn a_real_polygon_keeps_its_corners_but_a_coarsely_drawn_large_circle_is_a_curve() {
        // Twelve sides, on a circle: all on one circle, yet it is a polygon.
        assert!(fit_arcs(&circle([0.0, 0.0], 3.0, 12, 0.0), 0.0005).iter().all(|s| matches!(s, Step::Straight(_))));
        // A large circle in 128 sides bulges far more than the tolerance, and is still a circle.
        let steps = fit_arcs(&circle([0.0, 0.0], 20.0, 128, 0.0), 0.0005);
        assert!(steps.iter().filter(|s| matches!(s, Step::Arc { .. })).count() >= 3);
        assert!(steps.len() < 10, "{}", steps.len());
    }

    #[test]
    fn every_move_is_accounted_for() {
        assert!(fit_arcs(&[], 0.001).is_empty());
        let one = [feed(1.0, 2.0, 3.0)];
        assert_eq!(fit_arcs(&one, 0.001), vec![Step::Straight(one[0])]);
        let moves = circle([0.0, 0.0], 1.0, 128, 0.0);
        let steps = fit_arcs(&moves, 0.0005);
        let end = match steps.last().unwrap() {
            Step::Straight(m) => m.p,
            Step::Arc { end, .. } => *end,
        };
        assert_eq!(end, moves.last().unwrap().p);
    }
}
