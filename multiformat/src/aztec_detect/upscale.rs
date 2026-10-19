//! Enlarged retry for small Aztec symbols.
//!
//! A symbol whose modules span about 1.5 pixels has a recognizable bullseye but
//! too little detail for the grid samplers. The retry runs only for bullseye
//! candidates that no threshold pass turned into a read. It scans a bounded
//! window around each such candidate, resampled with Catmull-Rom weights, and
//! accepts a read only when its center coincides with the candidate.
use super::{Center, Search, binarization, detect};
use crate::{Detection, numeric, regions};

/// Most unresolved bullseye candidates a search remembers.
pub(super) const MAX_UNRESOLVED: usize = 32;
/// Half the window edge around the candidate, in bullseye ring widths, and its
/// limit in pixels. Smaller windows lose the surrounding contrast that the
/// thresholds adapt to.
const WINDOW_HALF_MODULES: f32 = 48.;
const WINDOW_HALF_LIMIT: f32 = 150.;
/// Ring widths, in enlarged pixels, the factors aim for.
const TARGET_MODULES: [f32; 3] = [4., 6., 3.];
/// Largest and smallest enlargement factors.
const MAX_FACTOR: usize = 4;
const MIN_FACTOR: usize = 2;
/// A read belongs to the candidate when its center lies within this many rings.
const CENTER_RINGS: f32 = 4.;
/// Candidates closer than this many rings are one bullseye.
const SAME_CANDIDATE_RINGS: f32 = 3.;
/// A Rune carries one number from 0 to 255 in 28 bits. Its decoder is too
/// permissive to trust on a resampled window, where it read a fragment of a
/// neighboring symbol as "011".
const RUNE_DIGITS: usize = 3;

/// Whether `point` lies within `rings` ring widths of the candidate.
fn near(c: &Center, point: [f32; 2], rings: f32) -> bool {
    (point[0] - c.x).hypot(point[1] - c.y) < c.module * rings
}

/// Whether `text` could be the value of a Rune, which a regular symbol of the
/// same text would be indistinguishable from.
fn rune_like(text: &str) -> bool {
    text.len() <= RUNE_DIGITS && text.bytes().all(|b| b.is_ascii_digit())
}
/// Overlap at which two reads of one text are one symbol, as in the final merge.
const DUPLICATE_OVERLAP: f32 = 0.65;

/// Limits of one scan's retries. Photos without Aztec still produce bullseye-like
/// candidates (barcode quiet zones, text), most of them with a poor ring fit or
/// wide rings, so Medium keeps only candidates that fit well at small sizes.
struct Policy {
    /// Candidates retried, best ring fit first.
    targets: usize,
    /// Enlargement factors tried per candidate, from `TARGET_MODULES`.
    factors: usize,
    /// Enlarged pixels all windows of one scan may search (about 5 ms per
    /// 200,000 pixels).
    pixels: usize,
    /// Worst accepted ring mismatch (of 81 cells) and widest accepted ring, in pixels.
    max_fit: usize,
    max_module: f32,
}
const MEDIUM: Policy = Policy {
    targets: 2,
    factors: 2,
    pixels: 600_000,
    max_fit: 18,
    max_module: 2.5,
};
const WIDE: Policy = Policy {
    targets: 3,
    factors: 3,
    pixels: 1_600_000,
    max_fit: 33,
    max_module: 5.,
};

fn center_of(polygon: &[[f32; 2]; 4]) -> [f32; 2] {
    polygon
        .iter()
        .fold([0., 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25])
}

impl Search {
    /// Retry bullseye candidates that stayed unread, on enlarged windows.
    ///
    /// `effort` 1 (Medium) uses the `MEDIUM` limits, higher efforts `WIDE`. The
    /// best-fitting candidates go first, and a candidate stops at its first read.
    pub fn upscaled(&mut self, gray: &[u8], width: usize, height: usize, effort: usize) {
        if effort == 0 || self.unresolved.is_empty() || gray.len() < width * height {
            return;
        }
        let policy = if effort == 1 { &MEDIUM } else { &WIDE };
        let mut targets: Vec<Center> = Vec::new();
        let mut unresolved = std::mem::take(&mut self.unresolved);
        unresolved.sort_by_key(|(fit, c)| (*fit, std::cmp::Reverse(c.support)));
        for (fit, c) in unresolved {
            if fit > policy.max_fit
                || c.module > policy.max_module
                || targets.len() == policy.targets
                || targets
                    .iter()
                    .any(|t| near(&c, [t.x, t.y], SAME_CANDIDATE_RINGS))
                || self
                    .reads
                    .iter()
                    .any(|d| near(&c, center_of(&d.polygon), CENTER_RINGS))
            {
                continue;
            }
            targets.push(c);
        }
        let mut pixels = policy.pixels;
        for c in targets {
            // An earlier target's window may have read this candidate's symbol.
            if self
                .reads
                .iter()
                .any(|d| near(&c, center_of(&d.polygon), CENTER_RINGS))
            {
                continue;
            }
            self.retry_candidate(&c, gray, [width, height], policy.factors, &mut pixels);
        }
    }

    fn retry_candidate(
        &mut self,
        c: &Center,
        gray: &[u8],
        size: [usize; 2],
        tries: usize,
        pixels: &mut usize,
    ) {
        let [width, height] = size;
        let half = (c.module * WINDOW_HALF_MODULES).min(WINDOW_HALF_LIMIT);
        let bound = |center: f32, limit: usize| {
            let low = numeric::f32_usize((center - half).floor().max(0.));
            let high = numeric::f32_usize((center + half).ceil().max(0.)).min(limit);
            (low, high.saturating_sub(low))
        };
        let (x0, window_width) = bound(c.x, width);
        let (y0, window_height) = bound(c.y, height);
        if window_width < 16 || window_height < 16 {
            return;
        }
        let mut factors: Vec<usize> = Vec::new();
        for target in &TARGET_MODULES[..tries] {
            let factor =
                numeric::f32_usize((target / c.module).round()).clamp(MIN_FACTOR, MAX_FACTOR);
            if !factors.contains(&factor) {
                factors.push(factor);
            }
        }
        for factor in factors {
            let cost = window_width * window_height * factor * factor;
            if cost > *pixels {
                self.limited = true;
                continue;
            }
            *pixels -= cost;
            let (larger, out_width, out_height) = crate::resample::catmull_rom_enlarge(
                gray,
                width,
                height,
                [x0, y0, window_width, window_height],
                factor,
            );
            let mut images = binarization::Images::new(&larger, out_width, out_height);
            let mut ignored = regions::Regions::default();
            let (reads, limited) = detect(
                out_width,
                out_height,
                &mut ignored,
                &larger,
                &mut images,
                self.policy,
                0,
            );
            self.limited |= limited;
            let scale = numeric::usize_f32(factor);
            let mapped = reads.into_iter().filter_map(|mut read: Detection| {
                for point in &mut read.polygon {
                    point[0] = point[0] / scale + numeric::usize_f32(x0);
                    point[1] = point[1] / scale + numeric::usize_f32(y0);
                }
                let center = center_of(&read.polygon);
                (!rune_like(&read.text) && near(c, center, CENTER_RINGS)).then_some(read)
            });
            let mut found: Vec<Detection> = Vec::new();
            for read in mapped {
                // Thresholds of the window may read one symbol several times.
                if !found.iter().any(|prior| {
                    prior.text == read.text
                        && regions::overlap(&prior.polygon, &read.polygon) >= DUPLICATE_OVERLAP
                }) {
                    found.push(read);
                }
            }
            if found.is_empty() {
                continue;
            }
            self.reads.extend(found);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aztec_detect::Policy;

    /// A 27-module compact symbol (4 layers), generated independently with ZXing-C++.
    const ROWS: [&str; 27] = [
        "010001010000110101101000011",
        "110011100100001001101011100",
        "010010101101000011110111101",
        "001001001101011100000001101",
        "001010001110111000110110000",
        "000100100111110010011110001",
        "101000010111101011100010101",
        "100001111011101011100110011",
        "010000101111101110101111011",
        "100110001111111111111010000",
        "000011101100000001111111000",
        "001011011101111101100100101",
        "010000001101000101010000000",
        "001010010101010101001110010",
        "100111000101000101110000000",
        "000000010101111101101001110",
        "100000110100000001101000001",
        "010101110111111111101001000",
        "001110000000111100000001000",
        "001101011110110000001010000",
        "010101000010111100101110011",
        "110011011101010111010100100",
        "110111111001001010011010110",
        "011110000101101011000011110",
        "000000111010110010000100000",
        "111100001011010110000101000",
        "000001110101100100001001101",
    ];
    const TEXT: &str = "MODE-CONSISTENCY-MODE-CONSISTENCY-MODE-CONSISTENCY-MODE-CONSISTENCY-";

    /// The symbol at `pixels` per module, averaged over each pixel, on a light
    /// frame whose center holds the symbol.
    fn frame(pixels: f32, side: usize) -> (Vec<u8>, [f32; 2]) {
        let center = numeric::usize_f32(side) / 2.;
        let origin = center - 13.5 * pixels;
        let dark = |x: f32, y: f32| {
            let (mx, my) = ((x - origin) / pixels, (y - origin) / pixels);
            (0. ..27.).contains(&mx)
                && (0. ..27.).contains(&my)
                && ROWS[numeric::f32_usize(my)].as_bytes()[numeric::f32_usize(mx)] == b'1'
        };
        let mut gray = vec![0_u8; side * side];
        for (i, value) in gray.iter_mut().enumerate() {
            let (x, y) = (numeric::usize_f32(i % side), numeric::usize_f32(i / side));
            let covered = (0..16)
                .filter(|&s| {
                    dark(
                        x + numeric::usize_f32(s % 4) * 0.25,
                        y + numeric::usize_f32(s / 4) * 0.25,
                    )
                })
                .count();
            *value = numeric::f32_u8((215. - 150. * numeric::usize_f32(covered) / 16.).round());
        }
        (gray, [center, center])
    }

    /// The policies of the scanner modes.
    const POLICIES: [Policy; 2] = [Policy::LOW, Policy::STANDARD];

    fn read(gray: &[u8], side: usize, policy: Policy, effort: usize) -> Vec<Detection> {
        let mut images = binarization::Images::new(gray, side, side);
        detect(
            side,
            side,
            &mut regions::Regions::default(),
            gray,
            &mut images,
            policy,
            effort,
        )
        .0
    }

    #[test]
    fn small_symbol_reads_only_on_the_enlarged_window() {
        let side = 160;
        // Modules of 1.3 pixels leave the grid samplers unable to read the symbol
        // although its bullseye is found.
        let (gray, center) = frame(1.3, side);
        for policy in POLICIES {
            assert!(read(&gray, side, policy, 0).is_empty());
            let retried = read(&gray, side, policy, 1);
            assert_eq!(retried.len(), 1);
            assert_eq!(retried[0].text, TEXT);
            let at = center_of(&retried[0].polygon);
            assert!((at[0] - center[0]).hypot(at[1] - center[1]) < 2.6);
            // Neighboring sizes never produce a different text.
            for pixels in [1.2, 1.4, 1.5] {
                let (gray, _) = frame(pixels, side);
                for effort in [1, 2] {
                    assert!(
                        read(&gray, side, policy, effort)
                            .iter()
                            .all(|r| r.text == TEXT)
                    );
                }
            }
        }
    }

    #[test]
    fn frames_without_a_bullseye_run_no_retry() {
        let side = 160;
        let gray: Vec<u8> = (0..side * side)
            .map(|i| {
                if (i % side / 3 + i / side / 5) % 2 == 0 {
                    40
                } else {
                    210
                }
            })
            .collect();
        for policy in POLICIES {
            let mut search = Search::new(policy);
            search.upscaled(&gray, side, side, 2);
            assert!(search.reads.is_empty() && !search.limited);
            assert!(read(&gray, side, policy, 2).is_empty());
        }
    }

    #[test]
    fn enlarging_keeps_source_geometry() {
        let (width, height) = (40, 30);
        let gray: Vec<u8> = (0..width * height)
            .map(|i| u8::try_from(i % width * 6).expect("ramp fits u8"))
            .collect();
        for factor in [2, 3] {
            let (larger, out_width, out_height) =
                crate::resample::catmull_rom_enlarge(&gray, width, height, [5, 4, 20, 10], factor);
            assert_eq!((out_width, out_height), (20 * factor, 10 * factor));
            // Away from the borders a linear ramp is reproduced exactly: the
            // enlarged pixel x sits at source x0 + (x + 0.5) / factor - 0.5.
            for x in 2 * factor..out_width - 2 * factor {
                let source = numeric::usize_f32(x) + 0.5;
                let expected = 6. * (5. + source / numeric::usize_f32(factor) - 0.5);
                let value = f32::from(larger[5 * out_width + x]);
                assert!(
                    (value - expected).abs() <= 1.,
                    "x {x}: {value} vs {expected}"
                );
            }
        }
    }
}
