#!/usr/bin/env bash
# Aggregate per-phase elapsed_ms from a theater stderr capture.
# Usage: ./analyze.sh bench.log

set -uo pipefail
log="${1:-bench.log}"

if [ ! -f "$log" ]; then
  echo "no file: $log" >&2
  exit 1
fi

# Find every distinct phase mentioned, then compute stats for each.
phases=$(grep -oE 'phase="?[a-z._]+"?' "$log" | sed 's/^phase=//; s/"//g' | sort -u)

printf '%-40s %6s %8s %8s %8s %8s %8s\n' phase n min p50 p95 p99 max
printf '%-40s %6s %8s %8s %8s %8s %8s\n' ---- - --- --- --- --- ---

for p in $phases; do
  vals=$(grep -E "phase=\"?${p}\"?[, ]" "$log" \
    | grep -oE 'elapsed_ms=[0-9]+' \
    | sed 's/elapsed_ms=//' \
    | sort -n)
  n=$(printf '%s\n' "$vals" | grep -c '.' || true)
  if [ "${n:-0}" -eq 0 ]; then continue; fi
  arr=( $vals )
  min=${arr[0]}
  max=${arr[$((n-1))]}
  p50_idx=$(( n * 50 / 100 ))
  p95_idx=$(( n * 95 / 100 ))
  p99_idx=$(( n * 99 / 100 ))
  [ "$p50_idx" -ge "$n" ] && p50_idx=$((n-1))
  [ "$p95_idx" -ge "$n" ] && p95_idx=$((n-1))
  [ "$p99_idx" -ge "$n" ] && p99_idx=$((n-1))
  printf '%-40s %6d %8d %8d %8d %8d %8d\n' "$p" "$n" "$min" "${arr[$p50_idx]}" "${arr[$p95_idx]}" "${arr[$p99_idx]}" "$max"
done
