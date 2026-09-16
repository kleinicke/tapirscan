//! Experimental assembly of equal-valued row fragments using source continuity.
//! Every link is checked against pixels; a missing decode is not itself a gap.
#![forbid(unsafe_code)]
use crate::{
    continuity::Continuity,
    row_scan::{Candidate, MAX_RESULTS},
    sampling::{Error, ImageView},
};
const MAX_CHECKS: usize = 512;
#[derive(Default)]
pub struct Assembler {
    output: Vec<Candidate>,
    parents: Vec<usize>,
    order: Vec<usize>,
    sample_bands: Vec<Option<[[f64; 2]; 4]>>,
    continuity: Continuity,
    pub checks: usize,
    pub merged: usize,
    pub exhausted: bool,
}
fn root(parents: &[usize], mut i: usize) -> usize {
    while parents[i] != i {
        i = parents[i];
    }
    i
}
fn centre(c: &Candidate) -> f64 {
    if c.axis == 0 {
        (c.polygon[0][1] + c.polygon[2][1]) * 0.5
    } else {
        (c.polygon[0][0] + c.polygon[2][0]) * 0.5
    }
}
fn bounds(c: &Candidate) -> (f64, f64, f64, f64) {
    let mut b = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in c.polygon {
        b.0 = b.0.min(p[0]);
        b.1 = b.1.min(p[1]);
        b.2 = b.2.max(p[0]);
        b.3 = b.3.max(p[1]);
    }
    b
}
// Row observations describe pixel edges. Only for sampling, intersect their
// footprint with pixel-center bounds; never change the returned source geometry.
// The generic continuity checker remains strict and never samples outside image.
fn sampling_band(image: ImageView<'_>, q: [[f64; 2]; 4]) -> Result<Option<[[f64; 2]; 4]>, Error> {
    if q.iter().any(|p| {
        !p[0].is_finite()
            || !p[1].is_finite()
            || p[0] < -0.5
            || p[1] < -0.5
            || p[0] > crate::numeric::usize_f64(image.width) - 0.5
            || p[1] > crate::numeric::usize_f64(image.height) - 0.5
    }) {
        return Err(Error::Geometry);
    }
    let band = q.map(|p| {
        [
            p[0].clamp(0., crate::numeric::usize_f64(image.width - 1)),
            p[1].clamp(0., crate::numeric::usize_f64(image.height - 1)),
        ]
    });
    Ok(crate::scan::transform(band).ok().map(|_| band))
}
impl Assembler {
    /// # Errors
    /// Returns `Parameters` for excessive or invalid candidates and `Geometry` for invalid projected groups; propagates sampling errors.
    #[expect(
        clippy::too_many_lines,
        reason = "Candidate union and geometric aggregation share the same parents and stable candidate ordering."
    )]
    pub fn assemble(
        &mut self,
        image: ImageView<'_>,
        input: &[Candidate],
    ) -> Result<&[Candidate], Error> {
        self.output.clear();
        self.parents.clear();
        self.order.clear();
        self.sample_bands.clear();
        self.checks = 0;
        self.merged = 0;
        self.exhausted = false;
        if input.len() > MAX_RESULTS {
            return Err(Error::Parameters);
        }
        for c in input {
            crate::scan::transform(c.polygon)?;
            if c.support == 0
                || c.support > image.width.max(image.height)
                || c.axis > 1
                || c.digits.is_some_and(|d| {
                    d.iter().any(|&first_root| first_root > 9) || !crate::ean::checksum(&d)
                })
            {
                return Err(Error::Parameters);
            }
        }
        self.output
            .try_reserve(input.len())
            .map_err(|_| Error::Allocation)?;
        self.parents
            .try_reserve(input.len())
            .map_err(|_| Error::Allocation)?;
        self.order
            .try_reserve(input.len())
            .map_err(|_| Error::Allocation)?;
        self.sample_bands
            .try_reserve(input.len())
            .map_err(|_| Error::Allocation)?;
        for c in input {
            self.sample_bands.push(sampling_band(image, c.polygon)?);
        }
        self.parents.extend(0..input.len());
        self.order
            .extend((0..input.len()).filter(|&i| input[i].digits.is_some()));
        self.order.sort_by(|&a, &b| {
            input[a]
                .axis
                .cmp(&input[b].axis)
                .then_with(|| centre(&input[a]).total_cmp(&centre(&input[b])))
                .then(a.cmp(&b))
        });
        for pos in 0..self.order.len() {
            let i = self.order[pos];
            let a = &input[i];
            let mut considered = 0;
            for back in (0..pos).rev() {
                let j = self.order[back];
                let b = &input[j];
                if b.axis != a.axis {
                    break;
                }
                if centre(a) - centre(b) > 512. {
                    break;
                }
                if a.digits != b.digits || root(&self.parents, i) == root(&self.parents, j) {
                    continue;
                }
                let (al, ar, bl, br) = if a.axis == 0 {
                    (
                        a.polygon[0][0],
                        a.polygon[1][0],
                        b.polygon[0][0],
                        b.polygon[1][0],
                    )
                } else {
                    (
                        a.polygon[0][1],
                        a.polygon[1][1],
                        b.polygon[0][1],
                        b.polygon[1][1],
                    )
                };
                if ar.min(br) - al.max(bl) < 0.9 * (ar.max(br) - al.min(bl)) {
                    continue;
                }
                let (Some(sample_a), Some(sample_b)) = (self.sample_bands[i], self.sample_bands[j])
                else {
                    continue;
                };
                if self.checks >= MAX_CHECKS {
                    self.exhausted = true;
                    break;
                }
                self.checks += 1;
                considered += 1;
                // Original fragments, never the growing union box, anchor each link.
                if self
                    .continuity
                    .check_strict(image, sample_b, sample_a)
                    .is_ok_and(|e| e.supported)
                {
                    let first_root = root(&self.parents, i);
                    let second_root = root(&self.parents, j);
                    self.parents[first_root.max(second_root)] = first_root.min(second_root);
                }
                if considered >= 4 {
                    break;
                }
            }
        }
        // Group membership is bounded/transitive pixel-supported connectivity. It is
        // experimental identity evidence, not a proof. All unlinked/undecoded survive.
        for i in 0..input.len() {
            if root(&self.parents, i) != i {
                continue;
            }
            let mut c = input[i].clone();
            let mut b = bounds(&c);
            c.fragments = 1;
            for (j, input_entry) in input.iter().enumerate().skip(i + 1) {
                if root(&self.parents, j) != i {
                    continue;
                }
                let quad = bounds(input_entry);
                b = (
                    b.0.min(quad.0),
                    b.1.min(quad.1),
                    b.2.max(quad.2),
                    b.3.max(quad.3),
                );
                c.support = c
                    .support
                    .checked_add(input_entry.support)
                    .ok_or(Error::Parameters)?;
                c.fragments += 1;
                self.merged += 1;
            }
            c.polygon = if c.axis == 0 {
                [[b.0, b.1], [b.2, b.1], [b.2, b.3], [b.0, b.3]]
            } else {
                [[b.0, b.1], [b.0, b.3], [b.2, b.3], [b.2, b.1]]
            };
            self.output.push(c);
        }
        self.output
            .sort_by_key(|c| (c.digits.is_none(), std::cmp::Reverse(c.support)));
        Ok(&self.output)
    }
}
