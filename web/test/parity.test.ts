// The browser pipeline must produce the same rows and layout as the Rust
// pipeline for the same grid. The fixture comes from
// `cargo run -p ridge-core --example dump_plane_fixture` (`just test-web`
// writes it first): a raw grid, and the rows and layout Rust computed for
// it at -33 degrees.
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { buildLayout, buildRows, contentBounds, preprocessGrid, rotatePlane } from "../src/lib/pipeline.ts";

const FIXTURE = "/tmp/plane_fixture.json";

function run() {
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));
  const nrows = fixture.input.length;
  const ncols = fixture.input[0].length;
  const raw = Float64Array.from(fixture.input.flat(), (v: number | null) => v ?? NaN);
  // Decisions on the unrotated grid, then rotate the masked result: the order
  // (and angle) the fixture was produced with.
  const pp = preprocessGrid(raw, nrows, ncols, 10, 3, 40);
  const rows = pp ? buildRows(rotatePlane(pp.grid, nrows, ncols, -33), nrows, ncols) : [];
  return { fixture, pp, rows };
}

// Without the fixture the suite is skipped, unless CI requires it.
describe.runIf(existsSync(FIXTURE) || process.env.RIDGE_REQUIRE_FIXTURES)("JS pipeline == Rust pipeline", () => {
  test("rows match bit for bit", () => {
    const { fixture, pp, rows } = run();
    expect(pp).not.toBeNull();
    expect(rows.length).toBe(fixture.expected_rows.length);
    let worst = 0;
    rows.forEach((js, r) => {
      const rs = fixture.expected_rows[r];
      expect(js.baseline).toBe(rs.baseline);
      js.y.forEach((a, c) => {
        const b = rs.y[c];
        const gapJs = Number.isNaN(a), gapRs = b === null || Number.isNaN(b);
        if (gapJs !== gapRs) throw new Error(`row ${r} col ${c}: gap mismatch js=${a} rust=${b}`);
        if (!gapJs) worst = Math.max(worst, Math.abs(a - b));
      });
    });
    expect(worst).toBeLessThanOrEqual(1e-9);
  });

  test("layout matches", () => {
    const { fixture, rows } = run();
    const layout = buildLayout(contentBounds(rows), 20, 24 / 30); // the fixture's size and bbox ratio
    for (const key of ["width_px", "height_px", "xlim", "ylim"] as const) {
      expect(layout[key]).toEqual(fixture.layout[key]);
    }
    layout.axes.forEach((v, i) => expect(v).toBeCloseTo(fixture.layout.axes[i], 9));
  });
});
