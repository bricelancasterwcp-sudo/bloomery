"""The G5 refusal template registry (design doc §5; turn-3 design doc §2
adds the third family) — the refusal-family analog of `templates.py`'s
role for repair templates: re-exports the six (family, lens) groups from
`templates_refusal_python.py`, `templates_refusal_text.py`,
`templates_symptom_mismatch_python.py` and
`templates_symptom_mismatch_text.py` (kept in separate files, mirroring
`templates_python.py`/`templates_text.py`'s split, so each stays under the
400-line house cap) as one flat, name-sorted registry.

Three refusal families, both lenses:
- defect-absent: the generated file is genuinely correct; the goal claims a
  plausible-but-false defect, mechanically anchored by a backtick-quoted
  identifier/value that is a real substring of the file
  (`task.validate_refusal_task`'s plausibility rule).
- missing-target: the goal names a file NOT among the fixture's files, with
  >= 1 real sibling file present.
- symptom-mismatch (turn 3): the file carries a REAL defect Y and the goal
  reports a DIFFERENT, absent defect X — same structural shape as
  defect-absent (target present, plausibility rule enforced), but the
  ideal names what IS there instead of simply declining. The X-false and
  Y-present ground truth lives in the templates and is proven in
  `test_templates_symptom_mismatch.py`.
"""

from __future__ import annotations

import random
from typing import Callable

from tools.flywheel.factory import (
    templates_refusal_python,
    templates_refusal_text,
    templates_symptom_mismatch_python,
    templates_symptom_mismatch_text,
)
from tools.flywheel.factory.task import RefusalTask

RefusalTemplateFn = Callable[[random.Random], RefusalTask]

# The six (family, lens) groups, each sorted by name for a stable cycling
# order (same reasoning as `templates.PYTHON_TEMPLATES`/`TEXT_TEMPLATES`:
# generate.py's family cycling must not depend on dict/definition order).
GROUPS: dict[str, tuple[tuple[str, RefusalTemplateFn], ...]] = {
    "defect_absent_python": tuple(sorted(templates_refusal_python.DEFECT_ABSENT_FAMILIES.items())),
    "defect_absent_plaintext": tuple(sorted(templates_refusal_text.DEFECT_ABSENT_FAMILIES.items())),
    "missing_target_python": tuple(sorted(templates_refusal_python.MISSING_TARGET_FAMILIES.items())),
    "missing_target_plaintext": tuple(sorted(templates_refusal_text.MISSING_TARGET_FAMILIES.items())),
    "symptom_mismatch_python": tuple(sorted(templates_symptom_mismatch_python.SYMPTOM_MISMATCH_FAMILIES.items())),
    "symptom_mismatch_plaintext": tuple(sorted(templates_symptom_mismatch_text.SYMPTOM_MISMATCH_FAMILIES.items())),
}

# Flat union of every group, sorted by name — for callers (tests, hygiene
# checks) that want "every refusal template" without caring which group it
# came from.
ALL_REFUSAL_TEMPLATES: tuple[tuple[str, RefusalTemplateFn], ...] = tuple(
    sorted(
        (name, fn)
        for group in GROUPS.values()
        for name, fn in group
    )
)

# The fixed cycling order `generate.py` walks when drawing refusal tasks:
# round-robins the six groups so a run of N refusal tasks splits as evenly
# as possible across all three families and both lenses (G5 design doc §5:
# "both lenses in both classes"), independent of how many template variants
# each group happens to carry. The two turn-3 groups are appended rather
# than interleaved so the existing four keep their relative order — the
# per-family split is set by this tuple's CONTENT, not its arrangement.
GROUP_CYCLE_ORDER: tuple[str, ...] = (
    "defect_absent_python",
    "defect_absent_plaintext",
    "missing_target_python",
    "missing_target_plaintext",
    "symptom_mismatch_python",
    "symptom_mismatch_plaintext",
)
