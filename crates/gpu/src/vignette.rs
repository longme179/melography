//! WGSL vignette kernel (FILM_MODEL §B, A2.4) — the lens falloff
//! `e · cos(r_n·half_fov)^k` per pixel, the last spatial stage the
//! full-chain GPU preview needed beyond A2.3.
//!
//! Parity contract: same formula as the CPU `apply_vignette_inplace`,
//! with positions resolved on the CPU in f64 (the A2.3 lesson —
//! absolute film coordinates never enter the shader; per-column `du`
//! and per-row `dv` LUTs leave only small-number math). Transcendentals
//! (cos, pow) run in f32 inside the ≤1e-3 tolerance; the repo-external
//! cross-check (`a24_gpu_wiring_check.py`) measured 2.2e-7 worst-case
//! over the sane parameter envelope.
//!
//! `k ≤ 0` is the exact identity on both sides (`cos(θ)^0 = 1`), so the
//! orchestrator skips the dispatch entirely — the packer still refuses
//! nothing; the skip is a `ChainStages` decision, mirrored from the CPU
//! fast path.

use crate::context::GpuContext;
use crate::spatial_types::{pack_vignette, VignetteGpuParams};
use crate::GpuError;
use wgpu::util::DeviceExt;

pub(crate) const VIGNETTE_SHADER: &str = r#"
// Vignette §B: out = src · cos(r_n·half_fov)^k. r_n comes from the
// per-column du and per-row dv LUTs (mm from frame centre) — small
// numbers only, no absolute coordinates in the shader.
@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

fn u32At(i: u32) -> u32 { return bitcast<u32>(params[i]); }
fn imgW() -> u32 { return u32At(1u); }
fn imgH() -> u32 { return u32At(2u); }

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = imgW() * imgH();
    let px = gid.x;
    if (px >= n) { return; }
    let w = imgW();
    let x = px % w;
    let y = px / w;
    let du = params[6u + x];
    let dv = params[6u + w + y];
    let r_n = length(vec2<f32>(du, dv)) / params[3];
    let theta = r_n * params[4];
    let falloff = pow(cos(theta), params[5]);
    let i3 = px * 3u;
    dst[i3]         = src[i3]         * falloff;
    dst[i3 + 1u]    = src[i3 + 1u]    * falloff;
    dst[i3 + 2u]    = src[i3 + 2u]    * falloff;
}
"#;

/// Run the vignette kernel on `src` (interleaved RGB f32, `w·h·3`) via
/// the persistent context (cached pipeline). Returns the vignetted
/// plane (same layout).
pub fn run_vignette_on(
    ctx: &GpuContext,
    params: &VignetteGpuParams,
    src: &[f32],
) -> Result<Vec<f32>, GpuError> {
    let packed = pack_vignette(params).map_err(GpuError::Wgpu)?;
    let n = params.w as usize * params.h as usize;
    let out_len = n * 3;
    if out_len == 0 {
        return Ok(Vec::new());
    }
    if src.len() != out_len {
        return Err(GpuError::Wgpu(format!(
            "vignette kernel: src length {} does not match w·h·3 = {out_len}",
            src.len()
        )));
    }

    let params_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vignette-params"),
            contents: bytemuck::cast_slice(&packed),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let src_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vignette-src"),
            contents: bytemuck::cast_slice(src),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let out_storage = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vignette-out"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let out_readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vignette-out-readback"),
        size: (out_len * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let pipeline = &ctx.pipelines.vignette;
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vignette-bg"),
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
            label: Some("vignette-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("vignette-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_storage, 0, &out_readback, 0, out_readback.size());
    ctx.queue.submit(Some(encoder.finish()));

    crate::context::read_back(&ctx.device, &out_readback)
}

#[cfg(all(test, feature = "wgpu"))]
mod shader_parse {
    /// Offline WGSL validation via naga — the A2.3 lesson: parse +
    /// validate everywhere, adapter or not.
    #[test]
    fn vignette_shader_parses_and_validates_in_naga() {
        let module = naga::front::wgsl::parse_str(super::VIGNETTE_SHADER)
            .unwrap_or_else(|e| panic!("vignette WGSL must parse in naga: {e}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("vignette WGSL must validate in naga: {e}"));
    }
}
