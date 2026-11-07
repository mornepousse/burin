//! A copy of work not yet saved, made every so often, so a crash can't take much with it. It's deleted
//! when the program closes normally, or once the work has been saved.

use crate::app::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const EVERY_SECONDS: u64 = 120;
const COPY: &str = "recovery.burin";
const WHERE_IT_WAS: &str = "recovery-path.txt";

static WRITING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub file: PathBuf,
    /// The project's own file, if it had one.
    pub project: Option<PathBuf>,
    pub saved_at: String,
}

pub fn find(data: &Path) -> Option<Found> {
    let file = data.join(COPY);
    let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
    let seconds = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let project = std::fs::read_to_string(data.join(WHERE_IT_WAS)).ok().filter(|p| !p.trim().is_empty()).map(|p| PathBuf::from(p.trim()));
    Some(Found { file, project, saved_at: crate::crash::stamp(seconds) })
}

pub fn remove(data: &Path) {
    let _ = std::fs::remove_file(data.join(COPY));
    let _ = std::fs::remove_file(data.join(WHERE_IT_WAS));
}

/// Written beside it first, so that a crash while writing leaves the last good copy.
pub fn write(data: &Path, doc: &Doc, project: Option<&Path>) -> Result<(), String> {
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let text = serde_json::to_string(doc).map_err(|e| e.to_string())?;
    let beside = data.join("recovery.saving");
    std::fs::write(&beside, text).map_err(|e| e.to_string())?;
    std::fs::rename(&beside, data.join(COPY)).map_err(|e| e.to_string())?;
    std::fs::write(data.join(WHERE_IT_WAS), project.map_or(String::new(), |p| p.to_string_lossy().to_string())).map_err(|e| e.to_string())
}

impl App {
    /// Called every frame: keeps the copy up to date, on another thread, and clears it once saved.
    pub(crate) fn keep_recovery(&mut self) {
        let Some(data) = self.data_dir.clone() else { return };
        // Waiting on an answer about the last copy: it mustn't be overwritten yet.
        if self.recovery.is_some() || self.recovery_left {
            return;
        }
        if !self.has_unsaved_work() {
            if self.recovery_written {
                remove(&data);
                self.recovery_written = false;
            }
            return;
        }
        let due = self.recovery_at.get_or_insert_with(Instant::now).elapsed().as_secs() >= self.recovery_every.unwrap_or(EVERY_SECONDS);
        if due && !WRITING.swap(true, Ordering::Relaxed) {
            self.recovery_at = Some(Instant::now());
            self.recovery_written = true;
            let (doc, project) = (self.doc.clone(), self.path.clone());
            let run = move || {
                let _ = write(&data, &doc, project.as_deref());
                WRITING.store(false, Ordering::Relaxed);
            };
            if self.surface_in_background {
                std::thread::spawn(run);
            } else {
                run();
            }
        }
    }

    /// Back to where the work was: unsaved, and still under its own file's name.
    pub fn restore_recovery(&mut self, found: &Found) -> Result<(), String> {
        let text = std::fs::read_to_string(&found.file).map_err(|e| e.to_string())?;
        let doc: Doc = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        self.doc = doc;
        self.reset_state();
        self.screen = Screen::Work;
        self.recalculate_all();
        self.path = found.project.clone();
        self.dirty = true;
        self.recovery_written = true;
        self.status = crate::i18n::tr("Work brought back. Save it to keep it.").into();
        Ok(())
    }
}
