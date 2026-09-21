use serde_json::Value;

pub(crate) fn quad(value: &Value) -> crate::Quad {
    std::array::from_fn(|i| std::array::from_fn(|j| value["polygon"][i][j].as_f64().unwrap_or(0.0)))
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = points[(i + 1) % points.len()];
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}

pub(crate) fn overlap(first: &Value, second: &Value) -> (f64, f64) {
    overlap_quads(&quad(first), &quad(second))
}

/// Same convex clipping and 0.65 reconciliation basis as the research host.
pub(crate) fn overlap_quads(first_quad: &crate::Quad, second_quad: &crate::Quad) -> (f64, f64) {
    let first_area = signed_area(first_quad).abs();
    let second_area = signed_area(second_quad).abs();
    if first_area.min(second_area) < 1e-6 {
        return (0.0, 0.0);
    }
    let winding = signed_area(second_quad).signum();
    let mut points = first_quad.to_vec();
    for edge_index in 0..4 {
        if points.is_empty() {
            break;
        }
        let edge_start = second_quad[edge_index];
        let edge_end = second_quad[(edge_index + 1) % 4];
        let side = |point: [f64; 2]| {
            winding
                * ((edge_end[0] - edge_start[0]) * (point[1] - edge_start[1])
                    - (edge_end[1] - edge_start[1]) * (point[0] - edge_start[0]))
        };
        let mut clipped = Vec::new();
        for point_index in 0..points.len() {
            let current = points[point_index];
            let next = points[(point_index + 1) % points.len()];
            let current_side = side(current);
            let next_side = side(next);
            if current_side >= 0.0 {
                clipped.push(current);
            }
            if (current_side >= 0.0) != (next_side >= 0.0) {
                let fraction = current_side / (current_side - next_side);
                clipped.push([
                    current[0] + fraction * (next[0] - current[0]),
                    current[1] + fraction * (next[1] - current[1]),
                ]);
            }
        }
        points = clipped;
    }
    let intersection = first_area.min(second_area).min(signed_area(&points).abs());
    (
        intersection / first_area.min(second_area),
        intersection / first_area,
    )
}
