//! IPC contract test (session 11) — the gate that was missing when the
//! app shipped with a JSON key-case mismatch on its main state channel.
//!
//! What happened: Tauri v2 deserializes command ARGUMENTS with camelCase
//! keys (the macro converts `maxDim` → `max_dim`), but RESPONSES are
//! serde-verbatim — `StateJson { image_open }` arrived as `"image_open"`
//! while the UI read `imageOpen` (undefined) and the editor never left
//! the Home screen. Every CI gate was green because nothing exercised
//! the real webview bridge.
//!
//! This test closes that hole mechanically: it parses the TypeScript
//! interfaces the UI actually consumes (`ui/src/lib/ipc.ts`,
//! `ui/src/lib/stores.ts`) and compares them, field by field, with the
//! JSON the Rust command layer serializes. A field added, renamed or
//! re-cased on either side without the other fails HERE, in CI, not on
//! the owner's machine.

use melography_ai::manager::{ModelInfo, ModelState};
use melography_app_lib::batch::{BatchJob, BatchQueue, JobStatus};
use melography_app_lib::commands::{
    cache_stats_json, FilmStateJson, ImageInfo, ImportLutArgs, LutRecordJson, LutStackEntry,
    LutStackJson, PresetJson, RecentJson, StateJson, StockJson, StockThumbJson, SuggestionJson,
    ToneStateJson,
};
use melography_app_lib::export::ExportSpec;
use melography_app_lib::state::EditorState;
use melography_color::tone::{ToneParams, WbGains};
use melography_film::{Adjustments, LensParams};
use melography_presets::sidecar::{StageEntry, SuggestionKind, SuggestionSource};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

// ─── TypeScript interface parser (just enough for .ts interface decls) ──────

#[derive(Debug, Clone)]
enum TsType {
    /// `field: OtherInterface` — recurse when the JSON value is an object.
    Ref(String),
    /// `field: { a: x; b: y }` — inline object; recurse into its fields.
    Inline(BTreeMap<String, TsType>),
    /// Primitive / union / unresolvable — presence-only.
    Other,
}

/// Parsed interfaces: name → ordered field set (BTreeMap: stable diffs).
type Interfaces = BTreeMap<String, BTreeMap<String, TsType>>;

fn parse_interfaces(src: &str) -> Interfaces {
    let mut out = Interfaces::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while let Some(start) = src[i..].find("interface ") {
        let i0 = i + start;
        // name
        let name_start = i0 + "interface ".len();
        let name_end = src[name_start..]
            .find(|c: char| c == '{' || c.is_whitespace())
            .map(|e| name_start + e)
            .expect("interface name");
        let name = src[name_start..name_end].trim().to_string();
        let Some(open_rel) = src[name_start..].find('{') else {
            i = name_end;
            continue;
        };
        let open = name_start + open_rel;
        let (body, next) = matching_braces(bytes, open);
        out.insert(name, parse_fields(body));
        i = next;
    }
    out
}

/// Return the text between the brace at `open` (exclusive) and its match
/// (exclusive), plus the index just past the closing brace.
fn matching_braces(bytes: &[u8], open: usize) -> (&str, usize) {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (
                        std::str::from_utf8(&bytes[open + 1..i]).expect("utf8"),
                        i + 1,
                    );
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unbalanced braces at byte {open}");
}

/// Split a body into `name: type` statements at depth 0 (nested inline
/// objects and arrays stay inside one statement); comments are skipped.
fn parse_fields(body: &str) -> BTreeMap<String, TsType> {
    let mut fields = BTreeMap::new();
    let mut depth = 0i32;
    let mut stmt = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                // line comment — skip to EOL
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        stmt.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                // block comment
                chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
                stmt.push(' ');
            }
            '{' | '[' | '(' => {
                depth += 1;
                stmt.push(c);
            }
            '}' | ']' | ')' => {
                depth -= 1;
                stmt.push(c);
            }
            ';' | ',' if depth == 0 => {
                if let Some((name, ty)) = parse_statement(&stmt) {
                    fields.insert(name, ty);
                }
                stmt.clear();
            }
            _ => stmt.push(c),
        }
    }
    if let Some((name, ty)) = parse_statement(&stmt) {
        fields.insert(name, ty);
    }
    fields
}

fn parse_statement(stmt: &str) -> Option<(String, TsType)> {
    let stmt = stmt.trim();
    if stmt.is_empty() {
        return None;
    }
    let (name, ty) = stmt.split_once(':')?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None; // method signatures, index signatures, spread…
    }
    let ty = ty.trim();
    let ts_type = if ty.starts_with('{') {
        let (inner, _) = matching_braces(ty.as_bytes(), 0);
        TsType::Inline(parse_fields(inner))
    } else {
        // strip array / null / undefined wrappers to find a plain ref
        let base = ty
            .trim_end_matches("[]")
            .split('|')
            .map(str::trim)
            .find(|t| !t.is_empty() && *t != "null" && *t != "undefined")
            .unwrap_or("");
        if base.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !base.is_empty()
            && base.chars().next().is_some_and(|c| c.is_uppercase())
        {
            TsType::Ref(base.to_string())
        } else {
            TsType::Other
        }
    };
    Some((name.to_string(), ts_type))
}

// ─── Comparison ─────────────────────────────────────────────────────────────

fn assert_matches(
    path: &str,
    ts: &BTreeMap<String, TsType>,
    interfaces: &Interfaces,
    json: &Value,
) {
    let Value::Object(obj) = json else {
        panic!("{path}: expected a JSON object, got {json:?}");
    };
    let ts_keys: Vec<&str> = ts.keys().map(String::as_str).collect();
    let json_keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        ts_keys, json_keys,
        "{path}: TS interface and Rust JSON keys disagree — \
         the webview reads undefined for every missing/renamed key \
         (session 11: exactly how the shipped app got stuck on Home)"
    );
    for (key, ty) in ts {
        let val = &obj[key];
        let child = format!("{path}.{key}");
        match (ty, val) {
            (TsType::Inline(inner), Value::Object(_)) => {
                assert_matches(&child, inner, interfaces, val);
            }
            (TsType::Ref(name), Value::Object(_)) => {
                if let Some(next) = interfaces.get(name) {
                    assert_matches(&child, next, interfaces, val);
                }
                // not an interface (union type alias) — presence-only.
            }
            (TsType::Ref(name), Value::Array(items)) => {
                if let (Some(next), Some(first)) = (interfaces.get(name), items.first()) {
                    assert_matches(&child, next, interfaces, first);
                }
            }
            // Inline arrays (`warnings: { kind }[]`) and primitives:
            // presence-only (key-set equality above already pinned them).
            _ => {}
        }
    }
}

// ─── Rust samples (the exact wire structs the commands return) ──────────────

/// `ToneParams` with every optional PRESENT: the sidecar-compat wire
/// format skips `None` fields, so key-set equality only holds with a
/// fully-populated sample (the UI contract is "absent ≈ null" — falsy).
fn full_tone_params() -> ToneParams {
    ToneParams {
        ev: 0.0,
        highlights: 0.0,
        shadows: 0.0,
        wb: Some(WbGains::NEUTRAL),
        black_source: Some(0.0),
        black_target: Some(0.0),
        white_source: Some(1.0),
        white_target: Some(1.0),
    }
}

fn samples() -> Vec<(&'static str, Value)> {
    let state = StateJson {
        image_open: true,
        image_name: Some("photo.jpg".into()),
        image_dims: Some([6000, 4000]),
        lut_stack: vec![LutStackJson {
            lut_hash: "abc123".into(),
            insertion: "finishing".into(),
            intensity: 0.5,
            declared_input: "srgb-display".into(),
            declared_output: "srgb-display".into(),
            enabled: true,
        }],
        dirty: true,
        sidecar_path: Some("/p/photo.melography.json".into()),
        last_clamped_pixels: 0,
        library_count: 1,
        can_undo: true,
        can_redo: false,
        film_stock: Some("warm-400-negative".into()),
        missing_stock: None,
        source_path: Some("/p/photo.jpg".into()),
        stocks_dir: "/usr/lib/melography/profiles".into(),
        stocks_count: 5,
    };

    let image = ImageInfo {
        name: "photo.jpg".into(),
        width: 6000,
        height: 4000,
        sidecar_loaded: false,
        fingerprint_sha256: "deadbeef".into(),
    };

    let lut = LutRecordJson {
        content_hash: "abc123".into(),
        original_filename: "look.cube".into(),
        title: Some("Look".into()),
        kind: "Lut3d".into(),
        size: 33,
        domain_min: [0.0; 3],
        domain_max: [1.0; 3],
        declared_input: "srgb-display".into(),
        declared_output: "srgb-display".into(),
        source_url: None,
        license_note: None,
        imported_utc: "2026-10-25T00:00:00Z".into(),
        warnings: vec![],
        implemented: true,
    };

    let film = FilmStateJson {
        stock: Some("warm-400-negative".into()),
        enabled: true,
        adjustments: Adjustments::default(),
        lens: LensParams::default(),
    };

    let stock = StockJson {
        id: "warm-400-negative".into(),
        display_name: "Warm 400".into(),
        provenance: "ArtistAuthored".into(),
        confidence: "Medium".into(),
        polarity: "Negative".into(),
        layers: "CurveGrain".into(),
        default_path: "Classic".into(),
        provenance_text: "artist-authored — no measured calibration".into(),
    };

    let source = SuggestionSource {
        kind: SuggestionKind::Baseline,
        name: "autolight-baseline-v1".into(),
        version: "1.0.0".into(),
        weights_sha256: Some("cafebabe".into()),
        accepted_utc: "2026-10-25T00:00:00Z".into(),
    };
    let tone = ToneStateJson {
        // Every optional filled: `ToneParams` skips `None` fields on the
        // wire (sidecar-compat), so the key set is only complete when
        // present — the UI contract is "absent ≈ null" (falsy), and this
        // sample pins that every TS-declared key is a REAL Rust key.
        params: Some(ToneParams {
            ev: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            wb: Some(WbGains::NEUTRAL),
            black_source: Some(0.0),
            black_target: Some(0.0),
            white_source: Some(1.0),
            white_target: Some(1.0),
        }),
        source: Some(source),
        cct_estimate_k: Some(5500.0),
        tint_estimate: Some(0.0),
    };

    let suggestion = SuggestionJson {
        params: ToneParams {
            ev: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            wb: Some(WbGains::NEUTRAL),
            black_source: Some(0.0),
            black_target: Some(0.0),
            white_source: Some(1.0),
            white_target: Some(1.0),
        },
        notes: vec!["n".into()],
        wb_cct_estimate_k: Some(5500.0),
        engine: "autolight-baseline-v1".into(),
        engine_version: "1.0.0".into(),
        analysis_ms: 1.0,
    };

    let model = ModelInfo {
        id: "u2net".into(),
        version: "1.0".into(),
        task: "segmentation".into(),
        output_kind: serde_json::from_value(json!("mask")).unwrap(),
        size_bytes: 1,
        weights_license: "Apache-2.0".into(),
        sha256: "abc".into(),
        state: ModelState::NotInstalled,
        known_issues: vec![],
    };

    let recent = RecentJson {
        path: "/p/photo.jpg".into(),
        name: "photo.jpg".into(),
        last_opened_utc: "2026-10-25T00:00:00Z".into(),
        width: 6000,
        height: 4000,
        has_recipe: true,
        missing: false,
        thumb_data_url: Some("data:image/png;base64,x".into()),
    };

    let preset = PresetJson {
        id: "p1".into(),
        name: "My look".into(),
        created_utc: "2026-10-25T00:00:00Z".into(),
        modified_utc: "2026-10-25T00:00:00Z".into(),
        source: "user".into(),
        summary: "warm-400-negative".into(),
    };

    let thumb = StockThumbJson {
        stock_id: "warm-400-negative".into(),
        thumb_data_url: "data:image/png;base64,x".into(),
    };

    let batch_job = BatchJob {
        id: 1,
        source: "/p/photo.jpg".into(),
        name: "photo.jpg".into(),
        preset_id: None,
        preset_name: None,
        stages: vec![StageEntry::FilmIdentity {
            enabled: true,
            extra: Default::default(),
        }],
        spec: ExportSpec {
            format: "png".into(),
            depth: 8,
            quality: 92,
        },
        dest_dir: "/out".into(),
        dest: None,
        status: JobStatus::Queued,
        error: None,
        warnings: vec![],
        dims: None,
        bytes: None,
        total_ms: None,
        added_at: "2026-10-25T00:00:00Z".into(),
        finished_at: None,
        thumb: None,
    };

    vec![
        ("AppState", serde_json::to_value(&state).unwrap()),
        ("ImageInfo", serde_json::to_value(&image).unwrap()),
        ("LutRecord", serde_json::to_value(&lut).unwrap()),
        ("FilmState", serde_json::to_value(&film).unwrap()),
        ("Stock", serde_json::to_value(&stock).unwrap()),
        (
            "FilmAdjustments",
            serde_json::to_value(Adjustments::default()).unwrap(),
        ),
        (
            "LensParams",
            serde_json::to_value(LensParams::default()).unwrap(),
        ),
        (
            "ToneParams",
            serde_json::to_value(full_tone_params()).unwrap(),
        ),
        ("WbGains", serde_json::to_value(WbGains::NEUTRAL).unwrap()),
        ("ToneState", serde_json::to_value(&tone).unwrap()),
        ("Suggestion", serde_json::to_value(&suggestion).unwrap()),
        ("ModelInfo", serde_json::to_value(&model).unwrap()),
        ("AppSettings", {
            let td = tempfile::tempdir().unwrap();
            serde_json::to_value(EditorState::new(td.path().to_path_buf()).settings).unwrap()
        }),
        ("RecentItem", serde_json::to_value(&recent).unwrap()),
        ("UserPreset", serde_json::to_value(&preset).unwrap()),
        ("StockThumb", serde_json::to_value(&thumb).unwrap()),
        ("BatchJob", batch_job.to_json()),
        ("BatchSnapshot", BatchQueue::new().snapshot()),
        (
            "CacheStats",
            cache_stats_json(&{
                let td = tempfile::tempdir().unwrap();
                EditorState::new(td.path().to_path_buf())
            }),
        ),
    ]
}

// ─── The test ───────────────────────────────────────────────────────────────

#[test]
fn ui_interfaces_match_rust_wire_json() {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/lib");
    let ipc_ts = std::fs::read_to_string(ui.join("ipc.ts")).expect("ipc.ts readable");
    let stores_ts = std::fs::read_to_string(ui.join("stores.ts")).expect("stores.ts readable");
    let interfaces = {
        let mut all = parse_interfaces(&ipc_ts);
        all.extend(parse_interfaces(&stores_ts));
        all
    };
    assert!(!interfaces.is_empty(), "parsed no TS interfaces?");

    for (name, json) in samples() {
        let ts = interfaces
            .get(name)
            .unwrap_or_else(|| panic!("interface {name} not found in the TS sources"));
        assert_matches(name, ts, &interfaces, &json);
    }
}

/// Request-side contract: the JSON the components actually SEND must
/// deserialize into the command argument structs. Tauri converts only
/// TOP-LEVEL argument keys to camelCase — nested payloads are serde
/// verbatim, which is why these spell out the exact wire shapes.
#[test]
fn ui_payloads_deserialize_into_command_args() {
    // LutPanel.push() — camelCase nested entry (fixed in session 11).
    let stack: Vec<LutStackEntry> = serde_json::from_value(json!([{
        "lutHash": "abc123",
        "insertion": "finishing",
        "intensity": 0.65,
        "declaredInput": "srgb-display",
        "declaredOutput": "srgb-display",
        "enabled": true
    }]))
    .expect("LutPanel's camelCase entry must deserialize");
    assert_eq!(stack[0].lut_hash, "abc123");
    assert_eq!(stack[0].intensity, 0.65);

    // ImportLutDialog — camelCase import args.
    let import: ImportLutArgs = serde_json::from_value(json!({
        "path": "/tmp/look.cube",
        "declaredInput": "srgb-display",
        "declaredOutput": "srgb-display",
        "sourceUrl": null,
        "licenseNote": null
    }))
    .expect("ImportLutDialog's camelCase args must deserialize");
    assert_eq!(import.path, "/tmp/look.cube");
}
