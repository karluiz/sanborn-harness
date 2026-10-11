#!/bin/sh
set -eu

mode=${1:-both}
case "$mode" in
  scout|analyst|both) ;;
  *) printf '%s\n' 'Usage: sh harness/examples/specialists/run.sh [scout|analyst|both]' >&2; exit 2 ;;
esac
command -v sb >/dev/null
command -v python3 >/dev/null
example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fixture=$(mktemp -d)
workspace="$fixture/workspace"
trap 'rm -f "$workspace/README.md" "$workspace/src/greeting.rs" "$workspace/tests/greeting.rs" "$fixture/result.json" "$fixture/events.jsonl"; rmdir "$workspace/src" "$workspace/tests" "$workspace" "$fixture"' EXIT
mkdir "$workspace" "$workspace/src" "$workspace/tests"
cp "$example_dir/fixture/README.md" "$workspace/README.md"
cp "$example_dir/fixture/src/greeting.rs" "$workspace/src/greeting.rs"
cp "$example_dir/fixture/tests/greeting.rs" "$workspace/tests/greeting.rs"

if ! sb --json --json-events --max-turns 4 --max-inference-calls 12 \
  --timeout 150 -w "$workspace" -p - < "$example_dir/$mode.txt" \
  > "$fixture/result.json" 2> "$fixture/events.jsonl"; then
  printf '%s\n' 'FAIL: sb returned an error.' >&2
  cat "$fixture/result.json" "$fixture/events.jsonl" >&2
  exit 1
fi
cat "$fixture/result.json"
if ! python3 "$example_dir/check.py" "$mode" "$workspace" "$example_dir/fixture" "$fixture"; then
  cat "$fixture/events.jsonl" >&2
  exit 1
fi
