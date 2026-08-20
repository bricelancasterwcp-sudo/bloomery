//! `flywheel-tool`'s throwaway workspace: the directory a trajectory's real
//! executor calls actually run against, materialized from the request's
//! `files` and torn down on drop.
//!
//! Split out of `flywheel_tool.rs` alongside `render.rs` when turn 3's two
//! new shapes pushed that file past the repo's 800-line ceiling. The
//! boundary is the mirror of `render.rs`'s: everything here touches the
//! filesystem and nothing here produces a byte of trained text.
//!
//! This is the piece that makes the parent module's central claim true for
//! turn 3 — "every observation comes from a real executor call". An
//! observation can only be real if there is a real directory to observe, so
//! [`Scratch`] builds one per request, hands out the [`Grant`] the
//! executors run under, and removes it again however the handler exits.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bloomery_core::grant::Grant;
use bloomery_daemon::task::{exec_read, ExecBounds, Observation};

use super::TrajectoryRequest;

/// One entry of a request's `files` — a workspace-relative path and its
/// exact contents. `path` is validated by [`safe_relative`] before anything
/// is written: a materialized scratch dir must never be a way to write
/// outside itself.
#[derive(serde::Deserialize)]
pub(crate) struct RequestFile {
    pub(crate) path: String,
    pub(crate) contents: String,
}

/// Rejects any `files` entry path that is not strictly inside the scratch
/// directory. A request is factory-authored, not user input, but a wire
/// field that names where bytes get written is exactly the field a factory
/// bug (or a bad template) turns into a write outside the throwaway dir —
/// so absolute paths, `..`, and root/prefix components are refused by name
/// rather than joined and hoped about.
pub(crate) fn safe_relative(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let escapes = p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if p.is_absolute() || escapes || path.is_empty() {
        return Err(format!(
            "file path {path:?} must be a non-empty path relative to the workspace root, with \
             no \"..\" component"
        ));
    }
    Ok(p.to_path_buf())
}

/// A throwaway directory this binary owns end to end: created fresh per
/// call, populated from a request's `files`, and removed on drop (including
/// on every early-return error path below, which is why this is a `Drop`
/// type rather than a pair of function calls).
pub(crate) struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// Creates the directory and writes every entry of `files` into it.
    /// `tag` only distinguishes concurrent scratch dirs by shape in a
    /// process listing; uniqueness comes from the pid plus a monotonic
    /// counter.
    pub(crate) fn materialize(tag: &str, files: &[RequestFile]) -> Result<Scratch, String> {
        static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("flywheel-tool-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create the {tag} scratch dir: {e}"))?;
        let scratch = Scratch { dir };
        for file in files {
            let path = scratch.dir.join(safe_relative(&file.path)?);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {parent:?} in the scratch dir: {e}"))?;
            }
            std::fs::write(&path, &file.contents)
                .map_err(|e| format!("failed to write {path:?} in the scratch dir: {e}"))?;
        }
        Ok(scratch)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    /// Builds the real [`Grant`] every executor call in this scratch dir
    /// runs under: readable, never writable through the grant (nothing here
    /// patches through `exec_patch` — landing is in-memory via `land`, and
    /// the one file this binary rewrites on disk it writes itself), and
    /// carrying exactly the request's declared `commands` so a `run_argv`
    /// outside them is refused by the real allowlist rather than trusted.
    pub(crate) fn grant(&self, commands: &[Vec<String>]) -> Result<Grant, String> {
        let wire = serde_json::json!({
            "read_roots": [&self.dir],
            "write_roots": [],
            "commands": commands,
        });
        Grant::from_json(&wire.to_string())
            .map_err(|e| format!("failed to build the scratch grant: {e}"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The files a shape's scratch dir gets. A request that carries `files`
/// gets exactly those; one that does not falls back to the single-file
/// workspace `{target: target_contents}`, so the spec's single-file
/// run-verified slice need not restate its own target in `files` just to
/// have somewhere to run.
pub(crate) fn files_to_materialize(req: &TrajectoryRequest) -> Vec<RequestFile> {
    if req.files.is_empty() {
        return vec![RequestFile {
            path: req.target.clone(),
            contents: req.target_contents.clone(),
        }];
    }
    req.files
        .iter()
        .map(|f| RequestFile {
            path: f.path.clone(),
            contents: f.contents.clone(),
        })
        .collect()
}

/// Runs the REAL [`exec_read`] of `req.target` inside `scratch`, and checks
/// that what the model would actually see there is what the request says
/// the target holds. A mismatch means `files` and `target_contents`
/// disagree — the trajectory would train a `patch` against bytes the
/// transcript never showed — so it is a named error, not a silent
/// preference for one of the two.
pub(crate) fn real_target_read(
    scratch: &Scratch,
    grant: &Grant,
    req: &TrajectoryRequest,
) -> Result<Observation, String> {
    let observation = exec_read(
        grant,
        scratch.path(),
        &req.target,
        None,
        &ExecBounds::default(),
    );
    if observation.failed {
        return Err(format!(
            "reading {t:?} inside the scratch dir materialized from \"files\" failed ({o}) — a \
             request that selects a find/run shape must carry {t:?} among its \"files\"; this is \
             a factory bug, not a tool bug",
            t = req.target,
            o = observation.outcome
        ));
    }
    if observation.content != req.target_contents {
        return Err(format!(
            "the real read of {t:?} in the scratch dir ({a} bytes) differs from the request's \
             \"target_contents\" ({b} bytes) — \"files\" and \"target_contents\" must agree, or \
             the rendered patch would act on bytes the rendered transcript never showed",
            t = req.target,
            a = observation.content.len(),
            b = req.target_contents.len()
        ));
    }
    Ok(observation)
}

/// Reads the exit code back out of a real `exec_run` observation.
///
/// `exec_run` reports a *completed* run as `failed: false` at any exit code
/// on purpose (its own docs: "a non-zero exit is a legitimate observation
/// the model acts on, not an executor failure"), and `Observation` carries
/// no structured code — so the only place the code exists is the
/// observation's own first content line. This **parses** that line; it
/// never re-renders it. A first line this cannot parse is a named error
/// rather than a silent zero, so a drift in `exec_run`'s
/// `"exit {code}\n{output}"` content format announces itself here instead
/// of quietly turning every verification into a pass.
pub(crate) fn run_exit_code(observation: &Observation) -> Result<i32, String> {
    let first = observation.content.lines().next().unwrap_or_default();
    first
        .strip_prefix("exit ")
        .and_then(|code| code.parse::<i32>().ok())
        .ok_or_else(|| {
            format!(
                "could not read an exit code out of exec_run's observation (first content line \
                 {first:?}): flywheel-tool's run verification reads the code back from that \
                 line, and exec_run's content format has drifted from \"exit {{code}}\\n{{output}}\""
            )
        })
}
