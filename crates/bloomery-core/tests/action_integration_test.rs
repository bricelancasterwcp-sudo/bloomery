//! Task 5: round-trip integration across the whole action codec — every verb
//! parsing from a realistic turn (narration + one action), the verb card's
//! content contract, and the multiple-actions diagnostic surfacing through
//! the same entry point the daemon's task loop (P3) will call.

use bloomery_core::action::card::verb_card;
use bloomery_core::action::{parse_action_with_codec, Action, ActionError, PatchBody, PatchCodec};

#[test]
fn every_verb_round_trips_from_a_realistic_turn() {
    // A turn with narration + one action, for each verb, asserting the parse.
    let read = "Let me look.\n<action verb=\"read\" path=\"src/lib.rs\" lines=\"1-40\">\n</action>";
    assert!(matches!(
        parse_action_with_codec(read, PatchCodec::SearchReplace).unwrap(),
        Action::Read { .. }
    ));

    let find = "I'll search for the definition first.\n<action verb=\"find\" pattern=\"fn parse_action\" path=\"src\">\n</action>";
    match parse_action_with_codec(find, PatchCodec::SearchReplace).unwrap() {
        Action::Find { pattern, path } => {
            assert_eq!(pattern, "fn parse_action");
            assert_eq!(path, "src");
        }
        other => panic!("expected Find, got {other:?}"),
    }

    let run =
        "Running the test suite now.\n<action verb=\"run\">\n[\"cargo\", \"test\"]\n</action>";
    match parse_action_with_codec(run, PatchCodec::SearchReplace).unwrap() {
        Action::Run { argv } => assert_eq!(argv, vec!["cargo".to_string(), "test".to_string()]),
        other => panic!("expected Run, got {other:?}"),
    }

    let patch = "Here's a whole-file replacement for the config.\n<action verb=\"patch\" path=\"bloomery.toml\">\nport = 8181\n</action>";
    match parse_action_with_codec(patch, PatchCodec::WholeFile).unwrap() {
        Action::Patch { path, body } => {
            assert_eq!(path, "bloomery.toml");
            assert_eq!(
                body,
                PatchBody::WholeFile {
                    contents: "port = 8181".into()
                }
            );
        }
        other => panic!("expected Patch, got {other:?}"),
    }

    let done = "<action verb=\"done\">\nall tests pass\n</action>";
    assert!(matches!(
        parse_action_with_codec(done, PatchCodec::SearchReplace).unwrap(),
        Action::Done { .. }
    ));
}

#[test]
fn the_verb_card_names_every_verb_and_the_one_action_rule() {
    let card = verb_card(PatchCodec::SearchReplace);
    for v in ["read", "find", "patch", "run", "done"] {
        assert!(card.contains(v), "card missing verb {v}");
    }
    assert!(card.to_lowercase().contains("one action"));
    assert!(card.contains("<<<<<<< SEARCH")); // the SR example
    let wf = verb_card(PatchCodec::WholeFile);
    assert!(!wf.contains("<<<<<<< SEARCH")); // whole-file card shows the other example
}

#[test]
fn multiple_actions_in_one_turn_is_a_single_named_error() {
    let turn = "<action verb=\"read\" path=\"a\">\n</action>\n<action verb=\"done\">\nx\n</action>";
    assert!(matches!(
        parse_action_with_codec(turn, PatchCodec::SearchReplace).unwrap_err(),
        ActionError::MultipleActions { found: 2 }
    ));
}
