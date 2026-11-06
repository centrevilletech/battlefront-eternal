
  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec3 vWorld;
  void main() {
    vec2 p = vWorld.xz;
    float r = length(p);
    vec2 cell = p / 0.6;
    vec2 g = abs(fract(cell - 0.5) - 0.5) / fwidth(cell);
    float grid = 1.0 - min(min(g.x, g.y), 1.0);
    float w = fract(r * 0.55 - time * 0.22);
    float ripple = smoothstep(0.0, 0.03, w) * (1.0 - smoothstep(0.03, 0.2, w));
    float fade = 1.0 - smoothstep(1.0, 7.0, r);
    float a = (grid * 0.2 + ripple * 0.28 + 0.06 * exp(-r * 0.8)) * fade * strength;
    gl_FragColor = vec4(color * a, a);
  }
