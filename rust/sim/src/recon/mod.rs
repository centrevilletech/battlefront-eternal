//! World-owned RemoteDroids flight and connection-scoped recon lifecycle.
//! Native services operate on committed rows: a death explosion can recurse
//! through the world without holding a droid or weapon borrow. Page replay is
//! deliberately separate in client::recon.
use crate::combat::{
    defs::WeaponDefs,
    weapons::{self, Admission, Aim, Arsenal, ArsenalInput, Shot, ShotKind, Trigger, WeaponDraws},
};
use crate::gen::{F3UnitKey, OwnerSnapshot, Posture, ReconContext, ReconDroid, ReconSalvo, Store};
use crate::store::{DefId, Ref};
use jsrt::rng::ClassicRandom;
use three::{Quaternion, Triangle, Vector3};
use v8math::{cos, hypot, max, min, sin};

mod camera;
mod defs;
pub(crate) mod module;

pub const RADIUS: f64 = 0.271;
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
fn v(a: [f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn a(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    max(lo, min(hi, x))
}
fn same(a: &F3UnitKey, b: &F3UnitKey) -> bool {
    a.slot == b.slot && a.generation == b.generation && a.life == b.life
}
fn key(k: &F3UnitKey) -> F3UnitKey {
    F3UnitKey {
        slot: k.slot,
        generation: k.generation,
        life: k.life,
    }
}

/// Numeric definitions are prepared at load, including each weapon level's
/// ordnance and destruct strike. No property or string lookup occurs in flight.
#[derive(Clone)]
pub struct DroidDef {
    pub health: f64,
    pub add_health: f64,
    pub altitude: f64,
    pub gravity: f64,
    pub spring: f64,
    pub damp: f64,
    pub acceleration: f64,
    pub speed: f64,
    pub strafe: f64,
    pub turn: f64,
    pub pitch: [f64; 2],
    pub threshold: [f64; 2],
    pub collision_scale: f64,
    pub center: [f64; 3],
    pub offset: [f64; 3],
    pub tilt: f64,
    pub eye: [f64; 3],
    pub muzzle: [f64; 3],
    pub third_fov: f64,
    pub first_fov: f64,
    pub explosion: Option<u32>,
    pub loadout: Vec<DefId>,
}
impl Default for DroidDef {
    fn default() -> Self {
        Self {
            health: 40.0,
            add_health: 0.0,
            altitude: 1.75,
            gravity: 1.0,
            spring: 5.0,
            damp: 2.0,
            acceleration: 70.0,
            speed: 9.0,
            strafe: 7.0,
            turn: 2.0,
            pitch: [-50.0, 50.0],
            threshold: [7.5, 2.5],
            collision_scale: 10.0,
            center: [0.0, 0.5, 0.0],
            offset: [0.0, -0.25, 1.5],
            tilt: 5.0,
            eye: [0.0, 0.5, 0.0],
            muzzle: [0.0; 3],
            third_fov: 48.0 * (std::f64::consts::PI / 180.0),
            first_fov: 48.0 * (std::f64::consts::PI / 180.0),
            explosion: None,
            loadout: Vec::new(),
        }
    }
}
#[derive(Clone, Copy)]
pub struct StrikeDef {
    pub ordnance: u32,
    pub elevation: f64,
    pub azimuth: f64,
    pub count: f64,
    pub delay: f64,
    pub scatter: f64,
    pub distance: f64,
}
#[derive(Clone, Copy)]
pub struct WeaponLevel {
    pub ordnance: Option<u32>,
    pub strike: Option<u32>,
    pub pressed_time: f64,
    pub zoom_min: f64,
    pub zoom_max: f64,
}
impl Default for WeaponLevel {
    fn default() -> Self {
        Self {
            ordnance: None,
            strike: None,
            pressed_time: 3.0,
            zoom_min: 1.0,
            zoom_max: 1.0,
        }
    }
}
#[derive(Default)]
// RemoteDroids content is resolved outside authoritative updates.
// Droid classes name loadouts by numeric weapon handles.
// Each weapon level names already-prepared ordnance and strike policy.
// A launch never traverses the original nested class objects.
// Definitions can therefore be borrowed through recursive world services.
// They are not duplicated for every physical droid or owner.
pub struct Definitions {
    pub droids: Vec<DroidDef>,
    pub strikes: Vec<StrikeDef>,
    pub weapons: WeaponDefs,
    pub levels: Vec<Vec<WeaponLevel>>,
}
#[derive(Clone, Copy, Default)]
pub struct Input {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub exit: bool,
    pub zoom: bool,
    pub primary: bool,
    pub held: bool,
    pub secondary: bool,
}
#[derive(Clone, Copy)]
pub struct Hit {
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
}
/// These are internal Rust world operations, never JS callbacks. Blast/fire
/// receive ownership explicitly so immediate nested damage observes live rows.
pub trait World {
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit>;
    fn static_cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit>;
    fn gather(&mut self, center: Vector3, radius: f64, out: &mut Vec<Triangle>);
    fn water(&self, x: f64, z: f64) -> Option<f64>;
    /// createDroid captures an immutable launch owner before any projectile
    /// notice can name it. The production world also emits its creation notice.
    /// RemoteDroids.control consumes edges before flight, while shoot takes
    /// fire edges only if its piloted droid survives collision this tick.
    fn control_input(&mut self, _store: &mut Store, _context: Ref<ReconContext>, input: Input) -> Input { input }
    fn weapon_input(&mut self, _store: &mut Store, _context: Ref<ReconContext>, input: Input) -> Input { input }
    fn capture_owner(&mut self, store: &mut Store, owner: &F3UnitKey) -> Ref<OwnerSnapshot>;
    /// Recon launch publication binds a fresh facade for each generation.
    fn created(&mut self, _store: &Store, _droid: Ref<ReconDroid>) {}
    /// Preserve a terminal old facade before its network slot is replaced.
    fn replacing(&mut self, _store: &Store, _droid: Ref<ReconDroid>) {}
    fn fire(
        &mut self,
        authority: &mut Authority,
        store: &mut Store,
        def: u32,
        at: Vector3,
        direction: Vector3,
        owner: Ref<OwnerSnapshot>,
    );
    /// Append the remoteShot presentation notification after native fire.
    fn fired(&mut self, store: &Store, droid: Ref<ReconDroid>, shot: Shot, at: Vector3);
    fn explode(
        &mut self,
        authority: &mut Authority,
        store: &mut Store,
        def: u32,
        at: Vector3,
        owner: Ref<OwnerSnapshot>,
    );
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub slot: u32,
    pub generation: u32,
    nonce: u64,
}
// RemoteDroids launch slots are independent of generated slab free order.
// reserved prevents two weapons admitting the same physical capacity.
// generation advances only when a body is successfully committed.
// nonce prevents an old cancelled token from affecting a later admission.
// droid retains terminal state while a detached salvo still needs its owner.
// Reusing it first recovers the preallocated weapon vector.
// Context order controls simulation order; this table controls admission only.
// A slot's reuse never rewrites an already-issued launch snapshot.
struct Slot {
    generation: u32,
    reserved: bool,
    nonce: u64,
    droid: Option<Ref<ReconDroid>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    Owner,
    Tick,
    Reservation,
    Definition,
}

// RemoteDroids separates connection order from physical launch slots.
// The order list preserves the sequence in which owners first attach.
// Each slot has its own visible generation and private reservation nonce.
// A droid can outlive the soldier life that launched it.
// The owner context can also outlive its connection through a detached strike.
// Those lifetimes must not be collapsed into the Unit slab's generation.
// The retained vectors below are phase work storage, not copied live state.
// Nested fire/explosion borrows this same authority through world services.
pub struct Authority {
    pub defs: Definitions,
    order: Vec<Ref<ReconContext>>,
    slots: Vec<Slot>,
    owner_capacity: usize,
    slot_capacity: usize,
    nonce: u64,
    triangles: Vec<Triangle>,
    shots: Vec<Shot>,
    // createDroid arms inside combat. Reserve the largest loadout per slot
    // now, then recover that same buffer from a retired droid on reuse.
    arsenals: Vec<Vec<crate::gen::Weapon>>,
    changed_droids: Vec<Ref<ReconDroid>>,
    changed_contexts: Vec<Ref<ReconContext>>,
}
impl Authority {
    /// Snapshot capture borrows the admission table, never the owner lists or
    /// the generated slab's unrelated free order (EntityRemoteCapture).
    pub(crate) fn wire_slots(&self) -> usize { self.slots.len() }
    pub(crate) fn wire_slot(&self, index: usize) -> Option<(u32, Ref<ReconDroid>)> {
        let slot = self.slots.get(index)?;
        slot.droid.map(|droid| (slot.generation, droid))
    }
    /// captureRemoteReplay follows the connection's current qualified owner,
    /// while the wire identity still belongs to its admitted launch slot.
    pub(crate) fn wire_own(&self, store: &Store, owner: &F3UnitKey, capture: &mut crate::wire::store_own::Capture,
        aspect: f64) -> Result<Option<(u32, u32, u32)>, &'static str> {
        let Some(context) = self.order.iter().filter_map(|&r| store.recon_context.get(r))
            .find(|c| !c.retired && same(&c.owner, owner)) else { return Ok(None) };
        let Some(reference) = context.piloted else { return Ok(None) };
        let droid = store.recon_droid.get(reference).ok_or("recon identity prerequisite missing")?;
        let (slot, identity) = self.slots.iter().enumerate().find(|(_, s)| s.droid == Some(reference))
            .ok_or("recon identity prerequisite missing")?;
        let mut salvo = None;
        if let Some(s) = &droid.salvo {
            // strikeLevels resolves each destruct level in authored order;
            // the first matching inventory weapon wins when content shares it.
            'weapons: for (index, weapon) in droid.weapons.iter().enumerate() {
                let Some(levels) = self.defs.levels.get(weapon.def.0 as usize) else { continue };
                for (level, def) in levels.iter().enumerate() {
                    if def.strike == Some(s.strike) {
                        let class = self.defs.weapons.classes.get(weapon.def.0 as usize).and_then(Option::as_ref)
                            .and_then(|c| c.levels.get(level)).ok_or("recon salvo definition missing")?;
                        let maximum = jsrt::js_max(1., jsrt::num_or(class.get(crate::combat::defs::K::SalvoCount).num(), 1.).floor());
                        salvo = Some((index as u32, level as u32, maximum)); break 'weapons;
                    }
                }
            }
        }
        capture.recon(droid, context, &self.defs.weapons, aspect, salvo)?;
        Ok(Some((slot as u32, identity.generation, droid.class)))
    }
    /// Recon admission uses the server's player limit, supplied before
    /// its first context. Reserve that bound here, outside every live phase.
    // RemoteDroids owner admission is bounded by the connection limit.
    // Growing this storage belongs to world construction, before any owner runs.
    // The order buffer also keeps detached strikes from disconnected owners.
    // Those owners no longer count against admission, but must keep their place.
    // Changed-context capacity includes their terminal publications as well.
    // Changing the limit after attachment would invalidate that reservation.
    pub fn configure_owners(&mut self, owners: usize) {
        assert!(self.order.is_empty(), "recon capacity after owner attachment");
        self.owner_capacity = owners;
        self.order.reserve(owners.saturating_sub(self.order.len()));
        self.changed_contexts.reserve((owners + self.slot_capacity).saturating_sub(self.changed_contexts.len()));
    }
    // RemoteDroids construction allocates the fixed launch pool here.
    // A physical slot and a context are independent: one owner may launch again.
    // The largest loaded arsenal determines every reusable slot's weapon capacity.
    // Weapon vectors move into droids on launch and return on slot reuse.
    // Neither death nor a class rebind frees a slot that owns a detached salvo.
    // This keeps first launch and later reuse off the allocator during combat.
    pub fn new(defs: Definitions, owners: usize, slots: usize) -> Self {
        assert!(slots > 0 && slots <= 4096);
        let weapons = defs.droids.iter().map(|d| d.loadout.len()).max().unwrap_or(0);
        let arsenals = (0..slots).map(|_| Vec::with_capacity(weapons)).collect();
        Self {
            defs,
            order: Vec::with_capacity(owners),
            slots: Vec::with_capacity(slots),
            owner_capacity: owners,
            slot_capacity: slots,
            nonce: 0,
            triangles: Vec::new(),
            shots: Vec::with_capacity(64),
            arsenals,
            changed_droids: Vec::with_capacity(slots),
            changed_contexts: Vec::with_capacity(owners + slots),
        }
    }
    /// Reserve content-dependent query and shot maxima at world load. The
    /// collision gather appends to the retained buffer and must not retain it.
    // RemoteDroids.collide and shoot share these work arrays serially.
    // A gather may append every triangle in its declared content bound.
    // Clearing it preserves backing storage and closest-triangle visitation order.
    // Arsenal updates write all shots before any fire service can recurse.
    // Content loading can increase the maximum loadout after Authority creation.
    // Reserve that increase here instead of extending arsenals during a launch.
    pub fn reserve_scratch(&mut self, triangles: usize, shots: usize) {
        self.triangles
            .reserve(triangles.saturating_sub(self.triangles.len()));
        self.shots.reserve(shots.saturating_sub(self.shots.len()));
        let weapons = self.defs.droids.iter().map(|d| d.loadout.len()).max().unwrap_or(0);
        for arsenal in &mut self.arsenals {
            arsenal.reserve(weapons.saturating_sub(arsenal.len()));
        }
    }
    /// Publications drain only these touched references. A missing context
    /// row is a retirement, rather than a reason to copy the other owners.
    // RemoteDroids publication follows the mutations recorded by this owner.
    // A caller consumes the list before clear_changes, retaining slab generations.
    // A replaced slot must bind its new facade before shots name that generation.
    // Publication observes the completed native row without running flight again.
    pub fn changed_droids(&self) -> &[Ref<ReconDroid>] {
        &self.changed_droids
    }
    // RemoteDroids control state is distinct from each flying body's state.
    // Entering, exiting and retiring can change only the owner's context row.
    // A removed row still needs a terminal publication to retire its mirror.
    // Readers therefore cannot infer context changes from the droid list.
    pub fn changed_contexts(&self) -> &[Ref<ReconContext>] {
        &self.changed_contexts
    }
    // RemoteDroids publication clears only the active lengths of these lists.
    // Their allocations survive into the next phase and synchronous command.
    // Clearing does not remove droids or cancel pending orbital strikes.
    // The owning coordinator decides when every reader has consumed the batch.
    pub fn clear_changes(&mut self) {
        self.changed_droids.clear();
        self.changed_contexts.clear();
    }
    // RemoteDroids can touch the same body in flight, fire and nested damage.
    // Keep its first mutation position so publication retains observation order.
    // The launch pool bounds this small list independently of infantry capacity.
    // A replacement generation is a distinct identity even in the same slot.
    fn mark_droid(&mut self, r: Ref<ReconDroid>) {
        if !self.changed_droids.contains(&r) {
            self.changed_droids.push(r);
        }
    }
    // RemoteDroids may exit control from both owner death and droid death.
    // Coalesce the mirror update while retaining its first mutation position.
    // The row contains the final native camera and control result for this span.
    // No JavaScript camera update is needed to finish authoritative bookkeeping.
    fn mark_context(&mut self, r: Ref<ReconContext>) {
        if !self.changed_contexts.contains(&r) {
            self.changed_contexts.push(r);
        }
    }
    pub fn contexts(&self) -> &[Ref<ReconContext>] {
        &self.order
    }
    // RemoteDroids owner keys identify connections rather than body slots.
    // A class change can replace the body while preserving the connection key.
    // Retired contexts stay discoverable until their detached work has drained.
    // The qualified body key on that context separately rejects stale lives.
    pub(crate) fn context(&self, store: &Store, id: u32) -> Option<Ref<ReconContext>> {
        self.order
            .iter()
            .copied()
            .find(|r| store.recon_context.get(*r).is_some_and(|c| c.key == id))
    }
    /// Recon context attachment: independent stream state is supplied once.
    // RemoteDroids owner attachment creates one independent stream and camera.
    // An equal numeric connection key may only reuse its matching qualified owner.
    // A retained retired context must pass admission again before reactivation.
    // New contexts reserve their launch and strike lists for the fixed slot pool.
    // The initial camera quaternion is identity, not a newly sampled body pose.
    // No random value is drawn while establishing the stream's initial state.
    pub fn attach(
        &mut self,
        store: &mut Store,
        id: u32,
        owner: F3UnitKey,
        seed: u32,
    ) -> Result<Ref<ReconContext>, Error> {
        if let Some(r) = self.context(store, id) {
            if !same(&store.recon_context.get(r).unwrap().owner, &owner) {
                return Err(Error::Owner);
            }
            if store.recon_context.get(r).unwrap().retired {
                self.admit_owner(store)?;
                self.mark_context(r);
                store.recon_context.get_mut(r).unwrap().retired = false;
            }
            return Ok(r);
        }
        self.admit_owner(store)?;
        let r = store.recon_context.insert(ReconContext {
            key: id,
            owner,
            seed,
            last_tick: -1.0,
            camera: crate::gen::ReconCamera {
                rotation: [0.0, 0.0, 0.0, 1.0], ..Default::default()
            },
            droids: Vec::with_capacity(self.slot_capacity),
            strikes: Vec::with_capacity(self.slot_capacity),
            ..Default::default()
        });
        self.order.push(r);
        self.mark_context(r);
        Ok(r)
    }
    // RemoteDroids admission counts active contexts, not slab occupancy.
    // A disconnected owner can retain a strike without blocking a new player.
    // Its rows remain valid so immutable launch attribution can finish later.
    // Capacity failure happens before a new context or reservation is committed.
    fn admit_owner(&self, store: &Store) -> Result<(), Error> {
        if self
            .order
            .iter()
            .filter(|r| !store.recon_context.get(**r).unwrap().retired)
            .count()
            >= self.owner_capacity
        {
            Err(Error::Capacity)
        } else {
            Ok(())
        }
    }
    /// Rebinding keeps flying droids and their captured old-life attribution.
    // RemoteDroids class replacement exits control before changing the owner.
    // Already flying bodies retain the launch key captured by createDroid.
    // The context keeps its RNG state instead of restarting from the new seed.
    // That distinction preserves future draw order after respawn or reconnect.
    // A retired connection is readmitted before its context becomes active again.
    // No existing salvo is retargeted to the replacement soldier.
    pub fn rebind(
        &mut self,
        store: &mut Store,
        id: u32,
        owner: F3UnitKey,
        seed: u32,
    ) -> Result<Ref<ReconContext>, Error> {
        let Some(r) = self.context(store, id) else {
            return self.attach(store, id, owner, seed);
        };
        if store.recon_context.get(r).unwrap().retired {
            self.admit_owner(store)?;
        }
        self.exit(store, r);
        // Rebinding replaces only the current connection-to-body association.
        // Do not reseed the context: an old salvo still consumes this stream.
        // The caller's seed is used only if this key needs a new context.
        // Already captured launch owners remain immutable in the owner slab.
        let c = store.recon_context.get_mut(r).unwrap();
        c.owner = owner;
        c.retired = false;
        Ok(r)
    }
    // RemoteDroids.exit releases control without killing the flying body.
    // Zoom belongs to that control session and resets with it.
    // The camera pose survives until a later enter/update supplies a new sample.
    // Detached strikes and their captured owner remain independent of piloting.
    pub fn exit(&mut self, store: &mut Store, r: Ref<ReconContext>) {
        self.mark_context(r);
        let c = store.recon_context.get_mut(r).unwrap();
        c.piloted = None;
        c.zoom = false;
    }
    // RemoteDroids.enter rebuilds the chase controller over its retained camera.
    // Recompose the quaternion through YXZ Euler angles as SoldierCamera did.
    // Copying the quaternion verbatim would change low bits after another launch.
    // Mouse accumulation starts fresh, while the prior camera position survives.
    // The next camera update, rather than this command, chooses the chase reach.
    // The body must still be alive before it can become the piloted droid.
    pub fn enter(&mut self, store: &mut Store, r: Ref<ReconDroid>) {
        let Some(d) = store.recon_droid.get(r) else {
            return;
        };
        if d.dead {
            return;
        }
        let context = d.context;
        let fov = self.defs.droids[d.class as usize].third_fov;
        self.mark_context(context);
        let c = store.recon_context.get_mut(context).unwrap();
        // SoldierCamera's order assignment also recomposes its existing
        // quaternion through the camera's synchronized YXZ Euler angles.
        // Keep that constructor arithmetic, including its sign and low bits.
        let mut rotation = Quaternion::new(c.camera.rotation[0], c.camera.rotation[1],
            c.camera.rotation[2], c.camera.rotation[3]);
        let mut euler = three::Euler::new(0.0, 0.0, 0.0, three::EulerOrder::YXZ);
        euler.set_from_quaternion(&rotation, None);
        rotation.set_from_euler(&euler);
        // RemoteDroids.enter creates a fresh SoldierCamera controller over
        // the same aimCamera. Keep that camera's last pose until its next
        // update; a second launch can publish a correction before then.
        c.camera = crate::gen::ReconCamera {
            position: c.camera.position,
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
            reach: 1.0,
            fresh: true,
            fov,
            ..Default::default()
        };
        c.piloted = Some(r);
        c.look_x = 0.0;
        c.look_y = 0.0;
        c.zoom = false;
    }
    // RemoteDroids.look accumulates individual mouse samples in input order.
    // Apply inversion before accumulation to keep mixed settings samples exact.
    // The fly operation later applies zoom to pitch, but not to yaw.
    // Publishing this context does not consume the pending look deltas.
    pub fn look(
        &mut self,
        store: &mut Store,
        r: Ref<ReconContext>,
        dx: f64,
        dy: f64,
        sensitivity: f64,
        invert: bool,
    ) {
        let c = store.recon_context.get_mut(r).unwrap();
        c.look_x += dx * sensitivity;
        c.look_y += (if invert { dy } else { -dy }) * sensitivity;
        self.mark_context(r);
    }
    /// Reservations happen before the remote weapon consumes its shot. Slots
    /// cannot be reused while a dead droid still owns a detached salvo.
    // RemoteDroids.launch admission precedes spending the remote weapon shot.
    // Search slots in ascending order, independent of the slab's free-list order.
    // Dead bodies remain unavailable while their orbital salvo is attached.
    // A reservation does not advance the visible generation until launch commits.
    // Its nonce separates cancellation from a later reservation of the same slot.
    // Exhaustion refuses the shot rather than recycling an active identity.
    pub fn reserve(&mut self, store: &Store) -> Option<Reservation> {
        let index = self.slots.iter().position(|s| {
            !s.reserved
                && s.generation < u32::MAX
                && s.droid.is_none_or(|r| {
                    store
                        .recon_droid
                        .get(r)
                        .is_none_or(|d| d.dead && d.salvo.is_none())
                })
        });
        let i = match index {
            Some(i) => i,
            None if self.slots.len() < self.slot_capacity => {
                self.slots.push(Slot {
                    generation: 0,
                    reserved: false,
                    nonce: 0,
                    droid: None,
                });
                self.slots.len() - 1
            }
            None => return None,
        };
        self.nonce = self
            .nonce
            .checked_add(1)
            .expect("recon reservation counter exhausted");
        self.slots[i].reserved = true;
        self.slots[i].nonce = self.nonce;
        Some(Reservation {
            slot: i as u32,
            generation: self.slots[i].generation + 1,
            nonce: self.nonce,
        })
    }
    /// Combat's pending-shot token keeps slot and generation exactly in f64.
    /// The reservation nonce never crosses the native service boundary.
    pub(crate) fn token(reservation: Reservation) -> f64 {
        reservation.slot as f64 + reservation.generation as f64 * 4096.0
    }
    pub(crate) fn reservation(&self, token: f64) -> Option<Reservation> {
        let generation = (token / 4096.0).floor() as u32;
        let slot = (token % 4096.0) as u32;
        let s = self.slots.get(slot as usize)?;
        (s.reserved && s.generation.checked_add(1) == Some(generation)
            && token == slot as f64 + generation as f64 * 4096.0)
            .then_some(Reservation { slot, generation, nonce: s.nonce })
    }
    // RemoteDroids failed launch returns exactly its outstanding reservation.
    // Both nonce and next generation must still match the slot being cancelled.
    // A stale token cannot cancel a later admission that reused the same slot.
    // Cancellation preserves the visible generation because no body was born.
    pub fn cancel(&mut self, token: Reservation) {
        if let Some(s) = self.slots.get_mut(token.slot as usize) {
            if s.nonce == token.nonce && s.generation.checked_add(1) == Some(token.generation) {
                s.reserved = false;
            }
        }
    }
    // RemoteDroids.of finds the living launch belonging to this weapon index.
    // Weapon indices repeat after reset, so the captured gameplay life also matters.
    // Using only the context would let a new class enter an old life's droid.
    // Retired or dead launches cannot be reacquired even if their salvo remains.
    pub fn of(
        &self,
        store: &Store,
        context: Ref<ReconContext>,
        weapon: u32,
    ) -> Option<Ref<ReconDroid>> {
        let owner = &store.recon_context.get(context)?.owner;
        store
            .recon_context
            .get(context)?
            .droids
            .iter()
            .copied()
            .find(|r| {
                store
                    .recon_droid
                    .get(*r)
                    .is_some_and(|d| !d.dead && d.weapon == weapon && same(&d.owner, owner))
            })
    }
    /// RemoteDroids.launch/createDroid. Definitions and the owner are checked
    /// before consuming the token, so failed construction can cancel it.
    // RemoteDroids.createDroid commits only after validating owner and content.
    // Capture launch attribution before replacing a reusable droid's terminal row.
    // Recover its reserved arsenal buffer and reset the prepared weapon states.
    // The birth service binds identity before aim or any later shot publication.
    // Immediate launches enter control only after their initial aim is computed.
    // A failure leaves the caller responsible for cancelling the admission token.
    pub fn launch<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        context: Ref<ReconContext>,
        token: Reservation,
        class: u32,
        weapon: u32,
        position: Vector3,
        velocity: Vector3,
        yaw: f64,
        team: f64,
        immediate: bool,
    ) -> Result<Ref<ReconDroid>, Error> {
        let slot = self
            .slots
            .get(token.slot as usize)
            .ok_or(Error::Reservation)?;
        if !slot.reserved
            || slot.nonce != token.nonce
            || slot.generation.checked_add(1) != Some(token.generation)
        {
            return Err(Error::Reservation);
        }
        let c = store.recon_context.get(context).ok_or(Error::Owner)?;
        if c.retired || c.owner.validate(store).is_err() {
            return Err(Error::Owner);
        }
        let def = self
            .defs
            .droids
            .get(class as usize)
            .ok_or(Error::Definition)?;
        // RemoteDroids.createDroid freezes attribution when the droid launches.
        // A context can later rebind while this droid still owns a salvo.
        let owner = key(&c.owner);
        let launch_owner = world.capture_owner(store,&owner);
        if let Some(old) = self.slots[token.slot as usize].droid {
            world.replacing(store, old);
            self.changed_droids.retain(|r| *r != old);
        }
        let mut armed = match self.slots[token.slot as usize].droid {
            Some(old) => std::mem::take(&mut store.recon_droid.get_mut(old).expect("admitted recon slot").weapons),
            None => std::mem::take(&mut self.arsenals[token.slot as usize]),
        };
        weapons::arm_into(&def.loadout,&self.defs.weapons,true,&mut armed);
        let d = ReconDroid {
            context,
            owner,
            launch_owner,
            class,
            weapon,
            position: a(position),
            velocity: a(velocity),
            yaw,
            team,
            health: def.health,
            countdown: -1.0,
            weapons: armed,
            selected: [0.0; 2],
            ..Default::default()
        };
        // Network identity uses the first reusable slot rather than slab free order.
        let slot = &mut self.slots[token.slot as usize];
        if let Some(old) = slot.droid {
            store.recon_droid.remove(old);
        }
        let r = store.recon_droid.insert_at(token.slot, token.generation, d);
        slot.generation = token.generation;
        slot.reserved = false;
        slot.droid = Some(r);
        store.recon_context.get_mut(context).unwrap().droids.push(r);
        self.mark_droid(r);
        self.mark_context(context);
        world.created(store, r);
        self.aim(store, world, r);
        if immediate {
            self.enter(store, r);
        }
        Ok(r)
    }
    /// Death commits before the immediate explosion, including exiting control.
    // RemoteDroids.kill commits death and relinquishes piloting before blasting.
    // The recursive blast may revisit this body, which must already reject damage.
    // Its immutable launch owner supplies credit even after a class rebind.
    // An orbital salvo is deliberately left attached to the terminal body.
    // Slot reuse waits for that salvo rather than ending the strike on death.
    // The explosion completes synchronously before the caller continues.
    pub fn kill<W: World>(&mut self, store: &mut Store, world: &mut W, r: Ref<ReconDroid>) {
        let Some(d) = store.recon_droid.get_mut(r) else {
            return;
        };
        if d.dead {
            return;
        }
        d.dead = true;
        d.health = 0.0;
        let (context, at, owner, explosion) = (
            d.context,
            v(d.position),
            d.launch_owner,
            self.defs.droids[d.class as usize].explosion,
        );
        self.mark_droid(r);
        if store.recon_context.get(context).unwrap().piloted == Some(r) {
            self.exit(store, context);
        }
        if let Some(def) = explosion {
            world.explode(self, store, def, at, owner);
        }
    }
    // RemoteDroids.hurt ignores nonpositive damage and already terminal bodies.
    // Publish the health subtraction before entering a recursive death explosion.
    // A nonlethal hit leaves flight and piloting unchanged.
    // Damage is a native mutation, not an observation of the JS droid facade.
    pub fn hurt<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        r: Ref<ReconDroid>,
        amount: f64,
    ) {
        let Some(d) = store.recon_droid.get_mut(r) else {
            return;
        };
        if d.dead || !(amount > 0.0) {
            return;
        }
        d.health -= amount;
        let dead = d.health <= 0.0;
        self.mark_droid(r);
        if dead {
            self.kill(store, world, r);
        }
    }
    /// Retired contexts stay ordered until both live droids and strikes drain.
    // RemoteDroids owner removal exits control and stops accepting new launches.
    // Living droids and detached strikes still run in the context's original order.
    // Dead bodies without relevant work can leave its active list immediately.
    // Pruning waits until both lists empty so strike attribution stays resolvable.
    pub fn retire(&mut self, store: &mut Store, id: u32) {
        if let Some(r) = self.context(store, id) {
            self.exit(store, r);
            let c = store.recon_context.get_mut(r).unwrap();
            c.retired = true;
            c.droids
                .retain(|r| store.recon_droid.get(*r).is_some_and(|d| !d.dead));
            self.prune(store);
        }
    }
    // RemoteDroids.finish retires contexts only after their detached work ends.
    // Stable retention preserves relative order among the surviving owners.
    // Removing a context is separate from retiring its droid slab generations.
    // The changed-context list still lets the coordinator publish the retirement.
    fn prune(&mut self, store: &mut Store) {
        self.order.retain(|r| {
            let c = store.recon_context.get(*r).unwrap();
            if c.retired && c.droids.is_empty() && c.strikes.is_empty() {
                store.recon_context.remove(*r);
                false
            } else {
                true
            }
        });
    }
    /// RemoteDroids.update: active owners run this before their combat.
    // RemoteDroids.update runs at the unit's pre-combat observation point.
    // The tick stamp prevents finish from advancing an active owner a second time.
    // Owner death exits control before camera and control edge consumption.
    // Capture the flying flag before movement; a fatal collision skips fire entirely.
    // Unpiloted droids receive neutral input, while their weapons keep advancing.
    // Strikes run after droid flight/fire and survive their launching body's death.
    pub fn step<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        id: u32,
        tick: f64,
        dt: f64,
        input: Input,
    ) -> Result<bool, Error> {
        let Some(r) = self.context(store, id) else {
            return Ok(false);
        };
        let c = store.recon_context.get(r).unwrap();
        if c.last_tick == tick {
            return Ok(false);
        }
        if tick < c.last_tick {
            return Err(Error::Tick);
        }
        let alive = c.owner.validate(store).is_ok_and(|u| u.alive);
        self.mark_context(r);
        store.recon_context.get_mut(r).unwrap().last_tick = tick;
        if !alive {
            self.exit(store, r);
        }
        self.update_camera(store, world, r, dt);
        let input = world.control_input(store, r, input);
        if store.recon_context.get(r).unwrap().piloted.is_some() {
            if input.exit {
                self.exit(store, r);
            } else if input.zoom && self.zoom_limit(store, r, false) > 1.0 {
                let c = store.recon_context.get_mut(r).unwrap();
                c.zoom = !c.zoom;
            }
        }
        let mut i = 0;
        while i < store.recon_context.get(r).unwrap().droids.len() {
            let droid = store.recon_context.get(r).unwrap().droids[i];
            i += 1;
            if store.recon_droid.get(droid).is_none_or(|d| d.dead) {
                continue;
            }
            self.mark_droid(droid);
            let flying = store.recon_context.get(r).unwrap().piloted == Some(droid);
            self.fly(
                store,
                world,
                droid,
                dt,
                if flying { Some(input) } else { None },
            );
            if store.recon_droid.get(droid).unwrap().dead {
                continue;
            }
            self.aim(store, world, droid);
            // RemoteDroids.update skips shoot entirely after a fatal impact.
            // Its captured flying flag still selects this droid's controls.
            let weapon_input = if flying { world.weapon_input(store, r, input) } else { Input::default() };
            self.shoot(store, world, droid, dt, weapon_input);
            let d = store.recon_droid.get_mut(droid).unwrap();
            d.health += self.defs.droids[d.class as usize].add_health * dt;
            if d.health <= 0.0 || d.position[1] < -500.0 {
                self.kill(store, world, droid);
            }
        }
        store.recon_context.get_mut(r).unwrap().droids.retain(|r| {
            store
                .recon_droid
                .get(*r)
                .is_some_and(|d| !d.dead || d.salvo.is_some())
        });
        self.update_strikes(store, world, r, dt);
        Ok(true)
    }
    /// Coordinator calls finish after vehicles/seats and before separation.
    // RemoteDroids.update also runs for seated, dead and disconnected owners.
    // Visit them after vehicles and seats, before soldier separation.
    // An active owner has already stamped this tick and therefore becomes a no-op.
    // Default input advances detached flight without consuming a soldier's edges.
    // Owner death releases control even if its unit tick never ran.
    // Only after these updates can empty retired contexts leave the order list.
    pub fn finish<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        tick: f64,
        dt: f64,
    ) -> Result<(), Error> {
        let mut i = 0;
        while i < self.order.len() {
            let r = self.order[i];
            i += 1;
            let c = store.recon_context.get(r).unwrap();
            if !c.owner.validate(store).is_ok_and(|u| u.alive) {
                self.exit(store, r);
            }
            let id = store.recon_context.get(r).unwrap().key;
            self.step(store, world, id, tick, dt, Input::default())?;
        }
        self.prune(store);
        Ok(())
    }
    // RemoteDroids zoom reads the selected primary weapon's prepared level.
    // An invalid selected index behaves like an absent weapon and returns one.
    // Channel-relative selection must not be treated as an arsenal-wide index.
    // Minimum zoom scales look input; maximum zoom controls whether toggle is useful.
    fn zoom_limit(&self, store: &Store, r: Ref<ReconContext>, minimum: bool) -> f64 {
        let c = store.recon_context.get(r).unwrap();
        let Some(pilot) = c.piloted else {
            return 1.0;
        };
        let d = store.recon_droid.get(pilot).unwrap();
        let selected = d.selected[0];
        if !(selected >= 0.0 && selected.fract() == 0.0) {
            return 1.0;
        }
        let Some(w) = d
            .weapons
            .iter()
            .filter(|w| w.in_channel(&self.defs.weapons, 0.0))
            .nth(selected as usize)
        else {
            return 1.0;
        };
        let level = self.defs.levels[w.def.0 as usize][w.level as usize];
        if minimum {
            level.zoom_min
        } else {
            level.zoom_max
        }
    }
    /// RemoteDroids.fly: spring and drive order, including the misspelled
    /// Acceleraton class field, are retained by the prepared definition.
    // RemoteDroids.fly samples the hover surface before integrating drive.
    // The body center sits above its origin, so preserve both height offsets.
    // Fast travel adds a slanted forward probe before spring and ground-normal drive.
    // The hover factor limits drive authority away from the desired altitude.
    // Only a piloted body consumes the context's accumulated mouse movement.
    // Collision follows position integration and may complete an immediate explosion.
    fn fly<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        r: Ref<ReconDroid>,
        dt: f64,
        input: Option<Input>,
    ) {
        let d = store.recon_droid.get(r).unwrap();
        let def = &self.defs.droids[d.class as usize];
        // Probe from the physical center, not the model's eye or muzzle.
        // Ground distance is then translated back to the hovering origin.
        // Keeping the 0.155 offsets separate preserves their cancellation order.
        // Water can replace a lower physical surface without changing its normal.
        let mut velocity = v(d.velocity);
        let mut center = v(d.position);
        center.y += 0.155;
        let reach = RADIUS + 10.0 * def.altitude;
        let mut height = reach - 0.155;
        let mut n = UP;
        if let Some(hit) = world.cast(center, Vector3::new(0.0, -1.0, 0.0), reach) {
            height = hit.distance - 0.155;
            n = hit.normal;
        }
        if let Some(level) = world.water(center.x, center.z) {
            if center.y - level < height + 0.155 {
                height = min(height, center.y - level - 0.155);
                n = UP;
            }
        }
        if hypot(velocity.x, velocity.z) > 5.0 {
            let mut ray = Vector3::new(velocity.x, velocity.y - reach, velocity.z);
            let length = ray.length();
            ray.divide_scalar(length);
            if let Some(hit) = world.cast(center, ray, length) {
                let mut along = hit.point;
                along.sub(&center);
                height = min(height, -along.dot(&hit.normal));
                if hit.normal.y > 0.0 {
                    n.lerp(&hit.normal, (1.0 - hit.distance / length) * hit.normal.y)
                        .normalize();
                }
            }
        }
        let f = clamp(
            1.0 - (height - def.altitude) / (9.0 * def.altitude),
            0.0,
            1.0,
        );
        // three.js lerp is (1-t)*x+t*y, not x+(y-x)*t.
        velocity.y -= ((1.0 - f) * 1.0 + f * def.gravity) * 9.8 * dt;
        let lift = (def.altitude - height) * def.spring + 9.8 * f - velocity.dot(&n) * def.damp;
        if lift > 0.0 {
            velocity.add_scaled_vector(&n, lift * dt);
        }
        // Project the yaw direction onto the sampled surface before driving.
        // The right vector uses forward cross normal, preserving the original sign.
        // Along and across errors are clamped independently by hover authority.
        // Apply forward drive before strafe so each addition retains its low bits.
        let mut forward = Vector3::new(sin(d.yaw), 0.0, cos(d.yaw));
        forward.add_scaled_vector(&n, -forward.dot(&n)).normalize();
        let mut right = Vector3::default();
        right.cross_vectors(&forward, &n).normalize();
        let along = velocity.dot(&forward);
        let across = velocity.dot(&right);
        let most = def.acceleration * f * max(n.y, 0.0) * dt;
        velocity.add_scaled_vector(
            &forward,
            clamp(
                def.speed * input.map_or(0.0, |i| i.forward) - along,
                -most,
                most,
            ),
        );
        velocity.add_scaled_vector(
            &right,
            clamp(
                def.strafe * input.map_or(0.0, |i| i.right) - across,
                -most,
                most,
            ),
        );
        let (turn, pitch) = (def.turn, def.pitch);
        let context = d.context;
        // Zoom scales only the accumulated pitch delta at this point.
        // The turn stick and horizontal mouse retain the unzoomed class speed.
        // A detached body receives no context mouse input and leaves it untouched.
        // The piloted body consumes those deltas exactly once per stamped tick.
        let zoom = if store.recon_context.get(context).unwrap().zoom {
            self.zoom_limit(store, context, true)
        } else {
            1.0
        };
        let d = store.recon_droid.get_mut(r).unwrap();
        d.velocity = a(velocity);
        if let Some(input) = input {
            let c = store.recon_context.get_mut(context).unwrap();
            d.yaw -= input.turn * turn * dt;
            d.yaw -= c.look_x * (turn / 2048.0);
            d.pitch = clamp(
                d.pitch + (c.look_y * (turn / 2048.0)) / zoom,
                pitch[0] * std::f64::consts::PI / 180.0,
                pitch[1] * std::f64::consts::PI / 180.0,
            );
            c.look_x = 0.0;
            c.look_y = 0.0;
        }
        let mut position = v(d.position);
        position.add_scaled_vector(&velocity, dt);
        d.position = a(position);
        self.collide(store, world, r);
    }
    /// RemoteDroids.collide: four passes and strict first closest-triangle
    /// ties. A crash death explodes before sliding/bounce completes.
    // RemoteDroids.collide gathers into one retained triangle buffer per pass.
    // Strict comparisons retain the first equally close triangle.
    // A short downward cast supplies the original fallback when no triangle wins.
    // Contact damage occurs after displacement but before sliding and restitution.
    // Even a fatal crash finishes that contact's velocity arithmetic afterward.
    // Four passes preserve corners and repeated-contact damage in source order.
    fn collide<W: World>(&mut self, store: &mut Store, world: &mut W, r: Ref<ReconDroid>) {
        let d = store.recon_droid.get(r).unwrap();
        let def = &self.defs.droids[d.class as usize];
        let ([base, slope], scale) = (def.threshold, def.collision_scale);
        for _ in 0..4 {
            let mut center = v(store.recon_droid.get(r).unwrap().position);
            center.y += 0.155;
            self.triangles.clear();
            world.gather(center, RADIUS, &mut self.triangles);
            let mut best = f64::INFINITY;
            let mut n = UP;
            for tri in &self.triangles {
                let mut closest = Vector3::default();
                tri.closest_point_to_point(&center, &mut closest);
                let distance = closest.distance_to(&center);
                if distance < RADIUS && distance < best && distance > 1e-5 {
                    best = distance;
                    n = center;
                    n.sub(&closest).divide_scalar(distance);
                }
            }
            let depth = if best < f64::INFINITY {
                RADIUS - best
            } else if let Some(hit) = world.cast(center, Vector3::new(0.0, -1.0, 0.0), RADIUS) {
                n = hit.normal;
                RADIUS - hit.distance
            } else {
                break;
            };
            if depth <= 0.0 {
                break;
            }
            let d = store.recon_droid.get_mut(r).unwrap();
            let mut p = v(d.position);
            p.add_scaled_vector(&n, depth);
            d.position = a(p);
            let approach = v(d.velocity).dot(&n);
            // Only an approaching contact causes damage, friction or restitution.
            // The positional correction still applies when the body is moving away.
            // Use the pre-slide approach speed for all following contact arithmetic.
            // A fatal hurt can recurse, so reacquire the row before changing velocity.
            if approach >= 0.0 {
                continue;
            }
            let speed = -approach;
            let threshold = base + slope * n.dot(&UP);
            if speed > threshold {
                let excess = speed - threshold;
                self.hurt(store, world, r, scale * (excess * excess));
            }
            let d = store.recon_droid.get_mut(r).unwrap();
            let mut velocity = v(d.velocity);
            velocity.add_scaled_vector(&n, -approach);
            let length = velocity.length();
            if length > 1e-6 {
                velocity.multiply_scalar(1.0 - min(1.0, (0.28125 * speed) / length));
            }
            velocity.add_scaled_vector(&n, 0.125 * speed);
            d.velocity = a(velocity);
        }
    }
    /// RemoteDroids.aim. The world has no visual model; its prepared muzzle
    /// and eye are used identically to the worker's simulation pose.
    // RemoteDroids.aim computes gameplay eye and target without a drawn model.
    // Chase control projects its camera backward along aim before casting.
    // First person and zoom keep the eye-based origin instead.
    // The target is retained because every weapon muzzle converges on that point.
    // Its initial value is computed at launch before control can be entered.
    // A missing obstruction keeps the fixed 512-metre target reach.
    // Own wire corrections omit these derived vectors. Private page replay
    // reconstructs them with the same collision/camera core before its first
    // predicted tick, including a frame with no outstanding commands.
    pub(crate) fn aim<W: World>(&mut self, store: &mut Store, world: &mut W, r: Ref<ReconDroid>) {
        let context = store.recon_droid.get(r).unwrap().context;
        let c = store.recon_context.get(context).unwrap();
        let zoom = if c.zoom {
            self.zoom_limit(store, context, true)
        } else {
            1.0
        };
        // A chase camera can be displaced from the gameplay eye.
        // Project its origin along aim rather than casting directly from the muzzle.
        // The body then converges its separately yawed muzzle on that target.
        // A nonpiloted or zoomed body uses its class eye instead.
        let chase = c.piloted == Some(r) && !c.first_person && zoom <= 1.0;
        let camera = v(c.camera.position);
        let d = store.recon_droid.get_mut(r).unwrap();
        let def = &self.defs.droids[d.class as usize];
        let mut eye = v(def.eye);
        eye.add(&v(d.position));
        let dir = Vector3::new(
            sin(d.yaw) * cos(d.pitch),
            sin(d.pitch),
            cos(d.yaw) * cos(d.pitch),
        );
        let mut from = eye;
        if chase {
            from = camera;
            let mut away = from;
            away.sub(&eye);
            from.add_scaled_vector(&dir, -away.dot(&dir));
        }
        let hit = world.cast(from, dir, 512.0);
        let mut target = from;
        target.add_scaled_vector(&dir, hit.map_or(512.0, |h| h.distance));
        d.eye = a(eye);
        d.aim = a(dir);
        d.target = a(target);
    }
    /// RemoteDroids.shoot uses the ordinary Arsenal solver and one retained
    /// shot buffer. All weapon updates precede any of their fire operations.
    // RemoteDroids.shoot gives all weapons the context's independent generator.
    // Finish Arsenal.update before firing any produced shot through F1.
    // Each muzzle is yaw-rotated, then converges on the retained aim target.
    // Spread rotates yaw first and pitch around the resulting horizontal axis.
    // The self-destruct channel starts a countdown independently of primary fire.
    // Once a salvo exists, further countdown updates must not launch it twice.
    fn shoot<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        r: Ref<ReconDroid>,
        dt: f64,
        input: Input,
    ) {
        let context = store.recon_droid.get(r).unwrap().context;
        let zoom = store.recon_context.get(context).unwrap().zoom
            && self.zoom_limit(store, context, true) > 1.0;
        let mut draws = Draws(ClassicRandom::from_state(
            store.recon_context.get(context).unwrap().seed,
        ));
        self.shots.clear();
        let d = store.recon_droid.get_mut(r).unwrap();
        for w in &mut d.weapons {
            w.random = true;
        }
        let mut arsenal = Arsenal {
            weapons: &mut d.weapons,
            selected: &mut d.selected,
        };
        arsenal.update(
            &self.defs.weapons,
            dt,
            &ArsenalInput {
                primary: Trigger {
                    pressed: input.primary,
                    held: input.held || input.primary,
                    ..Default::default()
                },
                ..Default::default()
            },
            &Aim {
                stick: 0.0,
                posture: Some(Posture::Stand),
                zoomed: zoom,
                repair_target: false,
            },
            false,
            &mut draws,
            &mut self.shots,
        );
        // Publish all arsenal draws before executing the first collected shot.
        // An instant hit can kill a droid or start a recursive explosion.
        // That nested work must observe the generator state after Arsenal.update.
        // It must not restart from the state preceding the shot list.
        store.recon_context.get_mut(context).unwrap().seed = draws.0.state;
        for i in 0..self.shots.len() {
            let shot = self.shots[i];
            let d = store.recon_droid.get(r).unwrap();
            let weapon = &d.weapons[shot.weapon as usize];
            let level = self.defs.levels[weapon.def.0 as usize][weapon.level as usize];
            let Some(ordnance) = level.ordnance else {
                continue;
            };
            let ShotKind::Round { yaw, pitch, .. } = shot.kind else {
                continue;
            };
            let mut at = v(self.defs.droids[d.class as usize].muzzle);
            let mut q = Quaternion::default();
            q.set_from_axis_angle(&UP, d.yaw);
            at.apply_quaternion(&q).add(&v(d.position));
            let mut dir = v(d.target);
            dir.sub(&at).normalize();
            q.set_from_axis_angle(&UP, yaw);
            dir.apply_quaternion(&q);
            let mut axis = Vector3::new(dir.z, 0.0, -dir.x);
            axis.normalize();
            q.set_from_axis_angle(&axis, -pitch);
            dir.apply_quaternion(&q);
            let owner = d.launch_owner;
            world.fire(self, store, ordnance, at, dir, owner);
            world.fired(store, r, shot, at);
        }
        let d = store.recon_droid.get_mut(r).unwrap();
        let arsenal = Arsenal {
            weapons: &mut d.weapons,
            selected: &mut d.selected,
        };
        let destruct = arsenal.current(&self.defs.weapons, 1).map(|i| {
            let w = &d.weapons[i];
            self.defs.levels[w.def.0 as usize][w.level as usize]
        });
        // The destruct action starts only while its countdown is inactive.
        // The same update can then advance it by this tick's dt.
        // This keeps a zero-press-time strike eligible immediately.
        // An already created salvo owns the countdown's eventual completion.
        if d.countdown < 0.0 && input.secondary && destruct.is_some_and(|d| d.strike.is_some()) {
            d.countdown = 0.0;
        }
        if d.countdown >= 0.0 && d.salvo.is_none() {
            d.countdown += dt;
            if let Some(level) = destruct {
                if d.countdown >= level.pressed_time {
                    if let Some(strike) = level.strike {
                        self.strike(store, r, strike);
                    }
                }
            }
        }
    }
    /// RemoteDroids.strike: freeze the target and frame at countdown expiry.
    // RemoteDroids.strike freezes its orbital frame at countdown expiry.
    // Later movement or death of the droid cannot move the stored strike target.
    // The right/up basis preserves the original azimuth/elevation operation order.
    // The strike is retained on both droid and context until all rounds fire.
    // An initial zero wait makes its first round eligible in the same update.
    // No scatter draws occur until updateStrikes actually emits a round.
    pub fn strike(&mut self, store: &mut Store, r: Ref<ReconDroid>, strike: u32) {
        self.mark_droid(r);
        let def = self.defs.strikes[strike as usize];
        let elevation = def.elevation * std::f64::consts::PI / 180.0;
        let azimuth = def.azimuth * std::f64::consts::PI / 180.0;
        let z = Vector3::new(
            sin(azimuth) * cos(elevation),
            sin(elevation),
            cos(azimuth) * cos(elevation),
        );
        let x = Vector3::new(cos(azimuth), 0.0, -sin(azimuth));
        let mut y = Vector3::default();
        y.cross_vectors(&z, &x);
        let d = store.recon_droid.get_mut(r).unwrap();
        let mut at = v(d.position);
        at.y += 0.155;
        d.salvo = Some(ReconSalvo {
            strike,
            at: a(at),
            x: a(x),
            y: a(y),
            z: a(z),
            left: def.count,
            wait: 0.0,
        });
        let context = d.context;
        store
            .recon_context
            .get_mut(context)
            .unwrap()
            .strikes
            .push(r);
        self.mark_context(context);
    }
    /// RemoteDroids.updateStrikes. Four draws occur even for zero scatter.
    /// The fire call precedes decrementing the salvo, matching nested blasts.
    // RemoteDroids.updateStrikes iterates the live strike list by index.
    // The stored frame and immutable owner survive disconnected contexts.
    // Four uniform draws produce triangular scatter, even when scatter is zero.
    // Commit the generator before firing because an immediate hit can recurse.
    // Decrement count and advance delay only after that fire completes.
    // The final round clears the salvo before kill can admit this slot for reuse.
    fn update_strikes<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        context: Ref<ReconContext>,
        dt: f64,
    ) {
        let mut i = 0;
        while i < store.recon_context.get(context).unwrap().strikes.len() {
            let r = store.recon_context.get(context).unwrap().strikes[i];
            i += 1;
            self.mark_droid(r);
            store
                .recon_droid
                .get_mut(r)
                .unwrap()
                .salvo
                .as_mut()
                .unwrap()
                .wait -= dt;
            loop {
                let d = store.recon_droid.get(r).unwrap();
                let s = d.salvo.as_ref().unwrap();
                if !(s.left > 0.0 && s.wait <= 0.0) {
                    break;
                }
                let def = self.defs.strikes[s.strike as usize];
                let mut rng =
                    ClassicRandom::from_state(store.recon_context.get(context).unwrap().seed);
                let mut start = v(s.at);
                start.add_scaled_vector(&v(s.x), def.scatter * (rng.uniform() - rng.uniform()));
                start.add_scaled_vector(&v(s.y), def.scatter * (rng.uniform() - rng.uniform()));
                start.add_scaled_vector(&v(s.z), def.distance);
                let mut dir = v(s.z);
                dir.negate();
                let owner = d.launch_owner;
                store.recon_context.get_mut(context).unwrap().seed = rng.state;
                world.fire(self, store, def.ordnance, start, dir, owner);
                let s = store
                    .recon_droid
                    .get_mut(r)
                    .unwrap()
                    .salvo
                    .as_mut()
                    .unwrap();
                s.left -= 1.0;
                s.wait += def.delay;
            }
            if store
                .recon_droid
                .get(r)
                .unwrap()
                .salvo
                .as_ref()
                .unwrap()
                .left
                <= 0.0
            {
                store.recon_droid.get_mut(r).unwrap().salvo = None;
                self.kill(store, world, r);
            }
        }
        store
            .recon_context
            .get_mut(context)
            .unwrap()
            .strikes
            .retain(|r| store.recon_droid.get(*r).is_some_and(|d| d.salvo.is_some()));
    }
    /// RemoteDroids.blast, in context and launch order. Death does not cancel
    /// an orbital salvo; retired owners can therefore finish detached strikes.
    // RemoteDroids.blast visits contexts and their launch lists in order.
    // A blast may kill a droid and recurse before the next live index is visited.
    // Measure from its offset body center, reducing distance by half the radius.
    // Only static geometry provides blast cover; moving hulls do not block it.
    // Apply push before damage so death observes the updated native velocity.
    // The upward push component remains even when the center direction is zero.
    pub fn blast<W: World>(
        &mut self,
        store: &mut Store,
        world: &mut W,
        explosion: Blast,
        position: Vector3,
    ) {
        for i in 0..self.order.len() {
            let context = self.order[i];
            let mut j = 0;
            while j < store.recon_context.get(context).unwrap().droids.len() {
                let r = store.recon_context.get(context).unwrap().droids[j];
                j += 1;
                let Some(d) = store.recon_droid.get(r) else {
                    continue;
                };
                if d.dead {
                    continue;
                }
                let mut center = v(d.position);
                center.y += 0.155;
                let distance = center.distance_to(&position) - 0.5 * RADIUS;
                // Damage and push have independent radial envelopes.
                // A zero damage scale can still leave a useful push.
                // Conversely, a missing push radius must not suppress ordinary damage.
                // Test usefulness before normalizing or casting the cover ray.
                let damage = explosion.damage
                    * crate::ordnance::defs::falloff(distance, explosion.inner, explosion.outer)
                    * explosion.droid_scale;
                let push = explosion.push
                    * crate::ordnance::defs::falloff(
                        distance,
                        explosion.push_inner,
                        explosion.push_outer,
                    );
                if !(damage > 0.0 || push > 0.0) {
                    continue;
                }
                let mut ray = center;
                ray.sub(&position);
                let length = ray.length();
                if length >= 1e-3 {
                    ray.divide_scalar(length);
                    if world
                        .static_cast(position, ray, length)
                        .is_some_and(|h| h.distance < length - 0.05)
                    {
                        continue;
                    }
                }
                if push > 0.0 {
                    self.mark_droid(r);
                    let mut away = center;
                    away.sub(&position);
                    let d = store.recon_droid.get_mut(r).unwrap();
                    let mut velocity = v(d.velocity);
                    if away.length_sq() > 1e-6 {
                        away.normalize();
                        velocity.add_scaled_vector(&away, push);
                    }
                    // The upward impulse is unconditional once push is positive.
                    // At coincident centers there is no normalized outward impulse.
                    // Keep the vertical addition as its separate operation afterward.
                    // Damage follows, so a death explosion sees this committed velocity.
                    velocity.y += 0.6 * push;
                    d.velocity = a(velocity);
                }
                self.hurt(store, world, r, damage);
            }
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct Blast {
    pub damage: f64,
    pub inner: f64,
    pub outer: f64,
    pub droid_scale: f64,
    pub push: f64,
    pub push_inner: f64,
    pub push_outer: f64,
}
// RemoteDroids weapon updates draw from their context's independent stream.
// The wrapper exposes the ordinary Arsenal draw interface without imports.
// A collected shot stores its resulting spread before any native F1 fire.
// The context seed is committed before recursive hit and blast services run.
// Page prediction retains a separate private stream for its replay session.
// Sharing the interface does not share those independent generator states.
struct Draws(ClassicRandom);
impl WeaponDraws for Draws {
    fn draw(&mut self, _: u32) -> f64 {
        self.0.uniform()
    }
    fn legacy(&mut self, _: u32) -> f64 {
        self.0.uniform()
    }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission {
        Admission::Yes
    }
}
