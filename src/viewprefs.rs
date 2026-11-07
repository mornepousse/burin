//! The 3D view's choices that are kept from one session to the next, as the original keeps them under
//! its application defaults (`0x140272410`): `3dViewDrawOrigin` and `3dViewDrawMaterialBlock`, both off at
//! first, and `3dViewShadedBackgroundStyle`, on at first (`0x140272d42`). Light Follows Viewer isn't
//! among them: the original starts each session with it off.

use crate::app::settings_file;

const FILE: &str = "view3d.txt";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPrefs {
    pub draw_origin: bool,
    pub draw_block: bool,
    pub shaded_background: bool,
}

impl Default for ViewPrefs {
    fn default() -> Self {
        ViewPrefs { draw_origin: false, draw_block: false, shaded_background: true }
    }
}

pub fn from_text(text: &str) -> ViewPrefs {
    let mut p = ViewPrefs::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let on = value.trim() == "yes";
        match key.trim() {
            "draw_origin" => p.draw_origin = on,
            "draw_block" => p.draw_block = on,
            "shaded_background" => p.shaded_background = on,
            _ => {}
        }
    }
    p
}

pub fn to_text(p: ViewPrefs) -> String {
    let word = |b: bool| if b { "yes" } else { "no" };
    format!("draw_origin={}\ndraw_block={}\nshaded_background={}\n", word(p.draw_origin), word(p.draw_block), word(p.shaded_background))
}

pub fn load() -> ViewPrefs {
    if cfg!(test) {
        return ViewPrefs::default();
    }
    settings_file(FILE).and_then(|f| crate::platform::read_to_string(f).ok()).map_or_else(ViewPrefs::default, |t| from_text(&t))
}

pub fn save(p: ViewPrefs) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = settings_file(FILE) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::platform::write(file, to_text(p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_choices_are_written_and_read_back() {
        let p = ViewPrefs { draw_origin: true, draw_block: false, shaded_background: false };
        assert_eq!(from_text(&to_text(p)), p);
        assert_eq!(from_text(""), ViewPrefs::default(), "as the original starts: no origin, no block, shaded");
        assert_eq!(from_text("draw_block=yes\nsomething=else"), ViewPrefs { draw_block: true, ..ViewPrefs::default() });
    }
}
