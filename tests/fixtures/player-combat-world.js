import { playersWorld } from './players-world.js';

// Rewind fixtures use the same input lanes and history capture as a server.
// The whole tick consumes each edge; no fixture restores the removed history
// entry or writes network shot fields onto an authoritative Unit.
export async function playerCombatWorld(definitions, unitOptions = {}, {className = 'shooter', position = [0,0,0], defer = false} = {}) {
  const ordered = { viewpoint: {class:'viewpoint',base:'soldier',MaxHealth:100,weapons:[]}, ...definitions };
  const f = await playersWorld({ definitions: ordered, unitOptions });
  let shooter = null;
  async function admit() {
    await f.world.addPlayer('shooter', { team: 1, className, reliableFire: true, separateActions: true });
    shooter = f.world.players.get('shooter').unit;
    f.game.unitsSim.placeBody(shooter, position[0], position[1], position[2], 0);
    return shooter;
  }
  if (!defer) await admit();
  return { ...f, get shooter() { return shooter; }, admit,
    spawn(...args) {
      const unit = f.spawn(...args);
      // These other synthetic bodies have foreign fixture controllers, not
      // connections. Their intents still enter the ordinary native unit phase.
      unit.networkHuman = false;
      unit.team = unit.combat.team = args[3] ? 1 : 2; unit.f3.publishMetadata(unit);
      return unit;
    },
    capture(through = 10) { while (f.game.worldClock.tick <= through) f.world.step(1000); },
    fire(extra = {}) {
      return f.world.fire('shooter', { epoch: 1, id: 41, tick: f.game.worldClock.tick + 1,
        generation: shooter.generation, button: 0, pressed: true, viewTick: 8, aimYaw: 0, aimPitch: 0, ...extra });
    },
    step() { return f.world.step(1000); },
  };
}
