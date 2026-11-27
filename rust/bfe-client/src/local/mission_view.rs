//! Owned observations crossing from the mission thread to the battle window.
use arrayvec::ArrayString;
use bfe_sim::{
    battle::script::{state::ScriptNotification, MissionScore},
    units::infantry::contact::LifeId,
    world::owner::World,
};
use std::sync::Arc;

pub(crate) type Label = ArrayString<192>;
pub(crate) fn label(value: &str) -> Label {
    let mut n = value.len().min(Label::new().capacity());
    while !value.is_char_boundary(n) {
        n -= 1;
    }
    Label::from(&value[..n]).unwrap()
}

fn push_key(out: &mut Label, prefix: &str, class: &str, splits: usize) {
    if class.is_empty() {
        return;
    }
    if !out.is_empty() && out.try_push('|').is_err() {
        return;
    }
    if out.try_push_str(prefix).is_err() {
        return;
    }
    let mut replaced = 0usize;
    for ch in class.chars() {
        let next = if ch == '_' && replaced < splits {
            replaced += 1;
            '.'
        } else {
            ch
        };
        if out.try_push(next).is_err() {
            break;
        }
    }
}

/// The game names any unmanned gun that kills by one string of its own (English "Auto Turret").
/// The prepared language tables hold that entry only by its identifier, no readable key hashes
/// to it, and no turret class names it, so the identifier is the one way to reach it.
pub(crate) const AUTO_TURRET_KEY: &str = "664ffe37";

fn unmanned_turret_keys(
    has_killer: bool,
    weapon: Option<bfe_sim::units::infantry::contact::Weapon>,
    entity: Option<&str>,
) -> Label {
    let mut out = Label::new();
    if has_killer
        || !matches!(
            weapon,
            Some(bfe_sim::units::infantry::contact::Weapon::SpaceBuilding(_))
        )
    {
        return out;
    }
    unmanned_gun_label(entity)
}

/// How the feed names a kill by a ship's gun no one operates: the game's
/// "Auto Turret" string, then the gun building's entity class.
pub(crate) fn unmanned_gun_label(entity: Option<&str>) -> Label {
    let mut out = Label::new();
    let _ = out.try_push_str(AUTO_TURRET_KEY);
    if let Some(e) = entity {
        push_key(&mut out, "entity.", e, 1);
    }
    out
}

/// The deadline the HUD's timer counts to, made in this one place for the
/// local world's view and the replicated one: a running timer's due tick. A
/// stopped timer shows nothing, and once the match is over the clock stands
/// still with the rules and none is shown.
pub(crate) fn hud_timer(name: Label, due: Option<u32>, over: bool) -> Option<(Label, u32)> {
    if over {
        return None;
    }
    due.map(|due| (name, due))
}

#[derive(Default)]
pub(crate) struct TeamInfo {
    pub name: String,
    pub icon: String,
    pub classes: Vec<Arc<str>>,
}

pub(crate) struct Admission {
    pub teams: [TeamInfo; 2],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Pose {
    pub position: [f32; 2],
    pub yaw: f32,
    pub health: f32,
    pub energy: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct Body {
    pub life: LifeId,
    pub team: u8,
    pub human: bool,
    pub alive: bool,
    pub awarded: bool,
    pub before: Pose,
    pub now: Pose,
    pub max_health: f32,
    pub max_energy: f32,
    pub guarding: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub id: u32,
    pub name: Label,
    pub base: Label,
    pub power: bool,
    pub icon: Label,
    pub icon_fallback: Label,
    pub reticle: Label,
    pub rounds: Option<u32>,
    pub reserve: Option<u32>,
    pub charge: Option<f32>,
    pub changed: u32,
    pub ready: Option<u32>,
    pub clip: Option<(f32, f32)>,
    pub heat: Option<(f32, f32)>,
    pub overheated: bool,
    pub refire: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct LockedPose {
    pub center: [f64; 3],
    pub radius: f64,
    pub health: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct Locked {
    pub life: LifeId,
    pub team: u8,
    pub participant: Option<Participant>,
    pub class: Option<u16>,
    pub before: LockedPose,
    pub now: LockedPose,
    pub max_health: f32,
}

pub(crate) fn next_lock_before(
    previous: &mut Option<(LifeId, u32, LockedPose)>,
    life: LifeId,
    tick: u32,
    now: LockedPose,
) -> LockedPose {
    let before = previous
        .filter(|(prev_life, prev_tick, _)| {
            *prev_life == life && prev_tick.wrapping_add(1) == tick
        })
        .map_or(now, |(_, _, pose)| pose);
    *previous = Some((life, tick, now));
    before
}

#[derive(Clone, Copy)]
pub(crate) struct Objective {
    pub id: u32,
    pub team: i32,
    pub key: Label,
    pub complete: bool,
}

/// A critical system the mission named, as the authority last said it.
#[derive(Clone, Copy)]
pub(crate) struct System {
    pub name: Label,
    pub state: bfe_sim::wire::mission::System<'static>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Participant {
    pub slot: usize,
    pub human: bool,
    pub class: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Notice {
    Message {
        team: Option<i32>,
        key: Label,
    },
    Kill {
        killer: Option<Participant>,
        victim: Participant,
        weapon: Label,
    },
    /// A voice-over the script asks for: the screens draw nothing for it, and
    /// the battle's sound plays it once (`VoicesHeard`).
    Voice {
        team: Option<i32>,
        sound: Label,
    },
}

impl Notice {
    /// A message the script shows (`ShowMessageText`) or a mission hint
    /// (`AddMissionHint`, no team): one line on the screens.
    pub(crate) fn message(team: Option<i32>, key: &str) -> Self {
        Self::Message { team, key: label(key) }
    }
    pub(crate) fn voice(team: Option<i32>, sound: &str) -> Self {
        Self::Voice { team, sound: label(sound) }
    }
}

/// The notice the script's own words make, in one place for the local world's
/// view and the replicated one: a message and a hint are lines on the screens,
/// a voice-over is a sound. A text the wire could not carry (empty, or past its
/// 96 bytes) makes none.
pub(crate) fn script_notice(notification: &ScriptNotification) -> Option<Notice> {
    let (text, notice): (&str, fn(Option<i32>, &str) -> Notice) = match notification {
        ScriptNotification::ShowMessageText { key, .. } => (key, Notice::message),
        ScriptNotification::AddMissionHint { key } => (key, Notice::message),
        ScriptNotification::BroadcastVoiceOver { sound_id, .. } => (sound_id, Notice::voice),
        _ => return None,
    };
    let team = match notification {
        // A hint is for everyone.
        ScriptNotification::ShowMessageText { team, .. } | ScriptNotification::BroadcastVoiceOver { team, .. } => *team,
        _ => None,
    };
    (1..=bfe_sim::wire::mission::MAX_TEXT).contains(&text.len()).then(|| notice(team, text))
}

/// Which mission voice-overs a battle's sound has played: each once, in the
/// order of the notices, for a view that may be shown many times.
#[derive(Default)]
pub(crate) struct VoicesHeard {
    sequence: u32,
    tick: u32,
}

impl VoicesHeard {
    /// Hands each voice-over of `view` that is still worth playing and has not
    /// been played to `play`, with the team it is for (none: everyone).
    pub(crate) fn take(&mut self, view: &Observation, mut play: impl FnMut(&str, Option<i32>)) {
        // A clock gone back is another battle's: its notices count again from one.
        if view.tick < self.tick {
            self.sequence = 0;
        }
        self.tick = view.tick;
        let heard = self.sequence;
        for notice in &view.notices {
            self.sequence = self.sequence.max(notice.sequence);
            if let Notice::Voice { team, sound } = notice.value {
                if notice.sequence > heard && notice.until > view.tick {
                    play(sound.as_str(), team);
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct NoticeAt {
    pub sequence: u32,
    pub until: u32,
    pub value: Notice,
}

pub struct Observation {
    pub(crate) admission: Arc<Admission>,
    pub(crate) tick: u32,
    pub(crate) score: MissionScore,
    pub(crate) show_points: [bool; 2],
    pub(crate) reinforcements: [Option<i32>; 2],
    pub(crate) map_hidden: bool,
    pub(crate) posts_hidden: bool,
    pub(crate) wave: Option<u32>,
    pub(crate) choose_team: bool,
    pub(crate) deploy_ready: bool,
    /// The player has chosen what to deploy as and waits for a body.
    pub(crate) deploying: bool,
    /// Per team, per admitted class (in `admission` order): whether a human could take it now.
    pub(crate) class_room: [Vec<bool>; 2],
    pub(crate) timer: Option<(Label, u32)>,
    pub(crate) bodies: Vec<Body>,
    pub(crate) roster: Vec<Option<Participant>>,
    pub(crate) local_character: Option<(usize, u8)>,
    /// What the local player controls is outside the battlefield (his leaving countdown runs).
    pub(crate) leaving: bool,
    pub(crate) locked: Option<Locked>,
    pub(crate) hit_marker: Option<(u32, bool)>,
    pub(crate) slots: [Option<Slot>; 2],
    pub(crate) personal: Vec<(usize, i32)>,
    pub(crate) objectives: Vec<Objective>,
    pub(crate) systems: Vec<System>,
    pub(crate) notices: Vec<NoticeAt>,
}

impl Observation {
    /// Whether the admitted class of team 1 or 2 has room for a player now;
    /// None for a class the observation does not list.
    pub fn class_has_room(&self, team: usize, class: &str) -> Option<bool> {
        let at = self
            .admission
            .teams
            .get(team.checked_sub(1)?)?
            .classes
            .iter()
            .position(|c| &**c == class)?;
        self.class_room[team - 1].get(at).copied()
    }

    pub(crate) fn new(admission: Arc<Admission>, score: MissionScore, capacity: usize) -> Self {
        Self {
            admission,
            tick: 0,
            score,
            show_points: [false; 2],
            reinforcements: [None; 2],
            choose_team: false,
            deploy_ready: false,
            deploying: false,
            class_room: [Vec::with_capacity(32), Vec::with_capacity(32)],
            wave: None,
            timer: None,
            map_hidden: false,
            posts_hidden: false,
            bodies: Vec::with_capacity(capacity),
            roster: Vec::with_capacity(capacity),
            local_character: None,
            leaving: false,
            locked: None,
            hit_marker: None,
            slots: [None; 2],
            personal: Vec::with_capacity(capacity),
            objectives: Vec::with_capacity(bfe_sim::battle::script::state::MAX_OBJECTIVES),
            systems: Vec::new(),
            notices: Vec::with_capacity(32),
        }
    }

    pub(crate) fn copy_into(&self, dest: &mut Option<Box<Self>>) {
        let out = dest.get_or_insert_with(|| {
            Box::new(Self::new(
                self.admission.clone(),
                self.score,
                self.bodies.capacity(),
            ))
        });
        out.admission.clone_from(&self.admission);
        out.tick = self.tick;
        out.score = self.score;
        out.show_points = self.show_points;
        out.reinforcements = self.reinforcements;
        out.wave = self.wave;
        out.timer = self.timer;
        out.deploy_ready = self.deploy_ready;
        out.deploying = self.deploying;
        for (to, from) in out.class_room.iter_mut().zip(&self.class_room) {
            to.clone_from(from);
        }
        out.choose_team = self.choose_team;
        out.map_hidden = self.map_hidden;
        out.posts_hidden = self.posts_hidden;
        out.slots = self.slots;
        out.bodies.clone_from(&self.bodies);
        out.roster.clone_from(&self.roster);
        out.local_character = self.local_character;
        out.leaving = self.leaving;
        out.locked = self.locked;
        out.hit_marker = self.hit_marker;
        out.personal.clone_from(&self.personal);
        out.objectives.clone_from(&self.objectives);
        out.systems.clone_from(&self.systems);
        out.notices.clone_from(&self.notices);
    }
}

/// The last complete tick is retained even when the window skips publications.
pub(crate) struct History {
    pub current: Observation,
    previous: Vec<Option<(LifeId, u32, Pose, bool)>>,
    previous_lock: Option<(LifeId, u32, LockedPose)>,
    local_life: Option<LifeId>,
    hit_cursor: u32,
    delivered: u32,
    sequence: u32,
    sampled: bool,
}

impl History {
    pub fn admit(world: &World) -> Option<Self> {
        let view = world.mission_view()?;
        let teams = [1, 2].map(|id| {
            view.host
                .setup
                .teams
                .get(&id)
                .map(|t| TeamInfo {
                    name: t.name.clone().unwrap_or_default(),
                    icon: t.icons.first().cloned().unwrap_or_default(),
                    classes: t
                        .classes
                        .iter()
                        .map(|c| Arc::from(c.class_name.as_str()))
                        .collect(),
                })
                .unwrap_or_default()
        });
        let capacity = world.screen_unit_capacity();
        Some(Self {
            current: Observation::new(Arc::new(Admission { teams }), view.score(), capacity),
            previous: vec![None; capacity],
            previous_lock: None,
            local_life: None,
            hit_cursor: 0,
            delivered: 0,
            sequence: 0,
            sampled: false,
        })
    }

    pub fn sample(&mut self, world: &World, tick: u32) {
        if self.sampled && self.current.tick == tick {
            return;
        }
        let Some(view) = world.mission_view() else {
            return;
        };
        self.sampled = true;
        let out = &mut self.current;
        out.tick = tick;
        out.score = view.score();
        out.show_points = [
            view.host.team_points_visible[1],
            view.host.team_points_visible[2],
        ];
        out.reinforcements = [1., 2.].map(|i| {
            view.battle
                .reinforcements
                .get(i)
                .copied()
                .filter(|v| v.is_finite() && *v >= 0.)
                .map(|v| v as i32)
        });
        out.map_hidden = view.host.minimap_disabled;
        out.posts_hidden = view.host.command_posts_hidden;
        out.choose_team = view.human_spawn.can_switch_teams;
        out.deploy_ready = world.mission_deployment_choice().is_some();
        out.deploying = world.local_deployment_chosen();
        out.wave = world.local_spawn_due(tick);
        let admission = out.admission.clone();
        for (team, room) in out.class_room.iter_mut().enumerate() {
            room.clear();
            room.extend(admission.teams[team].classes.iter().map(|class| {
                world.mission_class_has_room(team + 1, class).unwrap_or(true)
            }));
        }
        out.timer = view.shown_timer(tick).and_then(|shown| {
            let name = label(std::str::from_utf8(shown.name).ok()?);
            hud_timer(name, shown.due, view.score().over)
        });
        out.bodies.clear();
        let old_slots = out.slots;
        out.slots = [None; 2];
        world.visit_screen_soldiers(|b| {
            let now = Pose {
                position: [b.position[0] as f32, b.position[2] as f32],
                yaw: b.yaw as f32,
                health: b.health as f32,
                energy: b.energy as f32,
            };
            let Some(slot) = self.previous.get_mut(b.life.slot as usize) else {
                return;
            };
            let before = slot
                .filter(|(life, t, _, alive)| {
                    *life == b.life && t.checked_add(1) == Some(tick) && *alive == b.alive
                })
                .map_or(now, |(_, _, p, _)| p);
            *slot = Some((b.life, tick, now, b.alive));
            out.bodies.push(Body {
                life: b.life,
                team: b.team,
                human: b.human,
                alive: b.alive,
                awarded: b.awarded,
                before,
                now,
                max_health: b.max_health as f32,
                max_energy: b.max_energy as f32,
                guarding: b.guarding,
            });
            if b.human {
                let same_life = self.local_life == Some(b.life);
                out.slots = std::array::from_fn(|i| {
                    b.slots[i].map(|w| {
                        let prev = old_slots[i].filter(|old| old.id == w.id && same_life);
                        Slot {
                            id: w.id,
                            name: label(w.name),
                            base: label(w.base),
                            power: w.power,
                            icon: label(w.icon),
                            icon_fallback: label(w.icon_fallback),
                            reticle: label(w.reticle),
                            changed: prev.map_or(tick, |old| old.changed),
                            ready: (w.ready_in.is_finite() && w.ready_in > 0.)
                                .then(|| tick.saturating_add((w.ready_in * 60.).ceil() as u32)),
                            rounds: w.rounds,
                            reserve: w.reserve,
                            charge: w.charge,
                            clip: w.clip.map(|now| (prev.and_then(|p| p.clip).map_or(now, |(_, c)| c), now)),
                            heat: w.heat.map(|now| (prev.and_then(|p| p.heat).map_or(now, |(_, c)| c), now)),
                            overheated: w.overheated,
                            refire: w.refire.map(|now| (prev.and_then(|p| p.refire).map_or(now, |(_, c)| c), now)),
                        }
                    })
                });
                if !same_life {
                    out.hit_marker = None;
                }
                self.local_life = Some(b.life);
            }
        });
        if let (Some(contacts), Some(local)) = (world.infantry_contacts(), self.local_life) {
            use bfe_sim::units::infantry::contact::Outcome;
            for event in contacts.since(self.hit_cursor) {
                if event.outcome == Outcome::Hit
                    && event.source == Some(local)
                    && event.target != local
                    && event.after < event.before
                {
                    out.hit_marker = Some((event.tick, event.critical));
                }
            }
            self.hit_cursor = contacts.latest;
        }
        if let Some((hit_tick, critical)) = out.hit_marker {
            let max_ticks = if critical { 90 } else { 30 };
            if tick.saturating_sub(hit_tick) > max_ticks {
                out.hit_marker = None;
            }
        }
        out.roster.clear();
        out.local_character = world.screen_local_character();
        out.leaving = world.screen_local_leaving();
        out.roster.resize(view.battle.characters.len(), None);
        world.visit_screen_roster(|slot, human, class| {
            out.roster[slot] = Some(Participant { slot, human, class });
        });
        out.locked = world.screen_local_lock().map(|t| {
            let now = LockedPose {
                center: t.center,
                radius: t.radius,
                health: t.health as f32,
            };
            let before = next_lock_before(&mut self.previous_lock, t.life, tick as u32, now);
            let participant = world
                .screen_character(t.life)
                .and_then(|slot| out.roster.get(slot).copied().flatten());
            Locked {
                life: t.life,
                team: t.team,
                participant,
                class: t.class,
                before,
                now,
                max_health: t.max_health as f32,
            }
        });
        if out.locked.is_none() {
            self.previous_lock = None;
        }
        out.personal.clear();
        for row in view.rules.personal_points() {
            if let Some(id) = view
                .battle
                .characters
                .iter()
                .position(|c| *c == row.character)
            {
                out.personal.push((id, row.points));
            }
        }
        out.objectives.clear();
        out.objectives.extend(
            view.host
                .objectives
                .iter()
                .enumerate()
                .filter(|(_, o)| o.active)
                .map(|(i, o)| Objective {
                    id: i as u32,
                    team: o.team,
                    key: label(&o.text_key),
                    complete: o.completed,
                }),
        );
        out.systems.clear();
        world.visit_mission_systems(|system| out.systems.push(System { name: label(system.name), state: system.unnamed() }));
        out.notices.retain(|n| n.until > tick);
        let mut notice = |value: Notice| {
            if out.notices.len() == 32 {
                out.notices.remove(0);
            }
            self.sequence = self.sequence.saturating_add(1);
            out.notices.push(NoticeAt {
                sequence: self.sequence,
                until: tick.saturating_add(300),
                value,
            });
        };
        if let Some(messages) = view.host.notifications_after(self.delivered) {
            for message in messages {
                if let Some(value) = script_notice(&message.notification) {
                    notice(value);
                }
            }
        }
        self.delivered = view.host.last_notification_sequence();
        let participant = |life| out.roster.get(world.screen_character(life)?).copied().flatten();
        // The host lists the deaths it delivered on this rules tick. A death that
        // waited behind a script refusal arrives a tick late and still shows once.
        if view.host.tick == tick {
            for death in &view.host.deaths {
                if let Some(victim) = participant(death.victim) {
                    let killer = death.killer.and_then(participant);
                    notice(Notice::Kill {
                        killer,
                        victim,
                        weapon: unmanned_turret_keys(
                            killer.is_some(),
                            death.weapon,
                            death.weapon.and_then(|w| world.screen_weapon_entity(w)),
                        ),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{next_lock_before, unmanned_turret_keys, LockedPose};
    use crate::local::{
        launch::Prepared, mission_input::Frame, runtime::Runtime, thread::PublicationSlot,
    };
    use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
    use bfe_sim::{
        units::infantry::contact::LifeId,
        world::admit::{
            mission::{MissionPreferences, MissionSelection},
            Options,
        },
    };

    /// A mission may list as many objectives as its host allows: neither the battle's record of
    /// them nor the window's copy grows while a tick runs.
    #[test]
    fn a_missions_most_objectives_fit_without_growing() {
        use super::{Admission, Label, Objective, Observation};
        use bfe_sim::battle::script::{state::MAX_OBJECTIVES, MissionScore};
        let score = MissionScore {
            script_tick: 0,
            teams: [0; 2],
            target: None,
            outcome: None,
            over: false,
            game_over: false,
            finished: false,
            faulted: false,
        };
        let admission = std::sync::Arc::new(Admission { teams: Default::default() });
        let mut seen = Observation::new(admission, score, 4);
        let reserved = seen.objectives.capacity();
        assert!(reserved >= MAX_OBJECTIVES);
        seen.objectives.extend((0..MAX_OBJECTIVES as u32).map(|id| Objective {
            id,
            team: 1,
            key: Label::new(),
            complete: false,
        }));
        assert_eq!(seen.objectives.capacity(), reserved);
        let mut window = None;
        seen.copy_into(&mut window);
        let window = window.unwrap();
        assert_eq!((window.objectives.len(), window.objectives.capacity()), (MAX_OBJECTIVES, reserved));
    }

    #[test]
    fn locked_pose_interpolates_only_across_consecutive_ticks_of_same_life() {
        let mut previous = None;
        let first_life = LifeId {
            slot: 3,
            slab: 1,
            generation: 1,
        };
        let second_life = LifeId {
            slot: 5,
            slab: 1,
            generation: 1,
        };
        let p0 = LockedPose {
            center: [1.0, 1.0, 8.0],
            radius: 1.4,
            health: 100.0,
        };
        let p1 = LockedPose {
            center: [2.0, 1.0, 9.0],
            radius: 1.4,
            health: 90.0,
        };
        let p2 = LockedPose {
            center: [5.0, 1.0, 12.0],
            radius: 1.4,
            health: 80.0,
        };
        let b0 = next_lock_before(&mut previous, first_life, 10, p0);
        assert_eq!(b0.center, p0.center);
        let b1 = next_lock_before(&mut previous, first_life, 11, p1);
        assert_eq!(b1.center, p0.center);
        assert_eq!(b1.health, 100.0);
        // Non-consecutive tick resets interpolation.
        let b2 = next_lock_before(&mut previous, first_life, 13, p2);
        assert_eq!(b2.center, p2.center);
        // Different locked target on consecutive tick resets interpolation.
        let b3 = next_lock_before(&mut previous, second_life, 14, p0);
        assert_eq!(b3.center, p0.center);
        // Wrapping from u32::MAX to 0 is consecutive and keeps interpolation.
        let b_max = next_lock_before(&mut previous, first_life, u32::MAX, p0);
        assert_eq!(b_max.center, p0.center);
        let b_wrap = next_lock_before(&mut previous, first_life, 0, p1);
        assert_eq!(b_wrap.center, p0.center);
        assert_eq!(b_wrap.health, 100.0);
        // Non-consecutive jump across the u32 wrap resets interpolation.
        let _ = next_lock_before(&mut previous, first_life, u32::MAX, p0);
        let b_skip = next_lock_before(&mut previous, first_life, 1, p2);
        assert_eq!(b_skip.center, p2.center);
    }

    #[test]
    fn kill_feed_keys_name_only_unmanned_ship_turrets_without_allocation() {
        use bfe_sim::{store::DefId, units::infantry::contact::Weapon};
        // Manned kills emit no weapon key for infantry, hero, fighter, or turret-chair guns.
        for weapon in [
            None,
            Some(Weapon::MeleeBinding(17)),
            Some(Weapon::RangedClass(DefId(3))),
            Some(Weapon::Projectile(DefId(5))),
            Some(Weapon::SpaceBuilding(0)),
            Some(Weapon::SpaceFighter {
                profile: 1,
                weapon: 0,
            }),
        ] {
            assert_eq!(
                unmanned_turret_keys(true, weapon, Some("inv_bldg_deck_gun")).as_str(),
                ""
            );
        }
        // Unmanned non-building deaths also omit weapon keys.
        assert_eq!(unmanned_turret_keys(false, None, None).as_str(), "");
        assert_eq!(
            unmanned_turret_keys(
                false,
                Some(Weapon::SpaceFighter {
                    profile: 1,
                    weapon: 0,
                }),
                None,
            )
            .as_str(),
            ""
        );
        // Unmanned building turret uses the prepared automatic-turret id with entity fallback.
        assert_eq!(
            unmanned_turret_keys(false, Some(Weapon::SpaceBuilding(0)), None).as_str(),
            "664ffe37"
        );
        assert_eq!(
            unmanned_turret_keys(
                false,
                Some(Weapon::SpaceBuilding(0)),
                Some("inv_bldg_deck_gun"),
            )
            .as_str(),
            "664ffe37|entity.inv.bldg_deck_gun"
        );
        let long = format!("inv_bldg_{}", "a".repeat(256));
        assert_eq!(
            unmanned_turret_keys(false, Some(Weapon::SpaceBuilding(0)), Some(&long)).len(),
            192
        );
    }

    #[test]
    #[ignore = "needs prepared Hero Assault content"]
    fn live_spawn_screen_waits_for_the_player_and_shows_the_selected_equipment() {
        let root = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
        let family = bfe_family::GameFamily::Swbf2;
        let selection = MissionSelection {
            family,
            map: "tat2".into(),
            mode: "eli".into(),
            era: "gcw".into(),
            seed: 4242,
        };
        let options = Options {
            family,
            map: selection.map.clone(),
            mode: selection.mode.clone(),
            side: "all".into(),
            seed: 4242,
            ..Default::default()
        };
        let (mut prepared, _) = Prepared::developer_mission(
            &root,
            &options,
            &selection,
            MissionPreferences {
                bots_per_team: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        prepared.engine.use_mission_spawn_screen();
        let mut publication = PublicationSlot::new_preallocated();
        let mut world = Runtime::new_into_slot(
            *prepared.engine,
            *prepared.admission,
            CLOCK_START_MS,
            CLOCK_UNIX_MS,
            &mut publication,
        )
        .unwrap();
        let mut generation = 0.;
        for tick in 1..=1200 {
            if tick == 180 {
                let unit = std::env::var("BFE_MATCH_UNIT").expect("prepared saber class");
                world.spawn_human(1, &unit, "").unwrap();
            }
            Frame::default().submit(&mut world, generation).unwrap();
            world.step_into_slot(&mut publication).unwrap();
            generation = publication.infantry.map_or(0., |body| body.generation);
            let view = publication.mission_view.as_ref().unwrap();
            assert_eq!(view.tick, tick);
            if tick < 180 {
                assert!(view.bodies.iter().all(|b| !b.human || !b.alive));
                assert!(view.deploy_ready);
                continue;
            }
            if let Some(body) = view.bodies.iter().find(|b| b.human && b.alive) {
                assert!(body.now.health > 0. && body.max_energy > 0.);
                assert_eq!(
                    view.slots[0].as_ref().map(|s| s.base.as_str()),
                    Some("melee")
                );
                assert!(view.score.target.is_some_and(|target| target > 0));
                return;
            }
        }
        panic!("the selected hero never received a living body");
    }
}

#[cfg(test)]
mod timer_tests {
    use super::{hud_timer, label};

    // What the HUD's timer counts to: the due tick while it runs; nothing
    // while it is stopped and nothing once the match is over.
    #[test]
    fn the_hud_timer_counts_to_its_due_tick_while_it_runs() {
        let name = label("lantern");
        assert_eq!(hud_timer(name, Some(930), false), Some((name, 930)));
        assert_eq!(hud_timer(name, None, false), None);
        assert_eq!(hud_timer(name, Some(930), true), None);
    }
}
