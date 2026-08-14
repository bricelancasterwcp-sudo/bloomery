# Phase 0 — prior-art verification and decision record (D1–D4)

**Date:** 2026-08-14
**Task:** Phase 1 plan, Task 1.
**Status:** complete. All four defaults **CONFIRMED**; no plan amendment.
The "Overturned defaults" section at the end is mandatory and is present
whether or not anything was overturned, so silence is distinguishable from
omission.

This record either **confirms or overturns** four pre-registered defaults
(D1–D4) declared in `docs/superpowers/plans/2026-08-14-phase1-pager-daemon.md`.
Every symbol name below was read out of a real header or a real published
crate tarball; the file and line are cited. Nothing here is from memory.

## Verification environment

| Fact | Value | How checked |
|---|---|---|
| Box GPU | NVIDIA GeForce RTX 5080, 16303 MiB | `nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv` |
| Driver | 595.84 | same |
| Vulkan loader | libvulkan.so.1.4.341, instance API 1.4.341 | `vulkaninfo --summary`; `ls /usr/lib/x86_64-linux-gnu/libvulkan.so*` |
| Vulkan headers | `/usr/include/vulkan/vulkan.h` present | `ls` |
| Shader compiler | `/usr/bin/glslc`, `/usr/bin/glslangValidator` | `which` |
| Local llama.cpp checkout | `/home/brice/llama.cpp` @ `4988f6e`, 2026-06-13 (shallow clone) | `git log -1 --format="%H %ci %s"` |
| Rust | `~/.cargo/bin/cargo`, `~/.cargo/bin/rustc` | `which` |

The box is the enthusiast-16GB tier named in the spec (§4.7, G2). The Vulkan
build prerequisites for Task 11's `vulkan` feature are all present — this is
worth recording because `llama-cpp-sys-2`'s `build.rs` links `vulkan` and
builds shaders with `glslc` (`llama-cpp-sys-2-0.1.154/build.rs:882-953`), and
a missing `glslc` is the usual first failure of that build.

---

## D1 — llama.cpp bindings and state APIs

**Default:** the `llama-cpp-2` crate (feature `vulkan`) can (a) load a GGUF
with `n_gpu_layers`, (b) create a context with per-context `n_ctx`, (c) decode
and expose real token counts, and (d) save/restore full context state.

**Verdict: CONFIRMED**, with two implementation corrections for Task 11 that
do *not* overturn the default but change which symbols Task 11 should call
(both in D1.4).

### D1.0 Versions pinned by this verification

| Thing | Version | Source |
|---|---|---|
| `llama-cpp-2` | **0.1.154** (published 2026-08-05; current `max_version`) | `https://crates.io/api/v1/crates/llama-cpp-2` |
| `llama-cpp-sys-2` | **0.1.154** (path+version dep of the above) | `llama-cpp-2-0.1.154/Cargo.toml.orig` |
| llama.cpp vendored by that sys crate | submodule commit **`5f55650a78f92aff4d48d671423e888fac0469ff`**, dated **2026-07-30** | `gh api repos/utilityai/llama-cpp-rs/contents/llama-cpp-sys-2/llama.cpp?ref=0.1.154` |
| Crate repo | `https://github.com/utilityai/llama-cpp-rs` | `Cargo.toml.orig` |

Sources actually read (not summarized — the tarballs were downloaded and
extracted):

- `https://static.crates.io/crates/llama-cpp-2/llama-cpp-2-0.1.154.crate`
- `https://static.crates.io/crates/llama-cpp-sys-2/llama-cpp-sys-2-0.1.154.crate`
- `/home/brice/llama.cpp/include/llama.h` (local checkout @ `4988f6e`)
- `llama-cpp-sys-2-0.1.154/llama.cpp/include/llama.h` (the *vendored* header —
  this is the one that actually generates the bindings)

> **Note on docs.rs.** `https://docs.rs/llama-cpp-2/latest/llama_cpp_2/context/struct.LlamaContext.html`
> was consulted first and got one signature wrong when read through a
> summarizer (it reported `state_seq_set(&mut self, state: &[SeqState], …)`;
> the published source says `state: &SeqState`, singular). **Every signature in
> this document is quoted from the crate tarball, not from docs.rs.** Task 11
> should trust the file:line citations here over any rendered docs page.

### D1.1 GGUF load with `n_gpu_layers` — CONFIRMED

```rust
// llama-cpp-2-0.1.154/src/model.rs:762
pub fn load_from_file(
    _: &LlamaBackend,
    path: impl AsRef<Path>,
    params: &LlamaModelParams,
) -> Result<Self, LlamaModelLoadError>
```

```rust
// llama-cpp-2-0.1.154/src/model/params.rs:512
pub fn with_n_gpu_layers(mut self, n_gpu_layers: u32) -> Self
// getter at src/model/params.rs:433 -> i32
```

Also available and relevant to the pager: `with_main_gpu` (`params.rs:524`)
and `with_split_mode` (`params.rs:552`).

### D1.2 Per-context `n_ctx` — CONFIRMED

```rust
// llama-cpp-2-0.1.154/src/model.rs:820
pub fn new_context<'a>(
    &'a self,
    _: &LlamaBackend,
    params: LlamaContextParams,          // by value, not by reference
) -> Result<LlamaContext<'a>, LlamaContextLoadError>
```

```rust
// llama-cpp-2-0.1.154/src/context/params/get_set.rs:21
pub fn with_n_ctx(mut self, n_ctx: Option<NonZeroU32>) -> Self
// get_set.rs:38   pub fn n_ctx(&self) -> Option<NonZeroU32>
// get_set.rs:53   pub fn with_n_batch(mut self, n_batch: u32) -> Self
// get_set.rs:113  pub fn with_n_seq_max(mut self, n_seq_max: u32) -> Self
// get_set.rs:172  pub fn with_n_threads(mut self, n_threads: i32) -> Self
```

**Lifetime note for Task 11 (confirms the plan's borrow-check warning):**
`new_context<'a>(&'a self, …) -> LlamaContext<'a>` — the context genuinely
borrows the model. The plan's "arena struct owning models and contexts
together, expose only handles" guidance stands; `Box::leak` remains
prohibited.

### D1.3 Real token counts — CONFIRMED, with a nuance worth writing down

```rust
// llama-cpp-2-0.1.154/src/model.rs:302
pub fn str_to_token(&self, str: &str, add_bos: AddBos)
    -> Result<Vec<LlamaToken>, StringToTokenError>
// src/context.rs:101   pub fn decode(&mut self, batch: &mut LlamaBatch) -> Result<(), DecodeError>
// src/context.rs:88    pub fn n_ctx(&self) -> u32
// src/llama_batch.rs:147  LlamaBatch::new(n_tokens: usize, n_seq_max: i32) -> Self
// src/llama_batch.rs:169  LlamaBatch::get_one(tokens: &'a [LlamaToken]) -> Result<Self, BatchAddError>
// src/llama_batch.rs:50   pub fn add(...)
// src/llama_batch.rs:196  pub fn n_tokens(&self) -> i32
```

**Nuance:** llama.cpp does *not* hand back a usage/stats struct. There is no
"read the token counts off the reply" API to be lied to by. `prompt_tokens` is
`str_to_token(...)?.len()`; `completion_tokens` is the count of decode
iterations bloomery itself performs. This is a *stronger* position than the
default assumed — the counts are real **because we are the ones counting**,
which is exactly the construction design law 4 / spec §4.1 asks for ("a reply
without stats cannot be constructed"). Task 11 should note in code that the
honesty here is structural, not a trusted upstream field.

**Bonus for Task 4 (geometry / the window law):** `LlamaModel::n_ctx_train()
-> u32` (`src/model.rs:144`) supplies the `training_ctx` term of law 1, and
`LlamaContext::n_ctx() -> u32` (`src/context.rs:88`) reports the window
llama.cpp *actually allocated*, which may differ from what was requested.
Task 4 should read back the latter rather than trusting the requested value.

**Bonus for Task 5 (VRAM probe):** the crate exposes a device enumerator that
already carries free/total VRAM, so the probe need not shell out to
`nvidia-smi`:

```rust
// llama-cpp-2-0.1.154/src/lib.rs:512
pub fn list_llama_ggml_backend_devices() -> Vec<LlamaBackendDevice>
// struct at src/lib.rs:491 — fields: index, name ("Vulkan0"), description,
// backend ("Vulkan"|"CUDA"|"CPU"), memory_total: usize, memory_free: usize,
// device_type
```
It wraps `ggml_backend_dev_get_props` (`src/lib.rs:527`). Note this is a
`Vec` of *all* backends including CPU; the probe must filter by
`device_type == Gpu` and must still return `None`-with-a-reason (not zero) if
no GPU device is enumerated, per law 5.

### D1.4 Save/restore context state — CONFIRMED, but call the *seq* API

The C API in the vendored header (`llama-cpp-sys-2-0.1.154/llama.cpp/include/llama.h`;
identical shape in the local checkout at `/home/brice/llama.cpp/include/llama.h:783-905`):

| C symbol | vendored llama.h line | status |
|---|---|---|
| `llama_state_get_size(ctx) -> size_t` | 800 | current |
| `llama_state_get_data(ctx, uint8_t* dst, size_t size) -> size_t` | 807 | current |
| `llama_state_set_data(ctx, const uint8_t* src, size_t size) -> size_t` | 818 | current |
| `llama_get_state_size(ctx)` | 801 | **DEPRECATED** ("use `llama_state_get_size`") |
| `llama_copy_state_data(ctx, uint8_t* dst)` | 811 | **DEPRECATED**, no size argument |
| `llama_set_state_data(ctx, const uint8_t* src)` | 822 | **DEPRECATED**, no size argument |
| `llama_state_seq_get_size_ext(ctx, seq_id, flags)` | 905 | current |
| `llama_state_seq_get_data_ext(ctx, dst, size, seq_id, flags)` | 910 | current |
| `llama_state_seq_set_data_ext(ctx, src, size, dest_seq_id, flags)` | 917 | current |
| `llama_state_save_file` / `llama_state_load_file` | (local hdr 811, 825) | current |
| `llama_state_seq_save_file` / `llama_state_seq_load_file` | (local hdr 859, 866) | current |

The Rust wrappers in `llama-cpp-2` 0.1.154, all on `LlamaContext`, all in
`src/context/session.rs`:

| Rust method | line | wraps | safety |
|---|---|---|---|
| `get_state_size(&self) -> usize` | 449 | `llama_get_state_size` **(deprecated C symbol)** | safe |
| `unsafe copy_state_data(&self, dest: *mut u8) -> usize` | 460 | `llama_copy_state_data` **(deprecated, unbounded)** | `unsafe` |
| `unsafe set_state_data(&mut self, src: &[u8]) -> usize` | 470 | `llama_set_state_data` **(deprecated, drops `src.len()`)** | `unsafe` |
| `state_seq_get_size_ext(&self, seq_id: i32, flags: LlamaStateSeqFlags) -> usize` | 487 | `llama_state_seq_get_size_ext` | safe |
| `unsafe state_seq_get_data_ext(&self, dest: *mut u8, seq_id: i32, flags) -> usize` | 510 | `llama_state_seq_get_data_ext` (passes `usize::MAX` as size) | `unsafe` |
| `unsafe state_seq_set_data_ext(&mut self, src: &[u8], dest_seq_id: i32, flags) -> bool` | 545 | `llama_state_seq_set_data_ext` | `unsafe` |
| **`state_seq_get(&self, seq_id: i32, flags) -> Result<SeqState, StateSeqError>`** | **582** | `llama_state_seq_get_size_ext` + `_get_data_ext`, size-checked | **safe** |
| **`state_seq_set(&mut self, state: &SeqState, seq_id: i32) -> Result<(), StateSeqError>`** | **624** | `llama_state_seq_set_data_ext`, size-checked | **safe** |
| `state_save_file(&self, path, tokens: &[LlamaToken])` | 249 | `llama_state_save_file` | safe |
| `state_load_file(&mut self, path, max_tokens) -> Result<Vec<LlamaToken>, _>` | 293 | `llama_state_load_file` | safe |
| `state_seq_save_file(&self, filepath, seq_id, tokens) -> Result<usize, _>` | 352 | `llama_state_seq_save_file` | safe |
| `state_seq_load_file(&mut self, filepath, dest_seq_id, max_tokens) -> Result<(Vec<LlamaToken>, usize), _>` | 401 | `llama_state_seq_load_file` | safe |
| `save_session_file` / `load_session_file` | 157 / 195 | deprecated since crate 0.1.136 | do not use |

**Correction 1 (Task 11 must not follow the plan sketch verbatim).** The plan
sketched `unsafe` calls through
`llama_cpp_sys_2::{llama_state_get_size, llama_state_get_data, llama_state_set_data}`.
Those raw symbols *do* exist in the bindings (see D1.5), but they are not
needed. **Task 11 should use the fully safe, size-checked
`LlamaContext::state_seq_get` / `state_seq_set` pair** (`session.rs:582`,
`session.rs:624`), which returns an opaque `SeqState` whose bytes cannot be
forged from safe code, and which returns `StateSeqError::SizeMismatch` when
llama.cpp's own deserializer rejects a shape mismatch (different `n_ctx`,
`n_layer`, quantization). That error is the *mechanical* detection of the
spec §9 "KV image invalidated by an upgrade" risk, and per the spec it must be
handled as a cold start, never as an error.

`SeqState` also exposes `byte_len() -> usize` (`session.rs:675`) and
`flags() -> LlamaStateSeqFlags` (`session.rs:669`) — `byte_len` is the KV image
size the pager accounts and journals.

**Correction 2 (do not use the crate's whole-context trio).** The crate's
`get_state_size` / `copy_state_data` / `set_state_data` wrap the **deprecated,
size-unbounded** C symbols. In particular `set_state_data(&mut self, src: &[u8])`
calls `llama_set_state_data(ctx, src.as_ptr())` and **discards `src.len()`**
(`session.rs:471`) — a truncated or corrupt image is an out-of-bounds read
inside C, not an error return. If Task 11 ever needs whole-context state
rather than per-sequence state, it must go through the raw
`llama_cpp_sys_2::llama_state_set_data(ctx, src.as_ptr(), src.len())` (the
size-bounded current symbol), not the crate wrapper. The crate does **not**
wrap the modern non-seq trio at all — verified by
`grep -rn "llama_state_get_size\|llama_state_get_data\|llama_state_set_data" src/`
over the 0.1.154 tarball returning **no hits**.

**Flags (`LlamaStateSeqFlags`, `session.rs:14-47`):**
`empty()` = 0, `PARTIAL_ONLY` = 1, `ON_DEVICE` = 2.
For bloomery's VRAM→RAM→NVMe paging the correct value is
`LlamaStateSeqFlags::empty()`: `ON_DEVICE` explicitly keeps the bytes in
device buffers (faster, but *not host-accessible*), which defeats the entire
point of paging out of VRAM. The vendored header adds the warning that
getting a state with `ON_DEVICE` invalidates all prior `ON_DEVICE` states for
that `seq_id` (`/home/brice/llama.cpp/include/llama.h:882-884`).

**Design consequence for Tasks 12–13:** because the safe API is *per-sequence*,
an "agent" maps most naturally onto a `seq_id` and the KV image is that
sequence's `SeqState`. Whether Phase 1 gives each agent its own `LlamaContext`
(simplest; `state_seq_get(0, empty())`) or packs several agents as sequences
inside one context (better VRAM reuse; needs `with_n_seq_max`) is left to
Tasks 12–13 — both are reachable through the same two safe methods.

### D1.5 Raw sys symbols are available if ever needed

`llama-cpp-sys-2-0.1.154/build.rs:457-468` runs bindgen over `wrapper.h` with
`.allowlist_function("llama_.*")`, `.allowlist_type("llama_.*")`,
`.allowlist_function("ggml_.*")`, `.allowlist_function("gguf_.*")`.
`wrapper.h` includes `llama.cpp/include/llama.h` and
`llama.cpp/ggml/include/gguf.h`. Therefore **every** `LLAMA_API` function in
the table above — including the current, size-bounded
`llama_state_get_size` / `llama_state_get_data` / `llama_state_set_data` —
is generated as `llama_cpp_sys_2::<name>` and callable through `unsafe`.

### D1.6 Feature `vulkan` exists and is wired

`llama-cpp-2-0.1.154/Cargo.toml.orig` `[features]`:
`vulkan = ["llama-cpp-sys-2/vulkan"]`. Sibling features present:
`cuda`, `rocm`, `metal`, `opencl`, `mkl`, `openmp`, `mtmd`, `llguidance`,
`dynamic-link`, `system-ggml`.
Default features are `["openmp", "android-shared-stdcxx", "common"]`.
`llama-cpp-sys-2-0.1.154/build.rs:882-953` handles the `vulkan` cfg: it links
`vulkan` (`vulkan-1` on Windows), honours `VULKAN_SDK`, and needs `glslc` on
`PATH` for the shader-generation tool. All prerequisites are present on this
box (see "Verification environment").

**Not verified:** an actual end-to-end `cargo build --features vulkan` of
`llama-cpp-sys-2` was **not** performed in this task — it compiles llama.cpp
plus the Vulkan shader set and is Task 11's cost to pay. This record verifies
that the feature exists, the build script wires it, and the toolchain is
installed; it does not claim the build succeeds.

### D1.7 Chat template exposure (feeds D4) — CONFIRMED

```rust
// llama-cpp-2-0.1.154/src/model.rs:734
pub fn chat_template(&self, name: Option<&str>)
    -> Result<LlamaChatTemplate, ChatTemplateError>
//   wraps llama_model_chat_template(model, name_ptr)   [model.rs:744]
//   returns Err(ChatTemplateError::MissingTemplate) when the C call returns NULL

// llama-cpp-2-0.1.154/src/model.rs:935
pub fn apply_chat_template(
    &self,
    tmpl: &LlamaChatTemplate,
    chat: &[LlamaChatMessage],
    add_ass: bool,
) -> Result<String, ApplyChatTemplateError>
//   wraps llama_chat_apply_template(...)               [model.rs:~950]

// llama-cpp-2-0.1.154/src/model.rs:89
LlamaChatMessage::new(role: String, content: String) -> Result<Self, NewLlamaChatMessageError>
```

C side: `llama_model_chat_template` at vendored `llama.h:630`,
`llama_chat_apply_template` at vendored `llama.h:1205`. Both are plain
`LLAMA_API` — no `common` feature required.

---

## D2 — nobody already ships the pager

**Default:** no existing OSS system provides priority-driven paging of
**weights + KV images across multiple agents on one consumer GPU**.

**Verdict: CONFIRMED. No existing OSS system covers ≥80% of Tasks 12–13.**
The plan is **not** amended; the pager gets built. The closest single system is
`llama.cpp`'s own `llama-server` at ~40–45%; the closest *combination*
(`llama-swap` + `llama-server`) reaches ~50–55% as two uncoordinated processes.

Method: breadth-first (~45 min), web docs + `gh` + first-hand reading of the
local `llama.cpp` checkout. Claims below carry citations; claims that could not
be verified are marked **unverified** rather than smoothed over.

### D2.1 Survey

| System | What it pages | Levels | For whom | Covers pager? | Citation |
|---|---|---|---|---|---|
| **vLLM** (PagedAttention, swap, `cpu_offload_gb`, sleep mode, KV Offloading Connector) | KV blocks; separately weights (sleep mode) | KV: VRAM↔CPU DRAM (NVMe = stated future work). Weights: VRAM↔DRAM or discard | Datacenter farm; **one model per engine**; anonymous requests | **~40%** | [sleep mode](https://docs.vllm.ai/en/latest/features/sleep_mode/), [KV offloading connector](https://vllm.ai/blog/2026-01-08-kv-offloading-connector) |
| **SGLang RadixAttention + HiCache** | KV **only** — "exclusively manages KV cache, not model weights" | L1 GPU / L2 host RAM / L3 distributed (Mooncake, 3FS, NIXL, file) | Datacenter, cluster-shared, PD-disaggregated; one model | **~25%** | [HiCache design](https://docs.sglang.io/advanced_features/hicache_design.html) |
| **LMCache** | KV chunks only | GPU / pinned DRAM / local disk incl. NVMe GDS / remote (Redis, Mooncake) | Enterprise vLLM serving; no multi-model, no per-session priority | **~30%** | [architecture](https://docs.lmcache.ai/developer_guide/architecture.html) |
| **Ollama** scheduler + `keep_alive` | **Weights only** (whole runner processes) | VRAM ↔ gone (reload from disk); no RAM tier, no KV persistence | Single consumer box, **many models**, anonymous, no priority | **~30%** | [FAQ](https://docs.ollama.com/faq) |
| **llama.cpp `llama-server`** (slots + `--cache-ram` + router) | KV images per slot **and** whole models (router) | KV: VRAM→RAM (auto, byte-budgeted)→disk (manual REST). Weights: resident ↔ process killed | Single consumer GPU; multi-model via subprocess router; no priority | **~40–45% (closest single system)** | [manpage](https://manpages.debian.org/unstable/llama.cpp-tools/llama-server.1.en.html) + local source, see D2.2 |
| **llama-swap** | Weights, via process start/stop | VRAM ↔ process exit | Single consumer box, many models | **~20%** | [README](https://github.com/mostlygeek/llama-swap) |
| **ServerlessLLM** (OSDI'24) | Model **checkpoints/weights** only | GPU ↔ pinned DRAM ↔ NVMe, loading-optimized format | Serverless cluster | **~30%** | [OSDI'24](https://www.usenix.org/system/files/osdi24-fu.pdf) |
| **mistral.rs** | KV in a GPU-only paged pool; multi-model all co-resident | VRAM only for KV; static weight device-mapping | Rust inference engine, general | **~20%** | [paged attention guide](https://ericlbuehler.github.io/mistral.rs/guides/perf/paged-attention/) |
| **exo** | Nothing — *shards* one model across devices | network/RDMA, not a memory hierarchy | Home multi-device cluster, Apple-Silicon-first | **~5%** | [README](https://github.com/exo-explore/exo) |
| **AIOS** | Agent *semantic* memory; `past_key_values` in a Python dict | in-process dict; no tiering, no eviction, no disk | Research agent-OS framework over LLM APIs | **~10%** | [aios/context/base.py](https://github.com/agiresearch/AIOS/blob/main/aios/context/base.py) |
| **agent-memory** (yshk-mxim) | KV only, Q4 safetensors, LRU block pool | hot GPU / warm metadata / cold disk | **Multi-agent on one box** — but MLX/Apple-only, ~14 stars | **~30%** | [README](https://github.com/yshk-mxim/agent-memory) |

Notes per system, compressed to the load-bearing point:

- **vLLM.** The only surveyed system that offloads *both* weights and KV — sleep
  mode level 1 "offload the model weights and discard the KV cache", level 2
  "discard both". But both act at **whole-engine granularity**, so it can never
  hold agent A resident while evicting agent B; one engine serves one model, and
  multi-model is explicitly "separate instances behind a request router". Its KV
  storage tier is admitted future work, and V1's default preemption mode moved
  from `SWAP` to `RECOMPUTE` — i.e. vLLM has partly *retreated* from KV swapping.
- **SGLang HiCache.** The tier *shape* (L1/L2/L3) is exactly bloomery's and is
  the strongest prior art for the three-level KV hierarchy specifically. But it
  is KV-only by explicit design statement, its unit is a shared token prefix
  rather than an agent's suspendable image, and L3 is a cluster-shared store.
- **LMCache.** The most production-hardened multi-tier KV mover surveyed, and
  it can run as a standalone daemon. KV only, never weights; keyed on prefix
  match rather than agent identity; no priority.
- **Ollama.** Closest on the *weights* half and the only one squarely aimed at a
  consumer box with many models (`OLLAMA_MAX_LOADED_MODELS`, `keep_alive`,
  eviction preferring `refCount == 0` then earliest expiry then LRU). Misses on
  three axes: **no KV persistence at all** (a preempted conversation re-prefills
  from scratch), no RAM tier for weights, and no priority/deadline/budget. The
  `sched.go` internals came from an AI-generated wiki over the source, so
  **line-level details are unverified**; the behaviour is corroborated by the
  official FAQ.
- **exo** solves "this model doesn't fit on one device, add devices"; bloomery
  solves "these agents don't fit on one device, time-share it". Orthogonal.
- **AIOS.** Repository source read: `MemoryManager` is pluggable *semantic*
  memory (retrievable text), `BaseContextManager.gen_snapshot()/gen_recover()`
  are empty stubs, and the only concrete manager stashes `outputs.past_key_values`
  in an in-process dict with no tiering, eviction, disk format, or VRAM
  accounting; shipped schedulers are FIFO and round-robin. **It is an
  orchestration layer, not a pager** — exactly the shape spec §2 says bloomery
  is not. (The paper's claim of swapping + LRU-K eviction was **not** verified
  against the paper body.)
- **Research, no public code.** *TokenCake* ([arXiv 2510.18586](https://arxiv.org/html/2510.18586))
  is the closest published design — agent-aware temporal offload of idle KV
  during tool calls plus a spatial scheduler with a hybrid priority metric
  reserving memory for critical-path agents — but **no public repo**.
  *Continuum* ([arXiv 2511.02230](https://arxiv.org/html/2511.02230v5)) pins KV
  with a TTL derived from reload cost; no public code found.

### D2.2 First-hand correction to the survey: llama.cpp now ships a model router

The web-sourced survey concluded `llama-server` has "no weight paging
whatsoever". **That is out of date.** Reading the local checkout
(`/home/brice/llama.cpp`, commit `4988f6e`, 2026-06-13) directly:

- `tools/server/server-models.cpp` implements a **router** that spawns one
  `llama-server` child subprocess per model.
- `--models-max N` — "for router server, maximum number of models to load
  simultaneously (default: %d, 0 = unlimited)" (`common/arg.cpp:3140-3145`).
- `server_models::unload_lru()` (`server-models.cpp:695`) evicts the
  **least-recently-used** running model when `models_max` is reached, comparing
  `m.second.meta.last_used`.
- `--sleep-idle-seconds` (`common/arg.cpp:3266-3275`) puts an idle child to
  sleep; `handle_sleeping_state()` (`server-context.cpp:844-856`) calls
  `destroy()` on the way in and `load_model()` on the way out.
- Slot KV save/restore is real and uses the same C API bloomery will:
  `llama_state_seq_save_file` / `llama_state_seq_load_file` per slot
  (`server-context.cpp:2349`, `:2388`), dispatched from
  `action == "save"|"restore"|"erase"` (`server-context.cpp:4260-4266`) under
  `--slot-save-path` (`common/arg.cpp:3098`).
- Byte-budgeted RAM tier for idle KV: `-cram/--cache-ram` "set the maximum cache
  size in MiB (default: 8192, -1 - no limit, 0 - disable)" (`common/arg.cpp:1345`)
  and `--cache-idle-slots` "save idle slots to the prompt cache on new task…
  (default: enabled, requires cache-ram)" (`common/arg.cpp:1361-1364`).

So llama.cpp has **both halves** — and this is why it is the closest single
system. It still does not reach 80%, for reasons that are structural rather
than incidental:

1. **The limit is a count, not a memory budget.** `models_max` counts models;
   nothing consults free VRAM. Bloomery's admission is byte-arithmetic (law 1).
2. **The policy is LRU, not priority.** No deadline, no budget, no priority.
   A pinning escape hatch was drafted and left **commented out**
   (`common/arg.cpp:4186-4188`: "in server router mode, do not unload this model
   if models_max is exceeded").
3. **Weight eviction has no RAM or NVMe tier.** Unload kills the child process;
   sleep calls `destroy()` and reloads from disk. Weights go resident → gone,
   never resident → RAM → NVMe. Task 13's warm/cold distinction has no analogue.
4. **The two halves are uncoordinated and mutually destructive.** Killing a
   child to satisfy `models_max` destroys every slot that child owned, KV images
   included. There is no notion of "evict this agent's weights but keep its KV
   image", which is the entire bloomery premise.
5. **None of it is reachable from bloomery anyway.** The router, the slot
   endpoints, and the prompt cache all live in `tools/server/`, not in the
   `libllama` C API that `llama-cpp-2` wraps. Wrapping `llama-server` as a
   subprocess would mean adopting exactly the "shell wrapper over an existing
   serving daemon" shape spec §2 rules out.

### D2.3 The gap is corroborated, not just asserted

A May 2026 characterization study states that existing schedulers "primarily
optimize throughput for a single model", that multi-model deployment is handled
merely by "running separate instances behind a request router", and that
"comparatively little work addresses multi-model scheduling under these
conditions" ([arXiv 2605.19593](https://arxiv.org/html/2605.19593v1)). The field
has cleanly bisected: **KV systems** (HiCache, LMCache, vLLM's connector) build
three-tier hierarchies but refuse to touch weights and key on anonymous prefixes
rather than agent identity; **weight systems** (Ollama, llama-swap,
ServerlessLLM, vLLM sleep mode) move models but discard KV. Nobody unifies both
object types under one priority-driven admission controller on one consumer GPU.

**Directly load-bearing for G2:** that same study reports **model weight reload,
not KV transfer, dominates preemption cost (>98%)**. If it replicates, bloomery's
2 s warm / 5 s cold split is well-chosen — warm switches skip the expensive
term by construction — and Task 17's G2 run should report the two terms
separately so this is measured on our box rather than inherited.

### D2.4 Reuse rather than rebuild

1. **llama.cpp's KV serialization C API — the highest-value reuse, and already
   the plan of record.** `llama_state_seq_*` is a battle-tested per-sequence KV
   image serializer reachable from Rust. **Do not invent a KV image format**;
   wrap it and add bloomery's digest header, accounting, and journaling around
   it. (D1 already selects the safe `state_seq_get`/`state_seq_set` wrappers.)
2. **`--cache-ram` / `--cache-idle-slots` as a behavioural template for L1→L2
   demotion — plus a free bug lesson.** Do **not** copy its accounting.
   [Issue #22629](https://github.com/ggml-org/llama.cpp/issues/22629) (verified
   via `gh issue view`) reports that eviction in
   `server_prompt_cache::alloc()` triggers only on `std::bad_alloc`, which under
   Linux's default overcommit is never thrown — `malloc` returns virtual
   addresses, pages fault in on write, and the OOM killer arrives with SIGKILL,
   bypassing the C++ exception path entirely; the size limit is also checked
   only reactively, after the state is appended. **Task 13 must account KV image
   bytes explicitly and pre-check against the budget before allocating. Never
   infer memory pressure from allocation failure.** This is the same class of
   failure as design law 4: do not trust an implicit signal that can silently
   not arrive.
3. **ServerlessLLM's loading-optimized checkpoint idea** (chunked sequential
   reads into a pinned pool for DMA'd GPU transfer) for the weights→NVMe tier —
   port the idea, do not vendor the Ray/Kubernetes-shaped codebase.
4. **Ollama's eviction ordering** (refCount → earliest expiry → LRU) is the
   shipped consumer-box state of the art in ~40 lines. Treat it as the control
   arm the priority scheduler must beat, not as a dependency.
5. **SGLang's `write_through` / `write_through_selective` / `write_back`
   taxonomy** as vocabulary for tier-promotion knobs — no code, but the policy
   space is already well-named and matching the terms makes bloomery legible.
6. **No eviction-policy crate is worth adding.** Every system surveyed
   hand-rolls LRU, and bloomery's policy is priority/deadline/budget-driven, not
   recency-driven — a generic LRU crate would be the wrong abstraction. The
   allowlist stays as it is.

---

## D3 — `tiny_http` chunked responses for SSE

**Default:** `tiny_http` supports chunked responses adequate for SSE streaming
(Task 15); else fall back to `hyper` (which would be an allowlist change).

**Verdict: CONFIRMED. `tiny_http` 0.12.0 is adequate. No fallback to `hyper`,
no allowlist change.** Verified by reading the crate source *and* by compiling
and running two probe programs against a real socket.

`tiny_http` 0.12.0 is the current `max_version` (published 2022-10-06, per
`https://crates.io/api/v1/crates/tiny_http`) — old but stable, not abandoned
in a way that matters here.

### D3.1 Chunked transfer is selected automatically

`tiny_http-0.12.0/src/response.rs:174-180`: the transfer encoding is chunked
when there is no `Content-Length`, i.e. when `Response::new(..., data_length:
None, ...)` — `entity_length.map_or(true, |val| *val >= chunked_threshold)`
returns `true` for `None`. It is also forced to chunked whenever additional
headers are in play (`response.rs:169-172`). The body is then written through
`chunked_transfer::Encoder` (`response.rs:433-437`).

**Measured (probe 1):** an `Arc<Server>` shared across 4 worker threads — the
exact threading shape Task 14 specifies — compiled and served; a response
built with `data_length: None` from a channel-backed `Read` produced this on
the wire:

```
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Transfer-Encoding: chunked

56
data: {"delta":"tok0"}
...
0
```

Two things are proved at once: `Server` is `Send + Sync` and usable behind an
`Arc` from several threads (`recv(&self)` / `incoming_requests(&self)` take
`&self` — `src/lib.rs:363,380`), and chunked framing is emitted correctly.

### D3.2 The limit Task 15 must know about

In that same probe **all four SSE events arrived inside one 0x56-byte chunk**,
not four chunks. `Response::respond` does
`io::copy(&mut reader, &mut Encoder::new(writer))` (`response.rs:433-437`),
and `chunked_transfer::Encoder::new` is
`with_chunks_size(output, 8192)` (`chunked_transfer-1.5.0/src/encoder.rs:52`)
— an 8192-byte buffer that `io::copy` never flushes mid-stream, and which
`tiny_http` gives no way to shrink. So the high-level `Response` API gives
correct chunked framing but **not real-time incremental delivery**: a
`Read`-driven SSE stream is coalesced until ~8 KiB accumulates or the response
ends.

This is exactly compatible with what Task 15 Step 3 already plans for Phase 1
(buffer the whole reply, emit one delta chunk + usage + `[DONE]`): the payload
is small, the framing is SSE-correct, clients work. **No change to Task 15.**

### D3.3 The escape hatch for real token streaming — measured, works

`Request::into_writer(self) -> Box<dyn Write + Send + 'static>`
(`tiny_http-0.12.0/src/request.rs:390`) hands over the raw stream. Framing the
chunks by hand and calling `flush()` per event gives true incremental
delivery. **Measured (probe 2)**, client-side arrival times:

```
[t+   0ms] HTTP/1.1 200 OK ... Transfer-Encoding: chunked
[t+   0ms] 18\r\ndata: {"delta":"tok0"}\n\n\r\n
[t+ 120ms] 18\r\ndata: {"delta":"tok1"}\n\n\r\n
[t+ 240ms] 18\r\ndata: {"delta":"tok2"}\n\n\r\n
[t+ 360ms] E\r\ndata: [DONE]\n\n\r\n0\r\n\r\n
```

Server-side sleep between events was 120 ms and the client observed exactly
that spacing — per-event flush confirmed.

**Consequence:** when Phase 2 adds a streaming `Substrate::infer`, token-by-token
SSE stays inside `tiny_http` via `into_writer`; `hyper` is not needed then
either. The caveat in `into_writer`'s own docs applies — destroy the writer
promptly, since dropping it releases the next pipelined response.

---

## D4 — chat templating

**Default:** use the model's embedded template via llama.cpp if the binding
exposes it; else the documented fallback is the plain concatenation
`"{role}: {content}\n"` + `"assistant: "`.

**Verdict: CONFIRMED — Task 15 gets the model template as the primary path,
with the fallback genuinely reachable and therefore genuinely required.**
The `X-Bloomery-Template: model|fallback` response header the plan specifies
is not decoration; both branches really occur.

**Primary path.** `LlamaModel::chat_template(None)` (`model.rs:734`) →
`LlamaChatTemplate`; then `apply_chat_template(&tmpl, &messages, /*add_ass=*/ true)`
(`model.rs:935`) → `String`. Pass `add_ass = true` so the prompt ends with the
assistant opening tag (the crate's own doc note, `model.rs:925-928`).
Header value: `model`.

**Two distinct failure modes fall back — both must map to `fallback`:**

1. **No embedded template.** `llama_model_chat_template` returns `NULL` and the
   crate returns `ChatTemplateError::MissingTemplate` (`model.rs:747-748`).

2. **Embedded template present but not understood.** This is the one worth
   flagging: `llama_chat_apply_template` **does not run a Jinja parser**. The
   header says so verbatim — "*NOTE: This function does not use a jinja parser.
   It only support a pre-defined list of template*"
   (`/home/brice/llama.cpp/include/llama.h:1177`; vendored copy line 1197).
   The implementation calls `llm_chat_detect_template(curr_tmpl)` and, on
   `LLM_CHAT_TEMPLATE_UNKNOWN`, **returns `-1`**
   (`/home/brice/llama.cpp/src/llama.cpp:470-490`, verbatim:
   `if (detected_tmpl == LLM_CHAT_TEMPLATE_UNKNOWN) { return -1; }`;
   detector at `/home/brice/llama.cpp/src/llama-chat.cpp:89`, the built-in
   name table starts at `llama-chat.cpp:29` with `chatml`, `llama2*`,
   `mistral-v1/v3/v7*`, `phi3`, `phi4`, `falcon3`, `zephyr`, `monarch`,
   `gemma`, `orion`, `openchat`, `vicuna*`, …). The crate surfaces that as
   `ApplyChatTemplateError`.

   So a GGUF carrying a novel or heavily-customised Jinja template will be
   read successfully by `chat_template()` and then **rejected** by
   `apply_chat_template()`. Task 15 must catch the error from the *apply*
   step, not only from the *fetch* step, or a supported-looking model will
   500 instead of falling back.

**Fallback, as specified:** concatenate `"{role}: {content}\n"` over the
messages, then append `"assistant: "`. Header value: `fallback`.

**Note for Task 15, not a change:** llama.cpp's own built-in template renderer
is a hand-written C++ matcher, not the model's Jinja source. Its output can
differ in whitespace from HuggingFace's `apply_chat_template` for the same
model. That is acceptable for Phase 1 (it is what `llama-server` itself does)
but it is a real source of prompt drift, and it belongs in the journal record
alongside the `model|fallback` decision so a replay can tell which renderer
produced a prompt.

---

## Overturned defaults

**None. All four pre-registered defaults (D1, D2, D3, D4) are CONFIRMED.**
No plan amendment is required, and the dependency allowlist
(`llama-cpp-2` + `llama-cpp-sys-2` w/ `vulkan`, `tiny_http`, `serde`,
`serde_json`, `toml`, `sha2`) is unchanged — `hyper` is **not** added.

Two **corrections within a confirmed default** (D1 stands; the symbols Task 11
calls change):

1. **Task 11 must call `LlamaContext::state_seq_get` / `state_seq_set`
   (safe, size-checked, opaque `SeqState`)** rather than the plan sketch's
   `unsafe` route through `llama_cpp_sys_2::{llama_state_get_size,
   llama_state_get_data, llama_state_set_data}`. The safe pair is recent — it
   is **absent from 0.1.150 and present from 0.1.152** (published 2026-07-21),
   verified by extracting `src/context/session.rs` from the 0.1.150 / 0.1.152 /
   0.1.153 tarballs and grepping for it. So it did exist when the plan was
   written, but was new enough to be easy to miss. Using it removes the
   `unsafe` block from the KV-image path entirely.
2. **The crate's `get_state_size` / `copy_state_data` / `set_state_data` are
   prohibited.** They wrap deprecated, size-unbounded C symbols, and
   `set_state_data` silently drops `src.len()` — a corrupt KV image would be
   an out-of-bounds read in C rather than an error return. If whole-context
   state is ever needed, go through raw `llama_state_set_data(ctx, ptr, len)`.

Findings that **help other tasks** and were not asked for:

- **Task 4** gets `LlamaModel::n_ctx_train()` for law 1's `training_ctx` term,
  and should read back `LlamaContext::n_ctx()` (allocated) rather than trusting
  the requested `n_ctx`.
- **Task 5** can implement the VRAM probe with
  `llama_cpp_2::list_llama_ggml_backend_devices()` (`memory_free` /
  `memory_total` per device) instead of shelling out to `nvidia-smi` — while
  still returning `None`-with-a-reason if no GPU device is enumerated.
- **Task 13** gets its KV-image invalidation mechanism for free:
  `StateSeqError::SizeMismatch` from `state_seq_set` is llama.cpp's own
  deserializer rejecting a shape mismatch, and per spec §9 must be handled as
  a cold start, never as an error.
- **Task 13 must pre-check the KV-image byte budget before allocating.**
  llama.cpp issue #22629 (D2.4) is a shipped instance of the opposite: eviction
  gated on `std::bad_alloc`, which Linux overcommit guarantees never fires, so
  the documented `--cache-ram` cap is silently unenforced until the OOM killer
  arrives. Never infer memory pressure from allocation failure.
- **Task 15** must catch the chat-template failure at the *apply* step, not
  only at the *fetch* step (D4) — otherwise a model with a novel Jinja template
  500s instead of falling back.
- **Task 17 (G2)** should report warm and cold switch costs as two separate
  terms: the literature claims weight reload, not KV transfer, dominates
  preemption cost (>98%), and that is worth measuring on our box rather than
  inheriting (D2.3).

Explicitly **unverified** items, recorded so they are not mistaken for verified:

- A real `cargo build --features vulkan` of `llama-cpp-sys-2` was **not** run.
  Feature wiring and host toolchain are confirmed; build success is Task 11's
  to establish.
- Ollama's `sched.go` internals came from an AI-generated wiki over the source,
  corroborated only by the official FAQ. Behaviour is likely right; line-level
  details are not verified.
- The AIOS *paper*'s claim of swapping and LRU-K eviction was not checked
  against the paper body. The AIOS *repository* was read and contains no pager;
  that part is verified.
- The >98% weight-reload-dominates figure is a single cited study
  ([arXiv 2605.19593](https://arxiv.org/html/2605.19593v1)), not our own
  measurement. It is used to shape what G2 reports, never as a result.
