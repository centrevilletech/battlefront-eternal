//! The messages of a client that sits a battle out: what it is told on
//! admission, and the score table it sees in place of the world. Both are
//! 2005 records (`bfe_sim::wire::revisions`), written by the simulation's
//! lobby module so the server and the client read one shape.
use bfe_family::Selection;
use bfe_sim::wire::revisions::{record_message, BOARD, SIT_OUT};
use bfe_sim::world::lobby::{board_body, sit_out_body, LobbyTable};
use serde_json::Value;

/// The sit-out notice of a 2005 match: record kind 26, revision 1.
pub fn sit_out_message(match_token: &str, battle: &Selection, reason: &str) -> Value {
    record_message(SIT_OUT, 1, sit_out_body(match_token, &battle.mode, reason))
}

/// The complete lobby score table of one match: record kind 27, revision 1.
pub fn scoreboard_message(match_token: &str, table: &LobbyTable, pings: impl Fn(u16) -> Option<u16>) -> Value {
    record_message(BOARD, 1, board_body(match_token, table, pings))
}

/// The score table peers sitting a battle out are sent: the last table and
/// pings, its revision, and the revision each peer was last sent. A change of
/// the table or of a ping is a new revision. A peer whose queue refused a
/// table keeps it pending and is tried again at the next pass, once a second,
/// until it takes one or leaves.
#[derive(Default)]
pub struct Board {
    last: Option<(LobbyTable, Vec<(u16, u16)>)>,
    revision: u32,
    bytes: Vec<u8>,
    sent: std::collections::HashMap<String, u32>,
}

impl Board {
    /// The battle's table at this pass, with each slot's ping.
    pub fn update(&mut self, token: &str, table: LobbyTable, pings: Vec<(u16, u16)>) {
        let key = LobbyTable { tick: 0, ..table.clone() };
        if self.last.as_ref().is_some_and(|(last, known)| *last == key && *known == pings) { return; }
        let message = scoreboard_message(token, &table, |slot| pings.iter().find(|(s, _)| *s == slot).map(|(_, ping)| *ping));
        let Ok(bytes) = serde_json::to_vec(&message) else { return };
        (self.bytes, self.revision, self.last) = (bytes, self.revision.wrapping_add(1), Some((key, pings)));
    }
    /// Sends the current table to each of `peers` that has not taken it; the
    /// others leave the cache.
    pub fn send(&mut self, peers: &[String], mut send: impl FnMut(&str, &[u8]) -> bool) {
        self.sent.retain(|id, _| peers.contains(id));
        if self.bytes.is_empty() { return; }
        for id in peers {
            if self.sent.get(id) != Some(&self.revision) && send(id, &self.bytes) { self.sent.insert(id.clone(), self.revision); }
        }
    }
    /// A peer that newly sits out: it is sent the table at the next send.
    pub fn forget(&mut self, peer: &str) { self.sent.remove(peer); }
}

/// A battle query's `match` token.
pub fn match_token(query: &str) -> &str {
    query.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("match=")).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_sim::world::lobby::{LobbyRow, LobbyTeam};

    #[test]
    fn the_sit_out_messages_are_2005_records() {
        let eli = Selection::swbf2("eli").unwrap();
        let notice = sit_out_message("ab", &eli, "content-mismatch");
        assert_eq!(notice.to_string(), r#"{"body":{"match":"ab","mode":"eli","reason":"content-mismatch"},"kind":26,"revision":1,"type":"record"}"#);
        let table = LobbyTable { tick: 9, phase: 1, winner: 0, teams: vec![LobbyTeam { team: 1, label: "Blue".into(), score: 12, limit: 0 }],
            rows: vec![LobbyRow { slot: 0, team: 1, name: "Bot 1".into(), kills: -2, deaths: 3, points: 4, human: false }] };
        let message = scoreboard_message("ab", &table, |_| None);
        assert_eq!((message["kind"].as_u64(), message["revision"].as_u64()), (Some(27), Some(1)));
        assert_eq!(message["body"]["rows"].to_string(), r#"[[0,1,"Bot 1",-2,3,4,false,null]]"#);
        // A human row takes its player's ping by slot; a bot never has one.
        let mut people = table.clone();
        people.rows.push(LobbyRow { slot: 3, team: 2, name: "Bot 1".into(), kills: 0, deaths: 0, points: 0, human: true });
        let message = scoreboard_message("ab", &people, |slot| (slot == 3).then_some(48));
        assert_eq!(message["body"]["rows"].to_string(), r#"[[0,1,"Bot 1",-2,3,4,false,null],[3,2,"Bot 1",0,0,0,true,48]]"#);
        assert_eq!(message["body"]["teams"].to_string(), r#"[[1,"Blue",12,0]]"#);
        assert_eq!(match_token("?map=nab2&match=0f&seed=1"), "0f");
    }

    // An RTT-only change is a new table; a peer whose queue refused a table is
    // sent it at the next pass; a peer that took it is not sent it again; one
    // that left leaves the cache and is sent the table when it returns.
    #[test]
    fn the_board_follows_pings_and_retries_a_refused_peer() {
        let table = LobbyTable { tick: 60, phase: 1, winner: 0, teams: vec![LobbyTeam { team: 1, label: "Blue".into(), score: 10, limit: 0 }],
            rows: vec![LobbyRow { slot: 0, team: 1, name: "Ann".into(), kills: 1, deaths: 0, points: 0, human: true }] };
        let peers = ["a".to_string(), "b".to_string()];
        let mut board = Board::default();
        let mut sent: Vec<(String, Vec<u8>)> = Vec::new();
        board.update("ab", table.clone(), vec![(0, 40)]);
        board.send(&peers, |id, bytes| { sent.push((id.into(), bytes.to_vec())); id == "a" });
        assert_eq!(sent.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        sent.clear();
        // The same table and pings a second later: only the refused peer.
        board.update("ab", LobbyTable { tick: 120, ..table.clone() }, vec![(0, 40)]);
        board.send(&peers, |id, bytes| { sent.push((id.into(), bytes.to_vec())); true });
        assert_eq!(sent.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["b"]);
        sent.clear();
        // Only the ping changed: both are sent the new table.
        board.update("ab", LobbyTable { tick: 180, ..table.clone() }, vec![(0, 55)]);
        board.send(&peers, |id, bytes| { sent.push((id.into(), bytes.to_vec())); true });
        assert_eq!(sent.len(), 2);
        assert!(sent.iter().all(|(_, bytes)| String::from_utf8_lossy(bytes).contains(r#""Ann",1,0,0,true,55]"#)));
        sent.clear();
        board.send(&peers[..1], |id, bytes| { sent.push((id.into(), bytes.to_vec())); true });
        assert!(sent.is_empty());
        board.send(&peers, |id, bytes| { sent.push((id.into(), bytes.to_vec())); true });
        assert_eq!(sent.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["b"]);
    }
}
