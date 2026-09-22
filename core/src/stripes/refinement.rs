//! Source-pixel refinement and final bounded proposal assembly.
#[cfg(all(test, feature = "mode-very-high"))]
use super::raster::secondary_gray;
#[cfg(test)]
use super::{detect, detect_with_observer, edge_weight};
use super::{distance, Edge, ImageView, Proposal, Result};
use super::{groups::Groups, proposals::Fitted};
pub(super) fn finish(im: ImageView<'_>, scale: f64, groups: Groups, fitted: Fitted) -> Result {
    let Groups {
        items,
        originals,
        eligible,
        merged_count,
        merge_checks,
        growth_limited,
        ..
    } = groups;
    let unrefined = items.len().saturating_sub(128);
    let refined = items.len().min(128);
    let Fitted {
        mut proposals,
        extra_proposals,
        extra_bands,
        angle_bands,
        rescued_bands,
        band_refined,
        band_unexamined,
    } = fitted;
    // Preserve the order of previously admitted parents and bands. New bands
    // use the same proposal cap and explicit omitted-work accounting.
    proposals.extend(rescued_bands);
    let original_proposals = proposals.clone();
    proposals.extend(extra_proposals);
    proposals.extend(extra_bands);
    let mut extent_omitted = 0;
    let (scheduled, extension_eligible, ineligible) = extent_plan(&proposals, true);
    let extent_eligible = extension_eligible;
    let extent_examined = scheduled.len();
    let extent_ineligible = ineligible;
    let extent_unexamined = extension_eligible - scheduled.len();
    let mut extended = Vec::new();
    for index in scheduled {
        let p = &proposals[index];
        if let Some(quad) = source_extent(im, p) {
            if extended.len() < 8 {
                extended.push(quad);
            } else {
                extent_omitted += 1;
            }
        }
    }
    // All original041 regions stay first. Additional growth proposals must
    // earn source-axis extension evidence before admission to the decoder.
    proposals = original_proposals;
    proposals.extend(extended);
    proposals.extend(angle_bands);
    let mut fragment_omitted = 0;
    let neighbor_index = proposals.len().min(24);

    let mut joined = Vec::new();
    for i in 0..neighbor_index {
        for j in i + 1..neighbor_index {
            if let Some(quad) = join_fragments(&proposals[i], &proposals[j], 16. / scale) {
                if proposals
                    .iter()
                    .take(24)
                    .any(|old| covers_fragment(old, &quad, 2. / scale))
                {
                    continue;
                }
                if joined.len() < 8 {
                    joined.push(quad);
                } else {
                    fragment_omitted += 1;
                }
            }
        }
    }

    proposals.extend(joined);
    // Direction correction does not require an additional extent change.
    // Keep established extent refinements first, then supplement angle-only
    // hypotheses under the same explicit proposal cap.
    let parent_count = proposals.len().min(24);

    let (refinements, angle_only) =
        source_refinements_replacing(im, &mut proposals[..parent_count]);

    fragment_omitted += refinements.len().saturating_sub(8) + angle_only.len().saturating_sub(8);
    proposals.extend(refinements.into_iter().take(8));
    proposals.extend(angle_only.into_iter().take(8));
    let valid = proposals.len();
    let omitted = valid.saturating_sub(24)
        + fragment_omitted
        + unrefined
        + extent_omitted
        + extent_unexamined
        + band_unexamined;
    proposals.truncate(24);
    Result {
        proposals,
        omitted,
        limited: omitted > 0 || originals > 64 || growth_limited,
        trace: [
            originals,
            eligible,
            merged_count,
            merge_checks,
            refined,
            unrefined,
            valid,
            extent_unexamined,
            extent_eligible,
            extent_examined,
            extent_ineligible,
            band_refined,
            band_unexamined,
        ],
    }
}

// Reject only dimensions safely outside the downstream extent interval.
// Rotation preserves lengths; a coordinate-scaled slack retains rounding-edge
// cases for the unchanged post-rotation check.
#[cfg(test)]
fn source_extent_possible(p: &Proposal) -> bool {
    let q = p.polygon;
    let w = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
    let h = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    let scale = q.iter().flatten().fold(1f64, |a, &v| a.max(v.abs()));
    let slack = scale * 1e-10;
    w >= 76. - slack && w <= 400. + slack && h >= 12. - slack && h <= 200. + slack
}
#[cfg(test)]
mod source_extent_precheck_tests {
    use super::*;
    #[test]
    fn retains_rotation_boundary_cases() {
        for w in [76., 400.] {
            for h in [12., 200.] {
                for origin in [0., 10000., 1_000_000.] {
                    for degree in 0..360 {
                        let a = f64::from(degree).to_radians();
                        let (c, s) = (a.cos(), a.sin());
                        let p = Proposal {
                            polygon: [[0., 0.], [w, 0.], [w, h], [0., h]]
                                .map(|[x, y]| [origin + c * x - s * y, origin + s * x + c * y]),
                            score: 1.,
                        };
                        assert!(source_extent_possible(&p));
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_clearly_ineligible_dimensions() {
        for (w, h) in [(40., 50.), (800., 50.), (200., 5.), (200., 300.)] {
            let p = Proposal {
                polygon: [[0., 0.], [w, 0.], [w, h], [0., h]],
                score: 1.,
            };
            assert!(!source_extent_possible(&p));
        }
    }
}

// Bounded native-pixel tensor inside admitted quads. Supplement only: prior
// candidates retain order, dimensions and all-visible decode attempts.
#[cfg(test)]
fn source_refinements(im: ImageView<'_>, parents: &[Proposal]) -> (Vec<Proposal>, Vec<Proposal>) {
    let (mut extents, mut angles) = (Vec::new(), Vec::new());
    for p in parents {
        if let Some(q) = source_angle(im, p) {
            if let Some(grown) = source_extent(im, &q) {
                extents.push(grown);
            } else {
                angles.push(q);
            }
        }
    }
    (extents, angles)
}

// Research ablation only: a highly coherent, modest correction can stand in
// for its angle-only parent. Extent growth remains supplemental, unchanged.

fn angle_replacement_qualified(parent: &Proposal, q: &Proposal) -> bool {
    let direction =
        |p: &Proposal| (p.polygon[1][1] - p.polygon[0][1]).atan2(p.polygon[1][0] - p.polygon[0][0]);
    q.score >= 0.9 && distance(direction(parent), direction(q)) <= 10f64.to_radians() + 1e-12
}

fn admit_source_refinement(
    parent: &mut Proposal,
    q: Proposal,
    grown: Option<Proposal>,
    extents: &mut Vec<Proposal>,
    angles: &mut Vec<Proposal>,
) {
    if let Some(grown) = grown {
        extents.push(grown);
    } else if angle_replacement_qualified(parent, &q) {
        *parent = q;
    } else {
        angles.push(q);
    }
}

fn source_refinements_replacing(
    im: ImageView<'_>,
    parents: &mut [Proposal],
) -> (Vec<Proposal>, Vec<Proposal>) {
    let (mut extents, mut angles) = (Vec::new(), Vec::new());
    for p in parents {
        // One source tensor, then one extent check for this parent, as in control.
        if let Some(q) = source_angle(im, p) {
            let grown = source_extent(im, &q);
            admit_source_refinement(p, q, grown, &mut extents, &mut angles);
        }
    }
    (extents, angles)
}
fn source_angle(im: ImageView<'_>, proposal: &Proposal) -> Option<Proposal> {
    let quad = proposal.polygon;
    let ex = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let ey = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let edge_width = ex[0].hypot(ex[1]);
    let edge_height = ey[0].hypot(ey[1]);
    if edge_width < 76. || edge_height < 12. {
        return None;
    }
    let gray = |x: usize, y: usize| {
        let i = y * im.stride + x * im.channels;
        if im.channels == 1 {
            f64::from(im.data[i])
        } else {
            (77. * f64::from(im.data[i])
                + 150. * f64::from(im.data[i + 1])
                + 29. * f64::from(im.data[i + 2]))
                / 256.
        }
    };
    let (mut xx, mut xy, mut yy, mut count) = (0., 0., 0., 0);
    for j in 0..32 {
        for i in 0..64 {
            let u = (f64::from(i) + 0.5) / 64.;
            let v = (f64::from(j) + 0.5) / 32.;
            let x = (quad[0][0] + ex[0] * u + ey[0] * v).floor();
            let y = (quad[0][1] + ex[1] * u + ey[1] * v).floor();
            if x < 1.
                || y < 1.
                || x >= crate::numeric::usize_f64(im.width - 1)
                || y >= crate::numeric::usize_f64(im.height - 1)
            {
                continue;
            }
            let (x, y) = (crate::numeric::f64_usize(x), crate::numeric::f64_usize(y));
            let dx = gray(x + 1, y) - gray(x - 1, y);
            let dy = gray(x, y + 1) - gray(x, y - 1);
            {
                #[cfg(feature = "mode-low")]
                {
                    if dx * dx + dy * dy < 18. * 18. {
                        continue;
                    }
                }
                #[cfg(any(
                    feature = "mode-medium",
                    feature = "mode-high",
                    feature = "mode-very-high"
                ))]
                {
                    if dx.hypot(dy) < 18. {
                        continue;
                    }
                }
            }

            xx += dx * dx;
            xy += dx * dy;
            yy += dy * dy;
            count += 1;
        }
    }
    #[cfg(feature = "mode-low")]
    let coherence = ((xx - yy) * (xx - yy) + 4. * xy * xy).sqrt() / (xx + yy + 1.);
    #[cfg(any(
        feature = "mode-medium",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let coherence = (xx - yy).hypot(2. * xy) / (xx + yy + 1.);

    if count < 80 || coherence < 0.6 {
        return None;
    }
    let a = 0.5 * (2. * xy).atan2(xx - yy);
    let parent = ex[1].atan2(ex[0]);
    let delta = (a - parent + std::f64::consts::PI / 2.).rem_euclid(std::f64::consts::PI)
        - std::f64::consts::PI / 2.;
    if delta.abs() < 0.5f64.to_radians() || delta.abs() > 30f64.to_radians() {
        return None;
    }
    let center = [
        (quad[0][0] + quad[2][0]) * 0.5,
        (quad[0][1] + quad[2][1]) * 0.5,
    ];
    let (cos_delta, sin_delta) = (delta.cos(), delta.sin());
    Some(Proposal {
        polygon: quad.map(|v| {
            let (x, y) = (v[0] - center[0], v[1] - center[1]);
            [
                center[0] + cos_delta * x - sin_delta * y,
                center[1] + sin_delta * x + cos_delta * y,
            ]
        }),
        score: coherence.min(0.99),
    })
}

// Supplemental fit from the edges inside a previously admitted dense band.
// Parent geometry and all prior candidates remain unchanged. No decoder/GT input.
#[expect(
    clippy::too_many_arguments,
    reason = "The fit consumes paired gradient planes, grid geometry and source scaling without owning or copying these buffers."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The bounded stripe pass shares ordered geometry refinement, source-evidence gates and work accounting."
)]
pub(super) fn refine_band_angle(
    edges: &[Edge],
    gx: &[f32],
    gy: &[f32],
    stride: usize,
    parent: f64,
    b: [f64; 4],
    sx: f64,
    sy: f64,
) -> Option<Proposal> {
    let (cos_angle, sin_angle) = (parent.cos(), parent.sin());
    let subset: Vec<_> = edges
        .iter()
        .filter(|e| {
            let v = -e.x * sin_angle + e.y * cos_angle;
            v >= b[2] && v <= b[3]
        })
        .collect();
    if subset.len() < 80 {
        return None;
    }
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for e in &subset {
        let i = crate::numeric::f64_usize(e.y.floor()) * stride
            + crate::numeric::f64_usize(e.x.floor());
        let (dx, dy) = (f64::from(gx[i]), f64::from(gy[i]));
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    #[cfg(feature = "mode-low")]
    let coherence = ((xx - yy) * (xx - yy) + 4. * xy * xy).sqrt() / (xx + yy + 1.);
    #[cfg(any(
        feature = "mode-medium",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let coherence = (xx - yy).hypot(2. * xy) / (xx + yy + 1.);

    if coherence < 0.60 {
        return None;
    }
    let initial = 0.5 * (2. * xy).atan2(xx - yy);
    if distance(initial, parent) > 15f64.to_radians() {
        return None;
    }
    let offset =
        crate::numeric::usize_f64(stride).hypot(crate::numeric::usize_f64(gx.len() / stride));
    let mut bins = vec![0f32; crate::numeric::f64_usize((offset * 4.).ceil()) + 8];
    let (mut best, mut angle) = (f64::NEG_INFINITY, initial);
    let mut best_step: i32 = 0;
    for step in -10..=10 {
        let a = initial + f64::from(step) * std::f64::consts::PI / 360.;
        let (cos_angle, sin_angle) = (a.cos(), a.sin());
        bins.fill(0.);
        for e in &subset {
            let p = (e.x * cos_angle + e.y * sin_angle + offset) * 2.;
            let i = crate::numeric::f64_usize(p.floor());
            let fraction = p - crate::numeric::usize_f64(i);
            if i + 1 < bins.len() {
                bins[i] = crate::numeric::f64_f32(f64::from(bins[i]) + e.weight * (1. - fraction));
                bins[i + 1] = crate::numeric::f64_f32(f64::from(bins[i + 1]) + e.weight * fraction);
            }
        }
        let score = bins
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>();
        if score > best {
            best = score;
            angle = a;
            best_step = step;
        }
    }
    // A search endpoint collapsing to the parent gives no supplementary geometry.
    // Preserve every accepted projection fit; only this censored search may use
    // the already-qualified local tensor. The same output and work caps apply.
    if best_step.abs() == 10 && distance(angle, parent) < 0.5f64.to_radians() {
        angle = initial;
    }
    if distance(angle, parent) < 0.5f64.to_radians() || distance(angle, parent) > 15f64.to_radians()
    {
        return None;
    }
    let (cos_angle, sin_angle) = (angle.cos(), angle.sin());
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for e in subset {
        let (u, v) = (
            e.x * cos_angle + e.y * sin_angle,
            -e.x * sin_angle + e.y * cos_angle,
        );
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    if u1 - u0 < 45. || v1 - v0 < 7. || !(0.65..=30.).contains(&((u1 - u0) / (v1 - v0))) {
        return None;
    }
    let at = |u: f64, v: f64| {
        [
            (u * cos_angle - v * sin_angle) * sx,
            (u * sin_angle + v * cos_angle) * sy,
        ]
    };
    Some(Proposal {
        polygon: [
            at(u0 - 1., v0),
            at(u1 + 1., v0),
            at(u1 + 1., v1),
            at(u0 - 1., v1),
        ],
        score: coherence.min(0.99),
    })
}

// Same source-dimension predicate as the pixel-level extent verifier. Completing
// this cheap check is not an unexamined search; it rejects no base proposals.
fn extent_dimensions(p: &Proposal) -> Option<(f64, f64)> {
    let q = &p.polygon;
    let w = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
    let h = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    ((76. ..=400.).contains(&w) && (12. ..=200.).contains(&h)).then_some((w, h))
}
fn extent_plan(proposals: &[Proposal], eligible_budget: bool) -> (Vec<usize>, usize, usize) {
    let eligible: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter_map(|(i, p)| extent_dimensions(p).map(|_| i))
        .collect();
    let scheduled = eligible
        .iter()
        .copied()
        .filter(|&i| eligible_budget || i < 24)
        .take(24)
        .collect();
    let count = eligible.len();
    (scheduled, count, proposals.len() - count)
}
/// Signed source-pixel multiline edge evidence, ported from061/extent.mts.
#[expect(
    clippy::too_many_lines,
    reason = "The bounded stripe pass shares ordered geometry refinement, source-evidence gates and work accounting."
)]
fn source_extent(im: ImageView<'_>, p: &Proposal) -> Option<Proposal> {
    let quad = &p.polygon;
    let ex = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let ey = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    #[cfg(any(
        feature = "mode-low",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let (width, _h) = extent_dimensions(p)?;
    #[cfg(feature = "mode-medium")]
    let (w, _h) = extent_dimensions(p)?;

    #[cfg(any(
        feature = "mode-low",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let margin = (width * 0.35).min(100.);
    #[cfg(feature = "mode-medium")]
    let margin = (w * 0.35).min(100.);

    let start = -crate::numeric::f64_isize(margin.ceil());
    #[cfg(any(
        feature = "mode-low",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let end = crate::numeric::f64_isize((width + margin).ceil());
    #[cfg(feature = "mode-medium")]
    let end = crate::numeric::f64_isize((w + margin).ceil());

    let count = (end - start + 1).cast_unsigned();
    let gray = |x: usize, y: usize| {
        let i = y * im.stride + x * im.channels;
        if im.channels == 1 {
            f64::from(im.data[i])
        } else {
            (77. * f64::from(im.data[i])
                + 150. * f64::from(im.data[i + 1])
                + 29. * f64::from(im.data[i + 2]))
                / 256.
        }
    };
    let sample = |x: f64, y: f64| -> Option<f64> {
        if x < 0.
            || y < 0.
            || x >= crate::numeric::usize_f64(im.width - 1)
            || y >= crate::numeric::usize_f64(im.height - 1)
        {
            return None;
        }
        let (ix, iy) = (
            crate::numeric::f64_usize(x.floor()),
            crate::numeric::f64_usize(y.floor()),
        );
        let (fx, fy) = (
            x - crate::numeric::usize_f64(ix),
            y - crate::numeric::usize_f64(iy),
        );
        Some(
            (gray(ix, iy) * (1. - fx) + gray(ix + 1, iy) * fx) * (1. - fy)
                + (gray(ix, iy + 1) * (1. - fx) + gray(ix + 1, iy + 1) * fx) * fy,
        )
    };
    let mut positive = vec![0u8; count];
    let mut negative = vec![0u8; count];
    for fraction in [0.2, 0.35, 0.5, 0.65, 0.8] {
        let mut last = None;
        for i in 0..count {
            #[cfg(any(
                feature = "mode-low",
                feature = "mode-high",
                feature = "mode-very-high"
            ))]
            let u = crate::numeric::isize_f64(start + (i).cast_signed()) / width;
            #[cfg(feature = "mode-medium")]
            let u = crate::numeric::isize_f64(start + (i).cast_signed()) / w;

            let v = sample(
                quad[0][0] + ex[0] * u + ey[0] * fraction,
                quad[0][1] + ex[1] * u + ey[1] * fraction,
            );
            if let (Some(v), Some(old)) = (v, last) {
                if v - old >= 18. {
                    positive[i] += 1;
                }
                if old - v >= 18. {
                    negative[i] += 1;
                }
            }
            last = v;
        }
    }
    let mut events = Vec::new();
    let mut prior = 0i8;
    for i in 0..count {
        let polarity = if positive[i] >= 3 {
            1
        } else if negative[i] >= 3 {
            -1
        } else {
            0
        };
        if polarity != 0 && polarity != prior {
            events.push(crate::numeric::isize_f64(start + (i).cast_signed()));
        }
        prior = polarity;
    }
    #[cfg(any(
        feature = "mode-low",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let gap = (width * 0.05).clamp(8., 24.);
    #[cfg(feature = "mode-medium")]
    let gap = (w * 0.05).clamp(8., 24.);

    let (mut begin, mut best_begin, mut best_end) = (0, 0, 0);
    while begin < events.len() {
        let mut end = begin + 1;
        while end < events.len() && events[end] - events[end - 1] <= gap {
            end += 1;
        }
        {
            #[cfg(any(
                feature = "mode-low",
                feature = "mode-high",
                feature = "mode-very-high"
            ))]
            {
                if end - begin >= 30
                    && events[begin] < width * 0.5
                    && events[end - 1] > width * 0.5
                    && end - begin > best_end - best_begin
                {
                    best_begin = begin;
                    best_end = end;
                }
            }
            #[cfg(feature = "mode-medium")]
            {
                if end - begin >= 30
                    && events[begin] < w * 0.5
                    && events[end - 1] > w * 0.5
                    && end - begin > best_end - best_begin
                {
                    best_begin = begin;
                    best_end = end;
                }
            }
        }

        begin = end;
    }
    if best_end == 0 {
        return None;
    }
    let lo = (events[best_begin] - 2.).min(0.);
    #[cfg(any(
        feature = "mode-low",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let hi = (events[best_end - 1] + 2.).max(width);
    #[cfg(feature = "mode-medium")]
    let hi = (events[best_end - 1] + 2.).max(w);

    {
        #[cfg(any(
            feature = "mode-low",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        {
            if lo >= -2. && hi <= width + 2. {
                return None;
            }
        }
        #[cfg(feature = "mode-medium")]
        {
            if lo >= -2. && hi <= w + 2. {
                return None;
            }
        }
    }

    let at = |u: f64, v: f64| {
        #[cfg(any(
            feature = "mode-low",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        {
            [
                quad[0][0] + ex[0] * u / width + ey[0] * v,
                quad[0][1] + ex[1] * u / width + ey[1] * v,
            ]
        }
        #[cfg(feature = "mode-medium")]
        {
            [
                quad[0][0] + ex[0] * u / w + ey[0] * v,
                quad[0][1] + ex[1] * u / w + ey[1] * v,
            ]
        }
    };
    Some(Proposal {
        polygon: [at(lo, 0.), at(hi, 0.), at(hi, 1.), at(lo, 1.)],
        score: p.score,
    })
}
#[cfg(test)]
mod extent_tests {
    use super::*;
    fn image(kind: usize) -> Vec<u8> {
        let mut d = vec![255; 360 * 100];
        for y in 20..80 {
            for x in 60..260 {
                if kind > 0 && ((x - 60) / if kind == 2 { 1 } else { 3 }) % 2 == 0 {
                    d[y * 360 + x] = 0;
                }
            }
        }
        d
    }
    fn quad() -> Proposal {
        Proposal {
            polygon: [[90., 25.], [275., 25.], [275., 75.], [90., 75.]],
            score: 1.,
        }
    }
    #[test]
    fn source_angle_then_extent_preserves_source_stripes() {
        let d = image(1);
        let im = ImageView::new(&d, 360, 100, 1, 360).unwrap();
        let a = 5f64.to_radians();
        let (cos_angle, sin_angle) = (a.cos(), a.sin());
        #[cfg(feature = "mode-low")]
        let q = Proposal {
            polygon: [[90., 40.], [275., 40.], [275., 60.], [90., 60.]].map(|p| {
                let (x, y) = (p[0] - 182.5, p[1] - 50.);
                [
                    182.5 + cos_angle * x - sin_angle * y,
                    50. + sin_angle * x + cos_angle * y,
                ]
            }),
            score: 1.,
        };
        #[cfg(any(
            feature = "mode-medium",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let quad = Proposal {
            polygon: [[90., 40.], [275., 40.], [275., 60.], [90., 60.]].map(|p| {
                let (x, y) = (p[0] - 182.5, p[1] - 50.);
                [
                    182.5 + cos_angle * x - sin_angle * y,
                    50. + sin_angle * x + cos_angle * y,
                ]
            }),
            score: 1.,
        };

        #[cfg(feature = "mode-low")]
        let corrected = source_angle(im, &q).expect("native stripe tensor corrects slant");
        #[cfg(any(
            feature = "mode-medium",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let corrected = source_angle(im, &quad).expect("native stripe tensor corrects slant");

        let grown = source_extent(im, &corrected).expect("clipped stripe extent grows");
        assert!(grown.polygon[0][0] < 70.);
        assert!((grown.polygon[1][1] - grown.polygon[0][1]).abs() < 1.);
        let blank = image(0);
        {
            #[cfg(feature = "mode-low")]
            {
                assert!(
                    source_angle(ImageView::new(&blank, 360, 100, 1, 360).unwrap(), &q).is_none()
                );
            }
            #[cfg(any(
                feature = "mode-medium",
                feature = "mode-high",
                feature = "mode-very-high"
            ))]
            {
                assert!(
                    source_angle(ImageView::new(&blank, 360, 100, 1, 360).unwrap(), &quad)
                        .is_none()
                );
            }
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn source_extent_recovers_clipped_bars_and_polarity() {
        for kind in [1, 2] {
            let d = image(kind);
            let im = ImageView::new(&d, 360, 100, 1, 360).unwrap();
            let q = quad();
            let r = source_extent(im, &q).unwrap();
            assert!(r.polygon[0][0] <= 62.);
            assert_eq!(r.polygon[1][0], 275.);
            assert_eq!(q.polygon[0], [90., 25.]);
        }
    }
    #[test]
    fn source_extent_rejects_blank_and_single_row_artifacts() {
        let mut d = image(0);
        let q = quad();
        assert!(source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &q).is_none());
        for x in 60..260 {
            d[50 * 360 + x] = ((x / 3) % 2 * 255).to_le_bytes()[0];
        }
        assert!(source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &q).is_none());
    }
    #[test]
    fn source_extent_respects_quiet_gap() {
        let mut d = image(1);
        for y in 20..80 {
            for x in 8..35 {
                d[y * 360 + x] = ((x / 3) % 2 * 255).to_le_bytes()[0];
            }
        }
        let r = source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &quad()).unwrap();
        assert!(r.polygon[0][0] > 50.);
    }
}
#[cfg(test)]
mod extent_budget_tests {
    use super::*;
    #[test]
    fn clutter_reports_unexamined_extent_work() {
        let (w, h) = (1000, 800);
        let mut data = vec![255u8; w * h];
        for row in 0..6 {
            for col in 0..8 {
                let (x0, y0) = (15 + col * 122, 20 + row * 125);
                for y in y0..y0 + 40 {
                    for x in x0..x0 + 96 {
                        data[y * w + x] = if (x - x0) / 3 % 2 == 0 { 0 } else { 255 };
                    }
                }
            }
        }
        let r = detect(ImageView::new(&data, w, h, 1, w).unwrap()).unwrap();
        assert!(
            r.trace[7] > 0,
            "fixture must exceed extent budget: {:?}",
            r.trace
        );
        assert!(r.limited);
        assert!(r.omitted >= r.trace[7]);
        assert!(r.proposals.len() <= 24);
    }
    #[test]
    fn blank_has_no_unexamined_extent_work() {
        let d = vec![255u8; 256 * 256];
        let r = detect(ImageView::new(&d, 256, 256, 1, 256).unwrap()).unwrap();
        assert_eq!(r.trace[7], 0);
        assert_eq!(r.omitted, 0);
        assert!(!r.limited);
    }
}
#[cfg(test)]
mod angle_distance_tests {
    use super::distance;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn modulo_distance_matches_trigonometric_reference() {
        let pi = std::f64::consts::PI;
        for i in -720..=720 {
            for j in -90..=90 {
                let a = f64::from(i) * pi / 360.;
                let b = f64::from(j) * pi / 180.;
                let old = (2. * (a - b)).sin().atan2((2. * (a - b)).cos()).abs() / 2.;
                assert!((distance(a, b) - old).abs() < 2e-15);
            }
        }
        assert_eq!(distance(-pi / 2., pi / 2.), 0.);
        assert_eq!(distance(0., pi / 2.), pi / 2.);
        assert_eq!(distance(0., 0.), 0.);
    }
    #[test]
    fn angle_gate_preserves_sides_of_all_used_thresholds() {
        let pi = std::f64::consts::PI;
        for threshold in [pi / 9., pi / 18.] {
            for base in [-pi / 2., -0.3, 0., 0.7, pi / 2.] {
                for sign in [-1., 1.] {
                    for delta in [-1e-12, 1e-12] {
                        let other = base + sign * (threshold + delta);
                        assert_eq!(distance(base, other) < threshold, delta < 0.);
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tall_band_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "Mode-specific arithmetic is checked against the same connected-border fixture."
    )]
    fn dense_barcode_band_survives_tall_connected_border() {
        #[cfg(any(
            feature = "mode-low",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let (width, height) = (400, 400);
        #[cfg(feature = "mode-medium")]
        let (w, h) = (400, 400);

        #[cfg(any(
            feature = "mode-low",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let mut d = vec![255u8; width * height];
        #[cfg(feature = "mode-medium")]
        let mut d = vec![255u8; w * h];

        for y in 20..380 {
            for x in 80..83 {
                {
                    #[cfg(any(
                        feature = "mode-low",
                        feature = "mode-high",
                        feature = "mode-very-high"
                    ))]
                    {
                        d[y * width + x] = 0;
                    }
                    #[cfg(feature = "mode-medium")]
                    {
                        d[y * w + x] = 0;
                    }
                }
            }
        }
        for y in 200..260 {
            for x in 80..280 {
                if ((x - 80) / 3) % 2 == 0 {
                    {
                        #[cfg(any(
                            feature = "mode-low",
                            feature = "mode-high",
                            feature = "mode-very-high"
                        ))]
                        {
                            d[y * width + x] = 0;
                        }
                        #[cfg(feature = "mode-medium")]
                        {
                            d[y * w + x] = 0;
                        }
                    }
                }
            }
        }
        #[cfg(any(
            feature = "mode-low",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let im = ImageView::new(&d, width, height, 1, width).unwrap();
        #[cfg(feature = "mode-medium")]
        let im = ImageView::new(&d, w, h, 1, w).unwrap();

        let mut tall = false;
        let r = detect_with_observer(im, |g| {
            tall |= g.reason == "tall" && g.edges > 1000;
        })
        .unwrap();
        assert!(tall, "must exercise rejected connected parent");
        {
            #[cfg(feature = "mode-low")]
            {
                assert!(
                    r.proposals.iter().any(|p| {
                        let q = p.polygon;
                        let width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
                        let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
                        let cy = q.iter().map(|p| p[1]).sum::<f64>() / 4.;
                        width > 180. && height < 80. && (200. ..260.).contains(&cy)
                    }),
                    "dense band missing"
                );
            }
            #[cfg(any(
                feature = "mode-medium",
                feature = "mode-high",
                feature = "mode-very-high"
            ))]
            {
                assert!(
                    r.proposals.iter().any(|p| {
                        let quad = p.polygon;
                        let width = (quad[1][0] - quad[0][0]).hypot(quad[1][1] - quad[0][1]);
                        let height = (quad[3][0] - quad[0][0]).hypot(quad[3][1] - quad[0][1]);
                        let cy = quad.iter().map(|p| p[1]).sum::<f64>() / 4.;
                        width > 180. && height < 80. && (200. ..260.).contains(&cy)
                    }),
                    "dense band missing"
                );
            }
        }

        let plain = detect(im).unwrap();
        assert_eq!(plain.trace, r.trace);
        assert_eq!(plain.omitted, r.omitted);
        assert_eq!(plain.proposals.len(), r.proposals.len());
        for (a, b) in plain.proposals.iter().zip(&r.proposals) {
            assert_eq!(a.polygon, b.polygon);
            assert_eq!(a.score, b.score);
        }
    }
    #[test]
    fn lone_tall_border_does_not_become_a_barcode_band() {
        let (w, h) = (400, 400);
        let mut d = vec![255u8; w * h];
        for y in 20..380 {
            for x in 80..83 {
                d[y * w + x] = 0;
            }
        }
        assert!(detect(ImageView::new(&d, w, h, 1, w).unwrap())
            .unwrap()
            .proposals
            .is_empty());
    }
}
#[cfg(test)]
mod edge_bound_tests {
    use super::edge_weight;
    #[cfg(feature = "mode-low")]
    #[test]
    fn bounded_norm_preserves_decisions_and_weights_within_roundoff() {
        for ix in -255..=255 {
            for iy in -255..=255 {
                let (dx, dy) = (f64::from(ix) / 2., f64::from(iy) / 2.);
                for angle in [0f64, 0.4, 0.9, 1.5, 2.4] {
                    let (ax, ay) = (angle.cos(), angle.sin());
                    let w = dx.hypot(dy);
                    let original = (w > 18. && (dx * ax + dy * ay).abs() > 0.85 * w).then_some(w);
                    let actual = edge_weight(dx, dy, ax, ay);
                    assert_eq!(actual.is_some(), original.is_some());
                    if let (Some(a), Some(b)) = (actual, original) {
                        assert!((a - b).abs() <= 2. * f64::EPSILON * b.max(1.));
                    }
                }
            }
        }
    }
    #[cfg(any(
        feature = "mode-medium",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    #[test]
    fn prefilter_keeps_exact_original_weights_and_decisions() {
        for ix in -255..=255 {
            for iy in -255..=255 {
                let (dx, dy) = (f64::from(ix) / 2., f64::from(iy) / 2.);
                for angle in [0f64, 0.4, 0.9, 1.5, 2.4] {
                    let (ax, ay) = (angle.cos(), angle.sin());
                    let w = dx.hypot(dy);
                    let original = (w > 18. && (dx * ax + dy * ay).abs() > 0.85 * w).then_some(w);
                    let actual = edge_weight(dx, dy, ax, ay);
                    assert_eq!(actual.is_some(), original.is_some());
                    if let (Some(actual), Some(expected)) = (actual, original) {
                        assert!((actual - expected).abs() <= 4. * f64::EPSILON * expected.max(1.));
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod extent_plan_tests {
    use super::*;
    fn q(w: f64, h: f64) -> Proposal {
        Proposal {
            polygon: [[0., 0.], [w, 0.], [w, h], [0., h]],
            score: 1.,
        }
    }
    #[test]
    fn ineligible_regions_do_not_consume_sampling_slots() {
        let mut p = vec![q(500., 50.); 24];
        p.extend((0..30).map(|_| q(150., 50.)));
        let (old, n, bad) = extent_plan(&p, false);
        assert!(old.is_empty());
        assert_eq!((n, bad), (30, 24));
        let (new, n, bad) = extent_plan(&p, true);
        assert_eq!(new, (24..48).collect::<Vec<_>>());
        assert_eq!(n - new.len(), 6);
        assert_eq!(bad, 24);
        assert_eq!(p.len(), 54);
    }
    #[test]
    fn shared_dimension_check_preserves_boundaries_and_invalid_rejection() {
        for (w, h, valid) in [
            (76., 12., true),
            (400., 200., true),
            (75.9, 20., false),
            (400.1, 20., false),
            (100., 11.9, false),
            (100., 200.1, false),
            (f64::NAN, 50., false),
        ] {
            assert_eq!(extent_dimensions(&q(w, h)).is_some(), valid);
        }
        let p = vec![q(100., 50.); 30];
        let (a, _, _) = extent_plan(&p, false);
        let (b, _, _) = extent_plan(&p, true);
        assert_eq!(a, b);
        assert_eq!(a.len(), 24);
    }
}
#[cfg(test)]
mod band_angle_tests {
    use super::*;
    #[test]
    fn band_angle_rejects_blank_or_unsupported_edges() {
        let g = vec![0f32; 100 * 100];
        assert!(refine_band_angle(&[], &g, &g, 100, 0., [0., 99., 0., 99.], 1., 1.).is_none());
        let e: Vec<_> = (0..100)
            .map(|i| Edge {
                x: f64::from(i % 50) + 10.5,
                y: f64::from(i / 50) + 20.5,
                weight: 50.,
            })
            .collect();
        assert!(refine_band_angle(&e, &g, &g, 100, 0., [0., 99., 0., 99.], 1., 1.).is_none());
    }
    #[test]
    fn band_angle_fits_coherent_slanted_edges_in_parent_band() {
        let width = 320;
        let height = 240;
        let a = 0.10f64;
        let (cos_angle, sin_angle) = (a.cos(), a.sin());
        let mut gx = vec![0f32; width * height];
        let mut gy = gx.clone();
        let mut edges = Vec::new();
        for u in (-90..=90).step_by(6) {
            for v in -25..=25 {
                let x = 160. + f64::from(u) * cos_angle - f64::from(v) * sin_angle;
                let y = 120. + f64::from(u) * sin_angle + f64::from(v) * cos_angle;
                let i = crate::numeric::f64_usize(y.floor()) * width
                    + crate::numeric::f64_usize(x.floor());
                gx[i] = crate::numeric::f64_f32(100. * cos_angle);
                gy[i] = crate::numeric::f64_f32(100. * sin_angle);
                edges.push(Edge {
                    x: x.floor() + 0.5,
                    y: y.floor() + 0.5,
                    weight: 100.,
                });
            }
        }
        let p = refine_band_angle(&edges, &gx, &gy, width, 0., [50., 270., 85., 155.], 1., 1.)
            .expect("coherent slanted band");
        #[cfg(feature = "mode-low")]
        let q = p.polygon;
        #[cfg(any(
            feature = "mode-medium",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let quad = p.polygon;

        #[cfg(feature = "mode-low")]
        let angle = (q[1][1] - q[0][1]).atan2(q[1][0] - q[0][0]);
        #[cfg(any(
            feature = "mode-medium",
            feature = "mode-high",
            feature = "mode-very-high"
        ))]
        let angle = (quad[1][1] - quad[0][1]).atan2(quad[1][0] - quad[0][0]);

        assert!(distance(angle, a) < 1f64.to_radians());
    }
}

// Apply component compatibility after dense bands have isolated bar height.
fn join_fragments(a: &Proposal, b: &Proposal, max_gap: f64) -> Option<Proposal> {
    let qa = a.polygon;
    let qb = b.polygon;
    let angle = (qa[1][1] - qa[0][1]).atan2(qa[1][0] - qa[0][0]);
    let other = (qb[1][1] - qb[0][1]).atan2(qb[1][0] - qb[0][0]);
    if distance(angle, other) > std::f64::consts::PI / 18. {
        return None;
    }
    let (cos_angle, sin_angle) = (angle.cos(), angle.sin());
    #[cfg(feature = "mode-low")]
    let bounds = |q: crate::scan::Quad| {
        let mut extents = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in q {
            let (u, v) = (
                p[0] * cos_angle + p[1] * sin_angle,
                -p[0] * sin_angle + p[1] * cos_angle,
            );
            extents[0] = extents[0].min(u);
            extents[1] = extents[1].max(u);
            extents[2] = extents[2].min(v);
            extents[3] = extents[3].max(v);
        }
        extents
    };
    #[cfg(any(
        feature = "mode-medium",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    let bounds = |quad: crate::scan::Quad| {
        let mut extents = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in quad {
            let (u, v) = (
                p[0] * cos_angle + p[1] * sin_angle,
                -p[0] * sin_angle + p[1] * cos_angle,
            );
            extents[0] = extents[0].min(u);
            extents[1] = extents[1].max(u);
            extents[2] = extents[2].min(v);
            extents[3] = extents[3].max(v);
        }
        extents
    };

    let x = bounds(qa);
    let y = bounds(qb);
    let (hx, hy) = (x[3] - x[2], y[3] - y[2]);
    let gap = (x[0] - y[1]).max(y[0] - x[1]);
    if gap <= 0.
        || gap > max_gap
        || hx.min(hy) / hx.max(hy) < 0.65
        || x[3].min(y[3]) - x[2].max(y[2]) < 0.8 * hx.min(hy)
    {
        return None;
    }
    let (u0, u1, v0, v1) = (
        x[0].min(y[0]),
        x[1].max(y[1]),
        x[2].min(y[2]),
        x[3].max(y[3]),
    );
    let at = |u: f64, v: f64| [u * cos_angle - v * sin_angle, u * sin_angle + v * cos_angle];
    Some(Proposal {
        polygon: [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)],
        score: a.score.min(b.score),
    })
}
#[cfg(test)]
mod fragment_tests {
    use super::*;
    fn p(x: f64, y: f64, w: f64, h: f64) -> Proposal {
        Proposal {
            polygon: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            score: 0.9,
        }
    }
    #[test]
    fn adjacent_fragments_join_without_stacking_or_wide_gap() {
        let a = p(10., 20., 90., 60.);
        let b = p(110., 22., 140., 58.);
        let r = join_fragments(&a, &b, 16.).unwrap();
        assert_eq!(
            r.polygon,
            [[10., 20.], [250., 20.], [250., 80.], [10., 80.]]
        );
        assert!(join_fragments(&a, &p(10., 92., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(130., 20., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(90., 20., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(110., 60., 90., 60.), 16.).is_none());
    }
}
fn covers_fragment(old: &Proposal, new: &Proposal, tolerance: f64) -> bool {
    let q = old.polygon;
    let area = (0..4)
        .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[i][1] * q[(i + 1) % 4][0])
        .sum::<f64>();
    if area.abs() < 1e-6 {
        return false;
    }
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        if length < 1e-6 {
            return false;
        }
        for p in new.polygon {
            if area.signum() * (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) < -tolerance * length {
                return false;
            }
        }
    }
    true
}
#[cfg(test)]
mod fragment_coverage_tests {
    use super::*;
    fn p(x: f64, w: f64) -> Proposal {
        Proposal {
            polygon: [[x, 0.], [x + w, 0.], [x + w, 60.], [x, 60.]],
            score: 1.,
        }
    }
    #[test]
    fn coverage_only_suppresses_existing_extent() {
        assert!(covers_fragment(&p(0., 100.), &p(-1., 102.), 2.));
        assert!(!covers_fragment(&p(0., 100.), &p(0., 150.), 2.));
        let mut reversed = p(0., 100.);
        reversed.polygon.reverse();
        assert!(covers_fragment(&reversed, &p(1., 98.), 2.));
    }
}
#[cfg(test)]
mod angle_only_refinement_tests {
    use super::*;
    #[test]
    fn useful_rotation_does_not_require_crop_growth() {
        let (width, height) = (360, 160);
        let mut pixels = vec![255u8; width * height];
        for y in 30..130 {
            for x in 60..280 {
                pixels[y * width + x] = if x % 6 < 3 { 0 } else { 255 };
            }
        }
        let a = 5f64.to_radians();
        let (c, t) = (a.cos(), a.sin());
        let p = Proposal {
            polygon: [[-130., -45.], [130., -45.], [130., 45.], [-130., 45.]]
                .map(|[x, y]| [180. + c * x - t * y, 80. + t * x + c * y]),
            score: 0.9,
        };
        let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
        let (grown, angles) = source_refinements(im, &[p]);
        assert!(grown.is_empty());
        assert_eq!(angles.len(), 1);
        let quad = angles[0].polygon;
        assert!((quad[1][1] - quad[0][1]).abs() < 1.);
    }
}
#[cfg(test)]
mod angle_replacement_tests {
    use super::*;
    fn proposal(degree: f64, score: f64) -> Proposal {
        let a = degree.to_radians();
        let (c, s) = (a.cos(), a.sin());
        Proposal {
            polygon: [[-130., -45.], [130., -45.], [130., 45.], [-130., 45.]]
                .map(|[x, y]| [180. + c * x - s * y, 80. + s * x + c * y]),
            score,
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn only_high_coherence_small_correction_replaces_parent_geometry() {
        for (angle, score, replaces) in [
            (5., 0.9, true),
            (10., 0.95, true),
            (10.1, 0.99, false),
            (5., 0.899, false),
        ] {
            let mut parent = proposal(angle, 0.8);
            let old = parent.polygon;
            let corrected = proposal(0., score);
            let expected = corrected.polygon;
            let (mut grown, mut supplements) = (Vec::new(), Vec::new());
            admit_source_refinement(&mut parent, corrected, None, &mut grown, &mut supplements);
            assert!(grown.is_empty());
            if replaces {
                assert_eq!(parent.polygon, expected);
                assert!(supplements.is_empty());
                assert_eq!(parent.score, score);
            } else {
                assert_eq!(parent.polygon, old);
                assert_eq!(supplements.len(), 1);
                assert_eq!(supplements[0].polygon, expected);
            }
        }
        assert!(angle_replacement_qualified(
            &proposal(175., 0.8),
            &proposal(-179., 0.95)
        ));
    }
    #[test]
    fn grown_extent_keeps_parent_and_supplement_even_when_angle_qualifies() {
        let mut parent = proposal(5., 0.8);
        let before = parent.polygon;
        let corrected = proposal(0., 0.99);
        let mut grown = proposal(0., 0.99);
        grown.polygon[0][0] -= 30.;
        grown.polygon[3][0] -= 30.;
        let expected = grown.polygon;
        let (mut extents, mut angles) = (Vec::new(), Vec::new());
        admit_source_refinement(
            &mut parent,
            corrected,
            Some(grown),
            &mut extents,
            &mut angles,
        );
        assert_eq!(parent.polygon, before);
        assert_eq!(extents.len(), 1);
        assert_eq!(extents[0].polygon, expected);
        assert!(angles.is_empty());
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn source_tensor_replaces_qualified_angle_only_but_preserves_large_correction() {
        let (w, h) = (360, 160);
        let mut pixels = vec![255u8; w * h];
        for y in 30..130 {
            for x in 60..280 {
                pixels[y * w + x] = if x % 6 < 3 { 0 } else { 255 };
            }
        }
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        for degree in [5., 15.] {
            let parent = proposal(degree, 0.8);
            let before = parent.polygon;
            let mut parents = vec![parent];
            let (control_grown, control_angles) = source_refinements(im, &[parent]);
            let (grown, angles) = source_refinements_replacing(im, &mut parents);
            assert!(grown.is_empty());
            assert!(control_grown.is_empty());
            assert_eq!(control_angles.len(), 1);
            if degree == 5. {
                assert!(angles.is_empty());
                assert_eq!(parents[0].polygon, control_angles[0].polygon);
                assert_ne!(parents[0].polygon, before);
            } else {
                assert_eq!(parents[0].polygon, before);
                assert_eq!(angles.len(), 1);
                assert_eq!(angles[0].polygon, control_angles[0].polygon);
            }
        }
    }
}
#[cfg(test)]
mod exact_luma_gradient_tests {
    #[test]
    fn integer_luminance_matches_all_rgb_values() {
        for r in 0..256u32 {
            for g in 0..256u32 {
                for b in 0..256u32 {
                    let new = crate::numeric::f64_f32(f64::from(77 * r + 150 * g + 29 * b)) / 256.;
                    let old = crate::numeric::f64_f32(
                        (77. * f64::from(r) + 150. * f64::from(g) + 29. * f64::from(b)) / 256.,
                    );
                    assert_eq!(new.to_bits(), old.to_bits());
                }
            }
        }
    }
    #[test]
    fn raw_scharr_f32_matches_f64_at_extremes_and_random_pixels() {
        let mut seed = 19u32;
        for _ in 0..100_000 {
            let p: [f32; 8] = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                crate::numeric::f64_f32(f64::from(seed % 65281)) / 256.
            });
            let fast = (3. * (p[2] - p[0]) + 10. * (p[4] - p[3]) + 3. * (p[7] - p[5])) / 16.;
            let old = (3. * (f64::from(p[2]) - f64::from(p[0]))
                + 10. * (f64::from(p[4]) - f64::from(p[3]))
                + 3. * (f64::from(p[7]) - f64::from(p[5])))
                / 16.;
            assert_eq!(f64::from(fast).to_bits(), old.to_bits());
        }
    }
}
#[cfg(feature = "mode-low")]
#[cfg(test)]
mod quarter_lattice_tests {
    #[test]
    fn stepped_positions_match_original_on_tiny_and_odd_rasters() {
        for w in 3..800 {
            for y in 1..12 {
                for sparse in [false, true] {
                    let expected: Vec<_> = (1..w - 1)
                        .filter(|x| !sparse || (x + y + x / 2 + y / 2) % 4 == 0)
                        .collect();
                    let [a, b] = [[0usize, 3], [1, 6], [4, 7], [2, 5]][(4 - (y + y / 2) % 4) % 4];
                    let mut x = if sparse {
                        if a == 0 {
                            b
                        } else {
                            a
                        }
                    } else {
                        1
                    };
                    let mut actual = Vec::new();
                    while x < w - 1 {
                        actual.push(x);
                        x += if !sparse {
                            1
                        } else if x % 8 == a {
                            b - a
                        } else {
                            8 + a - b
                        };
                    }
                    assert_eq!(actual, expected);
                }
            }
        }
    }
}
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod secondary_sampler_tests {
    use super::*;
    #[test]
    fn cached_columns_preserve_every_output_bit_with_stride_and_channel_variants() {
        for channels in [1, 3, 4] {
            for (iw, ih, w, h) in [
                (3, 3, 3, 3),
                (17, 29, 11, 19),
                (83, 57, 41, 29),
                (31, 11, 43, 15),
            ] {
                let stride = iw * channels + 7;
                let mut bytes = vec![0; stride * ih];
                let mut seed = 0x1293_87ab_u32;
                for top_right in &mut bytes {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *top_right = (seed >> 24) as u8;
                }
                let im = ImageView::new(&bytes, iw, ih, channels, stride).unwrap();
                let mut actual = vec![0.; w * h];
                let mut histogram = [[0usize; 256]; 4];
                secondary_gray(im, w, h, &mut actual, &mut histogram);
                let mut gray = vec![0f32; w * h];
                // Diagnostic antialiased working-raster hypothesis; decoder source pixels
                // stay untouched. Same legacy RGB weights, bilinear center sampling.
                let luminance = |x: usize, y: usize| {
                    let i = y * im.stride + x * im.channels;
                    if im.channels == 1 {
                        f64::from(im.data[i])
                    } else {
                        (77. * f64::from(im.data[i])
                            + 150. * f64::from(im.data[i + 1])
                            + 29. * f64::from(im.data[i + 2]))
                            / 256.
                    }
                };
                for y in 0..h {
                    let sy = ((crate::numeric::usize_f64(y) + 0.5)
                        * crate::numeric::usize_f64(im.height)
                        / crate::numeric::usize_f64(h)
                        - 0.5)
                        .clamp(0., crate::numeric::usize_f64(im.height - 1));
                    let y0 = crate::numeric::f64_usize(sy.floor());
                    let y1 = (y0 + 1).min(im.height - 1);
                    let fy = sy - crate::numeric::usize_f64(y0);
                    for x in 0..w {
                        let sx = ((crate::numeric::usize_f64(x) + 0.5)
                            * crate::numeric::usize_f64(im.width)
                            / crate::numeric::usize_f64(w)
                            - 0.5)
                            .clamp(0., crate::numeric::usize_f64(im.width - 1));
                        let x0 = crate::numeric::f64_usize(sx.floor());
                        let x1 = (x0 + 1).min(im.width - 1);
                        let fx = sx - crate::numeric::usize_f64(x0);
                        let top_left = luminance(x0, y0);
                        let top_right = luminance(x1, y0);
                        let bottom_left = luminance(x0, y1);
                        let bottom_right = luminance(x1, y1);
                        let u = top_left + (top_right - top_left) * fx;
                        let v = bottom_left + (bottom_right - bottom_left) * fx;
                        gray[y * w + x] = crate::numeric::f64_f32(u + (v - u) * fy);
                    }
                }

                let mut expected_histogram = [0usize; 256];
                for &value in &gray {
                    expected_histogram[crate::numeric::f32_usize(value.clamp(0., 255.).floor())] +=
                        1;
                }
                for (bin, expected) in expected_histogram.iter().enumerate() {
                    assert_eq!(
                        histogram.iter().map(|lane| lane[bin]).sum::<usize>(),
                        *expected
                    );
                }
                assert!(
                    actual
                        .iter()
                        .zip(&gray)
                        .all(|(top_left, top_right)| top_left.to_bits() == top_right.to_bits()),
                    "channels={channels} input={iw}x{ih} output={w}x{h}"
                );
            }
        }
    }
}
