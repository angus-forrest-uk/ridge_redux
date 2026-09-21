/* Render parameters: the shape the API takes, with their defaults, and the
 * geography rules the UI applies to them. */
import type { Bbox } from "./pipeline.ts";

export interface Annotation {
  lon: number;
  lat: number;
  label: string;
  x_offset?: number;
  y_offset?: number;
  label_size_pt?: number;
  dot_pt?: number;
  color?: string;
  background?: boolean;
}

export interface Params {
  bbox: Bbox;
  num_lines: number;
  elevation_pts: number;
  viewpoint_angle: number;
  interpolation: number;
  water_ntile: number;
  lake_flatness: number;
  vertical_ratio: number;
  linewidth_pt: number;
  line_color: string;
  kind: "gradient" | "elevation";
  background_color: string;
  size_scale: number;
  label: string;
  label_x: number;
  label_y: number;
  label_size_pt: number;
  label_font: string;
  label_color?: string;
  annotation: Annotation | null;
  fit: "plane";
  region: "rect" | "disc";
  span_deg: number;
}

export const DEFAULTS: Params = {
  bbox: [-71.928864, 43.758201, -70.957947, 44.465151],
  // Upstream's composition: 80 lines x 300 points over the bbox. The
  // anisotropic window keeps it at every angle.
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
  region: "rect", // "rect" = the bbox; "disc" = rotation-invariant circle
  span_deg: 0, // disc side in degrees; 0 = bbox diagonal (nothing cut)
};

/* The parameters that change the sampled data, so a change needs a new
 * /api/elevation fetch. Everything else is redrawn locally. */
export type ElevationRequest = Pick<Params, "bbox" | "num_lines" | "elevation_pts" | "region" | "span_deg">;

export const SRTM_LAT_MAX = 60; // SRTM covers 60S..60N; the server rejects beyond it

export const clampLat = (v: number) => Math.min(SRTM_LAT_MAX, Math.max(-SRTM_LAT_MAX, v));
export const clampLng = (v: number) => Math.min(180, Math.max(-180, v));

/* The disc that always contains the bbox: its diagonal, in degrees. */
export function diagonal(bbox: Bbox, digits = 6): number {
  const [lon0, lat0, lon1, lat1] = bbox;
  return Number(Math.hypot(lon1 - lon0, lat1 - lat0).toFixed(digits));
}

export interface LatLng {
  lat: number;
  lng: number;
}

/* A bbox from two dragged corners, clamped to SRTM coverage. Null for a
 * drag too small to be anything but an accidental click. */
export function selectionBbox(a: LatLng, b: LatLng): Bbox | null {
  const w = clampLng(Math.min(a.lng, b.lng)), e = clampLng(Math.max(a.lng, b.lng));
  const s = clampLat(Math.min(a.lat, b.lat)), n = clampLat(Math.max(a.lat, b.lat));
  if (e - w < 0.02 || n - s < 0.02) return null;
  return [w, s, e, n].map((v) => Number(v.toFixed(6))) as Bbox;
}

export type MapTool = "move" | "select";

/* Whether a press on the map starts drawing a selection: always with the
 * select tool, and with Shift held under the move tool. */
export const startsSelection = (tool: MapTool, shiftKey: boolean) => tool === "select" || shiftKey;

/* Parameters from the URL hash (the permalink), over the defaults. */
export function paramsFromHash(hash: string): Params {
  if (hash.length > 1) {
    try {
      return { ...DEFAULTS, ...JSON.parse(decodeURIComponent(hash.slice(1))) };
    } catch { /* bad hash: keep the defaults */ }
  }
  return { ...DEFAULTS };
}

export const paramsToHash = (params: Params) => "#" + encodeURIComponent(JSON.stringify(params));
