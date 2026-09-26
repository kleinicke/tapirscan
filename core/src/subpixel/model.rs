//! Pixel-area inverse model for an affine EAN-13 region.
use super::sequence;
use crate::ean;
const N: usize = 95;
#[derive(Clone)]
struct Measurement {
    value: f64,
    weights: Vec<(usize, f64)>,
}

fn cdf(t: f64, first: f64, second: f64) -> f64 {
    let (first, second) = (first.max(second), first.min(second));
    if second < 1e-8 {
        return (t / first + 0.5).clamp(0., 1.);
    }
    let x = t + (first + second) * 0.5;
    let positive_square = |v: f64| v.max(0.).powi(2);
    ((positive_square(x) - positive_square(x - first) - positive_square(x - second)
        + positive_square(x - first - second))
        / (2. * first * second))
        .clamp(0., 1.)
}

// Normal CDF approximation for integrating optical Gaussian blur over first sensor pixel.
fn normal_cdf(x: f64) -> f64 {
    let t = 1. / (1. + 0.231_641_9 * x.abs());
    let density = (-x * x * 0.5).exp() / std::f64::consts::TAU.sqrt();
    let tail = density
        * t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    if x >= 0. {
        1. - tail
    } else {
        tail
    }
}
fn blurred_cdf(t: f64, first: f64, second: f64, sigma: f64) -> f64 {
    if sigma <= 0. {
        return cdf(t, first, second);
    }
    let (first, second) = (first.max(second), first.min(second));
    let f1 = |x: f64| {
        x * normal_cdf(x / sigma)
            + sigma * (-0.5 * (x / sigma).powi(2)).exp() / std::f64::consts::TAU.sqrt()
    };
    if second < 1e-5 {
        return ((f1(t + first * 0.5) - f1(t - first * 0.5)) / first).clamp(0., 1.);
    }
    let f2 = |x: f64| {
        (x * x + sigma * sigma) * normal_cdf(x / sigma)
            + x * sigma * (-0.5 * (x / sigma).powi(2)).exp() / std::f64::consts::TAU.sqrt()
    };
    let x = t + (first + second) * 0.5;
    ((f2(x) - f2(x - first) - f2(x - second) + f2(x - first - second)) / (2. * first * second))
        .clamp(0., 1.)
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub lambda: f64,
    pub sigma: f64,
    pub stride: usize,
    pub max_error: f64,
    pub photometric: bool,
    pub sequence: bool,
    pub oriented_stride: bool,
    pub gamma: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            lambda: 0.001,
            sigma: 0.,
            stride: 1,
            max_error: 0.0009,
            photometric: true,
            sequence: false,
            oriented_stride: false,
            gamma: 1.,
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "The bounded inverse solve shares pixel measurements, normal equations and the forward validation pass."
)]
pub(super) fn reconstruct_model(
    image: crate::sampling::ImageView<'_>,
    quad: [[f64; 2]; 4],
    settings: Settings,
) -> Option<[u8; 13]> {
    let Settings {
        lambda,
        sigma,
        stride,
        max_error,
        photometric,
        sequence,
        oriented_stride,
        gamma,
    } = settings;
    let (image_width, image_height) = (image.width, image.height);
    let dx = quad[1][0] - quad[0][0];
    let dy = quad[1][1] - quad[0][1];
    let length = dx.hypot(dy);
    let pitch = length / crate::numeric::usize_f64(N);
    let cosine = dx / length;
    let sine = dy / length;
    let height = (quad[3][0] - quad[0][0]).hypot(quad[3][1] - quad[0][1]);
    let along_x = oriented_stride && sine.abs() > cosine.abs();
    let row_step = if along_x { 1 } else { stride.max(1) };
    let col_step = if along_x { stride.max(1) } else { 1 };
    // All measurement and quiet-zone supports lie in this expanded source ROI.
    // Preserve the global sampling phase when skipping rows/columns.
    let margin = (10. * pitch + 2.).max(1. + 4. * sigma);
    let x0 = (crate::numeric::f64_usize(
        (quad.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min) - margin)
            .floor()
            .max(0.),
    ))
    .div_ceil(col_step)
        * col_step;
    let y0 = (crate::numeric::f64_usize(
        (quad.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min) - margin)
            .floor()
            .max(0.),
    ))
    .div_ceil(row_step)
        * row_step;
    let x1 = (crate::numeric::f64_usize(
        (quad.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) + margin)
            .ceil()
            .max(0.),
    ))
    .min(image_width);
    let y1 = (crate::numeric::f64_usize(
        (quad.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max) + margin)
            .ceil()
            .max(0.),
    ))
    .min(image_height);
    let background = if photometric {
        let mut samples = Vec::new();
        for y in (y0..y1).step_by(row_step) {
            for x in (x0..x1).step_by(col_step) {
                let rx = crate::numeric::usize_f64(x) + 0.5 - quad[0][0];
                let ry = crate::numeric::usize_f64(y) + 0.5 - quad[0][1];
                let u = cosine * rx + sine * ry;
                let v = -sine * rx + cosine * ry;
                if v > height * 0.15
                    && v < height * 0.85
                    && ((u < -2. && u > -10. * pitch - 2.)
                        || (u > length + 2. && u < length + 10. * pitch + 2.))
                {
                    samples.push(
                        image.gray(crate::numeric::usize_f64(x), crate::numeric::usize_f64(y))
                            / 255.,
                    );
                }
            }
        }
        if samples.len() < 8 {
            return None;
        }
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2].powf(gamma)
    } else {
        1.
    };
    let mut measurements = Vec::new();
    let mut gram = vec![0.; N * N];
    let mut rhs = [0.; N];
    let radius = (cosine.abs() + sine.abs()) * 0.5 + sigma * 4.;
    // A retained pixel must not integrate across the finite top/bottom of first bar.
    let vertical_margin = (height * 0.15).max(radius);
    let support = radius;
    let bandwidth = (crate::numeric::f64_usize((2. * radius / pitch).ceil()) + 1).min(N - 1);
    let lut: Vec<_> = (0usize..=if sigma > 0. { 2048 } else { 0 })
        .map(|i| {
            blurred_cdf(
                -radius + 2. * radius * crate::numeric::usize_f64(i) / 2048.,
                cosine.abs(),
                sine.abs(),
                sigma,
            )
        })
        .collect();
    let kernel = |t: f64| {
        if sigma == 0. {
            return cdf(t, cosine.abs(), sine.abs());
        }
        let at = ((t + radius) / (2. * radius) * 2048.).clamp(0., 2048.);
        let i = crate::numeric::f64_usize(at.floor());
        lut[i] + (lut[(i + 1).min(2048)] - lut[i]) * (at - crate::numeric::usize_f64(i))
    };
    for y in (y0..y1).step_by(row_step) {
        for x in (x0..x1).step_by(col_step) {
            let rx = crate::numeric::usize_f64(x) + 0.5 - quad[0][0];
            let ry = crate::numeric::usize_f64(y) + 0.5 - quad[0][1];
            let u = cosine * rx + sine * ry;
            let v = -sine * rx + cosine * ry;
            if v < vertical_margin
                || v > height - vertical_margin
                || u < -support
                || u > length + support
            {
                continue;
            }
            let lo = crate::numeric::f64_usize(((u - radius) / pitch).floor().max(0.));
            let hi = crate::numeric::f64_usize(((u + radius) / pitch).floor().max(0.)).min(N - 1);
            let mut weights = Vec::new();
            for j in lo..=hi {
                let weight = kernel(crate::numeric::usize_f64(j + 1) * pitch - u)
                    - kernel(crate::numeric::usize_f64(j) * pitch - u);
                if weight > 1e-10 {
                    weights.push((j, weight));
                }
            }
            if weights.is_empty() {
                continue;
            }
            let value = background
                - (image.gray(crate::numeric::usize_f64(x), crate::numeric::usize_f64(y)) / 255.)
                    .powf(gamma);
            for &(j, first) in &weights {
                rhs[j] += first * value;
                for &(k, second) in &weights {
                    gram[j * N + k] += first * second;
                }
            }
            measurements.push(Measurement { value, weights });
        }
    }
    let diagonal = (0..N).map(|j| gram[j * N + j]).sum::<f64>() / crate::numeric::usize_f64(N);
    let original_gram = if sequence { gram.clone() } else { Vec::new() };
    let original_rhs = rhs;
    if diagonal < 0.01 {
        return None;
    }
    for j in 0..N {
        gram[j * N + j] += lambda * diagonal;
        rhs[j] += 0.5 * lambda * diagonal;
    }
    // Cholesky normal-equation solve. Tikhonov prior is neutral gray, not barcode bits.
    for i in 0..N {
        for j in i.saturating_sub(bandwidth)..=i {
            let mut v = gram[i * N + j];
            for k in i.saturating_sub(bandwidth)..j {
                v -= gram[i * N + k] * gram[j * N + k];
            }
            gram[i * N + j] = if i == j {
                v.max(1e-12).sqrt()
            } else {
                v / gram[j * N + j]
            };
        }
    }
    let mut modules = [0.; N];
    for i in 0..N {
        let mut v = rhs[i];
        for j in i.saturating_sub(bandwidth)..i {
            v -= gram[i * N + j] * modules[j];
        }
        modules[i] = v / gram[i * N + i];
    }
    for i in (0..N).rev() {
        let mut v = modules[i];
        for j in i + 1..(i + bandwidth + 1).min(N) {
            v -= gram[j * N + i] * modules[j];
        }
        modules[i] = v / gram[i * N + i];
    }
    let (floor, gain) = if photometric && sequence {
        // Estimate contrast only from pixels whose entire support is known guards.
        // Uncertain reconstructed payload amplitudes must not choose another text.
        let mut xx = 0.;
        let mut xy = 0.;
        for m in &measurements {
            if m.weights
                .iter()
                .all(|&(j, _)| j < 3 || (45..50).contains(&j) || j >= 92)
            {
                let x = m
                    .weights
                    .iter()
                    .filter(|&&(j, _)| [0, 2, 46, 48, 92, 94].contains(&j))
                    .map(|&(_, image_width)| image_width)
                    .sum::<f64>();
                xx += x * x;
                xy += x * m.value;
            }
        }
        if xx < 0.02 {
            return None;
        }
        let gain = xy / xx;
        if !(0.1..=1.3).contains(&gain) {
            return None;
        }
        (0., gain)
    } else if photometric {
        let mut sorted = modules;
        sorted.sort_by(f64::total_cmp);
        (sorted[5], sorted[89] - sorted[5])
    } else {
        (0., 1.)
    };
    if gain < 0.1 {
        return None;
    }
    let clipped: [f32; N] = std::array::from_fn(|j| {
        crate::numeric::f64_f32(((modules[j] - floor) / gain).clamp(0., 1.))
    });
    let mut result = None;
    for reverse in [false, true] {
        let input =
            std::array::from_fn::<_, N, _>(|j| clipped[if reverse { N - 1 - j } else { j }]);
        let decoded = if sequence {
            let g = if reverse {
                (0..N * N)
                    .map(|i| original_gram[(N - 1 - i / N) * N + N - 1 - i % N])
                    .collect()
            } else {
                original_gram.clone()
            };
            let image_height =
                std::array::from_fn(|j| original_rhs[if reverse { N - 1 - j } else { j }]);
            sequence::decode(&g, &image_height, floor, gain, diagonal, bandwidth).map(|digits| {
                ean::Result {
                    digits,
                    cost: 0.,
                    gap: 0.,
                    guard: 0.,
                }
            })
        } else {
            ean::decode(&input, 0.1, 0.02)
        };
        let accepted = decoded.and_then(|r| {
            let mut bits = ean::encode(&r.digits);
            if reverse {
                bits.reverse();
            }
            let (offset, gain) = if photometric {
                let mut sx = 0.;
                let mut sy = 0.;
                let mut sxx = 0.;
                let mut sxy = 0.;
                for m in &measurements {
                    let x = m
                        .weights
                        .iter()
                        .map(|&(j, image_width)| image_width * f64::from(bits[j]))
                        .sum::<f64>();
                    sx += x;
                    sy += m.value;
                    sxx += x * x;
                    sxy += x * m.value;
                }
                let n = crate::numeric::usize_f64(measurements.len());
                let gain = (sxy - sx * sy / n) / (sxx - sx * sx / n).max(1e-8);
                (sy / n - gain * sx / n, gain)
            } else {
                (0., 1.)
            };
            if !(0.1..=1.3).contains(&gain) {
                return None;
            }
            let error = measurements
                .iter()
                .map(|m| {
                    let prediction = m
                        .weights
                        .iter()
                        .map(|&(j, image_width)| image_width * f64::from(bits[j]))
                        .sum::<f64>();
                    (offset + gain * prediction - m.value).powi(2)
                })
                .sum::<f64>()
                / crate::numeric::usize_f64(measurements.len());
            (error < max_error * gain * gain).then_some(r.digits)
        });
        if let Some(r) = accepted {
            if result.is_some_and(|old| old != r) {
                return None;
            }
            result = Some(r);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn area_kernel_is_symmetric_and_normalized() {
        for angle in [0f64, 0.1, 0.3, 0.7] {
            let first = angle.cos().abs();
            let second = angle.sin().abs();
            for t in [-2., -0.6, -0.1, 0., 0.1, 0.6, 2.] {
                assert!((cdf(t, first, second) + cdf(-t, first, second) - 1.).abs() < 1e-10);
            }
            assert!(cdf(-2., first, second).abs() < 1e-12);
            assert!((cdf(2., first, second) - 1.).abs() < 1e-12);
        }
    }
}
