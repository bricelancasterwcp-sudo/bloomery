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
    let r = s.infer(c, "hi", 32).unwrap();
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
    s.infer(c1, "first prompt", 32).unwrap();
    let img = s.save_state(c1).unwrap();
    s.destroy_context(c1).unwrap();
    let c2 = s.create_context(m, 4096).unwrap();
    s.load_state(c2, &img).unwrap();
    assert_eq!(s.ctx_history(c2).unwrap(), "first prompt");
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
