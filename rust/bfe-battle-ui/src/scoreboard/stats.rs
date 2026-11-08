// Battle statistics, personal stats rows, post-battle awards, and scoreboard port.
//
// Ports `src/battle-stats.js` (`RING_SIZE`, `slotOf`, `recordOf`, `packRecord`,
// `applyRecord`, `clock`, `personalStats`, `awards`), `src/scoreboard/port.js`
// (`LINK_LABELS`, `scoreboardPort`), and `src/runtime/network-entity-summary.js`
// (schemas 1 through 5).

use std::collections::BTreeMap;

use crate::shapes::GameFamily;

/// Ring buffer size for the shared 5-second killing spree window (`RING_SIZE` in `src/battle-stats.js`).
pub const RING_SIZE: usize = 100;

/// Maps a team number to its per-character record slot (`slotOf` in `src/battle-stats.js`).
///
/// Team 2 uses slot 1; every other team uses slot 0.
#[inline]
pub fn slot_of(team: i32) -> usize {
    if team == 2 {
        1
    } else {
        0
    }
}

/// Connection label strings displayed in the `CONNECTION` row of `DETAILS` (`LINK_LABELS` in `src/scoreboard/port.js`).
pub fn link_label(kind: &str) -> &'static str {
    match kind {
        "direct4" => "DIRECT IPV4",
        "direct6" => "DIRECT IPV6",
        "relay4" => "RELAY IPV4",
        "relay6" => "RELAY IPV6",
        _ => "",
    }
}

/// Time-tracked entry for favorite class, weapon, or vehicle (`src/battle-stats.js`).
#[derive(Clone, Debug, PartialEq)]
pub struct TimedEntry {
    pub key: String,
    pub label: Option<String>,
    pub time: f64,
}

/// Per-team-slot tallies inside a character's record (`slotRecord` in `src/battle-stats.js`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotRecord {
    pub team_kills: i32,
    pub vehicles_destroyed: i32,
    pub spree: i32,
    pub survivalist: i32,
    pub life_kills: i32,
    pub life_kills_best: i32,
    pub headshots: i32,
    pub deaths: i32,
    pub life: f64,
    pub life_best: f64,
    pub camp: f64,
    pub camp_best: f64,
    pub escapes: i32,
    /// Victim character slot -> number of kills of that victim (`victims` in `src/battle-stats.js`).
    pub victims: BTreeMap<usize, i32>,
    /// Up to 5 classes tracked by time (`classes` in `src/battle-stats.js`).
    pub classes: Vec<TimedEntry>,
    /// Up to 10 weapons tracked by time (`weapons` in `src/battle-stats.js`).
    pub weapons: Vec<TimedEntry>,
    /// Up to 5 vehicles tracked by time (`vehicles` in `src/battle-stats.js`).
    pub vehicles: Vec<TimedEntry>,
}

impl SlotRecord {
    /// Adds elapsed time to a timed table (`classes`, `weapons`, or `vehicles`),
    /// evicting the entry with the least accumulated time when full (`src/battle-stats.js`).
    pub fn add_time(
        table: &mut Vec<TimedEntry>,
        capacity: usize,
        key: &str,
        label: Option<&str>,
        dt: f64,
    ) {
        if key.is_empty() || dt <= 0.0 {
            return;
        }
        if let Some(existing) = table.iter_mut().find(|e| e.key == key) {
            existing.time += dt;
            if existing.label.is_none() {
                existing.label = label.map(|s| s.to_string());
            }
            return;
        }
        let entry = TimedEntry {
            key: key.to_string(),
            label: label.map(|s| s.to_string()),
            time: dt,
        };
        if table.len() < capacity {
            table.push(entry);
        } else if let Some((min_idx, _)) = table
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal))
        {
            table[min_idx] = entry;
        }
    }
}

/// Complete two-slot battle record for one character (`recordOf` in `src/battle-stats.js`).
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterRecord {
    pub slots: [SlotRecord; 2],
    pub anchor: Option<[f64; 3]>,
    pub health: i32,
    pub low: bool,
}

impl Default for CharacterRecord {
    fn default() -> Self {
        Self {
            slots: [SlotRecord::default(), SlotRecord::default()],
            anchor: None,
            health: 255,
            low: false,
        }
    }
}

/// Character entry combining roster scores and detailed `CharacterRecord` (`src/battle.js` + `src/battle-stats.js`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharacterStatsEntry {
    pub slot: usize,
    pub team: i32,
    pub name: String,
    pub kills: i32,
    pub team_kills: i32,
    pub deaths: i32,
    pub points: i32,
    pub human: bool,
    pub local: bool,
    pub ping: Option<i32>,
    pub link_kind: String,
    pub record: CharacterRecord,
}

impl CharacterStatsEntry {
    /// Net kills shown on the scoreboard (`kills - 2 * teamKills`, `shownStats` in `src/battle.js`).
    #[inline]
    pub fn shown_kills(&self) -> i32 {
        self.kills - 2 * self.team_kills
    }

    /// Converts this character into a `ScoreRow` for the scoreboard table (`roster` in `src/scoreboard/port.js`).
    pub fn to_score_row(&self, network: bool) -> ScoreRow {
        ScoreRow {
            slot: self.slot,
            team: self.team,
            name: self.name.clone(),
            kills: self.shown_kills(),
            deaths: self.deaths,
            points: self.points,
            human: self.human,
            local: self.local,
            ping: if network && self.human {
                self.ping
            } else {
                None
            },
        }
    }
}

/// Formats whole seconds as `m:ss` with unpadded minutes (`clock` in `src/battle-stats.js`).
pub fn clock(seconds: f64) -> String {
    let s = (seconds.trunc() as i64).max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

/// A labeled personal stat row for the `DETAILS` page (`personalStats` in `src/battle-stats.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonalStatRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// The 9 personal stat keys and fallback labels in screen order (`PERSONAL` in `src/battle-stats.js`).
pub const PERSONAL: [(&str, &str); 9] = [
    ("ifs.stats.headshots", "HEAD SHOTS"),
    ("ifs.stats.favclass", "FAVORITE CLASS"),
    ("ifs.stats.favweapon", "FAVORITE WEAPON"),
    ("ifs.stats.favvehicle", "FAVORITE VEHICLE"),
    ("ifs.stats.livingstreak", "LONGEST LIVING STREAK"),
    ("ifs.stats.narrowescapes", "NARROW ESCAPES"),
    ("ifs.stats.vehiclesdestroyed", "VEHICLES DESTROYED"),
    ("ifs.stats.nemesis", "NEMESIS"),
    ("ifs.stats.bait", "BAIT"),
];

fn favourite<'a>(
    record: &'a CharacterRecord,
    select: impl Fn(&'a SlotRecord) -> &'a [TimedEntry],
) -> Option<&'a TimedEntry> {
    let mut best: Option<&'a TimedEntry> = None;
    for s in &record.slots {
        for e in select(s) {
            if best.map(|b| e.time > b.time).unwrap_or(true) {
                best = Some(e);
            }
        }
    }
    best
}

fn nemesis(target_slot: usize, characters: &[CharacterStatsEntry]) -> Option<(usize, i32)> {
    let mut best = None;
    let mut most = 0;
    for (idx, other) in characters.iter().enumerate() {
        if other.slot == target_slot {
            continue;
        }
        let n: i32 = other
            .record
            .slots
            .iter()
            .map(|s| s.victims.get(&target_slot).copied().unwrap_or(0))
            .sum();
        if n > most {
            most = n;
            best = Some(idx);
        }
    }
    best.map(|idx| (idx, most))
}

fn bait(record: &CharacterRecord) -> Option<(usize, i32)> {
    let mut counts: BTreeMap<usize, i32> = BTreeMap::new();
    // Preserve insertion/slot iteration order by walking slot 0 then slot 1.
    let mut order: Vec<usize> = Vec::new();
    for s in &record.slots {
        for (&victim, &n) in &s.victims {
            if !counts.contains_key(&victim) {
                order.push(victim);
            }
            *counts.entry(victim).or_insert(0) += n;
        }
    }
    let mut best = None;
    let mut most = 0;
    for victim in order {
        let n = counts.get(&victim).copied().unwrap_or(0);
        if n > most {
            most = n;
            best = Some(victim);
        }
    }
    best.map(|v| (v, most))
}

/// Builds the 9 personal stat rows for `character` (`personalStats` in `src/battle-stats.js`).
///
/// `label_fn(kind, key)` resolves display names for `"class"`, `"weapon"`, or `"vehicle"` keys
/// when a `TimedEntry` has no inline label.
pub fn personal_stats(
    character: &CharacterStatsEntry,
    characters: &[CharacterStatsEntry],
    mut label_fn: impl FnMut(&str, &str) -> Option<String>,
) -> Vec<PersonalStatRow> {
    let rec = &character.record;
    let fav = |entries: Option<&TimedEntry>,
               kind: &str,
               label_fn: &mut dyn FnMut(&str, &str) -> Option<String>|
     -> String {
        if let Some(e) = entries {
            let named = e.label.clone().or_else(|| label_fn(kind, &e.key));
            if let Some(n) = named {
                if !n.is_empty() {
                    return format!("{} {}", n, clock(e.time));
                }
            }
        }
        String::new()
    };

    let headshots: i32 = rec.slots.iter().map(|s| s.headshots).sum();
    let fav_class = fav(favourite(rec, |s| &s.classes), "class", &mut label_fn);
    let fav_weapon = fav(favourite(rec, |s| &s.weapons), "weapon", &mut label_fn);
    let fav_vehicle = fav(favourite(rec, |s| &s.vehicles), "vehicle", &mut label_fn);
    let best_life: i64 = rec.slots.iter().map(|s| s.life_best.trunc() as i64).sum();
    let escapes: i32 = rec.slots.iter().map(|s| s.escapes).sum();
    let vehicles_destroyed: i32 = rec.slots.iter().map(|s| s.vehicles_destroyed).sum();
    let nemesis_str = nemesis(character.slot, characters)
        .and_then(|(idx, count)| characters.get(idx).map(|c| format!("{} ({})", c.name, count)))
        .unwrap_or_default();
    let bait_str = bait(rec)
        .and_then(|(slot, count)| {
            characters
                .iter()
                .find(|c| c.slot == slot)
                .map(|c| format!("{} ({})", c.name, count))
        })
        .unwrap_or_default();

    let values = [
        headshots.to_string(),
        fav_class,
        fav_weapon,
        fav_vehicle,
        format!(" {}", clock(best_life as f64)),
        escapes.to_string(),
        vehicles_destroyed.to_string(),
        nemesis_str,
        bait_str,
    ];

    PERSONAL
        .iter()
        .enumerate()
        .map(|(i, &(key, label))| PersonalStatRow {
            key,
            label,
            value: values[i].clone(),
        })
        .collect()
}

/// One post-battle award row for the `AWARDS` page (`awards` in `src/battle-stats.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AwardRow {
    pub key: &'static str,
    pub label: &'static str,
    pub winner_slot: Option<usize>,
    pub team: Option<i32>,
    pub value: i32,
    pub text: String,
}

/// The 8 post-battle awards in screen order (`AWARDS` in `src/battle-stats.js`).
pub const AWARDS: [(&str, &str); 8] = [
    ("ifs.stats.tankbuster", "TANK BUSTER"),
    ("ifs.stats.killingspree", "KILLING SPREE"),
    ("ifs.stats.camper", "CAMPER"),
    ("ifs.stats.survivalist", "SURVIVALIST"),
    ("ifs.stats.traitor", "TRAITOR"),
    ("ifs.stats.publicenemy", "PUBLIC ENEMY"),
    ("ifs.stats.meat", "BANTHA FODDER"),
    ("ifs.stats.deadeye", "DEAD EYE"),
];

/// Computes the 8 post-battle awards across `characters` (`awards` in `src/battle-stats.js`).
///
/// Scans slot 0 across every character in roster order, then slot 1 across every character
/// in roster order; only strictly greater (`v > best`) wins, so ties go to the first and 0 wins nothing.
pub fn awards(characters: &[CharacterStatsEntry]) -> Vec<AwardRow> {
    AWARDS
        .iter()
        .enumerate()
        .map(|(award_idx, &(key, label))| {
            let mut best: i32 = 0;
            let mut winner: Option<&CharacterStatsEntry> = None;
            for slot_idx in [0usize, 1usize] {
                for ch in characters {
                    let s = &ch.record.slots[slot_idx];
                    let v = match award_idx {
                        0 => s.vehicles_destroyed,
                        1 => s.spree,
                        2 => s.camp_best.trunc() as i32,
                        3 => s.survivalist,
                        4 => s.team_kills,
                        5 => s.life_kills_best,
                        6 => s.deaths,
                        7 => s.headshots,
                        _ => 0,
                    };
                    if v > best {
                        best = v;
                        winner = Some(ch);
                    }
                }
            }
            let formatted = if award_idx == 2 {
                clock(best as f64)
            } else {
                best.to_string()
            };
            let text = winner
                .map(|w| format!("{} ({})", w.name, formatted))
                .unwrap_or_default();
            AwardRow {
                key,
                label,
                winner_slot: winner.map(|w| w.slot),
                team: winner.map(|w| w.team),
                value: best,
                text,
            }
        })
        .collect()
}

/// One row of the scoreboard table (`roster()` in `src/scoreboard/port.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreRow {
    pub slot: usize,
    pub team: i32,
    pub name: String,
    pub kills: i32,
    pub deaths: i32,
    pub points: i32,
    pub human: bool,
    pub local: bool,
    pub ping: Option<i32>,
}

impl ScoreRow {
    /// Creates a `ScoreRow` for a team member.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot: usize,
        team: i32,
        name: impl Into<String>,
        kills: i32,
        deaths: i32,
        points: i32,
        human: bool,
        local: bool,
        ping: Option<i32>,
    ) -> Self {
        Self {
            slot,
            team,
            name: name.into(),
            kills,
            deaths,
            points,
            human,
            local,
            ping,
        }
    }
}

/// One row of the pause menu's network `PLAYER LIST` (`players()` in `src/scoreboard/port.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerListRow {
    pub slot: usize,
    pub team: i32,
    pub name: String,
    pub ping: Option<i32>,
    pub link: String,
    pub human: bool,
    pub local: bool,
}

/// Team header metadata for a team column (`teams()` in `src/scoreboard/port.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamHeader {
    pub name: String,
    pub reinforcements: i32,
    pub side: Option<String>,
    pub logo: Option<String>,
}

/// Match lifecycle flags read by the scoreboard (`state()` in `src/scoreboard/port.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchStateFlags {
    pub over: bool,
    pub game_over: bool,
    pub finished: bool,
}

/// Port interface between the battle world and the scoreboard (`src/scoreboard/port.js`).
pub trait ScoreboardPort {
    /// Game family of the map (`Swbf1` today, `Swbf2` later; `MULTIPLAYER.md`).
    fn family(&self) -> GameFamily {
        GameFamily::Swbf1
    }

    /// Whether this battle is a network game (enables `PING` and `CONNECTION` rows).
    fn network(&self) -> bool {
        false
    }

    /// Attached human players in roster order for network games, or `None` offline.
    fn players(&self) -> Option<Vec<PlayerListRow>> {
        None
    }

    /// Every character on teams 1 and 2 in roster order, bots included.
    fn roster(&self) -> Vec<ScoreRow>;

    /// Team headers for teams 1 and 2 (`[team_1, team_2]`).
    fn teams(&self) -> [Option<TeamHeader>; 2];

    /// `DETAILS` rows for the character at `slot`.
    fn personal(&self, slot: usize) -> Vec<PersonalStatRow>;

    /// `AWARDS` rows for the end-of-match screen.
    fn awards(&self) -> Vec<AwardRow>;

    /// The local human player's team (`1` or `2`).
    fn human_team(&self) -> i32 {
        1
    }

    /// Team `a`'s relation to team `b` (`1` friend, `0` neutral, `-1` enemy).
    fn relation(&self, a: i32, b: i32) -> i32 {
        if a == b {
            1
        } else {
            -1
        }
    }

    /// Match end flags (`over`, `game_over`, `finished`).
    fn state(&self) -> MatchStateFlags {
        MatchStateFlags::default()
    }

    /// Whether the `DONE` button on the end screen can advance/restart the battle.
    fn can_restart(&self) -> bool {
        true
    }

    /// Advances/restarts the battle when `DONE` or the 30s idle timer fires.
    fn restart(&mut self) -> bool {
        true
    }
}

/// Self-contained implementation of `ScoreboardPort` backed by a slice of `CharacterStatsEntry`
/// (`scoreboardPort` in `src/scoreboard/port.js`).
#[derive(Clone, Debug, Default)]
pub struct WorldScoreboardPort {
    pub family: GameFamily,
    pub network: bool,
    pub characters: Vec<CharacterStatsEntry>,
    pub teams: [Option<TeamHeader>; 2],
    pub human_team: i32,
    pub state: MatchStateFlags,
    pub can_restart: bool,
    pub restarted_count: usize,
    pub class_labels: BTreeMap<String, String>,
    pub weapon_labels: BTreeMap<String, String>,
    pub vehicle_labels: BTreeMap<String, String>,
}

impl WorldScoreboardPort {
    /// Creates a new `WorldScoreboardPort` with sensible defaults for teams 1 and 2.
    pub fn new() -> Self {
        Self {
            family: GameFamily::Swbf1,
            network: false,
            characters: Vec::new(),
            teams: [
                Some(TeamHeader {
                    name: "REPUBLIC".to_string(),
                    reinforcements: 150,
                    side: Some("rep".to_string()),
                    logo: Some("com_icon_republic".to_string()),
                }),
                Some(TeamHeader {
                    name: "CIS".to_string(),
                    reinforcements: 150,
                    side: Some("cis".to_string()),
                    logo: Some("com_icon_cis".to_string()),
                }),
            ],
            human_team: 1,
            state: MatchStateFlags::default(),
            can_restart: true,
            restarted_count: 0,
            class_labels: BTreeMap::new(),
            weapon_labels: BTreeMap::new(),
            vehicle_labels: BTreeMap::new(),
        }
    }
}

impl ScoreboardPort for WorldScoreboardPort {
    fn family(&self) -> GameFamily {
        self.family
    }

    fn network(&self) -> bool {
        self.network
    }

    fn players(&self) -> Option<Vec<PlayerListRow>> {
        if !self.network {
            return None;
        }
        let mut out = Vec::new();
        for ch in &self.characters {
            if !ch.human || (ch.team != 1 && ch.team != 2) {
                continue;
            }
            out.push(PlayerListRow {
                slot: ch.slot,
                team: ch.team,
                name: ch.name.clone(),
                ping: ch.ping,
                link: if ch.link_kind.is_empty() {
                    "unknown".to_string()
                } else {
                    ch.link_kind.clone()
                },
                human: true,
                local: ch.local,
            });
        }
        Some(out)
    }

    fn roster(&self) -> Vec<ScoreRow> {
        self.characters
            .iter()
            .filter(|ch| ch.team == 1 || ch.team == 2)
            .map(|ch| ch.to_score_row(self.network))
            .collect()
    }

    fn teams(&self) -> [Option<TeamHeader>; 2] {
        self.teams.clone()
    }

    fn personal(&self, slot: usize) -> Vec<PersonalStatRow> {
        let Some(ch) = self.characters.iter().find(|c| c.slot == slot) else {
            return Vec::new();
        };
        let mut rows = personal_stats(ch, &self.characters, |kind, key| match kind {
            "class" => self.class_labels.get(key).cloned(),
            "weapon" => self.weapon_labels.get(key).cloned(),
            "vehicle" => self.vehicle_labels.get(key).cloned(),
            _ => None,
        });
        if self.network && ch.human {
            rows.push(PersonalStatRow {
                key: "ifs.MPLobby.qos_header",
                label: "CONNECTION",
                value: link_label(&ch.link_kind).to_string(),
            });
        }
        rows
    }

    fn awards(&self) -> Vec<AwardRow> {
        awards(&self.characters)
    }

    fn human_team(&self) -> i32 {
        self.human_team
    }

    fn state(&self) -> MatchStateFlags {
        self.state
    }

    fn can_restart(&self) -> bool {
        self.can_restart
    }

    fn restart(&mut self) -> bool {
        if !self.can_restart {
            return false;
        }
        self.restarted_count += 1;
        true
    }
}

/// Network player row carried in `NetworkEntitySummary` (`src/runtime/network-entity-summary.js`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NetworkPlayerRow {
    pub peer_id: i32,
    pub name: String,
    pub team: i32,
    pub class_name: String,
    pub unit_id: i32,
    pub kills: i32,
    pub deaths: i32,
    pub captures: i32,
    pub ping: Option<i32>,
}

/// Decoded network entity summary (`src/runtime/network-entity-summary.js`, schemas 1..5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkEntitySummary {
    pub schema: u32,
    pub tick: u32,
    pub match_state: u32,
    pub winner_team: i32,
    pub tickets1: i32,
    pub tickets2: i32,
    pub players: Vec<NetworkPlayerRow>,
}

impl NetworkEntitySummary {
    /// Normalizes a network entity summary supporting schemas 1 through 5 (`src/runtime/network-entity-summary.js`).
    ///
    /// Schemas 1..4 zero out `captures` and clear `ping` to `None`; schemas outside `1..=5` return `None`.
    pub fn decode(mut summary: Self) -> Option<Self> {
        if !(1..=5).contains(&summary.schema) {
            return None;
        }
        if summary.schema < 5 {
            for p in &mut summary.players {
                p.captures = 0;
                p.ping = None;
            }
        }
        Some(summary)
    }
}
