// Delayed ordnance belongs to the launching life, even if its Unit is reused.
// Attribution is immutable; a Character remains the same score-bearing object.
const sources=new WeakMap();
export function captureOrdnanceOwner(owner){
 if(!owner||typeof owner!=='object'||!Number.isSafeInteger(owner.id)||!Number.isSafeInteger(owner.generation))return owner;
 if(sources.has(owner))return owner;
 const generation=owner.generation;
 const snapshot={id:owner.id,generation,team:owner.team,human:Boolean(owner.human),className:owner.className};
 // Bootstrap serialization needs only scalar attribution, never a recursive
 // copy of the launching body's resources or a later life of that body.
 Object.defineProperties(snapshot,{
  character:{value:owner.character??null},soldier:{value:owner.soldier},
  alive:{get:()=>owner.generation===generation&&owner.alive},
  removed:{get:()=>owner.generation!==generation||owner.removed},
  origin:{value:out=>owner.origin(out)},
 });
 sources.set(snapshot,owner);return Object.freeze(snapshot);
}
// Deliberate life-bound exclusion: a delayed shot may hit a newly spawned life
// on the same reused Unit. The previous pointer-only exclusion spared that life.
export function resolveOrdnanceOwner(owner){const source=sources.get(owner);return source?(source.generation===owner.generation?source:null):owner;}
