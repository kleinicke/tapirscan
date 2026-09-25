//! Ordered tile components, compatible merges and bounded growth.
use super::raster::Raster;
use super::{angle, bounds, distance, Tile};
pub(super) struct Groups {
    pub items: Vec<Vec<usize>>,
    pub originals: usize,
    pub eligible: usize,
    pub merged_count: usize,
    pub merge_checks: usize,
    pub base_count: usize,
    pub growth_limited: bool,
}
pub(super) fn collect(raster: &Raster) -> Groups {
    collect_with_growth(raster, true)
}

/// Compact policies consume only original and merged components.
pub(super) fn collect_compact(raster: &Raster) -> Groups {
    collect_with_growth(raster, false)
}

fn collect_with_growth(raster: &Raster, include_growth: bool) -> Groups {
    let (tw, th) = (raster.width.div_ceil(8), raster.height.div_ceil(8));
    let tiles = &raster.tiles;
    let mut groups = components(tiles, tw, th);
    let originals = groups.len();
    let eligible = originals.min(64);
    let (merged, merge_checks) = merge(&groups, tiles, tw);
    let merged_count = merged.len();
    groups.extend(merged);
    let base_count = groups.len();
    let growth_limited = include_growth && grow(&mut groups, tiles, tw, th, originals);
    Groups {
        items: groups,
        originals,
        eligible,
        merged_count,
        merge_checks,
        base_count,
        growth_limited,
    }
}
fn components(tiles: &[Tile], tw: usize, th: usize) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut seen = vec![false; tiles.len()];
    for seed in 0..tiles.len() {
        if seen[seed] || !tiles[seed].active {
            continue;
        }

        let mut quad = vec![seed];

        seen[seed] = true;
        let mut head = 0;
        while head < quad.len() {
            let k = quad[head];
            head += 1;
            let (tx, ty) = ((k % tw).cast_signed(), (k / tw).cast_signed());
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (nx, ny) = (tx + ox, ty + oy);
                    if nx < 0 || ny < 0 || nx >= (tw).cast_signed() || ny >= (th).cast_signed() {
                        continue;
                    }
                    let neighbor_index = (ny).cast_unsigned() * tw + (nx).cast_unsigned();
                    if !seen[neighbor_index]
                        && tiles[neighbor_index].active
                        && distance(tiles[k].angle, tiles[neighbor_index].angle)
                            < std::f64::consts::PI / 9.
                    {
                        seen[neighbor_index] = true;
                        quad.push(neighbor_index);
                    }
                }
            }
        }

        if quad.len()
            >= if option_env!("TAPIRSCAN_TURBO_SMALL_COMPONENTS").is_some() {
                2
            } else {
                3
            }
        {
            groups.push(quad);
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    groups
}
fn merge(groups: &[Vec<usize>], tiles: &[Tile], tw: usize) -> (Vec<Vec<usize>>, usize) {
    let eligible = groups.len().min(64);
    let angles: Vec<_> = groups
        .iter()
        .take(eligible)
        .map(|g| angle(g, tiles))
        .collect();
    let mut used = vec![false; eligible];
    let mut merged = Vec::new();
    let mut merge_checks = 0;
    for seed in 0..eligible {
        if used[seed] {
            continue;
        }
        let mut members = vec![seed];
        used[seed] = true;
        let mut cache = vec![None; eligible];
        let axis = angles[seed];
        let mut changed = true;
        while changed && members.len() < 8 {
            changed = false;
            for i in 0..eligible {
                if used[i] {
                    continue;
                }

                let neighbor_bounds = *cache[i].get_or_insert_with(|| bounds(&groups[i], axis, tw));

                let (mut close, mut compatible) = (false, true);
                for &j in &members {
                    merge_checks += 1;
                    let a = *cache[j].get_or_insert_with(|| bounds(&groups[j], axis, tw));

                    let (ha, hb) = (a.v1 - a.v0, neighbor_bounds.v1 - neighbor_bounds.v0);

                    let overlap = a.v1.min(neighbor_bounds.v1) - a.v0.max(neighbor_bounds.v0);

                    let gap = (a.u0 - neighbor_bounds.u1)
                        .max(neighbor_bounds.u0 - a.u1)
                        .max(0.);

                    if distance(angles[i], angles[j]) > std::f64::consts::PI / 18.
                        || ha.min(hb) / ha.max(hb) < 0.65
                        || overlap < 0.8 * ha.min(hb)
                    {
                        compatible = false;
                        break;
                    }
                    close |= gap <= 16.;
                }
                if compatible && close {
                    members.push(i);
                    used[i] = true;
                    changed = true;
                    if members.len() == 8 {
                        break;
                    }
                }
            }
        }
        if members.len() > 1 {
            merged.push(
                members
                    .iter()
                    .flat_map(|&i| groups[i].iter().copied())
                    .collect::<Vec<_>>(),
            );
        }
    }
    (merged, merge_checks)
}
fn grow(
    groups: &mut Vec<Vec<usize>>,
    tiles: &[Tile],
    tw: usize,
    th: usize,
    originals: usize,
) -> bool {
    let mut growth_limited = false;
    let mut grown = Vec::new();
    for seed in groups.iter().take(originals.min(64)) {
        if !(3..=24).contains(&seed.len()) {
            continue;
        }
        let axis = angle(seed, tiles);

        let neighbor_bounds = bounds(seed, axis, tw);

        let (cos_angle, sin_angle) = (axis.cos(), axis.sin());
        let mut ids = seed.clone();
        let mut frontier = seed.clone();
        // Fixed seed direction and bar-height envelope prevent indirect
        // angle/height bridges. Three tile rings, at most64 tiles per seed.
        for _ in 0..3 {
            let mut next = Vec::new();
            for &k in &frontier {
                let (tx, ty) = ((k % tw).cast_signed(), (k / tw).cast_signed());
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (x, y) = (tx + dx, ty + dy);
                        if x < 0 || y < 0 || x >= (tw).cast_signed() || y >= (th).cast_signed() {
                            continue;
                        }
                        let neighbor_index = (y).cast_unsigned() * tw + (x).cast_unsigned();

                        if ids.contains(&neighbor_index) || ids.len() >= 64 {
                            continue;
                        }

                        let neighbor_tile = &tiles[neighbor_index];

                        if !neighbor_tile.can_grow()
                            || distance(neighbor_tile.angle, axis) > std::f64::consts::PI / 9.
                        {
                            continue;
                        }
                        let (px, py) = (
                            crate::numeric::isize_f64(x) * 8. + 4.,
                            crate::numeric::isize_f64(y) * 8. + 4.,
                        );
                        let (u, v) = (
                            px * cos_angle + py * sin_angle,
                            -px * sin_angle + py * cos_angle,
                        );
                        if u < neighbor_bounds.u0 - 24.
                            || u > neighbor_bounds.u1 + 24.
                            || v < neighbor_bounds.v0
                            || v > neighbor_bounds.v1
                        {
                            continue;
                        }
                        ids.push(neighbor_index);

                        next.push(neighbor_index);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        growth_limited |= ids.len() >= 64;
        if ids.len() > seed.len() {
            grown.push(ids);
        }
    }
    groups.extend(grown);
    growth_limited
}
