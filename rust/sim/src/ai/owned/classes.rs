//! Class-job weapon policies read the canonical arsenal and item lists. All
//! definition text and service tables are resolved once when classes load.
use super::rows::{ReadContext,context_store};
use crate::{gen::{AiLifeHandle,ItemOwner,Store,Weapon},store::{DefId,Ref,StrId}};
use crate::combat::defs::{DefTable,Js,JsObj,WeaponClass};
use super::super::control::{classes::{remaining_ammo_values,quantity,FLT_MAX},world::QtySnap};

#[derive(Clone,Copy,PartialEq,Eq)]
enum Base { Other, Repair, Dispenser }
struct Facts {
    base: Base, rounds: Option<f64>, ammo: Option<f64>, max_items:f64,
    mine:bool, persistent_mine:bool, item:Option<DefId>,
    def_service:QtySnap, item_service:QtySnap,
}

macro_rules! dispenser_key {($keys:expr,$context:expr,$($a:expr),*)=>{($keys).owned_key($context,$($a),*)};}

pub trait DispenserKeys {
    /// F3's construction-time owner key, qualified by unit life and level.
    /// None denotes a missing ownedBy service, not an empty owned list.
    fn owned_key(&self,  w:&crate::world::owner::World,unit:&AiLifeHandle, weapon:DefId, level:u32)->Option<StrId>;
}
pub struct Classes { weapons:Vec<Option<Vec<Facts>>> }
impl Classes {
    pub fn new()->Self {Self {weapons:Vec::new()}}
    pub fn weapon(&mut self, id:DefId, class:&WeaponClass) {
        let at=id.0 as usize;if self.weapons.len()<=at {self.weapons.resize_with(at+1,||None);}
        self.weapons[at]=Some(class.levels.iter().map(|level|Facts::read(&level.obj)).collect());
    }
    fn facts(&self,weapon:&Weapon)->&Facts {
        &self.weapons[weapon.def.0 as usize].as_ref().expect("prepared class-job weapon")[weapon.level as usize]
    }
    pub fn is_repair(&self,weapon:&Weapon)->bool {self.facts(weapon).base==Base::Repair}
    pub fn remaining(&self,weapon:&Weapon)->f64 {
        remaining_ammo_values(weapon.bottomless,self.facts(weapon).rounds,Some(weapon.clip),Some(weapon.reserve))
    }
    pub fn needs_ammo(&self,store:&Store,unit:&AiLifeHandle)->Option<bool> {
        let unit=unit.unit(store)?;
        Some(unit.combat.weapons.iter().any(|w|self.weapon_needs_ammo(w)))
    }
    /// classes.needsAmmo uses reserve capacity, including bottomless weapons.
    /// SeatUnit observes its holder's current weapon through the same rule.
    pub fn weapon_needs_ammo(&self,weapon:&Weapon)->bool {
        let cap=self.facts(weapon).ammo.filter(|a|*a>0.).unwrap_or(FLT_MAX);
        weapon.reserve<cap-1.
    }
    pub fn needs_health(&self,store:&Store,unit:&AiLifeHandle)->Option<bool> {
        let unit=unit.unit(store)?;Some(unit.combat.health<unit.combat.max_health)
    }
    /// dispenserAvailable reads ownedBy only after ammo and MaxItems gates.
    /// The caller supplies the exact Unit or SeatUnit owner; a seat never
    /// substitutes its rider when asking for that object's dispenser list.
    pub fn available(&self,weapon:&Weapon,count:impl FnOnce()->usize)->bool {
        let (ready,limit)=self.availability(weapon);
        ready.unwrap_or_else(||(count() as f64)<limit)
    }
    /// Copy the scalar gates before a first-use owner notice borrows its
    /// journal. No weapon reference crosses that synchronous publication.
    pub fn availability(&self,weapon:&Weapon)->(Option<bool>,f64) {
        if self.remaining(weapon)==0. {return (Some(false),0.);}
        let max=self.facts(weapon).max_items;
        if max==0.||max.is_nan() {(Some(true),max)}else{(None,max)}
    }
    pub fn persistent_mine(&self,weapon:&Weapon)->bool {
        let facts=self.facts(weapon);facts.base==Base::Dispenser&&facts.mine&&facts.persistent_mine
    }
    pub fn dispenser(&self,weapon:&Weapon)->bool {self.facts(weapon).base==Base::Dispenser}
    pub fn service_quantity(&self,weapon:&Weapon,health_type:f64,ammo:bool)->f64 {
        let facts=self.facts(weapon);
        let qty=match facts.base {Base::Repair=>&facts.def_service,Base::Dispenser=>&facts.item_service,_=>return 0.};
        quantity(qty,health_type,ammo)
    }
    pub fn dispenser_available(&self,context:ReadContext<'_>,unit:&AiLifeHandle,index:usize,keys:&dyn DispenserKeys)->Option<bool> {let store=context_store!(context);
        let row=unit.unit(store)?;let weapon=row.combat.weapons.get(index)?;let facts=self.facts(weapon);
        Some(self.available(weapon,|| {
            if let Some(key)=dispenser_key!(keys,context,unit,weapon.def,weapon.level as u32) {
                // An absent key in a present ownedBy returns an empty list.
                // It must not turn into the degraded all-item fallback scan.
                store.items.owned.get(key).map(|items|items.iter().filter(|r|store.item.get(**r).is_some_and(|i|i.gone!=Some(true))).count()).unwrap_or(0)
            } else {
                let owner=ItemOwner::Unit(Ref::new(unit.slot,unit.generation));
                store.items.items.iter().filter(|r|store.item.get(**r).is_some_and(|i|i.gone!=Some(true) && i.owner==owner && Some(i.def)==facts.item)).count()
            }
        }))
    }

    /// mineWeapon (classes.js): the first usable persistent mine dispenser.
    pub fn mine_weapon(&self,context:ReadContext<'_>,unit:&AiLifeHandle,keys:&dyn DispenserKeys)->Option<usize> {let store=context_store!(context);
        let row=unit.unit(store)?;
        (0..row.combat.weapons.len()).find(|&i| {
            let facts=self.facts(&row.combat.weapons[i]);
            facts.base==Base::Dispenser && facts.mine && facts.persistent_mine
                && self.dispenser_available(context,unit,i,keys)==Some(true)
        })
    }
    /// serviceWeapon (classes.js): quantity preserves service-table presence
    /// and the distinct BuildingRebuild/BuildingBuild bare-health fallback.
    pub fn service_weapon(&self,context:ReadContext<'_>,unit:&AiLifeHandle,health_type:f64,ammo:bool,keys:&dyn DispenserKeys)->Option<usize> {let store=context_store!(context);
        let row=unit.unit(store)?;
        (0..row.combat.weapons.len()).find(|&i| {
            let facts=self.facts(&row.combat.weapons[i]);
            let qty=match facts.base {
                Base::Repair=>&facts.def_service,
                Base::Dispenser if self.dispenser_available(context,unit,i,keys)==Some(true)=>&facts.item_service,
                _=>return false,
            };
            quantity(qty,health_type,ammo)>0.
        })
    }
}
impl Default for Classes {fn default()->Self {Self::new()}}
fn number(value:&Js)->Option<f64> {if value.is_nullish(){None}else{Some(value.num())}}
impl Facts {
    fn read(def:&JsObj)->Self {
        let base=if def.get("base").is_str("repair"){Base::Repair}else if def.get("base").is_str("dispenser"){Base::Dispenser}else{Base::Other};
        let item=def.get("item");let life=item.get("LifeSpan");
        Self {base,rounds:number(def.get("RoundsPerClip")),ammo:number(def.get("ammo")),
            max_items:number(def.get("MaxItems")).unwrap_or(0.),mine:item.get("base").is_str("mine"),
            persistent_mine:life.is_nullish() || matches!(life,Js::Num(v) if *v==f64::INFINITY || *v==FLT_MAX),
            item:item.obj().and_then(|o|o.def).and_then(|(table,id)|(table==DefTable::Item).then_some(id)),
            def_service:read_quantities(def),item_service:item.obj().map(|o|read_quantities(o)).unwrap_or_default()}
    }
}
fn read_quantities(def:&JsObj)->QtySnap {
    use super::super::control::classes::HEALTH_TYPES;
    let table=|kind:&str|match def.get("service").get(kind) {
        Js::Arr(values)=>Some(std::array::from_fn(|i|values.get(i).and_then(number))),_=>None,
    };
    QtySnap {svc_health:table("health"),svc_ammo:table("ammo"),
        named_health:std::array::from_fn(|i|number(def.get(&format!("{}Health",HEALTH_TYPES[i])))),
        named_ammo:std::array::from_fn(|i|number(def.get(&format!("{}Ammo",HEALTH_TYPES[i])))),
        bare:std::array::from_fn(|i|number(def.get(HEALTH_TYPES[i])))}
}

/// The live world always has Items.ownedBy. A missing prepared key is a load
/// error; it must not silently choose the degraded all-items scan.

pub struct F3Keys;

impl DispenserKeys for F3Keys {
    fn owned_key(&self,  w:&crate::world::owner::World,unit:&AiLifeHandle, weapon:DefId, level:u32)->Option<StrId> {
        Some(crate::units::f3::items_read(&w.units).owned_key(unit.slot,unit.life,weapon,level).expect("prepared AI dispenser key"))
    }
}
