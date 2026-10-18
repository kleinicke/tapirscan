//! Periodic-texture evidence from original pixels.
//!
//! Stripes, rings, checkerboards and similar repeating patterns contain many parallel
//! edges, so the localizers box them and the retail decoders search them for symbols.
//! Every linear symbology mixes element widths and needs quiet zones, which such textures
//! lack. These tests sample source luminance along a box's scan axis and recognize two
//! cases: a profile whose bar and space widths all repeat (no symbol can be read there),
//! and a read whose pattern continues through both quiet zones without staying constant
//! along its bars.
use crate::{Image, Quad};
use barcode_research_core::numeric::{f64_isize, f64_usize, usize_f64};

/// Distance between profile samples along the scan axis, in pixels.
const STEP: f64 = 0.5;
/// Longest sampled profile; longer axes are sampled more coarsely.
const MAX_SAMPLES: usize = 8192;

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

/// Correlation along the bars at which a part of a box holds printed bars.
const COHERENT_WINDOW: f64 = 0.5;

/// True when a localized box lies on a texture that holds no linear symbol. Three lines
/// across the box (its center and a quarter of its height to either side) are sampled once:
///
/// - Periodic: the runs repeat (see [`repeating`]) on at least two lines. No linear
///   symbology encodes data that way; one line may cross the center of rings or the edge of
///   the texture.
/// - Incoherent: the two outer lines correlate in fewer than two sixths of the axis, each
///   aligned at its own small lateral displacement. Printed bars run through the whole box,
///   while text, noise and bar patterns that change every few rows do not.
pub(crate) fn texture_proposal(image: Image<'_>, q: &Quad) -> bool {
    let axis = Axis::of(q);
    let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    if axis.length < 16. {
        return false;
    }
    let step = STEP.max(axis.length / usize_f64(MAX_SAMPLES));
    let window = f64_usize((axis.length / 24. / step).max(6.));
    let mut lines: [Vec<Option<f64>>; 3] = Default::default();
    let mut profile = Vec::new();
    let mut widths = Vec::new();
    let mut repeating_lines = 0;
    let mut edged_outer = 0;
    for (line, offset) in lines.iter_mut().zip([0., -0.25, 0.25]) {
        axis.sample(image, offset * height, (0., 1.), step, line);
        profile.clear();
        profile.extend(line.iter().flatten());
        let first_dark = runs(&profile, window, &mut widths);
        repeating_lines += usize::from(repeating(&widths, first_dark, JITTER / step));
        // Flat or nearly flat outer lines give no evidence of incoherence.
        edged_outer += usize::from(offset != 0. && widths.len() >= PERIODIC_RUNS);
    }
    if repeating_lines >= 2 {
        return true;
    }
    if height < 8. || edged_outer < 2 {
        return false;
    }
    let [_, above, below] = &lines;
    // Perspective and shear move the bars between the lines differently along the axis:
    // align each sixth of the axis on its own, up to 15 degrees of bar slant. Lags beyond
    // one sample are tried every second sample, nearest first, until one correlates.
    let lag = f64_isize(((axis.length / 200.).max(1.5).max(0.13 * height) / step).ceil());
    let lags = || {
        [0, -1, 1]
            .into_iter()
            .chain((2..=lag).step_by(2).flat_map(|l| [-l, l]))
    };
    let part = above.len().div_ceil(6).max(16);
    let mut coherent = 0;
    let mut windows = 0;
    for start in (0..above.len()).step_by(part) {
        let end = (start + part).min(above.len());
        let mut measured = false;
        for l in lags() {
            if let Some(c) = correlation(&above[start..end], below, start, l) {
                measured = true;
                if c >= COHERENT_WINDOW {
                    coherent += 1;
                    break;
                }
            }
        }
        windows += usize::from(measured);
        if coherent == 2 {
            return false;
        }
    }
    windows >= 3
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
/// Correlation along the bars below which the pattern is two-dimensional.
const INCONSISTENT_BARS: f64 = 0.35;
/// Share of adjacent runs with similar widths at which the profile is a smooth
/// periodic or chirped texture.
const REGULAR_RUNS: f64 = 0.85;

/// True when a linear read is a texture: its pattern continues through both quiet zones
/// at the interior's activity, and either changes along the bars (a two-dimensional
/// pattern such as a checkerboard) or repeats similar widths run after run (stripes,
/// rings and zone plates). Every printed linear symbol is constant along its bars and
/// mixes narrow and wide elements.
pub(crate) fn texture_read(image: Image<'_>, q: &Quad) -> bool {
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
    // Two shifts in an irrational ratio (a periodic texture can repeat at one of them), and
    // a tenth of the length: printed bars are taller than that on at least one side.
    let shift = (length / 40.).max(2.5);
    let lag = f64_isize(((length / 250.).max(0.5) / step).ceil());
    let mut shifted = Vec::new();
    for distance in [shift, shift * 1.618, (length / 10.).max(shift * 2.6)] {
        // Lines outside the image or on flat surroundings give no evidence.
        let mut best: Option<f64> = None;
        for direction in [-1., 1.] {
            span(0., 1., direction * distance, &mut shifted);
            for l in -lag..=lag {
                if let Some(c) = correlation(&center, &shifted, 0, l) {
                    best = Some(best.map_or(c, |b| b.max(c)));
                }
            }
        }
        if best.is_some_and(|b| b < INCONSISTENT_BARS) {
            return true;
        }
    }
    let profile: Vec<f64> = center.iter().flatten().copied().collect();
    let mut widths = Vec::new();
    runs(
        &profile,
        f64_usize((length / 30. / step).max(4.)),
        &mut widths,
    );
    if widths.len() < 8 {
        return false;
    }
    let similar = widths
        .windows(2)
        .filter(|p| p[0].max(p[1]) <= 1.35 * p[0].min(p[1]))
        .count();
    usize_f64(similar) >= REGULAR_RUNS * usize_f64(widths.len() - 1)
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
}
