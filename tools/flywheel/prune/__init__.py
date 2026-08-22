"""tools.flywheel.prune — a REAP-compatible expert pruner for `qwen3_5_moe`.

Upstream REAP (CerebrasResearch/reap@1970473c, 2026-04-17) cannot touch
Qwen3.5/3.6 MoE: it pins `transformers==4.55.0` (which has no
`qwen3_5_moe`), has no `MODEL_ATTRS` / `OBSERVER_CONFIGS` entry for
`Qwen3_5Moe*`, walks `model.model.layers` (wrong under the multimodal
wrapper), calls `module.router(x)` / `module.experts(x)` with an API this
block does not have, and expects router logits the block never returns.
Five blockers, all verified in
`.superpowers/spikes/2026-08-21-runpod-reap-train-spike.md` §S4.

What this package keeps from REAP is the **saliency math**; what it
replaces is the observer and the pruned-config/save path. Every formula is
cited against `src/reap/pruning_metrics.py` at that commit.

Modules
    blocks.py     find the MoE blocks by class, force the eager kernel
    observer.py   ExpertSaliencyObserver — hooks, per-expert statistics
    saliency.py   the REAP formulas + the keep-count / selection rule
    prune.py      tensor surgery, config rewrite, checkpoint save
    calib.py      calibration corpus loading and the calibration pass
    cli.py        `python -m tools.flywheel.prune.cli`

Requires torch + transformers>=5.5 (use `~/flywheel-venv/bin/python`).
"""

REAP_UPSTREAM = "CerebrasResearch/reap@1970473c51ca3caeb98c10392f15b3a08a672974"
