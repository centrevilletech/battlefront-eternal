//! Online Space Assault wire receiver (`0x0211`..`0x0215`, `0x91`) and 60 Hz own-craft
//! and chair-turret prediction with smooth error correction.
//!
//! Feeds the exact same `local::space::History` -> `local::flight_view::Fleet` +
//! `online::combat::swbf2::space::SpaceEffects` + `app::battle::screens::flight::FlightMap`
//! pipeline used by local Space battles.
use crate::local::{
    flight_view::OwnSpaceOverride,
    space::{
        instruments::{
            Cockpit, CockpitInput, CockpitLockInput, CockpitTargetFacts, CockpitWeaponInput,
            ContactName, Names,
        },
        Aim, Chair, Counts, Destruction, Frame, History, Hull, Moving, Pose,
    },
};
use crate::online::{input::Sender, mission::Mission, receiver::Receiver, replay::Collision};
use audio_control::swbf2::zero_hash;
use bfe_content::defs::Defs;
use bfe_sim::{
    collision::CollisionOwner,
    space::{
        flight::{
            SpaceFlightProfile, SpaceFlightState, SpaceFighter, SpaceLockTargetKind,
            SpaceVehiclePad,
        },
        local::{
            advance_gunner_aim, advance_pilot_stick, terminal::advance_chair_aim, PilotControls,
        },
        observations::{Contact, Round},
        world_anim::SpaceWorldAnims,
        SpaceClientData, SpaceDoorSnapshot,
    },
    wire::{
        revisions::split_revision,
        space::{
            decode_pos_u24, decode_turret_angle, decode_vel, quantize_pilot_controls,
            read_fighter_field_into, read_frigates_into, read_ordnance_field_into, read_own_body,
            read_props_field_into, restore_fighter_from_own, unpack_oct_normal, unpack_quat,
            DeathSnap, DoorSnap, EventsSnap, FighterFieldKind, FighterSnap, GuidedSnap, ImpactSnap,
            LaunchSnap, OrdnanceFieldKind,
            OwnChairState, OwnPilotState, OwnSpaceBody, PropsFieldKind, SeatEntry, FIGHTER_SCHEMA,
            FIGHTERS_PER_CHUNK, FRIGATES_SCHEMA, MAX_ANIM_GROUPS, MAX_DEATHS, MAX_DOORS,
            MAX_FIGHTERS, MAX_GUIDED, MAX_IMPACTS, MAX_LAUNCHES, MAX_SEATS, MAX_TURRETS,
            ORDNANCE_SCHEMA, PROPS_SCHEMA, REVISION,
        },
        PositionBounds,
    },
};
use std::{collections::BTreeMap, sync::Arc};
use three::{Matrix4, Quaternion, Vector3};

/// A gunner sight evaluated without a step: no seat limit applies.
const NO_AIM_LIMITS: [f64; 4] = [f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY];

const STEP_DT: f64 = 1.0 / 60.0;
const AIR_TAU: f64 = 0.08;
const LANDED_TAU: f64 = 0.05;
const CHAIR_TAU: f64 = 0.06;
const ROT_SNAP_RAD: f64 = 35.0 * std::f64::consts::PI / 180.0;
const CHAIR_SNAP_RAD: f64 = 30.0 * std::f64::consts::PI / 180.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TurretMeta {
    pub chair: Chair,
    pub yaw_limits: [f64; 2],
    pub pitch_limits: [f64; 2],
    pub rates: [f64; 2],
}

/// Static Space battle metadata loaded once on admission from the same `BattleContent`
/// parsers the server uses.
pub struct SpaceClientMeta {
    pub data: SpaceClientData,
    pub profile_hashes: Vec<u32>,
    pub building_hashes: Vec<u32>,
    pub(crate) names: Arc<Names>,
    pub counts: Counts,
    pub radar: [f64; 3],
    pub(crate) turrets_by_building: BTreeMap<usize, TurretMeta>,
    pub building_by_index: BTreeMap<usize, usize>,
    pub pad_for_profile: Vec<SpaceVehiclePad>,
}

impl SpaceClientMeta {
    pub fn build(
        battle: &bfe_content::battle::Battle,
        defs: Option<&Defs>,
        collision: &mut CollisionOwner,
        collision_world: usize,
    ) -> Option<Arc<Self>> {
        let data = SpaceClientData::from_battle(battle, collision, collision_world)?;
        Some(Self::from_client_data(data, defs))
    }

    pub fn from_client_data(data: SpaceClientData, defs: Option<&Defs>) -> Arc<Self> {
        let profile_hashes: Vec<u32> = data
            .profiles
            .iter()
            .map(|p| zero_hash(&p.class_name))
            .collect();
        let building_hashes: Vec<u32> = data
            .buildings
            .iter()
            .map(|b| zero_hash(&b.class))
            .collect();
        let names = Arc::new(Names::from_parts(&data.profiles, &data.buildings, defs));
        let counts = Counts::from_client_data(&data);
        let b = &data.bounds;
        let radar = [
            b.center[0],
            b.center[2],
            ((b.max[0] - b.min[0]).max(b.max[2] - b.min[2]) * 0.5).max(1.0),
        ];
        let mut turrets_by_building = BTreeMap::new();
        let mut building_by_index = BTreeMap::new();
        for (slot, bld) in data.buildings.iter().enumerate() {
            building_by_index.insert(bld.building_index, slot);
            if let Some(t) = &bld.turret {
                turrets_by_building.insert(
                    bld.building_index,
                    TurretMeta {
                        chair: Chair {
                            instance: bld.instance,
                            building: bld.building_index,
                            class: zero_hash(&bld.class),
                            matrix: bld.matrix,
                            center: bld.local_center_offset,
                            radius: bld.radius,
                        },
                        yaw_limits: [t.yaw_min, t.yaw_max],
                        pitch_limits: [t.pitch_min, t.pitch_max],
                        rates: [t.turn_rate, t.pitch_rate],
                    },
                );
            }
        }
        let pad_for_profile = (0..data.profiles.len())
            .map(|pi| {
                data.pads
                    .iter()
                    .find(|p| p.profile_index == pi)
                    .cloned()
                    .unwrap_or_else(|| SpaceVehiclePad {
                        name: String::new(),
                        team: 0,
                        profile_index: pi,
                        control_zone: String::new(),
                        position: [0.0; 3],
                        yaw: 0.0,
                        matrix: [
                            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                        ],
                        spawn_count: 1,
                        spawn_time: 8.0,
                        expire_time_enemy: 20.0,
                        expire_time_field: 40.0,
                        decay_time: 10.0,
                        respawn_timer: 8.0,
                        floor_y: 0.0,
                    })
            })
            .collect();
        Arc::new(Self {
            data,
            profile_hashes,
            building_hashes,
            names,
            counts,
            radar,
            turrets_by_building,
            building_by_index,
            pad_for_profile,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct FighterWireState {
    generation: u32,
    profile: usize,
    team: u8,
    state: SpaceFlightState,
    thrusting: bool,
    pose: Pose,
    velocity: [f64; 3],
    health: f64,
    max_health: f64,
    max_speed: f64,
}

#[derive(Clone, Copy, Debug)]
struct BuildingWireState {
    alive: bool,
    /// The look that stands: 0 built, 1 destroyed, 2 unbuilt (its body is the one that collides).
    look: u8,
    team: u8,
    health: f32,
    max_health: f32,
}

/// Online Space snapshot assembler owned by [`Receiver`].
pub struct OnlineSpace {
    pub meta: Arc<SpaceClientMeta>,
    bounds: PositionBounds,
    world_anims: SpaceWorldAnims,
    frigate_clocks: Vec<(u8, u32)>,
    prev_prop_matrices: Vec<[f64; 16]>,
    fighters: Vec<Option<FighterWireState>>,
    prev_fighters: Vec<Option<(u32, Pose)>>,
    seated: [bool; 512],
    turret_aims: Vec<[f64; 2]>,
    prev_turret_aims: Vec<[f64; 2]>,
    turrets_scratch: Vec<[i16; 2]>,
    doors_scratch: Vec<DoorSnap>,
    building_states: Vec<Option<BuildingWireState>>,
    pub door_states: Vec<SpaceDoorSnapshot>,
    guided: Vec<GuidedSnap>,
    prev_guided: Vec<(u16, [f64; 3])>,
    next_guided: Vec<(u16, [f64; 3])>,
    pending_events: EventsSnap,
    has_pending_events: bool,
    /// The snapshot tick `pending_events` came with (0: the next published one).
    pending_tick: u32,
    /// Events of snapshots whose parts arrived but which never completed (a
    /// part or the own datagram was lost), each with its own snapshot tick,
    /// drawn with the next published frame: the carry in the next snapshot
    /// then only has to cover a lost events part.
    held_launches: Vec<(u32, LaunchSnap)>,
    held_impacts: Vec<(u32, ImpactSnap)>,
    held_deaths: Vec<DeathSnap>,
    seen_launches: [u16; 128],
    seen_launch_len: usize,
    seen_launch_at: usize,
    seen_impacts: [u16; 128],
    seen_impact_len: usize,
    seen_impact_at: usize,
    seen_deaths: [(bool, u16, [u32; 3]); 64],
    seen_death_len: usize,
    seen_death_at: usize,
    pub own_body: Option<OwnSpaceBody>,
    pub own_ack_seq: u32,
    pub own_ack_tick: u32,
    pub own_revision: u32,
    last_pub_tick: u32,
    dirty_fields: bool,
    scratch: Frame,
}

fn mark_seen_u16(ring: &mut [u16; 128], len: &mut usize, at: &mut usize, key: u16) -> bool {
    if ring[..*len].contains(&key) {
        return false;
    }
    ring[*at] = key;
    *at = (*at + 1) % ring.len();
    if *len < ring.len() {
        *len += 1;
    }
    true
}

fn mark_seen_death(
    ring: &mut [(bool, u16, [u32; 3]); 64],
    len: &mut usize,
    at: &mut usize,
    key: (bool, u16, [u32; 3]),
) -> bool {
    if ring[..*len].contains(&key) {
        return false;
    }
    ring[*at] = key;
    *at = (*at + 1) % ring.len();
    if *len < ring.len() {
        *len += 1;
    }
    true
}

impl OnlineSpace {
    pub fn new(meta: Arc<SpaceClientMeta>, bounds: PositionBounds) -> Self {
        let world_anims = meta.data.world_anims.clone();
        let prev_prop_matrices = world_anims.objects.iter().map(|o| o.base_matrix).collect();
        let slot_cap = meta.counts.fighters.max(MAX_FIGHTERS);
        let turret_count = meta.data.turret_buildings.len();
        let building_count = meta.data.buildings.len();
        let door_states = meta
            .data
            .doors
            .iter()
            .map(|d| SpaceDoorSnapshot {
                id: d.id,
                instance: d.instance,
                previous_fraction: 0.0,
                open_fraction: 0.0,
                locked: d.locked,
                visible: d.visible,
                collidable: bfe_sim::space::objects::SpaceDoor::is_blocking(
                    d.collidable,
                    d.locked,
                    0.0,
                ),
                prev_door_open: d.door_open,
                door_open: d.door_open,
                position: d.trigger_position,
            })
            .collect();
        let scratch = Frame::reserved(meta.counts);
        Self {
            meta,
            bounds,
            world_anims,
            frigate_clocks: Vec::with_capacity(MAX_ANIM_GROUPS),
            prev_prop_matrices,
            fighters: vec![None; slot_cap],
            prev_fighters: vec![None; slot_cap],
            seated: [false; 512],
            turret_aims: vec![[0.0; 2]; turret_count],
            prev_turret_aims: vec![[0.0; 2]; turret_count],
            turrets_scratch: Vec::with_capacity(MAX_TURRETS),
            doors_scratch: Vec::with_capacity(MAX_DOORS),
            building_states: vec![None; building_count],
            door_states,
            guided: Vec::with_capacity(MAX_GUIDED),
            prev_guided: Vec::with_capacity(MAX_GUIDED),
            next_guided: Vec::with_capacity(MAX_GUIDED),
            pending_events: EventsSnap {
                launches: Vec::with_capacity(MAX_LAUNCHES),
                impacts: Vec::with_capacity(MAX_IMPACTS),
                deaths: Vec::with_capacity(MAX_DEATHS),
            },
            has_pending_events: false,
            pending_tick: 0,
            held_launches: Vec::with_capacity(2 * MAX_LAUNCHES),
            held_impacts: Vec::with_capacity(2 * MAX_IMPACTS),
            held_deaths: Vec::with_capacity(2 * MAX_DEATHS),
            seen_launches: [0; 128],
            seen_launch_len: 0,
            seen_launch_at: 0,
            seen_impacts: [0; 128],
            seen_impact_len: 0,
            seen_impact_at: 0,
            seen_deaths: [(false, 0, [0; 3]); 64],
            seen_death_len: 0,
            seen_death_at: 0,
            own_body: None,
            own_ack_seq: 0,
            own_ack_tick: 0,
            own_revision: 0,
            last_pub_tick: 0,
            dirty_fields: false,
            scratch,
        }
    }

    pub fn carries(schema: u16) -> bool {
        matches!(
            schema,
            FIGHTER_SCHEMA | FRIGATES_SCHEMA | ORDNANCE_SCHEMA | PROPS_SCHEMA
        )
    }

    pub fn reset(&mut self) {
        self.world_anims = self.meta.data.world_anims.clone();
        self.frigate_clocks.clear();
        self.prev_prop_matrices.clear();
        self.prev_prop_matrices
            .extend(self.world_anims.objects.iter().map(|o| o.base_matrix));
        self.fighters.fill(None);
        self.prev_fighters.fill(None);
        self.seated.fill(false);
        self.turret_aims.fill([0.0; 2]);
        self.prev_turret_aims.fill([0.0; 2]);
        self.building_states.fill(None);
        for (dst, src) in self.door_states.iter_mut().zip(&self.meta.data.doors) {
            dst.previous_fraction = 0.0;
            dst.open_fraction = 0.0;
            dst.locked = src.locked;
            dst.collidable = bfe_sim::space::objects::SpaceDoor::is_blocking(
                src.collidable,
                src.locked,
                0.0,
            );
            dst.prev_door_open = src.door_open;
            dst.door_open = src.door_open;
        }
        self.guided.clear();
        self.prev_guided.clear();
        self.next_guided.clear();
        self.pending_events.launches.clear();
        self.pending_events.impacts.clear();
        self.pending_events.deaths.clear();
        self.has_pending_events = false;
        self.pending_tick = 0;
        self.held_launches.clear();
        self.held_impacts.clear();
        self.held_deaths.clear();
        self.seen_launch_len = 0;
        self.seen_launch_at = 0;
        self.seen_impact_len = 0;
        self.seen_impact_at = 0;
        self.seen_death_len = 0;
        self.seen_death_at = 0;
        self.own_body = None;
        self.own_ack_seq = 0;
        self.own_ack_tick = 0;
        self.own_revision = self.own_revision.wrapping_add(1);
        self.last_pub_tick = 0;
        self.dirty_fields = false;
    }

    /// Whether the soldier in unit slot `unit_slot` holds a craft's seat (a
    /// pilot or a gunner; a remote chair's operator is not seated in a craft).
    #[inline]
    pub fn is_seated(&self, unit_slot: u32) -> bool {
        (unit_slot as usize) < self.seated.len() && self.seated[unit_slot as usize]
    }

    /// Processes one validated Space world field (`0x0211`, `0x0213`, `0x0214`, `0x0215`)
    /// with zero heap allocations on steady updates.
    /// Returns `Ok(true)` if door states were updated and should be published into `DoorFrames`.
    pub fn receive_field(
        &mut self,
        schema: u16,
        id: u32,
        payload: &[u8],
    ) -> Result<bool, &'static str> {
        self.receive_field_at(0, schema, id, payload)
    }

    /// [`Self::receive_field`] for a field of the snapshot at authority `tick`
    /// (0: the next published one). Events still pending from an earlier
    /// snapshot that never completed are kept, with their own tick, for the
    /// next published frame instead of being replaced.
    pub fn receive_field_at(
        &mut self,
        tick: u32,
        schema: u16,
        id: u32,
        payload: &[u8],
    ) -> Result<bool, &'static str> {
        self.dirty_fields = true;
        if schema == ORDNANCE_SCHEMA && id == 1 {
            self.hold_unpublished_events(tick);
            self.pending_tick = tick;
        }
        if payload.is_empty() {
            match schema {
                FIGHTER_SCHEMA if id == 256 => self.seated.fill(false),
                FIGHTER_SCHEMA if (id as usize) < 16 => {
                    let base = (id as usize) * FIGHTERS_PER_CHUNK;
                    for slot in base..(base + FIGHTERS_PER_CHUNK).min(self.fighters.len()) {
                        self.fighters[slot] = None;
                    }
                }
                ORDNANCE_SCHEMA if id == 0 => self.guided.clear(),
                ORDNANCE_SCHEMA if id == 1 => {
                    self.has_pending_events = false;
                    self.pending_events.launches.clear();
                    self.pending_events.impacts.clear();
                    self.pending_events.deaths.clear();
                }
                _ => {}
            }
            return Ok(false);
        }
        let (rev, body) = split_revision(payload)?;
        if rev != REVISION {
            return Err("invalid Battlefront II space record");
        }
        let mut doors_updated = false;
        match schema {
            FIGHTER_SCHEMA => {
                let mut chunk_buf = [FighterSnap::default(); FIGHTERS_PER_CHUNK];
                let mut seats_buf = [SeatEntry::default(); MAX_SEATS];
                match read_fighter_field_into(id, body, &mut chunk_buf, &mut seats_buf)? {
                    FighterFieldKind::Chunk { chunk, alive_mask, count } => {
                        let base = usize::from(chunk) * FIGHTERS_PER_CHUNK;
                        for b in 0..FIGHTERS_PER_CHUNK {
                            let slot = base + b;
                            if slot < self.fighters.len() && (alive_mask & (1 << b)) == 0 {
                                self.fighters[slot] = None;
                            }
                        }
                        for snap in &chunk_buf[..count] {
                            let slot = base + usize::from(snap.slot_in_chunk);
                            let prof_idx = usize::from(snap.profile);
                            let Some(profile) = self.meta.data.profiles.get(prof_idx) else {
                                return Err("invalid space fighter profile index");
                            };
                            if slot >= self.fighters.len() {
                                continue;
                            }
                            let pos = decode_pos_u24(&self.bounds, snap.pos);
                            let vel = decode_vel(snap.vel, 32.0);
                            let rot = unpack_quat(snap.quat)?;
                            let state = match snap.state {
                                0 => SpaceFlightState::Landed,
                                1 => SpaceFlightState::TakingOff,
                                2 => SpaceFlightState::Flying,
                                _ => SpaceFlightState::Landing,
                            };
                            let health = f64::from(snap.health);
                            let bank = f64::from(snap.bank) / 127.0 * (std::f64::consts::PI * 0.5);
                            let gear = f64::from(snap.gear) / 255.0;
                            self.fighters[slot] = Some(FighterWireState {
                                generation: u32::from(snap.generation),
                                profile: prof_idx,
                                team: snap.team,
                                state,
                                thrusting: snap.thrusting || snap.boosting,
                                pose: Pose {
                                    position: pos,
                                    rotation: rot,
                                    bank,
                                    gear,
                                },
                                velocity: vel,
                                health,
                                max_health: profile.max_health,
                                max_speed: profile.max_speed,
                            });
                        }
                    }
                    FighterFieldKind::Seats(count) => {
                        self.seated.fill(false);
                        for s in seats_buf[..count].iter().filter(|s| !s.is_chair) {
                            let idx = usize::from(s.unit_slot);
                            if idx < self.seated.len() {
                                self.seated[idx] = true;
                            }
                        }
                    }
                }
            }
            FRIGATES_SCHEMA => {
                read_frigates_into(id, body, &mut self.frigate_clocks)?;
            }
            ORDNANCE_SCHEMA => match read_ordnance_field_into(
                id,
                body,
                &mut self.guided,
                &mut self.pending_events,
            )? {
                OrdnanceFieldKind::Guided => {}
                OrdnanceFieldKind::Events => {
                    self.has_pending_events = true;
                }
            },
            PROPS_SCHEMA => match read_props_field_into(
                id,
                body,
                &mut self.turrets_scratch,
                &mut self.doors_scratch,
            )? {
                PropsFieldKind::Turrets => {
                    for (dst, &[qy, qp]) in
                        self.turret_aims.iter_mut().zip(self.turrets_scratch.iter())
                    {
                        *dst = [decode_turret_angle(qy), decode_turret_angle(qp)];
                    }
                }
                PropsFieldKind::Doors => {
                    for (idx, (dst, d)) in
                        self.door_states.iter_mut().zip(self.doors_scratch.iter()).enumerate()
                    {
                        dst.previous_fraction = dst.open_fraction;
                        dst.open_fraction = f64::from(d.open) / 255.0;
                        dst.prev_door_open = dst.door_open;
                        dst.door_open = d.opening;
                        dst.locked = d.locked;
                        let base_collidable = self
                            .meta
                            .data
                            .doors
                            .get(idx)
                            .is_none_or(|m| m.collidable);
                        dst.collidable = bfe_sim::space::objects::SpaceDoor::is_blocking(
                            base_collidable,
                            dst.locked,
                            dst.open_fraction,
                        );
                    }
                    doors_updated = !self.door_states.is_empty();
                }
            },
            _ => {}
        }
        Ok(doors_updated)
    }

    /// Moves pending events of an earlier, unpublished snapshot into the held
    /// lists (allocation free: the lists keep their admitted capacity, and a
    /// serial or burst already held is not held twice).
    fn hold_unpublished_events(&mut self, tick: u32) {
        if !self.has_pending_events || self.pending_tick == 0 || self.pending_tick == tick {
            return;
        }
        let at = self.pending_tick;
        for l in &self.pending_events.launches {
            if self.held_launches.len() < self.held_launches.capacity()
                && !self.held_launches.iter().any(|(_, h)| h.serial == l.serial)
            {
                self.held_launches.push((at, *l));
            }
        }
        for i in &self.pending_events.impacts {
            if self.held_impacts.len() < self.held_impacts.capacity()
                && !self.held_impacts.iter().any(|(_, h)| h.serial == i.serial)
            {
                self.held_impacts.push((at, *i));
            }
        }
        for d in &self.pending_events.deaths {
            if self.held_deaths.len() < self.held_deaths.capacity() && !self.held_deaths.contains(d) {
                self.held_deaths.push(*d);
            }
        }
        self.has_pending_events = false;
    }

    /// The look the server says stands for the building in client slot `slot`
    /// (0 built, 1 destroyed, 2 unbuilt), once its summary has arrived.
    pub fn building_look(&self, slot: usize) -> Option<u8> {
        self.building_states.get(slot).copied().flatten().map(|b| b.look)
    }

    pub fn receive_building_summary(&mut self, id: u32, payload: &[u8]) {
        const BUILDING_STATE_BASE: u32 = 200_000;
        let Some(bidx) = id.checked_sub(BUILDING_STATE_BASE).map(|v| v as usize) else {
            return;
        };
        let Some(&slot) = self.meta.building_by_index.get(&bidx) else {
            return;
        };
        use serde::de::IgnoredAny as Rest;
        type State<'a> = (usize, &'a str, Option<f64>, Option<f64>, Rest, Rest, Rest, Rest, Rest, Rest, Rest);
        let Ok(state) = serde_json::from_slice::<State>(payload) else {
            return;
        };
        if state.0 != bidx {
            return;
        }
        let Some(look) = ["built", "destroyed", "unbuilt"].iter().position(|name| *name == state.1) else {
            return;
        };
        let alive = look == 0;
        let health = state.2.filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0) as f32;
        let team = state
            .3
            .filter(|v| v.is_finite() && (0.0..=255.0).contains(v))
            .map_or(0u8, |v| v as u8);
        let prev_max = self
            .building_states
            .get(slot)
            .and_then(|s| s.map(|b| b.max_health))
            .unwrap_or(0.0);
        if let Some(entry) = self.building_states.get_mut(slot) {
            *entry = Some(BuildingWireState {
                alive,
                look: look as u8,
                team,
                health,
                max_health: prev_max.max(health).max(1.0),
            });
        }
    }

    pub fn receive_own(
        &mut self,
        body_bytes: &[u8],
        ack_seq: u32,
        ack_tick: u32,
    ) -> Result<OwnSpaceBody, &'static str> {
        let (rev, body) = split_revision(body_bytes)?;
        if rev != REVISION {
            return Err("invalid Battlefront II space own body");
        }
        let parsed = read_own_body(body)?;
        self.own_body = Some(parsed);
        self.own_ack_seq = ack_seq;
        self.own_ack_tick = ack_tick;
        self.own_revision = self.own_revision.wrapping_add(1);
        Ok(parsed)
    }

    pub fn clear_own(&mut self) {
        if self.own_body.is_some() {
            self.own_body = None;
            self.own_revision = self.own_revision.wrapping_add(1);
        }
    }

    pub fn world_anim_matrix(&self, instance: usize) -> Option<([f64; 16], [f64; 16])> {
        self.world_anims
            .objects
            .iter()
            .find(|o| o.prop_instance == instance)
            .map(|o| (o.live_matrix, o.live_inverse))
    }

    fn build_own_fields(
        &self,
        own_team: u8,
        mission: Option<&Mission>,
    ) -> (
        Option<usize>,
        Option<Chair>,
        Option<Cockpit>,
        Option<(usize, u32, u8, f64)>,
        Option<[[f64; 3]; 2]>,
    ) {
        match &self.own_body {
            Some(OwnSpaceBody::Pilot(p)) => {
                let slot = usize::from(p.fighter_id);
                let prof_idx = usize::from(p.profile);
                let profile = self.meta.data.profiles.get(prof_idx);
                let cockpit = profile.map(|prof| self.cockpit_from_own(p, prof, own_team, mission));
                let fighter_lock = profile.and_then(|prof| {
                    let lock = p.lock.as_ref()?;
                    if lock.kind != SpaceLockTargetKind::Fighter as u8 {
                        return None;
                    }
                    let widx = usize::from(lock.weapon_idx);
                    if !SpaceFighter::weapon_selected(&prof.weapons, p.seat, p.selected_weapon, widx) {
                        return None;
                    }
                    let w = prof.weapons.get(widx)?;
                    let progress = if lock.locked {
                        1.0
                    } else if w.lock_time > 0.0 {
                        (f64::from(lock.progress) / w.lock_time).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    Some((
                        usize::from(lock.target_idx),
                        lock.target_gen,
                        w.channel,
                        progress,
                    ))
                });
                let gunner_aim = if p.is_gunner {
                    let q = Quaternion::new(
                        p.quaternion[0],
                        p.quaternion[1],
                        p.quaternion[2],
                        p.quaternion[3],
                    );
                    let mut m = Matrix4::default();
                    m.make_rotation_from_quaternion(&q);
                    let e = m.elements;
                    let right = [e[0], e[1], e[2]];
                    let up = [e[4], e[5], e[6]];
                    let forward = [e[8], e[9], e[10]];
                    let mut angles = p.gunner_angles;
                    let dir = advance_gunner_aim(
                        &mut angles,
                        Default::default(),
                        NO_AIM_LIMITS,
                        forward,
                        right,
                        up,
                        0.0,
                    );
                    Some([dir, dir])
                } else {
                    None
                };
                (Some(slot), None, cockpit, fighter_lock, gunner_aim)
            }
            Some(OwnSpaceBody::Chair(c)) => {
                let chair = self
                    .meta
                    .turrets_by_building
                    .get(&usize::from(c.turret_building))
                    .map(|t| t.chair);
                (None, chair, None, None, None)
            }
            None => (None, None, None, None, None),
        }
    }

    fn cockpit_from_own(
        &self,
        own: &OwnPilotState,
        profile: &SpaceFlightProfile,
        own_team: u8,
        mission: Option<&Mission>,
    ) -> Cockpit {
        let mut weapon_inputs = [CockpitWeaponInput {
            index: 0,
            selected: false,
            ammo_in_clip: 0,
            heat: 0.0,
        }; 5];
        let mut wcount = 0usize;
        let own_weapons = own.weapons();
        let count = own_weapons.len().min(profile.weapons.len()).min(5);
        for (i, rt) in own_weapons.iter().enumerate().take(count) {
            if profile.weapons[i].seat != own.seat {
                continue;
            }
            weapon_inputs[wcount] = CockpitWeaponInput {
                index: i,
                selected: SpaceFighter::weapon_selected(
                    &profile.weapons,
                    own.seat,
                    own.selected_weapon,
                    i,
                ),
                ammo_in_clip: u32::from(rt.ammo_in_clip),
                heat: rt.heat,
            };
            wcount += 1;
        }
        let lock_gen = own.lock.as_ref().map_or(0, |l| l.target_gen);
        let lock = own.lock.as_ref().and_then(|l| {
            let kind = match l.kind {
                1 => SpaceLockTargetKind::Fighter,
                2 => SpaceLockTargetKind::Object,
                _ => return None,
            };
            Some(CockpitLockInput {
                kind,
                weapon_index: usize::from(l.weapon_idx),
                target_index: usize::from(l.target_idx),
                progress: f64::from(l.progress),
                locked: l.locked,
            })
        });
        Cockpit::build(
            CockpitInput {
                slot: usize::from(own.fighter_id),
                generation: own.fighter_gen,
                seat: own.seat,
                can_land: own.can_land,
                team: own_team,
                profile_index: usize::from(own.profile),
                profile,
                hull: own.health as f32,
                hull_max: profile.max_health as f32,
                speed: own.airspeed as f32,
                energy: own.energy as f32,
                weapons: &weapon_inputs[..wcount],
                lock,
            },
            |kind, idx| match kind {
                SpaceLockTargetKind::Fighter => {
                    let f = self.fighters.get(idx).and_then(Option::as_ref)?;
                    if f.health <= 0.0 || (f.generation & 0xffff) != (lock_gen & 0xffff) {
                        return None;
                    }
                    let p = self.meta.data.profiles.get(f.profile)?;
                    Some(CockpitTargetFacts {
                        id: idx as u32,
                        life: f.generation,
                        team: f.team,
                        name: ContactName::Craft(f.profile),
                        hp: f.health as f32,
                        max_hp: f.max_health as f32,
                        position: f.pose.position,
                        radius: (p.bounds_max[0] - p.bounds_min[0]).abs() * 0.5,
                    })
                }
                SpaceLockTargetKind::Object => {
                    let slot = *self.meta.building_by_index.get(&idx)?;
                    let b = &self.meta.data.buildings[slot];
                    if let Some(sys) = mission.and_then(|m| m.system_for_building(idx)) {
                        if !sys.state.alive {
                            return None;
                        }
                        let [hp, max_hp] = sys.state.health?;
                        return Some(CockpitTargetFacts {
                            id: idx as u32,
                            life: lock_gen,
                            team: sys.state.team,
                            name: ContactName::Building(slot),
                            hp,
                            max_hp,
                            position: b.center,
                            radius: b.radius,
                        });
                    }
                    let state = self.building_states.get(slot).and_then(|s| *s)?;
                    if !state.alive {
                        return None;
                    }
                    Some(CockpitTargetFacts {
                        id: idx as u32,
                        life: lock_gen,
                        team: state.team,
                        name: ContactName::Building(slot),
                        hp: state.health,
                        max_hp: state.max_health,
                        position: b.center,
                        radius: b.radius,
                    })
                }
                SpaceLockTargetKind::None => None,
            },
        )
    }

    /// Immediately updates the latest published frame's own mount/chair/cockpit when a `0x91`
    /// correction arrives, without waiting for the next 20 Hz `BNS1` snapshot.
    pub(crate) fn apply_own_to_latest(
        &self,
        own_team: u8,
        mission: Option<&Mission>,
        history: &mut History,
    ) {
        let Some(frame) = history.latest_mut() else {
            return;
        };
        let (mount, chair, cockpit, fighter_lock, gunner_aim) =
            self.build_own_fields(own_team, mission);
        frame.mount = mount;
        frame.chair = chair;
        frame.cockpit = cockpit;
        frame.fighter_lock = fighter_lock;
        frame.gunner_aim = gunner_aim;
    }

    /// Publishes one complete Space snapshot without a `Mission` view (used by
    /// headless allocation and receiver tests).
    pub fn publish_snapshot_headless(&mut self, tick: u32, own_team: u8, history: &mut History) {
        self.publish_snapshot(tick, own_team, None, history);
    }

    /// Publishes one complete Space snapshot at authority `tick` into the shared [`History`].
    pub(crate) fn publish_snapshot(
        &mut self,
        tick: u32,
        own_team: u8,
        mission: Option<&Mission>,
        history: &mut History,
    ) {
        if tick == 0 || (tick <= self.last_pub_tick && !self.dirty_fields) {
            return;
        }
        self.dirty_fields = false;
        let prev_tick = if self.last_pub_tick > 0 && self.last_pub_tick < tick {
            self.last_pub_tick
        } else {
            tick.saturating_sub(1)
        };
        if !self.frigate_clocks.is_empty() {
            self.world_anims
                .sync_from_clocks(&self.frigate_clocks, tick, None);
        }

        let (mount, chair, cockpit, fighter_lock, gunner_aim) =
            self.build_own_fields(own_team, mission);
        let frame = &mut self.scratch;
        frame.tick = tick.into();
        frame.prev_tick = Some(prev_tick.into());
        frame.labels = Some(self.meta.names.clone());
        frame.radar = self.meta.radar;
        frame.points_visible = mission.map_or([false; 2], Mission::points_shown);
        frame.mission = mission.and_then(|m| m.score(tick.into()));
        frame.mount = mount;
        frame.chair = chair;
        frame.cockpit = cockpit;
        frame.fighter_lock = fighter_lock;
        frame.gunner_aim = gunner_aim;
        frame.systems.clear();

        // 1. Fighters in slot order (`0x0211`).
        frame.hulls.clear();
        for (slot, entry) in self.fighters.iter().enumerate() {
            let Some(f) = entry else {
                self.prev_fighters[slot] = None;
                continue;
            };
            let previous = match self.prev_fighters[slot] {
                Some((gen, prev_pose)) if gen == f.generation => prev_pose,
                _ => f.pose,
            };
            self.prev_fighters[slot] = Some((f.generation, f.pose));
            frame.hulls.push(Hull {
                id: slot,
                generation: f.generation,
                class: self.meta.profile_hashes[f.profile],
                team: f.team,
                previous,
                current: f.pose,
                health: f.health,
                max_health: f.max_health,
                max_speed: f.max_speed,
                thrusting: f.thrusting,
                velocity: f.velocity,
                state: f.state,
                // Boost speed, trick, bank, gear and the pilot flag are not on the wire.
                ..Default::default()
            });
        }

        // 2. Animated frigate / world-anim props (`0x0213`).
        frame.props.clear();
        for (i, obj) in self.world_anims.objects.iter().enumerate() {
            let prev = self
                .prev_prop_matrices
                .get(i)
                .copied()
                .unwrap_or(obj.live_matrix);
            if let Some(slot) = self.prev_prop_matrices.get_mut(i) {
                *slot = obj.live_matrix;
            }
            frame.props.push(Moving {
                instance: obj.prop_instance,
                previous: prev,
                current: obj.live_matrix,
            });
        }
        frame.props.sort_unstable_by_key(|p| p.instance);

        // 3. Ship turret aims (`0x0215` tag 0).
        frame.turrets.clear();
        for (i, &bidx) in self.meta.data.turret_buildings.iter().enumerate() {
            let cur = self.turret_aims[i];
            let prev = self.prev_turret_aims[i];
            self.prev_turret_aims[i] = cur;
            frame.turrets.push(Aim {
                building: bidx,
                previous: prev,
                current: cur,
                alive: true,
                // The gun's class, weapon, rates and fire state are not on the wire.
                ..Default::default()
            });
        }
        frame.turrets.sort_unstable_by_key(|t| t.building);

        // 4. Ordnance guided rounds + launches / impacts / deaths (`0x0214`).
        frame.rounds.clear();
        frame.deaths.clear();
        self.next_guided.clear();
        for g in &self.guided {
            let Some(&weapon) = self.meta.data.weapon_hashes.get(usize::from(g.weapon_idx)) else {
                continue;
            };
            let pos = decode_pos_u24(&self.bounds, g.pos);
            let vel = decode_vel(g.vel, 8.0);
            let prev = self
                .prev_guided
                .iter()
                .find(|(serial, _)| *serial == g.serial)
                .map_or(pos, |(_, p)| *p);
            self.next_guided.push((g.serial, pos));
            frame.rounds.push((
                usize::from(g.slot),
                Round {
                    serial: g.serial.into(),
                    weapon,
                    team: 0,
                    born: 0,
                    tick: tick.into(),
                    origin: pos,
                    previous: prev,
                    position: pos,
                    initial_velocity: vel,
                    velocity: vel,
                    active: true,
                    contact: None,
                    blast: false,
                    source_body: None,
                    barrel: None,
                    ..Default::default()
                },
            ));
        }
        std::mem::swap(&mut self.prev_guided, &mut self.next_guided);

        if self.has_pending_events
            || !self.held_launches.is_empty()
            || !self.held_impacts.is_empty()
            || !self.held_deaths.is_empty()
        {
            // Held events first (each with its own snapshot tick), then this
            // snapshot's; the rings drop what an earlier frame already drew.
            let live = self.has_pending_events;
            self.has_pending_events = false;
            let pending_at = if self.pending_tick == 0 { tick } else { self.pending_tick };
            let launches = self.held_launches.iter().copied().chain(
                self.pending_events.launches.iter().filter(|_| live).map(|l| (pending_at, *l)),
            );
            for (field_tick, l) in launches {
                if !mark_seen_u16(
                    &mut self.seen_launches,
                    &mut self.seen_launch_len,
                    &mut self.seen_launch_at,
                    l.serial,
                ) {
                    continue;
                }
                let Some(&weapon) = self.meta.data.weapon_hashes.get(usize::from(l.weapon_idx))
                else {
                    continue;
                };
                // Every fresh launch belongs to this snapshot window; clamp `born`
                // into `(prev_tick, tick]` so `SpaceEffects` never skips boundary-tick shots.
                let born = field_tick
                    .saturating_sub(u32::from(l.tick_offset))
                    .clamp(prev_tick.saturating_add(1), tick);
                let origin = decode_pos_u24(&self.bounds, l.origin);
                let vel = decode_vel(l.vel, 8.0);
                let serial = l.serial.into();
                if let Some((_, existing)) = frame.rounds.iter_mut().find(|(_, r)| r.serial == serial)
                {
                    existing.born = born.into();
                    existing.origin = origin;
                    existing.initial_velocity = vel;
                } else {
                    frame.rounds.push((
                        (l.serial as usize) % bfe_sim::space::combat::MAX_SPACE_PROJECTILES,
                        Round {
                            serial,
                            weapon,
                            team: i32::from(l.team),
                            born: born.into(),
                            tick: tick.into(),
                            origin,
                            previous: origin,
                            position: origin,
                            initial_velocity: vel,
                            velocity: vel,
                            active: true,
                            contact: None,
                            blast: false,
                            source_body: None,
                            barrel: None,
                            ..Default::default()
                        },
                    ));
                }
            }
            let impacts = self.held_impacts.iter().copied().chain(
                self.pending_events.impacts.iter().filter(|_| live).map(|i| (pending_at, *i)),
            );
            for (field_tick, imp) in impacts {
                if !mark_seen_u16(
                    &mut self.seen_impacts,
                    &mut self.seen_impact_len,
                    &mut self.seen_impact_at,
                    imp.serial,
                ) {
                    continue;
                }
                let Some(&weapon) = self.meta.data.weapon_hashes.get(usize::from(imp.weapon_idx))
                else {
                    continue;
                };
                let hit_tick = field_tick.saturating_sub(u32::from(imp.tick_offset));
                let pos = decode_pos_u24(&self.bounds, imp.pos);
                let normal = imp.oct_normal.map(unpack_oct_normal);
                let serial = imp.serial.into();
                frame.rounds.push((
                    (imp.serial as usize) % bfe_sim::space::combat::MAX_SPACE_PROJECTILES,
                    Round {
                        serial,
                        weapon,
                        team: 0,
                        born: 0,
                        tick: hit_tick.into(),
                        origin: pos,
                        previous: pos,
                        position: pos,
                        initial_velocity: [0.0; 3],
                        velocity: [0.0; 3],
                        active: false,
                        contact: Some(Contact {
                            shield: imp.shield,
                            rigid: imp.rigid,
                            normal,
                            fighter: None,
                            building: None,
                        }),
                        blast: imp.blast,
                        source_body: None,
                        barrel: None,
                        ..Default::default()
                    },
                ));
            }
            let deaths = self.held_deaths.iter().chain(self.pending_events.deaths.iter().filter(|_| live));
            for d in deaths {
                if !mark_seen_death(
                    &mut self.seen_deaths,
                    &mut self.seen_death_len,
                    &mut self.seen_death_at,
                    (d.is_building, d.class_idx, d.pos),
                ) {
                    continue;
                }
                let class = if d.is_building {
                    self.meta
                        .building_by_index
                        .get(&usize::from(d.class_idx))
                        .and_then(|&slot| self.meta.building_hashes.get(slot))
                        .copied()
                } else {
                    self.meta
                        .profile_hashes
                        .get(usize::from(d.class_idx))
                        .copied()
                };
                // A building's own name, as a local frame names it; a craft has none.
                let name = d.is_building.then(|| self.meta.building_by_index.get(&usize::from(d.class_idx)))
                    .flatten()
                    .and_then(|&slot| self.meta.data.buildings.get(slot))
                    .map_or(0, |b| audio_control::swbf2::zero_hash(&b.name));
                if let Some(class) = class {
                    frame.deaths.push(Destruction {
                        class,
                        name,
                        position: decode_pos_u24(&self.bounds, d.pos),
                    });
                }
            }
            self.pending_events.launches.clear();
            self.pending_events.impacts.clear();
            self.pending_events.deaths.clear();
            self.held_launches.clear();
            self.held_impacts.clear();
            self.held_deaths.clear();
            self.pending_tick = 0;
        }

        history.publish(&mut self.scratch);
        self.last_pub_tick = tick;
    }
}

/// 60 Hz client-side own-craft and chair-turret predictor with smooth exponential
/// error correction. Runs on [`Replay`] using the exact same `SpaceFighter::step_tick`,
/// `advance_pilot_stick`, `advance_gunner_aim`, and `advance_chair_aim` as the server.
pub struct SpacePredictor {
    pub meta: Arc<SpaceClientMeta>,
    collision: Collision,
    world: usize,
    flags: Vec<u8>,
    pending_scratch: Vec<(u32, u32, u32, PilotControls)>,
    /// The world animations (the frigates) as the replay places them: the
    /// bodies they move (object, body, local box), the group clocks of the
    /// newest snapshot and the tick the frame shows them at.
    anims: SpaceWorldAnims,
    anim_bodies: Vec<(usize, u32, [f64; 3], [f64; 3])>,
    anim_clocks: Vec<(u8, u32)>,
    anim_shown: u32,
    active_pilot: Option<(u16, u32, u8, u32)>,
    prev_pose: Pose,
    cur_pose: Pose,
    pos_offset: [f64; 3],
    rot_offset: Quaternion,
    gunner_aim: Option<[f64; 3]>,
    airspeed: f64,
    flight_state: SpaceFlightState,
    active_chair: Option<(u16, Chair)>,
    prev_chair_aim: [f64; 2],
    cur_chair_aim: [f64; 2],
    chair_offset: [f64; 2],
    last_own_rev: u32,
    last_replayed_seq: u32,
    last_replayed_tick: u32,
    pub(crate) override_pose: Option<OwnSpaceOverride>,
    pub corrections: u32,
    pub worst_error: f64,
    pub snaps: u32,
}

fn pose_of_fighter(f: &SpaceFighter) -> Pose {
    Pose {
        position: f.position,
        rotation: f.quaternion,
        bank: f.bank,
        gear: f.gear,
    }
}

fn quat_from_arr(q: [f64; 4]) -> Quaternion {
    Quaternion::new(q[0], q[1], q[2], q[3])
}

fn quat_angle(a: &Quaternion, b: &Quaternion) -> f64 {
    let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs().min(1.0);
    2.0 * dot.acos()
}

/// Places the bodies world animations move at their pose of `tick`.
fn place_animated(
    anims: &mut SpaceWorldAnims,
    bodies: &[(usize, u32, [f64; 3], [f64; 3])],
    clocks: &[(u8, u32)],
    tick: u32,
    collision: &mut bfe_sim::collision::State,
) {
    if bodies.is_empty() || clocks.is_empty() {
        return;
    }
    anims.sync_from_clocks(clocks, tick, None);
    for &(object, kernel, lo, hi) in bodies {
        let (Some(o), Some(body)) = (anims.objects.get(object), collision.bodies.get_mut(kernel as usize)) else { continue };
        body.bounds = crate::online::props::transform_aabb(lo, hi, &o.live_matrix);
        body.matrix = o.live_matrix;
        body.inverse = o.live_inverse;
    }
}

fn vec_dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

impl SpacePredictor {
    pub fn new(meta: Arc<SpaceClientMeta>, collision: Collision, world: usize) -> Self {
        let flags_len = collision
            .borrow()
            .state
            .worlds
            .get(world)
            .map_or(64, |w| w.order.len().max(64));
        Self {
            meta,
            collision,
            world,
            flags: vec![0u8; flags_len],
            pending_scratch: Vec::with_capacity(128),
            anims: SpaceWorldAnims::default(),
            anim_bodies: Vec::new(),
            anim_clocks: Vec::with_capacity(MAX_ANIM_GROUPS),
            anim_shown: 0,
            active_pilot: None,
            prev_pose: Pose::default(),
            cur_pose: Pose::default(),
            pos_offset: [0.0; 3],
            rot_offset: Quaternion::default(),
            gunner_aim: None,
            airspeed: 0.0,
            flight_state: SpaceFlightState::Landed,
            active_chair: None,
            prev_chair_aim: [0.0; 2],
            cur_chair_aim: [0.0; 2],
            chair_offset: [0.0; 2],
            last_own_rev: 0,
            last_replayed_seq: 0,
            last_replayed_tick: 0,
            override_pose: None,
            corrections: 0,
            worst_error: 0.0,
            snaps: 0,
        }
    }

    pub fn reset(&mut self) {
        self.active_pilot = None;
        self.active_chair = None;
        self.pos_offset = [0.0; 3];
        self.rot_offset = Quaternion::default();
        self.gunner_aim = None;
        self.chair_offset = [0.0; 2];
        self.override_pose = None;
        self.last_own_rev = 0;
        self.last_replayed_seq = 0;
        self.last_replayed_tick = 0;
    }

    /// Reconciles against the latest `0x91` correction and replays all unacknowledged
    /// commands in `sender.history` on the server's tick schedule through the exact
    /// server flight/terminal math and quantized `PILOT` wire controls.
    /// The bodies of the world-animated buildings, so a replayed craft meets
    /// a frigate where it is at each replayed tick, as the server's does.
    /// The battle's Space buildings' prop instances, in building order.
    pub fn building_instances(&self) -> impl Iterator<Item = usize> + '_ {
        self.meta.data.buildings.iter().map(|b| b.instance)
    }

    pub fn attach_animated_bodies(&mut self, bodies: &[crate::online::props::AnimatedBodies]) {
        self.anims = self.meta.data.world_anims.clone();
        self.anim_bodies.clear();
        for (instance, looks) in bodies {
            let Some(object) = self.anims.objects.iter().position(|o| o.prop_instance == *instance) else { continue };
            self.anim_bodies.extend(looks.iter().flatten().map(|&(body, lo, hi)| (object, body, lo, hi)));
        }
    }

    pub fn frame(&mut self, receiver: &Receiver, sender: &Sender) {
        let Some(space) = receiver.space_online.as_deref() else {
            self.override_pose = None;
            return;
        };
        self.anim_clocks.clear();
        self.anim_clocks.extend_from_slice(&space.frigate_clocks);
        self.anim_shown = space.world_anims.sim_tick;
        let Some(own_body) = &space.own_body else {
            self.active_pilot = None;
            self.active_chair = None;
            self.override_pose = None;
            return;
        };
        let newest_seq = sender.history.newest_sequence();
        let new_correction = space.own_revision != self.last_own_rev;
        if !new_correction && newest_seq == self.last_replayed_seq && self.override_pose.is_some() {
            return;
        }
        let own_gen = receiver.own.generation.max(0.0) as u32;
        let held_pilot = sender
            .history
            .held_pilot(space.own_ack_seq, receiver.own.generation);
        let mut pending = std::mem::take(&mut self.pending_scratch);
        pending.clear();
        pending.extend(sender.history.space_pending(space.own_ack_seq));
        match own_body {
            OwnSpaceBody::Pilot(own) => {
                self.reconcile_pilot(
                    own,
                    i32::from(receiver.own.team),
                    own_gen,
                    space.own_ack_seq,
                    space.own_ack_tick,
                    held_pilot,
                    new_correction,
                    &pending,
                );
            }
            OwnSpaceBody::Chair(chair) => {
                self.reconcile_chair(
                    chair,
                    own_gen,
                    space.own_ack_seq,
                    space.own_ack_tick,
                    held_pilot,
                    new_correction,
                    &pending,
                );
            }
        }
        self.pending_scratch = pending;
        self.last_own_rev = space.own_revision;
        self.last_replayed_seq = newest_seq;
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_pilot(
        &mut self,
        own: &OwnPilotState,
        team: i32,
        own_gen: u32,
        ack_seq: u32,
        ack_tick: u32,
        held_pilot: PilotControls,
        new_correction: bool,
        pending: &[(u32, u32, u32, PilotControls)],
    ) {
        let prof_idx = usize::from(own.profile);
        let pad = self
            .meta
            .data
            .fighter_pad_indices
            .get(usize::from(own.fighter_id))
            .and_then(|&pi| self.meta.data.pads.get(pi))
            .or_else(|| self.meta.pad_for_profile.get(prof_idx));
        let (Some(profile), Some(pad), Some(&class_hash)) = (
            self.meta.data.profiles.get(prof_idx),
            pad,
            self.meta.profile_hashes.get(prof_idx),
        ) else {
            return;
        };
        let identity = (own.fighter_id, own.fighter_gen, own.seat, class_hash);
        let same_craft = self.active_pilot == Some(identity);
        let compare_tick = self.last_replayed_tick;
        let old_visual = same_craft.then(|| {
            let old_pos = std::array::from_fn(|i| self.cur_pose.position[i] + self.pos_offset[i]);
            let mut old_rot = self.rot_offset;
            old_rot.multiply(&quat_from_arr(self.cur_pose.rotation));
            (old_pos, old_rot, self.flight_state)
        });
        self.active_chair = None;

        let mut fighter = restore_fighter_from_own(own, profile, pad, team);
        let mut stick = own.stick;
        let mut boarding_use = own.boarding_use;
        // The double taps go on from the authority's at the acknowledged tick.
        let mut taps = own.taps;
        let mut gunner_angles = own.gunner_angles;
        let mut prev_pose = pose_of_fighter(&fighter);
        let mut at_old_tick = (same_craft && compare_tick != 0 && ack_tick == compare_tick).then(|| {
            (
                fighter.position,
                quat_from_arr(fighter.quaternion),
                fighter.state,
            )
        });
        let mut last_seq = ack_seq;
        let mut gunner_dir = if own.is_gunner {
            let mut tmp = gunner_angles;
            Some(advance_gunner_aim(
                &mut tmp,
                Default::default(),
                NO_AIM_LIMITS,
                fighter.forward(),
                fighter.right(),
                fighter.up(),
                0.0,
            ))
        } else {
            None
        };

        let mut has_mine = false;
        let mut max_cmd_tick = ack_tick;
        for &(cmd_tick, _seq, cmd_gen, _) in pending {
            if cmd_gen == own_gen {
                has_mine = true;
                max_cmd_tick = max_cmd_tick.max(cmd_tick);
            }
        }
        let end_tick = if has_mine {
            max_cmd_tick
                .max(ack_tick.saturating_add(1))
                .min(ack_tick.saturating_add(120))
        } else {
            ack_tick
        };

        let mut collision = self.collision.borrow_mut();
        let needed = collision
            .state
            .worlds
            .get(self.world)
            .map_or(0, |w| w.order.len());
        if self.flags.len() < needed {
            self.flags.resize(needed, 0);
        }
        collision.body_flags_u8(self.world as u32, &mut self.flags[..needed]);

        let mut held = quantize_pilot_controls(held_pilot);
        let mut cursor = 0usize;
        for tick in ack_tick.saturating_add(1)..=end_tick {
            held.clear_edges();
            while cursor < pending.len() && pending[cursor].0 <= tick {
                let (_cmd_tick, seq, cmd_gen, raw_controls) = pending[cursor];
                cursor += 1;
                if cmd_gen != own_gen {
                    continue;
                }
                let mut c = quantize_pilot_controls(raw_controls);
                c.carry_edges(held);
                held = c;
                last_seq = seq;
            }
            let flying = own.seat == 0 && fighter.state == SpaceFlightState::Flying;
            let fl = advance_pilot_stick(&mut stick, &mut boarding_use, &mut taps, flying, held, STEP_DT);
            if own.seat == 0 {
                // The server moves the frigates to this tick before its craft step.
                place_animated(&mut self.anims, &self.anim_bodies, &self.anim_clocks, tick, &mut collision.state);
                prev_pose = pose_of_fighter(&fighter);
                fighter.pending_input = fl;
                fighter.step_tick(
                    STEP_DT,
                    profile,
                    pad,
                    &self.meta.data.bounds,
                    &mut collision,
                    self.world,
                    &mut self.flags,
                );
            } else {
                fighter.extrapolate_attitude(STEP_DT);
                gunner_dir = Some(advance_gunner_aim(
                    &mut gunner_angles,
                    fl,
                    profile.seat_aim_limits[usize::from(own.seat).min(5)],
                    fighter.forward(),
                    fighter.right(),
                    fighter.up(),
                    STEP_DT,
                ));
            }
            if same_craft && compare_tick != 0 && tick == compare_tick {
                at_old_tick = Some((
                    fighter.position,
                    quat_from_arr(fighter.quaternion),
                    fighter.state,
                ));
            }
        }
        // The frame shows the frigates at the newest snapshot again.
        place_animated(&mut self.anims, &self.anim_bodies, &self.anim_clocks, self.anim_shown, &mut collision.state);
        drop(collision);

        let cur_pose = pose_of_fighter(&fighter);
        if new_correction {
            if let Some((old_pos, old_rot, old_state)) = old_visual {
                let (ref_pos, ref_rot, ref_state) = at_old_tick.unwrap_or((
                    cur_pose.position,
                    quat_from_arr(cur_pose.rotation),
                    fighter.state,
                ));
                let err = vec_dist(old_pos, ref_pos);
                let ang = quat_angle(&old_rot, &ref_rot);
                let state_jump = (old_state == SpaceFlightState::Landed)
                    != (ref_state == SpaceFlightState::Landed);
                if state_jump || err > 1e-4 || ang > 1e-4 {
                    self.corrections += 1;
                    self.worst_error = self.worst_error.max(err);
                }
                let snap_m = if ref_state == SpaceFlightState::Flying {
                    (fighter.airspeed * 0.15).clamp(8.0, 30.0)
                } else {
                    8.0
                };
                if state_jump || err > snap_m || ang > ROT_SNAP_RAD {
                    self.pos_offset = [0.0; 3];
                    self.rot_offset = Quaternion::default();
                    self.snaps += 1;
                } else if err > 1e-4 || ang > 1e-4 {
                    self.pos_offset = std::array::from_fn(|i| old_pos[i] - ref_pos[i]);
                    let mut inv = ref_rot;
                    inv.invert();
                    let mut delta = old_rot;
                    delta.multiply(&inv);
                    self.rot_offset = delta;
                } else {
                    self.pos_offset = [0.0; 3];
                    self.rot_offset = Quaternion::default();
                }
            } else {
                self.pos_offset = [0.0; 3];
                self.rot_offset = Quaternion::default();
            }
        } else if !same_craft {
            self.pos_offset = [0.0; 3];
            self.rot_offset = Quaternion::default();
        }
        self.active_pilot = Some(identity);
        self.last_replayed_seq = last_seq;
        self.last_replayed_tick = end_tick;
        self.prev_pose = prev_pose;
        self.cur_pose = cur_pose;
        self.gunner_aim = gunner_dir;
        self.airspeed = fighter.airspeed;
        self.flight_state = fighter.state;
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_chair(
        &mut self,
        chair_state: &OwnChairState,
        own_gen: u32,
        ack_seq: u32,
        ack_tick: u32,
        held_pilot: PilotControls,
        new_correction: bool,
        pending: &[(u32, u32, u32, PilotControls)],
    ) {
        let Some(tmeta) = self
            .meta
            .turrets_by_building
            .get(&usize::from(chair_state.turret_building))
        else {
            return;
        };
        let same_chair = self
            .active_chair
            .is_some_and(|(tb, _)| tb == chair_state.turret_building);
        let compare_tick = self.last_replayed_tick;
        let old_visual = same_chair.then(|| {
            [
                self.cur_chair_aim[0] + self.chair_offset[0],
                self.cur_chair_aim[1] + self.chair_offset[1],
            ]
        });
        self.active_pilot = None;
        self.pos_offset = [0.0; 3];
        self.rot_offset = Quaternion::default();

        let mut aim = [chair_state.turret_yaw, chair_state.turret_pitch];
        let mut prev_aim = aim;
        let mut at_old_tick =
            (same_chair && compare_tick != 0 && ack_tick == compare_tick).then_some(aim);
        let mut has_mine = false;
        let mut max_cmd_tick = ack_tick;
        for &(cmd_tick, _seq, cmd_gen, _) in pending {
            if cmd_gen == own_gen {
                has_mine = true;
                max_cmd_tick = max_cmd_tick.max(cmd_tick);
            }
        }
        let end_tick = if has_mine {
            max_cmd_tick
                .max(ack_tick.saturating_add(1))
                .min(ack_tick.saturating_add(120))
        } else {
            ack_tick
        };

        let mut held = quantize_pilot_controls(held_pilot);
        let mut cursor = 0usize;
        let mut last_seq = ack_seq;
        for tick in ack_tick.saturating_add(1)..=end_tick {
            held.clear_edges();
            while cursor < pending.len() && pending[cursor].0 <= tick {
                let (_cmd_tick, seq, cmd_gen, raw_controls) = pending[cursor];
                cursor += 1;
                if cmd_gen != own_gen {
                    continue;
                }
                let mut c = quantize_pilot_controls(raw_controls);
                c.carry_edges(held);
                held = c;
                last_seq = seq;
            }
            prev_aim = aim;
            advance_chair_aim(
                &mut aim,
                held.turret_look,
                held.turret_turn,
                tmeta.yaw_limits,
                tmeta.pitch_limits,
                tmeta.rates,
                STEP_DT,
            );
            if same_chair && compare_tick != 0 && tick == compare_tick {
                at_old_tick = Some(aim);
            }
        }
        if new_correction {
            if let Some(old) = old_visual {
                let ref_aim = at_old_tick.unwrap_or(aim);
                let dy = (old[0] - ref_aim[0]).sin().atan2((old[0] - ref_aim[0]).cos());
                let dp = old[1] - ref_aim[1];
                if dy.abs() > CHAIR_SNAP_RAD || dp.abs() > CHAIR_SNAP_RAD {
                    self.chair_offset = [0.0; 2];
                    self.snaps += 1;
                    self.corrections += 1;
                } else if dy.abs() > 1e-4 || dp.abs() > 1e-4 {
                    self.chair_offset = [dy, dp];
                    self.corrections += 1;
                } else {
                    self.chair_offset = [0.0; 2];
                }
            } else {
                self.chair_offset = [0.0; 2];
            }
        } else if !same_chair {
            self.chair_offset = [0.0; 2];
        }
        self.active_chair = Some((chair_state.turret_building, tmeta.chair));
        self.last_replayed_seq = last_seq;
        self.last_replayed_tick = end_tick;
        self.prev_chair_aim = prev_aim;
        self.cur_chair_aim = aim;
    }

    /// Exponentially decays visual correction offsets and publishes the interpolated
    /// display override for [`crate::local::flight_view::Fleet::sample`].
    pub fn present(&mut self, alpha: f64, dt: f64) {
        let dt = dt.clamp(0.0, 0.25);
        if let Some((fighter_id, _gen, _seat, class)) = self.active_pilot {
            let tau = if self.flight_state == SpaceFlightState::Flying {
                AIR_TAU
            } else {
                LANDED_TAU
            };
            let decay = (-dt / tau).exp();
            for c in &mut self.pos_offset {
                *c *= decay;
                if c.abs() < 1e-5 {
                    *c = 0.0;
                }
            }
            let ident = Quaternion::default();
            self.rot_offset.slerp(&ident, 1.0 - decay);

            let mut rot = quat_from_arr(self.prev_pose.rotation);
            rot.slerp(&quat_from_arr(self.cur_pose.rotation), alpha);
            let mut smoothed_rot = self.rot_offset;
            smoothed_rot.multiply(&rot);
            let bank = self.prev_pose.bank + alpha * (self.cur_pose.bank - self.prev_pose.bank);
            let mut visual = Quaternion::default();
            visual.set_from_axis_angle(&Vector3::new(0., 0., 1.), bank);
            smoothed_rot.multiply(&visual);

            let pos: [f64; 3] = std::array::from_fn(|i| {
                self.prev_pose.position[i]
                    + alpha * (self.cur_pose.position[i] - self.prev_pose.position[i])
                    + self.pos_offset[i]
            });
            let mut matrix = Matrix4::default();
            matrix.compose(
                &Vector3::new(pos[0], pos[1], pos[2]),
                &smoothed_rot,
                &Vector3::new(1., 1., 1.),
            );
            self.override_pose = Some(OwnSpaceOverride::Pilot {
                fighter_id: usize::from(fighter_id),
                class,
                matrix,
                gunner_aim: self.gunner_aim,
            });
        } else if let Some((_tb, chair)) = self.active_chair {
            let decay = (-dt / CHAIR_TAU).exp();
            for c in &mut self.chair_offset {
                *c *= decay;
                if c.abs() < 1e-5 {
                    *c = 0.0;
                }
            }
            let turret_aim = std::array::from_fn(|i| {
                self.prev_chair_aim[i]
                    + alpha * (self.cur_chair_aim[i] - self.prev_chair_aim[i])
                    + self.chair_offset[i]
            });
            self.override_pose = Some(OwnSpaceOverride::Chair { chair, turret_aim });
        } else {
            self.override_pose = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn predicted_pilot_state(&self) -> Option<(u32, [f64; 3], SpaceFlightState)> {
        self.active_pilot
            .map(|_| (self.last_replayed_tick, self.cur_pose.position, self.flight_state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_sim::space::local::DoubleTapDetector;
    use bfe_sim::space::{
        bounds::SpaceBounds,
        combat::{SpaceDamageCategory, SpaceKick, SpaceShotClock},
        flight::{SpaceFlightProfile, SpacePilotInput, SpaceWeaponSpec},
        objects::{
            SpaceBuildingAttachment, SpaceTurretPilotType, SpaceTurretState, TurretTargetFilter,
        },
        SpaceClientData,
    };
    use bfe_sim::wire::space::write_own_pilot;
    use std::{cell::RefCell, rc::Rc};

    fn test_meta() -> (Arc<SpaceClientMeta>, Collision) {
        let mut profile = SpaceFlightProfile::default();
        profile.class_name = "test_fighter".into();
        profile.max_health = 1000.0;
        profile.max_speed = 105.0;
        profile.boost_speed = 170.0;
        profile.min_speed = 35.0;
        profile.acceleration = 50.0;
        profile.boost_acceleration = 80.0;
        profile.energy_bar = 100.0;
        profile.energy_auto_restore = 10.0;
        profile.energy_boost_drain = 15.0;
        profile.turn_rate = 1.25;
        profile.pitch_rate = 1.25;
        profile.roll_rate = 2.0;

        let pad = SpaceVehiclePad {
            name: "pad0".into(),
            team: 1,
            profile_index: 0,
            control_zone: String::new(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            spawn_count: 1,
            spawn_time: 8.0,
            expire_time_enemy: 20.0,
            expire_time_field: 40.0,
            decay_time: 10.0,
            respawn_timer: 8.0,
            floor_y: 0.0,
        };

        let turret = SpaceTurretState {
            pilot_type: SpaceTurretPilotType::Remote,
            weapon: SpaceWeaponSpec::default(),
            filter: TurretTargetFilter::default(),
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -1.5,
            yaw_max: 1.5,
            pitch_min: -0.6,
            pitch_max: 0.8,
            turn_rate: 1.2,
            pitch_rate: 1.0,
            clock: SpaceShotClock::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: SpaceKick::default(),
            trigger_was_held: false,
        };

        let bld = SpaceBuildingAttachment {
            building_index: 7,
            name: "turret0".into(),
            class: "test_turret".into(),
            base: "armedbuilding".into(),
            instance: 7,
            bodies: [None; 3],
            animated_index: None,
            matrix: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 5., -20., 1.,
            ],
            position: [10., 5., -20.],
            center: [10., 5., -20.],
            local_center_offset: [0., 1., 0.],
            radius: 3.0,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: Some(turret),
            terminal: None,
        };

        let mut bounds = SpaceBounds::from_world_json(&bfe_content::Value::Null);
        bounds.min = [-4000.0; 3];
        bounds.max = [4000.0; 3];

        let data = SpaceClientData {
            bounds,
            world_anims: SpaceWorldAnims::default(),
            profiles: vec![profile.clone()],
            pads: vec![pad.clone()],
            fighter_pad_indices: vec![0],
            fighter_slots: 1,
            buildings: vec![bld],
            turret_buildings: vec![7],
            doors: Vec::new(),
            weapon_hashes: Vec::new(),
        };
        let names = Arc::new(Names::from_parts(&data.profiles, &data.buildings, None));
        let counts = Counts::from_client_data(&data);
        let mut turrets_by_building = BTreeMap::new();
        let b = &data.buildings[0];
        let t = b.turret.as_ref().unwrap();
        turrets_by_building.insert(
            7,
            TurretMeta {
                chair: Chair {
                    instance: 7,
                    building: 7,
                    class: zero_hash(&b.class),
                    matrix: b.matrix,
                    center: b.local_center_offset,
                    radius: b.radius,
                },
                yaw_limits: [t.yaw_min, t.yaw_max],
                pitch_limits: [t.pitch_min, t.pitch_max],
                rates: [t.turn_rate, t.pitch_rate],
            },
        );
        let mut building_by_index = BTreeMap::new();
        building_by_index.insert(7, 0);
        let meta = Arc::new(SpaceClientMeta {
            profile_hashes: vec![zero_hash(&data.profiles[0].class_name)],
            building_hashes: vec![zero_hash(&data.buildings[0].class)],
            names,
            counts,
            radar: [0., 0., 4000.],
            turrets_by_building,
            building_by_index,
            pad_for_profile: vec![pad],
            data,
        });
        let mut col = Box::new(CollisionOwner::new());
        let _ = make_empty_world(&mut col);
        (meta, Rc::new(RefCell::new(col)))
    }

    fn make_empty_world(coll: &mut CollisionOwner) -> usize {
        use bfe_content::install::CollisionKernel;
        let empty_bvh =
            bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&[]), &[]);
        let empty_pos = coll.positions(&[]);
        let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
        coll.world_new(empty_tree, -1, -1) as usize
    }

    #[test]
    fn clean_rtt_flight_prediction_matches_server_to_zero_error() {
        for (rtt_ticks, boost) in [(6usize, false), (12usize, true)] {
            let (meta, collision) = test_meta();
            let mut predictor = SpacePredictor::new(meta.clone(), collision.clone(), 0);
            let profile = &meta.data.profiles[0];
            let pad = &meta.data.pads[0];

            // Authoritative server fighter starts flying at 105 m/s (or 170 m/s under boost).
            let mut server_f =
                SpaceFighter::spawned_on_pad(0, 1, 0, pad, 0, profile, 0.0, None, None);
            server_f.state = SpaceFlightState::Flying;
            server_f.airspeed = if boost { 170.0 } else { 105.0 };
            server_f.velocity = [0.0, 0.0, server_f.airspeed];
            server_f.pilot = Some(bfe_sim::space::flight::SpacePilotId {
                owner: bfe_sim::store::Ref::new(0, 1),
                team: 1,
                is_human: true,
                repair_capable: false,
            });
            let mut server_stick = [0.0; 2];
            let mut server_boarding_use = false;
            let mut srv_col = CollisionOwner::new();
            let _ = make_empty_world(&mut srv_col);
            let mut srv_flags = vec![0u8; 8];

            let mut history: Vec<(u32, u32, u32, PilotControls)> = Vec::new();
            let mut wire_queue: Vec<(u32, u32, PilotControls, OwnPilotState)> = Vec::new();

            for tick in 1..=90u32 {
                let controls = PilotControls {
                    mouse: [0.08 * (f64::from(tick) * 0.1).sin(), 0.04],
                    flight: SpacePilotInput {
                        throttle: 1.0,
                        roll: 0.4,
                        boost,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                history.push((tick, tick, 1, controls));

                // Server executes wire-quantized controls immediately and queues 0x91 snapshot delayed by RTT.
                let server_controls = quantize_pilot_controls(controls);
                let fl = advance_pilot_stick(
                    &mut server_stick,
                    &mut server_boarding_use,
                    &mut DoubleTapDetector::default(),
                    false,
                    server_controls,
                    STEP_DT,
                );
                server_f.pending_input = fl;
                server_f.step_tick(
                    STEP_DT,
                    profile,
                    pad,
                    &meta.data.bounds,
                    &mut srv_col,
                    0,
                    &mut srv_flags,
                );
                let mut buf = Vec::new();
                write_own_pilot(
                    &server_f,
                    profile,
                    0,
                    None,
                    server_stick,
                    server_boarding_use,
                    [0.0; 2],
                    false,
                    &bfe_sim::space::local::DoubleTapDetector::default(),
                    &mut buf,
                )
                .unwrap();
                let (_, body) = split_revision(&buf).unwrap();
                let OwnSpaceBody::Pilot(own) = read_own_body(body).unwrap() else {
                    unreachable!()
                };
                wire_queue.push((tick, tick, controls, own));

                if wire_queue.len() > rtt_ticks {
                    let (ack_tick, ack_seq, ack_controls, delivered) = wire_queue.remove(0);
                    let pending: Vec<_> = history
                        .iter()
                        .copied()
                        .filter(|(_, s, _, _)| *s > ack_seq)
                        .collect();
                    predictor.reconcile_pilot(
                        &delivered,
                        1,
                        1,
                        ack_seq,
                        ack_tick,
                        ack_controls,
                        true,
                        &pending,
                    );
                    predictor.present(1.0, STEP_DT);
                    let err = vec_dist(predictor.cur_pose.position, server_f.position);
                    assert!(
                        err < 1e-6,
                        "RTT={rtt_ticks} boost={boost} tick={tick}: predicted error {err} m must be 0"
                    );
                }
            }
            assert_eq!(predictor.snaps, 0);
            assert_eq!(predictor.corrections, 0);
            assert!(predictor.worst_error < 1e-6);
        }
    }

    #[test]
    fn dropped_packet_smooths_without_snap_and_large_discontinuity_snaps() {
        let (meta, collision) = test_meta();
        let mut predictor = SpacePredictor::new(meta.clone(), collision, 0);
        let profile = &meta.data.profiles[0];
        let pad = &meta.data.pads[0];
        let mut f = SpaceFighter::spawned_on_pad(0, 1, 0, pad, 0, profile, 0.0, None, None);
        f.state = SpaceFlightState::Flying;
        f.airspeed = 170.0;
        f.position = [100.0, 20.0, 300.0];

        let encode_own = |f: &SpaceFighter| {
            let mut buf = Vec::new();
            write_own_pilot(f, profile, 0, None, [0.0; 2], false, [0.0; 2], false, &bfe_sim::space::local::DoubleTapDetector::default(), &mut buf)
                .unwrap();
            let (_, body) = split_revision(&buf).unwrap();
            let OwnSpaceBody::Pilot(own) = read_own_body(body).unwrap() else {
                unreachable!()
            };
            own
        };

        let own1 = encode_own(&f);
        predictor.reconcile_pilot(&own1, 1, 1, 1, 1, PilotControls::default(), true, &[]);
        predictor.present(1.0, STEP_DT);

        // 4 m discrepancy at 170 m/s (< 25.5 m dynamic threshold) smooths without snapping.
        f.position[0] += 4.0;
        let own2 = encode_own(&f);
        predictor.reconcile_pilot(&own2, 1, 1, 2, 2, PilotControls::default(), true, &[]);
        assert_eq!(predictor.snaps, 0);
        assert!((predictor.pos_offset[0] + 4.0).abs() < 1e-6);
        for _ in 0..18 {
            predictor.present(1.0, STEP_DT);
        }
        assert!(predictor.pos_offset[0].abs() < 0.25);

        // 40 m teleport (> 30 m ceiling) triggers an immediate hard snap.
        f.position[0] += 40.0;
        let own3 = encode_own(&f);
        predictor.reconcile_pilot(&own3, 1, 1, 3, 3, PilotControls::default(), true, &[]);
        assert_eq!(predictor.snaps, 1);
        assert_eq!(predictor.pos_offset, [0.0; 3]);
    }

    #[test]
    fn chair_turret_rtt_prediction_matches_server_exactly() {
        let (meta, collision) = test_meta();
        let mut predictor = SpacePredictor::new(meta.clone(), collision, 0);
        let tmeta = meta.turrets_by_building[&7];
        let mut server_aim = [0.0, 0.0];
        let mut history = Vec::new();
        let mut queue = Vec::new();
        for tick in 1..=40u32 {
            let controls = PilotControls {
                turret_look: [0.03, -0.015],
                ..Default::default()
            };
            history.push((tick, tick, 1u32, controls));
            let quantized = quantize_pilot_controls(controls);
            advance_chair_aim(
                &mut server_aim,
                quantized.turret_look,
                quantized.turret_turn,
                tmeta.yaw_limits,
                tmeta.pitch_limits,
                tmeta.rates,
                STEP_DT,
            );
            queue.push((
                tick,
                tick,
                controls,
                OwnChairState {
                    chair_building: 6,
                    turret_building: 7,
                    aim: server_aim,
                    feet: [0.0; 3],
                    turret_yaw: server_aim[0],
                    turret_pitch: server_aim[1],
                    cooldown: 0.0,
                    heat: 0.0,
                    overheated: false,
                    shots_fired: 0,
                },
            ));
            if queue.len() > 12 {
                let (ack_tick, ack_seq, ack_controls, st) = queue.remove(0);
                let pending: Vec<_> = history
                    .iter()
                    .copied()
                    .filter(|(_, s, _, _)| *s > ack_seq)
                    .collect();
                predictor.reconcile_chair(&st, 1, ack_seq, ack_tick, ack_controls, true, &pending);
                predictor.present(1.0, STEP_DT);
                assert!((predictor.cur_chair_aim[0] - server_aim[0]).abs() < 1e-12);
                assert!((predictor.cur_chair_aim[1] - server_aim[1]).abs() < 1e-12);
            }
        }
        assert_eq!(predictor.snaps, 0);
        assert_eq!(predictor.corrections, 0);
    }

    #[test]
    fn tick_scheduled_prediction_handles_gaps_merges_quantization_and_gunner_turn() {
        let (meta, collision) = test_meta();
        let mut predictor = SpacePredictor::new(meta.clone(), collision, 0);
        let profile = &meta.data.profiles[0];
        let pad = &meta.data.pads[0];

        // Server fighter starts flying; simulate a 6-tick window with:
        // - tick 11: gap (no command stamped -> repeats held_pilot without edges)
        // - tick 12: TWO commands stamped for tick 12 (merged via carry_edges, oversized mouse > 4 clamped)
        // - tick 13: gap (repeats tick 12's held throttle/roll without edges)
        // - tick 14: normal single command
        let mut srv = SpaceFighter::spawned_on_pad(0, 1, 0, pad, 0, profile, 0.0, None, None);
        srv.state = SpaceFlightState::Flying;
        srv.airspeed = 100.0;
        srv.velocity = [0.0, 0.0, 100.0];
        srv.pilot = Some(bfe_sim::space::flight::SpacePilotId {
            owner: bfe_sim::store::Ref::new(0, 1),
            team: 1,
            is_human: true,
            repair_capable: false,
        });
        let mut buf = Vec::new();
        write_own_pilot(&srv, profile, 0, None, [0.0; 2], false, [0.0; 2], false, &bfe_sim::space::local::DoubleTapDetector::default(), &mut buf).unwrap();
        let (_, body) = split_revision(&buf).unwrap();
        let OwnSpaceBody::Pilot(own0) = read_own_body(body).unwrap() else {
            unreachable!()
        };

        let held_ack = PilotControls {
            mouse: [0.5, 0.0], // edge from tick 10: must NOT repeat on gap tick 11
            flight: SpacePilotInput {
                throttle: 0.75,
                roll: 0.25,
                ..Default::default()
            },
            ..Default::default()
        };
        let cmd12a = PilotControls {
            mouse: [6.0, -0.2], // > 4.0: clamped to 4.0 by PILOT wire codec!
            flight: SpacePilotInput {
                throttle: 0.5,
                trick_roll: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let cmd12b = PilotControls {
            mouse: [0.1, 0.0],
            flight: SpacePilotInput {
                throttle: 1.0,
                roll: -0.3,
                ..Default::default()
            },
            ..Default::default()
        };
        let cmd14 = PilotControls {
            flight: SpacePilotInput {
                throttle: 0.9,
                stick_x: 0.4,
                stick_y: -0.2,
                ..Default::default()
            },
            ..Default::default()
        };
        let pending = vec![
            (12u32, 11u32, 1u32, cmd12a),
            (12u32, 12u32, 1u32, cmd12b),
            (14u32, 13u32, 1u32, cmd14),
        ];

        // Step authoritative server using exact InputMailbox::take / Mailbox::take_command schedule.
        let mut srv_col = CollisionOwner::new();
        let _ = make_empty_world(&mut srv_col);
        let mut srv_flags = vec![0u8; 8];
        let mut srv_stick = [0.0; 2];
        let mut srv_boarding = false;
        let mut cur = quantize_pilot_controls(held_ack);
        for tick in 11..=14u32 {
            cur.clear_edges();
            for &(t, _, _, raw) in &pending {
                if t == tick {
                    let mut q = quantize_pilot_controls(raw);
                    q.carry_edges(cur);
                    cur = q;
                }
            }
            let fl = advance_pilot_stick(&mut srv_stick, &mut srv_boarding, &mut DoubleTapDetector::default(), false, cur, STEP_DT);
            srv.pending_input = fl;
            srv.step_tick(STEP_DT, profile, pad, &meta.data.bounds, &mut srv_col, 0, &mut srv_flags);
        }

        predictor.reconcile_pilot(&own0, 1, 1, 10, 10, held_ack, true, &pending);
        let err = vec_dist(predictor.cur_pose.position, srv.position);
        assert!(err < 1e-9, "tick-scheduled replay diverged by {err} m");

        // Gunner seat: craft yawing at 1.0 rad/s rotates gunner world-space aim across RTT.
        let mut gun_own = own0;
        gun_own.is_gunner = true;
        gun_own.seat = 1;
        gun_own.yaw_rate = 1.0;
        let gun_pending: Vec<_> = (11..=22u32)
            .map(|t| (t, t, 1u32, PilotControls::default()))
            .collect();
        predictor.reset();
        predictor.reconcile_pilot(&gun_own, 1, 1, 10, 10, PilotControls::default(), true, &gun_pending);
        let aim = predictor.gunner_aim.expect("gunner world aim");
        // Without attitude extrapolation, forward is [0,0,1]; with yaw_rate=1.0 for 0.2s, x rotates away from 0.
        assert!(aim[0].abs() > 0.15, "gunner aim {aim:?} did not follow turning craft");
    }

    #[test]
    fn a_craft_seat_is_seated_and_a_chair_operator_is_not() {
        let (meta, _) = test_meta();
        let mut online = OnlineSpace::new(meta, PositionBounds::default());
        let seat = |unit_slot, is_chair| SeatEntry { unit_slot, is_chair, seat: 0, mount_index: 0 };
        let mut payload = REVISION.to_le_bytes().to_vec();
        bfe_sim::wire::space::write_seats(&[seat(4, false), seat(9, true)], &mut payload).unwrap();
        online.receive_field(FIGHTER_SCHEMA, 256, &payload).unwrap();
        assert!(online.is_seated(4) && !online.is_seated(9) && !online.is_seated(5));
        online.receive_field(FIGHTER_SCHEMA, 256, &[]).unwrap();
        assert!(!online.is_seated(4));
    }

    #[test]
    fn online_door_collision_matches_server_passage_blocked_rule() {
        use bfe_sim::space::objects::SpaceDoor;
        let (base_meta, _) = test_meta();
        let mut meta_owned = Arc::try_unwrap(base_meta).ok().unwrap();
        meta_owned.data.doors = vec![
            SpaceDoor {
                id: 0,
                instance: 10,
                name: "door_solid".into(),
                class: "door_class".into(),
                body: None,
                matrix: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.],
                position: [0.; 3],
                center: [0.; 3],
                door_trigger_radius: 4.0,
                trigger_position: [0.; 3],
                opening_clip: None,
                door_open: false,
                prev_door_open: false,
                previous_fraction: 0.0,
                open_fraction: 0.0,
                locked: false,
                collidable: true,
                visible: true,
            },
            SpaceDoor {
                id: 1,
                instance: 11,
                name: "door_noncollidable".into(),
                class: "door_class".into(),
                body: None,
                matrix: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.],
                position: [0.; 3],
                center: [0.; 3],
                door_trigger_radius: 4.0,
                trigger_position: [0.; 3],
                opening_clip: None,
                door_open: false,
                prev_door_open: false,
                previous_fraction: 0.0,
                open_fraction: 0.0,
                locked: false,
                collidable: false,
                visible: true,
            },
        ];
        let meta = Arc::new(meta_owned);
        let mut online = OnlineSpace::new(meta, PositionBounds::default());

        let send_doors = |online: &mut OnlineSpace, d0: (u8, u8), d1: (u8, u8)| {
            let mut payload = REVISION.to_le_bytes().to_vec();
            payload.extend([1u8, 2u8, d0.0, d0.1, d1.0, d1.1]);
            assert!(online.receive_field(PROPS_SCHEMA, 1, &payload).unwrap());
        };

        // Opening door at 0.25 (64/255, flags=0x02 opening) STILL blocks passage (< 0.5).
        send_doors(&mut online, (64, 0x02), (0, 0x01));
        assert!(online.door_states[0].collidable, "opening door below 0.5 must block");
        assert!(!online.door_states[1].collidable, "non-collidable door must never block");

        // Locked door at 0.75 (192/255, flags=0x01 locked) STILL blocks passage.
        send_doors(&mut online, (192, 0x01), (64, 0x03));
        assert!(online.door_states[0].collidable, "locked door above 0.5 must block");
        assert!(!online.door_states[1].collidable);

        // Unlocked door at 0.75 (192/255, flags=0x02 opening) clears passage.
        send_doors(&mut online, (192, 0x02), (192, 0x00));
        assert!(!online.door_states[0].collidable, "unlocked door above 0.5 must clear");
    }

    #[test]
    fn online_cockpit_uses_shared_builder_for_ship_object_health_team_and_craft_life() {
        use bfe_sim::space::flight::{SpaceLockState, SpaceLockTargetKind};
        use bfe_sim::wire::mission as wire_m;
        let (base_meta, _) = test_meta();
        let mut meta_owned = Arc::try_unwrap(base_meta).ok().unwrap();
        meta_owned.data.profiles[0].weapons = vec![
            SpaceWeaponSpec {
                name: "w_primary".into(),
                seat: 0,
                channel: 0,
                lock_time: 1.0,
                ..Default::default()
            },
            SpaceWeaponSpec {
                name: "w_missile".into(),
                seat: 0,
                channel: 1,
                lock_time: 2.0,
                ..Default::default()
            },
        ];
        meta_owned.names = Arc::new(Names::from_parts(
            &meta_owned.data.profiles,
            &meta_owned.data.buildings,
            None,
        ));
        let meta = Arc::new(meta_owned);
        let mut online = OnlineSpace::new(meta.clone(), PositionBounds::default());
        let mut history = History::new(meta.counts);

        // Install a replicated critical system on building index 7 with real HP & team 2.
        let roster = wire_m::Roster {
            teams: [
                wire_m::Team { name: "t1".into(), icons: vec![], classes: vec![] },
                wire_m::Team { name: "t2".into(), icons: vec![], classes: vec![] },
            ],
        };
        let mut mission = Mission::new(&roster, |_| None);
        let mut sys_payload = wire_m::REVISION.to_le_bytes().to_vec();
        wire_m::write_system(
            &wire_m::System {
                index: 0,
                alive: true,
                marker: true,
                team: 2,
                placed: wire_m::Placed::Building(7),
                points: 20,
                health: Some([450.0, 1200.0]),
                shield: None,
                hud: None,
                name: "sys0",
            },
            &mut sys_payload,
        );
        mission.install(wire_m::SYSTEM_SCHEMA, 0, &sys_payload).unwrap();

        let profile = &meta.data.profiles[0];
        let pad = &meta.data.pads[0];
        let mut f = SpaceFighter::spawned_on_pad(0, 1, 0, pad, 0, profile, 0.0, None, None);
        f.weapon_locks[1] = SpaceLockState {
            weapon_index: 1,
            kind: SpaceLockTargetKind::Object,
            target_index: 7,
            target_generation: 3,
            progress: 1.0,
            locked: false,
            lost_los_time: 0.0,
        };
        let mut own_buf = Vec::new();
        write_own_pilot(&f, profile, 0, Some(1), [0.; 2], false, [0.; 2], false, &bfe_sim::space::local::DoubleTapDetector::default(), &mut own_buf).unwrap();
        online.receive_own(&own_buf, 1, 10).unwrap();
        online.publish_snapshot(10, 1, Some(&mission), &mut history);
        let target = history.latest().unwrap().cockpit.unwrap().target.expect("ship-object target");
        assert_eq!(target.team, 2);
        assert_eq!((target.hp, target.max_hp), (450.0, 1200.0));
        assert!((target.fraction - 0.5).abs() < 1e-5);

        // Once the ship object is destroyed, the cockpit target clears.
        let mut dead_sys = wire_m::REVISION.to_le_bytes().to_vec();
        wire_m::write_system(
            &wire_m::System {
                index: 0,
                alive: false,
                marker: true,
                team: 2,
                placed: wire_m::Placed::Building(7),
                points: 20,
                health: Some([0.0, 1200.0]),
                shield: None,
                hud: None,
                name: "sys0",
            },
            &mut dead_sys,
        );
        mission.install(wire_m::SYSTEM_SCHEMA, 0, &dead_sys).unwrap();
        online.dirty_fields = true;
        online.publish_snapshot(13, 1, Some(&mission), &mut history);
        assert!(history.latest().unwrap().cockpit.unwrap().target.is_none());

        // Craft lock checks fighter liveness and generation match.
        online.fighters[0] = Some(FighterWireState {
            generation: 5,
            profile: 0,
            team: 2,
            state: SpaceFlightState::Flying,
            thrusting: true,
            pose: Pose::default(),
            velocity: [0.; 3],
            health: 250.0,
            max_health: 1000.0,
            max_speed: 105.0,
        });
        f.weapon_locks[1] = SpaceLockState {
            weapon_index: 1,
            kind: SpaceLockTargetKind::Fighter,
            target_index: 0,
            target_generation: 99, // wrong life!
            progress: 2.0,
            locked: true,
            lost_los_time: 0.0,
        };
        own_buf.clear();
        write_own_pilot(&f, profile, 0, Some(1), [0.; 2], false, [0.; 2], false, &bfe_sim::space::local::DoubleTapDetector::default(), &mut own_buf).unwrap();
        online.receive_own(&own_buf, 2, 16).unwrap();
        online.dirty_fields = true;
        online.publish_snapshot(16, 1, Some(&mission), &mut history);
        assert!(
            history.latest().unwrap().cockpit.unwrap().target.is_none(),
            "stale fighter generation must not resolve as a live cockpit target"
        );
    }

    #[test]
    fn two_window_space_events_survive_lost_snapshot_and_deduplicate_clean_stream() {
        let (base_meta, _) = test_meta();
        let mut meta_owned = Arc::try_unwrap(base_meta).ok().unwrap();
        meta_owned.data.weapon_hashes = vec![0x1234_5678];
        let meta = Arc::new(meta_owned);
        let bounds = PositionBounds::default();

        // Build one 0x0214 tag 1 event payload with 1 launch (tick_offset=2), 1 impact (tick_offset=1), and 1 fighter death.
        let make_events = |launch_off: u8, impact_off: u8| {
            let mut payload = REVISION.to_le_bytes().to_vec();
            payload.extend([1u8, 1, 1, 1]); // tag=1, launches=1, impacts=1, deaths=1
            // Launch: serial=42, weapon_idx=0, flags=tick_offset | team 1 (1<<4), origin=[0,0,0], vel=[0,0,100]
            payload.extend(42u16.to_le_bytes());
            payload.extend([0u8, (launch_off & 7) | (1u8 << 4)]);
            payload.extend([0u8; 9]); // origin u24*3
            payload.extend([0i16, 0, 800].into_iter().flat_map(i16::to_le_bytes));
            // Impact: serial=42, weapon_idx=0, flags=tick_offset | blast (1<<6), pos=[0,0,0]
            payload.extend(42u16.to_le_bytes());
            payload.extend([0u8, (impact_off & 7) | (1u8 << 6)]);
            payload.extend([0u8; 9]);
            // Death: class_idx=0, is_building=0, pos=[1,2,3]
            payload.extend(0u16.to_le_bytes());
            payload.push(0u8);
            payload.extend([1u8, 0, 0, 2, 0, 0, 3, 0, 0]);
            payload
        };
        let payload_pub1 = make_events(2, 1);
        let payload_pub2 = make_events(5, 4);

        // Clean stream receiving the carried-over event in two consecutive publications
        // emits each launch, impact, and death ONCE (deduplicated on the second window).
        let mut clean = OnlineSpace::new(meta.clone(), bounds);
        let mut clean_hist = History::new(meta.counts);
        clean.publish_snapshot(7, 1, None, &mut clean_hist);
        clean.receive_field(ORDNANCE_SCHEMA, 1, &payload_pub1).unwrap();
        clean.publish_snapshot(10, 1, None, &mut clean_hist);
        let f1 = clean_hist.latest().unwrap();
        assert_eq!(f1.rounds.len(), 2, "first publication must emit 1 launch + 1 impact");
        assert_eq!(f1.deaths.len(), 1, "first publication must emit 1 death");
        assert_eq!(f1.rounds[0].1.born, 8);
        assert_eq!(f1.rounds[1].1.tick, 9);

        clean.receive_field(ORDNANCE_SCHEMA, 1, &payload_pub2).unwrap();
        clean.publish_snapshot(13, 1, None, &mut clean_hist);
        let f2 = clean_hist.latest().unwrap();
        assert_eq!(f2.rounds.len(), 0, "carried-over launch/impact must not duplicate");
        assert_eq!(f2.deaths.len(), 0, "carried-over death must not duplicate");

        // Lossy stream dropping publication 1 still receives the entire event on publication 2
        // with its exact birth/impact tick reconstructed via 0..=5 tick_offset.
        let mut lossy = OnlineSpace::new(meta.clone(), bounds);
        let mut lossy_hist = History::new(meta.counts);
        lossy.publish_snapshot(7, 1, None, &mut lossy_hist);
        lossy.receive_field(ORDNANCE_SCHEMA, 1, &payload_pub2).unwrap();
        lossy.publish_snapshot(13, 1, None, &mut lossy_hist);
        let recovered = lossy_hist.latest().unwrap();
        assert_eq!(recovered.rounds.len(), 2, "dropped first snapshot must recover launch + impact");
        assert_eq!(recovered.rounds[0].1.born, 8, "carried launch tick_offset=5 must reconstruct exact tick 8");
        assert_eq!(recovered.rounds[1].1.tick, 9, "carried impact tick_offset=4 must reconstruct exact tick 9");
        assert_eq!(recovered.deaths.len(), 1, "dropped first snapshot must recover death");
    }

    #[test]
    fn events_of_snapshots_that_never_complete_are_drawn_once_with_their_own_tick() {
        let (base_meta, _) = test_meta();
        let mut meta_owned = Arc::try_unwrap(base_meta).ok().unwrap();
        meta_owned.data.weapon_hashes = vec![0x0bad_cafe];
        let meta = Arc::new(meta_owned);
        // One events field: launches and impacts as (serial, tick offset),
        // destruction bursts by their first position code.
        let field = |launches: &[(u16, u8)], impacts: &[(u16, u8)], deaths: &[u8]| {
            let mut payload = REVISION.to_le_bytes().to_vec();
            payload.extend([1u8, launches.len() as u8, impacts.len() as u8, deaths.len() as u8]);
            for &(serial, off) in launches {
                payload.extend(serial.to_le_bytes());
                payload.extend([0u8, off | (2u8 << 4)]);
                payload.extend([0u8; 9]);
                payload.extend([0i16, 120, 0].into_iter().flat_map(i16::to_le_bytes));
            }
            for &(serial, off) in impacts {
                payload.extend(serial.to_le_bytes());
                payload.extend([0u8, off]);
                payload.extend([0u8; 9]);
            }
            for &at in deaths {
                payload.extend(0u16.to_le_bytes());
                payload.push(0u8);
                payload.extend([at, 0, 0, 0, 0, 0, 0, 0, 0]);
            }
            payload
        };
        let mut space = OnlineSpace::new(meta.clone(), PositionBounds::default());
        let mut history = History::new(meta.counts);
        space.publish_snapshot(7, 1, None, &mut history);
        // Snapshots 10 and 13 deliver their events part but never complete
        // (another part or the own datagram is lost); 16 completes.
        space.receive_field_at(10, ORDNANCE_SCHEMA, 1, &field(&[(50, 1)], &[(50, 0)], &[3])).unwrap();
        space.receive_field_at(13, ORDNANCE_SCHEMA, 1, &field(&[(50, 4), (51, 2)], &[(50, 3), (51, 1)], &[3])).unwrap();
        space.receive_field_at(16, ORDNANCE_SCHEMA, 1, &field(&[(51, 5), (52, 0)], &[(51, 4)], &[9])).unwrap();
        space.publish_snapshot(16, 1, None, &mut history);
        let frame = history.latest().unwrap();
        let launch = |serial: u32| {
            let found: Vec<_> = frame.rounds.iter().filter(|(_, r)| r.active && r.serial as u32 == serial).collect();
            assert_eq!(found.len(), 1, "launch {serial} drawn once");
            found[0].1.born
        };
        let impact = |serial: u32| {
            let found: Vec<_> = frame.rounds.iter().filter(|(_, r)| !r.active && r.serial as u32 == serial).collect();
            assert_eq!(found.len(), 1, "impact {serial} drawn once");
            found[0].1.tick
        };
        assert_eq!([launch(50), launch(51), launch(52)], [9, 11, 16]);
        assert_eq!([impact(50), impact(51)], [10, 12]);
        assert_eq!(frame.deaths.len(), 2, "each burst drawn once");
        // Nothing is held for the next frame.
        space.publish_snapshot(19, 1, None, &mut history);
        let next = history.latest().unwrap();
        assert!(next.rounds.is_empty() && next.deaths.is_empty());
    }
}
