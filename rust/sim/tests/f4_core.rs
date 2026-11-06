//! Score commands validate their complete lives before preparing lazy tallies.
use bfe_sim::battle::{commands::Result, core::Core, rules};
use bfe_sim::gen::*;
use bfe_sim::store::Nullable;
fn setup() -> (Store, Core, F4Handle, F4Handle) {
    let mut s = Store::default(); s.battle.stats = rules::new_stats();
    let a = s.character.insert(Character { team:1.0, ..Character::default() });
    let b = s.character.insert(Character { team:2.0, ..Character::default() });
    s.battle.characters.extend([a,b]);
    let mut core = Core::new(&s,2);
    let a = core.adopt(&mut s,a); let b = core.adopt(&mut s,b); core.flush(&s);
    (s,core,a,b)
}
#[test]
fn stale_and_point_commands_preserve_lazy_records() {
    let (mut s,mut core,a,b) = setup();
    let mut c = F4Command { target:a.clone(),other:Some(b.clone()),op:F4CommandOp::Credit,value:-1.0,..F4Command::default() };
    c.other.as_mut().unwrap().life += 1.0;
    assert_eq!(core.apply(&mut s,&c),Result::Stale);
    for (_,c) in s.character.iter() { assert!(matches!(c.record,Nullable::Undefined)); }
    c.op = F4CommandOp::Point; c.other = None;
    assert_eq!(core.apply(&mut s,&c),Result::Applied);
    assert_eq!(s.character.get(Ref::new(a.slot,a.generation)).unwrap().captures,1.0);
    assert!(matches!(s.character.get(Ref::new(a.slot,a.generation)).unwrap().record,Nullable::Undefined));
    s.battle.game_over = true;
    c.op = F4CommandOp::Headshot; assert_eq!(core.apply(&mut s,&c),Result::Applied);
    assert!(matches!(s.character.get(Ref::new(a.slot,a.generation)).unwrap().record,Nullable::Undefined));
}
#[path = "support/allocations.rs"] mod allocation;
#[global_allocator] static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn first_native_credits_achievements_and_sparse_publications_reuse_prepared_storage() {
    let (mut s,mut core,a,b) = setup();
    let a = Ref::new(a.slot,a.generation); let b = Ref::new(b.slot,b.generation);
    allocation::start();
    for _ in 0..1200 {
        core.credit(&mut s,Some(b),Some(a),-1.0);
        core.achievement(&mut s,Some(a),F4CommandOp::Headshot);
        core.achievement(&mut s,Some(a),F4CommandOp::VehicleDestroyed);
        assert!(core.flush(&s) <= 3);
    }
    assert_eq!(allocation::stop(),0);
    assert_eq!(s.character.get(a).unwrap().kills,1200.0);
    assert_eq!(s.character.get(b).unwrap().deaths,1200.0);
}

#[test]
fn objective_and_controller_commands_reject_stale_posts_bodies_and_character_lives() {
    let (mut s,mut core,a,_)=setup();let character=Ref::new(a.slot,a.generation);
    let post=s.post.insert(Post::default());
    let target=F4Handle{kind:F4ObjectKind::Post,slot:post.slot,generation:post.gen,life:1.0};
    let mut objective=F4Command{target:a.clone(),other:Some(target),op:F4CommandOp::Objective,..Default::default()};
    assert_eq!(core.apply(&mut s,&objective),Result::Applied);
    assert_eq!(s.character.get(character).unwrap().objective,PostRef::Post(post));
    s.post.remove(post);let next=s.post.insert(Post::default());assert_eq!(post.slot,next.slot);
    assert_eq!(core.apply(&mut s,&objective),Result::Stale);
    assert_eq!(s.character.get(character).unwrap().objective,PostRef::Post(post));
    objective.other=None;assert_eq!(core.apply(&mut s,&objective),Result::Applied);
    assert_eq!(s.character.get(character).unwrap().objective,PostRef::Null);
    let unit=s.unit.insert(Unit{generation:2.0,character:Some(character),..Default::default()});
    let controller=Ref::new(99,4);
    let mut command=F4Command{target:a,op:F4CommandOp::Controller,controller:Some(controller),
        unit_key:Some(F3UnitKey::of(unit,s.unit.get(unit).unwrap())),..Default::default()};
    assert_eq!(core.apply(&mut s,&command),Result::Applied);
    assert_eq!(s.unit.get(unit).unwrap().controller,Some(controller));
    s.unit.get_mut(unit).unwrap().generation+=1.0;command.controller=None;
    assert_eq!(core.apply(&mut s,&command),Result::Stale);
    assert_eq!(s.unit.get(unit).unwrap().controller,Some(controller));
    command.unit_key=Some(F3UnitKey::of(unit,s.unit.get(unit).unwrap()));
    core.adopt(&mut s,character);
    assert_eq!(core.apply(&mut s,&command),Result::Stale);
    assert_eq!(s.unit.get(unit).unwrap().controller,Some(controller));
}
