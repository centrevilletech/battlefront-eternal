//! Light graph facts preserve the old discovery clock and view snapshots.
use bfe_render::{scene::{SceneRegistry,layout::{scene_node as s,scene_flags as f}},cameras::{Cameras,layout::{point_change as q,input as i,pack as p,role as r}}};
const ID:[f64;16]=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
fn node(scene:&mut SceneRegistry,id:usize,parent:f64,visible:bool,x:f64){
 let a=&mut scene.changes[..s::STRIDE];a.fill(0.);a[s::SLOT]=id as f64;a[s::GENERATION]=1.;a[s::PARENT]=parent;a[s::PARENT_GENERATION]=1.;a[s::SIBLING]=id as f64;a[s::FLAGS]=(f::ALIVE|if visible{f::VISIBLE}else{0}) as f64;a[s::LAYERS]=1.;a[s::WORLD..s::WORLD+16].copy_from_slice(&ID);a[s::WORLD+12]=x;scene.apply(1);
}
fn point(scene:&mut SceneRegistry,id:usize,color:[f64;3],intensity:f64){
 let a=&mut scene.points.changes[..q::LEN];a.fill(0.);a[q::SLOT]=id as f64;a[q::GENERATION]=1.;a[q::COLOR..q::COLOR+3].copy_from_slice(&color);a[q::INTENSITY]=intensity;a[q::RANGE]=10.;scene.points_apply(1);
}
fn pack(scene:&mut SceneRegistry,camera:&mut Cameras,root:usize,view:bool,now:f64){
 camera.input[i::LIGHT_LAYERS]=1.;scene.point_lights_frame(camera,root,1.,r::MAIN,0,1.,view,now,0);
}
#[test]
fn light_discovery_keeps_cached_detached_rows_until_strict_refresh_boundary(){
 let mut scene=SceneRegistry::new(16);let mut camera=Cameras::default();camera.main();
 node(&mut scene,0,-1.,true,0.);node(&mut scene,1,0.,true,1.);node(&mut scene,2,0.,false,0.);node(&mut scene,3,2.,true,3.);
 point(&mut scene,1,[0.25,0.5,1.],2.);point(&mut scene,3,[1.,0.,0.],1.);scene.point_root(0,1.);pack(&mut scene,&mut camera,0,false,0.);
 assert_eq!(camera.packs[p::COUNT],1.);assert_eq!(&camera.packs[p::VALUES..p::VALUES+8],&[1.,0.,0.,10.,0.5,1.,2.,0.]);
 node(&mut scene,1,-1.,true,1.);node(&mut scene,4,0.,true,4.);point(&mut scene,4,[0.,1.,0.],1.);
 pack(&mut scene,&mut camera,0,false,500.);assert_eq!(camera.packs[p::COUNT],1.);assert_eq!(camera.packs[p::VALUES],1.);
 pack(&mut scene,&mut camera,0,false,500.0000001);assert_eq!(camera.packs[p::COUNT],1.);assert_eq!(camera.packs[p::VALUES],4.);
 node(&mut scene,2,0.,true,0.);pack(&mut scene,&mut camera,0,false,501.);assert_eq!(camera.packs[p::COUNT],2.);assert_eq!(camera.packs[p::VALUES],3.);
}
#[test]
fn first_person_sources_keep_admitted_visibility_and_layers_until_identity_changes(){
 let mut scene=SceneRegistry::new(16);let mut camera=Cameras::default();camera.main();
 node(&mut scene,0,-1.,true,0.);node(&mut scene,1,0.,true,1.);point(&mut scene,1,[1.,0.,0.],1.);
 node(&mut scene,6,-1.,true,5.);scene.point_root(6,1.);pack(&mut scene,&mut camera,6,true,0.);
 assert_eq!(camera.packs[p::COUNT],1.);assert_eq!(camera.packs[p::VALUES],6.);
 node(&mut scene,1,0.,false,1.);pack(&mut scene,&mut camera,6,true,1.);assert_eq!(camera.packs[p::COUNT],0.);
 node(&mut scene,1,0.,true,1.);scene.changes[s::LAYERS]=2.;scene.apply(1);pack(&mut scene,&mut camera,6,true,2.);assert_eq!(camera.packs[p::COUNT],1.,"current source layers do not replace the snapshot");
 node(&mut scene,2,0.,true,2.);point(&mut scene,2,[0.,1.,0.],1.);pack(&mut scene,&mut camera,6,true,3.);assert_eq!(camera.packs[p::COUNT],1.);assert_eq!(camera.packs[p::VALUES],7.,"identity change snapshots the new source list");
}
