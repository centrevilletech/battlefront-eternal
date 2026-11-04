// Whole vehicle-phase state alongside the existing method frames. The phase's
// normal capture journal records boundary calls and named random-stream draws.
// Turret crews remain in their adjacent slot until the final F2 integration.
import { exportWeapon } from '../../src/sim/gen/combat.js';

export function f2Spec({ hullExport, seatState, crewState, poseState, tetherState, boundaries }) {
  return { frames: [{
    phase: 'vehicles', world: true, whole: ['SeatInput'], boundaries,
    state(game, args, cap) {
      const vs = game.vehicles;
      if (!vs) return { vehicles: null };
      // Leases must be exported whole before their derived hull views. The
      // shared helper writes those views explicitly, avoiding shape aliasing.
      const seats = seatState(game, cap), turrets = crewState(game, cap);
      return {
        time: vs.time, ticks: vs.ticks, acc: vs.acc, flightAcc: vs.flightAcc,
        tickAlpha: vs.tickAlpha, spawnOccupant: vs.spawnOccupant?.clone() ?? null,
        slots: vs.slots.map(s => ({ remaining: s.remaining, vehicle: s.vehicle,
          variants: [...s.variants] })),
        hulls: vs.vehicles.map(v => hullExport(cap, v)),
        poses: vs.vehicles.map(v => poseState(v, [1, 0], false, true)),
        seats, turrets,
        mountedWeapons: vs.vehicles.map(v => v.seats.map(s => s.weapons.map(w => exportWeapon(w.state, { seen: new WeakMap(), def: (_table, def) => def })))),
        tethers: tetherState(game, cap),
        bodyFlags: vs.vehicles.map(v => ({ body: v.body, enabled: v.body.enabled })),
        pendingLooks: vs.vehicles.map(v => {
          const input = vs.f2?.looks.get(v);
          return input ? { ...input } : null;
        }),
      };
    },
  }] };
}
