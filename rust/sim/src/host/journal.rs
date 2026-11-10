//! The page's `SimEvents` journal (src/sim-events.js) with the records the
//! game's observers write into it: a ring of 4096 slots, a sequence number
//! and the simulation time the owner sets each tick. `worldState` reads the
//! ring's filled slots in slot order (`ring.filter(Boolean)`), and the AI's
//! hearing (`AiJournalFacts.capture`) reads the shots and damage since its
//! cursor. A record keeps its emitter's fields typed (`emit(kind, fields)`
//! with the field names in `Event`'s comments); strings are store ids, so a
//! record owns no heap and the ring never allocates after its creation.
use crate::gen::{F3WeaponAction, Posture};
use crate::store::StrId;

pub const CAPACITY: usize = 4096;

/// `units.ref(unit)` as a record keeps it (`{unit, generation}`, written
/// `{"id":slot,"generation":life}`): the unit's store slot and the life
/// the emitter named.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Life {
    pub id: u32,
    pub generation: f64,
}

/// A `presentation` record's event (`simEvents.emit('presentation',
/// {event})`), the value a unit's onPresentation snapshotted (src/game.js
/// instrument), in its key order; an absent action or posture is the
/// mirror's undefined, which JSON leaves out.
#[derive(Clone, Copy, Debug)]
pub enum Presentation {
    /// `{kind, action, bank, posture, prepare, ownerId, ownerGeneration}`.
    WeaponAction {
        action: Option<F3WeaponAction>,
        bank: StrId,
        posture: Option<Posture>,
        prepare: bool,
        owner: Life,
    },
    /// `{kind, action, ownerId, ownerGeneration}`.
    EndWeaponAction {
        action: Option<F3WeaponAction>,
        owner: Life,
    },
    /// `{kind, name, from, to, charge, channel?, ownerId, ownerGeneration}`.
    Lightning {
        name: StrId,
        from: [f64; 3],
        to: [f64; 3],
        charge: f64,
        channel: Option<f64>,
        owner: Life,
    },
    /// `hooks.presented(kind, unit, amount, source, hitKind, sample)`
    /// (src/game.js): `{kind, ownerId, ownerGeneration, amount, source,
    /// hitKind}`, the source a shield hit's or null.
    Presented {
        kind: Presented,
        owner: Life,
        amount: f64,
        source: Option<[f64; 3]>,
    },
    /// presentEffect (src/game.js): `{kind: 'effect', name, position}`; a
    /// melee deflection's onPresentation adds `ownerId, ownerGeneration`.
    Effect {
        name: StrId,
        position: [f64; 3],
        owner: Option<Life>,
    },
    /// A player's squad order (applyNetworkControls,
    /// src/runtime/multiplayer-world.js): `{kind: 'squad', ownerId,
    /// ownerGeneration, label}`, the label one of [`SQUAD_LABELS`].
    Squad { label: u32, owner: Life },
}

/// The squad orders' labels in the key table's order (squadMoveOut,
/// squadHold, squadFollow, squadAtEase), as the page's HUD names them.
pub const SQUAD_LABELS: [&str; 4] = ["moveout", "hold", "follow", "atease"];

impl Presentation {
    /// The unit life that owns the event (`ownerId`, `ownerGeneration`);
    /// presentEffect's effect has none.
    pub fn owner(&self) -> Option<Life> {
        match *self {
            Presentation::WeaponAction { owner, .. }
            | Presentation::EndWeaponAction { owner, .. }
            | Presentation::Lightning { owner, .. }
            | Presentation::Presented { owner, .. }
            | Presentation::Squad { owner, .. } => Some(owner),
            Presentation::Effect { owner, .. } => owner,
        }
    }
}

/// A `hooks.presented` kind and its `hitKind`: `'explosion'` for an
/// explosion's shield hit, else null (the parameter's default, which the
/// other shield hits' undefined argument takes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presented {
    DeathBlast,
    BlowApart,
    ShieldHit,
    ShieldHitExplosion,
}

impl Presented {
    /// The event's `kind`.
    pub fn name(self) -> &'static str {
        match self {
            Presented::DeathBlast => "deathBlast",
            Presented::BlowApart => "blowApart",
            Presented::ShieldHit | Presented::ShieldHitExplosion => "shieldHit",
        }
    }
}

/// One record's kind and fields, in the emitter's key order.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Recon(super::recon::Record), // matrix-b
    /// F4Tail.onSpawn (src/game.js): `spawn {target, team, position,
    /// className, post}`.
    Spawn {
        target: Life,
        team: f64,
        position: [f64; 3],
        class_name: StrId,
        post: Option<StrId>,
    },
    /// AiNotices.voice (src/sim/wasm/ai-world-notices.js): `vo {source,
    /// team, position, human, event, property}` and the notice's optional
    /// `command`, `context`, `positional`, `notice` when present. The
    /// record's time is the notice's.
    Vo {
        source: Life,
        team: f64,
        position: [f64; 3],
        human: bool,
        event: StrId,
        property: StrId,
        command: Option<f64>,
        context: Option<f64>,
        positional: Option<bool>,
        notice: Option<bool>,
    },
    /// unit.combat.onFired (src/game.js): `shot {source, team, position,
    /// direction, weapon, repair, melee, swing}`.
    Shot {
        source: Life,
        team: f64,
        position: [f64; 3],
        direction: [f64; 3],
        weapon: Option<StrId>,
        repair: bool,
        melee: bool,
        swing: f64,
    },
    /// mountedShot (src/game.js), a vehicle's or a turret's gun: `shot
    /// {source, mount, seat, team, position, direction, weapon, repair,
    /// melee, swing}`, the source null when the shot's owner had none.
    MountedShot {
        source: Option<Life>,
        mount: Life,
        seat: f64,
        team: f64,
        position: [f64; 3],
        direction: [f64; 3],
        weapon: Option<StrId>,
        repair: bool,
        melee: bool,
        swing: f64,
    },
    /// blastNotice (src/game.js): `explosion {position, team, source,
    /// radius}`, the source null when the blast's owner had none.
    Explosion {
        position: [f64; 3],
        team: f64,
        source: Option<Life>,
        radius: f64,
    },
    /// unit.combat.onDamaged (src/game.js instrument): `damage {target,
    /// team, position, amount, typed, generic, known, point, source, health,
    /// attacker, attackerTeam}`; `amount` is the hit's own amount and
    /// `health` what the body lost.
    Damage {
        target: Life,
        team: f64,
        position: [f64; 3],
        amount: f64,
        typed: f64,
        generic: f64,
        known: bool,
        point: Option<[f64; 3]>,
        source: Option<[f64; 3]>,
        health: f64,
        attacker: Option<Life>,
        attacker_team: f64,
    },
    /// `presentation {event}`.
    Presentation(Presentation),
    /// ordnance.onGrenadeContact (src/game.js): `grenade {position, team,
    /// source, def}`, `def` the round's class or null.
    Grenade {
        position: [f64; 3],
        team: f64,
        source: Option<Life>,
        def: Option<StrId>,
    },
    /// vehicles.onDamaged (src/game.js): `damage {target, team, position,
    /// amount, typed: 1, generic: 1, known: false, point: null, health:
    /// amount, attacker, attackerTeam}`, no source key; the target is the
    /// hull's registry target.
    HullDamage {
        target: Life,
        team: f64,
        position: [f64; 3],
        amount: f64,
        attacker: Option<Life>,
        attacker_team: f64,
    },
    /// a turret's onDamaged (src/game.js): `damage {target, team, position,
    /// amount, typed, generic, known, point, health, attacker,
    /// attackerTeam}`, `known` whether the hit had a direction, no source
    /// key; the target is the turret's registry target and the life the
    /// notice named (null when it named none).
    PropDamage {
        target: Option<(u32, Option<f64>)>,
        team: f64,
        position: [f64; 3],
        amount: f64,
        typed: f64,
        generic: f64,
        point: Option<[f64; 3]>,
        health: f64,
        attacker: Option<Life>,
        attacker_team: f64,
    },
    /// deathNotice (src/game.js): `death {target, team, position, attacker,
    /// attackerTeam}`.
    Death {
        target: Life,
        team: f64,
        position: [f64; 3],
        attacker: Option<Life>,
        attacker_team: f64,
    },
    /// vehicles/turretSeats.onBoard and onUnboard (src/game.js): `board` or
    /// `unboard {source, team, position, target, seat}`.
    Seat {
        boarded: bool,
        source: Option<Life>,
        team: f64,
        position: [f64; 3],
        target: Option<Life>,
        seat: f64,
    },
}

impl Event {
    /// The record's `kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Recon(_) => "remoteShot", // matrix-b
            Event::Spawn { .. } => "spawn",
            Event::Vo { .. } => "vo",
            Event::Shot { .. } | Event::MountedShot { .. } => "shot",
            Event::Explosion { .. } => "explosion",
            Event::Grenade { .. } => "grenade",
            Event::Damage { .. } | Event::HullDamage { .. } | Event::PropDamage { .. } => "damage",
            Event::Death { .. } => "death",
            Event::Presentation(_) => "presentation",
            Event::Seat { boarded: true, .. } => "board",
            Event::Seat { boarded: false, .. } => "unboard",
        }
    }
}

/// `{seq, time, kind, ...fields}`.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub seq: usize,
    pub time: f64,
    pub event: Event,
}

pub struct Journal {
    ring: Vec<Option<Record>>,
    /// The next record's sequence number.
    pub seq: usize,
    /// `simEvents.time`, as the owner set it before the emit.
    pub time: f64,
}

impl Default for Journal {
    fn default() -> Self {
        Self::new()
    }
}

impl Journal {
    pub fn new() -> Self {
        Self {
            ring: vec![None; CAPACITY],
            seq: 0,
            time: 0.0,
        }
    }

    /// `emit(kind, fields)` at the journal's time.
    pub fn emit(&mut self, event: Event) {
        self.emit_at(self.time, event);
    }

    /// `emit` with the record's own time (a voice notice's).
    pub fn emit_at(&mut self, time: f64, event: Event) {
        self.ring[self.seq % CAPACITY] = Some(Record {
            seq: self.seq,
            time,
            event,
        });
        self.seq += 1;
    }

    /// `AiJournalFacts.capture(journal)`: the shot and damage records since
    /// the cursor (at most the ring's capacity back), in sequence order,
    /// then the cursor at the head.
    pub fn capture(&self, cursor: &mut usize, mut heard: impl FnMut(&Record)) {
        let head = self.seq;
        if head < *cursor {
            *cursor = 0;
        }
        let start = (*cursor).max(head.saturating_sub(CAPACITY));
        for seq in start..head {
            if let Some(record) = &self.ring[seq % CAPACITY] {
                if record.seq == seq
                    && matches!(
                        record.event,
                        Event::Shot { .. }
                            | Event::MountedShot { .. }
                            | Event::Damage { .. }
                            | Event::HullDamage { .. }
                            | Event::PropDamage { .. }
                    )
                {
                    heard(record);
                }
            }
        }
        *cursor = head;
    }

    /// `g.simEvents.ring.filter(Boolean)`: every filled slot in slot order.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.ring.iter().flatten()
    }
}
