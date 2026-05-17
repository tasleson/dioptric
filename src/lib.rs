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
//! let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM")
//!     .expect("lens not found");
//! let camera = db.find_camera("Canon", "EOS 5D Mark III")
//!     .expect("camera not found");
//!
//! let profile = CorrectionProfile::new(
//!     lens,
//!     camera.crop_factor(),
//!     35.0,   // focal length in mm
//!     4.0,    // aperture f-number
//!     10.0,   // focus distance in metres
//! ).expect("failed to build profile");
//!
//! // Apply corrections to a DynamicImage:
//! // let corrected = profile.correct_all(&img).unwrap();
//! ```
//!
//! ## Corrections
//!
//! The raw-slice methods operate on `&[u8]` / `&mut [u8]` buffers directly and
//! have no dependency on the `image` crate:
//!
//! | Method | Description |
//! |--------|-------------|
//! | [`CorrectionProfile::correct_all_raw`] | Distortion + TCA + vignetting |
//! | [`CorrectionProfile::correct_distortion_raw`] | Geometric warp only |
//! | [`CorrectionProfile::correct_vignetting_raw`] | In-place brightness correction |
//! | [`CorrectionProfile::correct_tca_raw`] | Per-channel warp only |
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
//!
//! ## Database lookup
//!
//! [`Database::find_camera`] and [`Database::find_lens`] return the first
//! match; [`Database::find_cameras`] and [`Database::find_lenses`] return
//! iterators over all matches.  All four perform case-insensitive substring
//! matching, so EXIF strings rarely need to match exactly.
//!
//! [`Database::find_lens_by_name`] and [`Database::find_lenses_by_name`]
//! accept a single query string matched against the combined `"maker model"`
//! text — useful when only an EXIF `LensModel` field is available without a
//! separate maker.

pub mod correction;
pub mod database;
pub mod error;
pub mod models;

pub use correction::CorrectionProfile;
pub use database::Database;
pub use error::{Error, Result};
