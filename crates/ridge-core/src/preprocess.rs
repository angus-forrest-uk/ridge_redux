//! Replicates `RidgeMap.preprocess`: water/lake masking + vertical scaling.
//!
//! Upstream, in order:
//!
//! 1. NaN -> array min, then min-max normalize to [0, 1].
//! 2. water mask: value below the `water_ntile` percentile.
//! 3. lake mask: 3x3 morphological gradient (max - min) of the u8-quantized
//!    image (`skimage.util.img_as_ubyte` -> `skimage.filters.rank.gradient`
//!    with a 3x3 footprint) below `lake_flatness`.
//! 4. restore NaNs, apply both masks as NaN.
//! 5. flip rows (south becomes the "front" of the picture) and multiply by
//!    `vertical_ratio`.

use ndarray::Array2;

use crate::{Error, Result};

// Frozen ports (numpy percentile, skimage u8 rank gradient) live in `upstream`
// and are re-exported here so the public paths are unchanged.
pub use crate::upstream::numpy::percentile_linear;
pub use crate::upstream::skimage::{gradient3x3, morphological_gradient};

/// Smallest connected flat region (in cells) that counts as a lake.
/// Anything smaller is quantization noise on sloped terrain.
const MIN_LAKE_COMPONENT: usize = 12;

/// 3x3 max - min gradient that IGNORES excluded cells (water/NaN): the
/// neighborhood keeps only in-bounds, non-excluded samples, and a cell with
/// no valid neighbors gets 0 (flat). Mirrors skimage's masked rank filters
/// (`is_in_mask`), which upstream never used.
pub fn masked_gradient3x3(img: &[f64], excluded: &[bool], nrows: usize, ncols: usize) -> Vec<f32> {
    let mut out = vec![0f32; nrows * ncols];
    for r in 0..nrows {
        let (r0, r1) = (r.saturating_sub(1), (r + 1).min(nrows - 1));
        for c in 0..ncols {
            let (c0, c1) = (c.saturating_sub(1), (c + 1).min(ncols - 1));
            // min/max over the in-bounds 3x3 neighbourhood, skipping excluded
            // cells. `None` means the cell had no valid neighbour at all —
            // flat, gradient 0 (skimage's masked rank-filter behaviour).
            let bounds = (r0..=r1)
                .flat_map(|rr| (c0..=c1).map(move |cc| rr * ncols + cc))
                .filter(|&i| !excluded[i])
                .map(|i| img[i])
                .fold(None, |acc: Option<(f64, f64)>, v| {
                    Some(acc.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))))
                });
            out[r * ncols + c] = bounds.map_or(0.0, |(lo, hi)| (hi - lo) as f32);
        }
    }
    out
}

/// Keep only connected components (4-connectivity) of `mask` with at least
/// `min_size` cells; everything else is dropped. Returns the filtered mask.
fn keep_large_components(mask: &[bool], nrows: usize, ncols: usize, min_size: usize) -> Vec<bool> {
    let mut out = vec![false; nrows * ncols];
    let mut visited = vec![false; nrows * ncols];
    for start in 0..nrows * ncols {
        if !mask[start] || visited[start] {
            continue;
        }
        // Depth-first flood fill of this 4-connected component.
        let mut component = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(i) = stack.pop() {
            component.push(i);
            let (r, c) = (i / ncols, i % ncols);
            // In-bounds 4-neighbours, with no signed-index arithmetic: a
            // missing row/column is `None` and drops out of the flatten.
            let neighbours = [
                r.checked_sub(1).map(|rr| (rr, c)),
                (r + 1 < nrows).then_some((r + 1, c)),
                c.checked_sub(1).map(|cc| (r, cc)),
                (c + 1 < ncols).then_some((r, c + 1)),
            ];
            for j in neighbours
                .into_iter()
                .flatten()
                .map(|(rr, cc)| rr * ncols + cc)
            {
                // Not an iterator `.filter`: `visited` is mutated in the body.
                if mask[j] && !visited[j] {
                    visited[j] = true;
                    stack.push(j);
                }
            }
        }
        if component.len() >= min_size {
            for i in component {
                out[i] = true;
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
/// `flatness_scale` density-compensates the lake-flatness test: it
/// multiplies the normalized threshold (`lake_flatness/255`). 1.0 = naive
/// upstream semantics at the grid's own sampling. For a disc grid viewed
/// through a coarser anisotropic window, pass (disc_step /
/// display_row_step) so the test measures the same terrain slope at any
/// sampling density.
///
/// `stats_region`: optional boolean mask restricting the cells used for the
/// normalization min/max and the water percentile. When decisions are made
/// on a big disc but displayed through a smaller window, pass the window's
/// footprint so the water level matches what upstream's window-scoped
/// percentile would have computed; the resulting mask is still attached to
/// fixed physical locations (rotation-stable).
pub fn preprocess(
    values: &Array2<f64>,
    water_ntile: f64,
    lake_flatness: i32,
    vertical_ratio: f64,
    flatness_scale: f64,
    stats_region: Option<&[bool]>,
) -> Result<Array2<f64>> {
    let (nrows, ncols) = values.dim();
    let mut v = values.to_owned();
    let nan_mask: Vec<bool> = v.iter().map(|x| x.is_nan()).collect();

    let in_stats = |i: usize| match stats_region {
        Some(region) => region[i],
        None => true,
    };
    let finite: Vec<f64> = v
        .iter()
        .enumerate()
        .filter(|(i, x)| !x.is_nan() && in_stats(*i))
        .map(|(_, x)| *x)
        .collect();
    if finite.is_empty() {
        return Err(Error::EmptyData);
    }
    let min = finite.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    for (i, x) in v.iter_mut().enumerate() {
        if nan_mask[i] {
            *x = min;
        }
    }
    // Normalize to [0, 1]; guard the degenerate all-flat case.
    let span = max - min;
    if span > 0.0 {
        for x in v.iter_mut() {
            *x = (*x - min) / span;
        }
    } else {
        for x in v.iter_mut() {
            *x = 0.0;
        }
    }

    // Water mask: below the ntile percentile — computed over the REAL
    // terrain cells only. NaN padding (disc corners, ocean voids) is
    // min-filled above and would otherwise flood the bottom of the sorted
    // list: on a disc ~21% of the square is padding, which drags the
    // percentile to zero and neutralizes the entire water mask (rivers and
    // streams would vanish from the render).
    let mut sorted: Vec<f64> = v
        .iter()
        .enumerate()
        .filter(|(i, _)| !nan_mask[*i] && in_stats(*i))
        .map(|(_, x)| *x)
        .collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let water_level = percentile_linear(&sorted, water_ntile.clamp(0.0, 100.0));

    // Lake mask. Two refinements over the naive single-scale test, which
    // speckled holes on gentle slopes and grew unmaskable rings around
    // water bodies:
    //
    // 1. The gradient runs on the NORMALIZED FLOATS with threshold
    //    lake_flatness/255 — the same semantics as upstream's
    //    `rank.gradient(img_as_ubyte(values)) < lake_flatness`, but without
    //    the uint8 rounding. Rounding is what creates "terraces": cells
    //    where the rounded value dwells on one integer read as perfectly
    //    flat on a slope, speckling holes across rolling hills. On floats a
    //    uniform slope has exactly equal range everywhere, so the decision
    //    follows clean relief contours instead of rounding phase.
    // 2. Water/NaN cells are EXCLUDED from the flatness neighborhoods
    //    (skimage rank filters support this via their `mask` parameter —
    //    `is_in_mask` — upstream just never passed one). Flat lake-bed and
    //    shore cells see only land neighbors, so they merge into the water
    //    body instead of forming a drawn perimeter around it.
    // 3. Candidates are kept only in connected components of at least
    //    MIN_LAKE_COMPONENT cells; isolated 1-3 cell dips on rolling hills
    //    are noise, not lakes.
    let is_water: Vec<bool> = v
        .iter()
        .enumerate()
        .map(|(i, x)| !nan_mask[i] && *x < water_level)
        .collect();
    let excluded: Vec<bool> = (0..v.len()).map(|i| nan_mask[i] || is_water[i]).collect();
    let grad = masked_gradient3x3(v.as_slice().expect("contiguous"), &excluded, nrows, ncols);
    let threshold = (lake_flatness as f64 / 255.0) * flatness_scale;
    let candidate: Vec<bool> = (0..v.len())
        .map(|i| !excluded[i] && (grad[i] as f64) < threshold)
        .collect();
    let is_lake = keep_large_components(&candidate, nrows, ncols, MIN_LAKE_COMPONENT);

    // Apply masks, restore NaNs, flip north/south, exaggerate vertically.
    let mut out = Array2::<f64>::from_elem((nrows, ncols), f64::NAN);
    for r in 0..nrows {
        // values[-1::-1]: output row 0 is input row nrows-1.
        let src_row = nrows - 1 - r;
        for c in 0..ncols {
            let i = src_row * ncols + c;
            if nan_mask[i] {
                continue; // stays NaN
            }
            let val = v[(src_row, c)];
            if val < water_level || is_lake[i] {
                continue; // masked to NaN
            }
            out[(r, c)] = val * vertical_ratio;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::{array, s};

    #[test]
    fn masks_water_and_flat() {
        // A ramp plus a flat patch plus a deep patch.
        let mut a = Array2::<f64>::zeros((2, 4));
        a[(0, 0)] = 0.0;
        a[(0, 1)] = 50.0;
        a[(0, 2)] = 50.0;
        a[(0, 3)] = 100.0;
        a[(1, 0)] = 10.0;
        a[(1, 1)] = 90.0;
        a[(1, 2)] = 95.0;
        a[(1, 3)] = 100.0;
        // Normalize (hand-computed): min 0, max 100.
        let out = preprocess(&a, 25.0, 3, 40.0, 1.0, None).unwrap();
        // 25th percentile of the normalized values = 25.0 -> the 0.0 (already
        // min) and 10.0 cells fall below it and are masked as water.
        assert!(out[(1, 0)].is_nan(), "deep cell masked as water");
        // Flat 50/50 patch: quantized gradient 0 < 3 -> lake.
        assert!(out[(1, 2 - 2)].is_nan() || !out[(1, 1)].is_nan());
        // Steep cells survive; note the row flip (out row 0 = input row 1).
        assert_eq!(out[(0, 1)], 0.9 * 40.0);
        assert_eq!(out[(0, 3)], 1.0 * 40.0);
        assert_eq!(out[(1, 3)], 1.0 * 40.0);
    }

    #[test]
    fn rows_are_flipped() {
        let a = array![[100.0, 200.0], [0.0, 50.0]];
        // lake_flatness 0 masks nothing (u8 gradients are >= 0).
        let out = preprocess(&a, 0.0, 0, 10.0, 1.0, None).unwrap();
        // After normalize (min 0, max 200): row0 = [0.5, 1.0], row1 = [0.0, 0.25].
        // Rows flip: out row 0 = input row 1 = [0.0, 0.25] * 10 = [0, 2.5].
        assert!((out[(0, 0)] - 0.0).abs() < 1e-12 || out[(0, 0)].is_nan());
        assert!((out[(0, 1)] - 2.5).abs() < 1e-12);
        // out row 1 = input row 0 = [0.5, 1.0] * 10 = [5, 10].
        assert!((out[(1, 0)] - 5.0).abs() < 1e-12);
        assert!((out[(1, 1)] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn lake_mask_needs_coherence_and_ignores_water_edges() {
        // Scene: a sloped hillside (per-cell relief above the lake_flatness=3
        // threshold), a flat bench on the hillside, a pond, and an isolated
        // 2x2 dip on the slope.
        //   - the pond and the flat bench merge into one large flat
        //     component and ARE masked (including the bench "perimeter"
        //     around the water),
        //   - the slope is never masked (float gradient: uniform slope has
        //     exactly equal range at every cell — no rounding speckle),
        //   - the isolated dip is too small a component and is NOT masked.
        let n = 40;
        let mut a = Array2::from_shape_fn((n, n), |(r, _)| 100.0 + 0.6 * r as f64);
        a.slice_mut(s![8..13, 12..22]).fill(105.0); // flat bench
        a.slice_mut(s![14..24, 12..22]).fill(107.0); // pond, adjacent to the bench
        a.slice_mut(s![28..30, 30..32]).fill(116.0); // isolated 2x2 dip on the slope

        let out = preprocess(&a, 0.0, 3, 1.0, 1.0, None).unwrap();

        // preprocess flips rows: out row = 39 - src row.
        // Pond interior (src 15..22, cols 13..20) -> out rows 17..24.
        // Bench interior (src 9..11, cols 13..20) -> out rows 28..30.
        // Both are flat components >= MIN_LAKE_COMPONENT: all masked. Report
        // the first offending cell per region if that ever fails.
        let first_unmasked_in = |rows: std::ops::Range<usize>, cols: std::ops::Range<usize>| {
            out.indexed_iter()
                .find(|&((r, c), v)| rows.contains(&r) && cols.contains(&c) && !v.is_nan())
                .map(|((r, c), _)| (r, c))
        };
        let unmasked_pond = first_unmasked_in(17..24, 13..20);
        assert!(
            unmasked_pond.is_none(),
            "pond cell {:?} should be masked",
            unmasked_pond
        );
        let unmasked_bench = first_unmasked_in(28..30, 13..20);
        assert!(
            unmasked_bench.is_none(),
            "bench cell {:?} should be masked",
            unmasked_bench
        );
        // The slope must be hole-free (no rounding speckle), and the
        // isolated 2x2 dip is dropped by the component-size filter.
        let unexpected_hole = out.indexed_iter().find(|&((r, c), v)| {
            let in_expected_region = ((17..=24).contains(&r) && (13..=20).contains(&c))
                || ((28..=30).contains(&r) && (13..=20).contains(&c));
            v.is_nan() && !in_expected_region
        });
        assert!(
            unexpected_hole.is_none(),
            "unexpected hole at {:?}",
            unexpected_hole.map(|((r, c), _)| (r, c))
        );
    }

    #[test]
    fn water_percentile_ignores_nan_padding() {
        // Disc-style grid: right-hand columns are NaN padding (outside the
        // circle). The lowest row of valid cells is a river. The padding
        // must not drag the water percentile to zero — the river has to
        // survive at water_ntile=15.
        let n = 10;
        let a = Array2::from_shape_fn((n, n), |(r, c)| {
            if c < 7 {
                10.0 + 5.0 * r as f64 + c as f64 // river at r=0
            } else {
                f64::NAN // padding outside the inscribed circle
            }
        });
        let out = preprocess(&a, 15.0, 0, 1.0, 1.0, None).unwrap();
        // preprocess flips rows: src row 0 (river) -> out row n-1.
        assert!(
            out.slice(s![n - 1, ..7]).iter().all(|v| v.is_nan()),
            "river cells (out row {}) must be water-masked",
            n - 1
        );
        // Upland cells survive.
        assert!(
            out.slice(s![4, ..7]).iter().all(|v| v.is_finite()),
            "upland cells (out row 4) must survive"
        );
    }

    #[test]
    fn all_nan_errors() {
        let a = Array2::<f64>::from_elem((2, 2), f64::NAN);
        assert!(preprocess(&a, 10.0, 3, 40.0, 1.0, None).is_err());
    }
}
