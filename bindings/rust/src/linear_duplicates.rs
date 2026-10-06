//! Bounded source-pixel evidence for consolidating bands of one linear symbol.
use crate::{geometry::distance, Image, Quad};
mod area;
mod footprint;

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0].midpoint(b[0]), a[1].midpoint(b[1])]
}
fn line(q: Quad) -> [[f64; 2]; 2] {
    [midpoint(q[0], q[3]), midpoint(q[1], q[2])]
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
    #[cfg(any(
        feature = "low",
        feature = "medium",
        feature = "high",
        feature = "very-high"
    ))]
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

    #[cfg(any(
        feature = "low",
        feature = "medium",
        feature = "high",
        feature = "very-high"
    ))]
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
        if rate.abs() < 0.8
            || (start / rate).abs()
                > if matches!(crate::MODE_ID, 2 | 3) {
                    1024.
                } else {
                    384.
                }
        {
            return false;
        }
        let direction = -(start / rate).signum();
        let mut point = from;
        for _ in 0..if matches!(crate::MODE_ID, 2 | 3) {
            2048
        } else {
            768
        } {
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

    // Width is finite and clamped to a small positive sampling bound.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
        let count = if matches!(crate::MODE_ID, 2 | 3) {
            usize::try_from((aw.ceil().clamp(256., 2048.)) as u32).ok()?
        } else {
            256
        };
        let count_f = f64::from(u32::try_from(count).ok()?);
        #[cfg(not(feature = "low"))]
        let mut values = vec![0.; count];
        #[cfg(feature = "low")]
        let mut values = [0.; 256];
        let (mut low, mut high) = (255_f64, 0_f64);
        for (i, v) in values.iter_mut().enumerate() {
            let f = (f64::from(u32::try_from(i).ok()?) + 0.5) / count_f;
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
        while i < count - 1 {
            if values[i] >= threshold || values[i - 1] < threshold {
                i += 1;
                continue;
            }
            let start = i;
            while i < count - 1 && values[i] < threshold {
                i += 1;
            }
            let width = f64::from(u32::try_from(i - start).ok()?) * aw / count_f;
            if i < count - 1 && (0.8..=aw * 0.07).contains(&width) {
                bars.push((
                    (f64::from(u32::try_from(start + i).ok()?) * 0.5) / count_f,
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "Payload flags, geometry changes and an opt-in ownership proof are independent conditions."
)]
struct Read<T> {
    text: String,
    format: String,
    addon: Option<String>,
    gs1: bool,
    reader_initialization: bool,
    support: u64,
    polygon: Quad,
    geometry_changed: bool,
    allow_code93: bool,
    payload: T,
}
impl<T> Read<T> {
    fn supported(&self) -> bool {
        !self.text.is_empty()
            && (matches!(
                self.format.as_str(),
                "EAN13" | "UPCA" | "EAN8" | "UPCE" | "Code128" | "Code39" | "ITF"
            ) || (self.allow_code93 && self.format == "Code93")
                // Every mode localizes these from shared boxes as well as the full-image scan.
                || matches!(
                    self.format.as_str(),
                    "Codabar" | "DataBar" | "DataBarExpanded"
                )
                || (crate::LOW_FAST_PATH && self.format == "Code93"))
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
    (weak.format == "ITF"
        && weak.support <= 2
        && strong.support >= 3
        && matches!(strong.format.as_str(), "EAN13" | "UPCA" | "EAN8" | "UPCE"))
        || (matches!(crate::MODE_ID, 2 | 3)
            && weak.format == "UPCE"
            && weak.support <= strong.support
            && strong.support >= 3
            && matches!(strong.format.as_str(), "EAN13" | "UPCA"))
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
                if crate::geometry::overlap_quads(&other.polygon, &read.polygon).0 >= 0.65
                    || crate::geometry::adjacent_strips(&other.polygon, &read.polygon)
                {
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
    // enter this separately bounded ownership pass. Higher modes allow longer
    // traces on large source images without relaxing the continuity proof.
    let mut evidence = Evidence {
        image,
        remaining: if matches!(crate::MODE_ID, 2 | 3) {
            524_288
        } else {
            32_768
        },
    };
    let mut owned: Vec<Read<T>> = Vec::new();
    for read in extended {
        let mut merged = false;
        if read.supported() {
            for other in &mut owned {
                if read.same_symbol(other) {
                    if let Some(polygon) = evidence
                        .owned_bars(other.polygon, read.polygon, false)
                        .or_else(|| {
                            if matches!(crate::MODE_ID, 1..=3) {
                                evidence.owned_bars(read.polygon, other.polygon, false)
                            } else {
                                None
                            }
                        })
                    {
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
        if !matches!(weak.format.as_str(), "ITF" | "UPCE") {
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

/// Suppress reads owned by grown symbol areas; `geometry` also replaces kept outlines with them.
fn complete_footprints<T>(reads: Vec<Read<T>>, image: Image<'_>, geometry: bool) -> Vec<Read<T>> {
    trace_footprints(reads, image, true, geometry, None, geometry)
}

fn trace_footprints<T>(
    mut reads: Vec<Read<T>>,
    image: Image<'_>,
    suppress_conflicts: bool,
    display_recovery: bool,
    minimum_aspect: Option<f64>,
    geometry: bool,
) -> Vec<Read<T>> {
    // Each grown area samples about 25-200 thousand budget units (4 per sample).
    let mut evidence = Evidence {
        image,
        remaining: 2_097_152,
    };
    let mut owners: Vec<(usize, area::Area)> = Vec::new();
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
            if let Some(grown) = area::grow(&mut evidence, reads[i].polygon) {
                // An interruption such as glare can split one symbol into two grown areas.
                let joined = owners.iter().position(|(j, owner)| {
                    let (a, b) = (&reads[*j], &reads[i]);
                    suppress_conflicts
                        && a.same_symbol(b)
                        && a.addon == b.addon
                        && a.gs1 == b.gs1
                        && a.reader_initialization == b.reader_initialization
                        && owner.adjoins(&grown, &mut evidence)
                });
                if let Some(k) = joined {
                    owners[k].1.absorb(&grown);
                    let j = owners[k].0;
                    if geometry {
                        reads[j].polygon = owners[k].1.polygon;
                    }
                    keep[i] = false;
                    continue;
                }
                // Intermediate merges keep decoded outlines: later admission samples along them.
                if geometry {
                    reads[i].polygon = grown.polygon;
                    reads[i].geometry_changed = true;
                }
                if suppress_conflicts {
                    owners.push((i, grown));
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
/// Consolidate intermediate reads; decoded outlines stay unchanged.
#[cfg(any(not(feature = "low"), test))]
pub(crate) fn merge(reads: Vec<crate::read::Read>, image: Image<'_>) -> Vec<crate::read::Read> {
    merge_reads(reads, image, false, false)
}

/// Consolidate the reads returned to the caller, with outlines grown to the symbol areas.
pub(crate) fn merge_output(
    reads: Vec<crate::read::Read>,
    image: Image<'_>,
) -> Vec<crate::read::Read> {
    merge_reads(reads, image, false, true)
}

/// Extend source ownership proof for Code93 when a recovery path adds evidence.
#[cfg(any(not(feature = "low"), test))]
pub(crate) fn merge_selected(
    reads: Vec<crate::read::Read>,
    image: Image<'_>,
    allow_code93: bool,
) -> Vec<crate::read::Read> {
    merge_reads(reads, image, allow_code93, false)
}

fn merge_reads(
    reads: Vec<crate::read::Read>,
    image: Image<'_>,
    allow_code93: bool,
    geometry: bool,
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
            allow_code93,
            payload: value,
        })
        .collect();
    complete_footprints(consolidate(reads, image), image, geometry)
        .into_iter()
        .map(|mut read| {
            read.payload.polygon = read.polygon;
            read.payload
        })
        .collect()
}

/// Reuse the original pixel-continuity proof when joining fast observations.
#[cfg(any(
    feature = "low",
    feature = "medium",
    feature = "high",
    feature = "very-high"
))]
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
            allow_code93: false,
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
            true,
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
            allow_code93: false,
            payload: read,
        })
        .collect();
    *reads = complete_footprints(consolidate(typed, image), image, true)
        .into_iter()
        .map(|mut read| {
            read.payload.detection.polygon = read.polygon;
            read.payload
        })
        .collect();
}

#[cfg(test)]
mod tests;
