//! Bilinear source-gray samples under a grid projection.
use crate::numeric::{f32_usize, usize_f32};
use crate::qr_detect::map;

/// Bilinear gray value of `gray` at the grid position (`x`, `y`) under the projective
/// `transform`, or `None` off the frame. Pixels are areas centered at half-integer
/// source coordinates; the last row and column extend to the frame edge.
#[must_use]
pub fn bilinear(
    gray: &[u8],
    width: usize,
    height: usize,
    transform: &[f32; 8],
    x: f32,
    y: f32,
) -> Option<f32> {
    let [px, py] = map(transform, x, y);
    let (sx, sy) = (px - 0.5, py - 0.5);
    if !(sx.is_finite() && sy.is_finite())
        || sx < 0.
        || sy < 0.
        || sx > usize_f32(width - 1)
        || sy > usize_f32(height - 1)
    {
        return None;
    }
    let (left, top) = (f32_usize(sx), f32_usize(sy));
    let right = (left + 1).min(width - 1);
    let bottom = (top + 1).min(height - 1);
    let (fx, fy) = (sx - usize_f32(left), sy - usize_f32(top));
    let value = |column: usize, row: usize| f32::from(gray[row * width + column]);
    let upper = value(left, top) * (1. - fx) + value(right, top) * fx;
    let lower = value(left, bottom) * (1. - fx) + value(right, bottom) * fx;
    Some(upper * (1. - fy) + lower * fy)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bilinear_source_samples_preserve_pixel_centers_and_reject_outside_points() {
        let pixels = [0, 10, 20, 30];
        let identity = [1., 0., 0., 0., 1., 0., 0., 0.];
        for (x, y, expected) in [(0.5, 0.5, 0.), (1., 1., 15.), (1.5, 1.5, 30.)] {
            let actual = super::bilinear(&pixels, 2, 2, &identity, x, y).unwrap();
            assert!((actual - expected).abs() < f32::EPSILON);
        }
        for (x, y) in [
            (0.49, 0.5),
            (1.51, 1.5),
            (f32::NAN, 1.),
            (1., f32::INFINITY),
        ] {
            assert!(super::bilinear(&pixels, 2, 2, &identity, x, y).is_none());
        }
    }
}
