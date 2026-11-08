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
    self, AiActor, AiVoiceNotice, CbShotKind, F2SeatActionKind, F3OrdnanceEvent, F3OrdnanceNotice,
    F3PropEffect, F3PropEffectKind, F3SeatNotice, F3ShotNotice, F4SpawnNotice, PbStateName, Unit,
    WbEncode,
};
use crate::store::Nullable;
use crate::store::StrId;

use super::capture::{Capture, Token};
use super::journal::Journal;
use super::json::Json;

/// `units.ref(unit)` then `eventValue`: `{"id":slot,"generation":life}`.
fn reference(json: &mut Json, id: u32, generation: f64) {
    json.begin_object();
    json.key("id");
    json.number(f64::from(id));
    json.key("generation");
    json.number(generation);
    json.end_object();
}

fn string(json: &mut Json, context: &Context, id: Option<StrId>) {
    match id.and_then(|id| context.string(id).ok()) {
        Some(units) => json.string_utf16(&units),
        None => json.null(),
    }
}

/// The fields of one record after `kind`, as JSON text without braces.
fn fields(json: &Json) -> String {
    let text = json.text();
    text[1..text.len() - 1].to_string()
}

/// One tick's drain: the world's clock for the records, the observers'
/// handles, and the units' rows (`store.unit`) for the facts the page read
/// from its unit objects.
pub struct Drain<'a> {
    pub tick: f64,
    /// The presentation capture over the wire's combat capture, when
    /// the host publishes one.
    pub capture: Option<&'a Capture>,
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
}

impl Drain<'_> {
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
        let mut json = Json::new();
        while at + 2 <= end {
            let code = words[at] as u32;
            let length = words[at + 1] as usize;
            let record = &words[at + 2..(at + 2 + length).min(end)];
            self.journal.time = time(at - 1);
            self.handle(context, code, record, &mut json)?;
            at += 2 + length;
        }
        Ok(at - 1)
    }

    fn handle(
        &mut self,
        context: &Context,
        code: u32,
        record: &[f64],
        json: &mut Json,
    ) -> Result<(), String> {
        match code {
            // F4Tail.onSpawn (src/game.js): simEvents.emit("spawn", {target:
            // units.ref(event.unit), team, position, className, post}).
            gen::EVENT_F4_SPAWN if record.len() >= F4SpawnNotice::WORDS => {
                let e = F4SpawnNotice::take(
                    &record[..F4SpawnNotice::WORDS],
                    &record[F4SpawnNotice::WORDS..],
                );
                json.clear();
                json.begin_object();
                json.key("target");
                reference(json, e.unit.slot, e.key.life);
                json.key("team");
                json.number(e.team);
                json.key("position");
                json.vec3(&e.position);
                json.key("className");
                string(json, context, Some(e.class_name));
                json.key("post");
                string(json, context, e.post);
                json.end_object();
                let text = fields(json);
                self.journal.emit("spawn", None, &text);
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
                // src/sim/wasm/f2-seats.js); the seat unit shares its lease's slot.
                let owner = match e.actor {
                    AiActor::Unit(unit) => unit.slot,
                    AiActor::SeatUnit(seat) => self
                        .leases
                        .iter()
                        .find(|(s, _)| *s == seat.slot)
                        .and_then(|(_, l)| l.unit)
                        .map_or(0, |u| u.slot),
                };
                json.clear();
                json.begin_object();
                json.key("source");
                reference(json, owner, e.generation);
                json.key("team");
                json.number(e.team);
                json.key("position");
                json.vec3(&e.position);
                json.key("human");
                json.boolean(e.human);
                json.key("event");
                string(json, context, Some(e.event));
                json.key("property");
                string(json, context, Some(e.property));
                if let Some(command) = e.command {
                    json.key("command");
                    json.number(command);
                }
                if let Some(ctx) = e.context {
                    json.key("context");
                    json.number(ctx);
                }
                if let Some(positional) = e.positional {
                    json.key("positional");
                    json.boolean(positional);
                }
                if let Some(notice) = e.notice {
                    json.key("notice");
                    json.boolean(notice);
                }
                json.end_object();
                let text = fields(json);
                self.journal.emit("vo", Some(e.time), &text);
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
                json.clear();
                json.begin_object();
                json.key("source");
                reference(json, slot, life);
                json.key("team");
                json.number(e.team);
                json.key("position");
                json.vec3(&e.position);
                json.key("direction");
                json.vec3(&e.direction);
                json.key("weapon");
                match &weapon_name {
                    Some(name) => json.string(name),
                    None => json.null(),
                }
                json.key("repair");
                json.boolean(repair);
                json.key("melee");
                json.boolean(melee);
                json.key("swing");
                json.number(e.shot.swing);
                json.end_object();
                let text = fields(json);
                self.journal.emit_heard(
                    "shot",
                    None,
                    &text,
                    super::journal::Hearing::Shot {
                        source: Some(super::journal::Heard {
                            slot,
                            generation: life,
                        }),
                    },
                );
                // The shot counters count canonically once attached
                // (`bindCanonical` detached the page's listener): no observe.
                // attachPresentationCapture.describe -> events.capture "shot":
                // the weapon row is the first arsenal row of that name, its
                // channel the trigger's; a network human's press id comes
                // from his shots this tick, a bot has none.
                let index = names.iter().position(|(n, _)| *n == weapon_name);
                let channel = index.map_or(0.0, |i| names[i].1);
                let fire = self
                    .presses
                    .iter()
                    .find(|(unit, _)| *unit == slot)
                    .map_or(0.0, |(_, presses)| {
                        presses[if channel == 1.0 { 1 } else { 0 }].unwrap_or(0.0)
                    });
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
            // F2Seats.notice -> vehicles/turretSeats.onBoard/onUnboard
            // (src/game.js): simEvents.emit("board" | "unboard", {source,
            // team, position, target, seat}); the target is the holder's
            // registry target (a vehicle's or a turret's).
            gen::EVENT_F3_SEAT if record.len() >= F3SeatNotice::WORDS => {
                let e = F3SeatNotice::take(
                    &record[..F3SeatNotice::WORDS],
                    &record[F3SeatNotice::WORDS..],
                );
                let kind = match e.kind {
                    F2SeatActionKind::Boarded => "board",
                    F2SeatActionKind::Unboarded => "unboard",
                    _ => return Ok(()),
                };
                json.clear();
                json.begin_object();
                json.key("source");
                if e.has_source {
                    reference(json, e.source.id as u32, e.source.generation);
                } else {
                    json.null();
                }
                json.key("team");
                json.number(e.team);
                json.key("position");
                json.vec3(&e.position);
                json.key("target");
                if e.has_target {
                    reference(json, e.target.id as u32, e.target.generation);
                } else {
                    json.null();
                }
                json.key("seat");
                json.number(f64::from(e.index));
                json.end_object();
                let text = fields(json);
                self.journal.emit(kind, None, &text);
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
                let Some(capture) = self.capture else {
                    return Ok(());
                };
                let key = (e.key.kind as u8, e.key.slot, e.key.generation);
                match e.event {
                    F3OrdnanceEvent::Launch => {
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
                        let Some(at) = self.projectiles.iter().position(|(k, _)| *k == key) else {
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
                    _ => {}
                }
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
