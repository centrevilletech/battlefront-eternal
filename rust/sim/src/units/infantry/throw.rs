//! The selected offhand throw and its returning blade share the soldier's energy account.
use super::{
    blade::Root,
    contact::{self, Cause, Contact, Outcome, Strike},
    Hero, MeleeWeapon,
};
use crate::{
    combat::{
        combo::{
            self, MeleeHitOutcome, MeleeTarget, SaberThrowDef, SaberThrowPhase,
            ValidatedSaberThrowDef,
        },
        impacts::HitPacket,
    },
    gen::{Unit, UnitBody},
    store::Ref,
    world::owner::World,
};
use bfe_content::Value;

pub(super) struct Throw {
    pub weapon: u32,
    pub def: ValidatedSaberThrowDef,
    pub windup_secs: f64,
    pub windup: u32,
    pub reload: u32,
}

fn number(value: &Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(|n| n.as_f64().or_else(|| n.as_str()?.parse().ok()))
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("invalid throw property {key}"))
}

fn dimension(record: &Value, key: &str, edge: f64) -> Result<f64, String> {
    let Some(text) = record.get(key).and_then(Value::as_str) else {
        return number(record, key);
    };
    let mut tokens = text.split_ascii_whitespace();
    let n: f64 = tokens
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("invalid throw {key}"))?;
    match (tokens.next(), tokens.next()) {
        (None, None) => Ok(n),
        (Some(kind), None) if kind.eq_ignore_ascii_case("FromEdge") => Ok(n * edge),
        _ => Err(format!("invalid throw dimension {key}")),
    }
}

pub(super) fn bind(
    unit: &Value,
    primary: &combo::WeaponMeleeSpec,
) -> Result<Option<Throw>, String> {
    let Some(weapon) = bfe_pose::character::array(unit, "weapons")
        .iter()
        .find(|w| w.get("base").and_then(Value::as_str) == Some("meleethrow"))
    else {
        return Ok(None);
    };
    let ord = weapon.get("ordnance").ok_or("missing thrown ordnance")?;
    let edge = primary.edges[0];
    let mut scales = [1.; 5];
    for (scale, key) in scales.iter_mut().zip([
        "PersonScale",
        "AnimalScale",
        "DroidScale",
        "VehicleScale",
        "BuildingScale",
    ]) {
        if ord.get(key).is_some() {
            *scale = number(ord, key)?;
        }
    }
    let def = SaberThrowDef {
        energy_drain: number(weapon, "EnergyDrain")?,
        velocity: number(ord, "Velocity")?,
        life_span: number(ord, "LifeSpan")?,
        return_duration: number(ord, "ReturnDuration")?,
        rotation_rate: number(ord, "RotationRate")?,
        damage: number(ord, "Damage")?,
        damage_length: dimension(ord, "DamageLength", edge.default_length)?,
        damage_width: dimension(ord, "DamageWidth", edge.default_width)?,
        push: if ord.get("Push").is_some() {
            number(ord, "Push")?
        } else {
            0.
        },
        damage_scales: scales,
        shield_scale: if ord.get("ShieldScale").is_some() {
            number(ord, "ShieldScale")?
        } else {
            1.
        },
    };
    let ticks = |key| -> Result<u32, String> {
        if weapon.get(key).is_none() {
            return Ok(0);
        }
        let time = number(weapon, key)?;
        if time < 0. {
            return Err(format!("negative throw {key}"));
        }
        Ok((time * 60.).ceil() as u32)
    };
    let windup_secs = if weapon.get("InitialSalvoDelay").is_none() {
        0.
    } else {
        let time = number(weapon, "InitialSalvoDelay")?;
        if time < 0. {
            return Err("negative throw InitialSalvoDelay".into());
        }
        time
    };
    Ok(Some(Throw {
        weapon: combo::zero_fnv1a_lower(
            weapon
                .get("class")
                .and_then(Value::as_str)
                .ok_or("missing throw class")?,
        ),
        def: ValidatedSaberThrowDef::try_new(def).map_err(|e| e.to_string())?,
        windup_secs,
        windup: ticks("InitialSalvoDelay")?,
        reload: ticks("ReloadTime")?,
    }))
}

/// The share of the throw's full range (`Velocity x LifeSpan`) that is free of static
/// The soldier's aim origin for each posture (stand, crouch, prone) in its own frame (+x left,
/// y up, +z forward), and the fire point's offsets from it: along +x, along aim x (+x), along
/// the aim. The game's soldier update places its aimer's fire point there every frame, and a
/// thrown blade starts at that point, not at the hand (the throw clip's mount at the release
/// only sets the blade's orientation and the point it comes back to).
const AIM_ORIGIN: [[f64; 3]; 3] = [[0.06, 1.7, 0.], [-0.1, 1.2, 0.], [0.06, 0.6, 0.4]];
const FIRE_OFFSET: [[f64; 3]; 3] = [[-0.12, -0.1, 0.25], [-0.12, -0.05, 0.25], [-0.12, -0.2, 0.15]];

/// Where a thrown blade leaves: the fire point of a soldier at `root` aiming along `aim` (unit).
pub(super) fn fire_point(root: Root, aim: [f64; 3], posture: crate::gen::Posture) -> [f64; 3] {
    let p = match posture {
        crate::gen::Posture::Stand => 0,
        crate::gen::Posture::Crouch => 1,
        crate::gen::Posture::Prone => 2,
    };
    let eye = crate::wire::thrown::release_point(root.position, root.yaw, AIM_ORIGIN[p]);
    let (s, c) = v8math::sin_cos(root.yaw);
    let left = [c, 0., -s];
    let across = [aim[1] * left[2] - aim[2] * left[1], aim[2] * left[0] - aim[0] * left[2], aim[0] * left[1] - aim[1] * left[0]];
    let [a, b, along] = FIRE_OFFSET[p];
    std::array::from_fn(|i| eye[i] + aim[i] * along + left[i] * a + across[i] * b)
}

/// geometry from `origin` along `aim`: 1 when nothing stands in the way.
fn free_reach(w: &mut World, binding: &Throw, origin: [f64; 3], aim: [f64; 3]) -> f64 {
    let def = binding.def.as_def();
    let range = def.velocity * def.life_span;
    let end = std::array::from_fn(|i| origin[i] + aim[i] * range);
    match super::solid::cast(w, origin, end) {
        Some(hit) => {
            let flown = (0..3).map(|i| (hit.point[i] - origin[i]).powi(2)).sum::<f64>().sqrt();
            (flown / range).clamp(0.0, 1.0)
        }
        None => 1.0,
    }
}

struct CommittedEnergy;

impl combo::CombatEnergy for CommittedEnergy {
    fn balance(&self) -> f64 {
        0.0
    }
    fn available(&self) -> f64 {
        0.0
    }
    fn pay(&mut self, _amount: f64) -> bool {
        true
    }
}

impl Hero {
    /// What a viewer is told of this hero's throw, or `None` for a class without one.
    pub(super) fn published_throw(&self, weapon: &MeleeWeapon) -> Option<crate::wire::thrown::Throw> {
        let binding = weapon.offhand.as_ref()?;
        let def = binding.def.as_def();
        let launch = self.flight.is_active().then(|| crate::wire::thrown::Launch {
            tick: self.launched as f64,
            origin: self.launch_origin,
            direction: self.flight.direction,
            speed: def.velocity,
            out_time: self.flight.out_time,
            return_time: self.flight.return_time,
            rotation_rate: def.rotation_rate,
            release: weapon.pose.throw_hand_actor(),
        });
        Some(crate::wire::thrown::Throw { press: self.throw_anim_start.map(f64::from), launch })
    }

    pub(super) fn tick_throw(
        &mut self,
        w: &mut World,
        slot: usize,
        weapon: &mut MeleeWeapon,
        tick: u32,
    ) {
        let pressed = std::mem::take(&mut self.throw_pressed);
        let Some(binding) = &weapon.offhand else {
            return;
        };
        let Some((owner, u)) = w
            .sim
            .store
            .unit
            .at(slot as u32)
            .filter(|(_, u)| u.alive && !u.dead && !u.removed && u.seat.is_none())
        else {
            self.flight = Default::default();
            self.launch_due = None;
            self.throw_anim_start = None;
            return;
        };
        let held = matches!(u.held, crate::gen::Nullable::Value(_));
        let UnitBody::Soldier(body) = &u.soldier else {
            return;
        };
        let source = contact::identity(owner, u);
        let infantry = w.units.infantry.as_mut().unwrap();
        let Some(life) = infantry.lives[slot]
            .as_mut()
            .filter(|l| l.generation == u.generation && l.slab == owner.gen)
        else {
            return;
        };
        if !held
            && pressed
            && !self.input_lock.contains(combo::InputLockMask::RELOAD)
            && !self.flight.is_active()
            && self.launch_due.is_none()
            && tick >= self.ready_at
        {
            let cost = binding.def.as_def().energy_drain;
            if life
                .movement
                .charge(cost, &infantry.classes[self.class].tuning)
            {
                let can_leave_to_idle = self.state.state != weapon.machine.idle_state
                    && weapon
                        .machine
                        .state(self.state.state)
                        .transitions
                        .iter()
                        .any(|tr| tr.target == weapon.machine.idle_state);
                if can_leave_to_idle {
                    self.state.enter_idle(&weapon.machine);
                    self.guard = None;
                    life.guard = None;
                    self.reaction = None;
                    self.input_lock = combo::InputLockMask::NONE;
                    self.entered_tick = tick;
                }
                self.launch_due = Some(tick.saturating_add(binding.windup));
                self.throw_anim_start = Some(u32::try_from(tick).unwrap_or(u32::MAX));
            }
        }
        if !self.flight.is_active() && self.launch_due.is_none_or(|at| tick < at) {
            if let Some(sample) = infantry
                .sample
                .as_mut()
                .filter(|s| s.id == owner.slot && s.generation == life.generation)
            {
                sample.energy = life.movement.energy;
                sample.blades_off = self.state.blades_off_mask;
                sample.combo = Some(self.state.state.0);
                sample.state_time = self.state.state_time;
                sample.anim_time = self.state.anim_time;
                if let Some(saber) = &mut sample.saber {
                    saber.state_hash = weapon.state_hashes[self.state.state.0 as usize];
                }
            }
            return;
        }
        let root = Root {
            position: body.position,
            yaw: body.aim_yaw,
        };
        let aim_pitch = body.aim_pitch;
        let posture = body.posture;
        let hand = weapon.pose.throw_hand(root);
        if self.launch_due.take().is_some() {
            let (sy, cy) = v8math::sin_cos(root.yaw);
            let (sp, cp) = v8math::sin_cos(aim_pitch);
            let aim = [sy * cp, sp, cy * cp];
            let origin = fire_point(root, aim, posture);
            let reach = free_reach(w, binding, origin, aim);
            let mut paid = CommittedEnergy;
            if !self.flight.launch_validated_reaching(&binding.def, origin, aim, &mut paid, reach) {
                self.throw_anim_start = None;
                return;
            }
            self.launched = tick;
            self.launch_origin = origin;
            let infantry = w.units.infantry.as_mut().unwrap();
            let life = infantry.lives[slot].as_ref().unwrap();
            if let Some(sample) = infantry
                .sample
                .as_mut()
                .filter(|s| s.id == owner.slot && s.generation == life.generation)
            {
                sample.energy = life.movement.energy;
                sample.blades_off = self.state.blades_off_mask;
                sample.combo = Some(self.state.state.0);
                sample.state_time = self.state.state_time;
                sample.anim_time = self.state.anim_time;
                if let Some(saber) = &mut sample.saber {
                    saber.state_hash = weapon.state_hashes[self.state.state.0 as usize];
                    saber.thrown = Some((self.flight.position, self.flight.spin_angle));
                }
            }
            return;
        }
        let step = self.flight.step_with_combo_validated(
            &binding.def,
            &weapon.machine,
            &mut self.state,
            hand,
            true,
            combo::COMBO_FIXED_DT,
        );
        if step.caught {
            self.ready_at = tick.saturating_add(binding.reload);
            self.throw_anim_start = None;
            if step.catch_entry.is_some() {
                self.entered_tick = tick;
            }
        }
        let infantry = w.units.infantry.as_mut().unwrap();
        let life = infantry.lives[slot].as_ref().unwrap();
        if let Some(sample) = infantry
            .sample
            .as_mut()
            .filter(|s| s.id == owner.slot && s.generation == life.generation)
        {
            sample.energy = life.movement.energy;
            sample.blades_off = self.state.blades_off_mask;
            sample.combo = Some(self.state.state.0);
            sample.state_time = self.state.state_time;
            sample.anim_time = self.state.anim_time;
            if let Some(saber) = &mut sample.saber {
                saber.state_hash = weapon.state_hashes[self.state.state.0 as usize];
                saber.thrown = self
                    .flight
                    .is_active()
                    .then_some((self.flight.position, self.flight.spin_angle));
            }
        }
        if let Some(sweep) = step.sweep {
            self.throw_contacts(w, owner, source, binding, weapon.effects, &sweep, tick);
        }
    }

    fn throw_contacts(
        &mut self,
        w: &mut World,
        owner: Ref<Unit>,
        source: contact::LifeId,
        binding: &Throw,
        effects: contact::ContactEffects,
        sweep: &combo::BladeSweep,
        tick: u32,
    ) {
        let Some(shooter) = w.sim.store.unit.get(owner) else {
            return;
        };
        let (team, human) = (shooter.team, shooter.human);
        let spec = binding.def.as_def();
        let packet = HitPacket {
            damage: spec.damage,
            scales: spec.damage_scales,
            shield: spec.shield_scale,
        };
        let cause = Cause::Throw {
            weapon: binding.weapon,
            launched: self.launched,
            returning: self.flight.last_sweep_phase == SaberThrowPhase::Returning,
        };
        for i in 0..w.sim.store.units.list.len() {
            let crate::gen::Member::Unit(target) = w.sim.store.units.list[i] else {
                continue;
            };
            if owner == target {
                continue;
            }
            let Some(u) = w
                .sim
                .store
                .unit
                .get(target)
                .filter(|u| contact::eligible(w, target, *u, team, human))
            else {
                continue;
            };
            let life = contact::identity(target, u);
            let Some((_, serial)) = w.units.infantry.as_ref().unwrap().observed
                [target.slot as usize]
                .filter(|(k, _)| *k == life)
            else {
                continue;
            };
            let hp = u.combat.health;
            let p = u.position();
            let yaw = match &u.soldier {
                UnitBody::Soldier(s) => s.aim_yaw,
                _ => 0.,
            };
            let scale = crate::units::f3::health_scale_in(&w.units, target.slot);
            let origin = self.flight.position;
            // This projectile's combat policy settles interception in its own solver.
            let result = crate::units::module::with_unit_in(w, target, |scene, body| {
                self.flight.resolve_hit_validated(
                    &binding.def,
                    sweep,
                    &MeleeTarget {
                        target_id: crate::combat::combo::TargetId::Life(serial),
                        position: [p.x, p.y, p.z],
                        aim_yaw: yaw,
                        health_scale: scale,
                        shapes: body.hit_shapes(target.slot, scene),
                        deflect: None,
                    },
                    None,
                )
            });
            if let Some(MeleeHitOutcome::Hit {
                hit_point,
                push_impulse,
                ..
            }) = result
            {
                let after = contact::damage_in(
                    w,
                    owner,
                    target,
                    packet,
                    origin,
                    hit_point,
                    push_impulse,
                    Strike {
                        target: life,
                        source: Some(source),
                        cause,
                        weapon: Some(contact::Weapon::MeleeBinding(binding.weapon)),
                        credit: None,
                        reflected: false,
                    },
                );
                let direction = combo::classify_direction(combo::relative_horizontal_angle_deg(
                    yaw,
                    [origin[0] - p.x, 0., origin[2] - p.z],
                ));
                contact::keep_contact(w.units.infantry.as_mut().unwrap(), &mut w.sim.events, Contact {
                    sequence: 0,
                    tick,
                    source: Some(source),
                    target: life,
                    cause,
                    outcome: Outcome::Hit,
                    point: hit_point,
                    direction,
                    reaction: None,
                    before: hp,
                    after,
                    energy_spent: 0.,
                    critical: false,
                }, Some((self.class, effects)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::infantry::{
        blade::BladePose, bot_input::BotInputs, contact::Contacts, Class, Infantry, Life,
        SaberSample, Sample,
    };
    use crate::movement::soldier::swbf2::{ControlSpeed, State as MoveState, Tuning};
    use bfe_pose::{
        character::{Archive, Character},
        Clip,
    };
    use three::Matrix4;

    struct ThrowFixture {
        world: Box<World>,
        slot: usize,
        owner: Ref<Unit>,
        hero: Hero,
        weapon: MeleeWeapon,
    }

    fn invented_tuning() -> Tuning {
        let speed = ControlSpeed {
            forward: 1.0,
            strafe: 1.0,
            turn: 1.0,
        };
        Tuning {
            stand: speed,
            crouch: speed,
            jump: speed,
            jet: speed,
            jetpack: None,
            sprint: speed,
            roll: speed,
            tumble: speed,
            sprint_accelerate_time: 0.4,
            energy_capacity: 80.0,
            energy_floor: 0.0,
            energy_restore: 12.0,
            energy_restore_idle: 18.0,
            energy_drain_sprint: 8.0,
            energy_min_sprint: 14.0,
            energy_cost_jump: 10.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 16.0,
            jump_height: 2.4,
            jump_forward_speed_factor: 1.1,
            jump_strafe_speed_factor: 0.9,
            roll_speed_factor: 1.2,
            roll_frames: 0,
            recover_from_tumble: false,
            getup_front_seconds: 1.1,
            getup_back_seconds: 1.3,
        }
    }

    fn invented_machine() -> combo::ComboMachine {
        let doc = serde_json::json!({
            "states": [
                {
                    "name": "IDLE",
                    "children": [
                        { "key": "Duration", "values": [0.0] }
                    ]
                },
                {
                    "name": "WARD_OPEN",
                    "children": [
                        { "key": "Duration", "values": [0.0] },
                        {
                            "key": "Deflect",
                            "children": [
                                { "key": "DeflectAngle", "values": [-100.0, 100.0] },
                                { "key": "EnergyCost", "values": [3.0] }
                            ]
                        },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]
                },
                {
                    "name": "WARD_LOCKED",
                    "children": [
                        { "key": "Duration", "values": [0.0] },
                        { "key": "InputLock", "values": [0.0, "All", "!Thrust"] },
                        {
                            "key": "Deflect",
                            "children": [
                                { "key": "DeflectAngle", "values": [-100.0, 100.0] },
                                { "key": "EnergyCost", "values": [3.0] }
                            ]
                        },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]
                },
                {
                    "name": "CATCH_SABRE",
                    "children": [
                        { "key": "Duration", "values": [12, "Frames"] },
                        { "key": "Transition", "values": ["IDLE"] }
                    ]
                }
            ]
        });
        let spec = combo::WeaponMeleeSpec::default();
        combo::ComboMachine::from_json_value(&doc, &spec, combo::COMBO_ANIM_FPS, |_| Some(0.4))
            .unwrap()
    }

    fn invented_fixture(start_energy: f64, exhausted: bool) -> ThrowFixture {
        let mut world = crate::battle::script::tests::combat_arena();
        let (owner, generation) = world
            .sim
            .store
            .unit
            .iter()
            .find(|(_, u)| u.alive && !u.dead && !u.removed && matches!(u.soldier, UnitBody::Soldier(_)))
            .map(|(r, u)| (r, u.generation))
            .unwrap();
        let slot = owner.slot as usize;
        if let Some(u) = world.sim.store.unit.get_mut(owner) {
            u.held = crate::gen::Nullable::Null;
            u.seat = None;
            if let UnitBody::Soldier(body) = &mut u.soldier {
                body.position = [12.0, 0.0, -8.0];
                body.aim_yaw = 0.0;
                body.aim_pitch = 0.0;
            }
        }
        let unit_json = serde_json::json!({
            "weapons": [{
                "base": "meleethrow",
                "class": "invented_throw_blade",
                "EnergyDrain": 37.0,
                "InitialSalvoDelay": 0.25,
                "ReloadTime": 0.30,
                "ordnance": {
                    "Velocity": 24.0,
                    "LifeSpan": 0.45,
                    "ReturnDuration": 0.30,
                    "RotationRate": 3.5,
                    "Damage": 145.0,
                    "DamageLength": "1.2 FromEdge",
                    "DamageWidth": "0.5 FromEdge"
                }
            }]
        });
        let unit_val =
            bfe_content::json::parse_bytes(&serde_json::to_vec(&unit_json).unwrap()).unwrap();
        let mut spec = combo::WeaponMeleeSpec::default();
        spec.edges[0].default_length = 1.4;
        spec.edges[0].default_width = 0.3;
        let offhand = bind(&unit_val, &spec).unwrap().unwrap();
        assert_eq!(offhand.windup, 15);
        assert!((offhand.windup_secs - 0.25).abs() < 1e-9);

        let root_m = Matrix4::default().elements;
        let mut grip_m = root_m;
        grip_m[12] = -0.10;
        grip_m[13] = 1.05;
        grip_m[14] = 0.05;
        let actor_json = serde_json::json!({
            "parts": [],
            "bones": [
                { "name": "base", "parent": -1, "local": root_m },
                { "name": "grip", "parent": 0, "local": grip_m }
            ]
        });
        let actor = bfe_content::actor::build_actor(
            &bfe_content::json::parse_bytes(&serde_json::to_vec(&actor_json).unwrap()).unwrap(),
            &[],
        )
        .unwrap();
        let meta = bfe_content::json::parse_bytes(br#"{"fps":20}"#).unwrap();
        let bank = Archive::new(&meta, &[]).unwrap();
        let rig = Character::bind("test_hero", &Value::Null, &[], &actor, &bank, &bank)
            .unwrap()
            .rig;
        let joint = rig.joint("grip").unwrap();
        // Clip interpolates grip to (-0.31, 1.42, 0.48) at t = 0.25 s (InitialSalvoDelay).
        let clip = Clip {
            frames: 2,
            fps: 2.0,
            root_motion: false,
            tracks: vec![joint as i32],
            data: vec![
                0., 0., 0., 1., -0.10, 1.05, 0.05,
                0., 0., 0., 1., -0.52, 1.79, 0.91,
            ],
        };
        let pose = BladePose::from_throw_clip(rig, joint, Some(clip), offhand.windup_secs);
        let machine = invented_machine();
        let state_hashes: Vec<u32> = machine
            .state_names
            .iter()
            .map(|s| combo::zero_fnv1a_lower(s))
            .collect();
        let weapon = MeleeWeapon {
            weapon: combo::zero_fnv1a_lower("invented_saber"),
            effects: Default::default(),
            state_hashes,
            machine,
            spec,
            pose,
            offhand: Some(offhand),
        };
        let capacity = (world.sim.store.unit.slots() as usize).max(slot + 1) + 1;
        let tuning = invented_tuning();
        let mut movement = MoveState::new(&tuning);
        movement.energy = start_energy;
        movement.exhausted = exhausted;
        let mut lives = vec![None; capacity];
        lives[slot] = Some(Life {
            slab: owner.gen,
            class: 0,
            generation,
            movement,
            allow_fire: true,
            allow_reload: true,
            offhand_pressed: false,
            bot_secondary_held: None,
            guard: None,
            lock_pressed: false,
            power_input: Default::default(),
            energy_before: start_energy,
            power_thrust: 1.0,
            blast_pending: false,
            offhand_gun_salvo: false,
            offhand_gun_anim: None,
        });
        let hero = Hero::new(0, &weapon.machine, 0);
        let sample = Sample {
            id: owner.slot,
            generation,
            energy: start_energy,
            sprint: false,
            roll: None,
            roll_seconds: 0.0,
            jump_count: 0,
            land_cooldown: 0.0,
            tumble: None,
            tumble_state: None,
            combo: Some(0),
            state_time: 0.0,
            anim_time: 0.0,
            blades_off: 0,
            turn_factor: 1.0,
            swing_turn: 0.0,
            exhausted: false,
            sprint_time: None,
            combo_state: None,
            buttons_armed: 0,
            saber_locked: false,
            saber: Some(SaberSample {
                thrown: None,
                weapon_hash: weapon.weapon,
                first_omitted_attack: None,
                combo_hash: weapon.machine.name_hash,
                reaction: None,
                state_hash: weapon.state_hashes[0],
            }),
        };
        world.units.infantry = Some(Box::new(Infantry {
            power_kits: vec![None],
            classes: vec![Class {
                name: crate::store::StrId(1),
                registry_id: None,
                tuning,
                acceleration: None,
                offhand_guns: Vec::new(),
            }],
            lives,
            bot_inputs: BotInputs::new(capacity),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes: (0..capacity).map(|_| None).collect(),
            sample: Some(sample),
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; capacity],
            life_serial: 0,
            tick: 0,
        }));
        ThrowFixture {
            world,
            slot,
            owner,
            hero,
            weapon,
        }
    }

    impl ThrowFixture {
        fn step(&mut self, tick: u32, press: bool) {
            self.hero.throw_pressed = press;
            self.world.units.infantry.as_mut().unwrap().tick = tick.into();
            self.hero
                .tick_throw(&mut self.world, self.slot, &mut self.weapon, tick.into());
        }

        fn energy(&self) -> f64 {
            self.world.units.infantry.as_ref().unwrap().lives[self.slot]
                .as_ref()
                .unwrap()
                .movement
                .energy
        }

        fn exhausted(&self) -> bool {
            self.world.units.infantry.as_ref().unwrap().lives[self.slot]
                .as_ref()
                .unwrap()
                .movement
                .exhausted
        }

        fn sample(&self) -> Sample {
            self.world.units.infantry.as_ref().unwrap().sample.unwrap()
        }

        fn set_held(&mut self, held: bool) {
            let u = self.world.sim.store.unit.get_mut(self.owner).unwrap();
            u.held = if held {
                crate::gen::Nullable::Value(crate::gen::Held {
                    prop: Ref::new(0, 1),
                    since: 0.0,
                    frame: crate::gen::Nullable::Undefined,
                    time: None,
                })
            } else {
                crate::gen::Nullable::Null
            };
        }
    }

    #[test]
    fn throw_charges_energy_at_press_waits_for_windup_launches_from_throw_clip_and_keeps_blades_lit()
    {
        let mut f = invented_fixture(80.0, false);
        f.step(10, true);
        // Energy is charged at trigger press, not deferred until release.
        assert!((f.energy() - 43.0).abs() < 1e-9);
        assert!((f.sample().energy - 43.0).abs() < 1e-9);
        assert!(!f.hero.flight.is_active(), "must wait for 15-tick windup");
        assert_eq!(f.hero.throw_anim_start, Some(10));
        assert_eq!(f.sample().blades_off, 0);

        // Remain in windup for ticks 11..=24 with no second energy debit.
        for t in 11..=24u32 {
            f.step(t, false);
            assert!(!f.hero.flight.is_active(), "launched early at tick {t}");
            assert!((f.energy() - 43.0).abs() < 1e-9);
            assert_eq!(f.sample().blades_off, 0);
        }

        // Tick 25 (10 + 15) spawns at the soldier's fire point (standing, level: 0.06 m right,
        // 1.6 m up, 0.25 m ahead) without stepping early.
        f.step(25, false);
        assert!(f.hero.flight.is_active());
        assert!((f.energy() - 43.0).abs() < 1e-9, "must not debit again at launch");
        let expected_origin = [12.0 - 0.06, 1.6, -8.0 + 0.25];
        // The blade comes back to the throw clip's grip at t = 0.25 s, carried with the soldier.
        let release = [12.0 - 0.31, 1.42, -8.0 + 0.48];
        for i in 0..3 {
            assert!((f.hero.flight.prev_position[i] - expected_origin[i]).abs() < 1e-6);
            assert!((f.hero.flight.position[i] - expected_origin[i]).abs() < 1e-6);
        }
        assert_eq!(f.sample().blades_off, 0);
        assert!(f.sample().saber.unwrap().thrown.is_some());

        // Tick 26 advances the opening outbound flight step from expected_origin along +Z.
        f.step(26, false);
        assert!(f.hero.flight.is_active());
        assert!((f.hero.flight.position[2] - (expected_origin[2] + 24.0 / 60.0)).abs() < 1e-6);

        // Fly through outbound (27 ticks) and full return (18 ticks, no early 0.75 m cutoff)
        // into CATCH_SABRE without ever turning blades off.
        let catch_id = f.weapon.machine.catch_state.unwrap().0;
        let mut caught_at = None;
        let mut last_active_pos = f.hero.flight.position;
        for t in 27..=140u32 {
            f.step(t, false);
            assert_eq!(f.hero.state.blades_off_mask, 0);
            assert_eq!(f.sample().blades_off, 0);
            if f.hero.flight.is_active() {
                last_active_pos = f.hero.flight.position;
            } else {
                caught_at = Some(t);
                assert_eq!(f.hero.state.state.0, catch_id);
                assert_eq!(f.sample().combo, Some(catch_id));
                assert!(f.sample().saber.unwrap().thrown.is_none());
                break;
            }
        }
        assert_eq!(caught_at, Some(25 + 27 + 18), "must fly full outbound + return duration");
        let last_dist = (0..3)
            .map(|i| {
                let d = last_active_pos[i] - release[i];
                d * d
            })
            .sum::<f64>()
            .sqrt();
        assert!(
            last_dist < 0.75,
            "returning saber must not catch 0.75 m early (last active dist={last_dist:.3})"
        );
        // The catch tick does not move the blade: it ends one step short of the release point.
        assert_eq!(f.hero.flight.position, last_active_pos);
        assert!(last_dist > 0.1, "the blade stops a step short of the release point");
    }

    #[test]
    fn a_thrown_blade_leaves_from_the_soldiers_fire_point_not_from_the_hand() {
        // (posture, yaw, pitch) -> where the blade starts, from the game's aimer constants: the
        // aim origin of the posture, then along the aim, to the right and along aim x left.
        let cases = [
            (crate::gen::Posture::Stand, 0.0, 0.0, [11.94, 1.6, -7.75]),
            (crate::gen::Posture::Stand, 0.0, -0.5, [11.94, 1.492_385_359_159_912, -7.828_546_913_387_827]),
            (crate::gen::Posture::Crouch, 0.7, 0.3, [11.995_114_851_890_35, 1.226_113_227_209_054_5, -7.664_300_380_261_348]),
        ];
        for (posture, yaw, pitch, want) in cases {
            let mut f = invented_fixture(80.0, false);
            if let Some(u) = f.world.sim.store.unit.get_mut(f.owner) {
                if let UnitBody::Soldier(body) = &mut u.soldier {
                    body.posture = posture;
                    body.aim_yaw = yaw;
                    body.aim_pitch = pitch;
                }
            }
            f.step(10, true);
            for t in 11..=25u32 {
                f.step(t, false);
            }
            assert!(f.hero.flight.is_active(), "{posture:?}");
            for i in 0..3 {
                assert!((f.hero.launch_origin[i] - want[i]).abs() < 1e-9, "{posture:?} {yaw} {pitch}: {:?} vs {want:?}", f.hero.launch_origin);
                assert!((f.hero.flight.position[i] - want[i]).abs() < 1e-9);
            }
            let (sy, cy) = v8math::sin_cos(yaw);
            let (sp, cp) = v8math::sin_cos(pitch);
            let aim = [sy * cp, sp, cy * cp];
            for i in 0..3 {
                assert!((f.hero.flight.direction[i] - aim[i]).abs() < 1e-12, "it flies along the aim");
            }
            // The point it comes back to stays the throw clip's grip, turned with the soldier.
            let launch = f.hero.published_throw(&f.weapon).unwrap().launch.unwrap();
            for (got, want) in launch.release.into_iter().zip([-0.31, 1.42, 0.48]) {
                assert!((got - want).abs() < 1e-6, "release {got} vs {want}");
            }
            assert_eq!(launch.origin, f.hero.launch_origin);
        }
    }

    #[test]
    fn the_published_throw_follows_the_press_the_launch_and_the_catch() {
        let mut f = invented_fixture(80.0, false);
        assert_eq!(f.hero.published_throw(&f.weapon), Some(crate::wire::thrown::Throw::IDLE));
        f.step(10, true);
        let pressed = f.hero.published_throw(&f.weapon).unwrap();
        assert_eq!(pressed.press, Some(10.0));
        assert!(pressed.launch.is_none(), "nothing flies during the windup");
        for t in 11..=24u32 {
            f.step(t, false);
            assert_eq!(f.hero.published_throw(&f.weapon), Some(pressed));
        }
        f.step(25, false);
        let out = f.hero.published_throw(&f.weapon).unwrap();
        let launch = out.launch.expect("the blade is out");
        assert_eq!(out.press, Some(10.0));
        assert_eq!(launch.tick, 25.0);
        assert_eq!(launch.origin, f.hero.launch_origin);
        assert_eq!(launch.direction, [0.0, 0.0, 1.0]);
        assert_eq!((launch.speed, launch.rotation_rate), (24.0, 3.5));
        assert!((launch.out_time - 0.45).abs() < 1e-12 && (launch.return_time - 0.30).abs() < 1e-12);
        for (got, want) in launch.release.into_iter().zip([-0.31, 1.42, 0.48]) {
            assert!((got - want).abs() < 1e-6, "release {got} vs {want}");
        }
        // Constant from the launch to the catch: the codec sends it once.
        let mut caught = false;
        for t in 26..=140u32 {
            f.step(t, false);
            let now = f.hero.published_throw(&f.weapon).unwrap();
            if f.hero.flight.is_active() {
                assert_eq!(now, out, "tick {t}");
            } else {
                assert_eq!(now, crate::wire::thrown::Throw::IDLE, "after the catch");
                caught = true;
                break;
            }
        }
        assert!(caught);
        // A class without a throw publishes nothing.
        let mut bare = invented_fixture(80.0, false);
        bare.weapon.offhand = None;
        assert!(bare.hero.published_throw(&bare.weapon).is_none());
    }

    #[test]
    fn a_throw_into_the_ground_flies_and_returns_for_the_share_of_the_range_it_has() {
        let mut f = invented_fixture(80.0, false);
        if let Some(u) = f.world.sim.store.unit.get_mut(f.owner) {
            if let UnitBody::Soldier(body) = &mut u.soldier {
                body.aim_pitch = -0.5;
            }
        }
        f.step(10, true);
        for t in 11..=25u32 {
            f.step(t, false);
        }
        assert!(f.hero.flight.is_active());
        let (life, back) = (0.45, 0.30);
        let reach = f.hero.flight.out_time / life;
        assert!(reach > 0.05 && reach < 0.6, "the ground ends the range early: reach {reach}");
        // The ray meets the ground where the hand's height says it does.
        let drop = f.hero.launch_origin[1] - 0.0;
        let hit_distance = drop / 0.5f64.sin();
        assert!(
            (f.hero.flight.out_time * 24.0 - hit_distance).abs() < 0.15,
            "the blade stops at the ground: {} m of {hit_distance:.2} m",
            f.hero.flight.out_time * 24.0
        );
        assert!(
            (f.hero.flight.return_time - back * reach).abs() < 1e-12,
            "the return time shrinks by the same share"
        );
        // The blade is caught again after exactly its out time plus its return time.
        let end = 25 + (f.hero.flight.out_time * 60.0).ceil() as u32
            + (f.hero.flight.return_time * 60.0).ceil() as u32;
        let mut caught = None;
        for t in 26..=140u32 {
            f.step(t, false);
            if !f.hero.flight.is_active() {
                caught = Some(t);
                break;
            }
        }
        let caught = caught.expect("the throw ends");
        assert!(caught.abs_diff(end) <= 2, "caught at {caught}, expected about {end}");
        assert!(caught < 25 + 27 + 18, "shorter than a throw with the whole range");
    }

    #[test]
    fn throw_uses_exhausted_latch_allowing_single_overdraw() {
        // Below cost (14 < 37) but unexhausted: single overdraw succeeds and sets exhausted.
        let mut f = invented_fixture(14.0, false);
        f.step(10, true);
        assert_eq!(f.energy(), 0.0);
        assert!(f.exhausted());
        assert_eq!(f.hero.launch_due, Some(25));

        // Exhausted latch set with partial stamina (20 < 37): press is refused.
        let mut g = invented_fixture(20.0, true);
        g.step(10, true);
        assert_eq!(g.energy(), 20.0);
        assert!(g.exhausted());
        assert!(g.hero.launch_due.is_none());
        assert!(g.hero.throw_anim_start.is_none());
    }

    #[test]
    fn throw_respects_reload_input_lock_and_exits_unlocked_guard_to_idle() {
        // Case 1: unlocked guard (no InputLockMask::RELOAD) with transition to IDLE exits guard and throws.
        let mut open = invented_fixture(80.0, false);
        let ward_open = open.weapon.machine.find_state("WARD_OPEN").unwrap();
        open.hero.state.state = ward_open;
        open.hero.guard = open.weapon.machine.state(ward_open).deflect;
        open.hero.input_lock = combo::InputLockMask::NONE;
        open.world.units.infantry.as_mut().unwrap().lives[open.slot]
            .as_mut()
            .unwrap()
            .guard = open.hero.guard;
        open.step(20, true);
        assert_eq!(open.hero.state.state, open.weapon.machine.idle_state);
        assert!(open.hero.guard.is_none());
        assert!(
            open.world.units.infantry.as_ref().unwrap().lives[open.slot]
                .as_ref()
                .unwrap()
                .guard
                .is_none()
        );
        assert_eq!(open.hero.launch_due, Some(35));
        assert!((open.energy() - 43.0).abs() < 1e-9);

        // Case 2: guard with InputLockMask::RELOAD refuses the throw and keeps guard raised.
        let mut locked = invented_fixture(80.0, false);
        let ward_locked = locked.weapon.machine.find_state("WARD_LOCKED").unwrap();
        locked.hero.state.state = ward_locked;
        locked.hero.guard = locked.weapon.machine.state(ward_locked).deflect;
        locked.hero.input_lock = combo::InputLockMask::RELOAD;
        locked.world.units.infantry.as_mut().unwrap().lives[locked.slot]
            .as_mut()
            .unwrap()
            .guard = locked.hero.guard;
        locked.step(20, true);
        assert_eq!(locked.hero.state.state, ward_locked);
        assert!(locked.hero.guard.is_some());
        assert!(locked.hero.launch_due.is_none());
        assert!((locked.energy() - 80.0).abs() < 1e-9);
    }

    #[test]
    fn choke_blocks_new_throw_without_cancelling_active_windup_or_flight() {
        // 1. Already choked: new press is refused with no energy debit.
        let mut blocked = invented_fixture(80.0, false);
        blocked.set_held(true);
        blocked.step(10, true);
        assert!(blocked.hero.launch_due.is_none());
        assert!(blocked.hero.throw_anim_start.is_none());
        assert!((blocked.energy() - 80.0).abs() < 1e-9);

        // 2. Choked mid-windup: windup is NOT cancelled and still launches on tick 25.
        let mut windup = invented_fixture(80.0, false);
        windup.step(10, true);
        assert_eq!(windup.hero.launch_due, Some(25));
        windup.set_held(true);
        for t in 11..=25u32 {
            windup.step(t, false);
        }
        assert!(
            windup.hero.flight.is_active(),
            "choke during windup must not cancel the throw"
        );
        assert_eq!(windup.hero.flight.phase, SaberThrowPhase::Outbound);

        // 3. Choked mid-outbound flight: outbound flight is NOT recalled early.
        let mut flight = invented_fixture(80.0, false);
        flight.step(10, true);
        for t in 11..=27u32 {
            flight.step(t, false);
        }
        assert_eq!(flight.hero.flight.phase, SaberThrowPhase::Outbound);
        flight.set_held(true);
        flight.step(28, false);
        assert_eq!(
            flight.hero.flight.phase,
            SaberThrowPhase::Outbound,
            "choke during outbound flight must not force early return"
        );
    }
}
