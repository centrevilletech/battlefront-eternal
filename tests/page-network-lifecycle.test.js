import test from 'node:test';
import assert from 'node:assert/strict';
import {pageNetworkFor,createPageNetwork} from '../src/runtime/page-network.js';
import {wireInputFor} from '../src/runtime/wire-input.js';
import {NetworkSession} from '../src/runtime/network-session.js';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES} from '../src/runtime/network-entity-input.js';
const registry=()=>({inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES});

test('session disposal closes the last registry sender and leaves shared senders live',()=>{
  const r=registry(),owner=pageNetworkFor(r),session=new NetworkSession({transport:{dispose(){}}});
  const sender=()=>new EntityInputSender({registry:r,send:()=>true});
  session.entityInput=sender();const other=sender();session.dispose();
  assert.equal(owner.disposed,false);other.dispose();assert.equal(owner.disposed,true);
  session.dispose();other.dispose();
  const next=pageNetworkFor(r);assert.notEqual(next,owner);next.dispose();
});

test('keepalive encoding reserves one page kernel across battle registries',()=>{
  const first=wireInputFor(registry()),kernel=first.kernel;
  for(let battle=0;battle<32;battle++){
    const r=registry(),encoder=wireInputFor(r),owner=pageNetworkFor(r);
    assert.equal(encoder.kernel,kernel);assert.notEqual(owner,kernel);
    assert.equal(encoder.keepalive({epoch:1,tick:battle}).length,10);owner.dispose();
  }
  assert.equal(kernel.disposed,false);
});

test('closed receivers never supply an encode-only kernel and repeated battles reuse receivers',()=>{
  const bounds={xMin:0,xExtent:1,zMin:0,zExtent:1,yMin:0,yExtent:1};
  const first=createPageNetwork();first.receiveSetup(bounds);const retained=first.kernel;first.dispose();
  const encoder=createPageNetwork();assert.notEqual(encoder.kernel,retained);
  const second=createPageNetwork();second.receiveSetup(bounds);assert.equal(second.kernel,retained);
  first.dispose();assert.equal(second.disposed,false);encoder.dispose();second.dispose();
});
