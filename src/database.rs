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
    #[serde(rename = "mount", default)]
    mounts: Vec<RawMount>,
    #[serde(rename = "camera", default)]
    cameras: Vec<RawCamera>,
    #[serde(rename = "lens", default)]
    lenses: Vec<RawLens>,
}

/// A top-level `<mount>` compatibility record.
#[derive(Debug, Deserialize)]
struct RawMount {
    #[serde(rename = "name", default)]
    names: Vec<String>,
    #[serde(rename = "compat", default)]
    compatible_mounts: Vec<String>,
}

// ── Public structs ────────────────────────────────────────────────────────────

/// A camera body from the lensfun database.
#[derive(Debug, Clone)]
pub struct Camera {
    /// Manufacturer name (first language-neutral entry).
    maker: String,
    /// Model name (first language-neutral entry).
    model: String,
    /// Mount type.
    mount: String,
    /// Sensor crop factor relative to 35 mm full frame.
    crop_factor: f32,
}

impl Camera {
    /// Manufacturer name.
    pub fn maker(&self) -> &str {
        &self.maker
    }

    /// Model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Mount type.
    pub fn mount(&self) -> &str {
        &self.mount
    }

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
    pub(crate) distortions: Vec<DistortionEntry>,
    pub(crate) tcas: Vec<TcaEntry>,
    pub(crate) vignettings: Vec<VignettingEntry>,
}

impl Calibration {
    /// Build calibration data from distortion, TCA, and vignetting entries.
    pub fn new(
        distortions: Vec<DistortionEntry>,
        tcas: Vec<TcaEntry>,
        vignettings: Vec<VignettingEntry>,
    ) -> Self {
        Self {
            distortions,
            tcas,
            vignettings,
        }
    }

    /// Distortion calibration entries.
    pub fn distortions(&self) -> &[DistortionEntry] {
        &self.distortions
    }

    /// TCA calibration entries.
    pub fn tcas(&self) -> &[TcaEntry] {
        &self.tcas
    }

    /// Vignetting calibration entries.
    pub fn vignettings(&self) -> &[VignettingEntry] {
        &self.vignettings
    }
}

/// A lens from the lensfun database.
#[derive(Debug, Clone)]
pub struct Lens {
    /// Manufacturer name.
    pub(crate) maker: String,
    /// Model name.
    pub(crate) model: String,
    /// Compatible mount names.
    pub(crate) mounts: Vec<String>,
    /// Nominal crop factor.
    pub(crate) crop_factor: Option<f32>,
    /// Available calibration data.
    pub(crate) calibration: Calibration,
}

impl Lens {
    /// Build a lens profile from parsed or synthetic calibration data.
    pub fn new(
        maker: impl Into<String>,
        model: impl Into<String>,
        mounts: Vec<String>,
        crop_factor: Option<f32>,
        calibration: Calibration,
    ) -> Self {
        Self {
            maker: maker.into(),
            model: model.into(),
            mounts,
            crop_factor,
            calibration,
        }
    }

    /// Manufacturer name.
    pub fn maker(&self) -> &str {
        &self.maker
    }

    /// Model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Compatible mount names.
    pub fn mounts(&self) -> &[String] {
        &self.mounts
    }

    /// Nominal crop factor.
    pub fn crop_factor(&self) -> Option<f32> {
        self.crop_factor
    }

    /// Calibration data for this lens.
    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }
}

/// A top-level Lensfun mount compatibility record.
#[derive(Debug, Clone)]
pub struct MountCompatibility {
    /// Mount name.
    mount: String,
    /// Lens mounts accepted by this camera mount.
    compatible_mounts: Vec<String>,
}

impl MountCompatibility {
    /// Mount name.
    pub fn mount(&self) -> &str {
        &self.mount
    }

    /// Lens mounts accepted by this camera mount.
    pub fn compatible_mounts(&self) -> &[String] {
        &self.compatible_mounts
    }
}

/// How a lens mount relates to a camera mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LensMountMatch {
    /// Lens and camera mounts are the same after Lensfun-style normalisation.
    Exact,
    /// The camera mount declares the lens mount as compatible.
    Compatible,
    /// The lens has no mount data, so compatibility could not be verified.
    Unknown,
}

/// A ranked camera-aware lens lookup result.
#[derive(Debug, Clone, Copy)]
pub struct LensMatch<'a> {
    /// Matching lens profile.
    lens: &'a Lens,
    /// Mount compatibility class used for ranking.
    mount_match: LensMountMatch,
    /// Absolute crop-factor delta, or `None` if the lens profile has no crop.
    crop_factor_delta: Option<f32>,
    /// Number of calibration data types present on the profile.
    calibration_types: usize,
    /// Total calibration entry count across all data types.
    calibration_entries: usize,
}

impl<'a> LensMatch<'a> {
    /// Matching lens profile.
    pub fn lens(&self) -> &'a Lens {
        self.lens
    }

    /// Mount compatibility class used for ranking.
    pub fn mount_match(&self) -> LensMountMatch {
        self.mount_match
    }

    /// Absolute crop-factor delta, or `None` if the lens profile has no crop.
    pub fn crop_factor_delta(&self) -> Option<f32> {
        self.crop_factor_delta
    }

    /// Number of calibration data types present on the profile.
    pub fn calibration_types(&self) -> usize {
        self.calibration_types
    }

    /// Total calibration entry count across all data types.
    pub fn calibration_entries(&self) -> usize {
        self.calibration_entries
    }
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

fn convert_mount(raw: RawMount) -> Option<MountCompatibility> {
    let mount = raw.names.into_iter().next()?.trim().to_owned();
    if mount.is_empty() {
        return None;
    }

    Some(MountCompatibility {
        mount,
        compatible_mounts: raw
            .compatible_mounts
            .into_iter()
            .map(|mount| mount.trim().to_owned())
            .filter(|mount| !mount.is_empty())
            .collect(),
    })
}

// ── Database ──────────────────────────────────────────────────────────────────

/// The parsed lensfun database.
///
/// Load from the bundled data with [`Database::bundled`], or parse arbitrary
/// XML with [`Database::load_xml`].
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
    pub(crate) mounts: Vec<MountCompatibility>,
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
            mounts: Vec::new(),
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
    /// db.load_xml(r#"<lensdatabase version="2">
    ///   <camera>
    ///     <maker>Acme</maker><model>Acme X1</model>
    ///     <mount>M42</mount><cropfactor>1.5</cropfactor>
    ///   </camera>
    /// </lensdatabase>"#).unwrap();
    /// assert!(db.find_camera("acme", "x1").is_some());
    /// ```
    pub fn load_xml(&mut self, xml: &str) -> Result<()> {
        self.ingest_xml(xml)
    }

    fn ingest_xml(&mut self, xml: &str) -> Result<()> {
        let raw: RawLensDatabase = quick_xml::de::from_str(xml)?;
        for mount in raw.mounts {
            if let Some(mount) = convert_mount(mount) {
                self.mounts.push(mount);
            }
        }
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
            mounts: Vec::new(),
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

    /// All top-level mount compatibility records in the database.
    pub fn mounts(&self) -> &[MountCompatibility] {
        &self.mounts
    }

    /// Return whether a camera mount can accept a lens mount.
    ///
    /// Direct mount equality is accepted first. If the database contains a
    /// top-level compatibility record for the camera mount, its `<compat>`
    /// entries are also accepted.
    pub fn mount_accepts_lens(&self, camera_mount: &str, lens_mount: &str) -> bool {
        self.mount_match(camera_mount, lens_mount).is_some()
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

    /// Find the best lens match for a specific camera.
    ///
    /// Candidate lenses are matched by maker and model like [`Self::find_lens`],
    /// then filtered to lenses whose mount is compatible with the camera. When
    /// the database has duplicate same-name lens entries, this ranks exact mount
    /// matches ahead of entries with missing mount data, then prefers the lens
    /// crop factor closest to the camera crop factor. Calibration coverage is
    /// used as a final tie-breaker.
    ///
    /// # Example
    ///
    /// ```
    /// let db = dioptric::Database::bundled();
    /// let camera = db.find_camera("Canon", "EOS 5D Mark III").unwrap();
    /// let lens = db.find_lens_for_camera(
    ///     camera,
    ///     "Canon",
    ///     "EF 24-70mm f/2.8L II USM",
    /// );
    /// assert!(lens.is_some());
    /// ```
    pub fn find_lens_for_camera(
        &self,
        camera: &Camera,
        maker_query: &str,
        model_query: &str,
    ) -> Option<&Lens> {
        self.find_lenses_for_camera(camera, maker_query, model_query)
            .into_iter()
            .next()
            .map(|lens_match| lens_match.lens)
    }

    /// Find ranked lens matches for a specific camera.
    ///
    /// The returned matches are sorted in the same order used by
    /// [`Self::find_lens_for_camera`]. Profiles calibrated for a smaller
    /// sensor than the camera are excluded, matching Lensfun's lookup
    /// semantics for full-frame bodies versus crop-only calibrations.
    pub fn find_lenses_for_camera<'a>(
        &'a self,
        camera: &Camera,
        maker_query: &str,
        model_query: &str,
    ) -> Vec<LensMatch<'a>> {
        let mut matches: Vec<_> = self
            .lenses
            .iter()
            .filter(|lens| {
                fuzzy_contains(&lens.maker, maker_query) && fuzzy_contains(&lens.model, model_query)
            })
            .filter_map(|lens| self.lens_match_for_camera(lens, camera))
            .collect();
        matches.sort_by(compare_lens_matches);
        matches
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

    /// Find the best single-string lens match for a specific camera.
    ///
    /// This is the camera-aware variant of [`Self::find_lens_by_name`]. It is
    /// useful when EXIF metadata provides a camera body plus a single
    /// `LensModel` value without a separate lens maker field.
    pub fn find_lens_by_name_for_camera(&self, camera: &Camera, query: &str) -> Option<&Lens> {
        self.find_lenses_by_name_for_camera(camera, query)
            .into_iter()
            .next()
            .map(|lens_match| lens_match.lens)
    }

    /// Find ranked camera-aware lens matches using a single query string.
    ///
    /// This is the ranked variant of [`Self::find_lens_by_name_for_camera`].
    pub fn find_lenses_by_name_for_camera<'a>(
        &'a self,
        camera: &Camera,
        query: &str,
    ) -> Vec<LensMatch<'a>> {
        let mut matches: Vec<_> = self
            .lenses
            .iter()
            .filter(|lens| {
                let full = format!("{} {}", lens.maker, lens.model);
                fuzzy_contains(&full, query)
            })
            .filter_map(|lens| self.lens_match_for_camera(lens, camera))
            .collect();
        matches.sort_by(compare_lens_matches);
        matches
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

    fn lens_match_for_camera<'a>(
        &'a self,
        lens: &'a Lens,
        camera: &Camera,
    ) -> Option<LensMatch<'a>> {
        if !lens_crop_is_usable(lens, camera) {
            return None;
        }

        let mount_match = if lens.mounts.is_empty() {
            LensMountMatch::Unknown
        } else {
            lens.mounts
                .iter()
                .filter_map(|mount| self.mount_match(&camera.mount, mount))
                .min()?
        };

        Some(LensMatch {
            lens,
            mount_match,
            crop_factor_delta: lens
                .crop_factor
                .map(|crop_factor| (crop_factor - camera.crop_factor).abs()),
            calibration_types: calibration_type_count(lens),
            calibration_entries: calibration_entry_count(lens),
        })
    }

    fn mount_match(&self, camera_mount: &str, lens_mount: &str) -> Option<LensMountMatch> {
        if mount_matches(camera_mount, lens_mount) {
            return Some(LensMountMatch::Exact);
        }

        self.mounts
            .iter()
            .find(|mount| mount_matches(&mount.mount, camera_mount))
            .and_then(|mount| {
                mount
                    .compatible_mounts
                    .iter()
                    .any(|compatible| mount_matches(compatible, lens_mount))
                    .then_some(LensMountMatch::Compatible)
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

fn mount_matches(lens_mount: &str, camera_mount: &str) -> bool {
    normalise_search_text(&lens_mount.to_lowercase())
        == normalise_search_text(&camera_mount.to_lowercase())
}

fn lens_crop_is_usable(lens: &Lens, camera: &Camera) -> bool {
    const EPSILON: f32 = 1e-4;
    lens.crop_factor
        .map(|crop_factor| crop_factor <= camera.crop_factor + EPSILON)
        .unwrap_or(true)
}

fn compare_lens_matches(a: &LensMatch<'_>, b: &LensMatch<'_>) -> std::cmp::Ordering {
    a.mount_match
        .cmp(&b.mount_match)
        .then_with(|| crop_delta_sort_key(*a).total_cmp(&crop_delta_sort_key(*b)))
        .then_with(|| b.calibration_types.cmp(&a.calibration_types))
        .then_with(|| b.calibration_entries.cmp(&a.calibration_entries))
}

fn crop_delta_sort_key(lens_match: LensMatch<'_>) -> f32 {
    lens_match.crop_factor_delta.unwrap_or(f32::INFINITY)
}

fn calibration_type_count(lens: &Lens) -> usize {
    usize::from(!lens.calibration.distortions.is_empty())
        + usize::from(!lens.calibration.tcas.is_empty())
        + usize::from(!lens.calibration.vignettings.is_empty())
}

fn calibration_entry_count(lens: &Lens) -> usize {
    lens.calibration.distortions.len()
        + lens.calibration.tcas.len()
        + lens.calibration.vignettings.len()
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
        db.load_xml(xml).expect("parse should succeed");
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
        db.load_xml(xml).expect("parse should succeed");
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
        db.load_xml(xml).expect("parse should succeed");
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
        db.load_xml(xml).unwrap();
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
        db.load_xml(xml).unwrap();

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
    fn find_lens_for_camera_prefers_matching_crop() {
        let xml = r#"<lensdatabase version="2">
  <camera>
    <maker>Canon</maker>
    <model>Canon Full Frame</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </camera>
  <camera>
    <maker>Canon</maker>
    <model>Canon APS-C</model>
    <mount>Canon EF</mount>
    <cropfactor>1.6</cropfactor>
  </camera>
  <lens>
    <maker>Canon</maker>
    <model>Canon EF 35mm f/2 IS USM</model>
    <mount>Canon EF</mount>
    <cropfactor>1.6</cropfactor>
    <calibration>
      <vignetting model="pa" focal="35" aperture="2" distance="10" k1="-0.8" k2="0.2" k3="-0.1"/>
    </calibration>
  </lens>
  <lens>
    <maker>Canon</maker>
    <model>Canon EF 35mm f/2 IS USM</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
    <calibration>
      <distortion model="ptlens" focal="35" a="0.01" b="-0.03" c="0.02"/>
      <tca model="linear" focal="35" kr="1.001" kb="0.999"/>
    </calibration>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.load_xml(xml).unwrap();

        let full_frame = db.find_camera("Canon", "Full Frame").unwrap();
        let aps_c = db.find_camera("Canon", "APS-C").unwrap();

        let first = db.find_lens("Canon", "EF35mm f2").unwrap();
        assert_eq!(first.crop_factor, Some(1.6));

        let full_frame_lens = db
            .find_lens_for_camera(full_frame, "Canon", "EF35mm f2")
            .unwrap();
        assert_eq!(full_frame_lens.crop_factor, Some(1.0));
        assert_eq!(full_frame_lens.calibration.distortions.len(), 1);
        assert_eq!(full_frame_lens.calibration.tcas.len(), 1);

        let full_frame_matches = db.find_lenses_for_camera(full_frame, "Canon", "EF35mm f2");
        assert_eq!(
            full_frame_matches.len(),
            1,
            "full-frame lookup must reject crop-only calibrations"
        );

        let aps_c_lens = db
            .find_lens_by_name_for_camera(aps_c, "Canon EF35mm f2")
            .unwrap();
        assert_eq!(aps_c_lens.crop_factor, Some(1.6));
        assert_eq!(aps_c_lens.calibration.vignettings.len(), 1);
    }

    #[test]
    fn find_lens_for_camera_filters_incompatible_mounts() {
        let xml = r#"<lensdatabase version="2">
  <camera>
    <maker>Canon</maker>
    <model>Canon Body</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </camera>
  <lens>
    <maker>Sigma</maker>
    <model>Sigma 50mm f/1.4</model>
    <mount>Nikon F</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
  <lens>
    <maker>Sigma</maker>
    <model>Sigma 50mm f/1.4</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.load_xml(xml).unwrap();
        let camera = db.find_camera("Canon", "Body").unwrap();

        let first = db.find_lens("Sigma", "50mm").unwrap();
        assert_eq!(first.mounts, vec!["Nikon F".to_owned()]);

        let camera_lens = db.find_lens_for_camera(camera, "Sigma", "50mm").unwrap();
        assert_eq!(camera_lens.mounts, vec!["Canon EF".to_owned()]);
    }

    #[test]
    fn find_lenses_for_camera_uses_mount_compatibility_and_ranking() {
        let xml = r#"<lensdatabase version="2">
  <mount>
    <name>Canon RF</name>
    <compat>Canon EF</compat>
  </mount>
  <camera>
    <maker>Canon</maker>
    <model>Canon RF Body</model>
    <mount>Canon RF</mount>
    <cropfactor>1.0</cropfactor>
  </camera>
  <lens>
    <maker>Canon</maker>
    <model>Canon 50mm f/1.8</model>
    <mount>Canon EF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
  <lens>
    <maker>Canon</maker>
    <model>Canon 50mm f/1.8</model>
    <mount>Canon RF</mount>
    <cropfactor>1.0</cropfactor>
  </lens>
</lensdatabase>"#;
        let mut db = Database::empty();
        db.load_xml(xml).unwrap();
        let camera = db.find_camera("Canon", "RF Body").unwrap();

        assert!(db.mount_accepts_lens("Canon RF", "Canon EF"));
        assert_eq!(db.mounts().len(), 1);

        let matches = db.find_lenses_for_camera(camera, "Canon", "50mm");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].mount_match, LensMountMatch::Exact);
        assert_eq!(matches[0].lens.mounts, vec!["Canon RF".to_owned()]);
        assert_eq!(matches[1].mount_match, LensMountMatch::Compatible);
        assert_eq!(matches[1].lens.mounts, vec!["Canon EF".to_owned()]);
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
        db.load_xml(xml).unwrap();

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
