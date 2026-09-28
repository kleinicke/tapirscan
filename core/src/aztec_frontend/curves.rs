use super::decoder as aztec;
use barcode_multiformat::{
    numeric::{f32_usize, usize_f32, usize_f64},
    qr_detect, Detection,
};
/// Source pixels remain in the original image coordinate system.
#[derive(Clone, Copy)]
pub(super) struct Input<'a> {
    pub gray: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub mode: usize,
}
#[derive(Clone, Copy)]
struct Hypothesis {
    bend: f32,
    axis: usize,
    offset: [f32; 2],
    kind: usize,
}
pub(super) struct Grid {
    pub t: [f32; 8],
    pub n: usize,
    pub turn: usize,
    pub mirror: bool,
    pub compact: bool,
    pub descriptor: aztec::Mode,
}
pub(super) struct Seed {
    score: f32,
    grid: Grid,
}
pub(super) fn retain(seeds: &mut Vec<Seed>, input: Input<'_>, grid: Grid) {
    let t = grid.t;
    let n = grid.n;
    let Input {
        gray,
        width,
        height,
        mode,
    } = input;
    let half = usize_f32(n) * 0.5;
    let denominator = 1. + half * (t[6] + t[7]);
    let base = [
        t[0] / denominator,
        t[1] / denominator,
        (t[2] + half * (t[0] + t[1])) / denominator,
        t[3] / denominator,
        t[4] / denominator,
        (t[5] + half * (t[3] + t[4])) / denominator,
        t[6] / denominator,
        t[7] / denominator,
    ];
    let score = super::refinement::score(gray, width, height, &base, mode);
    if !score.is_finite() || score > -8. {
        return;
    }
    seeds.push(Seed { score, grid });
    seeds.sort_by(|a, b| a.score.total_cmp(&b.score));
    seeds.truncate(4);
}
fn warp(x: f32, y: f32, n: usize, bend: f32, axis: usize, kind: usize) -> [f32; 2] {
    let half = usize_f32(n) * 0.5;
    let mut p = [x - half, y - half];
    let fraction = p[axis] / half;
    if kind == 0 {
        p[axis] *= 1. + bend * (fraction * fraction - (4. / half).powi(2));
    } else {
        p[1 - axis] += bend * (p[axis] * p[axis] - 16.) / half;
    }
    [p[0] + half, p[1] + half]
}
fn sample(
    input: Input<'_>,
    transform: &[f32; 8],
    n: usize,
    hypothesis: Hypothesis,
    budget: &mut usize,
    limited: &mut bool,
) -> Option<Vec<bool>> {
    let Input {
        gray,
        width,
        height,
        mode,
    } = input;
    let Hypothesis {
        bend,
        axis,
        offset,
        kind,
    } = hypothesis;
    if *budget < n * n {
        *limited = true;
        return None;
    }
    *budget -= n * n;
    let mut vals = Vec::with_capacity(n * n);
    let mut hist = [0_usize; 256];
    let mut sum = 0_usize;
    for y in 0..n {
        for x in 0..n {
            let [x, y] = warp(usize_f32(x) + 0.5, usize_f32(y) + 0.5, n, bend, axis, kind);
            let [px, py] = qr_detect::map(transform, x + offset[0], y + offset[1]);
            let sx = px - 0.5;
            let sy = py - 0.5;
            if !sx.is_finite()
                || !sy.is_finite()
                || sx < 0.
                || sy < 0.
                || sx >= usize_f32(width - 1)
                || sy >= usize_f32(height - 1)
            {
                return None;
            }
            let ix = f32_usize(sx.floor());
            let iy = f32_usize(sy.floor());
            let dx = sx - usize_f32(ix);
            let dy = sy - usize_f32(iy);
            let at = iy * width + ix;
            let value = (f32::from(gray[at]) * (1. - dx) + f32::from(gray[at + 1]) * dx)
                * (1. - dy)
                + (f32::from(gray[at + width]) * (1. - dx) + f32::from(gray[at + width + 1]) * dx)
                    * dy;
            let v = f32_usize(value.round().clamp(0., 255.));
            vals.push(v);
            hist[v] += 1;
            sum += v;
        }
    }
    if kind == 2 {
        let side = n + 1;
        let mut sums = vec![0_usize; side * side];
        for y in 0..n {
            let mut row = 0;
            for x in 0..n {
                row += vals[y * n + x];
                sums[(y + 1) * side + x + 1] = sums[y * side + x + 1] + row;
            }
        }
        let mut bits = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let left = x.saturating_sub(3);
                let right = (x + 4).min(n);
                let top = y.saturating_sub(3);
                let bottom = (y + 4).min(n);
                let total = sums[bottom * side + right] + sums[top * side + left]
                    - sums[top * side + right]
                    - sums[bottom * side + left];
                let area = (bottom - top) * (right - left);
                bits.push((vals[y * n + x] * area < total) ^ matches!(mode, 2 | 3 | 5 | 7));
            }
        }
        return Some(bits);
    }
    let mut count = 0;
    let mut left = 0;
    let mut best = 0.;
    let mut cut = 128;
    for (v, &mass) in hist.iter().enumerate() {
        count += mass;
        left += v * mass;
        if count == 0 || count == n * n {
            continue;
        }
        let delta =
            usize_f64(left) / usize_f64(count) - usize_f64(sum - left) / usize_f64(n * n - count);
        let score = usize_f64(count) * usize_f64(n * n - count) * delta * delta;
        if score > best {
            best = score;
            cut = v;
        }
    }
    Some(
        vals.into_iter()
            .map(|v| (v <= cut) ^ matches!(mode, 2 | 3 | 5 | 7))
            .collect(),
    )
}
/// Independent source-gray measurement of a corrected, otherwise unstable Rune.
pub(super) fn sample_rune(
    input: Input<'_>,
    transform: &[f32; 8],
    budget: &mut usize,
    limited: &mut bool,
) -> Option<Vec<bool>> {
    sample(
        input,
        transform,
        11,
        Hypothesis {
            bend: 0.,
            axis: 0,
            offset: [0., 0.],
            kind: 0,
        },
        budget,
        limited,
    )
}
pub(super) fn recover(
    input: Input<'_>,
    seeds: &[Seed],
    support: usize,
    budget: &mut usize,
    bow_budget: &mut usize,
    adaptive_budget: &mut usize,
    limited: &mut bool,
) -> Option<Detection> {
    for kind in 0..3 {
        for seed in seeds.iter().take(2) {
            for (bend, offset) in [
                (0., [0., 0.]),
                (0., [-0.15, 0.]),
                (0., [0.15, 0.]),
                (0., [0., -0.15]),
                (0., [0., 0.15]),
                (-0.05, [0., 0.]),
                (0.05, [0., 0.]),
                (-0.1, [0., 0.]),
                (0.1, [0., 0.]),
                (-0.2, [0., 0.]),
                (0.2, [0., 0.]),
                (-0.3, [0., 0.]),
                (0.3, [0., 0.]),
            ] {
                if (kind == 1 && bend == 0.) || (kind == 2 && (bend != 0. || offset != [0., 0.])) {
                    continue;
                }
                for axis in 0..if bend == 0. { 1 } else { 2 } {
                    let Some(bits) = sample(
                        input,
                        &seed.grid.t,
                        seed.grid.n,
                        Hypothesis {
                            bend,
                            axis,
                            offset,
                            kind,
                        },
                        match kind {
                            0 => &mut *budget,
                            1 => &mut *bow_budget,
                            _ => &mut *adaptive_budget,
                        },
                        limited,
                    ) else {
                        continue;
                    };
                    if let Some(read) = aztec::decode_consistent(
                        &super::rotate(&bits, seed.grid.n, seed.grid.turn, seed.grid.mirror),
                        seed.grid.n,
                        seed.grid.compact,
                        &seed.grid.descriptor,
                    ) {
                        let n = usize_f32(seed.grid.n);
                        let polygon = [[0., 0.], [n, 0.], [n, n], [0., n]].map(|[x, y]| {
                            let [x, y] = warp(x, y, seed.grid.n, bend, axis, kind);
                            qr_detect::map(&seed.grid.t, x, y)
                        });
                        return Some(Detection {
                            bytes: Some(read.bytes),
                            structured_append: read.structured_append,
                            reader_initialization: read.reader_initialization,
                            addon: None,
                            format: "Aztec".into(),
                            text: read.text,
                            polygon,
                            support,
                            error: usize_f32(read.corrected),
                            gs1: read.gs1,
                        });
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_budget_is_charged_before_work_and_reports_exhaustion() {
        let gray = vec![255; 20 * 20];
        let input = Input {
            gray: &gray,
            width: 20,
            height: 20,
            mode: 0,
        };
        let seed = Seed {
            score: -20.,
            grid: Grid {
                t: [1., 0., 0., 0., 1., 0., 0., 0.],
                n: 15,
                turn: 0,
                mirror: false,
                compact: true,
                descriptor: aztec::Mode {
                    layers: 1,
                    data_words: 1,
                    reader_initialization: false,
                },
            },
        };
        let hypothesis = Hypothesis {
            bend: 0.,
            axis: 0,
            offset: [0., 0.],
            kind: 0,
        };
        let mut budget = 224;
        let mut limited = false;
        assert!(sample(
            input,
            &seed.grid.t,
            seed.grid.n,
            hypothesis,
            &mut budget,
            &mut limited
        )
        .is_none());
        assert!(limited);
        assert_eq!(budget, 224);
        budget = 225;
        limited = false;
        assert_eq!(
            sample(
                input,
                &seed.grid.t,
                seed.grid.n,
                hypothesis,
                &mut budget,
                &mut limited
            ),
            Some(vec![false; 225])
        );
        assert!(!limited);
        assert_eq!(budget, 0);
    }
    #[test]
    fn both_warps_anchor_the_outer_bullseye_reference() {
        for n in [15, 27, 63] {
            let half = usize_f32(n) * 0.5;
            for axis in 0..2 {
                for kind in 0..2 {
                    for bend in [-0.3, 0., 0.3] {
                        for offset in [-4., 4.] {
                            let point = [half + offset, half + offset];
                            let mapped = warp(point[0], point[1], n, bend, axis, kind);
                            assert!(
                                (mapped[0] - point[0]).abs() < 0.000_01
                                    && (mapped[1] - point[1]).abs() < 0.000_01
                            );
                        }
                    }
                }
            }
        }
    }
}
