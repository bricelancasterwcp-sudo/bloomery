# Envelope-v3 (action-terminated lens) + declared kv_per_token override

Governing texts (read BOTH first; doc comments cite them):
- docs/superpowers/evidence/2026-08-15-g4-protocol.md §11 (Amendment 3 — envelope-v3, the law-3 ruling, the alias/conflict rules)
- docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md §10 (kv_per_token_bytes addendum)

## Part A — envelope enum + stop-at-`</action>`

1. **Substrate stop support.** `Substrate::infer` (crates/bloomery-substrate/src/contract.rs) gains a `stop: Option<&str>` parameter. LlamaSubstrate (llama.rs): in the token-generation loop, after appending each detokenized piece, if the accumulated completion contains the stop string, truncate the completion at the END of the first occurrence (tag INCLUDED) and stop generating; completion_tokens = tokens actually generated (honest count). Mind partial-UTF8/token boundaries: match on the accumulated String after each append. FakeSubstrate (fake.rs): record the stop per call (like load_n_gpu_layers) AND apply the same truncate-at-first-occurrence-inclusive to the scripted reply text, so GPU-free tests exercise real truncation semantics.
2. **Pager ripple.** `Pager::infer` gains `stop: Option<&str>` and passes it through. Callers: api_v1 chat + anything POST-related pass `None` (the /v1 surface is untouched — §11 binding); the task loop passes per envelope (below).
3. **Config: envelope enum.** Per-model `envelope = "v1" | "v2" | "v3"` (ModelSpec::Tuned, optional string validated at load — unknown value = named config error). The SHIPPED `think_preseed = true` key stays parseable as an ALIAS for v2; if BOTH keys are set and disagree (think_preseed=true with envelope="v1", or think_preseed=false with envelope="v2"/"v3") that is a named config error, never a silent pick. Accessor returns a proper enum: `pub enum EnvelopeLens { V1, V2, V3 }` (put it where ModelSpec lives; re-export as needed).
4. **TaskSpec + loop.** Replace `TaskSpec.think_preseed: bool` with `envelope: EnvelopeLens`. `render_prompt`: pre-seed (`THINK_PRESEED`) for V2 and V3. `propose_action`'s infer call: `stop = Some("</action>")` for V3, `None` otherwise — a named const `ACTION_STOP: &str = "</action>"` with a §11-citing doc comment.
5. **One-source lens naming.** `envelope_lens(...)` becomes `EnvelopeLens::lens_name(&self) -> &'static str` returning `bloomery-task-envelope-v{1,2,3}`. The probe's ProbeContext carries the enum (single locked read, as Amendment 2's review verified); verdict detail uses it. HTTP task creation flows the same policy value.
6. Update the pager policy accessors (`model_think_preseed` → `model_envelope`; `agent_task_policy` tuple member changes accordingly — chase every caller).

## Part B — declared kv_per_token override (spec §10)

7. ModelSpec::Tuned gains `kv_per_token_bytes: Option<u64>`; flows via `set_model_tuning` (or its sibling — follow the existing style) into the model entry.
8. One-source helper `effective_kv_per_token()` (declared if present else GGUF-derived) used at EVERY kv charge site: `create_agent`'s `GeometryInput.kv_per_token` AND the `kv_bytes`/reservation computation (find them all — grep `kv_per_token` in the daemon crate; the /status `ModelStatus.kv_per_token` reports the effective value and ModelStatus gains `kv_per_token_declared: bool` so a declared number never reads as measured).
9. NO clamp against the GGUF value (declared smaller is the point; larger is allowed). Doc comments state the OOM direction (declared too small) and cite spec §10.

## Tests (TDD, failing first)
- Config: envelope parses for all three values; unknown value = named error mentioning the valid set; think_preseed=true alias → V2; conflict combos = named errors; kv_per_token_bytes parses; absent = None; all defaults = byte-for-byte v1 behavior for existing configs.
- Fake truncation: scripted reply containing TWO action blocks + trailing prose, infer with stop `</action>` → returned text ends exactly at the first `</action>` (ends_with + no second block).
- Loop: under V3, a scripted two-action reply parses as ONE clean action (the MultipleActions ramble is structurally gone); under V2 the same script still yields MultipleActions (the stop is v3-only); /v1-path infer records stop=None (assert via the fake's recording, wherever an existing api_v1 test can be extended minimally).
- Probe: verdict detail says `bloomery-task-envelope-v3` for a v3-configured model (extend the existing v1/v2 naming tests' pattern).
- KV override: asymmetric both-places test (a window that only comes out right if geometry uses the declared value AND a reservation that only fits if kv_bytes uses it — the weights-override test in pager_weights_test.rs is the template); /status shows effective value + declared flag true; absent override = GGUF value + flag false.
- Mutation checks (perform + record evidence): (1) drop the stop pass-through in propose_action → the one-clean-action test fails; (2) point ONE kv charge site back at the raw GGUF value → the asymmetric test fails. Restore both.

## Constraints
fmt + clippy `-D warnings` both feature sets; `cargo test --workspace`; `cargo build --features llama`; NEVER wrap commands in the `timeout` binary; files ≤800 lines (pager.rs is AT 800 — additions go in pager/ submodules); conventional commit `feat: envelope-v3 action-terminated lens + declared kv_per_token override (amendment 3 + spec §10)`; no attribution footers; do NOT commit report/brief files.
