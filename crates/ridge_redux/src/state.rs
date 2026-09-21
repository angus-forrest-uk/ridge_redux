//! Shared server state: tile source + a small in-memory grid cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ndarray::Array2;

use ridge_core::srtm::{DirSource, RemoteSource, TileSource};

use crate::ServerConfig;

const MAX_CACHED_GRIDS: usize = 64;

pub struct GridCache {
    inner: Arc<GridCacheInner>,
}

struct GridCacheInner {
    map: Mutex<HashMap<u64, Arc<Array2<f64>>>>,
    order: Mutex<Vec<u64>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl Clone for GridCache {
    fn clone(&self) -> Self {
        GridCache {
            inner: self.inner.clone(),
        }
    }
}

impl GridCache {
    fn new() -> Self {
        GridCache {
            inner: Arc::new(GridCacheInner {
                map: Mutex::new(HashMap::new()),
                order: Mutex::new(Vec::new()),
                hits: AtomicUsize::new(0),
                misses: AtomicUsize::new(0),
            }),
        }
    }

    pub fn get(&self, key: u64) -> Option<Arc<Array2<f64>>> {
        let hit = self.inner.map.lock().unwrap().get(&key).cloned();
        if hit.is_some() {
            self.inner.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner.misses.fetch_add(1, Ordering::Relaxed);
        }
        hit
    }

    pub fn insert(&self, key: u64, grid: Arc<Array2<f64>>) {
        let mut map = self.inner.map.lock().unwrap();
        if map.contains_key(&key) {
            return;
        }
        let mut order = self.inner.order.lock().unwrap();
        if map.len() >= MAX_CACHED_GRIDS {
            if let Some(oldest) = order.first().copied() {
                map.remove(&oldest);
                order.remove(0);
            }
        }
        map.insert(key, grid);
        order.push(key);
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.inner.hits.load(Ordering::Relaxed),
            self.inner.misses.load(Ordering::Relaxed),
        )
    }
}

#[derive(Clone)]
pub struct AppState {
    pub source: Arc<dyn TileSource>,
    pub grid_cache: GridCache,
}

impl AppState {
    pub fn new(config: &ServerConfig) -> AppState {
        let source: Arc<dyn TileSource> = if let Some(dir) = &config.fixture_dir {
            tracing::info!("using fixture tiles from {}", dir.display());
            Arc::new(DirSource::new(dir))
        } else {
            let bases: Vec<&str> = config.srtm_base.split(',').map(str::trim).collect();
            match RemoteSource::new(&bases, &config.cache_dir) {
                Ok(src) => {
                    tracing::info!(
                        "SRTM mirror: {} (cache: {})",
                        config.srtm_base,
                        config.cache_dir.display()
                    );
                    Arc::new(src)
                }
                Err(e) => {
                    tracing::error!("cannot init SRTM cache dir: {e}");
                    std::process::exit(1);
                }
            }
        };
        AppState {
            source,
            grid_cache: GridCache::new(),
        }
    }
}
