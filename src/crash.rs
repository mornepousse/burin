//! Reports of what went wrong, kept on this computer. Burin never sends one: the person reads it and
//! decides (`feedback.rs`). They hold what helps to find the fault, never the job or its file names.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const FORMAT: &str = "burin-crash/1";
const MOST_KEPT: usize = 20;
const MOST_DAYS: u64 = 90;
const MOST_PER_RUN: usize = 5;
const RECENT_ACTIONS: usize = 32;
const MOST_FRAMES: usize = 40;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub format: String,
    /// `panic`, `unclean_exit` or `no_window`.
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub backtrace: Vec<String>,
    pub version: String,
    pub system: String,
    #[serde(default)]
    pub graphics: String,
    /// The last thing the program had got to, from its log.
    #[serde(default)]
    pub last_step: String,
    #[serde(default)]
    pub seconds_running: u64,
    #[serde(default)]
    pub recent_actions: Vec<String>,
    /// How big the job was. Never what's in it.
    #[serde(default)]
    pub job: String,
    pub signature: String,
    pub count: u32,
    pub first_seen: String,
    pub last_seen: String,
    /// Has the person been told about it?
    #[serde(default)]
    pub seen: bool,
    /// The file it's kept in.
    #[serde(skip)]
    pub file: PathBuf,
}

impl Report {
    /// What it was in a few words, for a list or a title.
    pub fn headline(&self) -> String {
        let first = self.message.lines().next().unwrap_or("");
        let short: String = first.chars().take(90).collect();
        if short.chars().count() < first.chars().count() { format!("{short}...") } else { short }
    }

    /// The report as text a person can read and check.
    pub fn text(&self) -> String {
        let mut out = format!("{}\nKind: {}\nVersion: {}\nSystem: {}\n", self.message, self.kind, self.version, self.system);
        for (label, value) in [("Where", &self.location), ("Thread", &self.thread), ("Graphics", &self.graphics), ("Last step", &self.last_step), ("Job", &self.job)] {
            if !value.is_empty() {
                out.push_str(&format!("{label}: {value}\n"));
            }
        }
        out.push_str(&format!("Running for: {} s\nTimes seen: {} (first {}, last {})\n", self.seconds_running, self.count, self.first_seen, self.last_seen));
        if !self.recent_actions.is_empty() {
            out.push_str("\nLast actions:\n");
            for a in &self.recent_actions {
                out.push_str(&format!("  {a}\n"));
            }
        }
        if !self.backtrace.is_empty() {
            out.push_str("\nBacktrace:\n");
            for f in &self.backtrace {
                out.push_str(&format!("  {f}\n"));
            }
        }
        out
    }
}

// ---- What's going on, for when something goes wrong ----

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static STARTED: OnceLock<Instant> = OnceLock::new();
static SHAPES: AtomicUsize = AtomicUsize::new(0);
static TOOLPATHS: AtomicUsize = AtomicUsize::new(0);
static MOVES: AtomicU64 = AtomicU64::new(0);
static WRITTEN: AtomicUsize = AtomicUsize::new(0);

pub fn begin_session() {
    let _ = STARTED.set(Instant::now());
}

/// Notes something the person did: the name of a command, nothing they typed.
pub fn note(action: &str) {
    if let Ok(mut recent) = RECENT.lock() {
        if recent.len() == RECENT_ACTIONS {
            recent.pop_front();
        }
        recent.push_back(action.to_string());
    }
}

pub fn set_job(shapes: usize, toolpaths: usize, moves: u64) {
    SHAPES.store(shapes, Ordering::Relaxed);
    TOOLPATHS.store(toolpaths, Ordering::Relaxed);
    MOVES.store(moves, Ordering::Relaxed);
}

fn job_now() -> String {
    format!("{} shapes, {} toolpaths, {} moves", SHAPES.load(Ordering::Relaxed), TOOLPATHS.load(Ordering::Relaxed), MOVES.load(Ordering::Relaxed))
}

// ---- Keeping private things out ----

/// The person's home folder says who they are, and it turns up in paths.
pub fn without_home(text: &str) -> String {
    let mut out = text.to_string();
    for name in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(name).map(|h| h.to_string_lossy().to_string()).filter(|h| h.len() > 1) {
            out = out.replace(&home, "~");
        }
    }
    out
}

/// A panic's backtrace with the machinery of panicking left out.
fn clean_frames(raw: &str) -> Vec<String> {
    let mut frames: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if let Some(place) = line.strip_prefix("at ") {
            if let Some(last) = frames.last_mut() {
                last.push_str(&format!(" ({})", without_home(place)));
            }
        } else if let Some((number, symbol)) = line.split_once(": ") {
            if number.chars().all(|c| c.is_ascii_digit()) {
                frames.push(symbol.to_string());
            }
        }
    }
    // Everything up to the panic itself is the machinery of panicking, and of this report.
    let machinery = ["record_panic", "PanicHookInfo", "panic_with_hook", "rust_panic", "panic_handler", "begin_panic", "rust_begin_unwind", "__rust_end_short_backtrace", "core::panicking"];
    if let Some(last) = frames.iter().rposition(|f| machinery.iter().any(|m| f.contains(m))) {
        frames.drain(..=last);
    }
    frames.truncate(MOST_FRAMES);
    frames
}

/// The same fault gives the same mark, whatever numbers are in its message.
pub fn signature(kind: &str, location: &str, message: &str) -> String {
    let mut hash: u32 = 0x811c9dc5;
    for b in format!("{kind}|{location}|{message}").bytes().filter(|b| !b.is_ascii_digit()) {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08x}")
}

// ---- Dates, with no library for them ----

fn seconds_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `2026-11-07 09:00:00` in universal time.
pub fn stamp(seconds: u64) -> String {
    let (days, rest) = (seconds / 86400, seconds % 86400);
    // Days to a date in the Gregorian calendar, counting years from March.
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let day_of_era = z.rem_euclid(146097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

// ---- The folder of reports ----

pub fn folder() -> Option<PathBuf> {
    crate::app::settings_file("crashes")
}

pub fn list_in(dir: &Path) -> Vec<Report> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Report> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let mut report: Report = serde_json::from_str(&text).ok()?;
            report.file = e.path();
            Some(report)
        })
        .collect();
    out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    out
}

fn write(report: &Report) {
    if let Ok(text) = serde_json::to_string_pretty(report) {
        let _ = std::fs::write(&report.file, text);
    }
}

/// Keeps a report: a fault seen before only counts up. Old ones go when there are too many.
pub fn save_in(dir: &Path, mut report: Report, now: u64) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let existing = list_in(dir);
    let when = stamp(now);
    if let Some(mut same) = existing.iter().find(|r| r.signature == report.signature).cloned() {
        same.count += 1;
        same.last_seen = when;
        same.seen = false;
        same.seconds_running = report.seconds_running;
        same.recent_actions = std::mem::take(&mut report.recent_actions);
        same.job = report.job;
        same.version = report.version;
        write(&same);
        return same.file;
    }
    report.count = 1;
    report.first_seen = when.clone();
    report.last_seen = when.clone();
    let name = format!("{}-{}.json", when.replace([' ', ':', '-'], ""), report.signature);
    report.file = dir.join(name);
    write(&report);
    tidy_in(dir, now);
    report.file
}

/// Too many or too old: the oldest go first.
pub fn tidy_in(dir: &Path, now: u64) {
    let cutoff = stamp(now.saturating_sub(MOST_DAYS * 86400));
    for (i, report) in list_in(dir).iter().enumerate() {
        if i >= MOST_KEPT || report.last_seen < cutoff {
            let _ = std::fs::remove_file(&report.file);
        }
    }
}

pub fn mark_seen(reports: &mut [Report]) {
    for r in reports.iter_mut().filter(|r| !r.seen) {
        r.seen = true;
        write(r);
    }
}

fn base_report(kind: &str, message: String, location: String) -> Report {
    let recent = RECENT.try_lock().map(|r| r.iter().cloned().collect()).unwrap_or_default();
    Report {
        format: FORMAT.to_string(),
        kind: kind.to_string(),
        signature: signature(kind, &location, &message),
        message: without_home(&message),
        location: without_home(&location),
        version: env!("CARGO_PKG_VERSION").to_string(),
        system: crate::startup::system_line(),
        graphics: crate::startup::graphics_line(),
        seconds_running: STARTED.get().map_or(0, |s| s.elapsed().as_secs()),
        recent_actions: recent,
        job: job_now(),
        ..Default::default()
    }
}

/// Called as a panic happens. Nothing here may panic in turn.
pub fn record_panic(info: &std::panic::PanicHookInfo<'_>) {
    if WRITTEN.fetch_add(1, Ordering::Relaxed) >= MOST_PER_RUN {
        return;
    }
    let Some(dir) = folder() else { return };
    let message = crate::startup::panic_text(info.payload());
    let location = info.location().map_or(String::new(), |l| l.to_string());
    let mut report = base_report("panic", message, location);
    report.thread = std::thread::current().name().unwrap_or("unnamed").to_string();
    report.backtrace = clean_frames(&std::backtrace::Backtrace::force_capture().to_string());
    save_in(&dir, report, seconds_now());
}

pub fn record_no_window(reasons: &[String]) {
    let Some(dir) = folder() else { return };
    save_in(&dir, base_report("no_window", reasons.join("\n"), String::new()), seconds_now());
}

/// What the last start's log says about how it ended, if it ended badly: it says so itself when it
/// closes, so a log without that line belongs to a program that died. A panic was reported as it
/// happened. Logs from another version aren't compared.
pub fn died_last_time(log: &str, version: &str) -> Option<Report> {
    let mut lines = log.lines();
    if !lines.next()?.starts_with(&format!("Burin {version} ")) {
        return None;
    }
    let lines: Vec<&str> = lines.collect();
    if lines.iter().any(|l| *l == crate::startup::CLOSED || l.starts_with("Crash: ")) {
        return None;
    }
    let native = lines.iter().find(|l| l.starts_with("Crash outside")).map(|l| l.to_string());
    let last = lines.iter().rev().find(|l| !l.starts_with("Crash outside")).unwrap_or(&"").to_string();
    let drawing = lines.iter().any(|l| l.starts_with("Drawing: "));
    let message = native.clone().unwrap_or_else(|| {
        if drawing { "Burin ended without saying why while it was running (a crash in the system or a graphics driver, the program being killed, or the computer shutting down).".to_string() } else { "Burin ended without saying why while it was opening.".to_string() }
    });
    let graphics = lines.iter().find_map(|l| l.strip_prefix("Graphics card: ")).unwrap_or("").to_string();
    let system = lines.iter().find_map(|l| l.strip_prefix("System: ")).unwrap_or("").to_string();
    let location = if drawing { "while running".to_string() } else { format!("opening: {last}") };
    Some(Report {
        format: FORMAT.to_string(),
        kind: "unclean_exit".to_string(),
        signature: signature("unclean_exit", &location, &message),
        message: without_home(&message),
        location,
        last_step: without_home(&last),
        version: version.to_string(),
        system,
        graphics,
        ..Default::default()
    })
}

/// To be called before the new log starts, and only by the one copy of the program running.
pub fn check_last_run(log: &str) {
    if let (Some(dir), Some(report)) = (folder(), died_last_time(log, env!("CARGO_PKG_VERSION"))) {
        save_in(&dir, report, seconds_now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("burin-crash-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn report(message: &str) -> Report {
        Report { format: FORMAT.into(), kind: "panic".into(), message: message.into(), signature: signature("panic", "here", message), version: "1".into(), ..Default::default() }
    }

    #[test]
    fn dates_come_out_right() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00");
        assert_eq!(stamp(951782400), "2000-02-29 00:00:00");
        assert_eq!(stamp(1794041400), "2026-11-07 08:50:00");
    }

    #[test]
    fn the_same_fault_counts_up_whatever_its_numbers() {
        assert_eq!(signature("panic", "a.rs:1", "index 5 out of range"), signature("panic", "a.rs:1", "index 77 out of range"));
        assert_ne!(signature("panic", "a.rs:1", "index"), signature("panic", "b.rs:1", "index"));
        let dir = scratch("count");
        save_in(&dir, report("index 5"), 1000);
        save_in(&dir, report("index 6"), 2000);
        save_in(&dir, report("something else"), 3000);
        let all = list_in(&dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].message, "something else", "the newest comes first");
        assert_eq!(all[1].count, 2);
        assert_eq!(all[1].first_seen, stamp(1000));
        assert_eq!(all[1].last_seen, stamp(2000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_and_extra_reports_go() {
        let dir = scratch("tidy");
        let now = 200 * 86400;
        for i in 0..25 {
            save_in(&dir, report(&format!("fault {}", "x".repeat(i))), now + i as u64);
        }
        assert_eq!(list_in(&dir).len(), MOST_KEPT);
        assert!(list_in(&dir).iter().all(|r| r.message.len() >= 11), "the oldest went");
        tidy_in(&dir, now + (MOST_DAYS + 1) * 86400);
        assert!(list_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_tells_how_the_last_run_ended() {
        let closed = format!("Burin 1.0 on linux x86_64\nSystem: linux\nTrying: graphics\nDrawing: graphics\n{}\n", crate::startup::CLOSED);
        assert!(died_last_time(&closed, "1.0").is_none());
        assert!(died_last_time("", "1.0").is_none());
        // A panic has its own report already, and another version's log says nothing about this one.
        assert!(died_last_time("Burin 1.0 on linux\nDrawing: graphics\nCrash: boom\n", "1.0").is_none());
        assert!(died_last_time("Burin 0.9 on linux\nDrawing: graphics\n", "1.0").is_none());

        let running = died_last_time("Burin 1.0 on linux\nSystem: 8 threads\nTrying: graphics\nGraphics card: Some Card\nThe window is showing.\nDrawing: graphics\n", "1.0").unwrap();
        assert_eq!(running.kind, "unclean_exit");
        assert_eq!(running.location, "while running");
        assert_eq!(running.graphics, "Some Card");
        assert_eq!(running.system, "8 threads");

        let opening = died_last_time("Burin 1.0 on linux\nTrying: opengl\n", "1.0").unwrap();
        assert_eq!(opening.location, "opening: Trying: opengl");
        assert!(opening.message.contains("opening"));

        let native = died_last_time("Burin 1.0 on windows\nTrying: graphics\nCrash outside the program's code: 0xc0000005 at 0x1 in driver.dll\n", "1.0").unwrap();
        assert!(native.message.contains("driver.dll"));
    }

    #[test]
    fn a_backtrace_keeps_what_matters_and_not_the_home_folder() {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap();
        let raw = format!("   0: std::backtrace_rs::backtrace::trace\n             at /rustc/abc/x.rs:1:2\n   1: std::panicking::begin_panic\n   2: core::panicking::panic_fmt\n   3: burin::toolpath::generate\n             at {home}/code/src/toolpath.rs:12:3\n   4: burin::main\n");
        let frames = clean_frames(&raw);
        assert_eq!(frames, ["burin::toolpath::generate (~/code/src/toolpath.rs:12:3)", "burin::main"]);
        assert_eq!(without_home(&format!("open {home}/job.burin failed")), "open ~/job.burin failed");
    }

    #[test]
    fn the_text_of_a_report_is_readable() {
        let mut r = report("boom");
        r.location = "a.rs:1:2".into();
        r.recent_actions = vec!["Save".into()];
        r.backtrace = vec!["burin::main".into()];
        let text = r.text();
        for part in ["boom", "Where: a.rs:1:2", "Last actions:", "  Save", "Backtrace:", "  burin::main"] {
            assert!(text.contains(part), "{part}: {text}");
        }
        assert_eq!(report(&"long ".repeat(50)).headline().chars().count(), 93);
    }
}
