# Flywheel turn 7 — training record (rental run, sha chain, costs)

Governs: `2026-08-29-flywheel7-preregistration.md` (commit `62dc546`, the
floor lock). **No fixture, floor, endpoint, seed, corpus, or recipe
parameter was changed after a number was seen.** Deviations below are
infrastructure facts, recorded verbatim.

Turn cap **$10**; our spend, attributed by pod-hours × rate (the balance
series is polluted by a concurrent pod and a mid-run top-up, §4):
pod 1 ≈0.08 h × $1.59 ≈ **$0.13** + pod 2 ≈4.13 h × $1.59 ≈ **$6.57** →
≈ **$6.70** — within cap, matching the pre-registered worst case ($6.7).

## 1. Pod ledger

| | pod 1 (terminated) | pod 2 (this run) |
|---|---|---|
| id | `0cxnlfwgm046m4` | `43k7ws226ollg9` |
| GPU / DC / cloud | A100-SXM4-80GB / US-WA-1 / SECURE $1.59/h | same (same machine `2kbys5tpjs02` as turn 5's pod 2) |
| cut (UTC) | 14:34:50Z | ≈14:37Z |
| end (UTC) | ≈14:39Z (terminated) | 18:44Z (torn down, verified) |
| outcome | **KEYLESS**: the account's `pubKey` is empty (turn 5's keys were per-pod env, not account state — a fact the turn-5 record did not carry); both local keys refused. Terminated at discovery. | **Full success**: env, sha verify, smoke, train, post-train, download, teardown. Re-created with `env.PUBLIC_KEY` = `~/.ssh/runpod_spike.pub`. |

Preflight balance $10.5622887341; volume `s8qomynzbd` intact in US-WA-1
(base 36 G, turn-5 llama.cpp @ `8672290` prebuilt, bloomery clone, 2.1 G
`parts/` upload debris left in place; ≈10 G headroom) → **$0 upload**. A
concurrent pod (`oxide-v04-wave2`, $0.74/h, another session's) ran the
whole window and was never touched.

## 2. Environment and pre-run verification

`env-setup.sh` ran the turn-5 chain with its recorded deviations BAKED IN:
the convert-requirements clobber was pre-empted by re-pinning
`torch==2.9.1+cu129` and `transformers==5.5.0` after that install; final
version string **`2.9.1+cu129 5.5.0 0.20.0 True`** — exact. bloomery at
**`62dc546`** exactly; llama.cpp at `8672290`; `pip freeze` at
`~/flywheel7/flywheel7-pip-freeze.txt`. Corpus + fingerprint scp'd via
`/tmp`, sha-verified on arrival, then moved into `/workspace/flywheel7/`.
Pre-run, detached: base `model.safetensors` →
`8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b` and
corpus → `08c0bc6d…` — **both exact; the STOP rule was not triggered**
(`~/flywheel7/shas-pre.txt`).

## 3. Smoke (`--max-steps 5`) — PASS, and the pace stop-rule

Exit 0. `Qwen3_5MoeForCausalLM; num_experts=133; layers=40`; **trainable
21,166,080 / 19,194,718,848 (0.1103%) — experts+router frozen, asserted**
(the identical fingerprint to turn 5); `pairs: train=4330 val=233` —
exact match to the pre-registration; label-check ok (536 prompt tokens
masked — v5 prompts are longer than v4's 341, the declared card at work;
tail `\n</action>`); eval_loss 0.3304; 5-step train_loss 0.6693. **Peak
VRAM 51,337 MiB, artifact-backed** (`~/flywheel7/smoke-vram.log` — the
turn-5 §5 provenance gap, closed). Smoke scratch on the container disk,
deleted before the full run.

**Stop-rule check (pre-registered):** the smoke's own 5-step figure
carries warmup, so the projection used the full run's measured steady
state — **9.03–9.07 s/optimizer-step at step 97/1084**, tqdm ETA 2:28:30
→ projected turn total ≈ $5.7 < $10 cap → run continued. (Turn 5's full
run paced 7–10 s/step; same shape.)

## 4. Training run — COMPLETE

Launched 14:47:10Z via `setsid nohup train-wrapper.sh`; `train.DONE` +
`train.EXIT = 0` at ≈17:43Z. From `trainer_state.json` (committed with
the adapter artifacts at `~/flywheel7/`): **epoch 2.0, global_step 1084,
train_runtime 10,573.501 s (2.937 h), train_loss 0.012006**. Eval curve
(all points in `trainer_state.json`): 0.007249 @ 100 → 0.000502 @ 300 →
flat ≈0.00028 from ≈step 900 (0.000281 final). **Eval loss is monitored,
never a gate** (the standing stance); the battery is the sole decision
instrument. Adapter `adapter_model.safetensors` **84,751,528 B** — the
same byte size as fw5's (same LoRA geometry).

Hourly monitor readings during the run (balance shared with the
concurrent pod at ≈$2.33/h combined): $10.11 → $9.52 (step ~100) →
$7.13 (464/1084) → $4.75 (829/1084). **A top-up landed mid-run**
(post-teardown balance $11.1741958735 exceeds the last reading) — the
balance series is therefore not usable for spend attribution; pod-hours ×
rate is (header).

## 5. Post-train chain — first-try success

`posttrain.EXIT = 0`. Merge (peft `merge_and_unload`, CPU, bf16) 18:03:35Z
→ `merged ok`; convert `--outtype bf16 --no-mtp` (turn-5 deviation 5
baked in — **no failed first attempt this time**) → 38,382,368,832 B
bf16 GGUF; `llama-quantize Q4_K_M` → `quant size = 11200.56 MiB
(4.90 BPW)`, **11,755,624,192 B** — byte-identical size to fw5's Q4_K_M.
**`qwen35moe.block_count = 40`** (the 41-is-STOP rule untriggered);
**`nextn`/`mtp` keys: none** (GGUFReader over all kv fields). Scratch on
the container disk (`/root/flywheel7-scratch/`, died with the pod); the
adapter on the volume.

sha256 chain (pod == home, every artifact):

```
f049f13722f037b82e4260a7fdd2f7543a38fe41ea890cf01cb26e5c8748491c  adapter_model.safetensors
b392481216b7183c76b987b6d462c2eb312a14421c124f5c2d120636b4a5457f  qwen36-reap48-flywheel7-Q4_K_M.gguf
08c0bc6d8bffbd4051d2c8ebd9c33d8d0d4ce34433062ad047dc29602cfb864d  corpus.jsonl (re-verified post-train, unchanged)
```

## 6. Bring it home, teardown

Small artifacts by scp (adapter files, trainer_state, all logs, pip
freeze, shas-pre). GGUF by 6 parallel byte-range `dd` streams over ssh
(the turn-5 method); the local reassembly `cat` was killed by the session
harness's background-task lifetime with all six parts complete
(byte-sums exact) — re-assembled incrementally (99%-full local disk:
each part deleted as consumed), **local sha `b3924812…` = pod sha,
exact**. `~/flywheel7/SHAS.txt` carries the chain. Teardown: `DELETE`
→ 204; verified empty via both REST and GraphQL at 18:44Z (only the
other session's pod remains); volume left intact (now also holds the
turn-7 corpus + adapter). Balance after: $11.1741958735.

## 7. Deviations from the runbook, listed together

1. **Pod 1 keyless** (§1): account `pubKey` empty; fixed by per-pod
   `env.PUBLIC_KEY`; ≈$0.13. The runbook now knows: RunPod ssh keys on
   this account are per-pod env, never account state.
2. **GGUF reassembly interrupted** (§6): harness background-task
   lifetime, not a transfer failure; artifact integrity proven by sha.
3. Everything else ran the turn-5 recipe verbatim with its five recorded
   deviations baked in from the start — none of them recurred as
   failures (the clobber was pre-empted; `--no-mtp` was first-try; the
   scratch went to the container disk by design; SECURE pricing was
   assumed in the cost bound).

No fixture, floor, endpoint, seed, corpus, or recipe parameter changed
at any point.
