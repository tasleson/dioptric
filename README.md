# dioptric

Pure-Rust lens distortion, vignetting, and chromatic aberration (TCA)
correction using the [lensfun](https://lensfun.github.io/) database.

## Quick start

```rust
use dioptric::{Database, CorrectionProfile};

let db = Database::bundled();

let camera = db.find_camera("Canon", "EOS 5D Mark III")
    .expect("camera not found");
let lens = db.find_lens_for_camera(
    camera,
    "Canon",
    "EF 24-70mm f/2.8L II USM",
).expect("lens not found");

let profile = CorrectionProfile::builder(lens)
    .camera(camera)
    .focal_length(35.0) // mm
    .aperture(4.0)     // f-number
    .distance(10.0)    // metres
    .build()
    .expect("failed to build profile");

// Apply corrections to a DynamicImage:
// let corrected = profile.correct_all(&img).unwrap();
```

## Corrections

The raw-slice methods operate on `&[u8]` / `&mut [u8]` buffers directly and
do not require the `image` crate:

| Method | Description |
|--------|-------------|
| `CorrectionProfile::correct_all_raw` | Vignetting, then composed distortion + TCA |
| `CorrectionProfile::correct_distortion_raw` | Geometric warp only |
| `CorrectionProfile::correct_vignetting_raw` | In-place brightness correction |
| `CorrectionProfile::correct_tca_raw` | Per-channel warp only |

For callers working with 16-bit linear sensor data, the `_u16` methods operate
on `&[u16]` / `&mut [u16]` with values in `0..=65535` treated as linear:

| Method | Description |
|--------|-------------|
| `CorrectionProfile::correct_all_raw_u16` | Vignetting, then composed distortion + TCA |
| `CorrectionProfile::correct_distortion_raw_u16` | Geometric warp only |
| `CorrectionProfile::correct_vignetting_raw_u16` | In-place brightness correction |
| `CorrectionProfile::correct_tca_raw_u16` | Per-channel warp only |

For HDR and raw-processing pipelines already working in linear light, the
`_f32` methods avoid sRGB conversion and quantisation:

| Method | Description |
|--------|-------------|
| `CorrectionProfile::correct_all_raw_f32` | Vignetting, then composed distortion + TCA |
| `CorrectionProfile::correct_distortion_raw_f32` | Geometric warp only |
| `CorrectionProfile::correct_vignetting_raw_f32` | In-place brightness correction |
| `CorrectionProfile::correct_tca_raw_f32` | Per-channel warp only |

With the default `image` feature enabled, convenience methods are available for
`image::DynamicImage`:

| Method | Description |
|--------|-------------|
| `CorrectionProfile::correct_all` | Vignetting, then composed distortion + TCA |
| `CorrectionProfile::correct_distortion` | Geometric warp only |
| `CorrectionProfile::correct_vignetting` | In-place brightness correction |
| `CorrectionProfile::correct_tca` | Per-channel warp only |

All pixel-buffer APIs support 3-channel RGB and 4-channel RGBA data. RGBA
inputs preserve alpha. The `DynamicImage` convenience API supports
`ImageRgb8` and `ImageRgba8`; other image formats return
`Error::UnsupportedImageFormat`.

For integrations that own their resampling pipeline,
`CorrectionProfile::distortion_coordinate_map` and
`CorrectionProfile::tca_coordinate_map` expose source-coordinate maps.
`CorrectionOptions` provides stage selection while preserving Lensfun-style
color-first ordering through the `correct_with_options_raw*` methods.
`CoordinateMapOptions::reverse(true)` matches Lensfun's reverse-transform
terminology.

`CorrectionProfile::compute_autoscale` can compute a scale factor that keeps
geometry-corrected output in bounds, and `CorrectionOptions::with_scale` /
`CoordinateMapOptions::with_scale` can apply that scale to correction passes or
coordinate-map generation.

## Database lookup

`Database::find_camera` returns the first camera match.
`Database::find_cameras`, `Database::find_lenses`, and
`Database::find_lenses_by_name` return iterators over all non-camera-aware
matches. `Database::find_lens_for_camera` and
`Database::find_lens_by_name_for_camera` use camera mount and crop factor to
rank duplicate lens profiles.

Use `find_lenses_for_camera` or `find_lenses_by_name_for_camera` to inspect
ranked alternatives and surface ambiguous profile choices in a UI.
`Database::find_lenses_by_name` accepts a single query string matched against
the combined `"maker model"` lens text, which is useful when EXIF metadata
provides only `LensModel` without a separate lens maker field.

All lookup methods perform case-insensitive, normalised substring matching, so
EXIF strings rarely need to match exactly.

## Projection Support

Lensfun projection metadata is parsed for cameras and lenses. A correction
profile can target another projection with
`CorrectionProfile::builder(lens).target_projection(...)`, and
`CorrectionProfile::projection_mapping` reports when a profile includes
projection conversion.

## Provenance

dioptric implements lens correction models described by the Lensfun project and
uses the Lensfun XML database format. The implementation was written from the
documented formulas and public database schema, without copying source code from
the Lensfun C/C++ library or the Rust `lensfun` crate.

## License

The crate source code is licensed under MIT — see [LICENSE](LICENSE).

The bundled lens calibration database (in `db/`) is taken from the
[lensfun](https://github.com/lensfun/lensfun) project and is licensed under
CC-BY-SA 4.0 — see [LICENSE-lensfun](LICENSE-lensfun).
