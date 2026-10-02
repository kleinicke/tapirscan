//! Extra admission proof for Very High's larger unchecked recovery allowance.
use crate::{
    geometry::lerp,
    read::{Read, Region},
    Image, ImageView, Quad,
};
use barcode_research_core::{fast_profile::Sampler, numeric::usize_f64};

pub(crate) fn append_recovered(
    reads: Vec<Read>,
    additions: Vec<Read>,
    image: Image<'_>,
    sampler: &mut Sampler,
    strength: f32,
) -> (Vec<Read>, Vec<Region>) {
    let (additions, conflicts) = filter(&reads, additions, image, sampler);
    let (reads, mut pending) =
        crate::fast_linear::append_recovered(reads, additions, image, sampler, strength);
    pending.extend(conflicts);
    (reads, pending)
}

fn filter(
    established: &[Read],
    additions: Vec<Read>,
    image: Image<'_>,
    sampler: &mut Sampler,
) -> (Vec<Read>, Vec<Region>) {
    let Ok(im) = ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    ) else {
        return (additions, Vec::new());
    };
    let mut budget = 32_768usize;
    let mut accepted = Vec::new();
    let mut pending = Vec::new();
    for read in additions {
        let conflict = matches!(read.format.as_str(), "ITF" | "Code39")
            && established.iter().any(|prior| {
                let mask = match prior.format.as_str() {
                    "EAN13" => 1,
                    "UPCA" => 2,
                    "EAN8" => 4,
                    "UPCE" => 8,
                    _ => return false,
                };
                crate::geometry::overlap_quads(&read.polygon, &prior.polygon).0 > 0.
                    && [0.25, 0.5, 0.75]
                        .into_iter()
                        .all(|v| confirms(im, read.polygon, v, prior, mask, sampler, &mut budget))
            });
        if conflict {
            pending.push(Region {
                format: read.format,
                text: read.text,
                polygon: read.polygon,
                support: read.support,
                localization_score: None,
            });
        } else {
            accepted.push(read);
        }
    }
    (accepted, pending)
}

fn confirms(
    im: ImageView<'_>,
    q: Quad,
    v: f64,
    expected: &Read,
    mask: u32,
    sampler: &mut Sampler,
    budget: &mut usize,
) -> bool {
    if *budget < 1536 {
        return false;
    }
    let left = lerp(q[0], q[3], v);
    let right = lerp(q[1], q[2], v);
    sampler.sample_limited(
        im,
        lerp(left, right, -0.15),
        lerp(left, right, 1.15),
        3.,
        1536,
    );
    *budget = budget.saturating_sub(sampler.samples);
    if !sampler.has_contrast() {
        return false;
    }
    // Verify the established checksum-valid retail value on the new claim's
    // own source rows and complete width. Mere overlapping boxes are insufficient.
    for method in 0..3 {
        if method == 1 {
            sampler.adaptive_threshold();
        } else {
            sampler.threshold(method == 0);
        }
        for reverse in [false, true] {
            if reverse {
                sampler.runs.reverse();
            }
            let first = sampler.first_black ^ (reverse && sampler.runs.len().is_multiple_of(2));
            let decoded = barcode_multiformat::linear::decode(&sampler.runs, first, mask);
            if reverse {
                sampler.runs.reverse();
            }
            for read in decoded {
                if !read.decoded || read.format != expected.format || read.text != expected.text {
                    continue;
                }
                let (a, b) = if reverse {
                    (
                        sampler.runs.len() - read.end,
                        sampler.runs.len() - read.start,
                    )
                } else {
                    (read.start, read.end)
                };
                let a = -0.15 + 1.3 * f64::from(sampler.edges[a]) / usize_f64(sampler.samples - 1);
                let b = -0.15 + 1.3 * f64::from(sampler.edges[b]) / usize_f64(sampler.samples - 1);
                if a.abs() <= 0.035 && (b - 1.).abs() <= 0.035 {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defers_source_verified_retail_alias_but_preserves_separate_claims() {
        let bits = b"1010001011010111101111010110111010101001110111001010001001011100101";
        let width = 300;
        let mut pixels = vec![255; width * 160];
        for y in 20..80 {
            for (i, bit) in bits.iter().enumerate() {
                for dx in 0..3 {
                    pixels[y * width + 40 + i * 3 + dx] = if *bit == b'1' { 0 } else { 255 };
                }
            }
        }
        let q = [[40., 20.], [241., 20.], [241., 79.], [40., 79.]];
        let mut retail = Read::primary([0; 13], q, 7, 0, vec![]);
        retail.format = "EAN8".into();
        retail.text = "96385074".into();
        let mut alias = retail.clone();
        alias.format = "ITF".into();
        alias.text = "8888297522".into();
        let image = Image {
            data: &pixels,
            width,
            height: 160,
            channels: 1,
            stride: width,
        };
        let mut sampler = Sampler::default();
        let (reads, pending) = filter(&[retail.clone()], vec![alias.clone()], image, &mut sampler);
        assert!(reads.is_empty());
        assert_eq!(pending.len(), 1);
        for point in &mut alias.polygon {
            point[1] += 80.;
        }
        let (reads, pending) = filter(&[retail], vec![alias], image, &mut sampler);
        assert_eq!(reads.len(), 1);
        assert!(pending.is_empty());
    }
}
