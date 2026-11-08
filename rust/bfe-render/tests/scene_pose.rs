//! Producer facts are checked before renderer uniforms or GPU conversion.
use bfe_render::scene_pose::ScenePoses;
use bfe_render::scene_pose_layout::{pose_header as h,pose_world as w,pose_skin as s,pose_bind as b,pose_tree_header as th,pose_tree_kind as tk,pose_tree_node as tn};
const ID:[f64;16]=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
fn packet(p:&mut ScenePoses,sequence:f64,x:f64){
 p.input[..h::STRIDE].fill(0.);p.input[h::OWNER]=1.;p.input[h::GENERATION]=1.;p.input[h::SEQUENCE]=sequence;
 p.input[h::WORLDS]=1.;p.input[h::SKINS]=1.;p.input[h::BINDS]=1.;
 let a=h::STRIDE;p.input[a+w::MODEL]=2.;p.input[a+w::GENERATION]=1.;p.input[a+w::INDEX]=7.;
 p.input[a+w::WORLD..a+w::WORLD+16].copy_from_slice(&ID);p.input[a+w::WORLD+12]=x;p.input[a+w::WORLD+13]=-0.;
 let a=a+w::STRIDE;p.input[a+s::MODEL]=2.;p.input[a+s::GENERATION]=1.;p.input[a+s::INDEX]=3.;
 for i in 0..8{p.input[a+s::BITS+i]=f64::from_bits(((0x7fc00042u64)<<32)|0x80000000);}
 let a=a+s::STRIDE;p.input[a+b::MODEL]=2.;p.input[a+b::GENERATION]=1.;
 p.input[a+b::INVERSE..a+b::INVERSE+16].copy_from_slice(&ID);p.input[a+b::INVERSE+12]=-x;
 p.input[h::WORDS]=(a+b::STRIDE)as f64;
}
#[test]
fn whole_phase_validation_preserves_f64_and_raw_f32_publications(){
 let mut p=ScenePoses::default();assert!(p.register(1,1,2,1,7,2,3,1));packet(&mut p,1.,0.125);assert_eq!(p.apply(),3);
 let world=p.node(1,2,1,7).unwrap().world;assert_eq!(world[12].to_bits(),0.125f64.to_bits());assert_eq!(world[13].to_bits(),(-0.0f64).to_bits());
 let skin=p.skin(1,2,1,3).unwrap();assert_eq!(skin[0].to_bits(),0x80000000);assert_eq!(skin[1].to_bits(),0x7fc00042);
 assert_eq!(p.bind(1,2,1).unwrap()[12],-0.125);
 // A malformed late record cannot commit an earlier valid world or consume
 // its sequence. Correcting that packet at the same sequence must succeed.
 packet(&mut p,2.,0.375);p.input[h::STRIDE+w::STRIDE+s::STRIDE+b::MODEL]=99.;assert_eq!(p.apply(),0);assert_eq!(p.node(1,2,1,7).unwrap().world,world);
 packet(&mut p,2.,0.375);assert_eq!(p.apply(),3);assert_eq!(p.node(1,2,1,7).unwrap().world[12],0.375);assert_eq!(p.apply(),0);
 assert!(!p.register(1,0,2,0,7,2,3,1));assert!(p.register(1,2,2,2,7,2,3,1));assert_eq!(p.apply(),0);assert!(p.node(1,2,2,7).is_none());
}
#[test]
fn contiguous_prop_worlds_bind_and_skins_match_original_f64_order(){
 let mut p=ScenePoses::default();assert!(p.register(1,1,2,1,0,2,0,1));
 p.reserve_stream(2*tn::STRIDE);assert!(p.register_skin(1,2,1,0,0,0,&[1],&ID));
 p.stream_info[th::OWNER]=1.;p.stream_info[th::GENERATION]=1.;p.stream_info[th::MODEL]=2.;p.stream_info[th::WORDS]=(2*tn::STRIDE) as f64;p.stream_info[th::COUNT]=2.;p.stream_info[th::KIND]=tk::TREE as f64;
 for i in 0..2{let a=i*tn::STRIDE;p.stream[a+tn::WORLD..a+tn::WORLD+16].copy_from_slice(&ID);p.stream[a+tn::BIND_INVERSE..a+tn::BIND_INVERSE+16].copy_from_slice(&ID);p.stream[a+tn::CHANGED]=1.;}
 p.stream[tn::STRIDE+tn::WORLD+12]=0.12345678901234567;
 p.stream[tn::BIND_INVERSE+12]=-0.75;assert_eq!(p.import_stream(),2);
 let expected=bfe_render::math::mul(&p.node(1,2,1,1).unwrap().world,&ID);
 assert_eq!(p.skin(1,2,1,0).unwrap().iter().map(|x|x.to_bits()).collect::<Vec<_>>(),expected.iter().map(|x|(*x as f32).to_bits()).collect::<Vec<_>>());
 assert_eq!(p.node(1,2,1,0).unwrap().inverse.unwrap()[12],-0.75);assert_eq!(p.import_stream(),0);
 p.stream_info[th::GENERATION]=0.;p.stream[tn::WORLD+12]=9.;assert_eq!(p.import_stream(),0);assert_eq!(p.node(1,2,1,0).unwrap().world[12],0.);
 p.reserve_stream(usize::MAX);assert!(!p.register(usize::MAX,1,2,1,0,2,0,1));
}
