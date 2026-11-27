//! A scripted 2005 battle's mission as a network client knows it: the state
//! its authority replicates (`bfe_sim::wire::mission`), and the observation
//! the battle's screens read, the same one Instant Action's world gives them.
//!
//! The records are checked before they reach here (`revisions::Checks` for the
//! world fields, the receiver for the reliable ones); what is stored is the
//! newest of each, and a new epoch starts with nothing but the roster.
use super::{hud::{Hud, HudOwnSummary}, receiver::Receiver, replay::Replay};
use crate::local::mission_view::{hud_timer, label, Admission, Body, Label, Notice, NoticeAt, Objective, Observation, Participant, Pose, Slot, System, TeamInfo};
use bfe_presentation::network_layout::{net_kind as k, net_limits as l, net_record as r};
use bfe_sim::battle::script::MissionScore;
use bfe_sim::units::infantry::contact::LifeId;
use bfe_sim::wire::mission as wire;
use std::collections::VecDeque;
use std::sync::Arc;

/// The most notices kept for the screens and the sound.
const NOTICES: usize = 32;
/// The most roster slots a battle's personal points cover.
const SLOTS: usize = wire::PERSONAL_FIELDS * wire::PERSONAL_SLOTS;

#[derive(Clone, Copy)]
struct Goal {
    active: bool,
    completed: bool,
    team: u8,
    key: Label,
}

/// The mission HUD timer: either counting down to an authority tick (`Running`)
/// or paused with a fixed number of ticks left (`Paused`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MissionTimer {
    Running { name: Label, due: u32 },
    Paused { name: Label, left: u32 },
}

/// What a 2005 weapon is called and drawn as on the battle screens, by its
/// definition's place in the battle's weapon list.
#[derive(Clone, Copy)]
pub struct Loadout {
    pub(super) name: Label,
    pub(super) base: Label,
    pub(super) icon: Label,
    pub(super) reticle: Label,
    pub(super) power: bool,
    pub(super) rounds: f64,
}

/// What the screens' observation takes from the battle's HUD: the player's
/// spawn summary, a class's health by registry id, and a weapon's loadout and
/// the trigger it answers to by definition.
pub trait Panel {
    fn summary(&self) -> &HudOwnSummary;
    fn health(&self, class: usize) -> f64;
    fn loadout(&self, definition: u32) -> Option<(Loadout, usize)>;
    /// The roster slot of a unit life's character, as the unit summaries say it.
    fn unit_slot(&self, _id: u32, _generation: u32) -> Option<usize> { None }
}

impl Panel for Hud {
    fn summary(&self) -> &HudOwnSummary { &self.own_summary }
    fn health(&self, class: usize) -> f64 { self.class_health(class) }
    fn loadout(&self, definition: u32) -> Option<(Loadout, usize)> { self.weapon_loadout(definition) }
    fn unit_slot(&self, id: u32, generation: u32) -> Option<usize> { self.feed_unit_slot(id, generation) }
}

impl Loadout {
    pub(super) fn of(record: &bfe_content::Value) -> Self {
        let text = |key: &str| label(record.get(key).and_then(bfe_content::Value::as_str).unwrap_or(""));
        let number = |key: &str| bfe_content::odf::num(record.get(key), 0.);
        let base = text("base");
        Self { name: text("displayName"), base, icon: text("HUDTag"), reticle: text("ReticuleTexture"),
            power: number("EnergyDrain") > 0. || base.as_str() == "areaeffectweapon", rounds: number("RoundsPerClip") }
    }
}

/// The mission of the battle a client plays.
pub struct Mission {
    admission: Arc<Admission>,
    state: Option<wire::Match>,
    timer: Option<MissionTimer>,
    personal: Box<[Option<i32>; SLOTS]>,
    goals: Box<[Option<Goal>; wire::OBJECTIVE_FIELDS * wire::OBJECTIVE_ROWS]>,
    systems: Box<[Option<System>; wire::SYSTEMS]>,
    deployments: Box<[Option<wire::Deployment>; SLOTS]>,
    classes: Box<[Option<u16>; SLOTS]>,
    /// The authority tick each roster slot's leaving countdown runs out, while he is outside the battlefield.
    leaving: Box<[Option<u32>; SLOTS]>,
    class_room: Option<wire::ClassRoom>,
    notices: VecDeque<NoticeAt>,
    sequence: u32,
    // The last observed place of each soldier, and the weapon each hand held.
    poses: Vec<Option<(LifeId, u32, Pose)>>,
    held: [Option<(u32, u32)>; 2],
    /// The player's soldier is outside the battlefield: the seconds left on
    /// his leaving countdown, as the authority says it (see [`Mission::set_own_leaving`]).
    own_leaving: Option<f32>,
    /// The player's newest hit (its tick, whether critical) and the life that made it.
    hit: Option<(u32, bool)>,
    hit_life: Option<(u32, u32)>,
    /// The power lock target's pose on the tick before, for its bracket's motion.
    previous_lock: Option<(LifeId, u32, crate::local::mission_view::LockedPose)>,
    /// A field of this epoch has come: until then, the notices that came with
    /// the welcome are this epoch's.
    begun: bool,
}

const INVALID: &str = "invalid Battlefront II mission record";

impl Mission {
    /// The mission of a battle whose registry named its teams; `class` gives
    /// the class name of a registry id.
    pub fn new(roster: &wire::Roster, class: impl Fn(u16) -> Option<String>) -> Self {
        let teams = std::array::from_fn(|i| {
            let team = &roster.teams[i];
            TeamInfo {
                name: team.name.clone(),
                icon: team.icons.first().cloned().unwrap_or_default(),
                classes: team.classes.iter().filter_map(|id| class(*id)).map(Arc::from).collect(),
            }
        });
        Self {
            admission: Arc::new(Admission { teams }),
            state: None,
            timer: None,
            personal: Box::new([None; SLOTS]),
            goals: Box::new([None; wire::OBJECTIVE_FIELDS * wire::OBJECTIVE_ROWS]),
            systems: Box::new([None; wire::SYSTEMS]),
            deployments: Box::new([None; SLOTS]),
            classes: Box::new([None; SLOTS]),
            leaving: Box::new([None; SLOTS]),
            class_room: None,
            notices: VecDeque::with_capacity(NOTICES),
            sequence: 0,
            poses: vec![None; l::SLOTS],
            held: [None; 2],
            own_leaving: None,
            hit: None,
            hit_life: None,
            previous_lock: None,
            begun: false,
        }
    }

    /// What the authority says of the player's own soldier leaving the
    /// battlefield: the seconds left on his countdown while it runs, None while
    /// he is inside. `observe` sets it from the leaving state of his roster
    /// slot (`own_soldier_leaving`).
    pub fn set_own_leaving(&mut self, seconds_left: Option<f32>) {
        self.own_leaving = seconds_left.filter(|s| s.is_finite() && *s >= 0.);
    }

    /// The seconds left of the leaving countdown of the player in roster slot
    /// `slot` at authority tick `shown`, while he is outside the battlefield.
    pub fn leaving_left(&self, slot: usize, shown: f64) -> Option<f32> {
        let due = (*self.leaving.get(slot)?)?;
        Some((((f64::from(due) - shown) / 60.).max(0.)) as f32)
    }

    /// A new epoch is another battle: nothing the last one said holds.
    /// The notices that came before any field of the epoch stay: a battle's
    /// epoch starts with its state, which arrives after its welcome's notices
    /// (the opening hint among them).
    pub fn reset(&mut self) {
        if self.begun {
            self.notices.clear();
            self.sequence = 0;
        }
        self.begun = false;
        (self.state, self.timer, self.held) = (None, None, [None; 2]);
        self.personal.fill(None);
        self.goals.fill(None);
        self.systems.fill(None);
        self.deployments.fill(None);
        self.classes.fill(None);
        self.leaving.fill(None);
        self.class_room = None;
        self.poses.fill(None);
        self.own_leaving = None;
        (self.hit, self.hit_life, self.previous_lock) = (None, None, None);
    }

    /// Whether `schema` is one of the mission's world fields.
    pub fn carries(schema: u16) -> bool {
        schema == wire::STATE_SCHEMA || schema == wire::SYSTEM_SCHEMA
    }

    /// Installs one world field, checked whole again: its revision and body,
    /// and that it lies at the id its body names.
    pub fn install(&mut self, schema: u16, id: u32, payload: &[u8]) -> Result<(), &'static str> {
        let (revision, body) = bfe_sim::wire::revisions::split_revision(payload)?;
        if revision != wire::REVISION { return Err(INVALID); }
        self.begun = true;
        if schema == wire::SYSTEM_SCHEMA {
            let system = wire::read_system(body)?;
            if u32::from(system.index) != id { return Err(INVALID); }
            self.systems[usize::from(system.index)] = Some(System { name: label(system.name), state: system.unnamed() });
            return Ok(());
        }
        let state = wire::read_state(body)?;
        if state.id() != id { return Err(INVALID); }
        match state {
            wire::State::Match(state) => self.state = Some(state),
            wire::State::Timer(timer) => self.timer = if !timer.shown {
                None
            } else if let Some(due) = timer.due {
                Some(MissionTimer::Running { name: label(timer.name), due })
            } else {
                Some(MissionTimer::Paused { name: label(timer.name), left: timer.left })
            },
            wire::State::Personal(personal) => {
                let at = usize::from(personal.field) * wire::PERSONAL_SLOTS;
                self.personal[at..at + wire::PERSONAL_SLOTS].copy_from_slice(&personal.rows);
            }
            wire::State::Deployments(deployments) => {
                let at = usize::from(deployments.field) * wire::PERSONAL_SLOTS;
                self.deployments[at..at + wire::PERSONAL_SLOTS].copy_from_slice(&deployments.rows);
            }
            wire::State::Classes(classes) => {
                let at = usize::from(classes.field) * wire::PERSONAL_SLOTS;
                self.classes[at..at + wire::PERSONAL_SLOTS].copy_from_slice(&classes.rows);
            }
            wire::State::ClassRoom(room) => self.class_room = Some(room),
            wire::State::Leaving(leaving) => {
                let at = usize::from(leaving.field) * wire::PERSONAL_SLOTS;
                self.leaving[at..at + wire::PERSONAL_SLOTS].copy_from_slice(&leaving.rows);
            }
            wire::State::Objectives(objectives) => {
                let at = usize::from(objectives.field) * wire::OBJECTIVE_ROWS;
                for (goal, row) in self.goals[at..].iter_mut().zip(&objectives.rows) {
                    *goal = row.map(|row| Goal { active: row.active, completed: row.completed, team: row.team, key: label(row.key) });
                }
            }
        }
        Ok(())
    }

    /// Clears what the removed field at `id` held: an empty chunk has no
    /// field of its own to say so, and a system the mission stops naming
    /// leaves the screens with its field.
    pub fn remove(&mut self, schema: u16, id: u32) -> Result<(), &'static str> {
        if schema == wire::SYSTEM_SCHEMA {
            let at = usize::try_from(id).map_err(|_| INVALID)?;
            if at >= wire::SYSTEMS { return Err(INVALID); }
            self.systems[at] = None;
            return Ok(());
        }
        if schema != wire::STATE_SCHEMA { return Err(INVALID); }
        match wire::cleared(id).ok_or(INVALID)? {
            wire::Cleared::Match => self.state = None,
            wire::Cleared::Timer => self.timer = None,
            wire::Cleared::Personal(field) => {
                let at = usize::from(field) * wire::PERSONAL_SLOTS;
                self.personal[at..at + wire::PERSONAL_SLOTS].fill(None);
            }
            wire::Cleared::Deployments(field) => {
                let at = usize::from(field) * wire::PERSONAL_SLOTS;
                self.deployments[at..at + wire::PERSONAL_SLOTS].fill(None);
            }
            wire::Cleared::Classes(field) => {
                let at = usize::from(field) * wire::PERSONAL_SLOTS;
                self.classes[at..at + wire::PERSONAL_SLOTS].fill(None);
            }
            wire::Cleared::ClassRoom => self.class_room = None,
            wire::Cleared::Leaving(field) => {
                let at = usize::from(field) * wire::PERSONAL_SLOTS;
                self.leaving[at..at + wire::PERSONAL_SLOTS].fill(None);
            }
            wire::Cleared::Objectives(field) => {
                let at = usize::from(field) * wire::OBJECTIVE_ROWS;
                self.goals[at..at + wire::OBJECTIVE_ROWS].fill(None);
            }
        }
        Ok(())
    }

    /// Takes one notice record's body: each comes once, in order. Messages,
    /// hints and voice-overs become the notices a local world's view makes of
    /// the script's own words (`script_notice`).
    pub fn notice(&mut self, body: &serde_json::Value, weapon: impl Fn(u16) -> Option<String>) -> Result<(), &'static str> {
        let notice = wire::Notice::read(body)?;
        if notice.sequence <= self.sequence { return Err(INVALID); }
        self.sequence = notice.sequence;
        let participant = |actor: wire::Actor| Participant { slot: actor.slot.into(), human: actor.human, class: Some(actor.class).filter(|id| *id != 0) };
        let value = match &notice.cue {
            // The authority sent it to whom it is for.
            wire::Cue::Text(key) => Notice::message(None, key),
            wire::Cue::Hint(key) => Notice::message(None, key),
            wire::Cue::Voice(sound) => Notice::voice(None, sound),
            // A kill names its weapon as a local view names it: one with a
            // killer names none (the feed names the killer).
            wire::Cue::Kill { killer, victim, weapon: id, gun } => Notice::Kill { killer: killer.map(participant), victim: participant(*victim),
                weapon: match gun {
                    // A ship's gun no one operates, as a local view names it.
                    Some(entity) => crate::local::mission_view::unmanned_gun_label(Some(entity.as_str()).filter(|e| !e.is_empty())),
                    None if killer.is_some() => Label::new(),
                    None => label(weapon(*id).as_deref().unwrap_or("")),
                } },
        };
        if self.notices.len() == NOTICES { self.notices.pop_front(); }
        self.notices.push_back(NoticeAt { sequence: notice.sequence, until: notice.until.into(), value });
        Ok(())
    }

    /// The match, once the authority has said it.
    pub fn score(&self, tick: u32) -> Option<MissionScore> {
        self.state.map(|state| MissionScore { script_tick: tick, teams: state.points, target: state.target, outcome: state.outcome,
            over: state.over, game_over: state.game_over, finished: state.finished, faulted: state.faulted })
    }

    pub(crate) fn points_shown(&self) -> [bool; 2] {
        self.state.map_or([false; 2], |state| state.points_shown)
    }

    /// Replicated critical system placed on `building_index`, if any (`0x0212`).
    pub(crate) fn system_for_building(&self, building_index: usize) -> Option<&System> {
        let target = u16::try_from(building_index).ok()?;
        self.systems
            .iter()
            .flatten()
            .find(|s| s.state.placed == wire::Placed::Building(target))
    }

    /// The HUD timer deadline at authority `tick`, as a local world's view
    /// makes it from the same timer (`hud_timer`).
    fn observed_timer(&self, over: bool) -> Option<(Label, u32)> {
        let (name, due) = match self.timer? {
            MissionTimer::Running { name, due } => (name, Some(due)),
            MissionTimer::Paused { name, .. } => (name, None),
        };
        hud_timer(name, due, over)
    }

    /// Fills `out` with per-team, per-class availability in admitted roster order
    /// (the same field `local::mission_view::History` populates locally).
    fn write_class_room(&self, out: &mut [Vec<bool>; 2]) {
        for (team, room) in out.iter_mut().enumerate() {
            room.clear();
            let classes = self.admission.teams[team].classes.len();
            match self.class_room {
                Some(cr) => {
                    let count = usize::from(cr.count[team]);
                    let bits = cr.available[team];
                    room.extend((0..classes).map(|i| i >= count || (bits & (1u16 << i)) != 0));
                }
                None => room.resize(classes, true),
            }
        }
    }

    /// What the battle's screens read of this moment, from the mission's
    /// replicated state and the battle's own records. None until the match
    /// state has arrived.
    pub fn observe(&mut self, receiver: &Receiver, hud: &impl Panel, replay: &Replay, dest: &mut Option<Box<Observation>>) {
        let tick = receiver.latest.max(0.) as u32;
        let (Some(state), Some(score)) = (self.state, self.score(tick)) else {
            *dest = None;
            return;
        };
        self.notices.retain(|notice| notice.until > tick);
        let out = dest.get_or_insert_with(|| Box::new(Observation::new(self.admission.clone(), score, 64)));
        out.admission.clone_from(&self.admission);
        (out.tick, out.score) = (tick, score);
        (out.show_points, out.map_hidden, out.posts_hidden) = (state.points_shown, state.minimap_hidden, state.posts_hidden);
        out.reinforcements = state.reinforcements.map(|tickets| tickets.map(|n| i32::try_from(n).unwrap_or(i32::MAX)));
        out.timer = self.observed_timer(state.over);

        // Deploying: he may choose while the released spawn summary has him
        // neither alive nor dying, and deploys when his place's row is due.
        let own = hud.summary();
        let waits = receiver.roster.local.and_then(|slot| *self.deployments.get(slot)?);
        out.choose_team = receiver.team_select;
        out.deploy_ready = own.valid && !own.ended && !state.over && matches!(own.phase, 0 | 3);
        out.deploying = out.deploy_ready && waits.is_some_and(|row| row.chosen);
        out.wave = waits.map(|row| row.due).filter(|due| *due > tick && !state.over);
        self.write_class_room(&mut out.class_room);

        out.roster.clear();
        for (slot, row) in receiver.roster.rows() {
            out.roster.resize(slot + 1, None);
            out.roster[slot] = Some(Participant { slot, human: row.human, class: self.classes.get(slot).copied().flatten() });
        }
        let team = if receiver.own.team != 0 { receiver.own.team } else { own.team };
        out.local_character = receiver.roster.local.filter(|_| matches!(team, 1 | 2)).map(|slot| (slot, team));
        out.personal.clear();
        out.personal.extend(self.personal.iter().enumerate().filter_map(|(slot, points)| Some((slot, (*points)?))));
        out.objectives.clear();
        out.objectives.extend(self.goals.iter().enumerate().filter_map(|(id, goal)| Some((id, (*goal)?))).filter(|(_, goal)| goal.active)
            .map(|(id, goal)| Objective { id: id as u32, team: goal.team.into(), key: goal.key, complete: goal.completed }));
        out.systems.clear();
        out.systems.extend(self.systems.iter().flatten());
        out.notices.clear();
        out.notices.extend(self.notices.iter());

        self.bodies(receiver, hud, replay, tick, out);
        self.slots(receiver, hud, replay, tick, out);
        self.mark_hits(receiver, tick);
        out.hit_marker = self.hit;
        self.lock(receiver, hud, tick, out);
        self.set_own_leaving(self.own_soldier_leaving(receiver, tick));
        out.leaving = leaving(receiver.own.alive, self.own_leaving, receiver.space_online.as_deref().and_then(|space| space.own_body.as_ref()));
    }

    // The pose a life was last seen at, remembered by its id alone. The id
    // is read from the network, so storage is sized by the most bodies a
    // battle holds, never by the id: one past that is shown untracked.
    fn track_pose(&mut self, life: LifeId, tick: u32, now: Pose) -> Pose {
        let Some(last) = self.poses.get_mut(life.slot as usize) else { return now };
        let before = last.filter(|(known, at, _)| *known == life && *at < tick).map_or(now, |(_, _, pose)| pose);
        *last = Some((life, tick, now));
        before
    }

    // Every soldier the snapshots show, and the player's own from his own
    // correction. A soldier in a seat, a vehicle's or a Space craft's (the
    // craft's field lists its crew), has no body on the screens.
    fn bodies(&mut self, receiver: &Receiver, hud: &impl Panel, replay: &Replay, tick: u32, out: &mut Observation) {
        out.bodies.clear();
        for row in receiver.active().filter(|row| row[r::KIND] == f64::from(k::SOLDIER) && row[r::SEAT_PRESENT] == 0.
            && !receiver.is_space_seated(row[r::ID] as u32)) {
            let (slot, generation) = (row[r::ID] as u32, row[r::GENERATION] as u32);
            let own = row[r::ID] == receiver.own.id && row[r::GENERATION] == receiver.own.generation;
            // His own life is no body in the world while he chooses where to
            // deploy: before his first life (phase 0, unspawned) and once his
            // body is gone and he waits to deploy again (phase 3, awaiting).
            // Dying (2), his body lies where he fell.
            if own && !receiver.own.alive && hud.summary().valid && matches!(hud.summary().phase, 0 | 3) { continue; }
            let class = row[r::CLASS_ID] as usize;
            let max_health = hud.health(class) as f32;
            let display = receiver.swbf2_display(slot, generation);
            let position = match (&replay.displayed, own) {
                (Some(body), true) => body.position,
                _ => std::array::from_fn(|i| row[r::POSITION + i]),
            };
            let now = Pose {
                position: [position[0] as f32, position[2] as f32],
                yaw: if own { replay.view[0] as f32 } else { row[r::YAW] as f32 },
                health: if own { receiver.own.health as f32 } else { row[r::HEALTH] as f32 },
                // His own as he predicts it; another's display says his share.
                energy: match replay.swbf2_energy(class).filter(|_| own) {
                    Some(energy) => energy as f32,
                    None => display.map_or(0., |display| display.energy as f32),
                },
            };
            let life = LifeId { slot, slab: 0, generation };
            let before = self.track_pose(life, tick, now);
            out.bodies.push(Body { life, team: row[r::TEAM] as u8, human: own, alive: if own { receiver.own.alive } else { row[r::DEAD] == 0. && row[r::HEALTH] > 0. },
                awarded: false, before, now, max_health, max_energy: 1., guarding: false });
        }
    }

    /// His own soldier's leaving countdown at `tick`: the authority's leaving
    /// state for his roster slot.
    fn own_soldier_leaving(&self, receiver: &Receiver, tick: u32) -> Option<f32> {
        self.leaving_left(receiver.roster.local?, f64::from(tick))
    }

    // The player's own hits, as the authority feeds them back to him: a
    // damage record (`hud_feed::decode_for`, the released HUD's hit mark with
    // the 2005 critical bit) naming his life the attacker of another. A local
    // view marks the same hits from the world's contacts. A new life starts
    // unmarked, and a mark goes after half a second, a critical one's after a
    // second and a half.
    fn mark_hits(&mut self, receiver: &Receiver, tick: u32) {
        let life = (receiver.own.id as u32, receiver.own.generation as u32);
        if self.hit_life != Some(life) {
            (self.hit_life, self.hit) = (Some(life), None);
        }
        for field in receiver.publications.iter().filter(|f| f.row[r::KIND] == f64::from(k::COMBAT) && f.row[r::SCHEMA] == 3.) {
            let at = field.row[r::TICK];
            let Some((event, critical)) = super::hud_feed::decode_for(&field.bytes[..field.length], at, receiver.control_family) else { continue };
            // A combat record carries its capture's clock, the tick before the
            // step that made the hit; the marker starts on the step's own, as
            // a local view's does from the world's contact.
            if receiver.own.alive && event.attacker == life && event.target != life {
                self.hit = Some(((at.max(0.) as u32).saturating_add(1), critical));
            }
        }
        if self.hit.is_some_and(|(at, critical)| tick.saturating_sub(at) > if critical { 90 } else { 30 }) {
            self.hit = None;
        }
    }

    // A hero's power lock, as his own correction names its target, drawn on
    // that soldier's replicated row as a local view draws it on the soldier:
    // the blast sphere of his posture above his feet, his health and his
    // class's most.
    fn lock(&mut self, receiver: &Receiver, hud: &impl Panel, tick: u32, out: &mut Observation) {
        use crate::local::mission_view::{next_lock_before, Locked, LockedPose};
        out.locked = receiver.own.power_lock.filter(|_| receiver.own.alive).and_then(|(slot, life)| {
            let row = receiver.active().find(|row| row[r::KIND] == f64::from(k::SOLDIER)
                && row[r::ID] == f64::from(slot) && row[r::GENERATION] == f64::from(life))?;
            let posture = bfe_sim::gen::Posture::from_u8(row[r::POSTURE] as u8).unwrap_or(bfe_sim::gen::Posture::Stand);
            let [middle, radius, _] = bfe_sim::combat::combat::blast_body(posture);
            let now = LockedPose {
                center: [row[r::POSITION], row[r::POSITION + 1] + middle, row[r::POSITION + 2]],
                radius,
                health: row[r::HEALTH] as f32,
            };
            let life = LifeId { slot: u32::from(slot), slab: 0, generation: life };
            let before = next_lock_before(&mut self.previous_lock, life, tick, now);
            let class = row[r::CLASS_ID] as usize;
            Some(Locked {
                life,
                team: row[r::TEAM] as u8,
                participant: hud.unit_slot(life.slot, life.generation).and_then(|at| out.roster.get(at).copied().flatten()),
                class: u16::try_from(class).ok().filter(|id| *id != 0),
                before,
                now,
                max_health: hud.health(class) as f32,
            })
        });
        if out.locked.is_none() {
            self.previous_lock = None;
        }
    }

    // The weapons in the player's hands, from his predicted arsenal.
    fn slots(&mut self, receiver: &Receiver, hud: &impl Panel, replay: &Replay, tick: u32, out: &mut Observation) {
        let old = out.slots;
        out.slots = [None; 2];
        if !receiver.own.alive { self.held = [None; 2]; return; }
        let selected = replay.selected();
        let mut counts = [0usize; 2];
        for weapon in replay.weapons() {
            let Some((loadout, channel)) = hud.loadout(weapon.def.0) else { continue };
            let chosen = counts[channel] as f64 == selected[channel];
            counts[channel] += 1;
            if !chosen { continue; }
            let changed = match self.held[channel] {
                Some((id, since)) if id == weapon.def.0 => since,
                _ => tick,
            };
            self.held[channel] = Some((weapon.def.0, changed));
            let unlimited = loadout.rounds <= 0. || matches!(loadout.base.as_str(), "melee" | "meleethrow");
            let count = |share: f64| (!unlimited && share.is_finite()).then(|| (share * loadout.rounds.max(1.) + 0.5).floor() as u32);
            let ready_in = if weapon.reload_left > weapon.wait { weapon.reload_left } else { weapon.wait };
            // The clip's share is the predicted arsenal's, under a local view's
            // rule. The heat and refire gauges also need the weapon's own heat
            // and timing values, which the online loadout table does not hold:
            // they stay empty online.
            out.slots[channel] = Some(Slot { id: weapon.def.0, name: loadout.name, base: loadout.base, power: loadout.power, icon: loadout.icon,
                icon_fallback: Default::default(),
                reticle: loadout.reticle, rounds: count(weapon.clip), reserve: if weapon.bottomless { None } else { count(weapon.reserve) },
                charge: weapon.charging.then_some(weapon.charge as f32), changed,
                ready: (ready_in.is_finite() && ready_in > 0.).then(|| tick.saturating_add((ready_in * 60.).ceil() as u32)),
                clip: (!unlimited && !weapon.bottomless && weapon.clip.is_finite()).then(|| {
                    // The gauge moves from the share it showed last for the same weapon.
                    let now = weapon.clip.clamp(0., 1.) as f32;
                    (old[channel].filter(|slot| slot.id == weapon.def.0).and_then(|slot| slot.clip).map_or(now, |(_, last)| last), now)
                }),
                heat: None, overheated: false, refire: None });
        }
    }
}

/// Whether the player is shown leaving the battlefield: alive, and his soldier
/// is (`soldier`, his countdown as the authority says it), or the craft he sits
/// in (any seat) is outside: its out-of-bounds timer runs, as his own Space
/// body carries it.
fn leaving(alive: bool, soldier: Option<f32>, own_body: Option<&bfe_sim::wire::space::OwnSpaceBody>) -> bool {
    use bfe_sim::wire::space::OwnSpaceBody;
    alive && (soldier.is_some() || own_body.is_some_and(|body| matches!(body, OwnSpaceBody::Pilot(own) if own.oob_timer >= 0.)))
}

#[cfg(test)]
mod tests;
