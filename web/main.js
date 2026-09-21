/* ridge-redux frontend: a thin canvas renderer over the backend API.
 *
 * Division of labor:
 *   - The SERVER samples the raw SRTM grid for a bbox/resolution
 *     (POST /api/elevation) — the only round-trip that needs network.
 *   - The BROWSER rotates that grid about its center, masks water/lakes,
 *     derives ridge rows and draws them. The viewpoint angle, water and
 *     relief knobs therefore update instantly, with zero requests.
 *   - Export posts the same params (fit: "plane") so the SVG matches the
 *     canvas exactly.
 */
"use strict";

const $ = (id) => document.getElementById(id);
const canvas = $("canvas");
const ctx = canvas.getContext("2d");

// ---------------------------------------------------------------- state ---

const DEFAULTS = {
  bbox: [-71.928864, 43.758201, -70.957947, 44.465151],
  // Original upstream parameters: 80 lines x 300 points over the bbox —
  // the window (anisotropic) keeps this composition at every angle.
  num_lines: 80,
  elevation_pts: 300,
  viewpoint_angle: 0,
  interpolation: 0,
  water_ntile: 10,
  lake_flatness: 3,
  vertical_ratio: 40,
  linewidth_pt: 2,
  line_color: "black",
  kind: "gradient",
  background_color: "#ece8ec",
  size_scale: 20,
  label: "The White\nMountains",
  label_x: 0.62,
  label_y: 0.15,
  label_size_pt: 60,
  label_font: "Cinzel",
  annotation: null,
  fit: "plane", // export matches the browser's fixed-plane rotation
  region: "rect",  // "rect" = the bbox; "disc" = rotation-invariant circle
  span_deg: 0,     // disc side in degrees; 0 = bbox diagonal (nothing cut)
};

let params = { ...DEFAULTS };
let raw = null;            // {nrows, ncols, values, bboxRatio, crop}
let prepared = null;       // preprocessed (masked/flipped/scaled) disc grid
let scene = null;          // locally computed drawing model
let view = null;           // {scale, tx, ty} figure-px -> canvas-px
let fetchSeq = 0;
let computeQueued = false;

// ------------------------------------------------------- local pipeline ---

const LINE_SPACING = 6;
const SUBPLOT = { left: 0.125, right: 0.9, bottom: 0.11, top: 0.88 };
const AXES_MARGIN = 0.05;

/* Rotate the grid about its center within the same canvas (nearest
 * neighbor). Samples outside the source become NaN gaps — the plane never
 * grows, so zoom and distance stay fixed while the terrain spins. Mirrors
 * ridge_core::rotate::rotate_fixed_plane. */
function rotatePlane(src, nrows, ncols, angleDeg) {
  const a = ((angleDeg % 360) + 360) % 360;
  if (a === 0) return src;
  const rad = (a * Math.PI) / 180;
  const c = Math.cos(rad), s = Math.sin(rad);
  const cr = (nrows - 1) / 2, cc = (ncols - 1) / 2;
  const out = new Float64Array(nrows * ncols).fill(NaN);
  for (let r = 0; r < nrows; r++) {
    const dr = r - cr;
    for (let col = 0; col < ncols; col++) {
      const pr = c * dr + s * (col - cc) + cr;
      const pc = -s * dr + c * (col - cc) + cc;
      const sr = Math.floor(pr + 0.5), sc = Math.floor(pc + 0.5);
      if (sr >= 0 && sr < nrows && sc >= 0 && sc < ncols) {
        out[r * ncols + col] = src[sr * ncols + sc];
      }
    }
  }
  return out;
}

/* Port of ridge_core::preprocess::preprocess: NaN->min, normalize,
 * percentile water mask, lake mask (float gradient, water-excluded,
 * component-coherent), flip rows, vertical exaggeration.
 *
 * ALL threshold decisions happen here — on the UNROTATED disc, where every
 * cell is a fixed physical location — so water bodies and hills hold still
 * as the view rotates. Returns the display-ready grid (masked, flipped,
 * scaled) plus vmin/vmax. */
function preprocessGrid(src, nrows, ncols, waterNtile, lakeFlatness, vratio, flatnessScale, statsBbox, dLat0, dLon0, dSpan) {
  const n = nrows * ncols;
  let min = Infinity, max = -Infinity;
  const nanMask = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    const v = src[i];
    if (Number.isNaN(v)) { nanMask[i] = 1; continue; }
    if (v < min) min = v;
    if (v > max) max = v;
  }
  if (!Number.isFinite(min)) return null; // no data at all
  const span = max - min;
  const norm = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    // Upstream fills NaN cells with the min before normalizing.
    const v = nanMask[i] ? min : src[i];
    norm[i] = span > 0 ? (v - min) / span : 0;
  }

  // Water level: numpy-style linear percentile.
  // Percentile over REAL terrain cells only — NaN padding (disc corners)
  // must not drag the water level to zero (that would hide all rivers) —
  // and, when a reference region is given (the rect footprint), over that
  // region only, matching upstream's window-scoped water level. The region
  // is a fixed physical footprint, so decisions stay rotation-stable.
  const inStats = (i) => {
    if (nanMask[i]) return false;
    if (!statsBbox) return true;
    const r = (i / ncols) | 0, c = i % ncols;
    const lat = dLat0 + ((r + 0.5) / nrows) * dSpan;
    const lon = dLon0 + ((c + 0.5) / ncols) * dSpan;
    return lat >= statsBbox[1] && lat <= statsBbox[3]
        && lon >= statsBbox[0] && lon <= statsBbox[2];
  };
  const finite = [];
  for (let i = 0; i < n; i++) if (inStats(i)) finite.push(norm[i]);
  finite.sort((a, b) => a - b);
  const pick = (q) => {
    if (finite.length === 1) return finite[0];
    const idx = (q / 100) * (finite.length - 1);
    const lo = Math.floor(idx), hi = Math.ceil(idx);
    return lo === hi ? finite[lo] : finite[lo] + (finite[hi] - finite[lo]) * (idx - lo);
  };
  const waterLevel = pick(Math.min(100, Math.max(0, waterNtile)));

  // Lake mask. Mirrors ridge_core::preprocess::preprocess:
  //  1. gradient on the NORMALIZED FLOATS (threshold lakeFlatness/255) —
  //     same semantics as upstream's u8 rank gradient, without the uint8
  //     rounding that speckled holes across rolling hills;
  //  2. water/NaN cells EXCLUDED from the neighborhoods (skimage's `mask`
  //     parameter) so flat shore merges into the water body instead of
  //     forming a drawn perimeter around it;
  //  3. candidates kept only in connected components >= MIN_LAKE_COMPONENT.
  const isWater = new Uint8Array(n);
  for (let i = 0; i < n; i++) isWater[i] = nanMask[i] ? 0 : (norm[i] < waterLevel ? 1 : 0);
  const excluded = new Uint8Array(n);
  for (let i = 0; i < n; i++) excluded[i] = nanMask[i] || isWater[i] ? 1 : 0;
  const grad = new Float32Array(n);
  for (let r = 0; r < nrows; r++) {
    for (let c = 0; c < ncols; c++) {
      let lo = Infinity, hi = -Infinity, any = false;
      const r0 = Math.max(0, r - 1), r1 = Math.min(nrows - 1, r + 1);
      const c0 = Math.max(0, c - 1), c1 = Math.min(ncols - 1, c + 1);
      for (let rr = r0; rr <= r1; rr++) {
        for (let cc = c0; cc <= c1; cc++) {
          const i2 = rr * ncols + cc;
          if (excluded[i2]) continue;
          const v = norm[i2];
          if (v < lo) lo = v;
          if (v > hi) hi = v;
          any = true;
        }
      }
      grad[r * ncols + c] = any ? hi - lo : 0;
    }
  }
  const threshold = (lakeFlatness / 255) * (flatnessScale || 1);
  const candidate = new Uint8Array(n);
  for (let i = 0; i < n; i++) candidate[i] = !excluded[i] && grad[i] < threshold ? 1 : 0;
  // Connected components (4-conn), keep size >= 12.
  const MIN_LAKE_COMPONENT = 12;
  const isLake = new Uint8Array(n);
  const visited = new Uint8Array(n);
  const comp = new Int32Array(n);
  for (let start = 0; start < n; start++) {
    if (!candidate[start] || visited[start]) continue;
    let sp = 0;
    comp[sp++] = start;
    visited[start] = 1;
    let count = 1;
    while (sp > 0) {
      const i = comp[--sp];
      const r = (i / ncols) | 0, c = i % ncols;
      for (const [dr, dc] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) {
        const rr = r + dr, cc = c + dc;
        if (rr < 0 || cc < 0 || rr >= nrows || cc >= ncols) continue;
        const j = rr * ncols + cc;
        if (candidate[j] && !visited[j]) { visited[j] = 1; comp[sp++] = j; count++; }
      }
    }
    if (count >= MIN_LAKE_COMPONENT) for (let k = 0; k < count; k++) isLake[comp[k]] = 1;
  }

  // Apply masks, flip north/south, exaggerate vertically -> display grid.
  const grid = new Float64Array(nrows * ncols);
  let vmin = Infinity, vmax = -Infinity;
  for (let r = 0; r < nrows; r++) {
    const sr = nrows - 1 - r; // upstream values[-1::-1]
    for (let c = 0; c < ncols; c++) {
      const i = sr * ncols + c;
      grid[r * ncols + c] =
        nanMask[i] || norm[i] < waterLevel || isLake[i] ? NaN : norm[i] * vratio;
    }
  }
  if (!Number.isFinite(vmin)) { vmin = 0; vmax = 1; }
  return { grid, vmin, vmax };
}

/* Ridge rows from a display-ready grid: row i is drawn at
 * y = value - LINE_SPACING * i. */
function buildRows(grid, nrows, ncols) {
  const rows = [];
  for (let i = 0; i < nrows; i++) {
    const baseline = -LINE_SPACING * i;
    const y = new Array(ncols);
    for (let c = 0; c < ncols; c++) y[c] = grid[i * ncols + c] + baseline;
    rows.push({ baseline, y });
  }
  return rows;
}

/* Figure layout (matplotlib parity): figure size, subplot rect, and data
 * limits hugging the ACTUAL drawn content (autoscale with 5% margins). The
 * bounds are re-measured after every local rotation, so the camera keeps the
 * landscape centered at a constant apparent size while it spins — an orbit
 * with the subject kept in frame. */
function buildLayout(bounds, sizeScale, bboxRatio) {
  const dx = (bounds.xmax - bounds.xmin) * AXES_MARGIN;
  const dy = (bounds.ymax - bounds.ymin) * AXES_MARGIN;
  const width_px = sizeScale * 100;
  const height_px = sizeScale * bboxRatio * 100;
  return {
    width_px, height_px,
    axes: [
      SUBPLOT.left * width_px,
      (1 - SUBPLOT.top) * height_px,
      SUBPLOT.right * width_px,
      (1 - SUBPLOT.bottom) * height_px,
    ],
    xlim: [bounds.xmin - dx, bounds.xmax + dx],
    ylim: [bounds.ymin - dy, bounds.ymax + dy],
  };
}

/* Content bounds of a processed row set: x = finite column extent,
 * y = lowest baseline with data (fills reach it) up to the highest point. */
function contentBounds(rows) {
  let xmin = Infinity, xmax = -Infinity, ymin = Infinity, ymax = -Infinity;
  for (const row of rows) {
    let hasData = false;
    for (let c = 0; c < row.y.length; c++) {
      const y = row.y[c];
      if (Number.isFinite(y)) {
        hasData = true;
        if (y > ymax) ymax = y;
        if (c < xmin) xmin = c;
        if (c > xmax) xmax = c;
      }
    }
    if (hasData && row.baseline < ymin) ymin = row.baseline;
  }
  return { xmin, xmax, ymin, ymax, empty: xmin === Infinity };
}

/* Recompute the whole drawing model locally: rotate -> preprocess -> rows.
 * No network involved. */
/* Anisotropic display window over the rotated underlying grid: covers the
 * original bbox extent at num_lines x elevation_pts samples (upstream cell
 * shape), reading nearest nodes. The window dims never change with the
 * angle, so style and ratios are constant; the underlying disc supplies
 * previously unused points as the frame sweeps around. Mirrors
 * ridge_core::grid::sample_window. */
function sampleWindow(
  grid, dn, dLat0, dLon0, dSpan,
  bbox, numLines, elevationPts,
) {
  const out = new Float64Array(numLines * elevationPts).fill(NaN);
  const step = dSpan / dn;
  const dLat = bbox[1] - bbox[3] === 0 ? 0 : bbox[3] - bbox[1];
  for (let i = 0; i < numLines; i++) {
    const lat = bbox[1] + (i / numLines) * (bbox[3] - bbox[1]);
    for (let j = 0; j < elevationPts; j++) {
      const lon = bbox[0] + (j / elevationPts) * (bbox[2] - bbox[0]);
      const r = Math.round((lat - dLat0) / step);
      const c = Math.round((lon - dLon0) / step);
      if (r >= 0 && c >= 0 && r < dn && c < dn) {
        out[i * elevationPts + j] = grid[r * dn + c];
      }
    }
  }
  return out;
}

/* Make sure the water/lake decisions exist for the current data + water
 * knobs. Runs on fetch and when water/relief knobs move — NOT on angle
 * changes, which only rotate the pre-decided grid. */
function ensurePrepared() {
  if (prepared || !raw) return true;
  // Density compensation: the flatness threshold is expressed at the
  // display sampling (dlat/num_lines), so the disc-space decision measures
  // the same terrain slope as upstream's 80x300 composition. Disc mode
  // displays at disc density: scale 1.
  const [lon0, lat0, lon1, lat1] = params.bbox;
  const dSpan = params.span_deg || Math.hypot(lon1 - lon0, lat1 - lat0);
  const dStep = dSpan / raw.nrows;
  const refStep = params.region === "disc"
    ? dStep
    : (lat1 - lat0) / params.num_lines;
  const statsBbox = params.region === "disc" ? null : params.bbox;
  const pp = preprocessGrid(
    raw.values, raw.nrows, raw.ncols,
    params.water_ntile, params.lake_flatness, params.vertical_ratio,
    dStep / refStep, statsBbox,
    (lat0 + lat1) / 2 - dSpan / 2, (lon0 + lon1) / 2 - dSpan / 2, dSpan,
  );
  if (!pp) return false;
  prepared = pp;
  return true;
}

function recompute() {
  if (!raw) return;
  if (!ensurePrepared()) { setStatus("no data for this view", false); return; }

  // Rotate the pre-decided grid (negative angle: preprocess flips rows, and
  // R(-t)*F == F*R(t)) and sample the display window. Masks rotate rigidly
  // with the terrain — nothing flickers.
  const rotated = rotatePlane(
    prepared.grid, raw.nrows, raw.ncols, -params.viewpoint_angle,
  );
  const [lon0, lat0, lon1, lat1] = params.bbox;
  const dSpan = params.span_deg || Math.hypot(lon1 - lon0, lat1 - lat0);
  const dLat0 = (lat0 + lat1) / 2 - dSpan / 2;
  const dLon0 = (lon0 + lon1) / 2 - dSpan / 2;
  let view_grid, vrows, vcols;
  if (params.region === "disc") {
    view_grid = rotated; vrows = raw.nrows; vcols = raw.ncols;
  } else {
    vrows = Math.min(params.num_lines, raw.nrows);
    vcols = Math.min(params.elevation_pts, raw.ncols);
    view_grid = sampleWindow(
      rotated, raw.nrows, dLat0, dLon0, dSpan,
      params.bbox, vrows, vcols,
    );
  }
  const rows = buildRows(view_grid, vrows, vcols);
  const bounds = contentBounds(rows);
  if (bounds.empty) { setStatus("no data for this view", false); return; }
  const bboxRatio = params.region === "disc" ? 1.0 : (lat1 - lat0) / (lon1 - lon0);
  scene = {
    rows,
    vmin: prepared.vmin,
    vmax: prepared.vmax,
    layout: buildLayout(bounds, params.size_scale, bboxRatio),
    style: buildStyle(),
  };
  if (!view) fitView();
  draw();
}

// Colors: named values + hex, mirroring the backend's colormap module.
const NAMED_COLORS = {
  black: [0, 0, 0], white: [255, 255, 255], orange: [255, 165, 0],
  red: [255, 0, 0], green: [0, 128, 0], blue: [0, 0, 255],
  purple: [128, 0, 128], brown: [165, 42, 42], pink: [255, 192, 203],
  gray: [128, 128, 128], grey: [128, 128, 128], navy: [0, 0, 128],
  teal: [0, 128, 128], crimson: [220, 20, 60],
};
function parseColor(name) {
  if (typeof name !== "string") return [0, 0, 0];
  const lower = name.toLowerCase();
  if (NAMED_COLORS[lower]) return NAMED_COLORS[lower];
  const h = lower.replace(/^#/, "");
  if (/^[0-9a-f]{6}$/.test(h)) {
    const v = parseInt(h, 16);
    return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
  }
  return [0, 0, 0];
}
const IS_COLORMAP = (name) =>
  Object.prototype.hasOwnProperty.call(MPL_FORMULAS, name) ||
  Object.prototype.hasOwnProperty.call(CMAP_TABLES, name);

function resolveLineColor(name) {
  if (IS_COLORMAP(name.toLowerCase())) return { type: "map", name: name.toLowerCase() };
  return { type: "solid", rgb: parseColor(name) };
}

function buildStyle() {
  const line = resolveLineColor(params.line_color);
  const labelColor = params.label_color
    ? parseColor(params.label_color)
    : line.type === "solid"
      ? line.rgb
      : evalCmap(line.name, 0);
  const style = {
    line,
    kind: params.kind,
    background: parseColor(params.background_color),
    linewidth_pt: params.linewidth_pt,
    size_scale: params.size_scale,
    label: null,
    annotation: null,
  };
  if (params.label) {
    style.label = {
      text: params.label,
      color: labelColor,
      x: params.label_x,
      y: params.label_y,
      size_pt: params.label_size_pt,
      vertical_alignment: "bottom",
      font_family: params.label_font,
      background: true,
    };
  }
  if (params.annotation) {
    const [lon0, lat0, lon1, lat1] = params.bbox;
    style.annotation = {
      label: params.annotation.label || "",
      x: (params.annotation.lon - lon0) / (lon1 - lon0),
      y: (params.annotation.lat - lat0) / (lat1 - lat0),
      x_offset: 0.005, y_offset: 0.005,
      label_size_pt: params.annotation.label_size_pt ?? 20,
      dot_pt: params.annotation.dot_pt ?? 8,
      color: params.annotation.color ? parseColor(params.annotation.color) : labelColor,
      background: params.annotation.background ?? false,
    };
  }
  return style;
}

// ------------------------------------------------------------- controls ---

function el(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  for (const child of children) node.append(child);
  return node;
}

function section(title, ...rows) {
  return el("div", { class: "section" }, [el("h2", {}, [title]), ...rows]);
}

/* Slider bound to a *local* recompute (no network). */
function sliderRow(label, key, min, max, step) {
  const val = el("span", { class: "val" }, [String(params[key])]);
  const input = el("input", { type: "range", min, max, step, value: params[key] });
  input.addEventListener("input", () => {
    params[key] = Number(input.value);
    val.textContent = String(input.value);
    scheduleRecompute();
  });
  return el("div", { class: "row" }, [el("label", {}, [label]), input, val]);
}

/* Slider that reshapes the sampled data — triggers an elevation refetch. */
function dataSliderRow(label, key, min, max, step) {
  const val = el("span", { class: "val" }, [String(params[key])]);
  const input = el("input", { type: "range", min, max, step, value: params[key] });
  input.addEventListener("input", () => {
    params[key] = Number(input.value);
    val.textContent = String(input.value);
    scheduleRefetch();
  });
  return el("div", { class: "row" }, [el("label", {}, [label]), input, val]);
}

function colorRow(label, key) {
  const input = el("input", { type: "color", value: toHex(params[key]) });
  const text = el("input", { type: "text", value: params[key] });
  input.addEventListener("input", () => {
    params[key] = input.value;
    text.value = input.value;
    scheduleRecompute();
  });
  text.addEventListener("change", () => {
    params[key] = text.value;
    input.value = toHex(text.value) || input.value;
    scheduleRecompute();
  });
  return el("div", { class: "row" }, [el("label", {}, [label]), input, text]);
}

function toHex(c) {
  if (/^#[0-9a-f]{6}$/i.test(c)) return c;
  const named = { black: "#000000", white: "#ffffff", orange: "#ffa500", red: "#ff0000", blue: "#0000ff", green: "#008000" };
  return named[c.toLowerCase()] || "#000000";
}

const COLOR_DATALIST_NAMES = [
  "black", "white", "orange", "red", "navy", "teal", "crimson", "#414a4c",
  "viridis", "magma", "inferno", "plasma", "cividis",
  "spring", "summer", "autumn", "winter", "cool", "bone", "ocean", "gnuplot",
];

function colorOrMapRow(label, key) {
  const datalist = el("datalist", { id: "colors" });
  for (const name of COLOR_DATALIST_NAMES) datalist.append(el("option", { value: name }));
  const input = el("input", { type: "text", list: "colors", value: params[key] });
  input.addEventListener("change", () => {
    params[key] = input.value;
    scheduleRecompute();
  });
  return el("div", { class: "row" }, [el("label", {}, [label]), input, datalist]);
}

function buildControls() {
  const c = $("controls");
  c.replaceChildren();

  bboxInputs = params.bbox.map((v) =>
    el("input", { type: "number", step: "0.000001", value: v, style: "width:86px" })
  );
  bboxInputs.forEach((input, i) =>
    input.addEventListener("change", () => {
      // keep latitude inside the SRTM coverage window
      if (i === 1 || i === 3) input.value = clampLat(Number(input.value));
      params.bbox[i] = Number(input.value);
      // span follows the bbox (the span slider can still override after)
      const [lo0, la0, lo1, la1] = params.bbox;
      params.span_deg = Number(Math.hypot(lo1 - lo0, la1 - la0).toFixed(6));
      buildControls();
      updateMapRect();
      scheduleRefetch();
    })
  );

  const presetSel = $("preset");
  const regionSel = el("select", {},
    [el("option", { value: "rect" }, ["rectangle (orbiting window)"]), el("option", { value: "disc" }, ["full disc"])]);
  if (params.region !== "disc" && !params.span_deg) {
    const [lon0, lat0, lon1, lat1] = params.bbox;
    params.span_deg = Number(Math.hypot(lon1 - lon0, lat1 - lat0).toFixed(4));
  }
  regionSel.value = params.region;
  regionSel.addEventListener("change", () => {
    params.region = regionSel.value;
    buildControls();
    scheduleRefetch();
  });
  const spanRow = sliderRow("region span (deg)", "span_deg", 0.05, 5, 0.01);
  spanRow.style.display = params.region === "disc" ? "" : "none";
  c.append(
    section(
      "location",
      el("div", { class: "row bbox" }, bboxInputs),
      el("div", { class: "row" }, [el("label", {}, ["region"]), regionSel]),
      spanRow,
    ),
    section(
      "viewpoint",
      sliderRow("angle (deg)", "viewpoint_angle", 0, 360, 1),
      (() => {
        const sel = el("select", {},
          [el("option", { value: "0" }, ["nearest (0)"]), el("option", { value: "1" }, ["bilinear (1)"])]);
        sel.value = String(params.interpolation);
        sel.addEventListener("change", () => {
          params.interpolation = Number(sel.value);
          scheduleRecompute();
        });
        return el("div", { class: "row" }, [el("label", {}, ["interpolation"]), sel]);
      })(),
    ),
    section(
      "resolution",
      dataSliderRow("num lines", "num_lines", 10, 400, 5),
      dataSliderRow("pts / line", "elevation_pts", 20, 1000, 10),
    ),
    section(
      "water & relief",
      sliderRow("water ntile", "water_ntile", 0, 100, 1),
      sliderRow("lake flatness", "lake_flatness", 0, 10, 1),
      sliderRow("vertical ratio", "vertical_ratio", 5, 400, 5),
    ),
    section(
      "style",
      colorOrMapRow("line color", "line_color"),
      (() => {
        const sel = el("select", {},
          [el("option", { value: "gradient" }, ["gradient"]), el("option", { value: "elevation" }, ["elevation"])]);
        sel.value = params.kind;
        sel.addEventListener("change", () => {
          params.kind = sel.value;
          scheduleRecompute();
        });
        return el("div", { class: "row" }, [el("label", {}, ["colormap kind"]), sel]);
      })(),
      sliderRow("linewidth (pt)", "linewidth_pt", 0.5, 10, 0.5),
      colorRow("background", "background_color"),
      sliderRow("size scale", "size_scale", 8, 40, 1),
    ),
    section(
      "label",
      (() => {
        const ta = el("textarea", { rows: 2 });
        ta.value = params.label;
        ta.addEventListener("change", () => {
          params.label = ta.value;
          scheduleRecompute();
        });
        return ta;
      })(),
      sliderRow("label x", "label_x", 0, 1, 0.01),
      sliderRow("label y", "label_y", 0, 1, 0.01),
      sliderRow("label size", "label_size_pt", 10, 120, 2),
    ),
    section(
      "annotation",
      (() => {
        const wrap = el("div");
        const lon = el("input", { type: "number", step: "0.0001", placeholder: "lon", value: params.annotation?.lon ?? "" });
        const lat = el("input", { type: "number", step: "0.0001", placeholder: "lat", value: params.annotation?.lat ?? "" });
        const text = el("input", { type: "text", placeholder: "label", value: params.annotation?.label ?? "" });
        const clear = el("button", { onclick: () => { params.annotation = null; lon.value = lat.value = text.value = ""; scheduleRecompute(); } }, ["clear"]);
        const changed = () => {
          if (lon.value === "" || lat.value === "") { params.annotation = null; }
          else {
            params.annotation = {
              lon: Number(lon.value), lat: Number(lat.value), label: text.value,
              x_offset: 0.005, y_offset: 0.005, label_size_pt: 20, dot_pt: 8,
              color: "#ffffff", background: false,
            };
          }
          scheduleRecompute();
        };
        for (const inp of [lon, lat, text]) inp.addEventListener("change", changed);
        wrap.append(
          el("div", { class: "row" }, [lon, lat]),
          el("div", { class: "row" }, [text, clear]),
        );
        return wrap;
      })(),
    ),
    section(
      "about",
      el("div", { class: "check-row" }, [
        el("label", {}, [
          "rotate / water / relief / style update locally — only location and resolution hit the server.",
        ]),
      ]),
    ),
  );
}

// ---------------------------------------------------------------- fetch ---

let refetchTimer = null;
function scheduleRefetch() {
  setStatus("queued…", true);
  clearTimeout(refetchTimer);
  refetchTimer = setTimeout(refetchElevation, 350);
  updateHash();
}

/* One server round-trip: the raw sampled elevation grid. Rotation and
 * preprocessing happen locally, so this only runs when the location or
 * resolution changes. */
async function refetchElevation() {
  const seq = ++fetchSeq;
  setStatus("fetching elevation…", true);
  const t0 = performance.now();
  try {
    if (!params.span_deg) {
      const [lon0, lat0, lon1, lat1] = params.bbox;
      params.span_deg = Number(Math.hypot(lon1 - lon0, lat1 - lat0).toFixed(4));
    }
    const resp = await fetch("/api/elevation", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        bbox: params.bbox,
        num_lines: params.num_lines,
        elevation_pts: params.elevation_pts,
        region: params.region,
        span_deg: params.span_deg,
      }),
    });
    if (seq !== fetchSeq) return; // stale
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: resp.statusText }));
      setStatus(`error: ${err.error}`, false);
      return;
    }
    const data = await resp.json();
    const [nrows, ncols] = data.shape;
    const values = new Float64Array(nrows * ncols);
    for (let r = 0; r < nrows; r++) {
      for (let c = 0; c < ncols; c++) {
        const v = data.values[r][c];
        values[r * ncols + c] = v === null ? NaN : v;
      }
    }
    raw = { nrows, ncols, values };
    prepared = null; // new data: re-run the water/lake decisions
    scene = null;
    view = null; // new window: re-fit once
    recompute();
    const ms = Math.round(performance.now() - t0);
    setStatus(`${params.num_lines} × ${params.elevation_pts} window · ${ms} ms · angle is local`);
  } catch (e) {
    setStatus(`error: ${e.message}`, false);
  }
}

/* Local recompute after a knob that changes the preprocessed grid
 * (water/relief) — decisions re-run once, still no network. */
function scheduleRecompute() {
  prepared = null;
  if (computeQueued) return;
  computeQueued = true;
  requestAnimationFrame(() => {
    computeQueued = false;
    recompute();
  });
  updateHash();
}

function setStatus(text, busy) {
  const s = $("status");
  s.textContent = text;
  s.classList.toggle("busy", !!busy);
}

// ----------------------------------------------------------------- draw ---

function resizeCanvas() {
  const dpr = window.devicePixelRatio || 1;
  const { width, height } = canvas.getBoundingClientRect();
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  if (scene) draw();
}

function fitView() {
  const cw = canvas.width, ch = canvas.height;
  const fw = scene.layout.width_px, fh = scene.layout.height_px;
  const scale = Math.min(cw / fw, ch / fh) * 0.95;
  view = {
    scale: scale,
    tx: (cw - fw * scale) / 2,
    ty: (ch - fh * scale) / 2,
  };
}

function draw() {
  if (!scene) return;
  const { width_px: fw, height_px: fh, axes } = scene.layout;

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(view.scale, 0, 0, view.scale, view.tx, view.ty);

  // Figure background.
  ctx.fillStyle = cssColor(scene.style.background);
  ctx.fillRect(0, 0, fw, fh);

  // Clip to the axes rect (matplotlib behavior).
  ctx.save();
  ctx.beginPath();
  ctx.rect(axes[0], axes[1], axes[2] - axes[0], axes[3] - axes[1]);
  ctx.clip();

  const line = scene.style.line;
  const kind = scene.style.kind;
  const lw = lineWidthPx();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  const rows = scene.rows;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const runs = computeRuns(row.y);
    if (runs.length === 0) continue;

    // Fill: baseline -> curve, per run (occlusion trick).
    ctx.fillStyle = cssColor(scene.style.background);
    for (const [a, b] of runs) {
      ctx.beginPath();
      const [x0, yb] = toCanvas(a, row.baseline);
      ctx.moveTo(x0, yb);
      for (let k = a; k < b; k++) {
        const [x, y] = toCanvas(k, row.y[k]);
        ctx.lineTo(x, y);
      }
      const [x1] = toCanvas(b - 1, row.baseline);
      ctx.lineTo(x1, yb);
      ctx.closePath();
      ctx.fill();
    }

    // Stroke.
    if (kind === "elevation" && line.type === "map") {
      const span = scene.vmax - scene.vmin || 1;
      const BUCKETS = 24;
      const paths = Array.from({ length: BUCKETS }, () => null);
      for (const [a, b] of runs) {
        if (b - a < 2) continue;
        for (let k = a + 1; k < b; k++) {
          const t = Math.min(BUCKETS - 1, Math.max(0,
            Math.floor(((row.y[k - 1] - row.baseline - scene.vmin) / span) * BUCKETS)));
          const [x0, y0] = toCanvas(k - 1, row.y[k - 1]);
          const [x1, y1] = toCanvas(k, row.y[k]);
          if (!paths[t]) paths[t] = new Path2D();
          paths[t].moveTo(x0, y0);
          paths[t].lineTo(x1, y1);
        }
      }
      ctx.lineWidth = lw;
      for (let t = 0; t < BUCKETS; t++) {
        if (!paths[t]) continue;
        ctx.strokeStyle = cmapCss(line.name, (t + 0.5) / BUCKETS);
        ctx.stroke(paths[t]);
      }
    } else {
      const color = line.type === "solid"
        ? cssColor(line.rgb)
        : cmapCss(line.name, rows.length <= 1 ? 0 : i / (rows.length - 1));
      ctx.strokeStyle = color;
      ctx.lineWidth = lw;
      for (const [a, b] of runs) {
        if (b - a < 2) continue;
        ctx.beginPath();
        const [x0, y0] = toCanvas(a, row.y[a]);
        ctx.moveTo(x0, y0);
        for (let k = a + 1; k < b; k++) {
          const [x, y] = toCanvas(k, row.y[k]);
          ctx.lineTo(x, y);
        }
        ctx.stroke();
      }
    }
  }
  ctx.restore();

  // Label.
  const label = scene.style.label;
  if (label) {
    const fs = (label.size_pt / 72) * 100; // pt at 100 dpi -> figure px
    ctx.font = `${fs}px "${label.font_family}", serif`;
    ctx.fillStyle = cssColor(label.color);
    ctx.textBaseline = "alphabetic";
    const lines = label.text.split("\n");
    const lh = fs * 1.2;
    const bx = axes[0] + label.x * (axes[2] - axes[0]);
    const by = axes[1] + label.y * (axes[3] - axes[1]) - (lines.length - 1) * lh - fs * 0.15;
    if (label.background !== false) {
      const widest = Math.max(...lines.map((l) => ctx.measureText(l).width));
      const pad = fs * 0.25;
      const boxTop = by - fs * 0.85 - pad;
      const boxH = lines.length * lh + 1.4 * pad;
      ctx.fillStyle = cssColor(scene.style.background);
      ctx.fillRect(bx - pad, boxTop, widest + 2 * pad, boxH);
    }
    ctx.fillStyle = cssColor(label.color);
    for (let k = 0; k < lines.length; k++) {
      ctx.fillText(lines[k], bx, by + k * lh);
    }
  }

  // Annotation dot + text.
  const ann = scene.style.annotation;
  if (ann) {
    const ax = axes[0] + ann.x * (axes[2] - axes[0]);
    const ay = axes[1] + (1 - ann.y) * (axes[3] - axes[1]);
    ctx.fillStyle = cssColor(ann.color);
    ctx.beginPath();
    ctx.arc(ax, ay, (ann.dot_pt / 72) * 50, 0, Math.PI * 2);
    ctx.fill();
    if (ann.label) {
      const fs = (ann.label_size_pt / 72) * 100;
      ctx.font = `${fs}px "Cinzel", serif`;
      const lx = axes[0] + (ann.x + ann.x_offset) * (axes[2] - axes[0]);
      const ly = axes[1] + (1 - ann.y - ann.y_offset) * (axes[3] - axes[1]);
      ctx.fillText(ann.label, lx, ly - fs * 0.15);
    }
  }

  function toCanvas(x, y) {
    // Data -> figure px (same mapping as backend FigureLayout::to_px).
    const [x0, x1] = scene.layout.xlim;
    const [y0, y1] = scene.layout.ylim;
    const fx = (x - x0) / (x1 - x0);
    const fy = (y - y0) / (y1 - y0);
    return [
      axes[0] + fx * (axes[2] - axes[0]),
      axes[1] + (1 - fy) * (axes[3] - axes[1]),
    ];
  }
}

function lineWidthPx() {
  // pt -> figure px at 100 dpi.
  return (scene.style.linewidth_pt / 72) * 100;
}

function computeRuns(y) {
  const runs = [];
  let start = -1;
  for (let i = 0; i < y.length; i++) {
    const finite = y[i] !== null && Number.isFinite(y[i]);
    if (finite && start < 0) start = i;
    if (!finite && start >= 0) { runs.push([start, i]); start = -1; }
  }
  if (start >= 0) runs.push([start, y.length]);
  return runs;
}

// Client-side colormap evaluation, mirroring the backend's colormap module:
// matplotlib formulas + small stop tables for the d3/colorous family.
const MPL_FORMULAS = {
  spring: (t) => [255, 255 * t, 255 * (1 - t)],
  summer: (t) => [255 * t, 255 * (1 - 0.5 * t), 102 * t],
  autumn: (t) => [255, 255 * t, 0],
  winter: (t) => [0, 255 * t, 255 * (1 - 0.5 * t)],
  cool: (t) => [255 * t, 255 * (1 - t), 255],
  ocean: (t) => [255 * cl01(3 * t - 2), 255 * Math.abs((3 * t - 1) / 2), 255 * t],
  gnuplot: (t) => [255 * Math.sqrt(t), 255 * t ** 3, 255 * Math.sin(2 * Math.PI * t)],
};

const CMAP_TABLES = {
  viridis: [[68,1,84],[72,40,120],[62,74,137],[49,104,142],[38,130,142],[31,158,137],[53,183,121],[109,205,89],[180,222,44],[253,231,37]],
  magma: [[0,0,4],[28,16,68],[79,18,123],[129,37,129],[181,54,122],[229,80,100],[251,135,97],[254,194,135],[252,253,191]],
  inferno: [[0,0,4],[31,12,72],[85,15,109],[136,34,106],[166,55,74],[188,80,47],[221,124,27],[245,173,35],[252,255,164]],
  plasma: [[13,8,135],[84,2,163],[139,10,165],[185,50,137],[219,92,104],[244,136,73],[254,188,43],[240,249,33]],
  cividis: [[0,34,78],[31,52,98],[50,70,112],[70,89,122],[92,108,129],[116,128,133],[143,149,137],[171,170,138],[201,192,138],[233,215,136],[255,234,70]],
  bone: [[0,0,0],[80,99,131],[159,197,217],[255,255,255]],
};

function cl01(v) { return Math.min(1, Math.max(0, v)); }

function evalCmap(name, t) {
  t = cl01(t);
  const formula = MPL_FORMULAS[name];
  if (formula) return formula(t).map((v) => Math.round(cl01(v / 255) * 255));
  const table = CMAP_TABLES[name];
  if (!table) return [0, 0, 0];
  const x = t * (table.length - 1);
  const i = Math.min(table.length - 2, Math.floor(x));
  const f = x - i;
  return [0, 1, 2].map((c) => Math.round(table[i][c] + f * (table[i + 1][c] - table[i][c])));
}

function cmapCss(name, t) {
  const [r, g, b] = evalCmap(name, t);
  return `rgb(${r},${g},${b})`;
}

function cssColor(c) {
  if (typeof c === "string") return c;
  if (Array.isArray(c)) return `rgb(${c[0]},${c[1]},${c[2]})`;
  if (c && c.type === "solid") return `rgb(${c.rgb[0]},${c.rgb[1]},${c.rgb[2]})`;
  return "#000";
}

// ------------------------------------------------------------ navigation ---

function canvasPos(e) {
  const rect = canvas.getBoundingClientRect();
  return [e.clientX - rect.left, e.clientY - rect.top].map((v) => v * (window.devicePixelRatio || 1));
}

let dragging = null;
canvas.addEventListener("pointerdown", (e) => {
  dragging = { pos: canvasPos(e), view: { ...view } };
  canvas.classList.add("dragging");
  canvas.setPointerCapture(e.pointerId);
});
canvas.addEventListener("pointermove", (e) => {
  if (!dragging) return;
  const [x, y] = canvasPos(e);
  view.tx = dragging.view.tx + (x - dragging.pos[0]);
  view.ty = dragging.view.ty + (y - dragging.pos[1]);
  draw();
});
canvas.addEventListener("pointerup", () => { dragging = null; canvas.classList.remove("dragging"); });
canvas.addEventListener("wheel", (e) => {
  e.preventDefault();
  if (!view) return;
  const [mx, my] = canvasPos(e);
  const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
  const newScale = Math.min(40, Math.max(0.05, view.scale * factor));
  view.tx = mx - ((mx - view.tx) * newScale) / view.scale;
  view.ty = my - ((my - view.ty) * newScale) / view.scale;
  view.scale = newScale;
  draw();
}, { passive: false });
canvas.addEventListener("dblclick", () => { fitView(); draw(); });

// ---------------------------------------------------------------- export ---

$("export-svg").addEventListener("click", async () => {
  const resp = await fetch("/api/export.svg", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ...params, fit: "plane" }),
  });
  if (!resp.ok) { alert("export failed"); return; }
  const blob = await resp.blob();
  triggerDownload(URL.createObjectURL(blob), "ridge-map.svg");
});

$("export-png").addEventListener("click", async () => {
  const resp = await fetch("/api/export.svg", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ...params, fit: "plane" }),
  });
  if (!resp.ok) { alert("export failed"); return; }
  const svgText = await resp.text();
  const blob = new Blob([svgText], { type: "image/svg+xml" });
  const url = URL.createObjectURL(blob);
  const img = new Image();
  img.onload = () => {
    const scale = 2;
    const c = document.createElement("canvas");
    c.width = scene.layout.width_px * scale;
    c.height = scene.layout.height_px * scale;
    const cx = c.getContext("2d");
    cx.drawImage(img, 0, 0, c.width, c.height);
    c.toBlob((pngBlob) => triggerDownload(URL.createObjectURL(pngBlob), "ridge-map.png"));
    URL.revokeObjectURL(url);
  };
  img.src = url;
});

function triggerDownload(url, name) {
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

// ---------------------------------------------------------------- readme ---

let readmeLoaded = false;
$("readme-open").addEventListener("click", async () => {
  $("readme").showModal();
  if (readmeLoaded) return;
  try {
    const resp = await fetch("/api/readme");
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    $("readme-text").textContent = await resp.text();
    readmeLoaded = true;
  } catch (err) {
    $("readme-text").textContent = `could not load the README (${err.message})`;
  }
});

// ------------------------------------------------------------ map picker --

const SRTM_LAT_MAX = 60; // SRTM covers 60S..60N; the server rejects beyond it

let map = null;
let mapRect = null;
let mapDrawStart = null;
let mapTool = "move"; // "move" pans the map; "select" drags out a new bbox

function clampLat(v) { return Math.min(SRTM_LAT_MAX, Math.max(-SRTM_LAT_MAX, v)); }
function clampLng(v) { return Math.min(180, Math.max(-180, v)); }
function clampLL(ll) {
  return { lat: clampLat(ll.lat), lng: clampLng(ll.lng) };
}

/* Switch between panning the map and drawing a selection. */
function setMapTool(tool) {
  mapTool = tool;
  for (const [id, t] of [["map-tool-move", "move"], ["map-tool-select", "select"]]) {
    const btn = $(id);
    btn.classList.toggle("active", t === tool);
    btn.setAttribute("aria-pressed", String(t === tool));
  }
  $("map").classList.toggle("select-mode", tool === "select");
  if (map && !mapDrawStart) {
    if (tool === "select") map.dragging.disable();
    else map.dragging.enable();
  }
}

/* Keep the map rectangle in sync with params.bbox (no view jumps). */
function updateMapRect(fit = false) {
  if (!map || !mapRect) return;
  const [lon0, lat0, lon1, lat1] = params.bbox;
  mapRect.setBounds([
    [clampLat(lat0), clampLng(lon0)],
    [clampLat(lat1), clampLng(lon1)],
  ]);
  if (fit) {
    map.fitBounds([
      [clampLat(lat0), clampLng(lon0)],
      [clampLat(lat1), clampLng(lon1)],
    ], { padding: [12, 12] });
  }
}

function initMap() {
  if (typeof window.L === "undefined") return; // leaflet unavailable: inputs still work
  const container = $("map");
  // A single copy of the world: no wrapping, and panning stops at its edges,
  // so every point on the map is one real longitude for the selection.
  const world = [[-90, -180], [90, 180]];
  map = window.L.map(container, {
    boxZoom: false,
    maxBounds: world,
    maxBoundsViscosity: 1,
  }).setView([44.1, -71.4], 5);
  // Never zoom out past the point where the world is narrower than the panel.
  const fitWorldWidth = () => {
    map.setMinZoom(Math.max(0, Math.ceil(Math.log2(map.getSize().x / 256))));
  };
  fitWorldWidth();
  map.on("resize", fitWorldWidth);
  window.L
    .tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
      noWrap: true,
      bounds: world,
      maxZoom: 13,
      attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    })
    .addTo(map);

  // Excluded latitudes: SRTM covers only 60S..60N.
  const exclStyle = { className: "excluded-zone", interactive: false, stroke: false };
  window.L.polygon(
    [[SRTM_LAT_MAX, -180], [SRTM_LAT_MAX, 180], [90, 180], [90, -180]],
    exclStyle,
  ).addTo(map).bindTooltip("no SRTM data above 60°N", { permanent: false });
  window.L.polygon(
    [[-SRTM_LAT_MAX, -180], [-SRTM_LAT_MAX, 180], [-90, 180], [-90, -180]],
    exclStyle,
  ).addTo(map).bindTooltip("no SRTM data below 60°S", { permanent: false });

  mapRect = window.L.rectangle(
    [[0, 0], [1, 1]],
    { className: "bbox-rect", interactive: false },
  ).addTo(map);
  updateMapRect(true);

  $("map-tool-move").addEventListener("click", () => setMapTool("move"));
  $("map-tool-select").addEventListener("click", () => setMapTool("select"));

  // Drag-to-draw selection in select mode (or Shift-drag in move mode),
  // latitude clamped to the SRTM range. Leaflet's own Shift box-zoom is off.
  map.on("mousedown", (e) => {
    if (mapTool !== "select" && !e.originalEvent?.shiftKey) return;
    mapDrawStart = clampLL(e.latlng);
    map.dragging.disable();
    container.classList?.add?.("leaflet-drawing");
    mapRect.setBounds([mapDrawStart, mapDrawStart]);
  });
  map.on("mousemove", (e) => {
    if (!mapDrawStart) return;
    mapRect.setBounds([mapDrawStart, clampLL(e.latlng)]);
  });
  map.on("mouseup", (e) => {
    if (!mapDrawStart) return;
    if (mapTool === "move") map.dragging.enable();
    container.classList?.remove?.("leaflet-drawing");
    const a = mapDrawStart;
    const b = clampLL(e.latlng);
    mapDrawStart = null;
    const w = Math.min(a.lng, b.lng), e2 = Math.max(a.lng, b.lng);
    const s = Math.min(a.lat, b.lat), n = Math.max(a.lat, b.lat);
    if (e2 - w < 0.02 || n - s < 0.02) return; // accidental click
    params.bbox = [
      Number(w.toFixed(6)), Number(s.toFixed(6)),
      Number(e2.toFixed(6)), Number(n.toFixed(6)),
    ];
    // The disc always follows the bbox: span = its diagonal. A stale span
    // from a previous region would sample a huge disc around a tiny box.
    params.span_deg = Number(Math.hypot(e2 - w, n - s).toFixed(6));
    syncBboxInputs();
    buildControls();
    updateMapRect();
    scheduleRefetch();
  });

  // Collapsible bottom panel.
  const panel = $("map-panel");
  const toggle = $("map-toggle");
  function setPanel(open) {
    panel.classList.toggle("closed", !open);
    toggle.textContent = open ? "hide" : "show";
    if (open) {
      // Leaflet needs a re-measure after the container reappears.
      setTimeout(() => { map.invalidateSize(); updateMapRect(); }, 60);
    }
  }
  $("map-panel-head").addEventListener("click", () => {
    setPanel(panel.classList.contains("closed"));
  });
  toggle.addEventListener("click", (e) => {
    e.stopPropagation();
    setPanel(panel.classList.contains("closed"));
  });
}

/* Push params.bbox into the number inputs (map and inputs stay in sync). */
let bboxInputs = [];
function syncBboxInputs() {
  bboxInputs.forEach((input, i) => { input.value = params.bbox[i]; });
}

// ------------------------------------------------------------ presets -----

async function loadPresets() {
  try {
    const resp = await fetch("/api/presets");
    const presets = await resp.json();
    const sel = $("preset");
    sel.append(el("option", { value: "" }, ["presets…"]));
    presets.forEach((p, i) => sel.append(el("option", { value: i }, [p.name])));
    sel.addEventListener("change", () => {
      if (sel.value === "") return;
      const p = presets[Number(sel.value)];
      params = { ...DEFAULTS, ...p.params, bbox: p.bbox, fit: "plane" };
      if (!params.span_deg) {
        const [lon0, lat0, lon1, lat1] = params.bbox;
        params.span_deg = Number(Math.hypot(lon1 - lon0, lat1 - lat0).toFixed(4));
      }
      buildControls();
      updateMapRect(true); // center the map on the preset
      updateHash();
      refetchElevation();
      sel.value = "";
    });
  } catch { /* presets are optional sugar */ }
}

// -------------------------------------------------------------- permalink --

function updateHash() {
  history.replaceState(null, "", "#" + encodeURIComponent(JSON.stringify(params)));
}

function loadFromHash() {
  if (location.hash.length > 1) {
    try {
      const parsed = JSON.parse(decodeURIComponent(location.hash.slice(1)));
      params = { ...DEFAULTS, ...parsed };
    } catch { /* bad hash: keep defaults */ }
  }
}

// ----------------------------------------------------------------- boot ----

window.addEventListener("resize", () => { resizeCanvas(); map?.invalidateSize?.(); });
loadFromHash();
buildControls();
initMap();
loadPresets();
resizeCanvas();
refetchElevation();
