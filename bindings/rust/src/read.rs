//! Typed reader evidence shared by primary, retail, and additional readers.
use crate::{Error, Quad};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Read {
    pub text: String,
    pub format: String,
    pub polygon: Quad,
    pub support: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_indices: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "bytes")]
    pub payload_bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "eanAddOn")]
    pub addon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gs1: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "readerInitialization"
    )]
    pub reader_initialization: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "structuredAppend")]
    pub structured_append: Option<barcode_multiformat::StructuredAppend>,
    /// Reader-specific diagnostics do not participate in acceptance or ranking.
    #[serde(flatten)]
    pub payload: ReaderPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct ReaderPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<f64>,
}

impl Read {
    pub fn primary(
        digits: [u8; 13],
        polygon: Quad,
        support: usize,
        axis: usize,
        candidates: Vec<usize>,
    ) -> Self {
        Self {
            text: digits.iter().map(|d| char::from(b'0' + d)).collect(),
            format: "EAN13".into(),
            polygon,
            support: support as u64,
            axis: Some(axis),
            candidate_indices: Some(candidates),
            payload_bytes: None,
            addon: None,
            gs1: None,
            reader_initialization: None,
            structured_append: None,
            payload: ReaderPayload::default(),
            rank: None,
        }
    }

    pub fn retail(digits: [u8; 13], polygon: Quad, support: usize) -> Self {
        let mut read = Self::primary(digits, polygon, support, 0, Vec::new());
        read.text = digits[5..].iter().map(|d| char::from(b'0' + d)).collect();
        read.format = if digits[0] == 14 { "EAN8" } else { "UPCE" }.into();
        read.axis = None;
        read.candidate_indices = None;
        read
    }

    pub fn additional(value: barcode_multiformat::Detection) -> Self {
        Self {
            text: value.text,
            format: value.format,
            polygon: value.polygon.map(|point| point.map(f64::from)),
            support: value.support as u64,
            axis: Some(0),
            candidate_indices: Some(Vec::new()),
            payload_bytes: value.bytes,
            addon: value.addon,
            gs1: Some(value.gs1),
            reader_initialization: value.reader_initialization.then_some(true),
            structured_append: value.structured_append,
            payload: ReaderPayload {
                error: Some(f64::from(value.error)),
            },
            rank: None,
        }
    }

    pub fn into_public(self) -> Result<scanner_types::Barcode, Error> {
        validate_polygon(self.polygon)?;
        Ok(scanner_types::Barcode {
            text: self.text,
            format: Deserialize::deserialize(serde::de::value::StrDeserializer::<
                serde::de::value::Error,
            >::new(&self.format))
            .map_err(|_| Error::OutputShape)?,
            polygon: self.polygon,
            support: self.support,
            payload_bytes: self.payload_bytes,
            ean_add_on: self.addon,
            gs1: self.gs1,
            reader_initialization: self.reader_initialization,
            structured_append: self.structured_append.map(|value| {
                scanner_types::StructuredAppend {
                    index: value.index,
                    count: value.count,
                    id: value.id,
                    parity: value.parity,
                }
            }),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Region {
    pub format: String,
    pub text: String,
    pub polygon: Quad,
    pub support: u64,
    #[serde(rename = "localizationScore", skip_serializing_if = "Option::is_none")]
    pub localization_score: Option<f64>,
}
impl Region {
    pub fn unknown(polygon: Quad) -> Self {
        Self {
            format: "Unknown".into(),
            text: String::new(),
            polygon,
            support: 0,
            localization_score: None,
        }
    }
    pub fn into_public(self) -> Result<scanner_types::UndecodedRegion, Error> {
        validate_polygon(self.polygon)?;
        Ok(scanner_types::UndecodedRegion {
            format: if self.format == "Unknown" {
                None
            } else {
                Some(
                    Deserialize::deserialize(serde::de::value::StrDeserializer::<
                        serde::de::value::Error,
                    >::new(&self.format))
                    .map_err(|_| Error::OutputShape)?,
                )
            },
            polygon: self.polygon,
        })
    }
}

pub(crate) struct Recovery {
    pub unread: Vec<Region>,
    pub retail: Vec<Read>,
    pub diagnostics: Option<serde_json::Value>,
}

// JSON previously represented non-finite coordinates as null, which the public
// result parser rejected. Keep that malformed-reader contract without JSON.
fn validate_polygon(polygon: Quad) -> Result<(), Error> {
    if polygon
        .iter()
        .flatten()
        .all(|coordinate| coordinate.is_finite())
    {
        Ok(())
    } else {
        Err(Error::OutputShape)
    }
}

#[cfg(test)]
mod tests {
    use super::{Read, Region};

    #[test]
    fn malformed_geometry_retains_the_legacy_error_contract() {
        for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let polygon = [[coordinate, 0.0]; 4];
            let read = Read::primary([0; 13], polygon, 1, 0, Vec::new());
            let legacy = serde_json::to_value(&read).unwrap();
            assert!(serde_json::from_value::<scanner_types::Barcode>(legacy).is_err());
            assert!(matches!(read.into_public(), Err(crate::Error::OutputShape)));
            assert!(matches!(
                Region::unknown(polygon).into_public(),
                Err(crate::Error::OutputShape)
            ));
        }
    }
}
