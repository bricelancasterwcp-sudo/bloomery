"""Plaintext-lens template families (brief rule 1: >= 5 families):

config value mismatch, port/host mismatch vs surrounding lines,
version/date string, wrong name in prose, wrong URL/path in docs.

Same determinism contract as `templates_python.py`: pure `(rng) -> Task`
functions, every choice from `random.Random` or plain tuples, no `set`
iteration, no wall-clock.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import DONE_INSTRUCTION, Task
from tools.flywheel.factory.wordlists import (
    CONFIG_KEY_BASES,
    DOC_URL_PATHS,
    FAKE_DOMAIN_BASES,
    FAKE_DOMAIN_TLDS,
    HOST_KEY_NAMES,
    MONTH_NAMES,
    PERSON_NAMES,
    PORT_KEY_NAMES,
    PRODUCT_NAMES,
    THEMES,
)


def _stem_from_name(name: str) -> str:
    return "".join(ch for ch in name.lower() if ch.isalnum()) or "generated"


def _family_config_value_mismatch(rng: random.Random) -> Task:
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.ini"
    key = rng.choice(CONFIG_KEY_BASES)
    other_keys = rng.sample([k for k in CONFIG_KEY_BASES if k != key], 3)
    wrong_val = rng.randint(1, 40)
    correct_val = rng.randint(wrong_val + 5, wrong_val + 55)
    other_vals = [rng.randint(1, 500) for _ in other_keys]

    lines = [f"[{theme.id}]", f"{key} = {wrong_val}"]
    lines.extend(f"{k} = {v}" for k, v in zip(other_keys, other_vals))
    contents = "\n".join(lines) + "\n"

    search = f"{key} = {wrong_val}"
    replace = f"{key} = {correct_val}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=key,
        problem=f"is set to {wrong_val}, but operations requires it to be at least {correct_val}",
        evidence="staying below that threshold causes failures under load",
        fix_target=f"the {key} line in {target}",
        fix_goal=f"it reads {correct_val}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Changed {key} in {target} from {wrong_val} to {correct_val}."
    return Task("txt_config_value_mismatch", "plaintext", target, {target: contents}, goal, search, replace, summary)


def _family_port_host_mismatch(rng: random.Random) -> Task:
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.conf"
    kind = rng.choice(("port", "host"))

    if kind == "port":
        key = rng.choice(PORT_KEY_NAMES)
        correct_val = rng.randint(1024, 9999)
        wrong_val = rng.randint(1024, 9999)
        while wrong_val == correct_val:
            wrong_val = rng.randint(1024, 9999)
        mismatched_line = f"{key} = {wrong_val}"
        fixed_line = f"{key} = {correct_val}"
        reference_line = f"health_check_target = http://127.0.0.1:{correct_val}/status"
        wrong_display, correct_display = wrong_val, correct_val
    else:
        key = rng.choice(HOST_KEY_NAMES)
        base_correct = rng.choice(FAKE_DOMAIN_BASES)
        base_wrong = rng.choice([b for b in FAKE_DOMAIN_BASES if b != base_correct])
        tld = rng.choice(FAKE_DOMAIN_TLDS)
        correct_host = f"{base_correct}.{tld}"
        wrong_host = f"{base_wrong}.{tld}"
        mismatched_line = f"{key} = {wrong_host}"
        fixed_line = f"{key} = {correct_host}"
        reference_line = f"health_check_target = http://{correct_host}/status"
        wrong_display, correct_display = wrong_host, correct_host

    contents = (
        f"service_name = {theme.id}-relay\n"
        f"region = local\n"
        f"{mismatched_line}\n"
        f"health_path = /status\n"
        f"{reference_line}\n"
    )
    search = mismatched_line
    replace = fixed_line
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=key,
        problem=f"says {wrong_display}",
        evidence=(
            f"health_check_target at the bottom of the file uses {correct_display} -- the health "
            f"check never reaches the service"
        ),
        fix_target=f"the {key} line in {target}",
        fix_goal=f"it reads {correct_display}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Changed {key} in {target} from {wrong_display} to {correct_display}."
    return Task("txt_port_host_mismatch", "plaintext", target, {target: contents}, goal, search, replace, summary)


def _family_version_date_string(rng: random.Random) -> Task:
    product = rng.choice(PRODUCT_NAMES)
    target = f"{_stem_from_name(product)}_notes.txt"
    major, minor, patch1 = rng.randint(1, 9), rng.randint(0, 9), rng.randint(0, 8)
    patch2 = patch1 + 1
    version_dup = f"{major}.{minor}.{patch1}"
    version_correct = f"{major}.{minor}.{patch2}"
    month = rng.choice(MONTH_NAMES)
    day1 = rng.randint(1, 18)
    day2 = day1 + rng.randint(3, 9)
    feature_a = rng.choice(("a caching layer", "a retry policy", "a dashboard widget", "an export option"))
    feature_b = rng.choice(("a rendering glitch", "a slow query", "a stale cache entry", "a rounding error"))

    contents = (
        f"# {product} Release Notes\n"
        f"\n"
        f"## {version_dup} - {month} {day1}\n"
        f"- Added {feature_a}\n"
        f"\n"
        f"## {version_dup} - {month} {day2}\n"
        f"- Fixed {feature_b}\n"
    )
    search = f"## {version_dup} - {month} {day2}"
    replace = f"## {version_correct} - {month} {day2}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject="two release-note headings",
        problem=(
            f"share the heading {version_dup}, but only the {month} {day1} change ({feature_a}) "
            f"actually belongs to that version"
        ),
        evidence=(
            f"the {month} {day2} batch of work ({feature_b} fix) was tagged and shipped afterward as "
            f"{version_correct}, so its heading is stuck on the old number"
        ),
        fix_target=f"the second heading in {target}",
        fix_goal=f"it reads {version_correct}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Retitled the second heading in {target} as {version_correct}."
    return Task("txt_version_date_string", "plaintext", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_name_in_prose(rng: random.Random) -> Task:
    theme = rng.choice(THEMES)
    noun = rng.choice(theme.nouns)
    stem = rng.choice(theme.file_stems)
    target = f"{stem}_postmortem.txt"
    correct_person = rng.choice(PERSON_NAMES)
    wrong_person = rng.choice([p for p in PERSON_NAMES if p != correct_person])
    day_count = rng.randint(2, 9)

    contents = (
        f"Postmortem: {noun} incident\n"
        f"{'=' * (len(noun) + 10)}\n"
        f"\n"
        f"Detected {day_count} days ago and resolved the same day.\n"
        f"\n"
        f"Resolved by: {wrong_person}\n"
        f"\n"
        f"Follow-up: add an alert threshold so this triggers earlier next time.\n"
    )
    search = f"Resolved by: {wrong_person}"
    replace = f"Resolved by: {correct_person}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject="the postmortem",
        problem=f"credits {wrong_person} as the engineer who resolved the {noun} incident",
        evidence=f"the on-call log shows {correct_person} actually closed it out",
        fix_target=f"the resolver's name in {target}",
        fix_goal=f"it reads {correct_person}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Corrected the resolver name in {target} to {correct_person}."
    return Task("txt_wrong_name_in_prose", "plaintext", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_url_path_in_docs(rng: random.Random) -> Task:
    product = rng.choice(PRODUCT_NAMES)
    target = f"{_stem_from_name(product)}_guide.txt"
    domain_base = rng.choice(FAKE_DOMAIN_BASES)
    tld = rng.choice(FAKE_DOMAIN_TLDS)
    correct_path = rng.choice(DOC_URL_PATHS)
    wrong_path = rng.choice([p for p in DOC_URL_PATHS if p != correct_path])

    contents = (
        f"{product} CLI\n"
        f"{'=' * (len(product) + 4)}\n"
        f"\n"
        f"If something goes wrong, check the troubleshooting page at:\n"
        f"\n"
        f"    https://{domain_base}.{tld}{wrong_path}\n"
        f"\n"
        f"The troubleshooting page is linked from every error message.\n"
    )
    search = f"    https://{domain_base}.{tld}{wrong_path}"
    replace = f"    https://{domain_base}.{tld}{correct_path}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject="the troubleshooting link",
        problem=f"points readers to {wrong_path}",
        evidence=f"the actual troubleshooting page lives at {correct_path} -- the linked URL in {target} 404s",
        fix_target=f"the troubleshooting URL in {target}",
        fix_goal=f"it uses {correct_path}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed the troubleshooting URL path in {target} to {correct_path}."
    return Task("txt_wrong_url_path_in_docs", "plaintext", target, {target: contents}, goal, search, replace, summary)


FAMILIES = {
    "txt_config_value_mismatch": _family_config_value_mismatch,
    "txt_port_host_mismatch": _family_port_host_mismatch,
    "txt_version_date_string": _family_version_date_string,
    "txt_wrong_name_in_prose": _family_wrong_name_in_prose,
    "txt_wrong_url_path_in_docs": _family_wrong_url_path_in_docs,
}
