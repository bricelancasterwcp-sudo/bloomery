//! Fixtures shared by the `swap_*` tests: the cover-gate fakes and the job
//! harness (design §4's flow).
//!
//! Split out on 2026-09-01 (carried-debt slice D).

use bloomery_daemon::swap::CoverGate;

/// Every `(program, argv)` the gate spawned, in order. `Rc`/`RefCell` because
/// a [`bloomery_daemon::post::CommandRunner`] is deliberately not `Send` and
/// these tests are single-threaded.
pub type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<String>)>>>;

/// The signal `signalled` reports. Named rather than spelled `9` inline: the
/// number is a kernel constant, not a knob of this test.
pub const SIGKILL: i32 = 9;

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
pub fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

/// A wait status for a child killed by `SIGKILL`: no exit code at all, which
/// is the case `exit_code: None` exists for. The same construction
/// `drift_test.rs::signalled` makes, with the signal fixed — this file has
/// exactly one signal case and nothing here varies by which signal it was.
pub fn signalled() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(SIGKILL)
}

pub fn output(status: std::process::ExitStatus, stderr: &str) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// A gate whose subprocess is scripted: it records every spawn and answers
/// with `status` and no stderr.
pub fn gate_answering(status: std::process::ExitStatus) -> (CoverGate, Calls) {
    gate_saying(status, "")
}

/// The same, with assay given words of its own — what the infrastructure
/// details carry through verbatim for the operator.
pub fn gate_saying(status: std::process::ExitStatus, stderr: &str) -> (CoverGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let stderr = stderr.to_string();
    let gate = CoverGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        Ok(output(status, &stderr))
    }));
    (gate, calls)
}
