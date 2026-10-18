//! Low-resolution evidence for a large, dense 2D symbol.
//!
//! A frame is reduced to [`SIDE`] pixels on its long side and split into
//! [`BLOCK`]-pixel blocks. A block looks like matrix modules when it has strong
//! contrast, a balanced dark/light share and on average at least one transition
//! per row and per column. Printed text, product photos and 1D barcodes fail at
//! least one of these tests in most blocks. The evidence is the largest
//! 8-connected group of such blocks with a roughly square bounding box.
//!
//! The contrast a block needs follows the frame's own gray-level range, so dark,
//! washed-out or shadowed frames are judged like well-exposed ones. Underexposed
//! frames, whose modules cannot be told from sensor noise at this resolution,
//! always count as evidence.
//!
//! Modules larger than about half a block have too few transitions per block.
//! A copy reduced four times further looks for them with overlapping block
//! windows; there a window needs more, evenly spread transitions, and its group
//! must cover a larger share of the frame, as a symbol with such modules does.

/// Long side of the reduced copy, in pixels.
const SIDE: usize = 512;
/// Block side, in pixels of the reduced copy; a `u16` mask holds one block row.
const BLOCK: usize = 16;
/// Gray-level range a block needs in a frame that spans the full range.
const MAX_CONTRAST: u32 = 80;
/// Smallest gray-level range a block needs in a low-contrast frame.
const MIN_CONTRAST: u32 = 48;
/// Block contrast, in percent of the frame's 2nd-to-98th percentile range.
const CONTRAST_PERCENT: u32 = 35;
/// Frames whose third quartile is darker than this are underexposed.
const DARK_FRAME: u32 = 32;
/// Smallest share, in percent, of the darker and the lighter half of a block.
const MIN_BALANCE_PERCENT: usize = 35;
/// Smallest short-to-long side ratio, in percent, of a group's bounding box.
const MIN_ASPECT_PERCENT: usize = 40;
/// Smallest group, in ten-thousandths of all blocks of the frame.
const MIN_GROUP_PER_10K: usize = 120;
/// Reduction of the coarse copy searched for large modules, relative to the
/// reduced copy.
const COARSE_FACTOR: usize = 4;
/// Smallest transition count per direction of a coarse window, 1.75 per row.
const COARSE_TRANSITIONS: usize = 7 * BLOCK / 4;
/// Smallest ratio, in percent, of a coarse window's fewer to more transitions.
const COARSE_ISOTROPY_PERCENT: usize = 70;
/// Smallest number of rows and of columns with a transition in a coarse window.
const COARSE_LINES: usize = BLOCK - 4;
/// Smallest group of coarse evidence, in ten-thousandths of all blocks.
const MIN_COARSE_GROUP_PER_10K: usize = 400;

type Block = [[u8; BLOCK]; BLOCK];

/// Whether the frame holds a roughly square group of module-like blocks that
/// covers at least [`MIN_GROUP_PER_10K`] of its blocks (or, for large modules,
/// [`MIN_COARSE_GROUP_PER_10K`]), or is underexposed.
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
    let (copy_width, copy_height) = (columns * BLOCK, rows * BLOCK);
    let mut copy = Vec::with_capacity(copy_width * copy_height);
    let x_source: Vec<_> = (0..copy_width)
        .map(|x| scale(2 * x + 1, width, 2 * out_width).min(width - 1))
        .collect();
    for y in 0..copy_height {
        let source_y = scale(2 * y + 1, height, 2 * out_height).min(height - 1);
        let source = &pixels[source_y * width..(source_y + 1) * width];
        copy.extend(x_source.iter().map(|&x| source[x]));
    }
    let Some(min_contrast) = block_contrast(&copy) else {
        return true;
    };

    // Blocks with the frame-relative contrast, and those with the full
    // MAX_CONTRAST. A weak block can join strong ones into a group that is no
    // longer square, so the strong blocks are also grouped on their own.
    let mut textured = vec![false; columns * rows];
    let mut strong = vec![false; columns * rows];
    let mut block = [[0_u8; BLOCK]; BLOCK];
    for by in 0..rows {
        for bx in 0..columns {
            for (j, row) in block.iter_mut().enumerate() {
                row.copy_from_slice(&copy[(by * BLOCK + j) * copy_width + bx * BLOCK..][..BLOCK]);
            }
            if let Some(t) = transitions(&block, min_contrast)
                && t.horizontal >= BLOCK
                && t.vertical >= BLOCK
            {
                textured[by * columns + bx] = true;
                strong[by * columns + bx] = t.contrast >= MAX_CONTRAST;
            }
        }
    }
    let enough = |cells: &[bool], per_10k: usize| {
        largest_square_group(cells, columns, rows) * 10_000 >= per_10k * columns * rows
    };
    if enough(&textured, MIN_GROUP_PER_10K) || enough(&strong, MIN_GROUP_PER_10K) {
        return true;
    }

    // Coarse windows of BLOCK pixels at a stride of half a block. A window
    // marks the reduced-copy blocks under its central half.
    textured.fill(false);
    let (coarse, coarse_width, coarse_height) = shrink(&copy, copy_width, copy_height);
    let first =
        |origin: usize| (origin * COARSE_FACTOR + BLOCK * COARSE_FACTOR / 4 + BLOCK / 2) / BLOCK;
    let starts = |side: usize| (0..(side + 1).saturating_sub(BLOCK)).step_by(BLOCK / 2);
    for y0 in starts(coarse_height) {
        for x0 in starts(coarse_width) {
            for (j, row) in block.iter_mut().enumerate() {
                row.copy_from_slice(&coarse[(y0 + j) * coarse_width + x0..][..BLOCK]);
            }
            if transitions(&block, min_contrast).is_some_and(|t| t.is_coarse_module()) {
                let (left, top) = (first(x0), first(y0));
                let right = (left + COARSE_FACTOR / 2).min(columns);
                for y in top..(top + COARSE_FACTOR / 2).min(rows) {
                    textured[y * columns + left..y * columns + right].fill(true);
                }
            }
        }
    }
    enough(&textured, MIN_COARSE_GROUP_PER_10K)
}

/// The gray-level range a block needs in this reduced copy, or `None` when the
/// copy is underexposed.
fn block_contrast(copy: &[u8]) -> Option<u32> {
    // Every third pixel is enough for the percentiles.
    let mut histogram = [0_usize; 256];
    let mut samples = 0;
    for &value in copy.iter().step_by(3) {
        histogram[usize::from(value)] += 1;
        samples += 1;
    }
    let percentile = |percent: usize| {
        let target = samples * percent / 100;
        let mut seen = 0;
        (0_u32..)
            .zip(histogram)
            .find(|&(_, count)| {
                seen += count;
                seen > target
            })
            .map_or(255, |(value, _)| value)
    };
    if percentile(75) < DARK_FRAME {
        return None;
    }
    let spread = percentile(98).saturating_sub(percentile(2));
    Some((spread * CONTRAST_PERCENT / 100).clamp(MIN_CONTRAST, MAX_CONTRAST))
}

/// [`COARSE_FACTOR`]-square box average of an image; trailing rows and columns
/// that do not fill a box are dropped.
fn shrink(image: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let (small_width, small_height) = (width / COARSE_FACTOR, height / COARSE_FACTOR);
    let mut sums = vec![0_u16; small_width];
    let mut small = Vec::with_capacity(small_width * small_height);
    for y in 0..small_height {
        sums.fill(0);
        for row in image[y * COARSE_FACTOR * width..]
            .chunks(width)
            .take(COARSE_FACTOR)
        {
            for (sum, pixels) in sums.iter_mut().zip(row.as_chunks::<COARSE_FACTOR>().0) {
                *sum += pixels.iter().map(|&v| u16::from(v)).sum::<u16>();
            }
        }
        let area = u16::try_from(COARSE_FACTOR * COARSE_FACTOR).unwrap_or(u16::MAX);
        small.extend(
            sums.iter()
                .map(|&sum| u8::try_from((sum + area / 2) / area).unwrap_or(u8::MAX)),
        );
    }
    (small, small_width, small_height)
}

/// Dark/light transitions of a block, split at the middle of its gray-level range.
struct Transitions {
    /// Gray-level range of the block.
    contrast: u32,
    horizontal: usize,
    vertical: usize,
    /// Rows with at least one transition.
    rows: usize,
    /// Columns with at least one transition.
    columns: usize,
}

impl Transitions {
    /// Whether a coarse window has the many, evenly spread transitions of
    /// large modules rather than the few of an edge, a text line or bars.
    fn is_coarse_module(&self) -> bool {
        let (fewer, more) = (
            self.horizontal.min(self.vertical),
            self.horizontal.max(self.vertical),
        );
        fewer >= COARSE_TRANSITIONS
            && fewer * 100 >= more * COARSE_ISOTROPY_PERCENT
            && self.rows.min(self.columns) >= COARSE_LINES
    }
}

/// The transitions of a block with the contrast and balance of matrix modules.
fn transitions(block: &Block, min_contrast: u32) -> Option<Transitions> {
    let (low, high) = block
        .iter()
        .flatten()
        .fold((u8::MAX, 0), |(low, high), &v| (low.min(v), high.max(v)));
    let contrast = u32::from(high - low);
    if contrast < min_contrast {
        return None;
    }
    let middle = u16::from(low) + u16::from(high - low) / 2;
    // One bit per pixel, set where it is dark.
    let masks = block.map(|row| {
        (0..BLOCK).fold(0_u16, |mask, x| {
            mask | (u16::from(u16::from(row[x]) < middle) << x)
        })
    });
    let dark_count = masks.iter().map(|m| m.count_ones() as usize).sum::<usize>();
    let total = BLOCK * BLOCK;
    if dark_count * 100 < total * MIN_BALANCE_PERCENT
        || dark_count * 100 > total * (100 - MIN_BALANCE_PERCENT)
    {
        return None;
    }
    let mut result = Transitions {
        contrast,
        horizontal: 0,
        vertical: 0,
        rows: 0,
        columns: 0,
    };
    let mut changed_columns = 0_u16;
    for (i, &mask) in masks.iter().enumerate() {
        // Bit x is set where pixels x and x + 1 differ.
        let count = ((mask ^ (mask >> 1)) & (u16::MAX >> 1)).count_ones() as usize;
        result.horizontal += count;
        result.rows += usize::from(count > 0);
        if let Some(&below) = masks.get(i + 1) {
            result.vertical += (mask ^ below).count_ones() as usize;
            changed_columns |= mask ^ below;
        }
    }
    result.columns = changed_columns.count_ones() as usize;
    Some(result)
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

    /// A `width` x `height` frame of `background` with a pseudo-random module
    /// pattern of `module`-pixel squares, `dark` and `light`, spanning `extent`
    /// pixels at the center.
    fn frame(
        (width, height): (usize, usize),
        extent: usize,
        module: usize,
        (dark, light, background): (u8, u8, u8),
    ) -> Vec<u8> {
        let (left, top) = ((width - extent) / 2, (height - extent) / 2);
        let mut pixels = vec![background; width * height];
        for y in top..top + extent {
            for x in left..left + extent {
                let (mx, my) = (((x - left) / module) as u64, ((y - top) / module) as u64);
                let hash = ((mx * 73_856_093) ^ (my * 19_349_663)).wrapping_mul(2_654_435_761);
                pixels[y * width + x] = if (hash >> 7) & 1 == 0 { dark } else { light };
            }
        }
        pixels
    }

    const SHARP: (u8, u8, u8) = (20, 230, 200);

    #[test]
    fn module_area_is_evidence() {
        let pixels = frame((2048, 2048), 900, 12, SHARP);
        assert!(has_dense_square_texture(&pixels, 2048, 2048));
    }

    #[test]
    fn large_module_area_is_evidence() {
        // 93-pixel modules are 12 pixels in the reduced copy, close to a block.
        let pixels = frame((4000, 3000), 2700, 93, SHARP);
        assert!(has_dense_square_texture(&pixels, 4000, 3000));
    }

    #[test]
    fn low_contrast_module_area_is_evidence() {
        // A range of 60 levels in a frame that never gets brighter than 180.
        let pixels = frame((3264, 2448), 1400, 16, (110, 170, 140));
        assert!(has_dense_square_texture(&pixels, 3264, 2448));
    }

    #[test]
    fn low_contrast_noise_is_no_evidence() {
        // A range of 30 levels stays below the smallest block contrast.
        let pixels = frame((3264, 2448), 2400, 2, (120, 150, 135));
        assert!(!has_dense_square_texture(&pixels, 3264, 2448));
    }

    #[test]
    fn underexposed_frame_is_evidence() {
        let pixels = frame((3024, 4032), 400, 7, (5, 30, 8));
        assert!(has_dense_square_texture(&pixels, 3024, 4032));
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
    fn stripes_are_no_evidence() {
        // Large vertical bars: transitions in one direction only.
        let (width, height) = (4000, 3000);
        let pixels: Vec<u8> = (0..width * height)
            .map(|i| if (i % width) / 90 % 2 == 0 { 20 } else { 230 })
            .collect();
        assert!(!has_dense_square_texture(&pixels, width, height));
    }

    #[test]
    fn small_module_area_is_no_evidence() {
        // 64 pixels of 2048 cover less than one reduced block.
        let pixels = frame((2048, 2048), 64, 8, SHARP);
        assert!(!has_dense_square_texture(&pixels, 2048, 2048));
    }

    #[test]
    fn frames_below_the_reduced_side_are_always_evidence() {
        assert!(has_dense_square_texture(&vec![0_u8; 300 * 200], 300, 200));
    }
}
