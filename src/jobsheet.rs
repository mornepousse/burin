//! A page to take to the machine, saying how to set up and what each toolpath needs. A single HTML
//! file that any browser can show or print (`ID_TOOLPATHS_CREATESETUPSHEET` in the original).

use crate::app::*;
use crate::i18n::tr;
use crate::toolpath;
use crate::ui::duration;

/// Text made safe to put in a page: names come from the user.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn row(out: &mut String, label: &str, value: &str) {
    out.push_str(&format!("<tr><th>{}</th><td>{}</td></tr>\n", escaped(label), escaped(value)));
}

const STYLE: &str = "body{font:15px/1.4 system-ui,sans-serif;color:#1b1d21;margin:24px auto;max-width:900px;padding:0 16px}h1{font-size:26px;margin:0 0 4px}h2{font-size:18px;margin:26px 0 8px;border-bottom:2px solid #13a67f;padding-bottom:3px}h3{font-size:15px;margin:14px 0 4px}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:3px 8px 3px 0;border-bottom:1px solid #e4e4e0;font-weight:normal}th{color:#5b6068;width:38%}table.list th{width:auto;font-weight:600;color:#1b1d21}svg{display:block;width:100%;max-height:420px;background:#f9f8f5;border:1px solid #c8c8c2}.note{color:#5b6068;font-size:13px;margin-top:8px}section{break-inside:avoid}";

impl App {
    /// The whole page.
    pub fn job_sheet_html(&self, title: &str) -> String {
        let job = &self.doc.job;
        let inches = job.units == Units::Inches;
        let unit = if inches { "\"" } else { " mm" };
        let length = |v: f64| format!("{}{unit}", (v * 10000.0).round() / 10000.0);
        let machine = self.doc.machine_on_job();
        let rapid = if inches { toolpath::RAPID_RATE_MM / 25.4 } else { toolpath::RAPID_RATE_MM };
        let seconds = |t: &Toolpath| self.toolpath_seconds(t, rapid);
        let calculated = |t: &Toolpath| !t.moves.is_empty();

        let mut out = format!("<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title><style>{STYLE}</style></head><body>\n", escaped(title));
        out.push_str(&format!("<h1>{}</h1>\n<p>{}</p>\n", escaped(tr("Job Sheet")), escaped(title)));

        if !self.doc.notes.trim().is_empty() {
            out.push_str(&format!("<section><h2>{}</h2><p style=\"white-space:pre-wrap\">{}</p></section>\n", escaped(tr("Notes")), escaped(self.doc.notes.trim())));
        }
        out.push_str(&format!("<section><h2>{}</h2>\n", escaped(tr("Job layout"))));
        out.push_str(&format!("<svg viewBox=\"0 0 {} {}\" xmlns=\"http://www.w3.org/2000/svg\"><g transform=\"translate(0 {}) scale(1 -1)\" fill=\"none\" stroke=\"#1b1d21\">\n", job.width, job.height, job.height));
        for v in self.doc.vectors.iter().filter(|v| !v.construction && self.doc.shows(v)) {
            for p in &v.paths {
                let points: Vec<String> = p.pts.iter().map(|q| format!("{:.4},{:.4}", q[0], q[1])).collect();
                let shape = if p.closed { "polygon" } else { "polyline" };
                out.push_str(&format!("<{shape} points=\"{}\" vector-effect=\"non-scaling-stroke\"/>\n", points.join(" ")));
            }
        }
        out.push_str("</g></svg></section>\n");

        out.push_str(&format!("<section><h2>{}</h2><table>\n", escaped(tr("Material setup"))));
        row(&mut out, tr("Material block"), &format!("X {}  Y {}  Z {}", length(job.width), length(job.height), length(job.thickness)));
        row(&mut out, tr("Z zero"), tr(if job.z_on_bed { "Bottom of the material" } else { "Top of the material" }));
        row(&mut out, tr("XY origin"), job.datum.label());
        row(&mut out, tr("Home position"), &format!("X {}  Y {}  Z {}", length(machine.home[0]), length(machine.home[1]), length(machine.home[2])));
        row(&mut out, tr("Clearance height"), &length(machine.safe_z));
        row(&mut out, tr("Plunge height"), &length(machine.plunge_gap));
        out.push_str("</table></section>\n");

        let list = &self.doc.toolpaths;
        let total: f64 = list.iter().filter(|t| calculated(t)).map(seconds).sum();
        out.push_str(&format!("<section><h2>{}</h2><table class=\"list\">\n<tr><th>{}</th><th>{}</th><th>{}</th></tr>\n", escaped(tr("Toolpath summary")), escaped(tr("Toolpath")), escaped(tr("Tool")), escaped(tr("Time estimate"))));
        for t in list {
            let time = if calculated(t) { duration(seconds(t)) } else { tr("Not calculated").to_string() };
            out.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{time}</td></tr>\n", escaped(&t.op.name), escaped(&t.op.tool.name)));
        }
        out.push_str(&format!("<tr><th>{}</th><td></td><td>{}</td></tr>\n</table></section>\n", escaped(tr("Total time")), duration(total)));

        for t in list {
            let (op, tool) = (&t.op, &t.op.tool);
            let rate = if tool.metric { "mm/min".to_string() } else { format!("{}/min", tr("inch")) };
            out.push_str(&format!("<section><h2>{}</h2>\n<table>\n", escaped(&op.name)));
            row(&mut out, tr("Toolpath type"), &if op.merged { format!("{} ({})", op.kind.title(), tr("Merged")) } else { op.kind.title().to_string() });
            let time = if calculated(t) { duration(seconds(t)) } else { tr("Not calculated").to_string() };
            row(&mut out, tr("Time estimate"), &time);
            row(&mut out, tr("Cut depth"), &length(op.cut_depth));
            row(&mut out, tr("Tool"), &tool.name);
            row(&mut out, tr("Tool type"), tool.kind.name());
            row(&mut out, tr("Tool number"), &tool.number.to_string());
            row(&mut out, tr("Feed rate"), &format!("{} {rate}", tool.feed));
            row(&mut out, tr("Plunge rate"), &format!("{} {rate}", tool.plunge));
            row(&mut out, tr("Spindle speed"), &format!("{}", tool.rpm));
            row(&mut out, tr("Pass depth"), &length(tool.pass_depth));
            row(&mut out, tr("Stepover"), &length(op.stepover));
            if !op.notes.trim().is_empty() {
                row(&mut out, tr("Notes"), op.notes.trim());
            }
            out.push_str("</table></section>\n");
        }
        out.push_str(&format!("<p class=\"note\">{}</p>\n</body></html>\n", escaped(tr("Estimates: cutting and lifting count double, and fast moves go at 100 mm/s."))));
        out
    }

    /// Asks where to put the page. With Ctrl held on the menu, it opens straight away.
    pub fn save_job_sheet_dialog(&mut self, ctx: &egui::Context, open_after: bool) {
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| tr("Job sheet").to_string(), |s| s.to_string_lossy().to_string());
        let mut dialog = crate::platform::FileDialog::new().add_filter(tr("Job sheet"), &["html"]).set_file_name(format!("{}.html", crate::pieces::file_stem_of(&stem)));
        if let Some(folder) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(folder);
        }
        let Some(path) = dialog.save_file() else { return };
        let path = crate::pieces::with_extension(path, "html");
        match crate::platform::write(&path, self.job_sheet_html(&stem)) {
            Ok(()) => {
                self.status = tf!("Job sheet saved to {}", path.display());
                if open_after {
                    let address = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
                    let slash = if address.starts_with('/') { "" } else { "/" };
                    ctx.open_url(egui::OpenUrl::new_tab(format!("file://{slash}{address}")));
                }
            }
            Err(e) => self.message = Some(tf!("Could not write {}: {e}", path.display())),
        }
    }
}
