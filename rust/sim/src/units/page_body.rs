//! Soldier construction, spawn and immediate look from soldier.js. Cold
//! bodies publish generated mirrors; a frame consumes all pointer samples
//! together using the same Soldier methods as authority and private replay.
use crate::gen::{BodyLook, BodyPreviewFrame, Soldier};

// A stalled frame can retain many commands and pointer samples. Reserve the
// complete prefix at setup, and refuse its excess without growing a frame.
pub const LOOKS: usize = 65_536;

// Soldier.look and Soldier.lookRaw share the authoritative posture/air rules.
// The scratch Soldier is stack data: no body, arsenal or rig is cloned here.
pub fn preview(frame: &mut BodyPreviewFrame, looks: &[f64]) {
    let count = (frame.count as usize).min(LOOKS).min(looks.len() / BodyLook::WORDS);
    // Clamp both the advertised count and the actual span. Native callers
    // can submit a shortened fixture without exposing an out-of-bounds read.
    frame.skipped = frame.count.saturating_sub(count as u32);
    let mut body = Soldier { aim_yaw: frame.aim_yaw, aim_pitch: frame.aim_pitch,
        max_turn: frame.max_turn, pitch_limits: frame.pitch_limits,
        posture: frame.posture, air: frame.air, dive: frame.dive, lock: frame.lock,
        tick_length: frame.tick_length, look_x: frame.look_x, look_y: frame.look_y,
        ..Default::default() };
    // Clamping belongs to each sample. Combining counts before a prone or
    // airborne turn changes the final pitch at a limit or after reversal.
    for row in looks[..count * BodyLook::WORDS].chunks_exact(BodyLook::WORDS) {
        let look = BodyLook::take(row, &[]);
        if look.raw { body.look_raw(look.dx, look.dy, look.scale, look.invert); }
        else { body.look(look.dx, look.dy, look.scale, look.invert); }
    }
    frame.aim_yaw = body.aim_yaw; frame.aim_pitch = body.aim_pitch;
    frame.look_x = body.look_x; frame.look_y = body.look_y;
}

// Command aim shares the camera's offsets. World and unit ray responses stay
// at the existing collision boundary, then one resume completes convergence.
pub fn aim<M:bfe_presentation::cameras::math::Math>(f:&mut BodyPreviewFrame) {
    use bfe_presentation::{aim::Basis,cameras::math::{plus,sub}};
    use three::Vector3 as V;
    let position=V::new(f.position[0],f.position[1],f.position[2]);
    // Third-person droideka convergence historically used the infantry gun
    // pivot, even though its displayed gun/eye uses the built-in barrel.
    let b=Basis::new::<M>(position,f.aim_yaw,f.aim_pitch,f.posture as usize,f.air.is_some());
    f.basis=b.words();
    let eye=if f.walker{V::new(f.feet[0],f.feet[1]+if f.rolled{1.0}else{f.eye_height},f.feet[2])}
        else{b.eye(V::new(f.feet[0],f.feet[1],f.feet[2]),position,f.posture as usize,f.air.is_some(),f.zoom)};
    f.eye=[eye.x,eye.y,eye.z];f.shot_yaw=f.aim_yaw;f.shot_pitch=f.aim_pitch;
    f.needs_reach=f.has_camera&&!f.uses_eye;
    if f.needs_reach {
        let camera=V::new(f.camera[0],f.camera[1],f.camera[2]);
        let behind=sub(camera,b.pivot).dot(&b.direction);
        let mut from=plus(camera,b.direction,-behind);
        let posture=if f.air.is_some(){0}else{f.posture as usize};
        from=plus(from,b.direction,f.track_offsets[posture.min(2)]+2.0);
        f.from=[from.x,from.y,from.z];
    }
}
// PlayerCombat.convergedAim normalized only after adding the collision reach
// and subtracting the gun origin. Preserve that order for nearby parallax.
pub fn resolve<M:bfe_presentation::cameras::math::Math>(f:&mut BodyPreviewFrame) {
    use three::Vector3 as V;
    if !f.needs_reach{return;}
    let mut dir=V::new(f.from[0],f.from[1],f.from[2]);
    dir.add_scaled_vector(&V::new(f.basis[12],f.basis[13],f.basis[14]),f.reach);
    dir.sub(&V::new(f.basis[0],f.basis[1],f.basis[2])).normalize();
    f.shot_yaw=M::atan2(dir.x,dir.z);
    f.shot_pitch=M::asin(v8math::max(-1.0,v8math::min(1.0,dir.y)));
}

#[cfg(target_arch = "wasm32")]
mod module {
    use super::*;
    use crate::{abi::sim, gen::{BodySpawn,BodySpawnOutput}};
    use std::ptr::addr_of_mut;
    // Fixed pointers survive calls. Only WASM memory growth detaches a host
    // view; another preview does not resize either reserved allocation.
    struct Preview { frame: [f64; BodyPreviewFrame::WORDS], looks: Vec<f64> }
    const AIM_ROWS:usize=2048;
    static mut AIMS:Option<Vec<f64>>=None;
    static mut PREVIEW: Option<Preview> = None;
    fn owner() -> &'static mut Preview {
        // One thread owns the page preview; no borrow crosses a host import.
        unsafe { (*addr_of_mut!(PREVIEW)).get_or_insert_with(|| Preview {
            frame: [0.0; BodyPreviewFrame::WORDS], looks: vec![0.0; LOOKS * BodyLook::WORDS] }) }
    }
    fn aims()->&'static mut Vec<f64> {
        // All shot poses in a replay publication share this reserved block.
        // The host answers their rays before one resume resolves every aim.
        unsafe{(*addr_of_mut!(AIMS)).get_or_insert_with(||vec![0.0;AIM_ROWS*BodyPreviewFrame::WORDS])}
    }
    #[no_mangle]
    pub extern "C" fn body_aim_ptr()->*mut f64{aims().as_mut_ptr()}
    #[no_mangle]
    pub extern "C" fn body_aim_capacity()->u32{AIM_ROWS as u32}
    #[no_mangle]
    pub extern "C" fn body_aim_batch(count:u32,resume:u32) {
        // Count is untrusted independently of the host writer. Oversized
        // paused publications keep their admitted prefix and report the tail.
        let skipped=count.saturating_sub(AIM_ROWS as u32);
        let count=(count as usize).min(AIM_ROWS);let rows=aims();
        for row in rows[..count*BodyPreviewFrame::WORDS].chunks_exact_mut(BodyPreviewFrame::WORDS){
            let mut frame=BodyPreviewFrame::take(row,&[]);
            if resume!=0{resolve::<crate::cameras::HostMath>(&mut frame);}
            else{aim::<crate::cameras::HostMath>(&mut frame);}
            frame.put(row,&mut Vec::new());
        }
        // The first generated row also exposes refusal count to the host;
        // no diagnostic buffer or per-refusal event needs to grow here.
        rows[BodyPreviewFrame::SKIPPED]=skipped as f64;
    }
    #[no_mangle]
    pub extern "C" fn body_preview_frame_ptr() -> *mut f64 { owner().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn body_preview_looks_ptr() -> *mut f64 { owner().looks.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn body_preview_capacity() -> u32 { LOOKS as u32 }
    #[no_mangle]
    pub extern "C" fn body_preview() {
        let p = owner(); let mut f = BodyPreviewFrame::take(&p.frame, &[]);
        // convergedAim restored the tick basis after sampling pending look.
        // Publish it separately: HUD targeting reads the physical gun frame.
        let mut source=f.clone();source.aim_yaw=f.source_yaw;source.aim_pitch=f.source_pitch;
        aim::<crate::cameras::HostMath>(&mut source);
        preview(&mut f, &p.looks);
        // The camera's scalar host math preserves the browser's last bits;
        // body/scene methods never cross this numeric boundary.
        aim::<crate::cameras::HostMath>(&mut f);
        f.source_basis=source.basis;
        // Both records contain scalars only; no heap can grow on this path.
        f.put(&mut p.frame, &mut Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn body_preview_resolve() {
        let p=owner();let mut f=BodyPreviewFrame::take(&p.frame,&[]);
        resolve::<crate::cameras::HostMath>(&mut f);
        f.put(&mut p.frame,&mut Vec::new());
    }
    #[no_mangle]
    // Soldier.constructor's native class reader keeps authored defaults in
    // one place. JSON parsing is confined to this cold resource admission.
    pub extern "C" fn body_construct(ptr: *const u8, len: u32) -> *const f64 {
        // Soldier.constructor accepts only the owned scratch span, so no raw
        // slice can outlive its allocation or read beyond the supplied bytes.
        let Some(bytes)=sim().scratch_bytes(ptr,len as usize) else {return std::ptr::null()};
        let Ok(def)=bfe_content::json::parse_bytes(bytes) else {return std::ptr::null()};
        if !matches!(def,bfe_content::Value::Object(_)) {return std::ptr::null();}
        let body = super::super::class::soldier_body(&def, 200.0, 0.0, 0.0);
        let s = sim(); s.calls.result(Soldier::WORDS, |row, heap| body.put(row, heap));
        s.calls.block.ptr()
    }
    fn spawn_cast(world:u32,p:&three::Vector3,d:&three::Vector3,far:f64)->Option<super::super::world::CastHit> {
        // Cold resource ids are still boundary input. A stale or forged id
        // leaves the requested placement airborne instead of indexing a store.
        if world as usize>=crate::collision::state().worlds.len(){None}
        else{super::super::native::cast(world as usize,p,d,far)}
    }
    #[no_mangle]
    pub extern "C" fn body_spawn_droideka(world:u32)->*const f64 {
        use crate::gen::{Droideka,DroidekaSpawn,DroidekaSpawnOutput,Shield};
        let(row,heap)=sim().calls.args();let input=DroidekaSpawn::take(row,heap);
        let mut body=Droideka::default();
        if input.has_shield{body.shield=Some(Shield{max:input.shield_max,..Default::default()});}
        let[x,y,z]=input.position;
        // Droideka.spawn's grounding and shield reset are shared with the
        // canonical unit body. No probe vector or hit object belongs to JS.
        body.spawn_with(x,y,z,input.yaw,|p,d,far|{
            spawn_cast(world,p,d,far)
        });
        // Native Unit.spawn enters deployed mode and resets interpolation.
        // The page sends the two animator reset commands after publication.
        let out=DroidekaSpawnOutput{position:body.position,previous:body.position,settled:body.position,
            velocity:body.velocity,aim_yaw:body.aim_yaw,aim_pitch:body.aim_pitch,legs:body.legs,spin:body.spin,
            dead:body.dead,mode:0.0,time:0.0,reverse:false,
            shield_value:body.shield.as_ref().map_or(0.0,|s|s.value),shield_on:false};
        let s=sim();s.calls.result(DroidekaSpawnOutput::WORDS,|row,heap|out.put(row,heap));s.calls.block.ptr()
    }
    #[no_mangle]
    // Soldier.spawn uses the same reset as Unit.spawn. The collision store
    // is borrowed directly, so the grounding query cannot call the page.
    pub extern "C" fn body_spawn(world: u32) -> *const f64 {
        let (row, heap) = sim().calls.args(); let input = BodySpawn::take(row, heap);
        let mut body = Soldier::default(); body.jet.recharge = input.recharge;
        let [x,y,z] = input.position;
        body.spawn_with(x,y,z,input.yaw, |p,d,far| {
            spawn_cast(world,p,d,far)
        });
        // The old exact-correction schema carries the probe origin even
        // though native movement does not read that overwritten scratch.
        let out = BodySpawnOutput { position:body.position, previous:body.previous, settled:body.settled,
            velocity:body.velocity, normal:body.normal, origin:[x,y+1.5,z], aim_yaw:body.aim_yaw, aim_pitch:body.aim_pitch,
            posture:body.posture, air:body.air, dive:body.dive, state_time:body.state_time, lock:body.lock,
            fuel:body.fuel, grounded:body.grounded, ground_adjustment:body.ground_adjustment };
        let s = sim(); s.calls.result(BodySpawnOutput::WORDS, |row, heap| out.put(row, heap));
        s.calls.block.ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::Posture;
    // Frozen results include a pitch-limit reversal: summing the samples
    // before applying the limit would produce a different final angle.
    #[test]
    fn ordered_prone_look_and_locked_body() {
        let mut frame=BodyPreviewFrame {aim_yaw:0.25, aim_pitch:0.35, max_turn:2.0,
            pitch_limits:[-0.8,1.1], posture:Posture::Prone, count:2, ..Default::default()};
        let mut samples=[0.0;2*BodyLook::WORDS];let mut heap=Vec::new();
        BodyLook {dx:256.0,dy:-256.0,scale:1.0,invert:false,raw:true}.put(&mut samples,&mut heap);
        BodyLook {dx:128.0,dy:128.0,scale:1.0,invert:false,raw:true}.put(&mut samples[BodyLook::WORDS..],&mut heap);
        preview(&mut frame,&samples);
        assert_eq!((frame.aim_yaw,frame.aim_pitch),(0.0625,0.275));
        frame.lock=1.0;preview(&mut frame,&samples);
        assert_eq!((frame.aim_yaw,frame.aim_pitch),(0.0625,0.275));
    }
    #[test]
    fn truncated_or_oversized_prefix_is_bounded() {
        let mut frame=BodyPreviewFrame {count:u32::MAX, lock:1.0, ..Default::default()};
        preview(&mut frame,&[0.0;BodyLook::WORDS]);
        assert_eq!(frame.skipped,u32::MAX-1);
    }
}
