// Synthetic tracks and command sequences for presentation output goldens.
// These records contain no extracted assets.
import * as T from "three";
import { emitter, library, seeded } from "./effects.js";
import { Header as H, Effect as E } from "../../src/presentation/layout.js";
function clocks(fx) {
  const out = [];
  for (let i = 0; i < fx.header[H.EFFECTS]; i++) {
    const at = i * E.STRIDE;
    if (!fx.effectOutput[at + E.DONE]) out.push(fx.effectOutput[at + E.TIME]);
  }
  return out;
}
export function batches(fx) {
  return [...fx.renderer.batches.values()]
    .filter((b) => b.count)
    .map((b) => ({
      count: b.count,
      blend: (b.variant & 1 ? T.AdditiveBlending : T.NormalBlending),
      arrays: Object.fromEntries(
        Object.entries(b.arrays).map(([name, a]) => [
          name,
          Array.from(a.slice(0, b.count * (a.length / b.capacity))),
        ]),
      ),
    }));
}
export function emissionScenario(make, gpu, readClocks = clocks) {
  const defs = library();
  defs.circle = [emitter({ ...emitter(), spawner: { ...emitter().spawner, shape: "circle" } })];
  defs.child = [emitter({ geometry: { type: "emitter", blend: "normal", emitters: [emitter()] } })];
  defs.long = [
    emitter({
      transformer: {
        ...emitter().transformer,
        position: Array.from({ length: 9 }, () => ({ lifetime: 0.1, accelerate: [0, 1, 0] })),
      },
    }),
  ];
  const fx = make(defs, gpu),
    handles = seeded(() => Object.keys(defs).map((n) => fx.play(n, new T.Vector3()))),
    rows = [];
  for (let frame = 0; frame < 260; frame++) {
    for (const h of handles) {
      h.move(new T.Vector3(frame * 0.01, 0, 0));
      h.tint = 0.7;
      h.scale = 1.2;
      h.inherit = 0.4;
    }
    if (frame === 20) handles[0].stop();
    if (frame === 30) handles[1].kill();
    fx.update([1 / 30, 1 / 144, 0, 0.03][frame % 4]);
    rows.push([batches(fx), fx.particleCount, readClocks(fx)]);
  }
  fx.kernel?.free();
  return rows;
}
export function continuousScenario(make, readClocks = clocks) {
  const child = emitter({ maxParticles: [4, 4], transformer: { ...emitter().transformer, lifetime: 0.05 } });
  const defs = {
    trail: [
      emitter({
        maxParticles: [-1, -1],
        burstDelay: [0.03, 0.03],
        burstCount: [1, 3],
        transformer: { ...emitter().transformer, lifetime: 0.2 },
        geometry: { type: "emitter", blend: "normal", emitters: [child] },
      }),
    ],
  };
  const fx = make(defs, true),
    handle = seeded(() => fx.play("trail", new T.Vector3())),
    rows = [];
  for (let i = 0; i < 180; i++) {
    handle.move(new T.Vector3(i * 0.01, 0, 0));
    fx.update(1 / 60);
    rows.push([batches(fx), fx.particleCount, readClocks(fx)]);
  }
  handle.kill();
  fx.update(0);
  rows.push([batches(fx), fx.particleCount, readClocks(fx)]);
  fx.kernel?.free();
  return rows;
}
export function modelScenario(make) {
  const defs = { model: [emitter({ geometry: { type: "geometry", blend: "additive", model: "fixture" } })] };
  const model = () => () => {
    const mesh = new T.Group();
    mesh.userData.material = { color: new T.Color(), opacity: 1 };
    return mesh;
  };
  const fx = make(defs, true, model),
    handle = seeded(() => fx.play("model", new T.Vector3(1, 2, 3))),
    rows = [];
  for (let i = 0; i < 20; i++) {
    handle.attachNozzles([{ position: new T.Vector3(2, 3, 4), direction: new T.Vector3(0, 0, 1) }]);
    handle.tint = 0.4;
    handle.scale = 1.3;
    fx.update(1 / 60);
    rows.push(
      [...fx.meshes.values()].flatMap((p) =>
        p.list
          .slice(0, p.used)
          .map((m) => ({
            position: m.position.toArray(),
            q: m.quaternion.toArray(),
            scale: m.scale.toArray(),
            color: m.userData.material.color.toArray(),
            alpha: m.userData.material.opacity,
          })),
      ),
    );
  }
  fx.kernel?.free();
  return rows;
}
export function prewarmScenario(make, readClocks = clocks) {
  const defs = { a: [emitter({ maxParticles: [-1, -1], burstDelay: [0.2, 0.2] })] },
    fx = make(defs, true),
    h = seeded(() => fx.play("a", new T.Vector3())),
    rows = [];
  fx.step(h, 2);
  for (let i = 0; i < 12; i++) {
    fx.update(i ? 1 / 60 : 0);
    rows.push([batches(fx), fx.particleCount, readClocks(fx)]);
  }
  fx.kernel?.free();
  return rows;
}
export function eventDefinitions() {
  const defs = library();
  defs.child = [
    emitter({
      geometry: {
        type: "emitter",
        blend: "normal",
        emitters: [emitter({ transformer: { ...emitter().transformer, lifetime: 0.2 } })],
      },
    }),
  ];
  return defs;
}
export function eventScenario(fx, read) {
  const handles = seeded(() => Object.keys(fx.library).map((name) => fx.play(name, new T.Vector3()))),
    rows = [];
  for (let frame = 0; frame < 250; frame++) {
    if (frame === 3) handles[0].kill();
    if (frame === 10) handles[1].stop();
    fx.update(1 / 60);
    rows.push(read(fx));
  }
  fx.kernel?.free();
  return rows;
}
