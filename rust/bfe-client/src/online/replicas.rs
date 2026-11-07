//! Native resource admission and the remote-body display batch. Snapshot
//! samples remain in the shared Rust ring; no scalar facade sits between the
//! receiver, animation phase and renderer's existing instance/bone banks.
use crate::{render_bridge::Bridge,online::receiver::Receiver};
use bfe_content_native::{BattleContent,loader::{Material,MaterialKind,Blend}};
use bfe_presentation::{units::Units,layout::{unit_frame as f,unit_input as i,unit_output as o,unit_command as command,unit_command_kind as kind,unit_limits as lim,VERSION},network_layout::{net_record as r,net_kind as k}};
use bfe_render::instances as draw;
use bfe_transport::layout::INPUT_CODES;
use three::Matrix4;
use super::index::Index;
const POOL:usize=lim::ACTORS;
struct Model {row:[f64;draw::REC_STRIDE],inverses:Vec<[f64;16]>,content:usize}
struct Slot {actor:usize,model:usize,identity:Option<(u32,u32)>,active:bool}
pub struct Replicas {
    pub descriptor:serde_json::Value,pub turns:Vec<(f64,[f64;2])>,pub units:Units,
    models:Vec<Model>,classes:Vec<Option<usize>>,slots:Vec<Slot>,bound:Index,
    pub visible:usize,pub refused:u64,frame:u64,
}
impl Replicas {
    pub fn new(content:&mut BattleContent,bridge:&mut Bridge,textures:&[u32])->Result<Self,String>{
        // entityRegistryDescriptor derives ordered names from the same native
        // definitions as the server. Numeric handles resolve before frames.
        let defs=bfe_content::defs::Defs::build(&content.content)?;let registry=&defs.registry;
        let table=|names:&[String]|names.iter().enumerate().map(|(i,n)|serde_json::json!([i+1,n])).collect::<Vec<_>>();
        let descriptor=serde_json::json!({"contentEpoch":1,"combatDefinitions":registry.combat.iter().map(|(p,_)|p).collect::<Vec<_>>(),"classes":table(&registry.classes),"weapons":table(&registry.weapons),"posts":table(&registry.posts),"inputCodes":&INPUT_CODES[..],"bindingProfiles":[[0,{}]]});
        let mut units=Units::new();let mut models=Vec::new();let mut names=Vec::new();
        // Actor geometry and programs are immutable resources. Model variants
        // share them across every prepared skeleton in the retained pool.
        let material=Material{kind:MaterialKind::Dynamic,blend:Blend::Opaque,cutout:false,double_sided:false,flags:1,far:false,texture:None,bump:None,record:serde_json::Value::Null};
        let key=crate::static_scene::actor_key(&material)?;let program=bridge.add_owned_program(&key)?;
        let inputs=std::mem::take(&mut content.units);
        for (name,input)in inputs{
            let body_key=defs.classes.iter().find_map(|c|{
                let body=c.body?;let source=&content.content.actors[body].0;
                (name==*source||name==format!("{}/{}",defs.names.text(c.side),defs.names.text(c.name))).then_some(source)
            });
            let Some(body_key)=body_key else{continue;};
            let Some(mesh)=content.actors.iter().find(|m|m.name==*body_key)else{continue;};
            let Some((_,actor))=content.content.actors.iter().find(|(n,_)|n==body_key)else{continue;};
            if input.bones.len()!=actor.bones.len()||input.bones.is_empty()||!input.bones.iter().any(|b|b.name.eq_ignore_ascii_case("dummyroot")){continue;}
            if mesh.textures.len()>4{return Err("actor material groups exceed native instance layout".into());}
            // The renderer uses one sampler per group. Merge authored parts
            // by material while retaining each material's triangle order.
            let mut grouped=mesh.geometry.clone();grouped.indices.clear();grouped.groups.clear();
            for material in 0..mesh.textures.len(){let first=grouped.indices.len();for group in &mesh.geometry.groups{
                if group.material==material{grouped.indices.extend_from_slice(&mesh.geometry.indices[group.first..group.first+group.count]);}
            }grouped.groups.push(bfe_content_native::geometry::Group{first,count:grouped.indices.len()-first,material});}
            let geometry=grouped.register(&mut bridge.r,&key)?;let mut row=[0.;draw::REC_STRIDE];
            row[draw::REC_KIND]=1.;row[draw::REC_PROGRAM]=program as f64;row[draw::REC_GEOMETRY]=geometry as f64;row[draw::REC_GROUPS]=grouped.groups.len()as f64;
            row[draw::REC_TEXTURES..draw::REC_TEXTURES+4].fill(-1.);
            for (i,texture)in mesh.textures.iter().enumerate(){if let Some(texture)=texture{row[draw::REC_TEXTURES+i]=textures[*texture]as f64;}}
            row[draw::REC_BIND..draw::REC_BIND+16].copy_from_slice(&actor.bind_matrix.elements);
            row[draw::REC_SPHERE+1]=0.9;row[draw::REC_SPHERE+3]=2.2;row[draw::REC_FLAGS]=8.;row[draw::REC_LOOK]=1.;row[draw::REC_LOOK+3]=1.;
            let inverses=actor.bones.iter().map(|b|b.inverse.elements).collect();let id=units.load(input);names.push(name);models.push(Model{row,inverses,content:id});
        }
        // Soldier look limits are authored class constants, not a second
        // controller. Class registry ids are sparse and cannot index models.
        let mut turns=vec![(2.,[-0.8,1.1]);registry.classes.len()+1];let mut classes=vec![None;registry.classes.len()+1];
        for class in &defs.classes{
            let Some(id)=class.registry_id else{continue;};let body_constants=bfe_sim::units::class::soldier_body(&class.record,1e6,class.getup_time,class.dive_time);turns[id as usize]=(body_constants.max_turn,body_constants.pitch_limits);let Some(body)=class.body else{continue;};
            let key=if class.walker_clips.is_some(){format!("{}/{}",defs.names.text(class.side),defs.names.text(class.name))}else{content.content.actors[body].0.clone()};
            classes[id as usize]=names.iter().position(|n|n==&key);

        }
        // Units owns the generated actor/node limits. Stop cold registration
        // before either budget fills; a live refusal never creates a rig.
        let mut slots=Vec::with_capacity(POOL);let mut nodes=0;let per_model=POOL.checked_div(models.len()).unwrap_or(0);
        // Resource creation is cold. A bounded pool lends existing skeletons
        // to later lives; packet bursts and class changes cannot allocate rigs.
        for (model,data)in models.iter().enumerate(){for _ in 0..per_model{
            let count=data.inverses.len()+3;if nodes+count>lim::NODES||slots.len()==POOL{break;}
            nodes+=count;let actor=units.actor(data.content,&data.inverses);slots.push(Slot{actor,model,identity:None,active:false});
        }}
        Ok(Self{descriptor,turns,units,models,classes,slots,bound:Index::new(POOL),visible:0,refused:0,frame:0})
    }
    pub fn frame(&mut self,receiver:&Receiver,tick:f64,dt:f64,eye:[f64;3]){
        // SnapshotPresentation samples once for the whole display batch.
        // The unit links then borrow its output by identity and generation.
        receiver.network.borrow_mut().present(tick,-1.,-1.);self.frame=self.frame.wrapping_add(1);self.visible=0;self.units.input[f::COMMANDS]=0.;
        // Mark survivors first, then reclaim vanished identities. A new birth
        // cannot steal a resource whose current record occurs later in a packet.
        for slot in &mut self.slots{slot.active=false;}
        for row in receiver.active(){let key=(row[r::ID]as u32,row[r::GENERATION]as u32);if let Some(&slot)=self.bound.get(&key){if self.classes.get(row[r::CLASS_ID]as usize).copied().flatten()==Some(self.slots[slot].model){self.slots[slot].active=true;}}}
        for (at,slot)in self.slots.iter_mut().enumerate(){if !slot.active{if let Some(key)=slot.identity.take(){if self.bound.get(&key)==Some(&at){self.bound.remove(&key);}self.units.network.unbind(slot.actor);}}}
        for row in receiver.active(){
            let kind=row[r::KIND]as u32;if kind!=k::SOLDIER&&kind!=k::DROIDEKA{continue;}
            let Some(model)=self.classes.get(row[r::CLASS_ID]as usize).copied().flatten()else{continue;};let key=(row[r::ID]as u32,row[r::GENERATION]as u32);
            // A class change needs a matching prepared skeleton. Reusing a
            // body slot must reset animation even if generations coincide.
            let slot=if let Some(&slot)=self.bound.get(&key){slot}else{
                let Some(slot)=self.slots.iter().position(|s|s.model==model&&s.identity.is_none())else{self.refused+=1;continue;};
                self.slots[slot].identity=Some(key);self.slots[slot].active=true;self.bound.insert(key,slot);
                let actor=self.slots[slot].actor;self.units.reset_binding(actor);
                for operation in [kind::RESET,kind::PRESENTATION_RESET]{let at=self.units.input[f::COMMANDS]as usize*command::STRIDE;let c=&mut self.units.commands[at..at+command::STRIDE];c.fill(0.);c[command::ACTOR]=actor as f64;c[command::KIND]=operation as f64;c[command::YAW]=row[r::YAW];self.units.input[f::COMMANDS]+=1.;}
                self.units.network.bind(actor,key.0 as f64,key.1 as f64,if kind==k::SOLDIER{0.}else{2.},&receiver.network);slot
            };
            let actor=self.slots[slot].actor;let input=&mut self.units.input[f::STRIDE+actor*i::STRIDE..][..i::STRIDE];
            input[i::ID]=row[r::ID];input[i::GENERATION]=row[r::GENERATION];input[i::PRESENT]=if row[r::ID]==receiver.own.id{0.}else{1.};
            self.visible+=usize::from(input[i::PRESENT]!=0.);
        }
        // Network-linked rows receive motion from Rust. The retained parent
        // matrices describe placement only and do not reapply network motion.
        let identity=Matrix4::default().elements;
        for slot in &self.slots{
            let input=&mut self.units.input[f::STRIDE+slot.actor*i::STRIDE..][..i::STRIDE];
            if !slot.active{input[i::PRESENT]=0.;}
            input[i::ACTOR]=slot.actor as f64;input[i::ALPHA]=1.;input[i::HELD_CLIP]=-1.;input[i::DIVE_TIME]=0.6;input[i::ROOT_QUATERNION+3]=1.;
            input[i::POSE_PARENT..i::POSE_PARENT+16].copy_from_slice(&identity);input[i::ATTACHMENT_PARENT..i::ATTACHMENT_PARENT+16].copy_from_slice(&identity);
        }
        self.units.input[f::VERSION]=VERSION as f64;self.units.input[f::FRAME]=self.frame as f64;self.units.input[f::DT]=dt;self.units.input[f::ACTORS]=self.slots.len()as f64;self.units.input[f::FIRST_PERSON]=-1.;self.units.input[f::CAMERA_POSITION..f::CAMERA_POSITION+3].copy_from_slice(&eye);
        // Pose and placement publish skin once into the existing owner banks.
        // Renderer submission below borrows that storage synchronously.
        self.units.frame();
    }
    pub fn append(&self,bridge:&mut Bridge,mut records:usize,mut bones:usize)->(usize,usize){
        // Drawing clips at the renderer's existing record and bone budgets.
        // Filling either leaves gameplay, transport and the next frame alive.
        let camera=bridge.r.core.camera();
        for slot in &self.slots{
            if !slot.active{continue;}let input=&self.units.input[f::STRIDE+slot.actor*i::STRIDE..];if input[i::PRESENT]==0.{continue;}
            let output=&self.units.output[slot.actor*o::STRIDE..];let count=output[o::SKIN_COUNT]as usize;
            let mut matrix=[0.;16];matrix.copy_from_slice(&output[o::ROOT_WORLD..o::ROOT_WORLD+16]);let (center,radius)=bfe_render::math::sphere_to_world(&matrix,&[0.,0.9,0.],2.2);if !camera.frustum.sphere(&center,radius){continue;}
            if records==draw::MAX_RECORDS||bones+count>draw::MAX_BONES{continue;}
            // Bind inverses and skin bases belong to this prepared model.
            // Copy one instance record, then its contiguous skin prefix.
            let mut row=self.models[slot.model].row;row[draw::REC_WORLD..draw::REC_WORLD+16].copy_from_slice(&output[o::ROOT_WORLD..o::ROOT_WORLD+16]);row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE+16].copy_from_slice(&output[o::BIND_INVERSE..o::BIND_INVERSE+16]);row[draw::REC_BONES]=count as f64;row[draw::REC_BONE_AT]=bones as f64;
            let start=output[o::SKIN_START]as usize*16;bridge.r.core.bones[bones*16..(bones+count)*16].copy_from_slice(&self.units.skin[start..start+count*16]);
            let at=draw::IN_RECORDS+records*draw::REC_STRIDE;bridge.r.core.input[at..at+draw::REC_STRIDE].copy_from_slice(&row);records+=1;bones+=count;
        }(records,bones)
    }
}
