#!/usr/bin/env bash
#
# tools/memory_battery/watch_battery.sh -- polls a run_battery.sh-launched
# driver run and reports its state, distinguishing three outcomes a naive
# "is anything printing" check cannot tell apart (design spec §5; task-3
# brief):
#
#   1. DONE      -- driver.DONE exists and holds a real exit code.
#   2. RUNNING   -- driver.pid names a live process, no DONE marker yet.
#   3. DIED WITHOUT MARKER -- driver.pid named a process that is now gone,
#                   and driver.DONE never appeared: the process died a way
#                   no trap could observe (SIGKILL, OOM-kill, host reset).
#
# Plus a transient "STARTING" state before driver.pid even exists yet (the
# window between run_battery.sh backgrounding the job and the Python
# process reaching its own `--pid-file` write). Every poll prints a
# labeled line -- silence is never the report, and neither state 2 nor
# state 3 is ever mistaken for state 1.
#
# Usage: watch_battery.sh <out_dir> [poll_interval_s]
# Exit code: the driver's own exit code once DONE; 2 on a died-without-
# marker detection; runs until one of those, i.e. never exits on its own
# while still RUNNING/STARTING.

set -uo pipefail

out_dir="${1:?usage: watch_battery.sh <out_dir> [poll_interval_s]}"
interval="${2:-5}"

pid_file="$out_dir/driver.pid"
done_file="$out_dir/driver.DONE"

while true; do
    if [ -f "$done_file" ]; then
        rc="$(cat "$done_file" 2>/dev/null)"
        if [[ "$rc" =~ ^[0-9]+$ ]]; then
            echo "DONE exit_code=$rc"
            exit "$rc"
        fi
        echo "DONE marker present but unreadable (\"$rc\") -- treat as failure, do not assume success"
        exit 1
    fi

    if [ ! -f "$pid_file" ]; then
        echo "STARTING (no pid file yet)"
        sleep "$interval"
        continue
    fi

    pid="$(cat "$pid_file" 2>/dev/null)"
    if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
        echo "DIED WITHOUT MARKER (pid ${pid:-<empty>} gone, no driver.DONE) -- treat as failure, do not assume success"
        exit 2
    fi

    echo "RUNNING (pid $pid)"
    sleep "$interval"
done
