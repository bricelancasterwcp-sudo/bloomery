use bloomery_core::scheduler::*;

fn r(id: &str, pri: u8, kv: u64, busy: bool) -> Resident {
    Resident {
        id: id.into(),
        priority: pri,
        kv_bytes: kv,
        busy,
    }
}
fn req(pri: u8, kv: u64) -> ResidencyRequest {
    ResidencyRequest {
        id: "new".into(),
        priority: pri,
        kv_bytes: kv,
    }
}

#[test]
fn fits_when_free_vram_suffices() {
    assert_eq!(plan_residency(&[], &req(100, 500), 1000), Placement::Fits);
}

#[test]
fn evicts_lowest_priority_idle_first() {
    let residents = [r("low", 10, 400, false), r("mid", 50, 400, false)];
    assert_eq!(
        plan_residency(&residents, &req(100, 300), 0),
        Placement::Evict(vec!["low".into()])
    );
}

#[test]
fn evicts_multiple_in_priority_order() {
    let residents = [r("mid", 50, 300, false), r("low", 10, 300, false)];
    assert_eq!(
        plan_residency(&residents, &req(100, 550), 0),
        Placement::Evict(vec!["low".into(), "mid".into()])
    );
}

#[test]
fn never_evicts_busy_or_equal_priority() {
    let residents = [r("busy-low", 10, 400, true), r("peer", 100, 400, false)];
    match plan_residency(&residents, &req(100, 300), 0) {
        Placement::Refuse {
            needed,
            free,
            reclaimable,
        } => {
            assert_eq!((needed, free, reclaimable), (300, 0, 0));
        }
        other => panic!("expected Refuse, got {other:?}"),
    }
}

#[test]
fn tie_break_is_deterministic() {
    let residents = [r("b", 10, 100, false), r("a", 10, 100, false)];
    // same priority, same size -> lexical id order
    assert_eq!(
        plan_residency(&residents, &req(100, 150), 0),
        Placement::Evict(vec!["a".into(), "b".into()])
    );
}
