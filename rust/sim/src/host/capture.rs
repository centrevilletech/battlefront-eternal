//! The page's presentation capture (src/runtime/entity-presentation-events.js
//! `attachPresentationCapture` and src/runtime/network-entity-combat.js
//! `CombatCapture.emit`): a journal record the presentation crate accepts
//! becomes a presentation record (schema 8) in the wire's combat capture,
//! which the publisher sends as a cosmetic event. The event's fields are
//! what `bfe_presentation::ordnance::events::capture` orders for the kind,
//! encoded by the wire's presentation encoder (`wire_presentation_run` op 2)
//! from the same tokens the page's `event()` wrote, then emitted as the
//! page's `WireCombat.emit` did: a capture key, its expiry, its commit.
use crate::abi::context::Context;
use crate::gen::{
    WbCall, WbCommit, WbEncode, WbResult, WireDynamicFact, WireJournalFact, WireOutput, WireSpan,
};
use crate::store::StrId;
use crate::world::admit::{Call, Returned};

use super::intern;

/// `PRESENTATION_SCHEMA`: the schema of a presentation record.
const SCHEMA: f64 = 8.0;
/// `CombatCapture.maxRecords`.
const MAX_RECORDS: f64 = 4096.0;
/// `capture.generation`: the content epoch.
pub const GENERATION: f64 = 1.0;

/// `decodeCombat(record)` for a bolt or terminal record (schema 1 or 2):
/// the fact's view from the codec's JSON.
pub fn decode_view(tick: f64, schema: f64, payload: &[u8]) -> Result<View, String> {
    let decoded = bfe_presentation::ordnance::codec::decode("combat", schema, tick, payload)
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&decoded).map_err(|e| e.to_string())?;
    let vec3 = |value: &serde_json::Value| {
        value.as_array().filter(|a| a.len() == 3).map(|a| {
            [
                a[0].as_f64().unwrap_or(0.0),
                a[1].as_f64().unwrap_or(0.0),
                a[2].as_f64().unwrap_or(0.0),
            ]
        })
    };
    Ok(View {
        expiry: v["expiryTick"].as_f64().unwrap_or(tick),
        position: vec3(&v["position"]),
        owner: match &v["ownerId"] {
            serde_json::Value::Null if v.get("ownerId").is_some() => Some(None),
            serde_json::Value::Number(n) => Some(n.as_f64()),
            _ => None,
        },
        speed: v["speed"].as_f64().unwrap_or(f64::NAN),
        life: v["life"].as_f64().unwrap_or(f64::NAN),
    })
}

/// wire-presentation.js `ERRORS`: the message of each event writer error.
const PRESENTATION_ERRORS: [&str; 15] = [
    "",
    "Invalid presentation exchange",
    "Unit weapon count exceeds presentation limit",
    "Repair identity too long",
    "Invalid unit presentation number",
    "Unknown unit weapon",
    "World presentation record too large",
    "Invalid presentation kind",
    "Invalid presentation text",
    "Invalid presentation vector",
    "Invalid presentation channel",
    "Invalid presentation identity",
    "Invalid presentation number",
    "Invalid presentation flag",
    "Presentation event too large",
];

/// A value of the presentation event, as `eventValue` tokenizes it.
pub enum Token<'a> {
    Null,
    /// An entry whose value is undefined (`Object.entries` keeps it).
    Undefined,
    Number(f64),
    Boolean(bool),
    Text(&'a str),
    Vector(Option<[f64; 3]>),
}

/// The capture's handles from the admission: the wire combat capture and
/// the presentation encoder.
pub struct Capture {
    pub combat: u32,
    pub presentation: u32,
    /// The wire store source whose registry names the weapons and the
    /// ordnance definitions (`registry.weaponIds`, `definitionId`).
    pub store_source: u32,
    /// The combat registry's id for each of the battle's explosions, by
    /// explosion index (`registry.combat.definitionId(def)`; 0 for one it
    /// lacks). Whoever admitted the world knows which object each
    /// explosion is; the store source's tables carry no explosions.
    pub explosions: Vec<u32>,
}

/// What `decodeCombat(record)` gave the fact: the record's expiry, its
/// position, its owner (`undefined`, `null` or a number) and its numbers.
pub struct View {
    pub expiry: f64,
    pub position: Option<[f64; 3]>,
    pub owner: Option<Option<f64>>,
    pub speed: f64,
    pub life: f64,
}

impl Capture {
    /// `eventValue` (src/sim/wasm/runtime/wire-presentation.js): null 0,
    /// number 1 v, boolean 2 b, string 3 id, array 4 n then up to three
    /// children, undefined 6; the event's words are the kind, the blowApart amount, the
    /// entry count, then key id and value for each entry.
    fn tokens(
        context: &Context,
        kind: &str,
        amount: f64,
        fields: &[(&str, Token<'_>)],
    ) -> Result<Vec<f64>, String> {
        let mut words = Vec::with_capacity(8 + fields.len() * 6);
        let value = |words: &mut Vec<f64>, token: &Token<'_>| -> Result<(), String> {
            match token {
                Token::Null => words.push(0.0),
                Token::Undefined => words.push(6.0),
                Token::Number(n) => words.extend([1.0, *n]),
                Token::Boolean(b) => words.extend([2.0, f64::from(u8::from(*b))]),
                Token::Text(s) => words.extend([3.0, f64::from(intern(context, s)?.0)]),
                Token::Vector(None) => words.push(0.0),
                Token::Vector(Some(v)) => {
                    words.extend([4.0, 3.0]);
                    for x in v {
                        words.extend([1.0, *x]);
                    }
                }
            }
            Ok(())
        };
        value(&mut words, &Token::Text(kind))?;
        words.push(amount);
        // The event object's entries begin with its kind.
        words.push((fields.len() + 1) as f64);
        words.push(f64::from(intern(context, "kind")?.0));
        value(&mut words, &Token::Text(kind))?;
        for (key, token) in fields {
            words.push(f64::from(intern(context, key)?.0));
            value(&mut words, token)?;
        }
        Ok(words)
    }

    /// `allocateCapture(capture)`: `PageCombat.capture_identity(nextId)`,
    /// the next record id, the counter advanced.
    pub fn allocate(&self, context: &Context) -> Result<f64, String> {
        let id = match context.invoke_export(
            "wire_combat_next_id",
            &Call {
                args: vec![f64::from(self.combat)],
                ..Default::default()
            },
        ) {
            Ok(Returned::Scalar(id)) => id,
            other => return Err(format!("wire_combat_next_id: {other:?}")),
        };
        if id > 4294967295.0 {
            return Err("combat identity exhausted".into());
        }
        context
            .invoke_export(
                "wire_combat_set_next_id",
                &Call {
                    args: vec![f64::from(self.combat), id + 1.0],
                    ..Default::default()
                },
            )
            .map_err(|e| e.message().to_string())?;
        Ok(id)
    }

    /// `WireCombat.encode(op, args)`: the combat encoder's bytes for a
    /// record (`wire_combat_encode`), or its error text.
    pub fn encode(&self, context: &Context, encode: &WbEncode) -> Result<Vec<u8>, String> {
        let mut row = vec![0.0; WbEncode::WORDS];
        let mut heap = Vec::new();
        encode.put(&mut row, &mut heap);
        let words = match context.invoke_export(
            "wire_combat_encode",
            &Call {
                row,
                heap,
                ..Default::default()
            },
        ) {
            Ok(Returned::Row(words)) => words,
            other => return Err(format!("wire_combat_encode: {other:?}")),
        };
        let (row, heap) = super::block_row(&words);
        let out = WireOutput::take(row, heap);
        if let Some(error) = out.error {
            let text = context.string(error).map_err(|e| e.message().to_string())?;
            return Err(String::from_utf16_lossy(&text));
        }
        Ok(out
            .parts
            .into_iter()
            .next()
            .map(|p| p.bytes)
            .unwrap_or_default())
    }

    /// `capture.emit(presentationRecord({id: allocateCapture(capture),
    /// generation, tick, event, native: service}))`: false when the
    /// capture is full (the page dropped the record), as `WireCombat.emit`.
    pub fn emit(
        &self,
        context: &Context,
        tick: f64,
        kind: &str,
        amount: f64,
        fields: &[(&str, Token<'_>)],
    ) -> Result<bool, String> {
        let id = self.allocate(context)?;
        // service.event(event): the encoder's room, the run, the bytes.
        let words = Self::tokens(context, kind, amount, fields)?;
        let room = Call {
            args: vec![f64::from(self.presentation), words.len() as f64],
            scratch: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
            ..Default::default()
        };
        context
            .invoke_export("wire_presentation_room", &room)
            .map_err(|e| e.message().to_string())?;
        match context.invoke_export(
            "wire_presentation_run",
            &Call {
                args: vec![f64::from(self.presentation), 2.0],
                ..Default::default()
            },
        ) {
            Ok(Returned::Scalar(0.0)) => {}
            // `service.event(event)` threw (wire-presentation.js run's
            // ERRORS) and attachPresentationCapture's guard handed the error
            // to `capture.fault`: the id stays spent, the battle goes on.
            Ok(Returned::Scalar(error)) => {
                let message = match error as usize {
                    15 => {
                        let field = match context.invoke_export(
                            "wire_presentation_error_field",
                            &Call {
                                args: vec![f64::from(self.presentation)],
                                ..Default::default()
                            },
                        ) {
                            Ok(Returned::Scalar(field)) => field,
                            other => {
                                return Err(format!("wire_presentation_error_field: {other:?}"))
                            }
                        };
                        let name = context
                            .string(StrId(field as u32))
                            .map_err(|e| e.message().to_string())?;
                        format!(
                            "Unknown presentation field: {}",
                            String::from_utf16_lossy(&name)
                        )
                    }
                    code => PRESENTATION_ERRORS
                        .get(code)
                        .copied()
                        .unwrap_or(PRESENTATION_ERRORS[1])
                        .to_string(),
                };
                self.fault(context, &message)?;
                return Ok(false);
            }
            other => return Err(format!("wire_presentation_run: {other:?}")),
        }
        let payload = match context.invoke_export(
            "wire_presentation_ptr",
            &Call {
                args: vec![f64::from(self.presentation)],
                ..Default::default()
            },
        ) {
            Ok(Returned::Bytes(bytes)) => bytes,
            other => return Err(format!("wire_presentation_ptr: {other:?}")),
        };
        // decodeCombat(record): a presentation record decodes to its event
        // plus `expiryTick: tick + 120`; the fact reads the event's position
        // and owner, and has no speed or life.
        let decoded = bfe_presentation::ordnance::codec::decode("combat", SCHEMA, tick, &payload)
            .map_err(|e| e.to_string())?;
        let expiry = serde_json::from_str::<serde_json::Value>(&decoded)
            .ok()
            .and_then(|v| v["expiryTick"].as_f64())
            .unwrap_or(tick + 120.0);
        let position = fields
            .iter()
            .find(|(k, _)| *k == "position")
            .and_then(|(_, t)| match t {
                Token::Vector(Some(v)) => Some(*v),
                _ => None,
            });
        let owner = match fields.iter().find(|(k, _)| *k == "ownerId") {
            None => None,
            Some((_, Token::Null)) => Some(None),
            Some((_, Token::Number(n))) => Some(Some(*n)),
            Some(_) => None,
        };
        self.emit_record(
            context,
            tick,
            id,
            SCHEMA,
            payload,
            &View {
                expiry,
                position,
                owner,
                speed: f64::NAN,
                life: f64::NAN,
            },
        )
    }

    /// `WireCombat.dynamic(entry)`: a dynamic round's identity and sampled
    /// owner into the wire's capture, which samples it at each publication.
    pub fn dynamic(&self, context: &Context, fact: &WireDynamicFact) -> Result<(), String> {
        let mut row = vec![0.0; WireDynamicFact::WORDS];
        let mut heap = Vec::new();
        fact.put(&mut row, &mut heap);
        match context.invoke_export(
            "wire_combat_dynamic",
            &Call {
                args: vec![f64::from(self.combat)],
                row,
                heap,
                ..Default::default()
            },
        ) {
            Ok(Returned::Scalar(_)) => Ok(()),
            other => Err(format!("wire_combat_dynamic: {other:?}")),
        }
    }

    /// `CombatCapture.event` for a damage or death record (schema 3 or 4,
    /// `WireCombat.feedback`): an id, then the record's target and
    /// attacker (0 for none), position, teams and hit source, expiring 18
    /// ticks on.
    #[allow(clippy::too_many_arguments)]
    pub fn feedback(
        &self,
        context: &Context,
        tick: f64,
        schema: f64,
        target: Option<super::journal::Life>,
        attacker: Option<super::journal::Life>,
        position: [f64; 3],
        team: f64,
        attacker_team: f64,
        source: Option<[f64; 3]>,
    ) -> Result<bool, String> {
        let id = self.allocate(context)?;
        let encode = WbEncode {
            op: 1.0,
            schema,
            expiry_tick: tick + 18.0,
            target_id: target.map_or(0.0, |t| f64::from(t.id)),
            target_generation: target.map_or(0.0, |t| t.generation),
            attacker_id: attacker.map_or(0.0, |a| f64::from(a.id)),
            attacker_generation: attacker.map_or(0.0, |a| a.generation),
            position: position.to_vec(),
            team,
            attacker_team,
            has_source: source.is_some(),
            source: source.map_or_else(Vec::new, |s| s.to_vec()),
            ..WbEncode::default()
        };
        let payload = self.encode(context, &encode)?;
        let view = decode_view(tick, schema, &payload)?;
        self.emit_record(context, tick, id, schema, payload, &view)
    }

    /// `WireCombat.emit(capture, record, decode)` for a record whose id is
    /// allocated and whose payload is encoded: the key, the expiry, the
    /// commit with the fact `decodeCombat` read from it. False when the
    /// capture is full.
    pub fn emit_record(
        &self,
        context: &Context,
        tick: f64,
        id: f64,
        schema: f64,
        payload: Vec<u8>,
        view: &View,
    ) -> Result<bool, String> {
        let out = self.run(
            context,
            WbCall {
                op: 0.0,
                tick,
                capacity: MAX_RECORDS,
                key: 0.0,
                id,
                schema,
                expiry_tick: 0.0,
                text: StrId(0),
            },
        )?;
        if let Some(error) = out.error {
            let text = context.string(error).map_err(|e| e.message().to_string())?;
            if String::from_utf16_lossy(&text) == "combat capture capacity exceeded" {
                return Ok(false);
            }
            return Err(String::from_utf16_lossy(&text));
        }
        let key = out.key;
        self.run(
            context,
            WbCall {
                op: 1.0,
                tick: 0.0,
                capacity: 0.0,
                key,
                id: 0.0,
                schema,
                expiry_tick: view.expiry,
                text: StrId(0),
            },
        )?;
        let (owner_tag, owner) = match view.owner {
            None => (0, f64::NAN),
            Some(None) => (1, f64::NAN),
            Some(Some(n)) => (2, n),
        };
        let commit = WbCommit {
            key,
            fact: WireJournalFact {
                id,
                generation: GENERATION,
                tick,
                schema,
                payload: WireSpan {
                    offset: 0,
                    length: payload.len() as u32,
                },
                has_position: view.position.is_some(),
                position: view.position.unwrap_or([0.0; 3]),
                owner_tag,
                owner,
                speed: view.speed,
                life: view.life,
            },
            payload,
        };
        let mut row = vec![0.0; WbCommit::WORDS];
        let mut heap = Vec::new();
        commit.put(&mut row, &mut heap);
        match context.invoke_export(
            "wire_combat_commit",
            &Call {
                args: vec![f64::from(self.combat)],
                row,
                heap,
                ..Default::default()
            },
        ) {
            Ok(Returned::Scalar(v)) => Ok(v != 0.0),
            other => Err(format!("wire_combat_commit: {other:?}")),
        }
    }

    /// `CombatCapture.fault(error)` -> `WireCombat.fault`: the error's
    /// message into the capture's bounded fault journal (`run(6, {text})`).
    pub fn fault(&self, context: &Context, message: &str) -> Result<(), String> {
        let text = intern(context, message)?;
        self.run(
            context,
            WbCall {
                op: 6.0,
                tick: 0.0,
                capacity: 0.0,
                key: 0.0,
                id: 0.0,
                schema: 0.0,
                expiry_tick: 0.0,
                text,
            },
        )?;
        Ok(())
    }

    pub(super) fn run(&self, context: &Context, call: WbCall) -> Result<WbResult, String> {
        let mut row = vec![0.0; WbCall::WORDS];
        let mut heap = Vec::new();
        call.put(&mut row, &mut heap);
        match context.invoke_export(
            "wire_combat_run",
            &Call {
                args: vec![f64::from(self.combat)],
                row,
                heap,
                ..Default::default()
            },
        ) {
            Ok(Returned::Row(words)) => {
                let (row, heap) = super::block_row(&words);
                Ok(WbResult::take(row, heap))
            }
            other => Err(format!("wire_combat_run: {other:?}")),
        }
    }
}
