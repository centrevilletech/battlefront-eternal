use bfe_sim::wire::records;

#[test]
fn slot_reuse_and_exhaustion_preserve_allocation_order() {
    let mut slots = records::SnapshotSlotTable::default();
    assert_eq!(slots.assign_unit(0.0), Ok(0.0));
    assert_eq!(slots.assign_unit(-0.0), Ok(0.0));
    assert_eq!(slots.assign("item:0".into(), None), Ok(1.0));
    for i in 1..511 {
        assert_eq!(slots.assign_unit(i as f64), Ok((i + 1) as f64));
    }
    assert_eq!(slots.assign_unit(900.0), Err("snapshot slot table full"));
    assert_eq!(slots.slot_for_unit(900.0), -1.0);
    slots.release_unit(10.0);
    slots.release_unit(20.0);
    slots.release_unit(20.0);
    assert_eq!(slots.assign_unit(900.0), Ok(21.0));
    assert_eq!(slots.assign_unit(901.0), Ok(11.0));
    assert_eq!(slots.assign_unit(902.0), Err("snapshot slot table full"));
    assert_eq!(slots.slot_for("item:0"), 1.0);
}
