import * as THREE from "three";
import { lighting } from "./materials.js";

const HITS = 6;
const VERTEX = /* glsl */ `
  varying vec2 vUv;
  varying vec3 vLocal;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    vUv = uv;
    vLocal = position;
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vView = view.xyz;
    vNormal = normalMatrix * normal;
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D fieldMap;
  uniform float hasMap, time, fade, charge, enhanced, birth, inside, flash;
  uniform vec3 fieldColor, specularColor, sunDirection, sunColor;
  uniform vec4 impacts[${HITS}];
  uniform float powers[${HITS}], blasts[${HITS}];
  varying vec2 vUv;
  varying vec3 vLocal, vNormal, vView;

  float hash(vec3 p) { p = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3)); p *= 17.0; return fract(p.x * p.y * p.z * (p.x + p.y + p.z)); }
  float noise(vec3 p) {
    vec3 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash(i), hash(i+vec3(1,0,0)), f.x), mix(hash(i+vec3(0,1,0)), hash(i+vec3(1,1,0)), f.x), f.y),
      mix(mix(hash(i+vec3(0,0,1)), hash(i+vec3(1,0,1)), f.x), mix(hash(i+vec3(0,1,1)), hash(i+vec3(1,1,1)), f.x), f.y), f.z);
  }

  float cells(vec2 p) {
    vec2 period = vec2(1.7320508, 1.0);
    vec2 a = mod(p, period) - period * 0.5;
    vec2 b = mod(p - period * 0.5, period) - period * 0.5;
    vec2 h = abs(dot(a,a) < dot(b,b) ? a : b);
    float d = max(dot(h, vec2(0.8660254, 0.5)), h.y);
    float aa = max(fwidth(d), 0.012);
    return smoothstep(0.5 - aa * 1.4, 0.5, d);
  }
  void main() {
    // One shell face, including when the first-person camera is inside.
    if ((inside < 0.5 && !gl_FrontFacing) || (inside > 0.5 && gl_FrontFacing)) discard;
    // The interior covers the viewport. Keep a faint field and localized
    // damage feedback without running exterior plasma, lattice, specular and
    // expanding-ring work for every pixel the player aims through. Seen from
    // inside the shell is nearly face-on everywhere, so its rim term is no
    // cue there: a light veil, clear in the middle of the view and thicker
    // toward its edges, with the field's drifting cloud, shows it is up.
    if (inside > 0.5 && enhanced > 0.5) {
      vec3 p = normalize(vLocal);
      float hit = flash * 0.2;
      for (int i = 0; i < ${HITS}; i++) {
        float age = time - impacts[i].w;
        if (age >= 0.0 && age < 1.15) {
          float proximity = max(dot(p, impacts[i].xyz), 0.0);
          float spot = pow(proximity, blasts[i] > 0.5 ? 8.0 : 64.0);
          hit += spot * exp(-age * 9.0) * powers[i];
        }
      }
      float edge = smoothstep(0.2, 0.8, length(normalize(vView).xy));
      float cloud = mix(0.48, texture2D(fieldMap, vUv + vec2(time, 0.0)).r, hasMap);
      float veil = (0.04 + edge * 0.24) * (0.45 + cloud * 1.15);
      float alpha = (veil + exp(-birth * 12.0) * 0.1 + hit * 0.16) * fade;
      if (alpha < 0.002) discard;
      gl_FragColor = vec4(mix(vec3(0.12, 0.52, 0.9), vec3(0.38, 0.83, 1.0), edge) + hit * vec3(0.3, 0.5, 0.6), min(alpha, 0.34));
      return;
    }
    vec3 n = normalize(vNormal), eye = normalize(-vView), p = normalize(vLocal);
    float facing = abs(dot(n, eye));
    vec4 cloud = mix(vec4(0.48, 0.48, 0.48, 0.74), texture2D(fieldMap, vUv + vec2(time, 0.0)), hasMap);
    float strength = 1.0 - (1.0 - charge) * (1.0 - charge);
    vec3 light = normalize(mat3(viewMatrix) * -sunDirection);
    float highlight = pow(max(dot(reflect(-light, n), eye), 0.0), 72.0);
    // Native droideka shader uses 1 - 1.5*(N.V)^2 and scrolls one UV/s.
    float classicRim = max(1.0 - 1.5 * facing * facing, 0.0);
    float alpha = classicRim * cloud.a * fade * strength * 0.68;
    vec3 color = fieldColor * (0.65 + cloud.rgb * 0.7) + specularColor * highlight * sunColor * 1.7;
    float impactLight = 0.0, impactCore = 0.0, blastLight = 0.0;
    for (int i = 0; i < ${HITS}; i++) {
      float age = time - impacts[i].w;
      if (age >= 0.0 && age < 1.15) {
        float arc = acos(clamp(dot(p, impacts[i].xyz), -1.0, 1.0));
        if (enhanced > 0.5 && blasts[i] > 0.5) {
          // A broad pressure front washes around the shell, with a trailing
          // ring and a brief flare across the hemisphere facing the blast.
          float front = exp(-pow((arc - age * 5.0) / (0.12 + age * 0.14), 2.0));
          float wake = exp(-pow((arc - age * 3.4) / 0.09, 2.0));
          float wash = exp(-arc * arc / 1.5) * exp(-age * 8.0);
          blastLight += ((front + wake * 0.45) * exp(-age * 3.5) + wash * 0.8) * powers[i];
        }
        float ring = exp(-pow((arc - age * 2.9) / (0.028 + age * 0.065), 2.0));
        float echo = exp(-pow((arc - age * 1.8) / (0.022 + age * 0.04), 2.0));
        impactLight += (ring + echo * 0.35) * exp(-age * 3.0) * powers[i];
        impactCore += exp(-arc * arc / 0.025) * exp(-age * 9.0) * powers[i];
      }
    }
    if (enhanced > 0.5) {
      float rim = pow(1.0 - facing, 2.2);
      float edge = pow(1.0 - facing, 9.0);
      // Triplanar cell detail avoids a pinched grid at the sphere poles.
      vec3 weights = pow(abs(p), vec3(5.0));
      weights /= max(weights.x + weights.y + weights.z, 0.001);
      float lattice = dot(vec3(cells(p.yz * 19.0), cells(p.xz * 19.0), cells(p.xy * 19.0)), weights);
      float flow = 0.5 + 0.5 * sin(p.y * 36.0 - time * 3.0 + sin(p.x * 8.0 + time) * 0.6);
      float sweep = exp(-pow((p.y - (birth * 6.0 - 1.3)) / 0.10, 2.0)) * exp(-birth * 2.0);
      float pop = exp(-birth * 12.0);
      float stress = (1.0 - charge) * (0.5 + 0.5 * sin(time * 19.0 + p.y * 31.0));
      // Wandering plasma threads energize patches of the field. Using the
      // sphere direction avoids UV seams and keeps the clear center readable.
      float turbulence = noise(p * 9.0 + vec3(time * 0.37, -time * 0.23, time * 0.14));
      float current = p.y * 4.0 + p.x * 1.8 + turbulence * 1.5 - time * 0.42;
      float thread = 1.0 - smoothstep(0.012, 0.055 + fwidth(current), abs(fract(current) - 0.5));
      float energized = smoothstep(0.54, 0.79, noise(p * 3.0 + time * 0.24));
      float plasma = thread * energized * (0.18 + rim * 0.82);
      float detail = lattice * (rim * 0.18 + impactLight * 0.34 + blastLight * 0.75 + pop * 0.28 + sweep * 0.3);
      alpha = (0.010 + rim * 0.31 + edge * 0.25 + detail + plasma * 0.42 + flow * rim * 0.04 + sweep * 0.85 + pop * 0.30) * fade * (0.7 + strength * 0.3);
      color = mix(vec3(0.08, 0.40, 0.78), vec3(0.38, 0.83, 1.0), rim);
      color += vec3(0.32, 0.67, 0.93) * (detail * 2.0 + sweep * 1.8 + pop);
      color += vec3(0.28, 0.72, 1.0) * edge * 1.6;
      color += vec3(0.35, 0.85, 1.4) * plasma * 2.8;
      color += specularColor * highlight * 0.7;
      color = mix(color, vec3(0.45, 0.60, 1.0), stress * 0.16);
      alpha += blastLight * fade * 0.65;
      color += vec3(0.55, 0.85, 1.0) * blastLight * 1.8;
      // A faint moving interference field retains a clear center.
      alpha += cloud.r * 0.012 * fade;
    }
    float hit = (impactLight * 0.75 + impactCore + flash * (0.2 + classicRim * 0.8)) * fade;
    alpha += hit * mix(0.24, 0.8, enhanced);
    color += vec3(0.6, 0.9, 1.0) * hit * mix(0.6, 1.5, enhanced);
    // Inside the field, keep the player's view readable.
    alpha *= mix(1.0, 0.22, inside);
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(color, clamp(alpha, 0.0, 0.88));
  }
`;

// A narrow emission veil outside the field gives the rim an actual halo even
// without post bloom. A small sphere preserves depth at foreground edges.
const AURA = /* glsl */ `
  uniform float time, fade, birth, charge, inside, flash;
  varying vec3 vLocal, vNormal, vView;
  void main() {
    if (inside > 0.5 || !gl_FrontFacing) discard;
    float facing = abs(dot(normalize(vNormal), normalize(-vView)));
    float rim = pow(1.0 - facing, 5.0) * smoothstep(0.0, 0.08, facing);
    vec3 p = normalize(vLocal);
    float surge = 0.75 + 0.25 * sin(p.y * 16.0 + p.x * 11.0 - time * 3.0);
    float ignition = exp(-birth * 6.0);
    float alpha = rim * fade * (0.35 + ignition * 0.65 + flash * 0.3) * surge;
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(vec3(0.12, 0.52, 1.0), alpha);
  }
`;

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
      time: { value: 0 }, fade: { value: 0 }, charge: { value: 1 },
      enhanced: { value: 1 }, birth: { value: 10 }, inside: { value: 0 },
      flash: { value: 0 },
      fieldColor: { value: color(look.Color, [96, 160, 192]) },
      specularColor: { value: color(look.SpecularColor, [216, 240, 255]) },
      sunDirection: lighting.sunDirection, sunColor: lighting.sunColor,
      impacts: { value: Array.from({ length: HITS }, () => new THREE.Vector4(0, 0, 1, -1000)) },
      powers: { value: new Float32Array(HITS) },
      blasts: { value: new Float32Array(HITS) },
    };
    this.material = new THREE.ShaderMaterial({ uniforms: this.uniforms, vertexShader: VERTEX, fragmentShader: FRAGMENT,
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
    this.aura = new THREE.Mesh(new THREE.SphereGeometry(1, 48, 24), new THREE.ShaderMaterial({
      uniforms: this.uniforms, vertexShader: VERTEX, fragmentShader: AURA,
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
    this.uniforms.time.value = this.time;
    this.flash *= Math.exp(-10 * dt);
    this.uniforms.flash.value = this.flash;
    this.uniforms.birth.value = this.birth;
    this.uniforms.fade.value = this.fade;
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
