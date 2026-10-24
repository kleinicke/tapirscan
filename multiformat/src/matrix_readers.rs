//! Shared matrix-reader invocation. Callers own ordering, concurrency and finalization.
use crate::formats::{AZTEC, DATA_MATRIX, MAXI_CODE, MICRO_QR_CODE, PDF417, QR_CODE};
use crate::{
    Detection, aztec_detect, binarization, dm_detect, maxicode_detect, pdf417, qr_detect, regions,
};

/// Fixed reader order, also used when merging concurrent reader results. QR and
/// Micro QR share one reader, which reads the selected ones of the two.
pub const ORDER: [(u32, &str); 5] = [
    (QR_CODE | MICRO_QR_CODE, "QRCode"),
    (DATA_MATRIX, "DataMatrix"),
    (AZTEC, "Aztec"),
    (MAXI_CODE, "MaxiCode"),
    (PDF417, "PDF417"),
];

/// Run exactly one reader using the caller's region accumulator and threshold cache.
/// `bit` is the reader's entry in `ORDER`, restricted to the selected formats.
///
/// # Panics
/// Panics when `bit` selects no format of exactly one `ORDER` entry.
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
        qr if qr & !(QR_CODE | MICRO_QR_CODE) == 0 => qr_detect::detect_group(
            image,
            width,
            height,
            regions,
            binary_images,
            qr_detect::Policy::for_mode(mode),
            effort,
            qr_detect::Symbols {
                qr: qr & QR_CODE != 0,
                micro: qr & MICRO_QR_CODE != 0,
            },
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
