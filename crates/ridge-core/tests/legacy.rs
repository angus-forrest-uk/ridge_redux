//! Legacy-equivalence test for the figure frame.
//!
//! `fixtures/legacy/frame.json` is captured by `scripts/gen_legacy_fixture.py`
//! running the *actual* upstream `ridge_map` package over matplotlib, so the
//! reference is the legacy implementation rather than a snapshot of our own
//! output. It pins both axes of the `clip_to_land` toggle: with it off we frame
//! the whole requested window (the default), and with it on we reproduce the
//! legacy figure to within floating point.
//!
//! Skipped when absent unless `RIDGE_REQUIRE_FIXTURES=1` (CI).

use std::path::Path;

use ndarray::Array2;
use serde::Deserialize;

use ridge_core::geometry::{Frame, AXES_MARGIN, FIG_DPI, LINE_SPACING};
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
fn clip_to_land_reproduces_the_legacy_frame() {
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
    let window = RidgeScene::from_grid(&processed, ratio, f.size_scale, Frame::Window);
    let land = RidgeScene::from_grid(&processed, ratio, f.size_scale, Frame::Land);

    // The fixture is only meaningful if masking really did remove whole
    // columns and rows, i.e. that there is a crop to reproduce.
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

    // The figure itself is shared with the legacy run, on both settings.
    for (name, scene) in [("window", &window), ("land", &land)] {
        let (w, h) = (scene.layout.width_px, scene.layout.height_px);
        assert!(
            (w - f.expected.figure_inches[0] * FIG_DPI).abs() < 1e-9
                && (h - f.expected.figure_inches[1] * FIG_DPI).abs() < 1e-9,
            "{name}: figure {w}x{h} px, legacy says {}x{} in",
            f.expected.figure_inches[0],
            f.expected.figure_inches[1]
        );
    }

    // Toggled on, we agree with the legacy figure: matplotlib autoscales
    // around the cells it draws, so the water columns and rows leave the
    // frame. This is the equivalence the toggle exists to offer.
    for (axis, got, want) in [
        ("xlim", land.layout.xlim, f.expected.xlim),
        ("ylim", land.layout.ylim, f.expected.ylim),
    ] {
        for (which, (g, w)) in std::iter::zip(got, want).enumerate() {
            assert!(
                (g - w).abs() <= 1e-9 * w.abs().max(1.0),
                "{axis}[{which}]: clip_to_land gives {g}, legacy matplotlib gives {w}"
            );
        }
    }

    // Toggled off (the default), the frame is the whole requested window with
    // matplotlib's 5% margins, so nothing masking-related can move it.
    let xmax = (ncols - 1) as f64;
    let dx = xmax * AXES_MARGIN;
    assert!(
        (window.layout.xlim[0] + dx).abs() < 1e-9
            && (window.layout.xlim[1] - (xmax + dx)).abs() < 1e-9,
        "x frame {:?} should span columns 0..{xmax} +/- {dx}",
        window.layout.xlim
    );
    assert!(
        window.layout.ylim[0] < floor,
        "y frame {:?} should reach the window floor {floor}",
        window.layout.ylim
    );

    // ...and that really is wider than what the legacy plot showed.
    assert!(
        window.layout.xlim[0] < land.layout.xlim[0] - 1.0,
        "window x frame {:?} should extend left of the land-only {:?}",
        window.layout.xlim,
        land.layout.xlim
    );
    assert!(
        window.layout.ylim[0] < land.layout.ylim[0] - 1.0,
        "window y frame {:?} should extend below the land-only {:?}",
        window.layout.ylim,
        land.layout.ylim
    );
}
