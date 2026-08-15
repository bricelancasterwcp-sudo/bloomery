use bloomery_core::stats::wilson95;

#[test]
fn wilson95_golden_35_35() {
    let (lo, hi) = wilson95(35, 35);
    assert!((lo - 0.901099).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 1.000000).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_20_20() {
    let (lo, hi) = wilson95(20, 20);
    assert!((lo - 0.838875).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 1.000000).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_16_20() {
    let (lo, hi) = wilson95(16, 20);
    assert!((lo - 0.583983).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 0.919342).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_15_20() {
    let (lo, hi) = wilson95(15, 20);
    assert!((lo - 0.531299).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 0.888138).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_12_20() {
    let (lo, hi) = wilson95(12, 20);
    assert!((lo - 0.386582).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 0.781193).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_0_20() {
    let (lo, hi) = wilson95(0, 20);
    assert!((lo - 0.000000).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 0.161125).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_10_20() {
    let (lo, hi) = wilson95(10, 20);
    assert!((lo - 0.299298).abs() < 1e-6, "lo mismatch: {}", lo);
    assert!((hi - 0.700702).abs() < 1e-6, "hi mismatch: {}", hi);
}

#[test]
fn wilson95_golden_0_0() {
    let (lo, hi) = wilson95(0, 0);
    assert_eq!(lo, 0.0, "lo mismatch for n=0");
    assert_eq!(hi, 1.0, "hi mismatch for n=0");
}
