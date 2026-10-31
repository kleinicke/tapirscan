//! `MicroPDF417` sampled-row decoder. Image localization is a separate stage.
//! Standard row addresses constrain size and cluster before correction is tried.
use crate::{micro_pdf417_tables as table, pdf417, qr::Payload, reed_prime};

/// Decode one module per column and one sample per logical row, without quiet zones.
/// Only the 34 standard dimensions are accepted. No geometry is inferred here.
#[must_use]
pub fn decode_matrix(matrix: &[bool], width: usize, height: usize) -> Option<Payload> {
    let variant = (0..34).find(|&i| {
        let columns = table::VARIANTS[i];
        height == table::VARIANTS[i + 34]
            && width == 21 + 17 * columns + usize::from(columns >= 3) * 10
    })?;
    if matrix.len() != width * height {
        return None;
    }
    let columns = table::VARIANTS[variant];
    let ecc = table::VARIANTS[variant + 68];
    let mut words = Vec::with_capacity(columns * height);
    let mut erasures = Vec::new();
    for (row, modules) in matrix.chunks_exact(width).enumerate() {
        let read = |start: usize, count: usize| {
            modules[start..start + count]
                .iter()
                .fold(0usize, |v, &b| (v << 1) | usize::from(b))
        };
        let side = |offset: usize| table::SIDE[(table::RAP_START[variant + offset] - 1 + row) % 52];
        if read(0, 10) != side(0) || read(width - 11, 10) != side(68) || !modules[width - 1] {
            return None;
        }
        let cluster = (table::RAP_START[variant + 102] / 3 + row) % 3;
        let mut x = 10;
        for col in 0..columns {
            if (columns == 3 && col == 1) || (columns == 4 && col == 2) {
                let address = (table::RAP_START[variant + 34] - 1 + row) % 52;
                if read(x, 10) != table::CENTRE[address] {
                    return None;
                }
                x += 10;
            }
            let pattern = u32::try_from(read(x, 17)).ok()?;
            if let Some((value, found_cluster)) = pdf417::lookup(pattern)
                && found_cluster == cluster
            {
                words.push(value);
            } else {
                erasures.push(words.len());
                words.push(0);
            }
            x += 17;
        }
    }
    // Keep two checkwords as independent confirmation when cells are erased.
    if erasures.len() + 2 > ecc {
        return None;
    }
    let corrected = reed_prime::correct(&mut words, ecc, &erasures)?;
    let mut payload = pdf417::micro_payload(&words[..words.len() - ecc])?;
    payload.corrected = corrected;
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_dimensions_and_blank_grids_are_rejected() {
        for (w, h) in [(0, 0), (usize::MAX, 11), (38, 11), (99, 44)] {
            assert!(decode_matrix(&[], w, h).is_none());
        }
        for i in 0..34 {
            let columns = table::VARIANTS[i];
            let w = 21 + 17 * columns + usize::from(columns >= 3) * 10;
            let h = table::VARIANTS[i + 34];
            assert!(decode_matrix(&vec![false; w * h], w, h).is_none());
        }
    }
}
