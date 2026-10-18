//! Periodic-texture evidence from original pixels.
//!
//! Stripes, rings, checkerboards and similar repeating patterns contain many parallel
//! edges, so the localizers box them and the retail decoders search them for symbols.
//! Every linear symbology mixes element widths and needs quiet zones, which such textures
//! lack. These tests sample source luminance along a box's scan axis and recognize two
//! cases: a box whose bar and space widths repeat or whose rows show different bars (no
//! symbol can be read there), and a read whose pattern continues through both quiet zones
//! without staying constant along its bars. Both follow the bars in the direction their
//! own pixels show, so perspective and shear keep printed symbols.
use crate::{Image, Quad};
use barcode_research_core::numeric::{f64_isize, f64_usize, isize_f64, usize_f64};

/// Distance between profile samples along the scan axis, in pixels.
const STEP: f64 = 0.5;
/// Longest sampled profile; longer axes are sampled more coarsely.
const MAX_SAMPLES: usize = 8192;
/// Longest box side, in image diagonals, that the tests examine. Larger or non-finite boxes
/// come from no localizer; they are kept untested rather than sampled without bound.
const MAX_EXTENT: f64 = 4.;

/// Whether every corner of `q` is finite and every side at most `MAX_EXTENT` image diagonals.
fn measurable(image: Image<'_>, q: &Quad) -> bool {
    let (width, height) = (usize_f64(image.width), usize_f64(image.height));
    let limit = MAX_EXTENT * (width * width + height * height).sqrt();
    q.iter().flatten().all(|v| v.is_finite())
        && (0..4).all(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            (b[0] - a[0]).hypot(b[1] - a[1]) <= limit
        })
}

/// Center line of a box whose first edge runs across the bars.
struct Axis {
    origin: [f64; 2],
    /// Unit vector along the scan axis.
    across: [f64; 2],
    /// Unit vector along the bars.
    along: [f64; 2],
    length: f64,
}

impl Axis {
    fn of(q: &Quad) -> Self {
        let mid = |p: [f64; 2], r: [f64; 2]| [f64::midpoint(p[0], r[0]), f64::midpoint(p[1], r[1])];
        let (origin, end) = (mid(q[0], q[3]), mid(q[1], q[2]));
        let length = (end[0] - origin[0]).hypot(end[1] - origin[1]);
        let across = if length > 0. {
            [(end[0] - origin[0]) / length, (end[1] - origin[1]) / length]
        } else {
            [1., 0.]
        };
        let side = [
            (q[3][0] - q[0][0] + q[2][0] - q[1][0]) / 2.,
            (q[3][1] - q[0][1] + q[2][1] - q[1][1]) / 2.,
        ];
        let height = side[0].hypot(side[1]);
        let along = if height >= 1. {
            [side[0] / height, side[1] / height]
        } else {
            [-across[1], across[0]]
        };
        Self {
            origin,
            across,
            along,
            length,
        }
    }

    /// Luminance every `step` pixels from `from` to `to` (fractions of the length) on the
    /// line `offset` pixels along the bars; positions outside the image are `None`.
    fn sample(
        &self,
        image: Image<'_>,
        offset: f64,
        (from, to): (f64, f64),
        step: f64,
        out: &mut Vec<Option<f64>>,
    ) {
        out.clear();
        let (right, bottom) = (usize_f64(image.width - 1), usize_f64(image.height - 1));
        let count = f64_usize(((to - from) * self.length / step).ceil().max(0.));
        for i in 0..count {
            let d = from * self.length + usize_f64(i) * step;
            let x = self.origin[0] + self.across[0] * d + self.along[0] * offset;
            let y = self.origin[1] + self.across[1] * d + self.along[1] * offset;
            out.push(
                (x >= 0. && y >= 0. && x <= right && y <= bottom)
                    .then(|| image.bilinear_luminance(x, y)),
            );
        }
    }

    /// The whole axis like [`Self::sample`], on the line `offset` pixels along bars whose
    /// direction turns from the angle `fan[0]` against the box's sides at the start of the
    /// axis to `fan[1]` at its end (a perspective fan).
    fn sample_fan(
        &self,
        image: Image<'_>,
        offset: f64,
        fan: [f64; 2],
        step: f64,
        out: &mut Vec<Option<f64>>,
    ) {
        out.clear();
        let (right, bottom) = (usize_f64(image.width - 1), usize_f64(image.height - 1));
        let count = f64_usize((self.length / step).ceil().max(0.));
        for i in 0..count {
            let d = usize_f64(i) * step;
            let (sin, cos) = (fan[0] + (fan[1] - fan[0]) * d / self.length).sin_cos();
            let bar = [
                self.along[0] * cos - self.along[1] * sin,
                self.along[0] * sin + self.along[1] * cos,
            ];
            let x = self.origin[0] + self.across[0] * d + bar[0] * offset;
            let y = self.origin[1] + self.across[1] * d + bar[1] * offset;
            out.push(
                (x >= 0. && y >= 0. && x <= right && y <= bottom)
                    .then(|| image.bilinear_luminance(x, y)),
            );
        }
    }

    /// Angle of `v` against the bars direction of the box, in (-pi/2, pi/2].
    fn angle_to_along(&self, v: [f64; 2]) -> f64 {
        let cross = self.along[0] * v[1] - self.along[1] * v[0];
        let dot = self.along[0] * v[0] + self.along[1] * v[1];
        half_turn(cross.atan2(dot))
    }
}

/// `angle` folded into (-pi/2, pi/2]: bar directions have no sign.
fn half_turn(angle: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut a = angle;
    while a > FRAC_PI_2 {
        a -= PI;
    }
    while a <= -FRAC_PI_2 {
        a += PI;
    }
    a
}

/// Run lengths, in samples, of a profile split at a moving mean of `2 * window + 1`
/// samples with hysteresis. Run boundaries are the interpolated crossings of the mean;
/// the first and last partial runs are dropped. Returns whether the first run is dark.
fn runs(profile: &[f64], window: usize, out: &mut Vec<f64>) -> bool {
    out.clear();
    let n = profile.len();
    if n < 3 {
        return false;
    }
    let mut sorted = profile.to_vec();
    let high = *sorted.select_nth_unstable_by(n * 19 / 20, f64::total_cmp).1;
    let low = *sorted.select_nth_unstable_by(n / 20, f64::total_cmp).1;
    let hysteresis = 0.08 * (high - low);
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.);
    for v in profile {
        prefix.push(prefix[prefix.len() - 1] + v);
    }
    let mut state: Option<bool> = None;
    let mut first_dark = false;
    let mut start: Option<f64> = None;
    let mut previous = 0.;
    let mut crossing = 0.;
    for (i, v) in profile.iter().enumerate() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(n);
        let delta = v - (prefix[hi] - prefix[lo]) / usize_f64(hi - lo);
        if i > 0 && (delta > 0.) != (previous > 0.) {
            crossing = usize_f64(i) - delta / (delta - previous);
        }
        previous = delta;
        let next = if delta > hysteresis {
            Some(true)
        } else if delta < -hysteresis {
            Some(false)
        } else {
            None
        };
        if let Some(light) = next
            && state != Some(light)
        {
            if state.is_some() {
                if let Some(start) = start {
                    out.push(crossing - start);
                } else {
                    first_dark = !light;
                }
                start = Some(crossing);
            }
            state = Some(light);
        }
    }
    first_dark
}

/// Share of runs whose width is repeated by the next run of the same color, within a
/// quarter of the larger width or within `jitter` for narrow runs whose edges pixel
/// quantization moves. Repeating and smoothly chirped textures (stripes, rings, zone
/// plates) repeat nearly every run; symbols mix narrow and wide elements.
fn repeat_share(widths: &[f64], jitter: f64) -> f64 {
    if widths.len() < 3 {
        return 0.;
    }
    let repeated = widths
        .windows(3)
        .filter(|w| (w[0] - w[2]).abs() <= (0.25 * w[0].max(w[2])).max(jitter))
        .count();
    usize_f64(repeated) / usize_f64(widths.len() - 2)
}

/// Share of `widths` within a quarter of their median, or within `jitter` of it.
fn median_share(widths: &mut [f64], jitter: f64) -> f64 {
    if widths.is_empty() {
        return 0.;
    }
    widths.sort_by(f64::total_cmp);
    let median = widths[widths.len() / 2];
    let tolerance = (0.25 * median).max(jitter);
    let close = widths
        .iter()
        .filter(|&&w| (w - median).abs() <= tolerance)
        .count();
    usize_f64(close) / usize_f64(widths.len())
}

/// True when the runs of one line repeat: nearly every run repeats the previous run of its
/// color (also for smoothly chirped widths), or nearly all bars share one width and nearly
/// all spaces another (also when pixel quantization moves single edges).
fn repeating(widths: &[f64], first_dark: bool, jitter: f64) -> bool {
    if widths.len() < PERIODIC_RUNS {
        return false;
    }
    if repeat_share(widths, jitter) >= PERIODIC_SHARE {
        return true;
    }
    let start = usize::from(!first_dark);
    let mut bars: Vec<f64> = widths.iter().skip(start).step_by(2).copied().collect();
    let mut spaces: Vec<f64> = widths.iter().skip(1 - start).step_by(2).copied().collect();
    median_share(&mut bars, jitter) >= PERIODIC_SHARE
        && median_share(&mut spaces, jitter) >= PERIODIC_SHARE
}

/// Edge displacement, in pixels, that pixel quantization causes in a repeating pattern.
const JITTER: f64 = 0.6;
/// Runs below this count cannot establish a repeating pattern.
const PERIODIC_RUNS: usize = 16;
/// Share of runs that must repeat the previous run of their color.
const PERIODIC_SHARE: f64 = 0.8;
/// Consecutive runs, fewer than half of them regular, that show a symbol on a line that
/// otherwise crosses a periodic background.
const SYMBOL_RUNS: usize = 12;

/// How often the widths of one color turn from growing to shrinking or back, by more than a
/// quarter each, among runs marked `ordinary`: symbol elements alternate between narrow and
/// wide, while the runs where a line passes the center of rings widen and narrow once.
fn turns(widths: &[f64], ordinary: &[bool]) -> usize {
    let steps: Vec<f64> = (0..widths.len().saturating_sub(2))
        .filter(|&i| ordinary[i..i + 3].iter().all(|&o| o))
        .filter(|&i| (widths[i + 2] - widths[i]).abs() > 0.25 * widths[i].max(widths[i + 2]))
        .map(|i| widths[i + 2] - widths[i])
        .collect();
    steps.windows(2).filter(|p| p[0] * p[1] < 0.).count()
}

/// Narrowest background bar or space, in pixels, beside which a symbol stretch is sought.
const MIN_BACKGROUND: f64 = 2.;
/// Consecutive runs of the background's width on either side of a symbol stretch.
const BACKGROUND_RUNS: usize = 8;
/// Width, relative to the median run of a line, above which a run is no symbol element.
const WIDE_ELEMENT: f64 = 2.5;

/// Whether a line crosses a symbol on a periodic background: between two stretches of
/// `BACKGROUND_RUNS` consecutive runs of the background's widths (the line's median bar and
/// space), at least `SYMBOL_RUNS` other runs whose widths alternate between narrow and wide
/// (see [`turns`]) and, apart from two quiet zones, none much wider than the background's.
/// Symbologies with two element widths (ITF, Code 39) have at most a few wide elements in a
/// row. Where a line passes the center of rings or of a zone plate its runs widen and narrow
/// only once.
fn symbol_stretch(widths: &[f64], step: f64) -> bool {
    if widths.len() < SYMBOL_RUNS + 2 * BACKGROUND_RUNS {
        return false;
    }
    // Bars and spaces of the background can differ in width: each color has its own.
    let median = |parity: usize| {
        let mut sorted: Vec<f64> = widths.iter().skip(parity).step_by(2).copied().collect();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2]
    };
    let medians = [median(0), median(1)];
    // Pixel-fine runs (aliased rings and zone plates) are no background beside a symbol.
    if medians[0].min(medians[1]) * step < MIN_BACKGROUND {
        return false;
    }
    let background = |i: usize| medians[i % 2];
    let matches = |i: usize| (widths[i] - background(i)).abs() <= 0.25 * background(i);
    // Ends of background stretches: (first run after one, first run of the next).
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut streak = 0;
    let mut after: Option<usize> = None;
    for i in 0..widths.len() {
        if matches(i) {
            streak += 1;
            if streak == BACKGROUND_RUNS
                && let Some(from) = after
            {
                bounds.push((from, i + 1 - BACKGROUND_RUNS));
            }
        } else {
            if streak >= BACKGROUND_RUNS {
                after = Some(i);
            }
            streak = 0;
        }
    }
    bounds.into_iter().any(|(from, to)| {
        let wide = (from..to)
            .filter(|&i| widths[i] >= WIDE_ELEMENT * background(i))
            .count();
        // A run wider than the smoothing window can split into slivers beside it.
        let ordinary: Vec<bool> = (from..to)
            .map(|i| {
                (i.saturating_sub(1)..=(i + 1).min(widths.len() - 1))
                    .all(|j| widths[j] < WIDE_ELEMENT * background(j))
            })
            .collect();
        to - from >= SYMBOL_RUNS && wide <= 2 && turns(&widths[from..to], &ordinary) >= 3
    })
}

/// Longest period, in pixels, that [`fine_period`] looks for. Linear symbols cannot repeat
/// that finely: their shortest repeated unit (an EAN digit) has seven modules.
const FINE_PERIOD: f64 = 6.;
/// Autocorrelation a fine period must reach at one and at two periods.
const FINE_PERIOD_CORRELATION: f64 = 0.5;
/// Pixels of a window in which [`fine_period`] looks for one period. Chirped textures (rings
/// beside their center) change their period along a box.
const FINE_PERIOD_WINDOW: f64 = 48.;
/// Share of the windows with variation that must repeat a fine period.
const FINE_PERIOD_SHARE: f64 = 0.5;

/// Normalized autocorrelation of `part` at lags `0..r.len()`; false for a flat part.
fn autocorrelation(part: &[f64], r: &mut [f64]) -> bool {
    let mean = part.iter().sum::<f64>() / usize_f64(part.len());
    for (lag, value) in r.iter_mut().enumerate() {
        *value = part
            .iter()
            .zip(&part[lag..])
            .map(|(a, b)| (a - mean) * (b - mean))
            .sum::<f64>()
            / usize_f64(part.len() - lag);
    }
    let variance = r[0];
    if variance <= 1. {
        return false;
    }
    for value in r.iter_mut() {
        *value /= variance;
    }
    true
}

/// True when a profile repeats with a period of at most `FINE_PERIOD` pixels. Pixel
/// quantization of such fine stripes and rings at an angle moves single edges by a large part
/// of a run, which hides the repetition from [`repeating`]. Most windows (with variation)
/// must have an autocorrelation that turns negative and then exceeds
/// `FINE_PERIOD_CORRELATION` at a lag and at twice that lag. Stops once too few windows can
/// still repeat.
fn fine_period(profile: &[f64], step: f64) -> bool {
    let max_lag = f64_usize(2. * FINE_PERIOD / step);
    let window = f64_usize(FINE_PERIOD_WINDOW / step);
    if max_lag < 4 || window < 4 * max_lag || profile.len() < window {
        return false;
    }
    let mut r = vec![0.; max_lag + 1];
    let dip = |r: &[f64]| (1..r.len()).find(|&lag| r[lag] < 0.);
    let total = profile.len() / window;
    let allowed = usize_f64(total) * (1. - FINE_PERIOD_SHARE);
    let (mut periodic, mut other) = (0_usize, 0_usize);
    for part in profile.chunks_exact(window) {
        // Flat windows say nothing.
        if !autocorrelation(part, &mut r) {
            continue;
        }
        let repeats = dip(&r).is_some_and(|d| {
            (d + 1..=max_lag / 2).any(|lag| {
                r[lag] >= FINE_PERIOD_CORRELATION
                    && r[2 * lag - 1..=(2 * lag + 1).min(max_lag)]
                        .iter()
                        .any(|&v| v >= FINE_PERIOD_CORRELATION)
            })
        });
        if repeats {
            periodic += 1;
        } else {
            other += 1;
            if usize_f64(other) > allowed {
                return false;
            }
        }
    }
    periodic >= 2 && usize_f64(periodic) >= FINE_PERIOD_SHARE * usize_f64(periodic + other)
}

/// Ratio of edge energy along a box's scan axis over the energy across it at which the
/// box's edges run along the axis (beyond about 55 degrees from the bar direction).
const PARALLEL_EDGES: f64 = 2.;

/// True when the edges in a box run along its scan axis rather than across it: the box
/// follows stripes or the moire of fine stripes, while a symbol's bars cross the axis.
/// Luminance differences across and along the box are summed every four pixels on three
/// lines (the center and a quarter of the height to either side).
fn parallel_edges(image: Image<'_>, axis: &Axis, height: f64) -> bool {
    let (right, bottom) = (usize_f64(image.width - 1), usize_f64(image.height - 1));
    let lum = |p: [f64; 2]| {
        (p[0] >= 0. && p[1] >= 0. && p[0] <= right && p[1] <= bottom)
            .then(|| image.bilinear_luminance(p[0], p[1]))
    };
    let (mut across, mut along) = (0., 0.);
    let points = f64_usize(axis.length / 4.).min(MAX_SAMPLES);
    for offset in [-0.25, 0., 0.25] {
        for i in 0..points {
            let d = usize_f64(i) * 4.;
            let p = [
                axis.origin[0] + axis.across[0] * d + axis.along[0] * offset * height,
                axis.origin[1] + axis.across[1] * d + axis.along[1] * offset * height,
            ];
            let shifted = |v: [f64; 2], s: f64| lum([p[0] + v[0] * s, p[1] + v[1] * s]);
            if let (Some(a0), Some(a1), Some(b0), Some(b1)) = (
                shifted(axis.across, -1.),
                shifted(axis.across, 1.),
                shifted(axis.along, -1.),
                shifted(axis.along, 1.),
            ) {
                across += (a1 - a0) * (a1 - a0);
                along += (b1 - b0) * (b1 - b0);
            }
        }
    }
    along > PARALLEL_EDGES * across
}

/// Correlation along the bars at which a part of a box holds printed bars.
const COHERENT_WINDOW: f64 = 0.5;

/// Whether a box's lines repeat (see [`texture_proposal`]); also returns the three lines
/// across it (its center and a quarter of its height to either side) and the number of
/// outer lines with enough runs to show incoherence.
fn periodic_box(
    image: Image<'_>,
    axis: &Axis,
    height: f64,
    step: f64,
) -> (bool, [Vec<Option<f64>>; 3], usize) {
    let window = f64_usize((axis.length / 24. / step).max(6.));
    let mut lines: [Vec<Option<f64>>; 3] = Default::default();
    let mut profile = Vec::new();
    let mut widths = Vec::new();
    let mut repeats = [false; 3];
    // Lines with at least one run per `FINE_PERIOD`, which a fine period needs.
    let mut dense = [false; 3];
    let mut edged_outer = 0;
    for (i, (line, offset)) in lines.iter_mut().zip([0., -0.25, 0.25]).enumerate() {
        axis.sample(image, offset * height, (0., 1.), step, line);
        profile.clear();
        profile.extend(line.iter().flatten());
        let first_dark = runs(&profile, window, &mut widths);
        repeats[i] = repeating(&widths, first_dark, JITTER / step);
        dense[i] = usize_f64(widths.len()) * FINE_PERIOD >= usize_f64(profile.len()) * step;
        // Flat or nearly flat outer lines give no evidence of incoherence.
        edged_outer += usize::from(offset != 0. && widths.len() >= PERIODIC_RUNS);
    }
    // The costlier fine-period test runs only on dense lines that can still decide.
    let mut repeating_lines = repeats.iter().filter(|&&r| r).count();
    for (i, line) in lines.iter().enumerate() {
        let open = repeats[i..].iter().filter(|&&r| !r).count();
        if repeating_lines >= 2 || repeating_lines + open < 2 {
            break;
        }
        if !repeats[i] && dense[i] {
            profile.clear();
            profile.extend(line.iter().flatten());
            repeating_lines += usize::from(fine_period(&profile, step));
        }
    }
    // A box can lie mostly on a periodic background beside or around a symbol (a ribbed
    // carton): the symbol then interrupts the repetition along one of its lines, or along a
    // row elsewhere in the box (see [`symbol_stretch`]). Pixel-fine periods (see
    // [`fine_period`]) are too fine for a background beside a symbol.
    let coarse = repeats.iter().filter(|&&r| r).count() >= 2;
    let periodic = repeating_lines >= 2
        && (!coarse
            || !lines.iter().zip(repeats).any(|(line, repeat)| {
            !repeat && {
                profile.clear();
                profile.extend(line.iter().flatten());
                runs(&profile, window, &mut widths);
                symbol_stretch(&widths, step) && !fine_period(&profile, step)
            }
        })
        // A symbol shorter than the box can lie between the lines or at its edge: rows every
        // sixteenth of the height must not cross one either.
        && [-7, -6, -5, -3, -2, -1, 1, 2, 3, 5, 6, 7].into_iter().all(|k| {
            let offset = f64::from(k) / 16.;
            let mut line = Vec::new();
            axis.sample(image, offset * height, (0., 1.), step, &mut line);
            profile.clear();
            profile.extend(line.iter().flatten());
            runs(&profile, window, &mut widths);
            // Rows that leave and reenter the image join unrelated stretches.
            let inside = line.iter().skip_while(|v| v.is_none()).collect::<Vec<_>>();
            inside
                .iter()
                .rev()
                .skip_while(|v| v.is_none())
                .any(|v| v.is_none())
                || !symbol_stretch(&widths, step)
                || fine_period(&profile, step)
        }));
    (periodic, lines, edged_outer)
}

/// True when a localized box lies on a texture that holds no linear symbol. Three lines
/// across the box (its center and a quarter of its height to either side) are sampled once:
///
/// - Periodic: the runs repeat (see [`repeating`]), or the profile repeats a period of a
///   few pixels (see [`fine_period`]), on at least two lines, and neither those lines nor
///   rows every sixteenth of the height cross a symbol on the background (see
///   [`periodic_box`]). No linear symbology encodes data that way; one line may cross the
///   center of rings or the edge of the texture. A box whose edges run along its axis (see
///   [`parallel_edges`]) is also tested across it.
/// - Incoherent: no two lines across the box show the same bars along a run of consecutive
///   windows (see [`coherent_bars`]), at the displacement that bars crossing the box
///   squarely or in the direction nearby lines show (see [`bar_slopes`]) predict. Printed
///   bars run along the whole symbol, however loosely the box surrounds it, while text,
///   noise and bar patterns that change every few rows do not.
pub(crate) fn texture_proposal(image: Image<'_>, q: &Quad) -> bool {
    if !measurable(image, q) {
        return false;
    }
    let axis = Axis::of(q);
    let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    if axis.length < 16. {
        return false;
    }
    let step = STEP.max(axis.length / usize_f64(MAX_SAMPLES));
    let (periodic, lines, edged_outer) = periodic_box(image, &axis, height, step);
    if periodic {
        return true;
    }
    // A box whose edges run along its axis (on stripes, or along a column of symbols) is
    // read across: it lies on a texture when the lines across its axis repeat.
    if parallel_edges(image, &axis, height) {
        let turned = Axis::of(&[q[1], q[2], q[3], q[0]]);
        if turned.length >= 16. {
            let step = STEP.max(turned.length / usize_f64(MAX_SAMPLES));
            if periodic_box(image, &turned, axis.length, step).0 {
                return true;
            }
        }
    }
    if height < 8. || edged_outer < 2 {
        return false;
    }
    let [_, above, below] = &lines;
    // Most boxes cross their bars squarely; otherwise the bars are followed in the direction
    // that nearby lines show.
    let square = Lags {
        axis: &axis,
        step,
        square: true,
        slopes: None,
    };
    let (coherent, windows) = coherent_bars(above, below, &square, 0.5 * height, RUN);
    if coherent || windows < 3 {
        return false;
    }
    let lags = Lags {
        axis: &axis,
        step,
        square: false,
        slopes: bar_slopes(image, &axis, &lines, height, step),
    };
    if coherent_bars(above, below, &lags, 0.5 * height, RUN).0 {
        return false;
    }
    // A box taller than its symbol can put an outer line beside the bars. Printed bars run
    // through every row of the symbol, so a pair of lines at least three eighths of the
    // height apart shows them when each neighboring pair between them does too.
    let rows: Vec<Vec<Option<f64>>> = (-3..=3)
        .map(|k| {
            let mut row = Vec::new();
            axis.sample(
                image,
                f64::from(k) * 0.125 * height,
                (0., 1.),
                step,
                &mut row,
            );
            row
        })
        .collect();
    // Bars slant between two rows in proportion to their separation, and they run through
    // every row between the two at the same place: a window counts for a pair of rows only
    // where each neighboring pair between them shows the same bars there too. Neighboring
    // windows are matched once, when first needed.
    let samples = rows[0].len();
    let part = window_samples(samples, step);
    let count = samples.div_ceil(part);
    let center = |w: usize| usize_f64(2 * w * part + part) * step / (2. * axis.length);
    let mut neighbors: Vec<Vec<Option<Match>>> = vec![vec![None; count]; rows.len() - 1];
    for i in 0..rows.len() {
        for j in i + 3..rows.len() {
            let separation = usize_f64(j - i) * 0.125 * height;
            if (i, j) == (1, 5) || separation < MIN_SEPARATION {
                continue;
            }
            let matches = (0..count).map(|w| {
                let start = w * part;
                let mut flat = false;
                for k in i..j {
                    let m = *neighbors[k][w].get_or_insert_with(|| {
                        let ranges = lags.around(center(w), 0.125 * height);
                        window_match(&rows[k], &rows[k + 1], start, part, ranges)
                    });
                    match m {
                        Match::Different => return Match::Different,
                        Match::Flat => flat = true,
                        Match::Same(_) => {}
                    }
                }
                if flat {
                    Match::Flat
                } else {
                    let ranges = lags.around(center(w), separation);
                    window_match(&rows[i], &rows[j], start, part, ranges)
                }
            });
            if bars_run(matches, part, axis.length, step, separation, SHORT_RUN) {
                return false;
            }
        }
    }
    true
}

/// Half the bar slant (tangent, about 27 degrees in all) that the windows of a box whose
/// pixels show no bar direction may be aligned for.
const BAR_SLANT: f64 = 0.25;
/// Pixels of a window along the axis that is checked for printed bars.
const WINDOW: f64 = 24.;
/// Axis stretch, in pixels, over which consecutive windows of the outer lines must show the
/// same bars (a third of the axis in a shorter box). Printed symbols are wider than this.
const RUN: f64 = 96.;
/// The same for the other pairs of lines, which are also checked row by row.
const SHORT_RUN: f64 = 72.;
/// Closest two lines may be, in pixels, to count as different rows of a pattern redrawn
/// every few rows.
const MIN_SEPARATION: f64 = 16.;
/// Largest change of lateral displacement, in pixels, between neighboring windows of one bar
/// pattern.
const DISPLACEMENT_STEP: f64 = 1.5;
/// Closest vanishing point of perspective-converging bars, in axis lengths from the lines.
const FAN_DISTANCE: f64 = 2.;

/// How a window of one line matches another line (see [`window_match`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Match {
    /// Too little variation to measure (inside a wide bar, or beside the symbol).
    Flat,
    /// Measured, but no displacement correlates.
    Different,
    /// The bars correlate at this lateral displacement from the expected one, in samples.
    Same(isize),
}

/// Samples per window of lines of `samples` samples.
fn window_samples(samples: usize, step: f64) -> usize {
    samples.div_ceil(6).min(f64_usize(WINDOW / step)).max(16)
}

/// Where the bars of one line of a box appear on another line: their lateral displacement
/// follows the bar direction that the box's pixels show (see [`bar_fan`]). Without one
/// (text, noise), any bar slant up to `2 * BAR_SLANT` (tangent) is possible.
struct Lags<'a> {
    axis: &'a Axis,
    step: f64,
    /// Whether only bars crossing the box squarely are considered.
    square: bool,
    /// Axis displacement of the bars per pixel along the box's sides, at the start and the
    /// end of its axis (see [`bar_slopes`]).
    slopes: Option<[f64; 2]>,
}

impl Lags<'_> {
    /// The displacement ranges, in samples (expected displacement and half-width), searched
    /// at the fraction `t` of the axis between lines `separation` pixels apart: bars crossing
    /// the box squarely and bars in the measured direction, or any slant up to
    /// `2 * BAR_SLANT` when the box's pixels show no direction.
    fn around(&self, t: f64, separation: f64) -> [Option<(isize, isize)>; 2] {
        let range = |center: f64, tolerance: f64, slack: f64| {
            let half = (self.axis.length / 200.)
                .max(1.5)
                .max((tolerance * separation).min(slack));
            (
                f64_isize((center / self.step).round()),
                f64_isize((half / self.step).ceil()),
            )
        };
        let square = range(0., SLOPE_TOLERANCE, SLOPE_SLACK);
        match (self.square, self.slopes) {
            (true, _) => [Some(square), None],
            (false, Some(s)) => [
                Some(square),
                Some(range(
                    separation * (s[0] + (s[1] - s[0]) * t),
                    SLOPE_TOLERANCE,
                    SLOPE_SLACK,
                )),
            ],
            (false, None) => [Some(range(0., 2. * BAR_SLANT, f64::INFINITY)), None],
        }
    }
}

/// Pixels along a box's sides between the lines whose displacement [`bar_slopes`] measures.
const SLOPE_DISTANCE: f64 = 6.;
/// Largest slope [`bar_slopes`] measures (the tangent of 50 degrees).
const SLOPE_LIMIT: f64 = 1.2;
/// Correlation at which two nearby lines show the same bars for [`bar_slopes`].
const SLOPE_CORRELATION: f64 = 0.8;
/// Correlation below the peak that still counts as its plateau in [`bar_slopes`].
const SLOPE_PLATEAU: f64 = 0.02;
/// Samples around the middle of each window that [`bar_slopes`] correlates.
const SLOPE_SAMPLES: usize = 64;
/// Remaining slope around the measured one that may still align bars, and the largest
/// displacement, in pixels, it may allow: in tall boxes, many displacements would match
/// unrelated patterns by chance.
const SLOPE_TOLERANCE: f64 = 0.1;
const SLOPE_SLACK: f64 = 24.;

/// The bar direction in a box, as the displacement along the axis per pixel along its sides,
/// at the start and the end of the axis: a line fitted to the displacement at which each of
/// `FAN_WINDOWS` windows of the box's three `lines` (its center and a quarter of its height
/// to either side) matches the line `SLOPE_DISTANCE` pixels further. Lines that close show
/// the same bars even in patterns redrawn every few rows. `None` when fewer than two
/// windows match (text, noise).
fn bar_slopes(
    image: Image<'_>,
    axis: &Axis,
    lines: &[Vec<Option<f64>>; 3],
    height: f64,
    step: f64,
) -> Option<[f64; 2]> {
    let limit = f64_isize((SLOPE_LIMIT * SLOPE_DISTANCE / step).ceil());
    let mut further = Vec::new();
    // (position along the axis, slope, weight) of each window that matches.
    let mut fits: Vec<(f64, f64, f64)> = Vec::new();
    let samples = lines[0].len();
    for (line, offset) in lines.iter().zip([0., -0.25, 0.25]) {
        axis.sample(
            image,
            offset * height + SLOPE_DISTANCE,
            (0., 1.),
            step,
            &mut further,
        );
        for w in 0..FAN_WINDOWS {
            let middle = (2 * w + 1) * samples / (2 * FAN_WINDOWS);
            let start = middle.saturating_sub(SLOPE_SAMPLES / 2);
            let end = (start + SLOPE_SAMPLES).min(samples);
            let correlations: Vec<(isize, f64)> = (-limit..=limit)
                .filter_map(|l| correlation(&line[start..end], &further, start, l).map(|c| (l, c)))
                .collect();
            let peak = correlations.iter().map(|lc| lc.1).fold(f64::MIN, f64::max);
            if peak < SLOPE_CORRELATION {
                continue;
            }
            // Sharp edges correlate equally over a plateau of displacements; take its middle.
            let plateau: Vec<isize> = correlations
                .iter()
                .filter(|lc| lc.1 >= peak - SLOPE_PLATEAU)
                .map(|lc| lc.0)
                .collect();
            let lag = plateau.iter().map(|&l| isize_f64(l)).sum::<f64>() / usize_f64(plateau.len());
            fits.push((usize_f64(w), lag * step / SLOPE_DISTANCE, peak));
        }
    }
    if fits.len() < 2 {
        return None;
    }
    let position = |w: f64| (w + 0.5) / usize_f64(FAN_WINDOWS);
    let total: f64 = fits.iter().map(|f| f.2).sum();
    let mean_t = fits.iter().map(|f| position(f.0) * f.2).sum::<f64>() / total;
    let mean_s = fits.iter().map(|f| f.1 * f.2).sum::<f64>() / total;
    let (mut tt, mut ts) = (0., 0.);
    for (w, slope, weight) in &fits {
        let t = position(*w) - mean_t;
        tt += weight * t * t;
        ts += weight * t * (slope - mean_s);
    }
    let rise = if tt > 0. { ts / tt } else { 0. };
    Some([mean_s - rise * mean_t, mean_s + rise * (1. - mean_t)])
}

/// How the window of line `a` at sample `start` matches line `b`. Perspective and shear
/// move the bars between the lines differently along the axis, so each window aligns on its
/// own within `ranges` (each an expected displacement and the half-width around it, in
/// samples); displacements beyond one sample from the expected one are tried every second
/// sample, nearest first, until one correlates. A match reports its displacement from the
/// expected one.
fn window_match(
    a: &[Option<f64>],
    b: &[Option<f64>],
    start: usize,
    part: usize,
    ranges: [Option<(isize, isize)>; 2],
) -> Match {
    let end = (start + part).min(a.len());
    let mut measured = false;
    for (center, half) in ranges.into_iter().flatten() {
        let offsets = [0, -1, 1]
            .into_iter()
            .chain((2..=half).step_by(2).flat_map(|l| [-l, l]));
        for l in offsets {
            if let Some(c) = correlation(&a[start..end], b, start, center + l) {
                measured = true;
                if c >= COHERENT_WINDOW {
                    return Match::Same(l);
                }
            }
        }
    }
    if measured {
        Match::Different
    } else {
        Match::Flat
    }
}

/// Whether the window matches between two lines `separation` pixels apart show the same
/// bars: a run of consecutive windows along the axis (`run` pixels, or a third of the axis
/// in a shorter box) at nearly the same displacement, or at a displacement that changes
/// steadily (bars converging in perspective). Flat windows neither extend nor break a run.
/// Stops at the first such run.
fn bars_run(
    matches: impl IntoIterator<Item = Match>,
    part: usize,
    length: f64,
    step: f64,
    separation: f64,
    run: f64,
) -> bool {
    let needed = f64_usize((run.min(length / 3.) / (usize_f64(part) * step)).ceil()).max(2);
    let displacement_step = f64_isize((DISPLACEMENT_STEP / step).ceil());
    // Bars converging to a vanishing point at least `FAN_DISTANCE` axis lengths away change
    // the displacement by a constant amount from window to window, up to this many samples.
    let fan_step = f64_isize((separation * usize_f64(part) / (FAN_DISTANCE * length)).ceil())
        .max(displacement_step);
    let mut streak = 0;
    let mut previous = 0;
    let mut drift = 0;
    for m in matches {
        let l = match m {
            Match::Flat => continue,
            Match::Different => {
                streak = 0;
                continue;
            }
            Match::Same(l) => l,
        };
        let change = l - previous;
        let continues = streak > 0
            && (change.abs() <= displacement_step
                || (change.abs() <= fan_step
                    && (streak == 1 || (change - drift).abs() <= displacement_step)));
        streak = if continues { streak + 1 } else { 1 };
        drift = if continues { change } else { 0 };
        previous = l;
        if streak >= needed {
            return true;
        }
    }
    false
}

/// Whether lines `a` and `b`, `separation` pixels apart, show the same bars (see
/// [`window_match`] and [`bars_run`]). Also returns the number of windows with enough
/// variation to measure, up to the run that shows the bars.
fn coherent_bars(
    a: &[Option<f64>],
    b: &[Option<f64>],
    lags: &Lags<'_>,
    separation: f64,
    run: f64,
) -> (bool, usize) {
    let part = window_samples(a.len(), lags.step);
    let mut measured = 0;
    let matches = (0..a.len()).step_by(part).map(|start| {
        let t = usize_f64(2 * start + part) * lags.step / (2. * lags.axis.length);
        let m = window_match(a, b, start, part, lags.around(t, separation));
        measured += usize::from(m != Match::Flat);
        m
    });
    let coherent = bars_run(matches, part, lags.axis.length, lags.step, separation, run);
    (coherent, measured)
}

/// Mean absolute luminance step between adjacent samples; `None` when fewer than half
/// of the positions lie inside the image.
fn activity(samples: &[Option<f64>]) -> Option<f64> {
    let mut total = 0.;
    let mut steps = 0;
    for pair in samples.windows(2) {
        if let [Some(a), Some(b)] = pair {
            total += (a - b).abs();
            steps += 1;
        }
    }
    (steps > 0 && steps * 2 >= samples.len()).then(|| total / usize_f64(steps))
}

/// Pearson correlation of `a`, which starts at sample `offset` of `b`, and `b` displaced by
/// `lag` samples, over positions present in both.
fn correlation(a: &[Option<f64>], b: &[Option<f64>], offset: usize, lag: isize) -> Option<f64> {
    let (mut sa, mut sb, mut saa, mut sbb, mut sab, mut n) = (0., 0., 0., 0., 0., 0_usize);
    for (i, x) in a.iter().enumerate() {
        let Some(j) = (i + offset).checked_add_signed(lag) else {
            continue;
        };
        if let (Some(x), Some(Some(y))) = (x, b.get(j)) {
            sa += x;
            sb += y;
            saa += x * x;
            sbb += y * y;
            sab += x * y;
            n += 1;
        }
    }
    if n < 8 {
        return None;
    }
    let n = usize_f64(n);
    let va = saa - sa * sa / n;
    let vb = sbb - sb * sb / n;
    (va > 0. && vb > 0.).then(|| (sab - sa * sb / n) / (va * vb).sqrt())
}

/// Margin activity, relative to the symbol interior, at which a read lacks quiet zones.
const BUSY_MARGINS: f64 = 0.6;
/// Narrow element width, in samples: the fifth-smallest share of a profile's run widths
/// (zero without runs).
fn narrow_width(widths: &[f64]) -> f64 {
    if widths.is_empty() {
        return 0.;
    }
    let mut sorted = widths.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 5]
}

/// Correlation along the bars below which the pattern is two-dimensional.
const INCONSISTENT_BARS: f64 = 0.35;
/// Largest bar slant against the read's sides that the shifted lines may be aligned for
/// (the tangent, about 20 degrees), and the spacing of the slants tried.
const SLANT: f64 = 0.36;
const SLANT_STEP: f64 = 0.04;
/// Share of adjacent runs with similar widths at which the profile is a smooth
/// periodic or chirped texture.
const REGULAR_RUNS: f64 = 0.85;

/// Bar angles, in radians against the read's sides, below which a fan adds nothing to the
/// slants that [`consistent_shifts`] tries.
const FAN_MIN: f64 = 0.06;
/// Remaining slant (tangent) tried around a followed bar direction.
const FAN_SLANT: f64 = 0.08;
/// Windows along the read in which [`bar_fan`] measures the bar direction.
const FAN_WINDOWS: usize = 8;
/// Gradient coherence (0 for no preferred direction, 1 for parallel edges) a window needs to
/// show a bar direction.
const FAN_COHERENCE: f64 = 0.4;
/// Largest bar angle against the read's sides that [`bar_fan`] reports (50 degrees).
const FAN_LIMIT: f64 = 0.87;

/// Whether each pair of lines shifted along the bars (`shifted`, two per distance) holds the
/// center line's pattern at one shared slant up to `slant` (tangent): the lines shift sideways
/// in proportion to their distance, within `lag` samples.
fn consistent_shifts(
    center: &[Option<f64>],
    shifted: &[(f64, Vec<Option<f64>>)],
    slant: f64,
    lag: isize,
    step: f64,
) -> bool {
    let steps = f64_isize((slant / SLANT_STEP).round());
    let slants = || (0..=steps).flat_map(|k| if k == 0 { vec![0] } else { vec![-k, k] });
    slants().any(|k| {
        let slant = isize_f64(k) * SLANT_STEP;
        shifted.chunks(2).all(|pair| {
            let mut best: Option<f64> = None;
            for (offset, line) in pair {
                let middle = f64_isize((slant * offset / step).round());
                for l in middle - lag..=middle + lag {
                    if let Some(c) = correlation(center, line, 0, l) {
                        best = Some(best.map_or(c, |b| b.max(c)));
                    }
                }
            }
            // Lines outside the image or on flat surroundings give no evidence.
            best.is_none_or(|b| b >= INCONSISTENT_BARS)
        })
    })
}

/// The bar direction a read's pixels show at its start and its end, as angles against the
/// read's sides: a line fitted to the dominant edge direction of `FAN_WINDOWS` windows along
/// the read. `None` when the edges have no dominant direction in a quarter of the windows
/// (a two-dimensional pattern) or turn beyond `FAN_LIMIT`.
fn bar_fan(image: Image<'_>, axis: &Axis, height: f64) -> Option<[f64; 2]> {
    let (right, bottom) = (usize_f64(image.width - 1), usize_f64(image.height - 1));
    let lum = |x: f64, y: f64| {
        (x >= 1. && y >= 1. && x <= right - 1. && y <= bottom - 1.)
            .then(|| image.bilinear_luminance(x, y))
    };
    // (position along the read, angle, weight) of each window with a dominant direction.
    let mut fits: Vec<(f64, f64, f64)> = Vec::new();
    let per_window = 0.9 / usize_f64(FAN_WINDOWS);
    let points = f64_usize((per_window * axis.length).max(1.)).min(MAX_SAMPLES / FAN_WINDOWS);
    for w in 0..FAN_WINDOWS {
        let (mut xx, mut xy, mut yy) = (0., 0., 0.);
        let from = 0.05 + usize_f64(w) * per_window;
        for i in 0..points {
            let d = (from + per_window * usize_f64(i) / usize_f64(points)) * axis.length;
            for offset in [-0.25, 0., 0.25] {
                let x = axis.origin[0] + axis.across[0] * d + axis.along[0] * offset * height;
                let y = axis.origin[1] + axis.across[1] * d + axis.along[1] * offset * height;
                if let (Some(l), Some(r), Some(u), Some(b)) = (
                    lum(x - 1., y),
                    lum(x + 1., y),
                    lum(x, y - 1.),
                    lum(x, y + 1.),
                ) {
                    let (gx, gy) = (r - l, b - u);
                    xx += gx * gx;
                    xy += gx * gy;
                    yy += gy * gy;
                }
            }
        }
        let energy = xx + yy;
        if energy <= 0. {
            continue;
        }
        let coherence = (xx - yy).hypot(2. * xy) / energy;
        if coherence < FAN_COHERENCE {
            continue;
        }
        // Bars run perpendicular to the dominant gradient.
        let gradient = 0.5 * (2. * xy).atan2(xx - yy);
        let bar = [-gradient.sin(), gradient.cos()];
        fits.push((
            from + per_window / 2.,
            axis.angle_to_along(bar),
            energy * coherence,
        ));
    }
    // Angles near 90 degrees would wrap; such a read crosses no bars.
    if fits.len() * 4 < FAN_WINDOWS * 3 || fits.iter().any(|f| f.1.abs() > FAN_LIMIT) {
        return None;
    }
    let total: f64 = fits.iter().map(|f| f.2).sum();
    let mean_t = fits.iter().map(|f| f.0 * f.2).sum::<f64>() / total;
    let mean_a = fits.iter().map(|f| f.1 * f.2).sum::<f64>() / total;
    let (mut st, mut sta) = (0., 0.);
    for (t, a, w) in &fits {
        st += w * (t - mean_t) * (t - mean_t);
        sta += w * (t - mean_t) * (a - mean_a);
    }
    let slope = if st > 0. { sta / st } else { 0. };
    let fan = [mean_a - slope * mean_t, mean_a + slope * (1. - mean_t)];
    (fan[0].abs().max(fan[1].abs()) <= FAN_LIMIT).then_some(fan)
}

/// Whether a read's pattern is constant along its bars: lines shifted along them on both
/// sides hold the center line's pattern (see [`consistent_shifts`]); `module` is the
/// narrow element width in pixels.
fn constant_along_bars(
    image: Image<'_>,
    axis: &Axis,
    q: &Quad,
    center: &[Option<f64>],
    module: f64,
    step: f64,
) -> bool {
    let length = axis.length;
    let span = |offset: f64, out: &mut Vec<Option<f64>>| {
        axis.sample(image, offset, (0., 1.), step, out);
    };
    let lag = f64_isize(((length / 250.).max(0.5) / step).ceil());
    // Two shifts in an irrational ratio (a periodic texture can repeat at one of them), and
    // a tenth of the length: printed bars are taller than that on at least one side. Rows
    // of stacked symbols and DataBar Expanded are only a few dozen modules tall, so shifts
    // of one to two and a half narrow elements are also tried: a two-dimensional texture
    // changes within its own cells.
    let shift = (length / 40.).max(2.5);
    let sets = [
        [shift, shift * 1.618, (length / 10.).max(shift * 2.6)],
        [module, module * 1.618, module * 2.5],
    ];
    let height = (q[3][0] - q[0][0])
        .hypot(q[3][1] - q[0][1])
        .min((q[2][0] - q[1][0]).hypot(q[2][1] - q[1][1]));
    let fans = [
        Some([
            axis.angle_to_along([q[3][0] - q[0][0], q[3][1] - q[0][1]]),
            axis.angle_to_along([q[2][0] - q[1][0], q[2][1] - q[1][1]]),
        ]),
        bar_fan(image, axis, height),
    ];
    let consistent = sets.iter().any(|distances| {
        if distances[0] < 1. {
            return false;
        }
        let mut shifted: Vec<(f64, Vec<Option<f64>>)> = Vec::new();
        for distance in distances {
            for direction in [-1., 1.] {
                let mut line = Vec::new();
                span(direction * distance, &mut line);
                shifted.push((direction * distance, line));
            }
        }
        // The decoder's box may not follow slanted bars, which then shift sideways in
        // proportion to the distance. A texture matches at no slant up to `SLANT` on all
        // distances at once.
        consistent_shifts(center, &shifted, SLANT, lag, step)
            // Perspective turns the bars along the symbol, and a sheared symbol's read band
            // can cross its bars obliquely. Follow the bar direction that the read's sides
            // give at both ends, and the one its own pixels show (a texture without one
            // direction, such as a checkerboard, shows none).
            || fans
                .iter()
                .flatten()
                .filter(|fan| fan[0].abs().max(fan[1].abs()) >= FAN_MIN)
                .any(|&fan| {
                    for (offset, line) in &mut shifted {
                        axis.sample_fan(image, *offset, fan, step, line);
                    }
                    consistent_shifts(center, &shifted, FAN_SLANT, lag, step)
                })
    });
    // The read line of a stacked symbol can run along the separator between two rows; the
    // bars of either row are then constant along themselves beside the line.
    consistent
        || (module >= 1.
            && [-1., 1.].into_iter().any(|side| {
                let mut lines: Vec<(f64, Vec<Option<f64>>)> = Vec::new();
                for factor in [1.618, 2.5, 4.] {
                    let mut line = Vec::new();
                    span(side * factor * module, &mut line);
                    lines.push((side * factor * module, line));
                }
                let reference = lines.remove(0);
                let shifted: Vec<(f64, Vec<Option<f64>>)> = lines
                    .into_iter()
                    .flat_map(|(offset, line)| {
                        let relative = offset - reference.0;
                        [(relative, line.clone()), (relative, line)]
                    })
                    .collect();
                consistent_shifts(&reference.1, &shifted, SLANT, lag, step)
            }))
}

/// True when a linear read is a texture: its pattern continues through both quiet zones
/// at the interior's activity, and either changes along the bars (a two-dimensional
/// pattern such as a checkerboard; see [`constant_along_bars`]) or repeats similar widths
/// run after run on the read line and beside it (stripes, rings and zone plates). Every
/// printed linear symbol is constant along its bars and mixes narrow and wide elements.
pub(crate) fn texture_read(image: Image<'_>, q: &Quad) -> bool {
    if !measurable(image, q) {
        return false;
    }
    let axis = Axis::of(q);
    let length = axis.length;
    if length < 16. {
        return false;
    }
    let step = STEP.max(length / usize_f64(MAX_SAMPLES));
    let mut samples = Vec::new();
    let span = |from: f64, to: f64, offset: f64, out: &mut Vec<Option<f64>>| {
        axis.sample(image, offset, (from, to), step, out);
    };
    span(0.05, 0.95, 0., &mut samples);
    let Some(interior) = activity(&samples).filter(|a| *a > 0.) else {
        return false;
    };
    span(-0.12, -0.02, 0., &mut samples);
    let left = activity(&samples);
    span(1.02, 1.12, 0., &mut samples);
    let right = activity(&samples);
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    if left.min(right) < BUSY_MARGINS * interior {
        return false;
    }
    let mut center = Vec::new();
    span(0., 1., 0., &mut center);
    let profile: Vec<f64> = center.iter().flatten().copied().collect();
    let mut widths = Vec::new();
    let window = f64_usize((length / 30. / step).max(4.));
    runs(&profile, window, &mut widths);
    let module = narrow_width(&widths) * step;
    if !constant_along_bars(image, &axis, q, &center, module, step) {
        return true;
    }
    let regular = |widths: &[f64]| {
        widths.len() >= 8
            && usize_f64(
                widths
                    .windows(2)
                    .filter(|p| p[0].max(p[1]) <= 1.35 * p[0].min(p[1]))
                    .count(),
            ) >= REGULAR_RUNS * usize_f64(widths.len() - 1)
    };
    if !regular(&widths) {
        return false;
    }
    // The separator of a stacked symbol alternates single modules; the rows beside it do not.
    module < 1.
        || [-1., 1.].into_iter().all(|side| {
            span(0., 1., side * 2.5 * module, &mut samples);
            let profile: Vec<f64> = samples.iter().flatten().copied().collect();
            let mut beside = Vec::new();
            runs(&profile, window, &mut beside);
            beside.len() < 8 || regular(&beside)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use barcode_research_core::numeric::f64_u8;

    fn render(width: usize, height: usize, pixel: impl Fn(f64, f64) -> f64) -> Vec<u8> {
        (0..width * height)
            .map(|i| f64_u8(pixel(usize_f64(i % width), usize_f64(i / width)).clamp(0., 255.)))
            .collect()
    }

    fn view(data: &[u8], width: usize, height: usize) -> Image<'_> {
        Image {
            data,
            width,
            height,
            channels: 1,
            stride: width,
        }
    }

    fn checkerboard(cell: usize) -> impl Fn(f64, f64) -> f64 {
        move |x, y| {
            if (f64_usize(x) / cell + f64_usize(y) / cell).is_multiple_of(2) {
                0.
            } else {
                255.
            }
        }
    }

    fn rings(center: [f64; 2], period: f64) -> impl Fn(f64, f64) -> f64 {
        move |x, y| {
            if ((x - center[0]).hypot(y - center[1]) / period).fract() < 0.5 {
                0.
            } else {
                255.
            }
        }
    }

    /// An EAN-13 symbol (`4006381333931`) with 3-pixel modules and full quiet zones; returns
    /// the pixels and the symbol's polygon.
    fn ean13() -> (Vec<u8>, usize, usize, Quad) {
        let bits = barcode_research_core::ean::encode(&[4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1]);
        let (width, height, left, top) = (400, 200, 50., 40.);
        let data = render(width, height, |x, y| {
            let module = ((x - left) / 3.).floor();
            if (top..top + 120.).contains(&y)
                && (0. ..95.).contains(&module)
                && bits[f64_usize(module)] > 0.5
            {
                20.
            } else {
                235.
            }
        });
        let quad = [
            [left, top],
            [left + 285., top],
            [left + 285., top + 120.],
            [left, top + 120.],
        ];
        (data, width, height, quad)
    }

    #[test]
    fn huge_and_non_finite_boxes_are_kept_untested() {
        // Such boxes once sampled up to usize::MAX points (1e9-pixel boxes took seconds).
        let data = render(640, 480, checkerboard(5));
        let image = view(&data, 640, 480);
        let start = std::time::Instant::now();
        for size in [1e5, 1e9, 1e300, f64::INFINITY, f64::NAN] {
            let quads = [
                [[0., 0.], [size, 0.], [size, 100.], [0., 100.]],
                [[0., 0.], [100., 0.], [100., size], [0., size]],
                [[-size, -size], [size, -size], [size, size], [-size, size]],
            ];
            for quad in quads {
                assert!(!texture_proposal(image, &quad), "{quad:?}");
                assert!(!texture_read(image, &quad), "{quad:?}");
            }
        }
        // A box of four image diagonals is still tested, with bounded work.
        let long = [[-1000., 200.], [2200., 200.], [2200., 280.], [-1000., 280.]];
        let _ = (texture_proposal(image, &long), texture_read(image, &long));
        assert!(start.elapsed().as_secs_f64() < 2., "{:?}", start.elapsed());
    }

    #[test]
    fn checkerboard_false_reads_are_textures() {
        // High read EAN-13 and EAN-8 along these slanted lines of 5- and 2-pixel
        // checkerboards (shifted by whole periods).
        let data = render(640, 480, checkerboard(5));
        let quad = [[134., 137.], [490., 320.], [490., 327.], [134., 143.]];
        assert!(texture_read(view(&data, 640, 480), &quad));
        let data = render(300, 200, checkerboard(2));
        let quad = [[68., 42.], [146., 78.], [123., 97.], [50., 63.]];
        assert!(texture_read(view(&data, 300, 200), &quad));
    }

    #[test]
    fn zone_plate_false_read_is_a_texture() {
        // High read DataBar along this vertical line of a 1080p zone plate.
        let data = render(1920, 1080, |x, y| {
            127.5 + 127.5 * (((x - 960.).powi(2) + (y - 540.).powi(2)) / 2000.).cos()
        });
        let quad = [[1481., 246.], [1481., 824.], [1475., 824.], [1475., 246.]];
        assert!(texture_read(view(&data, 1920, 1080), &quad));
    }

    #[test]
    fn printed_symbols_are_not_textures() {
        let (data, width, height, quad) = ean13();
        let image = view(&data, width, height);
        assert!(!texture_read(image, &quad));
        assert!(!texture_proposal(image, &quad));
        // A loose proposal with margins and a thin read band are not textures either.
        let wide = [[20., 30.], [370., 30.], [370., 170.], [20., 170.]];
        assert!(!texture_proposal(image, &wide));
        let band = [[50., 98.], [335., 98.], [335., 102.], [50., 102.]];
        assert!(!texture_read(image, &band));
    }

    #[test]
    fn bars_redrawn_every_few_rows_are_incoherent() {
        // Bar widths 1 to 4 pixels from a fixed sequence, shifted every 8 rows.
        let data = render(600, 200, |x, y| {
            let band = f64_usize(y) / 8;
            let k = (f64_usize(x) + band * 7919) * 2_654_435_761 % 1_000_003;
            if (k / 3 + f64_usize(x) / (1 + band % 4)).is_multiple_of(2) {
                0.
            } else {
                255.
            }
        });
        let quad = [[20., 40.], [580., 40.], [580., 120.], [20., 120.]];
        assert!(texture_proposal(view(&data, 600, 200), &quad));
    }

    #[test]
    fn stripe_and_ring_proposals_are_periodic() {
        let data = render(800, 600, |x, y| {
            if ((x * 0.866 + y * 0.5) / 7.).fract() < 0.4 {
                0.
            } else {
                255.
            }
        });
        let quad = [[100., 100.], [446., 300.], [396., 387.], [50., 187.]];
        assert!(texture_proposal(view(&data, 800, 600), &quad));
        let data = render(800, 600, rings([400., 300.], 10.));
        // A box across the rings beside their center, and one through the center.
        let beside = [[450., 250.], [750., 250.], [750., 350.], [450., 350.]];
        let through = [[50., 250.], [750., 250.], [750., 350.], [50., 350.]];
        let image = view(&data, 800, 600);
        assert!(texture_proposal(image, &beside));
        assert!(texture_proposal(image, &through));
    }

    /// Modules of the EAN-13 symbol `4006381333931` (1 for a bar).
    fn ean_bits() -> [f32; 95] {
        barcode_research_core::ean::encode(&[4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1])
    }

    /// Luminance of a symbol module.
    fn module(bits: &[f32], index: f64) -> Option<f64> {
        (0. ..95.).contains(&index).then(|| {
            if bits[f64_usize(index)] > 0.5 {
                20.
            } else {
                235.
            }
        })
    }

    /// Deterministic per-pixel noise, for surroundings without quiet zones.
    fn noise(x: f64, y: f64) -> f64 {
        let k = f64_usize(x)
            .wrapping_mul(7919)
            .wrapping_add(f64_usize(y).wrapping_mul(104_729))
            .wrapping_mul(2_654_435_761)
            % 1_000_003;
        usize_f64(k % 256)
    }

    #[test]
    fn fanned_and_sheared_reads_are_not_textures() {
        let bits = ean_bits();
        // Bars converging to a point 350 pixels above the read band (22 degrees at both
        // ends), in noise without quiet zones (a Smart Engines crop lost its EAN-13 so).
        let data = render(600, 300, |x, y| {
            let u = 300. + (x - 300.) * 350. / (y + 200.);
            (40. ..260.)
                .contains(&y)
                .then(|| module(&bits, ((u - 157.) / 3.).floor()))
                .flatten()
                .unwrap_or_else(|| noise(x, y))
        });
        let band = [[157., 147.], [442., 147.], [442., 153.], [157., 153.]];
        let image = view(&data, 600, 300);
        let fan = bar_fan(image, &Axis::of(&band), 6.).unwrap();
        assert!(
            fan[0] * fan[1] < 0. && fan[0].abs().min(fan[1].abs()) > 0.3,
            "{fan:?}"
        );
        assert!(!texture_read(image, &band));
        // Vertical bars of a symbol sheared along a 45-degree read band (a DataBar Expanded
        // atlas image).
        let data = render(400, 400, |x, y| {
            ((y - (400. - x)).abs() < 40.)
                .then(|| module(&bits, ((x - 60.) / 3.).floor()))
                .flatten()
                .unwrap_or_else(|| noise(x, y))
        });
        let band = [[60., 340.], [345., 55.], [347.8, 57.8], [62.8, 342.8]];
        assert!(!texture_read(view(&data, 400, 400), &band));
    }

    #[test]
    fn converging_bars_in_a_tall_box_are_coherent() {
        // The bars turn by 20 degrees along the box: their displacement between the outer
        // lines changes by 5 pixels from window to window.
        let bits = ean_bits();
        let data = render(640, 480, |x, y| {
            let u = 320. + (x - 320.) * 1040. / (y + 800.);
            (20. ..460.)
                .contains(&y)
                .then(|| module(&bits, ((u - 130.) / 4.).floor()))
                .flatten()
                .unwrap_or(235.)
        });
        let quad = [[110., 20.], [530., 20.], [530., 460.], [110., 460.]];
        assert!(!texture_proposal(view(&data, 640, 480), &quad));
        // Bars slanted by 20 degrees in either direction against the box.
        for slant in [-0.36, 0.36] {
            let data = render(640, 480, |x, y| {
                (20. ..460.)
                    .contains(&y)
                    .then(|| module(&bits, ((x + slant * (y - 240.) - 130.) / 4.).floor()))
                    .flatten()
                    .unwrap_or(235.)
            });
            assert!(!texture_proposal(view(&data, 640, 480), &quad), "{slant}");
        }
    }

    #[test]
    fn symbol_beside_a_periodic_background_is_kept() {
        // A box on a ribbed carton whose bottom rows cross the symbol.
        let bits = ean_bits();
        let ribs = |x: f64| if (x / 10.).fract() < 0.4 { 60. } else { 200. };
        let with = render(600, 160, |x, y| {
            (50. ..140.)
                .contains(&y)
                .then(|| module(&bits, ((x - 150.) / 3.).floor()))
                .flatten()
                .unwrap_or_else(|| ribs(x))
        });
        let without = render(600, 160, |x, _| ribs(x));
        let quad = [[0., 0.], [599., 0.], [599., 64.], [0., 64.]];
        assert!(!texture_proposal(view(&with, 600, 160), &quad));
        assert!(texture_proposal(view(&without, 600, 160), &quad));
    }

    #[test]
    fn fine_rings_and_stripes_are_textures() {
        // Pixel quantization hides the repetition of 4-pixel rings and of 3-pixel stripes
        // at 7 degrees from their run widths.
        let data = render(800, 600, rings([400., 300.], 4.));
        let beside = [[450., 250.], [750., 250.], [750., 350.], [450., 350.]];
        assert!(texture_proposal(view(&data, 800, 600), &beside));
        let (sin, cos) = 7_f64.to_radians().sin_cos();
        let data = render(800, 600, |x, y| {
            if ((x * cos + y * sin) / 3.).fract() < 0.5 {
                0.
            } else {
                255.
            }
        });
        let image = view(&data, 800, 600);
        let across = [[100., 100.], [700., 100.], [700., 180.], [100., 180.]];
        assert!(texture_proposal(image, &across));
        // A box along the stripes follows their moire.
        let along = [[300., 50.], [300., 550.], [380., 550.], [380., 50.]];
        assert!(texture_proposal(image, &along));
    }

    #[test]
    fn long_box_on_bars_redrawn_every_16_rows_is_incoherent() {
        // Each window of such a long box matches some displacement by chance.
        let data = render(1100, 200, |x, y| {
            let band = f64_usize(y) / 16;
            let k = (f64_usize(x) + band * 7919) * 2_654_435_761 % 1_000_003;
            if (k / 3 + f64_usize(x) / (1 + band % 4)).is_multiple_of(2) {
                0.
            } else {
                255.
            }
        });
        let quad = [[40., 60.], [1060., 60.], [1060., 124.], [40., 124.]];
        assert!(texture_proposal(view(&data, 1100, 200), &quad));
    }

    /// Element widths, in modules, of a deterministic symbol-like sequence.
    fn elements(count: usize, seed: usize) -> Vec<usize> {
        (0..count)
            .map(|i| 1 + (i * 7919 + seed * 104_729) * 2_654_435_761 % 1_000_003 % 4)
            .collect()
    }

    /// Whether the column `x` lies in a dark element of `elements` drawn from `left` with
    /// `module`-pixel modules (`None` outside them).
    fn dark(elements: &[usize], left: f64, module: f64, x: f64) -> Option<bool> {
        let mut edge = left;
        for (i, &e) in elements.iter().enumerate() {
            let next = edge + usize_f64(e) * module;
            if (edge..next).contains(&x) {
                return Some(i % 2 == 0);
            }
            edge = next;
        }
        None
    }

    #[test]
    fn short_and_stacked_reads_are_not_textures() {
        // DataBar Expanded is 34 modules tall over hundreds of modules, without quiet zones:
        // lines a tenth of its length from the read line leave the symbol.
        let row = elements(120, 1);
        let shade = |d: Option<bool>| d.map(|d| if d { 20. } else { 235. });
        let data = render(600, 200, |x, y| {
            (66. ..134.)
                .contains(&y)
                .then(|| shade(dark(&row, 60., 2., x)))
                .flatten()
                .unwrap_or_else(|| noise(x, y))
        });
        let band = [[60., 98.], [540., 98.], [540., 102.], [60., 102.]];
        assert!(!texture_read(view(&data, 600, 200), &band));
        // A stacked symbol read along the separator of single modules between its rows.
        let below = elements(120, 2);
        let data = render(600, 200, |x, y| {
            let symbol = if (40. ..96.).contains(&y) {
                dark(&row, 60., 4., x)
            } else if (96. ..100.).contains(&y) {
                ((60. ..540.).contains(&x)).then(|| f64_usize(x / 4.).is_multiple_of(2))
            } else if (100. ..156.).contains(&y) {
                dark(&below, 60., 4., x)
            } else {
                None
            };
            shade(symbol).unwrap_or_else(|| noise(x, y))
        });
        let band = [[60., 95.], [540., 95.], [540., 101.], [60., 101.]];
        assert!(!texture_read(view(&data, 600, 200), &band));
    }

    #[test]
    fn symbol_among_parallel_stripes_is_kept() {
        // A two-width symbol (as ITF) in the middle of a frame-sized box on stripes parallel
        // to its bars: its lines repeat except where they cross the symbol.
        let widths: Vec<usize> = elements(48, 3).iter().map(|&e| 1 + 2 * (e % 2)).collect();
        let stripes = |x: f64| if (x / 14.).fract() < 0.5 { 40. } else { 210. };
        let with = render(1280, 960, |x, y| {
            let symbol = (380. ..580.)
                .contains(&y)
                .then(|| dark(&widths, 500., 4., x));
            match symbol.flatten() {
                Some(true) => 20.,
                Some(false) => 235.,
                None => stripes(x),
            }
        });
        let without = render(1280, 960, |x, _| stripes(x));
        let quad = [[-90., -70.], [1370., -70.], [1370., 1030.], [-90., 1030.]];
        assert!(!texture_proposal(view(&with, 1280, 960), &quad));
        assert!(texture_proposal(view(&without, 1280, 960), &quad));
    }
}
