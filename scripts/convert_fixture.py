#!/usr/bin/env python3
"""One-time fixture conversion: upstream npz -> raw f64 bin + shape json.

Uses only the Python stdlib (npz is a zip of .npy files; the .npy header is
trivial to parse). The result lets the Rust test-suite exercise the exact
elevation array the upstream tests use, without a Python dependency.

Usage: python3 scripts/convert_fixture.py
"""
import ast
import json
import struct
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "ridge_map" / "test" / "test_data" / "new_hampshire.npz"
OUT_BIN = ROOT / "fixtures" / "new_hampshire.f64.bin"
OUT_JSON = ROOT / "fixtures" / "new_hampshire.json"


def read_npy(data: bytes):
    if data[:6] != b"\x93NUMPY":
        raise ValueError("not a npy file")
    major, _minor = data[6], data[7]
    if major == 1:
        hlen = struct.unpack("<H", data[8:10])[0]
        offset = 10
    else:
        hlen = struct.unpack("<I", data[8:12])[0]
        offset = 12
    header = data[offset : offset + hlen].decode("latin1").strip()
    # header looks like: {'descr': '<f8', 'fortran_order': False, 'shape': (80, 300), }
    fields = ast.literal_eval(header.rstrip(" ,"))
    descr = fields["descr"]
    fortran_order = fields["fortran_order"]
    shape = tuple(int(s) for s in fields["shape"])
    if descr != "<f8":
        raise ValueError(f"unexpected dtype {descr}, only <f8 supported")
    if fortran_order:
        raise ValueError("fortran order not supported")
    body = data[offset + hlen :]
    expected = 1
    for s in shape:
        expected *= s
    if len(body) != expected * 8:
        raise ValueError(f"body size {len(body)} != {expected * 8}")
    floats = struct.unpack(f"<{expected}d", body)
    return shape, floats


def main():
    with zipfile.ZipFile(SRC) as zf:
        (name,) = zf.namelist()
        shape, floats = read_npy(zf.read(name))
    OUT_BIN.write_bytes(struct.pack(f"<{len(floats)}d", *floats))
    OUT_JSON.write_text(json.dumps({"shape": list(shape)}))
    print(f"wrote {OUT_BIN} ({len(floats) * 8} bytes) and {OUT_JSON} shape={shape}")


if __name__ == "__main__":
    main()
