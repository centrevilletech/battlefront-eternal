// Frozen Water.surface, farPass and ringMesh resources. These fixtures contain
// made-up water cells and patches, so every stream/raster check runs in CI.
use bfe_render::{water_resources as w,effect_resources::ring_topology,effect_layout as k};
use serde::Deserialize;
use sha2::{Digest,Sha256};
#[derive(Deserialize)]struct Geometry{position:Vec<u32>,uv:Vec<u32>,index:Vec<u32>,width:usize}
#[derive(Deserialize)]struct Material{kind:usize,far:bool,shadows:bool,key:String,blend:[u32;7],depth:u32,side:u32,fixed:std::collections::BTreeMap<String,Vec<f64>>}
#[derive(Deserialize)]struct Rings{quads:usize,width:usize,sha256:String}
#[derive(Deserialize)]struct Fixture{authored:serde_json::Value,near:Geometry,far:Geometry,materials:Vec<Material>,rings:Rings}
#[test]
fn frozen_water_geometry_material_and_topology(){
 let f:Fixture=serde_json::from_str(include_str!("fixtures/water-descriptors.json")).unwrap();let source=w::WaterSource::new(&f.authored.to_string(),false).unwrap();
 for (actual,expected)in [(&source.near,&f.near),(&source.far,&f.far)]{
  assert_eq!(actual.vertices.as_chunks::<5>().0.iter().flat_map(|v|v[..3].iter().map(|v|v.to_bits())).collect::<Vec<_>>(),expected.position);
  assert_eq!(actual.vertices.as_chunks::<5>().0.iter().flat_map(|v|v[3..].iter().map(|v|v.to_bits())).collect::<Vec<_>>(),expected.uv);
  assert_eq!(actual.index,expected.index);assert_eq!(if actual.index.iter().any(|i|*i>=65535){4}else{2},expected.width);
 }
 for m in f.materials{
  assert_eq!(w::key(m.kind,m.far,m.shadows&&!m.far).unwrap(),m.key);assert_eq!(w::blend(m.kind),m.blend);assert_eq!(m.side,2);
  assert_eq!(m.depth,if m.kind==k::WATER_REFLECTION{5}else{7});
  let fields=w::fields(m.kind).unwrap();let defaults=w::defaults(m.kind,&source).unwrap();
  for(name,expected)in m.fixed{let(at,n)=w::field(fields,&name).unwrap();assert_eq!(defaults[at..at+n],expected,"{name}");}
 }
 let index=ring_topology(f.rings.quads);assert_eq!(index.len(),24000);assert_eq!(f.rings.width,4);
 let bytes=index.iter().flat_map(|i|i.to_le_bytes()).collect::<Vec<_>>();assert_eq!(format!("{:x}",Sha256::digest(bytes)),f.rings.sha256);
 assert_eq!(ring_topology(usize::MAX),index);
}
