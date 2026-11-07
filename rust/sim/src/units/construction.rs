//! UnitFactory.build and PlayerBodies.adopt over prepared native bodies.
//! Preparation may allocate while content loads. Birth moves a complete row
//! into the canonical Store and assigns its registry id at that observation
//! point, so inactive classes never perturb ordered query ties.
use super::ownership::{Ownership, Stale};
use crate::{gen::{F3UnitKey, Store, Unit}, store::{Ref, StrId}};

/// A cold body also names its prepared rig, combat and mirror resources.
/// The cookie belongs to construction, independently of a future registry id.
pub struct Prepared {
    pub cookie: u32,
    pub source: Ref<Unit>,
    pub unit: Unit,
    pub local: Option<Ref<Unit>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Unprepared, Capacity, Admission, Stale(Stale) }

/// The coordinator activates these retained resources before publishing rows.
/// A warm acquisition has no birth; its existing facade and vectors survive.
pub struct Realized {
    pub key: F3UnitKey,
    pub prepared: Option<(u32, Ref<Unit>)>,
    pub replacement: bool,
}

#[derive(Default)]
pub struct Construction {
    ready: Vec<Prepared>,
    // Local class replacement keeps the discarded row until the outer return.
    // Dropping an arsenal during a nested lifecycle operation would invalidate
    // services that still refer to its prepared immutable class handles.
    retired: Vec<Unit>,
    limit: usize,
}

impl Construction {
    /// Reserve future births with the existing owner, not a parallel registry.
    /// `limit` bounds the complete mixed id namespace, including hull births.
    pub fn reserve(&mut self, store: &mut Store, owner: &mut Ownership, limit: usize) {
        assert!(limit >= store.units.next_id as usize);
        self.limit = self.limit.max(limit);
        self.ready.reserve(limit.saturating_sub(self.ready.len()));
        self.retired.reserve(limit.saturating_sub(self.retired.len()));
        store.unit.reserve(limit);
        store.units.list.reserve(limit.saturating_sub(store.units.list.len()));
        owner.reserve(limit);
    }

    /// Transfer a completed cold row once. It has no live registry member and
    /// cannot be acquired until all body and weapon services are prepared.
    pub fn prepare(&mut self, owner: &mut Ownership, body: Prepared) {
        // Preparation is cold; several classes may share the same future id
        // budget without reserving observable registry members for each one.
        owner.reserve_class(body.unit.class_name, self.limit);
        self.ready.push(body);
    }

    pub fn available(&self, class: StrId) -> usize {
        self.ready.iter().filter(|body| body.unit.class_name == class && body.local.is_none()).count()
    }
    pub fn can_acquire(&self, store:&Store, owner:&Ownership, class:StrId) -> bool {
        owner.available(class) || ((store.units.next_id as usize) < self.limit && self.available(class)>0)
    }

    /// A viewpoint's cached classes never enter another player's pool.
    pub fn local_available(&self, class:StrId, owner:Ref<Unit>) -> bool {
        self.ready.iter().any(|body|body.unit.class_name==class &&
            (body.local.is_none() || body.local==Some(owner)))
    }
    /// PlayerBodies keeps an inactive body's native state and class buffers.
    /// The old row is moved, never cloned or rebuilt when that class returns.
    pub fn recycle_local(&mut self, owner:&mut Ownership, cookie:u32, source:Ref<Unit>) {
        let unit=self.retired.pop().expect("outgoing local class row");
        self.prepare(owner,Prepared {cookie,source,unit,local:Some(source)});
    }

    /// UnitFactory.acquire keeps its original LIFO corpse pool. Only an empty
    /// pool consumes a cold body and appends one new mixed-registry member.
    /// UnitFactory admits AI identity before moving class stock or registry ids.
    /// Refusal leaves all resources available for the next construction attempt.
    pub fn acquire(&mut self, store:&mut Store, owner:&mut Ownership, class:StrId,
        mut admit:impl FnMut(Ref<Unit>)->bool)->Result<Realized,Error> {
        if let Some(reference)=owner.candidate(store,class) {
            if !admit(reference) {return Err(Error::Admission);}
        }
        if let Some(key) = owner.acquire(store, class) {
            return Ok(Realized {key, prepared: None, replacement: false});
        }
        if store.units.next_id as usize >= self.limit { return Err(Error::Capacity); }
        let at = self.ready.iter().rposition(|body| body.unit.class_name == class && body.local.is_none()).ok_or(Error::Unprepared)?;
        let reference=Ref::new(store.units.next_id as u32,1);
        if !admit(reference) {return Err(Error::Admission);}
        let prepared = self.ready.remove(at);
        let key = owner.build(store, prepared.unit);
        Ok(Realized {key, prepared: Some((prepared.cookie, prepared.source)), replacement: false})
    }

    /// PlayerBodies.adopt preserves the local viewpoint's Unit identity. A
    /// connection changing class instead acquires another pooled Unit, leaving
    /// its former body available to the caller's ordered removal operation.
    /// PlayerBodies also admits a replacement before retiring the old class.
    pub fn realize(&mut self,store:&mut Store,owner:&mut Ownership,class:StrId,
        previous:Option<F3UnitKey>,local:bool,mut admit:impl FnMut(Ref<Unit>)->bool)->Result<Realized,Error> {
        if let Some(key) = previous {
            let old = key.validate(store).map_err(Error::Stale)?;
            if old.class_name == class {
                if !admit(key.reference()) {return Err(Error::Admission);}
                return Ok(Realized {key, prepared: None, replacement: false});
            }
            if local {
                if self.retired.len() == self.retired.capacity() { return Err(Error::Capacity); }
                let at = self.ready.iter().rposition(|body| body.unit.class_name==class && body.local==Some(key.reference()))
                    .or_else(||self.ready.iter().rposition(|body|body.unit.class_name==class && body.local.is_none()))
                    .ok_or(Error::Unprepared)?;
                if !admit(key.reference()) {return Err(Error::Admission);}
                let mut prepared = self.ready.remove(at);
                // Class replacement is distinct from spawning. Its subsequent
                // Unit.spawn advances life and resets the new arsenal once.
                prepared.unit.generation = old.generation;
                prepared.unit.team = old.team;
                prepared.unit.combat.team = old.team;
                prepared.unit.human = old.human;
                prepared.unit.network_human = old.network_human;
                prepared.unit.character = old.character;
                prepared.unit.controller = None;
                self.retired.push(std::mem::replace(store.unit.get_mut(key.reference()).unwrap(), prepared.unit));
                owner.mark(key.reference());
                return Ok(Realized {key, prepared: Some((prepared.cookie, prepared.source)), replacement: true});
            }
        }
        self.acquire(store, owner, class,admit)
    }

    /// Called only after structural notices and their row readers finish.
    /// The retained list keeps capacity for the next class change.
    pub fn finish(&mut self) { self.retired.clear(); }
}
