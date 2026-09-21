//! What does the "camera" do as the angle changes?
//! For each angle: where does the terrain content sit inside the fixed frame,
//! and how does the plane footprint deform?

use ridge_core::srtm::DirSource;

fn main() {
    let srtm_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/srtm")
        .canonicalize()
        .expect("fixtures/srtm not found");
    let src = DirSource::new(srtm_dir);
    let bbox = ridge_core::DEFAULT_BBOX;
    let (n, p) = (80usize, 300usize);

    println!("auto-framed orbit: the view window hugs the content at every angle");
    println!();
    println!("angle | frame (data window)      | apparent size of the landscape          |");
    for deg in [0, 15, 30, 45, 60, 75, 90] {
        let mut values = ridge_core::grid::sample(&src, &bbox, n, p);
        values = ridge_core::rotate::rotate_fixed_plane(&values, deg as f64, 0);
        let processed =
            ridge_core::preprocess::preprocess(&values, 10.0, 3, 40.0, 1.0, None).unwrap();

        // Content bounds: fills reach the baseline of any row that has data.
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        let mut xmin = f64::INFINITY;
        let mut xmax = f64::NEG_INFINITY;
        for i in 0..n {
            let baseline = -6.0 * i as f64;
            let mut has_data = false;
            for c in 0..p {
                let v = processed[(i, c)];
                if v.is_finite() {
                    has_data = true;
                    let y = v + baseline;
                    if y > ymax {
                        ymax = y;
                    }
                    if (c as f64) < xmin {
                        xmin = c as f64;
                    }
                    if c as f64 > xmax {
                        xmax = c as f64;
                    }
                }
            }
            if has_data && baseline < ymin {
                ymin = baseline;
            }
        }
        let mid = (ymin + ymax) / 2.0;
        let _ = mid;
        let scene = ridge_core::RidgeScene::from_grid(&processed, bbox.ratio(), 20.0);
        let [_, ytop] = scene.layout.ylim;
        let [xl, xr] = scene.layout.xlim;
        let width_pct = 100.0 * (xmax - xmin) / ((p - 1) as f64);
        // Apparent size: content extent relative to the data window.
        let y_span = (scene.layout.ylim[1] - scene.layout.ylim[0]).abs();
        let x_span = xr - xl;
        println!(
            "{deg:4}° | frame y [{:7.1}, {:7.1}] | apparent size: x {:5.1}% of frame, y {:5.1}% of frame | content x {:5.1}% of canvas",
            ytop - y_span, ytop,
            100.0 * (xmax - xmin) / x_span,
            100.0 * (ymax - ymin) / y_span,
            width_pct,
        );
        eprintln!(
            "  content x-extent: {:.0} of {} cols  (apparent width {:.0}%)",
            xmax - xmin,
            p as f64,
            100.0 * (xmax - xmin) / (p as f64 - 1.0)
        );
    }
}
