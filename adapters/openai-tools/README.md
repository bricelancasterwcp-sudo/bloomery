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

- **No streaming.** Every response is a single, complete
  `chat.completion`; there is no `stream: true` / SSE support.
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
| `max_tokens` | no (default `512`) | Default generation cap when a request omits `max_tokens`. |
| `window_cap` | no | Passed through to `create_agent`'s `window_cap`, if bloomery should cap this agent's KV window below the model's default. |
| `keep_reasoning_in_history` | no (default `true`) | See below. |
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

**Agents accumulate; there is no cleanup.** Every new session key creates
one bloomery agent via `create_agent`, held in `server.sessions` for the
adapter process's lifetime. bloomery's native API has `POST
/agents/{id}/suspend` (used by `BloomeryClient.suspend`) but **no `DELETE
/agents/{id}`**, so a suspended agent's resources are not released back to
the daemon by this adapter — there is currently no code path that even
calls `suspend`. Long-running deployments with many distinct sessions will
accumulate agents (and their resident KV/window budget) on the daemon side
until the daemon itself is restarted. This is a known gap, not an oversight
to be silently worked around; closing it needs either a bloomery-side
delete/reap endpoint or an adapter-side idle-session suspend policy, and
that is future work, not something this task's scope covers.
