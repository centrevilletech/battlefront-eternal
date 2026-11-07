//! Game.step's authoritative interval. The host prepares one input boundary
//! and consumes publication only after every native phase has returned.
//! Each phase retains its original service loans and recursive death/shot
//! journal points; the coordinator never holds a store borrow across an entry.
use crate::{world::owner::World, gen::{WorldTickInput, WorldTickOutput, WorldTickSetup}};

impl World {
    /// Cold binding adopts WaterState's independent authoritative clock.
    /// Ordinary ticks never reimport or advance it from a page frame.
    pub fn step_init(&mut self, setup: &WorldTickSetup) {
        self.tick.water=setup.water.then_some(super::Water {
            time:setup.water_time,height:setup.water_height,bobbing:setup.water_bob,wave:0.0,
        });
    }

    /// The caller keeps input independent of the shared command block.
    /// Calling existing owner entries preserves their independent event epochs,
    /// numeric substeps and publication banks without a host phase dispatcher.
    pub fn step(&mut self, input: &WorldTickInput) -> WorldTickOutput {
        let mut output = WorldTickOutput {clock:input.clock.clone(),..Default::default()};
        let before=crate::allocations::snapshot();
        if input.dt.is_finite() && input.dt >= 0.0 && input.tick.is_finite() && !self.tick.running
            && crate::units::f3::registered_viewpoint().is_some_and(|r|self.sim.store.unit.get(r).is_some())
            && input.tick==input.clock.tick+1.0
            && super::clock::advance(&mut output.clock,input.dt,false,&mut CompletedClock)==Ok(1.0) {
            self.tick.running=true;
            let water=self.tick.water.as_mut();
            let level=if let Some(water)=water {
                let level=water.step(input.dt);output.water_time=water.time;output.wave=water.wave;level
            }else{f64::NAN};
            output.level=level;
            crate::ordnance::module::ord_water_in(self,0,u32::from(level.is_finite()));
            crate::ordnance::module::ord_water_in(self,1,u32::from(level.is_finite()));
            if input.ai {
                for (index, kind) in [0.0,2.0,3.0,1.0].into_iter().enumerate() {
                    if input.squads & (1<<index)==0 {continue;}
                    let Some(local)=crate::units::f3::registered_viewpoint() else {continue};
                    let Some(unit)=self.sim.store.unit.get(local) else {continue};
                    if unit.combat.dead {continue;}
                    let Some(character)=unit.character.and_then(|r| crate::battle::f4_core::character_handle_in(&mut self.battle, r)) else {continue};
                    let command=crate::gen::AiPlayerCommand {character,kind,time:input.tick/60.0};
                    self.sim.calls.result(crate::gen::AiPlayerCommand::WORDS,|row,heap|command.put(row,heap));
                    if crate::ai::owned::entry::ai_world_command()!=0 {output.squads|=1<<index;}
                }
            }
            crate::vehicles::module::f2::world_level_in(self,if level.is_finite() {level}else{0.0});
            crate::battle::f4_live::f4_viewpoint_clock_in(self);
            if input.camera!=0 {crate::cameras::world_frame(self,input.camera,input.offline_camera);}
            let ai = !input.ai || crate::ai::owned::entry::ai_world_phase() != 0;
            if ai && crate::units::f3::f3_step(input.units, input.dt, level, input.tick) != 0 {
                if input.live { crate::battle::f4_live::f4_live_in(self); }
                output.ordnance = crate::ordnance::module::ord_update_in(self,input.dt);
                let targets = crate::units::f3_items::world_targets();
                if let Some(targets)=targets {
                    output.items = crate::items::module::items_update_in(self,input.dt, targets);
                }
                crate::battle::f4_tail::f4_tail_in(self);
                if input.ai { crate::ai::owned::entry::ai_world_dangers(); }
                output.accepted = targets.is_some();
            }
            self.tick.running=false;
            // Each producer kept dirty rows and retired leases until this point.
            // Commit their encoded baselines once, against what the host consumed.
            crate::units::f3::publish_changed();
        }
        let used=crate::allocations::snapshot().since(before);
        output.allocations=used.calls as u32;output.reallocations=used.reallocs as u32;output.allocation_bytes=used.bytes as u32;
        output
    }
}

// The WASM facade decodes inputs and encodes the completed value only.
pub fn world_step_init_in(w: &mut World) -> Option<&mut [f64]> {
    let setup={let(row,heap)=w.sim.calls.args();WorldTickSetup::take(row,heap)};
    w.step_init(&setup); Some(&mut w.tick.input[..])
}

pub fn world_step_run_in(w: &mut World) -> Option<&[f64]> {
    let input=WorldTickInput::take(&w.tick.input,&[]);
    let output=w.step(&input);
    let result=&mut w.tick.output;
    result.begin(1,WorldTickOutput::WORDS);
    let (row,heap)=result.row(0);output.put(row,heap);result.finish();Some(&result.words[..])
}

// Game.step consumes one already-stamped command, so pacing and host now()
// are outside this interval. Reuse FixedClock's compensated arithmetic and
// publish its completion after the tick's ordered notices have been delivered.
struct CompletedClock;
impl super::clock::Host for CompletedClock {
    fn now(&mut self,_:&mut crate::gen::WorldClockState)->f64 {0.0}
    fn step(&mut self,_:&mut crate::gen::WorldClockState,_:f64,_:f64) {}
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn world_step_init() -> *mut f64 { world_step_init_in(crate::abi::facade::world()).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn world_step_run() -> *const f64 { world_step_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
