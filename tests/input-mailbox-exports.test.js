import test from 'node:test';
import assert from 'node:assert/strict';
import {loadSim} from '../src/sim/module.js';
test('simulation exposes only retained mailbox execution',async()=>{
 const {exports:x}=await loadSim();
 assert.equal(typeof x.input_mailbox_owned_run,'function');
 assert.equal(x.input_mailbox_run,undefined);assert.equal(x.input_mailbox_words,undefined);
});
