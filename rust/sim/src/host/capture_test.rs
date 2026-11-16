//! Who counts toward a command post, in a real battle (BFE_CONTENT_DIR, else
//! skipped): on every tick no post may count more soldiers of a team than
//! living soldiers of that team stand on foot inside its region, and a
//! soldier killed inside a region stops counting the tick he dies, while his
//! body lies there and after it is gone.
use crate::abi::context::Context;
use crate::gen::{Attacker, WorldClockState, WorldTickInput};
use crate::store::Ref;
use crate::world::admit::Options;

fn clock() -> WorldClockState {
    WorldClockState { hz: 60.0, dt: 1.0 / 60.0, max_steps: 8.0, budget: 0.035, max_debt: 8.0, ..Default::default() }
}

struct Battle {
    context: Context,
    template: WorldTickInput,
}

impl Battle {
    fn new(content: &std::path::Path, enhanced: bool) -> Battle {
        Self::on(content, "nab2", "rep", 1, enhanced)
    }
    fn on(content: &std::path::Path, map: &str, side: &str, seed: u32, enhanced: bool) -> Battle {
        Self::with_bots(content, map, side, seed, enhanced, Default::default())
    }
    fn with_bots(content: &std::path::Path, map: &str, side: &str, seed: u32, enhanced: bool, bots: crate::world::admit::Bots) -> Battle {
        let context = Context::new();
        let options = Options { map: map.into(), side: side.into(), enhanced, seed, heroes: false, unit: None, name: None, bots };
        let files = bfe_content::files::DirFiles::new(content);
        let admitted = context.world_admit(&files, &options).expect("admission");
        context.world_step_init(admitted.setup.clone()).unwrap();
        let h = admitted.handles;
        let (explosions, host) = super::content_tables(&context, content, &options, h.store_source).unwrap();
        context.world_host_start(Some(super::Capture { combat: h.combat, presentation: h.presentation, store_source: h.store_source, explosions }), host).unwrap();
        context.world_host_client(h.player, h.edges).unwrap();
        let template = WorldTickInput { clock: clock(), units: admitted.units, ai: true, live: true, mouse_scale: 1.0, ..Default::default() };
        Battle { context, template }
    }

    fn tick(&mut self) {
        let next = self.template.clock.tick + 1.0;
        let units = self.context.world_prepare_units().unwrap();
        let input = WorldTickInput { dt: 1.0 / 60.0, tick: next, at: 1000.0 + next * 1000.0 / 60.0, units, ..self.template.clone() };
        let output = self.context.world_tick(input).unwrap();
        assert!(output.accepted, "tick {next} refused");
        self.template.clock = output.clock;
    }

    /// Every post's counts against the living soldiers on foot in its region;
    /// panics on a surplus, naming who stands there. Also answers whether
    /// `watched` is inside any region, and his life flags.
    fn check(&self, watched: Option<u32>, what: &str) -> Option<(bool, bool)> {
        self.context.owned(|w| {
            let store = &w.sim.store;
            let mut seen = None;
            for (_, post) in store.post.iter().filter(|(_, p)| p.capture.is_some()) {
                let mut living = [0.0f64; 8];
                for (r, u) in store.unit.iter() {
                    let at = u.position();
                    if !crate::battle::posts::contains(post, &at) { continue; }
                    if Some(r.slot) == watched { seen = Some((u.alive, u.removed)); }
                    if u.alive && !u.dead && !u.removed && u.seat.is_none() && (0.0..8.0).contains(&u.team) { living[u.team as usize] += 1.0; }
                }
                // Locals may count for an army they side with (geo1's Geonosians count for the CIS).
                let locals: f64 = living[3..].iter().sum();
                living[1] += locals;
                living[2] += locals;
                for (team, &n) in post.counts.iter() {
                    if (1.0..3.0).contains(&team) && n > living[team as usize] {
                        // Who of that team is in or near the region, and how.
                        let mut near = String::new();
                        for (r, u) in store.unit.iter().filter(|(_, u)| u.team == team || u.team > 2.0) {
                            let at = u.position();
                            let d = ((at.x - post.position[0]).powi(2) + (at.y - post.position[1]).powi(2) + (at.z - post.position[2]).powi(2)).sqrt();
                            if crate::battle::posts::contains(post, &at) || d < 30.0 {
                                near += &format!("\n  unit {} team {} class {} human {} inside {} d {d:.1} alive {} dead {} removed {} seated {} held {} at {:.2},{:.2},{:.2}",
                                    r.slot, u.team, u.class_def.0, u.human, crate::battle::posts::contains(post, &at),
                                    u.alive, u.dead, u.removed, u.seat.is_some(), matches!(u.held, crate::store::Nullable::Value(_)), at.x, at.y, at.z);
                            }
                        }
                        panic!("{what}: post {} counts {n} of team {team}, {} alive inside{near}",
                            String::from_utf16_lossy(w.sim.strings.units(post.name)), living[team as usize]);
                    }
                }
            }
            seen
        }).unwrap()
    }

    /// A living soldier standing inside a capture region (the human when
    /// `human`), and his team.
    fn inside(&self, human: bool) -> Option<(Ref<crate::gen::Unit>, f64)> {
        self.context.owned(|w| {
            let store = &w.sim.store;
            store.unit.iter().find(|(_, u)| u.human == human && u.alive && !u.dead && u.seat.is_none()
                && store.post.iter().any(|(_, p)| crate::battle::posts::contains(p, &u.position())))
                .map(|(r, u)| (r, u.team))
        }).unwrap()
    }

    /// The post the soldier stands in: its name and counts.
    fn counts_at(&self, r: Ref<crate::gen::Unit>) -> (String, Vec<(f64, f64)>) {
        self.context.owned(|w| {
            let at = w.sim.store.unit.get(r).unwrap().position();
            let (_, post) = w.sim.store.post.iter().find(|(_, p)| crate::battle::posts::contains(p, &at)).unwrap();
            (String::from_utf16_lossy(w.sim.strings.units(post.name)), post.counts.iter().map(|(t, n)| (t, *n)).collect())
        }).unwrap()
    }
    fn post_counts(&self, of: &(String, Vec<(f64, f64)>)) -> Vec<(f64, f64)> {
        self.context.owned(|w| {
            let (_, post) = w.sim.store.post.iter().find(|(_, p)| String::from_utf16_lossy(w.sim.strings.units(p.name)) == of.0).unwrap();
            post.counts.iter().map(|(t, n)| (t, *n)).collect()
        }).unwrap()
    }

    /// Hands each named post to a team, as a finished capture leaves it.
    fn stage(&self, owners: &[(&str, f64)]) {
        self.context.owned(|w| {
            let named: Vec<_> = w.sim.store.post.iter().map(|(r, p)| (r, String::from_utf16_lossy(w.sim.strings.units(p.name)))).collect();
            for (r, name) in named {
                if let Some(&(_, team)) = owners.iter().find(|(n, _)| *n == name) {
                    let post = w.sim.store.post.get_mut(r).unwrap();
                    (post.team, post.progress, post.neutralize, post.toward) = (team, 0.0, 0.0, 0.0);
                }
            }
        }).unwrap();
    }
    /// A post's owner by name.
    fn owner(&self, name: &str) -> f64 {
        self.context.owned(|w| {
            w.sim.store.post.iter().find(|(_, p)| String::from_utf16_lossy(w.sim.strings.units(p.name)) == name).map_or(f64::NAN, |(_, p)| p.team)
        }).unwrap()
    }
    /// The teams' reinforcements and the loser once the battle is over.
    fn tickets(&self) -> ([f64; 2], Option<f64>) {
        self.context.owned(|w| {
            let b = &w.sim.store.battle;
            ([b.reinforcements.at(1.0).unwrap_or(f64::NAN), b.reinforcements.at(2.0).unwrap_or(f64::NAN)], b.over.then_some(b.loser).flatten())
        }).unwrap()
    }

    fn kill(&self, r: Ref<crate::gen::Unit>) {
        self.context.owned(|w| {
            crate::units::f3::with_services_in(w, |w, services| {
                crate::combat::live::damage_in(w, r, f64::INFINITY, Attacker::Null, services.expect("battle services"));
            })
        }).unwrap();
    }
}

fn dead_soldiers_stop_counting(enhanced: bool) {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let mut battle = Battle::new(std::path::Path::new(&content), enhanced);
    // The bots come in after ten seconds; wait for one standing in a post.
    let mut victim = None;
    for _ in 0..60 * 60 {
        battle.tick();
        battle.check(None, "before");
        // One his post already counts (his team present in its counts).
        victim = battle.inside(false).filter(|&(r, team)| battle.counts_at(r).1.iter().any(|&(t, n)| t == team && n > 0.0));
        if victim.is_some() { break; }
    }
    let (bot, _) = victim.expect("a bot inside a capture region within a minute");
    let human = battle.inside(true).filter(|&(r, team)| battle.counts_at(r).1.iter().any(|&(t, n)| t == team && n > 0.0));
    let before = battle.counts_at(bot);
    battle.kill(bot);
    if let Some((human, _)) = human { battle.kill(human); }
    // Dying, lying 4 s, removed, and his respawn elsewhere: never counted.
    for tick in 0..60 * 10 {
        battle.tick();
        if tick == 0 {
            let after = battle.post_counts(&before);
            let team = battle.context.owned(|w| w.sim.store.unit.get(bot).map(|u| u.team)).unwrap().unwrap();
            let n = |c: &[(f64, f64)]| c.iter().find(|p| p.0 == team).map_or(0.0, |p| p.1);
            assert!(n(&after) < n(&before.1), "the kill left {} counting {} of team {team} (was {})", before.0, n(&after), n(&before.1));
            eprintln!("killed bot {} (human inside: {}): its post counted {before:?}, a tick later {:?}", bot.slot, human.is_some(), after);
        }
        let at = battle.check(Some(bot.slot), &format!("tick {tick} after the kill"));
        if let Some((alive, _)) = at { assert!(!alive || tick > 4 * 60, "the killed bot is alive again at tick {tick}"); }
        if let Some((human, _)) = human { battle.check(Some(human.slot), &format!("tick {tick} after the human's death")); }
    }
    battle.context.dispose().unwrap();
}

#[test]
fn dead_soldiers_stop_counting_toward_posts_classic() {
    dead_soldiers_stop_counting(false);
}

#[test]
fn dead_soldiers_stop_counting_toward_posts_enhanced() {
    dead_soldiers_stop_counting(true);
}

/// The bleed and the no-post countdown on nab2 with no bots, each team given
/// posts by hand. nab2 scores 10 a post held and 1 a neutral one; a score up
/// to 1 bleeds 3.0, to 11 bleeds 2.25, to 21 0.75, more nothing, one ticket per
/// 4 points gathered; a team with a score of 0 counts down 20 s and loses.
#[test]
fn bleed_and_the_no_post_countdown_follow_the_posts_held() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let case = |owners: [f64; 6], seconds: u32| {
        let off = crate::world::admit::Bots::read("off", "medium", "full");
        let mut battle = Battle::with_bots(std::path::Path::new(&content), "nab2", "rep", 1, false, off);
        battle.tick();
        let names = ["cp1", "cp2", "cp3", "cp4", "cp5", "cp6"];
        battle.stage(&names.iter().zip(owners).map(|(n, t)| (*n, t)).collect::<Vec<_>>());
        let (start, _) = battle.tickets();
        let mut over = None;
        for tick in 1..=seconds * 60 {
            battle.tick();
            if over.is_none() { if let (left, Some(loser)) = battle.tickets() { over = Some((tick as f64 / 60.0, loser, left)); } }
        }
        let (end, _) = battle.tickets();
        battle.context.dispose().unwrap();
        ([start[0] - end[0], start[1] - end[1]], over)
    };
    // The player alone in cp2, handed to the other team: neutralized after
    // neutralizeTime (12 s) and his after captureTime (10 s) more.
    {
        let off = crate::world::admit::Bots::read("off", "medium", "full");
        let mut battle = Battle::with_bots(std::path::Path::new(&content), "nab2", "rep", 1, false, off);
        battle.tick();
        battle.stage(&[("cp2", 1.0)]);
        let (mut neutral_at, mut captured_at) = (None, None);
        for tick in 1..=30 * 60 {
            battle.tick();
            let owner = battle.owner("cp2");
            if owner == 0.0 && neutral_at.is_none() { neutral_at = Some(tick as f64 / 60.0); }
            if owner == 2.0 && captured_at.is_none() { captured_at = Some(tick as f64 / 60.0); }
        }
        battle.context.dispose().unwrap();
        eprintln!("one soldier at cp2: neutral at {neutral_at:?} s, captured at {captured_at:?} s");
        let (n, c) = (neutral_at.expect("neutralized"), captured_at.expect("captured"));
        assert!((11.9..12.1).contains(&n) && (21.9..22.1).contains(&c), "neutral {n} s, captured {c} s");
    }
    let one = case([1.0, 2.0, 2.0, 2.0, 2.0, 2.0], 60);
    // The harness's player stands at cp2: team 1 gets posts away from him.
    let two = case([1.0, 2.0, 1.0, 2.0, 2.0, 2.0], 60);
    let none = case([2.0; 6], 30);
    let neutral = case([0.0, 2.0, 2.0, 2.0, 2.0, 2.0], 60);
    eprintln!("tickets lost in 60 s holding one post {one:?}, two {two:?}; none: {none:?}; none but a neutral one {neutral:?}");
    assert_eq!(one.0, [33.0, 0.0], "one post bleeds 2.25 / 4 a second");
    assert_eq!(two.0, [11.0, 0.0], "two posts bleed 0.75 / 4 a second");
    let (at, loser, _) = none.1.expect("a team without posts loses");
    assert!(loser == 1.0 && (20.0..20.2).contains(&at), "lost by team {loser} at {at} s");
    assert!(neutral.1.is_none(), "a neutral post pauses the countdown");
    assert_eq!(neutral.0, [44.0, 0.0], "a neutral post alone scores 1: 3.0 / 4 a second (the staging tick gathers none)");
}

/// Whole battles on maps with droidekas, vehicles, turrets and locals: on
/// every tick no post counts more soldiers of a team than stand alive on foot
/// inside it (dying bodies, corpses, riders and gunners never count).
/// BFE_CAPTURE_MAPS (MAP:SIDE:SEED,...) and BFE_CAPTURE_TICKS choose them.
#[test]
#[ignore]
fn posts_count_only_the_living_on_foot_all_battle() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let maps = std::env::var("BFE_CAPTURE_MAPS").unwrap_or_else(|_| "nab2:rep:1,geo1:rep:1,kas2:cis:1,hot1:imp:1".into());
    let ticks: u32 = std::env::var("BFE_CAPTURE_TICKS").ok().and_then(|t| t.parse().ok()).unwrap_or(60 * 900);
    for spec in maps.split(',') {
        let mut part = spec.split(':');
        let (map, side, seed) = (part.next().unwrap(), part.next().unwrap_or("rep"), part.next().and_then(|s| s.parse().ok()).unwrap_or(1));
        let mut battle = Battle::on(std::path::Path::new(&content), map, side, seed, false);
        for tick in 0..ticks {
            battle.tick();
            battle.check(None, &format!("{map} tick {tick}"));
        }
        eprintln!("{map}: {ticks} ticks, no post counted more than its living soldiers on foot");
        battle.context.dispose().unwrap();
    }
}

/// Geonosis: every post with a destructible building (the Techno Union ships,
/// the forward command centre) knows it, arrives at its sphere and lends the
/// building its team, so destroying the building takes the post out.
#[test]
fn destructible_posts_know_their_buildings() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let context = Context::new();
    let options = Options { map: "geo1".into(), side: "cis".into(), enhanced: false, seed: 1, heroes: false, unit: None, name: None, bots: Default::default() };
    let files = bfe_content::files::DirFiles::new(std::path::Path::new(&content));
    context.world_admit(&files, &options).expect("admission");
    let linked = context.owned(|w| {
        let store = &w.sim.store;
        store.post.iter().filter_map(|(_, p)| p.building.map(|b| (String::from_utf16_lossy(w.sim.strings.units(p.name)), p.team, p.radius, b))).collect::<Vec<_>>()
    }).unwrap();
    assert_eq!(linked.len(), 4, "geo1 has four destructible posts: {linked:?}");
    for (name, team, radius, _) in &linked {
        assert!(radius.is_some_and(|r| r > 5.0), "{name} arrives at its building's sphere, not its middle: {radius:?}");
        assert!(*team == 1.0 || *team == 2.0, "{name} belongs to an army");
    }
    context.dispose().unwrap();
}

/// A landed transport is offered to human players as the spawn post named
/// after its pad (transport-<pad>), the name the entity registry gives it.
/// Offered under its hull class name, a player who spawned there had no post
/// id in their own summary and the battle stopped (Bespin Platforms, Clone Wars:
/// the Republic gunship).
#[test]
fn landed_transport_spawn_choices_are_registered_posts() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let context = Context::new();
    let options = Options { map: "bes1".into(), side: "rep".into(), enhanced: false, seed: 1, heroes: false, unit: None, name: None, bots: Default::default() };
    let admitted = context.world_admit(&bfe_content::files::DirFiles::new(std::path::Path::new(&content)), &options).expect("admission");
    let source = admitted.handles.store_source as usize;
    let transports = context.owned(|w| {
        let capture = &w.wire.as_deref().expect("wire owner").sources.get(source).expect("store source").capture;
        let mut transports = 0;
        for (hull, name) in crate::battle::f4_tail::human_choices(&w.battle).hull_names() {
            let v = w.sim.store.vehicle.get(hull).expect("prepared hull");
            let text = String::from_utf16_lossy(w.sim.strings.units(name));
            assert_eq!(text, format!("transport-{}", v.slot_index), "hull on pad {} offered as {text}", v.slot_index);
            assert!(capture.post_id(name).is_some(), "{text} has no registry id");
            if v.flight_class == Some(crate::gen::FlightClass::Transport) { transports += 1; }
        }
        transports
    }).unwrap();
    assert!(transports > 0, "Bespin Platforms in the Clone Wars has landing transports");
    context.dispose().unwrap();
}
