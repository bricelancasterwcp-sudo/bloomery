//! Run executor tests (Phase 2b/2c P3 Task 3) — the brief's six binding
//! acceptance tests for `exec_run`, the most security-sensitive executor
//! in the task module.
//!
//! Every test here spawns a real subprocess (`echo`, `sleep`, `false`,
//! `printf`) — GPU-free, fast, deterministic — rather than mocking
//! `std::process::Command`, matching this module's established pattern
//! (`task_exec_read_find_test.rs`'s module docs make the same argument for
//! real filesystem I/O over mocked `fs::canonicalize`): the whole point of
//! `exec_run` is its behavior against a *real* OS process (no shell, a
//! scrubbed real environment, a real kill+reap), so a mock would test
//! nothing of what actually matters.

use bloomery_core::grant::Grant;
use bloomery_daemon::task::{exec_run, ExecBounds};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn s(x: &str) -> String {
    x.to_string()
}

fn bounds() -> ExecBounds {
    ExecBounds {
        read_cap_bytes: 256 * 1024,
        find_result_cap: 100,
        run_output_cap_bytes: 64 * 1024,
        run_timeout_secs: 120,
    }
}

/// Builds a fresh, per-call-unique tempdir as the task's `cwd`, and a
/// `Grant` scoped to it whose `commands` are exactly `granted` (this suite
/// never exercises path grants — only `exec_run`'s
/// `grant.check_command` — so read/write roots are both the sandbox
/// itself, mirroring `task_exec_read_find_test.rs::sandbox`'s shape).
fn sandbox(granted: &[&[&str]]) -> (PathBuf, Grant) {
    static UNIQUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-exec-run-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();

    let commands: Vec<String> = granted
        .iter()
        .map(|prefix| {
            let items: Vec<String> = prefix.iter().map(|s| format!("\"{s}\"")).collect();
            format!("[{}]", items.join(","))
        })
        .collect();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{d}"],"write_roots":["{d}"],"commands":[{c}]}}"#,
        d = dir.display(),
        c = commands.join(",")
    ))
    .unwrap();
    (dir, g)
}

#[test]
fn a_granted_command_runs_and_captures_output() {
    let (cwd, g) = sandbox(&[&["echo"]]);
    let obs = exec_run(&g, &cwd, &[s("echo"), s("hello")], &bounds());

    assert!(!obs.failed);
    assert!(obs.content.contains("hello"), "content: {}", obs.content);
    assert_eq!(obs.outcome, "ran echo exit 0");
}

#[test]
fn a_nonzero_exit_is_reported_but_not_a_step_failure() {
    let (cwd, g) = sandbox(&[&["false"]]);
    let obs = exec_run(&g, &cwd, &[s("false")], &bounds());

    assert!(
        !obs.failed,
        "a completed run at a non-zero exit code must not be a step failure"
    );
    assert_eq!(obs.outcome, "ran false exit 1");
    assert!(obs.content.contains("exit 1"), "content: {}", obs.content);
}

/// Binding proof from the brief: an ungranted command is refused **before
/// any process is spawned**. Uses an `rm`-shaped argv against a canary
/// file in the sandbox — if `exec_run` ever spawned the process (say, by
/// checking the grant after spawning, or not at all), the canary would be
/// gone.
#[test]
fn an_ungranted_command_is_refused_without_spawning() {
    let (cwd, g) = sandbox(&[&["echo"]]);
    let canary = cwd.join("canary.txt");
    std::fs::write(&canary, "do not delete me").unwrap();

    let obs = exec_run(
        &g,
        &cwd,
        &[s("rm"), s("-f"), canary.display().to_string()],
        &bounds(),
    );

    assert!(obs.failed);
    assert!(
        obs.outcome.contains("grant violation"),
        "outcome: {}",
        obs.outcome
    );
    assert!(
        canary.exists(),
        "the subprocess must never have been spawned — the canary file was removed"
    );
    assert_eq!(
        std::fs::read_to_string(&canary).unwrap(),
        "do not delete me"
    );
}

#[test]
fn a_command_exceeding_the_timeout_is_killed_and_named() {
    let (cwd, g) = sandbox(&[&["sleep"]]);
    let mut b = bounds();
    b.run_timeout_secs = 1;

    let started = Instant::now();
    let obs = exec_run(&g, &cwd, &[s("sleep"), s("10")], &b);
    let elapsed = started.elapsed();

    assert!(obs.failed);
    assert!(
        obs.outcome.contains("timed out"),
        "outcome: {}",
        obs.outcome
    );
    assert!(
        elapsed < Duration::from_secs(4),
        "took {elapsed:?}; expected the ~1s timeout to fire, not the full 10s sleep"
    );
}

#[test]
fn output_over_the_cap_is_truncated_with_notice() {
    let (cwd, g) = sandbox(&[&["printf"]]);
    let mut b = bounds();
    b.run_output_cap_bytes = 16;

    let long = "A".repeat(100);
    let obs = exec_run(&g, &cwd, &[s("printf"), s("%s"), long.clone()], &b);

    assert!(
        !obs.failed,
        "a completed run — even with truncated output — is not a step failure"
    );
    assert!(
        obs.content.contains("truncated"),
        "content: {}",
        obs.content
    );
    assert!(
        !obs.content.contains(&long),
        "the full untruncated output must not appear: {}",
        obs.content
    );
    let over_cap_run = "A".repeat(b.run_output_cap_bytes + 1);
    assert!(
        !obs.content.contains(&over_cap_run),
        "must never keep more than the cap's worth of raw output: {}",
        obs.content
    );
}

/// Binding proof from the brief: `argv`'s elements reach the child exactly
/// as given, with no shell ever in the loop. `echo`'s own single argument
/// contains shell metacharacters (`$HOME`, `;`, a second "command"); if
/// `exec_run` ever ran this through `sh -c` (or any shell), `$HOME` would
/// expand to a real path and `;rm -rf /` would attempt a second command.
/// Run directly (`execve`, no shell), `echo` receives it as one literal
/// argument and prints it back unexpanded.
#[test]
fn no_shell_interpretation() {
    let (cwd, g) = sandbox(&[&["echo"]]);
    let obs = exec_run(&g, &cwd, &[s("echo"), s("$HOME;rm -rf /")], &bounds());

    assert!(!obs.failed);
    assert!(
        obs.content.contains("$HOME;rm -rf /"),
        "expected the literal, unexpanded string in content: {}",
        obs.content
    );
}

/// This suite's own adversarial addition (beyond the brief's six), mirroring
/// `task_exec_read_find_test.rs`'s pattern of pinning a security property
/// directly rather than trusting the six behavioral tests to catch a
/// regression by coincidence: the child's environment is *exactly*
/// `PATH`/`HOME`/`LANG`, never whatever the daemon process itself was
/// carrying. Sets a fake secret in *this test process's* environment before
/// calling `exec_run`, then runs `env` (which dumps its own environment) and
/// asserts the secret never reaches the child, and that exactly the three
/// documented variables came through.
#[test]
fn env_is_scrubbed_to_exactly_path_home_lang() {
    // SAFETY (not `unsafe` — plain `std::env::set_var`, but noted for the
    // reviewer): this mutates process-wide state, safe here because
    // `cargo test` runs each test in its own thread but this crate's test
    // binaries don't otherwise read `BLOOMERY_TEST_SECRET`, so no other
    // test can observe or race this value.
    std::env::set_var("BLOOMERY_TEST_SECRET", "leak-me-not");

    let (cwd, g) = sandbox(&[&["env"]]);
    let obs = exec_run(&g, &cwd, &[s("env")], &bounds());

    std::env::remove_var("BLOOMERY_TEST_SECRET");

    assert!(!obs.failed);
    assert!(
        !obs.content.contains("BLOOMERY_TEST_SECRET"),
        "the parent process's env var leaked into the child: {}",
        obs.content
    );
    assert!(
        !obs.content.contains("leak-me-not"),
        "the parent process's secret value leaked into the child: {}",
        obs.content
    );
    assert!(
        obs.content.contains("PATH=/usr/bin:/bin"),
        "{}",
        obs.content
    );
    assert!(
        obs.content.contains(&format!("HOME={}", cwd.display())),
        "{}",
        obs.content
    );
    assert!(obs.content.contains("LANG=C"), "{}", obs.content);

    let env_lines: usize = obs
        .content
        .lines()
        .filter(|l| l.contains('=') && !l.starts_with("exit"))
        .count();
    assert_eq!(
        env_lines, 3,
        "expected exactly PATH, HOME, LANG — got: {}",
        obs.content
    );
}

/// The security reviewer's exact repro for a real CRITICAL: a granted
/// command that backgrounds a descendant before its own (direct child)
/// process exits. Before the fix this test pins, `exec_run` only ever
/// signaled the direct child's own PID — a backgrounded descendant
/// survives that untouched, keeps the stdout/stderr pipe's write end
/// open, and the drain threads (which read those pipes to EOF) block
/// forever; joining them unconditionally then meant `exec_run` itself
/// never returned, wedging the task thread indefinitely, while the
/// descendant leaked as a permanently running orphan — an unbounded DoS
/// on exactly the cargo/make/pytest-shaped workload this executor exists
/// to run.
///
/// This grants `sh` directly (an operator choice the test itself makes,
/// not `exec_run` invoking a shell implicitly — that separate guarantee
/// is `no_shell_interpretation`, still true and unaffected by this fix)
/// and runs `sh -c 'sleep 30 & echo $! > <pidfile>; exit 0'`: the direct
/// child (`sh`) exits almost immediately with status 0. This is
/// deliberately the *clean-exit*, not-a-timeout half of the bug — a fix
/// that only swept the process group on timeout would still miss it,
/// because no timeout is ever reached here.
///
/// Mechanism this test proves: `exec_run` now (1) puts the child in its
/// own new process group at spawn (`process_group(0)`) and (2) sends
/// `SIGKILL` to that *whole group* — not just the direct child's PID —
/// unconditionally once the direct child is no longer running, including
/// right after an ordinary clean exit. A plain background job (`&`, no
/// `setsid`) stays in its parent's process group, so the group-wide kill
/// reaches it. Independently, (3) `exec_run` only ever waits a short
/// bounded deadline for the drain threads to notice their pipe closed
/// before detaching (not joining) them — see `task/exec_run.rs`'s module
/// docs — which is what guarantees `exec_run` itself returns promptly
/// even in a hypothetical case where the group kill alone weren't enough.
#[test]
fn a_backgrounded_descendant_is_killed_and_does_not_wedge_the_call() {
    let (cwd, g) = sandbox(&[&["sh"]]);
    let pidfile = cwd.join("bg.pid");
    let script = format!("sleep 30 & echo $! > {}; exit 0", pidfile.display());
    let mut b = bounds();
    b.run_timeout_secs = 5; // generous vs. the fix's own bound, tiny vs. 30s

    let started = Instant::now();
    let obs = exec_run(&g, &cwd, &[s("sh"), s("-c"), script], &b);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(10),
        "exec_run must return promptly even though the backgrounded sleep \
         is nominally 'running' for 30s of wall-clock time; took {elapsed:?}"
    );
    assert!(
        !obs.failed,
        "the direct child (sh) exited 0 — this is not a timeout or a grant violation: {obs:?}"
    );
    assert_eq!(obs.outcome, "ran sh exit 0");

    let pid_text = std::fs::read_to_string(&pidfile)
        .expect("the backgrounded job's PID must have been written before sh exited");
    let pid: i32 = pid_text.trim().parse().expect("a PID");

    // The background `sleep` must actually be dead — not merely detached
    // from our own wait, which would leave it running for the full 30s as
    // a live DoS. Poll /proc briefly: SIGKILL delivery (and reaping by
    // whatever process becomes its new parent once `sh` is killed) is
    // near-instant but not synchronous with `exec_run`'s return, so allow
    // a short grace window rather than asserting the instant it returns.
    let proc_path = std::path::PathBuf::from(format!("/proc/{pid}"));
    let deadline = Instant::now() + Duration::from_secs(2);
    while proc_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !proc_path.exists(),
        "backgrounded sleep pid {pid} is still alive after exec_run returned — \
         the process-group kill did not reach it"
    );
}

/// Control for the test above: an ordinary quick command with no
/// descendants still returns fast and clean — the process-group kill and
/// bounded drain-join added by this fix must not slow down or otherwise
/// change the common case.
#[test]
fn a_normal_quick_command_still_returns_fast_and_clean() {
    let (cwd, g) = sandbox(&[&["echo"]]);

    let started = Instant::now();
    let obs = exec_run(&g, &cwd, &[s("echo"), s("hi")], &bounds());
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "a plain echo must not be slowed down by the group-kill/bounded-join \
         machinery; took {elapsed:?}"
    );
    assert!(!obs.failed);
    assert!(obs.content.contains("hi"));
}
