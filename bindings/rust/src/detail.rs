//! Port of the pinned source-detail JS composition. Raw crop frames keep local indices.
//! Coordinate arithmetic uses validated image bounds (at most 32 Mi pixels).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use crate::read::{Read, Recovery, Region};
use crate::{Barcode, Error, Image, Proposal, Quad};
use serde_json::{json, Value};

type Point = [f64; 2];
fn pixel(im: Image<'_>, x: usize, y: usize, c: usize) -> f64 {
    let channel = if im.channels == 1 { 0 } else { c };
    if c == 3 {
        255.
    } else {
        f64::from(im.data[y * im.stride + x * im.channels + channel])
    }
}
fn gray(im: Image<'_>, x: usize, y: usize) -> f64 {
    (77. * pixel(im, x, y, 0) + 150. * pixel(im, x, y, 1) + 29. * pixel(im, x, y, 2)) / 256.
}
fn sample(im: Image<'_>, x: f64, y: f64) -> f64 {
    let x = x.clamp(0., (im.width - 1) as f64);
    let y = y.clamp(0., (im.height - 1) as f64);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(im.width - 1);
    let y1 = (y0 + 1).min(im.height - 1);
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    (gray(im, x0, y0) * (1. - fx) + gray(im, x1, y0) * fx) * (1. - fy)
        + (gray(im, x0, y1) * (1. - fx) + gray(im, x1, y1) * fx) * fy
}
// Keep the pinned JavaScript equations and operation order directly comparable.
#[allow(clippy::many_single_char_names, clippy::manual_midpoint)]
fn evidence(im: Image<'_>, polygon: Quad, local: bool, target: usize) -> usize {
    let mut best = 0;
    for axis in 0..2 {
        for f in [0.2, 0.4, 0.6, 0.8] {
            let a = polygon[axis];
            let b = polygon[(axis + 1) % 4];
            let c = polygon[(axis + 2) % 4];
            let d = polygon[(axis + 3) % 4];
            let p = [a[0] * (1. - f) + d[0] * f, a[1] * (1. - f) + d[1] * f];
            let q = [b[0] * (1. - f) + c[0] * f, b[1] * (1. - f) + c[1] * f];
            let n = (((q[0] - p[0]).hypot(q[1] - p[1]) * 1.25).ceil() as usize).clamp(64, 2048);
            let mut samples = Vec::with_capacity(n);
            let mut lo = 255_f64;
            let mut hi = 0_f64;
            for i in 0..n {
                let t = -0.125 + 1.25 * i as f64 / (n - 1) as f64;
                let v = sample(im, p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t);
                samples.push(f64::from(v as f32));
                lo = lo.min(v);
                hi = hi.max(v);
            }
            if hi - lo < if local { 12. } else { 20. } {
                continue;
            }
            let threshold = (lo + hi) / 2.;
            let hysteresis = (hi - lo) * 0.06;
            let mut state = samples[0] > threshold;
            let mut runs = 0;
            for v in &samples {
                if if state {
                    *v < threshold - hysteresis
                } else {
                    *v > threshold + hysteresis
                } {
                    state = !state;
                    runs += 1;
                }
            }
            best = best.max(runs);
            if best >= target {
                return best;
            }
            if local {
                let mut sum = vec![0.; n + 1];
                for i in 0..n {
                    sum[i + 1] = sum[i] + samples[i];
                }
                let mut previous = None;
                let mut runs = 0;
                for (i, v) in samples.iter().enumerate() {
                    let left = i.saturating_sub(8);
                    let right = (i + 9).min(n);
                    let mean = (sum[right] - sum[left]) / (right - left) as f64;
                    let next = if *v > mean + 2. {
                        Some(true)
                    } else if *v < mean - 2. {
                        Some(false)
                    } else {
                        None
                    };
                    if let Some(next) = next {
                        if previous.is_some_and(|p| p != next) {
                            runs += 1;
                        }
                        previous = Some(next);
                    }
                }
                best = best.max(runs);
                if best >= target {
                    return best;
                }
            }
        }
    }
    best
}
pub fn retry_mask(im: Image<'_>, proposals: &[Proposal]) -> u64 {
    let mut mask = 0;
    for (i, p) in proposals.iter().enumerate() {
        if evidence(im, p.polygon, false, 48) >= 48 {
            mask |= 1_u64 << i;
        }
    }
    if proposals.len() < 64 {
        mask |= 1_u64 << proposals.len();
    }
    mask
}
#[derive(Clone, Copy)]
struct Seed {
    x: f64,
    y: f64,
    score: f64,
}
fn seeds(im: Image<'_>) -> Vec<Seed> {
    let mut cells = Vec::new();
    for y in (1..im.height.saturating_sub(24)).step_by(24) {
        for x in (1..im.width.saturating_sub(24)).step_by(24) {
            let (mut xx, mut yy, mut xy, mut sx, mut sy, mut n) = (0., 0., 0., 0., 0., 0.);
            for j in (y..y + 24).step_by(4) {
                for i in (x..x + 24).step_by(4) {
                    let v = gray(im, i, j);
                    let gx = gray(im, i + 1, j) - v;
                    let gy = gray(im, i, j + 1) - v;
                    xx += gx * gx;
                    yy += gy * gy;
                    xy += gx * gy;
                    sx += gx;
                    sy += gy;
                    n += 1.;
                }
            }
            xx = (xx - sx * sx / n).max(0.);
            yy = (yy - sy * sy / n).max(0.);
            xy -= sx * sy / n;
            let energy = (xx + yy) / n;
            let coherence = ((xx - yy).powi(2) + 4. * xy * xy).sqrt() / (xx + yy + 1.);
            let score = energy.sqrt() * coherence * coherence;
            if score > 5. {
                cells.push(Seed {
                    x: (x + 12) as f64,
                    y: (y + 12) as f64,
                    score,
                });
            }
        }
    }
    cells.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut selected: Vec<Seed> = Vec::new();
    for c in cells {
        if selected.iter().any(|s| (s.x - c.x).hypot(s.y - c.y) < 128.) {
            continue;
        }
        selected.push(c);
        if selected.len() == 2 {
            break;
        }
    }
    selected
}
// Keep the pinned JavaScript equations and operation order directly comparable.
#[allow(clippy::many_single_char_names)]
fn refined_angle(im: Image<'_>, seed: Seed) -> f64 {
    let (mut xx, mut yy, mut xy, mut sx, mut sy, mut n) = (0., 0., 0., 0., 0., 0.);
    for y in ((seed.y.round() as usize).saturating_sub(12).max(1))
        ..((seed.y + 12.).ceil() as usize).min(im.height - 1)
    {
        for x in ((seed.x.round() as usize).saturating_sub(12).max(1))
            ..((seed.x + 12.).ceil() as usize).min(im.width - 1)
        {
            let a = gray(im, x - 1, y - 1);
            let b = gray(im, x, y - 1);
            let c = gray(im, x + 1, y - 1);
            let d = gray(im, x - 1, y);
            let e = gray(im, x + 1, y);
            let f = gray(im, x - 1, y + 1);
            let g = gray(im, x, y + 1);
            let h = gray(im, x + 1, y + 1);
            let gx = (3. * (c - a) + 10. * (e - d) + 3. * (h - f)) / 16.;
            let gy = (3. * (f - a) + 10. * (g - b) + 3. * (h - c)) / 16.;
            xx += gx * gx;
            yy += gy * gy;
            xy += gx * gy;
            sx += gx;
            sy += gy;
            n += 1.;
        }
    }
    0.5 * (2. * (xy - sx * sy / n)).atan2(xx - sx * sx / n - yy + sy * sy / n)
}
fn contains(point: Point, q: Quad) -> bool {
    let mut hit = false;
    let mut j = 3;
    for i in 0..4 {
        if (q[i][1] > point[1]) != (q[j][1] > point[1])
            && point[0] < (q[j][0] - q[i][0]) * (point[1] - q[i][1]) / (q[j][1] - q[i][1]) + q[i][0]
        {
            hit = !hit;
        }
        j = i;
    }
    hit
}
// Image-bounded coordinates cannot overflow; preserve upstream arithmetic.
#[allow(clippy::manual_midpoint)]
fn continuous(im: Image<'_>, point: Point, p: Quad) -> bool {
    let left = [(p[0][0] + p[3][0]) / 2., (p[0][1] + p[3][1]) / 2.];
    let right = [(p[1][0] + p[2][0]) / 2., (p[1][1] + p[2][1]) / 2.];
    let dx = right[0] - left[0];
    let dy = right[1] - left[1];
    let width = dx.hypot(dy);
    if width < 45. {
        return false;
    }
    let nx = -dy / width;
    let ny = dx / width;
    let along = ((point[0] - left[0]) * dx + (point[1] - left[1]) * dy) / (width * width);
    let offset = (point[0] - left[0]) * nx + (point[1] - left[1]) * ny;
    if !(0. ..=1.).contains(&along) || offset.abs() > 128_f64.min(width * 0.5) {
        return false;
    }
    let count = ((width * 1.5).round() as usize).clamp(95, 256);
    let profile = |displacement: f64| {
        let mut values = Vec::with_capacity(count);
        let (mut sum, mut energy) = (0., 0.);
        for i in 0..count {
            let fraction = 0.02 + 0.96 * i as f64 / (count - 1) as f64;
            let v = sample(
                im,
                left[0] + dx * fraction + nx * displacement,
                left[1] + dy * fraction + ny * displacement,
            );
            values.push(v);
            sum += v;
            energy += v * v;
        }
        (
            values,
            sum / count as f64,
            (energy / count as f64 - (sum / count as f64).powi(2))
                .max(0.)
                .sqrt(),
        )
    };
    let (reference, mean, deviation) = profile(0.);
    if deviation < 5. {
        return false;
    }
    let agrees = |displacement| {
        let (row, row_mean, row_deviation) = profile(displacement);
        if row_deviation < deviation * 0.65 || row_deviation < 5. {
            return false;
        }
        let covariance: f64 = reference
            .iter()
            .zip(row)
            .map(|(a, b)| (a - mean) * (b - row_mean))
            .sum();
        covariance / (count as f64 * deviation * row_deviation) > 0.8
    };
    if !agrees(offset) || !agrees(offset / 2.) {
        return false;
    }
    let steps = (offset.abs() * 2.).ceil() as usize;
    (1..steps).all(|i| agrees(offset * i as f64 / steps as f64))
}
fn covered(im: Image<'_>, point: Point, p: Quad) -> bool {
    contains(point, p) || continuous(im, point, p)
}
// Keep detail recovery for short symbols with undersampled modules or weak
// support: the enlarged crop can still improve their geometry and confidence.
fn resolved_retail(read: &Read) -> bool {
    let modules = match read.format.as_str() {
        "EAN8" => 67.,
        "UPCE" => 51.,
        _ => return false,
    };
    let p = read.polygon;
    let width = (p[1][0] - p[0][0])
        .hypot(p[1][1] - p[0][1])
        .min((p[2][0] - p[3][0]).hypot(p[2][1] - p[3][1]));
    read.support >= 4 && width >= modules * 3. && span(p) >= 2.
}

fn span(p: Quad) -> f64 {
    let dx = p[1][0] - p[0][0];
    let dy = p[1][1] - p[0][1];
    (dx * (p[3][1] - p[0][1]) - dy * (p[3][0] - p[0][0])).abs() / dx.hypot(dy).max(1e-9)
}
// Keep the pinned JavaScript equations and operation order directly comparable.
#[allow(clippy::many_single_char_names)]
fn upscale(im: Image<'_>, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut data = vec![0; w * h * 9 * 4];
    for row in 0..h * 3 {
        let sy = ((row as f64 + 0.5) / 3. - 0.5).clamp(0., (h - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(h - 1);
        let fy = sy - y0 as f64;
        for col in 0..w * 3 {
            let sx = ((col as f64 + 0.5) / 3. - 0.5).clamp(0., (w - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(w - 1);
            let fx = sx - x0 as f64;
            for c in 0..4 {
                let a = pixel(im, x + x0, y + y0, c);
                let b = pixel(im, x + x1, y + y0, c);
                let d = pixel(im, x + x0, y + y1, c);
                let e = pixel(im, x + x1, y + y1, c);
                data[(row * w * 3 + col) * 4 + c] = ((a * (1. - fx) + b * fx) * (1. - fy)
                    + (d * (1. - fx) + e * fx) * fy)
                    .round() as u8;
            }
        }
    }
    data
}
#[derive(Clone, Copy)]
pub(super) struct RecoveryOptions {
    pub directions: usize,
    pub complete: bool,
    pub shared_retail: bool,
    pub diagnostics: bool,
}

/// Runtime evidence is returned separately to preserve each crop's candidate namespace.
// Keep the pinned JavaScript equations and operation order directly comparable.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub fn recover(
    im: Image<'_>,
    primary: &mut Vec<Barcode>,
    scanner: &mut recovery_core::region_scan::RegionScanner,
    coverage: &[Quad],
    known_retail: &[Read],
    options: RecoveryOptions,
) -> Result<Recovery, Error> {
    let RecoveryOptions {
        directions,
        complete,
        shared_retail,
        diagnostics,
    } = options;
    let start = crate::timer::Timer::start();
    let seeds = seeds(im);
    let mut attempts = Vec::new();
    let mut proposals = Vec::new();
    let mut additions = Vec::new();
    let mut unread = Vec::new();
    let mut retail_reads = Vec::new();
    for seed in &seeds {
        if coverage
            .iter()
            .any(|quad| crate::formats::contains_point([seed.x, seed.y], quad))
            || known_retail.iter().chain(&retail_reads).any(|read: &Read| {
                resolved_retail(read) && covered(im, [seed.x, seed.y], read.polygon)
            })
            || seed.score < seeds[0].score * 0.6
            || primary
                .iter()
                .any(|b| covered(im, [seed.x, seed.y], b.detection.polygon))
        {
            continue;
        }
        let angle = refined_angle(im, *seed);
        let mut quads = Vec::new();
        for offset in [0_f64, -3., 3.] {
            let angle = angle + offset * std::f64::consts::PI / 180.;
            let (c, s) = (angle.cos(), angle.sin());
            let polygon = [[-96., -24.], [96., -24.], [96., 24.], [-96., 24.]]
                .map(|[u, v]| [seed.x + u * c - v * s, seed.y + u * s + v * c]);
            if evidence(im, polygon, true, 32) >= 32 {
                quads.push(polygon);
            }
            if quads.len() >= directions {
                break;
            }
        }
        if quads.is_empty() {
            continue;
        }
        let w = 256.min(im.width);
        let h = 256.min(im.height);
        let x = (seed.x - w as f64 / 2.)
            .round()
            .clamp(0., (im.width - w) as f64) as usize;
        let y = (seed.y - h as f64 / 2.)
            .round()
            .clamp(0., (im.height - h) as f64) as usize;
        let pixels = upscale(im, x, y, w, h);
        let view = recovery_core::sampling::ImageView::new(&pixels, w * 3, h * 3, 4, w * 12)
            .map_err(|_| Error::Parameters)?;
        for polygon in quads {
            let q = polygon.map(|[a, b]| [(a - x as f64) * 3., (b - y as f64) * 3.]);
            let policy = recovery_core::multi_scan::Policy {
                complete,
                transition_cleanup: true,
                source_identity: true,
                interior_normalization: true,
                guard_bias: true,
                max_retry_paths_per_candidate: 64,
                max_retry_paths_per_frame: 64,
                ..Default::default()
            };
            scanner
                .retail_configure(if shared_retail { 15 } else { 1 })
                .map_err(|_| Error::Parameters)?;
            let result = scanner
                .scan(view, &[q], policy)
                .map_err(|_| Error::Parameters)?;
            let mut raw: Option<Value> = if diagnostics {
                Some(
                    serde_json::from_str(&recovery_core::region_json::frame_json(&result.frame))
                        .map_err(|_| Error::OutputShape)?,
                )
            } else {
                None
            };
            if let Some(retail) = scanner.retail_finish_typed(view, &result.frame, diagnostics) {
                if let Some(raw) = &mut raw {
                    raw["retail"] = serde_json::from_str(
                        retail.diagnostics.as_deref().ok_or(Error::OutputShape)?,
                    )
                    .map_err(|_| Error::OutputShape)?;
                }
                for d in retail.detections {
                    let polygon = d
                        .polygon
                        .map(|[a, b]| [x as f64 + a / 3., y as f64 + b / 3.]);
                    retail_reads.push(Read::retail(d.digits, polygon, d.support));
                }
            }
            let mut reads = Vec::new();
            let mut deferred = Vec::new();
            let mut seed_covered = false;
            for b in &result.frame.barcodes {
                let d = &b.detection;
                let p = d
                    .polygon
                    .map(|[a, b]| [x as f64 + a / 3., y as f64 + b / 3.]);
                let read =
                    Read::primary(d.digits, p, d.support, d.axis, b.candidate_indices.clone());
                if span(p) < 1. {
                    deferred.push(read);
                    continue;
                }
                seed_covered |= covered(im, [seed.x, seed.y], p);
                let center = p
                    .iter()
                    .fold([0., 0.], |a, p| [a[0] + p[0] / 4., a[1] + p[1] / 4.]);
                if !primary.iter().any(|old| {
                    old.detection.digits == d.digits && covered(im, center, old.detection.polygon)
                }) {
                    if diagnostics {
                        additions.push(read.clone());
                    }
                    primary.push(Barcode {
                        detection: barcode_research_core::experiment::Detection {
                            digits: d.digits,
                            polygon: p,
                            support: d.support,
                            axis: d.axis,
                        },
                        candidate_indices: vec![],
                    });
                }
                reads.push(read);
            }
            if !reads.iter().any(|read| {
                read.candidate_indices
                    .as_ref()
                    .is_some_and(|indices| indices.contains(&0))
            }) {
                unread.push(Region::unknown(polygon));
            }
            if diagnostics {
                // Recovery reads historically omit format; keep the raw diagnostic schema.
                let raw_reads = |reads: &[Read]| {
                    reads.iter().map(|read| json!({"text":read.text,"polygon":read.polygon,"support":read.support,"axis":read.axis,"candidate_indices":read.candidate_indices})).collect::<Vec<_>>()
                };
                let proposal = json!({"polygon":polygon,"score":seed.score,"text":""});
                proposals.push(proposal.clone());
                attempts.push(json!({"x":x,"y":y,"w":w,"h":h,"factor":3,"frame":raw,"reads":raw_reads(&reads),"deferredReads":raw_reads(&deferred),"proposals":[proposal],"unfinished":result.frame.unfinished||!deferred.is_empty()}));
            }
            if seed_covered {
                break;
            }
        }
    }
    let diagnostics = diagnostics.then(|| {
        let additions: Vec<_> = additions.iter().map(|read| json!({"text":read.text,"polygon":read.polygon,"support":read.support,"axis":read.axis,"candidate_indices":read.candidate_indices})).collect();
        json!({"additions":additions,"attempts":attempts,"proposals":proposals,"extraMs":start.elapsed().as_secs_f64()*1000.,"searchLimited":true})
    });
    Ok(Recovery {
        unread,
        retail: retail_reads,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_retail_coverage_preserves_small_or_weak_detail_recovery() {
        let polygon = |width| {
            [
                [10., 10.],
                [10. + width, 10.],
                [10. + width, 30.],
                [10., 30.],
            ]
        };
        let mut digits = [0; 13];
        digits[0] = 14;
        assert!(!resolved_retail(&Read::retail(digits, polygon(200.), 8)));
        assert!(!resolved_retail(&Read::retail(digits, polygon(240.), 3)));
        assert!(resolved_retail(&Read::retail(digits, polygon(240.), 4)));
        digits[0] = 15;
        assert!(!resolved_retail(&Read::retail(digits, polygon(150.), 8)));
        assert!(resolved_retail(&Read::retail(digits, polygon(180.), 4)));
    }
}
