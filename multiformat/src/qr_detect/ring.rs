//! Isolated QR outer-ring square hypothesis.
//! This geometry is only a proposal; independent QR ECC remains authoritative.

#[expect(
    clippy::too_many_lines,
    reason = "The bounded flood-fill and moment calculation are one ordered geometric hypothesis."
)]
pub fn ring_square_quad(
    image: &[bool],
    w: usize,
    h: usize,
    center: [f32; 2],
    approximate_frame: [f32; 6],
) -> Option<[[f32; 2]; 4]> {
    if w == 0 || h == 0 || image.len() < w.checked_mul(h)? || !center.iter().all(|v| v.is_finite())
    {
        return None;
    }
    if !approximate_frame.iter().all(|v| v.is_finite()) {
        return None;
    }
    let pitch = approximate_frame[0]
        .hypot(approximate_frame[1])
        .max(approximate_frame[2].hypot(approximate_frame[3]));
    if !pitch.is_finite() || pitch <= 0. {
        return None;
    }
    let width = isize::try_from(w).ok()?;
    let height = isize::try_from(h).ok()?;
    let radius = (pitch * 5.).ceil() + 3.;
    #[expect(
        clippy::cast_precision_loss,
        reason = "The image dimensions are compared in the f32 geometry domain used by the detector."
    )]
    let minimum_dimension = w.min(h) as f32;
    if !radius.is_finite() || radius > minimum_dimension {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "The finite positive radius is bounded by the image dimensions before this geometric integer conversion."
    )]
    let radius = isize::try_from(radius as usize).ok()?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Finite f32 finder geometry is intentionally floored into the signed pixel-coordinate domain before checked image-bound comparisons."
    )]
    let (cx, cy) = (center[0].floor() as isize, center[1].floor() as isize);
    let left = cx.checked_sub(radius)?;
    let right = cx.checked_add(radius)?;
    let top = cy.checked_sub(radius)?;
    let bottom = cy.checked_add(radius)?;
    if left < 0 || top < 0 || right >= width || bottom >= height {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "This positive value is the component's intentional hard pixel-count limit; capacity is capped separately below."
    )]
    let max_count = (40. * pitch * pitch).ceil() as usize;
    let side = usize::try_from(radius.checked_mul(2)?.checked_add(1)?).ok()?;
    let neighborhood = side.checked_mul(side)?;
    let ex = [approximate_frame[0], approximate_frame[1]];
    let ey = [approximate_frame[2], approximate_frame[3]];
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Finite f32 seed geometry is intentionally floored into the signed pixel-coordinate domain and checked against the local bounds before indexing."
    )]
    let seeds = [[0., -3.], [3., 0.], [0., 3.], [-3., 0.]].map(|[sx, sy]| {
        let seed_x = (center[0] + sx * ex[0] + sy * ey[0]).floor() as isize;
        let seed_y = (center[1] + sx * ex[1] + sy * ey[1]).floor() as isize;
        (seed_x, seed_y)
    });
    let mut points = None;
    for &(seed_x, seed_y) in &seeds {
        if seed_x < left || seed_x > right || seed_y < top || seed_y > bottom {
            continue;
        }
        let mut stack = vec![(seed_x, seed_y)];
        let mut seen = vec![false; neighborhood];
        let mut component = Vec::with_capacity(max_count.min(4096));
        while let Some((x, y)) = stack.pop() {
            // The stack only receives points inside [left..=right] x [top..=bottom],
            // so these subtractions and products are bounded by `neighborhood`.
            let local_y = usize::try_from(y - top).ok()?;
            let local_x = usize::try_from(x - left).ok()?;
            let local = local_y * side + local_x;
            if seen[local] {
                continue;
            }
            seen[local] = true;
            // The ring bounds are inside the image and `image.len() >= w * h`,
            // established above, so this row-major index is representable.
            let image_x = usize::try_from(x).ok()?;
            let image_y = usize::try_from(y).ok()?;
            let image_index = image_y * w + image_x;
            if !image[image_index] {
                continue;
            }
            if x == left || x == right || y == top || y == bottom {
                component.clear();
                break;
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "Bounded integer pixel coordinates are promoted to the f64 moment domain used for the ring fit."
            )]
            let point = [x as f64 + 0.5, y as f64 + 0.5];
            component.push(point);
            if component.len() > max_count {
                component.clear();
                break;
            }
            if x > left {
                stack.push((x - 1, y));
            }
            if x < right {
                stack.push((x + 1, y));
            }
            if y > top {
                stack.push((x, y - 1));
            }
            if y < bottom {
                stack.push((x, y + 1));
            }
        }
        if component.is_empty() {
            continue;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "The bounded component count is intentionally compared with f64 geometric area estimates."
        )]
        let count = component.len() as f64;
        let pitch64 = f64::from(pitch);
        if !(12. * pitch64 * pitch64..=40. * pitch64 * pitch64).contains(&count) {
            continue;
        }
        let centroid = [
            component.iter().map(|p| p[0]).sum::<f64>() / count,
            component.iter().map(|p| p[1]).sum::<f64>() / count,
        ];
        if (centroid[0] - f64::from(center[0])).hypot(centroid[1] - f64::from(center[1])) > pitch64
        {
            continue;
        }
        points = Some(component);
        break;
    }
    let points = points?;
    let count = points.len();
    let pitch = f64::from(pitch);
    #[expect(
        clippy::cast_precision_loss,
        reason = "The bounded component count is intentionally compared with f64 geometric area estimates."
    )]
    let count_f64 = count as f64;
    if !(12. * pitch * pitch..=40. * pitch * pitch).contains(&count_f64) {
        return None;
    }
    let centroid = [
        points.iter().map(|p| p[0]).sum::<f64>() / count_f64,
        points.iter().map(|p| p[1]).sum::<f64>() / count_f64,
    ];
    let (mut real, mut imag) = (0., 0.);
    for [x, y] in points {
        let dx = x - centroid[0];
        let dy = y - centroid[1];
        real += dx.powi(4) - 6. * dx * dx * dy * dy + dy.powi(4);
        imag += 4. * dx * dy * (dx * dx - dy * dy);
    }
    let mut theta = (imag.atan2(real) + std::f64::consts::PI) * 0.25;
    theta %= std::f64::consts::FRAC_PI_2;
    let module = (count_f64 / 24.).sqrt();
    let (sin, cos) = theta.sin_cos();
    let corners = [[-3.5, -3.5], [3.5, -3.5], [3.5, 3.5], [-3.5, 3.5]];
    Some(corners.map(|[x, y]| {
        let coordinates = [
            centroid[0] + module * (x * cos - y * sin),
            centroid[1] + module * (x * sin + y * cos),
        ];
        let to_f32 = |coordinate: f64| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "The public quad uses f32 image coordinates; the computed geometry is finite before conversion."
            )]
            let coordinate = coordinate as f32;
            coordinate
        };
        [to_f32(coordinates[0]), to_f32(coordinates[1])]
    }))
}

#[cfg(test)]
mod tests {
    use super::ring_square_quad;

    fn raster(rotation: f32, pitch: f32) -> (Vec<bool>, usize, usize, [f32; 2], [f32; 6]) {
        let w = 160;
        let h = 160;
        let center = [80., 80.];
        let (sin, cos) = rotation.to_radians().sin_cos();
        let frame = [
            pitch * cos,
            pitch * sin,
            -pitch * sin,
            pitch * cos,
            center[0],
            center[1],
        ];
        let image = (0..w * h)
            .map(|i| {
                let x = i % w;
                let y = i / w;
                let dx = f32::from(u16::try_from(x).unwrap()) + 0.5 - center[0];
                let dy = f32::from(u16::try_from(y).unwrap()) + 0.5 - center[1];
                let ux = (dx * cos + dy * sin) / pitch;
                let uy = (-dx * sin + dy * cos) / pitch;
                ux.abs() <= 3.5 && uy.abs() <= 3.5 && ux.abs().max(uy.abs()) >= 2.5
            })
            .collect();
        (image, w, h, center, frame)
    }

    #[test]
    fn rasterized_rotations_recover_center_scale_and_square_angle() {
        for rotation in [0., 25., 90., 130.] {
            for pitch in [4., 10.] {
                let (image, w, h, center, frame) = raster(rotation, pitch);
                let quad = ring_square_quad(&image, w, h, center, frame).unwrap();
                let got_center = quad.iter().fold([0., 0.], |mut sum, p| {
                    sum[0] += p[0] * 0.25;
                    sum[1] += p[1] * 0.25;
                    sum
                });
                assert!((got_center[0] - center[0]).abs() < 0.6);
                assert!((got_center[1] - center[1]).abs() < 0.6);
                let side = (quad[1][0] - quad[0][0]).hypot(quad[1][1] - quad[0][1]);
                assert!((side / 7. - pitch).abs() < pitch * 0.05);
                let mut angle = (quad[1][1] - quad[0][1])
                    .atan2(quad[1][0] - quad[0][0])
                    .rem_euclid(std::f32::consts::FRAC_PI_2);
                let expected = rotation
                    .to_radians()
                    .rem_euclid(std::f32::consts::FRAC_PI_2);
                angle = (angle - expected)
                    .abs()
                    .min(std::f32::consts::FRAC_PI_2 - (angle - expected).abs());
                assert!(angle < 0.03);
            }
        }
    }

    #[test]
    fn invalid_or_out_of_frame_inputs_are_rejected_without_indexing() {
        let (image, w, h, center, frame) = raster(0., 4.);
        assert!(ring_square_quad(&image[..image.len() - 1], w, h, center, frame).is_none());
        assert!(ring_square_quad(&image, w, h, [0., 0.], frame).is_none());
        assert!(ring_square_quad(&image, w, h, [f32::MAX, 80.], frame).is_none());
        assert!(ring_square_quad(&image, w, h, [f32::MIN, 80.], frame).is_none());
    }
}
