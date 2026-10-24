//! Standard tauri build script: processes ./tauri.conf.json into OUT_DIR
//! artifacts for `tauri::generate_context!()`.

fn main() {
    tauri_build::build();
}
