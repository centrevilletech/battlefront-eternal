// The edge queue owns its state in the shared module. World mailbox calls
// run between native operations, with owned edge values and no live borrow.
import { bridgeFor } from '../../runtime.js';
import { WeResultMirror, putWeCall, syncWeResult } from '../../gen/index.js';

const output = { Mirror: WeResultMirror, sync: syncWeResult };
const numeric = v => typeof v === 'number' ? v : NaN;
const edgeValue = e => ({ ...e, epoch: numeric(e.epoch), generation: numeric(e.generation), id: numeric(e.id), tick: numeric(e.tick), kind: typeof e.kind === 'string' ? e.kind : '' });
const edgeCopy = value => Object.fromEntries(Object.entries(value).filter(([, value]) => value !== undefined));
const outcome = value => ({ id: value.id, status: value.status, tick: value.tick });

export class WireEntityEdges {
  constructor({ instance, bridge = instance ? bridgeFor(instance) : null, actor, world, client, registry, capacity = 256 }) {
    this.bridge = bridge; this.actor = actor; this.world = world; this.client = client; this.registry = registry; this.capacity = capacity;
    this.handle = bridge.x.wire_edges_create();
    this.input = { words: bridge.x.wire_edges_words(), put: putWeCall };
    this.dropped = { edges: 0, results: 0 };
    actor.actions.enableOutcomes(); actor.fire.enableOutcomes(); this.reset(1, actor.unit?.generation ?? 0);
  }
  run(op, args = {}) {
    if (!this.handle) throw Error('edge runtime disposed');
    this.bridge.callArgs(this.input, { op, epoch: 0, generation: 0, tick: 0, capacity: this.capacity,
      actionReceived: 0, fireReceived: 0, edges: [], ids: [], edge: null, outcomes: [], fire: false,
      id: 0, externalId: 0, accepted: false, status: '', resultTick: 0, ...args });
    const out = this.bridge.callResult(output, this.bridge.x.wire_edges_run(this.handle));
    this.epoch = out.epoch; this.generation = out.generation; this.tick = out.tick;
    this.actionSerial = out.actionSerial; this.fireSerial = out.fireSerial;
    this.dropped.edges = out.droppedEdges; this.dropped.results = out.droppedResults;
    return out;
  }
  reset(epoch, generation) { this.run(0, { epoch, generation, actionReceived: this.actor.actions.received, fireReceived: this.actor.fire.received }); }
  result(id, status, tick = this.tick) { this.run(9, { id, status, resultTick: tick }); }
  receive(packet, tick) {
    this.run(1, { epoch: numeric(packet.epoch), generation: numeric(packet.generation), tick, edges: packet.edges.map(edgeValue) });
  }
  advance(tick) {
    const edges = this.run(2, { tick }).edges.map(edgeCopy);
    for (const edge of edges) {
      const dispatch = this.run(3, { edge, tick, actionReceived: this.actor.actions.received, fireReceived: this.actor.fire.received });
      if (!dispatch.accepted) continue;
      const id = dispatch.id;
      let accepted;
      if (dispatch.fire) {
        accepted = this.world.fire(this.client, { ...edge, id, externalId: edge.id, button: edge.button === 'fireSecondary' ? 1 : 0 });
      } else {
        const actions = [], human = [];
        if (edge.kind === 'action') actions.push(edge.action);
        else if (edge.kind === 'spawn') {
          const className = this.registry.classes.get(edge.classId), post = edge.postId ? this.registry.posts?.get(edge.postId) : null;
          if (!className || edge.postId && !post) { this.result(edge.id, 'rejected'); continue; }
          human.push({ kind: 'spawn', className, post });
        } else human.push({ kind: edge.kind, ...(edge.kind === 'team' ? { team: edge.team } : {}) });
        accepted = this.world.actions(this.client, { epoch: this.epoch, generation: this.generation, id, tick, actions, human });
      }
      this.run(4, { fire: dispatch.fire, id, externalId: edge.id, accepted: !!accepted });
    }
  }
  consumed(tick = this.tick) {
    this.run(5, { tick });
    for (const lane of ['actions', 'fire']) {
      const outcomes = this.actor[lane].drainOutcomes().map(outcome);
      this.run(6, { fire: lane === 'fire', outcomes });
    }
  }
  acknowledge({ epoch, generation, ids }) { this.run(7, { epoch, generation, ids }); }
  feedback() {
    const out = this.run(8);
    return { epoch: out.epoch, generation: out.generation, received: out.received, results: out.results.map(outcome) };
  }
  get pending() { return new Map(this.run(10).pending.map(p => [`${p.fire ? 'fire' : 'actions'}:${p.id}`, p.externalId])); }
  get results() { return new Map(this.run(10).results.map(r => [r.id, outcome(r)])); }
  get received() { return this.run(8).received; }
  get receiver() {
    const out = this.run(10);
    return { queue: new Map(out.edges.map(e => [e.id, edgeCopy(e)])), seen: new Map(out.seen.map(id => [id, true])), highest: out.highest };
  }
  dispose() { if (this.handle) { this.bridge.x.wire_edges_drop(this.handle); this.handle = 0; } }
}
