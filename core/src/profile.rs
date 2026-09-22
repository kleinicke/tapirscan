//! Bounded EAN-13 boundary search using continuous module-center evidence.
//! Experimental: fixed 512-sample normalized profile, both directions, at most
//! 100 boundary pairs/direction and four guard-ranked candidates/direction.
#![forbid(unsafe_code)]
use crate::ean;
pub const LEN: usize = 512;
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Value,
}
#[derive(Clone, Copy, Debug)]
pub struct Read {
    pub digits: [u8; 13],
    pub cost: f32,
    pub gap: f32,
    pub left: f32,
    pub right: f32,
    pub reversed: bool,
}
#[derive(Clone, Copy)]
struct Hypothesis {
    modules: [f32; 95],
    guard: f32,
    left: f32,
    right: f32,
}
const EMPTY: Hypothesis = Hypothesis {
    modules: [0.; 95],
    guard: f32::INFINITY,
    left: 0.,
    right: 0.,
};
fn value(p: &[f32], i: usize, reverse: bool) -> f32 {
    p[if reverse { p.len() - 1 - i } else { i }]
}
fn sample(p: &[f32], x: f32, reverse: bool) -> f32 {
    let x = x.clamp(0., crate::numeric::usize_f32(p.len() - 1));
    let i = crate::numeric::f32_usize(x.floor());
    let a = value(p, i, reverse);
    a + (value(p, (i + 1).min(p.len() - 1), reverse) - a) * (x - crate::numeric::usize_f32(i))
}
/// No allocation or caller mutation. Rejection is distinct from invalid input.
/// A competing accepted text among searched candidates rejects the whole path.
fn module_sample(p: &[f32], left: f32, right: f32, i: usize, reverse: bool) -> f32 {
    let pitch = (right - left) / 95.;
    let center = left + (crate::numeric::usize_f32(i) + 0.5) * pitch;
    {
        (sample(p, center - pitch / 6., reverse)
            + sample(p, center, reverse)
            + sample(p, center + pitch / 6., reverse))
            / 3.
    }
}
/// EAN guard/digit max cost 0.1 and per-digit ambiguity gap 0.02 stay unchanged.
/// # Errors
/// Returns `Length` unless the profile has 512 samples, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode(p: &[f32]) -> Result<Option<Read>, Error> {
    decode_checked(p, true)
}

#[derive(Default, Debug)]
pub struct BlurTrace {
    pub boundary_pairs: usize,
    pub digit_hypotheses: usize,
    pub gated_windows: usize,
    pub model_attempts: usize,
    pub accepted_windows: usize,
    pub conflicts: usize,
    pub rejected_intervals: Vec<(f32, f32)>,
}

pub fn decode_with_blur_trace(p: &[f32]) -> (Result<Option<Read>, Error>, BlurTrace) {
    let mut trace = BlurTrace::default();
    let result = decode_impl(p, true, true, &mut trace);
    (result, trace)
}
fn decode_checked(p: &[f32], precheck: bool) -> Result<Option<Read>, Error> {
    decode_checked_with_prune(p, precheck, true)
}
fn decode_checked_with_prune(
    p: &[f32],
    precheck: bool,
    prune: bool,
) -> Result<Option<Read>, Error> {
    decode_impl(p, precheck, prune, &mut BlurTrace::default())
}
fn decode_impl(
    p: &[f32],
    precheck: bool,
    prune: bool,
    trace: &mut BlurTrace,
) -> Result<Option<Read>, Error> {
    decode_impl_length(p, precheck, prune, false, trace)
}
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
pub(crate) fn decode_native_with_blur_trace(p: &[f32]) -> (Result<Option<Read>, Error>, BlurTrace) {
    let mut trace = BlurTrace::default();
    let result = decode_impl_length(p, true, true, true, &mut trace);
    (result, trace)
}
#[expect(
    clippy::too_many_lines,
    reason = "The ordered boundary search shares one accepted identity and ambiguity veto across every window and blur model."
)]
fn decode_impl_length(
    p: &[f32],
    precheck: bool,
    prune: bool,
    native: bool,
    trace: &mut BlurTrace,
) -> Result<Option<Read>, Error> {
    if if native {
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                !(76..=384).contains(&p.len())
            }
            #[cfg(feature = "mode-very-high")]
            {
                !(76..=1536).contains(&p.len())
            }
        }
    } else {
        p.len() != LEN
    } {
        return Err(Error::Length);
    }
    if p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return Err(Error::Value);
    }
    let mut accepted: Option<Read> = None;
    for reverse in [false, true] {
        let (mut starts, mut ends) = ([0.; 10], [0.; 10]);
        let (mut ns, mut ne) = (0, 0);
        for i in 1..p.len() {
            let (a, b) = (value(p, i - 1, reverse), value(p, i, reverse));
            if a < 0.5
                && b >= 0.5
                && crate::numeric::usize_f32(i) < crate::numeric::usize_f32(p.len()) * 0.35
                && ns < 10
            {
                starts[ns] = crate::numeric::usize_f32(i) - 0.5;
                ns += 1;
            }
            if a >= 0.5
                && b < 0.5
                && crate::numeric::usize_f32(i) > crate::numeric::usize_f32(p.len()) * 0.65
            {
                if ne < 10 {
                    ends[ne] = crate::numeric::usize_f32(i) - 0.5;
                    ne += 1;
                } else {
                    ends.copy_within(1..10, 0);
                    ends[9] = crate::numeric::usize_f32(i) - 0.5;
                }
            }
        }

        {
            trace.boundary_pairs += ns * ne;
            trace.digit_hypotheses += (ns * ne).min(4);
        }
        let mut best = [EMPTY; 4];
        for &left in &starts[..ns] {
            'hypothesis: for &right in &ends[..ne] {
                let mut h = Hypothesis {
                    left,
                    right,
                    ..EMPTY
                };

                h.guard = 0.;
                for (i, b) in [
                    (0, 1.),
                    (1, 0.),
                    (2, 1.),
                    (45, 0.),
                    (46, 1.),
                    (47, 0.),
                    (48, 1.),
                    (49, 0.),
                    (92, 1.),
                    (93, 0.),
                    (94, 1.),
                ] {
                    let value = module_sample(p, left, right, i, reverse);

                    h.guard += (value - b).powi(2) / 11.;
                    // Guard terms are nonnegative and insertion uses a
                    // strict `<` comparison. Once this hypothesis reaches the
                    // current fourth-best score it cannot enter the top four;
                    // ties therefore remain ordered exactly as before.
                    if prune && h.guard >= best[3].guard {
                        continue 'hypothesis;
                    }
                }
                if let Some(i) = best.iter().position(|b| h.guard < b.guard) {
                    for j in (i + 1..4).rev() {
                        best[j] = best[j - 1];
                    }
                    best[i] = h;
                }
            }
        }
        #[allow(unused_mut)]
        for mut h in best {
            if !h.guard.is_finite() {
                continue;
            }
            // Reproduce ean::decode's operation order exactly, independently of
            // the ranking score (which divides each term before summation).

            let (legacy_allowed, blur_allowed) = {
                let mut guard = 0.;
                for (i, b) in [
                    (0, 1.),
                    (1, 0.),
                    (2, 1.),
                    (45, 0.),
                    (46, 1.),
                    (47, 0.),
                    (48, 1.),
                    (49, 0.),
                    (92, 1.),
                    (93, 0.),
                    (94, 1.),
                ] {
                    let v = module_sample(p, h.left, h.right, i, reverse);
                    guard += (v - b) * (v - b);
                }
                let center = |i: usize| {
                    sample(
                        p,
                        h.left + (crate::numeric::usize_f32(i) + 0.5) * (h.right - h.left) / 95.,
                        reverse,
                    )
                };
                let observed = [0, 1, 46, 47, 48, 93, 94].map(center);
                (
                    !precheck || guard / 11. <= 0.1,
                    ean::blurred_guard_possible(&observed),
                )
            };

            if !legacy_allowed && !blur_allowed {
                continue;
            }
            // Ranking depends only on the eleven observed guards. Materialize
            // all modules only after the exact same four windows are selected.

            for (i, v) in h.modules.iter_mut().enumerate() {
                *v = module_sample(p, h.left, h.right, i, reverse);
            }

            let result = {
                let legacy = if legacy_allowed {
                    ean::decode(&h.modules, 0.1, 0.02)
                } else {
                    None
                };
                let blurred = if blur_allowed {
                    trace.gated_windows += 1;
                    trace.model_attempts += 3;
                    let modules: [f32; 95] = std::array::from_fn(|i| {
                        sample(
                            p,
                            h.left
                                + (crate::numeric::usize_f32(i) + 0.5) * (h.right - h.left) / 95.,
                            reverse,
                        )
                    });
                    let read = ean::decode_blurred(&modules).map(|r| r.read);
                    trace.accepted_windows += usize::from(read.is_some());
                    read
                } else {
                    None
                };
                // Neither the model nor checksum can resolve competing texts.
                let Some(result) = combine_blur_read(legacy, blurred) else {
                    trace.conflicts += 1;
                    trace.rejected_intervals.push(if reverse {
                        (
                            crate::numeric::usize_f32(p.len() - 1) - h.right,
                            crate::numeric::usize_f32(p.len() - 1) - h.left,
                        )
                    } else {
                        (h.left, h.right)
                    });
                    return Ok(None);
                };
                result
            };
            if let Some(r) = result {
                if accepted.is_some_and(|a| a.digits != r.digits) {
                    return Ok(None);
                }
                if accepted.is_none_or(|a| r.cost < a.cost) {
                    accepted = Some(Read {
                        digits: r.digits,
                        cost: r.cost,
                        gap: r.gap,
                        left: h.left,
                        right: h.right,
                        reversed: reverse,
                    });
                }
            }
        }
    }
    Ok(accepted)
}

#[expect(
    clippy::option_option,
    reason = "Outer None vetoes conflicting decoded identities; Some(None) is an ordinary undecoded window and must continue the search."
)]
fn combine_blur_read(
    legacy: Option<ean::Result>,
    blurred: Option<ean::Result>,
) -> Option<Option<ean::Result>> {
    if legacy
        .zip(blurred)
        .is_some_and(|(a, b)| a.digits != b.digits)
    {
        return None;
    }
    Some(match (legacy, blurred) {
        (Some(a), Some(b)) => Some(if a.cost <= b.cost { a } else { b }),
        (a, b) => a.or(b),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_and_blank_profiles_are_rejected() {
        assert_eq!(decode(&[0.; 511]).unwrap_err(), Error::Length);
        for v in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert_eq!(decode(&[v; LEN]).unwrap_err(), Error::Value);
        }
        for v in [0., 0.5, 1.] {
            assert!(decode(&[v; LEN]).unwrap().is_none());
        }
    }
    #[test]
    fn independent_runs_decode_both_directions_and_damaged_checksum_rejects() {
        // Independent EAN-13 run string for 5901234123457; no ean::encode use.
        let bits="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        assert_eq!(bits.len(), 95);
        let mut p = [0.; LEN];
        for (i, v) in p.iter_mut().enumerate() {
            let m = (crate::numeric::usize_f32(i) - 64.) / 4.;
            if (0.0..95.0).contains(&m) {
                *v = f32::from(bits.as_bytes()[crate::numeric::f32_usize(m)] - b'0');
            }
        }
        let got = decode(&p).unwrap().unwrap();
        assert_eq!(got.digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        p.reverse();
        assert_eq!(decode(&p).unwrap().unwrap().digits, got.digits);
        p.reverse(); // Replace last digit 7 (1000100) with 8 (1001000).
        for i in 0..7 {
            for j in 0..4 {
                p[64 + (85 + i) * 4 + j] = f32::from(b"1001000"[i] - b'0');
            }
        }
        assert!(decode(&p).unwrap().is_none());
    }
}

#[cfg(test)]
mod precheck_tests {
    use super::*;
    #[test]
    fn exact_guard_precheck_preserves_results_on_varied_profiles() {
        let mut seed = 7u32;
        for mode in 0..256 {
            let mut p = [0.; 512];
            for (i, v) in p.iter_mut().enumerate() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *v = match mode % 4 {
                    0 => crate::numeric::f64_f32(f64::from(seed >> 24)) / 255.,
                    1 => {
                        if (i / (mode % 13 + 1)) % 2 == 0 {
                            0.
                        } else {
                            1.
                        }
                    }
                    2 => {
                        if i % 7 < 3 {
                            0.49
                        } else {
                            0.51
                        }
                    }
                    _ => 0.5,
                };
            }
            assert_eq!(
                format!("{:?}", decode_checked(&p, false)),
                format!("{:?}", decode_checked(&p, true))
            );
        }
    }

    #[test]
    fn guard_rank_prune_preserves_reference_results_on_signal_families() {
        let bits="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        let mut seed = 0x9e37_79b9_u32;
        for mode in 0..320 {
            let mut p = [0.; LEN];
            for i in 0..LEN {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (crate::numeric::f64_f32(f64::from(seed >> 24)) / 255. - 0.5) * 0.12;
                let x = (crate::numeric::usize_f32(i) - 64.) / 4.;
                let base = if (0.0..95.0).contains(&x) {
                    f32::from(bits.as_bytes()[crate::numeric::f32_usize(x)] - b'0')
                } else {
                    0.
                };
                p[i] = match mode % 8 {
                    0 => crate::numeric::f64_f32(f64::from(seed >> 24)) / 255.,
                    1 => {
                        if (i / (mode % 17 + 1)) % 2 == 0 {
                            0.
                        } else {
                            1.
                        }
                    }
                    2 => 0.49 + noise,
                    3 => 0.5 + noise,
                    4 => base,
                    5 => (base * 0.55 + 0.2 + noise).clamp(0., 1.),
                    6 => ((base * 0.8 + 0.1) + noise).clamp(0., 1.),
                    _ => {
                        let a = if i > 0 { p[i - 1] } else { base };
                        (0.65 * base + 0.35 * a).clamp(0., 1.)
                    }
                };
            }
            let reference = decode_checked_with_prune(&p, false, false);
            let pruned = decode_checked_with_prune(&p, false, true);
            assert_eq!(
                format!("{reference:?}"),
                format!("{:?}", pruned),
                "mode {mode}"
            );
        }
    }
}

#[cfg(test)]
mod forward_blur_tests {
    use super::*;
    fn physical_profile(d: &[u8; 13], sigma: f64) -> [f32; LEN] {
        let bits = ean::encode(d);
        let left = 64.;
        let pitch = 4.;
        std::array::from_fn(|i| {
            let (mut total, mut weight) = (0., 0.);
            for n in -640..=640 {
                let dx = f64::from(n) / 32.;
                let w = (-dx * dx / (2. * sigma * sigma * pitch * pitch)).exp();
                let m = crate::numeric::f64_i32(
                    ((crate::numeric::usize_f64(i) + dx - left) / pitch).floor(),
                );
                if (0..95).contains(&m) {
                    total += w * f64::from(bits[crate::numeric::i32_usize(m)]);
                }
                weight += w;
            }
            crate::numeric::f64_f32(total / weight)
        })
    }
    #[test]
    fn existing_boundary_search_reads_blurred_profile_both_directions() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        for sigma in [0.45, 0.65, 0.85] {
            let mut p = physical_profile(&digits, sigma);
            for _ in 0..2 {
                if let Some(read) = decode(&p).unwrap() {
                    assert_eq!(read.digits, digits);
                }
                p.reverse();
            }
        }
        // The finite boundary search must recover at least the moderately blurred
        // physical fixture; strong blur can erase its threshold-based boundary cues.
        assert_eq!(
            decode(&physical_profile(&digits, 0.65))
                .unwrap()
                .unwrap()
                .digits,
            digits
        );
        let mut invalid = digits;
        invalid[12] = 8;
        for sigma in [0.45, 0.65, 0.85] {
            assert!(decode(&physical_profile(&invalid, sigma))
                .unwrap()
                .is_none());
        }
    }
    #[test]
    fn legacy_blur_conflict_is_a_veto_not_a_cost_vote() {
        let a = ean::Result {
            digits: [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7],
            cost: 0.05,
            gap: 0.03,
            guard: 0.01,
        };
        let b = ean::Result {
            digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
            cost: 0.001,
            gap: 0.03,
            guard: 0.001,
        };
        assert!(combine_blur_read(Some(a), Some(b)).is_none());
        assert_eq!(
            combine_blur_read(Some(a), None).unwrap().unwrap().digits,
            a.digits
        );
        assert_eq!(
            combine_blur_read(None, Some(b)).unwrap().unwrap().digits,
            b.digits
        );
    }
}

#[cfg(all(test, any(feature = "mode-high", feature = "mode-very-high")))]
mod native_soft_tests {
    use super::*;
    #[test]
    fn native_lengths_keep_value_orientation_and_checksum_rejection() {
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        for n in [119usize, 238, 357] {
            for invalid in [false, true] {
                let mut digits = good;
                if invalid {
                    digits[12] = 8;
                }
                let bits = ean::encode(&digits);
                let pitch = crate::numeric::usize_f32(n) / 119.;
                let p: Vec<_> = (0..n)
                    .map(|i| {
                        let x = (crate::numeric::usize_f32(i) + 0.5) / pitch - 12.;
                        if (0.0..95.).contains(&x) {
                            bits[crate::numeric::f32_usize(x.floor())]
                        } else {
                            0.
                        }
                    })
                    .collect();
                for reverse in [false, true] {
                    let mut q = p.clone();
                    if reverse {
                        q.reverse();
                    }
                    let (read, _) = decode_native_with_blur_trace(&q);
                    let read = read.unwrap();
                    if invalid {
                        assert!(read.is_none());
                    } else {
                        assert_eq!(read.unwrap().digits, good);
                    }
                    assert!(matches!(decode(&q), Err(Error::Length)));
                }
            }
        }
    }
    #[test]
    fn native_soft_rejects_invalid_shapes_and_constant_profiles() {
        for p in [vec![0.; 76], vec![1.; 384]] {
            assert!(decode_native_with_blur_trace(&p).0.unwrap().is_none());
        }
        assert!(matches!(
            decode_native_with_blur_trace(&[0.; 75]).0,
            Err(Error::Length)
        ));
        assert!(matches!(
            decode_native_with_blur_trace(&[f32::NAN; 119]).0,
            Err(Error::Value)
        ));
    }
}
