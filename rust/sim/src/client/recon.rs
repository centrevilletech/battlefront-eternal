//! predictRemote runs the authoritative recon core over a private store.
//! Collision and water are borrowed native resources; shots and explosions
//! advance clocks and RNG without emitting authority gameplay or effects.
use super::{append_record, axis, span, BACKWARD, FORWARD, INVALID, LEFT, PRIMARY, RIGHT, TURN_LEFT, TURN_RIGHT};
use crate::collision::{self, State};
use crate::combat::{
    defs::{Js, WeaponClass},
    weapons::Shot,
};
use crate::gen::{
    ClientAction, ClientArsenalTail, ClientCommand, ClientFrame, ClientLook, ClientReconClass, ClientReconState, ClientResult,
    ClientWeapons, F3UnitKey, OwnerSnapshot, ReconContext, ReconDroid, Store, Unit, Weapon,
};
use crate::recon::{Authority, Definitions, DroidDef, Hit, Input, StrikeDef, WeaponLevel, World};
use crate::store::{DefId, Ref};
use crate::vehicles::native::MapRules;
use three::{Triangle, Vector3};

const STEP: f64 = 1.0 / 60.0;
const WEAPONS: usize = 32;
// F3 reserves these gather bounds before launch as well. Overflow keeps the
// bounded prefix rather than growing a paused or malformed display frame.
// Gather caps match the authoritative recon solver's scratch reservation.
// The page reads the same contacts without delegating a triangle operation
// to JavaScript or changing the global collision world's ownership.
const TRIANGLES: usize = 4096;
fn v(a: [f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn bit(bits: u32, action: ClientAction) -> bool {
    bits & (1 << action as u8) != 0
}

// This store belongs to one flown resource, never to the authoritative F3
// world. It keeps the same generation-qualified native ownership links so
// the shared core can apply its usual launch, exit and retirement rules.
// The page only borrows the final mutable state until its next correction.
pub(super) struct Session {
    authority: Authority,
    store: Store,
    context: Ref<ReconContext>,
    droid: Ref<ReconDroid>,
    gathered: collision::spheres::Gathered,
    candidates: Vec<u32>,
    flags: Vec<u8>,
    enabled: Vec<u8>,
    map: MapRules,
}
impl Session {
    // RemotePredictionContext resolves loadout and geometry only when the
    // physical droid or definition changes. No authored object enters replay.
    pub(super) fn new(words: &[f64]) -> Result<Self, &'static str> {
        // The generated configuration separates authored definition words
        // from the bridge's weapon ids. Both spans are checked before any
        // store is created, so a partial class cannot become a live session.
        // Resource construction and this configuration share one droid life.
        let row = ClientReconClass::take(span(words, 0, 1, ClientReconClass::WORDS)?, &[]);
        let ids = span(words, row.ids_at, row.count.min(WEAPONS as u32), 1)?;
        let def = crate::prediction::weapons::read_definition(words.get(row.definition_at as usize..).ok_or(INVALID)?)?;
        let authored = def.get("droid");
        if authored.get("weapons").len() != ids.len() {
            return Err(INVALID);
        }
        // Weapon classes resolve once at admission. Charge levels share their
        // wire definition id while their strike definitions have dense native
        // indices. Keeping that distinction preserves restored salvo cursors.
        // No frame traverses or serializes the authored property tree.
        let mut defs = Definitions::default();
        let mut loadout = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let id = DefId(id as u32);
            if id.0 > 65535 {
                return Err(INVALID);
            }
            let Js::Obj(value) = authored.get("weapons").at(i) else {
                return Err(INVALID);
            };
            let class = WeaponClass::new(value.clone());
            let count = class.levels.len();
            let index = id.0 as usize;
            if defs.levels.len() <= index {
                defs.levels.resize_with(index + 1, Vec::new);
            }
            for n in 0..count {
                let mut source = (**value).clone();
                if n > 0 {
                    if let Js::Obj(charge) = value.get("charges").at(n - 1) {
                        source = source.spread(charge);
                    }
                }
                let source = Js::Obj(std::rc::Rc::new(source));
                // Strike definitions are needed even in a quiet prediction:
                // their delays, scatter and salvo basis affect future state.
                // Suppressing a fire callback must not suppress those draws.
                let strike = if source.get("strike").truthy() {
                    let at = defs.strikes.len() as u32;
                    defs.strikes.push(StrikeDef::read(&source, 0));
                    Some(at)
                } else {
                    None
                };
                defs.levels[index].push(WeaponLevel::read(&source, source.get("ordnance").truthy().then_some(0), strike));
            }
            defs.weapons.set(id, class);
            loadout.push(id);
        }
        defs.droids.push(DroidDef::read(authored, loadout, None, row.muzzle));
        // Reuse F3's full controller rather than composing small WASM methods.
        // Its private order, change notices, shots and collision scratch all
        // reserve before replay. Calls below never borrow a JS service object.
        let mut authority = Authority::new(defs, 1, 1);
        authority.reserve_scratch(TRIANGLES, 128);
        let mut store = Store::default();
        let unit = store.unit.insert(Unit { generation: 1., alive: true, ..Default::default() });
        let owner = F3UnitKey { slot: unit.slot, generation: unit.gen, life: 1. };
        let context = authority.attach(&mut store, 0, owner.clone(), 1).map_err(|_| INVALID)?;
        // The existing launch snapshot is inert identity in this private
        // store. Quiet fire and explosion methods below never publish it to
        // authoritative ordnance or create a gameplay owner on the host.
        let launch_owner = store.owner_snapshot.insert(OwnerSnapshot::default());
        // Allocate the one physical row and its largest mutable arsenal now.
        // A dead replay can remove it from context order without freeing it.
        let droid = store.recon_droid.insert(ReconDroid {
            context,
            owner,
            launch_owner,
            weapons: Vec::with_capacity(WEAPONS),
            ..Default::default()
        });
        store.recon_context.get_mut(context).unwrap().droids.push(droid);
        let mut gathered = collision::spheres::Gathered::new();
        gathered.reserve(256, 16);
        // ClientHullContent has already admitted the battlefield's immutable
        // water cells to this simulation module, even on maps with no hulls.
        // Only its oscillating level changes during play; import that scalar
        // with the checkpoint rather than rebuilding a water object per tick.
        #[cfg(target_arch = "wasm32")]
        let map = crate::vehicles::module::map().cloned().unwrap_or_else(|| MapRules {
            water: None,
            level: f64::NAN,
            kill_y: -500.,
            regions: Vec::new(),
        });
        #[cfg(not(target_arch = "wasm32"))]
        let map = MapRules { water: None, level: f64::NAN, kill_y: -500., regions: Vec::new() };
        Ok(Self {
            authority,
            store,
            context,
            droid,
            gathered,
            map,
            candidates: vec![0; collision::MAX_BODIES],
            flags: Vec::with_capacity(collision::MAX_BODIES),
            enabled: vec![0; collision::MAX_BODIES],
        })
    }

    // restoreRemoteReplay replaces mutable rows only. Keep weapon and context
    // capacities even after a prior replay exited, crashed or drained a salvo.
    fn restore(&mut self, row: &ClientReconState, words: &[f64], frame: &ClientFrame) -> Result<(), &'static str> {
        let header = ClientWeapons::take(span(words, frame.weapons_at, 1, ClientWeapons::WORDS)?, &[]);
        let count = (header.count as usize).min(WEAPONS);
        let at = frame.weapons_at as usize + ClientWeapons::WORDS;
        let rows = span(words, at as u32, count as u32, Weapon::WORDS)?;
        let tail = ClientArsenalTail::take(span(words, (at + rows.len()) as u32, 1, ClientArsenalTail::WORDS)?, &[]);
        // A prediction may have exited or killed its private droid last frame.
        // Restore the same reserved row, arsenal and context membership before
        // replaying again. Authoritative packets remain the sole checkpoint.
        // Clearing these vectors keeps their admission-time reservations.
        let d = self.store.recon_droid.get_mut(self.droid).unwrap();
        d.position = row.position;
        d.velocity = row.velocity;
        d.eye = row.eye;
        d.aim = row.aim;
        d.target = row.target;
        d.yaw = row.yaw;
        d.pitch = row.pitch;
        d.health = row.health;
        d.dead = row.dead;
        d.countdown = row.countdown;
        d.team = row.team;
        // Salvo and camera clones contain fixed numerical records only.
        // The arsenal vector retains its reservation while each weapon row
        // restores full mutable state, including its random cursor and level.
        d.salvo = row.salvo.clone();
        d.weapons.clear();
        for words in rows.chunks_exact(Weapon::WORDS) {
            d.weapons.push(Weapon::take(words, &[]));
        }
        d.selected = [tail.primary, tail.secondary];
        // The computational camera is controller state: reconstructing it from
        // the displayed camera changes target rays after a short replay gap.
        // Preserve its easing history and optional first-person packet value.
        let c = self.store.recon_context.get_mut(self.context).unwrap();
        c.last_tick = frame.authority_tick;
        c.look_x = row.look_x;
        c.look_y = row.look_y;
        c.zoom = row.zoom;
        c.first_person = row.first_person.unwrap_or(false);
        c.seed = row.seed;
        c.piloted = (row.piloted && !row.dead).then_some(self.droid);
        c.camera = row.camera.clone();
        if !row.has_camera {
            c.camera = crate::gen::ReconCamera {
                rotation: [0., 0., 0., 1.],
                reach: 1.,
                fresh: true,
                fov: self.authority.defs.droids[0].third_fov,
                ..Default::default()
            };
        }
        c.droids.clear();
        c.droids.push(self.droid);
        c.strikes.clear();
        if row.salvo.is_some() {
            c.strikes.push(self.droid);
        }
        Ok(())
    }

    // replayTicks keeps source command order, unsent action edges and held
    // fallback. The 120-tick endpoint is frozen before consuming the stream.
    pub(super) fn predict(
        &mut self,
        frame: &ClientFrame,
        words: &[f64],
        commands: &[f64],
        collision: &mut State,
        output: &mut Vec<f64>,
    ) -> Result<(), &'static str> {
        // Replay always starts at the received checkpoint. Infantry and hull
        // continuation eligibility does not apply to the recon controller:
        // its camera, countdown and exit state were restored on every frame.
        // This also keeps a rewind independent of the prior predicted view.
        let mut row = ClientReconState::take(span(words, frame.recon_at, 1, ClientReconState::WORDS)?, &[]);
        self.restore(&row, words, frame)?;
        self.map.level = if row.has_water_level { row.water_level } else { f64::NAN };
        let flags = span(words, frame.flags_at, frame.flags_count, 1)?;
        if collision.worlds.get(frame.world as usize).is_none_or(|w| w.order.len() != flags.len()) {
            return Err(INVALID);
        }
        self.flags.clear();
        self.flags.extend(flags.iter().map(|&f| u8::from(f != 0.)));
        // A missing correction must not make the page replay an unbounded
        // blackout. Keep the original two-second horizon and command order.
        // An input stamped before the checkpoint can still carry an action;
        // the core consumes it on the first eligible replay tick below.
        let from = frame.authority_tick;
        let mut last = from;
        let mut mine = false;
        for words in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(words, &[]);
            if c.tick <= from + 120. && c.generation == frame.generation {
                mine = true;
                last = last.max(c.tick);
            }
        }
        // replayTicks advanced once for rolled-back commands of this life,
        // even when none could extend the endpoint. Another life's commands
        // alone advance nothing. These are observable weapon-timer semantics.
        if mine && last == from {
            last = from + 1.;
        }
        let mut held = frame.held;
        let mut scale = frame.mouse_scale;
        let mut invert = frame.invert;
        // Missing view facts retain the restored context. Explicit false
        // changes perspective; treating it like an absent value changes aim
        // and camera pullback on the first tick after a correction gap.
        let mut first = row.view_first;
        let mut aspect = row.has_view_aspect.then_some(row.view_aspect);
        let mut at = 0;
        let mut tick = from;
        let mut ticks = 0;
        let mut world = Environment {
            collision,
            world: frame.world as usize,
            flags: &self.flags,
            gathered: &mut self.gathered,
            candidates: &mut self.candidates,
            enabled: &mut self.enabled,
            map: &self.map,
        };
        // The own wire envelope carries position and angles, not eye/target
        // vectors. Restore their derived authority pose before replay, then
        // return that checkpoint separately so host restore never reintroduces
        // the resource's uninitialized vectors on an empty prediction frame.
        if row.needs_aim {
            self.authority.aim(&mut self.store,&mut world,self.droid);
            let d=self.store.recon_droid.get(self.droid).unwrap();
            row.eye=d.eye;row.aim=d.aim;row.target=d.target;row.needs_aim=false;
        }
        let source=row.clone();
        while mine && tick < last && ticks < 120 {
            tick += 1.;
            ticks += 1;
            let mut actions = 0;
            while at < commands.len() {
                let c = ClientCommand::take(&commands[at..], &[]);
                if c.tick > from + 120. {
                    at += ClientCommand::WORDS;
                    continue;
                }
                if c.tick > tick {
                    break;
                }
                at += ClientCommand::WORDS;
                if c.generation != frame.generation {
                    continue;
                }
                // Reliable actions survive a failed movement publication.
                // Held state, view settings and look only change when the
                // replaceable command actually reached the world.
                actions |= c.actions;
                if !c.sent {
                    continue;
                }
                held = c.held;
                if c.has_mouse_scale {
                    scale = c.mouse_scale;
                }
                if let Some(value) = c.invert {
                    invert = value;
                }
                if let Some(value) = c.first_person {
                    first = Some(value);
                }
                if c.has_aspect {
                    aspect = Some(c.aspect);
                }
                let looks = span(words, c.look_at, c.look_count, ClientLook::WORDS)?;
                if self.store.recon_context.get(self.context).unwrap().piloted.is_some() {
                    for look in looks.chunks_exact(ClientLook::WORDS) {
                        self.authority.look(
                            &mut self.store,
                            self.context,
                            look[0].clamp(-10000., 10000.),
                            look[1].clamp(-10000., 10000.),
                            scale.min(2.),
                            invert,
                        );
                    }
                }
            }
            let c = self.store.recon_context.get_mut(self.context).unwrap();
            if let Some(value) = first {
                c.first_person = value;
            }
            if let Some(value) = aspect.filter(|a| a.is_finite() && *a > 0.) {
                row.aspect = value;
            }
            // F3 consumes the same action names the page previously handed
            // to its JavaScript controller. Primary held fire remains distinct
            // from a discrete press, and cycling a weapon hands control back.
            // These flags are quiet intent, never calls into the page owner.
            let input = Input {
                forward: axis(held, FORWARD, BACKWARD),
                right: axis(held, RIGHT, LEFT),
                turn: axis(held, TURN_RIGHT, TURN_LEFT),
                exit: bit(actions, ClientAction::Use)
                    || bit(actions, ClientAction::NextPrimary)
                    || bit(actions, ClientAction::PreviousPrimary)
                    || bit(actions, ClientAction::NextSecondary),
                zoom: bit(actions, ClientAction::Zoom),
                primary: bit(actions, ClientAction::FirePrimary),
                held: held & PRIMARY != 0,
                secondary: bit(actions, ClientAction::FireSecondary),
            };
            // F3 runs camera/aim, control, weapons, movement and strike finish
            // in its established order. Quiet World methods suppress side
            // effects while the core still advances clocks and random state.
            self.authority.clear_changes();
            self.authority.step(&mut self.store, &mut world, 0, tick, STEP, input).map_err(|_| INVALID)?;
        }
        let d = self.store.recon_droid.get(self.droid).unwrap();
        let c = self.store.recon_context.get(self.context).unwrap();
        // Publication is a single stable block. Preserve every value the page
        // still reads, including the camera, ammo and pending strike basis.
        // The host copies into retained resources and restores its saved block
        // before applying another correction; no physics callback escapes here.
        row.position = d.position;
        row.velocity = d.velocity;
        row.eye = d.eye;
        row.aim = d.aim;
        row.target = d.target;
        row.yaw = d.yaw;
        row.pitch = d.pitch;
        row.health = d.health;
        row.dead = d.dead;
        row.countdown = d.countdown;
        row.salvo = d.salvo.clone();
        row.piloted = c.piloted == Some(self.droid);
        row.seed = c.seed;
        row.look_x = c.look_x;
        row.look_y = c.look_y;
        row.zoom = c.zoom;
        row.first_person = if first.is_some() || ticks > 0 && row.piloted { Some(c.first_person) } else { row.first_person };
        row.camera = c.camera.clone();
        row.has_camera = row.piloted;
        output.clear();
        output.resize(ClientResult::WORDS, 0.);
        let mut result = ClientResult { tick, ticks, ..Default::default() };
        result.recon_source_at=append_record(output,ClientReconState::WORDS,&source,ClientReconState::put);
        result.recon_at = append_record(output, ClientReconState::WORDS, &row, ClientReconState::put);
        result.recon_words = ClientReconState::WORDS as u32;
        result.weapons_at =
            append_record(output, ClientWeapons::WORDS, &ClientWeapons { count: d.weapons.len() as u32 }, ClientWeapons::put);
        for weapon in &d.weapons {
            append_record(output, Weapon::WORDS, weapon, Weapon::put);
        }
        append_record(
            output,
            ClientArsenalTail::WORDS,
            &ClientArsenalTail { primary: d.selected[0], secondary: d.selected[1], zoom_step: 0., barrels_count: 0 },
            ClientArsenalTail::put,
        );
        result.weapons_words = output.len() as u32 - result.weapons_at;
        // The result has no shot notices: F3 already advanced their numerical
        // consequences in this private store. Point the empty notice tail past
        // every state word so one host copy includes the complete publication.
        result.shots_at = output.len() as u32;
        result.put(&mut output[..ClientResult::WORDS], &mut Vec::new());
        Ok(())
    }
}

// F3's World operations borrow the page's collision geometry directly. No
// foreign callback or per-ray WASM crossing occurs inside recon replay.
// Native collision borrows the same immutable world as normal prediction.
// Gathered polygons and ray candidates are private scratch; the environment
// never moves a body or changes its enabled flag while a droid is predicted.
struct Environment<'a> {
    collision: &'a mut State,
    world: usize,
    flags: &'a [u8],
    enabled: &'a mut [u8],
    gathered: &'a mut collision::spheres::Gathered,
    candidates: &'a mut [u32],
    map: &'a MapRules,
}
impl World for Environment<'_> {
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit> {
        let ray = collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        let n = collision::world_candidates_into(self.collision, self.world, &ray, -1, false, false, self.candidates);
        // Ray traversal indexes its candidate list, not the world's body
        // table. Pair each enabled flag with that candidate id explicitly;
        // passing the whole table here silently changes wall/ground contacts.
        for i in 0..n {
            self.enabled[i] = self.flags[self.candidates[i] as usize];
        }
        let (kind, h) = collision::world_cast_ray_in(
            self.collision,
            self.world,
            &ray,
            far,
            v8math::hypot(direction.x, direction.z),
            &self.candidates[..n],
            &self.enabled[..n],
        );
        (kind != collision::MISS).then(|| Hit {
            distance: h.distance,
            point: Vector3::new(h.px, h.py, h.pz),
            normal: Vector3::new(h.nx, h.ny, h.nz),
        })
    }
    // The old replay environment exposed the page collision world's nearest
    // wall query here too. Retain its filtering and tie order instead of
    // introducing a different static-only broad phase for the chase camera.
    fn static_cast(&mut self, origin: Vector3, direction: Vector3, far: f64) -> Option<Hit> {
        self.cast(origin, direction, far)
    }
    fn gather(&mut self, center: Vector3, radius: f64, out: &mut Vec<Triangle>) {
        collision::spheres::gather(
            self.collision,
            self.world,
            [center.x, center.y, center.z],
            radius,
            None,
            None,
            -1,
            self.flags,
            self.gathered,
        );
        // Capacity is content-sized before admission. A dense overlap keeps
        // the same bounded prefix instead of reallocating while a frame draws.
        // The core applies its ordinary contact solver to these triangles.
        out.clear();
        for polygon in self.gathered.list() {
            for t in &polygon.tris {
                if out.len() == out.capacity() {
                    return;
                }
                out.push(Triangle::new(v(t.a), v(t.b), v(t.c)));
            }
        }
    }
    fn water(&self, x: f64, z: f64) -> Option<f64> {
        self.map.water.as_ref().filter(|w| w.over(x, z)).map(|_| self.map.level)
    }
    // Prediction observes no authority damage, lights, trails or owner
    // capture. These hooks deliberately carry no host callback, allocation
    // or event lifetime. The shared core still computes the shot and salvo.
    fn capture_owner(&mut self, _: &mut Store, _: &F3UnitKey) -> Ref<OwnerSnapshot> {
        Ref::default()
    }
    fn fire(&mut self, _: &mut Authority, _: &mut Store, _: u32, _: Vector3, _: Vector3, _: Ref<OwnerSnapshot>) {}
    fn fired(&mut self, _: &Store, _: Ref<ReconDroid>, _: Shot, _: Vector3) {}
    fn explode(&mut self, _: &mut Authority, _: &mut Store, _: u32, _: Vector3, _: Ref<OwnerSnapshot>) {}
}
