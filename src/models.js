import * as THREE from "three";
import {actorAsset,actorNode,glowNode} from "./actor-resources.js";
import { actorMaterial } from "./materials.js";

// Small static models as tools/extract.py packs them (write_meshes): each
// vertex is position, normal, uv (8 floats), and each model is a list of
// parts, one per material. Returns model name -> parts, each with its own
// geometry, texture name, material flags, and the bone it hangs from.
export function modelPack(records, buffer, indexBuffer) {
  const vertices=new Float32Array(buffer),indices=new Uint32Array(indexBuffer),models=new Map();
  for(const [name,parts]of Object.entries(records||{}))models.set(name,parts.map(part=>({...part,asset:actorAsset({kind:'model',parts:[part]},vertices,indices)})));
  return models;
}

// The material a part's MTRL flags ask for: lit like a soldier (or unlit),
// cut out (opaque, as the engine draws hard-edged parts), blended or added,
// double sided, scrolling.
export function partMaterial(part, texture) {
  return actorMaterial('dynamic',{map:texture(part.texture),flags:part.flags??1,scroll:part.scroll});
}

// A group of a model's parts, each with the material `material(part)` makes.
export function modelGroup(parts, material) {
  const group = new THREE.Group();
  for(const part of parts){const look=material(part);group.add(look.actorGlow?glowNode(part.asset,look):actorNode(part.asset,look));}
  return group;
}
