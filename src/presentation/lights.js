import {publishPointLight} from "../rust-render/point-inputs.js";
import {sceneTransform} from "../rust-render/scene-changes.js";
import * as THREE from "three";
import { LightFrame as F, LightCommand as C, LightOutput as O, LightHeader as H,
  LightHandle as T, LightLimits as L, LightKind as K } from "./page-layout.js";

// Scene resources are fixed at construction. Requests cross as scalar rows;
// Rust owns all ranking, timed state and the output order for the frame.
export class LightPool {
  constructor(scene, count = 8, loaded) {
    this.lights = Array.from({ length: Math.min(count, L.LIGHTS) }, () => {
      const light = new THREE.PointLight(0x000000, 0, 1, 1);
      scene.add(light);
      return light;
    });
    // All native stores are reserved before any producer can enqueue a
    // light. PointLight remains the renderer's required resource shell.
    this.kernel = new loaded.module.PageLights(this.lights.length);
    this.memory = loaded.memory;
    const k = this.kernel;
    this.addresses = [k.input_ptr(), k.commands_ptr(), k.output_ptr(), k.handles_ptr(), k.header_ptr()];
    this.start = 0; this.requestCount = 0; this.serial = 0; this.dropped = 0; this.flushed = 0;
    this.refresh();
  }
  // Other presentation owners share this module's memory. Cold asset
  // registration can grow it; every command reacquires borrowed views.
  refresh() {
    if (this.buffer === this.memory.buffer) return;
    this.buffer = this.memory.buffer;
    const a = this.addresses, b = this.buffer;
    this.input = new Float64Array(b, a[0], F.STRIDE);
    this.commands = new Float64Array(b, a[1], L.COMMANDS * C.STRIDE);
    this.output = new Float64Array(b, a[2], L.LIGHTS * O.STRIDE);
    this.handles = new Float64Array(b, a[3], L.TIMED * T.STRIDE);
    this.header = new Float64Array(b, a[4], H.STRIDE);
  }
  // The ring holds substantially more commands than a map can produce in
  // one frame. During an arbitrary pause excess replaces the oldest row.
  command(kind, position, rgb, radius, scale, duration, serial) {
    this.refresh();
    let slot = (this.start + this.requestCount) % L.COMMANDS;
    if (this.requestCount === L.COMMANDS) {
      this.start = (this.start + 1) % L.COMMANDS;
      this.dropped++;
    } else this.requestCount++;
    // Copy scratch positions and colours now: producers reuse both before
    // the frame closes. Neither object is retained as a second state owner.
    const a = this.commands, at = slot * C.STRIDE;
    a[at + C.KIND] = kind; a[at + C.SERIAL] = serial;
    a[at + C.POSITION] = position.x; a[at + C.POSITION + 1] = position.y; a[at + C.POSITION + 2] = position.z;
    a[at + C.RGB] = rgb[0]; a[at + C.RGB + 1] = rgb[1]; a[at + C.RGB + 2] = rgb[2];
    a[at + C.RADIUS] = radius; a[at + C.SCALE] = scale; a[at + C.DURATION] = duration;
  }
  add(position, rgb, radius, scale = 1) {
    if (!rgb || radius <= 0) return;
    this.command(K.ADD, position, rgb, radius, scale, 0, 0);
  }
  // A returned serial is a resource token only. Lifetime advancement and
  // fade scale are read from native records after the one frame call.
  flash(position, rgb, radius, duration) {
    if (radius <= 0 || duration <= 0) return null;
    const serial = ++this.serial;
    this.command(K.FLASH, position, rgb, radius, 1, duration, serial);
    return serial;
  }
  // Combat's muzzle host only asks whether its flash has begun fading.
  // Qualified serials survive packed compaction and reject reused slots.
  pending(serial) {
    this.refresh();
    if (!(serial > 0)) return false;
    // The host watermark also completes evicted commands. Native handles
    // still qualify existence and lifetime by their serial after flushing.
    if (serial > this.flushed) return true;
    if (serial > this.header[H.COMPLETED]) return false;
    const at = (serial % L.TIMED) * T.STRIDE, a = this.handles;
    return a[at + T.SERIAL] === serial && a[at + T.LEFT] === a[at + T.DURATION];
  }
  update(dt) {
    this.refresh();
    const a = this.input;
    a[F.DT] = dt; a[F.COMMANDS] = this.requestCount; a[F.START] = this.start; a[F.DROPPED] = this.dropped;
    this.kernel.frame();
    this.flushed = this.serial;
    // Scene publication never ranks or fades a light. Its fixed resource
    // slots receive the complete result, including zero-intensity slots.
    const out = this.output;
    for (let i = 0; i < this.lights.length; i++) {
      const light = this.lights[i], at = i * O.STRIDE;
      sceneTransform(light.position.set(out[at + O.POSITION], out[at + O.POSITION + 1], out[at + O.POSITION + 2]));
      light.color.setRGB(out[at + O.RGB], out[at + O.RGB + 1], out[at + O.RGB + 2]);
      light.intensity = out[at + O.SCALE]; light.distance = out[at + O.RADIUS];publishPointLight(light);
    }
    // Clearing the queue cursors leaves every byte of storage reusable.
    // The live timed bank is retained entirely on the native side.
    this.requestCount = 0; this.start = 0;
  }
}
