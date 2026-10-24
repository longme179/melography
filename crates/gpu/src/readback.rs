//! Staging-readback benchmark (Spike S1 building block): a compute pass
//! fills a texture-sized buffer, then maps it back to CPU memory.
//! Measures the GPU→CPU leg of the preview-transport budget on whatever
//! adapter is actually present.

use crate::{AdapterReport, GpuError};
use std::time::Instant;
use wgpu::util::DeviceExt;

/// WGSL: each invocation writes a deterministic RGBA8 pattern.
const FILL_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&out)) { return; }
    // deterministic pattern: x, 255-x, (x*7)%256, 255
    let x = idx % 256u;
    let px = (x << 24u) | ((255u - x) << 16u) | ((x * 7u) % 256u << 8u) | 255u;
    out[idx] = px;
}
"#;

/// One readback measurement (single dispatch + map).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ReadbackResult {
    pub width: u32,
    pub height: u32,
    /// Fill + readback wall time, milliseconds.
    pub ms: f64,
    pub bytes: usize,
    pub gb_per_s: f64,
}

/// Run the readback benchmark `iterations` times on the best adapter
/// available; returns `Err(GpuError::NoAdapter)` when none exists (honest
/// skip for the spike to report).
pub fn readback_bench(
    width: u32,
    height: u32,
    iterations: u32,
) -> Result<Vec<ReadbackResult>, GpuError> {
    let desc = wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    let instance = wgpu::Instance::new(desc);
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

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("s1-fill"),
        source: wgpu::ShaderSource::Wgsl(FILL_SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("s1-fill-pipeline"),
        layout: None,
        module: &module,
        entry_point: None,
        compilation_options: Default::default(),
        cache: None,
    });

    let elements = (width as usize) * (height as usize); // one u32 per RGBA8 pixel
    let mut results = Vec::with_capacity(iterations as usize);

    for _ in 0..iterations {
        // WebGPU readback idiom (validated on CI macos/windows adapters):
        // compute writes a STORAGE buffer; MAP_READ may only combine with
        // COPY_DST — so the bytes travel storage --copy--> readback buffer,
        // and only the readback buffer is mapped. A single buffer with
        // STORAGE|COPY_SRC|MAP_READ is rejected by wgpu validation (the
        // first CI run caught exactly that; no-adapter environments never
        // exercise this path).
        let storage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("s1-storage"),
            contents: &vec![0u8; elements * 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("s1-readback"),
            size: (elements * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("s1-bg"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: storage.as_entire_binding(),
            }],
        });

        let start = Instant::now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("s1-enc"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("s1-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(elements.div_ceil(64) as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, (elements * 4) as u64);
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        receiver
            .recv()
            .map_err(|_| GpuError::Wgpu("map callback lost".into()))?
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        {
            let data = slice
                .get_mapped_range()
                .map_err(|e| GpuError::Wgpu(e.to_string()))?;
            // touch the data so the read is not optimized away; BufferView
            // derefs to &[u8] (wgpu 30)
            let checksum = data
                .iter()
                .fold(0u64, |acc, b| acc.wrapping_add(u64::from(*b)));
            assert_ne!(checksum, u64::MAX, "mapped range must exist");
        }
        readback.unmap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        results.push(ReadbackResult {
            width,
            height,
            ms,
            bytes: elements * 4,
            gb_per_s: (elements * 4) as f64 / 1024.0 / 1024.0 / 1024.0 / (ms / 1000.0),
        });
    }
    Ok(results)
}

/// The adapter this bench would use (for the report).
#[must_use]
pub fn best_adapter() -> Option<AdapterReport> {
    crate::enumerate_adapters().into_iter().next()
}
