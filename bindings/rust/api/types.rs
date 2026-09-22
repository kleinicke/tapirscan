use crate::{
    format::{
        ALL_FORMATS_MASK, COMMON_LINEAR_MASK, COMMON_MASK, LINEAR_MASK, MATRIX_MASK, RETAIL_MASK,
    },
    Format, Image,
};
use serde::Deserialize;
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
    /// Every supported format. Additional formats remain experimental.
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

#[derive(Debug)]
pub(crate) struct EngineScan {
    pub(crate) barcodes: Vec<Barcode>,
    pub(crate) undecoded: Vec<UndecodedRegion>,
    pub(crate) unfinished: bool,
    pub(crate) localization_limited: bool,
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
    /// Include raw engine diagnostics. Undecoded geometry is always included in scan results.
    pub debug: bool,
    /// Allow reader-specific extra work; false by default. Valid for every format.
    /// Exact budgets may evolve; this does not guarantee exhaustive decoding.
    pub extended_budget: bool,
}
/// Four source-image points, with x rightward and y downward from the top left.
pub type Quad = [[f64; 2]; 4];

/// Decoded barcode instance. Equal values at distinct locations remain separate.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Barcode {
    /// Decoded text, excluding the separately reported retail supplement.
    pub text: String,
    /// Detected symbology.
    pub format: Format,
    /// Four points in source-image pixel coordinates.
    pub polygon: Quad,
    /// Uncalibrated, reader-specific evidence; used by [`ScanResult::best`].
    pub support: u64,
    /// Original decoded data bytes where supported, not UTF-8 re-encoded text.
    #[serde(default, rename = "bytes")]
    pub payload_bytes: Option<Vec<u8>>,
    /// Readable two/five-digit retail supplement, or `None` if absent/not requested.
    #[serde(default, rename = "eanAddOn")]
    pub ean_add_on: Option<String>,
    /// Whether the reader identified GS1 semantics; None means unavailable metadata.
    #[serde(default)]
    pub gs1: Option<bool>,
    /// Whether the symbol marks reader initialization; None when unreported.
    #[serde(default, rename = "readerInitialization")]
    pub reader_initialization: Option<bool>,
    /// Multipart sequence metadata where supported; no automatic assembly is performed.
    #[serde(default, rename = "structuredAppend")]
    pub structured_append: Option<StructuredAppend>,
}
impl Barcode {
    /// Enclosing integer pixel bounds as [x, y, width, height].
    #[must_use]
    pub fn rect(&self) -> [f64; 4] {
        let bounds = self.polygon.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            },
        );
        [
            bounds[0].floor(),
            bounds[1].floor(),
            bounds[2].ceil() - bounds[0].floor(),
            bounds[3].ceil() - bounds[1].floor(),
        ]
    }
}
/// Structured-append metadata. Index is one-based; callers assemble sequences.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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
/// Owned scan output, including all decoded instances.
#[derive(Clone, Debug)]
pub struct ScanResult {
    /// All decoded instances, including distinct physical copies with equal text.
    pub barcodes: Vec<Barcode>,
    /// Localized unread geometry; empty means no unread regions were reported.
    pub undecoded: Vec<UndecodedRegion>,
    /// Supplied image dimensions as `[width, height]` in pixels.
    pub image_size: [usize; 2],
    /// Effort mode. Scanner configuration defaults to [`Mode::Medium`].
    pub mode: Mode,
    /// Whole synchronous call time, including input validation and result conversion.
    pub elapsed: Duration,
    /// Reported work limits. False does not guarantee exhaustive scanning.
    pub unfinished: bool,
    /// Present only when [`ScanOptions::debug`] was true for this call.
    pub debug: Option<Diagnostics>,
}
impl ScanResult {
    /// Iterate over decoded instances without allocating.
    pub fn iter(&self) -> std::slice::Iter<'_, Barcode> {
        self.barcodes.iter()
    }

    /// Highest support, preserving the first read on ties. All reads remain available.
    #[must_use]
    pub fn best(&self) -> Option<&Barcode> {
        self.barcodes
            .iter()
            .enumerate()
            .max_by_key(|(i, b)| (b.support, std::cmp::Reverse(*i)))
            .map(|(_, b)| b)
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
        let unfinished = engine.unfinished || engine.localization_limited;
        let debug = engine.diagnostics.map(|raw| Diagnostics { raw });
        Self {
            barcodes,
            undecoded,
            image_size: [image.width, image.height],
            mode,
            elapsed,
            unfinished,
            debug,
        }
    }
}

impl<'a> IntoIterator for &'a ScanResult {
    type Item = &'a Barcode;
    type IntoIter = std::slice::Iter<'a, Barcode>;
    fn into_iter(self) -> Self::IntoIter {
        self.barcodes.iter()
    }
}
impl IntoIterator for ScanResult {
    type Item = Barcode;
    type IntoIter = std::vec::IntoIter<Barcode>;
    fn into_iter(self) -> Self::IntoIter {
        self.barcodes.into_iter()
    }
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
