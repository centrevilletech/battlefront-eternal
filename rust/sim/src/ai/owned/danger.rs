//! grenadeUpdate keeps warning state on each projectile object, independently
//! of its thrower's next body life. Callers validate canonical projectile
//! references before entering; storage reuse then renews the warning state.
use crate::{ai::combat::perception::DangerState,gen::Grenade,store::Ref};
#[derive(Clone,Copy)]
struct Entry {reference:Ref<Grenade>,contact:f64,state:DangerState}
#[derive(Default)]
pub struct Dangers {rows:Vec<Option<Entry>>}
impl Dangers {
    /// The native ordnance admission limit reserves this table during load.
    /// A first contact or an airborne warning must not allocate a map entry.
    pub fn reserve(&mut self,capacity:usize) {self.rows.resize(capacity.max(self.rows.len()),None);}
    fn entry(&mut self,reference:Ref<Grenade>)->Option<&mut Entry> {
        let row=self.rows.get_mut(reference.slot as usize)?;
        if row.is_none_or(|row|row.reference!=reference) {*row=Some(Entry {reference,contact:f64::INFINITY,state:DangerState::default()});}
        row.as_mut()
    }
    /// Game.onGrenadeContact writes the latest accepted contact. The later
    /// warning phase retains its minimum, as combat.grenadeUpdate does.
    pub fn contact(&mut self,reference:Ref<Grenade>,time:f64) {if let Some(row)=self.entry(reference) {row.contact=time;}}
    pub fn begin(&mut self,reference:Ref<Grenade>)->(DangerState,f64) {self.entry(reference).map_or((DangerState::default(),f64::INFINITY),|row|(row.state,row.contact))}
    pub fn finish(&mut self,reference:Ref<Grenade>,state:DangerState) {
        // Admission reserves all normal projectile slots. A malformed or excess
        // reference must still draw its delay without stopping the world.
        if let Some(row)=self.rows.get_mut(reference.slot as usize).and_then(Option::as_mut).filter(|row|row.reference==reference) {row.state=state;}
    }
}
