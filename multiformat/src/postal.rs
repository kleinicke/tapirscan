//! Height-modulated postal codes. One run of the four-state bar reader per
//! threshold image feeds every selected format's decoder, so each further
//! postal format costs only its decoder on the rows found.
use crate::formats::{AUSTRALIA_POST, INTELLIGENT_MAIL, JAPAN_POST, KIX, RM4SCC};
use crate::four_state::{self, Bar, Row};
use crate::{
    Detection, australia_post, binarization, intelligent_mail, japan_post, kix, regions, rm4scc,
};

/// Every postal format read by this module.
pub const MASK: u64 = RM4SCC | KIX | AUSTRALIA_POST | JAPAN_POST | INTELLIGENT_MAIL;

struct Format {
    bit: u64,
    name: &'static str,
    /// Text and the row in its reading direction.
    read: fn(&Row) -> Option<(String, Row)>,
    /// Whether bars in this reading direction carry the format's fixed frame;
    /// such rows become regions when they fail to decode. `None` for formats
    /// without a frame.
    framed: Option<fn(&[Bar]) -> bool>,
}

/// Decoders in priority order. The formats have disjoint bar counts, except
/// that RM4SCC's frame and check character are tried before KIX's bare
/// characters.
const FORMATS: [Format; 5] = [
    Format {
        bit: RM4SCC,
        name: "RM4SCC",
        read: |row| either(row, rm4scc::decode),
        framed: Some(rm4scc::framed),
    },
    Format {
        bit: AUSTRALIA_POST,
        name: "AustraliaPost",
        read: |row| either(row, australia_post::decode),
        framed: Some(australia_post::framed),
    },
    Format {
        bit: JAPAN_POST,
        name: "JapanPost",
        read: |row| either(row, japan_post::decode),
        framed: Some(japan_post::framed),
    },
    Format {
        bit: INTELLIGENT_MAIL,
        name: "IntelligentMail",
        read: |row| either(row, intelligent_mail::decode),
        framed: Some(intelligent_mail::framed),
    },
    Format {
        bit: KIX,
        name: "KIX",
        read: kix::read,
        framed: None,
    },
];

/// The first of `row` and its upside-down reading that `decode` accepts.
fn either(row: &Row, decode: fn(&[Bar]) -> Option<String>) -> Option<(String, Row)> {
    if let Some(text) = decode(&row.bars) {
        return Some((text, row.clone()));
    }
    let rotated = row.rotated();
    decode(&rotated.bars).map(|text| (text, rotated))
}

#[derive(Clone, Copy)]
enum Pass {
    /// A cached threshold image.
    Mode(usize),
    /// The local threshold restricted to pixels darker than this radius's mean.
    Fine(usize),
}

/// The postal formats of `mask` read in a threshold image. Rows that carry a
/// selected format's frame but fail to decode become undecoded regions.
pub fn detect(
    image: &[u8],
    [width, height]: [usize; 2],
    effort: usize,
    mask: u64,
    regions: &mut regions::Regions,
    binary_images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let selected: Vec<&Format> = FORMATS.iter().filter(|f| mask & f.bit != 0).collect();
    let mut reads: Vec<Detection> = Vec::new();
    let mut limited = false;
    // The local threshold separates thin bars and the global one recovers large
    // symbols whose bars are wider than the local window. When neither reads,
    // windows about a module across separate blurred bars.
    let passes: &[Pass] = match effort {
        0 => &[Pass::Mode(1)],
        1 => &[Pass::Mode(1), Pass::Mode(0), Pass::Fine(3)],
        _ => &[
            Pass::Mode(1),
            Pass::Mode(0),
            Pass::Fine(2),
            Pass::Fine(4),
            Pass::Fine(7),
        ],
    };
    for &pass in passes {
        let (rows, stopped) = match pass {
            Pass::Mode(0) if binary_images.is_duplicate(1) => continue,
            Pass::Mode(mode) => four_state::rows(binary_images.get(mode), width, height),
            Pass::Fine(_) if !reads.is_empty() => break,
            Pass::Fine(radius) => {
                let bits = four_state::fine_threshold(
                    &image[..width * height],
                    binary_images.get(1),
                    width,
                    height,
                    radius,
                );
                four_state::rows(&bits, width, height)
            }
        };
        limited |= stopped;
        for row in rows {
            let read = selected
                .iter()
                .find_map(|f| (f.read)(&row).map(|(text, row)| (f.name, text, row)));
            let Some((format, text, row)) = read else {
                if let Some(f) = selected.iter().find(|f| {
                    f.framed
                        .is_some_and(|framed| framed(&row.bars) || framed(&row.rotated().bars))
                }) {
                    regions.add(f.name, row.polygon, 0.5, row.bars.len());
                }
                continue;
            };
            // A part of a longer symbol can frame and check by chance; keep the
            // read with more bars where two overlap.
            let support = row.bars.len();
            if let Some(at) = reads
                .iter()
                .position(|r| regions::overlap(&r.polygon, &row.polygon) > 0.5)
            {
                if reads[at].support >= support {
                    continue;
                }
                reads.remove(at);
            }
            reads.push(Detection {
                assembled_rows: false,
                bytes: Some(text.clone().into_bytes()),
                structured_append: None,
                reader_initialization: false,
                addon: None,
                codabar_start_stop: None,
                format: format.into(),
                text,
                polygon: row.polygon,
                support,
                error: 0.,
                gs1: false,
            });
        }
    }
    (reads, limited)
}

#[cfg(test)]
#[must_use]
pub fn scan(gray: &[u8], width: usize, height: usize, effort: usize, mask: u64) -> Vec<String> {
    let mut images = binarization::Images::new(gray, width, height);
    let mut regions = regions::Regions::default();
    let (reads, _) = detect(
        gray,
        [width, height],
        effort,
        mask,
        &mut regions,
        &mut images,
    );
    reads.into_iter().map(|r| r.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{render, zint_bars};

    #[test]
    fn each_format_reads_only_when_selected() {
        let symbols = [
            // zint --dump --barcode=KIX -d 2500GG30250
            (
                KIX,
                [
                    "0A 0A 0A 0A 28 28 0A 0A 0A 0A 0A",
                    "AA AA AA AA AA AA AA AA AA AA AA",
                    "28 A0 0A 0A 88 88 82 0A 28 A0 0A",
                ],
                "2500GG30250",
            ),
            // zint --dump --barcode=AUSPOST -d 39987520
            (
                AUSTRALIA_POST,
                [
                    "8A AA 22 02 88 A0 88 8A 22 0",
                    "AA AA AA AA AA AA AA AA AA 8",
                    "08 82 22 A8 2A A0 A2 02 28 0",
                ],
                "1139987520",
            ),
        ];
        for (bit, dump, text) in symbols {
            let (gray, width, height) = render(&zint_bars(dump), 3., 12., 0);
            assert_eq!(scan(&gray, width, height, 1, bit), [text]);
            assert_eq!(scan(&gray, width, height, 1, MASK), [text]);
            assert_eq!(scan(&gray, width, height, 1, MASK & !bit), [""; 0]);
        }
    }
}
