/* The browser half of the terrain pipeline: rotate the sampled grid, mask
 * water and lakes, and turn it into ridge rows and a figure layout. Pure
 * functions over typed arrays, each one mirroring its ridge-core
 * counterpart; the parity test holds them to the Rust output. */

export const LINE_SPACING = 6;
export const SUBPLOT = { left: 0.125, right: 0.9, bottom: 0.11, top: 0.88 };
export const AXES_MARGIN = 0.05;
const MIN_LAKE_COMPONENT = 12;

export type Bbox = [number, number, number, number]; // lon0, lat0, lon1, lat1

export interface Row {
  baseline: number;
  y: number[];
}

export interface Bounds {
  xmin: number;
  xmax: number;
  ymin: number;
  ymax: number;
  empty: boolean;
}

export interface Layout {
  width_px: number;
  height_px: number;
  axes: [number, number, number, number];
  xlim: [number, number];
  ylim: [number, number];
}

export interface Prepared {
  grid: Float64Array;
  vmin: number;
  vmax: number;
}

/* Rotate the grid about its center within the same canvas (nearest
 * neighbor). Samples outside the source become NaN gaps: the plane never
 * grows, so zoom and distance stay fixed while the terrain spins. Mirrors
 * ridge_core::rotate::rotate_fixed_plane. */
export function rotatePlane(src: Float64Array, nrows: number, ncols: number, angleDeg: number): Float64Array {
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
      // scipy/ridge-core constant mode: the coordinate must lie within
      // [0, n-1]; anything outside is a gap, even if it would round in-range.
      if (pr < 0 || pc < 0 || pr > nrows - 1 || pc > ncols - 1) continue;
      const sr = Math.floor(pr + 0.5), sc = Math.floor(pc + 0.5);
      out[r * ncols + col] = src[sr * ncols + sc];
    }
  }
  return out;
}

/* Port of ridge_core::preprocess::preprocess: NaN->min, normalize,
 * percentile water mask, lake mask (float gradient, water-excluded,
 * component-coherent), flip rows, vertical exaggeration.
 *
 * ALL threshold decisions happen here, on the UNROTATED disc, where every
 * cell is a fixed physical location, so water bodies and hills hold still
 * as the view rotates. Returns the display-ready grid (masked, flipped,
 * scaled) plus vmin/vmax, or null when there is no data at all. */
export function preprocessGrid(
  src: Float64Array, nrows: number, ncols: number,
  waterNtile: number, lakeFlatness: number, vratio: number,
  flatnessScale = 1, statsBbox: Bbox | null = null,
  dLat0 = 0, dLon0 = 0, dSpan = 0,
): Prepared | null {
  const n = nrows * ncols;
  let min = Infinity, max = -Infinity;
  const nanMask = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    const v = src[i];
    if (Number.isNaN(v)) { nanMask[i] = 1; continue; }
    if (v < min) min = v;
    if (v > max) max = v;
  }
  if (!Number.isFinite(min)) return null;
  const span = max - min;
  const norm = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    // Upstream fills NaN cells with the min before normalizing.
    const v = nanMask[i] ? min : src[i];
    norm[i] = span > 0 ? (v - min) / span : 0;
  }

  // Water level: numpy-style linear percentile over REAL terrain cells only
  // (NaN padding in the disc corners would drag it to zero and hide every
  // river) and, when a reference region is given (the rect footprint), over
  // that region only, matching upstream's window-scoped water level.
  const inStats = (i: number) => {
    if (nanMask[i]) return false;
    if (!statsBbox) return true;
    const r = (i / ncols) | 0, c = i % ncols;
    const lat = dLat0 + ((r + 0.5) / nrows) * dSpan;
    const lon = dLon0 + ((c + 0.5) / ncols) * dSpan;
    return lat >= statsBbox[1] && lat <= statsBbox[3]
        && lon >= statsBbox[0] && lon <= statsBbox[2];
  };
  const finite: number[] = [];
  for (let i = 0; i < n; i++) if (inStats(i)) finite.push(norm[i]);
  finite.sort((a, b) => a - b);
  const pick = (q: number) => {
    if (finite.length === 1) return finite[0];
    const idx = (q / 100) * (finite.length - 1);
    const lo = Math.floor(idx), hi = Math.ceil(idx);
    return lo === hi ? finite[lo] : finite[lo] + (finite[hi] - finite[lo]) * (idx - lo);
  };
  const waterLevel = pick(Math.min(100, Math.max(0, waterNtile)));

  // Lake mask, mirroring ridge_core::preprocess::preprocess:
  //  1. gradient on the NORMALIZED FLOATS (threshold lakeFlatness/255), the
  //     same semantics as upstream's u8 rank gradient without its rounding;
  //  2. water/NaN cells EXCLUDED from the neighborhoods (skimage's `mask`),
  //     so flat shore merges into the water body;
  //  3. candidates kept only in connected components >= MIN_LAKE_COMPONENT.
  const excluded = new Uint8Array(n);
  for (let i = 0; i < n; i++) excluded[i] = nanMask[i] || norm[i] < waterLevel ? 1 : 0;
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
  // Connected components (4-connected).
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

  // Apply masks, flip north/south, exaggerate vertically.
  const grid = new Float64Array(n);
  for (let r = 0; r < nrows; r++) {
    const sr = nrows - 1 - r; // upstream values[-1::-1]
    for (let c = 0; c < ncols; c++) {
      const i = sr * ncols + c;
      grid[r * ncols + c] =
        nanMask[i] || norm[i] < waterLevel || isLake[i] ? NaN : norm[i] * vratio;
    }
  }
  return { grid, vmin: 0, vmax: 1 };
}

/* Anisotropic display window over the rotated underlying grid: covers the
 * original bbox at numLines x elevationPts samples, reading nearest nodes.
 * The window never changes shape with the angle; the disc supplies
 * previously unused points as the frame sweeps around. Mirrors
 * ridge_core::grid::sample_window. */
export function sampleWindow(
  grid: Float64Array, dn: number, dLat0: number, dLon0: number, dSpan: number,
  bbox: Bbox, numLines: number, elevationPts: number,
): Float64Array {
  const out = new Float64Array(numLines * elevationPts).fill(NaN);
  const step = dSpan / dn;
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

/* Ridge rows from a display-ready grid: row i is drawn at
 * y = value - LINE_SPACING * i. */
export function buildRows(grid: Float64Array, nrows: number, ncols: number): Row[] {
  const rows: Row[] = [];
  for (let i = 0; i < nrows; i++) {
    const baseline = -LINE_SPACING * i;
    const y = new Array<number>(ncols);
    for (let c = 0; c < ncols; c++) y[c] = grid[i * ncols + c] + baseline;
    rows.push({ baseline, y });
  }
  return rows;
}

/* Frame bounds of a row set. With `clipToLand` the axes hug the cells that
 * hold land, which is what the legacy plot gets from matplotlib autoscaling
 * around the points it draws: all-water columns and rows then drop out of the
 * frame, so `water_ntile` rescales the picture. The default frames the whole
 * requested window instead, so water, lakes and voids keep their place and
 * masking can never move the frame. Mirrors ridge_core::RidgeScene::from_grid.
 * `empty` = nothing at all to draw. */
export function frameBounds(rows: Row[], clipToLand = false): Bounds {
  let ymax = -Infinity;
  let xmin = 0;
  let xmax = rows.length > 0 ? rows[0].y.length - 1 : -1;
  // Baselines step down by LINE_SPACING, so the last row is the lowest.
  let ymin = rows.length > 0 ? rows[rows.length - 1].baseline : 0;

  if (clipToLand) {
    xmin = Infinity;
    xmax = -Infinity;
    ymin = Infinity;
    for (const row of rows) {
      let hasData = false;
      for (let c = 0; c < row.y.length; c++) {
        const y = row.y[c];
        if (!Number.isFinite(y)) continue;
        hasData = true;
        if (y > ymax) ymax = y;
        if (c < xmin) xmin = c;
        if (c > xmax) xmax = c;
      }
      if (hasData && row.baseline < ymin) ymin = row.baseline;
    }
    if (xmin === Infinity) {
      // Nothing drawable even by that reckoning: the theoretical frame.
      xmin = 0;
      xmax = rows.length > 0 ? rows[0].y.length - 1 : -1;
      ymin = rows.length > 0 ? rows[rows.length - 1].baseline : 0;
    }
  } else {
    for (const row of rows) {
      // NaN comparisons are false, so gaps drop out of the max.
      for (const y of row.y) if (y > ymax) ymax = y;
    }
  }

  return { xmin, xmax, ymin, ymax, empty: ymax === -Infinity };
}

/* Figure layout (matplotlib parity): figure size, subplot rect, and the data
 * limits the frame bounds describe, with 5% margins. */
export function buildLayout(bounds: Bounds, sizeScale: number, bboxRatio: number): Layout {
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

/* [start, end) index ranges of consecutive finite values. */
export function computeRuns(y: number[]): [number, number][] {
  const runs: [number, number][] = [];
  let start = -1;
  for (let i = 0; i < y.length; i++) {
    const finite = y[i] !== null && Number.isFinite(y[i]);
    if (finite && start < 0) start = i;
    if (!finite && start >= 0) { runs.push([start, i]); start = -1; }
  }
  if (start >= 0) runs.push([start, y.length]);
  return runs;
}
