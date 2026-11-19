fn main() {
    slint_build::compile("ui/main.slint").expect("Slint UI failed to compile");

    // Windows: give fuu.exe its icon (Explorer, taskbar, shortcuts).
    println!("cargo:rerun-if-changed=icons/app/fuu.ico");
    // (Skipped until `cargo run -p fuu-app --example make_icons` has made it.)
    let ico = std::path::Path::new("icons/app/fuu.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && ico.exists() {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("icons/app/fuu.ico");
            res.compile().expect("embed the Windows icon");
        }
    }
}
