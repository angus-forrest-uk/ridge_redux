// The app state's promise: one elevation fetch per location and resolution.
// The angle, water, relief and style knobs recompute the scene locally,
// without touching the network.
import { createRoot } from "solid-js";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { DEFAULTS, selectionBbox, startsSelection } from "../src/lib/params.ts";
import { createRidgeState, type RidgeState } from "../src/state.ts";

// A 10x10 grid with an ocean void and a flat patch, the shape the backend returns.
const GRID = Array.from({ length: 10 }, (_, r) =>
  Array.from({ length: 10 }, (_, c) =>
    r === 1 && c >= 6 ? null : r === 5 && c >= 3 && c <= 5 ? 500 : 100 + 20 * r + 3 * c));

const elevationRequests: Record<string, unknown>[] = [];
const settle = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));

let state: RidgeState;
let dispose: () => void;

beforeEach(async () => {
  elevationRequests.length = 0;
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    if (url !== "/api/elevation") throw new Error(`unexpected fetch ${url}`);
    elevationRequests.push(JSON.parse(String(init?.body)));
    return new Response(JSON.stringify({ shape: [10, 10], values: GRID }));
  });
  createRoot((d) => {
    dispose = d;
    state = createRidgeState(DEFAULTS);
  });
  await settle();
});

afterEach(() => {
  dispose();
  vi.unstubAllGlobals();
});

describe("loading", () => {
  test("fetches the grid once, immediately, and builds a scene", () => {
    expect(elevationRequests).toHaveLength(1);
    expect(state.scene()?.rows.length).toBeGreaterThan(0);
    expect(state.status().text).toContain("80 × 300");
  });

  test("the request carries no angle: rotation is local", () => {
    expect(elevationRequests[0]).not.toHaveProperty("viewpoint_angle");
    expect(elevationRequests[0]).toMatchObject({ num_lines: 80, elevation_pts: 300, region: "rect" });
  });
});

describe("local changes redraw without fetching", () => {
  test.each([
    ["viewpoint_angle", 45],
    ["water_ntile", 40],
    ["vertical_ratio", 120],
    ["line_color", "viridis"],
    ["size_scale", 30],
  ] as const)("%s", async (key, value) => {
    const before = state.scene();
    state.set(key, value);
    await settle(400);
    expect(state.scene()).not.toBe(before);
    expect(elevationRequests).toHaveLength(1);
  });
});

describe("data changes refetch", () => {
  test("resolution changes are debounced into one fetch", async () => {
    state.set("num_lines", 90);
    state.set("num_lines", 100);
    expect(state.status().text).toBe("queued…");
    await settle(400);
    expect(elevationRequests).toHaveLength(2);
    expect(elevationRequests[1]).toMatchObject({ num_lines: 100 });
  });

  test("a new area derives its span from that area", async () => {
    // A 0.04-degree box must not inherit the old 1.2-degree span: that would
    // blow the grid up to thousands of samples a side.
    state.setBbox([172.633667, -43.63036, 172.670403, -43.605256]);
    await settle(400);
    const req = elevationRequests[1] as { bbox: number[]; span_deg: number };
    expect(req.bbox[0]).toBe(172.633667);
    expect(req.span_deg).toBeGreaterThan(0.03);
    expect(req.span_deg).toBeLessThan(0.1);
  });

  test("a preset fetches right away and re-centers the map", async () => {
    const recenter = state.recenter();
    state.applyPreset({ name: "Austin", bbox: [-97.794285, 30.232226, -97.710171, 30.334509], params: { num_lines: 60 } });
    await settle();
    expect(elevationRequests).toHaveLength(2);
    expect(state.recenter()).toBe(recenter + 1);
    expect(state.params.label).toBe(DEFAULTS.label); // everything else back to the defaults
  });

  test("an imported config fetches, re-centers and resets the rest", async () => {
    const recenter = state.recenter();
    state.applyConfig({ num_lines: 60, label: "Imported" });
    await settle();
    expect(elevationRequests).toHaveLength(2);
    expect(elevationRequests[1]).toMatchObject({ num_lines: 60 });
    expect(state.recenter()).toBe(recenter + 1);
    expect(state.params.label).toBe("Imported");
    expect(state.params.viewpoint_angle).toBe(DEFAULTS.viewpoint_angle);
  });
});

describe("map selection", () => {
  test("the select tool draws; the move tool draws only with Shift", () => {
    expect(startsSelection("select", false)).toBe(true);
    expect(startsSelection("move", false)).toBe(false);
    expect(startsSelection("move", true)).toBe(true);
  });

  test("a drag becomes a bbox clamped to SRTM coverage", () => {
    expect(selectionBbox({ lat: 58, lng: -71 }, { lat: 70, lng: -70 })).toEqual([-71, 58, -70, 60]);
  });

  test("a click is not a selection", () => {
    expect(selectionBbox({ lat: 44, lng: -71 }, { lat: 44.01, lng: -71.01 })).toBeNull();
  });
});
