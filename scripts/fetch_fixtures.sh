#!/usr/bin/env bash
# Fetch the four SRTM tiles covering the default White Mountains bbox so the
# golden parity test can run offline afterwards.
set -euo pipefail
DIR="$(dirname "$0")/../fixtures/srtm"
mkdir -p "$DIR"
BASE="${RIDGE_SRTM_BASE:-https://srtm.kurviger.de/SRTM1/}"
REGION="${RIDGE_SRTM_REGION:-Region_06}"
for t in N43W072 N43W071 N44W072 N44W071; do
  if [ ! -f "$DIR/$t.hgt" ]; then
    echo "fetching $t"
    curl -sL "$BASE$REGION/$t.hgt.zip" -o "/tmp/$t.zip"
    python3 - "$DIR/$t.hgt" "/tmp/$t.zip" << 'PY'
import sys, zipfile
zipfile.ZipFile(sys.argv[2]).extractall("/tmp/hgtfix")
import shutil, glob, os
name = os.path.basename(sys.argv[1])
shutil.move(f"/tmp/hgtfix/{name}", sys.argv[1])
PY
  fi
done
echo "tiles ready in $DIR"
