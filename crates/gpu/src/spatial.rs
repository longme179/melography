//! WGSL spatial kernels (A2.3 — grain gather + halation), the
//! `film_pixel` parity-harness pattern (COLOR_PIPELINE §8, PLAN M5-A
//! A2.3).
//!
//! Scope — the two **spatial** stages of the film chain that the M2
//! pixel kernel deliberately left on the CPU:
//! * **grain gather** (FILM_MODEL §G, the FULL model — not the A2.2
//!   fast approximation): per pixel, k×k pixel-area supersampled gather
//!   over the hashed lattice cells covering each sub-sample. The u32
//!   hash stream (`hash.rs`) uses wrapping u32 arithmetic only, so it
//!   is bit-portable: presence decisions (via the packed u32 threshold)
//!   and jitter uniforms (24-bit, exactly representable in f32) match
//!   the CPU bit-for-bit; only the transcendental evaluation
//!   (Box–Muller, bump weights, σ(D) interpolation) runs in f32 —
//!   inside the parity tolerance, never claimed bit-exact. Boundary
//!   effects are benign by construction: the compact bump profile is
//!   C¹ at `t = 1` (an inclusion flip contributes weight ≈ 0) and the
//!   gather reach exceeds every possible radius by a full cell.
//! * **halation** (§C2): separable h+v convolution, per-channel unit-DC
//!   taps and gains, zero padding at the frame edge — the CPU math in
//!   f32.
//!
//! Layering mirrors `film_pixel`/`lut3d`: this module takes plain data
//! (no `melography-film` dependency); the parity *tests* own the CPU
//! reference and the verdict. Adapter-less environments get an honest
//! `NoAdapter` error. A fresh instance/adapter/device per call (the
//! harness pattern — the app-wiring step owns the persistent context
//! and the preview-budget measurement).

use crate::spatial_types::{pack_grain, pack_halation, GrainGpuParams, HalationGpuParams};
use crate::GpuError;
use wgpu::util::DeviceExt;

const GRAIN_SHADER: &str = r#"
// Grain gather — WGSL mirror of the CPU full model (FILM_MODEL §G).
// One invocation per output pixel: pixel-area supersampled gather over
// the hashed lattice cells whose bumps cover each sub-sample.
@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

fn u32At(i: u32) -> u32 { return bitcast<u32>(params[i]); }
fn flags() -> u32 { return u32At(20u); }
fn grainEnabled() -> bool { return (flags() & 1u) != 0u; }
fn isMono() -> bool { return (flags() & 2u) != 0u; }
fn presenceAlways() -> bool { return (flags() & 4u) != 0u; }
fn imgW() -> u32 { return u32At(21u); }
fn imgH() -> u32 { return u32At(22u); }
fn kSup() -> u32 { return u32At(23u); }
fn curveN() -> u32 { return u32At(24u); }

const CURVE_BASE: u32 = 25u;
const TAU: f32 = 6.283185307179586;

// Splitmix32 finalizer + hash4 — bit-portable with the CPU hash.rs
// (wrapping u32 arithmetic only; verified against the CPU stream in a
// Python cross-check, scripts/a23_grain_wgsl_check.py).
fn mix32(z0: u32) -> u32 {
    var z = z0;
    z = z + 0x9E3779B9u;
    z = (z ^ (z >> 16u)) * 0x21F0AAADu;
    z = (z ^ (z >> 15u)) * 0x735A2D97u;
    return z ^ (z >> 15u);
}

fn hash4(a: u32, b: u32, c: u32, d: u32) -> u32 {
    var z = (a * 0x85EBCA6Bu) ^ (b * 0xC2B2AE35u) ^ (c * 0x27D4EB2Fu) ^ (d * 0x165667B1u);
    z = mix32(z);
    return mix32(z ^ 0x9E3779B9u);
}

struct Rng {
    state: u32,
}

fn rngNextU32(r: ptr<function, Rng>) -> u32 {
    (*r).state = (*r).state + 0x9E3779B9u;
    return mix32((*r).state);
}

// f32(v >> 8) is exact (24-bit mantissa); × 2^-24 is exact — the draw
// is bit-identical to the CPU's f64 uniform.
fn rngNextF(r: ptr<function, Rng>) -> f32 {
    return f32(rngNextU32(r) >> 8u) * (1.0 / 16777216.0);
}

// Box–Muller pair, both halves (the CPU normal_pair).
fn rngNormalPair(r: ptr<function, Rng>) -> vec2<f32> {
    let u1 = max(rngNextF(r), 2.0e-9);
    let u2 = rngNextF(r);
    let rad = sqrt(-2.0 * log(u1));
    let ang = TAU * u2;
    return vec2<f32>(rad * cos(ang), rad * sin(ang));
}

fn curveX(i: u32) -> f32 { return params[CURVE_BASE + i * 2u]; }
fn curveY(i: u32) -> f32 { return params[CURVE_BASE + i * 2u + 1u]; }

// σ(D): piecewise-linear with flat extrapolation (the CPU sigma_at).
fn sigmaAt(d: f32) -> f32 {
    let n = curveN();
    if (n == 0u) { return 0.0; }
    if (d <= curveX(0u)) { return curveY(0u); }
    let last = n - 1u;
    if (d >= curveX(last)) { return curveY(last); }
    for (var i = 0u; i < last; i = i + 1u) {
        if (d < curveX(i + 1u)) {
            let t = (d - curveX(i)) / (curveX(i + 1u) - curveX(i));
            return curveY(i) * (1.0 - t) + curveY(i + 1u) * t;
        }
    }
    return curveY(last);
}

// Bilinear density read with Plane::bilinear semantics: zero outside
// the corner-coordinate box [0, w-1] × [0, h-1], edge-clamped taps.
fn srcBilinear(fx: f32, fy: f32) -> vec3<f32> {
    let w = imgW();
    let h = imgH();
    if (!(fx >= 0.0 && fy >= 0.0 && fx <= f32(w) - 1.0 && fy <= f32(h) - 1.0)) {
        return vec3<f32>(0.0);
    }
    let x0 = u32(floor(fx));
    let y0 = u32(floor(fy));
    let x1 = min(x0 + 1u, w - 1u);
    let y1 = min(y0 + 1u, h - 1u);
    let tx = fx - f32(x0);
    let ty = fy - f32(y0);
    let i00 = (y0 * w + x0) * 3u;
    let i10 = (y0 * w + x1) * 3u;
    let i01 = (y1 * w + x0) * 3u;
    let i11 = (y1 * w + x1) * 3u;
    let p00 = vec3<f32>(src[i00], src[i00 + 1u], src[i00 + 2u]);
    let p10 = vec3<f32>(src[i10], src[i10 + 1u], src[i10 + 2u]);
    let p01 = vec3<f32>(src[i01], src[i01 + 1u], src[i01 + 2u]);
    let p11 = vec3<f32>(src[i11], src[i11 + 1u], src[i11 + 2u]);
    let top = p00 * (1.0 - tx) + p10 * tx;
    let bot = p01 * (1.0 - tx) + p11 * tx;
    return top * (1.0 - ty) + bot * ty;
}

// Luminance density at a lattice cell centre, projected to image
// pixels (the full-image pass has y_origin = 0 and plane height =
// image height, so the CPU's absolute/local row split collapses).
fn cellDensityLum(cx: f32, cy: f32) -> f32 {
    let mmpp = params[0];
    let w = f32(imgW());
    let h = f32(imgH());
    let fx = (cx - 18.0) / mmpp + w * 0.5;
    let fy = (cy - 12.0) / mmpp + h * 0.5;
    if (fy < 0.0 || fy > h - 1.0) { return 0.0; }
    let d = srcBilinear(fx, fy);
    return 0.25 * d.x + 0.5 * d.y + 0.25 * d.z;
}

// Compact radial bump ρ(t) = (1 - t²)², C¹ at t = 1.
fn bump(t: f32) -> f32 {
    if (t < 1.0) {
        let u = 1.0 - t * t;
        return u * u;
    }
    return 0.0;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = imgW() * imgH();
    let px = gid.x;
    if (px >= n) { return; }
    let w = imgW();
    let x = px % w;
    let y = px / w;
    let i = px * 3u;
    if (!grainEnabled()) {
        dst[i] = src[i];
        dst[i + 1u] = src[i + 1u];
        dst[i + 2u] = src[i + 2u];
        return;
    }

    let mmpp = params[0];
    let s = params[1];
    let cell_mm = params[2];
    let reach = params[5] + cell_mm;
    let uc = 18.0 + (f32(x) + 0.5 - f32(w) * 0.5) * mmpp;
    let vc = 12.0 + (f32(y) + 0.5 - f32(imgH()) * 0.5) * mmpp;
    let k = kSup();
    let sub = mmpp / f32(k);

    // One accumulator per pixel across all k² sub-samples (the CPU
    // gather loop order: samples outer, cells inner, fixed order).
    var acc = vec3<f32>(0.0);
    for (var sj = 0u; sj < k; sj = sj + 1u) {
        for (var si = 0u; si < k; si = si + 1u) {
            let u = uc + (f32(si) + 0.5) * sub - 0.5 * mmpp;
            let v = vc + (f32(sj) + 0.5) * sub - 0.5 * mmpp;
            let ix_lo = i32(floor((u - reach) * s));
            let ix_hi = i32(floor((u + reach) * s));
            let iy_lo = i32(floor((v - reach) * s));
            let iy_hi = i32(floor((v + reach) * s));
            for (var iy = iy_lo; iy <= iy_hi; iy = iy + 1) {
                for (var ix = ix_lo; ix <= ix_hi; ix = ix + 1) {
                    // σ(D) at the CELL centre (not the jittered grain
                    // position) — the CPU cache-build contract.
                    let sigma = sigmaAt(cellDensityLum((f32(ix) + 0.5) * cell_mm,
                                                       (f32(iy) + 0.5) * cell_mm));
                    if (sigma <= 0.0) { continue; }

                    var r = Rng(hash4(bitcast<u32>(ix), bitcast<u32>(iy), u32At(18u), 0x6A11u));
                    let jit_u = rngNextF(&r);
                    let jit_v = rngNextF(&r);
                    let hd = rngNextU32(&r);
                    let present = presenceAlways() || (hd < u32At(19u));
                    if (!present) { continue; }

                    // Draw order is the CPU contract: jitter ×2,
                    // presence, radius pair, (n0,n1) pair, (n2,_) pair.
                    let p0 = rngNormalPair(&r);
                    let radius = max(params[3] + params[4] * p0.x, 1.0e-6);
                    let p1 = rngNormalPair(&r);
                    let p2 = rngNormalPair(&r);

                    let cx = (f32(ix) + jit_u) * cell_mm;
                    let cy = (f32(iy) + jit_v) * cell_mm;
                    let g = vec3<f32>(p1.x, p1.y, p2.x);
                    // amp[c] = a_norm[c]·σ·Σ_j chol[3c+j]·g[j]; mono
                    // uses chol[0]·g[0] only (rho ignored, as the CPU).
                    var amp0 = 0.0;
                    var amp1 = 0.0;
                    var amp2 = 0.0;
                    if (isMono()) {
                        amp0 = params[15] * sigma * (params[6] * g.x);
                        amp1 = amp0;
                        amp2 = amp0;
                    } else {
                        amp0 = params[15] * sigma * (params[6] * g.x + params[7] * g.y + params[8] * g.z);
                        amp1 = params[16] * sigma * (params[9] * g.x + params[10] * g.y + params[11] * g.z);
                        amp2 = params[17] * sigma * (params[12] * g.x + params[13] * g.y + params[14] * g.z);
                    }

                    let du = u - cx;
                    let dv = v - cy;
                    let d2 = du * du + dv * dv;
                    if (d2 >= radius * radius) { continue; }
                    let t = sqrt(d2) / radius;
                    let wgt = bump(t);
                    acc = acc + wgt * vec3<f32>(amp0, amp1, amp2);
                }
            }
        }
    }

    let inv = 1.0 / f32(k * k);
    dst[i] = max(src[i] + acc.x * inv, 0.0);
    dst[i + 1u] = max(src[i + 1u] + acc.y * inv, 0.0);
    dst[i + 2u] = max(src[i + 2u] + acc.z * inv, 0.0);
}
"#;

const HALATION_H_SHADER: &str = r#"
// Halation §C2, horizontal pass: out[c][y][x] = Σ_i taps_c[i]·src[c][y][x+i−r],
// zero padding beyond the frame (tile-safe, the CPU contract). Only
// active channels (radius > 0) are written; the vertical pass reads
// exactly those.
@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> tmp: array<f32>;

fn u32At(i: u32) -> u32 { return bitcast<u32>(params[i]); }
fn imgW() -> u32 { return u32At(1u); }
fn imgH() -> u32 { return u32At(2u); }
fn chanRadius(c: u32) -> u32 { return u32At(3u + c); }
fn tapOff(c: u32) -> u32 { return u32At(6u + c); }
const TAPS_BASE: u32 = 12u;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = imgW() * imgH();
    let px = gid.x;
    if (px >= n) { return; }
    let w = imgW();
    let x = px % w;
    let y = px / w;
    let wi = i32(w);
    let xi = i32(x);
    for (var c = 0u; c < 3u; c = c + 1u) {
        let r = chanRadius(c);
        if (r == 0u) { continue; }
        let base = TAPS_BASE + tapOff(c);
        var acc = 0.0;
        for (var ti = 0u; ti < 2u * r + 1u; ti = ti + 1u) {
            let sx = xi + i32(ti) - i32(r);
            if (sx < 0 || sx >= wi) { continue; }
            acc = acc + params[base + ti] * src[(y * w + u32(sx)) * 3u + c];
        }
        tmp[px * 3u + c] = acc;
    }
}
"#;

const HALATION_V_SHADER: &str = r#"
// Halation §C2, vertical pass + additive halo:
// dst[c] = src[c] + α_c · Σ_j taps_c[j]·tmp[c][y+j−r] (zero padding),
// pass-through for inactive channels (the CPU apply_halation skip).
@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read> tmp: array<f32>;
@group(0) @binding(3) var<storage, read_write> dst: array<f32>;

fn u32At(i: u32) -> u32 { return bitcast<u32>(params[i]); }
fn imgW() -> u32 { return u32At(1u); }
fn imgH() -> u32 { return u32At(2u); }
fn chanRadius(c: u32) -> u32 { return u32At(3u + c); }
fn tapOff(c: u32) -> u32 { return u32At(6u + c); }
fn chanAlpha(c: u32) -> f32 { return params[9u + c]; }
const TAPS_BASE: u32 = 12u;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = imgW() * imgH();
    let px = gid.x;
    if (px >= n) { return; }
    let w = imgW();
    let x = px % w;
    let y = px / w;
    let hi = i32(imgH());
    let yi = i32(y);
    for (var c = 0u; c < 3u; c = c + 1u) {
        let i3 = px * 3u + c;
        let r = chanRadius(c);
        if (r == 0u) {
            dst[i3] = src[i3];
            continue;
        }
        let base = TAPS_BASE + tapOff(c);
        var halo = 0.0;
        for (var tj = 0u; tj < 2u * r + 1u; tj = tj + 1u) {
            let sy = yi + i32(tj) - i32(r);
            if (sy < 0 || sy >= hi) { continue; }
            halo = halo + params[base + tj] * tmp[(u32(sy) * w + x) * 3u + c];
        }
        dst[i3] = src[i3] + chanAlpha(c) * halo;
    }
}
"#;

/// Adapter probe (parity tests print the adapter for the evidence log).
pub fn first_adapter_report() -> Option<crate::AdapterReport> {
    crate::readback::best_adapter()
}

struct GpuCtx {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn acquire() -> Result<GpuCtx, GpuError> {
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
    Ok(GpuCtx { device, queue })
}

fn compute_pipeline(device: &wgpu::Device, label: &str, code: &str) -> wgpu::ComputePipeline {
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

fn read_back(device: &wgpu::Device, readback: &wgpu::Buffer) -> Result<Vec<f32>, GpuError> {
    // Two-buffer readback idiom (S1 lesson — see lut3d.rs).
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

/// Run the grain-gather kernel on `src` (interleaved RGB f32, `w·h·3`).
/// Returns the grained plane (same layout). No adapter → honest
/// `NoAdapter`.
pub fn run_grain(params: &GrainGpuParams, src: &[f32]) -> Result<Vec<f32>, GpuError> {
    let packed = pack_grain(params).map_err(GpuError::Wgpu)?;
    let n = params.w as usize * params.h as usize;
    let out_len = n * 3;
    if out_len == 0 {
        return Ok(Vec::new());
    }
    if src.len() != out_len {
        return Err(GpuError::Wgpu(format!(
            "grain kernel: src length {} does not match w·h·3 = {out_len}",
            src.len()
        )));
    }

    let ctx = acquire()?;
    let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("grain-params"),
        contents: bytemuck::cast_slice(&packed),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let src_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("grain-src"),
        contents: bytemuck::cast_slice(src),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_storage = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grain-out"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grain-out-readback"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let pipeline = compute_pipeline(&ctx.device, "grain-gather", GRAIN_SHADER);
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grain-bg"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out_storage.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grain-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grain-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_storage, 0, &out_readback, 0, out_readback.size());
    ctx.queue.submit(Some(encoder.finish()));

    read_back(&ctx.device, &out_readback)
}

/// Run the halation kernel (two dispatches: horizontal then vertical)
/// on `src` (interleaved RGB f32, `w·h·3`). Returns `X + H(X)` per the
/// CPU `apply_halation`. No adapter → honest `NoAdapter`.
pub fn run_halation(params: &HalationGpuParams, src: &[f32]) -> Result<Vec<f32>, GpuError> {
    let packed = pack_halation(params).map_err(GpuError::Wgpu)?;
    let n = params.w as usize * params.h as usize;
    let out_len = n * 3;
    if out_len == 0 {
        return Ok(Vec::new());
    }
    if src.len() != out_len {
        return Err(GpuError::Wgpu(format!(
            "halation kernel: src length {} does not match w·h·3 = {out_len}",
            src.len()
        )));
    }

    let ctx = acquire()?;
    let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("halation-params"),
        contents: bytemuck::cast_slice(&packed),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let src_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("halation-src"),
        contents: bytemuck::cast_slice(src),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let tmp = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("halation-tmp"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let out_storage = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("halation-out"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("halation-out-readback"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let h_pipeline = compute_pipeline(&ctx.device, "halation-h", HALATION_H_SHADER);
    let v_pipeline = compute_pipeline(&ctx.device, "halation-v", HALATION_V_SHADER);
    let h_bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("halation-h-bg"),
        layout: &h_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: tmp.as_entire_binding(),
            },
        ],
    });
    let v_bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("halation-v-bg"),
        layout: &v_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: tmp.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_storage.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("halation-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("halation-h-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&h_pipeline);
        pass.set_bind_group(0, &h_bg, &[]);
        pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("halation-v-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&v_pipeline);
        pass.set_bind_group(0, &v_bg, &[]);
        pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_storage, 0, &out_readback, 0, out_readback.size());
    ctx.queue.submit(Some(encoder.finish()));

    read_back(&ctx.device, &out_readback)
}

#[cfg(all(test, feature = "wgpu"))]
mod shader_parse {
    /// Offline WGSL validation via naga — catches syntax/builtin errors
    /// WITHOUT an adapter (the `log10` lesson, CI run 40075333761: the
    /// sandbox cannot run the parity gate, but it can always parse).
    fn validate(src: &str, name: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{name} WGSL must parse in naga: {e}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name} WGSL must validate in naga: {e}"));
    }

    #[test]
    fn grain_shader_parses_and_validates_in_naga() {
        validate(super::GRAIN_SHADER, "grain");
    }

    #[test]
    fn halation_h_shader_parses_and_validates_in_naga() {
        validate(super::HALATION_H_SHADER, "halation-h");
    }

    #[test]
    fn halation_v_shader_parses_and_validates_in_naga() {
        validate(super::HALATION_V_SHADER, "halation-v");
    }
}
