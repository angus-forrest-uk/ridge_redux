//! ridge_redux: HTTP API + static frontend for interactive ridgeline art.
//!
//! The backend owns the data pipeline (SRTM fetch -> sample -> rotate ->
//! preprocess) and exposes it as JSON geometry; the frontend is a thin
//! canvas renderer. `POST /api/preview` returns geometry + resolved style;
//! `POST /api/export.svg` returns a standalone vector SVG.

pub mod api;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub web_dir: PathBuf,
    pub srtm_base: String,
    pub cache_dir: PathBuf,
    /// Serve tiles from this dir instead of the network (offline/demo mode).
    pub fixture_dir: Option<PathBuf>,
}

fn default_cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".cache")
        })
        .join("ridge-redux")
        .join("srtm")
}

impl ServerConfig {
    pub fn from_env_and_args() -> ServerConfig {
        let mut addr = SocketAddr::from(([127, 0, 0, 1], 8420));
        let mut web_dir = PathBuf::from("web/dist");
        let mut srtm_base =
            "https://srtm.kurviger.de/SRTM1/,https://srtm.kurviger.de/SRTM3/".to_string();
        let mut cache_dir = default_cache_dir();
        let mut fixture_dir = None;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut val = || args.next().expect("missing value");
            match arg.as_str() {
                "--addr" => addr = val().parse().expect("bad --addr"),
                "--web-dir" => web_dir = PathBuf::from(val()),
                "--srtm-base" => srtm_base = val(),
                "--cache-dir" => cache_dir = PathBuf::from(val()),
                "--fixture-dir" => fixture_dir = Some(PathBuf::from(val())),
                "--help" => {
                    println!(
                        "ridge_redux [--addr IP:PORT] [--web-dir DIR] [--srtm-base URL] \
                         [--cache-dir DIR] [--fixture-dir DIR]"
                    );
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown argument {other:?}");
                    std::process::exit(2);
                }
            }
        }
        ServerConfig {
            addr,
            web_dir,
            srtm_base,
            cache_dir,
            fixture_dir,
        }
    }
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ridge_redux=info,tower_http=info".into()),
        )
        .init();

    let config = ServerConfig::from_env_and_args();
    let state = state::AppState::new(&config);

    let app = build_router(state, &config);

    let cfg = config.clone();
    tracing::info!(
        "ridge_redux listening on http://{} (web dir: {})",
        cfg.addr,
        cfg.web_dir.display()
    );

    let listener = tokio::net::TcpListener::bind(cfg.addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");
}

/// Build the full app router (separated for integration tests).
pub fn build_router(state: state::AppState, config: &ServerConfig) -> Router {
    Router::new()
        .route("/healthz", get(api::healthz))
        .route("/api/presets", get(api::presets))
        .route("/api/readme", get(api::readme))
        .route("/api/preview", post(api::preview))
        .route("/api/elevation", post(api::elevation))
        .route("/api/export.svg", post(api::export_svg))
        .fallback_service(
            tower_http::services::ServeDir::new(&config.web_dir)
                .append_index_html_on_directories(true),
        )
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
