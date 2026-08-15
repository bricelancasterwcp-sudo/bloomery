//! The Python landing lens (P1-deferred, shipped here in Task 2).
//!
//! `PythonLens::parses` is the only lens in this crate that shells out: it
//! writes `contents` to a scratch file and runs `python3 -m py_compile
//! <tmp>` on it (chosen over the `ast.parse`-on-stdin alternative the brief
//! also allows, because `py_compile` matches CPython's own parser/compiler
//! front end exactly — the same one that will later run the file — rather
//! than reimplementing the choice via a `-c` script). Exit 0 is `Ok(())`;
//! a non-zero exit is `Err(<stderr's first line>)`.
//!
//! **Fail-closed is the load-bearing contract here**, carried straight from
//! the brief: if `python3` itself cannot be spawned (missing from `PATH`,
//! or any other spawn failure), `parses` returns
//! `Err("python3 unavailable")` rather than `Ok(())`. `land()` treats any
//! `Err` from a lens as "did not land" (see `bloomery_core::action::lens`),
//! so a missing `python3` means the patch **does not land** — we cannot
//! verify the result is valid Python, so we refuse to claim it is, rather
//! than silently accepting a possibly-broken file.

use bloomery_core::action::lens::LandingLens;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Wall-clock budget for one `python3 -m py_compile` call. `py_compile`
/// parsing a single source file is a parse-and-compile pass, not an
/// execution — this is generous headroom for a slow/contended box, not an
/// estimate of typical runtime. A hang past this (a pathological input, or
/// a `python3` shim that never exits) is killed and reported as a failure,
/// same fail-closed posture as a spawn failure: an unverifiable result
/// never lands.
const PY_COMPILE_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval for the timeout loop below. Short enough that the
/// happy-path (`py_compile` exits in a few milliseconds) doesn't pay a
/// visible delay, long enough not to busy-spin.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The P1-deferred Python lens: accepts a patch's landed content only if
/// `python3` can parse it as a valid module.
pub struct PythonLens;

impl LandingLens for PythonLens {
    fn name(&self) -> &'static str {
        "python"
    }

    fn parses(&self, contents: &str) -> Result<(), String> {
        let tmp = unique_scratch_path();
        write_scratch_file(&tmp, contents)?;
        // Whatever happens below, the scratch file must not linger — it is
        // a temp-dir artifact, not something the caller (or a later test
        // run reusing the same tempdir) should ever see.
        let result = run_py_compile(&tmp);
        let _ = std::fs::remove_file(&tmp);
        result
    }
}

/// Builds a per-call-unique path under the OS temp dir. PID plus a static
/// counter (rather than PID alone): `cargo test` runs many `#[test]`
/// functions concurrently in one process, all sharing one PID, so PID alone
/// would let two concurrent `parses()` calls collide on the same scratch
/// path.
fn unique_scratch_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("bloomery-pylens-{}-{n}.py", std::process::id()))
}

/// Writes `contents` to `tmp`, applying the same create-new + `O_NOFOLLOW`
/// discipline `exec_patch`'s own atomic write uses (see `exec.rs`'s module
/// docs): the OS temp directory is normally world-writable, so a
/// predictable name (PID + counter) is guessable — `create_new` refuses to
/// write through a pre-planted file or symlink at that exact path rather
/// than following or truncating it, and `O_NOFOLLOW` is the same
/// final-component belt-and-suspenders `exec.rs` documents (a named v1
/// limit, not a complete defense, for the identical reason stated there).
fn write_scratch_file(tmp: &Path, contents: &str) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(tmp)
        .map_err(|e| format!("python lens: failed to create scratch file: {e}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("python lens: failed to write scratch file: {e}"))
}

/// Runs `python3 -m py_compile <tmp>` and turns the result into the
/// `LandingLens::parses` contract: `Ok(())` on a clean exit, `Err(..)`
/// otherwise. Every failure path — spawn failure (`python3` absent),
/// timeout, or a non-zero exit — is `Err`, never a panic and never a
/// falsely-`Ok` guess.
fn run_py_compile(tmp: &Path) -> Result<(), String> {
    let mut child = match Command::new("python3")
        .arg("-m")
        .arg("py_compile")
        .arg(tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        // `NotFound` is the exact, documented "python3 isn't on PATH"
        // shape; any other spawn failure (e.g. a transient resource
        // exhaustion) still gets the same fail-closed message, because
        // either way we could not verify the content — the caller only
        // needs to know verification did not happen, not why.
        Err(_) => return Err("python3 unavailable".to_string()),
    };

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return finish(child, status),
            Ok(None) if started.elapsed() >= PY_COMPILE_TIMEOUT => {
                // Kill, then reap: without the `wait` the child would be
                // left a zombie for the life of the daemon process.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "python3 -m py_compile timed out after {}s",
                    PY_COMPILE_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(e) => return Err(format!("python lens: failed to wait for python3: {e}")),
        }
    }
}

/// Reads the exit status of an already-reaped `child`'s stderr and turns it
/// into the `parses()` contract's `Result`.
fn finish(mut child: std::process::Child, status: std::process::ExitStatus) -> Result<(), String> {
    if status.success() {
        return Ok(());
    }
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    let text = String::from_utf8_lossy(&stderr);
    let first_line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("py_compile failed with no stderr output")
        .trim();
    Err(first_line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_python() {
        assert_eq!(PythonLens.name(), "python");
    }

    #[test]
    fn a_syntactically_valid_module_parses_ok_or_fails_closed() {
        // Deterministic in both worlds (see this crate's
        // `tests/task_exec_patch_test.rs` module docs for the same
        // reasoning applied to the executor-level test): if `python3` is
        // present, "x = 1\n" really does parse; if it is absent, this
        // still returns `Err("python3 unavailable")`, which is this
        // lens's documented fail-closed contract, not a bug. Either way
        // the call must not panic.
        let result = PythonLens.parses("x = 1\n");
        if let Err(e) = &result {
            assert_eq!(e, "python3 unavailable", "unexpected failure: {e}");
        }
    }

    #[test]
    fn a_syntax_error_is_rejected_when_python3_is_present() {
        let result = PythonLens.parses("def (:\n");
        match result {
            Ok(()) => panic!("invalid Python must never report Ok"),
            Err(e) => assert!(!e.is_empty(), "error message must not be empty"),
        }
    }
}
