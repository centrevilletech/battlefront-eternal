//! Native Battle command/publication boundary. The tail and recursive combat
//! services share Core; no Core reference survives a lifecycle or AI hook.
use super::{core::Core, commands::Result};
use crate::{abi::sim, gen::*, store::{Ref, block::HEADER}};
use std::ptr::addr_of_mut;
pub(super) struct State {core:Core,handle:Vec<f64>,capacity:usize,battle_capacity:usize,
    tail:Vec<(F4Handle,usize)>,hero_names:Vec<(crate::store::StrId,Option<crate::store::StrId>)>,classic:Option<bool>,local:Option<Ref<Unit>>}
fn state() -> &'static mut State { unsafe { (*addr_of_mut!(super::owner::BATTLE.core)).as_mut().expect("battle owner initialized") } }
pub(crate) fn reset() { unsafe { *addr_of_mut!(super::owner::BATTLE.core) = None; } }
pub(crate) fn active() -> bool { unsafe { (*addr_of_mut!(super::owner::BATTLE.core)).is_some() } }
pub(crate) fn publication_ptr() -> u32 {
    if !active() { return 0; } state().core.publication.block.ptr() as u32
}
pub(crate) fn publish() {
    if crate::world::deferred() {return;}

    if active() {
        let rows = state().core.flush(&sim().store);
        let words = state().core.publication.block.words.len() - HEADER + state().core.publication.borrowed_words();
        super::f4_live::record_publication(rows,words);
    }
}
#[no_mangle]
pub extern "C" fn f4_battle_init() {
    let (row, heap) = sim().calls.args(); let setup = F4BattleSetup::take(row, heap);
    let capacity = (setup.capacity as usize).max(sim().store.battle.characters.len());
    let mut core = Core::new(&sim().store, capacity);
    let characters = sim().store.battle.characters.clone();
    for (index,r) in characters.into_iter().enumerate() { sim().store.character.get_mut(r).unwrap().roster_slot=Some(index as f64);core.adopt(&mut sim().store, r); }
    let mut row = vec![0.0; Battle::EXPORT_STRIDE]; let mut heap = Vec::new();
    sim().store.battle.put_export(&mut row, &mut heap);
    unsafe { *addr_of_mut!(super::owner::BATTLE.core) = Some(State { core, handle: vec![0.0; F4Handle::WORDS], capacity,
        battle_capacity: HEADER + Battle::EXPORT_STRIDE + heap.len() + capacity * 2 + sim().store.battle.team_ids.len()*4,tail:Vec::new(),hero_names:Vec::new(),classic:None,local:None }); }
}
#[no_mangle]
pub extern "C" fn f4_battle_capacity(character: u32) -> u32 {
    if character == 0 { state().battle_capacity as u32 }
    else { (HEADER + Character::EXPORT_STRIDE + Core::character_heap(state().capacity)) as u32 }
}
#[no_mangle]
pub extern "C" fn f4_character_handle(index: u32) -> *const f64 {
    let r = sim().store.battle.characters[index as usize];
    let handle = state().core.character(r).expect("prepared Character").clone();
    handle.put(&mut state().handle, &mut Vec::new()); state().handle.as_ptr()
}
#[no_mangle]
pub extern "C" fn f4_battle_publish() -> *const f64 {
    publish(); super::f4_live::publish(); super::f4_live::descriptor()
}
#[no_mangle]
pub extern "C" fn f4_battle_command() -> u32 {
    let (row, heap) = sim().calls.args(); let command = F4Command::take(row, heap);
    let outer = crate::units::f3::external();
    super::f4_live::count_command();
    let result = state().core.apply(&mut sim().store, &command);
    if result == Result::Applied {
        if command.op==F4CommandOp::Controller {
            let key=command.unit_key.as_ref().unwrap();crate::units::f3::changed(Ref::new(key.slot,key.generation));
        }
    }
    if !outer.is_outer(){crate::units::f3::publish_changed();}
    drop(outer); u32::from(result == Result::Applied)
}
/// Direct owner services do not need external command handles. The combat
/// service resolved these captured Character refs before pooling can resume.
pub(crate) fn achievement(character: Option<Ref<Character>>, op: F4CommandOp) {
    state().core.achievement(&mut sim().store, character, op);
}

pub(crate) fn prepare_record(s: &mut Store, r: Ref<Character>) { state().core.records.ensure(s,r); state().core.mark(r); }
pub(crate) fn point(s: &mut Store, r: Option<Ref<Character>>) {
    super::rules::point(s,r); if let Some(r) = r { state().core.mark(r); }
}

pub(crate) fn changed_battle() { state().core.mark_battle(); }

pub(crate) fn resolve_character(store:&Store,handle:&F4Handle)->Option<Ref<Character>> { state().resolve_character(store, handle) }
pub(crate) fn mark_character(r:Ref<Character>){state().core.mark(r);}
pub(crate) fn credit(victim:Option<Ref<Character>>,killer:Option<Ref<Character>>,relation:f64){state().core.credit(&mut sim().store,victim,killer,relation);}
pub(crate) fn configure_tail(classic:bool,local:Option<Ref<Unit>>){state().classic=Some(classic);state().local=local;}
pub(crate) fn owns_lifecycle()->bool{active() && state().classic.is_some()}
pub(crate) fn died(unit:Ref<Unit>,killer:Option<Ref<Character>>,relation:f64,services:crate::units::f3::WorldServices<'_>)->Option<Ref<Controller>>{
    let classic=state().classic.expect("prepared battle lifecycle");let local=state().local;
    super::native_lifecycle::died(unit,killer,relation,classic,local,services)
}
pub(crate) fn mark_mobile(){ if active() { state().mark_mobile(); } }
pub(crate) fn mark_tail(s:&Store){ state().mark_tail(s) }
fn register_tail(source:super::publication::Source,words:usize,heap:usize){ state().register_tail(source, words, heap) }
pub(crate) fn prepare_tail_publications(){
    use super::publication::Source;
    assert!(state().tail.is_empty(),"prepare battle publications once");
    let s=&sim().store;let mut heap=Vec::new();
    let mut row=vec![0.0;Post::EXPORT_STRIDE];
    for &r in &s.command_posts.posts{
        heap.clear();s.post.get(r).unwrap().put_export(&mut row,&mut heap);
        register_tail(Source::Post(r),Post::EXPORT_STRIDE,heap.len()+s.battle.team_ids.len()*2);
    }
    row.resize(MobilePost::EXPORT_STRIDE,0.0);
    for &r in &s.mobile_posts.posts{
        let p=s.mobile_post.get(r).unwrap();heap.clear();p.put_export(&mut row,&mut heap);
        let extra=p.ai.atk.capacity()+p.ai.def.capacity()+p.ai.points.capacity()*4;
        register_tail(Source::Mobile(r),MobilePost::EXPORT_STRIDE,heap.len()+extra);
    }
    row.resize(HumanSpawn::EXPORT_STRIDE,0.0);heap.clear();s.human_spawn.put_export(&mut row,&mut heap);
    register_tail(Source::Human,HumanSpawn::EXPORT_STRIDE,heap.len());
    row.resize(Spawning::EXPORT_STRIDE,0.0);heap.clear();s.spawning.put_export(&mut row,&mut heap);
    let posts=s.command_posts.posts.len()+s.mobile_posts.posts.len();
    // Cold native roster setup can fill both the manager and each team's
    // Character lists after this reservation, plus all eight class tables.
    let cold_teams=super::spawning::TEAM_COUNT*(SpawnTeam::WORDS+super::spawning::CLASS_CAPACITY*SpawnClass::WORDS);
    register_tail(Source::Spawning,Spawning::EXPORT_STRIDE,heap.len()+posts*(SpawnPost::WORDS+4)+state().capacity*4+cold_teams);
}
fn tail_handle(kind:u32,index:u32)->F4Handle{
    use super::publication::Source;
    let source=match F4ObjectKind::from_u8(kind as u8).expect("battle publication kind"){
        F4ObjectKind::Post=>Source::Post(sim().store.command_posts.posts[index as usize]),
        F4ObjectKind::MobilePost=>Source::Mobile(sim().store.mobile_posts.posts[index as usize]),
        F4ObjectKind::Human=>Source::Human,F4ObjectKind::Spawning=>Source::Spawning,_=>panic!("battle tail publication"),
    };
    state().core.publication.handle(source).expect("prepared battle publication").clone()
}
#[no_mangle]
pub extern "C" fn f4_tail_handle(kind:u32,index:u32)->*const f64{
    tail_handle(kind,index).put(&mut state().handle,&mut Vec::new());state().handle.as_ptr()
}
#[no_mangle]
pub extern "C" fn f4_tail_capacity(kind:u32,index:u32)->u32{
    let h=tail_handle(kind,index);state().tail.iter().find(|(k,_)|*k==h).unwrap().1 as u32
}

// SnapshotHost's roster position is canonical Character data. Admission may
// change the list; publication must precede any host reading its slot number.
pub(crate) fn roster_slots(s:&mut Store){
    for index in 0..s.battle.characters.len(){
        let r=s.battle.characters[index];let c=s.character.get_mut(r).unwrap();
        if c.roster_slot!=Some(index as f64){c.roster_slot=Some(index as f64);state().core.mark(r);}
    }
}
pub(crate) fn character_handle(r:Ref<Character>)->Option<F4Handle>{state().core.character(r).cloned()}
pub(crate) fn resolve(s:&Store,h:&F4Handle)->Option<Ref<Character>>{state().core.resolve(s,h)}
pub(crate) fn adopt(s:&mut Store,r:Ref<Character>){
    let handle=state().core.adopt(s,r);
    // Native AI hooks no longer enter JS while a new roster member is
    // attached. Admit its facade before sparse Battle/Spawning references.
    handle.emit(&mut sim().events);
}
pub(crate) fn reassign(s:&Store,r:Ref<Character>){state().core.reassign(s,r);}
pub(crate) fn recycle(s:&mut Store,r:Ref<Character>){state().core.reset_record(s,r);}

// Readiness may reenter while the tail lends its prepared world to a hook.
// Keep this small immutable naming table available without borrowing it.
pub(crate) fn prepare_hero(class:crate::store::StrId,name:Option<crate::store::StrId>){if let Some(row)=state().hero_names.iter_mut().find(|(key,_)|*key==class){row.1=name;}else{state().hero_names.push((class,name));}}
/// AI and network readiness cross once as a qualified scalar choice. The
/// native queue decides whether this Character is awaiting a human choice.
#[no_mangle]
pub extern "C" fn f4_human_ready() -> u32 {
    let (row,heap)=sim().calls.args();let command=F4HumanReady::take(row,heap);
    super::f4_live::count_external(F4HumanReady::WORDS);
    if state().classic!=Some(true) {return 0;}
    let Some(character)=state().core.resolve(&sim().store,&command.target) else{return 0};
    let c=sim().store.character.get(character).unwrap();
    if c.spawn.is_none() || !super::spawning::awaiting_human(c) {return 0;}
    let post=if command.supplied_post {
        match command.post {
            None=>Some(None),
            Some(h)=>{
                if h.life!=1.0 {return 0;}
                let post=match h.kind {
                    F4ObjectKind::Post=>{let r=crate::store::Ref::new(h.slot,h.generation);if !sim().store.post.contains(r){return 0;}SpawnPost::Post(r)},
                    F4ObjectKind::MobilePost=>{let r=crate::store::Ref::new(h.slot,h.generation);if !sim().store.mobile_post.contains(r){return 0;}SpawnPost::MobilePost(r)},
                    _=>return 0,
                };Some(Some(post))
            }
        }
    } else {None};
    let outer=crate::units::f3::external();
    super::spawning::on_human_ready_named(&mut sim().store,character,super::spawning::HumanChoice{
        class_name:command.class_name,class_index:command.class_index,post,
    },&ReadyText,|class|state().hero_names.iter().find(|(name,_)|*name==class).and_then(|(_,name)|*name));
    mark_tail(&sim().store);
    if !outer.is_outer(){crate::units::f3::publish_changed();}
    drop(outer);1
}
struct ReadyText;
impl super::stats::Text for ReadyText {
    fn nonempty(&self,id:crate::store::StrId)->bool {!sim().strings.units(id).is_empty()}
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
