use super::*;
#[test]
fn requires_shared_interior_module_position() {
    let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
    let b = [[20., 30.], [400., 70.], [400., 72.], [20., 32.]];
    assert!(crossing_read_paths(a, b));
    assert!(crossing_read_paths(a, [b[2], b[3], b[0], b[1]]));
    assert!(!crossing_read_paths(a, a.map(|p| [p[0], p[1] + 30.])));
    assert!(!crossing_read_paths(a, b.map(|p| [p[0] + 40., p[1]])));
    let rot = |p: [f64; 2]| [-p[1], p[0]];
    assert!(crossing_read_paths(a.map(rot), b.map(rot)));
}
