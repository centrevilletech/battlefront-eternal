// Draw the battle scene in Rust: opaque passes, far image, then ordered
// transparency. The walk validates that every visible item has a Rust owner.
// Three still maintains scene transforms, target allocation and resolve state.
import * as THREE from "three";
import { writeScreenStep } from "./screen.js";
import { plainProgram, plainProgramKnown } from "./post.js";

// post.rs's step layout, and its target for the framebuffer bound now.
const MAX_UNIFORMS = 12;
const ATTRIBUTES = "position:0,uv:1";
// The transparent pass's records a frame (transparent.rs MAX_OBJECTS).
const MAX_ITEMS = 512;

export class NearPass {
  constructor(core) {
    this.core = core;
    this.name = "near";
    this.phase = "begin";
    // The phase that draws the scene's passes after the begin phase (a
    // mirror's render: mirror.js).
    this.scenePhase = "near";
    this.frustum = new THREE.Frustum();
    this.screen = new THREE.Matrix4();
    this.point = new THREE.Vector4();
    this.color = new THREE.Color();
    this.clear = new Float64Array(4);
    // This frame's walk: the transparent pass's items (kept from frame to
    // frame), how many, and what stopped it (for the measurement tools).
    this.pool = [];
    this.items = [];
    this.stopped = null;
    this.drawn = 0;
    this.left = 0;
    this.early = 0;
  }

  // Validate full ownership, clear, draw opaque geometry and ordered transparency.
  cover(renderer, scene, camera) {
    const core = this.core, graphics = core.graphics;
    if (core.contextLost) return false;
    const target = renderer.getRenderTarget();
    // Only the frame's own render of the near scene (ours()), in the
    // configuration three's would draw as walked here: its clear (no
    // background or fog of three's), its sort, and the far scene's image in it.
    if (!this.ours(renderer, scene, camera, target) || graphics.background.parent !== scene) return false;
    if (!renderer.autoClear || !renderer.autoClearColor || !renderer.autoClearDepth || !renderer.autoClearStencil || !renderer.sortObjects) return false;
    if (scene.background || scene.fog || scene.overrideMaterial || renderer.localClippingEnabled || renderer.clippingPlanes?.length) return false;
    // Three compiles the unported far-image material. Registering its Rust
    // program changes GL state, even if validation subsequently fails.
    const fresh = !plainProgramKnown(core, graphics.background.material);
    const left = () => { if (fresh) core.stale = true; return false; };
    const program = plainProgram(core, graphics.background.material, ATTRIBUTES);
    if (!program || program.uniforms.length > MAX_UNIFORMS) return left();
    try {
      if (scene.matrixWorldAutoUpdate === true) scene.updateMatrixWorld();
      if (camera.parent === null && camera.matrixWorldAutoUpdate === true) camera.updateMatrixWorld();
      this.screen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
      this.frustum.setFromProjectionMatrix(this.screen);
      scene.onBeforeRender(renderer, scene, camera, target);
      if (!core.primed) {
        core.primed = true;
        core.phase(this.scenePhase, renderer, scene, camera, true, null, { priming: true });
      }
      this.items.length = 0;
      this.stopped = null;
      if (!this.walk(scene, camera, 0)) { this.left++; return left(); }
      renderer.info.render.frame++;
      if (renderer.info.autoReset) renderer.info.reset();
      core.phase("begin", renderer, scene, camera, true);
      core.phase(this.scenePhase, renderer, scene, camera, true, this.items);
      core.resolve(target);
      scene.onAfterRender(renderer, scene, camera);
      this.drawn++;
      return true;
    } catch (error) {
      core.fail(error, renderer);
      return false;
    }
  }

  // The render this pass draws in three's place: the frame's own of the near
  // scene.
  ours(renderer, scene, camera, target) {
    const graphics = this.core.graphics;
    return target === graphics.sceneTarget && camera === graphics.camera && scene === graphics.scene;
  }

  // projectObject's walk: a hidden object hides its subtree; one on other
  // layers is not listed, but its children may be; a group's render order
  // orders what is listed below it; an LOD picks its level. False at the
  // first item three would list that the renderer doesn't draw.
  walk(object, camera, groupOrder) {
    if (object.visible === false) return true;
    if (object.layers.test(camera.layers)) {
      if (object.isGroup) groupOrder = object.renderOrder;
      else if (object.isLOD) { if (object.autoUpdate === true) object.update(camera); }
      else if (object.isSprite) {
        if (object.material.visible && (!object.frustumCulled || this.frustum.intersectsSprite(object)) && !this.item(object, object.material, groupOrder)) return false;
      } else if (object.isMesh || object.isLine || object.isPoints) {
        const material = object.material;
        if (Array.isArray(material)) {
          // Each group with a shown material is an item of three's own.
          for (const group of object.geometry.groups) {
            const m = material[group.materialIndex];
            if (m && m.visible && (!object.frustumCulled || this.frustum.intersectsObject(object))) { this.stopped = object; return false; }
          }
        } else if (material.visible && (!object.frustumCulled || this.frustum.intersectsObject(object)) && !this.item(object, material, groupOrder)) return false;
      }
    }
    const children = object.children;
    for (let i = 0; i < children.length; i++) if (!this.walk(children[i], camera, groupOrder)) return false;
    return true;
  }

  item(object, material, groupOrder) {
    if (object === this.core.graphics.background) return true;
    if (object.isLine) {
      if (this.core.byName.lines.owns(object)) return true;
      this.stopped=object;return false;
    }
    if (!material.transparent || material.transmission > 0 || this.items.length >= MAX_ITEMS) { this.stopped = object; return false; }
    // z as projectObject takes it: a sprite's position, else the centre of
    // its bounding sphere (the object's own, where it has one), through its
    // world matrix, then the camera's projection and view (no divide).
    const point = this.point;
    if (object.isSprite) point.setFromMatrixPosition(object.matrixWorld);
    else {
      let sphere;
      if (object.boundingSphere !== undefined) { if (object.boundingSphere === null) object.computeBoundingSphere(); sphere = object.boundingSphere; }
      else { if (object.geometry.boundingSphere === null) object.geometry.computeBoundingSphere(); sphere = object.geometry.boundingSphere; }
      point.set(sphere.center.x, sphere.center.y, sphere.center.z, 1).applyMatrix4(object.matrixWorld);
    }
    point.applyMatrix4(this.screen);
    const item = this.pool[this.items.length] ??= {};
    item.object = object; item.material = material; item.geometry = object.geometry; item.group = null;
    item.groupOrder = groupOrder; item.renderOrder = object.renderOrder; item.z = point.z; item.id = object.id;
    if (!this.core.byName.transparent.takes(item)) { this.stopped = object; return false; }
    this.items.push(item);
    return true;
  }

  // The far scene's image as a post step into the target bound now (post.rs):
  // three's program for its material and every uniform it reads; false when
  // one has no value the renderer can set.
  writeImage(camera) {
    const core=this.core,background=core.graphics.background;
    const program=plainProgram(core,background.material,ATTRIBUTES);
    if(!program || !writeScreenStep(core,0,program,background.material,background,camera)) core.fail(new Error("Cannot draw far-scene image"));
  }

  // Clear before geometry. The depth-tested far image follows opaque draws.
  draw(ctx) {
    const renderer = ctx.renderer, clear = this.clear;
    renderer.getClearColor(this.color);
    const alpha = renderer.getClearAlpha(), pre = renderer.getContextAttributes()?.premultipliedAlpha !== false ? alpha : 1;
    clear[0] = this.color.r * pre; clear[1] = this.color.g * pre; clear[2] = this.color.b * pre; clear[3] = alpha;
    ctx.clear = clear;
    ctx.steps = 0;
  }

}

export function nearPass(core) {
  return new NearPass(core);
}
