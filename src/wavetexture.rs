//! Create Vector Texture: rows of wavy lines across the chosen shapes (or the whole material when
//! none is chosen), for engraving a texture. Each line is a wave of a given amplitude and length;
//! the lines lie side by side at most a set distance apart, nearer at random by the variation,
//! and the noise shakes each line a little. The same settings always give the same lines.

use crate::geom::{inside_even_odd, Path, P};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveSettings {
    /// The lines' direction, in degrees (0: they run across the sheet, along X, and swing up and
    /// down; 90: they run up it).
    pub angle: f64,
    /// The widest gap between two lines.
    pub spacing: f64,
    /// 0..1: how much nearer than `spacing` a line may come, at random.
    pub variation: f64,
    /// The waves' sideways swing, crest to trough.
    pub amplitude: f64,
    /// The length of one wave.
    pub length: f64,
    /// 0..1: how much each line is shaken, as a share of the gap.
    pub noise: f64,
}

/// A small random source, the same for the same seed.
struct Dice(u64);

impl Dice {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The most lines made, and points per line: past these the gap or the wave is too small for the
/// area, and the result would only be slow.
const MOST_LINES: usize = 4000;
const MOST_POINTS: usize = 4000;

/// The wavy lines filling `area` (closed loops, read even-odd), clipped to it.
pub fn lines(area: &[Path], s: WaveSettings) -> Vec<Path> {
    let loops: Vec<&Path> = area.iter().filter(|p| p.closed && p.pts.len() >= 3).collect();
    if loops.is_empty() || s.spacing <= 0.0 || s.length <= 0.0 {
        return Vec::new();
    }
    // The lines are made upright in their own frame, then turned: a quarter turn less than the angle.
    let (sin, cos) = (s.angle - 90.0).to_radians().sin_cos();
    // Into the lines' own frame (they run along v), and back.
    let to_frame = |p: P| [p[0] * cos + p[1] * sin, -p[0] * sin + p[1] * cos];
    let from_frame = |q: P| [q[0] * cos - q[1] * sin, q[0] * sin + q[1] * cos];
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for p in loops.iter().flat_map(|l| l.pts.iter()) {
        let q = to_frame(*p);
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    let swing = s.amplitude.abs() / 2.0;
    let (x0, x1) = (lo[0] - swing - s.spacing, hi[0] + swing + s.spacing);
    let (y0, y1) = (lo[1] - s.length / 4.0, hi[1] + s.length / 4.0);
    if (x1 - x0) / s.spacing > MOST_LINES as f64 {
        return Vec::new();
    }
    let step = (s.length / 36.0).max((y1 - y0) / MOST_POINTS as f64);
    let mut dice = Dice(0x9E37_79B9_7F4A_7C15);
    let variation = s.variation.clamp(0.0, 0.95);
    let noise = s.noise.clamp(0.0, 1.0) * s.spacing * 0.5;
    let mut out = Vec::new();
    let mut x = x0;
    while x <= x1 {
        // The shake changes smoothly along the line: random values a wave-length apart, eased between.
        let knots: Vec<f64> = (0..((y1 - y0) / s.length * 4.0).ceil() as usize + 2).map(|_| (dice.unit() - 0.5) * 2.0 * noise).collect();
        let shake = |y: f64| {
            let t = (y - y0) / s.length * 4.0;
            let k = (t.floor() as usize).min(knots.len().saturating_sub(2));
            let f = t - k as f64;
            let ease = f * f * (3.0 - 2.0 * f);
            knots[k] * (1.0 - ease) + knots[k + 1] * ease
        };
        let mut pts = Vec::new();
        let mut y = y0;
        while y <= y1 + step * 0.5 {
            let q = [x + swing * (std::f64::consts::TAU * y / s.length).sin() + shake(y), y];
            pts.push(from_frame(q));
            y += step;
        }
        out.extend(clip(&pts, &loops));
        x += s.spacing * (1.0 - variation * dice.unit());
    }
    out
}

/// The parts of the open line `pts` inside the loops.
fn clip(pts: &[P], loops: &[&Path]) -> Vec<Path> {
    let edges: Vec<(P, P)> = loops.iter().flat_map(|l| l.segments()).collect();
    let owned: Vec<Path> = loops.iter().map(|l| (*l).clone()).collect();
    let mut pieces: Vec<Path> = Vec::new();
    let mut current: Vec<P> = Vec::new();
    let inside = |p: P| inside_even_odd(owned.iter(), p);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Where this step crosses the loops, in order.
        let mut cuts: Vec<f64> = edges.iter().filter_map(|(c, d)| crate::edit::segment_cross(a, b, *c, *d)).collect();
        cuts.sort_by(|x, y| x.total_cmp(y));
        let mut bounds = vec![0.0];
        bounds.extend(cuts);
        bounds.push(1.0);
        for k in bounds.windows(2) {
            let (t0, t1) = (k[0], k[1]);
            if t1 - t0 < 1e-12 {
                continue;
            }
            let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            if inside(at((t0 + t1) / 2.0)) {
                if current.is_empty() {
                    current.push(at(t0));
                }
                current.push(at(t1));
            } else if current.len() >= 2 {
                pieces.push(Path::open(std::mem::take(&mut current)));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        pieces.push(Path::open(current));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Path> {
        vec![Path::closed(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]])]
    }

    #[test]
    fn waves_fill_the_area_and_stay_inside_it() {
        let s = WaveSettings { angle: 0.0, spacing: 3.0, variation: 0.0, amplitude: 40.0, length: 100.0, noise: 0.0 };
        let out = lines(&square(), s);
        assert!(out.len() > 30, "{}", out.len());
        for p in out.iter().flat_map(|l| l.pts.iter()) {
            assert!(p[0] >= -1e-6 && p[0] <= 100.0 + 1e-6 && p[1] >= -1e-6 && p[1] <= 100.0 + 1e-6, "{p:?}");
        }
        // Lines across the sheet, swinging 40 crest to trough over a wave of 100.
        let long = out.iter().max_by_key(|l| l.pts.len()).unwrap();
        let xs: Vec<f64> = long.pts.iter().map(|p| p[1]).collect();
        let swing = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
        assert!((swing - 40.0).abs() < 1.0, "{swing}");
        // The same settings give the same lines.
        assert_eq!(lines(&square(), s), out);
    }

    #[test]
    fn turned_lines_and_a_hole_left_empty() {
        let mut area = square();
        area.push(Path::closed(vec![[40.0, 40.0], [60.0, 40.0], [60.0, 60.0], [40.0, 60.0]]));
        let s = WaveSettings { angle: 90.0, spacing: 5.0, variation: 0.5, amplitude: 10.0, length: 50.0, noise: 0.3 };
        let out = lines(&area, s);
        assert!(!out.is_empty());
        for l in &out {
            for w in l.pts.windows(2) {
                let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
                assert!(!(m[0] > 40.5 && m[0] < 59.5 && m[1] > 40.5 && m[1] < 59.5), "inside the hole: {m:?}");
            }
        }
    }
}
