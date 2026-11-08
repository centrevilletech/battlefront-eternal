// Actor resource policy and shader selection are native. Cold admission keeps
// a numeric appearance handle; frames carry only pose and live lighting values.
import {ACTOR} from './actor-layout.js';
import {nativeActorSource} from '../actor-resources.js';
export function actorFailure(message) {
  const error=Error(message);error.ownedActor=true;error.skippable=true;return error;
}
// No program for a material the game made. The census and the derived variants
// make it unreachable (tests/actor-programs.test.js fails CI when a cell of the
// material space has no program); in a player's frame it is still never a stop:
// the objects that need it are skipped and the variant reported once.
export function variantFailure(message, variant) {
  const error=actorFailure(message);error.variant=variant;return error;
}
// An object's registration, where a failure about the object skips it (for as
// long as it keeps the material it failed with) and reports it once, while a
// failure of a program (the GPU rejected it) propagates. A missing variant is
// reported once for the variant, however many objects need it; the count of
// objects skipped for each is kept for the measurement tools (core.missingVariants).
export function claimActor(core, object, make) {
  const skipped=core.skippedActors ||= new WeakMap();
  if(skipped.get(object)===object.material)return null;
  try { return make(); } catch(error) {
    if(!error.skippable)throw error;
    skipped.set(object,object.material);
    if(error.variant!==undefined)missingVariant(core,object,error);
    else core.skip(object,object.material,error.message);
    return null;
  }
}
// One report per variant identity, then only its count.
function missingVariant(core,object,error) {
  const counts=core.missingVariants ||= new Map();
  const count=(counts.get(error.variant) || 0)+1;
  counts.set(error.variant,count);
  if(count===1)core.skip(object,object.material,error.message);
}
// Whether a material is an actor shell: it names an actor shader or, for the
// per-object Plain flash, an owned actors/ key. An array counts if any entry does.
export function isActorMaterial(look){if(Array.isArray(look)){for(const v of look)if(isActorMaterial(v))return true;return false;}return look?.actorAppearance===true;}
// Numeric sampler aliases observe late uploads and setting changes inside the
// resource bank. No texture Source/version/property lookup runs in a frame.
export function actorSample(core,look){if(look.map==null)return -1;look.map=core.resources.staticSample(look.map);return core.texture(look.map);}
export function nativeActorRecord(core,node,view=false){
 const asset=node.userData.actorAsset;if(!asset)throw actorFailure('Actor source missing');
 const source=nativeActorSource(core,asset),count=source.facts.textures.length||1;
 const looks=Array.isArray(node.material)?node.material.slice(0,count):[node.material];if(!looks.length||!looks.every(isActorMaterial))throw actorFailure('Actor appearance missing');
 const samplers=Int32Array.from(looks,look=>actorSample(core,look));
 const look=looks[0],style={...look.authored};
 let facts;try{facts=core.rust.actor_material(source.id,JSON.stringify(style),!!node.isSkinnedMesh,core.graphics.csm?.lights.length||0,!!core.graphics.settings.get('lampLighting'),view,samplers);}catch(error){if(String(error).startsWith('unknown actor variant'))throw actorFailure(String(error));throw error;}
 if(!facts.length)throw actorFailure('Actor appearance capacity');const d=ACTOR.DESCRIPTOR;
 core.buffer=null;core.views();
 return {node,appearance:node.material,look,handle:facts[d.HANDLE],blended:!!facts[d.BLENDED],lamps:!!facts[d.LAMPS],heated:!!facts[d.HEATED],view};
}
// Browser callers supply pacing only; native Prepare selects and links keys.
export async function warmActorPrograms(core,settle){await core.prewarm(1,settle);}

// Existing paced catalog warming is cold loading work. Native admission still
// owns every actual actor variant, group, sampler slot and fixed-function state.
export function prepareActors(core){
 for(const [scene,view]of [[core.graphics.scene,false],[core.graphics.viewScene,true]])scene.traverse(node=>{if(node.userData.actorAsset&&!node.userData.actorWhole&&!node.userData.shadowOnly&&isActorMaterial(node.material))core.byName[view?'view':'instances'].admit(node);});
}
