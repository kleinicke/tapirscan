//! Safe full-image facade for one pinned, compile-time-selected scanner mode.
#![forbid(unsafe_code)]
#[cfg(not(feature = "low"))]
mod detail;
mod effort;
#[cfg(any(
    feature = "low",
    feature = "medium",
    feature = "high",
    feature = "very-high"
))]
mod fast_linear;
#[cfg(any(feature = "low", feature = "medium"))]
mod fast_sparse;
mod format_registry;
mod geometry;
mod linear_duplicates;
mod pipeline;
mod read;
mod result;
#[cfg(feature = "medium")]
mod short_crop;
mod timer;
pub use barcode_research_core::region_scan::{Error, ImageView, RegionScanner, ScanResult};
pub use barcode_research_core::{frame::Barcode, oriented::Proposal, scan::Quad};

#[derive(Clone, Copy)]
pub struct Image<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub stride: usize,
}
impl Image<'_> {
    /// Binary-fraction luminance used by detail and continuity evidence.
    /// All weighted terms and their sum are exact integers below 65536.
    #[inline]
    pub(crate) fn fixed_luminance(self, offset: usize) -> f64 {
        if self.channels == 1 {
            f64::from(self.data[offset])
        } else {
            f64::from(
                77 * u32::from(self.data[offset])
                    + 150 * u32::from(self.data[offset + 1])
                    + 29 * u32::from(self.data[offset + 2]),
            ) / 256.
        }
    }
}
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent effort and output controls are not mutually exclusive states."
)]
pub struct ScanOptions {
    /// Finish selected EAN/UPC candidate work beyond shared frame budgets.
    pub finish_candidates: bool,
    /// False returns at most the highest-support decoded read after the full scan.
    pub multiple: bool,
    /// Include localization proposals, search windows and per-candidate evidence.
    pub include_regions: bool,
    pub retain_diagnostics: bool,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            finish_candidates: false,
            multiple: true,
            include_regions: false,
            retain_diagnostics: false,
        }
    }
}
pub struct Result {
    proposals: Vec<Proposal>,
    #[cfg_attr(
        feature = "low",
        expect(dead_code, reason = "Low has no short-code crop recovery to read them")
    )]
    short_fragments: Option<Vec<Proposal>>,
    localization_omitted: usize,
    localization_work_limited: bool,
    /// Full-frame search window, when it was scanned.
    search_window: Option<Quad>,
    scan: ScanResult,
    options: ScanOptions,
    recovery: Option<read::Recovery>,
    retail: Vec<read::Read>,
}
/// Detailed region evidence, inspected by the engine's own tests.
#[cfg(test)]
pub struct Regions<'a> {
    pub proposals: &'a [Proposal],
    pub frame: &'a barcode_research_core::frame::Frame,
}
#[derive(Default)]
pub struct Scanner {
    regions: RegionScanner,
    localizer: barcode_research_core::stripes::Detector,
    additional_gray: Vec<u8>,
    #[cfg(any(
        feature = "low",
        feature = "medium",
        feature = "high",
        feature = "very-high"
    ))]
    fast_profiles: barcode_research_core::fast_profile::Sampler,
    retail_rgba: Vec<u8>,
    /// Other linear formats share this scan: Medium retail crops decode only their box.
    #[cfg(feature = "medium")]
    shared_linear: bool,
    #[cfg(not(feature = "low"))]
    recovery: recovery_core::region_scan::RegionScanner,
}
impl Scanner {
    /// Scanning effort is independent of output options: every candidate is attempted.
    ///
    /// # Errors
    /// Returns an error for invalid image geometry, layout, or scanner work parameters.
    pub fn scan_with_options(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
    ) -> std::result::Result<Result, Error> {
        self.scan_with_coverage(image, options, &[], true, false)
    }

    // Keep primary decisions and crop recovery in the same ordered transaction.
    pub(crate) fn scan_with_coverage(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        coverage: &[Quad],
        consolidate: bool,
        shared_retail: bool,
    ) -> std::result::Result<Result, Error> {
        pipeline::scan(self, image, options, coverage, consolidate, shared_retail)
    }
}
impl Result {
    #[must_use]
    pub fn barcodes(&self) -> &[Barcode] {
        &self.scan.frame.barcodes
    }
    #[must_use]
    pub fn unfinished(&self) -> bool {
        self.scan.frame.unfinished
    }
    #[cfg(test)]
    #[must_use]
    pub fn regions(&self) -> Option<Regions<'_>> {
        self.options.include_regions.then_some(Regions {
            proposals: &self.proposals,
            frame: &self.scan.frame,
        })
    }
    /// Serialize JSON schema 2, omitting region evidence unless requested.
    ///
    /// # Panics
    /// Panics if `mode` is not a supported effort mode, or elapsed time is negative/non-finite.
    #[cfg(test)]
    #[must_use]
    pub fn to_json(&self, mode: &str, elapsed_ms: f64) -> String {
        serde_json::to_string(
            &result::value(self, mode, elapsed_ms)
                .expect("mode and elapsed time must satisfy the documented contract"),
        )
        .expect("schema values are JSON serializable")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_luminance_preserves_all_rgb_triples() {
        for red in 0..=255_u8 {
            for green in 0..=255_u8 {
                for blue in 0..=255_u8 {
                    let data = [red, green, blue];
                    let image = Image {
                        data: &data,
                        width: 1,
                        height: 1,
                        channels: 3,
                        stride: 3,
                    };
                    let expected =
                        (77. * f64::from(red) + 150. * f64::from(green) + 29. * f64::from(blue))
                            / 256.;
                    assert_eq!(image.fixed_luminance(0).to_bits(), expected.to_bits());
                }
            }
        }
    }

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
        let compact = scanner
            .scan_with_options(image(), ScanOptions::default())
            .unwrap();
        assert!(compact.barcodes().is_empty());
        assert!(compact.regions().is_none());
        assert!(!compact.to_json("medium", 0.).contains("\"candidates\""));
        let detailed = scanner
            .scan_with_options(
                image(),
                ScanOptions {
                    finish_candidates: false,
                    multiple: false,
                    include_regions: true,
                    retain_diagnostics: true,
                },
            )
            .unwrap();
        let regions = detailed.regions().unwrap();
        // The full-frame window is a candidate exactly when it was searched, and only then
        // reported. Medium searches it only on barcode evidence, which a blank frame lacks.
        let window = usize::from(detailed.search_window.is_some());
        assert_eq!(
            regions.frame.candidates.len(),
            regions.proposals.len() + window
        );
        assert_eq!(window, usize::from(!cfg!(feature = "medium")));
        assert!(detailed.barcodes().is_empty());
        assert!(detailed.to_json("medium", 0.).contains("\"searchWindows\""));
        assert!(
            scanner
                .scan_with_options(
                    Image {
                        data: &[],
                        width: 64,
                        height: 64,
                        channels: 1,
                        stride: 64
                    },
                    ScanOptions::default()
                )
                .is_err()
        );
    }
    #[test]
    fn single_selects_support_and_keeps_first_tie() {
        let make = |support, id| Barcode {
            detection: barcode_research_core::candidate_scanner::Detection {
                digits: [id; 13],
                polygon: [[0., 0.]; 4],
                support,
                axis: 0,
            },
            candidate_indices: vec![],
        };
        let mut reads = vec![make(1, 1), make(4, 2), make(4, 3)];
        pipeline::select_one(&mut reads);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].detection.digits, [2; 13]);
    }
}

// Low now selects the original Turbo policy. Classic is a demo-only build recipe;
// higher effort modes must never inherit the bounded Low policy.
pub(crate) const LOW_FAST_PATH: bool =
    cfg!(feature = "low") && option_env!("TAPIRSCAN_LOW_CLASSIC").is_none();

/// Compiled effort identity, matching the pinned JavaScript host policy.
pub const MODE_ID: u32 = if cfg!(feature = "low") {
    0
} else if cfg!(feature = "high") {
    2
} else if cfg!(feature = "very-high") {
    3
} else {
    1
};
pub const MODE: &str = ["low", "medium", "high", "very-high"][MODE_ID as usize];

pub mod formats;
#[cfg(not(feature = "low"))]
mod matrix_grid;
#[cfg(not(feature = "low"))]
mod signal_recovery;

#[cfg(feature = "very-high")]
mod recovery_conflicts;

#[cfg(any(feature = "medium", feature = "high"))]
use fast_linear as recovery_admission;
#[cfg(feature = "very-high")]
use recovery_conflicts as recovery_admission;

mod matrix_frontend;
