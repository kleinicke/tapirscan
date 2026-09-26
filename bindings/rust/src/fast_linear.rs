//! Public Low oriented linear fast path. Deliberately bounded, always reports deferred work.
use crate::{
    read::{Read, ReaderPayload, Region},
    Error, Image, ImageView, Quad, ScanOptions, Scanner,
};
use barcode_multiformat::linear;
use barcode_research_core::numeric::usize_f64;

// Build-only research selection, deliberately absent from every public API.
// Cargo tracks this environment input; artifact manifests must record it.
#[cfg(feature = "low")]
const TIER: usize = match option_env!("TAPIRSCAN_TURBO_TIER") {
    Some(value) => match value.as_bytes() {
        [b'2'] => 2,
        [b'4'] => 4,
        [b'8'] => 8,
        [b'1', b'6'] => 16,
        [b'0'] => 0,
        _ => panic!("unsupported private Turbo tier"),
    },
    None => 0,
};
#[cfg(not(feature = "low"))]
const TIER: usize = 0;

#[cfg(feature = "low")]
pub(crate) fn enabled(
    options: ScanOptions,
    mask: u32,
    addons: crate::formats::EanAddOnPolicy,
) -> bool {
    crate::LOW_FAST_PATH
        && mask & crate::format_registry::LINEAR_MASK != 0
        && addons == crate::formats::EanAddOnPolicy::Ignore
        && !options.finish_candidates
}

// Selected policies; the earlier exploratory variants remain in experiment commits.
const ROWS: &[f64] = match TIER {
    16 => &[0.3, 0.5, 0.7],
    4 => &[0.08, 0.29, 0.5, 0.71, 0.92],
    8 => &[0.2, 0.35, 0.5, 0.65, 0.8],
    _ => &[
        0.08, 0.15, 0.22, 0.29, 0.36, 0.43, 0.5, 0.57, 0.64, 0.71, 0.78, 0.85, 0.92,
    ],
};
const AXIS_ROWS: &[f64] = if TIER == 16 {
    &[0.4, 0.5, 0.6]
} else {
    &[0.35, 0.5, 0.65]
};
const WORKING_DIMENSION: f64 = match TIER {
    16 => 128.,
    4 => 384.,
    8 => 320.,
    _ => 768.,
};
// Preserve small source modules; normalized caps bound work on large symbols.
const DENSITY: f64 = 1.5;
const SAMPLE_LIMIT: usize = match TIER {
    2 => 3072,
    4 | 8 | 16 => 2048,
    _ => 4096,
};
const ROW_GAP: f64 = match TIER {
    4 | 16 => 0.211,
    8 => 0.151,
    _ => 0.141,
};

struct Observation {
    read: Read,
    left: f64,
    right: f64,
    first: f64,
    last: f64,
    anchor: f64,
    min_span: f64,
    last_row: usize,
    seen_rows: u64,
    dense: bool,
}
fn required_support(format: &str) -> u64 {
    if matches!(format, "ITF" | "Code39" | "Codabar") {
        3
    } else {
        2
    }
}
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn point(q: Quad, u: f64, v: f64) -> [f64; 2] {
    lerp(lerp(q[0], q[1], u), lerp(q[3], q[2], u), v)
}
fn quad(base: Quad, left: f64, right: f64, top: f64, bottom: f64) -> Quad {
    [
        point(base, left, top),
        point(base, right, top),
        point(base, right, bottom),
        point(base, left, bottom),
    ]
}

fn full_frame_proposals(image: Image<'_>) -> [crate::Proposal; 2] {
    // Sparse horizontal and vertical discovery remains available even without a proposal.
    let w = usize_f64(image.width - 1);
    let h = usize_f64(image.height - 1);
    [
        crate::Proposal {
            polygon: [[0., 0.], [w, 0.], [w, h], [0., h]],
            score: 0.,
        },
        crate::Proposal {
            polygon: [[0., h], [0., 0.], [w, 0.], [w, h]],
            score: 0.,
        },
    ]
}

#[cfg(feature = "low")]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the original Low scan stages and accounting in their established order."
)]
pub(crate) fn scan(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    mask: u32,
) -> Result<scanner_types::EngineScan, Error> {
    let im = ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )?;
    let localized = if TIER != 0 {
        scanner.localizer.detect_sparse(im, WORKING_DIMENSION)?
    } else {
        scanner.localizer.detect_fast(im, WORKING_DIMENSION)?
    };
    let mut proposals = localized.proposals;
    let local_count = proposals.len();
    proposals.extend(full_frame_proposals(image));
    let mut reads = Vec::new();
    let mut refined_reads = Vec::new();
    let mut unread = Vec::new();
    let mut lines = 0;
    let mut continuity_budget = 524_288;
    let mut refinement_budget = 131_072;
    let mut refinement_lines = 0;
    let mut dense_budget = 131_072;
    let mut dense_lines = 0;
    let mut rescue_axes = false;
    for (index, proposal) in proposals.iter().enumerate() {
        if index == local_count {
            rescue_axes = reads.is_empty() && refined_reads.is_empty();
        }
        if matches!(TIER, 8 | 16) && proposal.score == 0. && !rescue_axes {
            continue;
        }
        let q = proposal.polygon;
        let mut candidate = Candidate {
            image,
            im,
            quad: q,
            dense: false,
            contrast: 0.,
            localized: proposal.score > 0. || matches!(TIER, 8 | 16),
            mask: mask & crate::format_registry::LINEAR_MASK,
            remaining: continuity_budget,
            observations: Vec::new(),
            row_positions: Vec::new(),
        };
        // A bounded normalized axis retry recovers large clean symbols when sparse
        // localization clips their start/stop bars. Both axes run after an empty local pass.
        let rows = if matches!(TIER, 8 | 16) && proposal.score == 0. {
            AXIS_ROWS
        } else {
            ROWS
        };
        for (row, &v) in rows.iter().enumerate() {
            sample_line(&mut candidate, &mut scanner.fast_profiles, row, v);
            lines += 1;
        }
        continuity_budget = candidate.remaining;
        let refined_lines = if matches!(TIER, 8 | 16) && proposal.score == 0. {
            0
        } else {
            candidate.refine(
                &mut scanner.fast_profiles,
                &mut refinement_budget,
                &mut refinement_lines,
                &mut dense_budget,
                &mut dense_lines,
            )
        };
        lines += refined_lines;
        let before = reads.len() + refined_reads.len();
        candidate.append_confirmed(refined_lines > 0, &mut reads, &mut refined_reads);
        if options.include_regions
            && proposal.score > 0.
            && reads.len() + refined_reads.len() == before
        {
            unread.push(Region::unknown(q));
        }
    }
    reads.extend(refined_reads);
    let (border_reads, border_lines) = if matches!(TIER, 8 | 16) {
        (Vec::new(), 0)
    } else {
        border_discovery(scanner, image, im, &proposals[proposals.len() - 2..], mask)
    };
    reads.extend(border_reads);
    lines += border_lines;
    if mask & !127 != 0 {
        let (additional, regions) = crate::formats::fast_additional(scanner, image, options, mask)?;
        reads.extend(additional);
        unread.extend(regions);
    }
    let reads = finalize_reads(reads, &mut unread, image, mask, options.multiple);
    let raw = options.retain_diagnostics.then(|| {
        diagnostics(
            image,
            options,
            &proposals[..local_count],
            localized.limited,
            lines,
            &unread,
        )
    });
    crate::formats::typed_result(reads, unread, true, localized.limited, raw)
}

#[cfg(feature = "low")]
fn finalize_reads(
    reads: Vec<Read>,
    unread: &mut Vec<Region>,
    image: Image<'_>,
    mask: u32,
    multiple: bool,
) -> Vec<Read> {
    let mut reads = crate::linear_duplicates::merge_fast(reads, image);
    if mask & !127 != 0 {
        remove_decoded_regions(unread, &reads);
    }
    snap_integer_coordinates(&mut reads);
    reads.sort_by_key(|r| std::cmp::Reverse(r.support));
    if !multiple {
        reads.truncate(1);
    }
    reads
}

#[cfg(feature = "low")]
fn diagnostics(
    image: Image<'_>,
    options: ScanOptions,
    proposals: &[crate::Proposal],
    limited: bool,
    lines: usize,
    unread: &[Region],
) -> serde_json::Value {
    let mut raw = serde_json::json!({
        "schemaVersion": 2, "mode": "low", "policy": "low-fast", "tier": TIER,
        "multiple": options.multiple, "elapsedMs": 0.,
        "localizationLimited": limited, "lines": lines,
        "scan": {"barcodes": [], "unfinished": true}
    });
    if options.include_regions {
        raw["localization"] = serde_json::json!({
            "proposals": proposals.iter().map(|p| serde_json::json!({
                "polygon": p.polygon, "score": p.score, "text": ""
            })).collect::<Vec<_>>(),
            "omitted": null, "workLimited": limited
        });
        raw["searchWindows"] = serde_json::json!([{
            "kind": "full_frame_search", "candidateIndex": proposals.len(),
            "polygon": [[0., 0.], [usize_f64(image.width), 0.],
                [usize_f64(image.width), usize_f64(image.height)], [0., usize_f64(image.height)]]
        }]);
        raw["scan"]["regions"] = serde_json::json!(unread);
        // Fast profiles do not produce the core reader's per-candidate diagnostics.
        raw["scan"]["candidates"] = serde_json::json!([]);
        raw["scan"]["candidateDetailsAvailable"] = serde_json::json!(false);
    }
    raw
}

fn snap_integer_coordinates(reads: &mut [Read]) {
    // Floating-point trig differs at a few ulps across native and WASM.
    // Snap only coordinates indistinguishable from an integer before rectangle rounding.
    if TIER != 0 {
        for read in reads {
            for coordinate in read.polygon.iter_mut().flatten() {
                if (*coordinate - coordinate.round()).abs() < 1e-9 {
                    *coordinate = coordinate.round();
                }
            }
        }
    }
}

fn remove_decoded_regions(unread: &mut Vec<Region>, reads: &[Read]) {
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| crate::geometry::overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
}

/// Sparse original rows miss symbols confined to the outer eight percent.
/// Probe both borders of both full-frame axes without another localization pass.
/// New reads need separated source-row evidence and never outrank ordinary reads.
fn border_discovery(
    scanner: &mut Scanner,
    image: Image<'_>,
    im: ImageView<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
) -> (Vec<Read>, usize) {
    let mut reads = Vec::new();
    let mut budget = 131_072;
    let mut lines = 0;
    let evidence = scanner.localizer.border_stripe_evidence();
    for (axis, proposal) in proposals.iter().enumerate() {
        let q = proposal.polygon;
        let a = point(q, 0.5, 0.);
        let b = point(q, 0.5, 1.);
        let height = (b[0] - a[0]).hypot(b[1] - a[1]);
        let scale = (height / 256.).min(1.);
        for far in [false, true] {
            if !evidence[axis * 2 + usize::from(far)] {
                continue;
            }
            let mut candidate = Candidate {
                image,
                im,
                quad: q,
                dense: false,
                contrast: 0.,
                localized: false,
                mask: mask & crate::format_registry::LINEAR_MASK,
                remaining: budget,
                observations: Vec::new(),
                row_positions: Vec::new(),
            };
            for (row, offset) in [2., 6., 14., 30.].into_iter().enumerate() {
                let fraction = offset * scale / height;
                sample_line(
                    &mut candidate,
                    &mut scanner.fast_profiles,
                    row,
                    if far { 1. - fraction } else { fraction },
                );
                lines += 1;
            }
            budget = candidate.remaining;
            for mut observation in candidate.observations {
                let span = (observation.last - observation.first) * height;
                if observation.read.support < 3 || span < observation.min_span {
                    continue;
                }
                observation.read.polygon = quad(
                    q,
                    observation.left,
                    observation.right,
                    observation.first,
                    observation.last,
                );
                observation.read.support = required_support(&observation.read.format);
                reads.push(observation.read);
            }
        }
    }
    (reads, lines)
}

struct Candidate<'a> {
    image: Image<'a>,
    im: ImageView<'a>,
    quad: Quad,
    mask: u32,
    dense: bool,
    contrast: f32,
    localized: bool,
    remaining: usize,
    observations: Vec<Observation>,
    row_positions: Vec<f64>,
}
impl Candidate<'_> {
    fn append_confirmed(
        self,
        recovery: bool,
        reads: &mut Vec<Read>,
        refined_reads: &mut Vec<Read>,
    ) {
        let q = self.quad;
        for mut o in self.observations {
            // ITF and ordinary Code39 lack mandatory payload checksums; enhanced
            // profiles require a fourth independent row before acceptance.
            let required =
                if self.contrast > 0. && matches!(o.read.format.as_str(), "ITF" | "Code39") {
                    4
                } else {
                    required_support(&o.read.format)
                };
            // Close recovery rows are more correlated than the original grid.
            // Require three confirmations even for checksum-protected formats.
            let confirmation = if recovery {
                required.max(if o.dense { 4 } else { 3 })
            } else {
                required
            };
            let top = point(q, 0.5, o.first);
            let bottom = point(q, 0.5, o.last);
            let span = (bottom[0] - top[0]).hypot(bottom[1] - top[1]);
            if o.read.support < confirmation || (recovery && span < o.min_span) {
                continue;
            }
            o.read.polygon = quad(q, o.left, o.right, o.first, o.last);
            if recovery {
                // Nearby confirmation must not outrank broad, original evidence.
                o.read.support = required;
                refined_reads.push(o.read);
            } else {
                reads.push(o.read);
            }
        }
    }

    fn refine(
        &mut self,
        sampler: &mut barcode_research_core::fast_profile::Sampler,
        budget: &mut usize,
        used: &mut usize,
        dense_budget: &mut usize,
        dense_used: &mut usize,
    ) -> usize {
        if self
            .observations
            .iter()
            .any(|o| o.read.support >= required_support(&o.read.format))
        {
            return 0;
        }
        let before = *used;
        let top = point(self.quad, 0.5, 0.);
        let bottom = point(self.quad, 0.5, 1.);
        let height = (bottom[0] - top[0]).hypot(bottom[1] - top[1]);
        // Revisit only actual decoder evidence. Close, independent rows can
        // confirm a thin or damaged symbol without densifying every proposal.
        let mut anchors: Vec<(f64, f64)> = Vec::new();
        for observation in &self.observations {
            let delta = 0.018_f64.max(0.6 * observation.min_span / height);
            if delta <= 0.07
                && !anchors
                    .iter()
                    .any(|&(v, _)| (v - observation.anchor).abs() < 1e-9)
            {
                anchors.push((observation.anchor, delta));
            }
            if anchors.len() == 4 {
                break;
            }
        }
        let mut retry_rows = Vec::new();
        self.remaining = *budget;
        for (anchor, delta) in anchors {
            for offset in [-delta, delta] {
                if *used
                    >= if TIER == 16 {
                        4
                    } else if TIER == 8 {
                        8
                    } else {
                        32
                    }
                {
                    break;
                }
                retry_rows.push((13 + *used, anchor + offset));
                sample_line(self, sampler, 13 + *used, anchor + offset);
                *used += 1;
            }
        }
        *budget = self.remaining;
        let dense_before = *dense_used;
        if TIER < 4
            && !retry_rows.is_empty()
            && !self
                .observations
                .iter()
                .any(|o| o.read.support >= 3 && (o.last - o.first) * height >= o.min_span)
        {
            self.remaining = *dense_budget;
            self.dense = true;
            retry_rows.extend(
                [
                    0.08, 0.15, 0.22, 0.29, 0.36, 0.43, 0.5, 0.57, 0.64, 0.71, 0.78, 0.85, 0.92,
                ]
                .into_iter()
                .enumerate(),
            );
            for (row, v) in retry_rows {
                if *dense_used >= 32 {
                    break;
                }
                let a = point(self.quad, -0.15, v);
                let b = point(self.quad, 1.15, v);
                if (b[0] - a[0]).hypot(b[1] - a[1]) * 1.5 >= 4096. {
                    continue;
                }
                sample_line_density(self, sampler, row, v, true);
                *dense_used += 1;
            }
            *dense_budget = self.remaining;
        }
        *used - before + *dense_used - dense_before
    }

    fn admit(&mut self, found: linear::Read, l: f64, r: f64, _row: usize, v: f64) {
        // Recovery grids can assign different loop indices to the same physical
        // row. Give each source position one stable identity across all passes.
        let row = if let Some(index) = self
            .row_positions
            .iter()
            .position(|old| (old - v).abs() < 1e-9)
        {
            index
        } else {
            if self.row_positions.len() >= 64 {
                return;
            }
            let index = self.row_positions.len();
            self.row_positions.push(v);
            index
        };
        let q = self.quad;
        if let Some(old) = self.observations.iter_mut().find(|o| {
            o.read.text == found.text
                && o.read.format == found.format
                && (o.left - l).abs() < 0.06
                && (o.right - r).abs() < 0.06
                && (v - o.anchor).abs() <= ROW_GAP
                && (o.last_row == row
                    || crate::linear_duplicates::connect_fast(
                        quad(q, o.left, o.right, o.anchor - 0.001, o.anchor + 0.001),
                        quad(q, l, r, v - 0.001, v + 0.001),
                        self.image,
                        &mut self.remaining,
                    ))
        }) {
            if old.seen_rows & (1_u64 << row) == 0 {
                old.seen_rows |= 1_u64 << row;
                old.read.support += 1;
                old.dense |= self.dense;
                old.first = old.first.min(v);
                old.last = old.last.max(v);
                old.anchor = v;
                old.last_row = row;
            }
            return;
        }
        let left = point(q, l, v);
        let right = point(q, r, v);
        let width = (right[0] - left[0]).hypot(right[1] - left[1]);
        // Two average run widths span roughly three narrow modules. This also
        // prevents subpixel rows from being treated as independent confirmations.
        let min_span =
            (2. * width / usize_f64(found.end.saturating_sub(found.start).max(1))).max(2.);
        self.observations.push(Observation {
            min_span,
            left: l,
            right: r,
            first: v,
            last: v,
            anchor: v,
            last_row: row,
            seen_rows: 1_u64 << row,
            dense: self.dense,
            read: Read {
                text: found.text,
                format: found.format.into(),
                polygon: q,
                support: 1,
                axis: Some(0),
                candidate_indices: None,
                payload_bytes: None,
                addon: None,
                gs1: Some(found.gs1),
                reader_initialization: None,
                structured_append: None,
                payload: ReaderPayload {
                    error: Some(f64::from(found.error)),
                },
                rank: None,
            },
        });
    }
}
fn sample_line(
    candidate: &mut Candidate<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    row: usize,
    v: f64,
) {
    sample_line_density(candidate, sampler, row, v, false);
}
#[expect(
    clippy::many_single_char_names,
    reason = "Coordinates of one sampling line."
)]
fn sample_line_density(
    candidate: &mut Candidate<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    row: usize,
    v: f64,
    dense: bool,
) {
    let q = candidate.quad;
    let im = candidate.im;
    let a = point(q, -0.15, v);
    let b = point(q, 1.15, v);
    if dense {
        sampler.sample_dense(im, a, b);
    } else {
        let limit = if candidate.localized {
            match TIER {
                2 => 1024,
                4 => 768,
                16 => 512,
                8 => match option_env!("TAPIRSCAN_TURBO_PROFILE_CAP") {
                    Some(value) if value.as_bytes() == b"576" => 576,
                    Some(value) if value.as_bytes() == b"640" => 640,
                    _ => 512,
                },
                _ => SAMPLE_LIMIT,
            }
        } else {
            SAMPLE_LIMIT
        };
        sampler.sample_limited(im, a, b, DENSITY, limit);
    }
    if !sampler.has_contrast() {
        return;
    }

    let enhanced = candidate.contrast > 0.;
    let methods = if enhanced { 6 } else { 3 };
    for method in 0..methods {
        if method == 3 {
            sampler.restore_contrast(candidate.contrast);
        }
        // Code39 has no mandatory checksum. Restored narrow bars can turn H
        // into B consistently across rows, so only original profiles may add it.
        let active_mask = if method >= 3 {
            candidate.mask & !32
        } else {
            candidate.mask
        };
        if active_mask == 0 {
            break;
        }
        let method = method % 3;
        let mut decoded_wide = false;
        if method == 1 {
            sampler.adaptive_threshold();
        } else {
            sampler.threshold(method == 0);
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
            let mut decoded = linear::decode(&sampler.runs, first, active_mask);
            supplement_ean(&sampler.runs, first, active_mask, &mut decoded);
            let endpoints = if reverse { [b, a] } else { [a, b] };
            decoded.retain(|read| {
                source_quiet(&sampler.runs, read, im, endpoints)
                    && (candidate.contrast <= 0.
                        || read.format != "Code39"
                        || code39_source_resolution(
                            &sampler.runs,
                            read,
                            endpoints,
                            sampler.samples,
                        ))
            });
            if reverse {
                sampler.runs.reverse();
            }
            for found in decoded {
                let (start, end) = if reverse {
                    (
                        sampler.runs.len() - found.end,
                        sampler.runs.len() - found.start,
                    )
                } else {
                    (found.start, found.end)
                };
                let l =
                    -0.15 + 1.3 * f64::from(sampler.edges[start]) / usize_f64(sampler.samples - 1);
                let r =
                    -0.15 + 1.3 * f64::from(sampler.edges[end]) / usize_f64(sampler.samples - 1);
                decoded_wide |= r - l >= 0.5;
                candidate.admit(found, l, r, row, v);
            }
        }
        // Every row and candidate still receives its first all-symbol decode.
        // Narrow reads leave room for another barcode, so retain alternate thresholds.
        if decoded_wide {
            break;
        }
    }
}

/// Additional unchecked Code39 recovery needs two source pixels per narrow
/// element. Multiple rows cannot disambiguate systematic subpixel aliases.
fn code39_source_resolution(
    runs: &[f32],
    read: &linear::Read,
    endpoints: [[f64; 2]; 2],
    samples: usize,
) -> bool {
    let Some(segment) = runs.get(read.start..read.end) else {
        return false;
    };
    if segment.is_empty() || samples < 2 {
        return false;
    }
    let mut widths = segment.to_vec();
    let index = widths.len() / 3;
    let (_, narrow, _) = widths.select_nth_unstable_by(index, f32::total_cmp);
    let span = (endpoints[1][0] - endpoints[0][0]).hypot(endpoints[1][1] - endpoints[0][1]);
    f64::from(*narrow) * span / usize_f64(samples - 1) >= 2.
}

/// A proposal endpoint inside the image cannot stand in for a cropped image edge.
/// Four-digit ITF has little redundancy: require its actual interior quiet zones.
/// Longer values retain the decoder's cropped-zone tolerance.
fn source_quiet(
    runs: &[f32],
    read: &linear::Read,
    image: ImageView<'_>,
    endpoints: [[f64; 2]; 2],
) -> bool {
    if read.format != "ITF" || read.text.len() > if TIER == 16 { 6 } else { 4 } {
        return true;
    }
    let module = runs[read.start..read.start + 4].iter().sum::<f32>() / 4.;
    let boundary = |point: [f64; 2]| {
        point[0] <= 0.
            || point[1] <= 0.
            || point[0] >= usize_f64(image.width - 1)
            || point[1] >= usize_f64(image.height - 1)
    };
    let leading = read.start > 0 && runs[read.start - 1] >= 7. * module
        || read.start == 1 && boundary(endpoints[0]);
    let trailing = read.end < runs.len() && runs[read.end] >= 7. * module
        || read.end + 1 >= runs.len() && boundary(endpoints[1]);
    leading && trailing
}

fn supplement_ean(runs: &[f32], first_black: bool, mask: u32, out: &mut Vec<linear::Read>) {
    if mask & (linear::EAN13 | linear::UPCA) == 0 {
        return;
    }
    for start in (usize::from(!first_black)..runs.len().saturating_sub(59)).step_by(2) {
        let end = start + 59;
        if start == 0 || out.iter().any(|r| r.start < end && r.end > start) {
            continue;
        }
        let widths = &runs[start..end];
        let module = widths.iter().sum::<f32>() / 95.;
        if runs[start - 1] < 5. * module || runs[end] < 5. * module {
            continue;
        }
        if let Some(e) = barcode_research_core::run_ean::decode_evidence(widths) {
            let mut text: String = e.digits.iter().map(|d| char::from(b'0' + d)).collect();
            let format = if e.digits[0] == 0 && mask & 2 != 0 {
                text.remove(0);
                "UPCA"
            } else if mask & 1 != 0 {
                "EAN13"
            } else {
                continue;
            };
            out.push(linear::Read {
                decoded: true,
                addon: None,
                format,
                text,
                start,
                end,
                error: 0.,
                gs1: false,
            });
        }
    }
}

#[cfg(all(test, feature = "low"))]
mod tests {
    use super::*;

    #[test]
    fn public_low_selection_preserves_explicit_recovery_requests() {
        use crate::formats::EanAddOnPolicy;
        let basic = ScanOptions::default();
        assert_eq!(
            enabled(basic, 1, EanAddOnPolicy::Ignore),
            crate::LOW_FAST_PATH
        );
        assert!(!enabled(basic, 512, EanAddOnPolicy::Ignore));
        assert!(!enabled(basic, 1, EanAddOnPolicy::Read));
        assert!(!enabled(basic, 1, EanAddOnPolicy::Require));
        assert!(!enabled(
            ScanOptions {
                finish_candidates: true,
                ..basic
            },
            1,
            EanAddOnPolicy::Ignore
        ));
    }

    /// Independent ZXing-writer module patterns, reproduced without an encoder dependency.
    /// Small UPC-E modules previously aliased into another checksum-valid payload; a
    /// large EAN-8 exposed a clipped sparse proposal and needs the bounded axis retry.
    #[test]
    fn source_density_and_axis_retry_preserve_clean_retail_values() {
        let upce = b"101001110100100110111001001101101011110011001010101";
        let ean8 = b"1010001011010111101111010110111010101001110111001010001001011100101";
        for (bits, scale, padding, expected) in [
            (&upce[..], 1, 20, "04252614"),
            (&upce[..], 1, 60, "04252614"),
            (&ean8[..], 6, 60, "96385074"),
        ] {
            for rotated in [false, true] {
                let width = bits.len() * scale + 2 * padding;
                let height = 55 * scale + 2 * padding;
                let (output_width, output_height) = if rotated {
                    (height, width)
                } else {
                    (width, height)
                };
                let mut pixels = vec![255; width * height];
                for y in padding..height - padding {
                    for x in padding..width - padding {
                        let at = if rotated {
                            x * output_width + height - 1 - y
                        } else {
                            y * width + x
                        };
                        pixels[at] = if bits[(x - padding) / scale] == b'1' {
                            0
                        } else {
                            255
                        };
                    }
                }
                let image = Image {
                    data: &pixels,
                    width: output_width,
                    height: output_height,
                    channels: 1,
                    stride: output_width,
                };
                let result =
                    scan(&mut Scanner::default(), image, ScanOptions::default(), 127).unwrap();
                assert_eq!(
                    result
                        .barcodes
                        .iter()
                        .map(|read| read.text.as_str())
                        .collect::<Vec<_>>(),
                    [expected],
                    "tier={TIER}, scale={scale}, padding={padding}, rotated={rotated}"
                );
                assert!(result.unfinished);
            }
        }
    }

    #[test]
    fn short_clean_bars_keep_three_rows_and_do_not_invent_itf() {
        if TIER != 16 {
            return;
        }
        let itf =
            b"101011101000101011100011101110100010100011101000111000101010001010111000111011101";
        let code39 = b"1001011011010101101011001011011010010101101010010110101101010011011010110010101011001010110101001101011010100110110101011001011010100101101101";
        for (bits, scale, padding, expected) in [
            (&itf[..], 1, 60, "12345678"),
            (&code39[..], 1, 60, "SCALE2409"),
            (&code39[..], 4, 60, "SCALE2409"),
        ] {
            for rotated in [false, true] {
                let width = bits.len() * scale + 2 * padding;
                let height = 50 * scale + 2 * padding;
                let (output_width, output_height) = if rotated {
                    (height, width)
                } else {
                    (width, height)
                };
                let mut pixels = vec![255; width * height];
                for y in padding..height - padding {
                    for x in padding..width - padding {
                        let at = if rotated {
                            x * output_width + height - 1 - y
                        } else {
                            y * width + x
                        };
                        pixels[at] = if bits[(x - padding) / scale] == b'1' {
                            0
                        } else {
                            255
                        };
                    }
                }
                let image = Image {
                    data: &pixels,
                    width: output_width,
                    height: output_height,
                    channels: 1,
                    stride: output_width,
                };
                let result =
                    scan(&mut Scanner::default(), image, ScanOptions::default(), 127).unwrap();
                assert_eq!(
                    result
                        .barcodes
                        .iter()
                        .map(|read| read.text.as_str())
                        .collect::<Vec<_>>(),
                    [expected],
                    "tier={TIER}, scale={scale}, padding={padding}, rotated={rotated}"
                );
                assert!(result.unfinished);
            }
        }
    }

    #[test]
    fn nearby_confirmation_counts_distinct_rows_and_preserves_extent() {
        let mut pixels = vec![255; 100 * 60];
        for y in 0..60 {
            for x in 10..90 {
                if x / 3 % 2 == 0 {
                    pixels[y * 100 + x] = 0;
                }
            }
        }
        let image = Image {
            data: &pixels,
            width: 100,
            height: 60,
            channels: 1,
            stride: 100,
        };
        let mut candidate = Candidate {
            image,
            im: ImageView::new(&pixels, 100, 60, 1, 100).unwrap(),
            quad: [[10., 10.], [90., 10.], [90., 50.], [10., 50.]],
            mask: linear::CODE128,
            localized: true,
            dense: false,
            contrast: 0.,
            remaining: 100_000,
            observations: Vec::new(),
            row_positions: Vec::new(),
        };
        let read = || linear::Read {
            decoded: true,
            addon: None,
            format: "Code128",
            text: "example".into(),
            start: 0,
            end: 40,
            error: 0.,
            gs1: false,
        };
        let mut sampler = barcode_research_core::fast_profile::Sampler::default();
        let (mut budget, mut used, mut dense_budget, mut dense_used) = (100_000, 0, 100_000, 0);
        assert_eq!(
            candidate.refine(
                &mut sampler,
                &mut budget,
                &mut used,
                &mut dense_budget,
                &mut dense_used
            ),
            0
        );
        assert_eq!((used, dense_used), (0, 0));
        candidate.admit(read(), 0., 1., 6, 0.5);
        candidate.admit(read(), 0., 1., 19, 0.5);
        assert_eq!(candidate.observations[0].read.support, 1);
        candidate.admit(read(), 0., 1., 6, 0.482);
        candidate.admit(read(), 0., 1., 6, 0.518);
        assert_eq!(candidate.observations.len(), 1);
        candidate.admit(read(), 0., 1., 6, 0.5);
        candidate.admit(read(), 0., 1., 13, 0.482);
        let observation = &candidate.observations[0];
        assert_eq!(observation.read.support, 3);
        assert!((observation.first - 0.482).abs() < 1e-9);
        assert!((observation.last - 0.518).abs() < 1e-9);
    }

    #[test]
    fn truncated_proposal_is_not_a_cropped_image() {
        let pixels = vec![255; 100 * 60];
        let image = ImageView::new(&pixels, 100, 60, 1, 100).unwrap();
        let read = linear::Read {
            decoded: true,
            addon: None,
            format: "ITF",
            text: "0240".into(),
            start: 1,
            end: 8,
            error: 0.,
            gs1: false,
        };
        let mut runs = vec![0.5, 1., 1., 1., 1., 3., 1., 1., 0.5];
        assert!(!source_quiet(&runs, &read, image, [[20., 30.], [80., 30.]]));
        assert!(source_quiet(&runs, &read, image, [[-1., 30.], [100., 30.]]));
        assert!(source_quiet(&runs, &read, image, [[20., -1.], [80., 60.]]));
        runs[0] = 7.;
        runs[8] = 7.;
        assert!(source_quiet(&runs, &read, image, [[20., 30.], [80., 30.]]));
    }

    #[test]
    fn observation_join_respects_single_pixel_separator() {
        fn image(data: &[u8]) -> Image<'_> {
            Image {
                data,
                width: 100,
                height: 60,
                channels: 1,
                stride: 100,
            }
        }
        let mut pixels = vec![255; 100 * 60];
        for y in 0..60 {
            for x in 10..90 {
                if (x / 3) % 2 == 0 {
                    pixels[y * 100 + x] = 0;
                }
            }
        }
        let q = [[10., 10.], [90., 10.], [90., 11.], [10., 11.]];
        let r = [[10., 40.], [90., 40.], [90., 41.], [10., 41.]];
        assert!(crate::linear_duplicates::connect_fast(
            q,
            r,
            image(&pixels),
            &mut 100_000
        ));
        pixels[25 * 100..26 * 100].fill(255);
        assert!(!crate::linear_duplicates::connect_fast(
            q,
            r,
            image(&pixels),
            &mut 100_000
        ));
    }
}

/// Reuse confirmed oriented source profiles for localized regions in Medium, High and Very High.
#[cfg(not(feature = "low"))]
pub(crate) fn recover_proposals(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    recover_proposals_contrast(image, proposals, mask, sampler, 0.)
}
#[cfg(not(feature = "low"))]
pub(crate) fn recover_proposals_contrast(
    image: Image<'_>,
    proposals: &[crate::Proposal],
    mask: u32,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    contrast: f32,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
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
            contrast,
            localized: true,
            remaining: continuity_budget,
            observations: Vec::new(),
            row_positions: Vec::new(),
        };
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
    #[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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

#[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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

#[cfg(not(feature = "low"))]
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

#[cfg(all(test, not(feature = "low")))]
mod bridge_tests {
    use super::*;
    #[test]
    fn full_decoded_bridge_preserves_separate_same_value_labels() {
        // Independent Code39 writer fixture already used by the Low controls.
        let bits=b"1001011011010101101011001011011010010101101010010110101101010011011010110010101011001010110101001101011010100110110101011001011010100101101101";
        let width = bits.len() * 2 + 40;
        let height = 64;
        for separated in [false, true] {
            let mut pixels = vec![255; width * height];
            for y in 8..56 {
                if separated && (31..33).contains(&y) {
                    continue;
                }
                for (x, &bit) in bits.iter().enumerate() {
                    if bit == b'1' {
                        pixels[y * width + 20 + x * 2..y * width + 22 + x * 2].fill(0);
                    }
                }
            }
            let image = Image {
                data: &pixels,
                width,
                height,
                channels: 1,
                stride: width,
            };
            let quad = |top: f64, bottom: f64| {
                [
                    [20., top],
                    [usize_f64(width - 20), top],
                    [usize_f64(width - 20), bottom],
                    [20., bottom],
                ]
            };
            let mut a = Read::primary([0; 13], quad(12., 18.), 4, 0, vec![]);
            a.format = "Code39".into();
            a.text = "SCALE2409".into();
            let mut coincident = a.clone();
            for p in &mut coincident.polygon {
                p[1] += 0.2;
            }
            let mut proof_sampler = barcode_research_core::fast_profile::Sampler::default();
            assert_eq!(
                join_recovered_bands(vec![a.clone(), coincident], image, &mut proof_sampler, 2.)
                    .len(),
                1
            );
            let mut b = a.clone();
            b.polygon = quad(44., 50.);
            let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
            let mut sampler = barcode_research_core::fast_profile::Sampler::default();
            let result = decode_bridge(&a, &b, im, &mut sampler, 2., &mut 131_072, &mut 256);
            assert_eq!(result.is_some(), !separated, "separated={separated}");
            assert!(decode_bridge(&a, &b, im, &mut sampler, 2., &mut 0, &mut 256).is_none());
            let (accepted, deferred) =
                append_recovered(vec![a.clone()], vec![b.clone()], image, &mut sampler, 2.);
            assert_eq!(accepted.len(), if separated { 2 } else { 1 });
            assert!(deferred.is_empty());
            // A conflicting module-axis interpretation cannot establish a
            // second physical instance solely from the same value and overlap.
            let mut uncertain = a.clone();
            uncertain.polygon.rotate_left(1);
            let original = uncertain.polygon;
            let (accepted, deferred) =
                append_recovered(vec![uncertain], vec![a.clone()], image, &mut sampler, 2.);
            assert_eq!(accepted.len(), 1);
            assert_eq!(accepted[0].polygon, original);
            assert_eq!(deferred.len(), 1);
            // A bounded proof may join continuous bands, never the white gap.
            assert_eq!(
                join_recovered_bands(vec![a, b], image, &mut sampler, 2.).len(),
                if separated { 2 } else { 1 }
            );
        }
    }
}
