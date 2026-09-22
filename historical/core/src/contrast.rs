//! Experimental bounded local contrast on normalized 512-sample line evidence.
//! Sliding extrema use monotone queues: O(n), no per-call allocation. This does
//! not add information or relax EAN acceptance; unsupported contrast stays raw.
#![forbid(unsafe_code)]
use crate::profile::{Error, LEN};
pub struct Normalizer {
    output: [f32; LEN],
    minimum: [usize; LEN],
    maximum: [usize; LEN],
}
impl Default for Normalizer {
    fn default() -> Self {
        Self {
            output: [0.; LEN],
            minimum: [0; LEN],
            maximum: [0; LEN],
        }
    }
}
impl Normalizer {
    /// # Errors
    /// Returns `Length` unless the profile has 512 samples, or `Value` for non-finite samples or values outside [0, 1].
    pub fn normalize(&mut self, p: &[f32]) -> Result<&[f32; LEN], Error> {
        if p.len() != LEN {
            return Err(Error::Length);
        }
        if p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err(Error::Value);
        }
        let (mut min_head, mut min_tail, mut max_head, mut max_tail, mut next) = (0, 0, 0, 0, 0);
        for i in 0..LEN {
            let right = (i + 16).min(LEN - 1);
            let left = i.saturating_sub(16);
            while next <= right {
                while min_tail > min_head && p[self.minimum[min_tail - 1]] >= p[next] {
                    min_tail -= 1;
                }
                while max_tail > max_head && p[self.maximum[max_tail - 1]] <= p[next] {
                    max_tail -= 1;
                }
                self.minimum[min_tail] = next;
                min_tail += 1;
                self.maximum[max_tail] = next;
                max_tail += 1;
                next += 1;
            }
            while self.minimum[min_head] < left {
                min_head += 1;
            }
            while self.maximum[max_head] < left {
                max_head += 1;
            }
            let lo = p[self.minimum[min_head]];
            let range = p[self.maximum[max_head]] - lo;
            self.output[i] = if range >= 0.25 {
                ((p[i] - lo) / range).clamp(0., 1.)
            } else {
                p[i]
            };
        }
        Ok(&self.output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn matches_naive_windows_and_preserves_input() {
        let p = std::array::from_fn::<_, LEN, _>(|i| {
            crate::numeric::usize_f32((i * 37 + i / 7) % 257) / 256.
        });
        let original = p;
        let mut n = Normalizer::default();
        let out = n.normalize(&p).unwrap();
        for i in 0..LEN {
            let w = &p[i.saturating_sub(16)..=(i + 16).min(LEN - 1)];
            let lo = w.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = w.iter().copied().fold(0., f32::max);
            let expected = if hi - lo >= 0.25 {
                (p[i] - lo) / (hi - lo)
            } else {
                p[i]
            };
            assert_eq!(out[i], expected);
        }
        assert_eq!(p, original);
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn invalid_and_low_contrast() {
        let mut n = Normalizer::default();
        assert!(n.normalize(&[0.; 511]).is_err());
        for v in [f32::NAN, f32::INFINITY, -1., 2.] {
            assert!(n.normalize(&[v; LEN]).is_err());
        }
        assert_eq!(n.normalize(&[0.4; LEN]).unwrap(), &[0.4; LEN]);
    }
    #[test]
    fn shaded_independent_ean_and_checksum_rejection() {
        let bits=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        let make = |bits: &[u8]| {
            std::array::from_fn::<_, LEN, _>(|i| {
                let base = 0.5 - 0.45 * crate::numeric::usize_f32(i) / 511.;
                let m = ((i).cast_signed() - 64) / 4;
                let dark = i >= 64 && m < 95 && bits[(m).cast_unsigned()] == b'1';
                base + if dark { 0.4 } else { 0. }
            })
        };
        let p = make(bits);
        assert!(crate::profile::decode(&p).unwrap().is_none());
        let mut n = Normalizer::default();
        let got = crate::profile::decode(n.normalize(&p).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(got.digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        let mut bad = *bits;
        bad[85..92].copy_from_slice(b"1001000");
        assert!(crate::profile::decode(n.normalize(&make(&bad)).unwrap())
            .unwrap()
            .is_none());
    }
}
