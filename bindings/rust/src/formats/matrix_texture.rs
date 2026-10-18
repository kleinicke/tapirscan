//! Low-resolution evidence for a large, dense 2D symbol.
//!
//! A frame is reduced to [`SIDE`] pixels on its long side and split into
//! [`BLOCK`]-pixel blocks. A block looks like matrix modules when it has strong
//! contrast, a balanced dark/light share and on average at least one transition
//! per row and per column. Printed text, product photos and 1D barcodes fail at
//! least one of these tests in most blocks. The evidence is the largest
//! 8-connected group of such blocks with a roughly square bounding box.

/// Long side of the reduced copy, in pixels.
const SIDE: usize = 512;
/// Block side, in pixels of the reduced copy.
const BLOCK: usize = 16;
/// Smallest gray-level range of a module block.
const MIN_CONTRAST: u8 = 80;
/// Smallest share, in percent, of the darker and the lighter half of a block.
const MIN_BALANCE_PERCENT: usize = 35;
/// Smallest short-to-long side ratio, in percent, of a group's bounding box.
const MIN_ASPECT_PERCENT: usize = 40;
/// Smallest group, in ten-thousandths of all blocks of the frame.
const MIN_GROUP_PER_10K: usize = 120;

/// Whether the frame holds a roughly square group of module-like blocks that
/// covers at least [`MIN_GROUP_PER_10K`] of its blocks.
pub(super) fn has_dense_square_texture(pixels: &[u8], width: usize, height: usize) -> bool {
    let long = width.max(height);
    if long < SIDE {
        return true;
    }
    // u64 keeps the products exact where usize is 32 bits (WASM).
    let scale = |value: usize, numerator: usize, denominator: usize| {
        usize::try_from(value as u64 * numerator as u64 / denominator as u64).unwrap_or(usize::MAX)
    };
    let out_width = scale(width, SIDE, long);
    let out_height = scale(height, SIDE, long);
    let (columns, rows) = (out_width / BLOCK, out_height / BLOCK);
    if columns == 0 || rows == 0 {
        return true;
    }
    let x_source: Vec<_> = (0..columns * BLOCK)
        .map(|x| scale(2 * x + 1, width, 2 * out_width).min(width - 1))
        .collect();
    let mut textured = vec![false; columns * rows];
    let mut block = [[0_u8; BLOCK]; BLOCK];
    for by in 0..rows {
        for bx in 0..columns {
            for (j, row) in block.iter_mut().enumerate() {
                let source_y =
                    scale(2 * (by * BLOCK + j) + 1, height, 2 * out_height).min(height - 1);
                let source = &pixels[source_y * width..(source_y + 1) * width];
                for (pixel, &x) in row.iter_mut().zip(&x_source[bx * BLOCK..(bx + 1) * BLOCK]) {
                    *pixel = source[x];
                }
            }
            textured[by * columns + bx] = is_module_block(&block);
        }
    }
    largest_square_group(&textured, columns, rows) * 10_000 >= MIN_GROUP_PER_10K * columns * rows
}

/// Whether one block has the contrast, balance and transitions of matrix modules.
fn is_module_block(block: &[[u8; BLOCK]; BLOCK]) -> bool {
    let (low, high) = block
        .iter()
        .flatten()
        .fold((u8::MAX, 0), |(low, high), &v| (low.min(v), high.max(v)));
    if high - low < MIN_CONTRAST {
        return false;
    }
    let middle = u16::from(low) + u16::from(high - low) / 2;
    let dark = |v: u8| u16::from(v) < middle;
    let dark_count = block.iter().flatten().filter(|&&v| dark(v)).count();
    let total = BLOCK * BLOCK;
    if dark_count * 100 < total * MIN_BALANCE_PERCENT
        || dark_count * 100 > total * (100 - MIN_BALANCE_PERCENT)
    {
        return false;
    }
    let horizontal = block
        .iter()
        .map(|row| row.windows(2).filter(|p| dark(p[0]) != dark(p[1])).count())
        .sum::<usize>();
    let vertical = block
        .iter()
        .zip(&block[1..])
        .map(|(upper, lower)| {
            upper
                .iter()
                .zip(lower)
                .filter(|(a, b)| dark(**a) != dark(**b))
                .count()
        })
        .sum::<usize>();
    horizontal >= BLOCK && vertical >= BLOCK
}

/// Size of the largest 8-connected group of set cells whose bounding box is
/// roughly square.
fn largest_square_group(cells: &[bool], columns: usize, rows: usize) -> usize {
    let mut seen = vec![false; cells.len()];
    let mut stack = Vec::new();
    let mut best = 0;
    for start in 0..cells.len() {
        if !cells[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut count, mut left, mut right, mut top, mut bottom) = (0, columns, 0, rows, 0);
        while let Some(index) = stack.pop() {
            count += 1;
            let (x, y) = (index % columns, index / columns);
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
            for ny in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(columns - 1) {
                    let next = ny * columns + nx;
                    if cells[next] && !seen[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        let (box_width, box_height) = (right - left + 1, bottom - top + 1);
        if count > best
            && box_width.min(box_height) * 100 >= box_width.max(box_height) * MIN_ASPECT_PERCENT
        {
            best = count;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::has_dense_square_texture;

    /// A frame with a centered checkerboard of `module`-pixel squares spanning
    /// `extent` pixels on a light background.
    fn frame(side: usize, extent: usize, module: usize) -> Vec<u8> {
        let start = (side - extent) / 2;
        let mut pixels = vec![230_u8; side * side];
        for y in start..start + extent {
            for x in start..start + extent {
                if ((x - start) / module + (y - start) / module).is_multiple_of(2) {
                    pixels[y * side + x] = 20;
                }
            }
        }
        pixels
    }

    #[test]
    fn large_module_area_is_evidence() {
        assert!(has_dense_square_texture(&frame(2048, 900, 24), 2048, 2048));
    }

    #[test]
    fn plain_frame_is_no_evidence() {
        assert!(!has_dense_square_texture(
            &vec![200_u8; 2048 * 1536],
            2048,
            1536
        ));
    }

    #[test]
    fn small_module_area_is_no_evidence() {
        // 64 pixels of 2048 cover less than one reduced block.
        assert!(!has_dense_square_texture(&frame(2048, 64, 8), 2048, 2048));
    }

    #[test]
    fn frames_below_the_reduced_side_are_always_evidence() {
        assert!(has_dense_square_texture(&vec![0_u8; 300 * 200], 300, 200));
    }
}
