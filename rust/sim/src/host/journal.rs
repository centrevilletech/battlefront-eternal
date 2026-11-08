//! The page's `SimEvents` journal (src/sim-events.js) with the records the
//! game's observers write into it: a ring of 4096 slots, a sequence number
//! and the simulation time the owner sets each tick. `worldState` reads the
//! ring's filled slots in slot order (`ring.filter(Boolean)`), and the AI's
//! hearing (`AiJournalFacts.capture`) reads the records since its cursor.
//! Each record is kept as its `JSON.stringify` text, written with the
//! emitter's key order: `seq`, `time`, `kind`, then the fields.
use super::json::Json;

pub const CAPACITY: usize = 4096;

/// A reference as `AiJournalFacts.reference` reads it: a unit by its
/// store slot and the life the record named.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub slot: u32,
    pub generation: f64,
}

/// What the AI's hearing reads from a record (`AiJournalFacts.capture`
/// takes shots and damage only).
#[derive(Clone, Debug)]
pub enum Hearing {
    None,
    Shot {
        source: Option<Heard>,
    },
    Damage {
        target: Option<Heard>,
        attacker: Option<Heard>,
        amount: Option<f64>,
        generic: Option<f64>,
        typed: Option<f64>,
        known: bool,
        point: Option<[f64; 3]>,
    },
}

/// What a record keeps for readers other than `worldState`.
#[derive(Clone, Debug)]
pub struct Record {
    pub seq: usize,
    pub kind: &'static str,
    pub text: String,
    pub hearing: Hearing,
}

pub struct Journal {
    pub ring: Vec<Option<Record>>,
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
            ring: (0..CAPACITY).map(|_| None).collect(),
            seq: 0,
            time: 0.0,
        }
    }

    /// `emit(kind, fields)`: the record `{seq, time, kind, ...fields}`;
    /// `fields` is the JSON text of the fields after the opening brace
    /// (a `time` field there replaces the journal's, as the AI's voice
    /// notices do, in the second position).
    pub fn emit(&mut self, kind: &'static str, time: Option<f64>, fields: &str) {
        self.emit_heard(kind, time, fields, Hearing::None);
    }

    /// `emit` with what the AI's hearing takes from the record.
    pub fn emit_heard(
        &mut self,
        kind: &'static str,
        time: Option<f64>,
        fields: &str,
        hearing: Hearing,
    ) {
        let mut json = Json::new();
        json.begin_object();
        json.key("seq");
        json.number(self.seq as f64);
        json.key("time");
        json.number(time.unwrap_or(self.time));
        json.key("kind");
        json.string(kind);
        let mut text = json.text().to_string();
        if !fields.is_empty() {
            text.push(',');
            text.push_str(fields);
        }
        text.push('}');
        let index = self.seq % CAPACITY;
        self.ring[index] = Some(Record {
            seq: self.seq,
            kind,
            text,
            hearing,
        });
        self.seq += 1;
    }

    /// `AiJournalFacts.capture(journal)`: the shot and damage records since
    /// the cursor (at most the ring's capacity back), then the cursor at the
    /// head. Answers the records in sequence order.
    pub fn capture(&mut self, cursor: &mut usize) -> Vec<&Record> {
        let head = self.seq;
        if head < *cursor {
            *cursor = 0;
        }
        let start = (*cursor).max(head.saturating_sub(CAPACITY));
        let mut out = Vec::new();
        for seq in start..head {
            if let Some(record) = &self.ring[seq % CAPACITY] {
                if record.seq == seq
                    && matches!(
                        record.hearing,
                        Hearing::Shot { .. } | Hearing::Damage { .. }
                    )
                {
                    out.push(record);
                }
            }
        }
        *cursor = head;
        out
    }

    /// `g.simEvents.ring.filter(Boolean).map(eventValue)`: every filled
    /// slot in slot order, as JSON array elements.
    pub fn state_events(&self, json: &mut Json) {
        json.begin_array();
        for record in self.ring.iter().flatten() {
            json.raw(&record.text);
        }
        json.end_array();
    }
}
