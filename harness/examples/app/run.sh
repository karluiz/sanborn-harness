#!/bin/sh
# Launch sanborn-app on a disposable copy of the greeting fixture.
#   sh harness/examples/app/run.sh            # synthetic reasoning, offline
#   sh harness/examples/app/run.sh azure      # real inference; may incur charges
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
manifest="$example_dir/../../Cargo.toml"
case "${1:-synthetic}" in
  synthetic) reasoning=--synthetic ;;
  azure) reasoning= ;;
  *) printf '%s\n' 'usage: run.sh [synthetic|azure]' >&2; exit 2 ;;
esac

cargo build --quiet --locked --manifest-path "$manifest" -p sanborn-app
fixture=$(mktemp -d)
trap 'rm -f "$fixture/greeting.txt"; rmdir "$fixture"' EXIT
# Ctrl+C stops the app; the script continues to the byte check and cleanup.
trap ':' INT
cp "$example_dir/greeting.txt" "$fixture/greeting.txt"

printf 'Disposable workspace: %s\n' "$fixture"
printf 'Task to paste:\n  %s\n\n' "$(cat "$example_dir/task.txt")"
status=0
"$example_dir/../../target/debug/sanborn-app" -w "$fixture" $reasoning || status=$?

# Independent byte check after the app exits; the app's own report is not trusted here.
if printf 'hello new\n' | cmp -s - "$fixture/greeting.txt"; then
  printf '%s\n' 'Applied: greeting.txt contains exactly hello new followed by one newline.'
elif printf 'hello old\n' | cmp -s - "$fixture/greeting.txt"; then
  printf '%s\n' 'Unchanged: greeting.txt still contains hello old (nothing was applied).'
else
  printf '%s\n' 'WARNING: greeting.txt contains unexpected bytes.' >&2
  status=1
fi
# 130 = stopped with Ctrl+C, the normal way to close the app.
[ "$status" -eq 0 ] || [ "$status" -eq 130 ] || exit "$status"
