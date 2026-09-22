//! Conditional source conditioning and original-preserving physical reconciliation.
//! No expected text, labels, reference decoders or replacement of the normal arm.
use crate::{
    experiment::{AssociationBudget, Work},
    frame::{Barcode, Frame},
    multi_scan::Policy,
    sampling::ImageView,
};
const TILE: usize = 256;
#[derive(Default, Debug)]
pub struct Conditioning {
    pub sampled_pixels: usize,
    pub eligible_tiles: usize,
    pub tiles: usize,
    pub global_span: usize,
    pub global: bool,
    pub changed_pixels: usize,
}
impl Conditioning {
    pub fn json(&self) -> String {
        format!("{{\"sampledPixels\":{},\"eligibleTiles\":{},\"tiles\":{},\"globalSpan\":{},\"global\":{},\"changedPixels\":{}}}",self.sampled_pixels,self.eligible_tiles,self.tiles,self.global_span,self.global,self.changed_pixels)
    }
}
fn bounds(h: &[usize; 256]) -> (usize, usize) {
    let n = h.iter().sum::<usize>();
    let mut sum = 0;
    let (mut lo, mut hi) = (0, 255);
    for (i, &v) in h.iter().enumerate() {
        sum += v;
        if sum >= n.div_ceil(100) {
            lo = i;
            break;
        }
    }
    sum = 0;
    for (i, &v) in h.iter().enumerate() {
        sum += v;
        if sum >= (n * 99).div_ceil(100) {
            hi = i;
            break;
        }
    }
    (lo, hi)
}
/// A coarse contrast map is independent of localized proposals. One global LUT
/// is used for compressed frames; otherwise bilinear local gains rescue dim
/// regions alongside bright symbols. Isotropic noise alone cannot enable a tile.
pub fn condition(im: ImageView<'_>) -> (Option<Vec<u8>>, Conditioning) {
    let (nx, ny) = (im.width.div_ceil(TILE), im.height.div_ceil(TILE));
    let mut e = Conditioning {
        tiles: nx * ny,
        ..Default::default()
    };
    let mut maps = vec![(1f64, 0f64); nx * ny];
    let mut global = [0usize; 256];
    for ty in 0..ny {
        for tx in 0..nx {
            let mut hist = [0usize; 256];
            let (mut xx, mut yy, mut xy) = (0., 0., 0.);
            let mut n = 0;
            for y in (ty * TILE..((ty + 1) * TILE).min(im.height)).step_by(4) {
                let shift = (y / 4) % 4;
                for x in (tx * TILE + shift..((tx + 1) * TILE).min(im.width)).step_by(4) {
                    let i = y * im.stride + x * im.channels;
                    let channels = im.channels.min(3);
                    for &v in &im.data[i..i + channels] {
                        hist[v as usize] += 1;
                        global[v as usize] += 1;
                    }
                    let v = im.gray(x as f64, y as f64);
                    let dx = im.gray((x + 1) as f64, y as f64) - v;
                    let dy = im.gray(x as f64, (y + 1) as f64) - v;
                    xx += dx * dx;
                    yy += dy * dy;
                    xy += dx * dy;
                    n += 1;
                }
            }
            e.sampled_pixels += n;
            let (lo, hi) = bounds(&hist);
            let span = hi.saturating_sub(lo);
            let energy = xx + yy;
            let coherence = ((xx - yy).powi(2) + 4. * xy * xy).sqrt() / energy.max(1.);
            if (8..=96).contains(&span) && energy / n.max(1) as f64 >= 1. && coherence >= 0.55 {
                let gain = (255. / span as f64).min(16.);
                maps[ty * nx + tx] = (gain, -(lo as f64) * gain);
                e.eligible_tiles += 1;
            }
        }
    }
    let (lo, hi) = bounds(&global);
    e.global_span = hi.saturating_sub(lo);
    if e.eligible_tiles == 0 {
        return (None, e);
    }
    e.global = (8..=96).contains(&e.global_span);
    let gain = (255. / e.global_span.max(1) as f64).min(16.);
    let mut out = im.data.to_vec();
    // Exact LUT for the global affine arm: each byte value uses the same f64
    // expression once, rather than millions of repeated rounding operations.
    if e.global {
        let lut: [u8; 256] = std::array::from_fn(|v| {
            (v as f64 * gain - (lo as f64) * gain)
                .round()
                .clamp(0., 255.) as u8
        });
        for y in 0..im.height {
            for x in 0..im.width {
                let i = y * im.stride + x * im.channels;
                let mut changed = false;
                for c in 0..im.channels.min(3) {
                    let v = lut[im.data[i + c] as usize];
                    changed |= v != im.data[i + c];
                    out[i + c] = v;
                }
                e.changed_pixels += usize::from(changed);
            }
        }
        return (
            if e.changed_pixels == 0 {
                None
            } else {
                Some(out)
            },
            e,
        );
    }
    let columns: Vec<_> = (0..im.width)
        .map(|x| {
            let fx = ((x as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (nx - 1) as f64);
            let xa = fx.floor() as usize;
            (xa, (xa + 1).min(nx - 1), fx - xa as f64)
        })
        .collect();

    for y in 0..im.height {
        let fy = ((y as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (ny - 1) as f64);
        let ya = fy.floor() as usize;
        let yb = (ya + 1).min(ny - 1);
        let dy = fy - ya as f64;
        for x in 0..im.width {
            let (xa, xb, dx) = columns[x];
            if [
                maps[ya * nx + xa],
                maps[ya * nx + xb],
                maps[yb * nx + xa],
                maps[yb * nx + xb],
            ]
            .iter()
            .all(|&m| m == (1., 0.))
            {
                continue;
            }
            let (g, b) = {
                let mut m = (0., 0.);
                for (ix, iy, w) in [
                    (xa, ya, (1. - dx) * (1. - dy)),
                    (xb, ya, dx * (1. - dy)),
                    (xa, yb, (1. - dx) * dy),
                    (xb, yb, dx * dy),
                ] {
                    m.0 += w * maps[iy * nx + ix].0;
                    m.1 += w * maps[iy * nx + ix].1;
                }
                m
            };
            let i = y * im.stride + x * im.channels;
            let mut changed = false;
            for c in 0..im.channels.min(3) {
                let v = (im.data[i + c] as f64 * g + b).round().clamp(0., 255.) as u8;
                changed |= v != im.data[i + c];
                out[i + c] = v;
            }
            e.changed_pixels += usize::from(changed);
        }
    }
    if e.changed_pixels == 0 {
        (None, e)
    } else {
        (Some(out), e)
    }
}
#[derive(Default, Debug)]
pub struct MergeWork {
    pub comparisons: usize,
    pub redundant: usize,
    pub conflicting: usize,
    pub ambiguous: usize,
    pub added: usize,
    pub source_pixels: usize,
    pub truncated: bool,
}
impl MergeWork {
    pub fn json(&self) -> String {
        format!("{{\"comparisons\":{},\"redundant\":{},\"conflicting\":{},\"ambiguous\":{},\"added\":{},\"sourcePixels\":{},\"truncated\":{}}}",self.comparisons,self.redundant,self.conflicting,self.ambiguous,self.added,self.source_pixels,self.truncated)
    }
}
/// No transitive grouping: each rescue output is compared with all original
/// accepted outputs, then existing rescue additions. Original outputs are never
/// deleted/replaced. Ambiguous bridges or conflicting rescues remain raw only.
pub fn merge(
    mut normal: Frame,
    mut rescue: Frame,
    im: ImageView<'_>,
    policy: Policy,
) -> (Frame, MergeWork) {
    let mut work = MergeWork::default();
    let mut budget = AssociationBudget {
        checks_left: policy.max_association_checks,
        pixels_left: policy.max_association_pixels,
    };
    let mut counter = Work::default();
    let offset = normal.candidates.len();
    for c in &mut rescue.candidates {
        c.index += offset;
    }
    let mut additions: Vec<Barcode> = vec![];
    'outputs: for mut r in rescue.barcodes {
        let mut same = 0;
        let mut conflict = false;
        for b in normal.barcodes.iter().chain(additions.iter()) {
            if !budget.check(&mut counter) {
                work.truncated = true;
                break 'outputs;
            }
            let mut overlap = crate::frame::same_space(r.detection.polygon, b.detection.polygon);
            if !overlap && r.detection.digits == b.detection.digits {
                if let Some((a, b)) =
                    crate::identity::gap_edges(r.detection.polygon, b.detection.polygon)
                {
                    overlap = crate::identity::connected(im, a, b, &mut budget, &mut counter);
                    if counter.association_truncated > 0 {
                        work.truncated = true;
                        break 'outputs;
                    }
                }
            }
            if overlap {
                same += 1;
                conflict |= r.detection.digits != b.detection.digits;
            }
        }
        if conflict {
            work.conflicting += 1;
            continue;
        }
        if same > 1 {
            work.ambiguous += 1;
            continue;
        }
        if same == 1 {
            work.redundant += 1;
            continue;
        }
        if normal.barcodes.len() + additions.len() >= policy.max_results {
            work.truncated = true;
            break;
        }
        for i in &mut r.candidate_indices {
            *i += offset
        }
        additions.push(r);
        work.added += 1;
    }
    work.comparisons = counter.association_checks;
    work.source_pixels = counter.continuity_samples;
    normal.barcodes.extend(additions);
    normal.candidates.extend(rescue.candidates);
    normal.unfinished |=
        rescue.unfinished || work.truncated || work.conflicting > 0 || work.ambiguous > 0;
    (normal, work)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{experiment::Detection, frame::ReconciliationWork, scan::Quad};
    fn q(x: f64, y: f64, w: f64, h: f64) -> Quad {
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }
    fn frame(qs: &[Quad], digit: u8) -> Frame {
        Frame {
            candidates: vec![],
            barcodes: qs
                .iter()
                .map(|&polygon| Barcode {
                    detection: Detection {
                        digits: [digit; 13],
                        polygon,
                        support: 2,
                        axis: 0,
                    },
                    candidate_indices: vec![],
                })
                .collect(),
            reconciliation: ReconciliationWork::default(),
            unfinished: false,
        }
    }
    #[test]
    fn original_precedence_distinct_equal_text_and_bridge() {
        let data = vec![255; 600 * 300];
        let im = ImageView::new(&data, 600, 300, 1, 600).unwrap();
        let a = q(10., 20., 100., 30.);
        let b = q(210., 20., 100., 30.);
        let (f, w) = merge(
            frame(&[a, b], 1),
            frame(&[a, q(10., 120., 100., 30.)], 1),
            im,
            Policy::default(),
        );
        assert_eq!(f.barcodes.len(), 3);
        assert_eq!(f.barcodes[0].detection.polygon, a);
        assert_eq!(w.redundant, 1);
        let (f, w) = merge(
            frame(&[a, b], 1),
            frame(&[q(10., 20., 300., 30.)], 1),
            im,
            Policy::default(),
        );
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(w.ambiguous, 1);
        let (f, w) = merge(frame(&[a], 1), frame(&[a], 2), im, Policy::default());
        assert_eq!(f.barcodes[0].detection.digits, [1; 13]);
        assert_eq!(w.conflicting, 1);
        assert!(f.unfinished);
    }
    #[test]
    fn rotated_disjoint_boxes_are_not_aabb_matches() {
        let a = [[0., 0.], [100., 100.], [102., 98.], [2., -2.]];
        let b = [[0., 8.], [100., 108.], [102., 106.], [2., 6.]];
        assert!(!crate::frame::same_space(a, b));
        let mut reversed = a;
        reversed.reverse();
        assert!(crate::frame::same_space(a, reversed));
    }
    #[test]
    fn contrast_noise_and_mixed_brightness() {
        let (w, h) = (512, 256);
        let mut data = vec![255; w * h];
        for y in 0..h {
            for x in 0..w {
                data[y * w + x] = if x < 256 {
                    if x % 8 < 4 {
                        0
                    } else {
                        255
                    }
                } else {
                    if x % 8 < 4 {
                        4
                    } else {
                        24
                    }
                };
            }
        }
        let (out, e) = condition(ImageView::new(&data, w, h, 1, w).unwrap());
        assert!(!e.global);
        assert_eq!(e.eligible_tiles, 1);
        let out = out.unwrap();
        assert_eq!(out[30], data[30]);
        assert!(out[500] > data[500]);
        let mut seed = 17u32;
        for v in &mut data {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = ((seed >> 24) % 25) as u8;
        }
        assert!(condition(ImageView::new(&data, w, h, 1, w).unwrap())
            .0
            .is_none());
        data.fill(10);
        assert!(condition(ImageView::new(&data, w, h, 1, w).unwrap())
            .0
            .is_none());
    }
    #[test]
    fn close_equal_text_blank_gap_remains_two() {
        let (w, h) = (256, 80);
        let mut pixels = vec![255; w * h];
        for y in 10..=55 {
            if y > 30 && y < 35 {
                continue;
            }
            for x in 10..=200 {
                pixels[y * w + x] = if (x / 2) % 3 == 0 { 0 } else { 255 };
            }
        }
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        let a = q(10., 10., 190., 20.);
        let b = q(10., 35., 190., 20.);
        let (f, w) = merge(frame(&[a], 1), frame(&[b], 1), im, Policy::default());
        assert_eq!(f.barcodes.len(), 2);
        assert!(w.source_pixels > 0);
        assert_eq!(w.redundant, 0);
    }
    #[test]
    fn original_and_candidate_identity_survive_merge_exhaustion() {
        let pixels = vec![255; 600 * 300];
        let im = ImageView::new(&pixels, 600, 300, 1, 600).unwrap();
        let a = q(10., 10., 100., 30.);
        let b = q(210., 10., 100., 30.);
        let with_candidate = |q| {
            let mut f = frame(&[q], 1);
            f.barcodes[0].candidate_indices = vec![0];
            f.candidates.push(crate::experiment::Candidate {
                index: 0,
                coverage: q,
                observations: vec![],
                detections: vec![f.barcodes[0].detection.clone()],
                work: Work::default(),
                ms: 0.,
                error: false,
            });
            f
        };
        let (f, w) = merge(with_candidate(a), with_candidate(b), im, Policy::default());
        assert_eq!(w.added, 1);
        assert_eq!(f.barcodes[0].candidate_indices, vec![0]);
        assert_eq!(f.barcodes[1].candidate_indices, vec![1]);
        assert_eq!(f.candidates[1].index, 1);
        let (f, w) = merge(
            with_candidate(a),
            with_candidate(b),
            im,
            Policy {
                max_association_checks: 0,
                ..Default::default()
            },
        );
        assert!(w.truncated && f.unfinished);
        assert_eq!(f.barcodes.len(), 1);
        assert_eq!(f.barcodes[0].detection.polygon, a);
        assert_eq!(f.candidates.len(), 2);
    }
}
