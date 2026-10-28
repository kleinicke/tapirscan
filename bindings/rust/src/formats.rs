//! Format readers around the primary EAN-13/UPC-A scanner.
//!
//! Pass order of [`Scanner::scan_formats_typed_with_addons`] per mode (L, M, H, V:
//! Low, Medium, High, Very High). `cfg` marks passes compiled into only some modes;
//! the others are switched and bounded by [`policy::POLICY`]. Every recovery runs
//! only on what the earlier passes left unread. The `read` and `require` supplement
//! policies run this order exactly as `ignore` does; `supplements.rs` then reads the
//! supplements beside the EAN/UPC results.
//!
//! | Pass                                                         | L | M | H | V |
//! |--------------------------------------------------------------|---|---|---|---|
//! | Low fast path for linear selections (`cfg`)                  | x |   |   |   |
//! | EAN-13 alone: the primary scanner only                       | x | x | x | x |
//! | **`scan_additional`**, the whole-image readers               |   |   |   |   |
//! | linear sweep of every selected linear format                 | x |   |   |   |
//! | linear sweep of `SWEPT`, plus a Code 128/39/ITF group        |   | x | x | x |
//! | 2D reader groups; QR and MaxiCode apart from the other 2D    | x | x | x | x |
//! | QR and MaxiCode reduced copies above Full HD (`cfg` per mode)| x | x | x | x |
//! | enlarged QR thumbnails                                       | x | x | x | x |
//! | pending matrix regions: crop retries (up to 32)              |   | x | x | x |
//! | QR grid projection of unresolved regions                     | x | x | x | x |
//! | QR foreground thresholds, then sharpened (mixed: small only) |   | x | x | x |
//! | Data Matrix area-averaged copies                             |   |   | x | x |
//! | QR reduced copies (mixed, large: dim frame or square texture)|   |   | x | x |
//! | QR equalized and inverted copies (`cfg`)                     |   |   | x | x |
//! | **primary retail scan** (EAN-13/UPC-A; M+ also EAN-8/UPC-E)  | x | x | x | x |
//! | sheets (8 linear reads): voting boxes beyond the first 24    | x | x | x | x |
//! | **`recover_linear_regions`**, inside localized boxes         |   | x | x | x |
//! | sheets: unread and further boxes, Low's whole-image sweep    |   | x | x | x |
//! | other linear formats in retail boxes and their central bands |   | x | x | x |
//! | stripe boxes for unread or label frames (M: sparse search)   |   | x | x | x |
//! | unread linear-only frames: Low's sweep (Codabar, Code 93)    |   | x | x | x |
//! | large frames: half-scale localization                        |   |   | x | x |
//! | large frames: window localization                            |   |   |   | x |
//! | large batch frames: half-scale retail boxes                  |   | x |   |   |
//! | short retail contrast hypothesis (H+: repeats withheld)      |   | x | x | x |
//! | retail polarity probe                                        |   | x | x | x |
//! | Code 128/39/ITF/93 contrast retries (8, 8, 16 proposals)     |   | x | x | x |
//! | EAN-13/UPC-A source bands (`cfg`)                            |   |   | x | x |
//! | short retail crops (`cfg`)                                   |   | x |   |   |
//! | UPC-E separated raw rows: retry 2 uncovered proposals         |   | x | x | x |
//! | deduplication and ranking                                    | x | x | x | x |
use crate::format_registry::{ALL_FORMATS_MASK, ENGINE_OPT_IN_MASK, LINEAR_MASK};
use crate::read::{Read, Region};
use crate::{Error, Image, MODE, ScanOptions, Scanner, geometry::overlap_quads};
use barcode_multiformat::binarization::Images as Thresholds;
#[cfg(any(feature = "low", feature = "medium"))]
use barcode_multiformat::formats::COMMON_LINEAR_MASK;
use barcode_multiformat::formats::{
    AZTEC, CODE39, CODE128, DATA_BAR, DATA_BAR_EXPANDED, EAN8, EAN13, ITF, MATRIX_MASK, MAXI_CODE,
    MICRO_QR_CODE, QR_CODE, RETAIL_MASK, UPCA, UPCE,
};
#[cfg(not(feature = "low"))]
use barcode_multiformat::formats::{CODABAR, CODE93, DATA_BAR_LIMITED, DATA_MATRIX};
use barcode_multiformat::{MAX_IMAGE_PIXELS, MAX_IMAGE_SIDE};
use scanner_types::EngineScan;
use serde_json::{Value, json};

/// EAN-13 and UPC-A, the formats of the primary retail scanner path.
pub(crate) const PRIMARY_RETAIL: u32 = EAN13 | UPCA;
/// EAN-8 and UPC-E, the short retail formats.
pub(crate) const SHORT_RETAIL: u32 = EAN8 | UPCE;
/// Code 128, Code 39, ITF, Codabar, Code 93 and `DataBar` Limited: the linear
/// formats other than retail, `DataBar` and `DataBar` Expanded, read in localized boxes.
#[cfg(not(feature = "low"))]
const NON_RETAIL_LINEAR: u32 = CODE128 | CODE39 | ITF | CODABAR | CODE93 | DATA_BAR_LIMITED;

#[cfg(not(feature = "low"))]
mod linear_recovery;
#[cfg(not(feature = "low"))]
mod matrix_texture;
mod policy;
mod supplements;
use policy::POLICY;

/// Frames whose long side exceeds twice the 768-pixel localization raster count
/// as large: small symbols there fall below the localizer's minimum group size.
const LARGE_FRAME_SIDE: usize = 1536;
/// Frames whose short side exceeds this are treated as possible label sheets,
/// which carry many short Code 128 fields.
#[cfg(not(feature = "low"))]
const LABEL_SHEET_SHORT_SIDE: usize = 1536;
/// Frames whose long side exceeds Full HD read QR and `MaxiCode` on reduced copies.
const REDUCED_COPY_SIDE: usize = 1920;
/// Frames of at most 512 x 512 pixels count as small: there a fixed-cost QR
/// recovery stays cheap even when no QR is present.
const SMALL_FRAME_PIXELS: usize = 262_144;
/// Frames of at least 8 million pixels (a phone's full sensor) hold close-up symbols
/// whose coarse averaged copy shows no Data Matrix candidate, so they skip that gate.
#[cfg(not(feature = "low"))]
const CLOSE_UP_PIXELS: usize = 8_000_000;
/// The full-frame QR threshold recoveries are bounded to Full-HD-sized inputs.
const QR_RECOVERY_MAX_PIXELS: usize = 2_097_152;
/// Whether the frame's long side exceeds [`LARGE_FRAME_SIDE`].
fn is_large_frame(width: usize, height: usize) -> bool {
    width.max(height) > LARGE_FRAME_SIDE
}

/// Whether the frame's short side exceeds [`LABEL_SHEET_SHORT_SIDE`].
#[cfg(not(feature = "low"))]
fn is_label_sheet_sized(width: usize, height: usize) -> bool {
    width.min(height) > LABEL_SHEET_SHORT_SIDE
}

/// Whether EAN-8 and UPC-E move from the sweep into the primary retail scan.
fn shares_short_retail(mask: u32) -> bool {
    POLICY.shared_short_retail && mask & SHORT_RETAIL != 0
}

/// Whether the frame's long side exceeds [`REDUCED_COPY_SIDE`].
fn exceeds_full_hd(width: usize, height: usize) -> bool {
    width.max(height) > REDUCED_COPY_SIDE
}

/// Whether a retail barcode needs its adjacent two- or five-digit supplement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EanAddOnPolicy {
    /// Decode the main value without searching for a supplement.
    #[default]
    Ignore,
    /// Attach a confirmed supplement when available; keep the main value otherwise.
    Read,
    /// Accept retail reads only when a supplement is confirmed.
    Require,
}

impl Scanner {
    /// Scan selected formats into the shared typed result and optional raw diagnostics.
    /// # Errors
    /// Rejects invalid masks, image layouts, malformed reader output, and oversized inputs.
    pub fn scan_formats_typed_with_addons(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        mask: u32,
        addons: EanAddOnPolicy,
    ) -> Result<EngineScan, Error> {
        crate::matrix_frontend::with_parallel(options.parallel, || {
            if addons == EanAddOnPolicy::Ignore {
                return self.scan_formats_inner(image, options, mask);
            }
            // Supplements concern retail reads only: every format reads exactly as
            // without them, then the supplement stage attaches or requires them.
            let all = ScanOptions {
                multiple: true,
                ..options
            };
            let mut scan = self.scan_formats_inner(image, all, mask)?;
            supplements::apply(image, options, addons, &mut scan, &mut self.additional_gray)?;
            Ok(scan)
        })
    }

    fn scan_formats_inner(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        mask: u32,
    ) -> Result<EngineScan, Error> {
        validate(image, mask)?;
        #[cfg(feature = "low")]
        if crate::fast_linear::enabled(options, mask) {
            return crate::fast_linear::scan(self, image, options, mask);
        }

        if mask == EAN13 {
            return primary_result(image, &self.scan_with_options(image, options)?);
        }
        let start = crate::timer::Timer::start();
        // Rank only after all selected readers have finished.
        let full_options = ScanOptions {
            multiple: true,
            ..options
        };
        let shared_retail = shares_short_retail(mask);
        let (extras, coverage) = scan_additional(
            image,
            mask & !(if shared_retail { SHORT_RETAIL } else { 0 }),
            mask & RETAIL_MASK != 0,
            &mut self.additional_gray,
        )?;
        let primary = if shared_retail || mask & PRIMARY_RETAIL != 0 {
            Some(self.scan_primary(image, full_options, &coverage, shared_retail, mask)?)
        } else {
            None
        };
        let mut reads = primary.as_ref().map_or_else(Vec::new, primary_reads);
        retain_requested_primary(&mut reads, mask);
        extend_primary_retail(&mut reads, primary.as_ref(), mask);
        let mut unread = primary
            .as_ref()
            .map_or_else(Vec::new, |primary| unread_regions(primary, &reads));
        let localization_limited = primary
            .as_ref()
            .is_some_and(|p| p.localization_work_limited);
        let mut unfinished = primary.as_ref().is_some_and(crate::Result::unfinished);
        let had_extras = !extras.is_empty();
        for extra in extras {
            reads.extend(extra.barcodes.into_iter().map(Read::additional));
            if options.include_regions {
                unread.extend(extra.regions.into_iter().map(|region| Region {
                    format: region.format,
                    text: region.text,
                    polygon: region.polygon.map(|point| point.map(f64::from)),
                    support: region.support as u64,
                    localization_score: Some(f64::from(region.localization_score)),
                }));
            }
            unfinished |= extra.unfinished;
        }
        #[cfg(not(feature = "low"))]
        let mut deferred_reads = Vec::new();
        #[cfg(not(feature = "low"))]
        // Every selection with a linear format, including DataBar and DataBar Expanded
        // together or next to 2D formats, decodes the localized boxes.
        if mask
            & (RETAIL_MASK | NON_RETAIL_LINEAR | DATA_BAR | DATA_BAR_EXPANDED | ENGINE_OPT_IN_MASK)
            != 0
        {
            deferred_reads = self.recover_linear_regions(
                image,
                options,
                mask,
                primary.as_ref(),
                &mut reads,
                &mut unread,
            )?;
            unfinished = true;
        }
        if had_extras {
            reads = distinct(reads);
            unread = retain_unresolved(unread, &reads);
        }
        // Deferred physical-instance claims must survive the generic overlap
        // cleanup: an existing read was not sufficient proof of their identity.
        #[cfg(not(feature = "low"))]
        if options.include_regions {
            unread.extend(deferred_reads);
        }
        reads = crate::linear_duplicates::merge_output(reads, image);
        rank_reads(&mut reads);
        unfinished |= localization_limited;
        let raw = format_diagnostics(
            primary.as_ref(),
            &reads,
            &unread,
            options,
            start.elapsed().as_secs_f64() * 1000.0,
        )?;
        if !options.multiple && reads.len() > 1 {
            let remaining = reads.split_off(1);
            if options.include_regions {
                unread.extend(remaining.into_iter().map(|read| Region {
                    format: read.format,
                    text: read.text,
                    polygon: read.polygon,
                    support: read.support,
                    localization_score: None,
                }));
            }
        }
        typed_result(image, reads, unread, unfinished, raw)
    }
}

impl Scanner {
    /// Primary retail scan within a format selection. Medium notes whether other linear
    /// formats share it, which bounds its retail crop recovery.
    fn scan_primary(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        coverage: &[crate::Quad],
        shared_retail: bool,
        mask: u32,
    ) -> Result<crate::Result, Error> {
        #[cfg(feature = "low")]
        let _ = mask;
        crate::pipeline::scan(
            self,
            image,
            crate::pipeline::ScanContext {
                options,
                consolidate: false,
                shared_retail,
                #[cfg(not(feature = "low"))]
                shared_linear: mask & LINEAR_MASK & !RETAIL_MASK != 0,
                #[cfg(not(feature = "low"))]
                selected: mask,
            },
            coverage,
        )
    }
}

fn primary_result(image: Image<'_>, result: &crate::Result) -> Result<EngineScan, Error> {
    let reads = primary_reads(result);
    let unread = unread_regions(result, &reads);
    let raw = result
        .options
        .retain_diagnostics
        .then(|| crate::result::value(result, MODE, 0.0))
        .transpose()?;
    typed_result(image, reads, unread, result.unfinished(), raw)
}

fn format_diagnostics(
    primary: Option<&crate::Result>,
    reads: &[Read],
    unread: &[Region],
    options: ScanOptions,
    elapsed_ms: f64,
) -> Result<Option<Value>, Error> {
    if !options.retain_diagnostics {
        return Ok(None);
    }
    let mut raw = if let Some(primary) = primary {
        crate::result::value(primary, MODE, 0.0)?
    } else {
        json!({"schemaVersion":2,"mode":MODE,"multiple":true,"elapsedMs":0.0,"localizationLimited":false,"scan":{"barcodes":[],"unfinished":false}})
    };
    if options.include_regions {
        let mut regions: Vec<_> = reads
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|_| Error::OutputShape)?;
        regions.extend(
            unread
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| Error::OutputShape)?,
        );
        raw["scan"]["regions"] = json!(regions);
    }
    raw["multiple"] = json!(options.multiple);
    raw["elapsedMs"] = json!(elapsed_ms);
    Ok(Some(raw))
}

fn primary_reads(result: &crate::Result) -> Vec<Read> {
    result
        .barcodes()
        .iter()
        .map(|barcode| {
            let d = &barcode.detection;
            Read::primary(
                d.digits,
                d.polygon,
                d.support,
                d.axis,
                barcode.candidate_indices.clone(),
            )
        })
        .collect()
}

/// Narrow/wide bars of a Pharmacode value, left to right (`true` for wide).
fn pharmacode_bars(text: &str) -> Option<Vec<bool>> {
    let mut value: u32 = text.parse().ok()?;
    let mut bars = Vec::new();
    while value > 0 {
        let wide = value.is_multiple_of(2);
        bars.push(wide);
        value = (value - if wide { 2 } else { 1 }) / 2;
    }
    bars.reverse();
    Some(bars)
}

/// Whether `short`, read along a line (within about 18 degrees) parallel to `long`'s reading axis (its first edge),
/// lies within `long`'s extent along that axis and within `across` of its length across
/// it. A read outline covers only the rows that decoded; a fragment from other rows of
/// the same bars can lie beside it.
fn along_same_bars(long: &crate::Quad, short: &crate::Quad, across_limit: f64) -> bool {
    let axis = [long[1][0] - long[0][0], long[1][1] - long[0][1]];
    let length = axis[0].hypot(axis[1]);
    let short_axis = [short[1][0] - short[0][0], short[1][1] - short[0][1]];
    let short_length = short_axis[0].hypot(short_axis[1]);
    if length < 1. || short_length < 1. {
        return false;
    }
    let unit = [axis[0] / length, axis[1] / length];
    if (unit[0] * short_axis[0] + unit[1] * short_axis[1]).abs() < 0.95 * short_length {
        return false;
    }
    let center = |q: &crate::Quad| {
        q.iter()
            .fold([0., 0.], |c, p| [c[0] + p[0] / 4., c[1] + p[1] / 4.])
    };
    let (origin, middle) = (long[0], center(long));
    let across = |p: [f64; 2]| ((p[0] - middle[0]) * unit[1] - (p[1] - middle[1]) * unit[0]).abs();
    short.iter().all(|p| {
        let along = (p[0] - origin[0]) * unit[0] + (p[1] - origin[1]) * unit[1];
        (-0.05 * length..=1.05 * length).contains(&along)
    }) && across(center(short)) <= across_limit * length
}

/// MSI Plessey, the 2 of 5 variants and Pharmacode have no per-character check: a
/// threshold pass that erases part of a symbol can leave a fragment that still reads
/// (an MSI prefix whose last digit passes the mod-10 check, Industrial, IATA or Matrix
/// 2 of 5 digits before an erased run that looks like its stop and quiet zone, any run of
/// Pharmacode bars). Drop such a read
/// when a longer read of the same symbol that contains it covers it.
fn drop_unchecked_fragments(reads: &mut Vec<Read>) {
    let contains = |long: &Read, short: &Read| match short.format.as_str() {
        "MSIPlessey" => long.text.len() > short.text.len() && long.text.starts_with(&short.text),
        "Industrial25" | "IATA25" | "Matrix25" => {
            long.text.len() > short.text.len() && long.text.contains(&short.text)
        }
        "Pharmacode" => match (pharmacode_bars(&long.text), pharmacode_bars(&short.text)) {
            (Some(long), Some(short)) => {
                long.len() > short.len() && long.windows(short.len()).any(|w| w == short)
            }
            _ => false,
        },
        _ => false,
    };
    let fragments: Vec<bool> = reads
        .iter()
        .map(|read| {
            reads.iter().any(|other| {
                other.format == read.format
                    && contains(other, read)
                    && (overlap_quads(&read.polygon, &other.polygon).0 >= 0.65
                        || (read.format != "MSIPlessey"
                            && along_same_bars(&other.polygon, &read.polygon, 0.5)))
            })
        })
        .collect();
    let mut fragment = fragments.into_iter();
    reads.retain(|_| !fragment.next().unwrap_or(false));
    // Reads of the same opt-in bar code from rows of the same bars that the outline
    // merge left apart (small symbols give it little evidence): keep the first,
    // best-ranked one.
    let mut kept: Vec<Read> = Vec::with_capacity(reads.len());
    for read in reads.drain(..) {
        let duplicate = matches!(
            read.format.as_str(),
            "Industrial25" | "IATA25" | "Matrix25" | "Pharmacode"
        ) && kept.iter().any(|other| {
            other.format == read.format
                && other.text == read.text
                && (overlap_quads(&read.polygon, &other.polygon).0 >= 0.65
                    || along_same_bars(&other.polygon, &read.polygon, 1.)
                    || along_same_bars(&read.polygon, &other.polygon, 1.))
        });
        if !duplicate {
            kept.push(read);
        }
    }
    *reads = kept;
}

/// Whether a read is of a linear format; 2D reads do not stop linear recovery.
fn is_linear_read(read: &Read) -> bool {
    !matches!(
        read.format.as_str(),
        "QRCode" | "MicroQRCode" | "DataMatrix" | "PDF417" | "Aztec" | "MaxiCode"
    )
}

/// Drop linear reads of periodic or two-dimensional textures (see [`crate::texture`]). A
/// merged outline can cross the bars obliquely where none of the detections merged into it
/// does: a read is kept when one of them passes, and then reports that outline.
fn drop_texture_reads(reads: &mut Vec<Read>, image: Image<'_>) {
    reads.retain_mut(|read| {
        if !is_linear_read(read) || !crate::texture::texture_read(image, &read.polygon) {
            return true;
        }
        let source = read
            .sources
            .iter()
            .find(|q| !crate::texture::texture_read(image, q));
        source.copied().is_some_and(|q| {
            read.polygon = q;
            true
        })
    });
}

pub(crate) fn typed_result(
    image: Image<'_>,
    mut reads: Vec<Read>,
    unread: Vec<Region>,
    unfinished: bool,
    mut diagnostics: Option<Value>,
) -> Result<EngineScan, Error> {
    drop_texture_reads(&mut reads, image);
    drop_unchecked_fragments(&mut reads);
    if let Some(raw) = &mut diagnostics {
        raw["scan"]["barcodes"] = serde_json::to_value(&reads).map_err(|_| Error::OutputShape)?;
        raw["scan"]["unfinished"] = json!(unfinished);
    }
    Ok(EngineScan {
        barcodes: reads
            .into_iter()
            .map(Read::into_public)
            .collect::<Result<_, _>>()?,
        undecoded: unread
            .into_iter()
            .map(Region::into_public)
            .collect::<Result<_, _>>()?,
        diagnostics,
    })
}

fn distinct(mut reads: Vec<Read>) -> Vec<Read> {
    reads.sort_by_key(|read| std::cmp::Reverse(read.support));
    let mut result: Vec<Read> = Vec::new();
    for read in reads {
        if !result.iter().any(|other| {
            other.text == read.text
                && other.format == read.format
                && other.addon == read.addon
                && other.structured_append == read.structured_append
                && other.reader_initialization.unwrap_or(false)
                    == read.reader_initialization.unwrap_or(false)
                && overlap_quads(&other.polygon, &read.polygon).0 >= 0.65
        }) {
            result.push(read);
        }
    }
    result
}

fn rank_reads(reads: &mut [Read]) {
    reads.sort_by_key(|read| std::cmp::Reverse(read.support));
    for (i, read) in reads.iter_mut().enumerate() {
        read.rank = Some(i + 1);
    }
}

fn retain_unresolved(mut unread: Vec<Region>, reads: &[Read]) -> Vec<Region> {
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
    distinct_regions(unread)
}

fn distinct_regions(mut regions: Vec<Region>) -> Vec<Region> {
    regions.sort_by_key(|region| std::cmp::Reverse(region.support));
    let mut result: Vec<Region> = Vec::new();
    for region in regions {
        if !result.iter().any(|other| {
            other.text == region.text
                && other.format == region.format
                && overlap_quads(&other.polygon, &region.polygon).0 >= 0.65
        }) {
            result.push(region);
        }
    }
    result
}

fn unread_regions(result: &crate::Result, reads: &[Read]) -> Vec<Region> {
    if !result.options.include_regions {
        return Vec::new();
    }
    let decoded: std::collections::HashSet<_> = reads
        .iter()
        .flat_map(|read| read.candidate_indices.iter().flatten().copied())
        .collect();
    let mut unread: Vec<_> = result
        .proposals
        .iter()
        .enumerate()
        .filter(|(i, _)| !decoded.contains(i))
        .map(|(_, proposal)| Region::unknown(proposal.polygon))
        .collect();
    if let Some(recovery) = &result.recovery {
        unread.extend(recovery.unread.iter().cloned());
    }
    unread
}

// Source rows are validated before conversion. Specializing the channel count
// lets the compiler vectorize the exact integer luminance calculation.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Weighted bytes plus 128 are at most 65408; shifting eight bits is at most 255."
)]
fn gray_rows<const CHANNELS: usize>(image: Image<'_>, gray: &mut [u8]) {
    for (source, target) in image
        .data
        .chunks(image.stride)
        .zip(gray.chunks_mut(image.width))
    {
        for (pixel, value) in source[..image.width * CHANNELS]
            .as_chunks::<CHANNELS>()
            .0
            .iter()
            .zip(target)
        {
            *value = ((u32::from(pixel[0]) * 77
                + u32::from(pixel[1]) * 150
                + u32::from(pixel[2]) * 29
                + 128)
                >> 8) as u8;
        }
    }
}

fn gray_image(image: Image<'_>, gray: &mut Vec<u8>) -> Result<(), Error> {
    gray.resize(image.width * image.height, 0);
    match image.channels {
        1 => {
            for (source, target) in image
                .data
                .chunks(image.stride)
                .zip(gray.chunks_mut(image.width))
            {
                target.copy_from_slice(&source[..image.width]);
            }
        }
        3 => gray_rows::<3>(image, gray),
        4 => gray_rows::<4>(image, gray),
        _ => return Err(Error::Parameters),
    }
    Ok(())
}

/// Scan a bilinearly enlarged copy for QR, with results in source coordinates.
fn upscaled_qr_retry(
    pixels: &[u8],
    width: usize,
    height: usize,
    factor: usize,
    effort: usize,
) -> barcode_multiformat::Scan {
    let (out_width, out_height) = (width * factor, height * factor);
    let f = barcode_multiformat::numeric::usize_f32(factor);
    let source = |out: usize, limit: usize| {
        let position = ((barcode_multiformat::numeric::usize_f32(out) + 0.5) / f - 0.5)
            .clamp(0., barcode_multiformat::numeric::usize_f32(limit - 1));
        let low = barcode_multiformat::numeric::f32_usize(position);
        let high = (low + 1).min(limit - 1);
        (
            low,
            high,
            position - barcode_multiformat::numeric::usize_f32(low),
        )
    };
    let columns: Vec<_> = (0..out_width).map(|x| source(x, width)).collect();
    let mut larger = vec![0_u8; out_width * out_height];
    for (y, row) in larger.chunks_exact_mut(out_width).enumerate() {
        let (y0, y1, wy) = source(y, height);
        for (pixel, &(x0, x1, wx)) in row.iter_mut().zip(&columns) {
            let at = |yy: usize, xx: usize| f32::from(pixels[yy * width + xx]);
            let top = at(y0, x0) * (1. - wx) + at(y0, x1) * wx;
            let bottom = at(y1, x0) * (1. - wx) + at(y1, x1) * wx;
            *pixel = barcode_multiformat::numeric::f32_u8((top * (1. - wy) + bottom * wy).round());
        }
    }
    let mut scan = scan_reader(&larger, out_width, out_height, QR_CODE, effort);
    // Candidates of a failed enlargement would suppress the later recoveries,
    // which run only on frames without a QR candidate.
    if scan.barcodes.is_empty() {
        scan.regions.clear();
    }
    for polygon in scan
        .barcodes
        .iter_mut()
        .map(|value| &mut value.polygon)
        .chain(scan.regions.iter_mut().map(|value| &mut value.polygon))
    {
        for point in polygon {
            point[0] /= f;
            point[1] /= f;
        }
    }
    scan.unfinished = true;
    scan
}

/// Scan a copy reduced to `target_long` on its long side that samples one source
/// pixel per bin. With `averaged_first` (High and Very High only) an area-averaged
/// copy is scanned first and the sampled copy only when it reads nothing.
fn scaled_matrix_retry(
    pixels: &[u8],
    width: usize,
    height: usize,
    target_long: usize,
    mask: u32,
    effort: usize,
    averaged_first: bool,
) -> barcode_multiformat::Scan {
    // u64 keeps the products exact where usize is 32 bits (WASM).
    let scale = |value: usize, numerator: usize, denominator: usize| {
        usize::try_from(value as u64 * numerator as u64 / denominator as u64).unwrap_or(usize::MAX)
    };
    // On shaded or blurred photos one source pixel per bin keeps the noise that breaks
    // the reduced copy's finder runs; sharp photos can read only on the sampled copy.
    // Medium's large-frame retries read as many photos with either copy.
    #[cfg(not(any(feature = "high", feature = "very-high")))]
    debug_assert!(!averaged_first, "averaged copies are a High recovery");
    #[cfg(any(feature = "high", feature = "very-high"))]
    if averaged_first {
        let (out_width, out_height, smaller) = averaged_pixels(pixels, width, height, target_long);
        let scan = scaled_scan(
            &smaller,
            [out_width, out_height],
            [width, height],
            mask,
            effort,
        );
        if !scan.barcodes.is_empty() {
            return scan;
        }
    }
    let long = width.max(height);
    let out_width = scale(width, target_long, long).max(3);
    let out_height = scale(height, target_long, long).max(3);
    let x_source: Vec<_> = (0..out_width)
        .map(|x| scale(2 * x + 1, width, 2 * out_width).min(width - 1))
        .collect();
    let mut smaller = vec![0; out_width * out_height];
    for y in 0..out_height {
        let source_y = scale(2 * y + 1, height, 2 * out_height).min(height - 1);
        let source = &pixels[source_y * width..(source_y + 1) * width];
        let target = &mut smaller[y * out_width..(y + 1) * out_width];
        for (pixel, &x) in target.iter_mut().zip(&x_source) {
            *pixel = source[x];
        }
    }
    scaled_scan(
        &smaller,
        [out_width, out_height],
        [width, height],
        mask,
        effort,
    )
}

/// Scan a reduced copy and map its polygons back to source coordinates.
fn scaled_scan(
    smaller: &[u8],
    [out_width, out_height]: [usize; 2],
    [width, height]: [usize; 2],
    mask: u32,
    effort: usize,
) -> barcode_multiformat::Scan {
    let mut scan = scan_reader(smaller, out_width, out_height, mask, effort);
    #[cfg(any(feature = "high", feature = "very-high"))]
    if mask == QR_CODE && effort == 2 {
        let known: Vec<_> = scan.barcodes.iter().collect();
        let recovered =
            barcode_research_core::qr_grid::recover(smaller, out_width, out_height, &scan, &known);
        scan.barcodes.extend(recovered.barcodes);
        if scan.barcodes.is_empty() {
            let foreground =
                barcode_research_core::qr_frontend::scan_foreground(smaller, out_width, out_height);
            scan.barcodes.extend(foreground.barcodes);
            scan.regions.extend(foreground.regions);
        }
    }
    let scale_x = barcode_multiformat::numeric::usize_f32(width)
        / barcode_multiformat::numeric::usize_f32(out_width);
    let scale_y = barcode_multiformat::numeric::usize_f32(height)
        / barcode_multiformat::numeric::usize_f32(out_height);
    for polygon in scan
        .barcodes
        .iter_mut()
        .map(|value| &mut value.polygon)
        .chain(scan.regions.iter_mut().map(|value| &mut value.polygon))
    {
        for point in polygon {
            point[0] *= scale_x;
            point[1] *= scale_y;
        }
    }
    scan.unfinished = true;
    scan
}

#[cfg(not(feature = "low"))]
/// Whether a scan read a symbol of this format.
fn has_format_read(scan: &barcode_multiformat::Scan, format: &str) -> bool {
    scan.barcodes.iter().any(|read| read.format == format)
}

#[cfg(not(feature = "low"))]
fn averaged_pixels(
    pixels: &[u8],
    width: usize,
    height: usize,
    target_long: usize,
) -> (usize, usize, Vec<u8>) {
    let long = width.max(height);
    let out_width = (width * target_long / long).max(3);
    let out_height = (height * target_long / long).max(3);
    let bounds = |index: usize, out: usize, source: usize| {
        let start = index * source / out;
        (
            start,
            ((index + 1) * source / out).max(start + 1).min(source),
        )
    };
    // Keep the original bin centers and sample order. Horizontal positions are
    // identical on every output row, and vertical positions on every column.
    let columns: Vec<_> = (0..out_width)
        .map(|x| {
            let (start, end) = bounds(x, out_width, width);
            let count = (end - start).min(4);
            let positions: [usize; 4] = std::array::from_fn(|i| {
                if i < count {
                    start + (2 * i + 1) * (end - start) / (2 * count)
                } else {
                    0
                }
            });
            (count, positions)
        })
        .collect();
    let mut smaller = vec![0_u8; out_width * out_height];
    for y in 0..out_height {
        let (y0, y1) = bounds(y, out_height, height);
        let y_samples = (y1 - y0).min(4);
        let rows: [usize; 4] = std::array::from_fn(|j| {
            if j < y_samples {
                (y0 + (2 * j + 1) * (y1 - y0) / (2 * y_samples)) * width
            } else {
                0
            }
        });
        for (x, &(x_samples, positions)) in columns.iter().enumerate() {
            let mut sum = 0_u32;
            for &row in &rows[..y_samples] {
                for &column in &positions[..x_samples] {
                    sum += u32::from(pixels[row + column]);
                }
            }
            let count = u32::try_from(y_samples * x_samples).unwrap_or(1);
            smaller[y * out_width + x] = u8::try_from((sum + count / 2) / count).unwrap_or(255);
        }
    }
    (out_width, out_height, smaller)
}

/// Scan an area-averaged copy whose long side is `target_long`, with polygons
/// mapped back to source coordinates.
#[cfg(not(feature = "low"))]
fn averaged_matrix_retry(
    pixels: &[u8],
    width: usize,
    height: usize,
    target_long: usize,
    mask: u32,
    effort: usize,
) -> barcode_multiformat::Scan {
    let (out_width, out_height, smaller) = averaged_pixels(pixels, width, height, target_long);
    let mut scan = scan_reader(&smaller, out_width, out_height, mask, effort);
    let scale_x = barcode_multiformat::numeric::usize_f32(width)
        / barcode_multiformat::numeric::usize_f32(out_width);
    let scale_y = barcode_multiformat::numeric::usize_f32(height)
        / barcode_multiformat::numeric::usize_f32(out_height);
    for polygon in scan
        .barcodes
        .iter_mut()
        .map(|value| &mut value.polygon)
        .chain(scan.regions.iter_mut().map(|value| &mut value.polygon))
    {
        for point in polygon {
            point[0] *= scale_x;
            point[1] *= scale_y;
        }
    }
    scan
}

fn validate(image: Image<'_>, mask: u32) -> Result<(), Error> {
    if mask == 0
        || mask & !ALL_FORMATS_MASK != 0
        || image.width.max(image.height) > MAX_IMAGE_SIDE
        || image
            .width
            .checked_mul(image.height)
            .is_none_or(|n| n > MAX_IMAGE_PIXELS)
    {
        return Err(Error::Parameters);
    }
    crate::ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )?;
    if image.width < 3 || image.height < 3 {
        return Err(Error::Parameters);
    }

    Ok(())
}

pub(crate) fn contains_point(point: [f64; 2], quad: &crate::Quad) -> bool {
    if !point.iter().all(|v| v.is_finite()) || !quad.iter().flatten().all(|v| v.is_finite()) {
        return false;
    }
    let (mut positive, mut negative, mut area) = (false, false, 0.0_f64);
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        positive |= cross > 1e-6;
        negative |= cross < -1e-6;
        area += a[0] * b[1] - b[0] * a[1];
    }
    area.abs() > 1e-6 && !(positive && negative)
}

#[cfg(not(feature = "low"))]
pub(crate) fn uncovered_mask(
    proposals: &[crate::Proposal],
    coverage: &[crate::Quad],
    initial: u64,
) -> u64 {
    proposals
        .iter()
        .enumerate()
        .fold(initial, |mask, (i, proposal)| {
            if coverage.iter().any(|quad| {
                proposal
                    .polygon
                    .iter()
                    .all(|point| contains_point(*point, quad))
            }) {
                mask & !(1_u64 << i)
            } else {
                mask
            }
        })
}

/// Broad selections also retain the existing full-image readers, including their
/// complementary linear detections. Pure `Common1D` never pays for this pass.
#[cfg(feature = "low")]
pub(crate) fn fast_additional(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    mask: u32,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    let (scans, _) = scan_additional(
        image,
        mask,
        mask & RETAIL_MASK != 0,
        &mut scanner.additional_gray,
    )?;
    let mut reads = Vec::new();
    let mut unread = Vec::new();
    for scan in scans {
        reads.extend(scan.barcodes.into_iter().map(Read::additional));
        if options.include_regions {
            unread.extend(scan.regions.into_iter().map(|region| Region {
                format: region.format,
                text: region.text,
                polygon: region.polygon.map(|point| point.map(f64::from)),
                support: region.support as u64,
                localization_score: Some(f64::from(region.localization_score)),
            }));
        }
    }
    let reads = distinct(reads);
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
    Ok((reads, distinct_regions(unread)))
}

/// Linear formats that Medium and above read only with the whole-image sweep:
/// GS1 `DataBar` (stacked rows), `DataBar` Expanded and the opt-in linear formats with their
/// own reader.
const SWEPT: u32 = DATA_BAR | DATA_BAR_EXPANDED | ENGINE_OPT_IN_MASK;

/// Run the linear reader before primary discovery so strong reads can guide deep retries.
#[expect(
    clippy::too_many_lines,
    reason = "Keep shared grayscale, initial readers and bounded recovery ordered together."
)]
fn scan_additional(
    image: Image<'_>,
    mask: u32,
    retail: bool,
    gray: &mut Vec<u8>,
) -> Result<(Vec<barcode_multiformat::Scan>, Vec<crate::Quad>), Error> {
    let enabled = mask & !PRIMARY_RETAIL;
    if enabled == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    // Localized linear recovery reads the other linear formats inside segment-voting boxes. The
    // whole-image sweep stays for DataBar (stacked rows) and, outside Medium's shared retail
    // scan, EAN-8 and UPC-E; for the rest it only added wrong reads and time. Low keeps
    // the full sweep.
    let swept = if POLICY.sweep_all_linear {
        LINEAR_MASK
    } else {
        SWEPT
    };
    let linear = enabled & LINEAR_MASK & swept;
    let matrix = enabled & !LINEAR_MASK;
    // Medium sweeps Code 128 and Code 39 at Low's effort, High and Very High at the next: retail
    // boxes come from a 768-pixel localization raster, on which the many small fields of large
    // label photos vanish. With retail selected, Medium sweeps only frames above twice that
    // raster. Without retail, the group is ITF and Code 128: localized recovery reads Code 39
    // there, and sweeping it added no read on the 340 Code 39 variations. With retail
    // selected, localized recovery reads ITF at a fraction of the cost.
    let preserved = if POLICY.preserved_sweep {
        enabled
            & if !retail {
                ITF | CODE128
            } else if is_large_frame(image.width, image.height) || POLICY.preserved_sweep_any_size {
                CODE128 | CODE39
            } else {
                0
            }
    } else {
        0
    };
    // The denser sweep's scanlines lie between Medium's, which can cross a symbol they
    // miss; from High up the group also runs at Medium's effort where Medium runs it.
    let coarse_preserved = if POLICY.coarse_preserved_sweep
        && (!retail || is_large_frame(image.width, image.height))
    {
        preserved
    } else {
        0
    };
    // Localized recovery runs separately on the original image. When no
    // whole-image reader is selected, its grayscale copy would be unused.
    if linear | preserved | matrix == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let effort = POLICY.sweep_effort;
    let qr_effort = POLICY.qr_effort;
    let pixels = if image.channels == 1 && image.stride == image.width {
        &image.data[..image.width * image.height]
    } else {
        gray_image(image, gray)?;
        gray.as_slice()
    };
    #[cfg(any(feature = "low", feature = "medium"))]
    let sparse = (crate::LOW_FAST_PATH && (matrix != 0 || linear & !COMMON_LINEAR_MASK != 0))
        .then(|| crate::fast_sparse::Prepared::new(pixels, image.width, image.height));
    let mut scans = Vec::new();
    let mut coverage = Vec::new();
    // QR and MaxiCode selected with other 2D formats are reader groups of their
    // own, so they get the reader and recoveries of a single-format selection. Micro
    // QR runs at the mode's QR effort: above effort 1 (High, Very High) in a group of
    // its own; below, the other 2D readers' effort is the same and sharing their
    // group shares their threshold images.
    // Whether a selection is QR-only or Data Matrix-only decides the 2D recoveries. Where
    // Medium moves EAN-8 and UPC-E to the primary retail scan, they do not count, with
    // or without add-ons.
    #[cfg(not(feature = "low"))]
    let two_d_mask = if POLICY.shared_short_retail {
        mask & !SHORT_RETAIL
    } else {
        mask
    };
    let qr_apart = matrix & QR_CODE != 0 && matrix != QR_CODE;
    let micro_apart = qr_effort > 1 && matrix & MICRO_QR_CODE != 0 && matrix != MICRO_QR_CODE;
    let maxi_apart = matrix & MAXI_CODE != 0 && matrix != MAXI_CODE;
    let matrix_rest = matrix
        & !(if qr_apart { QR_CODE } else { 0 }
            | if micro_apart { MICRO_QR_CODE } else { 0 }
            | if maxi_apart { MAXI_CODE } else { 0 });
    let groups: Vec<(u32, usize)> = [
        (linear & !preserved, effort),
        (preserved, POLICY.preserved_sweep_effort),
        (coarse_preserved, 0),
        (
            matrix_rest,
            if matrix_rest & !(QR_CODE | MICRO_QR_CODE) == 0 {
                qr_effort
            } else {
                1
            },
        ),
        (if maxi_apart { MAXI_CODE } else { 0 }, 1),
        (if micro_apart { MICRO_QR_CODE } else { 0 }, qr_effort),
        // The QR group, usually the heaviest, comes last: `concurrent` runs the
        // last group on the calling thread.
        (if qr_apart { QR_CODE } else { 0 }, qr_effort),
    ]
    .into_iter()
    .filter(|&(selected, _)| selected != 0)
    .collect();
    // The 2D reader groups threshold the same pixels. Groups that run one after another
    // pass the threshold cache on; a group that finds it taken (on another thread)
    // starts its own. Thresholds are deterministic, so every group reads the same.
    let thresholds =
        std::sync::Mutex::new(Some(Thresholds::new(pixels, image.width, image.height)));
    let first_scan = |selected: u32, level: usize| {
        #[cfg(any(feature = "low", feature = "medium"))]
        if let Some(sparse) = sparse.as_ref().filter(|_| {
            selected & LINEAR_MASK == 0
                || (crate::LOW_FAST_PATH && selected & !COMMON_LINEAR_MASK != 0)
        }) {
            return sparse.scan(selected, level);
        }
        if selected & LINEAR_MASK != 0 {
            return scan_reader(pixels, image.width, image.height, selected, level);
        }
        let taken = thresholds.lock().ok().and_then(|mut shared| shared.take());
        let mut binary_images =
            taken.unwrap_or_else(|| Thresholds::new(pixels, image.width, image.height));
        let scan = scan_reader_with(
            pixels,
            image.width,
            image.height,
            selected,
            level,
            &mut binary_images,
        );
        if let Ok(mut shared) = thresholds.lock() {
            *shared = Some(binary_images);
        }
        scan
    };
    let first_scans = crate::matrix_frontend::concurrent(&groups, &first_scan);
    for (&(selected, level), scan) in groups.iter().zip(first_scans) {
        if selected & LINEAR_MASK != 0 && POLICY.sweep_coverage && mask & PRIMARY_RETAIL != 0 {
            coverage.extend(
                scan.barcodes
                    .iter()
                    .filter(|b| {
                        let checked = matches!(b.format.as_str(), "EAN8" | "UPCE" | "Code128")
                            && b.support >= 3
                            && b.error <= 0.08;
                        let unchecked = matches!(b.format.as_str(), "Code39" | "ITF")
                            && b.support >= 8
                            && b.error <= 0.035
                            && b.text.len() >= 8;
                        checked || unchecked
                    })
                    .map(|b| b.polygon.map(|p| p.map(f64::from))),
            );
        }
        #[cfg(feature = "low")]
        let retries = if crate::LOW_FAST_PATH
            && selected == QR_CODE
            && scan.barcodes.is_empty()
            && exceeds_full_hd(image.width, image.height)
        {
            vec![scaled_matrix_retry(
                pixels,
                image.width,
                image.height,
                960,
                selected,
                level,
                false,
            )]
        } else {
            Vec::new()
        };
        #[cfg(feature = "medium")]
        let retries = medium_retries(
            pixels,
            [image.width, image.height],
            matrix,
            selected,
            level,
            &scan,
        );
        #[cfg(any(feature = "high", feature = "very-high"))]
        let retries = high_retries(
            pixels,
            [image.width, image.height],
            matrix,
            selected,
            level,
            &scan,
        );
        // Tiny frames (thumbnails, cropped symbols) whose modules span about two
        // pixels read better enlarged. Camera frames, 640x480 and up, never take
        // this pass: there it would add a whole scan to every frame without QR.
        let mut retries = retries;
        if selected == QR_CODE && scan.barcodes.is_empty() && image.width.max(image.height) <= 300 {
            retries.push(upscaled_qr_retry(
                pixels,
                image.width,
                image.height,
                3,
                level,
            ));
        }
        #[cfg(not(feature = "low"))]
        let mut scan = scan;
        #[cfg(not(feature = "low"))]
        let grid_retries = if selected & LINEAR_MASK == 0 {
            matrix_retries(
                pixels,
                [image.width, image.height],
                selected,
                &mut scan,
                &retries,
            )
        } else {
            Vec::new()
        };
        let projected =
            if selected == QR_CODE && !scan.regions.is_empty() && POLICY.qr_grid_projection {
                let known: Vec<_> = scan
                    .barcodes
                    .iter()
                    .chain(retries.iter().flat_map(|r| &r.barcodes))
                    .collect();
                #[cfg(not(feature = "low"))]
                let known: Vec<_> = known
                    .into_iter()
                    .chain(grid_retries.iter().flat_map(|r| &r.barcodes))
                    .collect();
                Some(barcode_research_core::qr_grid::recover(
                    pixels,
                    image.width,
                    image.height,
                    &scan,
                    &known,
                ))
            } else {
                None
            };
        scans.push(scan);
        scans.extend(retries);
        scans.extend(projected);
        #[cfg(not(feature = "low"))]
        scans.extend(grid_retries);
    }
    // Whether the frame justifies the costly QR recoveries below; judged on the
    // first passes, before any recovery adds candidates of its own.
    #[cfg(not(feature = "low"))]
    let qr_evidence = qr_recovery_evidence(two_d_mask, image.width * image.height, &scans);
    // Foreground thresholds for unresolved QR frames. Restoration has a smaller pixel
    // budget in Medium because it introduces another image and threshold search.
    #[cfg(not(feature = "low"))]
    if qr_evidence
        && qr_foreground_wanted(two_d_mask, matrix, image.width * image.height, &scans)
        && pixels.iter().any(|&value| value != 0 && value != 255)
    {
        let foreground =
            barcode_research_core::qr_frontend::scan_foreground(pixels, image.width, image.height);
        let restore = foreground.barcodes.is_empty()
            && image.width * image.height <= POLICY.qr_restoration_pixels;
        scans.push(foreground);
        if restore {
            let sharpened =
                barcode_multiformat::qr_enhance::sharpen(pixels, image.width, image.height);
            if sharpened != pixels {
                let sharpened_scan = scan_reader(
                    &sharpened,
                    image.width,
                    image.height,
                    QR_CODE,
                    POLICY.qr_restoration_effort,
                );
                let retry_foreground = sharpened_scan.barcodes.is_empty();
                scans.push(sharpened_scan);
                if retry_foreground {
                    scans.push(barcode_research_core::qr_frontend::scan_foreground(
                        &sharpened,
                        image.width,
                        image.height,
                    ));
                }
            }
        }
    }
    // Large, blurred Data Matrix symbols span tens of pixels per module, far
    // beyond what the module-scale localizer expects. Unread frames are retried
    // on area-averaged copies, which also take the corner-refining search.
    // High tries the two cheap scales, the finer one on mixed selections' large frames
    // below `CLOSE_UP_PIXELS` only after the coarse copy left a candidate. Medium has no
    // copies.
    #[cfg(not(feature = "low"))]
    if !POLICY.data_matrix_averaged_scales.is_empty()
        && matrix & DATA_MATRIX != 0
        && !scans.iter().any(|scan| has_format_read(scan, "DataMatrix"))
    {
        let long = image.width.max(image.height);
        let needs_candidate = POLICY.data_matrix_averaged_needs_candidate
            && two_d_mask != DATA_MATRIX
            && (SMALL_FRAME_PIXELS + 1..CLOSE_UP_PIXELS).contains(&(image.width * image.height))
            && !scans
                .iter()
                .any(|scan| has_format_region(scan, "DataMatrix"));
        for &target in POLICY.data_matrix_averaged_scales {
            if long < target * 3 / 2 {
                continue;
            }
            let retry =
                averaged_matrix_retry(pixels, image.width, image.height, target, DATA_MATRIX, 2);
            let done = !retry.barcodes.is_empty()
                || (needs_candidate && !has_format_region(&retry, "DataMatrix"));
            scans.push(retry);
            if done {
                break;
            }
        }
    }
    // From High up, unread QR selections retry reduced copies, then equalized and
    // inverted copies (whose code only High and Very High compile).
    #[cfg(not(feature = "low"))]
    if !POLICY.qr_reduced_scales.is_empty()
        && matrix & QR_CODE != 0
        && !scans.iter().any(has_qr_read)
        && qr_reduced_wanted(pixels, image.width, image.height, two_d_mask)
    {
        // Each finer scale runs only when the coarser one read nothing. Each scale
        // tries its area-averaged copy before the sampled one.
        for &target in POLICY.qr_reduced_scales {
            if image.width.max(image.height) <= target {
                continue;
            }
            let retry =
                scaled_matrix_retry(pixels, image.width, image.height, target, QR_CODE, 2, true);
            let decoded = !retry.barcodes.is_empty();
            scans.push(retry);
            if decoded {
                break;
            }
        }
    }
    #[cfg(any(feature = "high", feature = "very-high"))]
    if matrix & QR_CODE != 0
        && image.width * image.height <= QR_RECOVERY_MAX_PIXELS
        && qr_evidence
        && !scans.iter().any(has_qr_read)
    {
        for method in 0..2 {
            let normalized = if method == 0 {
                crate::signal_recovery::equalize(pixels)
            } else {
                pixels.iter().map(|&v| 255 - v).collect::<Vec<_>>()
            };
            if normalized == pixels {
                continue;
            }
            let retry = scan_reader(&normalized, image.width, image.height, QR_CODE, 2);
            let decoded = !retry.barcodes.is_empty();
            scans.push(retry);
            if decoded {
                break;
            }
        }
    }
    Ok((scans, coverage))
}

/// Whether an unread frame takes the reduced QR copies. QR-only selections, small
/// frames and frames no larger than the coarsest copy always do; see
/// `ModePolicy::qr_reduced_needs_texture` for the others.
#[cfg(not(feature = "low"))]
fn qr_reduced_wanted(pixels: &[u8], width: usize, height: usize, mask: u32) -> bool {
    !POLICY.qr_reduced_needs_texture
        || mask == QR_CODE
        || width * height <= SMALL_FRAME_PIXELS
        || POLICY
            .qr_reduced_scales
            .iter()
            .all(|&target| width.max(height) <= target)
        || matrix_texture::has_dense_square_texture(pixels, width, height)
}

/// Whether a frame admits the foreground-threshold and equalized/inverted QR
/// recoveries. QR-only selections and small frames always do. From High up, a mixed
/// selection's larger frames need an unread QR candidate region; without one, these
/// passes would add much time to retail photos that hold no QR code.
#[cfg(not(feature = "low"))]
fn qr_recovery_evidence(
    mask: u32,
    pixel_count: usize,
    scans: &[barcode_multiformat::Scan],
) -> bool {
    !POLICY.qr_mixed_recovery_needs_region
        || mask == QR_CODE
        || pixel_count <= SMALL_FRAME_PIXELS
        || scans.iter().any(has_qr_region)
}

/// Whether the foreground-threshold QR pass runs. Extra foreground thresholds reach
/// unresolved QR frames without a surviving region; discovery is bounded to
/// Full-HD-sized inputs. QR-only selections always take this fixed-cost pass. Mixed
/// selections take it in High and Very High where `qr_recovery_evidence` admits the
/// frame, and in Medium only on small frames without a QR region, so Medium stays
/// fast on camera frames without QR. Reads of other formats do not stop it.
#[cfg(not(feature = "low"))]
fn qr_foreground_wanted(
    mask: u32,
    matrix: u32,
    pixel_count: usize,
    scans: &[barcode_multiformat::Scan],
) -> bool {
    let selected = mask == QR_CODE
        || (matrix & QR_CODE != 0
            && (POLICY.qr_foreground_mixed_any_size || pixel_count <= SMALL_FRAME_PIXELS));
    selected
        && pixel_count <= QR_RECOVERY_MAX_PIXELS
        && !scans.iter().any(has_qr_read)
        && (POLICY.qr_foreground_with_region || !scans.iter().any(has_qr_region))
}

/// High and Very High retries of a single-format QR or `MaxiCode` reader group on
/// frames above Full HD: QR once on a 960-pixel copy, `MaxiCode` as in Medium.
#[cfg(any(feature = "high", feature = "very-high"))]
fn high_retries(
    pixels: &[u8],
    [width, height]: [usize; 2],
    matrix: u32,
    selected: u32,
    level: usize,
    scan: &barcode_multiformat::Scan,
) -> Vec<barcode_multiformat::Scan> {
    if selected == QR_CODE && scan.barcodes.is_empty() && exceeds_full_hd(width, height) {
        vec![scaled_matrix_retry(
            pixels, width, height, 960, selected, 1, false,
        )]
    } else if selected == MAXI_CODE
        && scan.barcodes.is_empty()
        && exceeds_full_hd(width, height)
        && (matrix == MAXI_CODE || !scan.regions.is_empty())
    {
        // MaxiCode on large frames, as in Medium: reduced copies first.
        let mut retries = Vec::new();
        for target in [1600, 1200] {
            let retry = scaled_matrix_retry(pixels, width, height, target, selected, level, false);
            let decoded = !retry.barcodes.is_empty();
            retries.push(retry);
            if decoded {
                break;
            }
        }
        retries
    } else {
        Vec::new()
    }
}

#[cfg(not(feature = "low"))]
/// Whether a scan read a QR symbol.
fn has_qr_read(scan: &barcode_multiformat::Scan) -> bool {
    scan.barcodes.iter().any(|read| read.format == "QRCode")
}

#[cfg(not(feature = "low"))]
/// Whether a scan left a candidate region of this format.
fn has_format_region(scan: &barcode_multiformat::Scan, format: &str) -> bool {
    scan.regions.iter().any(|region| region.format == format)
}

#[cfg(not(feature = "low"))]
/// Whether a scan left a QR candidate region.
fn has_qr_region(scan: &barcode_multiformat::Scan) -> bool {
    has_format_region(scan, "QRCode")
}

#[cfg(feature = "medium")]
fn medium_retries(
    pixels: &[u8],
    [width, height]: [usize; 2],
    matrix: u32,
    selected: u32,
    level: usize,
    scan: &barcode_multiformat::Scan,
) -> Vec<barcode_multiformat::Scan> {
    // Retries belong to single-format reader groups, such as the QR group of an
    // EAN-13 and QR selection; groups of several formats keep their first pass.
    let mask = selected;
    // Large label photos carry many short Code 128 fields. Once the sweep read one, a sweep
    // with twice the lines and diagonal angles finds the others; retail photos rarely pay.
    if mask & CODE128 != 0
        && mask & RETAIL_MASK == 0
        && is_label_sheet_sized(width, height)
        && scan.barcodes.iter().any(|b| b.format == "Code128")
    {
        return vec![scan_reader(pixels, width, height, mask, 1)];
    }
    if !scan.barcodes.is_empty() {
        return Vec::new();
    }
    let mut retries = Vec::new();
    if mask == DATA_BAR {
        let sparse = crate::fast_sparse::Prepared::new(pixels, width, height);
        retries.push(sparse.scan(mask, 0));
    }
    // A MaxiCode group of a mixed 2D selection retries only where it left a
    // candidate, so other selections do not pay for it on every large image.
    let maxi_candidate = matrix == MAXI_CODE || !scan.regions.is_empty();
    if (mask == QR_CODE || (mask == MAXI_CODE && maxi_candidate)) && exceeds_full_hd(width, height)
    {
        // QR: a second, finer scale when the coarse one reads nothing; shadowed
        // large photos often decode at one but not the other.
        let dimensions: &[usize] = if mask == QR_CODE {
            &[960, 1280]
        } else {
            &[1600, 1200]
        };
        // In a mixed 2D selection the finer QR scale runs only where the first pass or
        // the coarse scale left a QR candidate; a QR-only selection always tries both.
        let mut candidate = matrix == QR_CODE || !scan.regions.is_empty();
        for (i, &target) in dimensions.iter().enumerate() {
            if mask == QR_CODE && i > 0 && !candidate {
                break;
            }
            let retry = scaled_matrix_retry(pixels, width, height, target, mask, level, false);
            let decoded = !retry.barcodes.is_empty();
            candidate |= !retry.regions.is_empty();
            retries.push(retry);
            if decoded {
                break;
            }
        }
    }
    // Revisit an unresolved QR candidate with the existing extended reader.
    // Requiring a pending region avoids the expensive pass on blank misses.
    if mask == QR_CODE
        && !exceeds_full_hd(width, height)
        && scan.unfinished
        && !scan.regions.is_empty()
    {
        retries.push(scan_reader(pixels, width, height, mask, 2));
    }
    retries
}

#[cfg(all(test, not(feature = "low")))]
mod coverage_tests {
    use super::{contains_point, uncovered_mask};
    use crate::Proposal;
    #[test]
    fn only_contained_proposals_lose_retries() {
        let quad = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let adjacent = [[9., 0.], [19., 0.], [19., 10.], [9., 10.]];
        let mut proposals: Vec<_> = (0..63)
            .map(|_| Proposal {
                polygon: adjacent,
                score: 1.,
            })
            .collect();
        proposals[0].polygon = quad;
        proposals[62].polygon = quad;
        assert_eq!(
            uncovered_mask(&proposals, &[quad], u64::MAX),
            !1 & !(1 << 62)
        );
        assert_eq!(uncovered_mask(&proposals, &[quad], 1 << 63), 1 << 63);
        assert!(contains_point([0., 5.], &quad));
        assert!(!contains_point([0., 0.], &[[0., 0.]; 4]));
        assert!(!contains_point([f64::NAN, 0.], &quad));
    }
}

#[cfg(test)]
mod fragment_tests {
    use super::drop_unchecked_fragments;
    use crate::read::Read;

    fn read(format: &str, text: &str, polygon: crate::Quad) -> Read {
        let mut read = Read::primary([0; 13], polygon, 3, 0, vec![]);
        read.format = format.into();
        read.text = text.into();
        read
    }

    #[test]
    fn a_fragment_beside_the_full_outline_of_the_same_bars_is_dropped() {
        // The full read covers rows 100-160; the fragment decoded on rows 180-190.
        let full = read(
            "Industrial25",
            "286503",
            [[0., 100.], [300., 100.], [300., 160.], [0., 160.]],
        );
        let fragment = read(
            "Industrial25",
            "286",
            [[20., 180.], [170., 180.], [170., 190.], [20., 190.]],
        );
        let mut reads = vec![full.clone(), fragment.clone()];
        drop_unchecked_fragments(&mut reads);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "286503");
        // A second read of the same symbol from other rows is dropped too.
        let mut beside = full.clone();
        beside.polygon = [[5., 175.], [305., 175.], [305., 215.], [5., 215.]];
        let mut reads = vec![full.clone(), beside];
        drop_unchecked_fragments(&mut reads);
        assert_eq!(reads.len(), 1);
        // A symbol farther away than half the length, or crossing the axis, stays.
        for polygon in [
            [[20., 320.], [170., 320.], [170., 330.], [20., 330.]],
            [[100., 0.], [100., 150.], [110., 150.], [110., 0.]],
        ] {
            let mut separate = fragment.clone();
            separate.polygon = polygon;
            let mut reads = vec![full.clone(), separate];
            drop_unchecked_fragments(&mut reads);
            assert_eq!(reads.len(), 2);
        }
    }
}

#[cfg(test)]
mod result_tests;

#[cfg(test)]
mod grayscale_reuse_tests;

#[cfg(test)]
mod texture_tests;

fn retain_requested_primary(reads: &mut Vec<Read>, mask: u32) {
    reads.retain_mut(|read| {
        if mask & UPCA != 0 && read.text.starts_with('0') {
            read.text.remove(0);
            read.format = "UPCA".into();
            true
        } else {
            mask & EAN13 != 0
        }
    });
}

fn extend_primary_retail(reads: &mut Vec<Read>, primary: Option<&crate::Result>, mask: u32) {
    if let Some(primary) = primary {
        reads.extend(
            primary
                .retail
                .iter()
                .filter(|read| match read.format.as_str() {
                    "EAN8" => mask & EAN8 != 0,
                    "UPCE" => mask & UPCE != 0,
                    _ => false,
                })
                .cloned(),
        );
    }
}

#[cfg(not(feature = "low"))]
fn retry_format(format: &str) -> u32 {
    match format {
        "QRCode" => QR_CODE,
        "DataMatrix" => DATA_MATRIX,
        "Aztec" => AZTEC,
        _ => 0,
    }
}

/// Localization score of a QR region whose timing patterns read without error.
#[cfg(not(feature = "low"))]
const LOCATED_QR_SCORE: f32 = 1.;

/// Unresolved matrix regions retried per frame. The crop pixel budget bounds the work; a
/// sheet of small symbols leaves many small regions within it, often before any of its
/// symbols is read.
#[cfg(not(feature = "low"))]
const REGION_RETRIES: usize = 32;
/// Localized crop retries of pending matrix regions under one pixel allowance for the
/// whole matrix group, independent of how many formats are enabled. Initial readers
/// have already tried every format; finder hints prioritize bounded recovery without
/// discarding pending regions. A region that an `earlier` retry of the frame (a scaled
/// copy) has read is not retried.
#[cfg(not(feature = "low"))]
fn matrix_retries(
    pixels: &[u8],
    [width, height]: [usize; 2],
    mask: u32,
    scan: &mut barcode_multiformat::Scan,
    earlier: &[barcode_multiformat::Scan],
) -> Vec<barcode_multiformat::Scan> {
    let mut crop_budget = POLICY.matrix_crop_pixels;
    let mut reduced = POLICY.qr_reduced_region_retries;
    let mut located_crop = POLICY.qr_located_crop_pixels;
    let mut retries: Vec<barcode_multiformat::Scan> = Vec::new();
    // Pending recovery remains unfinished even when its shared allowance or
    // crop size cap prevents a retry, including candidates beyond those retried.
    scan.unfinished |= scan.regions.iter().any(|region| {
        let selected = retry_format(&region.format);
        mask & selected != 0
            && !scan.barcodes.iter().any(|read| {
                read.format == region.format
                    && crate::signal_recovery::covered(region.polygon, read.polygon)
            })
    });
    for region in scan.regions.iter().take(REGION_RETRIES) {
        let hinted = retry_format(&region.format);
        if mask & hinted == 0 {
            continue;
        }
        // Only complete containment can skip a localized retry. Small decoded
        // matrix polygons cannot suppress a larger unresolved candidate.
        if scan
            .barcodes
            .iter()
            .chain(earlier.iter().chain(&retries).flat_map(|s| &s.barcodes))
            .any(|r| {
                r.format == region.format
                    && crate::signal_recovery::covered(region.polygon, r.polygon)
            })
        {
            continue;
        }
        let pending = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![region.clone()],
            unfinished: true,
            lines: 0,
        };
        // A QR region whose timing patterns read without error is a located symbol whose
        // data did not decode. One that no crop allowance holds is retried once per frame
        // on a reduced copy; the first one too large for what the shared allowance has left
        // takes its own larger crop, so it does not use up what the other regions share.
        let located = hinted == QR_CODE && region.localization_score >= LOCATED_QR_SCORE;
        let crop = crate::signal_recovery::crop_bounds(region, width, height);
        if located && let Some(window @ [x0, y0, x1, y1]) = crop {
            let area = (x1 - x0) * (y1 - y0);
            if area > POLICY.qr_located_crop_pixels.max(POLICY.matrix_crop_pixels) {
                if reduced > 0 {
                    reduced -= 1;
                    retries.push(crate::signal_recovery::reduced_retry(
                        pixels,
                        width,
                        window,
                        hinted,
                        POLICY.matrix_crop_effort,
                    ));
                }
                continue;
            }
            if area > crop_budget && area <= located_crop {
                located_crop = 0;
                let mut allowance = area;
                retries.extend(crate::signal_recovery::region_retries(
                    pixels,
                    width,
                    height,
                    hinted,
                    POLICY.matrix_crop_effort,
                    &pending,
                    &mut allowance,
                ));
                continue;
            }
        }
        if crop_budget > 0 {
            retries.extend(crate::signal_recovery::region_retries(
                pixels,
                width,
                height,
                hinted,
                POLICY.matrix_crop_effort,
                &pending,
                &mut crop_budget,
            ));
        }
    }
    retries
}

#[cfg(all(test, not(feature = "low")))]
mod matrix_budget_tests {
    use super::{DATA_MATRIX, QR_CODE, matrix_retries};
    #[test]
    fn no_pending_candidates_do_not_invent_work() {
        let mut scan = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: Vec::new(),
            unfinished: false,
            lines: 0,
        };
        assert!(
            matrix_retries(&[255; 64], [8, 8], QR_CODE | DATA_MATRIX, &mut scan, &[]).is_empty()
        );
        assert!(!scan.unfinished);
    }

    #[test]
    fn a_located_qr_region_gets_a_larger_crop() {
        // A 240-pixel square region needs a 352 x 352 crop (123,904 pixels).
        let pending = |score: f32| barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![barcode_multiformat::regions::Region {
                format: "QRCode".into(),
                text: String::new(),
                polygon: [[400., 400.], [640., 400.], [640., 640.], [400., 640.]],
                localization_score: score,
                support: 4,
            }],
            unfinished: false,
            lines: 0,
        };
        let pixels = vec![200; 1000 * 1000];
        let retries =
            |score| matrix_retries(&pixels, [1000, 1000], QR_CODE, &mut pending(score), &[]);
        // Three crop methods, none of which reads the flat crop.
        assert_eq!(retries(1.).len(), 3);
        if super::POLICY.matrix_crop_pixels < 352 * 352 {
            assert!(retries(0.95).is_empty());
        }
    }

    #[test]
    fn only_the_first_located_qr_region_gets_the_larger_crop() {
        let region = |[x, y]: [f32; 2], side: f32| barcode_multiformat::regions::Region {
            format: "QRCode".into(),
            text: String::new(),
            polygon: [[x, y], [x + side, y], [x + side, y + side], [x, y + side]],
            localization_score: 1.,
            support: 4,
        };
        let mut scan = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![
                region([100., 100.], 240.),
                region([500., 500.], 240.),
                region([100., 800.], 40.),
            ],
            unfinished: false,
            lines: 0,
        };
        let pixels = vec![200; 1000 * 1000];
        let retries = matrix_retries(&pixels, [1000, 1000], QR_CODE, &mut scan, &[]);
        // The first large region takes the larger crop; the second fits neither that
        // (spent) nor the shared allowance, which the small region still gets whole.
        // Three crop methods each.
        if super::POLICY.matrix_crop_pixels < 352 * 352 {
            assert_eq!(retries.len(), 6);
        }
    }

    #[test]
    fn a_located_qr_region_too_large_for_a_crop_is_retried_reduced_once() {
        // Version 1-M "TAPIRSCAN" with 28-pixel modules: 588 pixels wide at (300, 300),
        // so its crop (about 840 pixels square) exceeds every crop allowance.
        const ROWS: [&str; 21] = [
            "111111101101001111111",
            "100000101100001000001",
            "101110100011001011101",
            "101110101110101011101",
            "101110100001001011101",
            "100000100111101000001",
            "111111101010101111111",
            "000000001111100000000",
            "101101110111101001011",
            "000001001001111100011",
            "011110110111000001001",
            "100011010001001011111",
            "001100100010100100000",
            "000000001101001010111",
            "111111101111100101101",
            "100000101010000110100",
            "101110100010111000111",
            "101110101101001101010",
            "101110101000101100000",
            "100000100110010100100",
            "111111101000010011010",
        ];
        let (width, height, scale) = (1400, 1300, 28);
        let mut pixels = vec![220; width * height];
        for (y, row) in ROWS.iter().enumerate() {
            for (x, bit) in row.bytes().enumerate() {
                if bit == b'1' {
                    for dy in 0..scale {
                        let start = (300 + y * scale + dy) * width + 300 + x * scale;
                        pixels[start..start + scale].fill(30);
                    }
                }
            }
        }
        let outline = [[300., 300.], [888., 300.], [888., 888.], [300., 888.]];
        let flat = [[950., 50.], [1350., 50.], [1350., 1250.], [950., 1250.]];
        // The detector's region lies inside the read's outline.
        let region = [[304., 304.], [884., 304.], [884., 884.], [304., 884.]];
        let pending = |score: f32| barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: [region, flat]
                .map(|polygon| barcode_multiformat::regions::Region {
                    format: "QRCode".into(),
                    text: String::new(),
                    polygon,
                    localization_score: score,
                    support: 4,
                })
                .to_vec(),
            unfinished: false,
            lines: 0,
        };
        let retries = matrix_retries(&pixels, [width, height], QR_CODE, &mut pending(1.), &[]);
        // One reduced retry per frame: the flat region beside the code gets none.
        assert_eq!(retries.len(), 1);
        let read = &retries[0].barcodes[0];
        assert_eq!(read.text, "TAPIRSCAN");
        for (corner, expected) in read.polygon.iter().zip(outline) {
            assert!(
                (corner[0] - expected[0]).abs() <= 6. && (corner[1] - expected[1]).abs() <= 6.,
                "{corner:?} vs {expected:?}"
            );
        }
        // Read by an earlier retry, the code is not retried again; the flat region now is.
        let again = matrix_retries(
            &pixels,
            [width, height],
            QR_CODE,
            &mut pending(1.),
            &retries,
        );
        assert_eq!(again.len(), 1);
        assert!(again[0].barcodes.is_empty());
        assert!(
            matrix_retries(&pixels, [width, height], QR_CODE, &mut pending(0.95), &[]).is_empty()
        );
    }
}

#[cfg(all(test, not(feature = "low")))]
mod pending_recovery_tests {
    use super::QR_CODE;
    #[test]
    fn oversized_pending_qr_remains_unfinished_without_a_retry() {
        let pending = |score: f32| barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![barcode_multiformat::regions::Region {
                format: "QRCode".into(),
                text: String::new(),
                polygon: [[0., 0.], [999., 0.], [999., 999.], [0., 999.]],
                localization_score: score,
                support: 1,
            }],
            unfinished: false,
            lines: 0,
        };
        let pixels = vec![255; 1_000_000];
        let mut scan = pending(0.95);
        assert!(super::matrix_retries(&pixels, [1000, 1000], QR_CODE, &mut scan, &[]).is_empty());
        assert!(scan.unfinished);
        assert_eq!(scan.regions.len(), 1);
        // A located one gets only its reduced retry, which reads nothing here.
        let mut scan = pending(1.);
        let retries = super::matrix_retries(&pixels, [1000, 1000], QR_CODE, &mut scan, &[]);
        assert_eq!(retries.len(), 1);
        assert!(retries.iter().all(|retry| retry.barcodes.is_empty()));
        assert!(scan.unfinished);
    }
}

/// Dispatch effort-preserving QR-only scans through the maintained finder frontend.
pub(crate) fn scan_reader(
    pixels: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
) -> barcode_multiformat::Scan {
    let mut binary_images = Thresholds::new(pixels, width, height);
    scan_reader_with(pixels, width, height, mask, effort, &mut binary_images)
}

/// [`scan_reader`] with a threshold cache of the same `pixels` that the frame's other
/// reader groups share.
fn scan_reader_with(
    pixels: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    binary_images: &mut Thresholds<'_>,
) -> barcode_multiformat::Scan {
    if mask & AZTEC != 0 && mask & !MATRIX_MASK == 0 {
        crate::matrix_frontend::scan(pixels, width, height, mask, effort, binary_images)
    } else if mask == QR_CODE {
        barcode_research_core::qr_frontend::scan_with_images(
            pixels,
            width,
            height,
            effort,
            binary_images,
        )
    } else {
        barcode_multiformat::scan_with_images(
            pixels,
            width,
            height,
            mask,
            effort,
            crate::MODE_ID as usize,
            binary_images,
            |_, _| {},
        )
    }
}

#[cfg(test)]
mod upscale_tests {
    #[test]
    fn tiny_qr_reads_enlarged_with_source_coordinates() {
        // Version 1-M "TAPIRSCAN" at two pixels per module.
        let rows = [
            "111111101101001111111",
            "100000101100001000001",
            "101110100011001011101",
            "101110101110101011101",
            "101110100001001011101",
            "100000100111101000001",
            "111111101010101111111",
            "000000001111100000000",
            "101101110111101001011",
            "000001001001111100011",
            "011110110111000001001",
            "100011010001001011111",
            "001100100010100100000",
            "000000001101001010111",
            "111111101111100101101",
            "100000101010000110100",
            "101110100010111000111",
            "101110101101001101010",
            "101110101000101100000",
            "100000100110010100100",
            "111111101000010011010",
        ];
        let (n, scale, margin) = (21, 2, 4);
        let side = (n + 2 * margin) * scale;
        let pixels: Vec<u8> = (0..side * side)
            .map(|i| {
                let (x, y) = (i % side / scale, i / side / scale);
                let dark = (margin..n + margin).contains(&x)
                    && (margin..n + margin).contains(&y)
                    && rows[y - margin].as_bytes()[x - margin] == b'1';
                if dark { 30 } else { 225 }
            })
            .collect();
        let scan = super::upscaled_qr_retry(&pixels, side, side, 3, 0);
        let read = scan.barcodes.first().expect("enlarged copy reads");
        assert_eq!(read.text, "TAPIRSCAN");
        let low = barcode_multiformat::numeric::usize_f32(margin * scale) - 1.;
        let high = barcode_multiformat::numeric::usize_f32((n + margin) * scale) + 1.;
        for point in read.polygon {
            assert!(point.iter().all(|v| (low..=high).contains(v)), "{point:?}");
        }
    }
}

#[cfg(all(test, not(feature = "low")))]
mod finer_localization_tests {
    /// Code 39 `-02532876` at two pixels per module and 30 rows in a 3200x2400
    /// frame: on the 768-pixel localization raster it is below the minimum
    /// stripe-group size. High and Very High localize at finer scale, Medium finds it
    /// through its segment-voting boxes; Low does not read it.
    #[test]
    fn small_linear_symbol_in_large_frame_reads_from_medium_up() {
        let bits = "1001011011010100101011011010100110110101011001010110110100110101011011001010101011001010110110100101101010100101101101011001101010100101101101";
        let (width, height) = (3200, 2400);
        let mut data = vec![255; width * height];
        for y in 800..830 {
            for (i, bit) in bits.bytes().enumerate() {
                if bit == b'1' {
                    data[y * width + 1066 + 2 * i..y * width + 1068 + 2 * i].fill(0);
                }
            }
        }
        let image = crate::Image {
            data: &data,
            width,
            height,
            channels: 1,
            stride: width,
        };
        let scan = crate::Scanner::default()
            .scan_formats_typed_with_addons(
                image,
                crate::ScanOptions {
                    multiple: true,
                    ..crate::ScanOptions::default()
                },
                32,
                super::EanAddOnPolicy::Ignore,
            )
            .unwrap();
        let texts: Vec<_> = scan.barcodes.iter().map(|b| b.text.as_str()).collect();
        if crate::MODE_ID >= 1 {
            assert_eq!(texts, ["-02532876"]);
        } else {
            assert!(texts.is_empty(), "{texts:?}");
        }
    }
}

#[cfg(all(test, not(feature = "low")))]
mod averaged_coordinate_tests {
    fn reference(
        pixels: &[u8],
        width: usize,
        height: usize,
        target_long: usize,
    ) -> (usize, usize, Vec<u8>) {
        let long = width.max(height);
        let out_width = (width * target_long / long).max(3);
        let out_height = (height * target_long / long).max(3);
        let bounds = |index: usize, out: usize, source: usize| {
            let start = index * source / out;
            (
                start,
                ((index + 1) * source / out).max(start + 1).min(source),
            )
        };
        // At most four evenly spaced samples per axis and output pixel keep the
        // pass cheap on camera frames while still averaging the blur.
        let mut smaller = vec![0_u8; out_width * out_height];
        for y in 0..out_height {
            let (y0, y1) = bounds(y, out_height, height);
            let y_samples = (y1 - y0).min(4);
            for x in 0..out_width {
                let (x0, x1) = bounds(x, out_width, width);
                let x_samples = (x1 - x0).min(4);
                let mut sum = 0_u32;
                for j in 0..y_samples {
                    let row = y0 + (2 * j + 1) * (y1 - y0) / (2 * y_samples);
                    for i in 0..x_samples {
                        let column = x0 + (2 * i + 1) * (x1 - x0) / (2 * x_samples);
                        sum += u32::from(pixels[row * width + column]);
                    }
                }
                let count = u32::try_from(y_samples * x_samples).unwrap_or(1);
                smaller[y * out_width + x] = u8::try_from((sum + count / 2) / count).unwrap_or(255);
            }
        }
        (out_width, out_height, smaller)
    }
    #[test]
    fn sample_tables_preserve_every_output_byte() {
        for (width, height) in [
            (3, 3),
            (5, 17),
            (31, 29),
            (127, 65),
            (513, 97),
            (17, 511),
            (65535, 3),
        ] {
            let pixels: Vec<u8> = (0..width * height)
                .map(|i| u8::try_from((i * 73 + i / 11) % 256).unwrap())
                .collect();
            for target in [1, 2, 3, 7, 16, 31, 64, 127, 256] {
                assert_eq!(
                    super::averaged_pixels(&pixels, width, height, target),
                    reference(&pixels, width, height, target),
                    "{width}x{height} at {target}"
                );
            }
        }
        let (width, height) = (4031, 3023);
        let pixels: Vec<u8> = (0..width * height)
            .map(|i| u8::try_from((i * 19 + i / 37) % 256).unwrap())
            .collect();
        assert_eq!(
            super::averaged_pixels(&pixels, width, height, 1008),
            reference(&pixels, width, height, 1008)
        );
    }
}
