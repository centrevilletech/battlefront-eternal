import test from 'node:test';
import assert from 'node:assert/strict';
import {createPageNetwork} from '../src/runtime/page-network.js';
import {NetOutboundBatch as B} from '../src/runtime/network-layout.js';
// The batch is one transaction, including a malformed later command or a
// trailing word after otherwise valid commands. The live WASM export is used.
test('outbound batches validate all commands before changing accounting',()=>{
 const network=createPageNetwork(),id=network.kernel.outbound_new(),valid=[1,1,1,1,0,1,.25,-.5,1,21];
 const publish=rows=>{network.begin();for(const words of rows){const prefix=new Float64Array(B.STRIDE);prefix[B.WORD_COUNT]=words.length;prefix[B.MOUSE_SCALE]=1;for(const word of prefix)network.put(word);for(const word of words)network.put(word);}};
 try{publish([valid]);network.outboundBatch(id,1);const before=network.kernel.outbound_capture(id);
 publish([valid,[2,2,2,2,0,1,100]]);assert.throws(()=>network.outboundBatch(id,2),/invalid/);assert.equal(network.kernel.outbound_capture(id),before);
 publish([valid]);network.put(123);assert.throws(()=>network.outboundBatch(id,1),/invalid/);assert.equal(network.kernel.outbound_capture(id),before);
 publish([valid]);network.outboundBatch(id,1);assert.notEqual(network.kernel.outbound_capture(id),before);
 }finally{network.kernel.outbound_drop(id);network.dispose();}
});
