//! Explicit numeric semantics used by scanner geometry and diagnostic tools.
//!
//! Geometry stays in its original floating-point representation. Float-to-integer
//! conversions truncate toward zero and saturate (NaN becomes zero), as required
//! by Rust's cast semantics. Integer extraction keeps low bits where requested.
//! These functions do not add input limits, validation failures or rounding steps.

/// Round a usize coordinate/count to the existing f64 representation.
#[inline]
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Scanner geometry and scores intentionally round integer coordinates; preserving the existing IEEE conversion avoids changing sampling."
)]
pub const fn usize_f64(value: usize) -> f64 {
    value as f64
}

/// Truncate a f64 toward zero and saturate to isize; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f64_isize(value: f64) -> isize {
    value as isize
}

/// Round a isize coordinate/count to the existing f64 representation.
#[inline]
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Scanner geometry and scores intentionally round integer coordinates; preserving the existing IEEE conversion avoids changing sampling."
)]
pub const fn isize_f64(value: isize) -> f64 {
    value as f64
}

/// Truncate a f64 toward zero and saturate to u8; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f64_u8(value: f64) -> u8 {
    value as u8
}

/// Truncate a f32 toward zero and saturate to usize; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f32_usize(value: f32) -> usize {
    value as usize
}

/// Round an f64 geometry value to f32 using the hardware IEEE conversion.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "The scanner stores geometry in f32; this explicit rounding boundary preserves the original precision and overflow behavior."
)]
pub const fn f64_f32(value: f64) -> f32 {
    value as f32
}

/// Extract the low 32 bits of a word without changing wraparound semantics.
#[inline]
#[must_use]
pub const fn usize_i32(value: usize) -> i32 {
    let bytes = value.to_le_bytes();
    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Sign-extend an offset and reinterpret it as an unsigned pointer-sized word.
#[inline]
#[must_use]
#[expect(
    clippy::cast_sign_loss,
    reason = "Offset arithmetic intentionally preserves the two's-complement wrapping representation."
)]
pub const fn i32_usize(value: i32) -> usize {
    value as usize
}

/// Round a usize coordinate/count to the existing f32 representation.
#[inline]
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Scanner geometry and scores intentionally round integer coordinates; preserving the existing IEEE conversion avoids changing sampling."
)]
pub const fn usize_f32(value: usize) -> f32 {
    value as f32
}

/// Truncate a f64 toward zero and saturate to u32; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f64_u32(value: f64) -> u32 {
    value as u32
}

/// Extract the low 32 bits of a word without changing wraparound semantics.
#[inline]
#[must_use]
pub const fn usize_u32(value: usize) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Truncate a f64 toward zero and saturate to usize; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f64_usize(value: f64) -> usize {
    value as usize
}

/// Truncate a f64 toward zero and saturate to i32; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub const fn f64_i32(value: f64) -> i32 {
    value as i32
}

/// Round a u64 coordinate/count to the existing f32 representation.
#[inline]
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Scanner geometry and scores intentionally round integer coordinates; preserving the existing IEEE conversion avoids changing sampling."
)]
pub const fn u64_f32(value: u64) -> f32 {
    value as f32
}

/// Truncate a f64 toward zero and saturate to i64; NaN maps to zero.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Sampling uses Rust float-to-integer saturation and truncation; negatives, infinities and NaN retain their established conversion behavior."
)]
pub fn f64_i64(value: f64) -> i64 {
    value as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_indices_truncate_and_saturate() {
        assert_eq!(f32_usize(-3.75), 0);
        assert_eq!(f32_usize(f32::NAN), 0);
        assert_eq!(f32_usize(f32::NEG_INFINITY), 0);
        assert_eq!(f32_usize(f32::INFINITY), usize::MAX);
        assert_eq!(f32_usize(3.99), 3);
    }

    #[test]
    fn coordinate_rounding_uses_nearest_even_and_preserves_signed_zero() {
        assert_eq!(usize_f32(16_777_217).to_bits(), 16_777_216f32.to_bits());
        assert_eq!(usize_f32(16_777_219).to_bits(), 16_777_220f32.to_bits());
        assert_eq!(f64_f32(-0.).to_bits(), (-0f32).to_bits());
        assert!(f64_f32(f64::MAX).is_infinite());
        assert!(f64_f32(f64::NAN).is_nan());
    }

    #[test]
    fn signed_offsets_keep_their_wrapping_word_representation() {
        assert_eq!(i32_usize(-1), usize::MAX);
        assert_eq!(i32_usize(-2).wrapping_add(2), 0);
        assert_eq!(i32_usize(i32::MIN).wrapping_add(2_147_483_648), 0);
    }

    #[test]
    fn byte_samples_saturate_instead_of_wrapping() {
        assert_eq!(f64_u8(-1.), 0);
        assert_eq!(f64_u8(255.99), 255);
        assert_eq!(f64_u8(256.), 255);
        assert_eq!(f64_u8(f64::NAN), 0);
        assert_eq!(usize_i32(0xffff_ffff), -1);
        assert_eq!(f64_i32(-3.99), -3);
    }
}
