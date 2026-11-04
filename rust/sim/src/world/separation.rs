//! A narrow live boundary for the existing units::separate implementation.
//! It retains native cylinders and body slots between calls; only fields
//! the collision pass reads cross while the outer world still has mirrors.
use crate::gen::{Droideka, Held, Member, Soldier, Unit, UnitBody, Units, WsSeparated, WsSeparation, WsSeparationResult};
use crate::store::{word_u32, Nullable, Ref, Slab};
use crate::units::separate::{separate, Pushers, Scope};
use three::Vector3;

#[derive(Default)]
pub struct Separation {
    registry: Units,
    units: Slab<Unit>,
    pushers: Pushers,
}

impl Separation {
    pub fn run(&mut self, input: WsSeparation) -> WsSeparationResult {
        self.registry.list.clear();
        for member in &input.members {
            let id = word_u32(member.id);
            let key = Ref::new(id, 0);
            if self.units.get(key).is_none() { self.units.insert_at(id, 0, Unit::default()); }
            let unit = self.units.get_mut(key).unwrap();
            unit.seat = member.seat.then(|| Ref::new(0, 0));
            unit.held = if member.held { Nullable::Value(Held::default()) } else { Nullable::Undefined };
            unit.removed = member.removed;
            // The boundary supplies the resolved hit posture, including
            // corpse, dive and airborne rules. Keep eligibility separate
            // so Unit::hit_posture does not resolve that posture twice.
            unit.alive = member.alive || member.dead; unit.dead = false;
            unit.walker = member.walker; unit.tick_count = member.tick_count;
            if member.walker {
                if !matches!(unit.soldier, UnitBody::Droideka(_)) { unit.soldier = UnitBody::Droideka(Droideka::default()); }
                if let UnitBody::Droideka(body) = &mut unit.soldier { body.position = member.position; }
            } else {
                if !matches!(unit.soldier, UnitBody::Soldier(_)) { unit.soldier = UnitBody::Soldier(Soldier::default()); }
                if let UnitBody::Soldier(body) = &mut unit.soldier { body.position = member.position; body.posture = member.posture; }
            }
            self.registry.list.push(Member::Unit(key));
        }
        let scope = Scope { player: input.player.map(word_u32), player_out: input.player_out, global: input.global,
            camera: Vector3 { x: input.camera[0], y: input.camera[1], z: input.camera[2] } };
        separate(&self.registry, &mut self.units, &scope, &mut self.pushers);
        WsSeparationResult { members: input.members.iter().map(|member| {
            let unit = self.units.get(Ref::new(word_u32(member.id), 0)).unwrap();
            let p = unit.position();
            WsSeparated { position: [p.x, p.y, p.z], tick_count: unit.tick_count }
        }).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{Posture, WsMember};

    #[test]
    fn reused_body_slots_preserve_filtering_and_tick_counts() {
        let mut phase = Separation::default();
        let mut input = WsSeparation { global: true, members: vec![
            WsMember { id: 2.0, position: [0.0; 3], alive: true, posture: Posture::Stand, ..Default::default() },
            WsMember { id: 5.0, position: [0.25, 0.0, 0.0], alive: true, posture: Posture::Stand, ..Default::default() },
        ], ..Default::default() };
        let first = phase.run(input.clone());
        assert_eq!(first.members[0].tick_count, 1.0); assert_eq!(first.members[1].tick_count, 1.0);
        assert!(first.members[0].position[0] < 0.0); assert!(first.members[1].position[0] > 0.25);
        input.members[0].walker = true; input.members[1].held = true;
        let second = phase.run(input.clone());
        assert_eq!(second.members[0].position, input.members[0].position);
        assert_eq!(second.members[0].tick_count, 0.0);
        input.members[1].held = false; input.player = Some(2.0); input.player_out = true;
        let third = phase.run(input);
        assert_eq!(third.members[1].tick_count, 0.0);
    }
}
