//! Independent original-pixel evidence that confirms or contradicts recovered reads.
use super::{checked_image, Error, Image, Quad};

pub(crate) fn source_contradiction(
    image: Image<'_>,
    q: Quad,
    expected: [u8; 13],
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<bool, Error> {
    let im = checked_image(image)?;
    let point = |u: f64, v: f64| {
        let a = [
            q[0][0] + (q[1][0] - q[0][0]) * u,
            q[0][1] + (q[1][1] - q[0][1]) * u,
        ];
        let b = [
            q[3][0] + (q[2][0] - q[3][0]) * u,
            q[3][1] + (q[2][1] - q[3][1]) * u,
        ];
        [a[0] + (b[0] - a[0]) * v, a[1] + (b[1] - a[1]) * v]
    };
    let mut conflicts: Vec<([u8; 13], usize)> = Vec::new();
    for v in [0.2, 0.5, 0.8] {
        sampler.sample_limited(im, point(-0.15, v), point(1.15, v), 3., 1536);
        let mut row = Vec::new();
        for method in 0..3 {
            if method == 1 {
                sampler.adaptive_threshold();
            } else {
                sampler.threshold(method == 0);
            }
            for reverse in [false, true] {
                if reverse {
                    sampler.runs.reverse();
                }
                let black = if reverse {
                    sampler.first_black ^ sampler.runs.len().is_multiple_of(2)
                } else {
                    sampler.first_black
                };
                let total = sampler.runs.iter().sum::<f32>();
                for start in (usize::from(!black)..sampler.runs.len().saturating_sub(59)).step_by(2)
                {
                    if start == 0 {
                        continue;
                    }
                    let widths = &sampler.runs[start..start + 59];
                    let left = sampler.runs[..start].iter().sum::<f32>() / total;
                    let symbol_width = widths.iter().sum::<f32>();
                    let right = left + symbol_width / total;
                    if (left - 0.15 / 1.3).abs() > 0.12 || (right - 1.15 / 1.3).abs() > 0.12 {
                        continue;
                    }
                    let module = symbol_width / 95.;
                    if sampler.runs[start - 1] < module * 4.
                        || sampler.runs[start + 59] < module * 4.
                    {
                        continue;
                    }
                    if let Some(e) = barcode_research_core::run_ean::decode_visual_evidence(widths)
                    {
                        if e.digits != expected && !row.contains(&e.digits) {
                            row.push(e.digits);
                        }
                    }
                }
                if reverse {
                    sampler.runs.reverse();
                }
            }
        }
        for digits in row {
            if let Some((_, count)) = conflicts.iter_mut().find(|(d, _)| *d == digits) {
                *count += 1;
                if *count >= 2 {
                    return Ok(true);
                }
            } else {
                conflicts.push((digits, 1));
            }
        }
    }
    Ok(false)
}

pub(super) fn raw_source_normalize(values: &mut [f64; 256]) -> f64 {
    use barcode_research_core::numeric::usize_f64;
    const N: usize = 256;
    let mean = values.iter().sum::<f64>() / 256.;
    let slope = values
        .iter()
        .enumerate()
        .map(|(i, v)| (usize_f64(i) - 127.5) * (v - mean))
        .sum::<f64>()
        / (0..N).map(|i| (usize_f64(i) - 127.5).powi(2)).sum::<f64>();
    for (i, v) in values.iter_mut().enumerate() {
        *v -= mean + slope * (usize_f64(i) - 127.5);
    }
    let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0. {
        for v in values {
            *v /= norm;
        }
    }
    norm
}

pub(super) fn raw_source_profiles(image: Image<'_>, q: Quad) -> Vec<([f64; 256], f64)> {
    use barcode_research_core::numeric::{f64_usize, usize_f64};
    let gray = |x: usize, y: usize| {
        let at = y * image.stride + x * image.channels;
        if image.channels == 1 {
            f64::from(image.data[at])
        } else {
            0.299 * f64::from(image.data[at])
                + 0.587 * f64::from(image.data[at + 1])
                + 0.114 * f64::from(image.data[at + 2])
        }
    };
    let pixel = |x: f64, y: f64| {
        if x < 0. || y < 0. || x > usize_f64(image.width - 1) || y > usize_f64(image.height - 1) {
            return 255.;
        }
        let ix = f64_usize(x.floor());
        let iy = f64_usize(y.floor());
        let fx = x - x.floor();
        let fy = y - y.floor();
        let right = (ix + 1).min(image.width - 1);
        let bottom = (iy + 1).min(image.height - 1);
        (gray(ix, iy) * (1. - fx) + gray(right, iy) * fx) * (1. - fy)
            + (gray(ix, bottom) * (1. - fx) + gray(right, bottom) * fx) * fy
    };
    [0.2, 0.5, 0.8]
        .into_iter()
        .map(|v| {
            let a = [
                q[0][0] * (1. - v) + q[3][0] * v,
                q[0][1] * (1. - v) + q[3][1] * v,
            ];
            let b = [
                q[1][0] * (1. - v) + q[2][0] * v,
                q[1][1] * (1. - v) + q[2][1] * v,
            ];
            let mut row = std::array::from_fn(|i| {
                let u = -0.02 + 1.04 * usize_f64(i) / 255.;
                pixel(a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u)
            });
            let norm = raw_source_normalize(&mut row);
            (row, norm)
        })
        .collect()
}

// Optional recovery acceptance only: test the claimed payload against unmodified
// source luminance. Never repair digits or change existing decoder acceptance.
pub(crate) fn recovered_ean_source_agreement(image: Image<'_>, q: Quad, digits: &[u8; 13]) -> bool {
    use barcode_research_core::numeric::{f64_isize, f64_usize, isize_f64, usize_f64};
    let profiles = raw_source_profiles(image, q);
    let height = f64::midpoint(
        (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]),
        (q[2][0] - q[1][0]).hypot(q[2][1] - q[1][1]),
    );
    // Eligibility follows the same variance and physical-row separation gates.
    if !(0..3).any(|i| {
        ((i + 1)..3).any(|j| {
            profiles[i].1 >= 160. && profiles[j].1 >= 160. && usize_f64(j - i) * 0.3 * height >= 2.
        })
    }) {
        return false;
    }
    let bits = barcode_research_core::ean::encode(digits);
    let mut best = [0_f64; 3];
    for reverse in [false, true] {
        let raster: Vec<f64> = (0..824)
            .map(|i| {
                let module = f64_isize((usize_f64(i) / 8. - 4.).floor());
                if (0..95).contains(&module) {
                    f64::from(
                        bits[if reverse {
                            94 - usize::try_from(module).expect("module checked in 0..95")
                        } else {
                            usize::try_from(module).expect("module checked in 0..95")
                        }],
                    )
                } else {
                    0.
                }
            })
            .collect();
        for sigma in [0., 0.3, 0.6, 0.9] {
            let smoothed = if sigma == 0. {
                raster.clone()
            } else {
                let radius = f64_isize((sigma * 8. * 3_f64).ceil());
                let weights: Vec<_> = (-radius..=radius)
                    .map(|k| (-0.5 * (isize_f64(k) / (sigma * 8.)).powi(2)).exp())
                    .collect();
                let sum = weights.iter().sum::<f64>();
                (0..raster.len())
                    .map(|i| {
                        (-radius..=radius)
                            .zip(&weights)
                            .map(|(k, w)| {
                                let index =
                                    isize::try_from(i).expect("fixed 824-sample raster") + k;
                                if index < 0
                                    || index
                                        >= isize::try_from(raster.len())
                                            .expect("fixed 824-sample raster")
                                {
                                    0.
                                } else {
                                    raster
                                        [usize::try_from(index).expect("index checked nonnegative")]
                                        * w
                                }
                            })
                            .sum::<f64>()
                            / sum
                    })
                    .collect()
            };
            for pitch in [0.98, 1., 1.02] {
                for offset in [-1., -0.5, 0., 0.5, 1.] {
                    let mut template = std::array::from_fn(|i| {
                        let u = -0.02 + 1.04 * usize_f64(i) / 255.;
                        let index = ((95. * u - offset) / pitch + 4.) * 8.;
                        if index < 0. || index >= usize_f64(smoothed.len() - 1) {
                            return 0.;
                        }
                        let k = f64_usize(index.floor());
                        let f = index - index.floor();
                        smoothed[k] * (1. - f) + smoothed[k + 1] * f
                    });
                    raw_source_normalize(&mut template);
                    for (j, (row, norm)) in profiles.iter().enumerate() {
                        if *norm < 160. {
                            continue;
                        }
                        let corr = template.iter().zip(row).map(|(a, b)| a * b).sum::<f64>();
                        best[j] = best[j].min(corr);
                    }
                    if separated_source_rows_agree(&best, height) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(feature = "medium")]
pub(super) fn independently_confirmed_ean_reads(
    image: Image<'_>,
    extra: Vec<crate::read::Read>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<Vec<crate::read::Read>, Error> {
    if extra.len() != 1 {
        return Ok(Vec::new());
    }
    let mut accepted = Vec::new();
    for r in extra {
        let bytes = r.text.as_bytes();
        if r.support < 4 || bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let digits = std::array::from_fn(|i| bytes[i] - b'0');
        if recovered_ean_source_agreement(image, r.polygon, &digits)
            && !source_contradiction(image, r.polygon, digits, sampler)?
        {
            accepted.push(r);
        }
    }
    Ok(accepted)
}

pub(super) fn separated_source_rows_agree(best: &[f64; 3], height: f64) -> bool {
    use barcode_research_core::numeric::usize_f64;
    (0..3).any(|i| {
        ((i + 1)..3).any(|j| {
            best[i] < 0.
                && best[i] * best[i] >= 0.65
                && best[j] < 0.
                && best[j] * best[j] >= 0.65
                && usize_f64(j - i) * 0.3 * height >= 2.
        })
    })
}
