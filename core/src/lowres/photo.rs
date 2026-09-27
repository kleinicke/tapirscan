//! Bounded multirow decoder: original pixels, bilinear quad coordinates, profile fusion.
//! Geometry ranking never uses checksum or expected text.
use crate::{
    ean,
    numeric::{f64_f32, f64_usize, usize_f64},
    sampling::ImageView,
};
use std::collections::BTreeMap;
const BINS: usize = 1024;
const SCALE: f64 = 8.;
const MARGIN: f64 = 16.;

fn point(q: [[f64; 2]; 4], u: f64, v: f64) -> [f64; 2] {
    std::array::from_fn(|i| {
        (1. - v) * (q[0][i] + u * (q[1][i] - q[0][i])) + v * (q[3][i] + u * (q[2][i] - q[3][i]))
    })
}

/// Guard and quiet-zone screening only; no payload or checksum is used here.
pub fn regions(image: ImageView<'_>, q: [[f64; 2]; 4], angles: bool) -> Vec<[[f64; 2]; 4]> {
    let center = [
        q.iter().map(|p| p[0]).sum::<f64>() / 4.,
        q.iter().map(|p| p[1]).sum::<f64>() / 4.,
    ];
    let mut ranked = Vec::new();
    for &angle in if angles {
        &[-4f64, 0., 4.][..]
    } else {
        &[0.][..]
    } {
        let (sine, cosine) = angle.to_radians().sin_cos();
        let q = q.map(|p| {
            [
                center[0] + cosine * (p[0] - center[0]) - sine * (p[1] - center[1]),
                center[1] + sine * (p[0] - center[0]) + cosine * (p[1] - center[1]),
            ]
        });
        for fraction in [1., 0.6] {
            let lo = (1. - fraction) * 0.5;
            let q = [
                point(q, 0., lo),
                point(q, 1., lo),
                point(q, 1., 1. - lo),
                point(q, 0., 1. - lo),
            ];
            let px = pixels(image, q);
            if px.len() < 120 {
                continue;
            }
            let p = profile(&px, 0., 0, false);
            // Coherent transitions must survive averaging across height. Random
            // pixel noise, flat regions and strokes with no paper contrast fail.
            let transitions = (1..190)
                .filter(|&j| {
                    (sample(&p, f64::from(j) * 0.5) > 0.5)
                        != (sample(&p, f64::from(j - 1) * 0.5) > 0.5)
                })
                .count();
            if transitions < 24 {
                continue;
            }
            for span in 50..=104 {
                for start in -10..=(210 - 2 * span) {
                    let left = f64::from(start) * 0.5;
                    let right = left + f64::from(span) - 95.;
                    let h = Hypothesis {
                        result: ean::Result {
                            digits: [0; 13],
                            cost: 1.,
                            gap: 0.,
                            guard: 1.,
                        },
                        score: 1.,
                        left,
                        right,
                        shear: 0.,
                        bend: 0.,
                        sharpen: 0.,
                        reverse: false,
                        local: false,
                        quiet: 1.,
                    };
                    let guard = guard_score(&p, h);
                    if guard > 0.13 {
                        continue;
                    }
                    let quiet = quiet_score(&p, h);
                    if quiet > 0.22 {
                        continue;
                    }
                    ranked.push((
                        guard + 0.15 * quiet,
                        [
                            point(q, left / 95., 0.),
                            point(q, (95. + right) / 95., 0.),
                            point(q, (95. + right) / 95., 1.),
                            point(q, left / 95., 1.),
                        ],
                    ));
                }
            }
        }
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut output: Vec<[[f64; 2]; 4]> = Vec::new();
    for (_, q) in ranked {
        if output.iter().any(|old| {
            old.iter()
                .zip(q)
                .map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
                .sum::<f64>()
                < 8.
        }) {
            continue;
        }
        output.push(q);
        if output.len() == 3 {
            break;
        }
    }
    output
}

#[derive(Clone, Copy)]
struct Pixel {
    u: f64,
    v: f64,
    value: f64,
    split: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Hypothesis {
    pub result: ean::Result,
    pub score: f32,
    pub left: f64,
    pub right: f64,
    pub shear: f64,
    pub bend: f64,
    pub sharpen: f64,
    pub reverse: bool,
    pub local: bool,
    pub quiet: f64,
}

#[derive(Debug)]
pub struct Output {
    pub hypotheses: Vec<Hypothesis>,
    pub split_matches: usize,
    pub split_cost: f32,
    pub split_gap: f32,
    pub digit_support_min: usize,
    pub contradictions: usize,
    pub trials: usize,
    pub pixels: usize,
}

#[expect(
    clippy::many_single_char_names,
    reason = "Names follow the bilinear and projective coordinate equations."
)]
fn pixels(image: ImageView<'_>, q: [[f64; 2]; 4]) -> Vec<Pixel> {
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return Vec::new();
    }
    let a = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let b = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let d = [q[2][0] - q[1][0] - b[0], q[2][1] - q[1][1] - b[1]];
    let determinant = a[0] * b[1] - a[1] * b[0];
    if determinant.abs() < 1. {
        return Vec::new();
    }
    let margin = a[0].hypot(a[1]) * 0.18;
    let x0 = f64_usize((q.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min) - margin).max(0.))
        .min(image.width);
    let y0 = f64_usize((q.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min) - margin).max(0.))
        .min(image.height);
    let x1 = f64_usize((q.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) + margin).ceil())
        .min(image.width);
    let y1 = f64_usize((q.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max) + margin).ceil())
        .min(image.height);
    if x1 <= x0 || y1 <= y0 || (x1 - x0).saturating_mul(y1 - y0) > 200_000 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = [usize_f64(x) + 0.5 - q[0][0], usize_f64(y) + 0.5 - q[0][1]];
            let mut u = (p[0] * b[1] - p[1] * b[0]) / determinant;
            let mut v = (a[0] * p[1] - a[1] * p[0]) / determinant;
            for _ in 0..3 {
                let e = [
                    a[0] * u + b[0] * v + d[0] * u * v - p[0],
                    a[1] * u + b[1] * v + d[1] * u * v - p[1],
                ];
                let du = [a[0] + d[0] * v, a[1] + d[1] * v];
                let dv = [b[0] + d[0] * u, b[1] + d[1] * u];
                let det = du[0] * dv[1] - du[1] * dv[0];
                if det.abs() < 1e-8 {
                    break;
                }
                u -= (e[0] * dv[1] - e[1] * dv[0]) / det;
                v -= (du[0] * e[1] - du[1] * e[0]) / det;
            }
            if (-0.17..1.17).contains(&u) && (0.08..0.92).contains(&v) {
                out.push(Pixel {
                    u: u * 95.,
                    v,
                    value: image.gray(usize_f64(x), usize_f64(y)) / 255.,
                    split: (x / 3 + y / 3) % 2,
                });
            }
        }
    }
    out
}

fn profile(pixels: &[Pixel], shear: f64, split: usize, local: bool) -> Vec<f64> {
    let mut sums = vec![0.; BINS];
    let mut weights = vec![0.; BINS];
    for p in pixels {
        if split > 0 && p.split != split - 1 {
            continue;
        }
        let at = (p.u + shear * (p.v - 0.5) + MARGIN) * SCALE;
        if !(0. ..usize_f64(BINS - 1)).contains(&at) {
            continue;
        }
        let j = f64_usize(at.floor());
        let t = at - usize_f64(j);
        sums[j] += (1. - t) * p.value;
        weights[j] += 1. - t;
        sums[j + 1] += t * p.value;
        weights[j + 1] += t;
    }
    // Smooth only enough to combine nearby samples; fill empty bins by interpolation.
    let mut values = vec![f64::NAN; BINS];
    for (j, value) in values.iter_mut().enumerate() {
        let mut s = 0.;
        let mut w = 0.;
        for k in j.saturating_sub(2)..(j + 3).min(BINS) {
            let a = match j.abs_diff(k) {
                0 => 1.,
                1 => 0.55,
                _ => 0.1,
            };
            s += a * sums[k];
            w += a * weights[k];
        }
        if w > 0.05 {
            *value = s / w;
        }
    }
    let mut last = None;
    for j in 0..BINS {
        if values[j].is_finite() {
            if let Some(a) = last {
                for k in a + 1..j {
                    values[k] =
                        values[a] + (values[j] - values[a]) * usize_f64(k - a) / usize_f64(j - a);
                }
            } else {
                for k in 0..j {
                    values[k] = values[j];
                }
            }
            last = Some(j);
        }
    }
    if let Some(a) = last {
        for j in a + 1..BINS {
            values[j] = values[a];
        }
    } else {
        return vec![0.5; BINS];
    }
    let bounds = if local {
        local_bounds(&values)
    } else {
        let mut global = values.clone();
        global.sort_by(f64::total_cmp);
        vec![(global[BINS / 20], global[BINS * 19 / 20]); BINS]
    };
    values
        .iter()
        .zip(bounds)
        .map(|(&value, (lo, hi))| ((hi - value) / (hi - lo).max(0.10)).clamp(0., 1.))
        .collect()
}

// Preserve exact percentile values while updating one sorted window per bin.
fn local_bounds(values: &[f64]) -> Vec<(f64, f64)> {
    let mut window = values[..65.min(values.len())].to_vec();
    window.sort_by(f64::total_cmp);
    let mut bounds = Vec::with_capacity(values.len());
    for j in 0..values.len() {
        bounds.push((window[window.len() / 10], window[window.len() * 9 / 10]));
        if j >= 64 {
            let index = window
                .binary_search_by(|v| v.total_cmp(&values[j - 64]))
                .expect("outgoing value belongs to window");
            window.remove(index);
        }
        if let Some(&next) = values.get(j + 65) {
            let index = window
                .binary_search_by(|v| v.total_cmp(&next))
                .unwrap_or_else(|i| i);
            window.insert(index, next);
        }
    }
    bounds
}

fn sample(p: &[f64], x: f64) -> f64 {
    let x = ((x + MARGIN) * SCALE).clamp(0., usize_f64(BINS - 2));
    let j = f64_usize(x.floor());
    let t = x - usize_f64(j);
    p[j] * (1. - t) + p[j + 1] * t
}

fn module_samples(p: &[f64], h: Hypothesis) -> [f32; 95] {
    let mut bits = [0f32; 95];
    for (j, b) in bits.iter_mut().enumerate() {
        let t = (usize_f64(j) + 0.5) / 95.;
        let u = h.left + t * (95. + h.right - h.left) + h.bend * t * (1. - t);
        let center = sample(p, u);
        let average = (sample(p, u - 0.55) + sample(p, u + 0.55)) * 0.5;
        *b = f64_f32((center + h.sharpen * (center - average)).clamp(0., 1.));
    }
    if h.reverse {
        bits.reverse();
    }
    bits
}
fn read(p: &[f64], h: Hypothesis) -> Option<ean::Result> {
    ean::visual_fit(&module_samples(p, h), 0.3, 0.)
}

fn location(h: Hypothesis, j: f64) -> f64 {
    let t = (j + 0.5) / 95.;
    h.left + t * (95. + h.right - h.left) + h.bend * t * (1. - t)
}
fn guard_score(p: &[f64], h: Hypothesis) -> f64 {
    let mut error = 0.;
    for (j, b) in [
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
        let z = sample(p, location(h, f64::from(j))) - b;
        error += z * z;
    }
    error / 11.
}
fn quiet_score(p: &[f64], h: Hypothesis) -> f64 {
    let score = |left: bool| {
        let mut palette: Vec<_> = (0..13)
            .map(|j| {
                sample(
                    p,
                    location(
                        h,
                        if left {
                            f64::from(j)
                        } else {
                            94. - f64::from(j)
                        },
                    ),
                )
            })
            .collect();
        palette.sort_by(f64::total_cmp);
        let white = palette[2];
        let black = palette[10];
        let contrast = black - white;
        if contrast < 0.10 {
            return 1.;
        }
        let quiet = (1..=5)
            .map(|i| {
                sample(
                    p,
                    if left {
                        h.left - f64::from(i) - 1.
                    } else {
                        95. + h.right + f64::from(i) + 1.
                    },
                )
            })
            .sum::<f64>()
            / 5.;
        ((quiet - white) / contrast).clamp(0., 1.)
    };
    score(true).max(score(false))
}

fn add_fit(p: &[f64], h: Hypothesis, best: &mut BTreeMap<[u8; 13], Hypothesis>) {
    if let Some(r) = read(p, h) {
        let score = (r.cost * 84. + r.guard * 11.) / 95.;
        let h = Hypothesis {
            result: r,
            score,
            quiet: quiet_score(p, h),
            ..h
        };
        if best.get(&r.digits).is_none_or(|b| score < b.score) {
            best.insert(r.digits, h);
        }
    }
}

fn parameter_grid(wide: bool) -> (Vec<f64>, Vec<f64>) {
    let limit = if wide { 12 } else { 6 };
    let shifts = (-limit..=limit).map(|v| f64::from(v) * 0.5).collect();
    let limit = if wide { 20 } else { 4 };
    let bends = (-limit..=limit).map(|v| f64::from(v) * 3.).collect();
    (shifts, bends)
}
fn search_profiles(
    profiles: &[(f64, bool, Vec<f64>)],
    wide: bool,
    trials: &mut usize,
    best: &mut BTreeMap<[u8; 13], Hypothesis>,
) {
    for (shear, local, p) in profiles {
        let (shifts, bends) = parameter_grid(wide);
        let mut beam = Vec::new();
        for &left in &shifts {
            for &right in &shifts {
                for &bend in &bends {
                    let h = Hypothesis {
                        result: ean::Result {
                            digits: [0; 13],
                            cost: 1.,
                            gap: 0.,
                            guard: 1.,
                        },
                        score: 1.,
                        left,
                        right,
                        shear: *shear,
                        bend,
                        sharpen: 0.,
                        reverse: false,
                        local: *local,
                        quiet: 1.,
                    };
                    beam.push((guard_score(p, h), h));
                }
            }
        }
        let keep = if wide { 96 } else { 16 };
        if beam.len() > 512 {
            beam.select_nth_unstable_by(512, |a, b| a.0.total_cmp(&b.0));
            beam.truncate(512);
        }
        for (score, h) in &mut beam {
            *score += quiet_score(p, *h) * 0.1;
        }
        if beam.len() > keep {
            beam.select_nth_unstable_by(keep, |a, b| a.0.total_cmp(&b.0));
            beam.truncate(keep);
        }
        beam.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, h) in beam.iter().take(keep) {
            for sharpen in [0., 1., 2.] {
                for reverse in [false, true] {
                    *trials += 1;
                    add_fit(
                        p,
                        Hypothesis {
                            sharpen,
                            reverse,
                            ..h
                        },
                        best,
                    );
                }
            }
        }
    }
}

pub fn scan(image: ImageView<'_>, q: [[f64; 2]; 4], wide: bool) -> Output {
    let px = pixels(image, q);
    let mut out = Output {
        hypotheses: Vec::new(),
        split_matches: 0,
        split_cost: 1.,
        split_gap: 0.,
        digit_support_min: 0,
        contradictions: 0,
        trials: 0,
        pixels: px.len(),
    };
    if px.len() < 60 {
        return out;
    }
    let mut best = BTreeMap::<[u8; 13], Hypothesis>::new();
    let shears: &[f64] = if wide {
        &[-2., -1., 0., 1., 2.]
    } else {
        &[-1., 0., 1.]
    };
    let mut profiles = Vec::new();
    for &shear in shears {
        for local in [false, true] {
            profiles.push((shear, local, profile(&px, shear, 0, local)));
        }
    }
    search_profiles(&profiles, wide, &mut out.trials, &mut best);
    let mut seeds: Vec<_> = best.values().copied().collect();
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    for h in seeds.into_iter().take(if wide { 8 } else { 4 }) {
        let p = &profiles
            .iter()
            .find(|(s, l, _)| s.to_bits() == h.shear.to_bits() && *l == h.local)
            .unwrap()
            .2;
        for dl in [-0.25, 0., 0.25] {
            for dr in [-0.25, 0., 0.25] {
                for db in [-1.5, 0., 1.5] {
                    out.trials += 1;
                    add_fit(
                        p,
                        Hypothesis {
                            left: h.left + dl,
                            right: h.right + dr,
                            bend: h.bend + db,
                            ..h
                        },
                        &mut best,
                    );
                }
            }
        }
    }
    out.hypotheses = best.into_values().collect();
    out.hypotheses.sort_by(|a, b| a.score.total_cmp(&b.score));
    out.hypotheses.truncate(16);
    for h in &mut out.hypotheses {
        let quietp = &profiles
            .iter()
            .find(|(s, l, _)| s.to_bits() == h.shear.to_bits() && !*l)
            .unwrap()
            .2;
        h.quiet = quiet_score(quietp, *h);
    }
    corroborate(&px, &mut out);
    if !accepted(&out) {
        stabilize(&px, &mut out);
        corroborate(&px, &mut out);
    }
    out
}

// Keep the strongest visual payload fixed, and test whether a
// nearby geometry is more stable across disjoint pixel subsets. Never repairs
// checksum or promotes another text.
fn stabilize(px: &[Pixel], out: &mut Output) {
    let Some(original) = out.hypotheses.first().copied() else {
        return;
    };
    let full = profile(px, original.shear, 0, original.local);
    let mut fragments = vec![
        profile(px, original.shear, 1, original.local),
        profile(px, original.shear, 2, original.local),
    ];
    for band in 0..4 {
        let low = 0.08 + f64::from(band) * 0.21;
        let subset: Vec<_> = px
            .iter()
            .copied()
            .filter(|p| p.v >= low && p.v < low + 0.21)
            .collect();
        fragments.push(profile(&subset, original.shear, 0, original.local));
    }
    let target = ean::encode(&original.result.digits);
    let objective = |h| {
        fragments
            .iter()
            .enumerate()
            .map(|(i, p)| {
                module_samples(p, h)
                    .iter()
                    .zip(target)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>()
                    / 95.
                    * if i < 2 { 0.375 } else { 0.0625 }
            })
            .sum::<f32>()
    };
    let mut best = original;
    let mut error = objective(original);
    for dl in [-0.25, 0., 0.25] {
        for dr in [-0.25, 0., 0.25] {
            for db in [-0.75, 0., 0.75] {
                for sharpen in [0., 1., 2.] {
                    let h = Hypothesis {
                        left: original.left + dl,
                        right: original.right + dr,
                        bend: original.bend + db,
                        sharpen,
                        ..original
                    };
                    out.trials += 1;
                    let Some(result) =
                        read(&full, h).filter(|r| r.digits == original.result.digits)
                    else {
                        continue;
                    };
                    let cost = objective(h);
                    if cost < error {
                        error = cost;
                        best = Hypothesis {
                            result,
                            score: (result.cost * 84. + result.guard * 11.) / 95.,
                            ..h
                        };
                    }
                }
            }
        }
    }
    let quiet = profile(px, best.shear, 0, false);
    best.quiet = quiet_score(&quiet, best);
    out.hypotheses[0] = best;
}

fn corroborate(px: &[Pixel], out: &mut Output) {
    out.split_matches = 0;
    out.split_cost = 1.;
    out.split_gap = 0.;
    out.digit_support_min = 0;
    out.contradictions = 0;
    if let Some(&h) = out.hypotheses.first() {
        let mut worst = 0f32;
        let mut gap = 1f32;
        for split in [1, 2] {
            let p = profile(px, h.shear, split, h.local);
            if let Some(r) = read(&p, h) {
                if r.digits == h.result.digits {
                    out.split_matches += 1;
                }
                worst = worst.max(r.cost);
                gap = gap.min(r.gap);
            } else {
                worst = 1.;
                gap = 0.;
            }
        }
        out.split_cost = worst;
        out.split_gap = gap;
        let mut supports = [0usize; 12];
        let mut contradictory = [[0usize; 10]; 12];
        for band in 0..4 {
            let low = 0.08 + f64::from(band) * 0.21;
            let high = low + 0.21;
            let subset: Vec<_> = px
                .iter()
                .copied()
                .filter(|p| p.v >= low && p.v < high)
                .collect();
            if subset.len() < 40 {
                continue;
            }
            let p = profile(&subset, h.shear, 0, h.local);
            let bits = module_samples(&p, h);
            for (j, (digit, cost, gap)) in ean::visual_support(&bits, &h.result.digits)
                .expect("finite module samples and decoded decimal digits")
                .into_iter()
                .enumerate()
            {
                if digit == h.result.digits[j + 1] && cost < 0.12 && gap > 0.035 {
                    supports[j] += 1;
                }
                if digit != h.result.digits[j + 1] && cost < 0.06 && gap > 0.08 {
                    contradictory[j][usize::from(digit)] += 1;
                }
            }
        }
        out.digit_support_min = *supports.iter().min().unwrap();
        out.contradictions = contradictory.iter().flatten().filter(|&&n| n >= 2).count();
    }
}

/// Require visual margin, quiet zones and corroborating spatial evidence.
pub fn accepted(r: &Output) -> bool {
    let Some(h) = r.hypotheses.first() else {
        return false;
    };
    let margin = r.hypotheses.get(1).map_or(1., |s| s.score - h.score);
    ean::checksum(&h.result.digits)
        && h.score < 0.055
        && h.result.guard < 0.06
        && h.result.gap > 0.06
        && margin > 0.003
        && h.score < 0.6 * r.hypotheses.get(1).map_or(1., |s| s.score)
        && r.split_matches == 2
        && r.split_cost < 0.08
        && r.split_gap > 0.02
        && h.quiet < 0.20
        && r.digit_support_min >= 2
        && r.contradictions == 0
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_quantiles_preserve_window_sort_values() {
        for values in [
            vec![0.25; BINS],
            (0..BINS)
                .map(|j| f64::from(u32::try_from(j * 7919 % 113).unwrap()) / 113.)
                .collect(),
        ] {
            let actual = local_bounds(&values);
            for (j, &(low, high)) in actual.iter().enumerate() {
                let mut reference = values[j.saturating_sub(64)..(j + 65).min(BINS)].to_vec();
                reference.sort_by(f64::total_cmp);
                assert_eq!(low.to_bits(), reference[reference.len() / 10].to_bits());
                assert_eq!(
                    high.to_bits(),
                    reference[reference.len() * 9 / 10].to_bits()
                );
            }
        }
    }

    #[test]
    fn malformed_and_off_image_quads_are_bounded() {
        let bytes = vec![255; 100];
        let image = ImageView::new(&bytes, 10, 10, 1, 10).unwrap();
        for q in [
            [[f64::NAN, 0.]; 4],
            [[0., 0.]; 4],
            [[100., 100.], [120., 100.], [120., 110.], [100., 110.]],
        ] {
            let output = scan(image, q, true);
            assert!(output.hypotheses.is_empty());
            assert!(!accepted(&output));
        }
    }
    #[test]
    fn stability_never_repairs_an_invalid_visual_payload() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8];
        assert!(!ean::checksum(&digits));
        let mut gray = vec![255; 230 * 40];
        for y in 5..35 {
            for (j, bit) in ean::encode(&digits).into_iter().enumerate() {
                if bit > 0.5 {
                    gray[y * 230 + 20 + 2 * j] = 0;
                    gray[y * 230 + 21 + 2 * j] = 0;
                }
            }
        }
        let image = ImageView::new(&gray, 230, 40, 1, 230).unwrap();
        let quad = [[20., 5.], [210., 5.], [210., 35.], [20., 35.]];
        let mut result = scan(image, quad, false);
        assert_eq!(result.hypotheses[0].result.digits, digits);
        assert!(!accepted(&result));
        let px = pixels(image, quad);
        corroborate(&px, &mut result);
        let previous = result.split_matches;
        corroborate(&px, &mut result);
        assert_eq!(result.split_matches, previous);
        assert!(result.split_matches <= 2);
    }
}
