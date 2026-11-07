//! Unsupported callbacks must fail before acquiring an ambient exchange or
//! invoking a borrowed-result reader. Context handlers have separate entries.
#![cfg(not(target_arch = "wasm32"))]
use bfe_sim::gen;

fn message(call: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let panic = std::panic::catch_unwind(call).expect_err("native import must reject an absent host");
    if let Some(text) = panic.downcast_ref::<String>() { text.clone() }
    else { panic.downcast_ref::<&str>().expect("stable string panic").to_string() }
}

#[test]
fn native_import_rejection_names_the_requested_service() {
    assert_eq!(message(|| { gen::import_ask(&gen::Echo::default()); }), "native host import unavailable: ask");
    assert_eq!(message(|| { gen::import_f2_foreign(&gen::VehNumber::default()); }), "native host import unavailable: f2Foreign");
}

#[test]
fn borrowed_native_import_rejects_before_entering_the_reader() {
    let visited = std::cell::Cell::new(false);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        gen::import_veh_aim_points_read(&gen::VehMembers::default(), |_, _| visited.set(true));
    }));
    assert!(failure.is_err());
    assert!(!visited.get());
}
