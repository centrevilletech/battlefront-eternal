import {PointInputs} from "./point-inputs.js";
import {admitSkeleton} from "./scene-skeletons.js";
import {UnitLimits} from "../presentation/layout.js";
// Object3D shells publish only at mutation boundaries. The retained native
// registry owns parent visibility, layer tests, culling and order keys.
import { SceneNode as N, SceneWorldChange as W, SceneView as V, SceneSelection as S, SceneFlags as F,
  SceneCanopyView as CV, SceneCanopyHeader as CH,
  SceneGroup as G, SceneFact as Fact, SceneFactsHeader as Header, SceneFamily as Family, SceneMode as Mode, SceneCapacity as Capacity } from "./scene-layout.js";
export { Family, Mode, S };
import {sceneNodes,resourceNodes} from "./scene-changes.js";
import {TreePoseOwner} from "./pose-streams.js";
import {ScenePoseOwner} from "./scene-poses.js";
import {SceneWorlds} from "./scene-worlds.js";

// Presentation and rendering have distinct WASM memories. A native publisher
// hands changed f64 records to this observer, never a pointer in another heap.
// Three's copies remain readback facades for muzzle/audio/collision consumers.
const observers = sceneNodes;
const drawable = object => object.isMesh || object.isSkinnedMesh || object.isSprite || object.isLine || object.isPoints
  || object.userData.actorAsset || object.userData.effectBatch || object.userData.effectModel
  || object.userData.lineSource || object.userData.flyerSource || object.userData.flyerMotion
  || object.userData.nativeFar || object.userData.source;
// Native anchors have no representative geometry/material shell. Read actual
// legacy resources only for their remaining owners; actor appearances are cold
// authored values, while their native descriptor supplies blend policy.
const geometryOf=object=>object.isMesh||object.isSprite||object.isLine||object.isPoints?object.geometry:undefined;
const materialsOf=object=>object.isMesh||object.isSkinnedMesh||object.isSprite||object.isLine||object.isPoints||object.userData.actorAsset?object.material:undefined;
// Native publishers call this at their existing placement boundary. A hidden
// lazy actor is not placed merely because a later camera asks for selection.
// The renderer receives the native values before any legacy readback copy.
export function publishSceneWorld(object, data, at) {
  const node = observers.get(object);
  if (!node) return;
  node.worldVersion++;
  if(!node.families&&!object.isLOD&&!node.transformStop)return;
  let changed = !node.nativeWorld;
  for (let k = 0; k < 16; k++) {
    const value = data[at + k];
    if (!Object.is(node.world[k], value)) { node.world[k] = value; changed = true; }
  }
  node.nativeWorld = true;
  if (changed) node.owner.touch(node, false);
}

// Motion aliases the already published source transform. Native sources never
// round-trip through the Three readback used by unrelated presentation readers.
export function shareSceneWorld(target, source) {
  const node = observers.get(source);
  publishSceneWorld(target, node?.nativeWorld ? node.world : source.matrixWorld.elements, 0);
}

// Explicit writers coalesce changes without adding accessors to hot shells.
// Shared materials and geometries retain their admitted users, so a mutation
// marks those users directly instead of scanning every view's resources.
export class RetainedScene {
  constructor(core) {
    this.core = core;
    this.nodes = [];
    this.worlds=new SceneWorlds(this,publishSceneWorld);this.transforms=this.worlds;this.points=new PointInputs(this);this.poseOwners=[];
    core.addView("scenePoseWords",Uint32Array,()=>core.rust.scene_pose_ptr(),()=>core.rust.scene_pose_len()*2);
    this.nextSibling = 0;
    // A node occupies at most one pending row during a long load or pause.
    // The coalescing bitmap and queue have identical content-sized bounds.
    // Admission grows native storage cold; selection never grows these lists.
    this.dirty = new Uint32Array(Capacity.NODES);
    this.queued = new Uint8Array(Capacity.NODES);
    this.poseQueued=new Uint32Array(Capacity.NODES);
    this.poseDirty=new Uint32Array(Capacity.NODES);
    this.changed = 0;
    this.worldChanged = 0;
    this.lastView=new Float64Array(V.STRIDE);this.lastCount=0;this.lastValid=false;
    this.queryCalls=0;this.querySkips=0;
    this.clipped = 0;
    this.capacity = 8192;
    this.groupCapacity=32768;this.groupNext=0;this.groupChanged=0;
    this.materials = new WeakMap();
    this.geometries = new WeakMap();
    this.items = { opaque: [], transmissive: [], transparent: [] };
    this.nearItems = [];
    this.added = [];
    this.fresh = [];
    this.admission = [];
    for (const name of ["bounds", "view", "output", "states"]) {
      const key = "sceneCanopy" + name[0].toUpperCase() + name.slice(1);
      core.addView(key, name === "states" ? Float32Array : Float64Array,
        () => core.rust[`scene_canopy_${name}_ptr`](), () => core.rust[`scene_canopy_${name}_len`]());
    }
    core.rust.scene_reserve(this.capacity);
    core.addView("sceneMotionEvents",Float64Array,()=>core.rust.scene_motion_events_ptr(),()=>core.rust.scene_motion_events_len());
    core.addView("sceneLineList",Float64Array,()=>core.rust.scene_lines_ptr(),()=>core.rust.scene_lines_len());
    core.addView("sceneGroups",Float64Array,()=>core.rust.scene_groups_ptr(),()=>core.rust.scene_groups_len());
    core.addView("sceneWorldChanges",Float64Array,()=>core.rust.scene_world_changes_ptr(),()=>core.rust.scene_world_changes_len());
    core.addView("sceneChanges", Float64Array, () => core.rust.scene_changes_ptr(), () => core.rust.scene_changes_len());
    core.addView("sceneView", Float64Array, () => core.rust.scene_view_ptr(), () => core.rust.scene_view_len());
    core.addView("sceneOutput", Float64Array, () => core.rust.scene_output_ptr(), () => core.rust.scene_output_len());
    core.addView("sceneVisible",Uint32Array,()=>core.rust.scene_visible_ptr(),()=>core.rust.scene_visible_len());
    core.addView("sceneFacts", Float64Array, () => core.rust.scene_facts_ptr(), () => core.rust.scene_facts_len());
    core.addView("sceneOrder", Float64Array, () => core.rust.scene_order_ptr(), () => core.rust.scene_order_len());
  }

  treePoseOwner(source){if(source.treePoseOwner?.registry===this)return source.treePoseOwner;const owner=new TreePoseOwner(this,source);this.poseOwners.push(owner);source.treePoseOwner=owner;return owner;}
  poseOwner(source){if(source.scenePoseOwner?.registry===this)return source.scenePoseOwner;const owner=new ScenePoseOwner(this,source);this.poseOwners.push(owner);source.scenePoseOwner=owner;return owner;}
  // Cold topology admission and child events replace RustRender.watchScene.
  // Moving a retained subtree updates just its parent and insertion identity.
  admit(object, parent = null, root = true, scope = 0) {
    let node = observers.get(object);
    if (node?.owner === this) {
      node.parent = parent; node.sibling = this.nextSibling++;
      node.main = object === this.core.graphics.scene || Boolean(parent?.main);
      node.childIndex=parent?parent.object.children.indexOf(object):0;
      if(parent)this.worlds.reserveChildren(parent);
      this.enqueue(node); this.touch(node);this.worlds.dirty(node,true);
      for(let i=0;i<object.children.length;i++)this.admit(object.children[i],node,false);
      return node;
    }
    if (this.nodes.length >= this.capacity && this.capacity < Capacity.NODES) {
      this.capacity = Math.min(Capacity.NODES,this.capacity*2);
      this.core.rust.scene_reserve(this.capacity);this.core.buffer=null;this.core.views();
    }
    if (this.nodes.length >= this.capacity) { this.clipped++; return null; }
    node = { owner: this, object, slot: this.nodes.length, generation: 1, parent,
      sibling: this.nextSibling++, scope: parent?.scope ?? (object === this.core.graphics.farScene ? Family.FAR : object === this.core.graphics.viewScene ? Family.VIEW : scope), main: object === this.core.graphics.scene || Boolean(parent?.main), families: 0, resource: -1, instanceOrder: 0, records: [],
      row: new Float64Array(N.STRIDE), world: null, nativeWorld: false, propertiesDirty: true,
      item: { object, material: materialsOf(object), geometry: geometryOf(object), group: null,
        groupOrder: 0, renderOrder: object.renderOrder, z: 0, id: object.id } };
    // Capture shell identity once, before a pass can claim the drawable.
    // The retained item is also the generic near packet's permanent payload;
    // producing a view does not allocate another per-object item wrapper.
    node.world = node.row.subarray(N.WORLD, N.WORLD + 16); node.world.set(object.matrixWorld.elements);
    observers.set(object, node); this.nodes.push(node);
    this.worlds.admit(node);this.points.admit(node);
    const packet=object.userData.poseStream;if(packet&&!packet.owner)this.treePoseOwner(packet.source||packet).model(packet);
    const presentation=object.userData.presentationRow;
    if(presentation&&object.userData.presentationNode!==undefined){if(!presentation.scenePoseOwner)this.poseOwner(presentation.units).model(presentation,presentation.objects,presentation.start,presentation.actor?presentation.id:UnitLimits.ACTORS+presentation.id,presentation.skinStart||0,presentation.actor?.bones.length||0);}
    object.addEventListener("childadded", event => this.admit(event.child, node));
    // Removal only changes this node's root relationship. Its descendants
    // remain admitted for model pooling, and Rust suppresses the detached tree.
    // A later childadded mutation supplies the new parent and sibling order.
    object.addEventListener("childremoved", event => {
      const child = observers.get(event.child);
      if (child?.owner === this) { child.parent = null; this.touch(child);this.worlds.dirty(child,true);this.worlds.topology(node); }
    });
    this.resources(node);
    this.enqueue(node); this.touch(node);
    for (let i = 0; i < object.children.length; i++) this.admit(object.children[i], node, false);
    admitSkeleton(this,node);
    return node;
  }

  // Repeated replacements during a long load or pause retain one admission
  // per identity. Queue bounds depend on content capacity, never elapsed time.
  enqueue(node) {
    if (!node.freshQueued && this.fresh.length < Capacity.NODES) { node.freshQueued = true; this.fresh.push(node); }
    if (node.main && !node.addedQueued && this.added.length < Capacity.NODES) { node.addedQueued = true; this.added.push(node.object); }
  }

  // Resources can be shared by many shells. A visibility/version replacement
  // queues its known users once, rather than scanning materials every view.
  resources(node) {
    node.groupDirty=true;
    const object = node.object, material = materialsOf(object), geometry = geometryOf(object);
    node.item.material = material; node.item.geometry = geometry;
    // Material visibility is authored state, including a hidden group entry.
    // Native ownership no longer disguises a drawn material as invisible.
    // Version changes also queue cold program/resource replacement once.
    const observeMaterial = material => {
      if (!material) return;
      let users = this.materials.get(material);
      if (!users) {
        users = []; this.materials.set(material, users);
        resourceNodes.set(material,{users,groups:true});
      }
      if (!users.includes(node)) users.push(node);
    };
    if (Array.isArray(material)) { for (let i = 0; i < material.length; i++) observeMaterial(material[i]); }
    else observeMaterial(material);
    // Bounds/ranges and group tables are resource mutations. Observing their
    // shared geometry users avoids repeating descriptor reads in each view.
    // Dynamic vertex bytes still belong to their existing upload owner.
    if (geometry && !this.geometries.has(geometry)) {
      const users = []; this.geometries.set(geometry, users);
      const changed = () => { for (let i = 0; i < users.length; i++) {users[i].groupDirty=true;this.touch(users[i]);} };
      resourceNodes.set(geometry,{users,groups:true});resourceNodes.set(geometry.drawRange,{users,groups:true});
      const setRange=geometry.setDrawRange;
      if(setRange)geometry.setDrawRange=function(start,count){const beforeStart=this.drawRange.start,beforeCount=this.drawRange.count;setRange.call(this,start,count);if(beforeStart!==start||beforeCount!==count)changed();};
      const groupsChanged=()=>{for(let i=0;i<users.length;i++){users[i].groupDirty=true;this.touch(users[i]);this.enqueue(users[i]);}};

      for(const name of ["addGroup","clearGroups"]){const original=geometry[name];if(original)geometry[name]=function(a,b,c){const result=original.call(this,a,b,c);groupsChanged();return result;};}
    }
    if (geometry) {
      const users = this.geometries.get(geometry);
      if (!users.includes(node)) users.push(node);
    }
    if(node.scope&Family.MOTION && object.userData.source && object.userData.source!==object)node.motionSource=this.admitIfNew(object.userData.source);
    // A native static-depth anchor has no representative draw sphere. Its
    // resource owner selects cells using their real bounds, after the scene
    // registry has applied only topology, visibility and camera layers.
    if(object.userData.nativeShadow){node.families=Family.SHADOW;return;}
    if (drawable(object)) {
      node.families |= Family.SHADOW;
      if(object.userData.nativeFar)node.families |= Family.FAR;
      if (node.scope & (Family.MOTION | Family.EXTRA)) node.families |= object.userData.source ? Family.MOTION : Family.EXTRA;
      else node.families |= node.scope;
      if (object.isLine || object.userData.lineSource) node.families |= Family.LINE;
      let scope = node; while (scope.parent) scope = scope.parent;
      if (scope.object === this.core.graphics.farScene) node.families |= Family.FAR;
      if (scope.object === this.core.graphics.viewScene) node.families |= Family.VIEW;
      if (!(node.families & (Family.INSTANCE | Family.VIEW | Family.FAR | Family.LINE | Family.SEA | Family.WATER | Family.EFFECT))) node.families |= Family.NEAR;
    }
  }

  // Claiming a native family removes only the generic near admission.
  // Authored visibility and camera layers are never rewritten by the bridge.
  claim(object, tree = false) {
    const node = this.admitIfNew(object);
    // Repeated ownership validation changes no authored scene fact. Keeping
    // the existing claim avoids flushing the same background between views.
    if(node && !(node.families&Family.NEAR) && Boolean(node.claimedTree)===Boolean(tree))return;
    if (node) { node.families &= ~Family.NEAR; node.claimedTree = tree; this.touch(node); }
  }

  // Repeated transform, visibility and layer writes replace the same row.
  // The final mutation state crosses once, independent of how many writers
  // touched a model before the next requested view.
  touch(node, properties = true) {
    if (!properties) {
      // Native pose writers already hold the final matrix. Coalesce directly
      // into its generated block instead of queuing a second JS row walk.
      // Cold reserve preserves pending rows even when it relocates the block.
      let index=this.poseQueued[node.slot];
      if(node.propertiesDirty&&!index)return;
      const core=this.core;
      if(core.buffer!==core.wasm.memory.buffer)core.views();
      if(!index){index=++this.worldChanged;this.poseQueued[node.slot]=index;this.poseDirty[index-1]=node.slot;}
      const at=(index-1)*W.STRIDE,worlds=core.sceneWorldChanges;
      worlds[at+W.SLOT]=node.slot;worlds[at+W.GENERATION]=node.generation;
      worlds.set(node.world,at+W.WORLD);return;
    }
    node.propertiesDirty = true;
    if (this.queued[node.slot]) return;
    this.queued[node.slot] = 1;
    this.dirty[this.changed++] = node.slot;
  }

  // Pass admission attaches a retained resource record to scene identity.
  // Claiming one family does not erase shadow participation or another view's
  // ownership, and a missing resource never selects a browser draw path.
  bind(object, family, record, resource = -1) {
    const node = this.admitIfNew(object);
    if (!node) return null;
    node.families |= family; node.resource = resource;
    if (family & (Family.INSTANCE | Family.VIEW | Family.FAR | Family.LINE | Family.SEA | Family.WATER | Family.EFFECT)) node.families &= ~Family.NEAR;
    node.records[family] = record;
    this.touch(node);
    return node;
  }
  // Parent admission is cold and can recursively import a newly attached
  // subtree. Already-known nodes return immediately on the ordinary path.
  // Scope distinguishes first-person and motion clients from the main scene.
  admitIfNew(object, scope = 0) {
    const known = observers.get(object);
    if (known?.owner === this) return known;
    const parent = object.parent ? this.admitIfNew(object.parent) : null;
    return observers.get(object)?.owner === this ? observers.get(object) : this.admit(object, parent, true, scope);
  }

  // Write coalesced facts straight into the generated native block. This is
  // transport only: the host never decides effective visibility or view order.
  flush() {
    const core = this.core,count=this.changed;
    // Group tables change at geometry/material mutation, not with every pose.
    // Reserve once before acquiring any view: a cold growth can move all rows.
    this.groupChanged=0;
    for(let i=0;i<count;i++){
      const node=this.nodes[this.dirty[i]];if(!node.groupDirty)continue;
      const object=node.object,n=Array.isArray(materialsOf(object))?(geometryOf(object)?.groups.length ?? 0):0;
      if(n>(node.groupCapacity||0)){
        const available=Math.max(0,Capacity.GROUPS-this.groupNext),made=Math.min(n,available);
        node.groupFirst=this.groupNext;node.groupCapacity=made;this.groupNext+=made;this.clipped+=n-made;
      }
      node.groupCount=Math.min(n,node.groupCapacity||0);this.groupChanged+=node.groupCount;
    }
    if(this.groupNext>this.groupCapacity){
      this.groupCapacity=Math.min(Capacity.GROUPS,Math.max(this.groupNext,this.groupCapacity*2));
      core.rust.scene_groups_reserve(this.groupCapacity);core.buffer=null;
    }
    if(core.buffer!==core.wasm.memory.buffer)core.views();
    const changes=core.sceneChanges,groups=core.sceneGroups;let groupAt=0,written=0;
    for (let i = 0; i < count; i++) {
      const node = this.nodes[this.dirty[i]], object = node.object, out = node.row, o = 0;
      if (node.propertiesDirty) {
        const materials = materialsOf(object);
        let material = materials, visible = !materials || materials.visible !== false;
        if (Array.isArray(materials)) {
          visible = false; material = materials[0];
          for (let k = 0; k < materials.length; k++) if (materials[k] && materials[k].visible !== false) { visible = true; break; }
        }
        out[o + N.LOD_AUTO] = +(object.isLOD === true && object.autoUpdate === true);
        const levels = object.parent?.isLOD ? object.parent.levels : null;
        let level = -1;
        if (levels) for (let k = 0; k < levels.length; k++) if (levels[k].object === object) { level = k; break; }
        out[o + N.LOD_INDEX] = level;
        out[o + N.LOD_DISTANCE] = level >= 0 ? levels[level].distance : 0;
        out[o + N.LOD_HYSTERESIS] = level >= 0 ? levels[level].hysteresis : 0;
        out[o + N.SLOT] = node.slot; out[o + N.GENERATION] = node.generation;
        out[o + N.PARENT] = node.parent?.slot ?? -1; out[o + N.PARENT_GENERATION] = node.parent?.generation ?? 0;
        out[o + N.SIBLING] = node.sibling;
        out[o + N.FLAGS] = F.ALIVE | (object.visible ? F.VISIBLE : 0) | (visible ? F.MATERIAL_VISIBLE : 0)
          | (object.isGroup ? F.GROUP : 0) | (object.userData.patches ? F.SUBTREE_OWNER : 0) | (node.claimedTree ? F.CLAIMED_TREE : 0) | (object.frustumCulled ? F.CULLED : 0)
          | (node.motionSource ? F.MOTION_SOURCE : 0) | (object.userData.nativeShadow ? F.NATIVE_CULL : 0) | (Array.isArray(materials) && geometryOf(object) ? F.MATERIAL_ARRAY : 0) | (object.isSprite ? F.SPRITE : 0) | (node.blended ?? material?.transparent ? F.TRANSPARENT : 0) | (object.castShadow ? F.CAST_SHADOW : 0);
        out[o + N.LAYERS] = object.layers.mask; out[o + N.RENDER_ORDER] = object.renderOrder;
        out[o + N.OBJECT_ID] = object.id; out[o + N.MATERIAL_ID] = object.userData.order ?? material?.id ?? 0;
        out[o+N.MOTION_SOURCE]=node.motionSource?.slot??-1;out[o+N.MOTION_SOURCE+1]=node.motionSource?.generation??0;
        out[o+N.MOTION_FAR]=object.userData.far??0;
        out[o + N.FAMILIES] = node.families; out[o + N.RESOURCE] = node.resource; out[o + N.INSTANCE_ORDER] = node.instanceOrder;
        const geometry = geometryOf(object);
        out[o+N.GROUP_FIRST]=node.groupFirst||0;out[o+N.GROUP_COUNT]=node.groupCount||0;
        if(node.groupDirty){
          for(let k=0;k<(node.groupCount||0);k++){
            const authored=geometry.groups[k],m=materials[authored.materialIndex],at=groupAt++*G.STRIDE;
            groups[at+G.SLOT]=node.groupFirst+k;groups[at+G.OWNER]=node.slot;groups[at+G.GENERATION]=node.generation;
            groups[at+G.MATERIAL_ID]=m?.id||0;groups[at+G.MATERIAL_INDEX]=authored.materialIndex;
            groups[at+G.VISIBLE]=+Boolean(m?.visible);groups[at+G.FIRST]=authored.start;groups[at+G.COUNT]=authored.count;
          }
          node.groupDirty=false;
        }
        // Object bounds override geometry bounds, as Frustum.intersectsObject
        // does for skinned/instanced meshes. An unbound hidden shadow wrapper
        // cannot compute a skin sphere before its skeleton has been attached.
        let sphere = object.boundingSphere === undefined ? geometry?.boundingSphere : object.boundingSphere;
        if (!sphere && geometry?.attributes.position) {
          if (object.isSkinnedMesh && !object.skeleton) {
            // An unbound shadow placeholder can be admitted on hidden layers.
            // Its authored geometry supplies cold bounds until a rig is bound.
            if(!geometry.boundingSphere)geometry.computeBoundingSphere();sphere=geometry.boundingSphere;
          }
          else if (object.boundingSphere === null && object.computeBoundingSphere) object.computeBoundingSphere();
          else geometry.computeBoundingSphere();
          sphere ||= object.boundingSphere === undefined ? geometry.boundingSphere : object.boundingSphere;
        }
        const center=object.userData.effectModel?.center;
        out[o + N.SPHERE] = center?.[0] ?? sphere?.center.x ?? 0; out[o + N.SPHERE + 1] = center?.[1] ?? sphere?.center.y ?? 0;
        out[o + N.SPHERE + 2] = center?.[2] ?? sphere?.center.z ?? 0; out[o + N.SPHERE + 3] = object.isSprite ? 0.7071067811865476 : sphere?.radius ?? 0;
        out[o + N.RANGE] = geometry?.drawRange.start ?? 0;
        out[o + N.RANGE + 1] = geometry?.drawRange.count ?? object.userData.nativeIndexCount ?? 1;
        out[o + N.RANGE + 2] = geometry?.index?.count ?? geometry?.attributes.position?.count ?? object.userData.nativeIndexCount ?? 1;
        node.propertiesDirty = false;
      }
      // RetainedScene.flush imports descriptor facts only after a property or
      // resource mutation. A pose updates sixteen values in the cached row;
      // typed-array copy transports it without inspecting the shell again.
      if(node.pose)out.set(node.pose,N.POSE);
      changes.set(out, written++ * N.STRIDE);
    }
    for (let i = 0; i < count; i++) this.queued[this.dirty[i]] = 0;
    this.changed = 0;
    return written;
  }

  // One sparse publication and one camera block precede each native phase.
  // No object identity crosses through an individual selection/culling export.
  // Motion additionally qualifies the scene whose sources its pool follows.
  select(scene, camera, families, mode = Mode.SCENE, layers = camera.layers.mask, limit=0, motionRoot=null) {
    const sourceRoot=motionRoot?this.admitIfNew(motionRoot):null;
    const root = this.admitIfNew(scene, families), core = this.core;
    if (!root) return 0;
    if(scene.matrixWorldAutoUpdate)this.worlds.update(root,false,true);else this.worlds.flush();
    if(core.buffer!==core.wasm.memory.buffer)core.views();
    const view = core.sceneView;
    view[V.ROOT] = root.slot; view[V.GENERATION] = root.generation;
    view[V.MOTION_ROOT]=sourceRoot?.slot??-1;view[V.MOTION_ROOT+1]=sourceRoot?.generation??0;
    view[V.LAYERS] = layers; view[V.FAMILIES] = families; view[V.MODE] = mode;view[V.OUTPUT_LIMIT]=limit;
    core.rust.camera_scene_view_frame(core.cameras.query(camera));
    // An identical consecutive query already owns the published output and
    // fact banks. Compare exact doubles; camera cells cannot approximate clips.
    // Motion queries advance lifecycle even when every input bit is unchanged.
    let same=this.lastValid && mode!==Mode.MOTION_PREPARE && mode!==Mode.PROXY;
    for(let i=0;same && i<V.STRIDE;i++)if(!Object.is(view[i],this.lastView[i]))same=false;
    if(same){this.querySkips++;return this.lastCount;}
    this.lastView.set(view);this.lastValid=true;this.queryCalls++;
    const count = core.rust.scene_select(0,0,0);
    // Only consumed identities are cleared. Later preparation mutations start
    // a new sparse prefix; unchanged family queries never call flush at all.
    for(let i=0;i<this.worldChanged;i++)this.poseQueued[this.poseDirty[i]]=0;
    this.worldChanged=0;
    if(core.buffer!==core.wasm.memory.buffer)core.views();
    this.lastCount=count;this.lastClipped=core.sceneFacts[Header.CLIPPED];
    return count;
  }

  // TreeCanopies' authored sphere table is a cold resource. Its generation
  // qualifies every later query, while the native selector keeps active slots.
  registerCanopies(canopies) {
    const core = this.core;
    canopies.nativeGeneration = this.canopyGeneration = (this.canopyGeneration || 0) + 1;
    core.rust.scene_canopy_reserve(canopies.groups.length, canopies.nativeGeneration);
    core.buffer = null; core.views();
    const bounds = core.sceneCanopyBounds;
    for (let i = 0; i < Math.min(canopies.groups.length, bounds.length / 4); i++) {
      const sphere = canopies.groups[i].sphere, at = i * 4;
      bounds[at] = sphere.center.x; bounds[at + 1] = sphere.center.y;
      bounds[at + 2] = sphere.center.z; bounds[at + 3] = sphere.radius;
    }
  }
  // Each view publishes its raw camera once. Rust retains the old camera cache,
  // nearby-group order, changed leaf ranges and distant-card mask state.
  selectCanopies(canopies, camera) {
    const core = this.core; core.views(); const view = core.sceneCanopyView;
    view[CV.GENERATION] = canopies.nativeGeneration; view[CV.CAMERA] = camera.id;
    view[CV.ENABLED] = +(core.graphics.settings.get("treeDetail") !== "original");
    view[CV.LAYERS] = camera.layers.mask; view[CV.NODE_LAYERS] = canopies.node.layers.mask;
    core.rust.camera_canopy_view_frame(core.cameras.query(camera));
    core.rust.scene_canopy_select(); core.views();
    return core.sceneCanopyOutput[CH.COUNT];
  }

  // Effective state is native output for the most recently prepared view.
  // A hidden/detached ancestor leaves its descendants with an older epoch.
  // Membership in the most recently prepared view: in the scene, it and its
  // parents visible, in the camera's layers. A hidden/detached ancestor leaves
  // its descendants out of the native bitset. Culled adds the frustum receipt
  // of the last near, instance, line or sea preparation (water surfaces).
  shown(object, culled = false) {
    const node = observers.get(object);
    if (node?.owner !== this) return false;
    const visible=this.core.sceneVisible;
    if ((visible[node.slot>>>5]&(1<<(node.slot&31)))===0) return false;
    if (!culled || !node.families) return true;
    const facts = this.core.sceneFacts, at = Header.STRIDE + node.slot * Fact.STRIDE;
    return facts[at + Fact.EPOCH] === facts[Header.EPOCH] && facts[at + Fact.GENERATION] === node.generation && facts[at + Fact.IN_VIEW] !== 0;
  }
  // Effects and actor packets borrow the native group order and world centre.
  // This accessor copies numeric facts; it does not inspect parent visibility,
  // transform a sphere or recalculate a projected depth in JavaScript.
  key(object, out, at = 0) {
    const node = observers.get(object), facts = this.core.sceneFacts;
    if (node?.owner !== this) return;
    const from = Header.STRIDE + node.slot * Fact.STRIDE;
    out[at] = facts[from + Fact.GROUP_ORDER]; out[at + 1] = object.renderOrder;
    out[at + 2] = facts[from + Fact.CENTER]; out[at + 3] = facts[from + Fact.CENTER + 1]; out[at + 4] = facts[from + Fact.CENTER + 2]; out[at + 5] = object.id;
  }

  // Selected ids refer to retained items admitted with the shell. No transient
  // near items or painter comparisons are constructed by the browser.
  near(scene, camera) {
    const count = this.select(scene, camera, Family.NEAR), out = this.core.sceneOutput;
    // The native SCENE mode already retains insertion order and capacity.
    // The transparent owner later merges these retained payloads with its
    // world, particle and fountain sources through the existing native sorter.
    const items = this.nearItems; items.length = count;
    for (let i = 0; i < count; i++) {
      const at = i * S.STRIDE, item = this.nodes[out[at + S.SLOT]].item;
      item.groupOrder = out[at + S.GROUP_ORDER]; item.renderOrder = out[at + S.RENDER_ORDER]; item.z = out[at + S.DEPTH];
      items[i] = item;
    }
    return items;
  }
}
