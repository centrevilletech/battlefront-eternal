//! Controller post observations from F4's placed/mobile rows. Mission geometry
//! and names are prepared once; moving positions, class weights and lifecycle
//! flags are read from their canonical owners at each observation.
use std::rc::Rc;
use crate::gen::{AiPostPolicy,Store,SpawnPost,RegionType};
use crate::ai::{control::world::{PostSnap,PostAi,RawCapture,RegionSnap,RegionKind},nav::math::Vec3};
use super::population::{Population,Facts,Stale};

#[derive(Default)]
pub struct Definition {
    pub name:Option<Rc<str>>,
    pub ai:PostAi,
    pub capture:Option<Rc<RawCapture>>,
    pub spawn_path:bool,pub spawn_points:f64,pub ally_path:bool,
}
pub struct Posts {definitions:Vec<Definition>,names:Vec<u32>}
impl Posts {
    pub fn new(capacity:usize)->Self {Self {definitions:Vec::with_capacity(capacity),names:Vec::with_capacity(capacity)}}
    pub fn register(&mut self,store:&Store,population:&mut Population,policy:AiPostPolicy,definition:Definition)->Result<usize,Stale> {
        let index=population.add(store,policy)?;
        assert_eq!(index,self.definitions.len(),"post policy and authored definition order");
        let name=definition.name.as_deref().unwrap_or("undefined");
        let first=self.definitions.iter().position(|d|d.name.as_deref().unwrap_or("undefined")==name).unwrap_or(index);
        self.names.push(first as u32);self.definitions.push(definition);Ok(index)
    }
    pub fn name_id(&self,index:usize)->u32 {self.names[index]}
    pub fn read(&self,store:&Store,population:&Population,index:usize,facts:&dyn Facts)->Option<PostSnap> {
        let definition=self.definitions.get(index)?;
        let target=population.post(index)?;
        let view=population.view(store,index,facts)?;
        let mut ai=definition.ai.clone();
        for i in 0..6 {ai.atk[i]=view.attack.get(i).copied().unwrap_or(0.);ai.def[i]=view.defend.get(i).copied().unwrap_or(0.);}
        ai.bans=view.bans;ai.class_spawn_points=view.class_spawn_points;
        let (neutralize,radius,speed,region)=match target {
            SpawnPost::Post(reference)=>{
                let post=store.post.get(reference)?;
                if let Some(native)=&post.ai {
                    ai.spawn_present=native.spawn_path.as_ref().map(|p|p.present);
                    ai.ally_present=native.ally_path.as_ref().map(|p|p.present);
                    ai.ally_count=native.ally_count;ai.yaw=native.yaw;
                }
                (post.neutralize,post.radius.unwrap_or(0.),0.,post.capture.as_ref().map(|region|RegionSnap {
                    kind:match region.r#type {RegionType::Sphere=>RegionKind::Sphere,RegionType::Box=>RegionKind::Box,RegionType::Cylinder=>RegionKind::Cylinder},
                    half:region.half,radius:region.radius,center:v3(region.center),inverse:region.inverse}))
            },
            SpawnPost::MobilePost(reference)=>{
                let post=store.mobile_post.get(reference)?;
                ai.value_bleed=post.ai.value_bleed;ai.yaw=post.yaw;
                (0.,post.radius,post.speed,None)
            },
        };
        Some(PostSnap {index,name:definition.name.clone(),team:view.team,active:view.active,position:view.position,
            building_alive:view.building_alive,neutralize,radius,speed,ai,region,
            desc_capture:definition.capture.clone(),desc_spawn_path:definition.spawn_path,
            desc_spawn_points:definition.spawn_points,desc_ally_path:definition.ally_path})
    }
}
fn v3(p:[f64;3])->Vec3 {Vec3::new(p[0],p[1],p[2])}
