//! HEIF-probe CI tests: the committed fixtures must decode through the
//! system libheif with plausible content (go/no-go evidence, issue
//! tracked in the M0 report). Tolerances are loose on purpose: the
//! fixtures were encoded lossily (quality 60); we assert structure
//! (dimensions, gradient direction, rough levels), not exact pixels.

use heif_probe_spike::*;

const W: u32 = 64;
const H: u32 = 48;
/// Lossy-encode tolerance for a q60 tiny gradient (empirical bound).
const TOL: i32 = 48;

fn load(name: &str) -> Vec<u8> {
    let p = fixture_path(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("fixture {p:?} unreadable: {e}"))
}

fn recipe(x: u32, y: u32) -> (i32, i32, i32) {
    (
        ((x * 4) % 256) as i32,
        ((y * 5) % 256) as i32,
        (((x + y) * 3) % 256) as i32,
    )
}

fn assert_plausible_content(img: &DecodedRgb, label: &str) {
    assert_eq!((img.width, img.height), (W, H), "{label}: dimensions");
    let px = |x: u32, y: u32| -> (i32, i32, i32) {
        let i = (y as usize * W as usize + x as usize) * 3;
        (
            img.data[i] as i32,
            img.data[i + 1] as i32,
            img.data[i + 2] as i32,
        )
    };
    // corners + a mid pixel against the recipe
    for (x, y) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1), (32, 24)] {
        let (r, g, b) = px(x, y);
        let (er, eg, eb) = recipe(x, y);
        assert!(
            (r - er).abs() <= TOL && (g - eg).abs() <= TOL && (b - eb).abs() <= TOL,
            "{label}: pixel({x},{y}) = ({r},{g},{b}) vs recipe ({er},{eg},{eb})"
        );
    }
    // red channel must grow along +x (recipe step 4/px; lossy kept monotone)
    for x in 0..W - 1 {
        let (r0, _, _) = px(x, 5);
        let (r1, _, _) = px(x + 1, 5);
        assert!(
            r1 >= r0 - 2,
            "{label}: red not monotone at x={x}: {r0} -> {r1}"
        );
    }
}

#[test]
fn heif_probe_decode_heic_fixture() {
    let bytes = load("gradient.heic");
    // brand sanity: real HEVC payload (guards the mislabeled-fixture bug)
    assert_eq!(&bytes[4..8], b"ftyp", "heic: not an ISOBMFF box start");
    assert!(
        &bytes[8..12] == b"heic" || &bytes[8..12] == b"heix" || &bytes[8..12] == b"mif1",
        "heic: unexpected major brand {:?}",
        &bytes[8..12]
    );
    let (m, img) = measure(&bytes, "heic").expect("HEIC decode must work via libde265");
    assert_plausible_content(&img, "heic");
    println!(
        "heif-probe heic: {} bytes -> {:?}x{:?} in {:.3} ms",
        m.bytes_in, m.width, m.height, m.decode_ms
    );
}

#[test]
fn heif_probe_decode_avif_fixture() {
    let bytes = load("gradient.avif");
    assert_eq!(&bytes[4..8], b"ftyp", "avif: not an ISOBMFF box start");
    assert_eq!(
        &bytes[8..12],
        b"avif",
        "avif: unexpected major brand {:?}",
        &bytes[8..12]
    );
    let (m, img) = measure(&bytes, "avif").expect("AVIF decode must work via dav1d");
    assert_plausible_content(&img, "avif");
    println!(
        "heif-probe avif: {} bytes -> {:?}x{:?} in {:.3} ms",
        m.bytes_in, m.width, m.height, m.decode_ms
    );
}

#[test]
fn heif_probe_version_and_linkage() {
    let v = libheif_version();
    assert!(v.starts_with("1."), "system libheif version string: {v:?}");
    println!(
        "heif-probe linked libheif: {v} (os={})",
        std::env::consts::OS
    );
}

#[test]
fn heif_probe_rejects_garbage_honestly() {
    let garbage = b"not a heif file at all, just bytes";
    let err = decode_rgb(garbage).expect_err("garbage must not decode");
    println!("heif-probe garbage input rejected: {err}");
}
