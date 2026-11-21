fn main() {
    // @tr() strings are English; the catalogues in ../../lang (shared with
    // fuu-i18n for Rust strings) are bundled into the binary.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("../../lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint UI failed to compile");
    println!("cargo:rerun-if-changed=../../lang");

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
