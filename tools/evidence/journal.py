"""Journal loading and the CodecFixture <-> TaskStep join (turn-5 spec §3).

Two joins: KEYED (`CodecFixture.agent == TaskStep.id`, rows journaled from
turn 5 on) and ORDINAL (the turn-3/4 method: CodecFixture rows in journal
order <-> TaskStep groups in first-seen order, with three validations). When
rows carry `agent`, both run and must agree; older journals get ordinal only.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path


def load_rows(path: Path) -> list[dict]:
    with Path(path).open() as f:
        return [json.loads(line) for line in f if line.strip()]


def fixture_rows(journal: list[dict]) -> list[dict]:
    return [r for r in journal if r.get("event") == "CodecFixture"]


def task_groups(tasks: list[dict]) -> dict[str, list[dict]]:
    groups: dict[str, list[dict]] = {}
    for r in tasks:
        if r.get("event") != "TaskStep":
            continue
        groups.setdefault(r["id"], []).append(r)
    return groups


@dataclass
class Joined:
    fixture: dict
    steps: list[dict]


@dataclass
class JoinReport:
    mode: str
    keyed_equals_ordinal: bool | None
    fixtures: int
    groups: int
    violations: list[str] = field(default_factory=list)
    # The ordinal join's own violations, computed alongside a keyed join for
    # comparison but never folded into `violations` (which drives the exit
    # code) — surfaced here for a reader instead of silently discarded.
    # Empty in ordinal mode, where `violations` already IS this list.
    ordinal_violations: list[str] = field(default_factory=list)


def _ordinal(fixtures: list[dict], groups: dict[str, list[dict]]) -> tuple[list[Joined], list[str]]:
    violations: list[str] = []
    ids = list(groups)  # dict preserves first-seen order
    if len(ids) != len(fixtures):
        violations.append(f"group count {len(ids)} != CodecFixture count {len(fixtures)}")
    joined: list[Joined] = []
    prev_stamp = None
    for i, fx in enumerate(fixtures):
        steps = groups[ids[i]] if i < len(ids) else []
        if len(steps) != fx.get("steps"):
            violations.append(f"{fx['fixture']}: group length {len(steps)} != steps {fx.get('steps')}")
        stamp = fx.get("epoch_ms")
        if stamp is not None:
            for s in steps:
                if s.get("epoch_ms") is not None and not ((prev_stamp is None or s["epoch_ms"] >= prev_stamp) and s["epoch_ms"] <= stamp):
                    violations.append(f"{fx['fixture']}: step {s.get('step')} epoch_ms outside its fixture bracket")
            prev_stamp = stamp
        joined.append(Joined(fx, steps))
    return joined, violations


def _keyed(fixtures: list[dict], groups: dict[str, list[dict]]) -> tuple[list[Joined], list[str]]:
    violations: list[str] = []
    joined: list[Joined] = []
    for fx in fixtures:
        steps = groups.get(fx.get("agent"), [])
        if fx.get("agent") is None:
            violations.append(f"{fx['fixture']}: no agent key")
        if len(steps) != fx.get("steps"):
            violations.append(f"{fx['fixture']}: keyed group length {len(steps)} != steps {fx.get('steps')}")
        joined.append(Joined(fx, steps))
    return joined, violations


def join(journal: list[dict], tasks: list[dict]) -> tuple[list[Joined], JoinReport]:
    fixtures = fixture_rows(journal)
    groups = task_groups(tasks)
    ordinal, ov = _ordinal(fixtures, groups)
    if fixtures and all(fx.get("agent") for fx in fixtures):
        keyed, kv = _keyed(fixtures, groups)
        # Compare the joined ROWS themselves, not just their shapes: two
        # equal-length groups whose `agent` keys are swapped have identical
        # (fixture, [step numbers]) shapes (each group's steps are numbered
        # 1..N locally) but join different TaskStep rows — `s["id"]` is the
        # field that actually differs between them.
        same = [(a.fixture["fixture"], [s["id"] for s in a.steps]) for a in keyed] == \
               [(b.fixture["fixture"], [s["id"] for s in b.steps]) for b in ordinal]
        report = JoinReport("keyed", same, len(fixtures), len(groups),
                             kv + ([] if same else ["keyed != ordinal"]), ov)
        return keyed, report
    return ordinal, JoinReport("ordinal", None, len(fixtures), len(groups), ov)
