//! What differs between the program on a computer and the same program in a web browser.
//!
//! On a computer everything here is the usual thing. In a browser there are no files, no threads and
//! no clock from the system, so: files chosen to open come from the browser's own picker, files
//! saved go to its downloads, work is done in turn instead of side by side, and time comes from the page.

use std::io;
use std::path::{Path, PathBuf};

pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

/// In the web version, where settings files are: the browser keeps them for the site, in its local storage.
pub const SETTINGS: &str = "/settings";

/// True in the web version.
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// How many threads to share work between. Always one in a browser, where none can be started.
pub fn threads(leave_one: bool) -> usize {
    if WEB {
        return 1;
    }
    let all = std::thread::available_parallelism().map_or(4, |n| n.get());
    if leave_one {
        all.saturating_sub(1).max(1)
    } else {
        all
    }
}

/// Work shared between threads that all end before this returns. In a browser, each piece is done in turn.
#[cfg(not(target_arch = "wasm32"))]
pub use std::thread::scope;

#[cfg(target_arch = "wasm32")]
pub struct InTurn;

#[cfg(target_arch = "wasm32")]
impl InTurn {
    pub fn spawn<F: FnOnce()>(&self, f: F) {
        f()
    }
}

#[cfg(target_arch = "wasm32")]
pub fn scope<R>(f: impl FnOnce(&InTurn) -> R) -> R {
    f(&InTurn)
}

/// Runs `f` on its own thread, or right away in a browser.
pub fn spawn(f: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(f);
    #[cfg(target_arch = "wasm32")]
    f();
}

/// Reads a file. In a browser, only files the person chose or dropped on the page are there.
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::fs::read(path);
    #[cfg(target_arch = "wasm32")]
    return web::read(path.as_ref());
}

pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes a file. In a browser, a file the person chose to save is downloaded; anything else is kept
/// for as long as the page stays open.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::fs::write(path, contents);
    #[cfg(target_arch = "wasm32")]
    return web::write(path.as_ref(), contents.as_ref());
}

/// The file dialogs, with the calls Burin makes. In a browser, saving gives a name to download to,
/// and opening shows the browser's picker: what's chosen arrives a moment later, through `picked`.
#[derive(Default)]
pub struct FileDialog {
    #[cfg(not(target_arch = "wasm32"))]
    inner: Option<rfd::FileDialog>,
    #[cfg(target_arch = "wasm32")]
    extensions: Vec<String>,
    #[cfg(target_arch = "wasm32")]
    name: Option<String>,
}

impl FileDialog {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return FileDialog { inner: Some(rfd::FileDialog::new()) };
        #[cfg(target_arch = "wasm32")]
        return FileDialog::default();
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner = self.inner.map(|d| d.add_filter(name, extensions));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name.into();
            self.extensions.extend(extensions.iter().map(|e| e.to_string()));
        }
        self
    }

    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner = self.inner.map(|d| d.set_file_name(name));
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.name = Some(name.into());
        }
        self
    }

    #[allow(unused_mut)]
    pub fn set_directory(mut self, dir: impl AsRef<Path>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner = self.inner.map(|d| d.set_directory(dir));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = dir;
        self
    }

    pub fn pick_file(self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.inner?.pick_file();
        #[cfg(target_arch = "wasm32")]
        {
            web::pick(&self.extensions, false);
            None
        }
    }

    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.inner?.pick_files();
        #[cfg(target_arch = "wasm32")]
        {
            web::pick(&self.extensions, true);
            None
        }
    }

    /// A browser can't choose a folder for a program.
    pub fn pick_folder(self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.inner?.pick_folder();
        #[cfg(target_arch = "wasm32")]
        None
    }

    pub fn save_file(self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.inner?.save_file();
        #[cfg(target_arch = "wasm32")]
        {
            let ext = self.extensions.first().cloned().unwrap_or_else(|| "txt".into());
            let name = self.name.unwrap_or_else(|| format!("burin.{ext}"));
            Some(PathBuf::from(web::DOWNLOADS).join(name))
        }
    }
}

/// Files chosen in the browser's picker since the last call, kept so they can be read.
pub fn picked() -> Vec<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    return web::take_picked();
    #[cfg(not(target_arch = "wasm32"))]
    Vec::new()
}

/// Reads a file dropped on the page and keeps it, to arrive through `picked`. Only for the web version:
/// on a computer a dropped file is already on disk.
pub fn keep_dropped(file: std::sync::Arc<dyn egui::DroppedFile + Send + Sync>) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        let name = file.path().file_name().map_or("dropped".into(), |n| n.to_string_lossy().to_string());
        if let Ok(bytes) = file.bytes_async().await {
            let path = web::keep(&name, bytes);
            web::arrived(path);
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = file;
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::collections::HashMap;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use wasm_bindgen::{closure::Closure, JsCast};

    /// Where files to download are written; a save dialog answers with a name in here.
    pub const DOWNLOADS: &str = "/downloads";
    /// Where files from the person's computer are kept.
    const UPLOADS: &str = "/uploads";

    static FILES: Mutex<Option<HashMap<PathBuf, Vec<u8>>>> = Mutex::new(None);
    static PICKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    fn unsupported(path: &Path) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, format!("{} isn't in this page", path.display()))
    }

    /// A settings file's place in the browser's storage for this site.
    fn stored(path: &Path) -> Option<(web_sys::Storage, String)> {
        let name = path.strip_prefix(super::SETTINGS).ok()?;
        let storage = web_sys::window()?.local_storage().ok()??;
        Some((storage, format!("burin/{}", name.to_string_lossy())))
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        if let Some((storage, key)) = stored(path) {
            return storage.get_item(&key).ok().flatten().map(String::into_bytes).ok_or_else(|| unsupported(path));
        }
        FILES.lock().unwrap().as_ref().and_then(|f| f.get(path).cloned()).ok_or_else(|| unsupported(path))
    }

    pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        if let Some((storage, key)) = stored(path) {
            // Settings are text; anything else stays in the page only.
            if let Ok(text) = std::str::from_utf8(contents) {
                return storage.set_item(&key, text).map_err(|e| io::Error::other(format!("{e:?}")));
            }
        }
        if path.starts_with(DOWNLOADS) {
            let name = path.file_name().map_or("burin".into(), |n| n.to_string_lossy().to_string());
            download(&name, contents).map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    pub fn keep(name: &str, bytes: Vec<u8>) -> PathBuf {
        let path = Path::new(UPLOADS).join(name);
        FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.clone(), bytes);
        path
    }

    pub fn arrived(path: PathBuf) {
        PICKED.lock().unwrap().push(path);
        super::wake();
    }

    pub fn take_picked() -> Vec<PathBuf> {
        std::mem::take(&mut *PICKED.lock().unwrap())
    }

    /// Hands the bytes to the browser as a file to save.
    fn download(name: &str, contents: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let parts = js_sys::Array::new();
        parts.push(&js_sys::Uint8Array::from(contents));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/octet-stream");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let link: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
        link.set_href(&url);
        link.set_download(name);
        link.click();
        web_sys::Url::revoke_object_url(&url)?;
        Ok(())
    }

    /// Shows the browser's file picker; the files chosen are kept and listed for `take_picked`.
    pub fn pick(extensions: &[String], several: bool) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Ok(input) = document.create_element("input").and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().map_err(Into::into)) else { return };
        input.set_type("file");
        input.set_multiple(several);
        if !extensions.is_empty() {
            let accept: Vec<String> = extensions.iter().map(|e| format!(".{e}")).collect();
            input.set_accept(&accept.join(","));
        }
        let chosen = input.clone();
        let on_change = Closure::<dyn FnMut()>::new(move || {
            let Some(files) = chosen.files() else { return };
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                let name = file.name();
                wasm_bindgen_futures::spawn_local(async move {
                    let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await else { return };
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    arrived(keep(&name, bytes));
                });
            }
        });
        input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
        on_change.forget();
        input.click();
    }
}

/// Asks for a new frame when something arrives from outside, such as a file picked in the browser.
static WAKE: std::sync::Mutex<Option<egui::Context>> = std::sync::Mutex::new(None);

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set_waker(ctx: &egui::Context) {
    *WAKE.lock().unwrap() = Some(ctx.clone());
}

#[allow(dead_code)]
fn wake() {
    if let Some(ctx) = WAKE.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}
