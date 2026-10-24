//! MELOGRAPHY editor shell — Tauri 2 host (M1 MVP, issue #8).
//!
//! Hard invariants (ADR-0001, ARCHITECTURE §1):
//! * the webview never computes pixels — it receives preview frames
//!   (`render::PreviewFrame` wire format) and sends parameters;
//! * all pixel math flows through the shared headless pipeline
//!   (`melography-cli` lib = the same code the CLI/CI exercise);
//! * the original image file is never written to (sidecars only).
//!
//! Module map: `state` (session + library + caches) · `render` (preview
//! composition, pure + unit-tested) · `commands` (IPC surface) ·
//! `error` (ARCHITECTURE §8 taxonomy → stable UI codes).

pub mod commands;
pub mod error;
pub mod recents;
pub mod render;
pub mod settings;
pub mod state;
pub mod user_presets;

#[cfg(feature = "gui")]
use std::path::PathBuf;
#[cfg(feature = "gui")]
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    /// Library root: `$MELOGRAPHY_DATA`, else `<home>/.melography`
    /// (matches the CLI default — one library for both front ends).
    #[cfg(feature = "gui")]
    fn library_root() -> PathBuf {
        melography_cli::default_library_root()
    }

    /// Shipped stock registry: `$MELOGRAPHY_PROFILES`, else the repo's
    /// `profiles/` directory when running from a checkout (dev), else
    /// `<library_root>/profiles` (installed layout, M5 packaging owns it).
    #[cfg(feature = "gui")]
    fn stocks_root() -> PathBuf {
        if let Ok(p) = std::env::var("MELOGRAPHY_PROFILES") {
            return PathBuf::from(p);
        }
        for cand in ["profiles", "../profiles", "../../profiles"] {
            let c = PathBuf::from(cand);
            if c.join("warm-400-negative.json").is_file() {
                return c;
            }
        }
        library_root().join("profiles")
    }

    // The runtime (Wry) only exists with the `gui` feature — the headless
    // `--no-default-features` check type-checks the command/render/state
    // layer, not the window plumbing (Cargo.toml note). run() with gui off
    // is a loud no-op: nothing can start a window without a webview.
    #[cfg(feature = "gui")]
    {
        let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
        let mut state = state::EditorState::new(library_root());
        state.stocks_dir = stocks_root();
        state.stocks = melography_presets::StockRegistry::open(&state.stocks_dir)
            .map(|r| r.profiles)
            .unwrap_or_default(); // browser shows the empty state honestly
        builder
            .manage(Mutex::new(state))
            .invoke_handler(tauri::generate_handler![
                commands::open_image,
                commands::close_image,
                commands::render_preview,
                commands::original_frame,
                commands::import_lut,
                commands::list_luts,
                commands::remove_lut,
                commands::set_lut_stack,
                commands::get_film,
                commands::set_film,
                commands::list_stocks,
                commands::get_tone,
                commands::set_tone,
                commands::auto_light_suggest,
                commands::suggestion_provenance,
                commands::list_models,
                commands::install_model_from_file,
                commands::delete_model,
                commands::get_state,
                commands::undo,
                commands::redo,
                commands::save_recipe,
                commands::export_image,
                // M4 #23: settings, recents (S1), presets (S5), color panel.
                commands::get_settings,
                commands::set_settings,
                commands::list_recents,
                commands::purge_recents,
                commands::list_presets,
                commands::save_preset,
                commands::rename_preset,
                commands::delete_preset,
                commands::apply_preset,
                commands::wb_from_cct_tint,
                commands::stock_thumbs,
            ])
            .run(tauri::generate_context!())
            .expect("MELOGRAPHY window started");
    }
    #[cfg(not(feature = "gui"))]
    {
        // Headless check build (CI matrix + sandbox): commands/render/state
        // are type-checked; there is no webview to start.
        eprintln!("melography-app: headless build — start the GUI with --features gui");
    }
}
