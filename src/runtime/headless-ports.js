import * as THREE from 'three';
import { CommandControls } from './commands.js';

// Explicit presentation ports for replay/worker construction. No DOM, canvas,
// WebGL context, texture decoding, or audio device is created here.
export class HeadlessGraphics {
  constructor(settings) {
    this.settings = settings; this.scene = new THREE.Scene(); this.viewScene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(60, 16/9, .5, 200);
    this.renderer = null; this.localLights = {}; this.casters = new Set(); this.hiddenLayer = 1;
  }
  setMap() {}
  caster() {}
}
export class HeadlessHud {
  preload() {} announce() {} damageFrom() {} enemyHealth() {} hit() {}
  killMessage() {} message() {} noteShot() {} render() {} setReticleTarget() {}
  setUnits() {} squadCommand() {} stats() {} toggleMap() {} update() {} wake() {}
  setPlayer() {} setReticle() {} setScope() {} setSecondBar() {} setHull() {} setVehicle() {} setLeaving() {}
  text(key) { return key; }
}
export class HeadlessAudio {
  async load() {} attach() {} retarget() {} on() {} update() {} setPaused() {} playClass() { return null; }
}
export class HeadlessControls extends CommandControls {
  constructor() { super(); this.sequence = 0; this.onClear = null; this.human = []; }
  onLockChange() {}
  release() {}
  command(tick, epoch, generation) {
    const command = { tick, epoch, generation, sequence: ++this.sequence, down: [...this.down], actions: [...this.queued], look: [] };
    // The spawn screen's choices, in order (src/human-spawn.js).
    if (this.human.length) command.human = this.human.splice(0);
    this.queued.clear(); return command;
  }
  clear() { super.clear(); this.onClear?.(); }
}
