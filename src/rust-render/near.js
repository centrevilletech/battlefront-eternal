// The near scene drawn in place of three's render of it (graphics.js asks
// the core: coverNear), on a frame when nothing in it is left for three.
//
// What three's render would do first is done (the scene's matrices where it
// keeps them, its onBeforeRender: the point lights packed, the terrain's
// patches and the canopies chosen for the camera), then the scene is walked
// as three's projectObject walks it (what is shown, on the camera's layers,
// in its frustum) for what three would list: an item whose material is
// shown. Every pass hides from three what it draws (its materials, its
// meshes, or their layers), so three would list only the far scene's image
// (graphics.background), the objects the transparent pass takes out of its
// list (whose keys, three's group order, render order, z and id, are made
// here as projectObject makes them) and whatever no pass draws. At the
// first of those, the walk stops and three renders the scene as before.
//
// Otherwise the renderer draws all of it, as three would have: the target
// cleared and the far scene's image drawn into it (this pass, the "begin"
// phase: three's program for the image's material, as it compiled it), the
// near phase's passes, then the transparent phase with the walk's items;
// three's state is reset once, when three is next used. Then what three's
// render does as it ends: the target resolved, the scene's onAfterRender.
// three's render list for the scene is emptied (it listed nothing), and its
// frame count moves on as a render's would.
import * as THREE from "three";
import { plainProgram, writeUniform } from "./post.js";

// post.rs's step layout, and its target for the framebuffer bound now.
const STEP = { PROGRAM: 0, TARGET: 1, WIDTH: 2, HEIGHT: 3, UNIFORMS: 4, FIRST: 5 };
const UNIFORM = 18, MAX_UNIFORMS = 12, TARGET_BOUND = -2;
const ATTRIBUTES = "position:0,uv:1";
// The transparent pass's records a frame (transparent.rs MAX_OBJECTS).
const MAX_ITEMS = 512;

class NearPass {
  constructor(core) {
    this.core = core;
    this.name = "near";
    this.phase = "begin";
    // Behind ?rustNear for now: it draws the near scene only on frames with
    // nothing left for three (a dissolving body, a held gun the instances
    // can't draw yet send the frame back to three), so on busy maps it
    // saves little yet.
    this.allowed = new URLSearchParams(location.search).has("rustNear");
    this.on = this.allowed;
    this.input = false;
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

  // graphics.js: the near scene's render, drawn here when nothing in it is
  // left for three; true when it was.
  cover(renderer, scene, camera) {
    const core = this.core, graphics = core.graphics;
    if (!core.enabled || !core.active || core.lost || this.on === false || !core.supported()) return false;
    const target = renderer.getRenderTarget();
    // Only the frame's own render of the near scene, in the configuration
    // three's would draw as walked here: its clear (no background or fog of
    // three's), its sort, and the far scene's image in it.
    if (target !== graphics.sceneTarget || camera !== graphics.camera || scene !== graphics.scene || graphics.background.parent !== scene) return false;
    if (!renderer.autoClear || !renderer.autoClearColor || !renderer.autoClearDepth || !renderer.autoClearStencil || !renderer.sortObjects) return false;
    if (scene.background || scene.fog || scene.overrideMaterial || renderer.localClippingEnabled || renderer.clippingPlanes?.length) return false;
    // The far scene's image needs three's program for its material (three
    // has drawn it once).
    const program = plainProgram(core, graphics.background.material, ATTRIBUTES);
    if (!program || program.uniforms.length > MAX_UNIFORMS) return false;
    try {
      if (scene.matrixWorldAutoUpdate === true) scene.updateMatrixWorld();
      if (camera.parent === null && camera.matrixWorldAutoUpdate === true) camera.updateMatrixWorld();
      this.screen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
      this.frustum.setFromProjectionMatrix(this.screen);
      // What stopped the last walk, still in view as it was: three renders
      // the scene, unwalked (and its onBeforeRender is three's alone).
      if (this.stillStops(this.stopped, scene, camera)) { this.left++; this.early++; return false; }
      scene.onBeforeRender(renderer, scene, camera, target);
      this.items.length = 0;
      this.stopped = null;
      if (!this.walk(scene, camera, 0) || !this.writeImage(program, camera)) { this.left++; return false; }
      // three's render list of the scene: empty, as three listed nothing.
      renderer.renderLists.get(scene, 0).init();
      renderer.info.render.frame++;
      if (renderer.info.autoReset) renderer.info.reset();
      core.phase("begin", renderer, scene, camera, true);
      core.phase("near", renderer, scene, camera, true, this.items);
      core.resolve(target);
      scene.onAfterRender(renderer, scene, camera);
      this.drawn++;
      return true;
    } catch (error) {
      core.fail(error, renderer, target);
      return false;
    }
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

  // Whether an object that stopped a walk still would: in the scene and
  // shown (its parents too), on the camera's layers, in view, with a shown
  // material three draws itself (an opaque one, a group's, or one that
  // blends that the transparent pass has not taken).
  stillStops(object, scene, camera) {
    if (!object || object === this.core.graphics.background) return false;
    let o = object;
    for (; o && o !== scene; o = o.parent) if (o.visible === false) return false;
    if (o !== scene || !object.layers.test(camera.layers)) return false;
    const material = object.material;
    const shown = Array.isArray(material) ? object.geometry.groups.some((g) => material[g.materialIndex]?.visible)
      : Boolean(material?.visible) && (!material.transparent || material.transmission > 0 || !this.core.byName.transparent.taken.get(object));
    if (!shown) return false;
    return !object.frustumCulled || (object.isSprite ? this.frustum.intersectsSprite(object) : this.frustum.intersectsObject(object));
  }

  // An item three would list: the far scene's image (drawn by this pass), or
  // one the transparent pass takes, with three's key; else three draws it.
  item(object, material, groupOrder) {
    if (object === this.core.graphics.background) return true;
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
  writeImage(program, camera) {
    const core = this.core, background = core.graphics.background;
    if (!this.input) { core.addView("postInput", Float64Array, () => core.rust.post_input_ptr(), () => core.rust.post_input_len()); this.input = true; }
    core.views();
    const out = core.postInput;
    out[STEP.PROGRAM] = program.id; out[STEP.TARGET] = TARGET_BOUND; out[STEP.WIDTH] = 0; out[STEP.HEIGHT] = 0;
    out[STEP.UNIFORMS] = program.uniforms.length;
    for (let k = 0; k < program.uniforms.length; k++) {
      if (!writeUniform(core, out, STEP.FIRST + k * UNIFORM, k, program.uniforms[k], background.material, background, camera)) { this.stopped = background; return false; }
    }
    return true;
  }

  // The "begin" phase: three's clear colour (WebGLBackground's: premultiplied
  // by its alpha as the context is), and the image's step (writeImage).
  draw(ctx) {
    const renderer = ctx.renderer, clear = this.clear;
    renderer.getClearColor(this.color);
    const alpha = renderer.getClearAlpha(), pre = renderer.getContextAttributes()?.premultipliedAlpha !== false ? alpha : 1;
    clear[0] = this.color.r * pre; clear[1] = this.color.g * pre; clear[2] = this.color.b * pre; clear[3] = alpha;
    ctx.clear = clear;
    ctx.steps = 1;
  }

  // Off (this pass alone): three renders the near scene.
  setActive(on) {
    this.on = on && this.allowed;
  }
}

export function nearPass(core) {
  return new NearPass(core);
}
