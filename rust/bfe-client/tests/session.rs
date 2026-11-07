//! The native transport must run its owned phase, not republish a captured
//! output bank. This synthetic actor needs no prepared game files or GL.
use bfe_client::{presentation_bridge::PageUnits, session::bytes_mut};
use bfe_presentation::layout::{self, unit_frame as f, unit_input as i, unit_output as o};
fn put(bank: &mut [u8], at: usize, value: f64) {
    bank[at * 8..at * 8 + 8].copy_from_slice(&value.to_le_bytes());
}
fn number(bank: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bank[at * 8..at * 8 + 8].try_into().unwrap())
}
#[test]
fn shared_template_admits_distinct_actors_and_publishes_native_poses() {
    let mut phase = PageUnits::new();
    let tables = serde_json::json!({"sets":vec![[Option::<usize>::None;9];3],"alert":[null,null,null,null,null,null,null,null,null],"transitions":vec![[Option::<usize>::None;3];3],"jump":null,"fall":null,"landSoft":null,"landHard":null,"dive":null,"jet":null,"thrown":vec![[Option::<usize>::None;4];2],"upper":[]});
    let data = serde_json::json!({"bones":[{"name":"DummyRoot","parent":-1,"pose":[0,0,0,0,0,0,1,1,1,1]}],"clips":[],"tables":tables,"walker":null});
    let template: bfe_presentation::unit_content::Input = serde_json::from_value(data).unwrap();
    let content = phase.load_prepared(bfe_content_native::units::copy_input(&template));
    let second_content = phase.load_prepared(bfe_content_native::units::copy_input(&template));
    assert_ne!(content, second_content);
    let id = phase
        .add_actor(content, "[[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]]", "[]")
        .unwrap();
    let second = phase
        .add_actor(second_content, "[[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]]", "[]")
        .unwrap();
    let input = phase.bank("input").unwrap();
    for (at, value) in [
        (f::VERSION, layout::VERSION as f64),
        (f::FRAME, 1.0),
        (f::DT, 1.0 / 60.0),
        (f::ACTORS, 2.0),
        (f::FIRST_PERSON, -1.0),
        (f::FIRST_PERSON_SHOWN, -1.0),
        (f::STRIDE + i::ACTOR, id as f64),
        (f::STRIDE + i::ID, 41.0),
        (f::STRIDE + i::GENERATION, 1.0),
        (f::STRIDE + i::PRESENT, 1.0),
        (f::STRIDE + i::OWN, 1.0),
        (f::STRIDE + i::ALPHA, 1.0),
        (f::STRIDE + i::POSITION, 3.0),
        (f::STRIDE + i::POSITION + 1, 4.0),
        (f::STRIDE + i::POSITION + 2, 5.0),
        (f::STRIDE + i::ROOT_QUATERNION + 3, 1.0),
    ] {
        put(input, at, value);
    }
    let next = f::STRIDE + i::STRIDE;
    for (at, value) in [
        (i::ACTOR, second as f64),
        (i::ID, 42.0),
        (i::GENERATION, 1.0),
        (i::PRESENT, 1.0),
        (i::OWN, 1.0),
        (i::ALPHA, 1.0),
        (i::POSITION, -7.0),
        (i::ROOT_QUATERNION + 3, 1.0),
    ] {
        put(input, next + at, value);
    }
    phase.present_units();
    let out = phase.read_bank("output").unwrap();
    assert_eq!(number(out, id * o::STRIDE + o::POSITION), 3.0);
    assert_eq!(number(out, id * o::STRIDE + o::POSITION + 1), 4.0);
    assert_eq!(number(out, id * o::STRIDE + o::POSITION + 2), 5.0);
    assert_eq!(number(out, second * o::STRIDE + o::POSITION), -7.0);
    // The transport preserves authored signed zero; JSON is never a frame bank.
    let mut zero = [-0.0f64];
    assert_eq!(bytes_mut(&mut zero), (-0.0f64).to_le_bytes());
}

#[test]
fn shared_network_banks_keep_scoped_loans_and_publish_native_samples() {
    use bfe_client::session::PresentationOwner;
    use bfe_pose::timeline_layout::{
        self as l, timeline_frame as f, timeline_output as o, timeline_record as r,
    };
    let mut owner = PresentationOwner::new("pagenetwork", "{}", false).unwrap();
    if let PresentationOwner::Network(n) = &mut owner {
        n.timeline_setup();
    }
    {
        let mut bank = owner.bank("timeline_input").unwrap();
        for (at, value) in [
            (f::VERSION, l::VERSION as f64),
            (f::EPOCH, 1.),
            (f::TICK, 10.),
            (f::COUNT, 1.),
            (f::HZ, 60.),
            (f::EXTRAPOLATE, 3.),
            (f::SNAPSHOTS, 8.),
            (f::STRIDE + r::VALID, 1.),
            (f::STRIDE + r::ID, 7.),
            (f::STRIDE + r::GENERATION, 1.),
            (f::STRIDE + r::POSITION, 12.),
            (f::STRIDE + r::QUATERNION + 3, 1.),
        ] {
            put(&mut bank, at, value);
        }
    }
    if let PresentationOwner::Network(n) = &mut owner {
        n.timeline_push();
        n.present(10., -1., -1.);
    }
    let first = owner.read_bank("timeline_output").unwrap();
    let second = owner.read_bank("timeline_output").unwrap();
    assert_eq!(number(&first, o::POSITION), 12.);
    assert_eq!(first.as_ref(), second.as_ref());
    drop(first);
    drop(second);
    // Releasing the read loans permits a following native update immediately.
    if let PresentationOwner::Network(n) = &mut owner {
        n.present(11., -1., -1.);
    }
    assert_eq!(
        number(&owner.read_bank("timeline_output").unwrap(), o::ID),
        7.
    );
}
