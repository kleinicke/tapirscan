//! Bounded source-pixel evidence for consolidating bands of one linear symbol.
use crate::{Image, Quad};

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0].midpoint(b[0]), a[1].midpoint(b[1])]
}
fn line(q: Quad) -> [[f64; 2]; 2] {
    [midpoint(q[0], q[3]), midpoint(q[1], q[2])]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
struct Profile {
    bits: u64,
    dark: u64,
    light: u64,
}
struct Evidence<'a> {
    image: Image<'a>,
    remaining: usize,
}
impl Evidence<'_> {
    // Coordinates are checked against validated image dimensions before conversion.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn profile(&mut self, left: [f64; 2], right: [f64; 2]) -> Option<Profile> {
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
        let mut result = Profile {
            bits: 0,
            dark: 0,
            light: 0,
        };
        for (i, value) in values.iter().enumerate() {
            if *value < threshold {
                result.bits |= 1 << i;
            }
            if *value < lo + (hi - lo) * 0.25 {
                result.dark |= 1 << i;
            }
            if *value > lo + (hi - lo) * 0.75 {
                result.light |= 1 << i;
            }
        }
        Some(result)
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
        let reference = self.profile(al[0], al[1])?.bits;
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
            let sample = self.profile(left, right)?.bits;
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
    fn connected_warped(&mut self, a: Quad, mut b: Quad) -> Option<Quad> {
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
        if aw < 24. || !(0.7..=1.3).contains(&(bw / aw)) {
            return None;
        }
        if ax * bx + ay * by < 0. {
            b = [b[2], b[3], b[0], b[1]];
            bx = -bx;
            by = -by;
        }
        if (ax * bx + ay * by) / (aw * bw) < 0.8 {
            return None;
        }
        let ac = midpoint(al[0], al[1]);
        let bl = line(b);
        let bc = midpoint(bl[0], bl[1]);
        if ((bc[0] - ac[0]) * ax + (bc[1] - ac[1]) * ay).abs() / aw > aw * 0.12 {
            return None;
        }
        let steps = (2. * distance(bl[0], al[0]).max(distance(bl[1], al[1]))).ceil();
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
        let (mut dark, mut light) = (reference.dark, reference.light);
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
            let disagreement = (reference.bits ^ sample.bits).count_ones();
            if disagreement > 20 {
                return None;
            }
            dark &= sample.dark;
            light &= sample.light;
            if dark.count_ones() < minimum_dark || light.count_ones() < minimum_light {
                return None;
            }
        }
        {
            // Persistent ink and paper must be distributed across the symbol.
            if (0..4).any(|i| {
                (dark >> (16 * i)).trailing_zeros() >= 16
                    || (light >> (16 * i)).trailing_zeros() >= 16
            }) {
                return None;
            }
            let area = |q: Quad| {
                (0..4)
                    .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1])
                    .sum::<f64>()
                    .abs()
            };
            Some(if area(a) >= area(b) { a } else { b })
        }
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Finite source coordinates are bounds checked before indexing."
    )]
    fn smooth(&mut self, p: [f64; 2]) -> Option<f64> {
        if self.remaining < 4 || !p.iter().all(|v| v.is_finite()) {
            return None;
        }
        self.remaining -= 4;
        let width = f64::from(u32::try_from(self.image.width).ok()?);
        let height = f64::from(u32::try_from(self.image.height).ok()?);
        if p[0] < 0. || p[1] < 0. || p[0] >= width - 1. || p[1] >= height - 1. {
            return None;
        }
        let (x, y) = (p[0].floor() as usize, p[1].floor() as usize);
        let gray = |x: usize, y: usize| {
            let at = y * self.image.stride + x * self.image.channels;
            let d = self.image.data;
            if self.image.channels == 1 {
                f64::from(d[at])
            } else {
                (77. * f64::from(d[at]) + 150. * f64::from(d[at + 1]) + 29. * f64::from(d[at + 2]))
                    / 256.
            }
        };
        let (fx, fy) = (p[0] - p[0].floor(), p[1] - p[1].floor());
        Some(
            (gray(x, y) * (1. - fx) + gray(x + 1, y) * fx) * (1. - fy)
                + (gray(x, y + 1) * (1. - fx) + gray(x + 1, y + 1) * fx) * fy,
        )
    }

    fn trace_bar(
        &mut self,
        from: [f64; 2],
        target: [f64; 2],
        normal: [f64; 2],
        width: f64,
        cut: f64,
        endpoint_tolerance: f64,
    ) -> bool {
        let length = distance(from, target);
        if !(1.0..=96.).contains(&length) {
            return false;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "Bounded positive path length at most 96 pixels."
        )]
        // Half-pixel steps still inspect one-pixel white separators. Spend the
        // saved samples on light borders that widen or narrow along a fold.
        let steps = (length * 2.).ceil() as u32;
        let delta = [
            (target[0] - from[0]) / f64::from(steps),
            (target[1] - from[1]) / f64::from(steps),
        ];
        let mut point = from;
        for _ in 0..steps {
            let predicted = [point[0] + delta[0], point[1] + delta[1]];
            let Some(mut value) = self.smooth(predicted) else {
                return false;
            };
            point = predicted;
            for shift in [-0.25, 0.25] {
                let q = [
                    predicted[0] + shift * normal[0],
                    predicted[1] + shift * normal[1],
                ];
                let Some(v) = self.smooth(q) else {
                    return false;
                };
                if v < value {
                    value = v;
                    point = q;
                }
            }
            if value >= cut {
                return false;
            }
            for sign in [-1., 1.] {
                let mut bracketed = false;
                for factor in [0.5, 0.8, 1.1] {
                    let radius = sign * (width * factor + 0.7);
                    let Some(paper) =
                        self.smooth([point[0] + radius * normal[0], point[1] + radius * normal[1]])
                    else {
                        return false;
                    };
                    if paper - value >= 24. {
                        bracketed = true;
                        break;
                    }
                }
                if !bracketed {
                    return false;
                }
            }
        }
        // Folded labels need not map equal box fractions onto exact bar centers.
        // Endpoint slack never skips the continuous ink and light-border checks.
        distance(point, target) <= (width * 0.75).max(1.5).max(endpoint_tolerance)
    }

    fn connected_traces(&mut self, a: Quad, mut b: Quad) -> Option<Quad> {
        let al = line(a);
        let mut bl = line(b);
        let av = [al[1][0] - al[0][0], al[1][1] - al[0][1]];
        let mut bv = [bl[1][0] - bl[0][0], bl[1][1] - bl[0][1]];
        let aw = av[0].hypot(av[1]);
        let bw = bv[0].hypot(bv[1]);
        if aw < 48. || !(0.7..=1.3).contains(&(bw / aw)) {
            return None;
        }
        if av[0] * bv[0] + av[1] * bv[1] < 0. {
            b = [b[2], b[3], b[0], b[1]];
            bl = line(b);
            bv = [-bv[0], -bv[1]];
        }
        if (av[0] * bv[0] + av[1] * bv[1]) / (aw * bw) < 0.8 {
            return None;
        }
        let normal = [av[0] / aw, av[1] / aw];
        let ac = midpoint(al[0], al[1]);
        let bc = midpoint(bl[0], bl[1]);
        if ((bc[0] - ac[0]) * normal[0] + (bc[1] - ac[1]) * normal[1]).abs() > aw * 0.08 {
            return None;
        }
        let mut values = [0.; 256];
        let (mut low, mut high) = (255_f64, 0_f64);
        for (i, v) in values.iter_mut().enumerate() {
            let f = (f64::from(u32::try_from(i).ok()?) + 0.5) / 256.;
            *v = self.smooth([al[0][0] + f * av[0], al[0][1] + f * av[1]])?;
            low = low.min(*v);
            high = high.max(*v);
        }
        if high - low < 48. {
            return None;
        }
        let threshold = low.midpoint(high);
        let cut = low.midpoint(high);
        let mut bars = Vec::new();
        let mut i = 1usize;
        while i < 255 {
            if values[i] >= threshold || values[i - 1] < threshold {
                i += 1;
                continue;
            }
            let start = i;
            while i < 255 && values[i] < threshold {
                i += 1;
            }
            let width = f64::from(u32::try_from(i - start).ok()?) * aw / 256.;
            if i < 255 && (0.8..=aw * 0.07).contains(&width) {
                bars.push((
                    (f64::from(u32::try_from(start + i).ok()?) * 0.5) / 256.,
                    width,
                ));
            }
        }
        if bars.len() < 8 {
            return None;
        }
        let mut matched = Vec::new();
        for index in 0..8 {
            let (f, width) = bars[index * (bars.len() - 1) / 7];
            let from = [al[0][0] + f * av[0], al[0][1] + f * av[1]];
            let to = [bl[0][0] + f * bv[0], bl[0][1] + f * bv[1]];
            if self.trace_bar(from, to, normal, width, cut, aw * 0.08) {
                matched.push(f);
            }
        }
        if matched.len() < 6 || matched.last()? - matched.first()? < 0.6 {
            return None;
        }
        let area = |q: Quad| {
            (0..4)
                .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[(i + 1) % 4][0] * q[i][1])
                .sum::<f64>()
                .abs()
        };
        Some(if area(a) >= area(b) { a } else { b })
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
                if crate::geometry::overlap_quads(&other.polygon, &read.polygon).0 >= 0.65 {
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
    let mut extended: Vec<Read<T>> = Vec::new();
    for read in result {
        let mut merged = false;
        if read.supported() {
            for other in &mut extended {
                if !read.same_symbol(other) {
                    continue;
                }
                if let Some(polygon) = evidence
                    .connected_warped(other.polygon, read.polygon)
                    .or_else(|| evidence.connected_traces(other.polygon, read.polygon))
                {
                    other.polygon = polygon;
                    other.geometry_changed = true;
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            extended.push(read);
        }
    }
    extended
}

/// Reconcile typed evidence while preserving reader metadata and stable ties.
pub(crate) fn merge(reads: Vec<crate::read::Read>, image: Image<'_>) -> Vec<crate::read::Read> {
    let reads = reads
        .into_iter()
        .map(|value| Read {
            text: value.text.clone(),
            format: value.format.clone(),
            addon: value.addon.clone(),
            gs1: value.gs1.unwrap_or(false),
            reader_initialization: value.reader_initialization.unwrap_or(false),
            support: value.support,
            polygon: value.polygon,
            geometry_changed: false,
            payload: value,
        })
        .collect();
    consolidate(reads, image)
        .into_iter()
        .map(|mut read| {
            read.payload.polygon = read.polygon;
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
        let read = |lo, hi| {
            crate::read::Read::primary(
                [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                [[60., lo], [252., lo], [252., hi], [60., hi]],
                7,
                0,
                vec![],
            )
        };
        let reads = vec![read(30., 120.), read(280., 400.)];
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
        supplements[0].addon = Some("12".into());
        supplements[1].addon = Some("34".into());
        assert_eq!(scan(&pixels, supplements).len(), 2);
        pixels[200 * 460..202 * 460].fill(255);
        assert_eq!(scan(&pixels, reads).len(), 2);
    }
    #[test]
    fn crossing_bands_share_ink_but_an_oblique_separator_breaks_the_proof() {
        let mut pixels = vec![255; 320 * 260];
        for y in 20..240 {
            for x in 60..252 {
                pixels[y * 320 + x] = if (x - 60) / 3 % 3 == 0 { 20 } else { 220 };
            }
        }
        let a = [[60., 125.], [252., 125.], [252., 140.], [60., 140.]];
        let b = [[60., 75.], [252., 155.], [252., 180.], [60., 100.]];
        let proof = |pixels: &[u8], a, b| {
            Evidence {
                image: Image {
                    data: pixels,
                    width: 320,
                    height: 260,
                    channels: 1,
                    stride: 320,
                },
                remaining: 32768,
            }
            .connected_warped(a, b)
        };
        assert!(proof(&pixels, a, b).is_some());
        assert!(proof(&pixels, b, a).is_some());
        let trace = |pixels: &[u8], a, b| {
            Evidence {
                image: Image {
                    data: pixels,
                    width: 320,
                    height: 260,
                    channels: 1,
                    stride: 320,
                },
                remaining: 32768,
            }
            .connected_traces(a, b)
        };
        assert!(trace(&pixels, a, b).is_some());
        let near_upper = [[60., 50.], [252., 50.], [252., 70.], [60., 70.]];
        let near_lower = [[60., 120.], [252., 120.], [252., 140.], [60., 140.]];
        assert!(trace(&pixels, near_upper, near_lower).is_some());
        // Two disjoint equal-value labels separated by an oblique one-pixel gap.
        let upper = [[60., 30.], [252., 55.], [252., 95.], [60., 70.]];
        let lower = [[60., 165.], [252., 185.], [252., 225.], [60., 205.]];
        assert!(proof(&pixels, upper, lower).is_some());
        for x in 60..252 {
            let y = 110 + (x - 60) / 8;
            pixels[y * 320 + x] = 255;
        }
        assert!(proof(&pixels, upper, lower).is_none());
        assert!(trace(&pixels, near_upper, near_lower).is_none());
    }
    #[test]
    fn changing_bar_width_keeps_continuity_but_white_cuts_break_it() {
        let mut pixels = vec![220; 320 * 180];
        for y in 30_usize..160 {
            let width = (5 + y.saturating_sub(70) / 20).min(7);
            for x in 40..280 {
                if (x - 40) % 12 < width {
                    pixels[y * 320 + x] = 20;
                }
            }
        }
        let a = [[40., 77.], [280., 77.], [280., 83.], [40., 83.]];
        let b = [[40., 125.], [280., 125.], [280., 131.], [40., 131.]];
        let proof = |pixels: &[u8]| {
            Evidence {
                image: Image {
                    data: pixels,
                    width: 320,
                    height: 180,
                    channels: 1,
                    stride: 320,
                },
                remaining: 32768,
            }
            .connected_traces(a, b)
        };
        assert!(proof(&pixels).is_some());
        pixels[100 * 320..101 * 320].fill(255);
        assert!(proof(&pixels).is_none());
    }
}
