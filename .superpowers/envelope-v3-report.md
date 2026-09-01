# Envelope-v3 (action-terminated lens) + declared kv_per_token override — Report

**Branch:** `feat/envelope-v3-kv-override`
**Commit:** `ee76fe71967c9f14153e19a0cfd1e56eadc06103`
**Status:** DONE

Governing texts read in full before implementation:
- `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10 (Amendment 2, already shipped) and §11 (Amendment 3, this work)
- `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md` §10 addendum (declared kv_per_token override)

Starting state: envelope-v2 (think-preseed) was already fully shipped (config `think_preseed`, `Pager::set_think_preseed`/`model_think_preseed`, `TaskSpec::think_preseed`, `codec_probe::ENVELOPE_LENS_V2`). This work extended that machinery to a proper three-value `EnvelopeLens` enum, added the v3 action-terminated stop, and added the Part B declared-KV-per-token override.

---

## Part A — envelope enum + stop-at-`</action>`

### 1. Substrate stop support

- `crates/bloomery-substrate/src/lib.rs:71-107` — `Substrate::infer` gains `stop: Option<&str>`, with a doc comment stating the law-3 ruling (termination, not constraint).
- `crates/bloomery-substrate/src/llama.rs:232-260` (trait impl) and `:360-556` (`generate`/`generate_from`) — threaded `stop` through the real generation loop. After each generated token is fed back into the KV cache and decoded, the loop does `std::str::from_utf8(&bytes)` (**strict**, not `_lossy`) on the accumulated completion bytes; `Err` (a token that leaves the buffer mid-multibyte-character) is read as "wait for the next token's continuation bytes," never as "no match," and once the buffer parses as valid UTF-8, `text_so_far.find(stop)` gives an exact byte offset into `bytes` itself (no `U+FFFD` substitution to make earlier-in-the-buffer offsets lie). On a hit, `bytes.truncate(idx + stop.len())` (tag **included**) and the loop `break`s. `completion_tokens` is incremented before the check, so it counts every token actually sampled — never fudged down to match the shorter returned text.
  - **Documented boundary** (llama.rs:516-541): the KV cache has already absorbed the *full* token (feed-back happens before the stop check), so if a token's bytes carry content past the stop tag, the model's own context "saw" that trailing content even though the caller's returned text doesn't include it. This is the same behavior every token-based stop-sequence implementation has (OpenAI included) and is not reachable without a token straddling the `</action>` boundary in the shipped `codec-tasks-v1` fixtures — noted as protocol §11's own recorded limit, not silently hidden.
  - This code path requires a GPU and is not exercised by `cargo test`; it is covered by `cargo build`/`cargo clippy --features llama` only. **Live-run validation is still owed** — see Concerns.
- `crates/bloomery-substrate/src/fake.rs:23-38, 64-70, 148-181` — `FakeSubstrate` records every `stop` value passed to `infer` (`infer_stops()`, index-aligned with `calls()`'s `"infer:cN"` entries, mirroring `load_n_gpu_layers()`), and applies the identical truncate-at-first-occurrence-inclusive semantics to the scripted reply's `text` before returning it — so GPU-free tests exercise real truncation behavior, not a hand-pre-truncated fixture.
- `crates/bloomery-daemon/src/llama_send.rs:100-108` — `SendLlama`'s passthrough impl updated.

### 2. Pager ripple

- `crates/bloomery-daemon/src/pager.rs:642-702` — `Pager::infer` gains `stop: Option<&str>`, passed straight to `self.substrate.infer(ctx, prompt, max_tokens, stop)`.
- `crates/bloomery-daemon/src/api_v1.rs:273` — `/v1/chat/completions` passes `None` explicitly, with a comment citing §11 ("the `/v1` chat surface is untouched").
- `crates/bloomery-daemon/src/api_native.rs:137` — native `/agents/{id}/infer` passes `None` (POST-related, per brief).
- `crates/bloomery-daemon/src/task/task_loop.rs:283-289` — task loop passes per-envelope: `spec.envelope.action_stop().then_some(ACTION_STOP)`.
- All other direct `.infer(...)` call sites in the workspace (test-only `Pager`/`Substrate` callers across `pager_test.rs`, `pager_reservation_test.rs`, `pager_weights_test.rs`, `pager_timeshare_test.rs`, `pager_remove_agent_test.rs`, `fake_test.rs`, `llama_semantic_test.rs`, `llama_live_test.rs`) updated mechanically (paren-balanced Python rewrite, verified by hand on the nested-paren cases) to pass `None`. Custom test-local `Substrate` impls (`api_native_test.rs`, `api_task_test.rs`, `task/registry.rs`'s `PanicSubstrate`, `pager_test.rs`'s `ScriptedSubstrate`) gained the `stop: Option<&str>` parameter.

### 3. Config: envelope enum

- `crates/bloomery-daemon/src/config.rs:18-79` — `pub enum EnvelopeLens { V1, V2, V3 }` with `Default = V1`, `const fn lens_name(&self) -> &'static str` (single source for the pinned `bloomery-task-envelope-v{1,2,3}` strings), `const fn think_preseed(&self) -> bool` (`V2`/`V3`), `const fn action_stop(&self) -> bool` (`V3` only), and a private `parse(raw: &str)` that names the valid set on an unknown value.
- `ModelSpec::Tuned` gains `envelope: Option<String>` (raw, unvalidated at parse) and keeps `think_preseed` — **but retyped `bool` → `Option<bool>`** (`config.rs:105-131`). This was a real bug caught by TDD: a plain `bool` with `#[serde(default)]` cannot distinguish "operator never wrote `think_preseed`" from "operator wrote `think_preseed = false`," and the conflict rule only fires on the latter. `Option<bool>` fixes it; `ModelSpec::think_preseed()`'s public bool-returning contract is unchanged (`.unwrap_or(false)`).
- `ModelSpec::envelope_lens(&self) -> Result<EnvelopeLens, String>` (`config.rs:191-238`) is the one place that resolves both keys together:
  - absent + absent/false → `V1`; absent + `Some(true)` → `V2` (Amendment 2 alias, unchanged); explicit `envelope` wins when set and doesn't conflict.
  - Named errors for the three disagreeing pairs Amendment 3 states verbatim: `think_preseed=true` with `envelope="v1"`; `think_preseed=false` with `envelope="v2"`; `think_preseed=false` with `envelope="v3"`. (`envelope="v3"` + `think_preseed=true` is **not** a conflict — v3 implies the pre-seed v2 already requires.)
  - Unknown `envelope` string → named error listing `"v1", "v2", "v3"`.
- `load_config` (`config.rs:369-382`) calls `spec.envelope_lens()` for every model before returning `Ok`, so a bad config fails at load time, not first use — matches the brief's "validated at load."

### 4. TaskSpec + loop

- `crates/bloomery-daemon/src/task/task_loop.rs:48-77` — `TaskSpec.think_preseed: bool` replaced with `TaskSpec.envelope: EnvelopeLens`.
- `render_prompt` (`task_loop.rs:210-236`) — pre-seeds for `spec.envelope.think_preseed()` (`V2`/`V3`), byte-identical to before for `V1`.
- `crates/bloomery-daemon/src/task/task_loop.rs:210-221` — `const ACTION_STOP: &str = "</action>";` with a §11-citing doc comment (law-3 ruling, restated).
- `propose_action` (`task_loop.rs:283-289`) — `let stop = spec.envelope.action_stop().then_some(ACTION_STOP);` computed once per step (not per re-ask attempt, since `spec.envelope` never changes), passed to `pager.infer(agent_id, &prompt, STEP_MAX_TOKENS, stop)`.

### 5. One-source lens naming

- `EnvelopeLens::lens_name(&self) -> &'static str` (`config.rs:46-52`) is now the one source. `codec_probe::mod.rs`'s `ENVELOPE_LENS`/`ENVELOPE_LENS_V2`/`ENVELOPE_LENS_V3` pinned constants (`codec_probe/mod.rs:110-127`) are now `const` derived from it (`EnvelopeLens::V1.lens_name()` etc.) rather than retyped literals. The private `envelope_lens(bool) -> &'static str` helper is gone; `ProbeContext.think_preseed: bool` became `ProbeContext.envelope: EnvelopeLens` (`codec_probe/mod.rs:168-177`), read once via `Pager::model_envelope` at invariant 1 (same single-locked-read Amendment 2 established), and the verdict `detail` now calls `ctx.envelope.lens_name()` directly.
- `Pager::agent_task_policy` (`pager/codec_gate.rs:126-150`) — tuple changed from `(PatchCodec, bool, bool)` to `(PatchCodec, bool, EnvelopeLens)`; `api_task.rs::create_task` (`api_task.rs:157-216`) flows the third field into `TaskSpec.envelope` verbatim — the HTTP task-creation path.

### 6. Pager policy accessor rename

- `pager/tuning.rs:105-131` — `model_think_preseed`/`set_think_preseed` renamed to `model_envelope`/`set_model_envelope`, operating on `EnvelopeLens` instead of `bool`. `model_envelope` fail-closes to `EnvelopeLens::V1` for an unknown model (matches `model_patch_codec`/`model_mutating_verbs`'s fail-closed pattern). Every caller chased: `pager.rs` (`ModelEntry.envelope` field + `register_model` init), `codec_gate.rs` (`agent_task_policy`), `codec_probe/mod.rs` (`run_codec_probe`'s invariant-1 read), `main.rs` (wiring), and every test that called the old names (`pager_codec_gate_test.rs`, `codec_probe_test.rs`, `api_task_test.rs`).

---

## Part B — declared kv_per_token override (spec §10 addendum)

### 7. `ModelSpec::Tuned.kv_per_token_bytes`

- `config.rs:134-160` — `kv_per_token_bytes: Option<u64>` (raw bytes, no MiB conversion — matches the spec's `kv_per_token_bytes = N (bytes)` literally). Accessor `ModelSpec::kv_per_token_bytes()` (`config.rs:270-278`).
- `main.rs:187-198` — wired via `Pager::set_kv_per_token_bytes(name, spec.kv_per_token_bytes())`, alongside the existing `set_model_tuning`/`set_model_envelope` calls, no unit conversion (unlike `weights_vram_mib`).
- Followed the existing **sibling-setter** style (`pager::tuning.rs:132-151`) rather than a third positional arg on `set_model_tuning` — same arity-blowup reasoning the module doc already states for `set_think_preseed`/`set_model_envelope` (8 existing call sites in `pager_weights_test.rs` pass exactly two tuning args).

### 8. `effective_kv_per_token()` at every charge site

- `pager/tuning.rs:52-70` — `ModelEntry::effective_kv_per_token(&self) -> u64`: declared override when present, else the GGUF-derived `kv_per_token` field. **Grepped the whole daemon crate for `kv_per_token`** (brief's instruction) and found exactly three consuming sites, all now routed through this accessor:
  1. `pager.rs:512` (`create_agent`'s `GeometryInput.kv_per_token`) — `entry.effective_kv_per_token()`.
  2. `pager.rs:551-553` + `pager/tuning.rs:153-170` (`Pager::kv_reservation_bytes`, called from `create_agent` for `Agent.kv_bytes`) — a **second, independent** call to `effective_kv_per_token()`, deliberately not reusing the local bound for (1), so a one-sided wiring bug at either site is separately testable (mirrors `effective_weights_bytes()`'s four independent readers).
  3. `pager/status.rs:94-104, 187-190` — `ModelStatus.kv_per_token: m.effective_kv_per_token()`, plus a new `ModelStatus.kv_per_token_declared: bool = m.kv_per_token_bytes.is_some()` so a declared number never reads as measured.
- `pager.rs` was **exactly at the 800-line house cap** before this change (confirmed via `git show HEAD~1:crates/bloomery-daemon/src/pager.rs | wc -l` → 800). Moving `kv_reservation_bytes` into `pager/tuning.rs` (rather than inlining the second independent read in `pager.rs`) plus tightening several doc comments brought the file back to exactly 800 lines after all Part A + Part B additions. `pager/tuning.rs` grew to 184 lines (well under cap).

### 9. No clamp; OOM-direction doc comments

- `pager/tuning.rs:52-70` and `config.rs:141-159` both state explicitly: **no clamp** against the GGUF-derived value (unlike `effective_weights_bytes()`'s `min(declared, file)`), a larger declared value is allowed (extra conservative), and a smaller declared value is the point — with the OOM direction named ("declared too small is the OOM direction — the window law would grant tokens whose real KV exceeds VRAM") and spec §10's naming rule restated ("a declared number must never read as a measured one").

---

## TDD evidence (all failing-first where the brief specified)

Every test below was written, run, and confirmed failing before the corresponding implementation existed (or — for the `EnvelopeLens`/`kv_per_token_bytes` config fields and pager accessors, which required simultaneous struct+accessor+test changes to even compile — written immediately after the minimal enum/field scaffold and run red against the *unimplemented validation logic* before the logic was added). All now pass.

### Config (`crates/bloomery-daemon/tests/config_test.rs`, 20 tests, +9 new)
- `envelope_parses_for_all_three_values` — all three values parse to the matching `EnvelopeLens`.
- `unknown_envelope_value_is_a_named_error_mentioning_the_valid_set` — names the bad value AND lists `v1`/`v2`/`v3`.
- `think_preseed_true_alone_aliases_to_v2`.
- `absent_envelope_and_think_preseed_resolves_to_v1` (both shapes: table with omitted keys, bare-path).
- `conflicting_envelope_and_think_preseed_combos_are_named_errors` — the exact 3 disagreeing pairs, error names both keys.
- `consistent_envelope_and_think_preseed_combos_parse` — `v3`+`true` is NOT a conflict.
- `envelope_lens_names_are_pinned`.
- `kv_per_token_bytes_parses_and_defaults_to_none` (present, absent-table, bare-path).
- This is also where the `think_preseed: bool → Option<bool>` bug was caught: `envelope_parses_for_all_three_values` failed red with `envelope "v2": model qwen3.8:27b: envelope = "v2" conflicts with think_preseed = false` before the field-type fix.

### Fake truncation (`crates/bloomery-substrate/tests/fake_test.rs`, 6 tests, +3 new)
- `infer_with_a_stop_sequence_truncates_at_the_first_inclusive_occurrence` — scripted reply with TWO `<action>` blocks + trailing prose; `stop=Some("</action>")` → `ends_with("</action>")`, exact-equality to the first block only, `!contains("second")`.
- `infer_with_no_stop_sequence_leaves_the_reply_untouched`.
- `infer_stops_are_recorded_per_call_in_order`.

### Loop (`crates/bloomery-daemon/tests/task_loop_test.rs`, 16 tests, +4 new)
- `under_v3_a_two_action_scripted_reply_parses_as_one_clean_action` — the literal Q3-27B ramble shape (`ramble()` helper), envelope V3, one scripted reply, `run_task` → `TaskStatus::Done`, exactly 1 step, `summary == "first"` (the surviving, truncated action).
- `under_v2_the_same_script_still_yields_multiple_actions` — same script, envelope V2, first recorded step's outcome contains literal `"MultipleActions"` (the stop is v3-only).
- `under_v3_the_infer_call_carries_the_action_stop_sequence` / `under_v1_and_v2_the_infer_call_never_carries_a_stop_sequence` — direct `FakeSubstrate::infer_stops()` assertions on the wiring.

### /v1 (`crates/bloomery-daemon/tests/api_v1_test.rs`, 15 tests, +1 new)
- `v1_chat_completions_always_infers_with_no_stop_sequence` — extends the existing `fake_pager_for_v1`/`dispatch_v1_fake` no-socket pattern, asserts `infer_stops() == [None]`.

### Probe (`crates/bloomery-daemon/tests/codec_probe_test.rs`, 22 tests, +1 new, +1 extended)
- `a_v3_configured_model_probe_journals_the_v3_lens_in_the_verdict_detail` — same happy-path fixture as the v1/v2 naming tests, `set_model_envelope(MODEL, EnvelopeLens::V3)`, verdict detail contains `ENVELOPE_LENS_V3` and neither v1 nor v2.
- `instrument_parameters_match_the_pre_registered_protocol` extended with the `ENVELOPE_LENS_V3` pin.

### KV override (`crates/bloomery-daemon/tests/pager_weights_test.rs`, 21 tests, +6 new)
- `create_agent_window_uses_the_declared_kv_per_token_not_the_gguf_value` — asymmetric numbers (declared 14 336 B/token vs GGUF-derived 57 344 B/token; free VRAM 100 048 576 B) chosen so the window's bound (`training_ctx` vs `vram`) AND exact token count diverge sharply: declared → `training_ctx`-bound at 4096; GGUF-derived → `vram`-bound at 1743. No placement call in this test — sensitive to exactly `GeometryInput.kv_per_token`.
- `placement_uses_the_declared_kv_per_token_not_the_gguf_value` — a fixed `window_cap=2048` (pinned deterministic regardless of the reservation-site bug under test, because the window-law site is separately verified above and correct here) plus a 40 MiB budget chosen strictly between declared-kv demand (~29.0 MiB, fits) and GGUF-derived-kv demand (~113.0 MiB, would refuse). Sensitive to exactly the `Agent.kv_bytes`/reservation charge site.
- `status_reports_declared_kv_per_token_and_the_declared_flag_when_override_active` / `status_reports_gguf_kv_per_token_and_no_declared_flag_when_absent` — the third charge site.
- `a_declared_kv_per_token_larger_than_gguf_is_not_clamped` — no-clamp property, direct contrast with the weights precedent's clamp.
- `set_kv_per_token_bytes_on_unknown_model_is_refused`.

**Full-workspace test count:** 50 `test result: ok` blocks, 0 failures, 413+ individual assertions passing (`cargo test --workspace`).

---

## Mutation checks (both performed, restored)

### (1) Drop the stop pass-through in `propose_action`

Mutated `task/task_loop.rs`'s `propose_action` to hardcode `let stop = None;` instead of `spec.envelope.action_stop().then_some(ACTION_STOP)`.

```
$ cargo test -p bloomery-daemon --test task_loop_test
...
test under_v3_a_two_action_scripted_reply_parses_as_one_clean_action ... FAILED
test under_v3_the_infer_call_carries_the_action_stop_sequence ... FAILED
...
thread 'under_v3_a_two_action_scripted_reply_parses_as_one_clean_action' panicked:
assertion `left == right` failed: the truncated turn must parse as one clean Done action:
[TaskStepRecord { step: 1, verb: "?", outcome: "MultipleActions { found: 2 }", ... }]
  left: Error
 right: Done

thread 'under_v3_the_infer_call_carries_the_action_stop_sequence' panicked:
assertion `left == right` failed: a v3 turn's infer call must carry the action stop sequence
  left: [None]
 right: [Some("</action>")]

test result: FAILED. 14 passed; 2 failed
```

Restored (`let stop = spec.envelope.action_stop().then_some(ACTION_STOP);`); re-ran: `test result: ok. 16 passed; 0 failed`.

### (2) Point one kv charge site back at the raw GGUF value

Two independent sub-checks, each restored before the next:

**(2a) Reservation site** — `pager/tuning.rs::kv_reservation_bytes` mutated from `.effective_kv_per_token()` to `.kv_per_token` (raw):

```
$ cargo test -p bloomery-daemon --test pager_weights_test
...
test placement_uses_the_declared_kv_per_token_not_the_gguf_value ... FAILED
thread panicked: ... : Refused { needed: 118489088, free: 41943040, reclaimable: 0 }
test result: FAILED. 20 passed; 1 failed
```
(`create_agent_window_uses_the_declared_kv_per_token_not_the_gguf_value` still passed — confirming the mutation is one-sided, exactly as designed.) Restored; re-ran: `21 passed; 0 failed`.

**(2b) Window-law site** — `pager.rs::create_agent`'s `entry.effective_kv_per_token()` (the `GeometryInput.kv_per_token` read) mutated to `entry.kv_per_token` (raw):

```
$ cargo test -p bloomery-daemon --test pager_weights_test
...
test create_agent_window_uses_the_declared_kv_per_token_not_the_gguf_value ... FAILED
  left: 1743
 right: 4096
test placement_uses_the_declared_kv_per_token_not_the_gguf_value ... FAILED
  left: 713
 right: 2048
test result: FAILED. 19 passed; 2 failed
```
(The placement test also fails here because it depends on the window computed at this same site landing on `user_cap`=2048 — expected collateral, not a design flaw.) Restored; re-ran: `21 passed; 0 failed`.

`grep -rn "MUTATION-CHECK" crates/` confirms no mutation markers were left in the committed tree.

---

## Commands + final output

```
$ cargo fmt --all -- --check
(clean)

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.58s

$ cargo clippy --workspace --all-targets --features llama -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.59s

$ cargo test --workspace
50 "test result: ok" blocks; 0 FAILED; 0 error lines.

$ cargo build --features llama
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.38s

$ wc -l crates/bloomery-daemon/src/pager.rs
800 crates/bloomery-daemon/src/pager.rs
```

Commit: `ee76fe71967c9f14153e19a0cfd1e56eadc06103` — `feat: envelope-v3 action-terminated lens + declared kv_per_token override (amendment 3 + spec §10)` (31 files changed, 1375 insertions, 237 deletions). No attribution footer, per constraints. Brief/report files (`.superpowers/envelope-v2-brief.md`, `.superpowers/envelope-v2-report.md`, `.superpowers/envelope-v3-brief.md`) left untracked, not committed. This report itself is not committed either.

---

## Concerns / what's not GPU-validated

- **`llama.rs`'s stop loop cannot be exercised by `cargo test`** (no GPU in this environment). It compiles clean under `--features llama` (`cargo build`/`cargo clippy`), and its logic is a direct structural mirror of `FakeSubstrate`'s GPU-free-tested truncation (same "find, truncate-inclusive, break" shape), but the **live run is the only thing that validates**: (a) that `str::from_utf8` on a real multi-byte-token stream actually behaves as documented at the boundary, and (b) the KV-cache-absorbs-the-full-token boundary behavior described in the doc comment. Recommend a live smoke test (`BLOOMERY_LIVE=1`, `--features llama`) exercising a `</action>`-stop turn before this ships to a production gate run, per the brief's own instruction to "note in your report that the live run validates it."
- The two "MultipleActions ramble" task-loop tests intentionally use a single scripted reply (not 3), so the V2 counterpart test observes the re-ask's *first* recorded outcome rather than driving the full 3-attempt exhaustion to `TaskStatus::Error` — this was a deliberate simplification to keep the test direct and avoid scripting 3 identical replies; the underlying mechanism (raw untruncated text failing `parse_action_with_codec` as `MultipleActions`) is what's actually under test and is pinned exactly.
- `pager.rs` is back at exactly 800 lines (the house cap) after this work — any *further* additions to that file will need another submodule extraction; there is no headroom left.

---

## Fix round 1 (review finding, addressed)

**Fix commit:** `0d6ae6b` — `fix: stop-scan survives non-UTF8 accumulation (scan valid prefix; removes detection lag)`

**Finding (Important):** `llama.rs`'s stop check ran `std::str::from_utf8(&bytes)` on the ENTIRE accumulated completion buffer every iteration. The doc comment claimed the `Err` branch only ever meant "an incomplete trailing sequence, wait for the next token" — but `str::from_utf8` returns `Err` for a *genuinely invalid* sequence anywhere in the buffer too (`Utf8Error::error_len() == Some(_)`, e.g. a byte-fallback token emitting a lone continuation byte with no leading byte). Since `bytes` only grows and is never repaired, once such a byte landed anywhere in the stream, `from_utf8(&bytes)` returned `Err` for the rest of the turn — the stop check silently stopped running entirely, and a v3-labeled turn would degrade to v2 behavior (no truncation) with zero signal that anything had gone wrong.

### Fix

- **`crates/bloomery-substrate/src/stop_scan.rs`** (new file, 157 lines) — extracted the pure scan-and-truncate decision into `pub(crate) fn stop_hit(bytes: &[u8], stop: &str) -> Option<usize>`. On `Err(e)` from `std::str::from_utf8(bytes)`, it now re-decodes `&bytes[..e.valid_up_to()]` (guaranteed valid, `.expect()`-safe by construction) instead of giving up, and scans *that* prefix for `stop`. This handles both `Err` cases honestly, in one code path:
  - **Trailing-incomplete** (`error_len() == None`): the valid prefix covers everything except the still-in-flight multi-byte tail, so a stop tag already present earlier in the buffer is found in the SAME call — this also removes the one-token detection lag the old whole-buffer-only check had (previously: skip the whole iteration, wait for the tail to resolve, check again next token).
  - **Genuinely invalid** (`error_len() == Some(_)`): the valid prefix freezes at the first bad byte's position for the rest of the turn (that position never moves once written), so a stop tag AFTER the invalid byte is not reachable this way and is documented as such — but a tag BEFORE the invalid byte is still found, and — the actual regression fix — the check is never silently disabled for the rest of generation the way the old whole-buffer `.ok()` skip was.
- **`crates/bloomery-substrate/src/llama.rs`** — the generation loop now calls `crate::stop_scan::stop_hit(&bytes, stop)` instead of the inline `from_utf8(&bytes).ok()` check; the surrounding doc comment was rewritten to describe both `Err` cases honestly (no more absolute "an Err always means incomplete trailing" claim) and points to `stop_hit`'s own doc for the full reasoning.
- **`crates/bloomery-substrate/src/lib.rs`** — added `pub(crate) mod stop_scan;`, deliberately **not** `#[cfg(feature = "llama")]`-gated (unlike `llama` itself), specifically so `stop_hit` compiles and is unit-testable under the default, GPU-free `cargo test --workspace` even though its only production caller only exists under `--features llama`. `stop_hit` is `#[cfg_attr(not(feature = "llama"), allow(dead_code))]`'d for exactly that reason (no non-test caller exists in a plain build) — the allow does **not** apply under `--features llama`, where the real caller keeps the dead-code lint fully live.

### GPU-free test evidence (`crates/bloomery-substrate/src/stop_scan.rs`, `#[cfg(test)] mod tests`, 6 tests — run via `cargo test -p bloomery-substrate --lib`, no GPU, no `llama` feature required)

| Test | Case | Result |
|---|---|---|
| `finds_the_stop_in_clean_valid_utf8` | (a) clean UTF-8 containing the stop | ok |
| `no_match_in_clean_valid_utf8_is_none` | clean UTF-8, no stop present | ok |
| `finds_the_stop_before_an_incomplete_trailing_multibyte_sequence` | (c) incomplete trailing multibyte (`0xE2 0x9C`, missing 3rd byte), stop tag earlier in the buffer — the lag-removal case | ok |
| `a_genuinely_invalid_byte_before_the_stop_makes_it_unreachable` | (b) invalid byte (lone `0x80`) BEFORE the stop — documents the honest limitation (`None`, not a panic, not a false match) | ok |
| `a_genuinely_invalid_byte_after_the_stop_does_not_block_it` | invalid byte (`0x80`) AFTER the stop — **the exact regression this fix closes**: before the fix, this would have returned `None` forever for the rest of the turn; now returns `Some` correctly | ok |
| `empty_bytes_is_none` | degenerate empty buffer | ok |

```
$ cargo test -p bloomery-substrate --lib
running 6 tests
test stop_scan::tests::a_genuinely_invalid_byte_after_the_stop_does_not_block_it ... ok
test stop_scan::tests::a_genuinely_invalid_byte_before_the_stop_makes_it_unreachable ... ok
test stop_scan::tests::empty_bytes_is_none ... ok
test stop_scan::tests::finds_the_stop_before_an_incomplete_trailing_multibyte_sequence ... ok
test stop_scan::tests::no_match_in_clean_valid_utf8_is_none ... ok
test stop_scan::tests::finds_the_stop_in_clean_valid_utf8 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Verification commands + output

```
$ cargo fmt --all -- --check
(clean)

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.64s

$ cargo clippy --workspace --all-targets --features llama -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.68s

$ cargo test --workspace
50 "test result: ok" blocks; 0 FAILED; 0 error lines (bloomery-substrate's lib
unittest block went from "running 0 tests" to "running 6 tests", all ok — the
other 49 blocks unchanged from the prior report).

$ cargo build --features llama
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s

$ wc -l crates/bloomery-substrate/src/llama.rs crates/bloomery-substrate/src/stop_scan.rs
593 llama.rs
157 stop_scan.rs
```

Commit: `0d6ae6b` — `fix: stop-scan survives non-UTF8 accumulation (scan valid prefix; removes detection lag)` (3 files changed, 188 insertions, 28 deletions). No attribution footer. This report remains uncommitted, per constraints.

### Remaining concern, updated

The genuinely-invalid-byte-BEFORE-the-stop degradation (documented in `stop_hit`'s doc comment and pinned by `a_genuinely_invalid_byte_before_the_stop_makes_it_unreachable`) is a known, accepted limitation, not a further bug: if a byte-fallback token emits an unrecoverable invalid byte and the model's `</action>` tag would have appeared strictly after that byte in the stream, the stop check cannot see it and generation runs to `max_tokens` instead of stopping early (never a crash, never a false match — an honest miss). This was explicitly in scope for this fix round only to the extent of "scan the valid prefix instead of giving up entirely," which is what was implemented; a full fix (e.g. lossily substituting the invalid byte and continuing to scan past it) was not requested and was not added. The live-run validation note from the original report still stands — this whole code path is GPU-only and untested against a real llama.cpp byte stream.
