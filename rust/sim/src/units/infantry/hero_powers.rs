//! Forward each soldier life's offhand trigger and lock button to the 2005
//! hero powers once per fixed tick, after every soldier moved and before the
//! contact journal closes the tick.
use super::contact;
use crate::combat::combo::{ComboState, DeflectDef};
use crate::combat::powers::{self, world::{Forward, PowerHit, PowerHost}, Footing, LifeKey, PowerInput};
use crate::gen::{Member, UnitBody};
use crate::movement::soldier::{swbf2::{Tumble, TumblePhase}, V3};
use crate::store::{DefId, Ref};
use crate::world::owner::World;

/// The soldier phase's answers to the powers: footing, guard, stamina and the
/// contact journal around each power's damage.
struct Host;

fn life<'a>(w: &'a World, key: LifeKey) -> Option<&'a super::Life> {
    let l = w.units.infantry.as_ref()?.lives.get(key.slot as usize)?.as_ref()?;
    (l.slab == key.generation && l.generation == key.life).then_some(l)
}

fn enter_tumble(w: &mut World, key: LifeKey, ground_only: bool) {
    let body = key.unit_mut(&mut w.sim.store).and_then(|u| match &mut u.soldier {
        UnitBody::Soldier(s) => {
            s.lock = 0.0;
            Some((V3::from(s.velocity), s.aim_yaw))
        }
        UnitBody::Droideka(_) => None,
    });
    let Some(i) = w.units.infantry.as_mut() else { return };
    let tick = i.tick;
    let Some(l) = i.lives.get_mut(key.slot as usize).and_then(Option::as_mut) else { return };
    if l.slab != key.generation || l.generation != key.life {
        return;
    }
    l.movement.roll = None;
    l.movement.sprint_time = None;
    let (velocity, aim_yaw) = body.unwrap_or((V3::ZERO, 0.0));
    l.movement.tumble = Some(if ground_only {
        Tumble::enter_ground(velocity, aim_yaw)
    } else {
        let seed = tick.wrapping_mul(0x9e37_79b9) ^ key.slot;
        Tumble::enter(velocity, aim_yaw, seed)
    });
    l.guard = None;
    l.blast_pending = false;
    l.allow_fire = false;
    l.offhand_gun_salvo = false;
    l.offhand_gun_anim = None;
    if let Some(hero) = i.heroes.get_mut(key.slot as usize).and_then(Option::as_mut) {
        if let Some(weapon) = i.weapons.get(hero.class).and_then(Option::as_ref) {
            hero.state = ComboState::new(&weapon.machine);
        }
        hero.guard = None;
        hero.frame = None;
        hero.reaction = None;
    }
}

impl PowerHost for Host {
    fn footing(&mut self, w: &World, key: LifeKey) -> Option<Footing> {
        let l = life(w, key)?;
        if l.movement.tumble.is_some_and(|t| {
            matches!(t.phase, TumblePhase::Thrown | TumblePhase::Tumble | TumblePhase::Landed)
        }) {
            return Some(Footing::Thrown);
        }
        l.movement.roll.is_some().then_some(Footing::Rolling)
    }
    fn guard(&mut self, w: &World, key: LifeKey) -> Option<DeflectDef> {
        life(w, key)?.guard
    }
    fn pay_guard(&mut self, w: &mut World, key: LifeKey, cost: f64) -> bool {
        if cost <= 0.0 {
            return true;
        }
        let Some(i) = w.units.infantry.as_mut() else { return false };
        let tuning = i.lives.get(key.slot as usize).copied().flatten().map(|l| i.classes[l.class].tuning);
        match (i.lives.get_mut(key.slot as usize).and_then(Option::as_mut), tuning) {
            (Some(l), Some(t)) if l.slab == key.generation && l.generation == key.life => {
                l.movement.energy >= cost && l.movement.charge(cost, &t)
            }
            _ => false,
        }
    }
    fn thrown(&mut self, w: &mut World, key: LifeKey) {
        enter_tumble(w, key, false);
    }
    fn tumbled(&mut self, w: &mut World, key: LifeKey) {
        enter_tumble(w, key, true);
    }
    fn damage(&mut self, w: &mut World, hit: PowerHit, apply: &mut dyn FnMut(&mut World)) {
        // Kills, credit and the feed name the power that dealt the damage.
        let weapon = weapon_def(w, hit.caster, hit.weapon).unwrap_or(DefId(hit.weapon));
        let observed = contact::Impact::power(w, hit.target, hit.caster, weapon, hit.from);
        let previous = observed.and_then(|h| h.enter(w));
        apply(w);
        if let Some(h) = observed {
            h.finish(w, previous);
        }
    }
}

fn weapon_def(w: &World, caster: Ref<crate::gen::Unit>, hash: u32) -> Option<DefId> {
    let u = w.sim.store.unit.get(caster)?;
    let defs = &crate::combat::module::defs_ref(&w.combat).weapons;
    u.combat.weapons.iter().map(|g| g.def).find(|d| {
        defs.has(*d) && {
            let crate::combat::defs::Js::Str(name) = defs.class(*d).base().obj.get("class") else { return false };
            crate::combat::combo::zero_fnv1a_lower(name) == hash
        }
    })
}

/// Every soldier life whose class has powers, once per tick.
pub(crate) fn pass(w: &mut World) {
    if w.units.infantry.is_none() || powers::powers_in(w).is_none() {
        return;
    }
    let members = w.sim.store.units.list.len();
    for index in 0..members {
        let Member::Unit(r) = w.sim.store.units.list[index] else { continue };
        let Some(u) = w.sim.store.unit.get(r) else { continue };
        if !matches!(u.soldier, UnitBody::Soldier(_)) {
            continue;
        }
        let (human, key) = (u.human, LifeKey::of(r, u));
        let alive_unseated = u.alive
            && !u.dead
            && !u.removed
            && u.seat.is_none()
            && powers::held_in(w, key).is_none();
        let active_offhand = alive_unseated.then(|| {
            let class_idx = w
                .units
                .infantry
                .as_ref()?
                .lives
                .get(r.slot as usize)?
                .as_ref()
                .filter(|l| l.slab == key.generation && l.generation == key.life)?
                .class;
            let class = &w.units.infantry.as_ref()?.classes[class_idx];
            class.offhand_guns.iter().find_map(|&(idx, hash, fa)| {
                u.combat
                    .weapons
                    .get(idx)
                    .is_some_and(|wp| wp.salvos_left > 0.0)
                    .then_some((hash, fa))
            })
        }).flatten();
        let Some(i) = w.units.infantry.as_mut() else { return };
        let tick = i.tick as u32;
        let Some(Some(l)) = i.lives.get_mut(r.slot as usize) else { continue };
        if l.slab != key.generation || l.generation != key.life {
            continue;
        }
        if !alive_unseated {
            l.offhand_gun_salvo = false;
            l.offhand_gun_anim = None;
        } else {
            if let Some((hash, fa)) = active_offhand {
                if !l.offhand_gun_salvo {
                    l.offhand_gun_anim = Some((hash, fa, tick));
                }
                l.offhand_gun_salvo = true;
            } else {
                l.offhand_gun_salvo = false;
                if l.offhand_gun_anim
                    .is_some_and(|(_, _, start)| tick.saturating_sub(start) > 600)
                {
                    l.offhand_gun_anim = None;
                }
            }
        }
        let Some(kit) = i.power_kits.get(l.class).copied().flatten() else { continue };
        let tuning = i.classes[l.class].tuning;
        let mut state = l.movement;
        let mut input = Forward { power: l.power_input, lock_pressed: std::mem::take(&mut l.lock_pressed), bot: !human };
        l.power_input.pressed = false;
        let before = l.energy_before;
        if std::mem::take(&mut l.blast_pending) {
            powers::play_explosion_in(w, &mut Host, r, kit);
        }
        // An overheated selected power cannot start; the trigger it shares
        // with the other powers stays down for them.
        if input.power.selected.is_some_and(|power| power_overheated(w, r, kit, power)) {
            input.power.selected = None;
        }
        let step = powers::forward_in(w, &mut Host, r, kit, input, &mut state, &tuning);
        for request in step.requests() {
            if let powers::Request::Fire(power) = request {
                charge_power_heat(w, r, kit, power);
            }
        }
        // A held area weapon's rate replaces this tick's ordinary recovery.
        if let Some(rate) = step.energy_rate {
            if before <= state.energy {
                state.energy = before;
            }
            state.charge(-rate / 60.0, &tuning);
        }
        if let Some(l) = w.units.infantry.as_mut().and_then(|i| i.lives.get_mut(r.slot as usize)).and_then(Option::as_mut) {
            if l.slab == key.generation && l.generation == key.life {
                l.movement = state;
                l.power_thrust = step.thrust;
            }
        }
    }
    powers::finish_tick_in(w, 1.0 / 60.0);
}

pub(crate) fn input(pressed: bool, held: bool, selected: Option<u8>) -> PowerInput {
    PowerInput { selected, pressed, held }
}

/// Arsenal index and kit index of each power the class carries.
pub(crate) fn kit_slots(w: &World, class: usize) -> [Option<(usize, u8)>; powers::MAX_POWERS] {
    let mut out = [None; powers::MAX_POWERS];
    let Some(kit) = w.units.infantry.as_ref().and_then(|i| i.power_kits.get(class).copied().flatten()) else {
        return out;
    };
    let Some(kit) = powers::powers_in(w).and_then(|p| p.kit(kit)) else { return out };
    for (n, (index, _)) in kit.iter().enumerate() {
        if let Some(slot) = kit.slot(index) {
            out[n] = Some((slot.saturating_sub(1) as usize, index));
        }
    }
    out
}

/// An explosion record from the prepared classes, its properties flattened
/// through its parents (the last value of a key wins).
pub(crate) fn explosion_record(tables: &[bfe_content::Value], name: &str) -> Option<bfe_content::Value> {
    let find = |n: &str| tables.iter().find_map(|t| t.get("expc")?.get(n).cloned());
    let mut chain = vec![];
    let mut next = Some(name.to_owned());
    while let Some(n) = next.take() {
        if chain.len() > 16 {
            break;
        }
        let Some(record) = find(&n) else { break };
        next = record.get("base").and_then(bfe_content::Value::as_str).map(str::to_owned);
        chain.push(record);
    }
    if chain.is_empty() {
        return None;
    }
    let mut object = bfe_content::Object::new();
    for record in chain.iter().rev() {
        for pair in record.get("properties").and_then(bfe_content::Value::as_array).unwrap_or(&[]) {
            if let (Some(k), Some(v)) = (pair.at(0).and_then(bfe_content::Value::as_str), pair.at(1)) {
                object.set(std::rc::Rc::from(k), v.clone());
            }
        }
    }
    Some(bfe_content::Value::Object(std::rc::Rc::new(object)))
}

fn power_slot_index(w: &World, kit: u16, power: u8) -> Option<usize> {
    let slot = powers::powers_in(w)?.kit(kit)?.slot(power)?;
    Some(slot.saturating_sub(1) as usize)
}

fn power_overheated(w: &World, caster: Ref<crate::gen::Unit>, kit: u16, power: u8) -> bool {
    let Some(index) = power_slot_index(w, kit, power) else { return false };
    w.sim.store.unit.get(caster).and_then(|u| u.combat.weapons.get(index)).is_some_and(|w| w.overheated)
}

fn apply_shot_heat(weapon: &mut crate::gen::Weapon, heat_per_shot: f64) {
    if !(heat_per_shot > 0.0) {
        return;
    }
    weapon.heat += heat_per_shot;
    if weapon.heat >= 1.0 {
        weapon.overheated = true;
    }
}

fn charge_power_heat(w: &mut World, caster: Ref<crate::gen::Unit>, kit: u16, power: u8) {
    let Some(index) = power_slot_index(w, kit, power) else { return };
    let heat_per_shot = {
        let Some(u) = w.sim.store.unit.get(caster) else { return };
        let Some(weapon) = u.combat.weapons.get(index) else { return };
        let defs = &crate::combat::module::defs_ref(&w.combat).weapons;
        if !defs.has(weapon.def) {
            return;
        }
        weapon.level(defs.class(weapon.def)).get(crate::combat::defs::K::HeatPerShot).or(0.0)
    };
    if let Some(weapon) = w.sim.store.unit.get_mut(caster).and_then(|u| u.combat.weapons.get_mut(index)) {
        apply_shot_heat(weapon, heat_per_shot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offhand_power_heat_accumulates_and_latches_overheat() {
        let mut weapon = crate::gen::Weapon::default();
        for _ in 0..7 {
            apply_shot_heat(&mut weapon, 0.125);
            assert!(!weapon.overheated);
        }
        assert_eq!(weapon.heat, 0.875);
        apply_shot_heat(&mut weapon, 0.125);
        assert!(weapon.overheated);
        let mut cool = crate::gen::Weapon::default();
        apply_shot_heat(&mut cool, 0.0);
        assert_eq!((cool.heat, cool.overheated), (0.0, false));
    }

    #[test]
    fn live_hero_powers_pass_charges_weapon_heat_and_blocks_while_overheated() {
        use crate::combat::{
            defs::{Js, JsObj},
            module as cmb,
            weapons::{Admission, Aim, Trigger, WeaponDraws},
        };
        use crate::gen::{Unit, Weapon};
        use crate::movement::soldier::swbf2::{ControlSpeed, Tuning};
        use crate::store::Strings;
        use crate::units::{f3, infantry::{bot_input::BotInputs, contact::Contacts, Class, Infantry, Life}};
        use std::rc::Rc;

        fn push_js(words: &mut Vec<f64>, strings: &mut Strings, val: &Js) {
            match val {
                Js::Undefined => words.push(0.0),
                Js::Null => words.push(1.0),
                Js::Bool(false) => words.push(2.0),
                Js::Bool(true) => words.push(3.0),
                Js::Num(n) => {
                    words.push(4.0);
                    words.push(*n);
                }
                Js::Str(s) => {
                    words.push(5.0);
                    words.push(strings.intern_str(s).0 as f64);
                }
                Js::Arr(items) => {
                    words.push(6.0);
                    words.push(items.len() as f64);
                    for item in items.iter() {
                        push_js(words, strings, item);
                    }
                }
                Js::Obj(obj) => {
                    words.push(7.0);
                    words.push(obj.fields.len() as f64);
                    for (k, v) in &obj.fields {
                        words.push(strings.intern_str(k).0 as f64);
                        push_js(words, strings, v);
                    }
                }
            }
        }

        struct ZeroDraws;
        impl WeaponDraws for ZeroDraws {
            fn draw(&mut self, _weapon: u32) -> f64 { 0.5 }
            fn legacy(&mut self, _weapon: u32) -> f64 { 0.5 }
            fn seeded(&mut self, _seed: f64, _value: f64) {}
            fn admit(&mut self, _weapon: u32) -> Admission { Admission::Yes }
        }

        let mut w = World::new();
        w.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 4);

        // Register combat weapon DefId(0) with HeatPerShot = 0.4, HeatRecoverRate = 0.5, HeatThreshold = 0.25.
        let weapon_js = Js::Obj(Rc::new(JsObj {
            fields: vec![
                ("class".into(), Js::Str("inv_hot_cone".into())),
                ("base".into(), Js::Str("cannon".into())),
                ("channel".into(), Js::Num(1.0)),
                ("HeatPerShot".into(), Js::Num(0.4)),
                ("HeatRecoverRate".into(), Js::Num(0.5)),
                ("HeatThreshold".into(), Js::Num(0.25)),
                ("ShotDelay".into(), Js::Num(0.05)),
                ("InitialSalvoDelay".into(), Js::Num(0.05)),
            ],
            def: None,
        }));
        let mut words = Vec::new();
        push_js(&mut words, &mut w.sim.strings, &weapon_js);
        w.sim.calls.result(words.len(), |row, _| row.copy_from_slice(&words));
        cmb::cmb_weapon_in(&mut w, 0);

        let mut caster_unit = Unit { generation: 1.0, team: 1.0, alive: true, human: true, ..Default::default() };
        caster_unit.combat.health = 500.0;
        caster_unit.combat.max_health = 500.0;
        // Slot 2 in power kit maps to arsenal index 1 (slot - 1).
        caster_unit.combat.weapons = vec![
            Weapon { def: DefId(0), ..Default::default() },
            Weapon { def: DefId(0), ..Default::default() },
        ];
        if let UnitBody::Soldier(s) = &mut caster_unit.soldier {
            s.position = [0.0, 0.0, 0.0];
            s.normal = [0.0, 1.0, 0.0];
            s.grounded = true;
        }
        let mut enemy_unit = Unit { generation: 1.0, team: 2.0, alive: true, ..Default::default() };
        enemy_unit.combat.health = 500.0;
        enemy_unit.combat.max_health = 500.0;
        if let UnitBody::Soldier(s) = &mut enemy_unit.soldier {
            s.position = [0.0, 0.0, 4.0];
            s.normal = [0.0, 1.0, 0.0];
            s.grounded = true;
        }
        let caster = f3::with_owner_rows(&mut w, |store, owner| owner.build(store, caster_unit)).reference();
        let enemy = f3::with_owner_rows(&mut w, |store, owner| owner.build(store, enemy_unit)).reference();
        w.sim.store.units.list.push(Member::Unit(caster));
        w.sim.store.units.list.push(Member::Unit(enemy));

        let class_record = bfe_content::json::parse(
            r#"{"class":"inv_hot_hero","weapons":[{"class":"inv_hot_cone","base":"cannon","slot":2,"channel":1,"TriggerSingle":"1","InitialSalvoDelay":"0.05","ShotDelay":"0.05","EnergyDrain":"0","ordnance":{"base":"emitterordnance","ConeLength":"10","ConeAngle":"60","MaxTargets":"3","NoChaining":"1","Damage":"20"}}]}"#,
        )
        .unwrap();
        powers::world::install_in(&mut w, 4, [("inv_hot_hero", &class_record)], |_| None).unwrap();
        let kit = powers::powers_in(&w).unwrap().kit_of("inv_hot_hero").unwrap();

        let row = ControlSpeed { forward: 1.0, strafe: 1.0, turn: 1.0 };
        let tuning = Tuning {
 recover_from_tumble: false,
 getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
 getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
            stand: row, crouch: row, sprint: row, jump: row, jet: row, jetpack: None, roll: row, tumble: row,
            sprint_accelerate_time: 0.3, energy_capacity: 100.0, energy_floor: 0.0, energy_restore: 7.0,
            energy_restore_idle: 11.0, energy_drain_sprint: 13.0, energy_min_sprint: 9.0, energy_cost_jump: 0.0,
            energy_cost_jump_sprint: None, energy_cost_roll: 17.0, jump_height: 2.5,
            jump_forward_speed_factor: 1.1, jump_strafe_speed_factor: 0.9, roll_speed_factor: 1.4, roll_frames: 18,
        };
        let class_name = w.sim.strings.intern_str("inv_hot_hero");
        let mut lives = vec![None; 4];
        lives[caster.slot as usize] = Some(Life {
            slab: caster.gen,
            class: 0,
            generation: 1.0,
            movement: crate::movement::soldier::swbf2::State::new(&tuning),
            allow_fire: true,
            allow_reload: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: 100.0,
            power_thrust: 1.0,
            blast_pending: false,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        w.units.infantry = Some(Box::new(Infantry {
            power_kits: vec![Some(kit)],
            classes: vec![Class { name: class_name, registry_id: None, tuning, acceleration: None, offhand_guns: Vec::new() }],
            lives,
            bot_inputs: BotInputs::new(4),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes: (0..4).map(|_| None).collect(),
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: vec![None; 4],
            bullets: vec![None; 4],
            observed: vec![None; 4],
            life_serial: 1,
            tick: 0,
        }));

        let trigger_cast = |w: &mut World| {
            w.units.infantry.as_mut().unwrap().lives[caster.slot as usize].as_mut().unwrap().power_input =
                PowerInput { selected: Some(0), pressed: true, held: true };
            for _ in 0..8 {
                pass(w);
                w.units.infantry.as_mut().unwrap().lives[caster.slot as usize].as_mut().unwrap().power_input =
                    PowerInput { selected: Some(0), pressed: false, held: false };
            }
        };
        let casts = |w: &World| {
            powers::powers_in(w)
                .unwrap()
                .log()
                .since(0)
                .filter(|e| e.kind == powers::world::PowerEventKind::Cast)
                .count()
        };

        // First two casts raise heat to 0.4 and 0.8 without overheating.
        trigger_cast(&mut w);
        let wp = &w.sim.store.unit.get(caster).unwrap().combat.weapons[1];
        assert_eq!(casts(&w), 1);
        assert!((wp.heat - 0.4).abs() < 1e-6 && !wp.overheated);

        trigger_cast(&mut w);
        let wp = &w.sim.store.unit.get(caster).unwrap().combat.weapons[1];
        assert_eq!(casts(&w), 2);
        assert!((wp.heat - 0.8).abs() < 1e-6 && !wp.overheated);

        // Third cast pushes heat past 1.0 and latches overheat.
        trigger_cast(&mut w);
        let wp = &w.sim.store.unit.get(caster).unwrap().combat.weapons[1];
        assert_eq!(casts(&w), 3);
        assert!(wp.heat >= 1.0 && wp.overheated);

        // Fourth press while overheated is refused by hero_powers::pass.
        trigger_cast(&mut w);
        assert_eq!(casts(&w), 3, "overheated power weapon must block new casts");

        // Cooling the weapon below HeatThreshold clears overheat and allows casting again.
        let defs = cmb::defs_in(&mut w.combat);
        let class = defs.weapons.class(DefId(0));
        let mut shots = Vec::new();
        let aim = Aim { stick: 0.0, posture: None, zoomed: false, repair_target: false };
        for _ in 0..180 {
            let wp = &mut w.sim.store.unit.get_mut(caster).unwrap().combat.weapons[1];
            wp.update(1, class, 1.0 / 60.0, true, &Trigger::default(), false, &aim, &mut ZeroDraws, &mut shots);
        }
        assert!(!w.sim.store.unit.get(caster).unwrap().combat.weapons[1].overheated);
        trigger_cast(&mut w);
        assert_eq!(casts(&w), 4, "casting resumes once cooled below HeatThreshold");
    }
}

#[cfg(test)]
mod tumble_tests {
    use super::*;
    use crate::combat::combo::{ComboMachine, StateId, WeaponMeleeSpec};
    use crate::gen::Soldier;
    use crate::movement::soldier::swbf2::{
        ControlSpeed, Roll, RollDirection, State as Swbf2State, Tuning,
        DEFAULT_GETUP_BACK_SECONDS, DEFAULT_GETUP_FRONT_SECONDS,
    };

    fn synthetic_world_and_key() -> (Box<World>, LifeKey) {
        let mut w = World::new();
        let u = crate::gen::Unit {
            alive: true,
            generation: 1.0,
            soldier: UnitBody::Soldier(Soldier {
                velocity: [0.0, 2.0, -9.5],
                aim_yaw: 0.0,
                lock: 0.75,
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = w.sim.store.unit.insert(u);
        let key = LifeKey::of(r, w.sim.store.unit.get(r).unwrap());
        let cs = |t| ControlSpeed { forward: 1.0, strafe: 1.0, turn: t };
        let tuning = Tuning {
            stand: cs(1.0),
            crouch: cs(0.8),
            sprint: cs(0.5),
            jump: cs(0.5),
            jet: cs(1.0),
            jetpack: None,
            roll: cs(0.4),
            tumble: cs(0.1),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 20.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            recover_from_tumble: false,
            getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
        };
        let doc = serde_json::json!({
            "nameHash": "0000beef",
            "states": [
                {"name": "IDLE", "children": [{"key": "Duration", "values": [0.0]}]},
                {"name": "STRIKE", "children": [{"key": "Duration", "values": [0.6]}]}
            ]
        });
        let machine = ComboMachine::from_json_value(&doc, &WeaponMeleeSpec::default(), 30.0, |_| Some(0.5)).unwrap();
        let mut hero = super::super::Hero::new(0, &machine, 10);
        hero.state.state = StateId(1);
        let slot = r.slot as usize;
        let mut lives = vec![None; slot + 1];
        lives[slot] = Some(super::super::Life {
            slab: r.gen,
            class: 0,
            generation: 1.0,
            movement: Swbf2State::new(&tuning),
            allow_fire: true,
            allow_reload: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: 100.0,
            power_thrust: 1.0,
            blast_pending: false,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        let mut heroes: Vec<Option<super::super::Hero>> = (0..=slot).map(|_| None).collect();
        heroes[slot] = Some(hero);
        let name = w.sim.strings.intern_str("synthetic_hero");
        w.units.infantry = Some(Box::new(super::super::Infantry {
            power_kits: vec![None],
            classes: vec![super::super::Class {
                name,
                registry_id: Some(1),
                tuning,
                acceleration: None,
                offhand_guns: Vec::new(),
            }],
            lives,
            bot_inputs: super::super::bot_input::BotInputs::new(slot + 1),
            mailbox: 0,
            player: 0,
            weapons: vec![Some(super::super::MeleeWeapon {
                weapon: 1,
                state_hashes: vec![0, 1],
                machine,
                spec: WeaponMeleeSpec::default(),
                pose: super::super::blade::BladePose::empty(),
                offhand: None,
                effects: Default::default(),
            })],
            heroes,
            sample: None,
            contacts: super::super::contact::Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; slot + 1],
            life_serial: 0,
            tick: 10,
        }));
        (w, key)
    }

    #[test]
    fn host_thrown_and_tumbled_reset_combo_and_clear_lock_immediately() {
        let (mut w, key) = synthetic_world_and_key();
        // Before calling Infantry::advance, Host::thrown must immediately reset hero.state to IDLE (0),
        // enter TumblePhase::Thrown, and clear soldier.lock.
        Host.thrown(&mut w, key);
        let inf = w.units.infantry.as_ref().unwrap();
        assert_eq!(inf.heroes[key.slot as usize].as_ref().unwrap().state.state.0, 0, "combo reset immediately on Host::thrown");
        assert_eq!(inf.lives[key.slot as usize].unwrap().movement.tumble.unwrap().phase, TumblePhase::Thrown);
        let UnitBody::Soldier(s) = &key.unit(&w.sim.store).unwrap().soldier else { unreachable!() };
        assert_eq!(s.lock, 0.0);

        // Fast flat ground push via Host::tumbled enters TumblePhase::Tumble directly and resets combo.
        w.units.infantry.as_mut().unwrap().heroes[key.slot as usize].as_mut().unwrap().state.state = StateId(1);
        Host.tumbled(&mut w, key);
        let inf = w.units.infantry.as_ref().unwrap();
        assert_eq!(inf.heroes[key.slot as usize].as_ref().unwrap().state.state.0, 0);
        assert_eq!(inf.lives[key.slot as usize].unwrap().movement.tumble.unwrap().phase, TumblePhase::Tumble);
    }

    #[test]
    fn host_footing_protects_thrown_tumble_landed_and_not_getup_or_recover() {
        let (mut w, key) = synthetic_world_and_key();
        let set_phase = |w: &mut World, phase: Option<TumblePhase>, roll: bool| {
            let l = w.units.infantry.as_mut().unwrap().lives[key.slot as usize].as_mut().unwrap();
            l.movement.roll = roll.then_some(Roll { direction: RollDirection::Forward, elapsed: 0.1 });
            l.movement.tumble = phase.map(|p| {
                let mut t = Tumble::enter(V3::ZERO, 0.0, 1);
                t.phase = p;
                t
            });
        };
        for phase in [TumblePhase::Thrown, TumblePhase::Tumble, TumblePhase::Landed] {
            set_phase(&mut w, Some(phase), false);
            assert_eq!(Host.footing(&w, key), Some(Footing::Thrown), "phase {phase:?} must report Footing::Thrown");
        }
        for phase in [TumblePhase::GetUp, TumblePhase::Recover] {
            set_phase(&mut w, Some(phase), false);
            assert_eq!(Host.footing(&w, key), None, "phase {phase:?} falls back to grounded/air footing");
        }
        set_phase(&mut w, None, true);
        assert_eq!(Host.footing(&w, key), Some(Footing::Rolling));
    }

    #[test]
    fn tumble_look_uses_class_tumble_turn_scale_and_getup_locks_look() {
        let (mut w, key) = synthetic_world_and_key();
        w.units.infantry.as_mut().unwrap().classes[0].tuning.tumble.turn = 0.12;
        if let UnitBody::Soldier(s) = &mut key.unit_mut(&mut w.sim.store).unwrap().soldier {
            s.max_turn = 4.0;
            s.pitch_limits = [-1.0, 1.0];
            s.lock = 0.0;
        }
        let r = key.reference();
        w.units.infantry.as_mut().unwrap().heroes[r.slot as usize].as_mut().unwrap().state.state = StateId(0);
        let class_name = w.units.infantry.as_ref().unwrap().classes[0].name;
        // The one look-rate rule: the standing scale (1.0) times the combo factor (1.0 here).
        let scale_of = |w: &World| {
            let UnitBody::Soldier(s) = &key.unit(&w.sim.store).unwrap().soldier else { unreachable!() };
            w.units.infantry.as_ref().unwrap().mouse_turn_scale(r.slot, class_name, 1.0, s)
        };
        assert_eq!(scale_of(&w), Some(1.0));

        // Baseline standing turn for dx=100.
        let mut owner = crate::units::ownership::Ownership::default();
        owner.adopt(&w.sim.store, r).unwrap();
        let cmd = crate::gen::F3LookCommand {
            key: crate::gen::F3UnitKey { slot: r.slot, generation: r.gen, life: 1.0 },
            dx: 100.0,
            dy: -50.0,
            sensitivity: 1.0,
            invert: false,
            raw: true,
        };
        owner.look_scaled(&mut w.sim.store, &cmd, None).unwrap();
        let (stand_dyaw, stand_dpitch) = match &key.unit(&w.sim.store).unwrap().soldier {
            UnitBody::Soldier(s) => (s.aim_yaw, s.aim_pitch),
            UnitBody::Droideka(_) => unreachable!(),
        };
        assert!(stand_dyaw.abs() > 0.01 && stand_dpitch.abs() > 0.01);

        // Thrown, Tumble, and Landed each use the class's tumble turn multiplier (0.12, not 0.5 or 1.0).
        for phase in [TumblePhase::Thrown, TumblePhase::Tumble, TumblePhase::Landed] {
            {
                let l = w.units.infantry.as_mut().unwrap().lives[r.slot as usize].as_mut().unwrap();
                let mut t = Tumble::enter(V3::ZERO, 0.0, 7);
                t.phase = phase;
                l.movement.tumble = Some(t);
            }
            if let UnitBody::Soldier(s) = &mut key.unit_mut(&mut w.sim.store).unwrap().soldier {
                s.aim_yaw = 0.0;
                s.aim_pitch = 0.0;
                s.lock = 0.0;
                s.air = (phase == TumblePhase::Thrown).then_some(crate::gen::Air::Thrown);
            }
            let scale = scale_of(&w);
            assert_eq!(scale, Some(0.12), "phase {phase:?} must select class tumble turn scale");
            owner.look_scaled(&mut w.sim.store, &cmd, scale).unwrap();
            let UnitBody::Soldier(s) = &key.unit(&w.sim.store).unwrap().soldier else { unreachable!() };
            assert!(
                (s.aim_yaw - stand_dyaw * 0.12).abs() < 1e-12,
                "phase {phase:?} yaw delta {} must equal 0.12x standing {}",
                s.aim_yaw,
                stand_dyaw * 0.12,
            );
            assert!((s.aim_pitch - stand_dpitch).abs() < 1e-12);
        }

        // GetUp does not use tumble look scale and freezes both yaw and pitch via Soldier::lock.
        {
            let l = w.units.infantry.as_mut().unwrap().lives[r.slot as usize].as_mut().unwrap();
            let mut t = Tumble::enter(V3::ZERO, 0.0, 7);
            t.phase = TumblePhase::GetUp;
            l.movement.tumble = Some(t);
        }
        if let UnitBody::Soldier(s) = &mut key.unit_mut(&mut w.sim.store).unwrap().soldier {
            s.aim_yaw = 0.3;
            s.aim_pitch = -0.2;
            s.lock = 1.5;
            s.air = None;
        }
        let scale = scale_of(&w);
        assert_ne!(scale, Some(0.12));
        owner.look_scaled(&mut w.sim.store, &cmd, scale).unwrap();
        let UnitBody::Soldier(s) = &key.unit(&w.sim.store).unwrap().soldier else { unreachable!() };
        assert_eq!((s.aim_yaw, s.aim_pitch), (0.3, -0.2));
    }

    #[test]
    fn f3_command_in_look_applies_class_tumble_turn_scale_and_locks_getup() {
        use crate::gen::{Air, Attacker, F3Action, F3Change, F3UnitKey, Posture, Unit};
        let mut w = World::new();
        crate::units::f3::f3_init_in(&mut w, 0, 0.0, 0);
        let class_name = w.sim.strings.intern_str("synthetic_hero");
        let u = Unit {
            class_name,
            alive: true,
            human: true,
            generation: 1.0,
            soldier: UnitBody::Soldier(Soldier {
                max_turn: 4.0,
                pitch_limits: [-1.0, 1.0],
                tick_length: 1.0 / 60.0,
                grounded: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = crate::units::f3::with_owner_rows(&mut w, |store, owner| owner.build(store, u)).reference();
        let cs = |t| ControlSpeed { forward: 1.0, strafe: 1.0, turn: t };
        let tuning = Tuning {
            stand: cs(1.0),
            crouch: cs(0.85),
            sprint: cs(0.5),
            jump: cs(0.5),
            jet: cs(1.0),
            jetpack: None,
            roll: cs(0.4),
            tumble: cs(0.12),
            sprint_accelerate_time: 0.25,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 15.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 10.0,
            energy_cost_jump: 5.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 20.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            recover_from_tumble: false,
            getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
        };
        let slot = r.slot as usize;
        let mut lives = vec![None; slot + 1];
        lives[slot] = Some(super::super::Life {
            slab: r.gen,
            class: 0,
            generation: 1.0,
            movement: Swbf2State::new(&tuning),
            allow_fire: true,
            allow_reload: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: 100.0,
            power_thrust: 1.0,
            blast_pending: false,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        let heroes: Vec<Option<super::super::Hero>> = (0..=slot).map(|_| None).collect();
        w.units.infantry = Some(Box::new(super::super::Infantry {
            power_kits: vec![None],
            classes: vec![super::super::Class {
                name: class_name,
                registry_id: Some(1),
                tuning,
                acceleration: None,
                offhand_guns: Vec::new(),
            }],
            lives,
            bot_inputs: super::super::bot_input::BotInputs::new(slot + 1),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes,
            sample: None,
            contacts: super::super::contact::Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; slot + 1],
            life_serial: 0,
            tick: 10,
        }));

        let run_look = |w: &mut World, action: F3Action, dx: f64, dy: f64| {
            let unit = w.sim.store.unit.get(r).unwrap();
            let change = F3Change {
                key: F3UnitKey::of(r, unit),
                action,
                vector: [dx, dy, 1.0],
                flag: false,
                attacker: Attacker::Null,
                ..Default::default()
            };
            change.put(crate::units::f3::command_row_in(&mut w.units), &mut Vec::new());
            assert_eq!(crate::units::f3::f3_command_in(w), 1);
            let UnitBody::Soldier(s) = &w.sim.store.unit.get(r).unwrap().soldier else { unreachable!() };
            (s.aim_yaw, s.aim_pitch)
        };

        // The replay's rule is the authority's: the online client's look scale of the same state
        // (`client::look_scale`) equals what `mouse_turn_scale` gives the command.
        let tie = |w: &World| {
            let unit = w.sim.store.unit.get(r).unwrap();
            let UnitBody::Soldier(s) = &unit.soldier else { unreachable!() };
            let infantry = w.units.infantry.as_ref().unwrap();
            let authority = infantry.mouse_turn_scale(r.slot, class_name, 1.0, s).unwrap();
            let state = &infantry.lives[slot].as_ref().unwrap().movement;
            assert_eq!(crate::client::look_scale(&tuning, state, s), authority, "{:?} {:?} {:?}", s.posture, s.air, state.tumble.map(|t| t.phase));
        };
        for (action, dx, dy) in [(F3Action::LookRaw, 100.0, -40.0), (F3Action::Look, 40.0, -20.0)] {
            // Standing baseline.
            w.units.infantry.as_mut().unwrap().lives[slot].as_mut().unwrap().movement.tumble = None;
            if let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(r).unwrap().soldier {
                s.aim_yaw = 0.0;
                s.aim_pitch = 0.0;
                s.look_x = 0.0;
                s.look_y = 0.0;
                s.posture = Posture::Stand;
                s.air = None;
                s.lock = 0.0;
            }
            tie(&w);
            let (stand_dyaw, stand_dpitch) = run_look(&mut w, action, dx, dy);
            assert!(stand_dyaw.abs() > 0.01 && stand_dpitch.abs() > 0.01);

            // Thrown, Tumble (grounded crouch), and Landed (grounded crouch) all route through
            // f3::command_in and apply the class tumble turn scale (0.12).
            for (phase, posture, air) in [
                (TumblePhase::Thrown, Posture::Stand, Some(Air::Thrown)),
                (TumblePhase::Tumble, Posture::Crouch, None),
                (TumblePhase::Landed, Posture::Crouch, None),
            ] {
                let mut t = Tumble::enter(V3::ZERO, 0.0, 7);
                t.phase = phase;
                w.units.infantry.as_mut().unwrap().lives[slot].as_mut().unwrap().movement.tumble = Some(t);
                if let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(r).unwrap().soldier {
                    s.aim_yaw = 0.0;
                    s.aim_pitch = 0.0;
                    s.look_x = 0.0;
                    s.look_y = 0.0;
                    s.posture = posture;
                    s.air = air;
                    s.lock = 0.0;
                }
                tie(&w);
                let (yaw, pitch) = run_look(&mut w, action, dx, dy);
                assert!(
                    (yaw - stand_dyaw * 0.12).abs() < 1e-12,
                    "{action:?} phase {phase:?} yaw {yaw} must equal 0.12x standing {}",
                    stand_dyaw * 0.12,
                );
                assert!((pitch - stand_dpitch).abs() < 1e-12);
            }

            // GetUp freezes both yaw and pitch via Soldier::lock through f3::command_in.
            let mut getup = Tumble::enter(V3::ZERO, 0.0, 7);
            getup.phase = TumblePhase::GetUp;
            w.units.infantry.as_mut().unwrap().lives[slot].as_mut().unwrap().movement.tumble = Some(getup);
            if let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(r).unwrap().soldier {
                s.aim_yaw = 0.3;
                s.aim_pitch = -0.2;
                s.look_x = 0.0;
                s.look_y = 0.0;
                s.posture = Posture::Stand;
                s.air = None;
                s.lock = 1.5;
            }
            tie(&w);
            assert_eq!(run_look(&mut w, action, dx, dy), (0.3, -0.2));

            // Recover exits tumble-look scaling and uses Air::Fall (jump scale 0.5x).
            let mut recover = Tumble::enter(V3::ZERO, 0.0, 7);
            recover.phase = TumblePhase::Recover;
            w.units.infantry.as_mut().unwrap().lives[slot].as_mut().unwrap().movement.tumble = Some(recover);
            if let UnitBody::Soldier(s) = &mut w.sim.store.unit.get_mut(r).unwrap().soldier {
                s.aim_yaw = 0.0;
                s.aim_pitch = 0.0;
                s.look_x = 0.0;
                s.look_y = 0.0;
                s.posture = Posture::Stand;
                s.air = Some(Air::Fall);
                s.lock = 0.0;
            }
            tie(&w);
            let (yaw, pitch) = run_look(&mut w, action, dx, dy);
            assert!(
                (yaw - stand_dyaw * 0.5).abs() < 1e-12,
                "{action:?} Recover yaw {yaw} must equal 0.5x standing {}",
                stand_dyaw * 0.5,
            );
            assert!((pitch - stand_dpitch).abs() < 1e-12);
        }
    }
}
