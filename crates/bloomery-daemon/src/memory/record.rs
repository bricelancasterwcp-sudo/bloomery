//! Episode records — the canonical type for representing task identity, evidence, and
//! outcome per the memory-organ architecture.
//!
//! See spec §2 (task identity) and §6 (stored row format) in
//! docs/superpowers/specs/2026-08-26-memory-organ-design.md.

use serde::{Deserialize, Serialize};

/// Normalize a goal string by trimming and collapsing internal whitespace runs to one space.
pub fn normalize_goal(goal: &str) -> String {
    goal.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Hash a goal string via sha256 of its normalized form.
pub fn goal_hash(goal: &str) -> String {
    bloomery_core::journal::sha256_hex(&normalize_goal(goal))
}

/// A file's content fingerprint: either a sha256 hex or absent (untracked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fingerprint {
    Sha256(String),
    Absent,
}

/// A file cited in an episode's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CitedFile {
    pub path: String,
    pub fingerprint: Fingerprint,
}

/// A patch landed by an episode: search/replace or whole file replacement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredPatch {
    pub path: String,
    pub codec: String, // "search_replace" or "whole_file"
    pub body: String,
}

/// Evidence from running a command during the episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvidence {
    pub argv: Vec<String>,
    pub outcome: String,
}

/// A complete episode record: the task identity, context, patches, and outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeRecord {
    pub episode_id: String,
    pub goal_hash: String,
    pub goal_text: String,
    pub cited_files: Vec<CitedFile>,
    pub landed_patches: Vec<StoredPatch>,
    pub run_evidence: RunEvidence,
    pub trajectory: Vec<String>,
    pub minted_by_model: String,
    pub minted_by_envelope: String,
    pub status: String,
    pub contradicted_by: Option<String>,
    pub minted_at: u64,
}

/// Compute the episode identity: the sha256 of goal_hash plus each cited file's path
/// and fingerprint, with cited files sorted by path first. Landed patches are excluded.
pub fn episode_id(goal_hash: &str, cited: &[CitedFile]) -> String {
    let mut sorted_cited = cited.to_vec();
    sorted_cited.sort_by(|a, b| a.path.cmp(&b.path));

    let mut content = String::new();
    content.push_str(goal_hash);
    for file in sorted_cited {
        content.push('\n');
        content.push_str(&file.path);
        content.push('\n');
        match &file.fingerprint {
            Fingerprint::Sha256(hex) => {
                content.push_str("sha256:");
                content.push_str(hex);
            }
            Fingerprint::Absent => {
                content.push_str("absent");
            }
        }
    }

    bloomery_core::journal::sha256_hex(&content)
}

/// A row in the stored record: either an episode or a tombstone marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "row", rename_all = "snake_case")]
pub enum StoredRow {
    Episode(Box<EpisodeRecord>),
    Tombstone { episode_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_goal_trims_and_collapses_whitespace() {
        assert_eq!(normalize_goal("  fix\t the\n\n bug "), "fix the bug");
        assert_eq!(goal_hash("fix the bug"), goal_hash(" fix\tthe  bug "));
        assert_ne!(goal_hash("fix the bug"), goal_hash("fix the bugs"));
    }

    #[test]
    fn episode_id_is_the_task_identity_and_ignores_patches_and_order() {
        let a = CitedFile {
            path: "/w/a.py".into(),
            fingerprint: Fingerprint::Sha256("aa".into()),
        };
        let b = CitedFile {
            path: "/w/b.py".into(),
            fingerprint: Fingerprint::Absent,
        };
        let id1 = episode_id("gh", &[a.clone(), b.clone()]);
        let id2 = episode_id("gh", &[b.clone(), a.clone()]);
        assert_eq!(id1, id2, "citation order must not change the identity");
        let c = CitedFile {
            path: "/w/a.py".into(),
            fingerprint: Fingerprint::Sha256("bb".into()),
        };
        assert_ne!(
            id1,
            episode_id("gh", &[c, b.clone()]),
            "a fingerprint change is a different task"
        );
        assert_ne!(
            id1,
            episode_id("gh2", &[a]),
            "a goal change is a different task"
        );
    }

    #[test]
    fn stored_row_round_trips_and_is_tagged() {
        let row = StoredRow::Tombstone {
            episode_id: "e1".into(),
        };
        let line = serde_json::to_string(&row).unwrap();
        assert!(line.contains("\"row\":\"tombstone\""), "{line}");
        let back: StoredRow = serde_json::from_str(&line).unwrap();
        assert_eq!(back, row);
    }

    #[test]
    fn episode_round_trips_through_json() {
        let record = EpisodeRecord {
            episode_id: "ep1".into(),
            goal_hash: "deadbeef".into(),
            goal_text: "implement feature X".into(),
            cited_files: vec![
                CitedFile {
                    path: "/src/main.rs".into(),
                    fingerprint: Fingerprint::Sha256("abc123".into()),
                },
                CitedFile {
                    path: "/src/lib.rs".into(),
                    fingerprint: Fingerprint::Absent,
                },
            ],
            landed_patches: vec![StoredPatch {
                path: "/src/main.rs".into(),
                codec: "search_replace".into(),
                body: "<<<<<<< ORIGINAL\nold code\n=======\nnew code\n>>>>>>> UPDATED".into(),
            }],
            run_evidence: RunEvidence {
                argv: vec!["cargo".into(), "test".into()],
                outcome: "PASSED".into(),
            },
            trajectory: vec!["step1".into(), "step2".into()],
            minted_by_model: "claude-opus".into(),
            minted_by_envelope: "v1".into(),
            status: "verified".into(),
            contradicted_by: None,
            minted_at: 1234567890,
        };

        let row = StoredRow::Episode(Box::new(record.clone()));
        let json = serde_json::to_string(&row).unwrap();
        let deserialized: StoredRow = serde_json::from_str(&json).unwrap();

        match deserialized {
            StoredRow::Episode(ep) => {
                assert_eq!(*ep, record, "episode must round-trip exactly");
            }
            _ => panic!("expected Episode variant"),
        }
    }
}
