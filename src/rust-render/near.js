// Draw the battle scene in Rust: opaque passes, far image, then ordered
// transparency. Retained scene facts select the remaining transparent inputs.
// Scene transforms remain published inputs; the Rust bank owns targets and resolves.
import * as THREE from "three";
import { writeScreenStep } from "./screen.js";
import { ownedScreenProgram } from "./post.js";

// post.rs's step layout, and its target for the framebuffer bound now.
const MAX_UNIFORMS = 12;
const ATTRIBUTES = "position:0,uv:1";

export class NearPass {
  constructor(core) {
    this.core = core;
    this.name = "near";
    this.phase = "near";
    // Mirror views use the same producer with a different semantic role.
    this.scenePhase = "near";
    // This frame's walk: the transparent pass's items (kept from frame to
    // frame), how many, and what stopped it (for the measurement tools).
    this.drawn = 0;
  }

  // Validate full ownership, clear, draw opaque geometry and ordered transparency.
  // WebGLRenderer.render's near-scene role: update the scene and invoke its
  // hooks, then validate every drawable before submitting the required phase.
  // A rejected item is an error at its owner, never an instruction to draw it
  // through the interface renderer. Reflections use the same ownership check.
  cover(renderer, scene, camera) {
    const core = this.core, graphics = core.graphics;
    if (core.contextLost) return false;
    const target = renderer.getRenderTarget();
    // Only the frame's own render of the near scene (ours()), in the
    // configuration three's would draw as walked here: its clear (no
    // background or fog of three's), its sort, and the far scene's image in it.
    if (!this.ours(renderer, scene, camera, target) || graphics.background.parent !== scene) return false;
    if (!renderer.autoClear) return false;
    if (scene.background || scene.fog || scene.overrideMaterial) return false;
    const program = ownedScreenProgram(core, graphics.background.material, ATTRIBUTES);
    if (!program || program.uniforms.length > MAX_UNIFORMS) return false;
    try {
      if (scene.matrixWorldAutoUpdate === true) scene.updateMatrixWorld();
      if (camera.parent === null && camera.matrixWorldAutoUpdate === true) camera.updateMatrixWorld();
      core.prepareScene(renderer, scene, camera, target);
      scene.onBeforeRender(renderer, scene, camera, target);
      if (!core.primed) {
        core.primed = true;
        core.publishView(this.scenePhase, renderer, scene, camera, null, { priming: true });
      }
      const items = core.registry.near(scene, camera);
      core.rust.scene_stamp_frame(true);
      const ctx=core.publishView(this.scenePhase, renderer, scene, camera, items);
      core.frame.sceneTail(ctx,this);
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

  // The far scene's image as a post step into the target bound now (post.rs):
  // three's program for its material and every uniform it reads; false when
  // one has no value the renderer can set.
  // The far image belongs after opaque depth and before transparent copies.
  // It must use the active camera, including a mirror, and must not write depth.
  // Submitting it earlier changes MSAA edge samples even when all RGB values
  // look equivalent in an ordinary single-sample capture.
  writeImage(camera) {
    const core=this.core,background=core.graphics.background;
    const program=ownedScreenProgram(core,background.material,ATTRIBUTES);
    if(!program || !writeScreenStep(core,0,program,background.material,background,camera)) core.fail(new Error("Cannot draw far-scene image"));
  }

}

export function nearPass(core) {
  return new NearPass(core);
}
