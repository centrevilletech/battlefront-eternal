//! Dispatch 2005 abilities separately from ordinary gun triggers.
use crate::combat::{defs::{WeaponClass, WeaponDefs}, weapons::Arsenal};
use crate::gen::{Combat, Unit};

/// A weapon that is a soldier's ability: melee, a thrown blade or a power. Its
/// trigger is the soldier's, never the arsenal's.
pub(crate) fn special_action(class: &WeaponClass) -> bool {
    matches!(class.base().obj.get("base").str(), Some("melee" | "meleethrow" | "areaeffectweapon"))
}

/// The gun controller samples fire, then changes the equipped weapon before
/// firing. Resolve that same selection for simultaneous switch/fire input.
/// A special ability without a solver consumes its trigger on this tick.
pub(in crate::units) fn mask_special_actions(
    combat: &mut Combat,
    unit: &mut Unit,
    defs: &WeaponDefs,
    offhand_edge: bool,
    powers: &[Option<(usize, u8)>],
) -> (bool, Option<u8>) {
    let mut throw = false;
    let mut power = None;
    let mut selected = combat.selected;
    let arsenal = Arsenal {
        weapons: &mut combat.weapons,
        selected: &mut selected,
    };
    for channel in 0..2 {
        if unit.intent.buttons[channel] == 0.
            && !unit.controls.pressed[channel]
            && !(channel == 1 && offhand_edge)
        {
            continue;
        }
        let forward = if channel == 0 {
            unit.intent.next_primary
        } else {
            unit.intent.next_secondary
        };
        let backward = channel == 0 && unit.intent.previous_primary;
        if forward || backward {
            let count = arsenal
                .weapons
                .iter()
                .filter(|w| w.in_channel(defs, channel as f64))
                .count();
            if count > 1 {
                for direction in [i32::from(forward), -i32::from(backward)] {
                    if direction != 0
                        && !arsenal
                            .current(defs, channel)
                            .is_some_and(|i| arsenal.weapons[i].busy())
                    {
                        arsenal.selected[channel] = (arsenal.selected[channel]
                            + f64::from(direction))
                        .rem_euclid(count as f64);
                    }
                }
            }
        }
        if let Some(i) = arsenal.current(defs, channel) {
            let class = defs.class(arsenal.weapons[i].def);
            if channel == 1 && class.base().obj.get("base").str() == Some("meleethrow") {
                throw = offhand_edge;
            }
            // An offhand hero power runs in the powers pass, never on the gun path.
            let kit = (channel == 1).then(|| powers.iter().flatten().find(|(w, _)| *w == i).map(|(_, k)| *k)).flatten();
            if kit.is_some() {
                power = kit;
            }
            if kit.is_some() || special_action(class) {
                unit.intent.buttons[channel] = 0.;
                unit.controls.pressed[channel] = false;
            }
        }
    }
    (throw, power)
}

/// The kit index of the power selected on the offhand channel, read from the
/// equipped selection alone.
pub(in crate::units) fn offhand_power(combat: &Combat, defs: &WeaponDefs, powers: &[Option<(usize, u8)>]) -> Option<u8> {
    let selected = combat.selected.get(1).copied()?;
    if !(selected >= 0.0 && selected.fract() == 0.0) {
        return None;
    }
    let index = combat
        .weapons
        .iter()
        .enumerate()
        .filter(|(_, w)| w.in_channel(defs, 1.0))
        .nth(selected as usize)
        .map(|(i, _)| i)?;
    powers.iter().flatten().find(|(w, _)| *w == index).map(|(_, k)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        combat::defs::{Js, JsObj, WeaponClass},
        gen::Weapon,
        store::DefId,
    };
    use std::rc::Rc;

    #[test]
    fn special_actions_leave_the_other_channel_and_selection_intact() {
        let mut defs = WeaponDefs::default();
        let mut combat = Combat::default();
        for (i, (channel, base)) in [
            (0., "cannon"),
            (1., "meleethrow"),
            (1., "cannon"),
            (1., "areaeffectweapon"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = DefId(i as u32);
            let fields = vec![
                ("base".into(), Js::Str(base.into())),
                ("channel".into(), Js::Num(channel)),
            ];
            defs.set(id, WeaponClass::new(Rc::new(JsObj { fields, def: None })));
            combat.weapons.push(Weapon::create(id, &defs, false));
        }
        let mut unit = Unit::default();
        for (selected, cycle, allowed) in [
            (0., false, false),
            (0., true, true),
            (1., false, true),
            (1., true, false),
            (2., false, false),
        ] {
            combat.selected = [0., selected];
            unit.intent.buttons = [1., 1.];
            unit.controls.pressed = vec![true, true];
            unit.intent.next_secondary = cycle;
            mask_special_actions(&mut combat, &mut unit, &defs, false, &[]);
            assert_eq!(unit.controls.pressed, [true, allowed]);
            assert_eq!(unit.intent.buttons, [1., f64::from(u8::from(allowed))]);
            assert_eq!(combat.selected, [0., selected]);
            assert_eq!(unit.intent.next_secondary, cycle);
        }
        // An offhand weapon that is a hero power leaves the gun path and is
        // reported with its kit index; a primary emitter stays with the gun.
        combat.selected = [0., 1.];
        unit.intent.buttons = [1., 1.];
        unit.controls.pressed = vec![true, true];
        unit.intent.next_secondary = false;
        let (_, power) = mask_special_actions(&mut combat, &mut unit, &defs, false, &[Some((2, 7)), Some((0, 3))]);
        assert_eq!(power, Some(7));
        assert_eq!(unit.controls.pressed, [true, false]);
        assert_eq!(unit.intent.buttons, [1., 0.]);
        // The selected offhand power is known with the trigger up too.
        combat.selected = [0., 1.];
        assert_eq!(offhand_power(&combat, &defs, &[Some((2, 7)), Some((3, 4))]), Some(7));
        combat.selected = [0., 0.];
        assert_eq!(offhand_power(&combat, &defs, &[Some((2, 7)), Some((3, 4))]), None, "a blade throw is no power");
        combat.selected = [0., 2.];
        assert_eq!(offhand_power(&combat, &defs, &[Some((2, 7)), Some((3, 4))]), Some(4));
        // A returning blade on the offhand channel is thrown on a fresh press only: a trigger
        // held over from another weapon (the wheel turned while it was down) throws nothing.
        combat.selected = [0., 0.];
        unit.intent.buttons = [0., 1.];
        unit.controls.pressed = vec![false, false];
        let (thrown, power) = mask_special_actions(&mut combat, &mut unit, &defs, false, &[Some((3, 4))]);
        assert_eq!((thrown, power), (false, None), "a held trigger throws nothing");
        unit.intent.buttons = [0., 1.];
        let (thrown, _) = mask_special_actions(&mut combat, &mut unit, &defs, true, &[Some((3, 4))]);
        assert!(thrown, "a fresh press throws");
    }
}
