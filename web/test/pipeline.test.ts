// Framing: by default the layout spans the whole requested window, not just
// the cells that hold land, so masking water (or a bbox over a tile with no
// data) cannot rescale the scene. `clipToLand` opts back into the legacy
// matplotlib autoscale, which crops to the land. Mirrors
// ridge_core::RidgeScene::from_grid.
import { describe, expect, test } from "vitest";
import { buildLayout, buildRows, frameBounds } from "../src/lib/pipeline.ts";

const rows = (grid: number[][]) =>
  buildRows(Float64Array.from(grid.flat()), grid.length, grid[0].length);

/* Top two rows, bottom row and left three columns are all water (NaN): the
 * shapes that exposed the clipping. Rows 2..4 carry 20.0. */
const waterEdged = () =>
  rows(
    Array.from({ length: 6 }, (_, r) =>
      Array.from({ length: 10 }, (_, c) => (r < 2 || r === 5 || c < 3 ? NaN : 20)),
    ),
  );

describe("frame bounds", () => {
  test("the default frames the whole window, water columns and rows included", () => {
    expect(frameBounds(waterEdged())).toEqual({
      xmin: 0, // every column, water or not
      xmax: 9,
      ymin: -30, // every row, down to the last baseline
      ymax: 8, // row 2: 20 + its baseline of -12
      empty: false,
    });
  });

  test("clip to land reproduces the legacy crop", () => {
    expect(frameBounds(waterEdged(), true)).toEqual({
      xmin: 3, // matplotlib frames only the columns it draws...
      xmax: 9,
      ymin: -24, // ...and stops at the last baseline carrying land (row 4)
      ymax: 8,
      empty: false,
    });
  });

  test("masking water never moves the default frame", () => {
    const land = rows([[40, 40, 40], [40, 40, 40], [40, 40, 40]]);
    const half = rows([[NaN, 40, 40], [NaN, 40, 40], [NaN, 40, 40]]);
    const a = frameBounds(land);
    expect(a).toEqual({ xmin: 0, xmax: 2, ymin: -12, ymax: 40, empty: false });
    expect(frameBounds(half)).toEqual(a);
    // ...while clip-to-land does move, which is the legacy behaviour.
    expect(frameBounds(half, true).xmin).toBe(1);
  });

  test("no terrain at all is empty", () => {
    const none = rows([[NaN, NaN], [NaN, NaN]]);
    expect(frameBounds(none).empty).toBe(true);
    expect(frameBounds(none, true).empty).toBe(true);
  });

  test("the layout pads the window by 5% a side", () => {
    const layout = buildLayout(frameBounds(waterEdged()), 20, 1);
    expect(layout.xlim[0]).toBeCloseTo(-0.45, 12); // (10 - 1) * 0.05
    expect(layout.xlim[1]).toBeCloseTo(9.45, 12);
    expect(layout.ylim[0]).toBeCloseTo(-31.9, 12); // (-30 .. 8) * 0.05
    expect(layout.ylim[1]).toBeCloseTo(9.9, 12);
  });
});
