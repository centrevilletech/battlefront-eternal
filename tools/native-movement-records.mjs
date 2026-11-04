// Trace the existing bridge and compare every soldier tick to its JS core.
// Geometry and exchange values are encoded as bits for native replay.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import * as THREE from 'three';
import { Soldier } from '../src/soldier.js';

export const hex = a => Buffer.from(a.buffer, a.byteOffset, a.byteLength).toString('hex');
export const f64 = a => hex(Float64Array.from(a));
export const digest = data => createHash('sha256').update(data).digest('hex');
const identity = new THREE.Matrix4().elements;
const packed = (bvh, polygons = []) => ({ p: hex(bvh.geometry.attributes.position.array), wide: bvh.geometry.attributes.position.array instanceof Float64Array, i: [...bvh.geometry.index.array], polygons: [...polygons] });
const placement = b => f64([...b.bounds.min.toArray(), ...b.bounds.max.toArray(), ...(b.matrix?.elements ?? identity), ...(b.inverse?.elements ?? identity)]);
const vectors = ['position','settled','previous','velocity','normal','avoidance'];
const numbers = ['aimYaw','aimPitch','stateTime','lock','fuel','dive','groundAdjustment','collisionDamage','tickLength','lookX','lookY'];
const codes = ['posture','air','diveFrom','grounded'];

export class MovementTrace {
  constructor(file, header) {
    this.fd = fs.openSync(file, 'wx'); this.hash = createHash('sha256'); this.lines = 0;
    this.scenes = new Map(); this.ticks = 0; this.creates = 0; this.bodyChanges = 0;
    this.postures = new Set(); this.airs = new Set(); this.flags = new Set();
    this.emit({op:'header', ...header});
  }
  emit(value) {
    const text = JSON.stringify(value) + '\n'; fs.writeFileSync(this.fd, text); this.hash.update(text); this.lines++;
  }
  scene(world, cx) {
    let scene = this.scenes.get(world); if (scene) return scene;
    assert(world.rays); assert.equal(world.rayWorld, 0);
    const trees = new Map([[world.bvh, 0]]);
    if (world.terrain) trees.set(world.terrain, 1);
    scene = {cx, world, trees, bodies:[], placements:[], order:null, maxGather:0}; this.scenes.set(world, scene);
    const gather = world.gather;
    world.gather = function (...args) { const polygons=gather.apply(this,args); scene.maxGather=Math.max(scene.maxGather,polygons.length); return polygons; };
    this.emit({cx,op:'install',buildings:packed(world.bvh, world.polygonOf),terrain:world.terrain ? packed(world.terrain) : null});
    return scene;
  }
  sync(scene) {
    const {world, cx} = scene;
    for (let id = 0; id < world.rays.bodies.length; id++) {
      const body = world.rays.bodies[id]; assert(body, `missing body ${id}`);
      const v = placement(body);
      if (!scene.bodies[id]) {
        let tree = scene.trees.get(body.bvh);
        if (tree === undefined) {
          tree = scene.trees.size; scene.trees.set(body.bvh, tree);
          this.emit({cx,op:'shape',tree,...packed(body.bvh,body.polygonOf)});
        }
        this.emit({cx,op:'body',id,tree,moving:body.moving,offset:body.offset,v});
        scene.bodies[id] = body; scene.placements[id] = v;
      } else if (v !== scene.placements[id]) {
        this.emit({cx,op:'place',id,v}); scene.placements[id] = v; this.bodyChanges++;
      }
    }
    const order = world.bodies.map(b => b.raySlot), key = order.join(',');
    if (key !== scene.order) { this.emit({cx,op:'order',order}); scene.order = key; }
  }
  attach(kernel, cx, rawFlags = false) {
    if (kernel.movementTrace === this) return;
    const trace = this, world = kernel.collision, scene = this.scene(world,cx), exports = kernel.x;
    kernel.movementTrace = this;
    kernel.x = {...exports,
      mover_new(worldId) {
        const id = exports.mover_new(worldId); trace.sync(scene);
        const io = new Float64Array(exports.memory.buffer,exports.movement_io_ptr(),80);
        trace.emit({cx,op:'new',id,class:f64(io.slice(50,71))}); trace.creates++; return id;
      },
      mover_tick(id) {
        trace.sync(scene);
        const flags = new Uint8Array(exports.memory.buffer,exports.movement_flags_ptr(),world.bodies.length);
        if (rawFlags) for (let i=0;i<flags.length;i++) if (flags[i]) flags[i]=[1,2,255][i%3];
        const values = () => new Float64Array(exports.memory.buffer,exports.movement_io_ptr(),80);
        trace.postures.add(values()[21]); trace.airs.add(values()[22]); for (const flag of flags) trace.flags.add(flag);
        const input = f64(values()), enabled = [...flags], code = exports.mover_tick(id), expected = f64(values());
        trace.emit({cx,op:'tick',id,input,flags:enabled,code,expected}); trace.ticks++; return code;
      },
    };
  }
  tick(kernel, soldier, input, dt, run) {
    const result=run(); assert.equal(result,true,'movement must use the kernel');
    return result;
  }
  finish(extra={}) {
    const summary={op:'summary',contexts:this.scenes.size,ticks:this.ticks,creates:this.creates,bodyChanges:this.bodyChanges,postures:[...this.postures],airs:[...this.airs],flags:[...this.flags],maxGather:Math.max(...[...this.scenes.values()].map(s=>s.maxGather)),...extra};
    this.emit(summary); fs.closeSync(this.fd);
    return {...summary,lines:this.lines,sha256:this.hash.digest('hex')};
  }
}
