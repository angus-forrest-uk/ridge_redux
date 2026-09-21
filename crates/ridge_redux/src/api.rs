//! API handlers: parameter parsing, the render pipeline, JSON/SVG output.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use ridge_core::colormap::{hex_checked, LineColor};
use ridge_core::geometry::{ColorKind, DEFAULT_SIZE_SCALE};
use ridge_core::svg::{render_svg, Annotation, LabelStyle, LineColorSpec, PlotStyle, VAlign};
use ridge_core::{Bbox, DEFAULT_BBOX};

use crate::state::AppState;

#[derive(Deserialize, Debug, Clone)]
pub struct AnnotationParams {
    pub lon: f64,
    pub lat: f64,
    #[serde(default)]
    pub label: String,
    #[serde(default = "d_zero")]
    pub x_offset: f64,
    #[serde(default = "d_zero")]
    pub y_offset: f64,
    #[serde(default = "d_annotation_label_size")]
    pub label_size_pt: f64,
    #[serde(default = "d_annotation_dot")]
    pub dot_pt: f64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "d_false")]
    pub background: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct RenderParams {
    pub bbox: [f64; 4],
    pub num_lines: usize,
    pub elevation_pts: usize,
    pub viewpoint_angle: f64,
    pub crop: bool,
    pub interpolation: u32,
    pub lock_resolution: bool,
    pub water_ntile: f64,
    pub lake_flatness: i32,
    pub vertical_ratio: f64,
    pub linewidth_pt: f64,
    /// Color name, hex, or colormap name.
    pub line_color: String,
    pub kind: String,
    pub background_color: String,
    pub size_scale: f64,
    pub label: String,
    pub label_color: Option<String>,
    pub label_x: f64,
    pub label_y: f64,
    pub label_size_pt: f64,
    pub label_font: String,
    pub annotation: Option<AnnotationParams>,
    /// "reshape" (upstream scipy behavior) or "plane" (fixed canvas, WYSIWYG
    /// with the interactive client-side rotation).
    pub fit: String,
    /// "rect" or "disc" — see `ElevationParams`.
    pub region: String,
    /// Disc side in degrees; 0 = bbox diagonal.
    pub span_deg: f64,
}

fn d_zero() -> f64 {
    0.0
}
fn d_false() -> bool {
    false
}
fn d_annotation_label_size() -> f64 {
    20.0
}
fn d_annotation_dot() -> f64 {
    8.0
}

impl Default for RenderParams {
    fn default() -> Self {
        RenderParams {
            bbox: [
                DEFAULT_BBOX.lon0,
                DEFAULT_BBOX.lat0,
                DEFAULT_BBOX.lon1,
                DEFAULT_BBOX.lat1,
            ],
            num_lines: 80,
            elevation_pts: 300,
            viewpoint_angle: 0.0,
            crop: false,
            interpolation: 0,
            lock_resolution: false,
            water_ntile: 10.0,
            lake_flatness: 3,
            vertical_ratio: 40.0,
            linewidth_pt: 2.0,
            line_color: "black".into(),
            kind: "gradient".into(),
            background_color: "#ece8ec".into(),
            size_scale: DEFAULT_SIZE_SCALE,
            label: "The White\nMountains".into(),
            label_color: None,
            label_x: 0.62,
            label_y: 0.15,
            label_size_pt: 60.0,
            label_font: "Cinzel".into(),
            annotation: None,
            fit: "reshape".into(),
            region: "rect".into(),
            span_deg: 0.0,
        }
    }
}

impl RenderParams {
    fn bbox_struct(&self) -> Bbox {
        Bbox::new(self.bbox[0], self.bbox[1], self.bbox[2], self.bbox[3])
    }

    fn kind_enum(&self) -> Result<ColorKind, ApiError> {
        match self.kind.as_str() {
            "gradient" => Ok(ColorKind::Gradient),
            "elevation" => Ok(ColorKind::Elevation),
            other => Err(ApiError::bad_request(format!(
                "kind must be gradient|elevation, got {other:?}"
            ))),
        }
    }

    fn is_disc(&self) -> bool {
        self.region == "disc"
    }

    /// Underlying region side in degrees (explicit span or bbox diagonal).
    fn span(&self) -> f64 {
        if self.span_deg > 0.0 {
            return self.span_deg;
        }
        let b = self.bbox_struct();
        (b.lon1 - b.lon0).hypot(b.lat1 - b.lat0)
    }

    /// Underlying square grid size, density-matched to the original
    /// horizontal sampling. Mirrors `ElevationParams::underlying_n`.
    /// Rect mode caps the grid at the display window's own diagonal node
    /// count — more nodes could never be displayed, so a stale or
    /// malicious span cannot blow up the response.
    fn underlying_n(&self) -> usize {
        let b = self.bbox_struct();
        let dlon = (b.lon1 - b.lon0).abs();
        let step = if dlon > 0.0 {
            dlon / self.elevation_pts as f64
        } else {
            self.span()
        };
        let n = (self.span() / step).ceil() as usize;
        let diag_cells = ((self.num_lines.pow(2) + self.elevation_pts.pow(2)) as f64)
            .sqrt()
            .ceil() as usize;
        let cap = if self.is_disc() { 4000 } else { diag_cells + 2 };
        n.clamp(self.num_lines.max(self.elevation_pts).min(cap), cap.max(1))
    }

    /// Display window cropped from the rotated underlying grid.
    fn window(&self, n: usize) -> (usize, usize, usize, usize) {
        if self.is_disc() {
            return (0, 0, n, n);
        }
        let rows = self.num_lines.min(n);
        let cols = self.elevation_pts.min(n);
        ((n - rows) / 2, (n - cols) / 2, rows, cols)
    }

    fn fit_enum(&self) -> Result<ridge_core::geometry::Fit, ApiError> {
        match self.fit.as_str() {
            "reshape" => Ok(ridge_core::geometry::Fit::Reshape),
            "plane" => Ok(ridge_core::geometry::Fit::Plane),
            other => Err(ApiError::bad_request(format!(
                "fit must be reshape|plane, got {other:?}"
            ))),
        }
    }

    fn line_color(&self) -> Result<LineColor, ApiError> {
        LineColor::parse(&self.line_color).ok_or_else(|| {
            ApiError::bad_request(format!("unknown line_color {:?}", self.line_color))
        })
    }

    fn background(&self) -> Result<[u8; 3], ApiError> {
        hex_checked(&self.background_color).ok_or_else(|| {
            ApiError::bad_request(format!(
                "background_color must be hex, got {:?}",
                self.background_color
            ))
        })
    }

    fn validate(&self) -> Result<(), ApiError> {
        if !(1..=2000).contains(&self.num_lines) {
            return Err(ApiError::bad_request("num_lines must be in 1..=2000"));
        }
        if !(1..=4000).contains(&self.elevation_pts) {
            return Err(ApiError::bad_request("elevation_pts must be in 1..=4000"));
        }
        if self.interpolation > 1 {
            return Err(ApiError::bad_request("interpolation must be 0 or 1"));
        }
        if !(0.0..=360.0).contains(&self.viewpoint_angle) {
            return Err(ApiError::bad_request("viewpoint_angle must be in 0..=360"));
        }
        if !self.bbox_struct().is_valid() {
            return Err(ApiError::bad_request(
                "bbox must be [lon0, lat0, lon1, lon2...] with lon1 > lon0, lat1 > lat0, |lat| <= 60",
            ));
        }
        Ok(())
    }

    /// Cache key over the data-shaping parameters. For the plane fit the
    /// angle is excluded: masks are decided in disc space (angle-independent)
    /// and only the cheap rotate+crop runs per request.
    fn data_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in self.bbox {
            v.to_bits().hash(&mut h);
        }
        self.num_lines.hash(&mut h);
        self.elevation_pts.hash(&mut h);
        if self.fit == "plane" {
            self.fit.hash(&mut h);
            self.region.hash(&mut h);
            self.span_deg.to_bits().hash(&mut h);
            return h.finish();
        }
        self.viewpoint_angle.to_bits().hash(&mut h);
        self.crop.hash(&mut h);
        self.interpolation.hash(&mut h);
        self.lock_resolution.hash(&mut h);
        self.fit.hash(&mut h);
        self.region.hash(&mut h);
        self.span_deg.to_bits().hash(&mut h);
        h.finish()
    }
}

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn internal(message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

/// Resolve the full plot style from params + scene (label colors can depend
/// on the colormap).
fn resolve_style(
    params: &RenderParams,
    scene: &ridge_core::RidgeScene,
) -> Result<PlotStyle, ApiError> {
    let line = params.line_color()?;
    let kind = params.kind_enum()?;
    let background = params.background()?;

    let label_color = match &params.label_color {
        Some(name) => {
            let lc = LineColor::parse(name)
                .ok_or_else(|| ApiError::bad_request(format!("unknown label_color {name:?}")))?;
            match lc {
                LineColor::Solid(rgb) => rgb,
                LineColor::Map(cm) => cm.at(0.0),
            }
        }
        None => scene.label_color(&line),
    };

    let annotation = match &params.annotation {
        Some(a) => {
            let bbox = params.bbox_struct();
            let (lon0, lon1) = bbox.longs();
            let (lat0, lat1) = bbox.lats();
            let color = match &a.color {
                Some(name) => {
                    let c = LineColor::parse(name)
                        .ok_or_else(|| ApiError::bad_request(format!("unknown color {name:?}")))?;
                    match c {
                        LineColor::Solid(rgb) => rgb,
                        LineColor::Map(cm) => cm.at(0.0),
                    }
                }
                None => label_color,
            };
            Some(Annotation {
                label: a.label.clone(),
                x: (a.lon - lon0) / (lon1 - lon0),
                y: (a.lat - lat0) / (lat1 - lat0),
                x_offset: a.x_offset,
                y_offset: a.y_offset,
                label_size_pt: a.label_size_pt,
                dot_pt: a.dot_pt,
                color,
                background: a.background,
            })
        }
        None => None,
    };

    Ok(PlotStyle {
        line: LineColorSpec::from(&line),
        kind,
        background,
        linewidth_pt: params.linewidth_pt,
        size_scale: params.size_scale,
        label: if params.label.is_empty() {
            None
        } else {
            Some(LabelStyle {
                text: params.label.clone(),
                color: label_color,
                x: params.label_x,
                y: params.label_y,
                size_pt: params.label_size_pt,
                vertical_alignment: VAlign::Bottom,
                font_family: params.label_font.clone(),
                background: true,
            })
        },
        annotation,
    })
}

fn preprocess_and_scene(
    params: &RenderParams,
    grid: &ndarray::Array2<f64>,
) -> Result<ridge_core::RidgeScene, ApiError> {
    let processed = ridge_core::preprocess::preprocess(
        grid,
        params.water_ntile,
        params.lake_flatness,
        params.vertical_ratio,
        1.0,  // reshape path: upstream-faithful naive threshold
        None, // stats over the whole window
    )
    .map_err(|e| ApiError::bad_request(format!("preprocess failed: {e}")))?;
    // Figure ratio = the plotted window's degree aspect: disc = 1:1,
    // rect = its rows:cols (square cells), reshape = bbox aspect.
    let ratio = if params.is_disc() {
        1.0
    } else if params.fit == "plane" {
        params.num_lines as f64 / params.elevation_pts as f64
    } else {
        params.bbox_struct().ratio()
    };
    Ok(ridge_core::RidgeScene::from_grid(
        &processed,
        ratio,
        params.size_scale,
    ))
}

/// Compute (or fetch from cache) the rotated grid and build the scene for
/// the requested fit. `Fit::Plane` samples without the upstream axis swap
/// and rotates within the fixed canvas — identical to the browser's local
/// pipeline, so exports match what the user sees.
async fn render_scene(
    state: &AppState,
    params: &RenderParams,
) -> Result<ridge_core::RidgeScene, ApiError> {
    params.validate()?;
    let fit = params.fit_enum()?;

    let rotating = params.viewpoint_angle.rem_euclid(360.0) != 0.0;
    let grid: Arc<ndarray::Array2<f64>> = if fit == ridge_core::geometry::Fit::Plane {
        // Interactive path. The CACHED artifact is the preprocessed disc
        // grid: all threshold decisions (water percentile, lake flatness)
        // were made in disc space and are angle-independent, so the cache
        // key excludes the angle. Per request we only rotate the pre-decided
        // grid (by -angle: the preprocess flip reverses the apparent
        // rotation direction) and crop the display window.
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            "preprocessed-disc".hash(&mut h);
            for v in params.bbox {
                v.to_bits().hash(&mut h);
            }
            params.num_lines.hash(&mut h);
            params.elevation_pts.hash(&mut h);
            params.region.hash(&mut h);
            params.span_deg.to_bits().hash(&mut h);
            params.water_ntile.to_bits().hash(&mut h);
            params.lake_flatness.hash(&mut h);
            params.vertical_ratio.to_bits().hash(&mut h);
            params.size_scale.to_bits().hash(&mut h);
            h.finish()
        };
        let processed = match state.grid_cache.get(key) {
            Some(g) => g,
            None => {
                let source = state.source.clone();
                let p = params.clone();
                let g = tokio::task::spawn_blocking(move || {
                    let bbox = p.bbox_struct();
                    let n = p.underlying_n();
                    let span = p.span();
                    let raw = ridge_core::grid::sample_disc(
                        source.as_ref(),
                        (bbox.lat0 + bbox.lat1) / 2.0,
                        (bbox.lon0 + bbox.lon1) / 2.0,
                        span,
                        n,
                    );
                    // Density compensation: the lake-flatness threshold is
                    // expressed at the DISPLAY sampling (dlat/num_lines),
                    // so the disc-space decision measures the same terrain
                    // slope and matches upstream's composition.
                    let d_step = p.span() / p.underlying_n() as f64;
                    let ref_step = (bbox.lat1 - bbox.lat0).abs() / p.num_lines as f64;
                    // Statistics (normalization + water percentile) over the
                    // original bbox footprint when viewing through the rect
                    // window: matches upstream's window-scoped percentile,
                    // and is a fixed physical region, so it stays
                    // rotation-stable.
                    let stats_region: Option<Vec<bool>> = if p.is_disc() {
                        None
                    } else {
                        let c_lat = (bbox.lat0 + bbox.lat1) / 2.0;
                        let c_lon = (bbox.lon0 + bbox.lon1) / 2.0;
                        let mut m = vec![false; raw.len()];
                        for r in 0..n {
                            let lat = c_lat - span / 2.0 + r as f64 / n as f64 * span;
                            for c in 0..n {
                                let lon = c_lon - span / 2.0 + c as f64 / n as f64 * span;
                                if lat >= bbox.lat0
                                    && lat <= bbox.lat1
                                    && lon >= bbox.lon0
                                    && lon <= bbox.lon1
                                {
                                    m[r * n + c] = true;
                                }
                            }
                        }
                        Some(m)
                    };
                    ridge_core::preprocess::preprocess(
                        &raw,
                        p.water_ntile,
                        p.lake_flatness,
                        p.vertical_ratio,
                        d_step / ref_step,
                        stats_region.as_deref(),
                    )
                    .map_err(|e| format!("preprocess failed: {e}"))
                })
                .await
                .map_err(|e| ApiError::internal(format!("join error: {e}")))?
                .map_err(ApiError::internal)?;
                if g.iter().all(|v| v.is_nan()) {
                    return Err(ApiError::bad_request(
                        "no elevation data for this bbox (open ocean, or |lat| > 60?)",
                    ));
                }
                let g = Arc::new(g);
                state.grid_cache.insert(key, g.clone());
                g
            }
        };
        let rotated = if rotating {
            ridge_core::rotate::rotate_fixed_plane(
                &processed,
                -params.viewpoint_angle,
                0, // nearest: NaN holes must rotate rigidly, not smear
            )
        } else {
            (*processed).clone()
        };
        if params.is_disc() {
            let n = params.underlying_n();
            let (r0, c0, rows, cols) = params.window(n);
            Arc::new(
                rotated
                    .slice(ndarray::s![r0..r0 + rows, c0..c0 + cols])
                    .to_owned(),
            )
        } else {
            // Rectangle view: an anisotropic window covering the ORIGINAL
            // bbox extent at num_lines x elevation_pts -- upstream's
            // composition, sweeping over the disc.
            let bbox = params.bbox_struct();
            Arc::new(ridge_core::grid::sample_window(
                &rotated,
                bbox.lat0 - params.span() / 2.0,
                bbox.lon0 - params.span() / 2.0,
                params.span(),
                bbox.lat0,
                bbox.lon0,
                bbox.lat1,
                bbox.lon1,
                params.num_lines,
                params.elevation_pts,
            ))
        }
    } else {
        // Upstream-faithful reshape path: rotate the raw grid first
        // (scipy order), then let the caller preprocess per window.
        let source = state.source.clone();
        let p = params.clone();
        let key = params.data_key();
        let grid = match state.grid_cache.get(key) {
            Some(g) => g,
            None => {
                let grid =
                    tokio::task::spawn_blocking(move || -> Result<ndarray::Array2<f64>, String> {
                        let bbox = p.bbox_struct();
                        let (mut lines, mut pts) = (p.num_lines, p.elevation_pts);
                        if !p.lock_resolution && ridge_core::grid::swap_for_angle(p.viewpoint_angle)
                        {
                            std::mem::swap(&mut lines, &mut pts);
                        }
                        let mut values =
                            ridge_core::grid::sample(source.as_ref(), &bbox, lines, pts);
                        if p.viewpoint_angle.rem_euclid(360.0) != 0.0 {
                            values = ridge_core::rotate::rotate(
                                &values,
                                p.viewpoint_angle,
                                !p.crop,
                                p.interpolation,
                            );
                        }
                        Ok(values)
                    })
                    .await
                    .map_err(|e| ApiError::internal(format!("join error: {e}")))?
                    .map_err(ApiError::internal)?;
                let grid = Arc::new(grid);
                state.grid_cache.insert(key, grid.clone());
                grid
            }
        };
        grid
    };

    if grid.iter().all(|v| v.is_nan()) {
        return Err(ApiError::bad_request(
            "no elevation data for this bbox (open ocean, or |lat| > 60?)",
        ));
    }
    // The plane path already preprocessed in disc space; reshape still needs
    // its per-window preprocess here.
    if fit == ridge_core::geometry::Fit::Plane {
        let scene = ridge_core::RidgeScene::from_grid(
            &grid,
            if params.is_disc() {
                1.0
            } else {
                params.num_lines as f64 / params.elevation_pts as f64
            },
            params.size_scale,
        );
        Ok(scene)
    } else {
        preprocess_and_scene(params, &grid)
    }
}

/// `POST /api/preview` — geometry + resolved style for the canvas renderer.
pub async fn preview(
    State(state): State<AppState>,
    Json(params): Json<RenderParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let scene = render_scene(&state, &params).await?;
    let style = resolve_style(&params, &scene)?;

    let payload = serde_json::json!({
        "shape": [scene.rows.len(), scene.n_points],
        "rows": scene.rows,
        "vmin": scene.vmin,
        "vmax": scene.vmax,
        "layout": scene.layout,
        "style": style,
        "num_lines_effective": scene.rows.len(),
        "elevation_pts_effective": scene.n_points,
    });
    Ok(Json(payload))
}

/// `POST /api/export.svg` — standalone vector artwork.
pub async fn export_svg(
    State(state): State<AppState>,
    Json(params): Json<RenderParams>,
) -> Result<Response, ApiError> {
    let scene = render_scene(&state, &params).await?;
    let style = resolve_style(&params, &scene)?;
    let svg = render_svg(&scene, &style);
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "image/svg+xml; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"ridge-map.svg\"".to_string(),
            ),
        ],
        svg,
    )
        .into_response())
}

/// `POST /api/elevation` — the raw sampled grid (no rotation, no
/// preprocessing). This is the only server round-trip the interactive
/// frontend needs: rotation and water/lake masking run client-side, so the
/// viewpoint angle never triggers a refetch. The grid is cached purely on
/// (bbox, num_lines, elevation_pts) — angle-independent.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct ElevationParams {
    pub bbox: [f64; 4],
    pub num_lines: usize,
    pub elevation_pts: usize,
    /// "rect" (sample the bbox) or "disc" (square region around the bbox
    /// center, masked to the inscribed circle — rotation-invariant).
    pub region: String,
    /// Side of the square region in degrees (disc mode). 0 = derive from the
    /// bbox diagonal, so the whole rectangle is inside the circle.
    pub span_deg: f64,
}

impl Default for ElevationParams {
    fn default() -> Self {
        let d = RenderParams::default();
        ElevationParams {
            bbox: d.bbox,
            num_lines: d.num_lines,
            elevation_pts: d.elevation_pts,
            region: d.region,
            span_deg: d.span_deg,
        }
    }
}

impl ElevationParams {
    fn cache_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in self.bbox {
            v.to_bits().hash(&mut h);
        }
        self.num_lines.hash(&mut h);
        self.elevation_pts.hash(&mut h);
        self.region.hash(&mut h);
        self.span_deg.to_bits().hash(&mut h);
        h.finish()
    }

    fn bbox_struct(&self) -> Bbox {
        Bbox::new(self.bbox[0], self.bbox[1], self.bbox[2], self.bbox[3])
    }

    fn is_disc(&self) -> bool {
        self.region == "disc"
    }

    /// Underlying region side in degrees: explicit span, or the bbox
    /// diagonal so the whole rectangle fits inside the circle at any angle.
    fn span(&self) -> f64 {
        if self.span_deg > 0.0 {
            return self.span_deg;
        }
        let b = self.bbox_struct();
        (b.lon1 - b.lon0).hypot(b.lat1 - b.lat0)
    }

    /// Size of the underlying square grid. Density-matched to the original
    /// horizontal sampling (`elevation_pts` across the bbox width), so the
    /// rotated rectangle view keeps the same detail as the static one.
    /// Rect mode caps the grid at the display window's own diagonal node
    /// count — more nodes could never be displayed, so a stale or
    /// malicious span cannot blow up the response.
    fn underlying_n(&self) -> usize {
        let b = self.bbox_struct();
        let dlon = (b.lon1 - b.lon0).abs();
        let step = if dlon > 0.0 {
            dlon / self.elevation_pts as f64
        } else {
            self.span()
        };
        let n = (self.span() / step).ceil() as usize;
        let diag_cells = ((self.num_lines.pow(2) + self.elevation_pts.pow(2)) as f64)
            .sqrt()
            .ceil() as usize;
        let cap = if self.is_disc() { 4000 } else { diag_cells + 2 };
        n.clamp(self.num_lines.max(self.elevation_pts).min(cap), cap.max(1))
    }

    /// The display window cropped from the rotated underlying grid.
    /// Rect: `num_lines x elevation_pts` centered (the rotating frame).
    /// Disc: the whole grid.
    fn window(&self, n: usize) -> (usize, usize, usize, usize) {
        if self.is_disc() {
            return (0, 0, n, n);
        }
        let rows = self.num_lines.min(n);
        let cols = self.elevation_pts.min(n);
        ((n - rows) / 2, (n - cols) / 2, rows, cols)
    }
}

pub async fn elevation(
    State(state): State<AppState>,
    Json(params): Json<ElevationParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !(1..=2000).contains(&params.num_lines) {
        return Err(ApiError::bad_request("num_lines must be in 1..=2000"));
    }
    if !(1..=4000).contains(&params.elevation_pts) {
        return Err(ApiError::bad_request("elevation_pts must be in 1..=4000"));
    }
    let bbox = params.bbox_struct();
    if !bbox.is_valid() {
        return Err(ApiError::bad_request("invalid bbox"));
    }
    let key = params.cache_key();
    if let Some(grid) = state.grid_cache.get(key) {
        return Ok(Json(elevation_response(&grid, &params)));
    }
    let source = state.source.clone();
    let n = params.underlying_n();
    let span = params.span();
    let grid = tokio::task::spawn_blocking(move || {
        ridge_core::grid::sample_disc(
            source.as_ref(),
            (bbox.lat0 + bbox.lat1) / 2.0,
            (bbox.lon0 + bbox.lon1) / 2.0,
            span,
            n,
        )
    })
    .await
    .map_err(|e| ApiError::internal(format!("join error: {e}")))?;

    if grid.iter().all(|v| v.is_nan()) {
        return Err(ApiError::bad_request(
            "no elevation data for this bbox (open ocean, or |lat| > 60?)",
        ));
    }
    let grid = Arc::new(grid);
    state.grid_cache.insert(key, grid.clone());
    Ok(Json(elevation_response(&grid, &params)))
}

/// Elevation payload + the display-window spec. The window MUST be present
/// on cache hits too — the frontend crops the disc grid by it.
fn elevation_response(grid: &ndarray::Array2<f64>, params: &ElevationParams) -> serde_json::Value {
    let n = params.underlying_n();
    let (row0, col0, rows, cols) = params.window(n);
    let mut payload = elevation_payload(grid);
    payload["window"] = serde_json::json!({
        "row0": row0, "col0": col0, "rows": rows, "cols": cols,
    });
    payload
}

/// The grid as integers (SRTM samples are whole meters); voids -> null.
fn elevation_payload(grid: &ndarray::Array2<f64>) -> serde_json::Value {
    let values: Vec<Vec<Option<i64>>> = grid
        .rows()
        .into_iter()
        .map(|row| {
            row.iter()
                .map(|v| if v.is_finite() { Some(*v as i64) } else { None })
                .collect()
        })
        .collect();
    serde_json::json!({
        "shape": [grid.nrows(), grid.ncols()],
        "values": values,
    })
}

/// `GET /api/presets` — curated bboxes from the upstream README.
pub async fn presets() -> Json<serde_json::Value> {
    let presets = serde_json::json!([
        {"name": "The White Mountains", "bbox": [-71.928864, 43.758201, -70.957947, 44.465151],
         "params": {"label": "The White\nMountains", "label_x": 0.62, "label_y": 0.15}},
        {"name": "Karwendelgebirge", "bbox": [11.098251, 47.264786, 11.695633, 47.453630],
         "params": {"num_lines": 150, "label": "Karwendelgebirge", "label_y": 0.1, "label_x": 0.55,
                     "label_size_pt": 40, "lake_flatness": 2, "vertical_ratio": 240, "linewidth_pt": 1}},
        {"name": "Austin", "bbox": [-97.794285, 30.232226, -97.710171, 30.334509],
         "params": {"num_lines": 80, "label": "Austin\nTexas", "label_x": 0.75, "linewidth_pt": 6,
                     "line_color": "orange", "water_ntile": 12}},
        {"name": "San Francisco Bay", "bbox": [-123.107300, 36.820279, -121.519775, 38.210130],
         "params": {"num_lines": 150, "label": "The Bay\nArea", "label_x": 0.1,
                     "line_color": "spring", "lake_flatness": 3, "water_ntile": 50, "vertical_ratio": 30}},
        {"name": "Hawai'i", "bbox": [-156.250305, 18.890695, -154.714966, 20.275080],
         "params": {"num_lines": 100, "label": "Hawai'i", "label_y": 0.85, "label_x": 0.7,
                     "label_size_pt": 60, "lake_flatness": 2, "water_ntile": 10, "vertical_ratio": 240,
                     "line_color": "ocean", "kind": "elevation"}},
        {"name": "Kent, Connecticut", "bbox": [-73.509693, 41.678682, -73.342838, 41.761581],
         "params": {"label": "Kent\nConnecticut", "label_y": 0.7, "label_x": 0.65, "label_size_pt": 40,
                     "lake_flatness": 2, "water_ntile": 2, "vertical_ratio": 60}},
        {"name": "Cambridge & Boston", "bbox": [-71.167374, 42.324286, -70.952454, 42.402672],
         "params": {"num_lines": 50, "label": "Cambridge\nand Boston", "label_x": 0.75, "label_size_pt": 40,
                     "lake_flatness": 4, "water_ntile": 30, "vertical_ratio": 20, "linewidth_pt": 1}},
        {"name": "Concord, Massachusetts", "bbox": [-71.418858, 42.427511, -71.310024, 42.481719],
         "params": {"num_lines": 100, "label": "Concord\nMassachusetts", "label_x": 0.1, "label_size_pt": 30,
                     "water_ntile": 15, "vertical_ratio": 30}},
        {"name": "Washington State", "bbox": [-124.848974, 46.292035, -116.463262, 49.345786],
         "params": {"elevation_pts": 300, "num_lines": 300, "viewpoint_angle": 11, "label": "Washington",
                     "label_y": 0.8, "label_x": 0.05, "label_size_pt": 40, "lake_flatness": 2,
                     "water_ntile": 10, "vertical_ratio": 240, "linewidth_pt": 2}},
        {"name": "Santa Cruz Mountains", "bbox": [-122.087116, 36.945365, -121.999226, 37.023250],
         "params": {"num_lines": 150, "label": "Santa Cruz\nMountains", "label_x": 0.75, "label_y": 0.05,
                     "label_size_pt": 36, "lake_flatness": 1, "water_ntile": 0, "vertical_ratio": 240,
                     "kind": "elevation", "line_color": "cool", "background_color": "#414a4c"}},
    ]);
    Json(presets)
}

/// The project README, compiled in so the app can show it without the repo.
const README: &str = include_str!(env!("RIDGE_README"));

/// `GET /api/readme` — the README as plain text, for the in-app modal.
pub async fn readme() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        README,
    )
}

pub async fn healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (hits, misses) = state.grid_cache.stats();
    Json(serde_json::json!({ "ok": true, "grid_cache": { "hits": hits, "misses": misses } }))
}
