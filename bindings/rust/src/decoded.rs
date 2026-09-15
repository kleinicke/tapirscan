//! Typed multi-format results; raw evidence is available explicitly through JSON.
use crate::{Error, Image, ScanOptions, Scanner};
use serde::Deserialize;
use serde_json::Value;

/// A supported barcode symbology. Discriminants are native format-mask bits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[repr(u32)]
pub enum Format {
    #[serde(rename = "EAN13")]
    Ean13 = 1,
    #[serde(rename = "UPCA")]
    Upca = 2,
    #[serde(rename = "EAN8")]
    Ean8 = 4,
    #[serde(rename = "UPCE")]
    Upce = 8,
    Code128 = 16,
    Code39 = 32,
    #[serde(rename = "ITF")]
    Itf = 64,
    Codabar = 128,
    Code93 = 256,
    #[serde(rename = "QRCode")]
    QrCode = 512,
    DataMatrix = 1024,
    #[serde(rename = "PDF417")]
    Pdf417 = 2048,
    Aztec = 4096,
    DataBar = 8192,
    DataBarExpanded = 16384,
    MaxiCode = 131_072,
}

/// Format selection with presets and composable individual formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Formats(u32);
impl Formats {
    pub const RETAIL: Self = Self(15);
    pub const LINEAR: Self = Self(511 | 8192 | 16384);
    pub const MATRIX: Self = Self(512 | 1024 | 2048 | 4096 | 131_072);
    pub const ALL: Self = Self(Self::LINEAR.0 | Self::MATRIX.0);
}
impl TryFrom<u32> for Formats {
    type Error = Error;
    fn try_from(bits: u32) -> Result<Self, Error> {
        if bits == 0 || bits & !Self::ALL.0 != 0 {
            return Err(Error::Parameters);
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

/// A decoded string with its format and source-image polygon.
#[derive(Clone, Debug, Deserialize)]
pub struct DecodedBarcode {
    pub text: String,
    pub format: Format,
    pub polygon: [[f64; 2]; 4],
    support: u64,
}

/// Owned reads and optional diagnostic JSON, valid after the scanner is dropped.
#[derive(Debug)]
pub struct DecodedResult {
    barcodes: Vec<DecodedBarcode>,
    image_size: [usize; 2],
    unfinished: bool,
    localization_limited: bool,
    debug: bool,
    raw: Value,
}
impl DecodedResult {
    #[must_use]
    pub fn barcodes(&self) -> &[DecodedBarcode] {
        &self.barcodes
    }
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.barcodes.iter().map(|barcode| barcode.text.as_str())
    }
    #[must_use]
    pub fn best(&self) -> Option<&DecodedBarcode> {
        self.barcodes
            .iter()
            .enumerate()
            .max_by_key(|(i, b)| (b.support, std::cmp::Reverse(*i)))
            .map(|(_, b)| b)
    }
    /// Input dimensions as [width, height].
    #[must_use]
    pub fn image_size(&self) -> [usize; 2] {
        self.image_size
    }
    #[must_use]
    pub fn unfinished(&self) -> bool {
        self.unfinished
    }
    #[must_use]
    pub fn localization_limited(&self) -> bool {
        self.localization_limited
    }
    /// Diagnostic evidence is present only when `include_regions` was requested.
    #[must_use]
    pub fn debug(&self) -> Option<&Value> {
        self.debug.then_some(&self.raw)
    }
    /// Raw schema-2 output for ABI consumers or reader-specific metadata.
    #[must_use]
    pub fn json(&self) -> &Value {
        &self.raw
    }
}
impl Scanner {
    /// Scan selected formats, returning typed values and geometry.
    ///
    /// # Errors
    /// Rejects invalid images or malformed output from an internal reader.
    pub fn scan_formats(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        formats: impl Into<Formats>,
    ) -> Result<DecodedResult, Error> {
        let size = [image.width, image.height];
        let raw = self.scan_formats_json(image, options, formats.into().0)?;
        let barcodes = serde_json::from_value(raw["scan"]["barcodes"].clone())
            .map_err(|_| Error::Parameters)?;
        let unfinished = raw["scan"]["unfinished"]
            .as_bool()
            .ok_or(Error::Parameters)?;
        let localization_limited = raw["localizationLimited"]
            .as_bool()
            .ok_or(Error::Parameters)?;
        Ok(DecodedResult {
            barcodes,
            image_size: size,
            unfinished,
            localization_limited,
            debug: options.include_regions,
            raw,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_match_native_formats() {
        for (name, bit) in crate::formats::FORMATS {
            let format: Format = serde_json::from_value(Value::String(name.into())).unwrap();
            assert_eq!(Formats::from(format).0, bit);
        }
        assert!(Formats::try_from(0).is_err());
        assert!(Formats::try_from(65536).is_err());
        assert_eq!((Format::Ean13 | Format::Code128).0, 17);
        assert_eq!((Formats::LINEAR | Formats::MATRIX), Formats::ALL);
    }
}
