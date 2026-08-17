"""The G5 refusal template registry (design doc §5) — the refusal-family
analog of `templates.py`'s role for repair templates: re-exports the four
(family, lens) groups from `templates_refusal_python.py` and
`templates_refusal_text.py` (kept in separate files, mirroring
`templates_python.py`/`templates_text.py`'s split, so each stays under the
400-line house cap) as one flat, name-sorted registry.

Two refusal families (design doc §5), both lenses:
- defect-absent: the generated file is genuinely correct; the goal claims a
  plausible-but-false defect, mechanically anchored by a backtick-quoted
  identifier/value that is a real substring of the file
  (`task.validate_refusal_task`'s plausibility rule).
- missing-target: the goal names a file NOT among the fixture's files, with
  >= 1 real sibling file present.
"""

from __future__ import annotations

import random
from typing import Callable

from tools.flywheel.factory import templates_refusal_python, templates_refusal_text
from tools.flywheel.factory.task import RefusalTask

RefusalTemplateFn = Callable[[random.Random], RefusalTask]

# The four (family, lens) groups, each sorted by name for a stable cycling
# order (same reasoning as `templates.PYTHON_TEMPLATES`/`TEXT_TEMPLATES`:
# generate.py's family cycling must not depend on dict/definition order).
GROUPS: dict[str, tuple[tuple[str, RefusalTemplateFn], ...]] = {
    "defect_absent_python": tuple(sorted(templates_refusal_python.DEFECT_ABSENT_FAMILIES.items())),
    "defect_absent_plaintext": tuple(sorted(templates_refusal_text.DEFECT_ABSENT_FAMILIES.items())),
    "missing_target_python": tuple(sorted(templates_refusal_python.MISSING_TARGET_FAMILIES.items())),
    "missing_target_plaintext": tuple(sorted(templates_refusal_text.MISSING_TARGET_FAMILIES.items())),
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
# round-robins the four groups so a run of N refusal tasks splits as evenly
# as possible across both families and both lenses (G5 design doc §5: "both
# lenses in both classes"), independent of how many template variants each
# group happens to carry.
GROUP_CYCLE_ORDER: tuple[str, ...] = (
    "defect_absent_python",
    "defect_absent_plaintext",
    "missing_target_python",
    "missing_target_plaintext",
)
