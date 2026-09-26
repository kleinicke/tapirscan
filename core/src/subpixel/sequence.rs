//! Exact bounded second-order digit-chain fit to original-pixel normal equations.
//! Select the strongest visual sequence, then reject invalid checksum; never repair it.
use crate::ean;
const N: usize = 95;
const PARITY: [&[u8; 6]; 10] = [
    b"LLLLLL", b"LLGLGG", b"LLGGLG", b"LLGGGL", b"LGLLGG", b"LGGLLG", b"LGGGLL", b"LGLGLG",
    b"LGLGGL", b"LGGLGL",
];
const START: [usize; 12] = [3, 10, 17, 24, 31, 38, 50, 57, 64, 71, 78, 85];
#[derive(Clone, Copy)]
struct Path {
    cost: f64,
    digits: [u8; 12],
}
const EMPTY: Path = Path {
    cost: f64::INFINITY,
    digits: [0; 12],
};

#[expect(
    clippy::too_many_lines,
    reason = "The exact digit-chain dynamic program shares precomputed factors and ordered best/runner-up paths."
)]
pub(super) fn decode(
    gram: &[f64],
    rhs: &[f64; N],
    floor: f64,
    gain: f64,
    diagonal: f64,
    bandwidth: usize,
) -> Option<[u8; 13]> {
    if bandwidth > 14 || gain < 0.1 {
        return None;
    }
    let templates: [[[f64; 7]; 10]; 3] = std::array::from_fn(|side| {
        std::array::from_fn(|d| {
            let mut digits = [0; 13];
            digits[1] = u8::try_from(d).expect("digit index is below ten");
            let encoded = ean::encode(&digits);
            std::array::from_fn(|i| match side {
                0 => f64::from(encoded[3 + i]),
                1 => 1. - f64::from(encoded[3 + 6 - i]),
                _ => 1. - f64::from(encoded[3 + i]),
            })
        })
    });
    let guards = [0, 2, 46, 48, 92, 94];
    let adjusted: [f64; N] = std::array::from_fn(|j| {
        (rhs[j] - floor * gram[j * N..(j + 1) * N].iter().sum::<f64>()) / gain
            - guards.iter().map(|&k| gram[j * N + k]).sum::<f64>()
    });
    // Reuse all alphabet-pair factors across the ten leading-digit parity cases.
    let mut unary_cache = vec![0.; 12 * 3 * 10];
    let mut pair1_cache = vec![0.; 12 * 9 * 100];
    let mut pair2_cache = vec![0.; 12 * 9 * 100];
    for block in 0..12 {
        let start = START[block];
        for side in 0..3 {
            if (block < 6 && side == 2) || (block >= 6 && side != 2) {
                continue;
            }
            for d in 0..10 {
                let current = templates[side][d];
                let mut cost = 0.;
                for j in 0..7 {
                    cost -= 2. * current[j] * adjusted[start + j];
                    for k in 0..7 {
                        cost += current[j] * gram[(start + j) * N + start + k] * current[k];
                    }
                }
                unary_cache[(block * 3 + side) * 10 + d] = cost;
                for dist in 1..=2 {
                    if block < dist {
                        continue;
                    }
                    let old = START[block - dist];
                    if start.saturating_sub(old + 6) > bandwidth {
                        continue;
                    }
                    for (prev_side, previous_templates) in templates.iter().enumerate() {
                        if (block - dist < 6 && prev_side == 2)
                            || (block - dist >= 6 && prev_side != 2)
                        {
                            continue;
                        }
                        for (prev, previous_template) in previous_templates.iter().enumerate() {
                            let mut cross = 0.;
                            for j in 0..7 {
                                for k in (start + j).saturating_sub(old + bandwidth).min(7)..7 {
                                    cross += 2.
                                        * current[j]
                                        * gram[(start + j) * N + old + k]
                                        * previous_template[k];
                                }
                            }
                            let index = (block * 9 + side * 3 + prev_side) * 100 + prev * 10 + d;
                            if dist == 1 {
                                pair1_cache[index] = cross;
                            } else {
                                pair2_cache[index] = cross;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut best = (f64::INFINITY, [0; 13]);
    let mut second = f64::INFINITY;
    for (first, parity) in PARITY.iter().enumerate() {
        let side: [usize; 12] = std::array::from_fn(|i| {
            if i >= 6 {
                2
            } else {
                usize::from(parity[i] == b'G')
            }
        });
        let mut unary = [[0.; 10]; 12];
        let mut pair1 = [[0.; 100]; 12];
        let mut pair2 = [[0.; 100]; 12];
        for block in 0..12 {
            let at = (block * 3 + side[block]) * 10;
            unary[block].copy_from_slice(&unary_cache[at..at + 10]);
            if block >= 1 {
                let at = (block * 9 + side[block] * 3 + side[block - 1]) * 100;
                pair1[block].copy_from_slice(&pair1_cache[at..at + 100]);
            }
            if block >= 2 {
                let at = (block * 9 + side[block] * 3 + side[block - 2]) * 100;
                pair2[block].copy_from_slice(&pair2_cache[at..at + 100]);
            }
        }
        // At bandwidth < 8, no pixel couples digits two blocks apart. The exact
        // chain then needs only the previous digit, reducing state work tenfold.
        let second_order = bandwidth >= 8;
        let state_count = if second_order { 100 } else { 10 };
        let mut states = [[EMPTY; 2]; 100];
        for d in 0..10 {
            states[d][0].cost = unary[0][d];
            states[d][0].digits[0] = u8::try_from(d).expect("digit index is below ten");
        }
        for block in 1..12 {
            let mut next = [[EMPTY; 2]; 100];
            for (state, paths) in states.iter().enumerate().take(state_count) {
                for path in *paths {
                    if !path.cost.is_finite() {
                        continue;
                    }
                    let prev = state % 10;
                    let prev2 = state / 10;
                    for d in 0..10 {
                        let cost = path.cost
                            + unary[block][d]
                            + pair1[block][prev * 10 + d]
                            + if second_order && block >= 2 {
                                pair2[block][prev2 * 10 + d]
                            } else {
                                0.
                            };
                        let index = if second_order { prev * 10 + d } else { d };
                        if cost < next[index][1].cost {
                            let mut candidate = path;
                            candidate.cost = cost;
                            candidate.digits[block] =
                                u8::try_from(d).expect("digit index is below ten");
                            if cost < next[index][0].cost {
                                next[index][1] = next[index][0];
                                next[index][0] = candidate;
                            } else {
                                next[index][1] = candidate;
                            }
                        }
                    }
                }
            }
            states = next;
        }
        for path in states.iter().flatten() {
            if path.cost < second {
                if path.cost < best.0 {
                    second = best.0;
                    let mut digits = [0; 13];
                    digits[0] = u8::try_from(first).expect("digit index is below ten");
                    digits[1..].copy_from_slice(&path.digits);
                    best = (path.cost, digits);
                } else {
                    second = path.cost;
                }
            }
        }
    }
    (second - best.0 > 0.02 * diagonal && ean::checksum(&best.1)).then_some(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strongest_visual_invalid_checksum_is_not_repaired() {
        let mut gram = vec![0.; N * N];
        for i in 0..N {
            gram[i * N + i] = 1.;
        }
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        assert!(ean::checksum(&good));
        let rhs = ean::encode(&good).map(f64::from);
        assert_eq!(decode(&gram, &rhs, 0., 1., 1., 1), Some(good));
        let mut bad = good;
        bad[12] = 8;
        assert_eq!(
            decode(&gram, &ean::encode(&bad).map(f64::from), 0., 1., 1., 1),
            None
        );
    }
    #[test]
    fn coupled_measurements_recover_a_unique_visual_sequence() {
        let good = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        assert!(ean::checksum(&good));
        let bits = ean::encode(&good).map(f64::from);
        let mut gram = vec![0.; N * N];
        for i in 0..N {
            gram[i * N + i] = 1.;
            for j in i + 1..(i + 10).min(N) {
                let v = 0.04 / crate::numeric::usize_f64(j - i);
                gram[i * N + j] = v;
                gram[j * N + i] = v;
            }
        }
        let rhs = std::array::from_fn(|i| (0..N).map(|j| gram[i * N + j] * bits[j]).sum());
        assert_eq!(decode(&gram, &rhs, 0., 1., 1., 10), Some(good));
    }
}
