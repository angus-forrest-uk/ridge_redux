//! 2D array rotation: the frozen scipy port plus our interactive mode.
//!
//! [`rotate`] is a re-export of the frozen `scipy.ndimage.rotate` port in
//! [`crate::upstream::scipy_ndimage`]. [`rotate_fixed_plane`] is *ours*: it
//! uses the same interpolation, but rotates about the array center inside a
//! fixed canvas (out-of-plane cells become gaps) so the viewport never moves —
//! the mode the web frontend's orbit and `Fit::Plane` exports use.

use ndarray::Array2;

pub use crate::upstream::scipy_ndimage::rotate;
use crate::upstream::scipy_ndimage::{sample_bilinear, sample_nearest, sincosdg};

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
        assert_eq!(
            out,
            array![[3.0, 6.0, 9.0], [2.0, 5.0, 8.0], [1.0, 4.0, 7.0]]
        );
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn off_grid_angles_leave_corner_gaps() {
        // Big enough that the corners genuinely fall outside at 45 degrees.
        let a = Array2::from_shape_fn((8, 8), |(r, c)| (r * 8 + c) as f64);
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
}
