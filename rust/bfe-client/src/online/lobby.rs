//! A client that sits a battle out has no world: the server tells it why and
//! sends the battle's score table, which it shows with chat until a battle
//! it has content for begins.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason { MissingContent, ContentMismatch }

#[derive(Clone, Debug, PartialEq)]
pub struct SitOut { pub reason: Reason, pub match_token: String, pub mode: String }

#[derive(Clone, Debug, PartialEq)]
pub struct Team { pub team: u8, pub label: String, pub score: i32, pub limit: u32 }

#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub slot: u16, pub team: u8, pub name: String, pub kills: i32, pub deaths: u32, pub points: i32, pub human: bool, pub ping_ms: Option<u16> }

/// One complete table; it replaces the previous one of its match.
#[derive(Clone, Debug, PartialEq)]
pub struct Board { pub match_token: String, pub tick: u32, pub phase: u8, pub winner: u8, pub teams: Vec<Team>, pub rows: Vec<Row> }

const INVALID: &str = "invalid lobby message";

fn token(value: &Value) -> Result<String, &'static str> {
    let token = value.as_str().ok_or(INVALID)?;
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err(INVALID); }
    Ok(token.to_string())
}
fn int(value: &Value, min: i64, max: i64) -> Result<i64, &'static str> {
    value.as_i64().filter(|n| (min..=max).contains(n)).ok_or(INVALID)
}
// Display text: bounded, and free of control characters a table cannot show.
fn text(value: &Value, bytes: usize) -> Result<String, &'static str> {
    value.as_str().filter(|s| s.len() <= bytes && !s.chars().any(char::is_control)).map(str::to_string).ok_or(INVALID)
}

/// The body of a sit-out notice record (kind 26, revision 1): exactly its
/// match, mode and reason.
pub fn sit_out(body: &Value) -> Result<SitOut, &'static str> {
    let object = body.as_object().filter(|o| o.len() == 3).ok_or(INVALID)?;
    let reason = match object.get("reason").and_then(Value::as_str) {
        Some("missing-content") => Reason::MissingContent,
        Some("content-mismatch") => Reason::ContentMismatch,
        _ => return Err(INVALID),
    };
    let mode = object.get("mode").and_then(Value::as_str).filter(|m| bfe_family::mode_token(m)).ok_or(INVALID)?;
    Ok(SitOut { reason, match_token: token(object.get("match").ok_or(INVALID)?)?, mode: mode.to_string() })
}

/// The body of a lobby board record (kind 27, revision 1): exactly its six
/// members, every row's team one of the table's teams or none (0).
pub fn scoreboard(message: &Value) -> Result<Board, &'static str> {
    if message.as_object().is_none_or(|o| o.len() != 6) { return Err(INVALID); }
    let teams = message["teams"].as_array().filter(|t| t.len() <= 7).ok_or(INVALID)?;
    let rows = message["rows"].as_array().filter(|r| r.len() <= 256).ok_or(INVALID)?;
    let mut board = Board { match_token: token(&message["match"])?, tick: int(&message["tick"], 0, u32::MAX as i64)? as u32,
        phase: int(&message["phase"], 0, 2)? as u8, winner: int(&message["winner"], 0, 7)? as u8,
        teams: Vec::with_capacity(teams.len()), rows: Vec::with_capacity(rows.len()) };
    let mut last_team = 0;
    for team in teams {
        let t = team.as_array().filter(|t| t.len() == 4).ok_or(INVALID)?;
        let id = int(&t[0], 1, 7)? as u8;
        if id <= last_team { return Err(INVALID); }
        last_team = id;
        board.teams.push(Team { team: id, label: text(&t[1], 64)?, score: int(&t[2], i32::MIN as i64, i32::MAX as i64)? as i32,
            limit: int(&t[3], 0, u32::MAX as i64)? as u32 });
    }
    let mut last_slot = -1;
    for row in rows {
        let r = row.as_array().filter(|r| r.len() == 8).ok_or(INVALID)?;
        let slot = int(&r[0], 0, 65534)?;
        if slot <= last_slot { return Err(INVALID); }
        last_slot = slot;
        let team = int(&r[1], 0, 7)? as u8;
        if team != 0 && !board.teams.iter().any(|t| t.team == team) { return Err(INVALID); }
        board.rows.push(Row { slot: slot as u16, team, name: text(&r[2], 32)?,
            kills: int(&r[3], i32::MIN as i64, i32::MAX as i64)? as i32, deaths: int(&r[4], 0, u32::MAX as i64)? as u32,
            points: int(&r[5], i32::MIN as i64, i32::MAX as i64)? as i32, human: r[6].as_bool().ok_or(INVALID)?,
            ping_ms: if r[7].is_null() { None } else { Some(int(&r[7], 0, 65534)? as u16) } });
    }
    // A winner is one of the table's teams, and only once the battle ended.
    if board.winner != 0 && (board.phase != 2 || !board.teams.iter().any(|t| t.team == board.winner)) { return Err(INVALID); }
    Ok(board)
}

/// What a client sitting a 2005 match out knows: the server's notice, the
/// latest score table of that match, chat, and an announced next battle,
/// which it joins afresh when it has that battle's content.
pub struct Lobby {
    token: String,
    mode: String,
    /// The match's plan once its signed answer was checked; nothing is read before.
    plan: Option<bfe_sim::wire::revisions::Plan>,
    pub notice: Option<SitOut>,
    pub board: Option<Board>,
    pub chat: bfe_battle_ui::ChatLog,
    pub next: Option<super::rotation::NextMatch>,
    // The server's pings not answered yet, oldest first. A peer the server
    // has not started is closed as silent after three minutes, so a client
    // sitting a long match out stays only by answering them.
    pongs: std::collections::VecDeque<f64>,
}

/// A sitting-out connection received what only a client in the battle reads.
pub const NOT_IN_BATTLE: &str = "the server sent a battle to a client sitting it out";

impl Lobby {
    /// The lobby of the match `token` of `mode`, as discovery described it.
    pub fn new(token: &str, mode: &str) -> Self {
        Self { token: token.into(), mode: mode.into(), plan: None, notice: None, board: None, chat: bfe_battle_ui::ChatLog::new(), next: None, pongs: std::collections::VecDeque::new() }
    }
    /// The checked plan of the match: from now on its records are read.
    pub fn admit(&mut self, plan: bfe_sim::wire::revisions::Plan) { self.plan = Some(plan); }
    /// One message of the connection, checked before anything changes.
    pub fn message(&mut self, text: bool, bytes: &[u8], now: f64) -> Result<(), String> {
        const MESSAGE_BYTES: usize = 64 * 1024;
        // The largest message a sit-out peer is sent is a full score table:
        // 256 rows of at most about 130 bytes each, well under this bound,
        // which applies before any JSON is parsed.
        if !text { return Err(NOT_IN_BATTLE.into()); }
        if bytes.len() > MESSAGE_BYTES { return Err(INVALID.into()); }
        let message: Value = serde_json::from_slice(bytes).map_err(|_| INVALID.to_string())?;
        match message.get("type").and_then(Value::as_str) {
            Some("record") => {
                use bfe_sim::wire::revisions::{recorded, BOARD, SIT_OUT};
                let plan = self.plan.as_ref().ok_or(NOT_IN_BATTLE)?;
                match recorded(plan, &message)? {
                    // An optional record this build does not implement.
                    None => {}
                    Some((SIT_OUT, body)) => {
                        let notice = sit_out(body)?;
                        if notice.match_token != self.token { return Err(super::match2005::CHANGED.into()); }
                        // The notice comes once, for the mode discovery described.
                        if notice.mode != self.mode || self.notice.is_some() { return Err(INVALID.into()); }
                        self.notice = Some(notice);
                    }
                    Some((BOARD, body)) => {
                        let notice = self.notice.as_ref().ok_or(NOT_IN_BATTLE)?;
                        let board = scoreboard(body)?;
                        // Only this match's tables replace the last good one, each strictly
                        // newer by the authority tick, which wraps.
                        let newer = self.board.as_ref().is_none_or(|b| (board.tick.wrapping_sub(b.tick) as i32) > 0);
                        if board.match_token == notice.match_token && newer { self.board = Some(board); }
                    }
                    // A record of a battle a sitting-out client never reads.
                    Some(_) => return Err(NOT_IN_BATTLE.into()),
                }
            }
            Some("ping") => {
                let time = message.get("time").and_then(Value::as_f64).filter(|t| t.is_finite()).ok_or(INVALID)?;
                if self.pongs.len() == 32 { self.pongs.pop_front(); }
                self.pongs.push_back(time);
            }
            Some("chat") => super::chat::receive(bytes, &mut self.chat, now)?,
            Some("match") => {
                let query = message.get("query").and_then(Value::as_str).ok_or(INVALID)?.to_string();
                self.next = Some(super::rotation::NextMatch::new(query, message.get("delayMs").and_then(Value::as_f64), now)?);
            }
            Some("entity-registry" | "welcome" | "gameplay-events" | "entity-edge-results" | "input-ack") => return Err(NOT_IN_BATTLE.into()),
            _ => {}
        }
        Ok(())
    }
    /// The next answer to a server ping, as its message; none unanswered.
    pub fn pong(&mut self) -> Option<Vec<u8>> {
        let time = self.pongs.pop_front()?;
        serde_json::to_vec(&serde_json::json!({"type": "pong", "time": time})).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_sim::wire::revisions::{plan, record_message, Entry, Supports, Table, BOARD, SIT_OUT, SOLDIER, SUPPORTS, WRITES};
    use serde_json::json;
    const MATCH: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    fn written(kind: u8) -> Entry { bfe_sim::wire::revisions::written(kind).unwrap() }
    fn record(kind: u8, body: Value) -> Vec<u8> { serde_json::to_vec(&record_message(kind, 1, body)).unwrap() }
    fn notice(token: &str, mode: &str) -> Value { json!({"match": token, "mode": mode, "reason": "missing-content"}) }
    fn board(tick: u32, token: &str) -> Value {
        json!({"match": token, "tick": tick, "phase": 1, "winner": 0, "teams": [[1, "Blue", 1, 0]], "rows": []})
    }
    fn admitted(table: &[Entry]) -> Lobby {
        let mut lobby = Lobby::new(MATCH, "eli");
        lobby.admit(plan(&Table::of(table), &Supports::of(SUPPORTS)).unwrap());
        lobby
    }

    // With the real writer and the real reader: a Geonosis battle admitted
    // headless holds native locals on a team of their own; the table the
    // server writes of it, wrapped as the server wraps it, is read whole by a
    // client sitting the match out.
    #[test]
    fn the_server_table_of_a_battle_with_native_locals_is_read() {
        let Ok(content) = std::env::var("BFE_CONTENT_DIR") else { return };
        let files = bfe_content::files::DirFiles::new(content.as_str(), bfe_family::GameFamily::Swbf1);
        let mut world = bfe_sim::world::owner::World::new();
        let options = bfe_sim::world::admit::Options { map: "geo1".into(), side: "rep".into(), ..Default::default() };
        bfe_sim::world::admit::admit(&mut world, &files, &options).unwrap();
        let table = bfe_sim::world::lobby::table(&world, 30);
        assert!(table.rows.iter().any(|row| row.team == 3), "the fixture holds native locals");
        let body = bfe_sim::world::lobby::board_body(MATCH, &table, |slot| (slot == 0).then_some(40));
        let mut lobby = admitted(WRITES);
        lobby.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).unwrap();
        lobby.message(true, &record(BOARD, body), 1.).unwrap();
        let board = lobby.board.as_ref().expect("the table is read");
        assert_eq!(board.teams.iter().map(|t| t.team).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(board.teams[2].score, 0);
        assert_eq!(board.rows.len(), table.rows.len());
        println!("geo1 table: {} teams, {} rows ({} locals)", board.teams.len(), board.rows.len(), board.rows.iter().filter(|r| r.team == 3).count());
    }

    #[test]
    fn lobby_records_are_read_strictly() {
        let read = sit_out(&notice(MATCH, "eli")).unwrap();
        assert_eq!((read.reason, read.mode.as_str()), (Reason::MissingContent, "eli"));
        for (key, value) in [("reason", json!("lost")), ("mode", json!(7)), ("mode", json!("Eli")), ("extra", json!(1))] {
            let mut bad = notice(MATCH, "eli"); bad[key] = value;
            assert!(sit_out(&bad).is_err(), "{key}");
        }
        // The locals of a map are a team of their own in the table.
        let good = json!({"match": MATCH, "tick": 600, "phase": 1, "winner": 0,
            "teams": [[1, "Blue", 140, 0], [2, "Gold", 97, 0], [3, "Locals", 0, 0]],
            "rows": [[0, 1, "Ann", 3, 1, 2, true, 40], [4, 2, "Bot", -2, 5, 0, false, null], [9, 3, "Local", 0, 0, 0, false, null], [12, 0, "None", 0, 0, 0, false, null]]});
        let read = scoreboard(&good).unwrap();
        assert_eq!((read.teams.len(), read.rows[1].kills, read.rows[0].ping_ms, read.rows[2].team), (3, -2, Some(40), 3));
        for (path, value) in [("phase", json!(3)), ("teams", json!([[2, "x", 0, 0], [1, "y", 0, 0]])),
            ("rows", json!([[1, 1, "a", 0, 0, 0, true, null], [1, 1, "b", 0, 0, 0, true, null]])),
            ("match", json!("short")), ("rows", json!([[0, 4, "a", 0, 0, 0, true, null]])),
            ("rows", json!([[0, 1, "a\u{1b}[2J", 0, 0, 0, true, null]])), ("winner", json!(1)), ("extra", json!(0))] {
            let mut bad = good.clone(); bad[path] = value;
            assert!(scoreboard(&bad).is_err(), "{path}");
        }
        let mut ended = good.clone(); ended["phase"] = 2.into(); ended["winner"] = 2.into();
        assert!(scoreboard(&ended).is_ok());
    }

    #[test]
    fn a_sitting_out_client_keeps_its_match_s_tables_and_refuses_a_world() {
        let mut lobby = Lobby::new(MATCH, "eli");
        // Nothing is read before the signed answer gave the match's plan.
        assert!(lobby.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).is_err());
        let mut lobby = admitted(WRITES);
        // A table before the notice belongs to no role yet.
        assert!(lobby.message(true, &record(BOARD, board(60, MATCH)), 0.).is_err());
        lobby.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).unwrap();
        // The notice comes once.
        assert!(lobby.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).is_err());
        lobby.message(true, &record(BOARD, board(120, MATCH)), 0.).unwrap();
        // An older or equal table, or another match's, never replaces the last good one.
        for (tick, token) in [(60, MATCH), (120, MATCH), (180, OTHER)] {
            lobby.message(true, &record(BOARD, board(tick, token)), 0.).unwrap();
        }
        assert_eq!(lobby.board.as_ref().unwrap().tick, 120);
        // Ticks wrap: a table just past the wrap is newer.
        let mut wrapping = admitted(WRITES);
        wrapping.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).unwrap();
        wrapping.message(true, &record(BOARD, board(u32::MAX - 5, MATCH)), 0.).unwrap();
        wrapping.message(true, &record(BOARD, board(3, MATCH)), 0.).unwrap();
        assert_eq!(wrapping.board.as_ref().unwrap().tick, 3);
        lobby.message(true, br#"{"type":"chat","sender":"Ann","text":"hi","team":false}"#, 5.).unwrap();
        assert_eq!(lobby.chat.messages.len(), 1);
        lobby.message(true, br#"{"type":"match","query":"?map=kam9&match=00112233445566778899aabbccddeeff","delayMs":3000}"#, 10.).unwrap();
        assert_eq!(lobby.next.as_ref().unwrap().ready_at, 3010.);
        // A world never reaches a client sitting the match out.
        assert_eq!(lobby.message(false, &[1, 2, 3], 0.), Err(NOT_IN_BATTLE.into()));
        assert_eq!(lobby.message(true, br#"{"type":"welcome","epoch":1}"#, 0.), Err(NOT_IN_BATTLE.into()));
        assert_eq!(lobby.message(true, &record(SOLDIER, json!(1)), 0.), Err(NOT_IN_BATTLE.into()));
        // A notice of another mode is malformed; one of another match means this one ended while joining.
        assert!(admitted(WRITES).message(true, &record(SIT_OUT, notice(MATCH, "ass")), 0.).is_err());
        assert_eq!(admitted(WRITES).message(true, &record(SIT_OUT, notice(OTHER, "eli")), 0.), Err(super::super::match2005::CHANGED.into()));
    }

    #[test]
    fn an_optional_record_this_build_does_not_implement_is_skipped() {
        let mut lobby = admitted(&[WRITES[0], WRITES[1], written(SIT_OUT), Entry { kind: 99, revision: 4, required: false }]);
        lobby.message(true, &serde_json::to_vec(&record_message(99, 4, json!({"anything": [1, 2]}))).unwrap(), 0.).unwrap();
        // A kind the match does not name is refused.
        assert!(lobby.message(true, &record(77, json!(null)), 0.).is_err());
    }

    // Generated messages near the record shapes: whatever the reader keeps
    // obeys the table's limits, and nothing panics.
    #[test]
    fn generated_lobby_messages() {
        let mut seed = 0x10bb_5eedu64;
        let mut next = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        let odd = [json!(null), json!(-1), json!(65536), json!(1.5), json!("x\u{7}"), json!("a".repeat(80)), json!([]), json!({}),
            json!(u64::MAX), json!(true)];
        let good_board = json!({"match": MATCH, "tick": 900, "phase": 1, "winner": 0,
            "teams": [[1, "Blue", 140, 0], [2, "Gold", 97, 200]], "rows": [[0, 1, "Ann", 3, 1, 2, true, 40], [4, 2, "Bot", -2, 5, 0, false, null]]});
        let (mut kept, mut cases) = (0, 0);
        for _ in 0..8_192 {
            let mut lobby = admitted(WRITES);
            lobby.message(true, &record(SIT_OUT, notice(MATCH, "eli")), 0.).unwrap();
            let mut body = if next() % 2 == 0 { good_board.clone() } else { notice(MATCH, "eli") };
            for _ in 0..next() % 3 {
                let Some(keys) = body.as_object().map(|m| m.keys().cloned().collect::<Vec<String>>()).filter(|k| !k.is_empty()) else { break };
                let key = &keys[(next() % keys.len() as u64) as usize];
                let value = odd[(next() % odd.len() as u64) as usize].clone();
                match (next() % 3, body[key].as_array().cloned()) {
                    (0, Some(mut items)) if !items.is_empty() => {
                        let at = (next() % items.len() as u64) as usize;
                        if let Some(row) = items[at].as_array_mut().filter(|row| !row.is_empty()) {
                            let cell = (next() % row.len() as u64) as usize;
                            row[cell] = value;
                        } else { items[at] = value; }
                        body[key] = items.into();
                    }
                    (1, _) => { body.as_object_mut().unwrap().remove(key); }
                    _ => body[key] = value,
                }
            }
            cases += 1;
            let kind = if body.get("rows").is_some() || body.get("teams").is_some() { BOARD } else { SIT_OUT };
            if lobby.message(true, &record(kind, body), 1.).is_ok() {
                kept += 1;
                if let Some(read) = &lobby.board {
                    assert!(read.rows.len() <= 256 && read.teams.iter().all(|t| (1..=7).contains(&t.team)));
                    assert!(read.rows.iter().all(|row| row.name.len() <= 32 && !row.name.chars().any(char::is_control)));
                    assert!(read.rows.windows(2).all(|w| w[0].slot < w[1].slot));
                }
            }
        }
        assert!(kept > 100 && kept < cases);
    }

    // The server pings every peer and closes one it has not started after
    // three minutes of silence: each ping is answered once, in order, and
    // the unanswered stay bounded however long the match runs.
    #[test]
    fn server_pings_are_answered_in_order() {
        let mut lobby = Lobby::new(MATCH, "eli");
        let ping = |time: f64| serde_json::to_vec(&json!({"type": "ping", "time": time})).unwrap();
        lobby.message(true, &ping(1200.), 0.).unwrap();
        lobby.message(true, &ping(3400.), 0.).unwrap();
        assert!(lobby.message(true, &serde_json::to_vec(&json!({"type": "ping"})).unwrap(), 0.).is_err());
        assert_eq!(String::from_utf8(lobby.pong().unwrap()).unwrap(), "{\"time\":1200.0,\"type\":\"pong\"}");
        for time in 0..40 { lobby.message(true, &ping(time as f64), 0.).unwrap(); }
        let mut answered = 0;
        while lobby.pong().is_some() { answered += 1; }
        assert_eq!(answered, 32);
    }
}
