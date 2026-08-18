# Verdict-gated admission (seam slice 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a confirmed cumulative drift regression refuse new admission, with an operator-clearable block that never rewrites the measurement it came from.

**Architecture:** Slice 1 measures drift into `ModelDrift { step, cumulative }` and explicitly never acts on it. This slice adds `admission_block: Option<AdmissionBlock>` to the pager's per-model entry, set by the drift watch when — and only when — `cumulative` settles `DriftStatus::Confirmed`. `admit()` gains one clause consulting it. Recovery is a new `POST /models/{name}/unblock` that clears the block and journals the clearing, leaving both the reading and the blessed baseline untouched.

**Tech Stack:** Rust (workspace crates `bloomery-core`, `bloomery-daemon`), stdlib + serde. Tests are **integration** tests under `crates/bloomery-daemon/tests/` — `drift_test.rs` and `pager_test.rs` are the two this slice extends.

**Everything these tests touch must be `pub`.** Integration tests link against the crate from outside, so `admission_block_for`, `clear_admission_block`, `AdmissionBlock` and its `reference` field, and `ModelStatus.admission_block` are all public API, not `pub(crate)`. This is the single easiest way to write a task in this plan that does not compile.

**The real construction idioms**, copied from `tests/drift_test.rs:108` and `tests/pager_test.rs:99` — use these, do not invent helpers:

```rust
let mut pager = Pager::new(
    FakeSubstrate::new(),
    Journal::open(&jpath).expect("journal"),
    ImageStore::new(&dir.join("img")).expect("image store"),
    Box::new(|| Some(10u64.pow(9))),
);
let gguf = dir.join("qwen.gguf");
std::fs::write(&gguf, b"weights").unwrap();
pager.register_model("qwen", &gguf, qwen_like_meta(), None).unwrap();

// agent creation: (model, priority, window_cap, budget_tokens)
let agent = pager.create_agent("qwen", 50, None, 10_000).unwrap();
```

`scratch(tag)`, `store_in(tag)`, `qwen_like_meta()` and `scripted_assay()` already exist in `tests/drift_test.rs`; `Pager` comes from `bloomery_daemon::pager::Pager`, the drift types from `bloomery_daemon::drift::{...}`, `FakeSubstrate` from `bloomery_substrate::fake`. A model needs a profile before drift means anything — follow how `drift_test.rs` gets one onto a model rather than reaching for a shortcut.

**Spec:** [`docs/superpowers/specs/2026-08-18-verdict-gated-admission-design.md`](../specs/2026-08-18-verdict-gated-admission-design.md)

## Global Constraints

- **Only `cumulative` blocks; `step` never does.** `ModelDrift` carries both. Spec §2.
- **Only `DriftStatus::Confirmed` blocks.** The other six admit: `WithinNoise`, `Transient`, `Unconfirmed`, `NotComparable`, `InstrumentChanged`, `Unmeasured`. Principle for any future eighth: refuse only what was established; name everything else.
- **`InstrumentChanged` must never block.** assay v1.8 (0.10.0/v9) lands against blessed v8 references, so the first boot after that merge reads `InstrumentChanged` on every model at once. A slice that blocked on it would take the fleet out on a routine instrument upgrade. This is the single most important test in the plan.
- **The reading is immutable.** No operator action may rewrite `ModelDrift`. Slice 1's rule: "a comparison nobody re-ran must never acquire a new verdict."
- **`done_trust`, `codec_gate`, and the G4/G5 gates are untouched.** They remain the sole property of their own gates.
- **The gate is at agent creation, never per inference.** An agent admitted before a block appeared keeps working.
- **`PagerError` variants must be mapped on BOTH surfaces** — `api_native.rs` (bare JSON) and `api_v1.rs` (`error_envelope`). One without the other is a 500 waiting for whichever client hits the unmapped path.
- **Build/test:** `cargo test -p bloomery-daemon -p bloomery-core`. Daemon rebuilds that must run against real hardware need `--features vulkan`; the tests in this plan do not.
- **NEVER wrap a command in `timeout`** on this box — the uutils wrapper segfaults (exit 139) and the crash misreads as your program's failure.
- Conventional commits. Attribution is disabled globally — no co-author or "Generated with" trailer.
- House TDD: write the failing test first, see it fail, then implement. Mutation-check each load-bearing test by breaking its pinned line and confirming the test fails.

---

## File Structure

| File | Responsibility | Tasks |
| --- | --- | --- |
| `crates/bloomery-daemon/src/drift/watch.rs` | `AdmissionBlock` type, beside `DriftStatus`/`ModelDrift` which it derives from | 1 |
| `crates/bloomery-daemon/src/pager.rs` | The per-model entry's `admission_block` field; `admit()`'s new clause; repeal of the "never read for enforcement" comment | 1, 3, 5 |
| `crates/bloomery-daemon/src/pager/drift_watch.rs` | `set_drift` derives and stores the block; module header comment repealed | 2, 5 |
| `crates/bloomery-daemon/src/pager/status.rs` | `ModelStatus` renders the block beside `drift` | 1 |
| `crates/bloomery-daemon/src/pager/error.rs` | `PagerError::DriftBlocked` | 3 |
| `crates/bloomery-daemon/src/api_native.rs` | `DriftBlocked` → 422; the `unblock` route + its dispatch entry | 3, 4 |
| `crates/bloomery-daemon/src/api_v1.rs` | `DriftBlocked` → 422 through `error_envelope` | 3 |
| `crates/bloomery-core/src/journal.rs` | `Event::Admission` for block set / cleared | 4 |
| `docs/CARRIED-DEBT.md` | The slice-2 record | 5 |

---

### Task 1: `AdmissionBlock`, the pager field, and status rendering

The type and its storage, with nothing yet setting or reading it. Reviewable on its own: a new field that defaults to `None` and renders.

**Files:**
- Modify: `crates/bloomery-daemon/src/drift/watch.rs` (new type beside `ModelDrift`, ~:127)
- Modify: `crates/bloomery-daemon/src/drift.rs` (re-export, beside the existing `pub use watch::{DriftStatus, ModelDrift, ...}` at :45)
- Modify: `crates/bloomery-daemon/src/pager.rs` (per-model entry struct, ~:120-160)
- Modify: `crates/bloomery-daemon/src/pager/status.rs` (`ModelStatus`, ~:95)

**Interfaces:**
- Consumes: `DriftStatus` (`drift/watch.rs:46`), `ModelDrift { step, cumulative }` (`drift/watch.rs:127`).
- Produces: `AdmissionBlock { reference: String }` and `Pager`'s per-model `admission_block: Option<AdmissionBlock>`, both consumed by Tasks 2-4.

- [ ] **Step 1: Write the failing test**

In `crates/bloomery-daemon/src/pager/status.rs`'s test module (or the crate's existing pager test module — follow whichever the file already uses):

```rust
#[test]
fn a_model_with_no_admission_block_renders_none() {
    // A fresh model has never been drift-blocked. Absent is the default,
    // and it must be visible as absent rather than missing: an operator
    // reading /status learns "nothing is holding this model out", which
    // is a different fact from "this field was not rendered".
    let pager = test_pager_with_model("m");
    let status = pager.status();
    let model = status.models.iter().find(|m| m.name == "m").unwrap();
    assert!(model.admission_block.is_none());
}
```

Use whatever pager-construction helper the surrounding tests already use; do not invent a new one. Read the file's existing tests first and match their idiom.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p bloomery-daemon a_model_with_no_admission_block_renders_none`
Expected: FAIL to compile — no field `admission_block` on `ModelStatus`.

- [ ] **Step 3: Add the type**

In `crates/bloomery-daemon/src/drift/watch.rs`, immediately after `ModelDrift` (~:132):

```rust
/// Why this model is currently refused new admission, and by which
/// reference (design §3).
///
/// This is a POLICY derived from a reading, never the reading itself.
/// [`ModelDrift`] is written once when the watch settles it and is never
/// rewritten; this block may be cleared by an operator
/// (`POST /models/{name}/unblock`) without any measurement changing. The
/// two are separate fields for exactly that reason, the same way design
/// §7 keeps `done_trust` and `drift` apart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdmissionBlock {
    /// The blessed baseline's identity — the same `reference` string
    /// [`DriftStatus::Confirmed`] carried, so the 422 can name what
    /// refused without re-deriving it.
    pub reference: String,
}
```

In `crates/bloomery-daemon/src/drift.rs:45`, extend the re-export:

```rust
pub use watch::{AdmissionBlock, DriftStatus, ModelDrift, PROVENANCE_AUTO_FIRST, PROVENANCE_OPERATOR};
```

- [ ] **Step 4: Add the pager field**

In `crates/bloomery-daemon/src/pager.rs`, in the per-model entry struct, immediately after the `drift` field (~:157):

```rust
    /// Set when this boot's CUMULATIVE drift comparison settled
    /// `Confirmed` (design §2), cleared by the operator's explicit
    /// `POST /models/{name}/unblock`. While set, `admit` refuses new
    /// agents on this model.
    ///
    /// Separate from `drift` on purpose: the reading is a measurement and
    /// never changes; this is the policy derived from it, and a policy is
    /// the operator's to override.
    admission_block: Option<crate::drift::AdmissionBlock>,
```

Update the entry's constructor(s) to default it to `None`. Find every construction site with `rg 'ModelEntry\s*\{'` (or whatever the struct is named — read it first) and add the field; the compiler will name any you miss.

- [ ] **Step 5: Render it**

In `crates/bloomery-daemon/src/pager/status.rs`, add to `ModelStatus` beside the existing `drift` field:

```rust
    /// What is currently holding this model out of new admission, or
    /// `None` when nothing is. Rendered beside `drift` because the two
    /// say different things: `drift` is what was measured, this is
    /// whether it is being enforced.
    pub admission_block: Option<crate::drift::AdmissionBlock>,
```

…and populate it from the entry wherever `ModelStatus` is built.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p bloomery-daemon a_model_with_no_admission_block_renders_none`
Expected: PASS.

- [ ] **Step 7: Run the crate suites**

Run: `cargo test -p bloomery-daemon -p bloomery-core`
Expected: green. Record the pass count — later tasks compare against it.

- [ ] **Step 8: Commit**

```bash
git add crates/bloomery-daemon/src/drift/watch.rs crates/bloomery-daemon/src/drift.rs crates/bloomery-daemon/src/pager.rs crates/bloomery-daemon/src/pager/status.rs
git commit -m "feat: carry an admission block beside the drift reading"
```

---

### Task 2: The watch sets the block — the enumerated policy

The whole content of this slice's policy is which words mean what, so the test is an enumeration, not a sample.

**Files:**
- Modify: `crates/bloomery-daemon/src/pager/drift_watch.rs` (`set_drift`, :111)
- Test: the same file's test module

**Interfaces:**
- Consumes: `AdmissionBlock { reference }` from Task 1; `set_drift(&mut self, model: &str, drift: ModelDrift) -> Result<(), PagerError>` (existing).
- Produces: the invariant that `admission_block` is `Some` **iff** the stored `ModelDrift.cumulative` is `Confirmed`. Task 3 depends on it.

- [ ] **Step 1: Write the failing enumeration test**

```rust
#[test]
fn only_a_confirmed_cumulative_reading_blocks_admission() {
    // The policy IS this table — enumerate it rather than sample it.
    // Refuse only what was established; name everything else. An
    // outcome that declines to conclude must not be laundered into a
    // conclusion by the admission path.
    let cases: Vec<(DriftStatus, bool)> = vec![
        (DriftStatus::WithinNoise, false),
        (DriftStatus::Confirmed { reference: "abc1234".into() }, true),
        (DriftStatus::Transient, false),
        (DriftStatus::Unconfirmed { reason: "confirm probe failed".into() }, false),
        (DriftStatus::NotComparable, false),
        (DriftStatus::InstrumentChanged {
            reference: "0.9.0/v8".into(),
            current: "0.10.0/v9".into(),
        }, false),
        (DriftStatus::Unmeasured { reason: "no baseline blessed".into() }, false),
    ];

    for (cumulative, expect_blocked) in cases {
        let mut pager = test_pager_with_model("m");
        pager
            .set_drift("m", ModelDrift {
                step: DriftStatus::WithinNoise,
                cumulative: cumulative.clone(),
            })
            .unwrap();
        let blocked = pager.admission_block_for("m").is_some();
        assert_eq!(blocked, expect_blocked, "cumulative {cumulative:?}");
    }
}

#[test]
fn a_confirmed_step_reading_alone_does_not_block() {
    // step compares against the PREVIOUS BOOT, whose reference advances
    // every boot — a step-keyed block would clear itself next boot
    // whether or not the regression went away. Slice 1: step "alone
    // leaks the ratchet".
    let mut pager = test_pager_with_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::Confirmed { reference: "step99".into() },
            cumulative: DriftStatus::WithinNoise,
        })
        .unwrap();
    assert!(pager.admission_block_for("m").is_none());
}

#[test]
fn a_confirmed_cumulative_reading_blocks_even_when_step_is_clean() {
    // The ratchet case: stable at a degraded level. step sees nothing
    // because last boot was degraded too; cumulative sees the drift from
    // the blessed baseline, and that is the claim that holds a model out.
    let mut pager = test_pager_with_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed { reference: "base42".into() },
        })
        .unwrap();
    let block = pager.admission_block_for("m").expect("blocked");
    assert_eq!(block.reference, "base42");
}

#[test]
fn an_instrument_change_never_blocks_the_fleet() {
    // THE test of this slice. assay v1.8 (0.10.0/v9) lands against
    // blessed v8 references, so the first boot after that merge reads
    // InstrumentChanged on EVERY model at once. Blocking on it would
    // take the whole fleet out on a routine instrument upgrade.
    // Slice 1 §3: "never a pass, never a fail".
    let mut pager = test_pager_with_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::InstrumentChanged {
                reference: "0.9.0/v8".into(),
                current: "0.10.0/v9".into(),
            },
            cumulative: DriftStatus::InstrumentChanged {
                reference: "0.9.0/v8".into(),
                current: "0.10.0/v9".into(),
            },
        })
        .unwrap();
    assert!(pager.admission_block_for("m").is_none());
}
```

`admission_block_for` is a test-visible accessor you add in Step 3. Use the crate's existing test-pager helper rather than inventing one.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bloomery-daemon blocks_admission a_confirmed_step a_confirmed_cumulative an_instrument_change`
Expected: FAIL to compile — no `admission_block_for`.

- [ ] **Step 3: Implement**

In `crates/bloomery-daemon/src/pager/drift_watch.rs`, inside `set_drift`, after the reading is stored:

```rust
        // Design §2: the CUMULATIVE comparison decides, and only
        // `Confirmed` blocks. Derived here, at the moment the reading
        // settles, so the block and the reading it came from are written
        // in one place and cannot disagree.
        let block = match &drift.cumulative {
            DriftStatus::Confirmed { reference } => Some(crate::drift::AdmissionBlock {
                reference: reference.clone(),
            }),
            // Every other outcome admits. An operator-cleared block is
            // NOT resurrected here: a later boot's non-Confirmed reading
            // legitimately clears it, because the comparison was re-run
            // and came back otherwise.
            _ => None,
        };
```

…and store it on the entry alongside the drift. Add the accessor beside it:

```rust
    /// The block currently holding this model out, if any.
    pub fn admission_block_for(&self, model: &str) -> Option<&crate::drift::AdmissionBlock> {
        self.models.get(model).and_then(|e| e.admission_block.as_ref())
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bloomery-daemon blocks_admission a_confirmed_step a_confirmed_cumulative an_instrument_change`
Expected: 4 passed.

- [ ] **Step 5: Mutation-check the enumeration**

The enumeration is worthless if it passes against a broken policy. Temporarily change the match to also block on `DriftStatus::Transient`, rebuild, and confirm `only_a_confirmed_cumulative_reading_blocks_admission` FAILS naming the Transient row. Then change it to key off `drift.step` instead of `drift.cumulative` and confirm `a_confirmed_step_reading_alone_does_not_block` FAILS. Restore exactly (`git checkout -- crates/bloomery-daemon/src/pager/drift_watch.rs` if you edited beyond your intended change), confirm `git status --short` shows only your intended files, and record both failure outputs in your report.

- [ ] **Step 6: Run the crate suites and commit**

```bash
cargo test -p bloomery-daemon -p bloomery-core
git add crates/bloomery-daemon/src/pager/drift_watch.rs
git commit -m "feat: a confirmed cumulative regression sets an admission block"
```

---

### Task 3: `admit()` consults the block, on both error surfaces

**Files:**
- Modify: `crates/bloomery-daemon/src/pager/error.rs` (`PagerError`, :14)
- Modify: `crates/bloomery-daemon/src/pager.rs` (`admit`, ~:623)
- Modify: `crates/bloomery-daemon/src/api_native.rs` (~:289)
- Modify: `crates/bloomery-daemon/src/api_v1.rs` (~:511)

**Interfaces:**
- Consumes: Task 2's invariant — `admission_block` is `Some` iff cumulative is `Confirmed`.
- Produces: `PagerError::DriftBlocked { model: String, reference: String }`, rendered 422 on both surfaces.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_drift_blocked_model_refuses_new_agents() {
    let mut pager = test_pager_with_profiled_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed { reference: "base42".into() },
        })
        .unwrap();
    let err = pager.create_agent(/* the crate's usual agent spec for "m" */).unwrap_err();
    match err {
        PagerError::DriftBlocked { model, reference } => {
            assert_eq!(model, "m");
            assert_eq!(reference, "base42");
        }
        other => panic!("expected DriftBlocked, got {other:?}"),
    }
}

#[test]
fn an_agent_created_before_the_block_keeps_working() {
    // The gate is at agent CREATION, never per inference. Cutting a live
    // conversation mid-turn because the watch settled would be its own
    // dishonesty — the same argument that governs the POST window.
    let mut pager = test_pager_with_profiled_model("m");
    let agent = pager.create_agent(/* … */).unwrap();
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed { reference: "base42".into() },
        })
        .unwrap();
    // The existing agent still resolves and can still be inferred against.
    assert!(pager.agent(&agent).is_some());
    // New work on the same model is refused.
    assert!(matches!(
        pager.create_agent(/* … */),
        Err(PagerError::DriftBlocked { .. })
    ));
}

#[test]
fn an_unprofiled_model_still_refuses_as_unprofiled() {
    // The two refusals stay distinguishable: drift-blocked means a
    // profile exists and a regression was reproduced against it.
    let mut pager = test_pager_with_model("m"); // no profile
    assert!(matches!(
        pager.create_agent(/* … */),
        Err(PagerError::Unprofiled(_))
    ));
}
```

Match the crate's real `create_agent` signature and its existing agent-spec helper — read the surrounding tests and reuse them rather than inventing arguments.

Then, one test per error surface:

```rust
#[test]
fn the_native_surface_renders_drift_blocked_as_422() {
    let (status, body) = render_pager_error(PagerError::DriftBlocked {
        model: "m".into(),
        reference: "base42".into(),
    });
    assert_eq!(status, 422);
    assert_eq!(body["error"], "drift_blocked");
    assert_eq!(body["model"], "m");
    assert_eq!(body["reference"], "base42");
}

#[test]
fn the_v1_surface_renders_drift_blocked_as_422() {
    // A PagerError mapped on one surface and not the other is a 500
    // waiting for whichever client hits the unmapped path.
    let (status, body) = render_v1_pager_error(PagerError::DriftBlocked {
        model: "m".into(),
        reference: "base42".into(),
    });
    assert_eq!(status, 422);
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert_eq!(body["error"]["code"], "model_drift_blocked");
    // The sentence names the baseline that refused, so an operator
    // reading an OpenAI-shaped error still learns which one it was.
    assert!(body["error"]["message"].as_str().unwrap().contains("base42"));
}
```

Use each surface's existing error-rendering entry point — read how the current `Unprofiled` tests call it and match them exactly.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bloomery-daemon drift_blocked before_the_block still_refuses_as_unprofiled`
Expected: FAIL to compile — no `DriftBlocked` variant.

- [ ] **Step 3: Add the variant**

In `crates/bloomery-daemon/src/pager/error.rs`, after `Unprofiled(String)` (:19):

```rust
    /// Model has a profile, and this boot's cumulative drift comparison
    /// settled `Confirmed` against the blessed baseline named here
    /// (design §2). Distinct from `Unprofiled`: something WAS measured,
    /// and what it measured was a reproduced regression.
    DriftBlocked { model: String, reference: String },
```

Extend the `Display` impl in the same file, in its established voice.

- [ ] **Step 4: Add the `admit` clause**

In `crates/bloomery-daemon/src/pager.rs`'s `admit`, before the existing `has_profile` early return:

```rust
        // Design §2. Checked before the existence gate so a blocked model
        // reports the reason that actually applies: it HAS a profile, and
        // that is precisely why a regression against it could be measured.
        if let Some(block) = self.models.get(model).and_then(|e| e.admission_block.as_ref()) {
            return Err(PagerError::DriftBlocked {
                model: model.to_string(),
                reference: block.reference.clone(),
            });
        }
```

Extend the function's doc comment: it currently enumerates three outcomes (profile / POST-window-or-`allow_unprofiled` / `Unprofiled`). Add the block as the first, and state that the agent-creation-not-per-inference rule covers it too.

- [ ] **Step 5: Map both surfaces**

`api_native.rs`, beside the `Unprofiled` arm (:289):

```rust
        PagerError::DriftBlocked { model, reference } => (
            422,
            json!({"error": "drift_blocked", "model": model, "reference": reference}),
        ),
```

Also extend that function's doc-comment status table, which lists `| `Unprofiled` | 422 | …` — a table that silently omits a variant is the next reader's wrong answer.

`api_v1.rs`, beside its `Unprofiled` arm (:511):

```rust
        PagerError::DriftBlocked { model, reference } => (
            422,
            error_envelope(
                "invalid_request_error",
                "model_drift_blocked",
                format!(
                    "model '{model}' is held out: its capability profile drifted \
                     from blessed baseline '{reference}' and the change reproduced"
                ),
                Some("model"),
            ),
        ),
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p bloomery-daemon drift_blocked before_the_block still_refuses_as_unprofiled`
Expected: all pass.

- [ ] **Step 7: Run the crate suites and commit**

```bash
cargo test -p bloomery-daemon -p bloomery-core
git add crates/bloomery-daemon/src/pager/error.rs crates/bloomery-daemon/src/pager.rs crates/bloomery-daemon/src/api_native.rs crates/bloomery-daemon/src/api_v1.rs
git commit -m "feat: refuse admission while a drift block stands, on both surfaces"
```

---

### Task 4: `POST /models/{name}/unblock`

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (`Event`, after `Blessed` at :180)
- Modify: `crates/bloomery-daemon/src/pager/drift_watch.rs` (the clearing method)
- Modify: `crates/bloomery-daemon/src/api_native.rs` (route + dispatch entry at :61)

**Interfaces:**
- Consumes: `AdmissionBlock` (Task 1), `admission_block_for` (Task 2), `PROVENANCE_OPERATOR` (`drift/watch.rs:145`).
- Produces: `Pager::clear_admission_block(&mut self, model: &str) -> Result<AdmissionBlock, PagerError>` returning what was cleared; `Event::Admission`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn unblock_admits_and_leaves_the_reading_alone() {
    // The point of separating the block from the reading: an operator
    // may override the policy without any measurement changing. After
    // clearing, /status still says exactly what was measured.
    let mut pager = test_pager_with_profiled_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed { reference: "base42".into() },
        })
        .unwrap();
    let cleared = pager.clear_admission_block("m").unwrap();
    assert_eq!(cleared.reference, "base42");
    assert!(pager.admission_block_for("m").is_none());
    assert!(pager.create_agent(/* … */).is_ok());

    let status = pager.status();
    let model = status.models.iter().find(|m| m.name == "m").unwrap();
    assert_eq!(
        model.drift.as_ref().unwrap().cumulative,
        DriftStatus::Confirmed { reference: "base42".into() },
        "the reading is a measurement and must survive the override"
    );
}

#[test]
fn unblock_with_nothing_blocking_is_a_conflict_not_a_no_op() {
    // Answering 200 where nothing was blocking would tell an operator
    // they cleared something when nothing was written — the silent no-op
    // slice 1 §2 forbids, the same reason bless returns 409.
    let mut pager = test_pager_with_profiled_model("m");
    assert!(pager.clear_admission_block("m").is_err());
}

#[test]
fn unblock_does_not_rebaseline_and_bless_does_not_unblock() {
    // The two routes answer different questions and neither implies the
    // other. A future reader would assume this away.
    let mut pager = test_pager_with_profiled_model("m");
    pager
        .set_drift("m", ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed { reference: "base42".into() },
        })
        .unwrap();

    // bless leaves the block standing…
    pager.bless_baseline("m").unwrap();
    assert!(pager.admission_block_for("m").is_some());

    // …and unblock does not touch the baseline. Capture the blessed
    // identity before and after and assert it is unchanged, using
    // whatever accessor the bless tests already use.
    let before = blessed_identity(&pager, "m");
    pager.clear_admission_block("m").unwrap();
    assert_eq!(blessed_identity(&pager, "m"), before);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p bloomery-daemon unblock_`
Expected: FAIL to compile — no `clear_admission_block`.

- [ ] **Step 3: Add the journal event**

In `crates/bloomery-core/src/journal.rs`, after `Blessed` (:185):

```rust
    /// An admission block set by the drift watch, or cleared by an
    /// operator (verdict-gated-admission design §4). Two rows at most per
    /// model per boot: one when a confirmed cumulative regression set it,
    /// one if the operator cleared it. A replay reconstructs which models
    /// were held out, by which baseline, and who let them back in.
    Admission {
        model: String,
        /// `"blocked"` or `"cleared"`.
        action: String,
        /// The blessed baseline's identity that refused.
        reference: String,
        /// `PROVENANCE_OPERATOR` on a clearing; the drift watch's own
        /// name when the block was set.
        provenance: String,
    },
```

- [ ] **Step 4: Implement the clearing method**

In `crates/bloomery-daemon/src/pager/drift_watch.rs`:

```rust
    /// Clear this model's admission block, returning what was cleared.
    ///
    /// Touches neither the drift reading nor the blessed baseline: the
    /// reading is a measurement, and re-baselining is `bless`'s job and
    /// takes effect at the next boot. This says only "admit it anyway,
    /// now", and journals who said so.
    pub fn clear_admission_block(
        &mut self,
        model: &str,
    ) -> Result<crate::drift::AdmissionBlock, PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        let block = entry
            .admission_block
            .take()
            .ok_or_else(|| PagerError::Contract(format!("no admission block on '{model}'")))?;
        // journal the clearing with operator provenance
        Ok(block)
    }
```

Emit the `Event::Admission` row with `action: "cleared"` and `provenance: PROVENANCE_OPERATOR`, using the module's existing journalling helper (see how `blessed(...)` at `pager/journal.rs:194` is called). Also emit `action: "blocked"` from Task 2's `set_drift` path when a block is newly set — add that there and note it in this task's report.

- [ ] **Step 5: Add the route**

In `crates/bloomery-daemon/src/api_native.rs`, dispatch entry beside bless (:61):

```rust
        ("POST", ["models", name, "unblock"]) => unblock(pager, name),
```

And the handler, in the file's established doc-comment-table style:

```rust
/// `POST /models/{name}/unblock` — clear this boot's admission block
/// (verdict-gated-admission design §4).
///
/// | outcome | status | body |
/// |---|---|---|
/// | cleared | 200 | `{model, cleared: {reference}}` |
/// | no such model | 404 | the surface's one `unknown_model` shape |
/// | no block to clear | 409 | `{error: "no_admission_block", model, detail}` |
///
/// **The 409 is the load-bearing one**, for the same reason bless's is:
/// answering 200 where nothing was blocking would tell an operator they
/// had cleared something when nothing was written.
///
/// This does NOT re-baseline. `bless` accepts a new normal for the next
/// boot; this admits the model now, with the reading left exactly as
/// measured. Neither implies the other.
fn unblock<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
```

Follow `bless`'s body shape exactly — lock handling, error mapping, and the `unknown_model` 404.

- [ ] **Step 6: Run to verify, then the full suites**

Run: `cargo test -p bloomery-daemon unblock_`
Expected: all pass.
Run: `cargo test -p bloomery-daemon -p bloomery-core`

- [ ] **Step 7: Commit**

```bash
git add crates/bloomery-core/src/journal.rs crates/bloomery-daemon/src/pager/drift_watch.rs crates/bloomery-daemon/src/api_native.rs
git commit -m "feat: POST /models/{name}/unblock clears a block without touching the reading"
```

---

### Task 5: Repeal the stale comments, and record the slice

Two comments in shipped code now assert the opposite of what the code does. In this project a stale claim in a durable artifact is the defect, not a footnote to one.

**Files:**
- Modify: `crates/bloomery-daemon/src/pager/drift_watch.rs` (header, :11)
- Modify: `crates/bloomery-daemon/src/pager.rs` (the `drift` field comment, ~:155)
- Modify: `docs/CARRIED-DEBT.md`

- [ ] **Step 1: Repeal the module header**

`crates/bloomery-daemon/src/pager/drift_watch.rs:11` reads "Nothing here touches `done_trust`, `codec_gate` or admission. Design §7 is …". That is now false. Rewrite it to state what holds: this module touches **admission and only admission**, via the block derived in `set_drift`; `done_trust` remains the sole property of the G4 codec gate and the G5 refusal gate, and `codec_gate` is untouched. Cite the new design §2/§3.

- [ ] **Step 2: Repeal the field comment**

`crates/bloomery-daemon/src/pager.rs`'s `drift` field says "Never read for enforcement: drift is observability, and `done_trust` stays the sole property of the G4/G5 gates (design §7)." Rewrite the first clause: the cumulative reading is now read for enforcement, once, at the moment it settles, to derive `admission_block`. Keep the `done_trust` clause — it is still true and still load-bearing.

- [ ] **Step 3: Record the slice in CARRIED-DEBT**

Follow the file's established structure and its reading rules (a closed item is struck through with `~~…~~` and the closing text follows in bold; **nothing is ever deleted**). Record:

- What this slice settled: the refusal table, cumulative-not-step, the reading/block separation, the two operator routes.
- **Open, carried forward:** the `Infra`-folded-into-`Unmeasured` item from slice 1's Task 4 — this slice did not need them apart (both fold into outcomes that admit) but a verdict-floors slice will. Say so plainly rather than restating the old item.
- **New this slice:** `verdict.parallel` and assay's exit 3 exist and are deliberately not consumed; the block is per-model and there is no fleet-wide override (deliberate — `allow_unprofiled`'s all-or-nothing shape was rejected for this in design); whatever the implementers found and did not fix.

- [ ] **Step 4: Verify no stale claim survives**

Run: `rg -n "never read for enforcement|Nothing here touches" crates/`
Expected: no hits. If either phrase survives anywhere else, fix it there too and say so in your report.

- [ ] **Step 5: Run the full suites and commit**

```bash
cargo test -p bloomery-daemon -p bloomery-core
git add crates/bloomery-daemon/src/pager/drift_watch.rs crates/bloomery-daemon/src/pager.rs docs/CARRIED-DEBT.md
git commit -m "docs: repeal the no-enforcement claims; record seam slice 2"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
| --- | --- |
| §1 the seam, `admit()`'s new clause | 3 |
| §2 cumulative-not-step | 2 |
| §2 the seven-value refusal table | 2 |
| §3 reading and block as separate fields | 1 |
| §4 bless unchanged; `unblock` new; 200/404/409 | 4 |
| §4 neither route implies the other | 4 |
| §5 422 on both surfaces, each in its own idiom | 3 |
| §5 agent-creation-not-per-inference | 3 |
| §5 the comment repeals | 5 |
| §6 `InstrumentChanged` never blocks | 2 |
| §7 journal completeness | 4 |
| §8 non-goals (nothing implements them) | — |

No spec requirement is unassigned. §8's non-goals appear only as explicit non-actions in Task 5's debt record.

**Type consistency:** `AdmissionBlock { reference: String }` is defined in Task 1 and used under that name in Tasks 2, 3 and 4. `admission_block_for(&self, model: &str) -> Option<&AdmissionBlock>` is introduced in Task 2 and used in Tasks 3 and 4. `clear_admission_block(&mut self, model: &str) -> Result<AdmissionBlock, PagerError>` is introduced in Task 4 and used only there. `PagerError::DriftBlocked { model, reference }` is introduced in Task 3 with the same field names on both surfaces.

**Test-helper names are illustrative, and the header says which are real.** The tasks below write `test_pager_with_model(...)` / `test_pager_with_profiled_model(...)` / `blessed_identity(...)` / `render_pager_error(...)` as shorthand for setup the crate already has its own way of doing. **None of those four names exists** — I checked. The Tech Stack header carries the real construction idiom (`Pager::new(...)` + `register_model` + `create_agent("qwen", 50, None, 10_000)`) and names the real helpers that do exist in `tests/drift_test.rs` (`scratch`, `store_in`, `qwen_like_meta`, `scripted_assay`). Build the setup from those; do not add a parallel helper set, and do not treat the shorthand as an API to create.

For the two error-surface tests in Task 3, likewise: read how the existing `Unprofiled` cases are exercised on each surface and call the same entry point. `tests/api_native_test.rs` and `tests/api_v1_test.rs` drive real HTTP through `tests/common/mod.rs`'s `http(addr, method, path, body)` helper, so the natural shape is a request that gets a 422 back, not a direct call to a rendering function.

Task 4's journalling follows `pager/journal.rs:194`'s existing `blessed(...)` helper rather than a new one.
