import * as THREE from "three";
import { resolveRef } from "./controller.js";

// Squad commands, followers and tactical chatter (docs/ai-controller.md).
//
// A squad command is a small value: who issued it (a Character), what (move
// out, at ease, hold, follow; board and unboard for vehicles), when, and an
// optional entity (the leader for follow, the vehicle for board). Sending one
// rolls against a probability, then plays the sender's tactical VO and
// delivers it to nearby friendly AI. The 2004 game only delivers the
// infantry orders (0..3) when a human issued them: an AI's "follow me" or
// "move out" is chatter, heard but not obeyed. Board and unboard from an AI
// driver are delivered.
//
// The recipient's interrupt helper handles a delivered command (priority 5,
// interrupts.js): it says yes first, then may still refuse.

export const COMMAND = Object.freeze({ MOVE_OUT: 0, AT_EASE: 1, HOLD: 2, FOLLOW: 3, BOARD: 4, UNBOARD: 5 });

export class SquadCommand {
  // source: the issuing Character; target: {unit, generation} or null.
  constructor(source, kind, time = 0, target = null) {
    this.source = source;
    this.kind = kind;
    this.time = time;
    this.target = target;
  }
}

// The Character's current control: its remote droid, else the vehicle seat it
// occupies, else its soldier.
export function currentControllable(ai, character) {
  if (!character) return null;
  const vehicles = ai.vehicles;
  return vehicles?.currentControllable?.(character) ?? character.unit ?? null;
}

// Followers of `leader`, plus one: every Character whose current control's
// follow leader resolves to that entity. A null leader matches every
// control whose leader reference is empty or stale, as the original's
// comparison does.
export function countFollowers(ai, leader) {
  let count = 0;
  for (const character of ai.characters()) {
    if (!currentControllable(ai, character)) continue;
    const following = character.controller?.leader ? resolveRef(character.controller.leader) : null;
    if (following === (leader ?? null)) count += 1;
  }
  return count + 1;
}

// -- VO --------------------------------------------------------------------------------

// The command sound bank, keyed by entry offset: 13 entries for AI senders and
// the same 13 for the local player (the human offset below), plus the
// acknowledgement pair.
export const VO_BANK = Object.freeze({
  0x00: { name: "FieldMoveOut", suffix: "tac_moveout" },
  0x08: { name: "FieldHold", suffix: "tac_hold" },
  0x10: { name: "FieldFollow", suffix: "tac_follow" },
  0x18: { name: "DriverGetIn", suffix: "tac_needgunner" },
  0x20: { name: "DriverGetOut", suffix: "tac_moveout" },
  0x28: { name: "PassengerMoveOut", suffix: "tac_spreadout" },
  0x30: { name: "PassengerStop", suffix: "tac_stop" },
  0x38: { name: "PassengerGetIn", suffix: "tac_needgunner" },
  0x40: { name: "PassengerGetOut", suffix: "tac_moveout" },
  0x48: { name: "GunnerAllClear", suffix: "tac_allclear" },
  0x50: { name: "GunnerSteady", suffix: "tac_stop" },
  0x58: { name: "GunnerGetIn", suffix: "tac_needgunner" },
  0x60: { name: "GunnerGetOut", suffix: "tac_moveout" },
});

// Context: 0 unboarded sender, 1 other boarded control, 2 (bank only; the
// dispatcher never selects it), 3 mounted turret. Command -> bank offset.
export const VO_CONTEXT = Object.freeze([
  { [COMMAND.MOVE_OUT]: 0x00, [COMMAND.AT_EASE]: 0x00, [COMMAND.HOLD]: 0x08, [COMMAND.FOLLOW]: 0x10 },
  { [COMMAND.BOARD]: 0x18, [COMMAND.UNBOARD]: 0x20 },
  { [COMMAND.MOVE_OUT]: 0x28, [COMMAND.AT_EASE]: 0x28, [COMMAND.HOLD]: 0x30, [COMMAND.BOARD]: 0x38, [COMMAND.UNBOARD]: 0x40 },
  { [COMMAND.MOVE_OUT]: 0x48, [COMMAND.AT_EASE]: 0x48, [COMMAND.HOLD]: 0x50, [COMMAND.BOARD]: 0x58, [COMMAND.UNBOARD]: 0x60 },
]);

// The bank entry for a command in a context, or null when the context has
// none (no sound, no HUD notice).
export function voEntry(kind, context, human = false) {
  const offset = VO_CONTEXT[context]?.[kind];
  if (offset === undefined) return null;
  const entry = VO_BANK[offset];
  return {
    offset: human ? offset + 0x68 : offset,
    property: `${human ? "SC" : "AISC"}${entry.name}Sound`,
    suffix: entry.suffix,
  };
}

// Emit a semantic VO record into the simulation journal (audio decides what
// to play; AI never calls audio).
export function emitVo(ai, unit, fields) {
  const events = ai.world.events;
  if (!events?.emit || !unit) return null;
  return events.emit("vo", {
    source: { unit, generation: unit.generation },
    team: unit.team,
    position: unit.position,
    human: Boolean(unit.human),
    ...fields,
  });
}

// The sender's tactical VO for a command (positional for AI; the local
// player hears his own bank unpositioned and gets the HUD notice).
export function commandVo(ai, command, context) {
  const sender = currentControllable(ai, command.source);
  if (!sender) return null;
  const human = Boolean(command.source?.human);
  const entry = voEntry(command.kind, context, human);
  if (!entry) return null;
  return emitVo(ai, sender, {
    event: entry.suffix,
    property: entry.property,
    command: command.kind,
    context,
    positional: !human,
    notice: human,
  });
}

// -- sending and delivery ------------------------------------------------------------

// Send with a probability. A follow from a sender sitting in a vehicle becomes
// a board order for that vehicle. The draw is the second generator's uniform
// and always happens; at or under `probability` the command goes out.
export function sendCommand(ai, command, probability) {
  if (command.kind === COMMAND.FOLLOW) {
    const vehicle = ai.vehicles?.boardedVehicleOfCharacter?.(command.source) ?? null;
    if (vehicle) {
      command.kind = COMMAND.BOARD;
      command.target = { unit: vehicle, generation: vehicle.generation };
    }
  }
  const roll = ai.randomB.uniform();
  if (roll <= probability) {
    distributeCommand(ai, command);
    return true;
  }
  return false;
}

const _from = new THREE.Vector3();

// Play the sender's VO and deliver to recipients.
export function distributeCommand(ai, command) {
  const world = ai.world;
  if (world.paused) return [];
  const source = command.source;
  const sender = currentControllable(ai, source);
  if (!sender) return [];
  const vehicles = ai.vehicles;
  if (command.kind === COMMAND.BOARD) {
    const empty = vehicles?.emptySeats?.(sender) ?? 0;
    if (!empty) return [];
  }
  const boarded = vehicles?.boardedVehicleOfCharacter?.(source) ?? null;
  let context = 0;
  if (boarded) context = vehicles?.isMountedTurret?.(sender) ? 3 : 1;
  commandVo(ai, command, context);
  if (ai.secondaryNetworkWorld?.()) return [];
  // AI senders deliver only board/unboard.
  if (!source?.human && (command.kind < COMMAND.BOARD || command.kind > COMMAND.UNBOARD)) return [];
  const delivered = [];
  if (context === 0 || command.kind === COMMAND.BOARD) {
    const radius = command.kind < 2 ? 45 : 30;
    _from.copy(sender.position);
    for (const character of ai.characters()) {
      if (!character || character === source || character.human) continue;
      const control = currentControllable(ai, character);
      if (!control) continue;
      if (!(ai.relation(sender.team, control.team) > 0)) continue;
      if (character.unit?.hero || control.hero) continue;
      const seat = vehicles?.boardedVehicleOfCharacter?.(character) ?? null;
      if (command.kind === COMMAND.BOARD) {
        if (!seat && control.kind === 3) continue;
      } else if (command.kind === COMMAND.FOLLOW && seat) {
        continue;
      }
      const reach = seat ? radius * 3 : radius;
      if (!(_from.distanceToSquared(control.position) <= reach * reach)) continue;
      if (context === 0 && seat && seat.kind === 5) continue;
      const controller = character.controller;
      if (controller?.receiveCommand) {
        controller.receiveCommand(command);
        delivered.push(controller);
      }
    }
    return delivered;
  }
  // A boarded sender's other orders go to the AI in the same vehicle.
  for (const controller of vehicles?.crewControllers?.(boarded, source) ?? []) {
    if (controller?.receiveCommand) {
      controller.receiveCommand(command);
      delivered.push(controller);
    }
  }
  return delivered;
}

// The local player's squad buttons (up move out, down hold, right follow,
// left at ease). Follow names the player's own soldier as the leader.
export function playerCommand(ai, character, kind) {
  const unit = character?.unit;
  if (!unit) return [];
  const target = kind === COMMAND.FOLLOW ? { unit, generation: unit.generation } : null;
  return distributeCommand(ai, new SquadCommand(character, kind, ai.world.time ?? 0, target));
}
