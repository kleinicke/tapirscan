//! Optional retail evidence, with an independent assembly budget and no primary feedback.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
// Low intentionally compiles the zero-budget ROI path out.
#![allow(
    clippy::absurd_extreme_comparisons,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::struct_excessive_bools,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity
)]
use crate::{
    experiment::{AssociationBudget, Observation, Work},
    multi_profile::{self, retail_short::ShortReads, Reads},
    sampling::ImageView,
    scan::Quad,
};
use std::fmt::Write;
#[path = "retail_anchors.rs"]
mod anchors;
#[path = "retail_protection.rs"]
mod protection;
const PROTECT: bool = true;
#[path = "retail_peak_scratch.rs"]
mod peak_scratch;
#[path = "retail_peaks.rs"]
mod peaks;
#[path = "retail_roi.rs"]
mod roi;
#[cfg(feature = "mode-low")]
const ANCHORS: bool = false;
#[cfg(feature = "mode-medium")]
const ANCHORS: bool = true;
const LATE_ANCHORS: bool = true;
const CACHE_MODE: u8 = 0;
const SHARPEN: bool = false;
const TRACE: bool = false;
const SCRATCH: bool = true;
const ROW_REUSE: bool = true;
#[cfg(feature = "mode-low")]
const ROI_LIMIT: usize = 0;
#[cfg(feature = "mode-medium")]
const ROI_LIMIT: usize = 3;
const ROI_MODE: u8 = 2;
const ROI_ROWS: usize = 10;
const ROI_ORIENT: bool = false;
#[derive(Default)]
pub struct Collector {
    group_calls: Vec<usize>,
    primary: Vec<Quad>,
    protection_skips: usize,
    profile_calls: usize,
    gray_calls: usize,
    protection_rejects: usize,
    recovery_boxes: usize,
    recovery_paths: usize,
    recovery_samples: usize,
    scratch: peak_scratch::Scratch,
    peak_row: Vec<u8>,
    cache: Vec<Vec<(usize, u64, u64, u64, Vec<f32>)>>,
    peak_requests: usize,
    peak_unique: usize,
    peak_cached: usize,
    pub mask: u32,
    pub use_raw: bool,
    pub use_peaks: bool,
    pub peak_retry: bool,
    peak_counts: Vec<usize>,
    active: usize,
    groups: Vec<(Quad, Vec<Observation>)>,
    starts: Vec<usize>,
    paths: usize,
    calls: usize,
    capped: bool,
}
pub struct RetailResult {
    pub detections: Vec<crate::experiment::Detection>,
    pub unfinished: bool,
    pub diagnostics: Option<String>,
}

impl Collector {
    fn recovery_profile(
        &mut self,
        p: &[f32],
        primary: &Reads,
        axis: usize,
        f: f64,
        lo: f64,
        hi: f64,
    ) {
        if self.mask & 4 == 0 {
            return;
        }
        if self.gray_calls >= 64 {
            self.capped = true;
            return;
        }
        self.profile_calls += 1;
        let row: Vec<u8> = p
            .iter()
            .map(|v| ((1. - v) * 255.).round().clamp(0., 255.) as u8)
            .collect();
        let (reads, calls) = barcode_multiformat::recovery_gray(&row, 64 - self.gray_calls);
        self.gray_calls += calls;
        self.capped |= self.gray_calls >= 64;
        let mut out = ShortReads::default();
        for (text, left, right, _error) in reads {
            if primary
                .symbols
                .iter()
                .any(|r| r.left < right && left < r.right)
                || primary
                    .rejected_intervals
                    .iter()
                    .any(|r| r.0 < right && left < r.1)
            {
                continue;
            }
            let mut digits = [0; 8];
            for (d, b) in digits.iter_mut().zip(text.bytes()) {
                *d = b - b'0';
            }
            multi_profile::retail_short::insert(
                &mut out,
                multi_profile::retail_short::ShortRead {
                    digits,
                    format: 4,
                    left,
                    right,
                    cost: 0.12,
                    gap: 0.,
                    recovery: true,
                },
                64,
            );
        }
        self.append(out, axis, f, lo, hi, p.len());
    }

    pub fn reset(&mut self) {
        self.groups.clear();
        self.primary.clear();
        self.protection_skips = 0;
        self.profile_calls = 0;
        self.gray_calls = 0;
        self.protection_rejects = 0;
        self.group_calls.clear();
        self.recovery_boxes = 0;
        self.recovery_paths = 0;
        self.recovery_samples = 0;
        self.cache.clear();
        self.peak_requests = 0;
        self.peak_unique = 0;
        self.peak_cached = 0;
        self.peak_counts.clear();
        self.paths = 0;
        self.calls = 0;
        self.capped = false;
    }
    pub fn coverage(&mut self, q: Quad) {
        if self.mask & 12 == 0 {
            return;
        }
        if let Some(i) = self.groups.iter().position(|(a, _)| *a == q) {
            self.active = i;
        } else {
            self.active = self.groups.len();
            self.groups.push((q, Vec::new()));
            self.group_calls.push(0);
            self.cache.push(Vec::new());
            self.peak_counts.push(0);
        }
    }
    pub fn peaks(&mut self, p: &[f32], primary: &Reads, axis: usize, f: f64, lo: f64, hi: f64) {
        if self.mask & 12 == 0 || !self.use_peaks {
            return;
        }
        if self.groups.is_empty()
            || self.peak_counts[self.active] >= if self.peak_retry { 32 } else { 6 }
        {
            return;
        }
        self.peak_requests += 1;
        if CACHE_MODE != 2 {
            self.peak_counts[self.active] += 1;
        }
        if CACHE_MODE != 0 {
            let cache = &mut self.cache[self.active];
            if cache.iter().any(|(a, b, c, d, v)| {
                *a == axis
                    && *b == f.to_bits()
                    && *c == lo.to_bits()
                    && *d == hi.to_bits()
                    && v.as_slice() == p
            }) {
                self.peak_cached += 1;
                return;
            }
            cache.push((axis, f.to_bits(), lo.to_bits(), hi.to_bits(), p.to_vec()));
        }
        if CACHE_MODE == 2 {
            self.peak_counts[self.active] += 1;
        }
        self.peak_unique += 1;
        let row_owned;
        let row: &[u8] = if ROW_REUSE {
            self.peak_row.clear();
            self.peak_row.extend(
                p.iter()
                    .map(|v| ((1. - v) * 255.).round().clamp(0., 255.) as u8),
            );
            &self.peak_row
        } else {
            row_owned = p
                .iter()
                .map(|v| ((1. - v) * 255.).round().clamp(0., 255.) as u8)
                .collect::<Vec<_>>();
            &row_owned
        };
        let runs_owned;
        let runs: &[(f64, f64, bool)] = if SCRATCH {
            self.scratch.runs(row)
        } else {
            let (bits, widths, _) = peaks::runs(row);
            if bits.is_empty() {
                return;
            }
            let mut x = 0.;
            let mut dark = bits[0];
            runs_owned = widths
                .into_iter()
                .map(|w| {
                    let end = x + f64::from(w);
                    let r = (x, end, dark);
                    x = end;
                    dark = !dark;
                    r
                })
                .collect::<Vec<_>>();
            &runs_owned
        };
        let mut out = ShortReads::default();
        multi_profile::retail_short::decode_runs(runs, self.mask, 64, primary, &mut out);
        if SHARPEN && out.symbols.is_empty() && (24..=200).contains(&runs.len()) {
            let sharp = peaks::sharpen_row(row);
            let (bits, widths, _) = peaks::runs(&sharp);
            if !bits.is_empty() {
                let mut x = 0.;
                let mut dark = bits[0];
                let adjusted: Vec<_> = widths
                    .into_iter()
                    .map(|w| {
                        let end = x + f64::from(w);
                        let v = (x, end, dark);
                        x = end;
                        dark = !dark;
                        v
                    })
                    .collect();
                multi_profile::retail_short::decode_runs(
                    &adjusted,
                    self.mask & 4,
                    64,
                    primary,
                    &mut out,
                );
            }
        }
        if out.symbols.is_empty() {
            multi_profile::retail_short::legacy_runs(runs, self.mask & 12, 64, primary, &mut out);
        }
        self.append(out, axis, f, lo, hi, p.len());
    }
    pub fn raw(
        &mut self,
        runs: &[(usize, usize, bool)],
        primary: &Reads,
        axis: usize,
        f: f64,
        lo: f64,
        hi: f64,
        n: usize,
    ) {
        if self.mask & 12 == 0 || !self.use_raw {
            return;
        }
        let mut out = ShortReads::default();
        multi_profile::retail_short::decode_runs(runs, self.mask, 64, primary, &mut out);
        self.append(out, axis, f, lo, hi, n);
    }
    pub fn local(
        &mut self,
        s: &crate::local_signal::Scratch,
        primary: &Reads,
        axis: usize,
        f: f64,
        lo: f64,
        hi: f64,
        n: usize,
    ) {
        if self.mask & 12 == 0 {
            return;
        }
        let mut out =
            multi_profile::short_from_existing(&s.runs, self.mask, 64, primary, &mut self.starts);
        multi_profile::short_from_extrema(&s.extrema, self.mask, 64, primary, &mut out);
        self.append(out, axis, f, lo, hi, n);
    }
    fn append(&mut self, out: ShortReads, axis: usize, f: f64, lo: f64, hi: f64, n: usize) {
        self.paths += 1;
        self.calls += out.digit_calls;
        self.capped |= out.truncated;
        if self.groups.is_empty() {
            return;
        }
        self.group_calls[self.active] += out.digit_calls;
        let rows = &mut self.groups[self.active].1;
        for r in out.symbols {
            let mut digits = [0; 13];
            digits[0] = 10 + r.format as u8;
            digits[5..].copy_from_slice(&r.digits);
            let o = Observation {
                digits,
                axis,
                fraction: f,
                left: lo + (hi - lo) * (r.left + 0.5) / n as f64,
                right: lo + (hi - lo) * (r.right + 0.5) / n as f64,
                cost: r.cost,
                gap: r.gap,
                short_quiet: r.recovery,
                ambiguous: false,
            };
            if rows.iter().any(|a| {
                a.digits == o.digits
                    && a.axis == axis
                    && (a.fraction - f).abs() < 1e-9
                    && a.right.min(o.right) - a.left.max(o.left)
                        > 0.9 * (a.right - a.left).max(o.right - o.left)
            }) {
                continue;
            }
            if rows.len() >= 512 {
                self.capped = true;
                break;
            }
            rows.push(o);
        }
        for (l, r) in out.conflicts {
            if rows.len() >= 512 {
                self.capped = true;
                break;
            }
            rows.push(Observation {
                digits: [0; 13],
                axis,
                fraction: f,
                left: lo + (hi - lo) * (l + 0.5) / n as f64,
                right: lo + (hi - lo) * (r + 0.5) / n as f64,
                cost: 0.,
                gap: 0.,
                short_quiet: false,
                ambiguous: true,
            });
        }
    }
    pub fn finish(&mut self, im: ImageView<'_>) -> String {
        self.finish_typed(im, true).diagnostics.unwrap()
    }

    pub fn finish_typed(&mut self, im: ImageView<'_>, diagnostics: bool) -> RetailResult {
        let mut budget = AssociationBudget {
            checks_left: 100_000,
            pixels_left: 1_000_000,
        };
        let mut work = Work::default();
        let mut detections = Vec::new();
        let mut done = Vec::new();
        for (q, obs) in &self.groups {
            let Ok(m) = crate::scan::transform(*q) else {
                done.push(false);
                continue;
            };
            let found = crate::experiment::assemble_many_budget_options(
                im,
                m.0,
                obs,
                &mut work,
                true,
                &mut budget,
                false,
            );
            done.push(!found.is_empty());
            for d in found {
                if detections.iter().any(|a: &crate::experiment::Detection| {
                    a.digits == d.digits && crate::frame::same_space(a.polygon, d.polygon)
                }) {
                    continue;
                }
                detections.push(d);
            }
        }
        if ANCHORS && !LATE_ANCHORS {
            for d in self.recover_anchors(im, &done, &mut work, &mut budget) {
                if !detections
                    .iter()
                    .any(|a| a.digits == d.digits && crate::frame::same_space(a.polygon, d.polygon))
                {
                    detections.push(d);
                }
            }
        }
        if ROI_LIMIT > 0 {
            for d in self.recover_boxes(im, &done, &mut work, &mut budget) {
                if !detections
                    .iter()
                    .any(|a| a.digits == d.digits && crate::frame::same_space(a.polygon, d.polygon))
                {
                    detections.push(d);
                }
            }
        }
        if ANCHORS && LATE_ANCHORS {
            let after_done: Vec<bool> = (0..self.groups.len())
                .map(|i| done.get(i).copied().unwrap_or(false))
                .collect();
            for d in self.recover_anchors(im, &after_done, &mut work, &mut budget) {
                if !detections
                    .iter()
                    .any(|a| a.digits == d.digits && crate::frame::same_space(a.polygon, d.polygon))
                {
                    detections.push(d);
                }
            }
        }
        let before = detections.len();
        detections.retain(|d| !self.protected_detection(d));
        self.protection_rejects += before - detections.len();
        let diagnostics = diagnostics.then(|| self.format_result(&detections, &work));
        RetailResult {
            detections,
            unfinished: self.capped || work.association_truncated > 0,
            diagnostics,
        }
    }

    fn format_result(&self, detections: &[crate::experiment::Detection], work: &Work) -> String {
        let mut out = String::from("{\"barcodes\":[");
        for (i, d) in detections.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let text: String = d.digits[5..]
                .iter()
                .map(|v| char::from(b'0' + *v))
                .collect();
            let format = if d.digits[0] == 14 { "EAN8" } else { "UPCE" };
            write!(
                out,
                "{{\"format\":\"{format}\",\"text\":\"{text}\",\"polygon\":{:?},\"support\":{}}}",
                d.polygon, d.support
            )
            .unwrap();
        }
        write!(out,"],\"paths\":{},\"digitCalls\":{},\"observations\":{},\"checks\":{},\"pixels\":{},\"unfinished\":{}}}",self.paths,self.calls,self.groups.iter().map(|(_,o)|o.len()).sum::<usize>(),work.association_checks,work.continuity_samples,self.capped||work.association_truncated>0).unwrap();
        out.pop();
        write!(
            out,
            ",\"peakRequests\":{},\"peakUnique\":{},\"peakCached\":{}}}",
            self.peak_requests, self.peak_unique, self.peak_cached
        )
        .unwrap();
        out.pop();
        write!(
            out,
            ",\"recoveryBoxes\":{},\"recoveryPaths\":{},\"recoverySamples\":{}}}",
            self.recovery_boxes, self.recovery_paths, self.recovery_samples
        )
        .unwrap();
        out.pop();
        write!(
            out,
            ",\"protectionSkips\":{},\"protectionRejects\":{}}}",
            self.protection_skips, self.protection_rejects
        )
        .unwrap();
        out.pop();
        write!(out, ",\"recoveryProfileCalls\":{}}}", self.profile_calls).unwrap();
        out.pop();
        write!(out, ",\"grayCalls\":{}}}", self.gray_calls).unwrap();
        if TRACE {
            out.pop();
            out.push_str(",\"evidence\":[");
            for (i, (q, rows)) in self.groups.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write!(
                    out,
                    "{{\"quad\":{:?},\"calls\":{},\"rows\":[",
                    q, self.group_calls[i]
                )
                .unwrap();
                for (j, o) in rows.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    write!(out,"{{\"digits\":{:?},\"axis\":{},\"fraction\":{},\"left\":{},\"right\":{},\"cost\":{},\"gap\":{},\"ambiguous\":{}}}",o.digits,o.axis,o.fraction,o.left,o.right,o.cost,o.gap,o.ambiguous).unwrap();
                }
                out.push_str("]}");
            }
            out.push_str("]}");
        }
        out
    }
}
