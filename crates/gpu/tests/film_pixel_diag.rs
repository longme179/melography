//! Local f32-mirror diagnostic for the film pixel parity gap seen on CI
//! Metal (worst |Δ| = 1.017e-2). Simulates the WGSL formulas in Rust f32
//! exactly (same order of operations) and diffs against the CPU f64 chain
//! — no adapter needed. `#[ignore]`d diagnostic, not a gate.

#![cfg(feature = "wgpu")]

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render, FilmChainParams};
use melography_film::emulsion::{resolve_emulsion, Adjustments};
use melography_film::gpu_bridge::pack_film_pixel_params;
use melography_film::print::Print;

fn battery() -> Vec<f32> {
    let n = 219;
    let mut v = Vec::with_capacity(n * 3);
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let e = 10f64.powf(-3.6 + 3.8 * t);
        v.push((e * 1.08) as f32);
        v.push((e * 0.96) as f32);
        v.push((e * 0.82) as f32);
    }
    v
}

/// f32 simulation of the WGSL kernel (same formulas/order).
fn wgsl_sim(p: &melography_gpu::film_pixel_types::FilmPixelGpuParams, e: &[f32; 3]) -> [f32; 3] {
    use melography_gpu::film_pixel_types::CurveGpu;
    fn spline_eval(c: &CurveGpu, x: f32) -> f32 {
        if x <= c.xs[0] {
            return c.ys[0];
        }
        let n = c.xs.len();
        if x >= c.xs[n - 1] {
            return c.ys[n - 1];
        }
        let (mut lo, mut hi) = (0usize, n - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if c.xs[mid] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (xa, ya, ma) = (c.xs[lo], c.ys[lo], c.ms[lo]);
        let (xb, yb, mb) = (c.xs[lo + 1], c.ys[lo + 1], c.ms[lo + 1]);
        let h = xb - xa;
        let t = (x - xa) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * ya
            + (t3 - 2.0 * t2 + t) * h * ma
            + (-2.0 * t3 + 3.0 * t2) * yb
            + (t3 - t2) * h * mb
    }
    fn log10f(x: f32) -> f32 {
        x.log2() * 0.301_030_f32
    }
    fn mat3(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
        [
            m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
            m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
            m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
        ]
    }

    let layers = if p.layers_mono { 1 } else { 3 };
    let mut e = *e;
    if p.pre_desat > 0.0 {
        let lum = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
        for c in &mut e {
            *c = *c * (1.0 - p.pre_desat) + lum * p.pre_desat;
        }
    }
    let x = mat3(&p.m_exc, e);
    let x = [
        x[0] * p.total_scale,
        x[1] * p.total_scale,
        x[2] * p.total_scale,
    ];

    let mut d = [0.0f32; 3];
    let mut f_mid = [0.0f32; 3];
    let mut f_lo = [0.0f32; 3];
    let mut f_hi = [0.0f32; 3];
    for li in 0..layers {
        let c = &p.hd_curves[li];
        let log_e = log10f(x[li].max(p.x_floor));
        let n = c.xs.len();
        let f = spline_eval(c, log_e);
        let xm0 = c.xs[0];
        let xm1 = c.xs[n - 1];
        let vm = spline_eval(c, 0.5 * (xm0 + xm1));
        let v0 = spline_eval(c, xm0);
        let v1 = spline_eval(c, xm1);
        f_mid[li] = vm;
        f_lo[li] = v0.min(v1);
        f_hi[li] = v0.max(v1);
        let mut fg = vm + p.gamma_mult[li] * (f - vm);
        fg = fg.clamp(f_lo[li], f_hi[li]);
        d[li] = p.d_min[li] + fg;
    }
    if p.layers_mono {
        d = [d[0]; 3];
    }
    let d_in = d;
    for i in 0..layers {
        let mut off = 0.0f32;
        for j in 0..layers {
            if i != j {
                off += p.crosstalk[i * 3 + j] * d_in[j];
            }
        }
        d[i] += p.crosstalk_scale * off;
    }
    if p.bleach_bypass > 0.0 && !p.layers_mono {
        let lum = 0.25 * d[0] + 0.5 * d[1] + 0.25 * d[2];
        let silver = ((lum - 0.15) * 1.9).clamp(0.0, 2.3);
        for c in &mut d {
            *c = (1.0 - p.bleach_bypass) * *c + p.bleach_bypass * silver;
        }
    }
    for c in &mut d {
        *c = c.max(0.0);
    }

    let mut lin = [0.0f32; 3];
    if p.polarity_reversal {
        for li in 0..layers {
            let d_eff = d[li] - p.d_mask[li];
            lin[li] = 10f32.powf(-d_eff) * 10f32.powf(p.paper_white_density);
        }
    } else {
        for li in 0..layers {
            let d_eff = d[li] - p.d_mask[li];
            let t = 10f32.powf(-d_eff);
            match p.path {
                0 => {
                    let x_p = p.print_exposure * t;
                    let d_p = spline_eval(&p.paper_curves[li], log10f(x_p.max(1e-12)));
                    lin[li] = 10f32.powf(p.paper_white_density - d_p);
                }
                1 => {
                    let x_p = p.print_exposure * t + p.scanner_glare;
                    let y = ((log10f(x_p.max(1e-12)) - p.scanner_black)
                        / (p.scanner_white - p.scanner_black))
                        .clamp(0.0, 1.0);
                    let z = y.powf(1.0 / p.scanner_gamma);
                    let sm = mat3(&p.scanner_matrix, [z; 3]);
                    lin[li] =
                        10f32.powf(p.paper_white_density - 2.2 * (1.0 - sm[li].clamp(0.0, 1.0)));
                }
                _ => {
                    lin[li] = 10f32.powf(p.paper_white_density - d_eff);
                }
            }
        }
    }
    for v in &mut lin {
        *v = v.clamp(0.0, 1.0);
    }
    let mut out = mat3(&p.output_matrix, lin);
    if p.layers_mono {
        out = [out[0]; 3];
    }
    out
}

#[test]
#[ignore = "diagnostic for the CI parity gap — run with --ignored --nocapture"]
fn diagnose_f32_parity_gap() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let reg = melography_presets::StockRegistry::open(&root.join("profiles")).unwrap();
    let inputs = battery();

    for profile in &reg.profiles {
        let adj = Adjustments {
            halation_intensity: 0.0,
            grain_intensity: 0.0,
            ..Adjustments::default()
        };
        let params = FilmChainParams::new(profile.clone())
            .unwrap()
            .with_adjustments(adj.clone(), Default::default())
            .unwrap();
        let mut img = ImageF32::alloc(219, 1, Channels::Rgb, 0.0);
        img.data_mut().copy_from_slice(&inputs);
        let cpu = render(&img, &params).unwrap();

        let em = resolve_emulsion(profile, &adj, 36.0 / 219.0);
        let print = Print::resolve(profile, None, 0.0);
        let gpu_params = pack_film_pixel_params(&em, &print, &adj);

        let mut worst = 0.0f64;
        let mut worst_i = 0usize;
        let mut worst_c = 0usize;
        for s in 0..219 {
            let e = [inputs[s * 3], inputs[s * 3 + 1], inputs[s * 3 + 2]];
            let sim = wgsl_sim(&gpu_params, &e);
            for c in 0..3 {
                let d = (f64::from(cpu.data()[s * 3 + c]) - f64::from(sim[c])).abs();
                if d > worst {
                    worst = d;
                    worst_i = s;
                    worst_c = c;
                }
            }
        }
        println!(
            "{}: worst |Δ| = {worst:.3e} at sample {worst_i} ch {worst_c} (e = {:e})",
            profile.id,
            inputs[worst_i * 3]
        );
    }
}
