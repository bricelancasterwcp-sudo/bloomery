// Rust compiles each integration test file as its own binary and includes the
// whole of this module in every one of them, so a helper used by one of the
// `geometry_conformance_*` files is dead code in the other. The allow is the
// standard price of a shared test-fixture module; it is scoped to this module.
#![allow(dead_code)]

//! Shared fixtures for `bloomery-core`'s integration tests.

pub mod gguf_vectors;
