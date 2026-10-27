//! Session GPU runtime (A2.4 — full-chain GPU preview wiring).
//!
//! Owns the preview film-leg dispatch decision and the honest
//! degradation contract:
//!
//! * `user_enabled = false` (Settings) → CPU leg, no GPU touched.
//! * No adapter at init → CPU leg for the session, reason surfaced
//!   (`gpu_status`), not counted as a failure.
//! * The pinned adapter fingerprint is absent → CPU leg for the
//!   session with the honest reason (NEVER a silent wrong-adapter
//!   render — the A4 contract).
//! * A render/init error → THIS frame falls back to the CPU leg and
//!   the failure is counted; 3 consecutive failures disable the GPU
//!   leg for the session (device-loss-style protection — reset by
//!   changing the adapter in Settings or restarting).
//!
//! The CPU fallback is the A2.1/A2.2 threaded fast-grain leg — the
//! preview never blocks, never errors to the user over this.

use crate::settings::Settings;
use melography_core::ImageF32;
use melography_film::chain::FilmChainParams;

/// Consecutive failures before the session circuit breaker opens.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// The adapter policy implied by the persisted settings.
pub fn policy_from_settings(s: &Settings) -> melography_gpu::context::AdapterPolicy {
    if s.gpu_adapter == "auto" {
        melography_gpu::context::AdapterPolicy::Auto
    } else {
        melography_gpu::context::AdapterPolicy::ByFingerprint(s.gpu_adapter.clone())
    }
}

#[derive(Debug)]
pub struct GpuRuntime {
    /// Mirrors `Settings.gpu_enabled` (synced by `sync_from_settings`;
    /// the render path never reads Settings directly).
    user_enabled: bool,
    policy: melography_gpu::context::AdapterPolicy,
    consecutive_failures: u32,
    /// Session circuit breaker (no adapter / pinned adapter absent /
    /// 3 consecutive failures). Stays off until Settings changes the
    /// adapter or the app restarts — honest, never silent.
    session_disabled: bool,
    /// The honest reason for the current CPU-only state, if any.
    reason: Option<String>,
}

impl Default for GpuRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuRuntime {
    pub fn new() -> Self {
        Self {
            user_enabled: true,
            policy: melography_gpu::context::AdapterPolicy::Auto,
            consecutive_failures: 0,
            session_disabled: false,
            reason: None,
        }
    }

    /// Tests force the CPU path regardless of settings — determinism:
    /// a runner WITH an adapter must not change test bytes (the GPU
    /// composition is parity-gated in `melography-film`, not here).
    pub fn cpu_for_tests() -> Self {
        Self {
            user_enabled: false,
            ..Self::new()
        }
    }

    /// Re-read the user's intent (Settings load or change). An adapter
    /// change re-arms the circuit breaker and drops the shared context
    /// so the next render binds the new pick.
    pub fn sync_from_settings(&mut self, s: &Settings) {
        let new_policy = policy_from_settings(s);
        let adapter_changed = new_policy != self.policy;
        self.user_enabled = s.gpu_enabled;
        if adapter_changed {
            self.policy = new_policy;
            self.session_disabled = false;
            self.consecutive_failures = 0;
            self.reason = None;
            melography_gpu::context::reset();
        }
    }

    pub fn session_disabled(&self) -> bool {
        self.session_disabled
    }

    pub fn user_enabled(&self) -> bool {
        self.user_enabled
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// The film leg. `None` → render on the CPU. `Some(Err(e))` →
    /// attempted and failed: fall back to the CPU for THIS frame (the
    /// failure is counted here; 3 strikes open the breaker).
    pub fn try_film_leg(
        &mut self,
        img: &ImageF32,
        params: &FilmChainParams,
    ) -> Option<Result<ImageF32, String>> {
        if !self.user_enabled || self.session_disabled {
            return None;
        }
        let ctx = match melography_gpu::context::init(&self.policy) {
            Ok(c) => Some(c),
            Err(melography_gpu::GpuError::NoAdapter) => {
                // A state, not a failure: disable for the session with
                // the honest reason.
                self.session_disabled = true;
                self.reason = Some("no graphics adapter".into());
                None
            }
            Err(e) => {
                let msg = e.to_string();
                // A pinned fingerprint that is not present is a state,
                // not a flaky failure — hard-disable with the reason
                // (contract with melography-gpu: `GpuContext::new` words
                // this exact case "adapter not present: …").
                if msg.starts_with("adapter not present") {
                    self.session_disabled = true;
                    self.reason = Some(msg);
                    return None;
                }
                self.note_failure(msg.clone());
                return Some(Err(msg));
            }
        }?;
        match melography_film::gpu_chain::render_gpu(img, params, &ctx) {
            Ok(out) => {
                self.consecutive_failures = 0;
                Some(Ok(out))
            }
            Err(e) => {
                let msg = e.to_string();
                self.note_failure(msg.clone());
                Some(Err(msg))
            }
        }
    }

    fn note_failure(&mut self, msg: String) {
        self.consecutive_failures += 1;
        self.reason = Some(msg);
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            self.session_disabled = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(adapter: &str, enabled: bool) -> Settings {
        Settings {
            gpu_enabled: enabled,
            gpu_adapter: adapter.into(),
            ..Settings::default()
        }
    }

    #[test]
    fn disabled_by_settings_never_touches_the_gpu() {
        let mut rt = GpuRuntime::new();
        rt.sync_from_settings(&settings("auto", false));
        let img = ImageF32::alloc(2, 2, melography_core::Channels::Rgb, 0.25);
        let params = FilmChainParams::new(test_stock()).unwrap();
        assert!(
            rt.try_film_leg(&img, &params).is_none(),
            "user-disabled GPU must be a plain None (CPU path)"
        );
    }

    #[test]
    fn adapter_change_re_arms_the_breaker() {
        let mut rt = GpuRuntime::new();
        rt.sync_from_settings(&settings("auto", true));
        rt.note_failure("synthetic failure 1".into());
        rt.note_failure("synthetic failure 2".into());
        rt.note_failure("synthetic failure 3".into());
        assert!(
            rt.session_disabled(),
            "3 consecutive failures open the breaker"
        );
        // Switching adapters re-arms everything.
        rt.sync_from_settings(&settings("X|4098|Metal|Discrete", true));
        assert!(!rt.session_disabled());
        assert_eq!(rt.consecutive_failures, 0);
        assert!(rt.reason().is_none());
    }

    #[test]
    fn success_resets_the_failure_streak() {
        let mut rt = GpuRuntime::new();
        rt.note_failure("a".into());
        rt.note_failure("b".into());
        rt.consecutive_failures = 0; // a success did this
        rt.note_failure("c".into());
        assert!(
            !rt.session_disabled(),
            "1 failure after a success must not trip"
        );
    }

    #[test]
    fn tests_runtime_forces_cpu() {
        let rt = GpuRuntime::cpu_for_tests();
        assert!(!rt.user_enabled());
    }

    fn test_stock() -> melography_film::profile::StockProfile {
        melography_film::profile::testing::test_profile()
    }
}
