use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Internal root deserialization structure.
#[derive(Debug, Clone, Deserialize)]
struct ProfileData {
    assay_profile_version: u32,
    model: ModelInfo,
    #[serde(default)]
    ceiling: Option<CeilingInfo>,
    #[serde(default)]
    verdicts: Option<HashMap<String, VerdictEntry>>,
}

/// A capability profile from an external `assay` tool.
///
/// Contains measured capabilities and verdicts for a model.
#[derive(Debug, Clone)]
pub struct Profile {
    data: ProfileData,
}

impl Profile {
    /// Parse a profile from JSON string.
    ///
    /// Returns `Err(ProfileError::UnsupportedSchema)` if the schema version is < 2.
    pub fn from_json(s: &str) -> Result<Profile, ProfileError> {
        let data: ProfileData =
            serde_json::from_str(s).map_err(|e| ProfileError::Parse(e.to_string()))?;

        if data.assay_profile_version < 2 {
            return Err(ProfileError::UnsupportedSchema(data.assay_profile_version));
        }

        Ok(Profile { data })
    }

    /// Get the schema version of this profile.
    pub fn schema_version(&self) -> u32 {
        self.data.assay_profile_version
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
}
