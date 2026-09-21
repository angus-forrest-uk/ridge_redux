//! SciPy-compatible 2D array rotation.
//!
//! Replicates `scipy.ndimage.rotate(input, angle, axes=(1, 0), reshape,
//! order, mode='constant', cval=0.0)` for spline orders 0 (nearest) and 1
//! (bilinear), which covers everything ridge_map uses (`interpolation` in
//! 0..=1; higher orders upstream tend to produce all-NaN maps anyway).
//!
//! Algorithm (from `scipy/ndimage/_interpolation.py::rotate`):
//!
//! ```text
//! M = [[cos, sin], [-sin, cos]]      # cosdg/sindg: exact at 90-degree steps
//! in_plane  = (nrows, ncols)
//! if reshape:
//!     bounds  = M @ [(0,0), (0,ncols), (nrows,0), (nrows,ncols)]
//!     out_plane = floor(ptp(bounds) + 0.5)
//! else:
//!     out_plane = in_plane
//! out_center = M @ ((out_plane - 1) / 2)
//! in_center  = (in_plane - 1) / 2
//! offset     = in_center - out_center
//! output[o]  = interpolate(input, M @ o + offset)   # cval outside
//! ```

use ndarray::Array2;

/// `scipy.special.cosdg/sindg` — exact values at multiples of 90 degrees.
fn sincosdg(angle_deg: f64) -> (f64, f64) {
    let a = angle_deg.rem_euclid(360.0);
    match a {
        0.0 => (1.0, 0.0),
        90.0 => (0.0, 1.0),
        180.0 => (-1.0, 0.0),
        270.0 => (0.0, -1.0),
        v => {
            let rad = v * std::f64::consts::PI / 180.0;
            (rad.cos(), rad.sin())
        }
    }
}

fn output_shape(shape: (usize, usize), angle_deg: f64, reshape: bool) -> (usize, usize) {
    let (c, s) = sincosdg(angle_deg);
    let (nrows, ncols) = shape;
    if !reshape {
        return shape;
    }
    // M @ the four corners (0,0), (0,ncols), (nrows,0), (nrows,ncols).
    let mut rs = [0.0f64; 4];
    let mut cs = [0.0f64; 4];
    for (k, (r, col)) in [(0.0, 0.0), (0.0, ncols as f64), (nrows as f64, 0.0), (nrows as f64, ncols as f64)]
        .into_iter()
        .enumerate()
    {
        rs[k] = c * r + s * col;
        cs[k] = -s * r + c * col;
    }
    let ptp_r = rs.iter().cloned().fold(f64::MIN, f64::max)
        - rs.iter().cloned().fold(f64::MAX, f64::min);
    let ptp_c = cs.iter().cloned().fold(f64::MIN, f64::max)
        - cs.iter().cloned().fold(f64::MAX, f64::min);
    // (ptp + 0.5).astype(int): numpy casts truncate toward zero; values are positive.
    ((ptp_r + 0.5) as usize, (ptp_c + 0.5) as usize)
}

/// Rotate `input` by `angle_deg` degrees, replicating scipy's `mode='constant'`,
/// `cval=0.0` behavior (out-of-bounds samples become exactly 0.0).
pub fn rotate(input: &Array2<f64>, angle_deg: f64, reshape: bool, order: u32) -> Array2<f64> {
    assert!(order <= 1, "only spline orders 0 and 1 are supported");
    let (nrows, ncols) = (input.nrows(), input.ncols());
    let (c, s) = sincosdg(angle_deg);
    let (out_rows, out_cols) = output_shape((nrows, ncols), angle_deg, reshape);

    // Centers are at plane midpoints ((n - 1) / 2 each).
    let in_center = ((nrows as f64 - 1.0) / 2.0, (ncols as f64 - 1.0) / 2.0);
    let out_center_r = c * (out_rows as f64 - 1.0) / 2.0 + s * (out_cols as f64 - 1.0) / 2.0;
    let out_center_c = -s * (out_rows as f64 - 1.0) / 2.0 + c * (out_cols as f64 - 1.0) / 2.0;
    let offset = (in_center.0 - out_center_r, in_center.1 - out_center_c);

    let mut out = Array2::<f64>::zeros((out_rows, out_cols));
    for or in 0..out_rows {
        for oc in 0..out_cols {
            let pr = c * or as f64 + s * oc as f64 + offset.0;
            let pc = -s * or as f64 + c * oc as f64 + offset.1;
            out[(or, oc)] = match order {
                0 => sample_nearest(input, pr, pc, 0.0),
                _ => sample_bilinear(input, pr, pc, 0.0),
            };
        }
    }
    out
}

fn sample_nearest(input: &Array2<f64>, pr: f64, pc: f64, fill: f64) -> f64 {
    let r = (pr + 0.5).floor(); // scipy order-0: round half up
    let c = (pc + 0.5).floor();
    if r < 0.0 || c < 0.0 || r as usize >= input.nrows() || c as usize >= input.ncols() {
        return fill;
    }
    input[(r as usize, c as usize)]
}

fn sample_bilinear(input: &Array2<f64>, pr: f64, pc: f64, fill: f64) -> f64 {
    let max_r = input.nrows() as f64 - 1.0;
    let max_c = input.ncols() as f64 - 1.0;
    if pr < 0.0 || pc < 0.0 || pr > max_r || pc > max_c {
        return fill;
    }
    let r0 = pr.floor();
    let c0 = pc.floor();
    let tr = pr - r0;
    let tc = pc - c0;
    let (ri, ci) = (r0 as usize, c0 as usize);
    let r1 = (ri + 1).min(input.nrows() - 1);
    let c1 = (ci + 1).min(input.ncols() - 1);
    let top = input[(ri, ci)] * (1.0 - tc) + input[(ri, c1)] * tc;
    let bot = input[(r1, ci)] * (1.0 - tc) + input[(r1, c1)] * tc;
    top * (1.0 - tr) + bot * tr
}

/// Rotate about the array center *within the same canvas*: output dims equal
/// input dims, and samples that fall outside the source become `NaN` (a gap,
/// not a zero). This is the interactive-rotation mode used by the web
/// frontend and `Fit::Plane` exports — the plane never grows, so viewport
/// zoom/distance stays stable while the terrain spins about the center.
pub fn rotate_fixed_plane(input: &Array2<f64>, angle_deg: f64, order: u32) -> Array2<f64> {
    assert!(order <= 1, "only spline orders 0 and 1 are supported");
    let (nrows, ncols) = (input.nrows(), input.ncols());
    let (c, s) = sincosdg(angle_deg);

    // Both planes share one center: in_center == out_center, so the affine
    // offset vanishes and input_coord = M @ (out - center) + center.
    let cr = (nrows as f64 - 1.0) / 2.0;
    let cc = (ncols as f64 - 1.0) / 2.0;
    let mut out = Array2::<f64>::from_elem((nrows, ncols), f64::NAN);
    for or in 0..nrows {
        for oc in 0..ncols {
            let pr = c * (or as f64 - cr) + s * (oc as f64 - cc) + cr;
            let pc = -s * (or as f64 - cr) + c * (oc as f64 - cc) + cc;
            out[(or, oc)] = match order {
                0 => sample_nearest(input, pr, pc, f64::NAN),
                _ => sample_bilinear(input, pr, pc, f64::NAN),
            };
        }
    }
    out
}

#[cfg(test)]
mod fixed_plane_tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn same_dims_any_angle() {
        let a = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        for angle in [0.0, 17.0, 90.0, 331.0] {
            let out = rotate_fixed_plane(&a, angle, 0);
            assert_eq!(out.dim(), a.dim());
        }
    }

    #[test]
    fn identity_and_quarter_turn() {
        let a = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        assert_eq!(rotate_fixed_plane(&a, 0.0, 0), a);
        // 90-degree turn about the center, same convention as scipy:
        // out[i, j] == a[j, n-1-i]. All samples land on grid nodes.
        let out = rotate_fixed_plane(&a, 90.0, 0);
        assert_eq!(out, array![[3.0, 6.0, 9.0], [2.0, 5.0, 8.0], [1.0, 4.0, 7.0]]);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn off_grid_angles_leave_corner_gaps() {
        // Big enough that the corners genuinely fall outside at 45 degrees.
        let mut a = Array2::from_elem((8, 8), 0.0);
        for r in 0..8 { for c in 0..8 { a[(r, c)] = (r * 8 + c) as f64; } }
        let out = rotate_fixed_plane(&a, 45.0, 0);
        assert_eq!(out.dim(), (8, 8));
        assert!(out[[0, 0]].is_nan(), "corner becomes a gap, not zero");
        assert!(out[[7, 7]].is_nan());
        assert!(out[[3, 4]].is_finite(), "interior survives");
        // Center cell still samples one of the four central source values.
        let center = out[[3, 3]];
        assert!(
            [a[[3, 3]], a[[3, 4]], a[[4, 3]], a[[4, 4]]].contains(&center),
            "center sampled {center}"
        );
    }

    #[test]
    fn zero_fill_mode_untouched() {
        // The scipy-parity `rotate` still zero-fills out-of-bounds samples.
        let a = array![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0], [7.0, 8.0]];
        let out = rotate(&a, 45.0, false, 0);
        assert_eq!(out[(3, 0)], 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn identity_at_zero() {
        let a = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let out = rotate(&a, 0.0, true, 0);
        assert_eq!(out, a);
    }

    #[test]
    fn quarter_turn_matches_scipy() {
        // scipy.ndimage.rotate([[1,2,3],[4,5,6]], 90, order=0)
        // -> [[3, 6], [2, 5], [1, 4]]
        let a = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let out = rotate(&a, 90.0, true, 0);
        assert_eq!(out.dim(), (3, 2));
        assert_eq!(
            out,
            array![[3.0, 6.0], [2.0, 5.0], [1.0, 4.0]]
        );
    }

    #[test]
    fn crop_keeps_shape_and_zeroes_outside() {
        let a = array![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0], [7.0, 8.0]];
        let out = rotate(&a, 45.0, false, 0);
        assert_eq!(out.dim(), (4, 2));
        // Hand-computed nearest mappings:
        //   out(0,0) -> input (0.086, 1.207) -> (0, 1) = 2
        //   out(0,1) -> input (0.793, 1.914) -> (1, 2) out of bounds -> 0
        //   out(3,0) -> input (2.207, -0.914) -> c rounds to -1 -> 0
        //   out(3,1) -> input (2.914, -0.207) -> (3, 0) = 7
        assert_eq!(out[(0, 0)], 2.0);
        assert_eq!(out[(0, 1)], 0.0);
        assert_eq!(out[(3, 0)], 0.0);
        assert_eq!(out[(3, 1)], 7.0);
        // An interior cell well away from rounding boundaries.
        assert_eq!(out[(2, 1)], 6.0);
    }

    #[test]
    fn reshape_grows_the_canvas() {
        let a = Array2::<f64>::from_elem((80, 300), 1.0);
        let out45 = rotate(&a, 45.0, true, 0);
        // ptp for 45 deg: (80+300) * sqrt(2)/2 = 268.7 -> 269 both ways
        assert_eq!(out45.dim(), (269, 269));
        let out11 = rotate(&a, 11.0, true, 0);
        // bounds r: 80*cos11 + 300*sin11 = 78.53 + 57.24 = 135.77 -> 136
        // bounds c: 300*cos11 + 80*sin11 = 294.49 + 15.26 = 309.75 -> 310
        assert_eq!(out11.dim(), (136, 310));
    }

    #[test]
    fn negative_and_large_angles() {
        let a = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let out = rotate(&a, -270.0, true, 0); // == +90
        assert_eq!(out, rotate(&a, 90.0, true, 0));
        let out2 = rotate(&a, 450.0, true, 0);
        assert_eq!(out2, rotate(&a, 90.0, true, 0));
    }
}
