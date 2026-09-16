//! Component hull proposals with ECC200 border scoring and projective sampling.
use crate::{datamatrix, qr_detect::map, Detection};
type Point = [f32; 2];
fn cross(a: Point, b: Point, c: Point) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
pub(crate) fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 4 {
        return points;
    }
    let mut result = Vec::new();
    for &p in &points {
        while result.len() >= 2
            && cross(result[result.len() - 2], result[result.len() - 1], p) <= 0.
        {
            result.pop();
        }
        result.push(p);
    }
    let len = result.len();
    for &p in points.iter().rev().skip(1) {
        while result.len() > len
            && cross(result[result.len() - 2], result[result.len() - 1], p) <= 0.
        {
            result.pop();
        }
        result.push(p);
    }
    result.pop();
    result
}
pub(crate) fn quad(mut poly: Vec<Point>) -> Option<[Point; 4]> {
    while poly.len() > 4 {
        let i = (0..poly.len()).min_by(|&a, &b| {
            let area = |i: usize| {
                cross(
                    poly[(i + poly.len() - 1) % poly.len()],
                    poly[i],
                    poly[(i + 1) % poly.len()],
                )
                .abs()
            };
            area(a).total_cmp(&area(b))
        })?;
        poly.remove(i);
    }
    if poly.len() != 4 {
        return None;
    }
    Some([poly[0], poly[1], poly[2], poly[3]])
}
// Intersect supporting hull edges to retain a missing white corner. Simply
// deleting hull vertices gives an inscribed quadrilateral and clips that corner.
pub(crate) fn enclosing_quad(
    poly: &[Point],
    pixel_margin: f32,
    limited: &mut bool,
) -> Option<[Point; 4]> {
    if poly.len() < 4 {
        return None;
    }
    let mut edges: Vec<_> = (0..poly.len()).collect();
    edges.sort_by(|&a, &b| {
        let length = |i: usize| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            (p[0] - q[0]).hypot(p[1] - q[1])
        };
        length(b).total_cmp(&length(a))
    });
    *limited |= edges.len() > 12;
    edges.truncate(12);
    edges.sort_unstable();
    let lines: Vec<_> = edges
        .iter()
        .map(|&i| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            let a = p[1] - q[1];
            let b = q[0] - p[0];
            [
                a,
                b,
                a * p[0] + b * p[1] - pixel_margin * (a.abs() + b.abs()),
            ]
        })
        .collect();
    // The same ordered edge pair appears in many four-edge combinations.
    // Cache its exact intersection and half-plane violations once. Keep both
    // directions: reversing the floating-point arithmetic need not be bitwise identical.
    let mut intersections = [[None; 12]; 12];
    let mut violations = [[0_u16; 12]; 12];
    for (i, p) in lines.iter().enumerate() {
        for (j, q) in lines.iter().enumerate() {
            if i == j {
                continue;
            }
            let denominator = p[0] * q[1] - q[0] * p[1];
            if denominator.abs() < 1e-5 {
                continue;
            }
            let point = [
                (p[2] * q[1] - q[2] * p[1]) / denominator,
                (p[0] * q[2] - q[0] * p[2]) / denominator,
            ];
            intersections[i][j] = Some(point);
            for (k, l) in lines.iter().enumerate() {
                if l[0] * point[0] + l[1] * point[1] < l[2] - 0.02 {
                    violations[i][j] |= 1 << k;
                }
            }
        }
    }
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for a in 0..lines.len() {
        for b in a + 1..lines.len() {
            for c in b + 1..lines.len() {
                for d in c + 1..lines.len() {
                    let selected = [a, b, c, d];
                    let selected_mask = (1 << a) | (1 << b) | (1 << c) | (1 << d);
                    let mut quad = [[0.; 2]; 4];
                    let mut valid = true;
                    for i in 0..4 {
                        let p = selected[i];
                        let q = selected[(i + 1) % 4];
                        let Some(point) = intersections[p][q] else {
                            valid = false;
                            break;
                        };
                        if violations[p][q] & selected_mask != 0 {
                            valid = false;
                            break;
                        }
                        quad[i] = point;
                    }
                    if !valid {
                        continue;
                    }
                    let area = (0..4)
                        .map(|i| {
                            let p = quad[i];
                            let q = quad[(i + 1) % 4];
                            p[0] * q[1] - p[1] * q[0]
                        })
                        .sum::<f32>()
                        * 0.5;
                    if area > 0. && area < best_area {
                        best_area = area;
                        best = Some(quad);
                    }
                }
            }
        }
    }
    best
}
fn rotated_rectangle(poly: &[Point]) -> Option<[Point; 4]> {
    if poly.len() < 4 {
        return None;
    }
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for i in 0..poly.len() {
        let edge_start = poly[i];
        let edge_end = poly[(i + 1) % poly.len()];
        let dx = edge_end[0] - edge_start[0];
        let dy = edge_end[1] - edge_start[1];
        let length = dx.hypot(dy);
        if length < 1e-5 {
            continue;
        }
        let c = dx / length;
        let s = dy / length;
        let mut a0 = f32::INFINITY;
        let mut a1 = f32::NEG_INFINITY;
        let mut b0 = f32::INFINITY;
        let mut b1 = f32::NEG_INFINITY;
        for p in poly {
            let a = p[0] * c + p[1] * s;
            let b = -p[0] * s + p[1] * c;
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        let area = (a1 - a0) * (b1 - b0);
        if area < best_area {
            best_area = area;
            best = Some(
                [[a0, b0], [a1, b0], [a1, b1], [a0, b1]]
                    .map(|[a, b]| [a * c - b * s, a * s + b * c]),
            );
        }
    }
    best
}
#[derive(Clone, Copy, Debug)]
struct Span {
    y: usize,
    left: usize,
    right: usize,
}

#[derive(Clone, Debug)]
struct Component {
    parent: usize,
    origin: usize,
    count: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    // The initial run stays inline, avoiding a heap allocation for every
    // isolated texture run. Reducing all retained runs to the leftmost and
    // rightmost point of each row below is exactly the legacy boundary.
    first_span: Span,
    spans: Vec<Span>,
}

// A run carries one Component allocation and one Span. Cap the proposal's
// temporary state before texture or a checkerboard can retain unbounded
// per-run vectors. Returning None always transfers the image to the exact
// legacy extractor; it never emits a partial component list.
const FASTPATH_MAX_RUNS: usize = 262_144;

fn fastpath_run_budget(image_len: usize) -> usize {
    (image_len / 16).clamp(16_384, FASTPATH_MAX_RUNS)
}

// A projected excess after a completed quarter or half row pass is enough to
// abandon CCL before the hard budget. This only chooses the exact legacy
// extractor; it never suppresses a binary image or a component.
fn projected_run_over_budget(runs: usize, rows_scanned: usize, h: usize, budget: usize) -> bool {
    (runs as u64) * (h as u64) * 4 > (budget as u64) * (rows_scanned as u64) * 5
}

fn find_component(components: &mut [Component], mut index: usize) -> usize {
    let mut root = index;
    while components[root].parent != root {
        root = components[root].parent;
    }
    while components[index].parent != index {
        let parent = components[index].parent;
        components[index].parent = root;
        index = parent;
    }
    root
}

fn union_components(components: &mut [Component], a: usize, b: usize) -> usize {
    let a = find_component(components, a);
    let b = find_component(components, b);
    if a == b {
        return a;
    }
    // Keep the larger retained span vector as the parent. Candidate tie order
    // is carried separately by origin, so it does not require the oldest root
    // to receive every appended child vector.
    let a_spans = 1 + components[a].spans.len();
    let b_spans = 1 + components[b].spans.len();
    let (root, child) = if a_spans > b_spans
        || (a_spans == b_spans && components[a].origin <= components[b].origin)
    {
        (a, b)
    } else {
        (b, a)
    };
    let (origin, count, x0, x1, y0, y1, first_span, mut spans) = {
        let component = &mut components[child];
        component.parent = root;
        (
            component.origin,
            component.count,
            component.x0,
            component.x1,
            component.y0,
            component.y1,
            component.first_span,
            std::mem::take(&mut component.spans),
        )
    };
    let component = &mut components[root];
    component.origin = component.origin.min(origin);
    component.count += count;
    component.x0 = component.x0.min(x0);
    component.x1 = component.x1.max(x1);
    component.y0 = component.y0.min(y0);
    component.y1 = component.y1.max(y1);
    append_component_span(component, first_span);
    component.spans.append(&mut spans);
    root
}

fn append_component_span(component: &mut Component, span: Span) {
    if component.spans.is_empty() && component.first_span.y == span.y {
        component.first_span.left = component.first_span.left.min(span.left);
        component.first_span.right = component.first_span.right.max(span.right);
    } else if let Some(last) = component.spans.last_mut().filter(|last| last.y == span.y) {
        last.left = last.left.min(span.left);
        last.right = last.right.max(span.right);
    } else {
        component.spans.push(span);
    }
}

fn add_span(components: &mut [Component], index: usize, span: Span) {
    let index = find_component(components, index);
    let component = &mut components[index];
    component.count += span.right - span.left;
    component.x0 = component.x0.min(span.left);
    component.x1 = component.x1.max(span.right - 1);
    component.y0 = component.y0.min(span.y);
    component.y1 = component.y1.max(span.y);
    append_component_span(component, span);
}

// Connected-component labelling over horizontal runs. A current run overlaps
// the previous row when their half-open intervals are at most one pixel apart;
// that is precisely 8-connectivity. Each source pixel is inspected once while
// locating its run, avoiding the flood fill's repeated neighbor scans.
fn fast_components(image: &[bool], w: usize, h: usize) -> Option<Vec<Component>> {
    let budget = fastpath_run_budget(image.len());
    let quarter = h.div_ceil(4);
    let half = h.div_ceil(2);
    let mut components = Vec::new();
    let mut previous: Vec<(usize, usize, usize)> = Vec::new();
    let mut current: Vec<(usize, usize, usize)> = Vec::new();
    let mut runs = 0;
    for y in 0..h {
        let row = y * w;
        current.clear();
        let mut x = 0;
        let mut previous_start = 0;
        while x < w {
            while x < w && !image[row + x] {
                x += 1;
            }
            if x == w {
                break;
            }
            let left = x;
            while x < w && image[row + x] {
                x += 1;
            }
            let right = x;
            runs += 1;
            if runs > budget {
                return None;
            }
            while previous_start < previous.len() && previous[previous_start].1 < left {
                previous_start += 1;
            }
            let mut overlap = previous_start;
            let mut label = None;
            while overlap < previous.len() && previous[overlap].0 <= right {
                let previous_label = find_component(&mut components, previous[overlap].2);
                label = Some(match label {
                    Some(existing) => union_components(&mut components, existing, previous_label),
                    None => previous_label,
                });
                overlap += 1;
            }
            let span = Span { y, left, right };
            let label = if let Some(label) = label {
                add_span(&mut components, label, span);
                find_component(&mut components, label)
            } else {
                let index = components.len();
                components.push(Component {
                    parent: index,
                    origin: row + left,
                    count: right - left,
                    x0: left,
                    x1: right - 1,
                    y0: y,
                    y1: y,
                    first_span: span,
                    spans: Vec::new(),
                });
                index
            };
            current.push((left, right, label));
        }
        let rows_scanned = y + 1;
        if (rows_scanned == quarter || rows_scanned == half)
            && projected_run_over_budget(runs, rows_scanned, h, budget)
        {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Some(
        components
            .into_iter()
            .enumerate()
            .filter_map(|(index, component)| (component.parent == index).then_some(component))
            .collect(),
    )
}

// The retained baseline is deliberately complete: a fast-path budget overflow
// must take the old extraction route for the whole image, not omit unfinished
// components or apply a coverage gate.
fn legacy_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    legacy_components_impl(image, w, h, false)
}

#[cfg(test)]
fn legacy_all_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    legacy_components_impl(image, w, h, true)
}

fn legacy_components_impl(
    image: &[bool],
    w: usize,
    h: usize,
    retain_ineligible_for_test: bool,
) -> Vec<Component> {
    let mut seen = vec![false; image.len()];
    let mut components = Vec::new();
    let mut row_min = vec![w; h];
    let mut row_max = vec![0; h];
    for origin in 0..image.len() {
        if !image[origin] || seen[origin] {
            continue;
        }
        let mut stack = vec![origin];
        let mut count = 0;
        let mut touched_rows = Vec::new();
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        // Scanline flood fill visits the same eight-connected components while
        // marking contiguous spans instead of testing eight neighbors per pixel.
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            let x = i % w;
            let y = i / w;
            let row = y * w;
            let mut left = x;
            let mut right = x + 1;
            while left > 0 && image[row + left - 1] && !seen[row + left - 1] {
                left -= 1;
            }
            while right < w && image[row + right] && !seen[row + right] {
                right += 1;
            }
            seen[row + left..row + right].fill(true);
            count += right - left;
            x0 = x0.min(left);
            x1 = x1.max(right - 1);
            y0 = y0.min(y);
            y1 = y1.max(y);
            if row_min[y] == w {
                touched_rows.push(y);
            }
            row_min[y] = row_min[y].min(left);
            row_max[y] = row_max[y].max(right - 1);
            for adjacent in [y.checked_sub(1), (y + 1 < h).then_some(y + 1)]
                .into_iter()
                .flatten()
            {
                let neighbor = adjacent * w;
                let mut at = left.saturating_sub(1);
                let end = (right + 1).min(w);
                while at < end {
                    if image[neighbor + at] && !seen[neighbor + at] {
                        stack.push(neighbor + at);
                        at += 1;
                        while at < end && image[neighbor + at] && !seen[neighbor + at] {
                            at += 1;
                        }
                    } else {
                        at += 1;
                    }
                }
            }
        }
        let width = x1 - x0 + 1;
        let height = y1 - y0 + 1;
        let eligible = count >= 30
            && width >= 9
            && height >= 7
            && crate::numeric::usize_f32(count) / crate::numeric::usize_f32(width * height) <= 0.92;
        if eligible || retain_ineligible_for_test {
            let mut spans = Vec::with_capacity(touched_rows.len());
            for y in touched_rows {
                spans.push(Span {
                    y,
                    left: row_min[y],
                    right: row_max[y] + 1,
                });
                row_min[y] = w;
                row_max[y] = 0;
            }
            let first_span = spans.pop().expect("component touched at least one row");
            components.push(Component {
                parent: components.len(),
                origin,
                count,
                x0,
                x1,
                y0,
                y1,
                first_span,
                spans,
            });
        } else {
            for y in touched_rows {
                row_min[y] = w;
                row_max[y] = 0;
            }
        }
    }
    components
}

fn component_hull(
    component: &Component,
    row_min: &mut [usize],
    row_max: &mut [usize],
    touched_rows: &mut Vec<usize>,
) -> Vec<Point> {
    // A union may append older child rows behind newer parent rows, so spans
    // are not necessarily ordered. The legacy extractor retains exactly these
    // per-row extrema. Reuse image-height scratch rather than sorting all raw
    // spans and allocating a boundary sized to their unreduced count.
    for span in component
        .spans
        .iter()
        .chain(std::iter::once(&component.first_span))
    {
        if row_min[span.y] == usize::MAX {
            touched_rows.push(span.y);
        }
        row_min[span.y] = row_min[span.y].min(span.left);
        row_max[span.y] = row_max[span.y].max(span.right);
    }
    let mut boundary = Vec::with_capacity(touched_rows.len() * 2);
    for &y in touched_rows.iter() {
        boundary.push([
            crate::numeric::usize_f32(row_min[y]) + 0.5,
            crate::numeric::usize_f32(y) + 0.5,
        ]);
        boundary.push([
            crate::numeric::usize_f32(row_max[y]) - 0.5,
            crate::numeric::usize_f32(y) + 0.5,
        ]);
        row_min[y] = usize::MAX;
        row_max[y] = 0;
    }
    touched_rows.clear();
    hull(boundary)
}

fn component_is_eligible(component: &Component) -> bool {
    let width = component.x1 - component.x0 + 1;
    let height = component.y1 - component.y0 + 1;
    component.count >= 30
        && width >= 9
        && height >= 7
        && crate::numeric::usize_f32(component.count) / crate::numeric::usize_f32(width * height)
            <= 0.92
}

fn proposals_from_components(components: Vec<Component>, h: usize) -> (Vec<[Point; 4]>, bool) {
    let mut eligible_components = Vec::new();
    let mut row_min = vec![usize::MAX; h];
    let mut row_max = vec![0; h];
    let mut touched_rows = Vec::new();
    for component in components {
        if !component_is_eligible(&component) {
            continue;
        }
        let polygon = component_hull(&component, &mut row_min, &mut row_max, &mut touched_rows);
        let mut area = 0_f32;
        let mut perimeter = 0_f32;
        for i in 0..polygon.len() {
            let p = polygon[i];
            let q = polygon[(i + 1) % polygon.len()];
            area += p[0] * q[1] - p[1] * q[0];
            perimeter += (p[0] - q[0]).hypot(p[1] - q[1]);
        }
        // A filled convex stroke has no alternating clock track or data cells.
        // This rejects oblique 1D bars whose axis-aligned boxes look square.
        // The half-pixel perimeter allowance accounts for pixel-center hulls.
        if crate::numeric::usize_f32(component.count)
            > area.abs() * 0.5 * 0.92 + perimeter * 0.5 + 1.
        {
            continue;
        }
        eligible_components.push((
            component.count,
            component.origin,
            polygon,
            component.x0,
            component.x1,
            component.y0,
            component.y1,
        ));
    }
    // The final proposal order is descending component size. Construct costly
    // enclosing quadrilaterals only until that same ordered budget is filled.
    eligible_components.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut proposals = Vec::new();
    let mut base_count = 0;
    let mut limited = false;
    for (count, _origin, polygon, x0, x1, y0, y1) in eligible_components {
        let before = proposals.len();
        if let Some(q) = rotated_rectangle(&polygon) {
            proposals.push((count, q));
        }
        if let Some(q) = enclosing_quad(&polygon, 0., &mut limited) {
            proposals.push((count, q));
        }
        let inscribed = quad(polygon);
        if let Some(q) = inscribed {
            proposals.push((count, q));
        }
        proposals.push((
            count,
            [
                [crate::numeric::usize_f32(x0), crate::numeric::usize_f32(y0)],
                [
                    crate::numeric::usize_f32(x1) + 1.,
                    crate::numeric::usize_f32(y0),
                ],
                [
                    crate::numeric::usize_f32(x1) + 1.,
                    crate::numeric::usize_f32(y1) + 1.,
                ],
                [
                    crate::numeric::usize_f32(x0),
                    crate::numeric::usize_f32(y1) + 1.,
                ],
            ],
        ));
        base_count += proposals.len() - before;
        // A clock-track corner may be entirely white/disconnected. Three
        // observed corners still define an affine completion of that corner.
        if let Some(q) = inscribed {
            for i in 0..4 {
                let mut completed = q;
                completed[i] = [
                    q[(i + 1) % 4][0] + q[(i + 3) % 4][0] - q[(i + 2) % 4][0],
                    q[(i + 1) % 4][1] + q[(i + 3) % 4][1] - q[(i + 2) % 4][1],
                ];
                if (completed[i][0] - q[i][0]).hypot(completed[i][1] - q[i][1]) < 1. {
                    continue;
                }
                proposals.push((count, completed));
            }
        }
        if base_count > 100 {
            break;
        }
    }
    limited |= base_count > 100 || proposals.len() > 200;
    proposals.truncate(200);
    (proposals.into_iter().map(|p| p.1).collect(), limited)
}

fn candidates(image: &[bool], w: usize, h: usize) -> (Vec<[Point; 4]>, bool) {
    let components = fast_components(image, w, h).unwrap_or_else(|| legacy_components(image, w, h));
    proposals_from_components(components, h)
}

#[cfg(test)]
mod component_fastpath_tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct ComponentSignature {
        origin: usize,
        count: usize,
        bounds: (usize, usize, usize, usize),
        hull: Vec<Point>,
    }

    fn signatures(components: Vec<Component>, h: usize) -> Vec<ComponentSignature> {
        let mut row_min = vec![usize::MAX; h];
        let mut row_max = vec![0; h];
        let mut touched_rows = Vec::new();
        let mut signatures: Vec<_> = components
            .into_iter()
            .map(|component| ComponentSignature {
                origin: component.origin,
                count: component.count,
                bounds: (component.x0, component.x1, component.y0, component.y1),
                hull: component_hull(&component, &mut row_min, &mut row_max, &mut touched_rows),
            })
            .collect();
        signatures.sort_by_key(|component| component.origin);
        signatures
    }

    fn assert_fast_matches_legacy(image: &[bool], w: usize, h: usize) {
        assert_eq!(
            signatures(
                fast_components(image, w, h).expect("within fast-path budget"),
                h
            ),
            signatures(legacy_all_components(image, w, h), h),
            "component count, bounds, row extremes, or hull changed"
        );
        assert_eq!(
            proposals_from_components(fast_components(image, w, h).unwrap(), h),
            proposals_from_components(legacy_components(image, w, h), h),
            "proposal order or geometry changed"
        );
    }

    fn sorted_component_hull_reference(component: &Component) -> Vec<Point> {
        let mut spans = component.spans.clone();
        spans.push(component.first_span);
        spans.sort_unstable_by_key(|span| (span.y, span.left));
        let mut boundary = Vec::with_capacity(spans.len() * 2);
        let mut start = 0;
        while start < spans.len() {
            let y = spans[start].y;
            let mut left = spans[start].left;
            let mut right = spans[start].right;
            let mut end = start + 1;
            while end < spans.len() && spans[end].y == y {
                left = left.min(spans[end].left);
                right = right.max(spans[end].right);
                end += 1;
            }
            boundary.push([
                crate::numeric::usize_f32(left) + 0.5,
                crate::numeric::usize_f32(y) + 0.5,
            ]);
            boundary.push([
                crate::numeric::usize_f32(right) - 0.5,
                crate::numeric::usize_f32(y) + 0.5,
            ]);
            start = end;
        }
        hull(boundary)
    }

    #[test]
    fn row_scratch_preserves_sorted_span_hull_with_duplicates_and_collinear_rows() {
        let component = Component {
            parent: 0,
            origin: 0,
            count: 1,
            x0: 2,
            x1: 15,
            y0: 1,
            y1: 9,
            first_span: Span {
                y: 5,
                left: 5,
                right: 12,
            },
            // Late root unions can put the same row in non-adjacent entries.
            // Include repeated endpoints and collinear rows so `hull` receives
            // precisely the old sorted input set after row reduction.
            spans: vec![
                Span {
                    y: 9,
                    left: 8,
                    right: 9,
                },
                Span {
                    y: 1,
                    left: 2,
                    right: 15,
                },
                Span {
                    y: 5,
                    left: 5,
                    right: 12,
                },
                Span {
                    y: 9,
                    left: 8,
                    right: 9,
                },
                Span {
                    y: 3,
                    left: 3,
                    right: 14,
                },
                Span {
                    y: 7,
                    left: 6,
                    right: 11,
                },
            ],
        };
        let mut row_min = vec![usize::MAX; 10];
        let mut row_max = vec![0; 10];
        let mut touched_rows = Vec::new();
        assert_eq!(
            component_hull(&component, &mut row_min, &mut row_max, &mut touched_rows),
            sorted_component_hull_reference(&component)
        );
        assert!(touched_rows.is_empty());
        assert!(row_min.iter().all(|&value| value == usize::MAX));
    }

    fn paint(image: &mut [bool], w: usize, x: usize, y: usize) {
        image[y * w + x] = true;
    }

    #[test]
    fn preserves_diagonal_touching_runs_and_component_first_order() {
        let (w, h) = (71, 43);
        let mut image = vec![false; w * h];
        // Both diagonals are only 8-connected, and the right branch joins two
        // earlier roots on a later row. The isolated dots exercise tie order.
        for y in 4..34 {
            paint(&mut image, w, 8 + y, y);
            paint(&mut image, w, 66 - y, y);
        }
        for y in 18..25 {
            paint(&mut image, w, 35, y);
        }
        paint(&mut image, w, 2, 2);
        paint(&mut image, w, 68, 39);
        assert_fast_matches_legacy(&image, w, h);
    }

    #[test]
    fn preserves_multi_run_rows_that_merge_later() {
        let (w, h) = (96, 58);
        let mut image = vec![false; w * h];
        for y in 4..18 {
            for x in (6..88).step_by(6) {
                paint(&mut image, w, x + y / 5, y);
            }
        }
        for y in 18..41 {
            for x in 8..88 {
                if (x + y) % 5 == 0 || y % 9 == 0 {
                    paint(&mut image, w, x, y);
                }
            }
        }
        for y in 41..54 {
            for x in (10..90).step_by(7) {
                paint(&mut image, w, x - (y - 41) / 4, y);
            }
        }
        assert_fast_matches_legacy(&image, w, h);
    }

    #[test]
    fn compresses_same_root_row_runs_without_changing_the_component() {
        let (w, h) = (24, 3);
        let mut image = vec![false; w * h];
        for x in 3..21 {
            paint(&mut image, w, x, 0);
        }
        paint(&mut image, w, 3, 1);
        paint(&mut image, w, 20, 1);
        let components = fast_components(&image, w, h).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].spans.len(), 1);
        assert_fast_matches_legacy(&image, w, h);
    }

    #[test]
    fn union_by_size_keeps_the_earliest_component_origin() {
        let (w, h) = (32, 3);
        let mut image = vec![false; w * h];
        paint(&mut image, w, 1, 0);
        paint(&mut image, w, 2, 1);
        for x in 10..24 {
            paint(&mut image, w, x, 0);
            paint(&mut image, w, x, 1);
        }
        for x in 2..24 {
            paint(&mut image, w, x, 2);
        }
        let components = fast_components(&image, w, h).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].origin, 1);
        assert_fast_matches_legacy(&image, w, h);
    }

    #[test]
    fn preserves_random_noise_at_multiple_resolutions() {
        let mut seed = 0x4d59_5df4_u32;
        for (w, h) in [(17, 31), (91, 57), (181, 149)] {
            let mut image = vec![false; w * h];
            for pixel in &mut image {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *pixel = seed >> 30 != 0;
            }
            assert_fast_matches_legacy(&image, w, h);
        }
    }

    #[test]
    fn preserves_rotated_connected_shapes() {
        let (w, h) = (241, 193);
        for angle in [0.19_f32, 0.63, 1.11] {
            let (sin, cos) = angle.sin_cos();
            let mut image = vec![false; w * h];
            for y in 0..h {
                for x in 0..w {
                    let dx = crate::numeric::usize_f32(x) - crate::numeric::usize_f32(w) * 0.5;
                    let dy = crate::numeric::usize_f32(y) - crate::numeric::usize_f32(h) * 0.5;
                    let along = dx * cos + dy * sin;
                    let across = -dx * sin + dy * cos;
                    let border = along.abs() < 76. && (across.abs() - 20.).abs() < 1.1;
                    let stripes = along.abs() < 76.
                        && across.abs() < 20.
                        && crate::numeric::f32_i32((along + 76.).floor()).rem_euclid(7) == 0;
                    if border || stripes {
                        paint(&mut image, w, x, y);
                    }
                }
            }
            assert_fast_matches_legacy(&image, w, h);
        }
    }

    #[test]
    fn checkerboard_exceeds_budget_and_uses_complete_legacy_fallback() {
        let (w, h) = (512, 512);
        let image: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x + y) % 2 == 0))
            .collect();
        assert!(fast_components(&image, w, h).is_none());
        assert_eq!(
            candidates(&image, w, h),
            proposals_from_components(legacy_components(&image, w, h), h)
        );
    }

    #[test]
    fn projected_quarter_fallback_preserves_complete_legacy_proposals() {
        let (w, h) = (512, 512);
        let image: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x + y) % 2 == 0))
            .collect();
        let quarter_rows = h / 4;
        let quarter_runs = (0..quarter_rows)
            .map(|y| (0..w).filter(|x| (x + y) % 2 == 0).count())
            .sum();
        assert!(projected_run_over_budget(
            quarter_runs,
            quarter_rows,
            h,
            fastpath_run_budget(image.len())
        ));
        assert!(fast_components(&image, w, h).is_none());
        assert_eq!(
            candidates(&image, w, h),
            proposals_from_components(legacy_components(&image, w, h), h)
        );
    }
}
fn pixel(image: &[bool], w: usize, h: usize, transform: &[f32; 8], x: f32, y: f32) -> Option<bool> {
    let mapped = map(transform, x, y);
    let xx = crate::numeric::f32_isize(mapped[0].floor());
    let yy = crate::numeric::f32_isize(mapped[1].floor());
    if xx < 0 || yy < 0 || xx >= (w).cast_signed() || yy >= (h).cast_signed() {
        return None;
    }
    Some(image[(yy).cast_unsigned() * w + (xx).cast_unsigned()])
}
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_candidates(image: &[bool], w: usize, h: usize) -> serde_json::Value {
    let (proposals, limited) = candidates(image, w, h);
    serde_json::json!({"limited":limited,"proposals":proposals.into_iter().map(|q|
        serde_json::json!({"quad":q,"lBorder":possible_l_border(image,w,h,q)})
    ).collect::<Vec<_>>()})
}
// Before enumerating symbol sizes, require two adjacent mostly solid sides.
// Jittered samples avoid phase-locking to a particular clock-track frequency.
#[cfg(not(target_arch = "wasm32"))]
fn possible_l_border(image: &[bool], w: usize, h: usize, q: [Point; 4]) -> bool {
    let solid = l_border_edges(image, w, h, q);
    (0..4).any(|i| solid[i] >= 18 && solid[(i + 1) % 4] >= 18)
}
fn l_border_edges(image: &[bool], w: usize, h: usize, quad: [Point; 4]) -> [usize; 4] {
    let center = quad
        .iter()
        .fold([0., 0.], |s, p| [s[0] + p[0] * 0.25, s[1] + p[1] * 0.25]);
    let mut solid = [0; 4];
    for edge in 0..4 {
        let edge_start = quad[edge];
        let edge_end = quad[(edge + 1) % 4];
        let inward = [
            center[0] - (edge_start[0] + edge_end[0]) * 0.5,
            center[1] - (edge_start[1] + edge_end[1]) * 0.5,
        ];
        let length = inward[0].hypot(inward[1]);
        if length < 1. {
            return [0; 4];
        }
        for offset in [0.25, 0.75, 1.5, length / 32., length / 12.] {
            let mut black = 0;
            for i in 0..24 {
                let fraction = (crate::numeric::usize_f32(i)
                    + 0.2
                    + (crate::numeric::usize_f32(i) * 0.381_966).fract() * 0.6)
                    / 24.;
                let x = crate::numeric::f32_isize(
                    (edge_start[0]
                        + fraction * (edge_end[0] - edge_start[0])
                        + inward[0] * offset / length)
                        .floor(),
                );
                let y = crate::numeric::f32_isize(
                    (edge_start[1]
                        + fraction * (edge_end[1] - edge_start[1])
                        + inward[1] * offset / length)
                        .floor(),
                );
                if x >= 0
                    && y >= 0
                    && x < (w).cast_signed()
                    && y < (h).cast_signed()
                    && image[(y).cast_unsigned() * w + (x).cast_unsigned()]
                {
                    black += 1;
                }
                // Consumers only distinguish <15, 15..17 and >=18 samples.
                // Stop when this offset cannot improve that classification,
                // or when it has already reached the highest class.
                let needed = if solid[edge] >= 15 { 18 } else { 15 };
                if black >= 18 || black + (23 - i) < needed {
                    break;
                }
            }
            solid[edge] = solid[edge].max(black);
            if black >= 18 {
                break;
            }
        }
    }
    solid
}
// Closed-form rectangle-to-quadrilateral mapping. Every size hypothesis has
// this rectangular source, so a general eight-equation elimination is wasteful.
fn rectangle_transform(cols: usize, rows: usize, q: [Point; 4]) -> Option<[f32; 8]> {
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let sx = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let sy = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let denominator = dx1 * dy2 - dx2 * dy1;
    if denominator.abs() < 1e-7 || cols == 0 || rows == 0 {
        return None;
    }
    let g = (sx * dy2 - dx2 * sy) / denominator;
    let h = (dx1 * sy - sx * dy1) / denominator;
    Some([
        (q[1][0] - q[0][0] + g * q[1][0]) / crate::numeric::usize_f32(cols),
        (q[3][0] - q[0][0] + h * q[3][0]) / crate::numeric::usize_f32(rows),
        q[0][0],
        (q[1][1] - q[0][1] + g * q[1][1]) / crate::numeric::usize_f32(cols),
        (q[3][1] - q[0][1] + h * q[3][1]) / crate::numeric::usize_f32(rows),
        q[0][1],
        g / crate::numeric::usize_f32(cols),
        h / crate::numeric::usize_f32(rows),
    ])
}
fn border(
    image: &[bool],
    w: usize,
    h: usize,
    transform: &[f32; 8],
    cols: usize,
    rows: usize,
) -> Option<f32> {
    let mut errors = 0;
    let error_limit = (cols + rows) * 2 / 5;
    // The two clock tracks reject solid rectangles sooner. This changes only
    // sampling order: every admitted hypothesis has the same border error sum.
    for (edge, length) in [(0, cols), (1, rows), (2, cols), (3, rows)] {
        for at in 0..length {
            let (x, y, expected) = match edge {
                0 => (at, 0, at % 2 == 0),
                1 => (cols - 1, at, at % 2 == 1),
                2 => (at, rows - 1, true),
                _ => (0, at, true),
            };
            errors += usize::from(
                pixel(
                    image,
                    w,
                    h,
                    transform,
                    crate::numeric::usize_f32(x) + 0.5,
                    crate::numeric::usize_f32(y) + 0.5,
                )? != expected,
            );
            if errors > error_limit {
                return None;
            }
        }
    }
    Some(crate::numeric::usize_f32(errors) / crate::numeric::usize_f32((cols + rows) * 2))
}
#[expect(
    clippy::too_many_lines,
    reason = "The Data Matrix search shares proposal ordering, hypothesis caps and undecoded coverage across its sampling passes."
)]
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    for mode in 0..4 {
        if binary_images.is_duplicate(mode) {
            continue;
        }
        let image = binary_images.get(mode);
        let (proposals, limited) = candidates(image, w, h);
        regions.limited |= limited;
        for original in proposals {
            let solid = l_border_edges(image, w, h, original);
            if !(0..4).any(|i| solid[i] >= 18 && solid[(i + 1) % 4] >= 18) {
                continue;
            }
            let center = original
                .iter()
                .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
            if results.iter().any(|r| {
                let c = r
                    .polygon
                    .iter()
                    .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
                (center[0] - c[0]).hypot(center[1] - c[1]) < 8.
            }) {
                continue;
            }
            let mut hypotheses = Vec::new();
            for mirror in [false, true] {
                for rotation in 0..4 {
                    // A valid orientation puts the solid L on the bottom and
                    // left. Use a looser threshold than proposal admission to
                    // retain damaged edges and uncertain corner completion.
                    let left_edge = (rotation + if mirror { 0 } else { 3 }) % 4;
                    let bottom_edge = (rotation + if mirror { 1 } else { 2 }) % 4;
                    if solid[left_edge] < 15 || solid[bottom_edge] < 15 {
                        continue;
                    }
                    let q: [Point; 4] = std::array::from_fn(|i| {
                        original[(rotation + if mirror { 4 - i } else { i }) % 4]
                    });
                    let dx = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
                    let dy = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
                    for size in datamatrix::SIZES {
                        let ratio = (dx / dy)
                            / (crate::numeric::usize_f32(size.w)
                                / crate::numeric::usize_f32(size.h));
                        if !(0.65..=1.55).contains(&ratio)
                            || dx / crate::numeric::usize_f32(size.w) < 0.8
                            || dy / crate::numeric::usize_f32(size.h) < 0.8
                        {
                            continue;
                        }
                        for margin in [0., 0.25, 0.5, 1.] {
                            let scale_x = 1. + margin * 2. / crate::numeric::usize_f32(size.w);
                            let scale_y = 1. + margin * 2. / crate::numeric::usize_f32(size.h);
                            let ex = [
                                (q[1][0] - q[0][0] + q[2][0] - q[3][0]) * 0.25,
                                (q[1][1] - q[0][1] + q[2][1] - q[3][1]) * 0.25,
                            ];
                            let ey = [
                                (q[3][0] - q[0][0] + q[2][0] - q[1][0]) * 0.25,
                                (q[3][1] - q[0][1] + q[2][1] - q[1][1]) * 0.25,
                            ];
                            let expanded: [Point; 4] = std::array::from_fn(|i| {
                                let sx = if i == 0 || i == 3 { -1. } else { 1. };
                                let sy = if i < 2 { -1. } else { 1. };
                                [
                                    q[i][0]
                                        + sx * (scale_x - 1.) * ex[0]
                                        + sy * (scale_y - 1.) * ey[0],
                                    q[i][1]
                                        + sx * (scale_x - 1.) * ex[1]
                                        + sy * (scale_y - 1.) * ey[1],
                                ]
                            });
                            let Some(t) = rectangle_transform(size.w, size.h, expanded) else {
                                continue;
                            };
                            if let Some(score) = border(image, w, h, &t, size.w, size.h) {
                                if score <= 0.2 {
                                    hypotheses.push((score, *size, t, expanded));
                                }
                            }
                        }
                    }
                }
            }
            hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
            regions.limited |= hypotheses.len() > 12;
            for (score, size, t, polygon) in hypotheses.into_iter().take(12) {
                attempts += 1;
                if attempts > 800 {
                    return (results, true);
                }
                let mut matrix = Vec::with_capacity(size.w * size.h);
                let mut valid = true;
                for y in 0..size.h {
                    for x in 0..size.w {
                        if let Some(v) = pixel(
                            image,
                            w,
                            h,
                            &t,
                            crate::numeric::usize_f32(x) + 0.5,
                            crate::numeric::usize_f32(y) + 0.5,
                        ) {
                            matrix.push(v);
                        } else {
                            valid = false;
                        }
                    }
                }
                if valid {
                    if score <= 0.08 {
                        regions.add("DataMatrix", polygon, 1. - score, 1);
                    }
                    if let Some(read) = datamatrix::decode_matrix(&matrix, size.w, size.h) {
                        results.push(Detection {
                            bytes: Some(read.bytes),
                            structured_append: read.structured_append,
                            reader_initialization: read.reader_initialization,
                            addon: None,
                            format: "DataMatrix".into(),
                            text: read.text,
                            polygon,
                            support: 1,
                            error: score,
                            gs1: read.gs1,
                        });
                        break;
                    }
                }
            }
        }
    }
    (results, false)
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn rectangle_mapping_matches_general_projective_solution() {
        for q in [
            [[10., 20.], [170., 20.], [170., 90.], [10., 90.]],
            [[20., 12.], [154., 43.], [133., 121.], [6., 97.]],
            [[133., 121.], [154., 43.], [20., 12.], [6., 97.]],
        ] {
            let source = [[0., 0.], [32., 0.], [32., 12.], [0., 12.]];
            let general = crate::qr_detect::homography(source, q).unwrap();
            let fast = rectangle_transform(32, 12, q).unwrap();
            for y in 0..=12 {
                for x in 0..=32 {
                    let a = map(
                        &general,
                        crate::numeric::f64_f32(f64::from(x)),
                        crate::numeric::f64_f32(f64::from(y)),
                    );
                    let b = map(
                        &fast,
                        crate::numeric::f64_f32(f64::from(x)),
                        crate::numeric::f64_f32(f64::from(y)),
                    );
                    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 0.001);
                }
            }
        }
    }
    #[test]
    fn enclosing_edge_budget_reports_only_truncation() {
        for count in [12_u16, 13] {
            let points: Vec<_> = (0..count)
                .map(|i| {
                    let angle = f32::from(i) * std::f32::consts::TAU / f32::from(count);
                    [20. * angle.cos(), 20. * angle.sin()]
                })
                .collect();
            let mut limited = false;
            let _ = enclosing_quad(&points, 0., &mut limited);
            assert_eq!(limited, count > 12);
        }
    }
    #[test]
    fn reconstructs_an_unprinted_corner_from_supporting_edges() {
        let q = enclosing_quad(
            &[[0., 0.], [8., 0.], [10., 2.], [10., 10.], [0., 10.]],
            0.,
            &mut false,
        )
        .unwrap();
        for expected in [[0., 0.], [10., 0.], [10., 10.], [0., 10.]] {
            assert!(q
                .iter()
                .any(|p| (p[0] - expected[0]).abs() < 1e-5 && (p[1] - expected[1]).abs() < 1e-5));
        }
    }
}

#[cfg(test)]
fn enclosing_quad_reference(poly: &[Point], pixel_margin: f32) -> Option<[Point; 4]> {
    if poly.len() < 4 {
        return None;
    }
    let mut edges: Vec<_> = (0..poly.len()).collect();
    edges.sort_by(|&a, &b| {
        let length = |i: usize| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            (p[0] - q[0]).hypot(p[1] - q[1])
        };
        length(b).total_cmp(&length(a))
    });
    edges.truncate(12);
    edges.sort_unstable();
    let lines: Vec<_> = edges
        .iter()
        .map(|&i| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            let a = p[1] - q[1];
            let b = q[0] - p[0];
            [
                a,
                b,
                a * p[0] + b * p[1] - pixel_margin * (a.abs() + b.abs()),
            ]
        })
        .collect();
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for a in 0..lines.len() {
        for b in a + 1..lines.len() {
            for c in b + 1..lines.len() {
                for d in c + 1..lines.len() {
                    let selected = [lines[a], lines[b], lines[c], lines[d]];
                    let mut quad = [[0.; 2]; 4];
                    let mut valid = true;
                    for i in 0..4 {
                        let p = selected[i];
                        let q = selected[(i + 1) % 4];
                        let denominator = p[0] * q[1] - q[0] * p[1];
                        if denominator.abs() < 1e-5 {
                            valid = false;
                            break;
                        }
                        quad[i] = [
                            (p[2] * q[1] - q[2] * p[1]) / denominator,
                            (p[0] * q[2] - q[0] * p[2]) / denominator,
                        ];
                        if selected
                            .iter()
                            .any(|l| l[0] * quad[i][0] + l[1] * quad[i][1] < l[2] - 0.02)
                        {
                            valid = false;
                            break;
                        }
                    }
                    if !valid {
                        continue;
                    }
                    let area = (0..4)
                        .map(|i| {
                            let p = quad[i];
                            let q = quad[(i + 1) % 4];
                            p[0] * q[1] - p[1] * q[0]
                        })
                        .sum::<f32>()
                        * 0.5;
                    if area > 0. && area < best_area {
                        best_area = area;
                        best = Some(quad);
                    }
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod intersection_cache_tests {
    use super::*;
    #[test]
    fn cached_intersections_preserve_exact_enclosing_quad() {
        let mut seed = 1341_u32;
        for _ in 0..100 {
            let points = (0..64)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let x = f32::from(u16::try_from(seed % 1200).unwrap());
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    [x, f32::from(u16::try_from(seed % 1200).unwrap())]
                })
                .collect();
            let polygon = hull(points);
            for margin in [0., 0.25, 1.] {
                assert_eq!(
                    enclosing_quad(&polygon, margin, &mut false),
                    enclosing_quad_reference(&polygon, margin)
                );
            }
        }
    }
}

#[cfg(test)]
mod l_border_gate_tests {
    use super::*;
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "Test-only legacy pixel sampling reference deliberately retains the exact prior arithmetic and checked image indices."
    )]
    fn l_border_edges_reference(
        image: &[bool],
        w: usize,
        h: usize,
        quad: [Point; 4],
    ) -> [usize; 4] {
        let center = quad
            .iter()
            .fold([0., 0.], |s, p| [s[0] + p[0] * 0.25, s[1] + p[1] * 0.25]);
        let mut solid = [0; 4];
        for edge in 0..4 {
            let edge_start = quad[edge];
            let edge_end = quad[(edge + 1) % 4];
            let inward = [
                center[0] - (edge_start[0] + edge_end[0]) * 0.5,
                center[1] - (edge_start[1] + edge_end[1]) * 0.5,
            ];
            let length = inward[0].hypot(inward[1]);
            if length < 1. {
                return [0; 4];
            }
            for offset in [0.25, 0.75, 1.5, length / 32., length / 12.] {
                let mut black = 0;
                for i in 0..24 {
                    let fraction = (i as f32 + 0.2 + (i as f32 * 0.381_966).fract() * 0.6) / 24.;
                    let x = (edge_start[0]
                        + fraction * (edge_end[0] - edge_start[0])
                        + inward[0] * offset / length)
                        .floor() as isize;
                    let y = (edge_start[1]
                        + fraction * (edge_end[1] - edge_start[1])
                        + inward[1] * offset / length)
                        .floor() as isize;
                    if x >= 0
                        && y >= 0
                        && x < w as isize
                        && y < h as isize
                        && image[y as usize * w + x as usize]
                    {
                        black += 1;
                    }
                }
                solid[edge] = solid[edge].max(black);
                if black >= 18 {
                    break;
                }
            }
        }
        solid
    }
    #[test]
    fn early_sampling_preserves_every_used_border_class() {
        let mut seed = 1487_u32;
        for case in 0..200 {
            let image: Vec<bool> = (0..96 * 96)
                .map(|i| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    match case % 4 {
                        0 => seed % 7 < 4,
                        1 => (i % 96) % 13 < 9,
                        2 => (i / 96) % 13 < 9,
                        _ => !seed.is_multiple_of(31),
                    }
                })
                .collect();
            let shift = f32::from(u8::try_from(case % 17).unwrap());
            let quad = [[-2. + shift, 1.], [81., 3. + shift], [93., 91.], [4., 87.]];
            let actual = l_border_edges(&image, 96, 96, quad);
            let expected = l_border_edges_reference(&image, 96, 96, quad);
            for threshold in [15, 18] {
                assert_eq!(
                    actual.map(|n| n >= threshold),
                    expected.map(|n| n >= threshold)
                );
            }
        }
    }
}
