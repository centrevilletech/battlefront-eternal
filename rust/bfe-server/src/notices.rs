//! The notices of a scripted 2005 battle: what its mission announces, sent to
//! each player it is for as reliable records (kind 25).
//!
//! The state a mission keeps is in its world fields, which every player has
//! whole. A notice is what happened: it goes once, in order, to the players of
//! its team, and stops being worth sending a few seconds later. A player who
//! joins hears the battle's opening, what still shows and what comes after.
//!
//! The opening is what the battle's load posted (a mission script's hints,
//! messages and voice-overs of `ScriptPostLoad`), announced on its first step.
//! A server's battle starts with no one in it (a rotation's players arrive
//! after their loading screen, seconds after it began), so each player who
//! joins is sent the opening as if it were announced on his arrival, as a local
//! battle shows its player the opening at its start.
use std::collections::{HashMap, VecDeque};

use bfe_sim::wire::mission::{Cue, Feed, Notice, EVERYONE, NOTICE_TICKS, REVISION};
use bfe_sim::wire::revisions::{record_message, NOTICE};

/// The most notices kept for one player whose queue is full; the oldest goes.
pub const KEPT: usize = 64;

/// The authority tick of a battle's first step, which announces what its load posted.
const LOAD_TICK: u32 = 1;

#[derive(Default)]
struct Recipient {
    sequence: u32,
    waiting: VecDeque<(u32, Vec<u8>)>,
}

impl Recipient {
    /// Queues the cue announced at authority tick `tick` as his next notice.
    fn push(&mut self, tick: u32, cue: &Cue) {
        self.sequence = self.sequence.saturating_add(1);
        let notice = Notice { sequence: self.sequence, tick, until: tick.saturating_add(NOTICE_TICKS), cue: cue.clone() };
        let message = record_message(NOTICE, REVISION, notice.body());
        let Ok(bytes) = serde_json::to_vec(&message) else { return };
        if self.waiting.len() == KEPT {
            self.waiting.pop_front();
        }
        self.waiting.push_back((notice.until, bytes));
    }
}

#[derive(Default)]
pub struct Notices {
    pub(crate) feed: Feed,
    players: HashMap<String, Recipient>,
    /// The cues still showing, with their audience and the tick they were announced.
    recent: VecDeque<(u8, u32, Cue)>,
    /// The battle's opening, with each cue's audience (at most [`KEPT`]).
    opening: VecDeque<(u8, Cue)>,
}

impl Notices {
    /// A player whose match carries notices, joining at authority tick `tick`
    /// on `team`: he is sent the opening for him as of now, what still shows
    /// for him, then what comes after.
    pub fn join(&mut self, player: &str, tick: u32, team: Option<u8>) {
        let heard = |audience: u8| audience == EVERYONE || Some(audience) == team;
        let mut recipient = Recipient::default();
        for (_, cue) in self.opening.iter().filter(|(audience, _)| heard(*audience)) {
            recipient.push(tick, cue);
        }
        for (audience, at, cue) in &self.recent {
            if at.saturating_add(NOTICE_TICKS) > tick && heard(*audience) {
                recipient.push(*at, cue);
            }
        }
        self.players.insert(player.to_string(), recipient);
    }

    pub fn leave(&mut self, player: &str) {
        self.players.remove(player);
    }

    /// Queues the cues of authority tick `tick`, each for the players of its
    /// team (`team` gives a player's, now).
    pub fn queue(&mut self, tick: u32, cues: &[(u8, Cue)], team: impl Fn(&str) -> Option<u8>) {
        for (player, recipient) in &mut self.players {
            let side = if cues.iter().any(|(audience, _)| *audience != EVERYONE) { team(player) } else { None };
            for (_, cue) in cues.iter().filter(|(audience, _)| *audience == EVERYONE || Some(*audience) == side) {
                recipient.push(tick, cue);
            }
        }
        self.recent.retain(|(_, at, _)| at.saturating_add(NOTICE_TICKS) > tick);
        for (audience, cue) in cues {
            // What the load posted is the opening, kept for every joiner (a
            // kill never is: it happened, and stops showing).
            if tick <= LOAD_TICK && !matches!(cue, Cue::Kill { .. }) {
                if self.opening.len() < KEPT {
                    self.opening.push_back((*audience, cue.clone()));
                }
                continue;
            }
            if self.recent.len() == KEPT {
                self.recent.pop_front();
            }
            self.recent.push_back((*audience, tick, cue.clone()));
        }
    }

    /// Sends each player what waits for him, in order, until `send` refuses
    /// one; that one is tried again at the next call unless it expires first.
    pub fn send(&mut self, tick: u32, mut send: impl FnMut(&str, &[u8]) -> bool) {
        for (player, recipient) in &mut self.players {
            while let Some((until, bytes)) = recipient.waiting.front() {
                if *until > tick && !send(player, bytes) {
                    break;
                }
                recipient.waiting.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn text(key: &str) -> Cue {
        Cue::Text(key.to_string())
    }

    fn sent(notices: &mut Notices, tick: u32, takes: impl Fn(&str) -> bool) -> Vec<(String, u32, Value)> {
        let mut out = Vec::new();
        notices.send(tick, |player, bytes| {
            let message: Value = serde_json::from_slice(bytes).unwrap();
            let (kind, revision, body) = bfe_sim::wire::revisions::read_record(&message).unwrap();
            assert_eq!((kind, revision), (NOTICE, REVISION));
            let notice = Notice::read(body).unwrap();
            let Cue::Text(key) = notice.cue else { panic!("a text") };
            if takes(player) {
                out.push((player.to_string(), notice.sequence, Value::String(key)));
            }
            takes(player)
        });
        out.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        out
    }

    // A team's notice reaches its players only; a player whose queue refuses
    // one keeps his order and loses only what expired; a joiner hears what of
    // his audience still shows, in his own order, and nothing that has expired.
    #[test]
    fn notices_reach_their_team_in_order_and_expire() {
        let mut notices = Notices::default();
        notices.join("ann", 0, Some(1));
        notices.join("bo", 0, Some(2));
        let team = |player: &str| Some(if player == "ann" { 1 } else { 2 });
        notices.queue(100, &[(EVERYONE, text("all")), (2, text("two"))], team);
        let row = |player: &str, sequence: u32, key: &str| (player.to_string(), sequence, Value::String(key.into()));
        assert_eq!(sent(&mut notices, 100, |player| player == "ann"), [row("ann", 1, "all")]);
        notices.queue(160, &[(1, text("one")), (EVERYONE, text("again"))], team);
        notices.join("cy", 160, team("cy"));
        assert_eq!(sent(&mut notices, 160, |_| true),
            [row("ann", 2, "one"), row("ann", 3, "again"), row("bo", 1, "all"), row("bo", 2, "two"), row("bo", 3, "again"),
             row("cy", 1, "all"), row("cy", 2, "two"), row("cy", 3, "again")]);
        // Refused until it expires: the next one goes in its place.
        notices.queue(200, &[(EVERYONE, text("late"))], team);
        assert!(sent(&mut notices, 200, |_| false).is_empty());
        notices.queue(450, &[(EVERYONE, text("next"))], team);
        assert_eq!(sent(&mut notices, 200 + NOTICE_TICKS, |_| true), [row("ann", 5, "next"), row("bo", 5, "next"), row("cy", 5, "next")]);
        // A joiner after the opening announcements hears those still showing only.
        notices.join("di", 450 + NOTICE_TICKS - 1, Some(2));
        assert_eq!(sent(&mut notices, 450 + NOTICE_TICKS - 1, |player| player == "di"), [row("di", 1, "next")]);
        notices.leave("di");
        // A queue that takes nothing holds a bounded number.
        for tick in 0..200 {
            notices.queue(600 + tick, &[(EVERYONE, text("more"))], team);
        }
        assert_eq!(sent(&mut notices, 800, |player| player == "cy").len(), KEPT);
        notices.leave("ann");
        assert!(sent(&mut notices, 801, |_| true).iter().all(|(player, ..)| player == "bo"));
    }

    // What a battle's load posts is its opening: a player there at its first
    // step hears it as everything else; each player who joins later, however
    // late, hears it for him as of his arrival, then what still shows; a kill
    // on the load's step is not the opening, and what comes after expires.
    #[test]
    fn every_joiner_hears_the_opening_its_battle_loaded_with() {
        let mut notices = Notices::default();
        notices.join("ann", 0, Some(1));
        let killer = bfe_sim::wire::mission::Actor { slot: 1, human: false, class: 2 };
        let kill = Cue::Kill { killer: Some(killer), victim: killer, weapon: 3, gun: None };
        let team = |player: &str| Some(if player == "ann" { 1 } else { 2 });
        notices.queue(LOAD_TICK, &[(EVERYONE, text("hint")), (2, text("two")), (EVERYONE, kill.clone())], team);
        notices.queue(900, &[(EVERYONE, text("turn"))], team);
        let row = |player: &str, sequence: u32, key: &str| (player.to_string(), sequence, Value::String(key.into()));
        notices.send(LOAD_TICK, |_, _| true);
        // He arrives after his loading screen: the opening for his team as of
        // his arrival, then what still shows; the kill has expired.
        notices.join("bo", 1000, Some(2));
        let mut first = Vec::new();
        notices.send(1000, |_, bytes| {
            let message: Value = serde_json::from_slice(bytes).unwrap();
            let notice = Notice::read(bfe_sim::wire::revisions::read_record(&message).unwrap().2).unwrap();
            first.push((notice.tick, notice.until));
            false
        });
        assert_eq!(first, [(1000, 1000 + NOTICE_TICKS)]);
        assert_eq!(sent(&mut notices, 1000, |player| player == "bo"), [row("bo", 1, "hint"), row("bo", 2, "two"), row("bo", 3, "turn")]);
        // A much later joiner hears the opening only.
        notices.join("cy", 5000, Some(1));
        assert_eq!(sent(&mut notices, 5000, |player| player == "cy"), [row("cy", 1, "hint")]);
        assert_eq!(notices.opening.len(), 2);
    }
}
