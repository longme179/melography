//! Recent-files store (UX_SPEC §3 S1 Home, issue #23).
//!
//! One JSON document in the library root + content-addressed PNG
//! thumbnails under `<library>/thumbs/`. Both go through the durable
//! primitive; a corrupt store resets to empty (recents are a convenience,
//! never a correctness dependency). Missing originals are *listed* (with
//! a `missing` flag) — the Home screen shows them as inline error rows,
//! per UX_SPEC §3 "broken file = inline error row, not a modal".

use melography_core::durable::atomic_write_durable;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Cap: the Home list stays glanceable; older entries age out.
pub const RECENTS_LIMIT: usize = 20;
/// Thumbnail long edge (px) — plenty for a list row at 2× DPI.
const THUMB_DIM: u32 = 128;

/// Open-event payload for [`Recents::record_open`] — a struct, not a
/// 9-arg call (clippy agrees, and callers stay readable).
pub struct RecordOpen<'a> {
    pub path: &'a str,
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    /// A sidecar recipe existed next to the original at open time.
    pub has_recipe: bool,
    /// Downscaled original (RGBA8) for the list thumbnail.
    pub thumb_rgba: Option<(u32, u32, &'a [u8])>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecentEntry {
    /// Absolute path of the original (the open target).
    pub path: String,
    pub name: String,
    /// RFC 3339 UTC of the last open.
    pub last_opened_utc: String,
    pub width: u32,
    pub height: u32,
    /// A sidecar recipe existed next to the original at open time (the
    /// provenance line in the Home list).
    pub has_recipe: bool,
    /// Thumbnail file name inside `<library>/thumbs/` (content hash of
    /// the source, so re-opens reuse the same blob).
    pub thumb: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recents {
    pub entries: Vec<RecentEntry>,
}

/// Minimal FNV-1a content hash for thumb naming (the thumb is a cache
/// key, not a security boundary — the sidecar fingerprint remains the
/// authoritative content hash).
fn thumb_name(source: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}.png")
}

impl Recents {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write_durable(path, &bytes)
    }

    /// Record an open: move-to-front (dedup by path), cap the list,
    /// persist, and write the thumbnail (RGBA8 → PNG) when supplied.
    /// The thumbnail is best-effort — a failure downgrades to no thumb,
    /// never fails the open.
    pub fn record_open(&mut self, store_path: &Path, thumbs_dir: &Path, e: RecordOpen<'_>) {
        self.entries.retain(|r| r.path != e.path);
        let entry = RecentEntry {
            path: e.path.to_string(),
            name: e.name.to_string(),
            last_opened_utc: melography_presets::utc_now_rfc3339(),
            width: e.width,
            height: e.height,
            has_recipe: e.has_recipe,
            thumb: e.thumb_rgba.and_then(|(w, h, rgba)| {
                let fname = thumb_name(e.path);
                write_thumb(thumbs_dir, &fname, w, h, rgba).then_some(fname)
            }),
        };
        self.entries.insert(0, entry);
        self.entries.truncate(RECENTS_LIMIT);
        let _ = self.save(store_path);
    }

    /// Drop entries whose originals no longer exist (an explicit
    /// user-visible "clear broken" action, or housekeeping).
    pub fn purge_missing(&mut self, store_path: &Path) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| Path::new(&e.path).is_file());
        let removed = before - self.entries.len();
        if removed > 0 {
            let _ = self.save(store_path);
        }
        removed
    }

    /// Read a thumbnail as PNG bytes for the webview (data-URL-able).
    pub fn thumb_bytes(thumbs_dir: &Path, name: &str) -> Option<Vec<u8>> {
        // Defense: the stored name is ours, but a hand-edited store must
        // not turn into an arbitrary file read.
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return None;
        }
        std::fs::read(thumbs_dir.join(name)).ok()
    }
}

/// Write an RGBA8 thumbnail as PNG (downscaled to `THUMB_DIM` on the long
/// edge by simple box sampling — this is a list glyph, not a render).
/// Alpha composites over the app's neutral dark chrome color (the io
/// encoder is RGB8; photos are opaque, transparency reads as dark).
fn write_thumb(dir: &Path, name: &str, w: u32, h: u32, rgba: &[u8]) -> bool {
    let _ = std::fs::create_dir_all(dir);
    let scale = THUMB_DIM as f64 / f64::from(w.max(h)).max(1.0);
    let scale = scale.min(1.0);
    let tw = ((f64::from(w) * scale).round() as u32).max(1);
    let th = ((f64::from(h) * scale).round() as u32).max(1);
    let mut tmp_rgba = vec![0u8; (tw * th * 4) as usize];
    box_downsample(w, h, rgba, tw, th, &mut tmp_rgba);
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    const BG: f32 = 14.0; // ~#0e0f11 — the app chrome dark
    for px in tmp_rgba.as_chunks::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        for v in &px[..3] {
            let v = f32::from(*v) * a + BG * (1.0 - a);
            rgb.push(v.round().clamp(0.0, 255.0) as u8);
        }
    }
    // One encoder, one code path (the io crate's, with the 16-bit twin
    // next to it); write to a temp name then rename into place.
    let tmp = dir.join(format!("{name}.tmp"));
    match melography_io::encode::encode_png8(&tmp, tw, th, &rgb, None) {
        Ok(()) => std::fs::rename(&tmp, dir.join(name)).is_ok(),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

/// Integer box filter: each target pixel averages the source box that
/// maps to it. Deterministic, allocation-free, good enough at 128 px.
fn box_downsample(sw: u32, sh: u32, src: &[u8], tw: u32, th: u32, dst: &mut [u8]) {
    let (sw, sh) = (sw as usize, sh as usize);
    let (tw, th) = (tw as usize, th as usize);
    for ty in 0..th {
        let y0 = ty * sh / th;
        let y1 = ((ty + 1) * sh / th).max(y0 + 1);
        for tx in 0..tw {
            let x0 = tx * sw / tw;
            let x1 = ((tx + 1) * sw / tw).max(x0 + 1);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1.min(sh) {
                let row = y * sw;
                for x in x0..x1.min(sw) {
                    let i = (row + x) * 4;
                    r += u64::from(src[i]);
                    g += u64::from(src[i + 1]);
                    b += u64::from(src[i + 2]);
                    a += u64::from(src[i + 3]);
                    n += 1;
                }
            }
            if n == 0 {
                n = 1;
            }
            let o = (ty * tw + tx) * 4;
            dst[o] = (r / n) as u8;
            dst[o + 1] = (g / n) as u8;
            dst[o + 2] = (b / n) as u8;
            dst[o + 3] = (a / n) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                v[i] = (x * 255 / w.max(1)) as u8;
                v[i + 1] = (y * 255 / h.max(1)) as u8;
                v[i + 2] = 128;
                v[i + 3] = 255;
            }
        }
        v
    }

    #[test]
    fn record_open_moves_to_front_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("recents.json");
        let mut r = Recents::default();
        for i in 0..(RECENTS_LIMIT + 5) {
            r.record_open(
                &store,
                dir.path(),
                RecordOpen {
                    path: &format!("/img/{i}.png"),
                    name: &format!("{i}.png"),
                    width: 100,
                    height: 80,
                    has_recipe: i % 2 == 0,
                    thumb_rgba: None,
                },
            );
        }
        assert_eq!(r.entries.len(), RECENTS_LIMIT);
        assert_eq!(r.entries[0].path, format!("/img/{}.png", RECENTS_LIMIT + 4));
        // reload from disk persists
        let loaded = Recents::load(&store);
        assert_eq!(loaded, r);
    }

    #[test]
    fn dedup_by_path_updates_not_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("recents.json");
        let mut r = Recents::default();
        r.record_open(&store, dir.path(), rec("/a.png", false));
        r.record_open(&store, dir.path(), rec("/b.png", false));
        r.record_open(&store, dir.path(), rec("/a.png", true));
        assert_eq!(r.entries.len(), 2);
        assert_eq!(r.entries[0].path, "/a.png");
        assert!(r.entries[0].has_recipe, "re-open refreshes provenance");
    }

    fn rec(path: &'static str, has_recipe: bool) -> RecordOpen<'static> {
        RecordOpen {
            path,
            name: "x.png",
            width: 10,
            height: 10,
            has_recipe,
            thumb_rgba: None,
        }
    }

    #[test]
    fn thumb_written_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = dir.path().join("thumbs");
        let mut r = Recents::default();
        let rgba = rgba_gradient(64, 48);
        r.record_open(
            &dir.path().join("recents.json"),
            &thumbs,
            RecordOpen {
                path: "/pic.png",
                name: "pic.png",
                width: 64,
                height: 48,
                has_recipe: true,
                thumb_rgba: Some((64, 48, &rgba)),
            },
        );
        let name = r.entries[0].thumb.clone().expect("thumb recorded");
        let bytes = Recents::thumb_bytes(&thumbs, &name).expect("thumb readable");
        assert!(bytes.len() > 8);
        assert_eq!(&bytes[1..4], b"PNG", "it is a PNG");
    }

    #[test]
    fn thumb_read_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Recents::thumb_bytes(dir.path(), "../../etc/passwd").is_none());
        assert!(Recents::thumb_bytes(dir.path(), "a/b.png").is_none());
    }

    #[test]
    fn purge_missing_removes_only_gone_files() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("keep.png");
        std::fs::write(&keep, b"x").unwrap();
        let store = dir.path().join("recents.json");
        let mut r = Recents::default();
        let keep_str = keep.to_string_lossy().into_owned();
        r.record_open(
            &store,
            dir.path(),
            RecordOpen {
                path: &keep_str,
                name: "keep.png",
                width: 1,
                height: 1,
                has_recipe: false,
                thumb_rgba: None,
            },
        );
        r.record_open(&store, dir.path(), rec("/gone.png", false));
        assert_eq!(r.purge_missing(&store), 1);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].name, "keep.png");
    }

    #[test]
    fn corrupt_store_resets_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("recents.json");
        std::fs::write(&store, b"garbage").unwrap();
        assert_eq!(Recents::load(&store), Recents::default());
    }

    #[test]
    fn box_downsample_corners_and_size() {
        // 4×4 → 2×2: each target averages a 2×2 box.
        let src: Vec<u8> = vec![
            0, 0, 0, 255, 10, 10, 10, 255, 20, 20, 20, 255, 30, 30, 30, 255, //
            40, 40, 40, 255, 50, 50, 50, 255, 60, 60, 60, 255, 70, 70, 70, 255, //
            80, 80, 80, 255, 90, 90, 90, 255, 100, 100, 100, 255, 110, 110, 110, 255, //
            120, 120, 120, 255, 130, 130, 130, 255, 140, 140, 140, 255, 150, 150, 150, 255,
        ];
        let mut dst = vec![0u8; 2 * 2 * 4];
        box_downsample(4, 4, &src, 2, 2, &mut dst);
        // top-left 2×2 box = rows 0-1, cols 0-1 → [0,10,40,50] → 25
        assert_eq!(dst[0], 25);
        assert_eq!(dst[3], 255);
        // bottom-right 2×2 box = rows 2-3, cols 2-3 → [100,110,140,150]
        let i = 3 * 4; // (row 1, col 1) in a 2×2 target
        assert_eq!(dst[i] as u32, (100u32 + 110 + 140 + 150) / 4);
    }
}
