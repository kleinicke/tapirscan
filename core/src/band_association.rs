//! Bounded final observation association. Value IDs represent equality of actual
//! decoder outputs, never expected labels. Image evidence is necessary for every
//! merge. Ambiguous matches, invalid geometry and exhausted budgets retain reads.
#![forbid(unsafe_code)]
use crate::{
    continuity::Continuity,
    sampling::{Error, ImageView},
    scan::Quad,
};
pub const MAX_OBSERVATIONS: usize = 128;
const MAX_CHECKS: usize = 128;
#[derive(Clone, Copy)]
pub struct Observation {
    pub value_id: u32,
    pub polygon: Quad,
    pub is_anchor: bool,
}
#[derive(Default)]
pub struct Associator {
    continuity: Continuity,
    parents: Vec<usize>,
    anchors: Vec<usize>,
    checks: usize,
    invalid: usize,
}
impl Associator {
    pub fn parents(&self) -> &[usize] {
        &self.parents
    }
    pub fn checks(&self) -> usize {
        self.checks
    }
    pub fn invalid(&self) -> usize {
        self.invalid
    }
    pub fn associate(
        &mut self,
        image: ImageView<'_>,
        reads: &[Observation],
    ) -> Result<&[usize], Error> {
        if reads.len() > MAX_OBSERVATIONS {
            return Err(Error::Parameters);
        }
        self.parents
            .try_reserve(reads.len().saturating_sub(self.parents.len()))
            .map_err(|_| Error::Allocation)?;
        self.anchors
            .try_reserve(reads.len().saturating_sub(self.anchors.len()))
            .map_err(|_| Error::Allocation)?;
        self.parents.clear();
        self.parents.extend(0..reads.len());
        self.anchors.clear();
        self.checks = 0;
        self.invalid = 0;
        // Establish canonical independently decoded anchors before considering weaker
        // reader geometry. Never use a merged non-anchor as a new bridge.
        for phase in [true, false] {
            for (i, read) in reads.iter().enumerate() {
                if read.is_anchor != phase {
                    continue;
                }
                let mut matched = None;
                let mut ambiguous = false;
                let mut exhausted = false;
                for &a in &self.anchors {
                    if reads[a].value_id != read.value_id {
                        continue;
                    }
                    if self.checks >= MAX_CHECKS {
                        exhausted = true;
                        break;
                    }
                    self.checks += 1;
                    match self
                        .continuity
                        .check_strict(image, reads[a].polygon, read.polygon)
                    {
                        Ok(e) if e.supported => {
                            if matched.is_some() {
                                ambiguous = true;
                            } else {
                                matched = Some(a);
                            }
                        }
                        Err(_) => self.invalid += 1,
                        _ => {}
                    }
                }
                if !ambiguous && !exhausted {
                    if let Some(a) = matched {
                        self.parents[i] = a;
                        continue;
                    }
                }
                if phase {
                    self.anchors.push(i);
                }
            }
        }
        Ok(&self.parents)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn band(y: f64) -> Quad {
        [[20., y], [400., y], [400., y + 4.], [20., y + 4.]]
    }
    fn image() -> Vec<u8> {
        let mut p = vec![255; 420 * 240];
        for y in 10..230 {
            for x in 20..400 {
                if (x / 4) % 3 == 0 || (x / 4) % 7 == 0 {
                    p[y * 420 + x] = 0;
                }
            }
        }
        p
    }
    fn r(y: f64, id: u32, anchor: bool) -> Observation {
        Observation {
            value_id: id,
            polygon: band(y),
            is_anchor: anchor,
        }
    }
    #[test]
    fn duplicates_gap_and_conflict() {
        let mut p = image();
        for y in 110..120 {
            p[y * 420..(y + 1) * 420].fill(255);
        }
        let mut a = Associator::default();
        let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
        assert_eq!(
            a.associate(
                im,
                &[
                    r(20., 1, true),
                    r(160., 1, true),
                    r(30., 1, false),
                    r(170., 1, false),
                    r(30., 2, false)
                ]
            )
            .unwrap(),
            &[0, 1, 0, 1, 4]
        );
    }
    #[test]
    fn anchor_first_no_transitive_bridge_and_reuse() {
        let p = image();
        let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
        let mut a = Associator::default();
        assert_eq!(
            a.associate(im, &[r(30., 1, false), r(20., 1, true), r(40., 1, true)])
                .unwrap(),
            &[1, 1, 1]
        );
        let mut invalid = r(20., 1, false);
        invalid.polygon[0][0] = f64::NAN;
        assert_eq!(
            a.associate(im, &[r(20., 1, true), invalid]).unwrap(),
            &[0, 1]
        );
        assert_eq!(a.invalid(), 1);
        assert!(a.associate(im, &vec![r(20., 1, true); 129]).is_err());
    }
    #[test]
    fn budget_preserves_unchecked_observations() {
        let p = image();
        let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
        let mut a = Associator::default();
        let mut reads = vec![r(20., 1, true); 128];
        for (i, r) in reads.iter_mut().enumerate() {
            r.polygon[0][0] = f64::NAN;
            r.value_id = 1;
            r.polygon[0][1] = i as f64;
        }
        assert_eq!(
            a.associate(im, &reads).unwrap(),
            &(0..128).collect::<Vec<_>>()
        );
        assert_eq!(a.checks(), 128);
    }
    #[test]
    fn one_pixel_gap_at_fractional_phases_is_not_identity() {
        for gap in [1, 2, 3, 10] {
            for phase in [0., 0.25, 0.5, 0.75] {
                let mut p = image();
                for y in 110..110 + gap {
                    p[y * 420..(y + 1) * 420].fill(255);
                }
                let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
                let mut a = Associator::default();
                assert_eq!(
                    a.associate(im, &[r(20. + phase, 1, true), r(160. + phase, 1, true)])
                        .unwrap(),
                    &[0, 1]
                );
            }
        }
    }
}
