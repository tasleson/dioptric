use dioptric::{CorrectionProfile, Database};

// ── Database loading ───────────────────────────────────────────────────────────

#[test]
fn bundled_db_has_cameras_and_lenses() {
    let db = Database::bundled();
    assert!(db.cameras().len() > 10, "expected many cameras");
    assert!(db.lenses().len() > 10, "expected many lenses");
}

#[test]
fn find_canon_camera() {
    let db = Database::bundled();
    let cam = db.find_camera("Canon", "EOS 5D Mark III");
    assert!(cam.is_some(), "Canon EOS 5D Mark III must be findable");
    let cam = cam.unwrap();
    assert!(
        (cam.crop_factor() - 1.0).abs() < 0.05,
        "5D Mark III crop factor should be ~1.0"
    );
}

#[test]
fn find_canon_lens() {
    let db = Database::bundled();
    let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM");
    assert!(
        lens.is_some(),
        "Canon EF 24-70mm f/2.8L II USM must be findable"
    );
}

#[test]
fn find_camera_case_insensitive() {
    let db = Database::bundled();
    assert!(db.find_camera("canon", "5d mark iii").is_some());
    assert!(db.find_camera("CANON", "EOS 5D").is_some());
}

#[test]
fn find_missing_camera_returns_none() {
    let db = Database::bundled();
    assert!(
        db.find_camera("Imaginary Corp", "Nonexistent 9000")
            .is_none()
    );
}

// ── CorrectionProfile construction ────────────────────────────────────────────

#[test]
fn profile_for_known_lens() {
    let db = Database::bundled();
    let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
    let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
    let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();
    // This lens has distortion calibration
    assert!(profile.distortion.is_some(), "expected distortion data");
}

#[test]
fn profile_invalid_focal_returns_error() {
    let db = Database::bundled();
    let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
    let result = CorrectionProfile::new(lens, 1.0, -10.0, 4.0, 10.0);
    assert!(result.is_err());
}

#[test]
fn profile_invalid_aperture_returns_error() {
    let db = Database::bundled();
    let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
    let result = CorrectionProfile::new(lens, 1.0, 35.0, 0.0, 10.0);
    assert!(result.is_err());
}

#[test]
fn profile_uses_focus_distance_for_vignetting() {
    use dioptric::database::{Calibration, Lens, VignettingEntry};
    use dioptric::models::VignettingParams;

    let lens = Lens {
        maker: "Test".into(),
        model: "Distance-sensitive vignette".into(),
        mounts: vec![],
        crop_factor: Some(1.0),
        calibration: Calibration {
            distortions: vec![],
            tcas: vec![],
            vignettings: vec![
                VignettingEntry {
                    focal: 35.0,
                    aperture: 2.0,
                    distance: 1.0,
                    params: VignettingParams {
                        k1: -0.1,
                        k2: 0.0,
                        k3: 0.0,
                    },
                },
                VignettingEntry {
                    focal: 35.0,
                    aperture: 2.0,
                    distance: 10.0,
                    params: VignettingParams {
                        k1: -0.4,
                        k2: 0.0,
                        k3: 0.0,
                    },
                },
            ],
        },
    };

    let near = CorrectionProfile::new(&lens, 1.0, 35.0, 2.0, 1.0).unwrap();
    let far = CorrectionProfile::new(&lens, 1.0, 35.0, 2.0, 10.0).unwrap();
    let mid = CorrectionProfile::new(&lens, 1.0, 35.0, 2.0, 5.5).unwrap();

    assert_ne!(near.vignetting, far.vignetting);
    assert_eq!(near.vignetting.unwrap().k1, -0.1);
    assert_eq!(far.vignetting.unwrap().k1, -0.4);
    assert!((mid.vignetting.unwrap().k1 + 0.25).abs() < 1e-6);
}

// ── Rasterlab-shaped raw-buffer integration ─────────────────────────────────

#[test]
fn rasterlab_image_round_trip_uses_linear_raw_buffer_without_extra_pixel_copy() {
    struct RasterlabMetadata {
        camera_make: String,
        camera_model: String,
        lens_make: Option<String>,
        lens_model: String,
        focal_length_mm: f32,
        aperture: f32,
        subject_distance_m: Option<f32>,
    }

    struct RasterlabImage {
        width: u32,
        height: u32,
        channels: u32,
        metadata: RasterlabMetadata,
        pixels: Vec<f32>,
    }

    impl RasterlabImage {
        fn corrected_with(self, pixels: Vec<f32>) -> Self {
            Self { pixels, ..self }
        }
    }

    let db = Database::bundled();
    let metadata = RasterlabMetadata {
        camera_make: "Canon".into(),
        camera_model: "EOS 5D Mark III".into(),
        lens_make: Some("Canon".into()),
        lens_model: "EF 24-70mm f/2.8L II USM".into(),
        focal_length_mm: 35.0,
        aperture: 4.0,
        subject_distance_m: Some(10.0),
    };

    let width = 32;
    let height = 24;
    let channels = 3;
    let pixels: Vec<f32> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width) as f32 / width as f32;
            let y = (i / width) as f32 / height as f32;
            [x, y, 0.5]
        })
        .collect();
    let input = RasterlabImage {
        width,
        height,
        channels,
        metadata,
        pixels,
    };

    let camera = db
        .find_camera(&input.metadata.camera_make, &input.metadata.camera_model)
        .unwrap();
    let lens = match input.metadata.lens_make.as_deref() {
        Some(lens_make) => db.find_lens(lens_make, &input.metadata.lens_model).unwrap(),
        None => db.find_lens_by_name(&input.metadata.lens_model).unwrap(),
    };
    let profile = CorrectionProfile::new(
        lens,
        camera.crop_factor(),
        input.metadata.focal_length_mm,
        input.metadata.aperture,
        input.metadata.subject_distance_m.unwrap_or(1000.0),
    )
    .unwrap();

    let corrected_pixels = profile
        .correct_all_raw_f32(input.width, input.height, input.channels, &input.pixels)
        .unwrap();
    let corrected_ptr = corrected_pixels.as_ptr();
    let output = input.corrected_with(corrected_pixels);

    assert_eq!(output.width, width);
    assert_eq!(output.height, height);
    assert_eq!(output.channels, channels);
    assert_eq!(output.metadata.lens_model, "EF 24-70mm f/2.8L II USM");
    assert_eq!(output.pixels.len(), (width * height * channels) as usize);
    assert_eq!(
        output.pixels.as_ptr(),
        corrected_ptr,
        "corrected pixel buffer should be moved into the output image, not cloned"
    );
}

#[test]
fn poly5_distortion_pipeline_warps_raw_f32_pixels() {
    use dioptric::database::{Calibration, DistortionEntry, Lens};
    use dioptric::models::{DistortionModel, Poly5Params};

    let lens = Lens {
        maker: "Test".into(),
        model: "Poly5 pipeline".into(),
        mounts: vec![],
        crop_factor: Some(1.0),
        calibration: Calibration {
            distortions: vec![DistortionEntry {
                focal: 35.0,
                model: DistortionModel::Poly5(Poly5Params { k1: -0.2, k2: 0.04 }),
            }],
            tcas: vec![],
            vignettings: vec![],
        },
    };
    let profile = CorrectionProfile::new(&lens, 1.0, 35.0, 4.0, 10.0).unwrap();
    assert!(matches!(
        profile.distortion,
        Some(DistortionModel::Poly5(_))
    ));

    let (width, height, channels) = (8u32, 8u32, 3u32);
    let src: Vec<f32> = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [x as f32, y as f32, 0.25]))
        .collect();

    let corrected = profile
        .correct_distortion_raw_f32(width, height, channels, &src)
        .unwrap();

    let corner = &corrected[0..3];
    assert!(
        (0.55..0.75).contains(&corner[0]),
        "top-left red channel should sample inward, got {}",
        corner[0]
    );
    assert!(
        (0.55..0.75).contains(&corner[1]),
        "top-left green channel should sample inward, got {}",
        corner[1]
    );
    assert!(
        (corner[2] - 0.25).abs() < 1e-6,
        "constant blue channel should be preserved, got {}",
        corner[2]
    );

    let centre_idx = ((4 * width + 4) * channels) as usize;
    assert_eq!(&corrected[centre_idx..centre_idx + 3], &[4.0, 4.0, 0.25]);
}

// ── Correction smoke tests (DynamicImage API) ─────────────────────────────────

#[cfg(feature = "image")]
mod image_tests {
    use super::*;
    use image::{DynamicImage, GenericImageView, RgbImage, RgbaImage};

    fn test_image(w: u32, h: u32, rgb: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb(rgb)))
    }

    #[test]
    fn correct_all_preserves_dimensions() {
        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();

        let img = test_image(64, 48, [180, 120, 80]);
        let corrected = profile.correct_all(&img).unwrap();
        assert_eq!(corrected.dimensions(), img.dimensions());
    }

    #[test]
    fn correct_distortion_preserves_dimensions() {
        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();

        let img = test_image(64, 48, [128, 64, 32]);
        let result = profile.correct_distortion(&img).unwrap();
        assert_eq!(result.dimensions(), img.dimensions());
    }

    #[test]
    fn correct_tca_preserves_dimensions() {
        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();

        let img = test_image(64, 48, [200, 150, 100]);
        let result = profile.correct_tca(&img).unwrap();
        assert_eq!(result.dimensions(), img.dimensions());
    }

    #[test]
    fn vignetting_brightens_uniform_white_at_centre() {
        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 24.0, 2.8, 10.0).unwrap();

        if profile.vignetting.is_none() {
            return;
        }

        let mut img = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, image::Rgb([128u8; 3])));
        let before_centre = img.to_rgb8().get_pixel(32, 32).0;
        profile.correct_vignetting(&mut img).unwrap();
        let after_centre = img.to_rgb8().get_pixel(32, 32).0;
        for ch in 0..3 {
            let diff = (before_centre[ch] as i32 - after_centre[ch] as i32).abs();
            assert!(
                diff <= 5,
                "centre channel {ch}: before={} after={}",
                before_centre[ch],
                after_centre[ch]
            );
        }
    }

    #[test]
    fn correct_all_preserves_rgba_alpha() {
        use image::Rgba;

        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();

        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 24, Rgba([10, 20, 30, 77])));
        let corrected = profile.correct_all(&img).unwrap().to_rgba8();
        assert_eq!(corrected.dimensions(), (32, 24));
        assert_eq!(corrected.get_pixel(16, 12)[3], 77);
    }

    #[test]
    fn grayscale_inputs_return_an_error() {
        let db = Database::bundled();
        let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
        let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
        let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();

        let img =
            DynamicImage::ImageLuma8(image::GrayImage::from_pixel(16, 16, image::Luma([128])));
        let err = profile.correct_all(&img).unwrap_err();
        assert!(matches!(err, dioptric::Error::UnsupportedImageFormat(_)));
    }

    #[test]
    fn identity_distortion_unchanged() {
        use dioptric::database::{Calibration, DistortionEntry, Lens};
        use dioptric::models::{DistortionModel, Poly3Params};

        let lens = Lens {
            maker: "Test".into(),
            model: "Identity".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![DistortionEntry {
                    focal: 50.0,
                    model: DistortionModel::Poly3(Poly3Params { k1: 0.0 }),
                }],
                tcas: vec![],
                vignettings: vec![],
            },
        };

        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();
        let img = test_image(32, 32, [100, 150, 200]);
        let result = profile.correct_distortion(&img).unwrap();

        let src = img.to_rgb8();
        let dst = result.to_rgb8();
        for y in 0..32 {
            for x in 0..32 {
                let s = src.get_pixel(x, y);
                let d = dst.get_pixel(x, y);
                for ch in 0..3 {
                    let diff = (s[ch] as i32 - d[ch] as i32).abs();
                    assert!(
                        diff <= 1,
                        "identity distortion changed pixel ({x},{y}) ch {ch}: {s:?} vs {d:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn correct_all_applies_tca() {
        use dioptric::database::{Calibration, DistortionEntry, Lens, TcaEntry};
        use dioptric::models::{DistortionModel, Poly3Params, TcaLinearParams, TcaModel};

        let lens = Lens {
            maker: "Test".into(),
            model: "TCA test".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![DistortionEntry {
                    focal: 50.0,
                    model: DistortionModel::Poly3(Poly3Params { k1: 0.0 }),
                }],
                tcas: vec![TcaEntry {
                    focal: 50.0,
                    model: TcaModel::Linear(TcaLinearParams { kr: 1.05, kb: 0.95 }),
                }],
                vignettings: vec![],
            },
        };

        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let img = test_image(64, 64, [200, 150, 100]);
        let dist_only = profile.correct_distortion(&img).unwrap().to_rgb8();
        let all = profile.correct_all(&img).unwrap().to_rgb8();

        let corner_dist = dist_only.get_pixel(0, 0);
        let corner_all = all.get_pixel(0, 0);
        assert_ne!(
            corner_dist, corner_all,
            "correct_all must differ from correct_distortion when TCA is present"
        );
    }
}
