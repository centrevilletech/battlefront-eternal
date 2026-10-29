import { UnitFlyAgent } from "./agents.js";

// The mounted controllers (docs/ai-vehicles.md). The 2004 game gives
// every occupied control its own controller; they differ
// only in which source goals they turn into which states, and in the
// flyer's objective agent:
//
//   UnitMobileController (hover and walker pilots, as soldiers): 1..6 to the
//     same states, 8 to 9, 9 to 10, 10 to 11.
//   UnitStationaryController (gunner and passenger seats, turrets): 1 and 3
//     to Wait (1), 2 to Combat (2), 6 to Unboard (6); nothing else. Its
//     wait looks around +-30 degrees, not 75, when the control belongs to a
//     building (a turret).
//   UnitFlyController (flyer pilots): 2 to Combat, 3 to the flyer agent's
//     state 7, 4 to the crew wait, 10 to Hold (11); nothing else.
//
// A policy is { name, goalState(kind), agentFor(state, controller), bind(
// controller, unit) }; the controller's UnitController consults
// it when its unit is a seat (docs/ai-vehicles.md "Integration").

const MOBILE = new Map([[1, 1], [2, 2], [3, 3], [4, 4], [5, 5], [6, 6], [8, 9], [9, 10], [10, 11]]);
const STATIONARY = new Map([[1, 1], [3, 1], [2, 2], [6, 6]]);
const FLY = new Map([[2, 2], [3, 7], [4, 4], [10, 11]]);

export const MOBILE_POLICY = Object.freeze({
  name: "UnitMobileController",
  goalState: (kind) => MOBILE.get(kind) ?? 0,
  agentFor: () => null,
  bind() {},
});

export const STATIONARY_POLICY = Object.freeze({
  name: "UnitStationaryController",
  goalState: (kind) => STATIONARY.get(kind) ?? 0,
  agentFor: () => null,
  // The idle-look flag: only a control whose physical body is a building.
  bind(controller, unit) {
    controller.homeFacing = Boolean(unit.turret);
  },
});

export const FLY_POLICY = Object.freeze({
  name: "UnitFlyController",
  goalState: (kind) => FLY.get(kind) ?? 0,
  agentFor: (state) => (state === 7 ? (controller, goal) => new UnitFlyAgent(controller, goal) : null),
  bind() {},
});

// The policy for the control a controller drives: none for a soldier on
// foot (the ordinary unit controller).
export function controllerPolicy(unit) {
  if (!unit?.seat || unit.human || !unit.lease) return null;
  if (unit.kind === 5) return FLY_POLICY;
  if (unit.kind === 7) return STATIONARY_POLICY;
  return MOBILE_POLICY;
}
