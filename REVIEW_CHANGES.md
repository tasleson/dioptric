# Review Changes

Findings from code review of dioptric, assessed against integration with
rasterlab.

## API completeness

- [x] Add `find_cameras` / `find_lenses` methods returning iterators over all
      matches, not just the first
- [x] Add a single-string lens search variant (rasterlab has `lens_model` but
      no separate `lens_make`)
- [x] Remove the `from_xml_files` reference in the `Database` doc comment (the
      method does not exist), or implement it
- [x] Fix typo: `Calibration.vignetings` → `Calibration.vignettings`

## Data representation

- [x] Add a raw-slice or trait-based correction API that operates on
      `(width, height, &mut [u8])` so callers without the `image` crate can use
      dioptric without round-trip copies
- [x] Support f32 linear pixel data in the correction pipeline to avoid
      sRGB↔linear quantisation loss for callers that already work in linear
      space
- [x] Support 16-bit pixel data (many cameras produce 14-bit raw)
- [x] Remove or use the `_crop_factor` parameter in `normalisation_factor`
      (currently dead code)

## Rasterlab compatibility

- [x] Add `lens_make` field to rasterlab `ImageMetadata` (EXIF tag `0xa433`
      / `LensMake`)
- [ ] Add focus/subject distance to rasterlab `ImageMetadata` (EXIF tag
      `0x9206` / `SubjectDistance`)
- [ ] Verify end-to-end integration: rasterlab `Image` → dioptric correction →
      rasterlab `Image` without unnecessary copies

## Testing

- [ ] Add pipeline-level tests for Poly5 distortion model
- [ ] Add pipeline-level tests for PtLens distortion model
- [ ] Add pipeline-level tests for TcaPoly3 TCA model
- [ ] Add a test that verifies warp actually moves pixels to the expected
      location (e.g. barrel distortion moves corner pixels inward)
- [ ] Add a test for `from_xml` error path through the public API (not just
      the internal `ingest_bundled_file` helper)
- [ ] Document the sort-by-focal-length precondition on interpolation functions,
      or make them sort internally

## Search quality

- [ ] Evaluate whether case-insensitive substring matching is sufficient, or
      whether token-based / edit-distance fuzzy matching is needed for
      real-world EXIF strings (e.g. `"EF24-70mm"` vs `"EF 24-70mm"`)
