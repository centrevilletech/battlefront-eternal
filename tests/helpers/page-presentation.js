// The page's native presentation phases for tests in Node: the
// bfe-presentation module loaded once from its committed bytes, and a hull
// phase (src/presentation/vehicles.js) attached to a test's Vehicles, each
// queued draw run at once since a test reads it right after the call, and
// the prop phase (src/presentation/props.js) over a test's props and traps.
import fs from "node:fs";
import * as THREE from "three";
import * as module from "../../src/presentation-pkg/bfe_presentation.js";
import { PageVehicles } from "../../src/presentation/vehicles.js";
import { VehicleMode } from "../../src/presentation/layout.js";
import { PageProps } from "../../src/presentation/props.js";

let loaded = null;
export function presentationModule() {
  if (!loaded) {
    const wasm = module.initSync({ module: fs.readFileSync(new URL("../../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)) });
    loaded = { module, memory: wasm.memory };
  }
  return loaded;
}

// A Vehicles system's page hull phase: Vehicle.present draws at once.
export function attachPage(system) {
  const page = new PageVehicles(presentationModule());
  for (const name of ["alive", "dying"]) {
    const queue = page[name].bind(page);
    page[name] = (...args) => {
      const drawn = queue(...args);
      page.flush();
      return drawn;
    };
  }
  system.page = page;
  return page;
}

// A small presentation-only hull still registers a real native source rig.
// Its tests move root facts; native vehicle posing owns the saddle matrices.
export function fixtureVehicle(v){
  if(v.system)return v;
  v.system={world:{clock:()=>0,castRays(){}}};
  v.def={fx:{}};v.bones=[];v.bindPositions=[];v.bindQuaternions=[];v.byName={};v.clips=new Map();v.wheels=[];
  v.position=v.root.position.clone();v.previous=v.position.clone();v.visual=v.position.clone();v.quaternion=v.root.quaternion.clone();
  v.size=new THREE.Vector3(1,2,1);v.walkPhase=v.walkBlend=v.previousYaw=v.previousTilt=v.previousRoll=v.tilt=v.roll=0;
  v.held=()=>true;v.crewed=()=>true;
  for(const seat of v.seats){seat.weapons=[];seat.view.up=new THREE.Vector3(0,1,0);seat.view.yawFactor=1;seat.view.pitchFactor=0;}
  attachPage(v.system);return v;
}
// A world fixture's hull drawn by the page's hull phase in its render pose
// (between its last two ticks at alpha, no time, the seats render() poses),
// so a test reads its bones and seat frames. The world's own pose is native.
export function drawHull(v, alpha = 1) {
  const page = v.system.page || attachPage(v.system);
  page.write(v, VehicleMode.RENDER, alpha, 0, true);
  page.flush();
}

// The page's prop phase over `animated` props and `traps`, their bones as
// they stand now; feet() and bodies() as LiveObjects' providers give them.
const propPhases=new WeakMap();
export function propPhase(prop){return propPhases.get(prop);}
export function pageProps({ animated = [], traps = [], turrets = [] }, { feet = () => {}, bodies = () => [] } = {}) {
  const phase=new PageProps(presentationModule(), { animated, traps, turrets }, { feet, bodies });
  for(const prop of [...animated,...turrets])propPhases.set(prop,phase);
  return phase;
}

// A node's own transform as drawn: its fields, or once a phase publishes
// its tree (matrixAutoUpdate off), its published local matrix taken apart.
const scratch = new THREE.Vector3();
export function local(node) {
  if (node.matrixAutoUpdate) return { position: node.position, quaternion: node.quaternion };
  const position = new THREE.Vector3(), quaternion = new THREE.Quaternion();
  node.matrix.decompose(position, quaternion, scratch);
  return { position, quaternion };
}

// Synthetic turret fixtures run the same coarse prop phase as the page.
const turretPhases = new WeakMap();
export function turretPhase(turret) {
  let phase = turretPhases.get(turret)||propPhases.get(turret);
  if (!phase) { phase = pageProps({turrets:[turret]}); turretPhases.set(turret,phase); }
  return phase;
}
export function presentTurret(turret) {const phase=turretPhase(turret);phase.present(0,0);return phase;}
