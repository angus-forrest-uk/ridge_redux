# ridge-redux

*Ridgeline plots of ridges, in Rust, in your browser.*

> **TL;DR:** [ridge_map](https://github.com/ColCarroll/ridge_map) makes
> beautiful ridgeline maps. To make these maps, you need to modify a Python script: choose
> coordinates by hand, re-run for every angle or colour, and get a static
> matplotlib figure. ridge-redux is the same pipeline in Rust (its sampling
> is bit-for-bit identical to upstream) behind an interactive web app: draw the
> area on a map, rotate and restyle instantly, export SVG or PNG. One
> `cargo install`.

![The ridge-redux app: the artwork on the left, its controls on the right, the location map below](docs/screenshots/app.png)

| Rotate and restyle, instantly | Pick the area on the map |
|---|---|
| ![Karwendelgebirge rotated to 30°](docs/screenshots/rotated.png) | ![Drawing a new area with the select tool](docs/screenshots/map-select.png) |
| Presets, the viewpoint angle, water, relief and style all redraw in the browser as you drag. | **move** pans the map; **select** (or Shift-drag) draws the area to render. |

<details>
<summary>Every control</summary>

![The control sidebar](docs/screenshots/controls.png)

</details>

## Install

From crates.io, the only prerequisite is a Rust toolchain
([rustup.rs](https://rustup.rs)). From source you also need Node.js 22 or
later, to build the frontend.

**From crates.io**

```bash
cargo install ridge_redux
ridge_redux
# open http://127.0.0.1:8420
```

**From source**

```bash
git clone https://github.com/angus-forrest-uk/ridge_redux
cd ridge_redux
npm --prefix web ci && npm --prefix web run build   # frontend -> web/dist
cargo run --release
# open http://127.0.0.1:8420
```

## Why a local server?

ridge-redux runs as a small server on your own machine, and you use it in
the browser you already have. It isn't a downloadable app or a hosted website,
on purpose.

**Why not a desktop app?** A double-click app has to be signed to open
cleanly on each platform. On macOS, an unsigned download is quarantined, and
recent versions report it as "damaged". Signing and notarisation need a paid
Apple Developer account. On Windows, an unsigned installer gets a SmartScreen
warning unless it's signed with a code-signing certificate. On Linux, a webview
app depends on the distro's system libraries. That's recurring cost and upkeep
for a hobby project. `cargo install` compiles on your own machine, so nothing
needs signing, and every platform Rust supports works the same way.

**Why not a website?**

- **Data volume.** Every new area needs whole SRTM elevation tiles. An SRTM1
  tile covers 1°×1° and is about 26 MB unpacked, and one view often spans 2–4
  of them. A public site would have to fetch, store and serve that for every
  visitor's area, which means real storage and bandwidth bills.
- **The upstream mirrors.** The tiles come from free community mirrors
  (kurviger by default). A local user fetches each tile once and keeps it. A
  public service sending every visitor through those mirrors would be abusing
  a free resource, and would soon hit rate limits.
- **Compute.** The server samples the elevation grid again on every change of
  location or resolution. That's cheap on your CPU. A hosted service would pay
  for it on every request, from every visitor.
- **Caching is personal.** Your cache (`~/.cache/ridge-redux/srtm/`) only
  holds the places *you* look at. A shared cache has to hold everyone's
  places, which brings back the cost problem.
- **Nothing to keep running.** A local tool costs nothing while nobody is
  using it, and there's no service to keep up or protect from abuse.

**Security.** The server has no authentication or rate limiting, and it binds
to `127.0.0.1` by default. Don't expose it on an untrusted network with
`--addr 0.0.0.0`.

## Using it

On first use the server downloads SRTM elevation tiles on demand and caches
them under `~/.cache/ridge-redux/srtm/` (one fetch per tile, ever).
Drag to pan, wheel to zoom: both instant, purely client-side. The
**viewpoint-angle, water and relief sliders are also instant**: the server
ships the raw elevation grid once per location/resolution
(`POST /api/elevation`), and the browser rotates it about its center, masks
water/lakes and rebuilds the ridge lines locally, throttled to animation
frames. Only changing the location or resolution hits the network.

**Export**: `SVG` downloads vector artwork straight from the backend;
`PNG` rasterizes it at 2× resolution in the browser.

**Map picker**: the bottom panel hosts an OpenStreetMap slippy map with two
tools. **Move** (the default) drags to pan. **Select** drags to draw the
bounding box, and holding Shift draws one without leaving Move. The box syncs
both ways with the coordinate inputs and presets. Areas beyond SRTM coverage (|φ| > 60°) are shaded out and
drawn selections are clamped to the covered band.

**Rotation model** (all client-side, one elevation fetch): the backend ships
a rotation-invariant **disc** of samples: a square region around your bbox
center masked to the inscribed circle (span = the bbox diagonal by default,
adjustable). Both view modes are then just windows over that disc, rotated
about its center:

- **Rectangle**: a fixed `num_lines × elevation_pts` window; identical
  style, spacing and ratios at every angle, with previously unused points
  rotating into frame as it sweeps around.
- **Full disc**: shows the whole circle (square figure).

No zoom compensation, no re-requesting: the point count inside the window is
constant at every angle (±<2%, just coastline voids rotating at the rim).

## Examples

All of these were produced by this repo (the same bboxes and parameters as the
upstream README):

| | |
|---|---|
| ![Karwendelgebirge](examples/karwendelgebirge.png) | ![Hawaii](examples/hawaii.png) |
| Karwendelgebirge (SRTM3 fallback) | Hawai'i, `ocean` colormap, `kind=elevation` |
| ![Washington](examples/washington.png) | ![White Mountains](examples/white_mountains.png) |
| Washington | The default: The White Mountains |

You can also render headless with the CLI (great for piping to a file):

```bash
cargo run --release -p ridge-core --bin render -- \
  --bbox "11.098251,47.264786,11.695633,47.453630" \
  --num-lines 150 --vertical-ratio 240 --lake-flatness 2 \
  --label "Karwendelgebirge" --label-x 0.55 --label-y 0.1 --label-size 40 \
  --out karwendel.svg
```

Run `render --help` for every knob (bbox, viewpoint angle, colormaps,
water percentile, annotations, …).

## Architecture

```
┌────────────────────────── backend (Rust) ──────────────────────────┐
│ ridge-core                       ridge_redux (axum)                │
│ ├─ srtm: .hgt fetch/parse/cache  ├─ POST /api/elevation → raw grid │
│ │   (SRTM1 → SRTM3 fallback,     ├─ POST /api/preview  → JSON      │
│ │    zip support, disk cache)    ├─ POST /api/export.svg → SVG     │
│ ├─ grid sampling (=srtm.py)      ├─ GET  /api/presets, /healthz    │
│ │   rect + rotation-invariant                                      │
│ │   disc regions                                                   │
│ ├─ rotate (=scipy order 0/1)     ├─ static frontend + gzip         │
│ │   + rotate_fixed_plane         └─ in-memory grid cache (LRU)     │
│ ├─ preprocess (water/lakes)                                        │
│ ├─ geometry → RidgeScene                                           │
│ └─ colormaps + SVG writer                                          │
└────────────────────────────┬───────────────────────────────────────┘
                             │ raw grid (integers, voids→null), JSON geometry
┌────────────────────────────▼── frontend (Astro + SolidJS) ─────────┐
│ ONE fetch per location/resolution, then everything is local:       │
│ ├─ rotatePlane(): spin the grid about its center (fixed canvas)    │
│ ├─ preprocessGrid(): water percentile + lake gradient masks        │
│ ├─ rows + matplotlib-parity layout, drawn back-to-front            │
│ ├─ Solid memos: only location/resolution changes refetch           │
│ └─ pan/zoom = view transform; angle slider runs at frame rate      │
└────────────────────────────────────────────────────────────────────┘
```

Division of labor: the server owns anything that needs tiles or
authoritative export (sampling, SVG). The browser owns everything
per-interactive-frame: rotation about the landscape center (the plane never
moves, so zoom/distance stay fixed), water/lake masking, and drawing. A
parity test (`web/test/parity.test.ts`) proves the TypeScript pipeline
matches the Rust one bit-for-bit (worst delta 0.0 on a rotated fixture). No
WASM: a 300×300 grid pipelines in a few milliseconds of plain TypeScript; if you ever
want 1000×1000 at frame rate, `ridge-core` is structured to compile to WASM
via wasm-bindgen and drop in.

## API

`POST /api/elevation` is the call the frontend makes. It takes
`{ bbox, num_lines, elevation_pts, region: "rect" | "disc", span_deg }` and returns
the raw sampled grid as `{ shape, values, window }`: whole metres, with `null` for
voids. Rotation, masking and drawing then happen in the browser.

`POST /api/preview`: the body is a JSON `RenderParams` (all fields optional,
see [`crates/ridge_redux/src/api.rs`](crates/ridge_redux/src/api.rs)):

```jsonc
{
  "bbox": [-71.93, 43.76, -70.96, 44.47],   // (lon, lat, lon, lat)
  "num_lines": 80, "elevation_pts": 300,
  "viewpoint_angle": 0, "crop": false, "interpolation": 0,
  "water_ntile": 10, "lake_flatness": 3, "vertical_ratio": 40,
  "linewidth_pt": 2,
  "line_color": "black",     // name | "#0f0f0f" | colormap: viridis, ocean, ...
  "kind": "gradient",        // or "elevation"
  "background_color": "#ece8ec",
  "label": "The White\nMountains", "label_x": 0.62, "label_y": 0.15,
  "label_size_pt": 60,
  "annotation": { "lon": -71.3173, "lat": 44.2946, "label": "Mt Washington" }
}
```

Response: `{ shape, rows: [{baseline, y[...]}], vmin, vmax, layout, style }`
where `y` values are `null` across water/gaps. The frontend maps `layout`
into canvas coordinates and paints rows back-to-front with a background-color
fill under each line, the same occlusion trick as upstream `fill_between`.

`POST /api/export.svg` takes the identical body and returns standalone SVG.
`GET /api/presets` lists 10 curated locations (same as the upstream README).

## Fidelity to upstream

The port is deliberately faithful, down to the odd corners:

- **lake flatness is computed on floats with spatial coherence**: the
  gradient runs on normalized floats (threshold `lake_flatness/255`, same
  semantics as upstream's u8 rank gradient but without its rounding
  terraces), water/NaN cells are excluded from the neighborhoods (skimage's
  `mask` parameter, so flat shores merge into the water body instead of
  forming a drawn perimeter), and lake candidates survive only as connected
  components of ≥12 cells, so there are no speckle holes on rolling hills,
- **the water percentile ignores NaN padding**: in disc mode ~21% of the
  sample square is padding; computing the percentile over it would pin the
  water level to zero and erase every river and stream, so it is measured
  over in-disc terrain only,
- **masks are decided before rotation**: water percentile, lake mask and
  normalization run once on the unrotated disc, and the masked grid is then
  rotated for display. Decisions attach to physical locations, so nothing
  flickers as the angle changes,
- sampling uses `lat0 + r/N * dlat` (stops one step short of the far corner),
- viewpoint angles in (45°,135°) ∪ (225°,315°) swap `num_lines`/`elevation_pts`,
- rotation fills out-of-bounds samples with `0.0` (scipy `mode='constant'`),
- percentiles use numpy's linear interpolation; lake detection quantizes to
  u8 before the 3×3 morphological gradient (skimage `rank.gradient`),
- rows are flipped (south in front) and scaled by `vertical_ratio`,
- lines step `-6` per row, colored by row index (gradient) or elevation.

The golden test compares our sampled White Mountains grid against the upstream
test fixture ([`test/test_data/new_hampshire.npz`](https://github.com/ColCarroll/ridge_map/tree/main/test/test_data)): **0/24000
mismatches**, i.e. bit-for-bit identical sampling to Python + srtm.py.

## Development

Development tasks are [`just`](https://github.com/casey/just) recipes
(`cargo install just`, or your package manager). Run `just` to list them.

```bash
just web          # build the frontend (web/: Astro + SolidJS) into web/dist
just run          # build the frontend, then serve the app on localhost
just offline      # same, against the fixture tiles, with no network
just web-dev      # frontend dev server with live reload on :4321 (API from `just run`)
just test         # 44 Rust tests: 31 unit (incl. golden parity) + 13 API
just test-web     # Vitest: app state + TS pipeline == Rust pipeline (bit-for-bit)
just check-web    # type-check the frontend
just fixtures     # fetch the SRTM tiles for the golden parity test
just fmt          # cargo fmt
just clippy       # clippy, warnings are errors
just ci           # everything CI runs: fmt-check, clippy, test, check-web, test-web
just render --bbox "..." --out out.svg   # headless SVG render
just screenshots  # regenerate docs/screenshots/ with Playwright (app must be running)
```

Diagnostics that aren't recipes:

```bash
cargo run -p ridge-core --example debug_tile          # connectivity diagnostics
cargo run -p ridge-core --example dump_plane_fixture  # fixture for the parity check
```

- Offline/demo mode: `--fixture-dir fixtures/srtm` (server) or
  `--fixture-dir` + `DirSource` in code.
- Mirrors are configurable: `--srtm-base "URL1,URL2"` (default: kurviger
  SRTM1, falling back to the global SRTM3 set).
- Font: labels use [Cinzel](https://fonts.google.com/specimen/Cinzel), loaded
  from Google Fonts by the frontend. SVG export references the family by name
  (browsers fetch it); server-side rasterization falls back to serif.

## License

MIT, see [`LICENSE`](LICENSE). ridge-redux is a port of ridge_map, which
is also MIT. Its copyright notice is kept in [`LICENSE-upstream`](LICENSE-upstream).

Elevation data: NASA [Shuttle Radar Topography Mission](https://www2.jpl.nasa.gov/srtm/),
available between 60°N and 60°S.
