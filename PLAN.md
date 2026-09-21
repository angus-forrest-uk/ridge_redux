# ridge-redux — Plan of Attack

> **Status: Phases 0–5 complete, plus the client-side orbit model and a
> rewritten lake mask (float gradient + water exclusion + connected-component
> coherence).** Both
> view modes are windows over a rotation-invariant disc of samples fetched
> once: the rectangle is a fixed-size frame (constant num_lines/style/ratios)
> sweeping over previously unused points; the disc shows the whole circle.
> Visible point count is constant at any angle (±<2% coastline wobble),
> zoom and distance are fixed, and JS/Rust pipelines match bit-for-bit. The browser
> now owns rotate/preprocess/draw (angle slider at frame rate, zero
> requests); the server ships the raw grid per location/resolution and keeps
> scipy-faithful semantics for exports (`fit: "reshape"`) alongside WYSIWYG
> plane exports (`fit: "plane"`). JS/Rust pipeline parity is proven
> bit-for-bit. WASM remains an optional drop-in (Phase 6). The golden
> parity test matches upstream bit-for-bit (0/24000 mismatches); the webapp renders, navigates and exports.
> Phase 6 items (server-side PNG, WASM, font embedding) are optional future work. The golden parity test matches upstream
> bit-for-bit (0/24000 mismatches); the webapp renders, navigates and exports.
> Phase 6 items (server-side PNG, WASM, font embedding) are optional future work.


A Rust port of [ridge_map](https://github.com/ColCarroll/ridge_map) that replaces
matplotlib with a webapp: the Rust backend owns the entire data pipeline and exposes
a JSON API; a minimal frontend canvas consumes the API and lets the user navigate,
rotate, and style the landscape into artwork.

---

## 1. What the original does (reverse-engineered, exact)

`ridge_map.py` pipeline, in order:

1. **Fetch** — `get_elevation_data(num_lines=80, elevation_pts=300, viewpoint_angle=0, crop=False, interpolation=0, lock_resolution=False)`
   - `srtm.get_image((elevation_pts, num_lines), lats, longs, 5280, mode='array')`
     samples a `(num_lines, elevation_pts)` float array. For row `r`, col `c`:
     ```
     lat = lat0 + r/num_lines   * (lat1 - lat0)     # note: /N, not /(N-1)
     lon = lon0 + c/elev_pts    * (lon1 - lon0)
     elevation = nearest grid point in the .hgt tile   (voids -> NaN)
     ```
   - If `viewpoint_angle % 360` is in `(45,135)` or `(225,315)` and not
     `lock_resolution`, **num_lines and elevation_pts are swapped** before sampling.
   - Then `scipy.ndimage.rotate(values, angle, reshape=not crop, order=interpolation)`
     — default `order=0` = nearest neighbor. `reshape=True` grows the canvas.
2. **Preprocess** — `preprocess(values, water_ntile=10, lake_flatness=3, vertical_ratio=40)`
   - NaNs → array min; min-max normalize to `[0,1]`.
   - Water mask: `values < percentile(values, water_ntile)` → NaN.
   - Lake mask: 3×3 morphological gradient of the u8-quantized image
     (`img_as_ubyte` = `round(v*255)`, then `local_max - local_min < lake_flatness`) → NaN.
   - Restore NaNs; **flip rows** (`values[-1::-1]`, north/south switch);
     multiply by `vertical_ratio`.
3. **Render** — `plot_map(...)`
   - For each row `i` (back to front): polyline `y = row - 6*i`, then
     `fill_between` baseline→line in `background_color` (occlusion trick —
     front lines hide back lines, producing the ridgeline look).
   - Color: constant (`"black"`, `"orange"`, …), or a colormap evaluated at
     `i / n_rows` (`kind="gradient"`), or per-segment by elevation
     (`kind="elevation"`, matplotlib LineCollection).
   - Label text (custom TTF font, default Cinzel) with a background box, optional
     `plot_annotation` lat/lon dot + text.

**SRTM.py details to replicate** (verified against `tkrajina/srtm.py` source):

- Mirrors: `https://srtm.kurviger.de/SRTM1/` (1 arc-sec, 3601×3601) and
  `.../SRTM3/` (3 arc-sec, 1201×1201). Files `{N|S}{lat:02}{E|W}{lon:03}.hgt(.zip)`.
- `.hgt` = row-major big-endian i16 grid, north-up: `square_side = sqrt(bytes/2)`,
  `res = 1/(side-1)`,
  `row = floor((lat_lo + 1 - lat) * (side-1))`, `col = floor((lon - lon_lo) * (side-1))`.
- Void / bad values: outside `[-1000, 10000]` → NaN.
- Disk cache of downloaded tiles (we'll do `~/.cache/ridge-redux/srtm/`).

---

## 2. Architecture

```
┌──────────────────────────── backend (Rust) ────────────────────────────┐
│  ridge-core (lib)                     ridge-server (bin, axum)         │
│  ├─ srtm: tile fetch + parse +        ├─ static file serving (web/)   │
│  │   disk cache                       ├─ POST /api/preview  → JSON    │
│  ├─ sample(bbox, n, p) -> Grid        │   geometry for canvas         │
│  ├─ rotate(grid, angle, …)            ├─ GET  /api/export.svg → SVG   │
│  ├─ preprocess(water, lake, vratio)   ├─ GET  /api/presets            │
│  ├─ geometry() -> polylines           └─ in-memory cache keyed by     │
│  ├─ colormaps (viridis, …)                params hash (tile + grid +  │
│  └─ svg writer                            processed grid)             │
└────────────────────────────────────────────────────────────────────────┘
                     │ JSON (rows of y-values, NaN→null)
┌────────────────────▼───────── frontend (vanilla JS, minimal) ──────────┐
│  index.html + main.js + style.css                                      │
│  ├─ canvas 2D: draw fills (bg color) + strokes, back-to-front = parity │
│  ├─ pan/zoom = pure view transform (no server calls)                   │
│  ├─ controls → API params (bbox, angle, n, p, water, lake, vratio,     │
│  │   linewidth, colors/kind, label, font size…) → debounced refetch    │
│  └─ export: fetch SVG; PNG = rasterize SVG in offscreen canvas         │
└────────────────────────────────────────────────────────────────────────┘
```

**The key decision:** the backend generates *geometry*, the frontend is a dumb
renderer. Pan/zoom (navigation) is a client-side view transform — instant and
free. Anything that changes the data (bbox, viewpoint angle, resolution,
preprocessing knobs) is a backend round-trip. Final artwork export is
server-rendered SVG (vector, print-ready). This keeps the frontend ~200 lines of
JS and honors "minimal frontend, backend-driven".

### Workspace layout

```
ridge-redux/
├── PLAN.md
├── Cargo.toml               # [workspace]
├── crates/
│   ├── ridge-core/          # pure lib, no I/O frameworks; unit-testable offline
│   │   └── src/{srtm.rs, grid.rs, rotate.rs, preprocess.rs, geometry.rs,
│   │           colormap.rs, svg.rs, lib.rs}
│   └── ridge-server/        # axum binary; caches; api handlers
├── web/                     # index.html, main.js, style.css (no build step)
├── fixtures/                # new_hampshire f64 bin + shape.json (from upstream npz)
├── scripts/convert_fixture.py   # one-time npz → .bin/.json (only place needing Python)
└── ridge_map/               # upstream repo, kept as reference
```

### Dependency choices

| Need | Crate | Notes |
|---|---|---|
| HTTP | `axum` + `tower-http` (fs, compression, trace) | serves API + `web/` |
| JSON | `serde`, `serde_json` | |
| Tiles | `reqwest` (rustls), `flate2` | .hgt.zip download + gunzip |
| Grids | `ndarray` | rotate/warp, percentile ops |
| Parallelism | `rayon` (optional) | 300×300 is small; only if needed |
| Colormaps | `colorous` | viridis/magma/…; plus hand-rolled named colors |
| SVG | hand-rolled writer | line art is trivial; no `svg` crate needed |
| PNG (later, optional) | `resvg`/`tiny-skia` | only if server-side raster wanted; v1 rasterizes client-side |

---

## 3. API design (drafted now, firmed in Phase 3)

All render-relevant params ride along in every call (stateless API, server
caches intermediate stages keyed by a params hash — mirroring the Python class's
"keep state so servers aren't hit too often").

```
GET  /                    → web/index.html
GET  /api/presets         → [{name, bbox, suggested params…}] (from upstream README)
POST /api/preview
     { bbox: [lon0,lat0,lon1,lat1], num_lines, elevation_pts,
       viewpoint_angle, crop, interpolation, lock_resolution,
       water_ntile, lake_flatness, vertical_ratio,
       linewidth, line_color|colormap, kind: "gradient"|"elevation",
       background_color, label, label_x, label_y, label_size, label_color }
     → 200 {
         shape: [rows, cols],
         rows: [[y|null, …], …],          // processed elevations; null = gap
         meta: {vmin, vmax, line_step},   // lets client scale view
         cache_key                        // for export parity
       }
GET  /api/export.svg?<same params>  → standalone SVG artwork
GET  /healthz
```

Errors: 400 with `{error}` for bad bbox / unsupported angle / fetch failure;
404 for unknown cache keys.

---

## 4. Phases

### Phase 0 — Foundations (½ day)
- `cargo init` workspace, `ridge-core` + `ridge-server` + `web/` skeleton.
- `scripts/convert_fixture.py`: `test/test_data/new_hampshire.npz` →
  `fixtures/new_hampshire.f64.bin` + shape JSON (Python used once, here only).
- Copy license/attribution (MIT, upstream), README stub.

### Phase 1 — ridge-core: data (1–2 days)
- `srtm.rs`: tile name math, disk cache (`~/.cache/ridge-redux/srtm/`), download
  (zipped or raw), big-endian i16 parse, `square_side` detection, void → NaN,
  nearest-neighbor `elevation(lat, lon)` exactly as SRTM.py.
  Configurable mirror base URL + "offline fixture" mode.
- `grid.rs`: `sample(bbox, num_lines, elevation_pts)` reproducing
  `get_image(mode='array')` (incl. the `r/N` sampling convention).
- Tests against fixture: known elevations (Mount Washington ≈ 1917 m), NaN
  propagation, off-by-one parity vs. a Python-dumped reference array.

### Phase 2 — ridge-core: transform + geometry (2–3 days)
- `rotate.rs`: affine rotation with `reshape` growth math, `crop` mode,
  nearest (order 0) + bilinear (order 1) sampling; angle-swap logic from
  `get_elevation_data`. Compare shapes/rough content vs. scipy on the fixture.
- `preprocess.rs`: normalize, percentile (numpy 'linear' method), u8 quantize,
  3×3 morphological gradient, water/lake masks, row flip, vertical ratio.
- `geometry.rs`: rows → `(points, baseline)` polylines with the `−6·i` step;
  color resolution (constant / gradient-by-row / elevation-per-segment).
- `svg.rs`: background rect + fills + strokes + text label + annotation dot.
  Snapshot test the SVG string on fixture data.
- **Milestone:** CLI `cargo run -p ridge-core --bin render -- --bbox … > out.svg`
  produces a White-Mountains-style image visually matching upstream
  `examples/white_mountains.png`.

### Phase 3 — ridge-server (1–2 days)
- axum app: static serving, `/api/preview`, `/api/export.svg`, `/api/presets`,
  health; params struct with serde + validation; two-tier cache
  (tile cache on disk; `Grid` + processed rows in a `DashMap`/`Mutex<HashMap>`
  keyed by params hash) with a small LRU cap.
- Integration tests: start router with fixture-mode srtm, hit endpoints,
  assert JSON shape/values and SVG snapshot.
- gzip via tower-http compression layer (rows can be ~1 MB JSON).

### Phase 4 — Frontend (2–3 days)
- Single page: canvas + a compact control panel (all state → API params).
- Canvas renderer: iterate rows back→front, fill baseline→line polygon in bg
  color, stroke line — replicates matplotlib occlusion exactly.
- Navigation: drag = pan view, wheel/pinch = zoom (pure transform);
  viewpoint-angle slider + num_lines/elevation_pts/water/lake/vratio controls
  → debounced (300 ms) `/api/preview` refetch.
- Label: text overlay positioned by `label_x/label_y` in view space (or bake
  into export only); Cinzel loaded from Google Fonts.
- Export bar: "Download SVG" (`/api/export.svg`), "Download PNG"
  (client rasterizes the SVG blob). Copy-permalink button (params in URL hash).
- Optional (only if time allows): tiny Leaflet bbox-picker; otherwise lon/lat
  number inputs + preset dropdown are enough for v1.

### Phase 5 — Parity & polish (1–2 days)
- Side-by-side comparison against upstream examples (austin, hawaii,
  karwendelgebirge) using the same bbox/params; tune constants.
- Colormap gallery (viridis, ocean/spring-like, cool), `kind="elevation"`
  per-segment coloring.
- Annotations (`plot_annotation` parity), background picker, dark mode.
- Error UX (ocean bbox → friendly message), rate-limit/timeout tile fetches.
- README with screenshots + `cargo run -p ridge-server --release` quickstart.

### Phase 6 — Optional extensions
- Server-side PNG via `resvg` with text→path (fully self-contained SVG/PNG).
- WebSocket push for long fetches; job queue for huge `num_lines`.
- WASM build of ridge-core for fully client-side preprocessing tweaks.

---

## 5. Risks & gotchas (found while reading the source)

1. **The angle/axis swap** in `get_elevation_data` (45–135°/225–315° swaps
   `num_lines`↔`elevation_pts`) is easy to miss; it changes the sampled aspect
   ratio, not just rotation. Reproduce verbatim.
2. **scipy `reshape` growth**: rotating with `reshape=True` expands the canvas
   (new size = `h·|cos|+w·|sin|` etc.); `crop=False` is the default upstream.
   Must replicate or maps will clip. Nearest-neighbor (default `order=0`)
   keeps this simple; bilinear needs edge handling.
3. **NaN semantics everywhere**: gaps (water/lakes) are NaN, and `fill_between`
   still draws the baseline across gaps — the "dashes" in the renders. Keep
   NaN→`null` in JSON and handle gaps in canvas/SVG identically.
4. **numpy percentile** uses linear interpolation between order statistics —
   don't use nearest-rank or maps shift subtly.
5. **`img_as_ubyte` quantization** before the lake gradient: compute on
   `round(v*255)` u8, not on floats, or `lake_flatness` thresholds won't match.
6. **Mirror availability**: `srtm.kurviger.de` is a volunteer mirror. Make the
   base URL + SRTM1/SRTM3 choice configurable; the disk cache means one fetch
   per tile ever (same tiles reused across parameter tweaks).
7. **JSON size**: 300×300 rows of `y` values ≈ 0.5–1 MB; round to 4 decimals,
   gzip, and only ship `y` (x is implicit) to keep previews snappy.
8. **Fonts in SVG export**: v1 references Google-Fonts-hosted Cinzel (renders
   in any browser); true standalone export (font embedded as base64 or
   text→path) is a Phase 6 nicety.

## 6. Testing strategy

- **Offline-first**: everything in ridge-core is testable from `fixtures/`
  without network (the upstream conftest.py FakeSRTM pattern, ported).
- Unit tests per module (tile parse, sampling, rotate shapes, percentile,
  lake mask, geometry step size).
- Golden tests: Python script (Phase 0) dumps reference arrays
  (sampled grid, preprocessed grid) from the real stack; Rust asserts
  near-equality (tolerances for f64 noise).
- SVG snapshot tests for regressions.
- axum integration tests for the API with fixture mode.
