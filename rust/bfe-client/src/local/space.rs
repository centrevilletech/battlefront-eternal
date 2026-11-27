//! Local display publications and device inputs for a 2005 space battle.
pub(crate) mod instruments;
use crate::generated_input::{
    ClientAction, ClientCommand, ClientFlightAction, ClientFlightCommand, ClientFlightHeld,
    ClientHeld, ClientLook,
};
use bfe_sim::space::{
    flight::{SpaceFlightState, SpacePilotInput},
    local::PilotControls,
    SpaceBattlefield,
};
use three::{Matrix4, Quaternion, Vector3};

pub(crate) fn controls(
    command: &ClientCommand,
    look: &ClientLook,
    flight: &ClientFlightCommand,
    eject_held: bool,
) -> PilotControls {
    let held = |key: ClientFlightHeld| flight.held & (1 << key as u8) != 0;
    let action = |key: ClientFlightAction| flight.actions & (1 << key as u8) != 0;
    let axis = |plus, minus, pad: f64| {
        let keys = i8::from(held(plus)) - i8::from(held(minus));
        if keys != 0 {
            f64::from(keys)
        } else if pad.abs() > 0.12 {
            pad
        } else {
            0.
        }
    };
    let roll = axis(
        ClientFlightHeld::RollRight,
        ClientFlightHeld::RollLeft,
        flight.pad_axes[2],
    );
    let fire = |key: ClientHeld| command.held & (1 << key as u8) != 0;
    // Turret look uses the device's ground sensitivity and inversion.
    let scale = command.mouse_scale / 2048.;
    PilotControls {
        turret_look: [-look.x * scale, look.y * scale * if command.invert == Some(true) {1.} else {-1.}],
        turret_turn: f64::from(i8::from(fire(ClientHeld::TurnLeft)) - i8::from(fire(ClientHeld::TurnRight))),
        cycle_weapon: i8::from(command.actions & (1 << ClientAction::NextPrimary as u8) != 0)
            - i8::from(command.actions & (1 << ClientAction::PreviousPrimary as u8) != 0),
        terminal_cycle: 0,
        use_pressed: command.actions & (1 << ClientAction::Use as u8) != 0,
        mouse: [
            look.x * flight.scale / 120.,
            look.y * flight.scale / 120. * if flight.invert { -1. } else { 1. },
        ],
        flight: SpacePilotInput {
            throttle: axis(
                ClientFlightHeld::Throttle,
                ClientFlightHeld::Brake,
                -flight.pad_axes[3],
            ),
            stick_x: if flight.pad_axes[0].abs() > 0.12 {
                flight.pad_axes[0]
            } else {
                0.
            },
            stick_y: if flight.pad_axes[1].abs() > 0.12 {
                -flight.pad_axes[1]
            } else {
                0.
            },
            roll,
            roll_lock: held(ClientFlightHeld::RollLock),
            boost: held(ClientFlightHeld::Boost) || flight.pad_buttons & (1 << 11) != 0,
            eject_held,
            takeoff_or_land: action(ClientFlightAction::Takeoff),
            trick_roll: if action(ClientFlightAction::Trick) {
                if roll > 0.1 {
                    1
                } else if roll < -0.1 {
                    -1
                } else {
                    0
                }
            } else {
                0
            },
            trick_flip: if action(ClientFlightAction::Immelmann) {
                2
            } else {
                0
            },
            fire_primary: fire(ClientHeld::Primary) || command.primary_count > 0,
            fire_secondary: fire(ClientHeld::Secondary) || command.secondary_count > 0,
            lock_target: action(ClientFlightAction::TargetLock),
            ..Default::default()
        },
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pose {
    pub position: [f64; 3],
    pub rotation: [f64; 4],
    pub bank: f64,
    pub gear: f64,
}

impl Pose {
    pub fn unbanked_quaternion(&self, next: &Self, alpha: f64) -> Quaternion {
        let q = |[x, y, z, w]: [f64; 4]| Quaternion::new(x, y, z, w);
        let mut rotation = q(self.rotation);
        rotation.slerp(&q(next.rotation), alpha);
        rotation
    }

    pub fn matrix(&self, next: &Self, alpha: f64) -> Matrix4 {
        let mut rotation = self.unbanked_quaternion(next, alpha);
        let bank = self.bank + alpha * (next.bank - self.bank);
        let mut visual = Quaternion::default();
        visual.set_from_axis_angle(&Vector3::new(0., 0., 1.), bank);
        rotation.multiply(&visual);
        let [x, y, z] = std::array::from_fn(|i| {
            self.position[i] + alpha * (next.position[i] - self.position[i])
        });
        let mut m = Matrix4::default();
        m.compose(&Vector3::new(x, y, z), &rotation, &Vector3::new(1., 1., 1.));
        m
    }
}

/// Precomputed class and instance hashes for 2005 space audio (built once at admission).
#[derive(Debug, Default)]
pub(crate) struct AudioClasses {
    pub fighters: Vec<u32>,
    pub fighter_weapons: Vec<[u32; 5]>,
    pub building_classes: Vec<(u32, u32)>,
    pub turret_weapons: Vec<u32>,
    pub doors: Vec<u32>,
}

impl AudioClasses {
    pub fn from_space(space: &SpaceBattlefield) -> Self {
        use audio_control::swbf2::zero_hash;
        Self {
            fighters: space.profiles.iter().map(|p| zero_hash(&p.class_name)).collect(),
            fighter_weapons: space
                .profiles
                .iter()
                .map(|p| {
                    std::array::from_fn(|i| {
                        p.weapons.get(i).map_or(0, |w| zero_hash(&w.name))
                    })
                })
                .collect(),
            building_classes: space
                .buildings
                .iter()
                .map(|b| (zero_hash(&b.class), zero_hash(&b.name)))
                .collect(),
            turret_weapons: space
                .buildings
                .iter()
                .map(|b| {
                    b.turret
                        .as_ref()
                        .map_or(0, |t| zero_hash(&t.weapon.name))
                })
                .collect(),
            doors: space.doors.iter().map(|d| zero_hash(&d.class)).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HullWeapon {
    pub class: u32,
    pub fired: bool,
    pub firing: bool,
    pub spinning: bool,
    pub overheated: bool,
    pub heat: f32,
    pub heat_threshold: f32,
    pub reloading: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hull {
    pub id: usize,
    pub generation: u32,
    pub class: u32,
    pub team: u8,
    pub previous: Pose,
    pub current: Pose,
    pub health: f64,
    pub max_health: f64,
    pub max_speed: f64,
    pub boost_speed: f64,
    pub thrusting: bool,
    pub piloted: bool,
    pub trick: bfe_sim::space::flight::SpaceTrick,
    pub velocity: [f64; 3],
    pub state: SpaceFlightState,
    pub weapon_count: u8,
    pub weapons: [HullWeapon; 5],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Moving {
    pub instance: usize,
    pub previous: [f64; 16],
    pub current: [f64; 16],
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Aim {
    pub building: usize,
    pub class: u32,
    pub weapon: u32,
    pub position: [f64; 3],
    pub previous: [f64; 2],
    pub current: [f64; 2],
    pub yaw_rate: f32,
    pub pitch_rate: f32,
    pub alive: bool,
    pub held: bool,
    pub fired: bool,
    pub firing: bool,
    pub spinning: bool,
    pub overheated: bool,
    pub heat: f32,
    pub heat_threshold: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Chair {
    pub instance: usize,
    pub building: usize,
    pub class: u32,
    pub matrix: [f64; 16],
    pub center: [f64; 3],
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub fighters: usize,
    pub props: usize,
    pub turrets: usize,
    pub buildings: usize,
    pub rounds: usize,
}

impl Counts {
    pub fn of(space: &SpaceBattlefield) -> Self {
        Self {
            fighters: space.fighters.len(),
            buildings: space.buildings.len(),
            rounds: space.ordnance.slots.len(),
            props: space.world_anims.objects.len(),
            turrets: space
                .buildings
                .iter()
                .filter(|b| b.turret.is_some())
                .count(),
        }
    }

    /// The room an online battle's frames reserve: its client data's craft,
    /// buildings, props and turrets, and every round the wire can carry.
    pub fn from_client_data(data: &bfe_sim::space::SpaceClientData) -> Self {
        let max_online_rounds = bfe_sim::wire::space::MAX_GUIDED
            + bfe_sim::wire::space::MAX_LAUNCHES
            + bfe_sim::wire::space::MAX_IMPACTS;
        Self {
            fighters: data.fighter_slots.max(bfe_sim::wire::space::MAX_FIGHTERS),
            buildings: data.buildings.len(),
            rounds: bfe_sim::space::combat::MAX_SPACE_PROJECTILES.max(max_online_rounds),
            props: data.world_anims.objects.len(),
            turrets: data.turret_buildings.len(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct System {
    pub building: usize,
    pub generation: u32,
    pub team: u8,
    pub alive: bool,
    pub health: f64,
    pub max_health: f64,
    pub shield: f64,
    pub max_shield: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Destruction {
    pub class: u32,
    pub name: u32,
    pub position: [f64; 3],
}

/// A soldier working a remote turret chair: the soldier, the chair's placed
/// instance and the chair's world matrix.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChairRider {
    pub unit: (u32, u32),
    pub instance: usize,
    pub matrix: [f64; 16],
}

#[derive(Debug, Default)]
pub struct Frame {
    pub(crate) tick: u32,
    /// When published from multi-tick online snapshots (e.g. 20 Hz), the previous
    /// snapshot tick whose poses sit in `previous`; `None` for 60 Hz local frames.
    pub(crate) prev_tick: Option<u32>,
    pub(crate) mount: Option<usize>,
    pub(crate) mount_seat: u8,
    pub(crate) cockpit: Option<instruments::Cockpit>,
    pub(crate) weapon_changed: [Option<u32>; 2],
    pub(crate) labels: Option<std::sync::Arc<instruments::Names>>,
    pub(crate) radar: [f64; 3],
    pub(crate) points_visible: [bool; 2],
    pub(crate) mission: Option<bfe_sim::battle::script::MissionScore>,
    pub(crate) gunner_aim: Option<[[f64;3];2]>,
    /// The guided rocket the player is steering from this seat, if one flies:
    /// its position at the previous and this tick, and its velocity.
    pub(crate) own_rocket: Option<[[f64; 3]; 3]>,
    /// While the craft the player sits in (any seat) is outside the battlefield:
    /// the seconds left on its out-of-bounds timer (6.5 from leaving, the craft
    /// lost at 0). The 2005 HUD shows its "Leaving Battlefield" warning
    /// (`game.tooltips.leaving_battlefield`) for as long as this is set.
    pub out_of_bounds: Option<f64>,
    /// The same for the guided rocket the player steers from this seat (its
    /// own 2.17 s timer); the warning shows while either is set.
    pub rocket_out_of_bounds: Option<f64>,
    /// The craft a lock is building on or holds: slot, life, channel, progress.
    pub(crate) fighter_lock: Option<(usize, u32, u8, f64)>,
    pub(crate) chair: Option<Chair>,
    pub(crate) hulls: Vec<Hull>,
    pub(crate) hidden_crew: Vec<(u32, u32)>,
    pub(crate) chair_riders: Vec<ChairRider>,
    pub(crate) rounds: Vec<(usize, bfe_sim::space::observations::Round)>,
    pub(crate) deaths: Vec<Destruction>,
    pub(crate) respawns: Vec<u32>,
    pub(crate) door_events: Vec<audio_control::swbf2::vehicles::DoorEvent>,
    pub(crate) systems: Vec<System>,
    pub(crate) props: Vec<Moving>,
    pub(crate) turrets: Vec<Aim>,
}

/// The seconds left before a craft outside the battlefield is lost: its
/// out-of-bounds timer while that runs (at or above 0), none inside (-1).
fn out_of_bounds_left(timer: f64) -> Option<f64> {
    (timer >= 0.0).then_some(timer)
}

impl Frame {
    pub(crate) fn system(&self, building: usize) -> Option<&System> {
        let i = self.systems.binary_search_by_key(&building, |s| s.building).ok()?;
        Some(&self.systems[i])
    }

    pub(crate) fn reserved(size: Counts) -> Self {
        Self {
            hulls: Vec::with_capacity(size.fighters),
            hidden_crew: Vec::with_capacity(size.fighters * 6 + 64),
            chair_riders: Vec::with_capacity(size.buildings),
            systems: Vec::with_capacity(size.buildings),
            rounds: Vec::with_capacity(size.rounds),
            deaths: Vec::with_capacity(if size.fighters > 0 {
                size.buildings + bfe_sim::space::MAX_SPACE_EVENTS
            } else { 0 }),
            respawns: Vec::with_capacity(if size.fighters > 0 { 32 } else { 0 }),
            door_events: Vec::with_capacity(if size.fighters > 0 { 32 } else { 0 }),
            props: Vec::with_capacity(size.props),
            turrets: Vec::with_capacity(size.turrets),
            ..Default::default()
        }
    }
    /// Project the locked fighter's current life on the same clock as its mesh.
    pub(crate) fn draw_lock(
        &self,
        alpha: f64,
        view: &crate::scene::View,
        bridge: &mut crate::render_bridge::Bridge,
    ) {
        use bfe_render::hud_model::layout::HudFrame as H;
        if self.mount.is_none() {
            return;
        }
        let x = &mut bridge.hud_model.input;
        x[H::MARKER] = 0.;
        let Some((slot, life, channel, progress)) = self.fighter_lock else { return };
        let Some(target) = self.hulls.iter().find(|f| f.id == slot && f.generation == life) else {
            return;
        };
        let at = std::array::from_fn::<_, 3, _>(|i| {
            target.previous.position[i] + alpha * (target.current.position[i] - target.previous.position[i])
        });
        let mut p = Vector3::new(at[0], at[1], at[2]);
        let mut matrix = Matrix4::default();
        matrix.elements.copy_from_slice(&view.values[16..32]);
        p.apply_matrix4(&matrix);
        if p.z >= 0. {
            return;
        }
        matrix.elements.copy_from_slice(&view.values[..16]);
        p.apply_matrix4(&matrix);
        if (-1. ..=1.).contains(&p.z) {
            x[H::MARKER] = 1.;
            x[H::MARKER_X] = (p.x + 1.) * 0.5;
            x[H::MARKER_Y] = (1. - p.y) * 0.5;
            x[H::MARKER_PROGRESS] = progress;
            x[H::LOCK_CHANNEL] = f64::from(channel);
        }
    }

    pub(crate) fn placement(&self, instance: usize, alpha: f64) -> Option<[f64; 16]> {
        let p = &self.props[self
            .props
            .binary_search_by_key(&instance, |p| p.instance)
            .ok()?];
        Some(bfe_sim::space::world_anim::interpolate_transform(
            p.previous, p.current, alpha,
        ))
    }

    pub(crate) fn write(&mut self, world: &bfe_sim::world::owner::World, space: &SpaceBattlefield, audio: &AudioClasses, labels: &Option<std::sync::Arc<instruments::Names>>, player: u32) {
        use bfe_sim::battle::rules::{awards::SystemTransition, events::{Event, ObjectHandle}};
        use bfe_sim::space::{objects::SpaceTurretPilotType, SpaceEvent};
        let mission = world.mission_view();
        self.labels.clone_from(labels);
        let bounds = &space.bounds;
        self.radar = [bounds.center[0], bounds.center[2], ((bounds.max[0]-bounds.min[0]).max(bounds.max[2]-bounds.min[2]) * 0.5).max(1.)];
        self.points_visible = std::array::from_fn(|i| mission.as_ref().is_some_and(|m| m.host.team_points_visible[i+1]));
        self.mission = mission.as_ref().map(|m| m.score());
        self.cockpit = instruments::Cockpit::read(world, space, player);
        self.systems.clear();
        if let (Some(names), Some(view)) = (labels, mission.as_ref().filter(|m|
            m.host.phase == bfe_sim::battle::script::ScriptPhase::Tick)) {
            let hud = names.mission.get_or_init(|| instruments::MissionHud::new(world, view.host));
            self.systems.extend(hud.systems.iter().filter_map(|system| {
                let state = world.space_building_snapshot(system.building)?;
                Some(System {
                    building: system.building,
                    generation: state.generation,
                    team: state.team as u8,
                    alive: state.alive,
                    health: state.health,
                    max_health: state.max_health,
                    shield: state.cur_shield,
                    max_shield: state.max_shield,
                })
            }));
            self.systems.sort_unstable_by_key(|s| s.building);
        }
        self.tick = space.tick;
        self.prev_tick = None;
        let pilot = space.pilot_fighter(player);
        self.mount = pilot.map(|f| f.id);
        let seat = space.pilot_seat(player).map_or(0, |r| r.seat);
        self.mount_seat = seat;
        self.gunner_aim = space.pilot_gunner_aim(player);
        self.out_of_bounds = pilot.and_then(|f| out_of_bounds_left(f.oob_timer));
        let steered = pilot.and_then(|f| {
            space.ordnance.slots.iter().find(|p| {
                p.active && p.guided_pilot_fighter == Some((f.id, f.generation, seat))
            })
        });
        self.own_rocket = steered.map(|p| [p.previous_position, p.position, p.velocity]);
        self.rocket_out_of_bounds = steered.and_then(|p| out_of_bounds_left(p.guided_out_of_bounds));
        self.fighter_lock = pilot.and_then(|f| {
            let lock = space.profiles[f.profile_index].weapons.iter().enumerate()
                .filter(|(_,w)| w.seat == seat)
                .map(|(i,_)| f.weapon_locks[i])
                .find(|l| l.kind == bfe_sim::space::flight::SpaceLockTargetKind::Fighter)?;
            if lock.kind != bfe_sim::space::flight::SpaceLockTargetKind::Fighter {
                return None;
            }
            let weapon = space.profiles[f.profile_index].weapons.get(lock.weapon_index as usize)?;
            let progress = if lock.locked {
                1.
            } else if weapon.lock_time > 0. {
                (lock.progress / weapon.lock_time).clamp(0., 1.)
            } else {
                0.
            };
            Some((lock.target_index, u32::try_from(lock.target_generation).ok()?, weapon.channel, progress))
        });
        self.chair = space.pilot_terminal(player).map(|(_, gun)| Chair {
            instance: gun.instance, building: gun.building_index,
            class: audio_control::swbf2::zero_hash(&gun.class), matrix: gun.matrix,
            center: gun.local_center_offset, radius: gun.radius,
        });
        self.props.clear();
        self.props
            .extend(space.world_anims.objects.iter().map(|o| Moving {
                instance: o.prop_instance,
                previous: o.prev_matrix,
                current: o.live_matrix,
            }));
        self.turrets.clear();
        self.turrets.extend(space.buildings.iter().enumerate().filter_map(|(att_idx, b)| {
            let t = b.turret.as_ref()?;
            let alive = world.space_building_snapshot(b.building_index).is_some_and(|s| s.alive);
            let held = match t.pilot_type {
                SpaceTurretPilotType::Remote => alive && t.remote_operator.is_some(),
                SpaceTurretPilotType::SelfPiloted => alive,
                SpaceTurretPilotType::Player => false,
            };
            let fired = space.events.iter().any(|ev| {
                matches!(*ev, SpaceEvent::WeaponFired { building: Some(bidx), .. } if bidx == b.building_index)
            });
            let spinning = alive && t.trigger_was_held && !t.overheated;
            let firing = alive
                && !t.overheated
                && (fired
                    || (t.trigger_was_held
                        && (t.weapon.warm_up_time <= 0.0 || t.warm_up >= t.weapon.warm_up_time))
                    || t.clock.salvos > 0);
            let class = audio
                .building_classes
                .get(att_idx)
                .map_or(0, |&(c, _)| c);
            let weapon = audio.turret_weapons.get(att_idx).copied().unwrap_or(0);
            Some(Aim {
                building: b.building_index,
                class,
                weapon,
                position: b.center,
                previous: [t.prev_yaw, t.prev_pitch],
                current: [t.yaw, t.pitch],
                yaw_rate: t.turn_rate as f32,
                pitch_rate: t.pitch_rate as f32,
                alive,
                held,
                fired,
                firing,
                spinning,
                overheated: t.overheated,
                heat: t.heat as f32,
                heat_threshold: t.weapon.heat_threshold as f32,
            })
        }));
        self.rounds.clear();
        if let Some(obs) = &space.ordnance.observations {
            self.rounds.extend(obs.rounds.iter().copied().enumerate()
                .filter(|(_, r)| r.serial != 0 && r.tick == self.tick));
        }
        self.deaths.clear();
        self.respawns.clear();
        self.door_events.clear();
        if let Some(view) = &mission {
            for event in view.rules.observed_transitions() {
                if let Event::MissionObject { object: ObjectHandle::Prop(handle), transition: SystemTransition::Kill, .. } = event {
                    if let Some(b) = world.space_mission_building(handle) {
                        if self.deaths.len() < self.deaths.capacity() {
                            self.deaths.push(Destruction {
                                class: audio_control::swbf2::zero_hash(&b.class),
                                name: audio_control::swbf2::zero_hash(&b.name),
                                position: b.center,
                            });
                        }
                    }
                }
            }
        }
        for event in &space.events {
            match *event {
                SpaceEvent::BuildingDestroyed { building, .. } => {
                    if let Some((att_idx, b)) = space
                        .buildings
                        .iter()
                        .enumerate()
                        .find(|(_, b)| b.building_index == building)
                    {
                        let (class, name) = audio
                            .building_classes
                            .get(att_idx)
                            .copied()
                            .unwrap_or_else(|| {
                                (
                                    audio_control::swbf2::zero_hash(&b.class),
                                    audio_control::swbf2::zero_hash(&b.name),
                                )
                            });
                        let already = self
                            .deaths
                            .iter()
                            .any(|d| d.name == name && d.class == class);
                        if !already && self.deaths.len() < self.deaths.capacity() {
                            self.deaths.push(Destruction {
                                class,
                                name,
                                position: b.center,
                            });
                        }
                    }
                }
                SpaceEvent::BuildingRespawned { building, .. } => {
                    if let Some((att_idx, _)) = space
                        .buildings
                        .iter()
                        .enumerate()
                        .find(|(_, b)| b.building_index == building)
                    {
                        if let Some(&(_, name)) = audio.building_classes.get(att_idx) {
                            if name != 0 && self.respawns.len() < self.respawns.capacity() {
                                self.respawns.push(name);
                            }
                        }
                    }
                }
                SpaceEvent::FighterDestroyed { fighter, .. } => {
                    if let Some(f) = space.fighters.get(fighter) {
                        if self.deaths.len() < self.deaths.capacity() {
                            self.deaths.push(Destruction {
                                class: audio.fighters[f.profile_index],
                                name: 0,
                                position: f.position,
                            });
                        }
                    }
                }
                SpaceEvent::DoorChanged { door, open, locked } => {
                    if let Some(d) = space.doors.get(door) {
                        let class_hash = audio.doors.get(door).copied().unwrap_or(0);
                        if class_hash != 0 && self.door_events.len() < self.door_events.capacity() {
                            use audio_control::swbf2::vehicles::DoorEvent;
                            let ev = if open {
                                DoorEvent::Open { class_hash, position: d.position }
                            } else if locked {
                                DoorEvent::Locked { class_hash, position: d.position }
                            } else {
                                DoorEvent::Close { class_hash, position: d.position }
                            };
                            self.door_events.push(ev);
                        }
                    }
                }
                _ => {}
            }
        }
        self.hulls.clear();
        space.hidden_crew_units(world, &mut self.hidden_crew);
        self.chair_riders.clear();
        for b in &space.buildings {
            let Some(p) = b.terminal.as_ref().and_then(|t| t.occupant) else { continue };
            let Some(facts) = world.space_owner_facts(p.owner) else { continue };
            if self.chair_riders.len() < self.chair_riders.capacity() {
                self.chair_riders.push(ChairRider { unit: (facts.slot, facts.life as u32), instance: b.instance, matrix: b.matrix });
            }
        }
        for f in space.fighters.iter().filter(|f| f.alive) {
            let profile = &space.profiles[f.profile_index];
            let weapon_count = profile.weapons.len().min(5);
            let w_hashes = audio.fighter_weapons[f.profile_index];
            let mut weapons = [HullWeapon::default(); 5];
            for (wi, slot) in weapons.iter_mut().enumerate().take(weapon_count) {
                let spec = &profile.weapons[wi];
                let rt = &f.weapons[wi];
                let seat_occupied = f.occupant(spec.seat).is_some();
                let fired = space.events.iter().any(|ev| {
                    matches!(*ev, SpaceEvent::WeaponFired { fighter: Some(fid), weapon_index, .. } if fid == f.id && weapon_index as usize == wi)
                });
                let reloading = rt.reload_timer > 0.0;
                let spinning = seat_occupied && rt.trigger_was_held && !rt.overheated && !reloading;
                let firing = seat_occupied
                    && !rt.overheated
                    && !reloading
                    && (fired
                        || (rt.trigger_was_held
                            && (spec.warm_up_time <= 0.0 || rt.warm_up >= spec.warm_up_time))
                        || rt.clock.salvos > 0);
                *slot = HullWeapon {
                    class: w_hashes[wi],
                    fired,
                    firing,
                    spinning,
                    overheated: rt.overheated,
                    heat: rt.heat as f32,
                    heat_threshold: spec.heat_threshold as f32,
                    reloading,
                };
            }
            self.hulls.push(Hull {
                id: f.id,
                generation: f.generation,
                class: audio.fighters[f.profile_index],
                team: f.team as u8,
                previous: Pose {
                    position: f.previous_position,
                    rotation: f.previous_quaternion,
                    bank: f.prev_bank,
                    gear: f.prev_gear,
                },
                current: Pose {
                    position: f.position,
                    rotation: f.quaternion,
                    bank: f.bank,
                    gear: f.gear,
                },
                health: f.health,
                max_health: f.max_health,
                max_speed: profile.max_speed,
                boost_speed: profile.boost_speed,
                thrusting: matches!(
                    f.state,
                    SpaceFlightState::TakingOff | SpaceFlightState::Flying | SpaceFlightState::Landing
                ),
                piloted: f.pilot.is_some(),
                trick: f.trick,
                velocity: f.velocity,
                state: f.state,
                weapon_count: weapon_count as u8,
                weapons,
            });
        }
    }
}

/// Keep enough fixed publications to bracket the presentation clock. A stalled
/// window displays the nearest available endpoint until the clock catches up.
#[derive(Debug)]
pub struct History {
    frames: Vec<Frame>,
    next: usize,
    len: usize,
}

impl History {
    pub fn new(size: Counts) -> Self {
        Self {
            frames: (0..16).map(|_| Frame::reserved(size)).collect(),
            next: 0,
            len: 0,
        }
    }
    pub(crate) fn publish(&mut self, frame: &mut Frame) {
        let previous = self.latest();
        frame.weapon_changed = std::array::from_fn(|channel| {
            let cockpit = frame.cockpit?;
            let weapon = cockpit.weapons[channel]?;
            let old = previous?;
            if old.tick.checked_add(1) != Some(frame.tick) { return None; }
            match old.cockpit {
                Some(p)
                    if (p.mount, p.slot, p.generation, p.seat)
                        == (cockpit.mount, cockpit.slot, cockpit.generation, cockpit.seat)
                        && p.weapons[channel].is_some_and(|w| w.index == weapon.index) =>
                {
                    old.weapon_changed[channel]
                }
                _ => Some(frame.tick),
            }
        });
        std::mem::swap(&mut self.frames[self.next], frame);
        self.next = (self.next + 1) % self.frames.len();
        self.len = (self.len + 1).min(self.frames.len());
    }
    pub(crate) fn after(&self, previous: Option<u32>, through: u32) -> impl Iterator<Item = &Frame> {
        (0..self.len).map(move |i| &self.frames[(self.next + self.frames.len() - self.len + i) % self.frames.len()])
            .filter(move |f| previous.is_none_or(|t| f.tick > t) && f.tick <= through)
    }

    /// The kept frame of the tick before (or the preceding snapshot tick for
    /// multi-tick online frames), when it had a cockpit.
    pub(crate) fn previous(&self, tick: u32) -> Option<&Frame> {
        let target = self
            .frames
            .iter()
            .find(|f| f.tick == tick)
            .and_then(|f| f.prev_tick)
            .or_else(|| tick.checked_sub(1))?;
        self.frames.iter().find(|f| f.tick == target && f.cockpit.is_some())
    }

    pub(crate) fn latest(&self) -> Option<&Frame> {
        (self.len != 0)
            .then(|| &self.frames[(self.next + self.frames.len() - 1) % self.frames.len()])
    }

    pub(crate) fn latest_mut(&mut self) -> Option<&mut Frame> {
        let len = self.frames.len();
        (self.len != 0).then(|| &mut self.frames[(self.next + len - 1) % len])
    }

    pub(crate) fn reset(&mut self) {
        self.next = 0;
        self.len = 0;
    }

    pub(crate) fn sample(&self, shown: f64) -> Option<(&Frame, f64)> {
        let mut frame = self.latest()?;
        let start_of = |f: &Frame| f.prev_tick.map_or(f.tick as f64 - 1., |p| p as f64);
        for back in 1..self.len {
            if start_of(frame) < shown {
                break;
            }
            frame = &self.frames[(self.next + self.frames.len() - 1 - back) % self.frames.len()];
        }
        let start = start_of(frame);
        let span = (frame.tick as f64 - start).max(1.);
        Some((frame, ((shown - start) / span).clamp(0., 1.)))
    }
}

pub(super) struct Trace {
    out: std::io::BufWriter<std::fs::File>,
    player: u32,
    target: Option<usize>,
    mission_diagnostics: [u32; 4],
}

impl Trace {
    pub fn open(context: &bfe_sim::abi::context::Context, player: u32) -> Result<Option<Self>, String> {
        use std::io::Write;
        let Some(path) = std::env::var_os("BFE_DEV_SWBF2_FLIGHT_TRACE") else {
            return Ok(None);
        };
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::new(file);
        let name = std::env::var("BFE_DEV_SWBF2_TRACE_TARGET").ok();
        let target = context.owned(|w| -> std::io::Result<_> {
            let Some(s) = w.space() else { return Ok(None) };
            for p in &s.pads {
                writeln!(out, "# pad {:?} team={} class={} pos={:?} floor={} yaw={}",
                    p.name, p.team, s.profiles[p.profile_index].class_name, p.position, p.floor_y, p.yaw)?;
            }
            for profile in &s.profiles {
                for (index,w) in profile.weapons.iter().enumerate() {
                    writeln!(out,"# mount-def {} {} {} seat={} channel={} ordnance={:?} lock_range={} lock_cone={} lock_time={} speed={}",profile.class_name,index,w.name,w.seat,w.channel,w.ordnance_kind,w.lock_on_range,w.lock_on_angle_deg,w.lock_time,w.velocity)?;
                }
            }
            for b in &s.buildings {
                writeln!(out, "# building {} {:?} team={:?} center={:?} lock={}", b.building_index, b.name,
                    w.space_building_snapshot(b.building_index).map(|v| v.team), b.center, b.lock_on_enabled)?;
                if let Some(facts) = w.space_building_snapshot(b.building_index) {
                    writeln!(out, "# system {} {} hp={} shields={}/{} life={} lock={}", b.building_index,b.name,
                        facts.health,facts.cur_shield,facts.max_shield,facts.owner.life,facts.lock_on_enabled)?;
                }
            }
                for gate in &s.bounds.forcefields {
                    writeln!(out, "# shield {:?} team={} center={:?}", gate.name, gate.team, gate.center)?;
                }
            for door in &s.doors {
                writeln!(out,"# door {:?} center={:?} radius={}",door.name,door.trigger_position,door.door_trigger_radius)?;
            }
            for r in &s.bounds.landing_zones { writeln!(out, "# landing {:?} center={:?}", r.name, r.center)?; }
            writeln!(out,"tick,seat,hull_id,state,x,y,z,speed,health,energy,gear,bank,forward_x,forward_y,forward_z,target_hp,target_shield,right_x,right_y,right_z,up_x,up_y,up_z,in_landing_zone")?;
            Ok(name.as_ref().and_then(|n| w.space_building_by_name(n)))
        }).map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        Ok(Some(Self { out, player, target, mission_diagnostics: [0; 4] }))
    }

    pub fn sample(
        &mut self,
        world: &bfe_sim::world::owner::World,
        s: &SpaceBattlefield,
        frame: &Frame,
    ) -> std::io::Result<()> {
        use std::io::Write;
        let target = self.target.and_then(|id| world.space_building_snapshot(id));
        if let Some(view) = world.mission_view() {
            use bfe_sim::battle::rules::events::{Event, ObjectHandle};
            for event in view.rules.observed_transitions() {
                let placement = match event {
                    Event::MissionObject { object: ObjectHandle::Prop(handle), .. } => {
                        world.space_mission_building(handle).map(|b| b.name.as_str())
                    }
                    _ => None,
                };
                writeln!(self.out, "# rule {} placement={:?} {event:?}", s.tick, placement)?;
            }
            let score = view.score();
            if s.tick == 1 {
                writeln!(self.out, "# space-hud teams={:?} bitmaps={:?}", view.host.space_teams, view.host.space_assault_bitmaps)?;
            }
            if s.tick == 1 || s.tick % 60 == 0 {
                for (order, critical) in view.host.critical_systems.iter().enumerate() {
                    let ObjectHandle::Prop(handle) = &critical.object else { continue };
                    let Some(b) = world.space_mission_building(handle) else { continue };
                    if let Some(state) = world.space_building_snapshot(b.building_index) {
                        writeln!(self.out, "# critical {} {} team={} hp={} shields={}/{} life={} lock={} order={} building={} class={} marker={} max={} alive={}",
                            s.tick, b.name, state.team, state.health, state.cur_shield, state.max_shield,
                            state.owner.life, state.lock_on_enabled, order, b.building_index, b.class,
                            critical.show_marker, state.max_health, state.alive)?;
                        if let Some(published) = frame.system(b.building_index) {
                            writeln!(self.out, "# hud-system {} order={} building={} generation={} hull={}/{} shield={}/{} alive={}",
                                s.tick, order, published.building, published.generation, published.health, published.max_health,
                                published.shield, published.max_shield, published.alive)?;
                        }
                    }
                }
            }
            let diag = &view.host.diagnostics;
            let disabled: u32 = diag.errors.iter().filter(|e|
                e.kind == bfe_sim::battle::script::DiagnosticKind::ListenerDisabled).map(|e| e.occurrences).fold(0u32, u32::saturating_add);
            let counts = [diag.callback_errors, diag.callback_budget_refusals, diag.callbacks_dropped, disabled];
            writeln!(self.out, "# mission {} script={} scores={},{} target={:?} outcome={:?} over={} finished={} callbacks={} refused={} dropped={} disabled={} writes={} commands={} faulted={}",
                s.tick, score.script_tick, score.teams[0], score.teams[1], score.target, score.outcome,
                score.over, score.finished, diag.callbacks_executed, diag.callback_budget_refusals, diag.callbacks_dropped,
                disabled, diag.refused_writes, diag.refused_commands, diag.faulted)?;
            if self.mission_diagnostics != counts {
                for error in &diag.errors { writeln!(self.out, "# mission-diagnostic {} {error:?}", s.tick)?; }
                self.mission_diagnostics = counts;
            }
        }
        let hp = target.as_ref().map_or(f64::NAN, |b| b.health);
        let shield = target.as_ref().map_or(f64::NAN, |b| b.cur_shield);
        if let Some(f) = s.pilot_fighter(self.player) {
            let [x, y, z] = f.position;
            let [dx, dy, dz] = f.forward();
            let [rx, ry, rz] = f.right();
            let [ux, uy, uz] = f.up();
            let zone = s.bounds.landing_zone_at(f.position).is_some();
            let receipt = s.pilot_seat(self.player).unwrap();
            if s.tick % 12 == 0 { writeln!(self.out,"# local-seat {} fighter={} life={} seat={} owner={:?} weapon={:?} landing={:?}",s.tick,receipt.fighter,receipt.generation,receipt.seat,receipt.owner,f.seat_input(receipt.seat).weapon_index,world.space_landing_permission(receipt))?; }
            if let Some(panel) = instruments::Cockpit::read(world, s, self.player) {
                writeln!(self.out, "# instruments {} seat={} life={} hull={}/{} speed={}/{} boost={}/{} weapons={:?} target={:?}",
                    s.tick, panel.seat, panel.generation, panel.hull, panel.hull_max, panel.speed, panel.speed_max,
                    panel.energy, panel.energy_max, panel.weapons, panel.target)?;
            }
            let station = if receipt.seat == 0 { "pilot" } else { "gunner" };
            writeln!(self.out, "{},{station},{},{:?},{x},{y},{z},{},{},{},{},{},{dx},{dy},{dz},{hp},{shield},{rx},{ry},{rz},{ux},{uy},{uz},{zone}",
                s.tick, f.id, f.state, f.airspeed, f.health, f.energy, f.gear, f.bank)?;
        } else {
            let [x, y, z] = world.space_player_unit(self.player).and_then(|k| world.space_unit_position(k.reference())).unwrap_or([f64::NAN; 3]);
            let seat = if let Some((index,gun)) = s.pilot_terminal(self.player) {
                let t = gun.turret.as_ref().unwrap();
                writeln!(self.out,"# chair {} {} {} {:?} yaw={} pitch={} shots={} feet={:?}",s.tick,
                    index,gun.building_index,gun.name,t.yaw,t.pitch,t.shots_fired,[x,y,z])?;
                "turret"
            } else { "foot" };
            writeln!(
                self.out,
                "{},{seat},,,{x},{y},{z},,,,,,,,,{hp},{shield},,,,,,,",
                s.tick
            )?;
        }
        if let Some(obs) = &s.ordnance.observations {
            for (slot, r) in obs.rounds.iter().enumerate().filter(|(_, r)| r.serial != 0 && r.tick == s.tick) {
                if r.born == s.tick || r.contact.is_some() || s.tick % 12 == 0 {
                    writeln!(self.out, "# round {} {} {} {} {} {:?} {:?} {} {:?}",
                        s.tick, slot, r.serial, r.weapon, r.born, r.position, r.velocity, r.active, r.contact)?;
                    if r.born == s.tick {
                        writeln!(self.out, "# muzzle {} {} team={} body={:?} barrel={:?} position={:?}",
                            s.tick,r.serial,r.team,r.source_body,r.barrel,r.origin)?;
                    }
                }
            }
        }
        if let Some(obs) = &s.ordnance.observations {
            for (slot, shot) in s.ordnance.slots.iter().enumerate() {
                let r = &obs.rounds[slot];
                if r.serial != 0 && r.tick == s.tick && (r.born == s.tick || r.contact.is_some() || s.tick % 12 == 0)
                    && (shot.guided_pilot_fighter.is_some() || shot.lock_kind != bfe_sim::space::flight::SpaceLockTargetKind::None) {
                    writeln!(self.out, "# guidance {} {} {:?} operator={:?} team={} lock={:?}:{}:{}",
                        s.tick, r.serial, shot.kind, shot.guided_pilot_fighter, shot.owner_team, shot.lock_kind,
                        shot.lock_target, shot.lock_generation)?;
                }
            }
        }
        if s.tick % 6 == 0 {
            for f in s.fighters.iter().filter(|f| f.alive && f.pilot.is_some()) {
                writeln!(self.out, "# pilot {} {} {} {} {} {:?} {:?} {:?} {} {} {:?} {:?}",
                    s.tick, f.id, f.generation, f.team, f.pilot.unwrap().is_human, f.state,
                    f.position, f.velocity, f.health, f.max_health, f.lock, s.profiles[f.profile_index].class_name)?;
            }
        }
        for door in s.doors.iter().filter(|d| d.door_open != d.prev_door_open || d.previous_fraction != d.open_fraction) {
            writeln!(self.out,"# doorway {} {:?} open={} fraction={} solid={} blocked={}", s.tick,door.name,door.door_open,door.open_fraction,door.collidable,door.passage_blocked())?;
            for link in world.space_door_barriers(door.id) {
                writeln!(self.out,"# barrier {} {:?} requested={:?}",s.tick,link.name,link.requested_enabled(door))?;
            }
        }
        for event in &s.events {
            writeln!(self.out, "# event {} {event:?}", s.tick)?;
            use bfe_sim::space::SpaceEvent;
            let owner = match event {
                SpaceEvent::FighterDestroyed { attacker_unit, .. } | SpaceEvent::BuildingDestroyed { attacker_unit, .. } => *attacker_unit,
                _ => None,
            };
            if let Some(owner) = owner.and_then(|r| world.space_launch_snapshot(r)) {
                writeln!(self.out,"# credit {} unit={} life={} human={} team={} character={:?}",s.tick,owner.source_unit.slot,owner.generation,owner.human,owner.team,owner.character)?;
            }
        }
        for f in s.fighters.iter().filter(|f| f.contacts > 0) {
            writeln!(self.out, "# hull-contact {} slot={} life={} side={} state={:?} position={:?} contacts={} normal={:?} hull={}/{}",
                s.tick, f.id, f.generation, f.team, f.state, f.position, f.contacts, f.contact_normal, f.health, f.max_health)?;
        }
        if s.tick == 60 {
            let mut ordinary = [0_usize; 2];
            for (team, _) in world.space_bot_roster() {
                if let Some(count) = team.checked_sub(1).and_then(|i| ordinary.get_mut(i)) {
                    *count += 1;
                }
            }
            writeln!(self.out, "# pilot-roster {} ordinary={ordinary:?}", s.tick)?;
        }
        if s.tick % 60 == 0 {
            if let Some(census) = world.space_pilot_census() {
                writeln!(self.out, "# pilot-census {} {census:?}", s.tick)?;
            }
            for a in &s.world_anims.objects {
                writeln!(
                    self.out,
                    "# prop {} {} {:?}",
                    s.tick,
                    a.name,
                    &a.live_matrix[12..15]
                )?;
            }
            for b in &s.buildings {
                if let Some(t) = &b.turret {
                    writeln!(
                        self.out,
                        "# turret {} {} {} {} {}",
                        s.tick, b.name, t.yaw, t.pitch, t.shots_fired
                    )?;
                }
            }
        }
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leaving_warning_lasts_while_the_out_of_bounds_timer_runs() {
        assert_eq!(out_of_bounds_left(-1.0), None);
        assert_eq!(out_of_bounds_left(2.5), Some(2.5));
        assert_eq!(out_of_bounds_left(0.0), Some(0.0));
    }

    #[test]
    fn different_draw_rates_sample_the_same_fixed_path() {
        let size = Counts {
            fighters: 1,
            ..Default::default()
        };
        for fps in [60, 144, 500] {
            let mut history = History::new(size);
            let mut frame = Frame::reserved(size);
            let mut tick = 0;
            for redraw in 1..=fps * 2 {
                let shown = f64::from(redraw * 60) / f64::from(fps);
                while tick < shown.ceil() as u32 + 2 {
                    tick += 1;
                    let pose = |step| Pose {
                        position: [step as f64 * 0.25, 0., 0.],
                        rotation: [0., 0., 0., 1.],
                        ..Default::default()
                    };
                    frame.tick = tick;
                    frame.hulls.clear();
                    frame.hulls.push(Hull {
                        previous: pose(tick - 1),
                        current: pose(tick),
                        ..Default::default()
                    });
                    history.publish(&mut frame);
                }
                let (frame, fraction) = history.sample(shown).unwrap();
                let hull = &frame.hulls[0];
                let m = hull.previous.matrix(&hull.current, fraction);
                assert!((m.elements[12] - shown * 0.25).abs() < 1e-10);
            }
        }
    }
}
