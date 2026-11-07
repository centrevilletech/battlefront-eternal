//! The snapshot producer borrows canonical entity and sidecar state once.
//! Only immutable protocol maps enter from the host, at session construction.
use crate::{abi::sim, gen::{WireStoreSetup, WireStoreRecords}, store::StrId};
use super::{store_capture::{self, World}, shots::Counters};

// Validate descriptors before generated take can index them. A malformed
// count must reject at attachment instead of trapping the battle module.
fn valid_setup(row: &[f64], heap: &[f64], strings: usize) -> bool {
    use crate::gen::{WireStoreDefinition as D, WireStoreName as N, WireStoreSetup as S};
    fn integer(value: f64, max: usize) -> bool { value.is_finite() && value >= 0. && value.fract() == 0. && value <= max as f64 }
    if row.len() < S::WORDS { return false; }
    // String handles are content facts, not arbitrary u32 values. Checking
    // them here also protects later enum-to-name samples of empty maps.
    for field in [S::UNBUILT, S::BUILT, S::DESTROYED] {
        if !integer(row[field], strings.saturating_sub(1)) || strings == 0 { return false; }
    }
    for field in [S::ITEM_CAPACITY, S::REMOTE_CAPACITY, S::ENTITY_CAPACITY, S::RECORD_CAPACITY, S::SEAT_CAPACITY] {
        if !integer(row[field], u32::MAX as usize) { return false; }
    }
    for (field, width) in [(S::CLASSES, N::WORDS), (S::WEAPON_NAMES, N::WORDS), (S::POSTS, N::WORDS), (S::WEAPONS, D::WORDS), (S::ITEMS, D::WORDS), (S::REMOTES, D::WORDS), (S::ORDNANCE, D::WORDS)] {
        // List descriptors refer to the call heap, not to the whole module's
        // memory. Bound both dimensions before calculating their full span.
        if !integer(row[field], heap.len()) || !integer(row[field + 1], heap.len() / width) { return false; }
        let start = row[field] as usize; let count = row[field + 1] as usize;
        let Some(entries) = heap.get(start..start.saturating_add(count.saturating_mul(width))) else { return false };
        for entry in entries.chunks_exact(width) {
            let named = field == S::CLASSES || field == S::WEAPON_NAMES || field == S::POSTS;
            let first_max = if named { strings.saturating_sub(1) } else { u32::MAX as usize };
            let (key, id) = if named { (N::NAME, N::ID) } else { (D::DEFINITION, D::ID) };
            if !integer(entry[key], first_max) || !integer(entry[id], 65535) { return false; }
        }
    }
    use crate::gen::{WireStorePeer as P, F4Handle as H, F4ObjectKind};
    let field = S::PEERS;
    if !integer(row[field], heap.len()) || !integer(row[field + 1], 256) { return false; }
    let start = row[field] as usize; let count = row[field + 1] as usize;
    let Some(peers) = heap.get(start..start.saturating_add(count.saturating_mul(P::WORDS))) else { return false };
    for peer in peers.chunks_exact(P::WORDS) {
        if !integer(peer[P::ID], u32::MAX as usize) || !integer(peer[P::MAILBOX], u32::MAX as usize)
            || crate::abi::world().input.mailboxes.get(peer[P::MAILBOX] as usize).is_none() { return false; }
        if peer[P::CHARACTER] != 0. && peer[P::CHARACTER] != 1. { return false; }
        if peer[P::CHARACTER] == 1. { continue; }
        let h = &peer[P::CHARACTER + 1..][..H::WORDS];
        if h[H::KIND] != F4ObjectKind::Character as u8 as f64 || !integer(h[H::SLOT], u32::MAX as usize)
            || !integer(h[H::GENERATION], u32::MAX as usize) || !h[H::LIFE].is_finite() { return false; }
    }
    true
}

struct NativeWorld<'a> {
    // The immutable Store borrow spans this capture only. Its sidecar readers
    // touch neither that store nor the call block, and cannot invoke a host.
    store: &'a crate::gen::Store,
    recon: &'a crate::recon::Authority,
    states: [StrId; 3],
}
impl World for NativeWorld<'_> {
    fn walk(&self, r: crate::store::Ref<crate::gen::Vehicle>) -> (f64, f64) { crate::vehicles::module::wire_walk(r) }
    fn remote_count(&self) -> usize { self.recon.wire_slots() }
    fn remote(&self, index: usize) -> Option<(u32, crate::store::Ref<crate::gen::ReconDroid>)> { self.recon.wire_slot(index) }
    fn turret_count(&self) -> usize { crate::vehicles::module::f2::live_seats::wire_turret_count() }
    fn turret(&self, index: usize) -> Option<store_capture::Turret> {
        crate::vehicles::module::f2::live_seats::wire_turret(index, self.store, self.states)
    }
}
// entitySpawnPosts and classEligibility are read once per team and tick.
// Each team's retained vectors preserve authored post and class order.
// A publication with several clients must not repeat collision queries.
struct Eligibility { valid: bool, posts: Vec<f64>, classes: Vec<f64> }
impl Default for Eligibility { fn default() -> Self { Self { valid: false, posts: Vec::with_capacity(64), classes: Vec::with_capacity(64) } } }
// EntityWorldCapture owns immutable protocol maps alongside reusable rows.
// Foreign facts have fixed capacities independent of the current roster.
// Only cold admission can replace definition maps or grow weapon defaults.
pub struct Source { ordnance_ids: Vec<u32>, dynamic_faults: [StrId; 7], projectiles: super::store_projectiles::Capture, own_stagger: super::stagger::Stagger, eligibility: [Eligibility; 8], eligibility_key: Option<(f64, f64, bool)>, own: super::store_own::Capture, flight_names: [StrId; 14], pub capture: store_capture::Capture, states: [StrId; 3], world: super::store_world::Units, summaries: super::store_summaries::Summaries, stage: super::stagger::Stagger, peers: Vec<crate::gen::WireStorePeer>, input: Vec<f64>, fields: super::store_world::WorldFields, names: [StrId; 5] }
impl Source {
    // InputMailbox retains held/view values at consumption, not at publish.
    // Qualified handles reject a retired connection instead of following its
    // replacement into another player correction.
    fn mailbox(&self,id:u32)->super::codec::Result<&crate::input::owner::Mailbox> {
        let peer=self.peers.iter().find(|p|p.id==id).ok_or("own input prerequisite missing")?;
        crate::abi::world().input.mailboxes.get(peer.mailbox as usize).ok_or("own input prerequisite missing")
    }

    // SnapshotHost.publish supplies clocks before entering the native borrow.
    // Copy the fixed header so output allocation cannot invalidate a view.
    // The full descriptor is validated before any list is decoded.
    pub(crate) fn publication_clock(&self) -> [f64; crate::gen::WireWorldFacts::WORDS] { self.input[..crate::gen::WireWorldFacts::WORDS].try_into().unwrap() }
    // captureSpawnSummary queries choices without advancing spawn queues.
    // Its cache includes the completed tick, epoch and game-over state.
    // An invalid team is a normal capture fault, never an array index.
    fn eligibility(&mut self, tick: f64, epoch: f64, team: f64) -> super::codec::Result<usize> {
        let store = &sim().store; let key = (tick, epoch, store.battle.game_over);
        if self.eligibility_key != Some(key) {
            for entry in &mut self.eligibility { entry.valid = false; entry.posts.clear(); entry.classes.clear(); }
            self.eligibility_key = Some(key);
        }
        if !team.is_finite() || team.fract() != 0. || team < 0. || team >= 8. { return Err("invalid summary scalar"); }
        let index = team as usize; let entry = &mut self.eligibility[index];
        if entry.valid { return Ok(index); }
        entry.posts.clear(); entry.classes.clear();
        fn append(out: &mut Vec<f64>, value: f64) -> super::codec::Result<()> {
            if out.len() == out.capacity() { return Err("spawn eligibility limit"); } out.push(value); Ok(())
        }
        // entitySpawnPosts permits friendly paths and allied authored paths.
        // Convert post ownership with JS ToInt32 before relation lookup.
        // A neutral post is excluded even when it has authored spawn positions.
        fn allowed(owner: f64, team: f64, path: bool, points: f64, ally: bool) -> bool {
            let owner = jsrt::to_int32(owner) as f64;
            owner != 0. && if owner == team { path || points > 0. } else { crate::units::f3::relation(owner, team) > 0. && ally }
        }
        if !store.battle.game_over {
            // entitySpawnPosts keeps placed, mobile, then landed transport
            // order. Eligibility reads paths and ownership only; it never
            // refreshes a post, advances a spawn queue or draws randomness.
            for &r in &store.command_posts.posts {
                let Some(post) = store.post.get(r).filter(|p| p.active) else { continue };
                let ai = post.ai.as_ref();
                if allowed(post.team, team, ai.is_some_and(|a| a.spawn_path.as_ref().is_some_and(|p| p.present)),
                    ai.map_or(0., |a| a.class_spawn_points), ai.is_some_and(|a| a.ally_path.as_ref().is_some_and(|p| p.present))) {
                    append(&mut entry.posts, self.capture.post_id(post.name).ok_or("placed post registry prerequisite missing")?)?;
                }
            }
            for &r in &store.mobile_posts.posts {
                let Some(post) = store.mobile_post.get(r).filter(|p| p.active) else { continue };
                if allowed(post.team, team, false, post.ai.class_spawn_points, false) {
                    append(&mut entry.posts, self.capture.post_id(post.name).ok_or("mobile post registry prerequisite missing")?)?;
                }
            }
            for &r in &store.vehicles.vehicles {
                let Some(v) = store.vehicle.get(r).filter(|v| v.alive && v.flight_class == Some(crate::gen::FlightClass::Transport)
                    && v.flight_state == Some(crate::gen::FlightState::Landed) && jsrt::num_truthy(v.team.unwrap_or(0.))) else { continue };
                // transportPosts tests the exit floor before team filtering.
                // Preserve that observation even when this team's relation
                // later excludes the craft from its spawn choices.
                let mut at = three::Vector3::new(v.bounds_max[0] + 2., 1., 0.);
                at.apply_quaternion(&three::Quaternion::new(v.quaternion[0], v.quaternion[1], v.quaternion[2], v.quaternion[3]));
                at.add(&three::Vector3::new(v.position[0], v.position[1], v.position[2]));
                let floor = crate::units::native::ordnance_cast(crate::units::f3::collision_world(), [at.x, at.y, at.z], [0., -1., 0.],
                    12., Some(v.body), true, false);
                if floor.is_none_or(|hit| hit.normal[1] < 0.8) { continue; }
                if allowed(v.team.unwrap_or(0.), team, true, 0., false) {
                    append(&mut entry.posts, self.capture.transport_post_id(v.slot_index).ok_or("summary registry prerequisite missing")?)?;
                }
            }
            // Network admission prepares both complete human rosters before
            // registering its connection. Availability here is content, not
            // a count of currently free pooled bodies.
            for &name in crate::battle::f4_tail::wire_roster(index) {
                let id = self.capture.class_id(name); if id == 0. { return Err("spawn class registry prerequisite missing"); }
                append(&mut entry.classes, id)?;
            }
        }
        entry.valid = true; Ok(index)
    }
    // ownSummaryFields joins the mailbox to F4's connection by peer id.
    // Pending and auto requests are still mailbox facts; realized spawn state
    // and current unit generation are read from the native connection owner.
    fn own_summaries(&mut self, id: u32, epoch: f64) -> super::codec::Result<()> {
        use crate::gen::*;
        let header = WireWorldFacts::WORDS;
        let (tick, words) = facts(&self.input[..header], &self.input[header..]).ok_or("invalid canonical world facts")?;
        words.chunks_exact(WirePublicationLink::WORDS).find(|r| r[WirePublicationLink::ID] == id as f64)
            .ok_or("own input prerequisite missing")?;
        let foreign = self.mailbox(id)?.publication.spawn.clone();
        if !foreign.valid { return Err("spawn selection registry prerequisite missing"); }
        let peer = self.peers.iter().find(|p| p.id == id).ok_or("own spectator correction not implemented")?;
        let handle = peer.character.as_ref().ok_or("own spectator correction not implemented")?;
        let store = &sim().store;
        let character = crate::battle::f4_connections::wire_character(store, handle).ok_or("own spectator correction not implemented")?;
        let connection = crate::battle::f4_connections::wire_view(store, handle).ok_or("own spectator correction not implemented")?;
        let team = store.character.get(character).ok_or("own spectator correction not implemented")?.team;
        let slot = store.battle.characters.iter().position(|&r| r == character).unwrap_or(255);
        let identity = connection.unit.and_then(|r| store.unit.get(r).map(|u| (r.slot as f64, u.generation))).unwrap_or((0., 0.));
        let index = self.eligibility(tick, epoch, team)?; let eligibility = &self.eligibility[index];
        self.summaries.own(tick, epoch, store, &self.capture, character, super::store_summaries::Spawn {
            connection, identity, team, slot, posts: &eligibility.posts, classes: &eligibility.classes,
            pending: foreign.pending.then_some((foreign.pending_class, foreign.pending_post)),
            auto: foreign.auto.then_some((foreign.auto_team, foreign.auto_class, foreign.auto_post)),
        }, &mut self.own_stagger)
    }
    pub fn capture(&mut self, shots: &Counters) -> super::codec::Result<()> {
        // Borrow the existing recon authority rather than importing its JS
        // facade. with_services returns the same owner after this closure;
        // no launch, rewind sample or publication clock advances here.
        crate::units::f3::with_services(|services| {
            let services = services.ok_or("canonical entity services unavailable")?;
            let store = &sim().store;
            let defs = crate::combat::module::defs();
            let world = NativeWorld { store, recon: services.recon, states: self.states };
            self.capture.capture(store, &defs.weapons, &world, shots)
        })
    }
    // capturePlayerCorrection chooses a body from the qualified connection.
    // Its unit may differ from the previous publication after spawn or death.
    // Reset mount/recon scratch before choosing this client's current mode.
    fn own(&mut self, id: u32, epoch: f64) -> super::codec::Result<(f64, f64)> {
        use crate::gen::*;
        let header = WireWorldFacts::WORDS;
        let (_, words) = facts(&self.input[..header], &self.input[header..]).ok_or("invalid canonical world facts")?;
        words.chunks_exact(WirePublicationLink::WORDS).find(|r| r[WirePublicationLink::ID] == id as f64)
            .ok_or("own input prerequisite missing")?;
        let foreign = self.mailbox(id)?.publication.clone();
        let peer = self.peers.iter().find(|p| p.id == id).ok_or("own spectator correction not implemented")?;
        let store = &sim().store;
        let connection = crate::battle::f4_connections::wire_view(store, peer.character.as_ref().ok_or("own spectator correction not implemented")?).ok_or("own spectator correction not implemented")?;
        let reference = connection.unit.ok_or("own spectator correction not implemented")?;
        let unit = store.unit.get(reference).ok_or("own spectator correction not implemented")?;
        if unit.team != 1. && unit.team != 2. { return Err("invalid own team"); }
        if self.capture.class_id(unit.class_name) == 0. { return Err("player class prerequisite missing"); }
        let defs = &crate::combat::module::defs().weapons;
        self.own.mounted.value.clear(); self.own.remote.value.clear();
        let mut vehicle = None;
        // captureMountedControl requires both the unit life and the live lease.
        // A reused hull slot alone cannot authorize a rider correction.
        // Read weapon barrel state from the same native seat owner as simulation.
        if let Some(lease) = unit.seat {
            let lease = store.seat_lease.get(lease).filter(|l| !l.released && l.unit == Some(reference) && l.generation == unit.generation)
                .ok_or("mounted control lease prerequisite missing")?;
            if let Some(hull) = lease.vehicle {
                let v = store.vehicle.get(hull).ok_or("mounted control lease prerequisite missing")?;
                if !lease.index.is_finite() || lease.index < 0. || lease.index.fract() != 0. || lease.index > 255. { return Err("mounted control lease prerequisite missing"); }
                let seat = v.seats.get(lease.index as usize).ok_or("mounted control lease prerequisite missing")?;
                let mode = if lease.index == 0. { 4 } else { 2 }; let mut aim = [0.; 4]; let mut count = 0; let mut flags = 0;
                if mode == 2 {
                    aim[0] = seat.aim_yaw; aim[1] = seat.pitch; count = 2;
                    if let Some(value) = seat.look_yaw { flags |= 1; aim[count] = value; count += 1; }
                    if let Some(value) = seat.look_pitch { flags |= 2; aim[count] = value; count += 1; }
                }
                crate::vehicles::module::f2::firing::wire_weapons(hull, v.target_generation, lease.index as usize,
                    |weapons| self.own.mount(mode, v.target_id, v.target_generation, lease.index, &aim[..count], flags, weapons, defs))?;
                if mode == 4 { vehicle = Some((hull, v)); }
            } else if let Some(turret) = lease.turret {
                crate::vehicles::module::f2::live_seats::wire_own_turret(store, turret, &mut self.own, defs)?;
            } else { return Err("mounted control lease prerequisite missing"); }
        }
        if !foreign.view.profile_found { return Err("binding profile mismatch"); }
        let (schema, mount) = if let Some((reference, v)) = vehicle {
            let seat = v.seats.first().ok_or("pilot lease prerequisite missing")?;
            let (yaw_rate, last_yaw) = crate::vehicles::module::wire_seat_history(reference, 0).ok_or("pilot lease prerequisite missing")?;
            let fields = [Some(seat.aim_yaw), Some(seat.pitch), seat.look_yaw, seat.look_pitch, Some(yaw_rate), last_yaw];
            let schema = if v.flying {
                let names = self.flight_names;
                fn code(id: Option<StrId>, names: &[StrId], error: &'static str) -> super::codec::Result<u8> {
                    names.iter().position(|n| Some(*n) == id).map(|i| i as u8).ok_or(error)
                }
                let enums = [v.flight_state.ok_or("invalid flight flightState")? as u8,
                    code(v.trick, &names[..4], "invalid flight trick")?, code(v.flight_lock_state, &[names[0], names[4], names[5]], "invalid flight flightLockState")?,
                    code(v.flight_warning, &[names[0], names[6], names[7], names[8], names[9], names[10], names[11], names[12]], "invalid flight flightWarning")?,
                    code(v.landing_message, &[names[0], names[6], names[7], names[8], names[9], names[10], names[11], names[12]], "invalid flight landingMessage")?,
                    code(v.landing_reason, &[names[0], names[6], names[7], names[8], names[9], names[10], names[11], names[12]], "invalid flight landingReason")?];
                self.own.flight(v, enums, fields)?
            } else { self.own.ground(v, fields)? };
            (schema, (v.target_id, v.target_generation))
        } else { (self.own.foot(&unit.soldier)?, (0., 0.)) };
        let remote = crate::units::f3::with_services(|services| {
            let services = services.ok_or("canonical entity services unavailable")?;
            services.recon.wire_own(store, &F3UnitKey::of(reference, unit), &mut self.own, foreign.recon_aspect)
        })?;
        if remote.is_some() && !self.own.mounted.value.is_empty() { return Err("conflicting own control modes"); }
        let remote = remote.map(|(id, generation, class)| self.capture.remote_id(class).map(|definition| (id as f64, generation as f64, definition))
            .ok_or("recon content prerequisite missing")).transpose()?;
        let v = foreign.view;
        let flags = if v.flags.is_finite() && v.flags >= 0. && v.flags <= 2047. && v.flags.fract() == 0. { v.flags as u16 }
            else { return Err("invalid own-player integer") };
        let input = super::store_own::Facts { held: foreign.held.0, invalid_held: !foreign.held_valid, flags,
            view: [v.mouse_scale, v.aspect, v.view_tick, v.sensitivity, v.curve], axes: v.axes,
            // applyNetworkControls keeps the chosen perspective in the input
            // view. Combat's dormant mirror is not its publication owner.
            buttons: jsrt::to_uint32(v.buttons) as u16, binding: v.profile as u16, discarded: foreign.discarded,
            presentation_first: Some(flags & 2 != 0) };
        self.own.envelope(unit, schema, mount, epoch, epoch, &self.capture, defs, &input, remote)?;
        Ok((reference.slot as f64, unit.generation))
    }
}
#[no_mangle]
pub extern "C" fn wire_store_source_words() -> u32 { WireStoreSetup::WORDS as u32 }
#[no_mangle]
pub extern "C" fn wire_store_source_create() -> *mut Source {
    super::owner::with(|owner| {

    let (row, heap) = sim().calls.args();
    if !valid_setup(row, heap, sim().strings.list.len()) { return std::ptr::null_mut(); }
    let setup = WireStoreSetup::take(row, heap);
    // Only construction owns these maps. A future native publication borrows
    // the retained rows directly without decoding an entity argument block.
    let Ok(capture) = store_capture::Capture::new(&setup, &sim().strings) else { return std::ptr::null_mut() };
    let Ok(ordnance_ids) = store_capture::definitions(&setup.ordnance) else { return std::ptr::null_mut() };
    let own = super::store_own::Capture::new(&crate::combat::module::defs().weapons);
    // captureCompactFlight serializes authored status strings as enum bytes.
    // Intern those names cold, including the empty message and trick names.
    // No first-seen flight status may allocate during publication.
    let flight_names = ["", "barrel", "loop", "immelmann", "Locking", "Locked", "Takeoff path blocked", "Landing path blocked", "Too fast", "No landing zone", "Press SPACE to land", "Flight ceiling", "Leaving battlefield", ""].map(|n| sim().strings.intern_str(n));
    owner.sources.insert(Source { ordnance_ids, dynamic_faults: ["invalid projectile sample", "invalid projectile integer", "invalid projectile channel", "invalid projectile type/orientation", "combat capture capacity exceeded", "dynamic projectile capture capacity exceeded", "combat definition absent from matched registry"].map(|s| sim().strings.intern_str(s)), projectiles: Default::default(), own_stagger: super::stagger::Stagger::own(), eligibility: std::array::from_fn(|_| Eligibility::default()), eligibility_key: None, own, flight_names, capture, states: [setup.unbuilt, setup.built, setup.destroyed], world: Default::default(), summaries: Default::default(), stage: super::stagger::Stagger::new(), peers: setup.peers, input: vec![0.; crate::gen::WireWorldFacts::WORDS + 256 * crate::gen::WirePublicationLink::WORDS], fields: Default::default(),
        names: [setup.unbuilt, setup.built, setup.destroyed, sim().strings.intern_str("waiting"), sim().strings.intern_str("active")] }).unwrap_or(0) as *mut Source

    })
}
/// # Safety
/// This source belongs to this instance and is disposed exactly once.
#[no_mangle]
pub unsafe extern "C" fn wire_store_source_drop(source: *mut Source) {
    super::owner::with(|owner| {
if owner.sources.get(source as usize).is_none() {return;}
 if !source.is_null() { owner.sources.remove(source as usize); }
    })
}

/// Cold content preparation can extend native definition handles between
/// client admissions. It never replaces captured lives or protocol identities.
/// # Safety
/// The source is a live allocation belonging to this instance.
#[no_mangle]
pub unsafe extern "C" fn wire_store_source_configure(source: *mut Source) -> u32 {
    super::owner::with(|owner| {
if owner.sources.get(source as usize).is_none() {return 0;}

    let (row, heap) = sim().calls.args();
    if source.is_null() || !valid_setup(row, heap, sim().strings.list.len()) { return 0; }
    let setup = WireStoreSetup::take(row, heap);
    if (owner.sources.get_mut(source as usize).unwrap()).capture.rebind(&setup, &sim().strings).is_err() { return 0; }
    let Ok(ordnance_ids) = store_capture::definitions(&setup.ordnance) else { return 0 };
    (owner.sources.get_mut(source as usize).unwrap()).ordnance_ids = ordnance_ids;
    (owner.sources.get_mut(source as usize).unwrap()).states = [setup.unbuilt, setup.built, setup.destroyed];
    (owner.sources.get_mut(source as usize).unwrap()).peers = setup.peers;
    (owner.sources.get_mut(source as usize).unwrap()).eligibility_key = None;
    (owner.sources.get_mut(source as usize).unwrap()).own.rebind(&crate::combat::module::defs().weapons);
    1

    })
}

/// A read-only probe for the producer corpus. Production consumes the retained
/// records natively; it never materializes these generated record objects.
/// # Safety
/// Both pointers are live, distinct allocations belonging to this instance.
#[no_mangle]
pub unsafe extern "C" fn wire_store_source_probe(source: *mut Source, shots: *const Counters) -> *const f64 {
    super::owner::with(|owner| {
if owner.sources.get(source as usize).is_none() {return std::ptr::null();}
if owner.shots.get(shots as usize).is_none() {return std::ptr::null();}

    let before = crate::allocations::snapshot();
    let result = (owner.sources.get_mut(source as usize).unwrap()).capture(owner.shots.get(shots as usize).unwrap());
    let used = crate::allocations::snapshot().since(before);
    let mut out = match result {
        Ok(()) => WireStoreRecords { records: (owner.sources.get(source as usize).unwrap()).capture.records().to_vec(), ..Default::default() },
        Err(error) => WireStoreRecords { error: Some(sim().strings.intern_str(error)), ..Default::default() },
    };
    out.allocations = used.calls as f64; out.reallocations = used.reallocs as f64;
    sim().calls.result(WireStoreRecords::WORDS, |row, heap| out.put(row, heap));
    sim().calls.block.ptr()

    })
}


// captureWorldPresentation borrows F3/F4 scalar sidecars in authored order.
// These accessors never export a rig or refresh its lazy geometry.
// The presentation writer only observes state already owned by the phase.
struct NativeProps;
impl super::store_world::Props for NativeProps {
    fn building_count(&self) -> usize { crate::units::f3_props::count() }
    fn building(&self, index: usize) -> Option<super::store_world::Building> { crate::units::f3_props::wire_building(index) }
    fn animation_count(&self) -> usize { crate::battle::f4_live::wire_animation_count() }
    fn animation(&self, index: usize) -> Option<[f64; 7]> { crate::battle::f4_live::wire_animation(index) }
    fn trap_count(&self) -> usize { crate::battle::f4_live::wire_trap_count() }
    fn trap(&self, index: usize) -> Option<[f64; 3]> { crate::battle::f4_live::wire_trap(index) }
}

// captureRoster finds transport facts through qualified Characters.
// Protocol peer ids are stable across reorderings of the JS player Map.
// Spectators can supply links without becoming native Character owners.
struct NativeLinks<'a> { store: &'a crate::gen::Store, peers: &'a [crate::gen::WireStorePeer], words: &'a [f64] }
impl super::store_summaries::Links for NativeLinks<'_> {
    fn get(&self, character: crate::store::Ref<crate::gen::Character>) -> Option<super::store_summaries::Link> {
        for peer in self.peers {
            let Some(handle) = &peer.character else { continue };
            if crate::battle::f4_connections::wire_character(self.store, handle) != Some(character) { continue; }
            for row in self.words.chunks_exact(crate::gen::WirePublicationLink::WORDS) {
                let link = crate::gen::WirePublicationLink::take(row, &[]);
                if link.id == peer.id { return Some(super::store_summaries::Link { rtt: link.rtt, has_kind: link.has_kind,
                    relay: link.relay, has_family: link.has_family, family_six: link.family_six }); }
            }
        }
        None
    }
}
// The reserved block contains transport observations only. Borrow its fixed
// generated rows directly, after checking descriptors; take() on the whole
// list would allocate merely to return the same six scalar facts per link.
// SnapshotHost.publish writes one reserved input block at a time.
// Validate every list dimension before generated row decoding.
// This protects the frame path from malformed counts and boolean tags.
fn facts<'a>(row: &[f64], heap: &'a [f64]) -> Option<(f64, &'a [f64])> {
    use crate::gen::{WireWorldFacts as F, WirePublicationLink as L};
    fn index(n: f64, limit: usize) -> Option<usize> { (n.is_finite() && n >= 0. && n.fract() == 0. && n <= limit as f64).then_some(n as usize) }
    if row.len() < F::WORDS { return None; }
    let tick = row[F::TICK]; if !tick.is_finite() || tick.fract() != 0. || tick.abs() > 9007199254740991. { return None; }
    let at = index(row[F::LINKS], heap.len())?; let count = index(row[F::LINKS + 1], 256)?;
    let words = heap.get(at..at.checked_add(count.checked_mul(L::WORDS)?)?)?;
    for link in words.chunks_exact(L::WORDS) {
        index(link[L::ID], u32::MAX as usize)?;
        for field in [L::HAS_KIND, L::RELAY, L::HAS_FAMILY, L::FAMILY_SIX] { if link[field] != 0. && link[field] != 1. { return None; } }
    }
    Some((tick, words))
}
/// Reserved generated input remains owned by this source through memory growth.
/// # Safety
/// The source is live in this instance until wire_store_source_drop.
#[no_mangle]
pub unsafe extern "C" fn wire_store_world_input(source: *mut Source) -> *mut f64 {
    super::owner::with(|owner| {
if owner.sources.get(source as usize).is_none() {return std::ptr::null_mut();}
 (owner.sources.get_mut(source as usize).unwrap()).input.as_mut_ptr()
    })
}
impl Source {
    // captureSnapshot stages common, unit and world fields in that order.
    // Only a complete capture swaps into the live stagger bank.
    // A late field fault restores the roster clock so retrying is equivalent.
    fn world(&mut self, stagger: &mut super::stagger::Stagger, epoch: f64) -> super::codec::Result<()> {
        let source = self;
    let header = crate::gen::WireWorldFacts::WORDS;
    let input = facts(&source.input[..header], &source.input[header..]);
    let (tick, words) = input.ok_or("invalid canonical world facts")?;
    let roster_clock = source.summaries.roster_clock();
    let result = (|| {
        source.stage.copy_native_from(stagger)?; source.stage.begin();
        let store = &sim().store; let strings = &sim().strings;
        let links = NativeLinks { store, peers: &source.peers, words };
        source.summaries.take(tick, epoch, store, strings, &source.capture, &links, &mut source.stage)?;
        let defs = crate::combat::module::defs();
        source.world.take(tick, store, strings, &defs.weapons, &source.capture, &mut source.stage, crate::units::f3::wire_visible)?;
        source.fields.take(tick, epoch, store, strings, &NativeProps, source.names, &mut source.stage)?;
        source.stage.finish(&[7., 8., 9., 10.]); std::mem::swap(stagger, &mut source.stage); Ok(())
    })();
    if result.is_err() { source.summaries.restore_roster_clock(roster_clock); } result
    }
}

/// The publisher lends disjoint banks for one call. Local handles were bound
/// at admission, and no callback can reenter this borrow.
pub(crate) struct PublicationWorld<'a> {
    pub epoch: f64,
    pub source: &'a mut Source, pub stagger: &'a mut super::stagger::Stagger,
    pub shots: &'a Counters, pub combat: &'a mut super::combat::Capture,
    pub edges:&'a crate::store::handles::Pool<super::edges::Host>,
}
impl super::publisher::World for PublicationWorld<'_> {
    fn edges(&self,handle:usize)->Option<&super::edges::Host> {self.edges.get(handle)}
    // wire_publish validates the complete reserved window before capture.
    // A malformed input returns a normal host fault before any world walk.
    // It must not drain journal facts that a corrected retry still needs.
    // The fixed root header itself is always retained by the Source.
    fn validate(&self) -> super::codec::Result<()> {
        use crate::gen::WireWorldFacts as F;
        facts(&self.source.input[..F::WORDS], &self.source.input[F::WORDS..])
            .ok_or("invalid canonical world facts")?;
        Ok(())
    }

    fn capture(&mut self, tick: f64, epoch: f64, runtime: &mut super::Runtime) -> super::codec::Result<()> {
        self.source.capture(self.shots)?; self.source.world(self.stagger, epoch)?;
        runtime.capture_canonical(tick, epoch, self.source.capture.records(), self.stagger.entries.iter().filter_map(|e| e.field.as_ref()), &sim().strings)
    }
    // SnapshotHost resolves recipients from native connection ownership.
    // Mounted actors use their hull position for cosmetic range selection.
    // Mailbox ACK and discard counters are observations, not native decisions.
    fn actor(&self, peer: u32) -> super::publisher::Actor {
        use crate::gen::*;
        let source = &self.source; let header = WireWorldFacts::WORDS;
        let Some((_, rows)) = facts(&source.input[..header], &source.input[header..]) else { return Default::default() };
        let Some(row) = rows.chunks_exact(WirePublicationLink::WORDS).find(|r| r[WirePublicationLink::ID] == peer as f64) else { return Default::default() };
        let Some(peer) = source.peers.iter().find(|p| p.id == peer) else { return Default::default() };
        let store = &sim().store;
        let unit = peer.character.as_ref().and_then(|h| crate::battle::f4_connections::wire_view(store, h))
            .and_then(|c| c.unit).and_then(|r| store.unit.get(r).map(|u| (r, u)));
        let position = unit.map(|(_, u)| {
            u.seat.and_then(|r| store.seat_lease.get(r)).and_then(|l| l.vehicle).and_then(|r| store.vehicle.get(r)).map(|v| v.position)
                .unwrap_or_else(|| match &u.soldier { UnitBody::Soldier(s) => s.position, UnitBody::Droideka(d) => d.position })
        });
        super::publisher::Actor { present: true, spectator: peer.character.is_none(), unit: unit.map(|(r, u)| (r.slot as f64, u.generation)), position,
            ack: self.source.mailbox(peer.id).map_or(0.,|m|m.state.ack), ack_tick: self.source.mailbox(peer.id).map_or(0.,|m|m.state.ack_tick), discarded_input: self.source.mailbox(peer.id).map_or(0.,|m|m.state.discarded),
            discarded_actions: row[WirePublicationLink::DISCARDED_ACTIONS] }
    }
    fn summaries(&mut self, peer: u32, epoch: f64, fields: &mut super::publisher::Fields) -> super::codec::Result<()> {
        self.source.own_summaries(peer, epoch)?;
        fields.clear(); for field in self.source.own_stagger.entries.iter().filter_map(|e| e.field.as_ref()) { fields.push(field)?; } Ok(())
    }
    fn own(&mut self, peer: u32, epoch: f64) -> super::codec::Result<super::publisher::Correction<'_>> {
        let (id, generation) = self.source.own(peer, epoch)?;
        Ok(super::publisher::Correction { id, generation, schema: 11., payload: &self.source.own.payload.value })
    }
    // CombatCapture.drain runs only after shared capture succeeds.
    // Consume the foreign callback journal first, then native dynamic samples.
    // Their insertion and diagnostic order is part of the publication contract.
    fn events(&mut self, events: &mut super::journal::Events) {
        use crate::gen::WireWorldFacts as F;
        let source = &mut self.source; events.begin();
        events.saturated+=std::mem::take(&mut self.combat.saturated);
        let header = &source.input[..F::WORDS];
        source.projectiles.begin_updates();
        let mut identity_overflow = false;
        for fact in &self.combat.dynamics {if !source.projectiles.update(fact.clone()) {identity_overflow=true;}}
        let incoming=self.combat.len()+source.projectiles.facts.len().min(4096)*2;
        if events.prepare(incoming).is_err() {events.saturated+=incoming as u64;}
        self.combat.publish(events);
        let store = &sim().store; let tick = header[F::TICK];
        source.projectiles.take_with(tick, store, events, |sample| {
            // The raw journal already consumed every earlier callback id.
            // Resolve immutable definitions by handle and qualify owner lives;
            // a retired owner must not inherit its replacement's press edge.
            let id = self.combat.allocate_id();
            let definition_id = source.ordnance_ids.get(sample.definition.0 as usize).copied().filter(|&id| id != 0)
                .ok_or("combat definition absent from matched registry")?;
            let (owner_id, owner_generation, fire_id) = match sample.owner {
                crate::gen::OrdnanceOwner::Unit(r) => store.unit.get(r).map_or((0., 0., 0.), |u| {
                    let net = crate::units::f3::network(r.slot);
                    (r.slot as f64, u.generation, if sample.channel == 0. { net.fire0 } else { net.fire1 })
                }),
                crate::gen::OrdnanceOwner::OwnerSnapshot(r) => store.owner_snapshot.get(r).map_or((0., 0., 0.), |o| (o.id, o.generation, 0.)),
                _ => (0., 0., 0.),
            };
            Ok(crate::gen::WireDynamicFact { id, generation: self.epoch, launch_tick: tick, definition_id: definition_id as f64,
                owner_id, owner_generation, fire_id, channel: sample.channel, ..Default::default() })
        });
        // CombatCapture.fault keeps bounded literal errors in journal order.
        // All possible native sampling messages were interned at attachment.
        // A failed sampler stops its walk but still drains the earlier valid rows.
        if identity_overflow && source.projectiles.error.is_none() { source.projectiles.error = Some("dynamic projectile capture capacity exceeded"); }
        let error = source.projectiles.error.and_then(|e| ["invalid projectile sample", "invalid projectile integer", "invalid projectile channel", "invalid projectile type/orientation", "combat capture capacity exceeded", "dynamic projectile capture capacity exceeded", "combat definition absent from matched registry"].iter().position(|&s| s == e)).map(|at| source.dynamic_faults[at]);
        self.combat.sampled(header[F::TICK], error); self.combat.clear();
    }
    fn created(&self) -> &[crate::gen::WireDynamicFact] { &self.source.projectiles.created }
    fn retired(&self) -> &[u32] { &self.source.projectiles.retired }
    fn status(&self, id: StrId) -> &[u16] { sim().strings.units(id) }
}
