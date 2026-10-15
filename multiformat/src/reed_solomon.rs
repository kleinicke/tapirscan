//! Project-owned GF(256) error correction: Berlekamp-Massey locator,
//! Chien root search and a small linear solve for error magnitudes.
pub struct Field {
    exp: [u8; 512],
    log: [usize; 256],
}
impl Field {
    #[must_use]
    pub fn new(polynomial: u16) -> Self {
        let mut f = Self {
            exp: [0; 512],
            log: [0; 256],
        };
        let mut x = 1u16;
        for i in 0..255 {
            f.exp[i] = (x).to_le_bytes()[0];
            f.log[x as usize] = i;
            x <<= 1;
            if x & 256 != 0 {
                x ^= polynomial;
            }
        }
        for i in 255..512 {
            f.exp[i] = f.exp[i - 255];
        }
        f
    }
    #[must_use]
    pub fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            self.exp[self.log[a as usize] + self.log[b as usize]]
        }
    }
    #[must_use]
    /// # Panics
    ///
    /// Panics when `b` is zero because division by zero is undefined in the field.
    pub fn div(&self, a: u8, b: u8) -> u8 {
        assert_ne!(b, 0);
        if a == 0 {
            0
        } else {
            self.exp[self.log[a as usize] + 255 - self.log[b as usize]]
        }
    }
    fn power(&self, n: usize) -> u8 {
        self.exp[n % 255]
    }
    fn eval(&self, p: &[u8], x: u8) -> u8 {
        p.iter().fold(0, |a, &b| self.mul(a, x) ^ b)
    }
    #[must_use]
    pub fn correct(&self, code: &mut [u8], ecc: usize, base: usize) -> Option<usize> {
        if ecc == 0 || ecc >= code.len() || code.len() > 255 {
            return None;
        }
        let synd: Vec<u8> = (0..ecc)
            .map(|i| self.eval(code, self.power(i + base)))
            .collect();
        if synd.iter().all(|&x| x == 0) {
            return Some(0);
        }
        let mut c = vec![0; ecc + 1];
        c[0] = 1;
        let mut b = c.clone();
        let mut degree = 0;
        let mut shift = 1;
        let mut previous = 1;
        for n in 0..ecc {
            let mut discrepancy = synd[n];
            for i in 1..=degree {
                discrepancy ^= self.mul(c[i], synd[n - i]);
            }
            if discrepancy == 0 {
                shift += 1;
                continue;
            }
            let old = c.clone();
            let factor = self.div(discrepancy, previous);
            for j in 0..=ecc - shift {
                c[j + shift] ^= self.mul(factor, b[j]);
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
        if degree == 0 || degree * 2 > ecc {
            return None;
        }
        let locations: Vec<usize> = (0..code.len())
            .filter(|&i| {
                let x = self.power(255 - (code.len() - 1 - i) % 255);
                c[..=degree]
                    .iter()
                    .rev()
                    .fold(0, |a, &b| self.mul(a, x) ^ b)
                    == 0
            })
            .collect();
        if locations.len() != degree {
            return None;
        }
        let mut a = vec![vec![0; degree + 1]; degree];
        for j in 0..degree {
            for (k, &i) in locations.iter().enumerate() {
                a[j][k] = self.power((code.len() - 1 - i) * (j + base));
            }
            a[j][degree] = synd[j];
        }
        for col in 0..degree {
            let pivot = (col..degree).find(|&row| a[row][col] != 0)?;
            a.swap(col, pivot);
            let value = a[col][col];
            for x in &mut a[col][col..=degree] {
                *x = self.div(*x, value);
            }
            let (before, pivot_and_after) = a.split_at_mut(col);
            let (pivot, after) = pivot_and_after.split_first_mut()?;
            for row in before.iter_mut().chain(after.iter_mut()) {
                let factor = row[col];
                for (value, &coefficient) in row[col..=degree].iter_mut().zip(&pivot[col..=degree])
                {
                    *value ^= self.mul(factor, coefficient);
                }
            }
        }
        for (k, &i) in locations.iter().enumerate() {
            code[i] ^= a[k][degree];
        }
        if (0..ecc).any(|i| self.eval(code, self.power(i + base)) != 0) {
            return None;
        }
        Some(degree)
    }
}

impl Field {
    /// Correct errors and erasures: `erasures` are code positions known to be
    /// unreliable. Each erasure costs one parity symbol instead of two, so
    /// `2 * errors + erasures <= ecc - 2` symbols are corrected; the two spare
    /// symbols guard against miscorrection. Returns the number of changed symbols.
    #[must_use]
    pub fn correct_with_erasures(
        &self,
        code: &mut [u8],
        ecc: usize,
        base: usize,
        erasures: &[usize],
    ) -> Option<usize> {
        let length = code.len();
        let erased = erasures.len();
        if ecc == 0
            || ecc >= length
            || length > 255
            || erased > ecc
            || erasures.iter().any(|&i| i >= length)
        {
            return None;
        }
        let synd: Vec<u8> = (0..ecc)
            .map(|i| self.eval(code, self.power(i + base)))
            .collect();
        if synd.iter().all(|&x| x == 0) {
            return Some(0);
        }
        // Erasure locator: product of (1 + X_k x), X_k = alpha^(length - 1 - k).
        let mut locator = vec![0_u8; ecc + 1];
        locator[0] = 1;
        for &k in erasures {
            let x = self.power(length - 1 - k);
            for j in (1..=ecc).rev() {
                locator[j] ^= self.mul(locator[j - 1], x);
            }
        }
        let mut previous_locator = locator.clone();
        let mut degree = erased;
        let mut shift = 1;
        let mut previous = 1;
        for r in erased..ecc {
            let mut discrepancy = synd[r];
            for i in 1..=degree.min(r) {
                discrepancy ^= self.mul(locator[i], synd[r - i]);
            }
            if discrepancy == 0 {
                shift += 1;
                continue;
            }
            let old = locator.clone();
            let factor = self.div(discrepancy, previous);
            for j in 0..=ecc - shift {
                locator[j + shift] ^= self.mul(factor, previous_locator[j]);
            }
            if 2 * degree <= r + erased {
                degree = r + 1 + erased - degree;
                previous_locator = old;
                previous = discrepancy;
                shift = 1;
            } else {
                shift += 1;
            }
        }
        // Keep two parity symbols unused: they detect most miscorrections, which
        // erasures otherwise make likelier.
        if degree == 0 || degree > ecc || 2 * degree + 2 > ecc + erased {
            return None;
        }
        let locations: Vec<usize> = (0..length)
            .filter(|&i| {
                let x = self.power(255 - (length - 1 - i) % 255);
                locator[..=degree]
                    .iter()
                    .rev()
                    .fold(0, |a, &b| self.mul(a, x) ^ b)
                    == 0
            })
            .collect();
        if locations.len() != degree || erasures.iter().any(|k| !locations.contains(k)) {
            return None;
        }
        let mut a = vec![vec![0; degree + 1]; degree];
        for j in 0..degree {
            for (k, &i) in locations.iter().enumerate() {
                a[j][k] = self.power((length - 1 - i) * (j + base));
            }
            a[j][degree] = synd[j];
        }
        for col in 0..degree {
            let pivot = (col..degree).find(|&row| a[row][col] != 0)?;
            a.swap(col, pivot);
            let value = a[col][col];
            for x in &mut a[col][col..=degree] {
                *x = self.div(*x, value);
            }
            let (before, pivot_and_after) = a.split_at_mut(col);
            let (pivot, after) = pivot_and_after.split_first_mut()?;
            for row in before.iter_mut().chain(after.iter_mut()) {
                let factor = row[col];
                for (value, &coefficient) in row[col..=degree].iter_mut().zip(&pivot[col..=degree])
                {
                    *value ^= self.mul(factor, coefficient);
                }
            }
        }
        let mut changed = 0;
        for (k, &i) in locations.iter().enumerate() {
            changed += usize::from(a[k][degree] != 0);
            code[i] ^= a[k][degree];
        }
        if (0..ecc).any(|i| self.eval(code, self.power(i + base)) != 0) {
            return None;
        }
        Some(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correct_known_qr_block() {
        // QR tutorial numeric payload 01234567, version 1-M; 16 data +10 parity.
        let original = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 165, 36, 212,
            193, 237, 54, 199, 135, 44, 85,
        ];
        let f = Field::new(0x11d);
        let mut d = original;
        assert_eq!(f.correct(&mut d, 10, 0), Some(0));
        for i in [0, 7, 13, 20, 25] {
            d[i] ^= 0x25;
        }
        assert_eq!(f.correct(&mut d, 10, 0), Some(5));
        assert_eq!(d, original);
    }
    #[test]
    fn erasures_extend_the_correction_capacity() {
        let original = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 165, 36, 212,
            193, 237, 54, 199, 135, 44, 85,
        ];
        let f = Field::new(0x11d);
        // Ten parity symbols, two kept spare: four erasures plus two errors, six
        // wrong symbols, beyond the five errors of plain decoding.
        let mut d = original;
        let erased = [1, 9, 15, 23];
        for &i in &erased {
            d[i] ^= 0x5a;
        }
        d[6] ^= 0x11;
        d[20] ^= 0x80;
        assert_eq!(f.correct(&mut d.clone(), 10, 0), None);
        assert_eq!(f.correct_with_erasures(&mut d, 10, 0, &erased), Some(6));
        assert_eq!(d, original);
        // Erased but intact symbols cost capacity without changing.
        let mut d = original;
        d[3] ^= 0x01;
        assert_eq!(f.correct_with_erasures(&mut d, 10, 0, &[0, 3, 7]), Some(1));
        assert_eq!(d, original);
        // Eight erasures use the usable parity; one more error is too many.
        let all: Vec<usize> = (0..8).collect();
        let mut d = original;
        for &i in &all {
            d[i] ^= 0x33;
        }
        assert_eq!(f.correct_with_erasures(&mut d, 10, 0, &all), Some(8));
        assert_eq!(d, original);
        let mut d = original;
        for &i in &all {
            d[i] ^= 0x33;
        }
        d[12] ^= 0x01;
        assert_eq!(f.correct_with_erasures(&mut d, 10, 0, &all), None);
    }
}
