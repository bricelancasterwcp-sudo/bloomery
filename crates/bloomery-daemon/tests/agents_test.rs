use bloomery_daemon::agents::{model_digest, ImageFetch, ImageStore};

#[test]
fn image_ram_then_spill_then_take() {
    let dir = std::env::temp_dir().join("bloomery-img-test");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "digestX", vec![1, 2, 3]);
    match st.take("a1", "digestX") {
        ImageFetch::Ram(b) => assert_eq!(b, vec![1, 2, 3]),
        o => panic!("{o:?}"),
    }
    st.put_ram("a1", "digestX", vec![1, 2, 3]);
    st.spill("a1").unwrap();
    match st.take("a1", "digestX") {
        ImageFetch::Nvme(b) => assert_eq!(b, vec![1, 2, 3]),
        o => panic!("{o:?}"),
    }
}

#[test]
fn stale_digest_is_invalidation_not_error() {
    let dir = std::env::temp_dir().join("bloomery-img-test2");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "old", vec![9]);
    assert!(matches!(st.take("a1", "new"), ImageFetch::StaleDigest));
    assert!(matches!(st.take("nobody", "d"), ImageFetch::Missing));
}

#[test]
fn digest_changes_with_content() {
    let dir = std::env::temp_dir().join("bloomery-digest-test");
    std::fs::create_dir_all(&dir).unwrap();
    let (p1, p2) = (dir.join("m1"), dir.join("m2"));
    std::fs::write(&p1, b"AAAA").unwrap();
    std::fs::write(&p2, b"BBBB").unwrap();
    assert_ne!(model_digest(&p1).unwrap(), model_digest(&p2).unwrap());
}

/// Binding obligation from Task 11's review: an over-long or truncated KV
/// image must never restore as a bogus success. `spill` records the byte
/// length alongside the digest at save time; `take` must verify the bytes it
/// actually reads back off disk match that recorded length before handing
/// them to a caller, and report `Corrupt` (cold-start territory, not a
/// silent short read) when they don't.
#[test]
fn spilled_image_truncated_on_disk_is_corrupt_not_bogus_success() {
    let dir = std::env::temp_dir().join("bloomery-img-test-corrupt");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "digestX", vec![1, 2, 3, 4, 5]);
    st.spill("a1").unwrap();

    let spilled_path = dir.join("a1.digestX.kvimg");
    assert!(
        spilled_path.exists(),
        "spill must write {{id}}.{{digest}}.kvimg"
    );
    std::fs::write(&spilled_path, vec![1, 2]).unwrap();

    match st.take("a1", "digestX") {
        ImageFetch::Corrupt => {}
        o => panic!("{o:?}"),
    }
}

#[test]
fn agent_table_insert_get_remove_and_residents() {
    use bloomery_core::budget::Budget;
    use bloomery_core::geometry::{BoundBy, Window};
    use bloomery_core::scheduler::Resident;
    use bloomery_daemon::agents::{Agent, AgentState, AgentTable};

    let mut table = AgentTable::new();
    let window = Window {
        tokens: 4096,
        bound_by: BoundBy::TrainingCtx,
        vram_unmeasured: true,
    };
    let agent = Agent {
        id: "a1".to_string(),
        model: "qwen".to_string(),
        priority: 100,
        window,
        budget: Budget::new(200_000),
        kv_bytes: 123_456,
        state: AgentState::Resident { ctx: 7 },
    };
    table.insert(agent);

    assert!(table.get("a1").is_some());
    assert_eq!(table.get("a1").unwrap().kv_bytes, 123_456);

    table.get_mut("a1").unwrap().priority = 50;
    assert_eq!(table.get("a1").unwrap().priority, 50);

    let residents: Vec<Resident> = table.residents();
    assert_eq!(residents.len(), 1);
    assert_eq!(residents[0].id, "a1");
    assert_eq!(residents[0].kv_bytes, 123_456);

    let removed = table.remove("a1");
    assert!(removed.is_some());
    assert!(table.get("a1").is_none());
    assert!(table.residents().is_empty());
}
