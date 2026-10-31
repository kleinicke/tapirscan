use crate::{
    Format,
    format::{
        ALL_FORMATS_MASK, COMMON_LINEAR_MASK, COMMON_MASK, LINEAR_MASK, MATRIX_MASK, RETAIL_MASK,
        SUPPORTED_MASK,
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
    /// All linear formats except the opt-in ones, including `DataBar` variants.
    pub const LINEAR: Self = Self(LINEAR_MASK);
    /// QR Code, Data Matrix, PDF417, Aztec, `MaxiCode`, Micro QR and rMQR.
    pub const MATRIX: Self = Self(MATRIX_MASK);
    /// Every format except the opt-in formats, which are selected by name.
    pub const ALL: Self = Self(ALL_FORMATS_MASK);
}
impl TryFrom<u32> for Formats {
    type Error = Error;
    fn try_from(bits: u32) -> Result<Self, Error> {
        if bits == 0 || bits & !SUPPORTED_MASK != 0 {
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
///
/// Minor releases may add fields with defaults, so build it from the defaults:
///
/// ```
/// use tapirscan::{Mode, ScannerOptions};
/// let options = ScannerOptions {
///     mode: Mode::High,
///     ..ScannerOptions::default()
/// };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ScannerOptions {
    /// Effort mode. Scanner configuration defaults to [`Mode::Medium`].
    pub mode: Mode,
    /// Default readers, initially the common formats.
    pub formats: Formats,
    /// Retail supplement policy, initially [`EanAddOnPolicy::Ignore`].
    pub ean_add_on_policy: EanAddOnPolicy,
    /// Run one scan's formats on separate threads where the host allows it.
    ///
    /// Results are identical either way. Set `false` when you already run one
    /// scanner per core. The default is `true` unless the process environment sets
    /// `TAPIRSCAN_THREADS` to `0` or `1`. WebAssembly is single-threaded and
    /// ignores the option.
    pub parallel: bool,
}
/// The process-wide default of [`ScannerOptions::parallel`].
fn default_parallel() -> bool {
    static DEFAULT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEFAULT.get_or_init(|| {
        std::env::var("TAPIRSCAN_THREADS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .is_none_or(|threads| threads > 1)
    })
}
impl Default for ScannerOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Medium,
            formats: Formats::COMMON,
            ean_add_on_policy: EanAddOnPolicy::Ignore,
            parallel: default_parallel(),
        }
    }
}
/// Optional overrides for a single image.
///
/// Minor releases may add fields with defaults; build it with
/// `..ScanOptions::default()`.
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
/// Serializing also parses [`content`](Barcode::content) and writes it as
/// `{"kind": ..., "fields": [{"name": ..., "value": ...}]}`, omitted for plain
/// text; deserializing ignores it, so a round trip recomputes it from `text`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[non_exhaustive]
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
    /// Codabar start and stop characters (`A` to `D`) in reading order, such as
    /// `"AB"`; `text` omits them. `None` for other formats.
    #[serde(
        default,
        rename = "codabarStartStop",
        skip_serializing_if = "Option::is_none"
    )]
    pub codabar_start_stop: Option<String>,
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
impl Serialize for Barcode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Content parsing runs only here, so scanning and typed access never pay for it.
        #[derive(Serialize)]
        struct Wire<'a> {
            text: &'a str,
            format: Format,
            polygon: Quad,
            support: u64,
            #[serde(rename = "bytes", skip_serializing_if = "Option::is_none")]
            payload_bytes: Option<&'a [u8]>,
            #[serde(rename = "eanAddOn", skip_serializing_if = "Option::is_none")]
            ean_add_on: Option<&'a str>,
            #[serde(rename = "codabarStartStop", skip_serializing_if = "Option::is_none")]
            codabar_start_stop: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            gs1: Option<bool>,
            #[serde(
                rename = "readerInitialization",
                skip_serializing_if = "Option::is_none"
            )]
            reader_initialization: Option<bool>,
            #[serde(rename = "structuredAppend", skip_serializing_if = "Option::is_none")]
            structured_append: Option<&'a StructuredAppend>,
            #[serde(skip_serializing_if = "Option::is_none")]
            content: Option<crate::Content>,
        }
        Wire {
            text: &self.text,
            format: self.format,
            polygon: self.polygon,
            support: self.support,
            payload_bytes: self.payload_bytes.as_deref(),
            ean_add_on: self.ean_add_on.as_deref(),
            codabar_start_stop: self.codabar_start_stop.as_deref(),
            gs1: self.gs1,
            reader_initialization: self.reader_initialization,
            structured_append: self.structured_append.as_ref(),
            content: self.content(),
        }
        .serialize(serializer)
    }
}
impl Barcode {
    /// Structured content of the decoded text, such as GS1 elements, a Wi-Fi
    /// configuration or a payment slip; `None` for unstructured text.
    /// See [`parse_content`](crate::parse_content).
    #[must_use]
    pub fn content(&self) -> Option<crate::Content> {
        crate::parse_content(&self.text, self.gs1 == Some(true))
    }
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
#[non_exhaustive]
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
#[non_exhaustive]
pub struct UndecodedRegion {
    /// Suspected symbology, or `None` when unknown.
    pub format: Option<Format>,
    /// Four points in source-image pixel coordinates.
    pub polygon: Quad,
}
/// Optional search evidence. Raw reader schema is separate from the public API.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Diagnostics {
    /// Unstable engine-specific JSON, including proposals, work counters and recovery.
    /// Ordinary scanning and drawing barcodes do not require inspecting this value.
    pub raw: serde_json::Value,
}
/// Decoded values and their source-image locations, without diagnostic collection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ScanResult {
    /// All decoded instances; equal values at different locations stay separate.
    pub barcodes: Vec<Barcode>,
}
impl ScanResult {
    /// Borrow decoded text in scanner order, including repeated values.
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
#[non_exhaustive]
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
    pub fn values(&self) -> impl ExactSizeIterator<Item = &str> {
        self.barcodes.iter().map(|barcode| barcode.text.as_str())
    }
    /// Map geometry from a reduced scan back to the supplied image.
    pub(crate) fn restore_geometry(&mut self, [x, y]: [f64; 2], scanned: [usize; 2]) {
        let barcodes = self.barcodes.iter_mut().map(|barcode| &mut barcode.polygon);
        let regions = self.undecoded.iter_mut().map(|region| &mut region.polygon);
        for point in barcodes.chain(regions).flatten() {
            point[0] *= x;
            point[1] *= y;
        }
        if let Some(Diagnostics {
            raw: serde_json::Value::Object(raw),
        }) = &mut self.diagnostics
        {
            for value in raw.values_mut() {
                restore_diagnostic_geometry(value, [x, y]);
            }
            raw.insert("scannedImageSize".into(), serde_json::json!(scanned));
        }
    }
    pub(crate) fn from_engine(
        engine: EngineScan,
        [width, height]: [usize; 2],
        mode: Mode,
        elapsed: Duration,
    ) -> Self {
        let barcodes = engine.barcodes;
        let undecoded = engine.undecoded;
        let diagnostics = engine.diagnostics.map(|raw| Diagnostics { raw });
        Self {
            barcodes,
            undecoded,
            image_size: [width, height],
            mode,
            elapsed,
            diagnostics,
        }
    }
}

/// Scale the image geometry in raw diagnostics from the scanned to the supplied image:
/// every `polygon` and `coverage` point list, and the `x`, `y`, `w`, `h` crop of a
/// recovery attempt. A recovery attempt's nested `frame` describes the enlarged crop
/// in its own coordinates and stays unchanged.
fn restore_diagnostic_geometry(value: &mut serde_json::Value, [x, y]: [f64; 2]) {
    use serde_json::Value;
    let scale = |value: &mut Value, factor: f64| {
        if let Some(number) = value.as_f64() {
            *value = serde_json::json!(number * factor);
        }
    };
    match value {
        Value::Array(items) => {
            for item in items {
                restore_diagnostic_geometry(item, [x, y]);
            }
        }
        Value::Object(object) => {
            let crop = ["x", "y", "w", "h"]
                .iter()
                .all(|key| object.get(*key).is_some_and(Value::is_number));
            for (key, item) in object.iter_mut() {
                match key.as_str() {
                    "polygon" | "coverage" => {
                        for point in item.as_array_mut().into_iter().flatten() {
                            if let Some([px, py]) = point.as_array_mut().map(Vec::as_mut_slice) {
                                scale(px, x);
                                scale(py, y);
                            }
                        }
                    }
                    "x" | "w" if crop => scale(item, x),
                    "y" | "h" if crop => scale(item, y),
                    "frame" => {}
                    _ => restore_diagnostic_geometry(item, [x, y]),
                }
            }
        }
        _ => {}
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
