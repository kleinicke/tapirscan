use super::*;

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;

fn frame(pixel: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    (0..WIDTH * HEIGHT)
        .map(|i| pixel(i % WIDTH, i / WIDTH))
        .collect()
}

fn image(data: &[u8]) -> Image<'_> {
    Image {
        data,
        width: WIDTH,
        height: HEIGHT,
        channels: 1,
        stride: WIDTH,
    }
}

fn checkerboard(cell: usize) -> Vec<u8> {
    frame(|x, y| {
        if (x / cell + y / cell).is_multiple_of(2) {
            0
        } else {
            255
        }
    })
}

fn rings(period: f64) -> Vec<u8> {
    use barcode_research_core::numeric::usize_f64;
    frame(|x, y| {
        let r = (usize_f64(x) - 960.).hypot(usize_f64(y) - 540.);
        if (r / period).fract() < 0.5 { 0 } else { 255 }
    })
}

fn texts(data: &[u8], mask: u32) -> Vec<String> {
    crate::Scanner::default()
        .scan_formats_typed_with_addons(
            image(data),
            crate::ScanOptions {
                multiple: true,
                ..crate::ScanOptions::default()
            },
            mask,
            EanAddOnPolicy::Ignore,
        )
        .unwrap()
        .barcodes
        .into_iter()
        .map(|b| format!("{} {}", b.format, b.text))
        .collect()
}

/// High and Very High read EAN-13 from the 5-pixel and EAN-8 from the 2-pixel
/// checkerboard, and `DataBar` from the zone plate, before texture reads were rejected.
#[test]
fn periodic_textures_give_no_linear_reads() {
    for cell in [2, 5] {
        let found = texts(&checkerboard(cell), RETAIL_MASK);
        assert!(found.is_empty(), "{cell}-pixel checkerboard: {found:?}");
    }
    let zone = frame(|x, y| {
        use barcode_research_core::numeric::{f64_u8, usize_f64};
        let r2 = (usize_f64(x) - 960.).powi(2) + (usize_f64(y) - 540.).powi(2);
        f64_u8(127.5 + 127.5 * (r2 / 2000.).cos())
    });
    let found = texts(&zone, DATA_BAR | DATA_BAR_EXPANDED);
    assert!(found.is_empty(), "zone plate: {found:?}");
}

/// Ring boxes once drove every retail retry and crop recovery (about 8 times the work
/// of a photo); none of them may reach decoding.
#[test]
fn ring_frames_keep_no_retail_proposals() {
    for period in [10., 16.] {
        let data = rings(period);
        let result = crate::Scanner::default()
            .scan_with_options(
                image(&data),
                crate::ScanOptions {
                    multiple: true,
                    include_regions: true,
                    ..crate::ScanOptions::default()
                },
            )
            .unwrap();
        let regions = result.regions().unwrap();
        // 10-pixel rings kept 29 to 35 proposals with 2,500 to 6,300 decoding paths before.
        let paths: usize = regions.frame.candidates.iter().map(|c| c.work.paths).sum();
        assert!(
            regions.proposals.len() <= 2,
            "{} proposals",
            regions.proposals.len()
        );
        assert!(paths <= 400, "{paths} decoding paths");
        assert!(result.barcodes().is_empty());
    }
}

/// Bars 1 to 4 pixels wide, drawn anew every 8 rows (a deterministic generator).
fn random_bars(width: usize, height: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut row = vec![255_u8; width];
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        if y % 8 == 0 {
            let (mut x, mut dark) = (0, next() >> 63 == 0);
            while x < width {
                let end = (x + 1 + usize::try_from(next() % 4).unwrap()).min(width);
                row[x..end].fill(if dark { 0 } else { 255 });
                x = end;
                dark = !dark;
            }
        }
        data.extend_from_slice(&row);
    }
    data
}

/// Random bar patterns kept 26 Medium proposals with 1,900 decoding paths at 960x960
/// (about 300 ms) and gave Medium and High a false EAN-8 read.
#[test]
fn random_bar_frames_keep_no_retail_proposals() {
    let (width, height) = (960, 960);
    let data = random_bars(width, height);
    let image = Image {
        data: &data,
        width,
        height,
        channels: 1,
        stride: width,
    };
    let result = crate::Scanner::default()
        .scan_with_options(
            image,
            crate::ScanOptions {
                multiple: true,
                include_regions: true,
                ..crate::ScanOptions::default()
            },
        )
        .unwrap();
    let regions = result.regions().unwrap();
    let paths: usize = regions.frame.candidates.iter().map(|c| c.work.paths).sum();
    assert!(
        regions.proposals.len() <= 2,
        "{} proposals",
        regions.proposals.len()
    );
    assert!(paths <= 600, "{paths} decoding paths");
    assert!(result.barcodes().is_empty());
}
