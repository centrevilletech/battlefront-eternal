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
        let context = Context::new();
        let options = Options { map: "nab2".into(), side: "rep".into(), enhanced, seed: 1, heroes: false, unit: None, name: None, bots: Default::default() };
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
                for (team, &n) in post.counts.iter() {
                    if (1.0..3.0).contains(&team) {
                        assert!(n <= living[team as usize], "{what}: post {} counts {n} of team {team}, {} alive inside",
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
