//! Print Setup and the PDF that File > Print makes.
//!
//! The original's Print Setup is the Windows printer dialog (`CWinApp::OnFilePrintSetup`, `0x140b6e814`,
//! command 57606): printer, paper and orientation, kept for the session. Its 2D view prints a copy of what
//! the screen shows (`0x140a3b550`), and its 3D view a picture of itself (`0x1408ab900`, `0x1408ab980`).
//! Burin makes a PDF instead, so the setup says what paper the PDF's pages are and how the drawing goes on
//! them: at its real size over as many pages as it takes, or shrunk to one page.

use crate::app::App;
use crate::geom::Path;
use crate::i18n::tr;
use egui::Align2;

/// The original's Print Setup command.
pub const PRINT_SETUP_COMMAND: u32 = 57606;

/// Room left round each page, where printers often can't print: 10 mm, in points.
pub const MARGIN_PT: f64 = 10.0 * 72.0 / 25.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paper {
    /// One page the size of the job: nothing to work out, the drawing as it is.
    JobSize,
    A4,
    A3,
    Letter,
}

impl Paper {
    pub const ALL: [Paper; 4] = [Paper::JobSize, Paper::A4, Paper::A3, Paper::Letter];

    pub fn name(self) -> &'static str {
        match self {
            Paper::JobSize => tr("Size of the job"),
            Paper::A4 => "A4",
            Paper::A3 => "A3",
            Paper::Letter => "Letter",
        }
    }

    /// Width and height upright, in points.
    fn size(self) -> Option<[f64; 2]> {
        let mm = |w: f64, h: f64| Some([w * 72.0 / 25.4, h * 72.0 / 25.4]);
        match self {
            Paper::JobSize => None,
            Paper::A4 => mm(210.0, 297.0),
            Paper::A3 => mm(297.0, 420.0),
            Paper::Letter => Some([612.0, 792.0]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintSetup {
    pub paper: Paper,
    pub landscape: bool,
    /// Shrunk to one page rather than at its real size over several.
    pub fit: bool,
}

impl Default for PrintSetup {
    fn default() -> Self {
        PrintSetup { paper: Paper::JobSize, landscape: false, fit: false }
    }
}

/// How the drawing goes on the paper: the page size, its scale, and how many pages across and up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub page: [f64; 2],
    pub scale: f64,
    pub across: usize,
    pub up: usize,
}

impl PrintSetup {
    /// For a job of `size` points.
    pub fn layout(&self, size: [f64; 2]) -> Layout {
        let Some([w, h]) = self.paper.size() else {
            return Layout { page: size, scale: 1.0, across: 1, up: 1 };
        };
        let page = if self.landscape { [h, w] } else { [w, h] };
        let room = [page[0] - 2.0 * MARGIN_PT, page[1] - 2.0 * MARGIN_PT];
        if self.fit {
            let scale = (room[0] / size[0].max(1e-9)).min(room[1] / size[1].max(1e-9)).min(1.0);
            return Layout { page, scale, across: 1, up: 1 };
        }
        // A whisker over a page is still one page: 0.1 mm.
        let count = |job: f64, room: f64| ((job - 0.28) / room).ceil().max(1.0) as usize;
        Layout { page, scale: 1.0, across: count(size[0], room[0]), up: count(size[1], room[1]) }
    }
}

fn num(v: f64) -> String {
    let text = format!("{v:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The drawing as a PDF on the paper chosen. `size` is the job's width and height in its units, and
/// `points_per_unit` turns those into points.
pub fn pdf(paths: &[Path], size: [f64; 2], points_per_unit: f64, setup: &PrintSetup) -> Vec<u8> {
    let job = [size[0] * points_per_unit, size[1] * points_per_unit];
    let layout = setup.layout(job);
    let mut art = String::new();
    for path in paths {
        for (i, p) in path.pts.iter().enumerate() {
            art.push_str(&format!("{} {} {}\n", num(p[0] * points_per_unit), num(p[1] * points_per_unit), if i == 0 { "m" } else { "l" }));
        }
        art.push_str(if path.closed { "h S\n" } else { "S\n" });
    }
    let one_page = setup.paper == Paper::JobSize;
    let room = [layout.page[0] - 2.0 * MARGIN_PT, layout.page[1] - 2.0 * MARGIN_PT];
    let mut pages = Vec::new();
    for row in 0..layout.up {
        for column in 0..layout.across {
            let mut content = String::new();
            if one_page {
                content.push_str(&format!("0.5 w\n{art}"));
            } else if setup.fit {
                // Centred on the page.
                let (dx, dy) = ((layout.page[0] - job[0] * layout.scale) / 2.0, (layout.page[1] - job[1] * layout.scale) / 2.0);
                content.push_str(&format!("q {s} 0 0 {s} {} {} cm {} w\n{art}Q\n", num(dx), num(dy), num(0.5 / layout.scale), s = num(layout.scale)));
            } else {
                // This page's share of the job, from the bottom left, cut to the room inside the margins.
                let (left, bottom) = (column as f64 * room[0], row as f64 * room[1]);
                content.push_str(&format!(
                    "q {m} {m} {} {} re W n 1 0 0 1 {} {} cm 0.5 w\n{art}Q\n",
                    num(room[0]),
                    num(room[1]),
                    num(MARGIN_PT - left),
                    num(MARGIN_PT - bottom),
                    m = num(MARGIN_PT),
                ));
                // Where the page goes: its column and row, counted from the bottom left, and the edge of its share.
                let label = format!("{} {} / {}", tr("Page"), row * layout.across + column + 1, layout.across * layout.up);
                let place = format!("{}{}", (b'A' + (column % 26) as u8) as char, row + 1);
                content.push_str(&format!(
                    "0.2 w 0.6 G {m} {m} {} {} re S 0 g BT /F1 8 Tf {m} {} Td ({}) Tj ET\n",
                    num(room[0]),
                    num(room[1]),
                    num(MARGIN_PT * 0.4),
                    pdf_text(&format!("{place}   {label}")),
                    m = num(MARGIN_PT),
                ));
            }
            pages.push(content);
        }
    }
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string()];
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    for (i, content) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            num(layout.page[0]),
            num(layout.page[1]),
            5 + 2 * i
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
    }
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let table = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1));
    out.into_bytes()
}

/// Text for a PDF string: plain letters only, as the standard font has no others.
fn pdf_text(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '(' | ')' | '\\' => format!("\\{c}"),
            c if c.is_ascii() && !c.is_control() => c.to_string(),
            _ => "?".into(),
        })
        .collect()
}

impl App {
    pub(crate) fn print_setup_window(&mut self, ctx: &egui::Context) {
        if !self.print_setup_open {
            return;
        }
        let mut open = true;
        let (_, points) = if self.doc.job.units == crate::app::Units::Mm { ("mm", 72.0 / 25.4) } else { ("in", 72.0) };
        let job = [self.doc.job.width * points, self.doc.job.height * points];
        egui::Window::new(tr("Print Setup")).id(egui::Id::unique("print-setup")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            let setup = &mut self.print_setup;
            egui::Grid::new("print-setup-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Paper"));
                egui::ComboBox::from_id_salt("print-paper").selected_text(setup.paper.name()).show_ui(ui, |ui| {
                    for paper in Paper::ALL {
                        ui.selectable_value(&mut setup.paper, paper, paper.name());
                    }
                });
                ui.end_row();
                let paper = setup.paper != Paper::JobSize;
                ui.label(tr("Orientation"));
                ui.add_enabled_ui(paper, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut setup.landscape, false, tr("Portrait"));
                        ui.radio_value(&mut setup.landscape, true, tr("Landscape"));
                    });
                });
                ui.end_row();
                ui.label(tr("Scale"));
                ui.add_enabled_ui(paper, |ui| {
                    ui.vertical(|ui| {
                        ui.radio_value(&mut setup.fit, false, tr("Real size, over several pages"));
                        ui.radio_value(&mut setup.fit, true, tr("Shrink to one page"));
                    });
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            let layout = setup.layout(job);
            let note = if setup.paper == Paper::JobSize {
                tr("One page the size of the job, at its real size.").to_string()
            } else if setup.fit {
                tf!("One page, at {}% of the real size.", (layout.scale * 1000.0).round() / 10.0)
            } else {
                tf!("{} pages, {} across and {} up. Each is marked with where it goes.", layout.across * layout.up, layout.across, layout.up)
            };
            ui.label(egui::RichText::new(note).color(crate::theme::MUTED));
            ui.add_space(4.0);
            if ui.button(tr("Print...")).clicked() {
                self.print_setup_open = false;
                self.print_drawing(ctx);
            }
        });
        if !open {
            self.print_setup_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: [f64; 2] = [595.2756, 841.8898];

    #[test]
    fn a_job_bigger_than_the_paper_is_spread_over_pages_at_its_real_size() {
        // 600 x 400 mm on A4 upright: 190 x 277 mm inside the margins.
        let job = [600.0 * 72.0 / 25.4, 400.0 * 72.0 / 25.4];
        let tall = PrintSetup { paper: Paper::A4, landscape: false, fit: false }.layout(job);
        assert_eq!((tall.across, tall.up, tall.scale), (4, 2, 1.0));
        let wide = PrintSetup { landscape: true, ..PrintSetup { paper: Paper::A4, landscape: false, fit: false } }.layout(job);
        assert_eq!((wide.across, wide.up), (3, 3));
        assert!((wide.page[0] - A4[1]).abs() < 0.01, "turned: {:?}", wide.page);
    }

    #[test]
    fn shrinking_keeps_the_proportions_and_never_enlarges() {
        let job = [600.0 * 72.0 / 25.4, 400.0 * 72.0 / 25.4];
        let fit = PrintSetup { paper: Paper::A4, landscape: true, fit: true }.layout(job);
        assert_eq!((fit.across, fit.up), (1, 1));
        assert!((fit.scale - 277.0 / 600.0).abs() < 1e-3, "{}", fit.scale);
        let small = PrintSetup { paper: Paper::A4, landscape: false, fit: true }.layout([100.0, 100.0]);
        assert_eq!(small.scale, 1.0);
    }

    #[test]
    fn the_pdf_has_one_page_for_each_part() {
        let square = Path::closed(vec![[0.0, 0.0], [600.0, 0.0], [600.0, 400.0], [0.0, 400.0]]);
        let setup = PrintSetup { paper: Paper::A4, landscape: false, fit: false };
        let text = String::from_utf8(pdf(&[square.clone()], [600.0, 400.0], 72.0 / 25.4, &setup)).unwrap();
        assert!(text.contains("/Count 8"), "4 across and 2 up");
        // To look at it: BURIN_PDF_OUT=/some/file.pdf cargo test the_pdf_has
        if let Ok(out) = std::env::var("BURIN_PDF_OUT") {
            let circle = Path::closed(crate::geom::circle([300.0, 200.0], 150.0));
            std::fs::write(out, pdf(&[square.clone(), circle], [600.0, 400.0], 72.0 / 25.4, &setup)).unwrap();
        }
        assert!(text.contains("(B2   Page 6 / 8)"), "each page says where it goes");
        let whole = String::from_utf8(pdf(&[square], [600.0, 400.0], 72.0 / 25.4, &PrintSetup::default())).unwrap();
        assert!(whole.contains("/Count 1") && whole.contains("/MediaBox [0 0 1700.7874 1133.8583]"));
    }
}
