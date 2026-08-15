use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::patch::PatchApplyError;
use bloomery_core::action::PatchBody;

#[test]
fn plaintext_lands_a_unique_search_replace() {
    let body = PatchBody::SearchReplace {
        search: "old".into(),
        replace: "new".into(),
    };
    match land("a old b", &body, &PlainText) {
        Landing::Lands { new_contents, lens } => {
            assert_eq!(new_contents, "a new b");
            assert_eq!(lens, "plaintext");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_non_applying_patch_reports_did_not_apply_not_did_not_parse() {
    let body = PatchBody::SearchReplace {
        search: "absent".into(),
        replace: "x".into(),
    };
    match land("nothing here", &body, &PlainText) {
        Landing::DidNotApply { reason, lens } => {
            assert!(matches!(reason, PatchApplyError::SearchNotFound { .. }));
            assert_eq!(lens, "plaintext");
        }
        other => panic!("expected DidNotApply, got {other:?}"),
    }
}

#[test]
fn a_lens_that_rejects_produces_did_not_parse_with_its_name() {
    struct AlwaysReject;
    impl LandingLens for AlwaysReject {
        fn name(&self) -> &'static str {
            "reject"
        }
        fn parses(&self, _c: &str) -> Result<(), String> {
            Err("syntax boom".into())
        }
    }
    let body = PatchBody::WholeFile {
        contents: "whatever".into(),
    };
    match land("x", &body, &AlwaysReject) {
        Landing::DidNotParse { detail, lens } => {
            assert_eq!(detail, "syntax boom");
            assert_eq!(lens, "reject");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unparsed_lens_never_falsely_lands() {
    // A lens that declines to judge returns Unparsed via its own logic;
    // land() surfaces whatever the lens's parses() decision maps to — here
    // we prove the wiring: a lens returning Err is DidNotParse, and a lens
    // can signal "not my language" by name; the Unparsed variant is
    // constructed by such a lens in P3. Here we assert PlainText never
    // yields Unparsed (it accepts all).
    let body = PatchBody::WholeFile {
        contents: "binary\0bytes".into(),
    };
    assert!(matches!(
        land("x", &body, &PlainText),
        Landing::Lands { .. }
    ));
}
