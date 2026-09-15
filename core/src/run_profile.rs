//! Bounded run likelihood on a normalized512sample dark-is-one profile.
#![forbid(unsafe_code)]
#[derive(Clone, Copy, Debug)]
pub struct Read {
    pub digits: [u8; 13],
    pub left: f64,
    pub right: f64,
    pub cost: f32,
    pub gap: f32,
}
pub fn decode(p: &[f32]) -> Result<Option<Read>, crate::profile::Error> {
    if p.len() != 512 {
        return Err(crate::profile::Error::Length);
    }
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err(crate::profile::Error::Value);
    }
    let mut runs = [(0usize, 0usize, false); 512];
    let (mut count, mut start, mut black) = (0, 0, p[0] >= 0.5);
    for i in 1..=512 {
        let next = i < 512 && p[i] >= 0.5;
        if i == 512 || next != black {
            runs[count] = (start, i, black);
            count += 1;
            start = i;
            black = next;
        }
    }
    let mut accepted: Option<Read> = None;
    for i in 0..count.saturating_sub(60) {
        let r = &runs[i..i + 61];
        if !r[1].2 {
            continue;
        }
        let (left, right) = (r[1].0, r[59].1);
        let module = (right - left) as f64 / 95.;
        if module < 0.8
            || ((r[0].1 - r[0].0) as f64) < 7. * module
            || ((r[60].1 - r[60].0) as f64) < 7. * module
        {
            continue;
        }
        let mut widths = [0.; 59];
        for j in 0..59 {
            widths[j] = (r[j + 1].1 - r[j + 1].0) as f32;
        }
        for reverse in [false, true] {
            if reverse {
                widths.reverse();
            }
            let Some(e) = crate::run_ean::decode_evidence(&widths) else {
                continue;
            };
            let current = Read {
                digits: e.digits,
                left: left as f64 - 0.5,
                right: right as f64 - 0.5,
                cost: e.cost,
                gap: e.gap,
            };
            if accepted.is_some_and(|a| a.digits != current.digits) {
                return Ok(None);
            }
            if accepted.is_none_or(|a| current.cost < a.cost) {
                accepted = Some(current);
            }
        }
    }
    Ok(accepted)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widths_and_invalid_input() {
        let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let bits = crate::ean::encode(&d);
        let mut p = [0.; 512];
        for i in 0..380 {
            p[50 + i] = bits[i / 4];
        }
        assert_eq!(decode(&p).unwrap().unwrap().digits, d);
        p.reverse();
        assert_eq!(decode(&p).unwrap().unwrap().digits, d);
        assert!(decode(&p[..511]).is_err());
        p[0] = f32::NAN;
        assert!(decode(&p).is_err());
    }
}
