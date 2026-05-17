# dioptric — Implementation Plan

🤖 Assisted-by: Claude Sonnet 4.6

Pure-Rust lens correction library using the lensfun XML database. Covers distortion,
vignetting, and transverse chromatic aberration (TCA). Intended for eventual publication
on crates.io with no C dependencies.

---

## Status

- [x] Repo + crate scaffold
- [x] Download and bundle lensfun database
- [x] XML parser / camera+lens structs
- [x] Fuzzy camera/lens lookup
- [x] Distortion correction (PTLens, Poly3, Poly5)
- [x] Vignetting correction (model=pa)
- [x] TCA correction (linear + poly3)
- [x] Calibration interpolation (focal length × aperture)
- [x] Public API (`Database`, `CorrectionProfile`)
- [x] Unit tests
- [x] Integration tests
- [x] `cargo fmt` + `cargo clippy` clean
- [x] `cargo test` passing

---

## Crate metadata

| Field | Value |
|---|---|
| Name | `dioptric` |
| Location | `/Users/tony/dioptric` |
| License | MIT OR Apache-2.0 |
| Edition | 2024 |
| Target | crates.io |

---

## Dependencies

```toml
quick-xml   = { version = "0.37", features = ["serialize"] }
serde       = { version = "1", features = ["derive"] }
thiserror   = "2"
include_dir = "0.7"
image       = { version = "0.25", default-features = false, features = ["jpeg", "png"] }
```

---

## File structure

```
dioptric/
├── Cargo.toml
├── src/
│   ├── lib.rs          — public API re-exports, top-level doc
│   ├── error.rs        — Error enum (thiserror)
│   ├── database.rs     — XML parsing, Camera/Lens structs, fuzzy lookup
│   ├── models.rs       — distortion / vignetting / TCA math (f32)
│   └── correction.rs   — pixel warp, bilinear interpolation, image I/O
├── tests/
│   └── integration.rs
└── db/                 — bundled lensfun XML database (CC-BY-SA 3.0)
```

---

## Public API sketch

```rust
let db = Database::bundled();
let lens   = db.find_lens("Canon", "Canon EF 24-70mm f/2.8L II USM").ok_or(...)?;
let camera = db.find_camera("Canon", "Canon EOS 5D Mark III").ok_or(...)?;

let profile = CorrectionProfile::new(lens, camera.crop_factor(), 24.0, 2.8, 10.0)?;

let corrected = profile.correct_all(&img)?;          // all three corrections
let corrected = profile.correct_distortion(&img)?;   // distortion only
profile.correct_vignetting(&mut img);                // in-place
let corrected = profile.correct_tca(&img)?;          // TCA only
```

---

## Correction math

### Coordinate normalisation

All positions normalised by `image_diagonal / 2`, so r = 1.0 at the corner.

### Distortion (forward model: undistorted → distorted coords)

**PTLens:** `r_d = r_u * (a·r_u³ + b·r_u² + c·r_u + (1 - a - b - c))`
**Poly3:**  `r_d = r_u * (1 + k1·r_u²)`
**Poly5:**  `r_d = r_u * (1 + k1·r_u² + k2·r_u⁴)`

Image correction: for each output pixel, compute r_u → r_d via model → sample input at
scaled source coords. Bilinear interpolation; out-of-bounds → black.

### Vignetting (model=pa)

`factor = 1 + k1·r² + k2·r⁴ + k3·r⁶`  (r normalised, 0..1 at corner)

Convert sRGB → linear light, multiply by factor, convert back. Applied in-place.

### TCA

**Linear:** red scaled by `kr`, blue by `kb`, green unchanged.
**Poly3:** per-channel `r_corr = r * (1 + v·r² + c·r⁴)` with separate (vr, cr)/(vb, cb).

Same bilinear warp as distortion, applied per colour channel.

### Calibration interpolation

Multiple calibration entries exist per lens at different focal lengths (and apertures for
vignetting). Linearly interpolate parameters between the two bracketing entries. For
vignetting, bilinearly interpolate over focal_length × aperture. Clamp at range edges.

---

## Database licensing

The lensfun database is **CC-BY-SA 3.0**. The crate code is MIT OR Apache-2.0.
Attribution and licence notice must be included in the crate when shipping the database.

---

## Design constraints

- No panics in library code — all fallible paths return `Result`
- Core math on `f32` arrays; `image::RgbImage` at the public API boundary
- Case-insensitive fuzzy matching for camera/lens name lookup (EXIF strings are inconsistent)
- Out-of-bounds warp coords → black pixel (no OOB, no panic)
