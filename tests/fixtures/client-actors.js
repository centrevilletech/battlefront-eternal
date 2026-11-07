import './native-anim.js';
import * as THREE from 'three';
import { CollisionWorld } from '../../src/collision.js';
import { Soldier } from '../helpers/body.js';
import { Droideka } from '../helpers/droideka.js';
import { Clip } from '../../src/anim.js';
import { PlayerCombat } from '../../src/combat.js';

function box(x, y, z, width, height, depth, polygon = 0) {
  const g = new THREE.BoxGeometry(width, height, depth).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: Uint32Array.from(g.index.array),
    polygons: Uint32Array.from({ length: g.index.count / 3 }, (_, i) => polygon + Math.floor(i / 2)) };
}
function merge(parts) {
  const positions = [], indices = [], polygons = [];
  for (const p of parts) { const offset = positions.length / 3; positions.push(...p.positions); indices.push(...p.indices.map(i => i + offset)); polygons.push(...p.polygons); }
  return { positions: Float32Array.from(positions), indices: Uint32Array.from(indices), polygons: Uint32Array.from(polygons) };
}

export function actorCourse(rays = null, geometry = null) {
  const parts = [box(0, -.5, 0, 60, 1, 60), box(8, 1.5, 0, .5, 3, 20, 6), box(4, 1.3, -10, 8, .2, 8, 12), box(-12, 5, 8, 5, 10, 5, 18)];
  for (let i = 0; i < 5; i++) parts.push(box(0, (i + 1) * .15, 7 + i * .6, 4, (i + 1) * .3, .6, 24 + i * 6));
  const data = geometry || merge(parts), world = new CollisionWorld(data.positions, data.indices, data.polygons,
    data.terrainPositions, data.terrainIndices, rays);
  const hull = box(0, 0, 0, 3, 1.5, 3), shape = world.createBodyShape(hull.positions, hull.indices, hull.polygons);
  const moving = world.addMovingBody(shape, new THREE.Matrix4().setPosition(-4, .75, 3), { name: 'moving' });
  const prop = box(-2, 1, 4, 1, 2, 1), fixed = world.addBody(prop.positions, prop.indices, prop.polygons, { name: 'fixed' });
  return { world, moving, fixed, geometry: data, update(tick) {
    world.moveBody(moving, new THREE.Matrix4().makeRotationY(tick * .007).setPosition(-4 + Math.sin(tick * .03) * 2, .75, 3));
    fixed.enabled = tick % 70 < 45;
  }, dispose() { world.removeBody(moving); world.removeBody(fixed); shape.bvh.geometry.dispose(); world.bvh.geometry.dispose(); } };
}

export function actorClips() {
  const clips = new Map();
  for (const name of ['idle', 'turnl', 'turnr', 'death01', 'idle_to_leftfoot', 'walk_leftfoot_rightfoot', 'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold']) {
    const frames = name === 'death01' ? 43 : name === 'fold' || name === 'unfold' ? 19 : 13, data = new Float32Array(frames * 7);
    for (let i = 0; i < frames; i++) { data[i * 7 + 3] = 1; data[i * 7 + 6] = name.includes('foot') ? i / 30 + (i % 3) / 300 : 0; }
    clips.set(name, new Clip(name, { frames, tracks: [0], offset: 0, flags: 1, travel: [0, 0, .4] }, data, 30));
  }
  return clips;
}

export function createPredictionActor(scenario, course, content = null) {
  const kind = scenario.kind, clips = content?.clips || actorClips();
  let soldier, actor;
  if (kind === 'droideka') {
    const root = new THREE.Group(), bone = new THREE.Bone(); bone.name = 'dummyroot'; root.add(bone);
    actor = content?.actor?.() || { root, bones: [bone] };
    soldier = new Droideka(course.world, content?.definition || { AnimationSpeed: 2, ...scenario.definition }, clips, actor);
    soldier.arm(content?.weapons || [{ aimers: ['dummyroot'] }, { base: 'shield', MaxShield: 100, AddShield: -15, AddShieldOff: 5 }]);
    soldier.waterClock = 3.25;
  } else soldier = new Soldier(course.world, content?.definition || scenario.definition || {});
  soldier.spawn(...(scenario.spawn || [0, 0, 0, 0]));
  // The world's first shot leaves the shared barrel cursor on its second step.
  if (kind === 'droideka' && soldier.guns.length) { soldier.lastGun = { index: 0, barrel: 0 }; soldier.gun = 1; }
  soldier.getupTime = content?.getupTime ?? .9; soldier.diveTime = content?.diveTime ?? .6; soldier.maxFlyHeight = content?.maxFlyHeight ?? 14;
  if (scenario.patch) for (const [key, value] of Object.entries(scenario.patch)) {
    if (soldier[key]?.isVector3) soldier[key].fromArray(value); else soldier[key] = value;
  }
  if (scenario.ignoreMount) soldier.passThrough = course.moving;
  const weapons = [{ clip: 17, reserve: 34, timer: .2, def: { base: 'blaster' } }, { def: { base: 'shield' }, heat: .4 }];
  const combat = Object.assign(Object.create(PlayerCombat.prototype), { soldier, kick: { rate: .4, hold: .8, offset: .1, extra: 'retained' },
    gunUnderwater: false, arsenal: { weapons, current: channel => weapons[channel] },
    tick() { throw Error('prediction stepped authoritative combat'); }, fire() { throw Error('prediction fired an authoritative weapon'); } });
  for(const key of ['origin','pivot','left','up','aimDir','eyePoint'])combat[key]=new THREE.Vector3();
  if (scenario.kick) combat.kick = structuredClone(scenario.kick);
  let deathCalls = 0;
  if (kind === 'droideka') { soldier.onDeathBlast = () => deathCalls++; soldier.onExploded = () => deathCalls++; }
  const callbacks = [soldier.onDeathBlast, soldier.onExploded], arsenal = structuredClone(weapons), kick = structuredClone(combat.kick);
  return { soldier, combat, actor, clips, callbacks, arsenal, kick, deathCalls: () => deathCalls };
}
