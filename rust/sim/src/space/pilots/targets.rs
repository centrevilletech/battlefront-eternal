//! What a pilot attacks, and with which of its craft's weapons.
//!
//! A pilot sees enemy craft within its view range that are aloft, and the
//! enemy ship objects the mission makes lockable (the critical systems a
//! player's target selector also offers). Each weapon's own AI flags and
//! ranges decide what it is used on. Only one line-of-sight ray is cast per
//! choice.

use super::super::flight::{SpaceFlightProfile, SpaceFlightState, SpaceWeaponSpec};
use super::super::SpaceBattlefield;
use super::steer::Traffic;
use super::vec::{dist, dot, sub, unit};

/// How far a pilot looks for craft to fight (a target selector's reach on the
/// 2005 HUD is not measured).
pub const PROVISIONAL_VIEW_RANGE: f64 = 1500.0;
/// Ranking weight of a ship system against a craft at the same distance for
/// a craft whose weapons favour craft, and for one that carries building-only
/// ordnance (bombers prefer systems, and count a craft as this much farther).
pub const PROVISIONAL_SYSTEM_WEIGHT: f64 = 1.25;
pub const PROVISIONAL_BOMBER_SYSTEM_WEIGHT: f64 = 0.5;
pub const PROVISIONAL_BOMBER_CRAFT_WEIGHT: f64 = 4.0;
/// A ship object's ranking grows by this much per enemy gun covering it (a
/// gun counts fully at its muzzle, nothing at the edge of its reach).
pub const PROVISIONAL_EXPOSURE_WEIGHT: f64 = 0.75;
/// Damage a second against a ship object that counts as one: a pilot's
/// ranking of an object scales with its weapons' damage there in the state
/// the object is in (shield up or down), and an object its weapons hurt less
/// than a tenth of this is left alone.
pub const PROVISIONAL_OBJECT_DAMAGE_RATE: f64 = 1000.0;
/// Ranking counts an object no nearer than this from its surface.
pub const PROVISIONAL_NEAREST_SURFACE: f64 = 50.0;
/// An object nearly lost ranks nearer: its distance counts this share in
/// full, the rest scaled by what is left of its shield or hull.
pub const PROVISIONAL_FINISH_SHARE: f64 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PilotTarget {
    #[default]
    None,
    Craft { index: usize, generation: u32 },
    Building { index: usize, life: f64 },
}

/// The weapons of a craft's pilot seat, read once per profile.
#[derive(Clone, Copy, Debug, Default)]
pub struct Loadout {
    /// First weapon of each fire channel on the pilot seat.
    pub primary: Option<usize>,
    pub secondary: Option<usize>,
    pub hits_craft: bool,
    pub hits_buildings: bool,
    /// Carries a weapon for buildings only (bombs).
    pub bomber: bool,
    /// Damage a second its weapons for buildings do to a shield, and to the
    /// object itself once its shield is down.
    pub shield_rate: f64,
    pub hull_rate: f64,
    /// The same without a weapon that fires only on a lock, which works only
    /// on the objects the mission makes lockable.
    pub free_shield_rate: f64,
    pub free_hull_rate: f64,
}

impl Loadout {
    pub fn of(p: &SpaceFlightProfile) -> Self {
        let first = |channel: u8| p.weapons.iter().position(|w| w.seat == 0 && w.channel == channel);
        let mut out = Loadout { primary: first(0), secondary: first(1), ..Default::default() };
        for i in [out.primary, out.secondary].into_iter().flatten() {
            let w = &p.weapons[i];
            out.hits_craft |= w.ai_targets_vehicle;
            out.hits_buildings |= w.ai_targets_building;
            out.bomber |= w.ai_targets_building && !w.ai_targets_vehicle;
            if w.ai_targets_building {
                out.shield_rate += object_rate(w, true);
                out.hull_rate += object_rate(w, false);
                if !out.needs_lock(p, i) {
                    out.free_shield_rate += object_rate(w, true);
                    out.free_hull_rate += object_rate(w, false);
                }
            }
        }
        out
    }

    /// Whether pilot weapon `i` fires only on a lock: the second trigger's
    /// weapon with a lock range.
    fn needs_lock(&self, p: &SpaceFlightProfile, i: usize) -> bool {
        self.secondary == Some(i) && p.weapons[i].lock_on_range > 0.0
    }

    /// Damage a second to an object in its present state.
    pub fn rate(&self, shielded: bool, lockable: bool) -> f64 {
        match (shielded, lockable) {
            (true, true) => self.shield_rate,
            (false, true) => self.hull_rate,
            (true, false) => self.free_shield_rate,
            (false, false) => self.free_hull_rate,
        }
    }
}

/// Damage a second the rounds of weapon `w` do to a ship object they strike,
/// every one of its salvos counted: to its shield while that is up, to the
/// object itself after. How much of a round's blast reaches an object depends
/// on the object's size and is left out.
pub fn object_rate(w: &SpaceWeaponSpec, shielded: bool) -> f64 {
    let rounds = f64::from(w.salvo_count.max(1)) * f64::from(w.shots_per_salvo.max(1));
    let per_second = rounds / v8math::max(w.shot_time(), 0.05);
    let hit = if shielded {
        w.max_damage * w.shield_scale
    } else {
        w.max_damage * w.building_scale
    };
    hit * per_second
}

/// The pilot weapon (an index into the profile's weapons) that does a ship
/// object the most damage a second in its present state, if any may be used
/// on one (a lock weapon only on one the mission makes lockable).
pub fn object_weapon(p: &SpaceFlightProfile, loadout: &Loadout, shielded: bool, lockable: bool) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for i in [loadout.primary, loadout.secondary].into_iter().flatten() {
        let w = &p.weapons[i];
        let rate = object_rate(w, shielded);
        let usable = w.ai_targets_building && (lockable || !loadout.needs_lock(p, i));
        if usable && rate > 0.0 && best.is_none_or(|(r, _)| rate > r) {
            best = Some((rate, i));
        }
    }
    best.map(|(_, i)| i)
}

/// How far from a ship object weapon `w` starts to work on it: a lock
/// weapon's lock range, else its rounds' reach within its maximum range.
pub fn object_reach(w: &SpaceWeaponSpec) -> f64 {
    let reach = v8math::min(w.max_range, w.round_reach());
    if w.lock_on_range > 0.0 { v8math::min(reach, w.lock_on_range) } else { reach }
}

/// How far a round of weapon `w` falls on its way to a point `range` away.
pub fn drop_over(w: &SpaceWeaponSpec, range: f64) -> f64 {
    if w.round_speed() <= 1.0 {
        return 0.0;
    }
    let t = range / w.round_speed();
    0.5 * w.gravity * t * t
}

/// Whether `w` may be used on the target kind at `range`.
pub fn usable(w: &SpaceWeaponSpec, building: bool, range: f64) -> bool {
    let allowed = if building { w.ai_targets_building } else { w.ai_targets_vehicle };
    allowed && range >= w.ai_min_range && range <= w.max_range
}

/// Facts about a candidate system a pilot can read from the battlefield.
#[derive(Clone, Copy, Debug)]
pub struct System {
    pub index: usize,
    pub life: f64,
    pub team: i32,
    pub center: [f64; 3],
    pub radius: f64,
    /// The object's collision bodies (negative: none), for firing lines.
    pub bodies: [i32; 3],
    /// The reach of a self-piloted gun that fires on craft (its rounds' reach
    /// within the range it looks for targets in), zero for anything else: a
    /// gun that fires only on ship objects is no danger to a craft, however
    /// far it reaches.
    pub gun_reach: f64,
    /// Whether the mission makes it lockable.
    pub lockable: bool,
    /// The object's shield charge.
    pub shield: f64,
    /// How many of its own team's guns cover it, each weighted by how deep
    /// inside the gun's reach it lies.
    pub exposure: f64,
    /// A hull section that only carries a ship's shared shield: worth
    /// attacking while that shield is up, never after.
    pub hull: bool,
    /// What is left of what stands between it and its loss: its shield's
    /// share of the maximum while that is up, else its hull's share.
    pub left: f64,
}

/// The two best targets for a pilot at `me`, best first (`None` where there
/// is none). `systems` are the enemy objects still worth attacking.
pub fn choose(
    traffic: &[Traffic],
    systems: &[System],
    loadout: &Loadout,
    me: &Traffic,
    forward: [f64; 3],
) -> [PilotTarget; 2] {
    let mut ranked: [(f64, PilotTarget); 2] = [(f64::INFINITY, PilotTarget::None); 2];
    let mut offer = |score: f64, t: PilotTarget| {
        if score < ranked[0].0 {
            ranked[1] = ranked[0];
            ranked[0] = (score, t);
        } else if score < ranked[1].0 {
            ranked[1] = (score, t);
        }
    };
    if loadout.hits_craft {
        let weight = if loadout.bomber { PROVISIONAL_BOMBER_CRAFT_WEIGHT } else { 1.0 };
        for t in traffic.iter().filter(|t| t.team != me.team && t.team != 0) {
            let d = dist(t.position, me.position);
            if d > PROVISIONAL_VIEW_RANGE {
                continue;
            }
            let ahead = dot(unit(sub(t.position, me.position)), forward);
            let score = d * (1.6 - 0.6 * ahead) * weight;
            offer(score, PilotTarget::Craft { index: t.id, generation: t.generation });
        }
    }
    if loadout.hits_buildings {
        let weight = if loadout.bomber { PROVISIONAL_BOMBER_SYSTEM_WEIGHT } else { PROVISIONAL_SYSTEM_WEIGHT };
        for s in systems.iter().filter(|s| s.team != me.team) {
            // What its weapons do there now: against the shield while it is
            // up, against the object once it is down.
            if s.hull && s.shield <= 0.0 {
                continue;
            }
            let rate = loadout.rate(s.shield > 0.0, s.lockable);
            let effect = v8math::min(rate / PROVISIONAL_OBJECT_DAMAGE_RATE, 3.0);
            if effect < 0.1 {
                continue;
            }
            let cover = 1.0 + PROVISIONAL_EXPOSURE_WEIGHT * s.exposure;
            let finish = PROVISIONAL_FINISH_SHARE + (1.0 - PROVISIONAL_FINISH_SHARE) * s.left.clamp(0.0, 1.0);
            // An object is as near as its surface.
            let d = v8math::max(PROVISIONAL_NEAREST_SURFACE, dist(s.center, me.position) - s.radius);
            offer(d * weight * cover * finish / effect, PilotTarget::Building { index: s.index, life: s.life });
        }
    }
    [ranked[0].1, ranked[1].1]
}

/// Whether a craft target is still a live enemy craft aloft.
pub fn craft_alive(space: &SpaceBattlefield, index: usize, generation: u32) -> bool {
    space.fighters.get(index).is_some_and(|f| {
        f.alive && f.generation == generation && f.state != SpaceFlightState::Landed
    })
}
