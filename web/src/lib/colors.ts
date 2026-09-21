/* Colors: named values, hex, and colormaps, mirroring the backend's
 * colormap module (matplotlib formulas plus stop tables for the d3 family). */

export type Rgb = [number, number, number];
export type LineColor = { type: "solid"; rgb: Rgb } | { type: "map"; name: string };

const NAMED_COLORS: Record<string, Rgb> = {
  black: [0, 0, 0], white: [255, 255, 255], orange: [255, 165, 0],
  red: [255, 0, 0], green: [0, 128, 0], blue: [0, 0, 255],
  purple: [128, 0, 128], brown: [165, 42, 42], pink: [255, 192, 203],
  gray: [128, 128, 128], grey: [128, 128, 128], navy: [0, 0, 128],
  teal: [0, 128, 128], crimson: [220, 20, 60],
};

const cl01 = (v: number) => Math.min(1, Math.max(0, v));

const MPL_FORMULAS: Record<string, (t: number) => Rgb> = {
  spring: (t) => [255, 255 * t, 255 * (1 - t)],
  summer: (t) => [255 * t, 255 * (1 - 0.5 * t), 102 * t],
  autumn: (t) => [255, 255 * t, 0],
  winter: (t) => [0, 255 * t, 255 * (1 - 0.5 * t)],
  cool: (t) => [255 * t, 255 * (1 - t), 255],
  ocean: (t) => [255 * cl01(3 * t - 2), 255 * Math.abs((3 * t - 1) / 2), 255 * t],
  gnuplot: (t) => [255 * Math.sqrt(t), 255 * t ** 3, 255 * Math.sin(2 * Math.PI * t)],
};

const CMAP_TABLES: Record<string, Rgb[]> = {
  viridis: [[68,1,84],[72,40,120],[62,74,137],[49,104,142],[38,130,142],[31,158,137],[53,183,121],[109,205,89],[180,222,44],[253,231,37]],
  magma: [[0,0,4],[28,16,68],[79,18,123],[129,37,129],[181,54,122],[229,80,100],[251,135,97],[254,194,135],[252,253,191]],
  inferno: [[0,0,4],[31,12,72],[85,15,109],[136,34,106],[166,55,74],[188,80,47],[221,124,27],[245,173,35],[252,255,164]],
  plasma: [[13,8,135],[84,2,163],[139,10,165],[185,50,137],[219,92,104],[244,136,73],[254,188,43],[240,249,33]],
  cividis: [[0,34,78],[31,52,98],[50,70,112],[70,89,122],[92,108,129],[116,128,133],[143,149,137],[171,170,138],[201,192,138],[233,215,136],[255,234,70]],
  bone: [[0,0,0],[80,99,131],[159,197,217],[255,255,255]],
};

/* Suggestions for the line-color field: plain colors, then colormaps. */
export const COLOR_SUGGESTIONS = [
  "black", "white", "orange", "red", "navy", "teal", "crimson", "#414a4c",
  "viridis", "magma", "inferno", "plasma", "cividis",
  "spring", "summer", "autumn", "winter", "cool", "bone", "ocean", "gnuplot",
];

export function parseColor(name: unknown): Rgb {
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

const isColormap = (name: string) => Object.hasOwn(MPL_FORMULAS, name) || Object.hasOwn(CMAP_TABLES, name);

export function resolveLineColor(name: string): LineColor {
  const lower = name.toLowerCase();
  return isColormap(lower) ? { type: "map", name: lower } : { type: "solid", rgb: parseColor(name) };
}

export function evalCmap(name: string, t: number): Rgb {
  t = cl01(t);
  const formula = MPL_FORMULAS[name];
  if (formula) return formula(t).map((v) => Math.round(cl01(v / 255) * 255)) as Rgb;
  const table = CMAP_TABLES[name];
  if (!table) return [0, 0, 0];
  const x = t * (table.length - 1);
  const i = Math.min(table.length - 2, Math.floor(x));
  const f = x - i;
  return [0, 1, 2].map((c) => Math.round(table[i][c] + f * (table[i + 1][c] - table[i][c]))) as Rgb;
}

export const cssColor = (c: Rgb) => `rgb(${c[0]},${c[1]},${c[2]})`;
export const cmapCss = (name: string, t: number) => cssColor(evalCmap(name, t));

/* A value for <input type="color">, which only takes #rrggbb. */
export function toHex(c: string): string {
  if (/^#[0-9a-f]{6}$/i.test(c)) return c;
  const [r, g, b] = parseColor(c);
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}
