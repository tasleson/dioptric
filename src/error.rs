use thiserror::Error;

/// Errors that can occur when using dioptric.
#[derive(Debug, Error)]
pub enum Error {
    /// The requested lens was not found in the database.
    #[error("lens not found: {0}")]
    LensNotFound(String),

    /// The requested camera was not found in the database.
    #[error("camera not found: {0}")]
    CameraNotFound(String),

    /// The focal length, aperture, or distance value is out of range or invalid.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    /// An XML parsing error occurred while reading the lensfun database.
    #[error("XML parse error: {0}")]
    XmlParse(#[from] quick_xml::DeError),

    /// A calibration entry references an unknown distortion or TCA model name.
    #[error("unknown model '{0}'")]
    UnknownModel(String),

    /// The input image format is not supported by the current image pipeline.
    #[error("unsupported image format: {0}")]
    UnsupportedImageFormat(String),

    /// The supplied buffer length does not match `width × height × channels`.
    #[error(
        "buffer length {actual} does not match expected {expected} (width={width} × height={height} × channels={channels})"
    )]
    InvalidBufferLength {
        expected: usize,
        actual: usize,
        width: u32,
        height: u32,
        channels: u32,
    },

    /// The lens calibration has no entries for the requested correction type.
    #[error("no calibration data available for {0}")]
    NoCalibration(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
