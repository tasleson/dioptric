//! Lensfun XML database parsing and fuzzy lookup.
//!
//! The lensfun database consists of a collection of XML files, each containing
//! `<camera>` and `<lens>` elements.  This module parses those files into
//! in-memory structures and provides case-insensitive substring search.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::models::{
    DistortionModel, Poly3Params, Poly5Params, PtLensParams, TcaLinearParams, TcaModel,
    TcaPoly3Params, VignettingParams,
};

// ── Raw serde structs (mirror the XML) ───────────────────────────────────────

fn default_one() -> f32 {
    1.0
}

/// A single `<camera>` element.  Only the first (language-neutral) `<model>`
/// and `<maker>` values are captured; additional localised variants are ignored.
#[derive(Debug, Deserialize)]
struct RawCamera {
    #[serde(rename = "maker")]
    makers: Vec<String>,
    #[serde(rename = "model")]
    models: Vec<String>,
    mount: String,
    cropfactor: f32,
}

/// A single calibration `<distortion>` element.
#[derive(Debug, Deserialize)]
struct RawDistortion {
    #[serde(rename = "@model")]
    model: String,
    #[serde(rename = "@focal")]
    focal: f32,
    #[serde(rename = "@a", default)]
    a: f32,
    #[serde(rename = "@b", default)]
    b: f32,
    #[serde(rename = "@c", default)]
    c: f32,
    #[serde(rename = "@k1", default)]
    k1: f32,
    #[serde(rename = "@k2", default)]
    k2: f32,
}

/// A single calibration `<tca>` element.
#[derive(Debug, Deserialize)]
struct RawTca {
    #[serde(rename = "@model")]
    model: String,
    #[serde(rename = "@focal")]
    focal: f32,
    // linear model
    #[serde(rename = "@kr", default = "default_one")]
    kr: f32,
    #[serde(rename = "@kb", default = "default_one")]
    kb: f32,
    // poly3 model
    #[serde(rename = "@vr", default = "default_one")]
    vr: f32,
    #[serde(rename = "@cr", default)]
    cr: f32,
    #[serde(rename = "@br", default)]
    br: f32,
    #[serde(rename = "@vb", default = "default_one")]
    vb: f32,
    #[serde(rename = "@cb", default)]
    cb: f32,
    #[serde(rename = "@bb", default)]
    bb: f32,
}

/// A single calibration `<vignetting>` element.
#[derive(Debug, Deserialize)]
struct RawVignetting {
    #[serde(rename = "@model")]
    model: String,
    #[serde(rename = "@focal")]
    focal: f32,
    #[serde(rename = "@aperture")]
    aperture: f32,
    #[serde(rename = "@distance")]
    distance: f32,
    #[serde(rename = "@k1", default)]
    k1: f32,
    #[serde(rename = "@k2", default)]
    k2: f32,
    #[serde(rename = "@k3", default)]
    k3: f32,
}

#[derive(Debug, Deserialize)]
enum RawCalibrationEntry {
    #[serde(rename = "distortion")]
    Distortion(RawDistortion),
    #[serde(rename = "tca")]
    Tca(RawTca),
    #[serde(rename = "vignetting")]
    Vignetting(RawVignetting),
}

#[derive(Debug, Deserialize, Default)]
struct RawCalibration {
    #[serde(rename = "$value", default)]
    entries: Vec<RawCalibrationEntry>,
}

/// A single `<lens>` element.
#[derive(Debug, Deserialize)]
struct RawLens {
    #[serde(rename = "maker")]
    makers: Vec<String>,
    #[serde(rename = "model")]
    models: Vec<String>,
    #[serde(rename = "mount", default)]
    mounts: Vec<String>,
    cropfactor: Option<f32>,
    #[serde(rename = "calibration", default)]
    calibration: Option<RawCalibration>,
}

/// Top-level `<lensdatabase>` element.
#[derive(Debug, Deserialize)]
struct RawLensDatabase {
    #[serde(rename = "camera", default)]
    cameras: Vec<RawCamera>,
    #[serde(rename = "lens", default)]
    lenses: Vec<RawLens>,
}

// ── Public structs ────────────────────────────────────────────────────────────

/// A camera body from the lensfun database.
#[derive(Debug, Clone)]
pub struct Camera {
    /// Manufacturer name (first language-neutral entry).
    pub maker: String,
    /// Model name (first language-neutral entry).
    pub model: String,
    /// Mount type.
    pub mount: String,
    /// Sensor crop factor relative to 35 mm full frame.
    pub crop_factor: f32,
}

impl Camera {
    /// Sensor crop factor relative to 35 mm full frame.
    pub fn crop_factor(&self) -> f32 {
        self.crop_factor
    }
}

/// A calibrated focal-length point for distortion.
#[derive(Debug, Clone)]
pub struct DistortionEntry {
    pub focal: f32,
    pub model: DistortionModel,
}

/// A calibrated focal-length point for TCA.
#[derive(Debug, Clone)]
pub struct TcaEntry {
    pub focal: f32,
    pub model: TcaModel,
}

/// A calibrated focal-length + aperture point for vignetting.
#[derive(Debug, Clone)]
pub struct VignettingEntry {
    pub focal: f32,
    pub aperture: f32,
    pub distance: f32,
    pub params: VignettingParams,
}

/// Calibration data attached to a lens.
#[derive(Debug, Clone, Default)]
pub struct Calibration {
    pub distortions: Vec<DistortionEntry>,
    pub tcas: Vec<TcaEntry>,
    pub vignettings: Vec<VignettingEntry>,
}

/// A lens from the lensfun database.
#[derive(Debug, Clone)]
pub struct Lens {
    /// Manufacturer name.
    pub maker: String,
    /// Model name.
    pub model: String,
    /// Compatible mount names.
    pub mounts: Vec<String>,
    /// Nominal crop factor.
    pub crop_factor: Option<f32>,
    /// Available calibration data.
    pub calibration: Calibration,
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn parse_distortion(raw: &RawDistortion) -> Result<DistortionModel> {
    match raw.model.as_str() {
        "ptlens" => Ok(DistortionModel::PtLens(PtLensParams {
            a: raw.a,
            b: raw.b,
            c: raw.c,
        })),
        "poly3" => Ok(DistortionModel::Poly3(Poly3Params { k1: raw.k1 })),
        "poly5" => Ok(DistortionModel::Poly5(Poly5Params {
            k1: raw.k1,
            k2: raw.k2,
        })),
        other => Err(Error::UnknownModel(other.to_owned())),
    }
}

fn parse_tca(raw: &RawTca) -> Result<TcaModel> {
    match raw.model.as_str() {
        "linear" => Ok(TcaModel::Linear(TcaLinearParams {
            kr: raw.kr,
            kb: raw.kb,
        })),
        "poly3" => Ok(TcaModel::Poly3(TcaPoly3Params {
            vr: raw.vr,
            cr: raw.cr,
            br: raw.br,
            vb: raw.vb,
            cb: raw.cb,
            bb: raw.bb,
        })),
        other => Err(Error::UnknownModel(other.to_owned())),
    }
}

fn convert_lens(raw: RawLens) -> Lens {
    let maker = raw.makers.into_iter().next().unwrap_or_default();
    let model = raw.models.into_iter().next().unwrap_or_default();
    let mut cal = Calibration::default();

    if let Some(raw_cal) = raw.calibration {
        for entry in raw_cal.entries {
            match entry {
                RawCalibrationEntry::Distortion(d) => {
                    if let Ok(model) = parse_distortion(&d) {
                        cal.distortions.push(DistortionEntry {
                            focal: d.focal,
                            model,
                        });
                    }
                }
                RawCalibrationEntry::Tca(t) => {
                    if let Ok(model) = parse_tca(&t) {
                        cal.tcas.push(TcaEntry {
                            focal: t.focal,
                            model,
                        });
                    }
                }
                RawCalibrationEntry::Vignetting(v) => {
                    if v.model == "pa" {
                        cal.vignettings.push(VignettingEntry {
                            focal: v.focal,
                            aperture: v.aperture,
                            distance: v.distance,
                            params: VignettingParams {
                                k1: v.k1,
                                k2: v.k2,
                                k3: v.k3,
                            },
                        });
                    }
                }
            }
        }
    }

    Lens {
        maker,
        model,
        mounts: raw.mounts,
        crop_factor: raw.cropfactor,
        calibration: cal,
    }
}

fn convert_camera(raw: RawCamera) -> Camera {
    Camera {
        maker: raw.makers.into_iter().next().unwrap_or_default(),
        model: raw.models.into_iter().next().unwrap_or_default(),
        mount: raw.mount,
        crop_factor: raw.cropfactor,
    }
}

// ── Database ──────────────────────────────────────────────────────────────────

/// The parsed lensfun database.
///
/// Load from the bundled data with [`Database::bundled`], or parse arbitrary
/// XML with [`Database::from_xml`].
///
/// # Example
///
/// ```
/// let db = dioptric::Database::bundled();
/// let camera = db.find_camera("Canon", "5D Mark III");
/// assert!(camera.is_some(), "5D Mark III should be in the bundled database");
/// ```
pub struct Database {
    pub(crate) cameras: Vec<Camera>,
    pub(crate) lenses: Vec<Lens>,
}

impl Database {
    /// Load and parse the bundled lensfun database (included at compile time).
    ///
    /// # Panics
    ///
    /// Panics only if the bundled XML data is malformed — this should never
    /// happen with the shipped database files.
    pub fn bundled() -> Self {
        use include_dir::{Dir, include_dir};
        static DB_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/db");

        let mut db = Database {
            cameras: Vec::new(),
            lenses: Vec::new(),
        };
        for entry in DB_DIR.files() {
            let name = entry.path().to_string_lossy();
            if !name.ends_with(".xml") {
                continue;
            }
            db.ingest_bundled_file(&name, entry.contents())
                .unwrap_or_else(|err| panic!("failed to load bundled database file {name}: {err}"));
        }
        db
    }

    /// Parse a single lensfun-format XML string and add its entries to the
    /// database.
    ///
    /// Returns an error if the XML cannot be parsed; individual entries with
    /// unknown models are silently skipped.
    ///
    /// # Example
    ///
    /// ```
    /// let mut db = dioptric::Database::empty();
    /// db.from_xml(r#"<lensdatabase version="2">
    ///   <camera>
    ///     <maker>Acme</maker><model>Acme X1</model>
    ///     <mount>M42</mount><cropfactor>1.5</cropfactor>
    ///   </camera>
    /// </lensdatabase>"#).unwrap();
    /// assert!(db.find_camera("acme", "x1").is_some());
    /// ```
    pub fn from_xml(&mut self, xml: &str) -> Result<()> {
        self.ingest_xml(xml)
    }

    fn ingest_xml(&mut self, xml: &str) -> Result<()> {
        let raw: RawLensDatabase = quick_xml::de::from_str(xml)?;
        for c in raw.cameras {
            self.cameras.push(convert_camera(c));
        }
        for l in raw.lenses {
            self.lenses.push(convert_lens(l));
        }
        Ok(())
    }

    fn ingest_bundled_file(
        &mut self,
        name: &str,
        contents: &[u8],
    ) -> std::result::Result<(), String> {
        let text = std::str::from_utf8(contents)
            .map_err(|err| format!("invalid UTF-8 in {name}: {err}"))?;
        self.ingest_xml(text)
            .map_err(|err| format!("invalid XML in {name}: {err}"))
    }

    /// Create an empty database.
    pub fn empty() -> Self {
        Database {
            cameras: Vec::new(),
            lenses: Vec::new(),
        }
    }

    /// All cameras in the database.
    pub fn cameras(&self) -> &[Camera] {
        &self.cameras
    }

    /// All lenses in the database.
    pub fn lenses(&self) -> &[Lens] {
        &self.lenses
    }

    /// Find a camera by maker and model using case-insensitive substring matching.
    ///
    /// Both `maker_query` and `model_query` must appear as substrings of the
    /// respective camera fields.  Matching falls back to ignoring punctuation
    /// and whitespace for common EXIF formatting differences. Returns the
    /// first match.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let cam = db.find_camera("Canon", "EOS 5D Mark III");
    /// assert!(cam.is_some());
    /// ```
    pub fn find_camera(&self, maker_query: &str, model_query: &str) -> Option<&Camera> {
        self.cameras.iter().find(|c| {
            fuzzy_contains(&c.maker, maker_query) && fuzzy_contains(&c.model, model_query)
        })
    }

    /// Find a lens by maker and model using case-insensitive substring matching,
    /// with a fallback that ignores punctuation and whitespace.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM");
    /// assert!(lens.is_some());
    /// ```
    pub fn find_lens(&self, maker_query: &str, model_query: &str) -> Option<&Lens> {
        self.lenses.iter().find(|l| {
            fuzzy_contains(&l.maker, maker_query) && fuzzy_contains(&l.model, model_query)
        })
    }

    /// Find all cameras matching maker and model using case-insensitive
    /// substring matching, with a fallback that ignores punctuation and
    /// whitespace.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let cameras: Vec<_> = db.find_cameras("Canon", "EOS").collect();
    /// assert!(cameras.len() > 1);
    /// ```
    pub fn find_cameras<'a>(
        &'a self,
        maker_query: &'a str,
        model_query: &'a str,
    ) -> impl Iterator<Item = &'a Camera> {
        self.cameras.iter().filter(move |c| {
            fuzzy_contains(&c.maker, maker_query) && fuzzy_contains(&c.model, model_query)
        })
    }

    /// Find all lenses matching maker and model using case-insensitive
    /// substring matching, with a fallback that ignores punctuation and
    /// whitespace.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let lenses: Vec<_> = db.find_lenses("Canon", "EF").collect();
    /// assert!(lenses.len() > 1);
    /// ```
    pub fn find_lenses<'a>(
        &'a self,
        maker_query: &'a str,
        model_query: &'a str,
    ) -> impl Iterator<Item = &'a Lens> {
        self.lenses.iter().filter(move |l| {
            fuzzy_contains(&l.maker, maker_query) && fuzzy_contains(&l.model, model_query)
        })
    }

    /// Find a lens using a single query string, matched against the
    /// combined `"maker model"` text using case-insensitive substring
    /// matching, with a fallback that ignores punctuation and whitespace.
    ///
    /// This is useful when the caller only has a single lens description
    /// string (e.g. an EXIF `LensModel` field) without a separate maker.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let lens = db.find_lens_by_name("Canon EF 24-70mm f/2.8L II USM");
    /// assert!(lens.is_some());
    /// ```
    pub fn find_lens_by_name(&self, query: &str) -> Option<&Lens> {
        self.lenses.iter().find(|l| {
            let full = format!("{} {}", l.maker, l.model);
            fuzzy_contains(&full, query)
        })
    }

    /// Find all lenses matching a single query string against the combined
    /// `"maker model"` text using case-insensitive substring matching, with a
    /// fallback that ignores punctuation and whitespace.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let lenses: Vec<_> = db.find_lenses_by_name("Canon EF").collect();
    /// assert!(lenses.len() > 1);
    /// ```
    pub fn find_lenses_by_name(&self, query: &str) -> impl Iterator<Item = &Lens> {
        self.lenses.iter().filter(move |l| {
            let full = format!("{} {}", l.maker, l.model);
            fuzzy_contains(&full, query)
        })
    }
}

// ── Interpolation helpers (pub(crate)) ────────────────────────────────────────

fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
    let haystack_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    if haystack_lower.contains(&needle_lower) {
        return true;
    }

    normalise_search_text(&haystack_lower).contains(&normalise_search_text(&needle_lower))
}

fn normalise_search_text(value: &str) -> String {
    value.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Interpolate distortion parameters for the given focal length.
///
/// Returns `None` only if `entries` is empty.
pub(crate) fn interpolate_distortion(
    entries: &[DistortionEntry],
    focal: f32,
) -> Option<DistortionModel> {
    if entries.is_empty() {
        return None;
    }
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.focal.total_cmp(&b.focal));

    if entries.len() == 1 {
        return Some(entries[0].model);
    }

    // Clamp to range
    let first = &entries[0];
    let last = &entries[entries.len() - 1];
    if focal <= first.focal {
        return Some(first.model);
    }
    if focal >= last.focal {
        return Some(last.model);
    }

    // Find bracketing pair
    let idx = entries.partition_point(|e| e.focal < focal);
    let lo = &entries[idx - 1];
    let hi = &entries[idx];
    let t = (focal - lo.focal) / (hi.focal - lo.focal);

    let model = match (lo.model, hi.model) {
        (DistortionModel::PtLens(a), DistortionModel::PtLens(b)) => {
            DistortionModel::PtLens(PtLensParams::lerp(a, b, t))
        }
        (DistortionModel::Poly3(a), DistortionModel::Poly3(b)) => {
            DistortionModel::Poly3(Poly3Params::lerp(a, b, t))
        }
        (DistortionModel::Poly5(a), DistortionModel::Poly5(b)) => {
            DistortionModel::Poly5(Poly5Params::lerp(a, b, t))
        }
        // Mixed models: use the nearest entry
        _ => {
            if (focal - lo.focal) <= (hi.focal - focal) {
                lo.model
            } else {
                hi.model
            }
        }
    };
    Some(model)
}

/// Interpolate TCA parameters for the given focal length.
pub(crate) fn interpolate_tca(entries: &[TcaEntry], focal: f32) -> Option<TcaModel> {
    if entries.is_empty() {
        return None;
    }
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.focal.total_cmp(&b.focal));

    if entries.len() == 1 {
        return Some(entries[0].model);
    }

    let first = &entries[0];
    let last = &entries[entries.len() - 1];
    if focal <= first.focal {
        return Some(first.model);
    }
    if focal >= last.focal {
        return Some(last.model);
    }

    let idx = entries.partition_point(|e| e.focal < focal);
    let lo = &entries[idx - 1];
    let hi = &entries[idx];
    let t = (focal - lo.focal) / (hi.focal - lo.focal);

    let model = match (lo.model, hi.model) {
        (TcaModel::Linear(a), TcaModel::Linear(b)) => {
            TcaModel::Linear(TcaLinearParams::lerp(a, b, t))
        }
        (TcaModel::Poly3(a), TcaModel::Poly3(b)) => TcaModel::Poly3(TcaPoly3Params::lerp(a, b, t)),
        _ => {
            if (focal - lo.focal) <= (hi.focal - focal) {
                lo.model
            } else {
                hi.model
            }
        }
    };
    Some(model)
}

/// Trilinear interpolation of vignetting parameters over focal × aperture ×
/// distance, with each axis clamped to the calibrated range.
pub(crate) fn interpolate_vignetting(
    entries: &[VignettingEntry],
    focal: f32,
    aperture: f32,
    distance: f32,
) -> Option<VignettingParams> {
    if entries.is_empty() {
        return None;
    }

    // Unique focal lengths
    let mut focals: Vec<f32> = entries.iter().map(|entry| entry.focal).collect();
    focals.sort_by(f32::total_cmp);
    focals.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    // Unique apertures
    let mut apertures: Vec<f32> = entries.iter().map(|entry| entry.aperture).collect();
    apertures.sort_by(f32::total_cmp);
    apertures.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    // Unique focus distances
    let mut distances: Vec<f32> = entries.iter().map(|entry| entry.distance).collect();
    distances.sort_by(f32::total_cmp);
    distances.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    // For a given (f, a, d) tuple, look up the params (or None if not present).
    let lookup = |f: f32, a: f32, d: f32| -> Option<VignettingParams> {
        entries
            .iter()
            .find(|entry| {
                (entry.focal - f).abs() < 1e-4
                    && (entry.aperture - a).abs() < 1e-4
                    && (entry.distance - d).abs() < 1e-4
            })
            .map(|entry| entry.params)
    };

    fn axis_bounds(values: &[f32], sample: f32) -> Option<(f32, f32, f32)> {
        if values.is_empty() {
            return None;
        }
        let clamped = sample.clamp(values[0], values[values.len() - 1]);
        if values.len() == 1 {
            return Some((values[0], values[0], 0.0));
        }

        let idx = values.partition_point(|&x| x < clamped);
        let (lo, hi) = if idx == 0 {
            (values[0], values[1])
        } else if idx >= values.len() {
            (values[values.len() - 2], values[values.len() - 1])
        } else {
            (values[idx - 1], values[idx])
        };
        let t = if (hi - lo).abs() < 1e-6 {
            0.0
        } else {
            (clamped - lo) / (hi - lo)
        };
        Some((lo, hi, t))
    }

    // Interpolate along the focal axis for each aperture + distance bound.
    fn interp_focal(
        focals: &[f32],
        focal: f32,
        aperture: f32,
        distance: f32,
        lookup: &dyn Fn(f32, f32, f32) -> Option<VignettingParams>,
    ) -> Option<VignettingParams> {
        let (f0, f1, t) = axis_bounds(focals, focal)?;
        let p0 = lookup(f0, aperture, distance)?;
        let p1 = lookup(f1, aperture, distance)?;
        Some(VignettingParams::lerp(p0, p1, t))
    }

    // Interpolate along the aperture axis at a fixed distance.
    fn interp_aperture(
        focals: &[f32],
        apertures: &[f32],
        focal: f32,
        aperture: f32,
        distance: f32,
        lookup: &dyn Fn(f32, f32, f32) -> Option<VignettingParams>,
    ) -> Option<VignettingParams> {
        let (a0, a1, t) = axis_bounds(apertures, aperture)?;
        let p0 = interp_focal(focals, focal, a0, distance, lookup)?;
        let p1 = interp_focal(focals, focal, a1, distance, lookup)?;
        Some(VignettingParams::lerp(p0, p1, t))
    }

    let (d0, d1, t) = axis_bounds(&distances, distance)?;
    let p0 = interp_aperture(&focals, &apertures, focal, aperture, d0, &lookup)?;
    let p1 = interp_aperture(&focals, &apertures, focal, aperture, d1, &lookup)?;
    Some(VignettingParams::lerp(p0, p1, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_xml() {
        let xml = r#"<lensdatabase version="2">
  <camera>
    <maker>Acme</maker>
    <model>Acme X1</model>
    <mount>M42</mount>
    <cropfactor>1.5</cropfactor>
  </camera>
  <lens>
    <maker>Acme</maker>
    <model>Acme 35mm f/2</model>
    <mount>M42</mount>
    <cropfactor>1.5</cropfactor>
    <calibration>
      <distortion model="poly3" focal="35" k1="-0.005"/>
      <vignetting model="pa" focal="35" aperture="2" distance="1000" k1="-0.3" k2="0.1" k3="-0.05"/>
    </calibration>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.from_xml(xml).expect("parse should succeed");
        assert_eq!(db.cameras().len(), 1);
        assert_eq!(db.lenses().len(), 1);
    }

    #[test]
    fn parse_interleaved_calibration_entries() {
        let xml = r#"<lensdatabase version="2">
  <lens>
    <maker>Acme</maker>
    <model>Acme Zoom</model>
    <mount>M42</mount>
    <cropfactor>1.0</cropfactor>
    <calibration>
      <distortion model="poly3" focal="24" k1="-0.01"/>
      <tca model="linear" focal="24" kr="1.001" kb="0.999"/>
      <vignetting model="pa" focal="24" aperture="2.8" distance="1" k1="-0.3" k2="0.1" k3="-0.05"/>
      <distortion model="poly3" focal="70" k1="0.01"/>
      <tca model="linear" focal="70" kr="1.002" kb="0.998"/>
      <vignetting model="pa" focal="70" aperture="4" distance="10" k1="-0.2" k2="0.05" k3="-0.01"/>
    </calibration>
  </lens>
</lensdatabase>"#;

        let mut db = Database::empty();
        db.from_xml(xml).expect("parse should succeed");
        let lens = db.find_lens("Acme", "Zoom").expect("lens should load");
        assert_eq!(lens.calibration.distortions.len(), 2);
        assert_eq!(lens.calibration.tcas.len(), 2);
        assert_eq!(lens.calibration.vignettings.len(), 2);
    }

    #[test]
    fn parse_tca_poly3_uses_cr_cb_and_lensfun_defaults() {
        let xml = r#"<lensdatabase version="2">
  <lens>
    <maker>Acme</maker>
    <model>Acme TCA</model>
    <mount>M42</mount>
    <cropfactor>1.0</cropfactor>
    <calibration>
      <tca model="poly3" focal="35" cr="0.2" br="0.3" cb="-0.1" bb="0.05"/>
      <tca model="linear" focal="50"/>
    </calibration>
  </lens>
</lensdatabase>"#;

        let mut db = Database::empty();
        db.from_xml(xml).expect("parse should succeed");
        let lens = db.find_lens("Acme", "TCA").expect("lens should load");

        match lens.calibration.tcas[0].model {
            TcaModel::Poly3(params) => {
                assert_eq!(params.vr, 1.0);
                assert_eq!(params.cr, 0.2);
                assert_eq!(params.br, 0.3);
                assert_eq!(params.vb, 1.0);
                assert_eq!(params.cb, -0.1);
                assert_eq!(params.bb, 0.05);
            }
            other => panic!("expected poly3 TCA model, got {other:?}"),
        }

        match lens.calibration.tcas[1].model {
            TcaModel::Linear(params) => {
                assert_eq!(params.kr, 1.0);
                assert_eq!(params.kb, 1.0);
            }
            other => panic!("expected linear TCA model, got {other:?}"),
        }
    }

    #[test]
    fn find_camera_fuzzy() {
        let xml = r#"<lensdatabase version="2">
  <camera>
    <maker>Canon</maker>
    <model>Canon EOS 5D Mark III</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </camera>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.from_xml(xml).unwrap();
        assert!(db.find_camera("canon", "5d mark iii").is_some());
        assert!(db.find_camera("CANON", "EOS 5D").is_some());
        assert!(db.find_camera("nikon", "5d mark iii").is_none());
    }

    #[test]
    fn find_lens_by_name_single_string() {
        let xml = r#"<lensdatabase version="2">
  <lens>
    <maker>Canon</maker>
    <model>EF 24-70mm f/2.8L II USM</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
  <lens>
    <maker>Nikon</maker>
    <model>AF-S 70-200mm f/2.8E FL ED VR</model>
    <mount>Nikon F</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.from_xml(xml).unwrap();

        // Match against combined "maker model"
        assert!(db.find_lens_by_name("Canon EF 24-70").is_some());
        assert!(db.find_lens_by_name("Nikon AF-S 70-200").is_some());

        // Case-insensitive
        assert!(db.find_lens_by_name("canon ef 24-70").is_some());

        // Partial model-only match (model contains the substring)
        assert!(db.find_lens_by_name("24-70mm").is_some());

        // EXIF strings often omit spaces or punctuation found in lensfun names.
        assert!(db.find_lens("Canon", "EF24-70mm").is_some());
        assert!(db.find_lens_by_name("Canon EF24-70mm f28L").is_some());

        // No match
        assert!(db.find_lens_by_name("Sigma 50mm").is_none());
    }

    #[test]
    fn find_lenses_by_name_returns_multiple() {
        let xml = r#"<lensdatabase version="2">
  <lens>
    <maker>Canon</maker>
    <model>EF 24-70mm f/2.8L II USM</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
  <lens>
    <maker>Canon</maker>
    <model>EF 70-200mm f/2.8L IS II USM</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.from_xml(xml).unwrap();

        let matches: Vec<_> = db.find_lenses_by_name("Canon EF").collect();
        assert_eq!(matches.len(), 2);

        let compact_matches: Vec<_> = db.find_lenses_by_name("EF70200mm").collect();
        assert_eq!(compact_matches.len(), 1);
        assert_eq!(compact_matches[0].model, "EF 70-200mm f/2.8L IS II USM");
    }

    #[test]
    fn bundled_db_loads() {
        let db = Database::bundled();
        assert!(
            db.cameras().len() > 10,
            "expected many cameras in bundled db"
        );
        assert!(db.lenses().len() > 10, "expected many lenses in bundled db");
    }

    #[test]
    fn bundled_finds_canon_5d() {
        let db = Database::bundled();
        let cam = db.find_camera("Canon", "EOS 5D Mark III");
        assert!(
            cam.is_some(),
            "Canon EOS 5D Mark III not found in bundled db"
        );
    }

    #[test]
    fn bundled_loader_rejects_invalid_utf8() {
        let mut db = Database::empty();
        let err = db.ingest_bundled_file("broken.xml", &[0xff]).unwrap_err();
        assert!(err.contains("invalid UTF-8"));
        assert!(err.contains("broken.xml"));
    }

    #[test]
    fn bundled_loader_rejects_invalid_xml() {
        let mut db = Database::empty();
        let err = db
            .ingest_bundled_file("broken.xml", br#"<lensdatabase><camera>"#)
            .unwrap_err();
        assert!(err.contains("invalid XML"));
        assert!(err.contains("broken.xml"));
    }

    #[test]
    fn interpolate_distortion_clamping() {
        let entries = vec![
            DistortionEntry {
                focal: 24.0,
                model: DistortionModel::Poly3(Poly3Params { k1: -0.01 }),
            },
            DistortionEntry {
                focal: 70.0,
                model: DistortionModel::Poly3(Poly3Params { k1: 0.01 }),
            },
        ];
        // Below range
        let m = interpolate_distortion(&entries, 10.0).unwrap();
        assert_eq!(m, DistortionModel::Poly3(Poly3Params { k1: -0.01 }));
        // Above range
        let m = interpolate_distortion(&entries, 100.0).unwrap();
        assert_eq!(m, DistortionModel::Poly3(Poly3Params { k1: 0.01 }));
        // Mid
        if let Some(DistortionModel::Poly3(p)) = interpolate_distortion(&entries, 47.0) {
            assert!(
                (p.k1 - 0.0).abs() < 0.001,
                "mid should be near 0, got {}",
                p.k1
            );
        } else {
            panic!("expected Poly3 model");
        }
    }

    #[test]
    fn interpolate_distortion_sorts_by_focal() {
        let entries = vec![
            DistortionEntry {
                focal: 70.0,
                model: DistortionModel::Poly3(Poly3Params { k1: 0.01 }),
            },
            DistortionEntry {
                focal: 24.0,
                model: DistortionModel::Poly3(Poly3Params { k1: -0.01 }),
            },
        ];

        let m = interpolate_distortion(&entries, 47.0).unwrap();
        if let DistortionModel::Poly3(params) = m {
            assert!((params.k1 - 0.0).abs() < 1e-6);
        } else {
            panic!("expected Poly3 model");
        }
    }

    #[test]
    fn interpolate_tca_sorts_by_focal() {
        let entries = vec![
            TcaEntry {
                focal: 70.0,
                model: TcaModel::Linear(TcaLinearParams { kr: 1.02, kb: 0.98 }),
            },
            TcaEntry {
                focal: 24.0,
                model: TcaModel::Linear(TcaLinearParams { kr: 1.00, kb: 1.00 }),
            },
        ];

        let m = interpolate_tca(&entries, 47.0).unwrap();
        if let TcaModel::Linear(params) = m {
            assert!((params.kr - 1.01).abs() < 1e-6);
            assert!((params.kb - 0.99).abs() < 1e-6);
        } else {
            panic!("expected linear TCA model");
        }
    }

    #[test]
    fn interpolate_vignetting_respects_distance() {
        let entries = vec![
            VignettingEntry {
                focal: 35.0,
                aperture: 2.0,
                distance: 1.0,
                params: VignettingParams {
                    k1: -0.1,
                    k2: 0.0,
                    k3: 0.0,
                },
            },
            VignettingEntry {
                focal: 35.0,
                aperture: 2.0,
                distance: 10.0,
                params: VignettingParams {
                    k1: -0.4,
                    k2: 0.0,
                    k3: 0.0,
                },
            },
        ];

        let near = interpolate_vignetting(&entries, 35.0, 2.0, 1.0).unwrap();
        let far = interpolate_vignetting(&entries, 35.0, 2.0, 10.0).unwrap();
        let mid = interpolate_vignetting(&entries, 35.0, 2.0, 5.5).unwrap();

        assert_eq!(near.k1, -0.1);
        assert_eq!(far.k1, -0.4);
        assert!(
            (mid.k1 + 0.25).abs() < 1e-6,
            "expected midpoint interpolation"
        );
    }
}
