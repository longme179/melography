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

pub mod batch;
pub mod cache;
pub mod commands;
pub mod error;
pub mod export;
pub mod recents;
pub mod render;
pub mod settings;
pub mod state;
pub mod user_presets;

use std::path::{Path, PathBuf};

/// Sentinel stock used to probe candidate registry directories (the
/// registry is "present" when this file is — a dir of random JSON is not
/// a registry). Any shipped stock would do; this one is alphabetically
/// stable and referenced by tests.
pub const STOCK_SENTINEL: &str = "warm-400-negative.json";

/// Library root: `$MELOGRAPHY_DATA`, else `<home>/.melography`
/// (matches the CLI default — one library for both front ends).
pub fn library_root() -> PathBuf {
    melography_cli::default_library_root()
}

/// Resolve the shipped stock-registry directory (session 11).
///
/// Order (first hit wins, probed with [`STOCK_SENTINEL`]):
/// 1. `$MELOGRAPHY_PROFILES` — explicit override (tests, power users);
/// 2. checkout-relative `profiles/` — `tauri dev`/`tauri build` run with
///    cwd = the tauri dir (`apps/melography-app`), so `../../profiles`
///    is the repo's registry (dev + `--bin-only` from a checkout);
/// 3. the bundle resource dir — installed apps (deb/rpm/AppImage/dmg/
///    exe) ship `profiles/` via `tauri.conf.json > bundle.resources`;
/// 4. `<library_root>/profiles` — last resort, parity with the CLI.
///
/// Pure on purpose: the GUI passes the live `resource_dir()`, unit tests
/// pass fakes — resolution is exactly the kind of path logic that broke
/// the installed app while every test ran green from a checkout
/// (session 11 post-mortem).
pub fn resolve_stocks_root(
    env_override: Option<std::ffi::OsString>,
    search_root: &Path,
    resource_dir: Option<&Path>,
    library_root: &Path,
) -> PathBuf {
    if let Some(p) = env_override {
        return PathBuf::from(p);
    }
    for cand in ["profiles", "../profiles", "../../profiles"] {
        let c = search_root.join(cand);
        if c.join(STOCK_SENTINEL).is_file() {
            return c;
        }
    }
    if let Some(res) = resource_dir {
        let c = res.join("profiles");
        if c.join(STOCK_SENTINEL).is_file() {
            return c;
        }
    }
    library_root.join("profiles")
}

/// The runtime (Wry) only exists with the `gui` feature — the headless
/// `--no-default-features` check type-checks the command/render/state
/// layer, not the window plumbing (Cargo.toml note). run() with gui off
/// is a loud no-op: nothing can start a window without a webview.
pub fn run() {
    // The GUI stock resolution: same order as [`resolve_stocks_root`],
    // with the live bundle resource dir (needs a running app handle —
    // that is why the registry opens in `.setup()`, not here: the
    // installed app must find its BUNDLED profiles, which only exist
    // once Tauri resolves the resource dir).
    #[cfg(feature = "gui")]
    {
        use tauri::Manager;

        let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
        let state = state::EditorState::new(library_root());
        let queue = std::sync::Arc::new(batch::BatchQueue::new());
        builder
            .manage(std::sync::Mutex::new(state))
            .manage(queue.clone())
            .setup({
                let queue = queue.clone();
                move |app| {
                    let handle = app.handle().clone();
                    {
                        let state_cell = app.state::<std::sync::Mutex<state::EditorState>>();
                        let mut st = state_cell.lock().unwrap();
                        st.stocks_dir = resolve_stocks_root(
                            std::env::var_os("MELOGRAPHY_PROFILES"),
                            Path::new("."),
                            handle.path().resource_dir().ok().as_deref(),
                            &library_root(),
                        );
                        st.stocks = melography_presets::StockRegistry::open(&st.stocks_dir)
                            .map(|r| r.profiles)
                            .unwrap_or_default(); // the Film Lab empty state names the dir (session 11)
                    }
                    // S6 batch queue (session 08): the worker env snapshots
                    // the immutable roots AFTER stock resolution — exports
                    // with a film stock resolve the same registry the
                    // editor just opened.
                    let worker_env = {
                        let state_cell = app.state::<std::sync::Mutex<state::EditorState>>();
                        let st = state_cell.lock().unwrap();
                        batch::WorkerEnv {
                            library_root: st.library_root(),
                            profiles_dir: st.stocks_dir.clone(),
                        }
                    };
                    batch::spawn_worker(queue, worker_env, handle);
                    Ok(())
                }
            })
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
                commands::cache_stats,
                commands::list_recents,
                commands::purge_recents,
                commands::list_presets,
                commands::save_preset,
                commands::rename_preset,
                commands::delete_preset,
                commands::apply_preset,
                commands::wb_from_cct_tint,
                commands::stock_thumbs,
                commands::wb_pick,
                // S6 batch export queue (session 08, #23 part 2).
                batch::batch_add,
                batch::batch_list,
                batch::batch_thumb,
                batch::batch_pause,
                batch::batch_resume,
                batch::batch_cancel,
                batch::batch_cancel_all,
                batch::batch_retry,
                batch::batch_remove,
                batch::batch_clear,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sentinel_dir(root: &Path) -> PathBuf {
        let dir = root.join("profiles");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(STOCK_SENTINEL), "{}").unwrap();
        dir
    }

    #[test]
    fn env_override_wins_over_everything() {
        let td = tempfile::tempdir().unwrap();
        let search = sentinel_dir(td.path()); // checkout candidate present…
        let resource = sentinel_dir(&td.path().join("res")); // …and resource dir too
        let got = resolve_stocks_root(
            Some("/custom/profiles".into()),
            &search,
            Some(&resource),
            Path::new("/lib"),
        );
        assert_eq!(got, PathBuf::from("/custom/profiles"));
    }

    #[test]
    fn checkout_probe_hits_before_resource_dir() {
        // `tauri dev`/`tauri build` run with cwd = apps/melography-app:
        // ../../profiles is the repo registry and wins over a bundled
        // resource dir (dev must see LIVE edits to profiles).
        let td = tempfile::tempdir().unwrap();
        let cwd = td.path().join("cwd"); // simulated process cwd (a checkout)
        std::fs::create_dir_all(&cwd).unwrap();
        sentinel_dir(&cwd); // cwd/profiles — the "profiles" candidate
        let res_root = td.path().join("res");
        sentinel_dir(&res_root); // res/profiles — a bundled registry too
        let got = resolve_stocks_root(None, &cwd, Some(&res_root), Path::new("/lib"));
        assert_eq!(got, cwd.join("profiles"));
    }

    #[test]
    fn installed_app_falls_back_to_resource_dir() {
        // The session-11 regression: an INSTALLED app runs from /usr/bin
        // or an AppImage mount — no checkout relative to cwd; the bundled
        // resource dir is the only registry source.
        let td = tempfile::tempdir().unwrap();
        let empty_cwd = td.path().join("empty"); // no profiles anywhere near
        std::fs::create_dir_all(&empty_cwd).unwrap();
        let res_root = td.path().join("res");
        sentinel_dir(&res_root); // res/profiles — the bundled registry
        let got = resolve_stocks_root(None, &empty_cwd, Some(&res_root), Path::new("/lib"));
        assert_eq!(got, res_root.join("profiles"));
    }

    #[test]
    fn last_resort_is_the_library_profiles_dir() {
        // Nothing anywhere: fall back to the CLI-convention library dir
        // (parity between front ends; the UI names it in the empty state).
        let td = tempfile::tempdir().unwrap();
        let empty = td.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let got = resolve_stocks_root(None, &empty, None, Path::new("/lib"));
        assert_eq!(got, PathBuf::from("/lib/profiles"));
    }

    /// The packaging contract (session 11): `tauri.conf.json` must map the
    /// repo's stock registry into the bundle resources — the exact omission
    /// that shipped a film editor with no film stocks.
    #[test]
    fn tauri_config_bundles_the_stock_registry() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(manifest.join("tauri.conf.json")).unwrap(),
        )
        .unwrap();
        let resources = conf["bundle"]["resources"]
            .as_object()
            .expect("bundle.resources must exist (the stock registry ships with the app)");
        let maps_profiles = resources.iter().any(|(src, dest)| {
            src.starts_with("../../profiles") && dest.as_str() == Some("profiles/")
        });
        assert!(
            maps_profiles,
            "bundle.resources must map ../../profiles/* -> profiles/ (got {resources:?})"
        );
        // The mapped source actually exists in the repo (a stale mapping
        // would bundle nothing and fail the same way).
        let sentinel = manifest.join("../../profiles").join(STOCK_SENTINEL);
        assert!(sentinel.is_file(), "{} missing", sentinel.display());
    }
}
