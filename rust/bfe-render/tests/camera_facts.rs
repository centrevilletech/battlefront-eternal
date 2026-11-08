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
        cameras.settings.output[..3].copy_from_slice(&cameras.input[I::SUN..I::SUN+3]);
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

// setFog's admission rule belongs to the same native settings owner that
// writes shader inputs. Invalid ranges disable fog while keeping map colour.
#[test]
fn far_fog_ranges_and_sparse_environment_commands() {
    use bfe_render::cameras::layout::{light_input as I,light_output as O,light_command as C,light_flag as F};
    let mut lights=bfe_render::camera_lights::LightSettings::default();
    for (start,end,enabled) in [(80.,430.,true),(0.,1.,true),(9000.,8000.,false),(500.,500.,false),(-1.,100.,false),(0.,0.,false),(f64::NAN,100.,false)] {
        lights.input[I::FLAGS]=1.;lights.input[I::RANGE]=start;lights.input[I::RANGE+1]=end;
        lights.input[I::COLORS+15..I::COLORS+18].copy_from_slice(&[64.,128.,255.]);
        lights.command(C::FOG);
        let expected=if enabled {[start,end]}else{[1e9,2e9]};
        assert_eq!(&lights.output[O::RANGE..O::RANGE+2],&expected);
        assert_eq!(&lights.output[O::COLORS+15..O::COLORS+18],&[64./255.,128./255.,1.]);
        lights.input[I::FLAGS]=F::RANGE as f64;lights.command(C::ENVIRONMENT);
        assert_eq!(&lights.output[O::RANGE..O::RANGE+2],&expected);
    }
    // Missing map fog clears the range, and keeps the preceding fog colour.
    lights.input[I::FLAGS]=0.;lights.input[I::COLORS+15..I::COLORS+18].fill(0.);lights.command(C::FOG);
    assert_eq!(&lights.output[O::RANGE..O::RANGE+2],&[1e9,2e9]);
    assert_eq!(&lights.output[O::COLORS+15..O::COLORS+18],&[64./255.,128./255.,1.]);
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
    let mut cameras=Cameras::default();cameras.input[I::CASCADE_COUNT]=1e12;cameras.input[I::MAP_SIZE]=2048.;cameras.input[I::MAX_FAR]=500.;cameras.input[I::LIGHT_RANGE]=1.;cameras.input[I::LIGHT_RANGE+1]=3000.;cameras.input[I::MARGIN]=400.;cameras.settings.output[1]=-1.;cameras.input[I::SKY_COUNT]=1e12;cameras.input[I::LIGHT_COUNT]=1e12;
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

#[test]
fn view_light_metadata_survives_visibility_changes_without_proxy_objects(){
    let mut c=Cameras::default();c.input[I::LIGHT_COUNT]=1.;c.input[I::LIGHT_LAYERS]=1.;c.input[I::LIGHT_FAR]=200.;
    c.input[I::VIEW_WORLD..I::VIEW_WORLD+16].copy_from_slice(&[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,2.,3.,4.,1.]);
    c.light_input[P::SHOWN]=1.;c.light_input[P::LAYERS]=1.;c.light_input[P::COLOR]=0.5;c.light_input[P::INTENSITY]=2.;
    c.view_light_snapshot(1);c.light_input[P::LAYERS]=2.;c.view_lights(0,0);
    assert_eq!(c.packs[K::COUNT],1.);assert_eq!(&c.packs[K::VALUES..K::VALUES+3],&[2.,3.,4.]);
    c.light_input[P::SHOWN]=0.;c.view_lights(0,0);assert_eq!(c.packs[K::COUNT],0.);
    c.light_input[P::SHOWN]=1.;c.view_light_snapshot(1);c.view_lights(0,0);assert_eq!(c.packs[K::COUNT],0.,"new source list snapshots current layers");
    c.view_light_snapshot(usize::MAX);c.view_lights(0,usize::MAX);assert!(c.clipped);
}

#[test]
fn conservative_placement_uses_the_unshifted_native_camera(){
    use bfe_render::cameras::layout::placement as a;
    let mut c=Cameras::default();c.main();
    c.placement[a::POSITION+2]=-10.;c.placement[a::LEN+a::POSITION]=100.;c.placement[a::LEN+a::POSITION+2]=-10.;
    c.placement(2);assert_eq!(c.placement[a::HIDDEN],0.);assert_eq!(c.placement[a::LEN+a::HIDDEN],1.);
    c.placement(usize::MAX);assert!(c.clipped);
}

// Thousands of map/view identities must retain lights without depending on
// host garbage collection. Recycled generations cannot mutate a newer pair.
#[test]
fn light_pack_leases_recycle_and_reject_retained_pairs(){
    let mut c=Cameras::default();c.input[I::LIGHT_COUNT]=1.;c.input[I::LIGHT_LAYERS]=1.;
    c.light_input[P::SHOWN]=1.;c.light_input[P::LAYERS]=1.;c.light_input[P::INTENSITY]=1.;c.light_input[P::COLOR]=0.75;
    let first=c.pack_leases.acquire();c.pack_lights(first);
    let pointers=(c.packs.as_ptr(),c.pack_leases.handles.as_ptr());
    ALLOCS.with(|n|n.set(0));TRACK.with(|on|on.set(true));
    let mut last=first;
    for _ in 0..10000{last=c.pack_leases.acquire();c.pack_lights(last);let slot=(last%L::PACKS as f64) as usize;assert_eq!(c.packs[slot*K::LEN+K::COUNT],1.);}
    TRACK.with(|on|on.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
    assert_eq!(pointers,(c.packs.as_ptr(),c.pack_leases.handles.as_ptr()));
    assert!(!c.pack_leases.release(first));assert!(c.pack_leases.resolve(first).is_none());
    c.pack_lights(first);assert_eq!(c.rejected_packs,1);
    assert!(c.pack_leases.release(last));let replacement=c.pack_leases.acquire();
    assert_eq!(replacement%L::PACKS as f64,last%L::PACKS as f64);assert_ne!(replacement,last);
    assert!(!c.pack_leases.release(last));c.pack_lights(replacement);
    assert_eq!(c.packs[(replacement%L::PACKS as f64) as usize*K::LEN+K::COUNT],1.);
}

// Independent preview cameras use the same native owner, including transformed
// parents, authored local matrices and orthographic view offsets. Expected
// bits were frozen from Camera.updateMatrixWorld/updateProjectionMatrix.
#[test]
fn independent_cameras_match_frozen_matrix_publications(){
    let fixture:Value=serde_json::from_str(include_str!("independent-cameras.json")).unwrap();let mut c=Cameras::default();
    for case in fixture.as_array().unwrap(){
        for pair in case["input"].as_array().unwrap(){let at=match pair[0].as_str().unwrap(){"hasParent"=>I::HAS_PARENT,"lens"=>I::LENS,"localMatrix"=>I::LOCAL_MATRIX,"orthoBounds"=>I::ORTHO_BOUNDS,"orthographic"=>I::ORTHOGRAPHIC,"parentWorld"=>I::PARENT_WORLD,"position"=>I::POSITION,"published"=>I::PUBLISHED,"quaternion"=>I::QUATERNION,"scale"=>I::SCALE,"up"=>I::UP,"useLocalMatrix"=>I::USE_LOCAL_MATRIX,_=>panic!("unknown frozen camera field")};for(i,v)in pair[1].as_array().unwrap().iter().enumerate(){c.input[at+i]=value(v);}}
        let handle=c.extra_leases.acquire();let role=c.import(handle).unwrap();
        for(at,matrix)in [C::WORLD,C::INVERSE,C::PROJECTION,C::PROJECTION_INVERSE].into_iter().zip(case["matrices"].as_array().unwrap()){
            for(i,v)in matrix.as_array().unwrap().iter().enumerate(){assert_eq!(c.output[role*C::LEN+at+i].to_bits(),value(v).to_bits(),"{} matrix {at}/{i}",case["label"]);}
        }
    }
    let stale=c.extra_leases.acquire();for _ in 0..10000{let h=c.extra_leases.acquire();assert!(c.import(h).is_some());}
    assert!(c.import(stale).is_none());assert!(!c.extra_leases.release(stale));assert!(c.clipped);
}

// A stalled producer can issue more requests than any map without reserving
// another row. The next view starts with an empty marker and the same storage.
#[test]
fn depth_marker_requests_allocate_nothing_during_a_long_pause(){
 use bfe_render::cameras::depth_marker::DepthMarker;
 let mut marker=DepthMarker::default();let mut screen=[0.;16];screen[0]=1.;screen[5]=1.;screen[10]=1.;screen[15]=1.;
 marker.begin(&screen);ALLOCS.with(|n|n.set(0));TRACK.with(|on|on.set(true));
 for i in 0..100000{marker.key=[0.,0.,i as f64*0.125,1.,-4.,i as f64];marker.request();}marker.finish();
 TRACK.with(|on|on.set(false));assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(marker.output[bfe_render::cameras::layout::depth_marker::REQUESTS],100000.);
 marker.begin(&screen);marker.finish();assert_eq!(marker.output[bfe_render::cameras::layout::depth_marker::VISIBLE],0.);
}
