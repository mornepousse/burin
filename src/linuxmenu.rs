//! Putting Burin in the Linux applications menu and making it the program for `.burin` files. An
//! AppImage does nothing like this by itself, and it's the only thing writing outside the settings.

use std::path::{Path, PathBuf};

const DESKTOP: &str = "burin.desktop";
const MIME_TYPE: &str = "application/x-burin";

/// The program as the desktop should start it: an AppImage says where its own file is.
pub fn program_file() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

pub fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// A path as a desktop file's `Exec` line wants it quoted.
pub fn quoted(path: &Path) -> String {
    let mut out = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

pub fn desktop_text(program: &Path, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Burin\nComment=Design, model and cut for CNC carving\nExec={} %f\nIcon={}\nCategories=Graphics;Engineering;\nMimeType={MIME_TYPE};\nTerminal=false\n",
        quoted(program),
        icon.display()
    )
}

fn mime_text() -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  <mime-type type=\"{MIME_TYPE}\">\n    <comment>Burin project</comment>\n    <glob pattern=\"*.burin\"/>\n  </mime-type>\n</mime-info>\n")
}

fn desktop_file(data: &Path) -> PathBuf {
    data.join("applications").join(DESKTOP)
}

pub fn installed_in(data: &Path) -> bool {
    desktop_file(data).exists()
}

/// Writes the files. Telling the desktop is up to `refresh`, since it runs other programs.
pub fn install_in(data: &Path, program: &Path, icon_png: &[u8]) -> std::io::Result<()> {
    let icon = data.join("icons/hicolor/512x512/apps/burin.png");
    let mime = data.join("mime/packages/burin.xml");
    for file in [&icon, &mime, &desktop_file(data)] {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(&icon, icon_png)?;
    std::fs::write(&mime, mime_text())?;
    std::fs::write(desktop_file(data), desktop_text(program, &icon))
}

pub fn remove_from(data: &Path) {
    let _ = std::fs::remove_file(desktop_file(data));
    let _ = std::fs::remove_file(data.join("icons/hicolor/512x512/apps/burin.png"));
    let _ = std::fs::remove_file(data.join("mime/packages/burin.xml"));
}

/// Tells the desktop what changed, with whichever of its tools are here. None of them is essential:
/// most desktops notice the new files by themselves.
pub fn refresh(data: &Path) {
    let run = |program: &str, args: &[&std::ffi::OsStr]| {
        let _ = std::process::Command::new(program).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    };
    run("update-mime-database", &[data.join("mime").as_os_str()]);
    run("update-desktop-database", &[data.join("applications").as_os_str()]);
    if installed_in(data) {
        run("xdg-mime", &[std::ffi::OsStr::new("default"), std::ffi::OsStr::new(DESKTOP), std::ffi::OsStr::new(MIME_TYPE)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_spaces_stays_one_path() {
        assert_eq!(quoted(Path::new("/opt/my apps/burin")), "\"/opt/my apps/burin\"");
        assert_eq!(quoted(Path::new("/a\"b$c")), "\"/a\\\"b\\$c\"");
        let text = desktop_text(Path::new("/opt/my apps/burin"), Path::new("/i.png"));
        assert!(text.contains("Exec=\"/opt/my apps/burin\" %f\n") && text.contains("Icon=/i.png\n") && text.contains("MimeType=application/x-burin;\n"), "{text}");
    }

    #[test]
    fn it_goes_in_and_comes_out_again() {
        let data = std::env::temp_dir().join(format!("burin-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        assert!(!installed_in(&data));
        install_in(&data, Path::new("/opt/burin"), b"png").unwrap();
        assert!(installed_in(&data));
        assert_eq!(std::fs::read(data.join("icons/hicolor/512x512/apps/burin.png")).unwrap(), b"png");
        assert!(std::fs::read_to_string(data.join("mime/packages/burin.xml")).unwrap().contains("*.burin"));
        remove_from(&data);
        assert!(!installed_in(&data));
        let _ = std::fs::remove_dir_all(&data);
    }
}
