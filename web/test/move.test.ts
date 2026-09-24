// The geometry of moving the selection: the window is the live bbox,
// translated by the view rotation around the fetched disc's center. Pure
// formula, so the tests are exact — no grids, no magic cell values.
import { describe, expect, test } from "vitest";
import { DEFAULTS } from "../src/lib/params.ts";
import { windowBbox } from "../src/lib/scene.ts";
import type { ElevationRequest } from "../src/lib/params.ts";

const REQ: ElevationRequest = {
  bbox: [0, 0, 10, 10],
  num_lines: 10,
  elevation_pts: 10,
  region: "rect",
  span_deg: 10,
  move_margin: 0,
};

const at = (bbox: ElevationRequest["bbox"], angle: number) =>
  windowBbox(REQ, { ...DEFAULTS, bbox, viewpoint_angle: angle });

const closeTo = (actual: number[], expected: number[]) => {
  actual.forEach((v, i) => expect(v).toBeCloseTo(expected[i], 10));
};

describe("windowBbox", () => {
  test("an unmoved bbox is the fetched bbox, at any angle", () => {
    expect(windowBbox(REQ, { ...DEFAULTS, bbox: [0, 0, 10, 10], viewpoint_angle: 0 })).toEqual([0, 0, 10, 10]);
    closeTo(at([0, 0, 10, 10], 37.5), [0, 0, 10, 10]);
  });

  test("angle 0: the window is the live bbox", () => {
    expect(at([3, 2, 13, 12], 0)).toEqual([3, 2, 13, 12]);
  });

  test("180° moves the window to the opposite side of the disc", () => {
    // Live center (8, 7) = disc center (5, 5) + (3, 2); flipped: (2, 3).
    closeTo(at([3, 2, 13, 12], 180), [-3, -2, 7, 8]);
  });

  test("90° moves the window along the rotated axes", () => {
    // Mᵀ·(2, 3) = (3, -2): window center (8, 3).
    closeTo(at([3, 2, 13, 12], 90), [-2, 3, 8, 13]);
  });
});
