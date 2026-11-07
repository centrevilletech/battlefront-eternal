// Opaque event records and callback identities stay on the host. The
// native journal owns ring indexing, sequence and reader advancement.
import { bindEventJournal } from '../../../sim-events.js';
import { putWorldEventInput, WorldEventStateMirror, syncWorldEventState } from '../../gen/world.js';

const read = ({ source, reader }) => ({ capacity: source.capacity, seq: source.seq, time: source.time,
  cursor: reader?.cursor ?? 0, dropped: reader?.dropped ?? 0 });
function write({ source, reader }, state) {
  source.capacity = state.capacity; source.seq = state.seq; source.time = state.time;
  if (reader) { reader.cursor = state.cursor; reader.dropped = state.dropped; }
}

export function eventPorts(bridge) {
  const x = bridge.x, contexts = [], owned = new WeakSet();
  const input = { words: x.world_events_words(0), put: putWorldEventInput };
  const output = { Mirror: WorldEventStateMirror, sync: syncWorldEventState };
  bridge.register({ worldEvents(args) {
    const context = contexts.at(-1), op = args.op, index = args.index;
    write(context, args.state);
    const { source, fn } = context;
    let present = false;
    if (op === 'write') source.ring[index] = context.record;
    else if (op === 'notify') {
      const record = context.record;
      for (const listener of source.listeners) listener(record);
    } else if (op === 'clear') source.ring.fill(null);
    else if (op === 'load') { context.record = source.ring[index]; present = Boolean(context.record); }
    else if (op === 'deliver') { const record = context.record; fn(record); }
    return { state: read(context), present };
  } });
  function events(source) {
    if (owned.has(source)) return source;
    function run(op, reader = null, record = null, fn = null) {
      const context = { source, reader, record, fn };
      contexts.push(context);
      try {
        bridge.callArgs(input, { op, state: read(context) });
        const out = bridge.callResult(output, x.world_events_run());
        write(context, out);
      } finally { contexts.pop(); }
    }
    bindEventJournal(source, {
      emit(record) { run('emit', null, record); return record; },
      reset() { run('reset'); },
      read(reader, fn) { run('read', reader, null, fn); },
    });
    owned.add(source); return source;
  }
  return { events, ownsEvents: source => owned.has(source) };
}
