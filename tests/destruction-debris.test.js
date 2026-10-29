import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { DebrisPieces, heatMaterial, pieceShape, PieceSolver, RigidPiece } from '../src/destruction-debris.js';
import { zeroMaterial } from '../src/materials.js';

// Half-spaces ({ normal, point }) as a collision world: cast() finds the
// nearest plane a ray enters, the way CollisionWorld.cast reports it.
function planes(list) {
  return {
    casts: 0,
    cast(origin, direction, far, out) {
      this.casts++;
      let best = null;
      for (const plane of list) {
        const into = direction.dot(plane.normal);
        if (into >= -1e-9) continue;
        const t = plane.normal.dot(new THREE.Vector3().subVectors(plane.point, origin)) / into;
        if (t < -1e-6 || t > far) continue;
        if (!best || t < best.t) best = { t, plane };
      }
      if (!best) return null;
      out.distance = best.t;
      out.point.copy(origin).addScaledVector(direction, best.t);
      out.normal.copy(best.plane.normal);
      return out;
    },
  };
}
const FLOOR = { normal: new THREE.Vector3(0, 1, 0), point: new THREE.Vector3() };

function boxShape(w, h, d) {
  const group = new THREE.Group();
  group.add(new THREE.Mesh(new THREE.BoxGeometry(w, h, d)));
  return pieceShape(group);
}

function seeded(seed) {
  return () => ((seed = Math.imul(seed, 1664525) + 1013904223 >>> 0) / 4294967296);
}

// The lowest point of a piece's hull along a plane's normal (negative: in it).
function clearance(piece, plane) {
  let least = Infinity;
  for (const p of piece.shape.points) {
    const world = p.clone().applyMatrix3(piece.rotation).add(piece.position);
    least = Math.min(least, plane.normal.dot(world.sub(plane.point)));
  }
  return least;
}

// How nearly one of the piece's own axes points along `axis` (1: exactly).
function aligned(piece, axis) {
  return Math.max(...[new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)]
    .map((a) => Math.abs(a.applyMatrix3(piece.rotation).dot(axis))));
}

function drop(shape, world, random, { speed = 6, spin = 10, height = 1.2 } = {}) {
  const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(random() * 6, random() * 6, random() * 6));
  const v = new THREE.Vector3(random() - 0.5, 0.6 + random(), random() - 0.5).normalize().multiplyScalar(speed);
  const w = new THREE.Vector3(random() - 0.5, random() - 0.5, random() - 0.5).normalize().multiplyScalar(spin * random());
  return new RigidPiece().reset(shape, new THREE.Vector3(0, height, 0), q, v, w);
}

test('a piece shape is its box centre, half size and extreme vertices', () => {
  const group = new THREE.Group();
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.2, 0.4));
  mesh.position.set(0.3, 0, 0);
  group.add(mesh);
  const shape = pieceShape(group);
  assert.ok(shape.center.distanceTo(new THREE.Vector3(0.3, 0, 0)) < 1e-6);
  assert.ok(shape.half.distanceTo(new THREE.Vector3(0.3, 0.1, 0.2)) < 1e-6);
  assert.equal(shape.points.length, 8, 'a box touches with its corners');
  for (const p of shape.points) assert.ok(Math.abs(Math.abs(p.x) - 0.3) < 1e-6 && Math.abs(Math.abs(p.y) - 0.1) < 1e-6);
  assert.ok(Math.abs(shape.radius - Math.hypot(0.3, 0.1, 0.2)) < 1e-6);
  // A 1 kg box: I = (b^2 + c^2) / 3 for half sizes b, c.
  assert.ok(Math.abs(1 / shape.inverseInertia.x - (0.01 + 0.04) / 3) < 1e-6);
});

test('thrown pieces tumble, settle lying flat on a face and fall asleep without sinking', () => {
  const random = seeded(17);
  for (const [w, h, d] of [[0.6, 0.12, 0.35], [1, 0.12, 0.12], [0.35, 0.35, 0.35], [0.7, 0.25, 0.5]]) {
    for (let k = 0; k < 4; k++) {
      const world = planes([FLOOR]);
      const solver = new PieceSolver(world);
      const piece = drop(boxShape(w, h, d), world, random);
      let deepest = Infinity, slept = null;
      for (let t = 0; t < 6; t += 1 / 60) {
        solver.step([piece], 1 / 60);
        deepest = Math.min(deepest, clearance(piece, FLOOR));
        if (piece.sleeping && slept === null) slept = t;
      }
      assert.ok(deepest > -0.01, `${w}x${h}x${d} sank ${deepest} m into the floor`);
      assert.ok(slept !== null && slept < 4, `${w}x${h}x${d} still moving after 4 s`);
      assert.ok(aligned(piece, FLOOR.normal) > 0.99, `${w}x${h}x${d} came to rest on an edge or corner`);
      assert.ok(Math.abs(clearance(piece, FLOOR)) < 0.01, 'resting on the floor, not above it');
    }
  }
});

test('a rod landing on its end falls over rather than standing upright', () => {
  // Tipped past its base, and balanced exactly on it (which would stand).
  for (const tilt of [0.2, 0.03, 0]) {
    const world = planes([FLOOR]);
    const solver = new PieceSolver(world);
    const rod = boxShape(0.12, 1, 0.12);
    const turn = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), tilt);
    const piece = new RigidPiece().reset(rod, new THREE.Vector3(0, 0.52, 0), turn, new THREE.Vector3(), new THREE.Vector3());
    for (let t = 0; t < 6; t += 1 / 60) solver.step([piece], 1 / 60);
    const long = new THREE.Vector3(0, 1, 0).applyMatrix3(piece.rotation);
    assert.ok(Math.abs(long.y) < 0.1, `tilted ${tilt}: the rod's long axis should lie flat, y = ${long.y}`);
    assert.ok(piece.position.y < 0.1, 'lying down');
    assert.ok(piece.sleeping);
  }
});

test('pieces bounce off walls instead of passing through, at any frame rate', () => {
  const wall = { normal: new THREE.Vector3(-1, 0, 0), point: new THREE.Vector3(1, 0, 0) };
  for (const dt of [1 / 30, 1 / 60, 1 / 144]) {
    const world = planes([FLOOR, wall]);
    const solver = new PieceSolver(world);
    const shape = boxShape(0.5, 0.15, 0.3);
    const piece = new RigidPiece().reset(shape, new THREE.Vector3(0, 0.8, 0), new THREE.Quaternion(), new THREE.Vector3(18, 2, 0), new THREE.Vector3(0, 8, 3));
    let least = Infinity;
    for (let t = 0; t < 4; t += dt) {
      solver.step([piece], dt);
      least = Math.min(least, clearance(piece, wall));
    }
    assert.ok(least > -0.02, `at ${Math.round(1 / dt)} fps it went ${-least} m into the wall`);
    assert.ok(piece.position.x < 1, 'it came back off the wall');
  }
});

test('friction holds a piece on a gentle slope and lets it go on a steep one', () => {
  const random = seeded(5);
  for (const [degrees, holds] of [[15, true], [45, false]]) {
    const a = (degrees * Math.PI) / 180;
    const slope = { normal: new THREE.Vector3(Math.sin(a), Math.cos(a), 0), point: new THREE.Vector3() };
    const world = planes([slope]);
    const solver = new PieceSolver(world);
    const piece = drop(boxShape(0.6, 0.15, 0.35), world, random, { speed: 1, spin: 2, height: 0.6 });
    for (let t = 0; t < 4; t += 1 / 60) solver.step([piece], 1 / 60);
    assert.ok(clearance(piece, slope) > -0.01);
    if (holds) assert.ok(piece.sleeping && piece.position.length() < 2, `slid ${piece.position.length()} m down ${degrees} degrees`);
    else assert.ok(piece.position.x > 3, `held on ${degrees} degrees`);
  }
});

test('a pile of pieces separates, settles and sleeps', () => {
  for (let trial = 1; trial <= 4; trial++) {
    const random = seeded(trial * 977);
    const world = planes([FLOOR]);
    const solver = new PieceSolver(world);
    const shapes = [[0.6, 0.15, 0.35], [1, 0.12, 0.12], [0.35, 0.35, 0.35], [0.7, 0.25, 0.5], [0.4, 0.1, 0.4], [0.5, 0.2, 0.2]].map((s) => boxShape(...s));
    const pieces = shapes.map((shape, i) => new RigidPiece().reset(shape,
      new THREE.Vector3((random() - 0.5) * 0.4, 0.6 + i * 0.35, (random() - 0.5) * 0.4),
      new THREE.Quaternion().setFromEuler(new THREE.Euler(random() * 6, random() * 6, random() * 6)),
      new THREE.Vector3(random() - 0.5, 0, random() - 0.5), new THREE.Vector3(random() * 4, random() * 4, random() * 4)));
    for (const piece of pieces) piece.flight = 1;
    for (let t = 0; t < 10; t += 1 / 60) solver.step(pieces, 1 / 60);
    assert.ok(pieces.every((p) => p.sleeping), `trial ${trial}: still moving after 10 s`);
    for (const p of pieces) assert.ok(clearance(p, FLOOR) > -0.01);
    // No corner deep inside another piece.
    let worst = 0;
    for (const a of pieces) for (const b of pieces) {
      if (a === b) continue;
      const inverse = b.rotation.clone().transpose();
      for (const point of a.shape.points) {
        const local = point.clone().applyMatrix3(a.rotation).add(a.position).sub(b.position).applyMatrix3(inverse);
        const h = b.shape.half;
        worst = Math.max(worst, Math.min(h.x - Math.abs(local.x), h.y - Math.abs(local.y), h.z - Math.abs(local.z)));
      }
    }
    assert.ok(worst < 0.06, `trial ${trial}: a corner ${worst} m inside another piece`);
  }
});

test('a sleeping piece costs no rays', () => {
  const world = planes([FLOOR]);
  const solver = new PieceSolver(world);
  const piece = drop(boxShape(0.4, 0.2, 0.3), world, seeded(3));
  for (let t = 0; t < 5; t += 1 / 60) solver.step([piece], 1 / 60);
  assert.ok(piece.sleeping);
  const before = world.casts;
  for (let i = 0; i < 60; i++) solver.step([piece], 1 / 60);
  assert.equal(world.casts, before);
});

function model() {
  const material = zeroMaterial('dynamic', {});
  return () => {
    const group = new THREE.Group();
    group.add(new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.15, 0.3), material));
    return group;
  };
}

test('heated pieces keep the stock material and add heat, charring and crumbling', () => {
  const source = zeroMaterial('dynamic', {});
  const heated = heatMaterial(source);
  assert.notEqual(heated, source);
  assert.equal(heatMaterial(source), heated, 'made once per material');
  assert.ok(!source.fragmentShader.includes('heated('), 'the stock material is unchanged');
  assert.ok(heated.fragmentShader.includes('color.rgb = heated(color.rgb);'));
  assert.ok(heated.vertexShader.includes('vHeatLocal = transformed;'));
  assert.equal(heated.defines.DYNAMIC, '');
  assert.equal(heated.defines.HEATED, '');
  // The map's light uniforms are shared, not copied.
  assert.equal(heated.uniforms.sunDirection, source.uniforms.sunDirection);
  assert.notEqual(heated.uniforms.sunScale, source.uniforms.sunScale);
});

test('pieces linger, crumble away and go back to the pool; the oldest give way to new ones', () => {
  const scene = new THREE.Scene();
  const pieces = new DebrisPieces({ scene, collision: planes([FLOOR]), model: () => model(), capacity: 3 });
  const at = new THREE.Matrix4().makeTranslation(0, 1, 0);
  const options = { velocity: new THREE.Vector3(1, 2, 0), spin: new THREE.Vector3(3, 1, 2) };
  const first = pieces.spawn('chunk', at, options);
  assert.ok(first && first.object.visible && first.object.parent === scene);
  for (let i = 0; i < 3; i++) pieces.spawn('chunk', at, options);
  assert.equal(pieces.count, 3, 'capped');
  assert.equal(first.gone, true, 'the oldest went first');
  for (let t = 0; t < 40; t += 1 / 30) pieces.update(1 / 30);
  assert.equal(pieces.count, 0, 'all crumbled away');
  assert.ok(pieces.free.get('chunk').length >= 3, 'their models wait for reuse');
  assert.ok(pieces.free.get('chunk').every((object) => !object.visible));
  const again = pieces.spawn('chunk', at, options);
  assert.ok(pieces.free.get('chunk').length >= 2 && again.object.visible, 'a pooled model is reused');
  assert.equal(again.object.userData.heated.dissolve, 0);
});

test('a piece starting through the ground is brought out on the blast side; one lost below the world is let go', () => {
  const pieces = new DebrisPieces({ scene: new THREE.Scene(), collision: planes([FLOOR]), model: () => model() });
  const under = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(0, -0.3, 0), { velocity: new THREE.Vector3(), spin: new THREE.Vector3(), from: new THREE.Vector3(0, 0.9, 0) });
  assert.ok(under.body.position.y >= 0, 'lifted onto the floor');
  for (let t = 0; t < 3; t += 1 / 60) pieces.update(1 / 60);
  assert.ok(under.body.sleeping && clearance(under.body, FLOOR) > -0.01);
  const void_ = new DebrisPieces({ scene: new THREE.Scene(), collision: planes([]), model: () => model() });
  void_.spawn('chunk', new THREE.Matrix4(), { velocity: new THREE.Vector3(), spin: new THREE.Vector3() });
  for (let t = 0; t < 5; t += 1 / 60) void_.update(1 / 60);
  assert.equal(void_.count, 0);
});

test('far pieces and crumbling ones cast no shadow; near ones keep the setting graphics gave them', () => {
  const casting = () => () => {
    const group = model()();
    group.children[0].castShadow = true;
    return group;
  };
  const pieces = new DebrisPieces({ scene: new THREE.Scene(), collision: planes([FLOOR]), model: casting });
  const piece = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(0, 0.5, 0), { velocity: new THREE.Vector3(), spin: new THREE.Vector3() });
  const mesh = piece.object.children[0];
  pieces.update(1 / 60, new THREE.Vector3(5, 2, 5));
  assert.equal(mesh.castShadow, true);
  pieces.update(1 / 60, new THREE.Vector3(80, 2, 5));
  assert.equal(mesh.castShadow, false, 'far off');
  pieces.update(1 / 60, new THREE.Vector3(5, 2, 5));
  assert.equal(mesh.castShadow, true, 'back in reach');
  piece.crumble = 0.01;
  pieces.update(1 / 60, new THREE.Vector3(5, 2, 5));
  assert.equal(mesh.castShadow, false, 'crumbling');
});

test('heat is applied before a piece dissolves into the far scene, so it fades out at range', () => {
  const heated = heatMaterial(zeroMaterial('dynamic', {}));
  const shader = heated.fragmentShader;
  const heat = shader.indexOf('color.rgb = heated(color.rgb);');
  const fade = shader.indexOf('// Linear fog by depth');
  const near = shader.indexOf('float near = clamp((fade.y - vDepth)');
  assert.ok(heat > 0 && heat < fade && heat < near, 'heat, then the fog and the near-to-far dissolve');
});

test('a piece with a frame or motion that is not finite is refused, and one gone bad never spreads to another', () => {
  const pieces = new DebrisPieces({ scene: new THREE.Scene(), collision: planes([FLOOR]), model: () => model() });
  const still = { velocity: new THREE.Vector3(), spin: new THREE.Vector3() };
  assert.equal(pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(NaN, 1, 0), still), null);
  assert.equal(pieces.spawn('chunk', new THREE.Matrix4(), { velocity: new THREE.Vector3(Infinity, 0, 0), spin: new THREE.Vector3() }), null);
  const good = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(0, 0.3, 0), still);
  const bad = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(0.05, 0.3, 0), still);
  good.body.flight = bad.body.flight = 1;
  bad.body.position.set(NaN, NaN, NaN);
  pieces.update(1 / 60);
  assert.ok(Number.isFinite(good.body.position.x + good.body.position.y + good.body.position.z), 'its neighbour stays sound');
  assert.equal(pieces.count, 1, 'the bad one is let go');
});

test('a model reused in the frame it was let go starts its upscaler motion afresh', () => {
  const pieces = new DebrisPieces({ scene: new THREE.Scene(), collision: planes([FLOOR]), model: () => model(), capacity: 1 });
  const still = { velocity: new THREE.Vector3(), spin: new THREE.Vector3() };
  const first = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(0, 1, 0), still);
  const proxy = first.object.children[0].userData.motion;
  assert.ok(proxy, 'its meshes are tracked for the upscaler');
  proxy.userData.seen = 41; // drawn last frame
  const second = pieces.spawn('chunk', new THREE.Matrix4().makeTranslation(3, 1, 0), still);
  assert.equal(second.object, first.object, 'the same model, recycled by the cap');
  assert.equal(proxy.userData.seen, -1, 'no motion carried over from where it was');
});
