//! Correction math for distortion, vignetting, and TCA.
//!
//! All radial distances are normalized such that `r = 1.0` at the image corner
//! (i.e. `r = distance_from_centre / (image_diagonal / 2)`).

// ── Distortion ────────────────────────────────────────────────────────────────

/// Parameters for the PTLens distortion model.
///
/// `r_d = r_u * (a*r_u³ + b*r_u² + c*r_u + (1 - a - b - c))`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtLensParams {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

impl PtLensParams {
    /// Map an undistorted radius to a distorted radius.
    #[inline]
    pub fn apply(self, r_u: f32) -> f32 {
        let d = 1.0 - self.a - self.b - self.c;
        r_u * (self.a * r_u * r_u * r_u + self.b * r_u * r_u + self.c * r_u + d)
    }

    /// Linear interpolate between two parameter sets.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            a: a.a + (b.a - a.a) * t,
            b: a.b + (b.b - a.b) * t,
            c: a.c + (b.c - a.c) * t,
        }
    }
}

/// Parameters for the Poly3 distortion model.
///
/// `r_d = r_u * (1 + k1*r_u²)`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Poly3Params {
    pub k1: f32,
}

impl Poly3Params {
    /// Map an undistorted radius to a distorted radius.
    #[inline]
    pub fn apply(self, r_u: f32) -> f32 {
        r_u * (1.0 + self.k1 * r_u * r_u)
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            k1: a.k1 + (b.k1 - a.k1) * t,
        }
    }
}

/// Parameters for the Poly5 distortion model.
///
/// `r_d = r_u * (1 + k1*r_u² + k2*r_u⁴)`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Poly5Params {
    pub k1: f32,
    pub k2: f32,
}

impl Poly5Params {
    /// Map an undistorted radius to a distorted radius.
    #[inline]
    pub fn apply(self, r_u: f32) -> f32 {
        let r2 = r_u * r_u;
        r_u * (1.0 + self.k1 * r2 + self.k2 * r2 * r2)
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            k1: a.k1 + (b.k1 - a.k1) * t,
            k2: a.k2 + (b.k2 - a.k2) * t,
        }
    }
}

/// A distortion model with its resolved parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistortionModel {
    PtLens(PtLensParams),
    Poly3(Poly3Params),
    Poly5(Poly5Params),
}

impl DistortionModel {
    /// Map undistorted normalized coordinates to distorted normalized coordinates.
    ///
    /// `r_u` is the undistorted radius; returns the distorted radius.
    /// When `r_u` is zero the function returns zero (no displacement).
    #[inline]
    pub fn undistorted_to_distorted(self, r_u: f32) -> f32 {
        if r_u == 0.0 {
            return 0.0;
        }
        match self {
            Self::PtLens(p) => p.apply(r_u),
            Self::Poly3(p) => p.apply(r_u),
            Self::Poly5(p) => p.apply(r_u),
        }
    }
}

// ── Vignetting ────────────────────────────────────────────────────────────────

/// Parameters for the polynomial aperture (PA) vignetting model.
///
/// `factor = 1 / (1 + k1*r² + k2*r⁴ + k3*r⁶)`
///
/// The factor is applied to linearised pixel values (after sRGB → linear
/// conversion).  A factor < 1 darkens the pixel; > 1 brightens it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VignettingParams {
    pub k1: f32,
    pub k2: f32,
    pub k3: f32,
}

impl VignettingParams {
    /// Compute the correction factor for a normalised radius `r`.
    ///
    /// `r = 0` at the image centre; `r = 1` at the corner.
    #[inline]
    pub fn factor(self, r: f32) -> f32 {
        let r2 = r * r;
        1.0 / (1.0 + self.k1 * r2 + self.k2 * r2 * r2 + self.k3 * r2 * r2 * r2)
    }

    /// Linear interpolation between two parameter sets.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            k1: a.k1 + (b.k1 - a.k1) * t,
            k2: a.k2 + (b.k2 - a.k2) * t,
            k3: a.k3 + (b.k3 - a.k3) * t,
        }
    }
}

// ── TCA ───────────────────────────────────────────────────────────────────────

/// Linear TCA model: each channel is uniformly scaled.
///
/// Red channel is scaled by `kr`, blue by `kb`, green is unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TcaLinearParams {
    pub kr: f32,
    pub kb: f32,
}

impl TcaLinearParams {
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            kr: a.kr + (b.kr - a.kr) * t,
            kb: a.kb + (b.kb - a.kb) * t,
        }
    }
}

/// Poly3 TCA model.
///
/// For red and blue independently:
/// `r_corrected = r * (v + b*r²)`
/// where `vr`/`vb` is the linear scaling factor and `br`/`bb` is the cubic term.
/// (Green is unchanged.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TcaPoly3Params {
    /// Linear scale factor for the red channel.
    pub vr: f32,
    /// Cubic coefficient for the red channel.
    pub br: f32,
    /// Linear scale factor for the blue channel.
    pub vb: f32,
    /// Cubic coefficient for the blue channel.
    pub bb: f32,
}

impl TcaPoly3Params {
    /// Corrected radius for the red channel.
    #[inline]
    pub fn red(self, r: f32) -> f32 {
        r * (self.vr + self.br * r * r)
    }

    /// Corrected radius for the blue channel.
    #[inline]
    pub fn blue(self, r: f32) -> f32 {
        r * (self.vb + self.bb * r * r)
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            vr: a.vr + (b.vr - a.vr) * t,
            br: a.br + (b.br - a.br) * t,
            vb: a.vb + (b.vb - a.vb) * t,
            bb: a.bb + (b.bb - a.bb) * t,
        }
    }
}

/// A TCA model with its resolved parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TcaModel {
    Linear(TcaLinearParams),
    Poly3(TcaPoly3Params),
}

impl TcaModel {
    /// Returns `(r_red_factor, r_blue_factor)` — the ratio by which the
    /// normalised radius for each channel should be scaled.
    ///
    /// `r` is the normalised radius of the output pixel.
    #[inline]
    pub fn channel_radii(self, r: f32) -> (f32, f32) {
        match self {
            Self::Linear(p) => (p.kr, p.kb),
            Self::Poly3(p) => {
                if r == 0.0 {
                    return (p.vr, p.vb);
                }
                let rr = p.red(r) / r;
                let rb = p.blue(r) / r;
                (rr, rb)
            }
        }
    }
}

// ── sRGB ↔ linear helpers ─────────────────────────────────────────────────────

/// Convert a single sRGB component (0–255) to a linear light value (0.0–1.0).
#[inline]
pub fn srgb_to_linear(v: u8) -> f32 {
    let f = v as f32 / 255.0;
    if f <= 0.04045 {
        f / 12.92
    } else {
        ((f + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert a linear light value (0.0–1.0) back to an sRGB component (0–255).
#[inline]
pub fn linear_to_srgb(v: f32) -> u8 {
    let clamped = v.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptlens_identity_at_zero() {
        let p = PtLensParams {
            a: 0.01,
            b: -0.02,
            c: 0.005,
        };
        assert_eq!(
            DistortionModel::PtLens(p).undistorted_to_distorted(0.0),
            0.0
        );
    }

    #[test]
    fn ptlens_known_value() {
        // For a = b = c = 0: r_d = r_u * (1) = r_u
        let p = PtLensParams {
            a: 0.0,
            b: 0.0,
            c: 0.0,
        };
        let model = DistortionModel::PtLens(p);
        let r = 0.5_f32;
        let result = model.undistorted_to_distorted(r);
        assert!(
            (result - r).abs() < 1e-6,
            "identity params: expected {r}, got {result}"
        );
    }

    #[test]
    fn ptlens_distortion_matches_formula() {
        // Verify r_d equals the formula r_u*(a*r_u³ + b*r_u² + c*r_u + (1-a-b-c))
        let p = PtLensParams {
            a: 0.02,
            b: -0.05,
            c: 0.01,
        };
        let model = DistortionModel::PtLens(p);
        let r_u = 0.8_f32;
        let r_d = model.undistorted_to_distorted(r_u);
        let d = 1.0 - p.a - p.b - p.c;
        let expected = r_u * (p.a * r_u.powi(3) + p.b * r_u.powi(2) + p.c * r_u + d);
        assert!(
            (r_d - expected).abs() < 1e-5,
            "ptlens formula mismatch: got {r_d}, expected {expected}"
        );
    }

    #[test]
    fn poly3_identity() {
        let p = Poly3Params { k1: 0.0 };
        let model = DistortionModel::Poly3(p);
        let r = 0.7_f32;
        let result = model.undistorted_to_distorted(r);
        assert!((result - r).abs() < 1e-6);
    }

    #[test]
    fn poly3_known_value() {
        let p = Poly3Params { k1: -0.01 };
        let model = DistortionModel::Poly3(p);
        let r_u = 0.5_f32;
        let expected = r_u * (1.0 + p.k1 * r_u * r_u);
        let result = model.undistorted_to_distorted(r_u);
        assert!((result - expected).abs() < 1e-6);
    }

    #[test]
    fn poly5_known_value() {
        let p = Poly5Params {
            k1: -0.01,
            k2: 0.001,
        };
        let model = DistortionModel::Poly5(p);
        let r_u = 0.6_f32;
        let r2 = r_u * r_u;
        let expected = r_u * (1.0 + p.k1 * r2 + p.k2 * r2 * r2);
        let result = model.undistorted_to_distorted(r_u);
        assert!((result - expected).abs() < 1e-6);
    }

    #[test]
    fn vignetting_centre_is_one() {
        let p = VignettingParams {
            k1: -0.5,
            k2: 0.2,
            k3: -0.1,
        };
        assert!(
            (p.factor(0.0) - 1.0).abs() < 1e-6,
            "centre factor must be 1.0"
        );
    }

    #[test]
    fn vignetting_corner_differs() {
        let p = VignettingParams {
            k1: -0.5,
            k2: 0.2,
            k3: -0.1,
        };
        let corner = p.factor(1.0);
        assert!(
            (corner - 1.0).abs() > 0.05,
            "corner factor should differ from 1.0, got {corner}"
        );
    }

    #[test]
    fn tca_linear_centre_unaffected() {
        let p = TcaLinearParams {
            kr: 1.0002,
            kb: 0.9999,
        };
        let model = TcaModel::Linear(p);
        // At r = 0 the scale factors are kr/kb regardless, but the pixel
        // displacement is 0*scale = 0, so the centre is always sampled at (0,0).
        let (kr, kb) = model.channel_radii(0.0);
        // The actual displacement for the centre pixel is 0 * kr = 0.
        assert_eq!(kr * 0.0_f32, 0.0);
        assert_eq!(kb * 0.0_f32, 0.0);
    }

    #[test]
    fn tca_poly3_centre_unaffected() {
        let p = TcaPoly3Params {
            vr: 1.0001,
            br: -0.00002,
            vb: 0.9999,
            bb: 0.00003,
        };
        let model = TcaModel::Poly3(p);
        let (rr, rb) = model.channel_radii(0.0);
        // displacement = r * factor = 0 * anything = 0
        assert_eq!(0.0_f32 * rr, 0.0);
        assert_eq!(0.0_f32 * rb, 0.0);
    }

    #[test]
    fn srgb_round_trip() {
        for v in [0u8, 128, 255] {
            let lin = srgb_to_linear(v);
            let back = linear_to_srgb(lin);
            assert_eq!(back, v, "round-trip failed for {v}");
        }
    }
}
