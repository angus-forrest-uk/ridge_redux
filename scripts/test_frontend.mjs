// Frontend logic test: load web/main.js with stubbed DOM + canvas, feed it a
// real /api/elevation response, and verify the local pipeline renders with
// NO further server round-trips (rotation/water/relief are client-side).
import { readFileSync } from "fs";

// ---- minimal 2D context stub ------------------------------------------
function makeCtx() {
  const calls = { fillRect: 0, beginPath: 0, moveTo: 0, lineTo: 0, stroke: 0, fill: 0, fillText: 0, arc: 0 };
  const ctx = {
    setTransform: () => {},
    save: () => {},
    restore: () => {},
    rect: () => {},
    clip: () => {},
    closePath: () => {},
    measureText: (s) => ({ width: s.length * 10 }),
    canvas: null,
    ...Object.fromEntries(Object.keys(calls).map((k) => [k, (..._a) => { calls[k]++; }])),
    __calls: calls,
  };
  for (const prop of ["fillStyle", "strokeStyle", "lineWidth", "lineJoin", "lineCap", "font", "textBaseline"]) {
    let v;
    Object.defineProperty(ctx, prop, { get: () => v, set: (x) => { v = x; } });
  }
  return ctx;
}

// ---- fake raw elevation payload (same shape the backend returns) ------
// Underlying square grid + the display window cropped from it.
const NROWS = 10, NCOLS = 10;
const RAW = [];
for (let r = 0; r < NROWS; r++) {
  const row = [];
  for (let c = 0; c < NCOLS; c++) {
    if (r === 1 && c >= 6) row.push(null);           // voids (ocean)
    else if (r === 5 && c >= 3 && c <= 5) row.push(500); // flat-ish patch
    else row.push(100 + 20 * r + 3 * c);
  }
  RAW.push(row);
}
const elevationPayload = {
  shape: [NROWS, NCOLS],
  values: RAW,
  window: { row0: 3, col0: 2, rows: 4, cols: 6 },
};

// ---- DOM stubs ----------------------------------------------------------
const ctx = makeCtx();
const canvas = {
  getContext: () => ctx,
  width: 3000,
  height: 2000,
  getBoundingClientRect: () => ({ left: 0, top: 0, width: 1500, height: 1000 }),
  addEventListener: (type, fn) => { listeners[type] = fn; },
  setPointerCapture: () => {},
  classList: { add: () => {}, remove: () => {}, toggle: () => {} },
};
const listeners = {};

function makeEl(tag) {
  const node = {
    tag,
    children: [],
    attributes: {},
    value: "",
    checked: false,
    textContent: "",
    appendChild: (c) => node.children.push(c),
    append: (...c) => node.children.push(...c),
    replaceChildren: () => { node.children = []; },
    addEventListener: (type, fn) => { node[`on${type}`] = fn; },
    setAttribute: (k, v) => { node.attributes[k] = v; },
    classList: { add: () => {}, remove: () => {}, toggle: () => {} },
    style: {},
  };
  return node;
}

const controlsEl = makeEl("div");
const statusEl = makeEl("div");

global.document = {
  getElementById: (id) => {
    if (id === "canvas") return canvas;
    if (id === "controls") return controlsEl;
    if (id === "status") return statusEl;
    if (id === "preset") { const e = makeEl("select"); e.addEventListener = () => {}; return e; }
    return makeEl("div");
  },
  createElement: (tag) => makeEl(tag),
};
// Minimal Leaflet stub so the map-picker code path runs in the harness.
const mapStubHandlers = {};
const L = {
  __initCount: 0,
  map: () => {
    L.__initCount++;
    const m = {
      on: (type, fn) => { mapStubHandlers[type] = fn; return m; },
      setView: () => m,
      fitBounds: () => m,
      dragging: { enable: () => {}, disable: () => {} },
      invalidateSize: () => {},
    };
    return m;
  },
  tileLayer: () => { const t = { addTo: () => t }; return t; },
  rectangle: () => { const r = { setBounds: () => r, addTo: () => r, on: () => r }; return r; },
  polygon: () => { const p = { addTo: () => p, bindTooltip: () => p }; return p; },
  latLngBounds: (a, b) => ({ a, b }),
  latLng: (lat, lng) => ({ lat, lng }),
};
global.L = L;
global.window = { devicePixelRatio: 2, addEventListener: () => {}, L };
global.history = { replaceState: () => {} };
global.location = { hash: "" };
global.performance = { now: () => 0 };
global.requestAnimationFrame = (fn) => setTimeout(fn, 0);
global.alert = () => {};

let fetchCount = 0;
const fetchLog = [];
global.fetch = async (url, opts) => {
  fetchCount++;
  fetchLog.push({ url, body: opts?.body });
  if (url === "/api/elevation") {
    const params = JSON.parse(opts.body);
    if (!Array.isArray(params.bbox) || params.viewpoint_angle !== undefined) {
      throw new Error("elevation request must be angle-free {bbox, num_lines, elevation_pts}");
    }
    return {
      ok: true,
      json: async () => elevationPayload,
    };
  }
  if (url === "/api/presets") return { ok: true, json: async () => [] };
  throw new Error("unexpected fetch " + url);
};

// ---- load the real frontend code ---------------------------------------
const source = readFileSync(new URL("../web/main.js", import.meta.url), "utf8");
new Function(source)();

// Wait past the debounced refetch + rAF-throttled recompute.
await new Promise((r) => setTimeout(r, 600));

const c = ctx.__calls;
const assert = (cond, msg) => { if (!cond) { console.error("FAIL:", msg); process.exit(1); } console.log("ok:", msg); };

assert(c.fillRect >= 2, "background + label boxes drawn");
assert(c.stroke >= 3, "at least one stroke per row with data");
assert(c.fillText >= 2, "label text drawn");
assert(controlsEl.children.length > 0, "control panel built");
assert(statusEl.textContent.includes("80 × 300"), `status shows window dims (got: ${statusEl.textContent})`);
assert(typeof ctx.strokeStyle === "string" && ctx.strokeStyle.startsWith("rgb("), "colormap colors resolved to css");

// The core promise: exactly ONE elevation fetch + presets, nothing else —
// the rotation/recompute pipeline must run fully client-side.
assert(fetchCount === 2, `fetch count is 2 (elevation + presets), got ${fetchCount}`);
const elevReq = fetchLog.find((f) => f.url === "/api/elevation");
assert(!!elevReq, "an elevation request was made");
assert(
  !JSON.stringify(elevReq.body ?? "").includes("viewpoint_angle"),
  "elevation request carries no angle (rotation is client-side)",
);

// Navigation handlers still run without throwing.
listeners.wheel({ deltaY: -100, clientX: 10, clientY: 10, preventDefault: () => {} });
listeners.pointerdown({ clientX: 10, clientY: 10, pointerId: 1 });
listeners.pointermove({ clientX: 50, clientY: 30 });
listeners.pointerup({});
assert(true, "navigation handlers ran without throwing");

// Map picker: initialized once, two exclusion zones + one bbox rectangle.
assert(L.__initCount === 1, "map initialized exactly once");
assert(typeof mapStubHandlers.mousedown === "function"
     && typeof mapStubHandlers.mousemove === "function"
     && typeof mapStubHandlers.mouseup === "function",
     "map draw handlers registered");
// Simulate a draw inside the SRTM band: still no extra fetches.
mapStubHandlers.mousedown({ latlng: { lat: 44.0, lng: -71.5 } });
mapStubHandlers.mousemove({ latlng: { lat: 44.5, lng: -70.9 } });
mapStubHandlers.mouseup({ latlng: { lat: 44.5, lng: -70.9 } });
await new Promise((r) => setTimeout(r, 500));
assert(fetchCount === 3, `map draw triggers exactly one refetch (elevation+presets+draw = 3, got ${fetchCount})`);
// Clamping: a drag reaching above 60N must be pulled back into coverage.
mapStubHandlers.mousedown({ latlng: { lat: 58, lng: -71 } });
mapStubHandlers.mouseup({ latlng: { lat: 70, lng: -70 } });
await new Promise((r) => setTimeout(r, 500));
const lastBody = JSON.parse(fetchLog[fetchLog.length - 1].body);
assert(
  lastBody.bbox[3] <= 60,
  `draw clamped to SRTM coverage (n = ${lastBody.bbox[3]})`,
);

// Regression: a tiny far-away bbox (New Zealand) must derive its span from
// the NEW bbox, not reuse a stale one — a stale 1.2-degree span around a
// 0.04-degree box used to blow the grid up to 4000x4000 (~56 MB responses).
mapStubHandlers.mousedown({ latlng: { lat: -43.63036, lng: 172.633667 } });
mapStubHandlers.mouseup({ latlng: { lat: -43.605256, lng: 172.670403 } });
await new Promise((r) => setTimeout(r, 500));
const nzReq = JSON.parse(fetchLog[fetchLog.length - 1].body);
assert(
  Math.abs(nzReq.bbox[0] - 172.633667) < 1e-6,
  "NZ bbox sent as drawn",
);
assert(
  nzReq.span_deg > 0.03 && nzReq.span_deg < 0.1,
  `span auto-derived from the new bbox (${nzReq.span_deg})`,
);
assert(
  nzReq.num_lines === 80 && nzReq.elevation_pts === 300,
  "window dims stay at the original composition",
);

console.log("\nALL FRONTEND LOGIC TESTS PASSED");
