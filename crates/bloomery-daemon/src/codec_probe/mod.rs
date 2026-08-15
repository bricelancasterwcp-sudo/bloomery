//! The G4 codec-landing gate (Phase 2b/2c P4).
//!
//! Governing doc: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`, a
//! pre-registered measurement protocol written before this module existed.
//! Per that protocol's §2 ("Instrument"), the gate measures whether each
//! configured model's chosen patch codec **lands** a small, frozen set of
//! single-defect repair tasks through this daemon's own serving path — not
//! whether the model can "solve" anything more general.
//!
//! Task 5 (this module, so far) ships only the frozen fixture set itself
//! (`codec-tasks-v1`, N=20) and its parser: [`fixtures::FixtureSet`],
//! [`fixtures::Fixture`], and [`fixtures::shipped_fixture_set`]. Every
//! fixture's reference fix is proven to land through the real
//! `bloomery_core::action::lens::land` path by
//! `tests/codec_fixtures_test.rs` before this set is trusted for anything
//! downstream. The probe engine that actually runs a model against these
//! fixtures and applies the G4 scoring/decision rule (protocol §3–§5)
//! arrives in a later task.

pub mod fixtures;
