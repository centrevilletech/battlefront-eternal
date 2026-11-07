// A water reflection uses the same Rust ownership walk as the main scene,
// with its mirrored camera and target, followed by ordered transparency.
import { NearPass } from "./near.js";

class MirrorPass extends NearPass {
  constructor(core) {
    super(core);
    this.name = "mirrorCover";
    this.scenePhase = "mirror";
  }

  // A water mirror's render of the near scene, into a target.
  ours(renderer, scene, camera, target) {
    return camera.userData?.mirror === true && scene === this.core.graphics.scene && Boolean(target);
  }


}

export function mirrorPass(core) {
  return new MirrorPass(core);
}
