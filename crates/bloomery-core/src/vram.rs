/// Probe free VRAM in bytes via nvidia-smi.
///
/// The injectable runner allows tests to pass mock output without spawning processes.
/// The real caller will pass a runner that invokes:
/// `nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits`
///
/// Returns `Some(bytes)` if the query succeeds and output is parseable as MiB,
/// or `None` if the binary is missing, output is empty, or unparseable.
/// Never returns 0 — unmeasured is `None`.
pub fn free_vram_bytes<R: Fn(&str, &[&str]) -> std::io::Result<String>>(run: R) -> Option<u64> {
    let output = run(
        "nvidia-smi",
        &["--query-gpu=memory.free", "--format=csv,noheader,nounits"],
    )
    .ok()?;

    // Take first line and trim
    let first_line = output.lines().next()?.trim();

    // Parse as u64 MiB, multiply by 1024*1024 to get bytes
    first_line.parse::<u64>().ok().map(|mib| mib * 1024 * 1024)
}
