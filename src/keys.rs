//! The keyboard shortcuts, which can each be changed, and the file that keeps them with the language.

use crate::i18n::{self, tr, Lang};
use egui::{Event, InputState, Key};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

const fn plain(key: Key) -> Shortcut {
    Shortcut { ctrl: false, shift: false, alt: false, key }
}
const fn shift(key: Key) -> Shortcut {
    Shortcut { ctrl: false, shift: true, alt: false, key }
}
const fn ctrl(key: Key) -> Shortcut {
    Shortcut { ctrl: true, shift: false, alt: false, key }
}
const fn ctrl_shift(key: Key) -> Shortcut {
    Shortcut { ctrl: true, shift: true, alt: false, key }
}

impl Shortcut {
    /// As it's shown, in the current language.
    pub fn text(self) -> String {
        let mut text = String::new();
        if self.ctrl {
            text.push_str("Ctrl+");
        }
        if self.shift {
            text.push_str(tr("Shift"));
            text.push('+');
        }
        if self.alt {
            text.push_str("Alt+");
        }
        text.push_str(match self.key {
            Key::Delete => tr("Delete"),
            Key::Backspace => tr("Backspace"),
            key => key.name(),
        });
        text
    }

    /// As it's kept in the file, the same in any language.
    fn saved(self) -> String {
        format!("{}{}{}{}", if self.ctrl { "ctrl+" } else { "" }, if self.shift { "shift+" } else { "" }, if self.alt { "alt+" } else { "" }, self.key.name())
    }

    fn from_saved(text: &str) -> Option<Shortcut> {
        let mut s = plain(Key::A);
        let mut rest = text;
        loop {
            if let Some(after) = rest.strip_prefix("ctrl+") {
                s.ctrl = true;
                rest = after;
            } else if let Some(after) = rest.strip_prefix("shift+") {
                s.shift = true;
                rest = after;
            } else if let Some(after) = rest.strip_prefix("alt+") {
                s.alt = true;
                rest = after;
            } else {
                break;
            }
        }
        s.key = Key::from_name(rest)?;
        (!reserved(s.key)).then_some(s)
    }

    /// Whether this was pressed in this frame. The windowing library turns the usual clipboard keys
    /// into their own events, so those count as the keys they came from.
    pub fn fired(self, input: &InputState) -> bool {
        input.events.iter().any(|event| match event {
            Event::Key { key, pressed: true, modifiers, .. } => {
                *key == self.key && modifiers.command == self.ctrl && modifiers.shift == self.shift && modifiers.alt == self.alt
            }
            Event::Copy => self == clipboard_key(input, Key::C),
            Event::Cut => self == clipboard_key(input, Key::X),
            Event::Paste(_) => self == clipboard_key(input, Key::V),
            _ => false,
        })
    }

    /// The first key pressed in this frame, for choosing a new one.
    pub fn pressed_now(input: &InputState) -> Option<Shortcut> {
        input.events.iter().find_map(|event| match event {
            Event::Key { key, pressed: true, repeat: false, modifiers, .. } => {
                Some(Shortcut { ctrl: modifiers.command, shift: modifiers.shift, alt: modifiers.alt, key: *key })
            }
            Event::Copy => Some(clipboard_key(input, Key::C)),
            Event::Cut => Some(clipboard_key(input, Key::X)),
            Event::Paste(_) => Some(clipboard_key(input, Key::V)),
            _ => None,
        })
    }
}

/// The key a clipboard event came from. It doesn't say about Shift, which is held or not at the time.
fn clipboard_key(input: &InputState, key: Key) -> Shortcut {
    if input.modifiers.shift { ctrl_shift(key) } else { ctrl(key) }
}

/// Keys with a use of their own: drawing a line, moving the selection, leaving a tool, moving between boxes.
pub fn reserved(key: Key) -> bool {
    matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Escape | Key::Enter | Key::Space | Key::Tab)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    RedoAgain,
    Cut,
    Copy,
    Paste,
    Delete,
    DeleteAgain,
    SelectAll,
    SelectSimilar,
    Group,
    Ungroup,
    UngroupAll,
    UngroupStay,
    UngroupAllStay,
    MirrorX,
    MirrorXJob,
    MirrorXCopy,
    MirrorXJobCopy,
    MirrorY,
    MirrorYJob,
    MirrorYCopy,
    MirrorYJobCopy,
    TurnLeft,
    TurnRight,
    Import,
    ZoomFit,
    ZoomFitAgain,
    ZoomBox,
    View2d,
    View3d,
    TileSideBySide,
    TileStacked,
    TopSide,
    BottomSide,
    MultiSided,
    DrawingOrModeling,
    DrawingTab,
    LayersTab,
    LeftPanel,
    RightPanel,
    NodesOrSelect,
    MoveForm,
    SizeForm,
    RotateForm,
    JoinForm,
    AlignForm,
    CentreOnMaterial,
    Measure,
    IncrementSave,
    Refresh,
    DrawLines,
    DrawCircle,
    DrawEllipse,
    DrawRect,
    DrawPolygon,
    DrawStar,
    DrawArc,
    DrawCurve,
    DrawSpiral,
    DrawText,
    MakePiece,
    SelectPiece,
    AdjustPiece,
    ReleasePiece,
}

const COUNT: usize = 69;

/// Each action: its name in the file, what it's called, and its key to begin with.
const ACTIONS: [(Action, &str, &str, Option<Shortcut>); COUNT] = [
    (Action::New, "new", "New", Some(ctrl(Key::N))),
    (Action::Open, "open", "Open", Some(ctrl(Key::O))),
    (Action::Save, "save", "Save", Some(ctrl(Key::S))),
    (Action::SaveAs, "save_as", "Save as", Some(ctrl_shift(Key::S))),
    (Action::Undo, "undo", "Undo", Some(ctrl(Key::Z))),
    (Action::Redo, "redo", "Redo", Some(ctrl(Key::Y))),
    (Action::RedoAgain, "redo_again", "Redo (second key)", Some(ctrl_shift(Key::Z))),
    (Action::Cut, "cut", "Cut", Some(ctrl(Key::X))),
    (Action::Copy, "copy", "Copy", Some(ctrl(Key::C))),
    (Action::Paste, "paste", "Paste", Some(ctrl(Key::V))),
    (Action::Delete, "delete", "Delete", Some(plain(Key::Delete))),
    (Action::DeleteAgain, "delete_again", "Delete (second key)", Some(plain(Key::Backspace))),
    (Action::SelectAll, "select_all", "Select all", Some(ctrl(Key::A))),
    (Action::SelectSimilar, "select_similar", "Select similar", Some(ctrl_shift(Key::A))),
    (Action::Group, "group", "Group", Some(plain(Key::G))),
    (Action::Ungroup, "ungroup", "Ungroup", Some(plain(Key::U))),
    (Action::UngroupAll, "ungroup_all", "Ungroup all levels", Some(shift(Key::U))),
    (Action::UngroupStay, "ungroup_stay", "Ungroup, staying on the group's layer", Some(ctrl(Key::U))),
    (Action::UngroupAllStay, "ungroup_all_stay", "Ungroup all levels, staying on the group's layer", Some(ctrl_shift(Key::U))),
    (Action::MirrorX, "mirror_x", "Mirror left to right", Some(plain(Key::H))),
    (Action::MirrorXJob, "mirror_x_job", "Mirror left to right, about the job's centre", Some(shift(Key::H))),
    (Action::MirrorXCopy, "mirror_x_copy", "Mirrored copy, left to right", Some(ctrl(Key::H))),
    (Action::MirrorXJobCopy, "mirror_x_job_copy", "Mirrored copy, left to right, about the job's centre", Some(ctrl_shift(Key::H))),
    (Action::MirrorY, "mirror_y", "Mirror top to bottom", Some(plain(Key::V))),
    (Action::MirrorYJob, "mirror_y_job", "Mirror top to bottom, about the job's centre", Some(shift(Key::V))),
    // Ctrl+V pastes, so this one starts with no key.
    (Action::MirrorYCopy, "mirror_y_copy", "Mirrored copy, top to bottom", None),
    (Action::MirrorYJobCopy, "mirror_y_job_copy", "Mirrored copy, top to bottom, about the job's centre", Some(ctrl_shift(Key::V))),
    (Action::TurnLeft, "turn_left", "Turn 45 degrees anticlockwise", Some(plain(Key::Num9))),
    (Action::TurnRight, "turn_right", "Turn 45 degrees clockwise", Some(plain(Key::Num0))),
    (Action::Import, "import", "Import shapes", Some(ctrl(Key::I))),
    (Action::ZoomFit, "zoom_fit", "Zoom to job", Some(plain(Key::F))),
    (Action::ZoomFitAgain, "zoom_fit_again", "Zoom to job (second key)", Some(plain(Key::F6))),
    (Action::ZoomBox, "zoom_box", "Zoom to box", Some(plain(Key::Z))),
    (Action::View2d, "view_2d", "2D View", Some(plain(Key::F2))),
    (Action::View3d, "view_3d", "3D View", Some(plain(Key::F3))),
    // `0x1403ba694`: Page Down sends Tile Horizontal (57651), Page Up Tile Vertical (57652).
    (Action::TileSideBySide, "tile_side_by_side", "2D and 3D side by side", Some(plain(Key::PageUp))),
    (Action::TileStacked, "tile_stacked", "2D and 3D one above the other", Some(plain(Key::PageDown))),
    // The original's guide to two-sided machining: 1 makes the top side active, 2 the bottom.
    (Action::TopSide, "top_side", "Top side of a two-sided job", Some(plain(Key::Num1))),
    (Action::BottomSide, "bottom_side", "Bottom side of a two-sided job", Some(plain(Key::Num2))),
    // The manual's Shortcut Keys page: "= Toggle Multi-Sided View on".
    (Action::MultiSided, "multi_sided", "Multi-Sided View", Some(plain(Key::Equals))),
    (Action::DrawingOrModeling, "drawing_or_modeling", "Drawing or Modeling tab", Some(plain(Key::F8))),
    (Action::DrawingTab, "drawing_tab", "Drawing tab", Some(ctrl(Key::D))),
    (Action::LayersTab, "layers_tab", "Layers tab", Some(ctrl(Key::L))),
    (Action::LeftPanel, "left_panel", "Show or hide the left panel", Some(plain(Key::F11))),
    (Action::RightPanel, "right_panel", "Show or hide the right panel", Some(plain(Key::F12))),
    (Action::NodesOrSelect, "nodes_or_select", "Edit points, or back to selecting", Some(plain(Key::N))),
    (Action::MoveForm, "move_form", "Move Selection", Some(plain(Key::M))),
    (Action::SizeForm, "size_form", "Set Size", Some(plain(Key::T))),
    (Action::RotateForm, "rotate_form", "Rotate", Some(plain(Key::R))),
    (Action::JoinForm, "join_form", "Join Vectors", Some(plain(Key::J))),
    (Action::AlignForm, "align_form", "Alignment Tools", Some(plain(Key::F10))),
    (Action::CentreOnMaterial, "centre_on_material", "Centre on the material", Some(plain(Key::F9))),
    (Action::Measure, "measure", "Measure distance", Some(ctrl(Key::M))),
    (Action::IncrementSave, "increment_save", "Increment and Save", Some(Shortcut { ctrl: true, shift: false, alt: true, key: Key::S })),
    (Action::Refresh, "refresh", "Refresh 2D View", Some(plain(Key::F5))),
    (Action::DrawLines, "draw_lines", "Draw lines", Some(plain(Key::L))),
    (Action::DrawCircle, "draw_circle", "Draw circle", Some(plain(Key::C))),
    (Action::DrawEllipse, "draw_ellipse", "Draw ellipse", Some(plain(Key::E))),
    (Action::DrawRect, "draw_rectangle", "Draw rectangle", Some(plain(Key::Q))),
    (Action::DrawPolygon, "draw_polygon", "Draw polygon", Some(plain(Key::P))),
    (Action::DrawStar, "draw_star", "Draw star", Some(plain(Key::I))),
    (Action::DrawArc, "draw_arc", "Draw arc", Some(plain(Key::A))),
    (Action::DrawCurve, "draw_curve", "Draw curve", Some(plain(Key::B))),
    (Action::DrawSpiral, "draw_spiral", "Draw spiral", Some(plain(Key::S))),
    (Action::DrawText, "draw_text", "Draw text", Some(plain(Key::X))),
    (Action::MakePiece, "make_piece", "Make Piece", Some(ctrl(Key::G))),
    (Action::SelectPiece, "select_piece", "Select Piece", Some(ctrl_shift(Key::G))),
    (Action::AdjustPiece, "adjust_piece", "Adjust Alone", Some(plain(Key::D))),
    (Action::ReleasePiece, "release_piece", "Release Piece", Some(Shortcut { ctrl: true, shift: false, alt: true, key: Key::G })),
];

/// The actions as the list shows them.
pub const SECTIONS: [(&str, &[Action]); 8] = [
    ("Files", &[Action::New, Action::Open, Action::Save, Action::SaveAs, Action::IncrementSave, Action::Import]),
    ("Shapes", &[Action::DrawLines, Action::DrawCircle, Action::DrawEllipse, Action::DrawRect, Action::DrawPolygon, Action::DrawStar, Action::DrawArc, Action::DrawCurve, Action::DrawSpiral, Action::DrawText]),
    ("Editing", &[Action::Undo, Action::Redo, Action::RedoAgain, Action::Cut, Action::Copy, Action::Paste, Action::Delete, Action::DeleteAgain, Action::SelectAll, Action::SelectSimilar]),
    ("Groups", &[Action::Group, Action::Ungroup, Action::UngroupAll, Action::UngroupStay, Action::UngroupAllStay]),
    ("Pieces", &[Action::MakePiece, Action::SelectPiece, Action::AdjustPiece, Action::ReleasePiece]),
    ("Mirror and turn", &[Action::MirrorX, Action::MirrorXJob, Action::MirrorXCopy, Action::MirrorXJobCopy, Action::MirrorY, Action::MirrorYJob, Action::MirrorYCopy, Action::MirrorYJobCopy, Action::TurnLeft, Action::TurnRight]),
    ("Tools", &[Action::NodesOrSelect, Action::MoveForm, Action::SizeForm, Action::RotateForm, Action::JoinForm, Action::AlignForm, Action::CentreOnMaterial, Action::Measure]),
    ("Views and panels", &[Action::View2d, Action::View3d, Action::TileSideBySide, Action::TileStacked, Action::TopSide, Action::BottomSide, Action::MultiSided, Action::Refresh, Action::ZoomFit, Action::ZoomFitAgain, Action::ZoomBox, Action::DrawingOrModeling, Action::DrawingTab, Action::LayersTab, Action::LeftPanel, Action::RightPanel]),
];

/// Every action, in the order of the list of keys.
pub fn all_actions() -> impl Iterator<Item = Action> {
    ACTIONS.iter().map(|a| a.0)
}

pub fn action_name(action: Action) -> &'static str {
    tr(ACTIONS[action as usize].2)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Keys([Option<Shortcut>; COUNT]);

impl Default for Keys {
    fn default() -> Keys {
        let mut keys = [None; COUNT];
        for (i, (action, _, _, key)) in ACTIONS.iter().enumerate() {
            debug_assert_eq!(*action as usize, i);
            keys[i] = *key;
        }
        Keys(keys)
    }
}

impl Keys {
    pub fn get(&self, action: Action) -> Option<Shortcut> {
        self.0[action as usize]
    }

    pub fn set(&mut self, action: Action, key: Option<Shortcut>) {
        self.0[action as usize] = key;
    }

    /// Its key as it's shown.
    pub fn text(&self, action: Action) -> String {
        self.get(action).map_or_else(|| tr("Not set").to_string(), Shortcut::text)
    }

    pub fn fired(&self, action: Action, input: &InputState) -> bool {
        self.get(action).is_some_and(|key| key.fired(input))
    }

    /// The other action this key already does.
    pub fn taken_by(&self, key: Shortcut, asking: Action) -> Option<Action> {
        ACTIONS.iter().map(|a| a.0).find(|a| *a != asking && self.get(*a) == Some(key))
    }

    /// Only what differs from the keys to begin with goes in the file.
    fn to_text(self) -> String {
        let mut text = String::new();
        for (i, (_, name, _, first)) in ACTIONS.iter().enumerate() {
            if self.0[i] != *first {
                text.push_str(&format!("key.{name}={}\n", self.0[i].map_or("none".to_string(), Shortcut::saved)));
            }
        }
        text
    }

    fn read_line(&mut self, name: &str, value: &str) {
        let Some(at) = ACTIONS.iter().position(|a| a.1 == name) else { return };
        if value == "none" {
            self.0[at] = None;
        } else if let Some(key) = Shortcut::from_saved(value) {
            self.0[at] = Some(key);
        }
    }
}

/// What's kept between sessions.
pub struct Preferences {
    pub language: Option<Lang>,
    pub keys: Keys,
}

fn from_text(text: &str) -> Preferences {
    let mut prefs = Preferences { language: None, keys: Keys::default() };
    for line in text.lines() {
        let Some((name, value)) = line.split_once('=') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name == "language" {
            prefs.language = Lang::from_code(value);
        } else if let Some(action) = name.strip_prefix("key.") {
            prefs.keys.read_line(action, value);
        }
    }
    prefs
}

fn to_text(language: Lang, keys: Keys) -> String {
    format!("language={}\n{}", language.code(), keys.to_text())
}

/// Reads the file, and starts in the computer's language if none has been chosen yet.
pub fn load() -> Keys {
    if cfg!(test) {
        return Keys::default();
    }
    let text = crate::app::settings_file("preferences.txt").and_then(|f| crate::platform::read_to_string(f).ok()).unwrap_or_default();
    let prefs = from_text(&text);
    i18n::set_language(prefs.language.unwrap_or_else(i18n::of_this_computer));
    prefs.keys
}

pub fn save(keys: Keys) {
    if cfg!(test) {
        return;
    }
    if let Some(file) = crate::app::settings_file("preferences.txt") {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = crate::platform::write(file, to_text(i18n::language(), keys));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_in_the_list_once() {
        let listed: Vec<Action> = SECTIONS.iter().flat_map(|s| s.1.iter().copied()).collect();
        assert_eq!(listed.len(), COUNT);
        for (i, a) in ACTIONS.iter().enumerate() {
            assert_eq!(a.0 as usize, i);
            assert!(listed.contains(&a.0));
        }
    }

    #[test]
    fn no_two_actions_start_on_the_same_key() {
        let keys = Keys::default();
        for a in ACTIONS {
            if let Some(key) = a.3 {
                assert_eq!(keys.taken_by(key, a.0), None, "{}", a.1);
                assert!(!reserved(key.key));
            }
        }
    }

    #[test]
    fn the_file_keeps_changed_keys_and_the_language() {
        let mut keys = Keys::default();
        keys.set(Action::Group, Some(ctrl_shift(Key::K)));
        keys.set(Action::Save, None);
        keys.set(Action::TurnLeft, Some(plain(Key::F5)));
        let text = to_text(Lang::French, keys);
        assert!(!text.contains("key.undo"), "only what changed: {text}");
        let read = from_text(&text);
        assert_eq!(read.language, Some(Lang::French));
        assert_eq!(read.keys, keys);
    }

    #[test]
    fn a_bad_line_in_the_file_changes_nothing() {
        let read = from_text("nonsense\nlanguage=xx\nkey.nothing=K\nkey.group=ctrl+NotAKey\nkey.save=Space\n");
        assert_eq!(read.language, None);
        assert_eq!(read.keys, Keys::default());
    }

    #[test]
    fn keys_are_written_in_the_language() {
        i18n::set_language(Lang::French);
        assert_eq!(ctrl_shift(Key::U).text(), "Ctrl+Maj+U");
        i18n::set_language(Lang::English);
        assert_eq!(ctrl_shift(Key::U).text(), "Ctrl+Shift+U");
        assert_eq!(plain(Key::Num9).text(), "9");
    }
}
