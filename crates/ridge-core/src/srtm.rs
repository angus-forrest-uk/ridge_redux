//! SRTM elevation tiles: parsing, downloading, caching.
//!
//! Replicates the pieces of `SRTM.py` that upstream ridge_map relies on:
//!
//! * `.hgt` files are big-endian int16 grids, north-up, with `side` = 3601
//!   (SRTM 1-arc-second) or 1201 (SRTM 3-arc-second).
//! * Tiles are named `{N|S}{lat:02}{E|W}{lon:03}.hgt`, e.g. `N44W072.hgt`,
//!   and cover exactly one degree. `row = floor((lat_lo + 1 - lat) * (side-1))`,
//!   `col = floor((lon - lon_lo) * (side-1))`.
//! * Void / invalid samples (outside `[-1000, 10000]`) become NaN.
//! * Downloads come from a directory mirror (`srtm.kurviger.de` by default)
//!   as `.hgt.zip`, split across region subdirectories; the region index is
//!   scraped once and cached. Downloaded tiles are cached on disk unzipped.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::{Error, Result};

/// Memoized tile map shared by the cached sources.
type TileMap = HashMap<(i32, i32), Option<Arc<Tile>>>;

/// A parsed single-degree SRTM tile.
#[derive(Debug, Clone)]
pub struct Tile {
    /// Lower-left latitude of the tile, e.g. `44.0` for N44.
    pub lat_lo: f64,
    /// Lower-left longitude, e.g. `-72.0` for W072.
    pub lon_lo: f64,
    /// Grid side length (3601 or 1201).
    pub side: usize,
    /// Row-major, north-up elevation samples in meters.
    pub data: Vec<i16>,
}

impl Tile {
    /// Parse raw `.hgt` contents. `file_name` supplies the tile origin, e.g.
    /// `N44W072.hgt`.
    pub fn parse(file_name: &str, data: &[u8]) -> Result<Tile> {
        let (lat_lo, lon_lo) = parse_tile_name(file_name)
            .ok_or_else(|| Error::Srtm(format!("bad tile name {file_name:?}")))?;
        if !data.len().is_multiple_of(2) {
            return Err(Error::Srtm(format!("tile {file_name:?} has odd length")));
        }
        let n = data.len() / 2;
        let side = (n as f64).sqrt();
        if side.fract() != 0.0 {
            return Err(Error::Srtm(format!(
                "tile {file_name:?} has non-square size {n}"
            )));
        }
        let side = side as usize;
        let mut values = Vec::with_capacity(n);
        for chunk in data.chunks_exact(2) {
            values.push(i16::from_be_bytes([chunk[0], chunk[1]]));
        }
        Ok(Tile {
            lat_lo,
            lon_lo,
            side,
            data: values,
        })
    }

    pub fn resolution(&self) -> f64 {
        1.0 / (self.side - 1) as f64
    }

    /// Nearest-neighbor elevation at `(lat, lon)` in meters, or NaN for
    /// voids / out-of-tile points (mirrors `SRTM.py`'s `get_elevation`).
    pub fn elevation(&self, lat: f64, lon: f64) -> f64 {
        let s = (self.side - 1) as f64;
        let row = ((self.lat_lo + 1.0 - lat) * s).floor();
        let col = ((lon - self.lon_lo) * s).floor();
        if row < 0.0 || col < 0.0 || row as usize >= self.side || col as usize >= self.side {
            return f64::NAN;
        }
        let v = self.data[row as usize * self.side + col as usize] as f64;
        if !(-1000.0..=10000.0).contains(&v) {
            f64::NAN
        } else {
            v
        }
    }
}

/// `N44W072.hgt` -> `(44.0, -72.0)`.
pub fn parse_tile_name(name: &str) -> Option<(f64, f64)> {
    let base = name.rsplit('/').next()?;
    let stem = base.strip_suffix(".hgt")?;
    let bytes = stem.as_bytes();
    if bytes.len() != 7 {
        return None;
    }
    let ns = bytes[0];
    let lat: i32 = stem[1..3].parse().ok()?;
    let ew = bytes[3];
    let lon: i32 = stem[4..7].parse().ok()?;
    let lat_lo = match ns {
        b'N' => lat as f64,
        b'S' => -(lat as f64),
        _ => return None,
    };
    let lon_lo = match ew {
        b'E' => lon as f64,
        b'W' => -(lon as f64),
        _ => return None,
    };
    Some((lat_lo, lon_lo))
}

/// `lat=44.3, lon=-71.9` -> `"N44W072.hgt"` (mirrors `SRTM.py` `get_file_name`).
pub fn tile_name(lat: f64, lon: f64) -> String {
    let (ns, lat) = if lat >= 0.0 { ('N', lat) } else { ('S', -lat) };
    let (ew, lon) = if lon >= 0.0 { ('E', lon) } else { ('W', -lon) };
    // srtm.py: str(int(abs(floor(x)))).zfill(...)
    format!(
        "{ns}{:02}{ew}{:03}.hgt",
        lat.floor() as i32,
        lon.floor() as i32
    )
}

/// Where tiles come from.
pub trait TileSource: Send + Sync {
    fn tile(&self, lat_lo: i32, lon_lo: i32) -> Option<Arc<Tile>>;
}

/// Tiles read from a local directory (tests, offline demos, pre-seeded caches).
/// Parsed tiles are memoized, so repeated lookups don't re-read the file.
pub struct DirSource {
    dir: PathBuf,
    tiles: Mutex<TileMap>,
}

impl DirSource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            tiles: Mutex::new(HashMap::new()),
        }
    }
}

impl TileSource for DirSource {
    fn tile(&self, lat_lo: i32, lon_lo: i32) -> Option<Arc<Tile>> {
        if let Some(hit) = self.tiles.lock().unwrap().get(&(lat_lo, lon_lo)) {
            return hit.clone();
        }
        let name = tile_name(lat_lo as f64, lon_lo as f64);
        let loaded = std::fs::read(self.dir.join(&name))
            .ok()
            .and_then(|bytes| Tile::parse(&name, &bytes).ok())
            .map(Arc::new);
        self.tiles
            .lock()
            .unwrap()
            .insert((lat_lo, lon_lo), loaded.clone());
        loaded
    }
}

/// In-memory source for unit tests: synthesizes tiles on demand.
pub struct SyntheticSource {
    pub side: usize,
}

impl SyntheticSource {
    /// A smooth valley-and-ridge terrain, in meters, over global lat/lon.
    pub fn value(lat: f64, lon: f64) -> f64 {
        let r = ((lat - 44.0).powi(2) + 1.5 * (lon + 71.5).powi(2)).sqrt();
        900.0 * (-0.15 * r).exp() + 300.0 * (0.05 * lon).sin() * (0.07 * lat).cos() + 400.0
    }
}

impl TileSource for SyntheticSource {
    fn tile(&self, lat_lo: i32, lon_lo: i32) -> Option<Arc<Tile>> {
        let n = self.side * self.side;
        let mut data = Vec::with_capacity(n);
        for row in 0..self.side {
            let lat = lat_lo as f64 + 1.0 - row as f64 / (self.side - 1) as f64;
            for col in 0..self.side {
                let lon = lon_lo as f64 + col as f64 / (self.side - 1) as f64;
                data.push(Self::value(lat, lon).round() as i16);
            }
        }
        Some(Arc::new(Tile {
            lat_lo: lat_lo as f64,
            lon_lo: lon_lo as f64,
            side: self.side,
            data,
        }))
    }
}

/// Remote mirror + on-disk cache, replicating `SRTM.py`'s default behavior.
///
/// The mirror lists `.hgt.zip` files under region subdirectories; we scrape
/// the whole index once (8-ish requests) and memoize it. Tiles land in
/// `cache_dir` as plain `.hgt` files, so a `DirSource` pointed at the same
/// directory shares the cache.
pub struct RemoteSource {
    base_urls: Vec<String>,
    cache_dir: PathBuf,
    agent: ureq::Agent,
    index: RwLock<Option<HashMap<String, String>>>,
    tiles: Mutex<TileMap>,
}

impl RemoteSource {
    pub fn new(base_urls: &[&str], cache_dir: impl Into<PathBuf>) -> Result<Self> {
        let cache_dir = cache_dir.into();
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            base_urls: base_urls
                .iter()
                .map(|u| u.trim_end_matches('/').to_string())
                .collect(),
            cache_dir,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(20))
                .timeout(std::time::Duration::from_secs(180))
                .user_agent("ridge-redux/0.1")
                .build(),
            index: RwLock::new(None),
            tiles: Mutex::new(HashMap::new()),
        })
    }

    /// Default mirror and `~/.cache/ridge-redux/srtm`.
    pub fn default_paths() -> Result<Self> {
        let cache = std::env::var("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".cache"))
            .join("ridge-redux")
            .join("srtm");
        Self::new(
            &[
                "https://srtm.kurviger.de/SRTM1/",
                "https://srtm.kurviger.de/SRTM3/",
            ],
            cache,
        )
    }

    fn fetch_url(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| Error::Srtm(format!("GET {url}: {e}")))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(512 * 1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| Error::Srtm(format!("reading {url}: {e}")))?;
        Ok(buf)
    }

    /// Scrape each mirror's subdirectories for `NAME.hgt.zip -> url`.
    /// Earlier bases win, so pass higher-resolution mirrors first.
    fn build_index(&self) -> Result<HashMap<String, String>> {
        let mut index = HashMap::new();
        for base in &self.base_urls {
            let page = self.fetch_url(&format!("{base}/"))?;
            let text = String::from_utf8_lossy(&page);
            // Subdirectories are linked as `Region_01/index.html` (SRTM1) or
            // `Eurasia/index.html` (SRTM3) — or with a trailing slash.
            let dirs = scan_hrefs(&text)
                .into_iter()
                .filter_map(|h| {
                    if h.starts_with("..") {
                        return None;
                    }
                    if let Some(dir) = h.strip_suffix("/index.html") {
                        return Some(dir.to_string());
                    }
                    if h.ends_with('/') {
                        return Some(h.trim_end_matches('/').to_string());
                    }
                    None
                })
                .collect::<Vec<_>>();
            for dir in dirs {
                let dir_url = format!("{base}/{}", dir.trim_end_matches('/'));
                let sub = match self.fetch_url(&dir_url) {
                    Ok(sub) => sub,
                    Err(_) => continue,
                };
                let sub_text = String::from_utf8_lossy(&sub);
                for href in scan_hrefs(&sub_text) {
                    if let Some(name) = href.rsplit('/').next() {
                        if name.ends_with(".hgt.zip") {
                            let file = name.trim_end_matches(".zip");
                            index.entry(file.to_string()).or_insert_with(|| {
                                if href.starts_with("http") {
                                    href.clone()
                                } else {
                                    format!("{dir_url}/{href}")
                                }
                            });
                        }
                    }
                }
            }
        }
        if index.is_empty() {
            return Err(Error::Srtm("mirror index came back empty".into()));
        }
        Ok(index)
    }

    pub fn index(&self) -> Result<HashMap<String, String>> {
        if let Some(idx) = self.index.read().unwrap().as_ref() {
            return Ok(idx.clone());
        }
        let idx = self.build_index()?;
        *self.index.write().unwrap() = Some(idx.clone());
        Ok(idx)
    }

    pub fn load_or_download(&self, lat_lo: i32, lon_lo: i32) -> Result<Arc<Tile>> {
        let name = tile_name(lat_lo as f64, lon_lo as f64);
        let path = self.cache_dir.join(&name);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                let index = self.index()?;
                let url = index.get(&name).ok_or_else(|| {
                    Error::Srtm(format!(
                        "no tile {name} on the mirror (ocean or out of range?)"
                    ))
                })?;
                let zipped = self.fetch_url(url)?;
                let raw = unzip_single(&zipped, &name)
                    .ok_or_else(|| Error::Srtm(format!("could not unzip tile {name}")))?;
                std::fs::write(&path, &raw)?;
                raw
            }
        };
        Ok(Arc::new(Tile::parse(&name, &bytes)?))
    }
}

impl TileSource for RemoteSource {
    fn tile(&self, lat_lo: i32, lon_lo: i32) -> Option<Arc<Tile>> {
        if let Some(cached) = self.tiles.lock().unwrap().get(&(lat_lo, lon_lo)) {
            return cached.clone();
        }
        let loaded = self.load_or_download(lat_lo, lon_lo).ok();
        self.tiles
            .lock()
            .unwrap()
            .insert((lat_lo, lon_lo), loaded.clone());
        loaded
    }
}

/// Extract every `href="..."` from an HTML page.
fn scan_hrefs(page: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = page;
    while let Some(pos) = rest.find("href=\"") {
        rest = &rest[pos + 6..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        } else {
            break;
        }
    }
    out
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Minimal zip extractor: find the entry whose name ends with `want_suffix`
/// (or the first entry) and decompress it. Handles stored + deflate entries.
pub fn unzip_single(zip: &[u8], want_suffix: &str) -> Option<Vec<u8>> {
    // End of central directory record.
    let eocd = zip
        .windows(22)
        .rev()
        .find(|w| w[..4] == [0x50, 0x4b, 0x05, 0x06])?;
    let eocd_pos = zip.len() - eocd.len();
    let entries = u16::from_le_bytes([eocd[10], eocd[11]]) as usize;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as usize;
    let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as usize;
    if cd_offset == 0xFFFF_FFFF {
        return None; // zip64: not needed for 25 MB tiles
    }
    let _ = eocd_pos;
    let mut cd = &zip[cd_offset.min(zip.len())..(cd_offset + cd_size).min(zip.len())];

    for _ in 0..entries {
        if cd.len() < 46 || cd[..4] != [0x50, 0x4b, 0x01, 0x02] {
            return None;
        }
        let method = u16::from_le_bytes([cd[10], cd[11]]);
        let comp_size = u32::from_le_bytes([cd[20], cd[21], cd[22], cd[23]]) as usize;
        let name_len = u16::from_le_bytes([cd[28], cd[29]]) as usize;
        let extra_len = u16::from_le_bytes([cd[30], cd[31]]) as usize;
        let comment_len = u16::from_le_bytes([cd[32], cd[33]]) as usize;
        let local_off = u32::from_le_bytes([cd[42], cd[43], cd[44], cd[45]]) as usize;
        let name = String::from_utf8_lossy(&cd[46..46 + name_len]).to_string();
        cd = &cd[46 + name_len + extra_len + comment_len..];

        if !want_suffix.is_empty() && !name.ends_with(want_suffix) {
            continue;
        }
        // Local file header: skip name + extra to find data.
        if local_off + 30 > zip.len() || zip[local_off..local_off + 4] != [0x50, 0x4b, 0x03, 0x04] {
            return None;
        }
        let l_name = u16::from_le_bytes([zip[local_off + 26], zip[local_off + 27]]) as usize;
        let l_extra = u16::from_le_bytes([zip[local_off + 28], zip[local_off + 29]]) as usize;
        let data_start = local_off + 30 + l_name + l_extra;
        let data_end = (data_start + comp_size).min(zip.len());
        let data = &zip[data_start..data_end];
        return match method {
            0 => Some(data.to_vec()),
            8 => {
                let mut out = Vec::with_capacity(comp_size * 4);
                let mut decoder = flate2::read::DeflateDecoder::new(data);
                decoder.read_to_end(&mut out).ok()?;
                Some(out)
            }
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_names_roundtrip() {
        assert_eq!(tile_name(44.0, -72.0), "N44W072.hgt");
        assert_eq!(tile_name(-33.5, 151.2), "S33E151.hgt");
        assert_eq!(parse_tile_name("N44W072.hgt"), Some((44.0, -72.0)));
        assert_eq!(parse_tile_name("S33E151.hgt"), Some((-33.0, 151.0)));
        assert_eq!(parse_tile_name("junk.hgt"), None);
    }

    #[test]
    fn synthetic_tile_lookup() {
        let src = SyntheticSource { side: 1201 };
        let t = src.tile(44, -72).unwrap();
        let v = t.elevation(44.5, -71.5);
        let expected = SyntheticSource::value(44.5, -71.5).round();
        assert!((v - expected).abs() < 1e-9);
        assert!(t.elevation(45.5, -71.5).is_nan()); // out of tile
    }

    #[test]
    fn zip_roundtrip() {
        // Build a stored (method 0) zip by hand: header + data + central dir + eocd.
        let name = b"N44W072.hgt";
        let payload = b"hello-tile-data".to_vec();
        let mut zip = Vec::new();
        // local header
        zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&0u32.to_le_bytes()); // crc
        zip.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(name);
        zip.extend_from_slice(&payload);
        // central directory
        let cd_start = zip.len();
        // sig(4), ver_made(2), ver_needed(2), flags(2), method(2), time(2), date(2) = 16 bytes
        zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
        zip.extend_from_slice(&0u16.to_le_bytes()); // comment len
        zip.extend_from_slice(&0u16.to_le_bytes()); // disk number start
        zip.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        zip.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        zip.extend_from_slice(&0u32.to_le_bytes()); // local header offset (header starts at 0)
        zip.extend_from_slice(name);
        let cd_end = zip.len();
        // eocd
        zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0]);
        zip.extend_from_slice(&((cd_end - cd_start) as u32).to_le_bytes());
        zip.extend_from_slice(&(cd_start as u32).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());

        let out = unzip_single(&zip, ".hgt").expect("extract");
        assert_eq!(out, payload);
    }
}
