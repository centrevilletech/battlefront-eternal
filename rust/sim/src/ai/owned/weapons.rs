//! AI weapon edits target F3's canonical combat rows. These methods leave
//! timers, ammunition, trigger state and RNG ownership with the weapon owner.
use crate::{gen::{Store,F3UnitKey,Weapon},units::ownership::{Ownership,Stale}};
use crate::ai::combat::world::WeaponSettings;

pub trait Changes {
    fn mark(&mut self,unit:crate::store::Ref<crate::gen::Unit>);
    /// Tuning and selection join the next native unit span when promised.
    /// Other owners retain their ordinary immediate dirty-row contract.
    fn weapon(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {self.mark(unit);}
    fn body(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {self.mark(unit);}
    fn intent(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {self.mark(unit);}
    /// Native ownership also controls whether a command publication may copy
    /// held intents. Synthetic owners already observe their canonical rows.
    fn native_intent(&mut self,unit:crate::store::Ref<crate::gen::Unit>,_owned:bool) {self.mark(unit);}
}
impl Changes for Ownership {fn mark(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {Ownership::mark(self,unit);}}
/// Native F3 keeps publication membership through recursive seat commands.
/// A service view marks the owner without retaining its mutable row table.
#[cfg(target_arch="wasm32")]
pub struct F3;
#[cfg(target_arch="wasm32")]
impl Changes for F3 {
    fn mark(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {crate::units::f3::changed(unit);}
    fn intent(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {crate::units::f3::intent_changed(unit);}
    fn weapon(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {crate::units::f3::ai_changed(unit);}
    fn body(&mut self,unit:crate::store::Ref<crate::gen::Unit>) {crate::units::f3::ai_changed(unit);}
    fn native_intent(&mut self,unit:crate::store::Ref<crate::gen::Unit>,owned:bool) {crate::units::f3::native_intent(unit,owned);}
}

pub fn settings(store:&Store,key:&F3UnitKey,index:usize)->Result<WeaponSettings,Stale> {
    let unit=key.validate(store)?;
    Ok(read(unit.combat.weapons.get(index).expect("AI weapon index")))
}
pub fn edit<R,C:Changes+?Sized>(store:&mut Store,owner:&mut C,key:&F3UnitKey,index:usize,
    edit:impl FnOnce(&mut WeaponSettings)->R)->Result<R,Stale> {
    let unit=key.validate_mut(store)?;
    let weapon=unit.combat.weapons.get_mut(index).expect("AI weapon index");
    let (result,changed)=edit_state(weapon,edit);
    if changed {owner.weapon(key.reference());}
    Ok(result)
}
pub fn buttons(store:&mut Store,key:&F3UnitKey,first:f64,second:f64)->Result<(),Stale> {
    let unit=key.validate_mut(store)?;
    unit.intent.buttons[0]=first;unit.intent.buttons[1]=second;Ok(())
}
pub fn aim(store:&mut Store,key:&F3UnitKey,point:Option<three::Vector3>)->Result<(),Stale> {
    key.validate_mut(store)?.intent.aim=point.map(|p|[p.x,p.y,p.z]);Ok(())
}
/// Apply only AI-owned tuning fields to either canonical weapon owner.
pub fn edit_state<R>(weapon:&mut Weapon,edit:impl FnOnce(&mut WeaponSettings)->R)->(R,bool) {
    let before=read(weapon);let mut value=before;let result=edit(&mut value);
    // Object.is publication comparison keeps signed zero and NaN distinct.
    let changed=!same(before,value);
    weapon.spread_scale=value.spread_scale;weapon.sniper_scale=value.sniper_scale.expect("native sniper scale");
    weapon.yaw_spread_override=value.yaw_spread_override;weapon.floor=value.floor;weapon.kick=value.kick;
    weapon.enhanced_floor=value.enhanced_floor;
    (result,changed)
}
/// prepareSpread's individual yaw write uses the same Object.is comparison
/// as the complete tuning record, including signed zero and NaN payloads.
pub fn set_yaw(w:&mut Weapon,value:Option<f64>)->bool {
    let changed=match(w.yaw_spread_override,value) {
        (None,None)=>false,(Some(a),Some(b))=>a.to_bits()!=b.to_bits(),_=>true,
    };
    w.yaw_spread_override=value;changed
}
pub fn read(w:&Weapon)->WeaponSettings { WeaponSettings {spread_scale:w.spread_scale,sniper_scale:Some(w.sniper_scale),
    yaw_spread_override:w.yaw_spread_override,kick:w.kick,floor:w.floor,enhanced_floor:w.enhanced_floor} }
fn same(a:WeaponSettings,b:WeaponSettings)->bool {
    fn optional(a:Option<f64>,b:Option<f64>)->bool {match(a,b){(Some(a),Some(b))=>a.to_bits()==b.to_bits(),(None,None)=>true,_=>false}}
    a.spread_scale.to_bits()==b.spread_scale.to_bits()&&optional(a.sniper_scale,b.sniper_scale)
        &&optional(a.yaw_spread_override,b.yaw_spread_override)&&a.kick.to_bits()==b.kick.to_bits()
        &&a.floor.to_bits()==b.floor.to_bits()&&a.enhanced_floor==b.enhanced_floor
}

use std::rc::Rc;
use crate::{combat::defs::{Js,JsObj,WeaponClass,WeaponDefs,K},store::DefId};
use crate::ai::combat::world::{WeaponView,index_of};
struct Definition {levels:Rc<Vec<Rc<JsObj>>>,channel:Js,rounds:Js,chargeable:bool}
pub struct Definitions {classes:Vec<Option<Definition>>}
impl Definitions {
    pub fn new()->Self {Self {classes:Vec::new()}}
    pub fn contains(&self,id:DefId)->bool {self.classes.get(id.0 as usize).is_some_and(Option::is_some)}
    pub fn register(&mut self,id:DefId,class:&WeaponClass) {
        let i=id.0 as usize;if self.classes.len()<=i {self.classes.resize_with(i+1,||None);}
        self.classes[i]=Some(Definition {levels:Rc::new(class.levels.iter().map(|level|level.obj.clone()).collect()),
            channel:class.base().get(K::Channel).clone(),rounds:class.base().get(K::RoundsPerClip).clone(),
            chargeable:crate::ai::combat::weapons_ai::charge_definition(&class.levels[0].obj)});
    }
    fn facts(&self,w:&Weapon)->&Definition {self.classes[w.def.0 as usize].as_ref().expect("prepared AI weapon definition")}
    pub fn chargeable(&self,w:&Weapon)->bool {self.facts(w).chargeable}
    /// Range scans borrow immutable levels instead of a runtime weapon copy.
    pub fn visit_levels(&self,w:&Weapon,visit:&mut dyn FnMut(&JsObj)) {
        for level in self.facts(w).levels.iter() {visit(level);}
    }
    pub fn definition(&self,w:&Weapon)->Rc<JsObj> {self.facts(w).levels[w.level as usize].clone()}
    pub fn view_weapon(&self,w:&Weapon)->WeaponView {
        let facts=self.facts(w);
        WeaponView {def:facts.levels[w.level as usize].clone(),levels:Some(facts.levels.clone()),channel:facts.channel.clone(),
            clip:w.clip,reserve:w.reserve,rounds:facts.rounds.clone(),bottomless:w.bottomless,heat:w.heat,
            overheated:w.overheated,charging:w.charging,held:w.held,wait:w.wait,limit:Some(w.limit),
            reloading:w.reloading(),busy:w.busy(),spread_scale:w.spread_scale,sniper_scale:Some(w.sniper_scale),
            yaw_spread_override:w.yaw_spread_override,kick:w.kick,floor:w.floor,enhanced_floor:w.enhanced_floor}
    }
    pub fn view(&self,store:&Store,key:&F3UnitKey,index:usize)->Option<WeaponView> {
        Some(self.view_weapon(key.validate(store).ok()?.combat.weapons.get(index)?))
    }
    pub fn current(&self,store:&Store,key:&F3UnitKey,channel:f64)->Option<usize> {
        self.current_combat(&key.validate(store).ok()?.combat,channel)
    }
    pub fn current_combat(&self,combat:&crate::gen::Combat,channel:f64)->Option<usize> {
        let c=index_of(channel)?;
        let selected=index_of(*combat.selected.get(c)?)?;
        combat.weapons.iter().enumerate().filter(|(_,w)|matches!(&self.facts(w).channel,Js::Num(n) if *n==channel))
            .nth(selected).map(|(index,_)|index)
    }
}
pub fn select<C:Changes+?Sized>(store:&mut Store,owner:&mut C,key:&F3UnitKey,index:usize,defs:&WeaponDefs)->Result<bool,Stale> {
    let unit=key.validate_mut(store)?;let before=unit.combat.selected;
    let selected=crate::combat::weapons::select(&mut unit.combat,defs,index);
    if before.map(f64::to_bits)!=unit.combat.selected.map(f64::to_bits) {owner.weapon(key.reference());}
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_channels_outside_the_arsenal_are_rejected() {
        let definitions=Definitions::new();let combat=crate::gen::Combat::default();
        for channel in [2.,255.,4294967295.,-1.,0.5,f64::NAN,f64::INFINITY] {
            assert_eq!(definitions.current_combat(&combat,channel),None);
        }
    }
}
