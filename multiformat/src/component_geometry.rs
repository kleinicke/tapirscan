//! Bounded connected-component geometry shared by matrix finder detectors.
type Quad = [[f32; 2]; 4];
fn outline(
    bits: &[bool],
    width: usize,
    start_coord: [usize; 2],
    bounds: [usize; 4],
    count_range: [f32; 2],
) -> Option<Vec<[f32; 2]>> {
    let [left, top, right, bottom] = bounds;
    let [x, y] = start_coord;
    if x <= left || y <= top || x + 1 >= right || y + 1 >= bottom {
        return None;
    }
    let span = right - left;
    let height = bottom - top;
    let color = bits[y * width + x];
    let mut seen = vec![false; span * height];
    let mut stack = vec![(x, y)];
    let mut row_min = vec![right; height];
    let mut row_max = vec![left; height];
    let mut count = 0;
    while let Some((x, y)) = stack.pop() {
        let row = (y - top) * span;
        if seen[row + x - left] {
            continue;
        }
        if x <= left || y <= top || x + 1 >= right || y + 1 >= bottom {
            return None;
        }
        let mut a = x;
        let mut b = x + 1;
        while a > left && bits[y * width + a - 1] == color && !seen[row + a - 1 - left] {
            a -= 1;
        }
        while b < right && bits[y * width + b] == color && !seen[row + b - left] {
            b += 1;
        }
        if a == left || b == right {
            return None;
        }
        seen[row + a - left..row + b - left].fill(true);
        count += b - a;
        if count as f32 > count_range[1] {
            return None;
        }
        row_min[y - top] = row_min[y - top].min(a);
        row_max[y - top] = row_max[y - top].max(b - 1);
        // Four-connected component: adjacent rows overlap this span exactly.
        for yy in [y - 1, y + 1] {
            let mut xx = a;
            while xx < b {
                let at = (yy - top) * span + xx - left;
                if bits[yy * width + xx] == color && !seen[at] {
                    stack.push((xx, yy));
                    xx += 1;
                    while xx < b
                        && bits[yy * width + xx] == color
                        && !seen[(yy - top) * span + xx - left]
                    {
                        xx += 1;
                    }
                } else {
                    xx += 1;
                }
            }
        }
    }
    if (count as f32) < count_range[0] {
        return None;
    }
    // Row endpoints preserve the convex hull of the complete pixel squares.
    let mut boundary = Vec::new();
    for (y, (&a, &b)) in row_min.iter().zip(&row_max).enumerate() {
        if a == right {
            continue;
        }
        let y = (y + top) as f32;
        boundary.extend([
            [a as f32, y],
            [b as f32 + 1., y],
            [b as f32 + 1., y + 1.],
            [a as f32, y + 1.],
        ]);
    }
    Some(crate::dm_detect::hull(boundary))
}

pub(crate) fn quad(
    bits: &[bool],
    width: usize,
    seed: [usize; 2],
    bounds: [usize; 4],
    count_range: [f32; 2],
) -> Option<Quad> {
    crate::dm_detect::quad(outline(bits, width, seed, bounds, count_range)?)
}
pub(crate) fn quads(
    bits: &[bool],
    width: usize,
    seed: [usize; 2],
    bounds: [usize; 4],
    count_range: [f32; 2],
) -> Option<Vec<Quad>> {
    let hull = outline(bits, width, seed, bounds, count_range)?;
    Some(
        crate::dm_detect::quad(hull.clone())
            .into_iter()
            .chain(crate::dm_detect::enclosing_quad(&hull, 0.))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn span_trace_matches_pixel_flood_with_holes_and_size_limits() {
        let w = 31;
        let mut state = 71829_u32;
        for density in [25, 45, 65, 85] {
            for _ in 0..30 {
                let mut bits = vec![false; w * w];
                for y in 1..w - 1 {
                    for x in 1..w - 1 {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        bits[y * w + x] = state % 100 < density;
                    }
                }
                bits[15 * w + 15] = true;
                let mut seen = vec![false; bits.len()];
                let mut stack = vec![(15, 15)];
                let mut pixels = Vec::new();
                while let Some((x, y)) = stack.pop() {
                    if seen[y * w + x] || !bits[y * w + x] {
                        continue;
                    }
                    seen[y * w + x] = true;
                    pixels.push((x, y));
                    stack.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
                }
                let boundary = pixels
                    .iter()
                    .flat_map(|&(x, y)| {
                        let (x, y) = (x as f32, y as f32);
                        [[x, y], [x + 1., y], [x + 1., y + 1.], [x, y + 1.]]
                    })
                    .collect();
                let hull = crate::dm_detect::hull(boundary);
                for limits in [[0., 2000.], [5., 30.], [30., 200.]] {
                    let expected = if (limits[0]..=limits[1]).contains(&(pixels.len() as f32)) {
                        Some(hull.clone())
                    } else {
                        None
                    };
                    assert_eq!(outline(&bits, w, [15, 15], [0, 0, w, w], limits), expected);
                }
            }
        }
    }
}
