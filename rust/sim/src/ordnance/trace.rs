//! Optional shot outcomes share the HUD trace switch. No gameplay state changes.
use crate::gen::{F3OrdnanceNotice as Notice, F3OrdnanceKey, Impact};
use std::{cell::RefCell, collections::BTreeMap, sync::OnceLock, time::{SystemTime, UNIX_EPOCH}};
#[derive(Clone,Copy)]
struct Shot { player:f64, fire:f64, origin:[f64;3], direction:[f64;3], rewind:Option<f64>, wall:f64 }
thread_local! { static SHOTS:RefCell<BTreeMap<(u8,u32,u32),Shot>>=RefCell::new(BTreeMap::new()); }
pub fn enabled()->bool {static ENABLED:OnceLock<bool>=OnceLock::new();*ENABLED.get_or_init(||std::env::var_os("BFE_HUD_TRACE").is_some())}
fn key(k:&F3OrdnanceKey)->(u8,u32,u32){(k.kind as u8,k.slot,k.generation)}
pub fn begin(n:&Notice, direction:[f64;3], rewind:Option<f64>) {
    SHOTS.with(|shots|{let mut shots=shots.borrow_mut();if shots.len()==4096 {shots.clear();}
        let wall=SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.,|d|d.as_secs_f64()*1000.);
        shots.insert(key(&n.key),Shot {player:n.owner_id,fire:n.fire_id,origin:n.position,direction,rewind,wall});});
}
pub fn end(n:&Notice, impact:Option<&Impact>) {
    SHOTS.with(|shots|{if let Some(s)=shots.borrow_mut().remove(&key(&n.key)) {
        let distance=((n.point[0]-s.origin[0]).powi(2)+(n.point[1]-s.origin[1]).powi(2)+(n.point[2]-s.origin[2]).powi(2)).sqrt();
        let part=impact.map(|i|format!("{:?}",i.part));
        eprintln!("[combat-authority] {}",serde_json::json!({"player":s.player,"fire":s.fire,
            "projectile":[n.key.kind as u8 as u32,n.key.slot,n.key.generation],"origin":s.origin,"direction":s.direction,
            "rewind":s.rewind,"launchWall":s.wall,"hit":impact.and_then(|i|i.unit).map(|r|r.slot),"part":part,
            "point":n.point,"distance":distance,"surface":n.surface}));
    }});
}
