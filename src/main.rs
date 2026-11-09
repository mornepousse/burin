// A released Windows program opens its own window only, with no console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The web version leaves out crash reports, the ways of opening a window and the like: what only they
// use would be flagged as unused there. The program for computers is still checked for it.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

//! Burin: a desktop program for designing and carving with a CNC machine.

#[macro_use]
mod i18n;
mod platform;
mod formula;
mod modeltools;
mod fonts;
mod variables;
mod printing;
mod viewprefs;
#[cfg(target_arch = "wasm32")]
mod web;
mod app;
mod arcs;
mod bitmap;
mod bulk;
mod canvas;
mod constraints;
mod crv3d;
mod distort;
mod edit;
mod geom;
mod fr;
mod icons;
mod keys;
mod menus;
mod merge;
mod curvefit;
mod rates;
mod gcodein;
mod help;
mod checks;
mod backups;
mod crash;
mod feedback;
mod linuxmenu;
mod recovery;
mod jobsheet;
mod support;
mod templates;
mod opdialogs;
mod panels;
mod pieces;
mod recalc;
mod planetcnc;
mod postp;
mod snap;
mod startup;
mod svg;
mod theme;
mod relief;
mod relief_import;
mod sculpt;
mod sweep;
mod toolpath;
mod ui;
mod view3d;
#[cfg(test)]
mod ui_tests;

use app::App;
use std::path::PathBuf;

/// Once the window has been drawn, trying another way of drawing it no longer helps.
#[cfg(not(target_arch = "wasm32"))]
static SHOWING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(not(target_arch = "wasm32"))]
struct Program {
    app: App,
    screenshot: Option<PathBuf>,
    frames: u32,
    /// True while the window is being driven only by `--screenshot`.
    requested: bool,
    engine: startup::Engine,
    drawn: u32,
    crash_later: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl eframe::App for Program {
    /// Closing normally: nothing to get back next time.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let (Some(data), None, false) = (&self.app.data_dir, &self.app.recovery, self.app.recovery_left) {
            recovery::remove(data);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !SHOWING.swap(true, std::sync::atomic::Ordering::Relaxed) {
            startup::log("The window is showing.");
        }
        let ctx = ui.ctx().clone();
        fit_scale(&ctx);
        self.app.frame(ui);
        // Drawn more than once: this way of drawing works on this computer.
        self.drawn = self.drawn.saturating_add(1);
        if self.drawn == 3 {
            startup::log(&format!("Drawing: {}", self.engine.word()));
            match self.crash_later.as_deref() {
                Some("panic") => panic!("This is a test of the crash report, asked for with --test-crash"),
                Some("abort") => std::process::abort(),
                _ => {}
            }
        }
        // A picture of the window for a report, which may come to want it.
        if let Some(slot) = self.app.want_picture.take() {
            ctx.request_screenshot(move |image| {
                if let Ok(mut held) = slot.lock() {
                    *held = Some((*image).clone());
                }
            });
        }
        // Saving a picture of the 3D view from the preview panel.
        if let Some(path) = self.app.save_preview.take() {
            let area = self.app.view_rect;
            let ppp = ctx.pixels_per_point();
            ctx.request_screenshot(move |image| {
                let _ = write_png(&path, &image.region(&area, Some(ppp)));
            });
        }
        if self.screenshot.is_some() {
            self.frames += 1;
            ctx.request_repaint();
            if self.frames >= 120 && !self.requested {
                self.requested = true;
                let path = self.screenshot.clone().unwrap();
                ctx.request_screenshot(move |image| match write_png(&path, &image) {
                    Ok(()) => {
                        startup::log(startup::CLOSED);
                        std::process::exit(0)
                    }
                    Err(e) => {
                        eprintln!("Could not save {}: {e}", path.display());
                        std::process::exit(1);
                    }
                });
            }
        }
    }
}

/// Keeps the panels usable in narrow windows by drawing smaller, down to real pixel size.
fn fit_scale(ctx: &egui::Context) {
    const NARROWEST: f32 = 1300.0;
    let Some(native) = ctx.native_pixels_per_point() else { return };
    let pixels = ctx.content_rect().width() * ctx.pixels_per_point();
    let wanted = (pixels / NARROWEST).clamp(native.min(1.0), native);
    if (wanted - ctx.pixels_per_point()).abs() > 0.01 {
        ctx.set_zoom_factor(wanted / native);
    }
}

pub(crate) fn write_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty() && !platform::WEB) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut file, image.size[0] as u32, image.size[1] as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
        writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
    }
    platform::write(path, file).map_err(|e| e.to_string())
}

/// A square part held by tabs, cut with a 6 mm tool, with one tab in the middle of the job.
fn tab_demo(app: &mut App, tab_length: f64) {
    use toolpath::{blank_op, default_tools, generate, JobSize, OpKind, Side};
    app.load_sample();
    app.doc = app::Doc::default();
    app.doc.set_units(app::Units::Mm);
    let job = &mut app.doc.job;
    (job.width, job.height, job.thickness) = (240.0, 240.0, 12.0);
    let size = JobSize { width: 240.0, height: 240.0, thickness: 12.0 };
    let machine = app.doc.machine_on_job();
    let mut tool = default_tools().into_iter().find(|t| t.metric && t.kind == relief::ToolKind::EndMill).expect("an end mill in millimetres");
    tool.name = "End Mill (6 mm)".into();
    (tool.dia, tool.pass_depth) = (6.0, 6.0);
    let op_at = |c: [f64; 2]| {
        let mut op = blank_op(OpKind::Profile, "Part with tabs".into(), tool.clone(), 12.0, vec![geom::Path::closed(geom::rect(c, 80.0, 80.0, 0.0))]);
        op.side = Side::Outside;
        op.tabs = true;
        op.tab_length = tab_length;
        op.tab_thickness = 3.0;
        op.tab_count = 4;
        op
    };
    // Where the first tab falls, to move the part so it's in the middle.
    let first = generate(&op_at([120.0, 120.0]), machine, &size, None).expect("the demo's toolpath");
    let raised: Vec<[f64; 3]> = first.iter().filter(|m| !m.rapid && (m.p[2] + 9.0).abs() < 1e-9).map(|m| m.p).collect();
    let end = raised.iter().position(|p| (p[0] - raised[0][0]).hypot(p[1] - raised[0][1]) > 30.0).unwrap_or(raised.len());
    let (a, b) = (raised[0], raised[end - 1]);
    let centre = [240.0 - (a[0] + b[0]) / 2.0, 240.0 - (a[1] + b[1]) / 2.0];
    let op = op_at(centre);
    let moves = generate(&op, machine, &size, None).expect("the demo's toolpath");
    app.keep_calculated(vec![(op, std::sync::Arc::new(moves))], None);
    app.message = None;
    app.view3d = true;
    app.preview(vec![0]);
    app.message = None;
    app.finish_preview();
    app.doc.toolpaths[0].visible = false;
    app.right_open = false;
    app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 35f32.to_radians(), zoom: 12.0, pan: egui::Vec2::ZERO };
}

/// Opens one of the known screens, for checking the look against the pictures.
fn open_state(app: &mut App, state: &str) {
    use app::{Datum, DrawForm, DrawKind, LeftForm, Prim, RightMode};
    // The numbers below are in inches; only this one shows a new job as it really starts.
    app.metric_by_default = state == "millimetres";
    app.background_recalc = false;
    app.surface_in_background = false;
    // A real job carved in the preview, for looking at it closely: BURIN_CAM is zoom,yaw,tilt,pan x,pan y.
    if let Some(path) = state.strip_prefix("file:") {
        if let Err(why) = app.open_path(path.into()) {
            eprintln!("{why}");
        }
        app.right_open = true;
        app.right = RightMode::Preview;
        app.view3d = true;
        let all: Vec<usize> = (0..app.doc.toolpaths.len()).collect();
        let started = crate::platform::Instant::now();
        app.preview(all);
        app.message = None;
        app.finish_preview();
        eprintln!("carved in {:?}, {} cells", started.elapsed(), app.sim.as_ref().map_or(0, |s| s.grid.data.len()));
        for t in &mut app.doc.toolpaths {
            t.visible = false;
        }
        let cam: Vec<f32> = std::env::var("BURIN_CAM").unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if let [zoom, yaw, tilt, x, y] = cam[..] {
            app.cam = view3d::Camera { yaw: yaw.to_radians(), tilt: tilt.to_radians(), zoom, pan: egui::vec2(x, y) };
        }
        return;
    }
    let circle = |app: &mut App| {
        let (c, r) = ([4.3034, 5.9983], 1.2);
        let id = app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, r))], Prim::Circle { c, r });
        app.selected = vec![id];
    };
    match state {
        "job" => app.new_file(),
        "drawing" => {
            app.new_file();
            app.left = LeftForm::None;
        }
        "circle" => {
            app.new_file();
            circle(app);
            let mut f = DrawForm::new(DrawKind::Circle, &app.doc.job);
            f.c = [4.3034, 5.9983];
            f.r = 1.2;
            app.left = LeftForm::Draw(f);
            app.right_open = true;
            app.right = RightMode::List;
        }
        "smooth" => {
            app.load_sample();
            app.selected_comp = Some(0);
            app.smooth_form = Some(modeltools::SmoothForm::default());
            app.view3d = false;
        }
        "modeltrim" => {
            app.load_sample();
            let cut = app.doc.add_vector(vec![geom::Path::closed(geom::rect([6.2, 6.0], 2.4, 5.0, 0.0))], Prim::Free);
            app.selected = vec![cut];
            app.selected_comp = Some(1);
            app.reset_component(true);
            app.selected_comp = Some(1);
            app.boundary_around_component();
            app.view3d = false;
        }
        "extrude" => {
            app.new_file();
            app.left = LeftForm::None;
            // Two loops crossing each other, woven, with a rounded cross section.
            let a = app.doc.add_vector(vec![geom::Path::closed(geom::circle([4.0, 5.0], 2.5))], Prim::Free);
            let b = app.doc.add_vector(vec![geom::Path::closed(geom::circle([6.0, 5.0], 2.5))], Prim::Free);
            let section = app.doc.add_vector(vec![geom::Path::open((0..=16).map(|k| { let u = k as f64 / 16.0; [1.0 + u * 0.6, 9.0 + 0.2 * (std::f64::consts::PI * u).sin()] }).collect())], Prim::Free);
            app.selected = vec![a, b, section];
            app.open_extrude();
            if let Some(mut f) = app.extrude_form.clone() {
                f.weave = true;
                app.extrude(&f);
                app.extrude_form = Some(f);
            }
            app.selected.clear();
            app.left_tab = app::LeftTab::Modeling;
            app.view3d = true;
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 45f32.to_radians(), zoom: 1.4, pan: egui::Vec2::ZERO };
        }
        "extrudecomp" => {
            app.new_file();
            app.left = LeftForm::None;
            // A rounded bead, used as the cross section and repeated along a wave.
            app.doc.components.push(relief::Component { name: "Bead".into(), paths: vec![geom::Path::closed(geom::circle([1.0, 9.0], 0.45))], profile: relief::Profile::Round, height: 0.35, base: 0.0, combine: relief::Combine::Add, visible: false, surface: None, whole: false });
            let wave = app.doc.add_vector(vec![geom::Path::open((0..=60).map(|k| { let u = k as f64 / 60.0; [1.5 + u * 7.0, 5.0 + 1.6 * (std::f64::consts::TAU * u).sin()] }).collect())], Prim::Free);
            app.selected = vec![wave];
            app.open_extrude();
            app.selected_comp = Some(0);
            if let Some(f) = app.extrude_form.clone() {
                app.extrude(&f);
                app.extrude_form = Some(f);
            }
            app.selected.clear();
            app.left_tab = app::LeftTab::Modeling;
            app.view3d = true;
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 45f32.to_radians(), zoom: 1.4, pan: egui::Vec2::ZERO };
        }
        "adaptive" => {
            app.new_file();
            app.left = LeftForm::None;
            // An L-shaped pocket cleared adaptively with a quarter inch cutter.
            let shape = geom::Path::closed(vec![[2.0, 2.0], [8.0, 2.0], [8.0, 4.5], [4.5, 4.5], [4.5, 8.0], [2.0, 8.0]]);
            app.doc.add_vector(vec![shape.clone()], Prim::Free);
            let machine = app.doc.machine_on_job();
            let size = toolpath::JobSize { width: app.doc.job.width, height: app.doc.job.height, thickness: app.doc.job.thickness };
            let tool = toolpath::default_tools().into_iter().find(|t| !t.metric && t.kind == relief::ToolKind::EndMill).expect("an end mill in inches");
            let mut op = toolpath::blank_op(toolpath::OpKind::Pocket, "Adaptive Pocket".into(), tool, 0.25, vec![shape]);
            op.adaptive = true;
            op.ramp = true;
            op.ramp_kind = toolpath::RampKind::Spiral;
            let moves = toolpath::generate(&op, machine, &size, None).expect("the demo's toolpath");
            app.keep_calculated(vec![(op, std::sync::Arc::new(moves))], None);
            app.message = None;
            app.selected.clear();
        }
        "restpocket" => {
            app.new_file();
            app.left = LeftForm::None;
            // A room with a narrow slot: a 1 inch tool clears the room, a 1/8 inch one the rest.
            let shape = geom::Path::closed(vec![[2.0, 2.0], [7.0, 2.0], [7.0, 6.0], [4.8, 6.0], [4.8, 8.5], [4.2, 8.5], [4.2, 6.0], [2.0, 6.0]]);
            app.doc.add_vector(vec![shape.clone()], Prim::Free);
            let machine = app.doc.machine_on_job();
            let size = toolpath::JobSize { width: app.doc.job.width, height: app.doc.job.height, thickness: app.doc.job.thickness };
            let mut small = toolpath::default_tools().into_iter().find(|t| !t.metric && t.kind == relief::ToolKind::EndMill).expect("an end mill in inches");
            small.dia = 0.125;
            small.name = "End Mill (1/8 inch)".into();
            let mut big = small.clone();
            big.dia = 1.0;
            big.name = "End Mill (1 inch)".into();
            let mut clear = toolpath::blank_op(toolpath::OpKind::Pocket, "Pocket [Clear]".into(), big.clone(), 0.25, vec![shape.clone()]);
            clear.clearance_tool = None;
            let mut rest = toolpath::blank_op(toolpath::OpKind::Pocket, "Pocket".into(), small, 0.25, vec![shape]);
            rest.clearance_tool = Some(big);
            rest.rest_only = true;
            let a = toolpath::generate(&clear, machine, &size, None).expect("the demo's toolpath");
            let b = toolpath::generate(&rest, machine, &size, None).expect("the demo's toolpath");
            app.keep_calculated(vec![(clear, std::sync::Arc::new(a)), (rest, std::sync::Arc::new(b))], None);
            app.message = None;
            app.view3d = true;
            app.preview(vec![0, 1]);
            app.finish_preview();
            app.cam = view3d::Camera { yaw: 0f32.to_radians(), tilt: 70f32.to_radians(), zoom: 1.6, pan: egui::Vec2::ZERO };
        }
        "nestshape" => {
            app.new_file();
            // Triangles, L shapes and rings nested by their true shape, turning and mirroring.
            let mut ids = Vec::new();
            for k in 0..6 {
                let (x, y) = (0.3 + k as f64 * 1.6, 9.5);
                ids.push(app.doc.add_vector(vec![geom::Path::closed(vec![[x, y - 3.0], [x + 3.0, y - 3.0], [x, y]])], Prim::Free));
            }
            for k in 0..4 {
                let (x, y) = (0.3 + k as f64 * 2.4, 5.5);
                ids.push(app.doc.add_vector(vec![geom::Path::closed(vec![[x, y - 2.5], [x + 2.0, y - 2.5], [x + 2.0, y - 1.7], [x + 0.8, y - 1.7], [x + 0.8, y], [x, y]])], Prim::Free));
            }
            for k in 0..3 {
                let c = [1.5 + k as f64 * 3.0, 1.2];
                ids.push(app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, 1.0)), geom::Path::closed(geom::circle(c, 0.6))], Prim::Free));
            }
            app.selected = ids;
            app.start_xform(app::Xform::Nest);
            if let LeftForm::Xform(f) = &mut app.left {
                f.by_shape = true;
                f.nest_turn = true;
                f.nest_mirror = true;
            }
            app.apply_xform();
        }
        "nestoutline" => {
            app.new_file();
            // Parts nested inside a round outline chosen last.
            let mut ids: Vec<u64> = (0..14).map(|k| app.doc.add_vector(vec![geom::Path::closed(geom::rect([0.8 + (k % 7) as f64 * 1.3, 9.0 - (k / 7) as f64 * 0.6], 1.0, 0.5 + 0.1 * (k % 3) as f64, 0.0))], Prim::Free)).collect();
            ids.push(app.doc.add_vector(vec![geom::Path::closed(geom::circle([5.0, 4.0], 3.6))], Prim::Free));
            app.selected = ids;
            app.start_xform(app::Xform::Nest);
            if let LeftForm::Xform(f) = &mut app.left {
                f.into_boundary = true;
            }
            app.apply_xform();
        }
        "partinpart" => {
            app.new_file();
            // Two frames and a handful of small squares: the squares go in the frames' holes.
            let mut ids = Vec::new();
            for k in 0..2 {
                let c = [3.0 + k as f64 * 4.0, 7.0];
                ids.push(app.doc.add_vector(vec![geom::Path::closed(geom::rect(c, 4.0, 4.0, 0.0)), geom::Path::closed(geom::rect(c, 3.0, 3.0, 0.0))], Prim::Free));
            }
            for k in 0..9 {
                ids.push(app.doc.add_vector(vec![geom::Path::closed(geom::circle([1.0 + k as f64 * 1.0, 2.0], 0.45))], Prim::Free));
            }
            app.selected = ids;
            app.start_xform(app::Xform::Nest);
            if let LeftForm::Xform(f) = &mut app.left {
                f.part_in_part = true;
            }
            app.apply_xform();
        }
        "nestturn" => {
            app.new_file();
            app.doc.job.height = 5.0;
            // Parts taller than the sheet, turned to lie along the rows.
            let ids: Vec<u64> = (0..3).map(|k| app.doc.add_vector(vec![geom::Path::closed(geom::rect([2.0 + k as f64 * 1.5, 2.5], 1.0, 6.0, 0.0))], Prim::Free)).collect();
            app.selected = ids;
            app.start_xform(app::Xform::Nest);
            if let LeftForm::Xform(f) = &mut app.left {
                f.nest_turn = true;
            }
            app.apply_xform();
        }
        "nestsheets" => {
            app.new_file();
            app.left = LeftForm::None;
            // Twelve 3 inch squares nested on a 10 inch sheet: the ones that don't fit go on sheet 2.
            let mut ids = Vec::new();
            for k in 0..12 {
                let c = [1.0 + (k % 4) as f64 * 0.5, 1.0 + (k / 4) as f64 * 0.5];
                ids.push(app.doc.add_vector(vec![geom::Path::closed(geom::rect(c, 3.0, 3.0, 0.0))], Prim::Free));
            }
            app.selected = ids;
            app.start_xform(app::Xform::Nest);
            app.apply_xform();
            app.left = LeftForm::None;
            app.show_sheet(1);
            app.left_tab = app::LeftTab::Layers;
        }
        "sheets" => {
            app.new_file();
            app.left = LeftForm::None;
            // Two sheets: a circle on the first, a row of squares on the second, which shows.
            circle(app);
            app.add_sheet();
            for k in 0..3 {
                app.doc.add_vector(vec![geom::Path::closed(geom::rect([2.5 + 2.5 * k as f64, 5.0], 1.5, 1.5, 0.0))], Prim::Free);
            }
            app.selected.clear();
            app.left_tab = app::LeftTab::Layers;
        }
        "gridspacing" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.grid = true;
            app.snap = true;
            app.grid_spacing = 0.5;
        }
        "railpairs" => {
            app.new_file();
            app.left = LeftForm::None;
            // Two frames swept at once: two pairs of closed rails, one moulding.
            let rect = |c: [f64; 2], w: f64, h: f64| geom::Path::closed(geom::rect(c, w, h, 0.0));
            let o1 = app.doc.add_vector(vec![rect([3.0, 5.0], 4.0, 6.0)], Prim::Free);
            let i1 = app.doc.add_vector(vec![rect([3.0, 5.0], 2.6, 4.6)], Prim::Free);
            let o2 = app.doc.add_vector(vec![rect([7.5, 5.0], 4.0, 4.0)], Prim::Free);
            let i2 = app.doc.add_vector(vec![rect([7.5, 5.0], 2.6, 2.6)], Prim::Free);
            let moulding = app.doc.add_vector(vec![geom::Path::open(vec![[1.0, 9.6], [1.2, 9.75], [1.5, 9.8], [1.7, 9.95], [2.0, 9.98], [2.4, 9.9], [2.6, 9.6]])], Prim::Free);
            app.selected = vec![o1, i1, moulding];
            app.open_two_rail_sweep();
            if let Some(mut f) = app.sweep_form.clone() {
                f.rails.extend([o2, i2]);
                app.two_rail_sweep(&f);
                app.sweep_form = Some(f);
            }
            app.selected.clear();
            app.left_tab = app::LeftTab::Modeling;
            app.view3d = true;
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 45f32.to_radians(), zoom: 1.3, pan: egui::Vec2::ZERO };
        }
        "railsweep" => {
            app.new_file();
            app.left = LeftForm::None;
            // A frame: two closed rails, a moulding's cross section, the middle filled.
            let outer = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 8.0, 6.0, 0.0))], Prim::Free);
            let inner = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 6.0, 4.0, 0.0))], Prim::Free);
            let moulding = app.doc.add_vector(vec![geom::Path::open(vec![[1.0, 8.6], [1.2, 8.75], [1.5, 8.8], [1.7, 9.05], [2.0, 9.1], [2.4, 8.95], [2.6, 8.6]])], Prim::Free);
            app.selected = vec![outer, inner, moulding];
            app.open_two_rail_sweep();
            if let Some(mut f) = app.sweep_form.clone() {
                f.fill_centre = true;
                app.two_rail_sweep(&f);
                app.sweep_form = Some(f);
            }
            app.left_tab = app::LeftTab::Modeling;
            app.view3d = true;
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 45f32.to_radians(), zoom: 1.3, pan: egui::Vec2::ZERO };
        }
        "emboss" => {
            app.load_sample();
            app.left = LeftForm::None;
            app.selected.clear();
            let n = app.doc.components.len();
            for i in 0..n {
                app.selected_comp = Some(i);
                app.emboss_component(modeltools::EmbossForm { scale: 0.3, smoothness: 0.2 });
            }
            app.left_tab = app::LeftTab::Modeling;
            app.reset_preview();
            app.show_toolpaths = false;
            app.view3d = true;
            app.right_open = false;
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 50f32.to_radians(), zoom: 1.3, pan: egui::Vec2::ZERO };
        }
        "sculpt" => {
            app.new_file();
            app.left = LeftForm::None;
            let disc = app.doc.add_vector(vec![geom::Path::closed(geom::circle([5.0, 5.0], 3.5))], Prim::Free);
            app.selected = vec![disc];
            app.open_shape_form(None);
            if let LeftForm::Shape(mut f) = app.left.clone() {
                f.profile = relief::Profile::Round;
                f.height = 0.3;
                app.add_component(f);
            }
            app.left = LeftForm::None;
            app.selected.clear();
            app.selected_comp = Some(0);
            app.open_sculpt();
            // A ridge deposited across, and a dent removed beside it.
            if let Some(s) = app.sculpt.as_mut() {
                s.tool = sculpt::SculptTool::Deposit;
                s.strength = 1.0;
                s.diameter = 1.0;
            }
            for k in 0..30 {
                app.sculpt_at([3.0 + k as f64 * 0.13, 5.6], k == 0, false);
            }
            if let Some(s) = app.sculpt.as_mut() {
                s.tool = sculpt::SculptTool::Remove;
            }
            for _ in 0..8 {
                app.sculpt_at([5.5, 3.8], true, false);
            }
            if let Some(s) = app.sculpt.as_mut() {
                s.tool = sculpt::SculptTool::Smooth;
            }
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 45f32.to_radians(), zoom: 1.4, pan: egui::Vec2::ZERO };
        }
        "kerning" => {
            app.new_file();
            app.left = LeftForm::None;
            let mut style = fonts::TextStyle::default();
            let text = "WAVE\nWOOD";
            // A few clicks between the letters, as one would: W and A closer, A and V closer, the lines closer.
            for gap in [fonts::TextGap::Letter(1), fonts::TextGap::Letter(1), fonts::TextGap::Letter(2), fonts::TextGap::Line(1)] {
                fonts::adjust_gap(text, 1.4, [2.0, 6.0], &mut style, gap, false, true);
            }
            let plain = Prim::Text { at: [2.0, 9.0], height: 1.4, text: "WAVE".into(), style: fonts::TextStyle::default() };
            app.doc.add_vector(plain.paths(), plain);
            let prim = Prim::Text { at: [2.0, 6.0], height: 1.4, text: text.into(), style };
            let id = app.doc.add_vector(prim.paths(), prim);
            app.selected = vec![id];
        }
        "upright" => {
            app.new_file();
            let prim = Prim::Text { at: [5.0, 8.5], height: 1.0, text: "BOIS\nFER".into(), style: fonts::TextStyle { vertical: true, ..Default::default() } };
            app.doc.add_vector(prim.paths(), prim);
            let mut f = DrawForm::new(DrawKind::Text, &app.doc.job);
            f.text = "BOIS\nFER".into();
            f.text_style = fonts::TextStyle { vertical: true, ..Default::default() };
            app.left = LeftForm::Draw(f);
        }
        "textfit" => {
            app.new_file();
            let plate = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 8.0, 3.0, 0.4))], Prim::Rect { c: [5.0, 5.0], w: 8.0, h: 3.0, corner: 0.4 });
            app.selected = vec![plate];
            let mut f = DrawForm::new(DrawKind::Text, &app.doc.job);
            f.text = "Atelier".into();
            f.fill_percent = 85.0;
            if let Some((at, height)) = fonts::fit_in_box(&f.text, &f.text_style, [1.0, 3.5], [9.0, 6.5], f.fill_percent) {
                f.c = at;
                f.text_height = height;
            }
            app.left = LeftForm::Draw(f);
        }
        "fonts" | "fonts-symbols" => {
            app.new_file();
            let started = crate::platform::Instant::now();
            let installed = fonts::families();
            eprintln!("{} font families found in {:?}", installed.len(), started.elapsed());
            let system = ["Acens", "DejaVu Serif", "DejaVu Sans"].iter().find(|n| installed.iter().any(|f| f.name == **n)).map_or(String::new(), |n| n.to_string());
            let lines = [
                (8.0, fonts::TextStyle::default(), "Burin"),
                (5.5, fonts::TextStyle { font: "Single Line Sans".into(), ..Default::default() }, "Single line"),
                (3.0, fonts::TextStyle { font: "Single Line Script".into(), italic: false, ..Default::default() }, "Script"),
                (0.8, fonts::TextStyle { font: system.clone(), bold: true, ..Default::default() }, "Bois & Fer"),
            ];
            for (y, style, text) in lines {
                let prim = Prim::Text { at: [1.0, y], height: 1.2, text: text.into(), style };
                app.doc.add_vector(prim.paths(), prim);
            }
            let mut f = DrawForm::new(DrawKind::Text, &app.doc.job);
            f.text = "Bois & Fer".into();
            app.open_font_list = Some(if state == "fonts-symbols" { "Awesome".into() } else { String::new() });
            f.text_style = fonts::TextStyle { font: system, bold: true, ..Default::default() };
            app.left = LeftForm::Draw(f);
        }
        "profile" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.start_op(toolpath::OpKind::Profile);
        }
        "3d" | "cut" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.start_op(toolpath::OpKind::Profile);
            app.calculate_op();
            if state == "cut" {
                app.preview(vec![0]);
                app.finish_preview();
                app.doc.toolpaths[0].visible = false;
            }
        }
        "tools" => {
            app.new_file();
            app.left = LeftForm::None;
            app.right_open = true;
            app.start_op(toolpath::OpKind::Profile);
            app.open_tool_list(Some(0));
        }
        "toolgroups" => {
            app.new_file();
            app.left = LeftForm::None;
            app.right_open = true;
            app.start_op(toolpath::OpKind::Profile);
            app.open_tool_list(Some(0));
            app.tool_groups_edit = vec!["Roughing/Big".into(), "Finishing".into()];
            if let Some(tools) = app.tool_edit.as_mut() {
                tools[2].group = "Roughing/Big".into();
                tools[1].group = "Roughing".into();
            }
            app.tool_group_picked = Some("Roughing/Big".into());
        }
        "modeling" | "clipart" | "layers" | "material" | "pocket" | "vcarve" | "rough" | "finish" | "drill" | "engrave" | "save" | "machining" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.right_open = true;
            match state {
                "modeling" => app.left_tab = app::LeftTab::Modeling,
                "clipart" => app.left_tab = app::LeftTab::Clipart,
                "layers" => app.left_tab = app::LeftTab::Layers,
                "material" => app.right = RightMode::Material(Box::new(app.doc.clone())),
                "pocket" => app.start_op(toolpath::OpKind::Pocket),
                "vcarve" => app.start_op(toolpath::OpKind::VCarve),
                "rough" => app.start_op(toolpath::OpKind::Rough3D),
                "finish" => app.start_op(toolpath::OpKind::Finish3D),
                "drill" => app.start_op(toolpath::OpKind::Drill),
                "engrave" => app.start_op(toolpath::OpKind::Engrave),
                // The Machining workspace with its list of operations.
                "machining" => app.set_persona(app::Persona::Machining),
                _ => app.right = RightMode::Save,
            }
        }
        "posts" => {
            const DEMO: &str = "POST_NAME = \"Router G-code (mm) (*.tap)\"\nFILE_EXTENSION = \"tap\"\nUNITS = \"MM\"\nSUBSTITUTE = \"([91])[93]\"\nVAR X_POSITION = [X|C| X|1.3]\nbegin HEADER\n\"G21 [UNKNOWN_FIELD]\"\nbegin RAPID_MOVE\n\"G0[X]\"\nbegin FEED_MOVE\n\"G1[X]\"\n";
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.start_op(toolpath::OpKind::Profile);
            app.calculate_op();
            app.view3d = false;
            app.right_open = true;
            app.right = RightMode::Save;
            app.loaded_posts.push(postp::PostDef::parse(DEMO, &|_| None).unwrap());
            app.post_index = toolpath::POSTS.len();
        }
        "trim_before" | "trim_after" => {
            use constraints::{DimMode, Elem, Kind};
            app.new_file();
            app.left = LeftForm::None;
            let plate = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 6.0, 4.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 6.0, h: 4.0, corner: 0.0 });
            app.doc.add_vector(vec![geom::Path::open(vec![[5.0, 1.5], [5.0, 8.5]])], Prim::Free);
            app.add_constraint(Kind::Fixed, vec![Elem::Node { v: plate, path: 0, node: 0 }]);
            app.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 0 }], DimMode::Smart);
            app.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 1 }], DimMode::Smart);
            app.dim_edit = None;
            app.selected.clear();
            app.start_xform(app::Xform::Trim);
            if state == "trim_after" {
                app.trim_at([6.5, 3.0], 0.1, false);
            }
        }
        "tee_before" | "tee_after" => {
            use constraints::{DimMode, Elem, Kind};
            app.new_file();
            app.left = LeftForm::None;
            let mut rect_at = |c: [f64; 2], w: f64, h: f64| app.doc.add_vector(vec![geom::Path::closed(geom::rect(c, w, h, 0.0))], Prim::Rect { c, w, h, corner: 0.0 });
            let base = rect_at([5.0, 3.0], 6.0, 2.0);
            let post = rect_at([5.0, 5.5], 2.0, 5.0);
            app.add_constraint(Kind::Fixed, vec![Elem::Node { v: base, path: 0, node: 0 }]);
            app.make_dimension(&[Elem::Seg { v: base, path: 0, seg: 0 }], DimMode::Smart);
            app.make_dimension(&[Elem::Seg { v: base, path: 0, seg: 1 }], DimMode::Smart);
            app.make_dimension(&[Elem::Seg { v: post, path: 0, seg: 2 }], DimMode::Smart);
            app.dim_edit = None;
            app.trim_at([5.0, 4.0], 0.05, false);
            app.trim_at([5.0, 3.0], 0.05, false);
            app.rejoin_all(0.01);
            app.maintain_constraints();
            app.drop_stale_envelope();
            if state == "tee_after" {
                let i = app.doc.constraints.iter().position(|c| c.kind == Kind::Distance && c.items.iter().any(|e| e.vector_id() == post)).unwrap();
                app.set_dimension(i, 3.5);
            }
            app.selected.clear();
            app.start_constraint_form();
        }
        "sweep" => {
            app.new_file();
            app.left = LeftForm::None;
            for y in [3.0, 4.5, 6.0, 7.5] {
                app.doc.add_vector(vec![geom::Path::open(vec![[1.0, y], [9.0, y]])], Prim::Free);
            }
            app.doc.add_vector(vec![geom::Path::open(vec![[5.0, 1.0], [5.0, 9.0]])], Prim::Free);
            app.selected.clear();
            app.start_xform(app::Xform::Trim);
            for y in [3.0, 4.5, 6.0] {
                app.trim_at([7.0, y], 0.05, false);
            }
        }
        "origin" | "origin_set" => {
            app.new_file();
            app.confirm_job();
            let circle = app.doc.add_vector(vec![geom::Path::closed(geom::circle([5.0, 5.0], 2.0))], Prim::Free);
            app.selected = vec![circle];
            app.doc.job.datum = Datum::Centre;
            if state == "origin_set" {
                app.open_job_setup();
            }
        }
        "relief" | "relief3d" => {
            app.new_file();
            app.confirm_job();
            app.open_relief_picture("sample.png".into(), &demo_picture());
            if state == "relief3d" {
                if let LeftForm::Relief(f) = app.left.clone() {
                    app.apply_relief(&f);
                }
            }
        }
        "flute" | "chamfer" => {
            app.new_file();
            app.confirm_job();
            let line = geom::Path::open(vec![[2.0, 2.0], [8.0, 8.0]]);
            let shape = app.doc.add_vector(vec![line], Prim::Free);
            app.selected = vec![shape];
            app.start_op(if state == "flute" { toolpath::OpKind::Flute } else { toolpath::OpKind::Chamfer });
        }
        "editcircle" => {
            app.new_file();
            app.confirm_job();
            let circle = Prim::Circle { c: [4.0, 6.0], r: 1.5 };
            let id = app.doc.add_vector(circle.paths(), circle);
            app.selected = vec![id];
            app.start_draw(DrawKind::Circle);
        }
        "millimetres" => {
            app.new_file();
            app.confirm_job();
            let (c, r) = ([300.0, 200.0], 80.0);
            let id = app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, r))], Prim::Circle { c, r });
            app.selected = vec![id];
            app.right_open = true;
            app.start_op(toolpath::OpKind::Profile);
            app.calculate_op();
            app.selected = vec![id];
            app.start_op(toolpath::OpKind::Pocket);
            app.calculate_op();
            app.view3d = false;
            app.start_xform(app::Xform::Move);
            app.right = RightMode::List;
        }
        "progress" => {
            app.new_file();
            app.confirm_job();
            circle(app);
            let (send, messages) = std::sync::mpsc::channel::<recalc::Message>();
            let names = ["Outline 1", "Pocket 2", "Drilling 3"];
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(120));
                drop(send);
            });
            app.recalc = Some(recalc::Running { total: names.len(), results: Vec::new(), current: names[1].into(), messages, stop: Default::default(), started: crate::platform::Instant::now() - std::time::Duration::from_secs(3), finish: None });
        }
        "sharp3d" | "sharp3dfar" | "toodeep" => {
            app.metric_by_default = true;
            app.new_file();
            app.confirm_job();
            let square = app.doc.add_vector(vec![geom::Path::closed(geom::rect([180.0, 210.0], 240.0, 180.0, 0.0))], Prim::Free);
            let round = app.doc.add_vector(vec![geom::Path::closed(geom::circle([440.0, 240.0], 90.0))], Prim::Free);
            let star = app.doc.add_vector(vec![geom::Path::closed(vec![[100.0, 40.0], [330.0, 60.0], [400.0, 30.0], [520.0, 110.0], [250.0, 95.0]])], Prim::Free);
            app.right_open = true;
            let pass = |app: &mut App, ids: Vec<u64>, kind, depth: f64, setup: &dyn Fn(&mut toolpath::Op)| {
                app.selected = ids;
                app.start_op(kind);
                if let RightMode::Op { op, .. } = &mut app.right {
                    op.cut_depth = depth;
                    op.tool.pass_depth = 100.0;
                    setup(op);
                }
                app.calculate_op();
            };
            pass(app, vec![square], toolpath::OpKind::Pocket, 6.0, &|_| {});
            pass(app, vec![round], toolpath::OpKind::Profile, 18.8, &|op| op.side = toolpath::Side::Inside);
            pass(app, vec![star], toolpath::OpKind::Pocket, 2.0, &|_| {});
            app.selected.clear();
            if state == "toodeep" {
                app.view3d = false;
                app.right = RightMode::List;
                app.recalculate_all();
            } else {
                app.right = RightMode::Preview;
                app.view3d = true;
                app.preview(vec![0, 1, 2]);
                app.message = None;
                app.finish_preview();
                for t in &mut app.doc.toolpaths {
                    t.visible = false;
                }
                if state == "sharp3d" {
                    app.cam.zoom = 3.0;
                }
            }
        }
        "board" => {
            app.metric_by_default = true;
            app.new_file();
            app.doc.job.thickness = 13.0;
            app.doc.job.spoil_board = 3.0;
        }
        "boardmaterial" | "boardalert" => {
            app.metric_by_default = true;
            app.new_file();
            app.confirm_job();
            app.doc.job.thickness = 13.0;
            app.doc.job.spoil_board = 3.0;
            let id = app.doc.add_vector(vec![geom::Path::closed(geom::circle([300.0, 200.0], 80.0))], Prim::Free);
            app.selected = vec![id];
            app.right_open = true;
            app.start_op(toolpath::OpKind::Profile);
            if let RightMode::Op { op, .. } = &mut app.right {
                op.cut_depth = 16.5;
                op.tool.pass_depth = 100.0;
            }
            app.calculate_op();
            app.view3d = false;
            if state == "boardmaterial" {
                app.right = RightMode::Material(Box::new(app.doc.clone()));
            } else {
                app.right = RightMode::List;
                app.recalculate_all();
            }
        }
        "zero" => {
            app.metric_by_default = true;
            app.new_file();
            app.confirm_job();
            let id = app.doc.add_vector(vec![geom::Path::closed(geom::circle([300.0, 200.0], 80.0))], Prim::Free);
            app.selected = vec![id];
            app.doc.job.set_origin([317.0, 228.0]);
            app.grid = true;
            app.start_xform(app::Xform::Move);
        }
        "toolrates" => {
            app.new_file();
            app.confirm_job();
            circle(app);
            app.right_open = true;
            app.start_op(toolpath::OpKind::Profile);
            app.edit_passes = true;
            if let RightMode::Op { op, .. } = &mut app.right {
                op.tool.per_second = true;
            }
        }
        "ramp" | "leads" | "order" | "startat" | "corners" | "passes" | "selector" | "through" => {
            app.new_file();
            app.confirm_job();
            circle(app);
            app.right_open = true;
            app.show_advanced = true;
            app.start_op(toolpath::OpKind::Profile);
            if let RightMode::Op { tab, op, .. } = &mut app.right {
                *tab = match state {
                    "leads" => 1,
                    "order" => 2,
                    "startat" => 3,
                    "corners" => 4,
                    _ => 0,
                };
                if state == "through" {
                    op.cut_depth = 10.0;
                }
                op.ramp = state == "ramp";
                op.lead = state == "leads";
                op.ramp_kind = toolpath::RampKind::Smooth;
            }
            if let RightMode::Op { op, .. } = app.right.clone() {
                match state {
                    "passes" => {
                        let mut o = (*op).clone();
                        o.cut_depth = 1.0;
                        o.tool.pass_depth = 0.25;
                        app.right = RightMode::Op { op: Box::new(o.clone()), editing: None, tab: 0, error: None };
                        app.open_passes(&o);
                    }
                    "selector" => app.open_selector(&op),
                    "through" => app.confirm_depth_then_calculate(),
                    _ => {}
                }
            }
        }
        "trace" | "traced" => {
            app.new_file();
            app.left = LeftForm::None;
            app.place_bitmap("sample.png".into(), "sample".into(), demo_picture());
            app.start_trace();
            if let LeftForm::Trace(f) = &mut app.left {
                f.color = true;
            }
            if state == "traced" {
                let mut f = bitmap::TraceForm { color: true, ..bitmap::TraceForm::new() };
                f.sorted = Some(bitmap::sort_colors(&demo_picture(), 4));
                f.ticked = vec![true, true, true, false];
                app.apply_trace(&f);
                app.selected.clear();
            }
        }
        "distort" | "distorted" => {
            app.new_file();
            app.left = LeftForm::None;
            let text = Prim::Text { at: [2.2, 4.0], height: 1.6, text: "Burin".into(), style: Default::default() };
            let word = app.doc.add_vector(text.paths(), text);
            app.selected = vec![word];
            app.start_xform(app::Xform::Distort);
            if state == "distorted" {
                app.apply_envelope(distort::EnvelopeKind::BoundingBox);
                let c = app.envelope.as_ref().unwrap().corners;
                app.move_envelope_corner(2, [c[2][0] + 0.5, c[2][1] + 2.2]);
                app.move_envelope_corner(3, [c[3][0] - 0.5, c[3][1] + 0.6]);
                app.selected = vec![word];
            }
        }
        "grid" | "grid3d" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.selected.clear();
            app.grid = true;
            app.view3d = state == "grid3d";
        }
        "sketch" => {
            use constraints::{DimMode, Elem, Kind};
            app.new_file();
            app.left = LeftForm::None;
            app.right_open = false;
            let plate = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 7.0, 5.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 7.0, h: 5.0, corner: 0.0 });
            let (c, r) = ([5.0, 5.0], 1.0);
            let hole = app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, r))], Prim::Circle { c, r });
            app.doc.new_construction = true;
            let axis = app.doc.add_vector(vec![geom::Path::open(vec![[5.0, 1.5], [5.0, 8.5]])], Prim::Free);
            app.doc.new_construction = false;
            let side = |seg| Elem::Seg { v: plate, path: 0, seg };
            app.add_constraint(Kind::Fixed, vec![Elem::Node { v: plate, path: 0, node: 0 }]);
            app.make_dimension(&[side(0)], DimMode::Smart);
            app.make_dimension(&[side(1)], DimMode::Smart);
            app.make_dimension(&[Elem::Round { v: hole }], DimMode::Smart);
            app.add_constraint(Kind::Vertical, vec![Elem::Seg { v: axis, path: 0, seg: 0 }]);
            app.add_constraint(Kind::PointOnLine, vec![Elem::Centre { v: hole }, Elem::Seg { v: axis, path: 0, seg: 0 }]);
            app.dim_edit = None;
            app.selected.clear();
            app.start_constraint_form();
        }
        "similar" | "rules" | "paths2d" => {
            app.new_file();
            app.left = LeftForm::None;
            app.right_open = true;
            let mut holes = Vec::new();
            for (i, c) in [[2.0, 2.0], [8.0, 2.0], [2.0, 8.0], [8.0, 8.0]].into_iter().enumerate() {
                let r = if i < 4 { 0.08 } else { 0.2 };
                holes.push(app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, r))], Prim::Circle { c, r }));
            }
            let plate = app.doc.add_vector(vec![geom::Path::closed(geom::rect([5.0, 5.0], 8.0, 8.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 8.0, h: 8.0, corner: 0.0 });
            let (c, r) = ([5.0, 5.0], 1.5);
            let big = app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, r))], Prim::Circle { c, r });
            match state {
                "similar" => {
                    app.selected = vec![holes[0]];
                    app.start_xform(app::Xform::Bulk);
                    app.select_similar(0.01);
                }
                "rules" => {
                    use constraints::{Elem, Kind};
                    let round = |v| Elem::Round { v };
                    app.add_constraint(Kind::Equal, vec![round(holes[0]), round(holes[1])]);
                    app.add_constraint(Kind::Equal, vec![round(holes[1]), round(holes[2])]);
                    app.add_constraint(Kind::Diameter, vec![round(big)]);
                    app.add_constraint(Kind::Coincident, vec![Elem::Node { v: plate, path: 0, node: 0 }, Elem::Centre { v: holes[0] }]);
                    app.selected.clear();
                    app.start_constraint_form();
                }
                _ => {
                    app.selected = vec![big];
                    app.start_op(toolpath::OpKind::Pocket);
                    app.calculate_op();
                    app.view3d = false;
                    app.right = RightMode::List;
                    app.selected = vec![plate];
                    app.start_op(toolpath::OpKind::Profile);
                    app.calculate_op();
                    app.view3d = false;
                    app.right = RightMode::List;
                }
            }
        }
        "move" | "align" | "array" | "lines" => {
            app.new_file();
            circle(app);
            app.right_open = true;
            app.left = LeftForm::None;
            match state {
                "move" => app.start_xform(app::Xform::Move),
                "align" => app.start_xform(app::Xform::Align),
                "array" => app.start_xform(app::Xform::BlockCopy),
                _ => app.left = LeftForm::Draw(DrawForm::new(DrawKind::Polyline, &app.doc.job)),
            }
        }
        "unsaved" => {
            app.load_sample();
            app.dirty = true;
            app.leave_for(app::Leave::New);
        }
        "model" => {
            app.load_sample();
            app.right_open = true;
            app.right = RightMode::List;
            app.view3d = true;
        }
        // A shape selected for moving beside one selected for resizing can't be shown at once: this is the second.
        "handles" => {
            app.new_file();
            app.left = LeftForm::None;
            app.screen = app::Screen::Work;
            let id = app.doc.add_vector(vec![geom::Path::closed(geom::rect([4.0, 5.0], 4.0, 3.0, 0.2))], Prim::Rect { c: [4.0, 5.0], w: 4.0, h: 3.0, corner: 0.2 });
            app.selected = vec![id];
            app.handles_on = Some(vec![id]);
        }
        "material-changed" => {
            app.load_sample();
            app.view3d = false;
            app.right = RightMode::List;
            app.ask_recalc = true;
        }
        // A piece with one of its shapes selected, on a layer with a long name.
        "piece" => {
            app.new_file();
            app.left = LeftForm::None;
            app.screen = app::Screen::Work;
            app.doc.layers[0].name = "vue_du_solide_percages_et_decoupe".into();
            let plate = app.doc.add_vector(vec![geom::Path::closed(geom::rect([3.2, 6.0], 4.0, 3.0, 0.2))], Prim::Rect { c: [3.2, 6.0], w: 4.0, h: 3.0, corner: 0.2 });
            let mut holes = Vec::new();
            for c in [[2.0, 5.2], [4.4, 5.2], [2.0, 6.8], [4.4, 6.8]] {
                holes.push(app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, 0.2))], Prim::Circle { c, r: 0.2 }));
            }
            let c = [7.5, 3.0];
            app.doc.add_vector(vec![geom::Path::closed(geom::circle(c, 1.0))], Prim::Circle { c, r: 1.0 });
            app.selected = holes.iter().copied().chain([plate]).collect();
            app.make_piece();
            app.selected = vec![holes[1]];
        }
        // The list of pieces beside a piece that's cut, and the form to save it alone.
        "pieces" | "savepiece" => {
            open_state(app, "piece");
            app.selected = app.doc.pieces()[0].shapes.iter().copied().skip(1).collect();
            app.start_op(toolpath::OpKind::Drill);
            app.calculate_op();
            app.view3d = false;
            app.left_tab = app::LeftTab::Layers;
            app.right = if state == "savepiece" { app::RightMode::Save } else { app::RightMode::List };
            app.right_open = true;
            app.save_piece = (state == "savepiece").then(|| app.doc.pieces()[0].id);
            app.selected.clear();
        }
        // The sample's shapes in a group, selected.
        "preferences" => {
            app.load_sample();
            app.view3d = false;
            app.prefs_open = true;
        }
        "rotate" => {
            app.load_sample();
            app.view3d = false;
            app.right = RightMode::List;
            let id = app.doc.vectors[0].id;
            app.selected = vec![id];
            app.handles_on = Some(vec![id]);
        }
        "checks" => {
            app.load_sample();
            app.view3d = false;
            app.doc.toolpaths[2].op.tool.flute_length = 0.05;
            app.doc.toolpaths[2].op.tool.feed = 0.0;
            app.checks_open = true;
            app.checks_for_save = Some(None);
        }
        "drawmoves" => {
            app.load_sample();
            app.view3d = true;
            app.right = RightMode::Preview;
            app.draw_block = true;
            app.draw_origin = true;
            app.solid_previews = true;
        }
        "backups" => {
            app.load_sample();
            app.view3d = false;
            let dir = std::env::temp_dir().join("burin-sample-backups");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            let file = dir.join("exemple.burin");
            let now = crate::platform::SystemTime::now().duration_since(crate::platform::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            for (i, ago) in [180u64, 2400, 9000, 90000, 400000].iter().enumerate() {
                let text = serde_json::to_string(&app.doc).unwrap_or_default() + &" ".repeat(i);
                let _ = crate::platform::write(&file, text);
                let _ = backups::keep_before_saving(&file, b"", now - ago, backups::Settings::default());
            }
            app.path = Some(file);
            app.dirty = false;
            app.backups_open = true;
        }
        "feedback" => {
            app.load_sample();
            app.view3d = false;
            app.issues_page = Some("https://example.org/issues/new".into());
            let ctx = egui::Context::default();
            app.open_feedback(&ctx, None);
            app.want_picture = None;
            if let Some(form) = &mut app.feedback {
                form.title = "Le perçage saute un trou".into();
                form.text = "Sur un cercle de 6 mm, le troisième trou est oublié.".into();
            }
        }
        "crashes" => {
            app.load_sample();
            app.view3d = false;
            let dir = std::env::temp_dir().join("burin-sample-crashes");
            let _ = std::fs::remove_dir_all(&dir);
            let sample = crash::Report { format: "burin-crash/1".into(), kind: "panic".into(), message: "index out of bounds: the len is 3 but the index is 7".into(), location: "src/toolpath.rs:812:14".into(), thread: "main".into(), version: env!("CARGO_PKG_VERSION").into(), system: "linux, 8 threads".into(), job: "3 shapes, 2 toolpaths, 1200 moves".into(), recent_actions: vec!["Recalculate All".into(), "Save Toolpaths...".into()], backtrace: vec!["burin::toolpath::generate (src/toolpath.rs:812:14)".into(), "burin::app::App::frame (src/app.rs:40:9)".into()], signature: "0".into(), ..Default::default() };
            crash::save_in(&dir.join("crashes"), sample, 1794041400);
            app.data_dir = Some(dir);
            app.after_crash_checked = true;
            app.crashes_open = true;
        }
        "lasttime" => {
            app.load_sample();
            app.view3d = false;
            app.dirty = false;
        }
        "support" => {
            app.load_sample();
            app.view3d = false;
            app.support_open = true;
        }
        "variables" => {
            app.load_sample();
            app.view3d = false;
            app.doc.variables = vec![formula::Variable { name: "thick".into(), value: 0.75 }, formula::Variable { name: "tab_len".into(), value: 0.5 }];
            app.open_variables();
        }
        "allsides" | "multisided" => {
            app.new_file();
            app.left = LeftForm::None;
            app.doc.job.two_sided = true;
            app.right_open = true;
            // The same circle from each side, each cut a little past half way: it comes out whole.
            for (face, y) in [(0u8, 6.0), (1u8, 4.0)] {
                app.doc.active_face = face;
                let c = app.doc.add_vector(vec![geom::Path::closed(geom::circle([4.5, y], 2.0))], Prim::Free);
                app.selected = vec![c];
                app.start_op(toolpath::OpKind::Profile);
                if let RightMode::Op { op, .. } = &mut app.right {
                    op.cut_depth = app.doc.job.thickness * 0.55;
                }
                app.calculate_op();
            }
            app.doc.active_face = 0;
            app.selected.clear();
            app.right = RightMode::Preview;
            app.preview_all_sides();
            app.message = None;
            app.finish_preview();
            app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 40f32.to_radians(), zoom: 1.3, pan: egui::Vec2::ZERO };
            if state == "multisided" {
                app.reset_preview();
                app.view3d = true;
                app.cam = view3d::Camera { yaw: 25f32.to_radians(), tilt: 75f32.to_radians(), zoom: 1.3, pan: egui::Vec2::ZERO };
            }
        }
        "bottomside" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.doc.job.two_sided = true;
            app.doc.active_face = 1;
            app.doc.add_vector(vec![geom::Path::closed(vec![[5.5, 2.0], [8.5, 2.0], [8.5, 4.5], [5.5, 4.5]])], Prim::Free);
            app.selected.clear();
        }
        "joins" => {
            app.new_file();
            app.left = LeftForm::None;
            let r = app.doc.add_vector(vec![geom::Path::closed(vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]])], Prim::Free);
            app.selected = vec![r];
            app.right_open = true;
            app.start_op(toolpath::OpKind::Pocket);
            if let RightMode::Op { op, .. } = &mut app.right {
                op.raster = true;
                op.cut_depth = 0.1;
                op.profile_pass = toolpath::ProfilePass::None;
            }
            app.calculate_op();
            app.selected.clear();
            app.view3d = true;
            app.cam = view3d::Camera { yaw: 20f32.to_radians(), tilt: 50f32.to_radians(), zoom: 6.0, pan: egui::vec2(-900.0, 300.0) };
        }
        "texture" => {
            app.new_file();
            app.left = LeftForm::None;
            let star = app.doc.add_vector(vec![geom::Path::closed(geom::circle([1.0, 1.0], 0.8))], Prim::Free);
            app.selected = vec![star];
            app.open_shape_form(None);
            if let LeftForm::Shape(mut f) = app.left.clone() {
                f.profile = relief::Profile::Round;
                f.height = 0.2;
                app.add_component(f);
            }
            app.left = LeftForm::None;
            let bound = app.doc.add_vector(vec![geom::Path::closed(geom::circle([5.0, 5.0], 4.5))], Prim::Free);
            app.selected = vec![bound];
            app.selected_comp = Some(0);
            app.texture_area(modeltools::TextureForm { spacing: [10.0, 10.0], shift: [50.0, 0.0], mirror: [0; 4] });
            app.selected.clear();
            app.left_tab = app::LeftTab::Modeling;
            app.view3d = true;
        }
        "zeroplane" => {
            app.new_file();
            app.left = LeftForm::None;
            circle(app);
            app.open_shape_form(None);
            if let LeftForm::Shape(mut f) = app.left.clone() {
                f.combine = relief::Combine::Subtract;
                f.profile = relief::Profile::Round;
                f.height = 0.3;
                app.add_component(f);
            }
            app.left = LeftForm::None;
            app.add_zero_plane();
            app.left_tab = app::LeftTab::Modeling;
            app.selected.clear();
            app.view3d = true;
        }
        "tiled" | "stacked" => {
            app.load_sample();
            app.selected.clear();
            app.view3d = false;
            app.tiled = Some(if state == "tiled" { app::Tiling::SideBySide } else { app::Tiling::Stacked });
        }
        "turnspin" => {
            app.new_file();
            let arc = app.doc.add_vector(vec![geom::Path { pts: vec![[2.0, 5.0], [3.0, 6.2], [5.0, 6.4], [7.0, 6.0], [8.0, 5.0]], closed: false }], Prim::Free);
            app.selected = vec![arc];
            app.open_turn_spin();
            if let Some(form) = app.turn_form {
                app.turn_spin(form);
            }
            app.view3d = true;
        }
        "distortcomp" => {
            app.load_sample();
            app.selected.clear();
            app.selected_comp = Some(1);
            app.start_xform(app::Xform::Distort);
            app.selected_comp = Some(1);
            app.apply_envelope(distort::EnvelopeKind::BoundingBox);
            if let Some(c) = app.envelope.as_ref().map(|e| e.corners[2]) {
                app.move_envelope_corner(2, [c[0] + 2.0, c[1] + 1.0]);
            }
            app.view3d = false;
        }
        "draftmodel" => {
            app.load_sample();
            app.view3d = false;
            app.open_draft_model();
        }
        "slicemodel" => {
            app.load_sample();
            app.view3d = false;
            app.doc.job.thickness = 0.12;
            app.open_slice_model();
        }
        "offsetmodel" => {
            app.load_sample();
            app.view3d = false;
            app.open_offset_model();
        }
        "splitcomp" => {
            app.load_sample();
            let line = app.doc.add_vector(vec![geom::Path { pts: vec![[5.0, 2.0], [5.0, 9.9]], closed: false }], Prim::Free);
            app.selected = vec![line];
            app.selected_comp = Some(1);
            app.split_component();
            app.model_dirty = true;
            app.view3d = false;
        }
        "texturesum" => {
            app.load_sample();
            app.view3d = false;
            app.doc.variables = vec![formula::Variable { name: "grain".into(), value: 0.25 }];
            app.start_op(toolpath::OpKind::Texture);
            let context = app.formula_context();
            if let RightMode::Op { op, .. } = &mut app.right {
                op.formulas.texture_length = "{grain}*2".into();
                let _ = op.formulas.clone().apply(op, &context);
            }
        }
        "printsetup" => {
            app.load_sample();
            app.view3d = false;
            app.print_setup.paper = printing::Paper::A4;
            app.print_setup_open = true;
        }
        "help" => {
            app.load_sample();
            app.view3d = false;
            app.help_open = true;
            app.help_page = 4;
        }
        "shadows" => {
            app.load_sample();
            app.right_open = true;
            app.right = RightMode::Preview;
            app.view3d = true;
            app.preview(vec![0, 1, 2]);
            app.finish_preview();
            for t in &mut app.doc.toolpaths {
                t.visible = false;
            }
            app.shadow_shading = true;
        }
        "missinglayers" => {
            app.load_sample();
            app.view3d = false;
            let mut op = app.doc.toolpaths[0].op.clone();
            op.name = "Rough cut".into();
            op.selector = None;
            op.paths.clear();
            op.source.clear();
            app.template_waiting = Some(templates::Waiting { items: vec![templates::Item { op, layer: Some("Outlines".into()), selector_layers: Vec::new() }], missing: vec!["Outlines".into()] });
        }
        "merge" => {
            app.load_sample();
            app.view3d = false;
            app.open_merge_form();
        }
        "grouped" => {
            app.load_sample();
            app.selected = app.doc.vectors.iter().take(2).map(|v| v.id).collect();
            app.group_selected();
            app.view3d = false;
            app.right = RightMode::List;
        }
        // A 6 mm tab with a 6 mm tool, and the same tool stopping only 6 mm apart as it used to.
        "tabs" => tab_demo(app, 6.0),
        "tabs-before" => tab_demo(app, 0.001),
        "carved" => {
            app.load_sample();
            app.right_open = true;
            app.right = RightMode::Preview;
            app.view3d = true;
            app.preview(vec![0, 1, 2]);
            app.finish_preview();
            for t in &mut app.doc.toolpaths {
                t.visible = false;
            }
        }
        _ => {}
    }
}

/// A small made-up picture for checking the trace screen.
fn demo_picture() -> bitmap::Image {
    let (w, h) = (400usize, 300usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 - 130.0, y as f64 - 150.0);
            let r = dx.hypot(dy);
            let c: [u8; 3] = if r < 45.0 {
                [235, 180, 40]
            } else if r < 95.0 {
                [30, 30, 40]
            } else if (250..360).contains(&x) && (70..230).contains(&y) {
                [190, 50, 50]
            } else {
                [250, 250, 250]
            };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    bitmap::Image { w, h, rgba: std::sync::Arc::new(rgba) }
}

fn icon() -> egui::IconData {
    let picture = image::load_from_memory(include_bytes!("../release/burin.png")).expect("the icon is part of the program").into_rgba8();
    egui::IconData { width: picture.width(), height: picture.height(), rgba: picture.into_raw() }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut screenshot = None;
    let mut state = None;
    let mut asked = None;
    let mut test_crash = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--open" => state = args.next(),
            "--renderer" => asked = args.next(),
            // Dies while opening with this way of drawing, or `panic` or `abort` once running,
            // to try out what happens next.
            "--test-crash" => test_crash = args.next(),
            flag if flag.starts_with("--") => {}
            // As given by the desktop when a project's file is opened.
            file => {
                if state.is_none() {
                    state = Some(format!("project:{file}"));
                }
            }
        }
    }
    let first_copy = startup::claim_session();
    let previous = startup::previous_log();
    let avoid = if first_copy { startup::stopped_before(&previous, env!("CARGO_PKG_VERSION")) } else { Vec::new() };
    if first_copy && (screenshot.is_none() || state.as_deref() == Some("lasttime")) {
        crash::check_last_run(&previous);
    }
    startup::begin_log();
    startup::log_panics();
    crash::begin_session();
    let mut reasons = Vec::new();
    for engine in startup::attempts(asked.as_deref()) {
        if asked.is_none() && avoid.contains(&engine) {
            startup::log(&format!("Avoiding: {}", engine.word()));
            continue;
        }
        startup::log(&format!("Trying: {}", engine.word()));
        let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open_window(engine, screenshot.clone(), state.clone(), test_crash.as_deref())));
        let reason = match opened {
            Ok(Ok(())) => {
                startup::log(startup::CLOSED);
                return;
            }
            Ok(Err(e)) => e.to_string(),
            Err(crash) => startup::panic_text(crash.as_ref()),
        };
        let line = format!("{}: {reason}", engine.word());
        startup::log(&line);
        reasons.push(line);
        if SHOWING.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }
    if !SHOWING.load(std::sync::atomic::Ordering::Relaxed) {
        crash::record_no_window(&reasons);
    }
    startup::show_failure(&startup::failure_text(SHOWING.load(std::sync::atomic::Ordering::Relaxed), &reasons));
    startup::log(startup::CLOSED);
    std::process::exit(1);
}

#[cfg(not(target_arch = "wasm32"))]
fn open_window(engine: startup::Engine, screenshot: Option<PathBuf>, state: Option<String>, test_crash: Option<&str>) -> eframe::Result {
    let crash_now = test_crash == Some(engine.word());
    let crash_later = test_crash.filter(|t| matches!(*t, "panic" | "abort")).map(str::to_string);
    let options = engine.options(eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Burin").with_icon(icon()).with_inner_size([1910.0, 1040.0]).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    });
    eframe::run_native(
        "Burin",
        options,
        Box::new(move |cc| {
            if crash_now {
                // On purpose: nothing below this line can be reached.
                unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
            }
            ui::setup_style(&cc.egui_ctx);
            let adapter = cc.wgpu_render_state.as_ref().map(|s| s.adapter.get_info()).map(|i| format!("{} ({:?}, {:?}, driver {})", i.name, i.backend, i.device_type, i.driver));
            let gl = cc.gl.as_ref().map(|gl| {
                use eframe::glow::HasContext;
                unsafe { gl.get_parameter_string(eframe::glow::RENDERER) }
            });
            if let Some(card) = adapter.or(gl) {
                startup::set_graphics(&card);
            }
            let mut app = App::default();
            // The picture to save has to be the finished one.
            app.surface_in_background = screenshot.is_none();
            app.restore_posts();
            // Pictures for a check shouldn't show this computer's reports, or touch its copy of unsaved work.
            let quiet = screenshot.is_some() && !state.as_deref().is_some_and(|s| s == "lasttime");
            app.data_dir = if quiet || startup::is_second_copy() { None } else { app::settings_file("x").and_then(|f| f.parent().map(PathBuf::from)) };
            match state.as_deref().and_then(|s| s.strip_prefix("project:")) {
                Some(file) => {
                    if let Err(why) = app.open_path(file.into()) {
                        app.message = Some(why);
                    }
                }
                None => {
                    if let Some(s) = &state {
                        open_state(&mut app, s);
                    }
                }
            }
            Ok(Box::new(Program { app, screenshot, frames: 0, requested: false, engine, drawn: 0, crash_later }))
        }),
    )
}

/// The web version starts from the page's script instead (`web::start`).
#[cfg(target_arch = "wasm32")]
fn main() {}
