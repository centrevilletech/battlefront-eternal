// SpawnStage (vignette): Dark at the edges, a pool of light where the soldier stands, the battle
// behind graded toward the look's colour; a dip to dark on a cut.

  uniform vec3 shade;
  uniform vec3 pool;
  uniform vec2 center;
  uniform float aspect;
  uniform float strength;
  uniform float dip;
  varying vec2 vUv;
  void main() {
    vec2 d = (vUv - center) * vec2(aspect, 1.0);
    float r = length(d * vec2(0.8, 1.0));
    float dark = smoothstep(0.15, 1.0, r);
    float a = mix(0.42, 0.9, dark) * strength;
    vec3 c = mix(pool, shade, dark);
    gl_FragColor = vec4(c, min(a + dip, 1.0));
  }
