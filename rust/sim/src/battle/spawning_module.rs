//! Live spawn manager commands. World actions exchange the current
//! battle, Character, post and spawning state before calling JavaScript,
//! then take back any changes made by nested death/spawn/AI commands.
use super::mobile::Pad;
use super::posts::{post_ref, spawn_post};
use super::posts_module::{self, JsPostsWorld};
use super::rules_module::{self, ModuleText};
use super::spawning::{self as sp, Draws, HumanChoice};
use super::world::{PostsWorld, SpawningWorld};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};

pub(crate) fn swap(s: &mut Store) {
    rules_module::swap(s);
    posts_module::swap(s);
    std::mem::swap(&mut sim().store.spawning, &mut s.spawning);
}

fn query(op: SpQueryOp, unit: Option<Ref<Unit>>, class_name: Option<StrId>) -> SpQueryResult {
    import_spawning_query(&SpQueryArgs { op, unit, class_name, character: None })
}

pub(crate) struct JsDraws;
impl Draws for JsDraws {
    fn next(&mut self) -> f64 { query(SpQueryOp::Draw, None, None).number }
}

pub(crate) struct JsSpawningWorld;
impl JsSpawningWorld {
    fn action(&mut self, s: &mut Store, args: SpActionArgs) -> Option<Ref<Unit>> {
        swap(s);
        let result = import_spawning_action(&args);
        swap(s);
        result.unit
    }
}

impl PostsWorld for JsSpawningWorld {
    fn capture_team(&mut self, team: f64) -> f64 { JsPostsWorld.capture_team(team) }
    fn building_alive(&mut self, building: Ref<Building>) -> bool { JsPostsWorld.building_alive(building) }
    fn pad(&mut self, slot: f64) -> Pad { JsPostsWorld.pad(slot) }
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3] { JsPostsWorld.vehicle_position(vehicle) }
    fn class_spawn_world(&mut self, s: &Store, post: Ref<MobilePost>) -> Vec<[f64; 4]> { JsPostsWorld.class_spawn_world(s, post) }
}

impl SpawningWorld for JsSpawningWorld {
    fn spawn_goal(&mut self, character: Ref<Character>) -> Option<[f64; 3]> {
        import_spawning_query(&SpQueryArgs { op: SpQueryOp::SpawnGoal, character: Some(character), unit: None, class_name: None }).goal
    }
    fn health(&mut self, unit: Ref<Unit>) -> f64 { query(SpQueryOp::Health, Some(unit), None).number }
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId> { query(SpQueryOp::HeroName, None, Some(class_name)).name }
    fn force_kill(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) {
        self.action(s, SpActionArgs { op: SpActionOp::ForceKill, character, unit: Some(unit), ..Default::default() });
    }
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>) {
        self.action(s, SpActionArgs { op: SpActionOp::RequestObjective, character, ..Default::default() });
    }
    fn republish(&mut self, s: &mut Store, character: Ref<Character>) {
        self.action(s, SpActionArgs { op: SpActionOp::Republish, character, ..Default::default() });
    }
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        self.action(s, SpActionArgs { op: SpActionOp::SpawnUnit, character, unit: None, class_name, post: post_ref(Some(post)), position: at, yaw })
    }
}

#[no_mangle]
pub extern "C" fn spawning_words(which: u32) -> u32 {
    (match which { 0 => SpInput::WORDS, 1 => SpResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn spawning_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); SpInput::take(row, heap) };
    let mut s = Store::default();
    swap(&mut s);
    let mut world = JsSpawningWorld;
    let mut random = JsDraws;
    let mut out = SpResult::default();
    let r = || input.character.expect("a Character");
    let post = spawn_post(input.post);
    match input.op {
        SpOperation::Update => {
            sp::clock(&mut s, input.dt, &mut world);
            sp::update_characters(&mut s, input.dt, &ModuleText, &mut world, &mut random);
        }
        SpOperation::Clock => sp::clock(&mut s, input.dt, &mut world),
        SpOperation::UpdateCharacters => sp::update_characters(&mut s, input.dt, &ModuleText, &mut world, &mut random),
        SpOperation::UpdateCharacter => sp::update_character(&mut s, r(), input.dt, &ModuleText, &mut world, &mut random),
        SpOperation::AwaitingHuman => out.flag = sp::awaiting_human(s.character.get(r()).unwrap()),
        SpOperation::SetClass => sp::set_class(&mut s, r(), input.class_index.unwrap_or(0.0), &ModuleText, &mut world),
        SpOperation::Activate => sp::activate(&mut s, r(), &ModuleText, &mut world, &mut random),
        SpOperation::PickClass => out.number = sp::pick_class(&s, r(), &mut random),
        SpOperation::Prepare => sp::prepare(&mut s, r(), &ModuleText, &mut world, &mut random),
        SpOperation::ChoosePost => out.post = post_ref(sp::choose_post(&mut s, r(), &mut world, &mut random)),
        SpOperation::Reserve => out.number = sp::reserve(&mut s.spawning, post.expect("a queue's post"), input.human, input.own),
        SpOperation::Release => out.number = sp::release(&mut s.spawning, post.expect("a queue's post"), input.human, input.own),
        SpOperation::ChangePost => sp::change_post(&mut s, r(), post),
        SpOperation::Tickets => out.number = sp::tickets(&s, input.team),
        SpOperation::SpendTicket => sp::spend_ticket(&mut s, input.team),
        SpOperation::Waiting => out.flag = sp::waiting(&s.spawning, s.character.get(r()).unwrap()),
        SpOperation::Spawn => out.unit = sp::spawn(&mut s, r(), &ModuleText, &mut world, &mut random),
        SpOperation::OnDeath => sp::on_death(&mut s, r(), &mut world),
        SpOperation::OnHumanReady => sp::on_human_ready(&mut s, r(), HumanChoice {
            class_name: input.class_name, class_index: input.class_index, post: input.has_post.then_some(post),
        }, &ModuleText, &mut world),
        SpOperation::SwitchTeam => out.flag = sp::switch_team(&mut s, r(), input.team, &ModuleText, &mut world, &mut random),
        SpOperation::PadQuotas => sp::pad_quotas(&mut s.spawning, input.team as usize, &mut random),
        SpOperation::QueueOf => { sp::queue_of(&mut s.spawning, post.expect("a queue's post")); }
    }
    swap(&mut s);
    let sim = sim();
    sim.calls.result(SpResult::WORDS, |row, heap| out.put(row, heap));
    sim.calls.block.ptr()
}
