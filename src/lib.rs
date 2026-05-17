//! # dioptric
//!
//! Pure-Rust lens distortion, vignetting, and chromatic aberration (TCA)
//! correction using the [lensfun](https://lensfun.github.io/) database.
//!
//! ## Quick start
//!
//! ```
//! use dioptric::{Database, CorrectionProfile};
//!
//! let db = Database::bundled();
//!
//! let camera = db.find_camera("Canon", "EOS 5D Mark III")
//!     .expect("camera not found");
//! let lens = db.find_lens_for_camera(
//!     camera,
//!     "Canon",
//!     "EF 24-70mm f/2.8L II USM",
//! ).expect("lens not found");
//!
//! let profile = CorrectionProfile::builder(lens)
//!     .camera(camera)
//!     .focal_length(35.0) // mm
//!     .aperture(4.0)     // f-number
//!     .distance(10.0)    // metres
//!     .build()
//!     .expect("failed to build profile");
//!
//! // Apply corrections to a DynamicImage:
//! // let corrected = profile.correct_all(&img).unwrap();
//! ```
//!
//! ## Corrections
//!
//! The raw-slice methods operate on `&[u8]` / `&mut [u8]` buffers (sRGB, 8-bit
//! per channel) directly and have no dependency on the `image` crate:
//!
//! | Method | Description |
//! |--------|-------------|
//! | [`CorrectionProfile::correct_all_raw`] | Distortion + TCA + vignetting |
//! | [`CorrectionProfile::correct_distortion_raw`] | Geometric warp only |
//! | [`CorrectionProfile::correct_vignetting_raw`] | In-place brightness correction |
//! | [`CorrectionProfile::correct_tca_raw`] | Per-channel warp only |
//!
//! For callers working with 16-bit linear sensor data (many cameras produce
//! 14-bit raw), the `_u16` methods operate on `&[u16]` / `&mut [u16]` with
//! values in 0–65535 treated as linear:
//!
//! | Method | Description |
//! |--------|-------------|
//! | [`CorrectionProfile::correct_all_raw_u16`] | Distortion + TCA + vignetting |
//! | [`CorrectionProfile::correct_distortion_raw_u16`] | Geometric warp only |
//! | [`CorrectionProfile::correct_vignetting_raw_u16`] | In-place brightness correction |
//! | [`CorrectionProfile::correct_tca_raw_u16`] | Per-channel warp only |
//!
//! For callers working in linear f32 space (HDR pipelines, raw processors),
//! equivalent `_f32` methods avoid sRGB↔linear quantisation loss:
//!
//! | Method | Description |
//! |--------|-------------|
//! | [`CorrectionProfile::correct_all_raw_f32`] | Distortion + TCA + vignetting |
//! | [`CorrectionProfile::correct_distortion_raw_f32`] | Geometric warp only |
//! | [`CorrectionProfile::correct_vignetting_raw_f32`] | In-place brightness correction |
//! | [`CorrectionProfile::correct_tca_raw_f32`] | Per-channel warp only |
//!
//! With the `image` feature (enabled by default), convenience methods that
//! accept `image::DynamicImage` are also available:
//!
//! | Method | Description |
//! |--------|-------------|
//! | [`CorrectionProfile::correct_all`] | Distortion + TCA + vignetting |
//! | [`CorrectionProfile::correct_distortion`] | Geometric warp only |
//! | [`CorrectionProfile::correct_vignetting`] | In-place brightness correction |
//! | [`CorrectionProfile::correct_tca`] | Per-channel warp only |
//!
//! Both APIs support 3-channel (RGB) and 4-channel (RGBA) data. `Rgba` inputs
//! preserve alpha; unsupported formats return [`Error::UnsupportedImageFormat`].
//! Raw-processor integrations can use
//! [`CorrectionProfile::distortion_coordinate_map`] and
//! [`CorrectionProfile::tca_coordinate_map`] to feed their own resampling
//! pipeline, or [`CorrectionOptions`] with `correct_with_options_raw*` methods
//! to choose stages and Lensfun-style color-first ordering.
//! [`CoordinateMapOptions::reverse`] matches Lensfun's reverse-transform flag.
//!
//! ## Database lookup
//!
//! [`Database::find_camera`] and [`Database::find_lens`] return the first
//! match; [`Database::find_cameras`] and [`Database::find_lenses`] return
//! iterators over all matches. [`Database::find_lens_for_camera`] and
//! [`Database::find_lens_by_name_for_camera`] use camera mount and crop factor
//! to rank duplicate lens profiles.
//! [`Database::find_lenses_for_camera`] and
//! [`Database::find_lenses_by_name_for_camera`] expose those ranked
//! alternatives for ambiguous UI workflows.
//! All lookup methods perform case-insensitive substring matching, so EXIF
//! strings rarely need to match exactly.
//!
//! [`Database::find_lens_by_name`] and [`Database::find_lenses_by_name`]
//! accept a single query string matched against the combined `"maker model"`
//! text — useful when only an EXIF `LensModel` field is available without a
//! separate maker.

pub mod correction;
pub mod database;
pub mod error;
pub mod models;

pub use correction::{
    Coordinate, CoordinateMapOptions, CorrectionOptions, CorrectionProfile,
    CorrectionProfileBuilder, PipelineOrder, SubpixelCoordinates, TransformMode,
};
pub use database::{Database, LensMatch, LensMountMatch, MountCompatibility};
pub use error::{Error, Result};
