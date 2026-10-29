import test from 'node:test';
import assert from 'node:assert/strict';
import { BinaryWriter, BinaryReader } from '../src/runtime/packet.js';
import { PacketBacklog } from '../src/runtime/packet-backlog.js';
function fixture(options = {}) {
  const writer = new BinaryWriter(), reader = new BinaryReader(); let resyncs = 0;
  const queue = new PacketBacklog({ read: (buffer, accept) => reader.read(buffer, 0, accept), resync: () => resyncs++, ...options });
  function packet(tick, baseTick, keyframe = false, text = `value-${tick}`, epoch = null) {
    writer.begin(); if (keyframe) writer.keyframe(); writer.value(text);
    return writer.finish({ tick, baseTick, keyframe, ...(epoch===null?{}:{snapshotAck:{epoch,tick}}) });
  }
  return { queue, reader, packet, resyncs: () => resyncs };
}
test('stalled renderer bounds its queue and resumes only from complete keyframe tables', () => {
  const f = fixture({ maxPackets: 2 });
  f.queue.receive(f.packet(3, null, true));
  f.queue.receive(f.packet(6, 3));
  assert.equal(f.queue.receive(f.packet(9, 6)), null);
  assert.equal(f.queue.packets.length, 0); assert.equal(f.resyncs(), 1);
  assert.equal(f.queue.receive(f.packet(12, 9)), null); assert.equal(f.resyncs(), 1);
  const latest = f.queue.receive(f.packet(15, 12, true, 'recovered'));
  assert.equal(latest.reader.strings[latest.bin.u16[0]], 'recovered');
  assert.equal(f.queue.packets.length, 1); assert.equal(f.queue.awaitKeyframe, false);
  f.queue.shift(); assert.equal(f.queue.bytes, 0);
});
test('lost, reordered and duplicate packets cannot mutate the active binary tables', () => {
  const f = fixture();
  const first = f.packet(3, null, true), missed = f.packet(6, 3), after = f.packet(9, 6);
  f.queue.receive(first);
  const before = [...f.reader.strings];
  assert.equal(f.queue.receive(first), null);
  assert.deepEqual(f.reader.strings, before);
  assert.equal(f.queue.receive(after), null);
  assert.equal(f.queue.receive(missed), null);
  assert.deepEqual(f.reader.strings, before);
  assert.equal(f.resyncs(), 1);
  f.queue.receive(f.packet(12, 9, true));
  assert.equal(f.queue.shift().tick, 12);
});
test('tick age and byte pressure recover without retaining old world state', () => {
  for (const options of [{ maxTicks: 4 }, { maxBytes: 1 }]) {
    const f = fixture(options);
    f.queue.receive(f.packet(3, null, true));
    f.queue.receive(f.packet(9, 3));
    assert.equal(f.queue.packets.length, 0); assert.equal(f.queue.awaitKeyframe, true);
    assert.equal(f.resyncs(), 1);
  }
});


test('a new journal epoch replaces same-tick queued state only through a full keyframe',()=>{
 const f=fixture();f.queue.receive(f.packet(3,null,true,'old',1));
 const current=f.queue.receive(f.packet(3,null,true,'new',2));
 assert.equal(current.tick,3);assert.equal(f.queue.packets.length,1);assert.equal(f.queue.epoch,2);
 const strings=[...f.reader.strings];
 assert.equal(f.queue.receive(f.packet(6,3,false,'stale',1)),null);assert.deepEqual(f.reader.strings,strings);
 assert.equal(f.queue.receive(f.packet(6,3,false,'bad-new',3)),null);assert.equal(f.queue.awaitKeyframe,true);
 f.queue.receive(f.packet(6,null,true,'next',3));assert.equal(f.queue.epoch,3);
});
