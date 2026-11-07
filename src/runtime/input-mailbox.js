// InputMailbox's native owner keeps pending commands and all consumption
// watermarks. Object references below belong to the host's input producers;
// they never carry a serialized copy of the retained native queue.
import { validateMailboxCommand } from './transport.js';
import { publicationView } from './input-view.js';
import { InputMailboxCallMirror, InputMailboxCallLayout, ImEntryMirror, ImResultMirror, ImStateMirror, InputViewUpdateMirror,
  putInputMailboxCall, putInputViewUpdate, syncImResult, syncImState, InputViewUpdateLayout, WireSpawnFactsMirror, WireSpawnFactsLayout, putWireSpawnFacts } from '../sim/gen/index.js';
const keys = ['mouseScale','invertMouse','firstPerson','aspect','viewTick','flightSettings','gamepad'];
const result = { Mirror: ImResultMirror, sync: syncImResult }, inspected = { Mirror: ImStateMirror, sync: syncImState };
export class InputMailbox {
  constructor(epoch = 1, capacity = 256, bridge = null) {
    if (!bridge) throw Error('InputMailbox requires a battle simulation bridge');
    this.bridge = bridge; this.handle = this.bridge.x.input_mailbox_create(epoch, capacity);
    if (!this.handle) throw Error('input mailbox admission failed');
    this.epoch = epoch; this.capacity = capacity; this.received = this.ack = this.ackTick = this.discarded = this.generation = this.late = this.stateTick = 0;
    this.emptyDown = []; this.down = this.emptyDown; this.lookTotal = [0,0]; this.references = new Map(); this.serial = 0; this.tokens = new Map();
    this.values = {}; this.update = new InputViewUpdateMirror();
    this.view = new Proxy(this.values, {
      set: (object, key, value) => { object[key] = value; this.changedView(key, true); return true; },
      deleteProperty: (object, key) => { delete object[key]; this.changedView(key, false); return true; },
    });
    this.change = new InputMailboxCallMirror(); this.entry = new ImEntryMirror(); this.exchange = new ImResultMirror();
    this.input = { words: InputMailboxCallLayout.words, put: putInputMailboxCall };
    this.viewInput = { words: InputViewUpdateLayout.words, put: putInputViewUpdate };
  }
  changedView(key, present) {
    const index = keys.indexOf(key); if (index < 0 || !this.handle) return;
    this.update.key = index; this.update.present = present; publicationView(this.values, this.update.view);
    this.bridge.callArgs(this.viewInput, this.update); this.bridge.x.input_mailbox_view(this.handle);
  }
  run(op, tick = 0, generation = 0) {
    const c = this.change; c.op = op; c.tick = tick; c.generation = generation;
    this.bridge.callArgs(this.input, c);
    const out = this.bridge.callResult(result, this.bridge.x.input_mailbox_owned_run(this.handle), this.exchange), s = out.state;
    this.received = s.received; this.ack = s.ack; this.ackTick = s.ackTick; this.discarded = s.discarded; this.generation = s.generation; this.late = s.late; this.stateTick = s.stateTick;
    this.down = s.down === null ? this.emptyDown : this.references.get(s.down).command.down;
    if (out.replacedLookTotal) this.lookTotal = [0,0];
    this.lookTotal[0] = s.lookTotal[0]; this.lookTotal[1] = s.lookTotal[1];
    for (let i=0;i<keys.length;i++) {
      const key=keys[i];let present=false;
      for (let j=0;j<s.view.length;j++) if (s.view[j].key===key) {present=true;break;}
      if (!present) {delete this.values[key];this.tokens.delete(key);}
    }
    for (const field of s.view) if (this.tokens.get(field.key) !== field.value) {
      const row = this.references.get(Math.floor(field.value / 8));
      this.values[field.key] = row.command[field.key]; this.tokens.set(field.key, field.value);
    }
    for (const id of out.retired) { const row = this.references.get(id); if (row) row.pending = false; }
    if (op === 'clear') for (const row of this.references.values()) row.pending = false;
    if (out.error !== null) throw Error(out.error);
    return out;
  }
  cleanup() {
    for (const [id, row] of this.references) if (!row.pending && row.command.down !== this.down) {
      let held = false; for (const token of this.tokens.values()) if (Math.floor(token / 8) === id) { held = true; break; }
      if (!held) this.references.delete(id);
    }
  }
  push(command) {
    const c = this.change; c.entry = null; c.sequence = command.sequence; c.epochMatches = command.epoch === this.epoch;
    if (!this.run('admit').accepted) return false;
    const human = validateMailboxCommand(command), id = ++this.serial, row = human.length ? { ...command, human } : command;
    const e = this.entry; e.identity = id; e.sequence = command.sequence; e.tick = command.tick; e.generation = command.generation;
    e.down = id; e.codes = command.down; e.actions = command.actions; e.lookTotal = command.lookTotal ?? null;
    e.look = command.look.map((point, i) => ({ point, object: id * 2048 + i }));
    e.human = human.map((_, i) => id * 16 + i); e.fire = command.fire ?? [];
    e.view.length = 0;
    for (let i = 0; i < keys.length; i++) if (command[keys[i]] !== undefined) e.view.push({ key: keys[i], value: id * 8 + i });
    publicationView(command, e.publication); c.entry = e;
    this.references.set(id, { command: row, pending: true });
    try { const accepted=this.run('push').accepted; if (!accepted) this.references.delete(id); return accepted; }
    catch (error) { this.references.delete(id); throw error; }
    finally { c.entry = null; this.cleanup(); }
  }
  take(tick, generation) {
    const out = this.run('take', tick, generation), taken = out.command;
    const look = taken.look.map(row => row.object === null ? row.point.slice() : this.references.get(Math.floor(row.object / 2048)).command.look[row.object % 2048]);
    const human = taken.human.map(token => this.references.get(Math.floor(token / 16)).command.human[token % 16]);
    const command = { epoch: this.epoch, sequence: tick, tick, generation, down: this.down, actions: taken.actions.slice(), look, ...this.values };
    if (human.length) command.human = human;
    if (taken.fire.length) command.fire = taken.fire.map(edge => ({ id: edge.id, button: edge.button, pressed: edge.pressed }));
    this.cleanup(); return command;
  }
  clear() { this.run('clear'); this.cleanup(); }
  get queue() { return this.bridge.callResult(inspected, this.bridge.x.input_mailbox_inspect(this.handle)).queue.map(row => this.references.get(row.identity).command); }
  dispose() { if (this.handle) { this.bridge.x.input_mailbox_drop(this.handle); this.handle = 0; this.references.clear(); } }
}

// Spawn requests belong to the connection, even when no snapshot is due.
// Keep their protocol projection at mutation points; content registration is
// cold and stable for this connection's lifetime.
const selections = new WeakMap();
export function bindSpawnSelection(actor, registry) {
  const previous = selections.get(actor);
  if (previous?.mailbox === actor.mailbox) return;
  const mailbox = actor.mailbox, row = new WireSpawnFactsMirror();
  const input = { words: WireSpawnFactsLayout.words, put: putWireSpawnFacts };
  function changed(value) {
    const pending = value.pending, auto = value.auto;
    row.pending = !!pending; row.auto = !!auto;
    row.pendingClass = pending ? registry.classIds.get(pending.className) : 0;
    row.pendingPost = pending?.post == null ? 0 : registry.postIds.get(pending.post);
    row.autoTeam = auto ? auto.team : 0;
    row.autoClass = auto ? registry.classIds.get(auto.className) : 0;
    row.autoPost = auto?.post == null ? 0 : registry.postIds.get(auto.post);
    row.valid = row.pendingClass !== undefined && row.pendingPost !== undefined && row.autoClass !== undefined && row.autoPost !== undefined;
    mailbox.bridge.callArgs(input, row); mailbox.bridge.x.input_mailbox_spawn(mailbox.handle);
  }
  const value = previous?.value ?? actor.spawn;
  actor.spawn = new Proxy(value, { set(target, key, value) {
    target[key] = value;
    if (key === 'pending' || key === 'auto') changed(target);
    return true;
  } });
  selections.set(actor, {mailbox,value}); changed(value);
}
