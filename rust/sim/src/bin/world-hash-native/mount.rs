//! Recorded mount attempts from tools/world-hash.mjs's replay loop. The
//! recording chooses the target; authority code validates and applies the
//! transition before this frame's ordinary input, at the old tick's clock.
use super::host::Host;
use bfe_sim::gen::{self, BlockHeader as B, Unit, Vehicle, Vehicles};
use bfe_sim::host::mount::{self, Request};
use bfe_sim::store::Ref;
use serde_json::Value;

/// Decode the cold hull index in authored fleet order, rather than treating
/// it as a store slot (hull slots and registry unit ids are different).
fn hull(host: &Host, index: usize) -> Result<(Ref<Vehicle>, Vehicle), String> {
    let words = host
        .context
        .export(u32::from(gen::KIND_VEHICLE), 1)
        .map_err(|e| e.message().to_string())?;
    let fleet_words = host
        .context
        .export(u32::from(gen::KIND_VEHICLES), 1)
        .map_err(|e| e.message().to_string())?;
    let mut fleet = Vehicles::default();
    fleet.take_full(
        &fleet_words[B::WORDS..],
        &fleet_words[fleet_words[B::HEAP] as usize..],
    );
    let reference = *fleet
        .vehicles
        .get(index)
        .ok_or("recorded hull index is absent")?;
    let stride = words[B::STRIDE] as usize;
    let row = words[B::WORDS..B::WORDS + words[B::ROWS] as usize * stride]
        .chunks_exact(stride)
        .find(|row| row[0] as u32 == reference.slot && row[1] as u32 == reference.gen)
        .ok_or("recorded hull identity is absent")?;
    let mut vehicle = Vehicle::default();
    vehicle.take_full(row, &words[words[B::HEAP] as usize..]);
    Ok((Ref::new(row[0] as u32, row[1] as u32), vehicle))
}

fn request(host: &Host, action: &Value) -> Result<Request, String> {
    match action["kind"].as_str() {
        Some("turret") => {
            let index = action["turret"].as_u64().ok_or("mount without turret")? as usize;
            let life = host
                .context
                .turret_life(index)
                .map_err(|e| e.message().to_string())?
                .ok_or("recorded turret is absent")?;
            Ok(Request::Turret(index, life))
        }
        Some("leave") => Ok(Request::Leave),
        Some("hull") => {
            let (reference, vehicle) = hull(
                host,
                action["hull"].as_u64().ok_or("mount without hull")? as usize,
            )?;
            Ok(Request::Hull(
                reference,
                vehicle.target_generation,
                action["seat"].as_u64().ok_or("mount without seat")? as usize,
            ))
        }
        _ => Err("unknown recorded mount".into()),
    }
}

/// Fail on stale recorded lives or changed outcomes, just like the oracle;
/// a refused attempt is still executed because native validation owns it.
pub fn replay(host: &mut Host, actions: &Value, tick: f64) -> Result<(), String> {
    for action in actions.as_array().ok_or("frame without mount array")? {
        let rows = unit_rows(&host.context)?;
        let slot = host.player_unit.ok_or("mount without player")?;
        let (_, generation, unit): &(u32, u32, Unit) = rows
            .iter()
            .find(|(s, _, _)| *s == slot)
            .ok_or("mount without unit")?;
        if action["unit"].as_u64() != Some(u64::from(slot))
            || action["life"].as_f64() != Some(unit.generation)
        {
            return Err("recorded mount life is stale".into());
        }
        let command = mount::command(
            Ref::new(slot, *generation),
            unit.generation,
            unit.seat,
            request(host, action)?,
        );
        let accepted = host
            .context
            .world_host_mount(&command, tick)
            .map_err(|e| e.message().to_string())?;
        if action["kind"] != "leave" && action["accepted"].as_bool() != Some(accepted) {
            return Err("recorded mount outcome differs".into());
        }
        // F3Sim.afterCommand drains the immediate board/unboard notice at
        // the previous clock. Later player input must see that journal entry.
    }
    Ok(())
}

fn unit_rows(context: &bfe_sim::abi::context::Context) -> Result<Vec<(u32, u32, Unit)>, String> {
    let words = context
        .export(u32::from(gen::KIND_UNIT), 1)
        .map_err(|e| e.message().to_string())?;
    let stride = words[B::STRIDE] as usize;
    let heap = &words[words[B::HEAP] as usize..];
    Ok(words[B::WORDS..B::WORDS + words[B::ROWS] as usize * stride]
        .chunks_exact(stride)
        .map(|row| {
            let mut unit = Unit::default();
            unit.take_full(row, heap);
            (row[0] as u32, row[1] as u32, unit)
        })
        .collect())
}
