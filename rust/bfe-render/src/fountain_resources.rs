//! FountainJets' cold geometry, seed and aerated-water image descriptors.
//! Only visible slots, levels, time and viewport facts remain frame inputs.
use crate::{effect_layout as kind,effect_resources::Base,pixel_math::Math};
pub const FOUNTAINS:usize=5;
pub struct Geometry {pub base:Option<Base>,pub rows:Vec<f32>,pub per:usize}
// FountainJets seeds use unsigned LCG wrapping, independent of cosmetic
// page randomness or resource UUID creation. Recovery gets identical rows.
fn random(state:&mut u32)->f64{*state=state.wrapping_mul(1664525).wrapping_add(1013904223);*state as f64/4294967296.}
// FountainJets.particles preserves the shared splash phases and stable
// before/after kind order. Float32 conversion occurs after ordering/repetition.
pub fn geometry(kind:usize,variant:u32)->Option<Geometry>{
    let before=variant&1!=0;let tier=variant&2!=0;
    match kind {
        kind::JET_PARTICLES=>{
            let counts=[24,300,340,640,352,700,320,36,72,80];let mut state=0x51ed27;let mut seeds=vec![];
            for (kind,&count) in counts.iter().enumerate(){
                // A splash event shares its phase across all eight fragments.
                if kind==4{for event in 0..count/8 {let phase=random(&mut state);for part in 0..8{seeds.push([kind as f64,phase,event as f64+part as f64/8.]);}}}
                else{for j in 0..count{seeds.push([kind as f64,(j as f64+random(&mut state))/count as f64,random(&mut state)]);}}
            }
            let group=(variant>>2)&3;seeds.retain(|s|{let k=s[0] as u32;match group{0=>k>=8,1=>k<=3,_=>(4..8).contains(&k)}});
            // Before-water draws reverse kinds while preserving equal-kind
            // source order, so alpha accumulation matches the frozen batch.
            if before{seeds.sort_by(|a,b|b[0].partial_cmp(&a[0]).unwrap());}
            let per=seeds.len();let mut rows=Vec::with_capacity(per*FOUNTAINS*3);
            for _ in 0..FOUNTAINS{for seed in &seeds{rows.extend(seed.map(|v|v as f32));}}
            Some(Geometry{base:None,rows,per})
        },
        kind::JET_ROPES=>{
            // The tier adds 28 ropes to each fountain, not another draw.
            let streams=330+if tier{28}else{0};let mut data=Vec::with_capacity(23*4);let mut index=Vec::with_capacity(22*6);
            for k in 0..=22{let t=(k as f64/22.) as f32;data.extend_from_slice(&[t,-1.,t,1.]);}
            for k in 0..22{let a=k*2;index.extend_from_slice(&[a,a+1,a+2,a+1,a+3,a+2]);}
            let mut rows=Vec::with_capacity(streams*FOUNTAINS);
            // The after-water pass rotates rope ids through the original bank.
            for _ in 0..FOUNTAINS{for k in 0..streams{rows.push(if before{k}else{(k+330)%streams} as f32);}}
            Some(Geometry{base:Some(Base{vertices:46,attributes:vec![("strip",2)],data,index}),rows,per:streams})
        },
        // columnGeometry's jet and dome are disjoint grids in source order.
        // filmGeometry keeps each of the twelve tier faces a separate strip.
        kind::JET_CORE|kind::JET_FILM=>{
            let mut data=vec![];let mut index=vec![];let film=kind==kind::JET_FILM;
            let count=if film{12}else{2};let around=if film{8}else{24};
            for part in 0..count{let rings=if film{10}else if part==0{40}else{8};let base=data.len()/3;
                for k in 0..=rings{for j in 0..=around{data.extend_from_slice(&[part as f32,if film{j as f64/around as f64}else{k as f64/rings as f64} as f32,if film{k as f64/rings as f64}else{j as f64/around as f64} as f32]);}}
                for k in 0..rings{for j in 0..around{let a=(base+k*(around+1)+j) as u16;let b=a+around as u16+1;index.extend_from_slice(&[a,b,a+1,a+1,b,b+1]);}}
            }
            let vertices=(data.len()/3) as u32;
            Some(Geometry{base:Some(Base{vertices,attributes:vec![(if film{"film"}else{"sweep"},3)],data,index}),rows:vec![],per:1})
        },_=>None,
    }
}
// FountainJets.foamPixels stores height/density in Float32 before differences.
// Keep those boundaries as well as the page's stable hypot and byte rounding.
pub fn atlas<M:Math>()->Vec<u8>{
    let mut state=0x2545f491;let mut lattice=[0f32;256];for v in &mut lattice{*v=random(&mut state) as f32;}
    let smooth=|t:f64|{let t=t.clamp(0.,1.);t*t*(3.-2.*t)};
    let noise=|x:f64,y:f64|{
        let ix=x.floor() as i32;let iy=y.floor() as i32;let mut fx=x-ix as f64;let mut fy=y-iy as f64;
        fx=fx*fx*(3.-2.*fx);fy=fy*fy*(3.-2.*fy);
        let at=|x:i32,y:i32|lattice[((y&15)<<4|(x&15)) as usize] as f64;
        let (a,b,c,d)=(at(ix,iy),at(ix+1,iy),at(ix,iy+1),at(ix+1,iy+1));
        (a+(b-a)*fx)*(1.-fy)+(c+(d-c)*fx)*fy
    };
    let fbm=|x:f64,y:f64|noise(x,y)*0.52+noise(x*2.03+5.2,y*2.03+1.3)*0.3+noise(x*4.1+9.7,y*4.1+3.1)*0.18;
    // Foam height and density round separately before finite differences.
    // Combining these loops would change normals at the atlas cell edges.
    let mut height=vec![0f32;128*128];let mut density=height.clone();let mut data=vec![0u8;256*128*4];
    for cell in 0..2{let c=cell as f64;
        for y in 0..128{for x in 0..128{let u=(x as f64+0.5)/128.*2.-1.;let v=(y as f64+0.5)/128.*2.-1.;let i=y*128+x;
            let wu=u+0.25*(noise(u*2.5+c*5.,v*2.5)-0.5);let wv=v+0.25*(noise(u*2.5+9.,v*2.5+c*5.)-0.5);
            let d=M::hypot(wu/0.7,wv);let body=smooth((0.86-d+(fbm(u*2.2+c*7.3,v*1.4+c*3.1)-0.5)*0.9)/0.55);
            if body<=0.{height[i]=0.;density[i]=0.;continue;}
            let fiber=fbm(u*6.5+c*3.1,v*1.7+c*7.3);let fine=fbm(u*16.+c*1.7,v*5.+c*4.3);
            let torn=1.-(1.-smooth((fbm(u*4.5+c*2.3,v*3.1+c*9.1)-0.32)/0.25))*smooth((d-0.35)/0.4);
            height[i]=(body*torn*(0.12*(1.-d*d).max(0.).sqrt()+0.05*fiber+0.015*fine)) as f32;
            density[i]=(body*torn*(0.25+0.85*fiber)+(fine-0.5)*0.25*body) as f32;
        }}
        for y in 0..128{for x in 0..128{let row=y*128;
            // foamPixels clamps edge neighbours rather than wrapping them;
            // periodic noise does not make the final atlas periodic.
            let nx=-(height[row+(x+1).min(127)] as f64-height[row+x.saturating_sub(1)] as f64)*32.;
            let ny=-(height[(y+1).min(127)*128+x] as f64-height[y.saturating_sub(1)*128+x] as f64)*32.;
            let length=M::hypot3(nx,ny,1.);let o=(y*256+cell*128+x)*4;
            data[o]=((nx/length*0.5+0.5)*255.+0.5).floor() as u8;data[o+1]=((ny/length*0.5+0.5)*255.+0.5).floor() as u8;
            data[o+2]=((height[row+x] as f64/0.18).min(1.)*255.+0.5).floor() as u8;data[o+3]=((density[row+x] as f64).clamp(0.,1.)*255.+0.5).floor() as u8;
        }}
    }data
}

// FountainWater's tier discs formerly constructed a BufferGeometry per map.
// Authored basin centres remain cold input; topology and all Float32 rounding
// are native, using V8's trigonometry only where it reaches the pixels.
pub fn basin_tiers<M:crate::pixel_math::Math>(basins:&[[f64;4]])->(Vec<f32>,Vec<u32>) {
 // FountainWater keeps five authored basin slots. The native tier adds a
 // twelve-sided lip at the same raised water level used by its shader.
 let mut positions=Vec::with_capacity(basins.len()*42);let mut index=Vec::with_capacity(basins.len()*36);
 for b in basins.iter().take(5){let base=positions.len() as u32/3;let [x,y,z,_]=*b;
  positions.extend([x as f32,(y+0.797) as f32,z as f32]);
  for k in 0..=12 {let a=k as f64/12.*std::f64::consts::PI*2.;positions.extend([(x+M::cos(a)*4.) as f32,(y+0.797) as f32,(z+M::sin(a)*4.) as f32]);}
  for k in 1..=12 {index.extend([base,base+k+1,base+k]);}
 }
 (positions,index)
}
