// Frozen FountainJets geometry/seed/atlas output from cce4ec8c. Synthetic
// source policy only; CI needs neither game files nor a browser renderer.
use bfe_render::{effect_layout as kind,effect_resources::Descriptor,fountain_resources::{geometry,atlas},pixel_math::V8};
use serde::Deserialize;
use sha2::{Digest,Sha256};
#[derive(Deserialize)]struct Fixture{rows:Vec<Row>,atlas:Atlas}
#[derive(Deserialize)]struct Atlas{bytes:usize,sha256:String}
#[derive(Deserialize)]struct Row{name:String,tier:bool,key:String,per:usize,index:Vec<u16>,attributes:Vec<Attribute>}
#[derive(Deserialize)]struct Attribute{name:String,size:u32,instance:bool,bits:Vec<u32>}
#[test]fn frozen_fountain_geometry_seeds_and_image_bits(){
    let f:Fixture=serde_json::from_str(include_str!("fixtures/fountain-descriptors.json")).unwrap();
    for row in f.rows{
        let before=row.name.ends_with("-near")||row.name.ends_with("-before");
        let kind=if row.name.contains("ropes"){kind::JET_ROPES}else if row.name.contains("overflow"){kind::JET_FILM}else if row.name.ends_with("-core"){kind::JET_CORE}else{kind::JET_PARTICLES};
        let group=if row.name=="fountain-pool"{0}else if row.name.contains("spray"){1}else{2};
        let variant=u32::from(before)|if row.tier{2}else{0}|if kind==kind::JET_PARTICLES{group<<2}else{0};
        let d=Descriptor::new(kind,variant,true).unwrap();assert_eq!(d.key,row.key);
        let g=geometry(kind,variant).unwrap();assert_eq!(g.per,row.per);
        let spec=bfe_render::programs::spec(&d.key).unwrap();
        let base=g.base.map(|b|b.for_program(spec.attributes));
        if let Some(base)=&base{assert_eq!(base.index,row.index);}else{assert_eq!(Descriptor::corner().1.as_slice(),row.index);}
        for a in row.attributes{
            if a.instance{assert_eq!(g.rows.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),a.bits,"{} {}",row.name,a.name);}
            else if a.name=="corner"{assert_eq!(Descriptor::corner().0.map(f32::to_bits).as_slice(),a.bits);}
            else{let b=base.as_ref().unwrap();let mut at=0;let mut found=false;
                for &(name,size) in &b.attributes{let end=at+b.vertices as usize*size as usize;if name==a.name{found=true;assert_eq!(size,a.size);assert_eq!(b.data[at..end].iter().map(|v|v.to_bits()).collect::<Vec<_>>(),a.bits,"{} {}",row.name,name);}at=end;}
                assert!(found,"{} {}",row.name,a.name);
            }
        }
    }
    // The shader samples all four channels and generated mip levels. Hash the
    // full frozen source bytes before any codec, upload or GPU filtering.
    let pixels=atlas::<V8>();assert_eq!(pixels.len(),f.atlas.bytes);assert_eq!(format!("{:x}",Sha256::digest(&pixels)),f.atlas.sha256);
}

// FountainWater tier topology and bounds use the same page arithmetic as the
// original procedural discs, including Float32 conversion after placement.
#[test]
fn frozen_basin_tier_streams() {
 #[derive(Deserialize)]struct Tiers{basins:Vec<[f64;4]>,vertices:Vec<u32>,index:Vec<u32>}
 let f:Tiers=serde_json::from_str(include_str!("fixtures/basin-tiers.json")).unwrap();
 let (vertices,index)=bfe_render::fountain_resources::basin_tiers::<V8>(&f.basins);
 assert_eq!(vertices.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),f.vertices);assert_eq!(index,f.index);
}
