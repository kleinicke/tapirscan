use crate::Image;
use serde::Deserialize;
use std::{fmt, time::Duration};

/// A supported barcode symbology. Discriminants are native format-mask bits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[repr(u32)]
pub enum Format {
    #[serde(rename = "EAN13")]
    /// EAN-13 retail barcode.
    Ean13 = 1,
    #[serde(rename = "UPCA")]
    /// UPC-A retail barcode.
    Upca = 2,
    #[serde(rename = "EAN8")]
    /// EAN-8 retail barcode.
    Ean8 = 4,
    #[serde(rename = "UPCE")]
    /// UPC-E compressed retail barcode.
    Upce = 8,
    /// Code 128, including GS1-128.
    Code128 = 16,
    /// Code 39.
    Code39 = 32,
    #[serde(rename = "ITF")]
    /// Interleaved 2 of 5.
    Itf = 64,
    /// Codabar.
    Codabar = 128,
    /// Code 93.
    Code93 = 256,
    #[serde(rename = "QRCode")]
    /// QR Code.
    QrCode = 512,
    /// Data Matrix.
    DataMatrix = 1024,
    #[serde(rename = "PDF417")]
    /// PDF417 stacked barcode.
    Pdf417 = 2048,
    /// Aztec Code, including supported Aztec Rune reads.
    Aztec = 4096,
    /// GS1 `DataBar`.
    DataBar = 8192,
    /// GS1 `DataBar` Expanded.
    DataBarExpanded = 16384,
    /// `MaxiCode`.
    MaxiCode = 131_072,
}

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
    pub const COMMON_1D: Self = Self(127);
    /// Common linear formats plus QR Code and Data Matrix.
    pub const COMMON: Self = Self(127 | 512 | 1024);
    /// EAN-13, UPC-A, EAN-8 and UPC-E.
    pub const RETAIL: Self = Self(15);
    /// All supported linear formats, including `DataBar` variants.
    pub const LINEAR: Self = Self(511 | 8192 | 16384);
    /// QR Code, Data Matrix, PDF417, Aztec and `MaxiCode`.
    pub const MATRIX: Self = Self(512 | 1024 | 2048 | 4096 | 131_072);
    /// Every supported format. Additional formats remain experimental.
    pub const ALL: Self = Self(Self::LINEAR.0 | Self::MATRIX.0);
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
    pub(crate) fn from_raw(
        raw: serde_json::Value,
        image: Image<'_>,
        mode: Mode,
        elapsed: Duration,
        debug: bool,
    ) -> Result<Self, Error> {
        let parse_error = |error: serde_json::Error| Error::Engine(error.to_string());
        let barcodes =
            serde_json::from_value(raw["scan"]["barcodes"].clone()).map_err(parse_error)?;
        let unfinished = raw["scan"]["unfinished"]
            .as_bool()
            .ok_or_else(|| Error::Engine("missing work status".into()))?
            || raw["localizationLimited"].as_bool().unwrap_or(false);
        let undecoded = serde_json::from_value(serde_json::Value::Array(engine_unread(&raw)))
            .map_err(parse_error)?;
        let debug = debug.then_some(Diagnostics { raw });
        Ok(Self {
            barcodes,
            undecoded,
            image_size: [image.width, image.height],
            mode,
            elapsed,
            unfinished,
            debug,
        })
    }
}
fn proposals(value: &serde_json::Value, reads: &serde_json::Value) -> Vec<serde_json::Value> {
    let decoded: std::collections::HashSet<_> = reads
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|read| read["candidate_indices"].as_array().into_iter().flatten())
        .filter_map(serde_json::Value::as_u64)
        .collect();
    value
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(i, _)| !decoded.contains(&(*i as u64)))
        .map(|(_, region)| serde_json::json!({"polygon": region["polygon"], "format": null}))
        .collect()
}
fn engine_unread(raw: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(regions) = raw["scan"]["regions"].as_array() {
        let reads = raw["scan"]["barcodes"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        return regions
            .iter()
            .filter(|region| !reads.contains(region))
            .cloned()
            .map(unknown_format)
            .collect();
    }
    let mut unread = proposals(&raw["localization"]["proposals"], &raw["scan"]["barcodes"]);
    for attempt in raw["recovery"]["attempts"].as_array().into_iter().flatten() {
        unread.extend(proposals(&attempt["proposals"], &attempt["reads"]));
    }
    unread
}
fn unknown_format(mut region: serde_json::Value) -> serde_json::Value {
    if region["format"] == "Unknown" {
        region["format"] = serde_json::Value::Null;
    }
    region
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
