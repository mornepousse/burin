//! Reading drawings from PDF, Illustrator (.ai) and EPS files: the lines and curves a page paints,
//! as shapes at their real size. A PDF's page (and an .ai file of Illustrator 9 or later, which is a
//! PDF) is found through its objects, packed ones included; EPS and older .ai files are PostScript,
//! read from their drawing commands. Text, pictures and clipping are left out. Burin's own reader:
//! the original imports these formats too (its File > Import), how it reads them wasn't looked at.

use crate::geom::{Path, P};
use std::collections::HashMap;

/// One PostScript point is this much of an inch.
const POINT: f64 = 1.0 / 72.0;
/// Objects read in at most, and how deep forms within forms are followed.
const MOST_OBJECTS: usize = 200_000;
const DEEPEST_FORM: usize = 6;

/// What a file gave: each painted path as a shape (its outlines), in the job's units, and what was
/// left out.
#[derive(Debug, Default)]
pub struct Drawing {
    pub objects: Vec<Vec<Path>>,
    pub left_out: Vec<String>,
}

/// Reads a PDF, Illustrator or EPS file's drawing. `per_inch` is the job's units in an inch (25.4
/// for millimetres).
pub fn read(bytes: &[u8], per_inch: f64) -> Result<Drawing, String> {
    let scale = per_inch * POINT;
    if bytes.starts_with(b"%PDF") || find(bytes, b"%PDF-", 0).is_some_and(|i| i < 1024) {
        return read_pdf(bytes, scale);
    }
    // A DOS EPS (Windows) holds the PostScript at an offset given in its header.
    let ps = if bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) && bytes.len() > 12 {
        let at = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize;
        let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])) as usize;
        bytes.get(at..at.saturating_add(len)).ok_or_else(|| crate::i18n::tr("This EPS file is cut short.").to_string())?
    } else {
        bytes
    };
    if ps.starts_with(b"%!") {
        let mut d = Drawing::default();
        let mut painter = Painter::new([scale, 0.0, 0.0, scale, 0.0, 0.0]);
        painter.run(ps, true, &mut |_, _| None);
        d.objects = dedupe(painter.objects);
        return Ok(d);
    }
    Err(crate::i18n::tr("This file is neither a PDF nor a PostScript drawing.").into())
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// A PDF object: its dictionary's text (or the whole object's text), and where its stream lies.
#[derive(Clone)]
struct Object {
    text: Vec<u8>,
    stream: Option<(usize, usize)>,
}

fn read_pdf(bytes: &[u8], scale: f64) -> Result<Drawing, String> {
    let mut objects: HashMap<u32, Object> = HashMap::new();
    // Every "n g obj ... endobj" in the file; later copies (updates) replace earlier ones.
    let mut i = 0;
    while let Some(at) = find(bytes, b" obj", i) {
        i = at + 4;
        if objects.len() >= MOST_OBJECTS {
            break;
        }
        let Some((num, _)) = number_pair_before(bytes, at) else { continue };
        let body_start = at + 4;
        let end = find(bytes, b"endobj", body_start).unwrap_or(bytes.len());
        let stream = find(bytes, b"stream", body_start).filter(|s| *s < end && !bytes[..*s].ends_with(b"end")).map(|s| {
            let mut from = s + 6;
            if bytes.get(from) == Some(&b'\r') {
                from += 1;
            }
            if bytes.get(from) == Some(&b'\n') {
                from += 1;
            }
            let to = find(bytes, b"endstream", from).filter(|e| *e <= end).unwrap_or(end);
            (from, to)
        });
        let text_end = stream.map_or(end, |(from, _)| from);
        objects.insert(num, Object { text: bytes[body_start..text_end].to_vec(), stream });
        i = end;
    }
    // Objects packed in object streams.
    let packs: Vec<(u32, Object)> = objects.iter().filter(|(_, o)| has_name(&o.text, b"/Type", b"/ObjStm")).map(|(k, o)| (*k, o.clone())).collect();
    for (_, o) in packs {
        let Some(data) = stream_data(bytes, &o) else { continue };
        let count = dict_number(&o.text, b"/N").unwrap_or(0.0) as usize;
        let first = dict_number(&o.text, b"/First").unwrap_or(0.0) as usize;
        let head = String::from_utf8_lossy(data.get(..first.min(data.len())).unwrap_or(&[])).to_string();
        let nums: Vec<usize> = head.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        for k in 0..count.min(nums.len() / 2) {
            let (num, off) = (nums[2 * k] as u32, nums[2 * k + 1]);
            let next = if 2 * k + 3 < nums.len() { nums[2 * k + 3] } else { data.len() - first };
            if let Some(text) = data.get(first + off..(first + next).min(data.len())) {
                objects.entry(num).or_insert(Object { text: text.to_vec(), stream: None });
            }
        }
    }
    let pages: Vec<u32> = {
        let mut p: Vec<u32> = objects.iter().filter(|(_, o)| has_name(&o.text, b"/Type", b"/Page") && !has_name(&o.text, b"/Type", b"/Pages")).map(|(k, _)| *k).collect();
        p.sort_unstable();
        p
    };
    let Some(&page) = pages.first() else {
        return Err(crate::i18n::tr("No page was found in this PDF file.").into());
    };
    let mut d = Drawing::default();
    if pages.len() > 1 {
        d.left_out.push(crate::i18n::tr("Only the first page was read.").into());
    }
    // The page's box: its own, or its parents'.
    let mut media = [0.0, 0.0, 612.0, 792.0];
    let mut at = Some(page);
    for _ in 0..16 {
        let Some(o) = at.and_then(|k| objects.get(&k)) else { break };
        if let Some(b) = dict_array(&o.text, b"/MediaBox") {
            if b.len() == 4 {
                media = [b[0], b[1], b[2], b[3]];
                break;
            }
        }
        at = dict_ref(&o.text, b"/Parent");
    }
    let page_obj = objects[&page].clone();
    // The page's contents: one stream or several, in order.
    let mut content = Vec::new();
    for r in dict_refs(&page_obj.text, b"/Contents") {
        if let Some(o) = objects.get(&r) {
            // An array of streams held by reference.
            if o.stream.is_none() {
                for inner in refs_in(&o.text) {
                    if let Some(data) = objects.get(&inner).and_then(|o| stream_data(bytes, o)) {
                        content.extend_from_slice(&data);
                        content.push(b'\n');
                    }
                }
            } else if let Some(data) = stream_data(bytes, o) {
                content.extend_from_slice(&data);
                content.push(b'\n');
            }
        }
    }
    let resources = resolve_dict(&objects, &page_obj.text, b"/Resources");
    let start = [scale, 0.0, 0.0, scale, -media[0] * scale, -media[1] * scale];
    let mut painter = Painter::new(start);
    let objects_ref = &objects;
    let mut forms = |name: &[u8], res: &[u8]| -> Option<(Vec<u8>, [f64; 6], Vec<u8>)> {
        let xobjects = resolve_dict(objects_ref, res, b"/XObject")?;
        let r = dict_ref(&xobjects, name)?;
        let o = objects_ref.get(&r)?;
        if !has_name(&o.text, b"/Subtype", b"/Form") {
            return None;
        }
        let m = dict_array(&o.text, b"/Matrix").filter(|m| m.len() == 6).map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |m| [m[0], m[1], m[2], m[3], m[4], m[5]]);
        let res = resolve_dict(objects_ref, &o.text, b"/Resources").unwrap_or_default();
        Some((stream_data(bytes, o)?, m, res))
    };
    painter.resources = resources.unwrap_or_default();
    painter.run(&content, false, &mut forms);
    d.objects = dedupe(painter.objects);
    Ok(d)
}

/// The object number and generation written just before `at` (" obj").
fn number_pair_before(bytes: &[u8], at: usize) -> Option<(u32, u32)> {
    let from = at.saturating_sub(24);
    // Leniently: the bytes before may be the file's binary marker line.
    let text = String::from_utf8_lossy(&bytes[from..at]).to_string();
    let mut it = text.split_whitespace().rev();
    let generation: u32 = it.next()?.parse().ok()?;
    let num: u32 = it.next()?.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
    Some((num, generation))
}

/// A stream's bytes, its filters undone in order (a single name or an array): Flate, hexadecimal
/// and ASCII85, as drawing exports write them. A stream with any other filter isn't read.
fn stream_data(bytes: &[u8], o: &Object) -> Option<Vec<u8>> {
    let (from, to) = o.stream?;
    let raw = bytes.get(from..to)?;
    let raw = raw.strip_suffix(b"\r\n").or_else(|| raw.strip_suffix(b"\n")).unwrap_or(raw);
    let mut data = raw.to_vec();
    for filter in filters(&o.text) {
        data = match filter.as_str() {
            "FlateDecode" | "Fl" => miniz_oxide::inflate::decompress_to_vec_zlib(&data).ok().or_else(|| miniz_oxide::inflate::decompress_to_vec(&data).ok())?,
            "ASCIIHexDecode" | "AHx" => hex_decoded(&data),
            "ASCII85Decode" | "A85" => a85_decoded(&data)?,
            _ => return None,
        };
    }
    Some(data)
}

/// The names given as a stream's /Filter, in order.
fn filters(text: &[u8]) -> Vec<String> {
    let Some(rest) = after_key(text, b"/Filter") else { return Vec::new() };
    let rest = &rest[rest.iter().take_while(|c| c.is_ascii_whitespace()).count()..];
    let part = if rest.starts_with(b"[") { &rest[1..rest.iter().position(|c| *c == b']').unwrap_or(rest.len())] } else { &rest[..rest.iter().skip(1).position(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>')).map_or(rest.len(), |p| p + 1)] };
    String::from_utf8_lossy(part).split('/').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
}

fn hex_decoded(data: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = data.iter().take_while(|c| **c != b'>').filter_map(|c| (*c as char).to_digit(16).map(|d| d as u8)).collect();
    digits.chunks(2).map(|p| p[0] << 4 | p.get(1).copied().unwrap_or(0)).collect()
}

fn a85_decoded(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut group: Vec<u32> = Vec::with_capacity(5);
    let body = data.strip_prefix(b"<~").unwrap_or(data);
    for &c in body {
        match c {
            b'~' => break,
            b'z' if group.is_empty() => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group.push((c - b'!') as u32);
                if group.len() == 5 {
                    let v = group.iter().fold(0u64, |a, d| a * 85 + *d as u64);
                    out.extend_from_slice(&(v as u32).to_be_bytes());
                    group.clear();
                }
            }
            _ => {}
        }
    }
    if !group.is_empty() {
        let n = group.len();
        while group.len() < 5 {
            group.push(84);
        }
        let v = group.iter().fold(0u64, |a, d| a * 85 + *d as u64);
        out.extend_from_slice(&(v as u32).to_be_bytes()[..n - 1]);
    }
    Some(out)
}

fn has_name(text: &[u8], key: &[u8], value: &[u8]) -> bool {
    let mut i = 0;
    while let Some(at) = find(text, key, i) {
        i = at + key.len();
        let rest = &text[i..];
        let rest = &rest[rest.iter().take_while(|c| c.is_ascii_whitespace()).count()..];
        if rest.starts_with(value) && rest.get(value.len()).is_none_or(|c| !c.is_ascii_alphanumeric()) {
            return true;
        }
    }
    false
}

/// What follows `key` in a dictionary's text, up to the next key or the end.
fn after_key<'a>(text: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut i = 0;
    while let Some(at) = find(text, key, i) {
        let end = at + key.len();
        i = end;
        // The whole key, not the start of a longer one.
        if text.get(end).is_some_and(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        return Some(&text[end..]);
    }
    None
}

fn dict_number(text: &[u8], key: &[u8]) -> Option<f64> {
    let rest = String::from_utf8_lossy(after_key(text, key)?).to_string();
    rest.split(|c: char| c.is_whitespace() || c == '/' || c == '>').find(|t| !t.is_empty())?.parse().ok()
}

fn dict_array(text: &[u8], key: &[u8]) -> Option<Vec<f64>> {
    let rest = after_key(text, key)?;
    let rest = &rest[rest.iter().take_while(|c| c.is_ascii_whitespace()).count()..];
    if !rest.starts_with(b"[") {
        return None;
    }
    let end = rest.iter().position(|c| *c == b']')?;
    let inner = String::from_utf8_lossy(&rest[1..end]).to_string();
    Some(inner.split_whitespace().filter_map(|t| t.parse().ok()).collect())
}

/// The object a key refers to ("n 0 R").
fn dict_ref(text: &[u8], key: &[u8]) -> Option<u32> {
    let rest = after_key(text, key)?;
    let s = String::from_utf8_lossy(&rest[..rest.len().min(40)]).to_string();
    let mut it = s.split_whitespace();
    let num: u32 = it.next()?.parse().ok()?;
    let _generation: u32 = it.next()?.parse().ok()?;
    (it.next()? .starts_with('R')).then_some(num)
}

/// One reference after a key, or every reference in the array after it.
fn dict_refs(text: &[u8], key: &[u8]) -> Vec<u32> {
    let Some(rest) = after_key(text, key) else { return Vec::new() };
    let rest = &rest[rest.iter().take_while(|c| c.is_ascii_whitespace()).count()..];
    if rest.starts_with(b"[") {
        let end = rest.iter().position(|c| *c == b']').unwrap_or(rest.len());
        refs_in(&rest[..end])
    } else {
        dict_ref(text, key).into_iter().collect()
    }
}

/// Every "n g R" in some text.
fn refs_in(text: &[u8]) -> Vec<u32> {
    let s = String::from_utf8_lossy(text).replace(['[', ']'], " ");
    let t: Vec<&str> = s.split_whitespace().collect();
    (0..t.len().saturating_sub(2)).filter(|&k| t[k + 2] == "R").filter_map(|k| t[k].parse().ok()).collect()
}

/// A dictionary given after `key`, written in place (<< >>) or held by reference.
fn resolve_dict(objects: &HashMap<u32, Object>, text: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let rest = after_key(text, key)?;
    let rest = &rest[rest.iter().take_while(|c| c.is_ascii_whitespace()).count()..];
    if rest.starts_with(b"<<") {
        // Up to the matching >>.
        let mut depth = 0;
        let mut k = 0;
        while k + 1 < rest.len() {
            if &rest[k..k + 2] == b"<<" {
                depth += 1;
                k += 2;
                continue;
            }
            if &rest[k..k + 2] == b">>" {
                depth -= 1;
                k += 2;
                if depth == 0 {
                    return Some(rest[..k].to_vec());
                }
                continue;
            }
            k += 1;
        }
        Some(rest.to_vec())
    } else {
        let r = dict_ref(text, key)?;
        objects.get(&r).map(|o| o.text.clone())
    }
}

/// Removes repeated shapes: a page often fills a path, then strokes the same path again.
fn dedupe(objects: Vec<Vec<Path>>) -> Vec<Vec<Path>> {
    let mut seen = std::collections::HashSet::new();
    objects
        .into_iter()
        .filter(|o| {
            let key: Vec<(i64, i64)> = o.iter().flat_map(|p| p.pts.iter()).map(|q| ((q[0] * 1e4).round() as i64, (q[1] * 1e4).round() as i64)).collect();
            seen.insert(key)
        })
        .collect()
}

/// Follows a page's drawing commands, with the transforms in force.
struct Painter {
    ctm: [f64; 6],
    saved: Vec<[f64; 6]>,
    path: Vec<Path>,
    current: Vec<P>,
    start: P,
    pen: P,
    clipping: bool,
    objects: Vec<Vec<Path>>,
    resources: Vec<u8>,
    depth: usize,
}

/// [a b c d e f] then [g h i j k l]: the first applied first.
fn compose(m: [f64; 6], n: [f64; 6]) -> [f64; 6] {
    [m[0] * n[0] + m[1] * n[2], m[0] * n[1] + m[1] * n[3], m[2] * n[0] + m[3] * n[2], m[2] * n[1] + m[3] * n[3], m[4] * n[0] + m[5] * n[2] + n[4], m[4] * n[1] + m[5] * n[3] + n[5]]
}

impl Painter {
    fn new(ctm: [f64; 6]) -> Painter {
        Painter { ctm, saved: Vec::new(), path: Vec::new(), current: Vec::new(), start: [0.0; 2], pen: [0.0; 2], clipping: false, objects: Vec::new(), resources: Vec::new(), depth: 0 }
    }

    fn place(&self, x: f64, y: f64) -> P {
        let m = self.ctm;
        [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
    }

    fn end_subpath(&mut self, closed: bool) {
        if self.current.len() >= 2 {
            let mut pts = std::mem::take(&mut self.current);
            let ends_at_start = pts.len() > 2 && (pts[0][0] - pts[pts.len() - 1][0]).hypot(pts[0][1] - pts[pts.len() - 1][1]) < 1e-9;
            if ends_at_start {
                pts.pop();
            }
            self.path.push(if closed || ends_at_start { Path::closed(pts) } else { Path::open(pts) });
        }
        self.current.clear();
    }

    fn line_to(&mut self, q: P) {
        if self.current.is_empty() {
            self.current.push(self.place(self.pen[0], self.pen[1]));
        }
        self.current.push(self.place(q[0], q[1]));
        self.pen = q;
    }

    fn curve_to(&mut self, c1: P, c2: P, end: P) {
        if self.current.is_empty() {
            self.current.push(self.place(self.pen[0], self.pen[1]));
        }
        let (a, b, c, d) = (self.place(self.pen[0], self.pen[1]), self.place(c1[0], c1[1]), self.place(c2[0], c2[1]), self.place(end[0], end[1]));
        let n = crate::crv3d::bezier_steps(a, b, c, d).clamp(2, 400);
        for k in 1..=n {
            let t = k as f64 / n as f64;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            self.current.push([w[0] * a[0] + w[1] * b[0] + w[2] * c[0] + w[3] * d[0], w[0] * a[1] + w[1] * b[1] + w[2] * c[1] + w[3] * d[1]]);
        }
        self.pen = end;
    }

    /// The path painted (or only used to clip, then dropped).
    fn paint(&mut self, filled: bool, keep: bool) {
        self.end_subpath(filled);
        let path = std::mem::take(&mut self.path);
        if keep && !self.clipping && !path.is_empty() {
            self.objects.push(path);
        }
        self.clipping = false;
    }

    /// Runs drawing commands. `postscript` skips what lies between braces (procedure
    /// definitions) and knows PostScript's and Illustrator's names for the same commands.
    fn run(&mut self, data: &[u8], postscript: bool, forms: &mut dyn FnMut(&[u8], &[u8]) -> Option<(Vec<u8>, [f64; 6], Vec<u8>)>) {
        let mut stack: Vec<f64> = Vec::new();
        let mut last_name: Vec<u8> = Vec::new();
        let mut braces = 0usize;
        let mut i = 0;
        let n = data.len();
        while i < n {
            let c = data[i];
            match c {
                b'%' => {
                    while i < n && data[i] != b'\n' && data[i] != b'\r' {
                        i += 1;
                    }
                    continue;
                }
                b'(' => {
                    // A string: skipped, nested brackets and escapes too.
                    let mut depth = 0;
                    while i < n {
                        match data[i] {
                            b'\\' => i += 1,
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    i += 1;
                    stack.clear();
                    continue;
                }
                b'<' if data.get(i + 1) == Some(&b'<') => {
                    i += 2;
                    continue;
                }
                b'>' if data.get(i + 1) == Some(&b'>') => {
                    i += 2;
                    continue;
                }
                b'<' => {
                    while i < n && data[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                b'{' => {
                    braces += 1;
                    i += 1;
                    continue;
                }
                b'}' => {
                    braces = braces.saturating_sub(1);
                    i += 1;
                    stack.clear();
                    continue;
                }
                b'[' | b']' => {
                    i += 1;
                    continue;
                }
                b'/' => {
                    let from = i;
                    i += 1;
                    while i < n && !is_delimiter(data[i]) {
                        i += 1;
                    }
                    last_name = data[from..i].to_vec();
                    continue;
                }
                _ if c.is_ascii_whitespace() => {
                    i += 1;
                    continue;
                }
                _ => {}
            }
            let from = i;
            while i < n && !is_delimiter(data[i]) {
                i += 1;
            }
            if i == from {
                i += 1;
                continue;
            }
            let word = &data[from..i];
            if let Some(v) = std::str::from_utf8(word).ok().and_then(|w| w.parse::<f64>().ok()) {
                stack.push(v);
                if stack.len() > 64 {
                    stack.remove(0);
                }
                continue;
            }
            if postscript && braces > 0 {
                stack.clear();
                continue;
            }
            // Inline pictures: skipped up to their end.
            if word == b"BI" {
                if let Some(e) = find(data, b"EI", i) {
                    i = e + 2;
                }
                stack.clear();
                continue;
            }
            let take = |k: usize, stack: &Vec<f64>| -> Option<Vec<f64>> { (stack.len() >= k).then(|| stack[stack.len() - k..].to_vec()) };
            match word {
                b"q" | b"gsave" => self.saved.push(self.ctm),
                b"Q" | b"grestore" => {
                    if let Some(m) = self.saved.pop() {
                        self.ctm = m;
                    }
                }
                b"cm" | b"concat" => {
                    if let Some(v) = take(6, &stack) {
                        self.ctm = compose([v[0], v[1], v[2], v[3], v[4], v[5]], self.ctm);
                    }
                }
                b"m" | b"moveto" => {
                    if let Some(v) = take(2, &stack) {
                        self.end_subpath(false);
                        self.pen = [v[0], v[1]];
                        self.start = self.pen;
                    }
                }
                b"l" | b"L" | b"lineto" => {
                    if let Some(v) = take(2, &stack) {
                        self.line_to([v[0], v[1]]);
                    }
                }
                b"c" | b"C" | b"curveto" => {
                    if let Some(v) = take(6, &stack) {
                        self.curve_to([v[0], v[1]], [v[2], v[3]], [v[4], v[5]]);
                    }
                }
                b"v" | b"V" => {
                    if let Some(v) = take(4, &stack) {
                        self.curve_to(self.pen, [v[0], v[1]], [v[2], v[3]]);
                    }
                }
                b"y" | b"Y" => {
                    if let Some(v) = take(4, &stack) {
                        self.curve_to([v[0], v[1]], [v[2], v[3]], [v[2], v[3]]);
                    }
                }
                b"h" | b"closepath" => {
                    self.end_subpath(true);
                    self.pen = self.start;
                }
                b"re" => {
                    if let Some(v) = take(4, &stack) {
                        self.end_subpath(false);
                        let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
                        self.current = vec![self.place(x, y), self.place(x + w, y), self.place(x + w, y + h), self.place(x, y + h)];
                        self.end_subpath(true);
                        self.pen = [x, y];
                        self.start = self.pen;
                    }
                }
                b"W" | b"W*" | b"clip" | b"eoclip" => self.clipping = true,
                b"S" | b"stroke" => self.paint(false, true),
                b"s" => {
                    self.end_subpath(true);
                    self.paint(false, true);
                }
                b"f" | b"F" | b"f*" | b"fill" | b"eofill" | b"B" | b"B*" | b"b" | b"b*" => self.paint(true, true),
                b"n" | b"N" | b"newpath" => self.paint(false, false),
                b"Do" if self.depth < DEEPEST_FORM => {
                    let res = self.resources.clone();
                    if let Some((content, m, inner)) = forms(&last_name, &res) {
                        let (saved_ctm, saved_res) = (self.ctm, std::mem::replace(&mut self.resources, inner));
                        self.ctm = compose(m, self.ctm);
                        self.depth += 1;
                        self.run(&content, false, forms);
                        self.depth -= 1;
                        self.ctm = saved_ctm;
                        self.resources = if self.resources.is_empty() { saved_res } else { saved_res };
                    }
                }
                _ => {}
            }
            stack.clear();
        }
    }
}

fn is_delimiter(c: u8) -> bool {
    c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF as a drawing program writes one, its page content compressed: a rectangle stroked and
    /// a circle-ish curve filled, under a transform that turns points into a page of 72 per inch.
    fn small_pdf(content: &str, compressed: bool) -> Vec<u8> {
        let data = if compressed { miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6) } else { content.as_bytes().to_vec() };
        let filter = if compressed { "/Filter /FlateDecode " } else { "" };
        let mut pdf = b"%PDF-1.5\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 720 360] >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n".to_vec();
        pdf.extend_from_slice(format!("4 0 obj\n<< {filter}/Length {} >>\nstream\n", data.len()).as_bytes());
        pdf.extend_from_slice(&data);
        pdf.extend_from_slice(b"\nendstream\nendobj\ntrailer << /Root 1 0 R >>\n%%EOF\n");
        pdf
    }

    #[test]
    fn a_pdf_pages_lines_come_in_at_their_size() {
        // A 1 × 0.5 inch rectangle at (1, 1) inch, stroked; and the same filled then stroked (one shape).
        for compressed in [false, true] {
            let pdf = small_pdf("q 1 0 0 1 72 72 cm 0 0 72 36 re S Q\n0.5 w 300 100 m 372 100 l 372 172 l h f 300 100 m 372 100 l 372 172 l h S", compressed);
            let d = read(&pdf, 1.0).unwrap();
            assert_eq!(d.objects.len(), 2, "two shapes, the second painted twice: {:?}", d.objects);
            let r = &d.objects[0][0];
            assert!(r.closed && r.pts.len() == 4);
            let (lo, hi) = crate::geom::bbox(std::slice::from_ref(r)).unwrap();
            assert!((lo[0] - 1.0).abs() < 1e-9 && (lo[1] - 1.0).abs() < 1e-9 && (hi[0] - 2.0).abs() < 1e-9 && (hi[1] - 1.5).abs() < 1e-9, "{lo:?} {hi:?}");
            assert!(d.objects[1][0].closed, "a filled triangle");
        }
    }

    /// Streams written in hexadecimal then compressed, as a CAD program's PDF export writes them,
    /// and ASCII85 text, are read too.
    #[test]
    fn filter_chains_are_undone_in_order() {
        let content = b"0 0 m 72 0 l 72 72 l h S";
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(content, 6);
        let hex: String = packed.iter().map(|b| format!("{b:02X}")).collect::<String>() + ">";
        let o = Object { text: b"<< /Length 9 /Filter [/ASCIIHexDecode /FlateDecode ] >>".to_vec(), stream: Some((0, hex.len())) };
        assert_eq!(stream_data(hex.as_bytes(), &o).unwrap(), content);
        assert_eq!(a85_decoded(b"<~87cURD]i,\"Ebo80~>").unwrap(), b"Hello World!");
        assert_eq!(hex_decoded(b"48 65 6C6C6F>"), b"Hello");
    }

    #[test]
    fn curves_are_followed_and_clipping_paths_left_out() {
        let pdf = small_pdf("0 0 720 360 re W n 0 0 m 0 72 72 72 72 0 c S", true);
        let d = read(&pdf, 25.4).unwrap();
        assert_eq!(d.objects.len(), 1, "the clip isn't a shape");
        let p = &d.objects[0][0];
        assert!(!p.closed && p.pts.len() > 8);
        let top = p.pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max);
        // A cubic from (0,0) up to 72 and back peaks at 3/4 of its control height: 54 pt = 19.05 mm.
        assert!((top - 19.05).abs() < 0.01, "{top}");
    }

    #[test]
    fn an_eps_file_is_read_from_its_commands() {
        // Burin's own EPS export, read back.
        let paths = vec![crate::geom::Path::closed(crate::geom::rect([1.0, 1.0], 1.0, 1.0, 0.0))];
        let eps = crate::menus::eps_text(&paths, 10.0, 5.0, 72.0);
        let d = read(eps.as_bytes(), 1.0).unwrap();
        assert_eq!(d.objects.len(), 1, "{eps}");
        let (lo, hi) = crate::geom::bbox(&d.objects[0]).unwrap();
        assert!((lo[0] - 0.5).abs() < 1e-6 && (hi[1] - 1.5).abs() < 1e-6, "{lo:?} {hi:?}");
        // Illustrator's names, and a procedure definition that isn't drawn.
        let ai = b"%!PS-Adobe-3.0\n/m {moveto} def\n%%EndProlog\n0 0 m\n72 0 L\n72 72 L\n0 72 L\nf\n";
        let d = read(ai, 1.0).unwrap();
        assert_eq!(d.objects.len(), 1);
        assert!(d.objects[0][0].closed && d.objects[0][0].pts.len() == 4);
    }
}

#[cfg(test)]
mod try_files {
    /// For trying real files: BURIN_PRINTED=<file> cargo test try_printed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn try_printed() {
        let Some(f) = std::env::var_os("BURIN_PRINTED") else { return };
        let t = std::time::Instant::now();
        match super::read(&std::fs::read(&f).unwrap(), 25.4) {
            Ok(d) => {
                let all: Vec<crate::geom::Path> = d.objects.iter().flatten().cloned().collect();
                eprintln!("shapes {} outlines {} closed {} box {:?} left out {:?} in {:?}", d.objects.len(), all.len(), all.iter().filter(|p| p.closed).count(), crate::geom::bbox(&all), d.left_out, t.elapsed());
            }
            Err(e) => eprintln!("error {e}"),
        }
    }
}
