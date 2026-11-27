//! Bots that fly in a 2005 space battle.
//!
//! A bot on foot near a free parked craft of its team walks to it and boards
//! it through the seat call a player's use key reaches (the same reach, team
//! and occupancy check). Seated, it sends the craft the controls a player
//! sends (throttle, stick, roll, boost, the action key, the triggers and the
//! lock key) through the seat's input receipt, which the next fixed tick
//! consumes. It leaves its hangar through the authored opening and the map's
//! launch path, fights enemy craft and the enemy ship's lockable systems with
//! its craft's own weapons, steers around hulls and other craft by bounded
//! look-ahead tests, and lands in its home hangar to repair. Transport pilots
//! fly to the enemy hangar and land there; a second bot can take a transport's
//! gunner seat before it leaves.
//!
//! The pilots run once per fixed tick, after the bot controller's phase and
//! before the soldier phase, and only while a 2005 space battlefield is
//! attached. They write the intent of the bots they steer (a seated bot sends
//! no on-foot controls) and leave every other bot to the bot controller.
//! Choices are staggered by unit slot and storage is reserved at admission,
//! so a warm tick allocates nothing.

mod routes;
mod steer;
mod targets;
mod vec;
#[cfg(test)]
mod tests;

pub use routes::{Exit, Routes};
pub use targets::PilotTarget;

use super::flight::{HullMeets, SpaceFighter, SpaceFlightState, SpaceLandingPermission, SpaceLockTargetKind, SpaceOrdnanceKind, SpacePilotInput, SpaceVehiclePad};
use super::bounds::SpaceBounds;
use super::SpaceSeat;
use crate::gen::{F3UnitKey, Unit, UnitBody};
use crate::store::Ref;
use crate::world::owner::World;
use steer::{escape_from, heading, hull_reach, lead, traffic_escape, Traffic};
use targets::{choose, craft_alive, drop_over, object_reach, object_weapon, usable, Loadout, System};
use three::Vector3;
use vec::{add, clamp, dist, dot, len, scale, sub, unit};

/// A waiting craft lures a bot from as far as the mission's AI vehicle notify
/// radius (`SetAIVehicleNotifyRadius`), which the mission host passes in;
/// without it, the battle's bot controller's notify radius, whose released
/// default (a mission that never sets one) is this.
pub const RELEASED_NOTIFY_RADIUS: f64 = 50.0;
/// A human on foot heading for the free craft nearest them keeps it from
/// the bots: closing on it at least this fast, or this close to it.
pub const PROVISIONAL_HUMAN_CLOSING: f64 = 0.5;
pub const PROVISIONAL_HUMAN_AT: f64 = 4.0;
/// A walker heads for this far outside a parked hull's footprint (inside the
/// seat call's two-metre reach).
pub const PROVISIONAL_BOARD_STANDOFF: f64 = 1.2;
/// A remote turret chair uses the battle's AI vehicle notify radius (like a
/// waiting craft); the bot leaves it after the time limit or when no enemy
/// craft came in reach for the idle time.
pub const PROVISIONAL_CHAIR_LIMIT_S: f64 = 120.0;
/// The 2005 game's idle time in a stationary seat: a bot leaves after this
/// long without a target, unless the mission keeps bots in turrets
/// (`SetStayInTurrets`), when it waits the second.
pub const RELEASED_CHAIR_IDLE_S: f64 = 10.0;
/// The 2005 game's sight range: a target's radius times this plus the base,
/// doubled for a human target; all round within the last, else within the
/// cosine of half its 155-degree field of the looker's facing.
pub const SIGHT_RADIUS_SCALE: f64 = 20.0;
pub const SIGHT_BASE_M: f64 = 30.0;
pub const SIGHT_ALL_ROUND_M: f64 = 10.0;
pub const SIGHT_HALF_FIELD_COS: f64 = 0.216_439_613_938_102_9;
/// Where a soldier's eyes are above his feet when he looks for craft: the
/// released soldier class's eye point when it names no `EyePointOffset`.
pub const SOLDIER_EYE_M: f64 = 1.8;
pub const RELEASED_CHAIR_IDLE_STAYING_S: f64 = 10_000.0;
/// A second bot joins a transport whose pilot seat is taken from this far.
pub const PROVISIONAL_GUNNER_JOIN: f64 = 40.0;
/// Unit slots reserved for pilots when the battle states no larger limit.
pub const PROVISIONAL_UNIT_SLOTS: usize = 256;
/// Parked seats a free bot weighs per choice, nearest first: the first with
/// a clear walk wins (one ray each).
pub const PROVISIONAL_SEAT_CHOICES: usize = 4;
/// The most craft the pilots scan: the battle's admitted fighter slots. More
/// are counted (`PilotCensus::craft_truncated`) and left out.
pub const MAX_CRAFT: usize = super::MAX_PREPARED_FIGHTERS;
/// The most ship objects the target scan keeps; more are counted
/// (`PilotCensus::systems_truncated`) and left out.
pub const MAX_SYSTEMS: usize = 256;
/// Ticks between rescans of the ship objects worth attacking.
pub const SYSTEM_SCAN_TICKS: u32 = 30;
/// A launch path node not reached in this long is skipped.
pub const PROVISIONAL_NODE_LIMIT_S: f64 = 12.0;
/// Ticks between a pilot's choices; each slot thinks on its own tick.
pub const THINK_TICKS: u32 = 30;
/// Seated this long before the takeoff press.
pub const PROVISIONAL_BOARD_DELAY_S: f64 = 0.5;
/// A walk to a craft ends after this long, or after three one-second checks
/// without a metre of progress.
pub const PROVISIONAL_WALK_LIMIT_S: f64 = 25.0;
/// A routed walk round walls, doors or decks ends after this long.
pub const PROVISIONAL_ROUTED_WALK_LIMIT_S: f64 = 45.0;
/// Maximum new routed pilot walks started across all bots on a single tick.
pub const PROVISIONAL_ROUTED_STARTS_PER_TICK: u8 = 2;
/// A pilot heads home under this share of its hull and leaves the deck again
/// once repaired to the second share, or after the time limit.
pub const PROVISIONAL_RETURN_HULL: f64 = 0.50;
pub const PROVISIONAL_RELAUNCH_HULL: f64 = 0.90;
pub const PROVISIONAL_PARK_LIMIT_S: f64 = 45.0;
/// Under heavy fire (this share of the hull lost in half a second) a craft no
/// fresher than the second share turns for home at once.
pub const PROVISIONAL_HEAVY_FIRE_SHARE: f64 = 0.12;
pub const PROVISIONAL_HEAVY_FIRE_HULL: f64 = 0.75;
/// Inside enemy guns' reach a craft turns for home already under this share
/// of its hull, and flies clear of their reach first, for at most this long.
pub const PROVISIONAL_UNDER_GUNS_HULL: f64 = 0.75;
pub const PROVISIONAL_DISENGAGE_S: f64 = 8.0;
/// A pilot whose class repairs its craft in flight mends a damaged craft (to
/// the relaunch share) flying away from the enemy's ships instead of flying
/// home; still under the return share within this distance of its patrol
/// point, it lands.
pub const PROVISIONAL_MEND_REACH: f64 = 150.0;
/// A transport pilot waits this long on its deck for crew before leaving.
pub const PROVISIONAL_CREW_WAIT_S: f64 = 30.0;
/// Crew are lured to a waiting transport from this far.
pub const PROVISIONAL_CREW_LURE: f64 = 60.0;
/// Contact lasting this many ticks with no quiet spell pins a craft: it backs
/// off, and each such stretch counts as another scrape.
pub const PROVISIONAL_PINNED_TICKS: u32 = 45;
/// A craft still inside its hangar this long after boarding gives up.
pub const PROVISIONAL_HANGAR_LIMIT_S: f64 = 30.0;
/// An attack run on a ship object breaks off this close and comes round from
/// this far away.
pub const PROVISIONAL_BREAK_RANGE: f64 = 150.0;
/// A run on a ship object starts this far beyond its weapon's reach (counted
/// no farther than the second figure) on a side with a clear line to it (six
/// sides tried, nearest first, once per target), and slows to the craft's
/// least speed when it carries ordnance.
pub const PROVISIONAL_RUN_LEAD_IN: f64 = 150.0;
pub const PROVISIONAL_RUN_REACH_MAX: f64 = 600.0;
/// A run aimed at a point of an object's surface counts the point as this
/// big, and tests its rounds' path once the nose is this close (radians).
pub const PROVISIONAL_AIM_RADIUS: f64 = 10.0;
pub const PROVISIONAL_OBJECT_CONE: f64 = 0.2;
/// An idle pilot patrols this far out from its own ship's opening toward the
/// enemy, closer when the enemy's guns reach there.
pub const PROVISIONAL_PATROL_OUT: f64 = 500.0;
/// Ticks a pilot breaking off a run flies away from the object before it
/// heads back out to the run's start.
pub const PROVISIONAL_BREAK_TICKS: u32 = 150;
/// A dogfight breaks off this close to the other craft.
pub const PROVISIONAL_CRAFT_BREAK: f64 = 60.0;
/// A pilot that has had no shot at a craft for this long (two craft circling
/// each other) gives it up and leaves it alone for the second span.
pub const PROVISIONAL_NO_SHOT_LIMIT_S: f64 = 20.0;
pub const PROVISIONAL_GIVE_UP_S: f64 = 15.0;
/// A pilot fires its guns when its aim is this close (radians) to the line
/// to the target, or within the target's own angular size.
pub const PROVISIONAL_FIRE_CONE: f64 = 0.045;
/// Ticks a firing line's check holds before it is cast again: every tick a
/// shot is wanted.
pub const LINE_TICKS: u32 = 1;
/// Candidate targets a gunner or chair looks at per choice, nearest first;
/// the first it can see is chosen (one ray each).
pub const PROVISIONAL_SIGHT_CHOICES: usize = 2;
/// A hull loss of this share between two checks counts as being hit.
pub const PROVISIONAL_HIT_SHARE: f64 = 0.04;
/// How far into the hangar past the opening a craft is before it lands.
pub const PROVISIONAL_LAND_DEPTH: f64 = 14.0;
/// Ticks a pilot keeps to its way out from a hull ahead.
pub const PROVISIONAL_AVOID_TICKS: u32 = 75;
/// Below this airspeed a craft runs no look-ahead tests.
pub const PROVISIONAL_AVOID_MIN_SPEED: f64 = 10.0;
/// Pace across a hangar, and the angle off the nose above which a pilot in a
/// hangar stops to turn first.
pub const PROVISIONAL_HANGAR_SPEED: f64 = 15.0;
pub const PROVISIONAL_HANGAR_TURN_CONE: f64 = 0.2;
/// Clearance kept from an opening's edge beyond the craft's own reach.
pub const PROVISIONAL_TRACK_MARGIN: f64 = 6.0;
/// A craft coming in stops this high over its spot, within this distance of
/// it, before asking to land; it gives up the approach after the time limit.
pub const PROVISIONAL_SPOT_HEIGHT: f64 = 4.0;
pub const PROVISIONAL_SPOT_REACH: f64 = 5.0;
pub const PROVISIONAL_RETURN_LIMIT_S: f64 = 120.0;
/// A slow-turning craft lines up this many turn radii outside an opening.
pub const PROVISIONAL_APPROACH_TURNS: f64 = 2.5;
/// Separate hull contacts after which a craft gives up its way into a hangar.
pub const PROVISIONAL_SCRAPE_LIMIT: u8 = 4;
/// The far line-up point lies this many approach distances out.
pub const PROVISIONAL_FAR_APPROACH: f64 = 2.2;
/// A craft coming in counts as on the opening's axis within this share of
/// the opening's half height and this many metres of its track.
pub const PROVISIONAL_AXIS_SHARE: f64 = 0.8;
pub const PROVISIONAL_AXIS_TRACK: f64 = 15.0;
/// A craft coming in counts as lined up within this distance of the approach
/// point, and presses the action key this many seconds of flight before its spot.
pub const PROVISIONAL_LINE_UP: f64 = 35.0;
pub const PROVISIONAL_LAND_LEAD_S: f64 = 0.45;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PilotPhase {
    /// On foot and left to the bot controller.
    #[default]
    Free,
    /// Walking to a claimed seat.
    Walk,
    /// Seated in the pilot seat inside the hangar: takeoff and the way out.
    Hangar,
    /// Following the launch path.
    Launch,
    Fight,
    /// Damaged, flying home.
    Return,
    /// On the home deck, repairing.
    Parked,
    /// A transport flying to the enemy hangar, or landed there.
    Raid,
    /// Could not leave the hangar in time: landing to get out.
    Abandon,
    /// Walking to a remote turret chair on its own ship.
    ToChair,
    /// Working a remote turret from its chair.
    Chair,
    /// In a gunner seat.
    Gunner,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Run {
    #[default]
    Approach,
    Break,
}

#[derive(Clone, Copy, Debug, Default)]
struct Bot {
    active: bool,
    unit: Ref<Unit>,
    life: f64,
    phase: PilotPhase,
    since: u32,
    craft: usize,
    craft_gen: u32,
    seat: u8,
    routed: bool,
    routed_moved: bool,
    receipt: Option<SpaceSeat>,
    terminal: Option<super::SpaceTerminalSeat>,
    chair: usize,
    cooldown_until: u32,
    check_tick: u32,
    check_dist: f64,
    stalls: u8,
    exit: usize,
    step: u8,
    node: u32,
    target: PilotTarget,
    /// Tick of the latest shot (or lock) at the target, or of its choice.
    shot_tick: u32,
    /// A craft given up for lack of a shot, ignored until `shun_until`.
    shunned: PilotTarget,
    shun_until: u32,
    /// A gunner's or chair's latest firing line check and the tick it expires.
    line_ok: bool,
    line_tick: u32,
    run: Run,
    run_until: u32,
    avoid_until: u32,
    avoid_dir: [f64; 3],
    hull_check: f64,
    action_down: bool,
    salvo_down: bool,
    contacts: u32,
    contact_until: u32,
    /// When the current bout of contact began (contact ticks no more than 15 apart).
    bout_start: u32,
    /// Hull contacts since the current approach to a hangar began.
    scrapes: u8,
    /// Flying clear of enemy guns before heading home, until this tick.
    disengage_until: u32,
    disengage_dir: [f64; 3],
    /// A big hull loss in the latest half second.
    heavy_fire: bool,
    /// The latest hull avoidance (as opposed to a traffic turn) is still on.
    avoid_hull: bool,
    /// Going round: back out to the far line-up point before trying again.
    rounding: bool,
    /// Mending its craft in flight clear of the guns.
    mending: bool,
    /// Where runs on the current ship object start and the point they aim at
    /// (on its surface, else its centre) this tick, the same two in the
    /// object's own frame (frigates move), the object they are for and the
    /// reach of the weapon they were laid out for.
    run_from: [f64; 3],
    run_aim: [f64; 3],
    run_local_from: [f64; 3],
    run_local_aim: [f64; 3],
    run_surface: bool,
    run_for: PilotTarget,
    run_reach: f64,
    /// The launch path being flown ends before this node.
    path_end: u32,
    /// Track across the exit in use, and where a craft coming in sets down.
    track: f64,
    spot: [f64; 3],
    aim: crate::ai::combat::aim::Aimer,
    aiming: bool,
    /// The spread scale the aim filter handed the gun being aimed, which the
    /// seat passes on (`None` for a remote weapon, as the released code does).
    spread_scale: Option<f64>,
}

/// A census count saturates at `u32::MAX`: it is a diagnostic and never decides anything.
fn bump(count: &mut u32, by: u32) {
    *count = count.saturating_add(by);
}

/// A count of items as the `u32` the census keeps; more than `u32::MAX` saturates.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The tick as seen by one craft slot: the think and probe schedules are staggered by slot
/// (slots are below `MAX_CRAFT`), and the sum wraps harmlessly.
fn phase(tick: u32, slot: usize) -> u32 {
    tick.wrapping_add(count(slot))
}

impl Bot {
    fn fresh(life: f64, tick: u32) -> Self {
        Bot { active: true, life, since: tick, ..Default::default() }
    }

    /// Whether the craft this pilot chases has given it no shot for too long.
    fn no_shot(&self, tick: u32) -> bool {
        matches!(self.target, PilotTarget::Craft { .. })
            && tick.saturating_sub(self.shot_tick) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_NO_SHOT_LIMIT_S
    }

    /// Drops the chased craft and ignores it for a while.
    fn give_up(&mut self, tick: u32) {
        self.shunned = self.target;
        self.shun_until = tick.saturating_add((PROVISIONAL_GIVE_UP_S / super::SPACE_FIXED_DT) as u32);
        self.target = PilotTarget::None;
        self.aiming = false;
    }

    fn shuns(&self, target: PilotTarget, tick: u32) -> bool {
        tick < self.shun_until && self.shunned == target
    }
}

/// Running totals for evidence and tests. Each saturates at `u32::MAX` (see `bump`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PilotCensus {
    pub claims: u32,
    pub boardings: u32,
    pub gunner_boardings: u32,
    pub walk_failures: u32,
    pub takeoff_presses: u32,
    pub exits: u32,
    pub hangar_aborts: u32,
    pub returns: u32,
    pub land_presses: u32,
    pub raids_landed: u32,
    pub relaunches: u32,
    pub gunner_dismounts: u32,
    pub hull_avoidances: u32,
    pub craft_avoidances: u32,
    pub evasions: u32,
    pub lock_salvos: u32,
    pub chair_boardings: u32,
    pub chair_bursts: u32,
    pub approaches_given_up: u32,
    pub unclaimed_slots: u32,
    pub dogfights_given_up: u32,
    /// Target choices made (each on the chooser's own think tick).
    pub target_searches: u32,
    /// Seats passed over because the walk to them was blocked.
    pub blocked_seats: u32,
    pub craft_truncated: u32,
    pub systems_truncated: u32,
    pub nodes_skipped: u32,
    pub holds_refused: u32,
    /// Shots held back because the line to the target was blocked.
    pub blocked_shots: u32,
    /// Craft pinned against geometry that backed off.
    pub pinned: u32,
    /// Returns begun inside enemy guns' reach that flew clear first.
    pub disengages: u32,
    /// Transports that left with crew aboard, and without after waiting.
    pub crewed_departures: u32,
    pub empty_departures: u32,
    /// Approaches to a hangar begun again from the far point, and returns
    /// given up after the time limit.
    pub approach_rounds: u32,
    pub return_timeouts: u32,
    /// Ship objects given up because no side had a clear line to them.
    pub objects_unreachable: u32,
    /// Dives begun on ship objects, and runs broken off.
    pub object_dives: u32,
    pub object_breaks: u32,
    /// Damaged craft mended in flight instead of flown home.
    pub mends: u32,
    /// Walks to a pilot seat given up to a human heading for the craft.
    pub yields_to_humans: u32,
}

/// A pilot's state as tools and the bot goals read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PilotView {
    pub phase: PilotPhase,
    pub craft: Option<usize>,
    pub seat: u8,
    pub target: PilotTarget,
    pub since: u32,
    /// Progress through the phase (way points, approach stages, a run's
    /// start and dive), and an attack run's part: 0 on its way in, 1
    /// breaking off.
    pub step: u8,
    pub run: u8,
    /// The point an attack run on a ship object aims at this tick.
    pub aim_point: [f64; 3],
}

/// A living bot soldier as tools read it.
#[derive(Clone, Copy, Debug)]
pub struct PilotRow {
    pub slot: u32,
    pub life: f64,
    pub team: i32,
    pub position: [f64; 3],
    /// A pilot class (repairs the craft it flies).
    pub pilot_class: bool,
    pub view: Option<PilotView>,
}

/// The soldier life behind a launch owner.
#[derive(Clone, Copy, Debug)]
pub struct OwnerFacts {
    pub slot: u32,
    pub life: f64,
    pub team: i32,
    pub human: bool,
}

/// Every pilot of one battle. Reserved at admission for the unit slots and
/// craft the battle can hold.
#[derive(Default)]
pub struct PilotBots {
    bots: Vec<Bot>,
    /// Per craft and seat: the claiming unit slot plus one, or zero.
    claims: Vec<[u32; 6]>,
    /// Per craft this tick: how far the human heading for it is (infinite
    /// when none is).
    human_near: Vec<f64>,
    /// The mission's AI vehicle notify radius as its host passed it, and the
    /// radius in use this tick.
    notify_radius: Option<f64>,
    lure: f64,
    /// The mission keeps bots in turrets (`SetStayInTurrets`) as its host
    /// passed it: a chair needs no target and is left only after a long idle.
    stay_in_turrets: bool,
    /// Per ship object (attachment order): the unit slot plus one walking to
    /// or sitting in its chair, or zero.
    chair_claims: Vec<u32>,
    /// Per craft: whether a pilot holds it back for a gunner.
    loadouts: Vec<Loadout>,
    traffic: Vec<Traffic>,
    systems: Vec<System>,
    /// Bots the goal controller keeps on foot: per unit slot, the life held.
    held: Vec<Option<f64>>,
    pub routes: Routes,
    /// Fixed ticks since the battle began. A battle tick saturates at `u32::MAX` (2.2 years at 60 Hz): every deadline is
    /// `saturating_add`, every elapsed time `saturating_sub`, so no comparison inverts at the bound.
    tick: u32,
    routed_starts_this_tick: u8,
    pub census: PilotCensus,
    /// A tool's hook called at the start (`true`) and end of each pilot phase,
    /// to time it and count its allocations from outside. None in a battle.
    pub probe: Option<fn(bool)>,
    /// Off: the pilots send nothing and every bot stays the bot controller's.
    pub disabled: bool,
    /// The weapon classes the released aim code tells apart: cannon,
    /// launcher, remote.
    defs: [crate::combat::defs::JsObj; 3],
}

struct Soldier {
    r: Ref<Unit>,
    key: F3UnitKey,
    life: f64,
    team: i32,
    position: [f64; 3],
    yaw: f64,
    /// The bot controller drives it; only such a bot takes a new seat.
    controlled: bool,
    /// A pilot class (repairs the craft it flies); marines prefer to ride
    /// transports.
    pilot_class: bool,
}

fn soldier(world: &World, slot: u32) -> Option<Soldier> {
    let (r, u) = world.sim.store.unit.at(slot)?;
    if u.human || !u.alive || u.combat.dead || u.removed {
        return None;
    }
    let UnitBody::Soldier(s) = &u.soldier else { return None };
    Some(Soldier {
        r,
        key: F3UnitKey::of(r, u),
        life: u.generation,
        team: u.team as i32,
        position: s.position,
        yaw: s.aim_yaw,
        controlled: u.controller.is_some(),
        pilot_class: world.space().is_some_and(|space| space.repair_classes.contains(&u.class_name)),
    })
}

/// The intent a bot sends while the pilots steer it on foot.
fn walk_intent(world: &mut World, r: Ref<Unit>, forward: f64, turn: f64) {
    if let Some(u) = world.sim.store.unit.get_mut(r) {
        let i = &mut u.intent;
        *i = Default::default();
        i.forward = forward;
        i.turn = turn;
    }
}

/// A seated bot sends no on-foot controls: its soldier rides with the craft.
fn seated_intent(world: &mut World, r: Ref<Unit>) {
    if let Some(u) = world.sim.store.unit.get_mut(r) {
        u.intent = Default::default();
    }
}

/// Keeps the best (lowest score) entries in score order.
fn rank_into(ranked: &mut [(f64, usize, u8)], entry: (f64, usize, u8)) {
    if let Some(at) = ranked.iter().position(|r| entry.0 < r.0) {
        for i in (at + 1..ranked.len()).rev() {
            ranked[i] = ranked[i - 1];
        }
        ranked[at] = entry;
    }
}

fn wrap_pi(a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    a - tau * v8math::floor((a + std::f64::consts::PI) / tau)
}

impl PilotBots {
    /// Reserves the pilots for a battle at admission: one row a unit slot, one
    /// claim row a craft, and the exits and launch paths of the map.
    pub(crate) fn build(
        bounds: &SpaceBounds,
        pads: &[SpaceVehiclePad],
        fighters: &[SpaceFighter],
        profiles: &[super::flight::SpaceFlightProfile],
        map: &bfe_content::Value,
        unit_slots: usize,
        buildings: usize,
    ) -> Self {
        PilotBots {
            bots: vec![Bot::default(); unit_slots],
            claims: vec![[0; 6]; fighters.len().min(MAX_CRAFT)],
            human_near: vec![f64::INFINITY; fighters.len().min(MAX_CRAFT)],
            notify_radius: None,
            lure: RELEASED_NOTIFY_RADIUS,
            stay_in_turrets: false,
            chair_claims: vec![0; buildings],
            loadouts: profiles.iter().map(Loadout::of).collect(),
            traffic: Vec::with_capacity(fighters.len().min(MAX_CRAFT)),
            systems: Vec::with_capacity(MAX_SYSTEMS.min(buildings)),
            held: vec![None; unit_slots],
            routes: Routes::build(bounds, pads, map),
            tick: 0,
            routed_starts_this_tick: 0,
            census: PilotCensus::default(),
            probe: None,
            disabled: false,
            defs: ["cannon", "launcher", "remote"].map(|base| crate::combat::defs::JsObj {
                fields: vec![(std::rc::Rc::from("base"), crate::combat::defs::Js::Str(std::rc::Rc::from(base)))],
                def: None,
            }),
        }
    }

    /// The class the released aim code reads for a weapon: a guided rocket is
    /// remote, a missile or torpedo a launcher, every other craft or turret
    /// weapon a cannon.
    fn def_of(&self, w: &super::flight::SpaceWeaponSpec) -> &crate::combat::defs::JsObj {
        &self.defs[def_index(w.ordnance_kind)]
    }

    pub fn view(&self, slot: usize) -> Option<PilotView> {
        let b = self.bots.get(slot).filter(|b| b.active)?;
        let seated = b.receipt.is_some() || b.phase == PilotPhase::Walk;
        let seated = seated && !matches!(b.phase, PilotPhase::ToChair | PilotPhase::Chair);
        Some(PilotView { phase: b.phase, craft: seated.then_some(b.craft), seat: b.seat, target: b.target, since: b.since, step: b.step, run: b.run as u8, aim_point: b.run_aim })
    }

    fn is_held(&self, slot: usize, life: f64) -> bool {
        self.held.get(slot).copied().flatten().is_some_and(|held| held.to_bits() == life.to_bits())
    }

    /// Holds or frees one life; a hold cancels a walk to a seat or a chair.
    /// Returns `(accepted, stop_external_nav_walk)`.
    fn hold(&mut self, slot: usize, life: f64, hold: bool) -> (bool, bool) {
        let Some(h) = self.held.get_mut(slot) else { return (false, false) };
        *h = hold.then_some(life);
        let mut stop_nav = false;
        if hold {
            if let Some(b) = self.bots.get(slot).copied() {
                if b.active && b.life.to_bits() == life.to_bits() && matches!(b.phase, PilotPhase::Walk | PilotPhase::ToChair) {
                    stop_nav = b.routed;
                    self.release(slot);
                    self.bots[slot] = Bot {
                        phase: PilotPhase::Free,
                        since: self.tick,
                        routed: false,
                        routed_moved: false,
                        ..b
                    };
                }
            }
        }
        (true, stop_nav)
    }

    fn stop_routed_walk(&mut self, world: &mut World, bot: &mut Bot, unit: Ref<Unit>) {
        if bot.routed {
            world.nav_walk_stop(unit);
            bot.routed = false;
        }
        bot.routed_moved = false;
    }

    /// A person moved this bot: it now holds `seat` (a gunner's, or the
    /// pilot's of a craft in `state`), or stands on its feet again.
    pub(crate) fn reseat(&mut self, slot: usize, seat: Option<SpaceSeat>, state: SpaceFlightState) {
        let tick = self.tick;
        if self.bots.get(slot).is_none_or(|b| !b.active) {
            return;
        }
        self.release(slot);
        let bot = &mut self.bots[slot];
        bot.receipt = seat;
        bot.since = tick;
        bot.aiming = false;
        bot.target = PilotTarget::None;
        match seat {
            None => {
                bot.phase = PilotPhase::Free;
                bot.cooldown_until = tick.saturating_add(20 * THINK_TICKS);
            }
            Some(s) => {
                bot.craft = s.fighter;
                bot.craft_gen = s.generation;
                bot.seat = s.seat;
                bot.phase = match (s.seat, state) {
                    (0, SpaceFlightState::Landed) => PilotPhase::Hangar,
                    (0, _) => PilotPhase::Fight,
                    _ => PilotPhase::Gunner,
                };
                bot.step = 0;
            }
        }
    }

    fn release(&mut self, slot: usize) {
        let mark = slot as u32 + 1;
        for row in &mut self.claims {
            for c in row.iter_mut() {
                if *c == mark {
                    *c = 0;
                }
            }
        }
        for c in &mut self.chair_claims {
            if *c == mark {
                *c = 0;
            }
        }
    }

    fn step(&mut self, world: &mut World, dt: f64) {
        self.tick = self.tick.saturating_add(1);
        self.routed_starts_this_tick = 0;
        self.lure = self.lure_radius(world);
        self.snapshot(world);
        self.reserve_for_humans(world);
        let slots = world.sim.store.unit.slots() as usize;
        if slots > self.bots.len() {
            bump(&mut self.census.unclaimed_slots, count(slots - self.bots.len()));
        }
        for slot in 0..slots.min(self.bots.len()) {
            self.step_slot(world, slot, dt);
        }
    }

    /// How far a waiting craft lures a bot: the mission's notify radius as its
    /// host passed it, else the battle's bot controller's (the released
    /// default without a bot controller).
    fn lure_radius(&self, world: &World) -> f64 {
        self.notify_radius.unwrap_or_else(|| world.ai.battle().map_or(RELEASED_NOTIFY_RADIUS, |b| b.vehicles.settings.notify_radius))
    }

    /// The craft humans on foot are heading for: for each human not aboard a
    /// craft, the free craft of their team nearest them within the lure
    /// radius, when they close on it or stand at it. The released bot
    /// controller has no such rule; a bot does not take that craft's pilot
    /// seat from the human. One pass over the units and, per human, two over
    /// the craft.
    fn reserve_for_humans(&mut self, world: &World) {
        self.human_near.iter_mut().for_each(|d| *d = f64::INFINITY);
        let Some(space) = world.space() else { return };
        for slot in 0..world.sim.store.unit.slots() {
            let Some((r, u)) = world.sim.store.unit.at(slot) else { continue };
            if !u.human || !u.alive || u.combat.dead || u.removed {
                continue;
            }
            let UnitBody::Soldier(s) = &u.soldier else { continue };
            let aboard = |p: &Option<super::flight::SpacePilotId>| {
                p.is_some_and(|p| world.sim.store.owner_snapshot.get(p.owner).is_some_and(|o| o.source_unit == r && o.generation.to_bits() == u.generation.to_bits()))
            };
            if space.fighters.iter().take(MAX_CRAFT).any(|f| f.alive && (aboard(&f.pilot) || f.gunners.iter().any(aboard))) {
                continue;
            }
            let mut best: Option<(f64, usize)> = None;
            for f in space.fighters.iter().take(MAX_CRAFT) {
                if !f.alive || f.team != u.team as i32 || f.state != SpaceFlightState::Landed || f.pilot.is_some() {
                    continue;
                }
                let d = dist(f.position, s.position);
                if d <= self.lure && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, f.id));
                }
            }
            let Some((d, id)) = best else { continue };
            let to = sub(space.fighters[id].position, s.position);
            let level = unit([to[0], 0.0, to[2]]);
            let closing = s.velocity[0] * level[0] + s.velocity[2] * level[2];
            if closing >= PROVISIONAL_HUMAN_CLOSING || d <= PROVISIONAL_HUMAN_AT {
                if let Some(near) = self.human_near.get_mut(id) {
                    *near = v8math::min(*near, d);
                }
            }
        }
    }

    /// The craft aloft and the enemy systems worth attacking, this tick.
    fn snapshot(&mut self, world: &World) {
        self.traffic.clear();
        let Some(space) = world.space() else { return };
        if space.fighters.len() > MAX_CRAFT {
            bump(&mut self.census.craft_truncated, count(space.fighters.len() - MAX_CRAFT));
        }
        for f in space.fighters.iter().take(MAX_CRAFT) {
            if !f.alive || f.state == SpaceFlightState::Landed || self.traffic.len() == self.traffic.capacity() {
                continue;
            }
            self.traffic.push(Traffic {
                id: f.id,
                generation: f.generation,
                team: f.team,
                position: f.position,
                velocity: f.velocity,
                reach: hull_reach(&space.profiles[f.profile_index]),
            });
        }
        // The ship objects worth attacking change slowly: one pass over the
        // attachments every `SYSTEM_SCAN_TICKS`; attacks check their own target.
        if !self.tick.saturating_sub(1).is_multiple_of(SYSTEM_SCAN_TICKS) || !crate::units::f3::active_in(&world.units) {
            return;
        }
        self.systems.clear();
        for att in &space.buildings {
            // Lockable ship systems, the self-piloted guns that defend them, and
            // the hull sections that carry a ship's shield while it is up (a hit
            // on any of them comes off the shared shield).
            let gun = att.turret.as_ref().is_some_and(|t| t.pilot_type == super::objects::SpaceTurretPilotType::SelfPiloted);
            let shields = crate::units::f3_props::mission_shields(&world.units, att.building_index);
            let hull = !att.lock_on_enabled && !gun && shields.maximum > 0.0;
            if !(att.lock_on_enabled || gun || (hull && shields.current > 0.0)) {
                continue;
            }
            if self.systems.len() == self.systems.capacity() {
                bump(&mut self.census.systems_truncated, 1);
                continue;
            }
            let Some(facts) = super::building_core(world, att.building_index) else { continue };
            if !facts.alive || facts.team <= 0 {
                continue;
            }
            // Bots cannot destroy an object with an AI damage floor (the
            // mission keeps its last share for human players): not attacked.
            if att.ai_damage_threshold > 0.0 {
                continue;
            }
            self.systems.push(System {
                index: att.building_index,
                life: crate::units::f3_props::owner_in(world, att.building_index).life,
                team: facts.team,
                center: att.center,
                radius: att.radius,
                bodies: att.bodies.map(|b| b.map_or(-1, |b| b as i32)),
                gun_reach: att
                    .turret
                    .as_ref()
                    .filter(|t| gun && t.filter.target_vehicle)
                    .map_or(0.0, |t| v8math::min(t.filter.max_range, t.weapon.round_reach())),
                lockable: att.lock_on_enabled,
                shield: shields.current,
                exposure: 0.0,
                hull,
                left: if shields.current > 0.0 && shields.maximum > 0.0 {
                    shields.current / shields.maximum
                } else if facts.max_health > 0.0 {
                    facts.health / facts.max_health
                } else {
                    1.0
                },
            });
        }
        // Each object's cover by its own team's guns (bounded by the list's size).
        for i in 0..self.systems.len() {
            let (team, at) = (self.systems[i].team, self.systems[i].center);
            let cover: f64 = self
                .systems
                .iter()
                .enumerate()
                .filter(|(j, g)| *j != i && g.team == team && g.gun_reach > 0.0)
                .map(|(_, g)| v8math::max(0.0, 1.0 - dist(g.center, at) / g.gun_reach))
                .sum();
            self.systems[i].exposure = cover;
        }
    }

    fn step_slot(&mut self, world: &mut World, slot: usize, dt: f64) {
        let mut bot = self.bots[slot];
        let Some(u) = soldier(world, slot as u32) else {
            if bot.active {
                let prev = bot.unit;
                self.stop_routed_walk(world, &mut bot, prev);
                self.release(slot);
                self.bots[slot] = Bot::default();
            }
            return;
        };
        if !bot.active || bot.life.to_bits() != u.life.to_bits() {
            if bot.active {
                let prev = bot.unit;
                self.stop_routed_walk(world, &mut bot, prev);
                self.release(slot);
            }
            // A hold names one life; a new life in the slot starts unheld.
            if let Some(h) = self.held.get_mut(slot) {
                if h.is_some_and(|life| life.to_bits() != u.life.to_bits()) {
                    *h = None;
                }
            }
            if !u.controlled {
                self.bots[slot] = Bot::default();
                return;
            }
            bot = Bot { unit: u.r, ..Bot::fresh(u.life, self.tick) };
        }
        bot.unit = u.r;
        if let Some(seat) = bot.receipt {
            if world.space_seat_fighter(seat).is_none() {
                self.release(slot);
                bot.receipt = None;
                bot.phase = PilotPhase::Free;
                bot.since = self.tick;
                bot.cooldown_until = self.tick.saturating_add(2 * THINK_TICKS);
            }
        }
        if let Some(seat) = bot.terminal {
            if !world.space_terminal_current(seat) {
                self.release(slot);
                bot.terminal = None;
                bot.phase = PilotPhase::Free;
                bot.since = self.tick;
                bot.cooldown_until = self.tick.saturating_add(4 * THINK_TICKS);
            }
        }
        match bot.phase {
            PilotPhase::Free => self.free(world, slot, &mut bot, &u),
            PilotPhase::Walk => self.walk(world, slot, &mut bot, &u, dt),
            PilotPhase::ToChair => self.walk_to_chair(world, slot, &mut bot, &u, dt),
            PilotPhase::Chair => {
                seated_intent(world, u.r);
                self.chair(world, slot, &mut bot, dt);
            }
            PilotPhase::Gunner => {
                seated_intent(world, u.r);
                self.gunner(world, slot, &mut bot, dt);
            }
            _ => {
                seated_intent(world, u.r);
                self.pilot(world, slot, &mut bot, dt);
            }
        }
        self.bots[slot] = bot;
    }

    /// A free bot looks for a seat on its own think tick.
    fn free(&mut self, world: &mut World, slot: usize, bot: &mut Bot, u: &Soldier) {
        if !u.controlled || self.is_held(slot, u.life) || self.tick < bot.cooldown_until || !phase(self.tick, slot).is_multiple_of(THINK_TICKS) {
            return;
        }
        let Some(space) = world.space() else { return };
        let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); PROVISIONAL_SEAT_CHOICES];
        for f in space.fighters.iter().take(MAX_CRAFT) {
            if !f.alive || f.team != u.team || f.state != SpaceFlightState::Landed {
                continue;
            }
            let d = dist(f.position, u.position);
            if d > self.lure {
                continue;
            }
            let profile = &space.profiles[f.profile_index];
            let transport = profile.is_command_flyer && profile.spawn_point_count > 0;
            let claims = &self.claims[f.id];
            // A human heading for the craft keeps its pilot seat.
            let pilot_free = f.pilot.is_none() && claims[0] == 0 && self.human_near[f.id].is_infinite();
            let join = if transport { PROVISIONAL_CREW_LURE } else { PROVISIONAL_GUNNER_JOIN };
            let seat = if pilot_free {
                Some(0u8)
            } else if d <= join && f.landed_zone.is_some_and(|z| space.bounds.landing_zones.get(z).is_some_and(|zone| zone_team(&self.routes, zone) == u.team)) {
                // A gunner seat, or on a transport any free seat: crew ride along.
                (1..profile.seat_count.min(6)).find(|&s| {
                    f.occupant(s).is_none() && claims[s as usize] == 0 && (transport || profile.weapons.iter().any(|w| w.seat == s))
                })
            } else {
                None
            };
            let Some(seat) = seat else { continue };
            // Pilots take a fighter before a transport's seat; marines ride
            // transports before they fly fighters.
            let score = d + match (seat > 0, u.pilot_class) {
                (true, true) => 15.0,
                (true, false) => -20.0,
                (false, false) if !transport => 30.0,
                _ => 0.0,
            };
            rank_into(&mut ranked, (score, f.id, seat));
        }
        if ranked[0].1 == usize::MAX {
            self.free_chair(world, slot, bot, u);
            return;
        }
        // Fast path: the nearest seat with a clear straight walk line.
        let mut chosen = None;
        for &(_, id, seat) in ranked.iter().filter(|r| r.1 != usize::MAX) {
            let (craft_gen, target, body) = {
                let Some(space) = world.space() else { return };
                let f = &space.fighters[id];
                (f.generation, board_point(f, &space.profiles[f.profile_index], u.position), f.kernel_body)
            };
            if walk_line_clear(world, u.position, target, body) {
                chosen = Some((id, seat, craft_gen, target, false, false));
                break;
            }
            bump(&mut self.census.blocked_seats, 1);
        }
        // Fallback: when every straight ray is blocked, route through 2005
        // navigation's external ground-walk seam, capped per tick.
        if chosen.is_none() {
            if self.routed_starts_this_tick >= PROVISIONAL_ROUTED_STARTS_PER_TICK {
                return;
            }
            let count = ranked.iter().take_while(|r| r.1 != usize::MAX).count();
            if count > 0 {
                let pick = ((self.tick / THINK_TICKS) as usize + slot) % count;
                let (_, id, seat) = ranked[pick];
                let (craft_gen, target) = {
                    let Some(space) = world.space() else { return };
                    let f = &space.fighters[id];
                    (f.generation, board_point(f, &space.profiles[f.profile_index], u.position))
                };
                self.routed_starts_this_tick = self.routed_starts_this_tick.saturating_add(1);
                let status = super::hulls_meet(world, HullMeets::Soldiers, |w| {
                    w.nav_walk_begin(u.r, target)
                });
                if status != crate::world::owner::NavWalkStatus::Failed {
                    chosen = Some((
                        id,
                        seat,
                        craft_gen,
                        target,
                        true,
                        status == crate::world::owner::NavWalkStatus::Moving,
                    ));
                }
            }
        }
        let Some((id, seat, craft_gen, target, routed, routed_moved)) = chosen else {
            bot.cooldown_until = self.tick.saturating_add(2 * THINK_TICKS);
            return;
        };
        self.claims[id][seat as usize] = slot as u32 + 1;
        bump(&mut self.census.claims, 1);
        *bot = Bot {
            phase: PilotPhase::Walk,
            since: self.tick,
            craft: id,
            craft_gen,
            seat,
            routed,
            routed_moved,
            check_tick: self.tick,
            check_dist: dist(u.position, target),
            stalls: 0,
            ..*bot
        };
    }

    fn walk(&mut self, world: &mut World, slot: usize, bot: &mut Bot, u: &Soldier, dt: f64) {
        let Some(space) = world.space() else { return };
        let f = &space.fighters[bot.craft];
        let still = f.alive && f.generation == bot.craft_gen && f.state == SpaceFlightState::Landed && f.occupant(bot.seat).is_none();
        let target = board_point(f, &space.profiles[f.profile_index], u.position);
        let body = f.kernel_body;
        // A human heading for this craft and nearer to it than the bot gets
        // its pilot seat.
        let yield_to_human = bot.seat == 0 && self.human_near.get(bot.craft).is_some_and(|&h| h < dist(f.position, u.position));
        if !still || yield_to_human {
            self.stop_routed_walk(world, bot, u.r);
            self.release(slot);
            if yield_to_human {
                bump(&mut self.census.yields_to_humans, 1);
            }
            *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(THINK_TICKS), ..*bot };
            return;
        }
        let d = v8math::hypot(target[0] - u.position[0], target[2] - u.position[2]);
        if d < 6.0 {
            if let Some(seat) = world.space_board_seat(&u.key, bot.craft, bot.seat) {
                self.stop_routed_walk(world, bot, u.r);
                bot.receipt = Some(seat);
                bot.since = self.tick;
                if bot.seat == 0 {
                    bot.phase = PilotPhase::Hangar;
                    bot.step = 0;
                    bump(&mut self.census.boardings, 1);
                } else {
                    bot.phase = PilotPhase::Gunner;
                    bump(&mut self.census.gunner_boardings, 1);
                }
                seated_intent(world, u.r);
                return;
            }
        }
        if bot.routed {
            let elapsed = self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT;
            // Once on the same deck level with a clear straight line, release
            // the routed walk and finish on the direct fast path.
            if (target[1] - u.position[1]).abs() < 1.5
                && walk_line_clear(world, u.position, target, body)
            {
                self.stop_routed_walk(world, bot, u.r);
                bot.since = self.tick;
                bot.check_tick = self.tick;
                bot.check_dist = dist(u.position, target);
                bot.stalls = 0;
            } else if elapsed > PROVISIONAL_ROUTED_WALK_LIMIT_S {
                self.stop_routed_walk(world, bot, u.r);
                self.release(slot);
                bump(&mut self.census.walk_failures, 1);
                *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(10 * THINK_TICKS), stalls: 0, ..*bot };
                return;
            } else {
                let status = super::hulls_meet(world, HullMeets::Soldiers, |w| {
                    w.nav_walk_step(u.r, dt)
                });
                match status {
                    crate::world::owner::NavWalkStatus::Moving => {
                        bot.routed_moved = true;
                        return;
                    }
                    crate::world::owner::NavWalkStatus::Waiting => {
                        return;
                    }
                    crate::world::owner::NavWalkStatus::Arrived => {
                        bot.routed = false;
                        bot.routed_moved = false;
                        if let Some(seat) = world.space_board_seat(&u.key, bot.craft, bot.seat) {
                            bot.receipt = Some(seat);
                            bot.since = self.tick;
                            if bot.seat == 0 {
                                bot.phase = PilotPhase::Hangar;
                                bot.step = 0;
                                bump(&mut self.census.boardings, 1);
                            } else {
                                bot.phase = PilotPhase::Gunner;
                                bump(&mut self.census.gunner_boardings, 1);
                            }
                            seated_intent(world, u.r);
                            return;
                        }
                        bot.since = self.tick;
                        bot.check_tick = self.tick;
                        bot.check_dist = dist(u.position, target);
                        bot.stalls = 0;
                    }
                    crate::world::owner::NavWalkStatus::Failed => {
                        let moved = bot.routed_moved;
                        bot.routed = false;
                        bot.routed_moved = false;
                        self.release(slot);
                        let cooldown = if moved {
                            bump(&mut self.census.walk_failures, 1);
                            10 * THINK_TICKS
                        } else {
                            2 * THINK_TICKS
                        };
                        *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(cooldown), stalls: 0, ..*bot };
                        return;
                    }
                }
            }
        }
        let elapsed = self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT;
        if self.tick >= bot.check_tick.saturating_add(60) {
            let now = dist(u.position, target);
            bot.stalls = if now > bot.check_dist - 1.0 { bot.stalls + 1 } else { 0 };
            bot.check_dist = now;
            bot.check_tick = self.tick;
        }
        if bot.stalls >= 3 || elapsed > PROVISIONAL_WALK_LIMIT_S {
            self.release(slot);
            bump(&mut self.census.walk_failures, 1);
            *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(10 * THINK_TICKS), stalls: 0, ..*bot };
            return;
        }
        let want = v8math::atan2(target[0] - u.position[0], target[2] - u.position[2]);
        let err = wrap_pi(want - u.yaw);
        let forward = if err.abs() > 1.2 { 0.0 } else if err.abs() > 0.5 || d < 1.5 { 0.4 } else { 1.0 };
        walk_intent(world, u.r, forward, clamp(-err * 3.0, -1.0, 1.0));
    }

    /// A free bot with no craft seat in reach takes a free remote turret chair
    /// of its own ship within the notify radius, directly on the same deck or
    /// routed through interior navigation.
    fn free_chair(&mut self, world: &mut World, slot: usize, bot: &mut Bot, u: &Soldier) {
        // Unless the mission keeps bots in turrets, the 2005 game seats a bot
        // at a stationary gun only while it has a target the gun can turn to.
        let target = if stay_in_turrets(self.stay_in_turrets, world) {
            None
        } else {
            match current_target(world, u.r, u.team).or_else(|| self.craft_seen(world, u)) {
                Some(p) => Some(p),
                None => return,
            }
        };
        let Some(space) = world.space.as_deref() else { return };
        let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); PROVISIONAL_SEAT_CHOICES];
        for (i, att) in space.buildings.iter().enumerate() {
            let Some(term) = att.terminal.as_ref() else { continue };
            if term.occupant.is_some() || term.remote_turret.is_none() || self.chair_claims.get(i).copied().unwrap_or(1) != 0 {
                continue;
            }
            if target.is_some_and(|p| !term.remote_turret.is_some_and(|gun| gun_turns_to(space, gun, p))) {
                continue;
            }
            let d = dist(att.center, u.position);
            if d > self.lure {
                continue;
            }
            // Only a chair aboard the bot's own ship: its nearest opening is the team's.
            let own = self.routes.exits.iter().min_by(|a, b| dist(a.center, att.center).total_cmp(&dist(b.center, att.center))).is_some_and(|e| e.team == u.team);
            let Some(facts) = super::building_core(world, att.building_index) else { continue };
            if !own || !facts.alive || (facts.team != 0 && facts.team != u.team) {
                continue;
            }
            rank_into(&mut ranked, (d, i, 0));
        }
        if ranked[0].1 == usize::MAX {
            return;
        }
        // Fast path: nearest chair on the same deck with a clear straight walk line.
        let mut chosen = None;
        for &(_, i, _) in ranked.iter().filter(|r| r.1 != usize::MAX) {
            let (target, body) = {
                let Some(space) = world.space.as_deref() else { return };
                let att = &space.buildings[i];
                (att.center, att.bodies[0])
            };
            if (target[1] - u.position[1]).abs() < 1.5 && walk_line_clear(world, u.position, target, body) {
                chosen = Some((i, target, false, false));
                break;
            }
        }
        // Fallback: route through 2005 navigation's external ground-walk seam
        // with an approach radius matching the chair's boarding reach so chairs
        // inside their own navigation barrier remain reachable from any deck.
        if chosen.is_none() {
            if self.routed_starts_this_tick >= PROVISIONAL_ROUTED_STARTS_PER_TICK {
                return;
            }
            let count = ranked.iter().take_while(|r| r.1 != usize::MAX).count();
            if count > 0 {
                let pick = ((self.tick / THINK_TICKS) as usize + slot) % count;
                let (_, i, _) = ranked[pick];
                let (target, reach) = {
                    let Some(space) = world.space.as_deref() else { return };
                    let att = &space.buildings[i];
                    (att.center, att.radius + super::PROVISIONAL_BOARDING_REACH)
                };
                let approach = v8math::min(v8math::max(reach - 0.6, 0.0), 3.0);
                self.routed_starts_this_tick = self.routed_starts_this_tick.saturating_add(1);
                let status = super::hulls_meet(world, HullMeets::Soldiers, |w| {
                    w.nav_walk_begin_within(u.r, target, approach)
                });
                if status != crate::world::owner::NavWalkStatus::Failed {
                    chosen = Some((
                        i,
                        target,
                        true,
                        status == crate::world::owner::NavWalkStatus::Moving,
                    ));
                }
            }
        }
        let Some((i, target, routed, routed_moved)) = chosen else {
            bot.cooldown_until = self.tick.saturating_add(2 * THINK_TICKS);
            return;
        };
        self.chair_claims[i] = slot as u32 + 1;
        *bot = Bot {
            phase: PilotPhase::ToChair,
            since: self.tick,
            chair: i,
            routed,
            routed_moved,
            check_tick: self.tick,
            check_dist: dist(u.position, target),
            stalls: 0,
            ..*bot
        };
    }

    /// The nearest enemy craft a soldier on foot sees, as the 2005 game's
    /// sight has it: someone aboard, in front of him (within 77.5 degrees of
    /// his facing, or within 10 m), no farther than its model sphere's radius
    /// times 20 plus 30 m (twice that with someone human aboard), and a clear
    /// line from his eyes to it (a hangar's shield does not stop the eye).
    fn craft_seen(&self, world: &mut World, u: &Soldier) -> Option<[f64; 3]> {
        let eye = add(u.position, [0.0, SOLDIER_EYE_M, 0.0]);
        let facing = [v8math::sin(u.yaw), 0.0, v8math::cos(u.yaw)];
        let mut best: Option<(f64, [f64; 3], f64, i32)> = None;
        for t in self.traffic.iter().filter(|t| t.team != u.team && t.team != 0) {
            let Some((radius, human, body)) = world.space().and_then(|s| {
                let f = s.fighters.get(t.id).filter(|f| f.occupied())?;
                Some((s.profiles[f.profile_index].sphere[3], super::aim::human_aboard(f), f.kernel_body.map_or(-1, |b| b as i32)))
            }) else { continue };
            let d = dist(t.position, eye);
            let range = (radius * SIGHT_RADIUS_SCALE + SIGHT_BASE_M) * if human { 2.0 } else { 1.0 };
            let ahead = d < SIGHT_ALL_ROUND_M || dot(facing, scale(sub(t.position, eye), 1.0 / v8math::max(d, 1e-6))) >= SIGHT_HALF_FIELD_COS;
            if d <= range && ahead && best.is_none_or(|b| d < b.0) {
                best = Some((d, t.position, t.reach, body));
            }
        }
        let (_, at, reach, body) = best?;
        line_clear(world, eye, at, reach, -1, [body, -1, -1]).then_some(at)
    }

    fn walk_to_chair(&mut self, world: &mut World, slot: usize, bot: &mut Bot, u: &Soldier, dt: f64) {
        let Some(space) = world.space.as_deref() else { return };
        let Some(att) = space.buildings.get(bot.chair) else { return };
        let (index, target, reach) = (att.building_index, att.center, att.radius + super::PROVISIONAL_BOARDING_REACH);
        let taken = att.terminal.as_ref().is_none_or(|t| t.occupant.is_some());
        if taken {
            self.stop_routed_walk(world, bot, u.r);
            self.release(slot);
            *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(THINK_TICKS), ..*bot };
            return;
        }
        if dist(target, u.position) < reach - 0.3 {
            if let Some(seat) = world.space_board_terminal(&u.key, index) {
                self.stop_routed_walk(world, bot, u.r);
                bot.terminal = Some(seat);
                bot.phase = PilotPhase::Chair;
                bot.since = self.tick;
                bump(&mut self.census.chair_boardings, 1);
                seated_intent(world, u.r);
                return;
            }
        }
        if bot.routed {
            let elapsed = self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT;
            if elapsed > PROVISIONAL_ROUTED_WALK_LIMIT_S {
                self.stop_routed_walk(world, bot, u.r);
                self.release(slot);
                bump(&mut self.census.walk_failures, 1);
                *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(10 * THINK_TICKS), stalls: 0, ..*bot };
                return;
            }
            let status = super::hulls_meet(world, HullMeets::Soldiers, |w| {
                w.nav_walk_step(u.r, dt)
            });
            match status {
                crate::world::owner::NavWalkStatus::Moving => {
                    bot.routed_moved = true;
                    return;
                }
                crate::world::owner::NavWalkStatus::Waiting => {
                    return;
                }
                crate::world::owner::NavWalkStatus::Arrived => {
                    bot.routed = false;
                    bot.routed_moved = false;
                    if let Some(seat) = world.space_board_terminal(&u.key, index) {
                        bot.terminal = Some(seat);
                        bot.phase = PilotPhase::Chair;
                        bot.since = self.tick;
                        bump(&mut self.census.chair_boardings, 1);
                        seated_intent(world, u.r);
                        return;
                    }
                    bot.since = self.tick;
                    bot.check_tick = self.tick;
                    bot.check_dist = dist(u.position, target);
                    bot.stalls = 0;
                }
                crate::world::owner::NavWalkStatus::Failed => {
                    let moved = bot.routed_moved;
                    bot.routed = false;
                    bot.routed_moved = false;
                    self.release(slot);
                    let cooldown = if moved {
                        bump(&mut self.census.walk_failures, 1);
                        10 * THINK_TICKS
                    } else {
                        2 * THINK_TICKS
                    };
                    *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(cooldown), stalls: 0, ..*bot };
                    return;
                }
            }
        }
        if self.tick >= bot.check_tick.saturating_add(60) {
            let now = dist(u.position, target);
            bot.stalls = if now > bot.check_dist - 1.0 { bot.stalls + 1 } else { 0 };
            bot.check_dist = now;
            bot.check_tick = self.tick;
        }
        if bot.stalls >= 3 || self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_WALK_LIMIT_S {
            self.release(slot);
            bump(&mut self.census.walk_failures, 1);
            *bot = Bot { phase: PilotPhase::Free, since: self.tick, cooldown_until: self.tick.saturating_add(10 * THINK_TICKS), stalls: 0, ..*bot };
            return;
        }
        let want = v8math::atan2(target[0] - u.position[0], target[2] - u.position[2]);
        let err = wrap_pi(want - u.yaw);
        let d = v8math::hypot(target[0] - u.position[0], target[2] - u.position[2]);
        let forward = if err.abs() > 1.2 { 0.0 } else if err.abs() > 0.5 || d < 1.5 { 0.4 } else { 1.0 };
        walk_intent(world, u.r, forward, clamp(-err * 3.0, -1.0, 1.0));
    }

    /// One tick at a remote turret chair: the linked gun follows the nearest
    /// enemy craft in its reach that it can see, with lead, and fires when its
    /// barrel lies on its lead point and the line is clear. Without a target
    /// the trigger is released.
    fn chair(&mut self, world: &mut World, slot: usize, bot: &mut Bot, dt: f64) {
        let Some(seat) = bot.terminal else { return };
        let think = phase(self.tick, slot).is_multiple_of(THINK_TICKS);
        let team = world.space_owner_facts(seat.owner).map_or(0, |o| o.team);
        let curves = skill_curves(world, team);
        let (center, radius, speed, range, bore, own, weapon, gun_index, bubble) = {
            let Some(space) = world.space.as_deref() else { return };
            let Some(gun_index) = space.buildings.iter().position(|a| a.building_index == seat.turret) else { return };
            let gun = &space.buildings[gun_index];
            let Some(turret) = gun.turret.as_ref() else { return };
            (gun.center, gun.radius, turret.weapon.round_speed(), turret.filter.max_range, barrel(turret, &gun.matrix), gun.bodies[0].map_or(-1, |b| b as i32), turret.weapon.ordnance_kind, gun_index, turret.weapon.ai_bubble)
        };
        let muzzle_toward = |p: [f64; 3]| add(center, scale(unit(sub(p, center)), radius + 1.0));
        if think {
            bump(&mut self.census.target_searches, 1);
            let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); PROVISIONAL_SIGHT_CHOICES];
            for (i, t) in self.traffic.iter().enumerate() {
                let d = dist(t.position, center);
                if t.team != team && t.team != 0 && d <= range {
                    rank_into(&mut ranked, (d, i, 0));
                }
            }
            bot.target = PilotTarget::None;
            for &(_, i, _) in ranked.iter().filter(|r| r.1 != usize::MAX) {
                let t = self.traffic[i];
                let body = world.space().and_then(|s| s.fighters.get(t.id)).and_then(|f| f.kernel_body).map_or(-1, |b| b as i32);
                let from = muzzle_toward(t.position);
                if !friend_in_line(&self.traffic, team, usize::MAX, t.id, from, t.position) && line_clear(world, from, t.position, t.reach, own, [body, -1, -1]) {
                    bot.target = PilotTarget::Craft { index: t.id, generation: t.generation };
                    bot.run_until = self.tick;
                    // The firing line is tested afresh from the sight.
                    bot.line_tick = self.tick;
                    break;
                }
            }
        }
        let idle_s = self.tick.saturating_sub(bot.run_until.max(bot.since)) as f64 * super::SPACE_FIXED_DT;
        let sat_s = self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT;
        let idle_limit = if stay_in_turrets(self.stay_in_turrets, world) { RELEASED_CHAIR_IDLE_STAYING_S } else { RELEASED_CHAIR_IDLE_S };
        if idle_s > idle_limit || sat_s > PROVISIONAL_CHAIR_LIMIT_S {
            world.space_leave_terminal(seat);
            self.release(slot);
            bot.terminal = None;
            bot.phase = PilotPhase::Free;
            bot.since = self.tick;
            bot.cooldown_until = self.tick.saturating_add(20 * THINK_TICKS);
            return;
        }
        let target = match bot.target {
            PilotTarget::Craft { index, generation } => world
                .space()
                .filter(|s| craft_alive(s, index, generation))
                .map(|s| &s.fighters[index])
                .map(|t| (t.position, t.velocity, hull_reach(&world.space().unwrap().profiles[t.profile_index]), t.kernel_body.map_or(-1, |b| b as i32))),
            _ => None,
        };
        let Some((position, velocity, reach, body)) = target else {
            // Nothing to shoot at: the trigger is let go.
            bot.target = PilotTarget::None;
            bot.aiming = false;
            bot.salvo_down = false;
            world.space_control_terminal(seat, bore, false);
            return;
        };
        let from = muzzle_toward(position);
        let mut point = lead(from, position, velocity, speed);
        // The chair's gun is aimed by its own turret step, as a self-piloted
        // gun is: off a craft's lead by its sphere of miss.
        if let PilotTarget::Craft { index, .. } = bot.target {
            if let Some(dir) = world.space().and_then(|s| s.gun_spheres.get(gun_index)).map(|m| m.dir) {
                point = miss_aim(world, dir, index, center, point, bubble);
            }
        }
        let desired = unit(sub(point, from));
        if !bot.aiming {
            bot.aim.reset(&Vector3::new(bore[0], bore[1], bore[2]));
            bot.aiming = true;
        }
        let (filtered, scales) = bot.aim.update(&Vector3::new(desired[0], desired[1], desired[2]), dt, Some(&curves), Some(&self.defs[def_index(weapon)]), false);
        let sight = unit([filtered.x, filtered.y, filtered.z]);
        let spread_scale = scales.map(|s| s.spread_scale);
        let span = dist(position, center);
        let aligned = v8math::acos(clamp(dot(bore, desired), -1.0, 1.0)) <= v8math::max(PROVISIONAL_FIRE_CONE, v8math::atan2(reach, v8math::max(span, 1.0)));
        let mut fire = aligned && span <= range;
        if fire {
            if self.tick >= bot.line_tick {
                let index = match bot.target {
                    PilotTarget::Craft { index, .. } => index,
                    _ => usize::MAX,
                };
                bot.line_ok = !friend_in_line(&self.traffic, team, usize::MAX, index, from, position) && line_clear(world, from, position, reach, own, [body, -1, -1]);
                bot.line_tick = self.tick.saturating_add(LINE_TICKS);
            }
            if !bot.line_ok {
                bump(&mut self.census.blocked_shots, 1);
                fire = false;
            }
        }
        if fire && !bot.salvo_down {
            bump(&mut self.census.chair_bursts, 1);
        }
        bot.salvo_down = fire;
        world.space_control_terminal_scaled(seat, sight, fire, spread_scale);
    }

    /// One tick of a bot in its craft's pilot seat.
    fn pilot(&mut self, world: &mut World, slot: usize, bot: &mut Bot, dt: f64) {
        let Some(seat) = bot.receipt else { return };
        let (f, pv, in_zone, center, repairs) = {
            let Some(space) = world.space.as_deref() else { return };
            let fighter = &space.fighters[seat.fighter];
            let profile = &space.profiles[fighter.profile_index];
            // The pilot's class repairs the craft in flight.
            let repairs = fighter.pilot.is_some_and(|p| p.repair_capable) && profile.pilot_skill_repair_scale > 0.0;
            (fighter.clone_view(), ProfileView::of(profile), space.bounds.landing_zone_at(fighter.position).is_some(), space.bounds.center, repairs)
        };
        let loadout = self.loadouts[f.profile_index];
        let mut input = SpacePilotInput::default();
        let tick = self.tick;
        let think = phase(tick, slot).is_multiple_of(THINK_TICKS);
        if f.contacts != bot.contacts {
            // A bout of contact: ticks with a contact no more than 15 apart.
            // Each new bout, and each further pinned stretch of one, is a scrape.
            if tick > bot.contact_until.saturating_add(15) || bot.contact_until == 0 {
                bot.bout_start = tick;
                bot.scrapes = bot.scrapes.saturating_add(1);
            } else if tick.saturating_sub(bot.bout_start) >= PROVISIONAL_PINNED_TICKS {
                bot.bout_start = tick;
                bot.scrapes = bot.scrapes.saturating_add(1);
                bump(&mut self.census.pinned, 1);
                if matches!(bot.phase, PilotPhase::Return | PilotPhase::Raid) {
                    // Pinned on the way in: back off and line up again.
                    bot.step = bot.step.min(1);
                }
            }
            bot.contacts = f.contacts;
            bot.contact_until = tick;
            // Touching a surface in flight: turn along it and away from it.
            if f.state == SpaceFlightState::Flying && len(f.contact_normal) > 0.5 {
                let mut n = unit(f.contact_normal);
                if dot(n, f.heading) > 0.0 {
                    n = scale(n, -1.0);
                }
                bot.avoid_dir = escape_from(f.heading, n, f.up);
                bot.avoid_until = tick.saturating_add(PROVISIONAL_AVOID_TICKS);
                bot.avoid_hull = true;
            }
        }
        // An approach that keeps scraping the hull is given up.
        if matches!(bot.phase, PilotPhase::Return | PilotPhase::Raid) && f.state == SpaceFlightState::Flying && bot.scrapes >= PROVISIONAL_SCRAPE_LIMIT {
            bot.phase = PilotPhase::Fight;
            bot.since = tick;
            bot.target = PilotTarget::None;
            bump(&mut self.census.approaches_given_up, 1);
        }
        if !matches!(bot.phase, PilotPhase::Return | PilotPhase::Raid) {
            bot.scrapes = 0;
        }

        // Hit check twice a second: a sharp hull loss starts a barrel roll.
        if think {
            let share = if f.max_health > 0.0 { f.health / f.max_health } else { 1.0 };
            if bot.hull_check - share >= PROVISIONAL_HIT_SHARE && pv.tricks && f.energy >= pv.trick_cost
                && f.state == SpaceFlightState::Flying && !in_zone {
                input.trick_roll = if slot.is_multiple_of(2) { 1 } else { -1 };
                bump(&mut self.census.evasions, 1);
            }
            bot.heavy_fire = bot.hull_check - share >= PROVISIONAL_HEAVY_FIRE_SHARE && share < PROVISIONAL_HEAVY_FIRE_HULL;
            bot.hull_check = share;
        }

        let mut want: Option<[f64; 3]> = None;
        let mut throttle = 0.0;
        match bot.phase {
            PilotPhase::Hangar | PilotPhase::Parked => match f.state {
                SpaceFlightState::Landed => {
                    let parked_s = tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT;
                    // A transport waits for crew (a while, not forever).
                    let crew = pv.transport && world.space().is_some_and(|s| s.fighters[seat.fighter].gunners.iter().any(Option::is_some));
                    let crew_ready = !pv.transport || crew || parked_s > PROVISIONAL_CREW_WAIT_S;
                    let ready = if bot.phase == PilotPhase::Parked {
                        f.health >= f.max_health * PROVISIONAL_RELAUNCH_HULL || parked_s > PROVISIONAL_PARK_LIMIT_S
                    } else {
                        parked_s >= PROVISIONAL_BOARD_DELAY_S && crew_ready
                    };
                    if ready && !bot.action_down && pv.transport && bot.phase == PilotPhase::Hangar {
                        if crew {
                            bump(&mut self.census.crewed_departures, 1);
                        } else {
                            bump(&mut self.census.empty_departures, 1);
                        }
                    }
                    if ready && !bot.action_down {
                        input.takeoff_or_land = true;
                        bump(&mut self.census.takeoff_presses, 1);
                        if bot.phase == PilotPhase::Parked {
                            bump(&mut self.census.relaunches, 1);
                        }
                        bot.phase = PilotPhase::Hangar;
                        bot.since = tick;
                        bot.step = 0;
                        bot.hull_check = 1.0;
                        bot.exit = self.routes.nearest_exit(f.team, f.position).unwrap_or(usize::MAX);
                        if let Some(exit) = self.routes.exits.get(bot.exit) {
                            bot.track = exit.track(f.position, pv.reach + PROVISIONAL_TRACK_MARGIN);
                        }
                    }
                }
                SpaceFlightState::TakingOff | SpaceFlightState::Landing => {}
                SpaceFlightState::Flying => {
                    if bot.phase == PilotPhase::Parked {
                        bot.phase = PilotPhase::Hangar;
                        bot.step = 0;
                        bot.exit = self.routes.nearest_exit(f.team, f.position).unwrap_or(usize::MAX);
                        if let Some(exit) = self.routes.exits.get(bot.exit) {
                            bot.track = exit.track(f.position, pv.reach + PROVISIONAL_TRACK_MARGIN);
                        }
                    }
                    match self.routes.exits.get(bot.exit) {
                        Some(exit) => {
                            // Out along the craft's own track: pads face their opening,
                            // so the track ahead of a pad is clear of the other pads.
                            let depth = exit.depth(f.position);
                            let outside = pv.approach(&self.routes, bot.exit);
                            let waypoints = [
                                exit.track_point(bot.track, v8math::min(depth, routes::PROVISIONAL_EXIT_INSIDE)),
                                exit.track_point(bot.track, -2.0),
                                exit.track_point(bot.track, -outside),
                            ];
                            let passed = match bot.step {
                                0 => depth <= routes::PROVISIONAL_EXIT_INSIDE + 2.0 || dist(waypoints[0], f.position) < 6.0,
                                1 => depth < 0.0,
                                _ => depth < 10.0 - outside,
                            };
                            if passed {
                                bot.step += 1;
                            }
                            if bot.step >= 3 {
                                bump(&mut self.census.exits, 1);
                                bot.phase = PilotPhase::Launch;
                                // Of the launch paths from this opening, the one whose end
                                // the enemy's guns cover least (the nearest on a tie).
                                let (start, end) = exit.paths[..usize::from(exit.path_count)]
                                    .iter()
                                    .map(|&(a, b)| (len(self.away_from_guns(f.team, self.routes.nodes[b as usize - 1])), a, b))
                                    .fold(None, |best: Option<(f64, u32, u32)>, c| match best {
                                        Some(b) if b.0 <= c.0 => Some(b),
                                        _ => Some(c),
                                    })
                                    .map_or(exit.path, |(_, a, b)| (a, b));
                                bot.node = start;
                                bot.path_end = end;
                                bot.since = tick;
                                bot.check_tick = tick;
                            } else {
                                let dir = unit(sub(waypoints[bot.step as usize], f.position));
                                want = Some(dir);
                                throttle = hangar_throttle(&f, &pv, dir, in_zone, bot.contact_until.saturating_add(30) > tick);
                            }
                        }
                        None => {
                            bot.phase = PilotPhase::Fight;
                            bot.since = tick;
                        }
                    }
                    if bot.phase == PilotPhase::Hangar && tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_HANGAR_LIMIT_S {
                        // Nobody waits forever: set down where it is and get out.
                        bump(&mut self.census.hangar_aborts, 1);
                        bot.phase = PilotPhase::Abandon;
                        bot.since = tick;
                    }
                }
            },
            PilotPhase::Abandon => match f.state {
                SpaceFlightState::Landed => {
                    if tick.saturating_sub(bot.since).is_multiple_of(30) && world.space_leave_seat(seat).is_some() {
                        self.release(slot);
                        bot.receipt = None;
                        bot.phase = PilotPhase::Free;
                        bot.since = tick;
                        bot.cooldown_until = tick.saturating_add(20 * THINK_TICKS);
                        return;
                    }
                }
                SpaceFlightState::Flying => {
                    // Slow down over the deck, then ask to land.
                    throttle = 0.0;
                    if !bot.action_down && matches!(world.space_landing_permission(seat), SpaceLandingPermission::Allowed { .. }) {
                        input.takeoff_or_land = true;
                        bump(&mut self.census.land_presses, 1);
                    }
                    if tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_HANGAR_LIMIT_S {
                        bot.phase = PilotPhase::Fight;
                        bot.since = tick;
                    }
                }
                SpaceFlightState::TakingOff | SpaceFlightState::Landing => {}
            },
            PilotPhase::Launch => {
                let end = bot.path_end;
                let hit = f.health < bot.hull_check * f.max_health - 1.0;
                if bot.node >= end || hit {
                    bot.phase = if pv.transport { PilotPhase::Raid } else { PilotPhase::Fight };
                    bot.step = 0;
                    bot.since = tick;
                } else {
                    let node = self.routes.nodes[bot.node as usize];
                    let to = sub(node, f.position);
                    let stuck = tick.saturating_sub(bot.check_tick) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_NODE_LIMIT_S;
                    if len(to) < 70.0 || (dot(to, f.heading) < 0.0 && len(to) < 250.0) || stuck {
                        bot.node += 1;
                        bot.check_tick = tick;
                        if stuck {
                            bump(&mut self.census.nodes_skipped, 1);
                        }
                    }
                    want = Some(unit(to));
                }
            }
            PilotPhase::Fight => {
                let home = self.routes.nearest_exit(f.team, f.position);
                let away = self.away_from_guns(f.team, f.position);
                let under_guns = len(away) > 1e-6;
                let share = if f.max_health > 0.0 { f.health / f.max_health } else { 1.0 };
                let hurt = share < PROVISIONAL_RETURN_HULL || bot.heavy_fire || (under_guns && share < PROVISIONAL_UNDER_GUNS_HULL);
                // Only a seated pilot of a repairing class mends a craft (on the
                // deck as in flight): one that cannot gains nothing at home and
                // fights on, flying clear of the guns when hurt under them.
                let goes_home = repairs;
                // A pilot whose class repairs its craft in flight mends it
                // flying away from the enemy's ships, clear of the guns, and
                // fights again once mended; still badly hurt at its patrol
                // point, it goes in to land.
                if hurt && repairs && !pv.transport && !bot.mending {
                    bot.mending = true;
                    bump(&mut self.census.mends, 1);
                }
                if bot.mending && share >= PROVISIONAL_RELAUNCH_HULL {
                    bot.mending = false;
                }
                let mended_out = bot.mending && share < PROVISIONAL_RETURN_HULL && self.patrol_goal(&f).is_some_and(|g| dist(g, f.position) < PROVISIONAL_MEND_REACH);
                if hurt && under_guns && home.is_some() && !pv.transport && tick > bot.disengage_until {
                    // Inside enemy guns' reach a pilot does not turn for home yet:
                    // it breaks off and flies clear of their reach first.
                    bot.disengage_dir = unit(away);
                    bot.disengage_until = tick.saturating_add((PROVISIONAL_DISENGAGE_S / super::SPACE_FIXED_DT) as u32);
                    bot.target = PilotTarget::None;
                    bump(&mut self.census.disengages, 1);
                }
                if tick < bot.disengage_until {
                    // Clear of the guns first, at full throttle, steering away from
                    // them as they fall behind; done once out of their reach.
                    if under_guns {
                        bot.disengage_dir = unit(away);
                    } else {
                        bot.disengage_until = tick;
                    }
                    want = Some(bot.disengage_dir);
                    throttle = 1.0;
                } else if bot.mending && !mended_out {
                    // Away from the enemy's ships at full throttle, mending.
                    bot.target = PilotTarget::None;
                    want = Some(self.away_from_fleet(f.team, f.position).unwrap_or_else(|| self.patrol_dir(&f)));
                    throttle = 1.0;
                } else if (hurt || mended_out) && home.is_some() && !pv.transport && goes_home {
                    bot.mending = false;
                    // From outside the guns' reach (or after flying clear as long
                    // as it could), home.
                    bot.phase = PilotPhase::Return;
                    bot.step = 0;
                    bot.exit = home.unwrap_or(0);
                    bot.since = tick;
                    bot.target = PilotTarget::None;
                    bump(&mut self.census.returns, 1);
                } else {
                    if bot.no_shot(tick) {
                        bot.give_up(tick);
                        bump(&mut self.census.dogfights_given_up, 1);
                    }
                    // A pilot keeps to a run on a ship object until it breaks off.
                    let committed = matches!(bot.target, PilotTarget::Building { .. })
                        && (bot.run == Run::Break || (bot.run == Run::Approach && bot.step == 1));
                    if think && !committed {
                        bump(&mut self.census.target_searches, 1);
                        let me = Traffic { id: f.id, generation: 0, team: f.team, position: f.position, velocity: f.velocity, reach: 0.0 };
                        let ranked = choose(&self.traffic, &self.systems, &loadout, &me, f.forward);
                        // A craft behind a hull is not seen: one sight ray per pick.
                        let chosen = ranked.into_iter().find(|t| match *t {
                            PilotTarget::Craft { index, .. } => !bot.shuns(*t, tick) && craft_in_sight(world, seat.fighter, index),
                            PilotTarget::Building { .. } => !bot.shuns(*t, tick),
                            PilotTarget::None => true,
                        }).unwrap_or(PilotTarget::None);
                        if chosen != bot.target {
                            bot.target = chosen;
                            bot.run = Run::Approach;
                            bot.aiming = false;
                            bot.shot_tick = tick;
                        }
                    }
                    let (w, t, fire) = self.attack(world, bot, &f, &loadout, dt);
                    want = w;
                    throttle = t;
                    input.fire_primary = fire.0;
                    input.fire_secondary = fire.1;
                    input.lock_target = fire.2;
                    input.spread_scale = bot.spread_scale;
                }
            }
            PilotPhase::Return | PilotPhase::Raid => {
                if bot.step == 0 {
                    // Choose the way in once: home to a free pad of the team (else
                    // the side of the deck), or a transport to the enemy deck.
                    let choice = if bot.phase == PilotPhase::Return {
                        self.routes.nearest_exit(f.team, f.position).map(|e| (e, self.landing_spot(world, &f, e)))
                    } else {
                        self.routes
                            .exits
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| e.team != f.team && e.team > 0)
                            .min_by(|a, b| dist(a.1.outside, f.position).total_cmp(&dist(b.1.outside, f.position)))
                            .map(|(i, _)| (i, self.landing_spot(world, &f, i)))
                    };
                    // The spot search weighs at most the admitted craft slots.
                    let craft = world.space().map_or(0, |s| s.fighters.len());
                    if craft > MAX_CRAFT {
                        bump(&mut self.census.craft_truncated, count(craft - MAX_CRAFT));
                    }
                    match choice {
                        Some((exit, spot)) => {
                            bot.exit = exit;
                            bot.spot = spot;
                            bot.track = self.routes.exits[exit].track(spot, pv.reach + PROVISIONAL_TRACK_MARGIN);
                            bot.step = 1;
                        }
                        None => bot.phase = PilotPhase::Fight,
                    }
                } else if let Some(exit) = self.routes.exits.get(bot.exit).cloned() {
                    match f.state {
                        SpaceFlightState::Landed => {
                            if bot.phase == PilotPhase::Return {
                                bot.phase = PilotPhase::Parked;
                                bot.since = tick;
                            } else if bot.step < 9 {
                                bot.step = 9;
                                bump(&mut self.census.raids_landed, 1);
                            }
                        }
                        SpaceFlightState::Flying => {
                            // To the far line-up point out on the opening's track, then
                            // along the opening's axis by following the line (a point
                            // on the axis a little ahead), through the opening, then
                            // over the spot. Off the axis halfway in: go round again.
                            let outside = pv.approach(&self.routes, bot.exit);
                            let depth = exit.depth(f.position);
                            let above = add(bot.spot, [0.0, PROVISIONAL_SPOT_HEIGHT, 0.0]);
                            let ahead = v8math::max(60.0, 1.5 * pv.turn_radius);
                            let far_point = exit.track_point(bot.track, -outside * PROVISIONAL_FAR_APPROACH);
                            let rel = sub(f.position, exit.center);
                            let off_up = dot(rel, exit.up).abs();
                            let off_track = (dot(rel, exit.across) - bot.track).abs();
                            let on_axis = off_up < exit.half[1] * PROVISIONAL_AXIS_SHARE && off_track < PROVISIONAL_AXIS_TRACK;
                            let stage = bot.step.clamp(1, 3);
                            // The far point is reached within one and a half turns' room, or
                            // once past its plane near the axis (no circling a point).
                            let past_far = depth > -outside * PROVISIONAL_FAR_APPROACH && off_up < 60.0 && off_track < 60.0 && !bot.rounding;
                            match stage {
                                1 if dist(far_point, f.position) < v8math::max(PROVISIONAL_LINE_UP, 1.5 * pv.turn_radius) || past_far => {
                                    bot.step = 2;
                                    bot.rounding = false;
                                }
                                2 if (depth > -outside * 0.5 && depth < 0.0 && !on_axis) || (bot.contact_until.saturating_add(30) > tick && depth < 0.0) => {
                                    bot.step = 1;
                                    bot.rounding = true;
                                    bump(&mut self.census.approach_rounds, 1);
                                }
                                2 if depth > 2.0 => bot.step = 3,
                                _ => {}
                            }
                            let stage = bot.step.clamp(1, 3);
                            let wp = match stage {
                                1 => far_point,
                                2 => exit.track_point(bot.track, v8math::min(depth + ahead, PROVISIONAL_LAND_DEPTH + 20.0)),
                                _ => above,
                            };
                            let dir = unit(sub(wp, f.position));
                            want = Some(dir);
                            let flat = v8math::hypot(above[0] - f.position[0], above[2] - f.position[2]);
                            if stage == 3 && in_zone {
                                // Inside: at the slowest speed toward the spot, then the
                                // action key at landing speed; the landing brakes the craft.
                                throttle = if f.airspeed > pv.min_speed * 0.9 { -1.0 } else { hangar_throttle(&f, &pv, dir, in_zone, bot.contact_until.saturating_add(30) > tick) };
                                let behind = dot(sub(above, f.position), f.forward) < 0.0;
                                let lead = v8math::max(PROVISIONAL_SPOT_REACH, f.airspeed * PROVISIONAL_LAND_LEAD_S);
                                // At the spot's depth, land where it is rather than run on
                                // toward the back of the hangar.
                                let deep = depth > exit.depth(bot.spot) - PROVISIONAL_SPOT_REACH;
                                if (flat < lead || behind || deep) && !bot.action_down
                                    && matches!(world.space_landing_permission(seat), SpaceLandingPermission::Allowed { .. })
                                {
                                    input.takeoff_or_land = true;
                                    bump(&mut self.census.land_presses, 1);
                                }
                            } else if stage >= 2 || dist(wp, f.position) < 450.0 {
                                throttle = if f.airspeed > pv.min_speed * 0.9 { -1.0 } else { hangar_throttle(&f, &pv, dir, in_zone, bot.contact_until.saturating_add(30) > tick) };
                            } else if in_zone {
                                throttle = hangar_throttle(&f, &pv, dir, in_zone, bot.contact_until.saturating_add(30) > tick);
                            } else {
                                let far = dist(wp, f.position);
                                throttle = if far > 700.0 { 1.0 } else { 0.0 };
                            }
                            if tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT > PROVISIONAL_RETURN_LIMIT_S {
                                bot.phase = PilotPhase::Fight;
                                bot.since = tick;
                                bump(&mut self.census.return_timeouts, 1);
                            }
                        }
                        SpaceFlightState::Landing => {
                            // A landing held up on something below is called off and tried again.
                            if bot.contact_until.saturating_add(30) > tick && !bot.action_down {
                                input.takeoff_or_land = true;
                                bot.step = 1;
                            }
                        }
                        SpaceFlightState::TakingOff => {}
                    }
                } else {
                    bot.phase = PilotPhase::Fight;
                }
            }
            PilotPhase::Free | PilotPhase::Walk | PilotPhase::Gunner | PilotPhase::ToChair | PilotPhase::Chair => {}
        }

        // Inside a hangar a craft below its slowest speed hovers at no throttle;
        // a pilot on its way somewhere keeps it moving.
        if in_zone && f.airspeed < pv.min_speed * 0.9 && matches!(bot.phase, PilotPhase::Fight | PilotPhase::Launch) {
            throttle = v8math::max(throttle, 0.3);
        }

        // Bounded avoidance: one hull ray a tick, alternating between the
        // flight path (from the centre or a hull edge in turn) and the way the
        // pilot wants to turn, reaching as far as the craft needs to turn away;
        // then the traffic around the craft. A coming contact with a hull
        // overrides the plan, and a traffic turn does not overrule it.
        // Inside a hangar a craft below the avoidance speed creeps along its
        // pad's clear track; contact recovery looks after it there.
        if f.state == SpaceFlightState::Flying && (f.airspeed >= PROVISIONAL_AVOID_MIN_SPEED || !in_zone) {
            let turn_room = f.airspeed / v8math::max(0.05, pv.turn_rate);
            let reach_m = v8math::max(
                if f.airspeed < PROVISIONAL_AVOID_MIN_SPEED { 20.0 } else { steer::PROVISIONAL_LOOK_AHEAD_MIN },
                f.airspeed * steer::PROVISIONAL_LOOK_AHEAD_S + 1.6 * turn_room + pv.reach,
            );
            let odd = !phase(tick, slot).is_multiple_of(2);
            let along = match want {
                Some(w) if odd && dot(w, f.heading) < 0.955 => unit(w),
                _ => f.heading,
            };
            let probe = phase(tick, slot) / 2;
            let obstacle = super::hulls_meet(world, HullMeets::Craft, |world| {
                let space = world.space.as_deref()?;
                let fighter = &space.fighters[seat.fighter];
                steer::look_along(fighter, &space.profiles[fighter.profile_index], &space.bounds, &mut world.collision, space.collision_world, probe, along, reach_m)
            });
            // A dive's own target ahead is the run's to break off from, while
            // it lies farther than the craft needs to turn away.
            let dive_target = match bot.target {
                PilotTarget::Building { index, .. } if bot.phase == PilotPhase::Fight && bot.run == Run::Approach && bot.step == 1 => {
                    self.systems.iter().find(|s| s.index == index).map(|s| s.bodies)
                }
                _ => None,
            };
            let obstacle = obstacle.filter(|ob| {
                !dive_target.is_some_and(|b| ob.body >= 0 && b.contains(&ob.body) && ob.distance > v8math::max(PROVISIONAL_BREAK_RANGE, 1.5 * turn_room + pv.reach))
            });
            if let Some(ob) = obstacle {
                // Threading an opening: from the far line-up point in along the
                // axis when coming in, within 60 m otherwise.
                let lining = matches!(bot.phase, PilotPhase::Return | PilotPhase::Raid) && bot.step >= 2;
                let reach_in = if lining { pv.approach(&self.routes, bot.exit) * PROVISIONAL_FAR_APPROACH } else { 60.0 };
                let threading = matches!(bot.phase, PilotPhase::Hangar | PilotPhase::Return | PilotPhase::Raid | PilotPhase::Abandon)
                    && self.routes.exits.get(bot.exit).is_some_and(|e| e.near(f.position, reach_in));
                if !threading || ob.distance < v8math::max(25.0, 1.2 * turn_room) {
                    if tick >= bot.avoid_until {
                        bump(&mut self.census.hull_avoidances, 1);
                    }
                    bot.avoid_dir = escape_from(along, ob.normal, f.up);
                    bot.avoid_until = tick.saturating_add(PROVISIONAL_AVOID_TICKS);
                    bot.avoid_hull = true;
                    // The dive's own target this near ends the dive; anything
                    // else in the way (a frigate beside its line) is steered
                    // round and the dive goes on.
                    if dive_target.is_some_and(|b| ob.body >= 0 && b.contains(&ob.body)) {
                        bot.run = Run::Break;
                        bot.run_until = tick.saturating_add(PROVISIONAL_BREAK_TICKS);
                        bump(&mut self.census.object_breaks, 1);
                    }
                }
            }
            let hull_first = tick < bot.avoid_until && bot.avoid_hull;
            let me = Traffic { id: f.id, generation: 0, team: f.team, position: f.position, velocity: f.velocity, reach: pv.reach };
            if !hull_first && f.airspeed >= PROVISIONAL_AVOID_MIN_SPEED {
                if let Some(away) = traffic_escape(&me, &self.traffic, f.up) {
                    if tick >= bot.avoid_until {
                        bump(&mut self.census.craft_avoidances, 1);
                    }
                    bot.avoid_dir = unit(add(away, scale(f.heading, 0.3)));
                    bot.avoid_until = tick.saturating_add(20);
                    bot.avoid_hull = false;
                }
            }
            if f.oob_warning {
                want = Some(unit(sub(center, f.position)));
            }
            if tick < bot.avoid_until {
                want = Some(bot.avoid_dir);
                if !in_zone {
                    // Slow down to turn tighter.
                    throttle = if bot.avoid_hull { -1.0 } else { v8math::min(throttle, 0.0) };
                }
                input.fire_secondary = false;
            }
        }

        if let Some(dir) = want {
            let (sx, sy) = stick_toward(&f, dir);
            input.stick_x = sx;
            input.stick_y = sy;
        }
        input.throttle = clamp(throttle, -1.0, 1.0);
        input.boost = input.throttle > 0.9 && f.energy > pv.energy_bar * 0.5;
        if input.trick_roll != 0 {
            input.fire_primary = false;
        }
        bot.action_down = input.takeoff_or_land;
        world.space_control_seat(seat, input);
    }

    /// Where a craft coming in through `exit` sets down: inside the landing
    /// region past the opening, in the gap across it farthest from every pad's
    /// track and every craft on the deck, so it neither blocks a takeoff nor
    /// lands where a pad brings its next craft. Bounded: nine tracks, each
    /// searched inward in eight steps for the landing region, against the pads
    /// and at most the admitted craft slots (the caller counts any more).
    fn landing_spot(&self, world: &World, f: &CraftView, exit: usize) -> [f64; 3] {
        let e = &self.routes.exits[exit];
        let fallback = routes::PROVISIONAL_EXIT_INSIDE + 10.0;
        let Some(space) = world.space.as_deref() else { return e.track_point(0.0, fallback) };
        let mut best = (f64::NEG_INFINITY, e.track_point(0.0, fallback));
        for k in 0..9 {
            let track = (k as f64 - 4.0) / 4.0 * v8math::max(0.0, e.half[0] - 15.0);
            // The first depth on this track inside the landing region, a little past its edge.
            let depth = (0..8)
                .map(|i| routes::PROVISIONAL_EXIT_INSIDE + 10.0 * f64::from(i))
                .find(|d| space.bounds.landing_zone_at(e.track_point(track, *d)).is_some())
                .map_or(fallback, |d| d + 8.0);
            let spot = e.track_point(track, depth);
            let mut gap = f64::INFINITY;
            for pad in space.pads.iter().filter(|p| e.depth(p.position) > 0.0 && e.near(p.position, 0.0)) {
                gap = v8math::min(gap, (e.track(pad.position, 0.0) - track).abs());
            }
            for o in space.fighters.iter().take(MAX_CRAFT).filter(|o| o.alive && o.id != f.id && o.state == SpaceFlightState::Landed) {
                gap = v8math::min(gap, v8math::hypot(o.position[0] - spot[0], o.position[2] - spot[2]));
            }
            if gap > best.0 {
                best = (gap, spot);
            }
        }
        best.1
    }

    /// Steering, throttle and triggers (cannon, missile, lock) for an attack.
    /// The pilot steers along its filtered aim. At a craft it fires a weapon
    /// only when its nose lies on that weapon's lead point and the line from
    /// its muzzle to the target is clear of every other body; at a ship object
    /// only when the path its rounds take from the muzzle (along the nose,
    /// falling on the way) meets the object before anything else.
    fn attack(&mut self, world: &mut World, bot: &mut Bot, f: &CraftView, loadout: &Loadout, dt: f64) -> (Option<[f64; 3]>, f64, (bool, bool, bool)) {
        let tick = self.tick;
        let curves = skill_curves(world, f.team);
        // A new ship object to attack: find the side its runs start from, as
        // far out as the weapon that hurts it most reaches.
        if let PilotTarget::Building { index, .. } = bot.target {
            if bot.run_for != bot.target {
                bot.run_for = bot.target;
                bot.step = 0;
                bot.run = Run::Approach;
                let frame = world.space.as_deref().and_then(|space| object_frame(space, index));
                match (self.systems.iter().find(|s| s.index == index).copied(), frame) {
                    (Some(s), Some((live, _, center))) => {
                        let reach = {
                            let profile = &world.space.as_deref().unwrap().profiles[f.profile_index];
                            object_weapon(profile, loadout, s.shield > 0.0, s.lockable).map_or(PROVISIONAL_BREAK_RANGE, |i| object_reach(&profile.weapons[i]))
                        };
                        bot.run_reach = v8math::min(reach, PROVISIONAL_RUN_REACH_MAX);
                        // Laid out on the object where it is now (the scan's copy
                        // can be half a second old), kept in its own frame.
                        match self.run_start(world, &System { center, ..s }, f.position, bot.run_reach) {
                            Some((from, aim, surface)) => {
                                bot.run_from = from;
                                bot.run_aim = aim;
                                bot.run_local_from = to_local(&live, from);
                                bot.run_local_aim = to_local(&live, aim);
                                bot.run_surface = surface;
                            }
                            None => {
                                bump(&mut self.census.objects_unreachable, 1);
                                bot.give_up(tick);
                                return (Some(self.patrol_dir(f)), 0.0, (false, false, false));
                            }
                        }
                    }
                    _ => {
                        bot.target = PilotTarget::None;
                        return (Some(self.patrol_dir(f)), 0.0, (false, false, false));
                    }
                }
            }
        }
        let (aim, throttle, want_primary, want_secondary, lock, line) = {
            let space = world.space.as_deref().unwrap();
            let profile = &space.profiles[f.profile_index];
            let (point, velocity, building, radius, bodies, lock_kind, lock_index, lock_gen, shielded, lockable) = match bot.target {
                PilotTarget::Craft { index, generation } => {
                    if !craft_alive(space, index, generation) {
                        bot.target = PilotTarget::None;
                        return (Some(self.patrol_dir(f)), 0.0, (false, false, false));
                    }
                    let t = &space.fighters[index];
                    let body = t.kernel_body.map_or(-1, |b| b as i32);
                    (t.position, t.velocity, false, hull_reach(&space.profiles[t.profile_index]), [body, -1, -1], SpaceLockTargetKind::Fighter, index, u64::from(generation), false, true)
                }
                PilotTarget::Building { index, life } => {
                    // The object list is rescanned twice a second; the target's own
                    // life is read every tick.
                    let alive = super::building_core(world, index).is_some_and(|b| b.alive)
                        && crate::units::f3_props::owner_in(world, index).life.to_bits() == life.to_bits();
                    let (Some(s), Some((live, prev, center))) = (self.systems.iter().find(|s| s.index == index).filter(|_| alive), object_frame(space, index)) else {
                        bot.target = PilotTarget::None;
                        return (Some(self.patrol_dir(f)), 0.0, (false, false, false));
                    };
                    // A run aims at the point of the object's surface its side's
                    // line meets (the centre of an object with no body of its
                    // own), carried along as the object moves.
                    let local = if bot.run_surface { bot.run_local_aim } else { to_local(&live, center) };
                    let point = to_world(&live, local);
                    let velocity = scale(sub(point, to_world(&prev, local)), 1.0 / super::SPACE_FIXED_DT);
                    bot.run_from = to_world(&live, bot.run_local_from);
                    bot.run_aim = point;
                    let radius = if bot.run_surface { PROVISIONAL_AIM_RADIUS } else { s.radius };
                    (point, velocity, true, radius, s.bodies, SpaceLockTargetKind::Object, index, life.to_bits(), s.shield > 0.0, s.lockable)
                }
                PilotTarget::None => return (Some(self.patrol_dir(f)), 0.0, (false, false, false)),
            };
            let range = dist(point, f.position);
            let primary = loadout.primary.map(|i| &profile.weapons[i]).filter(|w| usable(w, building, range));
            let secondary = loadout.secondary.map(|i| &profile.weapons[i]).filter(|w| usable(w, building, range));
            // Lead with the weapon that does the work: at a ship object the one
            // that hurts it most in its state, else the gun, else the ordnance.
            let run_weapon = if building { object_weapon(profile, loadout, shielded, lockable).map(|i| &profile.weapons[i]) } else { None };
            let main = if building { run_weapon.filter(|w| usable(w, true, range)).or(primary.or(secondary)) } else { primary.or(secondary) };
            let mut aim_point = lead(f.position, point, velocity, main.map_or(0.0, |w| w.round_speed()));
            // Rounds that fall are aimed over the point by their fall on the way.
            let fall = main.map_or(0.0, |w| drop_over(w, dist(aim_point, f.position)));
            aim_point[1] += fall;
            let desired = unit(sub(aim_point, f.position));

            // Attack runs on ship objects: approach, break off, extend, come round.
            if building {
                // Break off with room to turn: farther out the faster the craft.
                let turn_room = f.airspeed / v8math::max(0.05, v8math::min(profile.turn_rate, profile.pitch_rate));
                let break_at = v8math::max(PROVISIONAL_BREAK_RANGE, 1.5 * turn_room + hull_reach(profile)) + radius * 0.5;
                match bot.run {
                    Run::Approach if range < break_at => {
                        bot.run = Run::Break;
                        bot.run_until = tick.saturating_add(PROVISIONAL_BREAK_TICKS);
                        bump(&mut self.census.object_breaks, 1);
                    }
                    Run::Break if tick >= bot.run_until => {
                        bot.run = Run::Approach;
                        bot.step = 0;
                        bot.aiming = false;
                    }
                    _ => {}
                }
                if bot.run == Run::Break {
                    bot.step = 0;
                    let away = unit(sub(f.position, point));
                    return (Some(unit(add(away, scale(f.up, 0.5)))), 1.0, (false, false, false));
                }
                // Out to the run's start first (the dive begins there, or on its
                // side as far out as it), then the dive along its clear line.
                let standoff = dist(bot.run_from, point);
                let side = unit(sub(bot.run_from, point));
                let on_side = dot(unit(sub(f.position, point)), side) > 0.9;
                if bot.step == 0 && (dist(bot.run_from, f.position) < 80.0 || (on_side && range > standoff * 0.8 && range < standoff * 1.3)) {
                    bot.step = 1;
                    bump(&mut self.census.object_dives, 1);
                }
                if bot.step == 0 {
                    let to = unit(sub(bot.run_from, f.position));
                    return (Some(to), if dist(bot.run_from, f.position) > 600.0 { 1.0 } else { 0.0 }, (false, false, false));
                }
            } else if range < PROVISIONAL_CRAFT_BREAK && dot(sub(velocity, f.velocity), sub(point, f.position)) < -20.0 * range {
                let side = unit(add(sub(f.position, point), scale(f.up, 0.6)));
                return (Some(side), 0.5, (false, false, false));
            }

            // The pilot steers along its aim, which follows the wanted line with
            // the battle's skill response (the released bot aim filter).
            if !bot.aiming {
                bot.aim.reset(&Vector3::new(f.forward[0], f.forward[1], f.forward[2]));
                bot.aiming = true;
            }
            let (filtered, scales) = bot.aim.update(&Vector3::new(desired[0], desired[1], desired[2]), dt, Some(&curves), main.map(|w| self.def_of(w)), false);
            let aim = unit([filtered.x, filtered.y, filtered.z]);
            bot.spread_scale = scales.map(|s| s.spread_scale);
            // Rounds leave along the nose: at a craft, fire when the nose is on
            // its lead point; at a ship object, when the nose is near enough
            // for the rounds' path to be tested against it.
            let off = v8math::acos(clamp(dot(f.forward, desired), -1.0, 1.0));
            let surface = building && bot.run_surface;
            let on_aim = if surface {
                off <= PROVISIONAL_OBJECT_CONE
            } else {
                let spread = primary.map_or(0.0, |w| w.auto_aim_size);
                off <= v8math::max(PROVISIONAL_FIRE_CONE, v8math::atan2(radius + spread, v8math::max(range, 1.0)))
            };

            let want_primary = primary.is_some() && on_aim;
            let mut want_secondary = false;
            let mut lock = false;
            if let (Some(w), Some(index)) = (secondary, loadout.secondary) {
                if w.lock_on_range > 0.0 {
                    let in_lock_range = range <= v8math::min(w.lock_on_range, w.target_lock_max_distance);
                    let held = &space.fighters[f.id].weapon_locks[index];
                    let ours = held.kind == lock_kind && held.target_index == lock_index && held.target_generation == lock_gen;
                    lock = in_lock_range && dot(desired, f.forward) > v8math::cos(w.lock_on_angle_deg.to_radians());
                    if ours && held.locked && !bot.salvo_down {
                        want_secondary = true;
                    }
                } else if building {
                    // Unguided ordnance on a ship object goes anywhere in its range.
                    want_secondary = on_aim;
                } else {
                    // Unguided ordnance at a craft is released near its optimum.
                    want_secondary = on_aim && range <= v8math::max(w.ai_optimal_range * 1.6, w.ai_min_range + 1.0);
                }
            }
            let throttle = if building {
                // A dive with ordnance (bombs, lock-on missiles) is flown at the
                // craft's least speed: the longer the run, the more it releases.
                if bot.step == 1 && run_weapon.is_some_and(|w| w.ordnance_kind != SpaceOrdnanceKind::Bolt && w.ordnance_kind != SpaceOrdnanceKind::Beam) {
                    -1.0
                } else if range > 1200.0 {
                    1.0
                } else {
                    0.0
                }
            } else if range > 600.0 {
                1.0
            } else if range < 180.0 {
                -1.0
            } else {
                0.0
            };
            let muzzle = add(f.position, scale(f.forward, hull_reach(profile) + 1.0));
            let own = space.fighters[f.id].kernel_body.map_or(-1, |b| b as i32);
            let guided = secondary.is_some_and(|w| w.lock_on_range > 0.0);
            // Each weapon's rounds leave along the nose and fall their own way.
            let chord = |w: Option<&super::flight::SpaceWeaponSpec>| unit(sub(scale(f.forward, range), [0.0, w.map_or(0.0, |w| drop_over(w, range)), 0.0]));
            let line = FireLine { from: muzzle, to: point, reach: radius, own, bodies, chord: [chord(primary), chord(secondary)], span: range + PROVISIONAL_AIM_RADIUS * 4.0, surface };
            (aim, throttle, want_primary, want_secondary, (lock, guided), line)
        };
        let (lock, guided) = lock;
        // The firing line is tested every tick a shot is wanted: at a craft the
        // line to it, at an object the path of each weapon that would fire (a
        // guided missile's is the line to its lock).
        let mut clear = [false; 2];
        if want_primary || want_secondary {
            let target = match bot.target {
                PilotTarget::Craft { index, .. } => index,
                _ => usize::MAX,
            };
            let mut blocked = friend_in_line(&self.traffic, f.team, f.id, target, line.from, line.to);
            if !blocked && line.surface {
                for (k, wanted) in [want_primary, want_secondary && !guided].into_iter().enumerate() {
                    if wanted {
                        match path_meets(world, line.from, line.chord[k], line.span, line.own, line.bodies) {
                            PathEnd::Target => clear[k] = true,
                            PathEnd::Other => blocked = true,
                            PathEnd::Nothing => {}
                        }
                    }
                }
                if want_secondary && guided {
                    clear[1] = line_clear(world, line.from, line.to, line.reach, line.own, line.bodies);
                    blocked |= !clear[1];
                }
            } else if !blocked {
                let ok = line_clear(world, line.from, line.to, line.reach, line.own, line.bodies);
                clear = [ok, ok];
                blocked = !ok;
            }
            if blocked {
                bump(&mut self.census.blocked_shots, 1);
            }
        }
        let fire_primary = want_primary && clear[0];
        let fire_secondary = want_secondary && clear[1];
        if fire_secondary && guided {
            bump(&mut self.census.lock_salvos, 1);
        }
        bot.salvo_down = fire_secondary;
        if fire_primary || fire_secondary || lock {
            bot.shot_tick = tick;
        }
        (Some(aim), throttle, (fire_primary, fire_secondary, lock))
    }

    /// Away from the enemy self-piloted guns whose reach covers `at`, each
    /// weighted by how deep inside its reach the point lies (zero when none).
    fn away_from_guns(&self, team: i32, at: [f64; 3]) -> [f64; 3] {
        let mut away = [0.0; 3];
        for g in self.systems.iter().filter(|g| g.team != team && g.gun_reach > 0.0) {
            let d = dist(g.center, at);
            if d < g.gun_reach && d > 1.0 {
                away = add(away, scale(unit(sub(at, g.center)), 1.0 - d / g.gun_reach));
            }
        }
        away
    }

    /// Away from the middle of the enemy's ship objects; `None` when there is
    /// none left.
    fn away_from_fleet(&self, team: i32, at: [f64; 3]) -> Option<[f64; 3]> {
        let (sum, n) = self
            .systems
            .iter()
            .filter(|s| s.team != team)
            .fold(([0.0; 3], 0.0), |(sum, n), s| (add(sum, s.center), n + 1.0));
        (n > 0.0).then(|| unit(sub(at, scale(sum, 1.0 / n))))
    }

    /// Where runs on ship object `s` start, what they aim at, and whether that
    /// is a point of the object's surface: from far out on the first of six
    /// sides (above, below and the four level ways, nearest the craft first)
    /// whose line toward the object's centre meets the object before anything
    /// else, the point that line meets; the run starts `reach` plus a lead-in
    /// back from it along the same clear line. An object with no body of its
    /// own is aimed at its centre when nothing lies in the way. None when every
    /// side is blocked. Six rays at most, once per target.
    fn run_start(&self, world: &mut World, s: &System, from: [f64; 3], reach: f64) -> Option<([f64; 3], [f64; 3], bool)> {
        let sides = [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        let back = reach + PROVISIONAL_RUN_LEAD_IN;
        let out = s.radius + back;
        let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); 6];
        for (i, side) in sides.iter().enumerate() {
            rank_into(&mut ranked, (dist(add(s.center, scale(*side, out)), from), i, 0));
        }
        let bodiless = s.bodies.iter().all(|b| *b < 0);
        for r in ranked.iter().filter(|r| r.1 != usize::MAX) {
            let p = add(s.center, scale(sides[r.1], out));
            let dir = unit(sub(s.center, p));
            let hit = {
                let space = world.space.as_deref()?;
                super::flight::cast_flyer_ray(p, dir, dist(p, s.center), -1, &space.bounds, &mut world.collision, space.collision_world)
            };
            match hit {
                None if bodiless => return Some((sub(s.center, scale(dir, back)), s.center, false)),
                Some((code, _, d)) if code >= 0 && s.bodies.contains(&code) => {
                    let aim = add(p, scale(dir, d));
                    return Some((sub(aim, scale(dir, back)), aim, true));
                }
                _ => {}
            }
        }
        None
    }

    /// Where an idle pilot flies: out in front of its own ship's nearest
    /// opening toward the enemy, kept out of the enemy guns' reach.
    fn patrol_dir(&self, f: &CraftView) -> [f64; 3] {
        match self.patrol_goal(f) {
            Some(g) if dist(g, f.position) > 150.0 => unit(sub(g, f.position)),
            _ => f.forward,
        }
    }

    /// The point an idle pilot patrols.
    fn patrol_goal(&self, f: &CraftView) -> Option<[f64; 3]> {
        let own = self.routes.nearest_exit(f.team, f.position).map(|i| &self.routes.exits[i]);
        let enemy = self
            .routes
            .exits
            .iter()
            .filter(|e| e.team != f.team && e.team > 0)
            .min_by(|a, b| dist(a.center, f.position).total_cmp(&dist(b.center, f.position)));
        match (own, enemy) {
            (Some(o), Some(e)) => {
                let mut g = add(o.outside, scale(unit(sub(e.outside, o.outside)), PROVISIONAL_PATROL_OUT));
                // Back toward home while the enemy's guns reach the point.
                for _ in 0..3 {
                    if len(self.away_from_guns(f.team, g)) < 1e-6 {
                        break;
                    }
                    g = add(o.outside, scale(sub(g, o.outside), 0.5));
                }
                Some(g)
            }
            (Some(o), None) => Some(add(o.outside, scale(o.outward, PROVISIONAL_PATROL_OUT))),
            _ => None,
        }
    }

    /// One tick of a bot in a gunner seat: aim the seat's weapon at the nearest
    /// enemy craft within its reach, leave a transport landed in the enemy hangar.
    fn gunner(&mut self, world: &mut World, slot: usize, bot: &mut Bot, dt: f64) {
        let Some(seat) = bot.receipt else { return };
        let think = phase(self.tick, slot).is_multiple_of(THINK_TICKS);
        let leave = {
            let Some(space) = world.space.as_deref() else { return };
            let f = &space.fighters[seat.fighter];
            f.state == SpaceFlightState::Landed && {
                let enemy_deck = f.landed_zone.and_then(|z| space.bounds.landing_zones.get(z)).is_some_and(|z| {
                    let team = zone_team(&self.routes, z);
                    team > 0 && team != f.team
                });
                let abandoned = f.pilot.is_none() && self.tick.saturating_sub(bot.since) as f64 * super::SPACE_FIXED_DT > 10.0;
                enemy_deck || abandoned
            }
        };
        if leave {
            if world.space_leave_seat(seat).is_some() {
                bump(&mut self.census.gunner_dismounts, 1);
            }
            self.release(slot);
            bot.receipt = None;
            bot.phase = PilotPhase::Free;
            bot.since = self.tick;
            bot.cooldown_until = self.tick.saturating_add(20 * THINK_TICKS);
            return;
        }
        let curves = skill_curves(world, bot_team(world, seat));
        let pick = {
            let Some(space) = world.space.as_deref() else { return };
            let f = &space.fighters[seat.fighter];
            let profile = &space.profiles[f.profile_index];
            let Some((index, w)) = profile.weapons.iter().enumerate().find(|(_, w)| w.seat == seat.seat && w.ai_targets_vehicle && !w.guided_missile) else {
                return;
            };
            let limits = profile.seat_aim_limits[usize::from(seat.seat).min(5)];
            (index, w.round_speed(), w.max_range, w.auto_aim_size, f.position, f.team, hull_reach(profile), f.kernel_body.map_or(-1, |b| b as i32), (w.ordnance_kind, w.ai_targets_building), limits, w.ai_bubble)
        };
        let (weapon, speed, max_range, spread, at, team, my_reach, own, class, limits, bubble) = pick;
        // Only what the seat's arc reaches can be aimed at.
        let bears = |world: &World, point: [f64; 3]| world.space().is_some_and(|s| s.fighters[seat.fighter].seat_can_bear(limits, point));
        let muzzle_toward = |p: [f64; 3]| add(at, scale(unit(sub(p, at)), my_reach + 1.0));
        if think {
            bump(&mut self.census.target_searches, 1);
            let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); PROVISIONAL_SIGHT_CHOICES];
            for (i, t) in self.traffic.iter().enumerate() {
                let d = dist(t.position, at);
                if t.team != team && t.team != 0 && d <= max_range && bears(world, t.position) {
                    rank_into(&mut ranked, (d, i, 0));
                }
            }
            bot.target = PilotTarget::None;
            for &(_, i, _) in ranked.iter().filter(|r| r.1 != usize::MAX) {
                let t = self.traffic[i];
                let body = world.space().and_then(|s| s.fighters.get(t.id)).and_then(|f| f.kernel_body).map_or(-1, |b| b as i32);
                let from = muzzle_toward(t.position);
                if !friend_in_line(&self.traffic, team, seat.fighter, t.id, from, t.position) && line_clear(world, from, t.position, t.reach, own, [body, -1, -1]) {
                    bot.target = PilotTarget::Craft { index: t.id, generation: t.generation };
                    // The firing line is tested afresh from the sight.
                    bot.line_tick = self.tick;
                    break;
                }
            }
            // With no craft to shoot, a gun that may be used on ship objects
            // takes the nearest enemy object in its reach it can see.
            if bot.target == PilotTarget::None && class.1 {
                let mut ranked = [(f64::INFINITY, usize::MAX, 0u8); PROVISIONAL_SIGHT_CHOICES];
                for (i, s) in self.systems.iter().enumerate() {
                    let d = v8math::max(0.0, dist(s.center, at) - s.radius);
                    if s.team != team && s.team > 0 && d <= max_range && !(s.hull && s.shield <= 0.0) && bears(world, s.center) {
                        rank_into(&mut ranked, (d, i, 0));
                    }
                }
                for &(_, i, _) in ranked.iter().filter(|r| r.1 != usize::MAX) {
                    let s = self.systems[i];
                    let from = muzzle_toward(s.center);
                    if !friend_in_line(&self.traffic, team, seat.fighter, usize::MAX, from, s.center) && line_clear(world, from, s.center, s.radius, own, s.bodies) {
                        bot.target = PilotTarget::Building { index: s.index, life: s.life };
                        // The firing line is tested afresh from the sight.
                        bot.line_tick = self.tick;
                        break;
                    }
                }
            }
        }
        let mut input = SpacePilotInput { weapon_index: Some(weapon as u8), ..Default::default() };
        let target = match bot.target {
            PilotTarget::Craft { index, generation } => world
                .space()
                .filter(|s| craft_alive(s, index, generation))
                .map(|s| (s.fighters[index].position, s.fighters[index].velocity, hull_reach(&s.profiles[s.fighters[index].profile_index]), [s.fighters[index].kernel_body.map_or(-1, |b| b as i32), -1, -1])),
            PilotTarget::Building { index, life } => {
                // The object's own life is read every tick; it is aimed at
                // where it is now, moving with it.
                let alive = super::building_core(world, index).is_some_and(|b| b.alive)
                    && crate::units::f3_props::owner_in(world, index).life.to_bits() == life.to_bits();
                match (self.systems.iter().find(|s| s.index == index).filter(|_| alive), world.space().and_then(|space| object_frame(space, index))) {
                    (Some(s), Some((live, prev, center))) => {
                        let before = to_world(&prev, to_local(&live, center));
                        Some((center, scale(sub(center, before), 1.0 / super::SPACE_FIXED_DT), s.radius, s.bodies))
                    }
                    _ => None,
                }
            }
            PilotTarget::None => None,
        };
        if let Some((position, velocity, reach, bodies)) = target {
            // Rounds leave the gun at their own speed; the craft's motion is not added.
            let from = muzzle_toward(position);
            let mut point = lead(from, position, velocity, speed);
            // A bot in a turret seat aims as the game's stationary bot controller
            // does: off a craft's lead by its sphere of miss.
            if let PilotTarget::Craft { index, .. } = bot.target {
                let dir = world.space().and_then(|s| s.seat_spheres.get(seat.fighter)?.get(usize::from(seat.seat).checked_sub(1)?).map(|m| m.dir));
                if let Some(dir) = dir {
                    point = miss_aim(world, dir, index, at, point, bubble);
                }
            }
            let desired = unit(sub(point, from));
            if !bot.aiming {
                bot.aim.reset(&Vector3::new(desired[0], desired[1], desired[2]));
                bot.aiming = true;
            }
            let (filtered, scales) = bot.aim.update(&Vector3::new(desired[0], desired[1], desired[2]), dt, Some(&curves), Some(&self.defs[def_index(class.0)]), false);
            let sight = unit([filtered.x, filtered.y, filtered.z]);
            let sight = world.space().map_or(sight, |s| s.fighters[seat.fighter].seat_aim_within(limits, sight));
            input.aim = Some(sight);
            input.spread_scale = scales.map(|s| s.spread_scale);
            let span = dist(position, at);
            let aligned = v8math::acos(clamp(dot(sight, desired), -1.0, 1.0)) <= v8math::max(PROVISIONAL_FIRE_CONE, v8math::atan2(reach + spread, v8math::max(span, 1.0)));
            // An object's surface is as far as its reach short of its centre.
            let near = if matches!(bot.target, PilotTarget::Building { .. }) { v8math::max(0.0, span - reach) } else { span };
            let mut fire = aligned && near <= max_range;
            if fire {
                if self.tick >= bot.line_tick {
                    let index = match bot.target {
                        PilotTarget::Craft { index, .. } => index,
                        _ => usize::MAX,
                    };
                    // From the muzzle along the sight the rounds leave by.
                    let muzzle = add(at, scale(sight, my_reach + 1.0));
                    bot.line_ok = !friend_in_line(&self.traffic, team, seat.fighter, index, muzzle, position) && line_clear(world, muzzle, position, reach, own, bodies);
                    bot.line_tick = self.tick.saturating_add(LINE_TICKS);
                }
                if !bot.line_ok {
                    bump(&mut self.census.blocked_shots, 1);
                    fire = false;
                }
            }
            input.fire_primary = fire;
        } else {
            bot.target = PilotTarget::None;
            bot.aiming = false;
        }
        world.space_control_seat(seat, input);
    }
}

fn bot_team(world: &World, seat: SpaceSeat) -> i32 {
    world.space().and_then(|s| s.fighters.get(seat.fighter)).map_or(0, |f| f.team)
}

/// The team whose pads lie in a landing zone, read from its nearest exit.
fn zone_team(routes: &Routes, zone: &super::bounds::OrientedRegion) -> i32 {
    routes
        .exits
        .iter()
        .min_by(|a, b| dist(a.center, zone.center).total_cmp(&dist(b.center, zone.center)))
        .map_or(zone.team, |e| e.team)
}

/// The nearest place beside a parked craft's hull to `from`, on its deck: a
/// soldier there is within the seat call's reach of the hull.
fn board_point(f: &SpaceFighter, p: &super::flight::SpaceFlightProfile, from: [f64; 3]) -> [f64; 3] {
    let apply = |m: &[f64; 16], v: [f64; 3]| {
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14],
        ]
    };
    let local = apply(&f.inverse, from);
    let pad = PROVISIONAL_BOARD_STANDOFF;
    let (lo, hi) = (p.contact_bounds_min, p.contact_bounds_max);
    let mut x = clamp(local[0], lo[0] - pad, hi[0] + pad);
    let mut z = clamp(local[2], lo[2] - pad, hi[2] + pad);
    if x > lo[0] - pad && x < hi[0] + pad && z > lo[2] - pad && z < hi[2] + pad {
        // Inside the footprint: step out to the nearest side.
        let gaps = [x - (lo[0] - pad), hi[0] + pad - x, z - (lo[2] - pad), hi[2] + pad - z];
        let side = (0..4).min_by(|a, b| gaps[*a].total_cmp(&gaps[*b])).unwrap_or(1);
        match side {
            0 => x = lo[0] - pad,
            1 => x = hi[0] + pad,
            2 => z = lo[2] - pad,
            _ => z = hi[2] + pad,
        }
    }
    apply(&f.matrix, [x, lo[1], z])
}

/// Whether a craft sees another: one flyer ray between them reaches the
/// other's hull before any other surface.
fn craft_in_sight(world: &mut World, from: usize, to: usize) -> bool {
    let Some(space) = world.space.as_deref() else { return false };
    let (Some(a), Some(b)) = (space.fighters.get(from), space.fighters.get(to)) else { return false };
    let span = dist(a.position, b.position);
    if span < 1.0 {
        return true;
    }
    let dir = scale(sub(b.position, a.position), 1.0 / span);
    let reach = hull_reach(&space.profiles[b.profile_index]);
    match super::flight::cast_flyer_ray(a.position, dir, span, a.kernel_body.map_or(-1, |k| k as i32), &space.bounds, &mut world.collision, space.collision_world) {
        None => true,
        Some((code, _, d)) => b.kernel_body.is_some_and(|k| k as i32 == code) || d >= span - reach,
    }
}

/// Whether the line from `from` toward `to`, stopped `reach` short of it,
/// meets nothing but the target's own bodies (`own` is ignored): one flyer ray.
fn line_clear(world: &mut World, from: [f64; 3], to: [f64; 3], reach: f64, own: i32, target: [i32; 3]) -> bool {
    let Some(space) = world.space.as_deref() else { return false };
    let span = dist(from, to) - reach;
    if span <= 1.0 {
        return true;
    }
    match super::flight::cast_flyer_ray(from, unit(sub(to, from)), span, own, &space.bounds, &mut world.collision, space.collision_world) {
        None => true,
        Some((code, _, _)) => code >= 0 && target.contains(&code),
    }
}

/// Where a firing line is tested: at a craft from the muzzle to the target
/// (stopped its reach short), at a point of an object's surface along each
/// weapon's path (primary, secondary).
#[derive(Clone, Copy, Debug)]
struct FireLine {
    from: [f64; 3],
    to: [f64; 3],
    reach: f64,
    own: i32,
    bodies: [i32; 3],
    chord: [[f64; 3]; 2],
    span: f64,
    surface: bool,
}

/// The frame of ship object `index` this tick and the tick before (the same
/// for one that does not move), and its centre.
fn object_frame(space: &super::SpaceBattlefield, index: usize) -> Option<([f64; 16], [f64; 16], [f64; 3])> {
    let slot = space.building_slot_by_index.get(index).copied().flatten()?;
    let att = space.buildings.get(slot)?;
    let prev = att.animated_index.and_then(|a| space.world_anims.objects.get(a)).map_or(att.matrix, |o| o.prev_matrix);
    Some((att.matrix, prev, att.center))
}

/// A point in frame `m` placed in the world.
fn to_world(m: &[f64; 16], v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12],
        m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13],
        m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14],
    ]
}

/// A world point in frame `m` (whose axes are at right angles).
fn to_local(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
    let d = [p[0] - m[12], p[1] - m[13], p[2] - m[14]];
    let along = |i: usize| {
        let axis = [m[4 * i], m[4 * i + 1], m[4 * i + 2]];
        let l2 = dot(axis, axis);
        if l2 > 1e-12 { dot(axis, d) / l2 } else { 0.0 }
    };
    [along(0), along(1), along(2)]
}

/// What a path meets first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathEnd {
    Target,
    Other,
    Nothing,
}

/// What the path from `from` along `dir` meets first within `span` (`own` is
/// ignored): one of the target's bodies, something else, or nothing. One ray.
fn path_meets(world: &mut World, from: [f64; 3], dir: [f64; 3], span: f64, own: i32, target: [i32; 3]) -> PathEnd {
    let Some(space) = world.space.as_deref() else { return PathEnd::Nothing };
    match super::flight::cast_flyer_ray(from, dir, span, own, &space.bounds, &mut world.collision, space.collision_world) {
        None => PathEnd::Nothing,
        Some((code, _, _)) if code >= 0 && target.contains(&code) => PathEnd::Target,
        Some(_) => PathEnd::Other,
    }
}

/// Whether a craft of `team` other than `me` (and `target`) lies within its
/// hull's reach and a margin of the segment from `from` to `to`: no round is
/// fired through a friend. One pass over the traffic.
fn friend_in_line(traffic: &[Traffic], team: i32, me: usize, target: usize, from: [f64; 3], to: [f64; 3]) -> bool {
    let seg = sub(to, from);
    let span2 = dot(seg, seg);
    traffic.iter().filter(|t| t.team == team && t.id != me && t.id != target).any(|t| {
        let s = if span2 > 1e-9 { clamp(dot(sub(t.position, from), seg) / span2, 0.0, 1.0) } else { 0.0 };
        dist(add(from, scale(seg, s)), t.position) < t.reach + 3.0 && s > 0.0
    })
}

fn def_index(kind: super::flight::SpaceOrdnanceKind) -> usize {
    use super::flight::SpaceOrdnanceKind as K;
    match kind {
        K::GuidedRocket => 2,
        K::Missile => 1,
        _ => 0,
    }
}

/// The world direction a turret's barrel points, from its yaw and pitch in
/// the mounting object's frame.
fn barrel(turret: &super::objects::SpaceTurretState, m: &[f64; 16]) -> [f64; 3] {
    let (cp, sp) = (v8math::cos(turret.pitch), v8math::sin(turret.pitch));
    let (cy, sy) = (v8math::cos(turret.yaw), v8math::sin(turret.yaw));
    let (lx, ly, lz) = (sy * cp, sp, cy * cp);
    unit([m[0] * lx + m[4] * ly + m[8] * lz, m[1] * lx + m[5] * ly + m[9] * lz, m[2] * lx + m[6] * ly + m[10] * lz])
}

/// A clear walk from a soldier to a seat: one ray at waist height, ignoring the
/// craft's own hull.
fn walk_line_clear(world: &mut World, from: [f64; 3], to: [f64; 3], craft_body: Option<u32>) -> bool {
    walk_blocker(world, from, to, craft_body.map_or(-1, |b| b as i32)).is_none()
}

/// The first body a waist-high walk line meets before its last metre, if any
/// (`ignore` aside); a walk too short to test is clear.
fn walk_blocker(world: &mut World, from: [f64; 3], to: [f64; 3], ignore: i32) -> Option<i32> {
    let space = world.space.as_deref()?;
    let a = add(from, [0.0, 1.0, 0.0]);
    let b = add(to, [0.0, 1.0, 0.0]);
    let span = dist(a, b);
    if span < 1.0 {
        return None;
    }
    let dir = scale(sub(b, a), 1.0 / span);
    let ray = crate::collision::Ray { ox: a[0], oy: a[1], oz: a[2], dx: dir[0], dy: dir[1], dz: dir[2] };
    let collision_world = space.collision_world as u32;
    match super::hulls_meet(world, HullMeets::Soldiers, |w| w.collision.ray_hit(collision_world, ray, span, ignore)) {
        Some((code, hit)) if code != crate::collision::MISS && hit.distance < span - 1.0 => Some(code),
        _ => None,
    }
}

/// The point a bot at a gun at `from` aims at: craft `target`'s lead point
/// `lead` moved by its sphere of miss along the gun's drifting direction
/// `dir`, scaled by the weapon's `bubble` (`space::aim`).
fn miss_aim(world: &mut World, dir: Vector3, target: usize, from: [f64; 3], lead: [f64; 3], bubble: super::aim::MissBubble) -> [f64; 3] {
    let Some(space) = world.space.as_deref_mut() else { return lead };
    let Some(f) = space.fighters.get(target) else { return lead };
    let (flying, human) = (super::aim::craft_flying(f), super::aim::human_aboard(f));
    let radius = space.craft_spheres.get(target).map_or(0.0, |s| s.radius);
    let ceiling = if flying { 0.0 } else { super::aim::offset_ceiling(&mut space.rng) };
    super::aim::miss_point(from, lead, dir, radius, human, flying, ceiling, bubble)
}

/// Whether the battle's mission keeps bots in turrets (`SetStayInTurrets`):
/// as the space mission host passed it, or as the bot controller has it.
fn stay_in_turrets(passed: bool, world: &World) -> bool {
    passed || world.ai.battle().is_some_and(|b| b.settings.mission_stay_in_turrets)
}

/// Where the bot's current target is: the living soldier of another team its
/// controller has as its target, if any.
fn current_target(world: &World, unit: Ref<Unit>, team: i32) -> Option<[f64; 3]> {
    let battle = world.ai.battle()?;
    let body = world.sim.store.unit.get(unit)?;
    let target = battle.state.controllers.read_reference(body.controller?)?.target?;
    let crate::ai::owned::entities::Entity::Unit(r) = battle.state.entities.get(target.obj)? else { return None };
    let t = world.sim.store.unit.get(r)?;
    let enemy = t.team != 0.0 && t.team != f64::from(team);
    (t.alive && !t.removed && enemy && t.generation.to_bits() == target.generation.to_bits()).then_some(t.bounds.center)
}

/// Whether the gun of ship object `gun` can turn to `point` within its yaw
/// and pitch arcs (the 2005 game asks no range).
fn gun_turns_to(space: &super::SpaceBattlefield, gun: usize, point: [f64; 3]) -> bool {
    let Some(att) = space.buildings.iter().find(|a| a.building_index == gun) else { return false };
    let Some(t) = att.turret.as_ref() else { return false };
    let m = &att.matrix;
    let d = [point[0] - m[12], point[1] - m[13], point[2] - m[14]];
    let rx = m[0] * d[0] + m[1] * d[1] + m[2] * d[2];
    let ry = m[4] * d[0] + m[5] * d[1] + m[6] * d[2];
    let rz = m[8] * d[0] + m[9] * d[1] + m[10] * d[2];
    let yaw = v8math::atan2(rx, rz);
    let pitch = v8math::atan2(ry, v8math::hypot(rx, rz));
    (t.yaw_min..=t.yaw_max).contains(&yaw) && (t.pitch_min..=t.pitch_max).contains(&pitch)
}

/// The released skill curves of `team` in this battle (medium when the bot
/// controller is absent).
fn skill_curves(world: &World, team: i32) -> crate::ai::combat::world::Curves {
    let c = world
        .ai
        .battle()
        .map(|b| b.settings.difficulty.borrow().curves(f64::from(team)))
        .unwrap_or_else(|| crate::ai::control::difficulty::skill_curves(14.0));
    crate::ai::combat::world::Curves { a: c.a, d: c.d, b: Some(c.b), s: Some(c.s) }
}

/// The parts of a craft a pilot reads in a tick, copied once.
#[derive(Clone, Copy, Debug)]
struct CraftView {
    id: usize,
    profile_index: usize,
    team: i32,
    state: SpaceFlightState,
    position: [f64; 3],
    velocity: [f64; 3],
    forward: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    heading: [f64; 3],
    airspeed: f64,
    health: f64,
    max_health: f64,
    energy: f64,
    oob_warning: bool,
    contacts: u32,
    contact_normal: [f64; 3],
}

trait View {
    fn clone_view(&self) -> CraftView;
}

impl View for SpaceFighter {
    fn clone_view(&self) -> CraftView {
        CraftView {
            id: self.id,
            profile_index: self.profile_index,
            team: self.team,
            state: self.state,
            position: self.position,
            velocity: self.velocity,
            forward: self.forward(),
            right: self.right(),
            up: self.up(),
            heading: heading(self),
            airspeed: self.airspeed,
            health: self.health,
            max_health: self.max_health,
            energy: self.energy,
            oob_warning: self.oob_warning,
            contacts: self.contacts,
            contact_normal: self.contact_normal,
        }
    }
}

/// Throttle inside a hangar: hover while turning to face `dir`, then cross at a
/// walking pace by short throttle taps (a craft below its slowest speed in a
/// landing zone slows to a hover at no throttle). After a hull contact the
/// pilot brakes and turns again. Outside the hangar: the slowest speed.
fn hangar_throttle(f: &CraftView, pv: &ProfileView, dir: [f64; 3], in_zone: bool, recovering: bool) -> f64 {
    if !in_zone {
        return if f.airspeed < pv.min_speed * 0.85 { 0.3 } else { -1.0 };
    }
    let off = v8math::acos(clamp(dot(dir, f.forward), -1.0, 1.0));
    if recovering || off > PROVISIONAL_HANGAR_TURN_CONE {
        return 0.0;
    }
    if f.airspeed < PROVISIONAL_HANGAR_SPEED { 0.3 } else { 0.0 }
}

/// Yaw and pitch stick that turn the nose toward `dir` (unit, world space),
/// in proportion to the angle off the nose, as a player holds the stick over.
fn stick_toward(f: &CraftView, dir: [f64; 3]) -> (f64, f64) {
    let lx = dot(dir, f.right);
    let ly = dot(dir, f.up);
    let lz = dot(dir, f.forward);
    let yaw = v8math::atan2(lx, lz);
    let pitch = v8math::atan2(ly, v8math::hypot(lx, lz));
    (
        clamp(-yaw * steer::PROVISIONAL_STICK_GAIN, -1.0, 1.0),
        clamp(pitch * steer::PROVISIONAL_STICK_GAIN, -1.0, 1.0),
    )
}

/// The parts of a craft class a pilot reads in a tick.
#[derive(Clone, Copy, Debug)]
struct ProfileView {
    min_speed: f64,
    turn_rate: f64,
    energy_bar: f64,
    tricks: bool,
    trick_cost: f64,
    transport: bool,
    reach: f64,
    turn_radius: f64,
}

impl ProfileView {
    /// How far outside an opening the craft lines up to fly through it.
    fn approach(&self, routes: &Routes, exit: usize) -> f64 {
        let _ = routes.exits.get(exit);
        v8math::max(routes::PROVISIONAL_EXIT_OUTSIDE, PROVISIONAL_APPROACH_TURNS * self.turn_radius)
    }
}

impl ProfileView {
    fn of(p: &super::flight::SpaceFlightProfile) -> Self {
        ProfileView {
            min_speed: p.min_speed,
            turn_rate: p.turn_rate,
            energy_bar: p.energy_bar,
            tricks: p.energy_trick_single > 0.0 && p.trick_roll_speed > 0.0,
            trick_cost: p.energy_trick_single,
            transport: p.is_command_flyer && p.spawn_point_count > 0,
            reach: hull_reach(p),
            turn_radius: p.min_speed / v8math::max(0.05, v8math::min(p.turn_rate, p.pitch_rate)),
        }
    }
}

/// Steps the pilots once, after the bot controller's phase, when a 2005
/// space battlefield is attached; nothing otherwise.
pub(crate) fn after_ai_in(world: &mut World, dt: f64) {
    let Some(space) = world.space.as_deref_mut() else { return };
    if space.pilots.bots.is_empty() || space.pilots.disabled {
        return;
    }
    let mut pilots = std::mem::take(&mut space.pilots);
    if let Some(probe) = pilots.probe {
        probe(true);
    }
    pilots.step(world, dt);
    if let Some(probe) = pilots.probe {
        probe(false);
    }
    if let Some(space) = world.space.as_deref_mut() {
        space.pilots = pilots;
    }
}

impl World {
    /// A bot's pilot state, for tools and the bot goals (`None`: not steered).
    pub fn space_pilot_view(&self, unit: Ref<Unit>) -> Option<PilotView> {
        let u = self.sim.store.unit.get(unit)?;
        let p = &self.space()?.pilots;
        let b = p.bots.get(unit.slot as usize)?;
        if !b.active || b.life.to_bits() != u.generation.to_bits() {
            return None;
        }
        p.view(unit.slot as usize)
    }

    /// Keeps a bot on foot (`true`) or lets waiting craft lure it again. The
    /// bot goals use this for bots their goal needs on foot; a bot already
    /// seated finishes its sortie.
    pub fn space_pilot_hold(&mut self, unit: Ref<Unit>, hold: bool) -> bool {
        // Only a living soldier the reference still names: a stale reference
        // to an earlier occupant of the slot is refused.
        let life = self.sim.store.unit.get(unit).filter(|u| u.alive && !u.removed).map(|u| u.generation);
        let Some(space) = self.space.as_deref_mut() else { return false };
        let Some(life) = life else {
            bump(&mut space.pilots.census.holds_refused, 1);
            return false;
        };
        let (ok, stop_nav) = space.pilots.hold(unit.slot as usize, life, hold);
        if stop_nav {
            self.nav_walk_stop(unit);
        }
        ok
    }

    /// Whether a chair receipt still names the chair, gun and occupant life.
    pub fn space_terminal_current(&self, seat: super::SpaceTerminalSeat) -> bool {
        self.current_terminal_seat(seat)
    }

    /// The pilots' running totals.
    pub fn space_pilot_census(&self) -> Option<PilotCensus> {
        Some(self.space()?.pilots.census)
    }

    /// Every living bot soldier and what the pilots do with it, for tools.
    pub fn space_pilot_rows(&self, out: &mut Vec<PilotRow>) {
        out.clear();
        let Some(space) = self.space() else { return };
        for (r, u) in self.sim.store.unit.iter() {
            if u.human || !u.alive || u.combat.dead || u.removed {
                continue;
            }
            let UnitBody::Soldier(s) = &u.soldier else { continue };
            let view = space.pilots.bots.get(r.slot as usize).filter(|b| b.active && b.life.to_bits() == u.generation.to_bits()).and_then(|_| space.pilots.view(r.slot as usize));
            out.push(PilotRow {
                slot: r.slot,
                life: u.generation,
                team: u.team as i32,
                position: s.position,
                pilot_class: space.repair_classes.contains(&u.class_name),
                view,
            });
        }
    }

    /// The soldier life a launch owner names, for tools crediting kills.
    pub fn space_owner_facts(&self, owner: super::SpaceOwner) -> Option<OwnerFacts> {
        let o = self.sim.store.owner_snapshot.get(owner)?;
        Some(OwnerFacts { slot: o.source_unit.slot, life: o.generation, team: o.team as i32, human: o.human })
    }

    /// Installs a tool's hook around each pilot phase (see `PilotBots::probe`).
    #[doc(hidden)]
    pub fn space_set_pilot_probe(&mut self, probe: Option<fn(bool)>) -> bool {
        match self.space.as_deref_mut() {
            Some(space) => {
                space.pilots.probe = probe;
                true
            }
            None => false,
        }
    }

    /// The mission's AI vehicle notify radius (`SetAIVehicleNotifyRadius`),
    /// as the mission host admitted it: how far a waiting craft lures a bot.
    /// Without it the battle's bot controller's radius is used. False when no
    /// space battlefield is attached or the radius is not a positive number.
    pub fn space_set_pilot_notify_radius(&mut self, radius: f64) -> bool {
        match self.space.as_deref_mut() {
            Some(space) if radius.is_finite() && radius > 0.0 => {
                space.pilots.notify_radius = Some(radius);
                true
            }
            _ => false,
        }
    }

    /// Whether the mission keeps bots in turrets (`SetStayInTurrets`), as its
    /// host passes it. False when no space battlefield is attached.
    pub fn space_set_pilot_stay_in_turrets(&mut self, stay: bool) -> bool {
        match self.space.as_deref_mut() {
            Some(space) => {
                space.pilots.stay_in_turrets = stay;
                true
            }
            None => false,
        }
    }

    /// How far a waiting craft lures a bot.
    pub fn space_pilot_notify_radius(&self) -> Option<f64> {
        self.space().map(|s| s.pilots.lure_radius(self))
    }

    /// Switches the pilots off (or on again) for this battle, for comparisons.
    #[doc(hidden)]
    pub fn space_set_pilots_disabled(&mut self, disabled: bool) -> bool {
        match self.space.as_deref_mut() {
            Some(space) => {
                space.pilots.disabled = disabled;
                true
            }
            None => false,
        }
    }

    /// The exits and launch paths the pilots read from the map.
    pub fn space_pilot_routes(&self) -> Option<&Routes> {
        Some(&self.space()?.pilots.routes)
    }

    /// The first collision body a flyer ray from `from` along `dir` meets
    /// within `max` metres, ignoring body `ignore` (negative: none), and its
    /// distance; hangar shield planes let it pass as they let craft pass.
    /// For tools that check sight and firing lines.
    pub fn space_ray(&mut self, from: [f64; 3], dir: [f64; 3], max: f64, ignore: i32) -> Option<(i32, f64)> {
        let space = self.space.as_deref()?;
        super::flight::cast_flyer_ray(from, unit(dir), max, ignore, &space.bounds, &mut self.collision, space.collision_world)
            .map(|(code, _, d)| (code, d))
    }

    /// The first body the waist-high line a soldier on foot walks from
    /// `from` to `to` meets (ignoring body `ignore`, negative: none), as the
    /// pilots test a walk to a seat; `None` when the walk is clear.
    pub fn space_walk_blocker(&mut self, from: [f64; 3], to: [f64; 3], ignore: i32) -> Option<i32> {
        walk_blocker(self, from, to, ignore)
    }
}
