//! ridge_redux: HTTP API + static frontend for interactive ridgeline art.
//!
//! The backend owns the data pipeline (SRTM fetch -> sample -> rotate ->
//! preprocess) and exposes it as JSON geometry; the frontend is a thin
//! canvas renderer. `POST /api/preview` returns geometry + resolved style;
//! `POST /api/export.svg` returns a standalone vector SVG.

pub mod api;
mod frontend;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Where to listen. None: 127.0.0.1:8420, or any free port if that's taken.
    pub addr: Option<SocketAddr>,
    /// Serve the frontend from this directory instead of the embedded copy.
    pub web_dir: Option<PathBuf>,
    pub srtm_base: String,
    pub cache_dir: PathBuf,
    /// Serve tiles from this dir instead of the network (offline/demo mode).
    pub fixture_dir: Option<PathBuf>,
    /// Open the app in the default browser once it's listening.
    pub open_browser: bool,
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
        let mut addr = None;
        let mut web_dir = None;
        let mut srtm_base =
            "https://srtm.kurviger.de/SRTM1/,https://srtm.kurviger.de/SRTM3/".to_string();
        let mut cache_dir = default_cache_dir();
        let mut fixture_dir = None;
        let mut open_browser = true;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut val = || args.next().expect("missing value");
            match arg.as_str() {
                "--addr" => addr = Some(val().parse().expect("bad --addr")),
                "--web-dir" => web_dir = Some(PathBuf::from(val())),
                "--srtm-base" => srtm_base = val(),
                "--cache-dir" => cache_dir = PathBuf::from(val()),
                "--fixture-dir" => fixture_dir = Some(PathBuf::from(val())),
                "--no-open" => open_browser = false,
                "--help" => {
                    println!(
                        "ridge_redux [--addr IP:PORT] [--no-open] [--web-dir DIR] [--srtm-base URL] \
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
            open_browser,
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

    if let Some(dir) = &config.web_dir {
        tracing::info!("serving the frontend from {}", dir.display());
    }
    let listener = match config.addr {
        Some(addr) => tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| {
                eprintln!("ridge_redux: can't listen on {addr}: {e}");
                std::process::exit(1);
            }),
        None => bind_preferring(DEFAULT_ADDR)
            .await
            .expect("no free port on 127.0.0.1"),
    };
    let url = format!("http://{}", listener.local_addr().expect("bound address"));
    println!("ridge_redux running at {url}");
    if config.open_browser {
        if let Err(e) = open::that_detached(&url) {
            tracing::warn!("couldn't open a browser ({e}); open {url} yourself");
        }
    }
    axum::serve(listener, app).await.expect("server error");
}

const DEFAULT_ADDR: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8420);

/// Listen on `preferred`, or on a free port of the same host if it's taken
/// (another ridge_redux, say), rather than failing to start.
async fn bind_preferring(preferred: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind(preferred).await {
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::info!("port {} is in use; using a free one", preferred.port());
            tokio::net::TcpListener::bind(SocketAddr::new(preferred.ip(), 0)).await
        }
        result => result,
    }
}

/// Build the full app router (separated for integration tests).
pub fn build_router(state: state::AppState, config: &ServerConfig) -> Router {
    let api = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/api/presets", get(api::presets))
        .route("/api/readme", get(api::readme))
        .route("/api/preview", post(api::preview))
        .route("/api/elevation", post(api::elevation))
        .route("/api/export.svg", post(api::export_svg));
    let app = match &config.web_dir {
        Some(dir) => api.fallback_service(
            tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true),
        ),
        None => api.fallback(frontend::serve),
    };
    app.layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn busy_port_falls_back_to_a_free_one() {
        let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = taken.local_addr().unwrap();
        let listener = bind_preferring(busy).await.unwrap();
        let got = listener.local_addr().unwrap();
        assert_eq!(got.ip(), busy.ip());
        assert_ne!(got.port(), busy.port());
    }

    #[tokio::test]
    async fn free_port_is_used_as_asked() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let free = probe.local_addr().unwrap();
        drop(probe);
        let listener = bind_preferring(free).await.unwrap();
        assert_eq!(listener.local_addr().unwrap(), free);
    }
}
