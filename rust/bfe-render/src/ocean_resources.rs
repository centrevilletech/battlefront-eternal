//! OceanSurface.oceanGeometry, EnhancedOcean.stormGeometry and horizon haze.
//! These procedural streams have no content decoder or browser resource owner.
use crate::{water_resources::Geometry,pixel_math::Math};
fn shape(vertices:Vec<f32>,index:Vec<u32>)->Geometry{
 let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];for p in vertices.as_chunks::<3>().0{for j in 0..3{lo[j]=lo[j].min(p[j] as f64);hi[j]=hi[j].max(p[j] as f64);}}
 let center=std::array::from_fn::<_,3,_>(|j|(lo[j]+hi[j])*0.5);let mut radius:f64=0.;for p in vertices.as_chunks::<3>().0{let d=std::array::from_fn::<_,3,_>(|j|p[j] as f64-center[j]);radius=radius.max(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]);}
 Geometry{vertices,index,stride:3,bounds:[center[0],center[1],center[2],radius.sqrt(),lo[0],lo[1],lo[2],hi[0],hi[1],hi[2]]}
}
pub fn geometry<M:Math>()->[Geometry;3]{
 // PlaneGeometry.rotateX rounds after the rotation, retaining its tiny Y
 // values. Replacing them with zero would change frozen resource bytes.
 let mut p=Vec::with_capacity(4241*3);let mut index=Vec::with_capacity(24600);let angle=-std::f64::consts::PI/2.;let(c,s)=(M::cos(angle),M::sin(angle));
 for y in 0..=64{for x in 0..=64{let px=-256.+x as f64*8.;let py=-(-256.+y as f64*8.);p.extend([(px+0.*py+0.+0.) as f32,(0.*px+c*py+(-s)*0.+0.) as f32,(0.*px+s*py+c*0.+0.) as f32]);}}
 for y in 0..64{for x in 0..64{let a=y*65+x;let b=a+65;index.extend([a,b,a+1,b,b+1,a+1]);}}
 let inner=[[-256.,-256.],[256.,-256.],[256.,256.],[-256.,256.]];let outer=[[-4096.,-4096.],[4096.,-4096.],[4096.,4096.],[-4096.,4096.]];
 for i in 0..4{let j=(i+1)%4;let base=p.len() as u32/3;for v in [inner[i],inner[j],outer[j],outer[i]]{p.extend([v[0] as f32,0.,v[1] as f32]);}index.extend([base,base+2,base+1,base,base+3,base+2]);}
 let classic=shape(p,index);

 // stormGeometry's concentric squares preserve triangle order across rings.
 // Only the exponent uses the page builtin; placement is ordinary f64 math.
 let mut p=vec![0.;3];let mut index=vec![];
 for ring in 1..=128{let radius=if ring<=64{ring as f64*3.}else{192.*M::pow(4096./192.,(ring-64) as f64/64.)};
  for side in 0..4{for i in 0..64{let t=-1.+2.*i as f64/64.;let x=[t,1.,-t,-1.][side];let z=[-1.,t,1.,-t][side];p.extend([(x*radius) as f32,0.,(z*radius) as f32]);let next=(side*64+i+1)%256;let a=1+(ring-1)*256+side*64+i;let b=1+(ring-1)*256+next;
   if ring==1{index.extend([0,b as u32,a as u32]);}else{index.extend([(a-256) as u32,b as u32,a as u32,(a-256) as u32,(b-256) as u32,b as u32]);}
  }}
 }
 let storm=shape(p,index);

 // SphereGeometry's two pole rows omit their degenerate triangles. Keep its
 // negative-X convention and trigonometric operation order before f32 storage.
 let mut p=Vec::with_capacity(33*13*3);let mut index=vec![];
 for y in 0..=12{let theta=y as f64/12.*std::f64::consts::PI;for x in 0..=32{let phi=x as f64/32.*(std::f64::consts::PI*2.);p.extend([(-850.*M::cos(phi)*M::sin(theta)) as f32,(850.*M::cos(theta)) as f32,(850.*M::sin(phi)*M::sin(theta)) as f32]);}}
 for y in 0..12{for x in 0..32{let a=y*33+x+1;let b=a-1;let c=b+33;let d=a+33;if y!=0{index.extend([a,b,d]);}
  if y!=11{index.extend([b,c,d]);}}}
 [
  classic,
  storm,
  shape(p,index),
 ]
}
// EnhancedOcean's fixed crest phases are native instance descriptors. They
// upload once, not through BufferAttribute version polling on every frame.
pub fn spray_seeds()->Vec<f32>{let mut data=Vec::with_capacity(384*4);for i in 0..384{data.extend([(((i%24) as f64+0.5)/24.) as f32,(((i/24) as f64+0.5)/16.) as f32,(i as f64*0.6180339%1.) as f32,(i as f64*0.7548776%1.) as f32]);}data}
