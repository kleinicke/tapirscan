//! Experimental bounded timing-guided nonplanar sampling; not promoted.
use super::map;

fn shifted(
    transform: &[f32; 8],
    size: usize,
    x: f32,
    y: f32,
    axis: usize,
    amplitude: f32,
    crease: f32,
) -> [f32; 2] {
    let size = crate::numeric::usize_f32(size);
    let fraction = ((if axis == 0 { x } else { y }) - 3.5) / (size - 7.);
    let basis = if fraction < crease {
        fraction / crease
    } else {
        (1. - fraction) / (1. - crease)
    };
    if axis == 0 {
        map(transform, x, y + amplitude * basis)
    } else {
        map(transform, x + amplitude * basis, y)
    }
}
fn pixel(image: &[bool], width: usize, height: usize, point: [f32; 2]) -> Option<bool> {
    if !point[0].is_finite() || !point[1].is_finite() {
        return None;
    }
    let x = crate::numeric::f32_isize(point[0].floor());
    let y = crate::numeric::f32_isize(point[1].floor());
    (x >= 0 && y >= 0 && x < width.cast_signed() && y < height.cast_signed())
        .then(|| image[y.cast_unsigned() * width + x.cast_unsigned()])
}
pub(super) fn recover(
    image: &[bool],
    width: usize,
    height: usize,
    size: usize,
    transform: &[f32; 8],
) -> Option<crate::qr::Payload> {
    if size < 25 {
        return None;
    }
    let mut hypotheses = Vec::new();
    let span = size - 16;
    for axis in 0..2 {
        let score = |amplitude, crease| {
            (8..size - 8)
                .filter(|&i| {
                    let point = crate::numeric::usize_f32(i) + 0.5;
                    let (x, y) = if axis == 0 {
                        (point, 6.5)
                    } else {
                        (6.5, point)
                    };
                    pixel(
                        image,
                        width,
                        height,
                        shifted(transform, size, x, y, axis, amplitude, crease),
                    ) == Some(i % 2 == 0)
                })
                .count()
        };
        let plain = score(0., 0.5);
        if plain * 10 >= span * 9 {
            continue;
        }
        let range = i32::try_from(size / 3).expect("QR dimensions fit i32");
        for crease in [0.35, 0.5, 0.65] {
            for half in -range..=range {
                if half == 0 {
                    continue;
                }
                let amplitude = crate::numeric::f64_f32(f64::from(half)) * 0.5;
                let found = score(amplitude, crease);
                if found * 100 >= span * 85 && found >= plain + 2 {
                    hypotheses.push((found, axis, amplitude, crease));
                }
            }
        }
    }
    hypotheses.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.abs().total_cmp(&b.2.abs())));
    for (_, axis, amplitude, crease) in hypotheses.into_iter().take(6) {
        if !crate::qr::plausible_image_header(size, |x, y| {
            pixel(
                image,
                width,
                height,
                shifted(
                    transform,
                    size,
                    crate::numeric::usize_f32(x) + 0.5,
                    crate::numeric::usize_f32(y) + 0.5,
                    axis,
                    amplitude,
                    crease,
                ),
            )
        }) {
            continue;
        }
        let mut matrix = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                matrix.push(pixel(
                    image,
                    width,
                    height,
                    shifted(
                        transform,
                        size,
                        crate::numeric::usize_f32(x) + 0.5,
                        crate::numeric::usize_f32(y) + 0.5,
                        axis,
                        amplitude,
                        crease,
                    ),
                )?);
            }
        }
        if let Some(read) = crate::qr::decode_matrix(&matrix, size) {
            return Some(read);
        }
        let transposed = (0..size * size)
            .map(|i| matrix[(i % size) * size + i / size])
            .collect::<Vec<_>>();
        if let Some(read) = crate::qr::decode_matrix(&transposed, size) {
            return Some(read);
        }
    }
    None
}
