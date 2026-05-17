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

| Method | Description |
|--------|-------------|
| `CorrectionProfile::correct_all` | Distortion + TCA + vignetting |
| `CorrectionProfile::correct_distortion` | Geometric warp only |
| `CorrectionProfile::correct_vignetting` | In-place brightness correction |
| `CorrectionProfile::correct_tca` | Per-channel warp only |

For raw-processor integrations, `CorrectionProfile::distortion_coordinate_map`
and `CorrectionProfile::tca_coordinate_map` expose source-coordinate maps for
callers that own their resampling pipeline. `CorrectionOptions` provides
stage selection and Lensfun-style color-first ordering via the
`correct_with_options_raw*` methods. `CoordinateMapOptions::reverse(true)`
matches Lensfun's reverse-transform terminology.

The image correction API supports `DynamicImage::ImageRgb8` and
`DynamicImage::ImageRgba8`. `Rgba8` inputs preserve alpha; other image formats
return `Error::UnsupportedImageFormat`.

## Database lookup

`Database::find_camera`, `Database::find_lens_for_camera`, and
`Database::find_lens_by_name_for_camera` perform case-insensitive substring
matching, so EXIF strings rarely need to match exactly. The camera-aware lens
lookups use the camera mount and crop factor to rank duplicate lens profiles.
Use `find_lenses_for_camera` or `find_lenses_by_name_for_camera` to inspect
ranked alternatives and surface ambiguous profile choices in a UI.

## License

The crate source code is licensed under MIT — see [LICENSE](LICENSE).

The bundled lens calibration database (in `db/`) is taken from the
[lensfun](https://github.com/lensfun/lensfun) project and is licensed under
CC-BY-SA 4.0 — see [LICENSE-lensfun](LICENSE-lensfun).
