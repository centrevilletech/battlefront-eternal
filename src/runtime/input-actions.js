import { humanCommands } from './commands.js';

// This reliable lane has its own sequence watermark. A newer replaceable
// movement packet must not discard a delayed jump, reload or spawn choice.
export class InputActions {
  constructor(epoch = 1, capacity = 128) {
    this.epoch = epoch; this.capacity = capacity;
    this.received = this.ack = this.discarded = 0; this.queue = []; this.outcomes = null;
  }
  enableOutcomes() { this.outcomes ||= []; return this; }
  drainOutcomes() { return this.outcomes?.splice(0) || []; }
  outcome(id,status) { if(this.outcomes){if(this.outcomes.length>=this.capacity*2)throw Error('action outcome backlog exceeded');this.outcomes.push({id,status});} }
  push(message, tick, generation) {
    if (!message || message.epoch !== this.epoch || !Number.isSafeInteger(message.id) || message.id <= this.received ||
      !Number.isSafeInteger(message.tick) || message.tick < 1 || message.tick > tick + 120 ||
      message.generation !== generation) return false;
    if (!Array.isArray(message.actions) || message.actions.length > 64 ||
      !message.actions.every(action => typeof action === 'string' && action.length <= 64 && !['firePrimary', 'fireSecondary'].includes(action))) throw Error('invalid reliable actions');
    const human = humanCommands(message.human);
    // A full queue refuses the press, as FireCommands refuses a click: a
    // spun mouse wheel sends weapon changes faster than one a tick drains,
    // and refusing them must not end the player's session.
    if (this.queue.length >= this.capacity) return false;
    if (this.queue.length && message.tick < this.queue.at(-1).tick) throw Error('action tick moved backwards');
    this.received = message.id;
    this.queue.push({ id: message.id, tick: message.tick, generation, actions: [...message.actions], human });
    return true;
  }
  take(tick, generation) {
    while (this.queue.length) {
      const entry = this.queue[0];
      if (entry.generation !== generation || entry.tick < tick - 120) {
        this.outcome(entry.id,entry.generation!==generation?'life-cancelled':'expired');this.discarded = Math.max(this.discarded,entry.id); this.queue.shift(); continue;
      }
      if (entry.tick > tick) break;
      // Repeated presses in a single desktop frame remain separate ticks.
      const actions = [...new Set(entry.actions)], human = entry.human;
      for (const action of actions) entry.actions.splice(entry.actions.indexOf(action), 1);
      entry.human = [];
      if (!entry.actions.length) { this.outcome(entry.id,'consumed'); this.ack = entry.id; this.queue.shift(); }
      return { actions, human };
    }
    return { actions: [], human: [] };
  }
  clear() { for(const entry of this.queue)this.outcome(entry.id,'cleared');this.discarded = Math.max(this.discarded,this.queue.at(-1)?.id || 0); this.queue.length = 0; }
}
