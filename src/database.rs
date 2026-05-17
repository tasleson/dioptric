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
    #[serde(rename = "@kr", default)]
    kr: f32,
    #[serde(rename = "@kb", default)]
    kb: f32,
    // poly3 model
    #[serde(rename = "@vr", default)]
    vr: f32,
    #[serde(rename = "@br", default)]
    br: f32,
    #[serde(rename = "@vb", default)]
    vb: f32,
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

#[derive(Debug, Deserialize, Default)]
struct RawCalibration {
    #[serde(rename = "distortion", default)]
    distortions: Vec<RawDistortion>,
    #[serde(rename = "tca", default)]
    tcas: Vec<RawTca>,
    #[serde(rename = "vignetting", default)]
    vignetings: Vec<RawVignetting>,
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
    pub vignetings: Vec<VignettingEntry>,
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
            br: raw.br,
            vb: raw.vb,
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
        for d in &raw_cal.distortions {
            // Skip entries with unknown model names
            if let Ok(model) = parse_distortion(d) {
                cal.distortions.push(DistortionEntry {
                    focal: d.focal,
                    model,
                });
            }
        }
        for t in &raw_cal.tcas {
            if let Ok(model) = parse_tca(t) {
                cal.tcas.push(TcaEntry {
                    focal: t.focal,
                    model,
                });
            }
        }
        for v in &raw_cal.vignetings {
            if v.model == "pa" {
                cal.vignetings.push(VignettingEntry {
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
/// XML with [`Database::from_xml`] and [`Database::from_xml_files`].
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
            if let Ok(text) = std::str::from_utf8(entry.contents()) {
                // Best-effort: skip files that fail to parse
                let _ = db.ingest_xml(text);
            }
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
    /// respective camera fields.  Returns the first match.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let cam = db.find_camera("Canon", "EOS 5D Mark III");
    /// assert!(cam.is_some());
    /// ```
    pub fn find_camera(&self, maker_query: &str, model_query: &str) -> Option<&Camera> {
        let mq = maker_query.to_lowercase();
        let mq2 = model_query.to_lowercase();
        self.cameras
            .iter()
            .find(|c| c.maker.to_lowercase().contains(&mq) && c.model.to_lowercase().contains(&mq2))
    }

    /// Find a lens by maker and model using case-insensitive substring matching.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let lens = db.find_lens("Canon", "EF 24-70mm f/2.8L II USM");
    /// assert!(lens.is_some());
    /// ```
    pub fn find_lens(&self, maker_query: &str, model_query: &str) -> Option<&Lens> {
        let mq = maker_query.to_lowercase();
        let mq2 = model_query.to_lowercase();
        self.lenses
            .iter()
            .find(|l| l.maker.to_lowercase().contains(&mq) && l.model.to_lowercase().contains(&mq2))
    }
}

// ── Interpolation helpers (pub(crate)) ────────────────────────────────────────

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

/// Bilinear interpolation of vignetting parameters over focal × aperture.
///
/// Distance is not interpolated — the entry with the largest distance
/// (infinite focus) is preferred for each (focal, aperture) pair.
pub(crate) fn interpolate_vignetting(
    entries: &[VignettingEntry],
    focal: f32,
    aperture: f32,
) -> Option<VignettingParams> {
    if entries.is_empty() {
        return None;
    }

    // Collapse distance: for each (focal, aperture) keep the entry with the
    // greatest distance (closest to infinity).
    let mut deduped: Vec<(f32, f32, VignettingParams)> = Vec::new();
    for e in entries {
        if let Some(existing) = deduped
            .iter_mut()
            .find(|(f, a, _)| (*f - e.focal).abs() < 1e-4 && (*a - e.aperture).abs() < 1e-4)
        {
            // Keep the largest distance (most representative of typical shooting)
            let old_dist = entries
                .iter()
                .find(|x| {
                    (x.focal - existing.0).abs() < 1e-4 && (x.aperture - existing.1).abs() < 1e-4
                })
                .map(|x| x.distance)
                .unwrap_or(0.0);
            if e.distance > old_dist {
                existing.2 = e.params;
            }
        } else {
            deduped.push((e.focal, e.aperture, e.params));
        }
    }

    if deduped.is_empty() {
        return None;
    }

    // Unique focal lengths
    let mut focals: Vec<f32> = deduped.iter().map(|(f, _, _)| *f).collect();
    focals.sort_by(f32::total_cmp);
    focals.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    // Unique apertures
    let mut apertures: Vec<f32> = deduped.iter().map(|(_, a, _)| *a).collect();
    apertures.sort_by(f32::total_cmp);
    apertures.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    // For a given (f, a) pair, look up the params (or None if not present)
    let lookup = |f: f32, a: f32| -> Option<VignettingParams> {
        deduped
            .iter()
            .find(|(df, da, _)| (*df - f).abs() < 1e-4 && (*da - a).abs() < 1e-4)
            .map(|(_, _, p)| *p)
    };

    // Interpolate along the focal axis for each aperture bound
    fn interp_focal(
        focals: &[f32],
        focal: f32,
        aperture: f32,
        lookup: &dyn Fn(f32, f32) -> Option<VignettingParams>,
    ) -> Option<VignettingParams> {
        if focals.is_empty() {
            return None;
        }
        let clamped_f = focal.clamp(focals[0], focals[focals.len() - 1]);
        if focals.len() == 1 {
            return lookup(focals[0], aperture);
        }
        let idx = focals.partition_point(|&x| x < clamped_f);
        let (f0, f1) = if idx == 0 {
            (focals[0], focals[1])
        } else if idx >= focals.len() {
            (focals[focals.len() - 2], focals[focals.len() - 1])
        } else {
            (focals[idx - 1], focals[idx])
        };
        let t = if (f1 - f0).abs() < 1e-6 {
            0.0
        } else {
            (clamped_f - f0) / (f1 - f0)
        };
        let p0 = lookup(f0, aperture)?;
        let p1 = lookup(f1, aperture)?;
        Some(VignettingParams::lerp(p0, p1, t))
    }

    // Clamp aperture
    let clamped_a = aperture.clamp(apertures[0], apertures[apertures.len() - 1]);
    if apertures.len() == 1 {
        return interp_focal(&focals, focal, apertures[0], &lookup);
    }

    let aidx = apertures.partition_point(|&x| x < clamped_a);
    let (a0, a1) = if aidx == 0 {
        (apertures[0], apertures[1])
    } else if aidx >= apertures.len() {
        (
            apertures[apertures.len() - 2],
            apertures[apertures.len() - 1],
        )
    } else {
        (apertures[aidx - 1], apertures[aidx])
    };
    let at = if (a1 - a0).abs() < 1e-6 {
        0.0
    } else {
        (clamped_a - a0) / (a1 - a0)
    };

    let p0 = interp_focal(&focals, focal, a0, &lookup)?;
    let p1 = interp_focal(&focals, focal, a1, &lookup)?;
    Some(VignettingParams::lerp(p0, p1, at))
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
}
