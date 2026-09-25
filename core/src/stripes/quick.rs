//! Sparse original-pixel orientation hypotheses for private speed-tier research.
//! This is localization only: source profiles still validate every payload.
use super::{bounds, groups, raster::Raster, Error, ImageView, Proposal, Result, Tile};
use crate::numeric::{f64_usize, usize_f64};

fn gray(im: ImageView<'_>, x: usize, y: usize) -> f64 {
    let at = y * im.stride + x * im.channels;
    if im.channels == 1 {
        f64::from(im.data[at])
    } else {
        f64::from(
            77 * u32::from(im.data[at])
                + 150 * u32::from(im.data[at + 1])
                + 29 * u32::from(im.data[at + 2]),
        ) / 256.
    }
}

fn prepare(
    raster: &mut Raster,
    im: ImageView<'_>,
    dimension: f64,
) -> std::result::Result<(), Error> {
    if im.width < 3 || im.height < 3 {
        return Err(Error::Dimensions);
    }
    raster.scale = (dimension / usize_f64(im.width.max(im.height))).min(1.);
    raster.width = f64_usize((usize_f64(im.width) * raster.scale).round()).max(3);
    raster.height = f64_usize((usize_f64(im.height) * raster.scale).round()).max(3);
    let tw = raster.width.div_ceil(8);
    let th = raster.height.div_ceil(8);
    raster.tiles.resize(tw * th, Tile::default());
    raster.tiles.fill(Tile::default());
    let columns: Vec<[usize; 4]> = (0..tw)
        .map(|tx| {
            [1, 3, 5, 7].map(|dx| {
                let wx = (tx * 8 + dx).min(raster.width - 1);
                f64_usize((usize_f64(wx) + 0.5) * usize_f64(im.width) / usize_f64(raster.width))
                    .clamp(1, im.width - 2)
            })
        })
        .collect();
    let rows: Vec<[usize; 4]> = (0..th)
        .map(|ty| {
            [2, 6, 1, 5].map(|dy| {
                let wy = (ty * 8 + dy).min(raster.height - 1);
                f64_usize((usize_f64(wy) + 0.5) * usize_f64(im.height) / usize_f64(raster.height))
                    .clamp(1, im.height - 2)
            })
        })
        .collect();
    // Four staggered source neighborhoods per working tile. Source-scale
    // gradients avoid erasing top_left small barcode through coarse nearest resizing.
    for (ty, row) in rows.iter().enumerate() {
        for (tx, column) in columns.iter().enumerate() {
            let tile = &mut raster.tiles[ty * tw + tx];
            for (&x, &y) in column.iter().zip(row) {
                let top_left = gray(im, x - 1, y - 1);
                let top = gray(im, x, y - 1);
                let top_right = gray(im, x + 1, y - 1);
                let left = gray(im, x - 1, y);
                let right = gray(im, x + 1, y);
                let bottom_left = gray(im, x - 1, y + 1);
                let bottom = gray(im, x, y + 1);
                let bottom_right = gray(im, x + 1, y + 1);
                let gx = (3. * (top_right - top_left)
                    + 10. * (right - left)
                    + 3. * (bottom_right - bottom_left))
                    / 16.;
                let gy = (3. * (bottom_left - top_left)
                    + 10. * (bottom - top)
                    + 3. * (bottom_right - top_right))
                    / 16.;
                tile.xx += gx * gx * 16.;
                tile.xy += gx * gy * 16.;
                tile.yy += gy * gy * 16.;
            }
            tile.classify(0.65);
        }
    }
    Ok(())
}

pub(super) fn detect(
    raster: &mut Raster,
    im: ImageView<'_>,
    dimension: f64,
) -> std::result::Result<Result, Error> {
    prepare(raster, im, dimension)?;
    let tw = raster.width.div_ceil(8);
    let groups = groups::collect_compact(raster);
    let mut proposals = Vec::new();
    for queue in groups.items.iter().take(groups.base_count.min(64)) {
        let axis = super::angle(queue, &raster.tiles);
        let b = bounds(queue, axis, tw);
        if b.u1 - b.u0 < 16. || b.v1 - b.v0 < 6. || (b.u1 - b.u0) / (b.v1 - b.v0) > 40. {
            continue;
        }
        let (c, s) = (axis.cos(), axis.sin());
        let point = |u: f64, v: f64| {
            [
                (u * c - v * s) * usize_f64(im.width) / usize_f64(raster.width),
                (u * s + v * c) * usize_f64(im.height) / usize_f64(raster.height),
            ]
        };
        proposals.push(Proposal {
            polygon: [
                point(b.u0, b.v0),
                point(b.u1, b.v0),
                point(b.u1, b.v1),
                point(b.u0, b.v1),
            ],
            score: 0.75,
        });
    }
    let omitted = proposals.len().saturating_sub(24);
    proposals.truncate(24);
    Ok(Result {
        proposals,
        omitted,
        limited: true,
        trace: [0; 13],
    })
}
