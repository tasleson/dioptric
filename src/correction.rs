//! Image-level warp and correction pipeline.
//!
//! [`CorrectionProfile`] bundles a resolved set of correction parameters for a
//! specific lens, camera, focal length, aperture, and focus distance.  The
//! three correction operations can be applied individually or all at once.
//!
//! The `correct_*_raw` methods operate directly on byte slices and require no
//! external image library.  When the `image` feature is enabled (default),
//! convenience methods that accept [`image::DynamicImage`] are also available.

#[cfg(feature = "image")]
use image::{DynamicImage, RgbImage, RgbaImage};

use crate::database::{Lens, interpolate_distortion, interpolate_tca, interpolate_vignetting};
use crate::error::{Error, Result};
use crate::models::{DistortionModel, TcaModel, VignettingParams, linear_to_srgb, srgb_to_linear};

// ── CorrectionProfile ─────────────────────────────────────────────────────────

/// A resolved set of lens correction parameters for a single capture.
///
/// Build one with [`CorrectionProfile::new`], then apply corrections using the
/// `correct_*_raw` methods (always available) or the `correct_*` convenience
/// methods (requires the `image` feature).
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

        let distortion = interpolate_distortion(&cal.distortions, focal);
        let tca = interpolate_tca(&cal.tcas, focal);
        let vignetting = interpolate_vignetting(&cal.vignettings, focal, aperture, distance);

        Ok(Self {
            distortion,
            tca,
            vignetting,
            crop_factor,
        })
    }

    // ── Raw-slice public API ────────────────────────────────────────────

    /// Apply distortion, TCA, and vignetting corrections in sequence to raw
    /// pixel data, returning a new buffer.
    ///
    /// `src` must be row-major `[R,G,B,…]` with no padding.  `channels` must
    /// be 3 (RGB) or 4 (RGBA); the buffer length must equal
    /// `width × height × channels`.
    pub fn correct_all_raw(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u8],
    ) -> Result<Vec<u8>> {
        validate_buffer(width, height, channels, src.len())?;
        let after_dist = self.warp_distortion_raw(src, width, height, channels)?;
        let mut after_tca = self.warp_tca_raw(&after_dist, width, height, channels)?;
        self.correct_vignetting_raw_inplace(&mut after_tca, width, height, channels);
        Ok(after_tca)
    }

    /// Apply distortion correction only to raw pixel data, returning a new
    /// buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_distortion_raw(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u8],
    ) -> Result<Vec<u8>> {
        validate_buffer(width, height, channels, src.len())?;
        self.warp_distortion_raw(src, width, height, channels)
    }

    /// Apply vignetting correction in-place to raw pixel data.
    ///
    /// Converts sRGB → linear, scales each pixel, then converts back.
    /// Only the first three channels (RGB) are modified; a fourth channel
    /// (alpha) is left unchanged.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_vignetting_raw(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        data: &mut [u8],
    ) -> Result<()> {
        validate_buffer(width, height, channels, data.len())?;
        self.correct_vignetting_raw_inplace(data, width, height, channels);
        Ok(())
    }

    /// Apply TCA (chromatic aberration) correction only to raw pixel data,
    /// returning a new buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_tca_raw(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u8],
    ) -> Result<Vec<u8>> {
        validate_buffer(width, height, channels, src.len())?;
        self.warp_tca_raw(src, width, height, channels)
    }

    // ── u16 linear raw-slice public API ──────────────────────────────────

    /// Apply distortion, TCA, and vignetting corrections in sequence to u16
    /// linear pixel data, returning a new buffer.
    ///
    /// `src` must be row-major `[R,G,B,…]` with no padding.  `channels` must
    /// be 3 (RGB) or 4 (RGBA); the buffer length must equal
    /// `width × height × channels`.  Values are expected in linear light
    /// space (0–65535).
    pub fn correct_all_raw_u16(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u16],
    ) -> Result<Vec<u16>> {
        validate_buffer_u16(width, height, channels, src.len())?;
        let after_dist = self.warp_distortion_raw_u16(src, width, height, channels)?;
        let mut after_tca = self.warp_tca_raw_u16(&after_dist, width, height, channels)?;
        self.correct_vignetting_raw_u16_inplace(&mut after_tca, width, height, channels);
        Ok(after_tca)
    }

    /// Apply distortion correction only to u16 linear pixel data, returning a
    /// new buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_distortion_raw_u16(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u16],
    ) -> Result<Vec<u16>> {
        validate_buffer_u16(width, height, channels, src.len())?;
        self.warp_distortion_raw_u16(src, width, height, channels)
    }

    /// Apply vignetting correction in-place to u16 linear pixel data.
    ///
    /// Scales each pixel directly — no gamma conversion is performed since
    /// 16-bit sensor data is typically already linear.  Only the first three
    /// channels (RGB) are modified; a fourth channel (alpha) is left unchanged.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_vignetting_raw_u16(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        data: &mut [u16],
    ) -> Result<()> {
        validate_buffer_u16(width, height, channels, data.len())?;
        self.correct_vignetting_raw_u16_inplace(data, width, height, channels);
        Ok(())
    }

    /// Apply TCA (chromatic aberration) correction only to u16 linear pixel
    /// data, returning a new buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_tca_raw_u16(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[u16],
    ) -> Result<Vec<u16>> {
        validate_buffer_u16(width, height, channels, src.len())?;
        self.warp_tca_raw_u16(src, width, height, channels)
    }

    // ── f32 linear raw-slice public API ───────────────────────────────────

    /// Apply distortion, TCA, and vignetting corrections in sequence to f32
    /// linear pixel data, returning a new buffer.
    ///
    /// `src` must be row-major `[R,G,B,…]` with no padding.  `channels` must
    /// be 3 (RGB) or 4 (RGBA); the buffer length must equal
    /// `width × height × channels`.  Values are expected in linear light
    /// space (0.0–1.0 nominal range, though >1.0 is allowed for HDR).
    pub fn correct_all_raw_f32(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[f32],
    ) -> Result<Vec<f32>> {
        validate_buffer_f32(width, height, channels, src.len())?;
        let after_dist = self.warp_distortion_raw_f32(src, width, height, channels)?;
        let mut after_tca = self.warp_tca_raw_f32(&after_dist, width, height, channels)?;
        self.correct_vignetting_raw_f32_inplace(&mut after_tca, width, height, channels);
        Ok(after_tca)
    }

    /// Apply distortion correction only to f32 linear pixel data, returning a
    /// new buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_distortion_raw_f32(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[f32],
    ) -> Result<Vec<f32>> {
        validate_buffer_f32(width, height, channels, src.len())?;
        self.warp_distortion_raw_f32(src, width, height, channels)
    }

    /// Apply vignetting correction in-place to f32 linear pixel data.
    ///
    /// Scales each pixel directly — no sRGB conversion is performed since
    /// the data is already in linear space.  Only the first three channels
    /// (RGB) are modified; a fourth channel (alpha) is left unchanged.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_vignetting_raw_f32(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        data: &mut [f32],
    ) -> Result<()> {
        validate_buffer_f32(width, height, channels, data.len())?;
        self.correct_vignetting_raw_f32_inplace(data, width, height, channels);
        Ok(())
    }

    /// Apply TCA (chromatic aberration) correction only to f32 linear pixel
    /// data, returning a new buffer.
    ///
    /// `channels` must be 3 (RGB) or 4 (RGBA).
    pub fn correct_tca_raw_f32(
        &self,
        width: u32,
        height: u32,
        channels: u32,
        src: &[f32],
    ) -> Result<Vec<f32>> {
        validate_buffer_f32(width, height, channels, src.len())?;
        self.warp_tca_raw_f32(src, width, height, channels)
    }

    // ── DynamicImage convenience API (requires "image" feature) ─────────

    /// Apply distortion, TCA, and vignetting corrections in sequence.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    /// Other formats return [`Error::UnsupportedImageFormat`].
    #[cfg(feature = "image")]
    pub fn correct_all(&self, img: &DynamicImage) -> Result<DynamicImage> {
        dynamic_to_raw(img, |data, w, h, ch| self.correct_all_raw(w, h, ch, data))
    }

    /// Apply distortion correction only, returning a new image.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    #[cfg(feature = "image")]
    pub fn correct_distortion(&self, img: &DynamicImage) -> Result<DynamicImage> {
        dynamic_to_raw(img, |data, w, h, ch| {
            self.correct_distortion_raw(w, h, ch, data)
        })
    }

    /// Apply vignetting correction in-place.
    ///
    /// Converts sRGB → linear, scales each pixel, then converts back.
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    #[cfg(feature = "image")]
    pub fn correct_vignetting(&self, img: &mut DynamicImage) -> Result<()> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                let (w, h) = (rgb.width(), rgb.height());
                self.correct_vignetting_raw(w, h, 3, rgb)
            }
            DynamicImage::ImageRgba8(rgba) => {
                let (w, h) = (rgba.width(), rgba.height());
                self.correct_vignetting_raw(w, h, 4, rgba)
            }
            _ => Err(unsupported_image_format(img)),
        }
    }

    /// Apply TCA (chromatic aberration) correction only, returning a new image.
    ///
    /// Supports `DynamicImage::ImageRgb8` and `DynamicImage::ImageRgba8`.
    #[cfg(feature = "image")]
    pub fn correct_tca(&self, img: &DynamicImage) -> Result<DynamicImage> {
        dynamic_to_raw(img, |data, w, h, ch| self.correct_tca_raw(w, h, ch, data))
    }

    // ── internal helpers ────────────────────────────────────────────────

    fn warp_distortion_raw_u16(&self, src: &[u16], w: u32, h: u32, ch: u32) -> Result<Vec<u16>> {
        let model = match self.distortion {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0u16; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r_u = (xn * xn + yn * yn).sqrt();

                let r_d = model.undistorted_to_distorted(r_u);
                let scale = if r_u > 1e-8 { r_d / r_u } else { 1.0 };

                let src_x = xn * scale * norm + cx;
                let src_y = yn * scale * norm + cy;

                let dst_idx = py as usize * stride + px as usize * ch;
                bilinear_sample_raw_u16(
                    src,
                    wi,
                    hi,
                    ch,
                    src_x,
                    src_y,
                    &mut dst[dst_idx..dst_idx + ch],
                );
            }
        }
        Ok(dst)
    }

    fn warp_tca_raw_u16(&self, src: &[u16], w: u32, h: u32, ch: u32) -> Result<Vec<u16>> {
        let tca = match self.tca {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0u16; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();

                let (scale_r, scale_b) = tca.channel_radii(r);

                let rx = xn * scale_r * norm + cx;
                let ry = yn * scale_r * norm + cy;
                let red = bilinear_sample_channel_raw_u16(src, wi, hi, ch, rx, ry, 0);

                let bx = xn * scale_b * norm + cx;
                let by = yn * scale_b * norm + cy;
                let blue = bilinear_sample_channel_raw_u16(src, wi, hi, ch, bx, by, 2);

                let src_idx = py as usize * stride + px as usize * ch;
                let dst_idx = src_idx;
                dst[dst_idx] = red;
                dst[dst_idx + 1] = src[src_idx + 1];
                dst[dst_idx + 2] = blue;

                if ch == 4 {
                    dst[dst_idx + 3] = src[src_idx + 3];
                }
            }
        }
        Ok(dst)
    }

    fn correct_vignetting_raw_u16_inplace(&self, data: &mut [u16], w: u32, h: u32, ch: u32) {
        let params = match self.vignetting {
            Some(p) => p,
            None => return,
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let stride = w as usize * ch;

        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();
                let factor = params.factor(r);

                let idx = py as usize * stride + px as usize * ch;
                for c in 0..3 {
                    let scaled = data[idx + c] as f32 * factor;
                    data[idx + c] = scaled.round().clamp(0.0, 65535.0) as u16;
                }
            }
        }
    }

    fn warp_distortion_raw_f32(&self, src: &[f32], w: u32, h: u32, ch: u32) -> Result<Vec<f32>> {
        let model = match self.distortion {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0.0f32; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r_u = (xn * xn + yn * yn).sqrt();

                let r_d = model.undistorted_to_distorted(r_u);
                let scale = if r_u > 1e-8 { r_d / r_u } else { 1.0 };

                let src_x = xn * scale * norm + cx;
                let src_y = yn * scale * norm + cy;

                let dst_idx = py as usize * stride + px as usize * ch;
                bilinear_sample_raw_f32(
                    src,
                    wi,
                    hi,
                    ch,
                    src_x,
                    src_y,
                    &mut dst[dst_idx..dst_idx + ch],
                );
            }
        }
        Ok(dst)
    }

    fn warp_tca_raw_f32(&self, src: &[f32], w: u32, h: u32, ch: u32) -> Result<Vec<f32>> {
        let tca = match self.tca {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0.0f32; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();

                let (scale_r, scale_b) = tca.channel_radii(r);

                let rx = xn * scale_r * norm + cx;
                let ry = yn * scale_r * norm + cy;
                let red = bilinear_sample_channel_raw_f32(src, wi, hi, ch, rx, ry, 0);

                let bx = xn * scale_b * norm + cx;
                let by = yn * scale_b * norm + cy;
                let blue = bilinear_sample_channel_raw_f32(src, wi, hi, ch, bx, by, 2);

                let src_idx = py as usize * stride + px as usize * ch;
                let dst_idx = src_idx;
                dst[dst_idx] = red;
                dst[dst_idx + 1] = src[src_idx + 1];
                dst[dst_idx + 2] = blue;

                if ch == 4 {
                    dst[dst_idx + 3] = src[src_idx + 3];
                }
            }
        }
        Ok(dst)
    }

    fn correct_vignetting_raw_f32_inplace(&self, data: &mut [f32], w: u32, h: u32, ch: u32) {
        let params = match self.vignetting {
            Some(p) => p,
            None => return,
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let stride = w as usize * ch;

        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();
                let factor = params.factor(r);

                let idx = py as usize * stride + px as usize * ch;
                for c in 0..3 {
                    data[idx + c] *= factor;
                }
            }
        }
    }

    fn warp_distortion_raw(&self, src: &[u8], w: u32, h: u32, ch: u32) -> Result<Vec<u8>> {
        let model = match self.distortion {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0u8; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r_u = (xn * xn + yn * yn).sqrt();

                let r_d = model.undistorted_to_distorted(r_u);
                let scale = if r_u > 1e-8 { r_d / r_u } else { 1.0 };

                let src_x = xn * scale * norm + cx;
                let src_y = yn * scale * norm + cy;

                let dst_idx = py as usize * stride + px as usize * ch;
                bilinear_sample_raw(
                    src,
                    wi,
                    hi,
                    ch,
                    src_x,
                    src_y,
                    &mut dst[dst_idx..dst_idx + ch],
                );
            }
        }
        Ok(dst)
    }

    fn warp_tca_raw(&self, src: &[u8], w: u32, h: u32, ch: u32) -> Result<Vec<u8>> {
        let tca = match self.tca {
            Some(m) => m,
            None => return Ok(src.to_vec()),
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let wi = w as i32;
        let hi = h as i32;
        let stride = w as usize * ch;

        let mut dst = vec![0u8; src.len()];
        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();

                let (scale_r, scale_b) = tca.channel_radii(r);

                let rx = xn * scale_r * norm + cx;
                let ry = yn * scale_r * norm + cy;
                let red = bilinear_sample_channel_raw(src, wi, hi, ch, rx, ry, 0);

                let bx = xn * scale_b * norm + cx;
                let by = yn * scale_b * norm + cy;
                let blue = bilinear_sample_channel_raw(src, wi, hi, ch, bx, by, 2);

                let src_idx = py as usize * stride + px as usize * ch;
                let dst_idx = src_idx;
                dst[dst_idx] = red;
                dst[dst_idx + 1] = src[src_idx + 1];
                dst[dst_idx + 2] = blue;

                if ch == 4 {
                    dst[dst_idx + 3] = src[src_idx + 3];
                }
            }
        }
        Ok(dst)
    }

    fn correct_vignetting_raw_inplace(&self, data: &mut [u8], w: u32, h: u32, ch: u32) {
        let params = match self.vignetting {
            Some(p) => p,
            None => return,
        };

        let (wf, hf) = (w as f32, h as f32);
        let norm = normalisation_factor(wf, hf, self.crop_factor);
        let cx = wf * 0.5;
        let cy = hf * 0.5;
        let ch = ch as usize;
        let stride = w as usize * ch;

        for py in 0..h {
            for px in 0..w {
                let xn = (px as f32 - cx) / norm;
                let yn = (py as f32 - cy) / norm;
                let r = (xn * xn + yn * yn).sqrt();
                let factor = params.factor(r);

                let idx = py as usize * stride + px as usize * ch;
                for c in 0..3 {
                    let linear = srgb_to_linear(data[idx + c]);
                    data[idx + c] = linear_to_srgb(linear * factor);
                }
            }
        }
    }
}

// ── Validation ───────────────────────────────────────────────────────────────

fn validate_buffer_u16(width: u32, height: u32, channels: u32, len: usize) -> Result<()> {
    if channels != 3 && channels != 4 {
        return Err(Error::UnsupportedImageFormat(format!(
            "{channels} channels (expected 3 or 4)"
        )));
    }
    let expected = width as usize * height as usize * channels as usize;
    if len != expected {
        return Err(Error::InvalidBufferLength {
            expected,
            actual: len,
            width,
            height,
            channels,
        });
    }
    Ok(())
}

fn validate_buffer_f32(width: u32, height: u32, channels: u32, len: usize) -> Result<()> {
    if channels != 3 && channels != 4 {
        return Err(Error::UnsupportedImageFormat(format!(
            "{channels} channels (expected 3 or 4)"
        )));
    }
    let expected = width as usize * height as usize * channels as usize;
    if len != expected {
        return Err(Error::InvalidBufferLength {
            expected,
            actual: len,
            width,
            height,
            channels,
        });
    }
    Ok(())
}

fn validate_buffer(width: u32, height: u32, channels: u32, len: usize) -> Result<()> {
    if channels != 3 && channels != 4 {
        return Err(Error::UnsupportedImageFormat(format!(
            "{channels} channels (expected 3 or 4)"
        )));
    }
    let expected = width as usize * height as usize * channels as usize;
    if len != expected {
        return Err(Error::InvalidBufferLength {
            expected,
            actual: len,
            width,
            height,
            channels,
        });
    }
    Ok(())
}

// ── DynamicImage bridge (requires "image" feature) ───────────────────────────

#[cfg(feature = "image")]
fn dynamic_to_raw(
    img: &DynamicImage,
    f: impl FnOnce(&[u8], u32, u32, u32) -> Result<Vec<u8>>,
) -> Result<DynamicImage> {
    match img {
        DynamicImage::ImageRgb8(rgb) => {
            let (w, h) = (rgb.width(), rgb.height());
            let data = f(rgb.as_raw(), w, h, 3)?;
            Ok(DynamicImage::ImageRgb8(
                RgbImage::from_raw(w, h, data).unwrap(),
            ))
        }
        DynamicImage::ImageRgba8(rgba) => {
            let (w, h) = (rgba.width(), rgba.height());
            let data = f(rgba.as_raw(), w, h, 4)?;
            Ok(DynamicImage::ImageRgba8(
                RgbaImage::from_raw(w, h, data).unwrap(),
            ))
        }
        _ => Err(unsupported_image_format(img)),
    }
}

#[cfg(feature = "image")]
fn unsupported_image_format(img: &DynamicImage) -> Error {
    Error::UnsupportedImageFormat(format!("{:?}", img.color()))
}

// ── Geometry helpers ─────────────────────────────────────────────────────────

/// Compute the normalisation factor that maps pixel coordinates to the lensfun
/// normalised coordinate system.  Lensfun calibration data is expressed relative
/// to a full-frame (crop=1) sensor, so on a crop sensor the normalised radius
/// must be scaled by 1/crop_factor.
#[inline]
fn normalisation_factor(w: f32, h: f32, crop_factor: f32) -> f32 {
    (w * w + h * h).sqrt() * 0.5 / crop_factor
}

// ── Bilinear interpolation on raw slices ─────────────────────────────────────

fn bilinear_sample_raw(src: &[u8], w: i32, h: i32, ch: usize, x: f32, y: f32, out: &mut [u8]) {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    for (c, dst) in out.iter_mut().enumerate().take(ch) {
        let p00 = get_component(src, w, h, ch, x0, y0, c);
        let p10 = get_component(src, w, h, ch, x0 + 1, y0, c);
        let p01 = get_component(src, w, h, ch, x0, y0 + 1, c);
        let p11 = get_component(src, w, h, ch, x0 + 1, y0 + 1, c);
        *dst = bilerp(p00, p10, p01, p11, tx, ty);
    }
}

fn bilinear_sample_channel_raw(
    src: &[u8],
    w: i32,
    h: i32,
    ch: usize,
    x: f32,
    y: f32,
    channel: usize,
) -> u8 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_component(src, w, h, ch, x0, y0, channel);
    let p10 = get_component(src, w, h, ch, x0 + 1, y0, channel);
    let p01 = get_component(src, w, h, ch, x0, y0 + 1, channel);
    let p11 = get_component(src, w, h, ch, x0 + 1, y0 + 1, channel);

    bilerp(p00, p10, p01, p11, tx, ty)
}

#[inline]
fn get_component(src: &[u8], w: i32, h: i32, ch: usize, x: i32, y: i32, c: usize) -> u8 {
    if x < 0 || y < 0 || x >= w || y >= h {
        0
    } else {
        src[y as usize * w as usize * ch + x as usize * ch + c]
    }
}

/// Bilinear interpolation of four u8 corner values.
#[inline]
fn bilerp(c00: u8, c10: u8, c01: u8, c11: u8, tx: f32, ty: f32) -> u8 {
    let top = c00 as f32 * (1.0 - tx) + c10 as f32 * tx;
    let bot = c01 as f32 * (1.0 - tx) + c11 as f32 * tx;
    (top * (1.0 - ty) + bot * ty + 0.5) as u8
}

// ── Bilinear interpolation on f32 slices ────────────────────────────────────

fn bilinear_sample_raw_f32(
    src: &[f32],
    w: i32,
    h: i32,
    ch: usize,
    x: f32,
    y: f32,
    out: &mut [f32],
) {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    for (c, dst) in out.iter_mut().enumerate().take(ch) {
        let p00 = get_component_f32(src, w, h, ch, x0, y0, c);
        let p10 = get_component_f32(src, w, h, ch, x0 + 1, y0, c);
        let p01 = get_component_f32(src, w, h, ch, x0, y0 + 1, c);
        let p11 = get_component_f32(src, w, h, ch, x0 + 1, y0 + 1, c);
        *dst = bilerp_f32(p00, p10, p01, p11, tx, ty);
    }
}

fn bilinear_sample_channel_raw_f32(
    src: &[f32],
    w: i32,
    h: i32,
    ch: usize,
    x: f32,
    y: f32,
    channel: usize,
) -> f32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_component_f32(src, w, h, ch, x0, y0, channel);
    let p10 = get_component_f32(src, w, h, ch, x0 + 1, y0, channel);
    let p01 = get_component_f32(src, w, h, ch, x0, y0 + 1, channel);
    let p11 = get_component_f32(src, w, h, ch, x0 + 1, y0 + 1, channel);

    bilerp_f32(p00, p10, p01, p11, tx, ty)
}

#[inline]
fn get_component_f32(src: &[f32], w: i32, h: i32, ch: usize, x: i32, y: i32, c: usize) -> f32 {
    if x < 0 || y < 0 || x >= w || y >= h {
        0.0
    } else {
        src[y as usize * w as usize * ch + x as usize * ch + c]
    }
}

#[inline]
fn bilerp_f32(c00: f32, c10: f32, c01: f32, c11: f32, tx: f32, ty: f32) -> f32 {
    let top = c00 * (1.0 - tx) + c10 * tx;
    let bot = c01 * (1.0 - tx) + c11 * tx;
    top * (1.0 - ty) + bot * ty
}

// ── Bilinear interpolation on u16 slices ────────────────────────────────────

fn bilinear_sample_raw_u16(
    src: &[u16],
    w: i32,
    h: i32,
    ch: usize,
    x: f32,
    y: f32,
    out: &mut [u16],
) {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    for (c, dst) in out.iter_mut().enumerate().take(ch) {
        let p00 = get_component_u16(src, w, h, ch, x0, y0, c);
        let p10 = get_component_u16(src, w, h, ch, x0 + 1, y0, c);
        let p01 = get_component_u16(src, w, h, ch, x0, y0 + 1, c);
        let p11 = get_component_u16(src, w, h, ch, x0 + 1, y0 + 1, c);
        *dst = bilerp_u16(p00, p10, p01, p11, tx, ty);
    }
}

fn bilinear_sample_channel_raw_u16(
    src: &[u16],
    w: i32,
    h: i32,
    ch: usize,
    x: f32,
    y: f32,
    channel: usize,
) -> u16 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x.floor();
    let ty = y - y.floor();

    let p00 = get_component_u16(src, w, h, ch, x0, y0, channel);
    let p10 = get_component_u16(src, w, h, ch, x0 + 1, y0, channel);
    let p01 = get_component_u16(src, w, h, ch, x0, y0 + 1, channel);
    let p11 = get_component_u16(src, w, h, ch, x0 + 1, y0 + 1, channel);

    bilerp_u16(p00, p10, p01, p11, tx, ty)
}

#[inline]
fn get_component_u16(src: &[u16], w: i32, h: i32, ch: usize, x: i32, y: i32, c: usize) -> u16 {
    if x < 0 || y < 0 || x >= w || y >= h {
        0
    } else {
        src[y as usize * w as usize * ch + x as usize * ch + c]
    }
}

#[inline]
fn bilerp_u16(c00: u16, c10: u16, c01: u16, c11: u16, tx: f32, ty: f32) -> u16 {
    let top = c00 as f32 * (1.0 - tx) + c10 as f32 * tx;
    let bot = c01 as f32 * (1.0 - tx) + c11 as f32 * tx;
    (top * (1.0 - ty) + bot * ty + 0.5) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinear_exact_pixel() {
        let pixel = [200u8, 150, 100];
        let data: Vec<u8> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [0u8; 3];
        bilinear_sample_raw(&data, 10, 10, 3, 5.0, 5.0, &mut out);
        assert_eq!(out, [200, 150, 100]);
    }

    #[test]
    fn bilinear_out_of_bounds_black() {
        let pixel = [200u8, 150, 100];
        let data: Vec<u8> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [0u8; 3];
        bilinear_sample_raw(&data, 10, 10, 3, -1.0, 5.0, &mut out);
        assert_eq!(out, [0, 0, 0]);
    }

    #[test]
    fn normalisation_factor_square() {
        let n = normalisation_factor(100.0, 100.0, 1.0);
        let expected = (100_f32 * 100.0 * 2.0).sqrt() * 0.5;
        assert!((n - expected).abs() < 1e-4);

        let n_crop = normalisation_factor(100.0, 100.0, 1.5);
        let expected_crop = (100_f32 * 100.0 * 2.0).sqrt() * 0.5 / 1.5;
        assert!((n_crop - expected_crop).abs() < 1e-4);
    }

    #[test]
    fn vignetting_raw_inplace() {
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
                vignettings: vec![VignettingEntry {
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

        let (w, h, ch) = (64u32, 64u32, 3u32);
        let mut data = vec![200u8; (w * h * ch) as usize];
        profile.correct_vignetting_raw(w, h, ch, &mut data).unwrap();
        let centre_idx = (32 * w as usize + 32) * ch as usize;
        let corner_idx = 0;
        assert!(
            data[centre_idx] >= 195,
            "centre should be nearly unchanged, got {}",
            data[centre_idx]
        );
        assert!(
            data[corner_idx] > 200,
            "negative vignetting coefficients should brighten corners, got {}",
            data[corner_idx]
        );
    }

    #[test]
    fn distortion_raw_no_data_returns_copy() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();
        assert!(profile.distortion.is_none());

        let src = vec![128u8; 16 * 16 * 3];
        let result = profile.correct_distortion_raw(16, 16, 3, &src).unwrap();
        assert_eq!(result, src);
    }

    #[test]
    fn invalid_buffer_length_rejected() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![0u8; 100];
        assert!(profile.correct_distortion_raw(10, 10, 3, &src).is_err());
    }

    #[test]
    fn invalid_channels_rejected() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![0u8; 200];
        assert!(profile.correct_distortion_raw(10, 10, 2, &src).is_err());
    }

    #[test]
    fn rgba_raw_preserves_alpha() {
        use crate::database::{Calibration, DistortionEntry, Lens};
        use crate::models::Poly3Params;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
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

        let (w, h) = (16u32, 16u32);
        let src: Vec<u8> = (0..w * h).flat_map(|_| [100u8, 150, 200, 77]).collect();
        let result = profile.correct_all_raw(w, h, 4, &src).unwrap();
        let centre = (8 * w as usize + 8) * 4;
        assert_eq!(result[centre + 3], 77, "alpha at centre must be preserved");
    }

    #[test]
    fn bilinear_f32_exact_pixel() {
        let pixel = [0.8f32, 0.6, 0.4];
        let data: Vec<f32> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [0.0f32; 3];
        bilinear_sample_raw_f32(&data, 10, 10, 3, 5.0, 5.0, &mut out);
        assert!((out[0] - 0.8).abs() < 1e-6);
        assert!((out[1] - 0.6).abs() < 1e-6);
        assert!((out[2] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn bilinear_f32_out_of_bounds_zero() {
        let pixel = [0.8f32, 0.6, 0.4];
        let data: Vec<f32> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [1.0f32; 3];
        bilinear_sample_raw_f32(&data, 10, 10, 3, -1.0, 5.0, &mut out);
        assert_eq!(out, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn vignetting_f32_no_srgb_roundtrip() {
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
                vignettings: vec![VignettingEntry {
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

        let (w, h, ch) = (64u32, 64u32, 3u32);
        let mut data = vec![0.75f32; (w * h * ch) as usize];
        profile
            .correct_vignetting_raw_f32(w, h, ch, &mut data)
            .unwrap();
        let centre_idx = (32 * w as usize + 32) * ch as usize;
        assert!(
            (data[centre_idx] - 0.75).abs() < 0.01,
            "centre should be nearly unchanged, got {}",
            data[centre_idx]
        );
    }

    #[test]
    fn distortion_f32_no_data_returns_copy() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![0.5f32; 16 * 16 * 3];
        let result = profile.correct_distortion_raw_f32(16, 16, 3, &src).unwrap();
        assert_eq!(result, src);
    }

    #[test]
    fn f32_invalid_buffer_length_rejected() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![0.0f32; 100];
        assert!(profile.correct_distortion_raw_f32(10, 10, 3, &src).is_err());
    }

    #[test]
    fn f32_rgba_preserves_alpha() {
        use crate::database::{Calibration, DistortionEntry, Lens};
        use crate::models::Poly3Params;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
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

        let (w, h) = (16u32, 16u32);
        let src: Vec<f32> = (0..w * h).flat_map(|_| [0.4f32, 0.6, 0.8, 0.3]).collect();
        let result = profile.correct_all_raw_f32(w, h, 4, &src).unwrap();
        let centre = (8 * w as usize + 8) * 4;
        assert!(
            (result[centre + 3] - 0.3).abs() < 1e-6,
            "alpha at centre must be preserved"
        );
    }

    #[test]
    fn f32_vignetting_skips_alpha() {
        use crate::database::{Calibration, Lens, VignettingEntry};
        use crate::models::VignettingParams;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 35mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![],
                tcas: vec![],
                vignettings: vec![VignettingEntry {
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

        let (w, h, ch) = (16u32, 16u32, 4u32);
        let mut data: Vec<f32> = (0..w * h)
            .flat_map(|_| [0.75f32, 0.75, 0.75, 0.9])
            .collect();
        profile
            .correct_vignetting_raw_f32(w, h, ch, &mut data)
            .unwrap();
        for i in (3..data.len()).step_by(4) {
            assert!(
                (data[i] - 0.9).abs() < 1e-6,
                "alpha must be untouched, got {} at index {}",
                data[i],
                i
            );
        }
    }

    #[test]
    fn bilinear_u16_exact_pixel() {
        let pixel = [40000u16, 30000, 20000];
        let data: Vec<u16> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [0u16; 3];
        bilinear_sample_raw_u16(&data, 10, 10, 3, 5.0, 5.0, &mut out);
        assert_eq!(out, [40000, 30000, 20000]);
    }

    #[test]
    fn bilinear_u16_out_of_bounds_zero() {
        let pixel = [40000u16, 30000, 20000];
        let data: Vec<u16> = pixel.iter().copied().cycle().take(10 * 10 * 3).collect();
        let mut out = [1u16; 3];
        bilinear_sample_raw_u16(&data, 10, 10, 3, -1.0, 5.0, &mut out);
        assert_eq!(out, [0, 0, 0]);
    }

    #[test]
    fn vignetting_u16_linear() {
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
                vignettings: vec![VignettingEntry {
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

        let (w, h, ch) = (64u32, 64u32, 3u32);
        let mut data = vec![50000u16; (w * h * ch) as usize];
        profile
            .correct_vignetting_raw_u16(w, h, ch, &mut data)
            .unwrap();
        let centre_idx = (32 * w as usize + 32) * ch as usize;
        assert!(
            (data[centre_idx] as i32 - 50000).unsigned_abs() < 200,
            "centre should be nearly unchanged, got {}",
            data[centre_idx]
        );
    }

    #[test]
    fn distortion_u16_no_data_returns_copy() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![32000u16; 16 * 16 * 3];
        let result = profile.correct_distortion_raw_u16(16, 16, 3, &src).unwrap();
        assert_eq!(result, src);
    }

    #[test]
    fn u16_invalid_buffer_length_rejected() {
        use crate::database::{Calibration, Lens};

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration::default(),
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 50.0, 4.0, 10.0).unwrap();

        let src = vec![0u16; 100];
        assert!(profile.correct_distortion_raw_u16(10, 10, 3, &src).is_err());
    }

    #[test]
    fn u16_rgba_preserves_alpha() {
        use crate::database::{Calibration, DistortionEntry, Lens};
        use crate::models::Poly3Params;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 50mm".into(),
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

        let (w, h) = (16u32, 16u32);
        let src: Vec<u16> = (0..w * h)
            .flat_map(|_| [10000u16, 20000, 40000, 5000])
            .collect();
        let result = profile.correct_all_raw_u16(w, h, 4, &src).unwrap();
        let centre = (8 * w as usize + 8) * 4;
        assert_eq!(
            result[centre + 3],
            5000,
            "alpha at centre must be preserved"
        );
    }

    #[test]
    fn u16_vignetting_skips_alpha() {
        use crate::database::{Calibration, Lens, VignettingEntry};
        use crate::models::VignettingParams;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 35mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![],
                tcas: vec![],
                vignettings: vec![VignettingEntry {
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

        let (w, h, ch) = (16u32, 16u32, 4u32);
        let mut data: Vec<u16> = (0..w * h)
            .flat_map(|_| [50000u16, 50000, 50000, 60000])
            .collect();
        profile
            .correct_vignetting_raw_u16(w, h, ch, &mut data)
            .unwrap();
        for i in (3..data.len()).step_by(4) {
            assert_eq!(
                data[i], 60000,
                "alpha must be untouched, got {} at index {}",
                data[i], i
            );
        }
    }

    #[cfg(feature = "image")]
    #[test]
    fn image_api_matches_raw() {
        use crate::database::{Calibration, DistortionEntry, Lens, VignettingEntry};
        use crate::models::Poly3Params;

        let lens = Lens {
            maker: "Test".into(),
            model: "Test 35mm".into(),
            mounts: vec![],
            crop_factor: Some(1.0),
            calibration: Calibration {
                distortions: vec![DistortionEntry {
                    focal: 35.0,
                    model: DistortionModel::Poly3(Poly3Params { k1: -0.01 }),
                }],
                tcas: vec![],
                vignettings: vec![VignettingEntry {
                    focal: 35.0,
                    aperture: 2.0,
                    distance: 1000.0,
                    params: VignettingParams {
                        k1: -0.3,
                        k2: 0.1,
                        k3: 0.0,
                    },
                }],
            },
        };
        let profile = CorrectionProfile::new(&lens, 1.0, 35.0, 2.0, 10.0).unwrap();

        let (w, h) = (32u32, 32u32);
        let mut raw_data = vec![0u8; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 3;
                raw_data[idx] = (x * 8) as u8;
                raw_data[idx + 1] = (y * 8) as u8;
                raw_data[idx + 2] = 128;
            }
        }

        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, raw_data.clone()).unwrap());
        let result_img = profile.correct_all(&img).unwrap();
        let result_raw = profile.correct_all_raw(w, h, 3, &raw_data).unwrap();

        let img_bytes = match result_img {
            DynamicImage::ImageRgb8(rgb) => rgb.into_raw(),
            _ => panic!("expected Rgb8"),
        };
        assert_eq!(img_bytes, result_raw);
    }
}
