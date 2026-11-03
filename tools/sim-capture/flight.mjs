// The flight package's frames (tools/sim-capture/specs.mjs `flight`, flip
// F2 with the hulls and the seats): a craft's tick (Vehicle.tick on a
// flyer: its crew's aim, then Vehicle.fly, which runs src/flight/model.js
// stepFlight, src/flight/pilotless.js or abandonLift, and
// src/flight/targeting.js updateFlightTarget) and its pose at the tick
// (Vehicle.aimTick and render on a flyer: its frame and lean, its gear, its
// seats' aim). rust/sim/tests/replay_flight.rs replays them.
//
// The frames are on the hulls' prototype, so every hull's call opens one; a
// ground hull's (the hulls package's frames, specs.mjs `vehicles`) is
// written as { skipped: true }.
//
// A craft's tick reads the battle's registry as it looks for targets (the
// units, hulls, turrets and buildings in the registry's order: whether each
// lives, its team and life, whether it is the craft's own entry, the hull
// behind a hull's entry, where a hull, turret or building is aimed at) and
// the relation between its team and each entry's. The frame's state carries
// those as the tick found them, and again after each call that can change
// them (a crash's damage, its blast and the battle's hooks). A unit's aim
// point is a call across the boundary: a unit works its hit shapes out on
// the first ask in a tick, so when it is asked is behaviour.
import { Vector3 } from 'three';
import { FLIGHT_CLASSES } from '../../src/flight/config.js';
import { Unit } from '../../src/units.js';

// The class tunings as the battle began: a frame carries the live ones only
// once a command has retuned one (src/game.js step, flightTuning).
const DEFAULT_TUNING = JSON.stringify(FLIGHT_CLASSES);
const tuningValue = () => {
  const now = JSON.stringify(FLIGHT_CLASSES);
  return now === DEFAULT_TUNING ? null : JSON.parse(now);
};

// The registry's entries as targeting reads them (src/flight/targeting.js
// inCone, clear, updateFlightTarget), through getters that only read: a
// unit's aim point is asked across the boundary (`unit`), any other entry's
// (a hull's, a turret's, a building's: the middle of its box as it stands)
// is here; one without an aim point is aimed at at its position.
function targetsValue(game, craft) {
  return game.units.all().map((t) => {
    const unit = t instanceof Unit, aims = typeof t.aimPoint === 'function';
    return {
      id: t.id, generation: t.generation, alive: t.alive, team: t.team, own: t === craft.aiTarget, unit,
      body: t.vehicle?.body ?? null, point: unit ? null : aims ? t.aimPoint(new Vector3()) : t.position.clone(),
    };
  });
}

// The registry's relation of the craft's team to each team among its
// entries (Units.relation, which reads the alliances set so far and the
// mission's table): [team, relation] pairs.
function relationsValue(game, craft) {
  const teams = [];
  for (const t of game.units.all()) if (!teams.some((x) => Object.is(x, t.team))) teams.push(t.team);
  return teams.map((team) => [team, game.units.relation(craft.team, team)]);
}

// What updateFlightTarget leaves in each weapon of each seat.
const locksValue = (v) => v.seats.map((s) => s.weapons.map((w) => ({
  lockTargetId: w.state.lockTargetId, lockGeneration: w.state.lockGeneration, lockElapsed: w.state.lockElapsed, lockComplete: w.state.lockComplete,
})));

// What fly() reads of its pilot's lease: whether he is a network player,
// and then his flight keys (src/runtime/multiplayer-world.js intent.flight).
function pilotValue(v) {
  const lease = v.seats[0]?.lease;
  const network = Boolean(lease?.unit?.networkHuman);
  return { networkHuman: network, flight: network ? lease.intent.flight ?? null : null };
}

// The battle's flight settings: the environment (ceiling, bounds, zones,
// space), the mission's fly height, whether steering assists are on, and
// the class tunings when retuned.
const worldValue = (game) => ({
  env: game.vehicles.flightEnvironment ?? null, maxFlyHeight: game.vehicles.maxFlyHeight, assists: Boolean(game.vehicles.flightAssists), tuning: tuningValue(),
});

export function flightSpec({ hullPrototype, rigNodes, poseState, poseBoundaries }) {
  // The craft whose tick is open (each boundary call's extra reads it).
  let current = null;

  // Vehicle.tick on a flyer.
  const tick = {
    method: 'flyer.tick', target: hullPrototype, whole: ['SeatInput'],
    state: (game, args, cap, self) => {
      if (!self.flying) { current = null; return { skipped: true }; }
      // A craft that is gone or asleep and empty only moves its last pose
      // on: the hull is all its tick reads.
      if (!self.alive || self.sleeping && !self.occupied) { current = null; return { hull: cap.export('vehicle', self), idle: true }; }
      current = self;
      return {
        hull: cap.export('vehicle', self),
        water: game.vehicles.water ? { level: game.vehicles.water.level } : null,
        pilot: pilotValue(self),
        targets: targetsValue(game, self),
        relations: relationsValue(game, self),
        locks: locksValue(self),
        world: worldValue(game),
      };
    },
    boundaries: (game) => {
      const w = game.vehicles?.world ?? null;
      // The registry and the hull as a call that can change them left them.
      const after = (g, cap) => (current ? { targets: targetsValue(g, current), relations: relationsValue(g, current), hull: cap.export('vehicle', current) } : { skipped: true });
      return [
        ['collision.cast', game.collision, 'cast'],
        ['collision.moveBody', game.collision, 'moveBody'],
        ['world.enableBody', w, 'enableBody'],
        ['world.bodyEnabled', w, 'bodyEnabled'],
        ['water.over', game.vehicles?.water ?? null, 'over'],
        ['world.inDeathRegion', w, 'inDeathRegion'],
        ['vehicles.inDeathRegion', game.vehicles ?? null, 'inDeathRegion'],
        ['world.damaged', w, 'damaged', after],
        ['vehicles.destroyedCrew', game.vehicles ?? null, 'destroyedCrew', after],
        ['world.destroyed', w, 'destroyed', after],
        ['world.explosion', w, 'explosion', after],
        ['world.effect', w, 'effect'],
        ['world.sound', w, 'sound'],
        ['unit.aimPoint', Unit.prototype, 'aimPoint'],
      ];
    },
  };

  // A flyer's pose (aimTick, render), as the hulls package writes a ground
  // hull's (specs.mjs poseState), with its rig every time and what its
  // frame, lean and gear read.
  const poseFrame = (method) => {
    let entering = true;
    return {
      method: method.replace('vehicle.', 'flyer.'), target: hullPrototype, whole: ['SeatInput'], boundaries: poseBoundaries,
      state: (game, args, cap, self) => {
        const end = !entering;
        entering = !entering;
        if (!self.flying) return { skipped: true };
        // A craft that is gone is not posed.
        if (!self.alive) return { hull: { alive: false } };
        const nodes = rigNodes(self);
        return {
          hull: {
            ...poseState(self, args, end),
            rig: nodes.flatMap((o) => [o.position.x, o.position.y, o.position.z, o.quaternion.x, o.quaternion.y, o.quaternion.z, o.quaternion.w, o.scale.x, o.scale.y, o.scale.z]),
            // A world's render after aimTick moves only the barrels, from
            // the world matrices aimTick left: those come with its start.
            world: end || method === 'vehicle.render' ? nodes.flatMap((o) => o.matrixWorld.elements) : null,
            gear: self.gear, flightPreviousQ: self.flightPreviousQ ?? null, flightFrameQ: self.flightFrameQ ?? null,
          },
        };
      },
    };
  };

  return { frames: [tick, poseFrame('vehicle.aimTick'), poseFrame('vehicle.render')] };
}
