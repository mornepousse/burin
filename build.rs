//! Puts the icon on the Windows program file.

fn main() {
    println!("cargo:rerun-if-changed=release/burin.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("release/burin.ico");
        res.compile().expect("the icon could not be added to the program");
    }
}
