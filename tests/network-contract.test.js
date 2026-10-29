import test from 'node:test';
import assert from 'node:assert/strict';
import { NETWORK_CONTRACT } from '../src/runtime/network-contract.js';
import { encodeEntities, emptySnapshot } from '../src/runtime/snapshot-codec.js';
import { OWN_PLAYER_SCHEMA } from '../src/runtime/network-entity-player.js';
import { UdpTransport } from '../src/runtime/udp-transport.js';

test('mounted engine manifest agrees with its real wire and replay adapter boundaries', () => {
  assert.equal(NETWORK_CONTRACT.serverAdapterABI,4);
  assert.equal(NETWORK_CONTRACT.simulation,'bfe-sim-27');
  assert.equal(NETWORK_CONTRACT.ownCorrectionSchema,OWN_PLAYER_SCHEMA);
  assert.equal(NETWORK_CONTRACT.profile,'snapshot-delta-v1');
  assert.ok(Object.isFrozen(NETWORK_CONTRACT));
  const bytes=encodeEntities({...emptySnapshot(1234,1)},null)[0];
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  assert.equal(view.getUint32(0,true),0x31534e42);
  assert.equal(view.getUint32(6,true),1234);
  assert.ok(bytes.length<=1100);
});

test('client greeting carries the same required manifest the server checks', async () => {
  globalThis.fetch ??= async () => new Response(null, { status: 404 });
  const posted=[],port={postMessage:message=>posted.push(message),start(){},close(){}};
  const transport=new UdpTransport({bridge:{openSession(){},request:async()=>{throw Error('no master for JOIN IP');}},openPort:async()=>port});
  const connecting=transport.connect({query:'?server='+'a'.repeat(64)+'&address=127.0.0.1&port=18451',content:'content',playerName:'Josh',channels:new Map(),receive(){},fail(){},status(){}});
  await new Promise(resolve=>setTimeout(resolve,10));
  const connect=posted.find(message=>message.op==='connect');
  for(const field of ['simulation','protocol','profile'])assert.equal(connect.hello[field],NETWORK_CONTRACT[field]);
  assert.equal(NETWORK_CONTRACT.protocol,3,'protocol 3 is the native UDP transport');
  assert.deepEqual(connect.hello.name,'Josh');
  transport.message({op:'error',for:'connected',message:'stop'});
  await assert.rejects(connecting,/stop/);
  transport.dispose();
});
