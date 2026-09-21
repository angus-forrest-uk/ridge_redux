//! Choose the frontend to embed: the workspace's `web/dist` (a local
//! `just web` build), else the crate's own `dist/` (a published crate ships
//! one, and has no workspace around it), else a placeholder page, so
//! building the Rust never needs Node. Likewise the README the app shows.

use std::path::{Path, PathBuf};

const PLACEHOLDER: &str = "<!doctype html>
<title>ridge-redux</title>
<p>ridge-redux: the frontend wasn't built when this binary was compiled.
Run <code>just web</code> (or <code>npm --prefix web ci &amp;&amp; npm --prefix web run build</code>)
and build again, or pass <code>--web-dir web/dist</code>.</p>
";

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let packaged = manifest.join("dist");
    let workspace = manifest.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", packaged.display());
    println!("cargo:rerun-if-changed={}", workspace.display());

    let dir = [&workspace, &packaged]
        .into_iter()
        .find(|d| d.join("index.html").is_file())
        .cloned()
        .unwrap_or_else(|| {
            let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("frontend-missing");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join("index.html"), PLACEHOLDER).unwrap();
            println!(
                "cargo:warning=web/dist not found; embedding a placeholder page (run `just web`)"
            );
            out
        });
    println!(
        "cargo:rustc-env=RIDGE_FRONTEND_DIR={}",
        canonical(&dir).display()
    );

    let readme = [manifest.join("../../README.md"), manifest.join("README.md")]
        .into_iter()
        .find(|p| p.is_file())
        .expect("README.md not found");
    println!("cargo:rerun-if-changed={}", readme.display());
    println!(
        "cargo:rustc-env=RIDGE_README={}",
        canonical(&readme).display()
    );
}

fn canonical(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}
