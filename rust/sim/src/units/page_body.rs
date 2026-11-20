//! Soldier and droideka bodies made and placed for a host: a soldier's
//! construction from its class, and both kinds' spawn placement. Cold
//! bodies publish generated mirrors.
use crate::gen::Soldier;

pub(crate) mod module {
    use super::*;
    use crate::{gen::{BodySpawn,BodySpawnOutput}};
    // Soldier.constructor's native class reader keeps authored defaults in
    // one place. JSON parsing is confined to this cold resource admission.
    pub(crate) fn body_construct_in(w: &mut crate::world::owner::World, def: bfe_content::Value) -> Option<&[f64]> {
        if !matches!(def,bfe_content::Value::Object(_)) {return None;}
        let body = super::super::class::soldier_body(&def, 200.0, 0.0, 0.0);
        let s = &mut w.sim; s.calls.result(Soldier::WORDS, |row, heap| body.put(row, heap));
        Some(&s.calls.block.words[..])
    }
    fn spawn_cast(w: &mut crate::world::owner::World, world:u32,p:&three::Vector3,d:&three::Vector3,far:f64)->Option<super::super::world::CastHit> {
        // A stale resource leaves the placement airborne instead of indexing
        // collision storage. The unit query keeps its original flag ordering.
        if world as usize >= w.collision.state.worlds.len() { None }
        else { super::super::native::cast_in(w, world as usize,p,d,far) }
    }

    pub(crate) fn body_spawn_droideka_in(w: &mut crate::world::owner::World, world:u32) -> Option<&[f64]> {
        use crate::gen::{Droideka,DroidekaSpawn,DroidekaSpawnOutput,Shield};
        let(row,heap)=w.sim.calls.args();let input=DroidekaSpawn::take(row,heap);
        let mut body=Droideka::default();
        if input.has_shield{body.shield=Some(Shield{max:input.shield_max,..Default::default()});}
        let[x,y,z]=input.position;
        // Droideka.spawn's grounding and shield reset are shared with the
        // canonical unit body. No probe vector or hit object belongs to JS.
        body.spawn_with(x,y,z,input.yaw,|p,d,far|{
            spawn_cast(w,world,p,d,far)
        });
        // Native Unit.spawn enters deployed mode and resets interpolation.
        // The screen sends the two animator reset commands after publication.
        let out=DroidekaSpawnOutput{position:body.position,previous:body.position,settled:body.position,
            velocity:body.velocity,aim_yaw:body.aim_yaw,aim_pitch:body.aim_pitch,legs:body.legs,spin:body.spin,
            dead:body.dead,mode:0.0,time:0.0,reverse:false,
            shield_value:body.shield.as_ref().map_or(0.0,|s|s.value),shield_on:false};
        let s=&mut w.sim;s.calls.result(DroidekaSpawnOutput::WORDS,|row,heap|out.put(row,heap));Some(&s.calls.block.words[..])
    }
    // Soldier.spawn uses the same reset as Unit.spawn. The collision store
    // is borrowed directly, so the grounding query cannot call the screen.
    pub(crate) fn body_spawn_in(w: &mut crate::world::owner::World, world: u32) -> Option<&[f64]> {
        let (row, heap) = w.sim.calls.args(); let input = BodySpawn::take(row, heap);
        let mut body = Soldier::default(); body.jet.recharge = input.recharge;
        let [x,y,z] = input.position;
        body.spawn_with(x,y,z,input.yaw, |p,d,far| {
            spawn_cast(w,world,p,d,far)
        });
        // The old exact-correction schema carries the probe origin even
        // though native movement does not read that overwritten scratch.
        let out = BodySpawnOutput { position:body.position, previous:body.previous, settled:body.settled,
            velocity:body.velocity, normal:body.normal, origin:[x,y+1.5,z], aim_yaw:body.aim_yaw, aim_pitch:body.aim_pitch,
            posture:body.posture, air:body.air, dive:body.dive, state_time:body.state_time, lock:body.lock,
            fuel:body.fuel, grounded:body.grounded, ground_adjustment:body.ground_adjustment };
        let s = &mut w.sim; s.calls.result(BodySpawnOutput::WORDS, |row, heap| out.put(row, heap));
        Some(&s.calls.block.words[..])
    }
}
