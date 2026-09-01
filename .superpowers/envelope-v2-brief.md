# Envelope-v2: the think-preseeded lens (protocol Amendment 2 code)

Implements docs/superpowers/evidence/2026-08-15-g4-protocol.md §10. Read §10 first — it governs; doc comments cite it.

**What changes:**
1. Config (`crates/bloomery-daemon/src/config.rs`): `ModelSpec::Tuned` gains `#[serde(default)] think_preseed: bool` (a bare-path entry = false). Accessor `pub fn think_preseed(&self) -> bool` (Path variant → false).
2. Pager plumbing: `set_model_tuning` gains the flag (or a sibling setter following its exact style — read `pager/tuning.rs` and choose the smaller diff); `agent_task_policy` grows to return it (check every caller — `api_task.rs` create_task and the codec probe read policy; keep one policy source).
3. Task loop (`crates/bloomery-daemon/src/task/task_loop.rs`): `TaskSpec` gains `pub think_preseed: bool`; `render_prompt` appends the LITERAL pre-seed `<think>\n\n</think>\n\n` at the very end of the rendered prompt when the flag is set (after the transcript; nothing after it). The literal must be a named const `THINK_PRESEED` with a doc comment citing §10 and the 2026-08-15 feasibility probe.
4. Codec probe (`crates/bloomery-daemon/src/codec_probe/mod.rs`): the fixture TaskSpecs carry the model's flag; the lens name in the CodecVerdict detail becomes envelope-dependent — `bloomery-task-envelope-v2` when preseeded, `bloomery-task-envelope-v1` otherwise. Replace the single ENVELOPE_LENS const usage with a small `fn envelope_lens(think_preseed: bool) -> &'static str` (keep both names as consts). Module docs updated citing §10.
5. HTTP task creation (`api_task.rs`): tasks created for a preseeded model render with the pre-seed too (flows via the policy tuple/struct — same one-source rule as patch_codec/mutating_verbs).

**Tests (TDD, failing first):**
- config: table entry with `think_preseed = true` parses; absent = false; bare path = false.
- task_loop (FakeSubstrate, existing harness): with `think_preseed: true` the prompt the substrate records ENDS WITH the literal `<think>\n\n</think>\n\n` (assert ends_with, not contains); with false the pre-seed is absent anywhere in the prompt.
- codec_probe: a probe run for a preseeded model journals a CodecVerdict whose detail contains `bloomery-task-envelope-v2`; a non-preseeded run still says v1 (extend an existing scripted test pair — don't duplicate whole scenarios, parametrize or add the minimal second assertions).
- Mutation check (record evidence): remove the `ends_with` append → the task_loop test fails; hardcode the lens fn to v1 → the v2 verdict test fails. Restore both.

**Constraints:** fmt + clippy `-D warnings` both feature sets; `cargo test --workspace` green; NEVER wrap commands in the `timeout` binary; files ≤800 lines (pager.rs is AT 800 — any pager addition goes in `pager/tuning.rs`); conventional commit `feat: envelope-v2 think-preseeded lens (G4 protocol amendment 2, §10)`, no attribution footers. Do NOT commit your report file.
