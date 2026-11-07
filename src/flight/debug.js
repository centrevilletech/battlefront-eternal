import {sceneProperty,sceneTransform} from "../rust-render/scene-changes.js";
import { lineSource, LINE_KIND } from '../line-source.js';
import * as THREE from 'three';
import { FLIGHT_CLASSES } from './config.js';
// Created only on the presentation host. The same panel is useful in test scenes.
export class FlightDebug {
  constructor(scene, parent = document.body) {
    this.end=new THREE.Vector3();
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
    this.axes = lineSource(LINE_KIND.AXES); scene.add(this.axes);
    this.lines = lineSource(LINE_KIND.FLIGHT,128); scene.add(this.lines);
  }
  update(v, cameraTarget) {
    sceneProperty(this.axes,"visible",sceneProperty(this.lines,"visible",Boolean(v?.flying))); if (!v?.flying) return;
    sceneTransform(this.axes.position.copy(v.root.position)); sceneTransform(this.axes.quaternion.copy(v.root.quaternion));
    this.readout.textContent = JSON.stringify({ state: v.flightState, speed: v.airspeed, rates: [v.tiltRate, v.yawRate, v.flightRollRate], stick: [v.stickX, v.stickY], bank: v.bank, rollLock: v.rollLock, power: v.power, lock: v.flightLockState, timer: v.flightLockTimer, landing: v.landingReason }, null, 2);
    const p=v.root.position,q=v.root.quaternion,source=this.lines.userData.lineSource;
    source.count=0;
    this.end.copy(p).addScaledVector(v.velocity,1.5);this.segment(p,this.end);
    this.end.copy(p);this.end.y-=v.flightConfig.landingHeight;this.segment(p,this.end);
    if(cameraTarget)this.segment(p,cameraTarget);
    const radius=Math.tan(v.flightConfig.lockCone*Math.PI/180)*60;
    for(let i=0;i<16;i++){sceneTransform(this.end.set(Math.cos(i*Math.PI/8)*radius,Math.sin(i*Math.PI/8)*radius,60).applyQuaternion(q)).add(p);this.segment(p,this.end);}
    source.serial++;
  }
  // FlightDebug's line helper writes the bounded source directly, without
  // temporary point arrays, replacement geometries or Vector3 clones.
  segment(a,b){
    const source=this.lines.userData.lineSource,at=source.count*3;
    source.positions[at]=a.x;source.positions[at+1]=a.y;source.positions[at+2]=a.z;
    source.positions[at+3]=b.x;source.positions[at+4]=b.y;source.positions[at+5]=b.z;source.count+=2;
  }
  dispose() { this.panel.remove(); this.scene.remove(this.axes, this.lines); }
}
