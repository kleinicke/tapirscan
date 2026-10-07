//! Display outlines for reads whose symbol area could not be grown. Never ownership evidence.
use super::{Evidence, distance, line};
use crate::Quad;

fn point(origin: [f64; 2], u: [f64; 2], x: f64, y: f64) -> [f64; 2] {
    [
        origin[0] + x * u[0] - y * u[1],
        origin[1] + x * u[1] + y * u[0],
    ]
}
fn project(p: [f64; 2], origin: [f64; 2], u: [f64; 2]) -> [f64; 2] {
    let d = [p[0] - origin[0], p[1] - origin[1]];
    [d[0] * u[0] + d[1] * u[1], -d[0] * u[1] + d[1] * u[0]]
}

#[path = "footprint/display.rs"]
mod display;

pub(super) fn display(evidence: &mut Evidence<'_>, quad: Quad) -> Option<Quad> {
    display::measure(evidence, quad)
}

#[cfg(test)]
mod display_tests {
    use super::*;
    #[test]
    fn profile_stops_at_a_one_pixel_separator() {
        for scale in [1, 4] {
            let (width, height) = (320 * scale, 260 * scale);
            let mut pixels = vec![240; width * height];
            for y in 20 * scale..180 * scale {
                for x in 60 * scale..252 * scale {
                    if (x - 60 * scale) / (3 * scale) % 3 == 0 {
                        pixels[y * width + x] = 20;
                    }
                }
            }
            let q = [[60., 68.], [252., 68.], [252., 72.], [60., 72.]]
                .map(|p| p.map(|v| v * f64::from(u32::try_from(scale).unwrap())));
            let run = |p: &[u8]| {
                let mut e = Evidence {
                    image: crate::Image {
                        data: p,
                        width,
                        height,
                        channels: 1,
                        stride: width,
                    },
                    remaining: 262_144,
                };
                display(&mut e, q).unwrap()
            };
            let full = run(&pixels);
            assert!(
                full[0][1] < 23. * f64::from(u32::try_from(scale).unwrap())
                    && full[2][1] > 177. * f64::from(u32::try_from(scale).unwrap()),
                "{full:?}"
            );
            pixels[120 * scale * width..(120 * scale + 1) * width].fill(240);
            let split = run(&pixels);
            assert!(
                split[2][1] < 121. * f64::from(u32::try_from(scale).unwrap()),
                "{split:?}"
            );
            let reversed = [q[3], q[2], q[1], q[0]];
            let mut e = Evidence {
                image: crate::Image {
                    data: &pixels,
                    width,
                    height,
                    channels: 1,
                    stride: width,
                },
                remaining: 262_144,
            };
            assert_eq!(display(&mut e, reversed).unwrap(), split);
        }
    }
}
