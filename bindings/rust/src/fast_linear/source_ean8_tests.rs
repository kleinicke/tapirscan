use super::supplement_ean8_visual;
const WIDTHS: [u8; 43] = [
    1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1, 1, 1, 4,
    1, 3, 1, 2, 3, 2, 1, 1, 1, 1, 1,
];
fn runs(quiet: f32) -> Vec<f32> {
    let mut runs = vec![quiet];
    runs.extend(WIDTHS.map(|width| f32::from(width) * 2.));
    runs.push(quiet);
    runs
}
#[test]
fn accepts_observed_ean8_without_synthesizing_quiet_space() {
    let mut reads = Vec::new();
    supplement_ean8_visual(&runs(8.), false, 4, &mut reads);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].text, "12345670");
    supplement_ean8_visual(&runs(8.), false, 4, &mut reads);
    assert_eq!(
        reads.len(),
        1,
        "an existing overlapping owner blocks another claim"
    );
    let mut absent = Vec::new();
    supplement_ean8_visual(&runs(7.9), false, 4, &mut absent);
    supplement_ean8_visual(&runs(8.), false, 0, &mut absent);
    assert!(absent.is_empty());
}
#[test]
fn rejects_invalid_profile_values_and_incomplete_symbols() {
    use barcode_research_core::multi_profile::retail_short::source_ean8_evidence;
    let valid = WIDTHS.map(|width| f32::from(width) * 2.);
    assert_eq!(
        source_ean8_evidence(&valid).unwrap().0,
        [1, 2, 3, 4, 5, 6, 7, 0]
    );
    assert!(source_ean8_evidence(&valid[..42]).is_none());
    for value in [0., -1., f32::NAN, f32::INFINITY] {
        let mut invalid = valid;
        invalid[10] = value;
        assert!(source_ean8_evidence(&invalid).is_none());
    }
}
