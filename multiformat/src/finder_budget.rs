//! Shared work bounds of the QR, Aztec and `MaxiCode` row finders.
//!
//! The finders test every run window of a scanned row and cross-check the
//! windows that pass. Periodic textures pass the row tests at nearly every run,
//! so cross checks are budgeted per scanned pixel. A row whose windows may need
//! more checks than are available is short: it skips the windows inside a
//! periodic texture, tests a bounded, evenly spread share of the rest
//! ([`Rows::starts`]), and [`thin`] keeps the passing windows that look least like
//! texture or noise. A symbol to the right of or inside a textured band is
//! still checked, and a row that fits is scanned exactly as without a budget.

/// Row step that scans about `rows` rows and at most about `2 * rows * sqrt(w * h)`
/// row pixels. The second bound only applies to frames more than four times wider
/// than tall: without it, a 65535x520 frame scans every one of its rows and costs
/// ten times as much as a square frame of the same area.
pub(crate) fn row_step(w: usize, h: usize, rows: usize) -> usize {
    (h / rows)
        .max(w.saturating_mul(h).isqrt() / (2 * rows))
        .max(1)
}

/// Keep at most `allowed` of `items` in their order: those with the lowest keys,
/// and among the windows tied at the cut, an evenly spaced selection starting at
/// a row-dependent `phase`. Keeps everything when it fits, so a budget that does
/// not bind never changes which windows are checked or their order.
pub(crate) fn thin<T>(
    items: &mut Vec<T>,
    allowed: usize,
    phase: usize,
    keys: &mut Vec<usize>,
    key: impl Fn(&T) -> usize,
) {
    if items.len() <= allowed {
        return;
    }
    if allowed == 0 {
        items.clear();
        return;
    }
    // The item keys, followed by a copy that the selection reorders.
    let n = items.len();
    keys.clear();
    keys.extend(items.iter().map(key));
    keys.extend_from_within(..);
    let cut = *keys[n..].select_nth_unstable(allowed - 1).1;
    let below = keys[..n].iter().filter(|&&k| k < cut).count();
    let tied = keys[..n].iter().filter(|&&k| k == cut).count();
    // 1 <= wanted <= tied, since `cut` is the allowed-th smallest key.
    let wanted = allowed - below;
    let offset = phase % tied;
    let (mut index, mut rank) = (0, 0);
    items.retain(|_| {
        let k = keys[index];
        index += 1;
        if k != cut {
            return k < cut;
        }
        // Bresenham spacing: exactly `wanted` of the `tied` windows are taken.
        let take = ((rank + 1) * wanted + offset) / tied > (rank * wanted + offset) / tied;
        rank += 1;
        take
    });
}

/// Runs counted on each side of a window by [`Streaks`].
pub(crate) const STREAK_CAP: usize = 32;

/// Whether two runs of one color continue a width: within 35% of the wider.
fn similar(a: usize, b: usize) -> bool {
    20 * a.abs_diff(b) <= 7 * a.max(b)
}

/// Whether run `j` of the row with transitions at `offsets` continues the width
/// of the run of its color before it. Comparing runs of one color keeps stripes
/// whose dark and light runs differ (thin lines, dots, thresholded gradients)
/// periodic, while a finder's center run differs from its rings.
#[inline]
fn continues(offsets: &[usize], j: usize) -> bool {
    j < 2 || similar(offsets[j - 1] - offsets[j - 2], offsets[j + 1] - offsets[j])
}

/// Periodic run streaks of one row, from its transition offsets. Stripes,
/// checkerboards and dot grids repeat their dark and light widths across the
/// row; a finder pattern's runs end after a few runs at its quiet zone or data,
/// and noise rarely repeats widths for long.
#[derive(Default)]
pub(crate) struct Streaks {
    /// The first run of the streak that each run belongs to.
    start: Vec<u32>,
}
impl Streaks {
    /// Measure every run of the row with transitions at `offsets`.
    pub(crate) fn fill(&mut self, offsets: &[usize]) {
        self.start.clear();
        let mut start = 0;
        for j in 0..offsets.len().saturating_sub(1) {
            if !continues(offsets, j) {
                start = u32::try_from(j).unwrap_or(u32::MAX);
            }
            self.start.push(start);
        }
    }
    /// [`around`] with [`STREAK_CAP`] from the measured streaks.
    pub(crate) fn around(&self, first: usize, last: usize) -> usize {
        let start = usize::try_from(self.start[first]).unwrap_or(usize::MAX);
        let before = (first - start.min(first)).min(STREAK_CAP);
        let end = self.start.len().min(last + STREAK_CAP + 1);
        let after = self.start[last..end].partition_point(|&s| s == self.start[last]) - 1;
        before + after
    }
    /// Whether the runs `first..=last` lie inside one streak that continues for
    /// [`STREAK_CAP`] runs on both sides.
    pub(crate) fn is_texture(&self, first: usize, last: usize) -> bool {
        self.start
            .get(last + STREAK_CAP)
            .is_some_and(|&start| usize::try_from(start).is_ok_and(|s| s + STREAK_CAP <= first))
    }
}

/// Window starts tested per cross check a short row can afford.
const TESTS_PER_CHECK: usize = 8;

/// How many runs before run `first` and after run `last` of the row with
/// transitions at `offsets` continue the widths at those ends, up to `cap` on
/// each side.
pub(crate) fn around(offsets: &[usize], first: usize, last: usize, cap: usize) -> usize {
    let runs = offsets.len().saturating_sub(1);
    let mut before = 0;
    while before < cap && before < first && continues(offsets, first - before) {
        before += 1;
    }
    let mut after = 0;
    while after < cap && last + after + 1 < runs && continues(offsets, last + after + 1) {
        after += 1;
    }
    before + after
}

/// [`Streaks::is_texture`] for one window without measuring the row: whether the
/// runs from `first - STREAK_CAP` to `last + STREAK_CAP` continue their widths.
/// A window outside a texture usually stops after a few runs.
pub(crate) fn inside_texture(offsets: &[usize], first: usize, last: usize) -> bool {
    (first + 1..=last).all(|j| continues(offsets, j))
        && around(offsets, first, last, STREAK_CAP) == 2 * STREAK_CAP
}

/// Short rows without texture before another texture scan: textures span many
/// rows, and noise rows would pay for scans that find nothing.
const TEXTURE_RECHECK: usize = 8;

/// Picks the window starts of a frame's rows for one finder.
#[derive(Default)]
pub(crate) struct Rows {
    /// Short rows left until the next texture scan.
    wait: usize,
    /// Per run of the scanned row: whether it starts a new streak.
    breaks: Vec<bool>,
    /// The last texture-scanned row's offsets, its starts outside textures and
    /// whether it had texture windows: stripes and dot grids repeat rows.
    previous: (Vec<usize>, Vec<usize>, bool),
}
impl Rows {
    /// Collect into `starts` the dark runs that start a row's windows: every
    /// second run from `first_dark` up to `end` (runs alternate). `span` gives
    /// a window's first and last run relative to its start.
    ///
    /// A row whose windows may need more checks than the `allowed` ones is short
    /// (one window per dark run at most). It skips the windows inside a streak
    /// that continues [`STREAK_CAP`] runs beyond them on both sides (while
    /// textures are found, and every [`TEXTURE_RECHECK`] short rows otherwise),
    /// and when more starts remain than [`TESTS_PER_CHECK`] times the allowed
    /// checks, it keeps an evenly spaced selection from a row-dependent phase,
    /// as noise passes the row tests at many windows. Returns `None` for a row
    /// that is not short, which keeps every start without collecting them (see
    /// [`for_each_start`]), and otherwise whether any start was left out.
    #[expect(
        clippy::too_many_arguments,
        reason = "The row's offsets, run range, window span and budget are separate values of the caller's loop."
    )]
    pub(crate) fn starts(
        &mut self,
        starts: &mut Vec<usize>,
        offsets: &[usize],
        first_dark: usize,
        end: usize,
        span: (usize, usize),
        allowed: usize,
        y: usize,
    ) -> Option<bool> {
        starts.clear();
        if offsets.len() / 2 <= allowed {
            return None;
        }
        let mut skipped = false;
        if self.wait == 0 {
            if self.previous.0 == offsets {
                starts.extend_from_slice(&self.previous.1);
                skipped = self.previous.2;
            } else {
                skipped = self.texture_starts(starts, offsets, first_dark, end, span);
                self.previous.0.clear();
                self.previous.0.extend_from_slice(offsets);
                self.previous.1.clear();
                self.previous.1.extend_from_slice(starts);
                self.previous.2 = skipped;
            }
            self.wait = if skipped { 0 } else { TEXTURE_RECHECK };
        } else {
            self.wait -= 1;
            starts.extend((first_dark..end).step_by(2));
        }
        let tests = allowed.saturating_mul(TESTS_PER_CHECK);
        let n = starts.len();
        if n > tests {
            // Bresenham spacing keeps exactly `tests` starts.
            let offset = phase(y) % n;
            let mut rank = 0;
            starts.retain(|_| {
                let keep = ((rank + 1) * tests + offset) / n > (rank * tests + offset) / n;
                rank += 1;
                keep
            });
            skipped = true;
        }
        Some(skipped)
    }
    /// The starts of [`Rows::starts`] whose windows (runs `span.0..=span.1`
    /// after the start) do not lie inside a periodic texture; returns whether
    /// any did.
    fn texture_starts(
        &mut self,
        starts: &mut Vec<usize>,
        offsets: &[usize],
        first_dark: usize,
        end: usize,
        (first, last): (usize, usize),
    ) -> bool {
        // Run j (from 2) breaks a streak unless it continues the width of run j - 2.
        self.breaks.clear();
        self.breaks.extend_from_slice(&[false, false]);
        self.breaks.extend(offsets.windows(4).map(|o| {
            let (a, b) = (o[1] - o[0], o[3] - o[2]);
            !similar(a, b)
        }));
        // Window i is decided at run i + last + STREAK_CAP by where the streak
        // through that run began; i and that run both advance by two.
        let mut skipped = false;
        let mut streak = 0;
        let mut j = 0;
        let mut i = first_dark;
        while i < end {
            let decide = i + last + STREAK_CAP;
            if decide >= self.breaks.len() {
                break;
            }
            while j <= decide {
                if self.breaks[j] {
                    streak = j;
                }
                j += 1;
            }
            if streak + STREAK_CAP <= i + first {
                skipped = true;
            } else {
                starts.push(i);
            }
            i += 2;
        }
        starts.extend((i..end).step_by(2));
        skipped
    }
}

/// Call `f` with every second run from `first_dark` up to `end`, or for a short
/// row (`plan` from [`Rows::starts`] is `Some`) with the collected `starts`.
pub(crate) fn for_each_start(
    plan: Option<bool>,
    starts: &[usize],
    first_dark: usize,
    end: usize,
    mut f: impl FnMut(usize),
) {
    if plan.is_some() {
        for &i in starts {
            f(i);
        }
    } else {
        for i in (first_dark..end).step_by(2) {
            f(i);
        }
    }
}

/// A row-dependent phase for [`thin`] and [`Rows::starts`], so consecutive rows
/// check different windows of a uniform texture.
pub(crate) fn phase(y: usize) -> usize {
    y.wrapping_mul(2_654_435_761)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thin_keeps_everything_that_fits_in_order() {
        let mut items: Vec<usize> = (0..10).collect();
        thin(&mut items, 10, 3, &mut Vec::new(), |&v| 10 - v);
        assert_eq!(items, (0..10).collect::<Vec<_>>());
    }
    #[test]
    fn thin_prefers_low_keys_and_spreads_ties() {
        // Two distinct windows among 100 equal ones.
        let mut items: Vec<usize> = (0..102).collect();
        let key = |&v: &usize| usize::from(v != 40 && v != 90);
        thin(&mut items, 12, 0, &mut Vec::new(), key);
        assert_eq!(items.len(), 12);
        assert!(items.contains(&40) && items.contains(&90));
        assert!(items.windows(2).all(|p| p[0] < p[1]), "order is kept");
        // The ten tied windows are spread over the row, not taken from the left.
        let tied: Vec<_> = items.iter().filter(|&&v| key(&v) == 1).collect();
        assert!(**tied.last().unwrap() > 90 && *tied[0] < 12, "{tied:?}");
        for phase in 0..200 {
            let mut items: Vec<usize> = (0..102).collect();
            thin(&mut items, 12, phase, &mut Vec::new(), key);
            assert_eq!(items.len(), 12);
        }
        let mut items: Vec<usize> = (0..5).collect();
        thin(&mut items, 0, 0, &mut Vec::new(), key);
        assert_eq!(items, [] as [usize; 0]);
    }
    #[test]
    fn streaks_tell_textures_from_finder_rows() {
        // 40 runs of thin lines with wider gaps, a 1:1:3:1:1 finder with light
        // margins, then 40 more.
        let lines = |n: usize| {
            (0..n)
                .map(|k| if k % 2 == 0 { 1 } else { 4 })
                .collect::<Vec<usize>>()
        };
        let mut widths = lines(40);
        widths.extend([12, 4, 4, 12, 4, 4, 12]);
        widths.extend(lines(40));
        let offsets: Vec<usize> = std::iter::once(0)
            .chain(widths.iter().scan(0, |end, w| {
                *end += w;
                Some(*end)
            }))
            .collect();
        let mut streaks = Streaks::default();
        streaks.fill(&offsets);
        // A texture window needs 32 equal runs on both sides.
        assert!(!streaks.is_texture(33, 37) && !streaks.is_texture(2, 6));
        widths = [lines(80), widths, lines(80)].concat();
        let offsets: Vec<usize> = std::iter::once(0)
            .chain(widths.iter().scan(0, |end, w| {
                *end += w;
                Some(*end)
            }))
            .collect();
        streaks.fill(&offsets);
        assert!(streaks.is_texture(40, 44), "inside the left texture");
        assert!(streaks.is_texture(170, 174), "inside the right texture");
        // The finder runs 121..=125 between their margins at 120 and 126.
        assert!(!streaks.is_texture(121, 125));
        assert!(
            around(&offsets, 121, 125, 8) <= 1,
            "the margins end the finder"
        );
        assert_eq!(around(&offsets, 40, 44, STREAK_CAP), 2 * STREAK_CAP);
        assert_eq!(around(&offsets, 110, 114, STREAK_CAP), STREAK_CAP + 5);
        assert_eq!(around(&offsets, 110, 114, 8), 8 + 5);
        for (first, last) in [
            (40, 44),
            (121, 125),
            (110, 114),
            (2, 6),
            (88, 92),
            (170, 174),
        ] {
            assert_eq!(
                streaks.around(first, last),
                around(&offsets, first, last, STREAK_CAP),
                "{first}..={last}"
            );
        }
        // A short row skips the texture windows and keeps the finder's.
        let mut starts = Vec::new();
        let n = offsets.len() - 1;
        let mut rows = Rows::default();
        let allowed = offsets.len() / 2 - 1;
        assert_eq!(
            rows.starts(&mut starts, &offsets, 0, n - 4, (0, 4), allowed, 0),
            Some(true)
        );
        for i in (0..n - 4).step_by(2) {
            let expected = !streaks.is_texture(i, i + 4);
            assert_eq!(starts.contains(&i), expected, "start {i}");
        }
        assert!(starts.contains(&120));
        for (first, last) in [(40, 44), (121, 125), (110, 114), (2, 6), (88, 92)] {
            assert_eq!(
                inside_texture(&offsets, first, last),
                streaks.is_texture(first, last),
                "{first}..={last}"
            );
        }
        // A row that fits keeps every start; with few checks, a short row an
        // evenly spread share.
        let fits = offsets.len() / 2;
        let plan = rows.starts(&mut starts, &offsets, 1, n - 4, (0, 4), fits, 0);
        assert_eq!(plan, None);
        let mut all = Vec::new();
        for_each_start(plan, &starts, 1, n - 4, |i| all.push(i));
        assert_eq!(all, (1..n - 4).step_by(2).collect::<Vec<_>>());
        assert_eq!(
            rows.starts(&mut starts, &offsets, 0, 100, (0, 4), 1, 3),
            Some(true)
        );
        assert_eq!(starts.len(), TESTS_PER_CHECK);
        // Equal runs are periodic too.
        let equal: Vec<usize> = (0..=100).map(|k| 2 * k).collect();
        streaks.fill(&equal);
        assert!(streaks.is_texture(40, 50) && inside_texture(&equal, 40, 50));
    }
    #[test]
    fn wide_frames_scan_like_square_frames() {
        assert_eq!(row_step(1920, 1080, 600), 1);
        assert_eq!(row_step(4000, 3000, 600), 5);
        assert_eq!(row_step(5800, 5800, 600), 9);
        assert_eq!(row_step(4320, 1080, 600), 1);
        assert_eq!(row_step(500, 65535, 600), 109);
        assert_eq!(row_step(65535, 520, 600), 4);
        assert_eq!(row_step(0, 0, 600), 1);
    }
}
