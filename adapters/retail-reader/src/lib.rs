pub mod linear;
pub mod databar;
pub mod expanded;
pub mod numeric;
mod retail_gray;
mod retail_profile;
/// One symbol in a structured-append sequence; index is one-based. Assembly is
/// deliberately left to callers because distinct frames can contain repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredAppend {
    pub index: usize,
    pub count: usize,
    pub id: Option<String>,
    pub parity: Option<u8>,
}
#[derive(Clone)]
pub struct Detection {
    /// Decoded payload bytes before character-set interpretation, when available.
    pub bytes: Option<Vec<u8>>,
    pub structured_append: Option<StructuredAppend>,
    pub reader_initialization: bool,
    pub addon: Option<String>,
    pub format: String,
    pub text: String,
    pub polygon: [[f32; 2]; 4],
    pub support: usize,
    pub error: f32,
    pub gs1: bool,
}
/// Bounded access to the unchanged EAN8 blurred-intensity matcher.
/// # Panics
/// Internal run boundaries must remain consistent with their cumulative edges.
#[must_use]
pub fn recovery_gray(row: &[u8], limit: usize) -> (Vec<(String, f64, f64, f32)>, usize) {
    if !(64..=4096).contains(&row.len()) || limit == 0 {
        return (Vec::new(), 0);
    }
    let (bits, mut widths, _) = retail_profile::runs(row);
    if bits.is_empty() || !(45..=512).contains(&widths.len()) {
        return (Vec::new(), 0);
    }
    let mut edges = vec![0f64];
    for w in &widths {
        edges.push(edges.last().unwrap() + f64::from(*w));
    }
    let mut out = Vec::new();
    let mut calls = 0;
    let mut limited = false;
    for reverse in [false, true] {
        if reverse {
            widths.reverse();
        }
        let dark = if reverse {
            *bits.last().unwrap()
        } else {
            bits[0]
        };
        for mut read in linear::decode_candidates(&widths, dark, linear::EAN8, &mut limited) {
            if !read.decoded {
                if calls >= limit {
                    break;
                }
                calls += 1;
                if let Some((text, error)) = retail_gray::decode(row, &widths, read.start, reverse)
                {
                    read.text = text;
                    read.error = error;
                    read.decoded = true;
                }
            }
            let (a, b) = if reverse {
                (widths.len() - read.end, widths.len() - read.start)
            } else {
                (read.start, read.end)
            };
            if read.decoded
                && read.error <= 0.14
                && read.text.len() == 8
                && a < b
                && b < edges.len()
                && out.len() < 64
            {
                out.push((read.text, edges[a] - 0.5, edges[b] - 0.5, read.error));
            }
        }
    }
    (out, calls)
}
