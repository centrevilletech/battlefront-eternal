//! The water the world adopts at `world_step_init`: WaterState's constructor
//! (src/runtime/water-state.js) read by WorldOperation (src/sim/wasm/world.js),
//! which stamps presence, the surface's own clock (0 at construction), its
//! height and whether it bobs.
use bfe_content::js::to_number;
use bfe_content::Value;
use bfe_sim::gen::WorldTickSetup;

/// game.js: `world.water ? new WaterState(world.water) : null`, then
/// `setup.water=Boolean(water); setup.waterTime=water?water.simulationTime:0;
/// setup.waterHeight=water?water.height:0; setup.waterBob=Boolean(water?.bobbing)`.
/// `world` is map.json as the battle plays it (selectMission).
pub fn tick_setup(world: &Value) -> WorldTickSetup {
    let Some(water) = world.get("water").filter(|w| w.truthy()) else {
        return WorldTickSetup::default();
    };
    WorldTickSetup {
        water: true,
        water_time: 0.0,
        // A height the map leaves out writes NaN into the row, as the typed
        // array write of `undefined` does; the content tools always set it.
        water_height: to_number(water.get("height")),
        water_bob: bobbing(water),
    }
}

/// WaterState: `(data.look.OscillationEnable?.[0] ?? 1) > 0`. A missing
/// flag means the surface bobs; an authored 0 (Jabba's Palace) keeps it flat.
fn bobbing(water: &Value) -> bool {
    let flag = water.get("look").and_then(|look| look.get("OscillationEnable")).and_then(|enable| enable.at(0));
    match flag {
        None | Some(Value::Null) => true,
        Some(value) => to_number(Some(value)) > 0.0,
    }
}
