import test from 'node:test';
import assert from 'node:assert/strict';
import { createEntityRegistry, entityRegistryDescriptor, verifyEntityRegistry } from '../src/runtime/network-entity-registry.js';
const options = () => ({ contentEpoch: 7, inputCodes: ['KeyW', 'Mouse0', 'Space'], bindingProfiles: new Map([[0, {}], [1, { takeoff: ['Space'], throttle: ['KeyW'] }]]) });
const data = () => ({ sides: { rep: { units: { trooper: { weapons: [{ class: 'rifle' }, { class: 'grenade' }] } } }, cis: { units: { droid: { weapons: [{ class: 'rifle' }] } } } },
  vehicles: { classes: { flyer: { seats: [{ weapons: [{ def: { class: 'cannon' } }] }] } } } });

test('both ends derive identical sorted registries from matching content irrespective of load order', () => {
  const a = data(), b = data(); b.sides = { cis: b.sides.cis, rep: b.sides.rep }; b.sides.rep.units.trooper.weapons.reverse();
  const first = createEntityRegistry(a, options()), second = createEntityRegistry(b, options());
  assert.deepEqual(entityRegistryDescriptor(first), entityRegistryDescriptor(second));
  assert.deepEqual([...first.classes.values()], ['droid', 'flyer', 'trooper']);
  assert.deepEqual([...first.weapons.values()], ['cannon', 'grenade', 'rifle']);
  assert.equal(first.classIds.get('trooper'), 3); assert.equal(first.weaponIds.get('rifle'), 3);
  assert.equal(verifyEntityRegistry(first, entityRegistryDescriptor(second)), first);
});

test('registry verification rejects mismatched content and profiles before numeric entity lookup', () => {
  const registry = createEntityRegistry(data(), options());
  for (const mutate of [d => d.contentEpoch++, d => d.classes[0][1] = 'other', d => d.weapons.reverse(),
    d => d.inputCodes.reverse(), d => d.bindingProfiles[1][1].throttle = ['Mouse0']]) {
    const descriptor = entityRegistryDescriptor(registry); mutate(descriptor);
    assert.throws(() => verifyEntityRegistry(registry, descriptor), /mismatch/);
  }
});

test('registries copy caller-owned profiles and reject unbounded or unknown input codes', () => {
  const context = options(), registry = createEntityRegistry(data(), context);
  context.inputCodes[0] = 'Other'; context.bindingProfiles.get(1).throttle.push('Other');
  assert.deepEqual(registry.inputCodes, ['KeyW', 'Mouse0', 'Space']);
  assert.deepEqual(registry.bindingProfiles.get(1).throttle, ['KeyW']);
  assert.throws(() => createEntityRegistry(data(), { ...options(), inputCodes: ['KeyW', 'KeyW'] }), /input-code/);
  assert.throws(() => createEntityRegistry(data(), { ...options(), inputCodes: Array.from({ length: 129 }, (_, i) => `K${i}`) }), /input-code/);
  const bad = options(); bad.bindingProfiles.set(2, { throttle: ['Unknown'] });
  assert.throws(() => createEntityRegistry(data(), bad), /missing/);
});

test('combat definition registry ignores geometry and resolves shared definitions deterministically',async()=>{
 const {createCombatRegistry}=await import('../src/runtime/network-combat-registry.js');
 const def={Velocity:60,LifeSpan:.2},a=createCombatRegistry({z:{ordnance:def},geometry:new Float32Array(10000),a:{ordnance:def}});
 const b=createCombatRegistry({a:{ordnance:{Velocity:60,LifeSpan:.2}},z:{ordnance:def},geometry:new Float32Array(10000)});
 assert.equal(a.definitionId(def),1);assert.equal(a.paths[0],JSON.stringify(['a','ordnance']));assert.equal(a.definitions.size,1);
 assert.equal(b.paths[0],a.paths[0]);
});

test('the registry names only the sides the mission fields, whatever side a machine loaded first', () => {
  const theed = () => ({ world: { mission: { sides: ['gar', 'cis'] } },
    sides: { cis: { units: { droid: { weapons: [{ class: 'rifle' }] } } }, gar: { units: { guard: { weapons: [{ class: 'pistol' }] } } } },
    sideFx: { cis: { effects: { shield: { Effect: 'bubble', LightRadius: 2 } } }, gar: { effects: {} } } });
  const server = theed();
  // The server's rotation said side=rep: it loaded the Republic too.
  server.sides.rep = { units: { trooper: { weapons: [{ class: 'rifle' }, { class: 'grenade' }] } } };
  server.sideFx.rep = { effects: { jet: { Effect: 'flame', LightRadius: 1 } } };
  const local = theed(), client = createEntityRegistry(local, options()), host = createEntityRegistry(server, options());
  assert.deepEqual(entityRegistryDescriptor(host), entityRegistryDescriptor(client));
  assert.deepEqual([...client.classes.values()], ['droid', 'guard']);
  assert.deepEqual([...client.weapons.values()], ['pistol', 'rifle']);
  assert.equal(host.combat.definitionId(server.sideFx.cis.effects.shield), client.combat.definitionId(local.sideFx.cis.effects.shield));
  assert.throws(() => host.combat.definitionId(server.sideFx.rep.effects.jet), /absent/);
});

test('a post with no name gets its place in the list as its network name', async () => {
  const { CommandPosts } = await import('../src/commandposts.js');
  const desc = name => ({ name, team: 1, matrix: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1], hologram: [0, 0, 0] });
  const posts = new CommandPosts(null, { commandPosts: [desc('cp3'), desc(''), desc('cp6')] }, { headless: true }).posts;
  assert.deepEqual(posts.map(post => post.name), ['cp3', '#1', 'cp6']);
  const registry = createEntityRegistry(data(), { ...options(), posts });
  assert.deepEqual([...registry.posts.values()], ['#1', 'cp3', 'cp6']);
  assert.throws(() => createEntityRegistry(data(), { ...options(), posts: [{ name: '' }] }), /registry name/);
});
