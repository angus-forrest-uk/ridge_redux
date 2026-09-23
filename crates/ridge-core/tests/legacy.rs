//! Legacy-equivalence test for the figure frame.
//!
//! `fixtures/legacy/frame.json` is captured by `scripts/gen_legacy_fixture.py`
//! running the *actual* upstream `ridge_map` package over matplotlib, so the
//! reference is the legacy implementation rather than a snapshot of our own
//! output. The frame is the one place ridge-core deliberately differs:
//! matplotlib autoscales tightly around the cells it draws, which lets water
//! crop the picture. The fixture records both frames, and this test holds us
//! to the intended one and to the legacy figure size we do share.
//!
//! Skipped when absent unless `RIDGE_REQUIRE_FIXTURES=1` (CI).

use std::path::Path;

use ndarray::Array2;
use serde::Deserialize;

use ridge_core::geometry::{AXES_MARGIN, FIG_DPI, LINE_SPACING};
use ridge_core::preprocess::preprocess;
use ridge_core::RidgeScene;

#[derive(Deserialize)]
struct Fixture {
    bbox: [f64; 4],
    water_ntile: f64,
    lake_flatness: i32,
    vertical_ratio: f64,
    size_scale: f64,
    input: Vec<Vec<Option<f64>>>,
    expected: Expected,
    legacy_extent: LegacyExtent,
}

#[derive(Deserialize)]
struct Expected {
    xlim: [f64; 2],
    ylim: [f64; 2],
    figure_inches: [f64; 2],
}

/// Where the legacy frame stopped: the first/last column carrying land, and
/// the lowest baseline that still had something on it.
#[derive(Deserialize)]
struct LegacyExtent {
    xmin: usize,
    lowest_baseline: f64,
}

fn fixture() -> Option<Fixture> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/legacy/frame.json");
    if !path.exists() {
        assert!(
            std::env::var_os("RIDGE_REQUIRE_FIXTURES").is_none(),
            "RIDGE_REQUIRE_FIXTURES is set but {} is missing; run scripts/gen_legacy_fixture.py",
            path.display()
        );
        eprintln!(
            "skipping legacy frame parity: {} not generated",
            path.display()
        );
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap())
}

#[test]
fn frame_covers_the_window_where_the_legacy_frame_clips() {
    let Some(f) = fixture() else {
        return;
    };

    let nrows = f.input.len();
    let ncols = f.input.first().map_or(0, Vec::len);
    let mut grid = Array2::from_elem((nrows, ncols), f64::NAN);
    for (r, row) in f.input.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            grid[(r, c)] = v.unwrap_or(f64::NAN);
        }
    }

    let processed = preprocess(
        &grid,
        f.water_ntile,
        f.lake_flatness,
        f.vertical_ratio,
        1.0,
        None,
    )
    .expect("fixture input has terrain");
    let ratio = (f.bbox[3] - f.bbox[1]) / (f.bbox[2] - f.bbox[0]);
    let scene = RidgeScene::from_grid(&processed, ratio, f.size_scale);

    // The fixture is only meaningful if masking really did remove whole
    // columns and rows, i.e. that the legacy frame had something to clip.
    assert!(
        f.legacy_extent.xmin > 0,
        "legacy frame starts at column {}, so no water columns were dropped",
        f.legacy_extent.xmin
    );
    let floor = -LINE_SPACING * (nrows - 1) as f64;
    assert!(
        f.legacy_extent.lowest_baseline > floor,
        "legacy frame floor {} already reaches the window floor {floor}",
        f.legacy_extent.lowest_baseline
    );

    // The figure itself is shared with the legacy run...
    let (w, h) = (scene.layout.width_px, scene.layout.height_px);
    assert!(
        (w - f.expected.figure_inches[0] * FIG_DPI).abs() < 1e-9
            && (h - f.expected.figure_inches[1] * FIG_DPI).abs() < 1e-9,
        "figure {}x{} px, legacy says {}x{} in",
        w,
        h,
        f.expected.figure_inches[0],
        f.expected.figure_inches[1]
    );

    // ...while the data limits are the deliberate deviation: the whole window
    // with matplotlib's 5% margins, so nothing masking-related can move them.
    let xmax = (ncols - 1) as f64;
    let dx = xmax * AXES_MARGIN;
    assert!(
        (scene.layout.xlim[0] + dx).abs() < 1e-9
            && (scene.layout.xlim[1] - (xmax + dx)).abs() < 1e-9,
        "x frame {:?} should span columns 0..{xmax} +/- {dx}",
        scene.layout.xlim
    );
    assert!(
        scene.layout.ylim[0] < floor,
        "y frame {:?} should reach the window floor {floor}",
        scene.layout.ylim
    );

    // And the legacy frame is measurably tighter, on both axes.
    assert!(
        f.expected.xlim[0] > scene.layout.xlim[0] + 1.0,
        "legacy x frame {:?} was expected to clip the water columns (ours {:?})",
        f.expected.xlim,
        scene.layout.xlim
    );
    assert!(
        f.expected.ylim[0] > scene.layout.ylim[0] + 1.0,
        "legacy y frame {:?} was expected to stop above the water rows (ours {:?})",
        f.expected.ylim,
        scene.layout.ylim
    );
}
