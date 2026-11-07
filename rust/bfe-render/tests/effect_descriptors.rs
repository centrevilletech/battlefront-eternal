// Frozen Batch, SquareBatch and BlasterBatch output from cce4ec8c. The
// reference constructors are absent from the product; CI uses synthetic data.
use bfe_render::effect_resources::Descriptor;
use serde::Deserialize;
#[derive(Deserialize)]
struct Fixture {
    kind:usize,variant:u32,shadows:bool,key:String,attributes:Vec<(String,u32)>,values:Vec<(String,u32)>,
    blend:[u32;7],side:u32,#[serde(rename="depthFlags")] depth_flags:u32,corner:Vec<u32>,index:Vec<u16>,#[serde(default)] base:Vec<Base>,
}
#[derive(Deserialize)]
struct Base {name:String,size:u32,bits:Vec<u32>}
#[test]
fn cold_particle_geometry_material_and_raster_bits() {
    let rows:Vec<Fixture>=serde_json::from_str(include_str!("fixtures/effect-descriptors.json")).unwrap();
    for f in rows {
        let d=Descriptor::new(f.kind,f.variant,f.shadows).unwrap();
        assert_eq!(d.key,f.key);assert_eq!(d.attributes,f.attributes);assert_eq!(d.values,f.values);
        assert_eq!(d.blend,f.blend);assert_eq!(d.side,f.side);assert_eq!(d.depth_flags,f.depth_flags);
        if let Some(base)=d.geometry(f.kind).map(|g|g.for_program(bfe_render::programs::spec(&d.key).unwrap().attributes)) {
            let mut at=0;
            assert_eq!(base.attributes.len(),f.base.len(),"{} base attributes",d.key);
            for ((name,size),held) in base.attributes.iter().zip(&f.base){
                assert_eq!(*name,held.name);assert_eq!(*size,held.size);
                let end=at+base.vertices as usize*(*size as usize);
                assert_eq!(base.data[at..end].iter().map(|v|v.to_bits()).collect::<Vec<_>>(),held.bits,"{} base {name}",d.key);at=end;
            }
            assert_eq!(base.index,f.index,"{} index",d.key);
        }else{
            let (vertices,index)=Descriptor::corner();
            assert_eq!(vertices.map(f32::to_bits).as_slice(),f.corner);assert_eq!(index.as_slice(),f.index);
        }
        // Every vertex input resolves against the source-owned catalog. Live
        // facts may include uniforms optimized out by this closed variant.
        let spec=bfe_render::programs::spec(&d.key).unwrap();
        for (name,_) in &d.attributes {assert!(spec.attributes.iter().any(|(n,_)|*n==name));}
    }
}
#[test]
fn particle_counts_clip_without_overflow() {
    for count in [0,1,16384,65536,65537,usize::MAX] {
        assert_eq!(Descriptor::count(count),count.min(65536));
    }
    assert!(Descriptor::new(usize::MAX,0,false).is_err());
}

// JetpackVolume.noiseTexture and EnvironmentVolume.noiseTexture were frozen
// before deletion. Full texel hashes also guard restoration source bytes.
#[test]
fn frozen_noise_and_dynamic_material_policy() {
 use bfe_render::{effect_resources::{flame_noise,steam_noise,MeshDescriptor,target},effect_layout as k};
 use sha2::{Digest,Sha256};
 let hashes:serde_json::Value=serde_json::from_str(include_str!("fixtures/effect-noise.json")).unwrap();
 assert_eq!(format!("{:x}",Sha256::digest(flame_noise())),hashes["flame"].as_str().unwrap());
 assert_eq!(format!("{:x}",Sha256::digest(steam_noise())),hashes["steam"].as_str().unwrap());
 for shadows in [false,true] { for kind in [k::LIGHTNING,k::DESTRUCTION_ARCS,k::DEATH_TRACE] {
  let d=MeshDescriptor::new(kind,shadows,&[]).unwrap();
  assert_eq!(d.passes.len(),if kind==k::LIGHTNING{2}else{1});
  for (key,_) in d.passes {let spec=bfe_render::programs::spec(&key).unwrap();for(name,_)in &d.attributes{assert!(spec.attributes.iter().any(|(n,_)|n==name));}}
 }}
 // Target recipes distinguish exact sampling and attachment policy. Resizing
 // may change storage dimensions, never multisampling or logical identities.
 for kind in 0..8 {let t=target(kind).unwrap();assert_eq!(t.depth_texture,matches!(kind,k::TARGET_FOUNTAIN|k::TARGET_WATER_COPY));assert_eq!(t.nearest,kind==k::TARGET_VOLUME);assert_eq!(t.mipmaps,kind==k::TARGET_JET_COPY);}
 assert!(target(usize::MAX).is_none());
}

// StormSky.bolt's native far recipe preserves its former constructor policy.
// The live positions remain borrowed environment output, not frozen content.
#[test]
fn frozen_storm_lightning_geometry_and_material(){
 let f:serde_json::Value=serde_json::from_str(include_str!("fixtures/storm-descriptors.json")).unwrap();
 let (key,attributes,blend,flags)=bfe_render::effect_resources::far_mesh(bfe_render::effect_layout::STORM_LIGHTNING,false).unwrap();
 assert_eq!(key,f["key"]);assert_eq!(serde_json::to_value(attributes).unwrap(),f["attributes"]);
 assert_eq!(bfe_render::effect_layout::STORM_LIGHTNING_CAPACITY,f["capacity"].as_u64().unwrap() as usize);
 assert_eq!(flags&3,f["side"].as_u64().unwrap() as u32);assert_eq!(flags&4!=0,f["depth"].as_bool().unwrap());assert_eq!(flags&8!=0,f["write"].as_bool().unwrap());
 assert_eq!(blend,[1,32774,32774,770,1,770,1]);
}
