//! Command allowlist checking for grants.

use super::GrantViolation;

/// Check if `argv` (the run action's exec vector) is allowed under this set
/// of granted command prefixes.
///
/// `argv` is allowed iff some granted prefix `p` satisfies:
/// `argv.len() >= p.len() && argv[..p.len()] == p[..]`
///
/// An empty `argv` is rejected with `CommandNotAllowed`. No granted prefix
/// match is also rejected with `CommandNotAllowed{argv}`.
pub fn check_command(prefixes: &[Vec<String>], argv: &[String]) -> Result<(), GrantViolation> {
    // Empty argv is always rejected.
    if argv.is_empty() {
        return Err(GrantViolation::CommandNotAllowed { argv: Vec::new() });
    }

    // Check if any prefix matches.
    for prefix in prefixes {
        if argv.len() >= prefix.len() && argv[..prefix.len()] == prefix[..] {
            return Ok(());
        }
    }

    // No prefix matched.
    Err(GrantViolation::CommandNotAllowed {
        argv: argv.to_vec(),
    })
}
