//! Frozen synthetic Weapon.update, lock, reload, spread and deselect results.
//! The page's former generic controller had no live caller after native recon
//! replay. These records keep its exact arithmetic coverage without a JS twin.
use bfe_sim::combat::defs::{Js,JsObj,WeaponClass};
use bfe_sim::combat::weapons::{Admission,Aim,LockTarget,Shot,ShotKind,Trigger,WeaponDraws};
use bfe_sim::gen::{Posture,Weapon};
use serde_json::Value;
use std::rc::Rc;

// The fixture stores generated Weapon rows as little-endian doubles. Keeping
// binary words preserves negative zero, infinities and every timer's low bit;
// JSON is used only for finite synthetic definitions and named operation facts.
struct Reader<'a>{bytes:&'a[u8],at:usize}
impl<'a> Reader<'a>{
    fn bytes(&mut self,n:usize)->&'a[u8]{let out=&self.bytes[self.at..self.at+n];self.at+=n;out}
    fn count(&mut self)->usize{u32::from_le_bytes(self.bytes(4).try_into().unwrap())as usize}
    fn json(&mut self)->Value{let n=self.count();serde_json::from_slice(self.bytes(n)).unwrap()}
    fn numbers(&mut self)->Vec<f64>{let n=self.count();self.bytes(n*8).chunks_exact(8).map(|v|f64::from_le_bytes(v.try_into().unwrap())).collect()}
}
fn definition(value:&Value)->Js{match value{
    Value::Null=>Js::Null,Value::Bool(v)=>Js::Bool(*v),Value::Number(v)=>Js::Num(v.as_f64().unwrap()),Value::String(v)=>Js::Str(v.as_str().into()),
    Value::Array(values)=>Js::Arr(Rc::new(values.iter().map(definition).collect())),
    Value::Object(fields)=>Js::Obj(Rc::new(JsObj{fields:fields.iter().map(|(k,v)|(k.as_str().into(),definition(v))).collect(),def:None})),
}}
// Only external draws are replayed. Seeded draws remain the real native RNG,
// whose result is independently checked by each shot and complete state row.
struct Draws{values:Vec<f64>,at:usize}
impl Draws{fn next(&mut self)->f64{let value=self.values[self.at];self.at+=1;value}}
impl WeaponDraws for Draws{
    fn draw(&mut self,_:u32)->f64{self.next()}
    fn legacy(&mut self,_:u32)->f64{self.next()}
    fn seeded(&mut self,_:f64,_:f64){}
    fn admit(&mut self,_:u32)->Admission{Admission::Yes}
}
fn number(v:&Value,key:&str)->f64{v[key].as_f64().unwrap_or(0.)}
fn flag(v:&Value,key:&str)->bool{v[key].as_bool().unwrap_or(false)}
fn aim(v:&Value)->Aim{Aim{stick:number(v,"stick"),posture:match v["posture"].as_str(){Some("crouch")=>Some(Posture::Crouch),Some("prone")=>Some(Posture::Prone),_=>Some(Posture::Stand)},zoomed:flag(v,"zoomed"),repair_target:flag(v,"repairTarget")}}
// Compare the IEEE words rather than approximate timer values. A tolerance
// could hide a shot moving to a different update after a correction.
fn bits(values:&[f64])->Vec<u64>{values.iter().map(|v|v.to_bits()).collect()}
fn shot_values(shots:&[Shot])->Vec<f64>{shots.iter().flat_map(|s|match s.kind{
    ShotKind::Round{yaw,pitch,charge,target_id,target_generation,..}=>[0.,yaw,pitch,charge,target_id,target_generation,0.],
    ShotKind::Repair=>[1.,0.,0.,0.,0.,0.,0.],_=>panic!("unexpected melee fixture"),
}).collect()}
#[test]
fn every_generic_weapon_checkpoint_matches_its_frozen_reference(){
    let mut input=Reader{bytes:include_bytes!("../../../tests/fixtures/prediction-reference/weapons.bin"),at:0};
    assert_eq!(input.bytes(4),b"WPR1");
    // Definitions are invented for this corpus and resolve once outside replay.
    // No extracted asset or alternate runtime implementation is required.
    let source=input.json();let classes:Vec<_>=source.as_array().unwrap().iter().map(|value|{
        let Js::Obj(object)=definition(value)else{unreachable!()};WeaponClass::new(object)
    }).collect();
    let count=input.count();assert_eq!(count,3880);
    for index in 0..count{
        let class=&classes[input.count()];let operation=input.json();let args=input.json();
        let before=input.numbers();assert_eq!(before.len(),Weapon::WORDS);
        let mut weapon=Weapon::take(&before,&[]);let mut draws=Draws{values:input.numbers(),at:0};
        let after=input.numbers();let expected=input.numbers();
        // Restore each observed boundary independently. A prior correct result
        // cannot hide a wrong reload/lock/charge input in the following record.
        let result=match operation.as_str().unwrap(){
            "update"=>{let mut shots=Vec::new();weapon.update(0,class,number(&args,"dt"),flag(&args,"selected"),
                &Trigger{held:flag(&args,"held"),pressed:flag(&args,"pressed"),seed:args["seed"].as_f64(),late:number(&args,"late")},
                flag(&args,"reload"),&aim(&args),&mut draws,&mut shots);shot_values(&shots)},
            "updateLock"=>{let target=&args["target"];weapon.update_lock(class,number(&args,"dt"),Some(LockTarget{id:number(target,"id"),generation:number(target,"generation"),alive:flag(target,"alive")}));Vec::new()},
            "startReload"=>vec![u8::from(weapon.start_reload(class))as f64],
            "deselect"=>{weapon.deselect();Vec::new()},
            "spread"=>{let a=aim(&args);weapon.spread(class,a.stick,a.posture,a.zoomed).to_vec()},
            name=>panic!("unknown fixture operation {name}"),
        };
        // Check both published state and emitted shots: identical final ammo
        // alone would miss a changed seeded pattern or delayed salvo order.
        let mut actual=vec![0.;Weapon::WORDS];weapon.put(&mut actual,&mut Vec::new());
        assert_eq!(bits(&actual),bits(&after),"state {index}: {operation}");
        assert_eq!(bits(&result),bits(&expected),"result {index}: {operation}");
        assert_eq!(draws.at,draws.values.len(),"draw count {index}");
    }
    assert_eq!(input.at,input.bytes.len(),"no trailing fixture words");
}
