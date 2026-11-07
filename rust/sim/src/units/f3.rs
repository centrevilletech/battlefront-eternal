//! The persistent Unit command boundary. New bodies are adopted once from
//! UnitFactory.build; ordinary inputs never contain a body or shape cache.
use super::{
    module,
    ownership::{self, Ownership},
};
use crate::abi::sim;
use crate::combat::{combat::Edges, services::WeaponService};
use crate::gen::*;
use crate::store::{block::Block, Ref};
use std::ptr::addr_of_mut;

const INPUT_WORDS:usize=PHASE_INPUT_WORDS as usize;
const RESERVED_UNITS: usize = 1024;
const RANDOM_CAPACITY: usize = 128;
const NO_STREAM:u32=NO_RANDOM_STREAM;
const AIM_CAPACITY: usize = 1024;
// The native held record and the foreign input scratch coexist here, rather
// than replacing a Vec-bearing enum for every alternating player/bot row.
// Only the selected generated variant is encoded into the publication.
#[derive(Default)]
struct PublicationSnapshot {value:F3Publication,held:Option<F3NativeIntent>}
impl PublicationSnapshot {
    fn put_controls(&self,row:&mut [f64],heap:&mut Vec<f64>) {
        if let Some(held)=&self.held {
            F3PublishedControls::F3NativeIntent(held.clone()).put(&mut row[F3Publication::CONTROLS..],heap);
        }else{self.value.put_controls(row,heap);}
    }
    fn put(&self,row:&mut [f64],heap:&mut Vec<f64>) {
        self.value.put_body(row,heap);self.value.put_arsenal(row,heap);
        self.value.put_combat(row,heap);self.put_controls(row,heap);
    }
}
struct State {
    // UnitFactory's order and pools are bookkeeping over generated rows.
    // No second copy of body/combat state lives in this sidecar.
    owner: Ownership,
    // The literal player owner resolves credit through this body, while its
    // original no-reference launch and ray-exclusion rules stay distinct.
    viewpoint: Option<Ref<Unit>>,
    separation: [f64; F3Separation::WORDS],
    pushers: super::separate::Pushers,
    world: usize,
    // Intent/foreign rows and scalar commands have separate reserved storage.
    // Hooks can consume a command without replacing the phase's input facts.
    input: Vec<f64>,
    command: [f64; F3Change::WORDS],
    layout: F3Boundary,
    descriptor: [f64; F3Boundary::WORDS],
    block: Block,
    services: Vec<Vec<WeaponService>>,
    items: super::f3_items::Items,
    props: super::f3_props::Props,
    recon: Option<crate::recon::Authority>,
    recon_borrowed: bool,
    prop_queries: crate::battle::live::Queries,
    recon_services: crate::recon::module::Services,
    tick: f64,
    infos: Vec<UnitsPhaseUnit>,
    edges: Vec<Edges>,
    // One reusable row stages each touched Unit. Shape sensor output has its
    // own block so AI can query during a publication without destroying it.
    snapshot: PublicationSnapshot,
    publication: Vec<crate::vehicles::f2_delta::Cache>,
    // Ordinary completed ticks have no cross-unit weapon readers inside
    // aimers. Keep their references for one outer publication; explicit
    // damage, lifecycle and seat barriers can still flush them immediately.
    completed: Vec<Ref<Unit>>,
    // AI holds axes and aim in canonical input. Publishing those changes
    // must not re-encode an unchanged body, arsenal or every weapon clock.
    intent_changed: Vec<Ref<Unit>>,
    intent_marked: Vec<Option<u32>>,
    // Avoidance and weapon tuning join the promised following unit phase.
    // Retain full-row touches without encoding a second body and arsenal.
    ai_full_marked: Vec<Option<u32>>,
    // Foreign rows name only current qualified bodies. Prepared slot lookup
    // restores the canonical factory order without a per-body host record.
    phase_inputs: Vec<Option<usize>>,
    phase_input_slots: Vec<usize>,
    publication_ready: Vec<Ref<Unit>>,
    publication_marks: Vec<Option<u32>>,
    acknowledgement:[f64;F3Acknowledgement::WORDS],
    sensor_block: Block,
    shape: F3ShapeView,
    sensor_result: F3SensorResult,
    bounds_cache:Vec<f64>,
    foreign: Vec<F3PropFact>,
    foreign_header: [f64; F3ForeignHeader::WORDS],
    bodies: Vec<Option<Ref<Body>>>,
    // PlayerCombat resolves its health scale from the class at construction.
    // Victims read the numeric index without decoding a String during a blast.
    health: Vec<crate::combat::impacts::HealthScale>,
    categories: Vec<u32>,
    bot_targets: Vec<Option<Option<Member>>>,
    native_intents:Vec<Option<Ref<Unit>>>,
    hidden_crew: Vec<Option<(Ref<Unit>,f64)>>,
    missile_classes: Vec<Option<FlightClass>>,
    network: Vec<F3NetworkView>,
    // PlayerCombat.reset replaces the arsenal; reconnect only changes life.
    // Keep those two identity events distinct for pooled mirror readers.
    arsenals: Vec<u32>,
    hit_policies: Vec<Option<crate::combat::impacts::HitPacket>>,
    blasts: Vec<Option<u32>>,
    friendly_fire: bool,
    // Notifications reuse the shared generated event stream. An epoch resets
    // only at an outer entry; aimer commands append to their surrounding phase.
    event_epoch: u32,
    running: bool,
    failed:bool,
    controls_consumed: bool,
    relations: Vec<f64>,
    relation_sides:usize,
    history: crate::combat::history::History<Member>,
    // Draw callbacks are construction bindings. A phase exchanges the small
    // generated states directly, including the shared AI generator at aimers.
    random_states: Vec<f64>,
    shared_random: Vec<Option<crate::ai::random::SharedRandom>>,
    shared_bindings: Vec<u32>,
    random_epoch:[f64;F3RandomEpoch::WORDS],
    random_globals: [f64; F3RandomGlobals::WORDS],
    random_bindings: Vec<Vec<u32>>,
    random_sequence: u32,
    aim_input: Vec<f64>,
    aim_header: [f64; F3AimHeader::WORDS],
    empty_text: crate::store::StrId,
    melee: crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey>,
    arc: Vec<crate::combat::live::ArcVictim>,
    // Native F1 calls can nest through immediate death blasts. Each world
    // frame borrows its own retained fat-ray query rather than cloning it.
    ordnance_queries: Vec<Vec<Ref<Unit>>>,
    ordnance_depth: usize,
}
static mut STATE: Option<State> = None;
fn state() -> &'static mut State {
    // One module thread. No state borrow is retained across a foreign import.
    unsafe { (&mut *addr_of_mut!(STATE)).as_mut().expect("F3 initialized") }
}
// A battle owns one instance. This identifies ownership, not whether a tick
// is currently on the stack: supply and damage commands between ticks still
// use canonical state and the same native collision/ordnance services.
pub(crate) fn active() -> bool {
    unsafe { (&*addr_of_mut!(STATE)).is_some() }
}

/// F1 continues stepping ordnance after the unit span. Its native hits use the
/// same changed rows and event epoch, then publish once at that outer boundary.
/// An instant shot inside F3 joins its existing span instead of resetting it.
pub(crate) struct External(bool);
pub(crate) fn external() -> External {
    let outer = active() && !state().running;
    if outer { begin_events(); state().running = true; }
    External(outer)
}
impl External {
    pub(crate) fn is_outer(&self)->bool {self.0}
    /// Game.step immediately enters the native unit owner after AI. Ordinary
    /// held controls can wait for that owner's completed body publication;
    /// damage, construction and synchronous foreign barriers stay immediate.
    pub(crate) fn before_units(mut self) {self.finish(false);}
    fn finish(&mut self,publish_intents:bool) {
        if self.0 {
            // The native export returned its authority borrow before this
            // guard. Recon views precede the final structural descriptor.
            with_services(|authority| {
                if let Some(services) = authority {
                    let authority = services.recon;
                    crate::recon::module::publish_active(authority);
                    crate::recon::module::publish(authority);
                }
            });
            // An event-free ordinary AI return has no foreign observer
            // before the promised Unit.tick span. Keep owner publications
            // together there; structural writes and notices stay immediate.
            // Shared RNG seeds still reach the facade before F3 pushes them.
            let deferred=!publish_intents && state().owner.changed().is_empty()
                && state().completed.is_empty() && sim().events.is_empty()
                && !crate::vehicles::module::f2::has_pending();
            if deferred {descriptor();}else{flush_completed();publish_changed_at(None,publish_intents);}
            state().running=false;self.0=false;
        }
    }
}
impl Drop for External {fn drop(&mut self) {self.finish(true);}}

// Unit.tick and Unit.spawn consume or reset controls. Scalar commands between
// ticks do neither: publishing their old native input would erase the next AI
// or network action already waiting on the retained JS intent object.
pub(crate) struct ControlSpan(Option<bool>);
pub(crate) fn control_span(consumed: bool) -> ControlSpan {
    if !active() { return ControlSpan(None); }
    let previous = state().controls_consumed;
    state().controls_consumed = consumed;
    ControlSpan(Some(previous))
}
impl Drop for ControlSpan {
    fn drop(&mut self) {
        if let Some(previous) = self.0 { state().controls_consumed = previous; }
    }
}

// UnitFactory's soldiers and F1 rounds share the world's prepared collision
// tree. A unit does not create a second collision world when it is pooled.
pub(crate) fn collision_world() -> usize {
    state().world
}
pub(crate) fn tick() -> f64 { state().tick }
pub(crate) fn viewpoint() -> Ref<Unit> { state().viewpoint.expect("registered player viewpoint") }
pub(crate) fn built() -> &'static [Ref<Unit>] { state().owner.built() }
pub(crate) fn unit_revision() -> u64 { state().owner.revision() }
// Vehicles.presentRiders keeps a hidden destroyed crew out of sight for
// that gameplay life. Pool reuse and slot reuse each invalidate this stamp.
pub(crate) fn hide_destroyed_crew(reference: Ref<Unit>) {
    let unit = sim().store.unit.get(reference).expect("destroyed crew Unit");
    state().hidden_crew[reference.slot as usize] = Some((reference,unit.generation));
    state().owner.mark(reference);
}
pub(crate) fn unit_reference(slot: u32) -> Ref<Unit> {
    state().owner.reference(slot).expect("owned seat unit")
}
pub(crate) fn recon_services() -> &'static mut crate::recon::module::Services {
    &mut state().recon_services
}
pub(crate) fn props() -> &'static mut super::f3_props::Props { &mut state().props }
pub(crate) fn foreign_barrier() { publish_changed(); }
pub(crate) fn items() -> &'static mut super::f3_items::Items { &mut state().items }
pub(crate) fn item_target_input(index: usize) -> F3ItemTarget {
    let at = index * F3ItemTarget::WORDS;
    F3ItemTarget::take(&state().input[at..], &[])
}
/// The authoritative camera finishes before the AI/unit interval. Replace
/// only the separation position; global/network scope remains an input fact.
pub(crate) fn world_camera(position: [f64;3]) {
    let mut row = F3Separation::take(&state().separation, &[]);
    row.camera = position;
    row.put(&mut state().separation, &mut Vec::new());
}
/// The enclosing world chooses target membership after ordnance, using the
/// same generated reservation as the standalone item entry.
pub(crate) fn world_item_target(index: usize, row: &F3ItemTarget) {
    let at = index * F3ItemTarget::WORDS;
    row.put(&mut state().input[at..at + F3ItemTarget::WORDS], &mut Vec::new());
}
/// Game.onExplode and LiveObjects share their mutable query vectors through
/// recursive damage. The outer entry takes both resources once; nested F1,
/// prop and recon services reborrow them without another global acquisition.
pub struct WorldServices<'a> {
    pub ai: Option<&'a mut dyn crate::ai::owned::callbacks::Callbacks>,
    pub recon: &'a mut crate::recon::Authority,
    pub queries: &'a mut crate::battle::live::Queries,
}
impl WorldServices<'_> {
    /// A nested service reborrows the already active Core, never a global
    /// AI owner. False means this outer entry has no native AI phase bound.
    pub(crate) fn aim(&mut self,key:&F3UnitKey,dt:f64)->bool {
        let Some(ai)=self.ai.as_mut() else{return false};ai.aim(key,dt,self.recon,self.queries);true
    }
    pub(crate) fn lifecycle(&mut self,hook:&F4AIHook)->bool {
        let Some(ai)=self.ai.as_mut() else{return false};ai.lifecycle(hook,self.recon,self.queries);true
    }

    pub(crate) fn reborrow(&mut self) -> WorldServices<'_> {
        WorldServices {
            ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),
            recon: self.recon,
            queries: self.queries,
        }
    }
}
pub(crate) fn with_services<T>(run: impl FnOnce(Option<WorldServices<'_>>) -> T) -> T {
    with_bare_services(|services|match services {
        Some(services)=>crate::ai::owned::entry::with_callbacks(services.recon,services.queries,|services|run(Some(services))),
        None=>run(None),
    })
}
/// The AI phase already owns its Core. It takes physical services without
/// first installing another callback scope over that enclosing AI instance.
pub(crate) fn with_bare_services<T>(run:impl FnOnce(Option<WorldServices<'_>>)->T)->T {
    if !active() { return run(None); }
    // A foreign callback cannot acquire the phase's services again. Report
    // the rejected boundary without trapping the entire battle instance.
    if state().recon_borrowed { fail(); return run(None); }
    let Some(mut authority) = state().recon.take() else { fail(); return run(None); };
    state().recon_borrowed = true;
    let mut queries = std::mem::take(&mut state().prop_queries);
    let result = run(Some(WorldServices {
        ai:None,
        recon: &mut authority,
        queries: &mut queries,
    }));
    state().recon = Some(authority);
    state().prop_queries = queries;
    state().recon_borrowed = false;
    result
}
// CombatWorld.fireRound/dispense resolve definitions once at construction.
// The immutable service remains at this address across synchronous hit hooks;
// no definition is installed by a weapon update in the middle of a phase.
pub(crate) fn weapon_service(weapon: crate::store::DefId, level: f64) -> *const WeaponService {
    &state().services[weapon.0 as usize][level as usize]
}

// UnitFactory construction reserves phase storage before units begin ticking.
// Inputs and the descriptor keep their addresses for the battle; output is a
// reused block whose current address is written into that descriptor.
// Large scratch capacities are not per-tick work and do not import any bodies.
#[no_mangle]
pub extern "C" fn f3_init(world: u32, level: f64, body_serial: u32) -> *const f64 {
    super::f3_hulls::body_serial(body_serial);
    super::f3_construction::reset();
    if let Some(collision) = crate::vehicles::module::collision_world() {
        assert_eq!(collision.world, world as usize, "one battle collision world");
        // Movement's enabled-byte reservation is fixed for the instance.
        unsafe { collision.share_flags(crate::movement::movement_flags_ptr()); }
    }
    module::setup(world);
    crate::battle::f4::reset();
    if let Some(map) = crate::vehicles::module::map() { map.level = level; }
    crate::allocations::prepare();
    sim().events.reserve(65536, 128, 1024);
    // Ordnance.fire may insert and retire an instant bolt inside this phase.
    // Prepare the ordinary F1 slabs and live lists before the first weapon tick.
    let store = &mut sim().store;
    store.bolt.reserve(1024); store.beam.reserve(1024);
    store.grenade.reserve(1024); store.round.reserve(1024);
    // Recon launch attribution is immutable for the battle. Its small rows
    // share the ordinary slab; later Unit pooling cannot rewrite a launch.
    store.owner_snapshot.reserve(8192);
    store.recon_context.reserve(64);store.recon_droid.reserve(4096);
    store.item.reserve(1024);store.items.items.reserve(1024);
    store.items.owned.entries.reserve(8192);
    store.ordnance.bolts.reserve(1024); store.ordnance.beams.reserve(1024);
    store.ordnance.grenades.reserve(1024); store.ordnance.rounds.reserve(1024);
    let mut input = vec![0.0; INPUT_WORDS];
    let mut random_states = vec![0.0; RANDOM_CAPACITY * F3RandomState::WORDS];
    let mut aim_input = vec![0.0; AIM_CAPACITY * F3AimChange::WORDS];
    let mut block = Block::default();
    block.words.reserve(RESERVED_UNITS * (F3Publication::WORDS + 256));
    block.heap.reserve(RESERVED_UNITS * 256);
    // PlayerCombat.update keeps notification latches and pending shots in
    // its canonical row. Foreign readers need only the weapon clocks here;
    // reserve that one variable list before the first publication.
    let mut snapshot = PublicationSnapshot::default();
    snapshot.value.combat.weapons.reserve(32);
    let F3PublishedControls::F3ForeignControls(foreign)=&mut snapshot.value.controls else{unreachable!()};
    foreign.values.resize_with(1,F3ControlView::default);
    let mut sensor_block = Block::default();
    sensor_block.words.reserve(16384);
    sensor_block.heap.reserve(16384);
    let mut shape = F3ShapeView::default();
    shape.shapes.reserve(10);
    let mut sensor_result = F3SensorResult::default();
    sensor_result.members.reserve(RESERVED_UNITS);
    let mut pushers = super::separate::Pushers::default();
    pushers.reserve(RESERVED_UNITS);
    // Widths come from the generated schema rather than duplicated offsets.
    // The input pointer refers to a reservation, not an imported body slab;
    // ordinary ticks overwrite intent facts in that reservation only.
    let layout = F3Boundary {
        descriptor_words: F3Boundary::WORDS as u32,
        allocation_words: F3Allocation::WORDS as u32,
        relation_words: F3RelationFact::WORDS as u32,
        hit_policy_words: F3HitPolicy::WORDS as u32,
        blast_binding_words: F3BlastBinding::WORDS as u32,
        impact_rules_words: F3ImpactRules::WORDS as u32,
        service_words: F3WeaponService::WORDS as u32,
        pose_words: F3PoseCommand::WORDS as u32,
        sensor_words: F3Sensor::WORDS as u32,
        foreign_words: F3PropFact::WORDS as u32,
        foreign_header_words: F3ForeignHeader::WORDS as u32,
        separation_words: F3Separation::WORDS as u32,
        building_setup_words: F3BuildingSetup::WORDS as u32,
        building_state_words: F3BuildingState::WORDS as u32,
        hull_setup_words: F3HullSetup::WORDS as u32,
        body_name_words: F3BodyName::WORDS as u32,
        input: input.as_mut_ptr() as u32,
        input_capacity: INPUT_WORDS as u32,
        input_stride: F3PhaseInput::WORDS as u32,
        network_words: F3PhaseNetwork::WORDS as u32,
        random_states: random_states.as_mut_ptr() as u32,
        random_capacity: RANDOM_CAPACITY as u32,
        random_words: F3RandomState::WORDS as u32,
        random_global_words: F3RandomGlobals::WORDS as u32,
        aim_input: aim_input.as_mut_ptr() as u32,
        aim_words: F3AimChange::WORDS as u32,
        aim_capacity: AIM_CAPACITY as u32,
        aim_header_words: F3AimHeader::WORDS as u32,
        missile_class_words: F3MissileClass::WORDS as u32,
        item_class_words: F3ItemClass::WORDS as u32,
        item_target_words: F3ItemTarget::WORDS as u32,
        dispenser_words: F3DispenserBinding::WORDS as u32,
        unit_items_words: F3UnitItems::WORDS as u32,
        blast_words: F3BlastCall::WORDS as u32,
        recon_class_words: F3ReconClass::WORDS as u32,
        recon_level_words: F3ReconLevel::WORDS as u32,
        recon_strike_words: F3ReconStrike::WORDS as u32,
        recon_binding_words: F3ReconBinding::WORDS as u32,
        recon_weapon_words: F3ReconWeapon::WORDS as u32,
        command_words: F3Change::WORDS as u32,
        adopt_words: F3Adoption::WORDS as u32,
        output_stride: F2Delta::WORDS as u32,
        ..Default::default()
    };
    unsafe {
        *addr_of_mut!(STATE) = Some(State {
            owner: Ownership::default(),
            viewpoint: None,
            separation: [0.0; F3Separation::WORDS],
            pushers,
            world: world as usize,
            input,
            command: [0.0; F3Change::WORDS],
            layout,
            descriptor: [0.0; F3Boundary::WORDS],
            block,
            services: Vec::new(),
            items: super::f3_items::Items::new(),
            props: super::f3_props::Props::new(),
            recon: Some(crate::recon::Authority::new(
                crate::recon::Definitions::default(), 64, 4096,
            )),
            recon_borrowed: false,
            prop_queries: crate::battle::live::Queries::default(),
            recon_services: crate::recon::module::Services::new(),
            tick: 0.0,
            infos: Vec::new(),
            edges: Vec::new(),
            snapshot,
            publication: Vec::new(),
            completed: Vec::with_capacity(RESERVED_UNITS),
            intent_changed: Vec::with_capacity(RESERVED_UNITS),
            intent_marked: Vec::with_capacity(RESERVED_UNITS),
            ai_full_marked: Vec::with_capacity(RESERVED_UNITS),
            phase_inputs: Vec::new(),phase_input_slots: Vec::with_capacity(RESERVED_UNITS),
            publication_ready: Vec::with_capacity(RESERVED_UNITS),
            publication_marks: Vec::new(),
            acknowledgement:[0.;F3Acknowledgement::WORDS],
            sensor_block,
            shape,
            sensor_result,
            bounds_cache:Vec::new(),
            foreign: Vec::with_capacity(RESERVED_UNITS),
            foreign_header: [0.0; F3ForeignHeader::WORDS],
            bodies: Vec::new(),
            health: Vec::new(),
            categories: Vec::new(),
            bot_targets: Vec::new(),native_intents:Vec::new(),
            hidden_crew: Vec::new(),
            missile_classes: Vec::new(),
            network: Vec::new(), arsenals: Vec::new(),
            relations: Vec::new(),
            relation_sides:0,
            hit_policies: Vec::new(),
            blasts: Vec::new(),
            friendly_fire: true,
            event_epoch: 0, running: false, failed:false, controls_consumed: false,
            history: crate::combat::history::History::new(48),
            random_states,
            shared_random:vec![None;RANDOM_CAPACITY],
            shared_bindings:Vec::with_capacity(3),
            random_epoch:[0.;F3RandomEpoch::WORDS],
            random_globals: [NO_STREAM as f64; F3RandomGlobals::WORDS],
            random_bindings: Vec::new(),
            random_sequence: 0,
            aim_input,
            aim_header: [0.0; F3AimHeader::WORDS],
            empty_text: sim().strings.intern_str(""),
            melee: crate::combat::targeting::MeleeScratch::new(RESERVED_UNITS),
            arc: Vec::with_capacity(RESERVED_UNITS * 2),
            ordnance_queries: (0..64).map(|_| Vec::with_capacity(RESERVED_UNITS)).collect(),
            ordnance_depth: 0,
        });
    }
    state().layout.command = state().command.as_mut_ptr() as u32;
    state().layout.acknowledgement=state().acknowledgement.as_mut_ptr() as u32;
    state().layout.random_epoch=state().random_epoch.as_mut_ptr() as u32;
    state().layout.random_globals = state().random_globals.as_mut_ptr() as u32;
    state().layout.aim_header = state().aim_header.as_mut_ptr() as u32;
    state().layout.foreign_header = state().foreign_header.as_mut_ptr() as u32;
    state().layout.separation = state().separation.as_mut_ptr() as u32;
    // Standalone phases have no single camera. Production supplies the same
    // camera scope as Game.separateSoldiers before entering its owned span.
    F3Separation { global: true, ..Default::default() }
        .put(&mut state().separation, &mut state().block.heap);
    descriptor()
}
// This generated record is the entire address/width contract. JavaScript
// reads memory directly after a call; getter exports would add a crossing for
// each output and make sparse publication needlessly expensive.
fn descriptor() -> *const f64 {
    let event_ptr = sim().events.ptr() as u32;
    let event_words = sim().events.len() as u32;
    let s = state();
    // AI decisions and mounted fire consume the same generators directly.
    // Refresh only those bound rows at publication; an older seed cache must
    // never overwrite draws made between F3 weapon updates.
    for &id in &s.shared_bindings {
        let at=id as usize*F3RandomState::WORDS;
        let mut row=F3RandomState::take(&s.random_states[at..],&[]);
        let seed=s.shared_random[id as usize].as_ref().unwrap().seed();
        if row.state!=seed {
            // AI can draw between unit phases without entering random_next.
            // Publish that change before the JS stream facade checks its epoch.
            let epoch=&mut s.random_epoch[F3RandomEpoch::REVISION];
            *epoch=(*epoch as u32).wrapping_add(1) as f64;
            row.state=seed;
        }
        row.put(&mut s.random_states[at..],&mut s.block.heap);
    }
    s.layout.controls_consumed=s.controls_consumed;
    s.layout.events = event_ptr;
    s.layout.event_words = event_words;
    s.layout.event_epoch = s.event_epoch;
    s.layout.put(&mut s.descriptor, &mut s.block.heap);
    s.descriptor.as_ptr()
}

/// Publish the touched body only; do not compute hit shapes as a side effect.
// Unit.spawn, remove and scalar setters must be visible before returning to
// a caller that can immediately read health, position or weapon readiness.
// Preserve the Unit's lazy spheresTick cache: exporting a body is not one of
// updateSpheres' observation points and cannot turn a stale death pose fresh.
// Reuse the fleet's generated numeric delta cache. TickCount alone still
// changes each live Unit, but does not justify resending 175 body words and
// every weapon clock. Candidate touches become output only when bits changed.
fn encode_publication(reference:Ref<Unit>, body_only:bool) -> bool {
    let unit=sim().store.unit.get(reference).expect("owned publication row");
    let s=state();
    if body_only && s.native_intents[reference.slot as usize]==Some(reference) {
        stage_body(&mut s.snapshot.value,reference,unit);stage_controls(&mut s.snapshot,reference,unit);
        let base=crate::store::block::HEADER;
        if let Some(changed)=s.publication[reference.slot as usize].prepare_retained(
            reference.slot,reference.gen,F3Publication::WORDS,
            &[base+F3Publication::BODY..base+F3Publication::ARSENAL,
              base+F3Publication::CONTROLS..base+F3Publication::WORDS],
            |row,heap|{s.snapshot.value.put_body(row,heap);s.snapshot.put_controls(row,heap)}) {
            return changed;
        }
    }
    stage(&mut s.snapshot,reference,unit);
    s.publication[reference.slot as usize].prepare(reference.slot,reference.gen,
        F3Publication::WORDS,|row,heap|s.snapshot.put(row,heap))
}
// JavaScript acknowledges a consumed version in the generated shared row.
// Until then every replacement block contains all pending deltas against the
// same consumed baseline. Native-only intermediate barriers cannot lose words.
fn acknowledge_publication() {
    let s=state();
    if s.acknowledgement[F3Acknowledgement::VERSION] as u32!=s.layout.version {return;}
    for r in s.publication_ready.drain(..) {
        s.publication[r.slot as usize].commit();
        s.publication_marks[r.slot as usize]=None;
    }
}
fn queue_publication(reference:Ref<Unit>,body_only:bool) -> bool {
    let changed=encode_publication(reference,body_only);
    queue_encoded(reference,changed)
}
// A narrow write still compares against the same acknowledged cache. Later
// full damage/movement publication can safely replace an unread intent delta.
fn queue_intent_publication(reference:Ref<Unit>)->bool {
    // A previous unread full row can carry damage, death or movement. A
    // retained baseline alone cannot represent that suffix, so restage it.
    if state().publication_marks[reference.slot as usize]==Some(reference.gen) {return queue_publication(reference,false);}
    let unit=sim().store.unit.get(reference).expect("owned AI input row");
    let s=state();stage_controls(&mut s.snapshot,reference,unit);
    if s.snapshot.held.is_none() {return queue_publication(reference,false);}
    let base=crate::store::block::HEADER;
    let changed=s.publication[reference.slot as usize].prepare_retained(reference.slot,reference.gen,F3Publication::WORDS,
        &[base+F3Publication::CONTROLS..base+F3Publication::WORDS],|row,heap|{
            s.snapshot.put_controls(row,heap);
        });
    queue_encoded(reference,changed.unwrap_or_else(||encode_publication(reference,false)))
}
fn queue_encoded(reference:Ref<Unit>,changed:bool)->bool {
    let s=state();let slot=reference.slot as usize;
    let pending=s.publication_marks[slot]==Some(reference.gen);
    if changed && !pending {
        s.publication_marks[slot]=Some(reference.gen);s.publication_ready.push(reference);
    } else if !changed && pending {
        s.publication_marks[slot]=None;
        s.publication_ready.retain(|&r|r!=reference);
    }
    changed||pending
}
pub(crate) fn publish(reference: Ref<Unit>) {
    if crate::world::deferred() {state().owner.mark(reference);return;}
    let _allocations=crate::allocations::at(F3AllocationStage::Publication);
    acknowledge_publication();
    let changed=queue_publication(reference,false);
    state().owner.unmark(reference);
    if changed {write_publication();} else {descriptor();}
}

// Unit.tick can hurt another Unit immediately. First-touch order also holds
// across unread barriers; source and victim updates share one consumed epoch.
pub(crate) fn publish_changed() {
    flush_completed();
    publish_changed_at(None,true);
}
fn flush_completed() {
    for i in 0..state().completed.len() {
        let reference = state().completed[i];
        // A structural replacement may have retired a prepared slab object.
        // Publish only its still-live identity, never the replacement slot.
        if sim().store.unit.contains(reference) { state().owner.mark(reference); }
    }
    state().completed.clear();
}
fn completed_tick(reference: Ref<Unit>) {
    state().owner.unmark(reference);
    state().completed.push(reference);
}
fn publish_changed_at(body_only:Option<Ref<Unit>>,publish_intents:bool) {
    if crate::world::deferred() {crate::vehicles::module::f2::publish_pending();crate::ai::owned::entry::publish_controls();return;}
    crate::battle::f4_core::publish();
    crate::battle::f4_live::publish();
    crate::vehicles::module::f2::publish_pending();
    crate::ai::owned::entry::publish_controls();
    let _allocations=crate::allocations::at(F3AllocationStage::Publication);
    acknowledge_publication();
    let mut changed=false;
    for i in 0..state().owner.changed().len() {
        let reference=state().owner.changed()[i];
        let weapon=state().ai_full_marked[reference.slot as usize]==Some(reference.gen);
        changed|=queue_publication(reference,body_only==Some(reference)&&!weapon);
    }
    let mut retained=0;
    for i in 0..state().intent_changed.len() {
        let reference=state().intent_changed[i];
        let live=sim().store.unit.contains(reference);
        let full=state().owner.is_marked(reference);
        if live&&!full {
            if !publish_intents {
                // Keep first-touch order and generation stamps until the
                // following unit span. A full lifecycle row supersedes it.
                state().intent_changed[retained]=reference;retained+=1;continue;
            }
            changed|=if state().ai_full_marked[reference.slot as usize]==Some(reference.gen) {
                queue_publication(reference,false)
            }else{queue_intent_publication(reference)};
        }
        let weapon=&mut state().ai_full_marked[reference.slot as usize];
        if *weapon==Some(reference.gen) {*weapon=None;}
        let stamp=&mut state().intent_marked[reference.slot as usize];
        if *stamp==Some(reference.gen) {*stamp=None;}
    }
    state().intent_changed.truncate(retained);state().owner.clear_changed();
    if changed {write_publication();} else {descriptor();}
}
fn write_publication() {
    let s=state();let count=s.publication_ready.len();
    s.block.begin(count,F2Delta::WORDS);
    for i in 0..count {
        let reference=s.publication_ready[i];
        let (row,heap)=s.block.row(i);
        s.publication[reference.slot as usize].delta.value.put(row,heap);
    }
    finish_publication(count);
}

fn finish_publication(count:usize) {
    let s=state();s.block.finish();
    s.layout.version=s.layout.version.wrapping_add(1);
    s.layout.output=s.block.ptr() as u32+(crate::store::block::HEADER*8) as u32;
    s.layout.output_rows=count as u32;
    s.layout.output_heap=s.block.ptr() as u32+(s.block.words[BlockHeader::HEAP] as u32*8);
    // Direct cache reads count as traffic too, although they no longer take
    // space in the output heap. The shared masks bound exactly these words.
    let borrowed: usize = s.publication_ready.iter()
        .map(|r| s.publication[r.slot as usize].borrowed_words()).sum();
    s.layout.output_words=(count*F2Delta::WORDS+s.block.heap.len()+borrowed) as u32;
    descriptor();
}

// Unit.onMoved receives its current facing/movement in UnitAimArgs. Publish
// actual damage/lifecycle changes here, without
// re-encoding this unchanged combat and the rest of its correction body.
// The current row is restored before the import so AI sensors still query
// canonical geometry at their original observation point.
pub(crate) fn before_hook() {
    // Aimers consume only their declared current movement and their own
    // previous combat. Other completed weapon clocks wait for the outer
    // barrier; actual damage/lifecycle writers remain in the changed queue.
    publish_changed_at(None,true);
}

/// Construction-only adoption. Refusing an unknown generation never creates
/// or overwrites a row as the legacy ensure/import path does.
// UnitFactory.build loads one complete Unit before entering the free-pool
// contract. Slab generation identifies that object; gameplay generation is a
// separate value that Unit.spawn advances on reuse. Adoption changes neither.
// Reserve the mover and shape scratch now, not on the first active tick.
#[no_mangle]
pub extern "C" fn f3_adopt() -> u32 {
    // Reading this row by value releases scratch before loading retained
    // class services. Nothing points into JS-owned memory after the call.
    let adoption = F3Adoption::take(&state().command, &[]);
    let reference = adoption.key.reference();
    let slot = reference.slot;
    if adoption.key.validate(&sim().store).is_err() {
        return 0;
    }
    let adopted = if adoption.passive { state().owner.adopt_passive(&sim().store, reference) }
        else { state().owner.adopt(&sim().store, reference) };
    if adopted.is_err() {
        return 0;
    }
    module::reserve_unit(slot);
    crate::vehicles::module::f2::live_seats::adopt_unit(reference, adoption.has_actor, adoption.no_enter);
    let capacity = state().owner.built().len();
    let history_capacity = sim().store.units.list.len().max(capacity);
    state().pushers.reserve(history_capacity);
    reserve_history();
    { let s = state(); s.completed.reserve(capacity.saturating_sub(s.completed.len())); s.intent_changed.reserve(capacity.saturating_sub(s.intent_changed.len())); }
    let size = state().categories.len().max(capacity).max(slot as usize + 1);
    state().intent_marked.resize(size,None);
    state().ai_full_marked.resize(size,None);
    state().phase_inputs.resize(size,None);
    { let s=state();s.phase_input_slots.reserve(size.saturating_sub(s.phase_input_slots.len())); }
    state().categories.resize(size, 0);
    prepare_bounds(size);invalidate_bounds(slot);
    state().items.reserve_unit(size);
    state().categories[slot as usize] = adoption.category;
    state().bot_targets.resize(size, None);state().native_intents.resize(size,None);
    state().hidden_crew.resize(size, None);
    state().network.resize_with(size, F3NetworkView::default);
    state().arsenals.resize(size, 0);
    let health_name = adoption.key.validate(&sim().store).unwrap().combat.scale;
    let name = String::from_utf16_lossy(sim().strings.units(health_name));
    let health = crate::combat::impacts::HealthScale::from_scale(&name);
    if state().health.len() <= slot as usize {
        state()
            .health
            .resize(slot as usize + 1, crate::combat::impacts::HealthScale::Person);
    }
    state().health[slot as usize] = health;
    let unit = sim().store.unit.get_mut(reference).unwrap();
    if let UnitBody::Soldier(body) = &mut unit.soldier {
        super::soldier::prepare(body, state().world);
        // Soldier.touchPolygons first contact must use construction storage.
        // Reservation does not run a hidden movement tick or fill lazy caches.
        crate::movement::reserve_mover(body.mover as u32, 256, 16, 512);
    }
    unit.combat.pending_muzzles.reserve(64);
    // Slot zero binds combat's chunks draw; subsequent slots bind arsenal
    // entries. Reset installs a new arsenal through this same construction seam.
    let weapons = unit.combat.weapons.len();
    if state().publication.len()<=slot as usize {
        state().publication.resize_with(slot as usize+1,||crate::vehicles::f2_delta::Cache::new(0));
    }
    state().publication_marks.resize(state().publication.len(),None);
    // Both cache banks must fit a foreign control record from construction;
    // a player/corpse publication cannot grow the second bank on its next tick.
    let words=crate::store::block::HEADER+F3Publication::WORDS+
        weapons*F3WeaponView::WORDS+F3ControlView::WORDS;
    state().publication[slot as usize]=crate::vehicles::f2_delta::Cache::borrowed(words);

    let bindings = &mut state().random_bindings;
    if bindings.len() <= slot as usize { bindings.resize_with(slot as usize + 1, Vec::new); }
    bindings[slot as usize].resize(weapons + 1, NO_STREAM);
    // Each combat invocation keeps its capped query through recursive damage.
    // A first swing must not allocate the seen set from inside a phase.
    // Melee and arc acquisition finish before the next Unit starts combat.
    // Their one retained phase buffer also survives nested damage; a blast
    // never enters another weapon update or acquisition operation.
    // Unit.updateSpheres may first need an exposed rider during a shot.
    // Prepare its maximum shape set now, preserving the cache's lazy timing
    // while removing the allocation from that first observation.
    if unit.rider_hits.is_none() {
        unit.rider_hits = Some(vec![HitShape::default(); super::unit::RIDER_SHAPES]);
    }
    if state().edges.len() <= slot as usize {
        state().edges.resize_with(slot as usize + 1, Edges::default);
    }
    let infos = &mut state().infos;
    if infos.len() <= slot as usize {
        infos.resize_with(slot as usize + 1, UnitsPhaseUnit::default);
    }
    infos[slot as usize].unit = slot as f64;
    infos[slot as usize].droideka_water = adoption.droideka_water;
    publish(reference);
    1
}

/// Prepared F1 handles for a weapon's charge levels (CombatWorld.fireRound).
// CombatWorld's weapon can change charge level without changing its facade.
// Keep a service for every merged level. Resolve the class name for dispenser
// admission here so the update loop never creates a String or searches a map.
#[no_mangle]
pub extern "C" fn f3_weapon_services(weapon: u32, count: u32) {
    let (rows, _) = sim().calls.args();
    let mut levels = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let at = i * F3WeaponService::WORDS;
        let row = F3WeaponService::take(&rows[at..], &[]);
        levels.push(WeaponService {
            ordnance: row.ordnance,
            item: row.item,
            channel: row.channel,
            bank: row.bank,
            lightning: row.lightning,
            has_lightning: row.has_lightning,
            lock: crate::combat::targeting::Lock {
                range: row.lock.range,
                on_angle: row.lock.on_angle,
                off_angle: row.lock.off_angle,
                relation_mask: row.lock.relation_mask,
                category_mask: row.lock.category_mask,
            },
        });
    }
    let all = &mut state().services;
    if all.len() <= weapon as usize {
        all.resize_with(weapon as usize + 1, Vec::new);
    }
    all[weapon as usize] = levels;
}

/// UnitFactory's built order is canonical. Keys in the input may update only
/// the intent of the same slab generation and the same gameplay life.
// Unit.tick visits the factory's construction order, including corpses that
// have not reached DEATH_LINGER. Inputs can update intent only for the same
// slab object and life. A stale row cannot recreate or overwrite a body.
// Consume inputs before callbacks, which may reuse the reserved command area.
#[no_mangle]
pub extern "C" fn f3_step(count: u32, dt: f64, level: f64, tick: f64) -> u32 {
    state().tick = tick;
    with_services(|authority| step(count,dt,level,authority))
}
fn step(count: u32, dt: f64, level: f64, mut recon: Option<crate::units::f3::WorldServices<'_>>) -> u32 {
    let _controls = control_span(true);
    let allocations = crate::allocations::snapshot();
    begin_events();
    // WaterState has already advanced in Game.step. Later F1 crossings use
    // this same bobbed surface until the next authoritative world tick.
    if let Some(map) = crate::vehicles::module::map() { map.level = level; }
    state().running = true;
    crate::allocations::begin();
    if count as usize>INPUT_WORDS/F3PhaseInput::WORDS {
        state().running=false;finish_allocations(allocations);return 0;
    }
    // Foreign props share the one phase input reservation after the intents.
    // Apply changed facts before a launcher or melee can read this registry.
    let foreign = F3ForeignHeader::take(&state().foreign_header,&[]);
    if foreign.network as usize>INPUT_WORDS/F3PhaseNetwork::WORDS ||
        foreign.count as usize>INPUT_WORDS/F3PropFact::WORDS ||
        foreign.buildings as usize>INPUT_WORDS/F3BuildingState::WORDS {
        state().running=false;finish_allocations(allocations);return 0;
    }
    let network_at = count as usize * F3PhaseInput::WORDS;
    let offset = network_at + foreign.network as usize * F3PhaseNetwork::WORDS;
    if offset+foreign.count as usize*F3PropFact::WORDS+foreign.buildings as usize*F3BuildingState::WORDS>INPUT_WORDS {
        state().running=false;finish_allocations(allocations);return 0;
    }
    foreign_rows(offset,foreign.count);
    building_rows(offset + foreign.count as usize * F3PropFact::WORDS, foreign.buildings);
    // F3.phase prepares mounted hints before updating the first body.
    // Keep that snapshot point: a later shot can replace a controller.
    if let Some(ai)=recon.as_ref().and_then(|services|services.ai.as_ref()) {crate::vehicles::module::f2::firing::freeze_ai_targets(&**ai);}
    // Foreign requests are indexed before any callbacks can reuse input memory.
    // Omitted native bots resolve current Core ownership before any body
    // advances, never the page's hint alone. Empty standalone phases
    // retain the existing no-new-request behavior for unowned bodies.
    for i in 0..state().phase_input_slots.len() {
        let slot=state().phase_input_slots[i];state().phase_inputs[slot]=None;
    }
    state().phase_input_slots.clear();
    for i in 0..count as usize {
        let at=i*F3PhaseInput::WORDS;
        let key=F3UnitKey::take(&state().input[at+F3PhaseInput::KEY..at+F3PhaseInput::KEY+F3UnitKey::WORDS],&[]);
        if key.validate(&sim().store).is_err() {
            if crate::world::deferred() {continue;}
            state().running=false;finish_allocations(allocations);return 0;
        }
        let slot=key.slot as usize;
        if state().phase_inputs.get(slot).is_none() || state().phase_inputs[slot].is_some() ||
            state().phase_input_slots.len()==state().phase_input_slots.capacity() {
            state().running=false;finish_allocations(allocations);return 0;
        }
        state().phase_inputs[slot]=Some(at);state().phase_input_slots.push(slot);
    }
    // Explicit rows also include passive human/rider bodies, which the body
    // tick list deliberately excludes. Preserve their source input order;
    // append only omitted native bots, whose requests are already canonical.
    let built=state().owner.built().len();
    for i in 0..count as usize+built {
        let (reference,mut input)=if i<count as usize {
            let at=i*F3PhaseInput::WORDS;
            let input=F3PhaseInput::take(&state().input[at..at+F3PhaseInput::WORDS],&[]);
            if crate::world::deferred() && input.key.validate(&sim().store).is_err() {continue;}
            (input.key.reference(),Some(input))
        }else{
            let reference=state().owner.built()[i-count as usize];
            if state().phase_inputs[reference.slot as usize].is_some() {continue;}
            let Some(unit)=sim().store.unit.get(reference) else {continue};
            if unit.removed {continue;}
            (reference,None)
        };
        let ai_owned=recon.as_ref().and_then(|services|services.ai.as_ref()).is_some_and(|ai|ai.owns_intent(&sim().store,reference));
        // The enclosing input was prepared before AI. Boarding can move
        // this controller to a native SeatUnit in that interval, leaving no
        // foreign foot-intent row. Its canonical seated body remains current.
        let retained_body=crate::world::deferred() && sim().store.unit.get(reference).is_some_and(|unit| {
            unit.combat.dead || unit.seat.is_some() && unit.character
                .and_then(|character|sim().store.character.get(character))
                .is_some_and(|character|character.unit==Some(reference)&&character.controller.is_some())
        });
        if !ai_owned && !retained_body {
            let Some(input)=input.as_mut() else {
                if recon.as_ref().is_some_and(|services|services.ai.is_some()) {
                    state().running=false;finish_allocations(allocations);return 0;
                }
                continue;
            };
            // Missing intent is accepted only for an independently verified
            // canonical AI owner; a foreign row cannot acquire that authority.
            let Some(intent)=input.intent.take() else {state().running=false;finish_allocations(allocations);return 0;};
            if state().owner.intent(&mut sim().store,F3IntentCommand {key:F3UnitKey {slot:input.key.slot,generation:input.key.generation,life:input.key.life},intent}).is_err() {
                state().running=false;finish_allocations(allocations);return 0;
            }
        }
        state().bot_targets[reference.slot as usize] = if let Some(ai)=recon.as_ref().and_then(|services|services.ai.as_ref()) {
            let hint=ai.weapon_hint(&sim().store,reference,None);hint.bot.then_some(hint.target)
        }else{input.as_ref().filter(|input|input.bot_target_set).map(|input|input.bot_target)};
        let network=if let Some(input)=input.as_ref().filter(|input|input.has_network) {
            if input.network_index>=foreign.network {state().running=false;finish_allocations(allocations);return 0;}
            let at=network_at+input.network_index as usize*F3PhaseNetwork::WORDS;
            Some(F3PhaseNetwork::take(&state().input[at..at+F3PhaseNetwork::WORDS],&[]))
        }else{None};
        state().network[reference.slot as usize]=network.as_ref().map(|r|F3NetworkView {
            view:r.network.view,lead:r.network.lead,playout:r.network.playout,
            fire0:r.network.fire0,fire1:r.network.fire1,view0:r.network.view0,view1:r.network.view1,
        }).unwrap_or_default();
        sim().store.unit.get_mut(reference).unwrap().remotes_active=crate::recon::module::active(reference);
        if let Some(context)=crate::recon::module::context(reference) {
            sim().store.recon_context.get_mut(context).unwrap().first_person=input.as_ref().is_some_and(|input|input.remote_first_person);
        }
        state().infos[reference.slot as usize].remote=crate::recon::module::context(reference).is_some();
        // An empty shot list remains present and suppresses a legacy edge.
        state().edges[reference.slot as usize]=network.map(|input|Edges {
            shots:None,shot:input.edge.as_ref().map(crate::combat::module::edge),
            fixed:input.edge_list.then(||[input.edge0.as_ref().map(crate::combat::module::edge),input.edge1.as_ref().map(crate::combat::module::edge)]),
        }).unwrap_or_default();
    }
    let _tick_allocations = crate::allocations::at(F3AllocationStage::Other);
    // Unit.tick itself keeps the seated, held and removed gates. Keep water
    // and death-region tests immediately after that individual tick: a kill
    // can affect what the next unit observes and who its shot is credited to.
    let n = state().owner.built().len();
    for i in 0..n {
        let reference = state().owner.built()[i];
        let Some(u) = sim().store.unit.get(reference) else {
            continue;
        };
        if u.removed {
            continue;
        }
        let facts = &state().infos[reference.slot as usize];
        let info = UnitsPhaseUnit {
            unit: facts.unit,
            remote: facts.remote,
            droideka_water: facts.droideka_water.as_ref().map(|r| DroidekaWater {
                interval: r.interval,
                amount: r.amount,
                upright: r.upright,
                ball: r.ball,
            }),
            ..Default::default()
        };
        // Pass a value frame into combat so a synchronous callback cannot
        // alias the phase's input reservation. No store borrow crosses tick.
        let edges = std::mem::take(&mut state().edges[reference.slot as usize]);
        module::tick_owned(&info, level, dt, edges,recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        state().owner.mark(reference);
        let unit = sim().store.unit.get(reference).unwrap();
        if unit.removed {
            state()
                .owner
                .removed(&sim().store, &F3UnitKey::of(reference, unit))
                .unwrap();
        }
        completed_tick(reference);
    }
    // Game.step interposes Vehicles.update and TurretSeats.update here.
    // Lend the same services to physics, seats and mounted fire; an exported
    // fleet wrapper would start another ownership boundary inside this one.
    if crate::vehicles::module::f2::active() {
        crate::vehicles::module::f2::step(dt,state().tick,
            recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
    }
    // Recon finish also advances removed, seated and retired
    // owners after hulls/seats, before the world's separation pass.
    if let Some(services) = recon.as_mut() {
        let authority = &mut *services.recon;
        authority.finish(&mut sim().store,&mut crate::recon::module::NativeWorld::idle(services.queries,services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks)),
            state().tick,dt).unwrap_or_else(|_|fail());
        crate::recon::module::publish_active(authority);
        crate::recon::module::publish(authority);
    }
    // Game.separateSoldiers follows vehicles/seats and recon finish. Read the
    // live registry now, including corpses and bodies moved by those services.
    // Taking retained scratch avoids keeping a State borrow beside the slab.
    let input = F3Separation::take(&state().separation, &[]);
    let scope = super::separate::Scope {
        player: state().viewpoint.map(|reference| reference.slot),
        player_out: state().viewpoint.and_then(|r| sim().store.unit.get(r))
            .is_some_and(|u| u.combat.dead || u.seat.is_some()),
        global: input.global,
        camera: three::Vector3::new(input.camera[0], input.camera[1], input.camera[2]),
    };
    let mut pushers = std::mem::take(&mut state().pushers);
    let store = &mut sim().store;
    super::separate::separate(&store.units, &mut store.unit, &scope, &mut pushers);
    for reference in pushers.changed() { state().owner.mark(reference); }
    state().pushers = pushers;
    publish_changed();
    // Include input parsing, solver work, callbacks and sparse serialization.
    // Descriptor writes below use fixed storage and cannot allocate themselves.
    finish_allocations(allocations);
    state().running = false;
    descriptor();
    u32::from(!state().failed)
}

// Owned spans report real allocator traffic, including serialization. Reuse
// this recorder for end-of-tick capture rather than estimating its Vec growth.
fn finish_allocations(before: crate::allocations::Counts) {
    crate::allocations::end();
    let used = crate::allocations::snapshot().since(before);
    state().layout.allocations = used.calls as u32;
    state().layout.reallocations = used.reallocs as u32;
    state().layout.allocation_bytes = used.bytes as u32;
}

// Unit spawn metadata contains no solver state. Network lifecycle commands
// can qualify a manually advanced gameplay generation through this old key.
fn metadata(reference: Ref<Unit>, m: &F3Metadata) {
    invalidate_bounds(reference.slot);
    let u = sim().store.unit.get_mut(reference).unwrap();
    u.character = m.character; u.controller = m.controller; u.team = m.team;
    u.generation = m.life; u.human = m.human; u.network_human = m.network_human;
    u.hero = m.hero; u.combat.team = m.team;
}

// PlayerCombat.reset replaces weapon identity even when gameplay life stays
// unchanged. Unit.spawn reaches the same hook through ModuleWorld.
pub(crate) fn arsenal_reset(id: u32) {
    if active() { state().arsenals[id as usize] = state().arsenals[id as usize].wrapping_add(1); }
}

// Game.step increments the dormant viewpoint before AI can observe shapes.
// The native world driver calls this at that slot; its host only consumes
// the touched publication, so no setter export reenters the driver.
pub(crate) fn viewpoint_count(count: f64) {
    if !active() { return; }
    if let Some(reference) = state().viewpoint {
        invalidate_bounds(reference.slot);
        sim().store.unit.get_mut(reference).unwrap().tick_count = count;
        changed(reference); publish_changed();
    }
}

// Vehicles.spawnSlots samples the player's blocker after unit combat. A
// death earlier in this span must clear it before the pad checks occupancy.
pub(crate) fn spawn_occupant() -> Option<three::Vector3> {
    let unit = sim().store.unit.get(state().viewpoint?)?;
    (!unit.combat.dead).then(|| unit.position())
}

/// Scalar commands share one reserved generated row. The store's key, never
/// a slot-only lookup or a JavaScript life counter, authorizes the mutation.
// The command is a stack value before any hook can recurse into this export.
// Thus a nested hurt cannot overwrite its caller's attacker, position or key.
// Validate before taking the body out of the store; failed keys change nothing.
// A valid command can still be refused by gameplay, such as a busy selection.
#[no_mangle]
pub extern "C" fn f3_command() -> u32 {
    // ClassicCombatBinding.attachController binds weapon streams inside the
    // tail's spawn hook. These scalar settings need no body/recon services;
    // borrowing them again would conflict with the enclosing native phase.
    if state().recon_borrowed {
        let action=F3Change::take(&state().command,&[]).action;
        assert!(matches!(action,F3Action::RandomBinding|F3Action::CombatField|F3Action::WeaponField),
            "body command inside native world hook");
        command(None)
    } else {with_services(command)}
}
fn command(mut recon: Option<crate::units::f3::WorldServices<'_>>) -> u32 {
    let command = F3Change::take(&state().command, &[]);
    let _controls = control_span(command.action == F3Action::Spawn);
    let reference = command.key.reference();
    if command.key.validate(&sim().store).is_err() {
        return 0;
    }
    if !state().running { begin_events(); }
    state().layout.accepted = true;
    match command.action {
        F3Action::RandomBinding | F3Action::CombatField | F3Action::WeaponField => {
            // Network spawn flags precede combat.player in the source. Keep
            // those pending metadata writes on this same touched-row command.
            if let Some(m) = command.metadata.as_ref() { metadata(reference,m); }
            if !apply_setting(reference,command.action,command.field,command.index,command.value) {return 0;}
        }
        // Game.spawnUnit sets hero status after Unit.spawn. Metadata has
        // no body fields and does not reset timers or advance gameplay life.
        F3Action::Metadata => {
            let Some(m)=command.metadata.as_ref() else {return 0;};
            let previous = sim().store.unit.get(reference).unwrap().generation;
            metadata(reference,m);
            // MultiplayerWorld.addPlayer advances a reconnect's life without
            // resetting combat. The bound context follows that live owner;
            // already launched droids keep their frozen attribution.
            if previous != m.life {
                if let Some(services) = recon.as_mut() {
                    crate::recon::module::spawned(reference, &mut *services.recon);
                }
            }
        }
        // Human spawning changes teams before reset; combat and Unit team
        // must agree before any synchronous credit or damage hook reads them.
        F3Action::Team => {
            state()
                .owner
                .team(&mut sim().store, &command.key, command.value)
                .unwrap();
        }
        // Soldier.lookRaw keeps each mouse sample's normalization order.
        // Mouse look itself does not invalidate tickCount's shape cache.
        F3Action::Look | F3Action::LookRaw => {
            state()
                .owner
                .look(
                    &mut sim().store,
                    &F3LookCommand {
                        key: command.key,
                        dx: command.vector[0],
                        dy: command.vector[1],
                        sensitivity: command.vector[2],
                        invert: command.flag,
                        raw: command.action == F3Action::LookRaw,
                    },
                )
                .unwrap();
        }
        // LocalServer preserves its original player placement after creating
        // the network life. This is a body reset, not a second Unit.spawn.
        F3Action::BodySpawn => {
            let mut world = module::ModuleWorld::sensor(state().world);
            invalidate_bounds(reference.slot);
            let u = sim().store.unit.get_mut(reference).unwrap();
            u.spawn_body(command.vector[0],command.vector[1],command.vector[2],command.value,&mut world);
        }
        // MultiplayerWorld advances a removed body's respawn clock before
        // factory iteration, which deliberately skips that removed body.
        F3Action::DeadTime => sim().store.unit.get_mut(reference).unwrap().dead_time = command.value,
        // PathExecutor.contactSidestep consumes the last movement contact at
        // its AI read slot. Clearing it is not a movement tick or a shape
        // invalidation, and the next solver starts with this exact zero.
        F3Action::ConsumeContact => match &mut sim().store.unit.get_mut(reference).unwrap().soldier {
            UnitBody::Soldier(s) => s.avoidance = [0.0;3],
            UnitBody::Droideka(d) => d.avoidance = [0.0;3],
        },
        // Game's warm factory construction explicitly releases unused bodies.
        // This joins the same class stack as Unit.remove, without running death
        // or lifecycle callbacks on a body that has never entered a life.
        F3Action::PoolRelease => {
            if state().owner.removed(&sim().store,&command.key).is_err() {return 0;}
        }
        F3Action::Health => {
            sim().store.unit.get_mut(reference).unwrap().combat.health = command.value;
        }
        // ClassicCombatBinding.attachController replaces a seat's default
        // stream after boarding returns. Apply that foreign scalar immediately;
        // a later unrelated publication must not restore the old callback.
        F3Action::MountedRandom => {
            state().layout.accepted = crate::vehicles::module::f2::live_seats::bind_random(
                reference,match command.lease {Some(lease)=>lease,None=>return 0},command.value as u32);
        }
        // Arsenal.select/next can refuse a busy channel without making this
        // a stale command. Report that gameplay result separately from key validity.
        F3Action::Select | F3Action::Next => {
            let defs = crate::combat::module::defs();
            let c = &mut sim().store.unit.get_mut(reference).unwrap().combat;
            state().layout.accepted = if command.action == F3Action::Select {
                crate::combat::module::select(c, &defs, command.index as usize)
            } else {
                crate::combat::weapons::Arsenal {
                    weapons: &mut c.weapons,
                    selected: &mut c.selected,
                }
                .next(&defs.weapons, command.index as usize, command.value)
            };
        }
        // Unit.spawn advances gameplay life after binding the new Character.
        // Keep the pooled slab handle and its vectors stable through that reset.
        F3Action::Spawn => {
            if let Some(m) = &command.metadata {
                let u = sim().store.unit.get_mut(reference).unwrap();
                u.character = m.character;
                u.controller = m.controller;
                u.team = m.team;
                u.generation = m.life;
                u.human = m.human;
                u.network_human = m.network_human;
                u.hero = m.hero;
                u.combat.team = m.team;
            }
            module::lifecycle_owned(command.key.slot, Some((command.vector, command.value)),recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            if let Some(services) = recon.as_mut() {
                crate::recon::module::spawned(reference, services.recon);
            }
        }
        // Unit.remove leaves a corpse's registry identity intact and returns
        // reusable bodies to their class stack. Network-owned bodies are not free.
        F3Action::Remove => {
            if let Some(m) = command.metadata.as_ref() { metadata(reference,m); }
            module::lifecycle_owned(command.key.slot, None,recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            let unit = sim().store.unit.get(reference).unwrap();
            state()
                .owner
                .removed(&sim().store, &F3UnitKey::of(reference, unit))
                .unwrap();
        }
        // Game's original viewpoint is registered once at construction. This
        // names its slab object, so an ordinary respawn keeps literal ownership.
        F3Action::Viewpoint => state().viewpoint = Some(reference),
        F3Action::Controller => sim().store.unit.get_mut(reference).unwrap().controller = command.controller,
        F3Action::LetGo => {
            sim().store.unit.get_mut(reference).unwrap().let_go();
            super::live_pose::clear(reference);
        }
        // Soldier.setAir clears posture/state time at this exact call point.
        // Death detachment must not leave a jet trooper in the jet state.
        F3Action::BodyAir => {
            if let UnitBody::Soldier(s) = &mut sim().store.unit.get_mut(reference).unwrap().soldier {
                let Ok(field)=u8::try_from(command.field) else {return 0;};
                let Some(air)=Air::from_u8(field) else {return 0;};
                s.air=Some(air);
                s.posture = Posture::Stand;
                s.state_time = 0.0;
            }
        }
        // MultiplayerWorld's between-lives body is removed and untargetable;
        // it is not a fresh spawn and must not advance gameplay generation.
        F3Action::Unspawn => {
            if command.flag {
                let u=sim().store.unit.get_mut(reference).unwrap();u.alive=false;u.combat.dead=true;u.removed=true;
            } else {unspawn_native(reference);}
        }

        F3Action::Reset | F3Action::Damage | F3Action::Explode => mutate_combat(reference, &command,recon.as_mut().map(crate::units::f3::WorldServices::reborrow)),
    }
    // Publish the command's row and every victim actually reached by its
    // immediate recursive damage. Unaffected registry members stay untouched.
    if let Some(services) = recon.as_mut() {
        let authority = &mut *services.recon;
        crate::recon::module::publish_active(authority);
        crate::recon::module::publish(authority);
    }
    state().owner.mark(reference);
    // Mouse look/contact consumption changes no weapon or combat clocks.
    // Reuse the consumed heap unless an unread publication still owns it.
    let body_only = (matches!(command.action, F3Action::Look | F3Action::LookRaw |
        F3Action::BodyAir | F3Action::BodySpawn | F3Action::DeadTime | F3Action::ConsumeContact)
        && state().publication_marks[reference.slot as usize] != Some(reference.gen)).then_some(reference);
    publish_changed_at(body_only,true);
    u32::from(!state().failed)
}

// CombatFacade's scalar setters and the AI aimer use one application path.
// A setting touches its own row; neither path imports a Unit or weapon table.
fn apply_setting(reference:Ref<Unit>,action:F3Action,field:u32,index:u32,value:f64) -> bool {
    match action {
        F3Action::RandomBinding=> {
            let Some(slot)=state().random_bindings[reference.slot as usize].get_mut(index as usize) else {return false;};
            let id=value as u32;
            if id!=NO_STREAM && id as usize>=RANDOM_CAPACITY {return false;}
            *slot=id;true
        }
        F3Action::CombatField=> {
            if crate::combat::module::cmb_set(reference.slot,field,value)==0 {return false;}
            if field==F3CombatField::Team as u32 {sim().store.unit.get_mut(reference).unwrap().team=value;}
            true
        }
        F3Action::WeaponField=>crate::combat::module::cmb_weapon_set(reference.slot,index,field,value)!=0,
        _=>false,
    }
}

// A missing service is a rejected boundary operation, not a WASM trap. Keep
// the current native frame valid until the outer phase reports its failure.
pub(crate) fn fail() {if active() {state().failed=true;state().layout.accepted=false;}}
macro_rules! required_services {
    ($value:expr,$fallback:expr)=>{match $value {Some(services)=>services,None=>{$crate::units::f3::fail();return $fallback;}}};
}
pub(crate) use required_services;

/// Unit.onMoved's aimer returns deltas in a generated block. Applying them
/// after the import preserves scalar write order without nested WASM exports.
pub(crate) fn begin_aimer() {
    F3AimHeader { count: 0 }.put(&mut state().aim_header, &mut state().block.heap);
}
pub(crate) fn finish_aimer() {
    let header = F3AimHeader::take(&state().aim_header, &[]);
    if header.count as usize>AIM_CAPACITY {fail();return;}
    for index in 0..header.count as usize {
        let at = index * F3AimChange::WORDS;
        let change = F3AimChange::take(&state().aim_input[at..], &[]);
        if change.key.validate(&sim().store).is_err() ||
            !apply_setting(change.key.reference(),change.action,change.field,change.index,change.value) {fail();return;}
        changed(change.key.reference());
    }
}

/// Bind the bare AI generator once at native world construction. Unit fire
/// consumes this same authority between decisions, rather than a copied seed.
pub fn bind_ai_stream(id:u32,stream:crate::ai::random::SharedRandom) {
    assert!(id<RANDOM_CAPACITY as u32,"native AI random binding");
    let at=id as usize*F3RandomState::WORDS;
    let mut row=F3RandomState::take(&state().random_states[at..],&[]);
    row.state=stream.seed();row.present=true;
    row.put(&mut state().random_states[at..],&mut state().block.heap);
    if state().shared_random[id as usize].is_none() {state().shared_bindings.push(id);}
    state().shared_random[id as usize]=Some(stream);
}
/// useClassicRandom changes the arsenal's spread generators only. Combat's
/// chunks roll retains its gameplay stream, independently of bot attachment.
pub fn use_ai_stream(key:&F3UnitKey,id:u32) {
    assert!(state().shared_random.get(id as usize).is_some_and(Option::is_some),"prepared native AI random stream");
    let changed={let unit=key.validate_mut(&mut sim().store).expect("current AI weapon owner");
        let mut changed=false;
        for weapon in &mut unit.combat.weapons {changed|=!weapon.random;weapon.random=true;}changed};
    let bindings=&mut state().random_bindings[key.slot as usize];
    for binding in bindings.iter_mut().skip(1) { *binding=id; }
    if changed {self::changed(key.reference());}
}

/// ClassicRandom.uniform (ai/random.js). The numeric binding keeps the shared
/// AI stream's draw order without an import for every spread or chunks draw.
pub(crate) fn random_draw(unit: Ref<Unit>, weapon: Option<u32>) -> f64 {
    let index = weapon.map_or(0, |i| i as usize + 1);
    let stream = state().random_bindings[unit.slot as usize][index];
    draw_stream(stream)
}
pub(crate) fn empty_text() -> crate::store::StrId { state().empty_text }
pub(crate) fn take_melee(_unit: Ref<Unit>) -> crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey> {
    std::mem::take(&mut state().melee)
}
pub(crate) fn put_melee(_unit: Ref<Unit>, scratch: crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey>) {
    state().melee = scratch;
}
pub(crate) fn take_arc(_unit: Ref<Unit>) -> Vec<crate::combat::live::ArcVictim> {
    std::mem::take(&mut state().arc)
}
pub(crate) fn put_arc(_unit: Ref<Unit>, targets: Vec<crate::combat::live::ArcVictim>) {
    state().arc = targets;
}
pub(crate) fn take_ordnance_query() -> (usize, Vec<Ref<Unit>>) {
    let depth = state().ordnance_depth;
    state().ordnance_depth += 1;
    (depth, std::mem::take(&mut state().ordnance_queries[depth]))
}
pub(crate) fn put_ordnance_query(depth: usize, mut query: Vec<Ref<Unit>>) {
    assert_eq!(state().ordnance_depth,depth + 1);
    query.clear();
    state().ordnance_depth = depth;
    state().ordnance_queries[depth] = query;
}
pub(crate) fn random_gameplay() -> f64 {
    let globals = F3RandomGlobals::take(&state().random_globals, &[]);
    draw_stream(globals.gameplay)
}
pub(crate) fn random_spin() -> f64 {
    let globals = F3RandomGlobals::take(&state().random_globals, &[]);
    draw_stream(globals.spin)
}
pub(crate) fn random_drop() -> f64 {
    let globals = F3RandomGlobals::take(&state().random_globals, &[]);
    draw_stream(globals.drop)
}
// Vehicle.fireSeat and TurretSeats.drive share the seat-weapons channel.
// Its first draw keeps the same lazy insertion slot as the JS named stream.
pub(crate) fn seat_binding() -> u32 {
    F3RandomGlobals::take(&state().random_globals, &[]).seat
}
// Mounted weapons can replace their default channel with the Classic AI's
// shared generator. The retained numeric id follows that specific weapon.
pub(crate) fn random_binding(binding: u32) -> f64 { draw_stream(binding) }
fn draw_stream(stream: u32) -> f64 { (random_next(stream) >> 9) as f64 / 8388608.0 }
/// Classic spawning uses raw modulo draws from the same generator as AI.
/// Preserve the lazy channel stamp before advancing either representation.
pub(crate) fn random_next(stream: u32) -> u32 {
    if stream>=RANDOM_CAPACITY as u32 {fail();return 0;}
    // The epoch covers both fractional weapon draws and raw spawn draws.
    let epoch=&mut state().random_epoch[F3RandomEpoch::REVISION];
    *epoch=(*epoch as u32).wrapping_add(1) as f64;
    let at = stream as usize * F3RandomState::WORDS;
    let mut row = F3RandomState::take(&state().random_states[at..], &[]);
    if !row.present {
        row.present = true;
        state().random_sequence += 1;
        row.sequence = state().random_sequence;
    }
    let value=if let Some(mut shared)=state().shared_random[stream as usize].clone() {
        let value=crate::ai::random::Random::next(&mut shared);row.state=shared.seed();value
    }else{
        let mut generator=jsrt::rng::ClassicRandom::from_state(row.state);
        let value=generator.next_u32();row.state=generator.state;value
    };
    row.put(&mut state().random_states[at..], &mut state().block.heap);
    value
}

// Arsenal and recon publish the same dynamic clocks. Definitions and other
// immutable weapon fields stay in the construction table, never in this row.
pub(crate) fn weapon_view(w:&Weapon) -> F3WeaponView {
    let defs=crate::combat::module::defs();
    weapon_view_with_defs(w,&defs.weapons)
}
fn weapon_view_with_defs(w: &Weapon,defs:&crate::combat::defs::WeaponDefs) -> F3WeaponView {
    F3WeaponView {
        thrown:w.thrown(defs),repair:w.repair(defs),
        level: w.level,
        admit_shot: w.admit_shot,
        deployed: w.deployed,
        firing: w.firing,
        timer: w.timer,
        bottomless: w.bottomless,
        per_shot: w.per_shot,
        clip: w.clip,
        reserve: w.reserve,
        heat: w.heat,
        overheated: w.overheated,
        floor: w.floor,
        kick: w.kick,
        spread_scale: w.spread_scale,
        sniper_scale: w.sniper_scale,
        yaw_spread_override: w.yaw_spread_override,
        wait: w.wait,
        reload_left: w.reload_left,
        throw_time: w.throw_time,
        released: w.released,
        hidden: w.hidden,
        charging: w.charging,
        held: w.held,
        charge: w.charge,
        limit: w.limit,
        pattern: w.pattern,
        salvos_left: w.salvos_left,
        salvo_wait: w.salvo_wait,
        salvo_seed: w.salvo_seed,
        salvo_count: w.salvo_count,
        lock_target_id: w.lock_target_id,
        lock_generation: w.lock_generation,
        lock_elapsed: w.lock_elapsed,
        lock_complete: w.lock_complete,
        melee: w.melee.as_ref().map(|m| Melee {
            time: m.time,
            index: m.index,
            active: m.active,
        }),
    }
}

/// captureUnitState observes the same authoritative visibility that the F3
/// mirror publishes. Missing rider nodes and destroyed hidden crew remain
/// invisible after vacating; no presentation object needs to be imported.
pub(crate) fn wire_visible(reference: Ref<Unit>, unit: &Unit) -> bool {
    let mut visible = !unit.removed && !(unit.dead && unit.combat.blow_apart)
        && !matches!(&unit.soldier, UnitBody::Droideka(d) if d.mode == 4.);
    if let Some(shown) = crate::vehicles::module::f2::live_seats::visible(unit) { visible &= shown; }
    if unit.dead && state().hidden_crew.get(reference.slot as usize).copied().flatten() == Some((reference, unit.generation)) { visible = false; }
    visible
}

// Build the narrow publication from native values, never from a JS mirror.
// Immutable class/loadout data stays in the construction tables. Dynamic
// correction fields remain here because network capture reads their timers.
// Reuse the weapon capacity. Native notification latches and pending muzzle
// shots have no foreign reader, so publication never stages those lists.
fn stage(out: &mut PublicationSnapshot, reference: Ref<Unit>, unit: &Unit) {
    out.value.arsenal = state().arsenals[reference.slot as usize];
    stage_body(&mut out.value,reference,unit);
    let c = &unit.combat;
    let v = &mut out.value.combat;
    v.health = c.health;
    v.dead = c.dead;
    v.death_time = c.death_time;
    v.last_attacker = c.last_attacker;
    v.repair_target = c.repair_target;
    v.repair_lock = c.repair_lock;
    v.blow_apart = c.blow_apart;
    v.zoom_step = c.zoom_step;
    v.kick.rate = c.kick.rate;
    v.kick.hold = c.kick.hold;
    v.kick.offset = c.kick.offset;
    v.mounted = c.mounted;
    v.gun_underwater = c.gun_underwater;
    v.origin = c.origin;
    v.target = c.target;
    v.aim_dir = c.aim_dir;
    v.shot_dir = c.shot_dir;
    v.left = c.left;
    v.up = c.up;
    v.pivot = c.pivot;
    v.selected = c.selected;
    // Network correction reads the complete dynamic weapon clocks. Class
    // levels, masks and definitions are immutable and are not mirrored here.
    // Reuse existing slots even when a new arsenal facade starts a life.
    v.weapons.resize_with(c.weapons.len(), F3WeaponView::default);
    let defs=crate::combat::module::defs();
    for (o, w) in v.weapons.iter_mut().zip(&c.weapons) {
        *o = weapon_view_with_defs(w,&defs.weapons);
    }
    stage_controls(out,reference,unit);
}
// Unit.onMoved precedes combat. Its body and consumed movement controls can
// change while the arsenal and weapon heap retain their last consumed bits.
fn stage_body(out:&mut F3Publication,reference:Ref<Unit>,unit:&Unit) {
    out.body = ownership::mirror(reference, unit);
    out.body.pooled = state().owner.pooled(reference);
    // Native rider placement owns visibility as well as the shape sample.
    // A hull's missing rider node hides its occupant; destruction keeps the
    // corpse hidden even after the lease has been cleared before death.
    if let Some(visible) = crate::vehicles::module::f2::live_seats::visible(unit) { out.body.visible &= visible; }
    if unit.dead && state().hidden_crew[reference.slot as usize] == Some((reference,unit.generation)) { out.body.visible = false; }
}
fn stage_controls(out:&mut PublicationSnapshot,reference:Ref<Unit>,unit:&Unit) {
    // Native F3 consumes bot controls directly. Only the held fields that
    // JavaScript still reads leave this owner; input and latch rows remain
    // native. Foreign controls keep their complete restoration contract.
    let i=&unit.intent;
    if state().native_intents[reference.slot as usize]==Some(reference) {
        out.held=Some(F3NativeIntent {forward:i.forward,right:i.right,turn:i.turn,pitch:i.pitch,
            buttons:i.buttons,aim:i.aim});
        return;
    }
    out.held=None;
    let F3PublishedControls::F3ForeignControls(foreign)=&mut out.value.controls else{unreachable!()};
    let controls=&mut foreign.values[0];
    controls.intent=i.clone();
    controls.was = unit.controls.was;
    controls.pressed0 = unit.controls.pressed.first().copied().unwrap_or(false);
    controls.pressed1 = unit.controls.pressed.get(1).copied().unwrap_or(false);
    let i = &unit.input;
    controls.input = MoveInput {
        forward: i.forward,
        right: i.right,
        turn: i.turn,
        pitch: i.pitch,
        jump: i.jump,
        crouch: i.crouch,
        prone: i.prone,
        r#use: i.r#use,
        jet_direction: i.jet_direction,
    };
}

// PlayerCombat.reset/damage/explode share the established arithmetic. Take
// the target and its combat out only while running it; JsWorld commits them
// around synchronous hooks so depth-first mutations observe current health.
// The command's source and attacker are copied facts, never store aliases.
fn mutate_combat(reference: Ref<Unit>, command: &F3Change, recon: Option<crate::units::f3::WorldServices<'_>>) {
    use crate::combat::{combat as cb, module as cm, world::DamageInfo};
    let defs = cm::defs();
    let (mut unit, mut combat) = cm::take(reference);
    let mut world = cm::JsWorld::with_services(reference, &unit, &combat,recon);
    match command.action {
        F3Action::Reset => {
            cb::reset(&mut combat, &unit, &defs);
            arsenal_reset(reference.slot);
        }
        F3Action::Damage => {
            let info = DamageInfo {
                explosion: command.flag,
                amount: command.damage_info.as_ref().and_then(|i| i.amount),
                typed: command.damage_info.as_ref().and_then(|i| i.typed),
                generic: command.damage_info.as_ref().and_then(|i| i.generic),
                known: command.damage_info.as_ref().and_then(|i| i.known),
                point: command.damage_info.as_ref().and_then(|i| match i.point_state {
                    F3OptionalState::Absent => None,
                    F3OptionalState::Null => Some(None),
                    F3OptionalState::Value => Some(Some(three::Vector3::new(i.point[0],i.point[1],i.point[2]))),
                }),
            };
            let source = command.source.map(|p| three::Vector3::new(p[0], p[1], p[2]));
            cb::damage_with(
                &mut combat,
                &mut unit,
                &defs,
                command.value,
                command.on_shield,
                source,
                command.attacker,
                &info,
                &mut world,
            );
        }
        F3Action::Explode => {
            let explosion = cm::blast_packet(command.index);
            let p = command.vector;
            cb::explode_prepared(
                &mut combat,
                &mut unit,
                &defs,
                &explosion,
                state().health[reference.slot as usize],
                &three::Vector3::new(p[0], p[1], p[2]),
                command.attacker,
                command.value,
                &mut world,
            );
        }
        _ => unreachable!(),
    }
    cm::put(reference, unit, combat);
}

// UnitFactory.acquire pops the last free body of this class. A pooled corpse
// keeps its registry slot and Unit identity; only a miss builds a new body.
// Acquiring does not advance life, reset weapons or observe the shape cache.
#[no_mangle]
pub extern "C" fn f3_acquire(class: u32) -> i32 {
    acquire_native(crate::store::StrId(class)).map_or(-1, |r| r.slot as i32)
}



/// Native Unit.sphere and hitShapes sensor service. Cache observation remains
/// lazy; publication and damage never ask it to rebuild.
// Units.query/raycast and Unit.sphere/hitShapes are explicit sensor reads.
// A shape request updates the cache only if tickCount invalidated it. Query
// output has separate storage so it cannot replace a pending body publication.
// Reuse result capacity even when a shorter query previously emptied the list.
#[no_mangle]
pub extern "C" fn f3_sensor() -> *const f64 {
    let input = F3Sensor::take(&state().command, &[]);
    let mut world = module::ModuleWorld::sensor(state().world);
    match input.mode {
        F3SensorMode::Bounds => {
            // Unit.sphere observes shapes at exactly the old updateSpheres
            // point. Only its bounding sphere crosses for an ordinary reader;
            // later hitShapes reads the already sampled canonical limb list.
            let Ok(unit) = input.key.validate_mut(&mut sim().store) else {
                fail();
                return std::ptr::null();
            };
            unit.sphere(input.key.slot, &mut world);
            cache_bounds(input.key.reference(),unit);
            let view = F3BoundsView {
                key: input.key, tick: unit.spheres_tick,
                bounds: Bounds { center: unit.bounds.center, radius: unit.bounds.radius },
            };
            let s = state();
            s.sensor_block.begin(1, F3BoundsView::WORDS);
            let (row, heap) = s.sensor_block.row(0);
            view.put(row, heap);
        }
        F3SensorMode::Shapes => {
            let Ok(unit) = input.key.validate_mut(&mut sim().store) else { fail(); return std::ptr::null(); };
            unit.hit_shapes(input.key.slot, &mut world);
            let s = state();
            s.shape.key = input.key;
            s.shape.tick = unit.spheres_tick;
            s.shape.bounds.center = unit.bounds.center;
            s.shape.bounds.radius = unit.bounds.radius;
            s.shape.shapes.clear();
            // JS boxes/capsules alias half to their existing halfSize vector.
            // Publish the half kind explicitly; the mirror can restore that
            // alias without replacing a vector when posture changes.
            for h in &unit.hit_set()[..unit.shape_count as usize] {
                s.shape.shapes.push(F3SensorShape {
                    r#type: h.r#type,
                    part: h.part,
                    center: h.center,
                    radius: h.radius,
                    half: h.half_number(),
                    half_is_size: matches!(h.half, Half::Vec3(_)),
                    half_size: h.half_size,
                    yaw: h.yaw,
                });
            }
            s.sensor_block.begin(1, F3ShapeView::WORDS);
            let (row, heap) = s.sensor_block.row(0);
            s.shape.put(row, heap);
        }
        F3SensorMode::Raycast => {
            // Game.los normally excludes its two endpoints with no custom
            // predicate. Canonical flags and these qualified refs need no
            // whole-registry acceptance mask from JavaScript.
            let mut excludes = [Member::Unit(Ref::new(0, 0)); 2];
            let mut count = 0;
            if let Some(r) = input.exclude0 { excludes[count] = Member::Unit(r); count += 1; }
            if let Some(r) = input.exclude1 { excludes[count] = Member::Unit(r); count += 1; }
            let hit = raycast(
                input.origin,
                input.direction,
                input.far,
                super::registry::Exclude::Many(&excludes[..count]),
                input.riders,
                input.masked,
            );
            let s = state();
            s.sensor_result.members.clear();
            s.sensor_result.hit.found = hit.is_some();
            if let Some(h) = hit {
                s.sensor_result.hit.unit = Some(sim().store.unit.at(h.unit).unwrap().0);
                s.sensor_result.hit.distance = h.distance;
                s.sensor_result.hit.point = [h.point.x, h.point.y, h.point.z];
                s.sensor_result.hit.normal = [h.normal.x, h.normal.y, h.normal.z];
                s.sensor_result.hit.part = crate::store::Nullable::Value(match h.part {
                    ShapePart::Body => HitPart::Body,
                    ShapePart::Head => HitPart::Head,
                });
            }
            s.sensor_block.begin(1, F3SensorResult::WORDS);
            let (row, heap) = s.sensor_block.row(0);
            s.sensor_result.put(row, heap);
        }
        F3SensorMode::Query => {
            // target_sphere reads foreign facts through the same owner.
            // Move only the retained output buffer out while querying; do
            // not hold a mutable State borrow across those sensor reads.
            let mut members = std::mem::take(&mut state().sensor_result.members);
            let store = &mut sim().store;
            store.units.query(
                &mut store.unit,
                &super::shapes::v3(input.origin),
                input.radius,
                &mut members,
                None,
                &mut world,
            );
            let s = state();
            s.sensor_result.members = members;
            s.sensor_result.hit.found = false;
            s.sensor_block.begin(1, F3SensorResult::WORDS);
            let (row, heap) = s.sensor_block.row(0);
            s.sensor_result.put(row, heap);
        }
    }
    let s = state();
    s.sensor_block.finish();
    s.sensor_block.ptr()
}

// Units.raycast keeps the first hit on an exact distance tie. Its ordered
// traversal and shape observation run over the canonical registry and slab.
// The optional mask is a JS sensor predicate's result, not imported geometry;
// excluded candidates must not observe their lazy shapes as a side effect.
pub(crate) fn raycast(
    origin: [f64; 3],
    direction: [f64; 3],
    far: f64,
    exclude: super::registry::Exclude,
    riders: bool,
    masked: bool,
) -> Option<super::registry::UnitHit> {
    let mut world = module::ModuleWorld::sensor(state().world);
    let store = &mut sim().store;
    let mut accept = |m: Member| !masked || state().input[super::registry::target_id(m) as usize] != 0.0;
    store.units.raycast(
        &mut store.unit,
        &super::shapes::v3(origin),
        &super::shapes::v3(direction),
        far,
        exclude,
        Some(&mut accept),
        riders,
        &mut world,
    )
}

/// AnimatedProp.seize/carry over the canonical body and retained skeleton.
// Seizure freezes motion without invalidating a previously observed shape.
// Only a successful carry advances tickCount. The later shape reader samples
// limbs in this final frame, preserving Unit.updateSpheres' lazy observation.
#[no_mangle]
pub extern "C" fn f3_pose() -> u32 {
    let mut input = F3PoseCommand::take(&state().command, &[]);
    let reference = input.key.reference();
    if input.key.validate(&sim().store).is_err() { return 0; }
    if state().running {return 0;}
    begin_events();
    let frame = match input.action {
        F3PoseAction::Seize => {
            let unit = sim().store.unit.get_mut(reference).unwrap();
            unit.held = crate::store::Nullable::Value(Held {
                prop: input.prop, since: input.since,
                frame: crate::store::Nullable::Undefined, time: None,
            });
            unit.combat.mounted = Some(true);
            match &mut unit.soldier {
                UnitBody::Soldier(s) => {
                    s.velocity = [0.0;3]; s.air = None; s.dive = 0.0;
                }
                UnitBody::Droideka(d) => d.velocity = [0.0;3],
            }
            super::live_pose::clear(reference);
            None
        }
        F3PoseAction::Carry => {
            let unit = sim().store.unit.get(reference).unwrap();
            if !matches!(&unit.held, crate::store::Nullable::Value(h)
                if h.prop == input.prop && h.since == input.since) { return 0; }
            let mut frame = three::Matrix4::default();
            if !super::live_pose::held_frame(&input, &mut frame) {return u32::from(!state().failed);}
            input.frame = frame.elements;
            let unit = sim().store.unit.get_mut(reference).unwrap();
            let position = [frame.elements[12], frame.elements[13], frame.elements[14]];
            match &mut unit.soldier {
                UnitBody::Soldier(s) => {
                    s.position = position; s.previous = position; s.settled = position;
                    s.velocity = [0.0;3];
                }
                UnitBody::Droideka(d) => {
                    d.position = position; d.previous = position; d.settled = position;
                    d.velocity = [0.0;3];
                }
            }
            if let crate::store::Nullable::Value(held) = &mut unit.held {
                held.frame = crate::store::Nullable::Value(frame.elements);
                held.time = Some(input.time);
            }
            unit.tick_count += 1.0;
            Some(frame.elements)
        }
    };
    F3HeldNotice { key: F3UnitKey {slot: input.key.slot,
        generation: input.key.generation, life: input.key.life}, prop: input.prop, grab: input.grab,
        since: input.since, placed: frame.is_some(), frame: frame.unwrap_or([0.0;16]),
        time: if frame.is_some() { Some(input.time) } else { None },
    }.emit(&mut sim().events);
    if frame.is_some() && !super::live_pose::set(input) {return 0;}
    publish(reference);
    1
}

/// Compact foreign target facts; these rows do not contain unit bodies.
// Units.query also sees buildings and other foreign targets. Their compact
// changed facts contain no Unit bodies. Updating one target must not clear the
// others: a later query can still meet a target that did not change this tick.
#[no_mangle]
pub extern "C" fn f3_foreign(count: u32) {
    foreign_rows(0,count);
    let header = F3ForeignHeader::take(&state().foreign_header, &[]);
    building_rows(count as usize * F3PropFact::WORDS, header.buildings);
}
// LiveObjects changes only affected building rows. This span follows registry
// facts in the same input reservation, including an outside AI sensor query.
fn building_rows(offset: usize, count: u32) {
    for i in 0..count as usize {
        let at = offset + i * F3BuildingState::WORDS;
        let frame = F3BuildingState::take(&state().input[at..], &[]);
        super::f3_props::changed(frame);
    }
}
pub(crate) fn prepare_prop_target(fact: F3PropFact) {
    let id = fact.id as usize;
    state().foreign.resize_with(state().foreign.len().max(id + 1), F3PropFact::default);
    state().foreign[id] = fact;
}
// A native prop mutation updates the existing registry fact without going back
// through JS geometry. Other targets keep their cached facts and insertion order.
pub(crate) fn prop_target(member: Member, alive: bool, health: f64, center: [f64; 3], radius: f64, generation: Option<f64>, team: f64) {
    let id = super::registry::target_id(member) as usize;
    if let Some(row) = state().foreign.get_mut(id).filter(|row| row.id as usize == id) {
        row.alive = alive;
        row.dead = !alive;
        row.health = health;
        row.team = team;
        row.bounds.center = center;
        row.bounds.radius = radius;
        row.aim = center;
        if let Some(generation) = generation { row.life = generation; }
    }
}
fn foreign_rows(offset: usize, count: u32) {
    let s = state();
    for i in 0..count as usize {
        let at = offset + i * F3PropFact::WORDS;
        // Changed rows are indexed by registry id. A short update is not
        // a shorter registry snapshot and must not discard untouched facts.
        let fact = F3PropFact::take(&s.input[at..], &[]);
        let id = fact.id as usize;
        if s.foreign.len() <= id {
            s.foreign.resize_with(id + 1, F3PropFact::default);
        }
        s.foreign[id] = fact;
    }
}
// Registry ids are shared by all member kinds. Indexing the prepared facts
// keeps a mixed query from looking up names or allocating per candidate.
pub(crate) fn target(id: u32) -> Option<&'static F3PropFact> {
    super::f3_hulls::refresh(id);
    super::f3_props::refresh(id);
    prepared_target(id)
}
/// Fixed prop facts require no mutable Store access. A phase holding Store
/// locally uses its owner directly for dynamic hull facts before this reader.
pub(crate) fn prepared_target(id: u32) -> Option<&'static F3PropFact> {
    state().foreign.get(id as usize).filter(|f| f.id == id)
}

// UnitFactory attachment may follow existing registry construction. History
// must reserve those initial members too, even before the first unit is built.
#[no_mangle]
pub extern "C" fn f3_registry_prepare() {
    reserve_registry();
}

// Units.add reserves future pad births at construction too. Their target
// ids are assigned only at activation, after any intervening soldier births.
pub(crate) fn reserve_registry() {
    let units = &mut sim().store.units;
    let count = units.list.len() + super::f3_hulls::prepared_count();
    units.list.reserve(count.saturating_sub(units.list.len()));
    let ids = units.next_id as usize + super::f3_hulls::prepared_count();
    state().foreign.resize_with(state().foreign.len().max(ids), F3PropFact::default);
    super::f3_hulls::reserve_members(ids);
    crate::vehicles::module::f2::reserve_registry(count);
    reserve_history();
}

/// Units.add gives every kind a registry id in one canonical sequence.
// Units.add allocates one id sequence for soldiers, hull targets, turrets
// and objectives. Keep append order: capture counts and equal-distance hits
// depend on it. Freeing a pooled body does not remove its registry membership.
#[no_mangle]
pub extern "C" fn f3_registry_add(kind: u32, generation: u32) -> u32 {
    let id = sim().store.units.add(|id| match kind as u16 {
        KIND_UNIT => Member::Unit(Ref::new(id, generation)),
        KIND_VEHICLE_TARGET => Member::VehicleTarget(Ref::new(id, generation)),
        KIND_TURRET_TARGET => Member::TurretTarget(Ref::new(id, generation)),
        KIND_OBJECTIVE_TARGET => Member::ObjectiveTarget(Ref::new(id, generation)),
        _ => unreachable!("a registry member kind"),
    });
    reserve_registry();
    id
}
// Units.remove erases the live list entry while preserving every survivor's
// order. This is distinct from Unit.remove, which returns a body to its pool.
#[no_mangle]
pub extern "C" fn f3_registry_remove(id: u32) {
    if let Some(m) = sim().store.units.by_id(id) {
        sim().store.units.remove(m);
    }
}

/// CollisionWorld.rayBodies names each body once when membership changes.
// CollisionWorld.rayBodies assigns stable body references when membership
// changes. Keep the generation as well as the kernel slot: ordnance callbacks
// must identify the exact body, including after a pooled slot is reused.
#[no_mangle]
pub extern "C" fn f3_body_names(count: u32) {
    for i in 0..count as usize {
        let at = i * F3BodyName::WORDS;
        let row = F3BodyName::take(&state().input[at..], &[]);
        let s = state();
        if s.bodies.len() <= row.kernel as usize {
            s.bodies.resize(row.kernel as usize + 1, None);
        }
        // Kernel slots and store references are different namespaces. The
        // reverse mapping belongs here so a cast can return an exact Ref
        // directly rather than asking JS to identify the collision body.
        s.bodies[row.kernel as usize] = Some(row.body);
        crate::vehicles::module::collision_world()
            .expect("prepared collision world")
            .name_body(row.body.slot, row.kernel);
    }
    // Body membership is a construction boundary. Build the cast index here,
    // rather than charging the first weapon's ray with index allocations.
    crate::collision::broad::prepare(crate::collision::state(),state().world);
    crate::movement::prepare_world_gathers(state().world);
}
// A native F1 cast returns the body that was named with the collision tree.
// It must never manufacture generation zero from the kernel's numeric slot.
pub(crate) fn body_reference(kernel: usize) -> Ref<Body> {
    state().bodies[kernel].expect("a collision body named at construction")
}

// Weapon-targeting's bot override is a phase fact from JS AI. Missing and null
// remain distinct: humans acquire through their cone, a bot with no target does not.
pub(crate) fn bot_target(slot: u32) -> Option<Option<Member>> {
    state().bot_targets[slot as usize]
}

// Ordnance.flightMissile stores a class name. Resolve the four immutable
// names once; guidance subsequently indexes this table without decoding text.
#[no_mangle]
pub extern "C" fn f3_missile_classes(count: u32) {
    let (rows, _) = sim().calls.args();
    for index in 0..count as usize {
        let row = F3MissileClass::take(&rows[index * F3MissileClass::WORDS..],&[]);
        let at = row.name.0 as usize;
        if state().missile_classes.len() <= at { state().missile_classes.resize(at + 1,None); }
        state().missile_classes[at] = Some(row.class);
    }
}
pub(crate) fn missile_class(name: Option<crate::store::StrId>) -> FlightClass {
    name.map_or(FlightClass::Fighter,|name| {
        state().missile_classes[name.0 as usize].expect("prepared missile class")
    })
}
pub(crate) fn category(slot: u32) -> u32 {
    state().categories[slot as usize]
}

// Units.setRelation and mission team tables arrive when the table changes.
// Sensor reads compare numeric pairs; they never format or intern "a:b" keys.
#[no_mangle]
pub extern "C" fn f3_relations(count:u32) -> u32 {
    let side=count.isqrt() as usize;
    if side*side!=count as usize {return 0;}
    let (rows,_)=sim().calls.args();
    let s=state();s.relation_sides=side;s.relations.resize(side*side,-1.);
    // Units.setTeams supplies complete directional pairs in numeric order.
    // Retain a dense index so each ray/arc/repair query reads one value.
    for row in rows.chunks_exact(F3RelationFact::WORDS).take(count as usize) {
        let fact=F3RelationFact::take(row,&[]);
        let a=fact.a as usize;let b=fact.b as usize;
        if a>=side || b>=side {return 0;}
        s.relations[a*side+b]=fact.value;
    }
    1
}
pub(crate) fn relation(a:f64,b:f64) -> f64 {
    if a==b {return if a!=0. && !a.is_nan() {1.} else {0.};}
    if a==0. || b==0. || a.is_nan() || b.is_nan() {return 0.;}
    let s=state();let ai=a as usize;let bi=b as usize;
    if ai>=s.relation_sides || bi>=s.relation_sides || ai as f64!=a || bi as f64!=b {return -1.;}
    s.relations[ai*s.relation_sides+bi]
}

// HitHistory.capture belongs at the multiplayer world's end-of-tick capture
// boundary, after ordnance and items. Capturing never imports body rows.
#[no_mangle]
pub extern "C" fn f3_history_capture(tick: u32) {
    let allocations = crate::allocations::snapshot();
    crate::allocations::begin();
    // F4 may have advanced foreign building life after the unit span. Those
    // changed rows start at zero because capture has no intent rows before it.
    let foreign = F3ForeignHeader::take(&state().foreign_header, &[]);
    foreign_rows(0, foreign.count);
    building_rows(foreign.count as usize * F3PropFact::WORDS, foreign.buildings);
    let history = &mut state().history as *mut crate::combat::history::History<Member>;
    // The history service calls only canonical sensors, which do not touch it.
    unsafe {
        (*history).capture(tick, &mut super::f3_sensors::Sensors);
    }
    finish_allocations(allocations);
    descriptor();
}
pub(crate) fn history() -> *mut crate::combat::history::History<Member> {
    &mut state().history
}

// networkShotFor's fixed channels are input facts, not body mirrors. An edge
// view overrides the current rendered tick only for that particular press.
pub(crate) fn network(slot: u32) -> &'static F3NetworkView {
    &state().network[slot as usize]
}

// OrdnanceSim.send resolves complete damage fields once. F1 keeps its flight
// definition numbering; this table adds policy without duplicating flight rows.
#[no_mangle]
pub extern "C" fn f3_hit_policy() {
    let (row, _) = sim().calls.args();
    let input = F3HitPolicy::take(row, &[]);
    let id = input.ordnance.0 as usize;
    let size = state().hit_policies.len().max(id + 1);
    state().hit_policies.resize(size, None);
    state().hit_policies[id] = Some(crate::combat::impacts::HitPacket {
        damage: input.damage,
        scales: [input.person, input.animal, input.droid, input.vehicle, input.building],
        shield: input.shield,
    });
}
#[no_mangle]
pub extern "C" fn f3_blast_binding() {
    let (row, _) = sim().calls.args();
    let input = F3BlastBinding::take(row, &[]);
    let size = state().blasts.len().max(input.explosion as usize + 1);
    state().blasts.resize(size, None);
    state().blasts[input.explosion as usize] = Some(input.combat);
}
#[no_mangle]
pub extern "C" fn f3_impact_rules() {
    let (row, _) = sim().calls.args();
    let rules = F3ImpactRules::take(row, &[]);
    state().friendly_fire = rules.friendly_fire;
}
/// Ordnance.explosion outside the owned span is one world command, not a
/// JavaScript loop issuing one damage command for every registry member.
#[no_mangle]
pub extern "C" fn f3_world_blast() {
    let (row,heap) = sim().calls.args();
    let input = F3BlastCall::take(row,heap);
    let _boundary = external();
    with_services(|recon| crate::combat::live::blast(crate::store::DefId(input.explosion),input.position,input.owner,recon));
}
pub(crate) fn blast_packet(def: crate::store::DefId) -> crate::combat::impacts::BlastPacket {
    let combat = state().blasts[def.0 as usize].expect("prepared explosion damage");
    crate::combat::module::blast_packet(combat)
}
pub(crate) fn hit_policy(def: crate::store::DefId) -> crate::combat::impacts::HitPacket {
    state().hit_policies[def.0 as usize].expect("prepared hit policy")
}
pub(crate) fn health_scale(slot: u32) -> crate::combat::impacts::HealthScale {
    state().health[slot as usize]
}
pub(crate) fn friendly_fire() -> bool {
    state().friendly_fire
}
// AI commands mutate only Intent; they do not invalidate lazy geometry.
// Keep a separate touched list so a concurrent lifecycle/weapon/body write
// wins the full publication without being mislabeled as an input-only edit.
pub(crate) fn intent_changed(reference:Ref<Unit>) {
    // Keep first-touch order while rejecting duplicates in constant time.
    // A retired storage generation cannot suppress its replacement's touch.
    let s=state();let stamp=&mut s.intent_marked[reference.slot as usize];
    if *stamp==Some(reference.gen) {return;}
    *stamp=Some(reference.gen);s.intent_changed.push(reference);
}
// AI navigation avoidance and prepareSpread keep their source writes. Only
// encoding waits for the following unit phase; synchronous barriers flush it.
pub(crate) fn ai_changed(reference:Ref<Unit>) {
    state().ai_full_marked[reference.slot as usize]=Some(reference.gen);
    intent_changed(reference);
}
pub(crate) fn changed(reference: Ref<Unit>) {
    // Seat placement can advance tickCount outside Unit.tick. Its qualified
    // touch must retire an earlier shared bounds sample before JS observes it.
    if let Some(unit)=sim().store.unit.get(reference) {
        let at=reference.slot as usize*F3BoundsView::WORDS;
        if state().bounds_cache.get(at+F3BoundsView::TICK).is_some_and(|tick|*tick!=unit.tick_count) {
            invalidate_bounds(reference.slot);
        }
    }
    state().owner.mark(reference);
}

// Pure shot, damage and presentation notices use Sim.events, in source order.
// Each consumer drains only its new prefix; resetting a nested aimer command
// would discard events the surrounding native phase has not yet published.
fn begin_events() {
    acknowledge_publication();
    state().failed=false;
    // A notification can issue an outside-phase command. Preserve the unread
    // journal suffix until its consumer acknowledges it; the command appends
    // behind that suffix instead of erasing another victim's notification.
    let ack=F3Acknowledgement::take(&state().acknowledgement,&[]);
    if sim().events.is_empty() || (ack.event_epoch==state().event_epoch
        && ack.event_offset as usize==sim().events.len()) {
        sim().events.clear();
        state().event_epoch=state().event_epoch.wrapping_add(1);
    }
    let s = state();
    // A whole-world entry can cross several owner boundaries without a host
    // read. Keep the unread block across empty phases until acknowledgement.
    s.layout.version = s.layout.version.wrapping_add(1);
    if s.publication_ready.is_empty() {
        s.layout.output_rows = 0;
        s.layout.output_words = 0;
    }
}

/// F4 and F5 borrow the actual generated store and ownership bookkeeping.
/// The closure must finish before calling a native world service or import.
pub(crate) fn with_rows<T>(run: impl FnOnce(&mut Store,&mut Ownership)->T) -> T {
    run(&mut sim().store,&mut state().owner)
}
/// F4 acquires warm bodies without copying any Unit, body or combat row.
pub(crate) fn acquire_native(class: crate::store::StrId) -> Option<Ref<Unit>> {
    state().owner.acquire(&sim().store,class).map(|key|key.reference())
}
/// A cold miss activates an already prepared row through the same owner.
pub(crate) fn construct_native(class: crate::store::StrId, _services: WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::cold(class)
}
/// Player and connection realization is independent of the subsequent spawn.
pub(crate) fn realize_native(request:&F3PlayerRealization, services:WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::realize(request,services)
}
/// F4's player factory supplies canonical Character/body handles. Admission
/// has already registered stream and recon identities outside the phase.
pub(crate) fn realize_player_native(character:Ref<Character>, previous:Option<Ref<Unit>>,
    class:crate::store::StrId, at:[f64;3], yaw:f64, local:bool, services:WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::player(character,previous,class,at,yaw,local,services)
}
/// Character spawning binds metadata before Unit.spawn resets its loadout.
pub(crate) fn spawn_native(unit: Ref<Unit>, character: Ref<Character>, at:[f64;3], yaw:f64, mut services:WorldServices<'_>) {
    let _controls=control_span(true);
    let c=sim().store.character.get(character).expect("spawning Character");
    let (team,hero)=(c.team,c.hero);
    let u=sim().store.unit.get_mut(unit).expect("acquired native body");
    u.team=team;u.combat.team=team;u.character=Some(character);u.controller=None;
    module::lifecycle_owned(unit.slot,Some((at,yaw)),Some(services.reborrow()));
    sim().store.unit.get_mut(unit).unwrap().hero=hero==Some(true);
    crate::recon::module::spawned(unit,services.recon);changed(unit);
}
/// Removal finishes Character/seat/body lifecycle before pooling the object.
pub(crate) fn remove_native(reference:Ref<Unit>, services:WorldServices<'_>) {
    module::lifecycle_owned(reference.slot,None,Some(services));
    let unit=sim().store.unit.get(reference).expect("removed native Unit");
    state().owner.removed(&sim().store,&F3UnitKey::of(reference,unit)).expect("current removed Unit");
}
/// HitHistory can observe current registry members plus every cold body and
/// hull still awaiting activation. Registry ids also leave room for unrelated
/// future slots; allocating ten shapes per id in every frame wastes that room.
/// Call only during preparation, never when a prepared body becomes live.
pub(crate) fn reserve_history() {
    let count=sim().store.units.list.len()+super::f3_construction::prepared_count()+
        super::f3_hulls::prepared_count();
    state().history.reserve(count,Member::Unit(Ref::new(0,0)),10);
}

// Unit.sphere stays lazy. History has often observed it before JS asks;
// retain that exact observation in the existing generated bounds layout.
// A native tick invalidates this mirror without evaluating canonical shapes.
fn prepare_bounds(count:usize) {
    let s=state();
    let words=count*F3BoundsView::WORDS;
    if s.bounds_cache.len()<words {s.bounds_cache.resize(words,f64::NAN);}
    s.layout.bounds_cache=s.bounds_cache.as_mut_ptr() as u32;
    s.layout.bounds_capacity=(s.bounds_cache.len()/F3BoundsView::WORDS) as u32;
    descriptor();
}
pub(crate) fn invalidate_bounds(slot:u32) {
    if let Some(tick)=state().bounds_cache.get_mut(slot as usize*F3BoundsView::WORDS+F3BoundsView::TICK) {
        *tick=f64::NAN;
    }
}
pub(crate) fn cache_bounds(reference:Ref<Unit>,unit:&Unit) {
    let start=reference.slot as usize*F3BoundsView::WORDS;
    let Some(row)=state().bounds_cache.get_mut(start..start+F3BoundsView::WORDS) else {return;};
    let view=F3BoundsView {key:F3UnitKey::of(reference,unit),tick:unit.spheres_tick,
        bounds:Bounds {center:unit.bounds.center,radius:unit.bounds.radius}};
    // This fixed record has no heap payload, so the empty heap never allocates.
    view.put(row,&mut Vec::new());
}

/// Cold storage reserves empty sidecar slots; each prepared body carries its
/// own allocated query/rig buffers into the destination at activation.
pub(crate) fn reserve_native_slots(count:usize) {
    let s=state();let count=count.max(s.categories.len());
    s.publication.resize_with(count,||crate::vehicles::f2_delta::Cache::new(0));
    s.publication_marks.resize(count,None);
    s.completed.reserve(count.saturating_sub(s.completed.len()));
    s.intent_changed.reserve(count.saturating_sub(s.intent_changed.len()));
    s.intent_marked.resize(count,None);
    s.ai_full_marked.resize(count,None);
    s.phase_inputs.resize(count,None);
    s.phase_input_slots.reserve(count.saturating_sub(s.phase_input_slots.len()));
    s.publication_ready.reserve(count.saturating_sub(s.publication_ready.len()));
    s.categories.resize(count,0);s.health.resize(count,crate::combat::impacts::HealthScale::Person);
    s.bot_targets.resize(count,None);s.native_intents.resize(count,None);s.hidden_crew.resize(count,None);
    s.network.resize_with(count,F3NetworkView::default);s.arsenals.resize(count,0);
    s.edges.resize_with(count,Edges::default);s.infos.resize_with(count,UnitsPhaseUnit::default);
    s.random_bindings.resize_with(count,Vec::new);
    s.foreign.resize_with(count.max(s.foreign.len()),F3PropFact::default);
    s.items.reserve_unit(count);s.pushers.reserve(count);
    super::f3_hulls::reserve_members(count);
    // Prepared future births may become flight targets without a JS registry add.
    crate::vehicles::module::f2::reserve_registry(count);
    prepare_bounds(count);
}
pub(crate) fn take_bindings(slot:u32)->Vec<u32> {
    std::mem::take(&mut state().random_bindings[slot as usize])
}
pub(crate) fn install_native_slot(reference:Ref<Unit>, category:u32,
    health:crate::combat::impacts::HealthScale,bindings:Vec<u32>,
    water:Option<DroidekaWater>,mut publication:crate::vehicles::f2_delta::Cache) {
    let slot=reference.slot as usize;
    invalidate_bounds(reference.slot);
    let s=state();
    publication.invalidate();s.publication[slot]=publication;
    s.categories[slot]=category;s.health[slot]=health;s.random_bindings[slot]=bindings;
    s.infos[slot].unit=slot as f64;s.infos[slot].droideka_water=water;
    s.hidden_crew[slot]=None;s.bot_targets[slot]=None;s.native_intents[slot]=None;s.network[slot]=F3NetworkView::default();
    s.arsenals[slot]=s.arsenals[slot].wrapping_add(1);
}
pub(crate) fn discard_publication(reference:Ref<Unit>) {
    state().owner.unmark(reference);
    publish_changed();
}
/// The native lifecycle supplies numeric stream bindings. Mirror consumers
/// subsequently attach the already registered callbacks without setter calls.
pub(crate) fn bind_native_life(reference:Ref<Unit>, metadata:&F3Metadata, combat:u32, weapon:u32) {
    self::metadata(reference,metadata);
    let unit=sim().store.unit.get_mut(reference).unwrap();
    unit.combat.random=combat!=NO_STREAM;
    let count=unit.combat.weapons.len();
    let row=&mut state().random_bindings[reference.slot as usize];
    assert!(row.len()>count,"prepared weapon stream slots");
    row[0]=combat;
    for index in 0..count {row[index+1]=weapon;unit.combat.weapons[index].random=weapon!=NO_STREAM;}
    F3LifeStreams {key:F3UnitKey::of(reference,unit),combat,weapon,
        metadata:F3Metadata {character:unit.character,controller:unit.controller,
            team:unit.team,life:unit.generation,human:unit.human,
            network_human:unit.network_human,hero:unit.hero}}.emit(&mut sim().events);
    changed(reference);
}

/// MultiplayerWorld.unspawn keeps the admitted body between lives without
/// a death, ticket, corpse or new generation. Character ownership survives.
pub(crate) fn unspawn_native(reference:Ref<Unit>) {
    let u=sim().store.unit.get_mut(reference).expect("admitted connection body");
    u.alive=false;u.dead=true;u.combat.dead=true;u.removed=true;u.targetable=Some(false);
    u.dead_time=4.;u.combat.death_time=0.;
    match &mut u.soldier {UnitBody::Soldier(s)=>s.dead=Some(true),UnitBody::Droideka(d)=>d.dead=true}
    changed(reference);
}
/// Removed bodies are absent from Unit.tick, so their connection advances
/// this one clock before the next network spawn decision.
pub(crate) fn advance_removed_clock(reference:Ref<Unit>,dt:f64) {
    let u=sim().store.unit.get_mut(reference).expect("current connection clock");
    if !u.alive && u.removed && u.dead_time<4. {u.dead_time=v8math::min(4.,u.dead_time+dt);changed(reference);}
}

/// Inactive class storage retains its publication buffers as well as its rig.
/// Activation moves these buffers into a reserved slot without allocating.
pub(crate) fn take_publication(reference:Ref<Unit>) -> crate::vehicles::f2_delta::Cache {
    std::mem::replace(&mut state().publication[reference.slot as usize],
        crate::vehicles::f2_delta::Cache::new(0))
}

/// UnitController.bind/unbind owns the right to publish held intent fields.
/// External human commands keep their pending JS input until Unit.tick takes
/// it; native bot commands expose their canonical intent at the outer return.
pub(crate) fn native_intent(reference:Ref<Unit>,owned:bool) {
    state().native_intents[reference.slot as usize]=owned.then_some(reference);
    changed(reference);
}

/// Encoded outer payload size, without another export or body traversal.
// Empty native services may return without opening another event epoch only
// after every changed body and notification has reached its foreign reader.
pub(crate) fn settled()->bool {
    let deferred=crate::world::deferred();
    let s=state();
    // Within the enclosing world tick these rows and notices are already
    // canonical. Their deferred facade bytes require no new danger loan.
    if deferred {return !s.running;}
    !s.running && s.owner.changed().is_empty() && s.completed.is_empty() && s.intent_changed.is_empty()
        && s.acknowledgement[F3Acknowledgement::VERSION] as u32==s.layout.version
        && (sim().events.is_empty() || (s.acknowledgement[F3Acknowledgement::EVENT_EPOCH] as u32==s.event_epoch
            && s.acknowledgement[F3Acknowledgement::EVENT_OFFSET] as usize==sim().events.len()))
}
pub(crate) fn output_words()->u32 {state().layout.output_words}
pub(crate) fn publication_version()->u32 {state().layout.version}
