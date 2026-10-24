//! GPU rendering for MELOGRAPHY: WGSL kernels mirroring the CPU oracle, the
//! render context, and the preview transport (ADR-0001). **M0 scope:** the
//! adapter probe and the staging-readback benchmark behind the `wgpu`
//! feature — the inputs Spike S1 needs to measure the preview-transport
//! budget on real hardware.
//!
//! The full kernel set + parity harness (`COLOR_PIPELINE` §8 GPU-vs-CPU
//! tolerances) is M1/M2 work. Without the `wgpu` feature this crate exposes
//! only the shared types (honest no-op, not a fake pass).

pub mod film_pixel_types;

#[cfg(feature = "wgpu")]
pub mod film_pixel;

#[cfg(feature = "wgpu")]
pub mod lut3d;

#[cfg(feature = "wgpu")]
pub mod readback;

#[derive(Debug, thiserror::Error)]
#[error("GPU error: {0}")]
pub enum GpuError {
    #[error("no suitable graphics adapter available")]
    NoAdapter,
    #[error("wgpu error: {0}")]
    Wgpu(String),
    #[cfg(feature = "wgpu")]
    #[error("adapter lost during operation")]
    AdapterLost,
}

/// Report of one enumerated graphics adapter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterReport {
    pub name: String,
    pub vendor: u32,
    pub backend: String,
    pub device_type: String,
}

#[cfg(feature = "wgpu")]
#[must_use]
pub fn enumerate_adapters() -> Vec<AdapterReport> {
    let desc = wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    let instance = wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    adapters
        .into_iter()
        .map(|a| {
            let info = a.get_info();
            AdapterReport {
                name: info.name,
                vendor: info.vendor,
                backend: format!("{:?}", info.backend),
                device_type: format!("{:?}", info.device_type),
            }
        })
        .collect()
}

#[cfg(not(feature = "wgpu"))]
#[must_use]
pub fn enumerate_adapters() -> Vec<AdapterReport> {
    Vec::new() // compiled without wgpu: honest empty report
}

#[cfg(test)]
mod tests {
    #[test]
    fn adapter_probe_compiles_and_reports_honestly() {
        let adapters = crate::enumerate_adapters();
        #[cfg(feature = "wgpu")]
        {
            // On machines without a GPU this is legitimately empty; the
            // spike prints the report and skips benchmarks (never fabricates).
            for a in &adapters {
                assert!(!a.name.is_empty());
            }
        }
        #[cfg(not(feature = "wgpu"))]
        assert!(adapters.is_empty());
    }
}
