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
    /// Two properties, and only the first is load-bearing:
    ///
    /// 1. **Same identity, same digest** — the determinism law. This is what
    ///    ruling bT7/R1 is about, and what the covering tests pin.
    /// 2. **Different identity, different digest** — a hygiene property, not
    ///    a safety one. Two different requests that shared a name would each
    ///    still get a correct workspace, because [`Scratch::materialize`]
    ///    clears the directory before writing; they would merely share a
    ///    path and a lock. It is still worth having, so every field is fed
    ///    **length-prefixed** rather than concatenated raw — otherwise
    ///    `("ab", "c")` and `("a", "bc")` would hash alike.
    ///
    /// `files` is fed in order, because order is part of the identity: the
    /// entries are written in order and a later entry can overwrite an
    /// earlier one's path.
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
/// `lock` is an exclusive `flock` held for the directory's whole lifetime —
/// see [`lock_for`] for why a content-derived name needs one. It is an
/// `Option` only so [`Scratch::drop`] can *release* it (by dropping the
/// `File`) before [`sweep_lock`] runs; it is `Some` for the whole life of a
/// live `Scratch`.
pub(crate) struct Scratch {
    dir: PathBuf,
    lock: Option<File>,
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
/// sibling of the directory.
///
/// **The lock file is now swept at teardown** (turn-4 ride-along, spec §3):
/// turn 3 left one 0-byte `.lock` per distinct request — ~999 files per
/// corpus run — in the system temp dir forever. Unlinking it is only safe
/// under a protocol, because the naive version reintroduces the very race
/// the lock removes: process B, blocked on the lock file's inode, would
/// acquire it *after* A unlinked the name, while process C creates a fresh
/// file at the same name and acquires that — two holders, one directory.
///
/// The protocol is the standard verify-after-acquire pair:
///
/// 1. **Acquire** (here): open the name, block on `flock`, then check that
///    the name still resolves to the inode we locked. If it does not, the
///    file we hold was unlinked by a departing holder — drop it and retry
///    against whatever now lives at the name.
/// 2. **Release** ([`sweep_lock`]): unlink only while holding the lock, and
///    only after confirming the name still resolves to our own inode, so
///    any process that acquires the detached inode afterwards is by
///    construction a waiter that will fail step 1's check and retry.
fn lock_for(dir: &Path) -> Result<File, String> {
    let path = dir.with_extension("lock");
    for _ in 0..LOCK_ACQUIRE_ATTEMPTS {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("failed to open the scratch lock {path:?}: {e}"))?;
        // SAFETY: `fd` is a valid, open descriptor owned by `file` for the
        // whole call, and `flock` neither takes ownership of it nor retains
        // it past return. `LOCK_EX` blocks rather than failing, which is the
        // intent.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(format!(
                "failed to lock the scratch dir {dir:?}: {}",
                std::io::Error::last_os_error()
            ));
        }
        if names_the_same_inode(&path, &file) {
            return Ok(file);
        }
        // The holder we waited behind unlinked this inode on its way out
        // (step 2). Our lock is real but guards a name nobody else will
        // open, so it guarantees nothing — drop it and lock the live file.
    }
    Err(format!(
        "failed to lock the scratch dir {dir:?}: the lock file was replaced \
         {LOCK_ACQUIRE_ATTEMPTS} times in a row"
    ))
}

/// How many times [`lock_for`] re-locks after finding the file it locked had
/// been unlinked. Each attempt only happens when another process actually
/// swept a lock, so exceeding this is a pathology (a livelock, or a
/// third party churning the file), not contention — and a named error beats
/// spinning forever.
const LOCK_ACQUIRE_ATTEMPTS: u32 = 64;

/// Whether `path` currently resolves to the same inode as the already-open
/// `file`. A missing (or unreadable) `path` answers `false`: it certainly is
/// not the same file.
fn names_the_same_inode(path: &Path, file: &File) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (std::fs::metadata(path), file.metadata()) {
        (Ok(on_disk), Ok(held)) => on_disk.dev() == held.dev() && on_disk.ino() == held.ino(),
        _ => false,
    }
}

/// Removes the scratch lock file — step 2 of [`lock_for`]'s protocol, run
/// once the directory is gone and the lock has been RELEASED.
///
/// Re-acquires non-blocking: if anyone else holds the lock (or takes it
/// between our release and this call), `LOCK_NB` fails and the file is left
/// exactly where it is — the next holder to depart sweeps it instead. The
/// inode check before `remove_file` makes sure a lock file some other
/// process created after unlinking ours is never the one we delete.
///
/// Every failure here is deliberately silent: a leftover 0-byte file is
/// hygiene, and this runs inside `Drop`, where there is nobody to report to.
fn sweep_lock(dir: &Path) {
    let path = dir.with_extension("lock");
    // No `create`: a lock file that is already gone needs no sweeping, and
    // recreating one just to delete it would be its own small race.
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) else {
        return;
    };
    // SAFETY: same contract as `lock_for`'s call — a valid open descriptor
    // owned by `file` for the duration. `LOCK_NB` returns rather than
    // blocking, which is what makes a concurrent holder a no-op here.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return;
    }
    if names_the_same_inode(&path, &file) {
        let _ = std::fs::remove_file(&path);
    }
    // `file` drops here, releasing the lock we just took.
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
        let scratch = Scratch {
            dir,
            lock: Some(lock),
        };
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
        // Order is the whole safety argument (see [`lock_for`]): the
        // directory is gone first, then the lock is RELEASED (dropping the
        // `File` closes the descriptor, which is what releases an `flock`),
        // and only then does the sweep try to take it again — so a waiter
        // that was blocked on this lock gets the directory it was waiting
        // for, and the sweep simply declines if it did.
        drop(self.lock.take());
        sweep_lock(&self.dir);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, contents: &str) -> RequestFile {
        RequestFile {
            path: path.to_string(),
            contents: contents.to_string(),
        }
    }

    fn id<'a>(target: &'a str, contents: &'a str, files: &'a [RequestFile]) -> ScratchId<'a> {
        ScratchId {
            target,
            target_contents: contents,
            find_pattern: None,
            files,
        }
    }

    /// Property 1, the load-bearing one: the digest is a pure function of
    /// the identity, so the same request always names the same directory.
    #[test]
    fn the_same_identity_always_digests_the_same() {
        let files = [file("a.py", "x = 1\n")];
        assert_eq!(
            id("a.py", "x = 1\n", &files).digest(),
            id("a.py", "x = 1\n", &files).digest()
        );
    }

    #[test]
    fn the_digest_is_sixteen_lowercase_hex_characters() {
        let d = id("a.py", "x = 1\n", &[]).digest();
        assert_eq!(d.len(), 16, "{d}");
        assert!(
            d.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{d}"
        );
    }

    /// Property 2: every field is part of the identity. Each case below
    /// differs from the baseline in exactly one field.
    #[test]
    fn every_identity_field_changes_the_digest() {
        let files = [file("a.py", "x = 1\n")];
        let base = id("a.py", "x = 1\n", &files).digest();

        let other_files = [file("b.py", "x = 1\n")];
        let other_contents = [file("a.py", "x = 2\n")];
        let two_files = [file("a.py", "x = 1\n"), file("b.py", "y = 2\n")];
        let reordered = [file("b.py", "y = 2\n"), file("a.py", "x = 1\n")];

        for (label, other) in [
            ("target", id("b.py", "x = 1\n", &files).digest()),
            ("target_contents", id("a.py", "x = 2\n", &files).digest()),
            ("files path", id("a.py", "x = 1\n", &other_files).digest()),
            (
                "files contents",
                id("a.py", "x = 1\n", &other_contents).digest(),
            ),
            ("files length", id("a.py", "x = 1\n", &two_files).digest()),
            (
                "find_pattern present",
                ScratchId {
                    find_pattern: Some("x"),
                    ..id("a.py", "x = 1\n", &files)
                }
                .digest(),
            ),
        ] {
            assert_ne!(base, other, "{label} does not change the digest");
        }

        // Order is identity: a later entry can overwrite an earlier one's
        // path, so the same set in a different order is a different
        // workspace.
        assert_ne!(
            id("a.py", "x = 1\n", &two_files).digest(),
            id("a.py", "x = 1\n", &reordered).digest(),
            "files order does not change the digest"
        );
    }

    /// The direct pin on length-prefixing: without it these two identities
    /// feed the hasher the same bytes and collide.
    #[test]
    fn fields_are_length_prefixed_so_a_shifted_split_does_not_collide() {
        assert_ne!(id("ab", "c", &[]).digest(), id("a", "bc", &[]).digest());
    }

    // -----------------------------------------------------------------
    // The lock file's lifecycle (turn-4 ride-along): swept at teardown, but
    // never out from under a concurrent holder.
    // -----------------------------------------------------------------

    /// Turn 3 left one 0-byte `.lock` per distinct request behind forever
    /// (~999 per corpus run). Dropping a `Scratch` now removes the directory
    /// AND its lock file.
    #[test]
    fn dropping_a_scratch_removes_the_directory_and_sweeps_its_lock_file() {
        let files = [file("a.py", "x = 1\n")];
        let scratch = Scratch::materialize(&id("a.py", "x = 1\n", &files))
            .expect("a one-file scratch materializes");
        let dir = scratch.path().to_path_buf();
        let lock = dir.with_extension("lock");
        assert!(dir.is_dir(), "the scratch dir exists while held");
        assert!(lock.is_file(), "the lock file exists while held");

        drop(scratch);

        assert!(!dir.exists(), "the scratch dir is removed on drop");
        assert!(!lock.exists(), "the lock file is swept on drop: {lock:?}");
    }

    /// The sweep declines when someone else holds the lock — the property
    /// that keeps the ride-along from reintroducing the collision the lock
    /// exists to prevent. (A second `flock` on a second open file
    /// description conflicts even within one process, so this test is a
    /// faithful stand-in for a second `flywheel-tool`.)
    #[test]
    fn the_sweep_leaves_a_lock_file_a_concurrent_holder_still_owns() {
        let dir = std::env::temp_dir().join(format!(
            "flywheel-tool-scratch-sweeptest-{}",
            std::process::id()
        ));
        let lock = dir.with_extension("lock");
        let _ = std::fs::remove_file(&lock);
        let holder = lock_for(&dir).expect("the lock is free");

        sweep_lock(&dir);
        assert!(
            lock.is_file(),
            "a lock another holder owns must survive the sweep: {lock:?}"
        );

        drop(holder);
        sweep_lock(&dir);
        assert!(
            !lock.exists(),
            "once released, the same lock file is swept: {lock:?}"
        );
    }

    /// An absent `find_pattern` and an empty one are different requests
    /// (`""` is a regex that matches every line), so they must not share a
    /// directory.
    #[test]
    fn an_absent_find_pattern_differs_from_an_empty_one() {
        let absent = id("a.py", "x\n", &[]).digest();
        let empty = ScratchId {
            find_pattern: Some(""),
            ..id("a.py", "x\n", &[])
        }
        .digest();
        assert_ne!(absent, empty);
    }
}
