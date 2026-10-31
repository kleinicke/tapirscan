//! Rectangular Micro QR sampled-grid decoding for all 32 sizes, M/H correction.
//! This module does not locate symbols in images or guess a sampling transform.
use crate::{qr, reed_solomon, rmqr_tables as table};

fn format_position(bit: usize, width: usize, height: usize, right: bool) -> (usize, usize) {
    if right {
        if bit < 15 {
            (width - 8 + bit / 5, height - 6 + bit % 5)
        } else {
            (width - 5 + bit - 15, height - 6)
        }
    } else if bit < 15 {
        (8 + bit / 5, 1 + bit % 5)
    } else {
        (11, 1 + bit - 15)
    }
}

fn format(matrix: &[bool], w: usize, h: usize) -> Option<usize> {
    let mut decoded = [None; 2];
    for (copy, codes) in [table::FORMAT_INFO_LEFT, table::FORMAT_INFO_RIGHT]
        .iter()
        .enumerate()
    {
        let word = (0..18).fold(0u32, |value, bit| {
            let (x, y) = format_position(bit, w, h, copy == 1);
            value | (u32::from(matrix[y * w + x]) << bit)
        });
        let mut best = (0, 19);
        let mut tied = false;
        for (index, &code) in codes.iter().enumerate() {
            let error = (word ^ code).count_ones();
            if error < best.1 {
                best = (index, error);
                tied = false;
            } else if error == best.1 {
                tied = true;
            }
        }
        if best.1 <= 3 && !tied {
            decoded[copy] = Some(best.0);
        }
    }
    match decoded {
        [Some(a), Some(b)] if a == b => Some(a),
        [Some(a), None] | [None, Some(a)] => Some(a),
        _ => None,
    }
}

fn reserved(w: usize, h: usize) -> Vec<bool> {
    let mut mask = vec![false; w * h];
    let mut reserve = |x: usize, y: usize| mask[y * w + x] = true;
    for x in 0..w {
        reserve(x, 0);
        reserve(x, h - 1);
    }
    for y in 0..h {
        reserve(0, y);
        reserve(w - 1, y);
    }
    for y in 0..h.min(8) {
        for x in 0..8 {
            reserve(x, y);
        }
    }
    for y in h - 5..h {
        for x in w - 5..w {
            reserve(x, y);
        }
    }
    reserve(1, h - 2);
    reserve(w - 2, 1);
    if let Some(group) = table::WIDTH[..5].iter().position(|&width| width == w) {
        for &x in &table::TABLE_D1[group * 4..group * 4 + 4] {
            if x == 0 {
                continue;
            }
            for y in 0..h {
                reserve(x, y);
            }
            for y in [1, 2, h - 3, h - 2] {
                reserve(x - 1, y);
                reserve(x + 1, y);
            }
        }
    }
    for right in [false, true] {
        for bit in 0..18 {
            let (x, y) = format_position(bit, w, h, right);
            reserve(x, y);
        }
    }
    mask
}

fn codewords(matrix: &[bool], w: usize, h: usize, count: usize) -> Option<Vec<u8>> {
    let mask = reserved(w, h);
    let mut bytes = vec![0; count];
    let mut at = 0;
    let mut up = true;
    for right in (1..w - 1).rev().step_by(2) {
        for offset in 0..h {
            let y = if up { h - 1 - offset } else { offset };
            for x in [right, right - 1] {
                if mask[y * w + x] {
                    continue;
                }
                if at < count * 8 {
                    let value = matrix[y * w + x] ^ (y / 2 + x / 3).is_multiple_of(2);
                    bytes[at / 8] |= u8::from(value) << (7 - at % 8);
                }
                at += 1;
            }
        }
        up = !up;
    }
    (at >= count * 8 && at < count * 8 + 8).then_some(bytes)
}

fn correct(stream: &[u8], data_count: usize, block_count: usize) -> Option<(Vec<u8>, usize)> {
    let ecc = (stream.len() - data_count) / block_count;
    if !(stream.len() - data_count).is_multiple_of(block_count) {
        return None;
    }
    let short = data_count / block_count;
    let short_count = block_count - data_count % block_count;
    let sizes: Vec<_> = (0..block_count)
        .map(|i| short + usize::from(i >= short_count))
        .collect();
    let mut blocks: Vec<_> = sizes.iter().map(|&n| vec![0; n + ecc]).collect();
    let mut at = 0;
    for col in 0..=short {
        for (block, &n) in blocks.iter_mut().zip(&sizes) {
            if col < n {
                block[col] = *stream.get(at)?;
                at += 1;
            }
        }
    }
    for col in 0..ecc {
        for (block, &n) in blocks.iter_mut().zip(&sizes) {
            block[n + col] = *stream.get(at)?;
            at += 1;
        }
    }
    let mut data = Vec::with_capacity(data_count);
    let mut corrected = 0;
    let field = reed_solomon::gf256(0x11d);
    for (block, &n) in blocks.iter_mut().zip(&sizes) {
        corrected += field.correct(block, ecc, 0)?;
        data.extend_from_slice(&block[..n]);
    }
    Some((data, corrected))
}

/// Decode an upright module grid without quiet zones. Dimensions and format
/// information must agree; ambiguous format copies are rejected.
#[must_use]
pub fn decode_matrix(matrix: &[bool], width: usize, height: usize) -> Option<qr::Payload> {
    let version = (0..32).find(|&i| table::WIDTH[i] == width && table::HEIGHT[i] == height)?;
    if matrix.len() != width * height {
        return None;
    }
    let info = format(matrix, width, height)?;
    if info % 32 != version {
        return None;
    }
    let stream = codewords(matrix, width, height, table::TOTAL_CODEWORDS[version])?;
    let (data, corrected) = correct(&stream, table::DATA_CODEWORDS[info], table::BLOCKS[info])?;
    let mut payload = qr::parse_rmqr(&data, version)?;
    payload.corrected = corrected;
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_has_the_standard_capacity() {
        for i in 0..32 {
            let (w, h) = (table::WIDTH[i], table::HEIGHT[i]);
            assert!(
                codewords(&vec![false; w * h], w, h, table::TOTAL_CODEWORDS[i]).is_some(),
                "{w}x{h}"
            );
            assert!(decode_matrix(&vec![false; w * h], w, h).is_none());
        }
    }

    #[test]
    fn malformed_dimensions_never_index_or_allocate() {
        for (w, h) in [(0, 0), (usize::MAX, 7), (43, 7), (43, usize::MAX)] {
            assert!(decode_matrix(&[], w, h).is_none());
        }
    }
}
