// Actor program selection. A soldier, rig, held part or view part is a
// source-free Material "shell": it carries flags (defines) and uniform values,
// and this file maps those to one of the closed set of actors/ programs the
// Rust renderer owns. There is no three program and no nearest-variant
// substitute: a combination the census never saw is an error naming it.
import { programLayouts } from '../render-programs.js';
import { ownedProgram } from './programs.js';
// selector identity -> program key, built once from the generated layouts.
// The identity is every input that changes the compiled source: the sorted
// material defines, skinned or not, the cascade count, the draw mode and
// whether the lamp table is read.
const variants=new Map();
const identity=s=>JSON.stringify([s.defines,s.skin,s.cascades,s.mode,s.lamps]);
for(const [key,layout] of Object.entries(programLayouts))if(key.startsWith('actors/') && layout.selector){
  const id=identity(layout.selector);
  if(variants.has(id))throw Error('Duplicate actor selector '+key);
  variants.set(id,key);
}
// An actor registration failure that is about one object: a material, group
// layout, geometry or resource no descriptor covers, or a store that is full.
// claimActor() turns it into a skipped, once-reported object.
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
export function isActorMaterial(material) {
  if(Array.isArray(material)){for(const m of material)if(isActorMaterial(m))return true;return false;}
  return Boolean(material?.userData.actorShader || material?.userData.bfeProgram?.startsWith('actors/'));
}
// Texture pixels may arrive after a pooled actor is shown. A pending image
// uses the renderer's empty texture, then adopts its real handle on update.
export function actorMap(core,material) {
 const map=material.uniforms?.map?.value;
 if(!map)return -1;
 if(!map.isTexture)throw actorFailure(`Invalid actor map: ${material.name}`);
 const p=core.renderer.properties.get(map);
 if(!map.isRenderTargetTexture && map.version>0 && p.__version!==map.version && map.image)core.renderer.initTexture(map);
 return core.texture(map);
}
// A record remembers which map (and which version of it) its texture slots
// were resolved from, so a swapped or late-loaded image is noticed cheaply by
// refreshActorMaps without looking the texture up again every frame.
export function trackActorMaps(record,materials) {
 record.mapMaterials=materials;
 record.mapSources=materials.map(m=>m.uniforms?.map?.value || null);
 record.mapVersions=record.mapSources.map(m=>m?.version || 0);
}
// Re-resolve only the slots whose map changed, or that were empty (-1) and now
// have pixels: a pooled soldier is shown before its texture has decoded.
export function refreshActorMaps(core,record) {
 for(let i=0;i<record.mapMaterials.length;i++){
  const material=record.mapMaterials[i],map=material.uniforms?.map?.value || null,version=map?.version || 0;
  if(map!==record.mapSources[i] || version!==record.mapVersions[i] || (record.textures[i]<0 && version>0)){
   record.textures[i]=actorMap(core,material);record.mapSources[i]=map;record.mapVersions[i]=version;
  }
 }
}
// The same refresh for a record with a single material and texture (a held
// part or a view part).
export function refreshActorMap(core,record) {
 const map=record.material.uniforms?.map?.value || null,version=map?.version || 0;
 if(map!==record.mapSource || version!==record.mapVersion || (record.texture<0 && version>0)){
  record.texture=actorMap(core,record.material);record.mapSource=map;record.mapVersion=version;
 }
}

// The owned program for an actor shell. kind: 1 skinned, 2 rigid, anything
// else plain; view: a first-person part, which has no shadow cascades.
export function actorProgram(core, material, kind, view=false) {
  // The selector is read back from the shell exactly as the census recorded it.
  const mode=kind===1?'skinned':kind===2?'rigid':'plain';
  // ADDITIVE only changes the far scene's branch of the zero shader (the pass's blend state sets how an
  // actor adds), so an additive part uses the program of the same part blended normally.
  const defines=Object.entries(material.defines || {}).filter(([n])=>n!=='ADDITIVE').map(([n,v])=>[n,String(v)]).sort(([a],[b])=>a.localeCompare(b));
  // The lamp table only replaces the dynamic (lit) soldier lighting.
  const lamps=mode!=='plain' && Boolean(core.graphics.settings.get('lampLighting')) && Object.hasOwn(material.defines,'DYNAMIC');
  // A view part is lit without shadows; a body reads as many cascades as are on.
  const cascades=view?0:core.graphics.csm?.lights.length || 0;
  const selector={defines,skin:kind===1,cascades,mode,lamps};
  const key=variants.get(identity(selector));
  if(!key)throw variantFailure('Unowned actor variant '+identity(selector),identity(selector));
  const layout=programLayouts[key];let entry;
  try { entry=ownedProgram(core,key,null); } catch(error) { error.ownedActor=true;throw error; }
  // Remember what the pass needs to know about the program without re-reading
  // the layout: whether it takes lamps and which uniform names it declares.
  entry.lamps=lamps;entry.names ||= new Set(layout.uniforms.map(u=>u.name));
  return entry;
}

// The actors/ programs a battle with the current settings can use: those with
// no selector, and selector variants for this cascade count (or none) whose
// lamp mode matches. Warming only these keeps load time down without leaving a
// first-use compile for the frame.
function actorKeys(core) {
  const cascades=core.graphics.csm?.lights.length || 0;
  const lamps=Boolean(core.graphics.settings.get('lampLighting'));
  const keys=[];
  for(const [key,layout] of Object.entries(programLayouts)) {
    if(!key.startsWith('actors/'))continue;
    const s=layout.selector;
    if(!s){keys.push(key);continue;}
    // Derived variants (tools/actor-shader-compose.py) are for materials no census saw: linked on first use.
    if(layout.derived)continue;
    if(![0,cascades].includes(s.cascades))continue;
    const dynamic=s.defines.some(([name])=>name==='DYNAMIC');
    if(s.lamps===(dynamic && lamps))keys.push(key);
  }
  return keys;
}

// Driver waits belong on the loading screen, with a GPU/UI settling point
// between new programs. A later warmup only reuses the context-local IDs.
export async function warmActorPrograms(core, settle) {
  const renderer=core.renderer,target=renderer.getRenderTarget();
  for(const key of actorKeys(core)) {
    if(core.ownedPrograms?.has(key))continue;
    ownedProgram(core,key,null);
    renderer.resetState();renderer.setRenderTarget(target);
    await settle();
  }
}

// Link the settings' programs and any shell already in a scene before the
// first frame, so no actor draw waits on the driver.
export function prepareActors(core) {
  for(const key of actorKeys(core))ownedProgram(core,key,null);
  for(const [scene,view] of [[core.graphics.scene,false],[core.graphics.viewScene,true]])scene.traverse(o=>{
    for(const material of Array.isArray(o.material)?o.material:[o.material]) {
      // A per-object flash names its key; a body or part resolves through the selector.
      if(material?.userData.bfeProgram) { ownedProgram(core,material.userData.bfeProgram,null); continue; }
      if(!material?.userData.actorShader)continue;
      // A shell no variant covers is left to its pass, which skips and reports it.
      try { actorProgram(core,material,o.isSkinnedMesh?1:2,view); } catch(error) { if(error.variant===undefined)throw error; }
    }
  });
}
