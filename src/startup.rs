//! Starting the window on whatever the computer can do, and saying why when it can't: a released
//! Windows program has no console, so anything not shown here would just look like nothing happening.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

    #[cfg(not(target_arch = "wasm32"))]
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

/// Written when the program closes normally: a log without it belongs to one that died.
pub const CLOSED: &str = "Closed.";

/// Another copy is running, and the log is that copy's.
static SECOND_COPY: AtomicBool = AtomicBool::new(false);
/// Held while the program runs. The system lets it go however the program ends.
static SESSION: Mutex<Option<std::fs::File>> = Mutex::new(None);
static GRAPHICS: Mutex<String> = Mutex::new(String::new());

fn log_file() -> Option<PathBuf> {
    if SECOND_COPY.load(Ordering::Relaxed) {
        return None;
    }
    crate::app::settings_file("startup-log.txt")
}

/// Whether this is the only copy running. If it isn't, it leaves the log and the reports alone.
pub fn claim_session() -> bool {
    let Some(file) = crate::app::settings_file("session.lock") else { return true };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(lock) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(file) else { return true };
    let first = lock.try_lock().is_ok();
    if first {
        if let Ok(mut held) = SESSION.lock() {
            *held = Some(lock);
        }
    } else {
        SECOND_COPY.store(true, Ordering::Relaxed);
    }
    first
}

pub fn is_second_copy() -> bool {
    SECOND_COPY.load(Ordering::Relaxed)
}

pub fn set_graphics(card: &str) {
    log(&format!("Graphics card: {card}"));
    if let Ok(mut g) = GRAPHICS.lock() {
        *g = card.to_string();
    }
}

pub fn graphics_line() -> String {
    GRAPHICS.try_lock().map_or(String::new(), |g| g.clone())
}

/// The ways of drawing that stopped the program dead when it last started, read from the log it left.
/// A driver can crash the whole program, leaving nothing to fall back from: the next start avoids it.
/// A new version tries them all again.
pub fn stopped_before(log: &str, version: &str) -> Vec<Engine> {
    let all = [Engine::Graphics, Engine::OpenGl, Engine::Software];
    let named = |word: &str| all.into_iter().find(|e| e.word() == word);
    let mut lines = log.lines();
    if !lines.next().is_some_and(|first| first.starts_with(&format!("Burin {version} "))) {
        return Vec::new();
    }
    let mut avoid: Vec<Engine> = Vec::new();
    let mut trying = None;
    for line in lines {
        if let Some(e) = line.strip_prefix("Avoiding: ").and_then(named) {
            avoid.push(e);
        } else if let Some(e) = line.strip_prefix("Trying: ").and_then(named) {
            trying = Some(e);
        } else if line.starts_with("Drawing: ") || trying.is_some_and(|e| line.starts_with(&format!("{}: ", e.word()))) {
            trying = None;
        }
    }
    avoid.extend(trying);
    // All of them failed: nothing is lost by trying again from the start.
    if all.iter().all(|e| avoid.contains(e)) {
        avoid.clear();
    }
    avoid
}

/// What the last start left behind, for choosing how to start now. To be read before `begin_log`.
pub fn previous_log() -> String {
    log_file().and_then(|f| crate::platform::read_to_string(f).ok()).unwrap_or_default()
}

/// A new log for this start, naming the program and the system.
pub fn begin_log() {
    let Some(file) = log_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = crate::platform::write(file, format!("Burin {} on {} {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
    log(&system_line());
    #[cfg(windows)]
    native_crash::catch();
}

/// The system and what the processor can do, as much as the program can tell.
pub fn system_line() -> String {
    #[cfg(windows)]
    let system = native_crash::windows_version();
    #[cfg(not(windows))]
    let system = std::env::consts::OS.to_string();
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    #[cfg(target_arch = "x86_64")]
    let extras = format!("sse4.2 {}, avx2 {}", std::arch::is_x86_feature_detected!("sse4.2"), std::arch::is_x86_feature_detected!("avx2"));
    #[cfg(not(target_arch = "x86_64"))]
    let extras = String::new();
    format!("System: {system}, {cores} threads, {extras}")
}

/// A crash outside the program's own code, in a graphics driver say, ends it with no message at all.
#[cfg(windows)]
mod native_crash {
    use std::ffi::c_void;

    #[repr(C)]
    struct Record {
        code: u32,
        flags: u32,
        next: *const Record,
        address: *const c_void,
    }
    #[repr(C)]
    struct Pointers {
        record: *const Record,
        context: *const c_void,
    }
    #[repr(C)]
    struct VersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    type Filter = unsafe extern "system" fn(*const Pointers) -> i32;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetUnhandledExceptionFilter(filter: Option<Filter>) -> Option<Filter>;
        fn GetModuleHandleExW(flags: u32, name: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: *mut c_void, text: *const u16, title: *const u16, kind: u32) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut VersionInfo) -> i32;
    }

    pub fn windows_version() -> String {
        let mut info = VersionInfo { size: std::mem::size_of::<VersionInfo>() as u32, major: 0, minor: 0, build: 0, platform: 0, service_pack: [0; 128] };
        unsafe { RtlGetVersion(&mut info) };
        format!("Windows {}.{} build {}", info.major, info.minor, info.build)
    }

    /// The file the crash happened in, without its folder.
    fn module_at(address: *const c_void) -> String {
        const FROM_ADDRESS: u32 = 4;
        const NO_COUNT: u32 = 2;
        let mut module = std::ptr::null_mut();
        let mut name = [0u16; 512];
        let found = unsafe { GetModuleHandleExW(FROM_ADDRESS | NO_COUNT, address, &mut module) };
        let size = if found != 0 { unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) } } else { 0 };
        let path = String::from_utf16_lossy(&name[..size as usize]);
        path.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()).unwrap_or("unknown").to_string()
    }

    unsafe extern "system" fn caught(info: *const Pointers) -> i32 {
        let (code, address) = unsafe { ((*(*info).record).code, (*(*info).record).address) };
        super::log(&format!("Crash outside the program's code: {code:#x} at {address:p} in {}", module_at(address)));
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let text = wide(&format!("{}\n\n{}", crate::i18n::tr("Burin stopped unexpectedly."), crate::i18n::tr("Start it again: it will try another way of drawing if this happened while opening.")));
        const ERROR_ICON: u32 = 0x10;
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), wide("Burin").as_ptr(), ERROR_ICON) };
        1
    }

    pub fn catch() {
        unsafe { SetUnhandledExceptionFilter(Some(caught)) };
    }
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
        log(&format!("Crash: {}", crate::crash::without_home(&info.to_string())));
        crate::crash::record_panic(info);
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
    #[cfg(not(target_arch = "wasm32"))]
    let answer = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Burin").set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
    #[cfg(not(target_arch = "wasm32"))]
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
    fn a_way_of_drawing_that_killed_the_program_is_avoided_next_time() {
        let v = "1.0";
        let clean = "Burin 1.0 on windows x86_64\nSystem: x\nTrying: graphics\nThe window is showing.\nDrawing: graphics\n";
        assert!(stopped_before(clean, v).is_empty());
        assert!(stopped_before("", v).is_empty());

        // Stopped while trying, and nothing said afterwards.
        let died = "Burin 1.0 on windows x86_64\nTrying: graphics\n";
        assert_eq!(stopped_before(died, v), [Engine::Graphics]);
        // A crash once it was drawing isn't to do with how it opened.
        assert!(stopped_before("Burin 1.0 on windows x86_64\nTrying: opengl\nDrawing: opengl\nCrash outside the program's code: 0xc0000005\n", v).is_empty());
        // An ordinary failure has already moved on to the next way.
        assert_eq!(stopped_before("Burin 1.0 on windows x86_64\nTrying: graphics\ngraphics: no adapter\nTrying: opengl\n", v), [Engine::OpenGl]);
        // Remembered from one start to the next, while the way that works goes on working.
        let later = "Burin 1.0 on windows x86_64\nAvoiding: graphics\nTrying: opengl\nDrawing: opengl\n";
        assert_eq!(stopped_before(later, v), [Engine::Graphics]);
        // Another version may have fixed it, and with all of them failed there is nothing to lose.
        assert!(stopped_before(died, "1.1").is_empty());
        let all = "Burin 1.0 on windows x86_64\nAvoiding: graphics\nAvoiding: opengl\nTrying: software\n";
        assert!(stopped_before(all, v).is_empty());
    }

    #[test]
    fn the_message_says_why_and_what_to_do() {
        let text = failure_text(false, &["graphics: no adapter".to_string()]);
        assert!(text.contains("could not open") && text.contains("driver") && text.contains("graphics: no adapter"), "{text}");
        assert!(failure_text(true, &[]).contains("stopped unexpectedly"));
    }
}
