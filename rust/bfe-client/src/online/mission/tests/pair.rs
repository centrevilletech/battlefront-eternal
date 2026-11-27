//! One world, two views: the observation an online client builds from what a
//! real server world sends it (`Mission::observe`) against the one the same
//! world gives a local client (`local::mission_view::History`), field by
//! field, tick by tick, in the launcher, alive and dead. The server's world
//! runs in this process (`NativeWorldHost`); the client is a real `Receiver`
//! fed the server's own messages and packets for its player.
use super::super::*;
use crate::local::mission_view::History;
use bfe_family::GameFamily;
use bfe_server::{udp_peer::INPUT_MAGIC, world_worker::NativeWorldHost};
use bfe_sim::wire::revisions::{self, Checks, Supports, Table, WRITES, WRITES_SPACE};
use bfe_test_gate::OPEN;
use std::collections::BTreeMap;

#[path = "../../../../../bfe-server/tests/common/mod.rs"]
mod common;

/// The parts of a HUD the observation reads: the player's spawn summary and
/// each unit life's roster slot, as the server sends them (a class's health
/// and a weapon's loadout come from the HUD's tables, which these comparisons
/// leave out).
#[derive(Default)]
struct Panel2005 {
    summary: HudOwnSummary,
    /// Per unit id: its life and its character's roster slot.
    units: std::collections::HashMap<u32, (u32, u16)>,
}
impl Panel for Panel2005 {
    fn summary(&self) -> &HudOwnSummary { &self.summary }
    fn health(&self, _: usize) -> f64 { 1. }
    fn loadout(&self, _: u32) -> Option<(Loadout, usize)> { None }
    fn unit_slot(&self, id: u32, generation: u32) -> Option<usize> {
        self.units.get(&id).filter(|(life, slot)| *life == generation && *slot != u16::MAX).map(|(_, slot)| usize::from(*slot))
    }
}
impl Panel2005 {
    fn read(&mut self, receiver: &Receiver) {
        for field in receiver.publications.iter().filter(|f| f.row[r::KIND] == f64::from(k::SUMMARY)) {
            let bytes = &field.bytes[..field.length];
            if field.row[r::SCHEMA] == 3. && bytes.len() >= 24 {
                (self.summary.valid, self.summary.team, self.summary.phase, self.summary.ended) = (true, bytes[8], bytes[9], bytes[15] != 0);
                self.summary.dead_time = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
            } else if field.row[r::SCHEMA] == 6. {
                if let Some((id, slot)) = crate::online::hud_feed::unit_slot(&field.row, bytes) {
                    self.units.insert(id, (field.row[r::GENERATION] as u32, slot));
                }
            }
        }
    }
}

/// What the player does on one tick: the codes he holds, his look (the
/// total turned, as a client sends it), edges as (kind, payload) and, in a
/// craft or a chair, his flight controls.
#[derive(Clone, Default)]
struct Act {
    held: Vec<usize>,
    look: [f64; 2],
    edges: Vec<(u8, Vec<u8>)>,
    pilot: Option<bfe_sim::space::local::PilotControls>,
}

/// One 2005 input packet as a client sends it: for `tick` of a life, its
/// sequence, the codes held, the look, `edges` as (id, kind, payload), the
/// newest state it has (`ack`, epoch and tick), and in a craft its controls.
#[allow(clippy::too_many_arguments)]
fn input_packet(epoch: u32, generation: u32, sequence: u32, tick: u32, act: &Act, edges: &[(u32, u8, Vec<u8>)], ack: Option<(u16, u32)>) -> Vec<u8> {
    use bfe_sim::wire::revisions::{write_section, Section, PILOT};
    use bfe_sim::wire::space::{write_pilot_entries, PilotEntry, REVISION};
    let mut bytes = INPUT_MAGIC.to_le_bytes().to_vec();
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&generation.to_le_bytes());
    bytes.extend_from_slice(&[1, 1, edges.len() as u8, u8::from(ack.is_some()) | (u8::from(act.pilot.is_some()) << 1)]);
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&tick.to_le_bytes());
    bytes.push(0);
    let mut mask = [0u8; 16];
    for &held in &act.held {
        mask[held / 8] |= 1 << (held % 8);
    }
    bytes.extend_from_slice(&mask);
    bytes.extend_from_slice(&act.look[0].to_le_bytes());
    bytes.extend_from_slice(&act.look[1].to_le_bytes());
    for (id, kind, payload) in edges {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.push(*kind);
        bytes.extend_from_slice(payload);
    }
    if let Some((epoch, tick)) = ack {
        bytes.extend_from_slice(&epoch.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
    }
    if let Some(controls) = act.pilot {
        let mut entries = Vec::new();
        write_pilot_entries(&[PilotEntry { sequence, controls }], &mut entries).unwrap();
        write_section(&[Section { kind: PILOT, revision: REVISION, body: &entries }], &mut bytes).unwrap();
    }
    bytes
}

/// The server's world and one client of it, a tick at a time.
struct Pair {
    host: NativeWorldHost,
    receiver: Box<Receiver>,
    replay: Replay,
    panel: Panel2005,
    history: History,
    now: f64,
    life: u32,
    edge: u32,
    sequence: u32,
    class: String,
}

impl Pair {
    fn start(root: &std::path::Path, query: &str, bare_map: &str) -> Self {
        let host = NativeWorldHost::load_battle(root, GameFamily::Swbf2, query, 1, OPEN).unwrap();
        Self::join(host, root, query, bare_map, 1000.)
    }

    /// A client joining `host`'s battle at `now` (ms) with a receiver of its
    /// own, as a client makes one for every battle it joins.
    fn join(mut host: NativeWorldHost, root: &std::path::Path, query: &str, bare_map: &str, now: f64) -> Self {
        let content_query = bfe_family::swbf2::content_query(query, bare_map).unwrap();
        let textures = crate::scene::TextureOptions { compressed: false, max_size: 4096, capabilities: 0 };
        let folder = GameFamily::Swbf2.content_dir(root);
        let content = crate::scene::Scene::online_content_swbf2(&folder, bare_map, &content_query, textures, None).unwrap();
        let replay = Replay::new(&content).unwrap();
        let defs = content.defs().unwrap();
        let descriptor = crate::online::replicas::registry_descriptor(&defs.registry, GameFamily::Swbf2);
        let bounds = bfe_sim::wire::bounds::for_battle(GameFamily::Swbf2, &content.content.battle.map_data).unwrap();
        let mut receiver = Box::new(Receiver::with_bounds(descriptor, bounds));
        receiver.clock = crate::online::clock::Clock::online();
        receiver.control_family = GameFamily::Swbf2;
        if let Some(space) = &replay.space {
            let meta = space.meta.clone();
            receiver.space = Some(crate::local::space::History::new(meta.counts));
            receiver.space_online = Some(Box::new(crate::online::space_receiver::OnlineSpace::new(meta, bounds)));
        }
        // The match plan a client implementing this build's records makes of the server's table.
        let table = Table::of(if host.has_space() { WRITES_SPACE } else { WRITES });
        let plan = revisions::plan(&table, &Supports::of(revisions::SUPPORTS)).unwrap();
        receiver.network.borrow_mut().check_records(Box::new(Checks::new(plan.clone(), replay.swbf2_classes.clone())));
        receiver.swbf2_plan = Some(plan.clone());
        let text = |receiver: &mut Receiver, value: serde_json::Value| receiver.message(true, &serde_json::to_vec(&value).unwrap(), now);
        // As the server meets a player: the registry, the admission, the plan, the welcome.
        text(&mut receiver, host.registry_message());
        let welcome = host.add_player("one", "One", false, false, true).unwrap();
        host.set_plan("one", Some(plan));
        text(&mut receiver, host.welcome_message(&serde_json::json!({}), "server", &welcome));
        host.handle_start("one", now);
        assert_eq!(receiver.error(), None);
        let history = host.context.owned(|w| History::admit(w)).unwrap().expect("a scripted battle has a mission view");
        Self { host, receiver, replay, panel: Panel2005::default(), history, now, life: welcome.generation, edge: 0, sequence: 0, class: welcome.class_name }
    }

    /// One tick: the player's input, the server's step, what it sends him, and
    /// both views of the tick.
    fn turn(&mut self, held: &[usize], edges: Vec<(u8, Vec<u8>)>) {
        self.act(&Act { held: held.to_vec(), edges, ..Default::default() });
    }

    fn act(&mut self, act: &Act) {
        let edges: Vec<_> = act.edges.iter().map(|(kind, payload)| { self.edge += 1; (self.edge, *kind, payload.clone()) }).collect();
        self.sequence += 1;
        // He says which state he has, as a client does with every input.
        let ack = (self.receiver.latest > 0.).then(|| (self.host.epoch as u16, self.receiver.latest as u32));
        let packet = input_packet(self.host.epoch, self.life, self.sequence, self.host.tick + 1, act, &edges, ack);
        self.host.handle_input("one", &packet, self.now);
        self.now += 1000. / 60.;
        // His transport link, as the server's loop reports it each tick: the
        // roster calls a character attached to one a player.
        let native = self.host.native_by_client["one"];
        self.host.links.clear();
        self.host.links.push(bfe_sim::gen::WirePublicationLink { id: native, discarded_actions: 0., rtt: 40., has_kind: true,
            relay: false, has_family: true, family_six: false });
        let publication = self.host.step(self.now, (1_700_000_000_000. + self.now) as _).unwrap().unwrap();
        assert!(publication.faults.is_empty(), "{:?}", publication.faults);
        let now = self.now;
        // His combat records first (the damage feedback among them), as the
        // server's loop sends them. (Its input receipts would move this
        // client's clock off the publication it is set against: left out, as
        // they always were here.)
        for event in publication.events.iter().filter(|e| e.peer_id == "one" && e.message["type"] == "gameplay-events") {
            self.receiver.message(true, &serde_json::to_vec(&event.message).unwrap(), now);
        }
        for own in publication.own_packets.iter().filter(|p| p.peer_id == "one") {
            self.receiver.message(false, &own.bytes, now);
        }
        for part in publication.shared.iter().filter(|g| g.clients.iter().any(|c| c == "one")).flat_map(|g| &g.parts) {
            self.receiver.message(false, part, now);
        }
        let receiver = &mut self.receiver;
        self.host.send_notices(|peer, bytes| {
            if peer == "one" { receiver.message(true, bytes, now); }
            true
        });
        assert_eq!(self.receiver.error(), None);
        // A new life counts its edges from one again, as a client does.
        if self.receiver.own.alive && self.receiver.own.generation as u32 != self.life {
            (self.life, self.edge) = (self.receiver.own.generation as u32, 0);
        }
        self.panel.read(&self.receiver);
        self.receiver.observe_mission(&self.panel, &self.replay);
        self.receiver.publications.clear();
        let (history, tick) = (&mut self.history, self.host.tick);
        self.host.context.owned(|w| history.sample(w, tick)).unwrap();
    }
}

/// What a player of `team` is shown of the world, where the two views say it
/// in their own ways: a local view keeps every team's notices and names the
/// team, the server sends a player his own and everyone's, unnamed; and the
/// server's world keeps the character it was admitted with, which no peer
/// plays: the world calls it human, the wire (no one attached) does not. The
/// one player here is `own`; every other human of this world is that one.
#[derive(Clone, Copy)]
struct Seen {
    team: i32,
    own: Option<usize>,
}

/// The notices a view shows, oldest first, with the tick each stops at.
fn notices(o: &Observation, seen: Option<Seen>) -> Vec<(u32, String)> {
    let heard = |team: Option<i32>| seen.is_none_or(|seen| team.is_none_or(|team| team == seen.team));
    o.notices.iter().filter_map(|n| match n.value {
        Notice::Message { team, key } => heard(team).then(|| format!("message {key}")),
        Notice::Voice { team, sound } => heard(team).then(|| format!("voice {sound}")),
        // A kill by an unmanned ship gun names the gun in both views: the
        // local one from its world, the online one from the kill cue's gun.
        Notice::Kill { killer, victim, weapon } => Some(format!("kill {:?} {:?} {weapon}",
            killer.map(|k| (k.slot, k.human, k.class)), (victim.slot, victim.human, victim.class))),
    }.map(|text| (n.until, text))).collect()
}

/// Every field both views must agree on, as text, keyed by its name; `seen`
/// is given for the local view only.
fn shared(o: &Observation, seen: Option<Seen>) -> BTreeMap<&'static str, String> {
    let mut f = BTreeMap::new();
    f.insert("tick", o.tick.to_string());
    f.insert("score", format!("{:?} {:?} {:?} {} {} {}", o.score.teams, o.score.target, o.score.outcome, o.score.over, o.score.game_over, o.score.finished));
    f.insert("show_points", format!("{:?}", o.show_points));
    f.insert("reinforcements", format!("{:?}", o.reinforcements));
    f.insert("map_hidden", o.map_hidden.to_string());
    f.insert("posts_hidden", o.posts_hidden.to_string());
    f.insert("timer", format!("{:?}", o.timer.map(|(name, due)| (name.to_string(), due))));
    f.insert("class_room", format!("{:?}", o.class_room));
    f.insert("admission", format!("{:?}", o.admission.teams.iter().map(|t| (t.name.clone(), t.icon.clone(), t.classes.clone())).collect::<Vec<_>>()));
    f.insert("objectives", format!("{:?}", o.objectives.iter().map(|g| (g.id, g.team, g.key.to_string(), g.complete)).collect::<Vec<_>>()));
    // Points are looked up by slot: their order says nothing.
    let mut personal = o.personal.clone();
    personal.sort_unstable();
    f.insert("personal", format!("{personal:?}"));
    let attached = |slot: usize| seen.is_none_or(|seen| seen.own == Some(slot));
    f.insert("roster", format!("{:?}", o.roster.iter().map(|p| p.map(|p| (p.slot, p.human && attached(p.slot), p.class))).collect::<Vec<_>>()));
    f.insert("notices", format!("{:?}", notices(o, seen)));
    // The hit marker: the tick of his last hit and whether it was critical.
    f.insert("hit_marker", format!("{:?}", o.hit_marker));
    // His power lock: whom, on which team, as which participant and class,
    // and the size of its sphere (its place and health are set against each
    // other within a margin, `lock`; how far it was a tick before is the
    // local view's alone, the wire's state is every third tick).
    f.insert("locked", format!("{:?}", o.locked.map(|l| (l.life.slot, l.life.generation, l.team,
        l.participant.map(|p| (p.slot, p.human && attached(p.slot), p.class)), l.class, l.now.radius))));
    f
}

/// Where a view draws his power lock and the health it shows on it.
fn lock(o: &Observation) -> Option<([f64; 3], f32)> {
    o.locked.map(|l| (l.now.center, l.now.health))
}

/// Where two views disagree about the bodies on the battlefield, if they do:
/// the same lives, each on the same team, alive or not and his own or not
/// alike, within 5 cm of the same place and with the same health to half a
/// point. A life the host begins after its world's step is in the local view
/// from the next tick (its lives are counted at the end of the step) and on
/// the wire from this one: an online body the local view lacks must be in its
/// view of the next tick (`next`), as it is there. Two moments of a Space
/// battle are counted in `moments`, not differences: a soldier stepping into a
/// craft's seat, whom the local view moves to the seat in the tick before he
/// leaves the battlefield and the wire leaves where he stood (within 1 m, and
/// gone from the local view of the next tick); and the player's own body for
/// the publication after his craft is destroyed, which his correction calls
/// dying where the local world has no body of him (dead, his own). A life's
/// first step, between where the wire has it and where the local view first
/// does, is counted too.
fn bodies_differ(local: &[Body], online: &[Body], next: Option<&[Body]>, moments: &mut [u32; 3]) -> Option<String> {
    let key = |b: &Body| (b.life.slot, b.life.generation);
    let alike = |a: &Body, b: &Body| {
        let apart = ((a.now.position[0] - b.now.position[0]).powi(2) + (a.now.position[1] - b.now.position[1]).powi(2)).sqrt();
        (a.team == b.team && a.alive == b.alive && a.human == b.human && apart <= 0.05 && (a.now.health - b.now.health).abs() <= 0.5)
            .then_some(()).ok_or_else(|| format!("{:?} t{} {} {} at {:?} h{} | t{} {} {} at {:?} h{} ({apart:.3} m apart)", key(a),
                a.team, a.alive, a.human, a.now.position, a.now.health, b.team, b.alive, b.human, b.now.position, b.now.health))
    };
    let leaves = |b: &Body| next.is_some_and(|next| !next.iter().any(|a| key(a) == key(b)));
    for b in online {
        // A life that begins this tick: the wire has it where the host placed
        // it, the local view from the next tick, after its first step (up to
        // a metre on, as a Space hangar's spawn moves it).
        if !local.iter().any(|a| key(a) == key(b)) {
            if let Some(a) = next.and_then(|next| next.iter().find(|a| key(a) == key(b))) {
                let apart = ((a.now.position[0] - b.now.position[0]).powi(2) + (a.now.position[1] - b.now.position[1]).powi(2)).sqrt();
                if alike(a, b).is_err() {
                    if apart <= 1. && a.team == b.team && a.alive == b.alive && a.human == b.human { moments[2] += 1; } else { return alike(a, b).err(); }
                }
                continue;
            }
        }
        let seen = local.iter().find(|a| key(a) == key(b));
        match seen {
            Some(a) => if let Err(why) = alike(a, b) {
                let apart = ((a.now.position[0] - b.now.position[0]).powi(2) + (a.now.position[1] - b.now.position[1]).powi(2)).sqrt();
                if apart <= 1. && a.alive && b.alive && leaves(b) {
                    moments[0] += 1;
                } else {
                    return Some(why);
                }
            },
            None if b.human && !b.alive => moments[1] += 1,
            None => return Some(format!("{:?} online only: t{} {} {} at {:?} h{}", key(b), b.team, b.alive, b.human, b.now.position, b.now.health)),
        }
    }
    local.iter().find(|a| !online.iter().any(|b| key(a) == key(b))).map(|a| format!("{:?} local only", key(a)))
}

/// What one tick showed: the local view's fields, notices and bodies.
struct Local {
    fields: BTreeMap<&'static str, String>,
    notices: Vec<(u32, String)>,
    bodies: Vec<Body>,
    systems: Vec<String>,
    lock: Option<([f64; 3], f32)>,
}

/// What the online view showed at one tick of the server, and the tick of
/// the state it was made from.
struct Online {
    at: u32,
    of: u32,
    fields: BTreeMap<&'static str, String>,
    bodies: Vec<Body>,
    systems: Vec<String>,
    roster: bool,
    lock: Option<([f64; 3], f32)>,
}

/// Each critical system as a view shows it.
fn systems(o: &Observation) -> Vec<String> {
    o.systems.iter().map(|s| format!("{} {:?}", s.name, s.state)).collect()
}

/// Both views of one battle, tick by tick, judged at the end: per field, the
/// ticks at which they differed. The server publishes every third tick: each
/// online view is set against the local view of the tick it was published at.
#[derive(Default)]
struct Judge {
    local: BTreeMap<u32, Local>,
    online: Vec<Online>,
}

impl Judge {
    fn see(&mut self, pair: &Pair) {
        let seen = Seen { team: pair.host.player_team("one").unwrap() as i32, own: pair.receiver.roster.local };
        let view = &pair.history.current;
        self.local.insert(view.tick, Local { fields: shared(view, Some(seen)), notices: notices(view, Some(seen)), bodies: view.bodies.clone(),
            systems: systems(view), lock: lock(view) });
        let Some(online) = pair.receiver.mission_view.as_deref() else { return };
        self.online.push(Online { at: pair.host.tick, of: online.tick, fields: shared(online, None), bodies: online.bodies.clone(),
            systems: systems(online), roster: pair.receiver.roster.rows().next().is_some(), lock: lock(online) });
    }

    /// Per field, the ticks at which the views differed, with both values.
    fn differ(&self) -> (BTreeMap<&'static str, Vec<(u32, String, String)>>, u32, [u32; 3]) {
        let mut differ: BTreeMap<&'static str, Vec<(u32, String, String)>> = BTreeMap::new();
        let (mut compared, mut moments) = (0, [0; 3]);
        for online in &self.online {
            let (Some(then), Some(now)) = (self.local.get(&online.of), self.local.get(&online.at)) else { continue };
            let mut local = then.fields.clone();
            // A notice is a message, not state: it comes as soon as it is
            // posted, ahead of the state it is set against, and goes with
            // that state's tick.
            let posted_after = online.of + bfe_sim::wire::mission::NOTICE_TICKS;
            let heard: Vec<_> = then.notices.iter().chain(now.notices.iter().filter(|(until, _)| *until > posted_after)).collect();
            local.insert("notices", format!("{heard:?}"));
            // The roster's first rows come a moment after the match state: the
            // joining player has none for those few ticks, never more than half
            // a second.
            if !online.roster {
                assert!(online.at < 30, "no roster half a second after joining");
                local.remove("roster");
            }
            let next = self.local.get(&(online.of + 1)).map(|next| next.bodies.as_slice());
            if let Some(why) = bodies_differ(&then.bodies, &online.bodies, next, &mut moments) {
                differ.entry("bodies").or_default().push((online.at, why, String::new()));
            }
            // A system's record goes out once every 12 ticks while it changes
            // (`SYSTEM_SCHEMA`'s cadence): each system online is as the local
            // view showed it at most that long before.
            let cadence = (0..=12).filter_map(|back| online.of.checked_sub(back).and_then(|at| self.local.get(&at)));
            let shown: Vec<_> = cadence.collect();
            if online.systems.len() != then.systems.len()
                || online.systems.iter().enumerate().any(|(i, system)| !shown.iter().any(|l| l.systems.get(i) == Some(system))) {
                differ.entry("systems").or_default().push((online.at, format!("{:?}", then.systems), format!("{:?}", online.systems)));
            }
            // His lock drawn within 5 cm of the same place, with the same
            // health to half a point.
            let near = |(a, h): ([f64; 3], f32), (b, g): ([f64; 3], f32)| {
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() <= 0.05 && (h - g).abs() <= 0.5
            };
            let alike = match (then.lock, online.lock) {
                (Some(a), Some(b)) => near(a, b),
                (a, b) => a.is_none() && b.is_none(),
            };
            if !alike {
                differ.entry("lock").or_default().push((online.at, format!("{:?}", then.lock), format!("{:?}", online.lock)));
            }
            for (name, value) in &local {
                if online.fields.get(name) != Some(value) {
                    differ.entry(name).or_default().push((online.at, value.clone(), online.fields.get(name).cloned().unwrap_or_default()));
                }
            }
            compared += 1;
        }
        (differ, compared, moments)
    }

    /// What the local view showed of the three things the wire now carries
    /// for the 2005 HUD: ticks with a hit marker, with a critical one, with a
    /// power lock, the lock values, and kills by an unmanned ship gun.
    fn shown(&self) -> [usize; 5] {
        let marked = |critical: bool| self.local.values().filter(|l| l.fields.get("hit_marker").is_some_and(|m| m.ends_with(&format!(", {critical}))")))).count();
        let locked = self.local.values().filter(|l| l.lock.is_some()).count();
        let targets = self.local.values().filter_map(|l| l.fields.get("locked")).filter(|l| *l != "None").collect::<std::collections::BTreeSet<_>>().len();
        let guns = self.local.values().flat_map(|l| &l.notices)
            .filter(|(_, n)| n.starts_with("kill None") && n.contains(crate::local::mission_view::AUTO_TURRET_KEY)).collect::<std::collections::BTreeSet<_>>().len();
        [marked(false) + marked(true), marked(true), locked, targets, guns]
    }

    /// Prints what differed and fails if anything did, or if the views were
    /// not both there for at least `ticks`.
    fn verdict(&self, ticks: u32) {
        let (differ, compared, moments) = self.differ();
        for (name, rows) in &differ {
            let (tick, local, online) = &rows[0];
            eprintln!("DIFFER {name}: {} ticks, first at {tick}: local {local} | online {online}", rows.len());
            let (tick, local, online) = &rows[rows.len() - 1];
            eprintln!("DIFFER-LAST {name}: at {tick}: local {local} | online {online}");
            eprintln!("DIFFER-TICKS {name}: {:?}", rows.iter().map(|r| r.0).collect::<Vec<_>>());
        }
        // What the run showed, so a pass says what it covered.
        let values = |name: &str| self.local.values().filter_map(|l| l.fields.get(name)).collect::<std::collections::BTreeSet<_>>().len();
        let kills: std::collections::BTreeSet<_> = self.local.values().flat_map(|l| &l.notices).filter(|(_, n)| n.starts_with("kill")).collect();
        let [marked, critical, locked, targets, guns] = self.shown();
        eprintln!("GUNS {guns} kills by an unmanned ship gun, named in both views");
        eprintln!("MARKED {marked} ticks with a hit marker, {critical} with a critical one; LOCKED {locked} ticks on {targets} lock values");
        let lives: std::collections::BTreeSet<_> = self.local.values().flat_map(|l| &l.bodies).map(|b| (b.life.slot, b.life.generation)).collect();
        let systems = self.local.values().map(|l| &l.systems).collect::<std::collections::BTreeSet<_>>().len();
        eprintln!("COVERED {} kills heard, {} lives, {} class room values, {} score values, {} objective values, {systems} system values, {} reinforcement values",
            kills.len(), lives.len(), values("class_room"), values("score"), values("objectives"), values("reinforcements"));
        eprintln!("MOMENTS {} ticks of a soldier stepping into a craft's seat, {} of his own body after his craft was destroyed, {} of a life's first step",
            moments[0], moments[1], moments[2]);
        eprintln!("COMPARED {compared} ticks");
        assert!(moments[0] <= 30 && moments[1] <= 3 && moments[2] <= 30, "only moments, never a state: {moments:?}");
        assert!(compared >= ticks, "both views were there for the run: {compared}");
        assert!(differ.is_empty(), "the views differ in {:?}", differ.keys().collect::<Vec<_>>());
    }
}

/// A fighting player's own state: his look's total, the tick of his last lock
/// press, where he stood a second ago, and the ticks left of a detour round
/// a wall and its side.
#[derive(Default)]
struct Fighter {
    look: [f64; 2],
    pressed: Option<u32>,
    stood: [f64; 3],
    detour: u32,
    side: f64,
}

/// What a fighting player does on one tick: turns toward the nearest living
/// enemy and walks at him while he is far (where a wall stops him, a second
/// and a half at right angles to his way, each time on the other side), taps
/// his trigger five times a second once in range, aimed from the enemy's chest
/// up to his head by turns as a client's fire edge aims, and, holding no power
/// lock, presses for one every second.
fn fight(pair: &Pair, fighter: &mut Fighter, tick: u32) -> Act {
    let codes = bfe_transport::layout::SWBF2_INPUT_CODES;
    let (forward, trigger) = (codes.iter().position(|c| *c == "KeyW").unwrap(), codes.iter().position(|c| *c == "Mouse0").unwrap());
    let lock = bfe_transport::layout::SWBF2_INPUT_ACTIONS.iter().position(|a| *a == "targetLock").unwrap() as u8;
    let own = &pair.receiver.own;
    let team = pair.host.player_team("one").unwrap_or(0.);
    let target = pair.receiver.active().filter(|row| own.alive && row[r::KIND] == f64::from(k::SOLDIER) && row[r::HEALTH] > 0.
            && row[r::TEAM] != 0. && row[r::TEAM] != team)
        .map(|row| {
            let at = [row[r::POSITION], row[r::POSITION + 1], row[r::POSITION + 2]];
            (at, (at[0] - own.position[0]).hypot(at[2] - own.position[2]))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let mut act = Act { look: fighter.look, ..Default::default() };
    let Some((at, distance)) = target else { return act };
    let at = [at[0], at[2]];
    if tick % 60 == 0 {
        let moved = (own.position[0] - fighter.stood[0]).hypot(own.position[2] - fighter.stood[2]);
        if fighter.detour == 0 && distance > 8. && moved < 0.5 {
            fighter.side = if fighter.side > 0. { -1. } else { 1. };
            fighter.detour = 90;
        }
        fighter.stood = own.position;
    }
    let off = if fighter.detour > 0 { fighter.side * std::f64::consts::FRAC_PI_2 } else { 0. };
    fighter.detour = fighter.detour.saturating_sub(1);
    let (dx, dz) = (at[0] - own.position[0], at[1] - own.position[2]);
    let mut turn = dx.atan2(dz) + off - own.yaw;
    while turn > std::f64::consts::PI { turn -= std::f64::consts::TAU; }
    while turn < -std::f64::consts::PI { turn += std::f64::consts::TAU; }
    fighter.look[0] += (-turn * 120.).clamp(-80., 80.);
    act.look = fighter.look;
    if distance > 8. { act.held.push(forward); }
    if distance < 40. && off == 0. && tick % 12 < 2 {
        // A tap: pressed on one tick, released on the next.
        let pressed = tick % 12 == 0;
        // From the chest up to the head by turns: a hit on the head is critical.
        let height = [1., 1.5, 1.7, 1.85][(tick / 12 % 4) as usize];
        let rise = target.map_or(0., |(at, _)| at[1] + height - (own.position[1] + 1.6));
        let mut edge = vec![u8::from(pressed) << 1];
        for value in [pair.receiver.latest, dx.atan2(dz), rise.atan2(distance)] {
            edge.extend_from_slice(&value.to_le_bytes());
        }
        act.edges.push((1, edge));
        if pressed { act.held.push(trigger); }
    }
    if distance < 30. && own.power_lock.is_none() && fighter.pressed.is_none_or(|at| tick >= at + 60) {
        act.edges.push((0, vec![lock]));
        fighter.pressed = Some(tick);
    }
    act
}

/// One player of `query` through the launcher (`launcher` ticks), a life
/// walking forward (`walk`), his own death and the launcher again (`dead`),
/// and a second life fighting the nearest enemy (`fight`), as long as the
/// battle lets him.
fn play_soldier(root: &std::path::Path, query: &str, bare_map: &str, (launcher, walk, dead, fighting): (u32, u32, u32, u32)) -> Judge {
    let mut pair = Pair::start(root, query, bare_map);
    let classes = pair.host.registry_descriptor["classes"].as_array().unwrap().clone();
    let class_id = |name: &str| classes.iter().find(|entry| entry[1].as_str() == Some(name)).unwrap()[0].as_u64().unwrap() as u16;
    let welcome_class = pair.class.clone();
    let forward = bfe_transport::layout::SWBF2_INPUT_CODES.iter().position(|c| *c == "KeyW").unwrap();
    let mut judge = Judge::default();
    for _ in 0..launcher {
        pair.turn(&[], vec![]);
        judge.see(&pair);
    }
    pair.turn(&[], vec![(3, [class_id(&welcome_class).to_le_bytes(), 0u16.to_le_bytes()].concat())]);
    for _ in 0..walk {
        pair.turn(&[forward], vec![]);
        judge.see(&pair);
    }
    pair.turn(&[], vec![(4, vec![])]);
    for _ in 0..dead {
        pair.turn(&[], vec![]);
        judge.see(&pair);
    }
    if fighting > 0 {
        pair.turn(&[], vec![(3, [class_id(&welcome_class).to_le_bytes(), 0u16.to_le_bytes()].concat())]);
        let mut fighter = Fighter::default();
        for tick in 0..fighting {
            let act = fight(&pair, &mut fighter, tick);
            pair.act(&act);
            judge.see(&pair);
        }
    }
    judge
}

/// `ticks` of a battle no one has joined, as a server steps it: from `now` (ms).
fn idle(host: &mut NativeWorldHost, now: &mut f64, ticks: u32) {
    for _ in 0..ticks {
        *now += 1000. / 60.;
        host.links.clear();
        let publication = host.step(*now, (1_700_000_000_000. + *now) as _).unwrap();
        assert!(publication.is_none_or(|p| p.faults.is_empty()));
    }
}

// Two battles in rotation, as a dedicated server plays them: one with its
// player, then the next, admitted when the announced delay is over and
// ticking alone while he probes it and sits through his loading screen; its
// load announces its opening hint, whose 5 s are over before he can arrive.
// He joins it as a client joins every battle, with a receiver of its own (the
// first battle's refuses another battle's registry: a mission never outlives
// its battle), and is shown the second battle's opening hint from his
// arrival, for its span. The gap here is 6 s: past the hint's 5 s (a real
// one is longer, the loading screen alone is 13.5 s) and before this fixture
// battle's own end (its winch reaches the score limit at 7.5 s).
#[test]
fn a_player_rotated_to_the_next_battle_is_shown_its_opening() {
    let dir = std::env::temp_dir().join(format!("bfe-pair-rotation-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = common::minimal_content_dir(&dir);
    common::invented_mission(&GameFamily::Swbf2.content_dir(&root), &["cw"]);
    let query = "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli";
    let hint = |pair: &Pair| pair.receiver.mission_view.as_deref()
        .is_some_and(|o| notices(o, None).iter().any(|(_, n)| n.starts_with("message") && n.contains("belfry.hint")));
    let mut first = Pair::start(&root, query, "yard");
    let mut shown = 0;
    for _ in 0..120 {
        first.turn(&[], vec![]);
        shown += u32::from(hint(&first));
    }
    assert!(shown > 100, "battle one's opening hint: {shown}");
    let mut second = NativeWorldHost::load_battle(&root, GameFamily::Swbf2, query, 1, OPEN).unwrap();
    let mut now = first.now;
    idle(&mut second, &mut now, 6 * 60);
    let receiver = &mut first.receiver;
    receiver.message(true, &serde_json::to_vec(&second.registry_message()).unwrap(), now);
    assert_eq!(receiver.error().as_deref(), Some("duplicate entity registry"));
    let mut pair = Pair::join(second, &root, query, "yard", now);
    let arrived = pair.host.tick;
    let mut shown = Vec::new();
    for _ in 0..bfe_sim::wire::mission::NOTICE_TICKS + 60 {
        pair.turn(&[], vec![]);
        if hint(&pair) { shown.push(pair.host.tick - arrived); }
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("ROTATION arrived at tick {arrived}; the opening hint shown {} ticks, from {:?} to {:?} after", shown.len(), shown.first(), shown.last());
    assert!(arrived > bfe_sim::wire::mission::NOTICE_TICKS + 1, "he arrived after the hint's first span: {arrived}");
    assert!(shown.first().is_some_and(|at| *at <= 6), "shown from his arrival: {shown:?}");
    assert!((bfe_sim::wire::mission::NOTICE_TICKS - 10..=bfe_sim::wire::mission::NOTICE_TICKS + 3).contains(&(shown.len() as u32)),
        "for its span: {}", shown.len());
}

#[test]
fn an_online_client_observes_what_the_local_world_shows_in_the_launcher_alive_and_dead() {
    let dir = std::env::temp_dir().join(format!("bfe-pair-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = common::minimal_content_dir(&dir);
    common::invented_mission(&GameFamily::Swbf2.content_dir(&root), &["cw"]);
    let judge = play_soldier(&root, "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli", "yard", (60, 240, 120, 0));
    let _ = std::fs::remove_dir_all(&dir);
    judge.verdict(300);
}

/// The prepared 2005 content's root (`BFE_SWBF2_PREPARED` names its `swbf2/.content`).
fn prepared_root() -> std::path::PathBuf {
    let prepared = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").expect("BFE_SWBF2_PREPARED"));
    prepared.parent().and_then(std::path::Path::parent).expect("BFE_SWBF2_PREPARED names <root>/swbf2/.content").to_path_buf()
}

// Mos Eisley's hero battle with its bots: a hero in the launcher, alive and
// dead and fighting (his hits marked, critical or not, and his power lock),
// every field of his online view equal to the local view of the same server
// world.
#[test]
#[ignore = "requires BFE_SWBF2_PREPARED (a folder's prepared swbf2/.content)"]
fn an_online_hero_observes_what_the_local_world_shows_in_mos_eisley() {
    let judge = play_soldier(&prepared_root(), "?map=swbf2/tat2&side=imp&mode=eli&seed=1", "tat2", (120, 600, 300, 3600));
    judge.verdict(4400);
    // How much of the fight reached the hit marker and the lock depends on
    // where the bots fight: said, not required (the fields are compared on
    // every tick either way).
    let [marked, _, locked, ..] = judge.shown();
    eprintln!("FOUGHT {marked} ticks with a hit marker, {locked} with a power lock");
}

/// The craft the player sits in, from his own Space body: (craft, seat).
fn own_craft(pair: &Pair) -> Option<(u16, u8)> {
    use bfe_sim::wire::space::OwnSpaceBody;
    match pair.receiver.space_online.as_deref()?.own_body.as_ref()? {
        OwnSpaceBody::Pilot(own) => Some((own.fighter_id, own.seat)),
        OwnSpaceBody::Chair(_) => None,
    }
}

/// Whether the server's world has the player's craft outside the battlefield.
fn craft_outside_on_server(pair: &Pair, craft: u16) -> bool {
    pair.host.context.owned(|w| w.space().and_then(|space| space.fighters.iter().find(|f| f.id == usize::from(craft)))
        .is_some_and(|f| f.oob_timer >= 0.)).unwrap()
}

/// One player of Space Assault through the launcher, deployed on his hangar
/// deck, walking to the nearest free landed craft of his team and boarding
/// it, a while in its gunner seat on the deck if it has one, back in the
/// pilot's seat, taking off and flying straight on until the battlefield's
/// edge warns him and its countdown ends his life (shot down first, he
/// deploys and flies again, three lives at most), and the launcher again. Returns the judge and,
/// per tick he sat in a craft, whether his online view showed him leaving and
/// whether the server had his craft outside.
fn play_space(root: &std::path::Path) -> (Judge, Vec<(u32, bool, bool)>, [u32; 4]) {
    use bfe_sim::space::flight::SpacePilotInput;
    use bfe_sim::space::local::PilotControls;
    let mut pair = Pair::start(root, "?map=swbf2/spa6&side=rep&mode=ass&seed=41", "spa6");
    let classes = pair.host.registry_descriptor["classes"].as_array().unwrap().clone();
    let class_id = classes.iter().find(|entry| entry[1].as_str() == Some(pair.class.as_str())).unwrap()[0].as_u64().unwrap() as u16;
    let codes = bfe_transport::layout::SWBF2_INPUT_CODES;
    let (forward, sprint) = (codes.iter().position(|c| *c == "KeyW").unwrap(), codes.iter().position(|c| *c == "ShiftLeft").unwrap());
    let mut judge = Judge::default();
    let mut leaving = Vec::new();
    // Ticks spent: in the launcher, on foot, as a gunner, as a pilot.
    let mut spent = [0u32; 4];
    let controls = |pilot: PilotControls| Act { pilot: Some(pilot), ..Default::default() };
    type Leaving = Vec<(u32, bool, bool)>;
    fn step(pair: &mut Pair, judge: &mut Judge, act: &Act, leaving: &mut Leaving, spent: &mut [u32; 4]) {
        pair.act(act);
        judge.see(pair);
        let seat = own_craft(pair);
        spent[match seat { None if pair.receiver.own.alive => 1, None => 0, Some((_, 0)) => 3, Some(_) => 2 }] += 1;
        if let (Some((craft, _)), Some(view)) = (seat, pair.receiver.mission_view.as_deref()) {
            leaving.push((pair.host.tick, view.leaving, craft_outside_on_server(pair, craft)));
        }
    }
    // To the nearest free landed craft of his team on his own deck, and aboard:
    // its seats, or None if he could not board it. (A two-seat craft further
    // along the Republic deck could not be boarded by walking up to it and
    // pressing use.) `look` is his look's total, as his client sends it.
    #[allow(clippy::too_many_arguments)]
    fn board(pair: &mut Pair, judge: &mut Judge, leaving: &mut Leaving, spent: &mut [u32; 4], look: &mut [f64; 2], (forward, sprint): (usize, usize)) -> Option<u8> {
        let team = pair.host.player_team("one").unwrap() as i32;
        let start = pair.receiver.own.position;
        let (craft, seats) = pair.host.context.owned(|w| {
            let space = w.space().unwrap();
            let free = |f: &&bfe_sim::space::flight::SpaceFighter| f.alive && f.team == team && f.pilot.is_none()
                && f.state == bfe_sim::space::flight::SpaceFlightState::Landed && (f.position[1] - start[1]).abs() < 4.;
            let near = |a: &&bfe_sim::space::flight::SpaceFighter, b: &&bfe_sim::space::flight::SpaceFighter| {
                let da = (a.position[0] - start[0]).powi(2) + (a.position[2] - start[2]).powi(2);
                let db = (b.position[0] - start[0]).powi(2) + (b.position[2] - start[2]).powi(2);
                da.total_cmp(&db)
            };
            space.fighters.iter().filter(free).min_by(near).map(|f| (f.position, space.profiles[f.profile_index].seat_count))
        }).unwrap()?;
        for tick in 0..600u32 {
            if own_craft(pair).is_some() { return Some(seats); }
            let (dx, dz) = (craft[0] - pair.receiver.own.position[0], craft[2] - pair.receiver.own.position[2]);
            let distance = (dx * dx + dz * dz).sqrt();
            let mut turn = dx.atan2(dz) - pair.receiver.own.yaw;
            while turn > std::f64::consts::PI { turn -= std::f64::consts::TAU; }
            while turn < -std::f64::consts::PI { turn += std::f64::consts::TAU; }
            look[0] += (-turn * 120.).clamp(-80., 80.);
            let held = if distance > 5. { vec![forward, sprint] } else if distance > 1. { vec![forward] } else { vec![] };
            let pilot = bfe_sim::space::local::PilotControls { use_pressed: distance < 12. && tick % 6 == 0, ..Default::default() };
            step(pair, judge, &Act { held, look: *look, pilot: Some(pilot), ..Default::default() }, leaving, spent);
        }
        own_craft(pair).map(|_| seats)
    }
    for _ in 0..120 {
        step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
    }
    let deploy = Act { edges: vec![(3, [class_id.to_le_bytes(), 0u16.to_le_bytes()].concat())], pilot: Some(PilotControls::default()), ..Default::default() };
    step(&mut pair, &mut judge, &deploy, &mut leaving, &mut spent);
    for _ in 0..600 {
        if pair.receiver.own.alive { break; }
        step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
    }
    assert!(pair.receiver.own.alive, "he deploys on his hangar deck");
    let mut look = [0.; 2];
    let seats = board(&mut pair, &mut judge, &mut leaving, &mut spent, &mut look, (forward, sprint));
    assert!(seats.is_some(), "he boards the nearest craft: he is at {:?}", pair.receiver.own.position);
    let seats = seats.unwrap_or(1);
    // The seat keys move him to the gunner's seat and back, on the deck.
    if seats > 1 {
        step(&mut pair, &mut judge, &controls(PilotControls { cycle_weapon: 1, ..Default::default() }), &mut leaving, &mut spent);
        for _ in 0..300 {
            step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
        }
    }
    for _ in 0..6 {
        if own_craft(&pair).is_some_and(|(_, seat)| seat == 0) { break; }
        step(&mut pair, &mut judge, &controls(PilotControls { cycle_weapon: -1, ..Default::default() }), &mut leaving, &mut spent);
        for _ in 0..10 {
            step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
        }
    }
    // Off the deck and straight on until the edge ends his life. Shot down
    // before he gets there, he deploys, boards and flies again (three lives).
    let fly = |takeoff_or_land: bool| PilotControls { flight: SpacePilotInput { takeoff_or_land, throttle: 1., ..Default::default() }, ..Default::default() };
    for life in 0..3 {
        if life > 0 {
            for _ in 0..240 {
                step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
            }
            step(&mut pair, &mut judge, &deploy, &mut leaving, &mut spent);
            for _ in 0..600 {
                if pair.receiver.own.alive { break; }
                step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
            }
            if board(&mut pair, &mut judge, &mut leaving, &mut spent, &mut look, (forward, sprint)).is_none() { continue; }
        }
        step(&mut pair, &mut judge, &controls(fly(true)), &mut leaving, &mut spent);
        for _ in 0..7200 {
            if !pair.receiver.own.alive { break; }
            step(&mut pair, &mut judge, &controls(fly(false)), &mut leaving, &mut spent);
        }
        if leaving.iter().any(|(_, _, server)| *server) { break; }
    }
    for _ in 0..300 {
        step(&mut pair, &mut judge, &controls(PilotControls::default()), &mut leaving, &mut spent);
    }
    (judge, leaving, spent)
}

// Space Assault over Mygeeto with its bots: in the launcher, on foot, as a
// gunner and as a pilot, alive and dead, every field of his online view equal
// to the local view of the same server world; and his leaving warning shown
// exactly while the server has his craft outside the battlefield.
#[test]
#[ignore = "requires BFE_SWBF2_PREPARED (a folder's prepared swbf2/.content)"]
fn an_online_pilot_and_gunner_observe_what_the_local_world_shows_in_space() {
    let (judge, leaving, spent) = play_space(&prepared_root());
    let shown = leaving.iter().filter(|(_, online, _)| *online).count();
    let outside = leaving.iter().filter(|(_, _, server)| *server).count();
    // His own correction is a publication's: it may come one publication
    // after the server's craft crossed the edge.
    let late: Vec<_> = leaving.iter().filter(|(_, online, server)| online != server).map(|(tick, ..)| *tick).collect();
    eprintln!("SPACE ticks: launcher {} on foot {} gunner {} pilot {}; leaving shown {shown}, craft outside {outside}, differing at {late:?}",
        spent[0], spent[1], spent[2], spent[3]);
    judge.verdict(1000);
    assert!(spent[3] > 200, "he was a pilot: {spent:?}");
    assert!(outside > 0, "his craft reached the edge");
    assert!(late.len() <= 6, "his leaving warning follows his craft's: {late:?}");
}
