//! Where do lake-flatness holes come from? Compare the lake mask across
//! sampling densities and map where masked cells land.

use ndarray::Array2;
use ridge_core::preprocess::{gradient3x3, percentile_linear};
use ridge_core::srtm::DirSource;

fn normalize(values: &Array2<f64>) -> (Array2<f64>, Vec<bool>) {
    let mut v = values.clone();
    let nan_mask: Vec<bool> = v.iter().map(|x| x.is_nan()).collect();
    let finite: Vec<f64> = v.iter().cloned().filter(|x| !x.is_nan()).collect();
    let min = finite.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = max - min;
    for (i, x) in v.iter_mut().enumerate() {
        if nan_mask[i] {
            *x = min;
        } else if span > 0.0 {
            *x = (*x - min) / span;
        } else {
            *x = 0.0;
        }
    }
    (v, nan_mask)
}

fn count_masks_gated(values: &Array2<f64>, lake_flatness: i32) -> (usize, usize, usize) {
    // Same as count_masks but with the two-scale gate (3x3 AND 5x5 flat).
    let (nrows, ncols) = values.dim();
    let (v, nan_mask) = normalize(values);
    let mut sorted = v.iter().cloned().collect::<Vec<f64>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let water_level = percentile_linear(&sorted, 10.0);
    let img: Vec<u8> = v.iter().map(|x| ((x * 255.0).round().clamp(0.0, 255.0)) as u8).collect();
    let g3 = ridge_core::preprocess::morphological_gradient(&img, nrows, ncols, 1);
    let g5 = ridge_core::preprocess::morphological_gradient(&img, nrows, ncols, 2);
    let mut water = 0;
    let mut lake = 0;
    let mut kept = 0;
    for (i, x) in v.iter().enumerate() {
        if nan_mask[i] {
            continue;
        }
        if *x < water_level {
            water += 1;
        } else if (g3[i] as i32) < lake_flatness && (g5[i] as i32) < lake_flatness {
            lake += 1;
        } else {
            kept += 1;
        }
    }
    (water, lake, kept)
}

fn count_masks(values: &Array2<f64>, lake_flatness: i32) -> (usize, usize, usize) {
    // returns (water, lake_only, kept)
    let (nrows, ncols) = values.dim();
    let (v, nan_mask) = normalize(values);
    let mut sorted = v.iter().cloned().collect::<Vec<f64>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let water_level = percentile_linear(&sorted, 10.0);
    // float gradient (the new implementation; no exclusions in this probe)
    let grad: Vec<f32> = ridge_core::preprocess::masked_gradient3x3(
        v.as_slice().expect("contiguous"),
        &vec![false; nrows * ncols],
        nrows,
        ncols,
    );
    let lf = lake_flatness as f32 / 255.0;
    let mut water = 0;
    let mut lake_only = 0;
    let mut kept = 0;
    for (i, x) in v.iter().enumerate() {
        if nan_mask[i] {
            continue;
        }
        if *x < water_level {
            water += 1;
        } else if grad[i] < lf {
            lake_only += 1;
        } else {
            kept += 1;
        }
    }
    (water, lake_only, kept)
}

fn main() {
    let srtm_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/srtm")
        .canonicalize()
        .unwrap();
    let src = DirSource::new(srtm_dir);
    let bbox = ridge_core::DEFAULT_BBOX;

    for (n, p, label) in [
        (80usize, 300usize, "old anisotropic (80x300)"),
        (219, 300, "new square-cell (219x300)"),
    ] {
        let values = ridge_core::grid::sample(&src, &bbox, n, p);
        let total = n * p;
        println!("{label}: total {total}");
        for lf in [2, 3, 4, 6] {
            let (water, lake, kept) = count_masks(&values, lf);
            let (water2, lake2, _kept2) = count_masks_gated(&values, lf);
            println!(
                "  lake_flatness={lf}: naive lake-only {lake:5} ({:4.1}%) -> gated {lake2:5} ({:4.1}%)",
                100.0 * lake as f64 / total as f64,
                100.0 * lake2 as f64 / total as f64,
            );
            let _ = (water, kept, water2);
        }
    }

    // ASCII map of lake-only masked cells at lake_flatness=3, new sampling.
    let (n, p) = (219usize, 300usize);
    let values = ridge_core::grid::sample(&src, &bbox, n, p);
    let (v, nan_mask) = normalize(&values);
    let mut sorted = v.iter().cloned().collect::<Vec<f64>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let water_level = percentile_linear(&sorted, 10.0);
    let img: Vec<u8> = v.iter().map(|x| ((x * 255.0).round().clamp(0.0, 255.0)) as u8).collect();
    let grad = ridge_core::preprocess::gradient3x3(&img, n, p);
    println!();
    println!("lake-only mask at lake_flatness=3 (219x300), 1 char = 9x9 cells:");
    for br in 0..(n / 9) {
        let mut line = String::new();
        for bc in 0..(p / 9) {
            let mut m = 0;
            for r in br * 9..(br + 1) * 9 {
                for c in bc * 9..(bc + 1) * 9 {
                    let i = r * p + c;
                    if !nan_mask[i] && v[[r, c]] >= water_level && (grad[i] as i32) < 3 {
                        m += 1;
                    }
                }
            }
            line.push(if m == 0 { '.' } else if m < 20 { '+' } else { '#' });
        }
        println!("{line}");
    }
}
