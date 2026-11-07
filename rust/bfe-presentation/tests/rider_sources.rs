//! Native joins must retain lease birth coordinates while live source frames
//! advance, and a busy phase must never allocate another skeleton or sample.
use bfe_presentation::{rider_sources::{Bindings,Vehicles,Props,Source},layout::{unit_input as i,unit_frame as f}};
use std::{alloc::{GlobalAlloc,Layout,System},cell::{Cell,RefCell},rc::Rc};
use three::{Matrix4,Vector3};
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static COUNT:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
 unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){COUNT.with(|n|n.set(n.get()+1));}System.alloc(l)}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){COUNT.with(|c|c.set(c.get()+1));}System.realloc(p,l,n)}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
#[test]
fn leases_keep_the_native_source_revision_without_frame_allocation(){
 let vehicles=Rc::new(RefCell::new(Vehicles::default()));let props=Rc::new(RefCell::new(Props::default()));
 let mut bindings=Bindings::default();bindings.reserve(256);bindings.bind(&vehicles,&props);
 let mut input=vec![0.0;f::STRIDE+256*i::STRIDE];input[f::ACTORS]=256.0;
 vehicles.borrow_mut().rig(1);let mut source=Source{shown:true,age:2.0,pitch:0.35,yaw:0.4,velocity:Vector3::new(2.0,3.0,4.0),position:Vector3::new(7.0,8.0,9.0),..Default::default()};
 source.frame=Matrix4::default();source.frame.elements[12]=7.0;
 vehicles.borrow_mut().epoch=1.0;vehicles.borrow_mut().publish(0,0,source);
 // The current hull advances before the body consumes the newly arrived lease.
 source.frame.elements[12]=25.0;source.age=9.0;source.pitch=0.7;
 vehicles.borrow_mut().epoch=2.0;vehicles.borrow_mut().publish(0,0,source);
 for k in 0..256 {let x=&mut input[f::STRIDE+k*i::STRIDE..][..i::STRIDE];x[i::ACTOR]=k as f64;x[i::GENERATION]=1.0;x[i::SOURCE_PROP]=-1.0;x[i::SOURCE_TURRET]=-1.0;
  x[i::RIDER_KIND]=1.0;x[i::SOURCE_VERSION]=1.0;x[i::RIDER_RESET]=1.0;}
 bindings.fill(&mut input);
 let x=&input[f::STRIDE..];assert_eq!(x[i::LEASE_POSITION],7.0);assert_eq!(x[i::RIDER_PITCH],0.35);assert_eq!(x[i::RIDER_TIME],2.0);assert_eq!(x[i::RIDER_MATRIX+12],25.0);
 // Retained birth samples survive arbitrary live history turnover.
 COUNT.with(|c|c.set(0));TRACK.with(|t|t.set(true));
 for frame in 3..50 {vehicles.borrow_mut().epoch=frame as f64;vehicles.borrow_mut().publish(0,0,source);
  for k in 0..256{input[f::STRIDE+k*i::STRIDE+i::RIDER_RESET]=0.0;}bindings.fill(&mut input);}
 TRACK.with(|t|t.set(false));assert_eq!(COUNT.with(Cell::get),0);
 assert_eq!(input[f::STRIDE+i::RIDER_TIME],2.0);
 // Invalid foreign handles and oversized counts produce a bounded prefix.
 input[f::ACTORS]=f64::INFINITY;input[f::STRIDE+i::SOURCE_RIG]=f64::INFINITY;bindings.fill(&mut input);
}

// The page has no authority body owner. Its coarse collision phase must
// join the same native source and anatomical clip used by held/rider poses.
#[test]
fn replica_hit_shapes_follow_native_mount_and_hold_frames(){
 use bfe_presentation::{units::Units,unit_content::{Bone,ClipInput,Input,TableInput},layout::{unit_collision_input as c,unit_collision_output as o,unit_shape as s},rider_sources::Held};
 let names=[("DummyRoot",-1),("bone_root",0),("bone_pelvis",1),("bone_neck",2),("bone_head",3),("bone_l_thigh",2),("bone_l_calf",5),("bone_l_foot",6),("bone_r_thigh",2),("bone_r_calf",8),("bone_r_foot",9)];
 let bones=names.iter().map(|(name,parent)|Bone{name:(*name).into(),parent:*parent,pose:[0.0,0.2,0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]}).collect();
 let clip=ClipInput{frames:2,fps:30.0,root_motion:false,tracks:(0..11).collect(),data:(0..22).flat_map(|_|[0.0,0.0,0.0,1.0,0.0,0.2,0.0]).collect(),duration:1.0/30.0,speed:0.0,idle:true};
 let mut units=Units::new();let content=units.load(Input{bones,clips:vec![clip],tables:TableInput{sets:[[Some(0);9];3],alert:[Some(0);9],transitions:[[Some(0);3];3],jump:None,fall:None,land_soft:None,land_hard:None,dive:None,jet:None,thrown:[[None;4];2],upper:vec![]},walker:None});
 let identity=Matrix4::default().elements;units.actor(content,&[identity;11]);
 let vehicles=Rc::new(RefCell::new(Vehicles::default()));let props=Rc::new(RefCell::new(Props::default()));units.rider_sources.bind(&vehicles,&props);
 vehicles.borrow_mut().rig(1);let mut frame=Matrix4::default();frame.elements[12]=7.0;
 vehicles.borrow_mut().publish(0,0,Source{frame,..Default::default()});
 let x=&mut units.collision.input;x[c::ENABLED]=1.0;x[c::GENERATION]=1.0;x[c::SCALE]=1.0;x[c::SEAT]=1.0;x[c::EXPOSED]=1.0;x[c::SOURCE_TURRET]=-1.0;x[c::SOURCE_PROP]=-1.0;
 units.collision_frame();assert_eq!(units.collision.output[o::COUNT],6.0);
 let head=units.collision.shapes[s::CENTER];assert!(head>6.0);
 // Held output replaces the mount resource, without a matrix crossing JS.
 frame.elements[12]=20.0;props.borrow_mut().held.push(Held{placed:true,time:0.0,frame});
 units.collision.input[c::SOURCE_PROP]=0.0;units.collision.input[c::SEAT]=0.0;units.collision.input[c::TICK]=1.0;
 units.collision_frame();assert_eq!(units.collision.output[o::COUNT],6.0);assert_eq!(units.collision.shapes[s::CENTER]-head,13.0);
 // Repeated live observations retain all limb and accumulator capacity.
 COUNT.with(|c|c.set(0));TRACK.with(|t|t.set(true));
 for tick in 2..600 {units.collision.input[c::TICK]=tick as f64;units.collision_frame();}
 TRACK.with(|t|t.set(false));assert_eq!(COUNT.with(Cell::get),0);
}
