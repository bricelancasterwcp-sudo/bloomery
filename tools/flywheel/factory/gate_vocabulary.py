"""Gate TOML parsing and `GATE_VOCABULARY` — responsibility 1 of the
contamination guard (design spec §3, brief rule 7), split out of
`contamination.py` to keep that file under the 400-line house cap (same
reasoning turn 1 used for `templates_python.py`/`templates_text.py`;
task 6a's `_violations_for_task`/`task_violates_gates` addition pushed
`contamination.py` over budget).

``GATE_VOCABULARY`` — the forbidden-word enumeration rule 1 needs.
``templates.py`` imports it and a test in ``test_templates.py`` asserts
every template word list is disjoint from it. It is built from the REAL
gate TOML (target filenames, filename stems, and every ``def`` name it
defines — parsed mechanically, not hand-transcribed, so it cannot
silently drift out of sync) unioned with a hand-curated set of the gate
set's other distinctive nouns, compound identifiers, domains, and
version/date strings (things a filename/function-name scan alone would
not surface, e.g. "bloomery", "ada", "listen_port", "example.com").
``test_contamination.py`` mechanically checks the auto-derived half for
completeness against the live TOML.

Deliberately kept scoped to `codec-tasks-v1` ONLY, not unioned with
`codec-tasks-v2-mixed`: `codec-tasks-v2-mixed` is itself factory-authored,
drawing its target filenames and identifiers from the SAME `wordlists.py`
pools every repair/refusal template already uses (Task 4 authors it "VIA
the factory"). Folding its vocabulary into `GATE_VOCABULARY` would ban the
factory's own shared word pools from ever being used again — the
`templates.py`-level ``assert not (ALL_TEMPLATE_WORDS & GATE_VOCABULARY)``
would fail at import time the moment `codec-tasks-v2-mixed` existed.
`codec-tasks-v1` is different: it was hand-authored independently of the
factory's word pools, so banning its specific vocabulary is meaningful
hygiene with no such paradox.

`load_gate_fixtures`/`GateFixture` are also `contamination.py`'s (and
`gate_sampling.py`'s) parsing primitive for the CLI's actual comparator —
that is a SEPARATE responsibility (contamination.py's own docstring
covers it) that simply reuses this module's parser rather than a second
one.
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

_REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_GATE_PATH = _REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v1.toml"

_DEF_NAME_RE = re.compile(r"\bdef\s+([A-Za-z_]\w*)")


@dataclass(frozen=True)
class GateFixture:
    """One `[[fixture]]` entry from the gate TOML. `search`/`replace` are
    `None` for an `expect = "refuse"` fixture (G5 design doc §2: a refuse
    fixture carries no `[fixture.reference]` at all) — `None`, never a
    fabricated empty string, so `check_corpus`'s `search_match` rule can
    tell "nothing to compare" apart from "an empty search string" and skip
    the comparison rather than risk a false positive."""

    name: str
    lens: str
    target: str
    files: dict[str, str]
    goal: str
    expect: str
    search: str | None
    replace: str | None
    refusal_reason: str | None


def load_gate_fixtures(path: Path) -> list[GateFixture]:
    """Parses the gate TOML (stdlib `tomllib`, per the brief) into
    structured fixtures, in file order (deterministic — TOML arrays
    preserve order; never a `set`). Handles both fixture classes (G5
    design doc §2): `expect` defaults to `"patch"` when absent (every
    `codec-tasks-v1` fixture), matching the Rust parser's own default."""
    with open(path, "rb") as f:
        data = tomllib.load(f)
    fixtures = []
    for fx in data["fixture"]:
        files = {file_entry["path"]: file_entry["contents"] for file_entry in fx["file"]}
        expect = fx.get("expect", "patch")
        reference = fx.get("reference")
        fixtures.append(
            GateFixture(
                name=fx["name"],
                lens=fx["lens"],
                target=fx["target"],
                files=files,
                goal=fx["goal"],
                expect=expect,
                search=reference["search"] if reference is not None else None,
                replace=reference["replace"] if reference is not None else None,
                refusal_reason=fx.get("refusal_reason"),
            )
        )
    return fixtures


def _extract_filenames_and_stems(fixtures: Iterable[GateFixture]) -> frozenset[str]:
    words: set[str] = set()
    for fx in fixtures:
        words.add(fx.target.lower())
        words.add(fx.target.split(".")[0].lower())
    return frozenset(words)


def _extract_function_names(fixtures: Iterable[GateFixture]) -> frozenset[str]:
    names: set[str] = set()
    for fx in fixtures:
        for contents in fx.files.values():
            names.update(m.group(1).lower() for m in _DEF_NAME_RE.finditer(contents))
    return frozenset(names)


# ---------------------------------------------------------------------------
# Hand-curated additions: the gate set's other distinctive vocabulary that
# a filename/function-name scan does not surface on its own — thematic
# nouns tied to a single fixture's premise, config/identifier keys that
# are not `def` names, domains/emails, and the specific version/date
# strings used in the changelog/release-notes fixtures. Each is banned as
# a whole token (an exact identifier or word), not a substring — e.g.
# "listen_port" is forbidden, but the generic word "port" that rule 1's
# own "port/host mismatch" family needs is not.
# ---------------------------------------------------------------------------

_EXTRA_DOMAIN_NOUNS = frozenset(
    {
        "cart", "billing", "shipping", "inventory", "restock", "reorder",
        "discontinued", "discount", "discounted", "pricing", "subtotal",
        "membership", "member", "password", "validator", "signup",
        "register", "username", "secret", "welcome", "acme", "corp",
        "support", "upstream", "proxy", "backend", "worker", "retries",
        "concurrency", "queue", "readme", "npm", "changelog", "settings",
        "health", "threshold", "tax", "stock", "valid", "free", "db",
        "release", "bloomery", "ada", "best",
    }
)

_EXTRA_COMPOUND_IDENTIFIERS = frozenset(
    {
        "listen_addr", "listen_port", "health_path", "db_host", "db_port",
        "db_name", "db_user", "health_check_url", "http_timeout_ms",
        "max_retries", "log_level", "upstream_url", "proxy_pass",
        "max_discount_pct",
    }
)

_EXTRA_DOMAINS_AND_EMAILS = frozenset(
    {
        "example.com", "example.org", "billing@example.org",
        "billing@example.com", "api.internal.example.com",
        "support.bloomery.example.com",
    }
)

_EXTRA_VERSION_AND_DATE_STRINGS = frozenset(
    {"2.4.0", "2.4.1", "3.1.0", "v3.1.0", "2026-02-30", "2026-02-28", "2026-07-18", "2026-08-02"}
)

_gate_fixtures_for_vocabulary = load_gate_fixtures(DEFAULT_GATE_PATH)

GATE_VOCABULARY: frozenset[str] = frozenset().union(
    _extract_filenames_and_stems(_gate_fixtures_for_vocabulary),
    _extract_function_names(_gate_fixtures_for_vocabulary),
    _EXTRA_DOMAIN_NOUNS,
    _EXTRA_COMPOUND_IDENTIFIERS,
    _EXTRA_DOMAINS_AND_EMAILS,
    _EXTRA_VERSION_AND_DATE_STRINGS,
)
"""Every target filename (and stem), function name, and distinctive
domain noun the gate set uses, lowercased. Rule 1's forbidden list."""
