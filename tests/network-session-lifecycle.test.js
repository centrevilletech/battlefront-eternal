import test from 'node:test';
import assert from 'node:assert/strict';
import { NetworkSession } from '../src/runtime/network-session.js';

test('connection disposal notifies its owner after pending work closes, once through reentry', () => {
  const trace = [], reason = Error('closed'), lifecycle = new EventTarget();
  let session;
  session = new NetworkSession({ lifecycle, transport: { dispose() { trace.push('transport'); } },
    onDispose(error) {
      assert.equal(error, reason); assert.equal(session.closed, true);
      assert.equal(session.pending.size, 0); assert.equal(session.entityQueue.length, 0);
      trace.push('owner'); session.dispose(error);
    } });
  session.rejectReady = error => { assert.equal(error, reason); trace.push('ready'); };
  session.entityWaiters.push({ reject(error) { assert.equal(error, reason); trace.push('entity'); } });
  session.pending.set(1, { reject(error) { assert.equal(error, reason); trace.push('request'); } });
  session.entityQueue.push({ tick: 1 });
  session.dispose(reason); session.dispose(); lifecycle.dispatchEvent(new Event('pagehide'));
  assert.deepEqual(trace, ['ready', 'transport', 'entity', 'request', 'owner']);
});

test('a failing transport still releases the owner and retains the original failure', () => {
  const transportError = Error('transport cleanup'), ownerError = Error('owner cleanup');
  let released = 0;
  const session = new NetworkSession({ lifecycle: null, transport: { dispose() { throw transportError; } },
    onDispose() { released++; throw ownerError; } });
  assert.throws(() => session.dispose(), error => error === transportError);
  session.dispose(); assert.equal(released, 1);
});

test('owner cleanup preserves even falsy exceptions from a transport', () => {
  for (const failure of [undefined, null, false, 0, '']) {
    let released = 0, caught = false;
    const session = new NetworkSession({ lifecycle: null, transport: { dispose() { throw failure; } },
      onDispose() { released++; throw Error('later cleanup'); } });
    try { session.dispose(); } catch (error) { caught = true; assert.equal(error, failure); }
    assert.equal(caught, true); assert.equal(released, 1);
  }
});

test('rotation and pagehide dispose the owner without cancelling the next battle prematurely', () => {
  let released = 0, cancelled = 0;
  const lifecycle = new EventTarget(), rotation = { cancelled: false };
  const session = new NetworkSession({ lifecycle, transport: { cancelMatchProbe() { cancelled++; }, dispose() {} }, onDispose() { released++; } });
  session.rotation = rotation;
  session.dispose(Error('next battle'), { preserveRotation: true });
  assert.equal(rotation.cancelled, false); assert.equal(cancelled, 0); assert.equal(released, 1);
  lifecycle.dispatchEvent(new Event('pagehide'));
  assert.equal(rotation.cancelled, true); assert.equal(cancelled, 1); assert.equal(released, 1);
});

// A paused page can receive more parts than its bounded queue holds. It
// discards the oldest replaceable state and accepts the final repair packet.
test('long receive pauses retain the newest repair window without closing',async()=>{
 const delivered=[],session=new NetworkSession({lifecycle:null,onEntities:bytes=>delivered.push(new DataView(bytes.buffer,bytes.byteOffset).getUint32(6,true))});
 session.entityMode=true;
 for(let tick=1;tick<=10000;tick++){
  const bytes=new ArrayBuffer(20),v=new DataView(bytes);v.setUint32(0,0x31534e42,true);v.setUint32(6,tick,true);
  await session.receive('state',bytes);
 }
 assert.equal(session.closed,false);assert.ok(session.entityQueue.length<=4096);assert.ok(session.droppedEntityPackets>0);
 session.drainEntities();assert.equal(delivered.at(-1),10000);assert.equal(session.entityQueue.length,0);session.dispose();
});
