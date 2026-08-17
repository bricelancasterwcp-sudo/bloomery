use crate::action::PatchCodec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The capability grade `preferred_patch_codec` reads from the `codecs`
/// grid — assay's `_GRADE_FOR_VERDICTS`. Named as a constant so the two
/// cannot silently diverge. See `docs/superpowers/evidence/2026-08-15-g4-protocol.md`
/// §4.
pub const VERDICT_GRADE: &str = "small";

/// Represents a verdict for a measured capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// The capability passed testing and is safe to use.
    Ready,
    /// The capability passed but with caveats; use with care.
    Risky,
    /// The capability failed testing; do not use.
    Unusable,
    /// The capability was not measured.
    Unmeasured,
}

/// Errors that can occur when parsing a profile.
#[derive(Debug)]
pub enum ProfileError {
    /// JSON parsing or deserialization error.
    Parse(String),
    /// Unsupported schema version (only v2+ is supported).
    UnsupportedSchema(u32),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Parse(msg) => write!(f, "parse error: {}", msg),
            ProfileError::UnsupportedSchema(v) => write!(f, "unsupported schema version: {}", v),
        }
    }
}

impl std::error::Error for ProfileError {}

/// Internal deserialization structure for model info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ModelInfo {
    name: String,
    #[serde(default)]
    quant: Option<String>,
}

/// Internal deserialization structure for ceiling info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CeilingInfo {
    max_verified: Option<u32>,
    #[serde(default)]
    first_failure: Option<u32>,
    #[serde(default)]
    failure_mode: Option<String>,
}

/// Internal deserialization structure for a single verdict entry.
#[derive(Debug, Clone, Deserialize)]
struct VerdictEntry {
    verdict: String,
}

/// Internal deserialization structure for a single `codecs` grid cell.
/// Mirrors [`CodecCell`] field-for-field.
#[derive(Debug, Clone, Deserialize)]
struct CodecCellData {
    lands: Option<f64>,
    lands_applies: Option<f64>,
    n: u32,
}

/// Internal root deserialization structure.
#[derive(Debug, Clone, Deserialize)]
struct ProfileData {
    assay_profile_version: u32,
    /// The version of the `assay` probe that wrote this document. Present in
    /// every assay schema since v1, so it is **mandatory** here: no
    /// `#[serde(default)]`, no `Option`. A document without it is a parse
    /// error, because a defaulted instrument identity would let the drift
    /// precheck (see [`instrument_precheck`]) call two different instruments
    /// comparable.
    probe_version: String,
    model: ModelInfo,
    #[serde(default)]
    ceiling: Option<CeilingInfo>,
    #[serde(default)]
    verdicts: Option<HashMap<String, VerdictEntry>>,
    /// `{codec_name: {grade: cell}}` — grades `tiny`/`small`/`medium`,
    /// codec names `search_replace`/`whole_file`/`json_object`. Absent on
    /// older/handmade profiles, hence `#[serde(default)]` so those still
    /// parse unchanged.
    #[serde(default)]
    codecs: Option<HashMap<String, HashMap<String, CodecCellData>>>,
}

/// A single measured cell of the `codecs` grid, at a given codec/grade
/// pair: how often the codec's patch landed, both unconditionally
/// (`lands`) and conditioned on the patch having applied at all
/// (`lands_applies`), plus the sample size `n`. `None` fields mean that
/// cell was not measured (assay ran fewer than the configured
/// `n_per_cell`, or skipped it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodecCell {
    /// Fraction of trials where the patch landed (unconditional).
    pub lands: Option<f64>,
    /// Fraction of trials where the patch landed, given it applied.
    pub lands_applies: Option<f64>,
    /// Number of trials this cell was measured over.
    pub n: u32,
}

/// A capability profile from an external `assay` tool.
///
/// Contains measured capabilities and verdicts for a model.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Profile {
    #[serde(flatten)]
    data: ProfileData,
}

impl Profile {
    /// Parse a profile from JSON string.
    ///
    /// Returns `Err(ProfileError::UnsupportedSchema)` if the schema version is < 2.
    pub fn from_json(s: &str) -> Result<Profile, ProfileError> {
        let profile: Profile =
            serde_json::from_str(s).map_err(|e| ProfileError::Parse(e.to_string()))?;

        if profile.schema_version() < 2 {
            return Err(ProfileError::UnsupportedSchema(profile.schema_version()));
        }

        Ok(profile)
    }

    /// Get the schema version of this profile.
    pub fn schema_version(&self) -> u32 {
        self.data.assay_profile_version
    }

    /// Get the version of the `assay` probe that wrote this profile.
    ///
    /// Returns `&str`, not `Option<&str>`: the field is mandatory at parse
    /// time, so an existing `Profile` always knows which instrument measured
    /// it.
    pub fn probe_version(&self) -> &str {
        &self.data.probe_version
    }

    /// This profile's instrument identity, `"<probe_version>/v<schema>"` —
    /// the form [`InstrumentPrecheck::InstrumentChanged`] carries into the
    /// journal.
    fn instrument_id(&self) -> String {
        format!("{}/v{}", self.probe_version(), self.schema_version())
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.data.model.name
    }

    /// Get the measured ceiling (max_verified token count), or None if not measured.
    pub fn measured_ceiling(&self) -> Option<u32> {
        self.data.ceiling.as_ref().and_then(|c| c.max_verified)
    }

    /// Get the verdict for a capability by name.
    ///
    /// If the capability is not found or verdicts are not present, returns `Verdict::Unmeasured`.
    pub fn verdict(&self, name: &str) -> Verdict {
        match &self.data.verdicts {
            None => Verdict::Unmeasured,
            Some(map) => match map.get(name) {
                None => Verdict::Unmeasured,
                Some(entry) => match entry.verdict.as_str() {
                    "ready" => Verdict::Ready,
                    "risky" => Verdict::Risky,
                    "unusable" => Verdict::Unusable,
                    _ => Verdict::Unmeasured,
                },
            },
        }
    }

    /// Get the `codecs` grid cell for `codec` at [`VERDICT_GRADE`].
    ///
    /// Returns `None` when the grid, the codec, or the grade is absent —
    /// callers cannot distinguish "unmeasured" from "not present" and
    /// should not need to (protocol §4 treats both as unmeasured).
    pub fn codec_cell(&self, codec: &str) -> Option<CodecCell> {
        let cell = self.data.codecs.as_ref()?.get(codec)?.get(VERDICT_GRADE)?;
        Some(CodecCell {
            lands: cell.lands,
            lands_applies: cell.lands_applies,
            n: cell.n,
        })
    }

    /// Select the per-model patch codec per protocol §4
    /// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md`):
    ///
    /// - `whole_file`'s `lands_applies` strictly greater than
    ///   `search_replace`'s → [`PatchCodec::WholeFile`]
    /// - Otherwise `search_replace`, when it is measured
    /// - Only one of the two measured → that one
    /// - Neither measured (or no `codecs` grid at all) → `None`; the
    ///   caller defaults to [`PatchCodec::SearchReplace`] (the
    ///   robigo-proven default) and records `"default (codecs
    ///   unmeasured)"` per protocol §4.
    pub fn preferred_patch_codec(&self) -> Option<PatchCodec> {
        let sr = self
            .codec_cell("search_replace")
            .and_then(|c| c.lands_applies);
        let wf = self.codec_cell("whole_file").and_then(|c| c.lands_applies);

        match (sr, wf) {
            (None, None) => None,
            (Some(_), None) => Some(PatchCodec::SearchReplace),
            (None, Some(_)) => Some(PatchCodec::WholeFile),
            (Some(s), Some(w)) => Some(if w > s {
                PatchCodec::WholeFile
            } else {
                PatchCodec::SearchReplace
            }),
        }
    }
}

/// Whether two profiles were measured by the same instrument, and so may be
/// compared at all.
#[derive(Debug, Clone, PartialEq)]
pub enum InstrumentPrecheck {
    /// Same `probe_version` and same `assay_profile_version`: a drift
    /// comparison between these two documents measures the model.
    Comparable,
    /// The instrument moved between the two documents. Carries both sides'
    /// identity in `"<probe_version>/v<schema>"` form (e.g. `"0.5.0/v4"`) so
    /// the journal row can name them without re-reading either file.
    InstrumentChanged {
        /// The reference profile's instrument identity.
        reference: String,
        /// The current profile's instrument identity.
        current: String,
    },
}

/// Decide whether `reference` and `current` are comparable at all, before any
/// drift gate runs (design §3, `docs/superpowers/specs/2026-08-17-drift-watch-design.md`).
///
/// `InstrumentChanged` iff the two disagree on `probe_version` **or**
/// `assay_profile_version` — never pass, never fail, never a score. The
/// rationale is measured, not theoretical: assay's 2026-08 campaign diffs
/// showed 12 of 15 models "improving" because the ceiling cap changed between
/// probe versions, not because the models did. Comparing across a changed
/// instrument reports the instrument.
///
/// Pure: reads the two documents' own version fields and nothing else — no
/// filesystem, no subprocess, no clock.
pub fn instrument_precheck(reference: &Profile, current: &Profile) -> InstrumentPrecheck {
    let same_probe = reference.probe_version() == current.probe_version();
    let same_schema = reference.schema_version() == current.schema_version();

    if same_probe && same_schema {
        InstrumentPrecheck::Comparable
    } else {
        InstrumentPrecheck::InstrumentChanged {
            reference: reference.instrument_id(),
            current: current.instrument_id(),
        }
    }
}
