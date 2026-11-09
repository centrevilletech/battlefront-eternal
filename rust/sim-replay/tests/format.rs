use sim_replay::{Arena, PHASES, Phase, Value, check_value, diff};

#[test]
fn diff_names_the_first_difference() {
    let mut arena = Arena::default();
    let list = arena.array(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)]);
    let wrong = arena.array(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(-0.0)]);
    assert!(diff(arena.view(&list), arena.view(&list)).is_none());
    let d = diff(arena.view(&list), arena.view(&wrong)).expect("difference");
    assert_eq!(d.path, "[2]");
    check_value("list", arena.view(&list), arena.view(&list)).unwrap();
    let e = check_value("list", arena.view(&list), arena.view(&wrong)).unwrap_err();
    assert!(e.0.contains("[2]") && e.0.contains("-0"), "{e}");
}

#[test]
fn phase_names_roundtrip() {
    let names: Vec<&str> = PHASES.iter().map(|(_, n)| *n).collect();
    assert!(!names.is_empty());
    for (phase, name) in PHASES {
        assert_eq!(Phase::from_name(name), Some(phase));
        assert_eq!(phase.name(), name);
    }
    assert_eq!(Phase::from_name("unitTick"), Some(Phase::UnitTick));
    assert_eq!(Phase::Ordnance.name(), "ordnance");
}
