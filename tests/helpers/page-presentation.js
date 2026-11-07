// The page's native presentation phases for tests in Node: the
// bfe-presentation module loaded once from its committed bytes, and a hull
// phase (src/presentation/vehicles.js) attached to a test's Vehicles, each
// queued draw run at once since a test reads it right after the call, and
// the prop phase (src/presentation/props.js) over a test's props and traps.
import fs from "node:fs";
import * as THREE from "three";
import * as module from "../../src/presentation-pkg/bfe_presentation.js";
import { PageVehicles } from "../../src/presentation/vehicles.js";
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

// The page's prop phase over `animated` props and `traps`, their bones as
// they stand now; feet() and bodies() as LiveObjects' providers give them.
export function pageProps({ animated = [], traps = [], turrets = [] }, { feet = () => {}, bodies = () => [] } = {}) {
  return new PageProps(presentationModule(), { animated, traps, turrets }, { feet, bodies });
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
export function presentTurret(turret) {
  let phase = turretPhases.get(turret);
  if (!phase) { phase = pageProps({turrets:[turret]}); turretPhases.set(turret,phase); }
  phase.present(0,0);
}
