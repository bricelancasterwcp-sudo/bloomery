//! Verb card tests (Phase 2b/2c P4 Task 7): the read-only card variant
//! [`verb_card_for`] must produce when `mutating` is `false`, plus the
//! non-demoted case's equivalence to the pre-existing [`verb_card`].
//!
//! Governing text: `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §6
//! ("Demoted/unmeasured models get a read-only verb card AND a structural
//! dispatch refusal") — this file covers the card half.

use bloomery_core::action::{verb_card, verb_card_for, DoneCard, PatchCodec};

/// The pinned G4 read-only notice line (wire/journal content, exact bytes).
const READ_ONLY_NOTICE: &str =
    "patch and run are not available in this task (this model is read-only under gate G4)";

#[test]
fn a_demoted_card_keeps_read_find_done_and_the_pinned_notice() {
    let card = verb_card_for(PatchCodec::SearchReplace, false, DoneCard::Summary);

    assert!(card.contains("## read"), "missing read section: {card}");
    assert!(card.contains("## find"), "missing find section: {card}");
    assert!(card.contains("## done"), "missing done section: {card}");
    assert!(
        card.contains(READ_ONLY_NOTICE),
        "missing pinned G4 notice line: {card}"
    );
}

#[test]
fn a_demoted_card_drops_the_patch_and_run_worked_examples() {
    let card = verb_card_for(PatchCodec::SearchReplace, false, DoneCard::Summary);

    assert!(
        !card.contains("verb=\"patch\""),
        "demoted card must not show a patch example: {card}"
    );
    assert!(
        !card.contains("verb=\"run\""),
        "demoted card must not show a run example: {card}"
    );
}

#[test]
fn a_demoted_whole_file_card_also_drops_patch_and_run() {
    let card = verb_card_for(PatchCodec::WholeFile, false, DoneCard::Summary);

    assert!(!card.contains("verb=\"patch\""));
    assert!(!card.contains("verb=\"run\""));
    assert!(card.contains(READ_ONLY_NOTICE));
}

#[test]
fn a_non_demoted_card_matches_the_existing_verb_card_exactly() {
    assert_eq!(
        verb_card_for(PatchCodec::SearchReplace, true, DoneCard::Summary),
        verb_card(PatchCodec::SearchReplace)
    );
    assert_eq!(
        verb_card_for(PatchCodec::WholeFile, true, DoneCard::Summary),
        verb_card(PatchCodec::WholeFile)
    );
}

#[test]
fn a_non_demoted_card_still_carries_patch_and_run_examples() {
    let card = verb_card_for(PatchCodec::SearchReplace, true, DoneCard::Summary);
    assert!(card.contains("verb=\"patch\""));
    assert!(card.contains("verb=\"run\""));
    assert!(
        !card.contains(READ_ONLY_NOTICE),
        "a non-demoted card must not carry the read-only notice: {card}"
    );
}

// --- envelope-v5 declared done card (turn-6 spec §3.2) ---

#[test]
fn declared_done_card_renders_in_the_full_branch() {
    let card = verb_card_for(PatchCodec::SearchReplace, true, DoneCard::Declared);
    assert!(card.contains("## done — end the task, declaring what happened"));
    assert!(card.contains(r#"outcome="patched"  reason="fixed""#));
    assert!(card.contains(r#""no-defect" | "no-such-file" | "different-defect""#));
    assert!(card.contains(r#"<action verb="done" outcome="refused" reason="different-defect">"#));
    assert!(card.contains("evidence: src/lib.rs:12 `return total / len(values)`"));
    // The v1-v4 archetype (the §1 confound) is gone under v5.
    assert!(!card.contains("fixed the failing test"));
    // The other four verbs are untouched.
    assert!(card.contains("## read") && card.contains("## patch") && card.contains("## run"));
}

#[test]
fn declared_done_card_renders_in_the_demoted_branch_too() {
    let card = verb_card_for(PatchCodec::SearchReplace, false, DoneCard::Declared);
    assert!(card.contains("declaring what happened"));
    assert!(!card.contains("fixed the failing test"));
    assert!(!card.contains("## patch"));
}

#[test]
fn summary_done_card_is_byte_identical_to_the_pre_v5_output() {
    // The wrapper pins the Summary selection; the existing content tests
    // above pin the bytes themselves.
    assert_eq!(
        verb_card(PatchCodec::SearchReplace),
        verb_card_for(PatchCodec::SearchReplace, true, DoneCard::Summary)
    );
    assert!(verb_card_for(PatchCodec::WholeFile, false, DoneCard::Summary)
        .contains("fixed the failing test"));
}
