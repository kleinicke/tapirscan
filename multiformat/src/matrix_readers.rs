//! Shared matrix-reader invocation. Callers own ordering, concurrency and finalization.
use crate::formats::{AZTEC, DATA_MATRIX, MAXI_CODE, PDF417, QR_CODE};
use crate::{
    Detection, aztec_detect, binarization, dm_detect, maxicode_detect, pdf417, qr_detect, regions,
};

/// Fixed reader order, also used when merging concurrent reader results.
pub const ORDER: [(u32, &str); 5] = [
    (QR_CODE, "QRCode"),
    (DATA_MATRIX, "DataMatrix"),
    (AZTEC, "Aztec"),
    (MAXI_CODE, "MaxiCode"),
    (PDF417, "PDF417"),
];

/// Run exactly one reader using the caller's region accumulator and threshold cache.
///
/// # Panics
/// Panics when `bit` is not one of the single-format bits in `ORDER`.
pub fn read(
    bit: u32,
    image: &[u8],
    [width, height]: [usize; 2],
    effort: usize,
    mode: usize,
    regions: &mut regions::Regions,
    binary_images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    match bit {
        QR_CODE => qr_detect::detect_group(
            image,
            width,
            height,
            regions,
            binary_images,
            qr_detect::Policy::for_mode(mode),
            effort,
        ),
        DATA_MATRIX => dm_detect::detect(width, height, regions, binary_images, effort > 1, mode),
        // Aztec's enlarged retry follows the compiled mode, not group effort.
        AZTEC => aztec_detect::detect(
            width,
            height,
            regions,
            image,
            binary_images,
            aztec_detect::Policy::for_mode(mode),
            mode,
        ),
        MAXI_CODE => maxicode_detect::detect(width, height, regions, binary_images),
        PDF417 => pdf417::detect(image, width, height, regions, mode),
        _ => panic!("expected one matrix-reader format bit"),
    }
}
