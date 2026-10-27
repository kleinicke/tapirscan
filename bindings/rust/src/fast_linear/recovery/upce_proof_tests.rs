use super::*;

fn candidate(positions: &[f64], raw_rows: u64) -> Candidate<'static> {
    let pixels = &[255];
    let q = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];
    let mut read = Read::primary([0; 13], q, positions.len(), 0, Vec::new());
    read.format = "UPCE".into();
    Candidate {
        image: Image {
            data: pixels,
            width: 1,
            height: 1,
            channels: 1,
            stride: 1,
        },
        im: ImageView::new(pixels, 1, 1, 1, 1).unwrap(),
        quad: q,
        mask: linear::UPCE,
        dense: false,
        restored: false,
        profile: SourceProfile::Gray(0.),
        localized: true,
        remaining: 0,
        row_positions: positions.to_vec(),
        speck_misses: 0,
        observations: vec![super::super::Observation {
            read,
            left: 0.,
            right: 1.,
            first: positions[0],
            last: *positions.last().unwrap(),
            anchor: positions[0],
            min_span: 2.,
            last_row: positions.len() - 1,
            seen_rows: (1_u64 << positions.len()) - 1,
            original_rows: raw_rows,
            dense: false,
        }],
    }
}

#[test]
fn raw_upce_proof_rejects_duplicate_and_subpixel_rows() {
    for rows in [[0.5, 0.5, 0.5], [0.5, 0.501, 0.502], [0.5, 0.501, 0.8]] {
        let mut candidate = candidate(&rows, 7);
        retain_separated_raw_upce(&mut candidate);
        assert!(candidate.observations.is_empty());
    }
}

#[test]
fn raw_upce_proof_does_not_count_restored_rows_or_other_formats() {
    for raw in [0, 1, 3] {
        let mut candidate = candidate(&[0.2, 0.5, 0.8], raw);
        retain_separated_raw_upce(&mut candidate);
        assert!(candidate.observations.is_empty());
    }
    let mut candidate = candidate(&[0.2, 0.5, 0.8], 7);
    candidate.observations[0].read.format = "EAN13".into();
    retain_separated_raw_upce(&mut candidate);
    assert!(candidate.observations.is_empty());
}

#[test]
fn raw_upce_proof_preserves_refinement_rank_and_dense_confirmation() {
    let mut confirmed = candidate(&[0.2, 0.5, 0.8], 7);
    retain_separated_raw_upce(&mut confirmed);
    assert_eq!(confirmed.observations.len(), 1);
    let (mut reads, mut refined) = (Vec::new(), Vec::new());
    confirmed.append_confirmed(true, &mut reads, &mut refined);
    assert!(reads.is_empty());
    assert_eq!(refined.len(), 1);
    assert_eq!(refined[0].support, 2);

    let mut dense = candidate(&[0.2, 0.5, 0.8], 7);
    dense.observations[0].dense = true;
    retain_separated_raw_upce(&mut dense);
    let (mut reads, mut refined) = (Vec::new(), Vec::new());
    dense.append_confirmed(true, &mut reads, &mut refined);
    assert!(
        reads.is_empty() && refined.is_empty(),
        "dense recovery still needs four confirmations"
    );
}
