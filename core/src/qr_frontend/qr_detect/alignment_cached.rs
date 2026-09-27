//! Exact separable sampling tables for QR alignment searches.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Matches the original bounded alignment sampler's f32 coordinates and saturating index casts."
)]
pub(super) fn search(
    image: &[bool],
    w: usize,
    h: usize,
    guess: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
) -> Vec<[f32; 2]> {
    let module = (ex[0].hypot(ex[1]) + ey[0].hypot(ey[1])) * 0.5;
    let range = (module * 8.).ceil() as isize;
    let step = (module * 0.4).round().max(1.) as usize;
    let offsets: Vec<_> = (-range..=range).step_by(step).collect();
    let centers: Vec<_> = offsets
        .iter()
        .map(|&offset| [guess[0] + offset as f32, guess[1] + offset as f32])
        .collect();
    let expected: [bool; 25] = std::array::from_fn(|i| {
        let x = (i % 5) as i32 - 2;
        let y = (i / 5) as i32 - 2;
        x.abs().max(y.abs()) != 1
    });
    let x_samples: Vec<[isize; 25]> = centers
        .iter()
        .map(|center| {
            std::array::from_fn(|i| {
                let x = (i % 5) as i32 - 2;
                let y = (i / 5) as i32 - 2;
                (center[0] + x as f32 * ex[0] + y as f32 * ey[0]).floor() as isize
            })
        })
        .collect();
    let y_samples: Vec<[isize; 25]> = centers
        .iter()
        .map(|center| {
            std::array::from_fn(|i| {
                let x = (i % 5) as i32 - 2;
                let y = (i / 5) as i32 - 2;
                (center[1] + x as f32 * ex[1] + y as f32 * ey[1]).floor() as isize
            })
        })
        .collect();
    let mut candidates = Vec::new();
    for (yi, &dy) in offsets.iter().enumerate() {
        'locations: for (xi, &dx) in offsets.iter().enumerate() {
            let mut errors = 0u8;
            for i in 0..25 {
                let xx = x_samples[xi][i];
                let yy = y_samples[yi][i];
                if xx < 0
                    || yy < 0
                    || xx >= w.cast_signed()
                    || yy >= h.cast_signed()
                    || image[yy.cast_unsigned() * w + xx.cast_unsigned()] != expected[i]
                {
                    errors += 1;
                    if errors > 3 {
                        continue 'locations;
                    }
                }
            }
            let d = (dx * dx + dy * dy) as f32 / module.powi(2);
            candidates.push((
                f32::from(errors) * 3. + d * 0.04,
                [centers[xi][0], centers[yi][1]],
            ));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<[f32; 2]> = Vec::new();
    for (_, p) in candidates {
        if out
            .iter()
            .all(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > module)
        {
            out.push(p);
        }
        if out.len() == 4 {
            break;
        }
    }
    out
}
