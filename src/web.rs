//! The web version: Burin drawn in a page, with the same screens as on a computer.
//!
//! What a browser doesn't allow is left out: files on disk (files are opened from the browser's picker
//! or dropped on the page, and saved as downloads), copies of unsaved work, backups and crash reports.
//! Work that runs side by side on a computer runs in turn, so long calculations hold the page meanwhile.

use crate::app::App;
use crate::{fit_scale, open_state, platform, ui, write_png};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

struct WebProgram {
    app: App,
}

impl eframe::App for WebProgram {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        fit_scale(&ctx);
        self.app.frame(ui);
        // Saving a picture of the 3D view: it downloads.
        if let Some(path) = self.app.save_preview.take() {
            let area = self.app.view_rect;
            let ppp = ctx.pixels_per_point();
            ctx.request_screenshot(move |image| {
                let _ = write_png(&path, &image.region(&area, Some(ppp)));
            });
        }
    }
}

/// Called by the page once the program is loaded: draws Burin in the canvas with this id.
#[wasm_bindgen]
pub fn start(canvas_id: &str) -> Result<(), JsValue> {
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no page")?;
    let canvas: web_sys::HtmlCanvasElement = document.get_element_by_id(canvas_id).ok_or("no canvas with that id")?.dyn_into()?;
    let options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async move {
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| {
                    ui::setup_style(&cc.egui_ctx);
                    platform::set_waker(&cc.egui_ctx);
                    let mut app = App::default();
                    // Nothing can run beside the page, and nothing is kept on disk.
                    app.surface_in_background = false;
                    app.background_recalc = false;
                    app.data_dir = None;
                    app.restore_posts();
                    // `?open=carved` and the like show one of the known screens, as `--open` does.
                    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
                    if let Some(state) = search.trim_start_matches('?').split('&').find_map(|p| p.strip_prefix("open=")) {
                        open_state(&mut app, state);
                        app.surface_in_background = false;
                        app.background_recalc = false;
                    }
                    Ok(Box::new(WebProgram { app }))
                }),
            )
            .await;
        if let Some(text) = started.err().map(|e| format!("{e:?}")) {
            let document = web_sys::window().and_then(|w| w.document());
            if let Some(status) = document.as_ref().and_then(|d| d.get_element_by_id("status")) {
                let _ = status.class_list().remove_1("hidden");
            }
            if let Some(note) = document.and_then(|d| d.get_element_by_id("message")) {
                note.set_text_content(Some(&format!("Burin n'a pas pu démarrer dans ce navigateur : {text}")));
            }
        }
    });
    Ok(())
}
