use bloomery_core::budget::{Budget, BudgetExhausted};

#[test]
fn check_refuses_with_arithmetic() {
    let b = Budget::new(100);
    assert_eq!(
        b.check(101),
        Err(BudgetExhausted {
            remaining: 100,
            requested: 101
        })
    );
    assert!(b.check(100).is_ok());
}

#[test]
fn charge_records_actuals_even_past_granted() {
    let mut b = Budget::new(100);
    b.charge(60);
    assert_eq!((b.spent(), b.remaining()), (60, 40));
    b.charge(60); // actual usage exceeded the estimate — record honestly
    assert_eq!((b.spent(), b.remaining()), (120, 0));
    assert!(b.check(1).is_err());
}

#[test]
fn charge_saturates_instead_of_overflowing() {
    let mut b = Budget::new(u64::MAX);
    b.charge(u64::MAX); // spent = u64::MAX
    assert_eq!((b.spent(), b.remaining()), (u64::MAX, 0));
    b.charge(1); // would overflow, should saturate
    assert_eq!((b.spent(), b.remaining()), (u64::MAX, 0));
}
