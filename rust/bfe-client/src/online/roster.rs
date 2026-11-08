//! Character publications from network-entity-summary.js, retained by roster
//! slot rather than transient visible soldier identities. Dead players remain
//! on the scoreboard and an unreceived chunk leaves team-zero placeholders.
// captureRosterSummaries groups twelve Characters, with at most 200 chunks.
// Its byte cap belongs to the wire format, not a native text-buffer policy.
const CHUNK: usize = 12;
const CAPACITY: usize = CHUNK * 200;
// LINK_KINDS in network-entity-summary.js: indices travel as one byte.
pub const LINKS: [&str; 5] = ["unknown", "direct4", "direct6", "relay4", "relay6"];
// Keep wire names in their admitted bytes until the cold scoreboard asks for
// text. Most roster heartbeats therefore copy no heap objects.
#[derive(Clone, Copy, Default)]
pub struct Character {
    pub team: u8,
    pub human: bool,
    pub kills: u16,
    pub team_kills: u16,
    pub deaths: u16,
    pub captures: u16,
    pub ping: Option<u16>,
    pub link: usize,
    name: [u8; 32],
    length: usize,
}
impl Character {
    // TextDecoder replaces invalid UTF-8, including a name cut at byte 32.
    pub fn name(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.name[..self.length])
    }
}
pub struct Roster {
    characters: Vec<Character>,
    pub local: Option<usize>,
    // Only the published prefix is visible; all reservations survive epochs.
    total: usize,
}
impl Default for Roster {
    fn default() -> Self {
        // Reserve the protocol limit before packet callbacks can arrive.
        Self {
            characters: vec![Character::default(); CAPACITY],
            local: None,
            total: 0,
        }
    }
}
// Summary Fields reads little-endian scalars; every take checks the payload
// boundary before advancing, including variable-length spawn policy lists.
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let part = self.0.get(..n)?;
        self.0 = &self.0[n..];
        Some(part)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn short(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
}
impl Roster {
    // A new network epoch cannot inherit Character slots from the old battle.
    pub fn reset(&mut self) {
        self.characters[..self.total].fill(Character::default());
        self.total = 0;
        self.local = None;
    }
    // Preserve roster order for score ties and the network player list.
    pub fn rows(&self) -> impl Iterator<Item = (usize, &Character)> {
        self.characters[..self.total].iter().enumerate()
    }
    // decodeWorldSummary/applyWorldSummary schema 4. Decode the whole chunk
    // before mutating retained rows; malformed publications change nothing.
    // Storage is reserved at admission, with no allocation on receive.
    pub fn receive(&mut self, schema: u32, id: u32, bytes: &[u8]) -> Option<()> {
        let mut r = Reader(bytes);
        if schema == 3 {
            // captureSpawnSummary ends with the local Character slot. Skip its
            // unit identity, team/phase, class/post, spawn flags and deadTime;
            // pending/auto policies and eligibility lists have variable sizes.
            // A respawn changes unit generation without changing this slot.
            if id != 65536 {
                return None;
            }
            r.take(24)?;
            if r.byte()? != 0 {
                r.take(4)?;
            }
            if r.byte()? != 0 {
                r.take(5)?;
            }
            for _ in 0..2 {
                let count = r.byte()? as usize;
                if count > 64 {
                    return None;
                }
                r.take(count * 2)?;
            }
            let slot = r.byte()?;
            if !r.0.is_empty() {
                return None;
            }
            // NO_SLOT (255) represents a spectator, not roster position 255.
            self.local = (slot != 255).then_some(slot as usize);
            return Some(());
        }
        if schema != 4 {
            return Some(());
        }
        // decodeWorldSummary checks chunk alignment and its summary identity.
        // Validate these before touching any Character, even on shrink.
        let total = r.short()? as usize;
        let start = r.short()? as usize;
        let count = r.byte()? as usize;
        if total > CAPACITY
            || count > CHUNK
            || !start.is_multiple_of(CHUNK)
            || start + count > total
            || id != 70000 + (start / CHUNK) as u32
        {
            return None;
        }
        let mut chunk = [Character::default(); CHUNK];
        for c in &mut chunk[..count] {
            c.team = r.byte()?;
            let human = r.byte()?;
            if c.team > 7 || human > 1 {
                return None;
            }
            c.human = human == 1;
            // Raw kills retain team kills separately; scoreboardPort applies
            // shownStats only when constructing the visible scoreboard row.
            c.kills = r.short()?;
            c.team_kills = r.short()?;
            c.deaths = r.short()?;
            c.captures = r.short()?;
            let ping = r.short()?;
            // The server reserves 65535 for an unmeasured round trip.
            c.ping = (ping != 65535).then_some(ping);
            c.link = r.byte()? as usize;
            c.length = r.byte()? as usize;
            if c.link >= LINKS.len() || c.length > 32 {
                return None;
            }
            c.name[..c.length].copy_from_slice(r.take(c.length)?);
        }
        if !r.0.is_empty() {
            return None;
        }
        // applyWorldSummary truncates the old roster. Clear discarded slots
        // too, so later growth cannot resurrect a disconnected player's name.
        if total < self.total {
            self.characters[total..self.total].fill(Character::default());
        }
        self.total = total;
        self.characters[start..start + count].copy_from_slice(&chunk[..count]);
        Some(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_keeps_names_scores_links_and_unknown_ping() {
        // Frozen schema-4 publication: a human and bot, with one team kill.
        let bytes = [
            2, 0, 0, 0, 2, 1, 1, 7, 0, 1, 0, 3, 0, 4, 0, 1, 0, 1, 6, b'P', b'l', b'a', b'y', b'e',
            b'r', 2, 0, 2, 0, 0, 0, 5, 0, 1, 0, 255, 255, 0, 3, b'B', b'o', b't',
        ];
        let mut roster = Roster::default();
        assert_eq!(roster.receive(4, 70000, &bytes), Some(()));
        let c = &roster.characters[0];
        assert_eq!(c.name(), "Player");
        assert!(c.human);
        assert_eq!(
            (
                c.kills,
                c.team_kills,
                c.deaths,
                c.captures,
                c.ping,
                LINKS[c.link]
            ),
            (7, 1, 3, 4, Some(1), "direct4")
        );
        assert!(!roster.characters[1].human);
        assert_eq!(roster.characters[1].ping, None);
        assert_eq!(roster.receive(4, 70000, &bytes[..bytes.len() - 1]), None);
        assert_eq!(roster.characters[0].kills, 7);
        let spawn = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        assert_eq!(roster.receive(3, 65536, &spawn), Some(()));
        assert_eq!(roster.local, Some(1));
        assert_eq!(roster.receive(4, 70000, &[0, 0, 0, 0, 0]), Some(()));
        assert_eq!(roster.rows().count(), 0);
    }
}
