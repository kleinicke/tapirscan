//! Refine an already decoded linear symbol using bar direction and cross-row
//! texture continuity. This never creates a payload or merges by text alone.
use crate::Detection;
type Quad = [[f32; 2]; 4];
fn pixel(image: &[u8], width: usize, height: usize, x: f32, y: f32) -> Option<f32> {
    if x < 0. || y < 0. || x >= (width - 1) as f32 || y >= (height - 1) as f32 {
        return None;
    }
    let xx = x.floor() as usize;
    let yy = y.floor() as usize;
    let fx = x - xx as f32;
    let fy = y - yy as f32;
    let at = yy * width + xx;
    Some(
        (f32::from(image[at]) * (1. - fx) + f32::from(image[at + 1]) * fx) * (1. - fy)
            + (f32::from(image[at + width]) * (1. - fx) + f32::from(image[at + width + 1]) * fx)
                * fy,
    )
}
pub(crate) fn refine(image: &[u8], width: usize, height: usize, quad: Quad) -> Quad {
    refine_axes(image, width, height, quad, false)
}
pub(crate) fn refine_retail(image: &[u8], width: usize, height: usize, quad: Quad) -> Quad {
    refine_axes(image, width, height, quad, true)
}
fn refine_axes(image: &[u8], width: usize, height: usize, quad: Quad, preserve_axis: bool) -> Quad {
    if width < 3 || height < 3 {
        return quad;
    }
    let edge = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let side = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let length = edge[0].hypot(edge[1]);
    let original_height = side[0].hypot(side[1]);
    if length < 30. {
        return quad;
    }
    let mut xx = 0_f64;
    let mut yy = 0_f64;
    let mut xy = 0_f64;
    for j in 0..5 {
        for i in 0..64 {
            let u = (i as f32 + 0.5) / 64.;
            let v = (j as f32 + 0.5) / 5.;
            let x = (quad[0][0] + edge[0] * u + side[0] * v).round() as isize;
            let y = (quad[0][1] + edge[1] * u + side[1] * v).round() as isize;
            if x < 1 || y < 1 || x >= width as isize - 1 || y >= height as isize - 1 {
                continue;
            }
            let at = y as usize * width + x as usize;
            let dx = f64::from(image[at + 1]) - f64::from(image[at - 1]);
            let dy = f64::from(image[at + width]) - f64::from(image[at - width]);
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
        }
    }
    if (xx - yy).hypot(2. * xy) < (xx + yy) * 0.5 || xx + yy < 100. {
        return quad;
    }
    let mut theta = (0.5 * (2. * xy).atan2(xx - yy)) as f32;
    if theta.cos() * edge[0] + theta.sin() * edge[1] < 0. {
        theta += std::f32::consts::PI;
    }
    let cosine = theta.cos();
    let rotation_sine = theta.sin();
    let agreement = (cosine * edge[0] + rotation_sine * edge[1]) / length;
    if agreement < 0.85 {
        return quad;
    }
    // A valid scanline need not be perpendicular to the bars, especially on
    // curved labels. Preserve its endpoints; extend along the bar direction
    // instead of rotating the decoded span away from the actual symbol.
    let (cosine, rotation_sine, axis_width, shear) = if preserve_axis {
        let u = [edge[0] / length, edge[1] / length];
        let shear = (cosine * u[1] - rotation_sine * u[0]) / agreement;
        (u[0], u[1], length, shear)
    } else {
        (cosine, rotation_sine, length * agreement, 0.)
    };
    let center = quad
        .iter()
        .fold([0_f32, 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25]);
    let a = center[0] * cosine + center[1] * rotation_sine;
    let b = -center[0] * rotation_sine + center[1] * cosine;
    let sample_count = (axis_width.ceil() as usize).clamp(64, 256);
    let a0 = a - axis_width * 0.5;
    let profile = |cross: f32| -> Option<Vec<f32>> {
        (0..sample_count)
            .map(|i| {
                let along =
                    a0 + axis_width * (i as f32 + 0.5) / sample_count as f32 + shear * (cross - b);
                pixel(
                    image,
                    width,
                    height,
                    along * cosine - cross * rotation_sine,
                    along * rotation_sine + cross * cosine,
                )
            })
            .collect()
    };
    let Some(reference) = profile(b) else {
        return quad;
    };
    let mean = reference.iter().sum::<f32>() / sample_count as f32;
    let reference: Vec<_> = reference.iter().map(|v| v - mean).collect();
    let variance = reference.iter().map(|v| v * v).sum::<f32>();
    if variance < sample_count as f32 * 36. {
        return quad;
    }
    let continuous = |row: &[f32], shift: isize| -> Option<isize> {
        let mean = row.iter().sum::<f32>() / sample_count as f32;
        let var = row.iter().map(|v| (v - mean).powi(2)).sum::<f32>();
        if var <= variance * 0.025 {
            return None;
        }
        let correlation = |offset: isize| {
            let mut cov = 0_f32;
            let mut reference_var = 0_f32;
            let mut row_var = 0_f32;
            for (i, &r) in reference.iter().enumerate() {
                let j = i as isize + offset;
                if j >= 0 && j < sample_count as isize {
                    let d = row[j as usize] - mean;
                    cov += r * d;
                    reference_var += r * r;
                    row_var += d * d;
                }
            }
            cov / (reference_var * row_var).sqrt().max(1.)
        };
        let mut best = (correlation(shift), shift);
        if best.0 < 0.8 {
            for offset in [shift - 1, shift + 1] {
                if offset.abs() > (sample_count / 12) as isize {
                    continue;
                }
                let score = correlation(offset);
                if score > best.0 {
                    best = (score, offset);
                }
            }
        }
        (best.0 > 0.4).then_some(best.1)
    };
    let step = (axis_width / 400.).clamp(1., 3.);
    let extent = |direction: f32| {
        let mut last = b;
        let mut misses = 0;
        let mut shift = 0;
        for i in 1..=((width as f32).hypot(height as f32) / step).ceil() as usize {
            let cross = b + direction * i as f32 * step;
            let Some(row) = profile(cross) else {
                break;
            };
            if let Some(next_shift) = continuous(&row, shift) {
                shift = next_shift;
                last = cross;
                misses = 0;
            } else {
                misses += 1;
                if misses >= 2 {
                    break;
                }
            }
        }
        last + direction * step * 0.5
    };
    let top = extent(-1.);
    let bottom = extent(1.);
    if bottom - top < original_height * 0.75 || bottom - top < 2. {
        return quad;
    }
    [
        [a0, top],
        [a0 + axis_width, top],
        [a0 + axis_width, bottom],
        [a0, bottom],
    ]
    .map(|[along, cross]| {
        let along = along + shear * (cross - b);
        [
            along * cosine - cross * rotation_sine,
            along * rotation_sine + cross * cosine,
        ]
    })
}

fn join_partial(first_detection: &Detection, second_detection: &Detection) -> Option<Quad> {
    if !matches!(
        first_detection.format.as_str(),
        "EAN13" | "UPCA" | "EAN8" | "UPCE" | "Code128" | "Code39" | "Code93" | "ITF" | "Codabar"
    ) {
        return None;
    }
    let axis = [
        first_detection.polygon[1][0] - first_detection.polygon[0][0],
        first_detection.polygon[1][1] - first_detection.polygon[0][1],
    ];
    let other = [
        second_detection.polygon[1][0] - second_detection.polygon[0][0],
        second_detection.polygon[1][1] - second_detection.polygon[0][1],
    ];
    let length = axis[0].hypot(axis[1]);
    let other_length = other[0].hypot(other[1]);
    if length < 20.
        || other_length < 20.
        || (axis[0] * other[0] + axis[1] * other[1]).abs() < length * other_length * 0.995
    {
        return None;
    }
    let cosine = axis[0] / length;
    let sine = axis[1] / length;
    let project = |q: &Quad| {
        q.iter().fold(
            [
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ],
            |mut bounds, p| {
                let x = p[0] * cosine + p[1] * sine;
                let y = -p[0] * sine + p[1] * cosine;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].max(x);
                bounds[2] = bounds[2].min(y);
                bounds[3] = bounds[3].max(y);
                bounds
            },
        )
    };
    let first = project(&first_detection.polygon);
    let second = project(&second_detection.polygon);
    let along_overlap = first[1].min(second[1]) - first[0].max(second[0]);
    if along_overlap < (first[1] - first[0]).max(second[1] - second[0]) * 0.95
        || crate::regions::overlap(&first_detection.polygon, &second_detection.polygon) < 0.4
    {
        return None;
    }
    let left = first[0].min(second[0]);
    let right = first[1].max(second[1]);
    let top = first[2].min(second[2]);
    let bottom = first[3].max(second[3]);
    Some(
        [[left, top], [right, top], [right, bottom], [left, bottom]]
            .map(|[x, y]| [x * cosine - y * sine, x * sine + y * cosine]),
    )
}
pub(crate) fn distinct(reads: &mut Vec<Detection>) {
    reads.sort_by_key(|r| std::cmp::Reverse(r.support));
    let mut result: Vec<Detection> = Vec::new();
    for read in reads.drain(..) {
        let same = |r: &Detection| {
            r.format == read.format
                && r.text == read.text
                && r.addon == read.addon
                && r.structured_append == read.structured_append
                && r.reader_initialization == read.reader_initialization
        };
        if result
            .iter()
            .any(|r| same(r) && crate::regions::overlap(&r.polygon, &read.polygon) >= 0.65)
        {
            continue;
        }
        if let Some((index, quad)) = result
            .iter()
            .enumerate()
            .filter(|(_, r)| same(r))
            .find_map(|(i, r)| join_partial(r, &read).map(|q| (i, q)))
        {
            result[index].polygon = quad;
        } else {
            result.push(read);
        }
    }
    *reads = result;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(top: f32, bottom: f32) -> Detection {
        Detection {
            bytes: None,
            structured_append: None,
            reader_initialization: false,
            addon: None,
            format: "EAN8".into(),
            text: "20013233".into(),
            polygon: [[20., top], [160., top], [160., bottom], [20., bottom]],
            support: 3,
            error: 0.,
            gs1: false,
        }
    }
    #[test]
    fn partial_fragments_join_but_separate_identical_symbols_survive() {
        let mut reads = vec![read(20., 80.), read(50., 110.), read(140., 200.)];
        distinct(&mut reads);
        assert_eq!(reads.len(), 2);
        assert_eq!(reads[0].polygon, read(20., 110.).polygon);
        assert_eq!(reads[1].polygon, read(140., 200.).polygon);
        let mut reads = vec![read(20., 80.), read(50., 110.)];
        reads[1].text = "12345670".into();
        distinct(&mut reads);
        assert_eq!(reads.len(), 2);
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "Deduplication must preserve exact input coordinates, not approximate them."
    )]
    fn matrix_duplicates_merge_without_collapsing_separate_copies() {
        let mut first = read(20., 160.);
        first.format = "DataMatrix".into();
        let mut duplicate = first.clone();
        duplicate.polygon = duplicate.polygon.map(|[x, y]| [x + 4., y - 3.]);
        let mut separate = first.clone();
        separate.polygon = separate.polygon.map(|[x, y]| [x + 180., y]);
        let mut reads = vec![first, duplicate, separate];
        distinct(&mut reads);
        assert_eq!(reads.len(), 2);
        assert_eq!(reads[0].polygon[0], [20., 20.]);
        assert_eq!(reads[1].polygon[0], [200., 20.]);
    }
    #[test]
    fn texture_extent_stops_between_distinct_identical_copies() {
        let (w, h) = (180, 160);
        let mut image = vec![255; w * h];
        for y in (20..60).chain(95..140) {
            for x in 20..160 {
                if (x - 20) / 3 % 2 == 0 {
                    image[y * w + x] = 0;
                }
            }
        }
        let first = refine(
            &image,
            w,
            h,
            [[20., 35.], [160., 35.], [160., 40.], [20., 40.]],
        );
        let second = refine(
            &image,
            w,
            h,
            [[20., 110.], [160., 110.], [160., 115.], [20., 115.]],
        );
        assert!((first[0][1] - 20.).abs() < 3. && (first[2][1] - 60.).abs() < 3.);
        assert!((second[0][1] - 95.).abs() < 3. && (second[2][1] - 140.).abs() < 3.);
        assert!(crate::regions::overlap(&first, &second) < 0.01);
    }
}
