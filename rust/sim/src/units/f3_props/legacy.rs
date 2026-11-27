// Native building decisions used by the 2004 game.
use super::*;

pub(super) fn commit_in(world_ctx: &mut crate::world::owner::World, index: usize, frame: &props::Building) {
    let entry = &mut f3::props_in(&mut world_ctx.units).entries[index];
    entry.put_frame(&mut world_ctx.battle, frame); entry.geometry_state = frame.state;
    if frame.last_attacker == props::Identity::Null { entry.put_attacker(&mut world_ctx.battle, Attacker::Null); }
    // Death does not advance a turret's target generation; a completed repair
    // does. Update the registry's health and life at the same write point.
    let member = entry.member;
    let (center, radius) = entry.trap_sphere.unwrap_or_else(|| frame.sphere(&entry.settings));
    let bodies = entry.bodies;
    let team = target_team(&world_ctx.fleet,&world_ctx.sim.store, entry);
    let target = entry.target;
    let attacker = entry.attacker(&mut world_ctx.battle);
    if let RepairTarget::Turret(turret) = target {
        crate::vehicles::module::f2::live_seats::turret_life_in(world_ctx, turret, frame.alive(), attacker,
            frame.target_generation.unwrap_or(0.0), center, radius);
    }
    if let Some(member) = member {
        f3::prop_target_in(&mut world_ctx.units, member, frame.alive(), frame.health, a(center), radius, frame.target_generation, team);
    }
    let collision = world_ctx.fleet.collision.as_ref().expect("prepared prop collision world");
    for (look, body) in bodies.into_iter().enumerate() {
        let Some(kernel) = body.and_then(|body| collision.kernel_slot(body.slot)) else { continue };
        let world = &world_ctx.collision.state.worlds[f3::collision_world_in(&world_ctx.units)];
        if let Some(order) = world.order.iter().position(|&slot| slot as usize == kernel) {
            world_ctx.movement.flags_mut()[order] = u8::from(look == frame.state.index());
        }
    }
}

pub(super) fn needs_regeneration_in(world_ctx: &mut crate::world::owner::World, index: usize) -> bool {
    let entry = &f3::props_read(&world_ctx.units).entries[index];
    if !entry.settings.regenerates { return false; }
    let frame = entry.frame(&mut world_ctx.battle); frame.add_health > 0.0 && frame.health < entry.settings.max_health
}

pub(super) fn regenerate_in(world_ctx: &mut crate::world::owner::World, index: usize, dt: f64, services: WorldServices<'_>) {
    let entry = &f3::props_in(&mut world_ctx.units).entries[index];
    let mut frame = entry.frame(&mut world_ctx.battle); let settings = entry.settings;
    if !settings.regenerates || !(frame.add_health > 0.0 && frame.health < settings.max_health) { return; }
    frame.regenerate(dt, &settings, &mut BuildingWorld { w: world_ctx, index, services, hit: None });
    commit_in(world_ctx, index, &frame);
}

pub(super) fn mutate_in(world_ctx: &mut crate::world::owner::World, index: usize, amount: f64, attacker: Option<Attacker>, hit: Option<HitInfo>, services: WorldServices<'_>) {
    let (target, gunner) = { let entry = &f3::props_read(&world_ctx.units).entries[index]; (entry.target, entry.gunner.is_some()) };
    let occupied = match target {
        RepairTarget::Turret(turret) => crate::vehicles::module::f2::live_seats::turret_gunner_in(world_ctx, turret).is_some(),
        _ => gunner,
    };
    let entry = &mut f3::props_in(&mut world_ctx.units).entries[index];
    let mut frame = entry.frame(&mut world_ctx.battle);
    let mut settings = entry.settings;
    settings.has_crew = occupied;
    // Building.damage rejects a dead target before replacing lastAttacker.
    // A repair has no attacker write, so an old repair lock or credit survives.
    if let Some(attacker) = attacker {
        if !frame.alive() || !(amount > 0.0) { return; }
        entry.put_attacker(&mut world_ctx.battle, attacker);
        frame.last_attacker = identity(attacker, index);
    }
    let trap = entry.trap(&mut world_ctx.battle);
    entry.put_frame(&mut world_ctx.battle, &frame);
    let mut world = BuildingWorld { w: world_ctx, index, services, hit };
    if let Some(mut trap) = trap {
        // Building.damage delegates to Trap.setHealth for this subclass. At
        // zero health it stays built until the next outside trap phase.
        let value = if attacker.is_some() { frame.health - amount } else { amount };
        trap.set_health_with(value, settings.max_health, &mut world);
        f3::props_in(&mut world.w.units).entries[index].put_trap(&mut world.w.battle, trap);
        frame.health = trap.health;
        frame.state = if trap.built { props::State::Built } else { props::State::Unbuilt };
    } else if attacker.is_some() {
        frame.damage(amount, props::Identity::Object(index), props::Identity::Null, &settings, &mut world);
    } else {
        frame.set_health(amount, &settings, &mut world);
    }
    // Full rebuild changes the turret life after show returns. Publish that
    // final scalar state too, without resending any other building or Unit.
    world.leaf(&frame, F3PropEffectKind::Publish);
}

impl QueryWorld<'_, '_> {
    pub(super) fn released_target(&mut self, index: usize) -> live::Target {
        let buildings = f3::props_in(&mut self.w.units).entries.len();
        if index >= buildings {
            let r = super::f3_hulls::reference_in(&self.w.sim, index - buildings);
            let v = self.w.sim.store.vehicle.get(r).expect("repair hull");
            let mut matrix = three::Matrix4::default(); matrix.elements = v.matrix;
            return live::Target { alive:v.alive, state:None,
                health_type:super::f3_hulls::repairable_in(&self.w.units, r).then_some(live::Health::Vehicle),
                health:v.health, max_health:v.max_health, team:v.team.unwrap_or(0.0),
                body:Some(v.body.slot as usize), gunner:false, position:Vector3::new(v.position[0],v.position[1],v.position[2]), matrix };
        }
        let target = f3::props_read(&self.w.units).entries[index].target;
        let gunner = match target { RepairTarget::Turret(r) => crate::vehicles::module::f2::live_seats::turret_gunner_in(self.w, r).is_some(), _ => false };
        let entry = &f3::props_in(&mut self.w.units).entries[index];
        let frame = entry.frame(&mut self.w.battle);
        let health_type = if entry.settings.kind == props::Kind::Objective && !frame.alive() { None } else {
            Some(match frame.state { props::State::Built => live::Health::Building,
                props::State::Destroyed => live::Health::Rebuild, props::State::Unbuilt => live::Health::Build })
        };
        let mut position = Vector3::default();
        position.set_from_matrix_position(&entry.settings.matrix);
        live::Target {
            alive: frame.alive(), state: Some(frame.state), health_type, health: frame.health,
            max_health: entry.settings.max_health, team: entry.team,
            body: entry.bodies[frame.state.index()].map(|b| b.slot as usize),
            gunner,
            position, matrix: entry.settings.matrix,
        }
    }
}

impl Entry {
    pub(super) fn put_frame(&self, battle: &mut crate::battle::owner::BattleOwner, frame: &props::Building) { f4::put_frame_in(battle, &self.owner, frame); }
    pub(super) fn put_trap(&self, battle: &mut crate::battle::owner::BattleOwner, trap: traps::Trap) { f4::put_trap_in(battle, &self.owner, trap); }
}

mod f4 {
use super::*;
use crate::battle::{owner::BattleOwner, f4::live_in};
pub(super) fn put_frame_in(battle: &mut BattleOwner, h: &F4Handle, frame: &props::Building) {
    if h.kind == F4ObjectKind::Trap {
        let h = live_in(battle).traps.current(h).expect("current trap row");
        live_in(battle).traps.lifecycle_command(&h, |e| {
            let was = e.state.built; e.state.built = frame.alive(); e.state.health = frame.health;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            !was && e.state.built
        });
    } else {
        let h = live_in(battle).buildings.current(h).expect("current building row");
        live_in(battle).buildings.lifecycle_command(&h, |e| {
            let was = e.state.building.alive(); e.state.building = *frame;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            !was && frame.alive()
        });
    }
}

pub(super) fn put_trap_in(battle: &mut BattleOwner, h: &F4Handle, trap: traps::Trap) {
    let h = live_in(battle).traps.current(h).expect("current trap row");
    live_in(battle).traps.lifecycle_command(&h, |e| { let was = e.state.built; e.state = trap; !was && trap.built });
}
}
