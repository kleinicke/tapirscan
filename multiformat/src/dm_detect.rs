//! Component hull proposals with ECC200 border scoring and projective sampling.
use crate::{Detection, datamatrix, qr_detect::map};
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
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    y: usize,
    left: usize,
    right: usize,
}

#[derive(Clone, Debug, Default)]
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

fn union_components(
    components: &mut [Component],
    a: usize,
    b: usize,
    merged: &mut Vec<usize>,
    pool: &mut Vec<Vec<Span>>,
) -> usize {
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
    merged.push(child);
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
    if spans.capacity() > 0 {
        pool.push(spans);
    }
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
//
// A component is complete once no run of the current row continues it. Then it
// is kept if eligible (or for tests) and its slot is recycled, like the slots of
// components merged into another root, so memory follows one row of runs rather
// than the whole image. Downstream orders proposals by size and origin, so the
// order in which components complete does not matter.
fn fast_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    fast_components_impl(image, w, h, false)
}

#[cfg(test)]
fn fast_all_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    fast_components_impl(image, w, h, true)
}

fn fast_components_impl(
    image: &[bool],
    w: usize,
    h: usize,
    retain_ineligible_for_test: bool,
) -> Vec<Component> {
    let mut components: Vec<Component> = Vec::new();
    let mut free: Vec<usize> = Vec::new();
    let mut merged: Vec<usize> = Vec::new();
    // Row in which each slot's root last had a run; marks completed roots.
    let mut seen_row: Vec<usize> = Vec::new();
    let mut done = Vec::new();
    let mut previous: Vec<(usize, usize, usize)> = Vec::new();
    let mut current: Vec<(usize, usize, usize)> = Vec::new();
    // Span vectors of discarded components are reused, so texture specks that
    // span a few rows do not allocate one each.
    let mut pool: Vec<Vec<Span>> = Vec::new();
    let finish = |component: Component, done: &mut Vec<Component>, pool: &mut Vec<Vec<Span>>| {
        if retain_ineligible_for_test || component_is_eligible(&component) {
            done.push(component);
        } else if component.spans.capacity() > 0 {
            let mut spans = component.spans;
            spans.clear();
            pool.push(spans);
        }
    };
    let mut offsets = Vec::new();
    for y in 0..h {
        let row = y * w;
        let line = &image[row..row + w];
        current.clear();
        let mut previous_start = 0;
        // Word-wise transitions give the same runs, left to right: offsets start
        // at 0 and end at w, and runs of set pixels alternate with unset ones.
        crate::transition_offsets_into(line, &mut offsets);
        let first_set = usize::from(!line.first().copied().unwrap_or(false));
        for &[left, right] in offsets[first_set..].as_chunks::<2>().0 {
            while previous_start < previous.len() && previous[previous_start].1 < left {
                previous_start += 1;
            }
            let mut overlap = previous_start;
            let mut label = None;
            while overlap < previous.len() && previous[overlap].0 <= right {
                let previous_label = find_component(&mut components, previous[overlap].2);
                label = Some(match label {
                    Some(existing) => union_components(
                        &mut components,
                        existing,
                        previous_label,
                        &mut merged,
                        &mut pool,
                    ),
                    None => previous_label,
                });
                overlap += 1;
            }
            let span = Span { y, left, right };
            let label = if let Some(label) = label {
                add_span(&mut components, label, span);
                find_component(&mut components, label)
            } else {
                let fresh = Component {
                    parent: 0,
                    origin: row + left,
                    count: right - left,
                    x0: left,
                    x1: right - 1,
                    y0: y,
                    y1: y,
                    first_span: span,
                    spans: pool.pop().unwrap_or_default(),
                };
                let index = if let Some(index) = free.pop() {
                    components[index] = fresh;
                    index
                } else {
                    components.push(fresh);
                    seen_row.push(usize::MAX);
                    components.len() - 1
                };
                components[index].parent = index;
                index
            };
            current.push((left, right, label));
        }
        // Resolve this row's labels to roots and mark them as continuing.
        for run in &mut current {
            run.2 = find_component(&mut components, run.2);
            seen_row[run.2] = y;
        }
        // Roots of the previous row that no current run continues are complete.
        for &(_, _, label) in &previous {
            let root = find_component(&mut components, label);
            if seen_row[root] != y {
                seen_row[root] = y;
                finish(std::mem::take(&mut components[root]), &mut done, &mut pool);
                // Other runs of this row may still look the root up.
                components[root].parent = root;
                free.push(root);
            }
        }
        // Merged children are referenced by no run once labels are resolved.
        free.append(&mut merged);
        std::mem::swap(&mut previous, &mut current);
    }
    for &(_, _, root) in &previous {
        if seen_row[root] != h {
            seen_row[root] = h;
            finish(std::mem::take(&mut components[root]), &mut done, &mut pool);
            components[root].parent = root;
        }
    }
    done
}

// The flood-fill extractor the streaming labeller replaced; tests compare the two.
#[cfg(test)]
fn legacy_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    legacy_components_impl(image, w, h, false)
}

#[cfg(test)]
fn legacy_all_components(image: &[bool], w: usize, h: usize) -> Vec<Component> {
    legacy_components_impl(image, w, h, true)
}

#[cfg(test)]
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
    // Reused across components: texture yields millions of tiny components.
    let mut stack = Vec::new();
    let mut touched_rows = Vec::new();
    for origin in 0..image.len() {
        if !image[origin] || seen[origin] {
            continue;
        }
        stack.clear();
        stack.push(origin);
        touched_rows.clear();
        let mut count = 0;
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
            for &y in &touched_rows {
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
            for &y in &touched_rows {
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
    let components = fast_components(image, w, h);
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
            signatures(fast_all_components(image, w, h), h),
            signatures(legacy_all_components(image, w, h), h),
            "component count, bounds, row extremes, or hull changed"
        );
        assert_eq!(
            proposals_from_components(fast_components(image, w, h), h),
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
        assert!(touched_rows.is_empty(), "{touched_rows:?}");
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
        let components = fast_all_components(&image, w, h);
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
        let components = fast_all_components(&image, w, h);
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
    fn checkerboards_and_dense_noise_match_the_legacy_extractor() {
        // These once exceeded the run budget of the non-streaming labeller.
        let (w, h) = (512, 512);
        let checkerboard: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x + y) % 2 == 0))
            .collect();
        assert_fast_matches_legacy(&checkerboard, w, h);
        let mut seed = 0x2f6b_1c3d_u32;
        for (w, h, cut) in [(640, 480, 1), (1200, 900, 2), (333, 777, 3)] {
            let image: Vec<_> = (0..w * h)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    seed >> 30 >= cut
                })
                .collect();
            assert_fast_matches_legacy(&image, w, h);
        }
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
/// Whether the binary image just inside one open side of a candidate is a flat line.
/// A clock track alternates along its whole length, so it shows at least five
/// transitions at one of several depths; solid blocks, rules and blank areas do not.
/// A side that leaves the frame is not called flat, so nothing is rejected on a guess.
fn clock_track_is_flat(
    image: &[bool],
    (width, height): (usize, usize),
    start: Point,
    end: Point,
    toward: Point,
) -> bool {
    const DEPTHS: [f32; 5] = [0.75, 1.5, 3., 5., 8.];
    const STEPS: usize = 160;
    let along = [end[0] - start[0], end[1] - start[1]];
    let length = along[0].hypot(along[1]);
    if length < 12. {
        return false;
    }
    let direction = [along[0] / length, along[1] / length];
    // Component of the direction to the opposite side that is perpendicular to the side.
    let dot = toward[0] * direction[0] + toward[1] * direction[1];
    let perpendicular = [
        toward[0] - dot * direction[0],
        toward[1] - dot * direction[1],
    ];
    let norm = perpendicular[0].hypot(perpendicular[1]);
    if norm < 1e-3 {
        return false;
    }
    let inward = [perpendicular[0] / norm, perpendicular[1] / norm];
    let steps = crate::numeric::f32_usize(length).clamp(16, STEPS);
    for depth in DEPTHS {
        let mut transitions = 0;
        let mut previous = false;
        for step in 0..steps {
            let at =
                (crate::numeric::usize_f32(step) + 0.5) / crate::numeric::usize_f32(steps) * length;
            let column = crate::numeric::f32_isize(
                (start[0] + direction[0] * at + inward[0] * depth).floor(),
            );
            let row = crate::numeric::f32_isize(
                (start[1] + direction[1] * at + inward[1] * depth).floor(),
            );
            if column < 0 || row < 0 || column >= width.cast_signed() || row >= height.cast_signed()
            {
                return false;
            }
            let bit = image[row.cast_unsigned() * width + column.cast_unsigned()];
            transitions += usize::from(step > 0 && bit != previous);
            previous = bit;
        }
        // The two outer runs are cut by the ends of the quad.
        if transitions >= 5 {
            return false;
        }
    }
    true
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
            center[0] - f32::midpoint(edge_start[0], edge_end[0]),
            center[1] - f32::midpoint(edge_start[1], edge_end[1]),
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
pub(crate) fn rectangle_transform(cols: usize, rows: usize, q: [Point; 4]) -> Option<[f32; 8]> {
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
    border_with_limit(image, w, h, transform, cols, rows, 2)
}
/// Border error fraction with at most `(cols + rows) * 2 * fifths / 5` wrong samples.
fn border_with_limit(
    image: &[bool],
    w: usize,
    h: usize,
    transform: &[f32; 8],
    cols: usize,
    rows: usize,
    fifths: usize,
) -> Option<f32> {
    let mut errors = 0;
    let error_limit = (cols + rows) * fifths / 5;
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
/// Hypotheses whose border is fair but not clean usually come from a hull quad a
/// few pixels off a blurred or strongly tilted symbol. Coordinate descent on
/// the corners, scored on five samples per border module, tightens the grid;
/// the refined hypothesis competes next to the original ones.
const REFINE_LIMIT: usize = 24;
const REFINE_SEED_LIMIT: f32 = 0.32;

fn fine_border_errors(
    image: &[bool],
    w: usize,
    h: usize,
    transform: &[f32; 8],
    cols: usize,
    rows: usize,
) -> Option<usize> {
    const OFFSETS: [[f32; 2]; 5] = [[0., 0.], [0.3, 0.], [-0.3, 0.], [0., 0.3], [0., -0.3]];
    let mut errors = 0;
    for (edge, length) in [(0, cols), (1, rows), (2, cols), (3, rows)] {
        for at in 0..length {
            let (x, y, expected) = match edge {
                0 => (at, 0, at % 2 == 0),
                1 => (cols - 1, at, at % 2 == 1),
                2 => (at, rows - 1, true),
                _ => (0, at, true),
            };
            for offset in OFFSETS {
                errors += usize::from(
                    pixel(
                        image,
                        w,
                        h,
                        transform,
                        crate::numeric::usize_f32(x) + 0.5 + offset[0],
                        crate::numeric::usize_f32(y) + 0.5 + offset[1],
                    )? != expected,
                );
            }
        }
    }
    Some(errors)
}

fn refine_hypotheses(
    image: &[bool],
    w: usize,
    h: usize,
    hypotheses: &mut Vec<(f32, datamatrix::Size, [f32; 8], [Point; 4])>,
    refinements: &mut usize,
) {
    let mut refined = Vec::new();
    for &(score, size, _, polygon) in hypotheses.iter().take(2) {
        if !(0.03..=REFINE_SEED_LIMIT).contains(&score) || *refinements >= REFINE_LIMIT {
            continue;
        }
        *refinements += 1;
        let evaluate = |q: &[Point; 4]| {
            rectangle_transform(size.w, size.h, *q)
                .and_then(|t| fine_border_errors(image, w, h, &t, size.w, size.h))
        };
        let Some(mut best) = evaluate(&polygon) else {
            continue;
        };
        let mut quad = polygon;
        let module = (quad[1][0] - quad[0][0]).hypot(quad[1][1] - quad[0][1])
            / crate::numeric::usize_f32(size.w);
        let mut step = (module * 0.5).max(1.);
        while step >= 0.2 {
            let mut improved = false;
            for corner in 0..4 {
                for axis in 0..2 {
                    for sign in [1., -1.] {
                        let mut candidate = quad;
                        candidate[corner][axis] += sign * step;
                        if let Some(errors) = evaluate(&candidate)
                            && errors < best
                        {
                            best = errors;
                            quad = candidate;
                            improved = true;
                        }
                    }
                }
            }
            if !improved {
                step *= 0.5;
            }
        }
        let Some(t) = rectangle_transform(size.w, size.h, quad) else {
            continue;
        };
        if let Some(new_score) = border(image, w, h, &t, size.w, size.h)
            && new_score < score
        {
            refined.push((new_score, size, t, quad));
        }
    }
    hypotheses.extend(refined);
    hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
}

/// Whether a read already covers the center of this candidate.
fn near_result(results: &[Detection], quad: &[Point; 4]) -> bool {
    let center = |poly: &[Point]| {
        poly.iter()
            .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.])
    };
    let c = center(quad);
    results.iter().any(|r| {
        let d = center(&r.polygon);
        (c[0] - d[0]).hypot(c[1] - d[1]) < 8.
    })
}
/// Detect with the retry limits of a scanner `mode` (0 Low, 1 Medium, 2 High, 3 Very
/// High); `refine` also refines the corners of every fair hypothesis.
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
    refine: bool,
    mode: usize,
) -> (Vec<Detection>, bool) {
    let mode = mode.min(2);
    detect_with_retries(
        w,
        h,
        regions,
        binary_images,
        refine,
        Retries {
            gray: GRAY_ATTEMPTS[mode],
            region_grids: mode >= 2,
            crops: CROP_RETRIES[mode],
            candidates: RETRY_CANDIDATES[mode],
            ranked: RANKED_CANDIDATES[mode],
        },
    )
}
/// Limits of the retries of one frame.
#[derive(Clone, Copy)]
struct Retries {
    /// Grayscale refinements.
    gray: usize,
    /// Whether the grayscale refinement refines the data regions of large symbols apart.
    region_grids: bool,
    /// Unread candidate boxes retried on area-averaged or enlarged crops.
    crops: usize,
    /// Failing candidates kept for the retries, which spend their work on the best-scoring
    /// ones; gathering a candidate costs little.
    candidates: usize,
    /// Whether the list keeps the best-ranked candidates with a possible orientation
    /// instead of the first ones found. Real-looking candidates cost more retry work
    /// than bare L borders, so only a short list is ranked.
    ranked: bool,
}
/// Retry limits per frame for Low, Medium and High (and Very High). Medium retries its
/// four best candidates; Low and High the first 64 (High ranking them cost about 3% in
/// Chrome on retail photos for no read).
const GRAY_ATTEMPTS: [usize; 3] = [3, 3, 6];
const CROP_RETRIES: [usize; 3] = [1, 1, 2];
const RETRY_CANDIDATES: [usize; 3] = [64, 4, 64];
const RANKED_CANDIDATES: [bool; 3] = [false, true, false];
/// Sizes per candidate that the grayscale refinement tries.
const GRAY_SIZES_PER_CANDIDATE: usize = 2;
/// The detection on a crop retries only a little itself and never crops again.
const CROP_PASS: Retries = Retries {
    gray: 3,
    region_grids: false,
    crops: 0,
    candidates: 64,
    ranked: false,
};
#[expect(
    clippy::too_many_lines,
    reason = "The Data Matrix search shares proposal ordering, hypothesis caps and undecoded coverage across its sampling passes."
)]
fn detect_with_retries(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
    refine: bool,
    retries: Retries,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let mut refinements = 0;
    let gray = binary_images.gray();
    let mut gray_attempts = retries.gray;
    let mut failed: Vec<(Vec<[Point; 4]>, usize, f32)> = Vec::new();
    let mut unread: Vec<[Point; 4]> = Vec::new();
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
            let mut flat: [Option<bool>; 4] = [None; 4];
            let mut hypotheses = Vec::new();
            let mut orientations: Vec<[Point; 4]> = Vec::new();
            let mut strength = 0;
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
                    // A real symbol alternates along both open sides.
                    let (top, right) = if mirror {
                        ((rotation + 3) % 4, (rotation + 2) % 4)
                    } else {
                        (rotation, (rotation + 1) % 4)
                    };
                    if [top, right].into_iter().any(|index| {
                        *flat[index].get_or_insert_with(|| {
                            let (start, end) = (original[index], original[(index + 1) % 4]);
                            let opposite = original[(index + 2) % 4];
                            clock_track_is_flat(
                                image,
                                (w, h),
                                start,
                                end,
                                [opposite[0] - start[0], opposite[1] - start[1]],
                            )
                        })
                    }) {
                        continue;
                    }
                    orientations.push(q);
                    strength = strength.max(solid[left_edge] + solid[bottom_edge]);
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
                            if let Some(score) = border(image, w, h, &t, size.w, size.h)
                                && score <= if refine { REFINE_SEED_LIMIT } else { 0.2 }
                            {
                                hypotheses.push((score, *size, t, expanded));
                            }
                        }
                    }
                }
            }
            hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
            if refine {
                refine_hypotheses(image, w, h, &mut hypotheses, &mut refinements);
            }
            regions.limited |= hypotheses.len() > 12;
            // Rank failures by their best hypothesis, then by the strength of their L.
            let best_score = hypotheses.first().map_or_else(
                || 1. + (36. - crate::numeric::usize_f32(strength)) / 36.,
                |h| h.0,
            );
            let before = results.len();
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
            // Busy frames fill the list with bare Ls of earlier thresholds. A ranked list
            // skips candidates without a possible orientation (never retried) and, when
            // full, gives its worst entry to a better one.
            if results.len() == before && (!retries.ranked || !orientations.is_empty()) {
                if failed.len() < retries.candidates {
                    failed.push((orientations, mode, best_score));
                } else if retries.ranked
                    && let Some(worst) =
                        (0..failed.len()).max_by(|&a, &b| failed[a].2.total_cmp(&failed[b].2))
                    && failed[worst].2 > best_score
                {
                    failed[worst] = (orientations, mode, best_score);
                }
            }
        }
    }
    // Strong perspective foreshortens one axis of a square symbol past the
    // ratio window of the first pass; unread candidates retry with a wider one.
    // Fair but unclean borders queue for the grayscale refinement below.
    let mut queue: Vec<(f32, datamatrix::Size, [Point; 4], bool)> = Vec::new();
    for (orientations, mode, _) in failed {
        if orientations
            .first()
            .is_some_and(|first| near_result(&results, first))
        {
            continue;
        }
        let image = binary_images.get(mode);
        let mut hypotheses = Vec::new();
        let mut fair = Vec::new();
        for &q in &orientations {
            let dx = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
            let dy = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
            for size in datamatrix::SIZES {
                let ratio = (dx / dy)
                    / (crate::numeric::usize_f32(size.w) / crate::numeric::usize_f32(size.h));
                if !(0.5..=2.0).contains(&ratio)
                    || dx / crate::numeric::usize_f32(size.w) < 0.8
                    || dy / crate::numeric::usize_f32(size.h) < 0.8
                {
                    continue;
                }
                let Some(t) = rectangle_transform(size.w, size.h, q) else {
                    continue;
                };
                let Some(score) = border_with_limit(image, w, h, &t, size.w, size.h, 2) else {
                    continue;
                };
                if score <= 0.2 && !(0.65..=1.55).contains(&ratio) {
                    hypotheses.push((score, *size, t, q));
                }
                fair.push((score, *size, q, mode >= 2));
            }
        }
        hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
        if refine {
            refine_hypotheses(image, w, h, &mut hypotheses, &mut refinements);
        }
        let mut found = None;
        for (score, size, t, polygon) in hypotheses.into_iter().take(12) {
            attempts += 1;
            if attempts > 800 {
                break;
            }
            let mut matrix = Vec::with_capacity(size.w * size.h);
            for y in 0..size.h {
                for x in 0..size.w {
                    let at = [
                        crate::numeric::usize_f32(x) + 0.5,
                        crate::numeric::usize_f32(y) + 0.5,
                    ];
                    if let Some(v) = pixel(image, w, h, &t, at[0], at[1]) {
                        matrix.push(v);
                    }
                }
            }
            if matrix.len() == size.w * size.h
                && let Some(read) = datamatrix::decode_matrix(&matrix, size.w, size.h)
            {
                found = Some(Detection {
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
        if let Some(read) = found {
            results.push(read);
        } else {
            // Only candidates with a clean-looking border earn an averaged crop.
            if !fair.is_empty() {
                unread.push(orientations[0]);
            }
            fair.sort_by(|a, b| a.0.total_cmp(&b.0));
            queue.extend(fair.into_iter().take(GRAY_SIZES_PER_CANDIDATE));
        }
    }
    queue.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (score, size, q, inverted) in queue {
        if gray_attempts == 0 {
            break;
        }
        if near_result(&results, &q) {
            continue;
        }
        gray_attempts -= 1;
        if let Some((read, polygon)) =
            crate::dm_gray::recover(gray, (w, h), q, size, inverted, retries.region_grids)
        {
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
        }
    }
    if retries.crops > 0 {
        results.extend(crop_retries_for(
            gray,
            (w, h),
            &unread,
            &results,
            retries.crops,
        ));
    }
    (results, false)
}

/// Bounding box of a quad padded by `pad` of its size, clamped to the frame.
fn padded_box(quad: &[Point; 4], pad: f32, (w, h): (usize, usize)) -> Option<[usize; 4]> {
    let low = |axis: usize| quad.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min);
    let high = |axis: usize| {
        quad.iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let (x0, x1, y0, y1) = (low(0), high(0), low(1), high(1));
    let (px, py) = ((x1 - x0) * pad, (y1 - y0) * pad);
    let clamp = |v: f32, limit: usize| {
        crate::numeric::f32_usize(v.max(0.).min(crate::numeric::usize_f32(limit)))
    };
    let result = [
        clamp(x0 - px, w),
        clamp(y0 - py, h),
        clamp(x1 + px, w),
        clamp(y1 + py, h),
    ];
    (result[2] > result[0] + 8 && result[3] > result[1] + 8).then_some(result)
}

/// Longest side of an unread candidate box that is enlarged instead of reduced.
const SMALL_BOX: usize = 96;

/// Bilinear enlargement of a crop by an integer factor.
fn upscale_crop(
    gray: &[u8],
    stride: usize,
    [x0, y0, x1, y1]: [usize; 4],
    factor: usize,
) -> Vec<u8> {
    let (cw, ch) = (x1 - x0, y1 - y0);
    let (out_w, out_h) = (cw * factor, ch * factor);
    let scale = crate::numeric::usize_f32(factor);
    let mut out = vec![0_u8; out_w * out_h];
    for y in 0..out_h {
        let fy = ((crate::numeric::usize_f32(y) + 0.5) / scale - 0.5)
            .clamp(0., crate::numeric::usize_f32(ch - 1));
        let (ya, wy) = (crate::numeric::f32_usize(fy.floor()), fy.fract());
        let yb = (ya + 1).min(ch - 1);
        for x in 0..out_w {
            let fx = ((crate::numeric::usize_f32(x) + 0.5) / scale - 0.5)
                .clamp(0., crate::numeric::usize_f32(cw - 1));
            let (xa, wx) = (crate::numeric::f32_usize(fx.floor()), fx.fract());
            let xb = (xa + 1).min(cw - 1);
            let at = |row: usize, column: usize| f32::from(gray[(y0 + row) * stride + x0 + column]);
            let top = at(ya, xa) * (1. - wx) + at(ya, xb) * wx;
            let bottom = at(yb, xa) * (1. - wx) + at(yb, xb) * wx;
            out[y * out_w + x] = crate::numeric::f32_u8((top * (1. - wy) + bottom * wy).round());
        }
    }
    out
}

/// Area-average a crop to the given size.
fn average_crop(
    gray: &[u8],
    stride: usize,
    [x0, y0, x1, y1]: [usize; 4],
    (out_w, out_h): (usize, usize),
) -> Vec<u8> {
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = vec![0_u8; out_w * out_h];
    for y in 0..out_h {
        let (ya, yb) = (
            y * ch / out_h,
            ((y + 1) * ch / out_h).max(y * ch / out_h + 1).min(ch),
        );
        for x in 0..out_w {
            let (xa, xb) = (
                x * cw / out_w,
                ((x + 1) * cw / out_w).max(x * cw / out_w + 1).min(cw),
            );
            // At most four evenly spaced samples per axis keep the pass cheap.
            let (ys, xs) = ((yb - ya).min(4), (xb - xa).min(4));
            let mut sum = 0_u32;
            for j in 0..ys {
                let row = y0 + ya + (2 * j + 1) * (yb - ya) / (2 * ys);
                for i in 0..xs {
                    let column = x0 + xa + (2 * i + 1) * (xb - xa) / (2 * xs);
                    sum += u32::from(gray[row * stride + column]);
                }
            }
            let count = u32::try_from(ys * xs).unwrap_or(1);
            out[y * out_w + x] = u8::try_from((sum + count / 2) / count).unwrap_or(255);
        }
    }
    out
}

/// Reads of a candidate box, detected on a bilinear enlargement by `factor` (1 is the
/// source scale) and mapped back to source coordinates.
fn enlarged_reads(
    gray: &[u8],
    stride: usize,
    candidate: [usize; 4],
    factor: usize,
) -> Vec<Detection> {
    let (cw, ch) = (candidate[2] - candidate[0], candidate[3] - candidate[1]);
    let (out_w, out_h) = (cw * factor, ch * factor);
    let large = upscale_crop(gray, stride, candidate, factor);
    let mut crop_regions = crate::regions::Regions::default();
    let mut images = crate::binarization::Images::new(&large, out_w, out_h);
    let (mut reads, _) = detect_with_retries(
        out_w,
        out_h,
        &mut crop_regions,
        &mut images,
        true,
        CROP_PASS,
    );
    let scale = crate::numeric::usize_f32(factor);
    for read in &mut reads {
        for point in &mut read.polygon {
            *point = [
                crate::numeric::usize_f32(candidate[0]) + point[0] / scale,
                crate::numeric::usize_f32(candidate[1]) + point[1] / scale,
            ];
        }
    }
    reads
}

/// Retry the largest small or mid-size boxes (with their enlargement factors) that no
/// read covers, adding what they read to `found`.
fn enlarged_retries(
    gray: &[u8],
    stride: usize,
    mut boxes: Vec<([usize; 4], usize)>,
    limit: usize,
    results: &[Detection],
    found: &mut Vec<Detection>,
) {
    boxes.sort_by_key(|(b, _)| std::cmp::Reverse((b[2] - b[0]) * (b[3] - b[1])));
    boxes.dedup_by(|(a, _), (b, _)| {
        (a[0].abs_diff(b[0]) + a[1].abs_diff(b[1])) * 4 < (a[2] - a[0]).max(a[3] - a[1])
    });
    for (candidate, factor) in boxes.into_iter().take(limit) {
        for read in enlarged_reads(gray, stride, candidate, factor) {
            if !near_result(found, &read.polygon) && !near_result(results, &read.polygon) {
                found.push(read);
            }
        }
    }
}

/// Large symbols whose modules span tens of pixels leave candidates that do not
/// decode at full resolution, and tiny ones with two-pixel modules leave candidates the
/// binary pass cannot sample. Retry the most extensive unread candidates on
/// area-averaged crops and the small ones on enlarged crops, which both removes the module blur and keeps the cost
/// proportional to the candidate, not the frame.
pub(crate) fn crop_retries_for(
    gray: &[u8],
    (w, h): (usize, usize),
    unread: &[[Point; 4]],
    results: &[Detection],
    limit: usize,
) -> Vec<Detection> {
    const TARGETS: [usize; 2] = [192, 96];
    let targets = &TARGETS[..if limit > 1 { 2 } else { 1 }];
    let mut boxes: Vec<[usize; 4]> = Vec::new();
    let mut smalls: Vec<([usize; 4], usize)> = Vec::new();
    for quad in unread {
        if near_result(results, quad) {
            continue;
        }
        let Some(candidate) = padded_box(quad, 0.12, (w, h)) else {
            continue;
        };
        let (cw, ch) = (candidate[2] - candidate[0], candidate[3] - candidate[1]);
        if cw.max(ch) <= SMALL_BOX && cw.max(ch) >= 20 && cw.max(ch) <= 3 * cw.min(ch) {
            if let Some(wide) = padded_box(quad, 0.3, (w, h)) {
                let long = (wide[2] - wide[0]).max(wide[3] - wide[1]);
                smalls.push((wide, (160 / long).clamp(2, 8)));
            }
            continue;
        }
        if cw.max(ch) < TARGETS[0] * 3 / 2 {
            // Between the two sizes, a crop at source scale gets a threshold of its own
            // (from High up, which retries two boxes).
            if limit > 1
                && cw.max(ch) > SMALL_BOX
                && cw.max(ch) <= 3 * cw.min(ch)
                && let Some(wide) = padded_box(quad, 0.3, (w, h))
            {
                smalls.push((wide, 1));
            }
            continue;
        }
        if cw.max(ch) > 3 * cw.min(ch) {
            continue;
        }
        let covered = |outer: &[usize; 4]| {
            candidate[0] >= outer[0]
                && candidate[1] >= outer[1]
                && candidate[2] <= outer[2]
                && candidate[3] <= outer[3]
        };
        boxes.retain(|other| {
            !(other[0] >= candidate[0]
                && other[1] >= candidate[1]
                && other[2] <= candidate[2]
                && other[3] <= candidate[3])
        });
        if !boxes.iter().any(covered) {
            boxes.push(candidate);
        }
    }
    boxes.sort_by_key(|b| std::cmp::Reverse((b[2] - b[0]) * (b[3] - b[1])));
    let mut found: Vec<Detection> = Vec::new();
    for candidate in boxes.into_iter().take(limit) {
        let (cw, ch) = (candidate[2] - candidate[0], candidate[3] - candidate[1]);
        let long = cw.max(ch);
        for &target in targets {
            if long < target * 3 / 2 {
                continue;
            }
            let (out_w, out_h) = ((cw * target / long).max(8), (ch * target / long).max(8));
            let small = average_crop(gray, w, candidate, (out_w, out_h));
            let mut crop_regions = crate::regions::Regions::default();
            let mut images = crate::binarization::Images::new(&small, out_w, out_h);
            let (reads, _) = detect_with_retries(
                out_w,
                out_h,
                &mut crop_regions,
                &mut images,
                true,
                CROP_PASS,
            );
            let scale = [
                crate::numeric::usize_f32(cw) / crate::numeric::usize_f32(out_w),
                crate::numeric::usize_f32(ch) / crate::numeric::usize_f32(out_h),
            ];
            let origin = [
                crate::numeric::usize_f32(candidate[0]),
                crate::numeric::usize_f32(candidate[1]),
            ];
            let before = found.len();
            for mut read in reads {
                for point in &mut read.polygon {
                    *point = [
                        origin[0] + point[0] * scale[0],
                        origin[1] + point[1] * scale[1],
                    ];
                }
                if !near_result(&found, &read.polygon) && !near_result(results, &read.polygon) {
                    found.push(read);
                }
            }
            if found.len() > before {
                break;
            }
        }
    }
    enlarged_retries(gray, w, smalls, limit, results, &mut found);
    found
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
    fn only_alternating_sides_are_clock_tracks() {
        let (w, h) = (64, 64);
        let side = ([8., 8.], [56., 8.], [0., 40.]);
        let mut image = vec![false; w * h];
        // A solid rule, then a blank area.
        for x in 8..56 {
            for y in 8..16 {
                image[y * w + x] = true;
            }
        }
        let flat = |image: &[bool]| clock_track_is_flat(image, (w, h), side.0, side.1, side.2);
        assert!(flat(&image));
        assert!(flat(&vec![false; w * h]));
        // Alternating modules of four pixels, the clock track of a 12-module symbol.
        for x in 8..56 {
            for y in 8..16 {
                image[y * w + x] = (x - 8) / 4 % 2 == 0;
            }
        }
        assert!(!flat(&image));
        // A side that leaves the frame is never called flat.
        assert!(!clock_track_is_flat(
            &image,
            (w, h),
            [8., 62.],
            [56., 62.],
            [0., 40.]
        ));
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
                center[0] - f32::midpoint(edge_start[0], edge_end[0]),
                center[1] - f32::midpoint(edge_start[1], edge_end[1]),
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
