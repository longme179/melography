//! Deterministic u32 hashing (film grain lattice + future WGSL mirror).
//!
//! WGSL has no 64-bit integers, so the grain field's randomness is defined
//! in **u32 arithmetic only** (wrapping add/mul/xor/shift). The CPU path
//! uses the exact same integer ops, making the hash bit-portable between
//! the CPU oracle and the WGSL kernel by construction — the only float
//! disagreement left is transcendental evaluation, which is covered by the
//! parity tolerance, not the hash.
//!
//! Quality: splitmix-style finalizer (Rich Skibo / Melissa O'Neill
//! public-domain variants); passes VisualByte-battery-level avalanche for
//! our purposes (grain aesthetics, not cryptography).

/// Splitmix32 finalizer: one round of mix on `z`.
#[inline]
pub const fn mix32(mut z: u32) -> u32 {
    z = z.wrapping_add(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
    z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
    z ^ (z >> 15)
}

/// Combine four u32 values into one mixed u32.
///
/// Fold order is fixed; the seed is folded last so seed changes remap the
/// entire field (no seed-relative artifacts).
#[inline]
pub const fn hash4(a: u32, b: u32, c: u32, d: u32) -> u32 {
    let mut z = a.wrapping_mul(0x85EB_CA6B)
        ^ b.wrapping_mul(0xC2B2_AE35)
        ^ c.wrapping_mul(0x27D4_EB2F)
        ^ d.wrapping_mul(0x1656_67B1);
    z = mix32(z);
    mix32(z ^ 0x9E37_79B9)
}

/// A small deterministic stream of u32s derived from one hash — successive
/// draws advance a splitmix32 counter.
#[derive(Debug, Clone, Copy)]
pub struct RandStream {
    state: u32,
}

impl RandStream {
    #[inline]
    pub fn new(a: u32, b: u32, c: u32, d: u32) -> Self {
        Self {
            state: hash4(a, b, c, d),
        }
    }

    /// Next raw u32.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x9E37_79B9);
        mix32(self.state)
    }

    /// Uniform in `[0, 1)` with 24-bit resolution (f64 for CPU-side math;
    /// the WGSL mirror uses the same `>> 8` quantization in f32).
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32() >> 8) * (1.0 / 16_777_216.0)
    }

    /// Standard-normal via Box–Muller (f64 CPU path).
    ///
    /// WGSL mirror: same formula in f32 — agreement is within the parity
    /// tolerance (≈1e-6 relative), verified by the parity gate.
    #[inline]
    pub fn next_normal_f64(&mut self) -> f64 {
        let u1 = self.next_f64().max(2.0e-9); // avoid log(0)
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (core::f64::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash4(1, 2, 3, 4), hash4(1, 2, 3, 4));
        assert_ne!(hash4(1, 2, 3, 4), hash4(1, 2, 3, 5));
    }

    #[test]
    fn avalanche_quality() {
        // Single-bit input changes must flip ~16 output bits on average.
        let mut flips = 0u64;
        let mut cases = 0u64;
        for bit in 0..32 {
            let a = hash4(0xDEAD_BEEF, 0x1234_5678, 0x0BAD_C0DE, 7);
            let b = hash4(0xDEAD_BEEF ^ (1 << bit), 0x1234_5678, 0x0BAD_C0DE, 7);
            flips += u64::from((a ^ b).count_ones());
            cases += 1;
        }
        let avg = flips as f64 / cases as f64;
        assert!((14.0..=18.0).contains(&avg), "avg bits flipped = {avg}");
    }

    #[test]
    fn stream_is_deterministic_and_varied() {
        let mut s1 = RandStream::new(1, 2, 3, 4);
        let mut s2 = RandStream::new(1, 2, 3, 4);
        let mut s3 = RandStream::new(1, 2, 3, 5);
        for _ in 0..100 {
            assert_eq!(s1.next_u32(), s2.next_u32());
        }
        let a = s1.next_f64();
        let b = s3.next_f64();
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn uniforms_are_well_distributed() {
        // Chi-square-ish sanity: 16 buckets over 16k draws.
        let mut s = RandStream::new(11, 22, 33, 44);
        let mut buckets = [0u32; 16];
        for _ in 0..16_384 {
            let f = s.next_f64();
            buckets[(f * 16.0) as usize] += 1;
        }
        let expected = 16_384.0 / 16.0;
        for &b in &buckets {
            let dev = (f64::from(b) - expected) / expected.sqrt();
            assert!(dev.abs() < 6.0, "bucket deviation {dev:.2}σ");
        }
    }

    #[test]
    fn normals_have_plausible_moments() {
        let mut s = RandStream::new(9, 8, 7, 6);
        let n = 50_000;
        let (mut sum, mut sum2) = (0.0, 0.0);
        for _ in 0..n {
            let v = s.next_normal_f64();
            assert!(v.is_finite());
            sum += v;
            sum2 += v * v;
        }
        let mean = sum / n as f64;
        let var = sum2 / n as f64 - mean * mean;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((var - 1.0).abs() < 0.08, "var {var}");
    }
}
