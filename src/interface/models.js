import { surfaceMaterial } from "../rust-render/interface-material.js";
import * as THREE from "three";
import {repeatImage} from "./images.js";
import {actorAsset} from "../actor-resources.js";

// The interface's 3D models (the spawn screen's room, the team logos, a
// class's body) and how the 2004 interface draws them. The spawn screen and
// the scoreboard draw theirs with it.
//
// An interface model hangs at its screen point, on the ray through it: the
// 2004 game multiplies the point's place on the interface plane (886.81
// units out) by a factor, then scales the model. On the plane that is the
// model scaled by scale / factor.
// - The room (shell_cube): factor 0.009, scale 1.6, at the screen's centre,
//   its own origin on the axis (the corner sits left of centre). That puts
//   the eye 5 room units in front of the origin; the grid lines of all
//   three retail screenshots fit it.
// - The logos: size parameter (0.25 to 0.5) x 221 units on the plane, which
//   gives the screenshots' 78 and 40 pixels across.
// - Bodies: 0.936 / (0.006 / (W/800)) = 200 units per metre, the clone
//   trooper's 385 pixels.
//
// Models are lit as the interface lights them, whatever their material
// says: ambient (144, 144, 144) and a white light from the front left, above
// (the screenshots' lit side), clamped at full. The room isn't lit; its
// vertices' own tints darken it (roomTints).
//
// A stage of the port's own (the page spawn screen, src/spawn-screen/page)
// may light them more: keyTint colours the light, rim adds light round the
// model's edges (its strength falling off by rimPower), recolor redraws a
// model in one colour by its brightness ([r, g, b, amount]), and cut hides
// what is above a height in the world (metres there), its edge lit in
// cutGlow, to build a body up from its feet. Unset, they leave the
// original's light as it is.

export const PLANE = 1024 / (2 * Math.tan(Math.PI / 6)); // 886.81
export const ROOM_SCALE = 1.6 / 0.009;
export const LOGO_UNITS = 221.3;
export const BODY_UNITS = 0.936 / (0.006 / (1024 / 800));
export const DROIDEKA_UNITS = 0.9 * BODY_UNITS;
// Interface light/cutout defaults and body material policy are native. These
// options are authored UI inputs; animated values use the generated live row.
export function interfaceMaterial(options={}){
  return surfaceMaterial("model",{blend:true,depth:true,...options,side:THREE.DoubleSide});
}

// A texture the material scrolls: repeating, set before its first upload.
export function repeating(texture){return repeatImage(texture);}

// The room's vertex tints, which the common model pack doesn't carry: grey
// 192, and 68 toward the open end of the right-hand wall (its last two
// columns of vertices, x > 0.4 and z > 1 in the model's units). The dark
// end is the screenshots' darker right edge.
export function roomTints(node){const old=node.userData.actorAsset;if(!old||old.authored.roomTints)return;node.userData.actorAsset=actorAsset({...old.authored,roomTints:true},old.vertices,old.indices);}

// The meshes of a model, in order (a hologram-pack model is one per part).
export function meshesOf(object) {
  const meshes = [];
  object?.traverse((child) => {
    if (child.isMesh || child.userData.actorAsset) meshes.push(child);
  });
  return meshes;
}

// Every mesh of an object gets the interface's material. parts: the model's
// records (common.json holograms) for each mesh's texture and scroll (a
// mesh's own userData.scroll wins); texture(name): the HUD's textures (the
// 4x atlases apply). Returns the materials made, for tinting and disposal.
export function dressModel(object, parts, texture, options = {}) {
  const materials = [];
  meshesOf(object).forEach((mesh, i) => {
    const part = parts?.[i] || parts?.[0] || {};
    let map = (part.texture?texture(part.texture):null)??mesh.material?.map??null;
    const scroll = mesh.userData.scroll ?? part.scroll ?? null;
    if (scroll) map=repeating(map);
    const material = interfaceMaterial({ map, scroll, ...options });
    mesh.material = material;
    mesh.frustumCulled = false;
    materials.push(material);
  });
  return materials;
}

// A body's own materials swapped for interface ones on the same textures
// (skinning kept); cut-out parts stay cut out.
export function dressBody(object) {
  const made = [];
  for (const mesh of meshesOf(object)) {
    // The shadow proxy draws only in the shadow pass.
    if (!mesh.layers.isEnabled(0)) continue;
    const swap = (material) => {
      if(material?.interfaceAppearance){made.push(material);return material;}
      const map=material?.map??null;
      const dressed=interfaceMaterial({map,fromActor:material.authored});
      made.push(dressed);
      return dressed;
    };
    mesh.material = Array.isArray(mesh.material) ? mesh.material.map(swap) : swap(mesh.material);
    mesh.frustumCulled = false;
  }
  return made;
}

// The camera the models are drawn with: the HUD's 46.8 degrees up, at the
// window's own shape, so a model keeps its proportions on any screen.
export class InterfaceCamera {
  constructor() {
    this.camera = new THREE.PerspectiveCamera(2 * THREE.MathUtils.radToDeg(Math.atan(0.75 * Math.tan(Math.PI / 6))), 4 / 3, 1, 20000);
  }

  // aspect: the canvas's width / height; H: the interface height.
  setView(aspect, H) {
    this.aspect = aspect;
    this.H = H;
    if (this.camera.aspect !== aspect) {
      this.camera.aspect = aspect;
      this.camera.updateProjectionMatrix();
    }
  }

  // An interface point (x, y) on the plane, in the camera's space: x keeps
  // its place across the screen and y its place down it.
  place(x, y, out = new THREE.Vector3()) {
    return out.set((x - 512) * 0.75 * this.aspect, (this.H / 2 - y) * (768 / this.H), -PLANE);
  }
}
