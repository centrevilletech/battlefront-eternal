//! Ports Lightning and ArcLightning. The owner retains clocks, random shapes,
//! shock state and draw storage; the host supplies scene and collision facts.
use super::lightning_layout::{lightning_frame as f, lightning_unit as u,
    lightning_node as no, lightning_spawn as sp, lightning_bolt as bo,
    lightning_shock as so, lightning_joint as jo, lightning_light as li,
    lightning_capsule as ca, lightning_counts as h, lightning_readers as rd, lightning_limits as lim};
use serde::Deserialize;
use three::{Vector3 as V, Quaternion as Q, Matrix4, Euler};
use v8math::{sin, cos, pow, log2, hypot, hypot3};
const UP: V=V::new(0.,1.,0.); const SIDE: V=V::new(1.,0.,0.);
const NONE:usize=usize::MAX; const POINTS:usize=257;
// These value helpers repeat Vector3 operations in their original order.
// Doubles stay native until the final Float32Array-compatible stores.
fn sub(a:V,b:V)->V{let mut r=a;r.sub(&b);r}
fn add(a:V,b:V)->V{let mut r=a;r.add(&b);r}
fn mul(a:V,s:f64)->V{let mut r=a;r.multiply_scalar(s);r}
fn scaled(a:V,b:V,s:f64)->V{let mut r=a;r.add_scaled_vector(&b,s);r}
fn norm(mut a:V)->V{a.normalize();a}
fn mix(a:V,b:V,t:f64)->V{let mut r=V::default();r.lerp_vectors(&a,&b,t);r}
fn cross(a:V,b:V)->V{let mut r=V::default();r.cross_vectors(&a,&b);r}
fn vec(a:&[f64],o:usize)->V{V::new(a[o],a[o+1],a[o+2])}
fn put(a:&mut [f64],o:usize,v:V){a[o]=v.x;a[o+1]=v.y;a[o+2]=v.z;}
fn basis(a:V,b:V)->(V,V){let d=norm(sub(b,a));let u=norm(cross(d,if d.y.abs()>0.95{SIDE}else{UP}));(u,norm(cross(d,u)))}
// A visual birth consumes the same unsigned seed range as newSeed().
// The host entropy source is consulted only when the native phase asks.
fn seed(random:&mut dyn FnMut()->f64)->u32{(random()*4294967295.).floor() as u32}
// ArcLightning.hash and wave keep the original integer wrapping and V8 math.
fn hash(seed:u32,n:i32)->f64{let mut t=seed^(n as u32).wrapping_mul(0x9e3779b1);t=(t^(t>>16)).wrapping_mul(0x85ebca6b);t=(t^(t>>13)).wrapping_mul(0xc2b2ae35);(t^(t>>16)) as f64/4294967296.}
fn signed(s:u32,n:i32)->f64{hash(s,n)*2.-1.}
fn wave(s:u32,x:f64)->f64{let i=x.floor();let f=x-i;let u=f*f*(3.-2.*f);signed(s,i as i32)*(1.-u)+signed(s,(i as i32).wrapping_add(1))*u}
#[derive(Clone,Copy)]
struct Path{points:[V;POINTS],count:usize}
impl Default for Path{fn default()->Self{Self{points:[V::default();POINTS],count:0}}}
#[derive(Clone,Copy,Default)]
struct Jag{rough:f64,spread:f64,seed:u32,jag:u32,time:f64,writhe:f64,coarse:usize}
fn jag(rough:f64,spread:f64,seed:u32,jag:u32)->Jag{Jag{rough,spread,seed,jag,..Default::default()}}
// Ports displace. Scratch is fixed even for a malformed depth; midpoint order
// and the two separate lateral additions are intentional rounding boundaries.
fn displace(path:&mut Path,a:V,b:V,depth:usize,u:V,w:V,o:Jag){
 let depth=depth.min(8);let n=1<<depth;path.points[0]=a;path.points[n]=b;
 let length=a.distance_to(&b);let mut step=n;
 for level in 0..depth{let half=step>>1;let reach=(length/(1<<level) as f64*o.rough).min(o.spread*pow(0.64,level as f64));
  let sx=o.seed.wrapping_add((level*13+1) as u32);let sy=sx.wrapping_add(1);let phase=o.time*o.writhe;
  for i in (half..n).step_by(step){let (x,y)=if level>=o.coarse{(signed(o.jag,(i*2) as i32),signed(o.jag,(i*2+1) as i32))}else{(wave(sx,phase+i as f64*0.61),wave(sy,phase+i as f64*0.61+17.3))};
   let q=mix(path.points[i-half],path.points[i+half],0.5);path.points[i]=scaled(scaled(q,u,x*reach),w,y*reach);
  }step>>=1;
 }path.count=n+1;
}
// Lightning.random is independent of the cosmetic birth stream. Reinitializing
// it for each shape preserves the same two-shape Classic interpolation.
struct ShapeRandom(u32);
impl ShapeRandom{fn next(&mut self)->f64{self.0=self.0.wrapping_add(0x6d2b79f5);let mut t=self.0;t=(t^(t>>15)).wrapping_mul(t|1);t^=t.wrapping_add((t^(t>>7)).wrapping_mul(t|61));(t^(t>>14)) as f64/4294967296.}}
// Missing Enhanced settings have defaults; authored zero is still a value.
// Options preserve that distinction instead of normalizing the definition.
#[derive(Clone,Copy,Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct Definition{pub width:Option<f64>,pub fade_time:Option<f64>,pub spread_factor:Option<f64>,pub interpolation_speed:Option<f64>,pub branch_factor:Option<f64>,pub branch_spread_factor:Option<f64>,pub branch_length:Option<f64>,pub max_branches:Option<f64>,pub break_distance:Option<f64>,pub texture_size:Option<f64>,pub child_break_distance:Option<f64>,pub child_texture_size:Option<f64>,pub child_width:Option<f64>,pub child_spread_factor:Option<f64>,pub num_children:Option<f64>,pub color:Option<[f64;4]>,pub child_color:Option<[f64;4]>}
impl Default for Definition{fn default()->Self{serde_json::from_str("{}").unwrap()}}
fn rgb(c:[f64;4],power:f64)->[f64;3]{let peak=c[0].max(c[1]).max(c[2]).max(1.);[pow(c[0]/peak,power),pow(c[1]/peak,power),pow(c[2]/peak,power)]}
#[derive(Clone,Copy,Default)]struct Child{seed:u32,next:f64,on:bool}
// Classic and Enhanced retain separate lifetimes on the same birth.
// A look change must not restart a ribbon or invent an Enhanced record.
#[derive(Clone,Copy)]struct Bolt{active:bool,classic:bool,def:usize,start:V,end:V,from:V,to:V,follow:bool,follow_unit:usize,charge:f64,classic_life:f64,phase:f64,seed_a:u32,seed_b:u32,born:f64,age:f64,delay:f64,life:f64,seed:u32,jag:u32,branch_seed:u32,next_strike:f64,strike:f64,flash:f64,arrived:bool,air:bool,own:bool,shooter:usize,shooter_gen:f64,source:usize,source_gen:f64,target:usize,target_gen:f64,surface:Option<V>,floor:Option<f64>,children:[Child;lim::CHILDREN],child_count:usize,rgb:[f64;3],child_rgb:[f64;3]}
impl Default for Bolt{fn default()->Self{Self{active:false,classic:false,def:0,start:V::default(),end:V::default(),from:V::default(),to:V::default(),follow:false,follow_unit:NONE,charge:0.,classic_life:0.,phase:0.,seed_a:0,seed_b:0,born:0.,age:0.,delay:0.,life:0.,seed:0,jag:0,branch_seed:0,next_strike:0.,strike:1.,flash:0.,arrived:false,air:false,own:false,shooter:NONE,shooter_gen:0.,source:NONE,source_gen:0.,target:NONE,target_gen:0.,surface:None,floor:None,children:[Child::default();lim::CHILDREN],child_count:0,rgb:[0.;3],child_rgb:[0.;3]}}}
#[derive(Clone,Copy,Default)]struct Arc{a:usize,b:usize,ground:bool,gx:f64,gz:f64,seed:u32,t1:f64,t2:f64,o1:[f64;2],o2:[f64;2],va:V,vb:V,reach:f64,along:f64}
#[derive(Clone,Copy,Default)]struct Joint{on:bool,base:Q,applied:Q,offset:Q}
// Fixed arc and joint banks cover a full body or shield shock.
// Their descriptors are reused on every strike instead of allocating paths.
#[derive(Clone,Copy)]struct Shock{on:bool,generation:f64,age:f64,life:f64,strength:f64,rgb:[f64;3],seed:u32,flicker:f64,next_flicker:f64,next_arcs:f64,next_twitch:f64,arcs:[Arc;lim::ARCS],arc_count:usize,joints:[Joint;lim::JOINTS],impact:V}
impl Default for Shock{fn default()->Self{Self{on:false,generation:0.,age:0.,life:0.,strength:0.,rgb:[0.;3],seed:0,flicker:1.,next_flicker:0.,next_arcs:0.,next_twitch:0.,arcs:[Arc::default();lim::ARCS],arc_count:0,joints:[Joint::default();lim::JOINTS],impact:V::default()}}}
#[derive(Clone,Copy,Default)]struct Spark{p:V,v:V,age:f64,life:f64,floor:f64,bounced:bool,rgb:[f64;3]}
#[derive(Default,Deserialize)]pub struct Actor{#[serde(skip)]pub node_start:Option<usize>,pub parents:Vec<i32>,pub bones:Vec<usize>,pub low:Vec<usize>,pub chest:Option<usize>,pub twitch:Vec<(usize,f64)>}
// Collision is an existing foreign owner. Its host callback returns a hit fact;
// neither a ray direction nor a visual effect decision is calculated in JS.
pub trait Collision{fn cast(&mut self,from:V,dir:V,length:f64)->Option<(f64,V)>;}
pub struct EmptyCollision;
impl Collision for EmptyCollision{fn cast(&mut self,_:V,_:V,_:f64)->Option<(f64,V)>{None}}
pub struct Lightning{
 pub used:Vec<u32>,pub unit_order:Vec<u32>,pub aim:Vec<u32>,pub readers:Vec<u32>,pub follows:Vec<u32>,pub frame:[f64;f::STRIDE],pub units:Vec<f64>,pub nodes:Vec<f64>,pub spawn:[f64;sp::STRIDE],pub held:Vec<f64>,pub output:Vec<f64>,pub shocks_output:Vec<f64>,pub joints:Vec<f64>,pub lights:Vec<f64>,pub capsules:Vec<f32>,pub view_capsules:Vec<f32>,pub positions:Vec<f32>,pub uvs:Vec<f32>,pub colors:Vec<f32>,pub indices:Vec<u32>,pub counts:[u32;h::STRIDE],
 shock_order:Vec<usize>,shield_order:Vec<usize>,defs:Vec<Definition>,bolts:Vec<Bolt>,order:Vec<usize>,shocks:Vec<Shock>,shields:Vec<Shock>,charges:Vec<Shock>,actors:Vec<Actor>,sparks:Vec<Spark>,time:f64,enhanced:bool,main:Path,worlds:[Matrix4;lim::NODES_PER_UNIT],view:bool,
}
impl Default for Lightning{fn default()->Self{Self{used:vec![0;lim::UNITS],unit_order:vec![0;lim::UNITS],aim:vec![0;lim::UNITS],readers:vec![0;lim::UNITS],follows:vec![0;lim::BOLTS],frame:[0.;f::STRIDE],units:vec![0.;lim::UNITS*u::STRIDE],nodes:Vec::new(),spawn:[0.;sp::STRIDE],held:vec![0.;lim::BOLTS*3],output:vec![0.;lim::BOLTS*bo::STRIDE],shocks_output:vec![0.;lim::UNITS*so::STRIDE],joints:vec![0.;lim::UNITS*lim::JOINTS*jo::STRIDE],lights:vec![0.;lim::LIGHTS*li::STRIDE],capsules:vec![0.;lim::CAPSULES*ca::STRIDE],view_capsules:vec![0.;lim::CAPSULES*ca::STRIDE],positions:vec![0.;lim::VERTICES*3],uvs:vec![0.;lim::VERTICES*2],colors:vec![0.;lim::VERTICES*4],indices:vec![0;lim::VERTICES*3],counts:[0;h::STRIDE],shock_order:Vec::with_capacity(lim::UNITS),shield_order:Vec::with_capacity(lim::UNITS),defs:Vec::new(),bolts:vec![Bolt::default();lim::BOLTS],order:Vec::with_capacity(lim::BOLTS),shocks:vec![Shock::default();lim::UNITS],shields:vec![Shock::default();lim::UNITS],charges:vec![Shock::default();lim::UNITS],actors:(0..lim::UNITS).map(|_|Actor::default()).collect(),sparks:Vec::with_capacity(lim::SPARKS),time:0.,enhanced:false,main:Path::default(),worlds:[Matrix4::default();lim::NODES_PER_UNIT],view:false}}}
impl Lightning{
 // Definitions are immutable resource admission, outside displayed frames.
 // The returned index also selects the Classic texture on the host.
 pub fn definition(&mut self,def:Definition)->usize{let id=self.defs.len();self.defs.push(def);id}
 // Actor admission allocates only cold topology. Parent indices must precede
 // children; invalid and oversized nodes are ignored without frame panics.
 pub fn actor(&mut self,id:usize,mut actor:Actor)->usize{if id>=lim::UNITS{return NONE;}self.shocks[id].joints=[Joint::default();lim::JOINTS];let n=actor.parents.len().min(lim::NODES_PER_UNIT);actor.parents.truncate(n);for (i,p) in actor.parents.iter_mut().enumerate(){if *p>=i as i32{*p = -1;}}
  actor.bones.retain(|i|*i<n);actor.bones.truncate(n);actor.low.retain(|i|*i<n);actor.low.truncate(n);actor.twitch.retain(|(i,_)|*i<n);actor.twitch.truncate(lim::JOINTS);actor.chest=actor.chest.filter(|i|*i<n);let start=self.actors[id].node_start.unwrap_or(self.nodes.len());
  // Local pose slabs are admitted with the actor, not reserved for the whole
  // army. Reusing a host slot retains its slab through subsequent lives.
  if self.actors[id].node_start.is_none(){self.nodes.resize(start+lim::NODES_PER_UNIT*no::STRIDE,0.);}
  actor.node_start=Some(start);self.actors[id]=actor;start
 }
 // Every unit reference is checked before addressing a generated row.
 // Missing ids observe no body, never the final occupied slot.
 fn fact(&self,id:usize,field:usize)->f64{if id<lim::UNITS{self.units[id*u::STRIDE+field]}else{0.}}
 fn point(&self,id:usize,field:usize)->V{if id<lim::UNITS{vec(&self.units,id*u::STRIDE+field)}else{V::default()}}
 fn same(&self,id:usize,generation:f64)->bool{id<lim::UNITS&&self.fact(id,u::ALIVE)!=0.&&self.fact(id,u::REMOVED)==0.&&self.fact(id,u::GENERATION)==generation}
 fn own(&self,id:usize)->bool{self.frame[f::HAS_VIEW]!=0.&&self.fact(id,u::OWN)!=0.}
 fn shield(&self,id:usize)->bool{self.fact(id,u::SHIELD)!=0.}
 fn on_shield(&self,id:usize,toward:V)->V{let c=self.point(id,u::CENTER);let r=self.point(id,u::RADIUS);let x=(toward.x-c.x)/r.x;let y=(toward.y-c.y)/r.y;let z=(toward.z-c.z)/r.z;let l=hypot3(x,y,z);let l=if l==0.{1.}else{l};V::new(c.x+x/l*r.x,c.y+y/l*r.y,c.z+z/l*r.z)}
 fn outside(&self,id:usize,p:V)->bool{let c=self.point(id,u::CENTER);let r=self.point(id,u::RADIUS);pow((p.x-c.x)/r.x,2.)+pow((p.y-c.y)/r.y,2.)+pow((p.z-c.z)/r.z,2.)>1.}
 fn light(&mut self,kind:f64,p:V,rgb:[f64;3],radius:f64,scale:f64){let count=self.counts[h::LIGHTS] as usize;if count==lim::LIGHTS{self.counts[h::DROPPED]=self.counts[h::DROPPED].saturating_add(1);return;}let at=count*li::STRIDE;self.lights[at+li::KIND]=kind;put(&mut self.lights,at+li::POSITION,p);self.lights[at+li::COLOR..at+li::COLOR+3].copy_from_slice(&rgb);self.lights[at+li::RADIUS]=radius;self.lights[at+li::SCALE]=scale;self.counts[h::LIGHTS]+=1;}
 // Ports Lightning.spawn and ArcLightning.add. Chains and soldier matching use
 // qualified ids; packed bolt slots never become host unit identities.
 pub fn add(&mut self,def:usize,classic:bool,enhanced:bool,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision)->usize{
  if def>=self.defs.len(){return NONE;}let Some(id)=self.bolts.iter().position(|b|!b.active)else{self.counts[h::DROPPED]=self.counts[h::DROPPED].saturating_add(1);return NONE;};
  let d=self.defs[def];let mut b=Bolt{active:true,classic,def,start:vec(&self.spawn,sp::START),end:vec(&self.spawn,sp::END),charge:self.spawn[sp::CHARGE],follow:self.spawn[sp::FOLLOW]!=0.,follow_unit:if self.spawn[sp::FOLLOW_UNIT]<0.{NONE}else{self.spawn[sp::FOLLOW_UNIT] as usize},classic_life:d.fade_time.unwrap_or(0.),..Default::default()};b.from=b.start;b.to=b.end;
  if classic{b.seed_a=seed(random);b.seed_b=seed(random);}b.born=self.time;
  if enhanced{b.charge=if b.charge.is_nan(){0.}else{b.charge.clamp(0.,1.)};b.life=d.fade_time.filter(|x|*x!=0.).unwrap_or(0.6).max(0.25)+0.1;b.seed=seed(random);b.jag=seed(random);b.branch_seed=seed(random);
   b.rgb=rgb(d.color.unwrap_or([130.,170.,255.,255.]),1.6);b.child_rgb=rgb(d.child_color.or(d.color).unwrap_or([140.,180.,255.,100.]),1.4);
   b.child_count=((d.num_children.unwrap_or(1.)*b.charge).floor() as usize).clamp(1, lim::CHILDREN);for child in &mut b.children[..b.child_count]{child.seed=seed(random);child.on=true;}
   let mut parent=false;for &i in &self.order{let other=self.bolts[i];if self.time-other.born<0.25&&other.end.distance_to_squared(&b.start)<1e-4{parent=true;b.delay=other.delay+0.05;b.source=other.target;b.source_gen=other.target_gen;self.bolts[i].air=false;}}
   let count=(self.frame[f::ORDER_COUNT] as usize).min(lim::UNITS);
   if b.follow{let at=vec(&self.spawn,sp::FOLLOW_AT);let mut nearest=0.25;
    for order_index in 0..count{let i=self.unit_order[order_index] as usize;if self.fact(i,u::LISTED)==0.||self.fact(i,u::COMBAT)==0.||self.fact(i,u::REMOVED)!=0.||self.fact(i,u::SOLDIER)==0.||self.fact(i,u::HAS_ORIGIN)==0.{continue;}let distance=self.point(i,u::ORIGIN).distance_to_squared(&at);if distance<nearest{nearest=distance;b.shooter=i;b.shooter_gen=self.fact(i,u::GENERATION);}}
   }
   let mut nearest=1.6*1.6;for order_index in 0..count{let i=self.unit_order[order_index] as usize;if self.fact(i,u::LISTED)==0.||i==b.shooter||self.fact(i,u::SOLDIER)==0.||self.fact(i,u::REMOVED)!=0.||self.fact(i,u::ALIVE)==0.{continue;}let mut p=self.point(i,u::ROOT);p.y+=1.;let distance=p.distance_to_squared(&b.end);if distance<nearest{nearest=distance;b.target=i;b.target_gen=self.fact(i,u::GENERATION);}}
   if b.target==NONE{let delta=sub(b.end,b.start);let length=delta.length();if length>=0.1{if let Some((distance,normal))=collision.cast(b.start,mul(delta,1./length),length+0.25){if (distance-length).abs()<0.35{b.surface=Some(normal);}}}
    if b.surface.is_none()&&!parent{let cone=if self.fact(b.shooter,u::HAS_WEAPON)!=0.{self.fact(b.shooter,u::CONE)}else{30.};b.air=(b.start.distance_to(&b.end)-cone).abs()<0.1;}
   }
  }
  self.bolts[id]=b;self.order.push(id);for unit in [b.shooter,b.source,b.target,b.follow_unit]{if unit<lim::UNITS{self.used[unit]=1;}}self.publish_bolt(id);id
 }
 // Readback is for host follow handles and cold effect inspection only.
 // It is never fed back as the effect lifetime or the next shape.
 fn publish_bolt(&mut self,id:usize){let b=self.bolts[id];let at=id*bo::STRIDE;let a=&mut self.output;a[at+bo::ACTIVE]=if b.active{1.}else{0.};a[at+bo::CLASSIC_LIFE]=b.classic_life;a[at+bo::AGE]=b.age;a[at+bo::DELAY]=b.delay;a[at+bo::ARRIVED]=if b.arrived{1.}else{0.};a[at+bo::AIR]=if b.air{1.}else{0.};a[at+bo::SHOOTER]=if b.shooter==NONE{-1.}else{b.shooter as f64};a[at+bo::SOURCE]=if b.source==NONE{-1.}else{b.source as f64};a[at+bo::TARGET]=if b.target==NONE{-1.}else{b.target as f64};put(a,at+bo::FROM,b.from);put(a,at+bo::TO,b.to);put(a,at+bo::START,b.start);put(a,at+bo::END,b.end);}
 // Ports ArcLightning.place: only the active body's local facts are needed.
 // The foreign parent remains a published matrix from the scene's own owner.
 fn place(&mut self,id:usize){let actor=&self.actors[id];let at=id*u::STRIDE+u::PARENT;let mut parent=Matrix4::default();parent.elements.copy_from_slice(&self.units[at..at+16]);
  for i in 0..actor.parents.len(){let o=actor.node_start.unwrap_or(0)+i*no::STRIDE;let a=&self.nodes;let mut local=Matrix4::default();
   if a[o+no::MATRIX_AUTO_UPDATE]!=0.{let p=vec(a,o+no::POSITION);let s=vec(a,o+no::SCALE);let at=o+no::QUATERNION;let mut q=Q::new(a[at],a[at+1],a[at+2],a[at+3]);
    for (k,(node,_)) in actor.twitch.iter().enumerate(){if *node==i&&self.shocks[id].joints[k].on{q=self.shocks[id].joints[k].applied;}}
    local.compose(&p,&q,&s);
   }else{
    // Native actors deliberately disable automatic updates. Their published
    // local matrix remains authoritative even while a shock changes a quaternion.
    local.elements.copy_from_slice(&a[o+no::MATRIX..o+no::MATRIX+16]);
   }
   let par=actor.parents[i];let world_parent=if par<0{parent}else{self.worlds[par as usize]};self.worlds[i].multiply_matrices(&world_parent,&local);
  }
 }
 fn node_point(&self,id:usize,index:usize)->V{if index<self.actors[id].parents.len(){let e=self.worlds[index].elements;V::new(e[12],e[13],e[14])}else{self.point(id,u::ROOT)}}
 fn chest(&mut self,id:usize)->V{self.place(id);if let Some(i)=self.actors[id].chest{self.node_point(id,i)}else{let mut p=self.point(id,u::ROOT);p.y+=1.;p}}
 fn bolt_from(&mut self,b:&Bolt)->V{if self.same(b.shooter,b.shooter_gen)&&self.fact(b.shooter,u::DEAD)==0.&&self.fact(b.shooter,u::MOUNTED)==0.&&self.fact(b.shooter,u::HAS_WEAPON)!=0.&&self.fact(b.shooter,u::HAS_MUZZLE)!=0.{return self.point(b.shooter,u::MUZZLE);}
  if self.same(b.source,b.source_gen){return if self.shield(b.source){self.on_shield(b.source,b.to)}else{self.chest(b.source)};}b.start
 }
 fn bolt_to(&mut self,b:&Bolt)->V{if self.same(b.target,b.target_gen){if self.own(b.target)&&self.fact(b.target,u::HAS_HAND)!=0.{return self.point(b.target,u::HAND);}
  if self.shield(b.target)&&self.outside(b.target,b.from){return self.on_shield(b.target,b.from);}return self.chest(b.target);}b.end}
 // Ports ArcBatch.push and ArcLightning.line. Counts clamp before every store,
 // including lines, whose old reserve() did not protect its direct writes.
 // Capsule segment emitter for lightning bolts and arcs.
 #[allow(clippy::too_many_arguments)]
 fn push(&mut self,a:V,b:V,na:V,nb:V,rgb:[f64;3],brightness:f64,core:f64,glow:f64,hot:f64){
  if brightness<=0.002{return;}let key=if self.view{h::VIEW_CAPSULES}else{h::CAPSULES};let n=self.counts[key] as usize;if n==lim::CAPSULES{self.counts[h::DROPPED]=self.counts[h::DROPPED].saturating_add(1);return;}
  let d=if self.view{&mut self.view_capsules}else{&mut self.capsules};let o=n*ca::STRIDE;
  for (at,p) in [(ca::A,a),(ca::B,b),(ca::NA,na),(ca::NB,nb)]{d[o+at]=p.x as f32;d[o+at+1]=p.y as f32;d[o+at+2]=p.z as f32;}
  d[o+ca::COLOR]=rgb[0] as f32;d[o+ca::COLOR+1]=rgb[1] as f32;d[o+ca::COLOR+2]=rgb[2] as f32;d[o+ca::COLOR+3]=brightness as f32;
  d[o+ca::SIZE]=core as f32;d[o+ca::SIZE+1]=glow as f32;d[o+ca::SIZE+2]=hot as f32;d[o+ca::SIZE+3]=0.;self.counts[key]+=1;
 }
 // Polyline capsule emitter with miter normals and taper.
 #[allow(clippy::too_many_arguments)]
 fn line(&mut self,path:&Path,shown:f64,rgb:[f64;3],brightness:f64,core:f64,glow:f64,hot:f64,taper:f64){
  let n=(shown.ceil() as usize).min(path.count.saturating_sub(1));if n<1||brightness<=0.002{return;}let mut normals=[V::default();POINTS];
  for (i, normal) in normals.iter_mut().enumerate().take(n).skip(1){let into=sub(path.points[i],path.points[i-1]);let out=sub(path.points[i+1],path.points[i]);let li=into.length();let lo=out.length();if li<1e-6||lo<1e-6{continue;}
   let ni=mul(into,1./li);let m=add(ni,mul(out,1./lo));let length=m.length();*normal=if length<0.1{ni}else{mul(m,1./length)};
  }
  let last=path.count.saturating_sub(1).max(1) as f64;for i in 0..n{let a=path.points[i];let b=if i==n-1&&shown<(n as f64){mix(a,path.points[i+1],shown-i as f64)}else{path.points[i+1]};let scale=1.+(taper-1.)*((i as f64+0.5)/last);
   self.push(a,b,if i>0{normals[i]}else{V::default()},if i+1<n{normals[i+1]}else{V::default()},rgb,brightness,core*scale,glow*scale,hot);
  }
 }
 // Ports Lightning.strand/ribbon. Branch start records are stack scratch,
 // because recursive strands overwrite the reusable main point bank.
 // Classic ribbon strand generator matching Lightning.strand.
 #[allow(clippy::too_many_arguments)]
 fn strand(&mut self,from:V,to:V,half_width:f64,spread:f64,break_distance:f64,texture_size:f64,color:[f64;4],seed_a:u32,seed_b:u32,phase:f64,branches:Option<[f64;4]>){
  let length=from.distance_to(&to);let n=((length/break_distance.max(1e-3)).floor() as usize).max(3);if n>100{return;}
  let dir=mul(sub(to,from),1./length.max(1e-6));let mut ra=ShapeRandom(seed_a);let mut rb=ShapeRandom(seed_b);let mut points=[V::default();102];let mut offsets=[V::default();102];let mut widths=[0.;102];
  for i in 0..=n{let mut p=mix(from,to,i as f64/n as f64);if i>0&&i<n{let a=mul(V::new(ra.next()-0.5,ra.next()-0.5,ra.next()-0.5),spread);let b=mul(V::new(rb.next()-0.5,rb.next()-0.5,rb.next()-0.5),spread);let mut offset=a;offset.lerp(&b,phase);offset=scaled(offset,dir,-offset.dot(&dir));offsets[i]=offset;p=add(p,offset);}points[i]=p;
   widths[i]=if i==0{half_width}else{(half_width*(1.-(i-1) as f64/(n+1) as f64)).max(2.*half_width/(n+1) as f64)};
  }
  let base=self.counts[h::VERTICES] as usize;let idx=self.counts[h::INDICES] as usize;
  if base+(n+1)*2<=lim::VERTICES&&idx+n*6<=lim::VERTICES*3{let eye=vec(&self.frame,f::EYE);let mut u=0.;let mut vertex=base;
   for i in 0..=n{let p=points[i];let prev=points[i.saturating_sub(1)];let next=points[(i+1).min(n)];let side=mul(norm(cross(sub(next,prev),sub(eye,p))),widths[i]);if i>0{u+=p.distance_to(&prev)/texture_size.max(1e-3);}let alpha=if i==0||i==n{0.}else{color[3]};
    for (sign,v) in [(1.,0.),(-1.,1.)]{self.positions[vertex*3]=(p.x+side.x*sign) as f32;self.positions[vertex*3+1]=(p.y+side.y*sign) as f32;self.positions[vertex*3+2]=(p.z+side.z*sign) as f32;self.uvs[vertex*2]=u as f32;self.uvs[vertex*2+1]=v;for (k, &c) in color.iter().take(3).enumerate(){self.colors[vertex*4+k]=c as f32;}self.colors[vertex*4+3]=alpha as f32;vertex+=1;}
   }
   for i in 0..n{let a=(base+i*2) as u32;self.indices[idx+i*6..idx+i*6+6].copy_from_slice(&[a,a+1,a+2,a+1,a+3,a+2]);}self.counts[h::VERTICES]=vertex as u32;self.counts[h::INDICES]=(idx+n*6) as u32;
  }else{self.counts[h::DROPPED]=self.counts[h::DROPPED].saturating_add(1);}
  if let Some(o)=branches{let mut random=ShapeRandom(seed_a^0x9e3779b9);let mut spots=[(V::default(),V::default(),0.);100];let mut count=0;
   for i in 1..n{if count as f64>=o[1]{break;}
   if random.next()>=o[0]{continue;}spots[count]=(points[i],norm(scaled(offsets[i],dir,o[3])),widths[i]);count+=1;}
   for (k,&(start,way,width)) in spots[..count].iter().enumerate(){self.strand(start,scaled(start,way,o[2]),width,spread,break_distance,texture_size,color,seed_a.wrapping_add(k as u32+1),seed_b.wrapping_add(k as u32+1),phase,None);}
  }
 }
 // Classic alpha uses authored byte colors, while Enhanced uses sheath hues.
 // Children consume fresh seeds even when their ribbon exceeds its segment cap.
 fn classic(&mut self,b:&Bolt,random:&mut dyn FnMut()->f64){let d=self.defs[b.def];let fade=(2.*b.classic_life/d.fade_time.unwrap_or(0.)).min(1.);let width=0.3+0.7*b.charge;let alpha=fade*(0.5+0.5*b.charge);let mut color=d.color.unwrap_or([0.;4]);for c in &mut color{*c/=255.;}color[3]*=alpha;
  self.strand(b.start,b.end,d.width.unwrap_or(0.)*width,d.spread_factor.unwrap_or(0.),d.break_distance.unwrap_or(0.),d.texture_size.unwrap_or(0.),color,b.seed_a,b.seed_b,b.phase,Some([d.branch_factor.unwrap_or(0.),d.max_branches.unwrap_or(0.),d.branch_length.unwrap_or(0.),d.branch_spread_factor.unwrap_or(0.)]));
  let children=((d.num_children.unwrap_or(0.)*b.charge).floor() as usize).clamp(1, lim::CHILDREN);let mut color=d.child_color.unwrap_or([0.;4]);for c in &mut color{*c/=255.;}color[3]*=alpha;
  for _ in 0..children{let s=seed(random);self.strand(b.start,b.end,d.child_width.unwrap_or(0.)*width,d.child_spread_factor.unwrap_or(0.),d.child_break_distance.unwrap_or(0.),d.child_texture_size.unwrap_or(0.),color,s,s,0.,None);}
 }
 // Ports ArcLightning.spray and stepSpark. The fixed 600-record store keeps
 // the old visual ceiling even after an arbitrarily long pause or burst.
 fn spray(&mut self,b:&mut Bolt,at:V,count:usize,speed:f64,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision){let floor=if let Some(floor)=b.floor{floor}else{let mut floor=at.y-1.5;if b.target!=NONE{floor=self.point(b.target,u::ROOT).y;}else{let from=V::new(at.x,at.y+0.2,at.z);if let Some((distance,_))=collision.cast(from,V::new(0.,-1.,0.),4.){floor=at.y+0.2-distance;}}b.floor=Some(floor);floor};
  let away=b.surface.unwrap_or_else(||norm(sub(b.from,b.to)));
  for _ in 0..count.min(lim::SPARKS-self.sparks.len()){let mut v=norm(V::new(random()*2.-1.,random()*2.-1.,random()*2.-1.));v=mul(norm(scaled(scaled(v,away,0.9),UP,0.6)),(2.+random()*4.5)*speed);self.sparks.push(Spark{p:at,v,age:0.,life:0.25+random()*0.4,floor,bounced:false,rgb:b.rgb});}
 }
 // Age is advanced before gravity and the single permitted floor bounce.
 // Stable compaction preserves spark order and therefore translucent draw order.
 fn draw_sparks(&mut self,dt:f64){let mut kept=0;for i in 0..self.sparks.len(){let mut s=self.sparks[i];s.age+=dt;if s.age>=s.life{continue;}s.v.y-=9.8*dt;s.p=scaled(s.p,s.v,dt);if s.p.y<s.floor{s.p.y=s.floor;if s.bounced{continue;}s.bounced=true;s.v.y*= -0.35;s.v.x*=0.55;s.v.z*=0.55;}self.sparks[kept]=s;kept+=1;}self.sparks.truncate(kept);
  for i in 0..kept{let s=self.sparks[i];let k=1.-s.age/s.life;self.push(s.p,scaled(s.p,s.v,-0.022),V::default(),V::default(),s.rgb,1.4*pow(k,0.7),0.006,0.022,1.);}
 }
 // Ports ArcLightning.land and shieldStruck. Existing generations qualify
 // every retained association, including a shock refreshed by another bolt.
 fn land(&mut self,b:&mut Bolt,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision){if b.air{return;}let to=self.bolt_to(b);b.own=self.same(b.target,b.target_gen)&&self.own(b.target);let unit=if self.same(b.target,b.target_gen){b.target}else{NONE};let shield=self.shield(unit);
  if shield{b.surface=Some(norm(sub(to,self.point(unit,u::CENTER))));}
  if !b.own{self.spray(b,to,10+(16.*b.charge).round() as usize,1.,random,collision);}self.light(1.,to,b.rgb,6.+4.*b.charge,0.16);if unit==NONE{return;}
  let mut shock=if shield{self.shields[unit]}else{self.shocks[unit]};let life=if shield{0.55+0.45*b.charge}else{0.7+0.7*b.charge};let strength=0.6+0.4*b.charge;
  if shock.on&&shock.generation==self.fact(unit,u::GENERATION){shock.life=(shock.life-shock.age).max(life);shock.age=0.;shock.strength=shock.strength.max(strength);if !shield{shock.rgb=b.rgb;}}
  else{if shield{self.shield_order.retain(|i|*i!=unit);self.shield_order.push(unit);}else{self.shock_order.retain(|i|*i!=unit);self.shock_order.push(unit);}shock=Shock{on:true,generation:self.fact(unit,u::GENERATION),age:0.,life,strength,rgb:b.rgb,seed:if shield{0}else{seed(random)},..Default::default()};}
  if shield{let c=self.point(unit,u::CENTER);let r=self.point(unit,u::RADIUS);shock.impact=norm(V::new((to.x-c.x)/r.x,(to.y-c.y)/r.y,(to.z-c.z)/r.z));self.shields[unit]=shock;}else{self.shocks[unit]=shock;}
 }
 // Ports twist. Its envelope is sampled at the original main-channel index,
 // so skipping alternate vertices leaves both endpoints exactly anchored.
 fn twist(&self,main:&Path,out:&mut Path,seed:u32,amplitude:f64,u:V,w:V){let n=main.count-1;let stride=if n>=16{2}else{1};let m=n/stride;for k in 0..=m{let i=(k*stride).min(n);let s=i as f64/n as f64;let envelope=pow(sin(std::f64::consts::PI*s),0.7);let x=(wave(seed,k as f64*0.42)*amplitude+signed(seed,(k*3+7) as i32)*0.04)*envelope;let y=(wave(seed.wrapping_add(101),k as f64*0.42)*amplitude+signed(seed,(k*3+8) as i32)*0.04)*envelope;out.points[k]=scaled(scaled(main.points[i],u,x),w,y);}out.count=m+1;}
 // Secondary branch emitter along the main bolt path.
 #[allow(clippy::too_many_arguments)]
 fn branches(&mut self,b:&Bolt,rgb:[f64;3],brightness:f64,core:f64,glow:f64,charge:f64,u:V,w:V){let d=self.defs[b.def];let main=self.main;let n=main.count-1;let length=b.from.distance_to(&b.to);let most=((d.max_branches.unwrap_or(3.)*(0.5+0.5*charge)).round() as usize).saturating_add(if length>12.{1}else{0}).min(100);let odds=0.35+d.branch_factor.unwrap_or(0.15);
  let mut branch=Path::default();let mut twig=Path::default();for k in 0..most{let seed=b.branch_seed.wrapping_add(k as u32*977);if hash(seed,1)>odds{continue;}let at=(n as f64*(0.12+0.68*hash(seed,2))).floor() as usize;let start=main.points[at];let next=main.points[(at+1).min(n)];let angle=hash(seed,3)*std::f64::consts::PI*2.;let spread=d.branch_spread_factor.unwrap_or(0.8)*(0.7+0.8*hash(seed,4));let mut dir=norm(sub(next,start));dir=norm(scaled(scaled(dir,u,cos(angle)*spread),w,sin(angle)*spread));let reach=d.branch_length.unwrap_or(4.)*(0.22+0.45*hash(seed,5))*(0.55+0.45*charge);let end=scaled(start,dir,reach);let (bu,bw)=basis(V::default(),dir);displace(&mut branch,start,end,3,bu,bw,jag(0.3,0.35,seed,seed^0x5bd1e995));self.line(&branch,(branch.count-1) as f64,rgb,brightness*0.8,core*0.75,glow*0.7,0.7,0.1);
   if hash(seed,6)<0.45{let from=branch.points[3+(hash(seed,7)*2.).floor() as usize];let angle2=angle+(hash(seed,8)-0.5)*2.4;
    // JS's d.copy(dir) aliases dir itself: retain that mutation for later use.
    dir=norm(scaled(scaled(dir,u,cos(angle2)*0.9),w,sin(angle2)*0.9));let end=scaled(from,dir,reach*0.45);let (bu,bw)=basis(V::default(),dir);displace(&mut twig,from,end,2,bu,bw,jag(0.3,0.2,seed.wrapping_add(5),seed^0x27d4eb2f));self.line(&twig,4.,rgb,brightness*0.6,core*0.45,glow*0.45,0.5,0.05);
   }
  }
 }
 // Terminal fizzle arc emitter when a bolt expires in air.
 #[allow(clippy::too_many_arguments)]
 fn fizzle(&mut self,b:&Bolt,to:V,rgb:[f64;3],brightness:f64,core:f64,glow:f64,u:V,w:V){let dir=norm(sub(b.to,b.from));let mut path=Path::default();for k in 0..3{let seed=b.branch_seed.wrapping_add(31*k+7);let angle=hash(seed,1)*std::f64::consts::PI*2.;let way=norm(scaled(scaled(dir,u,cos(angle)*0.9),w,sin(angle)*0.9));let end=scaled(to,way,0.3+0.6*hash(seed,2));let (bu,bw)=basis(V::default(),way);displace(&mut path,to,end,2,bu,bw,jag(0.32,0.12,seed,seed^0x2545f491));self.line(&path,4.,rgb,brightness*0.5,core*0.3,glow*0.4,0.4,0.05);}}
 // Ports ArcLightning.drawBolt. One retained record owns leader, strike,
 // child and afterglow timing; no state is mirrored back into JS objects.
 fn draw_bolt(&mut self,b:&mut Bolt,dt:f64,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision)->bool{b.age+=dt;let t=b.age-b.delay;if t<0.{return true;}
  if t>=b.life{return false;}let d=self.defs[b.def];let charge=b.charge;b.from=self.bolt_from(b);b.to=self.bolt_to(b);let from=b.from;let to=b.to;let length=from.distance_to(&to);if length<0.05{return true;}
  let leader=0.035+0.0014*length;let reach=(t/leader).min(1.);if reach>=1.&&!b.arrived{b.arrived=true;b.flash=1.;self.land(b,random,collision);}b.flash=(b.flash-dt*8.).max(0.);
  if self.time>=b.next_strike{b.jag=seed(random);b.branch_seed=seed(random);b.strike=0.62+0.38*random();if random()<0.12{b.strike*=0.45;}b.next_strike=self.time+0.028+random()*0.045;if b.arrived&&!b.air&&!b.own&&random()<0.45{self.spray(b,to,1+(random()*3.).floor() as usize,0.6,random,collision);}}
  let remain=1.-t/b.life;let fade=if remain>0.45{1.}else{pow(remain/0.45,1.4)};let width_scale=(0.3+0.7*charge)*(0.75+0.25*fade);let brightness=(0.55+0.45*charge)*b.strike*fade*(1.+1.5*b.flash);let cool=1.-fade;let rgb=[(1.-cool*0.6)*b.rgb[0]+cool*0.6*0.62,(1.-cool*0.6)*b.rgb[1]+cool*0.6*0.42,b.rgb[2]];
  let (u,w)=basis(from,to);let depth=(log2(length/0.32).ceil() as usize).clamp(2,8);let spread=d.spread_factor.unwrap_or(0.8).max(0.1)*0.75*(0.4+length/16.).min(1.);displace(&mut self.main,from,to,depth,u,w,Jag{time:self.time,writhe:2.6,coarse:2,..jag(0.2,spread,b.seed,b.jag)});
  let main=self.main;let n=main.count-1;let width=d.width.unwrap_or(0.5);let core=width*0.07*width_scale;let glow=width*0.3*width_scale;let shown=reach*n as f64;self.line(&main,shown,rgb,brightness,core,glow,0.9,if b.air{0.25}else{0.7});if reach<1.{let at=(shown.floor() as usize).min(n-1);let tip=mix(main.points[at],main.points[at+1],shown-at as f64);self.push(tip,tip,V::default(),V::default(),rgb,1.4*brightness,core*2.2,glow*2.2,1.);}
  if b.arrived{let child_width=d.child_width.unwrap_or(width*0.8);let child_alpha=d.child_color.map(|c|c[3]).unwrap_or(100.)/255.;let amplitude=d.child_spread_factor.unwrap_or(1.)*0.2*(0.6+0.4*charge)*(0.35+length/12.).min(1.);let mut child_path=Path::default();
   for child in &mut b.children[..b.child_count]{if self.time>=child.next{child.seed=seed(random);child.on=random()<0.78;child.next=self.time+0.02+random()*0.04;}
   if !child.on{continue;}self.twist(&main,&mut child_path,child.seed,amplitude,u,w);self.line(&child_path,(child_path.count-1) as f64,b.child_rgb,brightness*(0.5+child_alpha),child_width*0.035*width_scale,child_width*0.2*width_scale,0.5,1.);}
   self.branches(b,rgb,brightness,core,glow,charge,u,w);
  }
  let flicker=brightness*(0.85+0.15*random());if b.shooter!=NONE||b.follow{self.push(from,from,V::default(),V::default(),rgb,0.9*flicker,core*1.5,glow*if from.distance_to_squared(&vec(&self.frame,f::EYE))<2.25{0.6}else{1.1},1.);}
  if b.arrived&&!b.air&&!b.own{self.push(to,to,V::default(),V::default(),rgb,flicker,core*2.,glow*1.8,1.);self.push(to,to,V::default(),V::default(),rgb,0.18*flicker,0.,glow*6.,0.);}else if b.arrived&&b.air{self.fizzle(b,to,rgb,brightness,core,glow,u,w);}
  let light=1.1*flicker;if b.shooter!=NONE||b.follow{self.light(0.,from,rgb,4.+2.5*charge,light*0.8);}
  if b.arrived&&!b.air{self.light(0.,to,rgb,5.+4.*charge,light);}
  if length>10.&&reach>=1.{self.light(0.,mix(from,to,0.5),rgb,(length*0.5).min(10.),light*0.6);}true
 }
 // Ports twitch/endShock: an animation-written quaternion becomes the new
 // base; an untouched joint keeps its previous base until the shock ends.
 fn twitch(&mut self,id:usize,shock:&mut Shock,intensity:f64,random:&mut dyn FnMut()->f64){let actor=&self.actors[id];if self.time>=shock.next_twitch{shock.next_twitch=self.time+0.04+random()*0.035;for (i,(_,amount)) in actor.twitch.iter().enumerate(){let r=amount*intensity.min(1.);shock.joints[i].offset.set_from_euler(&Euler::new((random()*2.-1.)*r,(random()*2.-1.)*r,(random()*2.-1.)*r,three::EulerOrder::XYZ));}}
  for (i,(node,_)) in actor.twitch.iter().enumerate(){let at=self.actors[id].node_start.unwrap_or(0)+*node*no::STRIDE+no::QUATERNION;let q=Q::new(self.nodes[at],self.nodes[at+1],self.nodes[at+2],self.nodes[at+3]);let j=&mut shock.joints[i];if !j.on||q!=j.applied{j.base=q;}j.on=true;j.applied=j.base;j.applied.multiply(&j.offset);let at=(id*lim::JOINTS+i)*jo::STRIDE;self.joints[at+jo::ON]=1.;self.joints[at+jo::QUATERNION..at+jo::QUATERNION+4].copy_from_slice(&[j.applied.x,j.applied.y,j.applied.z,j.applied.w]);}
 }
 fn end_shock(&mut self,id:usize,shock:&mut Shock){for (i,(node,_)) in self.actors[id].twitch.iter().enumerate(){let at=self.actors[id].node_start.unwrap_or(0)+*node*no::STRIDE+no::QUATERNION;let q=Q::new(self.nodes[at],self.nodes[at+1],self.nodes[at+2],self.nodes[at+3]);let j=&mut shock.joints[i];let at=(id*lim::JOINTS+i)*jo::STRIDE;if j.on&&q==j.applied{self.joints[at+jo::ON]=1.;self.joints[at+jo::QUATERNION..at+jo::QUATERNION+4].copy_from_slice(&[j.base.x,j.base.y,j.base.z,j.base.w]);let input=self.actors[id].node_start.unwrap_or(0)+*node*no::STRIDE+no::QUATERNION;self.nodes[input..input+4].copy_from_slice(&[j.base.x,j.base.y,j.base.z,j.base.w]);}j.on=false;}shock.on=false;self.shocks_output[id*so::STRIDE+so::ACTIVE]=0.;}
 fn off_skin(mut p:V,root:V,by:f64)->V{let dx=p.x-root.x;let dz=p.z-root.z;let length=hypot(dx,dz);if length>1e-4{p.x+=dx/length*by;p.z+=dz/length*by;}p}
 // Ports pickArcs/drawCrawl. Bone order is registered from the actor's own
 // array, rather than the traversal order of its scene hierarchy.
 fn pick_arcs(&mut self,id:usize,shock:&mut Shock,intensity:f64,random:&mut dyn FnMut()->f64){shock.next_arcs=self.time+0.03+random()*0.035;shock.arc_count=0;let actor=&self.actors[id];let bones=&actor.bones;if bones.is_empty(){return;}let count=2+(2.5*intensity.min(1.2)).round() as usize;
  for _ in 0..count.min(lim::ARCS-1){let a=bones[(random()*bones.len() as f64).floor() as usize];let pa=self.node_point(id,a);let mut b=NONE;for _ in 0..6{if b!=NONE{break;}let candidate=bones[(random()*bones.len() as f64).floor() as usize];if candidate==a{continue;}let distance=pa.distance_to(&self.node_point(id,candidate));if distance>0.15&&distance<0.95{b=candidate;}}if b!=NONE{shock.arcs[shock.arc_count]=Arc{a,b,seed:seed(random),..Default::default()};shock.arc_count+=1;}}
  if random()<0.4{let low=if actor.low.is_empty(){bones}else{&actor.low};let a=low[(random()*low.len() as f64).floor() as usize];let angle=random()*std::f64::consts::PI*2.;let out=0.35+random()*0.6;shock.arcs[shock.arc_count]=Arc{a,b:NONE,ground:true,gx:sin(angle)*out,gz:cos(angle)*out,seed:seed(random),..Default::default()};shock.arc_count+=1;}
 }
 fn draw_crawl(&mut self,id:usize,shock:&Shock,arc:Arc,intensity:f64,random:&mut dyn FnMut()->f64){let root=self.point(id,u::ROOT);let pa=Self::off_skin(self.node_point(id,arc.a),root,0.07);let pb=if !arc.ground{Self::off_skin(self.node_point(id,arc.b),root,0.07)}else{V::new(root.x+arc.gx,root.y+0.03,root.z+arc.gz)};let (u,w)=basis(pa,pb);let mut path=Path::default();displace(&mut path,pa,pb,3,u,w,jag(0.34,0.1,arc.seed,seed(random)));self.line(&path,8.,shock.rgb,1.1*intensity,0.006,0.03,0.8,if arc.ground{0.4}else{1.});}
 // Ports pickArmArcs/gunPoint. Grip, wrist and muzzle are foreign pose facts;
 // all interpolation, camera rotation and random offsets remain native.
 fn pick_arm_arcs(&mut self,id:usize,shock:&mut Shock,random:&mut dyn FnMut()->f64){shock.next_arcs=self.time+0.03+random()*0.035;shock.arc_count=0;let count=2+(random()*2.).floor() as usize;for _ in 0..count{let t1=0.25+random()*0.75;let t2=(t1+if random()<0.5{-1.}else{1.}*(0.15+random()*0.35)).clamp(0.2,1.);let mut arc=Arc{t1,t2,..Default::default()};arc.o1=[(random()*2.-1.)*0.035,(random()*2.-1.)*0.035];arc.o2=[(random()*2.-1.)*0.035,(random()*2.-1.)*0.035];arc.seed=seed(random);shock.arcs[shock.arc_count]=arc;shock.arc_count+=1;}
  if self.fact(id,u::HAS_WRIST)!=0.&&random()<0.5{let mut arc=Arc{t1:0.3+random()*0.4,t2:-1.,..Default::default()};arc.o1=[(random()*2.-1.)*0.035,(random()*2.-1.)*0.035];arc.seed=seed(random);shock.arcs[shock.arc_count]=arc;shock.arc_count+=1;}
 }
 fn gun_point(&self,id:usize,t:f64,o:[f64;2])->Option<V>{if t<0.{return if self.fact(id,u::HAS_WRIST)!=0.{Some(self.point(id,u::WRIST))}else{None};}
  if self.fact(id,u::HAS_CURRENT_MUZZLE)==0.{return None;}let p=mix(self.point(id,u::GRIP),self.point(id,u::CURRENT_MUZZLE),t);let at=f::CAMERA;let q=Q::new(self.frame[at],self.frame[at+1],self.frame[at+2],self.frame[at+3]);let mut right=SIDE;right.apply_quaternion(&q);let mut up=UP;up.apply_quaternion(&q);Some(scaled(scaled(p,right,o[0]),up,o[1]))}
 fn draw_arm_arc(&mut self,id:usize,shock:&Shock,arc:Arc,intensity:f64,random:&mut dyn FnMut()->f64){if let (Some(a),Some(b))=(self.gun_point(id,arc.t1,arc.o1),self.gun_point(id,arc.t2,arc.o2)){let (u,w)=basis(a,b);let mut path=Path::default();displace(&mut path,a,b,3,u,w,jag(0.3,0.04,arc.seed,seed(random)));self.line(&path,8.,shock.rgb,1.3*intensity,0.0025,0.012,0.9,1.);}}
 // Overlay visibility and intensity are complete native results.
 // The host copies them alongside the matching body resource identity.
 fn publish_shock(&mut self,id:usize,shock:&Shock,intensity:f64,shield:bool){let o=id*so::STRIDE;let d=&mut self.shocks_output;d[o+so::ACTIVE]=if shock.on{1.}else{0.};d[o+so::GENERATION]=shock.generation;d[o+so::AGE]=shock.age;d[o+so::LIFE]=shock.life;d[o+so::STRENGTH]=shock.strength;d[o+so::INTENSITY]=intensity;d[o+so::OVERLAY_INTENSITY]=intensity*0.7;d[o+so::SHOW]=if intensity>0.01&&self.units[id*u::STRIDE+u::BODY_VISIBLE]!=0.{1.}else{0.};d[o+so::TIME]=self.time;d[o+so::SEED]=(shock.seed%1000) as f64*0.37;d[o+so::COLOR..o+so::COLOR+3].copy_from_slice(&shock.rgb);d[o+so::SHIELD]=if shield{1.}else{0.};}
 // Ports drawShock. Shock placement precedes bolt placement, preserving the
 // old effect's chest anchors after its own upper-body twitch.
 fn draw_shock(&mut self,id:usize,dt:f64,random:&mut dyn FnMut()->f64){let mut shock=self.shocks[id];shock.age+=dt;if !self.same(id,shock.generation)||shock.age>=shock.life{self.end_shock(id,&mut shock);self.shocks[id]=shock;return;}
  if self.time>=shock.next_flicker{shock.flicker=0.5+0.5*random();shock.next_flicker=self.time+1./30.;}let k=1.-shock.age/shock.life;let intensity=(shock.age/0.04).min(1.)*pow(k,1.2)*shock.flicker*shock.strength*(1.+1.4*(1.-shock.age/0.15).max(0.));
  if self.fact(id,u::WALKER)==0.&&self.fact(id,u::SEAT)==0.&&self.fact(id,u::HELD)==0.{self.twitch(id,&mut shock,intensity,random);}self.shocks[id]=shock;let chest=self.chest(id);let shown=self.fact(id,u::SHOWN)!=0.;
  if shown{if self.time>=shock.next_arcs{self.pick_arcs(id,&mut shock,intensity,random);}for i in 0..shock.arc_count{self.draw_crawl(id,&shock,shock.arcs[i],intensity,random);}}
  else if self.own(id){if self.time>=shock.next_arcs{self.pick_arm_arcs(id,&mut shock,random);}self.view=true;for i in 0..shock.arc_count{self.draw_arm_arc(id,&shock,shock.arcs[i],intensity,random);}self.view=false;}
  self.light(0.,chest,shock.rgb,3.+1.5*shock.strength,0.9*intensity);self.publish_shock(id,&shock,intensity,false);self.shocks[id]=shock;
 }
 fn turned(from:V,angle:f64,random:&mut dyn FnMut()->f64)->V{let (u,w)=basis(V::default(),from);let spin=random()*std::f64::consts::PI*2.;norm(scaled(scaled(mul(from,cos(angle)),u,sin(angle)*cos(spin)),w,sin(angle)*sin(spin)))}
 // Ports drawShieldShock/turned. Each path stays on the current ellipsoid;
 // the impact is retained in normalized shield coordinates as it moves.
 fn draw_shield(&mut self,id:usize,dt:f64,random:&mut dyn FnMut()->f64){let mut shock=self.shields[id];shock.age+=dt;if !self.same(id,shock.generation)||!self.shield(id)||shock.age>=shock.life{shock.on=false;self.shields[id]=shock;return;}
  if self.time>=shock.next_flicker{shock.flicker=0.55+0.45*random();shock.next_flicker=self.time+1./30.;}let k=1.-shock.age/shock.life;let intensity=(shock.age/0.03).min(1.)*pow(k,1.3)*shock.flicker*shock.strength*(1.+1.2*(1.-shock.age/0.12).max(0.));
  if self.time>=shock.next_arcs{shock.next_arcs=self.time+0.03+random()*0.03;shock.arc_count=(4.+(4.*intensity.min(1.)).round()) as usize;for i in 0..shock.arc_count{let va=Self::turned(shock.impact,random()*0.3,random);let reach=if i<2{0.3+random()*0.4}else{0.8+random()*(0.8+0.6*k)};let vb=Self::turned(shock.impact,reach,random);shock.arcs[i]=Arc{va,vb,..Default::default()};}}
  let c=self.point(id,u::CENTER);let r=self.point(id,u::RADIUS);let mut web=Path{count:16,..Path::default()};for arc in &shock.arcs[..shock.arc_count]{let jag=seed(random);for i in 0..16{let t=i as f64/15.;let sway=sin(std::f64::consts::PI*t)*0.07;let mut d=arc.va;d.lerp(&arc.vb,t);d.normalize();let (u,w)=basis(V::default(),d);d=norm(scaled(scaled(d,u,signed(jag,(i*2) as i32)*sway),w,signed(jag,(i*2+1) as i32)*sway));web.points[i]=V::new(c.x+d.x*r.x*1.02,c.y+d.y*r.y*1.02,c.z+d.z*r.z*1.02);}self.line(&web,15.,shock.rgb,1.4*intensity,0.008,0.045,0.85,0.3);}
  let toward=add(c,V::new(shock.impact.x*r.x,shock.impact.y*r.y,shock.impact.z*r.z));let at=self.on_shield(id,toward);self.push(at,at,V::default(),V::default(),shock.rgb,1.3*intensity,0.03,0.22,1.);self.light(0.,at,shock.rgb,3.5+1.5*shock.strength,0.8*intensity);self.shields[id]=shock;
 }
 // Ports drawCharges: all arc selection, pulse and radius math is native.
 // JS publishes the current weapon's authored color and raw held duration.
 fn draw_charge(&mut self,id:usize,random:&mut dyn FnMut()->f64){if self.fact(id,u::CHARGING)==0.{self.charges[id].on=false;return;}
  if self.fact(id,u::HAS_CURRENT_MUZZLE)==0.{return;}let most=self.fact(id,u::MAX_PRESSED);let charge=(self.fact(id,u::PRESSED)/if most==0.{1.}else{most}).clamp(0.,1.);let mut crackle=self.charges[id];
  if !crackle.on{crackle=Shock{on:true,..Default::default()};let at=id*u::STRIDE+u::COLOR;crackle.rgb=rgb([self.units[at],self.units[at+1],self.units[at+2],self.units[at+3]],1.6);}
  let muzzle=self.point(id,u::CURRENT_MUZZLE);let back=if self.fact(id,u::FIRST_PERSON)!=0.{Some(vec(&self.frame,f::EYE))}else if self.fact(id,u::HAS_ORIGIN)!=0.{Some(self.point(id,u::ORIGIN))}else{None};let barrel=back.map(|p|sub(p,muzzle)).filter(|p|p.length_sq()>1e-4).map(norm);let size=if muzzle.distance_to_squared(&vec(&self.frame,f::EYE))<2.25{0.55}else{1.};
  if self.time>=crackle.next_arcs{crackle.next_arcs=self.time+0.03+random()*0.03;crackle.arc_count=0;let out=1+(3.*charge).round() as usize;for _ in 0..out{let mut v=norm(V::new(random()*2.-1.,random()*2.-1.,random()*2.-1.));if let Some(barrel)=barrel{v=norm(scaled(v,barrel,-0.6));}crackle.arcs[crackle.arc_count]=Arc{va:v,reach:(0.08+random()*(0.1+0.4*charge))*size,seed:seed(random),..Default::default()};crackle.arc_count+=1;}
   if charge>0.5&&random()<charge*0.6{let mut v=norm(V::new(random()*2.-1.,random()*1.6-1.,random()*2.-1.));if let Some(barrel)=barrel{v=norm(scaled(v,barrel,-0.8));}crackle.arcs[crackle.arc_count]=Arc{va:v,reach:(0.45+random()*0.6*charge)*size,seed:seed(random),..Default::default()};crackle.arc_count+=1;}
   let wrap=if barrel.is_some(){(1.+2.*charge*random()).round() as usize}else{0};for _ in 0..wrap{let v=norm(V::new(random()*2.-1.,random()*2.-1.,random()*2.-1.));crackle.arcs[crackle.arc_count]=Arc{va:v,reach:(0.015+random()*0.04)*size,along:(0.08+random()*(0.12+0.2*charge))*size,seed:seed(random),..Default::default()};crackle.arc_count+=1;}
  }
  let bright=0.45+1.1*charge;self.view=self.own(id);let mut path=Path::default();for arc in &crackle.arcs[..crackle.arc_count]{let mut end=muzzle;if arc.along!=0.{if let Some(barrel)=barrel{end=scaled(end,barrel,arc.along);}}end=scaled(end,arc.va,arc.reach);let (u,w)=basis(muzzle,end);let long=arc.reach>0.4*size;let depth=if long{3}else{2};displace(&mut path,muzzle,end,depth,u,w,jag(0.36,if long{0.14}else{0.06},arc.seed,seed(random)));self.line(&path,(path.count-1) as f64,crackle.rgb,bright,0.005*size,0.024*size,0.8,0.3);}
  let pulse=0.85+0.15*sin(self.time*38.)+if charge>=0.98{0.25*random()}else{0.};self.push(muzzle,muzzle,V::default(),V::default(),crackle.rgb,bright*pulse,(0.015+0.02*charge)*size,(0.05+0.16*charge)*size*pulse,1.);self.view=false;self.light(0.,muzzle,crackle.rgb,1.5+2.5*charge,(0.3+0.7*charge)*pulse);self.charges[id]=crackle;
 }
 // Ports Lightning.update/collectAim's reader collection. The host receives
 // only ids needed by the next phase, and prepares each gun origin once.
 // Charging guns retain registry order even after a host id is recycled.
 // Body shock order remains birth order, independent of those host slots.
 pub fn prepare(&mut self){self.readers.fill(0);self.counts[h::AIM]=0;self.counts[h::FOLLOW]=0;let mut selected=[false;lim::UNITS];
  for &id in &self.order{let b=self.bolts[id];if self.frame[f::ENHANCED]!=0.||self.enhanced{
   if self.same(b.shooter,b.shooter_gen)&&self.fact(b.shooter,u::HAS_WEAPON)!=0.{self.readers[b.shooter]|=rd::CASTER as u32;}
   for unit in [b.source,b.target]{if unit<lim::UNITS{self.readers[unit]|=rd::POSE as u32;}}
   if self.own(b.target){self.readers[b.target]|=rd::VIEW as u32;}
  }
   if b.classic_life>self.frame[f::DT]&&b.follow{let n=self.counts[h::FOLLOW] as usize;self.follows[n]=id as u32;self.counts[h::FOLLOW]+=1;let unit=b.follow_unit;if unit<lim::UNITS&&!selected[unit]{selected[unit]=true;let n=self.counts[h::AIM] as usize;self.aim[n]=unit as u32;self.counts[h::AIM]+=1;}}
  }
  if self.frame[f::ENHANCED]==0.&&!self.enhanced{return;}for id in 0..lim::UNITS{if self.shocks[id].on{self.readers[id]|=rd::POSE as u32;if self.own(id){self.readers[id]|=(rd::CURRENT|rd::VIEW) as u32;}}
   if self.shields[id].on{self.readers[id]|=rd::DETAILS as u32;}}
  if self.frame[f::ENHANCED]==0.{return;}
  for at in 0..(self.frame[f::ORDER_COUNT] as usize).min(lim::UNITS){let id=self.unit_order[at] as usize;if id>=lim::UNITS{continue;}
   if self.fact(id,u::LISTED)!=0.&&self.fact(id,u::CHARGING)!=0.&&self.fact(id,u::DEAD)==0.&&self.fact(id,u::MOUNTED)==0.&&self.fact(id,u::SOLDIER)!=0.{self.readers[id]|=rd::CURRENT as u32;if self.fact(id,u::FIRST_PERSON)==0.{self.readers[id]|=rd::ORIGIN as u32;}
    if self.fact(id,u::FIRST_PERSON)==0.&&!selected[id]{selected[id]=true;let n=self.counts[h::AIM] as usize;self.aim[n]=id as u32;self.counts[h::AIM]+=1;}}}
 }
 // A frame is one whole effect phase. Scratch paths and all output banks
 // retain their capacity, including when the host pauses without drawing.
 pub fn step(&mut self,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision){let dt=self.frame[f::DT];let enhanced=self.frame[f::ENHANCED]!=0.;self.counts[h::CAPSULES]=0;self.counts[h::VIEW_CAPSULES]=0;self.counts[h::VERTICES]=0;self.counts[h::INDICES]=0;self.counts[h::LIGHTS]=0;self.counts[h::TEXTURE]=u32::MAX;if enhanced||self.enhanced{self.joints.fill(0.);}
  // Classic timing advances even while Enhanced is displayed, exactly as
  // Lightning.update did before choosing its draw family.
  for k in 0..self.order.len(){let id=self.order[k];let b=&mut self.bolts[id];if b.classic{b.classic_life-=dt;if b.classic_life>0.{b.phase+=self.defs[b.def].interpolation_speed.unwrap_or(0.)*dt;while b.phase>=1.{b.phase-=1.;b.seed_a=b.seed_b;b.seed_b=seed(random);}if b.follow{b.start=vec(&self.held,id*3);}}}}
  if enhanced{self.time+=dt;self.enhanced=true;for at in 0..self.shock_order.len(){let id=self.shock_order[at];if self.shocks[id].on{self.draw_shock(id,dt,random);}}for at in 0..self.shield_order.len(){let id=self.shield_order[at];if self.shields[id].on{self.draw_shield(id,dt,random);}}
   for k in 0..self.order.len(){let id=self.order[k];let mut b=self.bolts[id];if b.life>0. && !self.draw_bolt(&mut b,dt,random,collision){b.life=0.;}
    if b.life<=0.&&(!b.classic||b.classic_life<=0.){b.active=false;}self.bolts[id]=b;}
   self.draw_sparks(dt);for at in 0..(self.frame[f::ORDER_COUNT] as usize).min(lim::UNITS){let id=self.unit_order[at] as usize;if self.fact(id,u::LISTED)==0.||self.fact(id,u::COMBAT)==0.||self.fact(id,u::REMOVED)!=0.||self.fact(id,u::DEAD)!=0.||self.fact(id,u::MOUNTED)!=0.||self.fact(id,u::SOLDIER)==0.{continue;}self.draw_charge(id,random);}
  }else{if self.enhanced{for id in 0..lim::UNITS{if self.shocks[id].on{let mut shock=self.shocks[id];self.end_shock(id,&mut shock);self.shocks[id]=shock;}self.shields[id].on=false;self.charges[id].on=false;}self.sparks.clear();for b in &mut self.bolts{b.life=0.;}self.enhanced=false;}
   for k in 0..self.order.len(){let id=self.order[k];let b=self.bolts[id];if b.classic&&b.classic_life>0.{if self.counts[h::TEXTURE]==u32::MAX{self.counts[h::TEXTURE]=b.def as u32;}self.classic(&b,random);}else{self.bolts[id].active=false;}}
  }
  self.shock_order.retain(|id|self.shocks[*id].on);self.shield_order.retain(|id|self.shields[*id].on);let mut kept=0;for k in 0..self.order.len(){let id=self.order[k];self.publish_bolt(id);if self.bolts[id].active{self.order[kept]=id;kept+=1;}}self.order.truncate(kept);self.counts[h::BOLTS]=kept as u32;self.counts[h::SPARKS]=self.sparks.len() as u32;self.counts[h::ENHANCED_ACTIVE]=self.enhanced as u32;self.retain_units();
 }
 // A host slot cannot be recycled while a native effect still names it.
 // Removed charging guns release their otherwise invisible crackle state.
 fn retain_units(&mut self){self.used.fill(0);for &id in &self.order{let b=self.bolts[id];for unit in [b.shooter,b.source,b.target,b.follow_unit]{if unit<lim::UNITS{self.used[unit]=1;}}}
  for id in 0..lim::UNITS{if self.fact(id,u::REMOVED)!=0.||self.fact(id,u::ALIVE)==0.{self.charges[id].on=false;}
   if self.shocks[id].on||self.shields[id].on||self.charges[id].on{self.used[id]=1;}}
 }
 // Clearing the look restores native-owned joint offsets before hiding it.
 // All retained allocations remain available for a later Enhanced birth.
 pub fn clear(&mut self){self.used.fill(0);self.enhanced=false;self.counts[h::ENHANCED_ACTIVE]=0;self.joints.fill(0.);for id in 0..lim::UNITS{let mut shock=self.shocks[id];if shock.on{self.end_shock(id,&mut shock);}self.shocks[id]=shock;self.shields[id].on=false;self.charges[id].on=false;}for b in &mut self.bolts{b.active=false;}self.order.clear();self.sparks.clear();self.counts[h::CAPSULES]=0;self.counts[h::VIEW_CAPSULES]=0;self.counts[h::BOLTS]=0;self.counts[h::SPARKS]=0;self.counts[h::LIGHTS]=0;self.counts[h::VERTICES]=0;self.counts[h::INDICES]=0;}
}

// Native recordings supply the same collision answers as the browser host.
// A missing answer means no hit; playback never allocates a frame tape.
pub struct RecordedCollision<'a>{pub values:&'a [f64],pub cursor:usize}
impl Collision for RecordedCollision<'_>{fn cast(&mut self,_:V,_:V,_:f64)->Option<(f64,V)>{let at=self.cursor;self.cursor=self.cursor.saturating_add(4);let a=self.values.get(at..at+4)?;if a[0]<0.{None}else{Some((a[0],V::new(a[1],a[2],a[3])))}}}

// Inspection and synthetic geometry fixtures are cold tools. They exercise the
// same algorithms without retaining a JavaScript numerical implementation.
#[derive(Deserialize)]
#[serde(tag="kind",rename_all="camelCase")]
pub enum Fixture{
 Displace{a:[f64;3],b:[f64;3],depth:usize,u:[f64;3],w:[f64;3],rough:f64,spread:f64,seed:u32,jag:u32,time:f64,writhe:f64,coarse:usize},
 Line{points:Vec<[f64;3]>,shown:f64,rgb:[f64;3],brightness:f64,core:f64,glow:f64,hot:f64,taper:f64,count:usize},
 Spray{from:[f64;3],to:[f64;3],at:[f64;3],floor:Option<f64>,surface:Option<[f64;3]>,rgb:[f64;3],count:usize,speed:f64},
 Sparks{dt:f64,count:usize},
}
fn array(v:V)->[f64;3]{[v.x,v.y,v.z]}
fn v(a:[f64;3])->V{V::new(a[0],a[1],a[2])}
impl Lightning{
 pub fn fixture(&mut self,data:Fixture,random:&mut dyn FnMut()->f64,collision:&mut dyn Collision)->String{match data{
  Fixture::Displace{a,b,depth,u,w,rough,spread,seed,jag,time,writhe,coarse}=>{let mut p=Path::default();displace(&mut p,v(a),v(b),depth,v(u),v(w),Jag{rough,spread,seed,jag,time,writhe,coarse});serde_json::to_string(&p.points[..p.count].iter().map(|p|array(*p)).collect::<Vec<_>>()).unwrap()},
  Fixture::Line{points,shown,rgb,brightness,core,glow,hot,taper,count}=>{let mut path=Path{count:points.len().min(POINTS),..Path::default()};for (to,from) in path.points.iter_mut().zip(points){*to=v(from);}self.counts[h::CAPSULES]=count.min(lim::CAPSULES) as u32;self.line(&path,shown,rgb,brightness,core,glow,hot,taper);String::new()},
  Fixture::Spray{from,to,at,floor,surface,rgb,count,speed}=>{let mut b=Bolt{from:v(from),to:v(to),floor,surface:surface.map(v),rgb,..Default::default()};self.spray(&mut b,v(at),count,speed,random,collision);self.counts[h::SPARKS]=self.sparks.len() as u32;String::new()},
  Fixture::Sparks{dt,count}=>{self.counts[h::CAPSULES]=count.min(lim::CAPSULES) as u32;self.draw_sparks(dt);self.counts[h::SPARKS]=self.sparks.len() as u32;String::new()},
 }}
 pub fn inspect(&self)->String{serde_json::json!({"bolts":self.order,"shocks":self.shocks.iter().enumerate().filter(|(_,s)|s.on).map(|(i,_)|i).collect::<Vec<_>>(),"shields":self.shields.iter().enumerate().filter(|(_,s)|s.on).map(|(i,_)|i).collect::<Vec<_>>(),"charges":self.charges.iter().enumerate().filter(|(_,s)|s.on).map(|(i,_)|i).collect::<Vec<_>>(),"sparks":self.sparks.iter().map(|s|serde_json::json!({"p":array(s.p),"v":array(s.v),"age":s.age,"life":s.life,"bounced":s.bounced})).collect::<Vec<_>>()}).to_string()}
 pub fn inject_shock(&mut self,id:usize,data:&str)->Result<(),serde_json::Error>{#[derive(Deserialize)]struct Input{generation:f64,age:f64,life:f64,strength:f64,rgb:[f64;3],seed:u32}let a:Input=serde_json::from_str(data)?;if id<lim::UNITS{self.used[id]=1;if !self.shocks[id].on{self.shock_order.retain(|i|*i!=id);self.shock_order.push(id);}self.shocks[id]=Shock{on:true,generation:a.generation,age:a.age,life:a.life,strength:a.strength,rgb:a.rgb,seed:a.seed,..Default::default()};}Ok(())}
}

#[cfg(test)]
pub(crate) mod tests {
 use super::*;
 use std::alloc::{GlobalAlloc,Layout,System};
 use std::cell::Cell;
 thread_local!{pub(crate) static TRACK:Cell<bool>=const{Cell::new(false)};pub(crate) static ALLOCS:Cell<usize>=const{Cell::new(0)};}
 struct Counting;
 unsafe impl GlobalAlloc for Counting{
  unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.alloc(l)}
  unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|c|c.set(c.get()+1));}System.realloc(p,l,n)}
  unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
 }
 #[global_allocator]static ALLOCATOR:Counting=Counting;
 fn random()->impl FnMut()->f64{let mut seed=7u32;move||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed as f64/4294967296.}}
 fn definition()->Definition{serde_json::from_str(r#"{"Width":0.5,"FadeTime":0.6,"SpreadFactor":0.8,"InterpolationSpeed":5,"BranchFactor":0.15,"BranchSpreadFactor":0.8,"BranchLength":4,"MaxBranches":3,"BreakDistance":1,"TextureSize":3.5,"ChildBreakDistance":1.5,"ChildTextureSize":2,"ChildWidth":0.4,"ChildSpreadFactor":1,"NumChildren":3,"Color":[130,170,255,250],"ChildColor":[140,180,255,100]}"#).unwrap()}
 #[test]
 fn displaced_channels_keep_authored_ends_and_order(){let mut p=Path::default();let a=V::new(0.,1.,0.);let b=V::new(0.,1.,20.);displace(&mut p,a,b,6,SIDE,UP,Jag{time:0.3,writhe:2.6,coarse:2,..jag(0.2,0.6,7,11)});assert_eq!(p.count,65);assert_eq!(p.points[0],a);assert_eq!(p.points[64],b);for pair in p.points[..p.count].windows(2){assert!(pair[1].z>=pair[0].z);assert!(hypot(pair[0].x,pair[0].y-1.)<0.6/(1.-0.64));}}
 #[test]
 fn paused_birth_bursts_and_full_draw_banks_never_allocate(){let mut owner=Lightning::default();let def=owner.definition(definition());let mut random=random();put(&mut owner.spawn,sp::START,V::new(0.,1.,0.));put(&mut owner.spawn,sp::END,V::new(0.,1.,24.));owner.spawn[sp::CHARGE]=1.;owner.spawn[sp::FOLLOW_UNIT] = -1.;owner.frame[f::ENHANCED]=1.;owner.frame[f::DT]=0.1;
  ALLOCS.with(|n|n.set(0));TRACK.with(|t|t.set(true));for _ in 0..lim::BOLTS+32{owner.add(def,true,true,&mut random,&mut EmptyCollision);}owner.step(&mut random,&mut EmptyCollision);owner.frame[f::DT]=3600.;owner.step(&mut random,&mut EmptyCollision);TRACK.with(|t|t.set(false));
  assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(owner.counts[h::BOLTS],0);assert!(owner.counts[h::DROPPED]>=32);assert!(owner.counts[h::CAPSULES] as usize<=lim::CAPSULES);assert!(owner.sparks.len()<=lim::SPARKS);
 }
 #[test]
 fn published_local_matrices_override_trs_when_automatic_updates_are_disabled(){
  let mut owner=Lightning::default();let start=owner.actor(0,Actor{parents:vec![-1],chest:Some(0),..Default::default()});
  for i in [0,5,10,15]{owner.units[u::PARENT+i]=1.;owner.nodes[start+no::MATRIX+i]=1.;}
  put(&mut owner.nodes,start+no::MATRIX+12,V::new(2.,3.,4.));owner.place(0);assert_eq!(owner.node_point(0,0),V::new(2.,3.,4.));
  owner.nodes[start+no::MATRIX_AUTO_UPDATE]=1.;put(&mut owner.nodes,start+no::POSITION,V::new(100.,100.,100.));put(&mut owner.nodes,start+no::SCALE,V::new(1.,1.,1.));owner.nodes[start+no::QUATERNION+3]=1.;owner.place(0);assert_eq!(owner.node_point(0,0),V::new(100.,100.,100.));
 }
 #[test]
 fn charging_readers_keep_registry_order_and_ignore_invalid_slots(){
  let mut owner=Lightning::default();owner.frame[f::ENHANCED]=1.;owner.frame[f::ORDER_COUNT]=f64::MAX;
  owner.unit_order.fill(u32::MAX);owner.unit_order[0]=1;owner.unit_order[1]=0;
  for id in 0..2{let o=id*u::STRIDE;owner.units[o+u::LISTED]=1.;owner.units[o+u::SOLDIER]=1.;owner.units[o+u::CHARGING]=1.;}
  owner.prepare();assert_eq!(owner.counts[h::AIM],2);assert_eq!(&owner.aim[..2],&[1,0]);
  owner.actor(lim::UNITS,Actor::default());owner.actor(0,Actor{parents:vec![-1;4096],bones:(0..4096).collect(),low:(0..4096).collect(),..Default::default()});
  assert_eq!(owner.actors[0].parents.len(),lim::NODES_PER_UNIT);assert_eq!(owner.actors[0].bones.len(),lim::NODES_PER_UNIT);
 }
 #[test]
 fn chains_wait_for_the_previous_link_and_keep_qualified_targets(){let mut owner=Lightning::default();let def=owner.definition(definition());let mut random=random();owner.frame[f::ENHANCED]=1.;owner.frame[f::DT]=0.03;owner.frame[f::UNITS]=2.;owner.frame[f::ORDER_COUNT]=2.;owner.unit_order[0]=0;owner.unit_order[1]=1;for id in 0..2{let o=id*u::STRIDE;owner.units[o+u::ALIVE]=1.;owner.units[o+u::LISTED]=1.;owner.units[o+u::SOLDIER]=1.;owner.units[o+u::GENERATION]=1.;put(&mut owner.units,o+u::ROOT,V::new(0.,0.,8.+6.*id as f64));}
  put(&mut owner.spawn,sp::START,V::new(0.,1.,0.));put(&mut owner.spawn,sp::END,V::new(0.,1.,8.));owner.spawn[sp::CHARGE]=1.;let a=owner.add(def,false,true,&mut random,&mut EmptyCollision);put(&mut owner.spawn,sp::START,V::new(0.,1.,8.));put(&mut owner.spawn,sp::END,V::new(0.,1.,14.));let b=owner.add(def,false,true,&mut random,&mut EmptyCollision);assert_eq!(owner.bolts[b].delay,owner.bolts[a].delay+0.05);assert_eq!(owner.bolts[b].source,0);owner.step(&mut random,&mut EmptyCollision);assert!(!owner.bolts[b].arrived);for _ in 0..10{owner.step(&mut random,&mut EmptyCollision);}assert!(owner.shocks[0].on&&owner.shocks[1].on);owner.units[u::GENERATION]=2.;owner.step(&mut random,&mut EmptyCollision);assert!(!owner.shocks[0].on);owner.units[u::REMOVED]=1.;assert_eq!(owner.used[0],1);owner.frame[f::DT]=3.;owner.step(&mut random,&mut EmptyCollision);assert_eq!(owner.used[0],0);
 }
}


// ArcBatch.onBeforeRender's whole camera publication runs for the target that
// is actually drawn. Upscaling can give it a different height from the window.
#[derive(Default)]
pub struct DrawCamera {pub output:[f64;super::lightning_layout::lightning_camera::STRIDE]}
impl DrawCamera {
 pub fn step(&mut self,fov:f64,zoom:f64,near:f64,height:f64){
  use super::lightning_layout::lightning_camera as c;
  let zoom=if zoom==0.||zoom.is_nan(){1.}else{zoom};
  self.output[c::PIXEL_ANGLE]=(2.*v8math::tan(fov*(std::f64::consts::PI/180.)/2.))/height.max(1.)/zoom;
  self.output[c::NEAR_DEPTH]=near*1.02;
 }
}
