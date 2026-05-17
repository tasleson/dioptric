# dioptric

Pure-Rust lens distortion, vignetting, and chromatic aberration (TCA)
correction using the [lensfun](https://lensfun.github.io/) database.

## Quick start

```rust
use dioptric::{Database, CorrectionProfile};

let db = Database::bundled();

let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM")
    .expect("lens not found");
let camera = db.find_camera("Canon", "EOS 5D Mark III")
    .expect("camera not found");

let profile = CorrectionProfile::new(
    lens,
    camera.crop_factor(),
    35.0,   // focal length in mm
    4.0,    // aperture f-number
    10.0,   // focus distance in metres
).expect("failed to build profile");

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

The image correction API supports `DynamicImage::ImageRgb8` and
`DynamicImage::ImageRgba8`. `Rgba8` inputs preserve alpha; other image formats
return `Error::UnsupportedImageFormat`.

## Database lookup

`Database::find_camera` and `Database::find_lens` perform case-insensitive
substring matching, so EXIF strings rarely need to match exactly.

## License

The crate source code is licensed under MIT — see [LICENSE](LICENSE).

The bundled lens calibration database (in `db/`) is taken from the
[lensfun](https://github.com/lensfun/lensfun) project and is licensed under
CC-BY-SA 4.0 — see [LICENSE-lensfun](LICENSE-lensfun).
