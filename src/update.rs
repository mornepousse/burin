//! Whether a newer Burin is out. Burin sends nothing on its own: it asks only once the person has
//! said yes on the start page (or in the preferences), and then only GitHub's public page of the
//! project's latest release, by `curl` (on Linux, and on Windows 10 and later), once per start.
//! Nothing about the person or their work goes with it. The answer shows on the start page.

use std::sync::Mutex;

/// What is known so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The person's answer to "look for new versions at start?", once given.
    pub choice: Option<bool>,
    /// The newest release, when it is newer than this one: its number and its page.
    pub newer: Option<(String, String)>,
    started: bool,
    read: bool,
}

static STATE: Mutex<State> = Mutex::new(State { choice: None, newer: None, started: false, read: false });

/// Where the project is kept, as `owner/name`: from the page for reports written in at build time.
/// A copy built by hand has none, and never asks.
pub fn repository() -> Option<String> {
    let page = crate::feedback::ISSUES_PAGE?;
    let rest = page.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

/// Whether this copy can ask at all: a program on a computer, built for the project's page.
pub fn available() -> bool {
    !crate::platform::WEB && !cfg!(test) && repository().is_some()
}

fn file() -> Option<std::path::PathBuf> {
    crate::app::settings_file("updates.txt")
}

/// What is known, the saved answer read the first time.
pub fn state() -> State {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !s.read {
        s.read = true;
        if !cfg!(test) {
            let text = file().and_then(|f| crate::platform::read_to_string(f).ok()).unwrap_or_default();
            s.choice = read_choice(&text);
        }
    }
    s.clone()
}

fn read_choice(text: &str) -> Option<bool> {
    text.lines().find_map(|l| match l.trim() {
        "check=yes" => Some(true),
        "check=no" => Some(false),
        _ => None,
    })
}

/// Keeps the person's answer; a yes starts the question at once.
pub fn set_choice(yes: bool, ctx: &egui::Context) {
    {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        s.read = true;
        s.choice = Some(yes);
        if !yes {
            s.newer = None;
        }
    }
    if !cfg!(test) {
        if let Some(f) = file() {
            if let Some(folder) = f.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = crate::platform::write(f, if yes { "check=yes\n" } else { "check=no\n" });
        }
    }
    if yes {
        start(ctx);
    }
}

/// Asks GitHub for the latest release, once per start, when the person has said yes.
pub fn start(ctx: &egui::Context) {
    if !available() {
        return;
    }
    let repo = repository().unwrap_or_default();
    {
        let _ = state();
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if s.started || s.choice != Some(true) {
            return;
        }
        s.started = true;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        // A saved answer in place of GitHub's, to see the notice without a newer release out.
        let pretend = std::env::var_os("BURIN_LATEST_JSON").and_then(|f| std::fs::read_to_string(f).ok());
        let Some(text) = pretend.or_else(|| fetch(&format!("https://api.github.com/repos/{repo}/releases/latest"))) else { return };
        if let Some((version, page)) = newer_release(&text, env!("CARGO_PKG_VERSION")) {
            STATE.lock().unwrap_or_else(|e| e.into_inner()).newer = Some((version, page));
            ctx.request_repaint();
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch(address: &str) -> Option<String> {
    let mut command = std::process::Command::new("curl");
    command.args(["-fsSL", "--max-time", "10", "-H", "Accept: application/vnd.github+json", "-A", concat!("Burin/", env!("CARGO_PKG_VERSION")), address]);
    command.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        command.creation_flags(0x0800_0000);
    }
    let out = command.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_arch = "wasm32")]
fn fetch(_address: &str) -> Option<String> {
    None
}

/// The release GitHub gives, when its number is above `current`: the number and its page.
pub fn newer_release(json: &str, current: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = value.get("tag_name")?.as_str()?;
    let page = value.get("html_url")?.as_str()?;
    if value.get("draft").and_then(|v| v.as_bool()) == Some(true) || value.get("prerelease").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let version = tag.trim_start_matches('v');
    (numbers(version)? > numbers(current)? && page.starts_with("https://github.com/")).then(|| (version.to_string(), page.to_string()))
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_higher_number_is_a_newer_version() {
        let json = |tag: &str| format!(r#"{{"tag_name":"{tag}","html_url":"https://github.com/o/b/releases/tag/{tag}","draft":false,"prerelease":false}}"#);
        assert_eq!(newer_release(&json("v0.13.0"), "0.12.0"), Some(("0.13.0".into(), "https://github.com/o/b/releases/tag/v0.13.0".into())));
        assert_eq!(newer_release(&json("v0.12.10"), "0.12.9").map(|n| n.0), Some("0.12.10".into()));
        assert_eq!(newer_release(&json("v0.12.0"), "0.12.0"), None);
        assert_eq!(newer_release(&json("v0.11.0"), "0.12.0"), None);
        assert_eq!(newer_release("not json", "0.12.0"), None);
        assert_eq!(newer_release(r#"{"tag_name":"v1.0.0","html_url":"https://elsewhere.example/x"}"#, "0.12.0"), None);
    }

    #[test]
    fn the_answer_is_read_back() {
        assert_eq!(read_choice("check=yes\n"), Some(true));
        assert_eq!(read_choice("check=no"), Some(false));
        assert_eq!(read_choice(""), None);
    }
}
