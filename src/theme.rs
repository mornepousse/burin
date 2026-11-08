//! The look of the program: quiet graphite panels that stay out of the way of the work, soft
//! rounded controls, the green of the logo only for what acts or is chosen, and a pale sheet to draw on.

use egui::{Color32, CornerRadius, FontId, Stroke};

/// Menu and status bars, rulers.
pub const CHROME: Color32 = Color32::from_rgb(18, 20, 24);
/// Side panels.
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 32);
/// Grouped controls on a panel.
pub const CARD: Color32 = Color32::from_rgb(32, 36, 42);
/// Buttons.
pub const BUTTON: Color32 = Color32::from_rgb(44, 49, 57);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(58, 64, 74);
/// Number boxes and lists.
pub const INPUT: Color32 = Color32::from_rgb(16, 18, 22);
pub const LINE: Color32 = Color32::from_rgb(50, 55, 63);
pub const TEXT: Color32 = Color32::from_rgb(236, 238, 241);
pub const MUTED: Color32 = Color32::from_rgb(165, 171, 181);
pub const FAINT: Color32 = Color32::from_rgb(115, 122, 133);
/// The one bright colour: what is active or chosen. The background of the logo, exactly.
pub const ACCENT: Color32 = Color32::from_rgb(19, 166, 127);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(16, 46, 40);
/// For saying to be careful, and only that.
pub const WARNING: Color32 = Color32::from_rgb(245, 176, 65);
/// Around the sheet in the 2D view.
pub const WORKSPACE: Color32 = Color32::from_rgb(13, 15, 18);
/// The material in the 2D view: stays light, so shapes read as ink on a sheet.
pub const SHEET: Color32 = Color32::from_rgb(249, 248, 245);
pub const SHEET_INK: Color32 = Color32::from_rgb(24, 26, 30);
/// Text on a button in the bright colour.
pub const ON_ACCENT: Color32 = Color32::from_rgb(4, 28, 22);
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
pub const BACKDROP_TOP: Color32 = Color32::from_rgb(38, 42, 49);
pub const BACKDROP_BOTTOM: Color32 = Color32::from_rgb(13, 15, 18);
/// Guide lines that are never cut.
/// The rest of a piece, and the frame around it, when one of its shapes is selected.
pub const PIECE: Color32 = Color32::from_rgb(10, 125, 97);
pub const CONSTRUCTION: Color32 = Color32::from_rgb(104, 118, 150);
pub const GRID_FINE: Color32 = Color32::from_rgb(226, 224, 217);
pub const GRID_BOLD: Color32 = Color32::from_rgb(194, 192, 184);
pub const GRID_FINE_DARK: Color32 = Color32::from_rgb(24, 27, 32);
pub const GRID_BOLD_DARK: Color32 = Color32::from_rgb(38, 42, 49);
pub const ERROR: Color32 = Color32::from_rgb(255, 136, 120);

pub fn setup_style(ctx: &egui::Context) {
    // A press counts as a click however long the button stays down, as long as it doesn't move.
    ctx.options_mut(|o| o.input_options.max_click_duration = 3600.0);
    // Always this look, even where the computer is set to a light theme: nothing else is made for one.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals(egui::Visuals::dark());
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
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = INPUT;
        v.faint_bg_color = CARD;
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let round = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = round;
        w.inactive.weak_bg_fill = BUTTON;
        // Checkboxes and radio buttons use this one, so they stand out from the card behind them.
        w.inactive.bg_fill = Color32::from_rgb(78, 85, 96);
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = round;
        w.hovered.weak_bg_fill = BUTTON_HOVER;
        w.hovered.bg_fill = BUTTON_HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.corner_radius = round;
        w.active.weak_bg_fill = ACCENT_SOFT;
        w.active.bg_fill = ACCENT_SOFT;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.5, ACCENT);
        w.active.corner_radius = round;
        w.open.weak_bg_fill = BUTTON_HOVER;
        w.open.bg_fill = BUTTON_HOVER;
        w.open.corner_radius = round;
    });
}
