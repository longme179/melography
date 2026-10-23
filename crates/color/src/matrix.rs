//! 3×3 matrix math shared by the color pipeline. Derivation happens in f64
//! (once, at first use); runtime pixel transforms use f32. All operations are
//! written with a fixed evaluation order so results are bit-deterministic on
//! every supported platform (Spike S2 gate: identical hashes across
//! Linux/Windows/macOS).

use std::ops::Mul;

/// Row-major 3×3 matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    /// `[m00, m01, m02, m10, m11, m12, m20, m21, m22]`, row-major.
    pub m: [f64; 9],
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 {
        m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    #[must_use]
    pub const fn from_rows(r0: [f64; 3], r1: [f64; 3], r2: [f64; 3]) -> Self {
        Self {
            m: [
                r0[0], r0[1], r0[2], r1[0], r1[1], r1[2], r2[0], r2[1], r2[2],
            ],
        }
    }

    /// Closed-form inverse via adjugate / determinant (deterministic order).
    /// Returns `None` when (near-)singular.
    #[must_use]
    pub fn inverse(&self) -> Option<Mat3> {
        let [a, b, c, d, e, f, g, h, i] = self.m;
        // Cofactors for the first column of the adjugate:
        let adj00 = e * i - f * h;
        let adj10 = f * g - d * i;
        let adj20 = d * h - e * g;
        let det = a * adj00 + d * (c * h - b * i) + g * (b * f - c * e);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Mat3 {
            m: [
                adj00 * inv_det,
                (c * h - b * i) * inv_det,
                (b * f - c * e) * inv_det,
                adj10 * inv_det,
                (a * i - c * g) * inv_det,
                (c * d - a * f) * inv_det,
                adj20 * inv_det,
                (b * g - a * h) * inv_det,
                (a * e - b * d) * inv_det,
            ],
        })
    }

    /// Matrix–vector product.
    #[must_use]
    pub fn apply(&self, v: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
            m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
            m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
        ]
    }

    /// Matrix–vector product in f32 (runtime pixel path).
    #[must_use]
    pub fn apply_f32(&self, v: [f32; 3]) -> [f32; 3] {
        let r = self.apply([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]);
        [r[0] as f32, r[1] as f32, r[2] as f32]
    }

    /// Convert to f32 row-major for hot loops.
    #[must_use]
    pub fn to_f32(&self) -> [f32; 9] {
        self.m.map(|x| x as f32)
    }
}

impl Mul for Mat3 {
    type Output = Mat3;
    /// `self * other` (apply `other` first, then `self`).
    fn mul(self, rhs: Mat3) -> Mat3 {
        let a = &self.m;
        let b = &rhs.m;
        Mat3 {
            m: [
                a[0] * b[0] + a[1] * b[3] + a[2] * b[6],
                a[0] * b[1] + a[1] * b[4] + a[2] * b[7],
                a[0] * b[2] + a[1] * b[5] + a[2] * b[8],
                a[3] * b[0] + a[4] * b[3] + a[5] * b[6],
                a[3] * b[1] + a[4] * b[4] + a[5] * b[7],
                a[3] * b[2] + a[4] * b[5] + a[5] * b[8],
                a[6] * b[0] + a[7] * b[3] + a[8] * b[6],
                a[6] * b[1] + a[7] * b[4] + a[8] * b[7],
                a[6] * b[2] + a[7] * b[5] + a[8] * b[8],
            ],
        }
    }
}

/// Column-major scaling matrix `diag(s)`.
#[must_use]
pub fn diag(s: [f64; 3]) -> Mat3 {
    Mat3 {
        m: [s[0], 0.0, 0.0, 0.0, s[1], 0.0, 0.0, 0.0, s[2]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_inverse_is_identity() {
        assert_eq!(Mat3::IDENTITY.inverse().unwrap(), Mat3::IDENTITY);
    }

    #[test]
    fn inverse_roundtrip() {
        let m = Mat3::from_rows(
            [0.4124, 0.3576, 0.1805],
            [0.2126, 0.7152, 0.0722],
            [0.0193, 0.1192, 0.9505],
        );
        let prod = m * m.inverse().unwrap();
        for (got, want) in prod.m.iter().zip(Mat3::IDENTITY.m) {
            assert!((got - want).abs() < 1e-12, "roundtrip: {got} vs {want}");
        }
    }

    #[test]
    fn singular_has_no_inverse() {
        assert!(
            Mat3::from_rows([1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 1.0, 0.0])
                .inverse()
                .is_none()
        );
    }

    #[test]
    fn mul_order_matches_apply() {
        // a = x-shear, b = 2x scale (non-commuting pair)
        let a = Mat3::from_rows([1.0, 2.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let b = Mat3::from_rows([2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let v = [1.0, 1.0, 1.0];
        // (a*b) applies b FIRST: v -> (2,1,1) -> a -> (2+2, 1, 1)
        assert_eq!((a * b).apply(v), [4.0, 1.0, 1.0]);
        // (b*a) applies a FIRST: v -> (3,1,1) -> b -> (6,1,1)
        assert_eq!((b * a).apply(v), [6.0, 1.0, 1.0]);
    }
}
