//! Bounded contrast restoration before independent QR recovery.
/// Separable binomial smoothing, then a bounded unsharp mask. Border samples
/// replicate the nearest source pixel; all arithmetic is integer-exact.
pub(crate) fn sharpen(gray: &[u8], width: usize, height: usize) -> Vec<u8> {
    let weights = [1_u32, 4, 6, 4, 1];
    let mut horizontal = vec![0_u32; width * height];
    for y in 0..height {
        for x in 0..width {
            horizontal[y * width + x] = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| {
                    let xx = x.saturating_add(i).saturating_sub(2).min(width - 1);
                    u32::from(gray[y * width + xx]) * weight
                })
                .sum();
        }
    }
    let mut output = vec![0_u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let smooth: u32 = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| {
                    let yy = y.saturating_add(i).saturating_sub(2).min(height - 1);
                    horizontal[yy * width + x] * weight
                })
                .sum();
            let center = i32::from(gray[y * width + x]);
            let restored = center * 3
                - i32::try_from((smooth + 128) / 256).expect("smoothed pixel fits i32") * 2;
            output[y * width + x] =
                u8::try_from(restored.clamp(0, 255)).expect("clamped pixel fits u8");
        }
    }
    output
}
