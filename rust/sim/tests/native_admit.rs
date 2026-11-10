//! The native admission API's shape and refusals: signatures pinned by
//! function-pointer coercions, options composing the same query as the
//! page, the recorded admission line parsed into files and options, and
//! invalid or missing content refused with Host/Content errors.
#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::files::{Files, MemFiles};
use bfe_sim::abi::context::{Context, Error, ErrorKind};
use bfe_sim::world::admit::{admit, rows, AdmitError, Admitted, Call, Calls, Cx, Options, Returned, Rows, Stage};
use bfe_sim::world::owner::World;
use serde_json::{json, Value};

fn options() -> Options {
    Options { map: "nab2".into(), side: "rep".into(), enhanced: false, seed: 1, heroes: false, unit: None, name: None }
}

#[test]
fn native_admission_signatures_are_frozen() {
    let _: fn(&Context, &dyn Files, &Options) -> Result<Admitted, Error> = Context::world_admit;
    let _: fn(&Context, &Value) -> Result<bfe_sim::gen::WorldTickSetup, Error> = Context::world_replay_admit;
    let _: fn(&mut World, &dyn Files, &Options) -> Result<Admitted, AdmitError> = admit;
    let _: fn(Stage, &mut Cx) -> Result<Calls, AdmitError> = rows::build;
    let _: fn(&mut Cx) -> Result<Rows, AdmitError> = Rows::build;
    let _: fn() -> bool = bfe_sim::world::admit::provided;
    let _: fn(&Context, &str, &Call) -> Result<Returned, Error> = Context::invoke_export;
}

#[test]
fn options_compose_the_battle_query_as_the_page_does() {
    assert_eq!(options().query(), "?map=nab2&side=rep&era=cw&ai=classic&seed=1");
    let geonosis = Options { map: "geo1".into(), side: "cis".into(), enhanced: true, seed: 2, heroes: true, unit: Some("cis_inf_droideka".into()), name: None };
    assert_eq!(geonosis.query(), "?map=geo1&side=cis&era=cw&ai=enhanced&seed=2&heroes=1&unit=cis_inf_droideka");
    assert_eq!(Options { side: "imp".into(), ..options() }.era(), "gcw");
}

#[test]
fn entries_report_provided_and_refuse_missing_content() {
    assert!(bfe_sim::world::admit::provided());
    let context = Context::new();
    let files = MemFiles::default();
    let error = context.world_admit(&files, &options()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Host);
    let error = context.world_replay_admit(&Value::Null).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Host);
    let mut world = World::new();
    let mut files = MemFiles::default();
    files.insert("/maps/nab2/map.json", b"{}".to_vec());
    match admit(&mut world, &files, &options()) {
        Err(AdmitError::Content { stage: Stage::Content, .. }) => {}
        other => panic!("admission answered {other:?} with empty map.json"),
    }
    assert_eq!(AdmitError::Unavailable(Stage::Startup).to_string(), "native admission has no body for Startup");
    // Disposal still wins over everything.
    context.dispose().unwrap();
    assert_eq!(context.world_admit(&MemFiles::default(), &options()).unwrap_err().kind(), ErrorKind::Disposed);
    let _ = json!({});
}

fn call(args: &[f64], scratch: Vec<u8>) -> Call {
    Call { args: args.to_vec(), scratch, ..Default::default() }
}

#[test]
fn invoke_export_runs_the_module_bodies_with_the_page_arguments() {
    let context = Context::new();
    // The collision kernel as ray-kernel.js makes a one-triangle tree: the
    // corners' copy, the tree, its index filled from the call's bytes, built.
    assert_eq!(context.invoke_export("positions_new", &call(&[9.0, 0.0], vec![])).unwrap(), Returned::Scalar(0.0));
    let corners: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(context.invoke_export("positions_at", &call(&[0.0], corners)).unwrap(), Returned::None);
    assert_eq!(context.invoke_export("tree_new", &call(&[0.0, 3.0, 0.0], vec![])).unwrap(), Returned::Scalar(0.0));
    let index: Vec<u8> = [0u32, 1, 2].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(context.invoke_export("tree_index", &call(&[0.0], index)).unwrap(), Returned::None);
    assert_eq!(context.invoke_export("tree_build", &call(&[0.0], vec![])).unwrap(), Returned::None);
    assert_eq!(context.invoke_export("tree_roots", &call(&[0.0], vec![])).unwrap(), Returned::Scalar(1.0));
    assert_eq!(context.invoke_export("world_new", &call(&[0.0, -1.0, -1.0], vec![])).unwrap(), Returned::Scalar(0.0));
    // A buffer fill with more bytes than the buffer holds is refused, not truncated.
    let error = context.invoke_export("tree_index", &call(&[0.0], vec![0; 64])).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Host);
    // An export this table has no body for names itself.
    let error = context.invoke_export("no_such_export", &call(&[], vec![])).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Unavailable);
    assert_eq!(error.message(), "invoke_export no_such_export");
    // A publication without a session refuses, as the export answers null.
    assert_eq!(context.invoke_export("wire_publish", &call(&[1.0], vec![])).unwrap_err().kind(), ErrorKind::Host);
    // Strings interned for a call land in the battle's table in order.
    let mut named = call(&[], vec![]);
    named.strings.push("rep_inf_trooper".encode_utf16().collect());
    assert_eq!(context.invoke_export("sim_call_room", &named).unwrap(), Returned::None);
    assert_eq!(context.string_count().unwrap(), 1);
    context.dispose().unwrap();
    assert_eq!(context.invoke_export("positions_new", &call(&[1.0, 0.0], vec![])).unwrap_err().kind(), ErrorKind::Disposed);
}

#[test]
fn named_banks_read_and_write_owned_words_without_pointers() {
    let context = Context::new();
    // A kernel bank reads as owned words and takes a write of the same size.
    let io = context.invoke_export("bank:COLLISION_IO", &call(&[], vec![])).unwrap();
    let Returned::Row(words) = io else { panic!("io bank as words") };
    assert_eq!(words.len(), 64);
    let mut written = vec![0u8; 64 * 8];
    written[..8].copy_from_slice(&2.5f64.to_le_bytes());
    assert_eq!(context.invoke_export("bank:COLLISION_IO", &call(&[], written)).unwrap(), Returned::None);
    assert_eq!(context.invoke_export("bank:COLLISION_IO", &call(&[], vec![])).unwrap(), Returned::Row({ let mut w = vec![0.0; 64]; w[0] = 2.5; w }));
    // Byte banks come back as bytes; a write past the bank is refused.
    assert!(matches!(context.invoke_export("bank:COLLISION_ENABLED", &call(&[], vec![])).unwrap(), Returned::Bytes(b) if b.len() == 1 << 16));
    assert_eq!(context.invoke_export("bank:COLLISION_IO", &call(&[], vec![0; 65 * 8])).unwrap_err().kind(), ErrorKind::Host);
    // An owner's bank before its admission refuses; an unknown name is Unavailable.
    assert_eq!(context.invoke_export("bank:F3_INPUT", &call(&[], vec![])).unwrap_err().kind(), ErrorKind::Host);
    assert_eq!(context.invoke_export("bank:NO_SUCH_BANK", &call(&[], vec![])).unwrap_err().kind(), ErrorKind::Unavailable);
    // Every listed name resolves to a reader (a read-only bank refuses a write).
    for (name, direction, _) in bfe_sim::world::admit::banks::NAMES {
        let read = context.invoke_export(&format!("bank:{name}"), &call(&[], vec![]));
        assert_ne!(read.as_ref().err().map(|e| e.kind()), Some(ErrorKind::Unavailable), "{name} has no reader");
        if *direction == "r" { assert_eq!(context.invoke_export(&format!("bank:{name}"), &call(&[], vec![0; 8])).unwrap_err().kind(), ErrorKind::Host, "{name} took a write"); }
    }
}
