//! Per-verb validation for the action codec (Task 2).
//!
//! Each verb has specific attribute and body requirements; this module
//! implements that validation after the envelope has been scanned.

use super::{Action, ActionError, RawAction};
use regex::Regex;

/// Validates a `read` action: requires `path` attr, optional `lines="A-B"`.
pub fn validate_read(raw: &RawAction) -> Result<Action, ActionError> {
    let path = raw
        .attrs
        .get("path")
        .cloned()
        .ok_or(ActionError::MissingAttr {
            verb: "read",
            attr: "path",
        })?;

    let lines = if let Some(lines_str) = raw.attrs.get("lines") {
        let parts: Vec<&str> = lines_str.split('-').collect();
        if parts.len() != 2 {
            return Err(ActionError::BadRange {
                got: lines_str.clone(),
                expected: "lines=\"A-B\" with 1 ≤ A ≤ B",
            });
        }

        let a: u32 = parts[0].parse().map_err(|_| ActionError::BadRange {
            got: lines_str.clone(),
            expected: "lines=\"A-B\" with 1 ≤ A ≤ B",
        })?;
        let b: u32 = parts[1].parse().map_err(|_| ActionError::BadRange {
            got: lines_str.clone(),
            expected: "lines=\"A-B\" with 1 ≤ A ≤ B",
        })?;

        if a < 1 || b < 1 || a > b {
            return Err(ActionError::BadRange {
                got: lines_str.clone(),
                expected: "lines=\"A-B\" with 1 ≤ A ≤ B",
            });
        }

        Some((a, b))
    } else {
        None
    };

    Ok(Action::Read { path, lines })
}

/// Validates a `find` action: requires `pattern` (non-empty, valid regex) and `path` attrs.
pub fn validate_find(raw: &RawAction) -> Result<Action, ActionError> {
    let pattern = raw.attrs.get("pattern").ok_or(ActionError::MissingAttr {
        verb: "find",
        attr: "pattern",
    })?;

    if pattern.is_empty() {
        return Err(ActionError::MissingAttr {
            verb: "find",
            attr: "pattern",
        });
    }

    // Validate that pattern compiles as a regex
    Regex::new(pattern).map_err(|e| ActionError::BadRegex {
        pattern: pattern.clone(),
        detail: e.to_string(),
    })?;

    let path = raw.attrs.get("path").ok_or(ActionError::MissingAttr {
        verb: "find",
        attr: "path",
    })?;

    Ok(Action::Find {
        pattern: pattern.clone(),
        path: path.clone(),
    })
}

/// Validates a `run` action: body must be a non-empty JSON array of strings.
pub fn validate_run(raw: &RawAction) -> Result<Action, ActionError> {
    if raw.body.is_empty() {
        return Err(ActionError::BadArgv {
            detail: "empty body".into(),
            expected: "a JSON array of strings, e.g. [\"cargo\",\"test\"]",
        });
    }

    let argv: Vec<String> = serde_json::from_str(&raw.body).map_err(|e| ActionError::BadArgv {
        detail: e.to_string(),
        expected: "a JSON array of strings, e.g. [\"cargo\",\"test\"]",
    })?;

    if argv.is_empty() {
        return Err(ActionError::BadArgv {
            detail: "empty array".into(),
            expected: "a JSON array of strings, e.g. [\"cargo\",\"test\"]",
        });
    }

    Ok(Action::Run { argv })
}

/// Validates a `done` action: body must be non-empty (trimmed).
pub fn validate_done(raw: &RawAction) -> Result<Action, ActionError> {
    let trimmed = raw.body.trim();
    if trimmed.is_empty() {
        return Err(ActionError::EmptyBody {
            verb: "done",
            expected: "a non-empty summary",
        });
    }

    Ok(Action::Done {
        summary: trimmed.to_string(),
    })
}
