# Operator-configurable assay probe timeout

Branch: `feat/probe-timeout-config` (from `master`)

## What was done

Made the boot-POST assay probe's wall-clock cap operator-configurable via
`assay.probe_timeout_secs` in the daemon TOML config, defaulting to 600s
(today's hardcoded behavior, preserved byte-for-byte).

1. **`crates/bloomery-daemon/src/config.rs`**
   - Added `fn default_probe_timeout_secs() -> u64 { 600 }`.
   - Added `AssayConfig::probe_timeout_secs: u64` with
     `#[serde(default = "default_probe_timeout_secs")]` and a doc comment
     covering: what it bounds (the `post::PostRunner` subprocess cap), why
     it exists (citing `post.rs`'s "a wedged assay must not hold the
     provisional-admission window open forever"), the 600s/~110s/5x-headroom
     derivation, and the measured motivation for raising it (qwen3.8-27b Q3
     at ~15.5 tok/s, ~3.4x slower than the baseline model, projecting a
     `--quick` probe at ~25-30 min).

2. **`crates/bloomery-daemon/src/post.rs`**
   - `PostRunner::new` now takes `pub fn new(python: String, probe_timeout:
     Duration) -> PostRunner` and closes over the passed-in `probe_timeout`
     in the `run` closure (via `move ||`) instead of the module constant.
   - Removed the `PROBE_TIMEOUT` const (its 600s value now lives solely as
     `config::default_probe_timeout_secs`, per the brief's "replace it with
     a doc note" option) and folded its full reasoning (the ~110s measured
     baseline, 5x headroom, kill-on-expiry named-failure path) into
     `PostRunner::new`'s doc comment, plus the new configurability
     motivation (the qwen3.8-27b Q3 measurement).
   - Updated the module-level doc comment ("The subprocess is bounded")
     and the two remaining doc references in `run_bounded` and
     `run_bounded_for_test` that used to point at `[`PROBE_TIMEOUT`]`, so
     none of them dangle after the const's removal.
   - `with_runner` (test constructor) is unchanged — it still injects a
     `CommandRunner` directly and never touches the timeout.

3. **`crates/bloomery-daemon/src/main.rs`**
   - The `run` fn's boot sequence now extracts `probe_timeout =
     Duration::from_secs(config.assay.probe_timeout_secs)` before the
     `move` closure (same pattern as `python`, `tier`, `tasks_enabled`,
     etc.) and calls `PostRunner::new(python, probe_timeout)`.

## TDD evidence

Tests were written first and confirmed to fail (compile errors — the field
and the second constructor argument did not exist yet) before any source
change:

```
error[E0061]: this function takes 1 argument but 2 arguments were supplied
   --> crates/bloomery-daemon/tests/post_test.rs:708:18
    |
708 |     let runner = PostRunner::new(
    |                  ^^^^^^^^^^^^^^^

error[E0609]: no field `probe_timeout_secs` on type `AssayConfig`
  --> crates/bloomery-daemon/tests/config_test.rs:56:29
```

After implementing the source changes, both targeted test files pass (RED
-> GREEN):

```
$ cargo test -p bloomery-daemon --test config_test --test post_test
running 11 tests   (config_test) ... test result: ok. 11 passed
running 25 tests   (post_test)   ... test result: ok. 25 passed
```

New tests added:

- `crates/bloomery-daemon/tests/config_test.rs`
  - `minimal_toml_fills_defaults` extended with
    `assert_eq!(config.assay.probe_timeout_secs, 600)` (default when key
    absent).
  - `explicit_probe_timeout_secs_parses` (new test): a config with
    `assay = { ..., probe_timeout_secs = 1800 }` parses to `1800`.

- `crates/bloomery-daemon/tests/post_test.rs`
  - `post_runner_new_honors_its_configured_probe_timeout` (new test):
    follows the existing `run_bounded_for_test` real-subprocess pattern
    (`a_wedged_probe_is_killed_and_named_a_timeout`), but drives it through
    the *public* `PostRunner::new` + `.probe()` surface instead of the
    internal seam. Writes a throwaway `/bin/sh` script that ignores every
    argument and sleeps 1s before writing a marker file, constructs
    `PostRunner::new(script_path, Duration::from_millis(300))`, and asserts
    the probe fails with `PostError::Spawn` containing "timed out" well
    under 10s, and that the marker file never gets written (the child was
    actually killed, not merely abandoned). This is the load-bearing
    plumbing test: if `new` ignored its `probe_timeout` argument (e.g.
    fell back to the old hardcoded 600s), the 1s child would run to
    completion, the probe would instead fail with `PostError::BadProfile`
    (missing `--json` document) rather than `PostError::Spawn` naming a
    timeout, and the assertion on the error variant would fail.

## Commands run and output (summarized)

```
$ cargo fmt --all
(no diff beyond the edits already made)

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s   (clean)

$ cargo clippy --workspace --all-targets --features llama -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.90s   (clean;
llama-cpp-2/llama-cpp-sys-2 were already built in target/debug/build from a
prior session, so this exercised the real llama-gated `run` fn in main.rs,
not a skipped feature)

$ cargo test --workspace
All test binaries: test result: ok. (0 failed across every crate/binary,
including config_test: 11 passed, post_test: 25 passed)

$ cargo build --features llama
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s   (clean,
includes bloomery-daemon's llama-gated main.rs binary)
```

No `timeout` binary was used anywhere (per the box gotcha: uutils `timeout`
segfaults on this host).

## Commit

One conventional commit on `feat/probe-timeout-config`:

`feat: operator-configurable assay probe timeout (assay.probe_timeout_secs, default 600)`

(hash recorded after commit — see final status message)
