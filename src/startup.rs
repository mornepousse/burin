//! Starting the window on whatever the computer can do, and saying why when it can't: a released
//! Windows program has no console, so anything not shown here would just look like nothing happening.

use std::io::Write;
use std::path::PathBuf;

/// The ways of drawing the window, tried in this order. Older graphics cards often have no Direct3D 12
/// or recent OpenGL but do have older OpenGL; the last one needs no graphics card at all.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Engine {
    Graphics,
    OpenGl,
    Software,
}

impl Engine {
    pub fn word(self) -> &'static str {
        match self {
            Engine::Graphics => "graphics",
            Engine::OpenGl => "opengl",
            Engine::Software => "software",
        }
    }

    pub fn options(self, base: eframe::NativeOptions) -> eframe::NativeOptions {
        use eframe::egui_wgpu::{wgpu, WgpuSetupCreateNew};
        let mut options = base;
        match self {
            Engine::Graphics => options.renderer = eframe::Renderer::Wgpu,
            Engine::OpenGl => options.renderer = eframe::Renderer::Glow,
            Engine::Software => {
                options.renderer = eframe::Renderer::Wgpu;
                let mut setup = WgpuSetupCreateNew::without_display_handle();
                setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, _| {
                    adapters.iter().find(|a| a.get_info().device_type == wgpu::DeviceType::Cpu).cloned().ok_or_else(|| "no software drawing on this computer".to_string())
                }));
                options.wgpu_options.wgpu_setup = setup.into();
            }
        }
        options
    }
}

/// What to try: all of them, or only the one asked for on the command line.
pub fn attempts(asked: Option<&str>) -> Vec<Engine> {
    let all = [Engine::Graphics, Engine::OpenGl, Engine::Software];
    match asked {
        Some(word) => all.into_iter().filter(|e| e.word() == word).collect(),
        None => all.to_vec(),
    }
}

fn log_file() -> Option<PathBuf> {
    crate::app::settings_file("startup-log.txt")
}

/// A new log for this start, naming the program and the system.
pub fn begin_log() {
    let Some(file) = log_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("Burin {} on {} {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
}

/// Written straight away, so the last line tells how far it got even if the program dies suddenly.
pub fn log(line: &str) {
    if let Some(mut f) = log_file().and_then(|file| std::fs::OpenOptions::new().append(true).create(true).open(file).ok()) {
        let _ = writeln!(f, "{line}");
    }
}

/// Keeps the reason for a crash, which would otherwise go to a console nobody can see.
pub fn log_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(&format!("Crash: {info}"));
        before(info);
    }));
}

pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| crate::i18n::tr("it stopped unexpectedly").to_string())
}

/// The message for someone whose program didn't open, with what they can do and what to send on.
pub fn failure_text(started: bool, reasons: &[String]) -> String {
    let where_to = log_file().map_or(String::new(), |f| tf!("\n\nThis is also saved in:\n{}", f.display()));
    let (heading, advice) = if started {
        (crate::i18n::tr("Burin stopped unexpectedly."), crate::i18n::tr("Unsaved work may be lost."))
    } else {
        (crate::i18n::tr("Burin could not open its window."), crate::i18n::tr("This usually comes from the graphics card's driver. Updating it from the maker's website often fixes this."))
    };
    format!("{heading}\n\n{advice}\n\n{}{where_to}", reasons.join("\n"))
}

pub fn show_failure(text: &str) {
    eprintln!("{text}");
    let answer = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Burin").set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
    log(&format!("Message box closed: {answer:?}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_way_of_drawing_is_tried_unless_one_is_asked_for() {
        assert_eq!(attempts(None), [Engine::Graphics, Engine::OpenGl, Engine::Software]);
        assert_eq!(attempts(Some("opengl")), [Engine::OpenGl]);
        assert!(attempts(Some("none")).is_empty());
    }

    #[test]
    fn each_way_of_drawing_chooses_its_renderer() {
        assert_eq!(Engine::OpenGl.options(Default::default()).renderer, eframe::Renderer::Glow);
        assert_eq!(Engine::Graphics.options(Default::default()).renderer, eframe::Renderer::Wgpu);
        assert_eq!(Engine::Software.options(Default::default()).renderer, eframe::Renderer::Wgpu);
    }

    #[test]
    fn the_message_says_why_and_what_to_do() {
        let text = failure_text(false, &["graphics: no adapter".to_string()]);
        assert!(text.contains("could not open") && text.contains("driver") && text.contains("graphics: no adapter"), "{text}");
        assert!(failure_text(true, &[]).contains("stopped unexpectedly"));
    }
}
