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
    let mut img =
        melography_core::ImageF32::alloc(w, h, melography_core::Channels::Rgb, 0.0);
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
    let dir = std::env::temp_dir().join(format!(
        "melography-app-it-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // "Open" a session: decode-free construction of the same state the
    // command layer builds (AP1 base + fingerprint).
    let ap1 = warm_gradient_image(96, 64);
    let fingerprint = Fingerprint::compute(b"fake-original-bytes", &ap1);

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
    let f1 = compose_preview(&ap1, &mut st.preview_cache, &stack, 64).unwrap();
    assert_eq!((f1.width, f1.height), (64, 43));
    assert!(f1.rgba.len() == 64 * 43 * 4);
    let f2 = compose_preview(&ap1, &mut st.preview_cache, &stack, 64).unwrap();
    assert_eq!(f1.rgba, f2.rgba);
    assert_eq!(st.preview_cache.as_ref().unwrap().key, base_key);

    // Parameter change invalidates.
    st.lut_stack[0].intensity = 0.0;
    let stack2 = st.resolve_stack().unwrap();
    let f3 = compose_preview(&ap1, &mut st.preview_cache, &stack2, 64).unwrap();
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
    std::fs::remove_dir_all(&dir).ok();
}
