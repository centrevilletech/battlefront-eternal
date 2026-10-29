import * as THREE from "three";
import { segmentShape } from "./registry.js";

// Shared actor health, not hero immunity.
export function regenerate(combat, dt) {
  if (combat.dead || !(combat.health > 0) || !(dt > 0)) return;
  const rate = Number(combat.def?.AddHealth) || 0;
  if (rate > 0) combat.health = Math.min(combat.maxHealth, combat.health + rate * dt);
}

// WeaponMelee's hooks on the common Weapon/Arsenal interface. A press starts
// a swing; holding and presses during it do nothing. The entry update sets
// time to zero without advancing; hit is old <= hit && hit < old+dt.
// Completion is strict >.
export function prepareMelee(arsenal) {
  for (const weapon of arsenal.weapons) {
    if (weapon.def.base !== "melee") continue;
    weapon.bottomless = true;
    weapon.clip = 1;
    weapon.reserve = Infinity;
    const timeline = weapon.melee = new MeleeSwing(weapon);
    weapon.update = (dt, selected, trigger) => timeline.update(dt, selected, trigger);
  }
  return arsenal;
}

const meleeSwings=weapon=>weapon.def.swings?.length?weapon.def.swings.slice(0,3):[{duration:1,hit:.5,sound:null}];
export class MeleeSwing {
  constructor(weapon) {
    this.weapon = weapon;
    this.time = -1;
    this.swing = null;
    this.index = 0;
  }

  captureState(){return {time:this.time,index:this.index,active:Boolean(this.swing)};}
  restoreState(state){this.time=state.time;this.index=state.index;this.swing=state.active?meleeSwings(this.weapon)[state.index]:null;}

  update(dt, selected, trigger) {
    const w = this.weapon;
    const shots = [];
    if (this.swing && this.time > this.swing.duration) {
      this.swing = null;
      w.firing = false;
      shots.push({ weapon: w, meleeEnd: true });
    }
    if (!this.swing && selected && trigger.pressed) {
      const swings = meleeSwings(w);
      const random = w.random || Math.random;
      this.index = Math.max(0, Math.min(swings.length - 1, Math.trunc(random() * swings.length - .001)));
      this.swing = swings[this.index];
      this.time = -1;
      w.firing = true;
      shots.push({ weapon: w, meleeStart: true, swing: this.index + 1, sound: this.swing.sound });
    }
    if (this.swing) {
      if (this.time < 0) this.time = 0;
      else {
        const before = this.time;
        this.time += Math.max(dt, 0);
        if (before <= this.swing.hit && this.swing.hit < this.time) shots.push({ weapon: w, meleeHit: true });
      }
    }
    return shots;
  }
}

const packet = Object.freeze({ base: "melee", Damage: 1000, PersonScale: 1, AnimalScale: 1, DroidScale: 1, VehicleScale: 1, BuildingScale: 1, ShieldScale: 1 });
const direction = new THREE.Vector3();
const center = new THREE.Vector3();
const point = new THREE.Vector3();
const ray = new THREE.Ray();
const localRay = new THREE.Ray();

// Per-object rays, not one nearest-object ray: a swing can strike several
// objects. The native has no team filter or intervening-world LOS test.
// Soldiers use the port's shared hit shapes; hulls/buildings their BVHs.
export function meleeStrike(combat) {
  const candidates = [];
  const seen = new Set();
  const origin = combat.origin;
  center.copy(origin).add(combat.aimDir);
  const consider = (target, body = null) => {
    // A rider shown in the open is struck too (the native's query takes the
    // riders' collision list); anyone else seated is inside his mount.
    if (!target || target === combat.owner || seen.has(target) || candidates.length === 10 || target.alive === false || target.seat && !target.seat.exposed) return;
    seen.add(target);
    let sphere = target.sphere;
    if (typeof sphere === "function") {
      const p = new THREE.Vector3();
      sphere = { radius: target.sphere(p), center: p };
    }
    if (!sphere) return;
    const p = sphere.center, r = sphere.radius;
    if (Math.abs(p.x - center.x) > 1.2 + r || Math.abs(p.y - center.y) > 1.2 + r || Math.abs(p.z - center.z) > 1.2 + r) return;
    candidates.push({ target, body });
  };
  for (const target of combat.ordnance?.units?.all?.() || []) {
    if (target.kind === 10) continue;
    consider(target, target.vehicle?.body ?? target.turret?.bodies?.built ?? null);
  }
  for (const building of combat.live?.buildings || []) if (!building.aiTarget) consider(building, building.bodies?.built);
  let count = 0;
  // The recovered loop visits the query output back to front.
  for (let i = candidates.length - 1; i >= 0; i--) {
    const { target, body } = candidates[i];
    direction.subVectors(target.position, origin);
    if (direction.lengthSq() < 1e-12) direction.copy(combat.aimDir);
    direction.normalize();
    let distance = Infinity;
    if (target.hitShapes) {
      for (const shape of target.hitShapes()) {
        const hit = segmentShape(origin, direction, 2, shape);
        if (hit !== null) distance = Math.min(distance, hit);
      }
    } else if (body?.enabled && body.bvh) {
      ray.set(origin, direction);
      const hit = body.bvh.raycastFirst(body.inverse ? localRay.copy(ray).applyMatrix4(body.inverse) : ray, THREE.DoubleSide, 0, 2);
      if (hit) distance = hit.distance;
    }
    if (!(distance < 2)) continue;
    point.copy(origin).addScaledVector(direction, distance);
    combat.effects?.play?.("lightsabre_hit", point);
    if (target.combat?.tryMeleeBlock?.(combat.aimDir, combat.soldier.position)) continue;
    if (target.soldier) combat.ordnance.onHitUnit(target, packet, point, combat.owner, "body", combat.aimDir);
    else if (body) combat.ordnance.onHit?.(body, packet, combat.owner);
    count++;
  }
  return count;
}

// The blade lives under the existing held hilt, so visibility, pooling and
// hand animation use the same lifecycle. Constant native axis and widths;
// additive core blend is provisional (the native core blend is untraced).
export function attachBlade(group, base, def, texture) {
  if (!base || !def.Texture) return null;
  const axis = new THREE.Vector3(0, .8, .5);
  const length = axis.length();
  axis.normalize();
  const core = texture(def.Texture);
  if (!core) return null;
  const blade = new THREE.Group();
  blade.position.fromArray(base);
  group.add(blade);
  const cameraLocal = new THREE.Vector3();
  const right = new THREE.Vector3();
  const normal = new THREE.Vector3();
  const basis = new THREE.Matrix4();
  for (const [map, width, opacity] of [[core, .08, 1], [texture("lightsabreglow"), .16, 100 / 255]]) {
    if (!map) continue;
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(width, length), new THREE.MeshBasicMaterial({ map, color: 0xffffff, transparent: true, opacity, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, depthWrite: false, toneMapped: false }));
    mesh.position.copy(axis).multiplyScalar(length / 2);
    mesh.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), axis);
    mesh.onBeforeRender = (_renderer, _scene, camera) => {
      camera.getWorldPosition(cameraLocal);
      blade.worldToLocal(cameraLocal);
      right.crossVectors(axis, cameraLocal).normalize();
      if (right.lengthSq() < 1e-9) return;
      normal.crossVectors(right, axis).normalize();
      mesh.quaternion.setFromRotationMatrix(basis.makeBasis(right, axis, normal));
      mesh.updateMatrixWorld(true);
    };
    blade.add(mesh);
  }
  return blade;
}
