//! The connection of a client sitting a 2005 match out because it does not
//! have the map: authenticated like a battle join, but with no world, map,
//! prediction or camera. It reads the notice, the score table and chat, sends
//! global chat, and hands the app an announced next battle to join afresh
//! when the player has that battle's content.
use super::{lobby::Lobby, match2005::{self, Expected, Joined}};
use bfe_transport::{connection::Lane, endpoint::Endpoint};
use std::time::Duration;

pub struct LobbyClient {
    pub endpoint: Box<Endpoint>,
    expected: Expected,
    joined: Option<Joined>,
    held: match2005::Held,
    pub lobby: Lobby,
    connecting_at: f64,
    changed: bool,
    // The server's refusal that closed this join, given if the server still
    // runs the battle it refused.
    refusal: Option<String>,
}

impl LobbyClient {
    /// A connection opened with the 2005 hello of `expected`, whose content
    /// is None: the server gives it the sit-out role.
    pub fn connect(endpoint: Box<Endpoint>, expected: Expected) -> Self {
        Self::resume(endpoint, expected, Default::default())
    }
    /// The connection of a battle join the server gave the sit-out role, its
    /// content being another than the match's, with what arrived before that
    /// signed answer: held until the answer is checked here, by the same rule,
    /// then read in order.
    pub fn resume(endpoint: Box<Endpoint>, expected: Expected, held: match2005::Held) -> Self {
        let connecting_at = endpoint.now();
        let lobby = Lobby::new(&expected.token, &expected.mode);
        Self { endpoint, expected, joined: None, held, lobby, connecting_at, changed: false, refusal: None }
    }
    pub fn joined(&self) -> Option<&Joined> { self.joined.as_ref() }
    /// Whether the match ended while this connection joined it, or the
    /// server refused the join for its version or for a match that ended: the
    /// caller looks for the server's current battle again, as a battle join
    /// does.
    pub fn changed(&self) -> bool { self.changed }
    /// The server's refusal of this join when it closed the connection for its
    /// version or for a match that ended: the reason to give if the server
    /// still runs the battle it refused.
    pub fn refusal(&self) -> Option<&str> { self.refusal.as_deref() }
    /// Pumps the connection. Nothing the server sent is read before its
    /// signed answer names the match asked for, and nothing once the join is
    /// known to be for a match the server no longer runs.
    pub fn receive(&mut self) -> Result<(), String> {
        if self.changed { return Ok(()); }
        let mut overflow = false;
        let (held, lobby, checked) = (&mut self.held, &mut self.lobby, self.joined.is_some());
        let mut fault = None;
        let pumped = self.endpoint.pump(&mut |_, text, bytes, now| {
            if !checked {
                if !held.hold(text, bytes, now) { overflow = true; }
            } else if fault.is_none() {
                fault = lobby.message(text, bytes, now).err();
            }
        });
        if let Err(error) = pumped {
            let error = self.endpoint.failure(error);
            // Refused before its signed answer, for the server's version or a
            // match that ended: the same decision as a battle join's.
            if self.joined.is_none() && super::client::refused_for_another_battle(&self.endpoint) {
                self.changed = true;
                self.refusal = Some(error);
                let _ = self.held.drain().count();
                return Ok(());
            }
            return Err(error);
        }
        if overflow { return Err("the server sent a Battlefront II match's messages before its signed answer".into()); }
        if let Some(fault) = fault { return Err(fault); }
        if self.joined.is_none() && !self.changed {
            if self.endpoint.ready() {
                let joined = match match2005::accept(&self.endpoint.server_hello, &self.expected) {
                    Ok(joined) => joined,
                    // The match ended while joining: nothing held is read.
                    Err(match2005::Refusal::Changed) => {
                        self.changed = true;
                        let _ = self.held.drain().count();
                        return Ok(());
                    }
                    Err(refusal) => return Err(refusal.text()),
                };
                // This connection only ever sits the match out: a battle role here
                // means the server and this client disagree on the content.
                if joined.role != match2005::Role::SitOut { return Err("the server gave a battle role to a client sitting the battle out".into()); }
                self.lobby.admit(joined.plan.clone());
                self.joined = Some(joined);
                for (text, bytes, now) in self.held.drain() { self.lobby.message(text, &bytes, now)?; }
            } else if self.endpoint.now() - self.connecting_at >= Duration::from_secs(30).as_millis() as f64 {
                return Err(self.endpoint.waiting_reason().unwrap_or_else(|| "battle connection timed out".into()));
            }
        }
        // The server drops a peer it has not started after three minutes of
        // silence: its pings are answered in the same pump that read them.
        let mut answered = false;
        while let Some(bytes) = self.lobby.pong() {
            self.endpoint.send(Lane::Events, true, &bytes).map_err(|e| e.to_string())?;
            answered = true;
        }
        if answered { self.endpoint.flush().map_err(|e| e.to_string())?; }
        Ok(())
    }
    /// Global chat: the only channel a client sitting the match out has.
    pub fn chat(&mut self, text: &str) -> Result<(), String> {
        if self.joined.is_none() { return Ok(()); }
        self.endpoint.send(Lane::Events, true, &super::chat::encode(text, false)).map_err(|e| e.to_string())?;
        self.endpoint.flush().map_err(|e| e.to_string())
    }
}
