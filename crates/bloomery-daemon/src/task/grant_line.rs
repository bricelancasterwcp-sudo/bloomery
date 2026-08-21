//! The envelope-v4 grant line: the one line of a task prompt that tells the
//! model which commands it may `run`
//! (`docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md` §2).
//!
//! **Why this exists.** Through envelope-v3 the rendered prompt was `goal +
//! verb card + transcript` and never mentioned the grant, so a run-granted
//! task and a plain one were token-identical at the moment that matters (the
//! decision point after a successful patch observation). Turn 3's corpus
//! voted `done` 666 : `run` 333 on those indistinguishable inputs and
//! supervised fine-tuning did the only thing it can with conflicting labels
//! for identical inputs — it took the majority, and the trained model
//! emitted **zero** `run` verbs at probe time (spec §1). This line is the
//! cue that dissolves the conflict.
//!
//! **Rendered from the enforced `Grant`, never from task text.** The caller
//! ([`crate::task::task_loop::render_prompt`]) passes
//! `spec.grant.commands()` — the very allowlist `exec_run` checks against —
//! so the model can never be told something the loop would refuse. The
//! rejected alternative (a sentence in the fixture's goal text) needed no
//! envelope bump but would have taught permissions from the task author, with
//! two authoring surfaces obliged to agree.
//!
//! Exact bytes are load-bearing: they are prompt content the model reads
//! verbatim and every turn-4 verdict is measured under them, so they are
//! pinned by `tests/task_render_test.rs` against literals (including the em
//! dash, U+2014, in the `none` line).

/// The `none` line, verbatim (spec §2): what a task that grants no command
/// renders. Says what is *not* available rather than staying silent about
/// it — a plain and a find-shaped task both render this, which is exactly
/// what makes them distinguishable from a run-granted one.
const NONE_LINE: &str = "Granted commands: none — run is not available in this task";

/// The label every granted line opens with (spec §2's `Granted commands:
/// python3 -m unittest`).
const GRANTED_LABEL: &str = "Granted commands:";

/// Renders the grant line for `commands` — the argv prefixes a
/// [`bloomery_core::grant::Grant`] allows.
///
/// - No prefixes -> [`NONE_LINE`].
/// - One or more -> one **self-describing** line per prefix, the prefix's
///   words space-joined: `Granted commands: python3 -m unittest`. The label
///   repeats rather than the extra prefixes hanging off a continuation line,
///   so every line of the block stands alone and there is exactly one format
///   string for the whole renderer. Every fixture and corpus slice turn 4
///   ships carries a single prefix, so the multi-prefix shape is a
///   definition rather than a measured surface.
///
/// Never ends with a newline: the caller owns the separation between this
/// line and the verb card (the same blank line the card already uses), so
/// the prompt's shape lives in one place.
pub fn grant_line(commands: &[Vec<String>]) -> String {
    if commands.is_empty() {
        return NONE_LINE.to_string();
    }
    commands
        .iter()
        .map(|prefix| format!("{GRANTED_LABEL} {}", prefix.join(" ")))
        .collect::<Vec<_>>()
        .join("\n")
}
