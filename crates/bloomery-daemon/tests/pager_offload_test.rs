//! Task 3, second half: per-model `n_gpu_layers` and the declared
//! weights-VRAM charge that makes partial offload expressible.
//!
//! Split out of `pager_weights_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::{replay, Event};
use bloomery_daemon::pager::*;
use common::pager::{fresh_dir, meta, pager_in, write_gguf};

use common::pager_weights::{KV_BYTES, MIB, WINDOW_CAP};

// ---------------------------------------------------------------------
// Task 3: per-model n_gpu_layers + declared weights-VRAM charge.
// ---------------------------------------------------------------------

/// `create_agent`'s window law uses the declared, override value — not the
/// file's raw `weights_bytes` — as `GeometryInput.weights_bytes` (Task 3,
/// spec §3). Asymmetric numbers (declared 100 MiB, file 300 MiB, free VRAM
/// 500 MiB) are chosen so the window's bound (`training_ctx` vs `vram`) and
/// exact token count diverge sharply between the two: with declared (100
/// MiB) charged, `(500-100)/kv_per_token = 7314` tokens exceeds
/// `training_ctx` (4096), so the window is `training_ctx`-bound at exactly
/// 4096; with the file's raw weights (300 MiB) charged instead, `(500-300)
/// /kv_per_token = 3657` tokens is UNDER `training_ctx`, so the window would
/// instead be `vram`-bound at exactly 3657. No placement call happens in
/// this test at all, so it is sensitive to exactly one charge site:
/// `create_agent`'s `GeometryInput.weights_bytes`.
#[test]
fn create_agent_window_uses_the_declared_weights_not_the_file() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-window");
    let free_vram = 500 * MIB;
    let file = 300 * MIB;
    let declared = 100 * MIB;
    let (mut p, _, _) = pager_in(&dir, 0, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    // No window_cap: the window is purely training_ctx-vs-vram bound, so
    // this is a clean read of which weight value the geometry math used.
    let a1 = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        a1.window_tokens, 4096,
        "training_ctx (4096) must win over vram (7314 tokens at declared 100 MiB) \
         — if the geometry read the file's 300 MiB instead, vram (3657) would win"
    );
    assert_eq!(a1.bound_by, "training_ctx");
}

/// `place`'s demand term (cold-model load) AND the reservation budget's
/// supply term (`loaded_weights_bytes`) both use the declared, override
/// value — not the file's raw `weights_bytes` (Task 3, spec §3). One model,
/// declared 100 MiB vs file 300 MiB, free VRAM 212 MiB exactly, chosen so:
///
/// - a1 (cap 1024 tokens, 56 MiB ctx) loads the model cold: demand is
///   `weights_term + 56 MiB`. At declared (100), demand is 156 MiB <= 212
///   MiB budget: fits outright (nothing resident yet to evict). At the file
///   value (300), demand would be 356 MiB > 212 MiB: a hard refusal (no
///   resident to evict). This exercises `place`'s demand term.
/// - a2 (cap 2048 tokens, 112 MiB ctx, same already-loaded model so its own
///   demand carries no weights term) then needs `avail + reclaimable >=
///   112 MiB`. At declared (100) loaded, `avail = 212 - 100 - 56 = 56 MiB`;
///   `+ reclaimable (a1's 56 MiB kv) = 112 MiB` — an EXACT fit via evicting
///   a1. At the file value (300) loaded, `avail` saturates to 0 MiB;
///   `+ reclaimable (56) = 56 MiB < 112 MiB`: refused even after evicting
///   everything reclaimable. This exercises `loaded_weights_bytes`, the
///   supply-side site.
///
/// Each of the two `infer` calls below therefore only succeeds if its own
/// charge site reads the declared value — a one-sided wiring bug on either
/// site fails exactly one of the two assertions.
#[test]
fn placement_uses_the_declared_weights_not_the_file() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-placement");
    let free_vram = 212 * MIB;
    let file = 300 * MIB;
    let declared = 100 * MIB;
    let (mut p, _, _) = pager_in(&dir, 2, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p.create_agent("qwen", 50, Some(1024), 10_000).unwrap();
    let a2 = p.create_agent("qwen", 100, Some(2048), 10_000).unwrap();

    p.infer(&a1.id, "hello from a1", 16, None).expect(
        "demand = declared (100 MiB) + ctx (56 MiB) = 156 MiB <= 212 MiB budget: fits \
         without eviction — place's demand term must read the declared value",
    );
    p.infer(&a2.id, "hello from a2", 16, None).expect(
        "avail (56 MiB, from budget - declared 100 - a1's 56 MiB) + reclaimable (a1's \
         56 MiB) == demand (112 MiB) exactly: evicts a1, fits — loaded_weights_bytes \
         must read the declared value on the supply side",
    );
}

/// Clamp: a declared value LARGER than the file's own `weights_bytes` is
/// clamped down to the file value — a misconfigured, over-large declaration
/// must never inflate the charge past physical reality (spec §3's
/// `min(declared, weights_bytes)`). Free VRAM is set to exactly `file +
/// ctx`, so this only fits if the charge is clamped to `file`; the
/// (uncapped) declared value would blow the budget by nearly 800 MiB.
#[test]
fn declared_larger_than_file_clamps_to_file_no_inflation() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-clamp");
    let file = 200 * MIB;
    let declared = 999 * MIB;
    let free_vram = file + KV_BYTES; // fits iff the charge clamps to `file`
    let (mut p, _, _) = pager_in(&dir, 1, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello", 16, None).expect(
        "budget == file + ctx exactly: fits only if the 999 MiB declared value was \
         clamped down to the 200 MiB file value, not charged raw",
    );

    assert_eq!(
        p.status().loaded_weights_bytes,
        file,
        "the clamp applies to the loaded/status sum too — never the raw declared value"
    );
}

/// No override at all: `create_agent`, `place`, and `/status` all charge
/// the model's full file weight — byte-identical to today's behavior. An
/// EXPLICIT `set_model_tuning(model, None, None)` call (as opposed to never
/// calling it) must be an equivalent no-op, since spec §2 says omitting
/// both tuning fields in config is byte-for-byte identical to a bare-path
/// entry.
#[test]
fn explicit_no_override_tuning_call_is_a_full_charge_no_op() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-noop");
    let budget = 300 * MIB;
    let weights = 200 * MIB;
    let (mut p, _, _) = pager_in(&dir, 1, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_model_tuning("qwen", None, None)
        .expect("explicit no-op tuning call on a known model must succeed");

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello", 16, None)
        .expect("weights (200) + ctx (56) = 256 <= 300: full charge, no override applied");

    assert_eq!(
        p.status().loaded_weights_bytes,
        weights,
        "declared absent -> full file weight, exactly today's behavior"
    );
}

/// `/status`'s `loaded_weights_bytes` sum reflects the declared value when
/// an override is active — it flows through `loaded_weights_bytes`
/// automatically (the same method the supply side of `place` reads), so
/// this pins that the fourth charge site really does inherit the fix rather
/// than needing its own wiring.
#[test]
fn status_reports_declared_weights_when_override_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-status");
    let budget = 300 * MIB;
    let file = 300 * MIB;
    let declared = 120 * MIB;
    let (mut p, _, _) = pager_in(&dir, 1, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    assert_eq!(p.status().loaded_weights_bytes, 0, "nothing loaded yet");
    p.infer(&a1.id, "hello", 16, None).unwrap();

    assert_eq!(
        p.status().loaded_weights_bytes,
        declared,
        "declared (120 MiB), not the file's 300 MiB, must reach /status"
    );
}

/// The refusal string names the weights term "declared" exactly when an
/// override is active for the model being refused, and never fabricates
/// that label when there is none — spec §3: "a declared number must never
/// read as a measured one."
#[test]
fn refusal_names_declared_when_override_is_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-refusal-declared");
    let file_a = 100 * MIB;
    let file_b = 500 * MIB;
    let declared_b = 90 * MIB;
    let budget = 160 * MIB;
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(file_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(file_b), None)
        .unwrap();
    p.set_model_tuning("modelB", None, Some(declared_b))
        .unwrap();

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("modelA: weights (100) + ctx (56) = 156 <= 160: fits");

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!(
            "expected modelB refused (declared 90 + ctx 56 = 146, avail only 4 \
                          + reclaimable 56 = 60 < 146), got {other:?}"
        ),
    }

    let events = replay(&jpath).unwrap();
    let expected_clause =
        format!("weights {declared_b} B (declared weights_vram_mib; file {file_b} B)");
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id && detail.contains(&expected_clause))),
        "expected refusal detail to contain {expected_clause:?}: {events:?}"
    );
}

/// The mirror of the previous test: no override on the refused model means
/// the refusal string must NOT contain "declared" anywhere — the weights
/// term is plainly the file's own measured value.
#[test]
fn refusal_omits_declared_when_no_override_is_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-refusal-no-declared");
    let file_a = 100 * MIB;
    let file_b = 150 * MIB;
    let budget = 160 * MIB;
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(file_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(file_b), None)
        .unwrap();
    // No set_model_tuning call for modelB at all.

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("modelA: weights (100) + ctx (56) = 156 <= 160: fits");

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected modelB refused, got {other:?}"),
    }

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id
                    && detail.contains(&format!("weights {file_b} B"))
                    && !detail.contains("declared"))),
        "no override on modelB: the refusal must not say 'declared' anywhere: {events:?}"
    );
}

/// `set_model_tuning` on an unregistered model name is refused with
/// `UnknownModel`, naming the model — same contract as `attach_profile` and
/// `register_model`'s own re-registration guard.
#[test]
fn set_model_tuning_on_unknown_model_is_refused() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(300 * MIB));
    match p.set_model_tuning("nope", Some(1), Some(1)) {
        Err(PagerError::UnknownModel(name)) => assert_eq!(name, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

/// A per-model `n_gpu_layers` override wins over the pager-global default
/// at the `load_model` call; a model with no override gets the pager-global
/// default (`u32::MAX`, unless `Pager::set_n_gpu_layers` was called — it
/// wasn't, in this test, so this also pins that default's own value).
#[test]
fn per_model_n_gpu_layers_override_wins_absent_override_uses_global_default() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-ngpulayers");
    let budget = 1000 * MIB; // generous: placement isn't the point here
    let (mut p, _, _) = pager_in(&dir, 2, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(50 * MIB), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(50 * MIB), None)
        .unwrap();
    p.set_model_tuning("modelA", Some(28), None).unwrap();
    // modelB: no tuning call at all -> pager-global default.

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let b1 = p
        .create_agent("modelB", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hi", 8, None).unwrap();
    p.infer(&b1.id, "hi", 8, None).unwrap();

    assert_eq!(
        p.substrate().load_n_gpu_layers(),
        &[28, u32::MAX],
        "modelA's override (28) must win; modelB with no override must get the \
         pager-global default (u32::MAX)"
    );
}
