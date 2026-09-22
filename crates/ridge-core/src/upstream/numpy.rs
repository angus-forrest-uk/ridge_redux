//! `numpy.percentile` — frozen port.
//!
//! Only the default `'linear'` interpolation method is implemented, which is
//! all `ridge_map` uses.

/// Numpy's default ('linear') percentile on a non-empty slice.
pub fn percentile_linear(sorted: &[f64], q: f64) -> f64 {
    assert!(!sorted.is_empty(), "percentile of empty slice");
    assert!((0.0..=100.0).contains(&q), "q out of range");
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = q / 100.0 * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = idx - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_matches_numpy_linear() {
        // np.percentile([1,2,3,4,5,6,7,8,9,10], 10) = 1.9
        let s: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        assert!((percentile_linear(&s, 10.0) - 1.9).abs() < 1e-12);
        assert!((percentile_linear(&s, 50.0) - 5.5).abs() < 1e-12);
        assert!((percentile_linear(&s, 0.0) - 1.0).abs() < 1e-12);
        assert!((percentile_linear(&s, 100.0) - 10.0).abs() < 1e-12);
    }
}
