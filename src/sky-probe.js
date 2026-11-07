import * as THREE from 'three';

// The map's own sky (its dome and the models on its horizon, drawn as the far
// scene draws them) in a small cube around the eye, for a sea that reflects
// it and fades into it far off (ocean-storm.js, on a sea it doesn't storm
// over). The sky stands around the eye, so a capture holds as the eye moves
// about; it is taken again once a second of the sea's time, or at once when
// the eye climbs or sinks 4 m (the horizon models' MovementScale), which also
// picks up scrolling sky textures and ones that arrive late. Only the sky's
// meshes are on its layer, so a capture draws nothing else.
export const PROBE_LAYER = 3; // graphics.js uses layers 1 (hidden) and 2 (static)

let generation=0;
export class SkyProbe {
  constructor(graphics,size=128,slot=0) {
    this.graphics=graphics;this.slot=slot;this.generation=++generation;
    this.target=null;this.size=size;this.texture=-1;this.resources=null;
    this.camera=new THREE.CubeCamera(0.05,50,null);
    for (const face of this.camera.children) face.layers.set(PROBE_LAYER);
    this.layers = graphics.skyLayers?.length ? graphics.skyLayers : graphics.sky ? [{ mesh: graphics.sky, follow: null }] : [];
    for (const { mesh } of this.layers) mesh.layers.enable(PROBE_LAYER);
    this.enabled = false;
    this.captures = 0;
  }

  // Camera math remains a scene input. Rust owns the cube storage and mip
  // recipe; native frame scheduling receives numeric target and face facts.
  prepareResources(resources) {
    if(this.resources)return;this.resources=resources;this.target=resources.probeTarget(this.size);this.texture=this.target.texture;this.camera.renderTarget=this.target;
    for(let i=0;i<6;i++)this.camera.children[i].userData.nativeProbe={target:this.target,face:i,last:i===5};
  }
  update(time,camera) {const frame=this.graphics.rust.frame;if(frame.active)this.prepareResources(this.graphics.renderer.resources);frame.probe(this,time,camera);}


  dispose() {
    for (const { mesh } of this.layers) mesh.layers.disable(PROBE_LAYER);
    this.layers = [];
    if(this.resources)this.resources.disposeEffectTarget(this.target);
  }
}
