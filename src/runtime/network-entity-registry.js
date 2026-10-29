import { createCombatRegistry } from './network-combat-registry.js';
// Registry IDs are derived from already authenticated, matching local content.
// The bootstrap carries names and input profiles, never executable definitions.
const name = value => {
  if (typeof value !== 'string' || !value.length || value.length > 128) throw Error('invalid entity registry name');
  return value;
};
const integer = value => {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff) throw Error('invalid entity registry id');
  return value;
};
const table = names => {
  const ordered = [...new Set(names.map(name))].sort();
  if (ordered.length > 0xffff) throw Error('entity registry capacity exceeded');
  return new Map(ordered.map((value, index) => [index + 1, value]));
};
const reverse = values => new Map([...values].map(([id, value]) => [value, id]));

// The content a registry names: what the battle's mission fields (the one
// src/mission.js picked; data.world and data.vehicles are that battle's). A
// side a URL named that the mission does not use (a server rotation's
// side=rep on Hoth, where only the Galactic Civil War is fought) is loaded on
// that machine only, and must not change the ids. The same objects are
// referenced, never copied, so combat definitions keep their identity.
export function registryContent(data) {
  const fielded = data?.world?.mission?.sides;
  const names = [...new Set(Array.isArray(fielded) && fielded.length ? fielded : Object.keys(data?.sides || {}))].sort();
  const sides = {}, sideFx = {};
  for (const name of names) {
    if (data?.sides?.[name]) sides[name] = data.sides[name];
    if (data?.sideFx?.[name]) sideFx[name] = data.sideFx[name];
  }
  return { common: data?.common, effects: data?.effects, props: data?.props, vehicles: data?.vehicles, world: data?.world, sides, sideFx };
}

export function createEntityRegistry(data, { contentEpoch, inputCodes, bindingProfiles, posts = [] } = {}) {
  if (!Number.isSafeInteger(contentEpoch) || contentEpoch < 1 || contentEpoch > 0xffffffff) throw Error('invalid content epoch');
  if (!Array.isArray(inputCodes) || inputCodes.length > 128 || new Set(inputCodes).size !== inputCodes.length) throw Error('invalid input-code registry');
  inputCodes = inputCodes.map(name);
  if (!(bindingProfiles instanceof Map) || !bindingProfiles.size || bindingProfiles.size > 256) throw Error('invalid binding-profile registry');
  const profiles = new Map();
  for (const [id, bindings] of [...bindingProfiles].sort(([a], [b]) => a - b)) {
    integer(id);
    if (!bindings || typeof bindings !== 'object' || Array.isArray(bindings) || Object.keys(bindings).length > 32) throw Error('invalid binding profile');
    const profile = {};
    for (const action of Object.keys(bindings).sort()) {
      name(action); const codes = bindings[action];
      if (!Array.isArray(codes) || codes.length > 8 || codes.some(code => !inputCodes.includes(code))) throw Error('binding code missing from registry');
      profile[action] = [...codes];
    }
    profiles.set(id, profile);
  }
  const classes = [], weapons = [], content = registryContent(data);
  for (const side of Object.values(content.sides)) for (const [className, definition] of Object.entries(side.units || {})) {
    classes.push(className);
    for (const weapon of definition.weapons || []) weapons.push(weapon.class);
  }
  for (const [className, definition] of Object.entries(data?.vehicles?.classes || {})) {
    classes.push(className);
    for (const seat of definition.seats || []) for (const weapon of seat.weapons || []) weapons.push(weapon.def?.class);
  }
  for (const instance of data?.props?.instances || []) if(instance.weapon?.class) weapons.push(instance.weapon.class);
  const classTable = table(classes), weaponTable = table(weapons), postTable = table(posts.map(post => typeof post === 'string' ? post : post.name ?? post.desc?.name));
  if (!classTable.size) throw Error('entity registry has no classes');
  return { combat: createCombatRegistry(content), contentEpoch, classes: classTable, classIds: reverse(classTable), weapons: weaponTable,
    weaponIds: reverse(weaponTable), posts: postTable, postIds: reverse(postTable), inputCodes, bindingProfiles: profiles };
}

export function entityRegistryDescriptor(registry) {
  return structuredClone({ combatDefinitions: registry.combat?.paths ?? [], contentEpoch: registry.contentEpoch, classes: [...registry.classes], weapons: [...registry.weapons], posts: [...registry.posts],
    inputCodes: registry.inputCodes, bindingProfiles: [...registry.bindingProfiles] });
}

// Compare with a locally constructed registry before interpreting any numeric
// entity fields. Do not accept server-provided class names as a load request.
export function verifyEntityRegistry(registry, descriptor) {
  if (JSON.stringify(entityRegistryDescriptor(registry)) !== JSON.stringify(descriptor)) throw Error('entity content/input registry mismatch');
  return registry;
}
