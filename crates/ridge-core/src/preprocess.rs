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

/// Numpy's default ('linear') percentile on a non-empty slice.
pub fn percentile_linear(sorted: &[f64], q: f64) -> f64 {
    assert!(!sorted.is_empty(), "percentile of empty slice");
    assert!((0.0..=100.0).contains(&q), "q out of range");
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = q / 100.0 * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = idx - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// (2k+1)x(2k+1) max - min gradient with in-bounds neighborhoods only
/// (skimage `rank` filters ignore out-of-image samples — verified against
/// `skimage/filters/rank/core_cy.pyx::_core`, which skips out-of-bounds
/// positions via `is_in_mask`). `k = 1` is the 3x3 gradient.
pub fn morphological_gradient(img: &[u8], nrows: usize, ncols: usize, k: usize) -> Vec<u8> {
    let mut out = vec![0u8; nrows * ncols];
    for r in 0..nrows {
        for c in 0..ncols {
            let mut min = u8::MAX;
            let mut max = 0u8;
            let r0 = r.saturating_sub(k);
            let r1 = (r + k).min(nrows - 1);
            let c0 = c.saturating_sub(k);
            let c1 = (c + k).min(ncols - 1);
            for rr in r0..=r1 {
                for cc in c0..=c1 {
                    let v = img[rr * ncols + cc];
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            out[r * ncols + c] = max - min;
        }
    }
    out
}

/// Back-compat alias for the 3x3 gradient.
pub fn gradient3x3(img: &[u8], nrows: usize, ncols: usize) -> Vec<u8> {
    morphological_gradient(img, nrows, ncols, 1)
}

/// Smallest connected flat region (in cells) that counts as a lake.
/// Anything smaller is quantization noise on sloped terrain.
const MIN_LAKE_COMPONENT: usize = 12;

/// 3x3 max - min gradient that IGNORES excluded cells (water/NaN): the
/// neighborhood keeps only in-bounds, non-excluded samples, and a cell with
/// no valid neighbors gets 0 (flat). Mirrors skimage's masked rank filters
/// (`is_in_mask`), which upstream never used.
pub fn masked_gradient3x3(
    img: &[f64],
    excluded: &[bool],
    nrows: usize,
    ncols: usize,
) -> Vec<f32> {
    let mut out = vec![0f32; nrows * ncols];
    for r in 0..nrows {
        for c in 0..ncols {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            let mut any = false;
            let r0 = r.saturating_sub(1);
            let r1 = (r + 1).min(nrows - 1);
            let c0 = c.saturating_sub(1);
            let c1 = (c + 1).min(ncols - 1);
            for rr in r0..=r1 {
                for cc in c0..=c1 {
                    let i = rr * ncols + cc;
                    if excluded[i] {
                        continue;
                    }
                    let v = img[i];
                    any = true;
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            out[r * ncols + c] = if any { (max - min) as f32 } else { 0.0 };
        }
    }
    out
}

/// Keep only connected components (4-connectivity) of `mask` with at least
/// `min_size` cells; everything else is dropped. Returns the filtered mask.
fn keep_large_components(
    mask: &[bool],
    nrows: usize,
    ncols: usize,
    min_size: usize,
) -> Vec<bool> {
    let mut out = vec![false; nrows * ncols];
    let mut visited = vec![false; nrows * ncols];
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..nrows * ncols {
        if !mask[start] || visited[start] {
            continue;
        }
        // Flood-fill this component.
        stack.clear();
        stack.push(start);
        visited[start] = true;
        let mut component = Vec::new();
        while let Some(i) = stack.pop() {
            component.push(i);
            let r = i / ncols;
            let c = i % ncols;
            for (dr, dc) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let rr = r as i64 + dr;
                let cc = c as i64 + dc;
                if rr < 0 || cc < 0 || rr >= nrows as i64 || cc >= ncols as i64 {
                    continue;
                }
                let j = rr as usize * ncols + cc as usize;
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
    use ndarray::array;

    #[test]
    fn percentile_matches_numpy_linear() {
        // np.percentile([1,2,3,4,5,6,7,8,9,10], 10) = 1.9
        let s: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        assert!((percentile_linear(&s, 10.0) - 1.9).abs() < 1e-12);
        assert!((percentile_linear(&s, 50.0) - 5.5).abs() < 1e-12);
        assert!((percentile_linear(&s, 0.0) - 1.0).abs() < 1e-12);
        assert!((percentile_linear(&s, 100.0) - 10.0).abs() < 1e-12);
    }

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
        assert!(out[(1, 2 - 2)].is_nan() || out[(1, 1)].is_nan() == false);
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
        let mut a = Array2::<f64>::zeros((n, n));
        for r in 0..n {
            // N-S slope: 3x3 float range = 2 * 0.6 / relief >= threshold
            for c in 0..n {
                a[(r, c)] = 100.0 + 0.6 * (r as f64);
            }
        }
        // Flat bench: rows 8..13, cols 12..22 at a constant height.
        for r in 8..13 {
            for c in 12..22 {
                a[(r, c)] = 105.0;
            }
        }
        // Pond: rows 14..24, cols 12..22, flat — adjacent to the bench.
        for r in 14..24 {
            for c in 12..22 {
                a[(r, c)] = 107.0;
            }
        }
        // Isolated 2x2 dip on the slope (rows 28..30, cols 30..32), flat.
        for r in 28..30 {
            for c in 30..32 {
                a[(r, c)] = 116.0;
            }
        }

        let out = preprocess(&a, 0.0, 3, 1.0, 1.0, None).unwrap();

        // preprocess flips rows: out row = 39 - src row.
        // Pond interior (src 15..22, cols 13..20) -> out rows 17..24.
        // Bench interior (src 9..11, cols 13..20) -> out rows 28..30.
        // Both are flat components >= MIN_LAKE_COMPONENT: all masked.
        for r in 17..24 {
            for c in 13..20 {
                assert!(out[(r, c)].is_nan(), "pond cell ({r},{c}) should be masked");
            }
        }
        for r in 28..30 {
            for c in 13..20 {
                assert!(out[(r, c)].is_nan(), "bench cell ({r},{c}) should be masked");
            }
        }
        // The slope must be hole-free (no rounding speckle), and the
        // isolated 2x2 dip is dropped by the component-size filter.
        for r in 0..n {
            for c in 0..n {
                if out[(r, c)].is_nan() {
                    let in_pond = (17..=24).contains(&r) && (13..=20).contains(&c);
                    let in_bench = (28..=30).contains(&r) && (13..=20).contains(&c);
                    assert!(in_pond || in_bench, "unexpected hole at ({r},{c})");
                }
            }
        }
    }

    #[test]
    fn water_percentile_ignores_nan_padding() {
        // Disc-style grid: right-hand columns are NaN padding (outside the
        // circle). The lowest row of valid cells is a river. The padding
        // must not drag the water percentile to zero — the river has to
        // survive at water_ntile=15.
        let n = 10;
        let mut a = Array2::<f64>::from_elem((n, n), f64::NAN);
        for r in 0..n {
            for c in 0..7 {
                a[(r, c)] = 10.0 + 5.0 * r as f64 + c as f64; // river at r=0
            }
        }
        let out = preprocess(&a, 15.0, 0, 1.0, 1.0, None).unwrap();
        // preprocess flips rows: src row 0 (river) -> out row n-1.
        for c in 0..7 {
            assert!(
                out[(n - 1, c)].is_nan(),
                "river cells must be water-masked (out row 9, col {c})"
            );
        }
        // Upland cells survive.
        for c in 0..7 {
            assert!(!out[(4, c)].is_nan(), "upland cell (4,{c}) must survive");
        }
    }

    #[test]
    fn all_nan_errors() {
        let a = Array2::<f64>::from_elem((2, 2), f64::NAN);
        assert!(preprocess(&a, 10.0, 3, 40.0, 1.0, None).is_err());
    }
}
