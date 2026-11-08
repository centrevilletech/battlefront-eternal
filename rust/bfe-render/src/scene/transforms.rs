//! Graphics.updateVisible's matrix producer consumes retained qualified links.
//! Dirty ancestors coalesce, while hidden descendants retain pending placement.
use super::{SceneRegistry,NONE,IDENTITY,row,flags,bits,index};
use crate::scene_transform_layout::{transform_change as c,transform_state as s,transform_flags as f,transform_phase as p};
use three::{Matrix4,Vector3,Quaternion};
#[derive(Clone,Copy,Default)]
struct State{generation:f64,flags:usize,changed:bool,local_dirty:bool,subtree:bool,version:u64,parent_version:u64,parent_valid:bool,children_version:u64}
pub struct Transforms{
    pub rotation:[f64;crate::scene_transform_layout::transform_rotation::STRIDE],
    pub(super) place_stack:Vec<(usize,f64,f64)>,
    pub changes:Vec<f64>,pub values:Vec<f64>,pub phase:[f64;p::STRIDE],states:Vec<State>,
    stack:Vec<(usize,bool,bool)>,pub updates:u64,pub visits:u64,
}
impl Transforms{
    pub fn new(count:usize)->Self{Self{rotation:[0.;crate::scene_transform_layout::transform_rotation::STRIDE],place_stack:Vec::with_capacity(count),changes:vec![0.;count*c::STRIDE],values:vec![0.;count*s::STRIDE],phase:[0.;p::STRIDE],states:vec![State::default();count],stack:Vec::with_capacity(count),updates:0,visits:0}}
    pub fn reserve(&mut self,count:usize){self.place_stack.reserve(count.saturating_sub(self.place_stack.len()));self.changes.resize(count*c::STRIDE,0.);self.values.resize(count*s::STRIDE,0.);self.states.resize(count,State::default());self.stack.reserve(count.saturating_sub(self.stack.len()));}
    // Descriptor mutations must not overwrite a retained native world with
    // the descriptor's cold authored matrix. Facts-only fixtures have no state.
    pub fn world(&self,id:usize,generation:f64)->Option<crate::math::Mat4>{
        let state=self.states.get(id)?;if state.generation!=generation{return None;}
        self.values.get(id*s::STRIDE+s::WORLD..id*s::STRIDE+s::WORLD+16)?.try_into().ok()
    }
    pub fn position(&self,id:usize,generation:f64)->Option<[f64;3]>{
        if self.states.get(id)?.generation!=generation{return None;}
        self.values.get(id*s::STRIDE+s::POSITION..id*s::STRIDE+s::POSITION+3)?.try_into().ok()
    }
    // A native unit/hull/prop world is already solved. Import its version;
    // named child getters can later observe the changed parent without math.
    pub fn import_world(&mut self,id:usize,world:&crate::math::Mat4,generation:f64){
        let Some(state)=self.states.get_mut(id)else{return;};
        self.values[id*s::STRIDE+s::WORLD..id*s::STRIDE+s::WORLD+16].copy_from_slice(world);
        state.generation=generation;state.version=state.version.wrapping_add(1).max(1);
    }
}
impl SceneRegistry{
    // Object3D.onQuaternionChange derived Euler angles through a temporary
    // Matrix4. Keep that exact f64 conversion without a Three callback.
    pub fn quaternion_rotation(&mut self,q:[f64;4],order:usize){
        use three::{Euler,EulerOrder};use crate::scene_transform_layout::{rotation_order as ro,transform_rotation as r};
        let order=match order{ro::YXZ=>EulerOrder::YXZ,ro::ZXY=>EulerOrder::ZXY,ro::ZYX=>EulerOrder::ZYX,ro::YZX=>EulerOrder::YZX,ro::XZY=>EulerOrder::XZY,_=>EulerOrder::XYZ};
        let mut euler=Euler::default();euler.set_from_quaternion(&Quaternion::new(q[0],q[1],q[2],q[3]),Some(order));
        self.transforms.rotation[r::X]=euler.x;self.transforms.rotation[r::Y]=euler.y;self.transforms.rotation[r::Z]=euler.z;
    }
    pub(super) fn transform_dirty(&mut self,id:usize){
        let mut at=id;
        for _ in 0..self.nodes.len(){if at==NONE{break;}self.transforms.states[at].subtree=true;at=self.nodes[at].parent;}
    }
    pub fn transform_apply(&mut self,count:usize){
        for ordinal in 0..count.min(self.transforms.changes.len()/c::STRIDE){
            let start=ordinal*c::STRIDE;let r=&self.transforms.changes[start..start+c::STRIDE];
            let Some(id)=index(r[c::SLOT],self.nodes.len())else{self.rejected+=1;continue;};
            let generation=r[c::GENERATION];
            if generation<1.||generation!=self.nodes[id].data[row::GENERATION]{self.rejected+=1;continue;}
            let flags=bits(r[c::FLAGS]);let at=id*s::STRIDE;
            let previous=self.transforms.states[id];let retained=flags&f::RETAINED!=0&&previous.generation==generation;
            let position=if retained&&flags&f::NATIVE_POSITION!=0{self.transforms.values[at+s::POSITION..at+s::POSITION+3].try_into().unwrap_or([0.;3])}else{[r[c::POSITION],r[c::POSITION+1],r[c::POSITION+2]]};
            let local=if flags&f::AUTO_LOCAL!=0{
                let mut matrix=Matrix4::default();matrix.compose(&Vector3::new(position[0],position[1],position[2]),
                    &Quaternion::new(r[c::QUATERNION],r[c::QUATERNION+1],r[c::QUATERNION+2],r[c::QUATERNION+3]),
                    &Vector3::new(r[c::SCALE],r[c::SCALE+1],r[c::SCALE+2]));matrix.elements
            }else if retained{self.transforms.values[at+s::LOCAL..at+s::LOCAL+16].try_into().unwrap_or(IDENTITY)}else{r[c::LOCAL..c::LOCAL+16].try_into().unwrap_or(IDENTITY)};
            self.transforms.values[at+s::LOCAL..at+s::LOCAL+16].copy_from_slice(&local);
            self.transforms.values[at+s::POSITION..at+s::POSITION+3].copy_from_slice(&position);
            self.transforms.values[at+s::QUATERNION..at+s::QUATERNION+4].copy_from_slice(&r[c::QUATERNION..c::QUATERNION+4]);
            self.transforms.values[at+s::SCALE..at+s::SCALE+3].copy_from_slice(&r[c::SCALE..c::SCALE+3]);
            if !retained&&(flags&f::AUTO_WORLD==0||previous.generation!=generation){
                let cold=r[c::WORLD..c::WORLD+16].try_into().unwrap_or(IDENTITY);
                let world=if flags&f::NATIVE!=0{self.poses.arena_world(self.nodes[id].pose_slot).copied().unwrap_or(cold)}else{cold};
                self.transforms.values[at+s::WORLD..at+s::WORLD+16].copy_from_slice(&world);
            }
            if !retained{self.transforms.values[at+s::BIND..at+s::BIND+16].copy_from_slice(&r[c::BIND..c::BIND+16]);}
            if flags&f::DETACHED!=0{
                let mut bind=Matrix4::default();if retained{bind.elements.copy_from_slice(&self.transforms.values[at+s::BIND..at+s::BIND+16]);}else{bind.elements.copy_from_slice(&r[c::BIND..c::BIND+16]);}bind.invert();
                self.transforms.values[at+s::BIND_INVERSE..at+s::BIND_INVERSE+16].copy_from_slice(&bind.elements);
            }
            let mut state=if previous.generation==generation{previous}else{State::default()};
            state.generation=generation;state.flags=flags;state.changed=true;state.local_dirty=false;state.subtree=false;self.transforms.states[id]=state;self.transform_dirty(id);
        }
    }
    // Follow position is an authored fact in this owner. Composition waits
    // for the ordinary update boundary, including after mirror restoration.
    pub fn transform_position(&mut self,id:usize,generation:f64,p:[f64;3]){
        let Some(state)=self.transforms.states.get(id)else{return;};if state.generation!=generation{return;}
        let at=id*s::STRIDE+s::POSITION;if (0..3).all(|i|self.transforms.values[at+i].to_bits()==p[i].to_bits()){return;}
        self.transforms.values[at..at+3].copy_from_slice(&p);self.transforms.states[id].changed=true;self.transforms.states[id].local_dirty=true;self.transform_dirty(id);
    }
    // A parent-only getter updates its ancestors but leaves descendant force
    // pending through children_version, exactly as SceneWorlds.parents did.
    pub fn transform_frame(&mut self){
        let Some(root)=index(self.transforms.phase[p::ROOT],self.nodes.len())else{return;};
        if self.transforms.phase[p::GENERATION]!=self.nodes[root].data[row::GENERATION]{return;}
        let descendants=self.transforms.phase[p::CHILDREN]!=0.;
        self.transforms.stack.clear();self.transforms.stack.push((root,self.transforms.phase[p::FORCE]!=0.,descendants));
        while let Some((id,mut force,children))=self.transforms.stack.pop(){
            let state=self.transforms.states[id];
            if state.generation!=self.nodes[id].data[row::GENERATION]||bits(self.nodes[id].data[row::FLAGS])&flags::ALIVE==0{continue;}
            let parent=self.nodes[id].parent;
            let parent_version=if parent==NONE{0}else{self.transforms.states[parent].version};
            if parent!=NONE&&(!state.parent_valid||state.parent_version!=parent_version){force=true;}
            if !force&&!state.changed&&(!children||!state.subtree)&&state.children_version==state.version{continue;}
            self.transforms.visits=self.transforms.visits.saturating_add(1);
            if state.flags&f::STOP!=0{
                let s=&mut self.transforms.states[id];s.changed=false;s.subtree=false;s.parent_version=parent_version;s.parent_valid=true;s.children_version=s.version;continue;
            }
            let update=force||state.changed;let at=id*s::STRIDE;
            if update{
                if state.local_dirty&&state.flags&f::AUTO_LOCAL!=0{
                    let v=&self.transforms.values;let mut local=Matrix4::default();local.compose(&Vector3::new(v[at+s::POSITION],v[at+s::POSITION+1],v[at+s::POSITION+2]),&Quaternion::new(v[at+s::QUATERNION],v[at+s::QUATERNION+1],v[at+s::QUATERNION+2],v[at+s::QUATERNION+3]),&Vector3::new(v[at+s::SCALE],v[at+s::SCALE+1],v[at+s::SCALE+2]));
                    self.transforms.values[at+s::LOCAL..at+s::LOCAL+16].copy_from_slice(&local.elements);
                }
                self.transforms.states[id].local_dirty=false;
                if state.flags&f::AUTO_WORLD!=0{
                    let local=self.transforms.values[at+s::LOCAL..at+s::LOCAL+16].try_into().unwrap_or(IDENTITY);
                    let world=if parent==NONE{local}else{
                        let from=parent*s::STRIDE+s::WORLD;
                        let parent=self.transforms.values[from..from+16].try_into().unwrap_or(IDENTITY);
                        crate::math::mul(&parent,&local)
                    };
                    self.transforms.values[at+s::WORLD..at+s::WORLD+16].copy_from_slice(&world);
                }
                let world:crate::math::Mat4=self.transforms.values[at+s::WORLD..at+s::WORLD+16].try_into().unwrap_or(IDENTITY);
                let changed=(0..16).any(|k|self.nodes[id].data[row::WORLD+k].to_bits()!=world[k].to_bits());
                self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&world);
                if changed&&bits(self.nodes[id].data[row::FAMILIES])!=0{self.refresh_bounds(id);}
                if state.flags&f::SKINNED!=0&&state.flags&f::ATTACHED!=0{
                    let mut bind=Matrix4::default();bind.elements=world;bind.invert();
                    self.transforms.values[at+s::BIND_INVERSE..at+s::BIND_INVERSE+16].copy_from_slice(&bind.elements);
                }
                self.transforms.states[id].changed=false;
                self.transforms.states[id].version=state.version.wrapping_add(1).max(1);
                self.transforms.updates=self.transforms.updates.saturating_add(1);
            }
            let version=self.transforms.states[id].version;
            self.transforms.states[id].parent_version=parent_version;self.transforms.states[id].parent_valid=true;
            if !children{continue;}
            self.transforms.states[id].subtree=false;
            let descend=update||state.children_version!=version;
            self.order.clear();let mut child=self.nodes[id].first;
            while child!=NONE{
                if self.nodes[child].data[row::PARENT_GENERATION]==state.generation{
                    if self.visible(child){self.order.push(child);}else if descend{self.transforms.states[child].changed=true;}
                }
                child=self.nodes[child].next;
            }
            for &child in self.order.iter().rev(){self.transforms.stack.push((child,descend,true));}
            self.transforms.states[id].children_version=version;
        }
    }
}
