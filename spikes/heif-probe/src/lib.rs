//! # HEIF/AVIF go-no-go probe (PLAN M0 item 8, R-IN-2)
//!
//! Question this spike answers: **can MELOGRAPHY decode HEIC (camera
//! imports) and AVIF through system libheif (LGPL, dynamically linked)
//! with decode-only plugins (libde265 for HEVC, dav1d for AV1)?**
//!
//! Evidence produced:
//! 1. **build evidence** — `build.rs` links against the system libheif
//!    via pkg-config; on a machine without the backend, the build fails
//!    with an explicit message (that failure is the go/no-go answer for
//!    that machine — honest, not hidden);
//! 2. **decode evidence** — both committed fixtures decode to the
//!    expected dimensions with plausible pixel content (the fixtures
//!    are real HEVC / real AV1 payloads — see
//!    `fixtures/provenance.json`);
//! 3. **timing** — decode wall time per format for the report.
//!
//! Licensing posture (ADR-0003, GPL-3.0-or-later project): libheif is
//! LGPL-3.0 — consumed as a *dynamically linked* system library, which
//! the LGPL permits for any licensed host; we never statically link
//! it, never enable HEVC *encoding* (x265 is GPL and HEVC carries
//! patent risk — the product only *decodes* camera files; exporting
//! stays JPEG/PNG/TIFF/WebP per R-OUT-1), and the probe's own encoding
//! of test fixtures happened once on the dev machine via the
//! pillow-heif wheel (documented in provenance.json), never on user
//! machines.
//!
//! This spike is standalone (excluded from the main workspace) because
//! the production `io` crate gets a thin `libheif` *feature* at M1;
//! until then the probe holds the evidence.

use std::ffi::{c_char, CStr, CString};
use std::time::Instant;

/// Opaque libheif types (C API).
mod sys {
    use std::ffi::{c_char, c_int, c_uint};

    pub type HeifContext = core::ffi::c_void;
    pub type HeifImageHandle = core::ffi::c_void;
    pub type HeifImage = core::ffi::c_void;
    pub type HeifReadingOptions = core::ffi::c_void;
    pub type HeifDecodingOptions = core::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HeifError {
        pub code: c_int,
        pub subcode: c_int,
        pub message: *const c_char,
    }

    extern "C" {
        pub fn heif_context_alloc() -> *mut HeifContext;
        pub fn heif_context_free(ctx: *mut HeifContext);
        pub fn heif_context_read_from_memory_without_copy(
            ctx: *mut HeifContext,
            mem: *const u8,
            size: usize,
            opts: *const HeifReadingOptions,
        ) -> HeifError;
        pub fn heif_context_get_primary_image_handle(
            ctx: *mut HeifContext,
            out: *mut *mut HeifImageHandle,
        ) -> HeifError;
        pub fn heif_image_handle_release(handle: *const HeifImageHandle);
        pub fn heif_decode_image(
            handle: *const HeifImageHandle,
            out: *mut *mut HeifImage,
            colorspace: c_uint,
            chroma: c_uint,
            opts: *const HeifDecodingOptions,
        ) -> HeifError;
        pub fn heif_image_get_width(img: *const HeifImage, channel: c_uint) -> c_int;
        pub fn heif_image_get_height(img: *const HeifImage, channel: c_uint) -> c_int;
        pub fn heif_image_get_plane_readonly2(
            img: *const HeifImage,
            channel: c_uint,
            out_stride: *mut usize,
        ) -> *const u8;
        pub fn heif_image_release(img: *const HeifImage);
        pub fn heif_get_version() -> *const c_char;
    }
}

// enum values read from the installed headers (heif_image.h):
const COLORSPACE_RGB: u32 = 1;
const CHROMA_INTERLEAVED_RGB: u32 = 10; // = heif_chroma_interleaved_24bit
const CHANNEL_INTERLEAVED: u32 = 10;
const ERROR_OK: i32 = 0; // heif_error_Ok

#[derive(Debug)]
pub struct HeifError {
    pub code: i32,
    pub subcode: i32,
    pub message: String,
}

impl std::fmt::Display for HeifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "libheif error {}: {}", self.code, self.message)
    }
}

/// A decoded image: packed 24-bit interleaved RGB.
#[derive(Debug)]
pub struct DecodedRgb {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    /// Row-major, `height` rows of `width * 3` bytes (stride stripped).
    pub data: Vec<u8>,
}

/// libheif version string of the system library (build-time linkage +
/// runtime version — both from the same .so).
pub fn libheif_version() -> String {
    unsafe {
        let p = sys::heif_get_version();
        if p.is_null() {
            return "(null)".into();
        }
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn err_from(e: sys::HeifError) -> HeifError {
    let message = if e.message.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(e.message) }
            .to_string_lossy()
            .into_owned()
    };
    HeifError {
        code: e.code,
        subcode: e.subcode,
        message,
    }
}

fn check(e: sys::HeifError, ctx: &str) -> Result<(), HeifError> {
    if e.code == ERROR_OK {
        Ok(())
    } else {
        let mut err = err_from(e);
        err.message = format!("[{ctx}] {}", err.message);
        Err(err)
    }
}

/// Decode the primary image of a HEIF/AVIF file (bytes already in
/// memory) to interleaved RGB24.
pub fn decode_rgb(bytes: &[u8]) -> Result<DecodedRgb, HeifError> {
    unsafe {
        let ctx = sys::heif_context_alloc();
        if ctx.is_null() {
            return Err(HeifError {
                code: -1,
                subcode: 0,
                message: "heif_context_alloc returned null".into(),
            });
        }
        let r = {
            let result: Result<DecodedRgb, HeifError>;
            if let Err(e) = check(
                sys::heif_context_read_from_memory_without_copy(
                    ctx,
                    bytes.as_ptr(),
                    bytes.len(),
                    std::ptr::null(),
                ),
                "read_from_memory",
            ) {
                result = Err(e);
            } else {
                let mut handle: *mut sys::HeifImageHandle = std::ptr::null_mut();
                if let Err(e) = check(
                    sys::heif_context_get_primary_image_handle(ctx, &mut handle),
                    "primary_image_handle",
                ) {
                    result = Err(e);
                } else {
                    let mut image: *mut sys::HeifImage = std::ptr::null_mut();
                    if let Err(e) = check(
                        sys::heif_decode_image(
                            handle,
                            &mut image,
                            COLORSPACE_RGB,
                            CHROMA_INTERLEAVED_RGB,
                            std::ptr::null(),
                        ),
                        "decode_image",
                    ) {
                        result = Err(e);
                    } else {
                        let width = sys::heif_image_get_width(image, CHANNEL_INTERLEAVED);
                        let height = sys::heif_image_get_height(image, CHANNEL_INTERLEAVED);
                        let mut stride: usize = 0;
                        let plane = sys::heif_image_get_plane_readonly2(
                            image,
                            CHANNEL_INTERLEAVED,
                            &mut stride,
                        );
                        if plane.is_null()
                            || width <= 0
                            || height <= 0
                            || stride < width as usize * 3
                        {
                            result = Err(HeifError {
                                code: -1,
                                subcode: 0,
                                message: format!(
                                    "bad plane: ptr={plane:?} {width}x{height} stride={stride}"
                                ),
                            });
                        } else {
                            let w = width as usize;
                            let h = height as usize;
                            let mut data = vec![0u8; w * h * 3];
                            for (row, out) in data.chunks_exact_mut(w * 3).enumerate() {
                                let src = plane.add(row * stride);
                                std::ptr::copy_nonoverlapping(src, out.as_mut_ptr(), w * 3);
                            }
                            result = Ok(DecodedRgb {
                                width: width as u32,
                                height: height as u32,
                                stride,
                                data,
                            });
                        }
                        sys::heif_image_release(image);
                    }
                    sys::heif_image_handle_release(handle);
                }
            }
            sys::heif_context_free(ctx);
            result
        };
        r
    }
}

/// One decode measurement (used by `main.rs` and tests).
pub struct DecodeMeasurement {
    pub format: &'static str,
    pub bytes_in: usize,
    pub decode_ms: f64,
    pub width: u32,
    pub height: u32,
}

/// Decode + time; returns the decoded image for content checks.
pub fn measure(
    bytes: &[u8],
    format: &'static str,
) -> Result<(DecodeMeasurement, DecodedRgb), HeifError> {
    let t = Instant::now();
    let img = decode_rgb(bytes)?;
    let decode_ms = t.elapsed().as_secs_f64() * 1000.0;
    Ok((
        DecodeMeasurement {
            format,
            bytes_in: bytes.len(),
            decode_ms,
            width: img.width,
            height: img.height,
        },
        img,
    ))
}

/// `main.rs` report: version + per-format decode evidence.
pub fn run_report(heic: &[u8], avif: &[u8]) -> serde_json_value::Value {
    // tiny hand-rolled JSON (the probe deliberately has zero deps)
    let mut items = Vec::new();
    for (label, data) in [("heic", heic), ("avif", avif)] {
        match measure(data, label) {
            Ok((m, img)) => items.push(format!(
                "{{\"format\":\"{label}\",\"bytes_in\":{},\"decode_ms\":{:.3},\"width\":{},\"height\":{},\"stride\":{},\"rgb_bytes\":{}}}",
                m.bytes_in, m.decode_ms, m.width, m.height, img.stride, img.data.len()
            )),
            Err(e) => items.push(format!(
                "{{\"format\":\"{label}\",\"bytes_in\":{},\"error\":{}}}",
                data.len(),
                json_escape(&e.to_string())
            )),
        }
    }
    let version = libheif_version();
    format!(
        "{{\n  \"libheif_version\": {},\n  \"os\": \"{}\",\n  \"results\": [\n    {}\n  ],\n  \"decode_only_plugins_expected\": \"libde265 (HEVC) + dav1d (AV1)\",\n  \"note\": \"product policy: dynamic link to system libheif; no HEVC encoding (R-OUT-1 exports stay JPEG/PNG/TIFF/WebP)\"\n}}",
        json_escape(&version),
        std::env::consts::OS,
        items.join(",\n    ")
    )
    .into()
}

mod serde_json_value {
    /// stand-in for a JSON value string (zero-dep probe)
    #[derive(Debug)]
    pub struct Value(pub String);
    impl std::fmt::Display for Value {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl From<String> for Value {
        fn from(s: String) -> Self {
            Value(s)
        }
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Fixture paths (repo-relative). Unused-but-documented helper for main.
pub fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// Keep a CString-free helper unused warning away (path helper used in main).
#[allow(dead_code)]
fn _unused(c: CString) {
    let _ = c;
    let _: Option<c_char> = None;
}
