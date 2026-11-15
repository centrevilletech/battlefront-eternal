//! A once-in-a-while account of the battle's bots, for comparing hosts with
//! each other and with the browser build (BFE_AI_TRACE). Every ten seconds a
//! line: per team the bots alive, those that moved more than a metre in the
//! last second and those seated, the team's kills and deaths, its
//! reinforcements and posts, and the shots and damage heard since. Every
//! minute a behaviour summary since the battle began: bot shots by weapon
//! (on foot and mounted), grenades, repair and melee swings, boardings, long
//! hits, time spent standing, crouched and prone, hits on and by players, the
//! first capture, and bots standing still. A host calls `sample` after every
//! tick; it reads the world once a second.
use crate::abi::context::{Context, Error};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

const TEAMS: usize = 8;

#[derive(Default)]
pub struct Census {
    /// Each unit's horizontal position and life a second ago, by store slot.
    last: HashMap<u32, (f64, f64, f64)>,
    journal_seen: usize,
    /// Shots and damage since the last ten-second line.
    shots: u32,
    damage: u32,
    behaviour: Behaviour,
    deka: Deka,
    classes: Classes,
    /// The battle's end has been reported.
    ended: bool,
}

/// Droideka bots counted every tick: ticks alive, folding/rolled/unfolding,
/// rolled ball, shield up, folds begun, faster than 6 m/s; lives seen.
#[derive(Default)]
struct Deka {
    alive: u32,
    rolled: u32,
    ball: u32,
    shield: u32,
    folds: u32,
    fast: u32,
    mode: HashMap<u32, (f64, f64)>,
}

/// Bot soldiers by class, counted every tick: ticks alive on foot, ticks
/// jetting, jet and jump starts; and each one's air state last tick.
#[derive(Default)]
struct Classes {
    alive: BTreeMap<u32, u32>,
    jet: BTreeMap<u32, u32>,
    jets: BTreeMap<u32, u32>,
    jumps: BTreeMap<u32, u32>,
    air: HashMap<u32, (f64, u8)>,
    /// Hits on soldiers and on hulls or props, by the attacker's class.
    soldier_hits: BTreeMap<u32, u32>,
    object_hits: BTreeMap<u32, u32>,
}

#[derive(Default)]
struct Behaviour {
    shots: BTreeMap<u32, u32>,
    mounted: BTreeMap<u32, u32>,
    grenades: BTreeMap<u32, u32>,
    repair: u32,
    melee: u32,
    boards: u32,
    unboards: u32,
    hits: u32,
    hits_50m: u32,
    /// Of `hits`, those on hulls and props.
    object_hits: u32,
    /// Bot-seconds manned in turrets and in vehicles.
    turret_seconds: u32,
    vehicle_seconds: u32,
    hits_100m: u32,
    hits_on_players: u32,
    hits_by_players: u32,
    /// Bot shots on foot and bot hits on soldiers, by the shooter's team.
    team_shots: [u32; TEAMS],
    /// Damage bots dealt to soldiers, and their hits that dealt none.
    damage_sum: f64,
    /// Bot hits on soldiers by the attacker's class.
    class_hits: BTreeMap<u32, u32>,
    class_shots: BTreeMap<u32, u32>,
    /// Explosions, and the soldier damage records just before each (its
    /// blast's victims, which the world journals ahead of the explosion).
    /// Health living soldiers gained second to second (healing), and each
    /// one's health and life a second ago.
    healed: f64,
    /// Bot hits on soldiers by damage: under 10, 10-30, 30-60, 60-120, more.
    amounts: [u32; 5],
    health: HashMap<u32, (f64, f64)>,
    explosions: u32,
    blast_victims: u32,
    run: u32,
    zero_hits: u32,
    team_hits: [u32; TEAMS],
    /// Alive bot soldiers' seconds standing, crouched and prone.
    posture: [u32; 3],
    /// The posts' teams at the start, and the second of the first change.
    posts: Option<Vec<f64>>,
    first_capture: Option<u64>,
    /// Seconds each bot has stood within a metre, by store slot and life.
    still: HashMap<u32, (f64, u32)>,
    /// Bots that stood more than ten seconds on foot, once a life, by class.
    stood: HashMap<(u32, u64), u32>,
    /// Seconds each bot has pressed to move on foot without covering a metre.
    pressing: HashMap<u32, (f64, u32)>,
    /// Bots stuck five seconds or more that way, once a life, by class.
    stuck: HashMap<(u32, u64), u32>,
    /// Where each bot's life began, the second it began and whether it has
    /// gone 10 m from there; lives over 30 s that never did, by class.
    anchor: HashMap<u32, (f64, f64, f64, u64, bool, u32)>,
    never_left: BTreeMap<u32, u32>,
    /// Post-seconds where a post counted more of a team inside its region
    /// than living soldiers on foot of that team stand there.
    ghost_seconds: u32,
    /// Deaths inside a capture region (alive a second ago, dead now), and
    /// seconds bodies (dead or removed soldiers) lay inside one.
    deaths_in_regions: u32,
    body_seconds_in_regions: u32,
    removed_seconds_in_regions: u32,
    /// Corpses (dead, not yet removed) by how many seconds they have lain.
    corpse_age: HashMap<u32, u32>,
    corpse_max: u32,
    corpses_over_5s: u32,
    was_alive: HashSet<u32>,
    /// Deaths by the victim's team and cause (no attacker, himself, a
    /// teammate, an enemy; mounted when the attacker was seated).
    causes: BTreeMap<String, u32>,
    /// Bot spawns by team and post name.
    spawns: BTreeMap<(u32, u32), u32>,
    /// Characters whose shown kills (kills - 2 x team kills) are below zero.
    negative: Vec<String>,
}

#[derive(Clone, Copy, Default)]
struct Team {
    present: bool,
    alive: u32,
    moving: u32,
    seated: u32,
    kills: f64,
    team_kills: f64,
    deaths: f64,
    posts: u32,
}

impl Census {
    /// Reads the world once a second; answers a line every `every` ticks
    /// (and the behaviour summary every minute), else None.
    pub fn sample(&mut self, context: &Context, tick: u64, every: u64) -> Result<Option<String>, Error> {
        let deka = &mut self.deka;
        let cl = &mut self.classes;
        context.owned(|w| {
            for (r, unit) in w.sim.store.unit.iter() {
                if unit.human || !unit.alive || unit.dead { continue; }
                if let crate::gen::UnitBody::Soldier(s) = &unit.soldier {
                    if unit.seat.is_some() { continue; }
                    let class = unit.class_name.0;
                    *cl.alive.entry(class).or_default() += 1;
                    let air = match s.air { Some(crate::gen::Air::Jet) => 2u8, Some(crate::gen::Air::Jump) => 1, _ => 0 };
                    if air == 2 { *cl.jet.entry(class).or_default() += 1; }
                    let was = cl.air.insert(r.slot, (unit.generation, air)).filter(|(g, _)| *g == unit.generation).map_or(0, |(_, a)| a);
                    if air == 2 && was != 2 { *cl.jets.entry(class).or_default() += 1; }
                    if air == 1 && was != 1 { *cl.jumps.entry(class).or_default() += 1; }
                    continue;
                }
                let crate::gen::UnitBody::Droideka(d) = &unit.soldier else { continue };
                deka.alive += 1;
                deka.rolled += u32::from(d.mode >= 11.0);
                deka.ball += u32::from(d.mode == 12.0);
                deka.shield += u32::from(d.shield.as_ref().is_some_and(|s| s.on));
                deka.fast += u32::from(d.velocity[0].hypot(d.velocity[2]) > 6.0);
                let was = deka.mode.insert(r.slot, (unit.generation, d.mode));
                deka.folds += u32::from(d.mode == 11.0 && was.is_some_and(|(g, m)| g == unit.generation && m != 11.0));
            }
        })?;
        if tick % 60 != 0 {
            return Ok(None);
        }
        let second = tick / 60;
        let mut report = tick % every.max(60) == 0;
        let last = std::mem::take(&mut self.last);
        let behaviour = &mut self.behaviour;
        let mut over = false;
        let mut seated_slots = HashSet::new();
        let (teams, reinforcements, humans, classes) = context.owned(|w| {
            over = w.sim.store.battle.game_over;
            let store = &w.sim.store;
            let mut teams = [Team::default(); TEAMS];
            let mut humans = HashSet::new();
            let mut classes: HashMap<u32, u32> = HashMap::new();
            let mut next = HashMap::with_capacity(last.len());
            for (r, unit) in store.unit.iter() {
                let p = unit.position();
                next.insert(r.slot, (p.x, p.z, unit.generation));
                if unit.human {
                    humans.insert(r.slot);
                }
                classes.insert(r.slot, unit.class_name.0);
                if unit.human || !(0.0..TEAMS as f64).contains(&unit.team) {
                    continue;
                }
                let t = &mut teams[unit.team as usize];
                t.present = true;
                if unit.dead && !unit.removed {
                    let age = behaviour.corpse_age.entry(r.slot).or_default();
                    *age += 1;
                    behaviour.corpse_max = behaviour.corpse_max.max(*age);
                    if *age > 5 { behaviour.corpses_over_5s += 1; }
                } else {
                    behaviour.corpse_age.remove(&r.slot);
                }
                if !unit.alive || unit.dead {
                    behaviour.still.remove(&r.slot);
                    behaviour.pressing.remove(&r.slot);
                    if let Some((_, _, _, born, left, class)) = behaviour.anchor.remove(&r.slot) {
                        if !left && second >= born + 30 { *behaviour.never_left.entry(class).or_default() += 1; }
                    }
                    continue;
                }
                t.alive += 1;
                let hp = unit.combat.health;
                if let Some(&(was, life)) = behaviour.health.get(&r.slot) {
                    if life == unit.generation && hp > was { behaviour.healed += hp - was; }
                }
                behaviour.health.insert(r.slot, (hp, unit.generation));
                let seated = unit.seat.is_some();
                t.seated += u32::from(seated);
                if let Some(lease) = unit.seat.and_then(|l| store.seat_lease.get(l)) {
                    if lease.turret.is_some() { behaviour.turret_seconds += 1; } else if lease.vehicle.is_some() { behaviour.vehicle_seconds += 1; }
                }
                let mut moved = false;
                if let Some(&(x, z, life)) = last.get(&r.slot) {
                    moved = life != unit.generation || (p.x - x).hypot(p.z - z) > 1.0;
                }
                t.moving += u32::from(moved);
                if let crate::gen::UnitBody::Soldier(s) = &unit.soldier {
                    if !seated {
                        behaviour.posture[s.posture as usize] += 1;
                    }
                }
                let anchor = behaviour.anchor.entry(r.slot).or_insert((unit.generation, p.x, p.z, second, false, unit.class_name.0));
                if anchor.0 != unit.generation {
                    *anchor = (unit.generation, p.x, p.z, second, false, unit.class_name.0);
                } else if (p.x - anchor.1).hypot(p.z - anchor.2) > 10.0 {
                    anchor.4 = true;
                }
                let pushing = !seated && unit.intent.forward.abs() + unit.intent.right.abs() > 0.3;
                let press = behaviour.pressing.entry(r.slot).or_insert((unit.generation, 0));
                let covered = last.get(&r.slot).is_some_and(|&(x, z, life)| life == unit.generation && (p.x - x).hypot(p.z - z) > 1.0);
                if press.0 != unit.generation || !pushing || covered {
                    *press = (unit.generation, 0);
                } else {
                    press.1 += 1;
                    if press.1 >= 5 {
                        behaviour.stuck.insert((r.slot, unit.generation as u64), unit.class_name.0);
                    }
                }
                let still = behaviour.still.entry(r.slot).or_insert((unit.generation, 0));
                if still.0 != unit.generation || moved || seated {
                    *still = (unit.generation, 0);
                } else {
                    still.1 += 1;
                    if still.1 > 10 {
                        behaviour.stood.insert((r.slot, unit.generation as u64), unit.class_name.0);
                    }
                }
            }
            for (_, c) in store.character.iter() {
                if (0.0..TEAMS as f64).contains(&c.team) {
                    let t = &mut teams[c.team as usize];
                    t.kills += c.kills;
                    t.deaths += c.deaths;
                    t.team_kills += c.team_kills;
                }
            }
            // A post's counts against the living soldiers on foot inside its
            // region: any surplus is someone counted who should not be.
            for (_, post) in store.post.iter().filter(|(_, p)| p.capture.is_some()) {
                let mut living = [0.0f64; TEAMS];
                let mut inside = String::new();
                for (r, unit) in store.unit.iter() {
                    let at = unit.position();
                    if !crate::battle::posts::contains(post, &at) { continue; }
                    if unit.removed { behaviour.removed_seconds_in_regions += 1; }
                    if (!unit.alive || unit.dead) && !unit.removed {
                        behaviour.body_seconds_in_regions += 1;
                        if behaviour.was_alive.contains(&r.slot) { behaviour.deaths_in_regions += 1; }
                    }
                    let live = unit.alive && !unit.dead && !unit.combat.dead && !unit.removed && unit.combat.health > 0.0 && unit.seat.is_none();
                    if live && (0.0..TEAMS as f64).contains(&unit.team) { living[unit.team as usize] += 1.0; }
                    let _ = write!(inside, " {}:t{}:{}{}{}{}{}:h{}", r.slot, unit.team, if unit.alive {"A"} else {"a"}, if unit.dead {"D"} else {"-"},
                        if unit.combat.dead {"C"} else {"-"}, if unit.removed {"R"} else {"-"}, if unit.seat.is_some() {"S"} else {"-"}, unit.combat.health.round());
                }
                for (team, &n) in post.counts.iter() {
                    if (1.0..3.0).contains(&team) && n > living[team as usize] {
                        behaviour.ghost_seconds += 1;
                        eprintln!("ai capture ghost t={second} post={} team={team} counted={n} living={} progress={:.3} neutralize={:.3} owner={} inside:{inside}",
                            String::from_utf16_lossy(w.sim.strings.units(post.name)), living[team as usize], post.progress, post.neutralize, post.team);
                    }
                }
            }
            behaviour.was_alive = store.unit.iter().filter(|(_, u)| u.alive && !u.dead).map(|(r, _)| r.slot).collect();
            let mut posts = Vec::new();
            for (_, post) in store.post.iter() {
                posts.push(post.team);
                if (0.0..TEAMS as f64).contains(&post.team) {
                    teams[post.team as usize].posts += 1;
                }
            }
            match &behaviour.posts {
                None => behaviour.posts = Some(posts),
                Some(start) if behaviour.first_capture.is_none() && *start != posts => behaviour.first_capture = Some(second),
                _ => {}
            }
            let reinforcements: Vec<(f64, f64)> = store.battle.reinforcements.iter().filter(|(t, _)| *t < 3.0).map(|(t, v)| (t, *v)).collect();
            self.last = next;
            seated_slots = store.unit.iter().filter(|(_, u)| u.seat.is_some()).map(|(r, _)| r.slot).collect();
            behaviour.negative = store.character.iter().filter(|(_, c)| c.kills - 2.0 * c.team_kills < 0.0)
                .map(|(_, c)| format!("{}:{}/{}", c.team, c.kills, c.team_kills)).collect();
            (teams, reinforcements, humans, classes)
        })?;
        let seen = self.journal_seen;
        let positions = &self.last;
        let (shots, damage) = (&mut self.shots, &mut self.damage);
        let (soldier_hits, object_hits) = (&mut self.classes.soldier_hits, &mut self.classes.object_hits);
        let head = context.world_host_journal(|journal| {
            use super::journal::Event;
            let head = journal.seq;
            let start = seen.max(head.saturating_sub(super::journal::CAPACITY));
            let human = |life: &Option<super::journal::Life>| life.is_some_and(|l| humans.contains(&l.id));
            for record in journal.records().filter(|r| r.seq >= start && r.seq < head) {
                match &record.event {
                    Event::Damage { .. } | Event::HullDamage { .. } | Event::PropDamage { .. } => behaviour.run += 1,
                    Event::Explosion { .. } => { behaviour.explosions += 1; behaviour.blast_victims += behaviour.run; behaviour.run = 0; }
                    _ => behaviour.run = 0,
                }
                match &record.event {
                    Event::Shot { source, team, weapon, repair, melee, .. } => {
                        *shots += 1;
                        if humans.contains(&source.id) {
                            continue;
                        }
                        if !*repair && (0.0..TEAMS as f64).contains(team) { behaviour.team_shots[*team as usize] += 1; }
                        if !*repair { if let Some(&class) = classes.get(&source.id) { *behaviour.class_shots.entry(class).or_default() += 1; } }
                        *behaviour.shots.entry(weapon.map_or(0, |w| w.0)).or_default() += 1;
                        behaviour.repair += u32::from(*repair);
                        behaviour.melee += u32::from(*melee);
                    }
                    Event::MountedShot { source, weapon, repair, .. } => {
                        *shots += 1;
                        if human(source) {
                            continue;
                        }
                        *behaviour.mounted.entry(weapon.map_or(0, |w| w.0)).or_default() += 1;
                        behaviour.repair += u32::from(*repair);
                    }
                    Event::Grenade { source, def, .. } if !human(source) => {
                        *behaviour.grenades.entry(def.map_or(0, |d| d.0)).or_default() += 1;
                    }
                    Event::Death { target, team, attacker, attacker_team, .. } => {
                        let how = match attacker {
                            None => "none",
                            Some(a) if a.id == target.id => "self",
                            Some(_) if attacker_team == team => "friend",
                            Some(_) => "enemy",
                        };
                        let by = if attacker.is_some_and(|a| seated_slots.contains(&a.id)) { ":mounted" } else { "" };
                        *behaviour.causes.entry(format!("t{team}:{how}{by}")).or_default() += 1;
                    }
                    Event::Spawn { target, team, post, .. } if !humans.contains(&target.id) => {
                        *behaviour.spawns.entry((*team as u32, post.map_or(0, |p| p.0))).or_default() += 1;
                    }
                    Event::Seat { boarded, source, .. } if !human(source) => {
                        if *boarded { behaviour.boards += 1 } else { behaviour.unboards += 1 }
                    }
                    Event::Damage { target, position, attacker, attacker_team, amount, .. } => {
                        *damage += 1;
                        if humans.contains(&target.id) {
                            behaviour.hits_on_players += 1;
                        } else if human(attacker) {
                            behaviour.hits_by_players += 1;
                        } else {
                            behaviour.hits += 1;
                            if amount.is_finite() { behaviour.damage_sum += amount; }
                            behaviour.amounts[[10.0, 30.0, 60.0, 120.0].iter().take_while(|&&edge| *amount >= edge).count()] += 1;
                            behaviour.zero_hits += u32::from(!(*amount > 0.0));
                            if (0.0..TEAMS as f64).contains(attacker_team) { behaviour.team_hits[*attacker_team as usize] += 1; }
                            if let Some(&class) = attacker.and_then(|a| classes.get(&a.id)) { *behaviour.class_hits.entry(class).or_default() += 1; *soldier_hits.entry(class).or_default() += 1; }
                            // The attacker where the census last saw him (within a second).
                            if let Some(&(x, z, _)) = attacker.and_then(|a| positions.get(&a.id)) {
                                let d = ((position[0] - x).powi(2) + (position[2] - z).powi(2)).sqrt();
                                behaviour.hits_50m += u32::from(d > 50.0);
                                behaviour.hits_100m += u32::from(d > 100.0);
                            }
                        }
                    }
                    // Hits on hulls and props count as hits too, as the page's
                    // journal names every one of them a `damage` record.
                    Event::HullDamage { attacker, attacker_team, .. } | Event::PropDamage { attacker, attacker_team, .. } => {
                        *damage += 1;
                        if !human(attacker) {
                            behaviour.hits += 1;
                            behaviour.object_hits += 1;
                            if (0.0..TEAMS as f64).contains(attacker_team) { behaviour.team_hits[*attacker_team as usize] += 1; }
                            if let Some(&class) = attacker.and_then(|a| classes.get(&a.id)) { *behaviour.class_hits.entry(class).or_default() += 1; *object_hits.entry(class).or_default() += 1; }
                        }
                    }
                    _ => {}
                }
            }
            head
        })?;
        self.journal_seen = head;
        let end = over && !self.ended;
        self.ended |= over;
        report |= end;
        let mut out = String::new();
        if report {
            let _ = write!(out, "t={}s", second);
            for (index, t) in teams.iter().enumerate().filter(|(_, t)| t.present) {
                let _ = write!(out, " t{index}:{}/{}/{} kd{}-{} tk{}", t.alive, t.moving, t.seated, t.kills, t.deaths, t.team_kills);
            }
            let _ = write!(out, " reinf=");
            for (team, count) in &reinforcements {
                let _ = write!(out, "{team}:{count},");
            }
            let _ = write!(out, " posts=");
            for (index, t) in teams.iter().enumerate().filter(|(_, t)| t.posts > 0) {
                let _ = write!(out, "{index}:{},", t.posts);
            }
            let _ = write!(out, " shots={} dmg={}{}", self.shots, self.damage, if over { " over" } else { "" });
            let k = &self.deka;
            let sec = |n: u32| f64::from(n) / 60.0;
            let _ = write!(out, " deka alive={:.1} rolledAny={:.1} ball={:.1} shield={:.1} folds={} fast={:.1}",
                sec(k.alive), sec(k.rolled), sec(k.ball), sec(k.shield), k.folds, sec(k.fast));
            self.shots = 0;
            self.damage = 0;
        }
        if (second % 60 == 0 && second > 0) || end {
            let summary = self.summary(context, second)?;
            let _ = write!(out, "{}behaviour {summary}", if out.is_empty() { "" } else { "\n" });
        }
        Ok((!out.is_empty()).then_some(out))
    }

    /// The behaviour since the battle began, as one JSON object.
    fn summary(&self, context: &Context, second: u64) -> Result<String, Error> {
        let b = &self.behaviour;
        let cl = &self.classes;
        context.owned(|w| {
            let name = |id: &u32| -> String {
                w.sim.strings.list.get(*id as usize).map(|v| String::from_utf16_lossy(v)).unwrap_or_default()
            };
            let map = |m: &BTreeMap<u32, u32>| -> serde_json::Value {
                serde_json::Value::Object(m.iter().map(|(k, v)| (if *k == 0 { "?".into() } else { name(k) }, (*v).into())).collect())
            };
            let mut v = serde_json::json!({
                "t": second, "shots": map(&b.shots), "mounted": map(&b.mounted), "grenades": map(&b.grenades),
                "repair": b.repair, "melee": b.melee, "boards": b.boards, "unboards": b.unboards,
                "hits": b.hits, "objectHits": b.object_hits, "turretSeconds": b.turret_seconds, "vehicleSeconds": b.vehicle_seconds, "hits50m": b.hits_50m, "hits100m": b.hits_100m,
                "hitsOnPlayers": b.hits_on_players, "hitsByPlayers": b.hits_by_players,
                "teamShots": b.team_shots[..3], "teamHits": b.team_hits[..3],
                "damageSum": b.damage_sum.round(), "zeroHits": b.zero_hits,
                "classHits": map(&b.class_hits), "classShots": map(&b.class_shots),
                "explosions": b.explosions, "blastVictims": b.blast_victims, "healed": b.healed.round(), "hitAmounts": b.amounts,
                "postureSeconds": {"stand": b.posture[0], "crouch": b.posture[1], "prone": b.posture[2]},
                "firstCapture": b.first_capture, "stoodStill10s": b.stood.len(),
                "stoodByClass": map(&b.stood.values().fold(BTreeMap::new(), |mut m, c| { *m.entry(*c).or_default() += 1; m })),
                "stuck5s": b.stuck.len(),
                "stuckByClass": map(&b.stuck.values().fold(BTreeMap::new(), |mut m, c| { *m.entry(*c).or_default() += 1; m })),
                "neverLeftSpawn": b.never_left.values().sum::<u32>(), "neverLeftByClass": map(&b.never_left),
                "ghostCaptureSeconds": b.ghost_seconds, "deathsInRegions": b.deaths_in_regions, "bodySecondsInRegions": b.body_seconds_in_regions,
                "removedSecondsInRegions": b.removed_seconds_in_regions, "corpseMaxSeconds": b.corpse_max, "corpseSecondsOver5": b.corpses_over_5s,
            });
            v["causes"] = serde_json::json!(b.causes);
            v["classTicks"] = map(&cl.alive);
            v["classJetTicks"] = map(&cl.jet);
            v["classJets"] = map(&cl.jets);
            v["classJumps"] = map(&cl.jumps);
            v["classSoldierHits"] = map(&cl.soldier_hits);
            v["classObjectHits"] = map(&cl.object_hits);
            v["negative"] = serde_json::json!(b.negative);
            v["spawns"] = serde_json::Value::Object(b.spawns.iter().map(|((t, p), n)| (format!("t{t}:{}", if *p == 0 { "?".into() } else { name(p) }), (*n).into())).collect());
            v.to_string()
        })
    }
}
