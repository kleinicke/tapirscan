//! Safe full-image facade for one pinned, compile-time-selected scanner mode.
#![forbid(unsafe_code)]
#[cfg(not(feature = "low"))]
mod detail;
pub use barcode_research_core::region_scan::{Error, ImageView, RegionScanner, ScanResult};
pub use barcode_research_core::{
    frame::{Barcode, Frame},
    oriented::Proposal,
    scan::Quad,
};
use barcode_research_core::{multi_scan::Policy, shear, stripes};

#[derive(Clone, Copy)]
pub struct Image<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub stride: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    /// False returns at most the highest-support decoded read after the full scan.
    pub multiple: bool,
    /// Include localization proposals, search windows and per-candidate evidence.
    pub include_regions: bool,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            multiple: true,
            include_regions: false,
        }
    }
}
pub struct Result {
    proposals: Vec<Proposal>,
    localization_omitted: usize,
    localization_work_limited: bool,
    search_window: Quad,
    scan: ScanResult,
    options: ScanOptions,
    recovery: Option<serde_json::Value>,
}
/// Detailed region evidence is exposed only when requested before scanning.
pub struct Regions<'a> {
    pub proposals: &'a [Proposal],
    pub omitted: usize,
    pub work_limited: bool,
    pub search_window: Quad,
    pub frame: &'a Frame,
}
#[derive(Default)]
pub struct Scanner {
    regions: RegionScanner,
    #[cfg(not(feature = "low"))]
    recovery: recovery_core::region_scan::RegionScanner,
}
impl Scanner {
    /// Scan all visible EAN-13 candidates.
    ///
    /// # Errors
    /// Returns an error for invalid image geometry, layout, or scanner work parameters.
    pub fn scan(&mut self, image: Image<'_>) -> std::result::Result<Result, Error> {
        self.scan_with_options(image, ScanOptions::default())
    }
    /// Scanning effort is independent of output options: every candidate is attempted.
    ///
    /// # Errors
    /// Returns an error for invalid image geometry, layout, or scanner work parameters.
    pub fn scan_with_options(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
    ) -> std::result::Result<Result, Error> {
        if image.width < 3 || image.height < 3 {
            return Err(Error::Parameters);
        }
        let im = ImageView::new(
            image.data,
            image.width,
            image.height,
            image.channels,
            image.stride,
        )?;
        let found = stripes::detect(im)?;
        let count = found.proposals.len();
        let examined = count.min(FIT_LIMIT);
        let mut proposals = found.proposals;
        for i in 0..examined {
            if let Some(p) = shear::refine(im, proposals[i].polygon) {
                proposals.push(p);
            }
        }

        // At most eight alternate base proposals plus four shear refinements. Keep
        // the entire original prefix; no code value or GT selects this extra grid.
        let mut secondary_omitted = 0usize;
        let mut secondary_limited = false;
        if cfg!(feature = "very-high") && image.width.max(image.height) > 640 {
            if let Ok(second) = stripes::detect_secondary(im) {
                let secondary_count = second.proposals.len();
                secondary_limited = second.limited;
                let selected = secondary_count.min(8);
                secondary_omitted = second.omitted + secondary_count - selected;
                let extra: Vec<_> = second.proposals.into_iter().take(selected).collect();
                for p in &extra {
                    proposals.push(Proposal {
                        polygon: p.polygon,
                        score: p.score,
                    });
                }
                for p in extra.iter().take(4) {
                    if let Some(p) = shear::refine(im, p.polygon) {
                        proposals.push(p);
                    }
                }
                secondary_omitted += selected.saturating_sub(4);
            } else {
                secondary_limited = true;
            }
        }
        if proposals.len() > if cfg!(feature = "very-high") { 63 } else { 32 } {
            return Err(Error::Parameters);
        }
        let right = f64::from(u32::try_from(image.width - 1).map_err(|_| Error::Parameters)?);
        let bottom = f64::from(u32::try_from(image.height - 1).map_err(|_| Error::Parameters)?);
        let search_window = [[0., 0.], [right, 0.], [right, bottom], [0., bottom]];
        let mut candidates: Vec<_> = proposals.iter().map(|p| p.polygon).collect();
        candidates.push(search_window);
        let policy = Policy {
            #[cfg(not(feature = "low"))]
            candidate_retry_mask: detail::retry_mask(image, &proposals),
            transition_cleanup: true,
            source_identity: true,
            interior_normalization: true,
            guard_bias: true,
            ..Policy::default()
        };
        let mut scan = self.regions.scan(im, &candidates, policy)?;
        #[cfg(not(feature = "low"))]
        let recovery = {
            let result = detail::recover(
                image,
                &mut scan.frame.barcodes,
                &mut self.recovery,
                if cfg!(feature = "medium") { 1 } else { 2 },
            )?;
            scan.frame.unfinished = true;
            Some(result)
        };
        #[cfg(feature = "low")]
        let recovery = None;
        if !options.multiple {
            select_one(&mut scan.frame.barcodes);
        }
        Ok(Result {
            proposals,
            localization_omitted: found.omitted + count - examined + secondary_omitted,
            localization_work_limited: found.limited
                || count > examined
                || secondary_limited
                || secondary_omitted > 0,
            search_window,
            scan,
            options,
            recovery,
        })
    }
}
fn select_one(reads: &mut Vec<Barcode>) {
    // A strict improvement preserves the first result on equal support.
    let mut best = 0;
    for i in 1..reads.len() {
        if reads[i].detection.support > reads[best].detection.support {
            best = i;
        }
    }
    if !reads.is_empty() {
        let selected = reads.remove(best);
        reads.clear();
        reads.push(selected);
    }
}
impl Result {
    #[must_use]
    pub fn barcodes(&self) -> &[Barcode] {
        &self.scan.frame.barcodes
    }
    #[must_use]
    pub fn best(&self) -> Option<&Barcode> {
        self.scan.best()
    }
    #[must_use]
    pub fn unfinished(&self) -> bool {
        self.scan.frame.unfinished
    }
    #[must_use]
    pub fn localization_limited(&self) -> bool {
        self.localization_work_limited
    }
    #[must_use]
    pub fn regions(&self) -> Option<Regions<'_>> {
        self.options.include_regions.then_some(Regions {
            proposals: &self.proposals,
            omitted: self.localization_omitted,
            work_limited: self.localization_work_limited,
            search_window: self.search_window,
            frame: &self.scan.frame,
        })
    }
    /// Serialize JSON schema 2, omitting region evidence unless requested.
    ///
    /// # Panics
    /// Panics if `mode` is neither `fast` nor `quality`, or elapsed time is negative/non-finite.
    #[must_use]
    pub fn to_json(&self, mode: &str, elapsed_ms: f64) -> String {
        assert!(["low", "medium", "high", "very-high"].contains(&mode));
        assert!(elapsed_ms.is_finite() && elapsed_ms >= 0.0);
        let (details, frame) = if self.options.include_regions {
            let proposals = self
                .proposals
                .iter()
                .map(|p| {
                    format!(
                        "{{\"polygon\":{:?},\"score\":{},\"text\":\"\"}}",
                        p.polygon, p.score
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            (format!(",\"localization\":{{\"proposals\":[{}],\"omitted\":{},\"workLimited\":{}}},\"searchWindows\":[{{\"kind\":\"full_frame_search\",\"polygon\":{:?},\"candidateIndex\":{}}}]",
                proposals,self.localization_omitted,self.localization_work_limited,self.search_window,self.proposals.len()),
             barcode_research_core::region_json::frame_json(&self.scan.frame))
        } else {
            let reads=self.barcodes().iter().map(|b| {
                let d=&b.detection;
                let text:String=d.digits.iter().map(|v|(b'0'+v)as char).collect();
                format!("{{\"text\":\"{}\",\"polygon\":{:?},\"support\":{},\"axis\":{},\"candidate_indices\":{:?}}}",
                    text,d.polygon,d.support,d.axis,b.candidate_indices)
            }).collect::<Vec<_>>().join(",");
            (
                String::new(),
                format!(
                    "{{\"unfinished\":{},\"barcodes\":[{}]}}",
                    self.unfinished(),
                    reads
                ),
            )
        };
        let recovery = if self.options.include_regions {
            self.recovery
                .as_ref()
                .map(|v| format!(",\"recovery\":{v}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        format!("{{\"schemaVersion\":2,\"mode\":\"{}\",\"multiple\":{},\"elapsedMs\":{},\"localizationLimited\":{}{},\"scan\":{}}}",
            mode,self.options.multiple,elapsed_ms,self.localization_work_limited,format_args!("{details}{recovery}"),frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_optional_regions() {
        let mut scanner = Scanner::default();
        let data = vec![255; 64 * 64];
        let image = || Image {
            data: &data,
            width: 64,
            height: 64,
            channels: 1,
            stride: 64,
        };
        let compact = scanner.scan(image()).unwrap();
        assert!(compact.barcodes().is_empty());
        assert!(compact.regions().is_none());
        assert!(!compact.to_json("medium", 0.).contains("\"candidates\""));
        let detailed = scanner
            .scan_with_options(
                image(),
                ScanOptions {
                    multiple: false,
                    include_regions: true,
                },
            )
            .unwrap();
        let regions = detailed.regions().unwrap();
        assert_eq!(regions.frame.candidates.len(), regions.proposals.len() + 1);
        assert!(detailed.barcodes().is_empty());
        assert!(detailed.to_json("medium", 0.).contains("\"searchWindows\""));
        assert!(scanner
            .scan(Image {
                data: &[],
                width: 64,
                height: 64,
                channels: 1,
                stride: 64
            })
            .is_err());
    }
    #[test]
    fn single_selects_support_and_keeps_first_tie() {
        let make = |support, id| Barcode {
            detection: barcode_research_core::experiment::Detection {
                digits: [id; 13],
                polygon: [[0., 0.]; 4],
                support,
                axis: 0,
            },
            candidate_indices: vec![],
        };
        let mut reads = vec![make(1, 1), make(4, 2), make(4, 3)];
        select_one(&mut reads);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].detection.digits, [2; 13]);
    }
}

/// Compiled effort identity, matching the pinned JavaScript host policy.
pub const MODE: &str = if cfg!(feature = "low") {
    "low"
} else if cfg!(feature = "high") {
    "high"
} else if cfg!(feature = "very-high") {
    "very-high"
} else {
    "medium"
};
pub const MODE_ID: u32 = if cfg!(feature = "low") {
    0
} else if cfg!(feature = "high") {
    2
} else if cfg!(feature = "very-high") {
    3
} else {
    1
};
const FIT_LIMIT: usize = if cfg!(feature = "low") {
    0
} else if cfg!(feature = "high") {
    4
} else {
    1
};

pub mod formats;

mod decoded;
pub use decoded::{DecodedBarcode, DecodedResult, Format, Formats};
