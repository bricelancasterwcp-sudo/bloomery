//! The task surface's codec resolution: `create_task` resolving the real
//! per-model patch codec (Task 8), the envelope-v2 think-preseeded lens
//! (protocol §10, Amendment 2), and the window ladder
//! (`docs/superpowers/specs/2026-08-27-window-ladder-design.md`).
//!
//! Split out of `api_task_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_daemon::config::EnvelopeLens;
use common::http;

use common::task::{done_reply, serve_codec_gate_fixture, task_create_request, wait_for_terminal};

/// A `Profile` whose `codecs` grid picks `WholeFile` over `SearchReplace`
/// for "qwen" — the same fixture `pager_codec_gate_test.rs` uses to prove
/// `Pager::model_patch_codec`'s selection (protocol §4).
const WF_WINS_PROFILE: &str = r#"{
  "assay_profile_version": 3,
  "probe_version": "0.4.1",
  "model": {"name": "qwen"},
  "codecs": {
    "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 20}},
    "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 20}}
  }
}"#;

/// The `WholeFile` codec's worked `patch` example, verbatim from
/// `bloomery_core::action::card`'s private `WHOLE_FILE_PATCH_EXAMPLE` — that
/// constant isn't `pub`, so this is the same bytes duplicated at the
/// boundary this test actually observes (the rendered prompt), the same way
/// `task_loop_test.rs` duplicates its own scripted `<action>` bodies rather
/// than reaching into `bloomery-core`'s private internals.
const WHOLE_FILE_PATCH_EXAMPLE: &str = "<action verb=\"patch\" path=\"src/lib.rs\">\nfn greeting() -> &'static str { \"hello\" }\n</action>";

/// The pinned gate-G4 refusal outcome (P4 Task 7 brief — exact bytes; Task
/// 9's scoring and the journal read this string). Duplicated locally for the
/// same reason `task_loop_test.rs` duplicates it rather than importing a
/// private `task_loop` constant.
const MUTATING_VERB_DEMOTED: &str = "verb unavailable: mutating verbs demoted (gate G4)";

// ---------------------------------------------------------------------------
// Task 8: `create_task` resolves the real per-model patch codec and G4 verb
// policy through `Pager::agent_task_policy` (closes the carried-debt item
// "Profile has NO codec field") instead of the `PatchCodec::SearchReplace` +
// `mutating_verbs: true` literal this task replaced.
// ---------------------------------------------------------------------------

/// Test (a): a model with an attached wf-wins profile AND a stored keep gate
/// (mutating verbs on — otherwise the demoted read-only card would drop the
/// `patch` section entirely and there would be no patch example to select
/// between) gets tasks whose verb card shows the `WholeFile` patch example,
/// not the `SearchReplace` default `create_task` used to hardcode. Observed
/// via `FakeSubstrate::ctx_history` — the harness's existing seam
/// (`api_v1_test.rs::x_bloomery_agent_header_reuses_the_same_substrate_context`)
/// — since the rendered prompt is exactly what the model turn receives, sent
/// before the scripted reply even matters.
#[test]
fn a_wf_wins_profile_with_a_keep_gate_selects_the_whole_file_patch_example() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        let profile = bloomery_core::profile::Profile::from_json(WF_WINS_PROFILE).unwrap();
        p.attach_profile("qwen", profile, false).unwrap();
        p.set_codec_gate(
            "qwen",
            bloomery_daemon::pager::CodecGateResult {
                fixture_set: "codec-tasks-v1".to_string(),
                codec: bloomery_core::action::PatchCodec::SearchReplace,
                landed: 17,
                n: 20,
                interval95: (0.60, 0.94),
                provisional: false,
                mutating_verbs: true,
            },
        )
        .unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        history.contains(WHOLE_FILE_PATCH_EXAMPLE),
        "expected the WholeFile patch example in the verb card the model was \
         prompted with, got: {history}"
    );
    assert!(
        !history.contains("<<<<<<< SEARCH"),
        "a WholeFile-selected card must never also carry the SearchReplace \
         conflict-marker example: {history}"
    );
    drop(p);

    handle.shutdown();
}

/// Test (b): an unmeasured model (no `set_codec_gate` call at all — protocol
/// §3/§6's fail-closed default) still gets a task created (`202`), but a
/// scripted `patch` turn records the pinned G4 refusal rather than executing
/// — proving the fail-closed default set at agent-admission time actually
/// reaches `run_task`'s dispatch gate through this HTTP route, not just
/// through `Pager::agent_task_policy` in isolation (`pager_codec_gate_test.rs`
/// already covers that half).
#[test]
fn an_unmeasured_model_is_created_but_its_patch_turn_is_refused_by_gate_g4() {
    let patch_attempt = bloomery_substrate::Reply {
        text: "<action verb=\"patch\" path=\"file.txt\">\n\
               <<<<<<< SEARCH\n\
               hello\n\
               =======\n\
               goodbye\n\
               >>>>>>> REPLACE\n\
               </action>"
            .to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    };
    let (port, handle, sandbox, pager) =
        serve_codec_gate_fixture(vec![patch_attempt, done_reply("refused as expected")]);
    std::fs::write(sandbox.join("file.txt"), "hello\nworld\n").unwrap();
    let addr = format!("127.0.0.1:{port}");

    // No `set_codec_gate` call at all — this model is unmeasured, which
    // `agent_task_policy` must resolve to `mutating_verbs: false`.
    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "patch the file"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(
        v["status"], "Done",
        "a refused verb must not abort the task: {last_body}"
    );
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{last_body}");
    assert_eq!(steps[0]["verb"], "patch", "must record the real verb name");
    assert_eq!(steps[0]["failed"], true);
    assert_eq!(steps[0]["outcome"], MUTATING_VERB_DEMOTED);
    assert_eq!(steps[1]["verb"], "done");

    let on_disk = std::fs::read_to_string(sandbox.join("file.txt")).unwrap();
    assert_eq!(
        on_disk, "hello\nworld\n",
        "an unmeasured model's refused patch must never touch the file"
    );

    handle.shutdown();
}

/// Test (c): a model with a stored keep gate (`mutating_verbs: true`) gets a
/// patch turn that actually executes — the counterpart to test (b), proving
/// `create_task` reaches the real per-model verdict in both directions, not
/// just the fail-closed one.
#[test]
fn a_stored_keep_gate_lets_the_patch_turn_execute_for_real() {
    let patch_attempt = bloomery_substrate::Reply {
        text: "<action verb=\"patch\" path=\"file.txt\">\n\
               <<<<<<< SEARCH\n\
               hello\n\
               =======\n\
               goodbye\n\
               >>>>>>> REPLACE\n\
               </action>"
            .to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    };
    let (port, handle, sandbox, pager) =
        serve_codec_gate_fixture(vec![patch_attempt, done_reply("patched it")]);
    std::fs::write(sandbox.join("file.txt"), "hello\nworld\n").unwrap();
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.set_codec_gate(
            "qwen",
            bloomery_daemon::pager::CodecGateResult {
                fixture_set: "codec-tasks-v1".to_string(),
                codec: bloomery_core::action::PatchCodec::SearchReplace,
                landed: 17,
                n: 20,
                interval95: (0.60, 0.94),
                provisional: false,
                mutating_verbs: true,
            },
        )
        .unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "patch the file"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{last_body}");
    assert_eq!(steps[0]["verb"], "patch");
    assert_eq!(steps[0]["failed"], false, "{last_body}");
    assert!(
        steps[0]["outcome"]
            .as_str()
            .unwrap_or_default()
            .starts_with("patched (lens:"),
        "{last_body}"
    );

    let on_disk = std::fs::read_to_string(sandbox.join("file.txt")).unwrap();
    assert_eq!(
        on_disk, "goodbye\nworld\n",
        "a keep-gated model's patch must actually land"
    );

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Protocol §10, Amendment 2: envelope-v2 (think-preseeded) tasks over HTTP
// ---------------------------------------------------------------------------

/// Test (d): a model configured `think_preseed = true`
/// (`Pager::set_think_preseed`) gets its HTTP-created tasks rendering with
/// the literal pre-seed appended — resolved through the same
/// `Pager::agent_task_policy` one-source triple `patch_codec`/
/// `mutating_verbs`/`think_preseed` already flows through `create_task`
/// (closing the same "one policy source" rule test (a) proved for
/// `patch_codec`, now for the third field). Observed the same way test (a)
/// does: `FakeSubstrate::ctx_history` holds exactly the one rendered prompt
/// the model was sent, since this task's only scripted turn is `done`.
#[test]
fn a_think_preseed_model_renders_its_task_prompt_with_the_preseed_literal() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.set_model_envelope("qwen", EnvelopeLens::V2).unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        history.ends_with("<think>\n\n</think>\n\n"),
        "expected the rendered prompt to end with the think-preseed literal, \
         got: {history}"
    );
    drop(p);

    handle.shutdown();
}

/// The counterpart: a model with no `think_preseed` configured (the
/// default, `false`) never carries the literal — `create_task`'s policy
/// triple resolves the flag off just as reliably as it resolves it on.
#[test]
fn a_non_preseeded_model_never_renders_the_preseed_literal_over_http() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        !history.contains("<think>\n\n</think>\n\n"),
        "a non-preseeded model must never carry the literal, got: {history}"
    );
    drop(p);

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Window ladder (docs/superpowers/specs/2026-08-27-window-ladder-design.md
// §5): the REQUEST half of the wire. The ladder's own behavior — which rung
// a refused prompt lands on, what it elides, when it stays terminal — is
// owned by `task_ladder_test.rs` against `run_task` directly; the first two
// tests below pin only that `"window_ladder"` is a real, typed field of the
// create-task request, and the pair after them pins the one thing no
// in-process test can — that the field's VALUE reaches the spawned task's
// `TaskSpec`. The RESPONSE half (§6: every step object carries its `rung`) is
// pinned above, inside `a_task_runs_and_is_pollable_to_done`, where the
// ladder-off default it asserts is the same default that test's request
// already exercised — a second create-and-poll would re-assert it verbatim.
// ---------------------------------------------------------------------------

/// Spec §5: a live task opts in over HTTP. The field parses, the request is
/// accepted (`202`), and the task still runs to `Done` — an opt-in whose
/// prompts all fit is byte-identical work at rung 1, which its step row then
/// reports (spec §2: rung 1 IS today's rendering).
#[test]
fn create_task_accepts_window_ladder_true() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, vec![done_reply("ladder on")]);
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_req = serde_json::json!({
        "goal": "say done",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
        "window_ladder": true,
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");
    assert_eq!(v["summary"], "ladder on", "{last_body}");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{last_body}");
    assert_eq!(steps[0]["verb"], "done");
    assert_eq!(
        steps[0]["rung"], 1,
        "opting in must not degrade a prompt that already fits: {last_body}"
    );

    handle.shutdown();
}

/// The companion that keeps the test above from being vacuous: `CreateTaskReq`
/// declares no `#[serde(deny_unknown_fields)]`, so a `202` alone would be
/// just as green if `window_ladder` were never a field at all and serde
/// silently dropped it. A non-boolean value is refused with this route's one
/// `400 bad_request` shape — which only a really-declared `bool` field can
/// produce, making this the assertion that fails if the request wiring is
/// ever removed.
#[test]
fn a_non_boolean_window_ladder_is_400() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, vec![]);
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    // A real agent, so the refusal below can only be the field's type: an
    // otherwise-identical body with `true` is the `202` above.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_req = serde_json::json!({
        "goal": "say done",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
        "window_ladder": "yes",
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "bad_request");
    assert!(
        v["message"]
            .as_str()
            .unwrap_or_default()
            .contains("boolean"),
        "the parse must have failed on the bool field itself: {body}"
    );

    handle.shutdown();
}
