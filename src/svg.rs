//! Reading drawings from SVG files.

use crate::geom::{Path, P};

/// What was found in a file: one list of outlines per drawn object.
#[derive(Debug)]
pub struct SvgDrawing {
    pub objects: Vec<Vec<Path>>,
    /// Text, pictures and other things that have no outline to read.
    pub skipped: usize,
    /// Objects the file shows only part of, through a mask.
    pub masked: usize,
}

/// x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, p: P) -> P {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    /// `inner` happens first, then this one.
    fn then_inner(&self, inner: Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = inner.0;
        Matrix([a * g + c * h, b * g + d * h, a * i + c * j, b * i + d * j, a * k + c * l + e, b * k + d * l + f])
    }

    /// Roughly how much it enlarges.
    fn size(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        (a * d - b * c).abs().sqrt().max(1e-9)
    }
}

/// Reads numbers, commands and arc flags out of an attribute.
struct Scan<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    fn new(text: &'a str) -> Self {
        Scan { text: text.as_bytes(), at: 0 }
    }

    fn skip_gaps(&mut self) {
        while self.at < self.text.len() && (self.text[self.at].is_ascii_whitespace() || self.text[self.at] == b',') {
            self.at += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip_gaps();
        let c = *self.text.get(self.at)?;
        if c.is_ascii_alphabetic() {
            self.at += 1;
            Some(c)
        } else {
            None
        }
    }

    fn more_numbers(&mut self) -> bool {
        self.skip_gaps();
        self.text.get(self.at).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_gaps();
        let start = self.at;
        if matches!(self.text.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let mut seen_dot = false;
        while let Some(c) = self.text.get(self.at) {
            if c.is_ascii_digit() {
                self.at += 1;
            } else if *c == b'.' && !seen_dot {
                seen_dot = true;
                self.at += 1;
            } else {
                break;
            }
        }
        if matches!(self.text.get(self.at), Some(b'e' | b'E')) {
            let mut look = self.at + 1;
            if matches!(self.text.get(look), Some(b'-' | b'+')) {
                look += 1;
            }
            if self.text.get(look).is_some_and(u8::is_ascii_digit) {
                self.at = look;
                while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
            }
        }
        std::str::from_utf8(&self.text[start..self.at]).ok()?.parse().ok()
    }

    /// An arc flag is one digit and may touch the next number.
    fn flag(&mut self) -> Option<bool> {
        self.skip_gaps();
        let c = *self.text.get(self.at)?;
        self.at += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }
}

fn numbers(text: &str) -> Vec<f64> {
    let mut scan = Scan::new(text);
    let mut out = Vec::new();
    while scan.more_numbers() {
        match scan.number() {
            Some(v) => out.push(v),
            None => break,
        }
    }
    out
}

fn transform_of(text: &str) -> Matrix {
    let mut m = Matrix::IDENTITY;
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')') else { break };
        let name = rest[..open].trim_matches(|c: char| c.is_whitespace() || c == ',');
        let v = numbers(&rest[open + 1..open + close]);
        let get = |i: usize, or: f64| v.get(i).copied().unwrap_or(or);
        let step = match name {
            "matrix" if v.len() == 6 => Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]),
            "translate" => Matrix([1.0, 0.0, 0.0, 1.0, get(0, 0.0), get(1, 0.0)]),
            "scale" => Matrix([get(0, 1.0), 0.0, 0.0, get(1, get(0, 1.0)), 0.0, 0.0]),
            "rotate" => {
                let (s, c) = get(0, 0.0).to_radians().sin_cos();
                let (cx, cy) = (get(1, 0.0), get(2, 0.0));
                Matrix([1.0, 0.0, 0.0, 1.0, cx, cy]).then_inner(Matrix([c, s, -s, c, 0.0, 0.0])).then_inner(Matrix([1.0, 0.0, 0.0, 1.0, -cx, -cy]))
            }
            "skewX" => Matrix([1.0, 0.0, get(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0]),
            "skewY" => Matrix([1.0, get(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            _ => Matrix::IDENTITY,
        };
        m = m.then_inner(step);
        rest = &rest[open + close + 1..];
    }
    m
}

/// A length with its unit, in inches. Percentages have no size of their own.
fn inches(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(text.len());
    let value: f64 = text[..split].trim().parse().ok()?;
    let per_inch = match text[split..].trim() {
        "mm" => 25.4,
        "cm" => 2.54,
        "in" => 1.0,
        "pt" => 72.0,
        "pc" => 6.0,
        "" | "px" => 96.0,
        _ => return None,
    };
    Some(value / per_inch)
}

/// A plain number from an attribute; `10px` counts as 10.
fn attr(node: roxmltree::Node, name: &str) -> f64 {
    node.attribute(name).and_then(|t| numbers(t).first().copied()).unwrap_or(0.0)
}

/// Collects outlines in the file's own coordinates and hands them over transformed.
struct Outlines {
    m: Matrix,
    /// Curves are followed to about this distance, in job units.
    tolerance: f64,
    done: Vec<Path>,
    current: Vec<P>,
}

impl Outlines {
    fn new(m: Matrix, tolerance: f64) -> Self {
        Outlines { m, tolerance, done: Vec::new(), current: Vec::new() }
    }

    fn steps(&self, length: f64) -> usize {
        ((length * self.m.size() / self.tolerance).sqrt().ceil() as usize).clamp(4, 96)
    }

    fn finish(&mut self, closed: bool) {
        let mut pts: Vec<P> = Vec::with_capacity(self.current.len());
        for p in self.current.drain(..) {
            let q = self.m.apply(p);
            if pts.last().is_none_or(|l: &P| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-9) {
                pts.push(q);
            }
        }
        let ends_meet = pts.len() > 2 && (pts[0][0] - pts[pts.len() - 1][0]).hypot(pts[0][1] - pts[pts.len() - 1][1]) < self.tolerance * 0.1;
        if ends_meet {
            pts.pop();
        }
        if pts.len() >= 3 && (closed || ends_meet) {
            self.done.push(Path::closed(pts));
        } else if pts.len() >= 2 {
            self.done.push(Path::open(pts));
        }
    }

    fn cubic(&mut self, a: P, b: P, c: P, d: P) {
        let d1 = |p: P, q: P| (p[0] - q[0]).hypot(p[1] - q[1]);
        let n = self.steps(d1(a, b) + d1(b, c) + d1(c, d));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.current.push([w0 * a[0] + w1 * b[0] + w2 * c[0] + w3 * d[0], w0 * a[1] + w1 * b[1] + w2 * c[1] + w3 * d[1]]);
        }
    }

    fn quadratic(&mut self, a: P, b: P, c: P) {
        let third = |p: P, q: P| [p[0] + (q[0] - p[0]) * 2.0 / 3.0, p[1] + (q[1] - p[1]) * 2.0 / 3.0];
        self.cubic(a, third(a, b), third(c, b), c);
    }

    /// An arc from `from` to `to`, worked out the way the SVG format defines it.
    #[allow(clippy::too_many_arguments)]
    fn arc(&mut self, from: P, mut rx: f64, mut ry: f64, angle: f64, large: bool, sweep: bool, to: P) {
        rx = rx.abs();
        ry = ry.abs();
        if rx < 1e-12 || ry < 1e-12 {
            self.current.push(to);
            return;
        }
        let (s, c) = angle.to_radians().sin_cos();
        let (dx, dy) = ((from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0);
        let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
        let grow = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if grow > 1.0 {
            rx *= grow.sqrt();
            ry *= grow.sqrt();
        }
        let top = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
        let under = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut k = (top / under.max(1e-30)).max(0.0).sqrt();
        if large == sweep {
            k = -k;
        }
        let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
        let (cx, cy) = (c * cx1 - s * cy1 + (from[0] + to[0]) / 2.0, s * cx1 + c * cy1 + (from[1] + to[1]) / 2.0);
        let start = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
        let end = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
        let mut turn = end - start;
        if sweep && turn < 0.0 {
            turn += std::f64::consts::TAU;
        } else if !sweep && turn > 0.0 {
            turn -= std::f64::consts::TAU;
        }
        let n = self.steps(turn.abs() * rx.max(ry)).max(8);
        for i in 1..=n {
            let t = start + turn * i as f64 / n as f64;
            let (px, py) = (rx * t.cos(), ry * t.sin());
            self.current.push([c * px - s * py + cx, s * px + c * py + cy]);
        }
    }

    fn path_data(&mut self, d: &str) {
        let mut scan = Scan::new(d);
        let (mut at, mut start): (P, P) = ([0.0, 0.0], [0.0, 0.0]);
        // The control point of the last curve, for the smooth curve commands.
        let mut cubic_ctrl: Option<P> = None;
        let mut quad_ctrl: Option<P> = None;
        let mut command = b' ';
        loop {
            match scan.command() {
                Some(c) => command = c,
                None if scan.more_numbers() && command != b' ' => {
                    // A number with no letter repeats the last command.
                    if command == b'M' {
                        command = b'L';
                    } else if command == b'm' {
                        command = b'l';
                    }
                }
                None => break,
            }
            let rel = command.is_ascii_lowercase();
            let base = if rel { at } else { [0.0, 0.0] };
            let read = |scan: &mut Scan| -> Option<P> { Some([scan.number()? + base[0], scan.number()? + base[1]]) };
            let kind = command.to_ascii_uppercase();
            let ok = (|| -> Option<()> {
                match kind {
                    b'M' => {
                        let p = read(&mut scan)?;
                        if !self.current.is_empty() {
                            self.finish(false);
                        }
                        self.current.push(p);
                        at = p;
                        start = p;
                    }
                    b'L' => {
                        let p = read(&mut scan)?;
                        self.begin_if_needed(at);
                        self.current.push(p);
                        at = p;
                    }
                    b'H' => {
                        let x = scan.number()? + base[0];
                        self.begin_if_needed(at);
                        at = [x, at[1]];
                        self.current.push(at);
                    }
                    b'V' => {
                        let y = scan.number()? + base[1];
                        self.begin_if_needed(at);
                        at = [at[0], y];
                        self.current.push(at);
                    }
                    b'C' | b'S' => {
                        let first = if kind == b'C' {
                            read(&mut scan)?
                        } else {
                            cubic_ctrl.map_or(at, |c| [2.0 * at[0] - c[0], 2.0 * at[1] - c[1]])
                        };
                        let second = read(&mut scan)?;
                        let end = read(&mut scan)?;
                        self.begin_if_needed(at);
                        self.cubic(at, first, second, end);
                        cubic_ctrl = Some(second);
                        at = end;
                    }
                    b'Q' | b'T' => {
                        let ctrl = if kind == b'Q' {
                            read(&mut scan)?
                        } else {
                            quad_ctrl.map_or(at, |c| [2.0 * at[0] - c[0], 2.0 * at[1] - c[1]])
                        };
                        let end = read(&mut scan)?;
                        self.begin_if_needed(at);
                        self.quadratic(at, ctrl, end);
                        quad_ctrl = Some(ctrl);
                        at = end;
                    }
                    b'A' => {
                        let (rx, ry, angle) = (scan.number()?, scan.number()?, scan.number()?);
                        let (large, sweep) = (scan.flag()?, scan.flag()?);
                        let end = read(&mut scan)?;
                        self.begin_if_needed(at);
                        self.arc(at, rx, ry, angle, large, sweep, end);
                        at = end;
                    }
                    b'Z' => {
                        self.finish(true);
                        at = start;
                    }
                    _ => return None,
                }
                Some(())
            })();
            if !matches!(kind, b'C' | b'S') {
                cubic_ctrl = None;
            }
            if !matches!(kind, b'Q' | b'T') {
                quad_ctrl = None;
            }
            // A broken file keeps what was read before the fault.
            if ok.is_none() {
                break;
            }
        }
        if !self.current.is_empty() {
            self.finish(false);
        }
    }

    fn begin_if_needed(&mut self, at: P) {
        if self.current.is_empty() {
            self.current.push(at);
        }
    }

    fn ellipse(&mut self, c: P, rx: f64, ry: f64) {
        let n = self.steps(std::f64::consts::TAU * rx.max(ry)).max(12) * 2;
        for i in 0..n {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            self.current.push([c[0] + rx * t.cos(), c[1] + ry * t.sin()]);
        }
        self.finish(true);
    }

    fn rectangle(&mut self, x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) {
        let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
        if rx <= 0.0 || ry <= 0.0 {
            self.current.extend([[x, y], [x + w, y], [x + w, y + h], [x, y + h]]);
        } else {
            self.current.push([x + rx, y]);
            self.current.push([x + w - rx, y]);
            self.arc([x + w - rx, y], rx, ry, 0.0, false, true, [x + w, y + ry]);
            self.current.push([x + w, y + h - ry]);
            self.arc([x + w, y + h - ry], rx, ry, 0.0, false, true, [x + w - rx, y + h]);
            self.current.push([x + rx, y + h]);
            self.arc([x + rx, y + h], rx, ry, 0.0, false, true, [x, y + h - ry]);
            self.current.push([x, y + ry]);
            self.arc([x, y + ry], rx, ry, 0.0, false, true, [x + rx, y]);
        }
        self.finish(true);
    }
}

fn hidden(node: roxmltree::Node) -> bool {
    let plain = |t: &str| t.replace(' ', "");
    node.attribute("display") == Some("none") || node.attribute("style").is_some_and(|s| plain(s).contains("display:none"))
}

fn has_mask(node: roxmltree::Node) -> bool {
    node.attribute("clip-path").is_some() || node.attribute("style").is_some_and(|s| s.contains("clip-path"))
}

fn visit(node: roxmltree::Node, m: Matrix, tolerance: f64, inside_mask: bool, out: &mut SvgDrawing) {
    for child in node.children().filter(|c| c.is_element()) {
        if hidden(child) {
            continue;
        }
        let masked = inside_mask || has_mask(child);
        let here = child.attribute("transform").map_or(m, |t| m.then_inner(transform_of(t)));
        let mut shape = Outlines::new(here, tolerance);
        match child.tag_name().name() {
            "g" | "a" | "svg" | "switch" => {
                visit(child, here, tolerance, masked, out);
                continue;
            }
            "path" => shape.path_data(child.attribute("d").unwrap_or("")),
            "rect" => {
                let (rx, ry) = (child.attribute("rx").map(|_| attr(child, "rx")), child.attribute("ry").map(|_| attr(child, "ry")));
                let (rx, ry) = (rx.or(ry).unwrap_or(0.0), ry.or(rx).unwrap_or(0.0));
                let (w, h) = (attr(child, "width"), attr(child, "height"));
                if w > 0.0 && h > 0.0 {
                    shape.rectangle(attr(child, "x"), attr(child, "y"), w, h, rx, ry);
                }
            }
            "circle" => {
                let r = attr(child, "r");
                if r > 0.0 {
                    shape.ellipse([attr(child, "cx"), attr(child, "cy")], r, r);
                }
            }
            "ellipse" => {
                let (rx, ry) = (attr(child, "rx"), attr(child, "ry"));
                if rx > 0.0 && ry > 0.0 {
                    shape.ellipse([attr(child, "cx"), attr(child, "cy")], rx, ry);
                }
            }
            "line" => {
                shape.current.push([attr(child, "x1"), attr(child, "y1")]);
                shape.current.push([attr(child, "x2"), attr(child, "y2")]);
                shape.finish(false);
            }
            name @ ("polyline" | "polygon") => {
                let v = numbers(child.attribute("points").unwrap_or(""));
                shape.current.extend(v.chunks_exact(2).map(|c| [c[0], c[1]]));
                shape.finish(name == "polygon");
            }
            "text" | "image" | "use" | "flowRoot" => {
                out.skipped += 1;
                continue;
            }
            _ => continue,
        }
        if !shape.done.is_empty() {
            out.masked += usize::from(masked);
            out.objects.push(shape.done);
        }
    }
}

/// Reads a drawing, at its real size in job units (`per_inch` is 1 for inches, 25.4 for mm).
/// The bottom of the drawing's page lands on Y = 0, as SVG counts downward from the top.
pub fn read(text: &str, per_inch: f64) -> Result<SvgDrawing, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, options).map_err(|e| tf!("This SVG file can't be read: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(crate::i18n::tr("This file is not an SVG drawing.").into());
    }
    let view = root.attribute("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0);
    let (width, height) = (root.attribute("width").and_then(inches), root.attribute("height").and_then(inches));
    let inch_per_unit = match (&view, width) {
        (Some(v), Some(w)) if w > 0.0 => w / v[2],
        _ => 1.0 / 96.0,
    };
    let k = inch_per_unit * per_inch;
    let (x0, y0, page) = match &view {
        Some(v) => (v[0], v[1], Some(v[3])),
        None => (0.0, 0.0, height.map(|h| h * 96.0)),
    };
    // Page coordinates to job coordinates: same scale, Y turned over.
    let flip = Matrix([k, 0.0, 0.0, -k, -x0 * k, (page.unwrap_or(0.0) + y0) * k]);
    let top = root.attribute("transform").map_or(flip, |t| flip.then_inner(transform_of(t)));
    let mut out = SvgDrawing { objects: Vec::new(), skipped: 0, masked: 0 };
    visit(root, top, 0.002 * per_inch, false, &mut out);
    // With no page height to go by, the drawing itself sits on Y = 0.
    if page.is_none() {
        let low = out.objects.iter().flatten().flat_map(|p| p.pts.iter()).map(|p| p[1]).fold(f64::MAX, f64::min);
        for p in out.objects.iter_mut().flatten().flat_map(|p| p.pts.iter_mut()) {
            p[1] -= low;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{bbox, signed_area};

    fn box_of(paths: &[Path]) -> (P, P) {
        bbox(paths).unwrap()
    }

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 0.02, "{a} is not {b}");
    }

    #[test]
    fn real_size_comes_from_the_page_and_y_is_turned_over() {
        let s = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 200 100">
            <rect x="20" y="10" width="40" height="20"/></svg>"#;
        let d = read(s, 25.4).unwrap();
        let (lo, hi) = box_of(&d.objects[0]);
        // Two page units are one millimetre; 10 down from the top of 100 is 45 up.
        near(lo[0], 10.0);
        near(hi[0], 30.0);
        near(hi[1], 45.0);
        near(lo[1], 35.0);
        assert!(d.objects[0][0].closed);
    }

    #[test]
    fn the_same_file_reads_in_inches_too() {
        let s = r#"<svg width="1in" height="1in" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        let (lo, hi) = box_of(&read(s, 1.0).unwrap().objects[0]);
        near(hi[0] - lo[0], 1.0);
    }

    #[test]
    fn a_page_with_no_size_counts_96_units_to_the_inch() {
        let s = r#"<svg viewBox="0 0 96 96"><rect width="96" height="48"/></svg>"#;
        let (lo, hi) = box_of(&read(s, 1.0).unwrap().objects[0]);
        near(hi[0] - lo[0], 1.0);
        near(hi[1] - lo[1], 0.5);
    }

    #[test]
    fn circles_and_ellipses_have_the_right_size_and_area() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><circle cx="50" cy="50" r="48"/><ellipse cx="50" cy="50" rx="48" ry="24"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects.len(), 2);
        let area = signed_area(&d.objects[0][0].pts).abs();
        assert!((area - std::f64::consts::PI * 48.0 * 48.0).abs() < 40.0, "{area}");
        let (lo, hi) = box_of(&d.objects[1]);
        near(hi[0] - lo[0], 96.0);
        near(hi[1] - lo[1], 48.0);
    }

    #[test]
    fn path_commands_draw_what_they_say() {
        // Relative moves, horizontal and vertical lines, close.
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M10 10h20v20H10z"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        let p = &d.objects[0][0];
        assert!(p.closed);
        assert_eq!(p.pts.len(), 4);
        assert!((signed_area(&p.pts).abs() - 400.0).abs() < 1e-6);
        // A number with no letter repeats the line command after M.
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M0,0 10,0 10,10"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects[0][0].pts.len(), 3);
        assert!(!d.objects[0][0].closed);
    }

    #[test]
    fn curves_pass_through_their_end_points_and_bulge_the_right_way() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M0 50 C0 0 100 0 100 50"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        let (lo, hi) = box_of(&d.objects[0]);
        near(lo[0], 0.0);
        near(hi[0], 100.0);
        near(lo[1], 50.0);
        // Up on the page is up on the job: the middle of this curve is 37.5 above its ends.
        assert!((hi[1] - 87.5).abs() < 0.2, "{}", hi[1]);
        assert!(d.objects[0][0].pts.len() > 8, "smooth, not one straight line");
    }

    #[test]
    fn arc_flags_may_touch_their_neighbours() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M10 50a40 40 0 1150 0"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        let pts = &d.objects[0][0].pts;
        // "1150 0" is two flags, then a move of 50 across.
        let last = pts[pts.len() - 1];
        near(last[0], 60.0);
        near(last[1], 50.0);
        assert!(pts.len() > 8);
        // The long way round: the centre is sqrt(40^2 - 25^2) above the line, the top 40 more.
        let high = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let expected = 50.0 + (40.0_f64 * 40.0 - 25.0 * 25.0).sqrt() + 40.0;
        assert!((high - expected).abs() < 0.2, "{high} vs {expected}");
    }

    #[test]
    fn half_circle_arc_has_the_right_radius() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M10 50 A40 40 0 0 1 90 50"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        let (lo, hi) = box_of(&d.objects[0]);
        near(lo[0], 10.0);
        near(hi[0], 90.0);
        // Sweeping clockwise on the page goes over the top, so it rises 40 on the job.
        near(hi[1], 90.0);
        near(lo[1], 50.0);
    }

    #[test]
    fn group_transforms_add_up() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100">
            <g transform="translate(10,0)"><g transform="scale(2)"><rect x="0" y="0" width="10" height="10" transform="translate(5,0)"/></g></g></svg>"#;
        let (lo, hi) = box_of(&read(s, 96.0).unwrap().objects[0]);
        near(lo[0], 20.0);
        near(hi[0], 40.0);
        near(hi[1], 100.0);
    }

    #[test]
    fn rotation_goes_around_the_point_it_names() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><rect x="40" y="40" width="20" height="10" transform="rotate(90 50 50)"/></svg>"#;
        let (lo, hi) = box_of(&read(s, 96.0).unwrap().objects[0]);
        near(hi[0] - lo[0], 10.0);
        near(hi[1] - lo[1], 20.0);
    }

    #[test]
    fn letters_with_holes_stay_one_object() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100"><path d="M0 0H50V50H0Z M10 10H40V40H10Z"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects.len(), 1);
        assert_eq!(d.objects[0].len(), 2);
    }

    #[test]
    fn lines_polygons_and_rounded_rectangles_are_read() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100">
            <line x1="0" y1="0" x2="10" y2="0"/>
            <polyline points="0,0 10,0 10,10"/>
            <polygon points="0,0 10,0 10,10"/>
            <rect width="40" height="20" rx="5"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects.len(), 4);
        assert!(!d.objects[0][0].closed && !d.objects[1][0].closed);
        assert!(d.objects[2][0].closed);
        let area = signed_area(&d.objects[3][0].pts).abs();
        let expected = 40.0 * 20.0 - (4.0 - std::f64::consts::PI) * 25.0;
        assert!((area - expected).abs() < 1.5, "{area} vs {expected}");
    }

    #[test]
    fn hidden_things_and_definitions_are_left_out_and_text_is_counted() {
        let s = r#"<svg width="100" height="100" viewBox="0 0 100 100">
            <defs><rect width="9" height="9"/></defs>
            <rect width="9" height="9" display="none"/>
            <g style="display: none"><rect width="9" height="9"/></g>
            <text x="1" y="1">hello</text>
            <circle r="5"/></svg>"#;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects.len(), 1);
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn what_a_mask_hides_is_counted_because_it_comes_in_whole() {
        let s = r##"<svg width="100" height="100" viewBox="0 0 100 100">
            <g clip-path="url(#m)"><rect width="500" height="500"/></g>
            <circle r="5" style="clip-path:url(#m)"/>
            <circle r="5"/><flowRoot>words</flowRoot></svg>"##;
        let d = read(s, 96.0).unwrap();
        assert_eq!(d.objects.len(), 3);
        assert_eq!(d.masked, 2);
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn a_page_with_a_doctype_is_accepted() {
        let s = "<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x.dtd\"><svg width=\"10\" height=\"10\" viewBox=\"0 0 10 10\"><circle r=\"5\"/></svg>";
        assert_eq!(read(s, 1.0).unwrap().objects.len(), 1);
    }

    #[test]
    fn bad_files_are_explained_and_a_broken_path_keeps_what_it_read() {
        assert!(read("not xml at all", 1.0).unwrap_err().contains("can't be read"));
        assert!(read("<html></html>", 1.0).unwrap_err().contains("not an SVG"));
        let s = r#"<svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0 L5 5 L oops"/></svg>"#;
        assert_eq!(read(s, 96.0).unwrap().objects[0][0].pts.len(), 2);
    }
}
