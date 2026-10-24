//! WGSL tetrahedral 3D-LUT kernel + CPU↔GPU parity harness (luts.md §2/§5,
//! issue #7; COLOR_PIPELINE §8 "GPU when hosted runners provide it").
//!
//! The kernel mirrors the CPU implementation in `melography-lut::tetra`
//! *exactly* (same six-case increment-sum formula, same clamp-first policy)
//! so the parity gate (`max |Δ| ≤ 1e-6` on identical inputs) is meaningful:
//! a mismatch means one of the two is wrong, not merely "different".
//!
//! Layering: this module takes raw table/params (no `melography-lut`
//! dependency) — the parity *test* owns the CPU reference and the verdict.
//! Adapter-less environments get an honest `NoAdapter` error; CI runners
//! with real adapters (macOS Metal, Windows WARP) run the gate for real
//! and print the measured worst-case delta into the logs.

use crate::{AdapterReport, GpuError};
use wgpu::util::DeviceExt;

/// WGSL mirror of `melography_lut::tetra::sample3d`.
///
/// Bindings: 0 = uniform Params, 1 = table (read), 2 = inputs (read),
/// 3 = outputs (read_write). One invocation per sample.
const LUT3D_SHADER: &str = r#"
struct Params {
    n: u32,
    _pad: u32,
    dmin: vec3<f32>,
    dmax: vec3<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read>       table: array<f32>;
@group(0) @binding(2) var<storage, read>       inputs: array<f32>;
@group(0) @binding(3) var<storage, read_write> outputs: array<f32>;

fn corner(base: u32) -> vec3<f32> {
    return vec3<f32>(table[base], table[base + 1u], table[base + 2u]);
}

// Increment-sum tetrahedron path — identical arithmetic to the CPU oracle:
// a + u*(b-a) + v*(c-b) + w*(d-c)
fn tetra_path(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>,
              u: f32, v: f32, w: f32) -> vec3<f32> {
    return a + u * (b - a) + v * (c - b) + w * (d - c);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let s = gid.x;
    if (s * 3u + 2u >= arrayLength(&inputs)) { return; }

    let n = params.n;
    let last = f32(n - 1u);

    // Clamp to the declared domain first (CPU policy), per channel.
    var v: vec3<f32>;
    for (var c = 0u; c < 3u; c = c + 1u) {
        let x = inputs[s * 3u + c];
        v[c] = clamp(x, params.dmin[c], params.dmax[c]);
    }

    // Lattice coordinates: [dmin,dmax] -> [0, N-1].
    let g = (v - params.dmin) / (params.dmax - params.dmin) * last;
    let fl = floor(g);
    let i0 = min(u32(fl.x), n - 2u);
    let j0 = min(u32(fl.y), n - 2u);
    let k0 = min(u32(fl.z), n - 2u);
    let f = g - vec3<f32>(f32(i0), f32(j0), f32(k0));
    let fx = f.x; let fy = f.y; let fz = f.z;

    let base = (i0 + j0 * n + k0 * n * n) * 3u;
    let c000 = corner(base);
    let c100 = corner(base + 3u);
    let c010 = corner(base + 3u * n);
    let c110 = corner(base + 3u * n + 3u);
    let c001 = corner(base + 3u * n * n);
    let c101 = corner(base + 3u * n * n + 3u);
    let c011 = corner(base + 3u * n * n + 3u * n);
    let c111 = corner(base + 3u * n * n + 3u * n + 3u);

    var out: vec3<f32>;
    if (fx >= fy && fy >= fz) {
        out = tetra_path(c000, c100, c110, c111, fx, fy, fz);
    } else if (fx >= fz && fz >= fy) {
        out = tetra_path(c000, c100, c101, c111, fx, fz, fy);
    } else if (fy >= fx && fx >= fz) {
        out = tetra_path(c000, c010, c110, c111, fy, fx, fz);
    } else if (fy >= fz && fz >= fx) {
        out = tetra_path(c000, c010, c011, c111, fy, fz, fx);
    } else if (fz >= fx && fx >= fy) {
        out = tetra_path(c000, c001, c101, c111, fz, fx, fy);
    } else {
        out = tetra_path(c000, c001, c011, c111, fz, fy, fx);
    }

    outputs[s * 3u]      = out.x;
    outputs[s * 3u + 1u] = out.y;
    outputs[s * 3u + 2u] = out.z;
}
"#;

/// Result of one GPU LUT evaluation run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Lut3dRunReport {
    pub adapter: String,
    pub samples: usize,
    /// Wall time of dispatch+readback for the whole batch, milliseconds.
    pub ms: f64,
}

/// Evaluate a 3D LUT on the GPU for `samples` (declared-domain RGB triples).
/// Returns outputs in input order. `table` is the red-fastest N³×3 float
/// array; `size` = N. Errors with [`GpuError::NoAdapter`] when no adapter
/// exists (callers report an honest skip — never a fake pass).
///
/// The two-buffer readback idiom is mandatory (S1 lesson, commit `1668e76`):
/// compute writes a `STORAGE|COPY_SRC` buffer, the encoder copies into a
/// `COPY_DST|MAP_READ` buffer, only the latter is mapped.
pub fn run_lut3d(
    size: u32,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    table: &[f32],
    samples: &[[f32; 3]],
) -> Result<(Vec<[f32; 3]>, Lut3dRunReport), GpuError> {
    if samples.is_empty() {
        return Err(GpuError::Wgpu("no samples".into()));
    }
    let expected = size as usize * size as usize * size as usize * 3;
    if table.len() != expected {
        return Err(GpuError::Wgpu(format!(
            "table length {} != N³·3 for N={size}",
            table.len()
        )));
    }

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
    let adapter_info: AdapterReport = {
        let info = adapter.get_info();
        AdapterReport {
            name: info.name,
            vendor: info.vendor,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
        }
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;

    let start = std::time::Instant::now();

    // Uniform params: WGSL uniform layout — n@0, pad@4, dmin@16, dmax@32.
    let mut params_bytes = [0u8; 48];
    params_bytes[0..4].copy_from_slice(&size.to_le_bytes());
    params_bytes[16..28].copy_from_slice(bytemuck::cast_slice(&domain_min));
    params_bytes[32..44].copy_from_slice(bytemuck::cast_slice(&domain_max));
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lut3d-params"),
        contents: &params_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let table_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lut3d-table"),
        contents: bytemuck::cast_slice(table),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let flat_in: Vec<f32> = samples.iter().flat_map(|s| s.iter().copied()).collect();
    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lut3d-in"),
        contents: bytemuck::cast_slice(&flat_in),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_storage = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lut3d-out-storage"),
        size: (flat_in.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lut3d-out-readback"),
        size: (flat_in.len() * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lut3d"),
        source: wgpu::ShaderSource::Wgsl(LUT3D_SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("lut3d-pipeline"),
        layout: None,
        module: &module,
        entry_point: None,
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lut3d-binds"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: table_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: input_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_storage.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("lut3d-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("lut3d-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let wg = (samples.len() as u32).div_ceil(64);
        pass.dispatch_workgroups(wg, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_storage, 0, &out_readback, 0, out_readback.size());
    queue.submit(Some(encoder.finish()));

    let slice = out_readback.slice(..);
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
    out_readback.unmap();

    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let out: Vec<[f32; 3]> = floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    assert_eq!(out.len(), samples.len());
    Ok((
        out,
        Lut3dRunReport {
            adapter: adapter_info.name,
            samples: samples.len(),
            ms,
        },
    ))
}
