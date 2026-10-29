import { HOLDS, WORLD_AXES } from "../input.js";
import { ActionQueue } from './action-queue.js';

// The input clock that stamps commands lives in reconcile.js (advanceInputClock).

// The spawn screen's choices travel as world commands (command.human), in
// the order they were made, so a worker and a replay apply the same ones on
// the same tick:
//   { kind: 'team', team }               move to team 1..7 (while he has no unit)
//   { kind: 'spawn', className, post }   spawn as the class at the post (a
//                                        post name, or null for the world's pick)
//   { kind: 'suicide' }                  the pause menu's RESPAWN: a normal death
//   { kind: 'cancel' }                   withdraw a spawn choice not yet carried
//                                        out (the screen's BACK)
// humanEntry returns a clean copy of one, or null if it is malformed.
export const HUMAN_COMMAND_LIMIT = 16;
const NAME_LIMIT = 64;
export function humanEntry(entry) {
  if (!entry || typeof entry !== 'object') return null;
  if (entry.kind === 'team') return Number.isInteger(entry.team) && entry.team >= 1 && entry.team <= 7 ? { kind: 'team', team: entry.team } : null;
  if (entry.kind === 'spawn') {
    const { className, post = null } = entry;
    if (typeof className !== 'string' || !className || className.length > NAME_LIMIT) return null;
    if (post !== null && (typeof post !== 'string' || !post || post.length > NAME_LIMIT)) return null;
    return { kind: 'spawn', className, post };
  }
  if (entry.kind === 'suicide') return { kind: 'suicide' };
  if (entry.kind === 'cancel') return { kind: 'cancel' };
  return null;
}
// Queues a choice for the next command, kept small whatever arrives before
// it is sent (a burst of clicks or key repeats over a stalled frame): one of
// each kind, a team, spawn or cancel at its last place and a suicide at its
// first (a second one does nothing), so applying the queue does what
// applying everything would have. False if the choice is malformed.
export function queueHumanCommand(queue, entry) {
  const clean = humanEntry(entry);
  if (!clean) return false;
  const same = queue.findIndex((queued) => queued.kind === clean.kind);
  if (same >= 0) {
    if (clean.kind === 'suicide') return true;
    queue.splice(same, 1);
  }
  queue.push(clean);
  return true;
}
// A command's whole list, cleaned; throws on anything malformed.
export function humanCommands(list) {
  if (list === undefined) return [];
  if (!Array.isArray(list) || list.length > HUMAN_COMMAND_LIMIT) throw new Error('invalid human input');
  return list.map((entry) => {
    const clean = humanEntry(entry);
    if (!clean) throw new Error('invalid human input');
    return clean;
  });
}

// His command while the pause menu holds his input in a network game, where
// the battle plays on (src/game.js): no keys, presses or look, and the pad
// centred. Centred, not left out: a command without a pad is "no pad in this
// packet" on the wire, and the server's mailbox keeps the last pad it had
// (InputMailbox.take), so a stick held into the menu would go on flying.
// Spawn choices and RESPAWN (command.human) still go.
export function stillCommand(command) {
  return Object.assign(command, { down: [], actions: [], look: [], gamepad: { axes: [0, 0, 0, 0], buttons: [] } });
}

// Fixed-size replay history. Missing history is explicit: reconciliation must
// hard-correct rather than inventing commands or replaying overwritten slots.
export class CommandHistory {
  constructor(capacity = 256) {
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error('invalid command capacity');
    this.capacity = capacity; this.slots = new Array(capacity); this.reset(1);
  }
  reset(epoch) { this.epoch = epoch; this.latest = 0; this.slots.fill(null); }
  put(command) {
    if (command.epoch !== this.epoch || !Number.isSafeInteger(command.sequence) || command.sequence <= this.latest) return false;
    this.slots[command.sequence % this.capacity] = command; this.latest = command.sequence; return true;
  }
  get(sequence) {
    const value = this.slots[sequence % this.capacity];
    return value?.sequence === sequence && value.epoch === this.epoch ? value : null;
  }
  after(sequence) {
    const result = [];
    for (let i = sequence + 1; i <= this.latest; i++) {
      const command = this.get(i);
      if (!command) return null;
      result.push(command);
    }
    return result;
  }
}

// DOM-free adapter shared by simulation hosts and command replays. It reads
// the tables' keys: a command's held keys are already the world's
// (Controls.command), whatever the player bound.
export class CommandControls {
  constructor() { this.down = new Set(); this.queued = new ActionQueue(); this.epoch = 0; this.generation = 0; this.sequence = 0; }
  accept(command) {
    if (command.epoch < this.epoch || (command.epoch === this.epoch && (command.sequence <= this.sequence || command.generation < this.generation))) return false;
    if (command.epoch !== this.epoch || command.generation !== this.generation) this.clear();
    this.epoch = command.epoch; this.generation = command.generation; this.sequence = command.sequence;
    this.down.clear();
    for (const key of command.down) this.down.add(key);
    for (const action of command.actions) this.queued.add(action);
    return true;
  }
  take(name) { return this.queued.delete(name); }
  // flightScheme(): the key map of the seat held when asked (src/boarding.js
  // seatScheme), never one kept from before a seat change.
  held(name) { return (this.flightScheme?.()?.[name] || HOLDS[name] || []).some(code => this.down.has(code)); }
  axis(name) {
    const [plus, minus] = WORLD_AXES[name] || [[], []];
    return Number(plus.some(code => this.down.has(code))) - Number(minus.some(code => this.down.has(code)));
  }
  clear() { this.down.clear(); this.queued.clear(); }
}
