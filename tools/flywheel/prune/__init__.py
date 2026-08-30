# bloomery — an operating layer for local LLMs.
# Copyright (C) 2026 Brice Lancaster
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU Affero General Public License, version 3, as
# published by the Free Software Foundation.
#
# This program is distributed in the hope that it will be useful, but WITHOUT
# ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
# FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
# for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program. If not, see <https://www.gnu.org/licenses/>.
#
# Commercial licensing is available as an alternative to the AGPL — see
# LICENSING.md.

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


class PruneConfigurationError(ValueError):
    """A prune that would produce an unusable model was refused.

    Raised before any calibration, surgery or write happens — the caller
    gets a named error and an untouched output directory, not a corrupt
    checkpoint discovered at load time.
    """
