//! Fixtures shared by the `pager_weights` family after its 2026-09-01 split
//! (carried-debt slice D).

pub const MIB: u64 = 1024 * 1024;

/// `window_cap = 1024` tokens at `kv_per_token = 57_344` — see the module
/// doc comment.
pub const WINDOW_CAP: u32 = 1024;

pub const KV_BYTES: u64 = 1024 * 57_344;
