use super::source_evidence::separated_source_rows_agree;
#[test]
fn requires_contrast_agreement_on_physically_separated_rows() {
    assert!(!separated_source_rows_agree(&[-0.9, -0.9, 0.], 6.));
    assert!(separated_source_rows_agree(&[-0.9, -0.9, 0.], 7.));
    assert!(separated_source_rows_agree(&[-0.9, 0., -0.9], 4.));
    assert!(!separated_source_rows_agree(&[-0.8, -0.95, 0.], 100.));
    assert!(!separated_source_rows_agree(&[0.95, 0.95, 0.95], 100.));
}
