#!/usr/bin/env python3
"""Capture the figure the LEGACY ridge_map draws, as a fixture.

``gen_parity_fixtures.py`` pins the frozen ports in ``src/upstream`` against
numpy/scipy/skimage. This is the other half of the contract: the *composition*
— where the axes end up — produced by the real upstream package plus
matplotlib. ``ridge-core``'s ``RidgeScene::from_grid`` has to reproduce
matplotlib's autoscale, not a hand-derived idea of it, so the only honest
reference is running the legacy code.

The grid is deterministic (no tile fetching) and carries the shapes that
exposed the frame bug: low ground along the west edge, which the water
percentile masks into whole water *columns*, and a missing-data corner, which
turns into whole water *rows*. matplotlib frames the whole window regardless,
so a scene stays in proportion instead of cropping back onto its land.

Run it inside the flake:

    nix develop -c sh -c 'cd scripts && uv sync && uv run gen_legacy_fixture.py'

Writes ``fixtures/legacy/frame.json``.
"""

import json
import sys
import types
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent

# upstream ridge_map imports the `srtm` package to fetch tiles; this fixture
# never fetches, so a stub keeps the reference run offline.
sys.modules.setdefault("srtm", types.ModuleType("srtm"))
sys.modules["srtm"].get_data = lambda *args, **kwargs: None  # type: ignore[attr-defined]
sys.path.insert(0, str(ROOT / "ridge_map"))

import matplotlib  # noqa: E402

matplotlib.use("Agg")

from matplotlib.font_manager import FontProperties  # noqa: E402

from ridge_map import RidgeMap  # noqa: E402

OUT = ROOT / "fixtures" / "legacy"

# A Chattanooga-ish box: the report that exposed the clipping was a river
# town, so the water band sits along the edge of the box.
BBOX = (-85.57522077680608, 34.956367814108084, -85.00520661158271, 35.221757662288226)
NUM_LINES = 130
ELEVATION_PTS = 300
WATER_NTILE = 10.0
LAKE_FLATNESS = 2
VERTICAL_RATIO = 100.0
SIZE_SCALE = 30.0

# The composition from the report.
LABEL = "Chattanooga"
LABEL_X = 0.6
LABEL_Y = 0.1
LABEL_SIZE = 40
LINEWIDTH = 2


def raw_grid():
    """Deterministic raw elevations, shaped ``(num_lines, elevation_pts)``.

    A low border along the south and west edge sits below the water
    percentile, so masking turns it into whole water rows AND whole water
    columns; a void corner stands in for a tile the mirror does not carry.
    """
    rows, cols = NUM_LINES, ELEVATION_PTS
    r = np.arange(rows, dtype=float)[:, None]
    c = np.arange(cols, dtype=float)[None, :]
    border = (r < 8) | (c < 8)
    low = 20.0 + 3.0 * np.sin(r / 3.0) + 3.0 * np.cos(c / 5.0)
    interior = 120.0 + 180.0 * (c / (cols - 1)) + 40.0 * np.sin(r / 9.0) * np.cos(c / 17.0)
    values = np.where(border, low, interior)
    values[:12, -30:] = np.nan
    return np.round(values, 3)


def grid_json(arr):
    """2-D float array -> JSON (NaN becomes null)."""
    return [[None if not np.isfinite(v) else float(v) for v in row] for row in arr]


def main():
    values = raw_grid()
    rm = RidgeMap(BBOX, font=FontProperties())  # a stub font, so nothing is fetched
    processed = rm.preprocess(
        values=values.copy(),
        water_ntile=WATER_NTILE,
        lake_flatness=LAKE_FLATNESS,
        vertical_ratio=VERTICAL_RATIO,
    )
    ax = rm.plot_map(
        values=processed,
        label=LABEL,
        label_x=LABEL_X,
        label_y=LABEL_Y,
        label_size=LABEL_SIZE,
        linewidth=LINEWIDTH,
        size_scale=SIZE_SCALE,
    )
    xlim = [float(v) for v in ax.get_xlim()]
    ylim = [float(v) for v in ax.get_ylim()]
    fig = ax.get_figure()
    width, height = (float(v) for v in fig.get_size_inches())

    # The whole window, not just the land: water columns and rows keep their
    # share of the frame (matplotlib autoscales over `arange(ncols)` and the
    # per-row baselines, both of which are finite everywhere).
    xmax = values.shape[1] - 1
    assert xlim[0] <= 0.0 and xlim[1] >= xmax, f"frame does not span the columns: {xlim}"
    rows_with_data = np.nonzero(np.any(np.isfinite(processed), axis=1))[0]
    lowest_baseline = -6.0 * int(rows_with_data.max())
    assert ylim[0] <= lowest_baseline + 1e-9, f"frame misses the filled baselines: {ylim}"
    n_water_cols = int(np.all(~np.isfinite(processed), axis=0).sum())
    n_water_rows = int(np.all(~np.isfinite(processed), axis=1).sum())
    assert n_water_cols and n_water_rows, (
        f"fixture must exercise both directions (water cols={n_water_cols}, rows={n_water_rows})"
    )

    # Where the LEGACY frame actually stops. matplotlib autoscales tightly
    # around the cells it draws, so the all-water columns and rows above drop
    # out of it: its x starts at the first column carrying land, and its floor
    # is the last baseline that has any. ridge-core deliberately frames the
    # whole requested window instead, so the water_ntile knob shapes the
    # ridges without rescaling the picture — the fixture pins both sides of
    # that difference.
    cols_with_data = np.nonzero(np.any(np.isfinite(processed), axis=0))[0]
    legacy_extent = {
        "xmin": int(cols_with_data.min()),
        "xmax": int(cols_with_data.max()),
        "lowest_baseline": float(lowest_baseline),
    }

    doc = {
        "bbox": list(BBOX),
        "num_lines": NUM_LINES,
        "elevation_pts": ELEVATION_PTS,
        "water_ntile": WATER_NTILE,
        "lake_flatness": LAKE_FLATNESS,
        "vertical_ratio": VERTICAL_RATIO,
        "size_scale": SIZE_SCALE,
        "input": grid_json(values),
        "expected": {
            "xlim": xlim,
            "ylim": ylim,
            "figure_inches": [width, height],
        },
        "legacy_extent": legacy_extent,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / "frame.json"
    path.write_text(json.dumps(doc))

    print(f"wrote {path}")
    print(f"  shape          {values.shape}")
    print(f"  water cols     {n_water_cols} / {values.shape[1]}")
    print(f"  water rows     {n_water_rows} / {values.shape[0]}")
    print(f"  legacy xlim    {xlim}")
    print(f"  legacy ylim    {ylim}")
    print(f"  legacy extent  cols {legacy_extent['xmin']}..{legacy_extent['xmax']}"
          f", floor {legacy_extent['lowest_baseline']}")
    print(f"  window         cols 0..{values.shape[1] - 1}"
          f", floor {-(values.shape[0] - 1) * 6}")
    print(f"  figure inches  {width} x {height}")


if __name__ == "__main__":
    main()
