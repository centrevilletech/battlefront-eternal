import * as THREE from 'three';
import { FLIGHT_CLASSES } from './config.js';
// Created only on the presentation host. The same panel is useful in test scenes.
export class FlightDebug {
  constructor(scene, parent = document.body) {
    this.panel = document.createElement('details'); this.panel.style.cssText = 'position:fixed;top:40px;left:8px;background:#08131eee;color:#bdf;padding:10px;z-index:10000;max-height:85vh;overflow:auto;font:12px monospace';
    this.panel.innerHTML = '<summary>Flight debug / live tuning</summary>';
    this.readout = document.createElement('pre'); this.panel.append(this.readout);
    for (const [name, config] of Object.entries(FLIGHT_CLASSES)) {
      const group = document.createElement('details'); group.innerHTML = `<summary>${name}</summary>`;
      for (const key of Object.keys(config)) {
        const label = document.createElement('label'); label.style.display = 'block'; label.textContent = `${key} `;
        const input = document.createElement('input'); input.type = 'number'; input.step = '.05'; input.value = config[key]; input.style.width = '75px';
        input.onchange = () => { const value = Number(input.value); if (Number.isFinite(value) && value > 0) config[key] = value; };
        label.append(input); group.append(label);
      }
      this.panel.append(group);
    }
    parent.append(this.panel); this.scene = scene;
    this.axes = new THREE.AxesHelper(12); scene.add(this.axes);
    this.lines = new THREE.LineSegments(new THREE.BufferGeometry(), new THREE.LineBasicMaterial({ color: 0x00ffff, depthTest: false })); scene.add(this.lines);
  }
  update(v, cameraTarget) {
    this.axes.visible = this.lines.visible = Boolean(v?.flying); if (!v?.flying) return;
    this.axes.position.copy(v.root.position); this.axes.quaternion.copy(v.root.quaternion);
    this.readout.textContent = JSON.stringify({ state: v.flightState, speed: v.airspeed, rates: [v.tiltRate, v.yawRate, v.flightRollRate], stick: [v.stickX, v.stickY], bank: v.bank, rollLock: v.rollLock, power: v.power, lock: v.flightLockState, timer: v.flightLockTimer, landing: v.landingReason }, null, 2);
    const p = v.root.position, q = v.root.quaternion, pts = [];
    const line = (a, b) => pts.push(...a.toArray(), ...b.toArray());
    line(p, p.clone().addScaledVector(v.velocity, 1.5));
    line(p, p.clone().add(new THREE.Vector3(0, -v.flightConfig.landingHeight, 0)));
    if (cameraTarget) line(p, cameraTarget);
    const radius = Math.tan(v.flightConfig.lockCone * Math.PI / 180) * 60;
    for (let i = 0; i < 16; i++) line(p, new THREE.Vector3(Math.cos(i*Math.PI/8)*radius, Math.sin(i*Math.PI/8)*radius, 60).applyQuaternion(q).add(p));
    this.lines.geometry.dispose(); this.lines.geometry = new THREE.BufferGeometry(); this.lines.geometry.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
  }
  dispose() { this.panel.remove(); this.scene.remove(this.axes, this.lines); this.axes.dispose(); this.lines.geometry.dispose(); this.lines.material.dispose(); }
}
