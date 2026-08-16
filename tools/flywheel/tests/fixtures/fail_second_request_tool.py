#!/usr/bin/env python3
"""A stub tool that always lands the first `trajectory` request and always
reports `landed:false` on the second one — deterministically, regardless
of request content. Exists purely to give test_generate.py's rule-4
(abort-on-landing-failure) test a real subprocess to drive through
generate.py's CLI without generate.py itself needing any test-only
hooks: the forcing logic lives entirely in this fixture, not production
code.
"""

import json
import sys


def main():
    request_index = 0
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        request_index += 1
        req = json.loads(line)
        target_contents = req["target_contents"]
        search = req["search"]
        replace = req["replace"]
        target = req["target"]
        summary = req["summary"]

        completion1 = f'<action verb="read" path="{target}">\n</action>'
        completion2 = (
            f'<action verb="patch" path="{target}">\n<<<<<<< SEARCH\n{search}\n'
            f"=======\n{replace}\n>>>>>>> REPLACE\n</action>"
        )
        pairs = [
            {"prompt": f"PROMPT1 {request_index}", "completion": completion1},
            {"prompt": f"PROMPT2 {request_index}", "completion": completion2},
        ]

        if request_index == 2:
            resp = {
                "pairs": pairs,
                "landed": False,
                "landing_detail": "fail_second_request_tool: forced failure for testing",
            }
        else:
            patched = target_contents.replace(search, replace, 1)
            pairs.append(
                {
                    "prompt": f"PROMPT3 {request_index}",
                    "completion": f'<action verb="done">\n{summary}\n</action>',
                }
            )
            resp = {"pairs": pairs, "landed": True, "patched_contents": patched}

        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
