//! Fixture-driven parity tests for the frozen ports in `ridge_core::upstream`.
//!
//! Each case in `fixtures/parity/*.json` is an input plus the output produced
//! by the *actual* reference library (numpy / scipy.ndimage / skimage), emitted
//! by `scripts/gen_parity_fixtures.py`. These tests are the executable proof
//! that the ports are faithful; the folder designation alone is only a label.
//!
//! Fixtures are skipped when absent unless `RIDGE_REQUIRE_FIXTURES=1` is set
//! (CI), matching the sampling golden test in `grid::parity_tests`.

use std::path::{Path, PathBuf};

use ndarray::Array2;
use serde::Deserialize;

use ridge_core::upstream::numpy::percentile_linear;
use ridge_core::upstream::scipy_ndimage::rotate;
use ridge_core::upstream::skimage::morphological_gradient;

#[derive(Deserialize)]
struct Doc<T> {
    #[allow(dead_code)]
    reference: serde_json::Value,
    cases: Vec<T>,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/parity")
}

/// Load a fixture's cases, or `None` when it hasn't been generated. The
/// fixtures are the evidence, so CI (`RIDGE_REQUIRE_FIXTURES=1`) fails on a
/// missing file rather than silently passing.
fn load<T: serde::de::DeserializeOwned>(name: &str) -> Option<Vec<T>> {
    let path = fixtures_dir().join(format!("{name}.json"));
    if !path.exists() {
        assert!(
            std::env::var_os("RIDGE_REQUIRE_FIXTURES").is_none(),
            "RIDGE_REQUIRE_FIXTURES is set but {} is missing; run scripts/gen_parity_fixtures.py",
            path.display()
        );
        eprintln!("skipping {name} parity: {} not generated", path.display());
        return None;
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let doc: Doc<T> = serde_json::from_str(&text).unwrap();
    Some(doc.cases)
}

fn to_array(rows: &[Vec<Option<f64>>]) -> Array2<f64> {
    let ncols = rows.first().map_or(0, Vec::len);
    let mut a = Array2::from_elem((rows.len(), ncols), f64::NAN);
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            a[(r, c)] = v.unwrap_or(f64::NAN);
        }
    }
    a
}

// ---- numpy.percentile (method='linear') ----

#[derive(Deserialize)]
struct PercentileCase {
    sorted: Vec<f64>,
    q: f64,
    expected: f64,
}

#[test]
fn percentile_matches_numpy() {
    let Some(cases) = load::<PercentileCase>("numpy") else {
        return;
    };
    for (i, c) in cases.iter().enumerate() {
        let got = percentile_linear(&c.sorted, c.q);
        assert!(
            (got - c.expected).abs() <= 1e-9,
            "case {i}: percentile(q={}) got {got}, numpy says {}",
            c.q,
            c.expected
        );
    }
}

// ---- scipy.ndimage.rotate (mode='constant', cval=0.0) ----

#[derive(Deserialize)]
struct RotateCase {
    input: Vec<Vec<Option<f64>>>,
    angle: f64,
    reshape: bool,
    order: u32,
    expected: Vec<Vec<Option<f64>>>,
    /// Cells whose order-0 rounding is a floating-point knife edge (the
    /// coordinate sits on a half-integer). scipy's vectorized dot and our
    /// per-cell dot may then pick different neighbours; see the generator.
    tie: Vec<Vec<bool>>,
}

#[test]
fn rotate_matches_scipy() {
    let Some(cases) = load::<RotateCase>("scipy_rotate") else {
        return;
    };
    for (i, c) in cases.iter().enumerate() {
        let got = rotate(&to_array(&c.input), c.angle, c.reshape, c.order);
        let ctx = format!(
            "case {i} (angle {}, reshape {}, order {})",
            c.angle, c.reshape, c.order
        );

        let expected_rows = c.expected.len();
        let expected_cols = c.expected.first().map_or(0, Vec::len);
        assert_eq!(got.dim(), (expected_rows, expected_cols), "{ctx}: shape");

        // order 0 selects an input value verbatim, so it must be exact except
        // on an annotated rounding tie; order 1 is float interpolation.
        let tol = if c.order == 0 { 0.0 } else { 1e-9 };
        for (r, row) in c.expected.iter().enumerate() {
            for (col, want) in row.iter().enumerate() {
                let g = got[(r, col)];
                let ok = match (g.is_nan(), want) {
                    (true, None) => true,
                    (false, Some(w)) => (g - w).abs() <= tol || (c.order == 0 && c.tie[r][col]),
                    _ => false,
                };
                assert!(
                    ok,
                    "{ctx} at ({r},{col}): got {g}, scipy says {want:?} (tie={})",
                    c.tie[r][col]
                );
            }
        }
    }
}

// ---- skimage.filters.rank.gradient ----

#[derive(Deserialize)]
struct GradientCase {
    k: usize,
    input: Vec<Vec<u8>>,
    expected: Vec<Vec<u8>>,
}

#[test]
fn gradient_matches_skimage() {
    let Some(cases) = load::<GradientCase>("skimage") else {
        return;
    };
    for (i, c) in cases.iter().enumerate() {
        let nrows = c.input.len();
        let ncols = c.input.first().map_or(0, Vec::len);
        let img: Vec<u8> = c.input.iter().flatten().copied().collect();
        let got = morphological_gradient(&img, nrows, ncols, c.k);
        let want: Vec<u8> = c.expected.iter().flatten().copied().collect();

        assert_eq!(got.len(), want.len(), "case {i}: length");
        for (idx, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(
                g,
                w,
                "case {i} (k={}) at row {}, col {}: got {g}, skimage says {w}",
                c.k,
                idx / ncols,
                idx % ncols
            );
        }
    }
}
