//! Bounded short-code localization crops; all decoding uses original pixels.
use crate::{geometry::overlap_quads, read::Read, Error, Image, Proposal, Quad};
use barcode_research_core::{
    fast_profile::Sampler,
    numeric::{f64_usize, usize_f64},
    stripes::Detector,
};

/// Two nonoverlapping source regions, at most 32,768 localization pixels each.
/// Existing owners and independent source-row confirmation remain authoritative.
pub(crate) fn recover(
    localizer: &mut Detector,
    image: Image<'_>,
    proposals: &[Proposal],
    mask: u32,
    sampler: &mut Sampler,
    reads: &mut Vec<Read>,
) -> Result<(), Error> {
    let mut selected: Vec<_> = proposals
        .iter()
        .filter(|p| {
            let q = p.polygon;
            let width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
            let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
            p.score >= 0.8
                && width >= height * 1.5
                && width >= 24.
                && !reads.iter().any(|r| overlap_quads(&q, &r.polygon).0 > 0.)
        })
        .copied()
        .collect();
    selected.sort_by(|a, b| {
        let width = |p: &crate::Proposal| {
            (p.polygon[1][0] - p.polygon[0][0]).hypot(p.polygon[1][1] - p.polygon[0][1])
        };
        width(b)
            .total_cmp(&width(a))
            .then_with(|| b.score.total_cmp(&a.score))
    });
    let mut unique = Vec::<crate::Proposal>::new();
    for p in selected {
        if unique
            .iter()
            .any(|old| overlap_quads(&p.polygon, &old.polygon).0 >= 0.5)
        {
            continue;
        }
        unique.push(p);
        if unique.len() >= 2 {
            break;
        }
    }
    let mut remaining = 65_536;
    for p in unique {
        let q = p.polygon;
        let refined = crop_proposals(localizer, image, q, &mut remaining)?;
        let extra = crate::fast_linear::recover_proposals_contrast(
            image,
            &refined,
            mask & 12,
            sampler,
            0.0,
        )?
        .0;
        let extra = crate::linear_duplicates::merge(extra, image);
        for r in extra {
            let read_width =
                (r.polygon[1][0] - r.polygon[0][0]).hypot(r.polygon[1][1] - r.polygon[0][1]);
            let proposal_width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
            if (r.format == "UPCE" && read_width < proposal_width * 0.6)
                || r.support < 4
                || overlap_quads(&r.polygon, &q).0 < 0.3
                || reads.iter().any(|old| {
                    overlap_quads(&r.polygon, &old.polygon).0 > 0.
                        || (old.format == r.format && old.text == r.text)
                })
            {
                continue;
            }
            reads.push(r);
        }
    }
    Ok(())
}

fn crop_proposals(
    localizer: &mut Detector,
    image: Image<'_>,
    quad: Quad,
    remaining: &mut usize,
) -> Result<Vec<Proposal>, Error> {
    let xmin = quad.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
    let xmax = quad.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max);
    let ymin = quad.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
    let ymax = quad.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max);
    let dx = (xmax - xmin) * 0.25 + 4.;
    let dy = (ymax - ymin) * 0.3 + 4.;
    let x = f64_usize((xmin - dx).floor().clamp(0., usize_f64(image.width)));
    let y = f64_usize((ymin - dy).floor().clamp(0., usize_f64(image.height)));
    let right = f64_usize((xmax + dx).ceil().clamp(0., usize_f64(image.width)));
    let bottom = f64_usize((ymax + dy).ceil().clamp(0., usize_f64(image.height)));
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    if width < 3 || height < 3 || width * height > 1_048_576 || *remaining < 256 {
        return Ok(Vec::new());
    }
    let mut step = 1;
    while (width / step) * (height / step) > (*remaining).min(32_768) {
        step += 1;
    }
    let crop_width = width / step;
    let crop_height = height / step;
    if crop_width < 3 || crop_height < 3 {
        return Ok(Vec::new());
    }
    *remaining -= crop_width * crop_height;
    let mut reduced = Vec::new();
    let im = if step == 1 {
        crate::ImageView::new(
            &image.data[y * image.stride + x * image.channels..],
            width,
            height,
            image.channels,
            image.stride,
        )?
    } else {
        reduced.reserve(crop_width * crop_height * image.channels);
        for row in 0..crop_height {
            for col in 0..crop_width {
                let offset = (y + row * step + step / 2) * image.stride
                    + (x + col * step + step / 2) * image.channels;
                reduced.extend_from_slice(&image.data[offset..offset + image.channels]);
            }
        }
        crate::ImageView::new(
            &reduced,
            crop_width,
            crop_height,
            image.channels,
            crop_width * image.channels,
        )?
    };
    let found = localizer.detect(im)?;
    let offset = if step == 1 { 0. } else { usize_f64(step / 2) };
    let refined: Vec<_> = found
        .proposals
        .into_iter()
        .take(2)
        .map(|p| crate::Proposal {
            polygon: p.polygon.map(|v| {
                [
                    v[0] * usize_f64(step) + offset + usize_f64(x),
                    v[1] * usize_f64(step) + offset + usize_f64(y),
                ]
            }),
            score: p.score,
        })
        .collect();
    Ok(refined)
}

#[cfg(test)]
mod tests {
    use super::{crop_proposals, Detector, Image};
    #[test]
    fn localization_honors_per_crop_and_remaining_pixel_limits() {
        let pixels = vec![255; 600 * 400];
        let image = Image {
            data: &pixels,
            width: 600,
            height: 400,
            channels: 1,
            stride: 600,
        };
        let quad = [[0., 0.], [600., 0.], [600., 400.], [0., 400.]];
        let mut detector = Detector::default();
        for budget in [0, 255, 256, 32_768, 65_536] {
            let mut remaining = budget;
            assert!(crop_proposals(&mut detector, image, quad, &mut remaining)
                .unwrap()
                .is_empty());
            assert!(remaining <= budget);
            assert!(budget - remaining <= 32_768);
            if budget < 256 {
                assert_eq!(remaining, budget);
            }
        }
    }
    #[test]
    fn oversized_source_regions_do_not_spend_localization_budget() {
        let pixels = vec![255; 1600 * 800];
        let image = Image {
            data: &pixels,
            width: 1600,
            height: 800,
            channels: 1,
            stride: 1600,
        };
        let quad = [[0., 0.], [1600., 0.], [1600., 800.], [0., 800.]];
        let mut remaining = 65_536;
        assert!(
            crop_proposals(&mut Detector::default(), image, quad, &mut remaining)
                .unwrap()
                .is_empty()
        );
        assert_eq!(remaining, 65_536);
    }
}
