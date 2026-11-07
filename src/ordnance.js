import { OrdnanceWorld } from "./ordnance-world.js";
import { mountedMuzzle } from "./muzzles.js";
import * as THREE from "three";
import { ParticleRenderer } from "./particles.js";
import { BlasterBatch, boltLook } from "./blaster-bolts.js";
import { SniperTrace, sniperLook } from "./sniper-trace.js";
import { PageOrdnance } from "./presentation/ordnance.js";
import { OrdnanceFlag as FL } from "./presentation/ordnance-layout.js";

// The bolt look shared with graphics.js, for callers holding this module.
export { boltLook, sniperLook };
import { emitSound } from "./audio/events.js";

// What weapons fire, following the 2004 game.
//
// Bolts fly straight at Velocity for LifeSpan seconds, whatever their Gravity
// says. When one is fired, a single ray along its whole flight finds the
// buildings and terrain it will hit, and its life is cut to reach just that
// point, where it plays the impact effect for what it hit. It is drawn as two
// strips trailing back from its head, grown by the distance flown so they
// never show behind the muzzle: the core (LaserLength by LaserWidth, half as
// wide at the tail) and a glow (GlowLength by twice the width, LaserGlowColor).
// A light rides with it. A class with a FadeOutTime (only the sniper rifles
// name one, 1 s) is drawn on where its flight ended until that has run out:
// as long as it flew there or its longest length, whichever is shorter,
// fading out and thinning to a tenth of its width, its light still lit at
// the middle of its last step.
//
// Grenades ("sticky" ordnance) fall at Gravity x 9.8 and bounce off what they
// meet: the normal part of the velocity turns around at 0.37 of its speed and
// the rest loses up to 1.5755 x that speed to friction. Four slow bounces
// (under 1 m/s) and the grenade rests; its fuse, LifeSpan, only starts then.
// It then sets off ExplosionExpire.
//
// Rockets ("bullet" ordnance, the EMP launcher's) fly their model dead
// straight at Velocity (default 150) for LifeSpan (default 2), whatever their
// Gravity says, nose first, a TrailEffect at the tail and a light along. Like
// a bolt, one ray at the start finds the building or terrain they will hit.
// When life runs out they set off ExplosionExpire where they are; at a hit
// that is the point of impact, and the next frame the impact plays the
// ImpactEffect for what was hit and sets off ExplosionImpact 0.2 m off the
// surface, so a rocket naming the same explosion for both goes off twice
// there. "shell" ordnance falls at Gravity x 9.8 (default 1 g) and "missile"
// ordnance (default 35 m/s for 3 s) waver and turn as they fly; both test what
// they cross every frame instead, and go off on contact.
//
// Impact effects play at the point of impact on the world's axes, not turned
// to the surface.
//
// Explosions play their effect and light, and hand their damage, push, and
// shake to the caller, which knows who is near. A round that hits something
// with a body of its own (a turret, a droid) is handed over too, for its
// damage.

const UP = new THREE.Vector3(0, 1, 0);

const color01 = (rgba = [255, 255, 255, 255]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};
export { LightPool } from "./presentation/lights.js";

export class Ordnance {
  // texture(name) gives a texture; model(name) a function making a mesh.
  constructor({ scene, collision, effects, lights, texture, model, onExplode, onHit, presentation = null }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.lights = lights;
    this.texture = texture;
    this.model = model;
    this.onExplode = onExplode || (() => {});
    // Lifecycle observation follows the complete gameplay callback. Native
    // blast fan-out publishes this same point without invoking it twice.
    this.onExplosion = () => {};
    this.onHit = onHit || (() => {});
    // Soldiers: the battle's units (src/registry.js). A round meets the
    // first unit on its path other than its owner; `canHit(owner, unit)`
    // can rule more out. onHitUnit(unit, def, point, owner, part, direction)
    // applies it; part is "head" or "body", direction the round's flight.
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
    // Everything the rounds ask of the rest of the battle
    // (src/ordnance-world.js).
    this.world = new OrdnanceWorld(this);
    // The rounds on the simulation's module (src/ordnance-sim.js), once a
    // world hands them over: fire, update and flightMissile run there and
    // the lists hold mirrors of what it flies.
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
    this.ray = new THREE.Vector3();
    this.start = new THREE.Vector3();
    this.mid = new THREE.Vector3();
    this.basis = new THREE.Matrix4();
    this.right = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.nose = new THREE.Quaternion();
    this.axis = new THREE.Vector3();
    // The page's drawing of these rounds, in the presentation module
    // (src/presentation/ordnance.js). A world that never draws has none.
    this.draw = presentation && new PageOrdnance(presentation, {
      renderer: this.renderer, lights, texture,
      sniperTrace: () => this.sniperTrace(),
      enhanced: () => boltLook.enhanced,
      blaster: () => this.renderer.custom("blaster", () => new BlasterBatch()),
    });
  }

  // Create the drawing resources a battle will need, so graphics.warmup()
  // compiles their shaders before play instead of on the first shot.
  prepare() {
    this.renderer.custom("blaster", () => new BlasterBatch());
    // The Enhanced sniper look's two programs (its tracer's capsules and its
    // flares): made at the first shot, they stalled that frame on compiling.
    this.sniperTrace();
  }

  // `visual` (optional) is where the bolt is drawn from: the muzzle.
  // A mounted weapon can exclude its own live collision body from its shot.
  // `guidance` (optional): the shot (src/weapons.js), whose weapon's
  // channel a round answers and whose target a missile seeks.
  fire(def, origin, direction, owner, visual = null, ignoreBody = null, emitter = null, guidance = null) {
    return this.fireRound(def, origin, direction, owner, visual, ignoreBody, emitter, guidance?.weapon?.channel ?? 0, guidance?.targetId, guidance?.targetGeneration);
  }

  // fire() with the shot's numbers taken out of it.
  fireRound(def, origin, direction, owner, visual, ignoreBody, emitter, channel, targetId, targetGeneration) {
    return this.sim.fire(def, origin, direction, owner, visual, ignoreBody, emitter, channel, targetId, targetGeneration);
  }

  // A flyer's missile (vehicles.js fireSeat): steered by its seeker
  // (flight/targeting.js navigateMissile) for the class's missile lifetime
  // at its missile speed. The round's own writes, kept here with the rest.
  flightMissile(round, flightClass, life, speed) {
    return this.sim.flightMissile(round, flightClass, life, speed);
  }

  // The fat ray's sweep over one step: every unit whose sphere comes
  // A beam persists for LifeSpan and follows its emitting vehicle. Damage
  // Nose (+z) along the flight, its x across the world's up.
  orient(round) {
    const forward = this.axis.copy(round.velocity).normalize();
    if (forward.lengthSq() < 1e-8) return;
    this.right.crossVectors(UP, forward);
    if (this.right.lengthSq() < 1e-8) this.right.set(1, 0, 0);
    this.right.normalize();
    this.up.crossVectors(forward, this.right);
    round.quaternion.setFromRotationMatrix(this.basis.makeBasis(this.right, this.up, forward));
  }

  update(dt) {
    return this.sim.update(dt);
  }

  clearProjectilePresentation() {
    // Used when a networking adapter replaces a bootstrap's frozen lists.
    // Shared model geometry belongs to the content cache; only detach instances.
    for (const list of [this.grenades, this.rounds]) for (const projectile of list) {
      if (projectile.mesh) this.scene.remove(projectile.mesh);
      projectile.trail?.stop();
    }
    this.bolts.length = this.beams.length = this.grenades.length = this.rounds.length = 0;
  }

  projectileMesh(def) {
    const make = def.GeometryName ? this.model?.(def.GeometryName) : null;
    const mesh = make ? make() : null;
    if (mesh) this.scene.add(mesh);
    return mesh;
  }

  // Every round on screen, drawn by one call of the presentation module
  // (src/presentation/ordnance.js, rust/bfe-presentation/src/ordnance.rs):
  // beams, then bolts, then rockets, then grenades' meshes, then what `extra`
  // draws (a page online's replicated rounds and the player's predicted
  // ones, which add their own records), in that order. `extra.settle()` then
  // runs what those draws set off: impacts and explosions.
  present(alpha = 1, clockTick = null, extra = null) {
    const draw = this.draw;
    this.renderer.begin();
    draw.begin(boltLook.enhanced, sniperLook.enhanced);
    for (const beam of this.beams) draw.beam(beam);
    for (const bolt of this.bolts) {
      // Resolve a local cosmetic muzzle only after the rendered actor/camera
      // pose exists. The physical origin, direction and hit stay tick-owned.
      let muzzle = null;
      if (bolt.visualMuzzle || (bolt.mount && !bolt.mountResolved)) {
        muzzle = bolt.visualMuzzle?.() || mountedMuzzle(bolt);
        bolt.mountResolved = true;
        bolt.visualMuzzle = null;
      }
      draw.bolt(bolt, alpha, clockTick, muzzle, FL.RESOLVE_BOLT);
    }
    for (const round of this.rounds) {
      let muzzle = null;
      if (round.visualMuzzle || (round.mount && !round.mountResolved)) {
        muzzle = round.visualMuzzle?.() || mountedMuzzle(round);
        round.visualMuzzle = null; round.mountResolved = true;
      }
      draw.round(round, alpha, muzzle);
    }
    for (const grenade of this.grenades) draw.grenade(grenade, alpha);
    extra?.draw();
    draw.finish();
    extra?.settle();
    this.renderer.end();
  }

  sniperTrace() {
    return this.renderer.custom("sniper", () => new SniperTrace(this.lights, () => this.viewCamera, () => this.viewMuzzle?.() ?? null));
  }

  // The world's "headshot" effect at `point`, as this page shows it: the
  // Enhanced sniper look's head shot flourish.
  presentHeadshot(point) {
    if (sniperLook.enhanced) this.sniperTrace().headshot(point);
  }

  // A sniper round's impact (`play`: its effect and sound) for the
  // Enhanced look to play when its drawn bolt arrives; false to play it now.
  deferSniperImpact(def, point, play) {
    return Boolean(sniperLook.enhanced && def?.FadeOutTime > 0 && this.sniperTrace().defer(point, play));
  }

  // Sticky ordnance holds onto the kinds of thing its Stick* properties name.

  // The first unit a segment meets, other than the round's owner (and any
  // the battle's rules exclude), within `far`: soldiers on foot, bodies
  // lying where they fell, and riders shown on exposed seats (src/
  // rider-pose.js). A round from a hull's or turret's own guns (ignoreBody:
  // the body it passes through) leaves the riders on that hull alone.
  // `mount`: the hull a round has gone through to its riders (riderBehind),
  // whose riders alone it may strike.
  castUnits(origin, direction, far, owner, historyTick = null, ignoreBody = null, mount = null) {
    if (far <= 0) return null;
    return this.world.raycastUnits(origin, direction, far, owner, historyTick, ignoreBody, mount);
  }

  // A round that met a hull whose collision doesn't keep rounds off its
  // riders (Vehicle.shieldsRiders: a riding animal's) goes on through it to
  // a rider of it, if it crosses his body before anything else, met this
  // step even if he is a little past where the step ends. far: as far as
  // the round can still fly (a bolt's or rocket's world was traced at its
  // launch: then only moving bodies are tested here). The rider's hit, or
  // null.
  riderBehind(origin, direction, hit, far, owner, historyTick, ignoreBody, moving) {
    const mount = this.world.riderMount(hit?.body);
    if (!mount) return null;
    const reach = Math.min(far, hit.distance + 2 * (mount.boundRadius || 1));
    const next = moving ? this.world.castMoving(origin, direction, reach, this.behind, mount.body) : this.world.cast(origin, direction, reach, this.behind, mount.body);
    return this.castUnits(origin, direction, next ? next.distance : reach, owner, historyTick, ignoreBody, mount.vehicle);
  }

  // A soldier struck: droids spark like metal, the rest take the soft hit.
  unitImpactEffect(def, unit) {
    const droid = (unit.classDef?.HealthType || "").toLowerCase() === "droid";
    return (droid ? def.ImpactEffectRigid : def.ImpactEffectSoft) || def.ImpactEffectStatic || def.ImpactEffectTerrain;
  }

  // An explosion: its effect (its WaterEffect at the water's surface, with a
  // ring), its light, and its damage for the caller.
  explosion(explosion, position, owner, presentation = explosion) {
    return this.sim.nativeServices.explode(explosion,position,owner,presentation);
  }

  // Shared cosmetic path; callers never apply damage through presentation.
  presentExplosion(explosion, position, authoritativeWet = null) {
    const water = this.water;
    const wet = authoritativeWet ?? Boolean(water && water.over(position.x, position.z) && position.y <= water.level + 0.3);
    if (wet && water) {
      const r = Math.min((explosion.DamageRadiusOuter || 0) / 2, 0.4);
      water.ripple(position, 3 + r, r);
    }
    const effect = wet && explosion.WaterEffect ? explosion.WaterEffect : explosion.Effect;
    if (effect) this.effects.play(effect, position, UP);
    emitSound("explosion", explosion, position, wet);
    this.lights.flash(position, color01(explosion.LightColor), explosion.LightRadius || 0, explosion.LightDuration || 0);
    return wet;
  }
}

// Cold class preparation resolves authored scale keys before native import.
export const HEALTH_SCALES = { person: "PersonScale", animal: "AnimalScale", droid: "DroidScale", vehicle: "VehicleScale", building: "BuildingScale" };

// Foreign collision queries still ask the host for a static-only cast.
// Native blast owners apply distance, health scales, damage and push.
const SIGHT = new THREE.Vector3();
const SIGHT_HIT = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
export function blastBlocked(collision, position, target, ignoreBody = null) {
  const span = SIGHT.subVectors(target, position).length();
  if (!collision || span < 1e-3) return false;
  // Moving bodies (vehicles) never shield; static ones (buildings) do.
  const hit = collision.cast(position, SIGHT.divideScalar(span), span, SIGHT_HIT, ignoreBody, true);
  return Boolean(hit && hit.distance < span - 0.05);
}
