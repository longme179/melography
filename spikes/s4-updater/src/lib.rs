//! # Spike S4 — Tauri v2 updater dummy-channel roundtrip (issue #6, ADR-0005)
//!
//! Proves the updater *mechanics* end-to-end against the **real
//! `tauri-plugin-updater`** running in a **mock runtime** (no webview, no
//! GUI — CI-safe), with:
//!
//! - a **throwaway minisign keypair** generated in-memory per test run
//!   (never written to disk, never committed — test channel only; the
//!   production key is owner-held per ADR-0005 §2);
//! - a **local HTTP "dummy channel"** (std TcpListener on 127.0.0.1)
//!   serving the static `latest.json` manifest format and the bundle —
//!   the manifest is generated at request time so its bundle URL always
//!   references this channel's own ephemeral port;
//! - `check → download → verify` where verification is the plugin's own
//!   minisign check (a returned download means the signature passed).
//!
//! Negative paths are first-class evidence: tampered bundle, garbage
//! signature, and version monotonicity (older manifest ⇒ no update, and
//! no bundle fetch).
//!
//! Honest scope (what this spike does NOT do): `install()` is NOT
//! exercised — swapping executables needs real platform bundles and is
//! M5 territory (issue #16, gated release workflow). Channel-ID refusal
//! (ADR-0005 §1) is enforced by our *manifest generator* at M5, not by
//! the plugin, so it is covered there, not here. The live
//! `/releases/latest` semantics probe lives in `live_latest_probe.sh`
//! (run in CI; recorded in S4_REPORT.md).
//!
//! Run the evidence: `cargo test` (this crate is excluded from the main
//! workspace on purpose — it pulls the tauri tree).

use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

use minisign::{KeyPair, SecretKey, SignatureBox};
use serde_json::json;

/// A route served by [`DummyChannel`].
#[derive(Clone)]
pub enum Route {
    /// Static bytes at a path.
    Static {
        path: &'static str,
        content_type: &'static str,
        body: Vec<u8>,
    },
    /// `GET /latest.json` builds the manifest on request, pointing
    /// `bundle_path` at this channel's own port.
    Manifest {
        version: &'static str,
        signature: String,
        bundle_path: &'static str,
    },
}

/// A local dummy update channel (std TcpListener on 127.0.0.1).
pub struct DummyChannel {
    pub port: u16,
    hits: Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>>,
}

impl DummyChannel {
    /// Start the channel; serves in background threads until dropped.
    pub fn start(routes: Vec<Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        Self::_start_with(listener, port, routes)
    }

    fn _start_with(listener: TcpListener, port: u16, routes: Vec<Route>) -> Self {
        let hits = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let hits_srv = hits.clone();
        let routes_srv = routes.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                // read the request head (we only route on the first line)
                let mut head = vec![0u8; 8192];
                let mut n = 0;
                loop {
                    if n >= head.len() {
                        break;
                    }
                    match stream.read(&mut head[n..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => {
                            n += k;
                            if head[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let text = String::from_utf8_lossy(&head[..n]).into_owned();
                let first_line = text.lines().next().unwrap_or_default().to_string();
                let path = first_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_string();

                if let Ok(mut h) = hits_srv.lock() {
                    h.entry(path.clone()).and_modify(|c| *c += 1).or_insert(1);
                }

                // resolve the route into (content_type, body)
                let known = routes_srv.iter().any(|r| match r {
                    Route::Static { path: p, .. } => *p == path,
                    Route::Manifest { .. } => path == "/latest.json",
                });
                let resp: Option<(&'static str, Vec<u8>)> =
                    routes_srv.iter().find_map(|r| match r {
                        Route::Static {
                            path: p,
                            content_type,
                            body,
                        } if *p == path => Some((*content_type, body.clone())),
                        Route::Manifest {
                            version,
                            signature,
                            bundle_path,
                        } if path == "/latest.json" => {
                            let url = format!("http://127.0.0.1:{port}{bundle_path}");
                            let manifest = build_manifest(version, signature, &url);
                            Some(("application/json", manifest.into_bytes()))
                        }
                        _ => None,
                    });
                let (status, content_type, body) = match (known, resp) {
                    (true, Some((ct, body))) => ("200 OK", ct, body),
                    _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
                };
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        Self { port, hits }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    /// how many times a route was requested (manifest fetch, bundle fetch)
    pub fn hits_of(&self, path: &str) -> usize {
        self.hits
            .lock()
            .map(|h| h.get(path).copied().unwrap_or(0))
            .unwrap_or(0)
    }
}

/// Deterministic bundle bytes (update payload content is arbitrary for the
/// mechanics; realism of size is what matters).
pub fn dummy_bundle(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push((x & 0xff) as u8);
    }
    out
}

/// Generate a throwaway in-memory keypair; returns (pubkey_param,
/// secret_key). Nothing touches disk (test-channel only, ADR-0005 §2).
/// Unencrypted on purpose: the key lives for one process and is never
/// persisted, so a password would only migrate complexity, not risk.
///
/// `pubkey_param` is **base64 of the full .pub box** (comment line + key
/// line) — the format the plugin's `base64_to_string(pubkey)` expects
/// (verified against the plugin's own test fixtures).
pub fn throwaway_keypair() -> (String, SecretKey) {
    let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair generation");
    let pk_box_str = pk.to_box().expect("public key box").to_string();
    let pk_param = b64(pk_box_str.as_bytes());
    (pk_param, sk)
}

fn b64(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Sign `data`; returns the manifest `signature` value: **base64 of the
/// full SignatureBox string** (untrusted comment + sig base64 + trusted
/// comment + global-sig base64) — the format the plugin's
/// `base64_to_string(signature)` expects.
pub fn sign_bundle(sk: &SecretKey, data: &[u8]) -> String {
    let reader = Cursor::new(data.to_vec());
    let sig_box: SignatureBox = minisign::sign(None, sk, reader, None, None).expect("sign");
    b64(sig_box.into_string().as_bytes())
}

/// Build a static-format update manifest (v2 plugin shape) for the
/// current target (portable across CI OSes).
pub fn build_manifest(version: &str, platform_sig: &str, url: &str) -> String {
    let target = tauri_plugin_updater::target().unwrap_or_else(|| "linux-x86_64".into());
    json!({
        "version": version,
        "notes": format!("dummy channel {version} — test channel only, never published"),
        "pub_date": "2026-10-24T00:00:00Z",
        "platforms": {
            target: {
                "signature": platform_sig,
                "url": url,
            }
        }
    })
    .to_string()
}

/// Current app version the mock app reports.
/// Defined in `test-app/tauri.conf.json` ("0.1.0") — the same file carries
/// the `plugins.updater` config the plugin requires at init (mock_context
/// alone provides an empty plugins map, which the updater rejects).
/// Manifests must beat this (2.0.0) or lose to it (0.0.9).
pub const MOCK_APP_VERSION: &str = "0.1.0";

/// Build a mock-runtime app whose context comes from the real tauri.conf
/// (generate_context!) — the sanctioned pattern from tauri::test's docs.
fn make_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(tauri::generate_context!())
        .expect("mock app builds")
}

/// One positive roundtrip on a fresh app + channel; returns the report
/// object used by `main.rs` (and available to tests).
pub fn run_report() -> serde_json::Value {
    use tauri_plugin_updater::UpdaterExt;

    let app = make_app();

    let (pk_b64, sk) = throwaway_keypair();
    let payload = dummy_bundle(128 * 1024);
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
            body: payload,
        },
    ]);

    let updater = app
        .updater_builder()
        .endpoints(vec![channel.url("/latest.json").parse().expect("url")])
        .expect("endpoints valid")
        .pubkey(pk_b64)
        .build()
        .expect("updater builds");

    let t0 = std::time::Instant::now();
    let update = tauri::async_runtime::block_on(updater.check())
        .expect("check succeeds")
        .expect("update announced");
    let check_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let bytes = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
        .expect("download verifies the minisign signature");
    let download_ms = t1.elapsed().as_secs_f64() * 1000.0;

    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "current_version": update.current_version,
        "announced_version": update.version,
        "bundle_bytes": bytes.len(),
        "manifest_fetches": channel.hits_of("/latest.json"),
        "bundle_fetches": channel.hits_of("/bundle-v2.0.0.bin"),
        "check_ms": check_ms,
        "download_verify_ms": download_ms,
        "signature_scheme": "minisign (tauri-plugin-updater 2.x, verified during download)",
        "channel": "dummy localhost HTTP — test channel only, never published",
        "install_exercised": false,
        "install_note": "install() is M5 territory (real bundles, gated workflow, issue #16)",
    })
}
