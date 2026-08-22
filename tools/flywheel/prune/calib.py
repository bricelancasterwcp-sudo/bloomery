"""Calibration corpus loading and the calibration forward pass.

Accepted inputs:
  *.jsonl  one JSON value per line. A row may be
             {"prompt": ..., "completion": ...}  -> concatenated (the
                 flywheel corpus shape, see ~/flywheel4/corpus.jsonl)
             {"text": ...} / {"content": ...}    -> used as-is
             "a bare json string"                -> used as-is
  anything else  plain text, split into blank-line-separated paragraphs.

Blank lines and whitespace-only samples are dropped. A row that is neither
valid JSON nor a recognised shape aborts with its line number rather than
being skipped silently — a calibration set that quietly lost half its rows
would produce a plausible-looking but wrong saliency ranking.
"""

from __future__ import annotations

import json
import random
from pathlib import Path

import torch

from .observer import ExpertSaliencyObserver

_TEXT_KEYS = ("text", "content")


def _row_to_text(row, line_number: int) -> str:
    if isinstance(row, str):
        return row
    if isinstance(row, dict):
        for key in _TEXT_KEYS:
            if isinstance(row.get(key), str):
                return row[key]
        if isinstance(row.get("prompt"), str):
            return row["prompt"] + row.get("completion", "")
    raise ValueError(
        f"line {line_number}: unrecognised calibration row {row!r}; expected "
        f"a string or an object with 'text', 'content', or 'prompt'")


def load_calibration_texts(path) -> list[str]:
    """Read a calibration corpus into a list of non-empty text samples."""
    path = Path(path)
    if not path.is_file():
        raise FileNotFoundError(f"calibration corpus not found: {path}")
    raw = path.read_text()

    if path.suffix == ".jsonl":
        texts = []
        for number, line in enumerate(raw.splitlines(), start=1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(
                    f"line {number}: not valid JSON ({exc.msg})") from exc
            texts.append(_row_to_text(row, number))
    else:
        texts = raw.split("\n\n")

    kept = [t.strip() for t in texts if t and t.strip()]
    if not kept:
        raise ValueError(f"calibration corpus {path} has no usable samples")
    return kept


def select_samples(texts: list[str], samples: int | None,
                   seed: int) -> list[str]:
    """Deterministic subsample. The seed is recorded in the provenance."""
    if samples is None or samples >= len(texts):
        return list(texts)
    if samples < 1:
        raise ValueError(f"--samples must be >= 1, got {samples}")
    chosen = random.Random(seed).sample(range(len(texts)), samples)
    return [texts[i] for i in sorted(chosen)]


def run_calibration(model, tokenizer, texts: list[str], *, seq_len: int,
                    device: str = "cpu",
                    renormalize_router_weights: bool = False,
                    progress=None) -> tuple[ExpertSaliencyObserver, dict]:
    """One forward pass per sample, batch size 1, statistics accumulated.

    Batch size is fixed at 1 so no padding enters the statistics at all;
    the observer still supports an attention mask for callers that batch.
    """
    observer = ExpertSaliencyObserver(
        model, renormalize_router_weights=renormalize_router_weights)
    total_tokens = 0
    used = 0
    with observer:
        for position, text in enumerate(texts):
            encoded = tokenizer(text, return_tensors="pt", truncation=True,
                                max_length=seq_len,
                                add_special_tokens=True)
            input_ids = encoded["input_ids"].to(device)
            if input_ids.numel() == 0:
                continue
            with torch.no_grad():
                model(input_ids=input_ids)
            total_tokens += int(input_ids.shape[-1])
            used += 1
            if progress is not None:
                progress(position + 1, len(texts))
    if used == 0:
        raise ValueError("every calibration sample tokenised to zero tokens")
    return observer, {"samples": used, "seq_len": seq_len,
                      "tokens": total_tokens}
