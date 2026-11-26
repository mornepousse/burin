//! Reading the files a circuit board is made from, so it can be milled: Excellon drill files (each
//! hole a circle of its drill's size, slots as rounded slots, one layer per size) and Gerber files
//! (copper and the like as the outlines of what stays, the board's outline as its lines).
//! Burin's own reader; coordinates are kept as they are, so a board's files line up.

use crate::geom::{signed_area, Path, P};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

/// What a board file gave: objects (each with the layer it goes on) and what was left out.
#[derive(Default)]
pub struct Board {
    pub objects: Vec<(String, Vec<Path>)>,
    pub notes: Vec<String>,
}

/// Endings drill files use.
pub const DRILL_ENDINGS: &[&str] = &["drl", "xln", "exc", "drd"];
/// Endings Gerber files use.
pub const GERBER_ENDINGS: &[&str] = &["gbr", "ger", "pho", "gtl", "gbl", "gto", "gbo", "gts", "gbs", "gtp", "gbp", "gko", "gm1", "gml", "gm", "g1", "g2", "g3", "g4"];
/// Gerber endings that mean the board's outline.
const OUTLINE_ENDINGS: &[&str] = &["gko", "gm1", "gml", "gm"];

/// How far a drawn curve may stray from the true circle, in millimetres.
const ROUNDNESS_MM: f64 = 0.005;

/// A whole number written with or without its point: `digits` whole digits and `decimals` after,
/// with the leading zeros left out (`lead_omitted`) or the trailing ones.
fn coordinate(text: &str, digits: usize, decimals: usize, lead_omitted: bool) -> Option<f64> {
    if text.contains('.') {
        return text.parse().ok();
    }
    let (sign, body) = match text.as_bytes().first()? {
        b'-' => (-1.0, &text[1..]),
        b'+' => (1.0, &text[1..]),
        _ => (1.0, text),
    };
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: f64 = body.parse().ok()?;
    let v = if lead_omitted || body.len() >= digits + decimals {
        whole / 10f64.powi(decimals as i32)
    } else {
        // Trailing zeros left out: the digits start right after the point's whole part.
        whole * 10f64.powi((digits + decimals - body.len()) as i32) / 10f64.powi(decimals as i32)
    };
    Some(sign * v)
}

/// Points round a circle, close enough to it.
fn round(c: P, r: f64, tol: f64) -> Vec<P> {
    // An even count, so the circle is as wide as it is tall.
    let n = steps(r, TAU, tol).max(12).next_multiple_of(4);
    (0..n).map(|k| {
        let a = TAU * k as f64 / n as f64;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    })
    .collect()
}

/// How many straight pieces a turn of `sweep` at radius `r` needs.
fn steps(r: f64, sweep: f64, tol: f64) -> usize {
    if r <= tol {
        return 4;
    }
    let step = 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(2, 256)
}

/// The band a round tool or pen of `width` leaves from `a` to `b`, anticlockwise.
fn stadium(a: P, b: P, width: f64, tol: f64) -> Vec<P> {
    let r = width / 2.0;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx.hypot(dy) < 1e-9 {
        return round(a, r, tol);
    }
    let t = dy.atan2(dx);
    let n = steps(r, PI, tol).max(6);
    let mut out = Vec::with_capacity(2 * n + 2);
    for (c, from) in [(b, t - PI / 2.0), (a, t + PI / 2.0)] {
        for k in 0..=n {
            let ang = from + PI * k as f64 / n as f64;
            out.push([c[0] + r * ang.cos(), c[1] + r * ang.sin()]);
        }
    }
    out
}

/// Points along an arc from `from` to `to` round `centre` (clockwise or not), without `from`.
fn arc_points(from: P, to: P, centre: P, clockwise: bool, tol: f64) -> Vec<P> {
    let r = (from[0] - centre[0]).hypot(from[1] - centre[1]);
    let a0 = (from[1] - centre[1]).atan2(from[0] - centre[0]);
    let a1 = (to[1] - centre[1]).atan2(to[0] - centre[0]);
    let mut sweep = a1 - a0;
    if clockwise {
        while sweep >= -1e-9 {
            sweep -= TAU;
        }
        if (from[0] - to[0]).hypot(from[1] - to[1]) > 1e-9 && sweep < -TAU + 1e-9 {
            sweep += TAU;
        }
    } else {
        while sweep <= 1e-9 {
            sweep += TAU;
        }
        if (from[0] - to[0]).hypot(from[1] - to[1]) > 1e-9 && sweep > TAU - 1e-9 {
            sweep -= TAU;
        }
    }
    let n = steps(r, sweep, tol);
    let mut pts: Vec<P> = (1..n).map(|k| {
        let a = a0 + sweep * k as f64 / n as f64;
        [centre[0] + r * a.cos(), centre[1] + r * a.sin()]
    })
    .collect();
    pts.push(to);
    pts
}

/// Loops turned the right way for filling non-zero: outlines anticlockwise, holes clockwise.
fn turned(shapes: Vec<Vec<Vec<P>>>) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    for shape in shapes {
        for (k, mut l) in shape.into_iter().enumerate() {
            if l.len() < 3 {
                continue;
            }
            if (signed_area(&l) > 0.0) != (k == 0) {
                l.reverse();
            }
            out.push(l);
        }
    }
    out
}

fn anticlockwise(mut l: Vec<P>) -> Vec<P> {
    if signed_area(&l) < 0.0 {
        l.reverse();
    }
    l
}

/// Everything `add` covers put together with what `acc` covers (or taken away from it).
fn combine(acc: &[Vec<P>], add: &[Vec<P>], rule: OverlayRule) -> Vec<Vec<P>> {
    if add.is_empty() {
        return acc.to_vec();
    }
    if acc.is_empty() && rule == OverlayRule::Difference {
        return Vec::new();
    }
    let (subject, clip): (Vec<Vec<P>>, Vec<Vec<P>>) = if rule == OverlayRule::Union { (acc.iter().chain(add).cloned().collect(), Vec::new()) } else { (acc.to_vec(), add.to_vec()) };
    let rule = if rule == OverlayRule::Union { OverlayRule::Subject } else { rule };
    turned(subject.overlay(&clip, rule, FillRule::NonZero))
}

fn stem_of(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.rsplit_once('.').map_or(file, |(s, _)| s).to_string()
}

fn ending(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// Whether a file name is a drill file's or a Gerber file's.
pub fn is_board_file(name: &str) -> bool {
    let e = ending(name);
    DRILL_ENDINGS.contains(&e.as_str()) || GERBER_ENDINGS.contains(&e.as_str())
}

/// Reads a drill or Gerber file, telling them apart by what is in it. `per_mm` is the job's unit
/// in millimetres' worth (1 for millimetres, 1/25.4 for inches).
pub fn read(name: &str, text: &str, per_mm: f64) -> Board {
    let starts_as_drill = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with(';')).is_some_and(|l| l.starts_with("M48"));
    if starts_as_drill || (DRILL_ENDINGS.contains(&ending(name).as_str()) && !text.contains("%FS")) {
        read_drill(name, text, per_mm)
    } else {
        read_gerber(name, text, per_mm)
    }
}

/// The size a layer is named after, in the job's unit.
fn size_name(diameter_mm: f64, per_mm: f64) -> String {
    if (per_mm - 1.0).abs() < 1e-9 {
        format!("Ø{:.2} mm", diameter_mm)
    } else {
        format!("Ø{:.4} in", diameter_mm / 25.4)
    }
}

/// An Excellon drill file: its holes and slots.
pub fn read_drill(name: &str, text: &str, per_mm: f64) -> Board {
    let stem = stem_of(name);
    let tol = ROUNDNESS_MM * per_mm;
    let mut metric = false;
    let mut format: Option<(usize, usize)> = None;
    // Excellon's LZ means the leading zeros are written, so the trailing ones may be left out.
    let mut lead_omitted = true;
    let mut tools: HashMap<u32, f64> = HashMap::new();
    let mut tool = 0u32;
    let mut at: P = [0.0, 0.0];
    let mut routing = false;
    let mut down = false;
    let mut route: Vec<P> = Vec::new();
    let mut holes: Vec<(u32, P)> = Vec::new();
    let mut slots: Vec<(u32, Vec<P>)> = Vec::new();
    let mut unknown_tool = 0usize;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(comment) = line.strip_prefix(';') {
            if let Some(f) = comment.trim().strip_prefix("FILE_FORMAT=") {
                let mut it = f.split(':').filter_map(|v| v.trim().parse::<usize>().ok());
                if let (Some(a), Some(b)) = (it.next(), it.next()) {
                    format = Some((a, b));
                }
            }
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("METRIC") || upper.starts_with("INCH") || upper == "M71" || upper == "M72" {
            metric = upper.starts_with("METRIC") || upper == "M71";
            for part in upper.split(',').skip(1) {
                match part {
                    "LZ" => lead_omitted = false,
                    "TZ" => lead_omitted = true,
                    pattern if pattern.contains('.') && pattern.bytes().all(|b| b == b'0' || b == b'.') => {
                        let (a, b) = pattern.split_once('.').unwrap_or((pattern, ""));
                        format = Some((a.len(), b.len()));
                    }
                    _ => {}
                }
            }
            continue;
        }
        let (digits, decimals) = format.unwrap_or(if metric { (3, 3) } else { (2, 4) });
        let to_mm = if metric { 1.0 } else { 25.4 };
        // Tools: T1C0.8 defines one (in the header or the body); T1 alone picks it.
        if let Some(rest) = upper.strip_prefix('T') {
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(number) = n.parse::<u32>() {
                if let Some(c) = rest.find('C') {
                    let size: String = rest[c + 1..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    if let Ok(d) = size.parse::<f64>() {
                        tools.insert(number, d * to_mm);
                    }
                } else {
                    tool = number;
                }
                continue;
            }
        }
        match upper.as_str() {
            "M15" => {
                down = true;
                route = vec![at];
                continue;
            }
            "M16" | "M17" => {
                if down && route.len() >= 2 {
                    slots.push((tool, std::mem::take(&mut route)));
                }
                down = false;
                continue;
            }
            _ => {}
        }
        if upper.starts_with("G00") {
            routing = true;
        } else if upper.starts_with("G05") || upper.starts_with("G81") {
            routing = false;
        }
        // Coordinates: X..Y.., maybe with G85 and a slot's other end.
        let point = |s: &str, at: P| -> Option<P> {
            let mut p = at;
            let mut found = false;
            for (axis, k) in [('X', 0), ('Y', 1)] {
                if let Some(i) = s.find(axis) {
                    let v: String = s[i + 1..].chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')).collect();
                    if let Some(v) = coordinate(&v, digits, decimals, lead_omitted) {
                        p[k] = v * to_mm * per_mm;
                        found = true;
                    }
                }
            }
            found.then_some(p)
        };
        let (first, second) = match upper.find("G85") {
            Some(i) => (&upper[..i], Some(&upper[i + 3..])),
            None => (upper.as_str(), None),
        };
        let Some(p) = point(first, at) else { continue };
        if let Some(end) = second {
            let q = point(end, p).unwrap_or(p);
            slots.push((tool, vec![p, q]));
            at = q;
        } else if routing || upper.starts_with("G01") || upper.starts_with("G00") {
            if down && !upper.starts_with("G00") {
                route.push(p);
            }
            at = p;
        } else {
            holes.push((tool, p));
            at = p;
        }
    }
    let mut sizes: Vec<u32> = holes.iter().map(|h| h.0).chain(slots.iter().map(|s| s.0)).collect();
    sizes.sort();
    sizes.dedup();
    let mut board = Board::default();
    for t in sizes {
        let Some(&d) = tools.get(&t) else {
            unknown_tool += holes.iter().filter(|h| h.0 == t).count() + slots.iter().filter(|s| s.0 == t).count();
            continue;
        };
        let r = d * per_mm / 2.0;
        let layer = format!("{stem} {}", size_name(d, per_mm));
        for (_, c) in holes.iter().filter(|h| h.0 == t) {
            board.objects.push((layer.clone(), vec![Path::closed(round(*c, r, tol))]));
        }
        let slot_layer = tf!("{} slots {}", stem, size_name(d, per_mm));
        for (_, line) in slots.iter().filter(|s| s.0 == t) {
            let loops: Vec<Vec<P>> = line.windows(2).map(|w| stadium(w[0], w[1], 2.0 * r, tol)).collect();
            for l in combine(&[], &loops, OverlayRule::Union) {
                board.objects.push((slot_layer.clone(), vec![Path::closed(l)]));
            }
        }
    }
    if unknown_tool > 0 {
        board.notes.push(tf!("{} holes use a drill size the file doesn't give: they were left out.", unknown_tool));
    }
    board
}

/// An aperture's outline at the origin, in the job's unit: loops turned for non-zero filling.
#[derive(Clone)]
struct Aperture {
    loops: Vec<Vec<P>>,
    /// The width a line drawn with it has (round apertures; others are drawn this wide too).
    width: f64,
}

/// A tiny calculator for aperture macros: numbers, $n, + - x /, brackets.
fn macro_value(text: &str, vars: &HashMap<usize, f64>) -> f64 {
    struct Calc<'a> {
        s: &'a [u8],
        i: usize,
        vars: &'a HashMap<usize, f64>,
    }
    impl Calc<'_> {
        fn peek(&self) -> u8 {
            *self.s.get(self.i).unwrap_or(&0)
        }
        fn sum(&mut self) -> f64 {
            let mut v = self.product();
            loop {
                match self.peek() {
                    b'+' => { self.i += 1; v += self.product(); }
                    b'-' => { self.i += 1; v -= self.product(); }
                    _ => return v,
                }
            }
        }
        fn product(&mut self) -> f64 {
            let mut v = self.single();
            loop {
                match self.peek() {
                    b'x' | b'X' | b'*' => { self.i += 1; v *= self.single(); }
                    b'/' => {
                        self.i += 1;
                        let d = self.single();
                        v = if d == 0.0 { 0.0 } else { v / d };
                    }
                    _ => return v,
                }
            }
        }
        fn single(&mut self) -> f64 {
            match self.peek() {
                b'-' => { self.i += 1; -self.single() }
                b'+' => { self.i += 1; self.single() }
                b'(' => {
                    self.i += 1;
                    let v = self.sum();
                    if self.peek() == b')' {
                        self.i += 1;
                    }
                    v
                }
                b'$' => {
                    self.i += 1;
                    let start = self.i;
                    while self.peek().is_ascii_digit() {
                        self.i += 1;
                    }
                    let n: usize = std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
                    self.vars.get(&n).copied().unwrap_or(0.0)
                }
                _ => {
                    let start = self.i;
                    while self.peek().is_ascii_digit() || self.peek() == b'.' {
                        self.i += 1;
                    }
                    std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
                }
            }
        }
    }
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    Calc { s: clean.as_bytes(), i: 0, vars }.sum()
}

fn turn(p: P, degrees: f64) -> P {
    let (s, c) = degrees.to_radians().sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
}

fn rectangle(c: P, w: f64, h: f64) -> Vec<P> {
    vec![[c[0] - w / 2.0, c[1] - h / 2.0], [c[0] + w / 2.0, c[1] - h / 2.0], [c[0] + w / 2.0, c[1] + h / 2.0], [c[0] - w / 2.0, c[1] + h / 2.0]]
}

fn regular(c: P, diameter: f64, sides: usize, start_degrees: f64) -> Vec<P> {
    let sides = sides.clamp(3, 64);
    (0..sides).map(|k| {
        let a = (start_degrees + 360.0 * k as f64 / sides as f64).to_radians();
        [c[0] + diameter / 2.0 * a.cos(), c[1] + diameter / 2.0 * a.sin()]
    })
    .collect()
}

/// An aperture macro's shape for the given values (in the file's unit, scaled by `scale`).
fn macro_shape(body: &[String], params: &[f64], scale: f64, tol: f64) -> Vec<Vec<P>> {
    let mut vars: HashMap<usize, f64> = params.iter().enumerate().map(|(k, v)| (k + 1, *v)).collect();
    let mut acc: Vec<Vec<P>> = Vec::new();
    for line in body {
        let line = line.trim();
        // Code 0 is a comment.
        if line.is_empty() || line.split([',', ' ']).next() == Some("0") {
            continue;
        }
        if let Some((var, expr)) = line.strip_prefix('$').and_then(|l| l.split_once('=')) {
            if let Ok(n) = var.trim().parse::<usize>() {
                let v = macro_value(expr, &vars);
                vars.insert(n, v);
            }
            continue;
        }
        let v: Vec<f64> = line.split(',').map(|t| macro_value(t, &vars)).collect();
        let get = |k: usize| v.get(k).copied().unwrap_or(0.0);
        let code = get(0) as i64;
        let dark = get(1) != 0.0;
        let s = |x: f64| x * scale;
        let (loops, dark, rot): (Vec<Vec<P>>, bool, f64) = match code {
            1 => (vec![round([s(get(3)), s(get(4))], s(get(2)) / 2.0, tol)], dark, get(5)),
            20 | 2 => {
                let (a, b, w) = ([s(get(3)), s(get(4))], [s(get(5)), s(get(6))], s(get(2)));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy).max(1e-12);
                let n = [-dy / len * w / 2.0, dx / len * w / 2.0];
                (vec![vec![[a[0] - n[0], a[1] - n[1]], [b[0] - n[0], b[1] - n[1]], [b[0] + n[0], b[1] + n[1]], [a[0] + n[0], a[1] + n[1]]]], dark, get(7))
            }
            21 => (vec![rectangle([s(get(4)), s(get(5))], s(get(2)), s(get(3)))], dark, get(6)),
            4 => {
                let n = get(2).max(0.0) as usize;
                let pts: Vec<P> = (0..=n).map(|k| [s(get(3 + 2 * k)), s(get(4 + 2 * k))]).collect();
                (vec![pts], dark, get(5 + 2 * n))
            }
            5 => (vec![regular([s(get(3)), s(get(4))], s(get(5)), get(2) as usize, 0.0)], dark, get(6)),
            7 => {
                // A thermal: a ring (its gaps left out).
                let c = [s(get(1)), s(get(2))];
                let ring = vec![anticlockwise(round(c, s(get(3)) / 2.0, tol)), anticlockwise(round(c, s(get(4)) / 2.0, tol)).into_iter().rev().collect()];
                (ring, true, get(6))
            }
            _ => continue,
        };
        let loops: Vec<Vec<P>> = loops.into_iter().filter(|l| l.len() >= 3).map(|l| l.into_iter().map(|p| turn(p, rot)).collect()).collect();
        let loops: Vec<Vec<P>> = if code == 7 { loops } else { loops.into_iter().map(anticlockwise).collect() };
        acc = combine(&acc, &loops, if dark { OverlayRule::Union } else { OverlayRule::Difference });
    }
    acc
}

/// A Gerber file: copper (and the like) as the outlines of what stays, an outline file as lines.
pub fn read_gerber(name: &str, text: &str, per_mm: f64) -> Board {
    let stem = stem_of(name);
    let tol = ROUNDNESS_MM * per_mm;
    let lower = name.to_ascii_lowercase();
    let mut outline = OUTLINE_ENDINGS.contains(&ending(name).as_str()) || ["edge", "outline", "profile"].iter().any(|w| stem_of(&lower).contains(w));
    // Blocks end with '*'; extended commands sit between '%' signs.
    let mut blocks: Vec<(bool, String)> = Vec::new();
    let mut extended = false;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '%' => {
                if extended && !current.trim().is_empty() {
                    blocks.push((true, std::mem::take(&mut current)));
                }
                current.clear();
                extended = !extended;
            }
            '*' if !extended => {
                blocks.push((false, std::mem::take(&mut current)));
            }
            '\r' | '\n' => {}
            c => current.push(c),
        }
    }
    let (mut digits, mut decimals, mut lead_omitted, mut absolute) = (3usize, 4usize, true, true);
    let mut to_mm = 25.4;
    let mut macros: HashMap<String, Vec<String>> = HashMap::new();
    let mut defined: HashMap<u32, (String, Vec<f64>)> = HashMap::new();
    let mut made: HashMap<u32, Aperture> = HashMap::new();
    let mut aperture: Option<u32> = None;
    let mut at: P = [0.0, 0.0];
    let mut mode = 1u32;
    let mut single_quadrant = false;
    let mut region = false;
    let mut contour: Vec<P> = Vec::new();
    let mut operation = 0u32;
    let mut dark = true;
    let mut acc: Vec<Vec<P>> = Vec::new();
    let mut batch: Vec<Vec<P>> = Vec::new();
    let mut lines: Vec<Vec<P>> = Vec::new();
    let mut repeated = false;
    let mut flashes = 0usize;
    let mut regions = 0usize;
    let close = |contour: &mut Vec<P>, batch: &mut Vec<Vec<P>>, regions: &mut usize| {
        if contour.len() >= 3 {
            batch.push(anticlockwise(std::mem::take(contour)));
            *regions += 1;
        }
        contour.clear();
    };
    for (ext, block) in blocks {
        if ext {
            let commands: Vec<&str> = block.split('*').map(str::trim).filter(|c| !c.is_empty()).collect();
            let Some(first) = commands.first() else { continue };
            if let Some(name) = first.strip_prefix("AM") {
                macros.insert(name.to_string(), commands[1..].iter().map(|c| c.to_string()).collect());
                continue;
            }
            for c in commands {
                if let Some(fs) = c.strip_prefix("FS") {
                    lead_omitted = !fs.starts_with('T');
                    absolute = !fs[1.min(fs.len())..].starts_with('I');
                    if let Some(i) = fs.find('X') {
                        let b = fs.as_bytes();
                        if let (Some(a), Some(d)) = (b.get(i + 1), b.get(i + 2)) {
                            digits = (a - b'0') as usize;
                            decimals = (d - b'0') as usize;
                        }
                    }
                } else if c == "MOMM" {
                    to_mm = 1.0;
                } else if c == "MOIN" {
                    to_mm = 25.4;
                } else if let Some(def) = c.strip_prefix("ADD") {
                    let n: String = def.chars().take_while(|c| c.is_ascii_digit()).collect();
                    let rest = &def[n.len()..];
                    let (kind, values) = rest.split_once(',').unwrap_or((rest, ""));
                    let values: Vec<f64> = values.split('X').filter_map(|v| v.trim().parse().ok()).collect();
                    if let Ok(n) = n.parse() {
                        defined.insert(n, (kind.to_string(), values));
                        made.remove(&n);
                    }
                } else if c == "LPD" || c == "LPC" {
                    let now_dark = c == "LPD";
                    if now_dark != dark {
                        acc = combine(&acc, &std::mem::take(&mut batch), if dark { OverlayRule::Union } else { OverlayRule::Difference });
                        dark = now_dark;
                    }
                } else if let Some(sr) = c.strip_prefix("SR") {
                    if !sr.is_empty() && sr != "X1Y1I0J0" && sr != "X1Y1I0.0J0.0" {
                        repeated = true;
                    }
                } else if c.starts_with("TF.FileFunction") && c.contains("Profile") {
                    outline = true;
                }
            }
            continue;
        }
        let word = block.trim();
        if word.starts_with("G04") || word.is_empty() {
            continue;
        }
        if word.starts_with("M02") || word.starts_with("M00") {
            break;
        }
        // The letters and their numbers, in order.
        let mut codes: Vec<(char, &str)> = Vec::new();
        let bytes = word.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let letter = bytes[i] as char;
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || matches!(bytes[j], b'-' | b'+' | b'.')) {
                j += 1;
            }
            if letter.is_ascii_alphabetic() {
                codes.push((letter.to_ascii_uppercase(), &word[start..j]));
            }
            i = j.max(i + 1);
        }
        let mut next = at;
        let (mut off_i, mut off_j) = (0.0, 0.0);
        let mut moved = false;
        let unit = to_mm * per_mm;
        for (letter, value) in &codes {
            match letter {
                'G' => match value.parse::<u32>().unwrap_or(0) {
                    1 => mode = 1,
                    2 => mode = 2,
                    3 => mode = 3,
                    36 => {
                        region = true;
                        contour.clear();
                    }
                    37 => {
                        close(&mut contour, &mut batch, &mut regions);
                        region = false;
                    }
                    74 => single_quadrant = true,
                    75 => single_quadrant = false,
                    70 => to_mm = 25.4,
                    71 => to_mm = 1.0,
                    90 => absolute = true,
                    91 => absolute = false,
                    _ => {}
                },
                'X' | 'Y' => {
                    if let Some(v) = coordinate(value, digits, decimals, lead_omitted) {
                        let k = if *letter == 'X' { 0 } else { 1 };
                        next[k] = if absolute { v * unit } else { at[k] + v * unit };
                        moved = true;
                    }
                }
                'I' => off_i = coordinate(value, digits, decimals, lead_omitted).unwrap_or(0.0) * unit,
                'J' => off_j = coordinate(value, digits, decimals, lead_omitted).unwrap_or(0.0) * unit,
                'D' => {
                    let d = value.parse::<u32>().unwrap_or(0);
                    if d >= 10 {
                        aperture = Some(d);
                    } else {
                        operation = d;
                    }
                }
                _ => {}
            }
        }
        let says_operation = codes.iter().any(|(l, v)| *l == 'D' && v.parse::<u32>().is_ok_and(|d| d < 10));
        if !says_operation && !moved {
            continue;
        }
        match operation {
            1 => {
                // The points the move goes through, after `at`.
                let path: Vec<P> = if mode == 1 {
                    vec![next]
                } else {
                    let mut centre = [at[0] + off_i, at[1] + off_j];
                    if single_quadrant {
                        // The offsets come without their signs: the centre is the one equally far from both ends.
                        let mut best = f64::MAX;
                        for (si, sj) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                            let c = [at[0] + si * off_i.abs(), at[1] + sj * off_j.abs()];
                            let miss = ((at[0] - c[0]).hypot(at[1] - c[1]) - (next[0] - c[0]).hypot(next[1] - c[1])).abs();
                            if miss < best {
                                best = miss;
                                centre = c;
                            }
                        }
                    }
                    arc_points(at, next, centre, mode == 2, tol)
                };
                if region {
                    if contour.is_empty() {
                        contour.push(at);
                    }
                    contour.extend(path);
                } else {
                    // Each move on its own, joined at the end (programs draw some twice).
                    lines.push(std::iter::once(at).chain(path.iter().copied()).collect());
                    let Some(ap) = (!outline).then(|| aperture.and_then(|n| aperture_of(n, &defined, &macros, &mut made, unit, tol))).flatten() else {
                        at = next;
                        continue;
                    };
                    if ap.width > 0.0 {
                        let mut from = at;
                        for p in path {
                            batch.push(stadium(from, p, ap.width, tol));
                            from = p;
                        }
                    }
                }
            }
            2 => {
                if region {
                    close(&mut contour, &mut batch, &mut regions);
                }
            }
            3 => {
                if outline {
                    // A flash on an outline file is a hole or a mark: kept as its outline.
                    if let Some(ap) = aperture.and_then(|n| aperture_of(n, &defined, &macros, &mut made, unit, tol)) {
                        for l in ap.loops {
                            let mut moved: Vec<P> = l.into_iter().map(|p| [p[0] + next[0], p[1] + next[1]]).collect();
                            moved.push(moved[0]);
                            lines.push(moved);
                        }
                    }
                } else if let Some(ap) = aperture.and_then(|n| aperture_of(n, &defined, &macros, &mut made, unit, tol)) {
                    batch.extend(ap.loops.iter().map(|l| l.iter().map(|p| [p[0] + next[0], p[1] + next[1]]).collect::<Vec<P>>()));
                    flashes += 1;
                }
            }
            _ => {}
        }
        at = next;
    }
    close(&mut contour, &mut batch, &mut regions);
    let mut board = Board::default();
    // Only lines, with no pads or filled areas (an outline, a drawing): they come as lines.
    if outline || (flashes == 0 && regions == 0 && !lines.is_empty()) {
        // Some programs draw parts of the outline twice: one copy of each, or joining goes astray.
        let length = |l: &[P]| l.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
        let grid = |p: P| ((p[0] / (4.0 * tol)).round() as i64, (p[1] / (4.0 * tol)).round() as i64);
        let mut seen: HashMap<((i64, i64), (i64, i64)), Vec<f64>> = HashMap::new();
        let mut kept: Vec<Vec<P>> = Vec::new();
        for l in lines.into_iter().filter(|l| l.len() >= 2) {
            let (a, b, len) = (grid(l[0]), grid(*l.last().unwrap()), length(&l));
            // Specks no longer than the drawing is exact add nothing and would stay alone.
            if len <= 2.0 * tol {
                continue;
            }
            let lengths = seen.entry(if a <= b { (a, b) } else { (b, a) }).or_default();
            if lengths.iter().any(|k| (k - len).abs() <= 4.0 * tol + 0.01 * len) {
                continue;
            }
            lengths.push(len);
            kept.push(l);
        }
        let paths: Vec<Path> = kept.into_iter().map(Path::open).collect();
        for p in crate::edit::join_paths(paths, 2.0 * tol) {
            board.objects.push((stem.clone(), vec![p]));
        }
    } else {
        acc = combine(&acc, &batch, if dark { OverlayRule::Union } else { OverlayRule::Difference });
        // Each outline with the holes inside it is one object.
        let mut current: Vec<Path> = Vec::new();
        for l in acc {
            if signed_area(&l) > 0.0 && !current.is_empty() {
                board.objects.push((stem.clone(), std::mem::take(&mut current)));
            }
            current.push(Path::closed(l));
        }
        if !current.is_empty() {
            board.objects.push((stem.clone(), current));
        }
    }
    let _ = (flashes, regions);
    if repeated {
        board.notes.push(crate::i18n::tr("The file repeats parts of itself (step and repeat): only the first copy came in.").to_string());
    }
    board
}

/// The aperture numbered `n`, made once in the job's unit.
fn aperture_of(n: u32, defined: &HashMap<u32, (String, Vec<f64>)>, macros: &HashMap<String, Vec<String>>, made: &mut HashMap<u32, Aperture>, unit: f64, tol: f64) -> Option<Aperture> {
    if let Some(a) = made.get(&n) {
        return Some(a.clone());
    }
    let (kind, v) = defined.get(&n)?;
    let get = |k: usize| v.get(k).copied().unwrap_or(0.0) * unit;
    let o = [0.0, 0.0];
    let made_one = match kind.as_str() {
        "C" => Aperture { loops: vec![round(o, get(0) / 2.0, tol)], width: get(0) },
        "R" => Aperture { loops: vec![rectangle(o, get(0), get(1))], width: get(0).min(get(1)) },
        "O" => {
            let (w, h) = (get(0), get(1));
            let loop_ = if w >= h { stadium([-(w - h) / 2.0, 0.0], [(w - h) / 2.0, 0.0], h, tol) } else { stadium([0.0, -(h - w) / 2.0], [0.0, (h - w) / 2.0], w, tol) };
            Aperture { loops: vec![loop_], width: w.min(h) }
        }
        "P" => Aperture { loops: vec![regular(o, get(0), v.get(1).copied().unwrap_or(3.0) as usize, v.get(2).copied().unwrap_or(0.0))], width: get(0) },
        name => {
            let body = macros.get(name)?;
            let loops = macro_shape(body, v, unit, tol);
            Aperture { loops, width: 0.0 }
        }
    };
    let made_one = Aperture { loops: made_one.loops.into_iter().filter(|l| l.len() >= 3).map(|l| if kind.len() == 1 { anticlockwise(l) } else { l }).collect(), width: made_one.width };
    made.insert(n, made_one.clone());
    Some(made_one)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(b: &Board) -> f64 {
        b.objects.iter().flat_map(|o| o.1.iter()).map(|p| signed_area(&p.pts)).sum()
    }

    #[test]
    fn numbers_read_with_their_zeros_left_out() {
        assert!((coordinate("-007112", 3, 3, true).unwrap() + 7.112).abs() < 1e-9);
        assert!((coordinate("15", 3, 3, true).unwrap() - 0.015).abs() < 1e-9);
        assert!((coordinate("015", 3, 3, false).unwrap() - 15.0).abs() < 1e-9);
        assert!((coordinate("15", 3, 3, false).unwrap() - 150.0).abs() < 1e-9);
        assert!((coordinate("1.5", 3, 3, false).unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn drill_holes_come_as_circles_on_a_layer_per_size() {
        let text = "M48\nMETRIC,TZ\nT1C0.800\nT2C3.0\n%\nG90\nG05\nT1\nX010000Y010000\nX020000Y010000\nT2\nX5.0Y5.0\nX1.0Y1.0G85X6.0Y1.0\nM30\n";
        let b = read("plate.drl", text, 1.0);
        assert_eq!(b.objects.len(), 4);
        let layers: Vec<&str> = b.objects.iter().map(|o| o.0.as_str()).collect();
        assert_eq!(layers.iter().filter(|l| l.contains("Ø0.80 mm")).count(), 2);
        let c = &b.objects[0].1[0].pts;
        let (lo, hi) = crate::geom::bbox(&b.objects[0].1).unwrap();
        assert!(((hi[0] - lo[0]) - 0.8).abs() < 0.01 && ((lo[0] + hi[0]) / 2.0 - 10.0).abs() < 1e-6, "{lo:?} {hi:?} {}", c.len());
        // The slot: 5 long between centres, 3 wide.
        let slot = b.objects.iter().find(|o| o.0.contains("slots")).unwrap();
        let (lo, hi) = crate::geom::bbox(&slot.1).unwrap();
        assert!(((hi[0] - lo[0]) - 8.0).abs() < 0.01 && ((hi[1] - lo[1]) - 3.0).abs() < 0.01);
    }

    #[test]
    fn inch_drill_files_are_scaled() {
        let text = "M48\nINCH,LZ\nT1C0.0400\n%\nT1\nX01Y01\nM30\n";
        let b = read("x.drl", text, 1.0);
        let (lo, hi) = crate::geom::bbox(&b.objects[0].1).unwrap();
        assert!(((lo[0] + hi[0]) / 2.0 - 25.4).abs() < 1e-6 && ((hi[0] - lo[0]) - 1.016).abs() < 0.01);
    }

    #[test]
    fn gerber_tracks_and_pads_merge_into_copper() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,1.000000*%\n%ADD11R,2.000000X2.000000*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nD11*\nX10000000Y0D03*\nX30000000Y0D03*\nM02*\n";
        let b = read("board-F_Cu.gbr", text, 1.0);
        // The track and the first pad make one piece, the other pad another.
        assert_eq!(b.objects.len(), 2, "{:?}", b.objects.iter().map(|o| o.1.len()).collect::<Vec<_>>());
        let expected = 10.0 * 1.0 + std::f64::consts::PI * 0.25 / 2.0 + 4.0 - 1.0 * 1.0 + 4.0;
        assert!((area(&b) - expected).abs() < 0.05, "{}", area(&b));
    }

    #[test]
    fn gerber_regions_and_clear_polarity() {
        let text = "%FSLAX24Y24*%\n%MOIN*%\nG36*\nX0Y0D02*\nX10000Y0D01*\nX10000Y10000D01*\nX0Y10000D01*\nX0Y0D01*\nG37*\n%LPC*%\n%ADD10C,0.5*%\nD10*\nX5000Y5000D03*\nM02*\n";
        let b = read("pour.gbr", text, 1.0 / 25.4);
        let expected = 1.0 - std::f64::consts::PI * 0.0625;
        assert!((area(&b) - expected).abs() < 0.002, "{}", area(&b));
        assert_eq!(b.objects.len(), 1);
        assert_eq!(b.objects[0].1.len(), 2);
    }

    #[test]
    fn an_outline_file_comes_as_its_lines() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%TF.FileFunction,Profile,NP*%\n%ADD10C,0.05*%\nD10*\nX0Y0D02*\nX20000000Y0D01*\nX20000000Y10000000D02*\nX20000000Y0D01*\nX20000000Y10000000D02*\nG75*\nG03*\nX10000000Y20000000I-10000000J0D01*\nG01*\nX0Y20000000D01*\nX0Y0D01*\nM02*\n";
        let b = read("board.gbr", text, 1.0);
        assert_eq!(b.objects.len(), 1);
        let p = &b.objects[0].1[0];
        assert!(p.closed);
        let expected = 20.0 * 10.0 + 10.0 * 10.0 + std::f64::consts::PI * 100.0 / 4.0;
        assert!((signed_area(&p.pts).abs() - expected).abs() < 0.1, "{}", signed_area(&p.pts));
    }

    #[test]
    fn an_outline_drawn_twice_in_places_still_closes() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.2*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nX10000000Y0D02*\nX10000000Y5000000D01*\nX0Y0D02*\nX10000000Y0D01*\nX10000000Y5000000D02*\nX0Y5000000D01*\nX0Y0D01*\nM02*\n";
        let b = read("plate.gko", text, 1.0);
        assert_eq!(b.objects.len(), 1);
        assert!(b.objects[0].1[0].closed);
        assert!((signed_area(&b.objects[0].1[0].pts).abs() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn aperture_macros_are_drawn() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%AMRoundRect*\n0 Rectangle with rounded corners*\n0 $1 Rounding radius*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n1,1,$1+$1,$4,$5*\n1,1,$1+$1,$6,$7*\n1,1,$1+$1,$8,$9*\n20,1,$1+$1,$2,$3,$4,$5,0*\n20,1,$1+$1,$4,$5,$6,$7,0*\n20,1,$1+$1,$6,$7,$8,$9,0*\n20,1,$1+$1,$8,$9,$2,$3,0*%\n%ADD10RoundRect,0.25X-0.75X-0.5X0.75X-0.5X0.75X0.5X-0.75X0.5X0*%\nD10*\nX0Y0D03*\nM02*\n";
        let b = read("pads.gbr", text, 1.0);
        let (lo, hi) = crate::geom::bbox(&b.objects[0].1).unwrap();
        assert!(((hi[0] - lo[0]) - 2.0).abs() < 0.01 && ((hi[1] - lo[1]) - 1.5).abs() < 0.01, "{lo:?} {hi:?}");
        let expected = 2.0 * 1.5 - (4.0 - std::f64::consts::PI) * 0.0625;
        assert!((area(&b) - expected).abs() < 0.01, "{}", area(&b));
    }

    /// The board files in essais/pcb, when they are there.
    #[test]
    #[ignore]
    fn try_boards() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("essais/pcb");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut names: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names {
            let name = path.to_string_lossy().to_string();
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).to_string();
            let started = std::time::Instant::now();
            let b = read(&name, &text, 1.0);
            let paths: Vec<Path> = b.objects.iter().flat_map(|o| o.1.clone()).collect();
            let mut layers: Vec<&str> = b.objects.iter().map(|o| o.0.as_str()).collect();
            layers.dedup();
            if let (Ok(out), Some((lo, hi))) = (std::env::var("BURIN_PCB_SVG"), crate::geom::bbox(&paths)) {
                let mut svg = format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='{} {} {} {}' width='1200'><rect x='{}' y='{}' width='100%' height='100%' fill='white'/><g fill='#b33' fill-rule='evenodd' stroke='black' stroke-width='0.05' transform='scale(1,-1)'>", lo[0] - 1.0, -hi[1] - 1.0, hi[0] - lo[0] + 2.0, hi[1] - lo[1] + 2.0, lo[0] - 1.0, -hi[1] - 1.0);
                for o in &b.objects {
                    svg += "<path d='";
                    for p in &o.1 {
                        for (k, q) in p.pts.iter().enumerate() {
                            svg += &format!("{}{:.4},{:.4} ", if k == 0 { "M" } else { "L" }, q[0], q[1]);
                        }
                        if p.closed {
                            svg += "Z ";
                        }
                    }
                    svg += if o.1.iter().all(|p| p.closed) { "'/>" } else { "' fill='none'/>" };
                }
                svg += "</g></svg>";
                std::fs::write(format!("{out}/{}.svg", path.file_name().unwrap().to_string_lossy()), svg).unwrap();
            }
            println!("{}: {} objects, {} loops, layers {:?}, box {:?}, {:?}, {:?}", path.file_name().unwrap().to_string_lossy(), b.objects.len(), paths.len(), &layers[..layers.len().min(6)], crate::geom::bbox(&paths), started.elapsed(), b.notes);
        }
    }
}
