//! Row-address-driven `MicroPDF417` discovery. Rows vote by physical position;
//! error correction assembles missing cells without inventing observed addresses.
use crate::{Detection, micro_pdf417_tables as t, numeric::usize_f32 as f, pdf417, reed_prime};

#[derive(Clone)]
struct Row {
    variant: usize,
    index: usize,
    x: f32,
    y: f32,
    module: f32,
    words: Vec<usize>,
}
fn pattern_gain(runs: &[usize], modules: usize, gain: f32) -> Option<(usize, f32)> {
    let pitch = f(runs.iter().sum()) / f(modules);
    if pitch < 0.8 {
        return None;
    }
    let mut value = 0;
    let mut total = 0;
    for (i, &run) in runs.iter().enumerate() {
        let adjusted = f(run) / pitch - if i % 2 == 0 { gain } else { -gain };
        let width = crate::numeric::f32_usize(adjusted.round());
        if width == 0 || width > 6 || (adjusted - f(width)).abs() > 0.7 {
            return None;
        }
        for _ in 0..width {
            value = (value << 1) | usize::from(i % 2 == 0);
        }
        total += width;
    }
    (total == modules).then_some((value, pitch))
}
fn address_matches(runs: &[usize], expected: usize) -> Option<f32> {
    let pitch = f(runs.iter().sum()) / 10.;
    if pitch < 0.8 {
        return None;
    }
    let mut widths = [0usize; 6];
    let mut index = 0;
    let mut prior = true;
    for bit in (0..10).rev() {
        let dark = expected & (1 << bit) != 0;
        if dark != prior {
            index += 1;
            prior = dark;
        }
        widths[index] += 1;
    }
    for gain in [0., -0.3, 0.3, -0.6, 0.6] {
        let errors: [f32; 6] = std::array::from_fn(|i| {
            (f(runs[i]) / pitch - f(widths[i]) - if i % 2 == 0 { gain } else { -gain }).abs()
        });
        if errors.iter().all(|&e| e < 0.9) && errors.iter().sum::<f32>() < 1.8 {
            return Some(pitch);
        }
    }
    None
}
fn codeword(runs: &[usize]) -> Option<(usize, usize)> {
    pattern_gain(runs, 17, 0.)
        .and_then(|(bits, _)| u32::try_from(bits).ok())
        .and_then(pdf417::lookup)
        .or_else(|| {
            let widths: [f32; 8] = std::array::from_fn(|i| f(runs[i]));
            pdf417::symbol(&widths)
        })
}
fn rows(line: &[bool], y: usize, out: &mut Vec<Row>) {
    let mut edges = vec![0];
    for i in 1..line.len() {
        if line[i] != line[i - 1] {
            edges.push(i);
        }
    }
    edges.push(line.len());
    let runs: Vec<_> = edges.windows(2).map(|p| p[1] - p[0]).collect();
    let start = usize::from(!line[0]);
    for at in (start..runs.len().saturating_sub(20)).step_by(2) {
        let exact = pattern_gain(&runs[at..at + 6], 10, 0.).and_then(|(bits, module)| {
            t::SIDE
                .iter()
                .position(|&v| v == bits)
                .map(|address| (address, module))
        });
        let matched = exact.or_else(|| {
            let pitch = f(runs[at..at + 6].iter().sum()) / 10.;
            if at == 0 || f(runs[at - 1]) < pitch * 1.5 {
                return None;
            }
            [-0.3, 0.3, -0.6, 0.6].into_iter().find_map(|gain| {
                let (bits, module) = pattern_gain(&runs[at..at + 6], 10, gain)?;
                t::SIDE
                    .iter()
                    .position(|&v| v == bits)
                    .map(|address| (address, module))
            })
        });
        let Some((address, module)) = matched else {
            continue;
        };
        let mut candidates = Vec::new();
        for variant in 0..34 {
            let row = (address + 53 - t::RAP_START[variant]) % 52;
            if row < t::VARIANTS[variant + 34] {
                candidates.push((variant, row));
            }
        }
        // Row variants share codeword positions; quantize each run slice once.
        let mut decoded = [None; 40];
        for (variant, index) in candidates {
            let cols = t::VARIANTS[variant];
            let needed = 6 + 8 * cols + usize::from(cols >= 3) * 6 + 7;
            if at + needed > runs.len() {
                continue;
            }
            let mut pos = at + 6;
            let mut words = Vec::with_capacity(cols);
            let cluster = (t::RAP_START[variant + 102] / 3 + index) % 3;
            for col in 0..cols {
                if (cols == 3 && col == 1) || (cols == 4 && col == 2) {
                    let expected = t::CENTRE[(t::RAP_START[variant + 34] - 1 + index) % 52];
                    if address_matches(&runs[pos..pos + 6], expected)
                        .is_none_or(|m| (m / module - 1.).abs() > 0.3)
                    {
                        break;
                    }
                    pos += 6;
                }
                let pitch = f(runs[pos..pos + 8].iter().sum()) / 17.;
                if (pitch / module - 1.).abs() > 0.3 {
                    break;
                }
                let word = decoded[pos - at].get_or_insert_with(|| codeword(&runs[pos..pos + 8]));
                let Some((value, found)) = *word else {
                    break;
                };
                if found != cluster {
                    break;
                }
                words.push(value);
                pos += 8;
            }
            if words.len() != cols {
                continue;
            }
            let expected = t::SIDE[(t::RAP_START[variant + 68] - 1 + index) % 52];
            if address_matches(&runs[pos..pos + 6], expected)
                .is_none_or(|m| (m / module - 1.).abs() > 0.3)
            {
                continue;
            }
            if (f(runs[pos + 6]) / module - 1.).abs() > 0.7 {
                continue;
            }
            out.push(Row {
                variant,
                index,
                x: f(edges[at]),
                y: f(y),
                module,
                words,
            });
            if out.len() >= 8192 {
                return;
            }
        }
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "Row voting, geometric agreement and correction share the same bounded assembly transaction."
)]
fn assemble(
    rows: &[Row],
    transform: impl Fn(f32, f32) -> [f32; 2],
    regions: &mut crate::regions::Regions,
    limited: &mut bool,
) -> Vec<Detection> {
    let mut result = Vec::new();
    let mut used = vec![false; rows.len()];
    let mut attempts = 0;
    for (at, first) in rows.iter().enumerate() {
        if used[at] {
            continue;
        }
        if attempts >= 256 {
            *limited = true;
            break;
        }
        attempts += 1;
        let variant = first.variant;
        let cols = t::VARIANTS[variant];
        let count = t::VARIANTS[variant + 34];
        let width = f(21 + cols * 17 + usize::from(cols >= 3) * 10) * first.module;
        let mut group = Vec::new();
        for (i, row) in rows.iter().enumerate().skip(at) {
            if row.variant == variant
                && (row.module / first.module - 1.).abs() < 0.15
                && (row.x - first.x).abs() < width * 0.3
                && (row.y - first.y).abs() < first.module * f(count) * 6.
            {
                group.push((i, row));
            }
        }
        // A repeated logical row after a gap is a different physical symbol.
        // Keep its first contiguous band, never average vertically stacked copies.
        let mut band_end = vec![None; count];
        let mut closed = vec![false; count];
        for &(_, row) in &group {
            if closed[row.index] {
                continue;
            }
            if band_end[row.index].is_some_and(|last| row.y - last > first.module * 2.) {
                closed[row.index] = true;
            } else {
                band_end[row.index] = Some(row.y);
            }
        }
        group.retain(|(_, row)| band_end[row.index].is_some_and(|last| row.y <= last));
        let mut samples: Vec<Vec<&Row>> = vec![Vec::new(); count];
        for &(_, row) in &group {
            samples[row.index].push(row);
        }
        let points: Vec<_> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.is_empty())
            .map(|(i, s)| [f(i) + 0.5, s.iter().map(|r| r.y).sum::<f32>() / f(s.len())])
            .collect();
        if points.len() < 3 {
            continue;
        }
        let n = f(points.len());
        let mx = points.iter().map(|p| p[0]).sum::<f32>() / n;
        let my = points.iter().map(|p| p[1]).sum::<f32>() / n;
        let den = points.iter().map(|p| (p[0] - mx).powi(2)).sum::<f32>();
        let pitch = points
            .iter()
            .map(|p| (p[0] - mx) * (p[1] - my))
            .sum::<f32>()
            / den;
        if !(0.8..=6.).contains(&(pitch / first.module)) {
            continue;
        }
        let top = my - pitch * mx;
        if points
            .iter()
            .any(|p| (p[1] - top - pitch * p[0]).abs() > pitch * 0.7)
        {
            continue;
        }
        let center_y = group.iter().map(|(_, r)| r.y).sum::<f32>() / f(group.len());
        let center_x = group.iter().map(|(_, r)| r.x).sum::<f32>() / f(group.len());
        let variance = group
            .iter()
            .map(|(_, r)| (r.y - center_y).powi(2))
            .sum::<f32>();
        let shear = group
            .iter()
            .map(|(_, r)| (r.y - center_y) * (r.x - center_x))
            .sum::<f32>()
            / variance.max(1.);
        if group
            .iter()
            .any(|(_, r)| (r.x - center_x - shear * (r.y - center_y)).abs() > first.module * 2.)
        {
            continue;
        }
        let left = center_x + shear * (top - center_y);
        let bottom_left = left + shear * pitch * f(count);
        let polygon = [
            [left, top],
            [left + width, top],
            [bottom_left + width, top + pitch * f(count)],
            [bottom_left, top + pitch * f(count)],
        ]
        .map(|[x, y]| transform(x, y));
        regions.add("MicroPDF417", polygon, 0.9, points.len());
        for &(i, _) in &group {
            used[i] = true;
        }
        let mut words = vec![0; cols * count];
        let mut erasures = Vec::new();
        for (row, observations) in samples.iter().enumerate() {
            for col in 0..cols {
                let mut votes = std::collections::BTreeMap::new();
                for observation in observations {
                    *votes.entry(observation.words[col]).or_insert(0usize) += 1;
                }
                if let Some((&word, _)) = votes.iter().max_by_key(|(_, votes)| *votes) {
                    words[row * cols + col] = word;
                } else {
                    erasures.push(row * cols + col);
                }
            }
        }
        let ecc = t::VARIANTS[variant + 68];
        if erasures.len() + 2 > ecc {
            continue;
        }
        let Some(corrected) = reed_prime::correct(&mut words, ecc, &erasures) else {
            continue;
        };
        let Some(payload) = pdf417::micro_payload(&words[..words.len() - ecc]) else {
            continue;
        };
        for (i, _) in group {
            used[i] = true;
        }
        result.push(Detection {
            format: "MicroPDF417".into(),
            text: payload.text,
            bytes: Some(payload.bytes),
            polygon,
            support: points.len(),
            error: f(corrected),
            structured_append: payload.structured_append,
            reader_initialization: payload.reader_initialization,
            gs1: false,
            assembled_rows: true,
            addon: None,
            codabar_start_stop: None,
        });
    }
    result
}
/// Dominant non-cardinal bar normals. Diffuse texture never adds angle sweeps.
fn angles(gray: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut result = vec![0., 90., 180., 270.];
    if w < 3 || h < 3 {
        return result;
    }
    let mut bins = [0f32; 36];
    let stride = ((w * h) / 200_000).isqrt().max(1);
    for y in (1..h - 1).step_by(stride) {
        for x in (1..w - 1).step_by(stride) {
            let at = y * w + x;
            let sample = |offset: usize| f32::from(gray[offset]);
            let dx = sample(at - w + 1) + 2. * sample(at + 1) + sample(at + w + 1)
                - sample(at - w - 1)
                - 2. * sample(at - 1)
                - sample(at + w - 1);
            let dy = sample(at + w - 1) + 2. * sample(at + w) + sample(at + w + 1)
                - sample(at - w - 1)
                - 2. * sample(at - w)
                - sample(at - w + 1);
            let weight = dx * dx + dy * dy;
            if weight < 400. {
                continue;
            }
            let angle = dy.atan2(dx).to_degrees().rem_euclid(180.);
            let bin = crate::numeric::f32_usize((angle / 5.).round()) % 36;
            bins[bin] += weight;
        }
    }
    let total = bins.iter().sum::<f32>();
    for _ in 0..2 {
        let Some((index, &weight)) = bins.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))
        else {
            break;
        };
        if weight < total * 0.08 || weight == 0. {
            break;
        }
        for offset in [35, 0, 1] {
            bins[(index + offset) % 36] = 0.;
        }
        if index <= 1 || index >= 35 || (17..=19).contains(&index) {
            continue;
        }
        for delta in [0., -2., 2., -4., 4.] {
            let angle = f(index) * 5. + delta;
            result.extend([angle, angle + 180.]);
        }
    }
    result
}
fn sample_line(
    image: &[bool],
    [w, h]: [usize; 2],
    index: usize,
    y: usize,
    columns: &[[f32; 2]],
    offset: [f32; 2],
) -> Vec<bool> {
    match index {
        0 => image[y * w..(y + 1) * w].to_vec(),
        1 => (0..h).map(|x| image[x * w + w - 1 - y]).collect(),
        2 => image[(h - 1 - y) * w..(h - y) * w]
            .iter()
            .rev()
            .copied()
            .collect(),
        3 => (0..h).map(|x| image[(h - 1 - x) * w + y]).collect(),
        _ => columns
            .iter()
            .map(|column| {
                let xx = (column[0] - offset[0]).round();
                let yy = (column[1] + offset[1]).round();
                xx >= 0.
                    && yy >= 0.
                    && xx < f(w)
                    && yy < f(h)
                    && image[crate::numeric::f32_usize(yy) * w + crate::numeric::f32_usize(xx)]
            })
            .collect(),
    }
}
/// Scan cardinal directions and strong oblique bar directions with bounded observations.
pub fn detect(
    gray: &[u8],
    size: [usize; 2],
    images: &mut crate::binarization::Images<'_>,
    regions: &mut crate::regions::Regions,
) -> (Vec<Detection>, bool) {
    let [w, h] = size;
    let directions = angles(gray, w, h);
    let mut results: Vec<Detection> = Vec::new();
    let mut limited = false;
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let image = images.get(mode);
        for (index, &turn) in directions.iter().enumerate() {
            if index >= 4 && mode % 2 == 1 {
                continue;
            }
            let (sin, cos) = turn.to_radians().sin_cos();
            let corners = [
                [0., 0.],
                [f(w) - 1., 0.],
                [f(w) - 1., f(h) - 1.],
                [0., f(h) - 1.],
            ];
            let projected = corners.map(|[x, y]| [x * cos + y * sin, -x * sin + y * cos]);
            let low = [
                projected.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                projected.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
            ];
            let high = [
                projected
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                projected
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::NEG_INFINITY, f32::max),
            ];
            let (line_width, row_count) = if index < 4 {
                [(w, h), (h, w)][index % 2]
            } else {
                (
                    crate::numeric::f32_usize((high[0] - low[0]).round()) + 1,
                    crate::numeric::f32_usize((high[1] - low[1]).round()) + 1,
                )
            };
            let map = |x: f32, y: f32| {
                [
                    (x + low[0]) * cos - (y + low[1]) * sin,
                    (x + low[0]) * sin + (y + low[1]) * cos,
                ]
            };
            let columns: Vec<_> = if index < 4 {
                Vec::new()
            } else {
                (0..line_width)
                    .map(|x| [(f(x) + low[0]) * cos, (f(x) + low[0]) * sin])
                    .collect()
            };
            let mut observations = Vec::new();
            let step = (row_count / 1024).max(2);
            let mut previous = Vec::new();
            let mut cached: Vec<Row> = Vec::new();
            for y in (0..row_count).step_by(step) {
                let line = sample_line(
                    image,
                    size,
                    index,
                    y,
                    &columns,
                    [(f(y) + low[1]) * sin, (f(y) + low[1]) * cos],
                );
                if line == previous {
                    for row in cached.iter().take(8192 - observations.len()) {
                        let mut row = row.clone();
                        row.y = f(y);
                        observations.push(row);
                    }
                } else {
                    let start = observations.len();
                    rows(&line, y, &mut observations);
                    cached.clear();
                    cached.extend_from_slice(&observations[start..]);
                    previous = line;
                }
                if observations.len() >= 8192 {
                    limited = true;
                    break;
                }
            }
            for read in assemble(&observations, map, regions, &mut limited) {
                if !results.iter().any(|r| {
                    r.text == read.text && crate::regions::overlap(&r.polygon, &read.polygon) > 0.6
                }) {
                    results.push(read);
                }
            }
        }
    }
    (results, limited)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scanlines_preserve_rotated_pixel_coordinates() {
        let [w, h] = [7, 5];
        let image: Vec<bool> = (0..w * h).map(|i| (i * 13 + i / w) % 7 < 3).collect();
        for direction in 0..4 {
            let (width, height) = [(w, h), (h, w)][direction % 2];
            for y in 0..height {
                let expected: Vec<bool> = (0..width)
                    .map(|x| {
                        let (xx, yy) = match direction {
                            0 => (x, y),
                            1 => (w - 1 - y, x),
                            2 => (w - 1 - x, h - 1 - y),
                            _ => (y, h - 1 - x),
                        };
                        image[yy * w + xx]
                    })
                    .collect();
                assert_eq!(
                    sample_line(&image, [w, h], direction, y, &[], [0.; 2]),
                    expected
                );
            }
        }
        for angle in [17f32, 43., 137., 223.] {
            let (sin, cos) = angle.to_radians().sin_cos();
            let low = [-3.25, -2.75];
            let columns: Vec<_> = (0..13)
                .map(|x| [(f(x) + low[0]) * cos, (f(x) + low[0]) * sin])
                .collect();
            for y in 0..11 {
                let expected: Vec<bool> = (0..13)
                    .map(|x| {
                        let xx = ((f(x) + low[0]) * cos - (f(y) + low[1]) * sin).round();
                        let yy = ((f(x) + low[0]) * sin + (f(y) + low[1]) * cos).round();
                        xx >= 0.
                            && yy >= 0.
                            && xx < f(w)
                            && yy < f(h)
                            && image
                                [crate::numeric::f32_usize(yy) * w + crate::numeric::f32_usize(xx)]
                    })
                    .collect();
                let offset = [(f(y) + low[1]) * sin, (f(y) + low[1]) * cos];
                assert_eq!(
                    sample_line(&image, [w, h], 4, y, &columns, offset),
                    expected
                );
            }
        }
    }
    #[test]
    fn stacked_equal_payloads_remain_two_physical_symbols() {
        let patterns = [
            68708, 128_478, 102_290, 68708, 128_088, 100_732, 81384, 126_152, 81722, 116_080, 97830,
        ];
        let mut observations = Vec::new();
        for top in [20., 98.] {
            for (index, &pattern) in patterns.iter().enumerate() {
                let (word, _) = pdf417::lookup(pattern).unwrap();
                for offset in 0..6 {
                    observations.push(Row {
                        variant: 0,
                        index,
                        x: 18.,
                        y: top + f(index) * 6. + f(offset),
                        module: 3.,
                        words: vec![word],
                    });
                }
            }
        }
        let mut regions = crate::regions::Regions::default();
        let mut limited = false;
        let reads = assemble(&observations, |x, y| [x, y], &mut regions, &mut limited);
        assert_eq!(reads.len(), 2);
        assert!(!limited);
        for (read, top) in reads.iter().zip([20., 98.]) {
            assert_eq!(read.text, "A");
            assert!((read.polygon[0][1] - top).abs() < 1.);
        }
    }
}
