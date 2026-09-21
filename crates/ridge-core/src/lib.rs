//! ridge-core: the data pipeline behind ridgeline terrain art.
//!
//! Port of the Python `ridge_map` package (with the parts of `SRTM.py`,
//! `numpy` and `scipy.ndimage` it relies on) to Rust, minus matplotlib:
//! rendering is delegated to [`svg`] (server-side export) or to the web
//! frontend, which consumes [`geometry::RidgeScene`] JSON.

pub mod colormap;
pub mod geometry;
pub mod grid;
pub mod preprocess;
pub mod rotate;
pub mod srtm;
pub mod svg;

pub use geometry::{RidgeRow, RidgeScene};
pub use srtm::{Tile, TileSource};

use serde::{Deserialize, Serialize};

/// Default bounding box from upstream ridge_map: The White Mountains, NH.
pub const DEFAULT_BBOX: Bbox = Bbox {
    lon0: -71.928864,
    lat0: 43.758201,
    lon1: -70.957947,
    lat1: 44.465151,
};

/// Geographic bounding box, `(long, lat, long, lat)` of the bottom-left and
/// top-right corners, exactly like upstream `RidgeMap.__init__`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub lon0: f64,
    pub lat0: f64,
    pub lon1: f64,
    pub lat1: f64,
}

impl Bbox {
    pub fn new(lon0: f64, lat0: f64, lon1: f64, lat1: f64) -> Self {
        Self {
            lon0,
            lat0,
            lon1,
            lat1,
        }
    }

    /// Bottom and top latitude (upstream `lats` property).
    pub fn lats(&self) -> (f64, f64) {
        (self.lat0, self.lat1)
    }

    /// Left and right longitude (upstream `longs` property).
    pub fn longs(&self) -> (f64, f64) {
        (self.lon0, self.lon1)
    }

    /// Figure aspect ratio (height / width) used by upstream `plot_map`.
    pub fn ratio(&self) -> f64 {
        (self.lat1 - self.lat0) / (self.lon1 - self.lon0)
    }

    pub fn is_valid(&self) -> bool {
        self.lon1 > self.lon0
            && self.lat1 > self.lat0
            && self.lon0 >= -180.0
            && self.lon1 <= 180.0
            && self.lat0 >= -60.0
            && self.lat1 <= 60.0
    }
}

impl Default for Bbox {
    fn default() -> Self {
        DEFAULT_BBOX
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid bounding box: {0:?}")]
    InvalidBbox(Bbox),
    #[error("elevation data contains no valid points for this bbox")]
    EmptyData,
    #[error("srtm tile error: {0}")]
    Srtm(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
