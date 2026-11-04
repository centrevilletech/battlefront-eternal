// A water mirror's render of the near scene (water.js reflect at Ultra: the
// scene from the camera mirrored in the surface, into the reflection's
// target) drawn in place of three's render of it, as near.js draws the
// frame's: on a render with nothing in it left for three. The scene is
// walked as three's projectObject walks it for the mirror camera; where
// three would list only the far scene's image (the mirror's own, which
// water.js puts in the image's map), the near phase's hook and what the
// transparent pass takes, the renderer draws all of it: the target cleared
// as three clears it, the image (the begin phase), the mirror phase's passes
// and its transparent phase with the walk's items, three's state reset once,
// when three is next used. Else three renders it as before (the mirror
// phase inside it). ?rustMirror=0 leaves every mirror render to three.
import { NearPass } from "./near.js";

class MirrorPass extends NearPass {
  constructor(core) {
    super(core);
    this.name = "mirrorCover";
    this.scenePhase = "mirror";
    this.allowed = new URLSearchParams(location.search).get("rustMirror") !== "0";
    this.on = this.allowed;
  }

  // A water mirror's render of the near scene, into a target.
  ours(renderer, scene, camera, target) {
    return camera.userData?.mirror === true && scene === this.core.graphics.scene && Boolean(target);
  }

  // The begin phase's values are the near pass's (near.js draw: three's
  // clear colour, one step): nothing of its own.
  draw() {}
}

export function mirrorPass(core) {
  return new MirrorPass(core);
}
