#!/usr/bin/env python3
"""A canned stand-in for `flywheel-tool` used by generate.py's unit tests
(brief step 1: "the generate tests may use a stub tool script that echoes
canned responses, EXCEPT one integration test that uses the real built
bin"). Speaks the same one-JSON-request-per-line / one-JSON-response-
per-line protocol, but never touches serving code: it builds
deterministic placeholder prompts/completions from the request fields,
and fails to land only when the request's `goal` contains the sentinel
substring "TRIGGER_LANDING_FAILURE" — so tests can exercise the abort
path (brief rule 4) without depending on any real Python/plaintext lens
behavior.

Task 4 extends this stub additively for `expect="refuse"` requests
(G5 design doc §5 / task-3's wire shape): 2 pairs (`read`, `done`) instead
of 3, `verified: "refusal"` in the response — EXCEPT when the goal
contains the sentinel substring "TRIGGER_VERIFIED_MISMATCH", which
returns `verified: None` instead, so generate.py's tests can exercise its
own verification-mismatch abort path without depending on the real tool.

Task 7 extends it again for turn 3's two patch-mode shapes: `find_pattern`
selects a 4-pair find/read/patch/done response and `run_argv` a 4-pair
read/patch/run/done one. A stub cannot walk a directory or spawn a
process, so it does not pretend to: it substring-matches `find_pattern`
across the request's `files` and prefix-matches `run_argv` against
`commands`. Those two checks exist because they are the SHAPE-selecting
hard errors the real binary answers with (a find matching zero files; an
ungranted run argv) — a stub that answered them with a rendered
trajectory would let a factory bug pass every stub-driven test and only
surface against the real tool.

Turn 4 (spec §2) makes the stub speak envelope-v4. Three additions, all of
them for the same reason as the two checks above — the stub is what the
whole unit-level generate suite sees, so anything it silently ignores is
something no unit test can hold the factory to:

- `envelope` is PARSED, and an unrecognized value is a named error, exactly
  as `parse_envelope` answers one.
- under `"v4"` every prompt carries the GRANT LINE, rendered from the
  request's own `commands` with the same two forms and the same exact bytes
  as `task::grant_line` (em dash, U+2014, included). The rest of a stub
  prompt stays a placeholder — a stub has no verb card and no real
  transcript to render — so what a unit test can hold the factory to here
  is the grant line and the goal, which is precisely the v4 surface.
- an unusable argv prefix in `commands` (empty, or carrying a blank word)
  is a named error for EVERY mode, mirroring
  `scratch::check_command_prefixes`: under v4 those bytes reach the prompt,
  where an empty prefix renders a dangling label and a blank word a stray
  double space.
"""

import json
import sys

FIND_PATH = "."

ENVELOPES = ("v1", "v2", "v3", "v4")
GRANT_LINE_ENVELOPES = ("v4",)
NONE_LINE = "Granted commands: none — run is not available in this task"
GRANTED_LABEL = "Granted commands:"


def _files_of(req):
    """The request's `files` as a path -> contents dict, falling back to
    the single-file `{target: target_contents}` workspace exactly as the
    real binary's `files_to_materialize` does."""
    files = req.get("files")
    if not files:
        return {req["target"]: req["target_contents"]}
    return {entry["path"]: entry["contents"] for entry in files}


def _grant_section(req):
    """envelope-v4's grant line plus the blank line separating it from what
    follows, or "" under v1-v3 (where the line does not exist and every
    prompt must stay byte-identical to its pre-v4 self). One line per argv
    prefix, the label repeated — `task::grant_line`'s shape exactly."""
    if req.get("envelope") not in GRANT_LINE_ENVELOPES:
        return ""
    commands = req.get("commands") or []
    if not commands:
        return f"{NONE_LINE}\n\n"
    return "".join(f"{GRANTED_LABEL} {' '.join(prefix)}\n" for prefix in commands) + "\n"


def _handle_refuse(req):
    goal = req["goal"]
    target = req["target"]
    target_contents = req["target_contents"]
    refusal_reason = req["refusal_reason"]
    target_missing = req.get("target_missing", False)
    grant = _grant_section(req)

    prompt1 = f"PROMPT1 goal={goal}\n\n{grant}"
    completion1 = f'<action verb="read" path="{target}">\n</action>'

    if target_missing:
        read_outcome = f"read failed: stub NotFound for {target}"
    else:
        read_outcome = target_contents
    prompt2 = f"PROMPT2 goal={goal} read={read_outcome}\n\n{grant}"
    completion2 = f'<action verb="done">\n{refusal_reason}\n</action>'

    pairs = [
        {"prompt": prompt1, "completion": completion1},
        {"prompt": prompt2, "completion": completion2},
    ]

    verified = None if "TRIGGER_VERIFIED_MISMATCH" in goal else "refusal"
    return {"pairs": pairs, "landed": True, "verified": verified}


def _pairs(req, completions):
    """Placeholder prompts numbered by POSITION, so the same completion in
    a 3-pair and a 4-pair shape still gets the step number it actually
    occupies. Every prompt carries the same grant section, exactly as the
    real renderer rebuilds the line for every step."""
    goal = req["goal"]
    grant = _grant_section(req)
    return [
        {"prompt": f"PROMPT{i + 1} goal={goal}\n\n{grant}", "completion": completion}
        for i, completion in enumerate(completions)
    ]


def _find_completion(req):
    """The find shape's opening completion, or an `{"error": ...}` dict
    when the pattern matches nothing — the real binary's hard error (an
    ideal whose opening find finds nothing is not an ideal)."""
    pattern = req["find_pattern"]
    hits = [path for path, contents in _files_of(req).items() if pattern in contents]
    if not hits:
        return {"error": f'stub_tool: find_pattern {pattern!r} found 0 matches across "files"'}
    return f'<action verb="find" pattern="{pattern}" path="{FIND_PATH}">\n</action>'


def _run_completion(req):
    """The run shape's verification completion, or an `{"error": ...}` dict
    when `run_argv` is outside every granted `commands` prefix — the real
    `Grant`'s element-wise prefix rule, which the real binary surfaces as a
    "the run verification never ran" hard error."""
    argv = req["run_argv"]
    granted = [prefix for prefix in req.get("commands", []) if prefix]
    if not any(argv[: len(prefix)] == prefix for prefix in granted):
        return {"error": f"stub_tool: run_argv {argv} is not covered by any granted prefix {granted}"}
    return f'<action verb="run">\n{json.dumps(argv)}\n</action>'


def _handle_patch(req):
    goal = req["goal"]
    target = req["target"]
    target_contents = req["target_contents"]
    search = req["search"]
    replace = req["replace"]
    find_pattern = req.get("find_pattern")
    run_argv = req.get("run_argv")

    if find_pattern is not None and run_argv is not None:
        return {"error": "stub_tool: a request carries both find_pattern and run_argv"}

    completions = []
    if find_pattern is not None:
        found = _find_completion(req)
        if isinstance(found, dict):
            return found
        completions.append(found)

    completions.append(f'<action verb="read" path="{target}">\n</action>')
    completions.append(
        f'<action verb="patch" path="{target}">\n<<<<<<< SEARCH\n{search}\n'
        f"=======\n{replace}\n>>>>>>> REPLACE\n</action>"
    )

    if "TRIGGER_LANDING_FAILURE" in goal or search not in target_contents:
        return {
            "pairs": _pairs(req, completions),
            "landed": False,
            "landing_detail": "stub_tool: search not found (simulated)",
        }

    if run_argv is not None:
        ran = _run_completion(req)
        if isinstance(ran, dict):
            return ran
        completions.append(ran)

    completions.append(f'<action verb="done">\n{req["summary"]}\n</action>')
    return {
        "pairs": _pairs(req, completions),
        "landed": True,
        "patched_contents": target_contents.replace(search, replace, 1),
    }


def _envelope_error(req):
    """`parse_envelope`'s answer to an unrecognized value: a named error,
    never a silently-defaulted render."""
    envelope = req.get("envelope")
    if envelope not in ENVELOPES:
        return {"error": f"stub_tool: unknown envelope {envelope!r}: valid values are {list(ENVELOPES)}"}
    return None


def _commands_error(req):
    """`scratch::check_command_prefixes`, mirrored: an argv prefix this
    tool could not honor is a named error for every mode, because under v4
    those words are rendered into the prompt."""
    for index, prefix in enumerate(req.get("commands") or []):
        if not prefix:
            return {"error": f"stub_tool: commands[{index}] is an empty argv prefix"}
        for word_index, word in enumerate(prefix):
            if not word.strip():
                return {"error": f"stub_tool: commands[{index}][{word_index}] is blank"}
    return None


def handle(req):
    if req.get("cmd") != "trajectory":
        return {"error": f"stub_tool: unknown cmd {req.get('cmd')!r}"}

    for check in (_envelope_error, _commands_error):
        error = check(req)
        if error is not None:
            return error

    if req.get("expect") == "refuse":
        return _handle_refuse(req)
    return _handle_patch(req)


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            resp = handle(req)
        except Exception as exc:  # pragma: no cover - defensive only
            resp = {"error": f"stub_tool: {exc}"}
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
