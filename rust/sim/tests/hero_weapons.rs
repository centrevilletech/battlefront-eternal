//! Hero weapon timing fixtures run without content or captured method stubs.
use bfe_sim::combat::defs::{Js, JsObj, WeaponClass, WeaponDefs};
use bfe_sim::combat::weapons::{Admission, Aim, Shot, ShotKind, Trigger, WeaponDraws};
use bfe_sim::gen::Weapon;
use bfe_sim::store::DefId;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

struct Allocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|on| if on.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|on| if on.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn object(fields: Vec<(&str, Js)>) -> Js {
    Js::Obj(Rc::new(JsObj {
        fields: fields.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        def: None,
    }))
}
fn blade(swings: Js) -> (Weapon, WeaponClass) {
    let Js::Obj(def) = object(vec![("base", Js::Str("melee".into())), ("swings", swings)]) else { unreachable!() };
    let class = WeaponClass::new(def);
    let mut defs = WeaponDefs::default();
    defs.set(DefId(0), class.clone());
    let mut weapon = Weapon::create(DefId(0), &defs, false);
    weapon.prepare_melee(&class);
    weapon.random = true;
    (weapon, class)
}
struct Draws { value: f64, count: usize }
impl WeaponDraws for Draws {
    fn draw(&mut self, _: u32) -> f64 { self.count += 1; self.value }
    fn legacy(&mut self, _: u32) -> f64 { panic!("the blade has a bound stream") }
    fn seeded(&mut self, _: f64, _: f64) { panic!("melee does not seed a salvo") }
    fn admit(&mut self, _: u32) -> Admission { panic!("melee has no admission") }
}
fn tick(weapon: &mut Weapon, class: &WeaponClass, draws: &mut Draws, shots: &mut Vec<Shot>, dt: f64, press: bool) {
    shots.clear();
    weapon.update(0, class, dt, true, &Trigger { held: true, pressed: press, ..Trigger::default() }, false,
        &Aim { stick: 0.0, posture: None, zoomed: false, repair_target: false }, draws, shots);
}

#[test]
fn swing_uses_only_the_first_three_authored_entries() {
    let swings = Js::Arr(Rc::new((1..=4).map(|i| object(vec![
        ("duration", Js::Num(i as f64)), ("hit", Js::Num(i as f64 / 2.0)),
    ])).collect()));
    let (mut weapon, class) = blade(swings);
    let mut draws = Draws { value: 0.999, count: 0 };
    let mut shots = Vec::with_capacity(3);
    tick(&mut weapon, &class, &mut draws, &mut shots, 10.0, true);
    assert_eq!(shots[0].kind, ShotKind::MeleeStart { swing: 3.0 });
    assert_eq!(weapon.melee.as_ref().unwrap().time, 0.0);
    // An active press neither redraws nor starts a second swing.
    tick(&mut weapon, &class, &mut draws, &mut shots, 1.5, true);
    assert!(shots.is_empty());
    assert_eq!(draws.count, 1);
    tick(&mut weapon, &class, &mut draws, &mut shots, 0.25, false);
    assert_eq!(shots[0].kind, ShotKind::MeleeHit);
}

#[test]
fn default_swing_keeps_strict_hit_and_completion_boundaries() {
    let (mut weapon, class) = blade(Js::Undefined);
    let mut draws = Draws { value: 0.0, count: 0 };
    let mut shots = Vec::with_capacity(3);
    tick(&mut weapon, &class, &mut draws, &mut shots, 5.0, true);
    assert_eq!(shots[0].kind, ShotKind::MeleeStart { swing: 1.0 });
    tick(&mut weapon, &class, &mut draws, &mut shots, 0.5, false);
    assert!(shots.is_empty());
    tick(&mut weapon, &class, &mut draws, &mut shots, 0.5, false);
    assert_eq!(shots[0].kind, ShotKind::MeleeHit);
    tick(&mut weapon, &class, &mut draws, &mut shots, 0.125, false);
    assert!(shots.is_empty());
    assert!(weapon.firing);
    tick(&mut weapon, &class, &mut draws, &mut shots, 0.0, false);
    assert_eq!(shots[0].kind, ShotKind::MeleeEnd);
    assert!(!weapon.firing);
    assert_eq!(draws.count, 1);
}

#[test]
fn repeated_hero_weapon_updates_allocate_nothing() {
    let (mut weapon, class) = blade(Js::Undefined);
    let mut draws = Draws { value: 0.5, count: 0 };
    let mut shots = Vec::with_capacity(3);
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|on| on.set(true));
    for tick_number in 0..1200 {
        tick(&mut weapon, &class, &mut draws, &mut shots, 1.0 / 60.0, tick_number % 70 == 0);
    }
    TRACK.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert!(draws.count > 10);
}

#[test]
fn pooled_combat_reset_reuses_buffers_and_recreates_the_melee_state() {
    use bfe_sim::combat::{combat::{self,CombatDefs},defs::DefTable,weapons::arm};
    use bfe_sim::gen::{Attacker,Combat,Seen,Unit};
    use bfe_sim::store::{Ref,StrId};
    let mut defs = CombatDefs::default();
    let Js::Obj(rifle) = object(vec![
        ("base",Js::Str("cannon".into())),("RoundsPerClip",Js::Num(4.0)),
        ("ammo",Js::Num(2.0)),("SpreadThreshold",Js::Num(0.25)),
    ]) else { unreachable!() };
    let Js::Obj(saber) = object(vec![("base",Js::Str("melee".into()))]) else { unreachable!() };
    defs.weapons.set(DefId(0),WeaponClass::new(rifle));
    defs.weapons.set(DefId(1),WeaponClass::new(saber));
    let loadout = Js::Arr(Rc::new((0..2).map(|id| Js::Obj(Rc::new(JsObj {
        fields:Vec::new(),def:Some((DefTable::Weapon,DefId(id))),
    }))).collect()));
    let Js::Obj(class) = object(vec![("weapons",loadout)]) else { unreachable!() };
    defs.set_class(DefId(0),class);
    let unit = Unit {class_def:DefId(0),..Default::default()};
    let attacker = Attacker::Unit(Ref::new(99,2));
    let mut combat = Combat {
        max_health:200.0,player:true,repair_lock:StrId(7),last_attacker:attacker,
        weapons:arm(defs.loadout(DefId(0)),&defs.weapons,true),
        seen:vec![Seen::default();2],..Default::default()
    };
    let weapons = combat.weapons.as_ptr();
    let seen = combat.seen.as_ptr();
    let mut fresh = true;
    ALLOCATIONS.with(|n| n.set(0));TRACK.with(|on| on.set(true));
    for _ in 0..1200 {
        combat.health = 0.0;combat.dead = true;combat.blow_apart = true;
        combat.selected = [1.0,1.0];combat.seen[0].known = true;
        combat.weapons[0].clip = 0.0;combat.weapons[0].heat = 5.0;
        combat.weapons[1].melee.as_mut().unwrap().active = true;
        combat::reset(&mut combat,&unit,&defs);
        let melee = combat.weapons[1].melee.as_ref().unwrap();
        fresh &= combat.health == 200.0 && !combat.dead && !combat.blow_apart
            && combat.selected == [0.0,0.0] && !combat.seen[0].known
            && combat.weapons[0].clip == 1.0 && combat.weapons[0].reserve == 1.0
            && combat.weapons[0].heat == 0.0 && combat.weapons[0].floor == -0.25
            && !melee.active && melee.time == -1.0 && melee.index == 0.0;
    }
    TRACK.with(|on| on.set(false));
    assert!(fresh);
    assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(combat.weapons.as_ptr(),weapons);assert_eq!(combat.seen.as_ptr(),seen);
    // The original reset deliberately keeps a repair lock and last attacker.
    assert_eq!(combat.repair_lock,StrId(7));assert_eq!(combat.last_attacker,attacker);
}
