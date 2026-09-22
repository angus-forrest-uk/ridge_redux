#!/usr/bin/env bash
# Turn a `cargo llvm-cov --json --summary-only` report into a shields.io
# endpoint badge (https://shields.io/badges/endpoint-badge), which the
# README's coverage chip renders.
#
# Usage: scripts/coverage_badge.sh coverage.json badge/coverage.json
set -euo pipefail

report=${1:?usage: coverage_badge.sh REPORT OUT}
out=${2:?usage: coverage_badge.sh REPORT OUT}

pct=$(python3 -c 'import json, sys
try:
    print(json.load(open(sys.argv[1]))["data"][0]["totals"]["lines"]["percent"])
except Exception:
    pass' "$report" 2>/dev/null) || true
if [ -z "$pct" ]; then
    echo "coverage_badge: no line coverage in $report" >&2
    exit 1
fi
pct=$(printf '%.1f' "$pct")
color=$(awk -v p="$pct" 'BEGIN {
    if (p >= 90) print "brightgreen";
    else if (p >= 80) print "green";
    else if (p >= 70) print "yellowgreen";
    else if (p >= 60) print "yellow";
    else if (p >= 50) print "orange";
    else print "red";
}')

mkdir -p "$(dirname "$out")"
printf '{"schemaVersion":1,"label":"coverage","message":"%s%%","color":"%s"}\n' "$pct" "$color" >"$out"
echo "coverage_badge: $pct% ($color) -> $out"
