//! EAN-2/EAN-5 supplements read from grayscale profiles next to a decoded EAN-13/UPC-A.
//!
//! The run-length reader ([`crate::linear`]) needs every narrow bar and space of a
//! supplement to survive thresholding on the same scanline that decodes the main
//! symbol. Camera frames at 1-2 pixels per module with some blur lose them, although
//! the main value is read. This reader starts from the decoded main symbol instead:
//! its known bar pattern locates the symbol on bilinear profiles across the symbol
//! height and measures module width, its change under perspective, blur and ink
//! levels. Supplement digits are then chosen by comparing the profile right of the
//! main symbol with blurred models of every digit code, with the guard, separators
//! and quiet zones fixed and the main symbol's geometry extended past its end.
//!
//! A supplement is accepted only when its parity pattern matches its value, every
//! digit wins clearly over the next best code, the gap, the supplement and the quiet
//! zone after it fit about as well as the main symbol, on at least two profiles, with
//! no profile reading a different value.
//!
//! The blur kernel is a cubic B-spline (close to a Gaussian); its integral is a
//! piecewise polynomial, so results do not depend on platform `exp` implementations.

use std::cmp::Ordering;

use crate::linear::DIGITS;

/// Whether `value > limit`; false for NaN.
fn exceeds(value: f32, limit: f32) -> bool {
    value.partial_cmp(&limit) == Some(Ordering::Greater)
}

/// Whether `value >= limit`; false for NaN.
fn reaches(value: f32, limit: f32) -> bool {
    matches!(
        value.partial_cmp(&limit),
        Some(Ordering::Greater | Ordering::Equal)
    )
}

/// Left-half parity of EAN-13 per leading digit; bit 5 is the first digit, set for G codes.
const PARITY13: [u8; 10] = [0, 11, 13, 14, 19, 25, 28, 21, 22, 26];
/// UPC-E parity per check digit (number system 0); bit 5 is the first digit, set for G codes.
const PARITY_UPCE: [u8; 10] = [56, 52, 50, 49, 44, 38, 35, 42, 41, 37];
/// EAN-5 parity pattern per check value; bit 4 is the first digit, set for G codes.
const PARITY5: [u8; 10] = [24, 20, 18, 17, 12, 6, 3, 10, 9, 5];
/// Profile samples per module.
const SAMPLES_PER_MODULE: f32 = 4.;
/// Modules sampled before the main symbol's estimated start.
const LEADING_MODULES: f32 = 12.;
/// Modules sampled after the main symbol's estimated end: widest gap plus EAN-5 and quiet zone.
const TRAILING_MODULES: f32 = 76.;
/// Supplement gaps tried, in modules. The specification asks for 7-12; printed
/// symbols also use narrower or wider gaps.
const GAP_RANGE: (f32, f32) = (4.5, 16.);
/// Gap hypotheses decoded per profile, best guard fits first.
const GAP_HYPOTHESES: usize = 3;
/// Supplement module widths tried relative to the main symbol's extended geometry.
const SUPPLEMENT_SCALES: [f32; 3] = [1., 0.96, 1.04];
/// Blur widths tried for the main symbol, as standard deviations in modules.
const SIGMAS: [f32; 7] = [0.25, 0.35, 0.45, 0.55, 0.65, 0.8, 0.95];
/// Index of the blur width the coarse searches use.
const COARSE_SIGMA: usize = 3;
/// Main-symbol correlation required before a profile is used.
const MIN_MAIN_CORRELATION: f32 = 0.8;
/// Main-symbol correlation that settles the reading direction.
const CLEAR_CORRELATION: f32 = 0.9;
/// Bar-like darkness peaks required beside the symbol before any fitting.
const MIN_BARS_BEYOND: usize = 4;
/// Digit margin: the second-best code's squared error above the best, in modules'
/// worth of the main symbol's mean squared residual per sample.
const MIN_DIGIT_MARGIN: f32 = 6.;
/// Supplement RMS residual allowed relative to the main symbol's.
const MAX_RESIDUAL_RATIO: f32 = 1.6;
/// Absolute RMS residual floor, as a fraction of the fitted bar contrast.
const RESIDUAL_FLOOR: f32 = 0.08;
/// A digit's mean squared error above this multiple of the allowed squared RMS
/// residual ends a gap hypothesis before the remaining digits.
const ABORT_RESIDUAL_RATIO: f32 = 3.;
/// Modules after the supplement that must be free of bars. The specification
/// asks for five; labels often end sooner.
const QUIET_MODULES: f32 = 3.5;
/// Supplement bar contrast allowed relative to the main symbol's, and its inverse.
const MIN_CONTRAST_RATIO: f32 = 0.6;
/// Profiles that must agree on the supplement value.
const MIN_AGREEING_PROFILES: usize = 2;
/// Lines across the symbol height, as fractions from its top edge: near the middle
/// first. The supplement is shorter than the main bars; its digits are printed above.
const LINE_FRACTIONS: [f32; 9] = [0.55, 0.7, 0.4, 0.85, 0.25, 0.62, 0.48, 0.78, 0.32];

/// Read the supplement right of a decoded EAN-13 or UPC-A symbol.
///
/// `pixel(x, y)` returns the gray value of a pixel of the `width` x `height` image.
/// `polygon` is the main symbol's outline in reading order (start guard edge first);
/// `format` and `text` its decoded value (`EAN13`, `UPCA`, `EAN8` or `UPCE`). `profiles` bounds the scan
/// lines across the symbol height. Returns the supplement digits when confirmed.
#[must_use]
pub fn read(
    pixel: impl Fn(usize, usize) -> f32,
    width: usize,
    height: usize,
    polygon: &[[f32; 2]; 4],
    format: &str,
    text: &str,
    profiles: usize,
) -> Option<String> {
    read_with_evidence(pixel, width, height, polygon, format, text, profiles).0
}

/// [`read`], and whether a supplement may be printed although no value was
/// confirmed: bars beside the symbol fit a supplement's first digit on some profile,
/// or the main pattern itself did not fit a profile with bars beside it.
#[must_use]
pub fn read_with_evidence(
    pixel: impl Fn(usize, usize) -> f32,
    width: usize,
    height: usize,
    polygon: &[[f32; 2]; 4],
    format: &str,
    text: &str,
    profiles: usize,
) -> (Option<String>, bool) {
    profile_supplement(pixel, width, height, polygon, format, text, profiles)
        .unwrap_or((None, false))
}

fn profile_supplement(
    pixel: impl Fn(usize, usize) -> f32,
    width: usize,
    height: usize,
    polygon: &[[f32; 2]; 4],
    format: &str,
    text: &str,
    profiles: usize,
) -> Option<(Option<String>, bool)> {
    let (bars, span) = main_pattern(format, text)?;
    if width < 8 || height < 8 {
        return None;
    }
    let mut model = MainModel::new(bars, span);
    let normal = bar_normal(&pixel, width, height, polygon);
    // Outlines start at the start guard; a reversed outline fits the main pattern
    // only when read the other way.
    let mut geometry = Geometry::new(polygon, normal, span)?;
    let mut values: Vec<(String, usize)> = Vec::new();
    let mut previous: Option<MainFit> = None;
    let mut lines = 0;
    let mut promising = false;
    for &fraction in LINE_FRACTIONS.iter().take(profiles) {
        // Two agreeing profiles out of three or more, without disagreement, settle it;
        // two profiles on which no digit fits any code end the search.
        if (lines >= 3
            && matches!(values.as_slice(), [(_, count)] if *count >= MIN_AGREEING_PROFILES))
            || (lines >= 2 && !promising)
        {
            break;
        }
        let (profile, fit) = if let Some(previous) = previous {
            let Some(profile) = geometry.profile(&pixel, width, height, fraction) else {
                continue;
            };
            let Some(fit) = refit_main(&profile, &mut model, previous) else {
                continue;
            };
            (profile, fit)
        } else {
            let reversed = Geometry::new(
                &[polygon[1], polygon[0], polygon[3], polygon[2]],
                normal,
                span,
            )?;
            let mut best: Option<(Profile, MainFit, Geometry)> = None;
            let profiles = [geometry, reversed].map(|g| g.profile(&pixel, width, height, fraction));
            // Plain paper after the symbol, read either way, holds no supplement:
            // most frames end here before any fitting.
            if !profiles.iter().flatten().any(Profile::ink_beyond) {
                return None;
            }
            for (candidate, profile) in [geometry, reversed].into_iter().zip(profiles) {
                let Some(profile) = profile else {
                    continue;
                };
                if let Some(fit) = fit_main(&profile, &mut model)
                    && best
                        .as_ref()
                        .is_none_or(|b| fit.correlation > b.1.correlation)
                {
                    best = Some((profile, fit, candidate));
                }
                // A clear fit in the outline's direction needs no reversed attempt.
                if best
                    .as_ref()
                    .is_some_and(|b| b.1.correlation >= CLEAR_CORRELATION)
                {
                    break;
                }
            }
            // Bars beside a symbol whose pattern does not fit (glare, a curved
            // surface): no value, but no reason to rule a supplement out either.
            let Some((profile, fit, chosen)) = best else {
                return Some((None, true));
            };
            geometry = chosen;
            (profile, fit)
        };
        previous = Some(fit);
        let line = Line::new(&profile, fit, &model.bars);
        // Nothing but paper beside the first profile's symbol: no supplement.
        if values.is_empty() && lines == 0 && !ink_beyond(&line) {
            return None;
        }
        lines += 1;
        let (decoded, hopeful) = decode_supplement(&line);
        promising |= hopeful;
        if let Some(value) = decoded {
            if let Some(entry) = values.iter_mut().find(|(v, _)| *v == value) {
                entry.1 += 1;
            } else {
                values.push((value, 1));
            }
        }
    }
    let value = match values.as_slice() {
        [(value, count)] if *count >= MIN_AGREEING_PROFILES => Some(value.clone()),
        _ => None,
    };
    Some((value, promising))
}

fn digits(text: &str) -> Option<Vec<u8>> {
    text.bytes()
        .map(|b| b.is_ascii_digit().then(|| b - b'0'))
        .collect()
}

/// Bars of a decoded retail symbol in modules from its first bar, and its width in modules.
fn main_pattern(format: &str, text: &str) -> Option<(Vec<(f32, f32)>, f32)> {
    let d = digits(text)?;
    let mut runs = vec![1u8, 1, 1];
    match (format, d.len()) {
        ("EAN13", 13) | ("UPCA", 12) => {
            let (first, rest) = if d.len() == 13 {
                (d[0], &d[1..])
            } else {
                (0, &d[..])
            };
            let parity = PARITY13[usize::from(first)];
            for (i, &digit) in rest[..6].iter().enumerate() {
                runs.extend(code_runs(digit, parity & (1 << (5 - i)) != 0));
            }
            runs.extend([1, 1, 1, 1, 1]);
            for &digit in &rest[6..] {
                runs.extend(code_runs(digit, false));
            }
            runs.extend([1, 1, 1]);
        }
        ("EAN8", 8) => {
            for &digit in &d[..4] {
                runs.extend(code_runs(digit, false));
            }
            runs.extend([1, 1, 1, 1, 1]);
            for &digit in &d[4..] {
                runs.extend(code_runs(digit, false));
            }
            runs.extend([1, 1, 1]);
        }
        ("UPCE", 8) if d[0] <= 1 => {
            let parity = PARITY_UPCE[usize::from(d[7])] ^ if d[0] == 1 { 63 } else { 0 };
            for (i, &digit) in d[1..7].iter().enumerate() {
                runs.extend(code_runs(digit, parity & (1 << (5 - i)) != 0));
            }
            runs.extend([1, 1, 1, 1, 1, 1]);
        }
        _ => return None,
    }
    let span = runs.iter().map(|&r| f32::from(r)).sum();
    let mut bars = Vec::with_capacity(30);
    push_bars(&runs, 0., &mut bars);
    Some((bars, span))
}

/// Run widths of a digit code that starts with a space: L codes as tabulated, G codes reversed.
fn code_runs(digit: u8, even: bool) -> [u8; 4] {
    let mut runs = DIGITS[usize::from(digit)];
    if even {
        runs.reverse();
    }
    runs
}

/// Bars of a run sequence that starts with a bar at `origin`, as module intervals.
fn push_bars(runs: &[u8], origin: f32, bars: &mut Vec<(f32, f32)>) {
    let mut at = origin;
    for (i, &run) in runs.iter().enumerate() {
        let next = at + f32::from(run);
        if i % 2 == 0 && run > 0 {
            bars.push((at, next));
        }
        at = next;
    }
}

/// Supplement bars for `codes` (digit, even parity) from the add-on guard at 0.
fn supplement_bars(codes: &[(u8, bool)]) -> Vec<(f32, f32)> {
    let mut runs = vec![1u8, 1, 2];
    for (i, &(digit, even)) in codes.iter().enumerate() {
        runs.extend(code_runs(digit, even));
        if i + 1 < codes.len() {
            runs.extend([1, 1]);
        }
    }
    let mut bars = Vec::with_capacity(16);
    push_bars(&runs, 0., &mut bars);
    bars
}

/// Cumulative distribution of a unit-width cubic B-spline (support -2..2).
fn spline_cdf(x: f32) -> f32 {
    let a = x.abs();
    let half = if a >= 2. {
        0.5
    } else if a >= 1. {
        let r = 2. - a;
        11. / 24. + (1. - r * r * r * r) / 24.
    } else {
        let a2 = a * a;
        (4. * a - 2. * a2 * a + 0.75 * a2 * a2) / 6.
    };
    if x >= 0. { 0.5 + half } else { 0.5 - half }
}

/// Blurred ink coverage at `t` (modules) of sorted `bars` under blur `sigma` (modules).
fn coverage(bars: &[(f32, f32)], t: f32, sigma: f32) -> f32 {
    // A B-spline of width w has standard deviation w / sqrt(3).
    let w = sigma * 1.732_050_8;
    let reach = 2. * w;
    let mut value = 0.;
    for &(lo, hi) in bars {
        if hi < t - reach {
            continue;
        }
        if lo > t + reach {
            break;
        }
        value += spline_cdf((t - lo) / w) - spline_cdf((t - hi) / w);
    }
    value
}

/// Profile frame: the main symbol's axis through the outline.
#[derive(Clone, Copy)]
struct Geometry {
    origin: [f32; 2],
    along: [f32; 2],
    across: [f32; 2],
    /// Estimated main-symbol length along `along`, in pixels.
    length: f32,
    /// Cross-axis extent, in pixels along `across` from the image origin.
    top: f32,
    bottom: f32,
    /// Main-symbol width in modules.
    span: f32,
}

impl Geometry {
    /// Axes of outline `p`; `normal` (either sign) replaces the outline's long axis
    /// when it lies within about eight degrees of it.
    fn new(p: &[[f32; 2]; 4], normal: Option<[f32; 2]>, span: f32) -> Option<Self> {
        let dx = (p[1][0] - p[0][0]) + (p[2][0] - p[3][0]);
        let dy = (p[1][1] - p[0][1]) + (p[2][1] - p[3][1]);
        let norm = (dx * dx + dy * dy).sqrt();
        if !exceeds(norm, 1.) {
            return None;
        }
        let mut along = [dx / norm, dy / norm];
        if let Some(n) = normal {
            let dot = n[0] * along[0] + n[1] * along[1];
            if dot.abs() >= 0.99 {
                along = if dot > 0. { n } else { [-n[0], -n[1]] };
            }
        }
        let mut across = [-along[1], along[0]];
        let down = [
            (p[3][0] - p[0][0]) + (p[2][0] - p[1][0]),
            (p[3][1] - p[0][1]) + (p[2][1] - p[1][1]),
        ];
        if down[0] * across[0] + down[1] * across[1] < 0. {
            across = [-across[0], -across[1]];
        }
        let project = |q: [f32; 2], axis: [f32; 2]| q[0] * axis[0] + q[1] * axis[1];
        let start = f32::midpoint(project(p[0], along), project(p[3], along));
        let end = f32::midpoint(project(p[1], along), project(p[2], along));
        let top = f32::midpoint(project(p[0], across), project(p[1], across));
        let bottom = f32::midpoint(project(p[2], across), project(p[3], across));
        let length = end - start;
        if !exceeds(length, 20.) || !exceeds(bottom - top, 2.) {
            return None;
        }
        Some(Self {
            origin: [along[0] * start, along[1] * start],
            along,
            across,
            length,
            top,
            bottom,
            span,
        })
    }

    /// Profile of ink darkness along the symbol axis at `fraction` of its height.
    fn profile(
        &self,
        pixel: &impl Fn(usize, usize) -> f32,
        width: usize,
        height: usize,
        fraction: f32,
    ) -> Option<Profile> {
        let module = self.length / self.span;
        let step = module / SAMPLES_PER_MODULE;
        let cross = self.top + (self.bottom - self.top) * fraction;
        // Average three samples along the bars, which run parallel to `across`.
        let spread = (module * 0.75).min((self.bottom - self.top) * 0.05);
        let count = crate::numeric::f32_usize(
            (LEADING_MODULES + self.span + TRAILING_MODULES) * SAMPLES_PER_MODULE,
        );
        // At least two modules beyond the symbol's estimated end.
        let last_needed = (self.span + 2.) * module;
        let mut first = None;
        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            let a = (crate::numeric::usize_f32(i) - LEADING_MODULES * SAMPLES_PER_MODULE) * step;
            let sample: Option<f32> = [cross - spread, cross, cross + spread]
                .into_iter()
                .map(|c| {
                    let x = self.origin[0] + self.along[0] * a + self.across[0] * c;
                    let y = self.origin[1] + self.along[1] * a + self.across[1] * c;
                    bilinear(pixel, width, height, x, y)
                })
                .sum();
            match sample {
                Some(sum) => {
                    first.get_or_insert(a);
                    values.push(255. - sum / 3.);
                }
                // The profile may start after the image border, before the symbol...
                None if first.is_none() && a < 0. => {}
                // ...and ends at the border.
                None => break,
            }
        }
        let first = first?;
        let end = first + crate::numeric::usize_f32(values.len()) * step;
        (end >= last_needed).then_some(Profile {
            values,
            first,
            step,
            module,
            span: self.span,
        })
    }
}

/// Unit gradient direction of the bars inside `outline`, from the structure tensor
/// of central differences on a grid; `None` without a clear dominant direction.
fn bar_normal(
    pixel: &impl Fn(usize, usize) -> f32,
    width: usize,
    height: usize,
    outline: &[[f32; 2]; 4],
) -> Option<[f32; 2]> {
    let (mut xx, mut yy, mut xy) = (0f32, 0f32, 0f32);
    for j in 0..8u8 {
        let down = (f32::from(j) + 0.5) / 8.;
        for i in 0..64u8 {
            let along = (f32::from(i) + 0.5) / 64.;
            let x = (outline[0][0] * (1. - along) + outline[1][0] * along) * (1. - down)
                + (outline[3][0] * (1. - along) + outline[2][0] * along) * down;
            let y = (outline[0][1] * (1. - along) + outline[1][1] * along) * (1. - down)
                + (outline[3][1] * (1. - along) + outline[2][1] * along) * down;
            if !reaches(x, 1.) || !reaches(y, 1.) {
                continue;
            }
            let (xi, yi) = (
                crate::numeric::f32_usize(x + 0.5),
                crate::numeric::f32_usize(y + 0.5),
            );
            if xi + 1 >= width || yi + 1 >= height {
                continue;
            }
            let gx = pixel(xi + 1, yi) - pixel(xi - 1, yi);
            let gy = pixel(xi, yi + 1) - pixel(xi, yi - 1);
            xx += gx * gx;
            yy += gy * gy;
            xy += gx * gy;
        }
    }
    let (double_cos, double_sin) = (xx - yy, 2. * xy);
    let anisotropy = (double_cos * double_cos + double_sin * double_sin).sqrt();
    if !reaches(anisotropy, f32::midpoint(xx, yy)) {
        return None;
    }
    // Half the double angle without trigonometry.
    let (cos2, sin2) = (double_cos / anisotropy, double_sin / anisotropy);
    let cos = f32::midpoint(1., cos2).max(0.).sqrt();
    let sin = f32::midpoint(1., -cos2).max(0.).sqrt();
    Some([cos, if sin2 < 0. { -sin } else { sin }])
}

fn bilinear(
    pixel: &impl Fn(usize, usize) -> f32,
    width: usize,
    height: usize,
    x: f32,
    y: f32,
) -> Option<f32> {
    if !reaches(x, 0.) || !reaches(y, 0.) {
        return None;
    }
    let x0 = crate::numeric::f32_usize(x);
    let y0 = crate::numeric::f32_usize(y);
    if x0 + 1 >= width || y0 + 1 >= height {
        return None;
    }
    let fx = x - crate::numeric::usize_f32(x0);
    let fy = y - crate::numeric::usize_f32(y0);
    let top = pixel(x0, y0) * (1. - fx) + pixel(x0 + 1, y0) * fx;
    let bottom = pixel(x0, y0 + 1) * (1. - fx) + pixel(x0 + 1, y0 + 1) * fx;
    Some(top * (1. - fy) + bottom * fy)
}

/// Darkness samples (255 - gray) along the axis; sample `i` lies at `first + i * step` pixels.
struct Profile {
    values: Vec<f32>,
    first: f32,
    step: f32,
    /// Module width estimated from the outline, in pixels.
    module: f32,
    /// Main-symbol width in modules.
    span: f32,
}

impl Profile {
    /// Sample index range covering pixel positions `lo..hi`.
    fn range(&self, lo: f32, hi: f32) -> std::ops::Range<usize> {
        let a = ((lo - self.first) / self.step).ceil().max(0.);
        let b = ((hi - self.first) / self.step).floor() + 1.;
        let len = self.values.len();
        crate::numeric::f32_usize(a).min(len)..crate::numeric::f32_usize(b.max(0.)).min(len)
    }

    fn position(&self, i: usize) -> f32 {
        self.first + crate::numeric::usize_f32(i) * self.step
    }

    /// Whether the darkness range where a supplement would follow the outline's
    /// symbol reaches a quarter of the range across the symbol itself.
    fn ink_beyond(&self) -> bool {
        let spread = |range: std::ops::Range<usize>| {
            let values = &self.values[range];
            let lo = values.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            hi - lo
        };
        let m = self.module;
        let symbol = spread(self.range(0., self.span * m));
        let beyond = self.range(
            (self.span + GAP_RANGE.0) * m,
            (self.span + GAP_RANGE.1 + 47.) * m,
        );
        if beyond.is_empty() || spread(beyond.clone()) < symbol * 0.25 {
            return false;
        }
        // Bars, not a few glyph strokes: darkness peaks that rise above the lightest
        // sample within two modules on both sides. Every supplement has six or more
        // bars; blur can merge the closest ones.
        let values = &self.values[beyond];
        let prominence = symbol * 0.03;
        let reach = crate::numeric::f32_usize(2. * SAMPLES_PER_MODULE);
        let mut bars = 0;
        let mut last_peak: Option<usize> = None;
        for i in 1..values.len().saturating_sub(1) {
            let v = values[i];
            if v < values[i - 1] || v < values[i + 1] {
                continue;
            }
            let before = values[i.saturating_sub(reach)..i]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            let after = values[i + 1..(i + 1 + reach).min(values.len())]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            if v - before >= prominence
                && v - after >= prominence
                && last_peak.is_none_or(|p| i - p >= crate::numeric::f32_usize(SAMPLES_PER_MODULE))
            {
                bars += 1;
                last_peak = Some(i);
            }
        }
        bars >= MIN_BARS_BEYOND
    }
}

/// Pixel position along a profile of the main symbol's module coordinate `t`
/// (0 at its first bar, `span` after its last). Under perspective the module width
/// changes linearly with `t`: by the fraction `curve` over the symbol.
#[derive(Clone, Copy)]
struct Mapping {
    start: f32,
    /// Module width at the first bar, in pixels.
    module: f32,
    curve: f32,
    span: f32,
}

impl Mapping {
    fn quadratic(self) -> f32 {
        self.curve * self.module / (2. * self.span)
    }

    fn pixel(self, t: f32) -> f32 {
        self.start + t * (self.module + self.quadratic() * t)
    }

    /// Module coordinate of pixel position `x` (two Newton steps; exact without curve).
    fn modules(self, x: f32) -> f32 {
        let q = self.quadratic();
        let mut t = (x - self.start) / self.module;
        if q != 0. {
            for _ in 0..2 {
                t -= (self.pixel(t) - x) / (self.module + 2. * q * t);
            }
        }
        t
    }

    /// The same symbol length with a different `curve`.
    fn with_curve(self, curve: f32) -> Self {
        let length = self.pixel(self.span) - self.start;
        Self {
            module: length / (self.span * (1. + curve * 0.5)),
            curve,
            ..self
        }
    }
}

/// Blurred main patterns, tabulated per blur width on first use.
struct MainModel {
    bars: Vec<(f32, f32)>,
    span: f32,
    tables: [Option<Vec<f32>>; SIGMAS.len()],
}

/// Table resolution in samples per module, and its first position in modules.
const TABLE_RESOLUTION: f32 = 16.;
const TABLE_FIRST: f32 = -4.;

impl MainModel {
    fn new(bars: Vec<(f32, f32)>, span: f32) -> Self {
        Self {
            bars,
            span,
            tables: Default::default(),
        }
    }

    fn table(&mut self, sigma: usize) -> &[f32] {
        let bars = &self.bars;
        let len = crate::numeric::f32_usize((self.span - 2. * TABLE_FIRST) * TABLE_RESOLUTION) + 1;
        self.tables[sigma].get_or_insert_with(|| {
            // Positions increase: skip the bars left behind once.
            let reach = 2. * SIGMAS[sigma] * 1.732_050_8;
            let mut first = 0;
            (0..len)
                .map(|k| {
                    let t = TABLE_FIRST + crate::numeric::usize_f32(k) / TABLE_RESOLUTION;
                    while first < bars.len() && bars[first].1 < t - reach {
                        first += 1;
                    }
                    coverage(&bars[first..], t, SIGMAS[sigma])
                })
                .collect()
        })
    }
}

/// Linearly interpolated table value at `t` modules; no ink outside the table.
fn lookup(table: &[f32], t: f32) -> f32 {
    let u = (t - TABLE_FIRST) * TABLE_RESOLUTION;
    if !reaches(u, 0.) {
        return 0.;
    }
    let i = crate::numeric::f32_usize(u);
    if i + 1 >= table.len() {
        return 0.;
    }
    let f = u - crate::numeric::usize_f32(i);
    table[i] * (1. - f) + table[i + 1] * f
}

/// The main symbol on one profile.
#[derive(Clone, Copy)]
struct MainFit {
    map: Mapping,
    /// Index into [`SIGMAS`].
    sigma: usize,
    /// Darkness of white (`level`) and the bar contrast (`contrast`).
    level: f32,
    contrast: f32,
    /// Main-symbol mean squared residual per sample.
    noise: f32,
    /// Correlation of the profile with the blurred main pattern.
    correlation: f32,
}

/// Fit of the main pattern under `map` and blur `sigma`; `None` below the correlation floor.
/// `stride` > 1 uses every `stride`th sample (coarse searches).
fn main_score(
    profile: &Profile,
    model: &mut MainModel,
    map: Mapping,
    sigma: usize,
    floor: f32,
    stride: usize,
) -> Option<MainFit> {
    let table = model.table(sigma);
    let range = profile.range(map.pixel(-1.), map.pixel(map.span + 1.));
    if crate::numeric::usize_f32(range.len()) < map.span * 2. {
        return None;
    }
    let n = crate::numeric::usize_f32(range.len().div_ceil(stride));
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0f32, 0f32, 0f32, 0f32, 0f32);
    for i in range.step_by(stride) {
        let x = lookup(table, map.modules(profile.position(i)));
        let y = profile.values[i];
        sx += x;
        sy += y;
        sxx += x * x;
        syy += y * y;
        sxy += x * y;
    }
    let vx = sxx - sx * sx / n;
    let vy = syy - sy * sy / n;
    let cxy = sxy - sx * sy / n;
    if !exceeds(vx, 0.) || !exceeds(vy, 0.) || !exceeds(cxy, 0.) {
        return None;
    }
    let correlation = cxy / (vx * vy).sqrt();
    if !reaches(correlation, floor) {
        return None;
    }
    let contrast = cxy / vx;
    Some(MainFit {
        map,
        sigma,
        level: (sy - contrast * sx) / n,
        contrast,
        noise: ((vy - cxy * cxy / vx) / n).max(0.),
        correlation,
    })
}

/// Best fit over a grid of offsets (in `step` modules) and scales around `center`.
#[expect(
    clippy::too_many_arguments,
    reason = "Each search stage states its grid, blur, correlation floor and stride at the call."
)]
fn search(
    profile: &Profile,
    model: &mut MainModel,
    center: Mapping,
    sigma: usize,
    offsets: (i8, f32),
    scales: (i8, f32),
    floor: f32,
    stride: usize,
) -> Option<MainFit> {
    let mut best: Option<MainFit> = None;
    for s in -scales.0..=scales.0 {
        let module = center.module * (1. + f32::from(s) * scales.1);
        for o in -offsets.0..=offsets.0 {
            let map = Mapping {
                start: center.start + f32::from(o) * offsets.1 * center.module,
                module,
                ..center
            };
            if let Some(fit) = main_score(profile, model, map, sigma, floor, stride)
                && best.is_none_or(|b| fit.correlation > b.correlation)
            {
                best = Some(fit);
            }
        }
    }
    best
}

/// Best curve within `steps` of `step` around `fit`, the symbol length kept, with a
/// small offset search. The supplement extends the curve beyond the symbol, where
/// its error grows with the square of the distance.
fn search_curve(
    profile: &Profile,
    model: &mut MainModel,
    fit: MainFit,
    steps: i8,
    step: f32,
) -> MainFit {
    let mut best = fit;
    for c in -steps..=steps {
        let map = fit.map.with_curve(fit.map.curve + f32::from(c) * step);
        if let Some(found) = search(
            profile,
            model,
            map,
            fit.sigma,
            (2, 0.0625),
            (0, 0.),
            MIN_MAIN_CORRELATION,
            1,
        ) && found.correlation > best.correlation
        {
            best = found;
        }
    }
    best
}

/// Lowest-residual blur width at the fitted position.
fn best_sigma(profile: &Profile, model: &mut MainModel, fit: MainFit) -> MainFit {
    let mut best = fit;
    for sigma in 0..SIGMAS.len() {
        if let Some(found) = main_score(profile, model, fit.map, sigma, MIN_MAIN_CORRELATION, 1)
            && found.noise < best.noise
        {
            best = found;
        }
    }
    best
}

fn fit_main(profile: &Profile, model: &mut MainModel) -> Option<MainFit> {
    let estimate = Mapping {
        start: 0.,
        module: profile.module,
        curve: 0.,
        span: profile.span,
    };
    // Coarse: half-module offsets near the outline first, then within eight modules.
    let coarse = search(
        profile,
        model,
        estimate,
        COARSE_SIGMA,
        (6, 0.5),
        (2, 0.02),
        MIN_MAIN_CORRELATION,
        2,
    )
    .or_else(|| {
        search(
            profile,
            model,
            estimate,
            COARSE_SIGMA,
            (16, 0.5),
            (5, 0.02),
            MIN_MAIN_CORRELATION * 0.6,
            2,
        )
    })?;
    // Fine: eighth-module offsets and 0.5% scale steps, then perspective and blur.
    let fine = search(
        profile,
        model,
        coarse.map,
        COARSE_SIGMA,
        (4, 0.125),
        (4, 0.005),
        MIN_MAIN_CORRELATION,
        1,
    )?;
    let curved = search_curve(profile, model, fine, 4, 0.02);
    let blurred = best_sigma(profile, model, curved);
    let settled = search(
        profile,
        model,
        blurred.map,
        blurred.sigma,
        (2, 0.0625),
        (2, 0.0025),
        MIN_MAIN_CORRELATION,
        1,
    )?;
    // Perspective and the fitted length trade off: finer rounds under the blur.
    let mut settled = settled;
    for step in [0.01, 0.005] {
        let curved = search_curve(profile, model, settled, 2, step);
        settled = search(
            profile,
            model,
            curved.map,
            curved.sigma,
            (2, 0.0625),
            (2, 0.0025),
            MIN_MAIN_CORRELATION,
            1,
        )?;
    }
    Some(best_sigma(profile, model, settled))
}

/// Fit another profile of the same symbol near the previous fit: the offset changes
/// with tilt, the module width and its change with perspective.
fn refit_main(profile: &Profile, model: &mut MainModel, previous: MainFit) -> Option<MainFit> {
    let coarse = search(
        profile,
        model,
        previous.map,
        previous.sigma,
        (6, 0.25),
        (2, 0.01),
        MIN_MAIN_CORRELATION,
        2,
    )?;
    let fine = search(
        profile,
        model,
        coarse.map,
        previous.sigma,
        (2, 0.0625),
        (2, 0.0025),
        MIN_MAIN_CORRELATION,
        1,
    )?;
    Some(search_curve(profile, model, fine, 2, 0.005))
}

/// A profile with its main-symbol fit and the module coordinate of every sample.
struct Line<'a> {
    profile: &'a Profile,
    fit: MainFit,
    modules: Vec<f32>,
    /// The main symbol's last two bars, in its module coordinates.
    end: [(f32, f32); 2],
}

impl<'a> Line<'a> {
    fn new(profile: &'a Profile, fit: MainFit, bars: &[(f32, f32)]) -> Self {
        let modules = (0..profile.values.len())
            .map(|i| fit.map.modules(profile.position(i)))
            .collect();
        let end = [bars[bars.len() - 2], bars[bars.len() - 1]];
        Self {
            profile,
            fit,
            modules,
            end,
        }
    }

    /// Samples between main-symbol module coordinates `lo` and `hi`.
    fn range(&self, lo: f32, hi: f32) -> std::ops::Range<usize> {
        self.modules.partition_point(|&t| t < lo)..self.modules.partition_point(|&t| t <= hi)
    }
}

/// Supplement placement in main-symbol module coordinates: its guard starts at
/// `origin` and one supplement module spans `scale` main modules.
#[derive(Clone, Copy)]
struct Placement {
    origin: f32,
    scale: f32,
}

impl Placement {
    fn to_main(self, j: f32) -> f32 {
        self.origin + j * self.scale
    }

    fn to_supplement(self, t: f32) -> f32 {
        (t - self.origin) / self.scale
    }
}

/// Squared model error of `bars` (supplement modules) between supplement positions `lo` and `hi`.
fn segment_error(
    line: &Line<'_>,
    levels: (f32, f32),
    place: Placement,
    bars: &[(f32, f32)],
    lo: f32,
    hi: f32,
) -> (f32, usize) {
    let sigma = SIGMAS[line.fit.sigma] / place.scale;
    let range = line.range(place.to_main(lo), place.to_main(hi));
    let count = range.len();
    let mut error = 0.;
    for i in range {
        let model =
            levels.0 + levels.1 * coverage(bars, place.to_supplement(line.modules[i]), sigma);
        let d = line.profile.values[i] - model;
        error += d * d;
    }
    (error, count)
}

/// Least-squares levels (white darkness, bar contrast) of `bars` between `lo` and `hi`.
fn local_levels(
    line: &Line<'_>,
    place: Placement,
    bars: &[(f32, f32)],
    lo: f32,
    hi: f32,
) -> Option<(f32, f32)> {
    let sigma = SIGMAS[line.fit.sigma] / place.scale;
    let range = line.range(place.to_main(lo), place.to_main(hi));
    let n = crate::numeric::usize_f32(range.len());
    let (mut sx, mut sy, mut sxx, mut sxy) = (0f32, 0f32, 0f32, 0f32);
    for i in range {
        let x = coverage(bars, place.to_supplement(line.modules[i]), sigma);
        let y = line.profile.values[i];
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let vx = sxx - sx * sx / n;
    let cxy = sxy - sx * sy / n;
    if !exceeds(vx, 0.) {
        return None;
    }
    let contrast = cxy / vx;
    Some(((sy - contrast * sx) / n, contrast))
}

/// Whether the profile beside the main symbol, where a supplement would lie, holds
/// any ink: its darkness range exceeds a quarter of the main bar contrast.
fn ink_beyond(line: &Line<'_>) -> bool {
    let span = line.fit.map.span;
    let range = line.range(span + GAP_RANGE.0 - 1., span + GAP_RANGE.1 + 47.);
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in &line.profile.values[range] {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    hi - lo >= line.fit.contrast * 0.25
}

/// The main symbol's last two bars in supplement coordinates.
fn main_end_bars(line: &Line<'_>, place: Placement) -> [(f32, f32); 2] {
    line.end
        .map(|(lo, hi)| (place.to_supplement(lo), place.to_supplement(hi)))
}

/// The supplement value read on one profile, if any, and whether any hypothesis got
/// past its first digit.
fn decode_supplement(line: &Line<'_>) -> (Option<String>, bool) {
    let fit = line.fit;
    let levels = (fit.level, fit.contrast);
    // Gap hypotheses ranked by the fit of the main end guard, the white gap, the
    // supplement guard and the first digit's leading space.
    let mut hypotheses: Vec<(f32, f32)> = Vec::new();
    let mut gap = GAP_RANGE.0;
    while gap <= GAP_RANGE.1 {
        let place = Placement {
            origin: fit.map.span + gap,
            scale: 1.,
        };
        let mut bars = main_end_bars(line, place).to_vec();
        push_bars(&[1, 1, 2], 0., &mut bars);
        let (error, count) = segment_error(line, levels, place, &bars, -gap - 1., 5.);
        if count > 0 {
            hypotheses.push((error / crate::numeric::usize_f32(count), place.origin));
        }
        gap += 0.25;
    }
    hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut tried: Vec<f32> = Vec::new();
    let mut best: Option<(String, f32)> = None;
    let mut promising = false;
    for &(_, origin) in &hypotheses {
        if tried.len() >= GAP_HYPOTHESES {
            break;
        }
        if tried.iter().any(|&o| (o - origin).abs() < 0.75) {
            continue;
        }
        tried.push(origin);
        // Other module widths are tried only when the extended geometry's fails.
        // A first digit that fits no code at the extended geometry ends the gap
        // hypothesis: it lies next to the guard, where the width matters least.
        // Other widths may fail there and still fit.
        'scales: for (index, scale) in SUPPLEMENT_SCALES.into_iter().enumerate() {
            for count in [5usize, 2] {
                let (value, rms) = match decode_digits(line, Placement { origin, scale }, count) {
                    Decoded::Accepted(value, rms) => (value, rms),
                    Decoded::Rejected => {
                        promising = true;
                        continue;
                    }
                    Decoded::NoFirstDigit if index == 0 => break 'scales,
                    Decoded::NoFirstDigit => continue 'scales,
                };
                promising = true;
                match &best {
                    // Different values from one profile: ambiguous.
                    Some((other, _)) if *other != value => return (None, true),
                    Some((_, other_rms)) if *other_rms <= rms => {}
                    _ => best = Some((value, rms)),
                }
                break 'scales;
            }
        }
    }
    (best.map(|(value, _)| value), promising)
}

/// Bars that reach into digit slot `i`'s window: the guard or the previous digit and
/// its separator, the digit itself, and the following separator and digit.
/// [`slot_bars`] without the digit in slot `i` itself.
fn context_bars(codes: &[(u8, bool)], i: usize, bars: &mut Vec<(f32, f32)>) {
    slot_bars(codes, i, bars);
    let slot = 4. + 9. * crate::numeric::usize_f32(i);
    bars.retain(|&(lo, hi)| lo < slot || hi > slot + 7.);
}

fn slot_bars(codes: &[(u8, bool)], i: usize, bars: &mut Vec<(f32, f32)>) {
    bars.clear();
    let slot = 4. + 9. * crate::numeric::usize_f32(i);
    if i == 0 {
        push_bars(&[1, 1, 2], 0., bars);
    } else {
        let (digit, even) = codes[i - 1];
        let mut runs = [0u8; 6];
        runs[1..5].copy_from_slice(&code_runs(digit, even));
        runs[5] = 1;
        // Runs start with a bar; a zero-width one keeps the space first.
        push_bars(&runs, slot - 9., bars);
        bars.push((slot - 1., slot));
    }
    let (digit, even) = codes[i];
    let mut runs = [0u8; 5];
    runs[1..].copy_from_slice(&code_runs(digit, even));
    push_bars(&runs, slot, bars);
    if i + 1 < codes.len() {
        bars.push((slot + 8., slot + 9.));
        let (digit, even) = codes[i + 1];
        let mut runs = [0u8; 5];
        runs[1..].copy_from_slice(&code_runs(digit, even));
        push_bars(&runs, slot + 9., bars);
    }
}

/// Supplement digit codes: (digit, even parity).
type Codes = Vec<(u8, bool)>;

/// Choose every digit code by its fit, two sweeps; `Err(i)` when digit `i` fits no code.
fn choose_codes(
    line: &Line<'_>,
    levels: (f32, f32),
    place: Placement,
    count: usize,
    abort: f32,
) -> Result<(Codes, Vec<f32>), usize> {
    let mut codes: Vec<(u8, bool)> = vec![(0, false); count];
    let mut known = 0;
    let mut margins = vec![0f32; count];
    let mut bars = Vec::with_capacity(12);
    // Ink levels follow the light across the supplement: each digit starts from its
    // predecessor's and is refitted once chosen.
    let mut slot_levels = vec![levels; count];
    let sigma = SIGMAS[line.fit.sigma] / place.scale;
    let mut window: Vec<(f32, f32, f32)> = Vec::with_capacity(64);
    // The second sweep re-chooses each digit with both neighbours known.
    for sweep in 0..2 {
        for i in 0..count {
            let slot = 4. + 9. * crate::numeric::usize_f32(i);
            if sweep == 0 && i > 0 {
                slot_levels[i] = slot_levels[i - 1];
            }
            let levels = slot_levels[i];
            let mut best = (f32::INFINITY, (0u8, false));
            let mut second = f32::INFINITY;
            // The neighbours' ink is the same for every code: compute it once. In the
            // first sweep the next digit is unknown and modelled white.
            let known_codes = &codes[..known.max(i + 1)];
            context_bars(known_codes, i, &mut bars);
            window.clear();
            window.extend(
                line.range(place.to_main(slot - 1.), place.to_main(slot + 8.))
                    .map(|k| {
                        let j = place.to_supplement(line.modules[k]);
                        (j, coverage(&bars, j, sigma), line.profile.values[k])
                    }),
            );
            let samples = window.len();
            for digit in 0..10u8 {
                for even in [false, true] {
                    let mut code = Vec::with_capacity(2);
                    let mut runs = [0u8; 5];
                    runs[1..].copy_from_slice(&code_runs(digit, even));
                    push_bars(&runs, slot, &mut code);
                    let error: f32 = window
                        .iter()
                        .map(|&(j, ink, v)| {
                            let d = v - (levels.0 + levels.1 * (ink + coverage(&code, j, sigma)));
                            d * d
                        })
                        .sum();
                    if error < best.0 {
                        second = second.min(best.0);
                        best = (error, (digit, even));
                    } else {
                        second = second.min(error);
                    }
                }
            }
            if samples == 0 || (sweep == 0 && best.0 > abort * crate::numeric::usize_f32(samples)) {
                return Err(i);
            }
            codes[i] = best.1;
            margins[i] = second - best.0;
            known = known.max(i + 1);
            if sweep == 0 {
                slot_bars(&codes[..=i], i, &mut bars);
                if let Some(refit) = local_levels(line, place, &bars, slot - 5., slot + 8.)
                    && plausible_contrast(refit.1, slot_levels[0].1)
                {
                    slot_levels[i] = refit;
                }
            }
        }
    }
    Ok((codes, margins))
}

fn parity_valid(codes: &[(u8, bool)]) -> bool {
    let parity = codes
        .iter()
        .fold(0u8, |p, &(_, even)| (p << 1) | u8::from(even));
    let values: Vec<u32> = codes.iter().map(|&(d, _)| u32::from(d)).collect();
    if values.len() == 2 {
        u32::from(parity) == (values[0] * 10 + values[1]) % 4
    } else {
        let check = (3 * (values[0] + values[2] + values[4]) + 9 * (values[1] + values[3])) % 10;
        PARITY5[check as usize] == parity
    }
}

/// Model of the gap, the supplement `codes` and its quiet zone, with the main end guard.
fn full_bars(line: &Line<'_>, place: Placement, codes: &[(u8, bool)]) -> Vec<(f32, f32)> {
    let mut bars = main_end_bars(line, place).to_vec();
    bars.extend(supplement_bars(codes));
    bars
}

/// Placement with the lowest whole-window error near `place`: the origin first,
/// then the module width.
fn refine_placement(
    line: &Line<'_>,
    levels: (f32, f32),
    place: Placement,
    codes: &[(u8, bool)],
    hi: f32,
) -> Placement {
    let error = |candidate: Placement| {
        let bars = full_bars(line, candidate, codes);
        let (error, n) = segment_error(line, levels, candidate, &bars, 0., hi);
        if n == 0 {
            f32::INFINITY
        } else {
            error / crate::numeric::usize_f32(n)
        }
    };
    let mut best = (error(place), place);
    for step in 1..=2 {
        for k in -3i8..=3 {
            let center = best.1;
            let candidate = if step == 1 {
                Placement {
                    origin: center.origin + f32::from(k) * 0.125,
                    ..center
                }
            } else {
                Placement {
                    scale: center.scale * (1. + f32::from(k) * 0.01),
                    ..center
                }
            };
            let e = error(candidate);
            if e < best.0 {
                best = (e, candidate);
            }
        }
    }
    best.1
}

/// RMS residuals of the gap and supplement (`lo` to one module past `total`) and of
/// the quiet zone after it (to `end`), under paper and ink levels that may drift
/// linearly across the supplement. A quiet zone may end in one blurred step to
/// another uniform background (a label edge) 1.5 modules or more after the last bar.
fn final_residuals(
    line: &Line<'_>,
    place: Placement,
    bars: &[(f32, f32)],
    lo: f32,
    total: f32,
    end: f32,
) -> Option<(f32, f32)> {
    let sigma = SIGMAS[line.fit.sigma] / place.scale;
    let range = line.range(place.to_main(lo), place.to_main(end));
    let samples: Vec<(f32, f32, f32)> = range
        .map(|i| {
            let j = place.to_supplement(line.modules[i]);
            (j, coverage(bars, j, sigma), line.profile.values[i])
        })
        .collect();
    let body_end = total + 1.;
    let body: Vec<&(f32, f32, f32)> = samples.iter().filter(|s| s.0 <= body_end).collect();
    if body.len() < 8 {
        return None;
    }
    let mid = f32::midpoint(lo, body_end);
    // Least squares of v = a + slope * (j - mid) + contrast * ink.
    let (mut normal, mut rhs) = ([[0f32; 3]; 3], [0f32; 3]);
    for &&(position, ink, value) in &body {
        let terms = [1., position - mid, ink];
        for (row, &first) in normal.iter_mut().zip(&terms) {
            for (cell, &second) in row.iter_mut().zip(&terms) {
                *cell += first * second;
            }
        }
        for (target, &term) in rhs.iter_mut().zip(&terms) {
            *target += term * value;
        }
    }
    // Cramer's rule.
    let det = |matrix: [[f32; 3]; 3]| {
        matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
            - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
            + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
    };
    let determinant = det(normal);
    if !exceeds(determinant.abs(), 0.) {
        return None;
    }
    let solve = |column: usize| {
        let mut replaced = normal;
        for (row, &value) in replaced.iter_mut().zip(&rhs) {
            row[column] = value;
        }
        det(replaced) / determinant
    };
    let (level, slope, contrast) = (solve(0), solve(1), solve(2));
    if !plausible_contrast(contrast, line.fit.contrast) {
        return None;
    }
    let residual = |&(position, ink, value): &(f32, f32, f32)| {
        value - (level + slope * (position - mid) + contrast * ink)
    };
    let body_error: f32 = body.iter().map(|s| residual(s).powi(2)).sum();
    let rms = (body_error / crate::numeric::usize_f32(body.len())).sqrt();
    let quiet: Vec<(f32, f32)> = samples
        .iter()
        .filter(|s| s.0 > total + 0.5)
        .map(|s| (s.0, residual(s)))
        .collect();
    if quiet.is_empty() {
        return None;
    }
    let mean_square = |values: &mut dyn Iterator<Item = f32>| {
        let (sum, n) = values.fold((0f32, 0usize), |(sum, n), v| (sum + v * v, n + 1));
        (sum / crate::numeric::usize_f32(n.max(1))).sqrt()
    };
    let mut quiet_rms = mean_square(&mut quiet.iter().map(|q| q.1));
    let w = sigma * 1.732_050_8;
    let mut edge = total + 1.5;
    while edge <= end - 0.5 {
        // Least-squares height of a blurred step at `edge`.
        let (rg, gg) = quiet.iter().fold((0f32, 0f32), |(rg, gg), &(j, v)| {
            let g = spline_cdf((j - edge) / w);
            (rg + v * g, gg + g * g)
        });
        if gg > 0. {
            let step = rg / gg;
            let stepped = mean_square(
                &mut quiet
                    .iter()
                    .map(|&(j, v)| v - step * spline_cdf((j - edge) / w)),
            );
            quiet_rms = quiet_rms.min(stepped);
        }
        edge += 0.25;
    }
    Some((rms, quiet_rms))
}

/// Whether a supplement's bar contrast is within [`MIN_CONTRAST_RATIO`] of the main symbol's.
fn plausible_contrast(contrast: f32, main: f32) -> bool {
    contrast >= main * MIN_CONTRAST_RATIO && contrast <= main / MIN_CONTRAST_RATIO
}

/// Outcome of decoding one supplement hypothesis.
enum Decoded {
    Accepted(String, f32),
    Rejected,
    /// The first digit fit no code: the hypothesis is not worth other widths.
    NoFirstDigit,
}

fn decode_digits(line: &Line<'_>, place: Placement, count: usize) -> Decoded {
    match decode_inner(line, place, count) {
        Ok(Some((value, rms))) => Decoded::Accepted(value, rms),
        Ok(None) => Decoded::Rejected,
        Err(()) => Decoded::NoFirstDigit,
    }
}

fn decode_inner(
    line: &Line<'_>,
    place: Placement,
    count: usize,
) -> Result<Option<(String, f32)>, ()> {
    let fit = line.fit;
    let total = 4. + 9. * crate::numeric::usize_f32(count) - 2.;
    let end = total + QUIET_MODULES;
    // The supplement and its quiet zone must lie inside the profile.
    if line.modules.last().is_none_or(|&t| t < place.to_main(end)) {
        return Ok(None);
    }
    let noise = fit.noise.max(1.);
    let allowed = (noise.sqrt() * MAX_RESIDUAL_RATIO).max(fit.contrast * RESIDUAL_FLOOR);
    let abort = ABORT_RESIDUAL_RATIO * allowed * allowed;
    // Levels of the gap and the supplement guard: paper and ink beside the main
    // symbol differ from its averages under uneven light.
    let mut guard = main_end_bars(line, place).to_vec();
    push_bars(&[1, 1, 2], 0., &mut guard);
    let main_levels = local_levels(
        line,
        place,
        &guard,
        place.to_supplement(fit.map.span + 0.5),
        4.5,
    )
    .filter(|levels| plausible_contrast(levels.1, fit.contrast))
    .unwrap_or((fit.level, fit.contrast));
    let first = choose_codes(line, main_levels, place, count, abort);
    let codes = match first {
        Ok((codes, _)) => codes,
        Err(0) => return Err(()),
        Err(_) => return Ok(None),
    };
    // Align the supplement on its own bars, then choose again under its own levels:
    // light falls off across the label.
    let place = refine_placement(line, main_levels, place, &codes, end);
    let Some(levels) = local_levels(
        line,
        place,
        &full_bars(line, place, &codes),
        place.to_supplement(fit.map.span + 0.5),
        end,
    ) else {
        return Ok(None);
    };
    if !plausible_contrast(levels.1, fit.contrast) {
        return Ok(None);
    }
    let Ok((codes, margins)) = choose_codes(line, levels, place, count, abort) else {
        return Ok(None);
    };
    if !parity_valid(&codes) {
        return Ok(None);
    }
    let samples_per_module = place.scale * fit.map.module / line.profile.step;
    if margins
        .iter()
        .any(|&m| m < MIN_DIGIT_MARGIN * noise * samples_per_module)
    {
        return Ok(None);
    }
    // The gap from the main symbol's end, the supplement and its quiet zone: no
    // bars other than the modelled ones, so stray bars before or after are rejected.
    let bars = full_bars(line, place, &codes);
    let Some((rms, quiet_rms)) = final_residuals(
        line,
        place,
        &bars,
        place.to_supplement(fit.map.span + 0.5),
        total,
        end,
    ) else {
        return Ok(None);
    };
    if rms > allowed || quiet_rms > allowed {
        return Ok(None);
    }
    // A two-digit read must not be the start of a five-digit supplement cut by the
    // image border or blur: no separator bar may follow its second digit.
    if count == 2 {
        let mut separated = bars.clone();
        separated.push((total + 1., total + 2.));
        let (white, n) = segment_error(line, levels, place, &bars, total + 0.25, total + 2.75);
        let (separator, _) =
            segment_error(line, levels, place, &separated, total + 0.25, total + 2.75);
        if n == 0 || separator < white {
            return Ok(None);
        }
    }
    Ok(Some((
        codes.iter().map(|&(d, _)| char::from(b'0' + d)).collect(),
        rms,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generated label: a retail symbol, optional supplements and clutter, blurred
    /// with the reader's own kernel and sampled one pixel per column.
    struct Scene<'a> {
        format: &'a str,
        text: &'a str,
        /// Supplement printed on every row, and a different one on the lower half.
        supplement: Option<&'a str>,
        lower: Option<&'a str>,
        module: f32,
        gap: f32,
        sigma: f32,
        /// Relative module growth across the main symbol (perspective).
        curve: f32,
        /// Modules after the supplement's end where the label gives way to darker paper.
        edge: Option<f32>,
        /// Extra bars, in main-symbol modules.
        stray: &'a [(f32, f32)],
        /// The image ends this many modules after the main symbol's start.
        cut: Option<f32>,
    }

    const SCENE: Scene<'static> = Scene {
        format: "EAN13",
        text: "9781234567897",
        supplement: None,
        lower: None,
        module: 2.,
        gap: 9.,
        sigma: 0.4,
        curve: 0.,
        edge: None,
        stray: &[],
        cut: None,
    };

    fn supplement_codes(text: &str) -> Vec<(u8, bool)> {
        let values: Vec<u8> = text.bytes().map(|b| b - b'0').collect();
        let parity = if values.len() == 2 {
            (values[0] * 10 + values[1]) % 4
        } else {
            let v: Vec<u32> = values.iter().map(|&d| u32::from(d)).collect();
            PARITY5[((3 * (v[0] + v[2] + v[4]) + 9 * (v[1] + v[3])) % 10) as usize]
        };
        let n = values.len();
        values
            .iter()
            .enumerate()
            .map(|(i, &d)| (d, parity & (1 << (n - 1 - i)) != 0))
            .collect()
    }

    impl Scene<'_> {
        /// Gray pixels, width, height and the main symbol's outline.
        fn render(&self) -> (Vec<u8>, usize, usize, [[f32; 2]; 4]) {
            let (main, span) = main_pattern(self.format, self.text).unwrap();
            let quiet = 12.;
            let map = Mapping {
                start: quiet * self.module,
                module: self.module,
                curve: self.curve,
                span,
            };
            let rows = |supplement: Option<&str>| {
                let mut bars = main.clone();
                let mut end = span;
                if let Some(text) = supplement {
                    let codes = supplement_codes(text);
                    bars.extend(
                        supplement_bars(&codes)
                            .iter()
                            .map(|&(a, b)| (a + span + self.gap, b + span + self.gap)),
                    );
                    end = span + self.gap + 4. + 9. * crate::numeric::usize_f32(codes.len()) - 2.;
                }
                bars.extend_from_slice(self.stray);
                bars.sort_by(|a, b| a.0.total_cmp(&b.0));
                (bars, end)
            };
            let upper = rows(self.supplement);
            let lower = rows(self.lower.or(self.supplement));
            let modules = self.cut.unwrap_or(span + 80.);
            let width = crate::numeric::f32_usize(map.pixel(modules).ceil());
            let height = 48;
            let mut image = vec![0u8; width * height];
            let mut noise = 12_345u32;
            for y in 0..height {
                let (bars, end) = if y < height / 2 { &upper } else { &lower };
                for x in 0..width {
                    let t = map.modules(crate::numeric::usize_f32(x) + 0.5);
                    let paper = match self.edge {
                        Some(edge) if t > end + edge => 120.,
                        _ => 230.,
                    };
                    let ink = coverage(bars, t, self.sigma).min(1.);
                    noise = noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    let jitter = crate::numeric::usize_f32((noise >> 16) as usize % 7) - 3.;
                    let v = paper - (paper - 40.) * ink + jitter;
                    image[y * width + x] =
                        u8::try_from(crate::numeric::f32_usize(v.clamp(0., 255.))).unwrap();
                }
            }
            let (a, b) = (map.pixel(0.), map.pixel(span));
            (image, width, height, [[a, 6.], [b, 6.], [b, 42.], [a, 42.]])
        }

        fn read(&self) -> Option<String> {
            let (image, width, height, polygon) = self.render();
            read(
                |x, y| f32::from(image[y * width + x]),
                width,
                height,
                &polygon,
                self.format,
                self.text,
                5,
            )
        }
    }

    #[test]
    fn reads_blurred_supplements_at_low_resolution() {
        for (supplement, module, gap, sigma) in [
            ("12", 1.2, 9., 0.6),
            ("54495", 1.3, 7., 0.55),
            ("90000", 1.5, 12., 0.65),
            ("07", 2., 5., 0.4),
            ("51299", 1.1, 10., 0.5),
        ] {
            let scene = Scene {
                supplement: Some(supplement),
                module,
                gap,
                sigma,
                ..SCENE
            };
            assert_eq!(
                scene.read().as_deref(),
                Some(supplement),
                "{supplement} at {module} px/module"
            );
        }
    }

    #[test]
    fn reads_supplements_of_every_retail_format() {
        for (format, text, supplement) in [
            ("UPCA", "012345678905", "24"),
            ("UPCE", "01234565", "51234"),
            ("EAN8", "96385074", "12"),
        ] {
            let scene = Scene {
                format,
                text,
                supplement: Some(supplement),
                module: 1.6,
                sigma: 0.5,
                ..SCENE
            };
            assert_eq!(scene.read().as_deref(), Some(supplement), "{format}");
        }
    }

    #[test]
    fn reads_supplements_under_perspective_and_before_a_label_edge() {
        let perspective = Scene {
            supplement: Some("52495"),
            module: 1.5,
            sigma: 0.5,
            curve: 0.07,
            ..SCENE
        };
        assert_eq!(perspective.read().as_deref(), Some("52495"));
        let edge = Scene {
            supplement: Some("38"),
            module: 1.4,
            sigma: 0.55,
            edge: Some(3.),
            ..SCENE
        };
        assert_eq!(edge.read().as_deref(), Some("38"));
    }

    #[test]
    fn invents_no_supplement() {
        for (module, sigma) in [(1.2, 0.6), (2., 0.4), (3., 0.3)] {
            let scene = Scene {
                module,
                sigma,
                ..SCENE
            };
            assert_eq!(scene.read(), None, "{module} px/module");
        }
        // Bars of supplement widths after a supplement guard, but no valid digits.
        let stray = [
            (104., 105.),
            (106., 108.),
            (109., 112.),
            (113., 114.),
            (116., 117.),
            (118., 120.),
            (123., 124.),
            (125., 127.),
            (128., 129.),
            (131., 134.),
        ];
        for sigma in [0.35, 0.55] {
            let scene = Scene {
                stray: &stray,
                sigma,
                ..SCENE
            };
            assert_eq!(scene.read(), None, "stray bars, blur {sigma}");
        }
    }

    #[test]
    fn a_cut_five_digit_supplement_reads_as_nothing() {
        // The image border cuts the third digit: its first two digits and their
        // guard alone look like a two-digit supplement.
        for cut in [26., 30.] {
            let scene = Scene {
                supplement: Some("51495"),
                module: 1.6,
                sigma: 0.5,
                cut: Some(95. + 9. + cut),
                ..SCENE
            };
            assert_eq!(scene.read(), None, "cut {cut} modules into the supplement");
        }
    }

    #[test]
    fn conflicting_profiles_read_nothing() {
        let scene = Scene {
            supplement: Some("12"),
            lower: Some("34"),
            ..SCENE
        };
        assert_eq!(scene.read(), None);
        let agreeing = Scene {
            supplement: Some("34"),
            lower: Some("34"),
            ..SCENE
        };
        assert_eq!(agreeing.read().as_deref(), Some("34"));
    }
}
