//! S4 spike integration tests — the CI-enforced form of the evidence
//! (issue #6 exit criteria). Each test builds its own mock-runtime app
//! and local dummy channel; keypairs are throwaway and in-memory only.

use s4_updater_spike::*;
use std::time::Instant;

use tauri_plugin_updater::UpdaterExt;

// `tauri::test::mock_context` hardcodes package_info.version = "0.1.0";
// the updater sees exactly that (see lib.rs MOCK_APP_VERSION docs).
const CURRENT: &str = MOCK_APP_VERSION;

fn make_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(tauri::generate_context!())
        .expect("mock app builds")
}

fn bundle(len: usize) -> Vec<u8> {
    dummy_bundle(len)
}

#[test]
fn s4_roundtrip_positive_check_download_verify() {
    let app = make_app();
    let (pk_b64, sk) = throwaway_keypair();
    let payload = bundle(128 * 1024);
    let sig = sign_bundle(&sk, &payload);

    let channel = DummyChannel::start(vec![
        Route::Manifest {
            version: "2.0.0",
            signature: sig,
            bundle_path: "/bundle-v2.0.0.bin",
        },
        Route::Static {
            path: "/bundle-v2.0.0.bin",
            content_type: "application/octet-stream",
            body: payload.clone(),
        },
    ]);

    let updater = app
        .updater_builder()
        .endpoints(vec![channel.url("/latest.json").parse().expect("url")])
        .expect("endpoints valid")
        .pubkey(pk_b64)
        .build()
        .expect("updater builds");

    let t0 = Instant::now();
    let update = tauri::async_runtime::block_on(updater.check())
        .expect("check succeeds")
        .expect("update announced (2.0.0 > 0.1.0)");
    let check_ms = t0.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(update.version, "2.0.0");
    assert_eq!(update.current_version, CURRENT);
    assert_eq!(channel.hits_of("/latest.json"), 1);

    let t1 = Instant::now();
    let bytes = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
        .expect("download verifies the minisign signature");
    let download_ms = t1.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(bytes.len(), payload.len());
    assert_eq!(bytes, payload, "downloaded bytes must be the bundle");
    assert_eq!(channel.hits_of("/bundle-v2.0.0.bin"), 1);

    // the standalone verifier agrees (and rejects foreign bytes)
    update
        .verify(&bytes)
        .expect("verify accepts the real bundle");
    update
        .verify(b"not the bundle")
        .expect_err("verify rejects foreign bytes");

    println!(
        "s4 roundtrip: check {check_ms:.1} ms + download(verify) {download_ms:.1} ms, {} bytes, os={}",
        bytes.len(),
        std::env::consts::OS
    );
}

#[test]
fn s4_negative_tampered_bundle_signature() {
    let app = make_app();
    let (pk_b64, sk) = throwaway_keypair();
    // sign the ORIGINAL payload, then serve a byte-flipped mutant:
    // valid signature, wrong data — verification must reject it
    let original = bundle(64 * 1024);
    let sig = sign_bundle(&sk, &original);
    let mut served = original.clone();
    served[100] ^= 0xff;
    debug_assert_ne!(served, original);

    let channel = DummyChannel::start(vec![
        Route::Manifest {
            version: "2.0.0",
            signature: sig,
            bundle_path: "/tampered.bin",
        },
        Route::Static {
            path: "/tampered.bin",
            content_type: "application/octet-stream",
            body: served,
        },
    ]);

    let updater = app
        .updater_builder()
        .endpoints(vec![channel.url("/latest.json").parse().unwrap()])
        .unwrap()
        .pubkey(pk_b64)
        .build()
        .unwrap();

    let update = tauri::async_runtime::block_on(updater.check())
        .unwrap()
        .expect("announced");
    let err = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
        .expect_err("tampered bundle must fail verification");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("signature") || msg.contains("verification"),
        "expected a signature/verification-related failure, got: {msg}"
    );
    println!("s4 negative/tampered: download rejected: {msg}");
}

#[test]
fn s4_negative_garbage_signature_field() {
    let app = make_app();
    let (pk_b64, _sk) = throwaway_keypair();
    let served = bundle(32 * 1024);
    let garbage_sig = "this is not a minisign signature box".to_string();

    let channel = DummyChannel::start(vec![
        Route::Manifest {
            version: "2.0.0",
            signature: garbage_sig,
            bundle_path: "/garbage.bin",
        },
        Route::Static {
            path: "/garbage.bin",
            content_type: "application/octet-stream",
            body: served,
        },
    ]);

    let updater = app
        .updater_builder()
        .endpoints(vec![channel.url("/latest.json").parse().unwrap()])
        .unwrap()
        .pubkey(pk_b64)
        .build()
        .unwrap();
    let update = tauri::async_runtime::block_on(updater.check())
        .unwrap()
        .expect("announced");
    let err = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
        .expect_err("garbage signature must fail");
    println!("s4 negative/garbage-sig: download rejected: {err}");
}

#[test]
fn s4_negative_older_version_not_an_update() {
    let app = make_app();
    let (pk_b64, sk) = throwaway_keypair();
    let payload = bundle(16 * 1024);
    let sig = sign_bundle(&sk, &payload);

    let channel = DummyChannel::start(vec![
        // 0.0.9 < current 0.1.0: must not be announced (default
        // comparator: update iff manifest version > current version)
        Route::Manifest {
            version: "0.0.9",
            signature: sig,
            bundle_path: "/old.bin",
        },
        Route::Static {
            path: "/old.bin",
            content_type: "application/octet-stream",
            body: payload,
        },
    ]);

    let updater = app
        .updater_builder()
        .endpoints(vec![channel.url("/latest.json").parse().unwrap()])
        .unwrap()
        .pubkey(pk_b64)
        .build()
        .unwrap();

    let announced = tauri::async_runtime::block_on(updater.check())
        .expect("check succeeds (manifest is valid JSON)");
    assert!(
        announced.is_none(),
        "an older version must NOT be announced as an update (anti-downgrade, ADR-0005 §3)"
    );
    assert_eq!(channel.hits_of("/latest.json"), 1);
    assert_eq!(channel.hits_of("/old.bin"), 0, "no bundle fetch may happen");
    println!("s4 negative/older-version: correctly not an update (no bundle fetch)");
}

#[test]
fn s4_platform_key_matches_current_target() {
    // the manifest generator uses the plugin's own target() so the same
    // harness works on all three CI operating systems
    let target = tauri_plugin_updater::target().expect("supported desktop target");
    let os = std::env::consts::OS;
    assert!(
        target.contains(os) || (os == "macos" && target.starts_with("darwin")),
        "target()={target} must reflect the current OS ({os})"
    );
    println!("s4 target key on this OS: {target}");
}
