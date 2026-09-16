//! Local peak/valley crossings preserve narrow retail bars lost by a whole-row
//! threshold under blur. Guards, digit ambiguity, checksum and multi-row support
//! are still checked by the retail reader.
pub(crate) fn runs(row: &[u8]) -> (Vec<bool>, Vec<f32>, Vec<usize>) {
    if row.is_empty() {
        return (vec![], vec![], vec![0]);
    }
    let mut extrema = Vec::new();
    let mut low = row[0];
    let mut high = low;
    let mut low_at = 0;
    let mut high_at = 0;
    let mut direction = 0;
    for (i, &value) in row.iter().enumerate() {
        if direction >= 0 {
            if value >= high {
                high = value;
                high_at = i;
            }
            if high.saturating_sub(value) >= 8 {
                extrema.push((high_at, high));
                direction = -1;
                low = value;
                low_at = i;
            }
        }
        if direction <= 0 {
            if value <= low {
                low = value;
                low_at = i;
            }
            if value.saturating_sub(low) >= 8 {
                extrema.push((low_at, low));
                direction = 1;
                high = value;
                high_at = i;
            }
        }
    }
    extrema.push(if direction == 1 {
        (high_at, high)
    } else {
        (low_at, low)
    });
    let first_black = extrema.len() > 1 && extrema[0].1 < extrema[1].1;
    let mut offsets = vec![0];
    let mut edges = vec![0.];
    for pair in extrema.windows(2) {
        let [(left, a), (right, b)] = [pair[0], pair[1]];
        let cut = (f32::from(a) + f32::from(b)) * 0.5;
        for i in left + 1..=right {
            let previous = f32::from(row[i - 1]);
            let next = f32::from(row[i]);
            if (previous >= cut) != (next >= cut) {
                edges
                    .push(crate::numeric::usize_f32(i) - 1. + (cut - previous) / (next - previous));
                offsets.push(i);
                break;
            }
        }
    }
    offsets.push(row.len());
    edges.push(crate::numeric::usize_f32(row.len()));
    let mut bits = vec![false; row.len()];
    for (i, range) in offsets.windows(2).enumerate() {
        bits[range[0]..range[1]].fill(first_black ^ (i % 2 != 0));
    }
    (
        bits,
        edges.windows(2).map(|p| p[1] - p[0]).collect(),
        offsets,
    )
}

pub(crate) fn sharpen_row(row: &[u8]) -> Vec<u8> {
    row.iter()
        .enumerate()
        .map(|(i, &v)| {
            let at = |d: isize| i.saturating_add_signed(d).min(row.len() - 1);
            let blurred = (f32::from(row[at(-2)])
                + 4. * f32::from(row[at(-1)])
                + 6. * f32::from(v)
                + 4. * f32::from(row[at(1)])
                + f32::from(row[at(2)]))
                / 16.;
            crate::numeric::f32_u8((3. * f32::from(v) - 2. * blurred).round().clamp(0., 255.))
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::runs;

    #[test]
    fn flat_and_small_noise_do_not_create_bar_transitions() {
        for row in [vec![120; 100], (0..100).map(|i| 120 + i % 2).collect()] {
            for values in [row.clone(), super::sharpen_row(&row)] {
                let (_, widths, offsets) = runs(&values);
                assert_eq!(widths, [100.]);
                assert_eq!(offsets, [0, 100]);
            }
        }
        assert!(runs(&[]).0.is_empty());
    }

    #[test]
    fn crossings_follow_local_contrast_and_preserve_coordinates() {
        let row = [
            200, 200, 100, 40, 100, 180, 100, 60, 100, 150, 100, 90, 100, 200,
        ];
        let (bits, widths, offsets) = runs(&row);
        assert_eq!(widths.len(), 7);
        assert_eq!(offsets.len(), 8);
        assert_eq!(bits.len(), row.len());
        assert!((widths.iter().sum::<f32>() - crate::numeric::usize_f32(row.len())).abs() < 1e-5);
        assert!(offsets.windows(2).all(|p| p[0] < p[1]));
        for (i, range) in offsets.windows(2).enumerate() {
            assert!(bits[range[0]..range[1]].iter().all(|&b| b == (i % 2 == 1)));
        }
    }
}
