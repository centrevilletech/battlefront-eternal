// Frozen OceanSurface, EnhancedOcean and horizon geometry constructors. Full
// vertex/index/seed hashes guard every Float32 byte in content-free CI.
use bfe_render::{water_resources as w,ocean_resources as ocean,effect_layout as k};
use serde::Deserialize;
use sha2::{Digest,Sha256};
#[derive(Deserialize)]struct Geometry{vertices:usize,indices:usize,width:usize,position:String,index:String}
#[derive(Deserialize)]struct Fixture{authored:serde_json::Value,geometry:Vec<Geometry>,seeds:String,fixed:std::collections::BTreeMap<String,std::collections::BTreeMap<String,Vec<f64>>>}
fn digest(v:impl AsRef<[u8]>)->String{format!("{:x}",Sha256::digest(v))}
#[test]
fn frozen_ocean_geometry_and_material_values(){
 let f:Fixture=serde_json::from_str(include_str!("fixtures/ocean-descriptors.json")).unwrap();let source=w::WaterSource::new(&f.authored.to_string(),true).unwrap();
 for(actual,expected)in source.ocean.as_ref().unwrap().iter().zip(&f.geometry){
  assert_eq!(actual.vertices.len()/3,expected.vertices);assert_eq!(actual.index.len(),expected.indices);assert_eq!(expected.width,2);
  assert_eq!(digest(actual.vertices.iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>()),expected.position);
  assert_eq!(digest(actual.index.iter().flat_map(|v|(*v as u16).to_le_bytes()).collect::<Vec<_>>()),expected.index);
 }
 assert_eq!(digest(ocean::spray_seeds().iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>()),f.seeds);
 for(name,kind)in [("classic",k::OCEAN_CLASSIC),("ocean",k::OCEAN),("spray",k::OCEAN_SPRAY)]{
  let fields=w::fields(kind).unwrap();let values=w::defaults(kind,&source).unwrap();
  for(field,expected)in &f.fixed[name]{if let Some((at,n))=w::field(fields,field){assert_eq!(&values[at..at+n],expected,"{name} {field}");}}
 }
}
