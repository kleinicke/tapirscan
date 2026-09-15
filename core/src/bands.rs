//! Experimental multiple EAN read bands in a supplied region, not a detector.
//! Every axis/path is attempted; same text is joined only with source continuity.
#![forbid(unsafe_code)]
use crate::{
    continuity::Continuity,
    profile,
    sampling::{Error, ImageView, Path, Sampler},
    scan::{project, transform, PathEvidence, Quad, Read},
};
pub const PATHS: usize = 32;
pub const MAX_READS: usize = 8;
#[derive(Default)]
pub struct BandScanner {
    sampler: Sampler,
    continuity: Continuity,
    reads: Vec<Read>,
    attempts: usize,
    truncated: bool,
}
#[derive(Clone, Copy)]
struct Group {
    read: profile::Read,
    first: PathEvidence,
    last: PathEvidence,
    count: usize,
}
impl BandScanner {
    pub fn reads(&self) -> &[Read] {
        &self.reads
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// Original region remains coverage, including invalid/undecoded/truncated cases.
    /// Returned bands are evidence, not proof that all physical symbols were found.
    /// Work:64 path decodes, at most62 bounded continuity checks; <=8 output bands.
    pub fn scan(&mut self, image: ImageView<'_>, quad: Quad) -> Result<&[Read], Error> {
        self.reads.clear();
        self.attempts = 0;
        self.truncated = false;
        let m = transform(quad)?;
        self.reads
            .try_reserve(MAX_READS)
            .map_err(|_| Error::Allocation)?;
        for axis in 0..2 {
            let mut group: Option<Group> = None;
            for i in 0..PATHS {
                let fraction = (i as f64 + 0.5) / PATHS as f64;
                self.attempts += 1;
                let Some(p) = self.sampler.sample(
                    image,
                    m,
                    Path {
                        axis,
                        fraction,
                        curve: 0.,
                        margin: 0.15,
                    },
                )?
                else {
                    continue;
                };
                let Some(read) = profile::decode(p).map_err(|_| Error::Parameters)? else {
                    continue;
                };
                let (left, right) = if read.reversed {
                    (511. - f64::from(read.right), 511. - f64::from(read.left))
                } else {
                    (f64::from(read.left), f64::from(read.right))
                };
                let path = PathEvidence {
                    fraction,
                    left,
                    right,
                };
                let joined = if let Some(g) = group {
                    if g.read.digits != read.digits
                        || right.min(g.last.right) - left.max(g.last.left)
                            <= 0.8 * (right - left).max(g.last.right - g.last.left)
                    {
                        false
                    } else {
                        // Tiny source-supported anchor around the earlier path is used only by
                        // continuity; output polygons use the actual decoded path endpoints.
                        let eps = 1e-5;
                        let a = [
                            project(m, axis, g.last.left, g.last.fraction - eps)?,
                            project(m, axis, g.last.right, g.last.fraction - eps)?,
                            project(m, axis, g.last.right, g.last.fraction + eps)?,
                            project(m, axis, g.last.left, g.last.fraction + eps)?,
                        ];
                        let l = project(m, axis, left, fraction)?;
                        let r = project(m, axis, right, fraction)?;
                        self.continuity
                            .check(image, a, [l, r, r, l])
                            .map(|e| e.supported)
                            .unwrap_or(false)
                    }
                } else {
                    false
                };
                if joined {
                    let g = group.as_mut().unwrap();
                    g.last = path;
                    g.count += 1;
                    g.read = read;
                } else {
                    if let Some(g) = group.take() {
                        self.finish(m, axis, g)?;
                    }
                    group = Some(Group {
                        read,
                        first: path,
                        last: path,
                        count: 1,
                    });
                }
            }
            if let Some(g) = group {
                self.finish(m, axis, g)?;
            }
        }
        Ok(&self.reads)
    }
    fn finish(
        &mut self,
        m: crate::sampling::Transform,
        axis: usize,
        g: Group,
    ) -> Result<(), Error> {
        if g.count < 2 {
            return Ok(());
        }
        if self.reads.len() == MAX_READS {
            self.truncated = true;
            return Ok(());
        }
        let polygon = [
            project(m, axis, g.first.left, g.first.fraction)?,
            project(m, axis, g.first.right, g.first.fraction)?,
            project(m, axis, g.last.right, g.last.fraction)?,
            project(m, axis, g.last.left, g.last.fraction)?,
        ];
        transform(polygon)?;
        self.reads.push(Read {
            run_width: false,
            contrast_normalized: false,
            digits: g.read.digits,
            polygon,
            cost: g.read.cost,
            gap: g.read.gap,
            axis,
            paths: [g.first, g.last],
        });
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn image(split: bool) -> Vec<u8> {
        let mut p = vec![255; 420 * 400];
        for x in 0..380 {
            if BITS.as_bytes()[x / 4] == b'1' {
                for y in 20..380 {
                    if !split || !(160..240).contains(&y) {
                        p[y * 420 + x + 20] = 0;
                    }
                }
            }
        }
        p
    }
    #[test]
    fn separates_equal_codes_and_joins_continuous_bars() {
        let q = [[20., 20.], [400., 20.], [400., 380.], [20., 380.]];
        let mut s = BandScanner::default();
        for (split, count) in [(false, 1), (true, 2)] {
            let p = image(split);
            let r = s
                .scan(ImageView::new(&p, 420, 400, 1, 420).unwrap(), q)
                .unwrap();
            assert_eq!(r.len(), count);
            assert!(r
                .iter()
                .all(|r| r.digits == [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]));
            assert_eq!(s.attempts(), 64);
            assert!(!s.truncated());
        }
    }
    #[test]
    fn rotated_and_invalid_clear_previous_results() {
        let p = image(true);
        let mut rotated = vec![255; p.len()];
        for y in 0..400 {
            for x in 0..420 {
                rotated[x * 400 + 399 - y] = p[y * 420 + x];
            }
        }
        let mut s = BandScanner::default();
        let im = ImageView::new(&rotated, 400, 420, 1, 400).unwrap();
        let q = [[379., 20.], [379., 400.], [19., 400.], [19., 20.]];
        assert_eq!(s.scan(im, q).unwrap().len(), 2);
        assert!(s.scan(im, [[0.; 2]; 4]).is_err());
        assert!(s.reads().is_empty());
        assert_eq!(s.attempts(), 0);
    }
    #[test]
    fn output_cap_reports_truncation_after_all_paths() {
        let mut p = vec![255; 420 * 640];
        for y in 0..640 {
            if (y / 20) % 3 == 2 {
                continue;
            }
            for x in 0..380 {
                if BITS.as_bytes()[x / 4] == b'1' {
                    p[y * 420 + x + 20] = 0;
                }
            }
        }
        let mut s = BandScanner::default();
        assert_eq!(
            s.scan(
                ImageView::new(&p, 420, 640, 1, 420).unwrap(),
                [[20., 0.], [400., 0.], [400., 640.], [20., 640.]]
            )
            .unwrap()
            .len(),
            8
        );
        assert!(s.truncated());
        assert_eq!(s.attempts(), 64);
    }
}
