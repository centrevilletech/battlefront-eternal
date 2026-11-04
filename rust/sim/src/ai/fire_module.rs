//! Persistent trigger state and button edges on the shared AI instance.

use super::{module::state,combat::{weapons_ai::{FireControl,press_button},world::Ref}};

pub(super) struct FireState {
    pub data: FireControl,
    pub tag: u32,
    pub generation_tag: u32,
    pub serial: f64,
}

#[no_mangle]
pub extern "C" fn ai_fire_new() -> u32 {
    let s=state();s.fires.push(FireState {data:FireControl::default(),tag:0,generation_tag:0,serial:0.0});(s.fires.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_fire_ref(id: u32,tag: u32,target: u32,generation: f64,generation_tag: u32) -> f64 {
    let t=&mut state().fires[id as usize];t.tag=tag;t.generation_tag=generation_tag;t.serial+=1.0;
    t.data.weapon_target=(tag==2).then_some(Ref {obj:target as u64,generation});t.serial
}

#[no_mangle]
pub extern "C" fn ai_fire_read(id: u32) {
    let s=state();let t=&s.fires[id as usize];let r=t.data.weapon_target.unwrap_or(Ref {obj:0,generation:0.0});
    s.out.clear();s.out.extend([t.tag as f64,r.obj as f64,r.generation,t.generation_tag as f64,t.serial]);
}

#[no_mangle]
pub extern "C" fn ai_fire_field(id: u32,field: u32) -> f64 {
    let f=&state().fires[id as usize].data;
    match field {0=>f.channel,1=>f.last_release,2=>f.charging as u32 as f64,3=>f.hold,4=>f.sniper_active as u32 as f64,
        5=>f.sniper_deadline,6=>f.burst.state,7=>f.burst.heat_limit,8=>f.burst.last_end,9=>f.burst.saved_ammo,10=>f.burst.count,
        11=>f.buttons[0].count,12=>f.buttons[0].code,13=>f.buttons[1].count,14=>f.buttons[1].code,_=>panic!("invalid fire field")}
}

#[no_mangle]
pub extern "C" fn ai_fire_write(id: u32,field: u32,value: f64) {
    let f=&mut state().fires[id as usize].data;let flag=value!=0.0 && !value.is_nan();
    match field {0=>f.channel=value,1=>f.last_release=value,2=>f.charging=flag,3=>f.hold=value,4=>f.sniper_active=flag,
        5=>f.sniper_deadline=value,6=>f.burst.state=value,7=>f.burst.heat_limit=value,8=>f.burst.last_end=value,9=>f.burst.saved_ammo=value,10=>f.burst.count=value,
        11=>f.buttons[0].count=value,12=>f.buttons[0].code=value,13=>f.buttons[1].count=value,14=>f.buttons[1].code=value,_=>panic!("invalid fire field")}
}

#[no_mangle]
pub extern "C" fn ai_fire_reset(id: u32) {
    let t=&mut state().fires[id as usize];t.data.reset();t.tag=0;t.generation_tag=0;t.serial+=1.0;
}

#[no_mangle]
pub extern "C" fn ai_fire_press(id: u32,channel: u32,down: u32) {
    press_button(&mut state().fires[id as usize].data.buttons[channel as usize],down!=0);
}
