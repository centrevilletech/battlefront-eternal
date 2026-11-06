import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import * as T from "three";
import init, { PagePresentation } from "../src/presentation-pkg/bfe_presentation.js";
import { PageEffects } from "../src/presentation/effects.js";
import { emitter, library } from "./fixtures/effects.js";
import {
  emissionScenario,
  continuousScenario,
  modelScenario,
  prewarmScenario,
  eventDefinitions,
  eventScenario,
} from "./fixtures/effect-scenarios.js";
import {
  VERSION,
  Frame as F,
  Command as C,
  Header as H,
  Effect as E,
  Event as Ev,
} from "../src/presentation/layout.js";
import { PresentationCapture } from "../src/presentation/capture.js";
const wasm = await init({
  module_or_path: fs.readFileSync(new URL("../src/presentation-pkg/bfe_presentation_bg.wasm", import.meta.url)),
});
const loaded = { module: { PagePresentation }, memory: wasm.memory },
  texture = new T.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
function make(defs, gpu = true, model) {
  return new PageEffects(new T.Scene(), defs, { texture: () => texture, gpu, model }, loaded);
}
const goldens = JSON.parse(fs.readFileSync(new URL("./fixtures/presentation/effect-goldens.json", import.meta.url)));
function check(name, rows) {
  assert.equal(rows.length, goldens[name].frames);
  assert.equal(
    createHash("sha256").update(JSON.stringify(rows)).digest("hex"),
    goldens[name].sha256,
    name + " output golden",
  );
}
for (const gpu of [true, false])
  test(`native effects preserve synthetic particle, draw, clock and lifecycle goldens (GPU ${gpu})`, () =>
    check("emission-" + gpu, emissionScenario(make, gpu)));
test("native nested births and pool reuse match synthetic goldens", () =>
  check("continuous", continuousScenario(make)));
test("native nozzle and geometry records match synthetic goldens", () => check("model", modelScenario(make)));

test("pooled model meshes retain the blend of the birth that first uses each slot", () => {
  const blends = [];
  const definition = blend => [emitter({ geometry: { type: "geometry", model: "fixture", blend } })];
  const fx = make({ normal: definition("normal"), additive: definition("additive") }, false, () => additive => {
    blends.push(additive);
    const mesh = new T.Mesh(new T.BufferGeometry(), new T.MeshBasicMaterial());
    mesh.userData.material = mesh.material;
    return mesh;
  });
  // Library order differs from birth order. Existing entries retain their
  // material, and newly reached pool slots use their own first birth's blend.
  fx.play("additive", new T.Vector3()); fx.update(0);
  assert.deepEqual(blends, [true, true, true, true]);
  fx.play("normal", new T.Vector3()); fx.update(0);
  assert.deepEqual(blends, [true, true, true, true, false, false, false, false]);
  fx.kernel.free();
});
test("native map-source warmup matches synthetic goldens", () => check("prewarm", prewarmScenario(make)));
test("native ordered birth and death events match frozen synthetic goldens", () =>
  check(
    "events",
    eventScenario(make(eventDefinitions()), (fx) => {
      const rows = [];
      for (let i = 0; i < fx.header[H.EVENTS]; i++)
        rows.push(Array.from(fx.eventOutput.subarray(i * Ev.STRIDE, (i + 1) * Ev.STRIDE)));
      return rows;
    }),
  ));

test("presentation captures replay exact native inputs and outputs without growing frame buffers", () => {
  const fx = make(library());
  fx.capture = new PresentationCapture(100000);
  const effect = fx.play("particle", new T.Vector3(), null, null, null);
  const buffer = fx.input.buffer;
  for (let i = 0; i < 20; i++) {
    effect.move(new T.Vector3(i * 0.1, 0, 0));
    fx.update(1 / 60);
    assert.equal(fx.input.buffer, buffer);
  }
  const recording = fx.capture.export();
  if (process.env.BFE_PRESENTATION_CAPTURE_DIR) {
    fs.writeFileSync(process.env.BFE_PRESENTATION_CAPTURE_DIR + "/synthetic.json", JSON.stringify(fx.content.data));
    fs.writeFileSync(process.env.BFE_PRESENTATION_CAPTURE_DIR + "/synthetic.f64", new Uint8Array(recording.buffer));
  }
  const k = new PagePresentation(JSON.stringify(fx.content.data));
  const input = new Float64Array(wasm.memory.buffer, k.input_ptr(), k.input_len());
  const header = new Float64Array(wasm.memory.buffer, k.header_ptr(), H.STRIDE);
  const effects = new Float64Array(wasm.memory.buffer, k.effects_ptr(), k.effects_len());
  const draws = new Float64Array(wasm.memory.buffer, k.draws_ptr(), k.draws_len());
  const events = new Float64Array(wasm.memory.buffer, k.events_ptr(), k.events_len());
  let at = 0,
    frames = 0;
  while (at < recording.length) {
    assert.equal(recording[at++], VERSION);
    const a = recording[at++],
      b = recording[at++],
      c = recording[at++],
      d = recording[at++];
    input.set(recording.subarray(at, at + a));
    at += a;
    k.present_frame();
    for (const [data, n] of [
      [header, H.STRIDE],
      [effects, b],
      [draws, c],
      [events, d],
    ]) {
      assert.deepEqual(Array.from(data.subarray(0, n)), Array.from(recording.subarray(at, at + n)));
      at += n;
    }
    frames++;
  }
  assert.equal(frames, 20);
  k.free();
  fx.kernel.free();
});

test("quiet frames publish no repeated commands and frozen/non-presented replay does not advance time", () => {
  const fx = make(library());
  fx.play("particle", new T.Vector3());
  fx.update(0);
  assert.equal(fx.input[F.COMMANDS], 1);
  fx.update(0);
  assert.equal(fx.input[F.COMMANDS], 0);
  const time = fx.effectOutput[E.TIME];
  fx.input[F.FRAME]++;
  fx.input[F.DT] = 1;
  fx.input[F.FROZEN] = 1;
  fx.kernel.present_frame();
  assert.equal(fx.header[H.DRAWS], 0);
  fx.input[F.FRAME]++;
  fx.input[F.FROZEN] = 0;
  fx.input[F.PRESENT] = 0;
  fx.kernel.present_frame();
  assert.equal(fx.header[H.DRAWS], 0);
  fx.input[F.FRAME]++;
  fx.input[F.PRESENT] = 1;
  fx.input[F.DT] = 0;
  fx.kernel.present_frame();
  assert.equal(fx.effectOutput[E.TIME], time);
  fx.kernel.free();
});

test("retired generations cannot create duplicate births, and the phase never fails", () => {
  const fx = make(library());
  fx.play("particle", new T.Vector3());
  fx.update(4);
  assert.equal(fx.effects.length, 0);
  const skipped = fx.header[H.SKIPPED_COMMANDS];
  fx.input[F.FRAME]++;
  fx.input[F.COMMANDS] = 1;
  fx.input[F.DT] = 0;
  // Replay the retired spawn with a new command sequence but the old generation.
  fx.input[F.STRIDE + C.SEQUENCE] = 2;
  fx.kernel.present_frame();
  assert.equal(fx.header[H.STATUS], 0);
  assert.equal(fx.header[H.EFFECTS], 0, "no duplicate birth");
  assert.equal(fx.header[H.SKIPPED_COMMANDS], skipped + 1);
  fx.input[F.FRAME]++;
  fx.input[F.COMMANDS] = 0;
  fx.kernel.present_frame();
  assert.equal(fx.header[H.STATUS], 0);
  fx.kernel.free();
});

test("a long stall past every page store refuses births, defers commands and never fails a frame", () => {
  // One effect with many emitters makes many nozzle commands per birth.
  const defs = { wide: Array.from({ length: 12 }, () => emitter()) };
  const fx = make(defs);
  const warn = console.warn,
    warnings = [];
  console.warn = (...args) => warnings.push(args.join(" "));
  try {
    const ids = fx.kernel.handles_len() - 1,
      frames = [{ position: new T.Vector3(1, 0, 0), direction: new T.Vector3(0, 1, 0) }];
    const handles = [];
    // Births and nozzle changes keep arriving while no frame is drawn.
    for (let i = 0; i < ids + 50; i++) {
      const h = fx.play("wide", new T.Vector3(i, 0, 0));
      h.move(new T.Vector3(i, 1, 0));
      h.attachNozzles(frames);
      fx.step(h, 0.5);
      handles.push(h);
    }
    const refused = handles.filter((h) => h.id === 0);
    assert.equal(refused.length, 50, "handles past the id table are refused");
    assert.ok(refused.every((h) => h.done), "a refused handle is already finished");
    refused[0].kill();
    refused[0].move(new T.Vector3(9, 9, 9));
    let sent = 0;
    for (let frame = 0; frame < 40; frame++) {
      fx.update(1 / 60);
      assert.equal(fx.header[H.STATUS], 0);
      assert.ok(fx.input[F.COMMANDS] <= (fx.input.length - F.STRIDE) / C.STRIDE);
      sent += fx.input[F.COMMANDS];
      if (!fx.effects.some((h) => h.fresh || h.dirty || h.nozzleDirty || h.prewarm)) break;
    }
    assert.ok(!fx.effects.some((h) => h.fresh), "deferred births all reach the kernel");
    assert.ok(sent > (fx.input.length - F.STRIDE) / C.STRIDE, "commands spanned several frames");
    // Kill everything, then reuse the freed ids: a stale handle never
    // reaches the effect that now holds its id.
    for (const h of fx.effects) h.kill();
    fx.update(1 / 60);
    fx.update(1 / 60);
    assert.equal(fx.effects.length, 0);
    const fresh = fx.play("wide", new T.Vector3()),
      stale = handles.find((h) => h.id === fresh.id);
    assert.ok(stale, "the new effect reuses a retired id");
    assert.notEqual(fresh.generation, stale.generation);
    stale.move(new T.Vector3(50, 50, 50));
    fx.update(1 / 60);
    assert.equal(fresh.done, false);
    assert.equal(warnings.length, 1, "refusals are reported once");
  } finally {
    console.warn = warn;
    fx.kernel.free();
  }
});

test("a full presentation capture stops permanently rather than leaving replay gaps", () => {
  const capture = new PresentationCapture(50),
    input = new Float64Array(100),
    header = new Float64Array(H.STRIDE),
    empty = new Float64Array(0);
  input[F.COMMANDS] = 2;
  capture.record(input, header, empty, empty, empty);
  assert.equal(capture.full, true);
  input[F.COMMANDS] = 0;
  capture.record(input, header, empty, empty, empty);
  assert.equal(capture.frames, 0);
  assert.equal(capture.used, 0);
});

test("load-time recording includes initial births and refuses incomplete late captures", () => {
  const fx = new PageEffects(
    new T.Scene(),
    library(),
    { texture: () => texture, gpu: true, record: true, captureCapacity: 10000 },
    loaded,
  );
  fx.play("particle", new T.Vector3());
  fx.update(0);
  const recording = fx.endCapture();
  assert.equal(recording.frames, 1);
  assert.equal(recording.full, false);
  assert.equal(recording.records[5 + F.COMMANDS], 1);
  assert.throws(() => fx.beginCapture(), /before the first frame/);
  fx.kernel.free();
});
