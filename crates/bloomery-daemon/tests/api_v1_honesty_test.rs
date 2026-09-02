//! `/v1`'s honesty table (2026-08-31): every field whose meaning this shim
//! would otherwise discard is refused by name, with `param` set.
//!
//! The governing rule is **accept the no-op value, refuse the meaningful
//! one** -- a value describing what bloomery already does is honest to
//! accept, so `tools: []`, `temperature: 0`, `top_p: 1` and `n: 1` all still
//! pass. The accept-side tests are not decoration: they are the guard against
//! over-rejection, which would be its own dishonesty, and they are what kills
//! the mutants that widen a refusal too far.
//!
//! Split out of `api_v1_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use common::http;

/// A minimal valid chat body with `extra` spliced in as further top-level
/// fields, e.g. `r#""temperature":0.8"#`.
fn chat_req(extra: &str) -> String {
    let base = r#""model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16"#;
    if extra.is_empty() {
        format!("{{{base}}}")
    } else {
        format!("{{{base},{extra}}}")
    }
}

fn assert_refused(extra: &str, param: &str) {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", &chat_req(extra));
    assert_eq!(st, 400, "expected refusal for {extra}, got: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error", "{body}");
    assert_eq!(v["error"]["code"], "unsupported_parameter", "{body}");
    assert_eq!(v["error"]["param"], param, "{body}");
}

fn assert_accepted(extra: &str) {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", &chat_req(extra));
    assert_eq!(
        st, 200,
        "expected {extra} to be accepted as a no-op, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// The rest of the honesty table (2026-08-31). Same rule as `tools`: a field
// whose value describes what bloomery already does is honest to accept; a
// value that would change the reply is refused by name rather than dropped.
// ---------------------------------------------------------------------------

#[test]
fn tool_choice_auto_is_refused() {
    assert_refused(r#""tool_choice":"auto""#, "tool_choice");
}

#[test]
fn legacy_functions_field_is_refused() {
    assert_refused(r#""functions":[{"name":"f"}]"#, "functions");
}

#[test]
fn nonzero_temperature_is_refused_because_the_sampler_is_greedy() {
    assert_refused(r#""temperature":0.8"#, "temperature");
}

#[test]
fn top_p_below_one_is_refused() {
    assert_refused(r#""top_p":0.9"#, "top_p");
}

#[test]
fn n_greater_than_one_is_refused() {
    assert_refused(r#""n":2"#, "n");
}

#[test]
fn stop_sequences_are_refused_because_infer_takes_none() {
    assert_refused(r#""stop":["END"]"#, "stop");
}

#[test]
fn json_response_format_is_refused() {
    assert_refused(
        r#""response_format":{"type":"json_object"}"#,
        "response_format",
    );
}

#[test]
fn logprobs_true_is_refused() {
    assert_refused(r#""logprobs":true"#, "logprobs");
}

// --- the accept side: these values ARE what bloomery does, so refusing them
// --- would be its own dishonesty. Guards against over-rejection.

#[test]
fn empty_tools_array_is_accepted_it_asks_for_nothing() {
    assert_accepted(r#""tools":[]"#);
}

#[test]
fn temperature_zero_is_accepted_it_is_what_greedy_means() {
    assert_accepted(r#""temperature":0"#);
}

#[test]
fn top_p_one_and_n_one_are_accepted() {
    assert_accepted(r#""top_p":1,"n":1"#);
}

#[test]
fn text_response_format_and_false_logprobs_are_accepted() {
    assert_accepted(r#""response_format":{"type":"text"},"logprobs":false"#);
}

// --- message-shape refusals

#[test]
fn tool_role_message_is_refused() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let req = r#"{"model":"qwen","messages":[{"role":"tool","content":"result","tool_call_id":"c1"}],"max_tokens":16}"#;
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", req);
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], "unsupported_parameter", "{body}");
    assert_eq!(v["error"]["param"], "messages[].role", "{body}");
}

#[test]
fn assistant_message_carrying_tool_calls_is_refused() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let req = r#"{"model":"qwen","messages":[{"role":"assistant","content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"f","arguments":"{}"}}]}],"max_tokens":16}"#;
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", req);
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], "unsupported_parameter", "{body}");
    assert_eq!(v["error"]["param"], "messages[].tool_calls", "{body}");
}

/// Null content is legal OpenAI (it pairs with `tool_calls`). Today it is a
/// serde parse failure reported as `invalid_json`, which tells the caller
/// nothing about why. Name it.
#[test]
fn null_content_is_refused_by_name_not_as_a_json_parse_error() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let req = r#"{"model":"qwen","messages":[{"role":"user","content":null}],"max_tokens":16}"#;
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", req);
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], "unsupported_parameter", "{body}");
    assert_eq!(v["error"]["param"], "messages[].content", "{body}");
}

/// Multimodal content parts, same treatment.
#[test]
fn array_content_parts_are_refused_by_name() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let req = r#"{"model":"qwen","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}],"max_tokens":16}"#;
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", req);
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], "unsupported_parameter", "{body}");
    assert_eq!(v["error"]["param"], "messages[].content", "{body}");
}
