//! The unit phase's notifications (the `EVENTS` bank, `sim.events`) and
//! what the page did with each (`F3Sim.notifications()` in
//! src/sim/wasm/f3-sim.js, the owners' handlers, and the game's emitters in
//! src/game.js): the records they wrote into the SimEvents journal, the
//! wire observers they fed (the shot counters, the presentation capture)
//! and the calls they made back into the world. Ported one notification at
//! a time; a notification without a port here is left to the world as the
//! page would have left a notice nobody listened to.
use crate::abi::context::Context;
use crate::gen::{
    self, AiActor, AiVoiceNotice, Attacker, CbShotKind, F2SeatActionKind, F3BlastNotice,
    F3BlastStage, F3BodyEvent, F3BodyNotice, F3DamageNotice, F3DeathNotice, F3Effect,
    F3EffectNotice, F3HullEvent, F3HullNotice, F3OptionalState, F3OrdnanceEvent, F3OrdnanceNotice,
    F3PowerupEffect, F3PresentKind, F3Presentation, F3ProjectileKind, F3PropDamage, F3PropEffect,
    F3PropEffectKind, F3SeatNotice, F3ShieldNotice, F3ShotNotice, F3WeaponAction, F4Effect,
    F4Serve, F4SpawnNotice, FfShot, PbStateName, PickupArgs, PlayerSquad, Posture, Unit, UnitSplash,
    WbEncode,
    WireDynamicFact,
};
use crate::store::{Nullable, StrId};

use super::capture::{Capture, Token};
use super::journal::{Event, Journal, Life, Presentation, Presented, SQUAD_LABELS};

/// One tick's drain: the world's clock for the records, the observers'
/// handles, and the units' rows (`store.unit`) for the facts the page read
/// from its unit objects.
pub struct Drain<'a> {
    pub recon_definitions: &'a super::recon::Definitions, // matrix-b
    pub tick: f64,
    /// The presentation capture over the wire's combat capture, when
    /// the host publishes one.
    pub capture: Option<&'a Capture>,
    /// The content names the host's records carry (`super::Content`).
    pub content: &'a super::Content,
    pub journal: &'a mut Journal,
    /// Each network human's presses (`u.networkShots`) by his unit's
    /// slot, by button, as the host last mirrored them.
    pub presses: &'a [(u32, [Option<f64>; 2])],
    pub units: &'a [(u32, u32, Unit)],
    /// The seat leases (`KIND_SEAT_LEASE`), by slot.
    pub leases: &'a [(u32, gen::SeatLease)],
    /// The body flags the page toggled this tick (kernel slot, enabled),
    /// and each building's bodies (built, destroyed, unbuilt) as kernel slots.
    pub toggles: &'a mut Vec<(usize, u8)>,
    pub prop_bodies: &'a [(usize, [Option<u32>; 3])],
    /// `CombatCapture.projectiles`: each launched round's captured state
    /// (the JSON `capture_bolt` answered), by kind, slot and generation.
    pub projectiles: &'a mut Vec<((u8, u32, u32), String)>,
    /// `ProjectileCapture.active`: each dynamic round's entry, by kind,
    /// slot and generation, until its impact.
    pub dynamics: &'a mut Vec<((u8, u32, u32), WireDynamicFact)>,
}

impl Drain<'_> {
    /// A `presentation` record (`simEvents.emit('presentation', {event})`)
    /// and the presentation capture's record of its event, sent as is.
    fn present(&mut self, context: &Context, presentation: Presentation) -> Result<(), String> {
        self.journal.emit(Event::Presentation(presentation));
        // An effect has no owner; its record carries none.
        let owner = presentation.owner().unwrap_or_default();
        if let Some(capture) = self.capture {
            let text = |id: StrId| {
                context
                    .string(id)
                    .map(|units| String::from_utf16_lossy(&units))
                    .map_err(|e| e.message().to_string())
            };
            let action = |action: Option<F3WeaponAction>| {
                action.map_or(Token::Undefined, |a| {
                    Token::Text(F3WeaponAction::NAMES[a as usize])
                })
            };
            let owner_id = ("ownerId", Token::Number(f64::from(owner.id)));
            let owner_generation = ("ownerGeneration", Token::Number(owner.generation));
            match presentation {
                Presentation::WeaponAction {
                    action: a,
                    bank,
                    posture,
                    prepare,
                    ..
                } => {
                    let bank = text(bank)?;
                    let posture = posture.map_or(Token::Undefined, |p| {
                        Token::Text(Posture::NAMES[p as usize])
                    });
                    capture.emit(
                        context,
                        self.tick,
                        "weaponAction",
                        0.0,
                        &[
                            ("action", action(a)),
                            ("bank", Token::Text(&bank)),
                            ("posture", posture),
                            ("prepare", Token::Boolean(prepare)),
                            owner_id,
                            owner_generation,
                        ],
                    )?;
                }
                Presentation::EndWeaponAction { action: a, .. } => {
                    capture.emit(
                        context,
                        self.tick,
                        "endWeaponAction",
                        0.0,
                        &[("action", action(a)), owner_id, owner_generation],
                    )?;
                }
                Presentation::Lightning {
                    name,
                    from,
                    to,
                    charge,
                    channel,
                    ..
                } => {
                    let name = text(name)?;
                    let mut fields = vec![
                        ("name", Token::Text(&name)),
                        ("from", Token::Vector(Some(from))),
                        ("to", Token::Vector(Some(to))),
                        ("charge", Token::Number(charge)),
                    ];
                    if let Some(channel) = channel {
                        fields.push(("channel", Token::Number(channel)));
                    }
                    fields.push(owner_id);
                    fields.push(owner_generation);
                    capture.emit(context, self.tick, "lightning", 0.0, &fields)?;
                }
                Presentation::Effect {
                    name,
                    position,
                    owner: o,
                } => {
                    let name = text(name)?;
                    let fields = [
                        ("name", Token::Text(&name)),
                        ("position", Token::Vector(Some(position))),
                    ];
                    if o.is_some() {
                        let [name, position] = fields;
                        capture.emit(
                            context,
                            self.tick,
                            "effect",
                            0.0,
                            &[name, position, owner_id, owner_generation],
                        )?;
                    } else {
                        capture.emit(context, self.tick, "effect", 0.0, &fields)?;
                    }
                }
                Presentation::Presented {
                    kind,
                    amount,
                    source,
                    ..
                } => {
                    // `{kind, ownerId, ownerGeneration, amount, source,
                    // hitKind}`; the event writer reads a blowApart's amount.
                    let hit = match kind {
                        Presented::ShieldHitExplosion => Token::Text("explosion"),
                        // `hitKind=null` is the parameter's default, which
                        // an undefined argument takes.
                        Presented::ShieldHit | Presented::BlowApart | Presented::DeathBlast => {
                            Token::Null
                        }
                    };
                    let word = if kind == Presented::BlowApart {
                        amount
                    } else {
                        0.0
                    };
                    capture.emit(
                        context,
                        self.tick,
                        kind.name(),
                        word,
                        &[
                            owner_id,
                            owner_generation,
                            ("amount", Token::Number(amount)),
                            (
                                "source",
                                source.map_or(Token::Null, |v| Token::Vector(Some(v))),
                            ),
                            ("hitKind", hit),
                        ],
                    )?;
                }
                Presentation::Squad { label, .. } => {
                    let label = SQUAD_LABELS.get(label as usize).copied().unwrap_or("");
                    capture.emit(
                        context,
                        self.tick,
                        "squad",
                        0.0,
                        &[owner_id, owner_generation, ("label", Token::Text(label))],
                    )?;
                }
            }
        }
        Ok(())
    }

    /// CombatCapture.impact for a dynamic round: its entry marked terminal
    /// and handed to the dynamic capture again, then the terminal record
    /// (schema 2) from the entry (`capture_terminal`: the execution tick is
    /// the launch tick, the expiry 18 ticks on) at the hit's point, normal
    /// and surface. ProjectileCapture keeps the entry until the wire retires
    /// it; a round has one impact, so the host lets it go here.
    fn dynamic_terminal(
        &mut self,
        context: &Context,
        capture: &Capture,
        mut fact: WireDynamicFact,
        e: &F3OrdnanceNotice,
    ) -> Result<(), String> {
        fact.terminal = true;
        capture.dynamic(context, &fact)?;
        let encode = WbEncode {
            op: 0.0,
            id: fact.id,
            generation: fact.generation,
            tick: self.tick,
            schema: 2.0,
            expiry_tick: self.tick + 18.0,
            definition_id: fact.definition_id,
            owner_id: fact.owner_id,
            owner_generation: fact.owner_generation,
            fire_id: fact.fire_id,
            channel: fact.channel,
            launch_tick: fact.launch_tick,
            execution_tick: fact.launch_tick,
            position: e.point.to_vec(),
            direction: e.normal.to_vec(),
            // combatRecord's default offset ([0, 0, 0]).
            offset: vec![0.0; 3],
            position_array: true,
            direction_array: true,
            offset_array: true,
            surface: e.surface,
            ..WbEncode::default()
        };
        let payload = capture.encode(context, &encode)?;
        let view = super::capture::decode_view(self.tick, 2.0, &payload)?;
        capture.emit_record(context, self.tick, fact.id, 2.0, payload, &view)?;
        Ok(())
    }

    /// events.capture "shot"'s trigger identity: the body's press on the
    /// button of the weapon's channel (`u.networkShots`), 0 for a body with
    /// none (a bot's).
    fn press(&self, unit: u32, channel: f64) -> f64 {
        self.presses
            .iter()
            .find(|(slot, _)| *slot == unit)
            .map_or(0.0, |(_, presses)| {
                presses[usize::from(channel == 1.0)].unwrap_or(0.0)
            })
    }

    /// One pass over the bank's words from `offset`: every event decoded
    /// and handled; answers with the offset after the last event. `time`
    /// gives the journal clock for an event at a word offset
    /// (`unitsSim.eventClock`: the players' events carry the tick before).
    pub fn run(
        &mut self,
        context: &Context,
        words: &[f64],
        offset: usize,
        time: impl Fn(usize) -> f64,
    ) -> Result<usize, String> {
        let total = words.first().copied().unwrap_or(0.0) as usize;
        let mut at = 1 + offset;
        let end = 1 + total;
        while at + 2 <= end {
            let code = words[at] as u32;
            let length = words[at + 1] as usize;
            let record = &words[at + 2..(at + 2 + length).min(end)];
            self.journal.time = time(at - 1);
            // The page's capture observers ran inside try/catch (`observe`
            // in network-entity-combat.js, `guard` in entity-presentation-
            // events.js): a throw became a capture fault and the battle went
            // on. Without a capture nothing here can throw that way.
            if let Err(message) = self.handle(context, code, record) {
                match self.capture {
                    Some(capture) => capture.fault(context, &message)?,
                    None => return Err(message),
                }
            }
            at += 2 + length;
        }
        Ok(at - 1)
    }

    fn handle(&mut self, context: &Context, code: u32, record: &[f64]) -> Result<(), String> {
        let source = self.capture.map_or(0, |c| c.store_source); // matrix-b
        if (super::mount::Notifications {
            context,
            journal: self.journal,
            capture: self.capture,
            tick: self.tick,
            store_source: source,
            presses: self.presses,
        })
        .handle(code, record)?
        {
            return Ok(());
        } // matrix-b
        if (super::recon::Notifications {
            definitions: self.recon_definitions,
            context,
            journal: self.journal,
            capture: self.capture,
            store_source: source,
            tick: self.tick,
        })
        .handle(code, record)?
        {
            return Ok(());
        } // matrix-b

        match code {
            // applyNetworkControls (src/runtime/multiplayer-world.js): a
            // squad order the player gave is a `presentation` record of his
            // unit life, which his own page's HUD shows.
            gen::EVENT_PLAYER_SQUAD if record.len() >= PlayerSquad::WORDS => {
                let e = PlayerSquad::take(&record[..PlayerSquad::WORDS], &record[PlayerSquad::WORDS..]);
                let owner = Life {
                    id: e.unit,
                    generation: e.life,
                };
                self.present(context, Presentation::Squad { label: e.label, owner })?;
            }
            // F4Tail.onSpawn (src/game.js): simEvents.emit("spawn", {target:
            // units.ref(event.unit), team, position, className, post}).
            gen::EVENT_F4_SPAWN if record.len() >= F4SpawnNotice::WORDS => {
                let e = F4SpawnNotice::take(
                    &record[..F4SpawnNotice::WORDS],
                    &record[F4SpawnNotice::WORDS..],
                );
                self.journal.emit(Event::Spawn {
                    target: Life {
                        id: e.unit.slot,
                        generation: e.key.life,
                    },
                    team: e.team,
                    position: e.position,
                    class_name: e.class_name,
                    post: e.post,
                });
            }
            // AiNotices.voice (src/sim/wasm/ai-world-notices.js): the prepared
            // field shape for the notice's optional keys, `time` the notice's;
            // then the presentation capture's "vo" record.
            gen::EVENT_AI_VOICE if record.len() >= AiVoiceNotice::WORDS => {
                let e = AiVoiceNotice::take(
                    &record[..AiVoiceNotice::WORDS],
                    &record[AiVoiceNotice::WORDS..],
                );
                // A SeatUnit's id is its lease's unit's (`lease.unit?.id ?? 0`,
                // src/seats.js); the lease names its SeatUnit (`controllable`).
                let owner = match e.actor {
                    AiActor::Unit(unit) => unit.slot,
                    AiActor::SeatUnit(seat) => self
                        .leases
                        .iter()
                        .find(|(_, l)| l.controllable == Some(seat))
                        .and_then(|(_, l)| l.unit)
                        .map_or(0, |u| u.slot),
                };
                self.journal.emit_at(
                    e.time,
                    Event::Vo {
                        source: Life {
                            id: owner,
                            generation: e.generation,
                        },
                        team: e.team,
                        position: e.position,
                        human: e.human,
                        event: e.event,
                        property: e.property,
                        command: e.command,
                        context: e.context,
                        positional: e.positional,
                        notice: e.notice,
                    },
                );
                // events.capture "vo": kind, ownerId, ownerGeneration, team,
                // position, human, property, positional (true unless false).
                let property = context
                    .string(e.property)
                    .map_err(|e| e.message().to_string())?;
                let property = String::from_utf16_lossy(&property);
                if let Some(capture) = self.capture {
                    capture.emit(
                        context,
                        self.tick,
                        "vo",
                        0.0,
                        &[
                            ("ownerId", Token::Number(f64::from(owner))),
                            ("ownerGeneration", Token::Number(e.generation)),
                            ("team", Token::Number(e.team)),
                            ("position", Token::Vector(Some(e.position))),
                            ("human", Token::Boolean(e.human)),
                            ("property", Token::Text(&property)),
                            ("positional", Token::Boolean(e.positional != Some(false))),
                        ],
                    )?;
                }
            }
            // unit.combat.onFired (src/game.js instrument): simEvents.emit("shot",
            // {source, team, position, direction, weapon, repair, melee, swing});
            // the shot counters observe it, the presentation capture records it.
            gen::EVENT_F3_SHOT if record.len() >= F3ShotNotice::WORDS => {
                let e = F3ShotNotice::take(
                    &record[..F3ShotNotice::WORDS],
                    &record[F3ShotNotice::WORDS..],
                );
                let slot = e.key.slot;
                let life = e.key.life;
                let Some((_, _, unit)) = self.units.iter().find(|(s, _, _)| *s == slot) else {
                    return Ok(());
                };
                // c.arsenal.weapons[row.weapon].name: the class of the weapon.
                let mut names = Vec::with_capacity(unit.combat.weapons.len());
                for weapon in &unit.combat.weapons {
                    let levels = context
                        .weapon_levels(weapon.def.0)
                        .map_err(|e| e.message().to_string())?;
                    let base = levels.first().cloned().unwrap_or_default();
                    names.push((
                        base.class.map(|c| String::from_utf16_lossy(&c)),
                        base.channel,
                    ));
                }
                let (weapon_name, _) = names
                    .get(e.shot.weapon as usize)
                    .cloned()
                    .unwrap_or((None, 0.0));
                let repair = e.shot.kind == CbShotKind::Repair;
                let melee = e.shot.kind == CbShotKind::MeleeStart;
                let weapon = weapon_name
                    .as_deref()
                    .map(|name| super::intern(context, name))
                    .transpose()?;
                self.journal.emit(Event::Shot {
                    source: Life {
                        id: slot,
                        generation: life,
                    },
                    team: e.team,
                    position: e.position,
                    direction: e.direction,
                    weapon,
                    repair,
                    melee,
                    swing: e.shot.swing,
                });
                // The shot counters count canonically once attached
                // (`bindCanonical` detached the page's listener): no observe.
                // attachPresentationCapture.describe -> events.capture "shot":
                // the weapon row is the first arsenal row of that name, its
                // channel the trigger's; a network human's press id comes
                // from his shots this tick, a bot has none.
                let index = names.iter().position(|(n, _)| *n == weapon_name);
                let channel = index.map_or(0.0, |i| names[i].1);
                let fire = self.press(slot, channel);
                if let Some(capture) = self.capture {
                    let weapon_id = match &weapon_name {
                        Some(name) => {
                            let id = super::intern(context, name)?;
                            context
                                .wire_weapon_id(capture.store_source, id)
                                .map_err(|e| e.message().to_string())?
                        }
                        None => 0.0,
                    };
                    capture.emit(
                        context,
                        self.tick,
                        "shot",
                        0.0,
                        &[
                            ("ownerId", Token::Number(f64::from(slot))),
                            ("ownerGeneration", Token::Number(life)),
                            ("mountId", Token::Number(0.0)),
                            ("mountGeneration", Token::Number(0.0)),
                            ("seat", Token::Number(0.0)),
                            ("fireId", Token::Number(fire)),
                            ("channel", Token::Number(channel)),
                            ("weaponId", Token::Number(weapon_id)),
                            ("position", Token::Vector(Some(e.position))),
                            ("repair", Token::Boolean(repair)),
                            ("melee", Token::Boolean(melee)),
                            ("swing", Token::Number(e.shot.swing)),
                        ],
                    )?;
                }
            }
            // F3Sim's f2Shot handler: `fleet.f2.firing.presentShot(event)`
            // (a sound, a muzzle effect and a light: no record) and
            // `fleet.world.fired(...)` -> `mountedShot(object, seat, shot,
            // owner, origin, direction, sample)` (src/game.js): a `shot` record
            // with the hull's registry target as its mount, then the
            // presentation capture's "shot" record. `firing.shot(a)` sets the
            // weapon and aim only: repair, melee and swing are the row's
            // defaults (false, false, 0).
            gen::EVENT_F2_SHOT if record.len() >= FfShot::WORDS => {
                let e = FfShot::take(&record[..FfShot::WORDS], &record[FfShot::WORDS..]);
                let sample = &e.sample;
                let source = sample.has_source.then_some(Life {
                    id: sample.source.id as u32,
                    generation: sample.source.generation,
                });
                let mount = Life {
                    id: sample.mount.id as u32,
                    generation: sample.mount.generation,
                };
                // `shot.weapon.name`: the seat weapon state's class
                // (`Weapon.name = def.class`), its channel `def.channel`.
                let level = context
                    .weapon_levels(e.state.def.0)
                    .map_err(|e| e.message().to_string())?
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                let weapon = match &level.class {
                    Some(class) => {
                        Some(context.intern(class).map_err(|e| e.message().to_string())?)
                    }
                    None => None,
                };
                self.journal.emit(Event::MountedShot {
                    source,
                    mount,
                    seat: f64::from(e.seat),
                    team: sample.team,
                    position: e.origin,
                    direction: e.direction,
                    weapon,
                    repair: false,
                    melee: false,
                    swing: 0.0,
                });
                // events.capture "shot": the weapon row is the mount's seat
                // weapon of that name in the shot's seat (vehicle rows come
                // first), its channel the weapon state's; the press id is the
                // source body's, as for an infantry shot.
                if let Some(capture) = self.capture {
                    // seatArsenal (src/seats.js) gave a seat weapon without
                    // a channel its index in the seat when the seat's
                    // first SeatUnit was made; a fired seat has one.
                    let channel = if level.named_channel {
                        level.channel
                    } else {
                        f64::from(e.weapon)
                    };
                    let fire = source.map_or(0.0, |source| self.press(source.id, channel));
                    let weapon_id = match weapon {
                        Some(name) => context
                            .wire_weapon_id(capture.store_source, name)
                            .map_err(|e| e.message().to_string())?,
                        None => 0.0,
                    };
                    capture.emit(
                        context,
                        self.tick,
                        "shot",
                        0.0,
                        &[
                            (
                                "ownerId",
                                Token::Number(source.map_or(0.0, |s| f64::from(s.id))),
                            ),
                            (
                                "ownerGeneration",
                                Token::Number(source.map_or(0.0, |s| s.generation)),
                            ),
                            ("mountId", Token::Number(f64::from(mount.id))),
                            ("mountGeneration", Token::Number(mount.generation)),
                            ("seat", Token::Number(f64::from(e.seat))),
                            ("fireId", Token::Number(fire)),
                            ("channel", Token::Number(channel)),
                            ("weaponId", Token::Number(weapon_id)),
                            ("position", Token::Vector(Some(e.origin))),
                            ("repair", Token::Boolean(false)),
                            ("melee", Token::Boolean(false)),
                            ("swing", Token::Number(0.0)),
                        ],
                    )?;
                }
            }
            // F2Seats.notice -> vehicles/turretSeats.onBoard/onUnboard
            // (src/game.js): simEvents.emit("board" | "unboard", {source,
            // team, position, target, seat}); the target is the holder's
            // registry target (a vehicle's or a turret's).
            gen::EVENT_F3_SEAT if record.len() >= F3SeatNotice::WORDS => {
                let e = F3SeatNotice::take(
                    &record[..F3SeatNotice::WORDS],
                    &record[F3SeatNotice::WORDS..],
                );
                let boarded = match e.kind {
                    F2SeatActionKind::Boarded => true,
                    F2SeatActionKind::Unboarded => false,
                    _ => return Ok(()),
                };
                let life = |present: bool, id: f64, generation: f64| {
                    present.then_some(Life {
                        id: id as u32,
                        generation,
                    })
                };
                self.journal.emit(Event::Seat {
                    boarded,
                    source: life(e.has_source, e.source.id, e.source.generation),
                    team: e.team,
                    position: e.position,
                    target: life(e.has_target, e.target.id, e.target.generation),
                    seat: f64::from(e.index),
                });
            }
            // F3Sim's f3Ordnance handler -> ordnance.onLaunch / onImpact ->
            // CombatCapture.launch / impact (src/runtime/network-entity-combat.js):
            // a bolt record (schema 1) at the launch, a terminal record
            // (schema 2) at the impact, both through the wire's combat encoder.
            gen::EVENT_F3_ORDNANCE if record.len() >= F3OrdnanceNotice::WORDS => {
                let e = F3OrdnanceNotice::take(
                    &record[..F3OrdnanceNotice::WORDS],
                    &record[F3OrdnanceNotice::WORDS..],
                );
                let key = (e.key.kind as u8, e.key.slot, e.key.generation);
                match e.event {
                    // ordnance.onProjectile -> ProjectileCapture.launch: the
                    // round's capture identity (`c.nextId++`) with its sampled
                    // owner and the drain's clock as its launch tick, handed
                    // to the wire's dynamic capture (`wire_combat_dynamic`),
                    // which samples its flight at each publication.
                    F3OrdnanceEvent::Projectile => {
                        let Some(capture) = self.capture else {
                            return Ok(());
                        };
                        // TYPES = ['grenade', 'round', 'beam'].
                        let kind = match e.key.kind {
                            F3ProjectileKind::Grenade => 0,
                            F3ProjectileKind::Round => 1,
                            F3ProjectileKind::Beam => 2,
                            F3ProjectileKind::Bolt => {
                                return Err("a bolt reached the dynamic capture".into())
                            }
                        };
                        let id = capture.allocate(context)?;
                        let definition_id = context
                            .wire_ordnance_id(capture.store_source, e.def.0)
                            .map_err(|e| e.message().to_string())?;
                        if definition_id == 0.0 {
                            return Err("combat definition absent from matched registry".into());
                        }
                        let fact = WireDynamicFact {
                            slot: e.key.slot,
                            ref_generation: e.key.generation,
                            r#type: kind,
                            id,
                            generation: super::capture::GENERATION,
                            launch_tick: self.tick,
                            definition_id,
                            owner_id: e.owner_id,
                            owner_generation: e.owner_generation,
                            fire_id: e.fire_id,
                            channel: e.channel,
                            terminal: false,
                        };
                        capture.dynamic(context, &fact)?;
                        self.dynamics.retain(|(k, _)| *k != key);
                        self.dynamics.push((key, fact));
                    }
                    // ordnance.onGrenadeContact (src/game.js): the `grenade`
                    // record of a grenade's first touch, for the bots' danger
                    // warnings; `def` is the round's class or null.
                    F3OrdnanceEvent::GrenadeContact => {
                        let source = e.has_source.then_some(Life {
                            id: e.source.id as u32,
                            generation: e.source.generation,
                        });
                        let def = self
                            .content
                            .ordnance_classes
                            .get(e.def.0 as usize)
                            .copied()
                            .flatten();
                        self.journal.emit(Event::Grenade {
                            position: e.position,
                            team: e.team,
                            source,
                            def,
                        });
                    }
                    // ordnance.world.bounce -> emitSound('bounce', def,
                    // position, speed), which the presentation capture records
                    // (`observeSounds`) when the registry names the round.
                    F3OrdnanceEvent::Bounce => {
                        let Some(capture) = self.capture else {
                            return Ok(());
                        };
                        let definition_id = context
                            .wire_ordnance_id(capture.store_source, e.def.0)
                            .map_err(|e| e.message().to_string())?;
                        if definition_id != 0.0 {
                            capture.emit(
                                context,
                                self.tick,
                                "bounce",
                                0.0,
                                &[
                                    ("definitionId", Token::Number(definition_id)),
                                    ("position", Token::Vector(Some(e.position))),
                                    ("amount", Token::Number(e.speed)),
                                ],
                            )?;
                        }
                    }
                    F3OrdnanceEvent::Launch => {
                        let Some(capture) = self.capture else {
                            return Ok(());
                        };
                        let id = capture.allocate(context)?;
                        let definition_id = context
                            .wire_ordnance_id(capture.store_source, e.def.0)
                            .map_err(|e| e.message().to_string())?;
                        let sample_tick = match e.tick {
                            Nullable::Value(t) => serde_json::json!(t),
                            _ => serde_json::Value::Null,
                        };
                        let facts = serde_json::json!({
                            "sample": true, "sampleTick": sample_tick, "tick": self.tick, "id": id, "generation": super::capture::GENERATION,
                            "life": e.life, "definitionId": definition_id,
                            "sampleOwner": [e.owner_id, e.owner_generation, e.fire_id, e.channel],
                            "boltOwner": [serde_json::Value::Null, serde_json::Value::Null, serde_json::Value::Null, serde_json::Value::Null],
                        });
                        let state =
                            bfe_presentation::ordnance::network::capture_bolt(&facts.to_string())?;
                        let v: serde_json::Value =
                            serde_json::from_str(&state).map_err(|e| e.to_string())?;
                        let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
                        let encode = WbEncode {
                            op: 0.0,
                            id: n("id"),
                            generation: n("generation"),
                            tick: self.tick,
                            schema: 1.0,
                            expiry_tick: n("expiryTick"),
                            definition_id: n("definitionId"),
                            owner_id: n("ownerId"),
                            owner_generation: n("ownerGeneration"),
                            fire_id: n("fireId"),
                            channel: n("channel"),
                            launch_tick: n("launchTick"),
                            execution_tick: n("executionTick"),
                            position: e.position.to_vec(),
                            direction: e.direction.to_vec(),
                            offset: if e.has_offset {
                                e.offset.to_vec()
                            } else {
                                vec![0.0, 0.0, 0.0]
                            },
                            velocity: Vec::new(),
                            quaternion: Vec::new(),
                            end: Vec::new(),
                            position_array: true,
                            direction_array: true,
                            offset_array: true,
                            speed: e.speed,
                            life: e.life,
                            merge: e.merge,
                            surface: 5.0,
                            target_id: 0.0,
                            target_generation: 0.0,
                            attacker_id: 0.0,
                            attacker_generation: 0.0,
                            team: 0.0,
                            attacker_team: 0.0,
                            source: Vec::new(),
                            has_source: false,
                            wet: false,
                            terminal: false,
                            r#type: 0.0,
                        };
                        let payload = capture.encode(context, &encode)?;
                        let view = super::capture::decode_view(self.tick, 1.0, &payload)?;
                        self.projectiles.retain(|(k, _)| *k != key);
                        self.projectiles.push((key, state));
                        capture.emit_record(context, self.tick, id, 1.0, payload, &view)?;
                    }
                    F3OrdnanceEvent::Impact => {
                        let Some(capture) = self.capture else {
                            return Ok(());
                        };
                        // `this.projectiles.get(bolt) ?? dynamic`: a dynamic
                        // round's terminal goes to the dynamic capture first.
                        let Some(at) = self.projectiles.iter().position(|(k, _)| *k == key) else {
                            if let Some(at) = self.dynamics.iter().position(|(k, _)| *k == key) {
                                let (_, fact) = self.dynamics.remove(at);
                                return self.dynamic_terminal(context, capture, fact, &e);
                            }
                            return Ok(());
                        };
                        let (_, state) = self.projectiles.remove(at);
                        let terminal = bfe_presentation::ordnance::network::capture_terminal(
                            &state, self.tick,
                        )?;
                        let v: serde_json::Value =
                            serde_json::from_str(&terminal).map_err(|e| e.to_string())?;
                        let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
                        let encode = WbEncode {
                            op: 0.0,
                            id: n("id"),
                            generation: n("generation"),
                            tick: self.tick,
                            schema: 2.0,
                            expiry_tick: n("expiryTick"),
                            definition_id: n("definitionId"),
                            owner_id: n("ownerId"),
                            owner_generation: n("ownerGeneration"),
                            fire_id: n("fireId"),
                            channel: n("channel"),
                            launch_tick: n("launchTick"),
                            execution_tick: n("executionTick"),
                            position: e.point.to_vec(),
                            direction: e.normal.to_vec(),
                            offset: vec![0.0, 0.0, 0.0],
                            velocity: Vec::new(),
                            quaternion: Vec::new(),
                            end: Vec::new(),
                            position_array: true,
                            direction_array: true,
                            offset_array: true,
                            speed: 0.0,
                            life: 0.0,
                            merge: 0.0,
                            surface: e.surface,
                            target_id: 0.0,
                            target_generation: 0.0,
                            attacker_id: 0.0,
                            attacker_generation: 0.0,
                            team: 0.0,
                            attacker_team: 0.0,
                            source: Vec::new(),
                            has_source: false,
                            wet: false,
                            terminal: false,
                            r#type: 0.0,
                        };
                        let payload = capture.encode(context, &encode)?;
                        let view = super::capture::decode_view(self.tick, 2.0, &payload)?;
                        capture.emit_record(context, self.tick, n("id"), 2.0, payload, &view)?;
                    }
                }
            }
            // F3Sim's f3Blast handler, one notice a stage: "present" draws it
            // (`presentExplosion`, `blastPresent`: no record but a remote
            // droid's blast); "event" is `blastNotice` (src/game.js), the
            // `explosion` record; "finished" is `ordnance.onExplosion`, whose
            // combat capture writes the explosion record (schema 5,
            // `CombatCapture.explosion`) with the definition's registry id.
            gen::EVENT_F3_BLAST if record.len() >= F3BlastNotice::WORDS => {
                let e = F3BlastNotice::take(
                    &record[..F3BlastNotice::WORDS],
                    &record[F3BlastNotice::WORDS..],
                );
                match e.stage {
                    F3BlastStage::Present => {}
                    F3BlastStage::Event => {
                        // `source: hasSource ? {unit: owner, generation} : null`.
                        let source = e.has_source.then_some(Life {
                            id: e.source.id as u32,
                            generation: e.source.generation,
                        });
                        self.journal.emit(Event::Explosion {
                            position: e.position,
                            team: e.team,
                            source,
                            radius: e.radius,
                        });
                    }
                    F3BlastStage::Finished => {
                        let Some(capture) = self.capture else {
                            return Ok(());
                        };
                        // `{id: this.allocate(), ..., definitionId:
                        // this.definitionId(definition)}`: the id is taken
                        // before the registry answers.
                        let id = capture.allocate(context)?;
                        let definition_id = capture
                            .explosions
                            .get(e.explosion as usize)
                            .copied()
                            .filter(|&id| id != 0)
                            .ok_or("combat definition absent from matched registry")?;
                        let encode = WbEncode {
                            op: 2.0,
                            id,
                            generation: super::capture::GENERATION,
                            tick: self.tick,
                            schema: 0.0,
                            expiry_tick: self.tick + 18.0,
                            definition_id: f64::from(definition_id),
                            owner_id: 0.0,
                            owner_generation: 0.0,
                            fire_id: 0.0,
                            channel: 0.0,
                            launch_tick: 0.0,
                            execution_tick: 0.0,
                            position: e.position.to_vec(),
                            direction: Vec::new(),
                            offset: Vec::new(),
                            velocity: Vec::new(),
                            quaternion: Vec::new(),
                            end: Vec::new(),
                            position_array: true,
                            direction_array: false,
                            offset_array: false,
                            speed: 0.0,
                            life: 0.0,
                            merge: 0.0,
                            surface: 0.0,
                            target_id: 0.0,
                            target_generation: 0.0,
                            attacker_id: 0.0,
                            attacker_generation: 0.0,
                            team: 0.0,
                            attacker_team: 0.0,
                            source: Vec::new(),
                            has_source: false,
                            wet: e.wet,
                            terminal: false,
                            r#type: 0.0,
                        };
                        let payload = capture.encode(context, &encode)?;
                        let view = super::capture::decode_view(self.tick, 5.0, &payload)?;
                        capture.emit_record(context, self.tick, id, 5.0, payload, &view)?;
                    }
                }
            }
            // F3Sim's f3Damage handler -> unit.combat.onDamaged(amount,
            // attacker, info, sample) (src/game.js instrument): the `damage`
            // record, which the AI hears and the combat capture turns into a
            // feedback record (schema 3, `CombatCapture.event`).
            gen::EVENT_F3_DAMAGE if record.len() >= F3DamageNotice::WORDS => {
                let e = F3DamageNotice::take(
                    &record[..F3DamageNotice::WORDS],
                    &record[F3DamageNotice::WORDS..],
                );
                let info = &e.info;
                let target = Life {
                    id: e.key.slot,
                    generation: e.key.life,
                };
                // `sample.hasAttackerRef ? {unit, generation} : null`.
                let attacker = e.has_attacker_ref.then_some(Life {
                    id: e.attacker_ref.id as u32,
                    generation: e.attacker_ref.generation,
                });
                // `info.point`: undefined and null both write null.
                let point = (info.point_state == F3OptionalState::Value).then_some(info.point);
                let source = info.source_present.then_some(info.source);
                self.journal.emit(Event::Damage {
                    target,
                    team: e.team,
                    position: e.position,
                    amount: info.amount.unwrap_or(e.amount),
                    typed: info.typed.unwrap_or(1.0),
                    generic: info.generic.unwrap_or(1.0),
                    known: info.known.unwrap_or(false),
                    point,
                    source,
                    health: e.amount,
                    attacker,
                    attacker_team: e.attacker_team,
                });
                if let Some(capture) = self.capture {
                    capture.feedback(
                        context,
                        self.tick,
                        3.0,
                        Some(target),
                        attacker,
                        e.position,
                        e.team,
                        e.attacker_team,
                        source,
                    )?;
                }
            }
            // F3Sim's f3Presentation handler -> unit.combat.onPresentation
            // (event, event) (src/game.js instrument): the snapshot of the
            // notice as a `presentation` record, owned by the notice's unit
            // life, which the presentation capture sends as is.
            gen::EVENT_F3_PRESENTATION if record.len() >= F3Presentation::WORDS => {
                let e = F3Presentation::take(
                    &record[..F3Presentation::WORDS],
                    &record[F3Presentation::WORDS..],
                );
                let owner = Life {
                    id: e.key.slot,
                    generation: e.key.life,
                };
                let presentation = match e.kind {
                    F3PresentKind::WeaponAction => Presentation::WeaponAction {
                        action: e.action,
                        bank: e.bank,
                        posture: e.posture,
                        prepare: e.prepare,
                        owner,
                    },
                    F3PresentKind::EndWeaponAction => Presentation::EndWeaponAction {
                        action: e.action,
                        owner,
                    },
                    F3PresentKind::Lightning => Presentation::Lightning {
                        name: e.name,
                        from: e.from,
                        to: e.to,
                        charge: e.charge,
                        channel: e.channel,
                        owner,
                    },
                };
                self.present(context, presentation)?;
            }
            // F3Sim's f3Death handler -> deathNotice (src/game.js): the
            // `death` record, which the combat capture turns into a feedback
            // record (schema 4); `hooks.presented('died')` writes nothing.
            gen::EVENT_F3_DEATH if record.len() >= F3DeathNotice::WORDS => {
                let e = F3DeathNotice::take(
                    &record[..F3DeathNotice::WORDS],
                    &record[F3DeathNotice::WORDS..],
                );
                let target = Life {
                    id: e.key.slot,
                    generation: e.key.life,
                };
                let attacker = e.has_attacker_ref.then_some(Life {
                    id: e.attacker_ref.id as u32,
                    generation: e.attacker_ref.generation,
                });
                self.journal.emit(Event::Death {
                    target,
                    team: e.team,
                    position: e.position,
                    attacker,
                    attacker_team: e.attacker_team,
                });
                if let Some(capture) = self.capture {
                    capture.feedback(
                        context,
                        self.tick,
                        4.0,
                        Some(target),
                        attacker,
                        e.position,
                        e.team,
                        e.attacker_team,
                        None,
                    )?;
                }
            }
            // F3Sim's f3Body handler: a death blast and a body blown apart
            // are `hooks.presented` (src/game.js) presentation records owned
            // by the notice's unit life; a respawn's onRespawn does nothing.
            gen::EVENT_F3_BODY if record.len() >= F3BodyNotice::WORDS => {
                let e = F3BodyNotice::take(
                    &record[..F3BodyNotice::WORDS],
                    &record[F3BodyNotice::WORDS..],
                );
                let owner = Life {
                    id: e.key.slot,
                    generation: e.key.life,
                };
                let presented = match e.event {
                    F3BodyEvent::DeathBlast => Presentation::Presented {
                        kind: Presented::DeathBlast,
                        owner,
                        amount: 0.0,
                        source: None,
                    },
                    F3BodyEvent::BlowApart => Presentation::Presented {
                        kind: Presented::BlowApart,
                        owner,
                        amount: e.health,
                        source: None,
                    },
                    F3BodyEvent::Respawn => return Ok(()),
                };
                self.present(context, presented)?;
            }
            // F3Sim's f3Shield handler -> shieldHit: `hooks.presented
            // ('shieldHit', unit, taken, source, kind)`, the source the
            // notice's vector (`source.toArray()`) when it has one.
            gen::EVENT_F3_SHIELD if record.len() >= F3ShieldNotice::WORDS => {
                let e = F3ShieldNotice::take(
                    &record[..F3ShieldNotice::WORDS],
                    &record[F3ShieldNotice::WORDS..],
                );
                let owner = Life {
                    id: e.key.slot,
                    generation: e.key.life,
                };
                let kind = if e.explosion {
                    Presented::ShieldHitExplosion
                } else {
                    Presented::ShieldHit
                };
                self.present(
                    context,
                    Presentation::Presented {
                        kind,
                        owner,
                        amount: e.taken,
                        source: e.source_present.then_some(e.source),
                    },
                )?;
            }
            // F3Sim's f3Hull handler -> F3Hulls.notice (src/sim/wasm/f3-hulls.js):
            // a hull's damage is vehicles.onDamaged (src/game.js), a `damage`
            // record with the hull's registry target and no hit source,
            // which the AI hears and the combat capture turns into a
            // feedback record; its explosion effect is presentEffect's
            // `effect` presentation record.
            gen::EVENT_F3_HULL if record.len() >= F3HullNotice::WORDS => {
                let e = F3HullNotice::take(
                    &record[..F3HullNotice::WORDS],
                    &record[F3HullNotice::WORDS..],
                );
                match e.event {
                    F3HullEvent::Damaged => {
                        let id = context
                            .world(|w| w.sim.store.vehicle.get(e.hull).map(|v| v.target_id))
                            .map_err(|e| e.message().to_string())?
                            .ok_or("a damaged hull has no store row")?;
                        let target = Life {
                            id: id as u32,
                            generation: e.life,
                        };
                        let attacker = e.has_attacker_ref.then_some(Life {
                            id: e.attacker_ref.id as u32,
                            generation: e.attacker_ref.generation,
                        });
                        self.journal.emit(Event::HullDamage {
                            target,
                            team: e.team,
                            position: e.position,
                            amount: e.amount,
                            attacker,
                            attacker_team: e.attacker_team,
                        });
                        if let Some(capture) = self.capture {
                            capture.feedback(
                                context,
                                self.tick,
                                3.0,
                                Some(target),
                                attacker,
                                e.position,
                                e.team,
                                e.attacker_team,
                                None,
                            )?;
                        }
                    }
                    F3HullEvent::ExplosionEffect => {
                        let name = super::intern(context, "explosion")?;
                        self.present(
                            context,
                            Presentation::Effect {
                                name,
                                position: e.position,
                                owner: None,
                            },
                        )?;
                    }
                }
            }
            // F3Sim's f3Effect handler: a headshot is presentEffect's effect
            // record; a melee hit only plays an effect; a melee deflection
            // plays one and is the unit's onPresentation, an effect record
            // named lightsabre_deflect that its unit life owns.
            gen::EVENT_F3_EFFECT if record.len() >= F3EffectNotice::WORDS => {
                let e = F3EffectNotice::take(
                    &record[..F3EffectNotice::WORDS],
                    &record[F3EffectNotice::WORDS..],
                );
                let (name, owner) = match e.effect {
                    F3Effect::Headshot => ("headshot", None),
                    F3Effect::MeleeHit => return Ok(()),
                    F3Effect::MeleeDeflect => (
                        "lightsabre_deflect",
                        Some(Life {
                            id: e.key.slot,
                            generation: e.key.life,
                        }),
                    ),
                };
                let name = super::intern(context, name)?;
                self.present(
                    context,
                    Presentation::Effect {
                        name,
                        position: e.position,
                        owner,
                    },
                )?;
            }
            // F3Sim's f3PowerupEffect and f3Splash handlers: presentEffect's
            // effect records (a power-up's named effect, watersplash_md).
            gen::EVENT_F3_POWERUP_EFFECT if record.len() >= F3PowerupEffect::WORDS => {
                let e = F3PowerupEffect::take(
                    &record[..F3PowerupEffect::WORDS],
                    &record[F3PowerupEffect::WORDS..],
                );
                self.present(
                    context,
                    Presentation::Effect {
                        name: e.name,
                        position: e.position,
                        owner: None,
                    },
                )?;
            }
            gen::EVENT_F3_SPLASH if record.len() >= UnitSplash::WORDS => {
                let e =
                    UnitSplash::take(&record[..UnitSplash::WORDS], &record[UnitSplash::WORDS..]);
                let name = super::intern(context, "watersplash_md")?;
                self.present(
                    context,
                    Presentation::Effect {
                        name,
                        position: e.at,
                        owner: None,
                    },
                )?;
            }
            // F3Sim's f3Pickup handler -> items.world.pickup -> emitSound
            // ('pickup', def, position), which the presentation capture
            // records (`observeSounds`) when the registry names the item;
            // the sound carries no amount (0).
            gen::EVENT_F3_PICKUP if record.len() >= PickupArgs::WORDS => {
                let e =
                    PickupArgs::take(&record[..PickupArgs::WORDS], &record[PickupArgs::WORDS..]);
                let Some(capture) = self.capture else {
                    return Ok(());
                };
                let definition_id = context
                    .wire_item_id(capture.store_source, e.def.0)
                    .map_err(|e| e.message().to_string())?;
                if definition_id != 0.0 {
                    capture.emit(
                        context,
                        self.tick,
                        "pickup",
                        0.0,
                        &[
                            ("definitionId", Token::Number(definition_id)),
                            ("position", Token::Vector(Some(e.position))),
                            ("amount", Token::Number(0.0)),
                        ],
                    )?;
                }
            }
            // F3Sim's f3PropDamage handler -> F3Props.damaged -> the turret's
            // onDamaged (src/game.js): a `damage` record naming the turret's
            // registry target at the notice's life, team and position, its
            // known point 30 units back along the hit's flat direction, the
            // attacker as refOf and teamOf read it then; the AI hears it and
            // the combat capture turns it into a feedback record.
            gen::EVENT_F3_PROP_DAMAGE if record.len() >= F3PropDamage::WORDS => {
                let e = F3PropDamage::take(
                    &record[..F3PropDamage::WORDS],
                    &record[F3PropDamage::WORDS..],
                );
                let target_id = context
                    .world(|w| {
                        crate::units::f3_props::building_target_id_in(&w.units, e.index as usize)
                    })
                    .map_err(|e| e.message().to_string())?;
                // `sample && object.aiTarget ? {unit, generation} : refOf(...)`.
                let target = target_id.map(|id| (id, e.generation));
                // `turretFrom.set(d.x, 0.0001, d.z).normalize()
                // .multiplyScalar(-30).add(object.position)`.
                let point = e.has_direction.then(|| {
                    let mut from = three::Vector3::new(e.direction[0], 0.0001, e.direction[2]);
                    from.normalize();
                    from.multiply_scalar(-30.0);
                    from.add(&three::Vector3::new(
                        e.position[0],
                        e.position[1],
                        e.position[2],
                    ));
                    [from.x, from.y, from.z]
                });
                let (attacker, attacker_team) = match e.attacker {
                    Attacker::Unit(r) => {
                        let unit = self.units.iter().find(|(slot, _, _)| *slot == r.slot);
                        (
                            unit.map(|(slot, _, u)| Life {
                                id: *slot,
                                generation: u.generation,
                            }),
                            unit.map_or(0.0, |(_, _, u)| u.team),
                        )
                    }
                    Attacker::OwnerSnapshot(r) => {
                        let snapshot = context
                            .world(|w| {
                                w.sim
                                    .store
                                    .owner_snapshot
                                    .get(r)
                                    .map(|o| (o.id, o.generation, o.team))
                            })
                            .map_err(|e| e.message().to_string())?;
                        (
                            snapshot.map(|(id, generation, _)| Life {
                                id: id as u32,
                                generation,
                            }),
                            snapshot.map_or(0.0, |s| s.2),
                        )
                    }
                    _ => (None, 0.0),
                };
                self.journal.emit(Event::PropDamage {
                    target,
                    team: e.team,
                    position: e.position,
                    amount: e.untyped,
                    typed: e.typed,
                    generic: e.generic,
                    point,
                    health: e.amount,
                    attacker,
                    attacker_team,
                });
                if let Some(capture) = self.capture {
                    let target = target.map(|(id, generation)| Life {
                        id,
                        generation: generation.unwrap_or(0.0),
                    });
                    capture.feedback(
                        context,
                        self.tick,
                        3.0,
                        target,
                        attacker,
                        e.position,
                        e.team,
                        attacker_team,
                        None,
                    )?;
                }
            }
            // F4Live's f4Effect handler: presentEffect's effect record.
            gen::EVENT_F4_EFFECT if record.len() >= F4Effect::WORDS => {
                let e = F4Effect::take(&record[..F4Effect::WORDS], &record[F4Effect::WORDS..]);
                self.present(
                    context,
                    Presentation::Effect {
                        name: e.name,
                        position: e.position,
                        owner: None,
                    },
                )?;
            }
            // F4Live's f4Serve handler -> sound('serve', station, supply),
            // which the presentation capture records (`observeSounds`): the
            // station's index in LiveObjects.buildings and the served unit
            // (the first whose supply is the served one: that unit), none
            // when nothing was served.
            gen::EVENT_F4_SERVE if record.len() >= F4Serve::WORDS => {
                let e = F4Serve::take(&record[..F4Serve::WORDS], &record[F4Serve::WORDS..]);
                let Some(capture) = self.capture else {
                    return Ok(());
                };
                let building = context
                    .world(|w| crate::units::f3_props::building_of_in(&w.units, &e.station))
                    .map_err(|e| e.message().to_string())?;
                // events.capture "serve" answers null without the building.
                let Some(building) = building else {
                    return Ok(());
                };
                let owner = e
                    .unit
                    .and_then(|r| self.units.iter().find(|(slot, _, _)| *slot == r.slot))
                    .map(|(slot, _, u)| (f64::from(*slot), u.generation));
                capture.emit(
                    context,
                    self.tick,
                    "serve",
                    0.0,
                    &[
                        ("building", Token::Number(building as f64)),
                        ("ownerId", Token::Number(owner.map_or(0.0, |o| o.0))),
                        ("ownerGeneration", Token::Number(owner.map_or(0.0, |o| o.1))),
                    ],
                )?;
            }
            // F3Props.effect -> show(index, state): each of the building's
            // bodies (built, destroyed, unbuilt) is enabled when its look is
            // the shown state (`bodies[k].enabled = LOOKS[k] === shown`).
            gen::EVENT_F3_PROP_EFFECT if record.len() >= F3PropEffect::WORDS => {
                let e = F3PropEffect::take(
                    &record[..F3PropEffect::WORDS],
                    &record[F3PropEffect::WORDS..],
                );
                if e.kind == F3PropEffectKind::Show {
                    let index = e.state.index as usize;
                    if let Some((_, kernels)) = self.prop_bodies.iter().find(|(i, _)| *i == index) {
                        let looks = [
                            PbStateName::Built,
                            PbStateName::Destroyed,
                            PbStateName::Unbuilt,
                        ];
                        for (k, kernel) in kernels.iter().enumerate() {
                            if let Some(kernel) = kernel {
                                self.toggles
                                    .push((*kernel as usize, u8::from(looks[k] == e.state.state)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
