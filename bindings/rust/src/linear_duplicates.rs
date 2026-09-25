//! Bounded source-pixel evidence for consolidating bands of one linear symbol.
use crate::{Image, Quad};
mod footprint;

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0].midpoint(b[0]), a[1].midpoint(b[1])]
}
fn line(q: Quad) -> [[f64; 2]; 2] {
    [midpoint(q[0], q[3]), midpoint(q[1], q[2])]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
// Only finite, nonnegative image luminance enters these hot extrema loops.
#[inline]
fn include_intensity(value: f64, lo: &mut f64, hi: &mut f64) {
    if option_env!("TAPIRSCAN_TURBO_FINITE_EXTREMA").is_some() {
        if value < *lo {
            *lo = value;
        }
        if value > *hi {
            *hi = value;
        }
    } else {
        *lo = lo.min(value);
        *hi = hi.max(value);
    }
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
            *value = self.image.fixed_luminance(offset);
            include_intensity(*value, &mut lo, &mut hi);
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

    // Gap checks only need contrast, not the thresholded profile. For an interior
    // segment, later samples cannot invalidate contrast already found. Charge the
    // original 64-sample budget so this optimization never changes search limits.
    #[cfg(feature = "low")]
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn has_contrast(&mut self, left: [f64; 2], right: [f64; 2]) -> bool {
        let (Ok(width), Ok(height)) = (
            u32::try_from(self.image.width),
            u32::try_from(self.image.height),
        ) else {
            return self.profile(left, right).is_some();
        };
        let interior = |p: [f64; 2]| {
            p[0].is_finite()
                && p[1].is_finite()
                && p[0] >= 0.
                && p[1] >= 0.
                && p[0] < f64::from(width.saturating_sub(1))
                && p[1] < f64::from(height.saturating_sub(1))
        };
        if !interior(left) || !interior(right) {
            return self.profile(left, right).is_some();
        }
        if self.remaining < 64 {
            return false;
        }
        self.remaining -= 64;
        let (mut lo, mut hi) = (255_f64, 0_f64);
        for i in 0..64 {
            let f = (f64::from(i) + 0.5) / 64.;
            let x = (left[0] + (right[0] - left[0]) * f + 0.5).floor() as usize;
            let y = (left[1] + (right[1] - left[1]) * f + 0.5).floor() as usize;
            let offset = y * self.image.stride + x * self.image.channels;
            let value = self.image.fixed_luminance(offset);
            include_intensity(value, &mut lo, &mut hi);
            if hi - lo >= 24. {
                return true;
            }
        }
        false
    }

    #[cfg(feature = "low")]
    fn gap_free(&mut self, a: Quad, b: Quad) -> bool {
        let al = line(a);
        let bl = line(b);
        let count = (distance(al[0], bl[0]).max(distance(al[1], bl[1])) * 2.).ceil();
        if !(1.0..=768.).contains(&count) {
            return false;
        }
        let steps = barcode_research_core::numeric::f64_usize(count);
        for i in 0..=steps {
            let f = barcode_research_core::numeric::usize_f64(i) / count;
            let left = [
                al[0][0] + (bl[0][0] - al[0][0]) * f,
                al[0][1] + (bl[0][1] - al[0][1]) * f,
            ];
            let right = [
                al[1][0] + (bl[1][0] - al[1][0]) * f,
                al[1][1] + (bl[1][1] - al[1][1]) * f,
            ];
            if !self.has_contrast(left, right) {
                return false;
            }
        }
        true
    }
    fn fast_profile(&mut self, left: [f64; 2], right: [f64; 2], smooth: bool) -> Option<Profile> {
        if !smooth {
            return self.profile(left, right);
        }
        let mut values = [0.; 64];
        let (mut lo, mut hi) = (255_f64, 0_f64);
        for (i, v) in values.iter_mut().enumerate() {
            let f = (f64::from(u32::try_from(i).ok()?) + 0.5) / 64.;
            *v = self.smooth([
                left[0] + (right[0] - left[0]) * f,
                left[1] + (right[1] - left[1]) * f,
            ])?;
            include_intensity(*v, &mut lo, &mut hi);
        }
        if hi - lo < 24. {
            return None;
        }
        let mut result = Profile {
            bits: 0,
            dark: 0,
            light: 0,
        };
        for (i, &v) in values.iter().enumerate() {
            if v < lo.midpoint(hi) {
                result.bits |= 1 << i;
            }
            if v < lo + (hi - lo) * 0.25 {
                result.dark |= 1 << i;
            }
            if v > lo + (hi - lo) * 0.75 {
                result.light |= 1 << i;
            }
        }
        Some(result)
    }
    fn connected(&mut self, a: Quad, b: Quad) -> Option<Quad> {
        self.connected_sampling(a, b, false)
    }
    fn connected_sampling(&mut self, a: Quad, mut b: Quad, smooth: bool) -> Option<Quad> {
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
        let reference = self.fast_profile(al[0], al[1], smooth)?.bits;
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
            let sample = self.fast_profile(left, right, smooth)?.bits;
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
            self.image.fixed_luminance(at)
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
    // Prove ownership by following a source bar to the other decoding line.
    // Equal fractions in two warped boxes need not identify the same bar.
    fn trace_to_line(
        &mut self,
        from: [f64; 2],
        target: [[f64; 2]; 2],
        normal: [f64; 2],
        width: f64,
        cut: f64,
    ) -> bool {
        let v = [target[1][0] - target[0][0], target[1][1] - target[0][1]];
        let length = v[0].hypot(v[1]);
        if length < 24. {
            return false;
        }
        let signed =
            |p: [f64; 2]| ((p[0] - target[0][0]) * v[1] - (p[1] - target[0][1]) * v[0]) / length;
        let start = signed(from);
        let tangent = [-normal[1], normal[0]];
        let rate = (tangent[0] * v[1] - tangent[1] * v[0]) / length;
        if rate.abs() < 0.8 || (start / rate).abs() > 384. {
            return false;
        }
        let direction = -(start / rate).signum();
        let mut point = from;
        for _ in 0..768 {
            let d = signed(point);
            if d.abs() <= 0.5 || d * start < 0. {
                let f = ((point[0] - target[0][0]) * v[0] + (point[1] - target[0][1]) * v[1])
                    / (length * length);
                return (0.0..=1.0).contains(&f);
            }
            let predicted = [
                point[0] + 0.5 * direction * tangent[0],
                point[1] + 0.5 * direction * tangent[1],
            ];
            let Some(mut value) = self.smooth(predicted) else {
                return false;
            };
            point = predicted;
            for shift in [-0.5, 0.5] {
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
        false
    }

    fn owned_bars(&mut self, a: Quad, mut b: Quad, partial: bool) -> Option<Quad> {
        let al = line(a);
        let mut bl = line(b);
        let av = [al[1][0] - al[0][0], al[1][1] - al[0][1]];
        let mut bv = [bl[1][0] - bl[0][0], bl[1][1] - bl[0][1]];
        let aw = av[0].hypot(av[1]);
        let bw = bv[0].hypot(bv[1]);
        if aw < 48. || !(if partial { 0.4..=2.5 } else { 0.7..=1.3 }).contains(&(bw / aw)) {
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
        if !partial && ((bc[0] - ac[0]) * normal[0] + (bc[1] - ac[1]) * normal[1]).abs() > aw * 0.12
        {
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
            if self.trace_to_line(from, bl, normal, width, cut) {
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
            && (matches!(
                self.format.as_str(),
                "EAN13" | "UPCA" | "EAN8" | "UPCE" | "Code128" | "Code39" | "ITF"
            ) || (crate::LOW_FAST_PATH
                && matches!(
                    self.format.as_str(),
                    "Codabar" | "Code93" | "DataBar" | "DataBarExpanded"
                )))
    }
    fn same_symbol(&self, other: &Self) -> bool {
        self.text == other.text
            && self.format == other.format
            && self.addon == other.addon
            && self.gs1 == other.gs1
            && self.reader_initialization == other.reader_initialization
    }
}

// An unchecked weak ITF interpretation may occupy bars already established as
// a supported checksum-valid retail symbol. Geometry only admits the proof;
// distributed source-bar continuity must establish the shared physical region.
fn conflicts<T>(weak: &Read<T>, strong: &Read<T>) -> bool {
    weak.format == "ITF"
        && weak.support <= 2
        && strong.support >= 3
        && matches!(strong.format.as_str(), "EAN13" | "UPCA" | "EAN8" | "UPCE")
}

fn consolidate<T>(mut reads: Vec<Read<T>>, image: Image<'_>) -> Vec<Read<T>> {
    if !reads.iter().enumerate().any(|(i, a)| {
        a.supported()
            && reads[..i].iter().any(|b| {
                (a.text == b.text && a.format == b.format) || conflicts(a, b) || conflicts(b, a)
            })
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
    consolidate_owned(extended, image)
}

fn consolidate_owned<T>(extended: Vec<Read<T>>, image: Image<'_>) -> Vec<Read<T>> {
    // Preserve the original proofs and their budget. Only unresolved results
    // enter this separately bounded ownership pass.
    let mut evidence = Evidence {
        image,
        remaining: 32768,
    };
    let mut owned: Vec<Read<T>> = Vec::new();
    for read in extended {
        let mut merged = false;
        if read.supported() {
            for other in &mut owned {
                if read.same_symbol(other) {
                    if let Some(polygon) = evidence.owned_bars(other.polygon, read.polygon, false) {
                        other.polygon = polygon;
                        other.geometry_changed = true;
                        merged = true;
                        break;
                    }
                }
            }
        }
        if !merged {
            owned.push(read);
        }
    }
    let mut keep = vec![true; owned.len()];
    for (i, weak) in owned.iter().enumerate() {
        if weak.format != "ITF" || weak.support > 2 {
            continue;
        }
        for strong in &owned {
            if conflicts(weak, strong)
                && crate::geometry::overlap_quads(&weak.polygon, &strong.polygon).0 >= 0.65
                && evidence
                    .owned_bars(weak.polygon, strong.polygon, true)
                    .is_some()
            {
                keep[i] = false;
                break;
            }
        }
    }
    owned
        .into_iter()
        .zip(keep)
        .filter_map(|(read, keep)| keep.then_some(read))
        .collect()
}

fn complete_footprints<T>(reads: Vec<Read<T>>, image: Image<'_>) -> Vec<Read<T>> {
    trace_footprints(reads, image, true, true, None)
}

fn trace_footprints<T>(
    mut reads: Vec<Read<T>>,
    image: Image<'_>,
    suppress_conflicts: bool,
    display_recovery: bool,
    minimum_aspect: Option<f64>,
) -> Vec<Read<T>> {
    let mut evidence = Evidence {
        image,
        remaining: 262_144,
    };
    let mut owners: Vec<(usize, footprint::Footprint)> = Vec::new();
    let mut measured = vec![false; reads.len()];
    let mut keep = vec![true; reads.len()];
    let mut order: Vec<usize> = (0..reads.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(reads[i].support));
    for i in order {
        if !reads[i].supported() {
            continue;
        }
        if minimum_aspect.is_some_and(|limit| {
            if reads[i].support > 3 {
                return true;
            }
            let q = reads[i].polygon;
            let width = distance(q[0], q[1]) + distance(q[3], q[2]);
            let height = distance(q[0], q[3]) + distance(q[1], q[2]);
            height >= limit * width
        }) {
            // Display-only shortcut; strict ownership always passes no aspect filter.
            measured[i] = true;
            continue;
        }
        for (j, owner) in &owners {
            let a = &reads[*j];
            let b = &reads[i];
            let same_metadata = a.addon == b.addon
                && a.gs1 == b.gs1
                && a.reader_initialization == b.reader_initialization;
            if same_metadata
                && (a.same_symbol(b) || (a.support >= 3 && a.support >= b.support))
                && owner.owns(b.polygon)
            {
                keep[i] = false;
                break;
            }
        }
        if keep[i] {
            if let Some(owner) = footprint::measure(&mut evidence, reads[i].polygon) {
                reads[i].polygon = owner.polygon;
                reads[i].geometry_changed = true;
                if suppress_conflicts {
                    owners.push((i, owner));
                }
                measured[i] = true;
            }
        }
    }
    // Display recovery follows strict ownership and never suppresses a read.
    for (index, read) in reads.iter_mut().enumerate() {
        if display_recovery && keep[index] && !measured[index] && read.supported() {
            if let Some(polygon) = footprint::display(&mut evidence, read.polygon) {
                read.polygon = polygon;
                read.geometry_changed = true;
            }
        }
    }
    reads
        .into_iter()
        .zip(keep)
        .filter_map(|(r, k)| k.then_some(r))
        .collect()
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
    complete_footprints(consolidate(reads, image), image)
        .into_iter()
        .map(|mut read| {
            read.payload.polygon = read.polygon;
            read.payload
        })
        .collect()
}

/// Reuse the original pixel-continuity proof when joining fast observations.
#[cfg(feature = "low")]
pub(crate) fn connect_fast(a: Quad, b: Quad, image: Image<'_>, remaining: &mut usize) -> bool {
    let mut evidence = Evidence {
        image,
        remaining: *remaining,
    };
    let connected = evidence.gap_free(a, b)
        && evidence
            .connected(a, b)
            .or_else(|| evidence.connected_sampling(a, b, true))
            .or_else(|| evidence.connected_traces(a, b))
            .is_some();
    *remaining = evidence.remaining;
    connected
}

#[cfg(feature = "low")]
pub(crate) fn merge_fast(
    reads: Vec<crate::read::Read>,
    image: Image<'_>,
) -> Vec<crate::read::Read> {
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
    let reads = consolidate(reads, image);
    let ambiguous = reads
        .iter()
        .enumerate()
        .any(|(i, a)| reads[..i].iter().any(|b| a.same_symbol(b)));
    // Extend every accepted outline, but retain the original conflict policy.
    // A display footprint must not suppress another distinct valid payload.
    let selective = option_env!("TAPIRSCAN_TURBO_SELECTIVE_OUTLINES").is_some();
    let reads = if ambiguous || selective || option_env!("TAPIRSCAN_TURBO_BAND_OUTLINES").is_none()
    {
        trace_footprints(
            reads,
            image,
            ambiguous,
            option_env!("TAPIRSCAN_TURBO_LEGACY_OUTLINES").is_none(),
            (selective && !ambiguous).then_some(0.08),
        )
    } else {
        // No ownership suppression occurs in the unambiguous footprint pass.
        // Retain the confirmed source band instead of spending time on display extents.
        reads
    };
    let reads = if ambiguous {
        consolidate(reads, image)
    } else {
        reads
    };
    reads
        .into_iter()
        .map(|mut read| {
            read.payload.polygon = read.polygon;
            read.payload
        })
        .collect()
}

pub(crate) fn merge_primary(reads: &mut Vec<crate::Barcode>, image: Image<'_>) {
    if reads.is_empty() {
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
    *reads = complete_footprints(consolidate(typed, image), image)
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

    #[cfg(feature = "low")]
    #[test]
    fn contrast_shortcut_preserves_boundary_color_and_budget_semantics() {
        for channels in [1, 3, 4] {
            for contrast in [0_u8, 23, 24, 200] {
                let mut pixels = vec![0; 80 * 64 * channels];
                for y in 0..64 {
                    for x in 0..80 {
                        for c in 0..channels {
                            pixels[(y * 80 + x) * channels + c] = if (x / 3 + y / 5 + c) % 2 == 0 {
                                20
                            } else {
                                20 + contrast
                            };
                        }
                    }
                }
                let image = Image {
                    data: &pixels,
                    width: 80,
                    height: 64,
                    channels,
                    stride: 80 * channels,
                };
                for left in [
                    [0., 0.],
                    [2., 2.],
                    [40., 31.],
                    [-0.49, 10.],
                    [-1., 20.],
                    [79.4, 63.4],
                ] {
                    for right in [
                        [79., 63.],
                        [70., 10.],
                        [2., 2.],
                        [-1., 0.],
                        [80., 64.],
                        [40., 31.],
                    ] {
                        for budget in [0, 63, 64, 128, 4096] {
                            let mut reference = Evidence {
                                image,
                                remaining: budget,
                            };
                            let mut candidate = Evidence {
                                image,
                                remaining: budget,
                            };
                            assert_eq!(
                                candidate.has_contrast(left, right),
                                reference.profile(left, right).is_some()
                            );
                            assert_eq!(candidate.remaining, reference.remaining);
                        }
                    }
                }
            }
        }
    }

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
    #[test]
    fn weak_itf_requires_shared_bars_not_just_overlapping_retail_boxes() {
        let mut pixels = vec![220; 320 * 260];
        for y in 20..240 {
            for x in 60..252 {
                if (x - 60) / 3 % 3 == 0 {
                    pixels[y * 320 + x] = 20;
                }
            }
        }
        let primary = crate::read::Read::primary(
            [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
            [[60., 50.], [252., 50.], [252., 160.], [60., 160.]],
            7,
            0,
            vec![],
        );
        let mut weak = primary.clone();
        weak.format = "ITF".into();
        weak.text = "123456".into();
        weak.support = 2;
        weak.polygon = [[60., 70.], [252., 70.], [252., 180.], [60., 180.]];
        let scan = |pixels: &[u8], weak: crate::read::Read| {
            merge(
                vec![primary.clone(), weak],
                Image {
                    data: pixels,
                    width: 320,
                    height: 260,
                    channels: 1,
                    stride: 320,
                },
            )
        };
        assert_eq!(scan(&pixels, weak.clone()).len(), 1);
        let mut supported = weak.clone();
        supported.support = 3;
        // The full measured footprint now proves this stronger alias uses the same bars.
        assert_eq!(scan(&pixels, supported).len(), 1);
        pixels[115 * 320..116 * 320].fill(255);
        assert_eq!(scan(&pixels, weak).len(), 2);
    }
    #[test]
    fn footprints_arbitrate_different_payloads_only_on_connected_source_bars() {
        let mut pixels = vec![240; 320 * 260];
        for y in 20..240 {
            for x in 60..252 {
                if (x - 60) / 3 % 3 == 0 {
                    pixels[y * 320 + x] = 20;
                }
            }
        }
        let make = |text: &str, y, support| Read {
            text: text.to_owned(),
            format: "EAN8".to_owned(),
            addon: None,
            gs1: false,
            reader_initialization: false,
            support,
            polygon: [[60., y - 2.], [252., y - 2.], [252., y + 2.], [60., y + 2.]],
            geometry_changed: false,
            payload: (),
        };
        let scan = |p: &[u8]| {
            complete_footprints(
                vec![make("42267638", 70., 7), make("12345670", 180., 3)],
                Image {
                    data: p,
                    width: 320,
                    height: 260,
                    channels: 1,
                    stride: 320,
                },
            )
        };
        assert_eq!(scan(&pixels).len(), 1);
        pixels[120 * 320..121 * 320].fill(240);
        assert_eq!(scan(&pixels).len(), 2);
    }
}
