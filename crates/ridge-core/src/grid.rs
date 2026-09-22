//! Sample a bounding box into an elevation grid — the Rust twin of
//! `SRTM.py`'s `get_image(..., mode='array')` as used by ridge_map.

use ndarray::Array2;

use crate::srtm::TileSource;
use crate::Bbox;

/// Sample `num_lines x elevation_pts` elevations over `bbox`.
///
/// Mirrors the upstream sampling exactly (`/N`, not `/(N-1)`, so the last
/// row/column stops one step short of the far corner):
///
/// ```text
/// lat = lat0 + r / num_lines       * (lat1 - lat0)
/// lon = lon0 + c / elevation_pts   * (lon1 - lon0)
/// ```
///
/// Voids (open ocean, bad samples) are NaN.
pub fn sample(
    source: &dyn TileSource,
    bbox: &Bbox,
    num_lines: usize,
    elevation_pts: usize,
) -> Array2<f64> {
    let (lat0, lat1) = bbox.lats();
    let (lon0, lon1) = bbox.longs();
    Array2::from_shape_fn((num_lines, elevation_pts), |(r, c)| {
        let lat = lat0 + r as f64 / num_lines as f64 * (lat1 - lat0);
        let lon = lon0 + c as f64 / elevation_pts as f64 * (lon1 - lon0);
        // Missing tile (open ocean) or a void sample -> NaN.
        source
            .tile(lat.floor() as i32, lon.floor() as i32)
            .map_or(f64::NAN, |tile| tile.elevation(lat, lon))
    })
}

/// Sample a square, disc-masked region: `n x n` points over a square of
/// side `span_deg` centered at `(center_lat, center_lon)`, with samples
/// outside the inscribed circle (radius `span_deg / 2`) set to NaN.
///
/// The disc is rotation-invariant about the grid center: rotating the grid
/// by any angle leaves the set of finite samples unchanged (same count, same
/// extent), so an orbiting view keeps a constant amount of visible terrain —
/// the "camera distance" is genuinely fixed and no per-angle reframing is
/// needed. This is what the web frontend's disc mode uses.
pub fn sample_disc(
    source: &dyn TileSource,
    center_lat: f64,
    center_lon: f64,
    span_deg: f64,
    n: usize,
) -> Array2<f64> {
    let half = span_deg / 2.0;
    let radius_sq = half * half;
    Array2::from_shape_fn((n, n), |(r, c)| {
        let lat = center_lat - half + r as f64 / n as f64 * span_deg;
        let lon = center_lon - half + c as f64 / n as f64 * span_deg;
        let (dlat, dlon) = (lat - center_lat, lon - center_lon);
        if dlat * dlat + dlon * dlon > radius_sq {
            return f64::NAN; // outside the disc: never sampled
        }
        source
            .tile(lat.floor() as i32, lon.floor() as i32)
            .map_or(f64::NAN, |tile| tile.elevation(lat, lon))
    })
}

/// Sample an ANISOTROPIC display window from a preprocessed square data
/// grid. The window covers `lat0..lat1 x lon0..lon1` at
/// `num_lines x elevation_pts` samples (cells of
/// `(lat1-lat0)/num_lines x (lon1-lon0)/elevation_pts` degrees), reading the
/// data grid at nearest nodes; positions outside the data grid become NaN.
///
/// This is the rectangle view: the window keeps the upstream bbox extent,
/// line count and cell shape at every angle, while the underlying square
/// disc (rotation-invariant) supplies previously unused terrain as the
/// window sweeps around.
#[allow(clippy::too_many_arguments)]
pub fn sample_window(
    data: &Array2<f64>,
    d_lat0: f64,
    d_lon0: f64,
    d_span: f64,
    lat0: f64,
    lon0: f64,
    lat1: f64,
    lon1: f64,
    num_lines: usize,
    elevation_pts: usize,
) -> Array2<f64> {
    let step = d_span / data.nrows() as f64; // data grid is square
    Array2::from_shape_fn((num_lines, elevation_pts), |(i, j)| {
        let lat = lat0 + i as f64 / num_lines as f64 * (lat1 - lat0);
        let lon = lon0 + j as f64 / elevation_pts as f64 * (lon1 - lon0);
        let r = ((lat - d_lat0) / step).round();
        let c = ((lon - d_lon0) / step).round();
        // Reject anything that isn't a non-negative index, including the NaN
        // a degenerate `step` can produce (`NaN as usize` silently saturates
        // to 0); `get` then handles the far edge (out of bounds -> NaN).
        if !(r >= 0.0 && c >= 0.0) {
            return f64::NAN;
        }
        data.get((r as usize, c as usize))
            .copied()
            .unwrap_or(f64::NAN)
    })
}

/// Upstream `get_elevation_data` swaps the sampling resolution when the
/// viewpoint angle is within the (45..135 | 225..315) degree bands.
pub fn swap_for_angle(viewpoint_angle: f64) -> bool {
    let a = viewpoint_angle.rem_euclid(360.0);
    (45.0 < a && a < 135.0) || (225.0 < a && a < 315.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srtm::SyntheticSource;

    #[test]
    fn sample_shape_and_values() {
        let src = SyntheticSource { side: 1201 };
        let bbox = Bbox::new(-72.0, 43.0, -71.0, 44.0);
        let grid = sample(&src, &bbox, 4, 5);
        assert_eq!(grid.dim(), (4, 5));
        // First row: lat = 43.0 (bottom edge), matching upstream convention.
        let v00 = grid[(0, 0)];
        let expected = SyntheticSource::value(43.0, -72.0).round();
        assert!((v00 - expected).abs() < 1e-9);
        // Last sampled point stops short of the far corner: r/N, not r/(N-1).
        // Compare through the same nearest-neighbor tile lookup (floating
        // point can nudge r/N onto the adjacent grid cell, exactly like
        // upstream SRTM.py).
        let lat_last = 43.0 + 3.0 / 4.0 * 1.0;
        let lon_last = -72.0 + 4.0 / 5.0 * 1.0;
        let tile = src.tile(43, -72).unwrap();
        assert_eq!(grid[(3, 4)], tile.elevation(lat_last, lon_last));
        // An interior point that lands exactly on a grid node (1/4, 2/5 are
        // binary-exact fractions of the span).
        let lat = 43.0 + 1.0 / 4.0;
        let lon = -72.0 + 2.0 / 5.0;
        let expected = SyntheticSource::value(lat, lon).round();
        assert!((grid[(1, 2)] - expected).abs() < 1e-9);
    }

    /// Only serves one tile; everything else is open ocean.
    struct OneTileSource;
    impl crate::srtm::TileSource for OneTileSource {
        fn tile(&self, lat_lo: i32, lon_lo: i32) -> Option<std::sync::Arc<crate::srtm::Tile>> {
            if lat_lo == 43 && lon_lo == -72 {
                SyntheticSource { side: 1201 }.tile(lat_lo, lon_lo)
            } else {
                None
            }
        }
    }

    #[test]
    fn ocean_is_nan() {
        // Half the box falls into the missing neighbor tile -> NaN there.
        let src = OneTileSource;
        let bbox = Bbox::new(-71.5, 43.5, -70.5, 44.5);
        let grid = sample(&src, &bbox, 5, 5);
        assert!(grid.iter().any(|v| v.is_nan()), "missing tile -> NaN");
        assert!(grid.iter().any(|v| v.is_finite()), "present tile -> values");
    }

    #[test]
    fn disc_mask_is_rotation_invariant() {
        // The finite support of a disc-masked grid is identical at every
        // angle: same count, same extent. (This is the property that makes
        // the frontend's fixed-distance orbit work.)
        let src = SyntheticSource { side: 1201 };
        let grid = sample_disc(&src, 44.0, -72.0, 1.0, 40);
        let finite = grid.iter().filter(|v| v.is_finite()).count();
        // Disc covers pi/4 of the square (plus boundary-rounding slack).
        let expected = (std::f64::consts::FRAC_PI_4 * 1600.0) as usize;
        assert!(
            (finite as i64 - expected as i64).abs() < 40,
            "finite {finite} vs {expected}"
        );
        // Outside the disc: strictly NaN. `grid` is 40x40 centered at
        // (44, -72) with span 1.0, so the disc radius is 0.5.
        let outside_disc = |r: usize, c: usize| {
            let dlat = (44.0 - 0.5 + r as f64 / 40.0) - 44.0;
            let dlon = (-72.0 - 0.5 + c as f64 / 40.0) - -72.0;
            dlat * dlat + dlon * dlon > 0.25
        };
        assert!(
            grid.indexed_iter()
                .filter(|&((r, c), _)| outside_disc(r, c))
                .all(|(_, v)| v.is_nan()),
            "every cell outside the disc must be NaN"
        );
    }

    #[test]
    fn anisotropic_window_samples_correct_positions() {
        // Data grid: 8x8 over span 1.0 centered at (44, -72); value encodes
        // position so we can verify exactly which node each window cell read.
        let src = SyntheticSource { side: 8 }; // unused for this test
        let _ = src;
        let data = Array2::from_shape_fn((8, 8), |(r, c)| (r * 100 + c) as f64);
        // Data square: lat 43.5..44.5, lon -72.5..-71.5.
        let (d_lat0, d_lon0, d_span) = (43.5f64, -72.5f64, 1.0f64);
        // Window = the lower-left 4x8-degree... anisotropic: lat 43.5..44.3
        // (4 rows over 0.8), lon -72.5..-71.7 (8 cols over 0.8).
        let out = sample_window(
            &data, d_lat0, d_lon0, d_span, 43.5, -72.5, 44.3, -71.7, 4, 8,
        );
        assert_eq!(out.dim(), (4, 8));
        // Sample positions: lat_i = 43.5 + i/4*0.8 -> rows round((lat-43.5)/0.125)
        // i=0 -> row 0; i=3 -> row round(2.4)=2. lon_j = -72.5 + j/8*0.8 ->
        // cols j -> col j exactly.
        // Window col j -> data col round(j * 0.8 / 0.125) (window steps are
        // finer than data steps here: 0.1 vs 0.125 degrees).
        for (j, expected_col) in [
            (0usize, 0usize),
            (1, 1),
            (2, 2),
            (3, 2),
            (4, 3),
            (5, 4),
            (6, 5),
            (7, 6),
        ] {
            assert_eq!(out[(0, j)], (expected_col) as f64, "row 0 col {j}");
        }
        // Rows: window row i -> data row round(i * 0.2 / 0.125).
        assert_eq!(out[(2, 0)], 300.0); // round(3.2) = 3
        assert_eq!(out[(3, 0)], 500.0); // round(4.8) = 5
    }

    #[test]
    fn zero_span_window_is_all_gaps() {
        // A degenerate zero-degree span makes `step == 0`, so the coordinate
        // math yields NaN/inf. The window must come back entirely NaN rather
        // than silently reading cell (0, 0) (a NaN cast saturates to 0).
        let data = Array2::from_shape_fn((4, 4), |(r, c)| (r * 4 + c) as f64);
        let out = sample_window(&data, 43.0, -72.0, 0.0, 43.0, -72.0, 44.0, -71.0, 4, 4);
        assert!(out.iter().all(|v| v.is_nan()), "zero span must not sample");
    }

    #[test]
    fn angle_bands() {
        assert!(!swap_for_angle(0.0));
        assert!(!swap_for_angle(45.0));
        assert!(swap_for_angle(90.0));
        assert!(!swap_for_angle(135.0));
        assert!(swap_for_angle(-90.0)); // 270 rem_euclid
        assert!(swap_for_angle(280.0));
    }
}

#[cfg(test)]
mod parity_tests {
    //! Golden test against upstream: `ridge_map/test/test_data/new_hampshire.npz`
    //! is the exact array `RidgeMap().get_elevation_data()` returns. We compare
    //! our sampling of the same bbox against it. Requires the four SRTM tiles
    //! in `fixtures/srtm/` (run `scripts/fetch_fixtures.sh` once) — skipped
    //! otherwise.

    use super::*;
    use crate::srtm::DirSource;
    use std::path::Path;

    #[test]
    fn white_mountains_matches_upstream_fixture() {
        let bin = Path::new("../../fixtures/new_hampshire.f64.bin");
        let srtm_dir = Path::new("../../fixtures/srtm");
        if !bin.exists() || !srtm_dir.exists() {
            // This test is the evidence that sampling matches upstream, so CI
            // sets RIDGE_REQUIRE_FIXTURES and a missing fixture fails it.
            assert!(
                std::env::var_os("RIDGE_REQUIRE_FIXTURES").is_none(),
                "RIDGE_REQUIRE_FIXTURES is set but the fixtures are missing; run scripts/fetch_fixtures.sh"
            );
            eprintln!("skipping: fixtures not fetched (scripts/fetch_fixtures.sh)");
            return;
        }
        let bytes = std::fs::read(bin).unwrap();
        let expected: Vec<f64> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&b| f64::from_le_bytes(b))
            .collect();

        let src = DirSource::new(srtm_dir);
        let grid = sample(&src, &crate::DEFAULT_BBOX, 80, 300);
        assert_eq!(grid.len(), expected.len());

        // Compare every sample with upstream. A cell agrees when both sides
        // are gaps, or both are finite and within 1e-9; anything else counts
        // as a mismatch.
        let mut mismatches = 0usize;
        let mut worst = 0.0f64;
        for (&got, &want) in grid.iter().zip(&expected) {
            if got.is_nan() && want.is_nan() {
                continue; // both gaps: agree
            }
            let delta = (got - want).abs();
            if !got.is_nan() && !want.is_nan() && delta <= 1e-9 {
                continue; // both finite, within tolerance: agree
            }
            mismatches += 1;
            // A gap on one side gives a NaN delta, which has no magnitude to
            // track; only a finite disagreement can raise `worst`.
            if delta.is_finite() {
                worst = worst.max(delta);
            }
        }
        // Small mismatches can appear where nearest-neighbor rounding sits on
        // a floating-point knife edge; upstream has the same instability.
        let frac = mismatches as f64 / (expected.len() as f64);
        println!(
            "parity: {mismatches}/{} mismatches (worst delta {worst:.2e})",
            expected.len()
        );
        assert!(frac < 0.01, "{mismatches} mismatches (worst delta {worst})");
    }
}
