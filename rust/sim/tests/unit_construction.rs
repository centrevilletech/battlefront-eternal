//! Synthetic cold births use the same generated rows and ownership bookkeeping
//! as the live coordinator. No game content or JavaScript twin is required.
use bfe_sim::{gen::{F3UnitKey, Member, Store, Unit}, store::{Ref, StrId},
    units::{construction::{Construction, Error, Prepared}, ownership::Ownership}};

fn prepared(class: u32, cookie: u32) -> Prepared {
    Prepared {cookie, local:None, source: Ref::new(100 + cookie, 0),
        unit: Unit {class_name: StrId(class), removed: true, ..Default::default()}}
}

#[test]
fn cold_births_join_mixed_order_at_activation_and_corpses_keep_identity() {
    let mut store=Store::default();let mut owner=Ownership::default();let mut factory=Construction::default();
    factory.reserve(&mut store,&mut owner,32);
    factory.prepare(&mut owner,prepared(1,7));
    assert!(store.units.list.is_empty());
    store.units.add(|id|Member::VehicleTarget(Ref::new(id,0)));
    let first=factory.acquire(&mut store,&mut owner,StrId(1)).unwrap();
    assert_eq!(first.key.slot,1);assert_eq!(first.prepared,Some((7,Ref::new(107,0))));
    assert_eq!(store.units.list,vec![Member::VehicleTarget(Ref::new(0,0)),Member::Unit(first.key.reference())]);
    owner.removed(&store,&first.key).unwrap();
    let again=factory.acquire(&mut store,&mut owner,StrId(1)).unwrap();
    assert_eq!(again.key,first.key);assert!(again.prepared.is_none());
    assert_eq!(store.units.list.len(),2);
}

#[test]
fn local_realization_preserves_identity_and_life_without_aliasing_a_connection() {
    let mut store=Store::default();let mut owner=Ownership::default();let mut factory=Construction::default();
    factory.reserve(&mut store,&mut owner,32);
    let local=store.unit.insert_at(0,3,Unit {generation:8.,class_name:StrId(1),team:2.,human:true,..Default::default()});
    store.units.add(|_|Member::Unit(local));owner.adopt_passive(&store,local).unwrap();
    factory.prepare(&mut owner,prepared(2,4));factory.prepare(&mut owner,prepared(3,5));
    let key=F3UnitKey::of(local,store.unit.get(local).unwrap());
    let result=factory.realize(&mut store,&mut owner,StrId(2),Some(key.clone()),true).unwrap();
    assert!(result.replacement);assert_eq!(result.key,key);
    let unit=store.unit.get(local).unwrap();assert_eq!(unit.class_name,StrId(2));assert_eq!(unit.generation,8.);
    assert_eq!(unit.team,2.);assert!(unit.human);assert!(owner.built().is_empty());
    let connection=factory.realize(&mut store,&mut owner,StrId(3),Some(key.clone()),false).unwrap();
    assert_ne!(connection.key.reference(),local);assert!(!connection.replacement);
    assert_eq!(store.unit.get(local).unwrap().class_name,StrId(2));
}

#[test]
fn stale_realization_and_unprepared_classes_do_not_consume_stock() {
    let mut store=Store::default();let mut owner=Ownership::default();let mut factory=Construction::default();
    factory.reserve(&mut store,&mut owner,8);factory.prepare(&mut owner,prepared(1,1));
    let stale=F3UnitKey {slot:0,generation:1,life:1.};
    assert!(matches!(factory.realize(&mut store,&mut owner,StrId(1),Some(stale),true),Err(Error::Stale(_))));
    assert_eq!(factory.available(StrId(1)),1);assert!(store.units.list.is_empty());
    assert!(matches!(factory.acquire(&mut store,&mut owner,StrId(9)),Err(Error::Unprepared)));
    assert_eq!(factory.available(StrId(1)),1);
}
