#!/usr/bin/env bash
# Runs the Python suite ITERATIONS times in fresh processes, as the CI job does once, and
# records every native crash with a gdb backtrace of its core dump.
set -uo pipefail

out="$RUNNER_TEMP/results/$VARIANT-$SHARD"
mkdir -p "$out" /tmp/cores
sudo sysctl -q -w kernel.core_pattern=/tmp/cores/core.%p
ulimit -c unlimited
python_bin=$(python -c 'import os, sys; print(os.path.realpath(sys.executable))')

passed=0 crashed=0 failed=0
for i in $(seq 1 "$ITERATIONS"); do
  rm -f /tmp/cores/core.*
  python -X faulthandler -m pytest -q -p no:cacheprovider >"$out/run-$i.log" 2>&1
  rc=$?
  if [ "$rc" -eq 0 ]; then
    passed=$((passed + 1))
    rm "$out/run-$i.log"
  elif [ "$rc" -ge 128 ]; then
    crashed=$((crashed + 1))
    echo "::warning::$VARIANT shard $SHARD run $i died with signal $((rc - 128))"
    for core in /tmp/cores/core.*; do
      [ -e "$core" ] || continue
      gdb -batch -nx -ex 'info sharedlibrary' -ex 'thread apply all bt 40' "$python_bin" "$core" \
        >"$out/run-$i.$(basename "$core").bt" 2>&1
    done
  else
    failed=$((failed + 1))
    echo "::warning::$VARIANT shard $SHARD run $i failed with exit code $rc"
  fi
done

printf '%s %s %s %s %s\n' "$VARIANT" "$SHARD" "$passed" "$crashed" "$failed" >"$out/summary.txt"
cat "$out/summary.txt"
