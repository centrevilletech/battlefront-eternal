//! MultiplayerWorld's retained connection spawn state. Mailboxes remain at
//! the input boundary; eligibility, body realization and dead-body clocks
//! read canonical Characters and Units before Game.step begins.
use crate::{abi::sim,gen::*,store::{block::Block,Ref},units::f3};
use std::ptr::addr_of_mut;
struct Entry {handle:F4Handle,ordinal:f64,view:F4ConnectionView}
struct State {rows:Vec<Option<Entry>>,output:Block,result:F4ConnectionView}
impl State {
    fn publish(&mut self)->*const f64 {
        self.output.begin(1,F4ConnectionView::WORDS);
        let (row,heap)=self.output.row(0);self.result.put(row,heap);
        self.output.finish();self.output.ptr()
    }
}
static mut STATE:Option<State>=None;
pub(crate) fn reset(){unsafe{*addr_of_mut!(STATE)=None;}}
fn state()->&'static mut State {unsafe{(*addr_of_mut!(STATE)).as_mut().expect("connection owner prepared")}}
#[no_mangle]
pub extern "C" fn f4_connections_init(capacity:u32) {
    let outer=f3::external();
    let human=&mut sim().store.human_spawn;human.auto=None;human.pending=None;human.spawned_once=true;
    super::f4_core::mark_tail(&sim().store);
    let mut output=Block::default();output.words.reserve(crate::store::block::HEADER+F4ConnectionView::WORDS);
    unsafe{*addr_of_mut!(STATE)=Some(State{rows:(0..capacity).map(|_|None).collect(),output,result:F4ConnectionView::default()});}
    drop(outer);
}
/// attachedPlayers qualifies the connection, not merely a recycled roster
/// index. A released Character's old link facts cannot mark its bot reuse as
/// a human while the next transport publication is still queued.
pub(crate) fn wire_character(store: &Store, handle: &F4Handle) -> Option<crate::store::Ref<Character>> {
    let reference = super::f4_core::resolve(store, handle)?;
    let rows = state(); let entry = rows.rows.get(reference.slot as usize)?.as_ref()?;
    (entry.handle == *handle).then_some(reference)
}
/// The connection owns its retained dead/unspawned body and spawn phase.
/// A Character's current bot body is not a substitute after disconnection.
pub(crate) fn wire_view(store: &Store, handle: &F4Handle) -> Option<&'static F4ConnectionView> {
    let reference = wire_character(store, handle)?;
    Some(&state().rows.get(reference.slot as usize)?.as_ref()?.view)
}

fn spawn(entry:&mut Entry,character:Ref<Character>,class:Option<crate::store::StrId>,post:Option<crate::store::StrId>,services:f3::WorldServices<'_>)->bool {
    if sim().store.battle.game_over {return false;}
    if entry.view.unit.and_then(|r|sim().store.unit.get(r)).is_some_and(|u|!u.alive && u.dead_time+1e-6<4.) {return false;}
    let Some(class)=class.or(entry.view.class_name) else{return false};
    let Some((unit,post))=super::f4_tail::connection_spawn(character,entry.view.unit,class,post,entry.ordinal,services) else{return false};
    entry.view.unit=Some(unit);entry.view.class_name=Some(class);entry.view.post=Some(post);
    entry.view.phase=HumanPhase::Alive;entry.view.spawned_once=true;true
}
#[no_mangle]
pub extern "C" fn f4_connection_command()->*const f64 {
    let (row,heap)=sim().calls.args();let input=F4ConnectionCommand::take(row,heap);
    super::f4_live::count_external(F4ConnectionCommand::WORDS);
    let character=super::f4_core::resolve(&sim().store,&input.target);
    let owner=state();owner.result.accepted=false;
    let Some(character)=character else{return owner.publish()};
    // Qualified Characters can outlive a smaller connection owner. Refuse
    // admission without indexing its rows or changing an existing connection.
    let index=character.slot as usize;if index>=owner.rows.len(){return owner.publish();}
    if input.op==F4ConnectionOp::Admit {
        owner.rows[index]=Some(Entry{handle:input.target.clone(),ordinal:input.ordinal,view:F4ConnectionView{class_name:input.class_name,..Default::default()}});
    }
    let Some(entry)=owner.rows[index].as_mut().filter(|entry|entry.handle==input.target) else{return owner.publish()};
    let outer=f3::external();
    f3::with_services(|services| {
        let mut services=services.expect("canonical connection body services");
        entry.view.accepted=match input.op {
            F4ConnectionOp::Admit=>true,
            F4ConnectionOp::Spawn=>spawn(entry,character,input.class_name,input.post,services.reborrow()),
            F4ConnectionOp::Unspawn=>{
                if let Some(unit)=entry.view.unit {
                    f3::unspawn_native(unit);
                    let team=sim().store.character.get(character).unwrap().team;
                    if let Some(team)=sim().store.spawning.teams.iter_mut().find(|row|row.index==team) {team.living-=1.;}
                    entry.view.phase=HumanPhase::Unspawned;entry.view.spawned_once=false;entry.view.post=None;entry.view.dead_time=4.;
                }true
            },
            F4ConnectionOp::Team=>{
                if let Some(unit)=entry.view.unit {
                    let team=sim().store.character.get(character).unwrap().team;
                    f3::with_rows(|store,owner| {
                        let key=F3UnitKey::of(unit,store.unit.get(unit).unwrap());owner.team(store,&key,team).expect("current connection body");
                    });
                }true
            },
            F4ConnectionOp::State=>{
                if let Some(unit)=entry.view.unit {
                    f3::advance_removed_clock(unit,1./60.);
                    let u=sim().store.unit.get(unit).unwrap();
                    if !u.alive && input.auto_respawn && u.dead_time+1e-6>=4. {spawn(entry,character,None,entry.view.post,services.reborrow());}
                    let u=sim().store.unit.get(entry.view.unit.unwrap()).unwrap();
                    if !u.alive {
                        entry.view.phase=if !entry.view.spawned_once {HumanPhase::Unspawned} else if u.dead_time+1e-6>=4. {HumanPhase::Awaiting} else {HumanPhase::Dying};
                        entry.view.dead_time=u.dead_time;
                    }
                }true
            },
        };
        entry.view.ended=sim().store.battle.game_over;
        super::f4_core::mark_tail(&sim().store);
    });
    drop(outer);
    owner.result=entry.view.clone();owner.publish()
}
