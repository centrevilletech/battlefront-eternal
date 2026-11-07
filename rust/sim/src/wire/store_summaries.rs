//! captureWorldSummaries over F4's canonical battle, posts and Characters.
//! Only connection link facts are supplied by transport. Record packing reads
//! borrowed tally slots and never constructs packRecord's intermediate tables.
use crate::{gen::*, store::{Nullable, Ref, Strings}};
use super::{codec::Result, stagger::Stagger, store_capture::Capture as Maps};
use jsrt::{js_max, js_min, js_round, num_truthy, to_int32};

#[derive(Clone, Copy)]
pub struct Link { pub rtt: f64, pub has_kind: bool, pub relay: bool, pub has_family: bool, pub family_six: bool }
pub trait Links { fn get(&self, character: Ref<Character>) -> Option<Link>; }
struct Bytes { value: Vec<u8>, limit: usize }
impl Bytes {
    fn new(limit: usize) -> Self { Self { value: Vec::with_capacity(limit), limit } }
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        if self.value.len() + bytes.len() > self.limit { return Err("summary exceeds bounds"); }
        self.value.extend_from_slice(bytes); Ok(())
    }
    fn integer(&mut self, n: f64, size: usize) -> Result<()> {
        let max = match size { 1 => 255., 2 => 65535., _ => 4294967295. };
        if !n.is_finite() || n < 0. || n > max || n.fract() != 0. { return Err("invalid summary scalar"); }
        self.raw(&(n as u32).to_le_bytes()[..size])
    }
    fn u8(&mut self, n: f64) -> Result<()> { self.integer(n, 1) }
    fn u16(&mut self, n: f64) -> Result<()> { self.integer(n, 2) }
    fn u32(&mut self, n: f64) -> Result<()> { self.integer(n, 4) }
    fn number(&mut self, n: f64) -> Result<()> {
        if !n.is_finite() { return Err("invalid summary scalar"); }
        self.raw(&n.to_le_bytes())
    }
    fn flag(&mut self, flag: bool) -> Result<()> { self.u8(u8::from(flag) as f64) }
    fn nullable(&mut self, number: Nullable<f64>) -> Result<()> {
        self.flag(!matches!(number, Nullable::Null))?;
        match number { Nullable::Value(value) => self.number(value), Nullable::Null => Ok(()), _ => Err("invalid summary scalar") }
    }
}
fn tallies(slot: &StatSlot) -> [f64; 10] {
    [slot.team_kills, slot.vehicles_destroyed, slot.spree, slot.survivalist, slot.life_kills_best,
        slot.headshots, slot.deaths, slot.escapes, slot.life_best, slot.camp_best]
}
// packRecord selects the first favourite with the greatest time.
// A tied later entry must not displace the insertion-order winner.
// Authored names are mapped only after selecting that same entry.
fn best(table: &[TimeEntry]) -> Option<&TimeEntry> {
    let mut best = None;
    for entry in table { if best.is_none_or(|old: &TimeEntry| entry.time > old.time) { best = Some(entry); } }
    best
}
// packRecord truncates durations to whole milliseconds or seconds.
// Common end-table records use milliseconds; personal records keep seconds.
// Clamp after truncation to preserve the unsigned wire representation.
fn ms(seconds: f64, quantum: f64) -> f64 { js_min(4294967295., js_max(0., (seconds * 1000. / quantum).floor() * quantum)) }
fn u16_tally(n: f64) -> f64 { js_max(0., js_min(65535., to_int32(n) as f64)) }

pub struct Summaries {
    payload: Bytes, entry: Bytes, roster: Vec<Ref<Character>>, roster_length: Option<usize>,
}
/// Only queued spawn requests remain on the mailbox side. The realized
/// connection phase/class/post and its body identity come from F4.
pub struct Spawn<'a> {
    pub connection: &'a F4ConnectionView, pub identity: (f64, f64), pub team: f64, pub slot: usize,
    pub pending: Option<(f64, f64)>, pub auto: Option<(f64, f64, f64)>,
    pub posts: &'a [f64], pub classes: &'a [f64],
}
impl Default for Summaries {
    fn default() -> Self { Self { payload: Bytes::new(900), entry: Bytes::new(4096),
        roster: Vec::with_capacity(2400), roster_length: None } }
}
impl Summaries {
    // The enclosing producer stages its fields and can fail after these
    // summaries succeeded. Retain the old roster clock until that transaction
    // commits, so retrying the same tick still rebuilds a changed roster.
    pub(crate) fn roster_clock(&self) -> Option<usize> { self.roster_length }
    pub(crate) fn restore_roster_clock(&mut self, clock: Option<usize>) { self.roster_length = clock; }
    /// captureWorldSummaries's personal fields bypass common stagger clocks.
    /// A player's statistics remain whole seconds even at the end table.
    pub fn own(&mut self, tick: f64, epoch: f64, store: &Store, maps: &Maps, character: Ref<Character>,
        spawn: Spawn<'_>, stagger: &mut Stagger) -> Result<()> {
        if spawn.posts.len() > 64 || spawn.classes.len() > 64 { return Err("spawn eligibility limit"); }
        // ownSummaryFields builds independently for each connection.
        // Clear the temporary native stagger so an omitted personal record cannot
        // inherit the previous recipient's statistics on the same tick.
        self.prepare_roster(store)?; stagger.clear_native()?; stagger.begin();
        let out = &mut self.payload; let view = spawn.connection; out.value.clear();
        out.u32(spawn.identity.0)?; out.u32(spawn.identity.1)?; out.u8(spawn.team)?; out.u8(view.phase as u8 as f64)?;
        let class = view.class_name.map_or(0., |n| maps.class_id(n));
        if view.class_name.is_some() && class == 0. { return Err("summary registry prerequisite missing"); }
        out.u16(class)?; out.u16(view.post.map(|n| maps.post_id(n).ok_or("summary registry prerequisite missing")).transpose()?.unwrap_or(0.))?;
        out.flag(view.spawned_once)?; out.flag(view.ended)?; out.number(view.dead_time)?;
        out.flag(spawn.pending.is_some())?;
        if let Some((class, post)) = spawn.pending { out.u16(class)?; out.u16(post)?; }
        out.flag(spawn.auto.is_some())?;
        if let Some((team, class, post)) = spawn.auto { out.u8(team)?; out.u16(class)?; out.u16(post)?; }
        out.u8(spawn.posts.len() as f64)?; for &id in spawn.posts { out.u16(id)?; }
        out.u8(spawn.classes.len() as f64)?; for &id in spawn.classes { out.u16(id)?; }
        out.u8(if spawn.slot < 255 { spawn.slot as f64 } else { 255. })?;
        stagger.offer_native(tick, 3., 65536., epoch, &out.value, None)?;
        self.records(store, maps, Some(character), 79999., 1000., tick, epoch, stagger)?;
        stagger.finish(&[3., 5.]); Ok(())
    }
    /// captureSnapshot's common fields retain their separate clocks: battle
    /// and posts at 6 ticks, roster at 60, statistics at 300. A new roster
    /// length forces recapture even when no old roster chunk is due yet.
    pub fn take(&mut self, tick: f64, epoch: f64, store: &Store, strings: &Strings, maps: &Maps,
        links: &impl Links, stagger: &mut Stagger) -> Result<()> {
        if !stagger.any_due(tick, &[1., 2., 4., 5.]) { return Ok(()); }
        // captureWorldSummaries observes roster growth before any chunk due check.
        // An appended Character forces every roster chunk to use the new count.
        // Other common fields keep their original insertion and cadence behavior.
        let count = store.battle.characters.len();
        let roster = self.roster_length != Some(count) || stagger.schema_due(tick, &[4.]);
        let records = stagger.schema_due(tick, &[5.]);
        self.battle(&store.battle)?;
        stagger.offer_native(tick, 1., 0., epoch, &self.payload.value, None)?;
        for reference in &store.command_posts.posts {
            let Some(post) = store.post.get(*reference) else { continue };
            let id = maps.post_id(post.name).ok_or("summary registry prerequisite missing")?;
            self.post(post, id)?; stagger.offer_native(tick, 2., id, epoch, &self.payload.value, None)?;
        }
        // Common schemas complete only when captured. Other publication
        // schemas are swept separately by their own producer below this one.
        let mut complete = [0.; 4]; complete[0] = 1.; complete[1] = 2.; let mut completed = 2;
        if roster {
            self.roster(store, strings, links, tick, epoch, stagger)?;
            self.roster_length = Some(count); complete[completed] = 4.; completed += 1;
        }
        if records {
            self.prepare_roster(store)?;
            self.records(store, maps, None, 80000., if store.battle.game_over { 1. } else { 1000. }, tick, epoch, stagger)?;
            complete[completed] = 5.; completed += 1;
        }
        stagger.finish(&complete[..completed]); Ok(())
    }
    // captureRecordSummaries indexes the retained Character roster.
    // The registry's mixed Unit/prop ordering is a different identity space.
    // Reserve the complete roster at attachment and reject excess recoverably.
    fn prepare_roster(&mut self, store: &Store) -> Result<()> {
        if store.battle.characters.len() > 2400 { return Err("roster summary limit"); }
        self.roster.clear(); self.roster.extend_from_slice(&store.battle.characters); Ok(())
    }
    // captureBattleSummary walks reinforcement keys in JS Map order.
    // Missing start/no-post/bleed entries contribute zero for that same key.
    // Nullable victory and countdown clocks retain their presence markers.
    fn battle(&mut self, battle: &Battle) -> Result<()> {
        let out = &mut self.payload; out.value.clear();
        if battle.reinforcements.len() > 7 { return Err("summary team limit"); }
        out.u8(battle.reinforcements.len() as f64)?;
        for (team, reinforcements) in battle.reinforcements.iter() {
            out.u8(team)?; out.number(*reinforcements)?;
            for table in [&battle.start, &battle.no_post, &battle.bleed] { out.number(table.get(team).copied().unwrap_or(0.))?; }
        }
        for flag in [battle.over, battle.game_over, battle.finished, battle.losing] { out.flag(flag)?; }
        out.u8(battle.loser.filter(|v| num_truthy(*v)).unwrap_or(0.))?; out.number(battle.over_time)?;
        out.nullable(battle.victory_clock)?; out.nullable(battle.countdown)
    }
    // capturePostSummary reads the completed capture counters unchanged.
    // It must not call CommandPost.update or recompute nearby unit counts.
    // The native post name is translated through the cold protocol registry.
    fn post(&mut self, post: &Post, id: f64) -> Result<()> {
        let out = &mut self.payload; out.value.clear(); out.u16(id)?; out.u8(post.team)?; out.u8(post.toward)?;
        out.number(post.progress)?; out.number(post.neutralize)?; out.flag(post.contested)?; out.flag(post.active)?;
        if post.counts.len() > 7 { return Err("summary post team limit"); }
        out.u8(post.counts.len() as f64)?;
        for (team, count) in post.counts.iter() { out.u8(team)?; out.u16(*count)?; }
        Ok(())
    }
    // captureRoster chunks twelve Characters per field in roster order.
    // An unattached human has team zero even if its Character retains a team.
    // Transport attachment, RTT and path facts share the single input block.
    fn roster(&mut self, store: &Store, strings: &Strings, links: &impl Links,
        tick: f64, epoch: f64, stagger: &mut Stagger) -> Result<()> {
        self.prepare_roster(store)?;
        for start in (0..self.roster.len()).step_by(12) {
            let out = &mut self.payload; out.value.clear();
            let count = (self.roster.len() - start).min(12);
            out.u16(self.roster.len() as f64)?; out.u16(start as f64)?; out.u8(count as f64)?;
            for &reference in &self.roster[start..start + count] {
                let character = store.character.get(reference).ok_or("roster character prerequisite missing")?;
                let link = links.get(reference); let attached = link.is_some();
                out.u8(if !attached && character.human { 0. } else { js_max(0., js_min(255., to_int32(character.team) as f64)) })?;
                out.flag(attached)?;
                for n in [character.kills, character.team_kills, character.deaths, character.captures] { out.u16(u16_tally(n))?; }
                out.u16(link.map_or(65535., |link| if link.rtt.is_finite() { js_min(65534., js_max(0., js_round(link.rtt))) } else { 65535. }))?;
                out.u8(link.map_or(0., |link| if link.has_kind && link.has_family { (if link.relay { 3. } else { 1. }) + f64::from(u8::from(link.family_six)) } else { 0. }))?;
                let mut name = [0; 32]; let mut length = 0;
                if let Some(id) = character.name {
                    // TextEncoder replaces lone surrogates; the old wire then
                    // cuts bytes at 32 even when that splits a UTF-8 sequence.
                    for ch in char::decode_utf16(strings.units(id).iter().copied()) {
                        let mut bytes = [0; 4]; let bytes = ch.unwrap_or(char::REPLACEMENT_CHARACTER).encode_utf8(&mut bytes).as_bytes();
                        let count = bytes.len().min(32 - length); name[length..length + count].copy_from_slice(&bytes[..count]);
                        length += count; if length == 32 { break; }
                    }
                }
                out.u8(length as f64)?; out.raw(&name[..length])?;
            }
            stagger.offer_native(tick, 4., 70000. + (start / 12) as f64, epoch, &out.value, None)?;
        }
        Ok(())
    }
    // packRecord resolves victims by the qualified Character reference.
    // A reused slab slot with another generation is no longer that victim.
    // Entries outside the current roster do not keep an otherwise empty slot.
    fn victim(&self, reference: Ref<Character>) -> Option<usize> { self.roster.iter().position(|&r| r == reference) }
    fn nonzero(&self, slot: &StatSlot) -> bool {
        tallies(slot).into_iter().any(num_truthy) || slot.victims.iter().any(|(r, _)| self.victim(r).is_some())
            || !slot.classes.is_empty() || !slot.weapons.is_empty() || !slot.vehicles.is_empty()
    }
    /// captureRecordSummaries packs a complete Character entry before joining
    /// a 900-byte chunk. Oversized entries are skipped, and the next valid one
    /// keeps its roster identity rather than inheriting a skipped position.
    pub fn records(&mut self, store: &Store, maps: &Maps, only: Option<Ref<Character>>, id: f64,
        quantum: f64, tick: f64, epoch: f64, stagger: &mut Stagger) -> Result<()> {
        self.payload.value.clear(); self.payload.value.push(0); let mut count = 0; let mut chunk = 0;
        for index in 0..self.roster.len() {
            let reference = self.roster[index]; if only.is_some_and(|r| r != reference) { continue; }
            let Some(character) = store.character.get(reference) else { continue };
            let Nullable::Value(record) = &character.record else { continue };
            if record.slots.len() > 2 { return Err("invalid summary scalar"); }
            let mut mask = 0;
            for (slot, value) in record.slots.iter().enumerate() { if self.nonzero(value) { mask |= 1 << slot; } }
            if mask == 0 { continue; }
            self.entry.value.clear(); self.entry.u16(index as f64)?; self.entry.u8(mask as f64)?;
            for (index, slot) in record.slots.iter().enumerate() {
                if mask & (1 << index) == 0 { continue; }
                for (i, n) in tallies(slot).into_iter().enumerate() {
                    if i >= 8 { self.entry.u32(ms(n, quantum))?; }
                    else if i == 5 { self.entry.u16(js_min(65535., to_int32(n) as f64))?; }
                    else { self.entry.u8(js_min(255., to_int32(n) as f64))?; }
                }
                for (which, table) in [&slot.classes, &slot.weapons, &slot.vehicles].into_iter().enumerate() {
                    let favourite = best(table); let id = favourite.map_or(0., |entry|
                        if which == 1 { maps.weapon_name_id(entry.key) } else { maps.class_id(entry.key) });
                    self.entry.u16(id)?; if id != 0. { self.entry.u32(ms(favourite.unwrap().time, quantum))?; }
                }
                // packRecord keeps victim insertion order, capped at 255 pairs.
                // Filter missing roster lives before calculating the emitted count.
                // The wire stores a roster index, never a transient native slab index.
                let roster = &self.roster;
                let count = slot.victims.iter().filter(|(r, _)| roster.contains(r)).take(255).count();
                self.entry.u8(count as f64)?;
                for (victim, n) in slot.victims.iter().filter_map(|(r, n)| roster.iter().position(|&v| v == r).map(|i| (i, *n))).take(255) {
                    self.entry.u16(victim as f64)?; self.entry.u8(js_min(255., to_int32(n) as f64))?;
                }
            }
            // captureRecordSummaries skips an entry too large for a chunk.
            // Flush a previous nonempty chunk only when the next valid entry needs it.
            // This keeps chunk ids stable when a single Character has oversized stats.
            if self.entry.value.len() > 900 { continue; }
            if count != 0 && (self.payload.value.len() + self.entry.value.len() > 900 || count == 255) {
                self.payload.value[0] = count; stagger.offer_native(tick, 5., id + chunk as f64, epoch, &self.payload.value, None)?;
                chunk += 1; count = 0; self.payload.value.clear(); self.payload.value.push(0);
            }
            self.payload.raw(&self.entry.value)?; count += 1;
        }
        if count != 0 { self.payload.value[0] = count; stagger.offer_native(tick, 5., id + chunk as f64, epoch, &self.payload.value, None)?; }
        Ok(())
    }
}
