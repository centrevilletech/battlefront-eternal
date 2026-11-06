import test from 'node:test';
import assert from 'node:assert/strict';
import { ClassicRandom, RandomStreams, uniformRandom, randomBinding } from '../src/ai/random.js';
import { F3Random } from '../src/f3-random.js';
import { Heap } from '../src/sim/runtime.js';
import { F3RandomEpochLayout, F3RandomStateMirror, putF3RandomState, syncF3RandomState } from '../src/sim/gen/index.js';

// This fixture owns only the declared numeric stream block. Its draw function
// stands in for the native ClassicRandom whose arithmetic has native fixtures.
function boundary() {
  const counters={inputFields:0,outputFields:0};
  const bridge={f:new Float64Array(1024),heap:new Heap(),views(){}};
  const words=[];putF3RandomState(bridge,new F3RandomStateMirror(),words,0,bridge.heap);
  const stride=words.length;
  const phase={bridge,counters,layout:{randomEpoch:0,randomStates:F3RandomEpochLayout.words*8,randomWords:stride,randomCapacity:128}};
  const random=new F3Random(phase);
  function nativeDraw(id) {
    bridge.f[F3RandomEpochLayout.revision]=(bridge.f[F3RandomEpochLayout.revision]+1)>>>0;
    const at=F3RandomEpochLayout.words+id*stride, state=new F3RandomStateMirror();
    syncF3RandomState(bridge,bridge.f,at,0,state);
    if (!state.present) { state.present=true;state.sequence=++nativeDraw.sequence; }
    const generator=new ClassicRandom(state.state),value=generator.uniform();state.state=generator.state;
    putF3RandomState(bridge,state,bridge.f,at,bridge.heap);
    return value;
  }
  nativeDraw.sequence=0;
  return {random,nativeDraw,counters};
}

test('uniform callbacks retain an explicit shared generator binding',()=>{
  const random=new ClassicRandom(7),draw=uniformRandom(random),reference=new ClassicRandom(7);
  assert.equal(uniformRandom(random),draw);assert.equal(randomBinding(draw).random,random);
  assert.equal(draw(),reference.uniform());assert.equal(random.state,reference.state);
});

test('native named draws preserve lazy first-use order across an AI boundary',()=>{
  const streams=new RandomStreams(23),reference=new RandomStreams(23);
  const {random,nativeDraw}=boundary();
  const gameplay=random.register(streams.draw('gameplay')),spin=random.register(streams.draw('grenade-spin'));
  assert.equal(streams.streams.size,0);
  random.push();assert.equal(nativeDraw(spin),reference.get('grenade-spin').uniform());
  assert.equal(nativeDraw(gameplay),reference.get('gameplay').uniform());random.pull();
  assert.deepEqual([...streams.streams.keys()],['grenade-spin','gameplay']);
  assert.equal(streams.draw('gameplay')(),reference.get('gameplay').uniform());
  random.push();assert.equal(nativeDraw(gameplay),reference.get('gameplay').uniform());random.pull();
  assert.equal(streams.get('gameplay').state,reference.get('gameplay').state);
  assert.equal(streams.get('grenade-spin').state,reference.get('grenade-spin').state);
});

test('a named stream and the AI generator retain independent identities',()=>{
  const streams=new RandomStreams(0),ai=new ClassicRandom(0),reference=new ClassicRandom(0);
  const {random,nativeDraw}=boundary();
  const shared=random.register(uniformRandom(ai));
  assert.equal(random.register(uniformRandom(ai)),shared);
  const named=random.register(streams.draw('gameplay'));assert.notEqual(named,shared);
  random.push();assert.equal(nativeDraw(shared),reference.uniform());random.pull();
  assert.equal(ai.state,reference.state);assert.equal(streams.streams.size,0);
});

test('unchanged random boundaries retain words until a JS draw advances a stream',()=>{
  const streams=new RandomStreams(23),reference=new RandomStreams(23);
  const {random,nativeDraw,counters}=boundary();
  const draw=streams.draw('gameplay'),id=random.register(draw);
  random.push();nativeDraw(id);reference.get('gameplay').uniform();random.pull();
  counters.inputFields=0;
  random.push();random.push();
  assert.equal(counters.inputFields,0);
  assert.equal(draw(),reference.get('gameplay').uniform());
  random.push();assert.ok(counters.inputFields>0);
  assert.equal(nativeDraw(id),reference.get('gameplay').uniform());random.pull();
  assert.equal(draw(),reference.get('gameplay').uniform());
});

test('direct first use and reset refresh the generator held by an existing boundary',()=>{
  const streams=new RandomStreams(13),reference=new RandomStreams(13);
  const draw=streams.draw('gameplay'),binding=randomBinding(draw);
  const {random,nativeDraw}=boundary();
  const id=random.register(draw);
  assert.equal(binding.random,null);
  assert.equal(streams.get('gameplay').uniform(),reference.get('gameplay').uniform());
  assert.ok(binding.random===streams.get('gameplay'));
  random.push();assert.equal(nativeDraw(id),reference.get('gameplay').uniform());random.pull();
  const prior=binding.random;
  streams.reset(31);reference.reset(31);
  assert.equal(binding.random,null);assert.equal(streams.streams.size,0);
  random.push();assert.equal(nativeDraw(id),reference.get('gameplay').uniform());random.pull();
  assert.ok(binding.random!==prior);assert.ok(binding.random===streams.get('gameplay'));
  assert.equal(draw(),reference.draw('gameplay')());
  streams.reset();reference.reset();
  assert.equal(draw(),reference.draw('gameplay')());
  assert.equal(binding.random.state,reference.get('gameplay').state);
});

// Aimers can observe the same state repeatedly without a weapon firing.
// The epoch check reads one scalar and leaves all stream objects untouched.
test('idle native boundaries read only their shared random epoch',()=>{
  const {random,nativeDraw,counters}=boundary(),reference=new ClassicRandom(19);
  const generator=new ClassicRandom(19),id=random.register(uniformRandom(generator));
  random.push();random.pull();counters.outputFields=0;
  for(let i=0;i<40;i++)random.pull();
  assert.equal(counters.outputFields,40);
  assert.equal(nativeDraw(id),reference.uniform());random.pull();
  assert.equal(generator.state,reference.state);
  counters.outputFields=0;random.pull();assert.equal(counters.outputFields,1);
});

// Restores are real writes too: skipping an idle boundary cannot conceal one.
test('explicit generator state restores cross the next boundary',()=>{
  const {random,nativeDraw}=boundary(),generator=new ClassicRandom(17);
  const id=random.register(uniformRandom(generator));
  random.push();random.pull();generator.state=29;random.push();
  assert.equal(nativeDraw(id),new ClassicRandom(29).uniform());random.pull();
});
