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
"""

import json
import sys


def handle(req):
    if req.get("cmd") != "trajectory":
        return {"error": f"stub_tool: unknown cmd {req.get('cmd')!r}"}

    goal = req["goal"]
    target = req["target"]
    target_contents = req["target_contents"]
    search = req["search"]
    replace = req["replace"]
    summary = req["summary"]

    prompt1 = f"PROMPT1 goal={goal}"
    completion1 = f'<action verb="read" path="{target}">\n</action>'

    prompt2 = f"PROMPT2 goal={goal} read={target_contents}"
    completion2 = (
        f'<action verb="patch" path="{target}">\n<<<<<<< SEARCH\n{search}\n'
        f"=======\n{replace}\n>>>>>>> REPLACE\n</action>"
    )

    pairs = [
        {"prompt": prompt1, "completion": completion1},
        {"prompt": prompt2, "completion": completion2},
    ]

    if "TRIGGER_LANDING_FAILURE" in goal or search not in target_contents:
        return {
            "pairs": pairs,
            "landed": False,
            "landing_detail": "stub_tool: search not found (simulated)",
        }

    patched_contents = target_contents.replace(search, replace, 1)
    prompt3 = f"PROMPT3 goal={goal} read={target_contents} patched={patched_contents}"
    completion3 = f'<action verb="done">\n{summary}\n</action>'
    pairs.append({"prompt": prompt3, "completion": completion3})

    return {"pairs": pairs, "landed": True, "patched_contents": patched_contents}


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
