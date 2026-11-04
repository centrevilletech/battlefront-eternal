//! A whole battle phase on the shared module. No global store or string
//! borrow spans an import; callbacks publish all of this phase's kinds.
use super::mobile::{MobileEvent, Pad};
use super::phase::{self, PhaseWorld};
use super::posts;
use super::posts_module::JsPostsWorld;
use super::rules::RulesWorld;
use super::rules_module::{self, JsRulesWorld, ModuleText};
use super::spawning::Draws;
use super::spawning_module::{swap, JsDraws, JsSpawningWorld};
use super::stats::Seen;
use super::world::{Member, PostsWorld, SpawningWorld, Victim};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};

fn query(op: BpQueryOp, building: Option<Ref<Building>>, vehicle: Option<Ref<Vehicle>>) -> BpQueryResult {
    import_battle_phase_query(&BpQueryArgs { op, building, vehicle })
}

struct GameplayDraws;
impl Draws for GameplayDraws {
    fn next(&mut self) -> f64 { query(BpQueryOp::Draw, None, None).number }
    fn uniform(&mut self) -> f64 { self.next() }
}

struct JsPhaseWorld { spawning: bool }
impl JsPhaseWorld {
    fn action(&mut self, s: &mut Store, args: BpActionArgs) {
        swap(s);
        import_battle_phase_action(&args);
        swap(s);
    }
}

impl PostsWorld for JsPhaseWorld {
    fn capture_team(&mut self, team: f64) -> f64 { JsPostsWorld.capture_team(team) }
    fn building_alive(&mut self, building: Ref<Building>) -> bool { JsPostsWorld.building_alive(building) }
    fn pad(&mut self, slot: f64) -> Pad { JsPostsWorld.pad(slot) }
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3] { JsPostsWorld.vehicle_position(vehicle) }
    fn class_spawn_world(&mut self, s: &Store, post: Ref<MobilePost>) -> Vec<[f64; 4]> { JsPostsWorld.class_spawn_world(s, post) }
}

impl RulesWorld for JsPhaseWorld {
    fn relation(&mut self, a: f64, b: f64) -> f64 { JsRulesWorld.relation(a, b) }
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool { JsRulesWorld.unit_alive(unit) }
    fn observe(&mut self, character: Ref<Character>, unit: Option<Ref<Unit>>) -> Option<Seen> { JsRulesWorld.observe(character, unit) }
    fn living(&mut self, s: &Store, team: f64) -> Option<f64> {
        if self.spawning { s.spawning.teams.iter().find(|t| t.index == team).map(|t| t.living) }
        else { JsRulesWorld.living(s, team) }
    }
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>> {
        posts::owned(s, team, self).into_iter().map(|p| posts::value_bleed(s, p)).collect()
    }
    fn battle_over(&mut self, s: &mut Store, loser: f64) {
        self.action(s, BpActionArgs { op: BpActionOp::Over, number: loser, ..Default::default() });
    }
}

impl SpawningWorld for JsPhaseWorld {
    fn spawn_goal(&mut self, character: Ref<Character>) -> Option<[f64; 3]> { JsSpawningWorld.spawn_goal(character) }
    fn health(&mut self, unit: Ref<Unit>) -> f64 { JsSpawningWorld.health(unit) }
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId> { JsSpawningWorld.hero_name(class_name) }
    fn force_kill(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) { JsSpawningWorld.force_kill(s, character, unit); }
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>) { JsSpawningWorld.request_objective(s, character); }
    fn republish(&mut self, s: &mut Store, character: Ref<Character>) { JsSpawningWorld.republish(s, character); }
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        JsSpawningWorld.spawn_unit(s, character, class_name, post, at, yaw)
    }
}

impl PhaseWorld for JsPhaseWorld {
    fn members(&mut self) -> Vec<Member> {
        query(BpQueryOp::Members, None, None).members.into_iter().map(|m| Member {
            soldier: m.soldier, alive: m.alive, removed: m.removed, seated: m.seated, held: m.held,
            captures: m.captures, player_mounted: m.player_mounted, team: m.team, position: m.position, character: m.character,
        }).collect()
    }
    fn building_attacker(&mut self, building: Ref<Building>) -> Option<Ref<Character>> {
        query(BpQueryOp::BuildingAttacker, Some(building), None).character
    }
    fn vehicle_attacker(&mut self, vehicle: Ref<Vehicle>) -> Option<Ref<Character>> {
        query(BpQueryOp::VehicleAttacker, None, Some(vehicle)).character
    }
    fn kill_region_victims(&mut self) -> Vec<Victim> {
        let result = query(BpQueryOp::Victims, None, None);
        let mut victims: Vec<_> = result.members.into_iter().filter(|m| m.soldier && m.alive && !m.removed)
            .map(|m| Victim::Unit { id: m.unit, position: m.position }).collect();
        victims.extend(result.hulls.into_iter().filter(|h| h.alive).map(|h| Victim::Hull { id: h.vehicle, position: h.position }));
        victims
    }
    fn kill_victim(&mut self, s: &mut Store, victim: Victim) {
        let mut args = BpActionArgs { op: BpActionOp::Kill, ..Default::default() };
        match victim { Victim::Unit { id, .. } => args.unit = Some(id), Victim::Hull { id, .. } => args.vehicle = Some(id) }
        self.action(s, args);
    }
    fn effect(&mut self, s: &mut Store, name: &str, at: [f64; 3]) {
        let name = sim().strings.intern_str(name);
        self.action(s, BpActionArgs { op: BpActionOp::Effect, name: Some(name), position: at, ..Default::default() });
    }
    fn post_event(&mut self, s: &mut Store, event: &PostEvent) {
        self.action(s, BpActionArgs { op: BpActionOp::Post, post: Some(event.clone()), ..Default::default() });
    }
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent) {
        let mobile = match *event {
            MobileEvent::Lost { post, vehicle, from } => PsMobileEvent { kind: PsMobileEventKind::Lost, post, vehicle, from, to: None },
            MobileEvent::Owner { post, from, to } => PsMobileEvent { kind: PsMobileEventKind::Owner, post, vehicle: None, from, to: Some(to) },
        };
        self.action(s, BpActionArgs { op: BpActionOp::Mobile, mobile: Some(mobile), ..Default::default() });
    }
}

#[no_mangle]
pub extern "C" fn battle_phase_words() -> u32 { BpInput::WORDS as u32 }

#[no_mangle]
pub extern "C" fn battle_phase_run() {
    let input = { let (row, heap) = sim().calls.args(); BpInput::take(row, heap) };
    let mut s = Store::default();
    swap(&mut s);
    phase::update_battle(input.dt, &mut s, &ModuleText, input.spawning,
        |character, _| rules_module::query(BrQueryOp::StatsActive, 0.0, 0.0, None, Some(character)).flag,
        &mut JsPhaseWorld { spawning: input.spawning }, &mut JsDraws, &mut GameplayDraws);
    swap(&mut s);
}
