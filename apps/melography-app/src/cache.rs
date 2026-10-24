//! Bounded caches (#25, issue text: "Cache budgets configurable +
//! enforced; cache hit-rate surfaced in S2 status bar").
//!
//! Two caches with byte budgets derived from `Settings.cache_budget_mb`
//! (previously advisory — now enforced):
//!
//! * [`PreviewCacheState`] — the single full-resolution render slot the
//!   viewport composes from. Budget = the full setting. A rendered image
//!   larger than the budget is NOT cached (honest degradation: the next
//!   interaction re-renders; surfaced via `cache_stats` as
//!   `enabled: false`) rather than silently blowing the memory ceiling.
//! * [`LutCache`] — the loaded-and-verified LUT resolution cache, an
//!   LRU with a byte budget of one quarter of the setting (floor 64 MiB).
//!   A single LUT larger than the budget is served but not retained.
//!
//! Both count hits/misses; the `cache_stats` command exposes them and
//! the S2 status bar shows the preview hit-rate next to render-ms.
//! All eviction is by least-recent-use; nothing here can fail a render —
//! budgets degrade caching, never correctness.

use melography_lut::CubeLut;
use std::sync::Arc;

use crate::render::RenderCache;

/// LUT-cache share of the settings budget (the preview slot gets the
/// rest). Quarter share, floored at 64 MiB — a typical 33-point LUT is
/// ~0.4 MiB, so even the floor holds 150+ LUTs.
pub fn lut_budget_from_total(total_bytes: u64) -> u64 {
    (total_bytes / 4).max(64 * 1024 * 1024)
}

/// LRU of loaded LUTs keyed by content hash. Vec-backed: entry counts
/// are tens at most (the stack + recently used), so the O(n) touch is
/// invisible next to a hash-verified disk load.
pub struct LutCache {
    /// (hash, lut, approx_bytes) — back = most recent.
    entries: Vec<(String, Arc<CubeLut>, u64)>,
    budget_bytes: u64,
    resident_bytes: u64,
    pub hits: u64,
    pub misses: u64,
}

impl LutCache {
    pub fn new(budget_bytes: u64) -> Self {
        Self {
            entries: Vec::new(),
            budget_bytes,
            resident_bytes: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Resident bytes actually held (sum of `CubeLut::approx_bytes`).
    pub fn resident_bytes(&self) -> u64 {
        self.resident_bytes
    }

    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Look up by content hash; a hit moves the entry to the back
    /// (most-recently-used) and counts toward the hit-rate.
    pub fn get(&mut self, hash: &str) -> Option<Arc<CubeLut>> {
        if let Some(idx) = self.entries.iter().position(|(h, _, _)| h == hash) {
            self.hits += 1;
            let entry = self.entries.remove(idx);
            self.entries.push(entry);
            self.entries.last().map(|(_, l, _)| l.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert (after a hash-verified load). Evicts least-recently-used
    /// entries until the cache fits its budget; a single LUT larger
    /// than the whole budget is not retained (it is still returned to
    /// the caller — the cache must never fail a render).
    pub fn insert(&mut self, hash: String, lut: Arc<CubeLut>) -> Arc<CubeLut> {
        let bytes = lut.approx_bytes();
        if bytes > self.budget_bytes {
            return lut; // oversize: serve, don't retain
        }
        // Replace-in-place if the hash is already resident.
        if let Some(idx) = self.entries.iter().position(|(h, _, _)| *h == hash) {
            let (_, old, old_bytes) = self.entries.remove(idx);
            self.resident_bytes -= old_bytes;
            drop(old);
        }
        self.resident_bytes += bytes;
        self.entries.push((hash, lut.clone(), bytes));
        self.evict_to_budget();
        lut
    }

    /// Drop a specific entry (LUT removed from the library).
    pub fn remove(&mut self, hash: &str) {
        if let Some(idx) = self.entries.iter().position(|(h, _, _)| h == hash) {
            let (_, _, bytes) = self.entries.remove(idx);
            self.resident_bytes -= bytes;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.resident_bytes = 0;
        // Hit/miss counters survive a clear: the rate is a session
        // metric, not a per-image one.
    }

    /// Live budget change (Settings). Evicts immediately if the new
    /// budget is below current residency.
    pub fn set_budget(&mut self, budget_bytes: u64) {
        self.budget_bytes = budget_bytes;
        self.evict_to_budget();
    }

    fn evict_to_budget(&mut self) {
        while self.resident_bytes > self.budget_bytes && !self.entries.is_empty() {
            let (_, _, bytes) = self.entries.remove(0); // front = LRU
            self.resident_bytes -= bytes;
        }
    }
}

/// The preview render slot + its budget + hit/miss counters. Replaces
/// the old bare `Option<RenderCache>`.
pub struct PreviewCacheState {
    pub slot: Option<RenderCache>,
    pub budget_bytes: u64,
    pub hits: u64,
    pub misses: u64,
}

impl PreviewCacheState {
    pub fn new(budget_bytes: u64) -> Self {
        Self {
            slot: None,
            budget_bytes,
            hits: 0,
            misses: 0,
        }
    }

    /// Bytes held by the cached render (0 when uncached).
    pub fn cached_bytes(&self) -> u64 {
        self.slot
            .as_ref()
            .map(|c| c.image.byte_len() as u64)
            .unwrap_or(0)
    }

    /// Live budget change; drops the slot if it no longer fits.
    pub fn set_budget(&mut self, budget_bytes: u64) {
        self.budget_bytes = budget_bytes;
        if self.cached_bytes() > budget_bytes {
            self.slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use melography_lut::cube::parse_cube;

    fn lut_33() -> Arc<CubeLut> {
        // Real 33-point LUT (~0.4 MiB table).
        let mut text = String::from("TITLE \"t\"\nLUT_3D_SIZE 33\n");
        for i in 0..33 * 33 * 33 {
            let v = i as f32 / (33.0 * 33.0 * 33.0);
            text.push_str(&format!("{v} {v} {v}\n"));
        }
        Arc::new(parse_cube(&text).expect("lut parses"))
    }

    #[test]
    fn lru_evicts_least_recently_used_under_budget() {
        // Budget that fits exactly 2 of these LUTs.
        let one = lut_33().approx_bytes();
        let mut c = LutCache::new(one * 2);
        c.insert("a".into(), lut_33());
        c.insert("b".into(), lut_33());
        assert_eq!(c.len(), 2);
        c.insert("c".into(), lut_33()); // evicts "a" (LRU)
        assert_eq!(c.len(), 2);
        assert!(c.get("a").is_none(), "a was evicted");
        assert!(c.get("b").is_some(), "b was touched after a — survives");
        assert!(c.get("c").is_some());
        // get("a") counted a miss; b/c counted hits.
        assert_eq!(c.misses, 1);
        assert_eq!(c.hits, 2);
    }

    #[test]
    fn touch_promotes_recency() {
        let one = lut_33().approx_bytes();
        let mut c = LutCache::new(one * 2);
        c.insert("a".into(), lut_33());
        c.insert("b".into(), lut_33());
        assert!(c.get("a").is_some()); // promote a
        c.insert("c".into(), lut_33()); // evicts b now
        assert!(c.get("b").is_none());
        assert!(c.get("a").is_some());
    }

    #[test]
    fn oversize_lut_is_served_but_not_retained() {
        let mut c = LutCache::new(1024); // absurdly small budget
        let l = lut_33();
        let served = c.insert("big".into(), l.clone());
        assert_eq!(served.approx_bytes(), l.approx_bytes(), "served anyway");
        assert_eq!(c.len(), 0, "not retained");
        assert_eq!(c.resident_bytes(), 0);
    }

    #[test]
    fn budget_change_evicts_immediately() {
        let one = lut_33().approx_bytes();
        let mut c = LutCache::new(one * 3);
        for k in ["a", "b", "c"] {
            c.insert(k.into(), lut_33());
        }
        assert_eq!(c.len(), 3);
        c.set_budget(one);
        assert_eq!(c.len(), 1, "shrunk to the newest entry");
        assert!(c.get("c").is_some());
    }

    #[test]
    fn remove_drops_residency() {
        let one = lut_33().approx_bytes();
        let mut c = LutCache::new(one * 4);
        c.insert("a".into(), lut_33());
        c.insert("b".into(), lut_33());
        let before = c.resident_bytes();
        c.remove("a");
        assert_eq!(c.resident_bytes(), before - one);
        assert!(c.get("a").is_none());
    }

    #[test]
    fn clear_resets_residency_keeps_counters() {
        let mut c = LutCache::new(u64::MAX);
        c.insert("a".into(), lut_33());
        c.get("a");
        c.clear();
        assert_eq!(c.resident_bytes(), 0);
        assert_eq!(c.hits, 1, "counters are session metrics");
    }
}
