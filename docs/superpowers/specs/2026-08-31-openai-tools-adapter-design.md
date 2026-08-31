# The OpenAI tools adapter — a sidecar that gives bloomery its first real consumer

**Date:** 2026-08-31
**Status:** Approved in conversation (rulings: success criterion = *hermes
actually usable locally*, not a measurement rig and not a minimum viable
demo; model = `qwen36-reap48-ours` **untrained**, after Hermes-4 36B was
killed by the arithmetic in §2 — Brice's pivot once the numbers were on the
table; shape = a **Python sidecar process**, not daemon-side parsing and not
a Rust workspace crate, so the uncertain half iterates fast and the AGPL
daemon grows no model-specific heuristics; downstream API = bloomery's
**native** `/agents/{id}/infer`, not `/v1`, because the adapter must own the
prompt bytes; a tool call that does not parse is returned as text, never
fabricated; and — **amended 2026-08-31 during planning, see §6** —
conversation state is **incremental append**, not full-render diffing,
after the template was measured rewriting historical assistant turns).
**Lineage:** the hermes-consumer spike of 2026-08-30 (same session). Three
of its findings are load-bearing here and each is cited at the point it
decides something: hermes always sends `tools` (n=37, observed in captured
request dumps); `/v1` silently drops that array and answers **HTTP 200 with
empty content and `finish_reason: "stop"`** (observed, not inferred); and
`InferReq` carries no stop sequence. The spike also produced the finding
this spec does *not* address — the pager lock held across `infer`, so any
stuck inference wedges the whole daemon — recorded in CARRIED-DEBT and left
to its own slice.

## 1. What this builds and why

bloomery has no consumer but its own batteries. Every feature since
2026-08-20 ships default-off and is exercised only by the harness written to
measure it. This adapter is the smallest honest path to a real one: it lets
**hermes** — a live, daily-driven agent harness already running on this box —
drive a local model through bloomery, which puts the pager, the window law,
the refusal paths and the journal under load that no battery generates.

The adapter owns exactly what bloomery deliberately refuses to do. bloomery
applies no chat template (the flywheel recipe is explicitly "no chat
template, no EOS", and `/v1`'s `fallback_prompt` concatenates
`"user: …\nassistant: "`). For a tool-calling model the chat template *is*
the tool protocol. So templating, tool-schema rendering and tool-call
parsing live in the sidecar, and the daemon keeps its position that it does
not guess.

Nothing here changes the daemon. No crate is added, no endpoint is altered,
no frozen instrument is touched.

## 2. Why `qwen36-reap48-ours` and not Hermes-4 36B

Recorded because it is a kill, and kills are worth keeping.

Hermes-4 36B was the obvious first choice: Nous trains it for precisely this
agent's tool-calling format, and the GGUF is already on the box. Its GGUF
metadata says `general.architecture = seed_oss`, `block_count = 64`,
`attention.head_count_kv = 8`, `key_length = value_length = 128`. Key and
value lengths are equal and stated, so SPEC R9's MLA branch does not apply
and the dense identity governs:

```
kv_per_token = 64 × 8 × (128 + 128) × 2 = 262,144 B = 256 KiB/token
```

Against `qwen36-reap48-ours` at **20,480 B/token** (read from `/status`,
hybrid geometry, `recurrent_state_bytes = 65,863,680`), that is a **12.8×
span** — the README's opening thesis, that context length is geometry rather
than configuration, deciding a real integration for the first time.

The demand side comes from a captured hermes request
(`~/.hermes/sessions/request_dump_*.json`). **These are character counts
divided by four, not tokenizer output** — an estimate, and labelled as one
everywhere it is used:

| component | chars | ≈ tokens (chars/4) |
|---|---|---|
| system prompt | 21,781 | ~5,445 |
| 37 tool schemas | 65,945 | ~16,486 |
| **fixed floor, before any conversation** | 87,726 | **~21,931** |
| observed session messages | 441,678 | ~110,419 |

> **Corrected 2026-08-31, after the final whole-branch review.** The row above
> was taken from a single captured request and is **not representative** — it
> is near the low end. Measured across all **58** captured hermes requests
> that carry a tool set: the tool count ranges **2 to 132** (median 45), the
> serialised schemas run 6,673 to **145,650** chars (median 75,962 ≈ 18,990
> tokens, max ≈ 36,412), and the median system prompt is 21,281 chars. The
> **worst-case fixed floor is ≈41,970 tokens**, nearly double the 21,931
> quoted above, and the median is ≈24k rather than ~22k.
>
> Two consequences, both strengthening rather than weakening this section.
> First, §2's kill of Hermes-4 36B becomes more decisive, not less: at
> 262,144 B/token that floor costs **≈11.0 GiB of KV alone**, against the same
> ≈14.9 GiB budget, so the model is out by a wider margin than the original
> arithmetic showed. On `qwen36-reap48-ours` at 20,480 B/token the same floor
> costs ≈0.80 GiB — still comfortable. Second, the prefill-once benefit that
> justifies §6 is **larger** than claimed, since it is a median ≈24k and
> worst-case ≈42k tokens saved per turn rather than ~22k.
>
> One caveat this correction introduces: a ≈42k-token preamble against a
> 98k–103k window leaves materially less room for conversation than the
> original figure implied, so window exhaustion (§7's 413 path) will arrive
> sooner in a long session than this spec first suggested. All figures remain
> chars/4 estimates, not tokenizer output.

At 256 KiB/token the ~22k floor costs **5.5 GiB of KV alone**, against a
usable budget of ≈14.9 GiB (15.92 GiB card less ~1.0 GiB desktop):

| quant | weights (GiB) | + floor KV | fits |
|---|---|---|---|
| Q3_K_M (on box, measured) | 16.41 | — | no; weights alone exceed budget |
| Q2_K (estimated ~13) | 13 | 18.5 | no |
| IQ2_XS (estimated ~11) | 11 | 16.5 | no |

Only the first row is measured; the two smaller quants are size estimates,
and they are included to show the conclusion does not turn on them — the
floor KV term alone (5.5 GiB) leaves under 9.4 GiB for a 36B's weights.

**No quantization of Hermes-4 36B fits hermes's own tool-calling floor on
this tier.** A smaller quant does not rescue it, because the KV geometry is
the binding term, not the weights. Partial offload of a 36B leaves most of
the model on CPU, which fails the "usable" criterion by construction.

The same floor on `qwen36-reap48-ours` costs **0.42 GiB**. Its observed
window is 98,106–103,124 tokens (`bound_by: vram`; three values observed
across boots on 2026-08-30, and 107,681 in the committed turn-7 boot-1
journal of 2026-08-29 — it varies with VRAM free at boot), so the
~110k-token observed session overflows it, which §7 treats as designed
behaviour rather than a defect.

The base is a 48%-pruned Qwen3.6-35B-A3B, and its GGUF carries a 7,764-char
`tokenizer.chat_template` with native tool-calling. It is untrained for our
purposes, which is an advantage here: `flywheel7` was SFT'd hard onto the
narrow `<action>` codec, and turn 3 established that training collapses this
line toward the majority format.

## 3. The trained wire format

The template renders tool schemas into a system block and instructs:

```
<tool_call>
<function=example_function_name>
<parameter=example_parameter_1>
value_1
</parameter>
</function>
</tool_call>
```

The adapter therefore **invents no protocol**. It renders through the
model's own template and parses the format the model was trained to emit.
This is the single largest contributor to the odds of the success criterion
being reachable, and it is why templating must be exact rather than
approximate.

## 4. Components

Python, at `adapters/openai-tools/` — a new top-level directory. `tools/` is
measurement and factory tooling; this is a runtime component and does not
belong beside it.

| module | responsibility |
|---|---|
| `server.py` | the OpenAI surface: `/v1/models`, `/v1/chat/completions` |
| `template.py` | extract `tokenizer.chat_template` from the GGUF once at startup; render with jinja2 |
| `session.py` | session ↔ agent mapping, resident-prefix tracking, divergence reset |
| `toolcall.py` | scan `<tool_call>` blocks → OpenAI `tool_calls`; schema-driven type coercion |
| `bloomery.py` | client for `POST /agents`, `POST /agents/{id}/infer`, `POST /agents/{id}/suspend` |
| `errors.py` | `PagerError` → OpenAI error envelope |

Configuration: bloomery base URL, model name, GGUF path (for the template),
listen address. No secrets.

## 5. Data flow, one turn

1. Hermes POSTs `{model, messages, tools}`.
2. `template.py` renders the whole conversation through the model's template
   with `tools=` and `add_generation_prompt=True`.
3. `session.py` diffs that render against the prefix already resident in
   this session's agent KV and yields **only the new suffix** (§6).
4. `bloomery.py` POSTs the suffix to `/agents/{id}/infer`.
5. `toolcall.py` parses the reply into `tool_calls`, coercing each parameter
   to the type its tool schema declares.
6. The response carries `finish_reason` = `tool_calls`, `stop`, or `length`.

## 6. Session lifecycle — the load-bearing section

**bloomery appends.** `generate_from` sets
`entry_pos = kv_cache_seq_pos_max(SEQ_ID) + 1` and tokenizes with
`AddBos::Never` whenever `pos > 0`, so a prompt is added *after* whatever the
sequence already holds. Sending the full rendered conversation every turn
would duplicate the entire transcript into KV. The adapter must send deltas;
this is not an optimization, it is a correctness requirement.

One hermes session maps to one persistent bloomery agent. The adapter
records the exact rendered prefix that agent's KV holds, and each turn sends
only the suffix beyond it.

> **Amended 2026-08-31, during planning — the full-render diff below is
> WITHDRAWN and replaced by incremental append (Brice's ruling: option A).**
> Measured while writing the implementation plan: this model's chat template
> **rewrites historical assistant turns**, splitting on `</think>` and
> rebuilding the turn without the reasoning block
> (`content.split('</think>')[-1].lstrip('\n')`). So a re-render of the
> conversation is *not* a byte-prefix extension of what the KV holds.
> Demonstrated: with a one-tool conversation, `rerender.startswith(resident)`
> is **False**, diverging at index 1161 — KV holds
> `…assistant\n<think>\n\nI should call terminal.\n</think>\n\n<tool_call>…`
> while the re-render holds `…assistant\n<tool_call>…`. Under the original
> design the divergence check would fire **every turn**, resetting the agent
> every turn, and the preamble saving that motivates this whole section would
> never materialise once.
>
> **The replacement.** The adapter never re-renders history. It tracks the
> resident byte string itself: turn 1 is a full template render (system +
> tools + first user + generation prompt); after generation the raw generated
> bytes are appended to that record, because bloomery has already fed them
> into the KV; each later turn sends `<|im_end|>\n` (closing the assistant
> turn the model left open) + the per-turn rendering of the new user/tool
> messages + the generation prompt. Divergence detection moves from a byte
> diff of renders to a **semantic prefix check on hermes's `messages` list** —
> did it append, or did it edit/compress? — which is both simpler and
> strictly more robust.
>
> **The deviation this accepts, stated plainly:** history retains `<think>`
> blocks that the template's own convention drops. Its effect on quality is
> **unmeasured**. It is therefore a config flag (`keep_reasoning_in_history`,
> default true) so the alternative — full re-render with a fresh agent per
> turn, correct by construction and with no reuse — stays testable against
> it rather than merely arguable. The per-turn wrapper must be *derived from
> the template by differential rendering*, never hand-written ChatML, and
> pinned by a test that catches template drift.
>
> The token-boundary rule immediately below still governs, and matters more
> under append than it did under diffing.

**The delta must be split at a token-safe boundary.** The prefix lives in KV
as *tokens*, but the adapter diffs *strings*, and bloomery tokenizes each
suffix independently (`str_to_token(prompt, AddBos::Never)`). Tokenization
does not distribute over concatenation: `tok(A) ++ tok(B)` is not in general
`tok(A ++ B)`, so an arbitrary character-offset split silently produces a
token sequence the model never saw during training. The rule is therefore:
**split only at rendered turn boundaries** — the `<|im_start|>` / `<|im_end|>`
special-token delimiters the template emits — never at an arbitrary offset.
A diff that does not land on such a boundary is treated as divergence and
takes the reset path below rather than being sent. This is a correctness
constraint, not a heuristic, and it gets its own test.

**Why this is the reason to route hermes through bloomery at all.** The
~21,931-token fixed preamble (§2, chars/4 estimate) is resent by hermes on
every turn. Against a stateless backend it is re-prefilled every turn.
Against a persistent agent it is prefilled **once per session**. That is the
concrete benefit, and it is measurable as prefill tokens per turn after the
first.

**Divergence.** If the new render is not an extension of the recorded
prefix — hermes edited history, or its `trajectory_compressor` rewrote the
transcript — the KV is stale. The adapter then abandons that agent and
starts a fresh one. Exact-prefix or reset; never approximate reuse. This is
the memory organ's two-stage exact-match discipline applied to a different
store, and for the same reason: an approximate match here would silently
corrupt the model's context.

**Known gap, carried not worked around.** bloomery exposes `suspend` and
`resume` but no `DELETE /agents/{id}`, so a reset suspends and abandons.
Agents accumulate across a long-lived adapter process. Recorded in
CARRIED-DEBT on arrival; a delete endpoint is a daemon slice, not this one.

> **Amended 2026-08-31 after the live acceptance run (Brice's ruling).**
> The two-state classification above — *append* or *rewrite* — is
> incomplete, and the gap stopped a real hermes session dead. Evidence:
> `docs/superpowers/evidence/2026-08-31-openai-adapter-acceptance.md`.
>
> **The missing state is a retry.** hermes sent a byte-identical request
> twice (both hashing `493b2c9dbc94ceff`). After the first, this adapter's
> own `record_generation` had appended the assistant turn, so the tracked
> history was `[system, user, assistant]` while the retry carried
> `[system, user]` — *shorter*. `_is_extension` correctly reported "not an
> extension", the turn was classified a rewrite, and the reset created a
> fresh agent that the tier could not place. **The classification was right;
> the vocabulary was too small.** An ordinary client retry is
> indistinguishable from a truncation once the adapter has appended a turn
> the client never sent.
>
> **The cause is that one list was being asked to mean two things.** The
> adapter must track them separately:
>
> - the **KV view** — every turn whose bytes reached the cache, including
>   the assistant turn the adapter appended after generating it;
> - the **client view** — the message list as the client last actually sent
>   it, before any adapter-side append.
>
> Classification then falls out with no heuristics, and in this order:
> 1. incoming **equals the client view** → **retry**;
> 2. incoming **extends the KV view** → **append** (unchanged behaviour);
> 3. otherwise → **rewrite**, reset as before.
>
> **A retry replays the previous response.** This is not a shortcut. The
> substrate samples with `LlamaSampler::greedy()`, so re-inferring a
> byte-identical prompt yields byte-identical output: replay is
> *observationally equivalent* to re-inference, costs no tokens, and leaves
> the KV untouched. The equivalence holds **within one boot and one
> context**, which is exactly a session's scope; it is explicitly NOT
> claimed across launches, because turn-5's evidence recorded Vulkan greedy
> producing differing prose across boots.
>
> **Replay is bounded at one.** A client may be retrying because it could
> not use the previous answer, and replaying forever would loop. After a
> single replay, a further identical request falls through to the ordinary
> path — where rule 3 classifies it a rewrite and it is re-inferred against
> a fresh agent. Every replay is logged, so the behaviour is visible rather
> than silent.

## 7. Error mapping

| `PagerError` | HTTP | OpenAI shape |
|---|---|---|
| `PromptTooLarge` | 413 | `context_length_exceeded`, carrying the real token arithmetic |
| `Refused` | 409 | `server_error`, retryable, carrying bytes needed / free / reclaimable |
| `Budget` | 402 | `insufficient_quota` |
| `DriftBlocked` | 409 | `server_error`, naming the blocked model |
| `Contract` | 502 | `server_error` — the substrate broke its own protocol |

The 413 path is designed behaviour, not a failure mode. The observed session
(~110k tokens) exceeds the ~98–103k window, and hermes ships its own
`trajectory_compressor` and `compression:` configuration. bloomery's honest
oversize refusal, with arithmetic, becomes a signal a real client can act
on — the first time that path is exercised by anything but a test.

## 8. Non-goals for v1

Streaming (the captured hermes requests carry no `stream` key), multimodal
content parts, `tool_choice` forcing beyond `auto`, retry-on-malformed-output
loops, and any daemon change.

**A tool call that does not parse is returned as `content`. The adapter
never fabricates a `tool_calls` entry.** Guessing would reproduce inside the
adapter exactly the silent-failure class the spike just caught in `/v1`,
where a dropped `tools` array still produced a 200 claiming normal
completion. The adapter's whole reason to exist is that bloomery does not
guess; it does not get to guess either.

## 9. Testing

- **Unit, GPU-free.** Template rendering against golden bytes; tool-call
  parsing over a fixture set including malformed, nested, multi-call and
  truncated cases; schema type coercion; prefix-delta computation;
  divergence detection; error mapping.
- **Integration, GPU-free.** A stub bloomery HTTP server, so the entire
  request → render → delta → parse → respond loop runs with no model and no
  GPU. This matches the daemon's own discipline: its 911 tests need neither.
- **Live acceptance, human-gated.** Real daemon, real hermes session, one
  tool-using turn end to end. Hermes's own configuration is not edited in
  place; acceptance uses a scratch profile.

Mutation checks on the two modules where a passing test could be vacuous:
`toolcall.py` (a parser that accepts everything passes every positive test)
and `session.py` (a delta function that returns the whole render every time
is correct-looking and destroys the KV benefit).

## 10. Named risks

1. **The model may simply be bad at this.** It is an untrained, 48%-pruned
   MoE. Parse rate over the §9 fixture set is the first thing to measure; a
   poor result is a finding, not a failure, and the fixtures are already the
   instrument for it.
2. **Compression thrash.** Frequent divergence resets would negate the KV
   win. Countable as resets per session; if it dominates, the design
   question moves to whether hermes's compression can be pinned.
3. **Type coercion.** Parameters arrive as strings and schemas want ints,
   booleans and arrays. Coercion is best-effort and must fail loudly rather
   than pass a wrong type silently.
4. **Agent accumulation** from §6's missing delete.
5. **Token estimates.** Every token count in §2 and §6 is chars/4, not
   tokenizer output. They are used only to compare orders of magnitude and
   to justify a model choice that a 12.8× geometry span decides outright;
   no floor, gate, or endpoint in this spec rests on them. Anything that
   later needs a real number must tokenize.

## 11. What this spec does not settle

- Whether the success criterion is reachable at all. That is the point of
  building it; §10.1 is the honest statement of the odds.
- The pager-lock blast radius found by the spike. Its own slice.
- `/v1`'s silent `tools` drop. Worth fixing independently of this adapter,
  since it misleads *any* OpenAI client, and it is cheap: reject with 400
  rather than answer 200 with empty content.
- Whether the adapter graduates into a Rust workspace crate. Revisit with
  evidence that it works, never on faith.
- **A KV snapshot/restore (or truncate-to-position) endpoint on the daemon.**
  Added by the 2026-08-31 amendment in §6: it is the change that would let
  the adapter have both the template's exact history convention *and* the
  preamble saving — snapshot immediately after the invariant preamble,
  restore it each turn, re-render only the conversation. bloomery already
  owns the machinery (`save_state` / `load_state` and the KV image store);
  what is missing is a surface. That is a daemon slice with its own spec and
  it must not block this adapter.
- Whether keeping `<think>` in history costs anything. The flag exists so
  this becomes a measurement rather than an argument; nothing here claims an
  answer.
