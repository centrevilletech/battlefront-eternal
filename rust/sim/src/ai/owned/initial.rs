//! createAI constructs the roster before attaching Classic controllers.
//! Spawning.begin and initial objectives share A at each original observation;
//! the later owner activation adopts those Characters without another draw.
use crate::{ai::{control::world::TeamTable,phase::RandomState},battle::{names::NameLists,spawning::{self,Naming,HumanSpec},world::{PostsWorld,SpawningWorld},mobile::Pad},
    gen::{AiClassScalar,AiRosterSetup,Building,Character,MobilePost,PostRef,SpawnPost,Store,Unit,Vehicle},store::{Ref,StrId,Strings}};
use super::population::{Population,Facts};

/// Cold mission parsing is shared with the page's team metadata reader.
/// Mixed argument positions and explicit extraction overrides remain intact.
pub fn calls(input:&[crate::gen::AiRosterCall],strings:&Strings)->Vec<spawning::Call> {
    let text=|id|String::from_utf16_lossy(strings.units(id));
    input.iter().map(|call|spawning::Call {
        name:text(call.name),args:call.args.iter().map(|arg|match arg {
            AiClassScalar::Null=>spawning::Arg::Null,
            AiClassScalar::Bool(value)=>spawning::Arg::Bool(*value),
            AiClassScalar::F64(value)=>spawning::Arg::Num(*value),
            AiClassScalar::Str(id)=>spawning::Arg::Str(text(*id)),_=>spawning::Arg::Other,
        }).collect(),faction:call.faction,mask:call.mask,
    }).collect()
}

pub struct Program {pub spawning:crate::gen::Spawning,pub naming:Naming,pub humans:Vec<HumanSpec>}
impl Program {
    /// Prepared names are an availability set from cold factory loading,
    /// never a host callback while the native mission applies its script.
    pub fn read(input:&AiRosterSetup,strings:&mut Strings)->Self {
        let text=|id|String::from_utf16_lossy(strings.units(id));
        let calls=calls(&input.calls,strings);
        let names=NameLists {sides:input.names.iter().map(|side|(side.first.iter().map(|id|text(*id)).collect(),side.last.iter().map(|id|text(*id)).collect())).collect(),local:input.local.map(text)};
        let prepared:Vec<_>=input.prepared.iter().map(|id|text(*id)).collect();
        let humans=input.humans.iter().map(|human|HumanSpec {team:human.team,id:human.id,name:human.name.map(text)}).collect();
        let (spawning,naming)=spawning::new_spawning(&calls,input.unit_scale,input.heroes,input.demo,Some(names),input.capacity,
            &|name|prepared.iter().any(|prepared|prepared==name),strings);
        Self {spawning,naming,humans}
    }
}
struct World<'a,W:SpawningWorld+?Sized> {
    source:&'a mut W,population:&'a mut Population,random:RandomState,teams:&'a TeamTable,facts:&'a dyn Facts,enhanced:bool,
}
impl<W:SpawningWorld+?Sized> PostsWorld for World<'_,W> {
    fn capture_team(&mut self,team:f64)->f64 {self.source.capture_team(team)}
    fn building_alive(&mut self,building:Ref<Building>)->bool {self.source.building_alive(building)}
    fn pad(&mut self,s:&Store,slot:f64)->Pad {self.source.pad(s,slot)}
    fn with_pad(&mut self,s:&mut Store,slot:f64,visit:&mut dyn FnMut(&mut Store,&Pad)) {self.source.with_pad(s,slot,visit)}
    fn vehicle_position(&mut self,s:&Store,vehicle:Ref<Vehicle>)->[f64;3] {self.source.vehicle_position(s,vehicle)}
    fn class_spawn_world(&mut self,s:&Store,post:Ref<MobilePost>)->Vec<[f64;4]> {self.source.class_spawn_world(s,post)}
    fn class_spawn_world_into(&mut self,s:&Store,post:Ref<MobilePost>,into:&mut Vec<[f64;4]>) {self.source.class_spawn_world_into(s,post,into)}
}
impl<W:SpawningWorld+?Sized> SpawningWorld for World<'_,W> {
    fn spawn_goal(&mut self,s:&Store,character:Ref<Character>)->Option<[f64;3]> {
        if self.enhanced {spawning::objective_position(s,character,self)}else{None}
    }
    fn health(&mut self,s:&Store,unit:Ref<Unit>)->f64 {self.source.health(s,unit)}
    fn force_kill(&mut self,s:&mut Store,character:Ref<Character>,unit:Ref<Unit>) {self.source.force_kill(s,character,unit)}
    fn request_objective(&mut self,s:&mut Store,character:Ref<Character>) {
        // Initial Characters have no body or crew and no attached controller.
        // Select on the real population object so Enhanced caches survive.
        let post=self.population.select(s,character,&mut self.random,self.teams,self.facts,0.,None,false,PostRef::Undefined);
        s.character.get_mut(character).unwrap().objective=crate::battle::posts::post_ref(post);
    }
    fn republish(&mut self,_s:&mut Store,_character:Ref<Character>) {}
    fn hero_name(&mut self,class:StrId)->Option<StrId> {self.source.hero_name(class)}
    fn spawn_unit(&mut self,s:&mut Store,character:Ref<Character>,class:Option<StrId>,post:SpawnPost,at:[f64;3],yaw:Option<f64>)->Option<Ref<Unit>> {
        self.source.spawn_unit(s,character,class,post,at,yaw)
    }
}
/// Source begin is cold: all Characters are inserted before setupTeam starts.
/// The wrapper changes only objective dispatch; native F4 still owns queues,
/// class selection, mobile post refresh and hero display-name observations.
pub fn begin<W:SpawningWorld+?Sized>(s:&mut Store,program:&mut Program,population:&mut Population,random:&mut RandomState,
    teams:&TeamTable,facts:&dyn Facts,enhanced:bool,strings:&mut Strings,source:&mut W) {
    s.spawning=std::mem::take(&mut program.spawning);
    let mut world=World {source,population,random:random.share(),teams,facts,enhanced};
    spawning::begin(s,&program.humans,&mut program.naming,strings,&mut world,random);
}

/// Initial setup needs canonical fleet/post geometry before the later F4
/// phase host exists. This cold reader never creates or kills a body.

pub struct Native<'a> {collision: &'a mut crate::collision::CollisionOwner, movement: &'a mut crate::movement::MovementOwner, fleet: &'a crate::vehicles::module::Fleet, world: usize, pads:crate::battle::fleet_posts::Pads,names:&'a [crate::gen::F4HeroSetup],facts:&'a dyn Facts}

impl<'a> Native<'a> {
    pub fn new(s:&mut Store,input:&'a AiRosterSetup,facts:&'a dyn Facts, collision: &'a mut crate::collision::CollisionOwner, movement: &'a mut crate::movement::MovementOwner, fleet: &'a crate::vehicles::module::Fleet, world: usize)->Self {
        let mut pads=crate::battle::fleet_posts::Pads::default();
        for pad in &input.pads {
            pads.adopt(pad.index as usize,pad.pad.clone());
            for &reference in &s.mobile_posts.posts {
                let post=s.mobile_post.get_mut(reference).unwrap();
                if post.index==pad.index as f64 {pads.reserve_post(pad.index as usize,post);}
            }
        }
        Self {collision,movement,fleet,world,pads,names:&input.hero_names,facts}
    }
}

impl PostsWorld for Native<'_> {
    fn capture_team(&mut self,team:f64)->f64 {team}
    fn building_alive(&mut self,building:Ref<Building>)->bool {self.facts.building_alive(building)}
    fn pad(&mut self,s:&Store,slot:f64)->Pad {self.pads.get(s,slot as usize).clone()}
    fn with_pad(&mut self,s:&mut Store,slot:f64,visit:&mut dyn FnMut(&mut Store,&Pad)) {visit(s,self.pads.get(s,slot as usize));}
    fn vehicle_position(&mut self,s:&Store,vehicle:Ref<Vehicle>)->[f64;3] {s.vehicle.get(vehicle).unwrap().position}
    fn class_spawn_world(&mut self,s:&Store,post:Ref<MobilePost>)->Vec<[f64;4]> {
        let mut points=Vec::new();self.class_spawn_world_into(s,post,&mut points);points
    }
    fn class_spawn_world_into(&mut self,s:&Store,post:Ref<MobilePost>,into:&mut Vec<[f64;4]>) {
        crate::battle::fleet_posts::spawn_points(s,post,into,|from,direction,span,body| {
            let array=|point:three::Vector3|[point.x,point.y,point.z];
            crate::units::native::ordnance_cast_parts_in(self.collision,self.movement,self.fleet,self.world,array(from),array(direction),span,Some(body),true,false)
                .map(|hit|three::Vector3::new(hit.point[0],hit.point[1],hit.point[2]))
        });
    }
}

impl SpawningWorld for Native<'_> {
    fn health(&mut self,s:&Store,unit:Ref<Unit>)->f64 {s.unit.get(unit).map_or(0.,|unit|unit.combat.health)}
    fn force_kill(&mut self,_s:&mut Store,_character:Ref<Character>,_unit:Ref<Unit>) {unreachable!("initial roster has no bodies")}
    fn request_objective(&mut self,_s:&mut Store,_character:Ref<Character>) {unreachable!("initial objectives use the population owner")}
    fn republish(&mut self,_s:&mut Store,_character:Ref<Character>) {}
    fn hero_name(&mut self,class:StrId)->Option<StrId> {self.names.iter().find(|name|name.class_name==class).and_then(|name|name.name)}
    fn spawn_unit(&mut self,_s:&mut Store,_character:Ref<Character>,_class:Option<StrId>,_post:SpawnPost,_at:[f64;3],_yaw:Option<f64>)->Option<Ref<Unit>> {
        unreachable!("initial roster prepares queues before the first spawn wave")
    }
}
