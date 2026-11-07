//! Game.step's authoritative interval. The host prepares one input boundary
//! and consumes publication only after every native phase has returned.
//! Each phase retains its original service loans and recursive death/shot
//! journal points; the coordinator never holds a store borrow across an entry.
use crate::{abi::sim, gen::{WorldTickInput, WorldTickOutput, WorldTickSetup}};

/// Cold binding reserves the generated input and adopts the water clock.
/// Ordinary ticks never enter a host scratch allocator or reimport this clock.
#[no_mangle]
pub extern "C" fn world_step_init() -> *mut f64 {
    let setup={let(row,heap)=sim().calls.args();WorldTickSetup::take(row,heap)};
    crate::abi::world().tick.water=setup.water.then_some(super::Water {
        time:setup.water_time,height:setup.water_height,bobbing:setup.water_bob,wave:0.0,
    });
    crate::abi::world().tick.input.as_mut_ptr()
}

/// The input is copied before any phase can reuse the shared command block.
/// Calling existing owner entries preserves their independent event epochs,
/// numeric substeps and publication banks without a host phase dispatcher.
#[no_mangle]
pub extern "C" fn world_step_run() -> *const f64 {
    let input = WorldTickInput::take(&crate::abi::world().tick.input,&[]);
    let mut output = WorldTickOutput {clock:input.clock.clone(),..Default::default()};
    let before=crate::allocations::snapshot();
    if input.dt.is_finite() && input.dt >= 0.0 && input.tick.is_finite() && !crate::abi::world().tick.running
        && crate::units::f3::registered_viewpoint().is_some_and(|r|sim().store.unit.get(r).is_some())
        && input.tick==input.clock.tick+1.0
        && super::clock::advance(&mut output.clock,input.dt,false,&mut CompletedClock)==Ok(1.0) {
        crate::abi::world().tick.running=true;
        let water=crate::abi::world().tick.water.as_mut();
        let level=if let Some(water)=water {
            let level=water.step(input.dt);output.water_time=water.time;output.wave=water.wave;level
        }else{f64::NAN};
        output.level=level;
        crate::ordnance::module::ord_water(0,u32::from(level.is_finite()));
        crate::ordnance::module::ord_water(1,u32::from(level.is_finite()));
        if input.ai {
            for (index, kind) in [0.0,2.0,3.0,1.0].into_iter().enumerate() {
                if input.squads & (1<<index)==0 {continue;}
                let Some(local)=crate::units::f3::registered_viewpoint() else {continue};
                let Some(unit)=sim().store.unit.get(local) else {continue};
                if unit.combat.dead {continue;}
                let Some(character)=unit.character.and_then(crate::battle::f4_core::character_handle) else {continue};
                let command=crate::gen::AiPlayerCommand {character,kind,time:input.tick/60.0};
                sim().calls.result(crate::gen::AiPlayerCommand::WORDS,|row,heap|command.put(row,heap));
                if crate::ai::owned::entry::ai_world_command()!=0 {output.squads|=1<<index;}
            }
        }
        crate::vehicles::module::f2::world_level(if level.is_finite() {level}else{0.0});
        crate::battle::f4_live::f4_viewpoint_clock();
        if input.camera!=0 {crate::cameras::world_frame(input.camera,input.offline_camera);}
        let ai = !input.ai || crate::ai::owned::entry::ai_world_phase() != 0;
        if ai && crate::units::f3::f3_step(input.units, input.dt, level, input.tick) != 0 {
            if input.live { crate::battle::f4_live::f4_live(); }
            output.ordnance = crate::ordnance::module::ord_update(input.dt);
            let targets = crate::units::f3_items::world_targets();
            if let Some(targets)=targets {
                output.items = crate::items::module::items_update(input.dt, targets);
            }
            crate::battle::f4_tail::f4_tail();
            if input.ai { crate::ai::owned::entry::ai_world_dangers(); }
            output.accepted = targets.is_some();
        }
        crate::abi::world().tick.running=false;
        // Each producer kept dirty rows and retired leases until this point.
        // Commit their encoded baselines once, against what the host consumed.
        crate::units::f3::publish_changed();
    }
    let used=crate::allocations::snapshot().since(before);
    output.allocations=used.calls as u32;output.reallocations=used.reallocs as u32;output.allocation_bytes=used.bytes as u32;
    let result=&mut crate::abi::world().tick.output;
    result.begin(1,WorldTickOutput::WORDS);
    let (row,heap)=result.row(0);output.put(row,heap);result.finish();
    result.ptr()
}

// Game.step consumes one already-stamped command, so pacing and host now()
// are outside this interval. Reuse FixedClock's compensated arithmetic and
// publish its completion after the tick's ordered notices have been delivered.
struct CompletedClock;
impl super::clock::Host for CompletedClock {
    fn now(&mut self,_:&mut crate::gen::WorldClockState)->f64 {0.0}
    fn step(&mut self,_:&mut crate::gen::WorldClockState,_:f64,_:f64) {}
}
