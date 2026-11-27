//! The 2005 game's records and their revisions.
//!
//! Protocol 4 fixes every format a 2004 battle exchanges and the framing both
//! games share. Everything only a 2005 battle carries is a record of some kind,
//! each kind has numbered revisions, and every carrier marks a record with its
//! kind and revision so that a reader that does not know a kind can still find
//! where the record ends:
//!
//! - a shared world field of schema `0x0200 + kind`, whose payload starts with
//!   the revision (u16), its extent given by the field's length;
//! - an own correction body of schema `0x80 + kind`, starting with the
//!   revision, its extent given by the correction's counted body;
//! - a records section (in the 2005 input packet and own datagram): a count,
//!   then per record its kind (u8), revision (u16) and length (u16);
//! - a reliable message `{"type":"record","kind":K,"revision":R,"body":...}`;
//! - a checkpoint part, whose header names its kind and revision.
//!
//! For each 2005 match the server names, in its signed hello, the kinds the
//! match uses, each with one revision and whether a client must take part in
//! it: for a kind the server writes, the revision it writes; for a kind the
//! client writes, the revision the server reads. A joining client names, per
//! kind, the range of revisions it implements (reads for a kind the server
//! writes, writes for a kind it writes). A client that cannot take part in a
//! required kind is refused for that match alone, with the ordinary update
//! text. An optional kind a client does not implement is skipped where it is
//! read and never written. A record of a kind the match does not name is
//! refused. A later change to a 2005 format is a new revision of its kind,
//! never a new protocol number.
use serde_json::{json, Map, Value};

/// A record kind's number on the wire.
pub type Kind = u8;

/// The 2005 battle's input tables: actions, held codes and their meaning. The client writes it.
pub const INPUT: Kind = 1;
/// A 2005 soldier's own-body correction.
pub const SOLDIER: Kind = 2;
/// A 2005 soldier's shared display field.
pub const SOLDIER_DISPLAY: Kind = 3;
/// A hero's combat checkpoint, in checkpoint parts beside his own correction.
pub const COMBAT: Kind = 4;
/// A hero's combat display: his melee pose, the power he casts or holds and
/// the choke that holds him (`wire::melee`).
pub const COMBAT_DISPLAY: Kind = 5;
/// A hero's thrown saber: the press and the launch a viewer replays the flight from (`wire::thrown`).
pub const THROWN: Kind = 6;
/// A roster slot's points. Reserved: no producer yet.
pub const ROSTER_POINTS: Kind = 7;
/// A pilot's and a remote gunner's 2005 controls. The client writes it.
pub const PILOT: Kind = 16;
/// A starfighter's shared state.
pub const FIGHTER: Kind = 17;
/// A critical system the mission names: its life, health, shield and place.
pub const SYSTEM: Kind = 18;
/// The world animation clock that moves frigates and props.
pub const FRIGATES: Kind = 19;
/// Guided ordnance, launches, impacts and destruction bursts.
pub const ORDNANCE: Kind = 20;
/// Space turret aims and interior proximity doors.
pub const SPACE_PROPS: Kind = 21;
/// The script's persistent match state: scores, limits, timers, objectives.
pub const MATCH_STATE: Kind = 24;
/// The script's targeted notifications.
pub const NOTICE: Kind = 25;
/// The notice that a client sits the match out, and why.
pub const SIT_OUT: Kind = 26;
/// The score table a client sitting the match out is shown.
pub const BOARD: Kind = 27;
/// The 2005 battle's dictionaries in its entity registry.
pub const REGISTRY: Kind = 28;

/// Which side writes records of a kind this build knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer { Server, Client }

/// The writer of a known kind; `None` for a kind this build does not know.
pub fn writer(kind: Kind) -> Option<Writer> {
    match kind {
        INPUT | PILOT => Some(Writer::Client),
        SOLDIER | SOLDIER_DISPLAY | COMBAT | COMBAT_DISPLAY | THROWN | ROSTER_POINTS | FIGHTER | SYSTEM | FRIGATES | ORDNANCE
        | SPACE_PROPS | MATCH_STATE | NOTICE | SIT_OUT | BOARD | REGISTRY => Some(Writer::Server),
        _ => None,
    }
}

/// The most kinds one match may name or one client may implement.
pub const MAX_KINDS: usize = 32;
/// The longest canonical text of a table (4 bytes a kind) and of an offer (5
/// bytes a kind) at that bound; longer text is refused before it is decoded.
pub const MAX_TABLE_TEXT: usize = (MAX_KINDS * 4).div_ceil(3) * 4;
pub const MAX_OFFER_TEXT: usize = (MAX_KINDS * 5).div_ceil(3) * 4;

/// The kinds no client can take part in a 2005 match without: the input it
/// writes, its own soldier's correction and the notice of a sit-out role.
/// Every match names each, required, so a client that cannot use the
/// match's revision of any of them is refused, never left to skip it.
pub const INDISPENSABLE: [Kind; 3] = [INPUT, SOLDIER, SIT_OUT];

/// A kind's revision in one match, and whether a client must take part in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub revision: u16,
    pub required: bool,
}

/// The revisions of a kind a client implements: every revision from `min` to `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub kind: Kind,
    pub min: u16,
    pub max: u16,
}

/// What this build's server uses in a 2005 ground match. Drafts are absent.
pub const WRITES: &[Entry] = &[
    Entry { kind: INPUT, revision: 1, required: true },
    Entry { kind: SOLDIER, revision: 1, required: true },
    Entry { kind: SOLDIER_DISPLAY, revision: 1, required: false },
    Entry { kind: COMBAT_DISPLAY, revision: super::melee::REVISION, required: false },
    Entry { kind: THROWN, revision: super::thrown::REVISION, required: false },
    Entry { kind: SYSTEM, revision: 1, required: false },
    Entry { kind: MATCH_STATE, revision: 1, required: true },
    Entry { kind: NOTICE, revision: 1, required: false },
    Entry { kind: SIT_OUT, revision: 1, required: true },
    Entry { kind: BOARD, revision: 1, required: false },
    Entry { kind: REGISTRY, revision: 1, required: true },
];

/// What this build's server uses in a 2005 Space Assault match.
pub const WRITES_SPACE: &[Entry] = &[
    Entry { kind: INPUT, revision: 1, required: true },
    Entry { kind: SOLDIER, revision: 1, required: true },
    Entry { kind: SOLDIER_DISPLAY, revision: 1, required: false },
    Entry { kind: COMBAT_DISPLAY, revision: super::melee::REVISION, required: false },
    Entry { kind: THROWN, revision: super::thrown::REVISION, required: false },
    Entry { kind: PILOT, revision: 1, required: true },
    Entry { kind: FIGHTER, revision: 1, required: true },
    Entry { kind: SYSTEM, revision: 1, required: false },
    Entry { kind: FRIGATES, revision: 1, required: true },
    Entry { kind: ORDNANCE, revision: 1, required: true },
    Entry { kind: SPACE_PROPS, revision: 1, required: true },
    Entry { kind: MATCH_STATE, revision: 1, required: true },
    Entry { kind: NOTICE, revision: 1, required: false },
    Entry { kind: SIT_OUT, revision: 1, required: true },
    Entry { kind: BOARD, revision: 1, required: false },
    Entry { kind: REGISTRY, revision: 1, required: true },
];

/// This build's entry for a kind its server writes.
pub fn written(kind: Kind) -> Option<Entry> {
    WRITES_SPACE.iter().copied().find(|entry| entry.kind == kind)
}

/// What this build's client implements.
pub const SUPPORTS: &[Support] = &[
    Support { kind: INPUT, min: 1, max: 1 },
    Support { kind: SOLDIER, min: 1, max: 1 },
    Support { kind: SOLDIER_DISPLAY, min: 1, max: 1 },
    Support { kind: COMBAT_DISPLAY, min: super::melee::REVISION, max: super::melee::REVISION },
    Support { kind: THROWN, min: super::thrown::REVISION, max: super::thrown::REVISION },
    Support { kind: PILOT, min: 1, max: 1 },
    Support { kind: FIGHTER, min: 1, max: 1 },
    Support { kind: SYSTEM, min: 1, max: 1 },
    Support { kind: FRIGATES, min: 1, max: 1 },
    Support { kind: ORDNANCE, min: 1, max: 1 },
    Support { kind: SPACE_PROPS, min: 1, max: 1 },
    Support { kind: MATCH_STATE, min: 1, max: 1 },
    Support { kind: NOTICE, min: 1, max: 1 },
    Support { kind: SIT_OUT, min: 1, max: 1 },
    Support { kind: BOARD, min: 1, max: 1 },
    Support { kind: REGISTRY, min: 1, max: 1 },
];

const MALFORMED: &str = "invalid Battlefront II record table";

// Standard padded base64: the tables travel as text in the signed hellos.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() { out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])); } else { out.push('='); }
        }
    }
    out
}

/// Canonical padded base64 only: any other text, including a different
/// spelling of the same bytes, is refused.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 { return None; }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (at, chunk) in bytes.chunks(4).enumerate() {
        let last = at + 1 == bytes.len() / 4;
        let pad = chunk.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 || (pad > 0 && !last) { return None; }
        let mut n = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            let value = if i >= 4 - pad { 0 } else { ALPHABET.iter().position(|&c| c == b)? as u32 };
            n |= value << (18 - 6 * i);
        }
        let take = 3 - pad;
        // Bits beyond the encoded bytes must be zero.
        if n & ((1 << (8 * (3 - take))) - 1) != 0 { return None; }
        out.extend_from_slice(&n.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

/// A match's record table, as its signed hello names it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub entries: Vec<Entry>,
}

impl Table {
    pub fn of(entries: &[Entry]) -> Self { Self { entries: entries.to_vec() } }
    /// Four bytes per kind in ascending kind order (kind, revision u16, flags
    /// with bit 0 required), as base64.
    pub fn text(&self) -> String {
        let mut bytes = Vec::with_capacity(self.entries.len() * 4);
        for e in &self.entries {
            bytes.push(e.kind);
            bytes.extend(e.revision.to_le_bytes());
            bytes.push(u8::from(e.required));
        }
        base64(&bytes)
    }
    /// A table as a signed hello names it: at most 32 kinds in strictly
    /// ascending order, each revision at least 1, flags 0 or 1. Anything else
    /// refuses the whole table.
    pub fn read(text: &str) -> Result<Self, &'static str> {
        if text.len() > MAX_TABLE_TEXT { return Err(MALFORMED); }
        let bytes = unbase64(text).filter(|b| b.len() % 4 == 0 && b.len() / 4 <= MAX_KINDS).ok_or(MALFORMED)?;
        let mut entries: Vec<Entry> = Vec::with_capacity(bytes.len() / 4);
        for row in bytes.chunks(4) {
            let entry = Entry { kind: row[0], revision: u16::from_le_bytes([row[1], row[2]]), required: row[3] == 1 };
            if entry.kind == 0 || entry.revision == 0 || row[3] > 1 || entries.last().is_some_and(|e| e.kind >= entry.kind) {
                return Err(MALFORMED);
            }
            entries.push(entry);
        }
        Ok(Self { entries })
    }
    /// The entry of a kind, if the match uses it.
    pub fn entry(&self, kind: Kind) -> Option<Entry> {
        self.entries.iter().copied().find(|e| e.kind == kind)
    }
}

/// The kinds one client implements, as its hello names them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Supports {
    pub kinds: Vec<Support>,
}

impl Supports {
    pub fn of(kinds: &[Support]) -> Self { Self { kinds: kinds.to_vec() } }
    /// Five bytes per kind in ascending kind order (kind, min u16, max u16), as base64.
    pub fn text(&self) -> String {
        let mut bytes = Vec::with_capacity(self.kinds.len() * 5);
        for s in &self.kinds {
            bytes.push(s.kind);
            bytes.extend(s.min.to_le_bytes());
            bytes.extend(s.max.to_le_bytes());
        }
        base64(&bytes)
    }
    pub fn read(text: &str) -> Result<Self, &'static str> {
        if text.len() > MAX_OFFER_TEXT { return Err(MALFORMED); }
        let bytes = unbase64(text).filter(|b| b.len() % 5 == 0 && b.len() / 5 <= MAX_KINDS).ok_or(MALFORMED)?;
        let mut kinds: Vec<Support> = Vec::with_capacity(bytes.len() / 5);
        for row in bytes.chunks(5) {
            let s = Support { kind: row[0], min: u16::from_le_bytes([row[1], row[2]]), max: u16::from_le_bytes([row[3], row[4]]) };
            if s.kind == 0 || s.min == 0 || s.min > s.max || kinds.last().is_some_and(|k| k.kind >= s.kind) {
                return Err(MALFORMED);
            }
            kinds.push(s);
        }
        Ok(Self { kinds })
    }
    fn takes(&self, entry: Entry) -> bool {
        self.kinds.iter().any(|s| s.kind == entry.kind && (s.min..=s.max).contains(&entry.revision))
    }
    fn newest(&self, kind: Kind) -> Option<u16> {
        self.kinds.iter().find(|s| s.kind == kind).map(|s| s.max)
    }
}

/// Why a client cannot take part in a match: a required kind it does not
/// implement in the match's revision (the lowest such kind), and which side
/// is older.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub kind: Kind,
    pub revision: u16,
    pub client_older: bool,
}

impl Unreadable {
    /// The player's words, for this match only.
    pub fn text(self) -> String {
        if self.client_older {
            "Your game is older than this server's Battlefront II battle. Update Battlefront Eternal to join it.".into()
        } else {
            "This server's Battlefront II battle runs an older version of Battlefront Eternal. It can be joined once the server updates.".into()
        }
    }
}

/// Whether a client implementing `supports` can take part in a match using `table`.
pub fn check(table: &Table, supports: &Supports) -> Result<(), Unreadable> {
    for entry in table.entries.iter().copied().filter(|e| e.required) {
        if !supports.takes(entry) {
            // A client whose newest revision of the kind is below the match's
            // (or that never heard of the kind) is the older side.
            let client_older = supports.newest(entry.kind).is_none_or(|newest| newest < entry.revision);
            return Err(Unreadable { kind: entry.kind, revision: entry.revision, client_older });
        }
    }
    Ok(())
}

/// What one client does with a kind in a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// The match does not name the kind: a record of it is refused.
    Unnamed,
    /// An optional kind this client does not implement, at the revision the
    /// match advertises for it: a record of that revision is skipped where it
    /// is read, once its carrier's kind, revision and extent check out, and
    /// never written; a record of any other revision is refused.
    Skip(u16),
    /// The revision both sides use.
    Revision(u16),
}

/// What a client takes part in of a match it joined: for each kind the match
/// names, the revision both sides use or the advertised revision of an
/// optional kind it skips. Both ends compute the same plan from the signed
/// table and the client's supports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub kinds: Vec<(Kind, Use)>,
}

impl Plan {
    pub fn of(&self, kind: Kind) -> Use {
        self.kinds.iter().find(|(k, _)| *k == kind).map_or(Use::Unnamed, |&(_, used)| used)
    }
    /// Whether a present record of `kind` at `revision` is skipped (true) or
    /// read (false): a kind the match does not name, or a revision other than
    /// the one the match advertises for it, is refused.
    pub fn skips(&self, kind: Kind, revision: u16) -> Result<bool, &'static str> {
        match self.of(kind) {
            Use::Skip(advertised) if advertised == revision => Ok(true),
            Use::Revision(used) if used == revision => Ok(false),
            Use::Unnamed => Err("a Battlefront II record this match does not carry"),
            Use::Skip(_) | Use::Revision(_) => Err("a Battlefront II record of another revision than the match's"),
        }
    }
    /// The revision both sides use of a kind, refusing a kind the match does
    /// not name and an optional one this client skips.
    pub fn revision(&self, kind: Kind) -> Result<u16, &'static str> {
        match self.of(kind) { Use::Revision(r) => Ok(r), _ => Err("a Battlefront II record this match does not carry") }
    }
}

/// The plan of a client implementing `supports` in a match using `table`.
pub fn plan(table: &Table, supports: &Supports) -> Result<Plan, Unreadable> {
    check(table, supports)?;
    Ok(Plan { kinds: table.entries.iter()
        .map(|e| (e.kind, if supports.takes(*e) { Use::Revision(e.revision) } else { Use::Skip(e.revision) }))
        .collect() })
}

/// A 2005 match as the server's signed hello names it: the game, mode and
/// qualified map, the match's own token (two matches of one selection never
/// share it) and its record table. A 2004 battle's hello has no descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub mode: String,
    pub map: String,
    pub token: String,
    pub records: Table,
}

/// What a client asks for when it joins a 2005 match: the match discovery
/// described and the kinds it implements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub mode: String,
    pub map: String,
    pub token: String,
    pub supports: Supports,
}

const MATCH: &str = "invalid Battlefront II match";

// A qualified 2005 map id: the family, a slash, then 1 to 32 lowercase
// letters, digits or underscores.
fn map_id(map: &str) -> bool {
    map.strip_prefix("swbf2/").is_some_and(|id| (1..=32).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
}
/// A match token as every server writes it: 32 lowercase hexadecimal digits.
pub fn match_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    object.get(key).and_then(Value::as_str).ok_or(MATCH)
}
// The fields both objects share, checked before anything else is read.
fn match_fields<'a>(value: &'a Value, list: &str) -> Result<(String, String, String, &'a str), &'static str> {
    let object = value.as_object().ok_or(MATCH)?;
    if object.len() != 5 || text(object, "family")? != "swbf2" { return Err(MATCH); }
    let (mode, map, token_text) = (text(object, "mode")?, text(object, "map")?, text(object, "match")?);
    if !bfe_family::mode_token(mode) || !map_id(map) || !match_token(token_text) { return Err(MATCH); }
    Ok((mode.into(), map.into(), token_text.into(), text(object, list)?))
}

impl Descriptor {
    pub fn json(&self) -> Value {
        json!({"family": "swbf2", "mode": self.mode, "map": self.map, "match": self.token, "records": self.records.text()})
    }
    /// A descriptor as a signed hello names it. A table that leaves out an
    /// indispensable kind, or marks one optional, is malformed.
    pub fn read(value: &Value) -> Result<Self, &'static str> {
        let (mode, map, token, records) = match_fields(value, "records")?;
        let records = Table::read(records)?;
        if !INDISPENSABLE.iter().all(|&kind| records.entry(kind).is_some_and(|e| e.required)) { return Err(MALFORMED); }
        Ok(Self { mode, map, token, records })
    }
}

impl Request {
    pub fn json(&self) -> Value {
        json!({"family": "swbf2", "mode": self.mode, "map": self.map, "match": self.token, "supports": self.supports.text()})
    }
    pub fn read(value: &Value) -> Result<Self, &'static str> {
        let (mode, map, token, supports) = match_fields(value, "supports")?;
        Ok(Self { mode, map, token, supports: Supports::read(supports)? })
    }
    /// Whether the request names this very match (not merely its selection).
    pub fn names(&self, descriptor: &Descriptor) -> bool {
        (self.mode.as_str(), self.map.as_str(), self.token.as_str()) == (descriptor.mode.as_str(), descriptor.map.as_str(), descriptor.token.as_str())
    }
}

/// The base of the 2005 world field schemas: field `0x0200 + kind` carries
/// records of that kind.
pub const FIELD_BASE: u16 = 0x0200;

/// The kind a world field schema carries, for every schema of the 2005 range.
pub fn field_kind(schema: u16) -> Option<Kind> {
    (FIELD_BASE + 1..=FIELD_BASE + 0xff).contains(&schema).then(|| (schema - FIELD_BASE) as Kind)
}

/// The world field schema of a record kind.
pub const fn field_schema(kind: Kind) -> u16 { FIELD_BASE + kind as u16 }

/// The base of the 2005 own correction body schemas: body `0x80 + kind`
/// carries records of that kind.
pub const BODY_BASE: u8 = 0x80;

/// The kind an own correction body schema carries, for every schema of the 2005 range.
pub fn body_kind(schema: u8) -> Option<Kind> {
    (schema > BODY_BASE).then(|| schema - BODY_BASE)
}

/// The own correction body schema of a record kind.
pub const fn body_schema(kind: Kind) -> u8 { BODY_BASE + kind }

const RECORD: &str = "invalid Battlefront II record";

/// A record's revision and body: two leading bytes of a field payload or own
/// body, then the rest.
pub fn split_revision(payload: &[u8]) -> Result<(u16, &[u8]), &'static str> {
    match payload.split_first_chunk::<2>() {
        Some((revision, body)) => Ok((u16::from_le_bytes(*revision), body)),
        None => Err(RECORD),
    }
}

/// One record of a records section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section<'a> {
    pub kind: Kind,
    pub revision: u16,
    pub body: &'a [u8],
}

/// The most records one section holds.
pub const SECTION_RECORDS: usize = 16;

/// Writes a records section: a count, then per record its kind, revision,
/// length (u16) and body.
pub fn write_section(records: &[Section<'_>], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if records.len() > SECTION_RECORDS { return Err(RECORD); }
    out.push(records.len() as u8);
    for r in records {
        let length = u16::try_from(r.body.len()).map_err(|_| RECORD)?;
        out.push(r.kind);
        out.extend(r.revision.to_le_bytes());
        out.extend(length.to_le_bytes());
        out.extend_from_slice(r.body);
    }
    Ok(())
}

/// Reads a records section from the start of `bytes` into `out` with zero heap
/// allocations, returning the record count and total bytes consumed. Every
/// length is checked before it is used.
pub fn read_section_into<'a>(
    bytes: &'a [u8],
    out: &mut [Section<'a>; SECTION_RECORDS],
) -> Result<(usize, usize), &'static str> {
    let (&count, mut rest) = bytes.split_first().ok_or(RECORD)?;
    let n = usize::from(count);
    if n > SECTION_RECORDS {
        return Err(RECORD);
    }
    for slot in out.iter_mut().take(n) {
        let (head, tail) = rest.split_first_chunk::<5>().ok_or(RECORD)?;
        let length = usize::from(u16::from_le_bytes([head[3], head[4]]));
        // Kind and revision 0 do not exist, whether the reader then takes the
        // record or skips it.
        if tail.len() < length || head[0] == 0 || (head[1], head[2]) == (0, 0) {
            return Err(RECORD);
        }
        let (body, after) = tail.split_at(length);
        *slot = Section {
            kind: head[0],
            revision: u16::from_le_bytes([head[1], head[2]]),
            body,
        };
        rest = after;
    }
    Ok((n, bytes.len() - rest.len()))
}

/// Reads a records section from the start of `bytes`: its records and the
/// bytes it took. Every length is checked before it is used.
pub fn read_section(bytes: &[u8]) -> Result<(Vec<Section<'_>>, usize), &'static str> {
    let mut scratch = [Section { kind: 0, revision: 0, body: &[] }; SECTION_RECORDS];
    let (count, used) = read_section_into(bytes, &mut scratch)?;
    Ok((scratch[..count].to_vec(), used))
}

/// What a reader does with one record of a section in a match: its body when
/// the reader takes part in the kind at the record's revision, nothing for an
/// optional kind it skips at the revision the match advertises; a kind the
/// match does not name, or any other revision, is refused. The section's
/// reader has already checked the record's kind, revision and extent.
pub fn sectioned<'a>(plan: &Plan, record: &Section<'a>) -> Result<Option<&'a [u8]>, &'static str> {
    match plan.skips(record.kind, record.revision).map_err(|_| RECORD)? {
        true => Ok(None),
        false => Ok(Some(record.body)),
    }
}

/// A reliable message carrying one record.
pub fn record_message(kind: Kind, revision: u16, body: Value) -> Value {
    json!({"type": "record", "kind": kind, "revision": revision, "body": body})
}

/// The kind, revision and body of a reliable record message, read strictly:
/// exactly its four keys, a known shape for each.
pub fn read_record(message: &Value) -> Result<(Kind, u16, &Value), &'static str> {
    let object = message.as_object().filter(|o| o.len() == 4).ok_or(RECORD)?;
    if object.get("type").and_then(Value::as_str) != Some("record") { return Err(RECORD); }
    let kind = object.get("kind").and_then(Value::as_u64).and_then(|k| Kind::try_from(k).ok()).filter(|&k| k != 0).ok_or(RECORD)?;
    let revision = object.get("revision").and_then(Value::as_u64).and_then(|r| u16::try_from(r).ok()).filter(|&r| r != 0).ok_or(RECORD)?;
    Ok((kind, revision, object.get("body").ok_or(RECORD)?))
}

/// What a reader does with a reliable record in a match, as for a section's:
/// the message's kind and revision are read strictly before it is skipped.
pub fn recorded<'a>(plan: &Plan, message: &'a Value) -> Result<Option<(Kind, &'a Value)>, &'static str> {
    let (kind, revision, body) = read_record(message)?;
    match plan.skips(kind, revision).map_err(|_| RECORD)? {
        true => Ok(None),
        false => Ok(Some((kind, body))),
    }
}

/// The newest own correction a 2005 receiver accepted in its epoch: its
/// datagram's epoch and tick, and the unit, life and class it names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accepted {
    pub epoch: f64,
    pub tick: f64,
    pub unit: u32,
    pub life: u32,
    pub class: u16,
}

/// A 2005 match's receiver checks: the plan of the match, the match's class
/// table (each class's movement numbers, none for a class without them, such
/// as a droideka's) for its own corrections, and the newest correction it
/// accepted.
pub struct Checks {
    pub plan: Plan,
    pub classes: Vec<Option<crate::movement::soldier::swbf2::Tuning>>,
    pub accepted: std::cell::Cell<Option<Accepted>>,
}

impl Checks {
    pub fn new(plan: Plan, classes: Vec<Option<crate::movement::soldier::swbf2::Tuning>>) -> Self {
        Self { plan, classes, accepted: std::cell::Cell::new(None) }
    }
}

impl bfe_presentation::network::RecordChecks for Checks {
    fn reads(&self, schema: u16) -> Result<bool, &'static str> {
        let Some(kind) = field_kind(schema) else { return Ok(true) };
        match self.plan.of(kind) {
            Use::Unnamed => Err("a Battlefront II record this match does not carry"),
            Use::Skip(_) => Ok(false),
            Use::Revision(_) => Ok(true),
        }
    }
    fn skipped(&self, schema: u16, payload: &[u8]) -> Result<(), &'static str> {
        // A skipped record's whole payload, rebuilt from its base like any
        // field's, shows the revision the match advertises for its kind.
        let kind = field_kind(schema).ok_or(RECORD)?;
        let (revision, _) = split_revision(payload)?;
        match self.plan.skips(kind, revision) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(RECORD),
        }
    }
    fn field(&self, schema: u16, id: u32, payload: &[u8]) -> Result<(), &'static str> {
        let Some(kind) = field_kind(schema) else { return Ok(()) };
        let (revision, body) = split_revision(payload)?;
        if revision != self.plan.revision(kind)? { return Err(RECORD); }
        match (kind, revision) {
            (SOLDIER_DISPLAY, 1) => {
                if !id.checked_sub(100_000).is_some_and(|slot| (slot as usize) < super::records::SLOT_COUNT) {
                    return Err(RECORD);
                }
                crate::movement::soldier::swbf2::body::read_display(body).map(drop)
            }
            // A hero's display and thrown saber, keyed like his soldier's display;
            // their readers take the whole payload, revision first.
            (COMBAT_DISPLAY, super::melee::REVISION) => {
                if !id.checked_sub(100_000).is_some_and(|slot| (slot as usize) < super::records::SLOT_COUNT) {
                    return Err(RECORD);
                }
                super::melee::Display::decode(payload).map(drop).ok_or(RECORD)
            }
            (THROWN, super::thrown::REVISION) => {
                if !id.checked_sub(100_000).is_some_and(|slot| (slot as usize) < super::records::SLOT_COUNT) {
                    return Err(RECORD);
                }
                super::thrown::Throw::decode(payload).map(drop).ok_or(RECORD)
            }
            (FIGHTER, super::space::REVISION) => super::space::validate_fighter_field(id, body),
            (FRIGATES, super::space::REVISION) => super::space::validate_frigates(id, body),
            (ORDNANCE, super::space::REVISION) => super::space::validate_ordnance_field(id, body),
            (SPACE_PROPS, super::space::REVISION) => super::space::validate_props_field(id, body),
            (SYSTEM, super::mission::REVISION) => {
                let system = super::mission::read_system(body)?;
                if u32::from(system.index) != id { return Err(RECORD); }
                Ok(())
            }
            (MATCH_STATE, super::mission::REVISION) => {
                let state = super::mission::read_state(body)?;
                if state.id() != id { return Err(RECORD); }
                Ok(())
            }
            // A kind this build implements has a reader for each revision it implements.
            _ => Err(RECORD),
        }
    }
    fn correction(&self, epoch: f64, tick: f64, id: u32, generation: u32, schema: u16, payload: &[u8]) -> Result<(), &'static str> {
        // The envelope schema, the payload whole, then the subject: one life
        // of one unit is one class, in whatever order its corrections arrive.
        // The accepted identity is the newest correction's of the epoch: a
        // late correction of an earlier tick (an earlier life, or the unit
        // before a respawn) is the server's own and is read, but never
        // replaces it; a new epoch starts afresh.
        if schema != 11 { return Err("invalid Battlefront II correction"); }
        let class = check_correction(payload, &self.plan, &self.classes)?;
        let accepted = self.accepted.get();
        if let Some(current) = accepted.filter(|a| a.epoch == epoch) {
            if (current.unit, current.life) == (id, generation) && current.class != class {
                return Err("a Battlefront II correction that changes its life's class");
            }
            if tick < current.tick { return Ok(()); }
        } else if accepted.is_some_and(|a| epoch < a.epoch) {
            return Ok(());
        }
        self.accepted.set(Some(Accepted { epoch, tick, unit: id, life: generation, class }));
        Ok(())
    }
}

/// A 2005 own correction checked whole: the complete envelope (header,
/// counted body, life facts, weapons, controls, a mount or remote, nothing
/// trailing), a class of the match's table, then its body by schema with
/// every split checked: the 2005 soldier body at the match's revision against
/// the class's numbers and the compact body it holds, or a 2004 vehicle or
/// droideka body by its own decoder. A 2004 soldier body, or a 2005 body of
/// another kind, is refused. Returns the class.
pub fn check_correction(payload: &[u8], plan: &Plan, classes: &[Option<crate::movement::soldier::swbf2::Tuning>]) -> Result<u16, &'static str> {
    use crate::movement::soldier::swbf2::body;
    use crate::prediction::corrections::{decode_compact_flight, decode_compact_soldier_fixed, decode_droideka, decode_ground, decode_own_player};
    const INVALID: &str = "invalid Battlefront II soldier correction";
    let own = decode_own_player(Some(payload), true)?;
    let bytes = own.body.get(payload).ok_or(INVALID)?;
    let class = classes.get(usize::from(own.class_id)).filter(|_| own.class_id != 0).ok_or("a class this Battlefront II match does not have")?;
    match own.body_schema {
        body::SCHEMA => {
            let (revision, rest) = split_revision(bytes)?;
            if revision != plan.revision(SOLDIER)? || revision != body::REVISION { return Err(INVALID); }
            let tuning = class.ok_or("no movement numbers for this class")?;
            let (compact, suffix) = rest.split_at_checked(body::compact_len(rest).ok_or(INVALID)?).ok_or(INVALID)?;
            let compact = decode_compact_soldier_fixed(Some(compact), &[true; 6])?;
            let (state, _lock) = body::split_lock(suffix)?;
            body::read_suffix(state, &tuning, body::Body::of(&compact))?;
        }
        super::space::OWN_BODY_SCHEMA => {
            let (revision, rest) = split_revision(bytes)?;
            if revision != plan.revision(FIGHTER)? || revision != super::space::REVISION {
                return Err(INVALID);
            }
            super::space::read_own_body(rest)?;
        }
        6 => return Err("a 2004 soldier body in a Battlefront II match"),
        7 => { decode_compact_flight(Some(bytes), true, [true; 9], [true; 4])?; }
        10 if own.vehicle_id == 0 => { decode_droideka(Some(bytes))?; }
        10 => { decode_ground(Some(bytes), true, [true; 2])?; }
        _ => return Err(INVALID),
    }
    Ok(own.class_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supports() -> Supports { Supports::of(SUPPORTS) }

    #[test]
    fn this_build_takes_part_in_everything_it_writes() {
        assert_eq!(Supports::read(&supports().text()).unwrap(), supports());
        for writes in [WRITES, WRITES_SPACE] {
            let table = Table::of(writes);
            assert_eq!(Table::read(&table.text()).unwrap(), table);
            let plan = plan(&table, &supports()).unwrap();
            assert!(plan.kinds.iter().all(|(_, used)| matches!(used, Use::Revision(_))));
            assert!(writes.iter().all(|e| e.revision > 0 && writer(e.kind).is_some()));
        }
    }

    // A server one revision ahead and a client one revision ahead, with a
    // future kind of either importance.
    #[test]
    fn a_future_revision_refuses_only_where_it_is_required() {
        let ours = supports();
        let future_soldier = Table::of(&[Entry { kind: INPUT, revision: 1, required: true }, Entry { kind: SOLDIER, revision: 2, required: true }]);
        assert_eq!(check(&future_soldier, &ours), Err(Unreadable { kind: SOLDIER, revision: 2, client_older: true }));
        assert!(check(&future_soldier, &ours).unwrap_err().text().contains("Update Battlefront Eternal"));
        // A future optional kind is skipped; a future required kind refuses.
        let optional = Table::of(&[WRITES[0], WRITES[1], Entry { kind: 99, revision: 1, required: false }]);
        assert_eq!(plan(&optional, &ours).unwrap().of(99), Use::Skip(1));
        let required = Table::of(&[WRITES[0], WRITES[1], Entry { kind: 99, revision: 1, required: true }]);
        assert!(check(&required, &ours).unwrap_err().client_older);
        // A newer client still implementing revision 1 joins this server; one
        // that dropped it is the newer side.
        let newer = Supports::of(&[Support { kind: INPUT, min: 1, max: 2 }, Support { kind: SOLDIER, min: 1, max: 2 },
            Support { kind: SOLDIER_DISPLAY, min: 2, max: 2 }]);
        let joined = plan(&Table::of(&WRITES[..3]), &newer).unwrap();
        assert_eq!((joined.of(INPUT), joined.of(SOLDIER_DISPLAY), joined.of(BOARD)), (Use::Revision(1), Use::Skip(1), Use::Unnamed));
        let dropped = Supports::of(&[Support { kind: INPUT, min: 2, max: 2 }, Support { kind: SOLDIER, min: 2, max: 2 }]);
        assert_eq!(check(&Table::of(WRITES), &dropped), Err(Unreadable { kind: INPUT, revision: 1, client_older: false }));
    }

    // An optional kind skipped at the revision the match advertises: every
    // carrier checks a present record's kind and revision against it.
    #[test]
    fn every_carrier_checks_a_skipped_record_against_the_advertised_revision() {
        let plan = plan(&Table::of(&[WRITES[0], WRITES[1], written(SIT_OUT).unwrap(), Entry { kind: 91, revision: 2, required: false }]), &supports()).unwrap();
        assert_eq!(plan.of(91), Use::Skip(2));
        assert_eq!((plan.skips(91, 2), plan.skips(INPUT, 1)), (Ok(true), Ok(false)));
        assert!(plan.skips(91, 3).is_err() && plan.skips(90, 2).is_err() && plan.skips(INPUT, 2).is_err());
        // A reliable record message.
        assert_eq!(recorded(&plan, &record_message(91, 2, json!({"x": 1}))), Ok(None));
        assert_eq!(recorded(&plan, &record_message(91, 3, json!({"x": 1}))), Err(RECORD));
        assert!(recorded(&plan, &json!({"type": "record", "kind": 91, "revision": 0, "body": {}})).is_err());
        // A binary section, one record of one byte.
        let section = |revision: u16| [&[1, 91][..], &revision.to_le_bytes(), &1u16.to_le_bytes(), &[7]].concat();
        let read = |revision: u16| read_section(&section(revision)).map(|(records, _)| sectioned(&plan, &records[0]).map(|body| body.map(<[u8]>::to_vec)));
        assert_eq!(read(2), Ok(Ok(None)));
        assert_eq!(read(3), Ok(Err(RECORD)));
        assert!(read(0).is_err());
    }

    #[test]
    fn a_match_is_named_whole_and_strictly() {
        let token = "0a1b2c3d4e5f60718293a4b5c6d7e8f9";
        let descriptor = Descriptor { mode: "eli".into(), map: "swbf2/kam9".into(), token: token.into(), records: Table::of(WRITES) };
        assert_eq!(Descriptor::read(&descriptor.json()).unwrap(), descriptor);
        let request = Request { mode: "eli".into(), map: "swbf2/kam9".into(), token: token.into(), supports: supports() };
        assert_eq!(Request::read(&request.json()).unwrap(), request);
        assert!(request.names(&descriptor));
        assert!(!Request { token: token.replace('0', "1"), ..request.clone() }.names(&descriptor));
        let good = descriptor.json();
        for (key, value) in [("family", json!("swbf1")), ("mode", json!("Eli")), ("map", json!("kam9")), ("map", json!("swbf2/")),
            ("match", json!("0a1b")), ("match", json!(token.to_uppercase())), ("records", json!(null)), ("records", json!("AQ==")),
            ("extra", json!(1))]
        {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(Descriptor::read(&bad).is_err(), "{key}");
        }
        // A table without the input tables or the soldier body is no 2005 match's.
        let empty = Descriptor { records: Table::default(), ..descriptor.clone() }.json();
        assert!(Descriptor::read(&empty).is_err());
        assert!(Request::read(&good).is_err());
    }

    #[test]
    fn tables_are_canonical_and_bounded() {
        let row = |kind: u8, revision: u16, flags: u8| { let mut b = vec![kind]; b.extend(revision.to_le_bytes()); b.push(flags); b };
        for bad in [row(1, 0, 1), row(0, 1, 1), row(1, 1, 2), [row(2, 1, 1), row(1, 1, 1)].concat(), [row(1, 1, 1), row(1, 2, 0)].concat(),
            (1..=33u8).flat_map(|k| row(k, 1, 0)).collect()]
        {
            assert!(Table::read(&base64(&bad)).is_err(), "{bad:?}");
        }
        assert!(Table::read("AQEA").is_err());
        assert!(Table::read("AQEAAA").is_err());
        // The same row spelled with nonzero bits past its last byte.
        assert!(Table::read("AQEAAR==").is_err());
        assert_eq!(Table::read("AQEAAQ==").unwrap().entries, [Entry { kind: INPUT, revision: 1, required: true }]);
        let support = |kind: u8, min: u16, max: u16| { let mut b = vec![kind]; b.extend(min.to_le_bytes()); b.extend(max.to_le_bytes()); b };
        for bad in [support(1, 0, 1), support(1, 2, 1), [support(2, 1, 1), support(2, 1, 2)].concat(), support(0, 1, 1)] {
            assert!(Supports::read(&base64(&bad)).is_err(), "{bad:?}");
        }
        for bytes in [vec![], vec![1], vec![1, 2], vec![255, 0, 7, 9, 200]] { assert_eq!(unbase64(&base64(&bytes)), Some(bytes)); }
    }

    #[test]
    fn the_largest_offer_and_table_fit_a_hello() {
        let offer = Supports { kinds: (1..=32).map(|kind| Support { kind, min: 1, max: u16::MAX }).collect() };
        let table = Table { entries: (1..=32).map(|kind| Entry { kind, revision: u16::MAX, required: true }).collect() };
        assert_eq!((offer.text().len(), table.text().len()), (MAX_OFFER_TEXT, MAX_TABLE_TEXT));
        assert_eq!((MAX_OFFER_TEXT, MAX_TABLE_TEXT), (216, 172));
        assert_eq!((Supports::read(&offer.text()).unwrap(), Table::read(&table.text()).unwrap()), (offer, table));
    }

    // Text longer than the largest canonical list is refused before any of
    // it is decoded, whatever it holds.
    #[test]
    fn list_text_is_bounded_before_it_is_decoded() {
        for n in [MAX_TABLE_TEXT + 1, 1024, 16_384] { assert!(Table::read(&"A".repeat(n)).is_err(), "{n}"); }
        for n in [MAX_OFFER_TEXT + 1, 1024, 16_384] { assert!(Supports::read(&"A".repeat(n)).is_err(), "{n}"); }
    }

    // An input revision marked optional let an old client negotiate a skip
    // while its sender wrote the compiled tables.
    // Every indispensable kind must be present and required.
    #[test]
    fn indispensable_kinds_are_required_in_every_match() {
        let named = |records: Vec<Entry>| Descriptor { mode: "duel".into(), map: "swbf2/yard".into(), token: "0".repeat(32),
            records: Table { entries: records } }.json();
        assert!(Descriptor::read(&named(WRITES.to_vec())).is_ok());
        let optional_input = [&[Entry { kind: INPUT, revision: 2, required: false }], &WRITES[1..]].concat();
        assert!(Descriptor::read(&named(optional_input)).is_err());
        for kind in INDISPENSABLE {
            let mut records = WRITES.to_vec();
            records.iter_mut().filter(|e| e.kind == kind).for_each(|e| e.required = false);
            assert!(Descriptor::read(&named(records)).is_err(), "{kind} optional");
            assert!(Descriptor::read(&named(WRITES.iter().copied().filter(|e| e.kind != kind).collect())).is_err(), "{kind} missing");
        }
    }

    // Revision 0 does not exist on the wire, even for a record its reader
    // would skip.
    #[test]
    fn a_section_record_of_revision_zero_is_refused_before_any_decision() {
        let mut bytes = Vec::new();
        write_section(&[Section { kind: 91, revision: 0, body: &[1, 2] }], &mut bytes).unwrap();
        assert!(read_section(&bytes).is_err());
        bytes.clear();
        write_section(&[Section { kind: 91, revision: 2, body: &[1, 2] }], &mut bytes).unwrap();
        let optional = plan(&Table::of(&[WRITES[0], WRITES[1], written(SIT_OUT).unwrap(), Entry { kind: 91, revision: 2, required: false }]),
            &Supports::of(SUPPORTS)).unwrap();
        assert_eq!(sectioned(&optional, &read_section(&bytes).unwrap().0[0]), Ok(None));
    }

    #[test]
    fn records_sections_and_messages_tell_their_kind_revision_and_extent() {
        let mut bytes = Vec::new();
        write_section(&[Section { kind: 99, revision: 4, body: &[1, 2, 3] }, Section { kind: INPUT, revision: 1, body: &[] }], &mut bytes).unwrap();
        bytes.extend([7, 7]);
        let (records, used) = read_section(&bytes).unwrap();
        assert_eq!((records.len(), used, records[0].body), (2, bytes.len() - 2, &[1u8, 2, 3][..]));
        let plan = plan(&Table::of(&[WRITES[0], WRITES[1], Entry { kind: 99, revision: 4, required: false }]), &supports()).unwrap();
        // An optional kind this client does not implement is skipped whole at
        // the revision the match advertises, and refused at any other.
        assert_eq!(sectioned(&plan, &records[0]), Ok(None));
        assert!(sectioned(&plan, &Section { kind: 99, revision: 3, body: &[1, 2, 3] }).is_err());
        assert_eq!(sectioned(&plan, &records[1]), Ok(Some(&[][..])));
        assert!(sectioned(&plan, &Section { kind: 77, revision: 1, body: &[] }).is_err());
        assert!(sectioned(&plan, &Section { kind: INPUT, revision: 2, body: &[] }).is_err());
        for cut in 0..used { assert!(read_section(&bytes[..cut]).is_err(), "{cut}"); }
        let message = record_message(99, 4, json!({"x": 1}));
        assert_eq!(recorded(&plan, &message), Ok(None));
        assert!(recorded(&plan, &record_message(77, 1, json!(null))).is_err());
        for bad in [json!({"type": "record", "kind": 1, "revision": 0, "body": 1}), json!({"type": "record", "kind": 1, "revision": 1}),
            json!({"type": "record", "kind": 300, "revision": 1, "body": 1}), json!({"type": "combat", "kind": 1, "revision": 1, "body": 1})]
        {
            assert!(read_record(&bad).is_err(), "{bad}");
        }
        assert_eq!((field_kind(0x0203), field_kind(0x0200), field_kind(0x0300), field_schema(SOLDIER_DISPLAY)), (Some(3), None, None, 0x0203));
        assert_eq!((body_kind(0x82), body_kind(0x80), body_kind(12), body_schema(SOLDIER)), (Some(2), None, None, 0x82));
    }

    #[test]
    fn read_section_into_matches_read_section_and_checks_bounds() {
        let mut bytes = Vec::new();
        write_section(
            &[
                Section { kind: PILOT, revision: 1, body: &[1, 2, 3, 4] },
                Section { kind: 99, revision: 2, body: &[9] },
            ],
            &mut bytes,
        )
        .unwrap();
        let mut buf = [Section { kind: 0, revision: 0, body: &[] }; SECTION_RECORDS];
        let (count, used) = read_section_into(&bytes, &mut buf).unwrap();
        let (vec_recs, vec_used) = read_section(&bytes).unwrap();
        assert_eq!((count, used), (vec_recs.len(), vec_used));
        assert_eq!(&buf[..count], &vec_recs[..]);
        assert!(read_section_into(&[SECTION_RECORDS as u8 + 1], &mut buf).is_err());
        assert!(read_section_into(&bytes[..used - 1], &mut buf).is_err());
    }
}
