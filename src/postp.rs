//! Post processor files (.pp): text files that describe how a machine wants its file written.
//!
//! A file has settings (`UNITS = "MM"`), number formats (`VAR X_POSITION = [X|C| X|1.3]`),
//! and blocks of quoted lines (`begin RAPID_MOVE`) with fields in square brackets.

use crate::arcs::Step;
use crate::toolpath::{Move, PostPath, Tool};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// How one number is written.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberFormat {
    /// Written every time, rather than only when it changed.
    pub always: bool,
    pub prefix: String,
    pub digits: usize,
    pub decimals: usize,
    pub plus: bool,
    pub scale: f64,
}

impl NumberFormat {
    fn write(&self, value: f64) -> String {
        let v = value * self.scale;
        // Avoids "-0.000".
        let v = if v.abs() < 0.5 * 10f64.powi(-(self.decimals as i32)) { 0.0 } else { v };
        let mut number = if self.digits > 0 {
            format!("{v:0width$.decimals$}", width = self.digits + usize::from(self.decimals > 0) + self.decimals, decimals = self.decimals)
        } else {
            format!("{v:.decimals$}", decimals = self.decimals)
        };
        if self.plus && v >= 0.0 {
            number.insert(0, '+');
        }
        format!("{}{}", self.prefix, number)
    }
}

#[derive(Clone, Debug)]
pub struct PostDef {
    pub name: String,
    pub extension: String,
    /// `None` when the file doesn't say: the job's own unit is used.
    pub metric: Option<bool>,
    pub line_ending: String,
    pub line_start: f64,
    pub line_step: f64,
    pub line_max: f64,
    pub formats: HashMap<String, NumberFormat>,
    pub blocks: HashMap<String, Vec<String>>,
    /// Things in the file this program doesn't act on, to show the user.
    pub notes: Vec<String>,
    pub source: Option<PathBuf>,
}

/// Bytes to text; older files aren't always UTF-8.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    }
}

fn parse_format(body: &str) -> Option<(String, NumberFormat)> {
    let inner = body.trim().strip_prefix('[')?.strip_suffix(']')?;
    let parts: Vec<&str> = inner.split('|').collect();
    let token = parts.first()?.trim().to_uppercase();
    if token.is_empty() {
        return None;
    }
    let always = parts.get(1).is_none_or(|m| m.trim().eq_ignore_ascii_case("A"));
    let prefix = parts.get(2).copied().unwrap_or("").to_string();
    let spec = parts.get(3).copied().unwrap_or("1.3").trim();
    let plus = spec.starts_with('+');
    let spec = spec.trim_start_matches('+');
    let (whole, fraction) = spec.split_once('.').unwrap_or((spec, "0"));
    let zero_padded = whole.len() > 1 && whole.starts_with('0');
    let digits = if zero_padded { whole.parse().unwrap_or(0) } else { 0 };
    let decimals = fraction.parse().unwrap_or(3);
    let scale = parts.get(4).and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| *s != 0.0).unwrap_or(1.0);
    Some((token, NumberFormat { always, prefix, digits, decimals, plus, scale }))
}

fn parse_line_ending(text: &str) -> String {
    let mut out = String::new();
    for piece in text.split('[').skip(1) {
        if let Some(code) = piece.split(']').next().and_then(|c| c.trim().parse::<u8>().ok()) {
            out.push(code as char);
        }
    }
    if out.is_empty() {
        "\n".to_string()
    } else {
        out
    }
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(t).to_string()
}

const SETTINGS_WITHOUT_EFFECT: [&str; 7] = [
    "SUBSTITUTE",
    "DIRECT_OUTPUT",
    "RAPID_PLUNGE_TO_STARTZ",
    "ROTARY_WRAP_X",
    "ROTARY_WRAP_Y",
    "LASER_MODE",
    "SPINDLE_CONTROL",
];

impl PostDef {
    /// Whether the file says how to write arcs both ways round, and how to write their centres:
    /// an arc without a proper centre would send the machine somewhere else entirely.
    pub fn can_write_arcs(&self) -> bool {
        let names = ["FIRST_CW_ARC_MOVE", "CW_ARC_MOVE", "FIRST_CCW_ARC_MOVE", "CCW_ARC_MOVE"];
        let centres_known = names
            .iter()
            .filter_map(|n| self.blocks.get(*n))
            .flatten()
            .all(|line| tokens_in(line).iter().all(|t| !matches!(t.as_str(), "I" | "J" | "IA" | "JA" | "R") || self.formats.contains_key(t)));
        centres_known && names[..2].iter().any(|n| self.blocks.contains_key(*n)) && names[2..].iter().any(|n| self.blocks.contains_key(*n))
    }

    /// Reads a post processor. `base_loader` fetches a file named by `POST_BASE`.
    pub fn parse(text: &str, base_loader: &dyn Fn(&str) -> Option<String>) -> Result<PostDef, String> {
        Self::parse_inner(text, base_loader, 0)
    }

    fn parse_inner(text: &str, base_loader: &dyn Fn(&str) -> Option<String>, depth: usize) -> Result<PostDef, String> {
        let mut def = PostDef {
            name: String::new(),
            extension: "nc".into(),
            metric: None,
            line_ending: "\n".into(),
            line_start: 0.0,
            line_step: 1.0,
            line_max: 999_999.0,
            formats: HashMap::new(),
            blocks: HashMap::new(),
            notes: Vec::new(),
            source: None,
        };
        // A file can build on another one: start from that, then let this file override it.
        let base_name = text.lines().find_map(|l| {
            let (key, value) = l.split_once('=')?;
            key.trim().eq_ignore_ascii_case("POST_BASE").then(|| unquote(value))
        });
        if let Some(name) = base_name {
            if depth >= 5 {
                return Err("It builds on too many other files in a row.".into());
            }
            let base_text = base_loader(&name).ok_or_else(|| format!("It builds on \"{name}\", which isn't next to it."))?;
            def = Self::parse_inner(&base_text, base_loader, depth + 1)?;
            def.name = String::new();
        }
        let mut block: Option<String> = None;
        let mut own: Vec<(String, Vec<String>)> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('+') {
                continue;
            }
            let lower = line.to_lowercase();
            if lower == "begin" || lower.starts_with("begin ") || lower.starts_with("begin\t") {
                let name = line[5..].trim().to_uppercase();
                own.push((name.clone(), Vec::new()));
                block = Some(name);
                continue;
            }
            if block.is_some() && line.starts_with('"') {
                if let Some((_, lines)) = own.last_mut() {
                    lines.push(unquote(line));
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("VAR ").or_else(|| line.strip_prefix("var ")) {
                block = None;
                let Some((_, body)) = rest.split_once('=') else { continue };
                // The prefix may start with a space, so only the outer edge is trimmed.
                if let Some((token, format)) = parse_format(body) {
                    def.formats.insert(token, format);
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            block = None;
            let (key, value) = (key.trim().to_uppercase(), unquote(value));
            match key.as_str() {
                "POST_NAME" => def.name = value,
                "FILE_EXTENSION" => def.extension = value.trim_start_matches('.').to_string(),
                "UNITS" => {
                    let v = value.to_uppercase();
                    def.metric = if v.contains("MM") || v.contains("METRIC") {
                        Some(true)
                    } else if v.contains("INCH") {
                        Some(false)
                    } else {
                        None
                    };
                }
                "LINE_ENDING" => def.line_ending = parse_line_ending(&value),
                "LINE_NUMBER_START" => def.line_start = value.trim().parse().unwrap_or(0.0),
                "LINE_NUMBER_INCREMENT" => def.line_step = value.trim().parse().unwrap_or(1.0),
                "LINE_NUMBER_MAXIMUM" => def.line_max = value.trim().parse().unwrap_or(999_999.0),
                "POST_BASE" => {}
                k if SETTINGS_WITHOUT_EFFECT.contains(&k) => def.notes.push(format!("{k} is not applied.")),
                _ => {}
            }
        }
        for (name, lines) in own {
            def.blocks.insert(name, lines);
        }
        if depth == 0 {
            if def.name.trim().is_empty() {
                return Err("It has no POST_NAME, so it isn't a post processor.".into());
            }
            let has_rapid = ["INITIAL_RAPID_MOVE", "FIRST_RAPID_MOVE", "RAPID_MOVE"].iter().any(|b| def.blocks.contains_key(*b));
            if !has_rapid || !(def.blocks.contains_key("FEED_MOVE") || def.blocks.contains_key("FIRST_FEED_MOVE")) {
                return Err("It has no blocks for fast moves and cutting moves.".into());
            }
            def.note_unknown_fields();
            def.notes.sort();
            def.notes.dedup();
        }
        Ok(def)
    }

    fn note_unknown_fields(&mut self) {
        let mut unknown = BTreeSet::new();
        for (name, lines) in &self.blocks {
            if name.contains("ARC") || name.contains("HELICAL") || name.contains("SPIRAL") {
                continue;
            }
            for line in lines {
                for token in tokens_in(line) {
                    if !self.formats.contains_key(&token) && !is_known_field(&token) {
                        unknown.insert(token);
                    }
                }
            }
        }
        for t in unknown {
            self.notes.push(format!("[{t}] is left blank."));
        }
    }

    /// Reads a post processor file, and any file it builds on from the same folder.
    pub fn load(path: &Path) -> Result<PostDef, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read it: {e}"))?;
        let dir = path.parent().map(Path::to_path_buf);
        let loader = move |name: &str| -> Option<String> {
            let dir = dir.as_ref()?;
            let file = Path::new(name).file_name()?;
            std::fs::read(dir.join(file)).ok().map(|b| decode(&b))
        };
        let mut def = PostDef::parse(&decode(&bytes), &loader)?;
        def.source = Some(path.to_path_buf());
        Ok(def)
    }
}

fn tokens_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else { break };
        out.push(after[..end].trim().to_uppercase());
        rest = &after[end + 1..];
    }
    out
}

fn is_known_field(token: &str) -> bool {
    matches!(
        token,
        "I" | "J" | "IA" | "JA" | "R" | "X" | "Y" | "Z" | "XH" | "YH" | "ZH" | "F" | "FC" | "FP" | "S" | "N" | "T" | "SAFEZ" | "DWELL" | "XMIN" | "XMAX" | "YMIN" | "YMAX" | "ZMIN" | "ZMAX" | "XLENGTH" | "YLENGTH" | "ZLENGTH"
            | "TP_FILENAME" | "TP_DIR" | "TP_EXT" | "PATHNAME" | "TOOLPATH_NAME" | "TOOLPATHS_OUTPUT" | "TOOLNAME" | "TOOL_NAME" | "TOOL_DIAMETER" | "DATE" | "TIME" | "PRODUCT" | "FILE_NOTES" | "TOOLPATH_NOTES" | "UNITS"
    )
}

/// Facts about the job a post processor file can ask for.
pub struct PostJob<'a> {
    /// Where the machine's origin sits, in job coordinates.
    pub origin: [f64; 2],
    /// Added to Z: the thickness when zero is on the machine bed.
    pub z_shift: f64,
    /// Multiplies every length, when the file's unit differs from the job's.
    pub scale: f64,
    pub size: [f64; 3],
    pub home: [f64; 3],
    pub safe_z: f64,
    pub file_name: &'a str,
    pub date: String,
    pub time: String,
}

/// What the last line written was, as far as the choice of the next block goes.
#[derive(Clone, Copy, PartialEq)]
enum Wrote {
    Header,
    ToolChange,
    Segment,
    Rapid,
    Feed,
    Plunge,
    Retract,
    Clockwise,
    CounterClockwise,
}

struct Writer<'a> {
    wrote: Wrote,
    written_rate: Option<f64>,
    def: &'a PostDef,
    job: &'a PostJob<'a>,
    out: String,
    /// What each changing number was last written as.
    last: HashMap<String, String>,
    line: f64,
    at: [f64; 3],
    rate: f64,
    tool: Option<&'a Tool>,
    path_name: &'a str,
    dwell: f64,
    /// While an arc is written: its centre, and how far that is from where it starts.
    arc: Option<ArcFields>,
}

#[derive(Clone, Copy)]
struct ArcFields {
    centre: [f64; 2],
    from_start: [f64; 2],
    radius: f64,
}

impl Writer<'_> {
    fn number(&self, token: &str) -> Option<f64> {
        let j = self.job;
        let k = j.scale;
        let z = |v: f64| (v + j.z_shift) * k;
        Some(match token {
            "X" => (self.at[0] - j.origin[0]) * k,
            "Y" => (self.at[1] - j.origin[1]) * k,
            "Z" => z(self.at[2]),
            "XH" => (j.home[0] - j.origin[0]) * k,
            "YH" => (j.home[1] - j.origin[1]) * k,
            "ZH" => z(j.home[2]),
            "SAFEZ" => z(j.safe_z),
            "F" => self.rate * k,
            "FC" => self.tool?.feed * k,
            "FP" => self.tool?.plunge * k,
            "S" => self.tool?.rpm,
            "T" => self.tool?.number as f64,
            "N" => self.line,
            "DWELL" => self.dwell,
            "I" => self.arc?.from_start[0] * k,
            "J" => self.arc?.from_start[1] * k,
            "IA" => (self.arc?.centre[0] - j.origin[0]) * k,
            "JA" => (self.arc?.centre[1] - j.origin[1]) * k,
            "R" => self.arc?.radius * k,
            "XMIN" => (0.0 - j.origin[0]) * k,
            "YMIN" => (0.0 - j.origin[1]) * k,
            "XMAX" => (j.size[0] - j.origin[0]) * k,
            "YMAX" => (j.size[1] - j.origin[1]) * k,
            "ZMIN" => z(-j.size[2]),
            "ZMAX" => z(0.0),
            "XLENGTH" => j.size[0] * k,
            "YLENGTH" => j.size[1] * k,
            "ZLENGTH" => j.size[2] * k,
            _ => return None,
        })
    }

    fn field(&mut self, token: &str) -> String {
        if let Some(value) = self.number(token) {
            let text = match self.def.formats.get(token) {
                Some(format) => {
                    let written = format.write(value);
                    // A missing centre would be read as zero by the machine: always write it.
                    let centre = matches!(token, "I" | "J" | "IA" | "JA" | "R");
                    if !format.always && !centre {
                        if self.last.get(token) == Some(&written) {
                            return String::new();
                        }
                        self.last.insert(token.to_string(), written.clone());
                    }
                    written
                }
                None if token == "T" || token == "S" || token == "N" => format!("{value:.0}"),
                None => format!("{value:.3}"),
            };
            if token == "N" {
                self.line += self.def.line_step;
                if self.line > self.def.line_max {
                    self.line = self.def.line_start;
                }
            }
            return text;
        }
        let tool = self.tool;
        match token {
            "TP_FILENAME" | "PATHNAME" => self.job.file_name.to_string(),
            "TP_DIR" => String::new(),
            "TP_EXT" => self.def.extension.clone(),
            "TOOLPATH_NAME" | "TOOLPATHS_OUTPUT" => self.path_name.to_string(),
            "TOOLNAME" | "TOOL_NAME" => tool.map_or(String::new(), |t| t.name.clone()),
            "TOOL_DIAMETER" => tool.map_or(String::new(), |t| format!("{:.3}", t.dia * self.job.scale)),
            "DATE" => self.job.date.clone(),
            "TIME" => self.job.time.clone(),
            "PRODUCT" => "Burin".to_string(),
            "UNITS" => if self.def.metric == Some(false) { "inch".to_string() } else { "mm".to_string() },
            _ => String::new(),
        }
    }

    fn write_block(&mut self, names: &[&str]) {
        let Some(lines) = names.iter().find_map(|n| self.def.blocks.get(*n)) else { return };
        for line in lines.clone() {
            let mut text = String::new();
            let mut rest = line.as_str();
            while let Some(start) = rest.find('[') {
                text.push_str(&rest[..start]);
                let after = &rest[start + 1..];
                match after.find(']') {
                    Some(end) => {
                        text.push_str(&self.field(&after[..end].trim().to_uppercase()));
                        rest = &after[end + 1..];
                    }
                    None => {
                        text.push('[');
                        rest = after;
                    }
                }
            }
            text.push_str(rest);
            self.out.push_str(&text);
            self.out.push_str(&self.def.line_ending);
        }
    }
}

/// Writes the machine file the way the post processor describes.
pub fn render(def: &PostDef, paths: &[PostPath], job: &PostJob, arcs: Option<f64>) -> String {
    let mut w = Writer {
        wrote: Wrote::Header,
        written_rate: None,
        def,
        job,
        out: String::new(),
        last: HashMap::new(),
        line: def.line_start,
        at: job.home,
        rate: 0.0,
        tool: None,
        path_name: "",
        dwell: 0.0,
        arc: None,
    };
    let arcs = arcs.filter(|_| def.can_write_arcs());
    for (i, path) in paths.iter().enumerate() {
        let same_tool = w.tool == Some(path.tool);
        w.tool = Some(path.tool);
        w.path_name = path.name;
        w.rate = path.tool.feed;
        w.dwell = path.dwell;
        if i == 0 {
            w.write_block(&["HEADER"]);
            w.wrote = Wrote::Header;
        } else {
            // Each toolpath repeats everything rather than assume the machine remembers.
            w.last.clear();
            w.wrote = Wrote::Segment;
            if !same_tool {
                w.write_block(&["TOOLCHANGE"]);
                w.wrote = Wrote::ToolChange;
            }
            w.write_block(&["NEW_SEGMENT"]);
        }
        write_moves(&mut w, path, arcs);
    }
    w.write_block(&["FOOTER"]);
    w.out
}

impl Writer<'_> {
    fn has(&self, block: &str) -> bool {
        self.def.blocks.contains_key(block)
    }

    fn rapid(&mut self, tool: &Tool) {
        let changed = self.wrote != Wrote::Rapid;
        if changed && matches!(self.wrote, Wrote::Header | Wrote::ToolChange) && self.has("INITIAL_RAPID_MOVE") {
            self.write_block(&["INITIAL_RAPID_MOVE"]);
        } else if changed && self.has("FIRST_RAPID_MOVE") {
            self.write_block(&["FIRST_RAPID_MOVE"]);
        } else if self.has("RAPID_MOVE") {
            self.write_block(&["RAPID_MOVE"]);
        } else {
            self.rate = tool.feed;
            self.feed();
            return;
        }
        self.wrote = Wrote::Rapid;
    }

    fn feed(&mut self) {
        if self.wrote != Wrote::Feed {
            self.write_block(&["FIRST_FEED_MOVE", "FEED_MOVE"]);
        } else {
            self.write_block(&["FEED_MOVE", "FIRST_FEED_MOVE"]);
        }
        self.wrote = Wrote::Feed;
    }

    fn plunge(&mut self) {
        if self.wrote != Wrote::Plunge && self.has("FIRST_PLUNGE_MOVE") {
            self.write_block(&["FIRST_PLUNGE_MOVE"]);
        } else if self.has("PLUNGE_MOVE") {
            self.write_block(&["PLUNGE_MOVE"]);
        } else {
            self.feed();
        }
        self.wrote = Wrote::Plunge;
    }

    fn retract(&mut self, tool: &Tool) {
        if self.has("RETRACT_MOVE") {
            self.write_block(&["RETRACT_MOVE"]);
        } else {
            self.rapid(tool);
        }
        self.wrote = Wrote::Retract;
    }

    /// Says so ahead of the move, where the file has a line for a new cutting rate.
    fn note_rate(&mut self) {
        if self.written_rate != Some(self.rate) && self.has("FEED_RATE_CHANGE") {
            self.write_block(&["FEED_RATE_CHANGE"]);
        }
        self.written_rate = Some(self.rate);
    }
}

fn write_moves(w: &mut Writer, path: &PostPath, arcs: Option<f64>) {
    let moves: &[Move] = path.moves;
    let tool = path.tool;
    let steps: Vec<Step> = match (arcs, path.steps) {
        (Some(_), Some(exact)) => exact.to_vec(),
        (Some(tol), None) => crate::arcs::fit_arcs(moves, tol),
        _ => moves.iter().map(|m| Step::Straight(*m)).collect(),
    };
    for (i, step) in steps.iter().enumerate() {
        let before = w.at;
        let m = match step {
            Step::Straight(m) => m,
            Step::Arc { end, centre, counter_clockwise } => {
                let radius = (before[0] - centre[0]).hypot(before[1] - centre[1]);
                w.arc = Some(ArcFields { centre: *centre, from_start: [centre[0] - before[0], centre[1] - before[1]], radius });
                w.at = *end;
                w.rate = tool.feed;
                w.note_rate();
                let kind = if *counter_clockwise { Wrote::CounterClockwise } else { Wrote::Clockwise };
                let names: [&str; 2] = match (*counter_clockwise, w.wrote != kind) {
                    (false, true) => ["FIRST_CW_ARC_MOVE", "CW_ARC_MOVE"],
                    (false, false) => ["CW_ARC_MOVE", "FIRST_CW_ARC_MOVE"],
                    (true, true) => ["FIRST_CCW_ARC_MOVE", "CCW_ARC_MOVE"],
                    (true, false) => ["CCW_ARC_MOVE", "FIRST_CCW_ARC_MOVE"],
                };
                w.write_block(&names);
                w.arc = None;
                w.wrote = kind;
                continue;
            }
        };
        w.at = m.p;
        let flat = (m.p[0] - before[0]).hypot(m.p[1] - before[1]);
        let rise = m.p[2] - before[2];
        if m.rapid {
            if flat < 1e-9 && rise > 0.0 {
                w.retract(tool);
            } else {
                w.rapid(tool);
            }
            continue;
        }
        w.rate = if flat < rise.abs() { tool.plunge } else { tool.feed };
        w.note_rate();
        if flat < 1e-9 && rise < 0.0 {
            w.plunge();
        } else if flat < 1e-9 && rise > 0.0 {
            w.retract(tool);
        } else {
            w.feed();
        }
        if path.dwell > 0.0 && flat < 1e-9 && matches!(steps.get(i + 1), Some(Step::Straight(n)) if n.rapid) {
            w.write_block(&["DWELL_MOVE"]);
        }
    }
}

/// Today's date and time, in universal time.
pub fn now_strings() -> (String, String) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a calendar date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (format!("{year:04}-{month:02}-{day:02}"), format!("{:02}:{:02}", rest / 3600, rest / 60 % 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolpath::default_tools;

    const SAMPLE: &str = r#"
+ A made-up machine, written for this test.
POST_NAME = "Test Router (mm) (*.tap)"
FILE_EXTENSION = "tap"
UNITS = "MM"
LINE_NUMBER_START = 100
LINE_NUMBER_INCREMENT = 10
LINE_NUMBER_MAXIMUM = 999999
LINE_ENDING = "[13][10]"
SUBSTITUTE = "([91])[93]"

VAR LINE_NUMBER = [N|A|N|1.0]
VAR SPINDLE_SPEED = [S|A|S|1.0]
VAR FEED_RATE = [F|C| F|1.1]
VAR CUT_RATE = [FC|C|F|1.0]
VAR PLUNGE_RATE = [FP|C|F|1.0|0.5]
VAR X_POSITION = [X|C| X|1.3]
VAR Y_POSITION = [Y|C| Y|1.3]
VAR Z_POSITION = [Z|C| Z|1.3]
VAR X_HOME_POSITION = [XH|A| X|1.3]
VAR Y_HOME_POSITION = [YH|A| Y|1.3]
VAR Z_HOME_POSITION = [ZH|A| Z|1.3]
VAR SAFE_Z_HEIGHT = [SAFEZ|A|Z|1.3]
VAR DWELL_TIME = [DWELL|A|P|1.2]

begin HEADER
"( [TP_FILENAME].[TP_EXT] by [PRODUCT] )"
"( Tool: [TOOLNAME] D=[TOOL_DIAMETER] )"
"[N] G21 G90"
"[N] T[T] M6 [MYSTERY]"
"[N] [S] M3"

begin RAPID_MOVE
"[N] G0[X][Y][Z]"

begin FIRST_FEED_MOVE
"[N] G1[X][Y][Z][F]"

begin FEED_MOVE
"[N][X][Y][Z]"

begin PLUNGE_MOVE
"[N] G1[X][Y][Z][F]"

begin DWELL_MOVE
"[N] G4 [DWELL]"

begin TOOLCHANGE
"( Next: [TOOLPATH_NAME] )"
"[N] M5"
"[N] T[T] M6"
"[N] [S] M3"

begin NEW_SEGMENT
"( Same tool: [TOOLPATH_NAME] )"

begin FIRST_CW_ARC_MOVE
"[N] G2[X][Y][I][J][F] [ARCS_ARE_NOT_CHECKED]"

begin CW_ARC_MOVE
"[N] G2[X][Y][I][J]"

begin FIRST_CCW_ARC_MOVE
"[N] G3[X][Y][I][J][F]"

begin CCW_ARC_MOVE
"[N] G3[X][Y][I][J]"

begin FOOTER
"[N] M5"
"[N] M30"
"%"
"#;

    fn def() -> PostDef {
        PostDef::parse(SAMPLE, &|_| None).unwrap()
    }

    fn job() -> PostJob<'static> {
        PostJob {
            origin: [0.0, 0.0],
            z_shift: 0.0,
            scale: 1.0,
            size: [100.0, 50.0, 10.0],
            home: [0.0, 0.0, 20.0],
            safe_z: 5.0,
            file_name: "plate",
            date: "2026-11-04".into(),
            time: "12:00".into(),
        }
    }

    fn mm_tool() -> Tool {
        default_tools().into_iter().find(|t| t.metric).unwrap()
    }

    fn moves() -> Vec<Move> {
        let r = |p| Move { p, rapid: true };
        let f = |p| Move { p, rapid: false };
        vec![r([0.0, 0.0, 20.0]), r([10.0, 5.0, 5.0]), f([10.0, 5.0, -2.0]), f([20.0, 5.0, -2.0]), f([20.0, 15.0, -2.0]), r([20.0, 15.0, 5.0])]
    }

    fn lines(text: &str) -> Vec<String> {
        text.split("\r\n").map(str::to_string).collect()
    }

    #[test]
    fn settings_and_formats_are_read() {
        let d = def();
        assert_eq!(d.name, "Test Router (mm) (*.tap)");
        assert_eq!(d.extension, "tap");
        assert_eq!(d.metric, Some(true));
        assert_eq!(d.line_ending, "\r\n");
        let x = &d.formats["X"];
        assert_eq!(x.prefix, " X", "the space in the prefix is kept");
        assert!(!x.always);
        assert_eq!(x.decimals, 3);
        assert_eq!(d.formats["FP"].scale, 0.5);
        assert!(d.formats["S"].always);
    }

    #[test]
    fn what_can_not_be_honoured_is_listed() {
        let d = def();
        assert!(d.notes.iter().any(|n| n.contains("SUBSTITUTE")), "{:?}", d.notes);
        assert!(d.notes.iter().any(|n| n.contains("[MYSTERY]")), "{:?}", d.notes);
        assert!(!d.notes.iter().any(|n| n.contains("ARCS_ARE_NOT_CHECKED")), "arc blocks are never written");
    }

    #[test]
    fn a_file_is_written_the_way_the_blocks_say() {
        let t = mm_tool();
        let m = moves();
        let text = render(&def(), &[PostPath { name: "Profile 1", tool: &t, moves: &m, dwell: 0.0, steps: None }], &job(), None);
        let l = lines(&text);
        assert_eq!(l[0], "( plate.tap by Burin )");
        assert_eq!(l[1], format!("( Tool: {} D=2.000 )", t.name));
        assert_eq!(l[2], "N100 G21 G90");
        assert_eq!(l[3], format!("N110 T{} M6 ", t.number), "an unknown field is left blank");
        assert_eq!(l[4], "N120 S18000 M3");
        // First move to home writes all three axes, then only what changed.
        assert_eq!(l[5], "N130 G0 X0.000 Y0.000 Z20.000");
        assert_eq!(l[6], "N140 G0 X10.000 Y5.000 Z5.000");
        assert_eq!(l[7], format!("N150 G1 Z-2.000 F{:.1}", t.plunge), "plunging at the plunge rate");
        assert_eq!(l[8], format!("N160 G1 X20.000 F{:.1}", t.feed), "the first cut uses its own block and the cut rate");
        assert_eq!(l[9], "N170 Y15.000", "after that only what changed");
        assert_eq!(l[10], "N180 G0 Z5.000");
        assert_eq!(l[11], "N190 M5");
        assert_eq!(l[12], "N200 M30");
        assert_eq!(l[13], "%");
    }

    #[test]
    fn units_origin_and_bed_zero_apply() {
        let t = mm_tool();
        let m = moves();
        let j = PostJob { origin: [10.0, 5.0], z_shift: 10.0, scale: 2.0, ..job() };
        let text = render(&def(), &[PostPath { name: "p", tool: &t, moves: &m, dwell: 0.0, steps: None }], &j, None);
        assert!(text.contains(" X0.000 Y0.000 Z30.000"), "{text}");
        assert!(text.contains(&format!(" Z16.000 F{:.1}", t.plunge * 2.0)), "{text}");
    }

    #[test]
    fn tool_changes_and_segments_use_their_blocks() {
        let tools = default_tools();
        let (a, b) = (tools[0].clone(), tools[1].clone());
        let m = moves();
        let paths = [
            PostPath { name: "One", tool: &a, moves: &m, dwell: 0.0, steps: None },
            PostPath { name: "Two", tool: &a, moves: &m, dwell: 0.0, steps: None },
            PostPath { name: "Three", tool: &b, moves: &m, dwell: 0.0, steps: None },
        ];
        let text = render(&def(), &paths, &job(), None);
        assert!(text.contains("( Same tool: Two )"), "{text}");
        assert!(text.contains("( Next: Three )"), "{text}");
        assert!(!text.contains("( Next: Two )"));
        let after = text.split("( Next: Three )").nth(1).unwrap();
        assert!(after.contains(" X0.000 Y0.000 Z20.000"), "all axes are repeated after a tool change");
    }

    #[test]
    fn dwell_is_written_at_the_bottom_of_a_hole() {
        let t = mm_tool();
        let m = vec![Move { p: [5.0, 5.0, 5.0], rapid: true }, Move { p: [5.0, 5.0, -3.0], rapid: false }, Move { p: [5.0, 5.0, 5.0], rapid: true }];
        let text = render(&def(), &[PostPath { name: "d", tool: &t, moves: &m, dwell: 0.25, steps: None }], &job(), None);
        assert!(text.contains("G4 P0.25"), "{text}");
    }

    #[test]
    fn a_file_without_special_blocks_still_works() {
        let small = "POST_NAME = \"Tiny\"\nVAR X_POSITION = [X|A|X|1.1]\nbegin RAPID_MOVE\n\"G0 [X]\"\nbegin FEED_MOVE\n\"G1 [X]\"\n";
        let d = PostDef::parse(small, &|_| None).unwrap();
        assert_eq!(d.metric, None);
        let t = mm_tool();
        let m = moves();
        let text = render(&d, &[PostPath { name: "p", tool: &t, moves: &m, dwell: 0.0, steps: None }], &job(), None);
        assert!(text.contains("G1 X20.0"), "{text}");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn number_formats_handle_sign_padding_scale_and_negative_zero() {
        let (_, f) = parse_format("[Z|A|Z|+1.2]").unwrap();
        assert_eq!(f.write(1.5), "Z+1.50");
        assert_eq!(f.write(-1.5), "Z-1.50");
        assert_eq!(f.write(-0.0001), "Z+0.00");
        let (_, n) = parse_format("[N|A|N|04.0]").unwrap();
        assert_eq!(n.write(7.0), "N0007");
        let (_, s) = parse_format("[F|C|F|1.1|0.0166]").unwrap();
        assert_eq!(s.write(1000.0), "F16.6");
    }

    #[test]
    fn line_numbers_wrap_at_the_maximum() {
        let text = "POST_NAME = \"W\"\nLINE_NUMBER_START = 1\nLINE_NUMBER_INCREMENT = 1\nLINE_NUMBER_MAXIMUM = 2\nVAR N = [N|A|N|1.0]\nbegin HEADER\n\"[N]\"\n\"[N]\"\n\"[N]\"\nbegin RAPID_MOVE\n\"\"\nbegin FEED_MOVE\n\"\"\n";
        let d = PostDef::parse(text, &|_| None).unwrap();
        let t = mm_tool();
        let out = render(&d, &[PostPath { name: "p", tool: &t, moves: &[], dwell: 0.0, steps: None }], &job(), None);
        assert!(out.starts_with("N1\nN2\nN1\n"), "{out}");
    }

    #[test]
    fn a_base_file_supplies_what_the_child_leaves_out() {
        let base = "POST_NAME = \"Base\"\nFILE_EXTENSION = \"nc\"\nUNITS = \"INCHES\"\nVAR X_POSITION = [X|A|X|1.3]\nbegin RAPID_MOVE\n\"BASE G0 [X]\"\nbegin FEED_MOVE\n\"BASE G1 [X]\"\n";
        let child = "POST_NAME = \"Child\"\nPOST_BASE = \"base.pp\"\nVAR X_POSITION = [X|A|X|1.1]\nbegin FEED_MOVE\n\"CHILD G1 [X]\"\n";
        let d = PostDef::parse(child, &|n| (n == "base.pp").then(|| base.to_string())).unwrap();
        assert_eq!(d.name, "Child");
        assert_eq!(d.metric, Some(false));
        assert_eq!(d.blocks["RAPID_MOVE"], vec!["BASE G0 [X]".to_string()]);
        assert_eq!(d.blocks["FEED_MOVE"], vec!["CHILD G1 [X]".to_string()]);
        assert_eq!(d.formats["X"].decimals, 1);
        let missing = PostDef::parse(child, &|_| None).unwrap_err();
        assert!(missing.contains("base.pp"), "{missing}");
    }

    #[test]
    fn files_that_are_not_post_processors_are_refused_with_a_reason() {
        assert!(PostDef::parse("just some text\nmore text", &|_| None).unwrap_err().contains("POST_NAME"));
        assert!(PostDef::parse("POST_NAME = \"x\"\nbegin HEADER\n\"G21\"\n", &|_| None).unwrap_err().contains("blocks"));
    }

    #[test]
    fn old_style_text_and_windows_line_ends_are_read() {
        let mut bytes = b"POST_NAME = \"Caf\xE9\"\r\nbegin RAPID_MOVE\r\n\"G0\"\r\nbegin FEED_MOVE\r\n\"G1\"\r\n".to_vec();
        assert_eq!(PostDef::parse(&decode(&bytes), &|_| None).unwrap().name, "Caf\u{e9}");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.append(&mut bytes);
        assert!(decode(&with_bom).starts_with("POST_NAME"));
    }

    #[test]
    fn today_is_written_as_a_real_date() {
        let (date, time) = now_strings();
        assert_eq!(date.len(), 10);
        assert!(date.starts_with("20"), "{date}");
        assert_eq!(time.len(), 5);
    }

    fn circle_moves() -> Vec<Move> {
        let mut m = vec![Move { p: [7.0, 5.0, 5.0], rapid: true }];
        for i in 0..=256 {
            let a = i as f64 / 256.0 * std::f64::consts::TAU;
            m.push(Move { p: [5.0 + 2.0 * a.cos(), 5.0 + 2.0 * a.sin(), -1.0], rapid: false });
        }
        m
    }

    #[test]
    fn curves_use_the_arc_blocks_when_asked_and_their_centres_are_always_written() {
        let t = mm_tool();
        let m = circle_moves();
        let path = [PostPath { name: "p", tool: &t, moves: &m, dwell: 0.0, steps: None }];
        let plain = render(&def(), &path, &job(), None);
        assert!(!plain.contains("G3"));
        // Without a way to write the centres, it stays with straight moves.
        assert!(!render(&def(), &path, &job(), Some(0.012)).contains(" G3 "));
        let with_centres = SAMPLE.replacen("begin FIRST_CW_ARC_MOVE", "VAR ARC_CENTRE_I_INC_POSITION = [I|A|I|1.0|3]\nVAR ARC_CENTRE_J_INC_POSITION = [J|A|J|1.0|3]\n\nbegin FIRST_CW_ARC_MOVE", 1);
        let d = PostDef::parse(&with_centres, &|_| None).unwrap();
        assert!(d.formats.contains_key("I"), "{:?}", d.formats.keys());
        let text = render(&d, &path, &job(), Some(0.012));
        assert!(text.contains("G3"), "{text}");
        assert!(text.lines().count() < plain.lines().count() / 5, "{} vs {}", text.lines().count(), plain.lines().count());
        for line in text.lines().filter(|l| l.contains("G3")) {
            assert!(line.contains(" I") && line.contains(" J") || line.contains('I') && line.contains('J'), "{line}");
        }
    }

    #[test]
    fn a_post_processor_without_arc_blocks_gets_straight_moves() {
        let no_arcs = SAMPLE.replace("begin FIRST_CCW_ARC_MOVE", "begin UNUSED_ONE").replace("begin CCW_ARC_MOVE", "begin UNUSED_TWO");
        let d = PostDef::parse(&no_arcs, &|_| None).unwrap();
        let t = mm_tool();
        let m = circle_moves();
        let text = render(&d, &[PostPath { name: "p", tool: &t, moves: &m, dwell: 0.0, steps: None }], &job(), Some(0.012));
        assert!(!text.contains(" G2 ") && !text.contains(" G3 "), "{text}");
    }

    #[test]
    fn each_kind_of_move_uses_its_first_block_only_when_the_kind_changes() {
        let mut d = def();
        for (name, line) in [
            ("INITIAL_RAPID_MOVE", "INIT [X]"),
            ("FIRST_RAPID_MOVE", "G0 [X]"),
            ("RAPID_MOVE", "[X]"),
            ("FIRST_PLUNGE_MOVE", "DOWN1 [Z]"),
            ("PLUNGE_MOVE", "DOWN [Z]"),
            ("FIRST_FEED_MOVE", "G1 [X]"),
            ("FEED_MOVE", "[X]"),
            ("RETRACT_MOVE", "UP [Z]"),
            ("FEED_RATE_CHANGE", "RATE [F]"),
            ("NEW_SEGMENT", "SEGMENT"),
            ("TOOLCHANGE", "CHANGE"),
        ] {
            d.blocks.insert(name.to_string(), vec![line.to_string()]);
        }
        d.blocks.insert("HEADER".to_string(), vec!["HEAD".to_string()]);
        d.blocks.insert("FOOTER".to_string(), vec!["FOOT".to_string()]);
        let m = |p: [f64; 3], rapid: bool| Move { p, rapid };
        let one = vec![m([10.0, 0.0, 5.0], true), m([20.0, 0.0, 5.0], true), m([20.0, 0.0, -2.0], false), m([30.0, 0.0, -2.0], false), m([40.0, 0.0, -2.0], false), m([40.0, 0.0, 5.0], true)];
        let (t, mut other) = (mm_tool(), mm_tool());
        other.number += 1;
        let path = |tool, moves| PostPath { name: "P", tool, moves, dwell: 0.0, steps: None };
        let text = render(&d, &[path(&t, &one), path(&t, &one), path(&other, &one)], &job(), None);
        let blocks: Vec<String> = text.lines().map(|l| l.split_whitespace().next().unwrap_or("").trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-').to_string()).collect();
        let first = ["HEAD", "INIT", "X", "RATE", "DOWN", "RATE", "G", "X", "UP"];
        assert!(blocks[..9].iter().zip(first).all(|(b, f)| b.starts_with(f)), "{text}");
        // The next toolpath starts over with the first block, and repeats what the last one set.
        let second = text.lines().position(|l| l == "SEGMENT").unwrap();
        assert!(text.lines().nth(second + 1).unwrap().starts_with("G0  X10"), "{text}");
        // With a new tool, both lines, and then the block for after a change of tool.
        let change = text.lines().position(|l| l == "CHANGE").unwrap();
        assert_eq!(text.lines().nth(change + 1), Some("SEGMENT"));
        assert!(text.lines().nth(change + 2).unwrap().starts_with("INIT  X10"), "{text}");
        assert_eq!(text.lines().last(), Some("FOOT"));
    }
}
