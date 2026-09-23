// Framing: the layout spans the whole requested window, not just the cells
// that hold land, so masking water (or a bbox over a tile with no data) cannot
// rescale the scene. Mirrors ridge_core::RidgeScene::from_grid.
import { describe, expect, test } from "vitest";
import { buildLayout, buildRows, frameBounds } from "../src/lib/pipeline.ts";

const rows = (grid: number[][]) =>
  buildRows(Float64Array.from(grid.flat()), grid.length, grid[0].length);

describe("frame bounds", () => {
  test("spans the whole window, water columns and rows included", () => {
    const land = [[40, 40, 40], [40, 40, 40], [40, 40, 40]];
    // The left column and the top row are entirely water (NaN).
    const water = [[NaN, 40, 40], [NaN, 40, 40], [NaN, 40, 40]];
    const a = frameBounds(rows(land));
    expect(a).toEqual({ xmin: 0, xmax: 2, ymin: -12, ymax: 40, empty: false });
    expect(frameBounds(rows(water))).toEqual(a);
  });

  test("no terrain at all is empty", () => {
    expect(frameBounds(rows([[NaN, NaN], [NaN, NaN]])).empty).toBe(true);
  });

  test("the layout pads the window by 5% a side", () => {
    const layout = buildLayout(frameBounds(rows([[40, 40, 40], [40, 40, 40], [40, 40, 40]])), 20, 1);
    expect(layout.xlim[0]).toBeCloseTo(-0.1, 12); // (3 - 1) * 0.05
    expect(layout.xlim[1]).toBeCloseTo(2.1, 12);
    expect(layout.ylim[0]).toBeCloseTo(-14.6, 12); // (-12 .. 40) * 0.05
    expect(layout.ylim[1]).toBeCloseTo(42.6, 12);
  });
});
