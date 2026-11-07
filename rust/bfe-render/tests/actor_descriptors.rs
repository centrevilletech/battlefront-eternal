// Synthetic frozen buildActor/modelPack/partMaterial descriptors. These cold
// byte/state fixtures run in CI without authored game content or a GL context.
use bfe_render::actor_resources::{Record,Geometry,Stream,Style,Material};
fn floats(v:&serde_json::Value)->Vec<f32>{v.as_array().unwrap().iter().map(|v|f32::from_bits(v.as_u64().unwrap() as u32)).collect()}
fn uints(v:&serde_json::Value)->Vec<u32>{v.as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect()}
#[test]
fn frozen_actor_and_model_bytes(){
 let frozen:serde_json::Value=serde_json::from_str(include_str!("fixtures/actor-descriptors.json")).unwrap();
 for body in frozen["bodies"].as_array().unwrap(){let record:Record=serde_json::from_value(body["record"].clone()).unwrap();let g=Geometry::new(&record,&floats(&body["vertices"]),&uints(&body["indices"])).unwrap();
  for a in &g.attributes{let expected=&body["expected"]["streams"][a.name]["data"];let actual:Vec<u32>=match &g.streams[a.stream]{Stream::Float(v)=>v.iter().map(|v|v.to_bits()).collect(),Stream::Unsigned(v)=>v.iter().map(|v|*v as u32).collect()};assert_eq!(actual,uints(expected),"{}",a.name);}
  assert_eq!(g.index,uints(&body["expected"]["index"]));assert_eq!(g.facts.index_width,body["expected"]["indexWidth"].as_u64().unwrap() as u32);
  assert_eq!(serde_json::to_value(&g.facts.groups).unwrap(),body["expected"]["groups"]);
  for k in 0..10{assert_eq!(g.facts.bounds[k].to_bits(),body["expected"]["bounds"][k].as_f64().unwrap().to_bits());}
 }
 let model=&frozen["model"];let record:Record=serde_json::from_value(model["record"].clone()).unwrap();let g=Geometry::new(&record,&floats(&model["vertices"]),&uints(&model["indices"])).unwrap();
 let Stream::Float(data)=&g.streams[0]else{panic!()};assert_eq!(data.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),uints(&model["expected"]["data"]));assert_eq!(g.index,uints(&model["expected"]["index"]));
 for k in 0..10{assert_eq!(g.facts.bounds[k].to_bits(),model["expected"]["bounds"][k].as_f64().unwrap().to_bits());}
}
#[test]
fn frozen_model_material_flags_and_catalog(){
 let frozen:serde_json::Value=serde_json::from_str(include_str!("fixtures/actor-descriptors.json")).unwrap();
 for row in frozen["materials"].as_array().unwrap(){let style=Style{flags:Some(row["flags"].as_u64().unwrap() as u32),scroll:serde_json::from_value(row["scroll"].clone()).unwrap(),..Style::default()};let m=Material::policy(&style).unwrap();let e=&row["expected"];
  assert_eq!(m.flags&3,e["side"].as_u64().unwrap() as u32);assert_eq!(m.flags&4!=0,e["alphaToCoverage"].as_bool().unwrap());assert_eq!(m.flags&8!=0,e["depthWrite"].as_bool().unwrap());assert_eq!(m.blended,e["transparent"].as_bool().unwrap());
  let expected:Vec<(String,String)>=serde_json::from_value(e["defines"].clone()).unwrap();assert_eq!(m.defines,expected.into_iter().filter(|(n,_)|n!="ADDITIVE").collect::<Vec<_>>());if let Ok(selected)=Material::new(&style,false,0,false,false){assert!(bfe_render::programs::spec(&selected.key).is_ok());}
 }
}
#[test]
fn corrupt_actor_sources_are_rejected_before_slicing(){
 let bad=Record{kind:"model".into(),parts:vec![bfe_render::actor_resources::Part{vertex_count:usize::MAX,..Default::default()}],..Default::default()};assert!(Geometry::new(&bad,&[],&[]).is_err());
 let bad=Record{kind:"rig".into(),positions:vec![f32::NAN,0.,0.],..Default::default()};assert!(Geometry::new(&bad,&[],&[]).is_err());
}
#[test]
fn frozen_crowd_geometry_and_visible_prefix(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/crowd-descriptors.json")).unwrap();let b=&f["body"];let m=&f["model"];
 let body=Geometry::new(&serde_json::from_value(b["record"].clone()).unwrap(),&floats(&b["vertices"]),&uints(&b["indices"])).unwrap();let model=Geometry::new(&serde_json::from_value(m["record"].clone()).unwrap(),&floats(&m["vertices"]),&uints(&m["indices"])).unwrap();
 let mut crowd=bfe_render::actor_crowd::Crowd::default();let mut target=bfe_render::actor_crowd::Crowd::geometry();
 for i in 0..2{let matrix:Vec<f64>=serde_json::from_value(f["matrices"][i].clone()).unwrap();let parts=[(&body,i*2,None,0),(&model,i*2+1,Some(matrix.try_into().unwrap()),1)];let (spans,_)=crowd.append(&mut target,&parts,(i+1)*2).unwrap();assert_eq!(spans,vec![(i*2) as u32,(i*2+1) as u32]);}
 use bfe_render::actor_layout::crowd_input as I;crowd.input[I::FLAGS]=1.;crowd.input[I::FLAGS+1]=1.;assert!(crowd.pack(&mut target));assert_eq!(crowd.drawn,f["drawnIndices"].as_u64().unwrap() as usize);assert_eq!(target.index,uints(&f["drawn"]));
 for (index,name)in[(0,"position"),(1,"joints"),(2,"weights")]{let actual:Vec<u32>=match &target.streams[index]{Stream::Float(v)=>v.iter().map(|v|v.to_bits()).collect(),Stream::Unsigned(v)=>v.iter().map(|v|*v as u32).collect()};assert_eq!(actual,uints(&f[name]),"{name}");}
 assert_eq!(crowd.full,uints(&f["index"])[..crowd.full.len()]);assert!(!crowd.pack(&mut target));
}
#[test]
fn frozen_shield_spheres_and_hologram_beam(){
 let fixtures:serde_json::Value=serde_json::from_str(include_str!("fixtures/procedural-descriptors.json")).unwrap();
 for f in fixtures.as_array().unwrap(){let g=Geometry::new(&serde_json::from_value(f["record"].clone()).unwrap(),&[],&[]).unwrap();for a in &g.attributes{let Stream::Float(v)=&g.streams[a.stream]else{panic!()};assert_eq!(v.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),uints(&f["streams"][a.name]),"{} {}",f["record"]["kind"],a.name);}assert_eq!(g.index,uints(&f["index"]));assert_eq!(g.facts.index_width,f["width"].as_u64().unwrap()as u32);for k in 0..10{assert_eq!(g.facts.bounds[k].to_bits(),f["bounds"][k].as_f64().unwrap().to_bits(),"bound {k}");}}
}
#[test]
fn every_actor_material_cell_selects_native_catalog(){
 for kind in ["dynamic","unlit"]{for(alpha,blending)in[(false,None),(true,None),(false,Some("normal".to_owned()))]{for scroll in [None,Some([0.25,-0.5])]{for heated in [false,true]{for skin in [false,true]{for cascades in [0,2,3,4]{for lamps in [false,true]{let style=Style{kind:kind.into(),alpha_test:alpha,blending:blending.clone(),scroll,heated,..Default::default()};let m=Material::new(&style,skin,cascades,lamps,false).unwrap();assert!(bfe_render::programs::spec(&m.key).is_ok());}}}}}}}
}
#[test]
fn glow_and_shield_policies_use_existing_catalog(){
 use bfe_render::{actor_effects::{Policy,Look},effect_layout as e};
 for kind in [e::ACTOR_GLOW,e::ACTOR_BASIC,e::ACTOR_SHIELD,e::ACTOR_AURA]{for shadows in [false,true]{let p=Policy::new(kind,shadows,false,0,&Look::default()).unwrap();for(key,_)in &p.keys{assert!(bfe_render::programs::spec(key).is_ok(),"{key}");}assert_eq!(p.defaults.len(),e::PRODUCERS[kind].1.iter().map(|(_,n)|*n as usize).sum::<usize>());}}
}
#[test]
fn frozen_instanced_motion_bounds(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/flyer-bounds.json")).unwrap();let g=Geometry::new(&serde_json::from_value(f["record"].clone()).unwrap(),&[],&[]).unwrap();let matrices=f["matrices"].as_array().unwrap().iter().map(|v|v.as_f64().unwrap()as f32).collect::<Vec<_>>();let actual=bfe_render::actor_resources::flyer_bounds(&g.facts.bounds,&matrices);for i in 0..4{assert_eq!(actual[i].to_bits(),f["bounds"][i].as_f64().unwrap().to_bits());}
}

// vehicleBatches already publishes native bind-space streams. Retaining those
// exact bytes includes negative-zero UVs and its expanded authored sphere.
#[test]
fn frozen_vehicle_rig_streams(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/rig-descriptors.json")).unwrap();let g=Geometry::new(&serde_json::from_value(f["record"].clone()).unwrap(),&[],&[]).unwrap();
 for a in &g.attributes{let actual:Vec<u32>=match &g.streams[a.stream]{Stream::Float(v)=>v.iter().map(|v|v.to_bits()).collect(),Stream::Unsigned(v)=>v.iter().map(|v|*v as u32).collect()};assert_eq!(actual,uints(&f["streams"][a.name]),"{}",a.name);}
 assert_eq!(g.index,uints(&f["index"]));for i in 0..10{assert_eq!(g.facts.bounds[i].to_bits(),f["bounds"][i].as_f64().unwrap().to_bits());}
}
