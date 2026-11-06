import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from "three";
import { lighting } from "./materials.js";

const HITS = 6;



// A narrow emission veil outside the field gives the rim an actual halo even
// without post bloom. A small sphere preserves depth at foreground edges.


export class ShieldBubble {
  constructor(scene, look, texture, settings = null) {
    this.look = look;
    this.settings = settings;
    this.fade = 0;
    this.time = 0;
    this.birth = 10;
    this.on = false;
    this.pop = 0;
    this.fromPop = 0;
    this.fromFade = 0;
    this.flash = 0;
    this.hitIndex = 0;
    this.offset = new THREE.Vector3();
    this.source = new THREE.Vector3();
    this.cameraLocal = new THREE.Vector3();
    const map = look.Texture ? texture(look.Texture) : null;
    // Clone sampler state only: other users of the native atlas stay intact.
    this.map = map?.clone() || null;
    if (this.map) {
      this.map.wrapS = this.map.wrapT = THREE.RepeatWrapping;
      this.map.needsUpdate = true;
    }
    const color = (values, fallback) => new THREE.Vector3(...(values || fallback).slice(0, 3).map(x => x / 255));
    this.uniforms = {
      fieldMap: { value: this.map }, hasMap: { value: this.map ? 1 : 0 },
      shieldTime: { value: 0 }, shieldFade: { value: 0 }, charge: { value: 1 },
      enhanced: { value: 1 }, birth: { value: 10 }, inside: { value: 0 },
      flash: { value: 0 },
      fieldColor: { value: color(look.Color, [96, 160, 192]) },
      specularColor: { value: color(look.SpecularColor, [216, 240, 255]) },
      sunDirection: lighting.sunDirection, sunColor: lighting.sunColor,
      impacts: { value: Array.from({ length: HITS }, () => new THREE.Vector4(0, 0, 1, -1000)) },
      powers: { value: new Float32Array(HITS) },
      blasts: { value: new Float32Array(HITS) },
    };
    this.material = ownedEffectMaterial("effects/shield", { uniforms: this.uniforms,
      transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true });
    this.material.name = "droideka-energy-shield";
    this.mesh = new THREE.Mesh(new THREE.SphereGeometry(1, 80, 48), this.material);
    this.mesh.name = "droideka-shield";
    this.mesh.visible = false;
    this.mesh.renderOrder = 4;
    this.mesh.onBeforeRender = (renderer, scene, camera) => {
      if (this.map && this.sourceVersion !== this.map.source.version) {
        this.map.needsUpdate = true;
        this.sourceVersion = this.map.source.version;
      }
      this.cameraLocal.copy(camera.position).sub(this.mesh.position).divide(this.mesh.scale);
      this.uniforms.inside.value = this.cameraLocal.lengthSq() < 1 ? 1 : 0;
      this.material.uniformsNeedUpdate = true;
    };
    scene.add(this.mesh);
    this.aura = new THREE.Mesh(new THREE.SphereGeometry(1, 48, 24), ownedEffectMaterial("effects/shield-aura", {
      uniforms: this.uniforms,
      transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
    }));
    this.aura.name = "droideka-shield-aura";
    this.aura.renderOrder = 4;
    this.aura.visible = false;
    this.aura.onBeforeRender = (renderer, scene, camera) => {
      this.mesh.onBeforeRender(renderer, scene, camera);
      this.aura.material.uniformsNeedUpdate = true;
    };
    // Parenting makes external visibility/layer changes apply to both shells.
    this.mesh.add(this.aura);
    this.aura.scale.setScalar(1.035);
  }

  place(droideka, position = droideka.position) {
    const [x, y, z] = droideka.shield?.def.ShieldOffset || [0, 1, 0];
    const yaw = droideka.aimYaw;
    this.offset.set(Math.cos(yaw) * x + Math.sin(yaw) * z, y, -Math.sin(yaw) * x + Math.cos(yaw) * z);
    this.mesh.position.copy(position).add(this.offset);
  }

  // Called only when gameplay actually absorbs damage; source is optional.
  hit(source, amount, droideka, kind = null) {
    if (!(amount > 0)) return;
    this.place(droideka);
    const power = THREE.MathUtils.clamp(Math.sqrt(amount / Math.max(droideka.shield?.max || 1, 1)) * 3, 0.25, 1.5);
    if (!source || ![source.x, source.y, source.z].every(Number.isFinite)) {
      // Damage without provenance lights the field, without inventing a hit side.
      this.flash = Math.max(this.flash, power);
      return;
    }
    this.source.copy(source).sub(this.mesh.position);
    const radius = droideka.shield?.def.ShieldRadius ?? this.look.Radius ?? 1.25;
    this.source.x /= radius; this.source.y /= this.look.Height ?? radius; this.source.z /= radius;
    if (this.source.lengthSq() < 1e-8) this.source.set(0, 1, 0);
    this.source.normalize();
    const i = this.hitIndex++ % HITS;
    this.uniforms.impacts.value[i].set(this.source.x, this.source.y, this.source.z, this.time);
    this.uniforms.powers.value[i] = power;
    this.uniforms.blasts.value[i] = kind === "explosion" ? 1 : 0;
  }

  update(dt, droideka, { position = droideka.position } = {}) {
    dt = Math.max(dt, 0);
    this.time += dt;
    const shield = droideka.shield;
    const on = Boolean(shield?.on && !droideka.dead && !droideka.rolled);
    const enhanced = this.settings?.get("droidekaEffects") !== "classic";
    if (on !== this.on) {
      this.fromPop = this.pop;
      this.fromFade = this.fade;
      if (on) this.birth = 0;
    }
    this.on = on;
    this.birth += dt;
    const duration = enhanced ? (on ? 0.24 : 0.26) : (on ? this.look.FadeIn : this.look.FadeOut);
    this.fade = THREE.MathUtils.clamp(this.fade + (on ? dt : -dt) / Math.max(duration || 0.15, 1e-3), 0, 1);
    this.mesh.visible = this.fade > 0;
    this.aura.visible = enhanced && this.mesh.visible;
    this.place(droideka, position);
    const f = this.fade;
    // Native classic curve grows through 1.06 and settles to 1, shrinking off.
    const classic = on ? (f < 0.94 ? f * 1.06 / 0.94 : 1 + (1 - f)) : f;
    const progress = on ? (f - this.fromFade) / Math.max(1 - this.fromFade, 1e-4) : f / Math.max(this.fromFade, 1e-4);
    const expansion = 1 + 2.2 * (progress - 1) ** 3 + 1.2 * (progress - 1) ** 2;
    const pop = enhanced ? (on ? this.fromPop + (1 - this.fromPop) * expansion : this.fromPop * (0.7 + 0.3 * progress)) : classic;
    this.pop = f > 0 ? pop : 0;
    const radius = shield?.def.ShieldRadius ?? this.look.Radius ?? 1.25;
    this.mesh.scale.set(radius * Math.max(pop, 0.001), (this.look.Height ?? radius) * Math.max(pop, 0.001), radius * Math.max(pop, 0.001));
    this.uniforms.shieldTime.value = this.time;
    this.flash *= Math.exp(-10 * dt);
    this.uniforms.flash.value = this.flash;
    this.uniforms.birth.value = this.birth;
    this.uniforms.shieldFade.value = this.fade;
    this.uniforms.enhanced.value = enhanced ? 1 : 0;
    this.uniforms.charge.value = shield?.max > 0 ? THREE.MathUtils.clamp(shield.value / shield.max, 0, 1) : 0;
  }

  // Gone with its body (off the field): nothing left drawn, nothing to fade.
  hide() {
    this.on = false;
    this.fade = this.pop = 0;
    this.mesh.visible = false;
  }

  dispose() {
    this.aura.geometry.dispose(); this.aura.material.dispose();
    this.mesh.removeFromParent(); this.mesh.geometry.dispose(); this.material.dispose(); this.map?.dispose();
  }
}
