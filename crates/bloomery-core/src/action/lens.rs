//! The landing lens trait and PlainText implementation (Task 4).
//!
//! A landing test checks whether a patch both *applies* to the current
//! contents and *parses* correctly in its target language. The [`land`]
//! function orchestrates this two-step check, producing a [`Landing`]
//! outcome that captures whether the patch landed successfully, and if not,
//! which step (apply or parse) failed.
//!
//! Task 4 ships [`PlainText`], which accepts all content. P3 will add
//! language-specific lenses (e.g., Python syntax checking) behind the
//! [`LandingLens`] trait.

use super::{
    patch::{apply_patch, PatchApplyError},
    PatchBody,
};

/// The outcome of testing whether a patch LANDS: applies-and-parses.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Landing {
    /// The patch applied and the lens accepted the result.
    Lands {
        new_contents: String,
        lens: &'static str,
    },
    /// The patch failed to apply. The lens was not consulted.
    DidNotApply {
        reason: PatchApplyError,
        lens: &'static str,
    },
    /// The patch applied but the lens rejected the result.
    DidNotParse { detail: String, lens: &'static str },
    /// The lens cannot judge the language (e.g., Python lens on a .txt file).
    /// Produced by lenses in P3 that have language-specific logic; PlainText
    /// never emits this variant.
    Unparsed {
        language: String,
        lens: &'static str,
    },
}

/// A language-specific parse check. `PlainText` is pure, but a lens is not
/// guaranteed to be: P3's `PythonLens::parses` shells out to `python3` (up
/// to a 10s timeout) to get a real syntax check, and that call happens under
/// the caller's pager lock. Callers must not assume `land()` is cheap or
/// I/O-free — a lens may block on a subprocess.
///
/// Fail-closed contract: if a lens's external checker is unavailable (the
/// interpreter can't be found, spawning it fails, or it times out), the
/// lens must treat that as a parse failure, not a pass — an unreachable
/// checker means the patch does not land, never that it lands unverified.
pub trait LandingLens {
    /// The name of this lens, used in every Landing outcome (named-lens law).
    fn name(&self) -> &'static str;
    /// Does this string parse as a valid document in the lens's language?
    /// May shell out to an external checker; see the trait's fail-closed
    /// contract above for what an unavailable checker must produce.
    fn parses(&self, contents: &str) -> Result<(), String>;
}

/// A lens that accepts all content. Used in P1; P3 will add language-specific
/// lenses that perform syntax checking.
pub struct PlainText;

impl LandingLens for PlainText {
    fn name(&self) -> &'static str {
        "plaintext"
    }

    fn parses(&self, _contents: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Test whether a patch lands: apply it, then run the lens on the result.
///
/// First calls [`apply_patch`]. On error, returns [`Landing::DidNotApply`]
/// with the reason. On success, calls the lens's [`parses`](LandingLens::parses)
/// method. If parsing succeeds, returns [`Landing::Lands`]. If parsing fails,
/// returns [`Landing::DidNotParse`] with the error detail.
///
/// The lens name rides in every Landing outcome (named-lens law).
pub fn land(current: &str, body: &PatchBody, lens: &dyn LandingLens) -> Landing {
    match apply_patch(current, body) {
        Err(reason) => Landing::DidNotApply {
            reason,
            lens: lens.name(),
        },
        Ok(new_contents) => match lens.parses(&new_contents) {
            Ok(()) => Landing::Lands {
                new_contents,
                lens: lens.name(),
            },
            Err(detail) => Landing::DidNotParse {
                detail,
                lens: lens.name(),
            },
        },
    }
}
