//! Bounded contrast restoration before independent QR recovery.
/// Separable binomial smoothing, then a bounded unsharp mask. Border samples
/// replicate the nearest source pixel; all arithmetic is integer-exact.
///
/// # Panics
///
/// Panics if `gray` is shorter than `width * height`.
#[must_use]
pub fn sharpen(gray: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut horizontal = vec![0_u16; width * height];
    for y in 0..height {
        let row = &gray[y * width..(y + 1) * width];
        for x in 0..width {
            horizontal[y * width + x] = u16::from(row[x.saturating_sub(2)])
                + u16::from(row[x.saturating_sub(1)]) * 4
                + u16::from(row[x]) * 6
                + u16::from(row[(x + 1).min(width - 1)]) * 4
                + u16::from(row[(x + 2).min(width - 1)]);
        }
    }
    let mut output = vec![0_u8; width * height];
    for y in 0..height {
        let rows = [
            y.saturating_sub(2),
            y.saturating_sub(1),
            y,
            (y + 1).min(height - 1),
            (y + 2).min(height - 1),
        ];
        for x in 0..width {
            let smooth = u32::from(horizontal[rows[0] * width + x])
                + u32::from(horizontal[rows[1] * width + x]) * 4
                + u32::from(horizontal[rows[2] * width + x]) * 6
                + u32::from(horizontal[rows[3] * width + x]) * 4
                + u32::from(horizontal[rows[4] * width + x]);
            let center = i32::from(gray[y * width + x]);
            let blur = i32::try_from((smooth + 128) / 256).expect("pixel fits");
            output[y * width + x] =
                u8::try_from((center + 2 * (center - blur)).clamp(0, 255)).expect("clamped");
        }
    }
    output
}
