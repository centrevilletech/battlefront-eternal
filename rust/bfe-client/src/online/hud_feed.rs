//! Battle HUD events raised by received network facts. presentFeedback
//! (src/runtime/network-combat-feedback.js) calls BattleHud.hit, damageFrom
//! and killMessage; presentEntityEvent's "shot"
//! (src/runtime/entity-presentation-events.js) calls BattleHud.noteShot.
//! HudModel owns every ring, arc, line, fade and radar flash; this module
//! only decides which command a fact raises and with which arguments.
use super::radar::Teams;
use super::receiver::Receiver;
use super::roster::Character;
use bfe_content::defs::Defs;
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_render::hud_model::{layout::HudCommand as C, HudModel};
use std::collections::HashMap;

/// Where the commands go: the battle's HudModel. Tests record them instead,
/// since the model's rings, arcs and lines are private to its own crate.
pub trait Sink {
    fn event(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64);
    /// An uppercase text handle (HudInput.text(value, true)).
    fn text(&mut self, value: &str) -> u32;
}
impl Sink for HudModel {
    fn event(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64) {
        HudModel::event(self, kind, a, b, c, d);
    }
    fn text(&mut self, value: &str) -> u32 {
        self.add_text(value, true)
    }
}

// decodeFeedback: damage (3) and death (4) records are 78 bytes, and their
// expiry is the record tick plus 18.
const FEEDBACK_BYTES: usize = 78;
const FEEDBACK_EXPIRY: f64 = 18.;
// decodePresentation: a presentation event may wait 120 ticks for its unit.
const PRESENTATION_EXPIRY: f64 = 120.;
// entity-unit-state.js: a unit's state summary is 100000 plus its id, and
// 65535 is a unit without a Character.
const UNIT_STATE_ID: f64 = 100000.;
const NO_SLOT: u16 = 65535;
// captureUnitState's header: id u32, flags u8, two f32 clocks, then the
// roster slot.
const UNIT_STATE_SLOT: usize = 13;
// The repair key's length byte, after the slot, two selected weapons (u16
// each) and the weapon count; its text follows.
const UNIT_STATE_REPAIR: usize = 20;
// entity-world-state.js: a building's state summary is 200000 plus its number.
const BUILDING_STATE_ID: f64 = 200000.;
// humanName() (src/bot-names.js) for the page's own player without a name.
const HUMAN_NAME: &str = "Player 1";
// killMessage's last name fallback (src/hud-units.js).
const SOLDIER: &str = "Soldier";
// A burst of fire from units this page has not built yet is bounded.
const WAITING: usize = 64;
// isCommandClass (src/mobile-posts.js): the vehicle bases that are command
// posts while they live, and how many of them one frame may lose.
const COMMAND_BASES: [&str; 2] = ["commandwalker", "commandhover"];
const LOST: usize = 4;
// A hull's record while it burns out (ENTITY_VEHICLE_STATES' "dying").
const DYING: f64 = 6.;

/// decodeFeedback's fields in wire order. Generations and ids stay integers
/// so identity checks compare exactly what the server wrote.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feedback {
    pub target: (u32, u32),
    pub attacker: (u32, u32),
    pub position: [f64; 3],
    pub team: i32,
    pub attacker_team: i32,
    pub source: Option<[f64; 3]>,
}

// The presentation fields a "shot" needs. Unknown fields are ignored, as
// the browser reads only the ones it names.
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct Shot {
    #[serde(default)]
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    #[serde(default)]
    mount_id: u32,
    #[serde(default)]
    mount_generation: u32,
    #[serde(default)]
    seat: usize,
    #[serde(default)]
    weapon_id: u32,
}

#[derive(serde::Deserialize)]
struct Kind<'a> {
    kind: &'a str,
}

// A "squad" event: the unit whose order the AI took, and which order.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Squad<'a> {
    #[serde(default)]
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    #[serde(default)]
    label: &'a str,
}

/// A repair lock as PlayerCombat keeps it (repairLockKey, src/combat.js):
/// "l3" is the building numbered 3, "v1" the vehicle in slot 1, "" none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RepairLock {
    Building(usize),
    Vehicle(usize),
}

fn repair_lock(key: &[u8]) -> Option<RepairLock> {
    let (kind, digits) = key.split_first()?;
    let index = std::str::from_utf8(digits).ok()?.parse().ok()?;
    match kind {
        b'l' => Some(RepairLock::Building(index)),
        b'v' => Some(RepairLock::Vehicle(index)),
        _ => None,
    }
}

// A building's persistent state (captureWorldPresentation,
// src/runtime/entity-world-state.js): its number, its look and its health
// (null for one that cannot be hurt), then eight facts the HUD does not read.
#[derive(serde::Deserialize)]
struct BuildingState<'a>(
    usize,
    &'a str,
    Option<f64>,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
    serde::de::IgnoredAny,
);

// One living command vehicle, as MobilePost keeps its hull and team.
struct CommandHull {
    key: (u32, u32),
    class: usize,
    team: u8,
    seen: bool,
}

// What applyUnitState (entity-unit-state.js) leaves on a unit that the kill
// feed reads: its Character slot and the hero flag, for one life.
#[derive(Clone, Copy)]
struct UnitState {
    generation: u32,
    slot: u16,
    hero: bool,
}

/// A unit as game.units.byId finds it: one life, what kind of thing it is,
/// its class or building, its team, and the radar's handle for it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Unit {
    pub id: u32,
    pub generation: u32,
    pub kind: u32,
    pub source: usize,
    pub team: f64,
    pub radar: usize,
}

/// The authored facts the events name: classDef.Label by class, and each
/// weapon's ShotDelay as the shooter's own arsenal, seat or turret holds it.
pub struct Tables {
    labels: Vec<String>,
    soldier_shots: HashMap<(usize, u32), f64>,
    seat_shots: HashMap<(usize, usize, u32), f64>,
    turret_shots: HashMap<usize, f64>,
    // A command vehicle class's post label (MobilePost.refresh: the
    // vehicle's label, else its class name), by registry class id.
    posts: Vec<Option<String>>,
}

impl Tables {
    pub fn new(defs: &Defs) -> Self {
        let delay = |record: &bfe_content::Value| record.get("ShotDelay").and_then(bfe_content::Value::as_f64).unwrap_or(0.);
        let mut labels = vec![String::new(); defs.registry.classes.len() + 1];
        let mut soldier_shots = HashMap::new();
        for class in &defs.classes {
            let Some(id) = class.registry_id else { continue };
            if let Some(label) = class.record.get("Label").and_then(bfe_content::Value::as_str) {
                labels[id as usize] = label.to_string();
            }
            // arsenal.weapons.find(w => w.name === name): the first of a name.
            for weapon in class.weapons.iter().filter_map(|&index| defs.weapons.get(index)) {
                if let Some(weapon_id) = weapon.registry_id {
                    soldier_shots.entry((id as usize, weapon_id as u32)).or_insert_with(|| delay(&weapon.record));
                }
            }
        }
        let mut seat_shots = HashMap::new();
        let mut posts = vec![None; defs.registry.classes.len() + 1];
        for vehicle in &defs.vehicles {
            let Some(id) = vehicle.registry_id else { continue };
            if vehicle.base.is_some_and(|base| COMMAND_BASES.contains(&defs.names.text(base))) {
                let label = vehicle.record.get("label").and_then(bfe_content::Value::as_str).filter(|label| !label.is_empty());
                posts[id as usize] = Some(label.unwrap_or(defs.names.text(vehicle.name)).to_string());
            }
            for (seat, def) in vehicle.seats.iter().enumerate() {
                for (weapon, weapon_id) in def.weapons.iter().zip(&def.weapon_ids) {
                    if let (Some(record), Some(weapon_id)) = (weapon.get("def"), weapon_id) {
                        seat_shots.entry((id as usize, seat, *weapon_id as u32)).or_insert_with(|| delay(record));
                    }
                }
            }
        }
        let mut turret_shots = HashMap::new();
        for prop in &defs.props {
            if let Some(record) = prop.record.get("weapon") {
                turret_shots.insert(prop.index, delay(record));
            }
        }
        Self { labels, soldier_shots, seat_shots, turret_shots, posts }
    }

    // presentEntityEvent's weapon: the mount's seat weapon of that name, a
    // turret's own weapon, else the shooter's own weapon of that name.
    fn shot_delay(&self, shot: &Shot, owner: Option<Unit>, mount: Option<Unit>) -> Option<f64> {
        let seat = mount
            .filter(|unit| unit.kind == k::VEHICLE)
            .and_then(|unit| self.seat_shots.get(&(unit.source, shot.seat, shot.weapon_id)));
        let turret = mount.filter(|unit| unit.kind == k::TURRET).and_then(|unit| self.turret_shots.get(&unit.source));
        let own = owner
            .filter(|unit| matches!(unit.kind, k::SOLDIER | k::DROIDEKA))
            .and_then(|unit| self.soldier_shots.get(&(unit.source, shot.weapon_id)));
        seat.or(turret).or(own).copied()
    }
}

pub struct Feed {
    tables: Tables,
    units: HashMap<u32, UnitState>,
    // Each distinct name is interned once. Keys are the source text, so a
    // repeated name is found without building an uppercase copy.
    texts: HashMap<String, u32>,
    // Shots whose unit this page does not have yet, with their expiry tick.
    waiting: Vec<(f64, Shot)>,
    // The living command vehicles: each one's life, class and the team it
    // had when last seen, in the battle (epoch) they belong to.
    hulls: Vec<CommandHull>,
    epoch: f64,
    // The own soldier's repair lock, for the life it was reported for.
    repair: Option<((u32, u32), RepairLock)>,
    // Each building's health by its number (NaN until its state arrives),
    // and the hull in each vehicle slot, from the mount records.
    buildings: Vec<f64>,
    slots: Vec<Option<(u32, u32)>>,
    // BFE_HUD_TRACE names each hit, arc, kill line and message on stderr as
    // it is raised, for checking a battle's HUD against its log.
    trace: bool,
}

impl Feed {
    pub fn new(tables: Tables) -> Self {
        Self {
            tables,
            // A battle admits a few hundred unit lives at once; the maps
            // grow only for a battle beyond that.
            units: HashMap::with_capacity(1024),
            texts: HashMap::with_capacity(256),
            waiting: Vec::with_capacity(WAITING),
            hulls: Vec::with_capacity(16),
            epoch: f64::NAN,
            repair: None,
            buildings: Vec::with_capacity(256),
            slots: Vec::with_capacity(256),
            trace: std::env::var_os("BFE_HUD_TRACE").is_some(),
        }
    }

    /// MobilePosts.events (src/mobile-posts.js) as a page showing a server's
    /// battle reads it: a command vehicle that was alive and no longer is
    /// has lost its post. Each loss is the team the hull had and its class;
    /// at most four a frame. A new battle forgets the old one's hulls.
    pub fn lost_posts(&mut self, receiver: &Receiver) -> ([(u8, usize); LOST], usize) {
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.hulls.clear();
            self.repair = None;
            self.buildings.clear();
            self.slots.clear();
        }
        for hull in &mut self.hulls {
            hull.seen = false;
        }
        for fact in receiver.active() {
            let class = fact[r::CLASS_ID] as usize;
            let command = fact[r::KIND] == k::VEHICLE as f64 && self.tables.posts.get(class).is_some_and(Option::is_some);
            if !command || fact[r::DEAD] != 0. || fact[r::HEALTH] <= 0. || fact[r::STATE] == DYING {
                continue;
            }
            let key = (fact[r::ID] as u32, fact[r::GENERATION] as u32);
            let team = fact[r::TEAM] as u8;
            if let Some(hull) = self.hulls.iter_mut().find(|hull| hull.key == key) {
                hull.seen = true;
                hull.team = team;
            } else if self.hulls.len() < self.hulls.capacity() {
                self.hulls.push(CommandHull { key, class, team, seen: true });
            }
        }
        let mut lost = ([(0, 0); LOST], 0);
        self.hulls.retain(|hull| {
            if !hull.seen && lost.1 < LOST {
                lost.0[lost.1] = (hull.team, hull.class);
                lost.1 += 1;
            }
            hull.seen
        });
        lost
    }

    /// The own soldier's repair lock, while it is still the life it was
    /// reported for (PlayerCombat.lockedTarget on a page showing a server's
    /// battle).
    pub fn repair(&self, receiver: &Receiver) -> Option<RepairLock> {
        let (life, lock) = self.repair?;
        Own::of(receiver).is(life).then_some(lock)
    }

    /// A building's health as its last state gave it; None before any.
    pub fn building_health(&self, building: usize) -> Option<f64> {
        self.buildings.get(building).copied().filter(|health| !health.is_nan())
    }

    /// The hull the mount records last named for a vehicle slot.
    pub fn vehicle(&self, slot: usize) -> Option<(u32, u32)> {
        self.slots.get(slot).copied().flatten()
    }

    /// The post label of a command vehicle class, empty for any other.
    pub fn post_label(&self, class: usize) -> &str {
        self.tables.posts.get(class).and_then(|label| label.as_deref()).unwrap_or("")
    }

    /// One command to the HUD, named on stderr when tracing.
    pub fn raise(&self, hud: &mut impl Sink, name: &str, kind: u32, values: [f64; 4]) {
        if self.trace {
            eprintln!("hud feed: {name} {values:?}");
        }
        hud.event(kind, values[0], values[1], values[2], values[3]);
    }

    /// Received publications in arrival order, as CombatPresentation.receive
    /// hands them on, then the shots still waiting for their unit
    /// (CombatPresentation.present).
    pub fn frame(&mut self, hud: &mut impl Sink, receiver: &Receiver, teams: &Teams) {
        let lookup = |id: u32| find(receiver, id);
        let own = Own::of(receiver);
        let mut at = 0;
        while at < self.waiting.len() {
            let (expiry, shot) = self.waiting[at];
            if expiry < receiver.clock.shown || self.shot(hud, &shot, &lookup) {
                self.waiting.remove(at);
            } else {
                at += 1;
            }
        }
        for field in receiver.publications.iter() {
            let bytes = &field.bytes[..field.length];
            let kind = field.row[r::KIND] as u32;
            let schema = field.row[r::SCHEMA];
            let tick = field.row[r::TICK];
            if kind == k::SUMMARY && schema == 6. {
                self.unit_state(&field.row, bytes, &own);
            } else if kind == k::SUMMARY && schema == 7. {
                // applyWorldPresentation: a building's health, by its number.
                if let Ok(state) = serde_json::from_slice::<BuildingState>(bytes) {
                    if field.row[r::ID] == BUILDING_STATE_ID + state.0 as f64 && state.0 < 65536 && !state.1.is_empty() {
                        if self.buildings.len() <= state.0 {
                            self.buildings.resize(state.0 + 1, f64::NAN);
                        }
                        self.buildings[state.0] = state.2.unwrap_or(f64::INFINITY);
                    }
                }
            } else if kind == k::MOUNT && field.row[r::SLOT] >= 0. {
                // A mount record names its hull's slot among the vehicles.
                let slot = field.row[r::SLOT] as usize;
                if slot < 256 {
                    if self.slots.len() <= slot {
                        self.slots.resize(slot + 1, None);
                    }
                    self.slots[slot] = Some((field.row[r::ID] as u32, field.row[r::GENERATION] as u32));
                }
            } else if kind == k::COMBAT && (schema == 3. || schema == 4.) {
                // A record older than its expiry is refused whole.
                if let Some(event) = decode(bytes, tick).filter(|_| tick + FEEDBACK_EXPIRY >= receiver.latest) {
                    self.present(hud, &own, &lookup, receiver, teams, schema == 4., &event);
                }
            } else if kind == k::COMBAT && schema == 8. && tick + PRESENTATION_EXPIRY >= receiver.latest {
                let event = serde_json::from_slice::<Kind>(bytes).map_or("", |event| event.kind);
                if event == "shot" {
                    if let Ok(shot) = serde_json::from_slice::<Shot>(bytes) {
                        if !self.shot(hud, &shot, &lookup) && self.waiting.len() < WAITING {
                            self.waiting.push((tick + PRESENTATION_EXPIRY, shot));
                        }
                    }
                } else if event == "squad" {
                    // presentEntityEvent: only the page's own unit's order
                    // shows, as the line BattleHud.squadCommand lights.
                    if let Ok(squad) = serde_json::from_slice::<Squad>(bytes) {
                        if let Some(order) = squad_order(squad.label).filter(|_| own.is((squad.owner_id, squad.owner_generation))) {
                            self.raise(hud, "squad", C::SQUAD, [order, 0., 0., 0.]);
                        }
                    }
                }
            }
        }
    }

    /// BattleHud.message(text, relation): a line in the feed, in capitals,
    /// in the colour of a HudModel relation code.
    pub fn message(&mut self, hud: &mut impl Sink, line: &str, relation: f64) {
        if self.trace {
            eprintln!("hud feed: message {line}");
        }
        let id = text(&mut self.texts, hud, line);
        hud.event(C::MESSAGE, id as f64, relation, 0., 0.);
    }

    // applyUnitState: the Character slot and hero flag of one unit life.
    fn unit_state(&mut self, row: &[f64], bytes: &[u8], own: &Own) {
        let (Some(id), Some(&flags), Some(slot)) =
            (bytes.get(0..4), bytes.get(4), bytes.get(UNIT_STATE_SLOT..UNIT_STATE_SLOT + 2))
        else {
            return;
        };
        let id = u32::from_le_bytes(id.try_into().unwrap());
        if row[r::ID] != UNIT_STATE_ID + id as f64 {
            return;
        }
        let slot = u16::from_le_bytes(slot.try_into().unwrap());
        let generation = row[r::GENERATION] as u32;
        self.units.insert(id, UnitState { generation, slot, hero: flags & 1 != 0 });
        // unit.combat.repairLock, for the page's own soldier: the key after
        // the two selected weapons and the weapon count, with its length.
        if own.is((id, generation)) {
            let key = bytes.get(UNIT_STATE_REPAIR).and_then(|&size| bytes.get(UNIT_STATE_REPAIR + 1..UNIT_STATE_REPAIR + 1 + size as usize));
            self.repair = key.and_then(repair_lock).map(|lock| ((id, generation), lock));
        }
    }

    // presentFeedback. The browser also marks a sniper hit early from its
    // own predicted round (claimPredictedHit); this client flies no rounds
    // of its own, so every hit is marked from the server's record.
    #[allow(clippy::too_many_arguments)]
    fn present(
        &mut self,
        hud: &mut impl Sink,
        own: &Own,
        lookup: &dyn Fn(u32) -> Option<Unit>,
        receiver: &Receiver,
        teams: &Teams,
        death: bool,
        event: &Feedback,
    ) {
        if !death {
            if own.is(event.attacker) {
                // game.units.relation(event.attackerTeam, event.team).
                let relation = teams.relation(viewer(event.attacker_team), index(event.team));
                self.raise(hud, "hit", C::HIT, [relation_code(relation), 0., 0., 0.]);
            }
            if let (true, Some(source)) = (own.is(event.target), event.source) {
                // own.soldier.aimYaw is the acknowledged aim: feedback is
                // applied before this frame's prediction runs.
                let angle = damage_direction(source[0] - event.position[0], source[2] - event.position[2], own.yaw);
                self.raise(hud, "damage", C::DAMAGE, [angle, 0., 0., 0.]);
            }
            return;
        }
        let Some(victim) = lookup(event.target.0).filter(|unit| unit.generation == event.target.1) else {
            return;
        };
        let killer = lookup(event.attacker.0).filter(|unit| unit.generation == event.attacker.1);
        self.kill(hud, own, receiver, teams, killer, victim, event.attacker_team);
    }

    // killMessage (src/hud-units.js): only a line a human took part in, or
    // one a hero scored. HudModel substitutes the names into the template.
    #[allow(clippy::too_many_arguments)]
    fn kill(
        &mut self,
        hud: &mut impl Sink,
        own: &Own,
        receiver: &Receiver,
        teams: &Teams,
        killer: Option<Unit>,
        victim: Unit,
        attacker_team: i32,
    ) {
        let shown = self.human(own, receiver, &victim)
            || killer.is_some_and(|unit| self.human(own, receiver, &unit) || self.state(&unit).is_some_and(|state| state.hero));
        if !shown {
            return;
        }
        let killer_text = killer.map_or(0, |unit| self.name(hud, own, receiver, &unit));
        let victim_text = self.name(hud, own, receiver, &victim);
        // radarRelation(own.team, event.attackerTeam): the line's colour.
        let color = teams.color_relation(own.team, index(attacker_team));
        self.raise(hud, "kill", C::KILL, [kill_kind(killer, victim), killer_text as f64, victim_text as f64, color]);
    }

    // noteShot(mount || unit, weapon.def): the radar flashes the shooter's
    // icon for the weapon's ShotDelay. False while a named unit or its
    // weapon is not on this page yet, as presentEntityEvent returns it.
    fn shot(&self, hud: &mut impl Sink, shot: &Shot, lookup: &dyn Fn(u32) -> Option<Unit>) -> bool {
        let owner = lookup(shot.owner_id).filter(|unit| shot.owner_id != 0 && unit.generation == shot.owner_generation);
        if shot.owner_id != 0 && owner.is_none() {
            return false;
        }
        let mount = lookup(shot.mount_id).filter(|unit| shot.mount_id != 0 && unit.generation == shot.mount_generation);
        if shot.mount_id != 0 && mount.is_none() {
            return false;
        }
        let Some(delay) = self.tables.shot_delay(shot, owner, mount) else {
            return false;
        };
        // A shot with neither a mount nor an owner flashes nothing.
        if let Some(unit) = mount.or(owner).filter(|unit| unit.radar != 0) {
            hud.event(C::SHOT, unit.radar as f64, unit.generation as f64, delay, 0.);
        }
        true
    }

    fn state(&self, unit: &Unit) -> Option<&UnitState> {
        self.units.get(&unit.id).filter(|state| state.generation == unit.generation)
    }

    // unit.character: the own player's from his spawn summary, any other
    // unit's from its latest state, when the roster holds that slot. The
    // browser's pooled Unit can also keep a Character from an earlier life
    // whose state named no slot; that leftover is not reproduced.
    fn character<'a>(&self, own: &Own, receiver: &'a Receiver, unit: &Unit) -> Option<&'a Character> {
        let local = if own.is((unit.id, unit.generation)) { receiver.roster.local } else { None };
        let slot = local.or_else(|| self.state(unit).filter(|state| state.slot != NO_SLOT).map(|state| state.slot as usize))?;
        receiver.roster.rows().nth(slot).map(|(_, character)| character)
    }

    // victim.human || victim.character?.human: the page's own unit is its
    // one human Unit; every other human is known by his Character.
    fn human(&self, own: &Own, receiver: &Receiver, unit: &Unit) -> bool {
        own.is((unit.id, unit.generation)) || self.character(own, receiver, unit).is_some_and(|c| c.human)
    }

    // killMessage's name(): the Character's name, the human's default name,
    // then classDef.Label, then "Soldier". Network units carry no unit.name.
    fn name(&mut self, hud: &mut impl Sink, own: &Own, receiver: &Receiver, unit: &Unit) -> u32 {
        if let Some(character) = self.character(own, receiver, unit) {
            let name = character.name();
            if !name.is_empty() {
                return text(&mut self.texts, hud, &name);
            }
        }
        if own.is((unit.id, unit.generation)) {
            return text(&mut self.texts, hud, HUMAN_NAME);
        }
        let label = if matches!(unit.kind, k::SOLDIER | k::DROIDEKA) {
            self.tables.labels.get(unit.source).map_or("", String::as_str)
        } else {
            ""
        };
        text(&mut self.texts, hud, if label.is_empty() { SOLDIER } else { label })
    }
}

// game.playerUnit as feedback compares against it: one qualified life, its
// team and the soldier's acknowledged aim.
struct Own {
    id: u32,
    generation: u32,
    team: u8,
    yaw: f64,
}

impl Own {
    fn of(receiver: &Receiver) -> Self {
        // No correction yet means no unit of his own on this page.
        let known = receiver.corrections != 0 && receiver.own.id > 0.;
        Self {
            id: if known { receiver.own.id as u32 } else { 0 },
            generation: receiver.own.generation as u32,
            team: receiver.own.team,
            yaw: receiver.own.yaw,
        }
    }

    // The pair (0, 0) is "no attacker" on the wire and names no unit.
    fn is(&self, (id, generation): (u32, u32)) -> bool {
        self.id != 0 && self.id == id && self.generation == generation
    }
}

// Names and message lines are uppercase text handles. An interned handle is
// reused, so a battle's texts grow only with new names and lines.
fn text(texts: &mut HashMap<String, u32>, hud: &mut impl Sink, value: &str) -> u32 {
    if let Some(&id) = texts.get(value) {
        return id;
    }
    let id = hud.text(value);
    texts.insert(value.to_owned(), id);
    id
}

// game.units.byId(id): a soldier, droideka, vehicle or turret this page has
// built. The own player's unit is found even on the frames its soldier
// record is not indexed; it then has no radar handle, and needs none.
fn find(receiver: &Receiver, id: u32) -> Option<Unit> {
    if id == 0 {
        return None;
    }
    let unit = |row: &[f64], radar: usize| {
        let kind = row[r::KIND] as u32;
        Unit {
            id,
            generation: row[r::GENERATION] as u32,
            kind,
            source: row[if kind == k::TURRET { r::BUILDING } else { r::CLASS_ID }] as usize,
            team: row[r::TEAM],
            radar,
        }
    };
    let known = |row: &[f64]| row[r::ID] == id as f64 && matches!(row[r::KIND] as u32, k::SOLDIER | k::DROIDEKA | k::VEHICLE | k::TURRET);
    receiver
        .active_slots()
        .find(|(_, row)| known(*row))
        // Radar unit ids are one-based receiver slots (Hud::frame).
        .map(|(slot, row)| unit(row, slot + 1))
        .or_else(|| receiver.active().find(|row| known(*row)).map(|row| unit(row, 0)))
}

/// InterfaceSurface.text(key, ...args) (src/interface/surface.js): the
/// localized string with each %s filled in order, or the arguments joined
/// by spaces when there is no such string. The browser's replace also reads
/// `$` patterns in an argument; team and post names have none.
pub fn fill(template: Option<&str>, args: &[&str]) -> String {
    let Some(template) = template.filter(|text| !text.is_empty()) else {
        return args.join(" ");
    };
    let mut text = template.to_string();
    for arg in args {
        text = text.replacen("%s", arg, 1);
    }
    text
}

// BattleHud.squadCommand's names for the four orders, as their line index.
fn squad_order(label: &str) -> Option<f64> {
    match label {
        "moveout" | "move" => Some(0.),
        "hold" => Some(1.),
        "follow" => Some(2.),
        "atease" | "dismiss" => Some(3.),
        _ => None,
    }
}

/// What a command post's summary announces (applyWorldSummary,
/// src/runtime/network-entity-summary.js): the string's key and the team it
/// names. `before` is the team and standing the last summary gave, None for
/// the first. A destructible post destroyed is lost by its owner; a post
/// that changed hands is captured by its new team, or lost by its old one
/// when it went neutral.
pub fn post_change(before: Option<(u8, bool)>, team: u8, active: bool, building: bool) -> Option<(&'static str, u8)> {
    match before {
        Some((_, true)) if !active && building => Some(("game.infomsg.lost", team)),
        Some((previous, _)) if previous != team => {
            Some(if team != 0 { ("game.infomsg.captured", team) } else { ("game.infomsg.lost", previous) })
        }
        _ => None,
    }
}

/// battle.human's kills, team kills, deaths and captures for BattleHud.stats:
/// the local Character his spawn summary names, else the roster's first
/// human, else zeros (a Character the roster has not sent yet is all zeros).
pub fn score(roster: &super::roster::Roster) -> [f64; 4] {
    let character = match roster.local {
        Some(slot) => roster.rows().nth(slot),
        None => roster.rows().find(|(_, character)| character.human),
    };
    character.map_or([0.; 4], |(_, c)| [c.kills as f64, c.team_kills as f64, c.deaths as f64, c.captures as f64])
}

/// decodeFeedback: a malformed record changes nothing.
pub fn decode(bytes: &[u8], tick: f64) -> Option<Feedback> {
    if bytes.len() != FEEDBACK_BYTES {
        return None;
    }
    let u = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let i = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let f = |at: usize| f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let vector = |at: usize| [f(at), f(at + 8), f(at + 16)];
    let position = vector(20);
    let flags = u16::from_le_bytes([bytes[52], bytes[53]]);
    let source = vector(54);
    if flags > 1 || !position.iter().chain(&source).all(|n| n.is_finite()) || u(0) as f64 != tick + FEEDBACK_EXPIRY {
        return None;
    }
    Some(Feedback {
        target: (u(4), u(8)),
        attacker: (u(12), u(16)),
        position,
        team: i(44),
        attacker_team: i(48),
        source: (flags == 1).then_some(source),
    })
}

/// damageDirection (src/combat-input.js): toward the source in the HUD's
/// view convention, positive to the soldier's right.
pub fn damage_direction(x: f64, z: f64, yaw: f64) -> f64 {
    (-x * yaw.cos() + z * yaw.sin()).atan2(x * yaw.sin() + z * yaw.cos())
}

/// relationName for a numeric team relation, as a HudModel relation code:
/// 1 friendly, 2 enemy, else 0.
pub fn relation_code(relation: f64) -> f64 {
    if relation > 0. {
        1.
    } else if relation < 0. {
        2.
    } else {
        0.
    }
}

// killMessage's type: died, suicide, teamkill or killed. Teams are the
// units', as killer.team === victim.team compares them.
fn kill_kind(killer: Option<Unit>, victim: Unit) -> f64 {
    match killer {
        None => 0.,
        Some(unit) if unit.id == victim.id && unit.generation == victim.generation => 1.,
        Some(unit) if unit.team == victim.team => 2.,
        Some(_) => 3.,
    }
}

// Wire teams are signed; -1 (no team) has no relation row.
fn viewer(team: i32) -> u8 {
    u8::try_from(team).unwrap_or(u8::MAX)
}
fn index(team: i32) -> usize {
    usize::try_from(team).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests;
