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
//!
//! **The directory's NAME is part of the trained bytes** (controller ruling
//! bT7/R1, 2026-08-20). `exec_find` renders each hit as
//! `{canonicalized absolute path}:{lineno}: {line}`, so whatever this module
//! calls its scratch directory ends up verbatim inside every find-shaped
//! trajectory's transcript. The first implementation named it from the pid
//! plus a counter, which made two same-seed factory runs differ in exactly
//! the find rows (999 of 4263 measured) and broke the factory's determinism
//! law — same seed, byte-identical corpus.
//!
//! The ruled fix is [`ScratchId`]: the name is a digest of the directory's
//! own *content identity*, so two identical requests materialize at
//! identical paths and render identical bytes. Note what this deliberately
//! does NOT do — it does not post-process the rendered observation, and it
//! does not ask the factory to rewrite anything. The find hit stays exactly
//! what the real executor emitted, absolute path and all; determinism comes
//! from making the input to that executor reproducible, which is the only
//! kind of fix that leaves the observation real.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};

use bloomery_core::grant::Grant;
use bloomery_daemon::task::{exec_read, ExecBounds, Observation};
use sha2::{Digest, Sha256};

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

/// Everything about a request that determines what its scratch directory
/// will contain — and therefore, under ruling bT7/R1, everything the
/// directory's name is derived from.
///
/// `target`/`target_contents`/`find_pattern` are named by the ruling and
/// carried even though `files` already implies the directory's bytes: they
/// cost nothing and they keep two requests that merely *happen* to
/// materialize the same files from sharing a name.
///
/// Deliberately absent: `run_argv`, `search`, `replace`. The run shape
/// rewrites the scratch copy of `target` mid-flight, so a directory's
/// contents at the *end* of a run request are not what this identifies —
/// but they need not be. The stdin protocol is strictly sequential and
/// [`Scratch`] removes the directory on drop, so the next request always
/// starts from a clean materialize, and [`Scratch::materialize`] removes any
/// leftover first in case a previous process died before its `Drop` ran.
pub(crate) struct ScratchId<'a> {
    pub(crate) target: &'a str,
    pub(crate) target_contents: &'a str,
    pub(crate) find_pattern: Option<&'a str>,
    pub(crate) files: &'a [RequestFile],
}

impl ScratchId<'_> {
    /// The directory name's stable suffix: the first 16 hex characters of a
    /// SHA-256 over this identity.
    ///
    /// Every field is fed **length-prefixed**, never concatenated raw —
    /// otherwise `("ab", "c")` and `("a", "bc")` would hash alike and two
    /// genuinely different workspaces could land in one directory. `files`
    /// is fed in order, because order is part of the identity: the entries
    /// are written in order and a later entry can overwrite an earlier one's
    /// path.
    fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        let mut feed = |bytes: &[u8]| {
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        feed(self.target.as_bytes());
        feed(self.target_contents.as_bytes());
        match self.find_pattern {
            // An absent pattern and an empty one are different requests, so
            // they get different tags rather than both feeding "".
            Some(pattern) => {
                feed(b"find_pattern");
                feed(pattern.as_bytes());
            }
            None => feed(b"no_find_pattern"),
        }
        feed(&(self.files.len() as u64).to_le_bytes());
        for file in self.files {
            feed(file.path.as_bytes());
            feed(file.contents.as_bytes());
        }
        let full = hasher.finalize();
        full.iter().take(8).map(|b| format!("{b:02x}")).collect()
    }
}

/// A throwaway directory this binary owns end to end: named from its own
/// content identity, populated from a request's `files`, and removed on drop
/// (including on every early-return error path below, which is why this is a
/// `Drop` type rather than a pair of function calls).
///
/// `_lock` is an exclusive `flock` held for the directory's whole lifetime —
/// see [`lock_for`] for why a content-derived name needs one.
pub(crate) struct Scratch {
    dir: PathBuf,
    _lock: File,
}

/// Takes an exclusive advisory lock covering the scratch directory at `dir`,
/// blocking until it is available, and returns the handle whose drop
/// releases it.
///
/// **Why this exists.** Ruling bT7/R1 makes the directory's name a function
/// of the request, which is what buys determinism — and which also means two
/// `flywheel-tool` processes handling the *identical* request want the
/// identical directory at the identical moment. Without a lock that is not a
/// clean error: one process's `remove_dir_all` lands between the other's
/// write and its `exec_find`, and the loser renders a trajectory built on a
/// half-deleted workspace. A wrong observation that still looks like a valid
/// one is the worst failure this binary can have, so the collision is
/// serialized rather than documented away. (Measured, not hypothesized: with
/// the content-derived name and no lock, `cargo test`'s parallel threads —
/// each spawning a tool process on the same fixture — failed 3 runs out of 5.)
///
/// `flock` and not a lock*file*-as-mutex on purpose: the kernel releases a
/// `flock` when the holding process dies, so a crashed run cannot wedge
/// every later run at the same request. The lock file itself is a 0-byte
/// sibling of the directory and is deliberately never unlinked — unlinking
/// it while another process is blocked on it would hand that process a lock
/// on a detached inode, which is exactly the race this is here to remove.
fn lock_for(dir: &Path) -> Result<File, String> {
    let path = dir.with_extension("lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("failed to open the scratch lock {path:?}: {e}"))?;
    // SAFETY: `fd` is a valid, open descriptor owned by `file` for the whole
    // call, and `flock` neither takes ownership of it nor retains it past
    // return. `LOCK_EX` blocks rather than failing, which is the intent.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(format!(
            "failed to lock the scratch dir {dir:?}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(file)
}

impl Scratch {
    /// Creates the directory at the content-derived path and writes every
    /// entry of `id.files` into it.
    ///
    /// The `remove_dir_all` before `create_dir_all` is **load-bearing**, not
    /// hygiene: under a content-derived name a leftover directory from a
    /// process that died before its `Drop` ran sits at exactly the path this
    /// request wants, and any stale file in it would show up in this
    /// request's `exec_find` results as a real hit. Clearing first is what
    /// makes the observation a function of the request alone.
    ///
    /// Two *concurrent* processes fed the identical request derive the
    /// identical path by construction; [`lock_for`] serializes them rather
    /// than letting them corrupt each other's workspace. The factory drives
    /// this binary as a single sequential stdin/stdout pipe, so in the
    /// production regime the lock is uncontended.
    pub(crate) fn materialize(id: &ScratchId<'_>) -> Result<Scratch, String> {
        // Validate every path BEFORE creating anything, so a request that
        // was going to be refused never leaves a directory behind.
        let targets: Vec<PathBuf> = id
            .files
            .iter()
            .map(|file| safe_relative(&file.path))
            .collect::<Result<_, _>>()?;

        let dir = std::env::temp_dir().join(format!("flywheel-tool-scratch-{}", id.digest()));
        // Locked BEFORE the directory is touched: the clear-and-recreate
        // below is precisely the window another process must not be reading
        // through.
        let lock = lock_for(&dir)?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create the scratch dir {dir:?}: {e}"))?;
        let scratch = Scratch { dir, _lock: lock };
        for (file, relative) in id.files.iter().zip(targets) {
            let path = scratch.dir.join(relative);
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
