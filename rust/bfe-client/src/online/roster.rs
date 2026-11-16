//! Character publications, retained by roster
//! slot rather than transient visible soldier identities. Dead players remain
//! on the scoreboard and an unreceived chunk leaves team-zero placeholders.
// captureRosterSummaries groups twelve Characters, with at most 200 chunks.
// Its byte cap belongs to the wire format, not a native text-buffer policy.
const CHUNK: usize = 12;
const CAPACITY: usize = CHUNK * 200;
// LINK_KINDS: indices travel as one byte.
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
/// One team slot of a Character's end-of-match record as packRecord shares it
///: the tallies in SHARED_TALLIES order (team kills,
/// vehicles destroyed, spree, survivalist, best kills in a life, headshots,
/// deaths, escapes, longest life s, longest camp s), the favourite class,
/// weapon and vehicle as (registry id, seconds), and victims by roster slot.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Record {
    pub tallies: [f64; 10],
    pub favourites: [Option<(u16, f64)>; 3],
    pub victims: Vec<(u16, u8)>,
}
pub type Records = [Option<Record>; 2];
pub struct Roster {
    characters: Vec<Character>,
    // Schema 5: each slot's record from the everyone summaries, and the
    // player's own (sent ten times a second) with its slot.
    records: Vec<Records>,
    own_record: (Option<usize>, Records),
    pub local: Option<usize>,
    // Only the published prefix is visible; all reservations survive epochs.
    total: usize,
}
impl Default for Roster {
    fn default() -> Self {
        // Reserve the protocol limit before packet callbacks can arrive.
        Self {
            characters: vec![Character::default(); CAPACITY],
            records: vec![Records::default(); CAPACITY],
            own_record: (None, Records::default()),
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
    fn word(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}
// decodeWorldSummary schema 5: a count, then per entry its roster slot, the
// mask of team slots present and each present slot's packed record. With
// `into` the records are written there (by slot, or all into its first entry
// when it holds one), reusing their storage; without, only checked. Returns
// the last entry's slot, None for a malformed publication.
fn decode_records(bytes: &[u8], mut into: Option<&mut [Records]>) -> Option<Option<usize>> {
    let mut r = Reader(bytes);
    let mut last = None;
    for _ in 0..r.byte()? {
        let slot = r.short()? as usize;
        let mask = r.byte()?;
        if slot >= CAPACITY || !(1..=3).contains(&mask) {
            return None;
        }
        last = Some(slot);
        let mut entry = into.as_deref_mut().map(|all| {
            let single = all.len() == 1;
            &mut all[if single { 0 } else { slot }]
        });
        for k in 0..2 {
            // applyRecord leaves a team slot the entry does not carry as it was.
            if mask & (1 << k) == 0 {
                continue;
            }
            let mut record = entry.as_deref_mut().map(|pair| pair[k].get_or_insert_with(Record::default));
            let mut tallies = [0.; 10];
            for (i, tally) in tallies.iter_mut().enumerate() {
                *tally = match i {
                    8 | 9 => f64::from(r.word()?) / 1000.,
                    5 => f64::from(r.short()?),
                    _ => f64::from(r.byte()?),
                };
            }
            let mut favourites = [None; 3];
            for favourite in &mut favourites {
                let id = r.short()?;
                if id != 0 {
                    *favourite = Some((id, f64::from(r.word()?) / 1000.));
                }
            }
            if let Some(record) = record.as_deref_mut() {
                record.tallies = tallies;
                record.favourites = favourites;
                record.victims.clear();
            }
            for _ in 0..r.byte()? {
                let victim = (r.short()?, r.byte()?);
                if let Some(record) = record.as_deref_mut() {
                    record.victims.push(victim);
                }
            }
        }
    }
    r.0.is_empty().then_some(last)
}
impl Roster {
    // A new network epoch cannot inherit Character slots from the old battle.
    pub fn reset(&mut self) {
        self.characters[..self.total].fill(Character::default());
        self.records[..self.total].fill(Records::default());
        self.own_record = (None, Records::default());
        self.total = 0;
        self.local = None;
    }
    // Preserve roster order for score ties and the network player list.
    pub fn rows(&self) -> impl Iterator<Item = (usize, &Character)> {
        self.characters[..self.total].iter().enumerate()
    }
    /// A Character's end-of-match record by roster slot (applyWorldSummary
    /// schema 5): until the scores stop, the player's own comes only from his
    /// own frequent summary; after, from whichever summary named it last.
    pub fn record(&self, slot: usize, game_over: bool) -> Option<&Records> {
        let own = (self.own_record.0 == Some(slot)).then_some(&self.own_record.1);
        let all = self.records.get(slot).filter(|pair| pair.iter().any(Option::is_some));
        if game_over { all.or(own) } else { own.or(all) }
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
        if schema == 5 {
            // OWN_RECORD_ID 79999 carries the player's own; 80000 + n everyone's.
            if id < 79999 {
                return None;
            }
            decode_records(bytes, None)?;
            if id == 79999 {
                let slot = decode_records(bytes, Some(std::slice::from_mut(&mut self.own_record.1)))?;
                self.own_record.0 = slot;
            } else {
                decode_records(bytes, Some(&mut self.records))?;
            }
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
            self.records[total..self.total].fill(Records::default());
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
    #[test]
    fn record_publications_keep_tallies_favourites_and_victims() {
        // One entry, roster slot 3, second team slot only.
        let mut bytes = vec![1, 3, 0, 0b10, 1, 2, 3, 4, 5, 44, 1, 6, 7];
        bytes.extend(12500u32.to_le_bytes()); // longest life, ms
        bytes.extend(4000u32.to_le_bytes()); // longest camp, ms
        bytes.extend([9, 0]); // favourite class: registry id 9
        bytes.extend(65000u32.to_le_bytes());
        bytes.extend([0, 0, 0, 0]); // no favourite weapon or vehicle
        bytes.extend([1, 2, 0, 4]); // one victim: slot 2, killed 4 times
        let mut roster = Roster::default();
        assert_eq!(roster.receive(5, 80000, &bytes), Some(()));
        let record = roster.record(3, true).unwrap()[1].as_ref().unwrap();
        assert_eq!(record.tallies, [1., 2., 3., 4., 5., 300., 6., 7., 12.5, 4.]);
        assert_eq!(record.favourites, [Some((9, 65.)), None, None]);
        assert_eq!(record.victims, [(2, 4)]);
        assert!(roster.record(3, true).unwrap()[0].is_none());
        // His own frequent record wins until the scores stop.
        let mut own = bytes.clone();
        own[4] = 9;
        assert_eq!(roster.receive(5, 79999, &own), Some(()));
        assert_eq!(roster.record(3, false).unwrap()[1].as_ref().unwrap().tallies[0], 9.);
        assert_eq!(roster.record(3, true).unwrap()[1].as_ref().unwrap().tallies[0], 1.);
        // A cut publication changes nothing.
        assert_eq!(roster.receive(5, 80000, &own[..own.len() - 1]), None);
        assert_eq!(roster.record(3, true).unwrap()[1].as_ref().unwrap().tallies[0], 1.);
    }
}
