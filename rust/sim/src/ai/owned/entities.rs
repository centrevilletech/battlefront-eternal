//! Native AI identities. Object equality is independent of slab slots and of a
//! pooled object's gameplay life; weak references additionally carry that life.
use crate::{gen::{self, Store, AiLifeHandle}, store::{Ref, Slab}};
use crate::ai::vehicles::world::Obj;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    Unit(Ref<gen::Unit>), Hull(Ref<gen::Vehicle>), HullEntry(Ref<gen::Vehicle>),
    Turret(Ref<gen::Turret>), TurretEntry(Ref<gen::Turret>),
    Lease(Ref<gen::SeatLease>), Control(Ref<gen::SeatUnit>),
    Character(Ref<gen::Character>), Controller(Ref<gen::Controller>),
    Agent(Ref<super::agents::Identity>),
    Building(Ref<gen::Building>), BuildingService(Ref<gen::Building>),
    TurretService(Ref<gen::Turret>), ObjectiveEntry(Ref<gen::Building>),
    Post(Ref<gen::Post>), MobilePost(Ref<gen::MobilePost>),
    Hint(u32),
}
impl Entity {
    fn parts(self) -> (usize,u32,u32) {
        match self {
            Self::Unit(r)=>(0,r.slot,r.gen),Self::Hull(r)=>(1,r.slot,r.gen),
            Self::HullEntry(r)=>(2,r.slot,r.gen),Self::Turret(r)=>(3,r.slot,r.gen),
            Self::TurretEntry(r)=>(4,r.slot,r.gen),Self::Lease(r)=>(5,r.slot,r.gen),
            Self::Control(r)=>(6,r.slot,r.gen),Self::Character(r)=>(7,r.slot,r.gen),
            Self::Controller(r)=>(8,r.slot,r.gen),Self::Agent(r)=>(9,r.slot,r.gen),
            Self::Building(r)=>(10,r.slot,r.gen),Self::BuildingService(r)=>(11,r.slot,r.gen),
            Self::TurretService(r)=>(12,r.slot,r.gen),Self::ObjectiveEntry(r)=>(13,r.slot,r.gen),
            Self::Post(r)=>(14,r.slot,r.gen),Self::MobilePost(r)=>(15,r.slot,r.gen),
            Self::Hint(index)=>(16,index,0),
        }
    }
}

// This private identity slab uses insert/remove only: its generations start
// at one and never wrap to zero. Its qualified reference marks presence in
// three u32 words, without a discriminant or alignment word. The source's
// independent generation still keeps its full range, including zero.
#[derive(Clone, Copy, Default)]
struct Index {
    object: Ref<Entity>,
    generation: u32,
}
impl Index {
    fn read(&self) -> Option<(u32, Obj)> {
        if self.object.gen == 0 { return None; }
        Some((self.generation, object(self.object)))
    }
}

pub struct Entities {
    entries: Slab<Entity>,
    capacity: usize,
    indices: [Vec<Index>;17],
}
impl Entities {
    pub fn new(capacity: usize) -> Self {
        Self { entries:Slab::with_capacity(capacity), capacity, indices:std::array::from_fn(|_|vec![Index::default();capacity]) }
    }
    /// Construction/lifecycle operation. Hot reads below never insert or grow.
    pub fn register(&mut self, entity: Entity) -> Obj {
        if let Some(obj)=self.find(entity) { return obj; }
        let (kind,slot,generation)=entity.parts();
        let index=&mut self.indices[kind];
        if index.len()<=slot as usize { index.resize(slot as usize+1,Index::default()); }
        // Slot reuse retires its previous identity before the replacement is
        // exposed. A saved token cannot then name the replacement.
        if let Some((_,old))=index[slot as usize].read() { self.entries.remove(reference(old)); }
        let r=self.entries.insert(entity);
        let obj=object(r);
        index[slot as usize]=Index {generation,object:r}; obj
    }
    /// Runtime births use the battle's cold reservation. Replacing a storage
    /// generation releases its previous entry before consuming another slot.
    pub fn try_register(&mut self, entity: Entity) -> Option<Obj> {
        if let Some(object) = self.find(entity) { return Some(object); }
        let (kind, slot, _) = entity.parts();
        let previous = self.indices[kind].get(slot as usize)?;
        if previous.read().is_none() && self.entries.len() >= self.capacity { return None; }
        Some(self.register(entity))
    }
    pub fn find(&self, entity: Entity) -> Option<Obj> {
        let (kind,slot,generation)=entity.parts();
        let row=self.indices[kind].get(slot as usize)?;
        if row.object.gen==0 || row.generation!=generation {return None;}
        Some(object(row.object))
    }
    pub fn get(&self,obj:Obj)->Option<Entity> { self.entries.get(reference(obj)).copied() }
    /// classes.repairService compares the current lock with the wrapper, its
    /// building, or its vehicle. Missing objects never match one another.
    pub fn repair_matches(&self,target:Obj,current:Obj)->bool {
        let (Some(target_row),Some(current_row))=(self.get(target),self.get(current)) else{return false};
        target==current||match (target_row,current_row) {
            (Entity::HullEntry(a),Entity::Hull(b))=>a==b,
            (Entity::BuildingService(a)|Entity::ObjectiveEntry(a),Entity::Building(b))=>a==b,
            (Entity::TurretService(a)|Entity::TurretEntry(a),Entity::Turret(b))=>a==b,
            _=>false,
        }
    }
    pub fn retire(&mut self,obj:Obj)->bool {
        let Some(entity)=self.entries.remove(reference(obj)) else {return false};
        let (kind,slot,_)=entity.parts();
        if self.indices[kind][slot as usize].read().is_some_and(|(_,id)|id==obj) {self.indices[kind][slot as usize]=Index::default();}
        true
    }
    /// Build a command/weak-reference handle at its source observation point.
    pub fn life(&self,store:&Store,obj:Obj)->Option<AiLifeHandle> {
        let (kind,slot,generation,life)=match self.get(obj)? {
            Entity::Unit(r)=>(gen::KIND_UNIT,r.slot,r.gen,store.unit.get(r)?.generation),
            Entity::Hull(r)|Entity::HullEntry(r)=>(gen::KIND_VEHICLE,r.slot,r.gen,store.vehicle.get(r)?.target_generation),
            _=>return None,
        };
        Some(AiLifeHandle {kind:kind.into(),slot,generation,life})
    }
}
fn reference(obj:Obj)->Ref<Entity> { Ref::new(obj as u32,(obj>>32) as u32) }

fn object(r:Ref<Entity>)->Obj { (u64::from(r.gen)<<32)|u64::from(r.slot) }
