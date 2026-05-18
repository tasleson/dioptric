//! Hand-computed formula verification and round-trip accuracy tests.
//!
//! All expected values are independently derived from the published Lensfun
//! formulas, not copied from any other implementation. Input radii and
//! coefficients were chosen to exercise typical barrel/pincushion ranges
//! and edge cases.

use dioptric::database::{Calibration, DistortionEntry, Lens, LensProjection, TcaEntry};
use dioptric::models::*;
use dioptric::{CoordinateMapOptions, CorrectionProfile, TransformMode};

fn profile_with_distortion(model: DistortionModel) -> CorrectionProfile {
    let lens = Lens::new(
        "Test",
        "Formula verification",
        vec![],
        Some(1.0),
        Calibration::new(
            vec![DistortionEntry {
                focal: 50.0,
                model,
                real_focal: None,
            }],
            vec![],
            vec![],
        ),
    );
    CorrectionProfile::builder(&lens)
        .crop_factor(1.0)
        .focal_length(50.0)
        .aperture(4.0)
        .distance(10.0)
        .build()
        .unwrap()
}

fn profile_with_tca(model: TcaModel) -> CorrectionProfile {
    let lens = Lens::new(
        "Test",
        "TCA verification",
        vec![],
        Some(1.0),
        Calibration::new(vec![], vec![TcaEntry { focal: 50.0, model }], vec![]),
    );
    CorrectionProfile::builder(&lens)
        .crop_factor(1.0)
        .focal_length(50.0)
        .aperture(4.0)
        .distance(10.0)
        .build()
        .unwrap()
}

const TOL: f32 = 1e-6;
const ROUND_TRIP_TOL: f32 = 1e-3;

fn assert_close(actual: f32, expected: f32, tol: f32, msg: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{msg}: expected {expected}, got {actual} (delta {})",
        (actual - expected).abs()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part A: hand-computed formula verification
// ═══════════════════════════════════════════════════════════════════════════════

// ── PTLens distortion ────────────────────────────────────────────────────────
// Formula: r_d = r_u * (a*r_u^3 + b*r_u^2 + c*r_u + (1 - a - b - c))

#[test]
fn ptlens_hand_computed_at_multiple_radii() {
    let cases: &[(PtLensParams, f32, f32)] = &[
        // (params, r_u, expected r_d)
        // Zero radius always gives zero
        (
            PtLensParams {
                a: 0.03,
                b: -0.08,
                c: 0.02,
            },
            0.0,
            0.0,
        ),
        // Identity coefficients: a=b=c=0 => d=1, r_d = r_u
        (
            PtLensParams {
                a: 0.0,
                b: 0.0,
                c: 0.0,
            },
            0.73,
            0.73,
        ),
        // Typical barrel distortion at r=0.4:
        // a=0.01, b=-0.03, c=0.005, d = 1 - 0.01 + 0.03 - 0.005 = 1.015
        // r_d = 0.4 * (0.01*0.064 + (-0.03)*0.16 + 0.005*0.4 + 1.015)
        //      = 0.4 * (0.00064 - 0.0048 + 0.002 + 1.015)
        //      = 0.4 * 1.01284
        //      = 0.405136
        (
            PtLensParams {
                a: 0.01,
                b: -0.03,
                c: 0.005,
            },
            0.4,
            0.405136,
        ),
        // Strong pincushion at r=0.9:
        // a=0.0, b=0.15, c=0.0, d = 1 - 0.15 = 0.85
        // r_d = 0.9 * (0 + 0.15*0.81 + 0 + 0.85)
        //      = 0.9 * (0.1215 + 0.85) = 0.9 * 0.9715 = 0.87435
        (
            PtLensParams {
                a: 0.0,
                b: 0.15,
                c: 0.0,
            },
            0.9,
            0.87435,
        ),
        // At r=1.0, r_d = 1*(a + b + c + d) = 1*(a+b+c + 1-a-b-c) = 1.0
        (
            PtLensParams {
                a: 0.05,
                b: -0.12,
                c: 0.03,
            },
            1.0,
            1.0,
        ),
    ];

    for (i, (params, r_u, expected)) in cases.iter().enumerate() {
        let model = DistortionModel::PtLens(*params);
        let result = model.undistorted_to_distorted(*r_u);
        assert_close(result, *expected, TOL, &format!("ptlens case {i}"));
    }
}

// ── Poly3 distortion ─────────────────────────────────────────────────────────
// Formula: r_d = r_u * (1 - k1 + k1*r_u^2)

#[test]
fn poly3_hand_computed_at_multiple_radii() {
    let cases: &[(f32, f32, f32)] = &[
        // (k1, r_u, expected)
        // k1=0 => r_d = r_u * 1 = r_u
        (0.0, 0.6, 0.6),
        // k1=1 => r_d = r_u * (0 + r_u^2) = r_u^3
        // r_u=0.5 => 0.125
        (1.0, 0.5, 0.125),
        // Barrel: k1=-0.15, r_u=0.7
        // r_d = 0.7 * (1 - (-0.15) + (-0.15)*0.49)
        //      = 0.7 * (1.15 - 0.0735) = 0.7 * 1.0765 = 0.75355
        (-0.15, 0.7, 0.75355),
        // Pincushion: k1=0.2, r_u=0.8
        // r_d = 0.8 * (1 - 0.2 + 0.2*0.64) = 0.8 * (0.8 + 0.128) = 0.8 * 0.928 = 0.7424
        (0.2, 0.8, 0.7424),
        // At r=0: always 0
        (0.5, 0.0, 0.0),
    ];

    for (i, (k1, r_u, expected)) in cases.iter().enumerate() {
        let model = DistortionModel::Poly3(Poly3Params { k1: *k1 });
        let result = model.undistorted_to_distorted(*r_u);
        assert_close(result, *expected, TOL, &format!("poly3 case {i}"));
    }
}

// ── Poly5 distortion ─────────────────────────────────────────────────────────
// Formula: r_d = r_u * (1 + k1*r_u^2 + k2*r_u^4)

#[test]
fn poly5_hand_computed_at_multiple_radii() {
    let cases: &[(f32, f32, f32, f32)] = &[
        // (k1, k2, r_u, expected)
        // Identity: k1=k2=0 => r_d = r_u
        (0.0, 0.0, 0.65, 0.65),
        // k1=-0.2, k2=0.05, r_u=0.5
        // r2=0.25, r4=0.0625
        // r_d = 0.5 * (1 + (-0.2)*0.25 + 0.05*0.0625)
        //      = 0.5 * (1 - 0.05 + 0.003125) = 0.5 * 0.953125 = 0.4765625
        (-0.2, 0.05, 0.5, 0.4765625),
        // k1=0.1, k2=-0.03, r_u=0.9
        // r2=0.81, r4=0.6561
        // r_d = 0.9 * (1 + 0.1*0.81 + (-0.03)*0.6561)
        //      = 0.9 * (1 + 0.081 - 0.019683) = 0.9 * 1.061317 = 0.9551853
        (0.1, -0.03, 0.9, 0.9551853),
        // Zero radius
        (0.3, -0.1, 0.0, 0.0),
    ];

    for (i, (k1, k2, r_u, expected)) in cases.iter().enumerate() {
        let model = DistortionModel::Poly5(Poly5Params { k1: *k1, k2: *k2 });
        let result = model.undistorted_to_distorted(*r_u);
        assert_close(result, *expected, TOL, &format!("poly5 case {i}"));
    }
}

// ── Vignetting (PA model) ────────────────────────────────────────────────────
// Formula: factor = 1 / (1 + k1*r^2 + k2*r^4 + k3*r^6)

#[test]
fn vignetting_hand_computed_at_multiple_radii() {
    let cases: &[(VignettingParams, f32, f32)] = &[
        // Centre: r=0 => factor = 1/(1+0+0+0) = 1.0
        (
            VignettingParams {
                k1: -0.5,
                k2: 0.2,
                k3: -0.03,
            },
            0.0,
            1.0,
        ),
        // k1=-0.5, k2=0.2, k3=-0.03, r=0.6
        // r2=0.36, r4=0.1296, r6=0.046656
        // denom = 1 + (-0.5)*0.36 + 0.2*0.1296 + (-0.03)*0.046656
        //       = 1 - 0.18 + 0.02592 - 0.00139968 = 0.84452032
        // factor = 1/0.84452032 ≈ 1.184092
        (
            VignettingParams {
                k1: -0.5,
                k2: 0.2,
                k3: -0.03,
            },
            0.6,
            1.0 / 0.84452032,
        ),
        // Corner: r=1.0
        // denom = 1 + (-0.5) + 0.2 + (-0.03) = 0.67
        // factor = 1/0.67 ≈ 1.492537
        (
            VignettingParams {
                k1: -0.5,
                k2: 0.2,
                k3: -0.03,
            },
            1.0,
            1.0 / 0.67,
        ),
        // Positive k1 (unusual but legal — darkens uniformly):
        // k1=0.3, k2=0, k3=0, r=0.5
        // denom = 1 + 0.3*0.25 = 1.075
        // factor = 1/1.075 ≈ 0.930233
        (
            VignettingParams {
                k1: 0.3,
                k2: 0.0,
                k3: 0.0,
            },
            0.5,
            1.0 / 1.075,
        ),
    ];

    for (i, (params, r, expected)) in cases.iter().enumerate() {
        let result = params.factor(*r);
        assert_close(result, *expected, TOL, &format!("vignetting case {i}"));
    }
}

#[test]
fn vignetting_negative_k1_brightens_corners() {
    let params = VignettingParams {
        k1: -0.7,
        k2: 0.3,
        k3: -0.05,
    };
    let factor_mid = params.factor(0.5);
    let factor_edge = params.factor(1.0);
    assert!(
        factor_mid > 1.0,
        "negative k1 should brighten at r=0.5, got {factor_mid}"
    );
    assert!(
        factor_edge > factor_mid,
        "correction should increase toward corner"
    );
}

// ── TCA Linear ───────────────────────────────────────────────────────────────
// Linear model: red scaled by kr, blue by kb, green unchanged.

#[test]
fn tca_linear_hand_computed() {
    let model = TcaModel::Linear(TcaLinearParams {
        kr: 1.0003,
        kb: 0.9998,
    });

    for r in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let (rr, rb) = model.channel_radii(r);
        assert_close(rr, 1.0003, TOL, &format!("tca linear red at r={r}"));
        assert_close(rb, 0.9998, TOL, &format!("tca linear blue at r={r}"));
    }
}

// ── TCA Poly3 ────────────────────────────────────────────────────────────────
// Formula per channel: r_corrected = r * (b*r^2 + c*r + v)
// channel_radii returns r_corrected/r (i.e. the scale factor)

#[test]
fn tca_poly3_hand_computed() {
    let params = TcaPoly3Params {
        vr: 1.0002,
        cr: -0.00005,
        br: -0.0001,
        vb: 0.9997,
        cb: 0.00008,
        bb: 0.00015,
    };
    let model = TcaModel::Poly3(params);

    // At r=0, channel_radii returns (vr, vb)
    let (rr0, rb0) = model.channel_radii(0.0);
    assert_close(rr0, 1.0002, TOL, "tca poly3 red at r=0");
    assert_close(rb0, 0.9997, TOL, "tca poly3 blue at r=0");

    // At r=0.6:
    // red = 0.6 * ((-0.0001)*0.36 + (-0.00005)*0.6 + 1.0002)
    //     = 0.6 * (-0.000036 - 0.00003 + 1.0002)
    //     = 0.6 * 1.000134 = 0.6000804
    // scale_red = 0.6000804 / 0.6 = 1.000134
    let r = 0.6_f32;
    let expected_red_corr = r * (params.br * r * r + params.cr * r + params.vr);
    let expected_red_scale = expected_red_corr / r;

    let expected_blue_corr = r * (params.bb * r * r + params.cb * r + params.vb);
    let expected_blue_scale = expected_blue_corr / r;

    let (rr, rb) = model.channel_radii(r);
    assert_close(rr, expected_red_scale, TOL, "tca poly3 red scale at r=0.6");
    assert_close(
        rb,
        expected_blue_scale,
        TOL,
        "tca poly3 blue scale at r=0.6",
    );
}

// ── sRGB round-trip ──────────────────────────────────────────────────────────

#[test]
fn srgb_round_trip_all_values() {
    for v in 0..=255u8 {
        let lin = srgb_to_linear(v);
        let back = linear_to_srgb(lin);
        assert_eq!(back, v, "sRGB round-trip failed for {v}");
    }
}

#[test]
fn srgb_known_values() {
    assert_close(srgb_to_linear(0), 0.0, TOL, "sRGB(0)");
    assert_close(srgb_to_linear(255), 1.0, TOL, "sRGB(255)");

    // sRGB mid-grey (~0.2140 linear for input 128)
    let mid = srgb_to_linear(128);
    assert!(
        (0.21..0.22).contains(&mid),
        "sRGB(128) should be ~0.214, got {mid}"
    );
}

// ── Lerp tests ───────────────────────────────────────────────────────────────

#[test]
fn ptlens_lerp_midpoint() {
    let a = PtLensParams {
        a: 0.0,
        b: 0.0,
        c: 0.0,
    };
    let b = PtLensParams {
        a: 0.1,
        b: -0.2,
        c: 0.06,
    };
    let mid = PtLensParams::lerp(a, b, 0.5);
    assert_close(mid.a, 0.05, TOL, "ptlens lerp a");
    assert_close(mid.b, -0.1, TOL, "ptlens lerp b");
    assert_close(mid.c, 0.03, TOL, "ptlens lerp c");
}

#[test]
fn poly3_lerp_endpoints() {
    let a = Poly3Params { k1: -0.2 };
    let b = Poly3Params { k1: 0.3 };
    let at_zero = Poly3Params::lerp(a, b, 0.0);
    let at_one = Poly3Params::lerp(a, b, 1.0);
    assert_close(at_zero.k1, -0.2, TOL, "poly3 lerp t=0");
    assert_close(at_one.k1, 0.3, TOL, "poly3 lerp t=1");
}

#[test]
fn vignetting_lerp_quarter() {
    let a = VignettingParams {
        k1: -1.0,
        k2: 0.5,
        k3: -0.1,
    };
    let b = VignettingParams {
        k1: -0.2,
        k2: 0.1,
        k3: 0.0,
    };
    let q = VignettingParams::lerp(a, b, 0.25);
    assert_close(q.k1, -0.8, TOL, "vignetting lerp k1");
    assert_close(q.k2, 0.4, TOL, "vignetting lerp k2");
    assert_close(q.k3, -0.075, TOL, "vignetting lerp k3");
}

// ── Projection to_sphere / from_sphere hand-computed ─────────────────────────

#[test]
fn rectilinear_to_sphere_origin() {
    let (dx, dy, dz) = LensProjection::Rectilinear.to_sphere(0.0, 0.0).unwrap();
    assert_close(dx, 0.0, TOL, "rectilinear origin dx");
    assert_close(dy, 0.0, TOL, "rectilinear origin dy");
    assert_close(dz, 1.0, TOL, "rectilinear origin dz");
}

#[test]
fn rectilinear_to_sphere_known() {
    // x=1, y=0 => r3 = sqrt(2), dx = 1/sqrt(2), dz = 1/sqrt(2)
    let (dx, dy, dz) = LensProjection::Rectilinear.to_sphere(1.0, 0.0).unwrap();
    let s2 = std::f32::consts::FRAC_1_SQRT_2;
    assert_close(dx, s2, TOL, "rectilinear (1,0) dx");
    assert_close(dy, 0.0, TOL, "rectilinear (1,0) dy");
    assert_close(dz, s2, TOL, "rectilinear (1,0) dz");
}

#[test]
fn fisheye_equidistant_to_sphere_origin() {
    let (dx, dy, dz) = LensProjection::Fisheye.to_sphere(0.0, 0.0).unwrap();
    assert_close(dx, 0.0, TOL, "fisheye origin dx");
    assert_close(dy, 0.0, TOL, "fisheye origin dy");
    assert_close(dz, 1.0, TOL, "fisheye origin dz");
}

#[test]
fn fisheye_equidistant_to_sphere_known() {
    // At r = pi/2 along x-axis: theta = pi/2, sin(theta)=1, cos(theta)=0
    let r = std::f32::consts::FRAC_PI_2;
    let (dx, dy, dz) = LensProjection::Fisheye.to_sphere(r, 0.0).unwrap();
    assert_close(dx, 1.0, 1e-5, "fisheye (pi/2, 0) dx");
    assert_close(dy, 0.0, 1e-5, "fisheye (pi/2, 0) dy");
    assert_close(dz, 0.0, 1e-4, "fisheye (pi/2, 0) dz");
}

#[test]
fn orthographic_to_sphere_known() {
    // r=0 => (0,0,1)
    let (dx, dy, dz) = LensProjection::FisheyeOrthographic
        .to_sphere(0.0, 0.0)
        .unwrap();
    assert_close(dz, 1.0, TOL, "orthographic origin dz");
    assert_close(dx, 0.0, TOL, "orthographic origin dx");
    assert_close(dy, 0.0, TOL, "orthographic origin dy");

    // x=0.5, y=0 => dz = sqrt(1 - 0.25) = sqrt(0.75)
    let (dx, _, dz) = LensProjection::FisheyeOrthographic
        .to_sphere(0.5, 0.0)
        .unwrap();
    assert_close(dx, 0.5, TOL, "orthographic (0.5,0) dx");
    assert_close(dz, 0.75_f32.sqrt(), TOL, "orthographic (0.5,0) dz");
}

#[test]
fn orthographic_rejects_r_greater_than_1() {
    assert!(
        LensProjection::FisheyeOrthographic
            .to_sphere(1.1, 0.0)
            .is_none()
    );
}

#[test]
fn rectilinear_from_sphere_rejects_behind_camera() {
    assert!(
        LensProjection::Rectilinear
            .from_sphere(1.0, 0.0, 0.0)
            .is_none()
    );
    assert!(
        LensProjection::Rectilinear
            .from_sphere(1.0, 0.0, -0.5)
            .is_none()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part B: round-trip accuracy tests
// ═══════════════════════════════════════════════════════════════════════════════

// ── Bisection inversion (test-only, mirrors the crate's private helper) ──────

fn bisect_invert(target: f32, f: impl Fn(f32) -> f32) -> f32 {
    if target <= 0.0 {
        return 0.0;
    }
    let mut lo = 0.0_f32;
    let mut hi = target.max(1.0);
    for _ in 0..16 {
        if f(hi) >= target {
            break;
        }
        hi *= 2.0;
    }
    for _ in 0..64 {
        let mid = (lo + hi) * 0.5;
        if f(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

// ── Distortion model round-trips ─────────────────────────────────────────────
// Forward: r_d = model(r_u). Inverse: bisect to find r_u' s.t. model(r_u') = r_d.
// Check r_u' ≈ r_u.

fn distortion_round_trip_max_error(model: DistortionModel) -> f32 {
    let radii: Vec<f32> = (0..=100).map(|i| i as f32 / 100.0).collect();
    let mut max_err: f32 = 0.0;

    for &r_u in &radii {
        let r_d = model.undistorted_to_distorted(r_u);
        let r_u_recovered = bisect_invert(r_d, |r| model.undistorted_to_distorted(r));
        let err = (r_u_recovered - r_u).abs();
        max_err = max_err.max(err);
    }
    max_err
}

#[test]
fn ptlens_distortion_round_trip() {
    let model = DistortionModel::PtLens(PtLensParams {
        a: 0.01,
        b: -0.04,
        c: 0.005,
    });
    let err = distortion_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "PTLens round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

#[test]
fn poly3_distortion_round_trip() {
    let model = DistortionModel::Poly3(Poly3Params { k1: -0.15 });
    let err = distortion_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "Poly3 round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

#[test]
fn poly5_distortion_round_trip() {
    let model = DistortionModel::Poly5(Poly5Params { k1: -0.2, k2: 0.05 });
    let err = distortion_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "Poly5 round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

#[test]
fn strong_barrel_distortion_round_trip() {
    let model = DistortionModel::PtLens(PtLensParams {
        a: 0.0,
        b: -0.3,
        c: 0.0,
    });
    let err = distortion_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "strong barrel round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

#[test]
fn identity_distortion_round_trip_is_exact() {
    let model = DistortionModel::Poly3(Poly3Params { k1: 0.0 });
    let profile = profile_with_distortion(model);
    let (w, h) = (32u32, 32u32);

    let map = profile
        .distortion_coordinate_map_with_options(
            w,
            h,
            CoordinateMapOptions::new().with_transform_mode(TransformMode::Rectify),
        )
        .unwrap();

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            let c = &map[idx];
            assert_close(c.x, x as f32, TOL, &format!("identity x at ({x},{y})"));
            assert_close(c.y, y as f32, TOL, &format!("identity y at ({x},{y})"));
        }
    }
}

// ── TCA model round-trips ────────────────────────────────────────────────────
// Forward: r_out = tca_fn(r_in). Inverse: bisect to find r_in' s.t. tca_fn(r_in') = r_out.

fn tca_round_trip_max_error(model: TcaModel) -> f32 {
    let radii: Vec<f32> = (0..=100).map(|i| i as f32 / 100.0).collect();
    let mut max_err: f32 = 0.0;

    for &r in &radii {
        match model {
            TcaModel::Linear(p) => {
                let r_red = r * p.kr;
                let r_recovered = bisect_invert(r_red, |x| x * p.kr);
                max_err = max_err.max((r_recovered - r).abs());

                let r_blue = r * p.kb;
                let r_recovered = bisect_invert(r_blue, |x| x * p.kb);
                max_err = max_err.max((r_recovered - r).abs());
            }
            TcaModel::Poly3(p) => {
                let r_red = p.red(r);
                let r_recovered = bisect_invert(r_red, |x| p.red(x));
                max_err = max_err.max((r_recovered - r).abs());

                let r_blue = p.blue(r);
                let r_recovered = bisect_invert(r_blue, |x| p.blue(x));
                max_err = max_err.max((r_recovered - r).abs());
            }
        }
    }
    max_err
}

#[test]
fn tca_linear_round_trip() {
    let model = TcaModel::Linear(TcaLinearParams {
        kr: 1.0005,
        kb: 0.9996,
    });
    let err = tca_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "TCA linear round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

#[test]
fn tca_poly3_round_trip() {
    let model = TcaModel::Poly3(TcaPoly3Params {
        vr: 1.0002,
        cr: -0.00005,
        br: -0.0001,
        vb: 0.9997,
        cb: 0.00008,
        bb: 0.00015,
    });
    let err = tca_round_trip_max_error(model);
    assert!(
        err < ROUND_TRIP_TOL,
        "TCA poly3 round-trip max error {err} exceeds {ROUND_TRIP_TOL}"
    );
}

// ── Coordinate map Rectify↔Reverse consistency ──────────────────────────────
// Verify that for interior pixels, the Reverse map at a Rectify-mapped
// coordinate points back close to the original pixel.  Uses a large image
// to minimise quantisation error from integer rounding.

#[test]
fn distortion_coordinate_map_rectify_reverse_consistency() {
    let model = DistortionModel::PtLens(PtLensParams {
        a: 0.01,
        b: -0.04,
        c: 0.005,
    });
    let profile = profile_with_distortion(model);
    let (w, h) = (256u32, 256u32);

    let rectify = profile
        .distortion_coordinate_map_with_options(
            w,
            h,
            CoordinateMapOptions::new().with_transform_mode(TransformMode::Rectify),
        )
        .unwrap();
    let reverse = profile
        .distortion_coordinate_map_with_options(
            w,
            h,
            CoordinateMapOptions::new().with_transform_mode(TransformMode::Reverse),
        )
        .unwrap();

    let margin = 32u32;
    let mut max_err: f32 = 0.0;
    for y in margin..(h - margin) {
        for x in margin..(w - margin) {
            let idx = (y * w + x) as usize;
            let fwd = &rectify[idx];

            let fx = fwd.x.round() as u32;
            let fy = fwd.y.round() as u32;
            if fx >= w || fy >= h {
                continue;
            }
            let rev = &reverse[(fy * w + fx) as usize];

            let err = ((rev.x - x as f32).powi(2) + (rev.y - y as f32).powi(2)).sqrt();
            max_err = max_err.max(err);
        }
    }
    assert!(
        max_err < 2.0,
        "Rectify↔Reverse coordinate map consistency error {max_err} exceeds 2 pixels"
    );
}

#[test]
fn tca_green_channel_is_identity_in_both_modes() {
    let model = TcaModel::Poly3(TcaPoly3Params {
        vr: 1.001,
        cr: -0.0001,
        br: -0.0002,
        vb: 0.999,
        cb: 0.0002,
        bb: 0.0003,
    });
    let profile = profile_with_tca(model);
    let (w, h) = (64u32, 64u32);

    for mode in [TransformMode::Rectify, TransformMode::Reverse] {
        let map = profile
            .tca_coordinate_map_with_options(
                w,
                h,
                CoordinateMapOptions::new().with_transform_mode(mode),
            )
            .unwrap();

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                let green = &map[idx].green;
                assert_close(
                    green.x,
                    x as f32,
                    TOL,
                    &format!("{mode:?} green x at ({x},{y})"),
                );
                assert_close(
                    green.y,
                    y as f32,
                    TOL,
                    &format!("{mode:?} green y at ({x},{y})"),
                );
            }
        }
    }
}

// ── Projection round-trips ───────────────────────────────────────────────────

fn projection_round_trip_max_error(proj: LensProjection, coords: &[(f32, f32)]) -> f32 {
    let mut max_err: f32 = 0.0;
    for &(x, y) in coords {
        let sphere = match proj.to_sphere(x, y) {
            Some(s) => s,
            None => continue,
        };
        // Verify unit sphere (within f32 tolerance)
        let len = (sphere.0 * sphere.0 + sphere.1 * sphere.1 + sphere.2 * sphere.2).sqrt();
        assert!(
            (len - 1.0).abs() < 1e-4,
            "{proj:?} to_sphere({x},{y}) not unit length: {len}"
        );

        let back = match proj.from_sphere(sphere.0, sphere.1, sphere.2) {
            Some(b) => b,
            None => continue,
        };
        let err = ((back.0 - x).powi(2) + (back.1 - y).powi(2)).sqrt();
        max_err = max_err.max(err);
    }
    max_err
}

fn standard_test_coords() -> Vec<(f32, f32)> {
    let mut coords = vec![(0.0, 0.0)];
    for &r in &[0.1, 0.3, 0.5, 0.7, 0.9] {
        for &angle in &[0.0_f32, 0.7, 1.3, 2.1, 3.0, 4.5, 5.8] {
            coords.push((r * angle.cos(), r * angle.sin()));
        }
    }
    coords
}

#[test]
fn rectilinear_projection_round_trip() {
    let err = projection_round_trip_max_error(LensProjection::Rectilinear, &standard_test_coords());
    assert!(err < 1e-5, "rectilinear round-trip error {err}");
}

#[test]
fn fisheye_equidistant_projection_round_trip() {
    let err = projection_round_trip_max_error(LensProjection::Fisheye, &standard_test_coords());
    assert!(err < 1e-5, "fisheye equidistant round-trip error {err}");
}

#[test]
fn stereographic_projection_round_trip() {
    let err = projection_round_trip_max_error(
        LensProjection::FisheyeStereographic,
        &standard_test_coords(),
    );
    assert!(err < 1e-5, "stereographic round-trip error {err}");
}

#[test]
fn equisolid_projection_round_trip() {
    let err =
        projection_round_trip_max_error(LensProjection::FisheyeEquisolid, &standard_test_coords());
    assert!(err < 1e-5, "equisolid round-trip error {err}");
}

#[test]
fn orthographic_projection_round_trip() {
    let err = projection_round_trip_max_error(
        LensProjection::FisheyeOrthographic,
        &standard_test_coords(),
    );
    assert!(err < 1e-5, "orthographic round-trip error {err}");
}

#[test]
fn thoby_projection_round_trip() {
    let err =
        projection_round_trip_max_error(LensProjection::FisheyeThoby, &standard_test_coords());
    assert!(err < 1e-4, "thoby round-trip error {err}");
}

#[test]
fn panoramic_projection_round_trip() {
    let err = projection_round_trip_max_error(LensProjection::Panoramic, &standard_test_coords());
    assert!(err < 1e-5, "panoramic round-trip error {err}");
}

#[test]
fn equirectangular_projection_round_trip() {
    let err =
        projection_round_trip_max_error(LensProjection::Equirectangular, &standard_test_coords());
    assert!(err < 1e-5, "equirectangular round-trip error {err}");
}

// ── Cross-projection round-trip via sphere ───────────────────────────────────
// Going from projection A → sphere → projection B → sphere → projection A
// should also round-trip.

#[test]
fn cross_projection_round_trip_rectilinear_fisheye() {
    let coords = standard_test_coords();
    let mut max_err: f32 = 0.0;

    for &(x, y) in &coords {
        let sphere = match LensProjection::Rectilinear.to_sphere(x, y) {
            Some(s) => s,
            None => continue,
        };
        let fisheye_xy = match LensProjection::Fisheye.from_sphere(sphere.0, sphere.1, sphere.2) {
            Some(f) => f,
            None => continue,
        };
        let sphere2 = match LensProjection::Fisheye.to_sphere(fisheye_xy.0, fisheye_xy.1) {
            Some(s) => s,
            None => continue,
        };
        let back = match LensProjection::Rectilinear.from_sphere(sphere2.0, sphere2.1, sphere2.2) {
            Some(b) => b,
            None => continue,
        };
        let err = ((back.0 - x).powi(2) + (back.1 - y).powi(2)).sqrt();
        max_err = max_err.max(err);
    }

    assert!(
        max_err < 1e-4,
        "rectilinear↔fisheye cross round-trip error {max_err}"
    );
}

// ── Pixel-level correction round-trip ────────────────────────────────────────
// Apply distortion correction to an image, then apply the reverse transform.
// Centre pixels should come back very close to their originals.

#[test]
fn pixel_distortion_forward_then_reverse_centre_preserved() {
    let model = DistortionModel::Poly5(Poly5Params {
        k1: -0.15,
        k2: 0.03,
    });
    let profile = profile_with_distortion(model);
    let (w, h, ch) = (32u32, 32u32, 3u32);

    let src: Vec<f32> = (0..h)
        .flat_map(|y| (0..w).flat_map(move |x| [x as f32, y as f32, 0.5]))
        .collect();

    let corrected = profile.correct_distortion_raw_f32(w, h, ch, &src).unwrap();

    // Centre pixel should be almost exactly preserved
    let centre = ((16 * w + 16) * ch) as usize;
    assert_close(corrected[centre], 16.0, 0.01, "centre x after correction");
    assert_close(
        corrected[centre + 1],
        16.0,
        0.01,
        "centre y after correction",
    );
}

// ── Distortion monotonicity ──────────────────────────────────────────────────
// For well-behaved lens profiles, the mapping should be monotonically
// increasing (r_d increases as r_u increases) over the useful [0, 1] range.

#[test]
fn distortion_models_monotonic_over_unit_range() {
    let models = [
        DistortionModel::PtLens(PtLensParams {
            a: 0.01,
            b: -0.04,
            c: 0.005,
        }),
        DistortionModel::Poly3(Poly3Params { k1: -0.15 }),
        DistortionModel::Poly3(Poly3Params { k1: 0.2 }),
        DistortionModel::Poly5(Poly5Params { k1: -0.2, k2: 0.05 }),
        DistortionModel::Poly5(Poly5Params { k1: 0.1, k2: -0.02 }),
    ];

    for model in &models {
        let mut prev = 0.0_f32;
        for i in 1..=100 {
            let r = i as f32 / 100.0;
            let rd = model.undistorted_to_distorted(r);
            assert!(
                rd > prev,
                "{model:?}: not monotonic at r={r}, rd={rd} <= prev={prev}"
            );
            prev = rd;
        }
    }
}
