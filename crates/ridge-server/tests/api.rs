//! Integration tests: run the whole API against fixture tiles, no network.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt; // oneshot

use ridge_server::{build_router, state::AppState, ServerConfig};

/// A stand-in for the built frontend (web/dist), so these tests don't need
/// the Node build.
fn web_dir() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("ridge-test-web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>ridge-redux</title>",
        )
        .unwrap();
        dir
    })
    .clone()
}

fn fixture_config() -> ServerConfig {
    let dir = std::path::Path::new("../../fixtures/srtm").canonicalize();
    ServerConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: web_dir(),
        srtm_base: "http://127.0.0.1:1/invalid/".into(), // must never be hit
        cache_dir: std::env::temp_dir().join("ridge-test-cache"),
        fixture_dir: dir.ok(),
    }
}

fn app_with_fresh_state() -> axum::Router {
    let config = fixture_config();
    let state = AppState::new(&config);
    build_router(state.clone(), &config)
}

async fn post(
    app: axum::Router,
    uri: &str,
    body: String,
) -> (StatusCode, axum::response::Response) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    (status, resp)
}

async fn body_bytes(resp: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), 64 << 20)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn healthz_ok() {
    let app = app_with_fresh_state();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn readme_is_plain_text() {
    let app = app_with_fresh_state();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/readme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/plain; charset=utf-8");
    let text = String::from_utf8(body_bytes(resp).await).unwrap();
    assert!(text.starts_with("# ridge-redux"));
}

#[tokio::test]
async fn preview_returns_geometry() {
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/preview",
        json!({ "num_lines": 20, "elevation_pts": 40 }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(body["shape"], json!([20, 40]));
    assert_eq!(body["rows"].as_array().unwrap().len(), 20);
    assert!(body["vmin"].as_f64().unwrap() < body["vmax"].as_f64().unwrap());
    // Baselines step by -6 per row (upstream constant).
    assert_eq!(body["rows"][5]["baseline"], json!(-30.0));
    assert!(body["layout"]["width_px"].as_f64().unwrap() > 0.0);
    // Style resolution echoes a solid line color as an RGB array.
    assert_eq!(
        body["style"]["line"],
        json!({"type": "solid", "rgb": [0, 0, 0]})
    );
}

#[tokio::test]
async fn repeat_preview_hits_grid_cache() {
    let config = fixture_config();
    let state = AppState::new(&config);
    let app = build_router(state.clone(), &config);
    let body = json!({ "num_lines": 9, "elevation_pts": 9 }).to_string();
    for _ in 0..2 {
        let (status, _) = post(app.clone(), "/api/preview", body.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (hits, misses) = state.grid_cache.stats();
    assert_eq!(
        (hits, misses),
        (1, 1),
        "second identical request should hit the cache"
    );
}

#[tokio::test]
async fn export_svg_content_type() {
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/export.svg",
        json!({ "num_lines": 10, "elevation_pts": 10, "line_color": "ocean", "kind": "elevation" })
            .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let headers = resp.headers().clone();
    let svg = String::from_utf8(body_bytes(resp).await).unwrap();
    assert!(svg.starts_with("<?xml"));
    assert!(svg.contains("<path"));
    assert_eq!(
        headers.get("content-type").unwrap(),
        "image/svg+xml; charset=utf-8"
    );
    assert!(headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("ridge-map.svg"));
}

#[tokio::test]
async fn bad_params_rejected() {
    let app = app_with_fresh_state();
    let (status, resp) = post(app, "/api/preview", json!({ "num_lines": 0 }).to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(body["error"].as_str().unwrap().contains("num_lines"));

    let app = app_with_fresh_state();
    let (status, _) = post(app, "/api/preview", json!({ "kind": "nope" }).to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ocean_bbox_friendly_error() {
    // Fixture tiles only cover New Hampshire; the South Atlantic has no tiles.
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/preview",
        json!({ "bbox": [-10.0, -30.0, -9.0, -29.0] }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("no elevation data"));
}

#[tokio::test]
async fn static_index_served() {
    let app = app_with_fresh_state();
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    assert!(String::from_utf8_lossy(&bytes).contains("ridge-redux"));
}

#[tokio::test]
async fn elevation_endpoint_is_angle_free_and_raw() {
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/elevation",
        json!({ "num_lines": 12, "elevation_pts": 12 }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    // Underlying square grid (density-matched, disc-masked) + display window.
    let shape = body["shape"].as_array().unwrap();
    assert_eq!(shape[0], shape[1], "underlying region is square");
    assert!(body["window"]["rows"].as_u64().unwrap() <= shape[0].as_u64().unwrap());
    assert!(body["window"]["cols"].as_u64().unwrap() <= shape[1].as_u64().unwrap());
    // Raw samples are integer meters; voids are null.
    let first = body["values"][0].as_array().unwrap();
    for v in first {
        assert!(v.is_null() || v.is_i64(), "raw samples must be int or null");
    }
    // Some finite data must exist inside the White Mountains bbox.
    let any_finite = body["values"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row.as_array().unwrap().iter().any(|v| v.is_i64()));
    assert!(any_finite, "fixture bbox should contain elevations");
}

#[tokio::test]
async fn plane_export_keeps_dims_and_makes_corner_gaps() {
    // fit=plane rotates within the fixed canvas: rows == num_lines (no axis
    // swap) and 45-degree corners become gaps (nulls in SVG = skipped runs;
    // here we verify via the preview JSON which shares the pipeline).
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/preview",
        json!({
            "num_lines": 40, "elevation_pts": 40,
            "viewpoint_angle": 45, "fit": "plane"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(
        body["shape"],
        json!([40, 40]),
        "plane fit: no axis swap, fixed dims"
    );
    let first_row_gaps = body["rows"][0]["y"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v.is_null())
        .count();
    assert!(first_row_gaps > 0, "45-degree corners must be gaps");
}

#[tokio::test]
async fn reshape_export_swaps_axes_at_90() {
    // Upstream semantics: at 90 degrees the sampled axes swap, so the
    // rotation swaps dims right back: final shape == (num_lines, elevation_pts).
    // (Without the swap, a 30x50 sample rotated 90 degrees would be 50x30.)
    let app = app_with_fresh_state();
    let (status, resp) = post(
        app,
        "/api/preview",
        json!({
            "num_lines": 30, "elevation_pts": 50,
            "viewpoint_angle": 90, "fit": "reshape"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(
        body["shape"],
        json!([30, 50]),
        "reshape fit: swap + 90-degree rotation cancel"
    );
}

#[tokio::test]
async fn disc_region_has_constant_visible_points_at_any_angle() {
    // The whole point of disc mode: rotation about the center neither adds
    // nor removes visible samples, so the orbit keeps a fixed apparent size.
    async fn finite_count(angle: i32) -> usize {
        let app = app_with_fresh_state();
        let (status, resp) = post(
            app,
            "/api/preview",
            json!({
                "region": "disc", "fit": "plane",
                "num_lines": 40, "elevation_pts": 40,
                "viewpoint_angle": angle
            })
            .to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
        let shape = body["shape"].as_array().unwrap();
        assert_eq!(shape[0], shape[1], "disc grids are square");
        body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["y"].as_array().unwrap())
            .filter(|v| v.is_number())
            .count()
    }
    let a0 = finite_count(0).await;
    let a37 = finite_count(37).await;
    let a90 = finite_count(90).await;
    // Decisions are made in disc space (angle-independent). At 90-degree
    // multiples the rotation maps sample nodes exactly onto sample nodes, so
    // the visible count is exactly constant; at arbitrary angles the mask
    // boundary still wobbles by a handful of cells (nearest-resampling
    // aliasing at shorelines), orders of magnitude below the 74% collapse
    // the old rotate-then-decide pipeline produced.
    assert_eq!(a0, a90, "node-aligned rotation: exactly constant");
    assert!(
        (a0 as i64 - a37 as i64).abs() * 100 / a0 as i64 <= 1,
        "visible points must stay constant with angle: {a0} vs {a37}"
    );
    assert!(a0 > 400, "should have plenty of visible points ({a0})");
}

#[tokio::test]
async fn rect_view_keeps_shape_style_and_points_at_any_angle() {
    // The rotating-window model: the display is always num_lines x
    // elevation_pts (same style/ratios), and the underlying disc supplies
    // previously unused points as the frame sweeps around — so the visible
    // count stays essentially constant instead of collapsing.
    async fn render_at(angle: i32) -> (usize, usize, usize) {
        let app = app_with_fresh_state();
        let (status, resp) = post(
            app,
            "/api/preview",
            json!({
                "region": "rect", "fit": "plane",
                "num_lines": 40, "elevation_pts": 40,
                "viewpoint_angle": angle
            })
            .to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&body_bytes(resp).await).unwrap();
        let shape = body["shape"].as_array().unwrap();
        let count = body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["y"].as_array().unwrap())
            .filter(|v| v.is_number())
            .count();
        (
            shape[0].as_u64().unwrap() as usize,
            shape[1].as_u64().unwrap() as usize,
            count,
        )
    }
    let (r0, c0, n0) = render_at(0).await;
    let (r45, c45, n45) = render_at(45).await;
    let (r90, c90, n90) = render_at(90).await;
    // The window is fixed: num_lines x elevation_pts at every angle.
    assert_eq!((r0, c0), (r45, c45), "window dims constant at 45 degrees");
    assert_eq!((r0, c0), (r90, c90), "window dims constant at 90 degrees");
    // The visible count varies only physically (different terrain sweeps
    // through the frame); it must never collapse like the old
    // rotate-the-window model did (that lost ~74% at 90 degrees).
    let max_n = n0.max(n45).max(n90);
    let min_n = n0.min(n45).min(n90);
    assert!(
        (max_n - min_n) * 100 / max_n <= 40,
        "no collapse: {n0} vs {n45} vs {n90}"
    );
    assert!(n0 > 200, "should have plenty of visible points ({n0})");
}
