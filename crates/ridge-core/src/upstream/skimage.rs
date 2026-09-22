//! `skimage.filters.rank` — frozen ports.
//!
//! The u8 `max - min` gradient. Note the *masked* float variant used by the
//! production lake mask is deliberately **not** here: it diverges from skimage
//! on purpose and lives in [`crate::preprocess::masked_gradient3x3`].

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
