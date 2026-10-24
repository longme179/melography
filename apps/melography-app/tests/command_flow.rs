//! Command-layer integration test (no GUI, no tauri runtime — the exact
//! flow `open_image`/`set_lut_stack`/`render_preview` drive, minus the IPC
//! wrappers): session open → library import → hash-verified resolve →
//! cached preview composition → cache invalidation on parameter change.

use melography_app_lib::render::{compose_original, compose_preview, stack_key};
use melography_app_lib::state::EditorState;
use melography_lut::stage::{InsertionPoint, LutApply};
use melography_lut::{LutEncoding, LutLibrary};
use melography_presets::sidecar::Fingerprint;
use std::sync::Arc;

fn identity_cube(n: u32) -> String {
    let mut s = format!("TITLE \"it\"\nLUT_3D_SIZE {n}\n");
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                s.push_str(&format!(
                    "{} {} {}\n",
                    i as f32 / (n - 1) as f32,
                    j as f32 / (n - 1) as f32,
                    k as f32 / (n - 1) as f32
                ));
            }
        }
    }
    s
}

fn warm_gradient_image(w: u32, h: u32) -> melography_core::ImageF32 {
    let mut img = melography_core::ImageF32::alloc(w, h, melography_core::Channels::Rgb, 0.0);
    for y in 0..h {
        for x in 0..w {
            let px = img.pixel_mut(x, y);
            px[0] = x as f32 / (w - 1) as f32 * 0.8;
            px[1] = y as f32 / (h - 1) as f32 * 0.7;
            px[2] = 0.2;
        }
    }
    img
}

#[test]
fn session_library_preview_flow() {
    // Unique, panic-safe temp dir (drop = cleanup; replaces the hand-rolled
    // nanos-clock naming — see the lut crate note on the macOS µs-granularity
    // collision flake, PR #22 run 40086829632).
    let _td = tempfile::tempdir().unwrap();
    let dir = _td.path().to_path_buf();

    // "Open" a session: decode-free construction of the same state the
    // command layer builds (AP1 base + fingerprint).
    let ap1 = warm_gradient_image(96, 64);
    // Fingerprint computed exactly as open_image does (kept to mirror the
    // command flow; the session field itself is exercised via sidecars).
    let _fingerprint = Fingerprint::compute(b"fake-original-bytes", &ap1);

    // Import a LUT through the real library.
    let lut_path = dir.join("look.cube");
    std::fs::write(&lut_path, identity_cube(17)).unwrap();
    let mut st = EditorState::new(dir.join("library"));
    let record = st
        .library
        .import(
            &lut_path,
            LutEncoding::SrgbDisplay,
            LutEncoding::SrgbDisplay,
            Default::default(),
        )
        .unwrap();

    // User sets the stack (set_lut_stack semantics).
    st.lut_stack = vec![LutApply::new(
        record.content_hash.clone(),
        InsertionPoint::Finishing,
        0.65,
        LutEncoding::SrgbDisplay,
        LutEncoding::SrgbDisplay,
    )];

    // resolve_stack: hash-verified load through the library.
    let stack = st.resolve_stack().unwrap();
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].lut.size, 17);
    assert_eq!(stack[0].apply.intensity, 0.65);

    // Preview render + cache behavior.
    let base_key = stack_key(&ap1, &stack);
    let f1 = compose_preview(&ap1, &mut st.preview_cache, &stack, None, None, 64).unwrap();
    assert_eq!((f1.width, f1.height), (64, 43));
    assert!(f1.rgba.len() == 64 * 43 * 4);
    let f2 = compose_preview(&ap1, &mut st.preview_cache, &stack, None, None, 64).unwrap();
    assert_eq!(f1.rgba, f2.rgba);
    assert_eq!(st.preview_cache.as_ref().unwrap().key, base_key);

    // Parameter change invalidates.
    st.lut_stack[0].intensity = 0.0;
    let stack2 = st.resolve_stack().unwrap();
    let f3 = compose_preview(&ap1, &mut st.preview_cache, &stack2, None, None, 64).unwrap();
    assert_ne!(f1.rgba, f3.rgba);

    // Missing LUT → explicit error state (never a skip).
    st.lut_stack[0].lut_hash = "does-not-exist".into();
    let err = st.resolve_stack().unwrap_err();
    assert_eq!(err.code, "lut.missing");

    // Original compare path is LUT-independent.
    let o1 = compose_original(&ap1, 64);
    let o2 = compose_original(&ap1, 64);
    assert_eq!(o1.rgba, o2.rgba);
    assert_eq!(o1.width, 64);

    // Library root accessor mirrors construction (app display path).
    assert_eq!(
        LutLibrary::open(st.library.root()).root(),
        st.library.root()
    );

    let _ = Arc::new(0); // keep Arc import meaningful for future shared luts
}

/// M4 issue #24 — undo/redo semantics at the state layer (the exact
/// machinery the `undo`/`redo` commands drive): push-before-mutate,
/// restore-on-undo, redo cleared by new mutations, history reset per
/// session, bound honored.
#[test]
fn undo_redo_roundtrip_and_semantics() {
    let td = tempfile::tempdir().unwrap();
    let mut st = EditorState::new(td.path().to_path_buf());

    // Fresh session: nothing to undo/redo.
    assert!(st.history.is_empty() && st.redo.is_empty());

    // Mutation 1: LUT stack.
    st.push_history();
    st.lut_stack = vec![LutApply {
        lut_hash: "aaaa".into(),
        insertion: InsertionPoint::PreFilm,
        intensity: 0.5,
        declared_input: LutEncoding::SrgbDisplay,
        declared_output: LutEncoding::SrgbDisplay,
        enabled: true,
    }];
    st.dirty = true;

    // Mutation 2: tone.
    st.push_history();
    st.tone.params = Some(melography_color::tone::ToneParams::default());
    st.dirty = true;

    // Undo once → back to "LUT only".
    let snap = st.history.pop().unwrap();
    st.redo.push(st.snapshot());
    st.restore(snap);
    assert!(st.tone.params.is_none(), "tone undone");
    assert_eq!(st.lut_stack.len(), 1, "LUT mutation still present");

    // Redo → tone back.
    let snap = st.redo.pop().unwrap();
    st.history.push(st.snapshot());
    st.restore(snap);
    assert!(st.tone.params.is_some(), "tone restored");
    assert_eq!(st.lut_stack.len(), 1);

    // Undo again (to LUT-only), then a NEW mutation must clear redo.
    let snap = st.history.pop().unwrap();
    st.redo.push(st.snapshot());
    st.restore(snap);
    assert!(st.tone.params.is_none());
    st.push_history(); // new mutation invalidates the redo branch
    assert!(st.redo.is_empty(), "redo cleared by new mutation");

    // Restore marks dirty (next render recomputes).
    st.dirty = false;
    let snap = st.history.pop().unwrap();
    st.restore(snap);
    assert!(st.dirty, "restore marks the recipe dirty");

    // Session reset clears both stacks.
    st.reset_history();
    assert!(st.history.is_empty() && st.redo.is_empty());
}

/// M4 issue #24 — the undo bound: oldest entries drop, memory stays
/// bounded (UX_SPEC §4 "infinite within session, bounded persistence").
#[test]
fn undo_history_is_bounded() {
    let td = tempfile::tempdir().unwrap();
    let mut st = EditorState::new(td.path().to_path_buf());
    let limit = melography_app_lib::state::UNDO_LIMIT;
    for i in 0..(limit + 25) {
        st.push_history();
        st.lut_stack = vec![LutApply {
            lut_hash: format!("hash-{i}"),
            insertion: InsertionPoint::Finishing,
            intensity: 0.1,
            declared_input: LutEncoding::SrgbDisplay,
            declared_output: LutEncoding::SrgbDisplay,
            enabled: true,
        }];
    }
    assert_eq!(st.history.len(), limit, "bound enforced");
    // The oldest surviving snapshot is the (25+1)-th mutation's pre-state.
    assert_eq!(st.history[0].lut_stack.len(), 1);
    assert_eq!(st.history[0].lut_stack[0].lut_hash, "hash-24");
}
