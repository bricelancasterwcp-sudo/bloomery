//! Spec §8 test 8, second half: "with `true` the task degrades".
//!
//! Split out of `api_task_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::task_loop::render_task_prompt;
use common::http;

use common::task::{done_reply, serve_codec_gate_fixture, task_create_request, wait_for_terminal};

/// The squeeze fixture's goal. Held in one place because the window cap is
/// computed from a prompt rendered with it and the POST body must carry the
/// same bytes — a goal that differed between the two would size the cap for a
/// prompt the daemon never renders.
const SQUEEZE_GOAL: &str = "read the big file until the window squeezes";

/// One file this big, read three times, is the whole lever. Each read's
/// observation carries the file whole into the transcript (`ExecBounds::default`
/// caps reads at 256 KiB, sixty times this), so every entry costs ~4 000 chars
/// and step 4's rung-1 and rung-3 renderings end up ~4 000 chars apart — a gap
/// far too wide for an entry header's exact bytes to matter to the sizing
/// below.
const SQUEEZE_FILE_BYTES: usize = 4_000;

/// `pager.rs`'s CHARS_PER_TOKEN and `task_loop.rs`'s STEP_MAX_TOKENS, restated
/// as literals rather than imported for `task_ladder_test.rs`'s stated reason:
/// a sizing computed from the real constants would agree with a mutation of
/// them instead of catching it. If either ever drifts, the `rungs` assertion
/// below fails loudly (a different rung, or no degradation at all) rather than
/// quietly testing nothing.
const CHARS_PER_TOKEN: usize = 3;

const STEP_MAX_TOKENS: usize = 1024;

/// The window cap both squeeze tests give their agent: the task's own prompt
/// plus about two and a half big transcript entries.
///
/// Step 4 (the `done` turn, with three reads behind it) therefore refuses at
/// rung 1 — three full entries, ~12 000 chars — and fits at rung 3, where
/// entry 1 collapses to its header and ~8 000 chars remain. That leaves
/// ~2 000 chars of margin on each side against ~200 chars of unmodeled entry
/// headers and head note, which is what lets this sizing skip a
/// byte-exact model of an entry's header: `task_ladder_test.rs` owns those
/// bytes and this file must not restate them. Rung 2 is never a candidate —
/// with no memory block it renders identically to rung 1 (spec §2), and HTTP
/// has no way to set one.
///
/// The empty-transcript base is the one term too large to approximate, so it
/// is rendered exactly, through the same public `render_task_prompt` the
/// ladder tests size against. It matches what the daemon renders for these
/// tasks — same goal, `SearchReplace`, `EnvelopeLens::V1`, no granted
/// commands, no memory, `mutating_verbs: true` — which is why `serve_squeeze`
/// stores a keep gate before creating its agent.
fn squeeze_window_cap() -> u32 {
    let base = render_task_prompt(
        SQUEEZE_GOAL,
        bloomery_core::action::PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        "",
    );
    let admitted_chars = base.len() + SQUEEZE_FILE_BYTES * 5 / 2;
    u32::try_from(admitted_chars / CHARS_PER_TOKEN + STEP_MAX_TOKENS).expect("cap fits in u32")
}

fn read_reply(path: &str) -> bloomery_substrate::Reply {
    bloomery_substrate::Reply {
        text: format!("<action verb=\"read\" path=\"{path}\">\n</action>"),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// Both squeeze tests' setup, identical down to the scripted turns: three big
/// reads then `done`, a stored keep gate (so the rendered verb card is the
/// mutating one `squeeze_window_cap` sized against, matching
/// `render_task_prompt`'s hardcoded `mutating_verbs: true`), and an agent
/// whose window is that cap. Returns the agent id rather than the pager
/// handle — neither test needs to reach back in after setup.
fn serve_squeeze() -> (
    u16,
    bloomery_daemon::http::ServerHandle,
    std::path::PathBuf,
    String,
) {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![
        read_reply("big.txt"),
        read_reply("big.txt"),
        read_reply("big.txt"),
        done_reply("squeezed through"),
    ]);
    std::fs::write(sandbox.join("big.txt"), "x".repeat(SQUEEZE_FILE_BYTES)).unwrap();

    let cap = squeeze_window_cap();
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
        let info = p.create_agent("qwen", 100, Some(cap), 1_000_000).unwrap();
        // The same guard `task_ladder_test::fixture` keeps: if VRAM or the
        // training ctx ever bound lower than the requested cap, the cap would
        // stop being the lever and these tests would pass or fail for reasons
        // that have nothing to do with the ladder.
        assert_eq!(
            info.window_tokens, cap,
            "the requested cap must be the binding window term (bound_by {})",
            info.bound_by
        );
        info.id
    };

    (port, handle, sandbox, agent_id)
}

/// The control's body plus `"window_ladder": true` and nothing else — built by
/// inserting the key into the very request the ladder-off test posts, so "the
/// field is the only difference between these two tasks" is a fact of
/// construction rather than a claim about two hand-written literals.
fn opted_in_squeeze_request(sandbox: &std::path::Path) -> String {
    let mut v: serde_json::Value =
        serde_json::from_str(&task_create_request(sandbox, SQUEEZE_GOAL)).unwrap();
    v["window_ladder"] = serde_json::Value::Bool(true);
    v.to_string()
}

// ---------------------------------------------------------------------------
// Spec §8 test 8, second half: "with `true` the task degrades". The two tests
// above prove the field is declared and accepted; neither proves its VALUE is
// what `create_task` puts in the `TaskSpec` it spawns — mutate `api_task.rs`'s
// `window_ladder: req.window_ladder` to the literal `false` and both stay
// green, because a declared-but-ignored field parses and 202s exactly like a
// wired one. Only an HTTP-created task that actually degrades can tell those
// apart, so the pair below runs one real squeeze twice: opted in it walks the
// ladder and finishes, opted out (the same request minus the field) it dies.
// ---------------------------------------------------------------------------

/// Spec §5 + §8 test 8: a task that opted in over HTTP hits `PromptTooLarge`
/// on its fourth turn and, instead of dying there, re-renders one rung
/// smaller and finishes — with the rung it actually used visible in
/// `get_task`'s step row (§6). This is the test that fails if
/// `create_task` ever stops passing `req.window_ladder` through to the
/// `TaskSpec` it spawns.
#[test]
fn an_http_task_that_opted_in_degrades_instead_of_dying() {
    let (port, handle, sandbox, agent_id) = serve_squeeze();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &opted_in_squeeze_request(&sandbox),
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
        "an opted-in task must ride the ladder to completion, not die at the \
         first refusal: {last_body}"
    );
    assert_eq!(v["summary"], "squeezed through", "{last_body}");

    let steps = v["steps"].as_array().unwrap();
    let rungs: Vec<u64> = steps
        .iter()
        .map(|s| s["rung"].as_u64().expect("every step row carries a rung"))
        .collect();
    // Steps 1-3 fit as rendered; step 4's three full entries do not, and the
    // walk lands it on rung 3 (rung 2 is identical bytes with no memory
    // block, so it refuses too — spec §2's no-skip rule).
    assert_eq!(
        rungs,
        vec![1, 1, 1, 3],
        "the degraded turn must report the rung it was actually sent at: {last_body}"
    );

    handle.shutdown();
}

/// The control, and the half that makes the test above mean something: the
/// same fixture, the same window, the same four scripted turns, and a request
/// differing only by the absent `"window_ladder"` — which dies
/// `WindowExhausted` on step 4's first refusal (spec §4's ladder-off
/// identity), recording only the three steps that got through.
///
/// Together the two prove the field is load-bearing over the wire in both
/// directions, and they cross-check each other's sizing: a cap too small would
/// break the opted-in test's `Done`, a cap too large would break this one's
/// `WindowExhausted`.
#[test]
fn the_same_http_task_without_the_field_dies_window_exhausted() {
    let (port, handle, sandbox, agent_id) = serve_squeeze();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, SQUEEZE_GOAL),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(
        v["status"], "WindowExhausted",
        "absent → off: the first refusal stays terminal: {last_body}"
    );

    let steps = v["steps"].as_array().unwrap();
    let rungs: Vec<u64> = steps
        .iter()
        .map(|s| s["rung"].as_u64().expect("every step row carries a rung"))
        .collect();
    assert_eq!(
        rungs,
        vec![1, 1, 1],
        "step 4 never produced a row, and no ladder-off step is ever above \
         rung 1: {last_body}"
    );

    handle.shutdown();
}
