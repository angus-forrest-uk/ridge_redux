#!/usr/bin/env python3
"""Generate the Python-reference fixtures that ridge-core's frozen ports (in
``src/upstream/``) are tested against.

For every ported function we emit input/output pairs produced by the *actual*
reference library — numpy, scipy.ndimage, skimage — so the Rust port is proven
faithful against the thing it claims to reproduce, not against hand-computed
values.

Run it inside the flake (which supplies uv + python3.12):

    nix develop -c sh -c 'cd scripts && uv sync && uv run gen_parity_fixtures.py'

Writes ``fixtures/parity/{numpy,scipy_rotate,skimage}.json``. The Rust side
(``crates/ridge-core/tests/parity.rs``) loads these and skips when absent,
unless ``RIDGE_REQUIRE_FIXTURES=1`` is set (CI).
"""

import json
from pathlib import Path

import numpy as np
import scipy
import skimage
from scipy import ndimage
from skimage.filters import rank
from skimage.morphology import footprint_rectangle

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "fixtures" / "parity"

SEED = 20240923


def grid(arr):
    """2-D float array -> JSON (NaN/inf become null; scipy can return both)."""
    arr = np.asarray(arr, dtype=float)
    return [[None if not np.isfinite(v) else float(v) for v in row] for row in arr]


def rotate_coords(in_shape, angle, reshape, out_shape):
    """The source coordinate each output cell samples, using the same affine as
    scipy.ndimage.rotate (and ridge-core's port)."""
    nrows, ncols = in_shape
    rad = np.deg2rad(angle)
    c, s = np.cos(rad), np.sin(rad)
    M = np.array([[c, s], [-s, c]])
    if reshape:
        corners = np.array([[0, 0], [0, ncols], [nrows, 0], [nrows, ncols]], float)
        out_plane = (np.ptp(M @ corners.T, axis=1) + 0.5).astype(int)
    else:
        out_plane = np.array([nrows, ncols])
    out_center = M @ ((out_plane - 1) / 2.0)
    in_center = (np.array([nrows, ncols]) - 1) / 2.0
    offset = in_center - out_center
    orows, ocols = out_shape
    oc, orr = np.meshgrid(np.arange(ocols, dtype=float), np.arange(orows, dtype=float))
    return c * orr + s * oc + offset[0], -s * orr + c * oc + offset[1]


def tie_grid(in_shape, angle, reshape, out_shape, eps=1e-6):
    """Cells whose order-0 rounding sits on a floating-point knife edge, where
    scipy's vectorized dot and our per-cell dot may round a half-integer
    coordinate to different cells. Mismatches are only ever excused here."""
    pr, pc = rotate_coords(in_shape, angle, reshape, out_shape)
    tr = np.abs((pr + 0.5) - np.round(pr + 0.5)) < eps
    tc = np.abs((pc + 0.5) - np.round(pc + 0.5)) < eps
    return (tr | tc).tolist()


def versions():
    return {
        "python": f"{__import__('sys').version_info.major}.{__import__('sys').version_info.minor}",
        "numpy": np.__version__,
        "scipy": scipy.__version__,
        "skimage": skimage.__version__,
    }


def numpy_cases():
    """numpy.percentile(..., method='linear'), our `percentile_linear`."""
    rng = np.random.default_rng(SEED)
    arrays = [
        np.arange(1, 11, dtype=float),
        np.array([5.0]),
        np.array([3.0, 3.0, 3.0]),
        np.array([-4.0, -1.0, 0.0, 2.5, 10.0, 10.0]),
        rng.normal(size=100),
        rng.integers(0, 255, size=50).astype(float),
    ]
    qs = [0.0, 1.0, 10.0, 25.0, 33.3, 50.0, 66.7, 75.0, 90.0, 99.0, 100.0]
    cases = []
    for a in arrays:
        s = np.sort(a)
        for q in qs:
            cases.append(
                {
                    "sorted": [float(v) for v in s],
                    "q": float(q),
                    "expected": float(np.percentile(s, q)),
                }
            )
    return cases


def scipy_rotate_cases():
    """scipy.ndimage.rotate(mode='constant', cval=0.0), our `rotate`."""
    rng = np.random.default_rng(SEED + 1)
    shapes = [(5, 7), (8, 8), (3, 10), (12, 4)]
    angles = [0.0, 90.0, 180.0, 270.0, -90.0, 45.0, 33.5, 11.0, 137.0]
    cases = []
    for shape in shapes:
        base = rng.normal(size=shape) * 100.0
        for angle in angles:
            for reshape in (True, False):
                for order in (0, 1):
                    out = ndimage.rotate(
                        base, angle, reshape=reshape, order=order, mode="constant", cval=0.0
                    )
                    cases.append(
                        {
                            "input": grid(base),
                            "angle": angle,
                            "reshape": reshape,
                            "order": order,
                            "expected": grid(out),
                            "tie": tie_grid(shape, angle, reshape, out.shape),
                        }
                    )
    # NaN propagation, nearest only (bilinear would smear, which our port does
    # not model). Out-of-bounds is cval 0.0, a finite cell.
    nan_case = rng.normal(size=(6, 6)) * 10.0
    nan_case[2, 3] = np.nan
    nan_case[0, 0] = np.nan
    for angle in (0.0, 45.0, 90.0, 210.0):
        out = ndimage.rotate(nan_case, angle, reshape=True, order=0, mode="constant", cval=0.0)
        cases.append(
            {
                "input": grid(nan_case),
                "angle": angle,
                "reshape": True,
                "order": 0,
                "expected": grid(out),
                "tie": tie_grid(nan_case.shape, angle, True, out.shape),
            }
        )
    return cases


def skimage_cases():
    """skimage.filters.rank.gradient, our `morphological_gradient`."""
    rng = np.random.default_rng(SEED + 2)
    images = [
        rng.integers(0, 256, size=(10, 12), dtype=np.uint8),
        (np.add.outer(np.arange(8) * 20, np.arange(9) * 10)).astype(np.uint8),
        np.full((9, 9), 128, dtype=np.uint8),
        (rng.integers(0, 2, size=(15, 15)) * 255).astype(np.uint8),
        np.tile(np.arange(14, dtype=np.uint8) * 18, (11, 1)),
    ]
    cases = []
    for img in images:
        for k in (1, 2):
            out = rank.gradient(img, footprint_rectangle((2 * k + 1, 2 * k + 1)))
            cases.append(
                {
                    "k": k,
                    "input": img.astype(int).tolist(),
                    "expected": out.astype(int).tolist(),
                }
            )
    return cases


def write(name, cases):
    doc = {"reference": versions(), "cases": cases}
    path = OUT / f"{name}.json"
    path.write_text(json.dumps(doc) + "\n")
    print(f"wrote {path.relative_to(ROOT)} ({len(cases)} cases, {path.stat().st_size // 1024} KiB)")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    print("reference:", versions())
    write("numpy", numpy_cases())
    write("scipy_rotate", scipy_rotate_cases())
    write("skimage", skimage_cases())


if __name__ == "__main__":
    main()
