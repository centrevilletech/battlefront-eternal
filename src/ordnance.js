import { resolveOrdnanceOwner } from "./ordnance-owner.js";
import { strikeable } from "./registry.js";
import { linearBoltFrame } from './runtime/linear-bolt.js';
import { networkShotFor } from './runtime/network-shot-aim.js';
import { navigateMissile } from './flight/targeting.js';
import { mountedMuzzle } from "./muzzles.js";
import * as THREE from "three";
import { ParticleRenderer, STREAK } from "./particles.js";
import { BlasterBatch, boltLook, sheathColor, textureHue, vivid } from "./blaster-bolts.js";
import { SniperTrace, sniperLook } from "./sniper-trace.js";

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
const FORWARD = new THREE.Vector3(0, 0, 1);
const LIMIT = 5000; // m from the origin past which rounds are dropped
const RESTITUTION = 0.37;
const FRICTION = 1.5755;
const SLOW = 1; // m/s
const SLOW_BOUNCES = 4;
const MAX_FLIGHT = 20; // s a grenade may fly before it goes off anyway
const MERGE = 8; // m over which a bolt drawn from the muzzle joins its path
const FLOOR = -500;
const FATRAY_RADIUS = 1.5; // m: how fat a fatray round is (the 2004 data sets no width; its TrailEffect is its look)
const CORE_TAPER = [0.5, 0, 0], GLOW_TAPER = [0.8, 0, 0];

const color01 = (rgba = [255, 255, 255, 255]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};
// A bolt's glow length as the class parser leaves it: reading LaserLength
// sets GlowLength to three times it, and a GlowLength named after replaces
// that (docs/formats.md, "Bolts"). So vehicle and turret guns, which name
// none, still glow, white when they name no LaserGlowColor.
const glowLength = (def) => def.GlowLength ?? (def.LaserLength ? def.LaserLength * 3 : 0);
// A round's light colour: set when it was fired here, or, for a round a
// worker's packet described (src/runtime/replica.js carries no arrays),
// taken from its definition on first use.
const lightOf = (round) => round.light || (round.light = color01(round.def?.LightColor));

// Short-lived point lights (muzzle flashes, bolts, explosions) on a fixed set
// of scene lights, so the shaders' light count never changes. Each frame the
// brightest requests get the lights.
export class LightPool {
  constructor(scene, count = 8) {
    this.lights = Array.from({ length: count }, () => {
      const light = new THREE.PointLight(0x000000, 0, 1, 1);
      scene.add(light);
      return light;
    });
    this.timed = [];
    // This frame's requests: the first `requestCount` of the pool.
    this.requestCount = 0;
    this.requestPool = [];
    this.top = [];
  }

  // A light for this frame only. Colours are 0..1.
  add(position, rgb, radius, scale = 1) {
    if (!rgb || radius <= 0) return;
    const i = this.requestCount++;
    const request = this.requestPool[i] || (this.requestPool[i] = { position: new THREE.Vector3(), rgb: [0, 0, 0], radius: 0, scale: 0, weight: 0 });
    request.position.copy(position);
    request.rgb[0] = rgb[0]; request.rgb[1] = rgb[1]; request.rgb[2] = rgb[2];
    request.radius = radius; request.scale = scale;
    request.weight = (rgb[0] + rgb[1] + rgb[2]) * scale * radius;
  }

  // A light that fades out over `duration` seconds; returns it (null for
  // none) so a caller can tell whether it has started fading.
  flash(position, rgb, radius, duration) {
    if (radius <= 0 || duration <= 0) return null;
    const light = { position: position.clone(), rgb, radius, duration, left: duration };
    this.timed.push(light);
    return light;
  }

  // Every frame, with no garbage: the pool's lights are always in the
  // scene (an unused one at intensity 0), so the count three compiles its
  // lit programs for never changes.
  update(dt) {
    const timed = this.timed;
    let kept = 0;
    for (let i = 0; i < timed.length; i++) {
      const t = timed[i];
      t.left -= dt;
      if (t.left > 0) {
        this.add(t.position, t.rgb, t.radius, t.left / t.duration);
        timed[kept++] = t;
      }
    }
    timed.length = kept;
    // The heaviest requests, heaviest first (as a stable sort by weight
    // would order them), one per light.
    const requests = this.requestPool, top = this.top, lights = this.lights;
    let count = 0;
    for (let i = 0; i < this.requestCount; i++) {
      const r = requests[i];
      let at = count;
      while (at > 0 && r.weight > top[at - 1].weight) at--;
      if (at >= lights.length) continue;
      if (count < lights.length) count++;
      for (let j = count - 1; j > at; j--) top[j] = top[j - 1];
      top[at] = r;
    }
    for (let i = 0; i < lights.length; i++) {
      const light = lights[i];
      if (i >= count) {
        light.intensity = 0;
        continue;
      }
      const r = top[i];
      light.position.copy(r.position);
      light.color.setRGB(r.rgb[0], r.rgb[1], r.rgb[2]);
      light.intensity = r.scale;
      light.distance = r.radius;
    }
    this.requestCount = 0;
  }
}

export class Ordnance {
  // texture(name) gives a texture; model(name) a function making a mesh.
  constructor({ scene, collision, effects, lights, texture, model, onExplode, onHit }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.lights = lights;
    this.texture = texture;
    this.model = model;
    this.onExplode = onExplode || (() => {});
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
    this.onGrenadeContact = () => {}; // (grenade) at its first touch of anything
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    // The round castUnits is testing, for its filter (mayStrike).
    this.striking = { owner: null, body: null, mount: null, filter: (unit) => this.mayStrike(this.striking.owner, unit, this.striking.body) };
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
    this.drawnAt = new THREE.Vector3();
    this.start = new THREE.Vector3();
    this.mid = new THREE.Vector3();
    this.basis = new THREE.Matrix4();
    this.right = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.nose = new THREE.Quaternion();
    this.axis = new THREE.Vector3();
    this.boltStyles = new WeakMap();
    this.boltAxis = [0, 0, 1];
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
  fire(def, origin, direction, owner, visual = null, ignoreBody = null, emitter = null, guidance = null) {
    const channel=guidance?.weapon?.channel??0;
    if (def.base === "beam") this.beam(def, origin, direction, owner, ignoreBody, emitter, channel);
    else if (def.base === "sticky" || def.base === "grenade") this.throw(def, origin, direction, owner, channel);
    else if (def.base === "bullet" || def.base === "missile" || def.base === "shell") return this.round(def, origin, direction, owner, ignoreBody, guidance);
    else if (def.base === "fatray") return this.fatray(def, origin, direction, owner, ignoreBody, channel);
    else return this.bolt(def, origin, direction, owner, visual, ignoreBody, channel);
  }

  // A fat ray (the Geonosian LR1K sonic cannon's round, base fatray): a
  // thick, slow ball flying straight at Velocity for LifeSpan (the
  // retail class: 100 m/s for 0.45 s, so 45 m, no gravity, no explosion)
  // that strikes everyone it passes within FATRAY_RADIUS of, once each,
  // for Damage by their scale, and stops at the first wall or hull it
  // meets (struck through its body, as a round). Its TrailEffect
  // (geo_blunderbuss_blast) is its look.
  fatray(def, origin, direction, owner, ignoreBody = null, channel = 0) {
    const dir = direction.clone().normalize();
    const velocity = dir.clone().multiplyScalar(def.Velocity ?? 100);
    const round = {
      kind: "fatray", networkFireChannel:channel, ignoreBody, def, owner, position: origin.clone(), previous: origin.clone(), launchPosition: origin.clone(), velocity,
      life: def.LifeSpan ?? 0.45, impact: null, mesh: null, dead: false, gravity: 0, ramp: 0, desired: dir.clone(),
      phase: [0, 0], light: color01(def.LightColor), quaternion: new THREE.Quaternion(), struck: new Set(),
    };
    this.orient(round);
    round.trail = def.TrailEffect ? this.effects.play(def.TrailEffect, origin, null, velocity, round.quaternion) : null;
    this.rounds.push(round);
    this.onProjectile(round, "round");
    return round;
  }

  // The fat ray's sweep over one step: every unit whose sphere comes
  // within FATRAY_RADIUS of the segment flown (as far as the wall it met)
  // is struck, once over the ray's life.
  sweepFat(r, direction, span) {
    if (!this.units?.query || span <= 0) return;
    this.mid.copy(r.previous).addScaledVector(direction, span * 0.5);
    const near = this.units.query(this.mid, span * 0.5 + FATRAY_RADIUS + 4, this.fatQuery ||= [],
      // Soldiers on foot and riders in the open (those with hit shapes), as
      // the registry's raycast strikes them; vehicles and turrets are met
      // through their collision bodies on the centre ray.
      (unit) => unit !== r.owner && !r.struck.has(unit) && strikeable(unit) && this.mayStrike(r.owner, unit, r.ignoreBody));
    for (const unit of near) {
      const s = unit.sphere;
      if (!s) continue;
      const t = THREE.MathUtils.clamp(this.axis.subVectors(s.center, r.previous).dot(direction), 0, span);
      this.start.copy(r.previous).addScaledVector(direction, t);
      if (this.start.distanceTo(s.center) > s.radius + FATRAY_RADIUS) continue;
      r.struck.add(unit);
      this.onHitUnit(unit, r.def, this.start.lerp(s.center, 0.5), r.owner, null, direction);
      const effect = this.unitImpactEffect(r.def, unit);
      if (effect) this.effects.play(effect, this.start);
    }
    near.length = 0;
  }

  // A beam persists for LifeSpan and follows its emitting vehicle. Damage
  // is integrated over that span, so display rate never multiplies it.
  beam(def, origin, direction, owner, ignoreBody, emitter = null, channel = 0) {
    const duration = Math.max(.01, def.LifeSpan || .4);
    const source = emitter ?? owner;
    const beam = { networkFireChannel:channel, historyTick:this.hitHistory?.shotTick(owner,channel) ?? null, def, owner, source, ignoreBody, life: duration, duration, effectTime: 0,
      position: origin.clone(), end: origin.clone(), direction: direction.clone(),
      localOrigin: source?.matrix ? origin.clone().applyMatrix4(source.matrix.clone().invert()) : null,
      damageDef: { ...def }, axis: [0, 0, 1], color: color01(def.LaserGlowColor) };
    this.beams.push(beam);this.onProjectile(beam, "beam");
  }

  stepBeam(beam, dt, draw = true) {
    const span = Math.min(dt, beam.life);
    if (beam.owner?.alive === false || beam.source?.alive === false) { beam.life = 0; return; }
    if (beam.localOrigin) {
      beam.position.copy(beam.localOrigin).applyMatrix4(beam.source.matrix);
      beam.source.viewDirection?.(beam.direction);
    }
    const hit = this.collision.cast(beam.position, beam.direction, 600, this.hit, beam.ignoreBody);
    const unitHit = this.castUnits(beam.position, beam.direction, hit ? hit.distance : 600, beam.owner, beam.historyTick, beam.ignoreBody)
      ?? this.riderBehind(beam.position, beam.direction, hit, 600, beam.owner, beam.historyTick, beam.ignoreBody, false);
    beam.historyTick=null;
    const length = unitHit ? unitHit.distance : hit ? hit.distance : 600;
    beam.end.copy(beam.position).addScaledVector(beam.direction, length);
    if (unitHit) {
      beam.damageDef.Damage = (beam.def.Damage || 0) * span / beam.duration;
      this.onHitUnit(unitHit.unit, beam.damageDef, beam.end, beam.owner, unitHit.part, beam.direction);
    } else if (hit?.body) {
      beam.damageDef.Damage = (beam.def.Damage || 0) * span / beam.duration;
      this.onHit(hit.body, beam.damageDef, beam.owner, beam.direction);
    }
    beam.effectTime -= dt;
    // A beam leaves its ImpactEffect wherever it touches, hulls and soldiers
    // too (OrdnanceBeamClass); an older extract keeps the surface's.
    if ((hit || unitHit) && beam.effectTime <= 0) {
      const effect = beam.def.ImpactEffect ?? (unitHit ? null : hit.terrain ? beam.def.ImpactEffectTerrain : beam.def.ImpactEffectStatic);
      if (effect) this.effects.play(effect, beam.end);
      beam.effectTime = .12;
    }
    beam.life -= dt;
    if (draw) this.drawBeam(beam);
  }

  drawBeam(beam) {
    beam.color ||= color01(beam.def.LaserGlowColor);
    const length = beam.position.distanceTo(beam.end);
    if (boltLook.enhanced) {
      const look = this.blasterLook(beam.def);
      const fade = Math.min(1, Math.max(0, beam.life) / .08);
      this.renderer.custom("blaster", () => new BlasterBatch()).push(beam.end, beam.position, look.color, look.brightness * fade, look.core, look.glow, 0.37, 1);
      this.lights.add(beam.position, look.light, look.lightRadius);
      return;
    }
    let style = this.boltStyles.get(beam.def);
    if (!style) {
      const texture = this.texture(beam.def.LaserTexture || 'lightglow');
      style = { beam: texture ? this.renderer.batch(texture, 'additive', 'bolt') : null };
      this.boltStyles.set(beam.def, style);
    }
    // A beam a worker or server replicated arrives without the draw's
    // scratch axis, as without its colour.
    const p = beam.end, d = beam.direction, axis = beam.axis ||= [0, 0, 1];
    axis[0] = d.x; axis[1] = d.y; axis[2] = d.z;
    const fade = Math.min(1, Math.max(0, beam.life) / .08);
    style.beam?.push(p.x, p.y, p.z, 1, 1, 1, fade, (beam.def.LaserWidth || .3) / 2, 0, STREAK, length, axis);
    this.lights.add(beam.position, beam.color, beam.def.LightRadius || 3);
  }

  // A rocket, missile, or shell: a model flown nose first.
  round(def, origin, direction, owner, ignoreBody = null, guidance = null) {
    const kind = def.base;
    const bullet = kind === "bullet";
    const speed = def.Velocity ?? (bullet ? 150 : 35);
    let life = def.LifeSpan ?? (bullet ? 2 : 3);
    const dir = direction.clone().normalize();
    let impact = null;
    if (bullet) {
      const hit = this.collision.cast(origin, dir, speed * life, this.hit, ignoreBody, true);
      if (hit) {
        life = hit.distance / speed;
        impact = { point: hit.point.clone(), normal: hit.normal.clone(), terrain: hit.terrain, body: hit.body };
      }
    }
    const mesh = this.projectileMesh(def);
    const velocity = dir.clone().multiplyScalar(speed);
    const round = {
      kind,
      networkFireChannel:guidance?.weapon?.channel??0,
      ignoreBody,
      def,
      owner,
      position: origin.clone(),
      previous: origin.clone(),
      launchPosition: origin.clone(),
      velocity,
      life,
      impact,
      mesh,
      dead: false,
      gravity: kind === "shell" ? -9.8 * (def.Gravity ?? 1) : 0,
      // Missiles ease into their turning and waver over ScaleTime.
      ramp: 0,
      desired: dir.clone(),
      targetId: guidance?.targetId ?? -1,
      targetGeneration: guidance?.targetGeneration ?? -1,
      phase: [(this.random || Math.random)() * 2 * Math.PI, (this.random || Math.random)() * 2 * Math.PI],
      light: color01(def.LightColor),
      quaternion: new THREE.Quaternion(),
    };
    this.orient(round);
    round.trail = def.TrailEffect ? this.effects.play(def.TrailEffect, origin, null, velocity, round.quaternion) : null;
    this.rounds.push(round);
    this.onProjectile(round, "round");
    return round;
  }

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

  bolt(def, origin, direction, owner, visual = null, ignoreBody = null, channel = 0) {
    // A lag-compensated shot meets the others as its shooter saw them
    // (shotTick), but leaves from where he is: his page predicts his own
    // body ahead to the tick the press runs on, so that is where he fired
    // from. Moving the muzzle back to the rewound tick shifted the round by
    // his own movement over the round trip, off a head at any range.
    const history=this.hitHistory, shotTick=history?.shotTick(owner,channel);
    const speed = def.Velocity;
    let life = def.LifeSpan;
    let impact = null;
    const hit = this.collision.cast(origin, direction, speed * life, this.hit, ignoreBody, true);
    if (hit) {
      life = hit.distance / speed;
      impact = { point: hit.point.clone(), normal: hit.normal.clone(), terrain: hit.terrain, body: hit.body };
    }
    const bolt = {
      def,
      owner,
      networkFireId:networkShotFor(owner,channel)?.id ?? 0,
      networkFireChannel:channel,
      networkFireUnit:owner?.id ?? 0,
      networkFireGeneration:owner?.generation ?? 0,
      ignoreBody,
      position: origin.clone(),
      direction: direction.clone().normalize(),
      speed,
      life,
      drawn: 0,
      flown: 0,
      // Drawn offset from the true path at the start, shrinking to nothing
      // by MERGE meters (or the impact, if nearer).
      offset: visual ? visual.clone().sub(origin) : null,
      merge: Math.max(Math.min(MERGE, speed * life), 1e-3),
      impact,
      glow: color01(def.LaserGlowColor),
      light: color01(def.LightColor),
    };
    this.onLaunch(bolt, { tick: shotTick });
    if(shotTick!=null) {
      // Advance real flight one fixed step at a time. A target beyond speed *
      // elapsedTime remains unreachable, and unused life/range stays unchanged.
      let from=shotTick;
      for(let tick=Math.floor(shotTick)+1;tick<=history.tick && bolt.life>0;tick++){this.stepBolt(bolt,(tick-from)/60,false,tick);from=tick;}
    }
    if(bolt.life>0)this.bolts.push(bolt);
    return bolt;
  }

  throw(def, origin, direction, owner, channel = 0) {
    const mesh = this.projectileMesh(def);
    const velocity = direction.clone().normalize().multiplyScalar(def.Velocity);
    const grenade = {
      networkFireChannel:channel,
      def,
      owner,
      position: origin.clone(),
      previous: origin.clone(),
      launchPosition: origin.clone(),
      velocity,
      gravity: -9.8 * (def.Gravity ?? 1),
      slow: 0,
      resting: false,
      flight: 0,
      fuse: null,
      mesh,
      spin: new THREE.Vector3().randomDirection(),
      quaternion: mesh?.quaternion.clone() ?? new THREE.Quaternion(),
      trail: def.TrailEffect ? this.effects.play(def.TrailEffect, origin, UP, velocity) : null,
      light: color01(def.LightColor),
    };
    this.grenades.push(grenade);this.onProjectile(grenade, "grenade");
  }

  update(dt, draw = true) {
    if (draw) this.renderer.begin();
    for (const beam of this.beams) this.stepBeam(beam, dt, draw);
    this.beams = this.beams.filter(b => b.life > 0);
    for (const bolt of this.bolts) this.stepBolt(bolt, dt, draw);
    this.bolts = this.bolts.filter((b) => b.life > 0);
    for (const grenade of this.grenades) this.stepGrenade(grenade, dt);
    this.grenades = this.grenades.filter((g) => !g.gone);
    for (const round of this.rounds) this.stepRound(round, dt);
    this.rounds = this.rounds.filter((r) => !r.gone);
    if (draw) this.renderer.end();
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

  drawProjectileView(projectile, alpha = 1, light = false) {
    this.drawnAt.lerpVectors(projectile.previous, projectile.position, alpha);
    if (projectile.muzzleOffset) this.drawnAt.addScaledVector(projectile.muzzleOffset,
      Math.max(0, 1 - projectile.position.distanceTo(projectile.launchPosition) / MERGE));
    if (projectile.mesh) {
      projectile.mesh.position.copy(this.drawnAt);
      if (projectile.quaternion) projectile.mesh.quaternion.copy(projectile.quaternion);
    }
    projectile.trail?.move?.(this.drawnAt, projectile.quaternion, projectile.velocity);
    if (light) this.lights.add(this.drawnAt, lightOf(projectile), projectile.def.LightRadius || 0);
  }

  present(alpha = 1, clockTick = null, additional = null) {
    this.renderer.begin();
    for (const beam of this.beams) this.drawBeam(beam);
    for (const bolt of this.bolts) {
      // Resolve a local cosmetic muzzle only after the rendered actor/camera
      // pose exists. The physical origin, direction and hit stay tick-owned.
      if (bolt.visualMuzzle || (bolt.mount && !bolt.mountResolved)) {
        const muzzle = bolt.visualMuzzle?.() || mountedMuzzle(bolt);
        bolt.mountResolved = true;
        if (muzzle) {
          const offset = (bolt.offset ||= new THREE.Vector3()).copy(muzzle).sub(bolt.position).addScaledVector(bolt.direction, bolt.flown);
          // A respawn/teleport before the first draw must not pull a trail
          // across the map. Render that old round on its physical path.
          if (offset.lengthSq() > 400) offset.set(0, 0, 0);
        }
        bolt.visualMuzzle = null;
      }
      this.drawBolt(bolt, alpha, clockTick);
    }
    for (const round of this.rounds) {
      if (round.visualMuzzle || (round.mount && !round.mountResolved)) {
        const muzzle = round.visualMuzzle?.() || mountedMuzzle(round);
        if (muzzle && round.launchPosition) {
          round.muzzleOffset = muzzle.clone().sub(round.launchPosition);
          if (round.muzzleOffset.lengthSq() > 400) round.muzzleOffset.set(0, 0, 0);
        }
        round.visualMuzzle = null; round.mountResolved = true;
      }
      this.drawProjectileView(round, alpha);
    }
    for (const grenade of this.grenades) if (grenade.mesh) grenade.mesh.position.lerpVectors(grenade.previous, grenade.position, alpha);
    additional?.();
    this.renderer.end();
  }

  stepRound(r, dt) {
    const def = r.def;
    // Life ran out at the point the spawn ray found: the impact, a frame on.
    if (r.dead) {
      if (r.impact) this.impactAt(r, r.impact.point, r.impact.normal, r.impact.terrain, r.impact.body);
      this.remove(r);
      return;
    }
    if (r.kind === "shell") r.velocity.y += r.gravity * dt;
    if (r.kind === "missile") this.steer(r, dt);
    r.previous.copy(r.position);
    r.position.addScaledVector(r.velocity, Math.min(dt, r.life));
    r.life -= dt;
    if (r.position.y < FLOOR || Math.abs(r.position.x) > LIMIT || Math.abs(r.position.z) > LIMIT) {
      this.remove(r);
      return;
    }
    // Everything meets water coming down through it.
    if (this.water && this.water.crossing(r.previous, r.position, this.wet)) {
      this.water.ripple(this.wet, 1.5, 0.01);
      emitSound("splash", def, this.wet);
      const name = def.ImpactEffectWater ?? "watersplash_ord";
      if (name) this.effects.play(name, this.wet);
      if (def.ExplosionImpact) this.explosion(def.ExplosionImpact, this.mid.copy(this.wet).addScaledVector(UP, 0.2), r.owner);
      this.remove(r);
      return;
    }
    // Curved rounds test all geometry; straight rounds test moving bodies
    // over each segment in addition to their cached static impact.
    {
      const ray = this.ray.subVectors(r.position, r.previous);
      const length = ray.length();
      if (length > 1e-6) {
        const hit = r.kind === "bullet"
          ? this.collision.castMoving?.(r.previous, ray.divideScalar(length), length, this.hit, r.ignoreBody)
          : this.collision.cast(r.previous, ray.divideScalar(length), length, this.hit, r.ignoreBody);
        if (r.kind === "fatray") this.sweepFat(r, ray, hit ? hit.distance : length);
        const unitHit = r.kind === "fatray" ? null : this.castUnits(r.previous, ray, hit ? hit.distance : length, r.owner, null, r.ignoreBody)
          ?? this.riderBehind(r.previous, ray, hit, r.kind === "bullet" ? length + Math.max(r.life, 0) * r.velocity.length() : Infinity, r.owner, null, r.ignoreBody, r.kind === "bullet");
        if (unitHit) {
          this.impactAt(r, unitHit.point, unitHit.normal, false, null, unitHit.unit, unitHit.part);
          this.remove(r);
          return;
        }
        if (hit) {
          this.impactAt(r, hit.point, hit.normal, hit.terrain, hit.body);
          this.remove(r);
          return;
        }
      }
    }
    if (r.life <= 0) {
      if (def.ExplosionExpire) this.explosion(def.ExplosionExpire, r.position, r.owner);
      if (r.trail) r.trail.stop();
      r.trail = null;
      if (r.mesh) r.mesh.visible = false;
      if (r.impact) r.dead = true;
      else this.remove(r);
      return;
    }
    this.orient(r);
    if (r.mesh) {
      r.mesh.position.copy(r.position);
      r.mesh.quaternion.copy(r.quaternion);
    }
    if (r.trail) r.trail.move(r.position, r.quaternion, r.velocity);
    this.lights.add(this.mid.addVectors(r.previous, r.position).multiplyScalar(0.5), lightOf(r), def.LightRadius || 0);
  }

  // A missile turns toward where it wants to go at TurnRate (no target: the
  // way it was fired) and wavers by WaverTurn, both eased in over ScaleTime.
  steer(r, dt) {
    const def = r.def;
    const target = this.units?.byId?.(r.targetId);
    if (r.flightMissile) { if (target?.alive && (target.generation ?? 0) === r.targetGeneration) navigateMissile(r, target, dt); return; }
    if (target?.alive && (target.generation ?? 0) === r.targetGeneration) {
      if (target.aimPoint) target.aimPoint(this.mid);
      else this.mid.copy(target.position);
      r.desired.subVectors(this.mid, r.position).normalize();
    } else if (r.targetId >= 0) {
      r.targetId = -1;
    }
    r.ramp = Math.min(r.ramp + dt / (def.ScaleTime || 1), 1);
    const v = r.velocity;
    const speed = v.length();
    if (speed < 1e-6) return;
    const forward = this.up.copy(v).divideScalar(speed);
    const turn = (def.TurnRate ?? 1) * r.ramp;
    if (turn > 0) {
      const axis = this.axis.crossVectors(forward, r.desired);
      const sine = axis.length();
      if (sine * sine > 1e-8) v.applyAxisAngle(axis.divideScalar(sine), Math.min(sine, turn * dt));
    }
    const amp = (def.WaverTurn || 0) * r.ramp;
    if (amp > 0) {
      this.right.crossVectors(UP, forward).normalize();
      const up = this.start.crossVectors(forward, this.right);
      const w = this.axis.copy(up).multiplyScalar(Math.sin(r.phase[1])).addScaledVector(this.right, Math.sin(r.phase[0]));
      const size = w.length();
      if (size > 1e-6) v.applyAxisAngle(w.divideScalar(size), amp * dt * size);
      r.phase[0] += (this.random || Math.random)() * 2 * (def.WaverRate || 0) * dt;
      r.phase[1] += (this.random || Math.random)() * 2 * (def.WaverRate || 0) * dt;
    }
  }

  // What hit: its impact effect where it hit, then ExplosionImpact just off
  // the surface.
  impactAt(r, point, normal, terrain, body = null, unit = null, part = null) {
    const def = r.def;
    this.onImpact(r, { point, normal, surface: unit ? ((unit.classDef?.HealthType || "").toLowerCase() === "droid" ? 3 : 2) : terrain ? 0 : 1 });
    emitSound("impact", def, point, terrain, body);
    this.axis.copy(r.velocity).normalize();
    if (body) this.onHit(body, def, r.owner, this.axis);
    if (unit) this.onHitUnit(unit, def, point, r.owner, part, this.axis);
    this.lights.add(this.mid.copy(point).addScaledVector(normal, 0.5), lightOf(r), def.LightRadius || 0);
    const name = unit ? this.unitImpactEffect(def, unit) : terrain ? def.ImpactEffectTerrain ?? "dirtspray" : def.ImpactEffectStatic;
    if (name) this.effects.play(name, point);
    if (def.ExplosionImpact) this.explosion(def.ExplosionImpact, this.mid.copy(point).addScaledVector(normal, 0.2), r.owner);
  }

  remove(r) {
    r.gone = true;
    if (r.mesh) this.scene.remove(r.mesh);
    if (r.trail) r.trail.stop();
  }

  stepBolt(bolt, dt, draw = true, historyTick = null) {
    bolt.previousFlown = bolt.flown; bolt.previousDrawn = bolt.drawn;
    const def = bolt.def;
    const step = Math.min(dt, bolt.life);
    this.start.copy(bolt.position);
    bolt.position.addScaledVector(bolt.direction, bolt.speed * step);
    const movingHit = this.collision.castMoving?.(this.start, bolt.direction, bolt.speed * step, this.hit, bolt.ignoreBody);
    const unitHit = this.castUnits(this.start, bolt.direction, movingHit ? movingHit.distance : bolt.speed * step, bolt.owner, historyTick, bolt.ignoreBody)
      ?? this.riderBehind(this.start, bolt.direction, movingHit, bolt.speed * bolt.life, bolt.owner, historyTick, bolt.ignoreBody, true);
    if (unitHit) {
      bolt.position.copy(unitHit.point);
      bolt.life = 0;
      bolt.impact = { point: unitHit.point.clone(), normal: unitHit.normal.clone(), terrain: false, body: null, unit: unitHit.unit, part: unitHit.part };
    } else if (movingHit) {
      bolt.position.copy(movingHit.point);
      bolt.life = 0;
      bolt.impact = { point: movingHit.point.clone(), normal: movingHit.normal.clone(), terrain: false, body: movingHit.body };
    }
    // Water stops a bolt where it goes in, with a ring and a splash.
    if (this.water && bolt.life > 0 && this.water.crossing(this.start, bolt.position, this.wet)) {
      this.water.ripple(this.wet, 1.5, 0.01);
      emitSound("splash", def, this.wet);
      const name = def.ImpactEffectWater ?? "watersplash_ord";
      if (name) this.effects.play(name, this.wet);
      bolt.life = 0;
      this.onImpact(bolt, { point: this.wet.clone(), normal: UP.clone(), surface: 4 });
      bolt.impact = null;
      return;
    }
    bolt.flown += bolt.speed * step;
    bolt.drawn = Math.min(bolt.drawn + bolt.speed * dt, Math.max(def.LaserLength || 1, glowLength(def), def.BlurLength || 0));
    bolt.life -= dt;
    if (bolt.life <= 0) {
      this.onImpact(bolt, bolt.impact ? { ...bolt.impact, surface: bolt.impact.unit ? ((bolt.impact.unit.classDef?.HealthType || "").toLowerCase() === "droid" ? 3 : 2) : bolt.impact.terrain ? 0 : 1 } : { point: bolt.position, normal: UP, surface: 5 });
      if (bolt.impact) {
        emitSound("impact", def, bolt.impact.point, bolt.impact.terrain, bolt.impact.body);
        const name = bolt.impact.unit ? this.unitImpactEffect(def, bolt.impact.unit)
          : bolt.impact.terrain ? def.ImpactEffectTerrain : def.ImpactEffectStatic || def.ImpactEffectTerrain;
        if (name) this.effects.play(name, bolt.impact.point);
        if (bolt.impact.body) this.onHit(bolt.impact.body, def, bolt.owner, bolt.direction);
        if (bolt.impact.unit) this.onHitUnit(bolt.impact.unit, def, bolt.impact.point, bolt.owner, bolt.impact.part, bolt.direction);
        // A bolt with an explosion (the orbital strike's) sets it off there.
        if (def.ExplosionImpact) this.explosion(def.ExplosionImpact, this.mid.copy(bolt.impact.point).addScaledVector(bolt.impact.normal, 0.2), bolt.owner);
        bolt.impact = null;
      } else if (def.ExplosionExpire) {
        // A bolt that bursts at the end of its flight (the AAA turret's flak):
        // its position has flown with it.
        this.explosion(def.ExplosionExpire, bolt.position, bolt.owner);
      }
      return;
    }
    if (draw) this.drawBolt(bolt);
  }

  drawBolt(source, alpha = 1, clockTick = null) {
    const bolt = this.visualBolt ||= {};
    const linear=source.networkFlightTick!==undefined;
    const frame=linear ? linearBoltFrame(source,clockTick ?? source.networkFlightTick,this.linearFrame ||= {}) : null;
    if(linear && !frame)return;
    const flown = frame ? frame.flown : (source.previousFlown ?? source.flown) + (source.flown - (source.previousFlown ?? source.flown)) * alpha;
    const drawn = frame ? frame.drawn : (source.previousDrawn ?? source.drawn) + (source.drawn - (source.previousDrawn ?? source.drawn)) * alpha;
    Object.assign(bolt, source); bolt.flown = flown; bolt.drawn = drawn;
    // Past the end of its flight a class with a FadeOutTime is still drawn
    // where it stopped (linearBoltFrame, PredictedShots): it fades, and its
    // width falls to a tenth (0.9 x fade + 0.1).
    const ended = frame ? frame.ended : Boolean(source.ended);
    const fade = frame ? frame.fade : source.fade ?? 1;
    const thin = 0.9 * fade + 0.1;
    // A fast bolt can travel several metres before its first rendered
    // frame. Show that first interval back to the muzzle, then use the
    // normal streak length. This never changes collision or flight time.
    bolt.firstDraw = !source.presented && !ended;
    if (flown > 1e-8) source.presented = true;
    const def = bolt.def;
    const p = this.drawnAt.copy(bolt.position).addScaledVector(bolt.direction, flown - source.flown);
    if (bolt.offset) p.addScaledVector(bolt.offset, Math.max(1 - bolt.flown / bolt.merge, 0));
    // The light rides at the head; once the flight is over it stays lit at
    // the middle of the last step until the bolt is gone.
    const light = ended ? (this.fadeLight ||= new THREE.Vector3()).copy(p).addScaledVector(bolt.direction, -Math.min(flown, bolt.speed / 60) / 2) : p;
    // Enhanced sniper shots (sniper-trace.js): a rail from the gun as drawn
    // to where the round is, outliving it as vapour and sparks.
    if (sniperLook.enhanced && def.FadeOutTime > 0) {
      const origin = (this.sniperOrigin ||= new THREE.Vector3()).copy(bolt.position).addScaledVector(bolt.direction, -source.flown);
      const start = (this.sniperStart ||= new THREE.Vector3()).copy(origin);
      if (bolt.offset) start.add(bolt.offset);
      // The head on the round's own line, without the drawn muzzle's offset.
      const head = (this.sniperHead ||= new THREE.Vector3()).copy(origin).addScaledVector(bolt.direction, flown);
      this.sniperTrace().touch(source, def, start, head, bolt.direction, ended, fade, origin);
      return;
    }
    if (boltLook.enhanced) {
      const look = this.blasterLook(def);
      const length = this.boltSpan(bolt, Math.min(look.length, bolt.drawn));
      const tail = (this.blasterTail ||= new THREE.Vector3()).set(...this.boltAxis).multiplyScalar(-length).add(p);
      // Presentation only: a stable flicker phase for each bolt.
      source.seed ??= this.boltSeed = ((this.boltSeed || 0) + 0.618034) % 1;
      this.renderer.custom("blaster", () => new BlasterBatch()).push(p, tail, look.color, look.brightness * fade, look.core * thin, look.glow * thin, source.seed, 0.55);
      this.lights.add(light, look.light, look.lightRadius);
      return;
    }
    let style = this.boltStyles.get(def);
    if (!style) {
      const core = def.LaserTexture ? this.texture(def.LaserTexture) : null;
      const glow = this.texture("lightglow");
      style = { width: def.LaserWidth || 0.1, color: color01(def.LaserGlowColor),
        core: core ? this.renderer.batch(core, "additive", "bolt") : null,
        glow: glow ? this.renderer.batch(glow, "additive") : null };
      this.boltStyles.set(def, style);
    }
    const width = style.width * thin;
    // Fading, the core's alpha and every channel of the glow's colour are
    // scaled by what is left of the bolt.
    if (style.core) {
      const length = this.boltSpan(bolt, Math.min(def.LaserLength || 1, bolt.drawn));
      style.core.push(p.x, p.y, p.z, 1, 1, 1, fade, width / 2, 0, STREAK, length, this.boltAxis, CORE_TAPER);
    }
    if (style.glow && glowLength(def)) {
      const length = this.boltSpan(bolt, Math.min(glowLength(def), bolt.drawn));
      const [r, g, b] = style.color, bright = 1.35 * fade;
      style.glow.push(p.x, p.y, p.z, r * bright, g * bright, b * bright, fade, width * 1.2, 0, STREAK, length, this.boltAxis, GLOW_TAPER);
      style.glow.push(p.x, p.y, p.z, r * fade, g * fade, b * fade, 0.28 * fade, width * 3.2, 0, STREAK, length, this.boltAxis, GLOW_TAPER);
    }
    this.lights.add(light, lightOf(source), def.LightRadius || 0);
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

  // Enhanced bolts' proportions from the ordnance's own numbers: a core a
  // quarter of LaserWidth across inside a glow as wide as it, at least
  // LaserLength long (longer for rifles, whose glow ran past their core),
  // and a light every bolt carries, heavy guns' included. Guns with no
  // GlowLength are the vehicles' and turrets': the 2004 game drew only a thin
  // core for them, so they get a heavier minimum here.
  blasterLook(def) {
    let look = this.blasterLooks?.get(def);
    if (look && !look.pending) return look;
    if (!look) {
      const width = def.LaserWidth || 0.1;
      const heavy = !def.GlowLength;
      const glow = Math.max(width, heavy ? 0.26 : 0);
      look = {
        color: vivid(sheathColor(def.LaserGlowColor || def.LightColor)), light: [0, 0, 0], glow,
        core: Math.min(Math.max(width * 0.24, heavy ? 0.05 : 0), 0.11),
        length: Math.max(def.LaserLength || 1, (def.GlowLength || 0) * 0.6, width * 6, heavy ? 2.5 : 0),
        brightness: width >= 0.3 ? 1.15 : 1,
        lightRadius: Math.max(def.LightRadius || 0, 2.5 + glow * 5),
        // A gun without a glow showed only its core texture: take its hue
        // from that texture once the image has loaded.
        pending: heavy && def.LaserTexture ? this.texture(def.LaserTexture) : null,
      };
      (this.blasterLooks ||= new WeakMap()).set(def, look);
    }
    if (look.pending) {
      const hue = textureHue(look.pending);
      if (hue !== undefined) {
        if (hue) look.color = vivid(hue);
        look.pending = null;
      }
    }
    for (let i = 0; i < 3; i++) look.light[i] = look.color[i] * 0.75;
    return look;
  }

  // The visible path converges from the displayed muzzle to the gameplay
  // ray. Compute both ends on that path; merely offsetting the head leaves
  // the first-person streak pointing beside the barrel it came from.
  boltSpan(bolt, span) {
    if (bolt.firstDraw) span = Math.max(span, bolt.flown);
    const head = Math.max(1 - bolt.flown / bolt.merge, 0);
    const tail = Math.max(1 - Math.max(0, bolt.flown - span) / bolt.merge, 0);
    const offset = head - tail, dir = this.boltAxis;
    dir[0] = bolt.direction.x * span + (bolt.offset?.x || 0) * offset;
    dir[1] = bolt.direction.y * span + (bolt.offset?.y || 0) * offset;
    dir[2] = bolt.direction.z * span + (bolt.offset?.z || 0) * offset;
    const length = Math.hypot(...dir);
    if (length > 1e-8) { dir[0] /= length; dir[1] /= length; dir[2] /= length; }
    else { dir[0] = bolt.direction.x; dir[1] = bolt.direction.y; dir[2] = bolt.direction.z; }
    return length;
  }

  stepGrenade(g, dt) {
    const def = g.def;
    g.flight += dt;
    if (!g.resting) {
      if (g.sinking) g.velocity.set(0, -0.2, 0);
      else g.velocity.y += g.gravity * dt;
      g.previous.copy(g.position);
      g.position.addScaledVector(g.velocity, dt);
      // Into the water: a ring and a splash, then it sinks at 0.2 m/s with
      // its fuse running.
      if (!g.sinking && this.water && this.water.crossing(g.previous, g.position, this.wet)) {
        g.sinking = true;
        g.velocity.set(0, -0.2, 0);
        this.water.ripple(this.wet, 1.5, 0.01);
        emitSound("splash", def, this.wet);
        const name = def.ImpactEffectWater ?? "watersplash_ord";
        if (name) this.effects.play(name, this.wet);
        if (g.fuse === null) g.fuse = def.LifeSpan || 0;
      }
      this.bounce(g);
      g.quaternion.multiply(this.grenadeTurn.setFromAxisAngle(g.spin, g.velocity.length() * dt * 2));
      if (g.mesh) g.mesh.quaternion.copy(g.quaternion);
    }
    if (g.resting && g.fuse === null) g.fuse = def.LifeSpan || 0;
    if (g.fuse !== null) g.fuse -= dt;
    if (g.mesh) g.mesh.position.copy(g.position);
    if (g.trail) g.trail.move(g.position, null, g.velocity);
    this.lights.add(g.position, lightOf(g), def.LightRadius || 0);
    if ((g.fuse !== null && g.fuse <= 0) || g.flight > MAX_FLIGHT || g.position.y < FLOOR) this.explode(g);
  }

  // Sweep from just behind where the grenade was to just past where it is.
  bounce(g) {
    const ray = this.ray.subVectors(g.position, g.previous);
    const length = ray.length();
    if (length < 1e-6) return;
    ray.divideScalar(length);
    this.start.copy(g.previous).addScaledVector(ray, -0.1);
    const hit = this.collision.cast(this.start, ray, length + 0.3, this.hit);
    if (!hit) return;
    const n = hit.normal;
    const v = g.velocity;
    const d = v.dot(n);
    if (d >= 0) return;
    g.position.copy(hit.point).addScaledVector(n, 0.1);
    emitSound("bounce", g.def, g.position, -d);
    if (!g.touched) {
      g.touched = true;
      this.onGrenadeContact(g);
    }
    if (this.sticks(g.def, hit)) {
      v.set(0, 0, 0);
      g.resting = true;
      return;
    }
    v.addScaledVector(n, -d);
    const along = v.length();
    if (along > 0) v.multiplyScalar(Math.max(along - Math.min(FRICTION * -d, along), 0) / along);
    v.addScaledVector(n, RESTITUTION * -d);
    if (v.length() < SLOW && ++g.slow >= SLOW_BOUNCES) {
      v.set(0, 0, 0);
      g.resting = true;
    }
  }

  // Sticky ordnance holds onto the kinds of thing its Stick* properties name.
  sticks(def, hit) {
    return hit.terrain ? def.StickTerrain > 0 : def.StickBuilding > 0;
  }

  explode(g) {
    g.gone = true;
    if (g.mesh) this.scene.remove(g.mesh);
    if (g.trail) g.trail.stop();
    if (g.def.ExplosionExpire) this.explosion(g.def.ExplosionExpire, g.position, g.owner);
  }

  // The first unit a segment meets, other than the round's owner (and any
  // the battle's rules exclude), within `far`: soldiers on foot, bodies
  // lying where they fell, and riders shown on exposed seats (src/
  // rider-pose.js). A round from a hull's or turret's own guns (ignoreBody:
  // the body it passes through) leaves the riders on that hull alone.
  castUnits(origin, direction, far, owner, historyTick = null, ignoreBody = null) {
    if (!this.units || far <= 0) return null;
    const shooter = owner && typeof owner === "object" ? resolveOrdnanceOwner(owner) : null;
    let filter = null;
    if (this.canHit || ignoreBody || this.striking?.mount) {
      this.striking.owner = owner;
      this.striking.body = ignoreBody;
      filter = this.striking.filter;
    }
    if(historyTick!=null && this.hitHistory)return this.hitHistory.raycast(historyTick,origin,direction,far,shooter,this.unitHit,filter,true);
    return this.units.raycast(origin, direction, far, shooter, this.unitHit, filter, true);
  }

  // Whether a round may strike this unit: not a rider on the hull that fired
  // it, only a rider of the mount it is passing through (riderBehind), and
  // whatever the battle's rules (canHit) allow.
  mayStrike(owner, unit, ignoreBody = null) {
    if (ignoreBody && unit.seat && (unit.seat.vehicle?.body ?? unit.seat.turret?.bodies?.built) === ignoreBody) return false;
    if (this.striking.mount && unit.seat?.vehicle !== this.striking.mount) return false;
    return !this.canHit || this.canHit(owner, unit);
  }

  // A round that met a hull whose collision doesn't keep rounds off its
  // riders (Vehicle.shieldsRiders: a riding animal's) goes on through it to
  // a rider of it, if it crosses his body before anything else, met this
  // step even if he is a little past where the step ends. far: as far as
  // the round can still fly (a bolt's or rocket's world was traced at its
  // launch: then only moving bodies are tested here). The rider's hit, or
  // null.
  riderBehind(origin, direction, hit, far, owner, historyTick, ignoreBody, moving) {
    const mount = hit?.body?.owner;
    if (!mount || mount.shieldsRiders?.() !== false || !mount.occupied) return null;
    const reach = Math.min(far, hit.distance + 2 * (mount.boundRadius || 1));
    const next = moving ? this.collision.castMoving?.(origin, direction, reach, this.behind, mount.body) : this.collision.cast(origin, direction, reach, this.behind, mount.body);
    this.striking.mount = mount;
    const rider = this.castUnits(origin, direction, next ? next.distance : reach, owner, historyTick, ignoreBody);
    this.striking.mount = null;
    return rider;
  }

  // A soldier struck: droids spark like metal, the rest take the soft hit.
  unitImpactEffect(def, unit) {
    const droid = (unit.classDef?.HealthType || "").toLowerCase() === "droid";
    return (droid ? def.ImpactEffectRigid : def.ImpactEffectSoft) || def.ImpactEffectStatic || def.ImpactEffectTerrain;
  }

  // An explosion: its effect (its WaterEffect at the water's surface, with a
  // ring), its light, and its damage for the caller.
  explosion(explosion, position, owner, presentation = explosion) {
    // Presentation may suppress stock sprites; replication keeps matched content identity.
    const wet = this.presentExplosion(presentation, position);
    this.onExplode(explosion, position.clone(), owner, { wet });
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

// The falloff the engine uses for damage, push, and shake: full inside the
// inner radius, down to nothing at the outer.
export function falloff(distance, inner, outer) {
  if (distance < inner) return 1;
  if (distance < outer && outer > inner) return (outer - distance) / (outer - inner);
  return 0;
}

// What an explosion does to each thing in its reach, the same for soldiers,
// droids, vehicles, buildings (turrets, recharge droids), mines lying about
// and recon droids, following the 2004 game (ExplosionClass::Explode).
// The distance runs from the blast to the centre of the thing's bounding
// sphere, less half the sphere's radius (blastDistance); damage, push and
// shake fall off over it (falloff). Terrain or a building between the blast
// and the thing's aim point shields it from damage and push alike; a soldier
// or a vehicle never does, nor the thing's own body (blastBlocked). Damage
// is the explosion's Damage by the falloff and by the scale for the thing's
// HealthType (blastDamage), then by the battle's friendly fire factor for
// the thrower's team (the caller's). A round that strikes something also
// does its own Damage by the same scale (healthScale), and its explosion goes
// off 0.2 m off the surface, so the thing struck takes both.
export const HEALTH_SCALES = { person: "PersonScale", animal: "AnimalScale", droid: "DroidScale", vehicle: "VehicleScale", building: "BuildingScale" };

// An ordnance's or explosion's scale for a health type (person when unset).
export function healthScale(def, healthType = "person") {
  return def[HEALTH_SCALES[String(healthType).toLowerCase()] ?? "PersonScale"] ?? 1;
}

export function blastDistance(position, center, radius) {
  return center.distanceTo(position) - 0.5 * radius;
}

// The share of the explosion's Damage at that distance, before any scale.
export function blastShare(explosion, distance) {
  return falloff(distance, explosion.DamageRadiusInner || 0, explosion.DamageRadiusOuter || 0);
}

export function blastDamage(explosion, distance, healthType = "person") {
  return (explosion.Damage || 0) * blastShare(explosion, distance) * healthScale(explosion, healthType);
}

const SIGHT = new THREE.Vector3();
const SIGHT_HIT = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
export function blastBlocked(collision, position, target, ignoreBody = null) {
  const span = SIGHT.subVectors(target, position).length();
  if (!collision || span < 1e-3) return false;
  // Moving bodies (vehicles) never shield; static ones (buildings) do.
  const hit = collision.cast(position, SIGHT.divideScalar(span), span, SIGHT_HIT, ignoreBody, true);
  return Boolean(hit && hit.distance < span - 0.05);
}
