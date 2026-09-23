//! Dump a plane-fit pipeline fixture for the JS-parity check
//! (scripts/parity_frontend.mjs). Uses the New Hampshire test data.

use ndarray::Array2;
use ridge_core::srtm::DirSource;

fn main() {
    let srtm_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/srtm");
    let src = DirSource::new(srtm_dir.canonicalize().expect("fixtures/srtm not found"));
    let bbox = ridge_core::DEFAULT_BBOX;
    let (n, p) = (24usize, 30usize);
    // New order (flicker-free): ALL decisions (water percentile, lake
    // flatness) run on the unrotated grid; the masked/scaled/flipped result
    // is then rotated for display.
    let values = ridge_core::grid::sample(&src, &bbox, n, p);
    let processed = ridge_core::preprocess::preprocess(&values, 10.0, 3, 40.0, 1.0, None).unwrap();
    let rotated = ridge_core::rotate::rotate_fixed_plane(&processed, -33.0, 0);

    // Scene via RidgeScene::from_grid for layout parity too.
    let scene = ridge_core::RidgeScene::from_grid(
        &rotated,
        24.0 / 30.0,
        20.0,
        ridge_core::geometry::Frame::Window,
    );

    let to_val = |g: &Array2<f64>| {
        g.rows()
            .into_iter()
            .map(|r| {
                r.iter()
                    .map(|v| if v.is_finite() { Some(*v) } else { None })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let doc = serde_json::json!({
        "input": to_val(&values),
        "expected_rows": scene.rows.iter().map(|r| serde_json::json!({
            "baseline": r.baseline,
            "y": r.y.iter().map(|v| if v.is_finite() { Some(*v) } else { None }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "vmin": scene.vmin,
        "vmax": scene.vmax,
        "layout": serde_json::to_value(&scene.layout).unwrap(),
    });
    std::fs::write("/tmp/plane_fixture.json", doc.to_string()).unwrap();
    println!(
        "fixture written: {} rows x {} cols",
        processed.nrows(),
        processed.ncols()
    );
}
