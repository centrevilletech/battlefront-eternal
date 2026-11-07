//! Cold map content enters through generated rows. Navigation consumes the
//! original JSON bytes so signed zero and authored scalar forms survive;
//! no JSON parsing, name resolution or row construction runs during phases.
use super::rows::{row_buildings,row_seats,row_store,row_trap_geometry,row_traps};
use crate::{ai::nav::{Nav,PlannerOptions,math::Vec3},gen::{AiNavSetup,SpawnPost,Store},store::Strings};
use super::{state::State,population::Population};
pub struct Navigation {pub nav:Nav,posts:Vec<SpawnPost>}
impl Navigation {
    pub fn read(input:&AiNavSetup,_strings:&Strings)->Result<Self,String> {
        let pools=&input.pools;
        let pools=PlannerOptions {paths:pools.paths,path_nodes:pools.path_nodes,requests:pools.requests,search_nodes:pools.search_nodes,heap:pools.heap};
        let nav=if input.json.is_empty() {Nav::empty(pools)}else{
            let source=bfe_content::json::parse_bytes(&input.json).map_err(|error|error.to_string())?;
            // createAI gives ClassicNav the placed-post array. That array has
            // neither byName nor list, so named/nearest landing-post lookups
            // remain absent. Supplying a richer native adapter changes flight.
            Nav::new(&source,(!input.bin.is_empty()).then_some(input.bin.as_slice()),None,input.hint_capacity,pools)?
        };
        Ok(Self {nav,posts:input.posts.iter().map(|row|row.post).collect()})
    }
    /// Hint post ids belong to this cold loader, not entity/registry slots.
    /// Resolve them after F4's complete placed/mobile policy order is known.
    pub fn hint_posts(posts:&[SpawnPost],population:&Population)->Vec<Option<usize>> {
        posts.iter().map(|post|population.index(crate::battle::posts::post_ref(Some(*post)))).collect()
    }
    pub fn into_parts(self)->(Nav,Vec<SpawnPost>) {(self.nav,self.posts)}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stale {Character,Body}
impl State {
    /// F4's handles are validated before any roster entries are installed.
    /// Character order is authoritative and independent of body slab slots.
    pub fn prepare_roster(&mut self,context:super::rows::ReadContext<'_>,owner:&dyn super::roster::CharacterOwner,handles:&[crate::gen::F4Handle])->Result<(),Stale> {
        for handle in handles {owner.resolve(context,handle).ok_or(Stale::Character)?;}
        for handle in handles {
            let reference=owner.resolve(context,handle).unwrap();
            let index=self.roster.add(handle.clone());
            self.entities.register(super::entities::Entity::Character(reference));self.units.character(reference,index);
        }
        Ok(())
    }
    /// UnitFactory realizes late network bodies before Character.unit is
    /// published. Admission belongs at that owner boundary, not first aim.
    pub fn admit_body(&mut self, store:&Store, body:crate::store::Ref<crate::gen::Unit>)->bool {
        store.unit.contains(body) && self.entities.try_register(super::entities::Entity::Unit(body)).is_some()
    }
    /// Prepared removed bodies already have their native object identity.
    /// A later F3 activation changes live registry order without recreating it.
    pub fn prepare_bodies(&mut self,store:&Store,bodies:&[crate::store::Ref<crate::gen::Unit>])->Result<(),Stale> {
        for &body in bodies {store.unit.get(body).ok_or(Stale::Body)?;}
        for &body in bodies {self.entities.register(super::entities::Entity::Unit(body));}
        Ok(())
    }
}

/// Team factions stay with F4's native spawning rows. The caller supplies
/// the native registry's final relation rule, after authored alliances have
/// been applied; copying that fixed table performs no gameplay observation.
pub fn teams(store:&Store,relation:impl Fn(f64,f64)->f64)->crate::ai::control::world::TeamTable {
    let count=store.spawning.teams.len();
    crate::ai::control::world::TeamTable {
        relations:(0..count).map(|a|Some((0..count).map(|b|relation(a as f64,b as f64)).collect())).collect(),
        factions:store.spawning.teams.iter().map(|team|team.faction).collect(),alliances:Vec::new(),
    }
}

/// ai.json contains only JSON values. Missing properties remain Undefined
/// when a definition reads them; explicit null stays a distinct value.
fn js(value:&bfe_content::Value)->crate::combat::defs::Js {
    use crate::combat::defs::{Js,JsObj};use bfe_content::Value;use std::rc::Rc;
    match value {
        Value::Null=>Js::Null,Value::Bool(v)=>Js::Bool(*v),Value::Number(v)=>Js::Num(*v),Value::String(v)=>Js::Str(v.clone()),
        Value::Array(values)=>Js::Arr(Rc::new(values.iter().map(js).collect())),
        Value::Object(object)=>Js::Obj(Rc::new(JsObj {fields:object.entries().map(|(name,value)|(Rc::from(name),js(value))).collect(),def:None})),
    }
}
/// registerAIData visits side files and each file's weapon names in order.
/// Levels are keyed by the authored `at`, rather than their array position.
pub fn weapon_records(bytes:&[u8])->Result<Vec<(String,crate::ai::combat::weapons_ai::Registered)>,String> {
    use crate::combat::defs::Js;
    let input=bfe_content::json::parse_bytes(bytes).map_err(|error|error.to_string())?;
    let Some(bfe_content::Value::Object(weapons))=input.get("weapons") else{return Ok(Vec::new())};
    let mut out=Vec::new();
    for (name,record) in weapons.entries() {
        let Some(ai)=record.get("ai").filter(|value|value.truthy()) else{continue};
        let levels=record.get("levels").and_then(|value|value.as_array()).unwrap_or(&[]).iter().map(|level|(
            level.get("at").map(js).unwrap_or(Js::Undefined),level.get("ai").map(js).unwrap_or(Js::Undefined))).collect();
        out.push((name.to_string(),crate::ai::combat::weapons_ai::Registered {ai:js(ai),levels}));
    }
    Ok(out)
}

impl super::posts::Definition {
    /// postAI/captureRegion preserve authored fallback geometry separately
    /// from F4's moving capture region. This is cold content, never a tick copy.
    pub fn read(row:&crate::gen::AiPostDefinition,strings:&Strings)->Self {
        use crate::ai::control::world::{PostAi,CaptureSnap,RawCapture};
        use std::rc::Rc;
        Self {name:row.name.map(|id|Rc::from(String::from_utf16_lossy(strings.units(id)))),
            ai:PostAi {order:row.order,atk:row.policy.attack,def:row.policy.defend,bans:row.policy.bans,value_bleed:row.value_bleed,
                spawn_present:row.spawn_present,ally_present:row.ally_present,ally_count:row.ally_count,class_spawn_points:row.policy.class_spawn_points,
                capture:row.capture.as_ref().map(|capture|CaptureSnap {center:Vec3::new(capture.center[0],capture.center[1],capture.center[2]),half:capture.half}),radius:row.radius,yaw:row.yaw},
            capture:row.raw_capture.as_ref().map(|capture|Rc::new(RawCapture {center:capture.center,matrix:capture.matrix.clone(),size:capture.size,half:capture.half})),
            spawn_path:row.spawn_path,spawn_points:row.spawn_points,ally_path:row.ally_path}
    }
}

/// Cold registry bindings retain policies and qualified owner references.
/// The full batch is checked before installation; runtime reads never import
/// another hull, building or seat snapshot from JavaScript.
impl<C:super::weapons::Changes+?Sized,M:super::arsenals::Mounted> super::rows::Rows<'_,C,M> {
    pub fn prepare_members(&mut self,hulls:&[crate::gen::AiHullDefinition],props:&[crate::gen::AiPropDefinition],)->bool {
        use crate::gen::{Member,RepairTarget};
        for row in hulls {
            let Some(hull)=row_store!(self).vehicle.get(row.hull) else{return false};
            if row.member.is_some_and(|member|!matches!(member,Member::VehicleTarget(_))) || hull.seats.len()!=row.seats.len() {return false;}
        }
        for row in props {
            if row.owner.kind==crate::gen::F4ObjectKind::Trap {
                if !matches!(row.target,RepairTarget::Building(_))||row.member.is_some()
                    ||row_traps!(self).and_then(|rows|rows.get(&row.owner)).is_none()
                    ||row_trap_geometry!(self,row.target).is_none() {return false;}
                continue;
            }
            let Some(building)=row_buildings!(self).get(&row.owner) else{return false};
            match row.target {
                RepairTarget::Vehicle(_)=>return false,
                RepairTarget::Building(_)=>{
                    if building.settings.kind==crate::battle::props::Kind::Turret || row.member.is_some_and(|member|!matches!(member,Member::ObjectiveTarget(_))) {return false;}
                },
                RepairTarget::Turret(reference)=>{
                    if !row_seats!(self).crews.turrets.iter().any(|turret|turret.turret==reference) || building.settings.kind!=crate::battle::props::Kind::Turret || row.member.is_some_and(|member|!matches!(member,Member::TurretTarget(_))) {return false;}
                },
            }
            if row.post.is_some_and(|post|self.state.population.index(crate::battle::posts::post_ref(Some(post))).is_none()) {return false;}
        }
        for row in hulls {

            let strings=&self.w.sim.strings;
            let class=super::content::UnitClass::read(&row.policy,strings);
            self.state.bindings.vehicle(row.hull,super::content::traversal_size(row.traversal_size,strings) as f64);
            let seats=row.seats.iter().map(|seat|crate::ai::vehicles::world::SeatView {
                weapons:seat.weapons as usize,yaw_limits:seat.yaw.map(|id|String::from_utf16_lossy(strings.units(id)).into()),
                pitch_limits:seat.pitch.map(|id|String::from_utf16_lossy(strings.units(id)).into()),..Default::default()
            }).collect();
            self.state.mounts.register(row_store!(self),&mut self.state.entities,row.hull,super::mounts::HullClass {
                name:class.name,label:class.label,max_speed:row.max_speed,forward_speed:row.forward_speed,
                slot_post:row.slot_post,decay_time:row.decay_time,seats,
            });
            let entry=self.state.mounts.stage_target(&self.state.entities,row.hull,super::mounts::Target {
                member:row.member.unwrap_or_default(),kind:row.kind,category:row.category,policy:std::rc::Rc::new(class.policy),combat:class.combat,
            },row.member);
            self.state.mounts.target_vehicle_class(row.hull,class.vehicle);
            if let Some((member,entry))=row.member.zip(entry) {self.state.queries.target(member,entry);}
            let classes=row.seats.iter().map(|seat|self.state.control_class(&seat.policy,strings,seat.kind)).collect();
            self.state.prepare_hull_controls(row.hull,classes);
        }
        for row in props {
            let geometry=if row.owner.kind==crate::gen::F4ObjectKind::Trap {row_trap_geometry!(self,row.target)}else{None};

            let strings=&self.w.sim.strings;
            let class=super::content::UnitClass::read(&row.policy,strings);
            let target_class=row.target_policy.as_ref().map(|policy|super::content::UnitClass::read(policy,strings));
            let policy=std::rc::Rc::new(class.policy);
            // Post weighting names ordinary buildings. Trap repair wrappers
            // use their independent canonical trap row, never a post binding.
            if row.owner.kind!=crate::gen::F4ObjectKind::Trap {
                if let RepairTarget::Building(reference)=row.target {self.state.bindings.building(reference,row.owner.clone());}
            }
            let post=row.post.map(|post|self.state.entities.register(super::objects::post_entity(post)));
            let definition=super::buildings::Definition {name:Some(class.name.clone()),policy:policy.clone(),combat:class.combat.clone(),team:row.team,armed:row.armed,post};
            let wrapper=if row.owner.kind==crate::gen::F4ObjectKind::Trap {
                self.state.buildings.register_trap(row_traps!(self).unwrap(),&mut self.state.entities,row.target,row.owner.clone(),definition,
                    geometry.unwrap())
            }else {self.state.buildings.register(row_buildings!(self),&mut self.state.entities,row.target,row.owner.clone(),definition)};
            // createServiceCandidates includes wrecks and unbuilt stations,
            // which the living target registry deliberately omits. Keep the
            // authored building order and the command-post repair exclusion.
            self.state.candidates.building(row.target,wrapper,row.command_post);
            match row.target {
                RepairTarget::Turret(turret)=>{
                    self.state.turrets.register(row_seats!(self),row_buildings!(self),&mut self.state.entities,turret,row.owner.clone(),
                        row.yaw.map(|id|String::from_utf16_lossy(strings.units(id)).into()),row.pitch.map(|id|String::from_utf16_lossy(strings.units(id)).into()));
                    if let Some(member)=row.member {
                        // TurretTarget.classDef is the full descriptor; its
                        // repair wrapper reads descriptor.settings instead.
                        let (name,policy,combat)=match target_class {
                            Some(target)=>(target.name,std::rc::Rc::new(target.policy),target.combat),
                            None=>(class.name,policy,class.combat),
                        };
                        let entry=self.state.turrets.target(turret,super::turrets::Target {member,team:row.team,name,policy,combat}).unwrap();
                        self.state.queries.target(member,entry);
                    }
                },
                RepairTarget::Building(_)=>if let Some(member)=row.member {
                    let entry=self.state.buildings.objective(&mut self.state.entities,row.target,member).unwrap();self.state.queries.target(member,entry);
                },
                RepairTarget::Vehicle(_)=>unreachable!(),
            }
        }

        let strings=&self.w.sim.strings;
        // Following a live repair target accepts hull and building wrappers,
        // not only infantry. These passive identities own no movement helper.
        for row in hulls {
            let obj=self.state.entities.find(super::entities::Entity::HullEntry(row.hull)).unwrap();
            let policy=self.scene().policy(obj).unwrap_or_else(||crate::ai::control::world::AiUnit {
                key:obj,kind:row.kind,generation:row_store!(self).vehicle.get(row.hull).unwrap().target_generation,
                class_def:Some(std::rc::Rc::new(super::content::UnitClass::read(&row.policy,strings).policy)),..Default::default()
            });
            self.state.prepare_follow_target(obj,super::navigation::FollowSource::Hull(row.hull),&policy);
        }
        for row in props {
            use super::entities::Entity;
            let (wrapper,entry)=match row.target {
                RepairTarget::Building(r)=>(Entity::BuildingService(r),Entity::ObjectiveEntry(r)),
                RepairTarget::Turret(r)=>(Entity::TurretService(r),Entity::TurretEntry(r)),
                RepairTarget::Vehicle(_)=>unreachable!(),
            };
            for entity in [wrapper,entry] {
                if let Some(obj)=self.state.entities.find(entity) {
                    if let Some(policy)=self.scene().policy(obj) {
                        let source=if row.owner.kind==crate::gen::F4ObjectKind::Trap {super::navigation::FollowSource::Trap(row.owner.clone(),policy.position)}
                            else {super::navigation::FollowSource::Building(row.owner.clone())};
                        self.state.prepare_follow_target(obj,source,&policy);
                    }
                }
            }
        }
        true
    }
}
