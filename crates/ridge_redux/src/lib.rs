//! ridge_redux: HTTP API + static frontend for interactive ridgeline art.
//!
//! The backend owns the data pipeline (SRTM fetch -> sample -> rotate ->
//! preprocess) and exposes it as JSON geometry; the frontend is a thin
//! canvas renderer. `POST /api/preview` returns geometry + resolved style;
//! `POST /api/export.svg` returns a standalone vector SVG.

pub mod api;
mod frontend;
pub mod state;

use std::io::{BufRead, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::routing::{get, post};
use axum::Router;
use clap::{Parser, Subcommand};
use ridge_core::srtm;
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

/// Interactive ridgeline maps of real terrain, as a local web app.
#[derive(Parser, Debug)]
#[command(name = "ridge_redux", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Address to listen on [default: 127.0.0.1:8420, or a free port if taken]
    #[arg(long, value_name = "IP:PORT")]
    addr: Option<SocketAddr>,
    /// Don't open the app in the default browser
    #[arg(long)]
    no_open: bool,
    /// Don't load the default scene (the White Mountains) before starting
    #[arg(long)]
    no_prefetch: bool,
    /// Serve the frontend from this directory instead of the embedded copy
    #[arg(long, value_name = "DIR")]
    web_dir: Option<PathBuf>,
    /// Comma-separated SRTM mirror base URLs, tried in order
    #[arg(
        long,
        value_name = "URL",
        default_value = "https://srtm.kurviger.de/SRTM1/,https://srtm.kurviger.de/SRTM3/"
    )]
    srtm_base: String,
    /// Where downloaded tiles are kept [default: ridge-redux/srtm in the OS
    /// cache dir: ~/.cache on Linux, ~/Library/Caches on macOS,
    /// %LOCALAPPDATA% on Windows]
    #[arg(long, value_name = "DIR", global = true)]
    cache_dir: Option<PathBuf>,
    /// Serve tiles from this directory instead of the network (offline/demo mode)
    #[arg(long, value_name = "DIR")]
    fixture_dir: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Delete the downloaded tile cache, after asking. Run this before
    /// `cargo uninstall ridge_redux`, which can't remove it for you.
    CleanCache {
        /// Don't ask; delete it
        #[arg(long, short)]
        yes: bool,
    },
}

impl Cli {
    fn into_config(self) -> ServerConfig {
        ServerConfig {
            addr: self.addr,
            web_dir: self.web_dir,
            srtm_base: self.srtm_base,
            cache_dir: self.cache_dir.unwrap_or_else(srtm::default_cache_dir),
            fixture_dir: self.fixture_dir,
            open_browser: !self.no_open,
        }
    }
}

impl ServerConfig {
    pub fn from_env_and_args() -> ServerConfig {
        Cli::parse().into_config()
    }
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ridge_redux=info,tower_http=info".into()),
        )
        .init();

    // Prefetching is a startup step, not server config: `ServerConfig` is
    // public and built with struct literals, so a new field would break them.
    let cli = Cli::parse();
    if let Some(Command::CleanCache { yes }) = cli.command {
        std::process::exit(clean_cache(cli.cache_dir.as_deref(), yes));
    }
    if cli.cache_dir.is_none() && cli.fixture_dir.is_none() {
        let dir = srtm::default_cache_dir();
        match srtm::migrate_legacy_cache(&dir) {
            Ok(0) => {}
            Ok(n) => tracing::info!("moved {} to {}", tiles(n), dir.display()),
            Err(e) => tracing::warn!("couldn't move the old tile cache: {e}"),
        }
    }
    let prefetch = !cli.no_prefetch;
    let config = cli.into_config();
    let state = state::AppState::new(&config);

    let app = build_router(state.clone(), &config);

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
    // Bound but not yet announced: connections queue until we serve, and
    // the browser isn't pointed here until the default scene is cached.
    if prefetch {
        api::prefetch_default_scene(&state).await;
    }
    let local = listener.local_addr().expect("bound address");
    let url = browse_url(local);
    // A wildcard bind (a container, a LAN server) has no browsable address
    // of its own: browsers reach the server through the host's published
    // port, so announce loopback and say what is actually listening.
    if local.ip().is_unspecified() {
        println!("ridge_redux running at {url} (listening on {local})");
    } else {
        println!("ridge_redux running at {url}");
    }
    if config.open_browser {
        if let Err(e) = open::that_detached(&url) {
            tracing::warn!("couldn't open a browser ({e}); open {url} yourself");
        }
    }
    axum::serve(listener, app).await.expect("server error");
}

/// `ridge_redux clean-cache`: delete the tile cache (`--cache-dir`, or the
/// default and any pre-0.1.1 one), asking first unless `yes`. Returns the
/// exit code.
fn clean_cache(cache_dir: Option<&Path>, yes: bool) -> i32 {
    let mut dirs = match cache_dir {
        Some(dir) => vec![dir.to_path_buf()],
        None => vec![srtm::default_cache_dir(), srtm::legacy_cache_dir()],
    };
    dirs.dedup();
    dirs.retain(|d| d.is_dir());
    if dirs.is_empty() {
        println!("No tile cache to delete.");
        return 0;
    }
    for dir in &dirs {
        match srtm::cache_size(dir) {
            Ok((files, bytes)) => {
                println!("{}: {files} tiles, {}", dir.display(), human_bytes(bytes))
            }
            Err(e) => println!("{}: can't read it ({e})", dir.display()),
        }
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("Not deleting without confirmation; pass --yes to skip the prompt.");
            return 1;
        }
        print!("Delete the tile cache? Tiles are downloaded again when needed. [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Kept the tile cache.");
            return 0;
        }
    }
    let mut code = 0;
    for dir in &dirs {
        match srtm::remove_cache(dir) {
            Ok((files, bytes)) => println!(
                "Deleted {} ({}, {}).",
                dir.display(),
                tiles(files),
                human_bytes(bytes)
            ),
            Err(e) => {
                eprintln!("Couldn't delete {}: {e}", dir.display());
                code = 1;
            }
        }
    }
    code
}

fn tiles(n: usize) -> String {
    format!("{n} tile{}", if n == 1 { "" } else { "s" })
}

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= 1024.0 * MIB {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * MIB))
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
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

/// The URL a browser can actually open for a bound listener. A wildcard
/// bind (a container, a LAN server) has no address to open directly: the
/// user goes through the host's published port, which is loopback here.
fn browse_url(local: SocketAddr) -> String {
    if local.ip().is_unspecified() {
        format!("http://127.0.0.1:{}", local.port())
    } else {
        format!("http://{local}")
    }
}

/// Build the full app router (separated for integration tests).
pub fn build_router(state: state::AppState, config: &ServerConfig) -> Router {
    let api = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/api/presets", get(api::presets))
        .route("/api/readme", get(api::readme))
        .route("/api/tiles", get(api::tiles))
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

    #[test]
    fn cli_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

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

    #[test]
    fn wildcard_binds_announce_loopback() {
        let v4: SocketAddr = "0.0.0.0:8420".parse().unwrap();
        assert_eq!(browse_url(v4), "http://127.0.0.1:8420");
        let v6: SocketAddr = "[::]:8420".parse().unwrap();
        assert_eq!(browse_url(v6), "http://127.0.0.1:8420");
        let loopback: SocketAddr = "127.0.0.1:8420".parse().unwrap();
        assert_eq!(browse_url(loopback), "http://127.0.0.1:8420");
        let lan: SocketAddr = "192.168.1.10:8420".parse().unwrap();
        assert_eq!(browse_url(lan), "http://192.168.1.10:8420");
    }
}
