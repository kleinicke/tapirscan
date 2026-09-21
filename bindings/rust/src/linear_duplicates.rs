//! Bounded source-pixel evidence for consolidating bands of one linear symbol.
use crate::{Image, Quad};
use serde_json::{json, Value};

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0].midpoint(b[0]), a[1].midpoint(b[1])]
}
fn line(q: Quad) -> [[f64; 2]; 2] {
    [midpoint(q[0], q[3]), midpoint(q[1], q[2])]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
struct Evidence<'a> {
    image: Image<'a>,
    remaining: usize,
}
impl Evidence<'_> {
    // Coordinates are checked against validated image dimensions before conversion.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn profile(&mut self, left: [f64; 2], right: [f64; 2]) -> Option<u64> {
        if self.remaining < 64 {
            return None;
        }
        self.remaining -= 64;
        let mut values = [0.; 64];
        let (mut lo, mut hi) = (255_f64, 0_f64);
        for (i, value) in values.iter_mut().enumerate() {
            let f = (f64::from(u32::try_from(i).ok()?) + 0.5) / 64.;
            // Match Math.round, including negative half-integers.
            let x = (left[0] + (right[0] - left[0]) * f + 0.5).floor();
            let y = (left[1] + (right[1] - left[1]) * f + 0.5).floor();
            let width = f64::from(u32::try_from(self.image.width).ok()?);
            let height = f64::from(u32::try_from(self.image.height).ok()?);
            if x < 0. || y < 0. || x >= width || y >= height {
                return None;
            }
            let offset = y as usize * self.image.stride + x as usize * self.image.channels;
            let data = self.image.data;
            *value = if self.image.channels == 1 {
                f64::from(data[offset])
            } else {
                (77. * f64::from(data[offset])
                    + 150. * f64::from(data[offset + 1])
                    + 29. * f64::from(data[offset + 2]))
                    / 256.
            };
            lo = lo.min(*value);
            hi = hi.max(*value);
        }
        if hi - lo < 24. {
            return None;
        }
        let threshold = lo.midpoint(hi);
        Some(values.iter().enumerate().fold(0, |bits, (i, value)| {
            if *value < threshold {
                bits | (1 << i)
            } else {
                bits
            }
        }))
    }

    fn connected(&mut self, a: Quad, mut b: Quad) -> Option<Quad> {
        if !a.iter().chain(&b).flatten().all(|v| v.is_finite()) {
            return None;
        }
        let al = line(a);
        let bl = line(b);
        let ax = al[1][0] - al[0][0];
        let ay = al[1][1] - al[0][1];
        let mut bx = bl[1][0] - bl[0][0];
        let mut by = bl[1][1] - bl[0][1];
        let aw = ax.hypot(ay);
        let bw = bx.hypot(by);
        if aw < 24. || !(0.9..=1.1).contains(&(bw / aw)) {
            return None;
        }
        if ax * bx + ay * by < 0. {
            b = [b[2], b[3], b[0], b[1]];
            bx = -bx;
            by = -by;
        }
        if (ax * bx + ay * by) / (aw * bw) < 0.996 {
            return None;
        }
        let ac = midpoint(al[0], al[1]);
        let bl = line(b);
        let bc = midpoint(bl[0], bl[1]);
        if ((bc[0] - ac[0]) * ax + (bc[1] - ac[1]) * ay).abs() / aw > aw * 0.06 {
            return None;
        }
        let steps = distance(bl[0], al[0]).max(distance(bl[1], al[1])).ceil();
        if !(1.0..=384.0).contains(&steps) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "Checked finite integer step count in 1..=384."
        )]
        let steps = steps as u32;
        if usize::try_from(steps + 2).ok()? * 64 > self.remaining {
            return None;
        }
        let reference = self.profile(al[0], al[1])?;
        let (mut dark, mut light) = (reference, !reference);
        let minimum_dark = 4.max(dark.count_ones().div_ceil(4));
        let minimum_light = 4.max(light.count_ones().div_ceil(4));
        for step in 1..=steps {
            let f = f64::from(step) / f64::from(steps);
            let left = [
                al[0][0] + (bl[0][0] - al[0][0]) * f,
                al[0][1] + (bl[0][1] - al[0][1]) * f,
            ];
            let right = [
                al[1][0] + (bl[1][0] - al[1][0]) * f,
                al[1][1] + (bl[1][1] - al[1][1]) * f,
            ];
            let sample = self.profile(left, right)?;
            if (reference ^ sample).count_ones() > 12 {
                return None;
            }
            dark &= sample;
            light &= !sample;
            if dark.count_ones() < minimum_dark || light.count_ones() < minimum_light {
                return None;
            }
        }
        let along = |p: [f64; 2]| (-ay * p[0] + ax * p[1]) / aw;
        let top = if along(midpoint(a[0], a[1])) < along(midpoint(b[0], b[1])) {
            a
        } else {
            b
        };
        let bottom = if along(midpoint(a[2], a[3])) > along(midpoint(b[2], b[3])) {
            a
        } else {
            b
        };
        Some([top[0], top[1], bottom[2], bottom[3]])
    }
}
/// Algorithm inputs are typed; opaque payloads retain reader-specific evidence.
struct Read<T> {
    text: String,
    format: String,
    addon: Option<String>,
    gs1: bool,
    reader_initialization: bool,
    support: u64,
    polygon: Quad,
    geometry_changed: bool,
    payload: T,
}
impl<T> Read<T> {
    fn supported(&self) -> bool {
        !self.text.is_empty()
            && matches!(
                self.format.as_str(),
                "EAN13" | "UPCA" | "EAN8" | "UPCE" | "Code128" | "Code39" | "ITF"
            )
    }
    fn same_symbol(&self, other: &Self) -> bool {
        self.text == other.text
            && self.format == other.format
            && self.addon == other.addon
            && self.gs1 == other.gs1
            && self.reader_initialization == other.reader_initialization
    }
}

fn consolidate<T>(mut reads: Vec<Read<T>>, image: Image<'_>) -> Vec<Read<T>> {
    if !reads.iter().enumerate().any(|(i, a)| {
        a.supported()
            && reads[..i]
                .iter()
                .any(|b| a.text == b.text && a.format == b.format)
    }) {
        return reads;
    }
    reads.sort_by_key(|b| std::cmp::Reverse(b.support));
    let mut evidence = Evidence {
        image,
        remaining: 32768,
    };
    let mut result: Vec<Read<T>> = Vec::new();
    for read in reads {
        let mut merged = false;
        if read.supported() && evidence.remaining >= 192 {
            for other in &mut result {
                if !read.same_symbol(other) {
                    continue;
                }
                if crate::formats::overlap_quads(&other.polygon, &read.polygon).0 >= 0.65 {
                    merged = true;
                    break;
                }
                if let Some(polygon) = evidence.connected(other.polygon, read.polygon) {
                    other.polygon = polygon;
                    other.geometry_changed = true;
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            result.push(read);
        }
    }
    result
}

/// Decode the existing JSON boundary once; preserve all unrecognized metadata.
pub(crate) fn merge(reads: Vec<Value>, image: Image<'_>) -> Vec<Value> {
    let reads = reads
        .into_iter()
        .map(|value| Read {
            text: value["text"].as_str().unwrap_or_default().to_owned(),
            format: value["format"].as_str().unwrap_or_default().to_owned(),
            addon: value["eanAddOn"].as_str().map(str::to_owned),
            gs1: value["gs1"].as_bool().unwrap_or(false),
            reader_initialization: value["readerInitialization"].as_bool().unwrap_or(false),
            support: value["support"].as_u64().unwrap_or(0),
            polygon: crate::formats::quad(&value),
            geometry_changed: false,
            payload: value,
        })
        .collect();
    consolidate(reads, image)
        .into_iter()
        .map(|mut read| {
            if read.geometry_changed {
                read.payload["polygon"] = json!(read.polygon);
            }
            read.payload
        })
        .collect()
}

pub(crate) fn merge_primary(reads: &mut Vec<crate::Barcode>, image: Image<'_>) {
    if reads.len() < 2 {
        return;
    }
    let typed = std::mem::take(reads)
        .into_iter()
        .map(|read| Read {
            text: read
                .detection
                .digits
                .iter()
                .map(|d| char::from(b'0' + d))
                .collect(),
            format: "EAN13".to_owned(),
            addon: None,
            gs1: false,
            reader_initialization: false,
            support: u64::try_from(read.detection.support).unwrap_or(u64::MAX),
            polygon: read.detection.polygon,
            geometry_changed: false,
            payload: read,
        })
        .collect();
    *reads = consolidate(typed, image)
        .into_iter()
        .map(|mut read| {
            read.payload.detection.polygon = read.polygon;
            read.payload
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_bands_merge_but_separators_and_supplements_preserve_products() {
        let mut pixels = vec![255; 460 * 460];
        for y in 20..420 {
            for x in 60..252 {
                pixels[y * 460 + x] = if (x - 60) / 3 % 3 == 0 { 20 } else { 220 };
            }
        }
        let read = |lo, hi| json!({"text":"4006381333931","format":"EAN13","support":7,"polygon":[[60,lo],[252,lo],[252,hi],[60,hi]]});
        let reads = vec![read(30, 120), read(280, 400)];
        let scan = |pixels: &[u8], reads| {
            merge(
                reads,
                Image {
                    data: pixels,
                    width: 460,
                    height: 460,
                    channels: 1,
                    stride: 460,
                },
            )
        };
        assert_eq!(scan(&pixels, reads.clone()).len(), 1);
        let mut supplements = reads.clone();
        supplements[0]["eanAddOn"] = json!("12");
        supplements[1]["eanAddOn"] = json!("34");
        assert_eq!(scan(&pixels, supplements).len(), 2);
        pixels[200 * 460..202 * 460].fill(255);
        assert_eq!(scan(&pixels, reads).len(), 2);
    }
}
