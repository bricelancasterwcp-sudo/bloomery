//! Command allowlist checking for grants.

use super::GrantViolation;

/// Check if `argv` (the run action's exec vector) is allowed under this set
/// of granted command prefixes.
///
/// `argv` is allowed iff some granted prefix `p` satisfies:
/// `argv.starts_with(p)` (element-wise prefix match).
///
/// An empty `argv` is rejected with `CommandNotAllowed`. No granted prefix
/// match is also rejected with `CommandNotAllowed{argv}`.
///
/// **Invariant**: All prefixes are non-empty; this is enforced by
/// [`Grant::from_json`](super::Grant::from_json) at construction time.
pub fn check_command(prefixes: &[Vec<String>], argv: &[String]) -> Result<(), GrantViolation> {
    // Empty argv is always rejected.
    if argv.is_empty() {
        return Err(GrantViolation::CommandNotAllowed { argv: Vec::new() });
    }

    // Check if any prefix matches.
    for prefix in prefixes {
        if argv.starts_with(prefix) {
            return Ok(());
        }
    }

    // No prefix matched.
    Err(GrantViolation::CommandNotAllowed {
        argv: argv.to_vec(),
    })
}
