use barcode_research_core::{row_group::Assembler, row_scan::Scanner, sampling::ImageView};
const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
fn image(height: usize) -> Vec<u8> {
    let mut p = vec![255; 440 * height];
    for y in 10..height - 10 {
        for x in 30..410 {
            if BITS.as_bytes()[(x - 30) / 4] == b'1' {
                p[y * 440 + x] = 0;
            }
        }
    }
    p
}
#[test]
fn bounded_assembly_preserves_unlinked_and_rejects_invalid_support() {
    let p = image(1300);
    let im = ImageView::new(&p, 440, 1300, 1, 440).unwrap();
    let mut scanner = Scanner::default();
    let c = scanner.scan(im).unwrap()[0].clone();
    // Synthetic already-decoded observations isolate the assembler budget, not
    // decoder accuracy. No synthetic observation is counted in dataset results.
    let mut observations = Vec::new();
    for i in 0..600 {
        let mut q = c.clone();
        let y = 10. + 2. * f64::from(i);
        q.polygon = [[29.5, y], [409.5, y], [409.5, y + 1.], [29.5, y + 1.]];
        q.support = 2;
        observations.push(q);
    }
    let mut assembler = Assembler::default();
    let result = assembler.assemble(im, &observations).unwrap().to_vec();
    assert_eq!(result.len(), 88);
    assert_eq!(assembler.checks, 512);
    assert!(assembler.exhausted);
    assert_eq!(assembler.merged, 512);
    assert_eq!(result.iter().map(|c| c.fragments).sum::<usize>(), 600);
    observations[0].support = usize::MAX;
    assert!(assembler.assemble(im, &observations).is_err());
    assert_eq!(assembler.checks, 0);
    assert_eq!(result.len(), 88);
    assert!(assembler.assemble(im, &vec![c; 4097]).is_err());
}

#[test]
fn pixel_edge_bands_preserve_geometry_and_real_gaps_in_all_rotations() {
    for gap in [false, true] {
        for turn in 0..4 {
            let (mut w, mut h) = (440, 60);
            let mut p = vec![255; w * h];
            for y in 0..h {
                for x in 30..410 {
                    if BITS.as_bytes()[(x - 30) / 4] == b'1' {
                        p[y * w + x] = 0;
                    }
                }
            }
            if gap {
                p[30 * w..31 * w].fill(255);
            } else {
                for x in 50..54 {
                    p[30 * w + x] = 255 - p[30 * w + x];
                }
            }
            for _ in 0..turn {
                let mut q = vec![0; p.len()];
                for y in 0..h {
                    for x in 0..w {
                        q[x * h + h - 1 - y] = p[y * w + x];
                    }
                }
                p = q;
                (w, h) = (h, w);
            }
            let mut scanner = Scanner::default();
            let im = ImageView::new(&p, w, h, 1, w).unwrap();
            assert_eq!(
                scanner
                    .scan(im)
                    .unwrap()
                    .iter()
                    .filter(|c| c.digits.is_some())
                    .count(),
                2
            );
            let grouped = scanner.scan_grouped(im).unwrap();
            assert_eq!(
                grouped.iter().filter(|c| c.digits.is_some()).count(),
                if gap { 2 } else { 1 }
            );
            assert!(grouped
                .iter()
                .flat_map(|c| c.polygon)
                .any(|p| p[0] == -0.5 || p[1] == -0.5));
        }
    }
}
#[test]
fn single_pixel_extent_retains_undecoded_and_outside_geometry_rejects() {
    let mut p = vec![255; 440];
    for x in 30..410 {
        if BITS.as_bytes()[(x - 30) / 4] == b'1' {
            p[x] = 0;
        }
    }
    let im = ImageView::new(&p, 440, 1, 1, 440).unwrap();
    let mut s = Scanner::default();
    let raw = s.scan(im).unwrap().to_vec();
    let grouped = s.scan_grouped(im).unwrap();
    assert_eq!(grouped.len(), raw.len());
    assert_eq!(grouped.len(), 1);
    assert!(grouped[0].digits.is_none());
    assert_eq!(grouped[0].polygon, raw[0].polygon);
    let mut bad = raw;
    bad[0].polygon[0][1] = -0.5001;
    let mut a = Assembler::default();
    assert!(a.assemble(im, &bad).is_err());
}
