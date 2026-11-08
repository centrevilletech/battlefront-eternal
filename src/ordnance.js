import { OrdnanceWorld, resourceHost, bindResources } from './ordnance-world.js';
import * as THREE from 'three';
import { ParticleRenderer } from './particles.js';
import { BlasterBatch, boltLook } from './blaster-bolts.js';
import { SniperTrace, sniperLook } from './sniper-trace.js';
import { PageOrdnance } from './presentation/ordnance.js';
import { OrdnanceHostOwner_VALUE as O, OrdnanceHostOrd_NAMES, OrdnanceHostHelper_VALUE as H } from './sim/gen/ordnance-host.js';
export { boltLook, sniperLook };
export { LightPool } from './presentation/lights.js';

// Browser resource admission. Native ordnance chooses all queries, effects
// and drawing operations; presentation consumes its published shot records.
export class Ordnance {
  constructor({ scene, collision, effects, lights, texture, model, onExplode, onHit, presentation = null }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.lights = lights;
    this.texture = texture;
    this.model = model;
    this.onExplode = onExplode || (() => {});
    this.onExplosion = () => {};
    this.onHit = onHit || (() => {});
    this.units = null;
    this.canHit = null;
    this.onHitUnit = () => {};
    this.onProjectile = () => {}; // dynamic ordnance lifecycle observation
    this.onLaunch = () => {}; // observation only: authoritative projectile created
    this.onImpact = () => {}; // observation only: terminal bolt outcome
    this.grenadeTurn = new THREE.Quaternion();
    this.spinRandom = null; // the grenades' spin draws: () => [0, 1), or null for the legacy spin stream
    this.onGrenadeContact = () => {}; // (grenade) at its first touch of anything
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.resources = resourceHost(collision);
    this.world = new OrdnanceWorld(this);
    this.look = {bolt:boltLook,sniper:sniperLook};
    this.sim = null;
    this.behind = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.water = null; // the map's water, when it has some
    this.wet = new THREE.Vector3();
    this.renderer = new ParticleRenderer(scene);
    this.bolts = [];
    this.beams = [];
    this.grenades = [];
    this.rounds = [];
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false };
    this.start = new THREE.Vector3();
    this.draw = presentation && new PageOrdnance(presentation, {
      renderer: this.renderer, lights, texture,
      sniperTrace: () => this.sniperTrace(),
      enhanced: () => boltLook.enhanced,
      blaster: () => this.renderer.custom("blaster", () => new BlasterBatch()),
    });
  }
  prepare() {this.renderer.custom('blaster',()=>new BlasterBatch());this.sniperTrace();}
  sniperTrace() {return this.renderer.custom('sniper',()=>new SniperTrace(this.lights,()=>this.viewCamera,()=>this.viewMuzzle?.()??null));}
  fireRound(def,origin,direction,owner,visual,ignoreBody,emitter,channel,targetId,targetGeneration) {return this.sim.fire(def,origin,direction,owner,visual,ignoreBody,emitter,channel,targetId,targetGeneration);}
  flightMissile(round,flightClass,life,speed) {return this.sim.flightMissile(round,flightClass,life,speed);}
  update(dt) {return this.sim.update(dt);}
  explosion(def,position,owner,presentation=def) {return this.sim.nativeServices.explode(def,position,owner,presentation);}
}
bindResources(Ordnance,O.ordnance,OrdnanceHostOrd_NAMES);
export const HEALTH_SCALES={person:'PersonScale',animal:'AnimalScale',droid:'DroidScale',vehicle:'VehicleScale',building:'BuildingScale'};
export function blastBlocked(collision,position,target,ignoreBody=null) {const resources=resourceHost(collision);return resources.run(O.helper,H.blastBlocked,resources,collision,position,target,ignoreBody);}
