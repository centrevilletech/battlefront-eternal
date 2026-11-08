//! The persistent Unit command boundary. New bodies are adopted once from
//! UnitFactory.build; ordinary inputs never contain a body or shape cache.
use super::{
    module,
    ownership::{self, Ownership},
};
use crate::combat::{combat::Edges, services::WeaponService};
use crate::gen::*;
use crate::store::{block::Block, Ref};

const INPUT_WORDS:usize=PHASE_INPUT_WORDS as usize;
// Room for every connection's intent row and network tail (players::owner::PLAYER_CAPACITY).
const ROWS_WORDS:usize=65*(F3PhaseInput::WORDS+F3PhaseNetwork::WORDS);
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
pub(super) struct State {
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
    // The connections' rows (players::rows), beside the host's own rows.
    rows: Vec<f64>,
    rows_count: u32,
    rows_network: u32,
    command: [f64; F3Change::WORDS],
    layout: F3Boundary,
    descriptor: [f64; F3Boundary::WORDS],
    block: Block,
    services: Vec<Vec<std::rc::Rc<WeaponService>>>,
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
    history: Option<crate::combat::history::History<Member>>,
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
    ordnance_refused: u32,
}
/// The scalar command row a native caller fills before f3_command_in
/// (the host's F3Change mirror, src/f3-sim.js begin/commit).
pub(crate) fn command_row_in(units: &mut super::owner::Units) -> &mut [f64] {
    &mut units.state.as_mut().expect("prepared unit service").command[..]
}
/// Whether a native controller owns this body's intent (UnitController.bind).
pub(crate) fn native_intent_owned_in(units: &super::owner::Units, reference: Ref<Unit>) -> bool {
    units.state.as_ref().expect("prepared unit service").native_intents.get(reference.slot as usize) == Some(&Some(reference))
}
/// The connections' rows for this tick (players::rows), bounded by the
/// reserved room; the next unit phase reads them after the host's rows.
/// Returns how many rows did not fit (a degraded tick, counted by the caller).
pub(crate) fn connection_rows_in(units: &mut super::owner::Units, rows: &[F3PhaseInput], tails: &[F3PhaseNetwork]) -> usize {
    let state = units.state.as_mut().expect("prepared unit service");
    let mut heap = Vec::new();
    let count = rows.len().min(ROWS_WORDS / (F3PhaseInput::WORDS + F3PhaseNetwork::WORDS));
    let tails = &tails[..tails.len().min(count)];
    for (i, row) in rows[..count].iter().enumerate() { row.put(&mut state.rows[i * F3PhaseInput::WORDS..], &mut heap); }
    let at = count * F3PhaseInput::WORDS;
    for (i, tail) in tails.iter().enumerate() { tail.put(&mut state.rows[at + i * F3PhaseNetwork::WORDS..], &mut heap); }
    state.rows_count = count as u32;
    state.rows_network = tails.len() as u32;
    rows.len() - count
}
/// The connection rows the last players' phase wrote: rows and network tails.
pub(crate) fn connection_rows_written_in(units: &super::owner::Units) -> (u32, u32) {
    let state = units.state.as_ref().expect("prepared unit service");
    (state.rows_count, state.rows_network)
}
#[cfg(test)]
pub(crate) fn adopt_for_test_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>) {
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.adopt(&world_ctx.sim.store, reference).unwrap();
}
/// A recon context by its connection's key (F3Recon.contexts).
pub(crate) fn recon_context_by_key_in(world_ctx: &mut crate::world::owner::World, key: u32) -> Option<Ref<ReconContext>> {
    world_ctx.units.state.as_ref().expect("prepared unit service").recon.as_ref()?.context(&world_ctx.sim.store, key)
}
pub(crate) fn props_read(units: &super::owner::Units) -> &super::f3_props::Props { &units.state.as_ref().expect("prepared unit service").props }
// A battle owns one instance. This identifies ownership, not whether a tick
// is currently on the stack: supply and damage commands between ticks still
// use canonical state and the same native collision/ordnance services.
pub(crate) fn active_in(units: &super::owner::Units) -> bool {
    units.state.is_some()
}

/// F1 continues stepping ordnance after the unit span. Its native hits use the
/// same changed rows and event epoch, then publish once at that outer boundary.
/// An instant shot inside F3 joins its existing span instead of resetting it.
#[must_use]
pub(crate) struct External(bool);
pub(crate) fn external_in(world_ctx: &mut crate::world::owner::World) -> External {
    let outer = active_in(&world_ctx.units) && !world_ctx.units.state.as_mut().expect("prepared unit service").running;
    if outer { begin_events_in(world_ctx); world_ctx.units.state.as_mut().expect("prepared unit service").running = true; }
    External(outer)
}
impl External {
    pub(crate) fn is_outer(&self)->bool {self.0}
    /// Game.step immediately enters the native unit owner after AI. Ordinary
    /// held controls can wait for that owner's completed body publication;
    /// damage, construction and synchronous foreign barriers stay immediate.
    pub(crate) fn before_units(self, world_ctx: &mut crate::world::owner::World) { self.finish(world_ctx, false); }
    pub(crate) fn finish(mut self, world_ctx: &mut crate::world::owner::World, publish_intents:bool) {
        if self.0 {
            // The native export returned its authority borrow before this
            // guard. Recon views precede the final structural descriptor.
            with_services_in(world_ctx, |world_ctx, authority| {
                if let Some(services) = authority {
                    let authority = services.recon;
                    crate::recon::module::publish_active_in(world_ctx, authority);
                    crate::recon::module::publish_in(world_ctx, authority);
                }
            });
            // An event-free ordinary AI return has no foreign observer
            // before the promised Unit.tick span. Keep owner publications
            // together there; structural writes and notices stay immediate.
            // Shared RNG seeds still reach the facade before F3 pushes them.
            let deferred=!publish_intents && world_ctx.units.state.as_mut().expect("prepared unit service").owner.changed().is_empty()
                && world_ctx.units.state.as_mut().expect("prepared unit service").completed.is_empty() && world_ctx.sim.events.is_empty()
                && !crate::vehicles::module::f2::has_pending_in(world_ctx);
            if deferred {descriptor_in(world_ctx);}else{flush_completed_in(world_ctx);publish_changed_at_in(world_ctx, None,publish_intents);}
            world_ctx.units.state.as_mut().expect("prepared unit service").running=false;self.0=false;
        }
    }
}

// Unit.tick and Unit.spawn consume or reset controls. Scalar commands between
// ticks do neither: publishing their old native input would erase the next AI
// or network action already waiting on the retained JS intent object.
pub(crate) struct ControlSpan(Option<bool>);
pub(crate) fn control_span_in(world_ctx: &mut crate::world::owner::World, consumed: bool) -> ControlSpan {
    if !active_in(&world_ctx.units) { return ControlSpan(None); }
    let previous = world_ctx.units.state.as_mut().expect("prepared unit service").controls_consumed;
    world_ctx.units.state.as_mut().expect("prepared unit service").controls_consumed = consumed;
    ControlSpan(Some(previous))
}
impl ControlSpan {
    pub(crate) fn finish(self, world_ctx: &mut crate::world::owner::World) {
        if let Some(previous) = self.0 { world_ctx.units.state.as_mut().expect("prepared unit service").controls_consumed = previous; }
    }
}

// UnitFactory's soldiers and F1 rounds share the world's prepared collision
// tree. A unit does not create a second collision world when it is pooled.
pub(crate) fn collision_world_in(units: &super::owner::Units) -> usize {
    units.state.as_ref().expect("prepared unit service").world
}
pub(crate) fn tick_in(units: &super::owner::Units) -> f64 { units.state.as_ref().expect("prepared unit service").tick }
pub(crate) fn registered_viewpoint_in(units: &super::owner::Units) -> Option<Ref<Unit>> {units.state.as_ref().expect("prepared unit service").viewpoint}
pub(crate) fn viewpoint_in(units: &super::owner::Units) -> Ref<Unit> { units.state.as_ref().expect("prepared unit service").viewpoint.expect("registered player viewpoint") }
pub(crate) fn built_in(units: &super::owner::Units) -> & [Ref<Unit>] { units.state.as_ref().expect("prepared unit service").owner.built() }
pub(crate) fn unit_revision_in(units: &super::owner::Units) -> u64 { units.state.as_ref().expect("prepared unit service").owner.revision() }
// Vehicles.presentRiders keeps a hidden destroyed crew out of sight for
// that gameplay life. Pool reuse and slot reuse each invalidate this stamp.
pub(crate) fn hide_destroyed_crew_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>) {
    let unit = world_ctx.sim.store.unit.get(reference).expect("destroyed crew Unit");
    world_ctx.units.state.as_mut().expect("prepared unit service").hidden_crew[reference.slot as usize] = Some((reference,unit.generation));
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference);
}
pub(crate) fn unit_reference_in(units: &super::owner::Units, slot: u32) -> Ref<Unit> {
    units.state.as_ref().expect("prepared unit service").owner.reference(slot).expect("owned seat unit")
}
pub(crate) fn recon_services_in(units: &mut super::owner::Units) -> & mut crate::recon::module::Services {
    &mut units.state.as_mut().expect("prepared unit service").recon_services
}
pub(crate) fn props_in(units: &mut super::owner::Units) -> & mut super::f3_props::Props { &mut units.state.as_mut().expect("prepared unit service").props }
pub(crate) fn foreign_barrier_in(world_ctx: &mut crate::world::owner::World) { publish_changed_in(world_ctx); }
pub(crate) fn items_in(units: &mut super::owner::Units) -> & mut super::f3_items::Items { &mut units.state.as_mut().expect("prepared unit service").items }
pub(crate) fn item_target_input_in(units: &super::owner::Units, index: usize) -> F3ItemTarget {
    let at = index * F3ItemTarget::WORDS;
    F3ItemTarget::take(&units.state.as_ref().expect("prepared unit service").input[at..], &[])
}
/// The authoritative camera finishes before the AI/unit interval. Replace
/// only the separation position; global/network scope remains an input fact.
pub(crate) fn world_camera_in(units: &mut super::owner::Units, position: [f64;3]) {
    let mut row = F3Separation::take(&units.state.as_mut().expect("prepared unit service").separation, &[]);
    row.camera = position;
    row.put(&mut units.state.as_mut().expect("prepared unit service").separation, &mut Vec::new());
}
/// The enclosing world chooses target membership after ordnance, using the
/// same generated reservation as the standalone item entry.
pub(crate) fn world_item_target_in(units: &mut super::owner::Units, index: usize, row: &F3ItemTarget) {
    let at = index * F3ItemTarget::WORDS;
    row.put(&mut units.state.as_mut().expect("prepared unit service").input[at..at + F3ItemTarget::WORDS], &mut Vec::new());
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
    pub(crate) fn aim_in(&mut self, world_ctx: &mut crate::world::owner::World, key:&F3UnitKey,dt:f64)->bool {
        let Some(ai)=self.ai.as_mut() else{return false};ai.aim_in(world_ctx,key,dt,self.recon,self.queries);true
    }
    pub(crate) fn lifecycle_in(&mut self, world_ctx: &mut crate::world::owner::World, hook:&F4AIHook)->bool {
        let Some(ai)=self.ai.as_mut() else{return false};ai.lifecycle_in(world_ctx,hook,self.recon,self.queries);true
    }

    pub(crate) fn reborrow(&mut self) -> WorldServices<'_> {
        WorldServices {
            ai:self.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks),
            recon: self.recon,
            queries: self.queries,
        }
    }
}
pub(crate) fn with_services_in<T>(world_ctx: &mut crate::world::owner::World, run: impl FnOnce(&mut crate::world::owner::World, Option<WorldServices<'_>>) -> T) -> T {
    with_bare_services_in(world_ctx, |world_ctx, services| match services {
        Some(services) => crate::ai::owned::entry::with_callbacks_in(world_ctx, services.recon, services.queries, |world_ctx, services| run(world_ctx, Some(services))),
        None => run(world_ctx, None),
    })
}
/// The AI phase already owns its Core. It takes physical services without
/// first installing another callback scope over that enclosing AI instance.
pub(crate) fn with_bare_services_in<T>(world_ctx: &mut crate::world::owner::World, run: impl FnOnce(&mut crate::world::owner::World, Option<WorldServices<'_>>) -> T) -> T {
    if !active_in(&world_ctx.units) { return run(world_ctx, None); }
    let Some(mut loan) = world_ctx.units.state.as_mut().expect("prepared unit service").lend_services() else { return run(world_ctx, None); };
    let result = run(world_ctx, Some(WorldServices { ai: None, recon: &mut loan.recon, queries: &mut loan.queries }));
    world_ctx.units.state.as_mut().expect("prepared unit service").return_services(loan);
    result
}
// CombatWorld.fireRound/dispense resolve definitions once at construction.
// The immutable service remains at this address across synchronous hit hooks;
// no definition is installed by a weapon update in the middle of a phase.
pub(crate) fn weapon_service_in(units: &super::owner::Units, weapon: crate::store::DefId, level: f64) -> std::rc::Rc<WeaponService> {
    std::rc::Rc::clone(&units.state.as_ref().expect("prepared unit service").services[weapon.0 as usize][level as usize])
}

// UnitFactory construction reserves phase storage before units begin ticking.
// Inputs and the descriptor keep their addresses for the battle; output is a
// reused block whose current address is written into that descriptor.
// Large scratch capacities are not per-tick work and do not import any bodies.

pub fn f3_init_in(world_ctx: &mut crate::world::owner::World, world: u32, level: f64, body_serial: u32) -> Option<&[f64]> {
    super::f3_hulls::body_serial_in(&mut world_ctx.units, body_serial);
    super::f3_construction::reset_in(world_ctx);
    if let Some(collision) = crate::vehicles::module::collision_world_in(world_ctx) {
        assert_eq!(collision.world, world as usize, "one battle collision world");
    }
    module::setup_in(&mut world_ctx.units, world);
    crate::battle::f4::reset_in(&mut world_ctx.battle);
    if let Some(map) = crate::vehicles::module::map_in(world_ctx) { map.level = level; }
    crate::allocations::prepare();
    world_ctx.sim.events.reserve(65536, 128, 1024);
    // Ordnance.fire may insert and retire an instant bolt inside this phase.
    // Prepare the ordinary F1 slabs and live lists before the first weapon tick.
    let store = &mut world_ctx.sim.store;
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
    let input = vec![0.0; INPUT_WORDS];
    let rows = vec![0.0; ROWS_WORDS];
    let random_states = vec![0.0; RANDOM_CAPACITY * F3RandomState::WORDS];
    let aim_input = vec![0.0; AIM_CAPACITY * F3AimChange::WORDS];
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
        input: 0,
        input_capacity: INPUT_WORDS as u32,
        input_stride: F3PhaseInput::WORDS as u32,
        network_words: F3PhaseNetwork::WORDS as u32,
        random_states: 0,
        random_capacity: RANDOM_CAPACITY as u32,
        random_words: F3RandomState::WORDS as u32,
        random_global_words: F3RandomGlobals::WORDS as u32,
        aim_input: 0,
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
    {
        world_ctx.units.state = Some(State {
            owner: Ownership::default(),
            viewpoint: None,
            separation: [0.0; F3Separation::WORDS],
            pushers,
            world: world as usize,
            input,
            rows,
            rows_count: 0,
            rows_network: 0,
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
            history: Some(crate::combat::history::History::new(48)),
            random_states,
            shared_random:vec![None;RANDOM_CAPACITY],
            shared_bindings:Vec::with_capacity(3),
            random_epoch:[0.;F3RandomEpoch::WORDS],
            random_globals: [NO_STREAM as f64; F3RandomGlobals::WORDS],
            random_bindings: Vec::new(),
            random_sequence: 0,
            aim_input,
            aim_header: [0.0; F3AimHeader::WORDS],
            empty_text: world_ctx.sim.strings.intern_str(""),
            melee: crate::combat::targeting::MeleeScratch::new(RESERVED_UNITS),
            arc: Vec::with_capacity(RESERVED_UNITS * 2),
            ordnance_queries: (0..64).map(|_| Vec::with_capacity(RESERVED_UNITS)).collect(),
            ordnance_depth: 0,
            ordnance_refused: 0,
        });
    }
    #[cfg(target_arch = "wasm32")]
    world_ctx.units.state.as_mut().expect("prepared unit service").bind_inputs();
    // Standalone phases have no single camera. Production supplies the same
    // camera scope as Game.separateSoldiers before entering its owned span.
    { let state = world_ctx.units.state.as_mut().expect("prepared unit service"); F3Separation { global: true, ..Default::default() }
        .put(&mut state.separation, &mut state.block.heap); }
    descriptor_in(world_ctx)
}
// This generated record is the entire address/width contract. JavaScript
// reads memory directly after a call; getter exports would add a crossing for
// each output and make sparse publication needlessly expensive.
fn descriptor_in(world_ctx: &mut crate::world::owner::World) -> Option<&[f64]> {
    #[cfg(target_arch = "wasm32")]
    let event_ptr = world_ctx.sim.events.ptr() as u32;
    let event_words = world_ctx.sim.events.len() as u32;
    let s = world_ctx.units.state.as_mut().expect("prepared unit service");
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
    #[cfg(target_arch = "wasm32")]
    s.bind_events(event_ptr);
    s.layout.event_words = event_words;
    s.layout.event_epoch = s.event_epoch;
    s.layout.put(&mut s.descriptor, &mut s.block.heap);
    Some(&s.descriptor[..])
}

// The retained banks the descriptor names, for a native host that reads them
// by name rather than by address (Context::read_bank); None before f3_init.
pub(crate) fn descriptor_bank_in(w: &mut crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref()?; descriptor_in(w) }
pub(crate) fn output_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.block.words[..]) }
pub(crate) fn random_states_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.random_states[..]) }
pub(crate) fn random_epoch_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.random_epoch[..]) }
pub(crate) fn random_globals_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.random_globals[..]) }
pub(crate) fn aim_header_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.aim_header[..]) }
pub(crate) fn foreign_header_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.foreign_header[..]) }
pub(crate) fn bounds_cache_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.bounds_cache[..]) }
pub(crate) fn acknowledgement_bank_in(w: &crate::world::owner::World) -> Option<&[f64]> { w.units.state.as_ref().map(|s| &s.acknowledgement[..]) }
// The banks a host writes before a step or a command, for a host without
// the module's memory: the same rows the page writes at the descriptor's
// addresses, taken by name.
pub(crate) fn input_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.input[..]) }
pub(crate) fn separation_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.separation[..]) }
pub(crate) fn aim_input_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.aim_input[..]) }
pub(crate) fn aim_header_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.aim_header[..]) }
pub(crate) fn acknowledgement_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.acknowledgement[..]) }
pub(crate) fn random_globals_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.random_globals[..]) }
pub(crate) fn random_states_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.random_states[..]) }
pub(crate) fn foreign_header_bank_mut_in(w: &mut crate::world::owner::World) -> Option<&mut [f64]> { w.units.state.as_mut().map(|s| &mut s.foreign_header[..]) }

/// Publish the touched body only; do not compute hit shapes as a side effect.
// Unit.spawn, remove and scalar setters must be visible before returning to
// a caller that can immediately read health, position or weapon readiness.
// Preserve the Unit's lazy spheresTick cache: exporting a body is not one of
// updateSpheres' observation points and cannot turn a stale death pose fresh.
// Reuse the fleet's generated numeric delta cache. TickCount alone still
// changes each live Unit, but does not justify resending 175 body words and
// every weapon clock. Candidate touches become output only when bits changed.
fn encode_publication_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>, body_only:bool) -> bool {
    let unit=world_ctx.sim.store.unit.get(reference).expect("owned publication row");
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
    let flags = publication_flags(s, reference, unit);

    if body_only && s.native_intents[reference.slot as usize]==Some(reference) {
        stage_body_in(&world_ctx.fleet,&world_ctx.sim.store,flags, &mut s.snapshot.value,reference,unit);stage_controls_in(flags, &mut s.snapshot,reference,unit);
        let base=crate::store::block::HEADER;
        if let Some(changed)=s.publication[reference.slot as usize].prepare_retained(
            reference.slot,reference.gen,F3Publication::WORDS,
            &[base+F3Publication::BODY..base+F3Publication::ARSENAL,
              base+F3Publication::CONTROLS..base+F3Publication::WORDS],
            |row,heap|{s.snapshot.value.put_body(row,heap);s.snapshot.put_controls(row,heap)}) {
            return changed;
        }
    }
    stage_in(&world_ctx.fleet,&world_ctx.sim.store,&mut world_ctx.combat, flags, &mut s.snapshot,reference,unit);
    s.publication[reference.slot as usize].prepare(reference.slot,reference.gen,
        F3Publication::WORDS,|row,heap|s.snapshot.put(row,heap))
}
// JavaScript acknowledges a consumed version in the generated shared row.
// Until then every replacement block contains all pending deltas against the
// same consumed baseline. Native-only intermediate barriers cannot lose words.
fn acknowledge_publication_in(units: &mut super::owner::Units) {
    let s=units.state.as_mut().expect("prepared unit service");
    if s.acknowledgement[F3Acknowledgement::VERSION] as u32!=s.layout.version {return;}
    for r in s.publication_ready.drain(..) {
        s.publication[r.slot as usize].commit();
        s.publication_marks[r.slot as usize]=None;
    }
}
fn queue_publication_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>,body_only:bool) -> bool {
    let changed=encode_publication_in(world_ctx, reference,body_only);
    queue_encoded_in(&mut world_ctx.units, reference,changed)
}
// A narrow write still compares against the same acknowledged cache. Later
// full damage/movement publication can safely replace an unread intent delta.
fn queue_intent_publication_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>)->bool {
    // A previous unread full row can carry damage, death or movement. A
    // retained baseline alone cannot represent that suffix, so restage it.
    if world_ctx.units.state.as_mut().expect("prepared unit service").publication_marks[reference.slot as usize]==Some(reference.gen) {return queue_publication_in(world_ctx, reference,false);}
    let unit=world_ctx.sim.store.unit.get(reference).expect("owned AI input row");
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
    let flags = publication_flags(s, reference, unit);
stage_controls_in(flags, &mut s.snapshot,reference,unit);
    if s.snapshot.held.is_none() {return queue_publication_in(world_ctx, reference,false);}
    let base=crate::store::block::HEADER;
    let changed=s.publication[reference.slot as usize].prepare_retained(reference.slot,reference.gen,F3Publication::WORDS,
        &[base+F3Publication::CONTROLS..base+F3Publication::WORDS],|row,heap|{
            s.snapshot.put_controls(row,heap);
        });
    { let changed = changed.unwrap_or_else(|| encode_publication_in(world_ctx, reference, false)); queue_encoded_in(&mut world_ctx.units, reference, changed) }
}
fn queue_encoded_in(units: &mut super::owner::Units, reference:Ref<Unit>,changed:bool)->bool {
    let s=units.state.as_mut().expect("prepared unit service");let slot=reference.slot as usize;
    let pending=s.publication_marks[slot]==Some(reference.gen);
    if changed && !pending {
        s.publication_marks[slot]=Some(reference.gen);s.publication_ready.push(reference);
    } else if !changed && pending {
        s.publication_marks[slot]=None;
        s.publication_ready.retain(|&r|r!=reference);
    }
    changed||pending
}
pub(crate) fn publish_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>) {
    if crate::world::deferred_in(&world_ctx.tick) {world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference);return;}
    let _allocations=crate::allocations::at(F3AllocationStage::Publication);
    acknowledge_publication_in(&mut world_ctx.units);
    let changed=queue_publication_in(world_ctx, reference,false);
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.unmark(reference);
    if changed {write_publication_in(world_ctx);} else {descriptor_in(world_ctx);}
}

// Unit.tick can hurt another Unit immediately. First-touch order also holds
// across unread barriers; source and victim updates share one consumed epoch.
pub(crate) fn publish_changed_in(world_ctx: &mut crate::world::owner::World) {
    flush_completed_in(world_ctx);
    publish_changed_at_in(world_ctx, None,true);
}
fn flush_completed_in(world_ctx: &mut crate::world::owner::World) {
    for i in 0..world_ctx.units.state.as_mut().expect("prepared unit service").completed.len() {
        let reference = world_ctx.units.state.as_mut().expect("prepared unit service").completed[i];
        // A structural replacement may have retired a prepared slab object.
        // Publish only its still-live identity, never the replacement slot.
        if world_ctx.sim.store.unit.contains(reference) { world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference); }
    }
    world_ctx.units.state.as_mut().expect("prepared unit service").completed.clear();
}
fn completed_tick_in(units: &mut super::owner::Units, reference: Ref<Unit>) {
    units.state.as_mut().expect("prepared unit service").owner.unmark(reference);
    units.state.as_mut().expect("prepared unit service").completed.push(reference);
}
fn publish_changed_at_in(world_ctx: &mut crate::world::owner::World, body_only:Option<Ref<Unit>>,publish_intents:bool) {
    if crate::world::deferred_in(&world_ctx.tick) {crate::vehicles::module::f2::publish_pending_in(world_ctx);crate::ai::owned::entry::publish_controls_in(world_ctx);return;}
    crate::battle::f4_core::publish_in(world_ctx);
    crate::battle::f4_live::publish_in(world_ctx);
    crate::vehicles::module::f2::publish_pending_in(world_ctx);
    crate::ai::owned::entry::publish_controls_in(world_ctx);
    let _allocations=crate::allocations::at(F3AllocationStage::Publication);
    acknowledge_publication_in(&mut world_ctx.units);
    let mut changed=false;
    for i in 0..world_ctx.units.state.as_mut().expect("prepared unit service").owner.changed().len() {
        let reference=world_ctx.units.state.as_mut().expect("prepared unit service").owner.changed()[i];
        let weapon=world_ctx.units.state.as_mut().expect("prepared unit service").ai_full_marked[reference.slot as usize]==Some(reference.gen);
        changed|=queue_publication_in(world_ctx, reference,body_only==Some(reference)&&!weapon);
    }
    let mut retained=0;
    for i in 0..world_ctx.units.state.as_mut().expect("prepared unit service").intent_changed.len() {
        let reference=world_ctx.units.state.as_mut().expect("prepared unit service").intent_changed[i];
        let live=world_ctx.sim.store.unit.contains(reference);
        let full=world_ctx.units.state.as_mut().expect("prepared unit service").owner.is_marked(reference);
        if live&&!full {
            if !publish_intents {
                // Keep first-touch order and generation stamps until the
                // following unit span. A full lifecycle row supersedes it.
                world_ctx.units.state.as_mut().expect("prepared unit service").intent_changed[retained]=reference;retained+=1;continue;
            }
            changed|=if world_ctx.units.state.as_mut().expect("prepared unit service").ai_full_marked[reference.slot as usize]==Some(reference.gen) {
                queue_publication_in(world_ctx, reference,false)
            }else{queue_intent_publication_in(world_ctx, reference)};
        }
        let weapon=&mut world_ctx.units.state.as_mut().expect("prepared unit service").ai_full_marked[reference.slot as usize];
        if *weapon==Some(reference.gen) {*weapon=None;}
        let stamp=&mut world_ctx.units.state.as_mut().expect("prepared unit service").intent_marked[reference.slot as usize];
        if *stamp==Some(reference.gen) {*stamp=None;}
    }
    world_ctx.units.state.as_mut().expect("prepared unit service").intent_changed.truncate(retained);world_ctx.units.state.as_mut().expect("prepared unit service").owner.clear_changed();
    if changed {write_publication_in(world_ctx);} else {descriptor_in(world_ctx);}
}
fn write_publication_in(world_ctx: &mut crate::world::owner::World) {
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
let count=s.publication_ready.len();
    s.block.begin(count,F2Delta::WORDS);
    for i in 0..count {
        let reference=s.publication_ready[i];
        let (row,heap)=s.block.row(i);
        s.publication[reference.slot as usize].delta.value.put(row,heap);
    }
    finish_publication_in(world_ctx, count);
}

fn finish_publication_in(world_ctx: &mut crate::world::owner::World, count:usize) {
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
s.block.finish();
    s.layout.version=s.layout.version.wrapping_add(1);
    #[cfg(target_arch = "wasm32")]
    s.bind_output();
    s.layout.output_rows=count as u32;
    // Direct cache reads count as traffic too, although they no longer take
    // space in the output heap. The shared masks bound exactly these words.
    let borrowed: usize = s.publication_ready.iter()
        .map(|r| s.publication[r.slot as usize].borrowed_words()).sum();
    s.layout.output_words=(count*F2Delta::WORDS+s.block.heap.len()+borrowed) as u32;
    descriptor_in(world_ctx);
}

// Unit.onMoved receives its current facing/movement in UnitAimArgs. Publish
// actual damage/lifecycle changes here, without
// re-encoding this unchanged combat and the rest of its correction body.
// The current row is restored before the import so AI sensors still query
// canonical geometry at their original observation point.
pub(crate) fn before_hook_in(world_ctx: &mut crate::world::owner::World) {
    // Aimers consume only their declared current movement and their own
    // previous combat. Other completed weapon clocks wait for the outer
    // barrier; actual damage/lifecycle writers remain in the changed queue.
    publish_changed_at_in(world_ctx, None,true);
}

/// Construction-only adoption. Refusing an unknown generation never creates
/// or overwrites a row as the legacy ensure/import path does.
// UnitFactory.build loads one complete Unit before entering the free-pool
// contract. Slab generation identifies that object; gameplay generation is a
// separate value that Unit.spawn advances on reuse. Adoption changes neither.
// Reserve the mover and shape scratch now, not on the first active tick.

pub fn f3_adopt_in(world_ctx: &mut crate::world::owner::World) -> u32 {
    // Reading this row by value releases scratch before loading retained
    // class services. Nothing points into JS-owned memory after the call.
    let adoption = F3Adoption::take(&world_ctx.units.state.as_mut().expect("prepared unit service").command, &[]);
    let reference = adoption.key.reference();
    let slot = reference.slot;
    if adoption.key.validate(&world_ctx.sim.store).is_err() {
        return 0;
    }
    let adopted = if adoption.passive { world_ctx.units.state.as_mut().expect("prepared unit service").owner.adopt_passive(&world_ctx.sim.store, reference) }
        else { world_ctx.units.state.as_mut().expect("prepared unit service").owner.adopt(&world_ctx.sim.store, reference) };
    if adopted.is_err() {
        return 0;
    }
    module::reserve_unit_in(&mut world_ctx.units, slot);
    crate::vehicles::module::f2::live_seats::adopt_unit_in(world_ctx, reference, adoption.has_actor, adoption.no_enter);
    let capacity = world_ctx.units.state.as_mut().expect("prepared unit service").owner.built().len();
    let history_capacity = world_ctx.sim.store.units.list.len().max(capacity);
    world_ctx.units.state.as_mut().expect("prepared unit service").pushers.reserve(history_capacity);
    reserve_history_in(world_ctx);
    { let s = world_ctx.units.state.as_mut().expect("prepared unit service"); s.completed.reserve(capacity.saturating_sub(s.completed.len())); s.intent_changed.reserve(capacity.saturating_sub(s.intent_changed.len())); }
    let size = world_ctx.units.state.as_mut().expect("prepared unit service").categories.len().max(capacity).max(slot as usize + 1);
    world_ctx.units.state.as_mut().expect("prepared unit service").intent_marked.resize(size,None);
    world_ctx.units.state.as_mut().expect("prepared unit service").ai_full_marked.resize(size,None);
    world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs.resize(size,None);
    { let s=world_ctx.units.state.as_mut().expect("prepared unit service");
s.phase_input_slots.reserve(size.saturating_sub(s.phase_input_slots.len())); }
    world_ctx.units.state.as_mut().expect("prepared unit service").categories.resize(size, 0);
    prepare_bounds_in(world_ctx, size);invalidate_bounds_in(&mut world_ctx.units, slot);
    world_ctx.units.state.as_mut().expect("prepared unit service").items.reserve_unit(size);
    world_ctx.units.state.as_mut().expect("prepared unit service").categories[slot as usize] = adoption.category;
    world_ctx.units.state.as_mut().expect("prepared unit service").bot_targets.resize(size, None);world_ctx.units.state.as_mut().expect("prepared unit service").native_intents.resize(size,None);
    world_ctx.units.state.as_mut().expect("prepared unit service").hidden_crew.resize(size, None);
    world_ctx.units.state.as_mut().expect("prepared unit service").network.resize_with(size, F3NetworkView::default);
    world_ctx.units.state.as_mut().expect("prepared unit service").arsenals.resize(size, 0);
    let health_name = adoption.key.validate(&world_ctx.sim.store).unwrap().combat.scale;
    let name = String::from_utf16_lossy(world_ctx.sim.strings.units(health_name));
    let health = crate::combat::impacts::HealthScale::from_scale(&name);
    if world_ctx.units.state.as_mut().expect("prepared unit service").health.len() <= slot as usize {
        world_ctx.units.state.as_mut().expect("prepared unit service")
            .health
            .resize(slot as usize + 1, crate::combat::impacts::HealthScale::Person);
    }
    world_ctx.units.state.as_mut().expect("prepared unit service").health[slot as usize] = health;
    let unit = world_ctx.sim.store.unit.get_mut(reference).unwrap();
    if let UnitBody::Soldier(body) = &mut unit.soldier {
        super::soldier::prepare_in(&mut world_ctx.movement, body, world_ctx.units.state.as_mut().expect("prepared unit service").world);
        // Soldier.touchPolygons first contact must use construction storage.
        // Reservation does not run a hidden movement tick or fill lazy caches.
        world_ctx.movement.reserve_mover(body.mover as u32, 256, 16, 512);
    }
    unit.combat.pending_muzzles.reserve(64);
    // Slot zero binds combat's chunks draw; subsequent slots bind arsenal
    // entries. Reset installs a new arsenal through this same construction seam.
    let weapons = unit.combat.weapons.len();
    if world_ctx.units.state.as_mut().expect("prepared unit service").publication.len()<=slot as usize {
        world_ctx.units.state.as_mut().expect("prepared unit service").publication.resize_with(slot as usize+1,||crate::vehicles::f2_delta::Cache::new(0));
    }
    { let state = world_ctx.units.state.as_mut().expect("prepared unit service"); state.publication_marks.resize(state.publication.len(), None); }
    // Foreign controls occupy one heap row even for an empty arsenal. A body
    // may change controller ownership, so reserve both publication shapes at
    // adoption rather than growing its first movement or corpse publication.
    let words=crate::store::block::HEADER+F3Publication::WORDS+
        weapons*F3WeaponView::WORDS+F3ControlView::WORDS;
    world_ctx.units.state.as_mut().expect("prepared unit service").publication[slot as usize]=crate::vehicles::f2_delta::Cache::borrowed(words);
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Native publications carry values as well as masks. Reserve the
        // complete admitted roster here, before its first changed-row flush.
        let state = world_ctx.units.state.as_mut().expect("prepared unit service");
        let heap: usize = state.publication.iter().map(|cache| {
            let words = cache.next.words.capacity(); words + words.div_ceil(crate::vehicles::f2_delta::MASK_BITS)
        }).sum();
        state.block.heap.reserve(heap.saturating_sub(state.block.heap.len()));
        let total = crate::store::block::HEADER + state.publication.len() * F2Delta::WORDS + heap;
        state.block.words.reserve(total.saturating_sub(state.block.words.len()));
    }

    let bindings = &mut world_ctx.units.state.as_mut().expect("prepared unit service").random_bindings;
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
    if world_ctx.units.state.as_mut().expect("prepared unit service").edges.len() <= slot as usize {
        world_ctx.units.state.as_mut().expect("prepared unit service").edges.resize_with(slot as usize + 1, Edges::default);
    }
    let infos = &mut world_ctx.units.state.as_mut().expect("prepared unit service").infos;
    if infos.len() <= slot as usize {
        infos.resize_with(slot as usize + 1, UnitsPhaseUnit::default);
    }
    infos[slot as usize].unit = slot as f64;
    infos[slot as usize].droideka_water = adoption.droideka_water;
    publish_in(world_ctx, reference);
    1
}

/// Prepared F1 handles for a weapon's charge levels (CombatWorld.fireRound).
// CombatWorld's weapon can change charge level without changing its facade.
// Keep a service for every merged level. Resolve the class name for dispenser
// admission here so the update loop never creates a String or searches a map.

pub fn f3_weapon_services_in(world_ctx: &mut crate::world::owner::World, weapon: u32, count: u32) {
    let (rows, _) = world_ctx.sim.calls.args();
    let mut levels = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let at = i * F3WeaponService::WORDS;
        let row = F3WeaponService::take(&rows[at..], &[]);
        levels.push(std::rc::Rc::new(WeaponService {
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
        }));
    }
    let all = &mut world_ctx.units.state.as_mut().expect("prepared unit service").services;
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

pub fn f3_step_in(world_ctx: &mut crate::world::owner::World, count: u32, dt: f64, level: f64, tick: f64) -> u32 {
    world_ctx.units.state.as_mut().expect("prepared unit service").tick = tick;
    with_services_in(world_ctx, |world_ctx, authority| step_in(world_ctx, count,dt,level,authority))
}
fn step_in(world_ctx: &mut crate::world::owner::World, count: u32, dt: f64, level: f64, mut recon: Option<crate::units::f3::WorldServices<'_>>) -> u32 {
    let control_guard = control_span_in(world_ctx, true);
    let result = (|| {
    let allocations = crate::allocations::snapshot();
    begin_events_in(world_ctx);
    // WaterState has already advanced in Game.step. Later F1 crossings use
    // this same bobbed surface until the next authoritative world tick.
    if let Some(map) = crate::vehicles::module::map_in(world_ctx) { map.level = level; }
    world_ctx.units.state.as_mut().expect("prepared unit service").running = true;
    crate::allocations::begin();
    if count as usize>INPUT_WORDS/F3PhaseInput::WORDS {
        world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
    }
    // Foreign props share the one phase input reservation after the intents.
    // Apply changed facts before a launcher or melee can read this registry.
    let foreign = F3ForeignHeader::take(&world_ctx.units.state.as_mut().expect("prepared unit service").foreign_header,&[]);
    if foreign.network as usize>INPUT_WORDS/F3PhaseNetwork::WORDS ||
        foreign.count as usize>INPUT_WORDS/F3PropFact::WORDS ||
        foreign.buildings as usize>INPUT_WORDS/F3BuildingState::WORDS {
        world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
    }
    let network_at = count as usize * F3PhaseInput::WORDS;
    let offset = network_at + foreign.network as usize * F3PhaseNetwork::WORDS;
    if offset+foreign.count as usize*F3PropFact::WORDS+foreign.buildings as usize*F3BuildingState::WORDS>INPUT_WORDS {
        world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
    }
    foreign_rows_in(&mut world_ctx.units, offset,foreign.count);
    building_rows_in(world_ctx, offset + foreign.count as usize * F3PropFact::WORDS, foreign.buildings);
    // F3.phase prepares mounted hints before updating the first body.
    // Keep that snapshot point: a later shot can replace a controller.
    if let Some(ai)=recon.as_ref().and_then(|services|services.ai.as_ref()) {crate::vehicles::module::f2::firing::freeze_ai_targets_in(world_ctx, &**ai);}
    // Foreign requests are indexed before any callbacks can reuse input memory.
    // Omitted native bots resolve current Core ownership before any body
    // advances, never the page's hint alone. Empty standalone phases
    // retain the existing no-new-request behavior for unowned bodies.
    for i in 0..world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots.len() {
        let slot=world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots[i];world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs[slot]=None;
    }
    world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots.clear();
    let rows_count=world_ctx.units.state.as_ref().expect("prepared unit service").rows_count as usize;
    let rows_network=world_ctx.units.state.as_ref().expect("prepared unit service").rows_network;
    for i in 0..count as usize+rows_count {
        let at=if i<count as usize {i*F3PhaseInput::WORDS} else {(i-count as usize)*F3PhaseInput::WORDS};
        let key={let state=world_ctx.units.state.as_ref().expect("prepared unit service");
            let source=if i<count as usize {&state.input} else {&state.rows};
            F3UnitKey::take(&source[at+F3PhaseInput::KEY..at+F3PhaseInput::KEY+F3UnitKey::WORDS],&[])};
        if key.validate(&world_ctx.sim.store).is_err() {
            if crate::world::deferred_in(&world_ctx.tick) {continue;}
            world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
        }
        let slot=key.slot as usize;
        if world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs.get(slot).is_none() || world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs[slot].is_some() ||
            world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots.len()==world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots.capacity() {
            world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
        }
        world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs[slot]=Some(at);world_ctx.units.state.as_mut().expect("prepared unit service").phase_input_slots.push(slot);
    }
    // Explicit rows also include passive human/rider bodies, which the body
    // tick list deliberately excludes. Preserve their source input order;
    // append only omitted native bots, whose requests are already canonical.
    let built=world_ctx.units.state.as_mut().expect("prepared unit service").owner.built().len();
    let explicit=count as usize+rows_count;
    for i in 0..explicit+built {
        let (reference,mut input)=if i<explicit {
            let connection=i>=count as usize;
            let at=if connection {(i-count as usize)*F3PhaseInput::WORDS} else {i*F3PhaseInput::WORDS};
            let input={let state=world_ctx.units.state.as_ref().expect("prepared unit service");
                F3PhaseInput::take(&(if connection {&state.rows} else {&state.input})[at..at+F3PhaseInput::WORDS],&[])};
            if crate::world::deferred_in(&world_ctx.tick) && input.key.validate(&world_ctx.sim.store).is_err() {continue;}
            (input.key.reference(),Some(input))
        }else{
            let reference=world_ctx.units.state.as_mut().expect("prepared unit service").owner.built()[i-explicit];
            if world_ctx.units.state.as_mut().expect("prepared unit service").phase_inputs[reference.slot as usize].is_some() {continue;}
            let Some(unit)=world_ctx.sim.store.unit.get(reference) else {continue};
            if unit.removed {continue;}
            (reference,None)
        };
        let ai_owned=recon.as_ref().and_then(|services|services.ai.as_ref()).is_some_and(|ai|ai.owns_intent(&world_ctx.sim.store,reference));
        // The enclosing input was prepared before AI. Boarding can move
        // this controller to a native SeatUnit in that interval, leaving no
        // foreign foot-intent row. Its canonical seated body remains current.
        let retained_body=crate::world::deferred_in(&world_ctx.tick) && world_ctx.sim.store.unit.get(reference).is_some_and(|unit| {
            unit.combat.dead || unit.seat.is_some() && unit.character
                .and_then(|character|world_ctx.sim.store.character.get(character))
                .is_some_and(|character|character.unit==Some(reference)&&character.controller.is_some())
        });
        if !ai_owned && !retained_body {
            let Some(input)=input.as_mut() else {
                if recon.as_ref().is_some_and(|services|services.ai.is_some()) {
                    world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
                }
                continue;
            };
            // Missing intent is accepted only for an independently verified
            // canonical AI owner; a foreign row cannot acquire that authority.
            let Some(intent)=input.intent.take() else {world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;};
            if world_ctx.units.state.as_mut().expect("prepared unit service").owner.intent(&mut world_ctx.sim.store,F3IntentCommand {key:F3UnitKey {slot:input.key.slot,generation:input.key.generation,life:input.key.life},intent}).is_err() {
                world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;
            }
        }
        world_ctx.units.state.as_mut().expect("prepared unit service").bot_targets[reference.slot as usize] = if let Some(ai)=recon.as_ref().and_then(|services|services.ai.as_ref()) {
            let hint=ai.weapon_hint_in(world_ctx,reference,None);hint.bot.then_some(hint.target)
        }else{input.as_ref().filter(|input|input.bot_target_set).map(|input|input.bot_target)};
        // A connection's tail follows the connections' rows, as the host's
        // tail follows its own.
        let connection=i>=count as usize && i<explicit;
        let network=if let Some(input)=input.as_ref().filter(|input|input.has_network) {
            if input.network_index>=if connection {rows_network} else {foreign.network} {world_ctx.units.state.as_mut().expect("prepared unit service").running=false;finish_allocations_in(&mut world_ctx.units, allocations);return 0;}
            let at=(if connection {rows_count*F3PhaseInput::WORDS} else {network_at})+input.network_index as usize*F3PhaseNetwork::WORDS;
            let state=world_ctx.units.state.as_ref().expect("prepared unit service");
            Some(F3PhaseNetwork::take(&(if connection {&state.rows} else {&state.input})[at..at+F3PhaseNetwork::WORDS],&[]))
        }else{None};
        world_ctx.units.state.as_mut().expect("prepared unit service").network[reference.slot as usize]=network.as_ref().map(|r|F3NetworkView {
            view:r.network.view,lead:r.network.lead,playout:r.network.playout,
            fire0:r.network.fire0,fire1:r.network.fire1,view0:r.network.view0,view1:r.network.view1,
        }).unwrap_or_default();
        world_ctx.sim.store.unit.get_mut(reference).unwrap().remotes_active=crate::recon::module::active_in(world_ctx, reference);
        if let Some(context)=crate::recon::module::context_in(world_ctx, reference) {
            world_ctx.sim.store.recon_context.get_mut(context).unwrap().first_person=input.as_ref().is_some_and(|input|input.remote_first_person);
        }
        world_ctx.units.state.as_mut().expect("prepared unit service").infos[reference.slot as usize].remote=crate::recon::module::context_in(world_ctx, reference).is_some();
        // An empty shot list remains present and suppresses a legacy edge.
        world_ctx.units.state.as_mut().expect("prepared unit service").edges[reference.slot as usize]=network.map(|input|Edges {
            shots:None,shot:input.edge.as_ref().map(crate::combat::module::edge),
            fixed:input.edge_list.then(||[input.edge0.as_ref().map(crate::combat::module::edge),input.edge1.as_ref().map(crate::combat::module::edge)]),
        }).unwrap_or_default();
    }
    let _tick_allocations = crate::allocations::at(F3AllocationStage::Other);
    // Unit.tick itself keeps the seated, held and removed gates. Keep water
    // and death-region tests immediately after that individual tick: a kill
    // can affect what the next unit observes and who its shot is credited to.
    let n = world_ctx.units.state.as_mut().expect("prepared unit service").owner.built().len();
    for i in 0..n {
        let reference = world_ctx.units.state.as_mut().expect("prepared unit service").owner.built()[i];
        let Some(u) = world_ctx.sim.store.unit.get(reference) else {
            continue;
        };
        if u.removed {
            continue;
        }
        let facts = &world_ctx.units.state.as_mut().expect("prepared unit service").infos[reference.slot as usize];
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
        let edges = std::mem::take(&mut world_ctx.units.state.as_mut().expect("prepared unit service").edges[reference.slot as usize]);
        module::tick_owned_in(world_ctx, &info, level, dt, edges,recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference);
        let unit = world_ctx.sim.store.unit.get(reference).unwrap();
        if unit.removed {
            world_ctx.units.state.as_mut().expect("prepared unit service")
                .owner
                .removed(&world_ctx.sim.store, &F3UnitKey::of(reference, unit))
                .unwrap();
        }
        completed_tick_in(&mut world_ctx.units, reference);
    }
    // Game.step interposes Vehicles.update and TurretSeats.update here.
    // Lend the same services to physics, seats and mounted fire; an exported
    // fleet wrapper would start another ownership boundary inside this one.
    if crate::vehicles::module::f2::active_in(&world_ctx.fleet) {
        let tick = world_ctx.units.state.as_ref().expect("prepared unit service").tick;
        crate::vehicles::module::f2::step_in(world_ctx, dt, tick,
            recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
    }
    // Recon finish also advances removed, seated and retired
    // owners after hulls/seats, before the world's separation pass.
    if let Some(services) = recon.as_mut() {
        let authority = &mut *services.recon;
        let recon_tick = world_ctx.units.state.as_ref().unwrap().tick;
        crate::recon::module::with_native(world_ctx, None, authority, services.queries, services.ai.as_mut().map(|ai|&mut **ai as &mut dyn crate::ai::owned::callbacks::Callbacks), |authority, store, world| authority.finish(store, world, recon_tick, dt)).unwrap_or_else(|_|fail_in(&mut world_ctx.units));
        crate::recon::module::publish_active_in(world_ctx, authority);
        crate::recon::module::publish_in(world_ctx, authority);
    }
    // Game.separateSoldiers follows vehicles/seats and recon finish. Read the
    // live registry now, including corpses and bodies moved by those services.
    // Taking retained scratch avoids keeping a State borrow beside the slab.
    let input = F3Separation::take(&world_ctx.units.state.as_mut().expect("prepared unit service").separation, &[]);
    let scope = super::separate::Scope {
        player: world_ctx.units.state.as_mut().expect("prepared unit service").viewpoint.map(|reference| reference.slot),
        player_out: world_ctx.units.state.as_mut().expect("prepared unit service").viewpoint.and_then(|r| world_ctx.sim.store.unit.get(r))
            .is_some_and(|u| u.combat.dead || u.seat.is_some()),
        global: input.global,
        camera: three::Vector3::new(input.camera[0], input.camera[1], input.camera[2]),
    };
    let mut pushers = std::mem::take(&mut world_ctx.units.state.as_mut().expect("prepared unit service").pushers);
    let store = &mut world_ctx.sim.store;
    super::separate::separate(&store.units, &mut store.unit, &scope, &mut pushers);
    for reference in pushers.changed() { world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference); }
    world_ctx.units.state.as_mut().expect("prepared unit service").pushers = pushers;
    publish_changed_in(world_ctx);
    // Include input parsing, solver work, callbacks and sparse serialization.
    // Descriptor writes below use fixed storage and cannot allocate themselves.
    finish_allocations_in(&mut world_ctx.units, allocations);
    world_ctx.units.state.as_mut().expect("prepared unit service").running = false;
    descriptor_in(world_ctx);
    u32::from(!world_ctx.units.state.as_mut().expect("prepared unit service").failed)

    })();
    control_guard.finish(world_ctx);
    result
}

// Owned spans report real allocator traffic, including serialization. Reuse
// this recorder for end-of-tick capture rather than estimating its Vec growth.
fn finish_allocations_in(units: &mut super::owner::Units, before: crate::allocations::Counts) {
    crate::allocations::end();
    let used = crate::allocations::snapshot().since(before);
    units.state.as_mut().expect("prepared unit service").layout.allocations = used.calls as u32;
    units.state.as_mut().expect("prepared unit service").layout.reallocations = used.reallocs as u32;
    units.state.as_mut().expect("prepared unit service").layout.allocation_bytes = used.bytes as u32;
}

// Unit spawn metadata contains no solver state. Network lifecycle commands
// can qualify a manually advanced gameplay generation through this old key.
fn metadata_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>, m: &F3Metadata) {
    invalidate_bounds_in(&mut world_ctx.units, reference.slot);
    let u = world_ctx.sim.store.unit.get_mut(reference).unwrap();
    u.character = m.character; u.controller = m.controller; u.team = m.team;
    u.generation = m.life; u.human = m.human; u.network_human = m.network_human;
    u.hero = m.hero; u.combat.team = m.team;
}

// PlayerCombat.reset replaces weapon identity even when gameplay life stays
// unchanged. Unit.spawn reaches the same hook through ModuleWorld.
pub(crate) fn arsenal_reset_in(units: &mut super::owner::Units, id: u32) {
    if active_in(units) { units.state.as_mut().expect("prepared unit service").arsenals[id as usize] = units.state.as_mut().expect("prepared unit service").arsenals[id as usize].wrapping_add(1); }
}

// Game.step increments the dormant viewpoint before AI can observe shapes.
// The native world driver calls this at that slot; its host only consumes
// the touched publication, so no setter export reenters the driver.
pub(crate) fn viewpoint_count_in(world_ctx: &mut crate::world::owner::World, count: f64) {
    if !active_in(&world_ctx.units) { return; }
    if let Some(reference) = world_ctx.units.state.as_mut().expect("prepared unit service").viewpoint {
        invalidate_bounds_in(&mut world_ctx.units, reference.slot);
        world_ctx.sim.store.unit.get_mut(reference).unwrap().tick_count = count;
        changed_in(world_ctx, reference); publish_changed_in(world_ctx);
    }
}

// Vehicles.spawnSlots samples the player's blocker after unit combat. A
// death earlier in this span must clear it before the pad checks occupancy.
pub(crate) fn spawn_occupant_in(world_ctx: &mut crate::world::owner::World) -> Option<three::Vector3> {
    let unit = world_ctx.sim.store.unit.get(world_ctx.units.state.as_mut().expect("prepared unit service").viewpoint?)?;
    (!unit.combat.dead).then(|| unit.position())
}

/// Scalar commands share one reserved generated row. The store's key, never
/// a slot-only lookup or a JavaScript life counter, authorizes the mutation.
// The command is a stack value before any hook can recurse into this export.
// Thus a nested hurt cannot overwrite its caller's attacker, position or key.
// Validate before taking the body out of the store; failed keys change nothing.
// A valid command can still be refused by gameplay, such as a busy selection.

pub fn f3_command_in(world_ctx: &mut crate::world::owner::World) -> u32 {
    // ClassicCombatBinding.attachController binds weapon streams inside the
    // tail's spawn hook. These scalar settings need no body/recon services;
    // borrowing them again would conflict with the enclosing native phase.
    if world_ctx.units.state.as_mut().expect("prepared unit service").recon_borrowed {
        let action=F3Change::take(&world_ctx.units.state.as_mut().expect("prepared unit service").command,&[]).action;
        assert!(matches!(action,F3Action::RandomBinding|F3Action::CombatField|F3Action::WeaponField),
            "body command inside native world hook");
        command_in(world_ctx, None)
    } else {with_services_in(world_ctx, |world_ctx, services| command_in(world_ctx, services))}
}
fn command_in(world_ctx: &mut crate::world::owner::World, mut recon: Option<crate::units::f3::WorldServices<'_>>) -> u32 {
    let command = F3Change::take(&world_ctx.units.state.as_mut().expect("prepared unit service").command, &[]);
    let control_guard = control_span_in(world_ctx, command.action == F3Action::Spawn);
    let result = (|| {
    let reference = command.key.reference();
    if command.key.validate(&world_ctx.sim.store).is_err() {
        return 0;
    }
    if !world_ctx.units.state.as_mut().expect("prepared unit service").running { begin_events_in(world_ctx); }
    world_ctx.units.state.as_mut().expect("prepared unit service").layout.accepted = true;
    match command.action {
        F3Action::RandomBinding | F3Action::CombatField | F3Action::WeaponField => {
            // Network spawn flags precede combat.player in the source. Keep
            // those pending metadata writes on this same touched-row command.
            if let Some(m) = command.metadata.as_ref() { metadata_in(world_ctx, reference,m); }
            if !apply_setting_in(world_ctx, reference,command.action,command.field,command.index,command.value) {return 0;}
        }
        // Game.spawnUnit sets hero status after Unit.spawn. Metadata has
        // no body fields and does not reset timers or advance gameplay life.
        F3Action::Metadata => {
            let Some(m)=command.metadata.as_ref() else {return 0;};
            let previous = world_ctx.sim.store.unit.get(reference).unwrap().generation;
            metadata_in(world_ctx, reference,m);
            // MultiplayerWorld.addPlayer advances a reconnect's life without
            // resetting combat. The bound context follows that live owner;
            // already launched droids keep their frozen attribution.
            if previous != m.life {
                if let Some(services) = recon.as_mut() {
                    crate::recon::module::spawned_in(world_ctx, reference, &mut *services.recon);
                }
            }
        }
        // Human spawning changes teams before reset; combat and Unit team
        // must agree before any synchronous credit or damage hook reads them.
        F3Action::Team => {
            world_ctx.units.state.as_mut().expect("prepared unit service")
                .owner
                .team(&mut world_ctx.sim.store, &command.key, command.value)
                .unwrap();
        }
        // Soldier.lookRaw keeps each mouse sample's normalization order.
        // Mouse look itself does not invalidate tickCount's shape cache.
        F3Action::Look | F3Action::LookRaw => {
            world_ctx.units.state.as_mut().expect("prepared unit service")
                .owner
                .look(
                    &mut world_ctx.sim.store,
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
            invalidate_bounds_in(&mut world_ctx.units, reference.slot);
            module::with_unit_in(world_ctx,reference,|world,u| {
                u.spawn_body(command.vector[0],command.vector[1],command.vector[2],command.value,world);
            }).expect("validated unit placement");
        }
        // MultiplayerWorld advances a removed body's respawn clock before
        // factory iteration, which deliberately skips that removed body.
        F3Action::DeadTime => world_ctx.sim.store.unit.get_mut(reference).unwrap().dead_time = command.value,
        // PathExecutor.contactSidestep consumes the last movement contact at
        // its AI read slot. Clearing it is not a movement tick or a shape
        // invalidation, and the next solver starts with this exact zero.
        F3Action::ConsumeContact => match &mut world_ctx.sim.store.unit.get_mut(reference).unwrap().soldier {
            UnitBody::Soldier(s) => s.avoidance = [0.0;3],
            UnitBody::Droideka(d) => d.avoidance = [0.0;3],
        },
        // Game's warm factory construction explicitly releases unused bodies.
        // This joins the same class stack as Unit.remove, without running death
        // or lifecycle callbacks on a body that has never entered a life.
        F3Action::PoolRelease => {
            if world_ctx.units.state.as_mut().expect("prepared unit service").owner.removed(&world_ctx.sim.store,&command.key).is_err() {return 0;}
        }
        F3Action::Health => {
            world_ctx.sim.store.unit.get_mut(reference).unwrap().combat.health = command.value;
        }
        // ClassicCombatBinding.attachController replaces a seat's default
        // stream after boarding returns. Apply that foreign scalar immediately;
        // a later unrelated publication must not restore the old callback.
        F3Action::MountedRandom => {
            world_ctx.units.state.as_mut().expect("prepared unit service").layout.accepted = crate::vehicles::module::f2::live_seats::bind_random_in(world_ctx,
                reference,match command.lease {Some(lease)=>lease,None=>return 0},command.value as u32);
        }
        // Arsenal.select/next can refuse a busy channel without making this
        // a stale command. Report that gameplay result separately from key validity.
        F3Action::Select | F3Action::Next => {
            let defs = crate::combat::module::defs_in(&mut world_ctx.combat);
            let c = &mut world_ctx.sim.store.unit.get_mut(reference).unwrap().combat;
            world_ctx.units.state.as_mut().expect("prepared unit service").layout.accepted = if command.action == F3Action::Select {
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
                let u = world_ctx.sim.store.unit.get_mut(reference).unwrap();
                u.character = m.character;
                u.controller = m.controller;
                u.team = m.team;
                u.generation = m.life;
                u.human = m.human;
                u.network_human = m.network_human;
                u.hero = m.hero;
                u.combat.team = m.team;
            }
            module::lifecycle_owned_in(world_ctx, command.key.slot, Some((command.vector, command.value)),recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            if let Some(services) = recon.as_mut() {
                crate::recon::module::spawned_in(world_ctx, reference, services.recon);
            }
        }
        // Unit.remove leaves a corpse's registry identity intact and returns
        // reusable bodies to their class stack. Network-owned bodies are not free.
        F3Action::Remove => {
            if let Some(m) = command.metadata.as_ref() { metadata_in(world_ctx, reference,m); }
            module::lifecycle_owned_in(world_ctx, command.key.slot, None,recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            let unit = world_ctx.sim.store.unit.get(reference).unwrap();
            world_ctx.units.state.as_mut().expect("prepared unit service")
                .owner
                .removed(&world_ctx.sim.store, &F3UnitKey::of(reference, unit))
                .unwrap();
        }
        // Game's original viewpoint is registered once at construction. This
        // names its slab object, so an ordinary respawn keeps literal ownership.
        F3Action::Viewpoint => world_ctx.units.state.as_mut().expect("prepared unit service").viewpoint = Some(reference),
        F3Action::Controller => world_ctx.sim.store.unit.get_mut(reference).unwrap().controller = command.controller,
        F3Action::LetGo => {
            world_ctx.sim.store.unit.get_mut(reference).unwrap().let_go();
            super::live_pose::clear_in(&mut world_ctx.units, reference);
        }
        // Soldier.setAir clears posture/state time at this exact call point.
        // Death detachment must not leave a jet trooper in the jet state.
        F3Action::BodyAir => {
            if let UnitBody::Soldier(s) = &mut world_ctx.sim.store.unit.get_mut(reference).unwrap().soldier {
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
                let u=world_ctx.sim.store.unit.get_mut(reference).unwrap();u.alive=false;u.combat.dead=true;u.removed=true;
            } else {unspawn_native_in(world_ctx, reference);}
        }

        F3Action::Reset | F3Action::Damage | F3Action::Explode => mutate_combat_in(world_ctx, reference, &command,recon.as_mut().map(crate::units::f3::WorldServices::reborrow)),
    }
    // Publish the command's row and every victim actually reached by its
    // immediate recursive damage. Unaffected registry members stay untouched.
    if let Some(services) = recon.as_mut() {
        let authority = &mut *services.recon;
        crate::recon::module::publish_active_in(world_ctx, authority);
        crate::recon::module::publish_in(world_ctx, authority);
    }
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.mark(reference);
    // Mouse look/contact consumption changes no weapon or combat clocks.
    // Reuse the consumed heap unless an unread publication still owns it.
    let body_only = (matches!(command.action, F3Action::Look | F3Action::LookRaw |
        F3Action::BodyAir | F3Action::BodySpawn | F3Action::DeadTime | F3Action::ConsumeContact)
        && world_ctx.units.state.as_mut().expect("prepared unit service").publication_marks[reference.slot as usize] != Some(reference.gen)).then_some(reference);
    publish_changed_at_in(world_ctx, body_only,true);
    u32::from(!world_ctx.units.state.as_mut().expect("prepared unit service").failed)

    })();
    control_guard.finish(world_ctx);
    result
}

// CombatFacade's scalar setters and the AI aimer use one application path.
// A setting touches its own row; neither path imports a Unit or weapon table.
fn apply_setting_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>,action:F3Action,field:u32,index:u32,value:f64) -> bool {
    match action {
        F3Action::RandomBinding=> {
            let Some(slot)=world_ctx.units.state.as_mut().expect("prepared unit service").random_bindings[reference.slot as usize].get_mut(index as usize) else {return false;};
            let id=value as u32;
            if id!=NO_STREAM && id as usize>=RANDOM_CAPACITY {return false;}
            *slot=id;true
        }
        F3Action::CombatField=> {
            if crate::combat::module::cmb_set_in(world_ctx, reference.slot,field,value)==0 {return false;}
            if field==F3CombatField::Team as u32 {world_ctx.sim.store.unit.get_mut(reference).unwrap().team=value;}
            true
        }
        F3Action::WeaponField=>crate::combat::module::cmb_weapon_set_in(world_ctx, reference.slot,index,field,value)!=0,
        _=>false,
    }
}

// A missing service is a rejected boundary operation, not a WASM trap. Keep
// the current native frame valid until the outer phase reports its failure.
pub(crate) fn fail_in(units: &mut super::owner::Units) {if active_in(units) {units.state.as_mut().expect("prepared unit service").failed=true;units.state.as_mut().expect("prepared unit service").layout.accepted=false;}}
macro_rules! required_services {
    ($units:expr; $value:expr,$fallback:expr)=>{match $value {Some(services)=>services,None=>{$crate::units::f3::fail_in($units);return $fallback;}}};
}
pub(crate) use required_services;

/// Unit.onMoved's aimer returns deltas in a generated block. Applying them
/// after the import preserves scalar write order without nested WASM exports.
pub(crate) fn begin_aimer_in(units: &mut super::owner::Units) {
    { let state = units.state.as_mut().expect("prepared unit service"); F3AimHeader { count: 0 }.put(&mut state.aim_header, &mut state.block.heap); }
}
pub(crate) fn finish_aimer_in(world_ctx: &mut crate::world::owner::World) {
    let header = F3AimHeader::take(&world_ctx.units.state.as_mut().expect("prepared unit service").aim_header, &[]);
    if header.count as usize>AIM_CAPACITY {fail_in(&mut world_ctx.units);return;}
    for index in 0..header.count as usize {
        let at = index * F3AimChange::WORDS;
        let change = F3AimChange::take(&world_ctx.units.state.as_mut().expect("prepared unit service").aim_input[at..], &[]);
        if change.key.validate(&world_ctx.sim.store).is_err() ||
            !apply_setting_in(world_ctx, change.key.reference(),change.action,change.field,change.index,change.value) {fail_in(&mut world_ctx.units);return;}
        changed_in(world_ctx, change.key.reference());
    }
}

/// Bind the bare AI generator once at native world construction. Unit fire
/// consumes this same authority between decisions, rather than a copied seed.
pub(crate) fn bind_ai_stream_in(units: &mut super::owner::Units, id:u32,stream:crate::ai::random::SharedRandom) {
    assert!(id<RANDOM_CAPACITY as u32,"native AI random binding");
    let at=id as usize*F3RandomState::WORDS;
    let mut row=F3RandomState::take(&units.state.as_mut().expect("prepared unit service").random_states[at..],&[]);
    row.state=stream.seed();row.present=true;
    { let state = units.state.as_mut().expect("prepared unit service"); row.put(&mut state.random_states[at..],&mut state.block.heap); }
    if units.state.as_mut().expect("prepared unit service").shared_random[id as usize].is_none() {units.state.as_mut().expect("prepared unit service").shared_bindings.push(id);}
    units.state.as_mut().expect("prepared unit service").shared_random[id as usize]=Some(stream);
}
/// useClassicRandom changes the arsenal's spread generators only. Combat's
/// chunks roll retains its gameplay stream, independently of bot attachment.
pub fn use_ai_stream_in(world_ctx: &mut crate::world::owner::World, key:&F3UnitKey,id:u32) {
    assert!(world_ctx.units.state.as_mut().expect("prepared unit service").shared_random.get(id as usize).is_some_and(Option::is_some),"prepared native AI random stream");
    let changed={let unit=key.validate_mut(&mut world_ctx.sim.store).expect("current AI weapon owner");
        let mut changed=false;
        for weapon in &mut unit.combat.weapons {changed|=!weapon.random;weapon.random=true;}changed};
    let bindings=&mut world_ctx.units.state.as_mut().expect("prepared unit service").random_bindings[key.slot as usize];
    for binding in bindings.iter_mut().skip(1) { *binding=id; }
    if changed {self::changed_in(world_ctx, key.reference());}
}

/// ClassicRandom.uniform (ai/random.js). The numeric binding keeps the shared
/// AI stream's draw order without an import for every spread or chunks draw.
pub(crate) fn random_draw_in(units: &mut super::owner::Units, unit: Ref<Unit>, weapon: Option<u32>) -> f64 { units.state.as_mut().expect("prepared unit service").random_draw(unit, weapon) }
pub(crate) fn empty_text_in(units: &super::owner::Units) -> crate::store::StrId { units.state.as_ref().expect("prepared unit service").empty_text }
pub(crate) fn take_melee_in(units: &mut super::owner::Units, _unit: Ref<Unit>) -> crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey> { units.state.as_mut().expect("prepared unit service").take_melee(_unit) }
pub(crate) fn put_melee_in(units: &mut super::owner::Units, _unit: Ref<Unit>, scratch: crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey>) { units.state.as_mut().expect("prepared unit service").put_melee(_unit, scratch) }
pub(crate) fn take_arc_in(units: &mut super::owner::Units, _unit: Ref<Unit>) -> Vec<crate::combat::live::ArcVictim> { units.state.as_mut().expect("prepared unit service").take_arc(_unit) }
pub(crate) fn put_arc_in(units: &mut super::owner::Units, _unit: Ref<Unit>, targets: Vec<crate::combat::live::ArcVictim>) { units.state.as_mut().expect("prepared unit service").put_arc(_unit, targets) }
pub(crate) fn take_ordnance_query_in(units: &mut super::owner::Units) -> Option<(usize, Vec<Ref<Unit>>)> { units.state.as_mut().expect("prepared unit service").take_ordnance_query() }
pub(crate) fn put_ordnance_query_in(units: &mut super::owner::Units, depth: usize, query: Vec<Ref<Unit>>) { units.state.as_mut().expect("prepared unit service").put_ordnance_query(depth, query) }
pub(crate) fn admit_ordnance_fire_in(units: &mut super::owner::Units) -> bool {
    units.state.as_mut().is_none_or(|state| {
        if state.ordnance_depth < state.ordnance_queries.len() { return true; }
        state.ordnance_refused = state.ordnance_refused.saturating_add(1);
        false
    })
}
pub(crate) fn random_gameplay_in(units: &mut super::owner::Units) -> f64 { units.state.as_mut().expect("prepared unit service").random_gameplay() }
pub(crate) fn random_spin_in(units: &mut super::owner::Units) -> f64 { units.state.as_mut().expect("prepared unit service").random_spin() }
pub(crate) fn random_drop_in(units: &mut super::owner::Units) -> f64 { units.state.as_mut().expect("prepared unit service").random_drop() }
// Vehicle.fireSeat and TurretSeats.drive share the seat-weapons channel.
// Its first draw keeps the same lazy insertion slot as the JS named stream.
pub(crate) fn seat_binding_in(units: &mut super::owner::Units) -> u32 { units.state.as_mut().expect("prepared unit service").seat_binding() }
// Mounted weapons can replace their default channel with the Classic AI's
// shared generator. The retained numeric id follows that specific weapon.
pub(crate) fn random_binding_in(units: &mut super::owner::Units, binding: u32) -> f64 { units.state.as_mut().expect("prepared unit service").random_binding(binding) }
/// Classic spawning uses raw modulo draws from the same generator as AI.
/// Preserve the lazy channel stamp before advancing either representation.
pub(crate) fn random_next_in(units: &mut super::owner::Units, stream: u32) -> u32 {
    if stream >= RANDOM_CAPACITY as u32 { fail_in(units); return 0; }
    units.state.as_mut().expect("prepared unit service").random_next(stream)
}

// Arsenal and recon publish the same dynamic clocks. Definitions and other
// immutable weapon fields stay in the construction table, never in this row.
pub(crate) fn weapon_view_in(combat: &mut crate::combat::module::CombatOwner, w:&Weapon) -> F3WeaponView {
    let defs=crate::combat::module::defs_in(combat);
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
pub(crate) fn wire_visible_in(units:&super::owner::Units,fleet:&crate::vehicles::module::Fleet, store:&Store, reference: Ref<Unit>, unit: &Unit) -> bool {
    let mut visible = !unit.removed && !(unit.dead && unit.combat.blow_apart)
        && !matches!(&unit.soldier, UnitBody::Droideka(d) if d.mode == 4.);
    if let Some(shown) = crate::vehicles::module::f2::live_seats::visible_in(fleet, store, unit) { visible &= shown; }
    if unit.dead && units.state.as_ref().expect("prepared unit service").hidden_crew.get(reference.slot as usize).copied().flatten() == Some((reference, unit.generation)) { visible = false; }
    visible
}

// Build the narrow publication from native values, never from a JS mirror.
// Immutable class/loadout data stays in the construction tables. Dynamic
// correction fields remain here because network capture reads their timers.
// Reuse the weapon capacity. Native notification latches and pending muzzle
// shots have no foreign reader, so publication never stages those lists.
#[derive(Clone, Copy)]
struct PublicationFlags { arsenal: u32, pooled: bool, hidden: bool, native: bool }
fn publication_flags(state: &State, reference: Ref<Unit>, unit: &Unit) -> PublicationFlags {
    PublicationFlags { arsenal: state.arsenals[reference.slot as usize], pooled: state.owner.pooled(reference),
        hidden: unit.dead && state.hidden_crew[reference.slot as usize] == Some((reference, unit.generation)),
        native: state.native_intents[reference.slot as usize] == Some(reference) }
}
fn stage_in(fleet:&crate::vehicles::module::Fleet,store:&Store,combat: &mut crate::combat::module::CombatOwner, flags: PublicationFlags, out: &mut PublicationSnapshot, reference: Ref<Unit>, unit: &Unit) {
    out.value.arsenal = flags.arsenal;
    stage_body_in(fleet,store,flags, &mut out.value,reference,unit);
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
    let defs=crate::combat::module::defs_in(combat);
    for (o, w) in v.weapons.iter_mut().zip(&c.weapons) {
        *o = weapon_view_with_defs(w,&defs.weapons);
    }
    stage_controls_in(flags, out,reference,unit);
}
// Unit.onMoved precedes combat. Its body and consumed movement controls can
// change while the arsenal and weapon heap retain their last consumed bits.
fn stage_body_in(fleet:&crate::vehicles::module::Fleet,store:&Store,flags: PublicationFlags, out:&mut F3Publication,reference:Ref<Unit>,unit:&Unit) {
    out.body = ownership::mirror(reference, unit);
    out.body.pooled = flags.pooled;
    // Native rider placement owns visibility as well as the shape sample.
    // A hull's missing rider node hides its occupant; destruction keeps the
    // corpse hidden even after the lease has been cleared before death.
    if let Some(visible) = crate::vehicles::module::f2::live_seats::visible_in(fleet,store,unit) { out.body.visible &= visible; }
    if flags.hidden { out.body.visible = false; }
}
fn stage_controls_in(flags: PublicationFlags, out:&mut PublicationSnapshot,_reference:Ref<Unit>,unit:&Unit) {
    // Native F3 consumes bot controls directly. Only the held fields that
    // JavaScript still reads leave this owner; input and latch rows remain
    // native. Foreign controls keep their complete restoration contract.
    let i=&unit.intent;
    if flags.native {
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
fn mutate_combat_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>, command: &F3Change, recon: Option<crate::units::f3::WorldServices<'_>>) {
    use crate::combat::{combat as cb, module as cm, world::DamageInfo};
    let defs = cm::defs_in(&mut world_ctx.combat);
    let (mut unit, mut combat) = cm::take_in(world_ctx, reference);
    if command.action == F3Action::Reset {
        cb::reset(&mut combat, &unit, &defs);
        arsenal_reset_in(&mut world_ctx.units, reference.slot);
        cm::put_in(world_ctx, reference, unit, combat);
        return;
    }
    let explosion = (command.action == F3Action::Explode).then(|| cm::blast_packet_in(&mut world_ctx.combat, command.index));
    let health_scale = world_ctx.units.state.as_ref().expect("prepared unit service").health[reference.slot as usize];
    let mut world = cm::JsWorld::with_services_in(world_ctx, reference, &unit, &combat,recon);
    match command.action {
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
            let explosion = explosion.expect("explosion command packet");
            let p = command.vector;
            cb::explode_prepared(
                &mut combat,
                &mut unit,
                &defs,
                &explosion,
                health_scale,
                &three::Vector3::new(p[0], p[1], p[2]),
                command.attacker,
                command.value,
                &mut world,
            );
        }
        _ => unreachable!(),
    }
    world.finish();
    cm::put_in(world_ctx, reference, unit, combat);
}

// UnitFactory.acquire pops the last free body of this class. A pooled corpse
// keeps its registry slot and Unit identity; only a miss builds a new body.
// Acquiring does not advance life, reset weapons or observe the shape cache.

pub fn f3_acquire_in(world_ctx: &mut crate::world::owner::World, class: u32) -> i32 {
    acquire_native_in(world_ctx, crate::store::StrId(class)).map_or(-1, |r| r.slot as i32)
}

/// Native Unit.sphere and hitShapes sensor service. Cache observation remains
/// lazy; publication and damage never ask it to rebuild.
// Units.query/raycast and Unit.sphere/hitShapes are explicit sensor reads.
// A shape request updates the cache only if tickCount invalidated it. Query
// output has separate storage so it cannot replace a pending body publication.
// Reuse result capacity even when a shorter query previously emptied the list.

/// UnitRegistry.raycast's selection mask follows registry ids, including
/// inactive members. Refuse an incomplete snapshot before replacing any bits.
pub fn f3_sensor_mask_in(world_ctx: &mut crate::world::owner::World, mask: &[bool]) -> bool {
    let Some(state) = world_ctx.units.state.as_mut() else { return false };
    if mask.len() != world_ctx.sim.store.units.list.len() || mask.len() > state.input.len() { return false; }
    for (word, &enabled) in state.input.iter_mut().zip(mask) { *word = if enabled { 1.0 } else { 0.0 }; }
    true
}
pub fn f3_sensor_in(world_ctx: &mut crate::world::owner::World) -> Option<&[f64]> {
    let input = F3Sensor::take(&world_ctx.units.state.as_mut().expect("prepared unit service").command, &[]);
    match input.mode {
        F3SensorMode::Bounds => {
            // Unit.sphere observes shapes at exactly the old updateSpheres
            // point. Only its bounding sphere crosses for an ordinary reader;
            // later hitShapes reads the already sampled canonical limb list.
            if input.key.validate(&world_ctx.sim.store).is_err() { fail_in(&mut world_ctx.units); return None; }
            module::with_unit_in(world_ctx,input.key.reference(),|world,unit| {
                unit.sphere(input.key.slot, world);
                cache_bounds_in(&mut world.w.units, input.key.reference(),unit);
                let view = F3BoundsView {
                    key: input.key, tick: unit.spheres_tick,
                    bounds: Bounds { center: unit.bounds.center, radius: unit.bounds.radius },
                };
                let s = world.w.units.state.as_mut().expect("prepared unit service");
                s.sensor_block.begin(1, F3BoundsView::WORDS);
                let (row, heap) = s.sensor_block.row(0);
                view.put(row, heap);
            }).expect("validated sensor unit");
        }
        F3SensorMode::Shapes => {
            if input.key.validate(&world_ctx.sim.store).is_err() { fail_in(&mut world_ctx.units); return None; }
            module::with_unit_in(world_ctx,input.key.reference(),|world,unit| {
                unit.hit_shapes(input.key.slot, world);
                let s = world.w.units.state.as_mut().expect("prepared unit service");
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
            }).expect("validated sensor unit");
        }
        F3SensorMode::Raycast => {
            // Game.los normally excludes its two endpoints with no custom
            // predicate. Canonical flags and these qualified refs need no
            // whole-registry acceptance mask from JavaScript.
            let mut excludes = [Member::Unit(Ref::new(0, 0)); 2];
            let mut count = 0;
            if let Some(r) = input.exclude0 { excludes[count] = Member::Unit(r); count += 1; }
            if let Some(r) = input.exclude1 { excludes[count] = Member::Unit(r); count += 1; }
            let hit = raycast_in(world_ctx,
                input.origin,
                input.direction,
                input.far,
                super::registry::Exclude::Many(&excludes[..count]),
                input.riders,
                input.masked,
            );
            let s = world_ctx.units.state.as_mut().expect("prepared unit service");
            s.sensor_result.members.clear();
            s.sensor_result.hit.found = hit.is_some();
            if let Some(h) = hit {
                s.sensor_result.hit.unit = Some(world_ctx.sim.store.unit.at(h.unit).unwrap().0);
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
            let mut members = std::mem::take(&mut world_ctx.units.state.as_mut().expect("prepared unit service").sensor_result.members);
            module::with_registry_in(world_ctx,|world,registry,slab| registry.query(
                slab,
                &super::shapes::v3(input.origin),
                input.radius,
                &mut members,
                None,
                world,
            ));
            let s = world_ctx.units.state.as_mut().expect("prepared unit service");
            s.sensor_result.members = members;
            s.sensor_result.hit.found = false;
            s.sensor_block.begin(1, F3SensorResult::WORDS);
            let (row, heap) = s.sensor_block.row(0);
            s.sensor_result.put(row, heap);
        }
    }
    let s = world_ctx.units.state.as_mut().expect("prepared unit service");
    s.sensor_block.finish();
    Some(&s.sensor_block.words[..])
}

// Units.raycast keeps the first hit on an exact distance tie. Its ordered
// traversal and shape observation run over the canonical registry and slab.
// The optional mask is a JS sensor predicate's result, not imported geometry;
// excluded candidates must not observe their lazy shapes as a side effect.
pub(crate) fn raycast_in(world_ctx: &mut crate::world::owner::World,
    origin: [f64; 3],
    direction: [f64; 3],
    far: f64,
    exclude: super::registry::Exclude,
    riders: bool,
    masked: bool,
) -> Option<super::registry::UnitHit> {
module::with_registry_in(world_ctx, |world, registry, slab| {
        registry.raycast_with(slab, &super::shapes::v3(origin), &super::shapes::v3(direction), far, exclude,
            &mut |m, _, world: &mut module::ModuleWorld<'_, '_>| !masked || world.w.units.state.as_ref().expect("prepared unit service").input.get(super::registry::target_id(m) as usize).is_some_and(|&word| word != 0.0),
            riders, world, |world| world)
    })
}

/// AnimatedProp.seize/carry over the canonical body and retained skeleton.
// Seizure freezes motion without invalidating a previously observed shape.
// Only a successful carry advances tickCount. The later shape reader samples
// limbs in this final frame, preserving Unit.updateSpheres' lazy observation.

pub fn f3_pose_in(world_ctx: &mut crate::world::owner::World) -> u32 {
    let mut input = F3PoseCommand::take(&world_ctx.units.state.as_mut().expect("prepared unit service").command, &[]);
    let reference = input.key.reference();
    if input.key.validate(&world_ctx.sim.store).is_err() { return 0; }
    if world_ctx.units.state.as_mut().expect("prepared unit service").running {return 0;}
    begin_events_in(world_ctx);
    let frame = match input.action {
        F3PoseAction::Seize => {
            let unit = world_ctx.sim.store.unit.get_mut(reference).unwrap();
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
            super::live_pose::clear_in(&mut world_ctx.units, reference);
            None
        }
        F3PoseAction::Carry => {
            let unit = world_ctx.sim.store.unit.get(reference).unwrap();
            if !matches!(&unit.held, crate::store::Nullable::Value(h)
                if h.prop == input.prop && h.since == input.since) { return 0; }
            let mut frame = three::Matrix4::default();
            if !super::live_pose::held_frame_in(world_ctx, &input, &mut frame) {return u32::from(!world_ctx.units.state.as_mut().expect("prepared unit service").failed);}
            input.frame = frame.elements;
            let unit = world_ctx.sim.store.unit.get_mut(reference).unwrap();
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
    }.emit(&mut world_ctx.sim.events);
    if frame.is_some() && !super::live_pose::set_in(&mut world_ctx.units, input) {return 0;}
    publish_in(world_ctx, reference);
    1
}

/// Compact foreign target facts; these rows do not contain unit bodies.
// Units.query also sees buildings and other foreign targets. Their compact
// changed facts contain no Unit bodies. Updating one target must not clear the
// others: a later query can still meet a target that did not change this tick.

pub fn f3_foreign_in(world_ctx: &mut crate::world::owner::World, count: u32) {
    foreign_rows_in(&mut world_ctx.units, 0,count);
    let header = F3ForeignHeader::take(&world_ctx.units.state.as_mut().expect("prepared unit service").foreign_header, &[]);
    building_rows_in(world_ctx, count as usize * F3PropFact::WORDS, header.buildings);
}
// LiveObjects changes only affected building rows. This span follows registry
// facts in the same input reservation, including an outside AI sensor query.
fn building_rows_in(world_ctx: &mut crate::world::owner::World, offset: usize, count: u32) {
    for i in 0..count as usize {
        let at = offset + i * F3BuildingState::WORDS;
        let frame = F3BuildingState::take(&world_ctx.units.state.as_mut().expect("prepared unit service").input[at..], &[]);
        super::f3_props::changed_in(world_ctx, frame);
    }
}
pub(crate) fn prepare_prop_target_in(units: &mut super::owner::Units, fact: F3PropFact) { units.state.as_mut().expect("prepared unit service").prepare_prop_target(fact) }
// A native prop mutation updates the existing registry fact without going back
// through JS geometry. Other targets keep their cached facts and insertion order.
pub(crate) fn prop_target_in(units: &mut super::owner::Units, member: Member, alive: bool, health: f64, center: [f64; 3], radius: f64, generation: Option<f64>, team: f64) { units.state.as_mut().expect("prepared unit service").prop_target(member, alive, health, center, radius, generation, team) }
fn foreign_rows_in(units: &mut super::owner::Units, offset: usize, count: u32) { units.state.as_mut().expect("prepared unit service").foreign_rows(offset, count) }
// Registry ids are shared by all member kinds. Indexing the prepared facts
// keeps a mixed query from looking up names or allocating per candidate.
pub(crate) fn target_in(world_ctx: &mut crate::world::owner::World, id: u32) -> Option<& F3PropFact> {
    super::f3_hulls::refresh_in(world_ctx, id);
    super::f3_props::refresh_in(world_ctx, id);
    prepared_target_in(&world_ctx.units, id)
}
/// Fixed prop facts require no mutable Store access. A phase holding Store
/// locally uses its owner directly for dynamic hull facts before this reader.
pub(crate) fn prepared_target_in(units: &super::owner::Units, id: u32) -> Option<& F3PropFact> {
    units.state.as_ref().expect("prepared unit service").foreign.get(id as usize).filter(|f| f.id == id)
}

// UnitFactory attachment may follow existing registry construction. History
// must reserve those initial members too, even before the first unit is built.

pub fn f3_registry_prepare_in(world_ctx: &mut crate::world::owner::World) {
    reserve_registry_in(world_ctx);
}

// Units.add reserves future pad births at construction too. Their target
// ids are assigned only at activation, after any intervening soldier births.
pub(crate) fn reserve_registry_in(world_ctx: &mut crate::world::owner::World) {
    let members = &mut world_ctx.sim.store.units;
    let count = members.list.len() + super::f3_hulls::prepared_count_in(&world_ctx.units);
    members.list.reserve(count.saturating_sub(members.list.len()));
    let ids = members.next_id as usize + super::f3_hulls::prepared_count_in(&world_ctx.units);
    { let state = world_ctx.units.state.as_mut().expect("prepared unit service"); state.foreign.resize_with(state.foreign.len().max(ids), F3PropFact::default); }
    super::f3_hulls::reserve_members_in(&mut world_ctx.units, ids);
    crate::vehicles::module::f2::reserve_registry_in(world_ctx, count);
    reserve_history_in(world_ctx);
}

/// Units.add gives every kind a registry id in one canonical sequence.
// Units.add allocates one id sequence for soldiers, hull targets, turrets
// and objectives. Keep append order: capture counts and equal-distance hits
// depend on it. Freeing a pooled body does not remove its registry membership.

pub fn f3_registry_add_in(world_ctx: &mut crate::world::owner::World, kind: u32, generation: u32) -> u32 {
    let id = world_ctx.sim.store.units.add(|id| match kind as u16 {
        KIND_UNIT => Member::Unit(Ref::new(id, generation)),
        KIND_VEHICLE_TARGET => Member::VehicleTarget(Ref::new(id, generation)),
        KIND_TURRET_TARGET => Member::TurretTarget(Ref::new(id, generation)),
        KIND_OBJECTIVE_TARGET => Member::ObjectiveTarget(Ref::new(id, generation)),
        _ => unreachable!("a registry member kind"),
    });
    reserve_registry_in(world_ctx);
    id
}
// Units.remove erases the live list entry while preserving every survivor's
// order. This is distinct from Unit.remove, which returns a body to its pool.

pub fn f3_registry_remove_in(sim: &mut crate::abi::Sim, id: u32) {
    if let Some(m) = sim.store.units.by_id(id) {
        sim.store.units.remove(m);
    }
}

/// CollisionWorld.rayBodies names each body once when membership changes.
// CollisionWorld.rayBodies assigns stable body references when membership
// changes. Keep the generation as well as the kernel slot: ordnance callbacks
// must identify the exact body, including after a pooled slot is reused.

pub fn f3_body_names_in(world_ctx: &mut crate::world::owner::World, count: u32) {
    let count=(count as usize).min(world_ctx.units.state.as_mut().expect("prepared unit service").input.len()/F3BodyName::WORDS);
    for i in 0..count {
        let at=i*F3BodyName::WORDS;
        let row=F3BodyName::take(&world_ctx.units.state.as_mut().expect("prepared unit service").input[at..],&[]);
        if row.kernel as usize >= world_ctx.collision.state.bodies.len() {continue;}
        if let Some(collision)=crate::vehicles::module::collision_world_in(world_ctx) {
            collision.bind_body(row.body,row.kernel);
        }
    }
    // Body membership is a construction boundary. Build the cast index here,
    // rather than charging the first weapon's ray with index allocations.
    crate::collision::broad::prepare(&mut world_ctx.collision.state,world_ctx.units.state.as_mut().expect("prepared unit service").world);
    world_ctx.movement.prepare_world_gathers(&world_ctx.collision.state, world_ctx.units.state.as_mut().expect("prepared unit service").world);
}
// A native F1 cast returns the body that was named with the collision tree.
// It must never manufacture generation zero from the kernel's numeric slot.
pub(crate) fn body_reference_in(_world_ctx: &mut crate::world::owner::World, kernel: usize) -> Option<Ref<Body>> {
    crate::vehicles::module::collision_world_in(_world_ctx)?.kernel_reference(kernel)
}

// Weapon-targeting's bot override is a phase fact from JS AI. Missing and null
// remain distinct: humans acquire through their cone, a bot with no target does not.
pub(crate) fn bot_target_in(units: &super::owner::Units, slot: u32) -> Option<Option<Member>> {
    units.state.as_ref().expect("prepared unit service").bot_targets[slot as usize]
}

// Ordnance.flightMissile stores a class name. Resolve the four immutable
// names once; guidance subsequently indexes this table without decoding text.

pub fn f3_missile_classes_in(world_ctx: &mut crate::world::owner::World, count: u32) {
    let (rows, _) = world_ctx.sim.calls.args();
    for index in 0..count as usize {
        let row = F3MissileClass::take(&rows[index * F3MissileClass::WORDS..],&[]);
        let at = row.name.0 as usize;
        if world_ctx.units.state.as_mut().expect("prepared unit service").missile_classes.len() <= at { world_ctx.units.state.as_mut().expect("prepared unit service").missile_classes.resize(at + 1,None); }
        world_ctx.units.state.as_mut().expect("prepared unit service").missile_classes[at] = Some(row.class);
    }
}
pub(crate) fn missile_class_in(units: &super::owner::Units, name: Option<crate::store::StrId>) -> FlightClass {
    name.map_or(FlightClass::Fighter,|name| {
        units.state.as_ref().expect("prepared unit service").missile_classes[name.0 as usize].expect("prepared missile class")
    })
}
pub(crate) fn category_in(units: &super::owner::Units, slot: u32) -> u32 {
    units.state.as_ref().expect("prepared unit service").categories[slot as usize]
}

// Units.setRelation and mission team tables arrive when the table changes.
// Sensor reads compare numeric pairs; they never format or intern "a:b" keys.

pub fn f3_relations_in(world_ctx: &mut crate::world::owner::World, count:u32) -> u32 {
    let side=count.isqrt() as usize;
    if side*side!=count as usize {return 0;}
    let (rows,_)=world_ctx.sim.calls.args();
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
s.relation_sides=side;s.relations.resize(side*side,-1.);
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
pub(crate) fn relation_in(units: &super::owner::Units, a:f64,b:f64) -> f64 {
    if a==b {return if a!=0. && !a.is_nan() {1.} else {0.};}
    if a==0. || b==0. || a.is_nan() || b.is_nan() {return 0.;}
    let s=units.state.as_ref().expect("prepared unit service");let ai=a as usize;let bi=b as usize;
    if ai>=s.relation_sides || bi>=s.relation_sides || ai as f64!=a || bi as f64!=b {return -1.;}
    s.relations[ai*s.relation_sides+bi]
}

// HitHistory.capture belongs at the multiplayer world's end-of-tick capture
// boundary, after ordnance and items. Capturing never imports body rows.

/// The hit history's capture at the start of a match's tick
/// (multiplayer-world.js hitHistory.capture): the host's `units` intent
/// rows and its network tail precede the foreign facts it wrote.
pub(crate) fn f3_history_capture_at_in(world_ctx: &mut crate::world::owner::World, tick: u32, units: u32) {
    let allocations = crate::allocations::snapshot();
    crate::allocations::begin();
    // F4 may have advanced foreign building life after the unit span. Those
    // changed rows follow the intent rows the host prepared for this tick.
    let foreign = F3ForeignHeader::take(&world_ctx.units.state.as_mut().expect("prepared unit service").foreign_header, &[]);
    let offset = (units as usize).min(INPUT_WORDS / F3PhaseInput::WORDS) * F3PhaseInput::WORDS
        + (foreign.network as usize).min(INPUT_WORDS / F3PhaseNetwork::WORDS) * F3PhaseNetwork::WORDS;
    let count = if offset + foreign.count as usize * F3PropFact::WORDS + foreign.buildings as usize * F3BuildingState::WORDS > INPUT_WORDS { 0 } else { foreign.count };
    let buildings = if count == foreign.count { foreign.buildings } else { 0 };
    foreign_rows_in(&mut world_ctx.units, offset, count);
    building_rows_in(world_ctx, offset + count as usize * F3PropFact::WORDS, buildings);
    let mut history = world_ctx.units.state.as_mut().expect("prepared unit service").history.take().expect("available hit history");
    history.capture(tick, &mut super::f3_sensors::Sensors { w: world_ctx });
    world_ctx.units.state.as_mut().expect("prepared unit service").history = Some(history);
    finish_allocations_in(&mut world_ctx.units, allocations);
    descriptor_in(world_ctx);
}
pub(crate) fn history_in(units: &mut super::owner::Units) -> &mut crate::combat::history::History<Member> {
    units.state.as_mut().expect("prepared unit service").history.as_mut().expect("available hit history")
}

// networkShotFor's fixed channels are input facts, not body mirrors. An edge
// view overrides the current rendered tick only for that particular press.
pub(crate) fn network_in(units: &super::owner::Units, slot: u32) -> & F3NetworkView {
    &units.state.as_ref().expect("prepared unit service").network[slot as usize]
}

// OrdnanceSim.send resolves complete damage fields once. F1 keeps its flight
// definition numbering; this table adds policy without duplicating flight rows.

pub fn f3_hit_policy_in(world_ctx: &mut crate::world::owner::World) {
    let (row, _) = world_ctx.sim.calls.args();
    let input = F3HitPolicy::take(row, &[]);
    let id = input.ordnance.0 as usize;
    let size = world_ctx.units.state.as_mut().expect("prepared unit service").hit_policies.len().max(id + 1);
    world_ctx.units.state.as_mut().expect("prepared unit service").hit_policies.resize(size, None);
    world_ctx.units.state.as_mut().expect("prepared unit service").hit_policies[id] = Some(crate::combat::impacts::HitPacket {
        damage: input.damage,
        scales: [input.person, input.animal, input.droid, input.vehicle, input.building],
        shield: input.shield,
    });
}

pub fn f3_blast_binding_in(world_ctx: &mut crate::world::owner::World) {
    let (row, _) = world_ctx.sim.calls.args();
    let input = F3BlastBinding::take(row, &[]);
    let size = world_ctx.units.state.as_mut().expect("prepared unit service").blasts.len().max(input.explosion as usize + 1);
    world_ctx.units.state.as_mut().expect("prepared unit service").blasts.resize(size, None);
    world_ctx.units.state.as_mut().expect("prepared unit service").blasts[input.explosion as usize] = Some(input.combat);
}

pub fn f3_impact_rules_in(world_ctx: &mut crate::world::owner::World) {
    let (row, _) = world_ctx.sim.calls.args();
    let rules = F3ImpactRules::take(row, &[]);
    world_ctx.units.state.as_mut().expect("prepared unit service").friendly_fire = rules.friendly_fire;
}
/// Ordnance.explosion outside the owned span is one world command, not a
/// JavaScript loop issuing one damage command for every registry member.

pub fn f3_world_blast_in(world_ctx: &mut crate::world::owner::World) {
    let (row,heap) = world_ctx.sim.calls.args();
    let input = F3BlastCall::take(row,heap);
    let boundary = external_in(world_ctx);
    let result = (|| {
    with_services_in(world_ctx, |world_ctx, recon| crate::combat::live::blast_in(world_ctx, crate::store::DefId(input.explosion),input.position,input.owner,recon));

    })();
    boundary.finish(world_ctx, true);
    result
}
pub(crate) fn blast_packet_in(world_ctx: &mut crate::world::owner::World, def: crate::store::DefId) -> crate::combat::impacts::BlastPacket {
    let combat = world_ctx.units.state.as_mut().expect("prepared unit service").blasts[def.0 as usize].expect("prepared explosion damage");
    crate::combat::module::blast_packet_in(&mut world_ctx.combat, combat)
}
pub(crate) fn hit_policy_in(units: &super::owner::Units, def: crate::store::DefId) -> crate::combat::impacts::HitPacket {
    units.state.as_ref().expect("prepared unit service").hit_policies[def.0 as usize].expect("prepared hit policy")
}
pub(crate) fn health_scale_in(units: &super::owner::Units, slot: u32) -> crate::combat::impacts::HealthScale {
    units.state.as_ref().expect("prepared unit service").health[slot as usize]
}
pub(crate) fn friendly_fire_in(units: &super::owner::Units) -> bool {
    units.state.as_ref().expect("prepared unit service").friendly_fire
}
// AI commands mutate only Intent; they do not invalidate lazy geometry.
// Keep a separate touched list so a concurrent lifecycle/weapon/body write
// wins the full publication without being mislabeled as an input-only edit.
pub(crate) fn intent_changed_in(units: &mut super::owner::Units, reference:Ref<Unit>) { units.state.as_mut().expect("prepared unit service").intent_changed(reference) }
// AI navigation avoidance and prepareSpread keep their source writes. Only
// encoding waits for the following unit phase; synchronous barriers flush it.
pub(crate) fn ai_changed_in(units: &mut super::owner::Units, reference:Ref<Unit>) { units.state.as_mut().expect("prepared unit service").ai_changed(reference) }

pub(crate) fn changed_in(world_ctx: &mut crate::world::owner::World, reference: Ref<Unit>) { changed_parts(&mut world_ctx.units,&world_ctx.sim.store,reference); }
pub(crate) fn changed_parts(units:&mut super::owner::Units,store:&Store, reference:Ref<Unit>) {
    // Seat placement can advance tickCount outside Unit.tick. Its qualified
    // touch must retire an earlier shared bounds sample before JS observes it.
    if let Some(unit)=store.unit.get(reference) {
        let at=reference.slot as usize*F3BoundsView::WORDS;
        if units.state.as_mut().expect("prepared unit service").bounds_cache.get(at+F3BoundsView::TICK).is_some_and(|tick|*tick!=unit.tick_count) {
            invalidate_bounds_in(units, reference.slot);
        }
    }
    units.state.as_mut().expect("prepared unit service").owner.mark(reference);
}

// Pure shot, damage and presentation notices use Sim.events, in source order.
// Each consumer drains only its new prefix; resetting a nested aimer command
// would discard events the surrounding native phase has not yet published.
fn begin_events_in(world_ctx: &mut crate::world::owner::World) {
    acknowledge_publication_in(&mut world_ctx.units);
    world_ctx.units.state.as_mut().expect("prepared unit service").failed=false;
    // A notification can issue an outside-phase command. Preserve the unread
    // journal suffix until its consumer acknowledges it; the command appends
    // behind that suffix instead of erasing another victim's notification.
    let ack=F3Acknowledgement::take(&world_ctx.units.state.as_mut().expect("prepared unit service").acknowledgement,&[]);
    if world_ctx.sim.events.is_empty() || (ack.event_epoch==world_ctx.units.state.as_mut().expect("prepared unit service").event_epoch
        && ack.event_offset as usize==world_ctx.sim.events.len()) {
        world_ctx.sim.events.clear();
        world_ctx.units.state.as_mut().expect("prepared unit service").event_epoch=world_ctx.units.state.as_mut().expect("prepared unit service").event_epoch.wrapping_add(1);
    }
    let s = world_ctx.units.state.as_mut().expect("prepared unit service");
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
pub(super) fn with_rows_in<T>(store: &mut Store, state: &mut State, run: impl FnOnce(&mut Store,&mut Ownership)->T) -> T {
    run(store, &mut state.owner)
}
/// F4 acquires warm bodies without copying any Unit, body or combat row.
pub(crate) fn acquire_native_in(world_ctx: &mut crate::world::owner::World, class: crate::store::StrId) -> Option<Ref<Unit>> {
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.acquire(&world_ctx.sim.store,class).map(|key|key.reference())
}
/// A cold miss activates an already prepared row through the same owner.
pub(crate) fn construct_native_in(world_ctx: &mut crate::world::owner::World, class: crate::store::StrId, services: WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::cold_in(world_ctx, class,services)
}
/// Player and connection realization is independent of the subsequent spawn.
pub(crate) fn realize_native_in(world_ctx: &mut crate::world::owner::World, request:&F3PlayerRealization, services:WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::realize_in(world_ctx, request,services)
}
/// F4's player factory supplies canonical Character/body handles. Admission
/// has already registered stream and recon identities outside the phase.
pub(crate) fn realize_player_native_in(world_ctx: &mut crate::world::owner::World, character:Ref<Character>, previous:Option<Ref<Unit>>,
    class:crate::store::StrId, at:[f64;3], yaw:f64, local:bool, services:WorldServices<'_>) -> Option<Ref<Unit>> {
    super::f3_construction::player_in(world_ctx, character,previous,class,at,yaw,local,services)
}
/// Character spawning binds metadata before Unit.spawn resets its loadout.
pub(crate) fn spawn_native_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, character: Ref<Character>, at:[f64;3], yaw:f64, mut services:WorldServices<'_>) {
    let control_guard=control_span_in(world_ctx, true);
    let result = (|| {
    let c=world_ctx.sim.store.character.get(character).expect("spawning Character");
    let (team,hero)=(c.team,c.hero);
    let u=world_ctx.sim.store.unit.get_mut(unit).expect("acquired native body");
    u.team=team;u.combat.team=team;u.character=Some(character);u.controller=None;
    module::lifecycle_owned_in(world_ctx, unit.slot,Some((at,yaw)),Some(services.reborrow()));
    world_ctx.sim.store.unit.get_mut(unit).unwrap().hero=hero==Some(true);
    crate::recon::module::spawned_in(world_ctx, unit,services.recon);changed_in(world_ctx, unit);

    })();
    control_guard.finish(world_ctx);
    result
}
/// Removal finishes Character/seat/body lifecycle before pooling the object.
pub(crate) fn remove_native_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>, services:WorldServices<'_>) {
    module::lifecycle_owned_in(world_ctx, reference.slot,None,Some(services));
    let unit=world_ctx.sim.store.unit.get(reference).expect("removed native Unit");
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.removed(&world_ctx.sim.store,&F3UnitKey::of(reference,unit)).expect("current removed Unit");
}
/// HitHistory can observe current registry members plus every cold body and
/// hull still awaiting activation. Registry ids also leave room for unrelated
/// future slots; allocating ten shapes per id in every frame wastes that room.
/// Call only during preparation, never when a prepared body becomes live.
pub(crate) fn reserve_history_in(world_ctx: &mut crate::world::owner::World) {
    let count=world_ctx.sim.store.units.list.len()+super::f3_construction::prepared_count_in(&world_ctx.units)+
        super::f3_hulls::prepared_count_in(&world_ctx.units);
    world_ctx.units.state.as_mut().expect("prepared unit service").history.as_mut().expect("prepared hit history").reserve(count,Member::Unit(Ref::new(0,0)),10);
}

// Unit.sphere stays lazy. History has often observed it before JS asks;
// retain that exact observation in the existing generated bounds layout.
// A native tick invalidates this mirror without evaluating canonical shapes.
fn prepare_bounds_in(world_ctx: &mut crate::world::owner::World, count:usize) {
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");

    let words=count*F3BoundsView::WORDS;
    if s.bounds_cache.len()<words {s.bounds_cache.resize(words,f64::NAN);}
    #[cfg(target_arch = "wasm32")]
    s.bind_bounds();
    s.layout.bounds_capacity=(s.bounds_cache.len()/F3BoundsView::WORDS) as u32;
    descriptor_in(world_ctx);
}
pub(crate) fn invalidate_bounds_in(units: &mut super::owner::Units, slot:u32) { units.state.as_mut().expect("prepared unit service").invalidate_bounds(slot) }
pub(crate) fn cache_bounds_in(units: &mut super::owner::Units, reference:Ref<Unit>,unit:&Unit) { units.state.as_mut().expect("prepared unit service").cache_bounds(reference, unit) }

/// Cold storage reserves empty sidecar slots; each prepared body carries its
/// own allocated query/rig buffers into the destination at activation.
pub(crate) fn reserve_native_slots_in(world_ctx: &mut crate::world::owner::World, count:usize) {
    let s=world_ctx.units.state.as_mut().expect("prepared unit service");
let count=count.max(s.categories.len());
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
    super::f3_hulls::reserve_members_in(&mut world_ctx.units, count);
    // Prepared future births may become flight targets without a JS registry add.
    crate::vehicles::module::f2::reserve_registry_in(world_ctx, count);
    prepare_bounds_in(world_ctx, count);
}
pub(crate) fn take_bindings_in(units: &mut super::owner::Units, slot:u32)->Vec<u32> { units.state.as_mut().expect("prepared unit service").take_bindings(slot) }
pub(crate) fn install_native_slot_in(units: &mut super::owner::Units, reference:Ref<Unit>, category:u32,
    health:crate::combat::impacts::HealthScale,bindings:Vec<u32>,
    water:Option<DroidekaWater>,mut publication:crate::vehicles::f2_delta::Cache) {
    let slot=reference.slot as usize;
    invalidate_bounds_in(units, reference.slot);
    let s=units.state.as_mut().expect("prepared unit service");
    publication.invalidate();s.publication[slot]=publication;
    s.categories[slot]=category;s.health[slot]=health;s.random_bindings[slot]=bindings;
    s.infos[slot].unit=slot as f64;s.infos[slot].droideka_water=water;
    s.hidden_crew[slot]=None;s.bot_targets[slot]=None;s.native_intents[slot]=None;s.network[slot]=F3NetworkView::default();
    s.arsenals[slot]=s.arsenals[slot].wrapping_add(1);
}
pub(crate) fn discard_publication_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>) {
    world_ctx.units.state.as_mut().expect("prepared unit service").owner.unmark(reference);
    publish_changed_in(world_ctx);
}
/// The native lifecycle supplies numeric stream bindings. Mirror consumers
/// subsequently attach the already registered callbacks without setter calls.
pub(crate) fn bind_native_life_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>, metadata:&F3Metadata, combat:u32, weapon:u32) {
    self::metadata_in(world_ctx, reference,metadata);
    let unit=world_ctx.sim.store.unit.get_mut(reference).unwrap();
    unit.combat.random=combat!=NO_STREAM;
    let count=unit.combat.weapons.len();
    let row=&mut world_ctx.units.state.as_mut().expect("prepared unit service").random_bindings[reference.slot as usize];
    assert!(row.len()>count,"prepared weapon stream slots");
    row[0]=combat;
    for index in 0..count {row[index+1]=weapon;unit.combat.weapons[index].random=weapon!=NO_STREAM;}
    F3LifeStreams {key:F3UnitKey::of(reference,unit),combat,weapon,
        metadata:F3Metadata {character:unit.character,controller:unit.controller,
            team:unit.team,life:unit.generation,human:unit.human,
            network_human:unit.network_human,hero:unit.hero}}.emit(&mut world_ctx.sim.events);
    changed_in(world_ctx, reference);
}

/// MultiplayerWorld.unspawn keeps the admitted body between lives without
/// a death, ticket, corpse or new generation. Character ownership survives.
pub(crate) fn unspawn_native_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>) {
    let u=world_ctx.sim.store.unit.get_mut(reference).expect("admitted connection body");
    u.alive=false;u.dead=true;u.combat.dead=true;u.removed=true;u.targetable=Some(false);
    u.dead_time=4.;u.combat.death_time=0.;
    match &mut u.soldier {UnitBody::Soldier(s)=>s.dead=Some(true),UnitBody::Droideka(d)=>d.dead=true}
    changed_in(world_ctx, reference);
}
/// Removed bodies are absent from Unit.tick, so their connection advances
/// this one clock before the next network spawn decision.
pub(crate) fn advance_removed_clock_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>,dt:f64) {
    let u=world_ctx.sim.store.unit.get_mut(reference).expect("current connection clock");
    if !u.alive && u.removed && u.dead_time<4. {u.dead_time=v8math::min(4.,u.dead_time+dt);changed_in(world_ctx, reference);}
}

/// Inactive class storage retains its publication buffers as well as its rig.
/// Activation moves these buffers into a reserved slot without allocating.
pub(crate) fn take_publication_in(units: &mut super::owner::Units, reference:Ref<Unit>) -> crate::vehicles::f2_delta::Cache { units.state.as_mut().expect("prepared unit service").take_publication(reference) }

/// UnitController.bind/unbind owns the right to publish held intent fields.
/// External human commands keep their pending JS input until Unit.tick takes
/// it; native bot commands expose their canonical intent at the outer return.
pub(crate) fn native_intent_in(world_ctx: &mut crate::world::owner::World, reference:Ref<Unit>,owned:bool) {
    world_ctx.units.state.as_mut().expect("prepared unit service").native_intents[reference.slot as usize]=owned.then_some(reference);
    changed_in(world_ctx, reference);
}

// Empty native services may return without opening another event epoch only
// after every changed body and notification has reached its foreign reader.
pub(crate) fn settled_in(world_ctx: &mut crate::world::owner::World)->bool {
    let deferred=crate::world::deferred_in(&world_ctx.tick);
    world_ctx.units.state.as_mut().expect("prepared unit service").settled(deferred,&world_ctx.sim.events)
}
/// Encoded outer payload size, without another export or body traversal.
pub(crate) fn output_words_in(units: &super::owner::Units)->u32 {units.state.as_ref().expect("prepared unit service").layout.output_words}
pub(crate) fn publication_version_in(units: &super::owner::Units)->u32 {units.state.as_ref().expect("prepared unit service").layout.version}

// Game.onExplode and recon share these detached, retained query resources.
// A service callback receives the loan, never a mutable enclosing Units owner.
struct ServiceLoan { recon: crate::recon::Authority, queries: crate::battle::live::Queries }
impl State {
    fn settled(&self, deferred: bool, events: &crate::store::calls::Events) -> bool {
        // The enclosing world owns canonical rows and defers facade bytes;
        // no extra danger loan is needed after the active service returns.
        if deferred { return !self.running; }
        !self.running && self.owner.changed().is_empty() && self.completed.is_empty() && self.intent_changed.is_empty()
            && self.acknowledgement[F3Acknowledgement::VERSION] as u32==self.layout.version
            && (events.is_empty() || (self.acknowledgement[F3Acknowledgement::EVENT_EPOCH] as u32==self.event_epoch
                && self.acknowledgement[F3Acknowledgement::EVENT_OFFSET] as usize==events.len()))
    }
    fn lend_services(&mut self) -> Option<ServiceLoan> {
        // Reject the same nested acquisition as the original with_services;
        // changing this to a whole-phase RefCell borrow would trap callbacks.
        if self.recon_borrowed { self.failed = true; self.layout.accepted = false; return None; }
        let Some(recon) = self.recon.take() else {
            self.failed = true; self.layout.accepted = false; return None;
        };
        self.recon_borrowed = true;
        Some(ServiceLoan { recon, queries: std::mem::take(&mut self.prop_queries) })
    }
    fn return_services(&mut self, loan: ServiceLoan) {
        self.recon = Some(loan.recon); self.prop_queries = loan.queries;
        self.recon_borrowed = false;
    }
}

impl State {
    fn take_melee(&mut self, _unit: Ref<Unit>) -> crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey> {
        std::mem::take(&mut self.melee)
    }
    fn put_melee(&mut self, _unit: Ref<Unit>, scratch: crate::combat::targeting::MeleeScratch<super::f3_sensors::MeleeKey>) {
        self.melee = scratch;
    }
    fn take_arc(&mut self, _unit: Ref<Unit>) -> Vec<crate::combat::live::ArcVictim> {
        std::mem::take(&mut self.arc)
    }
    fn put_arc(&mut self, _unit: Ref<Unit>, targets: Vec<crate::combat::live::ArcVictim>) {
        self.arc = targets;
    }
    fn take_ordnance_query(&mut self) -> Option<(usize, Vec<Ref<Unit>>)> {
        let depth = self.ordnance_depth;
        let Some(query) = self.ordnance_queries.get_mut(depth) else {
            self.ordnance_refused = self.ordnance_refused.saturating_add(1);
            return None;
        };
        self.ordnance_depth += 1;
        Some((depth, std::mem::take(query)))
    }
    fn put_ordnance_query(&mut self, depth: usize, mut query: Vec<Ref<Unit>>) {
        // A refused query never enters the stack. Reject an invalid return
        // without replacing another frame's retained storage or panicking.
        if depth.checked_add(1) != Some(self.ordnance_depth) {
            self.ordnance_refused = self.ordnance_refused.saturating_add(1);
            return;
        }
        let Some(slot) = self.ordnance_queries.get_mut(depth) else {
            self.ordnance_refused = self.ordnance_refused.saturating_add(1);
            return;
        };
        query.clear();
        self.ordnance_depth = depth;
        *slot = query;
    }
    fn random_gameplay(&mut self) -> f64 {
        let globals = F3RandomGlobals::take(&self.random_globals, &[]);
        self.draw_stream(globals.gameplay)
    }
    fn random_spin(&mut self) -> f64 {
        let globals = F3RandomGlobals::take(&self.random_globals, &[]);
        self.draw_stream(globals.spin)
    }
    fn random_drop(&mut self) -> f64 {
        let globals = F3RandomGlobals::take(&self.random_globals, &[]);
        self.draw_stream(globals.drop)
    }
    fn seat_binding(&mut self) -> u32 {
        F3RandomGlobals::take(&self.random_globals, &[]).seat
    }
    fn random_binding(&mut self, binding: u32) -> f64 { self.draw_stream(binding) }
    fn draw_stream(&mut self, stream: u32) -> f64 { (self.random_next(stream) >> 9) as f64 / 8388608.0 }
    fn random_next(&mut self, stream: u32) -> u32 {
        if stream>=RANDOM_CAPACITY as u32 {self.failed=true;self.layout.accepted=false;return 0;}
        // The epoch covers both fractional weapon draws and raw spawn draws.
        let epoch=&mut self.random_epoch[F3RandomEpoch::REVISION];
        *epoch=(*epoch as u32).wrapping_add(1) as f64;
        let at = stream as usize * F3RandomState::WORDS;
        let mut row = F3RandomState::take(&self.random_states[at..], &[]);
        if !row.present {
            row.present = true;
            self.random_sequence += 1;
            row.sequence = self.random_sequence;
        }
        let value=if let Some(mut shared)=self.shared_random[stream as usize].clone() {
            let value=crate::ai::random::Random::next(&mut shared);row.state=shared.seed();value
        }else{
            let mut generator=jsrt::rng::ClassicRandom::from_state(row.state);
            let value=generator.next_u32();row.state=generator.state;value
        };
        row.put(&mut self.random_states[at..], &mut self.block.heap);
        value
    }
    fn random_draw(&mut self, unit: Ref<Unit>, weapon: Option<u32>) -> f64 {
        let index = weapon.map_or(0, |i| i as usize + 1);
        let stream = self.random_bindings[unit.slot as usize][index];
        self.draw_stream(stream)
    }
    fn take_bindings(&mut self, slot:u32)->Vec<u32> {
        std::mem::take(&mut self.random_bindings[slot as usize])
    }
    fn take_publication(&mut self, reference:Ref<Unit>) -> crate::vehicles::f2_delta::Cache {
        std::mem::replace(&mut self.publication[reference.slot as usize],
            crate::vehicles::f2_delta::Cache::new(0))
    }
    fn prepare_prop_target(&mut self, fact: F3PropFact) {
        let id = fact.id as usize;
        self.foreign.resize_with(self.foreign.len().max(id + 1), F3PropFact::default);
        self.foreign[id] = fact;
    }
    fn prop_target(&mut self, member: Member, alive: bool, health: f64, center: [f64; 3], radius: f64, generation: Option<f64>, team: f64) {
        let id = super::registry::target_id(member) as usize;
        if let Some(row) = self.foreign.get_mut(id).filter(|row| row.id as usize == id) {
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
    fn foreign_rows(&mut self, offset: usize, count: u32) {
        let s = self;
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
    fn intent_changed(&mut self, reference:Ref<Unit>) {
        // Keep first-touch order while rejecting duplicates in constant time.
        // A retired storage generation cannot suppress its replacement's touch.
        let s=self;let stamp=&mut s.intent_marked[reference.slot as usize];
        if *stamp==Some(reference.gen) {return;}
        *stamp=Some(reference.gen);s.intent_changed.push(reference);
    }
    fn ai_changed(&mut self, reference:Ref<Unit>) {
        self.ai_full_marked[reference.slot as usize]=Some(reference.gen);
        self.intent_changed(reference);
    }
    fn invalidate_bounds(&mut self, slot:u32) {
        if let Some(tick)=self.bounds_cache.get_mut(slot as usize*F3BoundsView::WORDS+F3BoundsView::TICK) {
            *tick=f64::NAN;
        }
    }
    fn cache_bounds(&mut self, reference:Ref<Unit>,unit:&Unit) {
        let start=reference.slot as usize*F3BoundsView::WORDS;
        let Some(row)=self.bounds_cache.get_mut(start..start+F3BoundsView::WORDS) else {return;};
        let view=F3BoundsView {key:F3UnitKey::of(reference,unit),tick:unit.spheres_tick,
            bounds:Bounds {center:unit.bounds.center,radius:unit.bounds.radius}};
        // This fixed record has no heap payload, so the empty heap never allocates.
        view.put(row,&mut Vec::new());
    }
}

/// Cold AI content completes the player's canonical targeting services too.
/// Keep clocks, channels and ordnance bindings from their original adoption.
pub(crate) fn refresh_weapon_masks_in(world_ctx: &mut crate::world::owner::World, table:&crate::ai::combat::weapons_ai::WeaponAiTable) {
    let definitions=&crate::combat::module::defs_in(&mut world_ctx.combat).weapons;
    for (index,services) in world_ctx.units.state.as_mut().expect("prepared unit service").services.iter_mut().enumerate() {
        let Some(class)=definitions.classes.get(index).and_then(Option::as_ref) else {continue};
        for (service,level) in services.iter_mut().zip(&class.levels) {
            let ai=table.weapon_ai(&level.obj);
            let service = std::rc::Rc::get_mut(service).expect("target masks precede physical callbacks");
            service.lock.relation_mask=ai.relation_mask;service.lock.category_mask=ai.category_mask;
        }
    }
}

pub(crate) fn with_history_in<T>(w: &mut crate::world::owner::World, run: impl FnOnce(&mut crate::world::owner::World, &mut crate::combat::history::History<Member>) -> T) -> T {
    let mut history = w.units.state.as_mut().expect("prepared unit service").history.take().expect("available hit history");
    let result = run(w, &mut history);
    w.units.state.as_mut().expect("prepared unit service").history = Some(history);
    result
}

// Read-only dispenser questions do not initialize or mutate the item owner.
pub(crate) fn items_read(units: &super::owner::Units) -> &super::f3_items::Items { &units.state.as_ref().expect("prepared unit service").items }


// Native readers borrow these banks directly. Only the linear-memory host
// binds addresses after the bank has reached its retained publication point.
#[cfg(target_arch = "wasm32")]
impl State {
    fn bind_inputs(&mut self) {
        self.layout.input = self.input.as_mut_ptr() as u32;
        self.layout.random_states = self.random_states.as_mut_ptr() as u32;
        self.layout.aim_input = self.aim_input.as_mut_ptr() as u32;
        self.layout.command = self.command.as_mut_ptr() as u32;
        self.layout.acknowledgement = self.acknowledgement.as_mut_ptr() as u32;
        self.layout.random_epoch = self.random_epoch.as_mut_ptr() as u32;
        self.layout.random_globals = self.random_globals.as_mut_ptr() as u32;
        self.layout.aim_header = self.aim_header.as_mut_ptr() as u32;
        self.layout.foreign_header = self.foreign_header.as_mut_ptr() as u32;
        self.layout.separation = self.separation.as_mut_ptr() as u32;
    }
    fn bind_output(&mut self) {
        self.layout.output = self.block.ptr() as u32 + (crate::store::block::HEADER * 8) as u32;
        self.layout.output_heap = self.block.ptr() as u32 + self.block.words[BlockHeader::HEAP] as u32 * 8;
    }
    fn bind_bounds(&mut self) { self.layout.bounds_cache = self.bounds_cache.as_mut_ptr() as u32; }
    fn bind_events(&mut self, events: u32) { self.layout.events = events; }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn f3_init(world: u32, level: f64, body_serial: u32) -> *const f64 { f3_init_in(crate::abi::facade::world(), world, level, body_serial).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f3_adopt() -> u32 { f3_adopt_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_weapon_services(weapon: u32, count: u32) { f3_weapon_services_in(crate::abi::facade::world(), weapon, count) }
    #[no_mangle]
    pub extern "C" fn f3_step(count: u32, dt: f64, level: f64, tick: f64) -> u32 { f3_step_in(crate::abi::facade::world(), count, dt, level, tick) }
    #[no_mangle]
    pub extern "C" fn f3_command() -> u32 { f3_command_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_acquire(class: u32) -> i32 { f3_acquire_in(crate::abi::facade::world(), class) }
    #[no_mangle]
    pub extern "C" fn f3_sensor() -> *const f64 { f3_sensor_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f3_pose() -> u32 { f3_pose_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_foreign(count: u32) { f3_foreign_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn f3_registry_prepare() { f3_registry_prepare_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_registry_add(kind: u32, generation: u32) -> u32 { f3_registry_add_in(crate::abi::facade::world(), kind, generation) }
    #[no_mangle]
    pub extern "C" fn f3_registry_remove(id: u32) { f3_registry_remove_in(&mut crate::abi::facade::world().sim, id) }
    #[no_mangle]
    pub extern "C" fn f3_body_names(count: u32) { f3_body_names_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn f3_missile_classes(count: u32) { f3_missile_classes_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn f3_relations(count:u32) -> u32 { f3_relations_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn f3_hit_policy() { f3_hit_policy_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_blast_binding() { f3_blast_binding_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_impact_rules() { f3_impact_rules_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_world_blast() { f3_world_blast_in(crate::abi::facade::world()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{f3_init, f3_adopt, f3_weapon_services, f3_step, f3_command, f3_acquire, f3_sensor, f3_pose, f3_foreign, f3_registry_prepare, f3_registry_add, f3_registry_remove, f3_body_names, f3_missile_classes, f3_relations, f3_hit_policy, f3_blast_binding, f3_impact_rules, f3_world_blast};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_banks {
    use super::*;
    #[test]
    fn nested_ordnance_refuses_at_capacity_and_reuses_every_frame_after_unwind() {
        let mut w = crate::world::owner::World::new();
        f3_init_in(&mut w, 0, f64::NAN, 0);
        let mut frames = Vec::new();
        for depth in 0..64 {
            let (frame, query) = take_ordnance_query_in(&mut w.units).unwrap();
            assert_eq!(frame, depth);
            assert_eq!(query.capacity(), RESERVED_UNITS);
            frames.push((frame, query));
        }
        let first_buffer = frames[0].1.as_ptr();
        // A nested fire is refused before even reading its definition. An
        // invalid definition here would panic if the guard ran too late.
        let args = FireArgs::default();
        let mut world = crate::ordnance::module::JsWorld::with_services(&mut w, None);
        let result = crate::ordnance::Cx::new(&mut world).fire_round(args.def, args.origin, args.direction,
            args.owner, args.visual, args.ignore_body, args.emitter, args.channel, args.target_id, args.target_generation);
        assert!(result.is_none());
        world.finish();
        assert_eq!(w.units.state.as_ref().unwrap().ordnance_refused, 1);
        assert!(take_ordnance_query_in(&mut w.units).is_none());
        assert!(take_ordnance_query_in(&mut w.units).is_none());
        assert_eq!(w.units.state.as_ref().unwrap().ordnance_depth, 64);
        for (frame, query) in frames.into_iter().rev() { put_ordnance_query_in(&mut w.units, frame, query); }
        assert!(admit_ordnance_fire_in(&mut w.units));
        let (frame, query) = take_ordnance_query_in(&mut w.units).unwrap();
        assert_eq!(frame, 0);
        assert_eq!(query.as_ptr(), first_buffer);
        assert_eq!(query.capacity(), RESERVED_UNITS);
        put_ordnance_query_in(&mut w.units, frame, query);
        put_ordnance_query_in(&mut w.units, usize::MAX, Vec::new());
        let state = w.units.state.as_ref().unwrap();
        assert_eq!(state.ordnance_depth, 0);
        assert_eq!(state.ordnance_refused, 4);
    }

    #[test]
    fn native_publication_keeps_widths_and_counts_without_linear_memory_addresses() {
        let mut w = crate::world::owner::World::new();
        f3_init_in(&mut w, 0, f64::NAN, 0);
        prepare_bounds_in(&mut w, 3);
        write_publication_in(&mut w);
        let s = w.units.state.as_ref().unwrap();
        let d = &s.layout;
        assert_eq!(d.descriptor_words, F3Boundary::WORDS as u32);
        assert_eq!(d.bounds_capacity, 3);
        assert_eq!(d.output_rows, 0);
        assert_eq!([d.input, d.random_states, d.aim_input, d.command, d.acknowledgement,
            d.random_epoch, d.random_globals, d.aim_header, d.foreign_header,
            d.separation, d.events, d.output, d.output_heap, d.bounds_cache], [0; 14]);
    }
}

pub(crate) fn recon_ref(units:&super::owner::Units)->Option<&crate::recon::Authority> { units.state.as_ref()?.recon.as_ref() }

pub(crate) fn recon_services_ref(units:&super::owner::Units)->&crate::recon::module::Services { &units.state.as_ref().expect("prepared unit service").recon_services }

pub(crate) fn with_owner_rows<T>(w:&mut crate::world::owner::World,run:impl FnOnce(&mut Store,&mut Ownership)->T)->T { with_rows_in(&mut w.sim.store,w.units.state.as_mut().expect("prepared unit service"),run) }

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_sensor_masks {
    use super::*;
    #[test]
    fn sensor_mask_is_complete_and_bounded_before_writing() {
        let mut w = crate::world::owner::World::new();
        assert!(!f3_sensor_mask_in(&mut w, &[]));
        f3_init_in(&mut w, 0, f64::NAN, 0);
        w.sim.store.units.list.push(Member::Unit(Ref::new(0, 1)));
        w.sim.store.units.list.push(Member::Unit(Ref::new(1, 1)));
        assert!(f3_sensor_mask_in(&mut w, &[true, false]));
        assert!(!f3_sensor_mask_in(&mut w, &[false]));
        assert_eq!(&w.units.state.as_ref().unwrap().input[..2], &[1., 0.]);
        assert!(f3_sensor_mask_in(&mut w, &[false, true]));
        assert_eq!(&w.units.state.as_ref().unwrap().input[..2], &[0., 1.]);
    }
}
