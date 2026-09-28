//! Safe independent supplied-region scanner. Localization is a caller provider.
//! Confidence is uncalibrated. Work exhaustion is retained in the frame result.
#![forbid(unsafe_code)]
pub use crate::{
    frame::{Barcode, Frame},
    multi_scan::Policy,
    sampling::{Error, ImageView},
    scan::Quad,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFailure {
    InvalidGeometry,
    Sampling,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateError {
    pub candidate_index: usize,
    pub kind: CandidateFailure,
}
#[derive(Debug)]
pub struct ScanResult {
    pub frame: Frame,
    pub errors: Vec<CandidateError>,
    /// False means per-candidate ms fields are unavailable zero placeholders.
    /// The host must measure actual preparation/copying and the complete call.
    pub candidate_timings_available: bool,
}
impl ScanResult {
    /// Single-result convenience after find-all. Support is a heuristic ordering,
    /// not probability. Equal-support ties preserve the earlier frame ordering.
    #[must_use]
    pub fn best(&self) -> Option<&Barcode> {
        self.frame
            .barcodes
            .iter()
            .enumerate()
            .max_by_key(|(i, b)| (b.detection.support, std::cmp::Reverse(*i)))
            .map(|(_, b)| b)
    }
}
#[derive(Default)]
pub struct RegionScanner {
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub(crate) engine: crate::experiment::CandidateScanner,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    engine: crate::experiment::CandidateScanner,
}
impl RegionScanner {
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    /// Run the optional threshold path in an isolated, caller-bounded transaction.
    /// # Errors
    /// Propagates the ordinary region scan errors and always restores the primary policy.
    pub fn scan_threshold_recovery(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
    ) -> Result<ScanResult, Error> {
        self.engine.threshold_recovery = true;
        let result = self.scan(image, candidates, policy);
        self.engine.threshold_recovery = false;
        result
    }

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    /// Configure shared retail evidence; bit 1 keeps the primary reader enabled.
    /// # Errors
    /// Rejects unsupported family bits.
    pub fn retail_configure(&mut self, mask: u32) -> Result<(), Error> {
        if mask & 1 == 0 || mask & !15 != 0 {
            return Err(Error::Parameters);
        }
        self.engine.retail.mask = mask;
        self.engine.retail.use_raw = mask & 12 != 0;
        self.engine.retail.use_peaks = mask & 12 != 0;
        self.engine.retail.peak_retry = mask & 12 != 0;
        self.engine.retail.reset();
        Ok(())
    }
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub fn retail_finish_typed(
        &mut self,
        image: ImageView<'_>,
        frame: &Frame,
        diagnostics: bool,
    ) -> Option<crate::retail_pipeline::RetailResult> {
        if self.engine.retail.mask & 12 == 0 {
            return None;
        }
        self.engine.retail.set_primary(frame);
        Some(self.engine.retail.finish_typed(image, diagnostics))
    }
    /// Always attempts every supplied candidate's cheap pass before retries.
    /// Input is borrowed for this call; output owns its polygons/observations.
    /// More than64candidates or invalid policies fail explicitly, never truncate
    /// the candidate set. Undecoded and invalid candidates retain their identity.
    /// # Errors
    /// Propagates frame-scanner setup errors, including invalid policy or excessive candidate count. Per-candidate failures are retained in the result.
    pub fn scan(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
    ) -> Result<ScanResult, Error> {
        let frame = self.engine.scan_frame(image, candidates, policy)?;
        let errors = frame
            .candidates
            .iter()
            .filter(|c| c.error)
            .map(|c| CandidateError {
                candidate_index: c.index,
                kind: if crate::scan::transform(c.coverage).is_err() {
                    CandidateFailure::InvalidGeometry
                } else {
                    CandidateFailure::Sampling
                },
            })
            .collect();
        Ok(ScanResult {
            frame,
            errors,
            candidate_timings_available: cfg!(all(
                feature = "native-timing",
                not(target_arch = "wasm32")
            )),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_candidates_retained_and_limits_rejected() {
        let pixels = vec![255; 100 * 100];
        let image = ImageView::new(&pixels, 100, 100, 1, 100).unwrap();
        let q = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];
        let mut bad = q;
        bad[0][0] = f64::NAN;
        let mut scanner = RegionScanner::default();
        let result = scanner.scan(image, &[q, bad], Policy::default()).unwrap();
        assert_eq!(result.frame.candidates.len(), 2);
        assert_eq!(
            result.errors,
            vec![CandidateError {
                candidate_index: 1,
                kind: CandidateFailure::InvalidGeometry
            }]
        );
        assert!(result.frame.unfinished);
        assert!(result.best().is_none());
        assert!(matches!(
            scanner.scan(image, &[q; 65], Policy::default()),
            Err(Error::Parameters)
        ));
    }
}

#[cfg(all(test, any(feature = "mode-high", feature = "mode-very-high")))]
mod threshold_recovery_tests {
    use super::*;

    #[test]
    fn optional_threshold_state_is_reset_after_success_and_invalid_request() {
        let data = vec![255; 64 * 32];
        let image = ImageView::new(&data, 64, 32, 1, 64).unwrap();
        let q = [[0., 0.], [63., 0.], [63., 31.], [0., 31.]];
        let mut scanner = RegionScanner::default();
        assert!(!scanner.engine.threshold_recovery);
        assert!(scanner
            .scan_threshold_recovery(image, &[q; 65], Policy::default())
            .is_err());
        assert!(!scanner.engine.threshold_recovery);
        let result = scanner
            .scan_threshold_recovery(image, &[q], Policy::default())
            .unwrap();
        assert!(result.frame.barcodes.is_empty());
        assert!(!scanner.engine.threshold_recovery);
        let ordinary = scanner.scan(image, &[q], Policy::default()).unwrap();
        let baseline = RegionScanner::default()
            .scan(image, &[q], Policy::default())
            .unwrap();
        assert_eq!(ordinary.frame.barcodes.len(), baseline.frame.barcodes.len());
        assert_eq!(ordinary.frame.unfinished, baseline.frame.unfinished);
        assert_eq!(ordinary.errors, baseline.errors);
        assert_eq!(
            format!("{:?}", ordinary.frame.candidates[0].work),
            format!("{:?}", baseline.frame.candidates[0].work)
        );
    }
}
