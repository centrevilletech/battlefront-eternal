//! Frozen Skeleton.update bytes precede upload padding and sampler conversion.
use bfe_render::{scene::{SceneRegistry,layout::{scene_node as n,scene_flags as f}},scene_transform_layout::{transform_change as c,transform_flags as tf,transform_phase as p,transform_state as s},cameras::{Cameras,layout::{input as i,role as r}}};
use serde_json::Value;
const ID:[f64;16]=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
fn number(v:&Value)->f64{let text=v.as_str().unwrap();let mut bytes=[0;8];for i in 0..8{bytes[i]=u8::from_str_radix(&text[i*2..i*2+2],16).unwrap();}f64::from_bits(u64::from_le_bytes(bytes))}
fn admit(scene:&mut SceneRegistry,id:usize,parent:f64,world:&[f64]){let a=&mut scene.changes[..n::STRIDE];a.fill(0.);a[n::SLOT]=id as f64;a[n::GENERATION]=1.;a[n::PARENT]=parent;a[n::PARENT_GENERATION]=if parent<0.{0.}else{1.};a[n::FLAGS]=(f::ALIVE|f::VISIBLE) as f64;a[n::WORLD..n::WORLD+16].copy_from_slice(world);scene.apply(1);}
#[test]
fn generic_skin_controls_and_pooled_replacement_match_frozen_bytes(){
 let cases:Value=serde_json::from_str(include_str!("skeleton-facts.json")).unwrap();let mut scene=SceneRegistry::new(4);admit(&mut scene,0,-1.,&ID);
 for case in cases.as_array().unwrap(){let world:Vec<f64>=case["world"].as_array().unwrap().iter().map(number).collect();let inverse:Vec<f64>=case["inverse"].as_array().unwrap().iter().map(number).collect();admit(&mut scene,1,0.,&world);
  assert!(scene.skeleton_register(0,1.,&[1],&[1],&inverse,&[1]));
  for visible in 0..2{assert!(scene.solve_skeleton_flags(0,1.,&[visible as f64]));let expected:Vec<u32>=case["words"][visible].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect();assert_eq!(scene.skeleton_skin(0,1.,0).unwrap().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected);}
 }
 // Replacement compacts retained storage; a long paused burst stays bounded.
 for _ in 0..10000{assert!(scene.skeleton_register(0,1.,&[1,1],&[1,1],&[ID,ID].concat(),&[0,0]));assert!(scene.skeleton_register(0,1.,&[1],&[1],&ID,&[0]));assert_eq!(scene.skeletons.values.len(),16);}
 assert!(!scene.skeleton_register(0,1.,&[1],&[2],&ID,&[0]));
}
#[test]
fn sky_restore_keeps_native_follow_position_through_authored_visibility(){
 let mut scene=SceneRegistry::new(2);admit(&mut scene,0,-1.,&ID);let a=&mut scene.transforms.changes[..c::STRIDE];a[c::SLOT]=0.;a[c::GENERATION]=1.;a[c::FLAGS]=(tf::AUTO_LOCAL|tf::AUTO_WORLD|tf::NATIVE_POSITION|tf::RETAINED) as f64;a[c::QUATERNION+3]=1.;a[c::SCALE..c::SCALE+3].fill(1.);a[c::POSITION+1]=2.;scene.transform_apply(1);
 let mut camera=Cameras::default();camera.input[i::POSITION+1]=7.;camera.input[i::SKY_COUNT]=1.;camera.main();camera.sky_nodes(&[0,1],u32::MAX,0);camera.save_sky(&scene);camera.scene_sky(&mut scene,r::MAIN,false);
 scene.transforms.phase[p::GENERATION]=1.;scene.transforms.phase[p::CHILDREN]=1.;scene.transform_frame();assert_eq!(scene.transforms.values[s::WORLD+13],7.);
 camera.restore_sky(&mut scene);assert_eq!(scene.transforms.values[s::WORLD+13],7.,"restoration waits for the ordinary update boundary");scene.transform_apply(1);scene.transform_frame();assert_eq!(scene.transforms.values[s::WORLD+13],2.);
}

// LiveObjects wrote quaternions while Object3D synchronized Euler through a
// temporary Matrix4. These frozen words cover every authored rotation order.
#[test]
fn quaternion_rotation_readbacks_match_all_frozen_orders(){
 let fixtures:Value=serde_json::from_str(include_str!("rotation-facts.json")).unwrap();let mut scene=SceneRegistry::new(1);
 for fixture in fixtures.as_array().unwrap(){let input:Vec<f64>=fixture["input"].as_array().unwrap().iter().map(number).collect();scene.quaternion_rotation(input.as_slice().try_into().unwrap(),fixture["order"].as_u64().unwrap() as usize);for(i,v)in fixture["output"].as_array().unwrap().iter().enumerate(){assert_eq!(scene.transforms.rotation[i].to_bits(),number(v).to_bits());}}
}

// Unit visibility reads the preceding camera publication. Preserve the raw
// normalized plane words before passing them to the native body phase.
#[test]
fn unit_frustums_match_the_frozen_projection_view_boundary(){
 let fixtures:Value=serde_json::from_str(include_str!("unit-frustums.json")).unwrap();
 for fixture in fixtures.as_array().unwrap(){let input:Vec<f64>=fixture["screen"].as_array().unwrap().iter().map(number).collect();let frustum=bfe_render::math::Frustum::from_matrix(input.as_slice().try_into().unwrap());for(i,plane)in frustum.planes.iter().enumerate(){for(k,actual)in [plane.n[0],plane.n[1],plane.n[2],plane.c].into_iter().enumerate(){assert_eq!(actual.to_bits(),number(&fixture["planes"][i*4+k]).to_bits());}}}
}

// An inactive crowd source can remain unplaced for many frames. Its original
// bind world still participates in world * zero, including signed-zero bytes.
#[test]
fn aggregate_skin_keeps_unplaced_native_sources_at_their_cold_bind_world(){
 use bfe_render::scene::layout::scene_pose_ref as r;
 let cases:Value=serde_json::from_str(include_str!("skeleton-facts.json")).unwrap();let fixture=&cases[7];let world:Vec<f64>=fixture["world"].as_array().unwrap().iter().map(number).collect();
 let mut scene=SceneRegistry::new(2);admit(&mut scene,0,-1.,&ID);admit(&mut scene,1,0.,&world);assert!(scene.poses.register(1,1,0,1,0,1,0,0));
 let row=&mut scene.changes[..n::STRIDE];row[n::POSE+r::OWNER]=1.;row[n::POSE+r::GENERATION]=1.;row[n::POSE+r::NODE_COUNT]=1.;scene.apply(1);
 let input=&mut scene.transforms.changes[..c::STRIDE];input[c::SLOT]=1.;input[c::GENERATION]=1.;input[c::FLAGS]=(tf::NATIVE|tf::STOP|tf::AUTO_WORLD) as f64;input[c::WORLD..c::WORLD+16].copy_from_slice(&world);scene.transform_apply(1);
 assert_eq!(scene.world(1).unwrap().as_slice(),world.as_slice());assert!(scene.skeleton_register(0,1.,&[1],&[1],&ID,&[1]));assert!(scene.solve_skeleton_flags(0,1.,&[0.]));
 let expected:Vec<u32>=fixture["words"][0].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect();assert_eq!(scene.skeleton_skin(0,1.,0).unwrap().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected);
 assert!(scene.poses.register(1,2,0,2,0,1,0,0));assert!(scene.world(1).is_none(),"a stale source generation cannot borrow the bind pose");
}
