use bloomery_substrate::{contract::*, fake::FakeSubstrate, Reply, Substrate};

fn ok_reply(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(10),
        completion_tokens: Some(3),
        duration_ms: 5,
    }
}

#[test]
fn fake_serves_scripted_replies_and_logs_calls() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply("hello"));
    let m = s
        .load_model(std::path::Path::new("/fake.gguf"), 99)
        .unwrap();
    let c = s.create_context(m, 4096).unwrap();
    let r = s.infer(c, "hi", 32, None).unwrap();
    assert_eq!(r.text, "hello");
    assert!(s.calls().iter().any(|x| x.starts_with("infer")));
}

#[test]
fn state_round_trip_preserves_context_history() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply("a"));
    let m = s
        .load_model(std::path::Path::new("/fake.gguf"), 99)
        .unwrap();
    let c1 = s.create_context(m, 4096).unwrap();
    s.infer(c1, "first prompt", 32, None).unwrap();
    let img = s.save_state(c1).unwrap();
    s.destroy_context(c1).unwrap();
    let c2 = s.create_context(m, 4096).unwrap();
    s.load_state(c2, &img).unwrap();
    assert_eq!(s.ctx_history(c2).unwrap(), "first prompt");
}

// ---------------------------------------------------------------------------
// Protocol §11 (Amendment 3): envelope-v3's `stop` sequence.
// ---------------------------------------------------------------------------

/// A scripted reply carrying TWO `<action>` blocks plus trailing prose,
/// infer'd with `stop = Some("</action>")`, must come back truncated to
/// exactly the first block, tag INCLUDED — the second block gone entirely.
#[test]
fn infer_with_a_stop_sequence_truncates_at_the_first_inclusive_occurrence() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply(
        "<action verb=\"done\">\nfirst\n</action>\nSome trailing prose.\n\
         <action verb=\"done\">\nsecond\n</action>",
    ));
    let m = s
        .load_model(std::path::Path::new("/fake.gguf"), 99)
        .unwrap();
    let c = s.create_context(m, 4096).unwrap();

    let r = s.infer(c, "go", 64, Some("</action>")).unwrap();

    assert!(
        r.text.ends_with("</action>"),
        "must end exactly at the stop tag: {:?}",
        r.text
    );
    assert_eq!(
        r.text, "<action verb=\"done\">\nfirst\n</action>",
        "must truncate at exactly the first inclusive occurrence, nothing more"
    );
    assert!(
        !r.text.contains("second"),
        "the second action block must be gone entirely: {:?}",
        r.text
    );
}

/// `stop = None` (today's behavior, and every non-v3 caller) leaves the
/// scripted reply untouched — both action blocks survive.
#[test]
fn infer_with_no_stop_sequence_leaves_the_reply_untouched() {
    let mut s = FakeSubstrate::new();
    let full =
        "<action verb=\"done\">\nfirst\n</action>\n<action verb=\"done\">\nsecond\n</action>";
    s.script_reply(ok_reply(full));
    let m = s
        .load_model(std::path::Path::new("/fake.gguf"), 99)
        .unwrap();
    let c = s.create_context(m, 4096).unwrap();

    let r = s.infer(c, "go", 64, None).unwrap();

    assert_eq!(r.text, full);
}

/// Every `stop` value passed to `infer` is recorded per call, in order —
/// the mechanism `api_v1`/`api_native`'s "always None" pins ride on.
#[test]
fn infer_stops_are_recorded_per_call_in_order() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply("a"));
    s.script_reply(ok_reply("b"));
    let m = s
        .load_model(std::path::Path::new("/fake.gguf"), 99)
        .unwrap();
    let c = s.create_context(m, 4096).unwrap();

    s.infer(c, "go", 8, None).unwrap();
    s.infer(c, "go", 8, Some("</action>")).unwrap();

    assert_eq!(s.infer_stops(), &[None, Some("</action>".to_string())]);
}

#[test]
fn contract_rejects_missing_stats() {
    let bad = Reply {
        text: "plausible".into(),
        prompt_tokens: None,
        completion_tokens: None,
        duration_ms: 9,
    };
    assert_eq!(enforce_contract(bad), Err(ContractViolation::MissingStats));
    let good = enforce_contract(ok_reply("x")).unwrap();
    assert_eq!((good.prompt_tokens, good.completion_tokens), (10, 3));
}
