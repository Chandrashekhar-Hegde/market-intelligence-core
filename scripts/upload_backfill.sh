#!/usr/bin/env bash
# Upload the workstation backfill to the server's volume through /api/v1/calibration/import, in
# chunks under the 2 MB body limit. Safe to rerun: records already there are skipped.
# Usage: BASE_URL=https://host GM_ACCESS_TOKEN=... scripts/upload_backfill.sh <data-dir>
set -euo pipefail
dir="${1:?usage: upload_backfill.sh <data-dir>}"
BASE_URL="${BASE_URL:-http://127.0.0.1:8000}"
auth=()
[ -n "${GM_ACCESS_TOKEN:-}" ] && auth=(-H "Authorization: Bearer $GM_ACCESS_TOKEN")
for pair in "backfill:news_backfill.jsonl" "model:model_readings.jsonl"; do
  file="${pair%%:*}"; path="$dir/${pair#*:}"
  [ -f "$path" ] || { echo "skip $file: no $path"; continue; }
  chunks="$(mktemp -d)"
  python3 - "$path" "$chunks" <<'PY'
import sys
src, out = sys.argv[1], sys.argv[2]
chunk, size, n = [], 0, 0
def flush():
    global chunk, size, n
    if chunk:
        open(f"{out}/{n:05d}.jsonl", "w").write("".join(chunk)); n += 1; chunk, size = [], 0
for line in open(src):
    if size + len(line.encode()) > 1_500_000: flush()
    chunk.append(line); size += len(line.encode())
flush()
PY
  added=0
  for c in "$chunks"/*.jsonl; do
    n="$(curl -fsS ${auth[@]+"${auth[@]}"} -X POST --data-binary @"$c" "$BASE_URL/api/v1/calibration/import?file=$file" | python3 -c 'import json,sys; print(json.load(sys.stdin)["added"])')"
    added=$((added + n))
  done
  rm -rf "$chunks"
  echo "$file: $added new records"
done
