#!/bin/sh
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -f "$fixture/greeting.txt" "$fixture/result.json" "$fixture/events.jsonl"; rmdir "$fixture"' EXIT
cp "$example_dir/greeting.txt" "$fixture/greeting.txt"

if ! sb --allow-write --json --json-events --max-turns 6 \
  --max-inference-calls 12 --timeout 150 -w "$fixture" -p - \
  < "$example_dir/task.txt" > "$fixture/result.json" 2> "$fixture/events.jsonl"; then
  printf '%s\n' 'FAIL: sb returned an error.' >&2
  cat "$fixture/result.json" "$fixture/events.jsonl" >&2
  exit 1
fi

cat "$fixture/result.json"
if ! printf 'hello new\n' | cmp -s - "$fixture/greeting.txt"; then
  printf '%s\n' 'FAIL: greeting.txt does not contain the exact expected replacement.' >&2
  cat "$fixture/events.jsonl" >&2
  exit 1
fi
printf '%s\n' 'PASS: greeting.txt contains exactly hello new followed by one newline.'
