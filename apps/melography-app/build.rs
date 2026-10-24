fn main() {
    // Feature-conditional capabilities (fixes the headless check broken
    // since M1, session 06 honest-gap note): the dialog plugin — and its
    // `dialog:*` permissions — only exists in gui builds, so a headless
    // `--no-default-features` check must not reference them in the ACL.
    // `capabilities/default.json` (core only) is always active;
    // `capabilities/dialogs.json` joins via the default glob in gui
    // builds; headless pins the pattern to the core file alone.
    let attrs = if std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        tauri_build::Attributes::new()
    } else {
        tauri_build::Attributes::new().capabilities_path_pattern("capabilities/default.json")
    };
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
    println!("cargo:rerun-if-changed=capabilities/");
}
