//! Image-level warp and correction pipeline.
//!
//! [`CorrectionProfile`] bundles a resolved set of correction parameters for a
//! specific lens, camera, focal length, aperture, and focus distance.  The
//! three correction operations can be applied individually or all at once via
//! [`CorrectionProfile::correct_all`].

use image::{DynamicImage, GrayImage, Luma, RgbImage, Rgba, RgbaImage};

use crate::database::{Lens, interpolate_distortion, interpolate_tca, interpolate_vignetting};
use crate::error::{Error, Result};
use crate::models::{DistortionModel, TcaModel, VignettingParams, linear_to_srgb, srgb_to_linear};

// ── CorrectionProfile ─────────────────────────────────────────────────────────

/// A resolved set of lens correction parameters for a single capture.
///
/// Build one with [`CorrectionProfile::new`], then apply corrections using the
/// `correct_*` family of methods.
///
/// # Example
///
/// ```
/// use dioptric::{Database, CorrectionProfile};
///
/// let db = Database::bundled();
/// let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
/// let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
/// let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct CorrectionProfile {
    /// Resolved distortion model (if calibration data is available).
    pub distortion: Option<DistortionModel>,
    /// Resolved TCA model (if calibration data is available).
    pub tca: Option<TcaModel>,
    /// Resolved vignetting parameters (if calibration data is available).
    pub vignetting: Option<VignettingParams>,
    /// Sensor crop factor, used to compute normalised coordinates.
    crop_factor: f32,
}

impl CorrectionProfile {
    /// Build a correction profile from a lens, camera crop factor, focal length,
    /// aperture (f-number), and focus distance (metres).
    ///
    /// Returns [`Error::InvalidParameter`] if any of the numeric parameters
    /// are non-finite or non-positive.
    ///
    /// # Example
    ///
    /// ```
    /// use dioptric::{Database, CorrectionProfile};
    ///
    /// let db = Database::bundled();
    /// let lens = db.find_lens("Canon", "EF 24-70").unwrap();
    /// let camera = db.find_camera("Canon", "5D Mark III").unwrap();
    /// let profile = CorrectionProfile::new(lens, camera.crop_factor(), 24.0, 2.8, 1000.0).unwrap();
    /// assert!(profile.distortion.is_some());
    /// ```
    pub fn new(
        lens: &Lens,
        crop_factor: f32,
        focal: f32,
        aperture: f32,
        distance: f32,
    ) -> Result<Self> {
        if !focal.is_finite() || focal <= 0.0 {
            return Err(Error::InvalidParameter(format!(
                "focal length {focal} is invalid"
            )));
        }
        if !aperture.is_finite() || aperture <= 0.0 {
            return Err(Error::InvalidParameter(format!(
                "aperture {aperture} is invalid"
            )));
        }
        if !distance.is_finite() || distance <= 0.0 {
            return Err(Error::InvalidParameter(format!(
                "distance {distance} is invalid"
            )));
        }
        if !crop_factor.is_finite() || crop_factor <= 0.0 {
            return Err(Error::InvalidParameter(format!(
                "crop_factor {crop_factor} is invalid"
            )));
        }

        let cal = &lens.calibration;

        // Sort entries by focal length for interpolation
        let mut dist_entries = cal.distortions.clone();
        dist_entries.sort_by(|a, b| a.focal.total_cmp(&b.focal));

        let mut tca_entries = cal.tcas.clone();
        tca_entries.sort_by(|a, b| a.focal.total_cmp(&b.focal));

        let mut vig_entries = cal.vignetings.clone();
        vig_entries.sort_by(|a, b| a.focal.total_cmp(&b.focal));

        let distortion = interpolate_distortion(&dist_entries, focal);
        let tca = interpolate_tca(&tca_entries, focal);
        let vignetting = interpolate_vignetting(&vig_entries, focal, aperture, distance);

        Ok(Self {
            distortion,
            tca,
            vignetting,
            crop_factor,
        })
    }

    /// Apply distortion, TCA, and vignetting corrections in sequence.
    ///
    /// Distortion and TCA are applied as image warps; vignetting is applied
    /// in-place on the result.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    /// `Rgba8` inputs preserve alpha; other image formats return
    /// [`Error::UnsupportedImageFormat`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dioptric::{Database, CorrectionProfile};
    /// use image::DynamicImage;
    ///
    /// let db = Database::bundled();
    /// let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM").unwrap();
    /// let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
    /// let profile = CorrectionProfile::new(lens, camera.crop_factor(), 35.0, 4.0, 10.0).unwrap();
    ///
    /// let img = image::open("photo.jpg").unwrap();
    /// let corrected = profile.correct_all(&img).unwrap();
    /// ```
    pub fn correct_all(&self, img: &DynamicImage) -> Result<DynamicImage> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                let after_dist = self.warp_distortion_rgb(rgb)?;
                let mut after_tca = self.warp_tca_rgb(&after_dist)?;
                self.correct_vignetting_inplace(&mut after_tca);
                Ok(DynamicImage::ImageRgb8(after_tca))
            }
            DynamicImage::ImageRgba8(rgba) => {
                let rgb = rgb_from_rgba(rgba);
                let alpha = alpha_from_rgba(rgba);
                let after_dist = self.warp_distortion_rgb(&rgb)?;
                let warped_alpha = self.warp_distortion_gray(&alpha)?;
                let mut after_tca = self.warp_tca_rgb(&after_dist)?;
                self.correct_vignetting_inplace(&mut after_tca);
                Ok(DynamicImage::ImageRgba8(rgba_from_rgb_and_alpha(
                    &after_tca,
                    &warped_alpha,
                )))
            }
            _ => Err(unsupported_image_format(img)),
        }
    }

    /// Apply distortion correction only, returning a new image.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    /// `Rgba8` inputs preserve alpha; other image formats return
    /// [`Error::UnsupportedImageFormat`].
    pub fn correct_distortion(&self, img: &DynamicImage) -> Result<DynamicImage> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                let result = if self.distortion.is_some() {
                    self.warp_distortion_rgb(rgb)?
                } else {
                    rgb.clone()
                };
                Ok(DynamicImage::ImageRgb8(result))
            }
            DynamicImage::ImageRgba8(rgba) => {
                let rgb = rgb_from_rgba(rgba);
                let alpha = alpha_from_rgba(rgba);
                let warped_rgb = if self.distortion.is_some() {
                    self.warp_distortion_rgb(&rgb)?
                } else {
                    rgb
                };
                let warped_alpha = if self.distortion.is_some() {
                    self.warp_distortion_gray(&alpha)?
                } else {
                    alpha
                };
                Ok(DynamicImage::ImageRgba8(rgba_from_rgb_and_alpha(
                    &warped_rgb,
                    &warped_alpha,
                )))
            }
            _ => Err(unsupported_image_format(img)),
        }
    }

    /// Apply vignetting correction in-place.
    ///
    /// Converts sRGB → linear, scales each pixel, then converts back.
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    /// `Rgba8` inputs preserve alpha; other image formats return
    /// [`Error::UnsupportedImageFormat`].
    pub fn correct_vignetting(&self, img: &mut DynamicImage) -> Result<()> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                self.correct_vignetting_inplace(rgb);
                Ok(())
            }
            DynamicImage::ImageRgba8(rgba) => {
                let mut rgb = rgb_from_rgba(rgba);
                self.correct_vignetting_inplace(&mut rgb);
                let alpha = alpha_from_rgba(rgba);
                *img = DynamicImage::ImageRgba8(rgba_from_rgb_and_alpha(&rgb, &alpha));
                Ok(())
            }
            _ => Err(unsupported_image_format(img)),
        }
    }

    /// Apply TCA (chromatic aberration) correction only, returning a new image.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    /// `Rgba8` inputs preserve alpha; other image formats return
    /// [`Error::UnsupportedImageFormat`].
    pub fn correct_tca(&self, img: &DynamicImage) -> Result<DynamicImage> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                let result = if self.tca.is_some() {
                    self.warp_tca_rgb(rgb)?
                } else {
                    rgb.clone()
                };
                Ok(DynamicImage::ImageRgb8(result))
            }
            DynamicImage::ImageRgba8(rgba) => {
                let rgb = rgb_from_rgba(rgba);
                let result = if self.tca.is_some() {
                    self.warp_tca_rgb(&rgb)?
                } else {
                    rgb
                };
                Ok(DynamicImage::ImageRgba8(rgba_from_rgb_and_alpha(
                    &result,
                    &alpha_from_rgba(rgba),
                )))
            }
            _ => Err(unsupported_image_format(img)),
        }
    }

    // ── internal helpers ─────────────────────────────────────────────────────

    /// Warp image for distortion correction.
    fn warp_distortion_rgb(&self, src: &RgbImage) -> Result<RgbImage> {
        let model = match self.distortion {
            Some(m) => m,
            None => return Ok(src.clone()),
        };

        let (w, h) = src.dimensions();
        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);

        let cx = wf * 0.5;
        let cy = hf * 0.5;

        let mut dst = RgbImage::new(w, h);
        for py in 0..h {
            for px in 0..w {
                // Normalise output pixel
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r_u = (xn * xn + yn * yn).sqrt();

                // Map to distorted radius
                let r_d = model.undistorted_to_distorted(r_u);
                let scale = if r_u > 1e-8 { r_d / r_u } else { 1.0 };

                let src_x = xn * scale * norm + cx;
                let src_y = yn * scale * norm + cy;

                let pixel = bilinear_sample(src, src_x, src_y);
                dst.put_pixel(px, py, pixel);
            }
        }
        Ok(dst)
    }

    /// Warp image for TCA correction (separate per-channel sampling).
    fn warp_tca_rgb(&self, src: &RgbImage) -> Result<RgbImage> {
        let tca = match self.tca {
            Some(m) => m,
            None => return Ok(src.clone()),
        };

        let (w, h) = src.dimensions();
        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;

        let mut dst = RgbImage::new(w, h);
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();

                let (scale_r, scale_b) = tca.channel_radii(r);

                // Red channel
                let rx = xn * scale_r * norm + cx;
                let ry = yn * scale_r * norm + cy;
                let red = bilinear_sample_channel(src, rx, ry, 0);

                // Green channel — unchanged
                let src_pixel = bilinear_sample(src, px as f32, py as f32);
                let green = src_pixel[1];

                // Blue channel
                let bx = xn * scale_b * norm + cx;
                let by = yn * scale_b * norm + cy;
                let blue = bilinear_sample_channel(src, bx, by, 2);

                dst.put_pixel(px, py, image::Rgb([red, green, blue]));
            }
        }
        Ok(dst)
    }

    /// Warp a single-channel image with the same distortion mapping used for RGB.
    fn warp_distortion_gray(&self, src: &GrayImage) -> Result<GrayImage> {
        let model = match self.distortion {
            Some(m) => m,
            None => return Ok(src.clone()),
        };

        let (w, h) = src.dimensions();
        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);

        let cx = wf * 0.5;
        let cy = hf * 0.5;

        let mut dst = GrayImage::new(w, h);
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r_u = (xn * xn + yn * yn).sqrt();

                let r_d = model.undistorted_to_distorted(r_u);
                let scale = if r_u > 1e-8 { r_d / r_u } else { 1.0 };

                let src_x = xn * scale * norm + cx;
                let src_y = yn * scale * norm + cy;

                let pixel = bilinear_sample_gray(src, src_x, src_y);
                dst.put_pixel(px, py, pixel);
            }
        }
        Ok(dst)
    }

    fn correct_vignetting_inplace(&self, img: &mut RgbImage) {
        let params = match self.vignetting {
            Some(p) => p,
            None => return,
        };

        let (w, h) = img.dimensions();
        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;

        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();
                let factor = params.factor(r);

                let pixel = img.get_pixel_mut(px, py);
                for ch in 0..3 {
                    let linear = srgb_to_linear(pixel[ch]);
                    pixel[ch] = linear_to_srgb(linear * factor);
                }
            }
        }
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Compute the normalisation factor (half image diagonal in pixels) that maps
/// pixel coordinates to the lensfun normalised coordinate system.
///
/// At the full-frame 36×24mm sensor the diagonal is 43.27 mm, so for a sensor
/// with crop factor `cf` the actual diagonal is `43.27/cf` mm.  However, since
/// the database calibration was done on actual pixel images we use the pixel
/// diagonal directly:
///
/// `norm = sqrt(w² + h²) / 2`
#[inline]
fn normalisation_factor(w: f32, h: f32, _crop_factor: f32) -> f32 {
    // lensfun normalises by the half-diagonal of the image in pixels.
    (w * w + h * h).sqrt() * 0.5
}

// ── Bilinear interpolation ────────────────────────────────────────────────────

/// Sample a pixel from `src` at floating-point coordinates using bilinear
/// interpolation.  Out-of-bounds coordinates return black.
fn bilinear_sample(src: &RgbImage, x: f32, y: f32) -> image::Rgb<u8> {
    let (w, h) = src.dimensions();
    let (w, h) = (w as i32, h as i32);

    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_pixel_clamped(src, x0, y0, w, h);
    let p10 = get_pixel_clamped(src, x0 + 1, y0, w, h);
    let p01 = get_pixel_clamped(src, x0, y0 + 1, w, h);
    let p11 = get_pixel_clamped(src, x0 + 1, y0 + 1, w, h);

    let r = bilerp(p00[0], p10[0], p01[0], p11[0], tx, ty);
    let g = bilerp(p00[1], p10[1], p01[1], p11[1], tx, ty);
    let b = bilerp(p00[2], p10[2], p01[2], p11[2], tx, ty);

    image::Rgb([r, g, b])
}

/// Sample a single channel from `src` at floating-point coordinates using
/// bilinear interpolation.  Out-of-bounds coordinates return 0.
fn bilinear_sample_channel(src: &RgbImage, x: f32, y: f32, channel: usize) -> u8 {
    let (w, h) = src.dimensions();
    let (w, h) = (w as i32, h as i32);

    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_pixel_clamped(src, x0, y0, w, h)[channel];
    let p10 = get_pixel_clamped(src, x0 + 1, y0, w, h)[channel];
    let p01 = get_pixel_clamped(src, x0, y0 + 1, w, h)[channel];
    let p11 = get_pixel_clamped(src, x0 + 1, y0 + 1, w, h)[channel];

    bilerp(p00, p10, p01, p11, tx, ty)
}

/// Sample a grayscale pixel from `src` at floating-point coordinates using
/// bilinear interpolation. Out-of-bounds coordinates return black.
fn bilinear_sample_gray(src: &GrayImage, x: f32, y: f32) -> Luma<u8> {
    let (w, h) = src.dimensions();
    let (w, h) = (w as i32, h as i32);

    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_gray_pixel_clamped(src, x0, y0, w, h)[0];
    let p10 = get_gray_pixel_clamped(src, x0 + 1, y0, w, h)[0];
    let p01 = get_gray_pixel_clamped(src, x0, y0 + 1, w, h)[0];
    let p11 = get_gray_pixel_clamped(src, x0 + 1, y0 + 1, w, h)[0];

    Luma([bilerp(p00, p10, p01, p11, tx, ty)])
}

/// Fetch a pixel, returning black for out-of-bounds coordinates.
#[inline]
fn get_pixel_clamped(src: &RgbImage, x: i32, y: i32, w: i32, h: i32) -> image::Rgb<u8> {
    if x < 0 || y < 0 || x >= w || y >= h {
        image::Rgb([0, 0, 0])
    } else {
        *src.get_pixel(x as u32, y as u32)
    }
}

/// Fetch a grayscale pixel, returning black for out-of-bounds coordinates.
#[inline]
fn get_gray_pixel_clamped(src: &GrayImage, x: i32, y: i32, w: i32, h: i32) -> Luma<u8> {
    if x < 0 || y < 0 || x >= w || y >= h {
        Luma([0])
    } else {
        *src.get_pixel(x as u32, y as u32)
    }
}

/// Bilinear interpolation of four u8 corner values.
#[inline]
fn bilerp(c00: u8, c10: u8, c01: u8, c11: u8, tx: f32, ty: f32) -> u8 {
    let top = c00 as f32 * (1.0 - tx) + c10 as f32 * tx;
    let bot = c01 as f32 * (1.0 - tx) + c11 as f32 * tx;
    (top * (1.0 - ty) + bot * ty + 0.5) as u8
}

fn rgb_from_rgba(src: &RgbaImage) -> RgbImage {
    let (w, h) = src.dimensions();
    let mut rgb = RgbImage::new(w, h);
    for (x, y, pixel) in src.enumerate_pixels() {
        rgb.put_pixel(x, y, image::Rgb([pixel[0], pixel[1], pixel[2]]));
    }
    rgb
}

fn alpha_from_rgba(src: &RgbaImage) -> GrayImage {
    let (w, h) = src.dimensions();
    let mut alpha = GrayImage::new(w, h);
    for (x, y, pixel) in src.enumerate_pixels() {
        alpha.put_pixel(x, y, Luma([pixel[3]]));
    }
    alpha
}

fn rgba_from_rgb_and_alpha(rgb: &RgbImage, alpha: &GrayImage) -> RgbaImage {
    let (w, h) = rgb.dimensions();
    let mut rgba = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let rgb_pixel = rgb.get_pixel(x, y);
            let alpha_pixel = alpha.get_pixel(x, y);
            rgba.put_pixel(
                x,
                y,
                Rgba([rgb_pixel[0], rgb_pixel[1], rgb_pixel[2], alpha_pixel[0]]),
            );
        }
    }
    rgba
}

fn unsupported_image_format(img: &DynamicImage) -> Error {
    Error::UnsupportedImageFormat(format!("{:?}", img.color()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_image(w: u32, h: u32) -> RgbImage {
        RgbImage::from_pixel(w, h, image::Rgb([200u8, 150, 100]))
    }

    #[test]
    fn bilinear_exact_pixel() {
        let img = white_image(10, 10);
        let p = bilinear_sample(&img, 5.0, 5.0);
        assert_eq!(p, image::Rgb([200, 150, 100]));
    }

    #[test]
    fn bilinear_out_of_bounds_black() {
        let img = white_image(10, 10);
        let p = bilinear_sample(&img, -1.0, 5.0);
        assert_eq!(p, image::Rgb([0, 0, 0]));
    }

    #[test]
    fn normalisation_factor_square() {
        // For a 100×100 image: diag/2 = sqrt(2)*50 ≈ 70.71
        let n = normalisation_factor(100.0, 100.0, 1.0);
        let expected = (100_f32 * 100.0 * 2.0).sqrt() * 0.5;
        assert!((n - expected).abs() < 1e-4);
    }

    #[test]
    fn vignetting_correction_darkens_no_panic() {
        use crate::database::{Calibration, Lens, VignettingEntry};
        use crate::models::VignettingParams;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 35mm".into(),
            mounts: vec!["M42".into()],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![],
                tcas: vec![],
                vignetings: vec![VignettingEntry {
                    focal: 35.0,
                    aperture: 2.0,
                    distance: 1000.0,
                    params: VignettingParams {
                        k1: -0.5,
                        k2: 0.1,
                        k3: -0.05,
                    },
                }],
            },
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 35.0, 2.0, 10.0).unwrap();
        assert!(profile.vignetting.is_some());

        let mut img = RgbImage::from_pixel(64, 64, image::Rgb([200u8, 200, 200]));
        profile.correct_vignetting_inplace(&mut img);
        // Centre pixel should be nearly unchanged (r ≈ 0, factor ≈ 1)
        let centre = img.get_pixel(32, 32);
        assert!(
            centre[0] >= 195,
            "centre should be nearly unchanged, got {}",
            centre[0]
        );
    }

    #[test]
    fn distortion_no_data_returns_clone() {
        use crate::database::{Calibration, Lens};
        use image::GenericImageView;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();
        assert!(profile.distortion.is_none());

        let img = DynamicImage::ImageRgb8(white_image(16, 16));
        let result = profile.correct_distortion(&img).unwrap();
        assert_eq!(result.dimensions(), img.dimensions());
    }
}
