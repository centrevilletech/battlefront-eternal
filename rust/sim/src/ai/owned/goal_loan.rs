//! The controller lent to the goal bots outside an AI tick, so rules being
//! replaced can hand their bots back before they are dropped.

use crate::ai::control::goal_orders::GoalCore;
use crate::ai::goals::GoalController;
use crate::world::owner::World;

/// Hand every bot `bots` steered back to the released controller. False
/// when the AI battle or the unit services are lent out at this moment;
/// nothing was changed then. Goal bots of an AI battle that was replaced
/// since are forgotten: their orders went with it.
pub(crate) fn retire_in(w: &mut World, bots: &mut GoalController) -> bool {
    let Some(battle) = w.ai.battle() else { return false };
    if bots.steered() != Some(std::ptr::from_ref(&battle.runtime.core) as usize) {
        bots.forget();
        return true;
    }
    if !battle.ready() {
        return false;
    }
    let outer = crate::units::f3::external_in(w);
    let mut owned = w.ai.take_battle().expect("ready AI battle");
    let battle = &mut *owned;
    let mut done = false;
    crate::units::f3::with_bare_services_in(w, |w, services| {
        let Some(services) = services else { return };
        use super::callbacks::Factory;
        let context = super::host::Context {
            time: battle.facts.time,
            follow_far: battle.runtime.core.follow_radii().1,
            stream_a: battle.stream_a,
            vehicles: battle.vehicles.clone(),
            voice: &battle.voices,
            items: &battle.item_owners,
        };
        let mut services = context.services(services.recon, services.queries);
        let mut random = battle.runtime.random.share();
        super::live::with_rows(w, &mut battle.state, |rows| {
            let mut world = super::world::World {
                rows,
                random: &mut random,
                combat: &mut battle.combat,
                services: &mut services,
                settings: &battle.settings,
                facts: &battle.facts,
            };
            bots.retire(GoalCore::new(&mut battle.runtime.core), &mut world, &mut battle.runtime.random);
        });
        done = true;
    });
    w.ai.return_battle(owned);
    outer.finish(w, true);
    done
}
