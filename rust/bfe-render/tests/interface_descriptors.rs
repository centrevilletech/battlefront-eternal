// Frozen interface/preview descriptors exercise the production cold compiler
// without authored game assets or a graphics context, so CI runs this gate.
use bfe_render::{actor_resources::{Geometry,Record,Stream},interface_resources::{Policy,Style}};
fn uints(v:&serde_json::Value)->Vec<u32>{v.as_array().unwrap().iter().map(|v|v.as_u64().unwrap()as u32).collect()}
#[test]
fn frozen_interface_procedural_streams(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/interface-descriptors.json")).unwrap();
 for shape in f["shapes"].as_array().unwrap(){let r:Record=serde_json::from_value(shape["record"].clone()).unwrap();let g=Geometry::new(&r,&[],&[]).unwrap();for a in &g.attributes{let Stream::Float(v)=&g.streams[a.stream]else{panic!()};assert_eq!(v.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),uints(&shape["streams"][a.name]),"{} {}",r.kind,a.name);}assert_eq!(g.index,uints(&shape["index"]),"{} index",r.kind);assert_eq!(g.facts.index_width,shape["width"].as_u64().unwrap()as u32);for i in 0..10{assert_eq!(g.facts.bounds[i].to_bits(),shape["bounds"][i].as_f64().unwrap().to_bits(),"{} bound {i}",r.kind);}}
}
#[test]
fn frozen_interface_raster_and_catalog(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/interface-descriptors.json")).unwrap();for row in f["materials"].as_array().unwrap(){let style:Style=serde_json::from_value(row["record"].clone()).unwrap();let p=Policy::new(&style,false,false).unwrap();assert!(bfe_render::programs::spec(&p.key).is_ok());let e=&row["expected"];assert_eq!(p.blend.to_vec(),uints(&e["blend"]));assert_eq!(p.flags&3,e["state"]["flags"].as_u64().unwrap()as u32&3);assert_eq!(p.depth_func,e["state"]["func"].as_u64().unwrap()as u32);assert_eq!(p.side,row["record"]["side"].as_u64().unwrap()as u32);assert_eq!(p.force_single,e["forceSinglePass"].as_bool().unwrap());}
}
// Repeated class/scoreboard preview changes reuse bounded skin transport. The
// old qualified handle stays invalid after both disposal and slot reuse.
#[test]
fn preview_skin_release_reuses_storage_without_stale_handles(){
 let mut bank=bfe_render::actor_depth::Bank::default();let first=bank.skin(3,32).unwrap();let base=bank.get(first).unwrap().base;bank.release(first);let second=bank.skin(7,32).unwrap();assert_ne!(first,second);assert_eq!(bank.slot(second),Some(0));assert_eq!(bank.get(second).unwrap().base,base);bank.release(first);assert!(bank.get(second).is_some());
 for _ in 0..2000{let previous=bank.skins[0].generation;bank.release((previous<<bfe_render::actor_layout::limit::SKIN_INDEX_BITS)|0);let id=bank.skin(7,32).unwrap();assert!(bank.get(id).is_some());}assert_eq!(bank.skins.len(),1);
}
// Recorded before deleting InterfacePass.write, including mirrored nonuniform
// scale, bind transforms, scroll, point size and all fifteen catalog variants.
#[test]
fn frozen_interface_uniform_packets(){
 use bfe_render::{interface_resources::Descriptor,interface_resource_layout::{raw as R,look as L},interface_scene::layout as D};
 let fixture:serde_json::Value=serde_json::from_str(include_str!("fixtures/interface-uniforms.json")).unwrap();
 for case in fixture.as_array().unwrap(){
  let key=case["key"].as_str().unwrap();let tail=key.strip_prefix("interface/").unwrap();let style=Style{program:if tail.starts_with("model"){"model"}else if tail.starts_with("basic"){"basic"}else{tail}.into(),vertex_colors:tail.contains("color"),..Default::default()};
  let policy=Policy::new(&style,tail.contains("skin"),key!="interface/basic").unwrap();assert_eq!(policy.key,key);let descriptor=Descriptor::new(2,3,120,4,policy,&[17,23,31,1]).unwrap();let mut row=[0.;D::STRIDE];row[D::COUNT]=120.;
  for(name,at)in[("world",R::WORLD),("view",R::VIEW),("projection",R::PROJECTION),("bind",R::BIND),("inverse",R::INVERSE),("look",R::LOOK)]{for(i,v)in case[name].as_array().unwrap().iter().enumerate(){row[D::VALUES+at+i]=v.as_f64().unwrap();}}
  assert_eq!(case["look"].as_array().unwrap().len(),L::LEN);row[D::VALUES+R::PIXEL_RATIO]=case["ratio"].as_f64().unwrap();row[D::VALUES+R::HEIGHT]=case["height"].as_f64().unwrap();descriptor.publish(&mut row);
  for(i,v)in case["expected"].as_array().unwrap().iter().enumerate(){assert_eq!(row[D::VALUES+i].to_bits(),u64::from_str_radix(v.as_str().unwrap(),16).unwrap(),"{key} uniform word {i}");}
  row[D::FIRST]=f64::MAX;row[D::COUNT]=f64::MAX;descriptor.publish(&mut row);assert_eq!(row[D::COUNT],0.);
 }
}
