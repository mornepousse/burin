//! Puts the icon on the Windows program file, and the address problem reports go to.

fn main() {
    // The address reports are mailed to: given by the release scripts, or else read from the file
    // kept on this computer only (`.backup/report-email`, never in the repository).
    println!("cargo:rerun-if-env-changed=BURIN_REPORT_EMAIL");
    println!("cargo:rerun-if-changed=.backup/report-email");
    if std::env::var("BURIN_REPORT_EMAIL").is_err() {
        if let Ok(text) = std::fs::read_to_string(".backup/report-email") {
            let address = text.trim();
            if address.contains('@') && !address.contains(char::is_whitespace) {
                println!("cargo:rustc-env=BURIN_REPORT_EMAIL={address}");
            }
        }
    }
    println!("cargo:rerun-if-changed=release/burin.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("release/burin.ico");
        res.compile().expect("the icon could not be added to the program");
    }
}
