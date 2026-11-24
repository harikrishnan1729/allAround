#!/usr/bin/env bash
# Records a gravity-sensor trace from a debug build (which logs "FaceTiltTrace" lines: time and
# the 3-axis gravity vector — never camera data) into the CSV format used by
# crates/core/tests/traces/. Rotate the phone, then press Ctrl-C.
# Usage: scripts/record_trace.sh my_scenario "head: Upright" "expect: R0"
set -euo pipefail
name="${1:?usage: $0 <scenario-name> [\"key: value\" ...]}"; shift
out="crates/core/tests/traces/${name}.csv"
{
  echo "# RECORDED on $(adb shell getprop ro.product.model | tr -d '\r') ($(date -u +%F))"
  echo "# scenario: $name"
  echo "# sensor: $(adb logcat -d -s FaceTilt | grep -o 'sensor: [a-z]*' | tail -1 | cut -d' ' -f2)"
  for kv in "$@"; do echo "# $kv"; done
  echo "t_ms,gx,gy,gz"
} > "$out"
adb logcat -c
echo "Recording to $out — rotate the phone, Ctrl-C to stop."
trap 'python3 - "$out" <<"PY"
import sys
p=sys.argv[1]; lines=open(p).read().splitlines()
hdr=[l for l in lines if l.startswith("#") or l.startswith("t_ms")]
rows=[l for l in lines if l and l[0].isdigit()]
t0=int(rows[0].split(",")[0]) if rows else 0
rows=[",".join([str(int(r.split(",")[0])-t0)]+r.split(",")[1:]) for r in rows]
open(p,"w").write("\n".join(hdr+rows)+"\n"); print(len(rows),"samples")
PY' EXIT
adb logcat -v raw -s FaceTiltTrace:D | grep --line-buffered -E '^[0-9]+,' >> "$out"
