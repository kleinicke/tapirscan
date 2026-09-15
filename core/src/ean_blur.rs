//! Diagnostic finite forward model: blurred rectangular modules, no run count.
//! Fixed Gaussian cell integrals at sigma .45/.65/.85 module units, radius two.
//! Input is normalized module-center darkness; gain, bias and phase are not fit.
use super::*;
pub const BLUR_SIGMAS: [f32; 3] = [0.45, 0.65, 0.85];
const KERNELS: [[f32; 3]; 3] = [
    [0.733_479_5, 0.132_831_2, 0.000_429_046_5],
    [0.558_310_7, 0.210_395_28, 0.010_449_389],
    [0.445_080_9, 0.240_165_84, 0.037_293_702],
];
const GUARDS: [usize; 7] = [0, 1, 46, 47, 48, 93, 94];
const MAX_GUARD: f32 = 0.008;
const MAX_GUARD_RESIDUAL: f32 = 0.04;
const MAX_FIT: f32 = 0.01;
const MIN_DIGIT_GAP: f32 = 0.001;
const MIN_PARITY_GAP: f32 = 0.0002;
const MIN_MODEL_GAP: f32 = 0.0002;
#[derive(Clone, Copy, Debug)]
pub struct BlurredResult {
    pub read: Result,
    pub sigma: f32,
    pub model_gap: f32,
    pub parity_gap: f32,
}
#[derive(Clone, Copy)]
struct Visual {
    read: Result,
    score: f32,
    parity_gap: f32,
    model: usize,
}
fn guard_target(model: usize, position: usize) -> f32 {
    let [w0, w1, w2] = KERNELS[model];
    match position {
        0 | 94 => w0 + w2,
        1 | 47 | 93 => 2. * w1,
        46 | 48 => w0 + 2. * w2,
        _ => unreachable!(),
    }
}
fn guard_fit(values: &[f32; 7], model: usize) -> Option<f32> {
    let mut cost = 0.;
    for (j, &position) in GUARDS.iter().enumerate() {
        let d = values[j] - guard_target(model, position);
        if d * d > MAX_GUARD_RESIDUAL {
            return None;
        }
        cost += d * d;
    }
    let cost = cost / 7.;
    (cost <= MAX_GUARD).then_some(cost)
}
/// Cheap gate using ONLY fully known guard contexts. Missing payload-neighbor
/// bits at guards 2/45/49/92 never enter a blur prediction.
pub fn blurred_guard_possible(values: &[f32; 7]) -> bool {
    values
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        && (0..3).any(|m| guard_fit(values, m).is_some())
}
const fn pattern(model: usize, side: u8, d: usize, position: usize) -> f32 {
    let k = KERNELS[model];
    let mut sum = 0.;
    let mut offset = -2i32;
    while offset <= 2 {
        let i = position as i32 + offset;
        // Every L/G digit starts0/ends1; R starts1/ends0. At the
        // adjacent guard boundaries the same outside bits are known exactly.
        let v = if i < 0 {
            if side == b'R' {
                0.
            } else {
                1.
            }
        } else if i > 6 {
            if side == b'R' {
                1.
            } else {
                0.
            }
        } else {
            bit(d, side, i as usize)
        };
        sum += v * k[offset.unsigned_abs() as usize];
        offset += 1;
    }
    sum
}
const fn templates() -> [[[[f32; 5]; 10]; 3]; 3] {
    let mut result = [[[[0.; 5]; 10]; 3]; 3];
    let sides = [b'L', b'G', b'R'];
    let mut m = 0;
    while m < 3 {
        let mut s = 0;
        while s < 3 {
            let mut d = 0;
            while d < 10 {
                let mut i = 0;
                while i < 5 {
                    result[m][s][d][i] = pattern(m, sides[s], d, i + 1);
                    i += 1;
                }
                d += 1;
            }
            s += 1;
        }
        m += 1;
    }
    result
}
const TEMPLATES: [[[[f32; 5]; 10]; 3]; 3] = templates();
fn blurred_digit(p: &[f32], model: usize, side: usize) -> Digit {
    let (mut best, mut second) = (
        Digit {
            value: 0,
            cost: f32::INFINITY,
            gap: 0.,
        },
        f32::INFINITY,
    );
    for d in 0..10 {
        let mut cost = 0.;
        for i in 0..5 {
            let delta = p[i + 1] - TEMPLATES[model][side][d][i];
            cost += delta * delta;
        }
        cost /= 5.;
        if cost < best.cost {
            second = best.cost;
            best.value = d as u8;
            best.cost = cost;
        } else if cost < second {
            second = cost;
        }
    }
    best.gap = second - best.cost;
    best
}
fn visual(p: &[f32], model: usize, guard: f32) -> Visual {
    let empty = Digit {
        value: 0,
        cost: 0.,
        gap: 0.,
    };
    let mut left = [[empty; 2]; 6];
    let mut right = [empty; 6];
    for j in 0..6 {
        left[j] = [
            blurred_digit(&p[3 + j * 7..10 + j * 7], model, 0),
            blurred_digit(&p[3 + j * 7..10 + j * 7], model, 1),
        ];
        right[j] = blurred_digit(&p[50 + j * 7..57 + j * 7], model, 2);
    }
    let mut best = Result {
        digits: [0; 13],
        cost: f32::INFINITY,
        gap: 0.,
        guard,
    };
    let mut second = f32::INFINITY;
    for first in 0..10 {
        let mut digits = [0; 13];
        digits[0] = first as u8;
        let (mut cost, mut gap) = (0., f32::INFINITY);
        for j in 0..12 {
            let d = if j < 6 {
                left[j][usize::from(PARITY[first][j] == b'G')]
            } else {
                right[j - 6]
            };
            digits[j + 1] = d.value;
            cost += d.cost;
            gap = gap.min(d.gap);
        }
        cost /= 12.;
        if cost < best.cost {
            second = best.cost;
            best = Result {
                digits,
                cost,
                gap,
                guard,
            };
        } else if cost < second {
            second = cost;
        }
    }
    Visual {
        read: best,
        score: (best.cost * 60. + guard * 7.) / 67.,
        parity_gap: second - best.cost,
        model,
    }
}
fn plausible(v: &Visual) -> bool {
    v.read.cost <= MAX_FIT && v.read.gap >= MIN_DIGIT_GAP && v.parity_gap >= MIN_PARITY_GAP
}
/// Choose visual parity and blur jointly before a single checksum rejection.
/// No checksum-driven digit, parity or blur search is permitted.
pub fn decode_blurred(p: &[f32]) -> Option<BlurredResult> {
    if p.len() != 95 || p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return None;
    }
    let guards = GUARDS.map(|i| p[i]);
    let mut models = [None; 3];
    for m in 0..3 {
        if let Some(guard) = guard_fit(&guards, m) {
            models[m] = Some(visual(p, m, guard));
        }
    }
    select_models(&models)
}
fn select_models(models: &[Option<Visual>; 3]) -> Option<BlurredResult> {
    let mut best: Option<Visual> = None;
    let mut second = f32::INFINITY;
    for v in models.iter().flatten() {
        if best.is_none_or(|b| v.score < b.score) {
            second = best.map_or(f32::INFINITY, |b| b.score);
            best = Some(*v);
        } else {
            second = second.min(v.score);
        }
    }
    let best = best?;
    if !plausible(&best) || second - best.score < MIN_MODEL_GAP {
        return None;
    }
    // Another visually plausible payload is a conflict even if its checksum is
    // invalid. Checksum may not act as a selector among blur models.
    if models
        .iter()
        .flatten()
        .any(|v| plausible(v) && v.read.digits != best.read.digits)
    {
        return None;
    }
    if !checksum(&best.read.digits) {
        return None;
    }
    Some(BlurredResult {
        read: best.read,
        sigma: BLUR_SIGMAS[best.model],
        model_gap: second - best.score,
        parity_gap: best.parity_gap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid(first: u8) -> [u8; 13] {
        let mut d = [0; 13];
        d[0] = first;
        for i in 1..12 {
            d[i] = ((i * 7) % 10) as u8;
        }
        let sum = d[..12]
            .iter()
            .enumerate()
            .map(|(i, &v)| v as usize * if i % 2 == 0 { 1 } else { 3 })
            .sum::<usize>();
        d[12] = ((10 - sum % 10) % 10) as u8;
        d
    }
    // Independent oversampled Gaussian integration of ideal rectangular modules.
    // Full symbol+white quiet zones; no digit-template helper is used here.
    pub(super) fn physical(d: &[u8; 13], sigma: f64) -> [f32; 95] {
        let bits = encode(d);
        std::array::from_fn(|i| {
            let (mut sum, mut norm) = (0., 0.);
            for n in -800..=800 {
                let dx = f64::from(n) / 160.;
                let weight = (-dx * dx / (2. * sigma * sigma)).exp();
                let module = (i as f64 + 0.5 + dx).floor() as i32;
                if (0..95).contains(&module) {
                    sum += weight * f64::from(bits[module as usize]);
                }
                norm += weight;
            }
            (sum / norm) as f32
        })
    }
    #[test]
    fn physically_blurred_valid_and_invalid_checksums() {
        let mut accepted = [0; 3];
        for first in 0..10 {
            for (m, sigma) in [0.45, 0.65, 0.85].iter().enumerate() {
                let d = valid(first);
                let p = physical(&d, *sigma);
                if let Some(r) = decode_blurred(&p) {
                    assert_eq!(r.read.digits, d);
                    accepted[m] += 1;
                }
                let mut bad = d;
                bad[12] = (bad[12] + 1) % 10;
                assert!(decode_blurred(&physical(&bad, *sigma)).is_none());
            }
        }
        assert_eq!(
            accepted,
            [10, 10, 10],
            "physically integrated fixture recoveries"
        );
    }
    #[test]
    fn diagnostic_rejects_noise_flat_invalid_and_missing_known_guards() {
        assert!(decode_blurred(&[0.; 94]).is_none());
        for v in [0., 0.5, 1., f32::NAN] {
            assert!(decode_blurred(&[v; 95]).is_none());
        }
        let mut seed = 17u32;
        for _ in 0..256 {
            let mut p = [0.; 95];
            for v in &mut p {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *v = (seed >> 24) as f32 / 255.;
            }
            assert!(decode_blurred(&p).is_none());
        }
        let d = valid(5);
        for sigma in [0.45, 0.65, 0.85] {
            let p = physical(&d, sigma);
            for region in [[0, 1], [46, 47], [93, 94]] {
                let mut bad = p;
                for i in region {
                    bad[i] = 0.;
                }
                assert!(decode_blurred(&bad).is_none());
            }
        }
    }
    #[test]
    fn fixed_outer_context_matches_full_symbol_convolution() {
        for first in 0..10 {
            let d = valid(first);
            let p = encode(&d);
            for m in 0..3 {
                for j in 0..12 {
                    let start = if j < 6 { 3 + j * 7 } else { 50 + (j - 6) * 7 };
                    let side = if j < 6 {
                        PARITY[first as usize][j]
                    } else {
                        b'R'
                    };
                    for i in 1..6 {
                        let full = (-2i32..=2)
                            .map(|o| {
                                p[((start + i) as isize + o as isize) as usize]
                                    * KERNELS[m][o.unsigned_abs() as usize]
                            })
                            .sum::<f32>();
                        assert!((full - pattern(m, side, d[j + 1] as usize, i)).abs() < 1e-6);
                    }
                }
            }
        }
    }
    #[test]
    fn visual_model_selection_never_uses_checksum_or_accepts_ties() {
        let d = valid(5);
        let mut bad = d;
        bad[12] = (bad[12] + 1) % 10;
        let v = |digits, score, model| Visual {
            read: Result {
                digits,
                cost: score,
                gap: 0.03,
                guard: 0.001,
            },
            score,
            parity_gap: 0.01,
            model,
        };
        assert!(select_models(&[Some(v(bad, 0.001, 0)), Some(v(d, 0.004, 1)), None]).is_none());
        assert!(select_models(&[Some(v(d, 0.001, 0)), Some(v(d, 0.0011, 1)), None]).is_none());
        let other = valid(4);
        assert!(select_models(&[Some(v(d, 0.001, 0)), Some(v(other, 0.004, 1)), None]).is_none());
        assert_eq!(
            select_models(&[Some(v(d, 0.001, 0)), None, None])
                .unwrap()
                .read
                .digits,
            d
        );
    }
}
