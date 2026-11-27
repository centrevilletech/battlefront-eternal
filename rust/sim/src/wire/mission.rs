//! A scripted 2005 mission's state on the wire: what the script host holds,
//! written by the authority and read by every client's mission view.
//!
//! - `MATCH_STATE` (kind 24) world fields: the match, its HUD timer, personal
//!   points by roster slot, the mission's objectives, when each waiting player
//!   may deploy, and each roster slot's class. They are the state itself, so a
//!   client that joins or resynchronizes reads the whole of it.
//! - `SYSTEM` (kind 18) world fields: one per critical system the mission
//!   named, with its life, shield and place on the HUD.
//! - `NOTICE` (kind 25) reliable records: messages, hints, voice cues and
//!   kills, each to the recipients the script meant.
//! - `REGISTRY` (kind 28), beside the entity registry: each team's name, icons
//!   and the classes a player may choose, by their ids in the matched registry.
//!
//! Every reader takes exactly one record, whole: an unknown flag, a value out
//! of range, a text that is too long or a byte left over refuses it, before
//! anything is stored.
use super::revisions::{field_schema, MATCH_STATE, SYSTEM};
use crate::battle::script::state::{CompactOutcome, ScriptNotification};
use crate::world::owner::World;
use serde_json::{json, Map, Value};

/// The revision of every record here.
pub const REVISION: u16 = 1;
/// The match state fields' schema, and the systems'.
pub const STATE_SCHEMA: u16 = field_schema(MATCH_STATE);
pub const SYSTEM_SCHEMA: u16 = field_schema(SYSTEM);

/// The longest text a record carries (a localization key, a sound or a name).
pub const MAX_TEXT: usize = 96;
/// Personal points: roster slots a field covers, and the fields of a battle.
pub const PERSONAL_SLOTS: usize = 32;
pub const PERSONAL_FIELDS: usize = 16;
/// Deployments: one field for the roster slots each of those covers.
pub const DEPLOYMENT_FIELDS: usize = PERSONAL_FIELDS;
/// Roster classes: one field for the roster slots each of those covers.
pub const CLASSES_FIELDS: usize = PERSONAL_FIELDS;
/// Players leaving the battlefield: one field for the roster slots each of those covers.
pub const LEAVING_FIELDS: usize = PERSONAL_FIELDS;
/// Objectives: how many a field covers, and the fields of a battle.
pub const OBJECTIVE_ROWS: usize = 8;
pub const OBJECTIVE_FIELDS: usize = 8;
/// The most critical systems a mission names.
pub const SYSTEMS: usize = crate::battle::script::state::MAX_CRITICAL_SYSTEMS;
/// How long a notice is worth showing, in ticks.
pub const NOTICE_TICKS: u32 = 300;

/// The largest payload of each field, its revision included.
pub const MATCH_BYTES: usize = 2 + 1 + 2 + 1 + 1 + 4 + 8 + 8;
pub const TIMER_BYTES: usize = 2 + 1 + 1 + 4 + 4 + 1 + MAX_TEXT;
pub const PERSONAL_BYTES: usize = 2 + 1 + 1 + 4 + 4 * PERSONAL_SLOTS;
pub const OBJECTIVES_BYTES: usize = 2 + 1 + 1 + 1 + OBJECTIVE_ROWS * (1 + 1 + 1 + MAX_TEXT);
pub const DEPLOYMENTS_BYTES: usize = 2 + 1 + 1 + 4 + 4 + 4 * PERSONAL_SLOTS;
pub const CLASSES_BYTES: usize = 2 + 1 + 1 + 4 + 2 * PERSONAL_SLOTS;
pub const CLASS_ROOM_BYTES: usize = 2 + 1 + 2 + 4;
pub const LEAVING_BYTES: usize = 2 + 1 + 1 + 4 + 4 * PERSONAL_SLOTS;
pub const SYSTEM_BYTES: usize = 2 + 1 + 1 + 1 + 1 + 2 + 4 + 16 + 8 + 1 + MAX_TEXT;

const TAG_MATCH: u8 = 0;
const TAG_TIMER: u8 = 1;
const TAG_PERSONAL: u8 = 2;
const TAG_OBJECTIVES: u8 = 3;
const TAG_DEPLOYMENTS: u8 = 4;
const TAG_CLASSES: u8 = 5;
const TAG_CLASS_ROOM: u8 = 6;
const TAG_LEAVING: u8 = 7;
const INVALID: &str = "invalid Battlefront II mission record";

/// The audience of an objective or a notice that names no team, and the one
/// of a team no player can be on.
pub const EVERYONE: u8 = 0;
pub const NOBODY: u8 = 255;

fn audience(team: i32) -> u8 {
    match team {
        ..=0 => EVERYONE,
        1..=7 => team as u8,
        _ => NOBODY,
    }
}

/// The match: its score, result and what the script shows of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Match {
    pub over: bool,
    pub game_over: bool,
    pub finished: bool,
    pub faulted: bool,
    pub points_shown: [bool; 2],
    pub minimap_hidden: bool,
    pub posts_hidden: bool,
    pub outcome: Option<CompactOutcome>,
    pub target: Option<u32>,
    pub points: [u32; 2],
    /// Tickets left; none for an unlimited team.
    pub reinforcements: [Option<u32>; 2],
}

/// The timer the script shows: when it is due while it runs, else the ticks
/// it has left. Not shown, it says nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timer<'a> {
    pub shown: bool,
    pub due: Option<u32>,
    pub left: u32,
    pub name: &'a str,
}

/// Personal points of the roster slots from `32 * field`; none for a slot
/// that has earned or lost none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Personal {
    pub field: u8,
    pub rows: [Option<i32>; PERSONAL_SLOTS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Objective<'a> {
    pub active: bool,
    pub completed: bool,
    /// Whom it is for: [`EVERYONE`], a team or [`NOBODY`].
    pub team: u8,
    pub key: &'a str,
}

/// The objectives from `8 * field` on, in the order the script added them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Objectives<'a> {
    pub field: u8,
    pub rows: [Option<Objective<'a>>; OBJECTIVE_ROWS],
}

/// A player who waits for a body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deployment {
    /// The tick from which the mission may give him one, past his own wait
    /// after a death, the spawn wave after it and his team's next slot in it;
    /// 0 while it may already.
    pub due: u32,
    /// Whether he has chosen what to deploy as.
    pub chosen: bool,
}

/// The living players of the roster slots from `32 * field` who are outside
/// the battlefield: the authority tick each one's countdown runs out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Leaving {
    pub field: u8,
    pub rows: [Option<u32>; PERSONAL_SLOTS],
}

/// The players of the roster slots from `32 * field` who wait for a body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deployments {
    pub field: u8,
    pub rows: [Option<Deployment>; PERSONAL_SLOTS],
}

/// The registry class ids of the roster slots from `32 * field`; none for a
/// slot without a class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Classes {
    pub field: u8,
    pub rows: [Option<u16>; PERSONAL_SLOTS],
}

/// Per-team class availability for teams 1 and 2, in each team's admitted
/// roster order: bit `i` of `available[t]` is set when a human could take
/// class `i` of team `t + 1` right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassRoom {
    /// Admitted class count for teams 1 and 2 (at most `CLASS_CAPACITY`).
    pub count: [u8; 2],
    /// Availability bitmask per team (bits at or past `count[t]` are 0).
    pub available: [u16; 2],
}

/// One match state field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State<'a> {
    Match(Match),
    Timer(Timer<'a>),
    Personal(Personal),
    Objectives(Objectives<'a>),
    Deployments(Deployments),
    Classes(Classes),
    ClassRoom(ClassRoom),
    Leaving(Leaving),
}

impl State<'_> {
    /// The id of the field this state belongs in.
    pub fn id(&self) -> u32 {
        let field = |tag: u8, field: u8| u32::from(tag) << 8 | u32::from(field);
        match self {
            State::Match(_) => field(TAG_MATCH, 0),
            State::Timer(_) => field(TAG_TIMER, 0),
            State::Personal(personal) => field(TAG_PERSONAL, personal.field),
            State::Objectives(objectives) => field(TAG_OBJECTIVES, objectives.field),
            State::Deployments(deployments) => field(TAG_DEPLOYMENTS, deployments.field),
            State::Classes(classes) => field(TAG_CLASSES, classes.field),
            State::ClassRoom(_) => field(TAG_CLASS_ROOM, 0),
            State::Leaving(leaving) => field(TAG_LEAVING, leaving.field),
        }
    }
}

/// What removing the state field at an id clears, by the field it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleared {
    Match,
    Timer,
    Personal(u8),
    Objectives(u8),
    Deployments(u8),
    Classes(u8),
    ClassRoom,
    Leaving(u8),
}

/// The state a removed field at `id` held; none for an id no state field has.
pub fn cleared(id: u32) -> Option<Cleared> {
    let field = u8::try_from(id & 0xff).ok()?;
    match (id >> 8, field) {
        (0, 0) => Some(Cleared::Match),
        (1, 0) => Some(Cleared::Timer),
        (2, f) if usize::from(f) < PERSONAL_FIELDS => Some(Cleared::Personal(f)),
        (3, f) if usize::from(f) < OBJECTIVE_FIELDS => Some(Cleared::Objectives(f)),
        (4, f) if usize::from(f) < DEPLOYMENT_FIELDS => Some(Cleared::Deployments(f)),
        (5, f) if usize::from(f) < CLASSES_FIELDS => Some(Cleared::Classes(f)),
        (6, 0) => Some(Cleared::ClassRoom),
        (7, f) if usize::from(f) < LEAVING_FIELDS => Some(Cleared::Leaving(f)),
        _ => None,
    }
}

/// What a critical system is in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placed {
    #[default]
    Elsewhere,
    /// The placed building of this index, as building fields name it.
    Building(u16),
    /// The hull of this target id, as hull records name it.
    Hull(u16),
}

/// One critical system of the mission.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct System<'a> {
    pub index: u8,
    pub alive: bool,
    /// Whether the script marks it for the players.
    pub marker: bool,
    pub team: u8,
    pub placed: Placed,
    pub points: i32,
    /// Health and its maximum.
    pub health: Option<[f32; 2]>,
    /// Shield charge and its capacity.
    pub shield: Option<[f32; 2]>,
    /// Its place on the HUD's system picture.
    pub hud: Option<[f32; 2]>,
    pub name: &'a str,
}

impl System<'_> {
    /// The system without its name, to keep beside one that owns it.
    pub fn unnamed(&self) -> System<'static> {
        let System { index, alive, marker, team, placed, points, health, shield, hud, .. } = *self;
        System { index, alive, marker, team, placed, points, health, shield, hud, name: "" }
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], &'static str> {
        let (head, rest) = self.0.split_at_checked(length).ok_or(INVALID)?;
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, &'static str> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16, &'static str> { Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| INVALID)?)) }
    fn u32(&mut self) -> Result<u32, &'static str> { Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| INVALID)?)) }
    fn i32(&mut self) -> Result<i32, &'static str> { self.u32().map(|bits| bits as i32) }
    fn f32(&mut self) -> Result<f32, &'static str> {
        Some(f32::from_bits(self.u32()?)).filter(|value| value.is_finite()).ok_or(INVALID)
    }
    fn text(&mut self, shortest: usize) -> Result<&'a str, &'static str> {
        let length = usize::from(self.u8()?);
        if !(shortest..=MAX_TEXT).contains(&length) { return Err(INVALID); }
        std::str::from_utf8(self.take(length)?).map_err(|_| INVALID)
    }
    fn end(self) -> Result<(), &'static str> { if self.0.is_empty() { Ok(()) } else { Err(INVALID) } }
}

fn flag(flags: u16, bit: u32) -> bool { flags & 1 << bit != 0 }

/// Reads a match state field's body, after its revision.
pub fn read_state(body: &[u8]) -> Result<State<'_>, &'static str> {
    let mut r = Reader(body);
    let state = match r.u8()? {
        TAG_MATCH => {
            let flags = r.u16()?;
            let (kind, team, target) = (r.u8()?, r.u8()?, r.u32()?);
            let (points, tickets) = ([r.u32()?, r.u32()?], [r.u32()?, r.u32()?]);
            let outcome = match (kind, team) {
                (0, 0) => None,
                (1, 1..=7) => Some(CompactOutcome::Victory(team.into())),
                (2, 1..=7) => Some(CompactOutcome::Defeat(team.into())),
                (3, 0) => Some(CompactOutcome::Draw),
                (4, 0) => Some(CompactOutcome::FaultedDraw),
                _ => return Err(INVALID),
            };
            let finite = [flag(flags, 9), flag(flags, 10)];
            if flags >> 11 != 0 || !flag(flags, 8) && target != 0 || (0..2).any(|i| !finite[i] && tickets[i] != 0) { return Err(INVALID); }
            State::Match(Match { over: flag(flags, 0), game_over: flag(flags, 1), finished: flag(flags, 2), faulted: flag(flags, 3),
                points_shown: [flag(flags, 4), flag(flags, 5)], minimap_hidden: flag(flags, 6), posts_hidden: flag(flags, 7), outcome,
                target: flag(flags, 8).then_some(target), points, reinforcements: std::array::from_fn(|i| finite[i].then_some(tickets[i])) })
        }
        TAG_TIMER => {
            let flags = u16::from(r.u8()?);
            let (due, left, name) = (r.u32()?, r.u32()?, r.text(0)?);
            let (shown, running) = (flag(flags, 0), flag(flags, 1));
            if flags >> 2 != 0 || running && !shown || !running && due != 0 || (running || !shown) && left != 0 || !shown && !name.is_empty() {
                return Err(INVALID);
            }
            State::Timer(Timer { shown, due: running.then_some(due), left, name })
        }
        TAG_PERSONAL => {
            let (field, mask) = (r.u8()?, r.u32()?);
            if usize::from(field) >= PERSONAL_FIELDS || mask == 0 { return Err(INVALID); }
            let mut rows = [None; PERSONAL_SLOTS];
            for (slot, row) in rows.iter_mut().enumerate() {
                if mask & 1 << slot != 0 { *row = Some(r.i32()?); }
            }
            State::Personal(Personal { field, rows })
        }
        TAG_OBJECTIVES => {
            let (field, count) = (r.u8()?, usize::from(r.u8()?));
            if usize::from(field) >= OBJECTIVE_FIELDS || !(1..=OBJECTIVE_ROWS).contains(&count) { return Err(INVALID); }
            let mut rows = [None; OBJECTIVE_ROWS];
            for row in &mut rows[..count] {
                let (flags, team) = (u16::from(r.u8()?), r.u8()?);
                if flags >> 2 != 0 || !matches!(team, EVERYONE..=7 | NOBODY) { return Err(INVALID); }
                *row = Some(Objective { active: flag(flags, 0), completed: flag(flags, 1), team, key: r.text(1)? });
            }
            State::Objectives(Objectives { field, rows })
        }
        TAG_DEPLOYMENTS => {
            let (field, mask, chosen) = (r.u8()?, r.u32()?, r.u32()?);
            if usize::from(field) >= DEPLOYMENT_FIELDS || chosen & !mask != 0 { return Err(INVALID); }
            let mut rows = [None; PERSONAL_SLOTS];
            for (slot, row) in rows.iter_mut().enumerate() {
                if mask & 1 << slot != 0 { *row = Some(Deployment { due: r.u32()?, chosen: chosen & 1 << slot != 0 }); }
            }
            State::Deployments(Deployments { field, rows })
        }
        TAG_LEAVING => {
            let (field, mask) = (r.u8()?, r.u32()?);
            if usize::from(field) >= LEAVING_FIELDS { return Err(INVALID); }
            let mut rows = [None; PERSONAL_SLOTS];
            for (slot, row) in rows.iter_mut().enumerate() {
                if mask & 1 << slot != 0 { *row = Some(r.u32()?); }
            }
            State::Leaving(Leaving { field, rows })
        }
        TAG_CLASSES => {
            let (field, mask) = (r.u8()?, r.u32()?);
            if usize::from(field) >= CLASSES_FIELDS || mask == 0 { return Err(INVALID); }
            let mut rows = [None; PERSONAL_SLOTS];
            for (slot, row) in rows.iter_mut().enumerate() {
                if mask & 1 << slot != 0 {
                    let class = r.u16()?;
                    if class == 0 { return Err(INVALID); }
                    *row = Some(class);
                }
            }
            State::Classes(Classes { field, rows })
        }
        TAG_CLASS_ROOM => {
            let count = [r.u8()?, r.u8()?];
            let available = [r.u16()?, r.u16()?];
            for team in 0..2 {
                let n = usize::from(count[team]);
                if n > crate::battle::spawning::CLASS_CAPACITY || available[team] >> n != 0 {
                    return Err(INVALID);
                }
            }
            State::ClassRoom(ClassRoom { count, available })
        }
        _ => return Err(INVALID),
    };
    r.end()?;
    Ok(state)
}

/// Writes a match state field's body, after its revision.
pub fn write_state(state: &State<'_>, out: &mut Vec<u8>) {
    match state {
        State::Match(m) => {
            out.push(TAG_MATCH);
            let bits = [m.over, m.game_over, m.finished, m.faulted, m.points_shown[0], m.points_shown[1], m.minimap_hidden, m.posts_hidden,
                m.target.is_some(), m.reinforcements[0].is_some(), m.reinforcements[1].is_some()];
            let flags = bits.iter().enumerate().fold(0u16, |flags, (bit, &on)| flags | u16::from(on) << bit);
            out.extend(flags.to_le_bytes());
            let (kind, team) = match m.outcome {
                None => (0, 0),
                Some(CompactOutcome::Victory(team)) => (1, team.clamp(1, 7) as u8),
                Some(CompactOutcome::Defeat(team)) => (2, team.clamp(1, 7) as u8),
                Some(CompactOutcome::Draw) => (3, 0),
                Some(CompactOutcome::FaultedDraw) => (4, 0),
            };
            out.extend([kind, team]);
            out.extend(m.target.unwrap_or(0).to_le_bytes());
            for value in m.points.into_iter().chain(m.reinforcements.map(|tickets| tickets.unwrap_or(0))) { out.extend(value.to_le_bytes()); }
        }
        State::Timer(t) => {
            out.extend([TAG_TIMER, u8::from(t.shown) | u8::from(t.due.is_some()) << 1]);
            out.extend(t.due.unwrap_or(0).to_le_bytes());
            out.extend(t.left.to_le_bytes());
            text(t.name, out);
        }
        State::Personal(p) => {
            out.extend([TAG_PERSONAL, p.field]);
            let mask = p.rows.iter().enumerate().fold(0u32, |mask, (slot, row)| mask | u32::from(row.is_some()) << slot);
            out.extend(mask.to_le_bytes());
            for points in p.rows.iter().flatten() { out.extend(points.to_le_bytes()); }
        }
        State::Objectives(o) => {
            out.extend([TAG_OBJECTIVES, o.field, o.rows.iter().flatten().count() as u8]);
            for row in o.rows.iter().flatten() {
                out.extend([u8::from(row.active) | u8::from(row.completed) << 1, row.team]);
                text(row.key, out);
            }
        }
        State::Deployments(d) => {
            out.extend([TAG_DEPLOYMENTS, d.field]);
            let mask = |of: fn(&Deployment) -> bool| d.rows.iter().enumerate()
                .fold(0u32, |mask, (slot, row)| mask | u32::from(row.as_ref().is_some_and(of)) << slot);
            out.extend(mask(|_| true).to_le_bytes());
            out.extend(mask(|row| row.chosen).to_le_bytes());
            for row in d.rows.iter().flatten() { out.extend(row.due.to_le_bytes()); }
        }
        State::Classes(c) => {
            out.extend([TAG_CLASSES, c.field]);
            let mask = c.rows.iter().enumerate().fold(0u32, |mask, (slot, row)| mask | u32::from(row.is_some()) << slot);
            out.extend(mask.to_le_bytes());
            for class in c.rows.iter().flatten() { out.extend(class.to_le_bytes()); }
        }
        State::ClassRoom(room) => {
            out.extend([TAG_CLASS_ROOM, room.count[0], room.count[1]]);
            for mask in room.available {
                out.extend(mask.to_le_bytes());
            }
        }
        State::Leaving(l) => {
            out.extend([TAG_LEAVING, l.field]);
            let mask = l.rows.iter().enumerate().fold(0u32, |mask, (slot, row)| mask | u32::from(row.is_some()) << slot);
            out.extend(mask.to_le_bytes());
            for due in l.rows.iter().flatten() { out.extend(due.to_le_bytes()); }
        }
    }
}

fn text(value: &str, out: &mut Vec<u8>) {
    out.push(value.len() as u8);
    out.extend_from_slice(value.as_bytes());
}

/// Reads a system field's body, after its revision.
pub fn read_system(body: &[u8]) -> Result<System<'_>, &'static str> {
    let mut r = Reader(body);
    let (index, flags, team, place, place_id) = (r.u8()?, u16::from(r.u8()?), r.u8()?, r.u8()?, r.u16()?);
    let points = r.i32()?;
    let mut pair = |known: bool, lowest: f32| -> Result<Option<[f32; 2]>, &'static str> {
        let values = [r.f32()?, r.f32()?];
        if !known { return if values.iter().all(|value| value.to_bits() == 0) { Ok(None) } else { Err(INVALID) }; }
        if values.iter().any(|value| *value < lowest) { return Err(INVALID); }
        Ok(Some(values))
    };
    let health = pair(flag(flags, 2), 0.)?;
    let shield = pair(flag(flags, 3), 0.)?;
    let hud = pair(flag(flags, 4), f32::MIN)?;
    let placed = match (place, place_id) {
        (0, 0) => Placed::Elsewhere,
        (1, building) => Placed::Building(building),
        (2, hull) => Placed::Hull(hull),
        _ => return Err(INVALID),
    };
    if usize::from(index) >= SYSTEMS || flags >> 5 != 0 || team > 7 { return Err(INVALID); }
    let name = r.text(1)?;
    r.end()?;
    Ok(System { index, alive: flag(flags, 0), marker: flag(flags, 1), team, placed, points, health, shield, hud, name })
}

/// Writes a system field's body, after its revision.
pub fn write_system(system: &System<'_>, out: &mut Vec<u8>) {
    let bits = [system.alive, system.marker, system.health.is_some(), system.shield.is_some(), system.hud.is_some()];
    let flags = bits.iter().enumerate().fold(0u8, |flags, (bit, &on)| flags | u8::from(on) << bit);
    let (place, id) = match system.placed {
        Placed::Elsewhere => (0, 0),
        Placed::Building(building) => (1, building),
        Placed::Hull(hull) => (2, hull),
    };
    out.extend([system.index, flags, system.team, place]);
    out.extend(id.to_le_bytes());
    out.extend(system.points.to_le_bytes());
    for pair in [system.health, system.shield, system.hud] {
        for value in pair.unwrap_or_default() { out.extend(value.to_le_bytes()); }
    }
    text(system.name, out);
}

/// A player's part in a kill: his roster slot and what he was at that moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actor {
    pub slot: u16,
    pub human: bool,
    /// His class's registry id, 0 for none.
    pub class: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A message banner's localization key.
    Text(String),
    /// A hint's localization key.
    Hint(String),
    /// A voice-over's sound id.
    Voice(String),
    Kill {
        killer: Option<Actor>,
        victim: Actor,
        /// The weapon's registry id, 0 for none.
        weapon: u16,
        /// A kill by a ship's gun no one operates (it has no killer): the gun
        /// building's entity class name, empty when it has none.
        gun: Option<String>,
    },
}

/// One notice as its recipient reads it: his own rising sequence, the
/// authority tick it belongs to and the tick it stops being worth showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub sequence: u32,
    pub tick: u32,
    pub until: u32,
    pub cue: Cue,
}

impl Notice {
    /// The body of its reliable record.
    pub fn body(&self) -> Value {
        let actor = |a: &Actor| json!([a.slot, a.human, a.class]);
        let (key, value) = match &self.cue {
            Cue::Text(key) => ("text", json!(key)),
            Cue::Hint(key) => ("hint", json!(key)),
            Cue::Voice(sound) => ("voice", json!(sound)),
            Cue::Kill { killer, victim, weapon, gun: None } => ("kill", json!([killer.as_ref().map(actor), actor(victim), weapon])),
            Cue::Kill { killer, victim, weapon, gun: Some(gun) } => ("kill", json!([killer.as_ref().map(actor), actor(victim), weapon, gun])),
        };
        json!({"sequence": self.sequence, "tick": self.tick, "until": self.until, key: value})
    }

    /// Reads a notice record's body.
    pub fn read(body: &Value) -> Result<Self, &'static str> {
        let object = body.as_object().filter(|o| o.len() == 4).ok_or(INVALID)?;
        // Whole numbers a JSON reader keeps exactly.
        let sequence = object.get("sequence").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).filter(|n| *n >= 1).ok_or(INVALID)?;
        let tick = |key: &str| object.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).ok_or(INVALID);
        let (at, until) = (tick("tick")?, tick("until")?);
        if until.wrapping_sub(at) > NOTICE_TICKS { return Err(INVALID); }
        let cue = match ["text", "hint", "voice", "kill"].into_iter().find(|key| object.contains_key(*key)) {
            Some("text") => Cue::Text(short(&object["text"])?),
            Some("hint") => Cue::Hint(short(&object["hint"])?),
            Some("voice") => Cue::Voice(short(&object["voice"])?),
            Some(_) => {
                let (killer, victim, weapon, gun) = match object["kill"].as_array().map(Vec::as_slice).ok_or(INVALID)? {
                    [killer, victim, weapon] => (killer, victim, weapon, None),
                    // An unmanned gun's kill has no killer.
                    [killer, victim, weapon, gun] if killer.is_null() => (killer, victim, weapon, Some(gun)),
                    _ => return Err(INVALID),
                };
                let killer = if killer.is_null() { None } else { Some(read_actor(killer)?) };
                let gun = gun.map(|gun| gun.as_str().filter(|g| g.len() <= MAX_TEXT && !g.contains('|')).map(str::to_string).ok_or(INVALID)).transpose()?;
                Cue::Kill { killer, victim: read_actor(victim)?, weapon: id(weapon)?, gun }
            }
            None => return Err(INVALID),
        };
        Ok(Self { sequence, tick: at, until, cue })
    }
}

fn id(value: &Value) -> Result<u16, &'static str> {
    value.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or(INVALID)
}

fn short(value: &Value) -> Result<String, &'static str> {
    value.as_str().filter(|text| (1..=MAX_TEXT).contains(&text.len())).map(str::to_owned).ok_or(INVALID)
}

fn read_actor(value: &Value) -> Result<Actor, &'static str> {
    let [slot, human, class] = value.as_array().map(Vec::as_slice).ok_or(INVALID)? else { return Err(INVALID) };
    Ok(Actor { slot: id(slot)?, human: human.as_bool().ok_or(INVALID)?, class: id(class)? })
}

/// One team of the mission's roster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Team {
    pub name: String,
    pub icons: Vec<String>,
    /// The registry ids of the classes a player may choose, in the script's
    /// order: the default leads.
    pub classes: Vec<u16>,
}

/// The mission's two teams, 1 and 2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
    pub teams: [Team; 2],
}

/// The most icons a team names.
pub const MAX_ICONS: usize = 4;

impl Roster {
    /// The roster of an admitted mission, its classes by their ids in the
    /// matched registry `descriptor`; none for a battle without one.
    pub fn of(world: &World, descriptor: &Value) -> Result<Option<Self>, String> {
        let Some(view) = world.mission_view() else { return Ok(None) };
        let ids = descriptor.get("classes").and_then(Value::as_array).ok_or("the battle's registry has no classes")?;
        let mut roster = Self::default();
        for (team, number) in roster.teams.iter_mut().zip([1, 2]) {
            let Some(setup) = view.host.setup.teams.get(&number) else { continue };
            let bounded = |text: &String| text.len() <= MAX_TEXT;
            team.name = setup.name.clone().filter(bounded).unwrap_or_default();
            team.icons = setup.icons.iter().filter(|icon| bounded(icon) && !icon.is_empty()).take(MAX_ICONS).cloned().collect();
            for class in &setup.classes {
                let id = ids.iter().find(|row| row[1].as_str() == Some(class.class_name.as_str())).and_then(|row| row[0].as_u64())
                    .and_then(|id| u16::try_from(id).ok()).ok_or_else(|| format!("the mission's class {} has no registry id", class.class_name))?;
                team.classes.push(id);
            }
        }
        Ok(Some(roster))
    }

    /// The body of its record.
    pub fn body(&self) -> Value {
        json!({"teams": self.teams.iter().map(|team| json!({"name": team.name, "icons": team.icons, "classes": team.classes})).collect::<Vec<_>>()})
    }

    /// Reads a roster record's body. Each class must be one `known` takes: an
    /// id of the matched registry.
    pub fn read(body: &Value, known: impl Fn(u16) -> bool) -> Result<Self, &'static str> {
        let fields = |value: &'_ Value, count: usize| -> Result<Map<String, Value>, &'static str> {
            value.as_object().filter(|o| o.len() == count).cloned().ok_or(INVALID)
        };
        let body = fields(body, 1)?;
        let [first, second] = body.get("teams").and_then(Value::as_array).map(Vec::as_slice).ok_or(INVALID)? else { return Err(INVALID) };
        let team = |value: &Value| -> Result<Team, &'static str> {
            let object = fields(value, 3)?;
            let name = object.get("name").and_then(Value::as_str).filter(|name| name.len() <= MAX_TEXT).ok_or(INVALID)?;
            let icons = object.get("icons").and_then(Value::as_array).filter(|icons| icons.len() <= MAX_ICONS).ok_or(INVALID)?;
            let classes = object.get("classes").and_then(Value::as_array)
                .filter(|classes| classes.len() <= crate::battle::spawning::CLASS_CAPACITY).ok_or(INVALID)?;
            let classes = classes.iter().map(id).collect::<Result<Vec<_>, _>>()?;
            if classes.iter().any(|class| *class == 0 || !known(*class)) { return Err(INVALID); }
            Ok(Team { name: name.to_owned(), icons: icons.iter().map(short).collect::<Result<_, _>>()?, classes })
        };
        Ok(Self { teams: [team(first)?, team(second)?] })
    }
}

// The authority's side.

/// One field of a capture: its carrier, what makes it send at once, and how
/// often it is otherwise refreshed.
pub(crate) struct Offer {
    pub schema: u16,
    pub id: u32,
    pub state: u64,
    pub cadence: f64,
    pub payload: Vec<u8>,
}

/// The fields a scripted battle publishes, prepared from the world before its
/// capture. The bank keeps its storage.
#[derive(Default)]
pub struct Bank {
    offers: Vec<Offer>,
    used: usize,
    /// Texts the script set that were too long for their record.
    pub omitted: u32,
}

// The change identity of a whole payload.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3))
}

impl Bank {
    pub(crate) fn offers(&self) -> &[Offer] { &self.offers[..self.used] }

    fn add(&mut self, schema: u16, id: u32, cadence: f64, write: impl FnOnce(&mut Vec<u8>)) -> &mut Offer {
        if self.used == self.offers.len() {
            self.offers.push(Offer { schema, id, state: 0, cadence, payload: Vec::with_capacity(OBJECTIVES_BYTES) });
        }
        let offer = &mut self.offers[self.used];
        self.used += 1;
        (offer.schema, offer.id, offer.cadence) = (schema, id, cadence);
        offer.payload.clear();
        offer.payload.extend(REVISION.to_le_bytes());
        write(&mut offer.payload);
        offer.state = fnv(&offer.payload);
        offer
    }

    // A change of a state field is sent at the next publication.
    fn state(&mut self, state: State<'_>) {
        self.add(STATE_SCHEMA, state.id(), 60., |out| write_state(&state, out));
    }

    /// Prepares what the mission says at authority tick `tick`.
    pub(crate) fn sample(&mut self, world: &World, tick: u32) {
        self.used = 0;
        let Some(view) = world.mission_view() else { return };
        let (host, score) = (view.host, view.score());
        self.state(State::Match(Match {
            over: score.over, game_over: score.game_over, finished: score.finished, faulted: score.faulted,
            points_shown: [host.team_points_visible[1], host.team_points_visible[2]],
            minimap_hidden: host.minimap_disabled, posts_hidden: host.command_posts_hidden,
            outcome: score.outcome, target: score.target, points: score.teams,
            reinforcements: [1., 2.].map(|team| view.battle.reinforcements.get(team).copied()
                .filter(|tickets| tickets.is_finite() && *tickets >= 0.).map(|tickets| tickets as u32)),
        }));
        let timer = view.shown_timer(tick).and_then(|shown| {
            let name = std::str::from_utf8(shown.name).ok().filter(|name| name.len() <= MAX_TEXT)?;
            let due = shown.due.and_then(|due| u32::try_from(due).ok());
            let left = if due.is_some() { 0 } else { shown.left as u32 };
            Some(Timer { shown: true, due, left, name })
        });
        self.state(State::Timer(timer.unwrap_or_default()));

        // Personal points by roster slot. A slot is found once per row, as the
        // roster is small and the rows fewer.
        let mut personal = [[None; PERSONAL_SLOTS]; PERSONAL_FIELDS];
        for row in view.rules.personal_points() {
            if let Some(slot) = view.battle.characters.iter().position(|c| *c == row.character) {
                if let Some(at) = personal.get_mut(slot / PERSONAL_SLOTS) { at[slot % PERSONAL_SLOTS] = Some(row.points); }
            }
        }
        for (field, rows) in personal.iter().enumerate().filter(|(_, rows)| rows.iter().any(Option::is_some)) {
            self.state(State::Personal(Personal { field: field as u8, rows: *rows }));
        }

        // Who waits for a body, for every roster slot: a field with nobody in
        // it says so, and one whose rows only count down does not change.
        for (field, chunk) in view.battle.characters.chunks(PERSONAL_SLOTS).take(DEPLOYMENT_FIELDS).enumerate() {
            let mut rows = [None; PERSONAL_SLOTS];
            for (row, reference) in rows.iter_mut().zip(chunk) {
                let Some(player) = world.sim.store.character.get(*reference).filter(|c| c.human && c.unit.is_none() && !score.over) else { continue };
                *row = view.rules.deploys_in(&world.sim.store, player).map(|ticks| Deployment {
                    due: if ticks == 0 { 0 } else { tick.saturating_add(ticks) },
                    // What he chose for his last life is not a choice of his next.
                    chosen: player.spawn.as_ref().is_some_and(|spawn| spawn.ready && spawn.timer <= 0.) });
            }
            self.state(State::Deployments(Deployments { field: field as u8, rows }));
        }

        // Who is outside the battlefield, for every roster slot: the tick his
        // countdown runs out, which stays the same while it runs, so the field
        // changes only as a player leaves or comes back.
        for (field, chunk) in view.battle.characters.chunks(PERSONAL_SLOTS).take(LEAVING_FIELDS).enumerate() {
            let mut rows = [None; PERSONAL_SLOTS];
            for (row, reference) in rows.iter_mut().zip(chunk) {
                let Some(player) = world.sim.store.character.get(*reference)
                    .filter(|c| c.human && c.unit.is_some() && !score.over && c.out_of_bounds >= 0. && c.out_of_bounds.is_finite()) else { continue };
                *row = Some(tick.saturating_add(jsrt::js_round(player.out_of_bounds * 60.) as u32));
            }
            self.state(State::Leaving(Leaving { field: field as u8, rows }));
        }

        // Current class id by roster slot, so the scoreboard names bots as
        // Instant Action does.
        let mut classes = [[None; PERSONAL_SLOTS]; CLASSES_FIELDS];
        world.visit_screen_roster(|slot, _human, class| {
            if let Some(id) = class.filter(|id| *id != 0) {
                if let Some(at) = classes.get_mut(slot / PERSONAL_SLOTS) { at[slot % PERSONAL_SLOTS] = Some(id); }
            }
        });
        for (field, rows) in classes.iter().enumerate().filter(|(_, rows)| rows.iter().any(Option::is_some)) {
            self.state(State::Classes(Classes { field: field as u8, rows: *rows }));
        }

        // Per-team class availability in admitted roster order: same check as
        // deployment refusal, sent as a delta whenever any class fills or opens.
        let mut count = [0u8; 2];
        let mut available = [0u16; 2];
        for (team_idx, number) in [1i32, 2].into_iter().enumerate() {
            let Some(setup) = host.setup.teams.get(&number) else { continue };
            let n = setup.classes.len().min(crate::battle::spawning::CLASS_CAPACITY);
            count[team_idx] = n as u8;
            for (bit, class) in setup.classes.iter().take(n).enumerate() {
                if world.mission_class_has_room(number as usize, &class.class_name).unwrap_or(true) {
                    available[team_idx] |= 1u16 << bit;
                }
            }
        }
        self.state(State::ClassRoom(ClassRoom { count, available }));

        for (field, chunk) in host.objectives.chunks(OBJECTIVE_ROWS).take(OBJECTIVE_FIELDS).enumerate() {
            let mut rows = [None; OBJECTIVE_ROWS];
            for (row, objective) in rows.iter_mut().zip(chunk) {
                // A row keeps its place, so a key too long for the wire still
                // counts: it is sent shortened and names nothing.
                let mut length = objective.text_key.len().min(MAX_TEXT);
                while !objective.text_key.is_char_boundary(length) { length -= 1; }
                if length < objective.text_key.len() { self.omitted = self.omitted.saturating_add(1); }
                let key = if length == 0 { "?" } else { &objective.text_key[..length] };
                *row = Some(Objective { active: objective.active, completed: objective.completed, team: audience(objective.team), key });
            }
            self.state(State::Objectives(Objectives { field: field as u8, rows }));
        }

        world.visit_mission_systems(|system| {
            if system.name.is_empty() || system.name.len() > MAX_TEXT { self.omitted = self.omitted.saturating_add(1); return; }
            let charge = |pair: Option<[f32; 2]>| pair.is_some_and(|[now, _]| now > 0.);
            // Its fate, side and shield go at once; its wear on a clock.
            let state = u64::from(system.alive) | u64::from(system.marker) << 1 | u64::from(charge(system.shield)) << 2 | u64::from(system.team) << 8;
            self.add(SYSTEM_SCHEMA, u32::from(system.index), 12., |out| write_system(&system, out)).state = state;
        });
    }
}

impl World {
    /// The mission's critical systems, as they are now.
    pub fn visit_mission_systems(&self, mut visit: impl FnMut(System<'_>)) {
        use crate::battle::rules::events::ObjectHandle;
        let Some(view) = self.mission_view() else { return };
        let finite = |pair: [f64; 2]| Some(pair.map(|v| v.max(0.) as f32)).filter(|pair| pair.iter().all(|v| v.is_finite()));
        for (index, system) in view.host.critical_systems.iter().take(SYSTEMS).enumerate() {
            // A rebuilt object is the same system in its present life.
            let object = self.current_mission_object(&system.object).ok();
            let state = object.as_ref().and_then(|object| self.mission_system_state(object).ok());
            let shield = object.as_ref().and_then(|object| self.mission_system_shields(object).ok()).filter(|(_, capacity, _)| *capacity > 0.);
            let placed = match &system.object {
                ObjectHandle::Prop(handle) => crate::units::f3_props::building_of_in(&self.units, handle)
                    .and_then(|at| u16::try_from(at).ok()).map_or(Placed::Elsewhere, Placed::Building),
                ObjectHandle::Vehicle { vehicle, .. } => self.sim.store.vehicle.get(*vehicle)
                    .filter(|v| (0.0..=65535.0).contains(&v.target_id) && v.target_id.fract() == 0.).map_or(Placed::Elsewhere, |v| Placed::Hull(v.target_id as u16)),
                ObjectHandle::Door(_) | ObjectHandle::Post(_) => Placed::Elsewhere,
            };
            let team = state.and_then(|state| state.team).filter(|team| (0.0..8.0).contains(team)).map_or(0, |team| team as u8);
            let hud = [system.hud_u, system.hud_v];
            visit(System {
                index: index as u8,
                alive: state.is_some_and(|state| state.alive),
                marker: system.show_marker,
                team,
                placed,
                points: system.points,
                health: state.and_then(|state| state.health).and_then(|(now, most)| finite([now, most])),
                shield: shield.and_then(|(now, capacity, _)| finite([now, capacity])),
                hud: Some(hud).filter(|hud| hud.iter().all(|v| v.is_finite() && *v >= 0.)),
                name: &system.name,
            });
        }
    }
}

/// What the authority has already announced: the script's notifications by
/// their sequence and the deaths by rules tick.
#[derive(Default)]
pub struct Feed {
    delivered: u32,
    rules_tick: u32,
}

impl Feed {
    /// The cues that came up since the last call, each with the team it is
    /// for ([`EVERYONE`] when the script named none). `weapon` gives a weapon
    /// class's id in the matched registry, 0 for none.
    pub fn collect(&mut self, world: &World, weapon: impl Fn(&str) -> u16, mut cue: impl FnMut(u8, Cue)) {
        let Some(view) = world.mission_view() else { return };
        // A gap (more notifications than the host keeps) starts again at the
        // newest: what was missed has passed.
        if let Some(notifications) = view.host.notifications_after(self.delivered) {
            for entry in notifications {
                let (team, value) = match &entry.notification {
                    ScriptNotification::ShowMessageText { key, team } => (*team, Cue::Text(key.to_string())),
                    ScriptNotification::BroadcastVoiceOver { sound_id, team } => (*team, Cue::Voice(sound_id.to_string())),
                    ScriptNotification::AddMissionHint { key } => (None, Cue::Hint(key.clone())),
                    // The state fields carry what these changed.
                    ScriptNotification::AddMissionObjective { .. } | ScriptNotification::ActivateObjective { .. }
                    | ScriptNotification::CompleteObjective { .. } | ScriptNotification::ShowTeamPoints { .. }
                    | ScriptNotification::ShowHuntScoreLimit { .. } | ScriptNotification::MapHideCommandPosts { .. }
                    | ScriptNotification::DisableSmallMapMiniMap | ScriptNotification::ShowTimer { .. } => continue,
                };
                let text = match &value { Cue::Text(text) | Cue::Voice(text) | Cue::Hint(text) => text, Cue::Kill { .. } => unreachable!("a script text") };
                if (1..=MAX_TEXT).contains(&text.len()) { cue(team.map_or(EVERYONE, audience), value); }
            }
        }
        self.delivered = view.host.last_notification_sequence();
        // The host keeps the deaths of the last rules tick it ran.
        if view.host.tick == self.rules_tick { return; }
        self.rules_tick = view.host.tick;
        if view.host.deaths.is_empty() { return; }
        let mut roster = vec![None; view.battle.characters.len()];
        world.visit_screen_roster(|slot, human, class| {
            let actor = u16::try_from(slot).ok().map(|slot| Actor { slot, human, class: class.unwrap_or(0) });
            if let Some(row) = roster.get_mut(slot) { *row = actor; }
        });
        let actor = |life| world.screen_character(life).and_then(|slot| roster.get(slot).copied().flatten());
        for death in &view.host.deaths {
            if let Some(victim) = actor(death.victim) {
                let weapon_id = death.weapon.and_then(|with| world.screen_weapon(with)).map_or(0, &weapon);
                let killer = death.killer.and_then(actor);
                // A ship's gun no one operates kills with no killer: the feed names the gun.
                let gun = match death.weapon {
                    Some(with @ crate::units::infantry::contact::Weapon::SpaceBuilding(_)) if killer.is_none() =>
                        Some(world.screen_weapon_entity(with).filter(|name| name.len() <= MAX_TEXT && !name.contains('|')).unwrap_or("").to_string()),
                    _ => None,
                };
                cue(EVERYONE, Cue::Kill { killer, victim, weapon: weapon_id, gun });
            }
        }
    }
}

#[cfg(test)]
mod tests;
