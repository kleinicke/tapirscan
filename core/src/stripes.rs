//! Bounded component-preserving Scharr localizer. TS037 reference port.
#![forbid(unsafe_code)]
use crate::{
    oriented::Proposal,
    sampling::{Error, ImageView},
};
#[derive(Clone, Default)]
struct Tile {
    xx: f64,
    xy: f64,
    yy: f64,
    angle: f64,
    active: bool,
    growable: bool,
}
const CACHE_TILES: bool = option_env!("TAPIRSCAN_EXPERIMENT_TILE_CACHE").is_some();
impl Tile {
    fn classify(&mut self, threshold: f64) {
        self.classify_policy(threshold, CACHE_TILES);
    }
    fn classify_policy(&mut self, threshold: f64, cached: bool) {
        let energy = self.xx + self.yy;
        if cached {
            let coherence = if energy > 64. * 80. {
                tensor_magnitude(self.xx, self.xy, self.yy) / (energy + 1.)
            } else {
                0.
            };
            self.growable = energy > 64. * 80. && coherence >= 0.4;
            self.active = energy > 64. * 80. && coherence > threshold;
            // All angle consumers reject inactive, non-growable tiles first.
            self.angle = if self.active || self.growable {
                0.5 * (2. * self.xy).atan2(self.xx - self.yy)
            } else {
                0.
            };
        } else {
            self.angle = 0.5 * (2. * self.xy).atan2(self.xx - self.yy);
            self.active = energy > 64. * 80.
                && tensor_magnitude(self.xx, self.xy, self.yy) / (energy + 1.) > threshold;
        }
    }
    fn can_grow(&self) -> bool {
        if CACHE_TILES {
            self.growable
        } else {
            let energy = self.xx + self.yy;
            energy > 64. * 80. && tensor_magnitude(self.xx, self.xy, self.yy) / (energy + 1.) >= 0.4
        }
    }
}
#[derive(Clone, Copy)]
struct Bounds {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
}
#[derive(Clone, Copy)]
struct Edge {
    x: f64,
    y: f64,
    weight: f64,
}
pub struct Result {
    pub proposals: Vec<Proposal>,
    pub omitted: usize,
    pub limited: bool,
    pub trace: [usize; 13],
}

// For finite Scharr gradients, max(|dx|,|dy|) <= hypot <= |dx|+|dy|.
// These one-sided bounds only reject edges the original tests must reject;
// Nano computes the bounded gradient norm directly; image gradients cannot
// overflow or underflow. This changes last-bit rounding versus hypot.
fn edge_weight(dx: f64, dy: f64, ax: f64, ay: f64) -> Option<f64> {
    let (x, y) = (dx.abs(), dy.abs());
    if x + y <= 18. {
        return None;
    }
    let projection = (dx * ax + dy * ay).abs();
    if projection <= 0.85 * x.max(y) {
        return None;
    }
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    let weight = (dx * dx + dy * dy).sqrt();
    #[cfg(any(feature = "mode-medium", feature = "mode-high"))]
    let weight = dx.hypot(dy);

    (weight > 18. && projection > 0.85 * weight).then_some(weight)
}
// Preserve each mode's original floating-point evaluation order.
fn tensor_magnitude(xx: f64, xy: f64, yy: f64) -> f64 {
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    {
        ((xx - yy) * (xx - yy) + 4. * xy * xy).sqrt()
    }
    #[cfg(any(feature = "mode-medium", feature = "mode-high"))]
    {
        (xx - yy).hypot(2. * xy)
    }
}
fn distance(a: f64, b: f64) -> f64 {
    // Undirected stripe axes repeat every pi. Avoid sin/cos/atan2 in the
    // per-neighbor component and hysteresis searches. Angles come from finite
    // structure tensors; modulo also handles callers outside atan2's range.
    let d = (a - b).abs() % std::f64::consts::PI;
    d.min(std::f64::consts::PI - d)
}
fn angle(ids: &[usize], tiles: &[Tile]) -> f64 {
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for &k in ids {
        xx += tiles[k].xx;
        xy += tiles[k].xy;
        yy += tiles[k].yy;
    }
    0.5 * (2. * xy).atan2(xx - yy)
}
fn bounds(ids: &[usize], angle: f64, tw: usize) -> Bounds {
    let (cos_angle, sin_angle) = (angle.cos(), angle.sin());
    let mut result = Bounds {
        u0: f64::INFINITY,
        u1: f64::NEG_INFINITY,
        v0: f64::INFINITY,
        v1: f64::NEG_INFINITY,
    };
    for &k in ids {
        let (x, y) = (
            crate::numeric::usize_f64(k % tw * 8),
            crate::numeric::usize_f64(k / tw * 8),
        );
        for (px, py) in [(x, y), (x + 8., y), (x, y + 8.), (x + 8., y + 8.)] {
            let (u, v) = (
                px * cos_angle + py * sin_angle,
                -px * sin_angle + py * cos_angle,
            );
            result.u0 = result.u0.min(u);
            result.u1 = result.u1.max(u);
            result.v0 = result.v0.min(v);
            result.v1 = result.v1.max(v);
        }
    }
    result
}
/// Diagnostic geometry uses the localizer's working raster, not source pixels.
#[derive(Debug)]
pub struct GroupDiagnostic {
    pub index: usize,
    pub tiles: usize,
    pub edges: usize,
    pub angle: f64,
    pub initial_angle: f64,
    pub bounds: [f64; 4],
    pub reason: &'static str,
}

mod groups;
mod proposals;
mod quick;
mod raster;
mod refinement;

/// Reusable working-raster storage. Owned by one scanner, never shared globally.
#[derive(Default)]
pub struct Detector {
    raster: raster::Raster,
}
impl Detector {
    /// Whether the latest working raster contains a coherent stripe tile.
    /// This reuses localization evidence without another source-image pass.
    #[must_use]
    pub fn has_stripe_evidence(&self) -> bool {
        self.raster.tiles.iter().any(|tile| tile.active)
    }
    /// Existing coherent tiles near the top, bottom, left and right image edges.
    /// Horizontal discovery needs mostly horizontal gradients, and conversely
    /// for vertical discovery. This schedules additional work only; it never
    /// suppresses an existing proposal or claims that an edge is fully searched.
    #[must_use]
    pub fn border_stripe_evidence(&self) -> [bool; 4] {
        let raster = &self.raster;
        let columns = raster.width.div_ceil(8);
        let margin = crate::numeric::f64_usize((30. * raster.scale).ceil()) + 8;
        let mut evidence = [false; 4];
        for (index, tile) in raster.tiles.iter().enumerate() {
            if !tile.active {
                continue;
            }
            let (x, y) = (index % columns * 8, index / columns * 8);
            if tile.angle.abs() <= std::f64::consts::FRAC_PI_4 {
                evidence[0] |= y < margin;
                evidence[1] |= raster.height.saturating_sub(y + 8) < margin;
            } else {
                evidence[2] |= x < margin;
                evidence[3] |= raster.width.saturating_sub(x + 8) < margin;
            }
        }
        evidence
    }
    /// Detect the primary grid while retaining scratch storage for the next frame.
    /// # Errors
    /// Rejects unsupported image dimensions.
    pub fn detect(&mut self, im: ImageView<'_>) -> std::result::Result<Result, Error> {
        self.detect_grid(im, 768., false, |_| {})
    }
    /// Private experimental grid size; source decoding remains full resolution.
    /// # Errors
    /// Rejects unsupported image dimensions.
    pub fn detect_fast(
        &mut self,
        im: ImageView<'_>,
        dimension: f64,
    ) -> std::result::Result<Result, Error> {
        self.detect_grid(im, dimension.clamp(128., 768.), false, |_| {})
    }
    /// Sparse source-gradient hypotheses for private speed-tier experiments.
    /// # Errors
    /// Rejects unsupported image dimensions.
    pub fn detect_sparse(
        &mut self,
        im: ImageView<'_>,
        dimension: f64,
    ) -> std::result::Result<Result, Error> {
        quick::detect(&mut self.raster, im, dimension.clamp(64., 768.))
    }
    /// Detect the supplementary grid after the primary pass.
    /// # Errors
    /// Rejects unsupported image dimensions.
    pub fn detect_secondary(&mut self, im: ImageView<'_>) -> std::result::Result<Result, Error> {
        self.detect_grid(im, 640., true, |_| {})
    }
    fn detect_grid(
        &mut self,
        im: ImageView<'_>,
        working_dimension: f64,
        bilinear: bool,
        observe: impl FnMut(GroupDiagnostic),
    ) -> std::result::Result<Result, Error> {
        self.raster.prepare(im, working_dimension, bilinear)?;
        let groups = groups::collect(&self.raster);
        let fitted = proposals::fit(im, &self.raster, &groups, observe);
        Ok(refinement::finish(im, self.raster.scale, groups, fitted))
    }
}
/// # Errors
/// Rejects unsupported image dimensions.
pub fn detect(im: ImageView<'_>) -> std::result::Result<Result, Error> {
    Detector::default().detect(im)
}
/// Read-only group trace; the observer never influences scanner decisions.
/// # Errors
/// Rejects unsupported image dimensions.
pub fn detect_with_observer(
    im: ImageView<'_>,
    observe: impl FnMut(GroupDiagnostic),
) -> std::result::Result<Result, Error> {
    Detector::default().detect_grid(im, 768., false, observe)
}
/// Pixel-only supplementary grid. The original detector remains the first pass.
/// # Errors
/// Rejects unsupported image dimensions.
pub fn detect_secondary(im: ImageView<'_>) -> std::result::Result<Result, Error> {
    Detector::default().detect_secondary(im)
}

#[cfg(test)]
mod reuse_tests {
    #[test]
    fn cached_tiles_preserve_threshold_boundaries_and_used_angles() {
        for energy in [0., 5119.999, 5120., 5120.001, 1_000_000.] {
            for coherence in [0., 0.399_999, 0.4, 0.400_001, 0.6, 0.600_001, 0.65, 1.] {
                for xy in [-1., 0., 1.] {
                    for threshold in [0.60, 0.65] {
                        let mut legacy = super::Tile {
                            xx: energy * (1. + coherence) * 0.5,
                            yy: energy * (1. - coherence) * 0.5,
                            xy,
                            ..super::Tile::default()
                        };
                        let mut cached = legacy.clone();
                        legacy.classify_policy(threshold, false);
                        cached.classify_policy(threshold, true);
                        let e = legacy.xx + legacy.yy;
                        let growable = e > 64. * 80.
                            && super::tensor_magnitude(legacy.xx, legacy.xy, legacy.yy) / (e + 1.)
                                >= 0.4;
                        assert_eq!(legacy.active, cached.active);
                        assert_eq!(growable, cached.growable);
                        if cached.active || cached.growable {
                            assert_eq!(legacy.angle.to_bits(), cached.angle.to_bits());
                        }
                    }
                }
            }
        }
    }
    use super::{Detector, ImageView, Result};
    #[test]
    fn border_evidence_tracks_axes_and_clears_between_images() {
        let mut detector = Detector::default();
        for side in 0..4 {
            let mut data = vec![255; 256 * 256];
            for y in 0..256 {
                for x in 0..256 {
                    let (along, across) = if side < 2 { (x, y) } else { (y, x) };
                    let near = if side % 2 == 0 {
                        across < 20
                    } else {
                        across >= 236
                    };
                    if near && (64..192).contains(&along) && along / 3 % 2 == 0 {
                        data[y * 256 + x] = 0;
                    }
                }
            }
            detector
                .detect(ImageView::new(&data, 256, 256, 1, 256).unwrap())
                .unwrap();
            let mut expected = [false; 4];
            expected[side] = true;
            assert_eq!(detector.border_stripe_evidence(), expected);
            data.fill(255);
            detector
                .detect(ImageView::new(&data, 256, 256, 1, 256).unwrap())
                .unwrap();
            assert_eq!(detector.border_stripe_evidence(), [false; 4]);
        }
    }
    fn same(a: &Result, b: &Result) {
        assert_eq!(
            (a.omitted, a.limited, a.trace),
            (b.omitted, b.limited, b.trace)
        );
        assert_eq!(a.proposals.len(), b.proposals.len());
        for (a, b) in a.proposals.iter().zip(&b.proposals) {
            assert_eq!(
                a.polygon.map(|p| p.map(f64::to_bits)),
                b.polygon.map(|p| p.map(f64::to_bits))
            );
            assert_eq!(a.score.to_bits(), b.score.to_bits());
        }
    }
    #[test]
    fn reused_rasters_match_fresh_after_size_layout_and_grid_changes() {
        let mut detector = Detector::default();
        for (width, height, channels, dark) in [
            (801, 177, 4, false),
            (75, 91, 1, true),
            (127, 701, 3, false),
            (801, 177, 4, false),
        ] {
            let stride = width * channels + 7;
            let pixels: Vec<u8> = (0..stride * height)
                .map(|i| {
                    let value = u8::try_from((i * 37 + i / 17) % 256).unwrap();
                    if dark {
                        10 + value % 20
                    } else {
                        value
                    }
                })
                .collect();
            let image = ImageView::new(&pixels, width, height, channels, stride).unwrap();
            for dimension in [64., 80., 128., 256., 320., 768.] {
                same(
                    &detector.detect_sparse(image, dimension).unwrap(),
                    &Detector::default().detect_sparse(image, dimension).unwrap(),
                );
            }
            same(
                &detector.detect(image).unwrap(),
                &Detector::default().detect(image).unwrap(),
            );
            same(
                &detector.detect_secondary(image).unwrap(),
                &Detector::default().detect_secondary(image).unwrap(),
            );
        }
    }
    #[test]
    fn repeated_frames_retain_the_four_large_allocations() {
        let mut detector = Detector::default();
        let pixels = vec![127; 801 * 151];
        let image = ImageView::new(&pixels, 801, 151, 1, 801).unwrap();
        detector.detect(image).unwrap();
        let pointers = (
            detector.raster.gray.as_ptr(),
            detector.raster.gx.as_ptr(),
            detector.raster.gy.as_ptr(),
            detector.raster.tiles.as_ptr(),
        );
        detector.detect(image).unwrap();
        assert_eq!(
            pointers,
            (
                detector.raster.gray.as_ptr(),
                detector.raster.gx.as_ptr(),
                detector.raster.gy.as_ptr(),
                detector.raster.tiles.as_ptr()
            )
        );
    }
    #[test]
    fn stripe_evidence_resets_between_structured_and_empty_frames() {
        let mut detector = Detector::default();
        let mut pixels = vec![255; 128 * 128];
        for y in 20..108 {
            for x in 20..108 {
                if x / 3 % 2 == 0 {
                    pixels[y * 128 + x] = 0;
                }
            }
        }
        detector
            .detect(ImageView::new(&pixels, 128, 128, 1, 128).unwrap())
            .unwrap();
        assert!(detector.has_stripe_evidence());
        pixels.fill(255);
        detector
            .detect(ImageView::new(&pixels, 128, 128, 1, 128).unwrap())
            .unwrap();
        assert!(!detector.has_stripe_evidence());
    }
}
