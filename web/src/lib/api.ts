/* The backend's HTTP API. */
import type { ElevationRequest, Params } from "./params.ts";
import type { Raw } from "./scene.ts";

export interface Preset {
  name: string;
  bbox: Params["bbox"];
  params: Partial<Params>;
}

async function postJson(url: string, body: unknown): Promise<Response> {
  return fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

/* The raw sampled grid for a location and resolution: whole metres, with
 * null for voids, which become NaN. */
export async function fetchElevation(request: ElevationRequest): Promise<Raw> {
  const resp = await postJson("/api/elevation", request);
  if (!resp.ok) {
    const err = await resp.json().catch(() => ({ error: resp.statusText }));
    throw new Error(err.error);
  }
  const data: { shape: [number, number]; values: (number | null)[][] } = await resp.json();
  const [nrows, ncols] = data.shape;
  const values = new Float64Array(nrows * ncols);
  data.values.forEach((row, r) => row.forEach((v, c) => { values[r * ncols + c] = v ?? NaN; }));
  return { nrows, ncols, values, request };
}

export async function fetchPresets(): Promise<Preset[]> {
  const resp = await fetch("/api/presets");
  return resp.ok ? resp.json() : [];
}

export async function fetchReadme(): Promise<string> {
  const resp = await fetch("/api/readme");
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.text();
}

/* Standalone SVG of the current view: `fit: "plane"` makes it match the
 * canvas exactly. */
export async function exportSvg(params: Params): Promise<string> {
  const resp = await postJson("/api/export.svg", { ...params, fit: "plane" });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.text();
}
