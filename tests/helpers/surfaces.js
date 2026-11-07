// Component fixtures use the battle's shared module and enclosing frame. Only
// authored camera and source facts are supplied here; Rust advances the state.
import * as THREE from 'three';
import { loadEnvironment, SurfaceOwner, VisibilityHost } from '../../src/environment/module.js';
import { SurfaceHost } from '../../src/environment/surfaces.js';
import { SurfaceFrame } from '../../src/environment/frame.js';
import { EnvironmentWakes } from '../../src/environment-volume.js';
import { SurfaceFrame as FRAME, EnvironmentFrame as ENV, EnvironmentInstance as INSTANCE, View } from '../../src/environment/layout.js';
await loadEnvironment();
export { SurfaceOwner, SurfaceHost };
const frames=new WeakMap(),empty=[],cameraDefault=new THREE.PerspectiveCamera();
const battles=new WeakMap();
const fields={Water:'water',WaterShore:'shore',Foliage:'foliage',Weather:'weather',Snow:'snow',Atmosphere:'atmosphere',SkyFlyers:'sky',EnvironmentEffects:'environment'};

export function wakeFixture() {
  const owner=new SurfaceOwner(),host=new VisibilityHost(owner,1);
  host.config[INSTANCE.SPHERE+3]=24;host.start();
  const wakes=new EnvironmentWakes(host);
  const environment={prepareFrame(dt,camera,actors){
    host.view.set(camera.matrixWorld.elements,View.CAMERA);host.view.set(camera.matrixWorldInverse.elements,View.INVERSE);host.view.set(camera.projectionMatrix.elements,View.PROJECTION);
    camera.position.toArray(host.view,View.POSITION);host.frame[ENV.DT]=dt;host.frame[ENV.REACH]=100;host.frame[ENV.ENHANCED]=1;wakes.prepare(actors);
  },publishFrame(){wakes.synced();}};
  frames.set(wakes,new SurfaceFrame(owner,null,null,null,null,null,null,null,environment));
  return wakes;
}
export function stepSurface(system,dt,camera=null,actors=empty) {
  let phase=frames.get(system);const field=fields[system.constructor.name];
  if(!phase){
    const host=system.surfaceHost||system.host,owner=host.owner,parts={};
    if(system.constructor.name==='OceanSurface') {
      // An ocean's resource-only fixture publishes its two selection facts;
      // the clock, snap, strikes and all geometry still run in surface_frame.
      parts.water={host,prepareFrame(dt,camera){host.prepare(dt,camera);host.frame[FRAME.OCEAN]=1;host.frame[FRAME.STORM]=+(system.enhanced&&system.waves.storm?.enabled);},publishFrame(camera){system.publishFrame(host.result,camera);}};
    }else parts[field]=system;
    phase=new SurfaceFrame(owner,parts.foliage,parts.water,parts.shore,parts.weather,parts.snow,parts.atmosphere,parts.sky,parts.environment);frames.set(system,phase);
  }
  const eye=camera||cameraDefault;eye.updateMatrixWorld();
  // The sky contract also supports a cold, eye-less publication. Exercise
  // that input through the installed native owner without a second update.
  const sky=phase.sky;
  if(sky&&!camera){phase.sky=null;sky.prepareFrame(dt,null);}
  phase.actors=actors;phase.update(dt,eye,actors[0]||null);
  phase.publishWater(eye);phase.publishEnvironment();
  if(sky&&!camera){sky.publishFrame();phase.sky=sky;}
}

// Browser view fixtures advance the installed battle components together.
// They may author the eye, but never instantiate another native environment.
export function stepBattleSurfaces(b,dt,camera=b.camera,actors=empty) {
  let phase=battles.get(b);
  if(!phase){
    const owner=(b.water?.host||b.weather?.host||b.snow?.host||b.atmosphere?.host||b.skyFlyers?.host||b.environment?.host||b.foliage?.surfaceHost).owner;
    phase=new SurfaceFrame(owner,b.foliage,b.water,b.water?.shore,b.weather,b.snow,b.atmosphere,b.skyFlyers,b.environment);
    battles.set(b,phase);
  }
  camera.updateMatrixWorld();phase.actors=actors;phase.update(dt,camera,actors[0]||null);
  phase.publishWater(camera);phase.publishEnvironment();
}
