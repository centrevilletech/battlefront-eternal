//! A client sitting a 2005 battle out: its map-free connection, what the
//! screen tells the player, and the move to the next battle the server runs,
//! which needs no action of the player's. The app draws it; this module
//! decides it, so a test can drive the same steps.
use super::{lobby::Reason, lobby_client::LobbyClient, rotation::Probe};

/// How long after the next battle was due the client waits for it, and how
/// long it looks for the server's current battle after a refused join.
const NEXT_BATTLE_PATIENCE_MS: f64 = 60_000.;

/// Why this client sits the battle out, as it decided before joining.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// This build does not play the 2005 game.
    Unavailable,
    /// The player has not prepared the battle's map.
    NoMap,
    /// The player's prepared game data for the battle is not the server's:
    /// the server's signed answer gave the join the sit-out role.
    OtherData,
}

/// Where a sitting-out client goes next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The server runs the battle it announced: join it.
    Announced(String),
    /// The match ended while this client joined: join what the server runs now.
    Current,
}

pub struct SitOut {
    pub client: LobbyClient,
    /// The `HOST:PORT` (or `listed:<id>`) that answered; later probes ask it.
    pub target: String,
    /// The battle query this connection joined.
    pub query: String,
    pub why: Why,
    probe: Option<Probe>,
    // When the join turned out to be for a match the server no longer runs.
    changed_at: Option<f64>,
}

impl SitOut {
    pub fn new(client: LobbyClient, target: String, query: String, why: Why) -> Self {
        Self { client, target, query, why, probe: None, changed_at: None }
    }

    /// Pumps the connection and follows the server. `Ok(Some(next))` once the
    /// next battle can be joined; an error ends the session with its reason.
    pub fn step(&mut self) -> Result<Option<Next>, String> {
        if let Some(probe) = &self.probe {
            // Another version on the server is final.
            if let Some(text) = probe.mismatch() {
                return Err(text);
            }
            if probe.ready() {
                return Ok(Some(match &self.client.lobby.next {
                    Some(next) => Next::Announced(next.query.clone()),
                    None => Next::Current,
                }));
            }
            // The server still runs the battle that refused this join.
            if probe.answer() == Some(false) {
                return Err(self.client.refusal().unwrap_or(super::match2005::CHANGED).to_string());
            }
        }
        let announced = self.client.lobby.next.is_some();
        // After announcing its next battle the server may close this
        // connection while it loads: the announcement stands.
        if let Err(error) = self.client.receive() {
            if !announced {
                return Err(error);
            }
        }
        let now = self.client.endpoint.now();
        if self.probe.is_none() {
            if let Some(next) = &self.client.lobby.next {
                if now >= next.ready_at {
                    self.probe = Some(Probe::start(self.target.clone(), next.query.clone()));
                }
            } else if self.client.changed() {
                self.probe = Some(Probe::changed(self.target.clone(), self.query.clone()));
                self.changed_at = Some(now);
            }
        }
        if self.changed_at.is_some_and(|since| now - since > NEXT_BATTLE_PATIENCE_MS) {
            return Err(self.client.refusal().unwrap_or("battle connection timed out: the server did not answer").to_string());
        }
        if let Some(next) = &self.client.lobby.next {
            if now > next.ready_at + NEXT_BATTLE_PATIENCE_MS {
                return Err("battle connection timed out: the server did not start the next battle".into());
            }
        }
        Ok(None)
    }

    /// What the screen says while the battle is sat out.
    pub fn message(&self) -> String {
        let why = match (self.why, self.client.lobby.notice.as_ref().map(|n| n.reason)) {
            (Why::Unavailable, _) => "Battlefront II (2005) is not available in this build",
            (Why::OtherData, _) | (Why::NoMap, Some(Reason::ContentMismatch)) => "this battle uses other Battlefront II game data than yours",
            (Why::NoMap, _) => "you have not prepared this battle's Battlefront II game data",
        };
        let next = match &self.client.lobby.next {
            Some(_) => "The next battle is announced: you join it when it starts.",
            None => "You join the next battle when it starts.",
        };
        format!("SITTING OUT: {why}. {next}")
    }
}
