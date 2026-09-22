//! Frozen ports of the reference implementations ridge-core has to match.
//!
//! Everything here reproduces an external reference — `scipy.ndimage`,
//! `numpy`, `skimage` (and, later, `srtm.py` / matplotlib) — so that our
//! output matches the Python stack byte-for-byte. **Treat this module as
//! read-only.** A changed constant, rounding rule or iteration order alters
//! rendered artwork and can silently break the parity tests
//! (`grid::parity_tests`, the `preprocess`/`colormap` unit tests, and the
//! JS/Rust pipeline check).
//!
//! New behaviour does not belong here. Where a port needs an extension or a
//! deliberate divergence, that code lives in its own module and *uses* these
//! primitives — e.g. [`crate::rotate::rotate_fixed_plane`] (interactive
//! rotation), [`crate::grid::sample_disc`]/[`crate::grid::sample_window`]
//! (disc/rect views), and [`crate::preprocess::masked_gradient3x3`] (the
//! float lake mask).

pub mod numpy;
pub mod scipy_ndimage;
pub mod skimage;
