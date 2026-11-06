// The page's debris is driven explicitly by the real-content probe. Only
// model/bone/geometry queries and scene/effect calls are served in replay;
// piece motion and lifetime are always computed by native Debris.
import * as THREE from 'three';
import { EffectHandle } from '../../src/presentation/effects.js';
import { DebrisIds, debrisChunk, debrisState } from './debris-state.mjs';

const books = new WeakMap();
const meshState = (mesh, ids) => ({ id: ids.key('mesh', mesh), position: mesh.position, quaternion: mesh.quaternion, scale: mesh.scale,
  children: mesh.children.map(child => child.position) });
function prepare(game) {
  if (books.has(game)) return books.get(game);
  const d = game.debris, ids = new DebrisIds(), book = { ids, bones: new Map(), factories: [], made: null, found: false, factory: -1, effect: null,
    changed: false, count: d.pieces.length };
  book.bone = b => { if (!b) return null; if (!book.bones.has(b)) book.bones.set(b, book.bones.size); return book.bones.get(b); };
  book.edge = { make(index) { return book.made = book.factories[index](); } };
  const model = d.model;
  d.model = function (name) {
    const make = model.call(this, name); book.found = Boolean(make); book.factory = -1;
    if (!make) return make;
    const index = book.factories.length; book.factories.push(make); book.factory = index;
    return () => book.edge.make(index);
  };
  const play = d.effects.play;
  d.effects.play = function (...args) { return book.effect = play.apply(this, args); };
  for (const method of ['blowApart', 'rest', 'clear']) {
    const base = d[method]; d[method] = function (...args) { book.changed = true; return base.apply(this, args); };
  }
  books.set(game, book); return book;
}
function boundaries(game) {
  const b = prepare(game), d = game.debris;
  return [
    ['debris.model', d, 'model', () => ({ found: b.found, factory: b.factory })],
    ['debris.make', b.edge, 'make', () => ({ mesh: meshState(b.made, b.ids) })],
    ['debris.matrix', THREE.Object3D.prototype, 'updateMatrixWorld', null, (_g, _c, _a, self) => ({ mesh: meshState(self, b.ids) })],
    ['debris.worldMatrix', THREE.Object3D.prototype, 'updateWorldMatrix', (_g, _c, _a, self) => ({ bone: b.bone(self), matrix: self.matrixWorld })],
    ['debris.bounds', THREE.Box3.prototype, 'setFromObject', (_g, _c, _a, self) => ({ low: self.min, high: self.max }),
      (_g, _c, args) => ({ mesh: meshState(args[0], b.ids) })],
    ['debris.add', d.scene, 'add', null, (_g, _c, args) => ({ mesh: meshState(args[0], b.ids) })],
    ['debris.remove', d.scene, 'remove', null, (_g, _c, args) => ({ mesh: meshState(args[0], b.ids) })],
    ['debris.cast', d.collision, 'cast'],
    ['debris.play', d.effects, 'play', () => ({ effect: b.ids.key('effect', b.effect) })],
    ['debris.move', EffectHandle.prototype, 'move', null, (_g, _c, _a, self) => ({ effect: b.ids.key('effect', self) })],
    ['debris.stop', EffectHandle.prototype, 'stop', null, (_g, _c, _a, self) => ({ effect: b.ids.key('effect', self) })],
  ];
}
const METHODS = ['blowApart', 'update', 'bounce', 'rest', 'clear'];
export const debris = { frames: METHODS.map(method => ({ method: `Debris.${method}`, target: game => game.debris, boundaries,
  state(game, args) {
    const b = prepare(game), d = game.debris;
    let spawn = null;
    if (method === 'blowApart') {
      const byName = new Map(Object.entries(args[1]).map(([name, bone]) => [name.toLowerCase(), bone]));
      const { centre = false, floor = -Infinity } = args[6] ?? {};
      spawn = { chunks: args[0].map(chunk => ({ chunk: debrisChunk(chunk), name: chunk.ChunkGeometryName,
        named: Boolean(chunk.ChunkGeometryName), bone: b.bone(byName.get(String(chunk.ChunkNodeName || '').toLowerCase())) })),
        origin: args[2], velocity: args[3], health: args[4], ignore: b.ids.key('body', args[5] ?? null), center: centre, floor };
    }
    const state = debrisState(d, b.ids);
    return { state, spawn, bodies: b.ids.bodies, effects: b.ids.effects,
      meshes: d.pieces.map(p => meshState(p.mesh, b.ids)),
      dt: method === 'update' ? args[0] : 0, id: ['bounce', 'rest'].includes(method) ? b.ids.key('mesh', args[0].mesh) : 0,
      floor: method === 'rest' && args[1] !== undefined ? args[1] : -Infinity };
  },
})), events(game, note) {
  const b = prepare(game), count = game.debris.pieces.length;
  if (b.changed || count !== b.count) note('debris', count);
  b.changed = false; b.count = count;
} };
