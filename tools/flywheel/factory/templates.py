"""The task factory's public surface: `Task`, structural validation (brief
rule 2), and the registry of template families (brief rule 1).

Template families themselves live in `templates_python.py` (>= 8 python
families) and `templates_text.py` (>= 5 plaintext families) to keep each
module under the 400-line budget; this module re-exports them.
"""

from __future__ import annotations

import random
from typing import Callable

from tools.flywheel.factory import templates_python, templates_refusal, templates_text
from tools.flywheel.factory.contamination import GATE_VOCABULARY
from tools.flywheel.factory.task import (
    DONE_INSTRUCTION,
    RefusalTask,
    Task,
    validate_refusal_task,
    validate_task,
)
from tools.flywheel.factory.wordlists import all_wordlist_tokens

__all__ = [
    "DONE_INSTRUCTION",
    "Task",
    "RefusalTask",
    "validate_task",
    "validate_refusal_task",
    "PYTHON_TEMPLATES",
    "TEXT_TEMPLATES",
    "REFUSAL_TEMPLATES",
    "REFUSAL_GROUPS",
    "ALL_TEMPLATE_WORDS",
]

TemplateFn = Callable[[random.Random], Task]

# Rule 1: >= 8 python families, >= 5 plaintext families. Sorted by name so
# generate.py's family cycling order is stable regardless of how these
# modules happen to define them (no dict/definition-order reliance).
PYTHON_TEMPLATES: tuple[tuple[str, TemplateFn], ...] = tuple(
    sorted(templates_python.FAMILIES.items(), key=lambda item: item[0])
)
TEXT_TEMPLATES: tuple[tuple[str, TemplateFn], ...] = tuple(
    sorted(templates_text.FAMILIES.items(), key=lambda item: item[0])
)

# G5 design doc §5's refusal registry (`templates_refusal.py`'s own module
# doc explains the (family, lens) group split): re-exported here so callers
# only need `from tools.flywheel.factory import templates` for every
# template family, repair or refusal.
REFUSAL_TEMPLATES = templates_refusal.ALL_REFUSAL_TEMPLATES
REFUSAL_GROUPS = templates_refusal.GROUPS

# Rule 1's disjointness contract: the word lists backing every template
# family must not contain any gate-set target filename, function name, or
# domain noun. `test_templates.py` asserts this against GATE_VOCABULARY.
ALL_TEMPLATE_WORDS: frozenset[str] = all_wordlist_tokens()

assert not (ALL_TEMPLATE_WORDS & GATE_VOCABULARY), (
    "template word lists reuse gate-set vocabulary — this should be impossible; "
    "see wordlists.py and contamination.GATE_VOCABULARY"
)
