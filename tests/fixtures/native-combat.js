import * as THREE from 'three';
import { f3World } from './f3-world.js';

// A completed synthetic body enters ownership once. Tests change only
// controller input or qualified combat commands after this construction.
export async function nativeCombat({health=300, base='soldier', posture='stand', wall=null,
  chunks=0, shield=null, velocity=null, position=null, mode=0, weapons=[], onShield=null, definition={}} = {}) {
  const log = {deaths:0, apart:0, died:0};
  const bodyDef = {class:'body', base, MaxHealth:health, weapons, ...definition,
    ...(chunks ? {chunks:[{}], ChunkFrequency:chunks} : {})};
  const world = await f3World({body:bodyDef}, {
    prepareCollision:collision => {
      if (wall === null) return;
      const geometry = new THREE.PlaneGeometry(100, wall.height ?? 5).rotateY(Math.PI/2).translate(wall.x ?? wall, (wall.height ?? 5)/2, 0);
      collision.addBody(geometry.attributes.position.array, geometry.index.array, new Uint32Array(geometry.index.count/3));
    },
    prepareUnit:unit => {
      const body = unit.soldier;
      body.spawn(0,0,0,0);
      if (base === 'walkerdroid') {
        body.arm(weapons);
        body.mode = mode;
        if (shield) body.shield = shield;
      } else body.posture = posture;
      if (velocity) body.velocity.copy(velocity);
      if (position) {body.position.copy(position);body.previous.copy(position);body.settled.copy(position);body.grounded=false;}
      unit.alive = true; unit.removed = false; unit.generation = 1; unit.team = 1;
    },
    bodyNotice:()=>log.apart++,
    deathNotice:unit=>{ log.deaths++; if (!unit.combat.blowApart) log.died++; },
    shieldHit:onShield,
  });
  const unit = world.factory.build('body');
  unit.combat.log = log;
  return {world, unit, combat:unit.combat, soldier:unit.soldier};
}
