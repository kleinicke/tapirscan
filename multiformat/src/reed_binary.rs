//! Project-owned binary extension field decoder for Aztec word sizes 4–12.
pub struct Field {
    exp: Vec<u16>,
    log: Vec<usize>,
    order: usize,
}
impl Field {
    #[must_use]
    pub fn new(bits: usize, poly: usize) -> Self {
        let size = 1usize << bits;
        let mut f = Self {
            exp: vec![0; size * 2],
            log: vec![0; size],
            order: size - 1,
        };
        let mut x = 1;
        for i in 0..size - 1 {
            f.exp[i] = crate::numeric::usize_u16(x);
            f.log[x] = i;
            x <<= 1;
            if x & size != 0 {
                x ^= poly;
            }
        }
        for i in size - 1..size * 2 {
            f.exp[i] = f.exp[i - (size - 1)];
        }
        f
    }
    fn mul(&self, a: u16, b: u16) -> u16 {
        if a == 0 || b == 0 {
            0
        } else {
            self.exp[self.log[a as usize] + self.log[b as usize]]
        }
    }
    fn div(&self, a: u16, b: u16) -> u16 {
        if a == 0 {
            0
        } else {
            self.exp[self.log[a as usize] + self.order - self.log[b as usize]]
        }
    }
    fn power(&self, n: usize) -> u16 {
        self.exp[n % self.order]
    }
    fn eval(&self, p: &[u16], x: u16) -> u16 {
        p.iter().fold(0, |a, &b| self.mul(a, x) ^ b)
    }
    pub fn correct(&self, code: &mut [u16], ecc: usize, base: usize) -> Option<usize> {
        if ecc == 0
            || ecc >= code.len()
            || code.len() > self.order
            || code.iter().any(|&x| x as usize > self.order)
        {
            return None;
        }
        let synd: Vec<u16> = (0..ecc)
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
        if degree == 0 || 2 * degree > ecc {
            return None;
        }
        let locations: Vec<usize> = (0..code.len())
            .filter(|&i| {
                let x = self.power(self.order - (code.len() - 1 - i) % self.order);
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
            a.swap(pivot, col);
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
