//! Source-profile recovery for localized regions in Medium, High and Very High:
//! contrast, color and polarity variants, band probes, and bridge decoding that
//! proves separate recovered bands belong to one physical symbol.
use super::{Candidate, DENSITY, ROWS, SourceProfile, point, sample_line};
use crate::{
    Error, Image, ImageView, Quad,
    geometry::lerp,
    read::{Read, Region},
};
use barcode_multiformat::linear;
use barcode_research_core::numeric::usize_f64;

/// Reuse confirmed oriented source profiles for localized regions in Medium, High and Very High.
pub(crate) fn recover_proposals(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_contrast(image, proposals, mask, sampler, 0.)
}
pub(crate) fn recover_proposals_contrast(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    contrast: f32,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(
        image,
        proposals,
        mask,
        sampler,
        SourceProfile::Gray(contrast),
    )
}
pub(crate) fn recover_proposals_color(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    color: u8,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(image, proposals, mask, sampler, SourceProfile::Color(color))
}
pub(crate) fn recover_proposals_inverted(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(image, proposals, mask, sampler, SourceProfile::Inverted)
}
#[cfg(feature = "medium")]
pub(crate) fn recover_proposals_highpass(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(
        image,
        proposals,
        mask,
        sampler,
        SourceProfile::Highpass(strength),
    )
}
#[cfg(feature = "medium")]
pub(crate) fn recover_proposals_deghost(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    backward: bool,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(
        image,
        proposals,
        mask,
        sampler,
        SourceProfile::Deghost(backward),
    )
}
#[cfg(feature = "medium")]
pub(crate) fn recover_proposals_wide(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(image, proposals, mask, sampler, SourceProfile::WideEan)
}
/// Probe unresolved bands before spending the full confirmation budget.
/// Probe observations never become outputs; the established pass starts fresh.
#[cfg(feature = "medium")]
fn band_has_evidence(
    candidate: &mut Candidate<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> bool {
    let previous_runs = sampler.runs.clone();
    let previous_budget = candidate.remaining;
    let previous_restored = candidate.restored;
    for (row, v) in [0.08, 0.22, 0.36, 0.5, 0.64, 0.78, 0.92]
        .into_iter()
        .enumerate()
    {
        sample_line(candidate, sampler, row, v);
        if !candidate.observations.is_empty() {
            break;
        }
    }
    let promising = !candidate.observations.is_empty();
    sampler.runs = previous_runs;
    candidate.observations.clear();
    candidate.row_positions.clear();
    candidate.remaining = previous_budget;
    candidate.restored = previous_restored;
    promising
}

fn recover_proposals_variant(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    profile: SourceProfile,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    let contrast = profile.contrast();
    let im = ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )?;
    let mut reads = Vec::new();
    let mut refined = Vec::new();
    let mut unread = Vec::new();
    let mut continuity_budget = 131_072;
    let mut refinement_budget = 32_768;
    let mut refinement_lines = 0;
    let mut dense_budget = 32_768;
    let mut dense_lines = 0;
    for proposal in proposals.iter().take(32) {
        let mut candidate = Candidate {
            image,
            im,
            quad: proposal.polygon,
            mask,
            dense: false,
            restored: false,
            profile,
            localized: true,
            remaining: continuity_budget,
            observations: Vec::new(),
            row_positions: Vec::new(),
        };
        #[cfg(feature = "medium")]
        if matches!(profile, SourceProfile::BandEan(_))
            && !band_has_evidence(&mut candidate, sampler)
        {
            unread.push(Region::unknown(proposal.polygon));
            continue;
        }
        for (row, &v) in ROWS.iter().enumerate() {
            sample_line(&mut candidate, sampler, row, v);
        }
        continuity_budget = candidate.remaining;
        let refined_count = candidate.refine(
            sampler,
            &mut refinement_budget,
            &mut refinement_lines,
            &mut dense_budget,
            &mut dense_lines,
        );
        let before = reads.len() + refined.len();
        candidate.append_confirmed(refined_count > 0, &mut reads, &mut refined);
        if reads.len() + refined.len() == before {
            unread.push(Region::unknown(proposal.polygon));
        }
    }
    reads.extend(refined);
    let reads = if contrast > 0. {
        join_recovered_bands(reads, image, sampler, contrast)
    } else {
        reads
    };
    Ok((reads, unread))
}

/// Additional physical proof for recovered bands: every intervening half-pixel
/// source line must decode the same complete symbol at the same endpoints.
/// A matching value or overlapping display footprint alone never joins bands.
pub(crate) fn join_recovered_bands(
    reads: Vec<Read>,
    image: Image<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
) -> Vec<Read> {
    reconcile_recovered(reads, usize::MAX, image, sampler, strength).0
}

/// Additional physical instances need evidence beyond an overlapping equal
/// value. Established reads survive; unresolved new claims remain pending.
pub(crate) fn append_recovered(
    mut reads: Vec<Read>,
    additions: Vec<Read>,
    image: Image<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
) -> (Vec<Read>, Vec<Region>) {
    let established = reads.len();
    reads.extend(additions);
    reconcile_recovered(reads, established, image, sampler, strength)
}

fn reconcile_recovered(
    reads: Vec<Read>,
    established: usize,
    image: Image<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
) -> (Vec<Read>, Vec<Region>) {
    let Ok(im) = ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    ) else {
        return (reads, Vec::new());
    };
    let mut deferred = Vec::new();
    let mut remaining = 262_144usize;
    let mut rows = 256usize;
    let mut merged: Vec<Read> = Vec::new();
    for (index, read) in reads.into_iter().enumerate() {
        let mut used = false;
        let mut ambiguous = false;
        for previous in &mut merged {
            if previous.format != read.format
                || previous.text != read.text
                || previous.payload_bytes != read.payload_bytes
                || previous.addon != read.addon
                || previous.gs1.unwrap_or(false) != read.gs1.unwrap_or(false)
                || previous.reader_initialization.unwrap_or(false)
                    != read.reader_initialization.unwrap_or(false)
                || previous.structured_append != read.structured_append
                || !matches!(
                    read.format.as_str(),
                    "Code128" | "Code39" | "ITF" | "Code93"
                )
            {
                continue;
            }
            ambiguous |= index >= established
                && crate::geometry::overlap_quads(&previous.polygon, &read.polygon).0 > 0.;
            if let Some(quad) = decode_bridge(
                previous,
                &read,
                im,
                sampler,
                strength,
                &mut remaining,
                &mut rows,
            ) {
                previous.polygon = quad;
                previous.support = previous.support.max(read.support);
                used = true;
                break;
            }
        }
        if !used {
            if ambiguous {
                deferred.push(Region {
                    format: read.format,
                    text: read.text,
                    polygon: read.polygon,
                    support: read.support,
                    localization_score: None,
                });
            } else {
                merged.push(read);
            }
        }
    }
    (merged, deferred)
}
fn decode_bridge(
    a: &Read,
    b: &Read,
    im: ImageView<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
    remaining: &mut usize,
    rows: &mut usize,
) -> Option<Quad> {
    decode_bridge_path(a, b, im, sampler, strength, remaining, rows, false)
        .or_else(|| decode_bridge_path(a, b, im, sampler, strength, remaining, rows, true))
}
#[expect(
    clippy::too_many_arguments,
    reason = "Source geometry, sampling and shared budgets define one bounded physical proof."
)]
fn decode_bridge_path(
    a: &Read,
    b: &Read,
    im: ImageView<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
    remaining: &mut usize,
    rows: &mut usize,
    facing_edges: bool,
) -> Option<Quad> {
    let aq = a.polygon;
    let mut bq = b.polygon;
    let mut am = [point(aq, 0., 0.5), point(aq, 1., 0.5)];
    let mut bm = [point(bq, 0., 0.5), point(bq, 1., 0.5)];
    let av = [am[1][0] - am[0][0], am[1][1] - am[0][1]];
    let mut bv = [bm[1][0] - bm[0][0], bm[1][1] - bm[0][1]];
    let aw = av[0].hypot(av[1]);
    let bw = bv[0].hypot(bv[1]);
    if aw < 24. || !(0.85..=1.15).contains(&(bw / aw)) {
        return None;
    }
    if av[0] * bv[0] + av[1] * bv[1] < 0. {
        bq = [bq[2], bq[3], bq[0], bq[1]];
        bm = [bm[1], bm[0]];
        bv = [-bv[0], -bv[1]];
    }
    if (av[0] * bv[0] + av[1] * bv[1]) / (aw * bw) < 0.98 {
        return None;
    }
    let ac = point(aq, 0.5, 0.5);
    let bc = point(bq, 0.5, 0.5);
    if ((bc[0] - ac[0]) * av[0] + (bc[1] - ac[1]) * av[1]).abs() / aw > aw * 0.05 {
        return None;
    }
    // These edges are actual confirmed first/last decode rows. Existing band
    // continuity already owns their interiors; the extra proof covers the gap.
    if facing_edges {
        if (bc[1] - ac[1]) * av[0] - (bc[0] - ac[0]) * av[1] >= 0. {
            am = [aq[3], aq[2]];
            bm = [bq[0], bq[1]];
        } else {
            am = [aq[0], aq[1]];
            bm = [bq[3], bq[2]];
        }
    }
    let distance = (am[0][0] - bm[0][0])
        .hypot(am[0][1] - bm[0][1])
        .max((am[1][0] - bm[1][0]).hypot(am[1][1] - bm[1][1]));
    if !(0.0..=96.).contains(&distance) {
        return None;
    }
    let steps = barcode_research_core::numeric::f64_usize((distance * 2.).ceil()).max(1);
    for step in 0..=steps {
        if *rows == 0 || *remaining < 1536 {
            return None;
        }
        *rows -= 1;
        let f = usize_f64(step) / usize_f64(steps);
        let left = lerp(am[0], bm[0], f);
        let right = lerp(am[1], bm[1], f);
        if !bridge_line(im, left, right, a, sampler, strength, remaining) {
            return None;
        }
    }
    let cross = |p: [f64; 2]| (-av[1] * p[0] + av[0] * p[1]) / aw;
    let top = if cross(point(aq, 0.5, 0.)) < cross(point(bq, 0.5, 0.)) {
        aq
    } else {
        bq
    };
    let bottom = if cross(point(aq, 0.5, 1.)) > cross(point(bq, 0.5, 1.)) {
        aq
    } else {
        bq
    };
    Some([top[0], top[1], bottom[2], bottom[3]])
}

fn bridge_line(
    im: ImageView<'_>,
    left: [f64; 2],
    right: [f64; 2],
    expected: &Read,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    strength: f32,
    remaining: &mut usize,
) -> bool {
    let mask = match expected.format.as_str() {
        "Code128" => 16,
        "Code39" => 32,
        "ITF" => 64,
        "Code93" => 256,
        _ => return false,
    };
    // Source-position phases alias differently across bounded oversampling grids.
    // Each hypothesis must validate the same complete payload and endpoints.
    for density in [DENSITY, 3., 2., 4., 6.] {
        if *remaining < 1536 {
            return false;
        }
        sampler.sample_limited(
            im,
            lerp(left, right, -0.15),
            lerp(left, right, 1.15),
            density,
            1536,
        );
        *remaining = remaining.saturating_sub(sampler.samples);
        if !sampler.has_contrast() {
            return false;
        }
        for method in 0..if mask == 32 { 3 } else { 6 } {
            if method == 3 {
                sampler.restore_contrast(strength);
            }
            if method % 3 == 1 {
                sampler.adaptive_threshold();
            } else {
                sampler.threshold(method % 3 == 0);
            }
            for reverse in [false, true] {
                if reverse {
                    sampler.runs.reverse();
                }
                let first = if reverse {
                    sampler.first_black ^ (sampler.runs.len().is_multiple_of(2))
                } else {
                    sampler.first_black
                };
                let found = linear::decode(&sampler.runs, first, mask);
                if reverse {
                    sampler.runs.reverse();
                }
                for read in found {
                    if read.text != expected.text || read.format != expected.format {
                        continue;
                    }
                    let (start, end) = if reverse {
                        (
                            sampler.runs.len() - read.end,
                            sampler.runs.len() - read.start,
                        )
                    } else {
                        (read.start, read.end)
                    };
                    let left = -0.15
                        + 1.3 * f64::from(sampler.edges[start]) / usize_f64(sampler.samples - 1);
                    let right = -0.15
                        + 1.3 * f64::from(sampler.edges[end]) / usize_f64(sampler.samples - 1);
                    if left.abs() <= 0.035 && (right - 1.).abs() <= 0.035 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(feature = "medium")]
pub(crate) fn recover_proposals_band(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    offset: f64,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_variant(
        image,
        proposals,
        mask,
        sampler,
        SourceProfile::BandEan(offset),
    )
}

#[cfg(all(test, feature = "medium"))]
mod band_probe_tests;
#[cfg(test)]
mod bridge_tests;
