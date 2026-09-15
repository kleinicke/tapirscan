//! Project-owned prime-field error/erasure correction for PDF417.
fn add(a: usize, b: usize) -> usize {
    (a + b) % 929
}
fn sub(a: usize, b: usize) -> usize {
    (a + 929 - b) % 929
}
fn mul(a: usize, b: usize) -> usize {
    a * b % 929
}
fn pow(mut x: usize, mut n: usize) -> usize {
    let mut out = 1;
    while n > 0 {
        if n & 1 != 0 {
            out = mul(out, x);
        }
        x = mul(x, x);
        n >>= 1;
    }
    out
}
fn div(a: usize, b: usize) -> usize {
    mul(a, pow(b, 927))
}
fn exp(n: usize) -> usize {
    pow(3, n % 928)
}
fn eval(p: &[usize], x: usize) -> usize {
    p.iter().fold(0, |a, &b| add(mul(a, x), b))
}
pub fn correct(code: &mut [usize], ecc: usize, erasures: &[usize]) -> Option<usize> {
    if code.len() > 928
        || ecc >= code.len()
        || erasures.len() > ecc
        || code.iter().any(|&n| n >= 929)
    {
        return None;
    }
    let synd: Vec<_> = (1..=ecc).map(|i| eval(code, exp(i))).collect();
    if synd.iter().all(|&x| x == 0) {
        return Some(0);
    }
    let mut reduced = synd.clone();
    for &i in erasures {
        if i >= code.len() {
            return None;
        }
        let x = exp(code.len() - 1 - i);
        for j in 0..reduced.len() - 1 {
            reduced[j] = sub(reduced[j + 1], mul(x, reduced[j]));
        }
        reduced.pop();
    }
    let nsynd = reduced.len();
    let mut c = vec![0; nsynd + 1];
    c[0] = 1;
    let mut b = c.clone();
    let mut degree = 0;
    let mut shift = 1;
    let mut previous = 1;
    for n in 0..nsynd {
        let mut discrepancy = reduced[n];
        for i in 1..=degree {
            discrepancy = add(discrepancy, mul(c[i], reduced[n - i]));
        }
        if discrepancy == 0 {
            shift += 1;
            continue;
        }
        let old = c.clone();
        let factor = div(discrepancy, previous);
        for j in 0..=nsynd - shift {
            c[j + shift] = sub(c[j + shift], mul(factor, b[j]));
        }
        if 2 * degree <= n {
            degree = n + 1 - degree;
            b = old;
            previous = discrepancy;
            shift = 1;
        } else {
            shift += 1;
        }
    }
    if degree * 2 + erasures.len() > ecc {
        return None;
    }
    let mut locations: Vec<usize> = (0..code.len())
        .filter(|&i| {
            let x = exp(928 - (code.len() - 1 - i) % 928);
            c[..=degree].iter().rev().fold(0, |a, &b| add(mul(a, x), b)) == 0
        })
        .collect();
    if locations.len() != degree {
        return None;
    }
    locations.extend_from_slice(erasures);
    locations.sort_unstable();
    locations.dedup();
    let n = locations.len();
    if n == 0 || n > ecc {
        return None;
    }
    // Forney magnitudes: S starts at alpha^1, so no additional field
    // power is required. Polynomial coefficients here are ascending.
    let mut locator = vec![1];
    for &i in &locations {
        let x = exp(code.len() - 1 - i);
        let mut next = vec![0; locator.len() + 1];
        for (j, &v) in locator.iter().enumerate() {
            next[j] = add(next[j], v);
            next[j + 1] = sub(next[j + 1], mul(v, x));
        }
        locator = next;
    }
    let mut omega = vec![0; n];
    for j in 0..n {
        for k in 0..=j {
            omega[j] = add(omega[j], mul(locator[k], synd[j - k]));
        }
    }
    let derivative: Vec<_> = locator
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, &v)| mul(i, v))
        .collect();
    for &i in &locations {
        let z = exp(928 - (code.len() - 1 - i) % 928);
        let denominator = derivative.iter().rev().fold(0, |a, &b| add(mul(a, z), b));
        if denominator == 0 {
            return None;
        }
        let numerator = omega.iter().rev().fold(0, |a, &b| add(mul(a, z), b));
        code[i] = add(code[i], div(numerator, denominator));
    }
    if (1..=ecc).any(|i| eval(code, exp(i)) != 0) {
        return None;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(data: &[usize], ecc: usize) -> Vec<usize> {
        let mut generator = vec![1];
        for i in 1..=ecc {
            let mut next = vec![0; generator.len() + 1];
            for (j, &v) in generator.iter().enumerate() {
                next[j] = add(next[j], v);
                next[j + 1] = sub(next[j + 1], mul(v, exp(i)));
            }
            generator = next;
        }
        let mut work = data.to_vec();
        work.resize(data.len() + ecc, 0);
        for i in 0..data.len() {
            let factor = work[i];
            for j in 0..generator.len() {
                work[i + j] = sub(work[i + j], mul(factor, generator[j]));
            }
        }
        let mut out = data.to_vec();
        out.extend(work[data.len()..].iter().map(|&v| sub(0, v)));
        out
    }
    #[test]
    fn restores_errors_and_erasures_to_capacity() {
        for ecc in [2, 4, 8, 16, 32] {
            let data: Vec<_> = (0..50).map(|i| (i * 73 + 19) % 929).collect();
            let original = encode(&data, ecc);
            assert!((1..=ecc).all(|i| eval(&original, exp(i)) == 0));
            for erased in 0..=ecc {
                let errors = (ecc - erased) / 2;
                let mut damaged = original.clone();
                let positions: Vec<_> = (0..erased).map(|i| i * 2).collect();
                for &i in &positions {
                    damaged[i] = 0;
                }
                for i in 0..errors {
                    let at = damaged.len() - 1 - i;
                    damaged[at] = add(damaged[at], i + 57);
                }
                assert!(
                    correct(&mut damaged, ecc, &positions).is_some(),
                    "ecc={ecc}, erasures={erased}"
                );
                assert_eq!(damaged, original);
            }
        }
    }
}
