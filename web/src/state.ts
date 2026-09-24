/* App state: the render parameters in a store, and everything derived from
 * them. Only the parameters that shape the sampled data (location and
 * resolution) reach the network; the water decisions and the scene are
 * memos, so the angle, water, relief and style knobs redraw locally. */
import { batch, createContext, createEffect, createMemo, createSignal, on, onCleanup, useContext } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { fetchElevation, fetchTiles, type Preset } from "./lib/api.ts";
import { DEFAULTS, diagonal, paramsToHash, type ElevationRequest, type Params } from "./lib/params.ts";
import type { Bbox } from "./lib/pipeline.ts";
import { buildScene, prepare, type Raw } from "./lib/scene.ts";

export interface Status {
  text: string;
  busy: boolean;
}

const REFETCH_DEBOUNCE_MS = 350;

const sameRequest = (a: ElevationRequest, b: ElevationRequest) =>
  JSON.stringify(a) === JSON.stringify(b);

/* A span of 0 means "the bbox diagonal"; resolve it so requests are explicit. */
const withSpan = (p: Params): Params => ({ ...p, span_deg: p.span_deg || diagonal(p.bbox, 4) });

export function createRidgeState(initial: Params = DEFAULTS) {
  const [params, setParams] = createStore<Params>(withSpan(initial));
  const [raw, setRaw] = createSignal<Raw>();
  const [status, setStatus] = createSignal<Status>({ text: "ready", busy: false });
  // Bumped when the map should re-center on the bbox (on load, on a preset).
  const [recenter, setRecenter] = createSignal(0);
  // The tiles the server has locally (in memory or on disk), for the map's
  // green coverage shading. Refreshed after each elevation fetch: the
  // prefetch grows the set as you move.
  const [tiles, setTiles] = createSignal<[number, number][]>([]);
  async function refreshTiles() {
    try {
      setTiles(await fetchTiles());
    } catch {
      /* shading is best-effort */
    }
  }

  const request = createMemo(
    (): ElevationRequest => ({
      bbox: [...params.bbox] as Bbox,
      num_lines: params.num_lines,
      elevation_pts: params.elevation_pts,
      region: params.region,
      span_deg: params.span_deg,
    }),
    undefined,
    { equals: sameRequest },
  );

  // Water and lake decisions: re-run for new data or the water/relief knobs.
  const prepared = createMemo(() => {
    const r = raw();
    return r ? prepare(r, {
      water_ntile: params.water_ntile,
      lake_flatness: params.lake_flatness,
      vertical_ratio: params.vertical_ratio,
    }) : null;
  });

  // The drawable scene: re-run for the angle and every style knob.
  const scene = createMemo(() => {
    const r = raw(), pp = prepared();
    return r && pp ? buildScene(r, pp, params) : null;
  });

  // Fetch the grid when the request changes: immediately on load and for a
  // preset, debounced while a slider or input is being moved.
  let fetchNow = true;
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  async function load(req: ElevationRequest) {
    const mine = ++seq;
    setStatus({ text: "fetching elevation…", busy: true });
    const t0 = performance.now();
    try {
      const data = await fetchElevation(req);
      if (mine !== seq) return; // a newer request is in flight
      setRaw(data);
      void refreshTiles();
      const ms = Math.round(performance.now() - t0);
      setStatus(scene()
        ? { text: `${req.num_lines} × ${req.elevation_pts} window · ${ms} ms · angle is local`, busy: false }
        : { text: "no data for this view", busy: false });
    } catch (e) {
      if (mine === seq) setStatus({ text: `error: ${(e as Error).message}`, busy: false });
    }
  }
  createEffect(on(request, (req) => {
    clearTimeout(timer);
    if (fetchNow) {
      fetchNow = false;
      load(req);
    } else {
      setStatus({ text: "queued…", busy: true });
      timer = setTimeout(() => load(req), REFETCH_DEBOUNCE_MS);
    }
  }));
  onCleanup(() => clearTimeout(timer));

  // A new window with no terrain in it (all ocean, say) after a local change.
  createEffect(() => {
    if (raw() && !scene() && !status().busy) setStatus({ text: "no data for this view", busy: false });
  });

  return {
    params,
    raw,
    scene,
    status,
    recenter,
    tiles,
    /* Set one parameter. */
    set<K extends keyof Params>(key: K, value: Params[K]) {
      setParams(key, value as never);
    },
    /* A new area. The disc follows it: a stale span from a previous area
     * would sample a huge disc around a small box. */
    setBbox(bbox: Bbox) {
      setParams({ bbox, span_deg: diagonal(bbox) });
    },
    /* Replace everything with a preset, fetch right away and re-center. */
    applyPreset(preset: Preset) {
      fetchNow = true;
      batch(() => {
        setParams(reconcile(withSpan({ ...DEFAULTS, ...preset.params, bbox: preset.bbox, fit: "plane" })));
        setRecenter((n) => n + 1);
      });
    },
    /* Load an imported configuration: reset to the defaults, overlay it,
     * resolve the span, fetch right away and re-center. */
    applyConfig(config: Partial<Params>) {
      fetchNow = true;
      batch(() => {
        setParams(reconcile(withSpan({ ...DEFAULTS, ...config, fit: "plane" })));
        setRecenter((n) => n + 1);
      });
    },
    /* The permalink for the current parameters. */
    hash: () => paramsToHash(params),
  };
}

export type RidgeState = ReturnType<typeof createRidgeState>;

export const RidgeContext = createContext<RidgeState>();

export function useRidge(): RidgeState {
  const state = useContext(RidgeContext);
  if (!state) throw new Error("useRidge() outside <RidgeContext.Provider>");
  return state;
}
