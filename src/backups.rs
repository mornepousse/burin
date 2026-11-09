//! Earlier versions of a project, kept beside it each time it's saved, like KiCad's
//! `<project>-backups` folder. Differences: each version is a plain `.burin` that opens as it is, a
//! version the same as the last isn't kept twice, and old ones are thinned out by age rather than cut
//! at a count, so last week stays as well as the last hour. The original has nothing like this.

use crate::app::*;
use crate::i18n::tr;
use egui::{Align2, RichText};
use std::path::{Path, PathBuf};

const EXTENSION: &str = "burin";
/// Within this, every version; then one an hour for a day, one a day for 30 days, one a week.
const RECENT: u64 = 3600;
const DAY: u64 = 86400;
const MONTH: u64 = 30 * DAY;
const BUCKET_WEEK: u64 = 7 * DAY;
pub const DEFAULT_MB: u64 = 200;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub on: bool,
    /// The most the versions of one project may take together.
    pub most_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { on: true, most_mb: DEFAULT_MB }
    }
}

pub fn load_settings() -> Settings {
    if cfg!(test) {
        return Settings::default();
    }
    let text = settings_file("backups.txt").and_then(|f| crate::platform::read_to_string(f).ok()).unwrap_or_default();
    let mut s = Settings::default();
    for line in text.lines() {
        match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("on", v)) => s.on = v != "no",
            Some(("most_mb", v)) => s.most_mb = v.parse().unwrap_or(DEFAULT_MB).max(1),
            _ => {}
        }
    }
    s
}

pub fn save_settings(s: Settings) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = settings_file("backups.txt") {
        let _ = crate::platform::write(file, format!("on={}\nmost_mb={}\n", if s.on { "yes" } else { "no" }, s.most_mb));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub file: PathBuf,
    /// Seconds since 1970, universal time, from its name.
    pub time: u64,
    pub size: u64,
}

pub fn folder_for(project: &Path) -> PathBuf {
    let stem = project.file_stem().map_or_else(|| "project".to_string(), |s| s.to_string_lossy().to_string());
    project.with_file_name(format!("{stem}-backups"))
}

fn stem_of(project: &Path) -> String {
    project.file_stem().map_or_else(|| "project".to_string(), |s| s.to_string_lossy().to_string())
}

/// `2026-11-07 090512` back to seconds, from the end of a version's name.
fn time_in_name(name: &str) -> Option<u64> {
    let tail = name.get(name.len().checked_sub(17)?..)?;
    let (date, clock) = tail.split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    if clock.len() != 6 || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let (h, min, s) = (clock[0..2].parse::<u64>().ok()?, clock[2..4].parse::<u64>().ok()?, clock[4..6].parse::<u64>().ok()?);
    // Days from a date, the other way round from `crash::stamp`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    Some(u64::try_from(days).ok()? * DAY + h * 3600 + min * 60 + s)
}

pub fn name_at(stem: &str, time: u64) -> String {
    let stamp = crate::crash::stamp(time);
    let (date, clock) = stamp.split_once(' ').unwrap_or((&stamp, ""));
    format!("{stem} {date} {}.{EXTENSION}", clock.replace(':', ""))
}

/// The versions of a project, the newest first.
pub fn list(project: &Path) -> Vec<Version> {
    let stem = stem_of(project);
    let Ok(entries) = std::fs::read_dir(folder_for(project)) else { return Vec::new() };
    let mut out: Vec<Version> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_stem()?.to_string_lossy().to_string();
            if path.extension()? != EXTENSION || !name.starts_with(&stem) {
                return None;
            }
            Some(Version { time: time_in_name(&name)?, size: e.metadata().ok()?.len(), file: path })
        })
        .collect();
    out.sort_by(|a, b| b.time.cmp(&a.time).then(b.file.cmp(&a.file)));
    out
}

/// Which versions to let go, newest first in `versions`: in each span of time only the newest of
/// each slot stays, then the oldest go until they fit in `most_bytes`. The newest always stays.
pub fn to_remove(versions: &[Version], now: u64, most_bytes: u64) -> Vec<usize> {
    let mut seen: Vec<(u8, u64)> = Vec::new();
    let mut keep: Vec<usize> = Vec::new();
    for (i, v) in versions.iter().enumerate() {
        let age = now.saturating_sub(v.time);
        let slot = if age < RECENT {
            (0, i as u64)
        } else if age < DAY {
            (1, v.time / 3600)
        } else if age < MONTH {
            (2, v.time / DAY)
        } else {
            (3, v.time / BUCKET_WEEK)
        };
        if i == 0 || !seen.contains(&slot) {
            seen.push(slot);
            keep.push(i);
        }
    }
    let mut total: u64 = keep.iter().map(|i| versions[*i].size).sum();
    while total > most_bytes && keep.len() > 1 {
        let oldest = keep.pop().unwrap();
        total -= versions[oldest].size;
    }
    (0..versions.len()).filter(|i| !keep.contains(i)).collect()
}

/// Keeps the file as it is now, before `new` is saved over it. Nothing is kept for a file that isn't
/// there yet, that isn't changing, or that's the same as the last version kept.
pub fn keep_before_saving(project: &Path, new: &[u8], now: u64, settings: Settings) -> Result<Option<PathBuf>, String> {
    if !settings.on || project.extension().is_none_or(|e| e != EXTENSION) || !project.exists() {
        return Ok(None);
    }
    let bytes = crate::platform::read(project).map_err(|e| e.to_string())?;
    if bytes == new {
        return Ok(None);
    }
    let versions = list(project);
    if versions.first().and_then(|v| crate::platform::read(&v.file).ok()).is_some_and(|last| last == bytes) {
        return Ok(None);
    }
    let folder = folder_for(project);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut time = now;
    // Saved twice within a second: the second goes a second on, so neither is lost.
    while folder.join(name_at(&stem_of(project), time)).exists() {
        time += 1;
    }
    let file = folder.join(name_at(&stem_of(project), time));
    let beside = file.with_extension("saving");
    crate::platform::write(&beside, &bytes).and_then(|()| std::fs::rename(&beside, &file)).map_err(|e| e.to_string())?;
    let versions = list(project);
    for i in to_remove(&versions, now, settings.most_mb * 1024 * 1024) {
        let _ = std::fs::remove_file(&versions[i].file);
    }
    Ok(Some(file))
}

fn seconds_now() -> u64 {
    crate::platform::SystemTime::now().duration_since(crate::platform::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "5 minutes ago" and the like.
pub fn how_long_ago(seconds: u64) -> String {
    match seconds {
        s if s < 60 => tr("just now").to_string(),
        s if s < 3600 => tf!("{} min ago", s / 60),
        s if s < DAY => tf!("{} h ago", s / 3600),
        s if s < 2 * DAY => tr("1 day ago").to_string(),
        s => tf!("{} days ago", s / DAY),
    }
}

impl App {
    /// Called just before the project's file is written.
    pub(crate) fn backup_before_saving(&mut self, path: &Path, new: &[u8]) -> Option<String> {
        match keep_before_saving(path, new, seconds_now(), self.backup_settings) {
            Ok(_) => None,
            Err(e) => Some(tf!("No backup could be kept: {}", e)),
        }
    }

    /// Opens a version as the project not yet saved: saving puts it back, after keeping what's there.
    pub fn open_version(&mut self, version: &Version) -> Result<(), String> {
        let project = self.path.clone();
        let text = crate::platform::read_to_string(&version.file).map_err(|e| e.to_string())?;
        let doc: Doc = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        self.checkpoint();
        self.doc = doc;
        self.reset_state();
        self.screen = Screen::Work;
        self.recalculate_all();
        self.path = project;
        self.dirty = true;
        self.status = tf!("The version from {} is open. Saving makes it the project again; what was saved before is kept as a backup.", crate::crash::stamp(version.time));
        Ok(())
    }

    pub(crate) fn backups_window(&mut self, ctx: &egui::Context) {
        if !self.backups_open {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        let versions = self.path.as_deref().map(list).unwrap_or_default();
        let now = seconds_now();
        egui::Window::new(tr("Project Backups")).id(egui::Id::unique("backups")).open(&mut open).collapsible(false).default_width(520.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            let mut s = self.backup_settings;
            ui.checkbox(&mut s.on, tr("Keep the earlier version each time a project is saved"));
            ui.horizontal(|ui| {
                ui.label(tr("At most, for each project (MB)"));
                ui.add(egui::DragValue::new(&mut s.most_mb).range(10..=10000));
            });
            if s != self.backup_settings {
                self.backup_settings = s;
                save_settings(s);
            }
            ui.label(RichText::new(tr("They go in a folder beside the project. Every version from the last hour is kept, then one an hour for a day, one a day for a month, and one a week after that.")).size(12.0).color(crate::theme::MUTED()));
            ui.separator();
            match &self.path {
                None => {
                    ui.label(tr("Save the project first: its backups are kept beside its file."));
                }
                Some(_) if versions.is_empty() => {
                    ui.label(tr("No backups yet: the first is kept the next time this project is saved."));
                }
                Some(_) => {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        egui::Grid::new("backup-list").num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
                            for v in &versions {
                                ui.label(how_long_ago(now.saturating_sub(v.time)));
                                ui.label(RichText::new(format!("{} UTC", &crate::crash::stamp(v.time)[..16])).color(crate::theme::MUTED()));
                                ui.label(tf!("{:.1} MB", v.size as f64 / 1_048_576.0));
                                if ui.button(tr("Open This Version")).clicked() {
                                    chosen = Some(v.clone());
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
            }
            if let Some(project) = &self.path {
                let folder = folder_for(project);
                if folder.is_dir() && ui.button(tr("Show the Folder")).clicked() {
                    let address = folder.to_string_lossy().replace('\\', "/").replace(' ', "%20");
                    let slash = if address.starts_with('/') { "" } else { "/" };
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{slash}{address}")));
                }
            }
        });
        self.backups_open = open;
        if let Some(v) = chosen {
            // Work not saved would go: asked about in the usual way first.
            if self.has_unsaved_work() {
                self.message = Some(tr("Save or undo your changes first: opening a version replaces what's on screen.").into());
            } else if let Err(why) = self.open_version(&v) {
                self.message = Some(why);
            } else {
                self.backups_open = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(time: u64, size: u64) -> Version {
        Version { file: PathBuf::from(format!("{time}")), time, size }
    }

    #[test]
    fn names_carry_their_time_both_ways() {
        for t in [0, 951782400, 1794041400, 1794041459] {
            let name = name_at("my wheel 2", t);
            assert_eq!(time_in_name(name.trim_end_matches(".burin")), Some(t), "{name}");
        }
        assert_eq!(name_at("wheel", 1794041400), "wheel 2026-11-07 085000.burin");
        assert_eq!(time_in_name("wheel"), None);
        assert_eq!(time_in_name("wheel 2026-13-07 085000"), None);
    }

    #[test]
    fn older_versions_thin_out_by_age() {
        let now = 100 * DAY;
        // Every minute for the last 2 hours, newest first.
        let versions: Vec<Version> = (0..120).map(|m| v(now - m * 60, 1)).collect();
        let gone = to_remove(&versions, now, u64::MAX);
        let kept: Vec<u64> = (0..versions.len()).filter(|i| !gone.contains(i)).map(|i| (now - versions[i].time) / 60).collect();
        assert_eq!(kept[0], 0, "the newest stays");
        assert_eq!(kept.iter().filter(|m| **m < 60).count(), 60, "all of the last hour: {kept:?}");
        assert!(kept.iter().filter(|m| **m >= 60).count() <= 2, "one an hour after that: {kept:?}");

        // A version a day for 90 days: daily for a month, then weekly.
        let versions: Vec<Version> = (0..90).map(|d| v(now - d * DAY, 1)).collect();
        let gone = to_remove(&versions, now, u64::MAX);
        let kept = versions.len() - gone.len();
        assert!((30 + 8..=30 + 10).contains(&kept), "{kept}");
    }

    #[test]
    fn the_size_limit_drops_the_oldest_but_never_the_newest() {
        let now = 10 * DAY;
        let versions: Vec<Version> = (0..5).map(|d| v(now - d * DAY, 100)).collect();
        assert_eq!(to_remove(&versions, now, 250), [2, 3, 4]);
        assert_eq!(to_remove(&versions, now, 1), [1, 2, 3, 4]);
    }

    #[test]
    fn a_version_is_kept_before_each_save_but_not_twice() {
        let dir = std::env::temp_dir().join(format!("burin-backups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("my wheel.burin");
        let on = Settings::default();
        assert_eq!(keep_before_saving(&project, b"new", 1000, on).unwrap(), None, "nothing there yet");
        crate::platform::write(&project, "one").unwrap();
        let first = keep_before_saving(&project, b"new", 2000, on).unwrap().unwrap();
        assert_eq!(crate::platform::read_to_string(&first).unwrap(), "one");
        assert!(first.starts_with(dir.join("my wheel-backups")));
        assert_eq!(keep_before_saving(&project, b"new", 2001, on).unwrap(), None, "the same again isn't kept");
        assert_eq!(keep_before_saving(&project, b"one", 2002, on).unwrap(), None, "not changing, nothing kept");
        crate::platform::write(&project, "two").unwrap();
        keep_before_saving(&project, b"new", 2000, on).unwrap().unwrap();
        assert_eq!(list(&project).len(), 2, "same second, both kept");
        assert_eq!(keep_before_saving(&project, b"new", 3000, Settings { on: false, ..on }).unwrap(), None);
        assert_eq!(keep_before_saving(&dir.join("other.nc"), b"new", 3000, on).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
