// Parity check: the browser pipeline (functions lifted from web/main.js)
// must produce the same rows as the Rust pipeline for the same rotated grid.
//
// The fixture (written by `cargo run -p ridge-core --example dump_plane_fixture`)
// contains: the rotated raw grid (33 degrees, plane fit), and the rows the
// Rust pipeline computed. We feed the input through the JS functions pulled
// out of main.js and compare.
import { readFileSync } from "fs";

const fixture = JSON.parse(readFileSync("/tmp/plane_fixture.json", "utf8"));
const mainJs = readFileSync(new URL("../web/main.js", import.meta.url), "utf8");

// ---- stub the DOM so main.js can load, then capture its pipeline fns ----
function makeCtx() {
  const noop = () => {};
  const ctx = {
    setTransform: noop, save: noop, restore: noop, rect: noop, clip: noop,
    closePath: noop, measureText: (s) => ({ width: s.length * 10 }),
    ...Object.fromEntries(
      ["fillRect", "beginPath", "moveTo", "lineTo", "stroke", "fill", "fillText", "arc"].map((k) => [k, noop]),
    ),
  };
  for (const p of ["fillStyle", "strokeStyle", "lineWidth", "lineJoin", "lineCap", "font", "textBaseline"]) {
    let v;
    Object.defineProperty(ctx, p, { get: () => v, set: (x) => { v = x; } });
  }
  return ctx;
}
function makeEl(tag) {
  const node = { tag, children: [], appendChild: (c) => node.children.push(c),
    append: (...c) => node.children.push(...c), replaceChildren: () => {},
    addEventListener: () => {}, setAttribute: () => {}, classList: { add() {}, remove() {}, toggle() {} }, style: {} };
  return node;
}
const canvasStub = { getContext: () => makeCtx(), width: 100, height: 100,
  getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
  addEventListener: () => {}, setPointerCapture: () => {}, classList: { add() {}, remove() {}, toggle() {} } };

global.document = {
  getElementById: (id) => (id === "canvas" ? canvasStub : makeEl("div")),
  createElement: (t) => makeEl(t),
};
global.window = { devicePixelRatio: 1, addEventListener: () => {} };
global.history = { replaceState: () => {} };
global.location = { hash: "" };
global.performance = { now: () => 0 };
global.requestAnimationFrame = (fn) => setTimeout(fn, 0);
global.fetch = async () => { throw new Error("no network in parity test"); };

// Load main.js with an instrumentation epilogue that exports the internals.
const epilogue = `;
global.__ridge = { rotatePlane, preprocessGrid, buildRows, buildLayout, contentBounds, LINE_SPACING, AXES_MARGIN, SUBPLOT };`;
new Function(mainJs + epilogue)();
const { rotatePlane, preprocessGrid, buildRows, buildLayout, contentBounds, LINE_SPACING, AXES_MARGIN, SUBPLOT } = global.__ridge;

// ---- run the JS pipeline on the Rust fixture input ----------------------
const nrows = fixture.input.length;
const ncols = fixture.input[0].length;
let raw = new Float64Array(nrows * ncols);
for (let r = 0; r < nrows; r++) {
  for (let c = 0; c < ncols; c++) {
    const v = fixture.input[r][c];
    raw[r * ncols + c] = v === null ? NaN : v;
  }
}
// New flicker-free order: decisions on the unrotated grid, then rotate the
// masked/scaled/flipped result (the Rust fixture was produced with exactly
// this order and angle -33).
const pp = preprocessGrid(raw, nrows, ncols, 10, 3, 40);
if (!pp) { console.error("FAIL: JS pipeline produced no data"); process.exit(1); }
const rotated = rotatePlane(pp.grid, nrows, ncols, -33);
const rows = buildRows(rotated, nrows, ncols);

// ---- compare against Rust rows ------------------------------------------
const assert = (cond, msg) => { if (!cond) { console.error("FAIL:", msg); process.exit(1); } console.log("ok:", msg); };
assert(rows.length === fixture.expected_rows.length, `row count ${rows.length} == ${fixture.expected_rows.length}`);

let checked = 0;
let worst = 0;
for (let r = 0; r < nrows; r++) {
  const js = rows[r], rs = fixture.expected_rows[r];
  assert(js.baseline === rs.baseline, `row ${r} baseline ${js.baseline} == ${rs.baseline}`);
  for (let c = 0; c < ncols; c++) {
    const a = js.y[c], b = rs.y[c];
    const aNull = a === null || a === undefined || Number.isNaN(a);
    const bNull = b === null || b === undefined || (typeof b === "number" && Number.isNaN(b));
    if (aNull !== bNull) {
      console.error(`FAIL: row ${r} col ${c} gap mismatch js=${a} rust=${b}`);
      process.exit(1);
    }
    if (!aNull) {
      const d = Math.abs(a - b);
      worst = Math.max(worst, d);
      if (d > 1e-9) { console.error(`FAIL: row ${r} col ${c} js=${a} rust=${b}`); process.exit(1); }
      checked++;
    }
  }
}
assert(worst <= 1e-9, `${checked} finite values match within 1e-9 (worst ${worst.toExponential(2)})`);

// Layout parity: JS re-framing must match the Rust scene layout exactly.
const bounds = contentBounds(rows);
const bboxRatio = 24 / 30; // matches the fixture dump
const layout = buildLayout(bounds, 20, bboxRatio);
const rl = fixture.layout;
for (const key of ["width_px", "height_px", "xlim", "ylim"]) {
  const a = JSON.stringify(layout[key]), b = JSON.stringify(rl[key]);
  assert(a === b, `layout.${key} == rust (${a})`);
}
for (let i = 0; i < 4; i++) {
  assert(Math.abs(layout.axes[i] - rl.axes[i]) < 1e-9, `layout.axes[${i}] == rust`);
}
assert(Number.isFinite(layout.ylim[0]) && layout.ylim[1] > layout.ylim[0], "layout ylim sane");

console.log("\nJS PIPELINE MATCHES RUST PIPELINE BIT-FOR-BIT (within 1e-9)");
