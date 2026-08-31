# OpenAI tools adapter — live acceptance (Task 6): **PARTIAL**

**Date:** 2026-08-31. Governs
`docs/superpowers/plans/2026-08-31-openai-tools-adapter.md` Task 6, the
human-gated step. Subject: `adapters/openai-tools/` at master `d511e24`.
Model: `qwen36-reap48-ours` **untrained** (Q4_K_M,
`/home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf`), served by a
scratch bloomery boot on port 8498 (`ctx_overhead_mib = 512`, no KV
override, `allow_unprofiled`, assay off, tasks off, fresh `data_dir`).
Adapter on port 8011. **Before this run no line of adapter code had ever
touched a real model.**

**Verdict: PARTIAL.** The adapter works end to end and the prefill-once
property is confirmed by measurement. It does **not** yet survive a real
hermes session, for one specific and newly-identified reason recorded in §4.

## 1. What worked, measured

**A correct tool call, first attempt.** Driven over HTTP with one tool
schema: `finish_reason: tool_calls`, parsed to `terminal` with
`{"command": "ls /tmp"}`. Turn 2 consumed the tool result and answered
correctly.

**The prefill-once property holds, and this is the headline number.**
Re-run with the **real 132-tool hermes schema** (145,650 chars, taken from
the largest captured request dump):

| | turn 1 | turn 2 |
|---|---|---|
| `prompt_tokens` | **40,003** | **21** |
| wall | 11.1 s | 0.4 s |
| outcome | tool call parsed | correct answer |

**≈39,982 tokens not re-prefilled on turn 2.** That is the adapter's entire
economic justification, confirmed against a live daemon rather than a fake.

It also corroborates the spec's 2026-08-31 §2 correction: that correction
predicted a ~42k-token floor from a chars/4 estimate; the tokenizer
measured **40,003**, within 5%.

**Tool selection is better than expected from an untrained base.** From 132
tools the model chose `mcp_filesystem_list_directory` with
`{"path": "/tmp"}` — a better fit than the `terminal` tool it was also
offered. From hermes's own 25-tool default set it produced a textbook
OpenAI call:

```json
{"role":"assistant","content":null,"finish_reason":"tool_calls",
 "tool_calls":[{"id":"call_d6d0b444…","type":"function",
   "function":{"name":"search_files",
     "arguments":"{\"target\":\"files\",\"path\":\"/tmp\",\"limit\":50}"}}]}
```

**The refusal path works to the real client.** hermes displayed
bloomery's arithmetic unaltered: `HTTP 409: the model could not be made
resident: needed 1944911872 B, free 1235380384 B, reclaimable 0 B.` The
honest-refusal chain — pager → native API → adapter → OpenAI envelope →
client — is proven end to end, which no fake had tested.

**The diagnostics added as final-review Important 4 are what diagnosed
this run.** Without the per-request JSON log the session/agent/reset
pattern in §4 would not have been visible at all.

## 2. Hermes was never at risk

Run under an isolated `HERMES_HOME` scratch profile. The live gateway
(PID 1169016, 34 days uptime) was untouched and `~/.hermes/config.yaml`
still carries its 2026-07-26 mtime. Nothing under `~/.hermes` was written.

## 3. The tier limit, quantified

The pager's **static boot-time budget** leaves far less room than a naive
read of the card suggests. From the refusal arithmetic:

```
budget            14,064,746,496 B   (13.10 GiB, static boot read)
− overhead         1,073,741,824 B
− weights         11,755,624,288 B   (10.95 GiB resident)
= for contexts     1,235,380,384 B   (1.15 GiB)
```

At 20,480 B/token plus a 512 MiB per-context reservation, that is a
**ceiling of ≈34,000 tokens for a single context, and exactly one context
at a time**. Two consequences, both honest limits rather than defects:

- A 65,536-token window needs 1,944,911,872 B and **cannot be placed at
  all** once the weights are resident. The successful 40,003-token run in
  §1 was served by an agent created before the budget tightened; it is not
  reproducible from a cold start at that window.
- hermes's **132-tool preamble (40,003 tokens) exceeds the ≈34,000-token
  ceiling** on this tier. Its 25-tool default set (≈14,077 tokens) fits
  comfortably.

## 4. Why a real hermes session fails — the finding this run exists for

Not the failure predicted. The final review ranked "hermes may not echo
assistant messages verbatim" as the top silent risk. That is **not** what
happened. What happened is simpler and was predicted by nobody:

**hermes retried a byte-identical request, and the adapter classified the
retry as a history rewrite.**

Both captured requests hash to `493b2c9dbc94ceff` — identical, each
carrying `[system, user]`. The mechanism:

1. Request 1 succeeds. `record_generation` appends the assistant turn, so
   the session now tracks `[system, user, assistant]`.
2. hermes retries the same turn, sending `[system, user]` again.
3. `_is_extension` sees a **shorter** list, correctly concludes it is not
   an extension, and classifies it a rewrite.
4. The reset creates a fresh agent — and on this tier (§3) a second agent
   cannot be placed beside the first. **409.**

Every subsequent turn repeats it: the adapter log shows `reset: true` with
a new agent on each attempt and a constant `delta_bytes: 54952`, i.e. a
full re-render every turn. Agents accumulated to `a7`.

The classification is not wrong in isolation — a shorter list genuinely is
not an extension, and Task 3's reviews were right to demand a reset there.
The gap is that **an ordinary client retry is indistinguishable from a
truncation** once the adapter has appended its own assistant turn to the
tracked history. The retry case was never considered, by me or by any
reviewer, and no fake produced it because fakes echo.

**This is a design question, not a bug to patch blind.** The obvious
treatment — recognising that the incoming list is a prefix of the tracked
list differing only by the assistant turn we ourselves appended, and
re-serving that turn rather than resetting — is plausible but needs its own
spec amendment and its own tests. It also interacts with idempotency: a
retry may or may not want the previous answer replayed.

## 5. Named limitations of this run

- **Not reproducible at the 65,536 window from cold** (§3); the 40,003-token
  measurement stands as measured but its window is not placeable on a fresh
  boot with weights resident.
- **Agent accumulation is now demonstrated, not theoretical.** Every failed
  attempt leaves an agent behind; bloomery has no `DELETE /agents/{id}`, so
  they were cleared by hand with `POST /agents/{id}/suspend`.
- **One prompt, one model, one session shape.** No parse-rate statistic is
  claimed: the sample is too small, and §4 stopped the session before a
  multi-turn trajectory existed. The pre-registered "poor parse rate is a
  finding" question is therefore **unanswered**, not answered favourably.
- `--safe-mode` was used, which may itself be why hermes retried rather
  than executing the returned tool. That was not isolated.

## 6. Artifacts

- `2026-08-31-openai-adapter-acceptance-adapter.log` — the adapter's
  structured per-request log, showing the session/agent/reset pattern.
- `2026-08-31-openai-adapter-acceptance-hermes-capture.jsonl` — every
  request and response between hermes and the adapter, captured by a
  throwaway proxy, including the two identical requests.
