//! Debug: fetch a single tile and print diagnostics.
use ridge_core::srtm::RemoteSource;

fn main() {
    let src = RemoteSource::new(
        &[
            "https://srtm.kurviger.de/SRTM1/",
            "https://srtm.kurviger.de/SRTM3/",
        ],
        std::env::temp_dir().join("ridge-debug-cache"),
    )
    .unwrap();
    match src.load_or_download(44, -72) {
        Ok(t) => {
            println!("tile ok: side={} samples={}", t.side, t.data.len());
            println!(
                "Mt Washington (44.2705, -71.3030) = {} m",
                t.elevation(44.2705, -71.3030)
            );
        }
        Err(e) => println!("download failed: {e}"),
    }
    let idx = src.index();
    match idx {
        Ok(m) => println!(
            "index size: {} (N44W072 -> {})",
            m.len(),
            m.get("N44W072.hgt")
                .map(|s| &s[..s.len().min(60)])
                .unwrap_or("MISSING")
        ),
        Err(e) => println!("index failed: {e}"),
    }
}
