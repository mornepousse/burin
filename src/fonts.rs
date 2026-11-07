//! Fonts for text shapes: the one built into Burin, the fonts installed on the computer, and single-line
//! fonts for engraving.
//!
//! Read from the original: a text's form keeps its font's name, bold, italic, whether it is a TrueType font
//! (on at first) and how it is justified (the first choice at first) (`0x1408dcdb0`); a name starting with `@`
//! is a font for upright writing. Besides the computer's TrueType fonts it has its own kind, for single-line
//! engraving (`TrueTypeFontName` and `VectricFontName`, `0x140756b30`). A text's font is saved with its name
//! and six numbers (`txtFont`, version 3, `0x1408e3670`).
//!
//! Burin's single-line fonts are the Hershey fonts, made by Dr. A. V. Hershey at the U. S. National Bureau of
//! Standards, in the format of James Hurt, Cognition, Inc. (see `assets/fonts/HERSHEY-NOTICE.txt`).

use crate::geom::{Path, P};
use ab_glyph::{Font, FontRef, FontVec, OutlineCurve};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// How much an italic made from an upright face leans: x moves by this much for each unit up.
pub const SLANT: f64 = 0.2;
/// Lines of text are this many capital heights apart.
pub const LINE_GAP: f64 = 1.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Centre,
    Right,
}

/// How a text is written. An empty font name is Burin's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub font: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default)]
    pub align: TextAlign,
    /// How far apart the letters are, as a share of their own widths: 1 as the font has them, the original's
    /// text block spacing at first (`_txtblkSpacingValue` = 1.0, `0x1408d6cd0`).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub spacing: f64,
    /// How far apart lines are, in capital heights.
    #[serde(default = "line_gap", skip_serializing_if = "is_line_gap")]
    pub line_spacing: f64,
}

fn one() -> f64 {
    1.0
}
fn is_one(v: &f64) -> bool {
    *v == 1.0
}
fn line_gap() -> f64 {
    LINE_GAP
}
fn is_line_gap(v: &f64) -> bool {
    *v == LINE_GAP
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font: String::new(), bold: false, italic: false, align: TextAlign::Left, spacing: 1.0, line_spacing: LINE_GAP }
    }
}

impl TextStyle {
    pub fn is_default(&self) -> bool {
        *self == TextStyle::default()
    }
}

/// The single-line fonts: their names, and the drawings for upright and bold.
pub const SINGLE_LINE: [(&str, &str, &str); 2] = [
    ("Single Line Sans", include_str!("../assets/fonts/futural.jhf"), include_str!("../assets/fonts/futuram.jhf")),
    ("Single Line Script", include_str!("../assets/fonts/scripts.jhf"), include_str!("../assets/fonts/scripts.jhf")),
];

/// Whether this font draws each letter as lines for the tool to follow, not outlines.
pub fn is_single_line(font: &str) -> bool {
    SINGLE_LINE.iter().any(|(name, ..)| *name == font)
}

/// What the font list says for a font's name.
pub fn shown_name(font: &str) -> String {
    if font.is_empty() {
        crate::i18n::tr("Burin (built in)").to_string()
    } else if is_single_line(font) {
        tf!("{} (single line)", font)
    } else {
        font.to_string()
    }
}

/// The font, bold, italic and alignment of a text. True if any changed.
pub fn style_picker(ui: &mut egui::Ui, style: &mut TextStyle) -> bool {
    let before = style.clone();
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Font:"));
        // A screen of the known states can ask for the list to be open.
        if let Some(filter) = ui.ctx().data_mut(|d| d.remove_temp::<String>(egui::Id::unique(OPEN_FONT_LIST))) {
            let button = ui.make_persistent_id(egui::IdSalt::new("text-font"));
            egui::Popup::open_id(ui.ctx(), button.with("popup"));
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("text-font-filter"), filter));
        }
        egui::ComboBox::from_id_salt("text-font").width(150.0).selected_text(shown_name(&style.font)).height(320.0).show_ui(ui, |ui| {
            let filter_id = egui::Id::unique("text-font-filter");
            let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(crate::i18n::tr("Search")).desired_width(160.0));
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            let wanted = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase());
            font_row(ui, &mut style.font, "");
            for (name, ..) in SINGLE_LINE {
                if wanted(name) {
                    font_row(ui, &mut style.font, name);
                }
            }
            let installed = families();
            if !installed.is_empty() {
                ui.separator();
            }
            for family in installed.iter().filter(|f| wanted(&f.name)) {
                font_row(ui, &mut style.font, &family.name);
            }
        });
    });
    ui.horizontal(|ui| {
        ui.toggle_value(&mut style.bold, crate::i18n::tr("Bold"));
        ui.toggle_value(&mut style.italic, egui::RichText::new(crate::i18n::tr("Italic")).italics());
    });
    ui.horizontal(|ui| {
        ui.selectable_value(&mut style.align, TextAlign::Left, crate::i18n::tr("Left"));
        ui.selectable_value(&mut style.align, TextAlign::Centre, crate::i18n::tr("Centre"));
        ui.selectable_value(&mut style.align, TextAlign::Right, crate::i18n::tr("Right"));
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Letters:"));
        let mut percent = style.spacing * 100.0;
        if crate::ui::number(ui, &mut percent, 40.0) {
            style.spacing = (percent / 100.0).clamp(0.1, 10.0);
        }
        ui.label("%");
        ui.add_space(4.0);
        ui.label(crate::i18n::tr("Lines:"));
        if crate::ui::number(ui, &mut style.line_spacing, 36.0) {
            style.line_spacing = style.line_spacing.clamp(0.1, 20.0);
        }
    })
    .response
    .on_hover_text(crate::i18n::tr("How far apart the letters are, as a share of their widths, and the lines, in capital heights."));
    if is_single_line(&style.font) {
        ui.label(egui::RichText::new(crate::i18n::tr("Each letter is lines, not outlines: engrave along them, with the tool on the line.")).size(12.0).color(crate::theme::MUTED));
    } else if !style.font.is_empty() && !families().iter().any(|f| f.name == style.font) {
        ui.label(egui::RichText::new(crate::i18n::tr("This computer doesn't have this font: Burin's own is used.")).size(12.0).color(crate::theme::WARNING));
    }
    *style != before
}

/// Set in the context's data to open the font list at the next frame.
pub const OPEN_FONT_LIST: &str = "open-font-list";

/// How tall a font's name is drawn in the list, in points.
pub const PREVIEW_SIZE: f32 = 18.0;
/// The widest a name's picture gets, in points.
const PREVIEW_WIDTH: f32 = 230.0;

/// One line of the font list: the font's name written in that font, so it shows what it looks like.
fn font_row(ui: &mut egui::Ui, chosen: &mut String, font: &str) {
    let height = PREVIEW_SIZE + 8.0;
    let width = ui.available_width().max(PREVIEW_WIDTH + 12.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let response = response.on_hover_text(shown_name(font));
    // What a screen reader says, and what the tests look for.
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *chosen == font, shown_name(font)));
    if response.clicked() {
        *chosen = font.to_string();
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    let visuals = ui.style().interact_selectable(&response, *chosen == font);
    if *chosen == font || response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    let colour = visuals.text_color();
    let left = rect.left() + 6.0;
    if let Some((_, upright, _)) = SINGLE_LINE.iter().find(|(name, ..)| *name == font) {
        // Lines, as the tool will follow them.
        let glyphs = single_glyphs(upright);
        let base = rect.center().y + PREVIEW_SIZE * 0.4;
        for path in single_line(font, f64::from(PREVIEW_SIZE * 0.7), [0.0, 0.0], glyphs, false, &TextStyle::default()) {
            let pts: Vec<egui::Pos2> = path.pts.iter().map(|p| egui::pos2(left + p[0] as f32, base - p[1] as f32)).filter(|p| p.x < rect.right() - 4.0).collect();
            ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.2, colour)));
        }
        return;
    }
    let texture = if font.is_empty() { None } else { preview_texture(ui.ctx(), font) };
    match texture {
        Some((t, symbol)) => {
            let mut left = left;
            if symbol {
                // The name plainly, then the font's own pictures 5 points after it.
                let name = ui.painter().text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, shown_name(font), egui::FontId::proportional(14.0), colour);
                left = name.right() + 5.0;
            }
            let size = t.size_vec2() / ui.ctx().pixels_per_point();
            let shown = egui::vec2(size.x.min((rect.right() - 6.0 - left).max(0.0)), size.y);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(shown.x / size.x.max(1.0), 1.0));
            let at = egui::Rect::from_min_size(egui::pos2(left, rect.center().y - shown.y / 2.0), shown);
            ui.painter().image(t.id(), at, uv, colour);
        }
        None => {
            ui.painter().text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, shown_name(font), egui::FontId::proportional(14.0), colour);
        }
    }
}

/// The single-line letters of a font's drawing, read once.
fn single_glyphs(src: &'static str) -> &'static [Stroked] {
    static READ: OnceLock<Mutex<HashMap<usize, &'static [Stroked]>>> = OnceLock::new();
    let key = src.as_ptr() as usize;
    let map = READ.get_or_init(Default::default);
    let mut map = map.lock().unwrap();
    map.entry(key).or_insert_with(|| Box::leak(parse_jhf(src).into_boxed_slice()))
}

/// The picture of a font's name in that font, made the first time its line shows and kept. None when the
/// font can't be read or has no letters for its own name (a font of symbols): the name is written plainly then.
/// The second part says it is a font of symbols: its name is then written plainly, with the picture after it,
/// as the original does for those (`0x140a04a90`).
type Preview = Option<(egui::TextureHandle, bool)>;

fn preview_texture(ctx: &egui::Context, font: &str) -> Preview {
    let id = egui::Id::unique("font-previews");
    let cached: Option<Preview> = ctx.data_mut(|d| d.get_temp::<HashMap<String, Preview>>(id).and_then(|m| m.get(font).cloned()));
    if let Some(t) = cached {
        return t;
    }
    let family = families().iter().find(|f| f.name == font)?;
    // Regular weight, as the original asks for each one (400, `0x140a004f0`).
    let loaded = pick_face(family, false, false).and_then(|(face, _)| {
        // Read for the picture only, not kept: going down the whole list mustn't keep every font in memory.
        let bytes = crate::platform::read(&face.path).ok()?;
        FontVec::try_from_vec_and_index(bytes, face.index).ok()
    });
    let (px, widest) = (PREVIEW_SIZE * ctx.pixels_per_point(), PREVIEW_WIDTH * ctx.pixels_per_point());
    let made = loaded.and_then(|f| {
        match name_picture(&f, font, px, widest) {
            Some(image) => Some((image, false)),
            // Symbol fonts keep their pictures where Windows looks for them: from U+F000 on.
            None => name_picture(&f, &symbol_text(font), px, widest * 0.5).map(|image| (image, true)),
        }
    });
    let made = made.map(|(image, symbol)| (ctx.load_texture(format!("font-preview-{font}"), image, egui::TextureOptions::LINEAR), symbol));
    ctx.data_mut(|d| {
        let mut map: HashMap<String, Preview> = d.get_temp(id).unwrap_or_default();
        map.insert(font.to_string(), made.clone());
        d.insert_temp(id, map);
    });
    made
}

/// The letters of `text` moved to where a font of symbols keeps them.
pub fn symbol_text(text: &str) -> String {
    text.chars().filter_map(|c| if c.is_ascii() { char::from_u32(0xF000 + c as u32) } else { Some(c) }).collect()
}

/// `text` written in `font`, `px` pixels from the top of its tallest letter to the bottom of its lowest,
/// as white with the letters' coverage for its transparency, cut at `widest` pixels.
pub fn name_picture(font: &impl Font, text: &str, px: f32, widest: f32) -> Option<egui::ColorImage> {
    use ab_glyph::ScaleFont;
    let missing = text.chars().filter(|c| !c.is_whitespace() && font.glyph_id(*c).0 == 0).count();
    if missing * 2 > text.chars().filter(|c| !c.is_whitespace()).count() {
        return None;
    }
    let scaled = font.as_scaled(ab_glyph::PxScale::from(px));
    let (ascent, descent) = (scaled.ascent(), scaled.descent());
    let mut caret = 0.0f32;
    let mut glyphs = Vec::new();
    let mut previous = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = previous {
            caret += scaled.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(px, ab_glyph::point(caret, ascent)));
        caret += scaled.h_advance(id);
        previous = Some(id);
        if caret > widest {
            break;
        }
    }
    let (w, h) = (caret.min(widest).ceil().max(1.0) as usize, (ascent - descent).ceil().max(1.0) as usize);
    let mut alpha = vec![0f32; w * h];
    for glyph in glyphs {
        let Some(outlined) = font.outline_glyph(glyph) else { continue };
        let b = outlined.px_bounds();
        outlined.draw(|x, y, c| {
            let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let a = &mut alpha[py as usize * w + px as usize];
                *a = (*a + c).min(1.0);
            }
        });
    }
    let pixels = alpha.iter().map(|a| egui::Color32::from_white_alpha((a * 255.0).round() as u8)).collect();
    Some(egui::ColorImage::new([w, h], pixels))
}

/// One face of a font on the computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub path: PathBuf,
    pub index: u32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Face>,
}

/// The fonts installed on this computer, by family, found once.
pub fn families() -> &'static [Family] {
    static FOUND: OnceLock<Vec<Family>> = OnceLock::new();
    FOUND.get_or_init(|| if crate::platform::WEB || cfg!(test) { Vec::new() } else { find_families(&font_folders()) })
}

/// Where systems keep fonts.
fn font_folders() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.extend(env("WINDIR").map(|w| w.join("Fonts")));
        dirs.extend(env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")));
    } else if cfg!(target_os = "macos") {
        dirs.extend(["/Library/Fonts", "/System/Library/Fonts"].map(PathBuf::from));
        dirs.extend(env("HOME").map(|h| h.join("Library").join("Fonts")));
    } else {
        dirs.extend(env("HOME").map(|h| h.join(".local/share/fonts")));
        dirs.extend(env("HOME").map(|h| h.join(".fonts")));
        let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        dirs.extend(data.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("fonts")));
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts", "/run/current-system/sw/share/X11/fonts"].map(PathBuf::from));
    }
    dirs
}

pub fn find_families(folders: &[PathBuf]) -> Vec<Family> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in folders {
        collect_fonts(dir, 0, &mut files, &mut seen);
    }
    let mut by_name: HashMap<String, Vec<Face>> = HashMap::new();
    for file in files {
        for (name, face) in read_faces(&file) {
            let faces = by_name.entry(name).or_default();
            if !faces.iter().any(|f| f.bold == face.bold && f.italic == face.italic) {
                faces.push(face);
            }
        }
    }
    let mut families: Vec<Family> = by_name.into_iter().filter(|(n, _)| !n.starts_with('.')).map(|(name, faces)| Family { name, faces }).collect();
    families.sort_by_key(|f| f.name.to_lowercase());
    families
}

fn collect_fonts(dir: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>, seen: &mut std::collections::HashSet<PathBuf>) {
    let Ok(real) = std::fs::canonicalize(dir) else { return };
    if depth > 6 || !seen.insert(real) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, depth + 1, out, seen);
        } else if path.extension().is_some_and(|e| ["ttf", "otf", "ttc", "otc"].iter().any(|k| e.eq_ignore_ascii_case(k))) {
            out.push(path);
        }
    }
}

/// The family name and style of each face in a font file, from its `name`, `OS/2` and `head` tables only:
/// a few kilobytes even of a big file.
fn read_faces(file: &std::path::Path) -> Vec<(String, Face)> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(file) else { return Vec::new() };
    let mut at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0; len];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let be16 = |b: &[u8], i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
    let be32 = |b: &[u8], i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let Some(head) = at(0, 12) else { return Vec::new() };
    let starts: Vec<u32> = if &head[0..4] == b"ttcf" {
        let count = be32(&head, 8).min(64) as usize;
        let Some(list) = at(12, count * 4) else { return Vec::new() };
        (0..count).map(|i| be32(&list, i * 4)).collect()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for (index, start) in starts.into_iter().enumerate() {
        let Some(dir) = at(start as u64, 12) else { continue };
        let tables = be16(&dir, 4).min(100) as usize;
        let Some(records) = at(start as u64 + 12, tables * 16) else { continue };
        let find = |tag: &[u8; 4]| (0..tables).map(|i| &records[i * 16..i * 16 + 16]).find(|r| &r[0..4] == tag).map(|r| (be32(r, 8) as u64, be32(r, 12) as usize));
        let Some((name_at, name_len)) = find(b"name") else { continue };
        let Some(names) = at(name_at, name_len.min(1 << 20)) else { continue };
        let Some(family) = family_name(&names) else { continue };
        let (mut bold, mut italic) = (false, false);
        if let Some(os2) = find(b"OS/2").and_then(|(o, l)| at(o, l.min(100))).filter(|b| b.len() >= 64) {
            let selection = be16(&os2, 62);
            italic = selection & 1 != 0;
            bold = selection & 0x20 != 0;
        } else if let Some(h) = find(b"head").and_then(|(o, l)| at(o, l.min(54))).filter(|b| b.len() >= 46) {
            let style = be16(&h, 44);
            bold = style & 1 != 0;
            italic = style & 2 != 0;
        }
        out.push((family, Face { path: file.to_path_buf(), index: index as u32, bold, italic }));
    }
    out
}

/// The typographic family name (16), or the plain one (1), preferring the Windows English entry.
fn family_name(table: &[u8]) -> Option<String> {
    let be16 = |i: usize| table.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let count = be16(2)? as usize;
    let strings = be16(4)? as usize;
    let mut best: Option<(u32, String)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, encoding, language, id, len, off) = (be16(r)?, be16(r + 2)?, be16(r + 4)?, be16(r + 6)?, be16(r + 8)? as usize, be16(r + 10)? as usize);
        if id != 1 && id != 16 {
            continue;
        }
        let bytes = table.get(strings + off..strings + off + len)?;
        let text = match (platform, encoding) {
            (3, 1) | (3, 10) | (0, _) => String::from_utf16_lossy(&bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()),
            (1, 0) => bytes.iter().map(|&b| b as char).collect(),
            _ => continue,
        };
        // Typographic names first, then Windows, then American English.
        let rank = u32::from(id == 16) * 4 + u32::from(platform == 3) * 2 + u32::from(language == 0x409 || platform == 0);
        if !text.trim().is_empty() && best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, text.trim().to_string()));
        }
    }
    best.map(|(_, t)| t)
}

/// A face's bytes, read once.
fn loaded(face: &Face) -> Option<Arc<FontVec>> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, u32), Arc<FontVec>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (face.path.clone(), face.index);
    if let Some(f) = cache.lock().ok()?.get(&key) {
        return Some(f.clone());
    }
    let bytes = crate::platform::read(&face.path).ok()?;
    let font = Arc::new(FontVec::try_from_vec_and_index(bytes, face.index).ok()?);
    cache.lock().ok()?.insert(key, font.clone());
    Some(font)
}

/// The face of a family nearest the style asked for, and whether italic still has to be made by leaning it.
pub fn pick_face(family: &Family, bold: bool, italic: bool) -> Option<(Face, bool)> {
    let score = |f: &Face| u8::from(f.bold == bold) * 2 + u8::from(f.italic == italic);
    let face = family.faces.iter().max_by_key(|f| score(f))?;
    Some((face.clone(), italic && !face.italic))
}

/// How much of a box text fills when fitted to it, at first, in percent: the original's text block starts at
/// 100 (`_txtblkFillPercent`, `0x1408d6cd0`).
pub const FILL_PERCENT: f64 = 100.0;

/// The height and point that make `text` fill `percent` of the box from `lo` to `hi`, keeping its proportions
/// and centred in it. None for an empty text or box.
pub fn fit_in_box(text: &str, style: &TextStyle, lo: P, hi: P, percent: f64) -> Option<(P, f64)> {
    let layout = BoxLayout { percent, margin: Margin::None, spread_letters: false, centre_across: true };
    lay_out_in_box(text, style, lo, hi, &layout).map(|(at, height, _)| (at, height))
}

/// The original's Auto Layout Text margin (`_txtAL_margin_spacing`, `0x1408d9c90`): none, a tenth or a fifth of the
/// box's width on each side (`0x1420e8d18`, `0x14211b1a8`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Margin {
    #[default]
    None,
    Normal,
    Wide,
}

impl Margin {
    pub fn share(self) -> f64 {
        match self {
            Margin::None => 0.0,
            Margin::Normal => MARGIN_NORMAL,
            Margin::Wide => MARGIN_WIDE,
        }
    }
}

pub const MARGIN_NORMAL: f64 = 0.1;
pub const MARGIN_WIDE: f64 = 0.2;

/// How a text is laid out in a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxLayout {
    /// Burin's share of the room left inside the margins.
    pub percent: f64,
    pub margin: Margin,
    /// The original's horizontal choice 2: the letters spread until the widest line fills the width (`0x1408de7c0`).
    pub spread_letters: bool,
    /// Centred across whatever the alignment, as Fit to Selected Shape always did.
    pub centre_across: bool,
}

/// Lays a text out in the box from `lo` to `hi` as the original's Auto Layout Text does (`0x1408d9c90`): inside
/// the margin (a share of the box's width each side, the room `1 - 2 × share` of it each way), as big as fits
/// both ways keeping its proportions, centred top to bottom, and starting at the margin (left aligned), ending at
/// it (right) or centred. Gives the place, the capital height and the letter spacing.
pub fn lay_out_in_box(text: &str, style: &TextStyle, lo: P, hi: P, layout: &BoxLayout) -> Option<(P, f64, f64)> {
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let m = layout.margin.share();
    let (room_w, room_h) = (w * (1.0 - 2.0 * m), h * (1.0 - 2.0 * m));
    let share = (layout.percent / 100.0).clamp(0.01, 10.0);
    let size_of = |style: &TextStyle, height: f64| crate::geom::bbox(text_paths(text, height, [0.0, 0.0], style).iter());
    let (a, b) = size_of(style, 1.0)?;
    let (tw, th) = (b[0] - a[0], b[1] - a[1]);
    if tw <= 1e-9 || th <= 1e-9 || room_w <= 0.0 || room_h <= 0.0 {
        return None;
    }
    let height = (room_w * share / tw).min(room_h * share / th);
    let mut spaced = style.clone();
    if layout.spread_letters && text.chars().filter(|c| !c.is_whitespace()).count() > 1 {
        // Wider letter spacing until the widest line reaches the width.
        let wanted = room_w * share;
        let width_at = |s: f64| size_of(&TextStyle { spacing: s, ..style.clone() }, height).map_or(0.0, |(p, q)| q[0] - p[0]);
        if width_at(style.spacing) < wanted - 1e-9 {
            let (mut lo_s, mut hi_s) = (style.spacing, style.spacing.max(1.0) * 2.0);
            while width_at(hi_s) < wanted && hi_s < 50.0 {
                hi_s *= 2.0;
            }
            for _ in 0..40 {
                let mid = (lo_s + hi_s) / 2.0;
                if width_at(mid) < wanted {
                    lo_s = mid;
                } else {
                    hi_s = mid;
                }
            }
            spaced.spacing = lo_s;
        }
    }
    let (a, b) = size_of(&spaced, height)?;
    let middle_y = (lo[1] + hi[1]) / 2.0 - (a[1] + b[1]) / 2.0;
    let x = if layout.centre_across {
        (lo[0] + hi[0]) / 2.0 - (a[0] + b[0]) / 2.0
    } else {
        match style.align {
            TextAlign::Left => lo[0] + w * m - a[0],
            TextAlign::Centre => (lo[0] + hi[0]) / 2.0 - (a[0] + b[0]) / 2.0,
            TextAlign::Right => hi[0] - w * m - b[0],
        }
    };
    Some(([x, middle_y], height, spaced.spacing))
}

/// The letters of `text` as paths; `height` is the capital height and `origin` the start of the first
/// baseline (or its middle or end, as the text is aligned). A font this computer doesn't have is
/// written in Burin's own.
pub fn text_paths(text: &str, height: f64, origin: P, style: &TextStyle) -> Vec<Path> {
    if let Some((_, upright, bold)) = SINGLE_LINE.iter().find(|(name, ..)| *name == style.font) {
        let glyphs = parse_jhf(if style.bold { bold } else { upright });
        return single_line(text, height, origin, &glyphs, style.italic, style);
    }
    if let Some(family) = families().iter().find(|f| f.name == style.font) {
        if let Some((face, lean)) = pick_face(family, style.bold, style.italic) {
            if let Some(font) = loaded(&face) {
                return outlines(text, height, origin, font.as_ref(), lean, style);
            }
        }
    }
    match FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT) {
        Ok(font) => outlines(text, height, origin, &font, style.italic, style),
        Err(_) => Vec::new(),
    }
}

/// Where each line starts, for its width.
fn line_start(align: TextAlign, width: f64) -> f64 {
    match align {
        TextAlign::Left => 0.0,
        TextAlign::Centre => -width / 2.0,
        TextAlign::Right => -width,
    }
}

fn outlines(text: &str, height: f64, origin: P, font: &impl Font, lean: bool, style: &TextStyle) -> Vec<Path> {
    let cap = font
        .outline(font.glyph_id('H'))
        .map(|o| {
            let ys = o.curves.iter().flat_map(|c| match c {
                OutlineCurve::Line(a, b) => vec![a.y, b.y],
                OutlineCurve::Quad(a, c, b) => vec![a.y, c.y, b.y],
                OutlineCurve::Cubic(a, c, d, b) => vec![a.y, c.y, d.y, b.y],
            });
            let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(y), h.max(y)));
            (hi - lo) as f64
        })
        .unwrap_or(700.0)
        .max(1.0);
    let k = height / cap;
    let slant = if lean { SLANT } else { 0.0 };
    let mut paths = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        // Each letter's place: its width times the spacing asked for, with the font's own kerning between
        // pairs, as the original keeps a font's kerning pairs with it (`txtKerningPairs`, `0x1408e5410`).
        let ids: Vec<_> = line.chars().map(|ch| font.glyph_id(ch)).collect();
        let mut places = Vec::with_capacity(ids.len());
        let mut caret = 0.0;
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                caret += font.kern_unscaled(ids[i - 1], *id) as f64 * k;
            }
            places.push(caret);
            caret += font.h_advance_unscaled(*id) as f64 * k * style.spacing;
        }
        let width = caret;
        let start = line_start(style.align, width);
        let y = -(row as f64) * height * style.line_spacing;
        for (ch, place) in line.chars().zip(places) {
            let x = start + place;
            let id = font.glyph_id(ch);
            if let Some(outline) = font.outline(id) {
                let map = |p: ab_glyph::Point| {
                    let (gx, gy) = (p.x as f64 * k, p.y as f64 * k);
                    [origin[0] + x + gx + gy * slant, origin[1] + y + gy]
                };
                let mut pts: Vec<P> = Vec::new();
                let mut last: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let first = match curve {
                        OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => *a,
                    };
                    if last.is_none_or(|l| (l.x - first.x).abs() + (l.y - first.y).abs() > 0.01) {
                        if pts.len() >= 3 {
                            paths.push(Path::closed(std::mem::take(&mut pts)));
                        }
                        pts.clear();
                        pts.push(map(first));
                    }
                    match curve {
                        OutlineCurve::Line(_, b) => {
                            pts.push(map(*b));
                            last = Some(*b);
                        }
                        OutlineCurve::Quad(a, c, b) => {
                            for s in 1..=8 {
                                let t = s as f32 / 8.0;
                                let u = 1.0 - t;
                                pts.push(map(ab_glyph::point(u * u * a.x + 2.0 * u * t * c.x + t * t * b.x, u * u * a.y + 2.0 * u * t * c.y + t * t * b.y)));
                            }
                            last = Some(*b);
                        }
                        OutlineCurve::Cubic(a, c, d, b) => {
                            for s in 1..=10 {
                                let t = s as f32 / 10.0;
                                let u = 1.0 - t;
                                pts.push(map(ab_glyph::point(
                                    u * u * u * a.x + 3.0 * u * u * t * c.x + 3.0 * u * t * t * d.x + t * t * t * b.x,
                                    u * u * u * a.y + 3.0 * u * u * t * c.y + 3.0 * u * t * t * d.y + t * t * t * b.y,
                                )));
                            }
                            last = Some(*b);
                        }
                    }
                }
                if pts.len() >= 3 {
                    paths.push(Path::closed(pts));
                }
            }
        }
    }
    // Font outlines repeat their first point at the end.
    for p in &mut paths {
        if p.pts.len() > 3 && p.pts.first() == p.pts.last() {
            p.pts.pop();
        }
    }
    paths
}

/// A single-line letter: its left and right edges, and its strokes, in the font's units with y going up.
#[derive(Clone, Debug, Default)]
pub struct Stroked {
    pub left: i32,
    pub right: i32,
    pub strokes: Vec<Vec<(i32, i32)>>,
}

/// Reads a Hershey font: one letter a line from the space on, each a number, a count of points, then the
/// points as pairs of letters counted from `R`, with ` R` lifting the pen. The first pair is the letter's edges.
pub fn parse_jhf(src: &str) -> Vec<Stroked> {
    src.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            if bytes.len() < 10 {
                return Stroked::default();
            }
            let pair = |i: usize| (bytes[i] as i32 - b'R' as i32, bytes[i + 1] as i32 - b'R' as i32);
            let (left, right) = pair(8);
            let mut strokes = vec![Vec::new()];
            let mut i = 10;
            while i + 1 < bytes.len() {
                if bytes[i] == b' ' && bytes[i + 1] == b'R' {
                    strokes.push(Vec::new());
                } else {
                    let (x, y) = pair(i);
                    strokes.last_mut().unwrap().push((x, -y));
                }
                i += 2;
            }
            strokes.retain(|s| s.len() >= 2);
            Stroked { left, right, strokes }
        })
        .collect()
}

fn single_line(text: &str, height: f64, origin: P, glyphs: &[Stroked], lean: bool, style: &TextStyle) -> Vec<Path> {
    let glyph = |ch: char| (ch as u32).checked_sub(32).and_then(|i| glyphs.get(i as usize)).or_else(|| glyphs.get('?' as usize - 32));
    // The capital's height and the baseline, from the H.
    let (base, top) = glyph('H').map_or((-9, 12), |g| {
        let ys = g.strokes.iter().flatten().map(|p| p.1);
        (ys.clone().min().unwrap_or(-9), ys.max().unwrap_or(12))
    });
    let k = height / ((top - base).max(1) as f64);
    let slant = if lean { SLANT } else { 0.0 };
    let mut paths = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let width: f64 = line.chars().filter_map(glyph).map(|g| (g.right - g.left) as f64 * k * style.spacing).sum();
        let mut x = line_start(style.align, width);
        let y = -(row as f64) * height * style.line_spacing;
        for ch in line.chars() {
            let Some(g) = glyph(ch) else { continue };
            for stroke in &g.strokes {
                let pts = stroke
                    .iter()
                    .map(|&(sx, sy)| {
                        let (gx, gy) = ((sx - g.left) as f64 * k, (sy - base) as f64 * k);
                        [origin[0] + x + gx + gy * slant, origin[1] + y + gy]
                    })
                    .collect();
                paths.push(Path::open(pts));
            }
            x += (g.right - g.left) as f64 * k * style.spacing;
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(paths: &[Path]) -> ([f64; 2], [f64; 2]) {
        crate::geom::bbox(paths.iter()).unwrap()
    }

    #[test]
    fn a_single_line_font_draws_open_strokes_at_the_height_asked() {
        let style = TextStyle { font: "Single Line Sans".into(), ..Default::default() };
        let paths = text_paths("HI", 10.0, [0.0, 0.0], &style);
        assert!(!paths.is_empty() && paths.iter().all(|p| !p.closed), "lines for the tool to follow");
        let (lo, hi) = bounds(&paths);
        assert!((hi[1] - lo[1] - 10.0).abs() < 1e-9 && lo[1].abs() < 1e-9, "a capital is 10 high on the baseline: {lo:?} {hi:?}");
        // Every letter of the alphabet has strokes.
        let glyphs = parse_jhf(SINGLE_LINE[0].1);
        assert_eq!(glyphs.len(), 96, "the space to the last of the 96 printable places");
        for ch in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            assert!(!glyphs[ch as usize - 32].strokes.is_empty(), "{ch}");
        }
    }

    #[test]
    fn text_lines_up_on_its_point_as_aligned() {
        let left = text_paths("Wood", 1.0, [5.0, 0.0], &TextStyle::default());
        let centre = text_paths("Wood", 1.0, [5.0, 0.0], &TextStyle { align: TextAlign::Centre, ..Default::default() });
        let right = text_paths("Wood", 1.0, [5.0, 0.0], &TextStyle { align: TextAlign::Right, ..Default::default() });
        assert!(bounds(&left).0[0] >= 5.0 - 0.1);
        let (lo, hi) = bounds(&centre);
        assert!(((lo[0] + hi[0]) / 2.0 - 5.0).abs() < 0.1, "centred: {lo:?} {hi:?}");
        assert!(bounds(&right).1[0] <= 5.0 + 1e-9);
    }

    #[test]
    fn italic_leans_the_letters() {
        let upright = text_paths("I", 1.0, [0.0, 0.0], &TextStyle { font: "Single Line Sans".into(), ..Default::default() });
        let leaning = text_paths("I", 1.0, [0.0, 0.0], &TextStyle { font: "Single Line Sans".into(), italic: true, ..Default::default() });
        let top = |p: &[Path]| p.iter().flat_map(|p| p.pts.iter()).max_by(|a, b| a[1].total_cmp(&b[1])).copied().unwrap();
        assert!((top(&leaning)[0] - top(&upright)[0] - SLANT).abs() < 1e-9);
    }

    #[test]
    fn a_fonts_name_is_drawn_in_that_font() {
        let font = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        let image = name_picture(&font, "Ubuntu", 18.0, 230.0).expect("a picture");
        assert!(image.size[0] > 30 && image.size[1] >= 18, "{:?}", image.size);
        assert!(image.pixels.iter().any(|p| p.a() > 200), "the letters show");
        assert!(name_picture(&font, "\u{E000}\u{E001}", 18.0, 230.0).is_none(), "no letters for it: named plainly");
        let wide = name_picture(&font, &"W".repeat(80), 18.0, 230.0).unwrap();
        assert!(wide.size[0] <= 230, "cut to the list's width");
    }

    #[test]
    fn text_fitted_to_a_box_fills_it_one_way_and_sits_in_the_middle() {
        let style = TextStyle::default();
        let (at, height) = fit_in_box("Wood", &style, [0.0, 0.0], [10.0, 4.0], 100.0).unwrap();
        let (lo, hi) = bounds(&text_paths("Wood", height, at, &style));
        assert!((hi[0] - lo[0] - 10.0).abs() < 1e-6 || (hi[1] - lo[1] - 4.0).abs() < 1e-6, "fills the box one way: {lo:?} {hi:?}");
        assert!(lo[0] >= -1e-6 && hi[0] <= 10.0 + 1e-6 && lo[1] >= -1e-6 && hi[1] <= 4.0 + 1e-6, "and stays inside");
        assert!(((lo[0] + hi[0]) / 2.0 - 5.0).abs() < 1e-6 && ((lo[1] + hi[1]) / 2.0 - 2.0).abs() < 1e-6, "centred");
        let (_, half) = fit_in_box("Wood", &style, [0.0, 0.0], [10.0, 4.0], 50.0).unwrap();
        assert!((half - height / 2.0).abs() < 1e-9);
        assert!(fit_in_box("", &style, [0.0, 0.0], [10.0, 4.0], 100.0).is_none());
    }

    #[test]
    fn a_text_laid_out_in_a_box_keeps_its_margin_and_can_spread_its_letters() {
        let style = TextStyle::default();
        let layout = |margin, spread| BoxLayout { percent: 100.0, margin, spread_letters: spread, centre_across: false };
        // A wide box: the height decides. A normal margin is a tenth of the width each side.
        let (at, height, spacing) = lay_out_in_box("Wood", &style, [0.0, 0.0], [20.0, 4.0], &layout(Margin::Normal, false)).unwrap();
        let (lo, hi) = bounds(&text_paths("Wood", height, at, &style));
        assert!((lo[0] - 2.0).abs() < 1e-6, "starts at the margin: {lo:?}");
        assert!((hi[1] - lo[1] - 4.0 * 0.8).abs() < 1e-6, "fills the room left up and down: {lo:?} {hi:?}");
        assert!(((lo[1] + hi[1]) / 2.0 - 2.0).abs() < 1e-6, "centred up and down");
        assert_eq!(spacing, 1.0);
        // Spread: the same height, the letters further apart until the word reaches the right margin.
        let (at2, height2, spacing2) = lay_out_in_box("Wood", &style, [0.0, 0.0], [20.0, 4.0], &layout(Margin::Normal, true)).unwrap();
        assert!((height2 - height).abs() < 1e-9 && spacing2 > 1.0);
        let spread = TextStyle { spacing: spacing2, ..style.clone() };
        let (lo, hi) = bounds(&text_paths("Wood", height2, at2, &spread));
        assert!((lo[0] - 2.0).abs() < 1e-6 && (hi[0] - 18.0).abs() < 1e-3, "{lo:?} {hi:?}");
        // Right aligned: it ends at the right margin. A wide margin is a fifth.
        let right = TextStyle { align: TextAlign::Right, ..style.clone() };
        let (at3, height3, _) = lay_out_in_box("Wood", &right, [0.0, 0.0], [20.0, 4.0], &layout(Margin::Wide, false)).unwrap();
        let (_, hi) = bounds(&text_paths("Wood", height3, at3, &right));
        assert!((hi[0] - 16.0).abs() < 1e-6, "{hi:?}");
    }

    #[test]
    fn letters_and_lines_spread_as_asked() {
        let plain = TextStyle { font: "Single Line Sans".into(), ..Default::default() };
        let wide = TextStyle { spacing: 1.5, ..plain.clone() };
        let w = |style: &TextStyle| {
            let (lo, hi) = bounds(&text_paths("HHHH", 1.0, [0.0, 0.0], style));
            hi[0] - lo[0]
        };
        assert!(w(&wide) > w(&plain) * 1.2, "{} {}", w(&wide), w(&plain));
        let two = |style: &TextStyle| {
            let (lo, hi) = bounds(&text_paths("H\nH", 1.0, [0.0, 0.0], style));
            hi[1] - lo[1]
        };
        assert!((two(&plain) - (1.0 + LINE_GAP)).abs() < 1e-9);
        assert!((two(&TextStyle { line_spacing: 2.0, ..plain.clone() }) - 3.0).abs() < 1e-9);
        // Left as they are, they aren't written in the file.
        let text = serde_json::to_string(&TextStyle::default()).unwrap();
        assert!(!text.contains("spacing"), "{text}");
        let back: TextStyle = serde_json::from_str("{}").unwrap();
        assert_eq!(back, TextStyle::default());
    }

    #[test]
    fn the_fonts_kerning_brings_pairs_together() {
        let font = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        let kern = font.kern_unscaled(font.glyph_id('A'), font.glyph_id('V'));
        let together = |a: &str| {
            let (lo, hi) = bounds(&text_paths(a, 1.0, [0.0, 0.0], &TextStyle::default()));
            hi[0] - lo[0]
        };
        // Only fonts with kerning pairs move: the built-in one's are taken as it has them.
        if kern < 0.0 {
            assert!(together("AV") < together("AA") + 1e-9);
        }
        assert!(together("AV") > 0.0);
    }

    #[test]
    fn a_symbol_fonts_letters_are_looked_for_where_windows_keeps_them() {
        assert_eq!(symbol_text("Ab"), "\u{F041}\u{F062}");
        assert_eq!(symbol_text("é"), "é", "only plain letters move");
    }

    #[test]
    fn a_font_files_family_and_style_are_read_from_its_tables() {
        let dir = std::env::temp_dir().join(format!("burin-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ubuntu.ttf"), epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a font").unwrap();
        let found = find_families(&[dir.clone()]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].name.starts_with("Ubuntu"), "{}", found[0].name);
        assert!(!found[0].faces[0].italic);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
