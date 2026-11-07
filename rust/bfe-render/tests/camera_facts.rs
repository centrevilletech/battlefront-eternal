//! Frozen f64 publications from the former camera, mirror, cube and CSM
//! producers. Inputs include held windows, fast turns, cuts and lens changes.
use bfe_render::cameras::{Cameras,layout::{input as I,camera as C,cascade as W,role as R,limit as L,light as P,pack as K}};
use serde_json::Value;
fn value(v:&Value)->f64 {
    let s=v.as_str().unwrap();let mut bytes=[0;8];
    for i in 0..8{bytes[i]=u8::from_str_radix(&s[i*2..i*2+2],16).unwrap();}
    f64::from_le_bytes(bytes)
}
fn check(actual:&[f64],expected:&Value,label:&str){
    for pair in expected.as_array().unwrap(){let at=pair[0].as_u64().unwrap() as usize;
        for(i,v)in pair[1].as_array().unwrap().iter().enumerate(){assert_eq!(actual[at+i].to_bits(),value(v).to_bits(),"{label} slot {}: {:?} != {:?}",at+i,actual[at+i],value(v));}
    }
}
#[test]
fn frozen_camera_publications(){
    let fixtures:Value=serde_json::from_str(include_str!("camera-facts.json")).unwrap();let mut cameras=Cameras::default();
    for case in fixtures["cases"].as_array().unwrap(){
        let step=case["step"].as_u64().unwrap();
        for(i,v)in case["input"].as_array().unwrap().iter().enumerate(){cameras.input[i]=value(v);}
        cameras.main();cameras.derived(R::FAR);cameras.derived(R::VIEW);cameras.cascades();cameras.reflect();cameras.probe(0);
        for i in 0..4{cameras.cascades.publish(i);}
        for row in case["rows"].as_array().unwrap(){let role=row[0].as_u64().unwrap() as usize;check(&cameras.output[role*C::LEN..(role+1)*C::LEN],&row[1],&format!("sample {step} camera {role}"));}
        for(i,row)in case["windows"].as_array().unwrap().iter().enumerate(){check(&cameras.cascades.output[i*W::LEN..(i+1)*W::LEN],row,&format!("sample {step} window {i}"));}
        for(i,v)in case["mirror"].as_array().unwrap().iter().enumerate(){assert_eq!(cameras.mirror[i].to_bits(),value(v).to_bits(),"sample {step} mirror {i}");}
        cameras.cascades.finish(&[1.;4]);
    }
}
#[test]
fn bounded_stores_survive_paused_bursts_and_restore(){
    let mut cameras=Cameras::default();cameras.main();let original=cameras.output.clone();
    cameras.input[I::SHIFT]=0.000125;cameras.input[I::SHIFT+1]=-0.000225;
    for role in 0..R::COUNT+8{cameras.copy(R::MAIN,role);}
    for role in 0..R::COUNT+8{cameras.shift(role);}cameras.restore();cameras.restore();
    for role in 0..R::COUNT{assert_eq!(&cameras.output[role*C::LEN+C::PROJECTION..role*C::LEN+C::PROJECTION+16],&original[C::PROJECTION..C::PROJECTION+16]);}
    cameras.input[I::LIGHT_COUNT]=1e12;cameras.input[I::LIGHT_LAYERS]=1.;cameras.input[I::LIGHT_FAR]=1000.;
    for i in 0..L::RAW_LIGHTS{let at=i*P::LEN;cameras.light_input[at+P::COLOR]=1.;cameras.light_input[at+P::INTENSITY]=1.;cameras.light_input[at+P::LAYERS]=1.;cameras.light_input[at+P::SHOWN]=1.;}
    let pointers=(cameras.output.as_ptr(),cameras.packs.as_ptr(),cameras.light_input.as_ptr());
    for _ in 0..10000{cameras.lights(0);cameras.sky(R::MAIN,true);cameras.probe(999);cameras.lights(999);}
    assert_eq!(cameras.packs[K::COUNT],16.);assert!(cameras.clipped);
    assert_eq!(pointers,(cameras.output.as_ptr(),cameras.packs.as_ptr(),cameras.light_input.as_ptr()));
}

// Packing retains separate views even when one scene draw nests another.
#[test]
fn frozen_point_light_packs(){
    let fixture:Value=serde_json::from_str(include_str!("camera-facts.json")).unwrap();let mut cameras=Cameras::default();
    for case in fixture["lights"].as_array().unwrap(){
        for(i,v)in case["input"].as_array().unwrap().iter().enumerate(){cameras.input[i]=value(v);}
        for(i,v)in case["rows"].as_array().unwrap().iter().enumerate(){cameras.light_input[i]=value(v);}
        cameras.lights(0);let retained=cameras.packs[..K::LEN].to_vec();
        for(i,v)in case["expected"].as_array().unwrap().iter().enumerate(){assert_eq!(cameras.packs[i].to_bits(),value(v).to_bits(),"light sample {} slot {i}",case["sample"]);}
        cameras.input[I::LIGHT_COUNT]=0.;cameras.lights(1);assert_eq!(&cameras.packs[..K::LEN],retained.as_slice());
    }
}

#[test]
fn frozen_global_lighting_and_flash(){
    let fixture:Value=serde_json::from_str(include_str!("light-settings.json")).unwrap();let mut lights=bfe_render::camera_lights::LightSettings::default();
    for case in fixture["cases"].as_array().unwrap(){
        for(i,v)in case["input"].as_array().unwrap().iter().enumerate(){lights.input[i]=value(v);}
        lights.command(case["command"].as_u64().unwrap() as usize);
        for(i,v)in case["expected"].as_array().unwrap().iter().enumerate(){assert_eq!(lights.output[i].to_bits(),value(v).to_bits(),"light command {} slot {i}",case["command"]);}
    }
}

// Count native allocations on this test thread after cold admission. Other
// parallel tests may parse their fixtures without affecting the observation.
use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
struct CountAlloc;
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self,layout:Layout)->*mut u8{let _=TRACK.try_with(|on|{if on.get(){let _=ALLOCS.try_with(|n|n.set(n.get()+1));}});System.alloc(layout)}
    unsafe fn dealloc(&self,p:*mut u8,layout:Layout){System.dealloc(p,layout)}
    unsafe fn realloc(&self,p:*mut u8,layout:Layout,size:usize)->*mut u8{let _=TRACK.try_with(|on|{if on.get(){let _=ALLOCS.try_with(|n|n.set(n.get()+1));}});System.realloc(p,layout,size)}
}
#[global_allocator]static ALLOCATOR:CountAlloc=CountAlloc;
#[test]
fn camera_publication_allocates_nothing_after_admission(){
    let mut cameras=Cameras::default();cameras.input[I::CASCADE_COUNT]=1e12;cameras.input[I::MAP_SIZE]=2048.;cameras.input[I::MAX_FAR]=500.;cameras.input[I::LIGHT_RANGE]=1.;cameras.input[I::LIGHT_RANGE+1]=3000.;cameras.input[I::MARGIN]=400.;cameras.input[I::SUN+1]=-1.;cameras.input[I::SKY_COUNT]=1e12;cameras.input[I::LIGHT_COUNT]=1e12;
    ALLOCS.with(|n|n.set(0));TRACK.with(|on|on.set(true));
    for _ in 0..1000{cameras.main();cameras.cascades();cameras.reflect();cameras.probe(0);cameras.lights(0);cameras.sky(R::MAIN,true);cameras.shift(R::MAIN);cameras.restore();}
    TRACK.with(|on|on.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
}

#[test]
fn signed_layer_masks_keep_the_high_bit(){
    let mut c=Cameras::default();c.input[I::LIGHT_COUNT]=2.;c.input[I::LIGHT_LAYERS]=-2147483648.;
    for i in 0..2{let at=i*P::LEN;c.light_input[at+P::SHOWN]=1.;c.light_input[at+P::INTENSITY]=1.;c.light_input[at+P::COLOR]=1.;c.light_input[at+P::LAYERS]=if i==0{-1.}else{1.};}
    c.lights(0);assert_eq!(c.packs[K::COUNT],1.);
}

// configure must retire old light matrices before mirrors, without advancing
// the receiver slices until updateCascades prepares the new main view.
#[test]
fn shadow_settings_preserve_publication_order(){
    use bfe_render::shader::{SHADOW_FLOATS,SHADOW_MATRICES,SHADOW_PLANES,SHADOW_SLICES};
    let fixture:Value=serde_json::from_str(include_str!("camera-facts.json")).unwrap();let mut c=Cameras::default();
    for(i,v)in fixture["cases"][0]["input"].as_array().unwrap().iter().enumerate(){c.input[i]=value(v);}
    c.main();c.cascades();let mut before=[0.;SHADOW_FLOATS];c.cascades.write_shadow(&mut before,&[0.;24]);
    let identity=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];assert_eq!(&before[SHADOW_MATRICES..SHADOW_MATRICES+16],&identity);
    c.cascades.publish(0);let mut after=[0.;SHADOW_FLOATS];c.cascades.write_shadow(&mut after,&[0.;24]);assert_ne!(&before[SHADOW_MATRICES..SHADOW_MATRICES+16],&after[SHADOW_MATRICES..SHADOW_MATRICES+16]);
    assert_eq!(&before[SHADOW_SLICES..],&after[SHADOW_SLICES..]);
    c.input[I::RESET]+=1.;c.input[I::CASCADE_COUNT]=2.;c.cascades.configure(&c.input);c.cascades.write_shadow(&mut after,&[0.;24]);
    assert_eq!(&after[SHADOW_MATRICES..SHADOW_MATRICES+16],&identity);assert_eq!(&before[SHADOW_SLICES..],&after[SHADOW_SLICES..]);
    assert_eq!(&after[SHADOW_PLANES..SHADOW_PLANES+12],&[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.]);
    c.cascades();c.cascades.write_shadow(&mut after,&[0.;24]);assert_eq!(&after[SHADOW_SLICES+8..SHADOW_SLICES+16],&[1.,1.,0.,0.,0.,1.,0.,0.]);
}

#[test]
fn camera_shader_layout_matches_the_existing_uniform_block(){
    use bfe_render::{cameras::layout::shader_frame as F,shader as S};
    assert_eq!(F::LEN,S::FRAME_FLOATS);assert_eq!(F::LIGHT_COUNT,S::FRAME_LIGHT_COUNT);assert_eq!(F::LIGHTS,S::FRAME_LIGHTS);
    assert_eq!([F::PROJECTION,F::VIEW,F::EYE,F::TIME,F::SUN_DIRECTION,F::BUMP,F::SUN_COLOR,F::SHADOW_STRENGTH,F::AMBIENT,F::AA_SAMPLES,F::AMBIENT_TOP,F::TERRAIN_SHADE,F::AMBIENT_BOTTOM,F::FAR_AA_SAMPLES,F::CHARACTER_AMBIENT,F::CASCADE_NEAR,F::FOG_COLOR,F::CASCADE_FAR,F::FADE,F::RESOLUTION,F::FOG_RANGE],S::FRAME_OFFSETS);
}

// A paused view can be requested by several auxiliary producers. Publication
// is shared until a bit changes, including the sign of an authored zero.
#[test]
fn unchanged_main_views_publish_once(){
    let mut c=Cameras::default();assert!(c.main());assert!(!c.main());
    c.input[I::POSITION]=-0.;assert!(c.main());assert!(!c.main());
    c.invalidate();assert!(c.main());
}

// A refused oblique plane must not discard the last usable projective map.
// The plain pose was still published before the old denominator guard.
#[test]
fn refused_mirrors_keep_the_preceding_reflection(){
    use bfe_render::cameras::layout::{lens as N,mirror as H};
    let mut c=Cameras::default();c.input[I::POSITION+1]=4.;c.input[I::WATER_LEVEL]=1.;c.main();c.reflect();assert_eq!(c.mirror[H::VALID],1.);
    let saved=c.mirror[H::REFLECTION..H::REFLECTION+16].to_vec();
    c.input[I::LENS+N::FOV]=f64::NAN;c.input[I::POSITION+1]=7.;c.main();c.reflect();assert_eq!(c.mirror[H::VALID],0.);
    assert_eq!(&c.mirror[H::REFLECTION..H::REFLECTION+16],saved.as_slice());
    for i in 0..C::LEN{assert_eq!(c.output[R::MIRROR*C::LEN+i].to_bits(),c.output[R::MIRROR_PLAIN*C::LEN+i].to_bits());}
}
