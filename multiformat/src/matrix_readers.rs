//! Shared matrix-reader invocation. Callers own ordering, concurrency and finalization.
use crate::formats::{
    AZTEC, DATA_MATRIX, MAXI_CODE, MICRO_PDF417, MICRO_QR_CODE, PDF417, QR_CODE, RM_QR_CODE,
};
use crate::{
    Detection, aztec_detect, binarization, dm_detect, maxicode_detect, pdf417, qr_detect, regions,
};

/// One matrix reader. The QR reader reads QR Code, Micro QR or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    QrCode(qr_detect::Symbols),
    DataMatrix,
    Aztec,
    MaxiCode,
    Pdf417,
    MicroPdf417,
    RmQrCode,
}

impl Reader {
    /// This reader restricted to the formats of `mask`, or `None` when it reads none
    /// of them.
    #[must_use]
    pub const fn select(self, mask: u32) -> Option<Self> {
        let bit = match self {
            Self::QrCode(_) => {
                let symbols = qr_detect::Symbols {
                    qr: mask & QR_CODE != 0,
                    micro: mask & MICRO_QR_CODE != 0,
                };
                return if symbols.qr || symbols.micro {
                    Some(Self::QrCode(symbols))
                } else {
                    None
                };
            }
            Self::DataMatrix => DATA_MATRIX,
            Self::Aztec => AZTEC,
            Self::MaxiCode => MAXI_CODE,
            Self::Pdf417 => PDF417,
            Self::MicroPdf417 => MICRO_PDF417,
            Self::RmQrCode => RM_QR_CODE,
        };
        if mask & bit == 0 { None } else { Some(self) }
    }

    /// The reader's format name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::QrCode(_) => "QRCode",
            Self::DataMatrix => "DataMatrix",
            Self::Aztec => "Aztec",
            Self::MaxiCode => "MaxiCode",
            Self::Pdf417 => "PDF417",
            Self::MicroPdf417 => "MicroPDF417",
            Self::RmQrCode => "RMQRCode",
        }
    }
}

/// Fixed reader order, also used when merging concurrent reader results. Restrict
/// each entry to the selected formats with [`Reader::select`].
pub const ORDER: [Reader; 7] = [
    Reader::QrCode(qr_detect::Symbols {
        qr: true,
        micro: true,
    }),
    Reader::DataMatrix,
    Reader::Aztec,
    Reader::MaxiCode,
    Reader::Pdf417,
    Reader::MicroPdf417,
    Reader::RmQrCode,
];

/// Run exactly one reader using the caller's region accumulator and threshold cache.
pub fn read(
    reader: Reader,
    image: &[u8],
    [width, height]: [usize; 2],
    effort: usize,
    mode: usize,
    regions: &mut regions::Regions,
    binary_images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    match reader {
        Reader::QrCode(symbols) => qr_detect::detect_group(
            image,
            width,
            height,
            regions,
            binary_images,
            qr_detect::Policy::for_mode(mode),
            effort,
            symbols,
        ),
        Reader::DataMatrix => {
            dm_detect::detect(width, height, regions, binary_images, effort > 1, mode)
        }
        // Aztec's enlarged retry follows the compiled mode, not group effort.
        Reader::Aztec => aztec_detect::detect(
            width,
            height,
            regions,
            image,
            binary_images,
            aztec_detect::Policy::for_mode(mode),
            mode,
        ),
        Reader::MaxiCode => maxicode_detect::detect(width, height, regions, binary_images),
        Reader::Pdf417 => pdf417::detect(image, width, height, regions, mode),
        Reader::MicroPdf417 => {
            crate::micro_pdf417_detect::detect(image, [width, height], binary_images, regions)
        }
        Reader::RmQrCode => qr_detect::rectangular::detect([width, height], binary_images, regions),
    }
}
