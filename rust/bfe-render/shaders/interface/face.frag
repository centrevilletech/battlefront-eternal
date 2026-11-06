// HoloMap (face): The map's picture in the hologram's colour, the posts' and the ring's
// kept; a split of red and blue at the sides, scan lines, a band of light
// rolling up, a glow round the rim.

  uniform sampler2D map;
  uniform vec3 tint;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  void main() {
    vec2 p = vUv * 2.0 - 1.0;
    float r = length(p);
    if (r > 1.0) discard;
    float split = 0.0022 * (0.6 + r);
    vec4 m = texture2D(map, vUv);
    vec3 col = vec3(texture2D(map, vUv + vec2(split, 0.0)).r, m.g, texture2D(map, vUv - vec2(split, 0.0)).b);
    float lum = dot(col, vec3(0.299, 0.587, 0.114));
    // The map's own picture (blue) takes the hologram's colour; the posts,
    // units and ring (green, red, yellow, grey) keep theirs.
    float picture = smoothstep(0.015, 0.1, col.b - max(col.r, col.g));
    vec3 holo = mix(col * 1.7, tint * lum * 2.7, picture);
    float scan = 0.8 + 0.2 * sin(vUv.y * 520.0 - time * 7.0);
    float band = exp(-pow((fract(vUv.y * 0.55 - time * 0.11) - 0.5) * 8.0, 2.0));
    float flicker = 0.95 + 0.05 * sin(time * 41.0) * sin(time * 13.0);
    float rim = smoothstep(0.84, 1.0, r);
    vec3 c = holo * scan * (1.0 + 0.7 * band) + tint * (0.05 + 0.05 * band + 0.55 * rim * rim);
    c *= (1.0 - smoothstep(0.97, 1.0, r)) * strength * flicker;
    gl_FragColor = vec4(c, 1.0);
  }
