use barcode_multiformat::{
    numeric::{f32_usize, usize_f32},
    qr_detect,
};
pub(super) fn score(gray: &[u8], w: usize, h: usize, t: &[f32; 8], mode: usize) -> f32 {
    let mut black = 0.;
    let mut white = 0.;
    for y in -4_i16..=4 {
        for x in -4_i16..=4 {
            let [px, py] = qr_detect::map(t, f32::from(x), f32::from(y));
            let sx = px - 0.5;
            let sy = py - 0.5;
            if !sx.is_finite()
                || !sy.is_finite()
                || sx < 0.
                || sy < 0.
                || sx >= usize_f32(w - 1)
                || sy >= usize_f32(h - 1)
            {
                return f32::INFINITY;
            }
            let ix = f32_usize(sx.floor());
            let iy = f32_usize(sy.floor());
            let dx = sx - usize_f32(ix);
            let dy = sy - usize_f32(iy);
            let at = iy * w + ix;
            let value = (f32::from(gray[at]) * (1. - dx) + f32::from(gray[at + 1]) * dx)
                * (1. - dy)
                + (f32::from(gray[at + w]) * (1. - dx) + f32::from(gray[at + w + 1]) * dx) * dy;
            if x.abs().max(y.abs()) % 2 == 0 {
                black += value;
            } else {
                white += value;
            }
        }
    }
    let v = black / 49. - white / 32.;
    if matches!(mode, 2 | 3 | 5 | 7) { -v } else { v }
}
pub(super) fn refine(
    gray: &[u8],
    w: usize,
    h: usize,
    t: [f32; 8],
    module: f32,
    mode: usize,
    budget: &mut usize,
) -> Option<[f32; 8]> {
    let source = [[-4.5, -4.5], [4.5, -4.5], [4.5, 4.5], [-4.5, 4.5]];
    let mut q = source.map(|[x, y]| qr_detect::map(&t, x, y));
    let mut best = t;
    let mut error = score(gray, w, h, &best, mode);
    for scale in [0.3, 0.15, 0.075] {
        for _ in 0..2 {
            let mut changed = false;
            for corner in 0..4 {
                for axis in 0..2 {
                    for sign in [-1., 1.] {
                        if *budget < 81 {
                            return None;
                        }
                        *budget -= 81;
                        let mut trial = q;
                        trial[corner][axis] += module * scale * sign;
                        let Some(t) = qr_detect::homography(source, trial) else {
                            continue;
                        };
                        let value = score(gray, w, h, &t, mode);
                        if value + 0.001 < error {
                            error = value;
                            best = t;
                            q = trial;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    Some(best)
}
