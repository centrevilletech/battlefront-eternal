//! Optional local and wire presentation samples for Space projectiles and bursts.
use super::combat::{EmittingBody, SpaceProjectile};
use super::flight::SpaceOrdnanceKind;

#[derive(Clone, Copy, Debug, Default)]
pub struct Contact {
    pub normal: Option<[f64; 3]>,
    pub shield: bool,
    pub rigid: bool,
    pub fighter: Option<usize>,
    pub building: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Round {
    pub serial: u64,
    pub weapon: u32,
    pub barrel: Option<usize>,
    pub source_body: Option<u32>,
    pub emitter: Option<EmittingBody>,
    pub team: i32,
    pub born: u32,
    pub tick: u32,
    pub origin: [f64; 3],
    pub initial_velocity: [f64; 3],
    pub previous: [f64; 3],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub active: bool,
    pub contact: Option<Contact>,
    pub blast: bool,
    pub tracer: bool,
}

/// Bounded wire launch observation captured when a Space weapon fires.
#[derive(Clone, Copy, Debug, Default)]
pub struct WireLaunch {
    pub tick: u32,
    pub serial: u16,
    pub weapon_hash: u32,
    pub is_beam: bool,
    pub team: u8,
    pub is_human: bool,
    pub origin: [f64; 3],
    pub velocity: [f64; 3],
    pub published_once: bool,
}

/// Bounded wire impact observation captured when a Space round hits or detonates.
#[derive(Clone, Copy, Debug, Default)]
pub struct WireImpact {
    pub tick: u32,
    pub serial: u16,
    pub weapon_hash: u32,
    pub shield: bool,
    pub rigid: bool,
    pub blast: bool,
    pub is_human: bool,
    pub position: [f64; 3],
    pub normal: Option<[f64; 3]>,
    pub published_once: bool,
}

/// Bounded wire destruction burst observation for fighters and Space buildings.
#[derive(Clone, Copy, Debug, Default)]
pub struct WireDeath {
    pub tick: u32,
    pub is_building: bool,
    pub class_idx: u16,
    pub position: [f64; 3],
    pub published_once: bool,
}

const WIRE_LAUNCH_CAP: usize = 64;
const WIRE_IMPACT_CAP: usize = 64;
const WIRE_DEATH_CAP: usize = 32;
/// Maximum ticks an undrained wire event stays buffered before rolling off.
const WIRE_MAX_AGE_TICKS: u32 = 12;

#[derive(Clone, Debug)]
pub struct Observations {
    pub rounds: Vec<Round>,
    pub(super) tick: u32,
    serial: u64,
    pub(crate) wire_enabled: bool,
    pub(crate) wire_launches: Vec<WireLaunch>,
    pub(crate) wire_impacts: Vec<WireImpact>,
    pub(crate) wire_deaths: Vec<WireDeath>,
}

impl Observations {
    pub(crate) fn new(slots: usize) -> Self {
        Self {
            rounds: vec![Round::default(); slots],
            tick: 0,
            serial: 0,
            wire_enabled: false,
            wire_launches: Vec::new(),
            wire_impacts: Vec::new(),
            wire_deaths: Vec::new(),
        }
    }

    pub(crate) fn enable_wire(&mut self) {
        if !self.wire_enabled {
            self.wire_enabled = true;
            self.wire_launches = Vec::with_capacity(WIRE_LAUNCH_CAP);
            self.wire_impacts = Vec::with_capacity(WIRE_IMPACT_CAP);
            self.wire_deaths = Vec::with_capacity(WIRE_DEATH_CAP);
        }
    }

    pub fn wire_launches(&self) -> &[WireLaunch] {
        &self.wire_launches
    }

    pub fn wire_impacts(&self) -> &[WireImpact] {
        &self.wire_impacts
    }

    pub fn wire_deaths(&self) -> &[WireDeath] {
        &self.wire_deaths
    }

    pub(super) fn advance_tick(&mut self, tick: u32) {
        self.tick = tick;
        if self.wire_enabled {
            let cutoff = tick.saturating_sub(WIRE_MAX_AGE_TICKS);
            self.wire_launches.retain(|e| e.tick >= cutoff);
            self.wire_impacts.retain(|e| e.tick >= cutoff);
            self.wire_deaths.retain(|e| e.tick >= cutoff);
        }
    }

    pub(super) fn launch(
        &mut self,
        slot: usize,
        shot: &SpaceProjectile,
        weapon: &str,
        barrel: Option<usize>,
    ) {
        self.serial = self.serial.wrapping_add(1).max(1);
        let weapon_hash = crate::combat::combo::zero_fnv1a_lower(weapon);
        self.rounds[slot] = Round {
            serial: self.serial,
            weapon: weapon_hash,
            barrel,
            source_body: shot.ignore_body,
            emitter: shot.emitter,
            team: shot.owner_team,
            born: self.tick,
            tick: self.tick,
            origin: shot.position,
            initial_velocity: shot.velocity,
            previous: shot.position,
            position: shot.position,
            velocity: shot.velocity,
            active: true,
            tracer: shot.is_tracer,
            ..Default::default()
        };
        if self.wire_enabled {
            let entry = WireLaunch {
                tick: self.tick,
                serial: (self.serial & 0xffff) as u16,
                weapon_hash,
                is_beam: shot.kind == SpaceOrdnanceKind::Beam,
                team: shot.owner_team.clamp(0, 7) as u8,
                is_human: !shot.owner_is_ai,
                origin: shot.position,
                velocity: shot.velocity,
                published_once: false,
            };
            push_prioritized(&mut self.wire_launches, entry, |e| e.is_human);
        }
    }

    pub(super) fn sample(
        &mut self,
        slot: usize,
        shot: &SpaceProjectile,
        hit: Option<[f64; 3]>,
        contact: Contact,
    ) {
        let r = &mut self.rounds[slot];
        r.tick = self.tick;
        r.previous = shot.previous_position;
        r.position = hit.unwrap_or(shot.position);
        r.velocity = shot.velocity;
        r.active = shot.active;
        r.contact = hit.map(|_| contact);
        let impact_blast = shot.impact_blast();
        r.blast = hit.is_some() && impact_blast.hurts();
        if self.wire_enabled && (hit.is_some() || (!shot.active && shot.end_blast(true).outer_radius > 0.)) {
            let blast = if hit.is_some() {
                impact_blast.outer_radius > 0.
            } else {
                shot.end_blast(true).outer_radius > 0.
            };
            let entry = WireImpact {
                tick: self.tick,
                serial: (r.serial & 0xffff) as u16,
                weapon_hash: r.weapon,
                shield: hit.is_some() && contact.shield,
                rigid: hit.is_some() && contact.rigid,
                blast,
                is_human: !shot.owner_is_ai,
                position: r.position,
                normal: hit.and_then(|_| contact.normal),
                published_once: false,
            };
            push_prioritized(&mut self.wire_impacts, entry, |e| e.is_human || e.blast);
        }
    }

    pub(super) fn record_death(&mut self, is_building: bool, class_idx: u16, position: [f64; 3]) {
        if !self.wire_enabled {
            return;
        }
        let entry = WireDeath {
            tick: self.tick,
            is_building,
            class_idx,
            position,
            published_once: false,
        };
        if self.wire_deaths.len() < self.wire_deaths.capacity() {
            self.wire_deaths.push(entry);
        } else if let Some(slot) = self.wire_deaths.iter_mut().find(|d| !d.is_building) {
            if is_building {
                *slot = entry;
            }
        }
    }

    /// Advances the wire observation ring by one 20 Hz publication: drops events
    /// already carried across two publications (or aged past the 6-tick carry
    /// window) and marks freshly published ones for one carry-over window so a
    /// single lost or skipped snapshot loses zero launches, impacts, or
    /// destruction bursts. Zero heap allocations.
    pub(crate) fn advance_wire_window(&mut self) {
        let tick = self.tick;
        self.wire_launches.retain_mut(|e| {
            if e.published_once || tick.saturating_sub(e.tick) >= 6 {
                false
            } else {
                e.published_once = true;
                true
            }
        });
        self.wire_impacts.retain_mut(|e| {
            if e.published_once || tick.saturating_sub(e.tick) >= 6 {
                false
            } else {
                e.published_once = true;
                true
            }
        });
        self.wire_deaths.retain_mut(|e| {
            if e.published_once || tick.saturating_sub(e.tick) >= 6 {
                false
            } else {
                e.published_once = true;
                true
            }
        });
    }
}

fn push_prioritized<T>(vec: &mut Vec<T>, entry: T, important: impl Fn(&T) -> bool) {
    if vec.len() < vec.capacity() {
        vec.push(entry);
    } else if important(&entry) {
        if let Some(pos) = vec.iter().position(|e| !important(e)) {
            vec[pos] = entry;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::combat::SpaceOrdnancePool;

    #[test]
    fn contact_uses_the_accepted_point_and_reuse_starts_another_life() {
        let mut pool = SpaceOrdnancePool::new(1);
        pool.observations = Some(Observations::new(1));
        pool.observations.as_mut().unwrap().tick = 17;
        let shot = SpaceProjectile {
            position: [4., 3., 0.],
            velocity: [0., 0., 2.],
            blast: crate::space::combat::SpaceExplosionSpec {
                damage: 2.,
                outer_radius: 3.,
                ..Default::default()
            },
            emitter: Some(EmittingBody::Building { index: 4, life: 1. }),
            is_tracer: true,
            ..Default::default()
        };
        assert_eq!(pool.spawn_weapon(shot, "sivlet", Some(3)), Some(0));
        assert!(pool.observations.as_ref().unwrap().rounds[0].tracer);
        assert_eq!(pool.observations.as_ref().unwrap().rounds[0].emitter, shot.emitter);
        let obs = pool.observations.as_mut().unwrap();
        let impact = [4., 3., 0.01];
        obs.sample(
            0,
            &shot,
            Some(impact),
            Contact {
                rigid: true,
                ..Default::default()
            },
        );
        let end = obs.rounds[0];
        assert_eq!((end.origin, end.position), (shot.position, impact));
        assert_eq!(end.barrel, Some(3));
        assert!(end.blast && end.contact.unwrap().rigid);
        assert_eq!(pool.spawn_weapon(shot, "pebble", None), None);
        pool.slots[0].active = false;
        assert_eq!(pool.spawn_weapon(shot, "pebble", None), Some(0));
        let next = pool.observations.as_ref().unwrap().rounds[0];
        assert!(next.active && next.contact.is_none() && next.serial != end.serial);
        assert_ne!(next.weapon, end.weapon);
        pool.slots[0].active = false;
        assert_eq!(pool.spawn(shot), Some(0));
        assert_eq!(pool.observations.as_ref().unwrap().rounds[0].serial, 0);
    }

    #[test]
    fn advance_wire_window_carries_events_across_two_publications_then_drops() {
        let mut obs = Observations::new(1);
        obs.enable_wire();
        obs.tick = 10;
        let shot = SpaceProjectile {
            position: [1., 2., 3.],
            velocity: [0., 0., 10.],
            active: true,
            ..Default::default()
        };
        obs.launch(0, &shot, "laser", None);
        obs.sample(0, &shot, Some([1., 2., 4.]), Contact { rigid: true, ..Default::default() });
        obs.record_death(false, 2, [5., 6., 7.]);
        assert_eq!((obs.wire_launches.len(), obs.wire_impacts.len(), obs.wire_deaths.len()), (1, 1, 1));
        assert!(!obs.wire_launches[0].published_once);

        // First publication marks events as published_once and keeps them for one carry-over window.
        obs.advance_wire_window();
        assert_eq!((obs.wire_launches.len(), obs.wire_impacts.len(), obs.wire_deaths.len()), (1, 1, 1));
        assert!(obs.wire_launches[0].published_once);
        assert!(obs.wire_impacts[0].published_once);
        assert!(obs.wire_deaths[0].published_once);

        // Fresh event recorded in window 2 coexists with carried-over window 1 event.
        obs.tick = 13;
        obs.launch(0, &shot, "laser", None);
        assert_eq!(obs.wire_launches.len(), 2);

        // Second publication drops window 1 events and retains window 2's fresh launch.
        obs.advance_wire_window();
        assert_eq!((obs.wire_launches.len(), obs.wire_impacts.len(), obs.wire_deaths.len()), (1, 0, 0));
        assert_eq!(obs.wire_launches[0].tick, 13);
        assert!(obs.wire_launches[0].published_once);

        // Third publication drops window 2's launch too.
        obs.advance_wire_window();
        assert!(obs.wire_launches.is_empty());

        // An event already aged >= 6 ticks before its first publication is pruned immediately.
        obs.tick = 20;
        obs.launch(0, &shot, "laser", None);
        obs.tick = 26;
        obs.advance_wire_window();
        assert!(obs.wire_launches.is_empty());
    }
}

impl crate::world::owner::World {
    /// Enable bounded wire event capture (launches, impacts, deaths) for
    /// dedicated-server Space replication.
    pub fn enable_space_wire_observations(&mut self) -> bool {
        let Some(space) = self.space.as_deref_mut() else { return false };
        let slots = space.ordnance.slots.len();
        let obs = space.ordnance.observations.get_or_insert_with(|| Observations::new(slots));
        obs.enable_wire();
        true
    }
}
