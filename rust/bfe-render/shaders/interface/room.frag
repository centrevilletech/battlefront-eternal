
  uniform sampler2D map;
  uniform float time;
  uniform float scroll;
  uniform vec3 tint;
  uniform vec3 pulse;
  uniform float strength;
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    vec3 n = abs(vNormal);
    // Walls the right way up (the texture's rows run down).
    vec2 uv = n.y > 0.5 ? vWorld.xz : (n.x > 0.5 ? vec2(vWorld.z, -vWorld.y) : vec2(vWorld.x, -vWorld.y));
    uv = uv / 1.80;
    uv.x += time * scroll;
    vec3 t = texture2D(map, uv).rgb;
    float lum = dot(t, vec3(0.299, 0.587, 0.114));
    float line = smoothstep(0.18, 0.42, lum);
    float f = fract(vWorld.x * 0.09 + vWorld.y * 0.06 + vWorld.z * 0.05 - time * 0.13);
    float band = smoothstep(0.0, 0.012, f) * (1.0 - smoothstep(0.012, 0.08, f));
    float g = fract(vWorld.y * 0.085 + time * 0.06);
    float rise = smoothstep(0.0, 0.006, g) * (1.0 - smoothstep(0.006, 0.035, g));
    float pool = exp(-length(vWorld.xz) * 0.32) * step(0.5, n.y);
    float far = 1.0 / (1.0 + length(vWorld - vec3(0.0, 1.0, 0.0)) * 0.05);
    vec3 c = t * tint * (0.8 + 0.9 * pool) * far + pulse * line * (band * 1.5 + rise * 0.9 + pool * 0.6);
    gl_FragColor = vec4(c * strength, 1.0);
  }
