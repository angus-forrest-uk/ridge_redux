//! Turn a processed elevation grid into drawable ridge lines.
//!
//! Upstream `plot_map` draws row `i` at `y = row - 6*i` with a
//! background-colored fill from the baseline up to the line (the occlusion
//! trick that makes front ridges hide back ridges). `RidgeScene` captures
//! that geometry, plus the matplotlib figure layout (figure size, axes rect,
//! data limits) so the SVG exporter and the web canvas render identically.

use ndarray::Array2;

use crate::colormap::LineColor;
use crate::Error;

/// Upstream: `y_base = -6 * idx * np.ones_like(row)`.
pub const LINE_SPACING: f64 = 6.0;
/// matplotlib default dpi used for px math (figsize is in inches).
pub const FIG_DPI: f64 = 100.0;
/// matplotlib default `figure.figsize` used by upstream (`size_scale=20`).
pub const DEFAULT_SIZE_SCALE: f64 = 20.0;
/// matplotlib default `subplot_params` (fractions of the figure).
pub const SUBPLOT_LEFT: f64 = 0.125;
pub const SUBPLOT_RIGHT: f64 = 0.9;
pub const SUBPLOT_BOTTOM: f64 = 0.11;
pub const SUBPLOT_TOP: f64 = 0.88;
/// matplotlib default axes margins (5% padding each side of the data).
pub const AXES_MARGIN: f64 = 0.05;

/// Where the axes rect sits and how data maps into it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FigureLayout {
    /// Figure size in px (`size_scale` inches at `FIG_DPI`).
    pub width_px: f64,
    pub height_px: f64,
    /// Axes rect in px from the top-left: `[x0, y0, x1, y1]`.
    pub axes: [f64; 4],
    /// Data window shown in the axes (includes matplotlib's 5% margins).
    pub xlim: [f64; 2],
    pub ylim: [f64; 2],
}

impl FigureLayout {
    /// Map scene data coords to figure px (y flipped, SVG convention).
    pub fn to_px(&self, x: f64, y: f64) -> (f64, f64) {
        let fx = (x - self.xlim[0]) / (self.xlim[1] - self.xlim[0]);
        let fy = (y - self.ylim[0]) / (self.ylim[1] - self.ylim[0]);
        let ax_x0 = self.axes[0];
        let ax_x1 = self.axes[2];
        let ax_y0 = self.axes[1]; // top
        let ax_y1 = self.axes[3]; // bottom
        (
            ax_x0 + fx * (ax_x1 - ax_x0),
            ax_y0 + (1.0 - fy) * (ax_y1 - ax_y0),
        )
    }

    /// Axes-fraction coords (upstream `transform=ax.transAxes`) to figure px.
    pub fn frac_to_px(&self, fx: f64, fy: f64) -> (f64, f64) {
        let x = self.axes[0] + fx * (self.axes[2] - self.axes[0]);
        let y = self.axes[1] + (1.0 - fy) * (self.axes[3] - self.axes[1]);
        (x, y)
    }
}

/// One ridge line: its baseline and y-values (`NaN` = gap, no line drawn).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RidgeRow {
    pub baseline: f64,
    /// y = elevation * vertical_ratio + baseline; `NaN` marks water/gaps.
    pub y: Vec<f64>,
}

impl RidgeRow {
    /// Contiguous (start, end) runs of finite values, for gap-aware drawing.
    pub fn runs(&self) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut start: Option<usize> = None;
        for (i, v) in self.y.iter().enumerate() {
            if v.is_finite() {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                if i > s {
                    runs.push((s, i)); // exclusive end
                }
            }
        }
        if let Some(s) = start {
            if self.y.len() > s {
                runs.push((s, self.y.len()));
            }
        }
        runs
    }
}

/// What drives per-line color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorKind {
    /// Color by line index (upstream `kind="gradient"`).
    Gradient,
    /// Color by actual elevation along the line (upstream `kind="elevation"`).
    Elevation,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RidgeScene {
    pub rows: Vec<RidgeRow>,
    pub n_points: usize,
    /// Range of the (scaled) processed elevations, for `Elevation` coloring.
    pub vmin: f64,
    pub vmax: f64,
    pub layout: FigureLayout,
}

impl RidgeScene {
    /// Build a scene from a preprocessed grid (already masked, flipped and
    /// vertically scaled by `preprocess::preprocess`).
    pub fn from_grid(processed: &Array2<f64>, bbox_ratio: f64, size_scale: f64) -> RidgeScene {
        let (nrows, ncols) = processed.dim();
        let rows: Vec<RidgeRow> = processed
            .outer_iter()
            .enumerate()
            .map(|(i, src)| {
                let baseline = -LINE_SPACING * i as f64;
                RidgeRow {
                    baseline,
                    y: src.iter().map(|&v| v + baseline).collect(),
                }
            })
            .collect();

        let (mut vmin, mut vmax) = processed
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        if !vmin.is_finite() {
            vmin = 0.0;
            vmax = 1.0;
        }

        // matplotlib autoscale with 5% margins — around the ACTUAL drawn
        // content, so the scene stays framed at any rotation angle (the
        // camera operator keeps the subject centered at constant size).
        let (xmin, xmax_data, ymax_data) = rows
            .iter()
            .flat_map(|row| row.y.iter().enumerate())
            .filter(|&(_, y)| y.is_finite())
            .fold(
                (usize::MAX, 0usize, f64::NEG_INFINITY),
                |(xmin, xmax, ymax), (c, &y)| (xmin.min(c), xmax.max(c), ymax.max(y)),
            );
        // Fills reach the baseline, so a row with any data bounds the content
        // from below.
        let ymin = rows
            .iter()
            .filter(|row| row.y.iter().any(|y| y.is_finite()))
            .map(|row| row.baseline)
            .fold(f64::INFINITY, f64::min);
        let (xmin, xmax_data, ymin, ymax_data) = if xmin == usize::MAX {
            // No drawable content: fall back to the theoretical frame.
            (0, ncols - 1, -LINE_SPACING * (nrows - 1) as f64, vmax)
        } else {
            (xmin, xmax_data, ymin, ymax_data)
        };
        let dx = (xmax_data - xmin) as f64 * AXES_MARGIN;
        let dy = (ymax_data - ymin) * AXES_MARGIN;

        let width_px = size_scale * FIG_DPI;
        let height_px = size_scale * bbox_ratio * FIG_DPI;
        let layout = FigureLayout {
            width_px,
            height_px,
            axes: [
                SUBPLOT_LEFT * width_px,
                (1.0 - SUBPLOT_TOP) * height_px,
                SUBPLOT_RIGHT * width_px,
                (1.0 - SUBPLOT_BOTTOM) * height_px,
            ],
            xlim: [xmin as f64 - dx, xmax_data as f64 + dx],
            ylim: [ymin - dy, ymax_data + dy],
        };

        RidgeScene {
            rows,
            n_points: ncols,
            vmin,
            vmax,
            layout,
        }
    }

    /// Color for line `idx` under `Gradient` mode (upstream `line_color(i/n)`).
    pub fn gradient_color(&self, line: &LineColor, idx: usize) -> crate::colormap::Rgb {
        match line {
            LineColor::Solid(rgb) => *rgb,
            LineColor::Map(cm) => {
                let denom = self.rows.len().saturating_sub(1).max(1) as f64;
                cm.at(idx as f64 / denom)
            }
        }
    }

    /// Color for a point value under `Elevation` mode (upstream norm).
    pub fn elevation_color(&self, line: &LineColor, value: f64) -> crate::colormap::Rgb {
        let LineColor::Map(cm) = line else {
            return [0, 0, 0];
        };
        let t = if self.vmax > self.vmin {
            (value - self.vmin) / (self.vmax - self.vmin)
        } else {
            0.0
        };
        cm.at(t)
    }

    /// The upstream default label color: `line_color(0.0)` for colormaps.
    pub fn label_color(&self, line: &LineColor) -> crate::colormap::Rgb {
        match line {
            LineColor::Solid(rgb) => *rgb,
            LineColor::Map(cm) => cm.at(0.0),
        }
    }
}

/// How the rotated grid fits the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Upstream behavior: scipy `reshape=True/False`, zero-filled borders.
    Reshape,
    /// Fixed canvas rotating about the center; out-of-plane cells become
    /// gaps. Used for interactive client-side rotation and WYSIWYG export.
    Plane,
}

/// Run the full pipeline: sample -> (optional rotate) -> preprocess -> scene.
#[allow(clippy::too_many_arguments)]
pub fn build_scene(
    source: &dyn crate::srtm::TileSource,
    bbox: &crate::Bbox,
    num_lines: usize,
    elevation_pts: usize,
    viewpoint_angle: f64,
    crop: bool,
    interpolation: u32,
    lock_resolution: bool,
    water_ntile: f64,
    lake_flatness: i32,
    vertical_ratio: f64,
    size_scale: f64,
) -> Result<RidgeScene, Error> {
    build_scene_fit(
        source,
        bbox,
        num_lines,
        elevation_pts,
        viewpoint_angle,
        crop,
        interpolation,
        lock_resolution,
        Fit::Reshape,
        water_ntile,
        lake_flatness,
        vertical_ratio,
        size_scale,
    )
}

/// As `build_scene`, with an explicit [`Fit`]. `Fit::Plane` also skips the
/// upstream axis swap (the client rotates a fixed-resolution plane), so the
/// scene the browser recomputes locally matches the exported SVG exactly.
#[allow(clippy::too_many_arguments)]
pub fn build_scene_fit(
    source: &dyn crate::srtm::TileSource,
    bbox: &crate::Bbox,
    num_lines: usize,
    elevation_pts: usize,
    viewpoint_angle: f64,
    crop: bool,
    interpolation: u32,
    lock_resolution: bool,
    fit: Fit,
    water_ntile: f64,
    lake_flatness: i32,
    vertical_ratio: f64,
    size_scale: f64,
) -> Result<RidgeScene, Error> {
    if !bbox.is_valid() {
        return Err(Error::InvalidBbox(*bbox));
    }
    let rotating = viewpoint_angle.rem_euclid(360.0) != 0.0;
    let mut values = match fit {
        Fit::Plane => {
            let _ = lock_resolution;
            crate::grid::sample(source, bbox, num_lines, elevation_pts)
        }
        Fit::Reshape => {
            let (mut lines, mut pts) = (num_lines, elevation_pts);
            if !lock_resolution && crate::grid::swap_for_angle(viewpoint_angle) {
                std::mem::swap(&mut lines, &mut pts);
            }
            crate::grid::sample(source, bbox, lines, pts)
        }
    };
    if rotating {
        values = match fit {
            Fit::Plane => {
                crate::rotate::rotate_fixed_plane(&values, viewpoint_angle, interpolation)
            }
            Fit::Reshape => crate::rotate::rotate(&values, viewpoint_angle, !crop, interpolation),
        };
    }
    let processed = crate::preprocess::preprocess(
        &values,
        water_ntile,
        lake_flatness,
        vertical_ratio,
        1.0,  // reshape is upstream-faithful: naive threshold at grid sampling
        None, // stats over the whole (rotated) grid
    )?;
    Ok(RidgeScene::from_grid(&processed, bbox.ratio(), size_scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srtm::SyntheticSource;

    #[test]
    fn runs_split_at_gaps() {
        let row = RidgeRow {
            baseline: -6.0,
            y: vec![1.0, f64::NAN, f64::NAN, 2.0, 3.0, f64::NAN, 4.0],
        };
        assert_eq!(row.runs(), vec![(0, 1), (3, 5), (6, 7)]);
    }

    #[test]
    fn scene_geometry() {
        let src = SyntheticSource { side: 1201 };
        let scene = build_scene(
            &src,
            &crate::DEFAULT_BBOX,
            20,
            30,
            0.0,
            false,
            0,
            false,
            10.0,
            3,
            40.0,
            DEFAULT_SIZE_SCALE,
        )
        .unwrap();
        assert_eq!(scene.rows.len(), 20);
        assert_eq!(scene.n_points, 30);
        // Row baselines step by -6.
        assert_eq!(scene.rows[0].baseline, 0.0);
        assert_eq!(scene.rows[7].baseline, -42.0);
        // Layout: 20 in x 100 dpi = 2000 px wide.
        assert_eq!(scene.layout.width_px, 2000.0);
        assert_eq!(scene.layout.height_px, 2000.0 * crate::DEFAULT_BBOX.ratio());
    }
}
