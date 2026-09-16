//! Independent affine recovery from one intact high-version QR finder.
//! Version bit placement/BCH polynomial verified against Nayuki's MIT encoder:
//! <https://github.com/nayuki/QR-Code-generator/blob/master/rust/src/lib.rs>
//! This only proposes grids; the project-owned QR payload/ECC decoder validates them.
use super::{distance, map, sample, Finder};
use crate::{qr, Detection};

const fn version_word(version: u32) -> u32 {
    let mut remainder = version << 12;
    let mut bit = 18;
    while bit > 12 {
        bit -= 1;
        if remainder & (1 << bit) != 0 {
            remainder ^= 0x1f25 << (bit - 12);
        }
    }
    (version << 12) | remainder
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "The fixed 34-entry version-word table has indices 0 through 33."
)]
const WORDS: [u32; 34] = {
    let mut words = [0; 34];
    let mut index = 0;
    while index < words.len() {
        words[index] = version_word(index as u32 + 7);
        index += 1;
    }
    words
};

fn qr_coordinate(value: usize) -> f32 {
    f32::from(u16::try_from(value).expect("validated QR coordinates fit u16"))
}

fn version(word: u32) -> Option<usize> {
    // Exact version metadata is the inexpensive tier. More damaged headers
    // remain the responsibility of the unchanged multi-finder search.
    WORDS
        .iter()
        .position(|&expected| expected == word)
        .map(|i| i + 7)
}

fn frame(quad: [[f32; 2]; 4], rotation: usize, mirror: bool, span: f32) -> [f32; 6] {
    let ordered: [[f32; 2]; 4] =
        std::array::from_fn(|i| quad[(rotation + if mirror { 4 - i } else { i }) % 4]);
    std::array::from_fn(|i| {
        let axis = i % 2;
        match i / 2 {
            0 => {
                ((ordered[1][axis] - ordered[0][axis]) + (ordered[2][axis] - ordered[3][axis]))
                    / (span * 2.)
            }
            1 => {
                ((ordered[3][axis] - ordered[0][axis]) + (ordered[2][axis] - ordered[1][axis]))
                    / (span * 2.)
            }
            _ => ordered.iter().map(|p| p[axis]).sum::<f32>() * 0.25,
        }
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "This image-geometry sampler checks finite nonnegative floored coordinates against the f32 projection bounds before indexing."
)]
fn pixel(image: &[bool], w: usize, h: usize, frame: &[f32; 6], x: f32, y: f32) -> Option<bool> {
    let px = (frame[4] + frame[0] * x + frame[2] * y).floor();
    let py = (frame[5] + frame[1] * x + frame[3] * y).floor();
    if !px.is_finite() || !py.is_finite() || px < 0. || py < 0. || px >= w as f32 || py >= h as f32
    {
        return None;
    }
    Some(image[py as usize * w + px as usize])
}

fn header(image: &[bool], w: usize, h: usize, frame: &[f32; 6], offset: f32) -> Option<usize> {
    let mut word = 0;
    for bit in 0_u8..18 {
        let x = f32::from(bit % 3) - 7. + offset;
        let y = f32::from(bit / 3) - 3. + offset;
        word |= u32::from(pixel(image, w, h, frame, x, y)?) << bit;
    }
    version(word)
}

#[expect(
    clippy::too_many_arguments,
    reason = "The decode attempt keeps image geometry, candidate transform, and shared attempt state explicit in the bounded recovery loop."
)]
fn decode_transform(
    image: &[bool],
    w: usize,
    h: usize,
    finder: &Finder,
    n: usize,
    t: &[f32; 8],
    offset: f32,
    attempts: &mut usize,
    limited: &mut bool,
) -> Option<(Detection, [[f32; 2]; 3])> {
    if *attempts >= 64 {
        *limited = true;
        return None;
    }
    *attempts += 1;
    let grid = sample(image, w, h, n, t, offset)?;
    for transpose in [false, true] {
        let transposed;
        let grid = if transpose {
            transposed = (0..n * n)
                .map(|i| grid[(i % n) * n + i / n])
                .collect::<Vec<_>>();
            transposed.as_slice()
        } else {
            grid.as_slice()
        };
        if let Some(read) = qr::decode_matrix(grid, n) {
            let nf = qr_coordinate(n);
            return Some((
                Detection {
                    bytes: Some(read.bytes),
                    structured_append: read.structured_append,
                    reader_initialization: false,
                    addon: None,
                    format: "QRCode".into(),
                    text: read.text,
                    polygon: [[0., 0.], [nf, 0.], [nf, nf], [0., nf]].map(|[x, y]| map(t, x, y)),
                    support: finder.support,
                    error: qr_coordinate(read.corrected),
                    gs1: read.gs1,
                },
                [[3.5, 3.5], [nf - 3.5, 3.5], [3.5, nf - 3.5]].map(|[x, y]| map(t, x, y)),
            ));
        }
    }
    None
}

fn aligned_transforms(
    image: &[bool],
    w: usize,
    h: usize,
    version: usize,
    frame: &[f32; 6],
    initial: &[f32; 8],
) -> Vec<[f32; 8]> {
    let n = qr_coordinate(17 + 4 * version);
    let positions = qr::alignment_positions(version);
    let inner = qr_coordinate(positions[1]) + 0.5;
    // These are real alignment locations for every version>=7: an interior
    // top/bottom pair and bottom-right. Reserved finder corners are excluded.
    let source = [
        [n - 3.5, 3.5],
        [inner, 6.5],
        [inner, n - 6.5],
        [n - 6.5, n - 6.5],
    ];
    let targets: [Vec<[f32; 2]>; 3] = std::array::from_fn(|i| {
        let guess = map(initial, source[i + 1][0], source[i + 1][1]);
        let mut found = super::alignment(
            image,
            w,
            h,
            guess,
            [frame[0], frame[1]],
            [frame[2], frame[3]],
        );
        found.truncate(2);
        found
    });
    let mut out = Vec::new();
    for &a in &targets[0] {
        for &b in &targets[1] {
            for &c in &targets[2] {
                if let Some(t) = super::homography(source, [[frame[4], frame[5]], a, b, c]) {
                    out.push(t);
                }
            }
        }
    }
    out
}

#[expect(
    clippy::too_many_arguments,
    reason = "The bounded one-finder recovery loop keeps finder geometry, shared decode budget, and alignment budget explicit."
)]
fn recover_geometry(
    image: &[bool],
    w: usize,
    h: usize,
    finder: &Finder,
    quad: [[f32; 2]; 4],
    span: f32,
    attempts: &mut usize,
    limited: &mut bool,
    alignments: &mut usize,
    allow_alignment: bool,
) -> Option<(Detection, [[f32; 2]; 3])> {
    for rotation in 0..4 {
        for mirror in [false, true] {
            let frame = frame(quad, rotation, mirror, span);
            let mut aligned = false;
            for offset in [0., -0.2, 0.2] {
                let Some(version) = header(image, w, h, &frame, offset) else {
                    continue;
                };
                let n = 17 + 4 * version;
                let nf = qr_coordinate(n);
                let t = [
                    frame[0],
                    frame[2],
                    frame[4] - frame[0] * (nf - 3.5) - frame[2] * 3.5,
                    frame[1],
                    frame[3],
                    frame[5] - frame[1] * (nf - 3.5) - frame[3] * 3.5,
                    0.,
                    0.,
                ];
                if !qr::plausible_image_header(n, |x, y| {
                    pixel(
                        image,
                        w,
                        h,
                        &frame,
                        qr_coordinate(x) + 0.5 + offset - (nf - 3.5),
                        qr_coordinate(y) + 0.5 + offset - 3.5,
                    )
                }) {
                    continue;
                }
                if let Some(read) =
                    decode_transform(image, w, h, finder, n, &t, offset, attempts, limited)
                {
                    return Some(read);
                }
                if *attempts >= 64 {
                    return None;
                }
                if !aligned && allow_alignment {
                    aligned = true;
                    if *alignments >= 8 {
                        *limited = true;
                        continue;
                    }
                    *alignments += 1;
                    let transforms = aligned_transforms(image, w, h, version, &frame, &t);
                    for refined_offset in [0., -0.2, 0.2] {
                        for candidate in &transforms {
                            if let Some(read) = decode_transform(
                                image,
                                w,
                                h,
                                finder,
                                n,
                                candidate,
                                refined_offset,
                                attempts,
                                limited,
                            ) {
                                return Some(read);
                            }
                            if *attempts >= 64 {
                                return None;
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "The outer-ring finder clips finite projected geometry in its f32 image-coordinate domain before converting accepted bounds and seeds to indices."
)]
fn outer_quad(image: &[bool], w: usize, h: usize, finder: &Finder) -> Option<[[f32; 2]; 4]> {
    let inner = frame(finder.quad?, 0, false, 3.);
    let center = [inner[4], inner[5]];
    let module = inner[0].hypot(inner[1]).max(inner[2].hypot(inner[3]));
    let radius = (module * 5. + 3.).ceil();
    let bounds = [
        (center[0] - radius).max(0.) as usize,
        (center[1] - radius).max(0.) as usize,
        (center[0] + radius + 1.).min(w as f32) as usize,
        (center[1] + radius + 1.).min(h as f32) as usize,
    ];
    // The black outer ring is separated from payload and central square by
    // white modules. Its seven-module edges give a longer orientation baseline.
    for side in [[0., -3.], [3., 0.], [0., 3.], [-3., 0.]] {
        let x = (center[0] + inner[0] * side[0] + inner[2] * side[1]).floor();
        let y = (center[1] + inner[1] * side[0] + inner[3] * side[1]).floor();
        if x < 0. || y < 0. || x >= w as f32 || y >= h as f32 {
            continue;
        }
        let seed = [x as usize, y as usize];
        if !image[seed[1] * w + seed[0]] {
            continue;
        }
        if let Some(quad) = crate::component_geometry::quad(
            image,
            w,
            seed,
            bounds,
            [module * module * 12., module * module * 40.],
        ) {
            return Some(quad);
        }
    }
    None
}

fn recover_one(
    image: &[bool],
    w: usize,
    h: usize,
    finder: &Finder,
    attempts: &mut usize,
    limited: &mut bool,
    alignments: &mut usize,
) -> Option<(Detection, [[f32; 2]; 3])> {
    let quad = finder.quad?;
    if let Some(read) = recover_geometry(
        image, w, h, finder, quad, 3., attempts, limited, alignments, false,
    ) {
        return Some(read);
    }
    if *attempts >= 64 {
        return None;
    }
    let approximate = frame(quad, 0, false, 3.);
    if let Some(moment_quad) =
        super::ring::ring_square_quad(image, w, h, [approximate[4], approximate[5]], approximate)
    {
        if let Some(read) = recover_geometry(
            image,
            w,
            h,
            finder,
            moment_quad,
            7.,
            attempts,
            limited,
            alignments,
            true,
        ) {
            return Some(read);
        }
    }
    if *attempts >= 64 {
        return None;
    }
    if let Some(outer) = outer_quad(image, w, h, finder) {
        if let Some(read) = recover_geometry(
            image, w, h, finder, outer, 7., attempts, limited, alignments, true,
        ) {
            return Some(read);
        }
    }
    if *attempts >= 64 {
        return None;
    }
    recover_geometry(
        image, w, h, finder, quad, 3., attempts, limited, alignments, true,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "This scanner entry point carries image data plus shared result, finder-use, and bounded recovery state without allocating a per-frame context."
)]
pub(super) fn recover(
    image: &[bool],
    w: usize,
    h: usize,
    finders: &[Finder],
    results: &mut Vec<Detection>,
    used: &mut Vec<Finder>,
    attempts: &mut usize,
    alignments: &mut usize,
) -> bool {
    let mut limited = false;
    for finder in finders {
        if used
            .iter()
            .any(|prior| distance(finder, prior) < finder.module.min(prior.module) * 2.)
        {
            continue;
        }
        if let Some((read, centers)) =
            recover_one(image, w, h, finder, attempts, &mut limited, alignments)
        {
            results.push(read);
            used.push(finder.clone());
            // Only payload-validated symbols claim their other physical
            // finder centers. This prevents re-reading the same QR from its
            // other intact corner, while separate nearby QR codes survive.
            for other in finders {
                if centers
                    .iter()
                    .any(|p| (p[0] - other.x).hypot(p[1] - other.y) < other.module * 2.)
                    && !used
                        .iter()
                        .any(|prior| distance(other, prior) < other.module.min(prior.module) * 2.)
                {
                    used.push(other.clone());
                }
            }
        }
        if *attempts >= 64 {
            break;
        }
    }
    limited
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_metadata_matches_known_codewords_and_rejects_bit_errors() {
        assert_eq!(version_word(7), 0x07c94);
        assert_eq!(version_word(8), 0x085bc);
        assert_eq!(version_word(40), 0x28c69);
        for (i, &word) in WORDS.iter().enumerate() {
            assert_eq!(version(word), Some(i + 7));
            for bit in 0..18 {
                assert_eq!(version(word ^ (1 << bit)), None);
            }
        }
    }
}
