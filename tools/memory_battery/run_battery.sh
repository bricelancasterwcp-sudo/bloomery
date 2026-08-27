#!/usr/bin/env bash
#
# tools/memory_battery/run_battery.sh -- OS-detached launcher for the
# memory-battery driver (design spec §5; task-3 brief).
#
# setsid+nohup so the run survives this shell exiting and its controlling
# terminal closing; a pid file so watch_battery.sh can find the real
# process; a trap-guaranteed `driver.DONE` marker so silence is never
# mistaken for success -- every termination path a shell trap CAN observe
# (a normal exit, an uncaught Python exception, any signal bash itself
# traps by default) writes the exit code before the detached shell exits.
# SIGKILL is the one death mode no trap anywhere can observe; that is
# exactly what watch_battery.sh's pid-death-without-marker branch exists
# to catch.
#
# The pid file is written by the Python driver itself (`--pid-file`), not
# captured here via `$!`: `setsid` double-forks when the invoking process
# is already a process-group leader (common for a backgrounded job), so
# `$!` can name an intermediate process that exits almost immediately --
# see docs/superpowers/evidence/2026-08-26-memory-organ-acceptance.md's
# own "real PID ... found via ps (never $!)" note. The child writing its
# own real pid sidesteps that ambiguity entirely.
#
# Usage: run_battery.sh <out_dir> [driver args...]
#   e.g. run_battery.sh out/arm-C --manifest corpus/manifest.json \
#        --base-url http://127.0.0.1:8397 --arm C \
#        --expected-digest 7020b925c07c...
# Run from the repository root (python3 -m tools.memory_battery.driver
# needs `tools` importable, exactly like this package's own test command).

set -uo pipefail

if [ "$#" -lt 1 ]; then
    echo "usage: run_battery.sh <out_dir> [driver args...]" >&2
    exit 2
fi

out_dir="$1"
shift

mkdir -p "$out_dir"
rm -f "$out_dir/driver.DONE" "$out_dir/driver.pid"

setsid nohup bash -c '
    out_dir="$1"
    shift
    trap "echo \$rc > \"$out_dir/driver.DONE\"" EXIT
    python3 -m tools.memory_battery.driver --pid-file "$out_dir/driver.pid" "$@"
    rc=$?
' _ "$out_dir" "$@" >"$out_dir/driver.out" 2>&1 &

echo "launched (out_dir=$out_dir); watch with: tools/memory_battery/watch_battery.sh $out_dir" >&2
