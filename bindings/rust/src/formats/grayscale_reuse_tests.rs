use super::{gray_image, Image};
#[test]
fn specialized_luminance_matches_scalar_pixels_and_short_final_rows() {
    for channels in [1, 3, 4] {
        for padding in [0, 1, 7] {
            let (width, height) = (17, 9);
            let stride = width * channels + padding;
            let data: Vec<u8> = (0..((height - 1) * stride + width * channels))
                .map(|i| u8::try_from((i * 73 + 19) % 256).unwrap())
                .collect();
            let image = Image {
                data: &data,
                width,
                height,
                channels,
                stride,
            };
            let mut gray = vec![123; 300];
            gray_image(image, &mut gray).unwrap();
            for y in 0..height {
                for x in 0..width {
                    let offset = y * stride + x * channels;
                    let expected = if channels == 1 {
                        data[offset]
                    } else {
                        u8::try_from(
                            (u32::from(data[offset]) * 77
                                + u32::from(data[offset + 1]) * 150
                                + u32::from(data[offset + 2]) * 29
                                + 128)
                                >> 8,
                        )
                        .unwrap()
                    };
                    assert_eq!(gray[y * width + x], expected);
                }
            }
            assert_eq!(gray.len(), width * height);
        }
    }
}

#[test]
fn packed_luminance_matches_every_rgb_color() {
    for channels in [3, 4] {
        let width = 256 * 256;
        let mut data = vec![0; width * channels];
        let mut gray = Vec::new();
        for red in 0_u32..256 {
            for green in 0_u32..256 {
                for blue in 0_u32..256 {
                    let at = usize::try_from(green * 256 + blue).unwrap() * channels;
                    data[at] = u8::try_from(red).unwrap();
                    data[at + 1] = u8::try_from(green).unwrap();
                    data[at + 2] = u8::try_from(blue).unwrap();
                    if channels == 4 {
                        data[at + 3] = u8::try_from((red + green + blue) % 256).unwrap();
                    }
                }
            }
            gray_image(
                Image {
                    data: &data,
                    width,
                    height: 1,
                    channels,
                    stride: width * channels,
                },
                &mut gray,
            )
            .unwrap();
            for (index, &value) in gray.iter().enumerate() {
                let green = u32::try_from(index / 256).unwrap();
                let blue = u32::try_from(index % 256).unwrap();
                assert_eq!(
                    u32::from(value),
                    (red * 77 + green * 150 + blue * 29 + 128) >> 8
                );
            }
        }
    }
}
#[test]
fn color_then_padded_gray_reuses_storage_without_stale_pixels() {
    let mut gray = Vec::with_capacity(64);
    let pointer = gray.as_ptr();
    let color = [0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 0];
    gray_image(
        Image {
            data: &color,
            width: 2,
            height: 2,
            channels: 3,
            stride: 6,
        },
        &mut gray,
    )
    .unwrap();
    assert_eq!(gray, [0, 255, 77, 149]);
    let padded = [5, 9, 99, 99, 4, 3, 88, 88];
    gray_image(
        Image {
            data: &padded,
            width: 2,
            height: 2,
            channels: 1,
            stride: 4,
        },
        &mut gray,
    )
    .unwrap();
    assert_eq!(gray, [5, 9, 4, 3]);
    assert_eq!(gray.as_ptr(), pointer);
}
