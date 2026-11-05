
  uniform highp sampler2D depthMap;
  uniform highp sampler2D velocityMap;
  uniform mat4 inverseJittered;
  uniform mat4 currentViewProjection;
  uniform mat4 previousViewProjection;
  vec2 motionAt(ivec2 t, float depth, vec2 size) {
    vec4 moved = texelFetch(velocityMap, t, 0);
    if (moved.a > 0.25 && moved.a < 0.75) return moved.rg;
    vec2 uv = (vec2(t) + 0.5) / size;
    vec4 world = inverseJittered * vec4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
    world /= world.w;
    vec4 now = currentViewProjection * world;
    vec4 then = previousViewProjection * world;
    return (now.xy / now.w - then.xy / then.w) * 0.5;
  }
  // rg: the motion (UV) of the nearest surface in the 3x3; b: how far, in
  // input pixels, that differs from this pixel's own (an edge where history
  // from either side may be wrong).
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 size = textureSize(velocityMap, 0);
    // The first-person weapon stays where it is on screen.
    if (texelFetch(velocityMap, p, 0).a > 0.75) { gl_FragColor = vec4(0.0); return; }
    float own = texelFetch(depthMap, p, 0).r;
    float closest = own;
    ivec2 at = p;
    for (int y = -1; y <= 1; y++) for (int x = -1; x <= 1; x++) {
      ivec2 t = clamp(p + ivec2(x, y), ivec2(0), size - 1);
      float depth = texelFetch(depthMap, t, 0).r;
      if (depth < closest) { closest = depth; at = t; }
    }
    vec2 fsize = vec2(size);
    vec2 motion = motionAt(at, closest, fsize);
    float spread = at == p ? 0.0 : length((motion - motionAt(p, own, fsize)) * fsize);
    gl_FragColor = vec4(motion, spread, 1.0);
  }
