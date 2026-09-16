//! Blur-aware intensity matching for structurally recognized EAN-8 candidates.
//! Each digit is selected independently before the checksum is checked. This
//! does not search for a checksum-compatible replacement of an uncertain digit.
use std::sync::OnceLock;

const SAMPLES: usize = 42;
struct Template {
    digit: u8,
    values: [f32; SAMPLES],
}

// Integrate the normal density with Simpson's rule when constructing the small
// cached template bank. No decoder, learned weights or external tables are used.
fn normal_cdf(x: f32) -> f32 {
    if x.abs() >= 6. {
        return if x < 0. { 0. } else { 1. };
    }
    let step = x / 16.;
    let density = |v: f32| (-0.5 * v * v).exp() / (2. * std::f32::consts::PI).sqrt();
    let mut sum = density(0.) + density(x);
    for i in 1..16 {
        sum += density(step * crate::numeric::f64_f32(f64::from(i)))
            * if i % 2 == 0 { 2. } else { 4. };
    }
    0.5 + sum * step / 3.
}
fn normalize(values: &mut [f32; SAMPLES]) -> f32 {
    let mean = values.iter().sum::<f32>() / crate::numeric::usize_f32(SAMPLES);
    let mut variance = 0.;
    for v in values.iter_mut() {
        *v -= mean;
        variance += *v * *v;
    }
    let norm = variance.sqrt().max(1e-6);
    for v in values.iter_mut() {
        *v /= norm;
    }
    variance
}
fn templates() -> &'static [Template] {
    static BANK: OnceLock<Vec<Template>> = OnceLock::new();
    BANK.get_or_init(|| {
        let mut bank = Vec::new();
        for sigma in [0.3, 0.45, 0.6, 0.8, 1.] {
            for phase in [-0.2, 0., 0.2] {
                for (digit, widths) in crate::linear::DIGITS.iter().enumerate() {
                    let mut values = std::array::from_fn(|i| {
                        let x = (crate::numeric::usize_f32(i) + 0.5) / 6. - phase;
                        let mut v = 1. - normal_cdf(x / sigma) - normal_cdf((x - 7.) / sigma);
                        let mut edge = 0.;
                        for (j, &width) in widths.iter().take(3).enumerate() {
                            edge += f32::from(width);
                            v += normal_cdf((x - edge) / sigma) * if j % 2 == 0 { 1. } else { -1. };
                        }
                        v
                    });
                    normalize(&mut values);
                    bank.push(Template {
                        digit: (digit).to_le_bytes()[0],
                        values,
                    });
                }
            }
        }
        bank
    })
}
pub(crate) fn decode(
    row: &[u8],
    runs: &[f32],
    start: usize,
    reverse: bool,
) -> Option<(String, f32)> {
    if start + 43 > runs.len() || row.len() < 2 {
        return None;
    }
    let mut digits = Vec::with_capacity(8);
    let mut worst = 0_f32;
    for position in 0..8 {
        let at = start + 3 + position * 4 + if position >= 4 { 5 } else { 0 };
        let left = runs[..at].iter().sum::<f32>();
        let length = runs[at..at + 4].iter().sum::<f32>();
        if length < 7. {
            return None;
        }
        let pixel = |i: usize| f32::from(row[if reverse { row.len() - 1 - i } else { i }]);
        let mut values = std::array::from_fn(|i| {
            let x = (left
                + length * (crate::numeric::usize_f32(i) + 0.5)
                    / crate::numeric::usize_f32(SAMPLES))
            .clamp(0., crate::numeric::usize_f32(row.len() - 2));
            let lo = crate::numeric::f32_usize(x.floor());
            let fraction = x - crate::numeric::usize_f32(lo);
            (pixel(lo) * (1. - fraction) + pixel(lo + 1) * fraction)
                * if position < 4 { -1. } else { 1. }
        });
        if normalize(&mut values) < crate::numeric::usize_f32(SAMPLES) * 16. {
            return None;
        }
        let mut scores = [-1_f32; 10];
        for template in templates() {
            let score = values
                .iter()
                .zip(template.values)
                .map(|(a, b)| a * b)
                .sum::<f32>();
            scores[template.digit as usize] = scores[template.digit as usize].max(score);
        }
        let mut order: Vec<_> = (0..10).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
        let best = scores[order[0]];
        if best < 0.92 || best - scores[order[1]] < 0.015 {
            return None;
        }
        worst = worst.max(1. - best);
        digits.push((order[0]).to_le_bytes()[0]);
    }
    crate::linear::checksum(&digits)
        .then(|| (digits.iter().map(|d| char::from(b'0' + d)).collect(), worst))
}

#[cfg(test)]
mod tests {
    fn blurred_symbol(text: &str) -> (Vec<u8>, Vec<f32>) {
        let mut widths = vec![12, 1, 1, 1];
        for (i, digit) in text.bytes().enumerate() {
            if i == 4 {
                widths.extend([1, 1, 1, 1, 1]);
            }
            widths.extend(crate::linear::DIGITS[(digit - b'0') as usize].map(usize::from));
        }
        widths.extend([1, 1, 1, 12]);
        let pixels: Vec<f32> = widths
            .iter()
            .enumerate()
            .flat_map(|(i, &w)| std::iter::repeat_n(if i % 2 == 0 { 220. } else { 40. }, w * 3))
            .collect();
        let row = (0..pixels.len())
            .map(|i| {
                let mut sum = 0.;
                let mut mass = 0.;
                for d in -5_isize..=5 {
                    let weight =
                        (-crate::numeric::isize_f32(d).powi(2) / (2. * 1.5_f32.powi(2))).exp();
                    sum += pixels[i.saturating_add_signed(d).min(pixels.len() - 1)] * weight;
                    mass += weight;
                }
                crate::numeric::f32_u8((sum / mass).round())
            })
            .collect();
        (
            row,
            widths
                .iter()
                .map(|&w| crate::numeric::usize_f32(w * 3))
                .collect(),
        )
    }
    #[test]
    fn blurred_digits_decode_in_both_directions() {
        for text in ["96385074", "12345670"] {
            let (mut row, widths) = blurred_symbol(text);
            assert_eq!(
                super::decode(&row, &widths, 1, false)
                    .map(|r| r.0)
                    .as_deref(),
                Some(text)
            );
            row.reverse();
            assert_eq!(
                super::decode(&row, &widths, 1, true)
                    .map(|r| r.0)
                    .as_deref(),
                Some(text)
            );
        }
    }
    #[test]
    fn a_bad_checksum_is_not_repaired_by_guessing_digits() {
        let (row, widths) = blurred_symbol("96385075");
        assert!(super::decode(&row, &widths, 1, false).is_none());
        assert!(super::decode(&vec![180; row.len()], &widths, 1, false).is_none());
    }
}
