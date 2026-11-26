//! Puts the icon on the Windows program file.

fn main() {
    // Reports go through the relay (relay/), so no address is written into the program. One can
    // still be given for a build of its own with BURIN_REPORT_EMAIL (read by option_env!).
    println!("cargo:rerun-if-env-changed=BURIN_REPORT_EMAIL");
    println!("cargo:rerun-if-changed=release/burin.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("release/burin.ico");
        res.compile().expect("the icon could not be added to the program");
    }
}
