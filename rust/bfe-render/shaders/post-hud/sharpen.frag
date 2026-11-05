
  uniform sampler2D map;
  uniform float sharpness;
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 last = textureSize(map, 0) - 1;
    vec3 e = texelFetch(map, p, 0).rgb;
    vec3 b = texelFetch(map, clamp(p + ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 d = texelFetch(map, clamp(p - ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 f = texelFetch(map, clamp(p + ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 h = texelFetch(map, clamp(p - ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 lo = min(e, min(min(b, d), min(f, h)));
    vec3 hi = max(e, max(max(b, d), max(f, h)));
    vec3 amount = sqrt(clamp(min(lo, 2.0 - hi) / max(hi, vec3(1e-4)), 0.0, 1.0));
    vec3 w = -amount / mix(8.0, 5.0, sharpness);
    vec3 color = (e + (b + d + f + h) * w) / (1.0 + 4.0 * w);
    gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
  }
