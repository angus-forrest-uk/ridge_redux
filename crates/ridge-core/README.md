# ridge-core

[![crates.io](https://img.shields.io/crates/v/ridge-core.svg)](https://crates.io/crates/ridge-core)
[![docs.rs](https://img.shields.io/docsrs/ridge-core)](https://docs.rs/ridge-core)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/angus-forrest-uk/ridge_redux/blob/main/LICENSE)

The terrain pipeline behind [ridge_redux](https://crates.io/crates/ridge_redux):
a Rust port of the Python package
[ridge_map](https://github.com/ColCarroll/ridge_map) ("3D maps with 1D lines"),
without matplotlib.

![The White Mountains, rendered by ridge-core](https://raw.githubusercontent.com/angus-forrest-uk/ridge_redux/main/examples/white_mountains.png)

It turns a bounding box into ridgeline artwork:

1. **Fetch** SRTM elevation tiles from a mirror, cached on disk
   ([`srtm`](https://docs.rs/ridge-core/latest/ridge_core/srtm/)).
2. **Sample** a grid of lines over the box
   ([`grid`](https://docs.rs/ridge-core/latest/ridge_core/grid/)). The
   sampling is bit-for-bit identical to ridge_map's, checked against its test
   fixture.
3. **Rotate** to a viewpoint angle, like `scipy.ndimage.rotate`
   ([`rotate`](https://docs.rs/ridge-core/latest/ridge_core/rotate/)).
4. **Mask** water and lakes, and exaggerate the relief
   ([`preprocess`](https://docs.rs/ridge-core/latest/ridge_core/preprocess/)).
5. **Lay out** the ridge rows as a matplotlib-style figure
   ([`geometry`](https://docs.rs/ridge-core/latest/ridge_core/geometry/)) and
   **write SVG** ([`svg`](https://docs.rs/ridge-core/latest/ridge_core/svg/)).

## Command line

`cargo install ridge-core` installs `render`, which draws one map to SVG:

```bash
render --bbox "11.098251,47.264786,11.695633,47.453630" \
  --num-lines 150 --vertical-ratio 240 --lake-flatness 2 \
  --label "Karwendelgebirge" --label-x 0.55 --label-y 0.1 --label-size 40 \
  --out karwendel.svg
```

`render --help` lists every option. For an interactive editor (pick the area
on a map, rotate and restyle live), use `cargo install ridge_redux`.

## Library

```rust,no_run
use ridge_core::colormap::{hex, LineColor};
use ridge_core::geometry::{build_scene, ColorKind};
use ridge_core::srtm::RemoteSource;
use ridge_core::svg::{render_svg, LineColorSpec, PlotStyle};
use ridge_core::Bbox;

fn main() -> ridge_core::Result<()> {
    // Downloads tiles on first use and caches them in ~/.cache/ridge-redux/srtm.
    let source = RemoteSource::default_paths()?;
    let white_mountains = Bbox::new(-71.928864, 43.758201, -70.957947, 44.465151);
    let scene = build_scene(
        &source,
        &white_mountains,
        80, 300,              // lines, points per line
        0.0, false, 0, false, // viewpoint angle, crop, interpolation, lock resolution
        10.0, 3, 40.0,        // water percentile, lake flatness, vertical ratio
        20.0,                 // figure width in inches
    )?;
    let style = PlotStyle {
        line: LineColorSpec::from(&LineColor::parse("black").unwrap()),
        kind: ColorKind::Gradient,
        background: hex("#ece8ec"),
        linewidth_pt: 2.0,
        size_scale: 20.0,
        label: None,
        annotation: None,
    };
    std::fs::write("white_mountains.svg", render_svg(&scene, &style))?;
    Ok(())
}
```

`srtm::DirSource` reads `.hgt` tiles from a directory instead, for offline
use. SRTM covers latitudes between 60°S and 60°N.

## License

MIT. ridge-core is a port of ridge_map, also MIT, whose copyright notice is
kept in [LICENSE-upstream](https://github.com/angus-forrest-uk/ridge_redux/blob/main/LICENSE-upstream).

Elevation data: NASA [Shuttle Radar Topography Mission](https://www2.jpl.nasa.gov/srtm/).
