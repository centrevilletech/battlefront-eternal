// Run against installed/imported content: node tools/audit-emissions.mjs [report.json]
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { vehiclePacks } from './vehicle-packs.mjs';
import { modelPack } from '../src/models.js';
import { weaponMuzzle, descendantMuzzles } from '../src/muzzles.js';
import { buildActor } from '../src/actor.js';
import { JetpackNozzles } from '../src/jetpack-nozzles.js';
import { Vehicles } from '../src/vehicles.js';
import { CollisionWorld } from '../src/collision.js';
const json = p => JSON.parse(fs.readFileSync(p));
const binary = p => { const b = fs.readFileSync(p); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const report = { scope: 'All imported unit definitions, nested recon droids, vehicle classes and placed armed turrets. Checks model-space muzzles, runtime hardpoint resolution and skinned jet outlets; this is not a full visual playthrough.', units: [], vehicles: [], turrets: [], failures: [] };
const check = (ok, message) => { if (!ok) report.failures.push(message); };
for (const side of fs.readdirSync('public/sides')) {
  const dir = `public/sides/${side}`;
  if (!fs.existsSync(`${dir}/units.json`)) continue;
  const pack = json(`${dir}/weapons.json`), models = modelPack(pack.models, binary(`${dir}/weapons.bin`), binary(`${dir}/weapons_index.bin`));
  for (const [name, def] of Object.entries(json(`${dir}/units.json`).units)) {
    const row = { side, name, weapons: [] }; report.units.push(row);
    for (const w of def.weapons) {
      if (w.droid?.weapons?.some(weapon => weapon.ordnance)) {
        const model = w.droid.GeometryName, point = pack.firePoints[model];
        (row.remotes ||= []).push({ model, point });
        check(models.has(model) && point?.every(Number.isFinite), `${name}/${model}: missing remote muzzle`);
      }
      if (!w.ordnance || w.base === 'grenade') continue;
      const entry = { weapon: w.class, models: [] }; row.weapons.push(entry);
      if (!w.GeometryName) { entry.builtIn = w.aimers; continue; }
      for (const model of new Set([w.GeometryName, w.HighResGeometry].filter(Boolean))) {
        const point = weaponMuzzle(models.get(model), pack.firePoints[model], w);
        check(models.has(model) && point?.every(Number.isFinite), `${name}/${model}: no finite muzzle`);
        entry.models.push({ model, point, imported: pack.firePoints[model] });
        check(point && pack.firePoints[model]?.every((n, i) => Math.abs(n - point[i]) < 1e-5), `${name}/${model}: importer/runtime mismatch`);
      }
    }
    if (def.JetEffect) {
      const p = `${dir}/actors/${def.model}`, actorDef = json(`${p}.json`);
      const actor = buildActor(actorDef, binary(`${p}.bin`), binary(`${p}_index.bin`), () => null, { caster() {} });
      const nozzles = new JetpackNozzles(actor, actorDef);
      row.jet = { effect: def.JetEffect, outlets: nozzles.frames.length };
      if (/jetpack/i.test(def.JetEffect)) check(nozzles.frames.length === 2, `${name}: missing pack outlets`);
      // Independently compare each anchor to actual skinned vertices over poses.
      let error = 0;
      for (let pose = 0; pose < 8; pose++) {
        actor.root.position.set(pose, pose * .2, -pose); actor.root.rotation.y = pose * .31;
        actor.byName.bone_ribcage.rotation.x += .04; actor.root.updateMatrixWorld(true);
        for (const frame of nozzles.update() || []) {
          const center = new THREE.Vector3(), vertex = new THREE.Vector3();
          for (const index of frame.rimIndices) center.add(actor.body.getVertexPosition(index, vertex).applyMatrix4(actor.body.matrixWorld));
          center.divideScalar(frame.rimIndices.length); error = Math.max(error, center.distanceTo(frame.rimCenter));
        }
      }
      row.jet.maxSkinError = error; check(error < 1e-6, `${name}: detached exhaust`);
    }
  }
}
const seen = new Set(), turretSeen = new Set();
// Every battle's pack (tools/vehicle-packs.mjs); a class the packs share is checked once.
const mapsDir = path.resolve('public/maps');
for (const pack of vehiclePacks(pathToFileURL(mapsDir + '/'))) {
  const file = fileURLToPath(pack), dir = path.dirname(file), map = path.relative(mapsDir, dir).split(path.sep)[0];
  if (fs.existsSync(file)) {
    const data = json(file), names = Object.keys(data.classes).filter(n => !seen.has(n));
    if (names.length) {
      names.forEach(n => seen.add(n));
      const g = new THREE.BoxGeometry(20000, 2, 20000).translate(0, -1, 0);
      const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
      const spawns = names.map((name, i) => ({ name, matrix: new THREE.Matrix4().makeTranslation(i * 100, 0, 0).toArray(), team: 1, post: '', respawnTime: 45, classes: { 1: name } }));
      const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision, data: { ...data, spawns, blockers: null }, buffer: binary(`${dir}/models.bin`), indexBuffer: binary(`${dir}/models_index.bin`), animationBuffer: binary(`${dir}/animations.bin`), texture: () => null, ordnance: { explosion() {}, lights: { flash() {} } }, effects: { play() {} } });
      for (const vehicle of system.vehicles) {
        const row = { name: vehicle.name, map, weapons: [] }; report.vehicles.push(row);
        for (const [seat, s] of vehicle.seats.entries()) for (const w of s.weapons) {
          const entry = { seat, weapon: w.def.class, barrels: w.muzzles.length, anchors: [] }; row.weapons.push(entry);
          for (let i = 0; i < Math.max(1, w.muzzles.length); i++) {
            const p = new THREE.Vector3(); vehicle.muzzleOfSeat(seat, w, p);
            const node = vehicle.shotNode;
            const name = Object.keys(vehicle.byName).find(n => vehicle.byName[n] === node);
            entry.anchors.push(name || (node ? 'mesh mouth' : 'hull fallback'));
            check(node && p.toArray().every(Number.isFinite), `${vehicle.name}/${seat}/${w.def.class}: hull fallback`);
            if (node) {
              vehicle.root.rotation.y += .13; vehicle.root.updateMatrixWorld(true);
              const expected = node.getWorldPosition(new THREE.Vector3());
              check(expected.toArray().every(Number.isFinite), `${vehicle.name}: invalid animated anchor`);
            }
          }
        }
      }
      system.dispose();
    }
  }
  const propFile = `public/maps/${map}/props.json`;
  if (!fs.existsSync(propFile)) continue;
  const props = json(propFile);
  for (const p of props.instances.filter(p => p.weapon)) {
    const key = `${p.class}/${p.model}`; if (turretSeen.has(key)) continue; turretSeen.add(key);
    const names = new Set(props.models[p.model].bones.map(b => b.name.toLowerCase()));
    let points = [p.settings.FirePointName || 'hp_fire'].flat();
    if (!points.some(n => names.has(n.toLowerCase()))) {
      const bones = props.models[p.model].bones.map(b => { const node = new THREE.Object3D(); node.name = b.name; return node; });
      props.models[p.model].bones.forEach((b, i) => { if (b.parent >= 0) bones[b.parent].add(bones[i]); });
      const byName = Object.fromEntries(bones.map(b => [b.name.toLowerCase(), b]));
      points = descendantMuzzles(byName[(p.settings.AimerNodeName || p.settings.TurretNodeName || '').toLowerCase()], byName).map(n => n.name);
    }
    report.turrets.push({ name: p.class, model: p.model, points });
    check(points.length > 0 && points.every(n => names.has(n.toLowerCase())), `${p.class}: missing turret fire point`);
  }
}
if (process.argv[2]) fs.writeFileSync(path.resolve(process.argv[2]), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ units: report.units.length, vehicles: report.vehicles.length, turrets: report.turrets.length, failures: report.failures }, null, 2));
process.exitCode = report.failures.length ? 1 : 0;
