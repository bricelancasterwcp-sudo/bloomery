# Flywheel turn 5 pre-registration — amendment 1

Amends: `docs/superpowers/evidence/2026-08-22-flywheel5-preregistration.md`
(commit `84e5a57`). Per that document's own amendment rule ("any amendment
to this pre-registration after this commit is a **separate dated file** in
`docs/superpowers/evidence/`, cross-linked from here by a later commit, and
**never** an in-place edit of this document"), this is that separate dated
file. Full run facts are in
`docs/superpowers/evidence/2026-08-23-flywheel5-training.md`; this file
records only the two assumptions that changed and their cost-bound
consequences.

## 1. Cloud type and rate

**Pre-registered**: RunPod A100-SXM4-80GB, `US-WA-1`, COMMUNITY cloud,
`$1.39/h` (the rate the Task-8 pod ledger's datacenter probe found, `Low`
stock flagged as "not guaranteed to hold" and to be "re-checked at pod-cut
time").

**Actual**: `POST /v1/pods` with `cloudType:"COMMUNITY"` returned `{"error":
"create pod: There are no instances currently available","status":500}` on
**both** cut attempts (pod 1, 2026-08-22T23:42:59Z; pod 2,
2026-08-23T05:07:25Z — ≈9 hours apart), each time falling back to
`cloudType:"SECURE"` per the runbook's own documented fallback. SECURE
succeeded both times at **`$1.59/h`** (+14.4% vs the pre-registered rate).

This is a datacenter-availability fact, observed twice independently, not a
retry-until-cheaper search and not a recipe change — storage/cloud-tier
selection was never part of the training recipe (LoRA config, seeds,
corpus, hyperparameters), and the fallback path itself was already
pre-registered in the runbook.

**Revised cost bounds** (recomputed at $1.59/h, ×1.144 vs the $1.39/h table):

| step | pre-registered upper bound ($1.39/h) | recomputed upper bound ($1.59/h) | actual |
|---|---|---|---|
| upload | ≈$0.8 | ≈$0.9 | $0 (moved off the pod entirely, §2) |
| train | ≈$5.1 | ≈$5.8 | $5.86 (train+post-train+download combined on pod 2) |
| evals | ≈$0.5 | ≈$0.6 | n/a (battery not yet run — Task 11) |
| post-train | ≈$0.5 | ≈$0.6 | included above |
| download | ≈$0.3 | ≈$0.3 | included above |
| **total** | **≈$7.2** | **≈$8.2** | **$6.32** (pod 1 $0.46 + pod 2 $5.86) |

Actual spend came in under the recomputed upper bound, still comfortably
inside the $10 turn cap.

## 2. Upload path

**Pre-registered**: base model uploaded once, via the pod's own SSH path
(`split` + parallel `scp` per the brief's Step 2, or the task's own
dd-over-ssh variant), at an assumed ≈19 MB/s aggregate (a **download**
figure quoted from the pod's own `maxDownloadSpeedMbps: 21623` /
`maxUploadSpeedMbps: 1832` machine spec — i.e., the pod's fast downlink, not
this local box's uplink).

**Actual**: pod 1's SSH-path upload attempt measured **≈2.665 MB/s
aggregate (≈21.3 Mbps)** across 6 parallel streams, flat with no slow-start
ramp. Root-caused via `/proc/net/dev` on the local box: the 6-stream
aggregate matched the box's total outbound interface TX rate almost
exactly, with no competing process (no I2P/qBittorrent/torrent activity) —
i.e., this **is** the local box's own outbound uplink ceiling, not a
pod-side or transfer-method artifact. **The pre-registered plan's ≈19 MB/s
figure was a download-speed number misapplied to the upload direction** —
the actual measured **uplink** is ≈2.3-2.7 MB/s, ≈7-8x slower.

Projected at the pod-path rate: full 38.35 GB upload ≈4h, ≈$6.36 at the
SECURE rate — more than half the $10 cap before training even started.
**Deviation**: the base was instead uploaded via RunPod's **S3-compatible
API** (`https://s3api-us-wa-1.runpod.io/`) directly to the network volume,
**with no pod running** — 4,572 × 8 MiB multipart parts, 2 concurrent
threads, ≈2.3 MB/s average (consistent with the same uplink ceiling,
confirming it is a property of this box, not the transfer method), ≈4h22m
wall time, **$0 POD cost** — but **not $0 account cost** (see below) —
since no billed pod was active during the transfer. Verified correct on the
pod afterward: exact byte size (38,349,435,696) and sha256 match (§3 of the
training-record file).

**Account-balance drawdown during the upload window, not fully explained.**
Re-hashing the ledger's own balance readings: a **baseline** quiet-window
storage trickle (prereg-time → pod-1 cut, ≈3.99 h, no upload active) of
$0.019444 (≈$0.00487/h, consistent with ≈$3.50/mo at 50 GB) versus the
**S3-upload window**'s drawdown (pod-1 teardown $12.4848233407 ~00:02Z →
pod-2 pre-cut $12.4074403658 ~05:06Z, a ≈5.06 h window almost entirely
coincident with the upload's active span 00:36→04:58Z) of $0.077383
(≈$0.0153/h) — **≈3.1× the baseline rate**. This is real, measured account
cost during the "no pod running" window, distinct from and additional to
pod-billing cost. **Hypothesis, flagged unverified**: an in-progress
multipart upload's already-received parts may occupy volume storage
(up to ≈38 GB beyond the 50 GB nominal volume) until the multipart upload
completes, roughly doubling the billed storage footprint for the window;
per-part `PutObject` API operation charges (≈4,572 calls) may also
contribute. Neither is confirmed against RunPod's actual billing model —
recorded as an open, unattributed cost, not a closed explanation.

This is an infrastructure/transport change, not a recipe change: the same
bytes landed on the same volume, verified byte-identical by sha256 before
any training step. Nothing about the corpus, seeds, hyperparameters, or
battery was touched. The small unattributed account-cost drawdown above
does not change this — it is a storage/billing fact about the transport
method, not a training-recipe fact.

## 3. Pod 1's cost ($0.46), recorded here for completeness

Pod 1 (`7al24l12yuhaqs`) ran ≈17-19 minutes before the SSH-path upload's
infeasibility was discovered and the stop rule invoked. Spend:
**$0.4605308916**, included in this turn's cost total (§10, and §1, of the
training-record file). This was not wasted exploration — it is the
measurement that motivated the S3-path switch in §2 above.

## 4. What is UNCHANGED

The **recipe** (LoRA r16/alpha32 on the twelve named target modules,
experts+router frozen, `TrainingArguments` per `train_common.PINNED_ARGS`,
`torch.manual_seed(20260816)`, unpacked batch size 1, completion-only loss,
no EOS), the **seeds** (`20260816` on both the LoRA-init and
`TrainingArguments.seed`), the **corpus** (`~/flywheel4/corpus.jsonl`
copied byte-identical, sha256
`9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`,
re-verified before and after training with no change), and the **battery**
(G4 on `codec-tasks-v1` ≥16/20, G5 on `codec-tasks-v4-mixed` ≥13/16 per
class, kill condition G4<16/20 OR refuse<8/16, two boots, boot 1 anchor —
all exactly as pre-registered) are **all unchanged** by anything in this
amendment. This amendment touches only the pod-cost/cloud-rate assumption
(§1) and the upload transport (§2) — infrastructure facts discovered while
executing the pre-registered procedure, not adjustments to the procedure
itself. The full training run, post-train chain, and sha chain are recorded
in `docs/superpowers/evidence/2026-08-23-flywheel5-training.md`; the
battery has not yet run (Task 11).
