//! Grayscale crop resampling for the enlarged and reduced matrix retries.
//!
//! Each kernel keeps its own arithmetic: readers were tuned on these exact
//! pixels, so the kernels are not interchangeable approximations of each other.
//! - [`bilinear_enlarge`]: per-pixel bilinear weights in f32, edge-clamped
//!   (Data Matrix small-box retry).
//! - [`catmull_rom_enlarge`]: separable Catmull-Rom with precomputed taps, a
//!   horizontal f32 pass over the touched rows, then a vertical pass (Aztec).
//! - [`sampled_average`]: integer area average from at most four evenly spaced
//!   samples per axis (Data Matrix large-box retry).
use crate::numeric;

/// Bilinear enlargement of the crop `[x0, y0, x1, y1)` by an integer factor.
pub(crate) fn bilinear_enlarge(
    gray: &[u8],
    stride: usize,
    [x0, y0, x1, y1]: [usize; 4],
    factor: usize,
) -> Vec<u8> {
    let (cw, ch) = (x1 - x0, y1 - y0);
    let (out_w, out_h) = (cw * factor, ch * factor);
    let scale = numeric::usize_f32(factor);
    let mut out = vec![0_u8; out_w * out_h];
    for y in 0..out_h {
        let fy =
            ((numeric::usize_f32(y) + 0.5) / scale - 0.5).clamp(0., numeric::usize_f32(ch - 1));
        let (ya, wy) = (numeric::f32_usize(fy.floor()), fy.fract());
        let yb = (ya + 1).min(ch - 1);
        for x in 0..out_w {
            let fx =
                ((numeric::usize_f32(x) + 0.5) / scale - 0.5).clamp(0., numeric::usize_f32(cw - 1));
            let (xa, wx) = (numeric::f32_usize(fx.floor()), fx.fract());
            let xb = (xa + 1).min(cw - 1);
            let at = |row: usize, column: usize| f32::from(gray[(y0 + row) * stride + x0 + column]);
            let top = at(ya, xa) * (1. - wx) + at(ya, xb) * wx;
            let bottom = at(yb, xa) * (1. - wx) + at(yb, xb) * wx;
            out[y * out_w + x] = numeric::f32_u8((top * (1. - wy) + bottom * wy).round());
        }
    }
    out
}

/// Area average of the crop `[x0, y0, x1, y1)` at the given output size, from
/// at most four evenly spaced samples per axis and output pixel.
pub(crate) fn sampled_average(
    gray: &[u8],
    stride: usize,
    [x0, y0, x1, y1]: [usize; 4],
    (out_w, out_h): (usize, usize),
) -> Vec<u8> {
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = vec![0_u8; out_w * out_h];
    for y in 0..out_h {
        let (ya, yb) = (
            y * ch / out_h,
            ((y + 1) * ch / out_h).max(y * ch / out_h + 1).min(ch),
        );
        for x in 0..out_w {
            let (xa, xb) = (
                x * cw / out_w,
                ((x + 1) * cw / out_w).max(x * cw / out_w + 1).min(cw),
            );
            // At most four evenly spaced samples per axis keep the pass cheap.
            let (ys, xs) = ((yb - ya).min(4), (xb - xa).min(4));
            let mut sum = 0_u32;
            for j in 0..ys {
                let row = y0 + ya + (2 * j + 1) * (yb - ya) / (2 * ys);
                for i in 0..xs {
                    let column = x0 + xa + (2 * i + 1) * (xb - xa) / (2 * xs);
                    sum += u32::from(gray[row * stride + column]);
                }
            }
            let count = u32::try_from(ys * xs).unwrap_or(1);
            out[y * out_w + x] = u8::try_from((sum + count / 2) / count).unwrap_or(255);
        }
    }
    out
}

/// Catmull-Rom taps (source indices, weights) for one output coordinate.
fn catmull_rom_taps(
    out: usize,
    origin: usize,
    factor: usize,
    limit: usize,
) -> ([usize; 4], [f32; 4]) {
    let position = (numeric::usize_f32(out) + 0.5) / numeric::usize_f32(factor) - 0.5
        + numeric::usize_f32(origin);
    let floor = position.floor();
    let t = position - floor;
    let base = numeric::f32_isize(floor);
    let last = numeric::usize_f32(limit - 1);
    let index =
        |offset: isize| numeric::f32_usize(numeric::isize_f32(base + offset).clamp(0., last));
    let (t2, t3) = (t * t, t * t * t);
    (
        [index(-1), index(0), index(1), index(2)],
        [
            -0.5 * t3 + t2 - 0.5 * t,
            1.5 * t3 - 2.5 * t2 + 1.,
            -1.5 * t3 + 2. * t2 + 0.5 * t,
            0.5 * t3 - 0.5 * t2,
        ],
    )
}

/// The window `[x0, x0 + width) x [y0, y0 + height)` of a `stride` x `rows`
/// image, enlarged `factor` times with Catmull-Rom weights.
pub(crate) fn catmull_rom_enlarge(
    gray: &[u8],
    stride: usize,
    rows: usize,
    window: [usize; 4],
    factor: usize,
) -> (Vec<u8>, usize, usize) {
    let [x0, y0, width, height] = window;
    let (out_width, out_height) = (width * factor, height * factor);
    let columns: Vec<_> = (0..out_width)
        .map(|x| catmull_rom_taps(x, x0, factor, stride))
        .collect();
    let lines: Vec<_> = (0..out_height)
        .map(|y| catmull_rom_taps(y, y0, factor, rows))
        .collect();
    // Horizontal pass for the source rows the vertical taps touch.
    let first = lines.iter().map(|l| l.0[0]).min().unwrap_or(0);
    let last = lines.iter().map(|l| l.0[3]).max().unwrap_or(0);
    let mut wide = vec![0_f32; (last + 1 - first) * out_width];
    for (row, target) in wide.chunks_exact_mut(out_width).enumerate() {
        let source = &gray[(first + row) * stride..(first + row + 1) * stride];
        for (value, (index, weight)) in target.iter_mut().zip(&columns) {
            *value = (0..4)
                .map(|k| f32::from(source[index[k]]) * weight[k])
                .sum();
        }
    }
    let mut output = vec![0_u8; out_width * out_height];
    for (target, (index, weight)) in output.chunks_exact_mut(out_width).zip(&lines) {
        for (x, value) in target.iter_mut().enumerate() {
            let sum: f32 = (0..4)
                .map(|k| wide[(index[k] - first) * out_width + x] * weight[k])
                .sum();
            *value = numeric::f32_u8(sum.round().clamp(0., 255.));
        }
    }
    (output, out_width, out_height)
}
