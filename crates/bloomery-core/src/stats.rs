/// Compute Wilson score interval (95% confidence) for a binomial proportion.
///
/// Given `passes` successes out of `n` trials, returns the lower and upper bounds of a
/// 95% confidence interval for the true success rate. Used in verdict logic: when a
/// measurement sits near a threshold, the interval width determines whether the verdict
/// is clear or ambiguous.
///
/// This is a direct port of assay's reference implementation (`profile.py::wilson95`).
/// The z-value is fixed at 1.959963984540054 for 95% confidence. For n=0, returns the
/// vacuous interval (0.0, 1.0).
///
/// # Arguments
///
/// * `passes` — number of successes (0 ≤ passes ≤ n)
/// * `n` — total number of trials
///
/// # Returns
///
/// A tuple `(lo, hi)` representing the lower and upper bounds of the confidence interval,
/// clamped to [0, 1].
pub fn wilson95(passes: u32, n: u32) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    debug_assert!(passes <= n);
    let z = 1.959_963_984_540_054_f64;
    let n_f = f64::from(n);
    let phat = f64::from(passes) / n_f;
    let denom = 1.0 + z * z / n_f;
    let centre = phat + z * z / (2.0 * n_f);
    let margin = z * ((phat * (1.0 - phat) + z * z / (4.0 * n_f)) / n_f).sqrt();
    (
        ((centre - margin) / denom).max(0.0),
        ((centre + margin) / denom).min(1.0),
    )
}
