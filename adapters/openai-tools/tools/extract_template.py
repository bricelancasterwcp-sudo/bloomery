# bloomery — an operating layer for local LLMs.
# Copyright (C) 2026 Brice Lancaster
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU Affero General Public License, version 3, as
# published by the Free Software Foundation.
#
# This program is distributed in the hope that it will be useful, but WITHOUT
# ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
# FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
# for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program. If not, see <https://www.gnu.org/licenses/>.
#
# Commercial licensing is available as an alternative to the AGPL — see
# LICENSING.md.

"""One-time: lift `tokenizer.chat_template` out of a GGUF into a file.

Needs the `gguf` package, which on this box lives beside llama.cpp:

    PYTHONPATH=~/llama.cpp/gguf-py python3 adapters/openai-tools/tools/extract_template.py \\
        /home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf \\
        adapters/openai-tools/templates/qwen36-reap48-ours.jinja

The adapter itself never imports `gguf`: it reads the committed file, so the
template is a versioned, sha-pinned artifact rather than something re-derived
at every boot.
"""
import hashlib
import sys

import gguf


def main(gguf_path: str, out_path: str) -> int:
    reader = gguf.GGUFReader(gguf_path)
    for field in reader.fields.values():
        if "chat_template" in field.name:
            text = bytes(field.parts[field.data[0]]).decode("utf-8")
            with open(out_path, "w", encoding="utf-8") as handle:
                handle.write(text)
            print(f"wrote {out_path} ({len(text)} chars)")
            print("sha256:", hashlib.sha256(text.encode("utf-8")).hexdigest())
            return 0
    print("no chat_template in", gguf_path, file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1], sys.argv[2]))
