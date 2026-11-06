use bfe_sim::combat::owners;
use bfe_sim::gen::{F3UnitKey, Store, Unit};
use bfe_sim::store::{Ref, StrId};
use bfe_sim::units::ownership::Stale;

#[test]
fn capture_retains_launch_metadata_when_the_source_rebinds() {
    let mut store = Store::default();
    let source = store.unit.insert(Unit {
        generation:7.0, team:1.0, human:true, class_name:StrId(12),
        character:Some(Ref::new(4,2)), alive:true, ..Default::default()
    });
    let key = F3UnitKey::of(source,store.unit.get(source).unwrap());
    let launch = owners::capture(&mut store,&key).unwrap();
    let notice = owners::created(&store,launch);
    assert_eq!(notice.source_unit,source);
    assert_eq!(notice.character,Some(Ref::new(4,2)));
    assert_eq!(notice.life,7.0);
    assert_eq!(owners::resolve(&store,launch),Some(source));
    assert!(owners::alive(&store,launch));
    assert!(!owners::removed(&store,launch));

    // A same-life corpse remains the excluded pointer. Liveness is separate.
    store.unit.get_mut(source).unwrap().alive = false;
    assert_eq!(owners::resolve(&store,launch),Some(source));
    assert!(!owners::alive(&store,launch));
    let unit = store.unit.get_mut(source).unwrap();
    unit.generation = 8.0; unit.team = 2.0; unit.character = Some(Ref::new(5,1));
    unit.alive = true;
    assert_eq!(owners::resolve(&store,launch),None);
    assert!(!owners::alive(&store,launch));
    assert!(owners::removed(&store,launch));
    let old = store.owner_snapshot.get(launch).unwrap();
    assert_eq!((old.generation,old.team,old.human,old.class_name),(7.0,1.0,true,StrId(12)));
    assert_eq!(old.character,Some(Ref::new(4,2)));
    assert_eq!(owners::capture(&mut store,&key),Err(Stale::Life));
}

#[test]
fn replacing_a_slot_never_resolves_a_captured_source() {
    let mut store = Store::default();
    let source = store.unit.insert(Unit { generation:2.0, alive:true, ..Default::default() });
    let key = F3UnitKey::of(source,store.unit.get(source).unwrap());
    let launch = owners::capture(&mut store,&key).unwrap();
    store.unit.remove(source).unwrap();
    let replacement = store.unit.insert(Unit { generation:2.0, alive:true, ..Default::default() });
    assert_eq!(source.slot,replacement.slot);
    assert_ne!(source.gen,replacement.gen);
    assert_eq!(owners::resolve(&store,launch),None);
    assert_eq!(owners::capture(&mut store,&key),Err(Stale::Slot));
}
