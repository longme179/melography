//! WGSL film pixel-stage kernel + CPU↔GPU parity harness (FILM_MODEL
//! §C1/C3–F per-pixel scope; issue #10; COLOR_PIPELINE §8).
//!
//! Scope of this kernel — the **per-pixel** portion of the chain:
//! excitation (M_exc, EV/push/anchor scale) → H&D response (spline + γ
//! pivot + crosstalk + bleach bypass) → print/scan inversion → output
//! matrix. The *spatial* stages (halation §C2, veiling, grain §G) stay on
//! the CPU path in v1 — their GPU kernels are the documented follow-up
//! (grain's u32 lattice was designed WGSL-portable from day one; the
//! quality report tracks this honestly).
//!
//! Parity contract: the kernel mirrors the CPU math *exactly* (same
//! formulas, f32; tangents precomputed on the CPU in f64), so the gate
//! `max |Δ| ≤ 1e-3` (linear) against the CPU chain with spatial stages
//! disabled is meaningful — a mismatch means one of the two is wrong.
//!
//! Layering mirrors `lut3d`: this module takes plain data (no
//! `melography-film` dependency); the parity *test* owns the CPU
//! reference and the verdict. Adapter-less environments get an honest
//! `NoAdapter` error.

use crate::film_pixel_types::{pack, FilmPixelGpuParams};
use crate::{AdapterReport, GpuError};
use wgpu::util::DeviceExt;

const FILM_PIXEL_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read>       params: array<f32>;
@group(0) @binding(1) var<storage, read>       inputs: array<f32>;
@group(0) @binding(2) var<storage, read_write> outputs: array<f32>;

fn flags() -> u32 { return u32(params[0]); }
fn nHd() -> u32 { return u32(params[1]); }
fn nPp() -> u32 { return u32(params[2]); }
fn isMono() -> bool { return (flags() & 1u) != 0u; }
fn isReversal() -> bool { return (flags() & 2u) != 0u; }
fn path() -> u32 { return (flags() >> 2u) & 3u; }

fn hdBase(layer: u32) -> u32 {
    return 59u + layer * nHd() * 3u;
}
fn ppBase(layer: u32) -> u32 {
    return 59u + 3u * nHd() * 3u + layer * nPp() * 3u;
}

// Monotone Hermite eval with flat extrapolation — the CPU formula in f32.
fn splineEval(base: u32, n: u32, x: f32) -> f32 {
    let x0 = params[base];
    if (x <= x0) { return params[base + 1u]; }
    let last = base + (n - 1u) * 3u;
    if (x >= params[last]) { return params[last + 1u]; }
    // Binary search for the segment.
    var lo = 0u; var hi = n - 1u;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) / 2u;
        let mx = params[base + mid * 3u];
        if (mx <= x) { lo = mid; } else { hi = mid; }
    }
    let i0 = base + lo * 3u;
    let i1 = i0 + 3u;
    let xa = params[i0]; let ya = params[i0 + 1u]; let ma = params[i0 + 2u];
    let xb = params[i1]; let yb = params[i1 + 1u]; let mb = params[i1 + 2u];
    let h = xb - xa;
    let t = (x - xa) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    return (2.0*t3 - 3.0*t2 + 1.0) * ya
         + (t3 - 2.0*t2 + t) * h * ma
         + (-2.0*t3 + 3.0*t2) * yb
         + (t3 - t2) * h * mb;
}

fn mat3Apply(m: u32, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        params[m] * v.x + params[m+1u] * v.y + params[m+2u] * v.z,
        params[m+3u] * v.x + params[m+4u] * v.y + params[m+5u] * v.z,
        params[m+6u] * v.x + params[m+7u] * v.y + params[m+8u] * v.z
    );
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let s = gid.x;
    if (s * 3u + 2u >= arrayLength(&inputs)) { return; }

    var e = vec3<f32>(inputs[s*3u], inputs[s*3u+1u], inputs[s*3u+2u]);

    // §A5 pre-desaturation [ART].
    let pre = params[5];
    if (pre > 0.0) {
        let lum = 0.2126*e.x + 0.7152*e.y + 0.0722*e.z;
        e = e * (1.0 - pre) + vec3<f32>(lum) * pre;
    }

    // §C1/C3 excitation with total scale.
    let x = mat3Apply(17u, e) * params[4];

    // §D response per active layer.
    let layers = select(3u, 1u, isMono());
    var d = vec3<f32>(0.0);
    var f_mid = vec3<f32>(0.0);
    var f_lo = vec3<f32>(0.0);
    var f_hi = vec3<f32>(0.0);
    for (var li = 0u; li < layers; li = li + 1u) {
        let base = hdBase(li);
        let n = nHd();
        let log_e = log10(max(x[li], params[3]));
        let f = splineEval(base, n, log_e);
        let xm0 = params[base];
        let xm1 = params[base + (n - 1u) * 3u];
        let vm = splineEval(base, n, 0.5 * (xm0 + xm1));
        let v0 = splineEval(base, n, xm0);
        let v1 = splineEval(base, n, xm1);
        f_mid[li] = vm;
        f_lo[li] = min(v0, v1);
        f_hi[li] = max(v0, v1);
        var fg = vm + params[6u + li] * (f - vm);
        fg = clamp(fg, f_lo[li], f_hi[li]);
        d[li] = params[35u + li] + fg;   // d_min + response-above-fog
    }
    if (isMono()) { d = vec3<f32>(d.x); }

    // §D crosstalk: D += C·D_off (scale from §E coupling).
    let ct = 26u;
    let d_in = d;
    for (var i = 0u; i < layers; i = i + 1u) {
        var off = 0.0;
        for (var j = 0u; j < layers; j = j + 1u) {
            if (i != j) {
                off = off + params[ct + i*3u + j] * d_in[j];
            }
        }
        d[i] = d[i] + params[9] * off;
    }

    // §E bleach bypass [ART].
    let bb = params[10];
    if (bb > 0.0 && !isMono()) {
        let lum = 0.25*d.x + 0.5*d.y + 0.25*d.z;
        let silver = clamp((lum - 0.15) * 1.9, 0.0, 2.3);
        d = mix(d, vec3<f32>(silver), bb);
    }
    d = max(d, vec3<f32>(0.0));

    // §F inversion.
    var lin = vec3<f32>(0.0);
    if (isReversal()) {
        // Direct projection, normalized at the white anchor.
        for (var li = 0u; li < layers; li = li + 1u) {
            let d_eff = d[li] - params[38u + li];
            lin[li] = pow(10.0, -d_eff) * pow(10.0, params[12]);
        }
    } else {
        let p = path();
        for (var li = 0u; li < layers; li = li + 1u) {
            let d_eff = d[li] - params[38u + li];
            let t = pow(10.0, -d_eff);
            if (p == 0u) {
                // Paper path.
                let x_p = params[11] * t;
                let d_p = splineEval(ppBase(li), nPp(), log10(max(x_p, 1e-12)));
                lin[li] = pow(10.0, params[12] - d_p);
            } else if (p == 1u) {
                // Scanner path: log window + gamma + matrix + positive map.
                let x_p = params[11] * t + params[16];
                let y = clamp((log10(max(x_p, 1e-12)) - params[13])
                              / (params[14] - params[13]), 0.0, 1.0);
                let z = pow(y, 1.0 / params[15]);
                let sm = mat3Apply(50u, vec3<f32>(z));
                lin[li] = pow(10.0, params[12] - 2.2 * (1.0 - clamp(sm[li], 0.0, 1.0)));
            } else {
                // Direct scan [ART].
                lin[li] = pow(10.0, params[12] - d_eff);
            }
        }
    }
    lin = clamp(lin, vec3<f32>(0.0), vec3<f32>(1.0));
    var out = mat3Apply(41u, lin);
    if (isMono()) { out = vec3<f32>(out.x); }
    outputs[s*3u]   = out.x;
    outputs[s*3u+1u] = out.y;
    outputs[s*3u+2u] = out.z;
}
"#;

/// Adapter probe (parity tests print the adapter for the evidence log).
pub fn first_adapter_report() -> Option<AdapterReport> {
    crate::readback::best_adapter()
}

/// Run the film pixel kernel on `inputs` (interleaved RGB f32).
/// Returns the outputs (same layout). No adapter → honest `NoAdapter`.
pub fn run_film_pixel(params: &FilmPixelGpuParams, inputs: &[f32]) -> Result<Vec<f32>, GpuError> {
    let packed = pack(params).map_err(GpuError::Wgpu)?;
    let n = inputs.len() / 3;
    let out_len = n * 3;
    if out_len == 0 {
        return Ok(Vec::new());
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
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;

    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("film-params"),
        contents: bytemuck::cast_slice(&packed),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let inputs_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("film-inputs"),
        contents: bytemuck::cast_slice(inputs),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_storage = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("film-outputs"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("film-out-readback"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("film-pixel"),
        source: wgpu::ShaderSource::Wgsl(FILM_PIXEL_SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("film-pixel-pipeline"),
        layout: None,
        module: &module,
        entry_point: None,
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("film-pixel-bg"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: inputs_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out_storage.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("film-pixel-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("film-pixel-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_storage, 0, &out_readback, 0, out_readback.size());
    queue.submit(Some(encoder.finish()));

    // Two-buffer readback idiom (S1 lesson — see lut3d.rs).
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
    Ok(floats)
}
