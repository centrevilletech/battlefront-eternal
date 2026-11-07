//! Exercise the generated admission boundary, including damaged list headers.
use bfe_sim::{battle::startup::class_allowed_record, gen::{F4RosterClasses, F4StartupClass}, store::StrId};

#[test]
fn off_roster_class_and_invalid_teams_are_rejected() {
    let mut input = F4StartupClass { class_name: StrId(1), team: 1.0,
        rosters: vec![F4RosterClasses { classes: vec![] },
        F4RosterClasses { classes: vec![StrId(1)] }, F4RosterClasses { classes: vec![StrId(2)] }] };
    let mut row = [0.0; F4StartupClass::WORDS];
    let mut heap = Vec::new();
    input.put(&mut row, &mut heap);
    assert!(class_allowed_record(&row, &heap));
    input.class_name = StrId(2); input.team = 2.0;
    heap.clear(); input.put(&mut row, &mut heap);
    assert!(class_allowed_record(&row, &heap));
    row[F4StartupClass::CLASS_NAME] = 3.0;
    assert!(!class_allowed_record(&row, &heap));
    row[F4StartupClass::CLASS_NAME] = 1.0;
    for team in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1.5, 3.0, 4294967296.0] {
        row[F4StartupClass::TEAM] = team;
        assert!(!class_allowed_record(&row, &heap));
    }
    row[F4StartupClass::TEAM] = 1.0;
    assert!(!class_allowed_record(&[], &heap));
    assert!(!class_allowed_record(&row, &[]));
    for bad in [f64::NAN, f64::INFINITY, -1.0, 0.5, 1e18] {
        let mut damaged = row;
        damaged[F4StartupClass::ROSTERS + 1] = bad;
        assert!(!class_allowed_record(&damaged, &heap));
        damaged = row; damaged[F4StartupClass::ROSTERS] = bad;
        assert!(!class_allowed_record(&damaged, &heap));
        let mut damaged_heap = heap.clone();
        damaged_heap[F4RosterClasses::WORDS + F4RosterClasses::CLASSES + 1] = bad;
        assert!(!class_allowed_record(&row, &damaged_heap));
    }
}
