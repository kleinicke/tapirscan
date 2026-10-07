use crate::{
    Format, Image,
    format::{
        ALL_FORMATS_MASK, COMMON_LINEAR_MASK, COMMON_MASK, LINEAR_MASK, MATRIX_MASK, RETAIL_MASK,
    },
};
use serde::{Deserialize, Serialize};
use std::{fmt, time::Duration};

/// Nonempty format selection with presets and composable individual formats.
///
/// Use `Format::Ean13.into()` for one format, or combine formats/presets with `|`.
/// `Formats::try_from(u32)` accepts the enum's native bit mask and rejects empty
/// masks, supplement-policy bits and unknown bits with [`Error::InvalidOptions`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Formats(u32);
impl Formats {
    pub(crate) fn bits(self) -> u32 {
        self.0
    }
    /// Retail formats plus Code 128, Code 39 and ITF.
    pub const COMMON_1D: Self = Self(COMMON_LINEAR_MASK);
    /// Common linear formats plus QR Code and Data Matrix.
    pub const COMMON: Self = Self(COMMON_MASK);
    /// EAN-13, UPC-A, EAN-8 and UPC-E.
    pub const RETAIL: Self = Self(RETAIL_MASK);
    /// All supported linear formats, including `DataBar` variants.
    pub const LINEAR: Self = Self(LINEAR_MASK);
    /// QR Code, Data Matrix, PDF417, Aztec and `MaxiCode`.
    pub const MATRIX: Self = Self(MATRIX_MASK);
    /// Every supported format.
    pub const ALL: Self = Self(ALL_FORMATS_MASK);
}
impl TryFrom<u32> for Formats {
    type Error = Error;
    fn try_from(bits: u32) -> Result<Self, Error> {
        if bits == 0 || bits & !Self::ALL.0 != 0 {
            return Err(Error::InvalidOptions(
                "empty or unsupported format selection",
            ));
        }
        Ok(Self(bits))
    }
}
impl From<Format> for Formats {
    fn from(format: Format) -> Self {
        Self(format as u32)
    }
}
impl std::ops::BitOr for Format {
    type Output = Formats;
    fn bitor(self, rhs: Self) -> Formats {
        Formats(self as u32 | rhs as u32)
    }
}
impl std::ops::BitOr<Format> for Formats {
    type Output = Self;
    fn bitor(self, rhs: Format) -> Self {
        Self(self.0 | rhs as u32)
    }
}
impl std::ops::BitOr for Formats {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOr<Formats> for Format {
    type Output = Formats;
    fn bitor(self, rhs: Formats) -> Formats {
        rhs | self
    }
}

#[derive(Debug)]
pub(crate) struct EngineScan {
    pub(crate) barcodes: Vec<Barcode>,
    pub(crate) undecoded: Vec<UndecodedRegion>,
    pub(crate) diagnostics: Option<serde_json::Value>,
}

/// Work effort. Medium is the default, matching Python and JavaScript.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    /// Least work, intended for clear images and inexpensive retries.
    Low,
    /// Default balance of work and recovery.
    #[default]
    Medium,
    /// Additional effort and source-detail recovery.
    High,
    /// Highest available effort, including an additional localization grid.
    VeryHigh,
}
impl Mode {
    /// Stable schema name used by language adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::VeryHigh => "very-high",
        }
    }
}

/// Whether to read the adjacent two/five-digit EAN/UPC supplement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EanAddOnPolicy {
    /// Decode the main retail value without searching for a supplement (default).
    #[default]
    Ignore,
    /// Read a supplement when possible; keep the main value if none is readable.
    Read,
    /// Return retail reads only with a readable supplement; other formats are unaffected.
    Require,
}

/// Scanner configuration; per-image settings live in [`ScanOptions`].
#[derive(Clone, Copy, Debug)]
pub struct ScannerOptions {
    /// Effort mode. Scanner configuration defaults to [`Mode::Medium`].
    pub mode: Mode,
    /// Default readers, initially all retail formats.
    pub formats: Formats,
    /// Retail supplement policy, initially [`EanAddOnPolicy::Ignore`].
    pub ean_add_on_policy: EanAddOnPolicy,
}
impl Default for ScannerOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Medium,
            formats: Formats::RETAIL,
            ean_add_on_policy: EanAddOnPolicy::Ignore,
        }
    }
}
/// Optional overrides for a single image.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    /// Per-call reader override; `None` (default) uses the scanner configuration.
    pub formats: Option<Formats>,
}
/// Four source-image points, with x rightward and y downward from the top left.
pub type Quad = [[f64; 2]; 4];

/// Decoded barcode instance. Equal values at distinct locations remain separate.
///
/// Serde uses the shared barcode schema; absent optional metadata is omitted.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Barcode {
    /// Decoded text, excluding the separately reported retail supplement.
    pub text: String,
    /// Detected symbology.
    pub format: Format,
    /// Four points in source-image pixel coordinates.
    pub polygon: Quad,
    /// Uncalibrated, reader-specific evidence; used by `best()`.
    pub support: u64,
    /// Original decoded data bytes where supported, not UTF-8 re-encoded text.
    #[serde(default, rename = "bytes", skip_serializing_if = "Option::is_none")]
    pub payload_bytes: Option<Vec<u8>>,
    /// Readable two/five-digit retail supplement, or `None` if absent/not requested.
    #[serde(default, rename = "eanAddOn", skip_serializing_if = "Option::is_none")]
    pub ean_add_on: Option<String>,
    /// Whether the reader identified GS1 semantics; None means unavailable metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gs1: Option<bool>,
    /// Whether the symbol marks reader initialization; None when unreported.
    #[serde(
        default,
        rename = "readerInitialization",
        skip_serializing_if = "Option::is_none"
    )]
    pub reader_initialization: Option<bool>,
    /// Multipart sequence metadata where supported; no automatic assembly is performed.
    #[serde(
        default,
        rename = "structuredAppend",
        skip_serializing_if = "Option::is_none"
    )]
    pub structured_append: Option<StructuredAppend>,
}
impl Barcode {
    /// Enclosing integer pixel bounds: floor of the minimum to ceil of the maximum.
    /// Values outside the `i32` range saturate, for barcodes built by callers.
    #[must_use]
    // `as` saturates out-of-range floats and maps NaN to 0, which is the intent here.
    #[allow(clippy::cast_possible_truncation)]
    pub fn rect(&self) -> Rect {
        let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
        let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for [x, y] in self.polygon {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
        let (left, top) = (left.floor() as i32, top.floor() as i32);
        // Subtract in f64: the integer difference can exceed i32 before saturating.
        Rect {
            left,
            top,
            width: (right.ceil() - f64::from(left)) as i32,
            height: (bottom.ceil() - f64::from(top)) as i32,
        }
    }
}
/// Enclosing integer pixel bounds of a barcode polygon.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Rect {
    /// Leftmost pixel column.
    pub left: i32,
    /// Topmost pixel row.
    pub top: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}
/// Structured-append metadata. Index is one-based; callers assemble sequences.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct StructuredAppend {
    /// One-based position in the sequence.
    pub index: usize,
    /// Declared number of symbols in the sequence.
    pub count: usize,
    /// Reader-provided sequence identifier, when available.
    pub id: Option<String>,
    /// Reader-provided sequence parity byte, when available.
    pub parity: Option<u8>,
}
/// Localized but unread region, distinct from a decoded barcode.
#[derive(Clone, Debug, Deserialize)]
pub struct UndecodedRegion {
    /// Suspected symbology, or `None` when unknown.
    pub format: Option<Format>,
    /// Four points in source-image pixel coordinates.
    pub polygon: Quad,
}
/// Optional search evidence. Raw reader schema is separate from the public API.
#[derive(Clone, Debug)]
pub struct Diagnostics {
    /// Unstable engine-specific JSON, including proposals, work counters and recovery.
    /// Ordinary scanning and drawing barcodes do not require inspecting this value.
    pub raw: serde_json::Value,
}
/// Decoded values and their source-image locations, without diagnostic collection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScanResult {
    /// All decoded instances; equal values at different locations stay separate.
    pub barcodes: Vec<Barcode>,
}
impl ScanResult {
    /// Borrow decoded text in scanner order, including repeated values.
    #[must_use]
    pub fn values(&self) -> impl ExactSizeIterator<Item = &str> {
        self.barcodes.iter().map(|barcode| barcode.text.as_str())
    }
    /// Highest support, keeping the first tie.
    #[must_use]
    pub fn best(&self) -> Option<&Barcode> {
        best(&self.barcodes)
    }
}

/// Owned scan output, including all decoded instances.
#[derive(Clone, Debug)]
pub struct InspectionResult {
    /// All decoded instances, including distinct physical copies with equal text.
    pub barcodes: Vec<Barcode>,
    /// Localized unread geometry; empty means no unread regions were reported.
    pub undecoded: Vec<UndecodedRegion>,
    /// Supplied image dimensions as `[width, height]` in pixels.
    pub image_size: [usize; 2],
    /// Effort mode the scanner used.
    pub mode: Mode,
    /// Whole synchronous call time, including input validation and result conversion.
    pub elapsed: Duration,
    /// Engine evidence returned by inspection; the schema is unstable.
    pub diagnostics: Option<Diagnostics>,
}
impl InspectionResult {
    /// Highest support, preserving the first read on ties.
    #[must_use]
    pub fn best(&self) -> Option<&Barcode> {
        best(&self.barcodes)
    }
    /// Borrow decoded text without allocating a second collection.
    #[must_use]
    pub fn values(&self) -> impl ExactSizeIterator<Item = &str> {
        self.barcodes.iter().map(|barcode| barcode.text.as_str())
    }
    pub(crate) fn from_engine(
        engine: EngineScan,
        image: Image<'_>,
        mode: Mode,
        elapsed: Duration,
    ) -> Self {
        let barcodes = engine.barcodes;
        let undecoded = engine.undecoded;
        let diagnostics = engine.diagnostics.map(|raw| Diagnostics { raw });
        Self {
            barcodes,
            undecoded,
            image_size: [image.width, image.height],
            mode,
            elapsed,
            diagnostics,
        }
    }
}

/// Highest support, preserving the first read on ties; `None` when empty.
fn best(barcodes: &[Barcode]) -> Option<&Barcode> {
    barcodes
        .iter()
        .enumerate()
        .max_by_key(|(i, b)| (b.support, std::cmp::Reverse(*i)))
        .map(|(_, b)| b)
}

/// Invalid caller input or a failed internal reader. No detection is a successful empty result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Invalid dimensions, stride or buffer size; carries a descriptive message.
    InvalidImage(&'static str),
    /// Invalid format selection or incompatible options; carries a descriptive message.
    InvalidOptions(&'static str),
    /// Reader failure or malformed internal output; carries the engine message.
    Engine(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage(message) | Self::InvalidOptions(message) => f.write_str(message),
            Self::Engine(message) => write!(f, "scanner failed: {message}"),
        }
    }
}
impl std::error::Error for Error {}
