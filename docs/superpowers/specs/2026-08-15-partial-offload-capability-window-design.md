# Partial offload + the G4 capability window (qwen3:14b, qwen3.8:27b)

**Date:** 2026-08-15
**Status:** Approved in conversation (design presented in sections, Brice
approved). Parent: `2026-08-14-phase2-os-surface-design.md` (gate G4, now
merged and live per `docs/superpowers/evidence/2026-08-15-g4-codec-landing.md`);
umbrella laws govern.

## 1. What this builds and why

G4's first live verdict demoted qwen2.5-coder-7B (0/20, non-provisional).
The recorded escalation is the **capability window**: does a bigger model
clear the ≥80% landing floor under the same envelope? Two rungs:

- **qwen3:14b** (9.3 GB GGUF) — fits the card fully. Zero code changes;
  run the existing instrument as-is. This rung goes first.
- **qwen3.8:27b** (15.65 GiB main GGUF) — does not fit alongside overhead
  on the 16 GB card. Today it cannot even boot: `DEFAULT_N_GPU_LAYERS =
  u32::MAX` (full offload, not config-wired) and admission charges **full**
  `weights_bytes` against the static VRAM budget, so the pager refuses it
  outright. This rung needs the small enablement below.

**The G4 protocol needs NO amendment for either run.** It is per-model and
pins the instrument (fixture set, scoring, decision rule) — not the
subject. The pinned parameters are untouched.

## 2. Config: per-model tuning

A `models` entry accepts **either** shape (serde untagged):

```toml
[models]
# today's shape — unchanged, still valid
"qwen3:14b" = "/mnt/extra/ollama-models/blobs/sha256-…"

# new shape — per-model tuning
[models."qwen3.8:27b"]
path = "/mnt/extra/ollama-models/blobs/sha256-f5f1dd89…"
n_gpu_layers = 28          # optional; omitted = full offload
weights_vram_mib = 11264   # optional; omitted = charge full weights
```

Both new fields optional; omitting both is byte-for-byte today's behavior.
Every existing config keeps parsing.

## 3. Pager accounting: the declared weights-VRAM charge

- The model entry carries `weights_vram_bytes: Option<u64>` from config.
- **One value, both places**: the effective weights charge
  `effective_weights = min(declared, weights_bytes)` (declared absent →
  `weights_bytes`) is used by BOTH the placement budget
  (`avail = budget − overhead − Σ effective_weights − Σ reservations`)
  AND the window law's VRAM term. One number in two formulas — this must
  not add a new asymmetry on top of carried-debt item 7.
- Refusal arithmetic prints the effective value and **names it declared**
  (e.g. `weights 11264 MiB (declared weights_vram_mib; file 16031 MiB)`) —
  a declared number must never read as a measured one.
- No override = full charge = today's fail-closed behavior.
- **KV stays fully charged to VRAM under partial offload.** llama.cpp
  places KV for CPU-resident layers in host RAM, so full-charge overcounts
  VRAM need — conservative (smaller windows, earlier refusals), never an
  OOM direction. Recorded as an honest limit (README + carried debt), not
  changed in this slice.

## 4. Substrate plumbing

`Substrate::load_model` already takes `n_gpu_layers` per call. The pager
keeps its global default (`set_n_gpu_layers`, `u32::MAX`); a per-model
override from config wins when present. One plumbing change, no substrate
API change.

## 5. Deriving the declared numbers (the ctx_overhead_mib precedent)

Configured, not measured per-run — measured **once**, declared with
headroom, derivation committed:

1. Load the 27B once at the chosen `n_gpu_layers` (a boot with
   `allow_unprofiled` on a scratch data_dir is fine).
2. Read llama.cpp's own buffer-size log lines and the nvidia-smi delta.
3. Declare `weights_vram_mib` with headroom above the observed number.
4. Commit the log excerpt as evidence beside the run (exactly like the
   384 MiB ctx_overhead derivation,
   `2026-08-14-2a-daemon-log-excerpt.txt`).

Setting it too low is an OOM, not a refusal — the doc comment says so, the
README honest-limits section says so.

## 6. The two live runs (lens notes)

- Same instrument, same protocol, same greedy sampler, same
  raw-completion prompt path (no chat template) — the lens every model
  gets. qwen3-family thinking behavior under this envelope is measured
  **as-is** and stated in each evidence doc; a chat-template or
  thinking-toggle serving change would be a different lens and is out of
  scope.
- Each run: GPU preflight (free VRAM floor, no in-flight runs), fresh
  data_dir, assay pinned by commit for the boot POST (the assay working
  tree is mid-v1.5 development — pin and record, as the first G4 run did).
- The 27B probe at partial offload will be slow (CPU-bound layers). If the
  small-N extrapolation projects >2 h, OS-detach the daemon
  (`setsid nohup`) with a pid file and completion marker per the long-run
  discipline; never leave it harness-tracked only.
- Each run ships its own evidence doc + committed journals, with the
  recomputability check (fixture rows vs verdict).

## 7. Testing posture

GPU-free (`cargo test --workspace`), the P4 habits:

- Config: both entry shapes parse; both fields optional; existing configs
  unchanged (backward-compat test).
- Accounting: the declared value reaches BOTH the placement budget and the
  window-law term (asymmetric test values so a one-sided wiring fails);
  `min(declared, full)` clamp; refusal string names "declared";
  no-override = full charge. Mutation-pin the both-places property.
- Plumbing: per-model `n_gpu_layers` reaches the substrate
  (FakeSubstrate records the value); global default still wins when the
  override is absent.

## 8. Non-goals

- No auto-tuning of `n_gpu_layers` or the declared MiB.
- No live VRAM reads (standing ruling — static boot budget).
- No vision/mmproj loading (the 27B's 931 MB projector blob is ignored).
- No chat templates / thinking toggles (a different lens, a later
  decision).
- No per-layer weight computation (layers aren't uniform; an undercount
  is an OOM — the 2a lesson).

## 9. Deliverable order

1. Run **qwen3:14b** through the existing gate (no code) — first
   capability-window data point, its own evidence doc.
2. Plan + execute the enablement (config + accounting + plumbing + tests).
3. Derive the 27B's declared numbers (one measured load, committed).
4. Run **qwen3.8:27b** through the gate — second data point, its own
   evidence doc.
