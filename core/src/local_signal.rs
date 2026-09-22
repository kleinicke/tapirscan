//! Diagnostic local contrast hypothesis on the original normalized signal.
//! Uses observed run scale, never digits/checksum to choose a threshold. No
//! missing transitions are inserted. Not enabled in the production scanner.
use crate::{
    multi_profile::{self, Reads},
    profile::Error,
};
/// Symmetric local envelopes track illumination/print contrast. The radius is
/// six lower-quartile observed runs, enough to span a four-module element.
/// Low local contrast retains the global signal instead of amplifying noise.
#[derive(Default)]
pub(crate) struct Scratch {
    pub(crate) extrema: multi_profile::ExtremaScratch,
    normalized: Vec<f32>,

    envelope: Vec<[f32; 4]>,

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub(crate) runs: multi_profile::LocalRuns,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    runs: multi_profile::LocalRuns,
}
fn quartile_radius(histogram: &[usize; 44], run_count: usize) -> usize {
    let target = run_count / 4;
    let mut seen = 0;
    for (width, &count) in histogram.iter().enumerate() {
        seen += count;
        if seen > target {
            return (width * 6).clamp(6, 256);
        }
    }
    6
}
/// # Errors
/// Returns `Length` for unsupported profile size or `Value` for non-finite samples or values outside [0, 1].
pub fn normalize(p: &[f32]) -> Result<Vec<f32>, Error> {
    let mut scratch = Scratch::default();
    scratch.prepare(p)?;
    Ok(scratch.normalized)
}
impl Scratch {
    fn prepare(&mut self, p: &[f32]) -> Result<(), Error> {
        if !(64..=4096).contains(&p.len()) {
            return Err(Error::Length);
        }
        if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
            return Err(Error::Value);
        }
        self.prepare_validated(p);
        Ok(())
    }
    fn prepare_validated(&mut self, p: &[f32]) {
        let mut histogram = [0usize; 44];
        let (mut run_count, mut start) = (0usize, 0usize);
        let mut dark = p[0] >= 0.5;
        for i in 1..p.len() {
            let next = p[i] >= 0.5;
            if next != dark {
                histogram[(i - start).min(43)] += 1;
                run_count += 1;
                start = i;
                dark = next;
            }
        }
        if run_count < 20 {
            self.normalized.clear();
            self.normalized.extend_from_slice(p);
            return;
        }
        let radius = quartile_radius(&histogram, run_count);
        self.normalized.clear();
        let out = &mut self.normalized;
        // Monotone queues visit every source sample once. This is exactly the same
        // symmetric min/max window as the diagnostic prototype, without rescanning
        // up to 513 values for each output sample on broad/high-resolution paths.

        {
            // Block prefix/suffix extrema produce the identical centered min/max
            // window with predictable contiguous passes. Equal extrema retain the latest
            // source value, including signed zero, as in the original monotone queues.
            let profile_len = p.len();
            let window_len = radius * 2 + 1;
            let last_block = (profile_len - 1) / window_len * window_len;
            self.envelope.resize(profile_len, [0.; 4]);
            let envelope = &mut self.envelope;
            for begin in (0..profile_len).step_by(window_len) {
                let end = (begin + window_len).min(profile_len);
                let (mut lo, mut hi) = (p[begin], p[begin]);
                for j in begin..end {
                    if p[j] <= lo {
                        lo = p[j];
                    }
                    if p[j] >= hi {
                        hi = p[j];
                    }
                    envelope[j][0] = lo;
                    envelope[j][1] = hi;
                }
                let (mut lo, mut hi) = (p[end - 1], p[end - 1]);
                for j in (begin..end).rev() {
                    if p[j] < lo {
                        lo = p[j];
                    }
                    if p[j] > hi {
                        hi = p[j];
                    }
                    envelope[j][2] = lo;
                    envelope[j][3] = hi;
                }
            }
            for (i, p_entry) in p.iter().enumerate().take(profile_len) {
                let a = i.saturating_sub(radius);
                let b = (i + radius).min(profile_len - 1);
                let (lo, hi) = if a == 0 {
                    (envelope[b][0], envelope[b][1])
                } else if b == profile_len - 1 && a >= last_block {
                    (envelope[a][2], envelope[a][3])
                } else {
                    (
                        if envelope[b][0] <= envelope[a][2] {
                            envelope[b][0]
                        } else {
                            envelope[a][2]
                        },
                        if envelope[b][1] >= envelope[a][3] {
                            envelope[b][1]
                        } else {
                            envelope[a][3]
                        },
                    )
                };
                out.push(if hi - lo >= 0.25 {
                    (((*p_entry) - lo) / (hi - lo)).clamp(0., 1.)
                } else {
                    *p_entry
                });
            }
        }
    }
}
/// # Errors
/// Returns `Length` for unsupported profile size or `Value` for non-finite samples or values outside [0, 1].
pub fn decode(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_mode(p, max_symbols, false)
}
/// Optional observed guard correction after local contrast normalization.
/// The scanner enables this only when its explicit guard-bias policy is true.
/// # Errors
/// Returns `Length` for unsupported profile size or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_guard_bias(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_mode(p, max_symbols, true)
}
fn decode_mode(p: &[f32], max_symbols: usize, guard_bias: bool) -> Result<Reads, Error> {
    decode_reusing(p, max_symbols, guard_bias, &mut Scratch::default())
}
pub(crate) fn decode_reusing(
    p: &[f32],
    max_symbols: usize,
    guard_bias: bool,
    scratch: &mut Scratch,
) -> Result<Reads, Error> {
    scratch.prepare(p)?;
    let reads = multi_profile::decode_local_variants(
        &scratch.normalized,
        max_symbols,
        guard_bias,
        &mut scratch.runs,
    )?;

    let reads = crate::transition::merge_reads(
        reads,
        multi_profile::decode_extrema(p, max_symbols, guard_bias, &mut scratch.extrema)?,
        max_symbols,
    );
    Ok(reads)
}
/// Private caller has just validated the unchanged original signal with
/// `sample_runs`; normalized output is generated internally and remains valid.
pub(crate) fn decode_reusing_validated(
    p: &[f32],
    max_symbols: usize,
    guard_bias: bool,
    scratch: &mut Scratch,
) -> Result<Reads, Error> {
    scratch.prepare_validated(p);
    let reads = multi_profile::decode_local_variants_validated(
        &scratch.normalized,
        max_symbols,
        guard_bias,
        &mut scratch.runs,
    )?;

    let reads = crate::transition::merge_reads(
        reads,
        multi_profile::decode_extrema_validated(p, max_symbols, guard_bias, &mut scratch.extrema),
        max_symbols,
    );
    Ok(reads)
}

/// Only call immediately after `decode_reusing` on the same scratch/profile.
pub(crate) fn prepared_short(scratch: &Scratch, max_symbols: usize, guard_bias: bool) -> Reads {
    multi_profile::decode_prepared_short(&scratch.runs, max_symbols, guard_bias)
}

pub(crate) fn reused_calls(scratch: &Scratch) -> usize {
    scratch.runs.reused_calls.get()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_quartile_matches_sort_at_clamp_boundaries_and_reuse() {
        let cases = vec![
            vec![1usize; 20],
            (1..=43).collect(),
            vec![2usize; 80],
            vec![43usize; 20],
            (1..=200).map(|i| if i % 3 == 0 { 1 } else { 43 }).collect(),
        ];
        for widths in cases {
            let mut hist = [0usize; 44];
            for &w in &widths {
                hist[w.min(43)] += 1;
            }
            let mut sorted = widths.clone();
            sorted.sort_unstable();
            let expected = (sorted[sorted.len() / 4] * 6).clamp(6, 256);
            assert_eq!(quartile_radius(&hist, widths.len()), expected);
        }
        let mut scratch = Scratch::default();
        let p = (0..512)
            .map(|i| if (i / 7) % 2 == 0 { 0.1 } else { 0.9 })
            .collect::<Vec<_>>();
        scratch.prepare(&p).unwrap();
        let first = scratch.normalized.clone();
        scratch.prepare(&p).unwrap();
        assert_eq!(scratch.normalized, first);
    }
    const BITS:&[u8]=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn signal() -> Vec<f32> {
        let mut p = vec![0.; 36];
        for b in BITS {
            p.extend([f32::from(*b == b'1'); 3]);
        }
        p.extend([0.; 36]);
        p
    }
    #[test]
    fn clean_reverse_and_separate_equal_symbols() {
        let p = signal();
        let d = decode(&p, 64).unwrap();
        assert_eq!(d.symbols.len(), 1);
        let mut rev = p.clone();
        rev.reverse();
        assert_eq!(
            decode(&rev, 64).unwrap().symbols[0].digits,
            d.symbols[0].digits
        );
        let mut two = p.clone();
        two.extend(p);
        assert_eq!(decode(&two, 64).unwrap().symbols.len(), 2);
    }
    #[test]
    fn local_illumination_preserves_visual_value() {
        let mut p = signal();
        let n = crate::numeric::usize_f32(p.len());
        for (i, v) in p.iter_mut().enumerate() {
            let offset = 0.5 * crate::numeric::usize_f32(i) / n;
            *v = offset + 0.4 * (*v);
        }
        assert_eq!(
            decode(&p, 64).unwrap().symbols[0].digits,
            [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
        );
    }
    #[test]
    fn observed_guard_bias_is_opt_in() {
        for bias in [-7isize, 7] {
            let mut p = vec![0.; 240];
            let mut i = 0;
            while i < BITS.len() {
                let mut j = i + 1;
                while j < BITS.len() && BITS[i] == BITS[j] {
                    j += 1;
                }
                let dark = BITS[i] == b'1';
                let width = ((j - i) * 20).cast_signed() + if dark { bias } else { -bias };
                p.extend(std::iter::repeat_n(
                    f32::from(dark),
                    (width).cast_unsigned(),
                ));
                i = j;
            }
            p.extend([0.; 240]);
            assert!(decode(&p, 64).unwrap().symbols.is_empty());
            assert_eq!(
                decode_guard_bias(&p, 64).unwrap().symbols[0].digits,
                [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
            );
        }
    }
    #[test]
    fn negatives_and_invalid() {
        for p in [
            vec![0.; 512],
            vec![1.; 512],
            (0..512)
                .map(|i| crate::numeric::f64_f32(f64::from(i % 2)))
                .collect(),
            (0..512)
                .map(|i| crate::numeric::f64_f32(f64::from(i)) / 511.)
                .collect(),
        ] {
            assert!(decode(&p, 64).unwrap().symbols.is_empty());
        }
        assert!(normalize(&[0.; 63]).is_err());
        assert!(normalize(&[f32::NAN; 512]).is_err());
    }
    #[test]
    fn queued_envelopes_equal_brute_windows() {
        let mut seed = 71u32;
        for n in [64, 357, 512, 4096] {
            for stretch in [1, 3, 11, 80] {
                let mut p = Vec::new();
                while p.len() < n {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let v = crate::numeric::f64_f32(f64::from(seed >> 8)) / 16_777_215.;
                    for _ in 0..stretch {
                        if p.len() < n {
                            p.push(v);
                        }
                    }
                }
                let mut widths = Vec::new();
                let mut start = 0;
                for i in 1..n {
                    if (p[i] >= 0.5) != (p[i - 1] >= 0.5) {
                        widths.push(i - start);
                        start = i;
                    }
                }
                let expected = if widths.len() < 20 {
                    p.clone()
                } else {
                    widths.sort_unstable();
                    let radius = (widths[widths.len() / 4] * 6).clamp(6, 256);
                    (0..n)
                        .map(|i| {
                            let a = &p[i.saturating_sub(radius)..(i + radius + 1).min(n)];
                            let lo = a.iter().copied().fold(f32::INFINITY, f32::min);
                            let hi = a.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                            if hi - lo >= 0.25 {
                                ((p[i] - lo) / (hi - lo)).clamp(0., 1.)
                            } else {
                                p[i]
                            }
                        })
                        .collect()
                };
                assert_eq!(normalize(&p).unwrap(), expected);
            }
        }
    }
}
