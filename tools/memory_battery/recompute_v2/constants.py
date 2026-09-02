"""The locked constants recompute-v2 measures against.

Split out of `recompute_v2.py` on 2026-09-01 (carried-debt slice D) into their
own module rather than left in the package root: `arms.py` and `endpoints.py`
both need them, and importing from the root would be circular. Keeping them
here also gives the seed-drift mutation checks one unambiguous file.
"""

from __future__ import annotations
import argparse
import json
import random
import statistics
import sys
from pathlib import Path
from typing import Any, Sequence
from tools.memory_battery.recompute import _corpus_sha
from tools.memory_battery.recompute_bootstrap import (
    HYGIENE_SE_MULTIPLIER,
    INFRA_RATE_CEILING,
    _bootstrap_diff_independent,
    _check_arm_completeness,
    _check_identity,
)
from tools.memory_battery.recompute_join import _load_arm
from tools.memory_battery.recompute_journal import (
    _index_memory_stamps,
    _read_ledger,
    _task_step_duration_by_agent,
)


# Design spec §6/§4 (locked numbers, task-1 brief): seed 20260828, B=10,000
# -- DELIBERATELY DIFFERENT from `recompute_bootstrap.BOOTSTRAP_SEED`
# (20260826, v1's own lock). Module-level constants so mutation check #5
# ("seed drifts -- any literal") has one unambiguous line to mutate.
SEED_V2 = 20260828

B_V2 = 10_000


# Design spec §5: honest v2 arm labels -- never v1's "C"/"M".
ARM_LABEL_M_PRIME = "m_prime"

ARM_LABEL_R = "r"


# v1's arm labels, unconditionally forbidden in v2's ledgers regardless of
# what `expected_arm_labels` a caller passes (see `_check_arm_labels`).
FORBIDDEN_ARM_LABELS = frozenset({"C", "M"})


# Refalsify spellings design spec §4 names by name: the two LIVE v2
# spellings ("premise_held", "premise_gone"), the two named-zero/tolerated
# ones ("inconclusive", "skipped_ungranted"), and the two RETIRED v1
# spellings that must appear nowhere under a v2 build ("passed", "failed").
FORBIDDEN_REFALSIFY_SPELLINGS = frozenset({"passed", "failed"})

NO_PROBE_SPELLING_KEY = "none"  # JSON-safe stand-in for a `None` refalsify.


# Design spec §4, stamp audit, quoted verbatim: "inconclusive (probe
# timeout/spawn) and skipped_ungranted expected 0; tolerated within H3's
# infra budget, counted and named individually." Review finding
# IMPORTANT-2: these two spellings are TOLERATED on an injected stamp --
# counted, but never themselves an `premise_held_complete` violation (only
# a genuinely offending spelling -- premise_gone, a forbidden v1 spelling,
# or an unexpected None -- counts as offending).
TOLERATED_NON_PREMISE_HELD_SPELLINGS = frozenset({"inconclusive", "skipped_ungranted"})
