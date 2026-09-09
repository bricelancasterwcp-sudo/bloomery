# openai-tools adapter

A small Python sidecar that lets an OpenAI tool-calling client (any SDK or
harness that speaks `POST /v1/chat/completions` with a `tools` array) drive a
local model served by the **bloomery** daemon.

## What it does

- Renders the model's own chat template (`ChatTemplate`) so the model sees
  exactly the prompt bytes it was trained on — never a hand-written
  approximation of ChatML.
- Sends only the NEW turns on every request after the first (`Session`,
  differential rendering against the daemon's append-only KV cache), so the
  `<tools>` schema preamble and prior history are prefilled once per
  session, not resent every turn.
- Parses the model's trained `<tool_call>` XML format back into OpenAI
  `tool_calls` (`toolcall.py`'s strict left-to-right scanner) — anything
  that does not parse cleanly is returned as plain `content`, never
  fabricated into a fake call.
- Maps bloomery's own typed refusals (oversized prompt, residency refused,
  budget exhausted, ...) onto OpenAI-shaped error envelopes with the
  numbers bloomery itself reports, instead of a bare 500.

## What it deliberately does not do

- **Buffered streaming only.** `stream: true` is honoured in the shape
  bloomery's own `/v1` ships: the whole completion is generated first, then
  rendered as SSE chunks (`stream_options.include_usage` respected). A
  refusal before the first chunk still returns a real HTTP status, never a
  streamed 200. There is no token-by-token streaming.
- **No multi-model routing, no load balancing, no auth.** One adapter
  process talks to one bloomery daemon serving one resident model. Put a
  reverse proxy in front of it if you need TLS, auth, or fan-out.
- **No persistence.** Session state (which bloomery agent belongs to which
  conversation) lives in this process's memory only. Restarting the
  adapter drops every session; clients get a fresh agent and a full
  `<tools>` re-render on their next turn.
- **No fabrication, ever.** If the model's output does not parse as a
  complete, well-formed tool call, it comes back as `content` with
  `finish_reason: "stop"` — never as a synthesized `tool_calls` entry.
  Guessing here would reproduce the exact silent-failure class the rest of
  this project (and bloomery itself) exists to refuse.

## Running it

```bash
cp adapters/openai-tools/config.example.json adapters/openai-tools/config.json
# edit config.json: base_url, model, max_tokens, ...
python3 -m openai_tools.server adapters/openai-tools/config.json
```

This starts a `ThreadingHTTPServer` exposing:

- `GET /v1/models` — lists the one configured model.
- `POST /v1/chat/completions` — the main entry point.

The bloomery client is constructed from `config["base_url"]` inside
`main()`, but `server.build_server(config, client)` takes the client as a
**parameter**, not something it constructs internally. That is what lets
the entire request/response loop — template rendering, KV-append
bookkeeping, tool-call parsing, error mapping — be exercised in
`tests/test_server.py` against a scripted stub with no daemon and no GPU.

## Config keys (`config.json`)

| Key | Required | Meaning |
|---|---|---|
| `base_url` | yes (for `main()`) | The bloomery daemon's native API base URL, e.g. `http://127.0.0.1:8080`. Only read when running as `__main__`; `build_server` itself never touches it, since the client is injected. |
| `model` | yes | The model name passed to bloomery's `create_agent` and reported by `GET /v1/models`. |
| `template` | yes | Path to the committed `.jinja` chat template (see below). |
| `max_tokens` | no (default `512`) | Default generation cap when a request omits both `max_tokens` and `max_completion_tokens`. |
| `max_tokens_cap` | no (default `4096`) | Hard ceiling the resolved `max_tokens` is clamped to before it reaches bloomery. Real hermes requests carry `max_tokens` of 64000-128000; sent through unclamped against a ~98k-103k-token window, the daemon's window law 413s nearly every request. A smaller client-supplied value is still honoured verbatim; only the ceiling is enforced. |
| `window_cap` | no | Passed through to `create_agent`'s `window_cap`, if bloomery should cap this agent's KV window below the model's default. |
| `keep_reasoning_in_history` | no (default `true`) | See below. |
| `idle_ttl_secs` | no (default `900`) | A session idle longer than this is reaped: its bloomery agent is `DELETE`d and the session forgotten, so its window is free for the next conversation. `null` disables the sweep. See "Idle reaping" below; the residency-refusal reap runs regardless of this key. |
| `host` / `port` | no (default `127.0.0.1` / OS-assigned) | Bind address. |

## Re-extracting the template, and why it is committed

The adapter never imports the `gguf` package at runtime — it reads a
plain committed file, `templates/qwen36-reap48-ours.jinja`, pinned by its
own SHA-256 (`ChatTemplate.sha256`). This is deliberate: the template *is*
the contract for what bytes the model was trained to see, and pinning it
as a versioned artifact means a template drift shows up as a **failing
test** (`ChatTemplate` identity / `render_turns` self-consistency checks),
not as a silent prompt-format regression discovered by the model behaving
strangely in production.

To re-extract it from a GGUF (one-time, only needed when swapping to a
different model or a re-quantized build that changed its embedded
template):

```bash
PYTHONPATH=~/llama.cpp/gguf-py python3 adapters/openai-tools/tools/extract_template.py \
    /path/to/model.gguf \
    adapters/openai-tools/templates/<name>.jinja
```

Then update `config.json`'s `template` path (and `model`) to match, and
commit the new `.jinja` file alongside the config change.

## `keep_reasoning_in_history`

`Session(..., keep_reasoning=True)` (the default) reuses the resident KV
cache across turns by sending only the new tail on every request after the
first — the entire point of this adapter's session bookkeeping. Setting it
`false` makes every turn a full re-render (system + tools + entire history
resent every time), which foregoes that KV-reuse benefit entirely but never
diverges from a byte-exact re-render of history.

**The quality effect of `keep_reasoning=True` on the model's answers is
UNMEASURED.** It changes what the model actually sees on turn 2+ (its own
prior reasoning stays in context verbatim, appended rather than
re-derived) relative to `keep_reasoning=False`'s from-scratch re-render.
Whether that helps, hurts, or is neutral for answer quality on any given
task has not been benchmarked as part of this adapter; only the mechanical
claim — that the two modes send byte-consistent, template-derived prompts
— is covered by tests. Treat `keep_reasoning_in_history` as a performance
knob with an open quality question (spec's §6 amendment), not a
tuned default.

## Known limitations

**Session identity is a heuristic, not a real identity.** Absent an
explicit `X-Session-Id` request header, the adapter keys sessions by a
SHA-256 hash of the first user message (`_first_user_message_key` in
`server.py`). This means: two genuinely different clients that happen to
open a conversation with the *identical* first user message will collide
onto the same bloomery agent and share its session state — clients that
care about isolation (multi-tenant use, automated test suites issuing the
same canned first prompt, etc.) **must** send their own `X-Session-Id`
header. There is no cryptographic or client-identity component to the
fallback; it is purely a convenience default for casual single-client use.

**Agents used to accumulate; since 2026-09-07 the adapter reaps them.**
Every new session key creates one bloomery agent via `create_agent`, and the
adapter has no session-end signal (a one-shot client simply exits), so before
the idle reap every conversation left its agent resident for the adapter
process's lifetime. The 25-tool hermes dogfood
([evidence](../../docs/superpowers/evidence/2026-09-07-hermes-dogfood-25-tools.md))
turned that into a user-facing failure: this tier holds two 30k-token
windows, so the third conversation was refused with 409 — bloomery's
`plan_residency` evicts only agents of strictly lower priority (every adapter
agent shares `default_priority`), and its equal-priority time-share admits a
requester only after *it* has waited a full quantum, which a one-shot client
never does. What remains true: a session that is mid-request is never
reaped, so a client that holds many conversations open *simultaneously* can
still fill the tier, and the daemon's 409 then reaches it unchanged.

## Idle reaping

Two paths, both `openai_tools/reap.py`, both `DELETE /agents/{id}` (which
frees the agent's window — `suspend` only parks an agent and keeps its
budget, which is why suspend never fixed the leak):

- **Idle TTL.** A daemon thread started by `main()` sweeps every
  `min(idle_ttl_secs / 2, 30)` seconds and deletes every session idle
  *strictly* longer than `idle_ttl_secs` (default 900; `null` disables the
  sweep). Idle is measured from the start of the session's last request (a request in flight holds its lock and is never a candidate, so the difference from "end" is at most one inference).
- **Residency refusal.** When bloomery refuses an inference with 409, the
  adapter deletes the *least-recently-used* idle session and retries that
  inference exactly once. If nothing is idle, or the retry is refused too,
  the 409 reaches the client unchanged. A pure TTL cannot cover
  back-to-back conversations: any TTL long enough not to reap
  mid-conversation is too long to free a window for the next one.

A session is idle only if its request lock can be taken without blocking;
a request in flight is never reaped, whatever the clock says. A reaped
session that comes back simply gets a fresh agent and a full re-render of
its history (the ordinary first-turn path), at the cost of one preamble
prefill. A failed `DELETE` is logged (`delete_failed`) and the session kept
for the next sweep; a 404 means the agent is already gone and the session
is dropped. `build_server` never starts the sweeper — only `main()` does —
so tests drive `reap.reap_idle` with an injected clock and no thread.

## Diagnostics

Every request writes one structured JSON line to stderr via the stdlib
`logging` module (`openai_tools.server` logger): the session key, the
bloomery agent id used, whether this turn was a reset, the delta size in
bytes, `prompt_tokens`/`completion_tokens` from the daemon (when the turn
reached the daemon), and the outcome (`"ok"`, `"retry_replayed"`, `"invalid_tool_arguments"`, or
`"bloomery_error_<status>"`). A reap writes `{"event": "session_reaped",
"session", "agent", "reason": "idle_ttl" | "residency", "idle_secs"}`, and a
delete that failed writes `{"event": "delete_failed", ...}`; a residency
reap is therefore visible as a `bloomery_error_409` line, a `session_reaped`
line, then the retried turn's `ok`. An unexpected (non-`BloomeryError`) exception
also logs its full traceback to stderr before the client gets the generic
500 — the response body never carries internal detail, but the process's
own stderr always does. `log_message` (raw per-connection HTTP access
logging) stays quiet by design; this is a separate, purpose-built request
log.
