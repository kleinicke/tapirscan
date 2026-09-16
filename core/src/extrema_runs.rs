//! Bounded observed-prominence topology, no payload-guided edges or repairs.
use super::{decode_positions, Error, Reads};
const PROMINENCE: f32 = 0.04;
const MAX_EXAMINED: usize = 8192;
#[derive(Default)]
pub(crate) struct ExtremaScratch {
    extrema: Vec<(usize, bool)>,
    runs: Vec<(f64, f64, bool)>,
    pub attempted: bool,
    pub examined: usize,
    pub capped: bool,
    pub ambiguous: bool,
    pub decoder_calls: usize,
}
impl ExtremaScratch {
    fn charge(&mut self) -> bool {
        if self.examined == MAX_EXAMINED {
            self.capped = true;
            false
        } else {
            self.examined += 1;
            true
        }
    }
    fn empty(&self, max: usize) -> Reads {
        let mut r = decode_positions::<f64>(&[], max, false);
        r.truncated = self.capped;
        r
    }
}
pub(crate) fn decode_extrema(
    p: &[f32],
    max: usize,
    guard: bool,
    s: &mut ExtremaScratch,
) -> Result<Reads, Error> {
    if !(64..=4096).contains(&p.len()) || !(1..=64).contains(&max) {
        return Err(Error::Length);
    }
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err(Error::Value);
    }
    Ok(decode_extrema_validated(p, max, guard, s))
}
/// Private caller has already validated profile length and values.
#[expect(
    clippy::float_cmp,
    reason = "A plateau consists of identical observed sample values; approximate equality would merge distinct extrema."
)]
#[expect(
    clippy::too_many_lines,
    reason = "Extrema tracking, hysteresis and decode assembly share a single bounded work counter and ambiguity state."
)]
pub(crate) fn decode_extrema_validated(
    p: &[f32],
    max: usize,
    guard: bool,
    scratch: &mut ExtremaScratch,
) -> Reads {
    scratch.extrema.clear();
    scratch.runs.clear();
    scratch.attempted = false;
    scratch.examined = 0;
    scratch.capped = false;
    scratch.ambiguous = false;
    scratch.decoder_calls = 0;
    let raw = 1 + p
        .windows(2)
        .filter(|a| (a[0] >= 0.5) != (a[1] >= 0.5))
        .count();
    // Broad observed-structure gate; no whole-profile early success exit.
    if !(45..=512).contains(&raw) {
        return scratch.empty(max);
    }
    scratch.attempted = true;
    // Linear hysteresis: a direction change requires observed amplitude >=.04.
    // Track plateau centers; only actual extrema and endpoint values create edges.
    let (mut low, mut high) = (0usize, 0usize);
    let mut direction = 0i8;
    let (mut first, mut last) = (0usize, 0usize);
    for i in 1..p.len() {
        if !scratch.charge() {
            return scratch.empty(max);
        }
        if direction == 0 {
            if p[i] < p[low] {
                low = i;
            }
            if p[i] > p[high] {
                high = i;
            }
            if p[i] - p[low] >= PROMINENCE {
                scratch.extrema.push((low, false));
                direction = 1;
                first = i;
                last = i;
            } else if p[high] - p[i] >= PROMINENCE {
                scratch.extrema.push((high, true));
                direction = -1;
                first = i;
                last = i;
            }
        } else if direction == 1 {
            if p[i] > p[first] {
                first = i;
                last = i;
            } else if p[i] == p[first] && i == last + 1 {
                last = i;
            }
            if p[first] - p[i] >= PROMINENCE {
                scratch.extrema.push((usize::midpoint(first, last), true));
                direction = -1;
                first = i;
                last = i;
            }
        } else {
            if p[i] < p[first] {
                first = i;
                last = i;
            } else if p[i] == p[first] && i == last + 1 {
                last = i;
            }
            if p[i] - p[first] >= PROMINENCE {
                scratch.extrema.push((usize::midpoint(first, last), false));
                direction = 1;
                first = i;
                last = i;
            }
        }
    }
    if direction != 0 {
        scratch
            .extrema
            .push((usize::midpoint(first, last), direction == 1));
    }
    if scratch.extrema.len() < 2 {
        return scratch.empty(max);
    }
    let mut start = 0.;
    #[cfg(feature = "experimental-local-extrema-gaps")]
    let mut pending_start = false;
    #[cfg(feature = "experimental-local-extrema-gaps")]
    let mut chunks = Vec::<Reads>::new();
    for k in 1..scratch.extrema.len() {
        let (a, color) = scratch.extrema[k - 1];
        let (b, _) = scratch.extrema[k];
        let threshold = (f64::from(p[a]) + f64::from(p[b])) * 0.5;
        let mut crossing = None;
        for j in a + 1..=b {
            if !scratch.charge() {
                return scratch.empty(max);
            }
            let (v, width) = (f64::from(p[j - 1]), f64::from(p[j]));
            if (v >= threshold) != (width >= threshold) {
                // More than one observed crossing is ambiguous, never choose by digits.
                if crossing.is_some() {
                    scratch.ambiguous = true;
                    #[cfg(not(feature = "experimental-local-extrema-gaps"))]
                    return scratch.empty(max);
                    #[cfg(feature = "experimental-local-extrema-gaps")]
                    {
                        crossing = None;
                        break;
                    }
                }
                crossing = Some(crate::numeric::usize_f64(j) - 0.5 + (threshold - v) / (width - v));
            }
        }
        let Some(end) = crossing else {
            scratch.ambiguous = true;
            #[cfg(not(feature = "experimental-local-extrema-gaps"))]
            return scratch.empty(max);
            #[cfg(feature = "experimental-local-extrema-gaps")]
            {
                let a = decode_positions(&scratch.runs, max, false);
                let r = if guard {
                    crate::transition::merge_reads(
                        a,
                        decode_positions(&scratch.runs, max, true),
                        max,
                    )
                } else {
                    a
                };
                chunks.push(r);
                scratch.runs.clear();
                pending_start = true;
                continue;
            }
        };
        #[cfg(feature = "experimental-local-extrema-gaps")]
        if pending_start {
            start = end;
            pending_start = false;
            continue;
        }
        if end <= start {
            scratch.ambiguous = true;
            return scratch.empty(max);
        }
        scratch.runs.push((start, end, color));
        start = end;
    }
    #[cfg(not(feature = "experimental-local-extrema-gaps"))]
    scratch
        .runs
        .push((start, p.len() as f64, scratch.extrema.last().unwrap().1));
    #[cfg(feature = "experimental-local-extrema-gaps")]
    if !pending_start {
        scratch.runs.push((
            start,
            crate::numeric::usize_f64(p.len()),
            scratch.extrema.last().unwrap().1,
        ));
    }
    let a = decode_positions(&scratch.runs, max, false);
    let reads = if guard {
        crate::transition::merge_reads(a, decode_positions(&scratch.runs, max, true), max)
    } else {
        a
    };
    #[cfg(feature = "experimental-local-extrema-gaps")]
    let reads = if scratch.ambiguous {
        let strict = |mut r: Reads| {
            r.symbols.retain(|v| v.cost <= 0.06 && v.gap >= 0.1);
            r
        };
        chunks.into_iter().fold(strict(reads), |all, part| {
            crate::transition::merge_reads(all, strict(part), max)
        })
    } else {
        reads
    };
    scratch.decoder_calls = reads.decoder_calls;
    reads
}
#[cfg(test)]
mod tests {
    use super::*;
    const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    fn signal(d: [u8; 13]) -> Vec<f32> {
        let mut p = vec![0.; 40];
        for b in crate::ean::encode(&d) {
            p.extend([b; 4]);
        }
        p.extend([0.; 40]);
        p
    }
    #[test]
    fn clean_reverse_and_invalid_checksum() {
        for bad in [false, true] {
            let mut d = A;
            if bad {
                d[12] = 8;
            }
            let mut p = signal(d);
            for _ in 0..2 {
                let r = decode_extrema(&p, 64, true, &mut ExtremaScratch::default()).unwrap();
                assert!(!r.truncated);
                assert_eq!(r.symbols.len(), usize::from(!bad));
                if !bad {
                    assert_eq!(r.symbols[0].digits, d);
                }
                p.reverse();
            }
        }
    }
    #[test]
    fn actual_blurred_profiles_recover_without_invented_edges() {
        for text in [
            include_str!("../fixtures/blurred_0.1.txt"),
            include_str!("../fixtures/blurred_0.9.txt"),
        ] {
            let mut p: Vec<f32> = text
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            for _ in 0..2 {
                let mut scratch = ExtremaScratch::default();
                let reads = decode_extrema(&p, 64, false, &mut scratch).unwrap();
                assert!(scratch.attempted);
                assert!(!scratch.capped);
                assert!(!scratch.ambiguous);
                assert_eq!(reads.symbols.len(), 1);
                assert_eq!(
                    reads.symbols[0].digits,
                    [4, 3, 1, 1, 5, 0, 1, 6, 2, 5, 2, 1, 7]
                );
                assert!(reads.decoder_calls > 0);
                assert!(scratch.examined <= MAX_EXAMINED);
                p.reverse();
            }
        }
    }
    #[test]
    fn multicode_merge_preserves_equal_and_different_positions() {
        let mut p = signal(A);
        p.extend(signal(B));
        p.extend(signal(A));
        let mut scratch = crate::local_signal::Scratch::default();
        let reads = crate::local_signal::decode_reusing(&p, 64, true, &mut scratch).unwrap();
        assert_eq!(reads.symbols.len(), 3);
        assert_eq!(reads.symbols.iter().filter(|r| r.digits == A).count(), 2);
        assert_eq!(reads.symbols.iter().filter(|r| r.digits == B).count(), 1);
    }
    #[test]
    fn blank_noise_bounds_and_conflicting_models() {
        let mut seed = 17u32;
        for n in [64, 512, 4096] {
            for kind in 0..3 {
                let p: Vec<_> = (0..n)
                    .map(|i| match kind {
                        0 => 0.,
                        1 => crate::numeric::f64_f32(f64::from(i % 2)),
                        _ => {
                            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            crate::numeric::f64_f32(f64::from(seed >> 8)) / 16_777_215.
                        }
                    })
                    .collect();
                let mut s = ExtremaScratch::default();
                let r = decode_extrema(&p, 64, true, &mut s).unwrap();
                assert!(r.symbols.is_empty());
                assert!(s.examined <= MAX_EXAMINED);
            }
        }
        assert!(
            decode_extrema(&[f32::NAN; 64], 64, false, &mut ExtremaScratch::default()).is_err()
        );
        let a = decode_extrema(&signal(A), 64, false, &mut ExtremaScratch::default()).unwrap();
        let mut b = a.clone();
        b.symbols[0].digits = B;
        let r = crate::transition::merge_reads(a, b, 64);
        assert!(r.symbols.is_empty());
        assert!(!r.rejected_intervals.is_empty());
    }
}

#[cfg(all(test, feature = "experimental-local-extrema-gaps"))]
mod local_gap_tests {
    use super::*;
    fn symbol(d: [u8; 13]) -> Vec<f32> {
        let mut p = vec![0.; 40];
        for b in crate::ean::encode(&d) {
            p.extend([b; 4]);
        }
        p.extend([0.; 40]);
        p
    }
    #[test]
    fn unrelated_ambiguous_crossing_does_not_hide_two_complete_symbols() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let mut profile = symbol(a);
        profile.extend([1., 0., 1., 0.49, 0.51, 0.49, 0., 1., 0.]);
        profile.extend(symbol(b));
        for _ in 0..2 {
            let mut s = ExtremaScratch::default();
            let r = decode_extrema(&profile, 64, true, &mut s).unwrap();
            assert!(s.ambiguous);
            assert_eq!(r.symbols.len(), 2);
            assert!(r.symbols.iter().any(|r| r.digits == a));
            assert!(r.symbols.iter().any(|r| r.digits == b));
            profile.reverse();
        }
    }
    #[test]
    fn ambiguous_guard_is_not_repaired() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut p = symbol(a);
        p.splice(44..48, [0.49, 0.51, 0.49, 0.]);
        let r = decode_extrema(&p, 64, true, &mut ExtremaScratch::default()).unwrap();
        assert!(r.symbols.is_empty());
    }
}
