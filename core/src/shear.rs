//! Bounded image-evidence envelope fit for sheared barcode reading regions.
//! No decoder, expected text, or ground-truth geometry enters this module.
#![forbid(unsafe_code)]
use crate::{oriented::Proposal, sampling::ImageView, scan::Quad};
#[cfg(test)]
fn gradient_reference(im: ImageView<'_>, x: f64, y: f64) -> Option<[f64; 2]> {
    let (x, y) = (x.round(), y.round());
    if x < 1. || y < 1. || x >= (im.width - 1) as f64 || y >= (im.height - 1) as f64 {
        return None;
    }
    let g = |dx, dy| im.gray(x + dx, y + dy);
    Some([
        g(1., -1.) + 2. * g(1., 0.) + g(1., 1.) - g(-1., -1.) - 2. * g(-1., 0.) - g(-1., 1.),
        g(-1., 1.) + 2. * g(0., 1.) + g(1., 1.) - g(-1., -1.) - 2. * g(0., -1.) - g(1., -1.),
    ])
}
#[inline]
fn luminance(im: ImageView<'_>, x: usize, y: usize) -> f64 {
    let i = y * im.stride + x * im.channels;
    if im.channels == 1 {
        f64::from(im.data[i])
    } else if cfg!(feature = "experimental-green-luminance") {
        f64::from(im.data[i + 1])
    } else {
        0.299 * f64::from(im.data[i])
            + 0.587 * f64::from(im.data[i + 1])
            + 0.114 * f64::from(im.data[i + 2])
    }
}
fn gradient(im: ImageView<'_>, x: f64, y: f64) -> Option<[f64; 2]> {
    let (x, y) = (x.round() as isize, y.round() as isize);
    if x < 1 || y < 1 || x >= (im.width - 1) as isize || y >= (im.height - 1) as isize {
        return None;
    }
    let (x, y) = (x as usize, y as usize);
    let (a, b, c, d, e, f, g, h) = (
        luminance(im, x - 1, y - 1),
        luminance(im, x, y - 1),
        luminance(im, x + 1, y - 1),
        luminance(im, x - 1, y),
        luminance(im, x + 1, y),
        luminance(im, x - 1, y + 1),
        luminance(im, x, y + 1),
        luminance(im, x + 1, y + 1),
    );
    Some([
        c + 2. * e + h - a - 2. * d - f,
        f + 2. * g + h - a - 2. * b - c,
    ])
}
fn quantile(v: &mut [f64], fraction: f64) -> f64 {
    let x = (v.len() - 1) as f64 * fraction;
    let i = x.floor() as usize;
    let j = (i + 1).min(v.len() - 1);
    let (_, pivot, suffix) = v.select_nth_unstable_by(i, f64::total_cmp);
    let lo = *pivot;
    let hi = if j == i {
        lo
    } else {
        *suffix.select_nth_unstable_by(j - i - 1, f64::total_cmp).1
    };
    lo + (hi - lo) * (x - i as f64)
}
fn line(x: &[f64], y: &[f64], weights: &[f64]) -> Option<[f64; 2]> {
    let mut robust = vec![1.; x.len()];
    let mut result = [0.; 2];
    for iteration in 0..3 {
        let (mut sw, mut sx, mut sy, mut sxx, mut sxy) = (0., 0., 0., 0., 0.);
        for i in 0..x.len() {
            let w = weights[i] * robust[i];
            sw += w;
            sx += w * x[i];
            sy += w * y[i];
            sxx += w * x[i] * x[i];
            sxy += w * x[i] * y[i];
        }
        let den = sw * sxx - sx * sx;
        if den.abs() < 1e-8 {
            return None;
        }
        result = [(sw * sxy - sx * sy) / den, (sxx * sy - sx * sxy) / den];
        if iteration < 2 {
            let mut residual: Vec<_> = x
                .iter()
                .zip(y)
                .map(|(&x, &y)| (y - result[0] * x - result[1]).abs())
                .collect();
            let scale = (1.4826 * quantile(&mut residual, 0.5)).max(1.);
            for i in 0..x.len() {
                robust[i] =
                    (2.5 * scale / (y[i] - result[0] * x[i] - result[1]).abs().max(1e-6)).min(1.);
            }
        }
    }
    Some(result)
}
pub fn refine(im: ImageView<'_>, q: Quad) -> Option<Proposal> {
    crate::scan::transform(q).ok()?;
    let e = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let f = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let length = e[0].hypot(e[1]);
    let height = f[0].hypot(f[1]);
    if length < 48. || height < 12. {
        return None;
    }
    let mut derivatives = Vec::with_capacity(3840);
    let mut magnitudes = Vec::with_capacity(3840);
    for j in 0..40 {
        for i in 0..96 {
            let (u, v) = ((f64::from(i) + 0.5) / 96., (f64::from(j) + 0.5) / 40.);
            if let Some(d) = gradient(
                im,
                q[0][0] + u * e[0] + v * f[0],
                q[0][1] + u * e[1] + v * f[1],
            ) {
                let m = d[0].hypot(d[1]);
                derivatives.push((d, m));
                magnitudes.push(m);
            }
        }
    }
    if magnitudes.len() < 80 {
        return None;
    }
    let threshold = quantile(&mut magnitudes, 0.72).max(18.);
    let (mut xx, mut xy, mut yy, mut count) = (0., 0., 0., 0);
    for (d, m) in derivatives {
        if m < threshold {
            continue;
        }
        let w = m.min(100.);
        xx += w * d[0] * d[0];
        xy += w * d[0] * d[1];
        yy += w * d[1] * d[1];
        count += 1;
    }
    if count < 80 {
        return None;
    }
    let theta = 0.5 * (2. * xy).atan2(xx - yy);
    let mut normal = [theta.cos(), theta.sin()];
    let dot = (normal[0] * e[0] + normal[1] * e[1]) / length;
    if dot.abs() < 0.55 {
        return None;
    }
    if dot < 0. {
        normal = [-normal[0], -normal[1]];
    }
    let tangent = [-normal[1], normal[0]];
    let center = [
        q.iter().map(|p| p[0]).sum::<f64>() / 4.,
        q.iter().map(|p| p[1]).sum::<f64>() / 4.,
    ];
    // Four subpositions per normal bin and 64 tangent samples: 18,432 bounded
    // gradient probes independent of input resolution. Source pixels are unchanged.
    let mut samples = Vec::with_capacity(18432);
    let mut values = Vec::with_capacity(18432);
    for i in 0..288 {
        let alpha = (-0.58 + 1.16 * (i as f64 + 0.5) / 288.) * length;
        for j in 0..64 {
            let beta = (-1.05 + 2.1 * (f64::from(j) + 0.5) / 64.) * height;
            if let Some(d) = gradient(
                im,
                center[0] + alpha * normal[0] + beta * tangent[0],
                center[1] + alpha * normal[1] + beta * tangent[1],
            ) {
                let response = (d[0] * normal[0] + d[1] * normal[1]).abs();
                if response / d[0].hypot(d[1]).max(1.) >= 0.86 {
                    samples.push((i / 4, beta, response));
                    values.push(response);
                }
            }
        }
    }
    if values.len() < 80 {
        return None;
    }
    let threshold = quantile(&mut values, 0.78).max(22.);
    let mut bins: Vec<Vec<f64>> = (0..72).map(|_| Vec::new()).collect();
    for (i, b, r) in samples {
        if r >= threshold {
            bins[i].push(b);
        }
    }
    let (mut x, mut low, mut high, mut weights) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, b) in bins.iter_mut().enumerate() {
        if b.len() < 5 {
            continue;
        }
        x.push((-0.58 + 1.16 * (i as f64 + 0.5) / 72.) * length);
        low.push(quantile(b, 0.08));
        high.push(quantile(b, 0.92));
        weights.push(b.len() as f64);
    }
    if x.len() < 31 {
        return None;
    }
    let lo = line(&x, &low, &weights)?;
    let hi = line(&x, &high, &weights)?;
    let mut widths: Vec<_> = x
        .iter()
        .map(|&a| (hi[0] - lo[0]) * a + hi[1] - lo[1])
        .collect();
    if widths.iter().any(|&v| v < 5.) {
        return None;
    }
    let median = quantile(&mut widths, 0.5);
    if median < 8. || median > 2.4 * height {
        return None;
    }
    let point = |a: f64, b: f64| {
        [
            center[0] + a * normal[0] + b * tangent[0],
            center[1] + a * normal[1] + b * tangent[1],
        ]
    };
    let (a, b) = (-0.58 * length, 0.58 * length);
    let polygon = [
        point(a, lo[0] * a + lo[1]),
        point(b, lo[0] * b + lo[1]),
        point(b, hi[0] * b + hi[1]),
        point(a, hi[0] * a + hi[1]),
    ];
    crate::scan::transform(polygon).ok()?;
    let score = (xx - yy).hypot(2. * xy) / (xx + yy + 1e-6);
    Some(Proposal { polygon, score })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn quantile_reference(v: &mut [f64], fraction: f64) -> f64 {
        v.sort_by(f64::total_cmp);
        let x = (v.len() - 1) as f64 * fraction;
        let i = x.floor() as usize;
        v[i] + (v[(i + 1).min(v.len() - 1)] - v[i]) * (x - i as f64)
    }
    #[test]
    fn select_quantile_matches_full_sort_on_ties_signed_and_endpoints() {
        let fixtures = vec![
            vec![0., 0., 0., 1., 1., -2., 4.],
            vec![-9., -3., -3., -1., 2., 8.],
            vec![f64::NAN, -1., 0., 1., f64::NAN],
        ];
        for values in fixtures {
            for fraction in [0., 0.01, 0.08, 0.5, 0.72, 0.78, 0.92, 0.999, 1.] {
                let mut a = values.clone();
                let mut b = values.clone();
                let got = quantile(&mut a, fraction);
                let expected = quantile_reference(&mut b, fraction);
                assert_eq!(got.to_bits(), expected.to_bits(), "fraction {fraction}");
            }
        }
    }
    #[test]
    fn rejects_uniform_invalid_and_degenerate_inputs() {
        let data = vec![127; 512 * 256];
        let im = ImageView::new(&data, 512, 256, 1, 512).unwrap();
        assert!(refine(im, [[10., 10.], [450., 10.], [450., 200.], [10., 200.]]).is_none());
        assert!(refine(im, [[0., 0.]; 4]).is_none());
    }
    #[test]
    fn robust_lines_reject_sparse_degenerate_and_resist_outlier() {
        assert!(line(&[0., 0.], &[1., 2.], &[1., 1.]).is_none());
        let x: Vec<_> = (0..32).map(f64::from).collect();
        let mut y: Vec<_> = x.iter().map(|x| 2. * x + 3.).collect();
        y[15] += 30.;
        let l = line(&x, &y, &vec![1.; 32]).unwrap();
        assert!((l[0] - 2.).abs() < 0.01);
        assert!((l[1] - 3.).abs() < 0.2);
    }
    #[test]
    fn direct_gradient_matches_reference_across_formats_strides_and_edges() {
        let mut seed = 17u32;
        for (channels, pad) in [(1, 0), (3, 2), (4, 3)] {
            let (w, h) = (11, 9);
            let stride = w * channels + pad;
            let mut data = vec![0; stride * h];
            for v in &mut data {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *v = (seed >> 24) as u8;
            }
            let im = ImageView::new(&data, w, h, channels, stride).unwrap();
            for x in [0., 0.4, 1., 1.49, 2.5, 5., 9., 10., 10.6] {
                for y in [0., 0.5, 1., 2.49, 4., 7.5, 8., 8.9] {
                    assert_eq!(
                        gradient(im, x, y),
                        gradient_reference(im, x, y),
                        "channels {channels} stride {stride} at {x},{y}"
                    );
                }
            }
        }
    }
}
