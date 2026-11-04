import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireRegistryBridge } from '../src/runtime/wire-registry.js';
import { createEntityRegistry, entityRegistryDescriptor } from '../src/runtime/network-entity-registry.js';
const options = () => ({ contentEpoch: 7, inputCodes: ['KeyW', 'Mouse0', 'Space'], bindingProfiles: new Map([[2, { throttle: ['KeyW'], takeoff: ['Space'] }], [0, {}]]) });
const names = ['z', 'a', '2', '10', '\ud800', '\ud800\udc00', '\ue000', 'ä', '\udfff', 'ひらがな', '__proto__', 'constructor'];
const data = () => ({ sides: { rep: { units: Object.fromEntries(names.map(name => [name, { weapons: [{ class: name }, { class: name }] }])) } }, vehicles: { classes: { flyer: { seats: [{ weapons: [{ def: { class: 'cannon' } }] }] } } } });
test('native registry tables preserve UTF-16 IDs, input profile order and private copies', async () => {
  const instance = await loadSim(), native = createWireRegistryBridge(bridgeFor(instance));
  for (let n = 0; n < 75; n++) {
    const content = data(), opts = options();
    opts.posts = names.slice(n % names.length).map(name => ({ name }));
    opts.inputCodes = names.slice(n % 6); opts.bindingProfiles = new Map([[4, Object.fromEntries(names.map(name => [name, opts.inputCodes.slice(0, 2)]))], [1, {}], [0, { forward: [] }]]);
    const expected = entityRegistryDescriptor(createEntityRegistry(content, opts));
    const got = createEntityRegistry(content, { ...opts, native });
    assert.deepEqual(entityRegistryDescriptor(got), expected);
    opts.inputCodes[0] = 'mutated'; opts.bindingProfiles.get(4).z.push('mutated');
    assert.deepEqual(entityRegistryDescriptor(got), expected);
    if (n === 13) instance.exports.memory.grow(2);
  }
});
test('native registry rejects malformed content and profiles with the original first error', async () => {
  const native = createWireRegistryBridge(bridgeFor(await loadSim()));
  const mutations = [
    (d,o) => o.contentEpoch = '7', (d,o) => o.contentEpoch = 0, (d,o) => o.contentEpoch = 2**32,
    (d,o) => o.inputCodes = null, (d,o) => o.inputCodes = ['x','x'], (d,o) => o.inputCodes = ['',3], (d,o) => o.inputCodes = [null, undefined],
    (d,o) => o.bindingProfiles = {}, (d,o) => o.bindingProfiles = new Map(), (d,o) => o.bindingProfiles = new Map([[-1,{}]]), (d,o) => o.bindingProfiles = new Map([[NaN,{}]]), (d,o) => o.bindingProfiles = new Map([['1',{}]]),
    (d,o) => o.bindingProfiles = new Map([[0,[]]]), (d,o) => o.bindingProfiles = new Map([[0,{'':[]}]]), (d,o) => o.bindingProfiles = new Map([[0,{fire:'Mouse0'}]]), (d,o) => o.bindingProfiles = new Map([[0,{fire:['not-held']}]]),
    (d,o) => o.bindingProfiles = new Map([[0,{fire:Array(9).fill('Mouse0')}]]),
    d => d.sides.rep.units.z.weapons[0].class = null, (d,o) => o.posts = [{name:''}], d => d.sides.rep.units[''] = {},
    (d,o) => {d.sides.rep.units[''] = {}; o.posts = [null];}, (d,o) => {o.contentEpoch = 0; o.inputCodes = null; o.bindingProfiles = null;},
  ];
  for (const change of mutations) {
    const d = data(), o = options(); change(d,o); let expected;
    try { createEntityRegistry(d,o); } catch (error) { expected = error.message; }
    assert.ok(expected); assert.throws(() => createEntityRegistry(d,{...o,native}), error => error.message === expected);
  }
});
