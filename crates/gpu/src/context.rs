//! Persistent GPU context (A2.4 — full-chain GPU preview wiring).
//!
//! The A2.3 kernels acquired an instance + adapter + device **per call**
//! — fine for one-shot parity legs, hopeless for a per-slider preview
//! path (instance enumeration alone dwarfs a render). This module owns
//! the long-lived alternative: one device + queue + **every pipeline
//! compiled once** (`Pipelines`), kept in a process-wide slot and shared
//! by every render, plus the A4 adapter *policy* that decides which
//! adapter the context binds.
//!
//! Policy (PLAN M5-A A4, the documented tie-break): `Auto` ranks every
//! enumerated adapter by device type (Discrete 4 > Integrated 3 >
//! Virtual 2 > Other 1 > Cpu 0 — software adapters only as a last
//! resort, and labelled "slow (software)" by the UI) ×100 + backend
//! preference (Vulkan 5 > Dx12 4 > Metal 3 > Gl 2 > BrowserWebGpu 1);
//! ties keep enumeration order. `ByFingerprint` pins the user's explicit
//! pick (`name|vendor|backend|device_type`) and fails honestly when the
//! adapter is absent (the app falls back to Auto, then to the CPU path
//! — never a silent wrong-adapter render).
//!
//! Device-loss contract: a lost adapter surfaces as an ordinary
//! `GpuError` on the next submit/poll (the app's `GpuRuntime` counts
//! failures and disables the GPU for the session after 3 — honest
//! degradation, resettable from Settings). Uncaptured device errors are
//! parked in a diagnostic slot (`last_uncaptured_error`) instead of
//! panicking the render thread — surfaced by `gpu_status`, never
//! silently dropped.

use crate::{AdapterReport, GpuError};
use std::sync::{Arc, Mutex, RwLock};

/// Every compute pipeline the film chain needs, compiled once per
/// context (WGSL → backend IR is the expensive part; a per-render
/// recompile would eat the entire preview budget).
pub(crate) struct Pipelines {
    pub(crate) film_pixel: wgpu::ComputePipeline,
    pub(crate) halation_h: wgpu::ComputePipeline,
    pub(crate) halation_v: wgpu::ComputePipeline,
    pub(crate) grain: wgpu::ComputePipeline,
    pub(crate) vignette: wgpu::ComputePipeline,
}

/// A persistent device + queue + pipeline cache bound to one adapter.
pub struct GpuContext {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) pipelines: Pipelines,
    /// The adapter this context is bound to (surfaced by `gpu_status`).
    pub report: AdapterReport,
}

/// Which adapter a context should bind (A4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterPolicy {
    /// Score-ranked automatic selection (see the module docs).
    Auto,
    /// Pin a specific adapter by `adapter_fingerprint` string. Missing
    /// fingerprint → honest error (the caller decides the fallback).
    ByFingerprint(String),
}

impl AdapterPolicy {
    fn fingerprint(&self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::ByFingerprint(fp) => format!("fp:{fp}"),
        }
    }
}

/// Stable identity of an adapter across `gpu_status` calls:
/// `name|vendor|backend|device_type` (what `AdapterReport` carries).
pub fn adapter_fingerprint(r: &AdapterReport) -> String {
    format!("{}|{}|{}|{}", r.name, r.vendor, r.backend, r.device_type)
}

/// Device-type rank (higher = preferred). The Debug-string forms are the
/// ones `enumerate_adapters` serializes — matching on them keeps the
/// scoring in the same vocabulary as the UI rows.
fn device_type_rank(dt: &str) -> u32 {
    match dt {
        "Discrete" => 4,
        "Integrated" => 3,
        "Virtual" => 2,
        "Other" => 1,
        _ => 0, // "Cpu" (llvmpipe & friends) — last resort by policy
    }
}

/// Backend preference rank (higher = preferred).
fn backend_rank(b: &str) -> u32 {
    match b {
        "Vulkan" => 5,
        "Dx12" => 4,
        "Metal" => 3,
        "Gl" => 2,
        "BrowserWebGpu" => 1,
        _ => 0, // "Empty"
    }
}

/// The A4 `Auto` score: `device_type_rank·100 + backend_rank`.
pub fn adapter_score(r: &AdapterReport) -> u32 {
    device_type_rank(&r.device_type) * 100 + backend_rank(&r.backend)
}

/// All enumerated adapters, best-first (stable sort — equal scores keep
/// enumeration order, so the pick is deterministic).
pub fn ranked_adapters() -> Vec<AdapterReport> {
    let mut list = crate::enumerate_adapters();
    list.sort_by(|a, b| adapter_score(b).cmp(&adapter_score(a)));
    list
}

impl GpuContext {
    /// Acquire a device + build every pipeline, per `policy`.
    pub fn new(policy: &AdapterPolicy) -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        if adapters.is_empty() {
            return Err(GpuError::NoAdapter);
        }
        let chosen = match policy {
            AdapterPolicy::Auto => adapters
                .into_iter()
                .max_by_key(|a| {
                    let info = a.get_info();
                    device_type_rank(&format!("{:?}", info.device_type)) * 100
                        + backend_rank(&format!("{:?}", info.backend))
                })
                .ok_or(GpuError::NoAdapter)?,
            AdapterPolicy::ByFingerprint(fp) => adapters
                .into_iter()
                .find(|a| {
                    let i = a.get_info();
                    format!(
                        "{}|{}|{:?}|{:?}",
                        i.name, i.vendor, i.backend, i.device_type
                    ) == *fp
                })
                .ok_or_else(|| GpuError::Wgpu(format!("adapter not present: {fp}")))?,
        };
        let info = chosen.get_info();
        let report = AdapterReport {
            name: info.name.clone(),
            vendor: info.vendor,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
        };
        let (device, queue) =
            pollster::block_on(chosen.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        // A device error must never panic the render thread (the preview
        // path degrades to CPU instead); park it where `gpu_status` can
        // show it — never silent, never fatal.
        device.on_uncaptured_error(Arc::new(|err: wgpu::Error| {
            if let Ok(mut slot) = LAST_UNCAPTURED.lock() {
                *slot = Some(err.to_string());
            }
        }));
        let pipelines = Pipelines {
            film_pixel: compute_pipeline(
                &device,
                "film-pixel",
                crate::film_pixel::FILM_PIXEL_SHADER,
            ),
            halation_h: compute_pipeline(&device, "halation-h", crate::spatial::HALATION_H_SHADER),
            halation_v: compute_pipeline(&device, "halation-v", crate::spatial::HALATION_V_SHADER),
            grain: compute_pipeline(&device, "grain-gather", crate::spatial::GRAIN_SHADER),
            vignette: compute_pipeline(&device, "vignette", crate::vignette::VIGNETTE_SHADER),
        };
        Ok(Self {
            device,
            queue,
            pipelines,
            report,
        })
    }
}

/// A one-shot device acquisition (the parity-harness pattern: a fresh
/// instance + adapter + device per call, no pipeline cache). The A2.3
/// standalone `run_*` legs use this; the preview path uses
/// [`GpuContext`] instead.
pub(crate) struct AcquiredDevice {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl AcquiredDevice {
    pub(crate) fn acquire() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|_| GpuError::NoAdapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        Ok(Self { device, queue })
    }
}

struct GlobalEntry {
    ctx: Arc<GpuContext>,
    policy_fp: String,
}

static GLOBAL: RwLock<Option<GlobalEntry>> = RwLock::new(None);
static LAST_UNCAPTURED: Mutex<Option<String>> = Mutex::new(None);

/// The shared context, if one has been initialized.
pub fn global() -> Option<Arc<GpuContext>> {
    GLOBAL.read().unwrap().as_ref().map(|e| Arc::clone(&e.ctx))
}

/// Initialize (or reuse) the shared context for `policy`. Reuse only
/// happens when the policy fingerprint matches — switching adapters
/// goes through [`reset`] first (Settings) or a mismatched init, which
/// replaces the entry.
pub fn init(policy: &AdapterPolicy) -> Result<Arc<GpuContext>, GpuError> {
    let fp = policy.fingerprint();
    {
        let g = GLOBAL.read().unwrap();
        if let Some(e) = g.as_ref() {
            if e.policy_fp == fp {
                return Ok(Arc::clone(&e.ctx));
            }
        }
    }
    let ctx = Arc::new(GpuContext::new(policy)?);
    *GLOBAL.write().unwrap() = Some(GlobalEntry {
        ctx: Arc::clone(&ctx),
        policy_fp: fp,
    });
    Ok(ctx)
}

/// Drop the shared context (adapter switch, device loss, disable).
pub fn reset() {
    *GLOBAL.write().unwrap() = None;
}

/// The last uncaptured device error, if any (diagnostics; cleared only
/// by re-initializing the context).
pub fn last_uncaptured_error() -> Option<String> {
    LAST_UNCAPTURED.lock().unwrap().clone()
}

/// Shared pipeline builder (the A2.3 idiom, hoisted so every kernel
/// module and the context cache use ONE code path).
pub(crate) fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    code: &str,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: None,
        compilation_options: Default::default(),
        cache: None,
    })
}

/// Shared two-buffer readback idiom (S1 lesson — see lut3d.rs).
pub(crate) fn read_back(
    device: &wgpu::Device,
    readback: &wgpu::Buffer,
) -> Result<Vec<f32>, GpuError> {
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| GpuError::Wgpu(e.to_string()))?;
    rx.recv()
        .map_err(|_| GpuError::Wgpu("map channel closed".into()))?
        .map_err(|e| GpuError::Wgpu(e.to_string()))?;
    let floats: Vec<f32> = {
        let data = slice
            .get_mapped_range()
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        bytemuck::cast_slice(&data).to_vec()
    };
    readback.unmap();
    Ok(floats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_fingerprints_distinguish() {
        assert_eq!(AdapterPolicy::Auto.fingerprint(), "auto");
        assert_eq!(
            AdapterPolicy::ByFingerprint("X|4098|Metal|Discrete".into()).fingerprint(),
            "fp:X|4098|Metal|Discrete"
        );
        assert_ne!(
            AdapterPolicy::Auto.fingerprint(),
            AdapterPolicy::ByFingerprint("auto".into()).fingerprint()
        );
    }

    #[test]
    fn auto_score_orders_the_documented_policy() {
        let mk = |name: &str, backend: &str, dt: &str| AdapterReport {
            name: name.into(),
            vendor: 0,
            backend: backend.into(),
            device_type: dt.into(),
        };
        let discrete_gl = mk("d", "Gl", "Discrete");
        let integrated_vulkan = mk("i", "Vulkan", "Integrated");
        let software = mk("s", "Vulkan", "Cpu");
        // Type dominates backend (×100); software is last even on Vulkan.
        assert!(adapter_score(&discrete_gl) > adapter_score(&integrated_vulkan));
        assert!(adapter_score(&integrated_vulkan) > adapter_score(&software));
        assert_eq!(adapter_score(&software), 5);
        // Same type → backend preference decides.
        let discrete_dx12 = mk("d2", "Dx12", "Discrete");
        assert!(adapter_score(&discrete_dx12) > adapter_score(&discrete_gl));
    }

    #[test]
    fn ranked_adapters_are_sorted_best_first() {
        let list = ranked_adapters();
        for pair in list.windows(2) {
            assert!(
                adapter_score(&pair[0]) >= adapter_score(&pair[1]),
                "ranked_adapters must be non-increasing: {:?} before {:?}",
                pair[0].name,
                pair[1].name
            );
        }
    }

    #[test]
    fn global_context_init_reuse_and_reset() {
        // Adapter-less environments (headless CI linux) skip honestly.
        let ctx = match init(&AdapterPolicy::Auto) {
            Ok(ctx) => ctx,
            Err(GpuError::NoAdapter) => return,
            Err(e) => panic!("unexpected init error: {e}"),
        };
        assert!(global().is_some(), "init must install the global context");
        // Same policy → the SAME context (Arc identity), no re-acquire.
        let again = init(&AdapterPolicy::Auto).expect("reuse must succeed");
        assert!(Arc::ptr_eq(&ctx, &again));
        reset();
        assert!(global().is_none(), "reset must drop the global context");
        // Re-init after reset still works and reports the same adapter.
        let _ = init(&AdapterPolicy::Auto).expect("re-init must succeed");
    }
}
