// Retained projectObject inputs. Scene mutation publishes sparse rows; views
// never import a second scene or ask the host to choose visible objects.
// Identities include generations so a queued write cannot revive a dead mesh.
use crate::math::{self, Frustum, Mat4};
#[path = "scene_layout.rs"]
pub mod layout;
use layout::{scene_fact as fact, scene_facts_header as header, scene_family as family, scene_capacity as capacity, scene_flags as flags, scene_mode as mode,
    scene_node as row, scene_pose_ref as pose_ref, scene_world_change as world_change, scene_group as group, scene_line_header as line_header, scene_motion_header as mh, scene_motion_event as me, scene_motion_kind as mk, scene_selection as selected, scene_view as view};
use std::cmp::Ordering;
mod transforms;
mod pose_join;
mod skeletons;
mod point_lights;
mod placements;

// Near/view/shadow selection has a small set of root/layer policies. One
// retained traversal per scene root records authored topology once; each
// camera/family policy derives its compact draw list from that traversal.
// Fixed caches bound memory even if a tool cycles through many unique roots;
// eviction rebuilds into already reserved storage and never drops a draw.
const MASTERS: usize = 6;
const VISIBILITY_VIEWS: usize = 16;
const NONE: usize = usize::MAX;
const NO_PARENT: u32 = u32::MAX;
const IDENTITY: Mat4 = [1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];

#[derive(Clone, Copy)]
struct Node {
    data: [f64; row::STRIDE],
    parent: usize,
    first: usize,
    next: usize,
    previous: usize,
    lod_visible: bool,
    motion_detached: u16, motion_cleared: bool,
    pose_slot:usize,pose_next:usize,
}
impl Default for Node {
    fn default() -> Self {
        Self { data: [0.; row::STRIDE], parent: NONE, first: NONE, next: NONE, previous: NONE, lod_visible: true, motion_detached:0, motion_cleared:false,pose_slot:NONE,pose_next:NONE }
    }
}

// The facts a query reads per candidate, packed apart from the published row.
// A slow laptop streams these contiguous records instead of touching six
// cache lines of a 52-value row and trailing bounds for every drawable.
#[derive(Clone, Copy, Default)]
struct Hot {
    flags: u32, families: u32, group_first: u32, group_count: u32,
    generation: f64, resource: f64, render_order: f64, object_id: f64, material_id: f64,
    range: [f64; 3], sphere: [f64; 3], radius: f64, clip: [f64; 4],
}

// Material-array groups belong to their node's life. Keeping them outside Node
// avoids copying a material table while traversing every ordinary transform.
#[derive(Clone, Copy)]
struct Group {
    owner: usize, generation: f64, material: f64, material_index: f64,
    visible: bool, first: f64, count: f64,
}
impl Default for Group {
    fn default()->Self {Self{owner:NONE,generation:0.,material:0.,material_index:0.,visible:false,first:0.,count:0.}}
}

// One visit of the retained traversal: authored order, the subtree length
// that lets a policy skip a pruned branch without walking it, and the few
// row values a policy needs. A childless drawable is a lazy leaf: its own
// visibility is read when a query runs, so an effect mesh blinking on and
// off does not expire every root's traversal.
#[derive(Clone, Copy)]
struct Visit { id:u32, parent:u32, subtree:u32, layers:u32, flags:u32, families:u32, lazy:bool, render_order:f64 }
struct Master { key:Option<(usize,u32)>, revision:u64, serial:u64, visits:Vec<Visit>, lods:Vec<u32>, lazy:Vec<u32> }
impl Master {
    fn new(count:usize)->Self {Self{key:None,revision:0,serial:0,visits:Vec::with_capacity(count),lods:Vec::with_capacity(count),lazy:vec![0;count.div_ceil(32)]}}
}
// A derived policy keeps only the candidates its family can draw, in scene
// order, plus the membership bitset shown() reads for terrain and effects.
#[derive(Clone, Copy)]
struct Draw { id:u32, lazy:bool, group:f64 }
#[derive(Clone, Copy, PartialEq, Eq)]
struct ViewKey { root:usize, generation:u32, layers:usize, families:usize, near:bool, far:bool, all_groups:bool, motion:usize, receipts:bool }
struct View { key:Option<ViewKey>, serial:u64, draws:Vec<Draw>, visible:Vec<u32> }
impl View {
    fn new(count:usize)->Self {Self{key:None,serial:0,draws:Vec::with_capacity(count),visible:vec![0;count.div_ceil(32)]}}
}

// Persistent scratch is reserved with the content. A paused producer coalesces
// each touched identity, so queue volume is bounded by admitted nodes, not time.
pub struct SceneRegistry {
    pub skin_admit:Vec<f32>,
    pub place_output:Vec<u32>,
    pub points:point_lights::PointLights,
    pub skeletons:skeletons::Skeletons,
    pub transforms:transforms::Transforms,
    pub poses:crate::scene_pose::ScenePoses,
    pub canopies: crate::scene_canopies::CanopySelection,
    nodes: Vec<Node>,
    hot: Vec<Hot>,
    // Each node's own authored/LOD visibility as a bit, kept current by every
    // mutation so lazy leaves never need their row during a query.
    authored: Vec<u32>,
    pub changes: Vec<f64>,
    pub world_changes:Vec<f64>,
    groups: Vec<Group>,
    pub group_changes: Vec<f64>,
    pub view: [f64; view::STRIDE],
    pub output: Vec<f64>,
    pub line_output: Vec<f64>,
    pub motion_events: Vec<f64>,
    motion_root: usize, motion_root_generation:f64, motion_event_count:usize,
    pub scene_order: Vec<f64>,
    pub facts: Vec<f64>,
    pub visible:Vec<u32>,
    pub visibility_visits:u64,pub drawable_visits:u64,pub queries:u64,
    epoch:u32,
    visibility_revision:u64, traversals:u64,
    masters:Vec<Master>, next_master:usize,
    views:Vec<View>, next_view:usize,
    pub visibility_builds:u64, pub bounds_updates:u64,
    order: Vec<usize>,
    stack: Vec<(usize, u32, f64)>,
    group_stack: Vec<(u32, f64)>,
    selection: Vec<[f64; selected::STRIDE]>,
    pub selected: usize,
    pub rejected: u64,
    pub clipped: u64,
}

impl Default for SceneRegistry {
    fn default() -> Self { Self::new(8192) }
}
impl SceneRegistry {
    // RetainedScene.admit reserves topology and every selection scratch store
    // together. A query can visit each admitted node without a later reserve.
    // The group atlas has its own limit because one mesh may have many groups.
    // Headers remain outside the indexed record span at every capacity.
    pub fn new(count: usize) -> Self {
        let count = count.clamp(1, capacity::NODES);
        Self {
            skin_admit:Vec::new(),place_output:vec![0;crate::scene_pose_layout::pose_limits::MODELS],points:point_lights::PointLights::new(count),skeletons:Default::default(),transforms:transforms::Transforms::new(count),poses:Default::default(),
            canopies: Default::default(),
            nodes: vec![Node::default(); count],
            hot: vec![Hot::default(); count],
            authored: vec![0; count.div_ceil(32)],
            changes: vec![0.; count * row::STRIDE],
            world_changes:vec![0.;count*world_change::STRIDE],
            groups: vec![Group::default();(count*4).min(capacity::GROUPS)],
            group_changes: vec![0.;(count*4).min(capacity::GROUPS)*group::STRIDE],
            view: [0.; view::STRIDE],
            output: vec![0.; count * selected::STRIDE],
            motion_events:vec![0.;mh::STRIDE+count*me::STRIDE],motion_root:NONE,motion_root_generation:0.,motion_event_count:0,
            line_output:vec![0.;line_header::STRIDE+count*selected::STRIDE],
            scene_order: vec![0.; count * selected::STRIDE],
            facts: vec![0.; header::STRIDE + count * fact::STRIDE],
            visible:vec![0;count.div_ceil(32)],visibility_visits:0,drawable_visits:0,queries:0,epoch:0,
            visibility_revision:1,traversals:0,
            // The last master is scratch for motion policies, which re-walk
            // their small pool each query to advance source lifecycles.
            masters:(0..=MASTERS).map(|_|Master::new(count)).collect(),next_master:0,
            views:(0..VISIBILITY_VIEWS).map(|_|View::new(count)).collect(),next_view:0,
            visibility_builds:0,bounds_updates:0,
            order: Vec::with_capacity(count), stack: Vec::with_capacity(count), group_stack: Vec::with_capacity(count),
            selection: Vec::with_capacity(count), selected: 0, rejected: 0, clipped: 0,
        }
    }

    // Cold reservation only, before publishing topology. Existing rows and ids
    // survive growth; a live store filling clips instead of growing or panicking.
    pub fn reserve(&mut self, count: usize) {
        let count = count.min(capacity::NODES);
        if count <= self.nodes.len() { return; }
        self.transforms.reserve(count);self.points.reserve(count);
        self.nodes.resize(count, Node::default());
        self.hot.resize(count, Hot::default());
        self.authored.resize(count.div_ceil(32),0);
        self.changes.resize(count * row::STRIDE, 0.);
        self.world_changes.resize(count*world_change::STRIDE,0.);
        self.output.resize(count * selected::STRIDE, 0.);
        self.motion_events.resize(mh::STRIDE+count*me::STRIDE,0.);
        self.line_output.resize(line_header::STRIDE+count*selected::STRIDE,0.);
        self.scene_order.resize(count * selected::STRIDE, 0.);
        self.facts.resize(header::STRIDE + count * fact::STRIDE, 0.);
        self.visible.resize(count.div_ceil(32),0);
        self.order.reserve(count.saturating_sub(self.order.len()));
        self.stack.reserve(count.saturating_sub(self.stack.len()));
        self.group_stack.reserve(count.saturating_sub(self.group_stack.len()));
        self.selection.reserve(count.saturating_sub(self.selection.len()));
        for master in &mut self.masters {
            master.visits.reserve(count.saturating_sub(master.visits.len()));
            master.lods.reserve(count.saturating_sub(master.lods.len()));
            master.lazy.resize(count.div_ceil(32),0);
        }
        for view in &mut self.views {
            view.draws.reserve(count.saturating_sub(view.draws.len()));
            view.visible.resize(count.div_ceil(32),0);
        }
    }

    // Geometry/material replacement imports a coalesced group block. Ordinary
    // matrix publication leaves these immutable ranges and visibility rows alone.
    pub fn reserve_groups(&mut self,count:usize) {
        let count=count.min(capacity::GROUPS);
        if count<=self.groups.len(){return;}
        self.groups.resize(count,Group::default());
        self.group_changes.resize(count*group::STRIDE,0.);
    }
    // BufferGeometry.addGroup/clearGroups publish authored ranges separately
    // from moving node rows. Owner generation prevents a delayed group block
    // from attaching an old material range to a replacement node life.
    pub fn apply_groups(&mut self,count:usize) {
        for i in 0..count.min(self.group_changes.len()/group::STRIDE) {
            let r=&self.group_changes[i*group::STRIDE..(i+1)*group::STRIDE];
            let Some(slot)=index(r[group::SLOT],self.groups.len()) else{self.rejected+=1;continue;};
            let Some(owner)=index(r[group::OWNER],self.nodes.len()) else{self.rejected+=1;continue;};
            let generation=r[group::GENERATION];
            if generation!=self.nodes[owner].data[row::GENERATION] || generation<1.
                || bits(self.nodes[owner].data[row::FLAGS])&flags::ALIVE==0 {self.rejected+=1;continue;}
            // Imported material indices cannot address a host array through
            // NaN, a fraction or an overflowing integer. Infinite authored
            // counts remain valid because selection clips against the mesh.
            if [r[group::MATERIAL_ID],r[group::MATERIAL_INDEX]].iter().any(|v|
                !v.is_finite() || *v<0. || *v>u32::MAX as f64 || v.fract()!=0.)
                || r[group::FIRST].is_nan() || r[group::COUNT].is_nan() {self.rejected+=1;continue;}
            self.groups[slot]=Group{owner,generation,material:r[group::MATERIAL_ID],material_index:r[group::MATERIAL_INDEX],
                visible:r[group::VISIBLE]!=0.,first:r[group::FIRST],count:r[group::COUNT]};
        }
    }

    pub fn world(&self,id:usize)->Option<Mat4>{
        let n=self.nodes.get(id)?;let p=&n.data[row::POSE..row::POSE+pose_ref::STRIDE];
        if p[pose_ref::OWNER]==0.{return self.transforms.world(id,n.data[row::GENERATION]).or_else(||n.data[row::WORLD..row::WORLD+16].try_into().ok());}
        let owner=index(p[pose_ref::OWNER],crate::scene_pose_layout::pose_limits::OWNERS)?;
        let model=index(p[pose_ref::ID],crate::scene_pose_layout::pose_limits::MODELS)?;
        let generation=p[pose_ref::GENERATION];if !generation.is_finite()||generation<1.||generation>u32::MAX as f64||generation.fract()!=0.{return None;}
        let node=index(p[pose_ref::NODE_START],crate::scene_pose_layout::pose_limits::SOURCE_NODES)?;
        if let Some(pose)=self.poses.node(owner,model,generation as u32,node){return Some(pose.world);}
        // Hidden actors can enter an aggregate skin before their first lazy
        // placement. Their qualified cold bind world remains authoritative.
        self.poses.slot(owner,model,generation as u32,node)?;
        self.transforms.world(id,n.data[row::GENERATION]).or_else(||n.data[row::WORLD..row::WORLD+16].try_into().ok())
    }
    pub fn pose_ref(&self,id:usize,generation:f64)->Option<[f64;pose_ref::STRIDE]>{
        if !generation.is_finite()||generation<1.||generation.fract()!=0.||generation>u32::MAX as f64{return None;}
        let n=self.nodes.get(id)?;if n.data[row::GENERATION]!=generation||bits(n.data[row::FLAGS])&flags::ALIVE==0{return None;}
        let pose=&n.data[row::POSE..row::POSE+pose_ref::STRIDE];
        if pose.iter().any(|v|!v.is_finite()||*v<0.||v.fract()!=0.||*v>u32::MAX as f64){return None;}
        pose.try_into().ok()
    }

    // Generated rows are copied once per scene phase. Every imported length and
    // handle is validated here; malformed or stale rows leave their owner intact.
    pub fn apply(&mut self, count: usize) {
        let count = count.min(self.changes.len() / row::STRIDE);
        for r in 0..count {
            let mut data = [0.; row::STRIDE];
            data.copy_from_slice(&self.changes[r * row::STRIDE..(r + 1) * row::STRIDE]);
            let Some(id) = index(data[row::SLOT], self.nodes.len()) else { self.rejected += 1; continue; };
            let generation = data[row::GENERATION];
            if !generation.is_finite() || generation < 1. || generation>u32::MAX as f64 || generation.fract() != 0.
                || generation < self.nodes[id].data[row::GENERATION] { self.rejected += 1; continue; }
            if let Some(world)=self.transforms.world(id,generation){data[row::WORLD..row::WORLD+16].copy_from_slice(&world); }
            // Object3D.remove may leave descendants retained for pooling.
            // Retiring the root must suppress that old subtree immediately;
            // it does not require walking or deleting its resource children.
            let alive = bits(data[row::FLAGS]) & flags::ALIVE != 0;
            let parent = index(data[row::PARENT], self.nodes.len()).unwrap_or(NONE);
            // An unchanged parent cannot introduce a cycle. Pose-only rows
            // must not walk their complete ancestor chain at each publication.
            if parent != NONE && parent != self.nodes[id].parent && (parent == id || self.cycle(id, parent)) { self.rejected += 1; continue; }
            // A newer life invalidates the old parent link before it is admitted.
            // Child links retain their qualified parent generation and stay hidden.
            let old=&self.nodes[id].data;
            // A childless drawable keeps its own visibility as a lazy leaf: an
            // effect mesh blinking on and off is read at query time instead
            // of expiring every root's traversal. Hidden parents still do.
            let leaf = self.nodes[id].first==NONE && bits(old[row::FAMILIES])!=0 && bits(data[row::FAMILIES])!=0;
            let visibility_flags=flags::ALIVE|flags::GROUP|flags::SUBTREE_OWNER|flags::CLAIMED_TREE|flags::MOTION_SOURCE
                | if leaf {0} else {flags::VISIBLE};
            // Only inputs to the topology walk expire retained visits. Blend,
            // cast, range and material changes are read when emitting a draw.
            // In particular, a moving actor does not expire every static view.
            let visibility_changed=[row::GENERATION,row::PARENT,row::PARENT_GENERATION,row::SIBLING,row::LAYERS,
                row::RENDER_ORDER,row::LOD_AUTO,row::LOD_INDEX,row::LOD_DISTANCE,row::LOD_HYSTERESIS]
                .iter().any(|&at|old[at].to_bits()!=data[at].to_bits())
                || (bits(old[row::FLAGS])^bits(data[row::FLAGS]))&visibility_flags!=0
                // Families select which derived list a node belongs to, so a
                // claim or pass binding is a topology change, not a draw fact.
                || old[row::FAMILIES].to_bits()!=data[row::FAMILIES].to_bits();
            let bounds_changed=old[row::GENERATION]!=generation
                || bits(old[row::FAMILIES])==0 && bits(data[row::FAMILIES])!=0
                || (row::SPHERE..row::WORLD+16).any(|at|old[at].to_bits()!=data[at].to_bits())
                || (bits(old[row::FLAGS])^bits(data[row::FLAGS]))&flags::SPRITE!=0;
            // A pose-only row leaves the node's own visibility bit alone.
            let authored_changed=old[row::GENERATION]!=generation || old[row::LOD_INDEX]!=data[row::LOD_INDEX]
                || old[row::PARENT]!=data[row::PARENT] || old[row::SIBLING]!=data[row::SIBLING] || !alive
                || (bits(old[row::FLAGS])^bits(data[row::FLAGS]))&flags::VISIBLE!=0;
            if visibility_changed {self.invalidate_visibility();}
            let relink = self.nodes[id].parent != parent
                || self.nodes[id].data[row::SIBLING] != data[row::SIBLING]
                || !alive || self.nodes[id].data[row::GENERATION] != generation;
            if relink { self.unlink(id); }
            if self.nodes[id].data[row::GENERATION] != generation
                || bits(self.nodes[id].data[row::FLAGS]) & flags::VISIBLE != bits(data[row::FLAGS]) & flags::VISIBLE {
                self.nodes[id].lod_visible = bits(data[row::FLAGS]) & flags::VISIBLE != 0;
            }
            if self.nodes[id].data[row::GENERATION]!=generation {
                self.nodes[id].motion_detached=0;self.nodes[id].motion_cleared=false;
            }
            self.nodes[id].data = data;
            let h=&mut self.hot[id];
            h.flags=bits(data[row::FLAGS]) as u32;h.families=bits(data[row::FAMILIES]) as u32;
            h.group_first=index(data[row::GROUP_FIRST],u32::MAX as usize).unwrap_or(u32::MAX as usize) as u32;
            h.group_count=index(data[row::GROUP_COUNT],u32::MAX as usize).unwrap_or(0) as u32;
            h.generation=generation;h.resource=data[row::RESOURCE];h.render_order=data[row::RENDER_ORDER];
            h.object_id=data[row::OBJECT_ID];h.material_id=data[row::MATERIAL_ID];
            h.range=[data[row::RANGE],data[row::RANGE+1],data[row::RANGE+2]];
            if authored_changed {self.set_authored(id);}
            let p=&data[row::POSE..row::POSE+pose_ref::STRIDE];
            let source=self.poses.slot(p[pose_ref::OWNER] as usize,p[pose_ref::ID] as usize,p[pose_ref::GENERATION] as u32,p[pose_ref::NODE_START] as usize).unwrap_or(NONE);
            if source!=self.nodes[id].pose_slot {
                let old=self.nodes[id].pose_slot;
                if let Some(head)=self.poses.heads.get_mut(old){
                    let mut at=*head;let mut previous=NONE;
                    for _ in 0..self.nodes.len(){if at==NONE{break;}
                     if at==id{let next=self.nodes[at].pose_next;if previous==NONE{*head=next;}else{self.nodes[previous].pose_next=next;}break;}previous=at;at=self.nodes[at].pose_next;}
                }
                self.nodes[id].pose_slot=source;self.nodes[id].pose_next=NONE;
                if let Some(head)=self.poses.heads.get_mut(source){self.nodes[id].pose_next = *head;*head=id;}
            }
            if source!=NONE {if let Some(world)=self.poses.arena_world(source).copied(){self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&world);self.transforms.import_world(id,&world,generation);}}

            if bounds_changed && bits(data[row::FAMILIES])!=0 {self.refresh_bounds(id);}
            if relink && alive && parent != NONE { self.link(id, parent); }
            if authored_changed||relink{self.transform_dirty(id);}
        }
    }
    // Object3D.visible or an LOD level choice, as one bit per node. Queries
    // read it for lazy leaves; traversal and mutation keep it current.
    fn set_authored(&mut self,id:usize) {
        let bit=1u32<<(id%32);
        if self.visible(id) {self.authored[id/32]|=bit;} else {self.authored[id/32]&=!bit;}
    }

    // Changed native worlds refresh only qualified drawing subscribers.
    pub fn pose_frame(&mut self)->usize{
        let count=self.poses.apply();
        for i in 0..self.poses.changed.len(){
            let source=self.poses.changed[i];let Some(world)=self.poses.arena_world(source).copied()else{continue;};
            let mut id=self.poses.heads[source];
            for _ in 0..self.nodes.len(){if id==NONE{break;}self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&world);self.transforms.import_world(id,&world,self.nodes[id].data[row::GENERATION]);if bits(self.nodes[id].data[row::FAMILIES])!=0{self.refresh_bounds(id);}id=self.nodes[id].pose_next;}
        }
        count
    }

    pub fn tree_frame(&mut self)->usize{
        let count=self.poses.import_stream();
        for i in 0..self.poses.changed.len(){
            let source=self.poses.changed[i];let Some(world)=self.poses.arena_world(source).copied()else{continue;};let mut id=self.poses.heads[source];
            for _ in 0..self.nodes.len(){if id==NONE{break;}self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&world);self.transforms.import_world(id,&world,self.nodes[id].data[row::GENERATION]);if bits(self.nodes[id].data[row::FAMILIES])!=0{self.refresh_bounds(id);}id=self.nodes[id].pose_next;}
        }count
    }
    // RetainedScene.flush changes descriptor or pose rows independently. A
    // matrix update cannot invalidate every root's visibility and layer list.
    fn invalidate_visibility(&mut self) {
        self.visibility_revision=self.visibility_revision.wrapping_add(1);
        if self.visibility_revision==0 {
            for master in &mut self.masters {master.revision=0;}
            self.visibility_revision=1;
        }
    }
    // Sphere.applyMatrix4 and NearPass.item's first matrix multiplication
    // depend only on this row. Retaining both results preserves each helper's
    // different W convention while avoiding their repetition for every view.
    // The world centre is also the sort-key fact effects read through key();
    // publishing it here keeps that fact current without a per-query copy.
    fn refresh_bounds(&mut self,id:usize) {
        let n=&self.nodes[id];
        let world:Mat4=n.data[row::WORLD..row::WORLD+16].try_into().unwrap_or(IDENTITY);
        let center=[n.data[row::SPHERE],n.data[row::SPHERE+1],n.data[row::SPHERE+2]];
        let (sphere,radius)=math::sphere_to_world(&world,&center,n.data[row::SPHERE+3]);
        let clip=world_point(&world,&center,bits(n.data[row::FLAGS])&flags::SPRITE!=0);
        let h=&mut self.hot[id];h.sphere=sphere;h.radius=radius;h.clip=clip;
        let at=header::STRIDE+id*fact::STRIDE+fact::CENTER;
        self.facts[at..at+3].copy_from_slice(&sphere);
        self.bounds_updates=self.bounds_updates.saturating_add(1);
    }

    // RetainedScene.flush publishes poses without resending immutable flags,
    // ranges or topology. A delayed pose cannot target a replacement node life.
    pub fn apply_worlds(&mut self,count:usize) {
        for i in 0..count.min(self.world_changes.len()/world_change::STRIDE) {
            let at=i*world_change::STRIDE;
            let Some(id)=index(self.world_changes[at+world_change::SLOT],self.nodes.len()) else{self.rejected+=1;continue;};
            let generation=self.world_changes[at+world_change::GENERATION];
            if generation<1. || generation!=self.nodes[id].data[row::GENERATION] {self.rejected+=1;continue;}
            let from=at+world_change::WORLD;
            if (0..16).all(|k|self.world_changes[from+k].to_bits()==self.nodes[id].data[row::WORLD+k].to_bits()){continue;}
            self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&self.world_changes[from..from+16]);
            let world=self.nodes[id].data[row::WORLD..row::WORLD+16].try_into().unwrap_or(IDENTITY);self.transforms.import_world(id,&world,self.nodes[id].data[row::GENERATION]);
            if bits(self.nodes[id].data[row::FAMILIES])!=0{self.refresh_bounds(id);}
        }
    }

    fn cycle(&self, id: usize, mut parent: usize) -> bool {
        // Invalid cyclic input is rejected without recursion or unbounded work.
        for _ in 0..self.nodes.len() {
            if parent == id { return true; }
            parent = self.nodes[parent].parent;
            if parent == NONE { return false; }
        }
        true
    }
    // Object3D.remove changes only sibling links. Selection never borrows a
    // host children array, and detachment does not destroy the retained node.
    // Repair both neighbours so later reattachment preserves scene order.
    fn unlink(&mut self, id: usize) {
        let n = self.nodes[id];
        if n.parent != NONE && self.nodes[n.parent].first == id { self.nodes[n.parent].first = n.next; }
        if n.previous != NONE { self.nodes[n.previous].next = n.next; }
        if n.next != NONE { self.nodes[n.next].previous = n.previous; }
        self.nodes[id].parent = NONE;
        self.nodes[id].previous = NONE;
        self.nodes[id].next = NONE;
    }
    // Object3D.add appends in authored insertion order, independent of slot
    // allocation. A reparented old slot therefore follows existing siblings.
    // Only mutation pays this ordered insertion cost; queries remain linear.
    fn link(&mut self, id: usize, parent: usize) {
        let mut next = self.nodes[parent].first;
        let mut previous = NONE;
        while next != NONE && self.nodes[next].data[row::SIBLING] <= self.nodes[id].data[row::SIBLING] {
            previous = next; next = self.nodes[next].next;
        }
        self.nodes[id].parent = parent;
        self.nodes[id].previous = previous;
        self.nodes[id].next = next;
        if previous == NONE { self.nodes[parent].first = id; } else { self.nodes[previous].next = id; }
        if next != NONE { self.nodes[next].previous = id; }
    }

    // LOD.update is a near-list decision. Shadow and far views consume the
    // level left by that decision, including hysteresis and camera zoom. The
    // native state is separate from the shell's authored local visibility.
    fn update_lod(&mut self, id: usize) {
        let world = &self.nodes[id].data[row::WORLD..row::WORLD + 16];
        let dx = self.view[view::EYE] - world[12];
        let dy = self.view[view::EYE + 1] - world[13];
        let dz = self.view[view::EYE + 2] - world[14];
        let distance = (dx * dx + dy * dy + dz * dz).sqrt() / self.view[view::ZOOM];
        // Three's LOD levels are ordered by distance. Hysteresis applies to
        // the level that was previously visible, before choosing the new one.
        // Keep that visibility until a later near query actually updates it.
        let mut chosen = 0.;
        let mut level = 1.;
        loop {
            let mut child = self.nodes[id].first;
            let mut found = NONE;
            while child != NONE {
                if self.nodes[child].data[row::LOD_INDEX] == level { found = child; break; }
                child = self.nodes[child].next;
            }
            if found == NONE { break; }
            let node = &self.nodes[found];
            let mut threshold = node.data[row::LOD_DISTANCE];
            if node.lod_visible { threshold -= threshold * node.data[row::LOD_HYSTERESIS]; }
            if !matches!(distance.partial_cmp(&threshold), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)) { break; }
            chosen = level; level += 1.;
        }
        let mut child = self.nodes[id].first;
        while child != NONE {
            if self.nodes[child].data[row::LOD_INDEX] >= 0. {
                let visible=self.nodes[child].data[row::LOD_INDEX] == chosen;
                if self.nodes[child].lod_visible!=visible {
                    self.nodes[child].lod_visible=visible;self.set_authored(child);
                    // A level that is itself a childless mesh is a lazy leaf.
                    if self.nodes[child].first!=NONE || self.hot[child].families==0 {self.invalidate_visibility();}
                }
            }
            child = self.nodes[child].next;
        }
    }

    // NearPass.walk, ViewPass.visit and ShadowPass.candidates share hidden-parent
    // suppression. A layer mismatch never prunes children. Near Group order only
    // changes on a matching layer, while ViewPass inherits every visible Group.
    pub fn select(&mut self) -> usize {
        self.queries=self.queries.saturating_add(1);
        let mode = bits(self.view[view::MODE]);
        if mode==mode::MOTION_PREPARE {
            self.motion_event_count=0;self.motion_events[..mh::STRIDE].fill(0.);
            self.motion_root=index(self.view[view::MOTION_ROOT],self.nodes.len()).unwrap_or(NONE);
            self.motion_root_generation=self.view[view::MOTION_ROOT+1];
        }
        self.selection.clear(); self.selected = 0;
        self.visible.fill(0);self.facts[header::CLIPPED]=0.;
        // Culling receipts are stamped with the query that wrote them, so a
        // water surface checked after a shadow query keeps its near-view fact.
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            for id in 0..self.nodes.len() { self.facts[header::STRIDE + id * fact::STRIDE + fact::EPOCH] = 0.; }
            self.epoch = 1;
        }
        self.facts[header::EPOCH] = self.epoch as f64;
        let families = bits(self.view[view::FAMILIES]);
        if families&family::LINE!=0{self.line_output[line_header::COUNT]=0.;}
        let Some(root) = index(self.view[view::ROOT], self.nodes.len()) else { return 0; };
        if self.nodes[root].data[row::GENERATION] != self.view[view::GENERATION] { return 0; }
        // A view supplies one root life, camera block and family mask.
        // Reusing the same registry across reflected and shadow views keeps
        // resource admission independent of whichever camera arrives first.
        let mask = bits(self.view[view::LAYERS]);
        let projection: Mat4 = self.view[view::PROJECTION..view::PROJECTION + 16].try_into().unwrap_or(IDENTITY);
        let camera: Mat4 = self.view[view::VIEW..view::VIEW + 16].try_into().unwrap_or(IDENTITY);
        let screen = math::mul(&projection, &camera);
        let frustum = Frustum::from_matrix(&screen);
        let motion=if mode==mode::MOTION_PREPARE || mode==mode::PROXY {mode}else{0};
        // WaterPass.shown reads the frustum receipt of the last near, instance,
        // line or sea preparation. Those queries stamp every drawable they
        // visit, as projectObject did, so a surface bound after the query
        // still carries this frame's receipt; shadow, far and motion do not.
        let receipts=mode==mode::SCENE || mode==mode::INSTANCE || families&(family::LINE|family::SEA)!=0;
        let key=ViewKey{root,generation:self.view[view::GENERATION] as u32,layers:mask,families,near:families&family::NEAR!=0,
            far:mode==mode::FAR,all_groups:mode==mode::VIEW || mode==mode::INSTANCE,motion,receipts};
        // Motion lifecycle advances even on identical views, through the
        // scratch master. Ordinary cameras share one traversal per root.
        let master=if motion!=0 {MASTERS} else {
            match self.masters[..MASTERS].iter().position(|m|m.key==Some((root,key.generation))) {
                Some(m)=>m,
                None=>{let m=self.next_master;self.next_master=(m+1)%MASTERS;self.masters[m].key=None;m}
            }
        };
        // LOD hysteresis still advances at each near query. A stable choice
        // does not rebuild every static ancestor and bone merely because a
        // visible LOD exists. Stop at a changed outer choice before rebuilding
        // so a newly hidden nested LOD cannot advance outside its old boundary.
        let near_mask=if key.near {mask} else {0};
        let retained=motion==0 && self.masters[master].key.is_some() && self.masters[master].revision==self.visibility_revision;
        if retained && near_mask!=0 {
            let revision=self.visibility_revision;
            for i in 0..self.masters[master].lods.len() {
                let id=self.masters[master].lods[i] as usize;
                if bits(self.nodes[id].data[row::LAYERS])&near_mask==0 {continue;}
                self.update_lod(id);
                if self.visibility_revision!=revision {break;}
            }
        }
        if motion!=0 || self.masters[master].key.is_none() || self.masters[master].revision!=self.visibility_revision {
            self.traverse(master,root,key.generation,motion,near_mask);
        }
        let serial=self.masters[master].serial;
        let cache=match self.views.iter().position(|v|v.key==Some(key)) {
            Some(v)=>v,
            None=>{let v=self.next_view;self.next_view=(v+1)%VISIBILITY_VIEWS;self.views[v].key=None;v}
        };
        if self.views[cache].key.is_none() || self.views[cache].serial!=serial {self.derive(master,cache,key);}
        // Membership is a retained bitset; lazy leaves consult their current
        // authored bit so shown() sees an effect mesh hidden since the walk.
        for w in 0..self.visible.len() {
            self.visible[w]=self.views[cache].visible[w] & (!self.masters[master].lazy[w] | self.authored[w]);
        }
        self.drawable_visits=self.drawable_visits.saturating_add(self.views[cache].draws.len() as u64);
        for ordinal in 0..self.views[cache].draws.len() {
            let Draw{id,lazy,group}=self.views[cache].draws[ordinal];
            let id=id as usize;
            if lazy && self.authored[id/32]&(1<<(id%32))==0 {continue;}
            let h=&self.hot[id];let f=h.flags as usize;
            let motion_source=f&flags::MOTION_SOURCE!=0 && motion!=0;
            // ShadowPass.walk admitted native static anchors before its sphere
            // test: their cells and real bounds belong to the static depth
            // owner. An anchor's origin cannot stand in for those bounds.
            // Far owners likewise retain their existing spatial selection.
            let in_view=f&flags::CULLED==0 || mode==mode::MOTION_PREPARE || f&flags::NATIVE_CULL!=0 || frustum.sphere(&h.sphere,h.radius);
            if key.receipts {
                let at=header::STRIDE+id*fact::STRIDE;
                self.facts[at+fact::EPOCH]=self.epoch as f64;self.facts[at+fact::GENERATION]=h.generation;
                self.facts[at+fact::IN_VIEW]=in_view as u8 as f64;
                if h.families as usize&families==0 {continue;}
            }
            if !motion_source && f & flags::MATERIAL_VISIBLE == 0 { continue; }
            if mode == mode::SHADOW && f & flags::CAST_SHADOW == 0 { continue; }
            if mode != mode::FAR && mode!=mode::MOTION_PREPARE && f & flags::NATIVE_CULL == 0 && f & flags::CULLED != 0 && !in_view { continue; }
            if families&family::LINE!=0 && h.resource<0.{continue;}
            // Selection identities retain the original Object3D/Material IDs.
            // Native resource handles are payloads, never painter tie breakers.
            // Sphere centres also remain f64 until the existing GPU upload.
            // Motion extras used Vector3.applyMatrix4 twice; unlike near/view
            // Vector4 sort keys, these two stages each divide by W.
            let depth = if mode == mode::EXTRA { vector3_depth(&h.sphere, &screen) } else { clip_z(&h.clip,&screen) };
            let first = h.range[0].max(0.);
            let end = (h.range[0] + h.range[1]).min(h.range[2]);
            if end <= first && mode != mode::SHADOW && mode!=mode::PROXY && mode!=mode::EXTRA && mode!=mode::MOTION_PREPARE { continue; }
            let mut out = [0.; selected::STRIDE];
            out[selected::SLOT] = id as f64; out[selected::GENERATION] = h.generation;
            out[selected::RESOURCE] = h.resource; out[selected::GROUP_ORDER] = group;
            out[selected::RENDER_ORDER] = h.render_order; out[selected::DEPTH] = depth;
            out[selected::OBJECT_ID] = h.object_id; out[selected::MATERIAL_ID] = h.material_id;
            out[selected::FIRST] = first; out[selected::COUNT] = end - first;
            if mode==mode::PROXY {out[selected::FIRST]=0.;out[selected::COUNT]=h.range[2];}
            out[selected::GROUP] = -1.;out[selected::MATERIAL_INDEX] = -1.;out[selected::FLAGS]=f as f64;
            if f&flags::MATERIAL_ARRAY!=0 && (mode==mode::SHADOW || mode==mode::LOCKED) {
                let base=(h.group_first as usize).min(self.groups.len());
                let count=(h.group_count as usize).min(self.groups.len().saturating_sub(base));
                let generation=h.generation;
                // WebGLShadowMap and the locked view mask emit groups in authored
                // order. Each range intersects the mesh range and index length.
                for ordinal in 0..count {
                    let g=self.groups[base+ordinal];
                    if !g.visible || g.owner!=id || g.generation!=generation{continue;}
                    let start=first.max(g.first).max(0.);let finish=end.min(g.first+g.count);
                    if finish<=start{continue;}
                    let mut draw=out;draw[selected::MATERIAL_ID]=g.material;
                    draw[selected::GROUP]=ordinal as f64;draw[selected::MATERIAL_INDEX]=g.material_index;
                    draw[selected::FIRST]=start;draw[selected::COUNT]=finish-start;
                    self.push(draw);
                }
            } else {self.push(out);}
        }
        for i in 0..self.selection.len() {
            self.scene_order[i * selected::STRIDE..(i + 1) * selected::STRIDE].copy_from_slice(&self.selection[i]);
        }
        // sort_unstable needs no scratch allocation. Original object identities
        // break depth ties; stable scene order is retained for shadow submission.
        if mode != mode::SCENE && mode != mode::SHADOW && mode != mode::FAR && mode!=mode::LOCKED && mode!=mode::MOTION_PREPARE {
            let nodes = &self.nodes;
            self.selection.sort_unstable_by(|a, b| compare(a, b, mode, nodes));
        }
        let limit = if families & family::NEAR != 0 && mode == mode::SCENE {
            crate::transparent::MAX_OBJECTS
        } else { self.output.len() / selected::STRIDE };
        // A consumer can have less room than the registry. Clip the ordered
        // prefix once, after sorting, so a full store cannot change which
        // surviving commands win equal-depth or equal-material ties.
        let requested=self.view[view::OUTPUT_LIMIT] as usize;
        let limit=if requested>0{limit.min(requested)}else{limit};
        self.selected = self.selection.len().min(limit).min(self.output.len() / selected::STRIDE);
        self.facts[header::CLIPPED]=self.selection.len().saturating_sub(self.selected) as f64;
        self.clipped += self.selection.len().saturating_sub(self.selected) as u64;
        for i in 0..self.selected {
            self.output[i * selected::STRIDE..(i + 1) * selected::STRIDE].copy_from_slice(&self.selection[i]);
        }
        // LinesPass.prepare runs after other families have reused output.
        // Retain this selected prefix so those later queries cannot overwrite
        // its order keys, ranges or opaque/transparent placement.
        if families&family::LINE!=0 {
            self.line_output[line_header::COUNT]=self.selected as f64;
            self.line_output[line_header::STRIDE..line_header::STRIDE+self.selected*selected::STRIDE]
                .copy_from_slice(&self.output[..self.selected*selected::STRIDE]);
        }
        self.selected
    }

    // NearPass.walk/ViewPass.visit topology is walked only after authored
    // state changes, once per root. Visits keep authored sibling order and
    // record each subtree's length so a policy can skip a pruned branch.
    // Group order facts and lazy-leaf bits are published as the walk goes.
    fn traverse(&mut self,master:usize,root:usize,generation:u32,motion:usize,near_mask:usize) {
        let mut visits=std::mem::take(&mut self.masters[master].visits);
        let mut lods=std::mem::take(&mut self.masters[master].lods);
        let mut lazy=std::mem::take(&mut self.masters[master].lazy);
        visits.clear();lods.clear();lazy.fill(0);
        self.stack.clear();
        self.stack.push((root, NO_PARENT, 0.));
        while let Some((id, parent, mut all_groups)) = self.stack.pop() {
            // Snapshot this node before LOD can change its children's state.
            // A hidden parent suppresses traversal, while a layer mismatch
            // still pushes children, matching projectObject's separate tests.
            let f = bits(self.nodes[id].data[row::FLAGS]);
            let families = bits(self.nodes[id].data[row::FAMILIES]);
            let motion_source=f&flags::MOTION_SOURCE!=0 && motion!=0;
            let lazy_leaf=motion==0 && self.nodes[id].first==NONE && families!=0;
            let mut visible=self.visible(id);
            if motion_source {visible=self.prepare_motion_source(id,motion==mode::MOTION_PREPARE);}
            if f & flags::ALIVE == 0 || !visible && !lazy_leaf { continue; }
            if lazy_leaf {
                lazy[id/32]|=1<<(id%32);
                if visible {self.authored[id/32]|=1<<(id%32);} else {self.authored[id/32]&=!(1u32<<(id%32));}
            }
            if motion==mode::MOTION_PREPARE && families&family::EXTRA!=0{self.motion_events[mh::EXTRAS]+=1.;}
            if f & flags::GROUP != 0 { all_groups = self.nodes[id].data[row::RENDER_ORDER]; }
            self.facts[header::STRIDE+id*fact::STRIDE+fact::GROUP_ORDER]=all_groups;
            // LOD.update is a near-list decision made while walking, so the
            // levels pushed below already reflect this camera's choice.
            if self.nodes[id].data[row::LOD_AUTO] != 0. {
                lods.push(id as u32);
                if near_mask!=0 && bits(self.nodes[id].data[row::LAYERS])&near_mask!=0 {self.update_lod(id);}
            }
            // Push siblings backwards to retain the original insertion order.
            // The retained order scratch avoids allocating a child vector per node.
            let ordinal=visits.len() as u32;
            let life=self.nodes[id].data[row::GENERATION];
            self.order.clear();
            let mut child = self.nodes[id].first;
            while child != NONE {
                if self.nodes[child].data[row::PARENT_GENERATION] == life { self.order.push(child); }
                child = self.nodes[child].next;
            }
            for &child in self.order.iter().rev() { self.stack.push((child, ordinal, all_groups)); }
            let n=&self.nodes[id];
            visits.push(Visit{id:id as u32,parent,subtree:0,layers:bits(n.data[row::LAYERS]) as u32,flags:f as u32,
                families:families as u32,lazy:lazy_leaf,render_order:n.data[row::RENDER_ORDER]});
        }
        // Children follow their parent, so a reverse pass totals each subtree.
        for i in (1..visits.len()).rev() {
            let parent=visits[i].parent as usize;let length=visits[i].subtree+1;
            visits[parent].subtree+=length;
        }
        self.visibility_visits=self.visibility_visits.saturating_add(visits.len() as u64);
        self.visibility_builds=self.visibility_builds.saturating_add(1);
        self.traversals=self.traversals.wrapping_add(1);
        let m=&mut self.masters[master];
        m.visits=visits;m.lods=lods;m.lazy=lazy;
        m.key=Some((root,generation));m.revision=self.visibility_revision;m.serial=self.traversals;
    }

    // One camera/family policy over the retained walk: layer matching, Group
    // order inheritance, near claimed-tree and far subtree-owner pruning.
    // Pruned branches are skipped by subtree length, never walked again.
    fn derive(&mut self,master:usize,cache:usize,key:ViewKey) {
        let mut draws=std::mem::take(&mut self.views[cache].draws);
        let mut visible=std::mem::take(&mut self.views[cache].visible);
        let mut groups=std::mem::take(&mut self.group_stack);
        draws.clear();visible.fill(0);groups.clear();
        let visits=&self.masters[master].visits;
        let mut i=0usize;
        while i<visits.len() {
            let v=visits[i];let f=v.flags as usize;let id=v.id as usize;
            while let Some(&(end,_))=groups.last() { if i as u32>=end {groups.pop();} else {break;} }
            let mut group=groups.last().map_or(0.,|g|g.1);
            let matches=key.motion==mode::MOTION_PREPARE || (v.layers as usize)&key.layers!=0;
            if f&flags::GROUP!=0 && (matches || key.all_groups) { group=v.render_order; }
            if matches {
                visible[id/32]|=1<<(id%32);
                let listed=v.families!=0 && ((v.families as usize)&key.families!=0 || key.receipts);
                if listed {draws.push(Draw{id:v.id,lazy:v.lazy,group});}
            }
            let pruned = key.far && matches && f & flags::SUBTREE_OWNER != 0 || key.near && f & flags::CLAIMED_TREE != 0;
            if pruned { i+=1+v.subtree as usize; }
            else {
                if v.subtree>0 {groups.push((i as u32+1+v.subtree,group));}
                i+=1;
            }
        }
        self.group_stack=groups;
        let view=&mut self.views[cache];
        view.draws=draws;view.visible=visible;view.key=Some(key);view.serial=self.masters[master].serial;
    }

    // LOD.update owns level visibility independently of authored flags. Motion
    // source walks consume the level left by the main colour selection too.
    fn visible(&self,id:usize)->bool {
        let node=&self.nodes[id];
        let lod=node.parent!=NONE && self.nodes[node.parent].data[row::LOD_AUTO]!=0. && node.data[row::LOD_INDEX]>=0.;
        if lod {node.lod_visible}else{bits(node.data[row::FLAGS])&flags::VISIBLE!=0}
    }

    // MotionVectors.shown checks the source's default layer, then every visible
    // ancestor through the requested scene. Layer-mismatched parents do not
    // prune it, and a detached hidden model stays pooled rather than expiring.
    fn source_visibility(&self,proxy:usize)->Option<bool> {
        let p=&self.nodes[proxy].data;
        let mut id=index(p[row::MOTION_SOURCE],self.nodes.len())?;
        if self.nodes[id].data[row::GENERATION]!=p[row::MOTION_SOURCE+1]{return None;}
        // A first-person body remains pooled on its hidden layer. The old
        // source walk returned before checking its scene membership there.
        if bits(self.nodes[id].data[row::LAYERS])&1==0{return Some(false);}
        // Bound the ancestor walk even if a foreign producer supplied stale
        // topology. Parent generations qualify every step, including the root.
        // A source outside the requested root has a different lifetime policy
        // from a hidden source still retained inside it.
        for _ in 0..self.nodes.len() {
            let node=&self.nodes[id];let f=bits(node.data[row::FLAGS]);
            if f&flags::ALIVE==0{return None;}
            if !self.visible(id){return Some(false);}
            if id==self.motion_root && node.data[row::GENERATION]==self.motion_root_generation{return Some(true);}
            let parent=node.parent;
            if parent==NONE || self.nodes[parent].data[row::GENERATION]!=node.data[row::PARENT_GENERATION]{return None;}
            id=parent;
        }
        None
    }
    // MotionVectors.prepare's distance policy runs before frustum selection:
    // even an off-screen source updates its history while it is within reach.
    // Lifecycle events are retained numeric output, never per-object exports.
    fn prepare_motion_source(&mut self,id:usize,advance:bool)->bool {
        let shown=self.source_visibility(id);
        // Only a visible source outside the requested scene ages toward
        // release. Hidden pool entries preserve their resource lifetime.
        if shown.is_none(){
            if advance {
                self.nodes[id].motion_detached=self.nodes[id].motion_detached.saturating_add(1);
                if self.nodes[id].motion_detached>600{self.motion_event(id,mk::RELEASE);}
            }
            return false;
        }
        if advance {self.nodes[id].motion_detached=0;}
        let source=self.nodes[id].data[row::MOTION_SOURCE] as usize;
        let world=self.world(source).unwrap_or(IDENTITY);let far=self.nodes[id].data[row::MOTION_FAR];
        let dx=world[12]-self.view[view::EYE];
        let dy=world[13]-self.view[view::EYE+1];
        let dz=world[14]-self.view[view::EYE+2];
        let visible=shown==Some(true) && dx*dx+dy*dy+dz*dz<far*far;
        if advance&&visible{let world=self.world(source).unwrap_or(IDENTITY);self.nodes[id].data[row::WORLD..row::WORLD+16].copy_from_slice(&world);self.transforms.import_world(id,&world,self.nodes[id].data[row::GENERATION]);self.refresh_bounds(id);}
        // Emit one history reset on entering hidden/far state. Repeated
        // paused queries neither grow a host queue nor resend that reset.
        if advance {
            if !visible && !self.nodes[id].motion_cleared{self.motion_event(id,mk::CLEAR);}
            self.nodes[id].motion_cleared = !visible;
        }
        visible
    }
    // MotionVectors.prepare formerly called release/reset per proxy.
    // A retained event block makes those lifecycle changes one phase result.
    // Its capacity is tied to nodes, including an entirely detached pool.
    fn motion_event(&mut self,id:usize,kind:usize) {
        let at=mh::STRIDE+self.motion_event_count*me::STRIDE;
        if at+me::STRIDE>self.motion_events.len(){self.clipped+=1;return;}
        self.motion_events[at+me::KIND]=kind as f64;self.motion_events[at+me::SLOT]=id as f64;
        self.motion_events[at+me::GENERATION]=self.nodes[id].data[row::GENERATION];
        self.motion_event_count+=1;self.motion_events[mh::EVENTS]=self.motion_event_count as f64;
    }

    // Selected commands never grow storage during a frame. Group-heavy meshes
    // can exceed node count, so even a pre-reserved scene needs this bound.
    // Keeping the earlier prefix matches the existing capped draw consumers.
    fn push(&mut self,row:[f64;selected::STRIDE]) {
        if self.selection.len()<self.selection.capacity(){self.selection.push(row);}else{self.clipped+=1;}
    }
}

fn index(value: f64, len: usize) -> Option<usize> {
    (value.is_finite() && value >= 0. && value.fract() == 0. && value < len as f64).then_some(value as usize)
}
// Layers.enableAll and bit 31 store signed JS masks. Preserve their low bits
// instead of saturating a negative f64 to zero during the native conversion.
fn bits(value: f64) -> usize { value as i64 as u32 as usize }
fn cmp(a: f64, b: f64) -> Ordering { a.partial_cmp(&b).unwrap_or(Ordering::Equal) }
// Each original producer had a distinct ordering contract: actor admission
// order, proxy material order, or Three's opaque/transparent painter order.
// Keep those contracts explicit instead of forcing every family through one
// comparator merely because they share the generated selection row.
fn compare(a: &[f64; selected::STRIDE], b: &[f64; selected::STRIDE], mode: usize, nodes: &[Node]) -> Ordering {
    if mode == mode::INSTANCE {
        return cmp(nodes[a[selected::SLOT] as usize].data[row::INSTANCE_ORDER], nodes[b[selected::SLOT] as usize].data[row::INSTANCE_ORDER])
            .then_with(|| cmp(a[selected::SLOT], b[selected::SLOT]));
    }
    if mode == mode::PROXY {
        return cmp(a[selected::RENDER_ORDER], b[selected::RENDER_ORDER])
            .then_with(|| cmp(a[selected::MATERIAL_ID], b[selected::MATERIAL_ID]))
            .then_with(|| cmp(a[selected::OBJECT_ID], b[selected::OBJECT_ID]));
    }
    let transparent = |a: &[f64; selected::STRIDE]| bits(nodes[a[selected::SLOT] as usize].data[row::FLAGS]) & flags::TRANSPARENT != 0;
    let ta = transparent(a); let tb = transparent(b);
    if mode == mode::VIEW && ta != tb { return ta.cmp(&tb); }
    let blended = mode == mode::TRANSPARENT || mode == mode::VIEW && ta;
    let group = if mode == mode::EXTRA { Ordering::Equal } else { cmp(a[selected::GROUP_ORDER], b[selected::GROUP_ORDER]) };
    group.then_with(|| cmp(a[selected::RENDER_ORDER], b[selected::RENDER_ORDER]))
        .then_with(|| if blended { Ordering::Equal } else { cmp(a[selected::MATERIAL_ID], b[selected::MATERIAL_ID]) })
        .then_with(|| if blended { cmp(b[selected::DEPTH], a[selected::DEPTH]) } else { cmp(a[selected::DEPTH], b[selected::DEPTH]) })
        .then_with(|| cmp(a[selected::OBJECT_ID], b[selected::OBJECT_ID]))
        .then_with(|| cmp(a[selected::SLOT], b[selected::SLOT]))
}

// NearPass.item and LinesPass.beforeSort use Vector4 with W kept through both
// multiplications. Combining the matrices, or dividing, changes ties and bits.
fn world_point(world: &Mat4, center: &[f64; 3], sprite: bool) -> [f64;4] {
    let [x, y, z] = *center;
    if sprite { [world[12], world[13], world[14], 1.] } else {
        [world[0]*x+world[4]*y+world[8]*z+world[12], world[1]*x+world[5]*y+world[9]*z+world[13],
         world[2]*x+world[6]*y+world[10]*z+world[14], world[3]*x+world[7]*y+world[11]*z+world[15]]
    }
}
fn clip_z(p:&[f64;4],screen:&Mat4)->f64 {
    screen[2]*p[0]+screen[6]*p[1]+screen[10]*p[2]+screen[14]*p[3]
}
fn vector3_depth(p:&[f64;3], screen: &Mat4) -> f64 {
    let inv = 1. / (screen[3]*p[0]+screen[7]*p[1]+screen[11]*p[2]+screen[15]);
    (screen[2]*p[0]+screen[6]*p[1]+screen[10]*p[2]+screen[14])*inv
}
