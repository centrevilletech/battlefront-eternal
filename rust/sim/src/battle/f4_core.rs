//! Native Battle command/publication boundary. The tail and recursive combat
//! services share Core; no Core reference survives a lifecycle or AI hook.
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use super::{core::Core, commands::Result};
use crate::{ gen::*, store::{Ref, block::HEADER}};

pub(super) struct State {core:Core,handle:Vec<f64>,capacity:usize,battle_capacity:usize,
    tail:Vec<(F4Handle,usize)>,hero_names:Vec<(crate::store::StrId,Option<crate::store::StrId>)>,classic:Option<bool>,local:Option<Ref<Unit>>}
fn state(battle: &mut BattleOwner) -> &mut State { battle.core.as_mut().expect("battle owner initialized") }
pub(crate) fn reset(battle: &mut BattleOwner) { battle.core = None; }
pub(crate) fn active_in(battle: &mut BattleOwner) -> bool { battle.core.is_some() }
#[cfg(target_arch = "wasm32")]
pub(crate) fn publication_ptr(battle: &mut BattleOwner) -> u32 {
    if !active_in(battle) { return 0; } state(battle).core.publication.block.ptr() as u32
}
pub(crate) fn publish_in(w: &mut Root) {
    if crate::world::deferred_in(&w.tick) {return;}

    if active_in(&mut w.battle) {
        let rows = state(&mut w.battle).core.flush(&w.sim.store);
        let words = state(&mut w.battle).core.publication.block.words.len() - HEADER + state(&mut w.battle).core.publication.borrowed_words();
        super::f4_live::record_publication(&mut w.battle, rows,words);
    }
}
pub fn f4_battle_init_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args(); let setup = F4BattleSetup::take(row, heap);
    let capacity = (setup.capacity as usize).max(w.sim.store.battle.characters.len());
    let mut core = Core::new(&w.sim.store, capacity);
    let characters = w.sim.store.battle.characters.clone();
    for (index,r) in characters.into_iter().enumerate() { w.sim.store.character.get_mut(r).unwrap().roster_slot=Some(index as f64);core.adopt(&mut w.sim.store, r); }
    let mut row = vec![0.0; Battle::EXPORT_STRIDE]; let mut heap = Vec::new();
    w.sim.store.battle.put_export(&mut row, &mut heap);
    { w.battle.core = Some(State { core, handle: vec![0.0; F4Handle::WORDS], capacity,
        battle_capacity: HEADER + Battle::EXPORT_STRIDE + heap.len() + capacity * 2 + w.sim.store.battle.team_ids.len()*4,tail:Vec::new(),hero_names:Vec::new(),classic:None,local:None }); }
}
pub fn f4_battle_capacity_in(w: &mut Root, character: u32) -> u32 {
    if character == 0 { state(&mut w.battle).battle_capacity as u32 }
    else { (HEADER + Character::EXPORT_STRIDE + Core::character_heap(state(&mut w.battle).capacity)) as u32 }
}
pub fn f4_character_handle_in(w: &mut Root, index: u32) -> &[f64] {
    let r = w.sim.store.battle.characters[index as usize];
    let handle = state(&mut w.battle).core.character(r).expect("prepared Character").clone();
    handle.put(&mut state(&mut w.battle).handle, &mut Vec::new()); &state(&mut w.battle).handle[..]
}
pub fn f4_battle_publish_in(w: &mut Root) -> Option<&[f64]> {
    publish_in(w); super::f4_live::publish_in(w); super::f4_live::descriptor_words(w)
}
pub fn f4_battle_command_in(w: &mut Root) -> u32 {
    let (row, heap) = w.sim.calls.args(); let command = F4Command::take(row, heap);
    let outer = crate::units::f3::external_in(w);
    super::f4_live::count_command(&mut w.battle);
    let result = state(&mut w.battle).core.apply(&mut w.sim.store, &command);
    if result == Result::Applied {
        if command.op==F4CommandOp::Controller {
            let key=command.unit_key.as_ref().unwrap();crate::units::f3::changed_in(w, Ref::new(key.slot,key.generation));
        }
    }
    if !outer.is_outer(){crate::units::f3::publish_changed_in(w);}
    outer.finish(w, true); u32::from(result == Result::Applied)
}
/// Direct owner services do not need external command handles. The combat
/// service resolved these captured Character refs before pooling can resume.
pub(crate) fn achievement_in(w: &mut Root, character: Option<Ref<Character>>, op: F4CommandOp) {
    state(&mut w.battle).core.achievement(&mut w.sim.store, character, op);
}

pub(crate) fn prepare_record(battle: &mut BattleOwner, s: &mut Store, r: Ref<Character>) { state(battle).core.records.ensure(s,r); state(battle).core.mark(r); }
pub(crate) fn point(battle: &mut BattleOwner, s: &mut Store, r: Option<Ref<Character>>) {
    super::rules::point(s,r); if let Some(r) = r { state(battle).core.mark(r); }
}

pub(crate) fn changed_battle(battle: &mut BattleOwner) { state(battle).core.mark_battle(); }

/// AI observations validate the same retained command life as battle commands.
pub(crate) fn resolve_character_ref(battle: &BattleOwner, store: &Store, handle: &F4Handle) -> Option<Ref<Character>> {
    battle.core.as_ref().expect("battle owner initialized").resolve_character(store, handle)
}

pub(crate) fn resolve_character_in(battle: &mut BattleOwner, store:&Store,handle:&F4Handle)->Option<Ref<Character>> { state(battle).resolve_character(store, handle) }
pub(crate) fn mark_character_in(battle: &mut BattleOwner, r:Ref<Character>){state(battle).core.mark(r);}
pub(crate) fn credit(w: &mut Root, victim:Option<Ref<Character>>,killer:Option<Ref<Character>>,relation:f64){state(&mut w.battle).core.credit(&mut w.sim.store,victim,killer,relation);}
pub(crate) fn configure_tail(battle: &mut BattleOwner, classic:bool,local:Option<Ref<Unit>>){state(battle).classic=Some(classic);state(battle).local=local;}
pub(crate) fn owns_lifecycle_in(battle: &mut BattleOwner)->bool{active_in(battle) && state(battle).classic.is_some()}
pub(crate) fn died_in(w: &mut Root, unit:Ref<Unit>,killer:Option<Ref<Character>>,relation:f64,services:crate::units::f3::WorldServices<'_>)->Option<Ref<Controller>>{
    let classic=state(&mut w.battle).classic.expect("prepared battle lifecycle");let local=state(&mut w.battle).local;
    super::native_lifecycle::died(w, unit,killer,relation,classic,local,services)
}
pub(crate) fn mark_mobile(battle: &mut BattleOwner){ if active_in(battle) { state(battle).mark_mobile(); } }
pub(crate) fn mark_tail_in(battle: &mut BattleOwner, s:&Store){ state(battle).mark_tail(s) }
fn register_tail(battle: &mut BattleOwner, source:super::publication::Source,words:usize,heap:usize){ state(battle).register_tail(source, words, heap) }
pub(crate) fn prepare_tail_publications(w: &mut Root){
    use super::publication::Source;
    assert!(state(&mut w.battle).tail.is_empty(),"prepare battle publications once");
    let s=&w.sim.store;let mut heap=Vec::new();
    let mut row=vec![0.0;Post::EXPORT_STRIDE];
    for &r in &s.command_posts.posts{
        heap.clear();s.post.get(r).unwrap().put_export(&mut row,&mut heap);
        register_tail(&mut w.battle, Source::Post(r),Post::EXPORT_STRIDE,heap.len()+s.battle.team_ids.len()*2);
    }
    row.resize(MobilePost::EXPORT_STRIDE,0.0);
    for &r in &s.mobile_posts.posts{
        let p=s.mobile_post.get(r).unwrap();heap.clear();p.put_export(&mut row,&mut heap);
        let extra=p.ai.atk.capacity()+p.ai.def.capacity()+p.ai.points.capacity()*4;
        register_tail(&mut w.battle, Source::Mobile(r),MobilePost::EXPORT_STRIDE,heap.len()+extra);
    }
    row.resize(HumanSpawn::EXPORT_STRIDE,0.0);heap.clear();s.human_spawn.put_export(&mut row,&mut heap);
    register_tail(&mut w.battle, Source::Human,HumanSpawn::EXPORT_STRIDE,heap.len());
    row.resize(Spawning::EXPORT_STRIDE,0.0);heap.clear();s.spawning.put_export(&mut row,&mut heap);
    let posts=s.command_posts.posts.len()+s.mobile_posts.posts.len();
    // Cold native roster setup can fill both the manager and each team's
    // Character lists after this reservation, plus all eight class tables.
    let cold_teams=super::spawning::TEAM_COUNT*(SpawnTeam::WORDS+super::spawning::CLASS_CAPACITY*SpawnClass::WORDS);
    let capacity=state(&mut w.battle).capacity;
    register_tail(&mut w.battle, Source::Spawning,Spawning::EXPORT_STRIDE,heap.len()+posts*(SpawnPost::WORDS+4)+capacity*4+cold_teams);
}
fn tail_handle(w: &mut Root, kind:u32,index:u32)->F4Handle{
    use super::publication::Source;
    let source=match F4ObjectKind::from_u8(kind as u8).expect("battle publication kind"){
        F4ObjectKind::Post=>Source::Post(w.sim.store.command_posts.posts[index as usize]),
        F4ObjectKind::MobilePost=>Source::Mobile(w.sim.store.mobile_posts.posts[index as usize]),
        F4ObjectKind::Human=>Source::Human,F4ObjectKind::Spawning=>Source::Spawning,_=>panic!("battle tail publication"),
    };
    state(&mut w.battle).core.publication.handle(source).expect("prepared battle publication").clone()
}
pub fn f4_tail_handle_in(w: &mut Root, kind:u32,index:u32)->&[f64]{
    tail_handle(w, kind,index).put(&mut state(&mut w.battle).handle,&mut Vec::new());&state(&mut w.battle).handle[..]
}
pub fn f4_tail_capacity_in(w: &mut Root, kind:u32,index:u32)->u32{
    let h=tail_handle(w, kind,index);state(&mut w.battle).tail.iter().find(|(k,_)|*k==h).unwrap().1 as u32
}

// SnapshotHost's roster position is canonical Character data. Admission may
// change the list; publication must precede any host reading its slot number.
pub(crate) fn roster_slots_in(battle: &mut BattleOwner, s:&mut Store){
    for index in 0..s.battle.characters.len(){
        let r=s.battle.characters[index];let c=s.character.get_mut(r).unwrap();
        if c.roster_slot!=Some(index as f64){c.roster_slot=Some(index as f64);state(battle).core.mark(r);}
    }
}
pub(crate) fn character_handle_in(battle: &mut BattleOwner, r:Ref<Character>)->Option<F4Handle>{state(battle).core.character(r).cloned()}
pub(crate) fn resolve(battle: &mut BattleOwner, s:&Store,h:&F4Handle)->Option<Ref<Character>>{state(battle).core.resolve(s,h)}
pub(crate) fn adopt_in(w: &mut Root, s:&mut Store,r:Ref<Character>){
    adopt_character_in(&mut w.battle,s,&mut w.sim.events,r);
}
/// Cold roster admission borrows the committed slabs beside their journal.
pub(crate) fn adopt_character_in(battle:&mut BattleOwner,s:&mut Store,events:&mut crate::store::calls::Events,r:Ref<Character>){
    let handle=state(battle).core.adopt(s,r);
    handle.emit(events);
}
pub(crate) fn reassign(battle: &mut BattleOwner, s:&Store,r:Ref<Character>){state(battle).core.reassign(s,r);}
pub(crate) fn recycle(battle: &mut BattleOwner, s:&mut Store,r:Ref<Character>){state(battle).core.reset_record(s,r);}

// Readiness may reenter while the tail lends its prepared world to a hook.
// Keep this small immutable naming table available without borrowing it.
pub(crate) fn prepare_hero(battle: &mut BattleOwner, class:crate::store::StrId,name:Option<crate::store::StrId>){if let Some(row)=state(battle).hero_names.iter_mut().find(|(key,_)|*key==class){row.1=name;}else{state(battle).hero_names.push((class,name));}}
/// AI and network readiness cross once as a qualified scalar choice. The
/// native queue decides whether this Character is awaiting a human choice.
pub fn f4_human_ready_in(w: &mut Root) -> u32 {
    let (row,heap)=w.sim.calls.args();let command=F4HumanReady::take(row,heap);
    super::f4_live::count_external(&mut w.battle, F4HumanReady::WORDS);
    if state(&mut w.battle).classic!=Some(true) {return 0;}
    let Some(character)=state(&mut w.battle).core.resolve(&w.sim.store,&command.target) else{return 0};
    let c=w.sim.store.character.get(character).unwrap();
    if c.spawn.is_none() || !super::spawning::awaiting_human(c) {return 0;}
    let post=if command.supplied_post {
        match command.post {
            None=>Some(None),
            Some(h)=>{
                if h.life!=1.0 {return 0;}
                let post=match h.kind {
                    F4ObjectKind::Post=>{let r=crate::store::Ref::new(h.slot,h.generation);if !w.sim.store.post.contains(r){return 0;}SpawnPost::Post(r)},
                    F4ObjectKind::MobilePost=>{let r=crate::store::Ref::new(h.slot,h.generation);if !w.sim.store.mobile_post.contains(r){return 0;}SpawnPost::MobilePost(r)},
                    _=>return 0,
                };Some(Some(post))
            }
        }
    } else {None};
    let outer=crate::units::f3::external_in(w);
    super::spawning::on_human_ready_named(&mut w.sim.store,character,super::spawning::HumanChoice{
        class_name:command.class_name,class_index:command.class_index,post,
    },&w.sim.strings,|class|state(&mut w.battle).hero_names.iter().find(|(name,_)|*name==class).and_then(|(_,name)|*name));
    mark_tail_in(&mut w.battle, &w.sim.store);
    if !outer.is_outer(){crate::units::f3::publish_changed_in(w);}
    outer.finish(w, true);1
}

impl State {
    fn mark_mobile(&mut self) {
        let state=self;
        for (h,_) in &state.tail{if h.kind==F4ObjectKind::MobilePost{state.core.publication.mark(h);}}
    }
    fn mark_tail(&mut self, s: &Store) {
        let state=self;state.core.mark_battle();
        for &r in &s.battle.characters{state.core.mark(r);}
        for (h,_) in &state.tail{state.core.publication.mark(h);}
    }
    fn resolve_character(&self, store: &Store, handle: &F4Handle) -> Option<Ref<Character>> {
        if handle.kind!=F4ObjectKind::Character {return None;}
        let reference=Ref::new(handle.slot,handle.generation);
        (store.character.contains(reference)&&self.core.character(reference)==Some(handle)).then_some(reference)
    }
    fn register_tail(&mut self, source: super::publication::Source, words: usize, heap: usize) {
        let h=self.core.publication.register(source,1.0,heap);self.tail.push((h,HEADER+words+heap));
    }
}

// facade shims
#[cfg(target_arch = "wasm32")]
pub(crate) fn active() -> bool { active_in(&mut crate::abi::cold().battle) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn publish() { publish_in(crate::abi::world()) }





#[cfg(target_arch = "wasm32")]
pub(crate) fn achievement(character: Option<Ref<Character>>, op: F4CommandOp) { achievement_in(crate::abi::world(), character, op) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn mark_character(r:Ref<Character>) { mark_character_in(&mut crate::abi::cold().battle, r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn owns_lifecycle() ->bool { owns_lifecycle_in(&mut crate::abi::cold().battle) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn died(unit:Ref<Unit>,killer:Option<Ref<Character>>,relation:f64,services:crate::units::f3::WorldServices<'_>) ->Option<Ref<Controller>> { died_in(crate::abi::world(), unit, killer, relation, services) }




#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn f4_battle_init() { f4_battle_init_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_battle_capacity(character: u32) -> u32 { f4_battle_capacity_in(crate::abi::facade::world(), character) }
    #[no_mangle]
    pub extern "C" fn f4_character_handle(index: u32) -> *const f64 { f4_character_handle_in(crate::abi::facade::world(), index).as_ptr() }
    #[no_mangle]
    pub extern "C" fn f4_battle_publish() -> *const f64 { f4_battle_publish_in(crate::abi::facade::world()).map_or(std::ptr::null(), |bank| bank.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_battle_command() -> u32 { f4_battle_command_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_handle(kind:u32,index:u32) ->*const f64 { f4_tail_handle_in(crate::abi::facade::world(), kind, index).as_ptr() }
    #[no_mangle]
    pub extern "C" fn f4_tail_capacity(kind:u32,index:u32) ->u32 { f4_tail_capacity_in(crate::abi::facade::world(), kind, index) }
    #[no_mangle]
    pub extern "C" fn f4_human_ready() -> u32 { f4_human_ready_in(crate::abi::facade::world()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{f4_battle_init, f4_battle_capacity, f4_character_handle, f4_battle_publish, f4_battle_command, f4_tail_handle, f4_tail_capacity, f4_human_ready};
