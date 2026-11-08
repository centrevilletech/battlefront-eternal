//! PredictedShots: cosmetic shots have a private clock and identity bank.
//! They never enter gameplay projectile storage or spend authoritative ammo.
//! A frame publishes swept segments, accepts host collision observations, and
//! finishes all flight records together. Resource objects stay on the page.
use crate::gen::{ClientShotInput as Input, ClientShotRecord as Record, ClientShotReply as Reply,
    ClientShotContact as Contact, ClientShotOp as Op};
#[path="shot_host.rs"] pub mod host;
use three::{Vector3, Quaternion};
use v8math::{min, max};

// PredictedShots.slots capped speculative flights at 32. Claims outlive the
// visible flights and need their own ring, as a terminal can arrive after a
// cosmetic expires. Weapon ids are opaque page resource identities.
pub const CAPACITY: usize = crate::gen::CLIENT_SHOTS_CAPACITY as usize;
const CLAIMS: usize = crate::gen::CLIENT_SHOTS_CLAIMS as usize;
const WEAPONS: usize = crate::gen::CLIENT_SHOTS_WEAPONS as usize;
const OUTPUT: usize = Reply::WORDS + CAPACITY * Record::WORDS;
const INPUT: usize = Input::WORDS + CAPACITY * Contact::WORDS;
fn v(a: [f64; 3]) -> Vector3 { Vector3::new(a[0], a[1], a[2]) }
fn a(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn truthy(n: f64) -> bool { n != 0.0 && !n.is_nan() }
fn or(n: f64, default: f64) -> f64 { if truthy(n) { n } else { default } }

#[derive(Clone, Copy, Default)]
struct Claim { occupied: bool, player: f64, generation: f64, channel: f64, fire: f64, tick: f64, id: u32 }
impl Claim {
    // claim's pressed identity ignores the arrival tick; held fire uses it.
    fn matches(&self, i: &Input) -> bool { self.occupied && self.player == i.player_id && self.generation == i.generation
        && self.channel == i.channel && self.fire == i.fire_id && (truthy(i.fire_id) || self.tick == i.tick) }
}
// A correction must never replace a locally established firing deadline.
// Resolving resource identity separately also keeps Weapon objects out of
// native state and prevents prediction from writing the authority's weapon.
#[derive(Clone, Copy, Default)]
struct Cooldown { weapon: u32, deadline: f64 }
#[derive(Clone, Default)]
struct Shot { record: Record, fade_time: f64, length: f64, grouped: bool, epoch: f64 }

// Exchange storage is reserved at admission. Run only reuses fixed rows and
// stack iterators; a paused page can receive arbitrarily many observations
// without growing a flight or claim store on its eventual drawing frame.
pub struct Shots {
    host: host::Host,
    rows: [Option<Shot>; CAPACITY], claims: [Claim; CLAIMS], cooldowns: [Cooldown; WEAPONS],
    claim_next: usize, serial: u32, life: Option<(f64, f64, f64)>,
    pub input: Vec<f64>, pub output: Vec<f64>, heap: Vec<f64>,
}
impl Default for Shots {
    fn default() -> Self { Self { host: host::Host::default(), rows: std::array::from_fn(|_| None), claims: [Claim::default(); CLAIMS],
        cooldowns: [Cooldown::default(); WEAPONS], claim_next: 0, serial: 0, life: None,
        input: vec![0.0; INPUT], output: vec![0.0; OUTPUT], heap: Vec::new() } }
}
impl Shots {
    fn count(&self) -> usize { self.rows.iter().filter(|s| s.is_some()).count() }
    // PredictedShots.clear invalidates every life-qualified association, while
    // retaining the native serial so an old resource mirror is never reused.
    // A new epoch with recycled player and generation numbers still resets.
    fn clear(&mut self) {
        self.rows.fill(None); self.claims.fill(Claim::default()); self.cooldowns.fill(Cooldown::default());
        self.claim_next = 0; self.life = None;
    }
    fn own(&self, i: &Input) -> bool { self.life.is_some_and(|(_, p, g)| p == i.player_id && g == i.generation) }
    // PredictedShots.fire checks the initial authority cooldown only until a
    // weapon has a local deadline. Corrections describe older server ticks.
    fn prepare(&mut self, i: &Input, reply: &mut Reply) {
        if !i.pressed || !i.alive || i.dead { return; }
        let life = (i.epoch, i.player_id, i.generation);
        if self.life != Some(life) { self.clear(); self.life = Some(life); reply.reset = true; }
        if self.count() == CAPACITY || i.flight_blocked { return; }
        if !i.mounted && !i.replayed && (i.underwater || i.air_blocked || i.dive || i.lock > 0.0 || i.rolled ||
            i.prone && max(i.forward.abs(), i.right.abs()) > 0.1) { return; }
        // This resource cache exceeds the battle's complete authored arsenal.
        // An excessive stream of new resources evicts only an old deadline;
        // it cannot overrun storage or panic in an input or drawing phase.
        let cd = self.cooldowns[i.weapon as usize % WEAPONS];
        let ready = (cd.weapon == i.weapon && i.weapon != 0).then_some(cd.deadline);
        if !i.has_ordnance || !i.replayed && (i.thrown || i.charging || i.levels > 1 || i.busy || i.reloading || !i.can_shoot || ready.is_some_and(|r| r > i.now + 1e-6)) { return; }
        // Replayed Weapon notices already passed eligibility and spent their
        // private ammunition. Testing that spent state would suppress pellets.
        // Held single-shot and salvo inputs never manufacture extra rounds.
        if !i.replayed && (if i.held { i.single > 0.0 || i.salvo > 1.0 } else { ready.is_none() && i.wait > 1e-5 }) { return; }
        if i.excluded { return; }
        reply.accepted = true;
        reply.direction = [v8math::sin(i.yaw) * v8math::cos(i.pitch), v8math::sin(i.pitch), v8math::cos(i.yaw) * v8math::cos(i.pitch)];
        reply.speed = max(1.0, min(2000.0, or(i.velocity, 100.0)));
        reply.span = min(1.0, or(i.span, 1.0));
    }
    // PredictedShots.fire resumes after the host has sampled its muzzle and
    // static cast. Convergence and spread retain Three's operation order.
    fn launch(&mut self, i: &Input, reply: &mut Reply) {
        if self.life != Some((i.epoch, i.player_id, i.generation)) { return; }
        if self.count() == CAPACITY { return; }
        let mut direction = v(i.direction);
        if i.converge {
            let mut converged = v(i.target); converged.add_scaled_vector(&direction,i.target_distance); converged.sub(&v(i.origin)).normalize();
            if converged.x.is_finite() && converged.y.is_finite() && converged.z.is_finite() && converged.dot(&direction) > 0.0 { direction = converged; }
        }
        if i.replayed {
            let mut q = Quaternion::default(); q.set_from_axis_angle(&Vector3::new(0.0,1.0,0.0), or(i.spread_yaw,0.0)); direction.apply_quaternion(&q);
            let mut side = Vector3::new(direction.z,0.0,-direction.x); side.normalize();
            q.set_from_axis_angle(&side,-or(i.spread_pitch,0.0)); direction.apply_quaternion(&q);
            if truthy(i.elevate) { direction.y += i.elevate; }
        }
        if !direction.x.is_finite() || !direction.y.is_finite() || !direction.z.is_finite() { return; }
        direction.normalize();
        // The host's static query is a continuation over this native aim.
        // A NaN contact requests the ray and does not create a shot yet.
        reply.direction = a(direction);
        reply.speed = max(1.0,min(2000.0,or(i.velocity,100.0))); reply.span = min(1.0,or(i.span,1.0));
        if i.contact.is_nan() { reply.accepted = true; return; }
        // The speculative lifetime is bounded to a second independently of
        // the class lifetime. The static surface receipt can only shorten it;
        // a confirmed authority record never rewinds a visible muzzle streak.
        // Map iteration draws surviving shots in birth order. Reclaim holes
        // without letting a new muzzle flash jump ahead of an older tracer.
        // Moving fixed records here allocates nothing on a frame boundary.
        let mut slot=0;
        for read in 0..CAPACITY {
            if self.rows[read].is_some() { self.rows.swap(slot,read);slot+=1; }
        }
        let speed = reply.speed; let life = min(reply.span,i.contact/speed);
        self.serial = self.serial.wrapping_add(1).max(1);
        let grouped = i.replayed && i.tick.is_finite();
        let fire_id = if i.held { 0.0 } else { i.fire_id };
        // A volley owns one timing notice but retains all of its pellets.
        // Group membership uses the firing tick, not its confirmation tick.
        // Separate channels may share a class without sharing a claim.
        reply.first = !grouped || !self.rows.iter().flatten().any(|s| s.grouped && s.epoch == i.epoch && s.record.tick == i.tick && s.record.channel == i.channel && s.record.fire_id == fire_id);
        let mut offset = v(i.muzzle); let has_offset = i.has_muzzle && offset.distance_to_squared(&v(i.origin)) <= 400.0;
        if has_offset { offset.sub(&v(i.origin)); }
        let record = Record { id: self.serial, player_id: i.player_id, generation: i.generation, channel: i.channel,
            fire_id, tick: i.tick, confirmed: f64::NAN, straight: i.straight, held: i.held,
            origin: i.origin, direction: a(direction), position: i.origin, offset: a(offset), has_offset,
            speed, life, born: i.now, until: i.now + max(1.0,life+or(i.fade_time,0.0)), merge: max(min(8.0,speed*life),1e-3),
            flying: i.straight, fade: if i.straight {1.0} else {0.0}, ..Record::default() };
        // Repeated ordinary presses replace the same Map identity, as before.
        // A replay's duplicate press is a pellet and gets an independent row.
        // Existing claim ids continue naming their original resource mirror.
        if !i.replayed && !i.held {
            if let Some(old)=self.rows.iter().position(|old| old.as_ref().is_some_and(|s|
                !s.record.held && s.record.player_id==i.player_id && s.record.generation==i.generation &&
                s.record.channel==i.channel && s.record.fire_id==i.fire_id)) {slot=old;}
        }
        self.rows[slot] = Some(Shot { record, fade_time: i.fade_time, length: max(max(or(i.laser,1.0),or(i.glow,0.0)),or(i.blur,0.0)), grouped, epoch: i.epoch });
        let delay = max(0.04,or(i.delay,0.1)); let cd = &mut self.cooldowns[i.weapon as usize % WEAPONS];
        // Keep a held weapon's fractional remainder between sampled rounds.
        // A long stall emits one round with a fresh deadline, never a burst
        // of catch-up cosmetics on the first frame after the pause.
        let deadline = if i.held && cd.weapon == i.weapon && i.now-cd.deadline < delay { cd.deadline+delay } else { i.now+delay };
        *cd = Cooldown { weapon: i.weapon, deadline };
        reply.id = self.serial; reply.accepted = true;
    }
    // PredictedShots.confirm consumes an exact press or the oldest unclaimed
    // held shot of that channel. Multi-pellet groups confirm together.
    fn confirm(&mut self, i: &Input, reply: &mut Reply) {
        let found = self.rows.iter().flatten().filter(|s| s.record.player_id == i.player_id && s.record.generation == i.generation && s.record.channel == i.channel)
            .filter(|s| if i.fire_id == 0.0 { s.record.held && s.record.confirmed.is_nan() } else { !s.record.held && s.record.fire_id == i.fire_id })
            .next();
        let Some(found) = found else { return };
        let id = found.record.id; let grouped = found.grouped; let epoch = found.epoch; let tick = found.record.tick;
        let channel = found.record.channel; let fire_id = found.record.fire_id;
        reply.first = found.record.confirmed.is_nan(); reply.id = id; reply.accepted = true;
        for shot in self.rows.iter_mut().flatten() {
            if shot.record.id == id || grouped && shot.grouped && shot.epoch == epoch && shot.record.tick == tick && shot.record.channel == channel && shot.record.fire_id == fire_id {
                if i.tick.is_nan() { shot.record.confirmed = f64::NEG_INFINITY; shot.record.flying = false; }
                else if shot.record.confirmed.is_nan() || i.tick < shot.record.confirmed { shot.record.confirmed = i.tick; }
            }
        }
    }
    fn claim(&mut self, i: &Input, reply: &mut Reply) {
        if !self.own(i) { return; }
        if i.tick.is_nan() { self.confirm(i,reply); return; }
        // Remote combat does not enter this FIFO. Otherwise busy servers could
        // evict an own shot's association before its later sound or terminal.
        // Cache misses as well, so repeated copies make the same decision.
        if let Some(c) = self.claims.iter().find(|c| c.matches(i)) { reply.id = c.id; reply.accepted = c.id != 0; return; }
        self.confirm(i,reply);
        self.claims[self.claim_next] = Claim { occupied: true, player: i.player_id, generation: i.generation, channel: i.channel, fire: i.fire_id, tick: i.tick, id: reply.id };
        self.claim_next = (self.claim_next+1)%CLAIMS;
    }
    // PredictedShots.stop cuts the flight at the authoritative terminal point.
    // Confirmed claims survive visual expiry; a reused flight slot never does.
    fn stop(&mut self, i: &Input) {
        if !self.own(i) { return; }
        let Some(c) = self.claims.iter().find(|c| c.matches(i)) else { return };
        let Some(shot) = self.rows.iter_mut().flatten().find(|s| s.record.id == c.id && s.record.straight) else { return };
        let r = &mut shot.record; let mut delta = v(i.target); delta.sub(&v(r.origin));
        r.life = min(r.life,max(0.0,delta.dot(&v(r.direction)))/r.speed);
    }
    // PredictedShots.update first computes every sweep. Expiry happens before
    // collision, and the fade value uses life before this frame's unit hit.
    fn advance(&mut self, now: f64) {
        for slot in &mut self.rows {
            let Some(s) = slot else { continue }; let r = &mut s.record;
            if r.confirmed == f64::NEG_INFINITY || now >= r.until { *slot = None; continue; }
            r.notify = false; r.cast_far = 0.0;
            // Fade is sampled before this frame's new unit contact. This is
            // observable for sniper streaks at the instant a unit stops them.
            // The following frame uses the shortened life normally.
            let age = now-r.born; let after = age-r.life;
            let fade = if !r.straight { 0.0 } else if after < 0.0 { 1.0 } else if s.fade_time > 0.0 { max(0.0,1.0-after/s.fade_time) } else { 0.0 };
            r.flying = fade > 0.0;
            if !r.flying { continue; }
            let from = r.flown; r.flown = min(age,r.life)*r.speed;
            let mut p = v(r.origin); p.add_scaled_vector(&v(r.direction),from); r.cast_from = a(p);
            if r.flown > from { r.cast_far = r.flown-from; }
            r.fade = fade;
        }
    }
    // The continuation accepts at most the fixed shot capacity and pairs hits
    // by native identity. Reordered or stale responses cannot cut another shot.
    fn settle(&mut self, now: f64, contacts: &[f64]) {
        for s in self.rows.iter_mut().flatten() {
            let r = &mut s.record; if !r.flying { continue; }
            for words in contacts.chunks_exact(Contact::WORDS).take(CAPACITY) {
                let hit = Contact::take(words,&[]);
                if hit.id == r.id && hit.distance.is_finite() && r.cast_far > 0.0 {
                    let from = r.flown-r.cast_far;
                    r.flown = from+hit.distance; r.life = min(r.life,r.flown/r.speed);
                    if !r.struck { r.struck = true; r.notify = true; }
                    break;
                }
            }
            r.drawn = min(r.flown,s.length); r.ended = now-r.born >= r.life;
            let mut p = v(r.origin); p.add_scaled_vector(&v(r.direction),r.flown); r.position = a(p);
            r.cast_far = 0.0;
        }
    }
    // All commands publish one bounded block keyed by native shot identity.
    // Advance and settle are continuations around the host's resource query;
    // neither continuation delegates timing or flight arithmetic to the host.
    pub fn run(&mut self) {
        let i = Input::take(&self.input[..Input::WORDS],&[]); let mut reply = Reply::default();
        match i.op {
            Op::Prepare => self.prepare(&i,&mut reply), Op::Launch => self.launch(&i,&mut reply),
            Op::Confirm => self.confirm(&i,&mut reply), Op::Claim => self.claim(&i,&mut reply), Op::Stop => self.stop(&i),
            Op::Advance => self.advance(i.now),
            Op::Settle => { let words = std::mem::take(&mut self.input); self.settle(i.now,&words[Input::WORDS..]); self.input = words; },
            Op::Clear => self.clear(), Op::Slots => {}, Op::Remove => { for row in &mut self.rows { if row.as_ref().is_some_and(|s| s.record.id == i.id) { *row = None; break; } } },
        }
        reply.count = self.count() as u32;
        reply.slots = if self.life == Some((i.epoch,i.player_id,i.generation)) { (CAPACITY-self.count()) as u32 } else { CAPACITY as u32 };
        reply.put(&mut self.output[..Reply::WORDS],&mut self.heap);
        for (n,s) in self.rows.iter().flatten().enumerate() { let at = Reply::WORDS+n*Record::WORDS; s.record.put(&mut self.output[at..at+Record::WORDS],&mut self.heap); }
    }
}

// The battle's page bank owns the session. Both targets compile the same phase;
// WASM only translates its retained exchange buffers into linear addresses.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle] pub extern "C" fn client_shots_create() -> usize { crate::abi::facade::world().page.shots.insert(Shots::default()).unwrap_or(0) }
    #[no_mangle] pub extern "C" fn client_shots_input(id: usize) -> *mut f64 { crate::abi::facade::world().page.shots.get_mut(id).map_or(std::ptr::null_mut(),|s| s.input.as_mut_ptr()) }
    #[no_mangle] pub extern "C" fn client_shots_run(id: usize) -> *const f64 {
        let Some(s) = crate::abi::facade::world().page.shots.get_mut(id) else { return std::ptr::null() }; s.run(); s.output.as_ptr()
    }
    #[no_mangle] pub extern "C" fn client_shots_drop(id: usize) { crate::abi::facade::world().page.shots.remove(id); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn invoke(s: &mut Shots, i: &Input) -> Reply {
        i.put(&mut s.input[..Input::WORDS],&mut Vec::new()); s.run(); Reply::take(&s.output[..Reply::WORDS],&[])
    }
    fn input() -> Input { Input { op: Op::Prepare, epoch: 1.0, player_id: 8.0, generation: 2.0,
        weapon: 1, pressed: true, alive: true, has_ordnance: true, can_shoot: true, straight: true,
        velocity: 100.0, span: 0.3, fire_id: 1.0, tick: f64::NAN, contact: f64::INFINITY,
        ..Input::default() } }
    fn fire(s: &mut Shots, i: &mut Input) -> Reply {
        i.op=Op::Prepare; let r=invoke(s,i); if !r.accepted { return r; }
        i.direction=r.direction;i.op=Op::Launch;invoke(s,i)
    }
    #[test]
    fn shot_handles_never_resolve_as_network_mailboxes() {
        // The page and the input owner share a battle but not a resource bank.
        // Their first slots must remain distinct even with equal generations.
        let mut strings=crate::store::Strings::default();
        let mut network=crate::input::owner::Owner::new();
        let mailbox=crate::input::owner::Mailbox::new(1.0,0.0,Vec::new(),&mut strings).unwrap();
        let inbox=network.mailboxes.insert(mailbox).unwrap();
        let mut page=crate::client::resources::Resources::new();
        let shots=page.shots.insert(Shots::default()).unwrap();
        assert_ne!(inbox,shots);
        assert!(page.shots.get(inbox).is_none());
        assert!(network.mailboxes.get(shots).is_none());
    }
    #[test]
    fn whole_phase_owns_cadence_claims_and_expiry() {
        let mut s=Shots::default();let mut i=input();let first=fire(&mut s,&mut i);assert!(first.accepted);
        i.now=0.05;assert!(!fire(&mut s,&mut i).accepted);
        i.now=0.1;i.held=true;i.fire_id=0.0;let held=fire(&mut s,&mut i);assert!(held.accepted);
        i.op=Op::Claim;i.tick=20.0;assert_eq!(invoke(&mut s,&i).id,held.id);assert_eq!(invoke(&mut s,&i).id,held.id);
        i.tick=21.0;assert_eq!(invoke(&mut s,&i).id,0);
        i.fire_id=1.0;i.tick=19.0;assert_eq!(invoke(&mut s,&i).id,first.id);
        i.op=Op::Advance;i.now=0.2;invoke(&mut s,&i);i.op=Op::Settle;invoke(&mut s,&i);
        let r=Record::take(&s.output[Reply::WORDS..Reply::WORDS+Record::WORDS],&[]);assert_eq!(r.position,[0.0,0.0,20.0]);
        i.op=Op::Advance;i.now=1.2;assert_eq!(invoke(&mut s,&i).count,0);
        i.op=Op::Claim;i.fire_id=0.0;i.tick=20.0;assert_eq!(invoke(&mut s,&i).id,held.id);
    }
    #[test]
    fn reclaimed_slots_keep_the_original_tracer_draw_order() {
        // A later, longer-lived bolt survives the first one's resource expiry.
        // The next launch must draw after that survivor even when slot zero is free.
        let mut s=Shots::default();let mut i=input();let first=fire(&mut s,&mut i);
        i.now=0.5;i.fire_id=2.0;i.span=1.0;let second=fire(&mut s,&mut i);
        i.op=Op::Advance;i.now=1.1;assert_eq!(invoke(&mut s,&i).count,1);
        i.op=Op::Settle;invoke(&mut s,&i);
        i.fire_id=3.0;let third=fire(&mut s,&mut i);
        let old=Record::take(&s.output[Reply::WORDS..Reply::WORDS+Record::WORDS],&[]);
        let new=Record::take(&s.output[Reply::WORDS+Record::WORDS..Reply::WORDS+2*Record::WORDS],&[]);
        assert_eq!(old.id,second.id);assert_eq!(new.id,third.id);assert_ne!(old.id,first.id);
        assert!(old.position[2]>0.0);assert_eq!(new.position,[0.0,0.0,0.0]);
    }
    #[test]
    fn paused_bursts_saturate_without_allocation_or_cross_identity_hits() {
        let mut s=Shots::default();let mut i=input();i.replayed=true;i.tick=10.0;
        let input_ptr=s.input.as_ptr();let output_ptr=s.output.as_ptr();
        for _ in 0..CAPACITY { assert!(fire(&mut s,&mut i).accepted); }
        for _ in 0..4096 { assert!(!fire(&mut s,&mut i).accepted); }
        assert_eq!(s.count(),CAPACITY);assert_eq!(s.input.as_ptr(),input_ptr);assert_eq!(s.output.as_ptr(),output_ptr);
        i.op=Op::Claim;i.tick=20.0;let first=invoke(&mut s,&i);assert!(first.first);
        assert!(s.rows.iter().flatten().all(|s|s.record.confirmed==20.0));
        // Remote traffic must not evict a local claim or advance its FIFO.
        i.player_id=9.0;for n in 0..4096 { i.tick=n as f64;invoke(&mut s,&i); }
        i.player_id=8.0;i.tick=20.0;assert_eq!(invoke(&mut s,&i).id,first.id);
        i.op=Op::Advance;i.now=0.1;invoke(&mut s,&i);
        Contact{id:u32::MAX,distance:0.0}.put(&mut s.input[Input::WORDS..Input::WORDS+Contact::WORDS],&mut Vec::new());
        i.op=Op::Settle;invoke(&mut s,&i);assert!(s.rows.iter().flatten().all(|s|s.record.flown==10.0));
    }
}
