/* From a fetched elevation grid to a drawable scene, entirely in the
 * browser. Split in two so each step re-runs only when its inputs change:
 * prepare() makes the water and lake decisions, buildScene() rotates,
 * windows and lays out the result. */
import { evalCmap, parseColor, resolveLineColor, type LineColor, type Rgb } from "./colors.ts";
import type { ElevationRequest, Params } from "./params.ts";
import {
  buildLayout, buildRows, frameBounds, preprocessGrid, rotatePlane, sampleWindow,
  type Bbox, type Layout, type Prepared, type Row,
} from "./pipeline.ts";

/* The grid /api/elevation returned, with the request that produced it. */
export interface Raw {
  nrows: number;
  ncols: number;
  values: Float64Array;
  request: ElevationRequest;
}

export interface LabelStyle {
  text: string;
  color: Rgb;
  x: number;
  y: number;
  size_pt: number;
  font_family: string;
  background: boolean;
}

export interface AnnotationStyle {
  label: string;
  x: number;
  y: number;
  x_offset: number;
  y_offset: number;
  label_size_pt: number;
  dot_pt: number;
  color: Rgb;
}

export interface Style {
  line: LineColor;
  kind: Params["kind"];
  background: Rgb;
  linewidth_pt: number;
  label: LabelStyle | null;
  annotation: AnnotationStyle | null;
}

export interface Scene {
  rows: Row[];
  vmin: number;
  vmax: number;
  layout: Layout;
  style: Style;
}

/* The disc the grid was sampled over: its side and south-west corner.
 * Rect requests add move_margin on every side, so the disc extends past
 * the bbox and a moved selection re-windows the same grid. */
function disc(req: ElevationRequest) {
  const [lon0, lat0, lon1, lat1] = req.bbox;
  const margin = req.region === "rect" ? req.move_margin : 0;
  const span = (req.span_deg || Math.hypot(lon1 - lon0, lat1 - lat0)) + 2 * margin;
  return { span, lat0: (lat0 + lat1) / 2 - span / 2, lon0: (lon0 + lon1) / 2 - span / 2 };
}

/* Water and lake decisions, made once on the unrotated grid. Only the water
 * and relief knobs re-run this; the angle never does. */
export function prepare(raw: Raw, p: Pick<Params, "water_ntile" | "lake_flatness" | "vertical_ratio">): Prepared | null {
  const req = raw.request;
  const d = disc(req);
  const [, lat0, , lat1] = req.bbox;
  // Density compensation: the flatness threshold is expressed at the display
  // sampling (dlat/num_lines), so the disc-space decision measures the same
  // slope as upstream's 80x300 composition. Disc mode displays at disc
  // density: scale 1.
  const dStep = d.span / raw.nrows;
  const refStep = req.region === "disc" ? dStep : (lat1 - lat0) / req.num_lines;
  return preprocessGrid(
    raw.values, raw.nrows, raw.ncols,
    p.water_ntile, p.lake_flatness, p.vertical_ratio,
    dStep / refStep, req.region === "disc" ? null : req.bbox,
    d.lat0, d.lon0, d.span,
  );
}

/* The bbox whose disc-space image is the view window. The fetched disc is
 * bigger than the view (move_margin), so a moved selection is served by
 * re-windowing the SAME grid. rotatePlane maps a grid position p to the
 * source cell M·(p − c) + c, so world w appears at Mᵀ·(F(w) − c) + c; the
 * window for a bbox translated by o (from the fetched center) therefore
 * centers at F(center) + Mᵀ·o, i.e. the live bbox shifted by (Mᵀ − I)·o.
 * At angle 0 that is exactly the live bbox, so drags compose with the
 * rotation in screen space. Beyond the margin the window reads NaN
 * padding; the state layer then refetches. */
export function windowBbox(req: ElevationRequest, params: Params): Bbox {
  const [w, s, e, n] = params.bbox;
  const cLng = (req.bbox[0] + req.bbox[2]) / 2;
  const cLat = (req.bbox[1] + req.bbox[3]) / 2;
  const oLat = (s + n) / 2 - cLat;
  const oLng = (w + e) / 2 - cLng;
  const a = ((-params.viewpoint_angle % 360) + 360) % 360;
  const rad = (a * Math.PI) / 180;
  const cos = Math.cos(rad), sin = Math.sin(rad);
  const rLat = cos * oLat - sin * oLng; // Mᵀ · o
  const rLng = sin * oLat + cos * oLng;
  const dLat = rLat - oLat, dLng = rLng - oLng;
  return [w + dLng, s + dLat, e + dLng, n + dLat];
}

/* Rotate the prepared grid, sample the display window and lay it out. Null
 * when nothing of the terrain is in view. */
export function buildScene(raw: Raw, prepared: Prepared, params: Params): Scene | null {
  const req = raw.request;
  // Negative angle: preprocessing flips rows, and R(-t)*F == F*R(t). The
  // masks rotate rigidly with the terrain, so nothing flickers.
  const rotated = rotatePlane(prepared.grid, raw.nrows, raw.ncols, -params.viewpoint_angle);
  let grid = rotated, vrows = raw.nrows, vcols = raw.ncols;
  if (req.region !== "disc") {
    const d = disc(req);
    vrows = Math.min(req.num_lines, raw.nrows);
    vcols = Math.min(req.elevation_pts, raw.ncols);
    grid = sampleWindow(rotated, raw.nrows, d.lat0, d.lon0, d.span, windowBbox(req, params), vrows, vcols);
  }
  const rows = buildRows(grid, vrows, vcols);
  const bounds = frameBounds(rows, params.clip_to_land);
  if (bounds.empty) return null;
  const [lon0, lat0, lon1, lat1] = req.bbox;
  const bboxRatio = req.region === "disc" ? 1.0 : (lat1 - lat0) / (lon1 - lon0);
  return {
    rows,
    vmin: prepared.vmin,
    vmax: prepared.vmax,
    layout: buildLayout(bounds, params.size_scale, bboxRatio),
    style: buildStyle(params),
  };
}

export function buildStyle(p: Params): Style {
  const line = resolveLineColor(p.line_color);
  const labelColor = p.label_color
    ? parseColor(p.label_color)
    : line.type === "solid" ? line.rgb : evalCmap(line.name, 0);
  const [lon0, lat0, lon1, lat1] = p.bbox;
  return {
    line,
    kind: p.kind,
    background: parseColor(p.background_color),
    linewidth_pt: p.linewidth_pt,
    label: p.label
      ? {
          text: p.label,
          color: labelColor,
          x: p.label_x,
          y: p.label_y,
          size_pt: p.label_size_pt,
          font_family: p.label_font,
          background: true,
        }
      : null,
    annotation: p.annotation
      ? {
          label: p.annotation.label || "",
          x: (p.annotation.lon - lon0) / (lon1 - lon0),
          y: (p.annotation.lat - lat0) / (lat1 - lat0),
          x_offset: 0.005,
          y_offset: 0.005,
          label_size_pt: p.annotation.label_size_pt ?? 20,
          dot_pt: p.annotation.dot_pt ?? 8,
          color: p.annotation.color ? parseColor(p.annotation.color) : labelColor,
        }
      : null,
  };
}
