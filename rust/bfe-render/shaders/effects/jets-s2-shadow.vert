#version 300 es

#define attribute in
#define varying out
#define texture2D texture
precision highp float;
	precision highp int;
	precision highp sampler2D;
	precision highp samplerCube;
	precision highp sampler3D;
	precision highp sampler2DArray;
	precision highp sampler2DShadow;
	precision highp samplerCubeShadow;
	precision highp sampler2DArrayShadow;
	precision highp isampler2D;
	precision highp isampler3D;
	precision highp isamplerCube;
	precision highp isampler2DArray;
	precision highp usampler2D;
	precision highp usampler3D;
	precision highp usamplerCube;
	precision highp usampler2DArray;
	
#define HIGH_PRECISION
#define SHADER_TYPE ShaderMaterial
#define SHADER_NAME 
#define DOUBLE_SIDED
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
uniform mat4 modelMatrix;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat3 normalMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
#ifdef USE_INSTANCING
	attribute mat4 instanceMatrix;
#endif
#ifdef USE_INSTANCING_COLOR
	attribute vec3 instanceColor;
#endif
#ifdef USE_INSTANCING_MORPH
	uniform sampler2D morphTexture;
#endif
attribute vec3 position;
attribute vec3 normal;
attribute vec2 uv;
#ifdef USE_UV1
	attribute vec2 uv1;
#endif
#ifdef USE_UV2
	attribute vec2 uv2;
#endif
#ifdef USE_UV3
	attribute vec2 uv3;
#endif
#ifdef USE_TANGENT
	attribute vec4 tangent;
#endif
#if defined( USE_COLOR_ALPHA )
	attribute vec4 color;
#elif defined( USE_COLOR )
	attribute vec3 color;
#endif
#ifdef USE_SKINNING
	attribute vec4 skinIndex;
	attribute vec4 skinWeight;
#endif


  attribute vec2 corner;
  attribute vec3 seed; // kind, phase within its life, salt (a splash's: its event, and part in eighths)
  uniform vec4 sources[5]; // nozzle; w is 1 while the fountain is drawn
  uniform float levels[5], lows[5]; // the tier's pool and the outer pool
  uniform float jetTime, side;
  uniform vec3 wind;
  uniform vec2 depthResolution;
  varying vec2 vUv;
  varying vec4 vCell; // atlas cell: offset, size; size 0 is a drop, -1 a splash's crown
  varying vec4 vLook; // coverage, erosion (a ring's radius), how aerated (a ring's width), kind
  varying vec2 vFlow; // a core streak: when its water left the nozzle; a salt
  varying vec3 vWorld, vSide, vUp, vTint;
  varying vec2 vLight; // sunlight reaching it through the plume, glint
  varying float vLevel, vDepth;
  const float G = 9.81, APEX = 8.00, TAU = 6.2831853, ROPES = 170.0, STRANDS = 160.0;
  const float ARCS = 28.0, BUNDLES = 7.0;
  // The tier's lip and the overflow's foot, and the outer pool's wall at the water.
  const float LIP = 4.00, FOOT = 4.82, WALL = 6.2;
  const int MIST = 0, FROTH = 1, SKIRT = 2, SPRAY = 3, SPLASH = 4, DROP = 5, CLUMP = 6, CORE = 7, RING = 8, FOAM = 9;
  // How a particle faces: along its motion, the eye (turned by spin), the eye but upright, or flat on the pool.
  const int ALONG = 0, EYE = 1, UPRIGHT = 2, FLAT = 3;
  
  float hash1(float p) { p = fract(p * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
  float noise1(float x) { float i = floor(x), f = fract(x); f = f * f * (3.0 - 2.0 * f); return mix(hash1(i), hash1(i + 1.0), f); }
  vec4 hash4(float p) {
    vec4 q = fract(vec4(p) * vec4(.1031, .1030, .0973, .1099));
    q += dot(q, q.wzxy + 33.33);
    return fract((q.xxyz + q.yzzw) * q.zywx);
  }

  
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform vec3 ambientTop;
  uniform vec3 ambientBottom;
  // The sun, a little less yellow than the map's (tuned for its lightmaps).
  vec3 sunLight() { return mix(sunColor, vec3(dot(sunColor, vec3(0.3333))), 0.35); }
  vec3 skyAbove() { return ambientTop * 1.3 + vec3(0.1, 0.13, 0.27); }
  // Light from the sky, and from the warm square below, on a face toward N.
  vec3 skyFill(vec3 N) { return mix(mix(ambient, ambientTop, 0.5) * 1.4 + vec3(0.06, 0.08, 0.14), ambientBottom * 0.85, 0.3 * max(-N.y, 0.0)); }
  // What a mirror shows: sky above, the square's walls toward the horizon, its floor below.
  vec3 skyReflect(vec3 R) {
    vec3 above = skyAbove(), around = mix(above, ambientBottom, 0.3), below = ambientBottom * 0.6;
    return R.y > 0.0 ? mix(around, above, smoothstep(0.0, 0.6, R.y)) : mix(around, below, smoothstep(0.0, 0.4, -R.y));
  }
  // Sunlight scattered toward the eye by spray and thin water, strongest
  // looking toward the sun (Henyey-Greenstein, g = 0.45; 1 is even
  // scattering), held below the sun's own glare.
  float forward(vec3 V) {
    float c = dot(-V, -sunDirection);
    return min(0.7975 / pow(1.2025 - 0.9 * c, 1.5), 2.5);
  }
  // Bright water rolls off toward a light grey-white: only glints, drawn
  // over this, reach white.
  vec3 soften(vec3 c) {
    float m = max(c.r, max(c.g, c.b));
    return m > 0.8 ? c * ((0.8 + 0.15 * (1.0 - exp(-(m - 0.8) / 0.15))) / m) : c;
  }
  // Spray, lit as the shape it hangs in (normal N): the sun on that side, the
  // sky in its shade, and the sun scattered through it toward the eye.
  vec3 sprayLight(vec3 N, vec3 V, float lit) {
    float facing = clamp(dot(N, -sunDirection) * 0.6 + 0.4, 0.0, 1.0);
    return soften(skyFill(N) * 0.9 + sunLight() * lit * (0.65 * facing * facing + 0.6 * forward(V)));
  }

  
  void ropeStart(float j, float salt, float born, out vec2 dir, out vec4 start) {
    float n = j * 1.913 + salt;
    vec4 h = hash4(j * 3.17 + salt * 11.0);
    vec4 w = 0.5 + 0.5 * sin(born * vec4(0.43, 0.53, 0.71, 0.61) + n * vec4(0.9, 1.7, 3.1, 2.3));
    if (j >= ROPES + STRANDS) {
      float theta = (j - ROPES - STRANDS + 0.5 + 0.6 * (h.x - 0.5)) / ARCS * TAU + salt + 0.08 * sin(born * 0.5 + n);
      dir = vec2(cos(theta), sin(theta));
      start = vec4(0.6 + 0.4 * w.x, 3.2 + 0.6 * w.y, APEX * (0.84 + 0.08 * w.z), 0.6 + 0.8 * w.w);
      return;
    }
    float fine = step(ROPES, j), bundle = floor(h.x * BUNDLES), reach = 0.85 + 0.3 * hash1(bundle * 7.1 + salt);
    float theta = h.y < 0.8 ? (bundle + hash1(bundle * 5.3 + salt)) / BUNDLES * TAU + (h.z - 0.5) * 0.45 : h.z * TAU;
    theta += salt + 0.2 * sin(born * 0.37 + n * 2.1) + 0.1 * sin(born * 0.91 + n * 5.3);
    dir = vec2(cos(theta), sin(theta));
    float close = step(h.w, 0.4) * (1.0 - fine);
    start = fine > 0.5 ? vec4(0.35 + 0.8 * w.x, (0.6 + 1.2 * w.y) * reach, APEX * (0.76 + 0.18 * w.z), 0.8 * w.w)
      : vec4(0.3 + 0.7 * w.x, mix((1.0 + 0.9 * w.y) * reach, 0.25 + 0.4 * w.y, close), APEX * (0.84 + 0.12 * w.z), 0.3 + 0.6 * w.w);
  }
  vec3 ropeAt(vec2 dir, vec4 start, float age) {
    float d = start.x + start.y * age;
    return vec3(dir.x * d, start.z + start.w * age - 0.5 * G * age * age, dir.y * d) + wind * 0.3 * age * age;
  }
  // Which pool a stream falls into: the tier's (inner) or the outer one.
  bool ropeOuter(float j) { return j >= ROPES + STRANDS; }
  // Where a stream's water that left the crown at born meets its pool.
  vec3 ropeLanding(float j, float salt, float born, float level, out vec2 dir) {
    vec4 start;
    ropeStart(j, salt, born, dir, start);
    return ropeAt(dir, start, (start.w + sqrt(start.w * start.w + 2.0 * G * max(start.z - level, 0.0))) / G);
  }
  // Its own look: radius (m), opacity, where it breaks into drops (as a
  // share of its fall), how aerated.
  vec4 ropeLook(float j, float salt) {
    vec4 h = hash4(j * 5.11 + salt * 7.3 + 3.0);
    if (j >= ROPES + STRANDS) return vec4(0.012 + 0.012 * h.x, 0.7 + 0.3 * h.y, 0.55 + 0.35 * h.z, 0.45 + 0.4 * h.w);
    return j < ROPES ? vec4(0.011 * exp(1.5 * h.x), 0.6 + 0.4 * h.y, 0.3 + 0.6 * h.z, 0.4 + 0.6 * h.w)
      : vec4(0.004 + 0.008 * h.x, 0.5 + 0.4 * h.y, 0.15 + 0.45 * h.z, 0.25 + 0.45 * h.w);
  }

  
  float sunThrough(vec3 p) {
    vec3 L = -sunDirection;
    float reach = max(APEX + 0.4 - p.y, 0.0) / max(L.y, 0.2), depth = 0.0;
    for (int k = 0; k < 8; k++) {
      vec3 q = p + L * reach * (float(k) + 0.5) / 8.0;
      float r = length(q.xz), R = 0.6 + 1.45 * sqrt(max(2.0 * (APEX * 0.9 - q.y) / 9.81, 0.0));
      depth += 0.9 * exp(-pow((r - R) / 0.3, 2.0)) * step(q.y, APEX * 0.95) * reach / 8.0;
    }
    float l2 = max(dot(L.xz, L.xz), 1e-4), tc = -dot(p.xz, L.xz) / l2, rc = 0.15 + 0.05 * clamp(p.y + L.y * tc, 0.0, APEX);
    float miss = length(p.xz + L.xz * tc), half_ = sqrt(max(rc * rc - miss * miss, 0.0)) / sqrt(l2);
    if (p.y + L.y * tc < APEX * 0.8) depth += 2.5 * clamp(tc + half_, 0.0, 2.0 * half_);
    vec3 c = vec3(0.0, APEX * 0.88, 0.0) - p;
    float tb = dot(c, L), across = sqrt(max(1.0 - (dot(c, c) - tb * tb), 0.0));
    depth += 0.8 * clamp(tb + across, 0.0, 2.0 * across);
    return exp(-0.7 * depth);
  }

  uniform float perFountain;
  
  uniform float visible[5];
  int fountain(int block) { return int(visible[gl_InstanceID / block] + 0.5); }

  // A stream to follow, for u in [0, 1): an outer arc for the first share
  // of u, one of the ropes and strands for the rest.
  float pick(float u, float share) {
    return u < share ? ROPES + STRANDS + floor(u / share * ARCS) : floor((u - share) / (1.0 - share) * (ROPES + STRANDS));
  }
  void main() {
    int i = fountain(int(perFountain)), kind = int(seed.x + 0.5);
    vec4 source = sources[i];
    float lives[10] = float[10](6.00, 1.70, 1.80, 1.45, 0.50, 1.40, 2.45, 0.70, 1.10, 3.40);
    float life = lives[kind];
    // level: the tier's pool, low: the outer pool, pool: the one this
    // particle belongs to (all from the nozzle).
    float launch = sqrt(2.0 * G * APEX), level = levels[i] - source.y, low = lows[i] - source.y, pool = level, salt = float(i) * 2.17;
    float clock = jetTime + fract(seed.y + salt) * life, cycle = floor(clock / life), age = clock - cycle * life, t = age / life;
    // A splash's parts share its event (r): where it strikes, how hard.
    vec4 r = hash4((kind == SPLASH ? floor(seed.z) : seed.z) * 97.13 + cycle * 3.71 + salt), s = hash4(seed.z * 41.7 + cycle * 1.93 + 17.3 + salt);
    vec2 dir = vec2(cos(r.x * TAU), sin(r.x * TAU));
    vec3 p, v = vec3(0.0), shape = vec3(0.0, 1.0, 0.0);
    vec2 size;
    // trail: seconds of its motion it is drawn along; cap: the most that adds.
    float coverage, erode = 0.0, aerated = 0.0, trail = 0.0, cap = 0.0, glint = 0.0, spin = 0.0, born = 0.0;
    int facing = ALONG;
    bool drop = false;
    vec4 cell = vec4(0.0), mass = vec4(step(0.5, s.x) * 0.5, 0.0, 0.5, 1.0);
    if (kind == CORE) {
      // Streaks of water racing up the jet's surface.
      float up = launch * (0.95 + 0.06 * r.y), lat = 0.2 * r.z, d = 0.08 + 0.03 * r.w + lat * age;
      p = vec3(dir.x * d, up * age - 0.5 * G * age * age, dir.y * d);
      v = vec3(dir.x * lat, up - G * age, dir.y * lat);
      size = vec2(0.025 + 0.02 * s.y, 0.0);
      trail = 1.0 / 25.0; cap = 0.35;
      coverage = 0.75 * smoothstep(0.0, 0.04, age) * (1.0 - smoothstep(0.5 * life, life, age));
      aerated = 0.3 + 0.6 * smoothstep(level + 0.2, level + 1.2, p.y);
      born = jetTime - age;
      glint = 1.0;
    } else if (kind == CLUMP) {
      // Aerated water boiling up the column and churning in the crown.
      float up = launch * (0.86 + 0.13 * r.y), lat = 0.6 * pow(r.z, 1.5), apex = up / G, rise = age / apex;
      float d = 0.06 * r.w + lat * (1.0 - exp(-1.3 * age)) / 1.3 + (0.4 + 0.6 * s.w) * max(age - 0.5 * apex, 0.0);
      p = vec3(dir.x * d, up * age - 0.5 * G * age * age, dir.y * d);
      p.xz += wind.xz * max(age - 0.5 * apex, 0.0) * 0.3;
      v = vec3(0.0, up - G * age, 0.0);
      size = vec2(mix(0.1, 0.5, smoothstep(0.0, 1.0, rise)) * (1.0 + 0.3 * smoothstep(1.0, 1.6, rise)) * (0.75 + 0.5 * s.y));
      facing = EYE;
      spin = (s.z - 0.5) * 0.6;
      trail = 0.04; cap = 0.25;
      coverage = mix(0.45, 0.7, smoothstep(0.6, 1.0, rise)) * smoothstep(0.0, 0.05, age) * smoothstep(0.1, 0.35, rise) * (1.0 - smoothstep(1.1, 1.6, rise));
      erode = 0.1 + 0.25 * smoothstep(0.7, 1.2, rise);
      aerated = 0.95;
      cell = mass;
    } else if (kind == DROP) {
      if (r.y < 0.65) {
        // Shaken off a stream, with a kick from the water around it.
        float j = pick(s.x, 0.15), a0 = 0.05 + 0.75 * s.y;
        if (ropeOuter(j)) pool = low;
        vec2 rd; vec4 st; ropeStart(j, salt, jetTime - age - a0, rd, st);
        vec3 v0 = vec3(rd.x * st.y, st.w - G * a0, rd.y * st.y) + vec3(dir.x, 0.0, dir.y) * (0.3 + 0.9 * r.z) + vec3(0.0, 0.2 + 0.8 * r.w, 0.0);
        p = ropeAt(rd, st, a0) + v0 * age;
        v = v0;
      } else {
        // Thrown from the crown.
        float lat = 0.6 + 1.6 * r.w, up = 0.5 + 1.5 * r.z;
        p = vec3(dir.x * (0.2 + lat * age), APEX * 0.88 + up * age, dir.y * (0.2 + lat * age));
        v = vec3(dir.x * lat, up, dir.y * lat);
      }
      p.y -= 0.5 * G * age * age;
      v.y -= G * age;
      size = vec2(0.0025 + 0.0035 * s.z, 0.0);
      trail = 1.0 / 60.0; cap = 0.07;
      coverage = smoothstep(0.0, 0.08, age);
      drop = true;
      // Some catch the sun, flashing as they wobble and turn.
      glint = step(0.5, s.w) * pow(0.5 + 0.5 * sin(jetTime * (7.0 + 9.0 * s.z) + s.w * 40.0), 10.0);
    } else if (kind == SPRAY) {
      // Fine spray torn from a rope: drag holds it back a little and lets it
      // drift out, so it falls with the rope as a soft veil of streaks,
      // smeared along its fall, widening.
      float j = floor(s.x * ROPES);
      vec2 rd; vec4 st; ropeStart(j, salt, jetTime - age, rd, st);
      p = ropeAt(rd, st, age) + vec3(rd.x, 0.0, rd.y) * (0.1 + 0.3 * r.z) * age + vec3(0.0, 1.2 * r.w * age * age, 0.0);
      v = vec3(rd.x * st.y, st.w - G * age, rd.y * st.y);
      size = vec2(mix(0.06, 0.2, t) * (0.8 + 0.4 * s.y), 0.0);
      trail = 0.1; cap = 0.7;
      coverage = 0.22 * smoothstep(0.0, 0.17, t) * (1.0 - smoothstep(0.65, 1.0, t));
      shape = normalize(vec3(p.x, 0.25, p.z));
      cell = mass;
    } else if (kind == FROTH) {
      // Spray thrown off where the jet stalls and breaks at the crown, hanging
      // around it and settling out over the umbrella.
      float out_ = 0.3 + 1.0 * r.z, d = 0.2 + 1.2 * r.y + out_ * age;
      p = vec3(dir.x * d, APEX * (0.74 + 0.24 * s.x) + 0.6 * r.w * age - 0.9 * age * age, dir.y * d) + wind * age * 0.4;
      size = vec2(mix(0.35, 1.0, t) * (0.7 + 0.6 * s.y));
      facing = EYE;
      spin = s.z * TAU + age * 0.3;
      coverage = 0.34 * pow(sin(3.14159 * t), 1.2);
      shape = normalize(vec3(p.x, (p.y - APEX * 0.75) * 0.9, p.z));
      cell = mass;
    } else if (kind == SKIRT) {
      // Spray thrown up where the water lands, hanging over the pool.
      float j = pick(s.x, 0.25);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, jetTime - age - 1.25, pool, rd);
      // A metre at most: up to 1.4 m/s, slowed as it spreads.
      float up = 0.5 + 0.9 * r.w, out_ = 0.15 + 0.5 * r.z;
      p = vec3(at.x + rd.x * out_ * age, pool + 0.05 + max(up * age - 0.5 * age * age, 0.0), at.z + rd.y * out_ * age) + wind * age * 0.3;
      size = vec2(mix(0.15, 0.55, t) * (0.7 + 0.6 * s.y));
      facing = EYE;
      spin = s.z * TAU;
      coverage = 0.24 * pow(sin(3.14159 * t), 1.2);
      shape = normalize(vec3(p.x, 0.7, p.z));
      cell = mass;
    } else if (kind == MIST) {
      // A faint haze over the pools, drifting downwind, never above a metre or so.
      float d = 1.0 + 3.5 * r.z;
      p = vec3(dir.x * d, (d > LIP ? low : level) + 0.3 + 0.6 * r.w + 0.08 * age, dir.y * d) + wind * age * 0.8;
      size = vec2(mix(0.9, 1.7, t) * (0.8 + 0.4 * s.y));
      facing = EYE;
      spin = s.z * TAU + age * 0.1;
      coverage = 0.07 * pow(sin(3.14159 * t), 1.5);
      shape = normalize(vec3(p.x, 0.6, p.z));
      cell = mass;
    } else if (kind == SPLASH) {
      // A drop striking the pool where a stream lands: a crown thrown up
      // around it, a spike from its middle as the crown falls, a puff of
      // white water, and droplets from the crown's rim.
      int part = int(fract(seed.z) * 8.0 + 0.5);
      float j = pick(r.y, 0.3);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, jetTime - age - 1.25, pool, rd);
      if (part == 0) {
        float width = (0.04 + 0.06 * r.z) * (0.5 + t), height = (0.05 + 0.09 * r.w) * sin(3.14159 * min(t * 1.4, 1.0));
        p = vec3(at.x, pool + 0.5 * height, at.z);
        size = vec2(width, 0.5 * height);
        facing = UPRIGHT;
        coverage = 0.8 * step(0.005, height);
        erode = t;
        aerated = 0.25;
        cell = vec4(0.0, 0.0, -1.0, 0.0);
      } else if (part == 1) {
        float rise = 1.0 + 1.0 * r.w, a = max(age - 0.12, 0.0), h = max(rise * a - 0.5 * G * a * a, 0.0);
        p = vec3(at.x, pool + 0.5 * h, at.z);
        v = vec3(0.0, 1.0, 0.0);
        size = vec2(0.005 + 0.004 * r.z, 0.5 * h);
        coverage = 0.8 * step(0.01, h);
        glint = 0.6;
      } else if (part == 2) {
        p = vec3(at.x, pool + 0.04 + 0.12 * sin(3.14159 * t), at.z);
        size = vec2(mix(0.06, 0.22, sqrt(t)) * (0.8 + 0.5 * r.z));
        facing = EYE;
        spin = s.w * TAU;
        coverage = 0.6 * smoothstep(0.0, 0.1, t) * (1.0 - smoothstep(0.4, 1.0, t));
        erode = 0.25 + 0.4 * t;
        aerated = 0.9;
        cell = mass;
      } else {
        float a = (float(part) - 3.0) / 5.0 * TAU + r.z * TAU + (s.x - 0.5) * 0.8, up = 1.3 + 1.0 * s.y, speed = 0.3 + 0.4 * s.z;
        vec2 out_ = vec2(cos(a), sin(a));
        p = vec3(at.x + out_.x * (0.04 + speed * age), pool + up * age - 0.5 * G * age * age, at.z + out_.y * (0.04 + speed * age));
        v = vec3(out_.x * speed, up - G * age, out_.y * speed);
        size = vec2(0.003 + 0.003 * s.w, 0.0);
        trail = 1.0 / 60.0; cap = 0.05;
        coverage = step(pool, p.y);
        drop = true;
        glint = step(0.4, s.x) * 0.9;
      }
    } else if (kind == RING) {
      // A ripple spreading from a strike.
      float j = pick(r.y, 0.3);
      if (ropeOuter(j)) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, jetTime - age - 1.25, pool, rd);
      float radius = 0.04 + 0.5 * pow(t, 0.6);
      p = vec3(at.x, pool + 0.01, at.z);
      size = vec2(radius + 0.08);
      facing = FLAT;
      erode = radius / size.x;
      aerated = (0.025 + 0.035 * t) / size.x;
      coverage = (1.0 - t) * (1.0 - t) * 0.9;
    } else {
      // Foam riding outward from where the water lands, thinning as its
      // bubbles burst: in the tier out to its lip, in the outer pool from
      // the arcs' landings and from the overflow's foot toward the wall.
      float j = pick(r.y, 0.45);
      bool outer = ropeOuter(j);
      if (outer) pool = low;
      vec2 rd; vec3 at = ropeLanding(j, salt, jetTime - age - 1.25, pool, rd);
      if (outer && s.x < 0.5) at.xz = rd * FOOT * (0.94 + 0.04 * s.y);
      vec2 radial = normalize(at.xz + vec2(1e-4, 0.0));
      float limit = outer ? WALL : LIP - 0.15, reach = min(length(at.xz) + (0.12 + 0.25 * r.z) * age, limit);
      p = vec3(radial.x * reach, pool + 0.012, radial.y * reach);
      size = vec2(mix(0.12, 0.45, sqrt(t)) * (0.7 + 0.6 * s.y));
      facing = FLAT;
      spin = s.z * TAU + age * (s.w - 0.5) * 0.4;
      erode = 0.15 + 0.6 * t;
      coverage = 0.9 * smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.6, 1.0, t)) * (1.0 - smoothstep(limit - 0.15, limit, reach));
    }
    // Only this draw's half, beyond or before the axis; what lies on the
    // pool is drawn in one.
    vec2 eye = cameraPosition.xz - source.xz;
    if (side > -0.5 && (dot(p.xz, eye) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    vec3 center = source.xyz + p, toEye = normalize(cameraPosition - center);
    vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
    vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
    float depth = -(viewMatrix * vec4(center, 1.0)).z;
    // Metres per pixel there.
    float pixel = 2.0 * max(depth, 0.1) / (projectionMatrix[1][1] * depthResolution.y);
    vec3 axisX, axisY;
    vec2 extent = vec2(size.x, size.y > 0.0 ? size.y : size.x);
    float seen = 0.0;
    if (facing == ALONG) {
      // Along its motion as the eye sees it, smeared by its speed.
      vec3 across = v - dot(v, toEye) * toEye;
      seen = length(across);
      axisY = seen > 1e-4 ? across / seen : up;
      axisX = normalize(cross(axisY, toEye));
      extent.y += min(0.5 * trail * seen, cap);
    } else if (facing == UPRIGHT) {
      axisY = vec3(0.0, 1.0, 0.0);
      axisX = normalize(cross(axisY, toEye) + vec3(1e-5, 0.0, 0.0));
    } else if (facing == FLAT) {
      axisX = vec3(cos(spin), 0.0, sin(spin));
      axisY = vec3(-sin(spin), 0.0, cos(spin));
    } else {
      float c = cos(spin), sn = sin(spin);
      axisX = right * c + up * sn;
      axisY = up * c - right * sn;
      extent.y += min(0.5 * trail * abs(v.y), cap);
    }
    // Nothing thinner than about a pixel: drawn that wide and fainter. Drops
    // never grow past a few pixels, however near.
    vec2 shown = max(extent, vec2(0.7 * pixel));
    if (drop) shown = min(shown, vec2(1.5, 9.0) * pixel);
    float area = min(1.0, extent.x * extent.y / (shown.x * shown.y));
    coverage *= area;
    // A drop passes on how much of what is drawn it fills, so its glint keeps its light.
    if (drop) erode = area;
    vec3 world = center + axisX * corner.x * shown.x + axisY * corner.y * shown.y;
    vUv = corner * 0.5 + 0.5;
    vCell = cell;
    vLook = vec4(coverage * source.w, erode, aerated, float(kind));
    // Water further along its motion left earlier.
    vFlow = vec2(born - corner.y * shown.y / max(seen, 1e-3), salt + seed.z * 13.0);
    vWorld = world;
    vSide = axisX;
    vUp = axisY;
    float lit = 0.25 + 0.75 * sunThrough(world - source.xyz);
    vLight = vec2(lit, glint);
    // Spray and foam are lit here, by the shape they hang in; a ripple's
    // crest shows the sky.
    vTint = kind == RING ? skyAbove() : sprayLight(shape, toEye, lit);
    vLevel = pool + source.y;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
