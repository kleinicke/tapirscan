//! Public Low oriented linear fast path. Deliberately bounded, always reports deferred work.
use crate::{
    geometry::lerp,
    read::{Read, ReaderPayload},
    Image, ImageView, Quad,
};
#[cfg(feature = "low")]
use crate::{read::Region, Error, ScanOptions, Scanner};
use barcode_multiformat::linear;
use barcode_research_core::numeric::usize_f64;

#[cfg(not(feature = "low"))]
mod recovery;
#[cfg(not(feature = "low"))]
pub(crate) use recovery::{
    append_recovered, recover_proposals, recover_proposals_color, recover_proposals_contrast,
    recover_proposals_inverted,
};
#[cfg(feature = "medium")]
pub(crate) use recovery::{
    recover_proposals_band, recover_proposals_highpass, recover_proposals_wide,
};

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
#[cfg(feature = "low")]
const AXIS_ROWS: &[f64] = if TIER == 16 {
    &[0.4, 0.5, 0.6]
} else {
    &[0.35, 0.5, 0.65]
};
#[cfg(feature = "low")]
const WORKING_DIMENSION: f64 = match TIER {
    16 => 128.,
    4 => 384.,
    8 => 320.,
    _ => 768.,
};
// Preserve small source modules; normalized caps bound work on large symbols.
const DENSITY: f64 = 1.5;
/// Running-box half-widths, in profile samples, of the high-pass local mean:
/// boxes of 11, 13 and 11 samples, about a four-pixel Gaussian at `DENSITY`.
const HIGHPASS_BOXES: [usize; 3] = [5, 6, 5];
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
    original_rows: u64,
    dense: bool,
}
fn required_support(format: &str) -> u64 {
    if matches!(format, "ITF" | "Code39" | "Codabar") {
        3
    } else {
        2
    }
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

#[cfg(feature = "low")]
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
        if matches!(TIER, 2 | 4 | 8) {
            scanner
                .localizer
                .detect_sparse_with_recovery(im, WORKING_DIMENSION)?
        } else {
            scanner.localizer.detect_sparse(im, WORKING_DIMENSION)?
        }
    } else {
        scanner.localizer.detect_fast(im, WORKING_DIMENSION)?
    };
    let mut proposals = localized.proposals;
    let additional = if TIER != 0 && proposals.len() > 24 {
        proposals.split_off(24)
    } else {
        Vec::new()
    };
    let local_count = proposals.len();
    proposals.extend(full_frame_proposals(image));
    proposals.extend(additional);
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
    let mut recovery_seed = None;
    for (index, proposal) in proposals.iter().take(local_count + 2).enumerate() {
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
            restored: false,
            profile: SourceProfile::Gray(0.),
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
        let refined_lines = {
            if matches!(TIER, 8 | 16) && proposal.score == 0. {
                // Keep initial observations for every requested format. The new
                // confirmation work excludes unchecksummed Code39 fragments.
                candidate.mask &= linear::EAN13
                    | linear::EAN8
                    | linear::UPCA
                    | linear::UPCE
                    | linear::CODE128
                    | linear::ITF;
            }
            candidate.refine(
                &mut scanner.fast_profiles,
                &mut refinement_budget,
                &mut refinement_lines,
                &mut dense_budget,
                &mut dense_lines,
            )
        };
        lines += refined_lines;
        if TIER != 0
            && recovery_seed.is_none()
            && proposal.score > 0.
            && !candidate.observations.is_empty()
        {
            let top = point(q, 0.5, 0.);
            let bottom = point(q, 0.5, 1.);
            let height = (bottom[0] - top[0]).hypot(bottom[1] - top[1]);
            for o in &candidate.observations {
                let delta = 0.018_f64.max(0.6 * o.min_span / height);
                if o.read.support == 1
                    && o.original_rows != 0
                    && delta <= 0.07
                    && o.anchor >= delta
                    && o.anchor + delta <= 1.
                    && matches!(o.read.format.as_str(), "EAN13" | "UPCA" | "EAN8" | "UPCE")
                {
                    recovery_seed = Some((
                        q,
                        o.anchor,
                        delta,
                        o.read.text.clone(),
                        o.read.format.clone(),
                        o.left,
                        o.right,
                    ));
                    break;
                }
            }
        }
        if matches!(TIER, 8 | 16) && proposal.score == 0. && refined_lines > 0 {
            candidate.observations.retain(|o| {
                // Ineligible observations cannot become outputs, so they need no
                // independent source-template comparison.
                let required = if o.dense { 4 } else { 3 };
                if o.read.support < required {
                    return false;
                }
                retail_recovery_agrees(
                    image,
                    quad(q, o.left, o.right, o.first, o.last),
                    &o.read,
                    &mut scanner.fast_profiles,
                )
            });
        }
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
        border_discovery(
            scanner,
            image,
            im,
            &proposals[local_count..local_count + 2],
            mask,
        )
    };
    reads.extend(border_reads);
    lines += border_lines;
    if mask & !127 != 0 {
        let (additional, regions) = crate::formats::fast_additional(scanner, image, options, mask)?;
        reads.extend(additional);
        unread.extend(regions);
    }
    if reads.is_empty() && matches!(TIER, 2 | 4 | 8) {
        for proposal in proposals.iter().skip(local_count + 2).take(1) {
            let mut candidate = Candidate {
                image,
                im,
                quad: proposal.polygon,
                dense: false,
                restored: false,
                profile: SourceProfile::Gray(0.),
                localized: true,
                mask: mask & crate::format_registry::LINEAR_MASK,
                remaining: continuity_budget,
                observations: Vec::new(),
                row_positions: Vec::new(),
            };
            for (row, &v) in ROWS.iter().enumerate() {
                sample_line(&mut candidate, &mut scanner.fast_profiles, row, v);
                lines += 1;
            }
            let refined = if TIER == 2 {
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
            lines += refined;
            if TIER == 2 {
                // A busy-frame merged proposal needs broad original-row evidence;
                // close retries alone can split a glared label into two reads.
                candidate
                    .observations
                    .retain(|o| o.original_rows.count_ones() >= 4);
            }
            let before = reads.len();
            let mut recovered = Vec::new();
            candidate.append_confirmed(refined > 0, &mut reads, &mut recovered);
            reads.extend(recovered);
            reads.retain(|read| {
                retail_recovery_agrees(image, read.polygon, read, &mut scanner.fast_profiles)
            });
            if options.include_regions && reads.len() == before {
                unread.push(Region::unknown(proposal.polygon));
            }
        }
    }
    if TIER == 0 && reads.is_empty() && mask & 15 != 0 {
        let mut recovered = Vec::new();
        let mut unused = Vec::new();
        let mut remaining = 32_768;
        for proposal in proposals.iter().take(local_count.min(2)) {
            let mut candidate = Candidate {
                image,
                im,
                quad: proposal.polygon,
                dense: false,
                restored: false,
                profile: SourceProfile::Gray(2.25),
                localized: true,
                mask: mask & 15,
                remaining,
                observations: Vec::new(),
                row_positions: Vec::new(),
            };
            for (row, &v) in ROWS.iter().enumerate() {
                sample_line(&mut candidate, &mut scanner.fast_profiles, row, v);
                lines += 1;
            }
            remaining = candidate.remaining;
            candidate.append_confirmed(false, &mut recovered, &mut unused);
        }
        recovered.retain(|read| read.support >= 4);
        let mut accepted = Vec::new();
        for r in recovered {
            if matches!(r.format.as_str(), "EAN13" | "UPCA") {
                let text = if r.text.len() == 12 {
                    format!("0{}", r.text)
                } else {
                    r.text.clone()
                };
                let bytes = text.as_bytes();
                if bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
                    continue;
                }
                let digits = std::array::from_fn(|i| bytes[i] - b'0');
                if !crate::pipeline::recovered_ean_source_agreement(image, r.polygon, &digits)
                    || crate::pipeline::source_contradiction(
                        image,
                        r.polygon,
                        digits,
                        &mut scanner.fast_profiles,
                    )?
                {
                    continue;
                }
            }
            accepted.push(r);
        }
        let recovered = crate::linear_duplicates::merge_fast(accepted, image);
        if recovered.len() == 1 {
            reads.extend(recovered);
        }
    }
    if reads.is_empty() && TIER != 0 {
        if let Some((q, anchor, delta, text, format, left, right)) = recovery_seed {
            let mut candidate = Candidate {
                image,
                im,
                quad: q,
                dense: false,
                restored: false,
                profile: SourceProfile::Gray(1.5),
                localized: true,
                mask: mask & 15,
                remaining: 32768,
                observations: Vec::new(),
                row_positions: Vec::new(),
            };
            for (row, v) in [anchor - delta, anchor, anchor + delta]
                .into_iter()
                .enumerate()
            {
                sample_line(&mut candidate, &mut scanner.fast_profiles, row, v);
                lines += 1;
            }
            candidate.observations.retain(|o| {
                o.read.text == text
                    && o.read.format == format
                    && (o.left - left).abs() < 0.06
                    && (o.right - right).abs() < 0.06
                    && o.read.support >= 3
            });
            let mut recovered = Vec::new();
            candidate.append_confirmed(false, &mut recovered, &mut Vec::new());
            for read in recovered {
                if retail_recovery_agrees(image, read.polygon, &read, &mut scanner.fast_profiles) {
                    reads.push(read);
                }
            }
        }
    }
    let reads = finalize_reads(reads, &mut unread, image, mask, options.multiple);
    let raw = options.retain_diagnostics.then(|| {
        let mut local_proposals = proposals[..local_count].to_vec();
        local_proposals.extend_from_slice(&proposals[local_count + 2..]);
        diagnostics(
            image,
            options,
            &local_proposals,
            localized.limited,
            lines,
            &unread,
        )
    });
    crate::formats::typed_result(reads, unread, true, localized.limited, raw)
}

// Additional retail recovery must agree with source pixels independently of
// its decoding thresholds. An inconclusive check rejects only this optional read.
#[cfg(feature = "low")]
fn retail_recovery_agrees(
    image: Image<'_>,
    polygon: Quad,
    read: &Read,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> bool {
    if !matches!(read.format.as_str(), "EAN13" | "UPCA") {
        return true;
    }
    let text = if read.format == "UPCA" {
        format!("0{}", read.text)
    } else {
        read.text.clone()
    };
    let bytes = text.as_bytes();
    if bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let digits = std::array::from_fn(|i| bytes[i] - b'0');
    crate::pipeline::recovered_ean_source_agreement(image, polygon, &digits)
        && !crate::pipeline::source_contradiction(image, polygon, digits, sampler).unwrap_or(true)
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

#[cfg(feature = "low")]
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

#[cfg(feature = "low")]
fn remove_decoded_regions(unread: &mut Vec<Region>, reads: &[Read]) {
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| crate::geometry::overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
}

#[cfg(feature = "low")]
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
                restored: false,
                profile: SourceProfile::Gray(0.),
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

#[derive(Clone, Copy)]
enum SourceProfile {
    Gray(f32),
    #[cfg(not(feature = "low"))]
    Color(u8),
    #[cfg(not(feature = "low"))]
    Inverted,
    #[cfg(feature = "medium")]
    WideEan,
    #[cfg(feature = "medium")]
    BandEan(f64),
    /// Wide high-pass hypothesis for blurred or ghosted photographs.
    #[cfg(feature = "medium")]
    Highpass(f32),
}
impl SourceProfile {
    fn band(profile: Self) -> Option<f64> {
        #[cfg(feature = "medium")]
        {
            if let Self::BandEan(offset) = profile {
                Some(offset)
            } else {
                None
            }
        }
        #[cfg(not(feature = "medium"))]
        {
            let _ = profile;
            None
        }
    }
    fn contrast(self) -> f32 {
        match self {
            Self::Gray(strength) => strength,
            // Discovery uses the wider 1.5 kernel; continuity uses the existing
            // narrow 2.25 kernel and its stricter source-line confirmation.
            #[cfg(feature = "medium")]
            Self::WideEan | Self::BandEan(_) => 2.25,
            #[cfg(feature = "medium")]
            Self::Highpass(strength) => strength,
            #[cfg(not(feature = "low"))]
            _ => 0.,
        }
    }
    fn color(self) -> u8 {
        match self {
            #[cfg(not(feature = "low"))]
            Self::Color(channel) => channel,
            _ => 0,
        }
    }
    fn inverted(self) -> bool {
        match self {
            Self::Gray(_) => false,
            #[cfg(not(feature = "low"))]
            Self::Color(_) => false,
            #[cfg(not(feature = "low"))]
            Self::Inverted => true,
            #[cfg(feature = "medium")]
            Self::WideEan | Self::BandEan(_) | Self::Highpass(_) => false,
        }
    }
    fn highpass(self) -> Option<f32> {
        #[cfg(feature = "medium")]
        {
            if let Self::Highpass(strength) = self {
                Some(strength)
            } else {
                None
            }
        }
        #[cfg(not(feature = "medium"))]
        {
            let _ = self;
            None
        }
    }
    fn wide(self) -> bool {
        match self {
            Self::Gray(_) => false,
            #[cfg(not(feature = "low"))]
            Self::Color(_) | Self::Inverted => false,
            #[cfg(feature = "medium")]
            Self::WideEan | Self::BandEan(_) => true,
            #[cfg(feature = "medium")]
            Self::Highpass(_) => false,
        }
    }
}
struct Candidate<'a> {
    image: Image<'a>,
    im: ImageView<'a>,
    quad: Quad,
    mask: u32,
    dense: bool,
    profile: SourceProfile,
    restored: bool,
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
            let required = if self.profile.inverted()
                || self.profile.color() > 0
                || (self.profile.contrast() > 0.
                    && (matches!(o.read.format.as_str(), "ITF" | "Code39")
                        || (cfg!(feature = "medium")
                            && self.mask == 1
                            && o.original_rows.count_ones() < 3)))
            {
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
            if !self.restored {
                old.original_rows |= 1_u64 << row;
            }
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
            original_rows: if self.restored { 0 } else { 1_u64 << row },
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
    if candidate.mask == 0 {
        return;
    }
    let im = candidate.im;
    let Some([a, b]) = sample_candidate_profile(candidate, sampler, v, dense) else {
        return;
    };

    let enhanced = candidate.profile.contrast() > 0.;
    let methods = if enhanced { 6 } else { 3 };
    for method in 0..methods {
        candidate.restored = method >= 3;
        if method == 3 {
            if cfg!(feature = "medium") && candidate.profile.wide() {
                sampler.restore_contrast_wide(1.5);
            } else if let Some(strength) = candidate.profile.highpass() {
                sampler.restore_highpass(HIGHPASS_BOXES, strength);
            } else {
                sampler.restore_contrast(candidate.profile.contrast());
            }
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
            let first = candidate.profile.inverted()
                ^ if reverse {
                    sampler.first_black ^ (sampler.runs.len().is_multiple_of(2))
                } else {
                    sampler.first_black
                };
            let mut decoded = linear::decode(&sampler.runs, first, active_mask);
            supplement_ean(&sampler.runs, first, active_mask, &mut decoded);
            #[cfg(any(feature = "low", feature = "medium"))]
            if TIER == 0 && enhanced && decoded.is_empty() {
                supplement_ean8_visual(&sampler.runs, first, active_mask & 4, &mut decoded);
            }
            let endpoints = if reverse { [b, a] } else { [a, b] };
            decoded.retain(|read| {
                source_quiet(&sampler.runs, read, im, endpoints)
                    && (candidate.profile.contrast() <= 0.
                        || read.format != "UPCE"
                        || recovered_upce_quiet(&sampler.runs, read, im, endpoints))
                    && (candidate.profile.contrast() <= 0.
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

fn sample_candidate_profile(
    candidate: &Candidate<'_>,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
    v: f64,
    dense: bool,
) -> Option<[[f64; 2]; 2]> {
    let q = candidate.quad;
    let im = candidate.im;
    let a = point(q, -0.15, v);
    let b = point(q, 1.15, v);
    if candidate.profile.color() > 0 {
        if !sampler.sample_color_limited(
            im,
            a,
            b,
            if dense { 3. } else { 1.5 },
            1024,
            candidate.profile.color(),
        ) {
            return None;
        }
    } else if let Some(offset) = SourceProfile::band(candidate.profile) {
        sampler.sample_band_limited(im, a, b, 2., 1536, offset);
    } else if dense {
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
        sampler.sample_limited(
            im,
            a,
            b,
            if cfg!(feature = "medium") && candidate.profile.wide() && candidate.mask == 1 {
                2.0
            } else if cfg!(feature = "medium")
                && candidate.profile.contrast() > 0.
                && candidate.mask == 1
            {
                1.0
            } else {
                DENSITY
            },
            if cfg!(feature = "medium") && candidate.profile.wide() && candidate.mask == 1 {
                limit.min(1536)
            } else if cfg!(feature = "medium")
                && candidate.profile.contrast() > 0.
                && candidate.mask == 1
            {
                limit.min(768)
            } else {
                limit
            },
        );
    }
    if !sampler.has_contrast() {
        return None;
    }

    Some([a, b])
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

// Additional restored short-code evidence must have actual quiet space.
// Only a real image edge permits the decoder's cropped-margin exception.
fn recovered_upce_quiet(
    runs: &[f32],
    read: &linear::Read,
    image: ImageView<'_>,
    endpoints: [[f64; 2]; 2],
) -> bool {
    let Some(guard) = runs.get(read.start..read.start + 3) else {
        return false;
    };
    let module = guard.iter().sum::<f32>() / 3.;
    let boundary = |point: [f64; 2]| {
        point[0] <= 0.
            || point[1] <= 0.
            || point[0] >= usize_f64(image.width - 1)
            || point[1] >= usize_f64(image.height - 1)
    };
    let leading = read.start > 0 && runs[read.start - 1] >= 7.0 * module
        || read.start == 1 && boundary(endpoints[0]);
    let trailing = read.end < runs.len() && runs[read.end] >= 7.0 * module
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

#[cfg(any(feature = "low", feature = "medium"))]
fn supplement_ean8_visual(runs: &[f32], first_black: bool, mask: u32, out: &mut Vec<linear::Read>) {
    if mask & 4 == 0 {
        return;
    }
    let mut attempts = 0;
    for start in (usize::from(!first_black)..runs.len().saturating_sub(43)).step_by(2) {
        let end = start + 43;
        if start == 0 || out.iter().any(|r| r.start < end && r.end > start) {
            continue;
        }
        // Conservative consequences of the unchanged minimum module and guard gates.
        let exterior = runs[start - 1].min(runs[end]);
        if exterior < 4. * 0.79 {
            continue;
        }
        let widths = &runs[start..end];
        let guard = widths[..3].iter().copied().fold(0_f32, f32::max);
        if exterior < 4. * guard / 1.66 {
            continue;
        }
        let module = widths.iter().sum::<f32>() / 67.;
        if exterior < 4. * module {
            continue;
        }
        if attempts >= 16 {
            break;
        }
        attempts += 1;
        if let Some((digits, cost, _)) =
            barcode_research_core::multi_profile::retail_short::source_ean8_evidence(widths)
        {
            out.push(linear::Read {
                decoded: true,
                addon: None,
                format: "EAN8",
                text: digits.iter().map(|d| char::from(b'0' + d)).collect(),
                start,
                end,
                error: cost,
                gs1: false,
            });
        }
    }
}

#[cfg(all(test, feature = "low"))]
mod tests;

#[cfg(test)]
mod restored_quiet_tests;

#[cfg(all(test, any(feature = "low", feature = "medium")))]
mod source_ean8_tests;
