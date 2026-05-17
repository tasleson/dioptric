# FIXME

## Findings

1. DONE: `CorrectionProfile::new` now passes `distance` through to vignetting interpolation, and vignetting resolution now interpolates over focus distance instead of collapsing to the farthest calibration.

2. DONE: the public image API no longer silently downconverts every `DynamicImage`.

   `correct_all`, `correct_distortion`, `correct_tca`, and `correct_vignetting`
   now support `ImageRgb8` and `ImageRgba8` explicitly. `Rgba8` inputs preserve
   alpha, and unsupported formats return `Error::UnsupportedImageFormat`
   instead of silently dropping alpha or bit depth. The crate-level docs now
   document that contract.

3. DONE: `Database::bundled()` now validates every bundled XML file and fails loudly.

   UTF-8 and XML parse failures are no longer ignored during bundled DB
   ingestion. The raw calibration parser was also updated to handle the
   interleaved calibration layout used by the real lensfun XML files, so the
   stricter bundled loader matches the documented panic-on-malformed-data
   contract without rejecting valid upstream files.

4. DONE: The crate is now publish-complete for crates.io.

   `Cargo.toml` now has `readme`, `repository`, and `documentation`. A
   `README.md` was added at the repo root. `LICENSE-lensfun` provides the
   required CC-BY-SA 4.0 attribution notice for the bundled lensfun database.

## Current Status

- Core implementation exists and is not a stub.
- Database parsing, profile building, and correction paths are implemented.
- `cargo test` passed locally.
- `cargo clippy --all-targets --all-features -- -D warnings` passed locally.

## Completeness Assessment

All identified issues are resolved. The library is ready for an initial crates.io release.
