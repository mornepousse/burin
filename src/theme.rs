//! The look of the program: quiet graphite panels that stay out of the way of the work, soft
//! rounded controls, the orange of the logo only for what acts or is chosen, and a pale sheet to draw on.
//! The panels take the logo's grounds: deep slate in the dark look, cream in the light one.

use egui::{Color32, CornerRadius, FontId, Stroke};
use std::sync::atomic::{AtomicBool, Ordering};

/// The light "workshop" look instead of the dark one: warm paper panels, darker family colours.
/// Burin's own, chosen in the preferences.
static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Changes the look; `setup_style` is to be called again for the controls to follow.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

/// The look chosen last time, kept with the other settings.
pub fn load_look() {
    if cfg!(test) {
        return;
    }
    let chosen = crate::app::settings_file("look.txt").and_then(|f| crate::platform::read_to_string(f).ok());
    set_light(chosen.is_some_and(|t| t.trim() == "light"));
}

pub fn save_look() {
    if cfg!(test) {
        return;
    }
    if let Some(file) = crate::app::settings_file("look.txt") {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = crate::platform::write(file, if light() { "light" } else { "dark" });
    }
}

/// Menu and status bars, rulers.
#[allow(non_snake_case)]
pub fn CHROME() -> Color32 {
    if light() { Color32::from_rgb(236, 230, 218) } else { Color32::from_rgb(15, 19, 25) }
}
/// Side panels.
#[allow(non_snake_case)]
pub fn PANEL() -> Color32 {
    if light() { Color32::from_rgb(245, 240, 231) } else { Color32::from_rgb(27, 32, 41) }
}
/// Grouped controls on a panel.
#[allow(non_snake_case)]
pub fn CARD() -> Color32 {
    if light() { Color32::from_rgb(251, 247, 239) } else { Color32::from_rgb(36, 42, 54) }
}
/// Buttons.
#[allow(non_snake_case)]
pub fn BUTTON() -> Color32 {
    if light() { Color32::from_rgb(230, 222, 207) } else { Color32::from_rgb(48, 56, 71) }
}
#[allow(non_snake_case)]
pub fn BUTTON_HOVER() -> Color32 {
    if light() { Color32::from_rgb(218, 207, 188) } else { Color32::from_rgb(63, 72, 91) }
}
/// Number boxes and lists.
#[allow(non_snake_case)]
pub fn INPUT() -> Color32 {
    if light() { Color32::from_rgb(255, 253, 248) } else { Color32::from_rgb(12, 15, 20) }
}
#[allow(non_snake_case)]
pub fn LINE() -> Color32 {
    if light() { Color32::from_rgb(212, 201, 183) } else { Color32::from_rgb(56, 64, 82) }
}
#[allow(non_snake_case)]
pub fn TEXT() -> Color32 {
    if light() { Color32::from_rgb(23, 19, 15) } else { Color32::from_rgb(236, 238, 241) }
}
#[allow(non_snake_case)]
pub fn MUTED() -> Color32 {
    if light() { Color32::from_rgb(96, 84, 70) } else { Color32::from_rgb(166, 173, 189) }
}
#[allow(non_snake_case)]
pub fn FAINT() -> Color32 {
    if light() { Color32::from_rgb(142, 128, 112) } else { Color32::from_rgb(112, 122, 142) }
}
/// The one bright colour: what is active or chosen. The orange of the logo's block, exactly.
pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 0);
#[allow(non_snake_case)]
pub fn ACCENT_SOFT() -> Color32 {
    if light() { Color32::from_rgb(255, 226, 196) } else { Color32::from_rgb(60, 38, 20) }
}
/// For saying to be careful, and only that.
pub const WARNING: Color32 = Color32::from_rgb(245, 176, 65);
/// Around the sheet in the 2D view.
#[allow(non_snake_case)]
pub fn WORKSPACE() -> Color32 {
    if light() { Color32::from_rgb(229, 222, 208) } else { Color32::from_rgb(10, 13, 17) }
}
/// The material in the 2D view: stays light, so shapes read as ink on a sheet.
pub const SHEET: Color32 = Color32::from_rgb(249, 248, 245);
pub const SHEET_INK: Color32 = Color32::from_rgb(24, 26, 30);
/// Text on a button in the bright colour.
pub const ON_ACCENT: Color32 = Color32::from_rgb(43, 22, 9);
/// What is selected on the sheet.
pub const PICK: Color32 = Color32::from_rgb(222, 38, 120);
/// Cutting and fast moves over the 3D view.
pub const CUT: Color32 = Color32::from_rgb(92, 200, 255);
pub const RAPID: Color32 = Color32::from_rgb(255, 112, 92);
/// Going straight down into the material, and coming straight back up, as the original tells them apart.
pub const PLUNGE: Color32 = Color32::from_rgb(0, 220, 220);
pub const RETRACT: Color32 = Color32::from_rgb(60, 205, 90);
/// Joins: purple, as in the original, made lighter to show on the dark view.
/// The rulers while the bottom side of a two-sided job is worked on: the original's default,
/// `0x64C2ED` as Windows keeps colours, blue first.
pub const BOTTOM_RULER: Color32 = Color32::from_rgb((BOTTOM_RULER_VALUE & 0xff) as u8, ((BOTTOM_RULER_VALUE >> 8) & 0xff) as u8, ((BOTTOM_RULER_VALUE >> 16) & 0xff) as u8);
/// The same colour as the original keeps it, a Windows colour value (red in the low byte).
pub const BOTTOM_RULER_VALUE: u32 = 6603501;
pub const JOIN: Color32 = Color32::from_rgb(190, 100, 230);
#[allow(non_snake_case)]
pub fn BACKDROP_TOP() -> Color32 {
    if light() { Color32::from_rgb(240, 234, 223) } else { Color32::from_rgb(40, 46, 58) }
}
#[allow(non_snake_case)]
pub fn BACKDROP_BOTTOM() -> Color32 {
    if light() { Color32::from_rgb(210, 200, 183) } else { Color32::from_rgb(11, 14, 18) }
}
/// Guide lines that are never cut.
/// The rest of a piece, and the frame around it, when one of its shapes is selected.
pub const PIECE: Color32 = Color32::from_rgb(224, 74, 0);
pub const CONSTRUCTION: Color32 = Color32::from_rgb(104, 118, 150);
pub const GRID_FINE: Color32 = Color32::from_rgb(226, 224, 217);
pub const GRID_BOLD: Color32 = Color32::from_rgb(194, 192, 184);
#[allow(non_snake_case)]
pub fn GRID_FINE_DARK() -> Color32 {
    if light() { Color32::from_rgb(228, 220, 206) } else { Color32::from_rgb(27, 32, 41) }
}
#[allow(non_snake_case)]
pub fn GRID_BOLD_DARK() -> Color32 {
    if light() { Color32::from_rgb(208, 197, 179) } else { Color32::from_rgb(40, 46, 58) }
}
pub const ERROR: Color32 = Color32::from_rgb(255, 136, 120);

/// A colour for each family of tools, so the panels aren't all one colour on grey: the shapes take
/// the logo's orange, sketching and 3D the gold of its top face, moving and layers are blue, editing
/// teal, repeating violet, machine files green. Burin's own.
#[allow(non_snake_case)]
pub fn HUE_ORANGE() -> Color32 {
    if light() { Color32::from_rgb(214, 88, 0) } else { Color32::from_rgb(255, 138, 40) }
}
#[allow(non_snake_case)]
pub fn HUE_BLUE() -> Color32 {
    if light() { Color32::from_rgb(46, 104, 196) } else { Color32::from_rgb(112, 168, 250) }
}
#[allow(non_snake_case)]
pub fn HUE_TEAL() -> Color32 {
    if light() { Color32::from_rgb(12, 140, 122) } else { Color32::from_rgb(70, 205, 190) }
}
#[allow(non_snake_case)]
pub fn HUE_VIOLET() -> Color32 {
    if light() { Color32::from_rgb(122, 82, 206) } else { Color32::from_rgb(180, 142, 250) }
}
#[allow(non_snake_case)]
pub fn HUE_AMBER() -> Color32 {
    if light() { Color32::from_rgb(168, 106, 0) } else { Color32::from_rgb(255, 190, 40) }
}
#[allow(non_snake_case)]
pub fn HUE_GREEN() -> Color32 {
    if light() { Color32::from_rgb(52, 140, 48) } else { Color32::from_rgb(126, 212, 112) }
}

/// The colour of a panel section, by its title in the language shown.
pub fn section_hue(title: &str) -> Option<Color32> {
    use crate::i18n::tr;
    [
        (tr("Shapes"), HUE_ORANGE()),
        (tr("Move and Size"), HUE_BLUE()),
        (tr("Edit Shapes"), HUE_TEAL()),
        (tr("Offset and Repeat"), HUE_VIOLET()),
        (tr("Repeat"), HUE_VIOLET()),
        (tr("Sketch"), HUE_AMBER()),
        (tr("3D Shapes"), HUE_AMBER()),
        (tr("New Toolpath"), HUE_ORANGE()),
    ]
    .into_iter()
    .find(|(t, _)| *t == title)
    .map(|(_, c)| c)
}

/// A colour seen through the dark panels: for the soft ground under what's chosen.
pub fn hue_soft(c: Color32) -> Color32 {
    let ground = PANEL();
    let share = if light() { 0.18 } else { 0.22 };
    let mix = |v: u8, g: u8| (v as f32 * share + g as f32 * (1.0 - share)) as u8;
    Color32::from_rgb(mix(c.r(), ground.r()), mix(c.g(), ground.g()), mix(c.b(), ground.b()))
}

pub fn setup_style(ctx: &egui::Context) {
    // A press counts as a click however long the button stays down, as long as it doesn't move.
    ctx.options_mut(|o| o.input_options.max_click_duration = 3600.0);
    // The look chosen in the preferences, whatever the computer is set to.
    if light() {
        ctx.set_theme(egui::ThemePreference::Light);
        ctx.set_visuals(egui::Visuals::light());
    } else {
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.set_visuals(egui::Visuals::dark());
    }
    ctx.global_style_mut(|s| {
        use egui::{FontFamily::Proportional, TextStyle::*};
        s.text_styles.insert(Body, FontId::new(13.0, Proportional));
        s.text_styles.insert(Button, FontId::new(13.0, Proportional));
        s.text_styles.insert(Small, FontId::new(11.0, Proportional));
        s.text_styles.insert(Heading, FontId::new(15.0, Proportional));
        s.spacing.item_spacing = egui::vec2(6.0, 5.0);
        s.spacing.button_padding = egui::vec2(10.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        let v = &mut s.visuals;
        v.override_text_color = Some(TEXT());
        v.panel_fill = PANEL();
        v.window_fill = CARD();
        v.window_stroke = Stroke::new(1.0, LINE());
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = INPUT();
        v.faint_bg_color = CARD();
        v.selection.bg_fill = ACCENT_SOFT();
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let round = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE());
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT());
        w.noninteractive.corner_radius = round;
        w.inactive.weak_bg_fill = BUTTON();
        // Checkboxes and radio buttons use this one, so they stand out from the card behind them.
        w.inactive.bg_fill = if light() { Color32::from_rgb(196, 188, 175) } else { Color32::from_rgb(78, 85, 96) };
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT());
        w.inactive.corner_radius = round;
        w.hovered.weak_bg_fill = BUTTON_HOVER();
        w.hovered.bg_fill = BUTTON_HOVER();
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED());
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT());
        w.hovered.corner_radius = round;
        w.active.weak_bg_fill = ACCENT_SOFT();
        w.active.bg_fill = ACCENT_SOFT();
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5, ACCENT);
        w.active.corner_radius = round;
        w.open.weak_bg_fill = BUTTON_HOVER();
        w.open.bg_fill = BUTTON_HOVER();
        w.open.corner_radius = round;
    });
}
