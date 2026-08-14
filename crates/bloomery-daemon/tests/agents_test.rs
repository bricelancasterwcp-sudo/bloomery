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

/// Review finding (Important #1): `spill` used to remove the RAM entry
/// *before* attempting the disk write. If the write failed (ENOSPC, EIO,
/// permissions), the bytes were already gone from RAM and never made it to
/// disk — silently indistinguishable from an id that was never spilled.
/// This replaces the spill dir with a plain file after `ImageStore::new`
/// creates it as a real directory, so any write underneath it fails
/// structurally (`ENOTDIR`) regardless of process privileges — robust even
/// when tests run as root, where a chmod-based permission test would be
/// silently bypassed.
#[test]
fn spill_write_failure_preserves_ram_copy() {
    let dir = std::env::temp_dir().join("bloomery-img-test-spill-fail");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "digestX", vec![1, 2, 3]);

    // Swap the spill dir out for a regular file: any write under it now
    // fails at the filesystem level, not via a permission check.
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::write(&dir, b"not a directory").unwrap();

    let result = st.spill("a1");
    assert!(result.is_err(), "spill should report the write failure");

    match st.take("a1", "digestX") {
        ImageFetch::Ram(b) => assert_eq!(b, vec![1, 2, 3]),
        o => panic!("spill failure must not lose the only copy: {o:?}"),
    }

    // Leave a clean filesystem behind for other tests / future runs.
    let _ = std::fs::remove_file(&dir);
}

/// Review finding (Important #2): traced resurrection path.
/// `put_ram(id, D1) + spill` -> `put_ram(id, D2)` -> `take(id, D2)` consumes
/// the RAM copy -> a later `take(id, D1)` used to find the untouched D1
/// spill entry (digest and recorded length both still match) and return a
/// clean `Nvme(..)` — resurrecting an image the id's owner has already
/// moved past. `put_ram` must invalidate (drop the index entry for, and
/// best-effort delete the file of) any spilled image for that id, so the
/// old digest is simply gone, not silently servable again.
#[test]
fn put_ram_invalidates_previously_spilled_entry_for_same_id() {
    let dir = std::env::temp_dir().join("bloomery-img-test-resurrection");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();

    st.put_ram("a1", "D1", vec![1, 1, 1]);
    st.spill("a1").unwrap();

    st.put_ram("a1", "D2", vec![2, 2, 2]);
    match st.take("a1", "D2") {
        ImageFetch::Ram(b) => assert_eq!(b, vec![2, 2, 2]),
        o => panic!("{o:?}"),
    }

    // The D1 image no longer exists as far as the store is concerned —
    // never a clean Nvme hit for a digest the id has moved past.
    match st.take("a1", "D1") {
        ImageFetch::Missing => {}
        o => panic!("stale spilled image resurrected as {o:?}, expected Missing"),
    }
}

/// `drop_image` (Task 15's `Pager::remove_agent` cleanup): removes a RAM
/// image outright, and removes a spilled image's index entry *and* its
/// backing file, so neither tier can resurrect it for a later `take`.
#[test]
fn drop_image_removes_ram_and_spilled_entries_and_the_spill_file() {
    let dir = std::env::temp_dir().join("bloomery-img-test-drop");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();

    st.put_ram("a1", "digestX", vec![1, 2, 3]);
    st.drop_image("a1");
    assert!(matches!(st.take("a1", "digestX"), ImageFetch::Missing));

    st.put_ram("a2", "digestX", vec![4, 5, 6]);
    st.spill("a2").unwrap();
    let spilled_path = dir.join("a2.digestX.kvimg");
    assert!(spilled_path.exists());

    st.drop_image("a2");
    assert!(matches!(st.take("a2", "digestX"), ImageFetch::Missing));
    assert!(
        !spilled_path.exists(),
        "drop_image must delete the spill file, not just forget it"
    );

    // Dropping an id with no image at all is a no-op, not an error.
    st.drop_image("nobody");
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
