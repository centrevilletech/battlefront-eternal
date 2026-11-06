//! Animated props work on scalar body frames backed by canonical Units. Only
//! membership is retained: each reader refreshes current life, position and
//! hold state, and each carry writes the owning Unit before another reader.
use super::animated::{Animated, Body, Held, Settings};
use crate::gen::{F3UnitKey, Held as UnitHeld, Member, Prop, Store, Unit, UnitBody};
use crate::store::{Nullable, Ref};
use three::{Matrix4, Vector3};

pub struct Bodies {
    pub frames: Vec<Body>,
    pub order: Vec<usize>,
    pub positions: Positions,
    stamp: Option<(u64, usize)>,
}
impl Bodies {
    pub fn new(unit_slots: usize, units: usize) -> Self {
        Self { frames: Vec::with_capacity(unit_slots), order: Vec::with_capacity(units),
            positions: Positions { members: Vec::with_capacity(units + 1) }, stamp: None }
    }
    pub fn adopt(&mut self, reference: Ref<Unit>) {
        let slot = reference.slot as usize;
        self.frames.resize_with(self.frames.len().max(slot + 1), Body::default);
        self.frames[slot].reference = Some(reference); self.frames[slot].key = slot;
        self.stamp = None;
    }
    /// Units.all returns its live array. Rebuild its integer traversal only
    /// after add/remove; ordinary body and gameplay-life changes need no copy.
    pub fn refresh_order(&mut self, s: &Store) {
        let stamp = (s.units.next_id.to_bits(), s.units.list.len());
        if self.stamp == Some(stamp) { return; }
        self.order.clear();
        for &member in &s.units.list {
            if let Member::Unit(r) = member {
                assert_eq!(self.frames[r.slot as usize].reference, Some(r), "prepared animated body");
                self.order.push(r.slot as usize);
            }
        }
        self.stamp = Some(stamp);
    }
}
/// Fixed phase membership with live coordinate reads. Kept separate from the
/// mutable working frames so a native World can lend it during each prop.
pub struct Positions { members: Vec<Ref<Unit>> }
impl Positions {
    /// LiveObjects' soldiers() includes the local viewpoint even while seated
    /// and every living factory soldier, including held or seated bodies.
    pub fn begin(&mut self, s: &Store, local: Option<Ref<Unit>>, built: &[Ref<Unit>]) {
        self.members.clear();
        if let Some(r) = local { if !s.unit.get(r).expect("animated viewpoint").combat.dead { self.members.push(r); } }
        for &r in built { if s.unit.get(r).expect("animated factory body").alive { self.members.push(r); } }
    }
    pub fn near(&self, s: &Store, settings: &Settings) -> bool {
        let Some(trigger) = settings.triggers.first() else { return false };
        self.members.iter().any(|&r| s.unit.get(r).is_some_and(|u|
            u.position().distance_to_squared(&trigger.point) <= trigger.radius * trigger.radius))
    }
}
fn vector(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn array(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }

/// A held body's grab belongs to its prop's victim record. Unit holds retain
/// the original prop reference and frame, rather than another mutable rig.
pub fn refresh(s: &Store, prop: &Animated, original: Ref<Prop>, body: &mut Body) {
    let Some(r) = body.reference else { return };
    let Some(u) = s.unit.get(r) else { body.removed = true; return };
    body.id = r.slot as f64; body.generation = u.generation; body.soldier = true;
    body.walker = u.walker; body.removed = u.removed; body.alive = u.alive; body.dead = u.dead;
    body.seated = u.seat.is_some(); body.combat = true; body.mounted = u.combat.mounted.unwrap_or(false);
    body.position = u.position(); body.velocity = u.velocity(); body.tick_count = Some(u.tick_count);
    body.let_go = true; body.remove = true;
    body.held = match &u.held {
        Nullable::Value(h) => Some(Held { prop: if h.prop == original { prop.index } else { usize::MAX },
            grab: prop.victim.as_ref().filter(|v| v.unit == body.key && v.generation == u.generation).map_or(0, |v| v.grab),
            since: h.since, frame: match h.frame { Nullable::Value(elements) => { let mut m = Matrix4::default(); m.elements = elements; Some(m) }, _ => None }, time: h.time }),
        _ => None,
    };
    if let UnitBody::Soldier(s) = &u.soldier {
        body.previous = Some(vector(s.previous)); body.settled = Some(vector(s.settled)); body.air = s.air.is_some(); body.dive = s.dive;
    } else { body.previous = None; body.settled = None; body.air = false; body.dive = 0.0; }
}
/// Seize and carry are qualified native writes. A retired or replaced body
/// cannot receive a stale working frame. No lazy shape query runs here: only
/// carry's existing tick-count increment invalidates the Unit's shape cache.
pub fn commit(s: &mut Store, original: Ref<Prop>, body: &Body) -> Option<F3UnitKey> {
    let r = body.reference?;
    let u = s.unit.get_mut(r).filter(|u| u.generation == body.generation)?;
    u.held = body.held.as_ref().map_or(Nullable::Null, |h| Nullable::Value(UnitHeld { prop: original, since: h.since,
        frame: h.frame.as_ref().map_or(Nullable::Undefined, |m| Nullable::Value(m.elements)), time: h.time }));
    u.combat.mounted = Some(body.mounted);
    if let UnitBody::Soldier(s) = &mut u.soldier {
        s.position = array(body.position); s.velocity = array(body.velocity);
        if let Some(p) = body.previous { s.previous = array(p); }
        if let Some(p) = body.settled { s.settled = array(p); }
        if !body.air { s.air = None; } s.dive = body.dive;
    }
    if let Some(tick) = body.tick_count { u.tick_count = tick; }
    Some(F3UnitKey::of(r, u))
}
