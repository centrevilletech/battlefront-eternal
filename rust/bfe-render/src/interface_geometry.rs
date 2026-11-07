//! SpawnStage, HoloMap and GridRoom's cold procedural geometry recipes.
//! These preserve Three's vertex/index order and Float32 rounding boundaries;
//! actor_resources retains the resulting streams with the other native sources.
use crate::actor_resources::{Geometry,Record,Stream,Attribute};
use crate::pixel_math::Math;
const TAU:f64=std::f64::consts::PI*2.;
fn finish(g:&mut Geometry,p:Vec<f32>,n:Vec<f32>,uv:Vec<f32>){
    g.facts.count=p.len()/3;g.facts.index_width=2;
    g.streams=vec![Stream::Float(p),Stream::Float(n),Stream::Float(uv)];
    g.attributes=vec![Attribute{name:"position",stream:0,size:3,stride:0,offset:0},Attribute{name:"normal",stream:1,size:3,stride:0,offset:0},Attribute{name:"uv",stream:2,size:2,stride:0,offset:0}];
}
// BoxGeometry.buildPlane gives each face independent normals/UVs. Keep those
// seams and material groups even when a room draws all six with one material.
fn box_geometry(g:&mut Geometry,r:&Record){
    let(w,h,d)=(r.width.unwrap_or(1.),r.height.unwrap_or(1.),r.depth.unwrap_or(1.));
    let mut p=vec![];let mut n=vec![];let mut uv=vec![];
    for (face,(u,v,z,ud,vd,width,height,depth)) in [(2,1,0,-1.,-1.,d,h,w),(2,1,0,1.,-1.,d,h,-w),(0,2,1,1.,1.,w,d,h),(0,2,1,1.,-1.,w,d,-h),(0,1,2,1.,-1.,w,h,d),(0,1,2,-1.,-1.,w,h,-d)].into_iter().enumerate(){
        let base=p.len()as u32/3;let start=g.index.len()as u32;
        for y in 0..2{for x in 0..2{let mut point=[0.;3];point[u]=(x as f64*width-width/2.)*ud;point[v]=(y as f64*height-height/2.)*vd;point[z]=depth/2.;p.extend(point.map(|v|v as f32));let mut normal=[0.;3];normal[z]=if depth>0.{1.}else{-1.};n.extend(normal.map(|v|v as f32));uv.extend([x as f32,1.-y as f32]);}}
        g.index.extend([base,base+2,base+1,base+2,base+3,base+1]);g.facts.groups.push([start,6,face as u32]);
    }
    finish(g,p,n,uv);
}
// CircleGeometry and RingGeometry compute UVs from the unrounded point, then
// store both streams as Float32. Reading the stored position here changes UVs.
fn radial<M:Math>(g:&mut Geometry,r:&Record,ring:bool){
    let segments=r.segments.unwrap_or(32).clamp(3,512);let length=r.theta_length.unwrap_or(TAU);
    let outer=if ring{r.outer.unwrap_or(1.)}else{r.radius.unwrap_or(1.)};let inner=r.inner.unwrap_or(0.5);
    let mut p=vec![];let mut n=vec![];let mut uv=vec![];
    if !ring{p.extend([0.,0.,0.]);n.extend([0.,0.,1.]);uv.extend([0.5,0.5]);}
    for row in 0..if ring{2}else{1}{let radius=if ring{inner+row as f64*(outer-inner)}else{outer};for i in 0..=segments{let theta=i as f64/segments as f64*length;let x=radius*M::cos(theta);let y=radius*M::sin(theta);p.extend([x as f32,y as f32,0.]);n.extend([0.,0.,1.]);uv.extend([((x/outer+1.)/2.)as f32,((y/outer+1.)/2.)as f32]);}}
    for i in 0..segments as u32{if ring{let a=i;let b=a+segments as u32+1;g.index.extend([a,b,a+1,b,b+1,a+1]);}else{g.index.extend([i+1,i+2,0]);}}
    finish(g,p,n,uv);
}
// CylinderGeometry.generateTorso/generateCap serve the stage column, projector
// beam, metal base and cone. A zero cone tip omits its degenerate first face;
// caps duplicate one centre for each triangle, as the frozen geometry does.
fn cylinder<M:Math>(g:&mut Geometry,r:&Record){
    let top=r.top.unwrap_or(1.);let bottom=r.bottom.unwrap_or(1.);let height=r.height.unwrap_or(1.);let segments=r.segments.unwrap_or(32).clamp(3,512);let slope=(bottom-top)/height;
    let mut p=vec![];let mut n=vec![];let mut uv=vec![];
    for row in 0..=1{let v=row as f64;let radius=v*(bottom-top)+top;for i in 0..=segments{let u=i as f64/segments as f64;let sin=M::sin(u*TAU);let cos=M::cos(u*TAU);p.extend([(radius*sin)as f32,(-v*height+height/2.)as f32,(radius*cos)as f32]);let inv=1./(sin*sin+slope*slope+cos*cos).sqrt();n.extend([(sin*inv)as f32,(slope*inv)as f32,(cos*inv)as f32]);uv.extend([u as f32,(1.-v)as f32]);}}
    for x in 0..segments as u32{let a=x;let b=a+segments as u32+1;if top>0.{g.index.extend([a,b,a+1]);}if bottom>0.{g.index.extend([b,b+1,a+1]);}}
    g.facts.groups.push([0,g.index.len()as u32,0]);
    if !r.open{for is_top in [true,false]{let radius=if is_top{top}else{bottom};if radius<=0.{continue;}let sign=if is_top{1.}else{-1.};let start=g.index.len()as u32;let centers=p.len()as u32/3;
        for _ in 0..segments{p.extend([0.,(height/2.*sign)as f32,0.]);n.extend([0.,sign as f32,0.]);uv.extend([0.5,0.5]);}
        let rim=p.len()as u32/3;for i in 0..=segments{let t=i as f64/segments as f64*TAU;let sin=M::sin(t);let cos=M::cos(t);p.extend([(radius*sin)as f32,(height/2.*sign)as f32,(radius*cos)as f32]);n.extend([0.,sign as f32,0.]);uv.extend([(cos*0.5+0.5)as f32,(sin*0.5*sign+0.5)as f32]);}
        for x in 0..segments as u32{if is_top{g.index.extend([rim+x,rim+x+1,centers+x]);}else{g.index.extend([rim+x+1,rim+x,centers+x]);}}g.facts.groups.push([start,g.index.len()as u32-start,if is_top{1}else{2}]);
    }}
    finish(g,p,n,uv);
}
// Batch's fixed quad topology belongs to the native descriptor. Its dynamic
// corners/UVs/tints still come from the existing native interface layout phase.
// Points likewise reserve their maximum row count at admission, never draw time.
pub fn populate<M:Math>(g:&mut Geometry,r:&Record)->Result<(),&'static str>{
    match r.kind.as_str(){
        "box"=>box_geometry(g,r),"circle"=>radial::<M>(g,r,false),"ring"=>radial::<M>(g,r,true),"uiCylinder"=>cylinder::<M>(g,r),
        "uiPoints"=>{let count=r.count.unwrap_or(40).clamp(1,4096);g.facts.count=count;g.facts.index_width=0;g.streams=vec![Stream::Float(vec![0.;count*3])];g.attributes=vec![Attribute{name:"position",stream:0,size:3,stride:0,offset:0}];},
        "uiBatch"=>{let count=r.count.unwrap_or(2048).clamp(1,2048);g.facts.count=count*4;g.facts.index_width=4;g.streams=vec![Stream::Float(vec![0.;count*12]),Stream::Float(vec![0.;count*8]),Stream::Float(vec![0.;count*8]),Stream::Float(vec![0.;count*16])];g.attributes=[("position",3),("uv",2),("maskUv",2),("tint",4)].into_iter().enumerate().map(|(stream,(name,size))|Attribute{name,stream,size,stride:0,offset:0}).collect();for i in 0..count as u32{let a=i*4;g.index.extend([a,a+1,a+2,a+2,a+1,a+3]);}},
        _=>return Err("unknown interface geometry"),
    }Ok(())
}
// BufferGeometry.applyMatrix4 rounded positions after every authored operation,
// then transformed and renormalized normals. Preserve that cold f64 order.
pub fn transform<M:Math>(g:&mut Geometry,ops:&[(String,f64)])->Result<(),&'static str>{
    for (op,value)in ops{if !value.is_finite(){return Err("invalid interface transform");}let mut m=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];let(c,s)=(M::cos(*value),M::sin(*value));match op.as_str(){"translateY"=>m[13]=*value,"rotateX"=>{m[5]=c;m[6]=s;m[9]=-s;m[10]=c;},"rotateY"=>{m[0]=c;m[2]=-s;m[8]=s;m[10]=c;},_=>return Err("unknown interface transform")};
        for a in &g.attributes{if !matches!(a.name,"position"|"normal"){continue;}let Stream::Float(data)=&mut g.streams[a.stream]else{continue;};let normal=crate::math::normal_matrix(&m);for p in data.chunks_exact_mut(3){let(x,y,z)=(p[0]as f64,p[1]as f64,p[2]as f64);let v=if a.name=="position"{{let w=1./(m[3]*x+m[7]*y+m[11]*z+m[15]);[(m[0]*x+m[4]*y+m[8]*z+m[12])*w,(m[1]*x+m[5]*y+m[9]*z+m[13])*w,(m[2]*x+m[6]*y+m[10]*z+m[14])*w]}}else{let v=[normal[0]*x+normal[3]*y+normal[6]*z,normal[1]*x+normal[4]*y+normal[7]*z,normal[2]*x+normal[5]*y+normal[8]*z];let len=(v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();let inv=1./if len==0.{1.}else{len};v.map(|v|v*inv)};for i in 0..3{p[i]=v[i]as f32;}}
        }
    }Ok(())
}
