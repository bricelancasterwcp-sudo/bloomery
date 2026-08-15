//! Verb card tests (Phase 2b/2c P4 Task 7): the read-only card variant
//! [`verb_card_for`] must produce when `mutating` is `false`, plus the
//! non-demoted case's equivalence to the pre-existing [`verb_card`].
//!
//! Governing text: `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §6
//! ("Demoted/unmeasured models get a read-only verb card AND a structural
//! dispatch refusal") — this file covers the card half.

use bloomery_core::action::{verb_card, verb_card_for, PatchCodec};

/// The pinned G4 read-only notice line (wire/journal content, exact bytes).
const READ_ONLY_NOTICE: &str =
    "patch and run are not available in this task (this model is read-only under gate G4)";

#[test]
fn a_demoted_card_keeps_read_find_done_and_the_pinned_notice() {
    let card = verb_card_for(PatchCodec::SearchReplace, false);

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
    let card = verb_card_for(PatchCodec::SearchReplace, false);

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
    let card = verb_card_for(PatchCodec::WholeFile, false);

    assert!(!card.contains("verb=\"patch\""));
    assert!(!card.contains("verb=\"run\""));
    assert!(card.contains(READ_ONLY_NOTICE));
}

#[test]
fn a_non_demoted_card_matches_the_existing_verb_card_exactly() {
    assert_eq!(
        verb_card_for(PatchCodec::SearchReplace, true),
        verb_card(PatchCodec::SearchReplace)
    );
    assert_eq!(
        verb_card_for(PatchCodec::WholeFile, true),
        verb_card(PatchCodec::WholeFile)
    );
}

#[test]
fn a_non_demoted_card_still_carries_patch_and_run_examples() {
    let card = verb_card_for(PatchCodec::SearchReplace, true);
    assert!(card.contains("verb=\"patch\""));
    assert!(card.contains("verb=\"run\""));
    assert!(
        !card.contains(READ_ONLY_NOTICE),
        "a non-demoted card must not carry the read-only notice: {card}"
    );
}
