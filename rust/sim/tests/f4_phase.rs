//! Whole battle-phase contracts over synthetic stores, without game files.
use bfe_sim::battle::{mobile::{MobileEvent, Pad}, phase::{self, PhaseWorld},
    rules::{self, RulesWorld}, spawning::Draws, stats::Seen,
    world::{Member, PostsWorld, SpawningWorld, Victim}};
use bfe_sim::gen::*;
use bfe_sim::store::{Nullable, Ref, StrId, Strings};

#[derive(Default)]
struct World {
    members: Vec<Member>,
    victims: Vec<Victim>,
    events: Vec<&'static str>,
    attacker: Option<Ref<Character>>,
    destroyed: bool,
    remove_after_post: bool,
    spawn_unit: Option<Ref<Unit>>,
    move_victim: Option<Ref<Unit>>,
    registry: Option<bfe_sim::battle::registry::Registry>,
}
impl PostsWorld for World {
    fn capture_team(&mut self, team: f64) -> f64 { team }
    fn building_alive(&mut self, _: Ref<Building>) -> bool { !self.destroyed }
    fn pad(&mut self, _: &Store, _: f64) -> Pad { self.events.push("pad"); Pad::default() }
    fn vehicle_position(&mut self, _: &Store, _: Ref<Vehicle>) -> [f64; 3] { [0.0; 3] }
    fn class_spawn_world(&mut self, _: &Store, _: Ref<MobilePost>) -> Vec<[f64; 4]> { Vec::new() }
}
impl RulesWorld for World {
    fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
    fn unit_alive(&mut self, _: &Store, _: Ref<Unit>) -> bool { true }
    fn observe(&mut self, _: &Store, _: Ref<Character>, _: Option<Ref<Unit>>) -> Option<Seen> { self.events.push("stats"); None }
    fn living(&mut self, _: &Store, _: f64) -> Option<f64> { Some(1.0) }
    fn owned_values(&mut self, _: &mut Store, _: f64) -> Vec<Option<f64>> { vec![Some(1.0)] }
    fn owned_summary(&mut self, _: &mut Store, _: f64) -> (usize, f64) { (1, 1.0) }
    fn battle_over(&mut self, _: &mut Store, _: f64) { self.events.push("over"); }
}
impl SpawningWorld for World {
    fn clock_post(&mut self, s: &Store, post: SpawnPost) -> (bool, f64, f64) {
        match post {
            SpawnPost::Post(r) => { let p = s.post.get(r).unwrap(); (p.active, p.team, p.ai.as_ref().map_or(0.0, |a| a.ally_count)) }
            SpawnPost::MobilePost(r) => { let p = s.mobile_post.get(r).unwrap(); (p.active, p.team, 0.0) }
        }
    }
    fn health(&mut self, _: &Store, _: Ref<Unit>) -> f64 { 100.0 }
    fn force_kill(&mut self, _: &mut Store, _: Ref<Character>, _: Ref<Unit>) { panic!("unexpected forced death"); }
    fn request_objective(&mut self, _: &mut Store, _: Ref<Character>) { self.events.push("objective"); }
    fn republish(&mut self, _: &mut Store, _: Ref<Character>) { self.events.push("republish"); }
    fn hero_name(&mut self, _: StrId) -> Option<StrId> { None }
    fn spawn_unit(&mut self, s: &mut Store, c: Ref<Character>, class: Option<StrId>, _: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        let unit = self.spawn_unit.expect("an expected prepared unit spawn");
        assert_eq!(at, [1.0, 2.0, 3.0]); assert_eq!(yaw, Some(4.0));
        s.unit.get_mut(unit).unwrap().alive = true;
        let character = s.character.get_mut(c).unwrap(); character.unit = Some(unit); character.class_name = class;
        self.events.push("spawn"); Some(unit)
    }
}
impl PhaseWorld for World {
    fn visit_members(&mut self, s: &mut Store, visit: &mut dyn FnMut(&mut Store, Member)) {
        self.events.push("members");
        if let Some(registry) = &self.registry { registry.visit(s, None, false, &mut NativeTargets, visit); }
        else { for &m in &self.members { visit(s, m); } }
    }
    fn victims_into(&mut self, _: &Store, into: &mut Vec<Victim>) { into.clear(); self.events.push("victims"); into.extend_from_slice(&self.victims); }
    fn building_attacker(&mut self, _: &Store, _: Ref<Building>) -> Option<Ref<Character>> { self.attacker }
    fn vehicle_attacker(&mut self, _: &Store, _: Ref<Vehicle>) -> Option<Ref<Character>> { self.attacker }
    fn victim_position(&mut self, s: &Store, victim: Victim) -> [f64; 3] {
        if self.move_victim.is_some() { victim.position_in(s) }
        else { match victim { Victim::Unit { position, .. } | Victim::Hull { position, .. } => position } }
    }
    fn kill_victim(&mut self, s: &mut Store, _: Victim) {
        // A death's synchronous credit must be visible before post notices.
        self.events.push("kill"); rules::point(s, self.attacker);
        if let Some(next) = self.move_victim { s.unit.get_mut(next).unwrap().soldier = UnitBody::Soldier(Soldier { position: [100.0, 0.0, 0.0], ..Soldier::default() }); }
    }
    fn effect(&mut self, _: &mut Store, _: &str, _: [f64; 3]) { self.events.push("effect"); }
    fn post_event(&mut self, _: &mut Store, _: &PostEvent) {
        self.events.push("post");
        if self.remove_after_post { self.members[0].removed = true; }
    }
    fn mobile_event(&mut self, _: &mut Store, _: &MobileEvent) { self.events.push("mobile"); }
}
#[derive(Default)]
struct Random(usize);
impl Draws for Random { fn next(&mut self) -> f64 { self.0 += 1; 0.0 } }
fn region() -> Built { Built { r#type: RegionType::Sphere, radius: 10.0, ..Built::default() } }
fn post(s: &mut Store) -> Ref<Post> {
    let r = s.post.insert(Post { active: true, capture_time: 1.0, neutralize_time: 1.0,
        capture: Some(region()), ..Post::default() });
    s.command_posts.posts.push(r); r
}
fn character(s: &mut Store) -> Ref<Character> {
    let r = s.character.insert(Character { team: 1.0, ..Character::default() });
    s.battle.characters.push(r); r
}
fn member(c: Option<Ref<Character>>) -> Member {
    Member { soldier: true, alive: true, captures: true, team: 1.0, character: c, ..Member::default() }
}
fn run(s: &mut Store, w: &mut World) -> usize {
    let mut random = Random::default(); let mut gameplay = Random::default();
    phase::update_battle(1.0, s, &Strings::default(), false, |_, _| false, w, &mut random, &mut gameplay);
    assert_eq!(random.0, 0); gameplay.0
}
#[test]
fn capture_awards_count_non_characters_toward_the_64_member_limit() {
    let mut s = Store::default(); let p = post(&mut s); let winner = character(&mut s); let late = character(&mut s);
    let mut w = World::default(); w.members.resize(63, member(None));
    w.members.push(member(Some(winner))); w.members.push(member(Some(late)));
    run(&mut s, &mut w);
    assert_eq!(s.post.get(p).unwrap().team, 1.0);
    assert_eq!(s.character.get(winner).unwrap().captures, 1.0);
    assert_eq!(s.character.get(late).unwrap().captures, 0.0);
    assert!(s.command_posts.events.is_empty());
}
#[test]
fn noncapturing_heroes_and_seated_units_still_receive_capture_awards() {
    let mut s = Store::default(); let p = post(&mut s); let hero = character(&mut s); let rider = character(&mut s);
    let mut w = World::default();
    w.members = vec![member(None), Member { captures: false, character: Some(hero), ..member(None) },
        Member { seated: true, character: Some(rider), ..member(None) }];
    run(&mut s, &mut w);
    assert_eq!(s.post.get(p).unwrap().counts.get(&1.0), Some(&1.0));
    assert_eq!(s.character.get(hero).unwrap().captures, 1.0);
    assert_eq!(s.character.get(rider).unwrap().captures, 1.0);
}
#[test]
fn next_post_awards_observe_removal_by_the_previous_post_hook() {
    let mut s = Store::default(); post(&mut s); post(&mut s); let c = character(&mut s);
    let mut w = World { members: vec![member(Some(c))], remove_after_post: true, ..World::default() };
    run(&mut s, &mut w);
    assert_eq!(s.character.get(c).unwrap().captures, 1.0);
    assert_eq!(w.events.iter().filter(|e| **e == "post").count(), 2);
}
#[test]
fn destroyed_objectives_apply_effects_then_deaths_before_post_and_mobile_notices() {
    let mut s = Store::default(); let p = post(&mut s); let c = character(&mut s);
    let building = Ref::new(0, 1);
    let prop = s.post.get_mut(p).unwrap(); prop.building = Some(building); prop.team = 2.0;
    prop.kill_region = Nullable::Value(RegionDesc { r#type: RegionType::Sphere, size: vec![10.0, 0.0, 0.0], ..RegionDesc::default() });
    let mobile = s.mobile_post.insert(MobilePost { reported: Reported { active: true, team: 2.0, ..Reported::default() }, ..MobilePost::default() });
    s.mobile_posts.posts.push(mobile);
    let mut w = World { destroyed: true, attacker: Some(c),
        victims: vec![Victim::Unit { id: Ref::new(1, 1), position: [0.0; 3] }], ..World::default() };
    let draws = run(&mut s, &mut w);
    assert_eq!(draws, bfe_sim::battle::objectives::KILL_REGION_BLASTS * 4);
    let killed = w.events.iter().position(|e| *e == "kill").unwrap();
    assert!(w.events[..killed].iter().filter(|e| **e == "effect").count() > 0);
    assert_eq!(&w.events[killed..], &["kill", "post", "pad", "mobile"]);
    assert_eq!(s.character.get(c).unwrap().captures, 2.0);
}
#[test]
fn empty_no_vehicle_battle_advances_the_post_clock_without_random_draws() {
    let mut s = Store::default(); let mut w = World::default();
    assert_eq!(run(&mut s, &mut w), 0);
    assert_eq!(s.command_posts.time, 1.0);
    assert_eq!(w.events, ["members"]);
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn warmed_capture_award_and_rule_phases_allocate_nothing() {
    let mut s = Store::default(); let p = post(&mut s); let c = character(&mut s);
    let mut w = World { members: vec![member(Some(c))], events: Vec::with_capacity(32), ..World::default() };
    let strings = Strings::default(); let mut random = Random::default(); let mut gameplay = Random::default();
    let mut scratch = phase::Scratch::new(1, 1, 0, 0);
    // Both event buffers have seen a capture before measuring their reuse.
    for _ in 0..2 {
        s.post.get_mut(p).unwrap().team = 0.0;
        phase::update_battle_with_scratch(1.0, &mut s, &strings, false, |_, _| false,
            &mut w, &mut random, &mut gameplay, &mut scratch);
    }
    allocation::start();
    for _ in 0..1200 {
        w.events.clear(); s.post.get_mut(p).unwrap().team = 0.0;
        phase::update_battle_with_scratch(1.0, &mut s, &strings, false, |_, _| false,
            &mut w, &mut random, &mut gameplay, &mut scratch);
    }
    let requests = allocation::stop(); assert_eq!(requests, 0);
    assert_eq!(s.character.get(c).unwrap().captures, 1202.0);
    assert_eq!(random.0 + gameplay.0, 0);
}

struct NativeStats;
impl RulesWorld for NativeStats {
    fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
    fn unit_alive(&mut self, s: &Store, u: Ref<Unit>) -> bool { s.unit.get(u).is_some_and(|u| u.alive) }
    fn observe(&mut self, s: &Store, c: Ref<Character>, unit: Option<Ref<Unit>>) -> Option<Seen> {
        // recordOf precedes observation in the original tally loop. Canonical
        // unit reads need no global Store alias or foreign query callback.
        assert!(matches!(s.character.get(c).unwrap().record, Nullable::Value(_)));
        let u = s.unit.get(unit?)?;
        Some(Seen { health: Some(u.combat.health / u.combat.max_health), ..Seen::default() })
    }
    fn living(&mut self, _: &Store, _: f64) -> Option<f64> { None }
    fn owned_values(&mut self, _: &mut Store, _: f64) -> Vec<Option<f64>> { vec![] }
    fn battle_over(&mut self, _: &mut Store, _: f64) { panic!("unexpected game over"); }
}
#[test]
fn statistics_read_the_canonical_unit_store_between_short_character_borrows() {
    let mut s = Store::default(); let c = character(&mut s);
    let u = s.unit.insert(Unit { alive: true, combat: Combat { health: 25.0, max_health: 100.0, ..Combat::default() }, ..Unit::default() });
    s.character.get_mut(c).unwrap().unit = Some(u);
    rules::update(&mut s, 1.0, &Strings::default(), |_, _| true, &mut NativeStats);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { panic!("record missing"); };
    assert_eq!(record.health, 63.0);
    s.unit.get_mut(u).unwrap().combat.health = 100.0;
    rules::update(&mut s, 1.0, &Strings::default(), |_, _| true, &mut NativeStats);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { panic!("record missing"); };
    assert_eq!(record.health, 255.0); assert_eq!(s.battle.stats.time, 2.0);
}

impl bfe_sim::battle::battlefield::BattlefieldWorld for World {
    fn body(&mut self, _: &Store, _: Ref<Unit>) -> bfe_sim::battle::world::Body { panic!("empty boundary must not query units"); }
    fn vehicle(&mut self, _: &Store, _: Ref<Unit>) -> Option<Ref<Vehicle>> { panic!("empty boundary must not query hulls"); }
    fn combat_dead(&mut self, _: &Store, _: Ref<Unit>) -> bool { panic!("empty boundary must not query combat"); }
    fn kill(&mut self, _: &mut Store, _: Ref<Unit>) { panic!("unexpected boundary death"); }
    fn kill_droid(&mut self, _: &mut Store, _: Ref<Unit>) { panic!("unexpected boundary droid death"); }
    fn wreck(&mut self, _: &mut Store, _: Ref<Vehicle>) { panic!("unexpected boundary hull death"); }
}
impl bfe_sim::battle::tail::World for World {
    fn human_tail(&mut self, _: &mut Store, _: f64, _: &[bfe_sim::battle::human::Command]) { self.events.push("human"); }
    fn corpse_tail(&mut self, _: &mut Store) { self.events.push("corpse"); }
}
#[test]
fn whole_tail_orders_battle_work_before_local_realization_and_corpse_removal() {
    use bfe_sim::battle::{battlefield::Battlefield, tail::Tail};
    let mut s = Store::default(); let mut w = World::default();
    let mut tail = Tail::new(Battlefield::default(), 0, 0, 0, 0);
    let mut random = Random::default(); let mut gameplay = Random::default();
    tail.step(&F4TailInput { dt: 1.0, server: false, ..F4TailInput::default() }, &mut s,
        &Strings::default(), false, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    assert_eq!(w.events, ["members", "human", "corpse"]);
    w.events.clear();
    tail.step(&F4TailInput { dt: 1.0, server: true, ..F4TailInput::default() }, &mut s,
        &Strings::default(), false, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    assert_eq!(w.events, ["members", "corpse"]);
}

#[test]
fn native_spawn_clock_counts_posts_and_advances_each_clock_once() {
    use bfe_sim::battle::spawning;
    let mut s = Store::default(); let p = post(&mut s);
    s.post.get_mut(p).unwrap().ai = Some(PostAi { ally_count: 5.0, ..PostAi::default() });
    s.post.get_mut(p).unwrap().team = 1.0;
    s.spawning.teams.resize(8, SpawnTeam::default());
    s.spawning.teams[1].relations[3] = 1;
    s.spawning.teams[3].unit_count = 3.0;
    s.spawning.a = 15.0; s.spawning.b = 0.5; s.spawning.q = 30.0;
    s.spawning.wave_countdown = 0.0; s.spawning.stagger = 0.0;
    let mut w = World::default(); spawning::clock(&mut s, 0.25, &mut w);
    assert_eq!(s.spawning.locals_cap, 3.0); assert_eq!(s.spawning.wave, 1.0);
    assert_eq!(s.spawning.wave_countdown, 14.75); assert_eq!(s.spawning.released, 1.0);
    assert_eq!(s.spawning.stagger, 0.25);
}

#[test]
fn warmed_tail_with_native_spawn_clocks_and_living_characters_allocates_nothing() {
    use bfe_sim::battle::{battlefield::Battlefield, tail::Tail};
    let mut s = Store::default(); let p = post(&mut s); let c = character(&mut s);
    let u = s.unit.insert(Unit { alive: true, ..Unit::default() }); s.character.get_mut(c).unwrap().unit = Some(u);
    s.spawning.characters.push(c); s.spawning.teams.resize(8, SpawnTeam::default());
    s.spawning.a = 15.0; s.spawning.b = 0.5; s.spawning.q = 30.0;
    let mut w = World { members: vec![member(Some(c))], events: Vec::with_capacity(32), ..World::default() };
    let strings = Strings::default(); let mut random = Random::default(); let mut gameplay = Random::default();
    let mut tail = Tail::new(Battlefield::default(), 1, 1, 0, 0);
    let input = F4TailInput { dt: 1.0, server: true, ..F4TailInput::default() };
    for _ in 0..2 { s.post.get_mut(p).unwrap().team = 0.0;
        tail.step(&input, &mut s, &strings, true, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    }
    allocation::start();
    for _ in 0..1200 {
        w.events.clear(); s.post.get_mut(p).unwrap().team = 0.0;
        tail.step(&input, &mut s, &strings, true, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    }
    let requests = allocation::stop(); assert_eq!(requests, 0);
    assert_eq!(s.character.get(c).unwrap().captures, 1202.0);
    assert!(s.spawning.wave > 0.0); assert_eq!(random.0 + gameplay.0, 0);
}

#[test]
fn native_tail_releases_a_bot_wave_once_and_spawns_at_its_prepared_path() {
    use bfe_sim::battle::{battlefield::Battlefield, spawning, tail::Tail};
    let mut s = Store::default(); let p = post(&mut s); s.post.get_mut(p).unwrap().team = 1.0;
    s.post.get_mut(p).unwrap().ai = Some(PostAi { spawn_path: Some(SpawnPath { present: true, points: vec![[1.0, 2.0, 3.0, 4.0]] }), ..PostAi::default() });
    let c = character(&mut s); let u = s.unit.insert(Unit::default());
    let ch = s.character.get_mut(c).unwrap(); ch.class_index = Nullable::Value(0.0); ch.class_name = Some(StrId(1));
    ch.spawn = Some(Spawn { ready: true, timer: 0.0, wave: Some(0.0), ordinal: Some(-1.0), post: PostRef::Post(p), ..Spawn::default() });
    s.spawning.teams = spawning::make_teams(32.0); s.spawning.teams[1].characters.push(c);
    s.spawning.teams[1].classes.push(SpawnClass { name: StrId(1), target: 1.0, hero: false });
    s.spawning.teams[1].tickets = 10.0; s.spawning.characters.push(c);
    s.spawning.a = 15.0; s.spawning.b = 0.5; s.spawning.q = 30.0; s.spawning.wave_countdown = 15.0;
    s.battle.reinforcements.set(1.0, 10.0);
    let mut w = World { spawn_unit: Some(u), ..World::default() };
    let mut tail = Tail::new(Battlefield::default(), 1, 1, 0, 0);
    let input = F4TailInput { dt: 1.0, server: true, ..F4TailInput::default() };
    let mut random = Random::default(); let mut gameplay = Random::default();
    tail.step(&input, &mut s, &Strings::default(), true, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    assert_eq!(w.events, ["members", "spawn", "republish", "corpse"]); assert_eq!(s.character.get(c).unwrap().unit, Some(u));
    assert_eq!(s.spawning.teams[1].living, 1.0); assert_eq!(random.0, 1); assert_eq!(gameplay.0, 0);
    w.events.clear();
    tail.step(&input, &mut s, &Strings::default(), true, |_, _| false, &[], &mut w, &mut random, &mut gameplay);
    assert_eq!(w.events, ["members", "corpse"]); assert_eq!(s.spawning.teams[1].living, 1.0); assert_eq!(random.0, 1);
}

#[test]
fn class_deficit_selection_keeps_heroes_and_uses_one_draw_only_when_needed() {
    use bfe_sim::battle::spawning;
    let mut s = Store::default(); let c = character(&mut s); let other = character(&mut s);
    s.character.get_mut(c).unwrap().class_index = Nullable::Value(0.0);
    s.character.get_mut(other).unwrap().class_index = Nullable::Value(0.0);
    s.spawning.teams = spawning::make_teams(32.0); let team = &mut s.spawning.teams[1];
    team.characters.extend([c, other]); team.hero_count = 1.0;
    team.classes = vec![SpawnClass { name: StrId(1), target: 2.0, hero: false },
        SpawnClass { name: StrId(2), target: 2.0, hero: false }, SpawnClass { name: StrId(3), target: 1.0, hero: true }];
    let mut random = Random::default(); assert_eq!(spawning::pick_class(&s, c, &mut random), 1.0); assert_eq!(random.0, 1);
    s.character.get_mut(c).unwrap().class_index = Nullable::Value(2.0);
    assert_eq!(spawning::pick_class(&s, c, &mut random), 2.0); assert_eq!(random.0, 1);
}

#[test]
fn repeated_dead_character_post_choices_reuse_scalar_views_and_prepared_paths() {
    use bfe_sim::battle::spawning;
    let mut s = Store::default(); let p = post(&mut s); s.post.get_mut(p).unwrap().team = 1.0;
    s.post.get_mut(p).unwrap().ai = Some(PostAi { spawn_path: Some(SpawnPath { present: true,
        points: vec![[1.0, 2.0, 3.0, 4.0]; 16] }), ..PostAi::default() });
    let c = character(&mut s); s.character.get_mut(c).unwrap().spawn = Some(Spawn { post: PostRef::Post(p), ..Spawn::default() });
    s.spawning.teams = spawning::make_teams(32.0);
    let mut scratch = spawning::Scratch::new(1, 0); let mut w = World::default(); let mut random = Random::default();
    assert_eq!(spawning::choose_post_with_scratch(&mut s, c, &mut w, &mut random, &mut scratch), Some(SpawnPost::Post(p)));
    allocation::start();
    for _ in 0..1200 {
        assert_eq!(spawning::choose_post_with_scratch(&mut s, c, &mut w, &mut random, &mut scratch), Some(SpawnPost::Post(p)));
    }
    let requests = allocation::stop(); assert_eq!(requests, 0); assert_eq!(random.0, 0);
}

#[test]
fn objective_membership_is_frozen_but_later_positions_follow_nested_deaths() {
    let mut s = Store::default(); let p = post(&mut s);
    let prop = s.post.get_mut(p).unwrap(); prop.building = Some(Ref::new(0, 1)); prop.team = 2.0;
    prop.kill_region = Nullable::Value(RegionDesc { r#type: RegionType::Sphere, size: vec![10.0, 0.0, 0.0], ..RegionDesc::default() });
    let a = s.unit.insert(Unit::default()); let b = s.unit.insert(Unit::default());
    let mut w = World { destroyed: true, move_victim: Some(b), victims: vec![
        Victim::Unit { id: a, position: [0.0; 3] }, Victim::Unit { id: b, position: [0.0; 3] }], ..World::default() };
    run(&mut s, &mut w);
    assert_eq!(w.events.iter().filter(|&&e| e == "kill").count(), 1);
    assert_eq!(w.events.iter().filter(|&&e| e == "effect").count(), 15);
}

struct NativeTargets;
impl bfe_sim::battle::registry::Targets for NativeTargets {
    fn member(&mut self, _: &Store, _: bfe_sim::gen::Member) -> Member { Member { alive: true, ..Member::default() } }
}
#[test]
fn canonical_mixed_registry_counts_targets_and_awards_a_noncapturing_hero() {
    use bfe_sim::battle::registry::Registry;
    use bfe_sim::store::DefId;
    let mut s = Store::default(); let p = post(&mut s); let hero = character(&mut s); let late = character(&mut s);
    for i in 0..63 { s.units.list.push(bfe_sim::gen::Member::VehicleTarget(Ref::new(100 + i, 1))); }
    let h = s.unit.insert(Unit { alive: true, team: 1.0, character: Some(hero), class_def: DefId(0), ..Unit::default() });
    let b = s.unit.insert(Unit { alive: true, team: 1.0, character: Some(late), class_def: DefId(1), ..Unit::default() });
    s.units.list.push(bfe_sim::gen::Member::Unit(h)); s.units.list.push(bfe_sim::gen::Member::Unit(b));
    let mut registry = Registry::default(); registry.class(DefId(0), false); registry.class(DefId(1), true);
    let mut w = World { registry: Some(registry), ..World::default() }; run(&mut s, &mut w);
    assert_eq!(s.post.get(p).unwrap().team, 1.0); assert_eq!(s.character.get(hero).unwrap().captures, 1.0);
    assert_eq!(s.character.get(late).unwrap().captures, 0.0);
    let mut victims = Vec::with_capacity(2); Registry::victims(&s, &mut victims);
    assert_eq!(victims, [Victim::Unit { id: h, position: [0.0; 3] }, Victim::Unit { id: b, position: [0.0; 3] }]);
}

#[test]
fn an_oversized_class_roster_stays_within_fixed_deficit_scratch() {
    use bfe_sim::battle::spawning;
    let mut s=Store::default();let c=character(&mut s);
    s.character.get_mut(c).unwrap().class_index=Nullable::Value(4095.0);
    s.spawning.teams=spawning::make_teams(32.0);let team=&mut s.spawning.teams[1];
    team.characters.push(c);team.classes=vec![SpawnClass{name:StrId(1),target:1.0,..Default::default()};4096];
    let mut random=Random::default();let index=spawning::pick_class(&s,c,&mut random);
    assert!(index>=0.0 && index<(spawning::CLASS_CAPACITY as f64));assert_eq!(random.0,1);
}
