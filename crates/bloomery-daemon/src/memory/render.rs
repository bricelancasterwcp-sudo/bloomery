//! Rendering one retrieved episode into the block the model actually sees
//! (memory-organ Task 6; spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4).
//!
//! **Quoted evidence only.** The block states three things and nothing
//! else: that this exact goal was completed before against byte-identical
//! starting files, the patch bodies that landed (verbatim, in step order),
//! and the granted command that exited 0 afterward. No advice, no
//! paraphrase, no summary — the spec's §2 rule ("nothing in the record is
//! model prose") is a rule about the *store*, and this is where it would be
//! easiest to quietly break it by re-introducing prose at render time. In
//! particular the episode's `done` summary is never rendered: it is not even
//! carried by [`EpisodeRecord`], deliberately, because the flywheel5 battery
//! §6.6 caught exactly that text fabricating repairs.
//!
//! **Deterministic.** The output is a pure function of the record — no
//! clock, no map iteration, no formatting that depends on anything outside
//! the fields read here. A retrieved episode injected twice renders twice
//! the same, which is what makes a memory-on prompt reproducible at all.
//!
//! **No trailing newline.** The block ends at `[end memory]`; the *section*
//! formatting in `task::task_loop::render_prompt_from` supplies the blank
//! line that separates it from what follows, exactly as the grant section
//! already does. Putting the separator here instead would give a caller two
//! places to get the spacing wrong.

use super::record::EpisodeRecord;

/// The block's opening delimiter and its one standing claim.
const HEADER: &str = "[memory: verified prior attempt]\n\
                      This exact goal was completed before against byte-identical starting files.\n";

/// The block's closing delimiter. Unterminated on purpose — see this
/// module's docs.
const FOOTER: &str = "[end memory]";

/// Renders `e` as the injectable memory block (spec §4). Pinned bytes:
///
/// ```text
/// [memory: verified prior attempt]
/// This exact goal was completed before against byte-identical starting files.
/// --- patch {path} ({codec})
/// {body}
/// Verification: {argv joined with spaces} -> {outcome}
/// [end memory]
/// ```
///
/// with one `--- patch` stanza per landed patch, in the record's own order.
/// `tests/memory_render_test.rs` pins this shape as a literal golden.
pub fn render_memory_block(e: &EpisodeRecord) -> String {
    let mut out = String::from(HEADER);
    for patch in &e.landed_patches {
        out.push_str("--- patch ");
        out.push_str(&patch.path);
        out.push_str(" (");
        out.push_str(&patch.codec);
        out.push_str(")\n");
        out.push_str(&patch.body);
        out.push('\n');
    }
    out.push_str("Verification: ");
    out.push_str(&e.run_evidence.argv.join(" "));
    out.push_str(" -> ");
    out.push_str(&e.run_evidence.outcome);
    out.push('\n');
    out.push_str(FOOTER);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::record::{RunEvidence, StoredPatch};

    fn record(landed: Vec<StoredPatch>) -> EpisodeRecord {
        EpisodeRecord {
            episode_id: "ep".into(),
            goal_hash: "gh".into(),
            goal_text: "a goal".into(),
            cited_files: Vec::new(),
            landed_patches: landed,
            run_evidence: RunEvidence {
                argv: vec!["cargo".into(), "test".into()],
                outcome: "exit 0".into(),
            },
            trajectory: Vec::new(),
            minted_by_model: "m".into(),
            minted_by_envelope: "v4".into(),
            status: "verified".into(),
            contradicted_by: None,
            minted_at: 0,
        }
    }

    /// A record with no landed patches cannot be minted (the mint bar
    /// requires at least one — spec §2), but the renderer must still be
    /// total rather than panicking or emitting a half-formed block: an
    /// operator-supplied or hand-edited store row is untrusted input.
    #[test]
    fn a_patchless_record_still_renders_a_well_formed_block() {
        let block = render_memory_block(&record(Vec::new()));
        assert_eq!(
            block,
            "[memory: verified prior attempt]\n\
             This exact goal was completed before against byte-identical starting files.\n\
             Verification: cargo test -> exit 0\n\
             [end memory]"
        );
    }

    /// The delimiters are what let a reader (and the model) tell quoted
    /// evidence from the rest of the prompt, so both must be present and the
    /// block must not end with the separator the prompt renderer adds.
    #[test]
    fn the_block_is_delimited_and_carries_no_trailing_newline() {
        let block = render_memory_block(&record(vec![StoredPatch {
            path: "/w/a.txt".into(),
            codec: "whole_file".into(),
            body: "x".into(),
        }]));
        assert!(block.starts_with("[memory: verified prior attempt]\n"));
        assert!(block.ends_with("[end memory]"), "{block:?}");
        assert!(!block.ends_with('\n'), "{block:?}");
    }
}
