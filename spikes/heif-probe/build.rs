// build.rs — link against system libheif via pkg-config.
//
// The go/no-go evidence this probe produces is only meaningful against a
// real system libheif + decode-only plugins. If this script fails, the
// machine cannot build the decode path — that failure IS the honest
// evidence (recorded, not hidden). Required packages (Debian names):
//   libheif-dev, libheif-plugin-libde265 (HEVC decode), libheif-plugin-dav1d (AV1 decode)

use std::process::Command;

fn main() {
    let out = Command::new("pkg-config")
        .args(["--libs", "--cflags", "libheif"])
        .output()
        .expect("pkg-config must be installed to locate libheif");
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        panic!(
            "system libheif not found via pkg-config.\n\
             Install: libheif-dev libheif-plugin-libde265 libheif-plugin-dav1d\n\
             pkg-config said: {stderr}"
        );
    }
    let flags = String::from_utf8_lossy(&out.stdout);
    for tok in flags.split_whitespace() {
        if let Some(dir) = tok.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(lib) = tok.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
        // -I flags are irrelevant: FFI declarations are hand-written in
        // src/lib.rs against the installed headers (no bindgen).
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
}
