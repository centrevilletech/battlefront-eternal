//! MultiplayerWorld's retained connection spawn state. Mailboxes remain at
//! the input boundary; eligibility, body realization and dead-body clocks
//! read canonical Characters and Units before Game.step begins.
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use crate::{gen::*,store::{block::Block,Ref},units::f3};

struct Entry {handle:F4Handle,ordinal:f64,view:F4ConnectionView}
pub(super) struct State {rows:Vec<Option<Entry>>,output:Block,result:F4ConnectionView}
impl State {
    fn publish(&mut self)->&[f64] {
        self.output.begin(1,F4ConnectionView::WORDS);
        let (row,heap)=self.output.row(0);self.result.put(row,heap);
        self.output.finish();&self.output.words
    }
}
pub(crate) fn reset(battle: &mut BattleOwner){{battle.connections=None;}}
fn state(battle: &mut BattleOwner)->&mut State {battle.connections.as_mut().expect("connection owner prepared")}
pub fn f4_connections_init_in(w: &mut Root, capacity:u32) {
    let outer=f3::external_in(w);
    let human=&mut w.sim.store.human_spawn;human.auto=None;human.pending=None;human.spawned_once=true;
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    let mut output=Block::default();output.words.reserve(crate::store::block::HEADER+F4ConnectionView::WORDS);
    {w.battle.connections=Some(State{rows:(0..capacity).map(|_|None).collect(),output,result:F4ConnectionView::default()});}
    outer.finish(w, true);
}
/// attachedPlayers qualifies the connection, not merely a recycled roster
/// index. A released Character's old link facts cannot mark its bot reuse as
/// a human while the next transport publication is still queued.
pub(crate) fn wire_character_in(battle: &BattleOwner, store: &Store, handle: &F4Handle) -> Option<crate::store::Ref<Character>> {
    let reference = super::f4_core::resolve_character_ref(battle, store, handle)?;
    let rows = battle.connections.as_ref().expect("prepared connections"); let entry = rows.rows.get(reference.slot as usize)?.as_ref()?;
    (entry.handle == *handle).then_some(reference)
}
/// The connection owns its retained dead/unspawned body and spawn phase.
/// A Character's current bot body is not a substitute after disconnection.
pub(crate) fn wire_view_in<'w>(battle: &'w BattleOwner, store: &Store, handle: &F4Handle) -> Option<&'w F4ConnectionView> {
    let reference = wire_character_in(battle, store, handle)?;
    battle.connections.as_ref().expect("prepared connections").view(reference)
}

fn spawn(w: &mut Root, entry:&mut Entry,character:Ref<Character>,class:Option<crate::store::StrId>,post:Option<crate::store::StrId>,services:f3::WorldServices<'_>)->bool {
    if w.sim.store.battle.game_over {return false;}
    if entry.view.unit.and_then(|r|w.sim.store.unit.get(r)).is_some_and(|u|!u.alive && u.dead_time+1e-6<4.) {return false;}
    let Some(class)=class.or(entry.view.class_name) else{return false};
    let Some((unit,post))=super::f4_tail::connection_spawn(w, character,entry.view.unit,class,post,entry.ordinal,services) else{return false};
    entry.view.unit=Some(unit);entry.view.class_name=Some(class);entry.view.post=Some(post);
    entry.view.phase=HumanPhase::Alive;entry.view.spawned_once=true;true
}
pub fn f4_connection_command_in(w: &mut Root)-> Option<&[f64]> {
    let (row,heap)=w.sim.calls.args();let input=F4ConnectionCommand::take(row,heap);
    super::f4_live::count_external(&mut w.battle, F4ConnectionCommand::WORDS);
    let character=super::f4_core::resolve(&mut w.battle, &w.sim.store,&input.target);
    let prepared = {
        let owner=state(&mut w.battle);owner.result.accepted=false;
        character.and_then(|character| {
            // Refuse rows beyond the retained connection capacity.
            let index=character.slot as usize;
            if index>=owner.rows.len() { return None; }
            if input.op==F4ConnectionOp::Admit {
                owner.rows[index]=Some(Entry{handle:input.target.clone(),ordinal:input.ordinal,view:F4ConnectionView{class_name:input.class_name,..Default::default()}});
            }
            if !owner.rows[index].as_ref().is_some_and(|entry|entry.handle==input.target) { return None; }
            Some((character,index,owner.rows[index].take().unwrap()))
        })
    };
    let Some((character,index,mut entry))=prepared else {return Some(state(&mut w.battle).publish())};
    let outer=f3::external_in(w);
    f3::with_services_in(w, |w, services| {
        let mut services=services.expect("canonical connection body services");
        entry.view.accepted=match input.op {
            F4ConnectionOp::Admit=>true,
            F4ConnectionOp::Spawn=>spawn(w, &mut entry,character,input.class_name,input.post,services.reborrow()),
            F4ConnectionOp::Unspawn=>{
                if let Some(unit)=entry.view.unit {
                    f3::unspawn_native_in(w, unit);
                    let team=w.sim.store.character.get(character).unwrap().team;
                    if let Some(team)=w.sim.store.spawning.teams.iter_mut().find(|row|row.index==team) {team.living-=1.;}
                    entry.view.phase=HumanPhase::Unspawned;entry.view.spawned_once=false;entry.view.post=None;entry.view.dead_time=4.;
                }true
            },
            F4ConnectionOp::Team=>{
                if let Some(unit)=entry.view.unit {
                    let team=w.sim.store.character.get(character).unwrap().team;
                    f3::with_owner_rows(w,|store,owner| {
                        let key=F3UnitKey::of(unit,store.unit.get(unit).unwrap());owner.team(store,&key,team).expect("current connection body");
                    });
                }true
            },
            F4ConnectionOp::State=>{
                if let Some(unit)=entry.view.unit {
                    f3::advance_removed_clock_in(w, unit,1./60.);
                    let u=w.sim.store.unit.get(unit).unwrap();
                    if !u.alive && input.auto_respawn && u.dead_time+1e-6>=4. {let post=entry.view.post;spawn(w, &mut entry,character,None,post,services.reborrow());}
                    let u=w.sim.store.unit.get(entry.view.unit.unwrap()).unwrap();
                    if !u.alive {
                        entry.view.phase=if !entry.view.spawned_once {HumanPhase::Unspawned} else if u.dead_time+1e-6>=4. {HumanPhase::Awaiting} else {HumanPhase::Dying};
                        entry.view.dead_time=u.dead_time;
                    }
                }true
            },
        };
        entry.view.ended=w.sim.store.battle.game_over;
        super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    });
    let result=entry.view.clone();
    state(&mut w.battle).rows[index]=Some(entry);
    outer.finish(w, true);
    let owner=state(&mut w.battle);owner.result=result;Some(owner.publish())
}

impl State {
    fn view(&self, reference: Ref<Character>) -> Option<&F4ConnectionView> {
        Some(&self.rows.get(reference.slot as usize)?.as_ref()?.view)
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;

    #[no_mangle]
    pub extern "C" fn f4_connections_init(capacity:u32) { f4_connections_init_in(crate::abi::facade::world(), capacity) }
    #[no_mangle]
    pub extern "C" fn f4_connection_command() ->*const f64 { f4_connection_command_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
