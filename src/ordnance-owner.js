// Delayed ordnance belongs to the launching life, even if its Unit is reused.
// Attribution is immutable; a Character remains the same score-bearing object.
const sources = new WeakMap();

// captureOrdnanceOwner and native recon notices construct the same plain
// facade. Only scalar attribution is enumerable, so bootstrap serialization
// cannot walk the launching body's resources or a later life of that body.
function snapshotOf(source, id, generation, team, human, className, character) {
  const snapshot = { id, generation, team, human, className };
  Object.defineProperties(snapshot, {
    sourceUnit: { value: source },
    character: { value: character },
    soldier: { value: source.soldier },
    alive: { get: () => source.generation === generation && source.alive },
    removed: { get: () => source.generation !== generation || source.removed },
    // The original origin closure follows the body even after its next spawn.
    // This differs deliberately from life-qualified ray exclusion below.
    origin: { value: out => source.origin(out) },
  });
  sources.set(snapshot, source);
  return Object.freeze(snapshot);
}

export function captureOrdnanceOwner(owner) {
  if (!owner || typeof owner !== 'object' || !Number.isSafeInteger(owner.id) || !Number.isSafeInteger(owner.generation)) return owner;
  if (sources.has(owner)) return owner;
  return snapshotOf(owner, owner.id, owner.generation, owner.team, Boolean(owner.human), owner.className, owner.character ?? null);
}

// Native launches can become attackers before their outer phase returns.
// Prepare the immutable facade and its private cell at construction; binding
// sampled metadata later mutates only that inaccessible cell, without making
// another object or closure inside a synchronous publication barrier.
const prepared = new WeakMap();
const PREPARED_FIELDS = {
  id: {enumerable:true,get() { return prepared.get(this).id; }},
  generation: {enumerable:true,get() { return prepared.get(this).generation; }},
  team: {enumerable:true,get() { return prepared.get(this).team; }},
  human: {enumerable:true,get() { return prepared.get(this).human; }},
  className: {enumerable:true,get() { return prepared.get(this).className; }},
  sourceUnit: {get() { return prepared.get(this).source; }},
  character: {get() { return prepared.get(this).character; }},
  soldier: {get() { return prepared.get(this).soldier; }},
  alive: {get() {
    const row = prepared.get(this);
    return row.source.generation === row.generation && row.source.alive;
  }},
  removed: {get() {
    const row = prepared.get(this);
    return row.source.generation !== row.generation || row.source.removed;
  }},
};
export function prepareOrdnanceOwner() {
  const state = {source:null,id:0,generation:0,team:0,human:false,className:'',character:null,soldier:null};
  const snapshot = {};
  prepared.set(snapshot,state);sources.set(snapshot,null);
  Object.defineProperties(snapshot,PREPARED_FIELDS);
  // Like captureOrdnanceOwner's closure, origin follows the same body's
  // position after a later spawn and does not depend on a caller's `this`.
  Object.defineProperty(snapshot,'origin',{value:out=>state.source.origin(out)});
  Object.freeze(snapshot);
  return {snapshot,state};
}

// A native launch already sampled these values. Reading today's source team
// or Character here would silently credit a reused Unit's new life instead.
export function restoreOrdnanceOwner(source, sample, frame) {
  const state = frame.state;
  state.source = source;state.soldier = source.soldier;
  state.id = sample.id;state.generation = sample.life;state.team = sample.team;
  state.human = sample.human;state.className = sample.className;state.character = sample.character;
  sources.set(frame.snapshot,source);
  return frame.snapshot;
}

// Deliberate life-bound exclusion: a delayed shot may hit a newly spawned life
// on the same reused Unit. The previous pointer-only exclusion spared that life.
export function resolveOrdnanceOwner(owner) {
  const source = sources.get(owner);
  return source ? (source.generation === owner.generation ? source : null) : owner;
}
