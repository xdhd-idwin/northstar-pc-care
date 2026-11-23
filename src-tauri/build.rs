fn main() {
    let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") {
        include_str!("app.manifest")
    } else {
        include_str!("app-dev.manifest")
    };
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
