#[cfg(not(feature = "low"))]
use super::restoration::restore_with_kernel;
use super::*;

// The original per-tap implementation, kept as an exact reference.
#[cfg(not(feature = "low"))]
fn reference_restore(image: Image<'_>, weights: &[u32]) -> Vec<u8> {
    let (w, h) = (image.width, image.height);
    let gray: Vec<u32> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let at = y * image.stride + x * image.channels;
                if image.channels == 1 {
                    u32::from(image.data[at])
                } else {
                    (77 * u32::from(image.data[at])
                        + 150 * u32::from(image.data[at + 1])
                        + 29 * u32::from(image.data[at + 2])
                        + 128)
                        / 256
                }
            })
        })
        .collect();
    let center = weights.len() / 2;
    let normalizer = weights.iter().sum::<u32>().pow(2);
    let mut horizontal = vec![0_u32; w * h];
    for y in 0..h {
        for x in 0..w {
            horizontal[y * w + x] = weights
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    a * gray[y * w + x.saturating_add(i).saturating_sub(center).min(w - 1)]
                })
                .sum();
        }
    }
    let mut out = vec![0_u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let smooth: u32 = weights
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    a * horizontal[y.saturating_add(i).saturating_sub(center).min(h - 1) * w + x]
                })
                .sum();
            let value = (5 * i32::try_from(gray[y * w + x]).expect("gray")
                - 3 * i32::try_from((smooth + normalizer / 2) / normalizer).expect("smooth")
                + 1)
                / 2;
            out[y * w + x] = u8::try_from(value.clamp(0, 255)).expect("clamped gray");
        }
    }
    out
}

#[cfg(not(feature = "low"))]
#[test]
fn restored_crops_match_reference_filter() {
    for channels in [1, 3, 4] {
        for (width, height) in [(1, 1), (3, 2), (17, 11)] {
            let stride = width * channels + 2;
            let data: Vec<u8> = (0..stride * height)
                .map(|i| u8::try_from(i * 89 % 256).unwrap())
                .collect();
            let image = Image {
                data: &data,
                width,
                height,
                channels,
                stride,
            };
            for weights in [&[1, 4, 6, 4, 1][..], &[1, 4, 11, 21, 26, 21, 11, 4, 1]] {
                assert_eq!(
                    restore_with_kernel(image, weights),
                    reference_restore(image, weights),
                    "{channels} channels, {width}x{height}, {} taps",
                    weights.len()
                );
            }
        }
    }
}

#[test]
fn retail_pixels_match_per_pixel_packing_for_padded_rows() {
    for channels in [1, 3, 4] {
        let (width, height, stride) = (5, 3, 5 * channels + 3);
        let data: Vec<u8> = (0..stride * height)
            .map(|i| u8::try_from(i * 37 % 251).unwrap())
            .collect();
        let image = Image {
            data: &data,
            width,
            height,
            channels,
            stride,
        };
        let mut packed = vec![7; 2];
        retail_pixels(image, &mut packed);
        let mut expected = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let source = y * stride + x * channels;
                let step = usize::from(channels != 1);
                expected.extend([
                    data[source],
                    data[source + step],
                    data[source + 2 * step],
                    255,
                ]);
            }
        }
        assert_eq!(packed, expected, "{channels} channels");
    }
}

#[test]
fn retail_storage_reuse_preserves_stride_channels_and_alpha() {
    let mut storage = Vec::new();
    for (width, height) in [(12, 8), (3, 4), (12, 8)] {
        for channels in [1, 3, 4] {
            let stride = width * channels + 7;
            let mut data = vec![37; stride * height];
            let mut expected = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    let source = y * stride + x * channels;
                    let value = u8::try_from(x + y).unwrap();
                    data[source] = value;
                    if channels != 1 {
                        data[source + 1] = value + 1;
                        data[source + 2] = value + 2;
                    }
                    expected.extend_from_slice(&[
                        value,
                        value + u8::from(channels != 1),
                        value + 2 * u8::from(channels != 1),
                        255,
                    ]);
                }
            }
            data.truncate((height - 1) * stride + width * channels);
            retail_pixels(
                Image {
                    data: &data,
                    width,
                    height,
                    channels,
                    stride,
                },
                &mut storage,
            );
            assert_eq!(storage, expected);
        }
    }
}
