import '../fixtures/native-render-descriptors.js';
import {actor_source_description,actor_appearance_description,actor_effect_description,program_warmup_keys} from '../../src/rust-pkg/bfe_render.js';
import {ACTOR} from '../../src/rust-render/actor-layout.js';
import { LINE as D } from '../../src/rust-render/line-layout.js';
import {TransformChange} from "../../src/rust-render/scene-transform-layout.js";
// Stand-ins for the Rust renderer's tests (tests/rust-render*.test.js): the
// WebAssembly module (src/rust-pkg) and the page's side of three that the
// bridge (src/rust-render/index.js) is given. One module answers every
// binding any pass calls, so a new pass's views and calls don't break the
// other passes' tests.
//
// The module (standinModule):
// - `<name>_ptr()` and `<name>_len()`: each view the page makes gets a
//   region of its own in one memory, the first time it is asked for: its
//   length from `lengths` (a number, or a function for one that changes),
//   16 bytes aligned and room for 8 bytes an element (f64 views too).
//   `region(name)` is its byte offset, for a test that writes into it.
// - The values the page lays its writes out by (`layout`: records_at,
//   record_stride, transparent_object_stride).
// - Every other call is logged in `calls` as [name, ...args] and answers a
//   phase's stats (`stats`) for a phase entry (`frame`, `*_frame`,
//   `transparent_segment`), else an id: how many calls of that name came
//   before (registration calls hand ids back). add_texture is logged
//   without its WebGL texture. Names in `quiet` (by default the per-frame
//   bookkeeping, transparent_bucket_shown, and a pass's own switch,
//   foliage_on) are not logged. `methods` gives any binding a body of its
//   own (an object, or a function of { calls, region } making one); a test
//   can also set one on the instance later.
import { FACTS as F, STATE as S, BINDING as B, META as M, ROLE as R, LINE as V, LIMIT as L, STATS as T } from "../../src/rust-render/frame-layout.js";
import { FACTS as PF, STATE as PS } from "../../src/rust-render/post-layout.js";
import { DEPTH_KEY as DK,DEPTH_MARKER as DM,POINT_CHANGE as PC, UNIFORM_INPUT as UI, UNIFORM_OUTPUT as UO, INPUT as CI, CAMERA as CC, CASCADE as CW, LIGHT as CL, PACK as CP, MIRROR as CM, ROLE as CR, LIMIT as CX, LIGHT_INPUT as CLI, LIGHT_OUTPUT as CLO } from "../../src/rust-render/camera-layout.js";
import * as THREE from "three";

import { readFileSync } from "node:fs";
import { initSync, BfeScene, BfePoseHistory, BfeUniformTransforms, BfeDepthMarker } from "../../src/rust-pkg/bfe_render.js";
import { SceneNode, SceneGroup, SceneWorldChange } from "../../src/rust-render/scene-layout.js";

// Visibility fixtures use the real CPU-only producer. Only GPU registration
// and submission are stood in; there is no JavaScript scene-selection twin.
const sceneWasm = initSync({ module: readFileSync(new URL("../../src/rust-pkg/bfe_render_bg.wasm", import.meta.url)) });

export const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];

export const LENGTHS = {
  camera_unit_view:24,scene_rotation:3,camera_depth_key:DK.LEN,camera_depth_marker:DM.LEN,camera_projection_view:16,camera_uniform_input:UI.LEN,camera_uniform_output:UO.LEN,
  camera_extra_handles:CX.EXTRA_CAMERAS,camera_pack_handles:CX.PACKS,
  camera_light_input:CLI.LEN,camera_light_output:CLO.LEN,
  camera_input:CI.LEN,camera_output:CR.COUNT*CC.LEN,camera_windows:CX.CASCADES*CW.LEN,camera_lights:CX.RAW_LIGHTS*CL.LEN,camera_packs:CX.PACKS*CP.LEN,camera_mirror:CM.LEN,camera_sky:CX.SKY*3,
  frame_facts:F.LEN,frame_state:S.LEN,frame_bindings:R.COUNT*B.LEN,frame_meta:R.COUNT*M.LEN,frame_stats:R.COUNT*T.LEN,frame_lines:3*L.LINES*V.LEN,
  line_input:D.VIEW.LEN+D.LIMIT.SOURCES*D.ROW.LEN,line_data:D.LIMIT.SOURCES*D.LIMIT.VERTICES*3,
  input: 200 + 100 * 8, bones: 64,
  far_input: 1024,
  transparent_keys: 64, transparent_objects: 1 + 2 * 40, transparent_segments: 0,
  fx_input: 2600, fx_data: 16384, fx_matrices: 1024,
  foliage_input: 53,
  post_input: 8 * 221,
  hud_input: 47 + 1024 * 5, hud_position: 64 * 3, hud_uv: 64 * 2, hud_mask_uv: 64 * 2, hud_tint: 64 * 4,
  water_input: 8 * (27 + 32 * 18 + 35) + 2 + 4, water_rings_position: 16 * 3, water_rings_uv: 16 * 2, water_rings_alpha: 16, water_instances: 4096,
  view_input: 1 + 16 * (17 + 32 * 18),
};
export const LAYOUT = { records_at: 200, record_stride: 100, transparent_object_stride: 40 };

export const QUIET = ["camera_lights_frame","camera_view_lights_frame","camera_global_shader_frame","camera_shader_frame","camera_pack_shader_frame","transparent_bucket_shown", "foliage_on"];

// Shader-upload fixtures submit individual phases. Supply the scene-fact
// publication normally performed by coverNear/cover before those calls; the
// production native selector still makes every visibility and ordering choice.
export function prepareSceneFixture(core) {
  if(core.renderer.renderLists)core.registry.items = core.renderer.renderLists.get(core.graphics.scene, 0);
  // These upload fixtures start at scene selection, after the ordinary camera
  // phase. Supply its retained publication without duplicating camera math.
  // Foliage publishes a view before scene selection. Seed that existing
  // publication boundary from authored fixture matrices, without a JS solver.
  const main=core.cameras.main;
  core.cameras.main=function(camera){
    const world=Float64Array.from(camera.matrixWorld.elements),inverse=Float64Array.from(camera.matrixWorldInverse.elements),projection=Float64Array.from(camera.projectionMatrix.elements),projectionInverse=Float64Array.from(camera.projectionMatrixInverse.elements);
    const result=main.call(this,camera);core.views();const out=core.cameraOutput,at=CR.MAIN*CC.LEN;
    out.set(world,at+CC.WORLD);out.set(inverse,at+CC.INVERSE);out.set(projection,at+CC.PROJECTION);out.set(projectionInverse,at+CC.PROJECTION_INVERSE);out.set(world.subarray(12,15),at+CC.POSITION);out[at+CC.LENS+4]=camera.zoom??1;
    this.roles.set(camera,CR.MAIN);return result;
  };
  const select=core.registry.select;
  core.registry.select=function(scene,camera,...args){
    const role=core.cameras.roles.get(camera)??core.cameras.bindings.get(camera);
    if(role!==undefined){core.views();const out=core.cameraOutput,at=role*CC.LEN;
      out.set(camera.projectionMatrix.elements,at+CC.PROJECTION);out.set(camera.matrixWorldInverse.elements,at+CC.INVERSE);out.set(camera.matrixWorld.elements,at+CC.WORLD);out[at+CC.LENS+4]=camera.zoom??1;}
    return select.call(this,scene,camera,...args);
  };
  const publish = core.publishView;
  core.publishView = function(name, renderer, scene, camera, ...args) {
    if (scene && camera) {
      if (scene === core.graphics.scene) {
        if(name === "near" || name === "mirror" || name === "transparent" || name === "mirrorTransparent")core.prepareScene(renderer, scene, camera, renderer.getRenderTarget());
        core.registry.select(scene, camera, 0);
      }
    }
    return publish.call(this, name, renderer, scene, camera, ...args);
  };
  return core;
}

export function standinModule({ lengths = {}, layout = {}, methods = {}, quiet = QUIET, stats = STATS, memoryBytes = 1 << 27 } = {}) {
  const calls = [];
  const nativeScene = new BfeScene();
  const nativeHistory = new BfePoseHistory();
  const nativeTransforms=new BfeUniformTransforms();const nativeDepth=new BfeDepthMarker();
  const memory = { buffer: new ArrayBuffer(memoryBytes) };
  const sizes = { ...LENGTHS, ...lengths }, values = { ...LAYOUT, ...layout }, silent = new Set(quiet);
  const length = (name) => { const n = sizes[name] ?? 1024; return typeof n === "function" ? n() : n; };
  const regions = new Map(), regionBytes = new Map();
  let next=0,sceneStamp=0,prepareKeys=[],prepareAt=0;
  const region = (name) => {
    const bytes=Math.ceil(Math.max(length(name),1024)*8/16)*16;
    if (!regions.has(name) || regionBytes.get(name)<bytes) {
      const previous=regions.get(name),oldBytes=regionBytes.get(name)||0;
      regions.set(name, next);
      regionBytes.set(name,bytes);next += bytes;
      if (next > memory.buffer.byteLength) throw new Error(`stand-in memory too small for ${name}`);
      if(oldBytes)new Uint8Array(memory.buffer,regions.get(name),oldBytes).set(new Uint8Array(memory.buffer,previous,oldBytes));
    }
    return regions.get(name);
  };
  if (typeof methods === "function") methods = methods({ calls, region });
  for (const name of ["changes", "world_changes", "view", "output", "order", "facts", "visible", "groups", "lines", "motion_events", "canopy_bounds", "canopy_view", "canopy_output", "canopy_states", "transform_changes", "transform_values", "transform_phase", "point_changes", "pose", "pose_skins", "tree_input", "tree_info", "place_output", "skin_admit"]) sizes["scene_" + name] = () => nativeScene["scene_" + name + "_len"]();
  const sceneArray = name => new Float64Array(sceneWasm.memory.buffer, nativeScene["scene_" + name + "_ptr"](), nativeScene["scene_" + name + "_len"]());
  const sceneCopy=(name,count)=>{const target=sceneArray(name);target.set(new Float64Array(memory.buffer,region("scene_"+name),Math.min(count,target.length)));};
  const sceneOutput=name=>{const source=sceneArray(name);new Float64Array(memory.buffer,region("scene_"+name),source.length).set(source);};
  const sceneSkins=()=>{const source=new Float32Array(sceneWasm.memory.buffer,nativeScene.scene_pose_skins_ptr(),nativeScene.scene_pose_skins_len());new Float32Array(memory.buffer,region("scene_pose_skins"),source.length).set(source);};
  // History assertions exercise the Rust producer, not a JS history twin.
  for(const name of ["poses","selected","records","current"])sizes["motion_history_"+name]=()=>nativeHistory[name+"_len"]();
  const historyInput=name=>{
    const Type=name==="poses"?Float64Array:name==="current"?Float32Array:Uint32Array;
    const target=new Type(sceneWasm.memory.buffer,nativeHistory[name+"_ptr"](),nativeHistory[name+"_len"]());
    target.set(new Type(memory.buffer,region("motion_history_"+name),target.length));
  };
  methods = {
    scene_place_frame(...args){const count=nativeScene.scene_place_frame(...args);const source=new Uint32Array(sceneWasm.memory.buffer,nativeScene.scene_place_output_ptr(),nativeScene.scene_place_output_len());new Uint32Array(memory.buffer,region('scene_place_output'),source.length).set(source);return count;},
    scene_rotation_frame(...args){nativeScene.scene_rotation_frame(...args);const values=new Float64Array(sceneWasm.memory.buffer,nativeScene.scene_rotation_ptr(),nativeScene.scene_rotation_len());row('scene_rotation').set(values);},
    scene_transform_apply_frame(changes,groups,worlds,transforms){
      sceneCopy("changes",changes*SceneNode.STRIDE);sceneCopy("groups",groups*SceneGroup.STRIDE);sceneCopy("world_changes",worlds*SceneWorldChange.STRIDE);sceneCopy("transform_changes",transforms*TransformChange.STRIDE);
      nativeScene.scene_transform_apply_frame(changes,groups,worlds,transforms);sceneOutput("transform_values");
    },
    scene_transform_frame(){sceneCopy("transform_phase",nativeScene.scene_transform_phase_len());const updates=nativeScene.scene_transform_frame();sceneOutput("transform_values");return updates;},
    scene_transform_updates:()=>nativeScene.scene_transform_updates(),scene_transform_visits:()=>nativeScene.scene_transform_visits(),
    scene_skin_admit_frame(...args){nativeScene.scene_skin_admit_frame(...args);const source=new Float32Array(sceneWasm.memory.buffer,nativeScene.scene_skin_admit_ptr(),nativeScene.scene_skin_admit_len());new Float32Array(memory.buffer,region('scene_skin_admit'),source.length).set(source);},
    scene_motion_locked_frame(record,id,generation){nativeScene.scene_motion_locked_frame(record,id,generation,new Float64Array(memory.buffer,region('motion_input'),length('motion_input')));},
    scene_skeleton_register:(...args)=>nativeScene.scene_skeleton_register(...args),
    scene_point_root_frame:(...args)=>nativeScene.scene_point_root_frame(...args),
    scene_points_apply_frame(count){sceneCopy("point_changes",count*PC.LEN);nativeScene.scene_points_apply_frame(count);},
    scene_pose_register:(...args)=>nativeScene.scene_pose_register(...args),
    scene_pose_frame(){sceneCopy("pose",nativeScene.scene_pose_len());const count=nativeScene.scene_pose_frame();sceneOutput("transform_values");sceneSkins();return count;},
    scene_tree_reserve:words=>nativeScene.scene_tree_reserve(words),
    scene_pose_skin_register:(...args)=>nativeScene.scene_pose_skin_register(...args),
    scene_pose_skin_offset:(...args)=>nativeScene.scene_pose_skin_offset(...args),
    scene_tree_frame(){sceneCopy("tree_input",nativeScene.scene_tree_input_len());sceneCopy("tree_info",nativeScene.scene_tree_info_len());const count=nativeScene.scene_tree_frame();sceneOutput("transform_values");sceneSkins();return count;},
    scene_instances_frame(count){return nativeScene.scene_instances_frame(new Float64Array(memory.buffer,region("input"),length("input")),new Float32Array(memory.buffer,region("bones"),length("bones")),count,values.records_at);},
    scene_motion_model_frame:(skin,id,generation)=>nativeScene.scene_motion_model_frame(nativeHistory,skin,id,generation),
    scene_motion_source_frame(id,slot,generation){
      const ready=nativeScene.scene_motion_source_frame(nativeHistory,id,slot,generation);
      for(const name of ["poses","current"]){const Type=name==="poses"?Float64Array:Float32Array;const source=new Type(sceneWasm.memory.buffer,nativeHistory[name+"_ptr"](),nativeHistory[name+"_len"]());new Type(memory.buffer,region("motion_history_"+name),source.length).set(source);}return ready;
    },
    motion_history_flyer_frame:()=>nativeHistory.flyer_frame(),
    motion_history_flyer_sample_frame:(...args)=>nativeHistory.flyer_sample_frame(...args),
    motion_history_sequence:()=>nativeHistory.sequence(),
    motion_history_skin_frame:count=>nativeHistory.skin_frame(count),
    motion_history_skin_start:id=>nativeHistory.skin_start(id),
    motion_history_model_frame:(skin,bind)=>nativeHistory.model_frame(skin,bind),
    motion_history_reskin_frame:(id,skin)=>nativeHistory.reskin_frame(id,skin),
    motion_history_reset_frame:id=>nativeHistory.reset_frame(id),
    motion_history_release_frame:id=>nativeHistory.release_frame(id),
    motion_history_prepare_frame:count=>{for(const name of ["poses","selected","current"])historyInput(name);nativeHistory.prepare_frame(count);},
    motion_history_commit_frame:()=>nativeHistory.commit_frame(),
    motion_history_write_frame:count=>{
      historyInput("records");
      nativeHistory.write_frame(count,new Float64Array(memory.buffer,region("motion_input"),length("motion_input")),
        new Float32Array(memory.buffer,region("motion_bones"),length("motion_bones")),new Float32Array(memory.buffer,region("motion_previous"),length("motion_previous")));
    },
    scene_canopy_reserve: (count,generation) => nativeScene.scene_canopy_reserve(count,generation),
    scene_canopy_select: () => {
      for (const name of ["canopy_bounds", "canopy_view"]) {
        const target = sceneArray(name);
        target.set(new Float64Array(memory.buffer, region("scene_" + name), target.length));
      }
      const count = nativeScene.scene_canopy_select();
      const output = sceneArray("canopy_output");
      new Float64Array(memory.buffer, region("scene_canopy_output"), output.length).set(output);
      const states = new Float32Array(sceneWasm.memory.buffer, nativeScene.scene_canopy_states_ptr(), nativeScene.scene_canopy_states_len());
      new Float32Array(memory.buffer, region("scene_canopy_states"), states.length).set(states);
      return count;
    },
    scene_mirrored_frame(id,generation){
      const a=row('camera_uniform_input');if(!nativeScene.scene_uniform_world_frame(id,generation,a))return false;
      new Float64Array(sceneWasm.memory.buffer,nativeTransforms.input_ptr(),nativeTransforms.input_len()).set(a);
      return nativeTransforms.mirrored();
    },
    scene_reserve: count => nativeScene.scene_reserve(count),
    scene_groups_reserve:count=>nativeScene.scene_groups_reserve(count),
    scene_select: (count,groups=0,worlds=0) => {
      const changes = sceneArray("changes");
      changes.set(new Float64Array(memory.buffer, region("scene_changes"), Math.min(count * SceneNode.STRIDE, changes.length)));
      sceneArray("view").set(new Float64Array(memory.buffer, region("scene_view"), nativeScene.scene_view_len()));
      const groupInput=sceneArray("groups");groupInput.set(new Float64Array(memory.buffer,region("scene_groups"),Math.min(groups*SceneGroup.STRIDE,groupInput.length)));
      const worldInput=sceneArray("world_changes");worldInput.set(new Float64Array(memory.buffer,region("scene_world_changes"),Math.min(worlds*SceneWorldChange.STRIDE,worldInput.length)));
      const selected = nativeScene.scene_select(count,groups,worlds);
      for (const name of ["output", "order", "facts", "lines", "motion_events"]) {
        const source = sceneArray(name);
        new Float64Array(memory.buffer, region("scene_" + name), source.length).set(source);
      }
      const visible=new Uint32Array(sceneWasm.memory.buffer,nativeScene.scene_visible_ptr(),nativeScene.scene_visible_len());
      new Uint32Array(memory.buffer,region("scene_visible"),visible.length).set(visible);
      return selected;
    }, ...methods,
  };
  const counts = new Map();
  const generic = (name) => (...args) => {
    if (!silent.has(name)) calls.push(name === "add_texture" ? [name] : [name, ...args]);
    if (/(^|_)frame$|^transparent_segment$/.test(name)) return stats;
    const n = counts.get(name) ?? 0;
    counts.set(name, n + 1);
    return n;
  };
  const row=name=>new Float64Array(memory.buffer,region(name),length(name));
  const actorSources=[],actorModels=new Map();let actorHandles=0;
  let packSequence=0,cameraSequence=0;
  const frameMethods={
    prepare_keys_frame:program_warmup_keys,
    prepare_begin_frame(family,cascades,lamps,shadows){prepareKeys=JSON.parse(program_warmup_keys(family,cascades,lamps,shadows));prepareAt=0;},
    prepare_next_frame(){return prepareKeys[prepareAt++]||"";},
    scene_stamp_frame(advance){if(advance)sceneStamp++;return sceneStamp;},
    renderer_diagnostics_frame(){return JSON.stringify({frame:sceneStamp,programs:counts.get('add_owned_program')||0});},
    camera_post_facts_frame(role){const source=row('camera_output'),out=row('post_chain_input'),at=role*CC.LEN;for(const [to,from]of [[PF.PROJECTION,CC.PROJECTION],[PF.VIEW,CC.INVERSE],[PF.WORLD,CC.WORLD]])out.set(source.subarray(at+from,at+from+16),to);},
    camera_canopy_view_frame(role){const source=row('camera_output'),out=row('scene_canopy_view'),at=role*CC.LEN;for(const [to,from]of [[5,CC.PROJECTION],[21,CC.INVERSE],[37,CC.WORLD]])out.set(source.subarray(at+from,at+from+16),to);out.set(source.subarray(at+CC.POSITION,at+CC.POSITION+3),53);},
    camera_foliage_view_frame(role){const source=row('camera_output'),out=row('foliage_input'),at=role*CC.LEN;for(const [to,from]of [[0,CC.PROJECTION],[16,CC.INVERSE],[32,CC.WORLD]])out.set(source.subarray(at+from,at+from+16),to);out.set(source.subarray(at+CC.POSITION,at+CC.POSITION+3),48);},
    camera_scene_view_frame(role){const source=row('camera_output'),out=row('scene_view'),at=role*CC.LEN;out.set(source.subarray(at+CC.PROJECTION,at+CC.PROJECTION+16),5);out.set(source.subarray(at+CC.INVERSE,at+CC.INVERSE+16),21);out.set(source.subarray(at+CC.WORLD+12,at+CC.WORLD+15),37);out[40]=source[at+CC.LENS+4];},

    camera_depth_begin_frame(role){nativeDepth.begin_camera(row('camera_output'),role);},
    camera_depth_request_frame(){new Float64Array(sceneWasm.memory.buffer,nativeDepth.key_ptr(),nativeDepth.key_len()).set(row('camera_depth_key'));return nativeDepth.request();},
    camera_depth_finish_frame(){nativeDepth.finish();row('camera_depth_marker').set(new Float64Array(sceneWasm.memory.buffer,nativeDepth.output_ptr(),nativeDepth.output_len()));},
    camera_scene_uniform_frame(id,generation,role){
      const a=row('camera_uniform_input'),camera=row('camera_output'),at=role*CC.LEN;
      nativeScene.scene_uniform_world_frame(id,generation,a);a.set(camera.subarray(at+CC.INVERSE,at+CC.INVERSE+16),UI.VIEW);a.set(camera.subarray(at+CC.PROJECTION,at+CC.PROJECTION+16),UI.PROJECTION);a.set(camera.subarray(at+CC.WORLD+12,at+CC.WORLD+15),UI.EYE);
      new Float64Array(sceneWasm.memory.buffer,nativeTransforms.input_ptr(),nativeTransforms.input_len()).set(a);nativeTransforms.frame(false);row('camera_uniform_output').set(new Float64Array(sceneWasm.memory.buffer,nativeTransforms.output_ptr(),nativeTransforms.output_len()));
    },
    camera_mirrored_frame(){
      new Float64Array(sceneWasm.memory.buffer,nativeTransforms.input_ptr(),nativeTransforms.input_len()).set(row('camera_uniform_input'));
      return nativeTransforms.mirrored();
    },
    camera_uniform_frame(uv){
      new Float64Array(sceneWasm.memory.buffer,nativeTransforms.input_ptr(),nativeTransforms.input_len()).set(row('camera_uniform_input'));
      nativeTransforms.frame(uv);
      row('camera_uniform_output').set(new Float64Array(sceneWasm.memory.buffer,nativeTransforms.output_ptr(),nativeTransforms.output_len()));
    },
    motion_camera_frame(role,view){
      const out=row('motion_input'),cameras=row('camera_output'),post=row('post_chain_state');
      for(const [to,from]of [[0,role*CC.LEN+CC.PROJECTION],[16,role*CC.LEN+CC.INVERSE],[32,view*CC.LEN+CC.PROJECTION],[48,view*CC.LEN+CC.INVERSE]])out.set(cameras.subarray(from,from+16),to);
      out.set(post.subarray(PS.CURRENT,PS.CURRENT+16),64);out.set(post.subarray(PS.PREVIOUS,PS.PREVIOUS+16),80);
    },
    actor_source(authored,vertices,indices){calls.push(['actor_source',authored]);actorSources.push(JSON.parse(actor_source_description(authored,vertices,indices)));return actorSources.length-1;},
    actor_material(source,authored,skin,cascades,lamps,view,samplers){calls.push(['actor_material',source,authored,skin,cascades,lamps,view,samplers]);const policy=JSON.parse(actor_appearance_description(authored,skin,view?0:cascades,lamps)),facts=actorSources[source],row=new Float64Array(ACTOR.DESCRIPTOR.LEN),d=ACTOR.DESCRIPTOR;row[d.HANDLE]=actorHandles++;row[d.BLENDED]=+policy.blended;row[d.LAMPS]=+policy.lamps;row[d.HEATED]=+policy.heated;row.set(facts.bounds,d.BOUNDS);row[d.COUNT]=facts.indexCount;return row;},
    actor_effect(source,kind,shadows,view,texture,authored){calls.push(['actor_effect',source,kind,shadows,view,texture,authored]);const policy=JSON.parse(actor_effect_description(kind,shadows,view,texture,authored)),key=JSON.stringify([source,kind,shadows,view,texture,authored]);if(!actorModels.has(key))actorModels.set(key,actorModels.size);return Float64Array.of(actorModels.get(key),...actorSources[source].bounds.slice(0,3),...policy.defaults);},
    camera_acquire_frame(){const slot=cameraSequence%CX.EXTRA_CAMERAS,handle=(Math.floor(cameraSequence++/CX.EXTRA_CAMERAS)+1)*CX.EXTRA_CAMERAS+slot;row('camera_extra_handles')[slot]=handle;return handle;},
    camera_release_frame(handle){const a=row('camera_extra_handles'),slot=handle%CX.EXTRA_CAMERAS;if(a[slot]!==handle)return false;a[slot]=-1;return true;},
    camera_import_frame(handle){
      const role=CR.EXTRA0+handle%CX.EXTRA_CAMERAS,a=row('camera_input'),out=row('camera_output'),at=role*CC.LEN;
      const c=a[CI.ORTHOGRAPHIC]?new THREE.OrthographicCamera(a[CI.ORTHO_BOUNDS],a[CI.ORTHO_BOUNDS+1],a[CI.ORTHO_BOUNDS+2],a[CI.ORTHO_BOUNDS+3],a[CI.LENS+2],a[CI.LENS+3]):new THREE.PerspectiveCamera(a[CI.LENS],a[CI.LENS+1],a[CI.LENS+2],a[CI.LENS+3]);
      c.zoom=a[CI.LENS+4];c.filmGauge=a[CI.LENS+5];c.filmOffset=a[CI.LENS+6];
      if(a[CI.LENS+7])c.setViewOffset(a[CI.LENS+8],a[CI.LENS+9],a[CI.LENS+10],a[CI.LENS+11],a[CI.LENS+12],a[CI.LENS+13]);
      c.updateProjectionMatrix();
      c.position.fromArray(a,CI.POSITION);c.quaternion.fromArray(a,CI.QUATERNION);c.scale.fromArray(a,CI.SCALE);c.up.fromArray(a,CI.UP);
      if(a[CI.USE_LOCAL_MATRIX])c.matrix.fromArray(a,CI.LOCAL_MATRIX);else c.updateMatrix();
      if(a[CI.HAS_PARENT])c.matrixWorld.multiplyMatrices(new THREE.Matrix4().fromArray(a,CI.PARENT_WORLD),c.matrix);else c.matrixWorld.copy(c.matrix);
      c.matrixWorldInverse.copy(c.matrixWorld).invert();
      out.set(c.position.toArray(),at+CC.POSITION);out.set(c.up.toArray(),at+CC.UP);out.set(c.quaternion.toArray(),at+CC.QUATERNION);
      out.set(a.subarray(CI.LENS,CI.LENS+14),at+CC.LENS);
      out.set(c.matrixWorld.elements,at+CC.WORLD);out.set(c.matrixWorldInverse.elements,at+CC.INVERSE);
      out.set(c.projectionMatrix.elements,at+CC.PROJECTION);out.set(c.projectionMatrixInverse.elements,at+CC.PROJECTION_INVERSE);
      return role;
    },
    camera_pack_acquire_frame(){const slot=packSequence%CX.PACKS,handle=(Math.floor(packSequence++/CX.PACKS)+1)*CX.PACKS+slot;row('camera_pack_handles')[slot]=handle;return handle;},
    camera_pack_release_frame(handle){const a=row('camera_pack_handles'),slot=handle%CX.PACKS;if(a[slot]!==handle)return false;a[slot]=-1;return true;},
    camera_pack_shader_frame(pack){const out=row('input'),input=row('camera_packs'),at=(pack%CX.PACKS)*CP.LEN;out[70]=input[at+CP.COUNT];for(let i=0;i<out[70]*8;i++)out[72+i]=input[at+CP.VALUES+i];},
    camera_shader_frame(role){const out=row('input'),input=row('camera_output'),at=role*CC.LEN;for(let i=0;i<16;i++){out[i]=input[at+CC.PROJECTION+i];out[16+i]=input[at+CC.INVERSE+i];}for(let i=0;i<3;i++)out[32+i]=input[at+CC.WORLD+12+i];},

    frame_publish_frame(role){calls.push(['frame_publish_frame',role]);return true;},
    // This is transport only. Native frame_plan fixtures prove scheduling;
    // fabricating per-phase calls here would pin a second scheduling policy.
    frame_aux_frame(owner,role){calls.push(['frame_aux_frame',owner,role]);row('frame_stats').set(stats,role*T.LEN);return true;},
  };
  const handler = {
    get(target, name, receiver) {
      if (typeof name !== "string" || Object.prototype.hasOwnProperty.call(target, name)) return Reflect.get(target, name, receiver);
      if (Object.prototype.hasOwnProperty.call(methods, name)) return methods[name];
      if (Object.prototype.hasOwnProperty.call(frameMethods,name))return frameMethods[name];
      if (name.endsWith("_ptr")) return () => region(name.slice(0, -4));
      if (name.endsWith("_len")) return () => length(name.slice(0, -4));
      if (Object.prototype.hasOwnProperty.call(values, name)) return () => values[name];
      if (name === "then" || name === "constructor") return undefined;
      return generic(name);
    },
  };
  class BfeRenderer {
    constructor(context) {
      this.context = context;
      return new Proxy(this, handler);
    }
  }
  return { BfeRenderer, memory, calls, region };
}

// GL enums the bridge and its passes read from the context.
export const GL = {
  VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30, SHADER_TYPE: 0x8b4f, ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89,
  FLOAT: 0x1406, FLOAT_VEC2: 0x8b50, FLOAT_VEC3: 0x8b51, FLOAT_VEC4: 0x8b52, INT: 0x1404, BOOL: 0x8b56, FLOAT_MAT3: 0x8b5b, FLOAT_MAT4: 0x8b5c, SAMPLER_2D: 0x8b5e,
  READ_FRAMEBUFFER: 0x8ca8, DRAW_FRAMEBUFFER: 0x8ca9, COLOR_BUFFER_BIT: 0x4000, DEPTH_BUFFER_BIT: 0x100, STENCIL_BUFFER_BIT: 0x400, NEAREST: 0x2600,
};

// A three program as the bridge reads one (fakeProgram): its active uniforms
// ([name, GL type]) and attributes, a source each stage.
export function fakeProgram(uniforms, attributes = ["position", "uv"]) {
  return { id: Math.floor(Math.random() * 1e6), program: { uniforms, attributes } };
}

// A context answering the program queries for fakeProgram's programs, and
// logging blits into `calls`.
export function standinContext(calls) {
  return {
    ...GL,
    isContextLost:()=>false,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i][0], type: p.uniforms[i][1], size: p.uniforms[i][2] ?? 1 }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: () => [{ type: GL.FRAGMENT_SHADER }, { type: GL.VERTEX_SHADER }],
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => (s.type === GL.VERTEX_SHADER ? "vs" : "fs"),
    blitFramebuffer: (...args) => calls.push(["blitFramebuffer", ...args]),
    bindFramebuffer: (t, fb) => calls.push(["bindFramebuffer", t, fb]),
  };
}

// three's renderer and the page's graphics as the bridge uses them: its
// render settings and info, the render lists (`list`, the near scene's,
// which init() empties), material and texture properties
// (each object's made on first use, with a WebGL texture), the target being
// drawn into (`setTarget` for a test; setRenderTarget, resetState, state
// framebuffer binds and initTexture logged in `calls`), the canvas (an event
// target), and graphics' scenes, cameras and targets (the near scene's
// multisampled, 8x4). `graphics` merges into the graphics stand-in.
export function standinThree(calls, { graphics: extra = {}, context = standinContext(calls) } = {}) {
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene", samples: 2, width: 8, height: 4, depthBuffer: true, resolveDepthBuffer: false, texture: new THREE.Texture() };
  const farTarget = { name: "far", samples: 0, width: 4, height: 4, depthBuffer: true, texture: new THREE.Texture() };
  const properties = new Map();
  properties.set(sceneTarget, { __webglMultisampledFramebuffer: "ms", __webglFramebuffer: "fb" });
  let target = sceneTarget;
  const list = { opaque: [], transmissive: [], transparent: [], init() { this.opaque.length = 0; this.transmissive.length = 0; this.transparent.length = 0; } };
  const renderer = {
    domElement: new EventTarget(),
    autoClear: true, autoClearColor: true, autoClearDepth: true, autoClearStencil: true, sortObjects: true,
    localClippingEnabled: false, clippingPlanes: [],
    info: { autoReset: true, render: { frame: 0, calls: 0 }, reset() { this.render.calls = 0; } },
    renderLists: { get: () => list },
    getContext: () => context,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initRenderTarget() {},
    compile() {},
    initTexture(texture) { calls.push(["initTexture", texture.name]); renderer.properties.get(texture).__version = texture.version; },
    shadowMap: { enabled: false },
    state: { bindFramebuffer: (t, fb) => calls.push(["bindFramebuffer", t, fb]) },
    getRenderTarget: () => target,
    getActiveCubeFace:()=>0,getActiveMipmapLevel:()=>0,getCurrentViewport:v=>v.set(0,0,8,4),
    getScissor:v=>v.set(0,0,400,300),getScissorTest:()=>false,
    clear(){},clearColor(){},clearDepth(){},clearStencil(){},
    setRenderTarget(t) { calls.push(["setRenderTarget", t]); target = t; },
    resetState() { calls.push(["resetState"]); },
    getClearColor: (c) => c.setRGB(0, 0, 0),
    getClearAlpha: () => 1,
    getContextAttributes: () => ({ premultipliedAlpha: true }),
    getViewport: (v) => v.set(0, 0, 400, 300),
    getPixelRatio: () => 2,
  };
  standinResources(renderer,calls);
  const graphics = {
    renderer, scene, camera, farScene: new THREE.Scene(), farCamera:new THREE.PerspectiveCamera(),viewCamera:new THREE.PerspectiveCamera(),cascadeCamera:new THREE.PerspectiveCamera(),shadowCamera:new THREE.PerspectiveCamera(),staticShadowCamera:new THREE.PerspectiveCamera(), viewScene: new THREE.Scene(), sceneTarget, farTarget,
    background: new THREE.Object3D(), settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null, ...extra,
  };
  return { renderer, graphics, scene, camera, list, properties: renderer.properties, setTarget: (t) => { target = t; } };
}

// Presentation tests isolate the command boundary from real WebGL storage.
// Backend smoke tests exercise GpuResources itself, including restore/copies.
export function standinResources(renderer, calls) {
  const fixtureGL=renderer.getContext?.();if(fixtureGL&&!fixtureGL.isContextLost){fixtureGL.isContextLost=()=>false;renderer.getContext=()=>fixtureGL;}
  renderer.domElement ??= new EventTarget();renderer.dispose ??= ()=>{};
  renderer.getActiveCubeFace ??= ()=>0;renderer.getActiveMipmapLevel ??= ()=>0;
  renderer.getCurrentViewport ??= v=>v.set(0,0,800,600);
  renderer.getViewport ??= v=>v.set(0,0,800,600);renderer.getScissor ??= v=>v.set(0,0,800,600);
  renderer.getScissorTest ??= ()=>false;renderer.getContextAttributes ??= ()=>({premultipliedAlpha:true});renderer.getPixelRatio ??= ()=>1;
  renderer.getClearColor ??= c=>c.setRGB(0,0,0);renderer.getClearAlpha ??= ()=>1;
  for(const name of ['clear','clearColor','clearDepth','clearStencil'])renderer[name] ??= ()=>{};
  const ids = new WeakMap(),handles=[]; let next = 0;
  const native=()=>{const id=next++;handles[id]={native:id};return id;};
  const owner = {
    gpu: { epoch: () => 0, retired_sample: native, texture:id=>handles[id],
      skin_image:(...args)=>{calls.push(["skin_image",...args]);return native();},
      release_skin_image:(...args)=>{calls.push(["release_skin_image",...args]);},
      effect_data:(...args)=>{calls.push(["effect_data",...args]);return native();},
      effect_noise:kind=>{calls.push(["effect_noise",kind]);return native();},
      canopy_state:(width,pixels)=>{calls.push(["canopy_state",width,pixels]);return native();},
      update_canopy_state:(id,pixels)=>{calls.push(["update_canopy_state",id,pixels]);},
      fountain_atlas:native,effect_pixels(){},snow_pixels(){},weather_cover:()=>{},dispose_sample(){} },
    lightningSample(texture){return this.staticSample(texture);},
    effectTarget(kind){return {nativeTarget:native(),texture:native(),depthTexture:-1,width:1,height:1,samples:0};},
    resizeEffectTarget(target,width,height){target.width=width;target.height=height;},
    disposeEffectTarget(){},
    targetId:target=>{target.viewport ??= new THREE.Vector4(0,0,target.width||1,target.height||1);target.scissor ??= target.viewport.clone();if(!ids.has(target))ids.set(target,next++);return ids.get(target);},

    handle: texture => typeof texture === "number" ? handles[texture] : renderer.properties.get(texture).__webglTexture,
    owns: texture => ids.has(texture),
    staticAnisotropy(value) { calls.push(["static_anisotropy",value]); },
    staticSample(texture) { return typeof texture === "number" ? texture : this.textureId(texture); },
    textureId(texture) {
      if(typeof texture==="number")return texture;
      if(!texture)return -1;
      renderer.initTexture(texture);
      if (!ids.has(texture)) ids.set(texture,next++);
      const id=ids.get(texture);handles[id]=renderer.properties.get(texture).__webglTexture;
      return id;
    },
    resolve(target) { if (target?.samples) calls.push(["resolveTarget",target]); },
    copy(source,target,mask,mipmaps=false) { calls.push(["copyTarget",source,target,mask,mipmaps]); },
    beforeInterface() {},
  };
  renderer.resources=owner; return owner;
}
