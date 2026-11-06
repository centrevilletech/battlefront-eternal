//! RemoteDroids' world services. Collision, F1 shots and recursive explosions
//! share the owning phase's Store and authority borrow; no service calls JS.
use super::{a, v, Authority, DroidDef, Hit, StrikeDef, WeaponLevel, World};
use crate::abi::sim;
use crate::combat::{module as combat, weapons::Shot};
use crate::gen::*;
use crate::store::{DefId, Ref};
use crate::units::{f3, native};
use three::{Triangle, Vector3};

// Game.createNetworkRemotes supplies the player bound once during setup.
// This changes admission policy without reconstructing prepared droid content.
#[no_mangle]
pub extern "C" fn f3_recon_owners(owners: u32) {
    f3::with_services(|services| {
        services.expect("recon construction services").recon.configure_owners(owners as usize);
    });
}

// These buffers belong to F3's existing instance sidecar. Collision gathers
// and sparse snapshots reuse them, including the first launch of each slot.
pub(crate) struct Services {
    gathered: crate::collision::spheres::Gathered,
    context: F3ReconContextState,
    droid: F3ReconDroidState,
    weapons: Vec<Option<(u32, bool)>>,
    contexts: Vec<Option<Ref<ReconContext>>>,
}
impl Services {
    pub(crate) fn new() -> Self {
        // RemoteDroids.fly can gather on its first launch tick. Prepare the
        // collision pool now instead of warming it with a fake gameplay tick.
        // Neither reservation observes lazy Unit shapes or consumes RNG.
        let mut gathered = crate::collision::spheres::Gathered::new();
        gathered.reserve(256, 16);
        // Context lists can include detached strikes after their owner has
        // retired. Their bound matches the shared network slot capacity,
        // rather than a soldier's two selected weapon channels.
        let mut context = F3ReconContextState::default();
        context.droids.reserve(4096);
        context.strikes.reserve(4096);
        // All droids share one serialization frame. Its weapon views contain
        // clocks only; immutable loadouts stay in the prepared class table.
        // Replacing this Vec on publication would allocate every flight tick.
        let mut droid = F3ReconDroidState::default();
        droid.weapons.reserve(32);
        Self {
            gathered,
            context,
            droid,
            weapons: Vec::new(),
            contexts: Vec::with_capacity(1024),
        }
    }
}

// NetworkRemoteWorld.finish supplies idle controls even when its owner is
// seated or removed. Only that owner's Unit.remoteStep may consume intent.
pub(crate) struct NativeWorld<'a> {
    controls: Option<Ref<Unit>>,
    queries: &'a mut crate::battle::live::Queries,
}
impl<'a> NativeWorld<'a> {
    pub(crate) fn idle(queries: &'a mut crate::battle::live::Queries) -> Self {
        Self {
            controls: None,
            queries,
        }
    }
    pub(crate) fn controlled(owner: Ref<Unit>, queries: &'a mut crate::battle::live::Queries) -> Self {
        Self {
            controls: Some(owner),
            queries,
        }
    }
}
impl World for NativeWorld<'_> {
    // RemoteDroids.aim and SoldierCamera.clearance use the same live world
    // as Unit movement. The cast must see the current hull transforms and
    // enabled flags, including changes made earlier in this outer span.
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit> {
        native::cast(f3::collision_world(), &origin, &direction, far).map(|h| Hit {
            distance: h.distance,
            point: h.point,
            normal: h.normal,
        })
    }
    // RemoteDroids.blast ignores moving bodies for occlusion. Reusing the
    // ordinary aim cast here would let a passing hull shield a droid from
    // an explosion that the JavaScript static-only ray lets through.
    fn static_cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit> {
        native::cast_static(f3::collision_world(), &origin, &direction, far, true).map(|h| Hit {
            distance: h.distance,
            point: h.point,
            normal: h.normal,
        })
    }
    fn gather(&mut self, center: Vector3, radius: f64, out: &mut Vec<Triangle>) {
        // RemoteDroids.fly uses uncached collision.gather. Flatten polygons in
        // their original order; moving hulls are visible at this exact point.
        // The collision world's order supplies its enabled bytes. Gather's
        // internal candidate order is retained when polygons are flattened;
        // reversing triangles can change which contact resolves first.
        let collision = crate::collision::state();
        let world = f3::collision_world();
        let flags = unsafe {
            std::slice::from_raw_parts(
                crate::movement::movement_flags_ptr(),
                collision.worlds[world].order.len(),
            )
        };
        let gathered = &mut f3::recon_services().gathered;
        crate::collision::spheres::gather(
            collision,
            world,
            a(center),
            radius,
            None,
            None,
            -1,
            flags,
            gathered,
        );
        out.clear();
        for polygon in gathered.list() {
            for triangle in &polygon.tris {
                out.push(Triangle::new(v(triangle.a), v(triangle.b), v(triangle.c)));
            }
        }
    }
    // RemoteDroids.fly tests the water grid and uses this tick's bobbed
    // surface. A map with water elsewhere must still return no surface for
    // a dry cell; a global level alone would change its hover height.
    fn water(&self, x: f64, z: f64) -> Option<f64> {
        let map = crate::vehicles::module::map()?;
        map.water.as_ref()?.over(x, z).then_some(map.level)
    }
    // RemoteDroids.control runs after the simulation camera update. Exit
    // edges short-circuit left to right, and zoom is taken only if no exit
    // won. The subsequent soldier combat can consume edges left unvisited.
    fn control_input(
        &mut self,
        store: &mut Store,
        context: Ref<ReconContext>,
        mut input: super::Input,
    ) -> super::Input {
        let c = store.recon_context.get(context).unwrap();
        if c.piloted.is_none() || self.controls != Some(c.owner.reference()) {
            return input;
        }
        let owner = super::key(&c.owner);
        if let Ok(u) = owner.validate_mut(store) {
            // Keep JavaScript's short circuit. Unvisited next-weapon edges
            // remain available to the soldier's combat after control exits.
            input.exit = crate::combat::combat::take(u,crate::combat::combat::Control::Use)
                || crate::combat::combat::take(u,crate::combat::combat::Control::NextPrimary)
                || crate::combat::combat::take(u,crate::combat::combat::Control::PreviousPrimary)
                || crate::combat::combat::take(u,crate::combat::combat::Control::NextSecondary);
            if !input.exit {
                input.zoom = crate::combat::combat::take(u,crate::combat::combat::Control::Zoom);
            }
        }
        input
    }
    // RemoteDroids.shoot takes both press edges only after fly survives.
    // Holding secondary never starts another countdown. The finish pass
    // uses idle controls even if a seated owner's intent still has buttons.
    fn weapon_input(
        &mut self,
        store: &mut Store,
        context: Ref<ReconContext>,
        mut input: super::Input,
    ) -> super::Input {
        let owner = super::key(&store.recon_context.get(context).unwrap().owner);
        if self.controls != Some(owner.reference()) {
            return input;
        }
        if let Ok(u) = owner.validate_mut(store) {
            input.primary = crate::combat::combat::take(u,crate::combat::combat::Control::FirePrimary);
            input.secondary = crate::combat::combat::take(u,crate::combat::combat::Control::FireSecondary);
            input.held = u.intent.buttons[0] > 0.0;
        }
        input
    }
    // captureOrdnanceOwner freezes team, Character and gameplay life before
    // launch. A later rebind keeps these values for flight and detached
    // strikes; resolving the owner's current Unit is only for ray exclusion.
    fn capture_owner(&mut self, store: &mut Store, owner: &F3UnitKey) -> Ref<OwnerSnapshot> {
        let captured = crate::combat::owners::capture(store, owner).expect("recon launch owner");
        crate::combat::owners::created(store, captured).emit(&mut sim().events);
        captured
    }
    // NetworkRemoteWorld.commitLaunch creates a new network identity even
    // when its slot held another droid earlier in this phase. Its facade
    // must exist before a shot or state notice can decode that reference.
    fn created(&mut self, store: &Store, reference: Ref<ReconDroid>) {
        let d = store.recon_droid.get(reference).expect("new recon droid");
        F3ReconDroidCreated {
            slot: reference.slot,
            generation: reference.gen,
            context: d.context,
            owner: super::key(&d.owner),
            launch_owner: d.launch_owner,
            weapon: d.weapon,
            class: d.class,
            team: d.team,
        }
        .emit(&mut sim().events);
    }
    fn replacing(&mut self, store: &Store, droid: Ref<ReconDroid>) {
        // A new launch can reuse a slot before the outer publication. Sample
        // the old terminal state first so its facade never becomes the new life.
        publish_droid(store, droid);
        F3ReconRetired {
            context: false,
            slot: droid.slot,
            generation: droid.gen,
        }
        .emit(&mut sim().events);
    }
    // RemoteDroids.shoot and updateStrikes both enter F1 synchronously.
    // An instant round can kill a Unit or droid and complete a nested death
    // explosion before the next salvo counter is decremented.
    fn fire(
        &mut self,
        authority: &mut Authority,
        store: &mut Store,
        def: u32,
        at: Vector3,
        direction: Vector3,
        owner: Ref<OwnerSnapshot>,
    ) {
        // RemoteDroids.fire uses the ordinary F1 entry, default channel zero.
        // The explicit Store borrow is released before any immediate damage.
        let (_, strings) = crate::ordnance::module::parts();
        let mut world =
            crate::ordnance::module::JsWorld::with_services(store, strings, Some(f3::WorldServices { recon: authority, queries: self.queries }));
        let mut cx = unsafe {
            crate::ordnance::Cx::new(store, strings, crate::ordnance::module::defs(), &mut world)
        };
        cx.fire_round(
            DefId(def),
            a(at),
            a(direction),
            OrdnanceOwner::OwnerSnapshot(owner),
            None,
            None,
            BeamSource::Null,
            0.0,
            None,
            None,
        );
    }
    // RemoteDroids.onFired is a notification after the native fire returns.
    // Sample its position and class now, since subsequent movement or slot
    // reuse must not rename a shot already recorded by the world journal.
    fn fired(&mut self, reference: Ref<ReconDroid>, shot: Shot, at: Vector3) {
        let d = sim()
            .store
            .recon_droid
            .get(reference)
            .expect("fired recon droid");
        let weapon = &d.weapons[shot.weapon as usize];
        let channel = weapon.channel(&combat::defs().weapons);
        F3ReconShot {
            droid: reference,
            owner: d.launch_owner,
            class: d.class,
            weapon: shot.weapon,
            level: weapon.level as u32,
            position: a(at),
            yaw: d.yaw,
            pitch: d.pitch,
            channel,
        }
        .emit(&mut sim().events);
    }
    fn explode(
        &mut self,
        authority: &mut Authority,
        _: &mut Store,
        def: u32,
        at: Vector3,
        owner: Ref<OwnerSnapshot>,
    ) {
        // RemoteDroids.kill commits dead before its explosion reenters the
        // live recon list. The same authority is reborrowed down that chain.
        crate::combat::live::blast(
            DefId(def),
            a(at),
            OrdnanceOwner::OwnerSnapshot(owner),
            Some(f3::WorldServices { recon: authority, queries: self.queries }),
        );
    }
}

// Full views drain only after the outer return. The narrow remotesActive bit
// is separately available on Unit publications consumed by synchronous aimers.
// Remote replay checkpoints read every dynamic weapon timer as well as the
// flight body. Copy those values into retained storage without recomputing
// aim, camera placement or weapon selection in the JavaScript facade.
fn publish_droid(store: &Store, reference: Ref<ReconDroid>) {
    let Some(d) = store.recon_droid.get(reference) else {
        return;
    };
    let out = &mut f3::recon_services().droid;
    out.droid = reference;
    out.position = d.position;
    out.velocity = d.velocity;
    out.yaw = d.yaw;
    out.pitch = d.pitch;
    out.health = d.health;
    out.dead = d.dead;
    out.eye = d.eye;
    out.aim = d.aim;
    out.target = d.target;
    out.countdown = d.countdown;
    // A dead droid can still own an orbital salvo. Dead and salvo are
    // independent fields, so a terminal body snapshot must keep its frame
    // and remaining rounds until updateStrikes drains them.
    out.salvo = d.salvo.as_ref().map(|s| ReconSalvo {
        strike: s.strike,
        at: s.at,
        x: s.x,
        y: s.y,
        z: s.z,
        left: s.left,
        wait: s.wait,
    });
    out.selected = d.selected;
    out.weapons
        .resize_with(d.weapons.len(), F3WeaponView::default);
    for (view, weapon) in out.weapons.iter_mut().zip(&d.weapons) {
        *view = f3::weapon_view(weapon);
    }
    out.emit(&mut sim().events);
}
// NetworkRemoteWorld retains context insertion order across owner deaths.
// Publish only references marked by native mutations; scanning or copying
// all droids after every hook would recreate the old all-body boundary.
pub(crate) fn publish(authority: &mut Authority) {
    for &reference in authority.changed_droids() {
        publish_droid(&sim().store,reference);
    }
    // A pruned context no longer has a row. Its retirement notice uses the
    // raw qualified key so decoding cannot accidentally construct a new
    // context facade in place of the one that has just finished.
    for &reference in authority.changed_contexts() {
        let Some(c) = sim().store.recon_context.get(reference) else {
            F3ReconRetired {
                context: true,
                slot: reference.slot,
                generation: reference.gen,
            }
            .emit(&mut sim().events);
            continue;
        };
        let out = &mut f3::recon_services().context;
        out.context = reference;
        out.owner = super::key(&c.owner);
        out.retired = c.retired;
        out.last_tick = c.last_tick;
        out.seed = c.seed;
        out.first_person = c.first_person;
        out.camera.position = c.camera.position;
        out.camera.rotation = c.camera.rotation;
        out.camera.reach = c.camera.reach;
        out.camera.fresh = c.camera.fresh;
        out.camera.fov = c.camera.fov;
        out.piloted = c.piloted;
        out.look_x = c.look_x;
        out.look_y = c.look_y;
        out.zoom = c.zoom;
        // These lists describe actual launch and strike order. Keep empty
        // lists and dead droids with a pending salvo distinct; captures and
        // the next owner step observe the same ordering.
        out.droids.clear();
        out.droids.extend_from_slice(&c.droids);
        out.strikes.clear();
        out.strikes.extend_from_slice(&c.strikes);
        out.emit(&mut sim().events);
    }
    authority.clear_changes();
}

// Immutable content uses the existing valueWords/read_js encoding. Generated
// wrappers carry all boundaries, including optional handles and variable lists.
#[no_mangle]
pub extern "C" fn f3_recon_class() {
    // RemoteDroids.createDroid uses nullish class defaults, including the
    // misspelled Acceleraton field. The shared value encoding retains NaN,
    // Infinity and absent values that a JSON round trip would lose.
    let (row, heap) = sim().calls.args();
    let input = F3ReconClass::take(row, heap);
    let def = combat::read_js(&input.value, &mut 0, &sim().strings);
    f3::with_services(|authority| {
        let services = authority.unwrap();
        let authority = services.recon;
        let defs = &mut authority.defs;
        while defs.droids.len() <= input.id as usize {
            defs.droids.push(DroidDef::default());
        }
        defs.droids[input.id as usize] =
            DroidDef::read(&def, input.loadout, input.explosion, input.muzzle);
        // This is content construction, before any flight tick. Share the
        // same prepared weapon values as ordinary combat, then reserve each
        // network slot's largest loadout before its first admitted launch.
        defs.weapons = combat::defs().weapons.clone();
        authority.reserve_scratch(4096, 64);
    });
}
#[no_mangle]
pub extern "C" fn f3_recon_level() {
    // RemoteDroids.shoot reads the merged charge level. Numeric ordnance
    // and strike handles keep property and string lookup out of the shot
    // loop, including a destruct weapon that has no ordinary ordnance.
    let (row, heap) = sim().calls.args();
    let input = F3ReconLevel::take(row, heap);
    let def = combat::read_js(&input.value, &mut 0, &sim().strings);
    f3::with_services(|authority| {
        let defs = &mut authority.unwrap().recon.defs;
        defs.levels
            .resize_with(defs.levels.len().max(input.weapon.0 as usize + 1), Vec::new);
        let levels = &mut defs.levels[input.weapon.0 as usize];
        levels.resize(input.level as usize + 1, WeaponLevel::default());
        levels[input.level as usize] =
            WeaponLevel::read(&def, input.ordnance.map(|d| d.0), input.strike);
    });
}
#[no_mangle]
pub extern "C" fn f3_recon_strike() {
    // RemoteDroids.strike takes salvo geometry from the destruct weapon,
    // not from StrikeOrdnanceName. Several such weapons may fire the same
    // F1 class with different counts, angles and scatter.
    let (row, heap) = sim().calls.args();
    let input = F3ReconStrike::take(row, heap);
    let def = combat::read_js(&input.value, &mut 0, &sim().strings);
    f3::with_services(|authority| {
        let defs = &mut authority.unwrap().recon.defs;
        while defs.strikes.len() <= input.id as usize {
            defs.strikes
                .push(StrikeDef::read(&crate::combat::defs::Js::Undefined, 0));
        }
        defs.strikes[input.id as usize] = StrikeDef::read(&def, input.ordnance.0);
    });
}
#[no_mangle]
pub extern "C" fn f3_recon_weapon() {
    // CombatWorld.remoteLaunch has an arsenal index at runtime. Bind its
    // immutable class handle here, along with SwitchImmediately, rather
    // than searching the authored droid table during a throw.
    let (row, heap) = sim().calls.args();
    let input = F3ReconWeapon::take(row, heap);
    let all = &mut f3::recon_services().weapons;
    all.resize(all.len().max(input.weapon.0 as usize + 1), None);
    all[input.weapon.0 as usize] = Some((input.class, input.immediate));
}

/// CombatWorld.remoteOf and NetworkRemoteWorld.step use the current owner's
/// exact life. A context left on a pooled object cannot control its new life.
pub(crate) fn context(owner: Ref<Unit>) -> Option<Ref<ReconContext>> {
    context_for(owner, sim().store.unit.get(owner)?.generation)
}
pub(crate) fn context_for(owner: Ref<Unit>, life: f64) -> Option<Ref<ReconContext>> {
    // Unit.tick and PlayerCombat.update temporarily take the owner out of its
    // row. Their value frame supplies life; a placeholder cannot validate it.
    let reference = *f3::recon_services()
        .contexts
        .get(owner.slot as usize)?
        .as_ref()?;
    let c = sim().store.recon_context.get(reference)?;
    (c.owner.reference() == owner && c.owner.life == life && !c.retired).then_some(reference)
}
// Unit.tick samples this gate before movement. It is canonical context
// state, never a copy-in of JavaScript's piloted pointer. Later native
// deaths refresh the corresponding narrow Unit mirror before an aimer.
pub(crate) fn active(owner: Ref<Unit>) -> Option<bool> {
    context(owner).map(|c| sim().store.recon_context.get(c).unwrap().piloted.is_some())
}
/// MultiplayerWorld rebinds a context before Unit.spawn advances gameplay
/// life. Keep that owner pointer live without rewriting old launch snapshots.
pub(crate) fn spawned(owner: Ref<Unit>, authority: &mut Authority) {
    let Some(Some(reference)) = f3::recon_services()
        .contexts
        .get(owner.slot as usize)
        .copied()
    else {
        return;
    };
    let Some(c) = sim().store.recon_context.get(reference) else {
        return;
    };
    if c.owner.reference() != owner {
        return;
    }
    let life = sim().store.unit.get(owner).unwrap().generation;
    sim()
        .store
        .recon_context
        .get_mut(reference)
        .unwrap()
        .owner
        .life = life;
    authority.mark_context(reference);
    publish_active(authority);
}
pub(crate) fn weapon(definition: DefId) -> (u32, bool) {
    f3::recon_services().weapons[definition.0 as usize].expect("prepared remote weapon")
}
/// Only changed contexts can alter a soldier's remote gate. Refresh this bit
/// before a later aimer; full droid geometry waits for the outer publication.
pub(crate) fn publish_active(authority: &Authority) {
    for &reference in authority.changed_contexts() {
        let Some(c) = sim().store.recon_context.get(reference) else {
            continue;
        };
        let key = super::key(&c.owner);
        let owner = key.reference();
        // A removed body can still have a surviving context and strike.
        // Only its current piloted gate belongs on Unit; flight position
        // and weapon clocks remain on their independent recon rows.
        let active = Some(c.piloted.is_some() && !c.retired);
        let Ok(u) = key.validate_mut(&mut sim().store) else {
            continue;
        };
        if u.remotes_active != active {
            u.remotes_active = active;
            f3::changed(owner);
        }
    }
}

#[no_mangle]
pub extern "C" fn f3_recon_command() -> u32 {
    // Network owner lifecycle is outside the unit span. The one shared row
    // carries an exact owner life and any droid's separate network generation.
    let (row, heap) = sim().calls.args();
    let input = F3ReconBinding::take(row, heap);
    if input.owner.validate(&sim().store).is_err() {
        return 0;
    }
    // This outer guard joins recursive Unit and F1 changes into one event
    // epoch. Restoring the authority borrow precedes the final publication;
    // no JavaScript facade is read while these operations own native rows.
    let _outer = f3::external();
    f3::with_services(|authority| {
        let services = authority.unwrap();
        let authority = services.recon;
        let before = authority.context(&sim().store, input.key);
        match input.action {
            F3ReconAction::Attach | F3ReconAction::Rebind => {
                // Rebinding exits control on the old body too. Old flying
                // droids retain their captured attribution and context order.
                if let Some(previous) = before {
                    let key = super::key(&sim().store.recon_context.get(previous).unwrap().owner);
                    if !super::same(&key, &input.owner) {
                        if let Ok(u) = key.validate_mut(&mut sim().store) {
                            u.remotes_active = Some(false);
                            f3::changed(key.reference());
                        }
                    }
                }
                let owner = input.owner.reference();
                let result = if input.action == F3ReconAction::Attach {
                    authority.attach(&mut sim().store, input.key, input.owner, input.seed)
                } else {
                    authority.rebind(&mut sim().store, input.key, input.owner, input.seed)
                };
                let reference = result.expect("admitted recon owner");
                let all = &mut f3::recon_services().contexts;
                all.resize(all.len().max(owner.slot as usize + 1), None);
                all[owner.slot as usize] = Some(reference);
                sim().store.unit.get_mut(owner).unwrap().combat.has_remotes = true;
                if before.is_none() {
                    let c = sim().store.recon_context.get(reference).unwrap();
                    F3ReconContextCreated {
                        slot: reference.slot,
                        generation: reference.gen,
                        key: input.key,
                        owner: super::key(&c.owner),
                    }
                    .emit(&mut sim().events);
                }
            }
            _ => {
                let Some(reference) = before else { return 0 };
                let c = sim().store.recon_context.get(reference).unwrap();
                if !super::same(&c.owner, &input.owner) {
                    return 0;
                }
                match input.action {
                    // Retirement exits control without killing surviving
                    // droids or cancelling a frozen strike. Core pruning
                    // removes the context only when both lists are empty.
                    F3ReconAction::Retire => authority.retire(&mut sim().store, input.key),
                    F3ReconAction::Exit => authority.exit(&mut sim().store, reference),
                    F3ReconAction::Look => authority.look(
                        &mut sim().store,
                        reference,
                        input.dx,
                        input.dy,
                        input.sensitivity,
                        input.flag,
                    ),
                    // This flag is an input setting. The native camera
                    // observes it on the next authority step; toggling it
                    // alone does not resample aim or move the camera early.
                    F3ReconAction::FirstPerson => {
                        sim()
                            .store
                            .recon_context
                            .get_mut(reference)
                            .unwrap()
                            .first_person = input.flag;
                        authority.mark_context(reference);
                    }
                    // A qualified droid can outlive the soldier who
                    // launched it, but it still belongs to this context.
                    // Refuse another context's slot before changing control.
                    F3ReconAction::Kill | F3ReconAction::Enter => {
                        let droid = input.droid.expect("remote droid command");
                        if !sim()
                            .store
                            .recon_droid
                            .get(droid)
                            .is_some_and(|d| d.context == reference)
                        {
                            return 0;
                        }
                        if input.action == F3ReconAction::Kill {
                            authority.kill(&mut sim().store, &mut NativeWorld::idle(services.queries), droid);
                        } else {
                            authority.enter(&mut sim().store, droid);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        // Damage reached during kill may have changed several Unit rows.
        // The outer guard publishes exactly those touched victims after
        // these recon snapshots, rather than republishing the whole registry.
        publish_active(authority);
        publish(authority);
        1
    })
}

/// Battlefield reads the current piloted context through its explicit Store.
/// The tail may own that Store locally while owner work buffers stay retained.
pub(crate) fn piloted_in(s: &crate::gen::Store, owner: Ref<Unit>) -> Option<Ref<ReconDroid>> {
    let unit = s.unit.get(owner)?;
    let reference = f3::recon_services().contexts.get(owner.slot as usize).copied().flatten()?;
    let c = s.recon_context.get(reference)?;
    if c.owner.reference() != owner || c.owner.life != unit.generation || c.retired { return None; }
    c.piloted.filter(|r| s.recon_droid.get(*r).is_some_and(|d| !d.dead))
}
pub(crate) fn kill_piloted(owner: Ref<Unit>, services: f3::WorldServices<'_>) {
    let Some(droid) = piloted_in(&sim().store, owner) else { return };
    services.recon.kill(&mut sim().store, &mut NativeWorld::idle(services.queries), droid);
}

/// Player and connection class realization rebinds the native context before
/// spawning. Surviving droids keep their immutable launch owner and RNG state.
pub(crate) fn rebind_native(owner:Ref<Unit>, key:u32, seed:u32, services:&mut f3::WorldServices<'_>) {
    let authority=&mut *services.recon;
    let before=authority.context(&sim().store,key);
    let input=F3UnitKey::of(owner,sim().store.unit.get(owner).expect("realized recon owner"));
    if let Some(previous)=before {
        let old=super::key(&sim().store.recon_context.get(previous).unwrap().owner);
        if !super::same(&old,&input) {
            if let Ok(unit)=old.validate_mut(&mut sim().store) {
                unit.remotes_active=Some(false);f3::changed(old.reference());
            }
        }
    }
    let reference=authority.rebind(&mut sim().store,key,input,seed).expect("admitted native recon owner");
    let contexts=&mut f3::recon_services().contexts;
    assert!((owner.slot as usize)<contexts.len(),"reserved native recon owner slots");
    contexts[owner.slot as usize]=Some(reference);
    sim().store.unit.get_mut(owner).unwrap().combat.has_remotes=true;
    if before.is_none() {
        let context=sim().store.recon_context.get(reference).unwrap();
        F3ReconContextCreated {slot:reference.slot,generation:reference.gen,key,
            owner:super::key(&context.owner)}.emit(&mut sim().events);
    }
    publish_active(authority);
}

/// Factory admission reserves context links for future mixed unit ids.
/// The authority retains its own separately admitted owner capacity.
pub(crate) fn reserve_unit_slots(count:usize) {
    let contexts=&mut f3::recon_services().contexts;
    contexts.resize(count.max(contexts.len()),None);
}
