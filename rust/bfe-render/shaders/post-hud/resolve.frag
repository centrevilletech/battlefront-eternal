
  uniform sampler2D colorMap;
  uniform highp sampler2D motionMap;
  uniform sampler2D historyMap;
  uniform vec2 inputSize;
  uniform vec2 outputSize;
  uniform vec2 jitter;
  uniform float reset;
  uniform float maxWeight;
  uniform float kernel;
  vec3 toYCoCg(vec3 c) { return vec3(0.25 * c.r + 0.5 * c.g + 0.25 * c.b, 0.5 * c.r - 0.5 * c.b, -0.25 * c.r + 0.5 * c.g - 0.25 * c.b); }
  vec3 toRGB(vec3 c) { return vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z); }
  // Catmull-Rom from five bilinear taps (the corners dropped).
  vec4 pastAt(vec2 uv) {
    vec2 position = uv * outputSize;
    vec2 center = floor(position - 0.5) + 0.5;
    vec2 f = position - center;
    vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    vec2 w3 = f * f * (-0.5 + 0.5 * f);
    vec2 w12 = w1 + w2;
    vec2 t0 = (center - 1.0) / outputSize, t3 = (center + 2.0) / outputSize, t12 = (center + w2 / w12) / outputSize;
    vec4 sum = texture2D(historyMap, vec2(t12.x, t0.y)) * (w12.x * w0.y)
      + texture2D(historyMap, vec2(t0.x, t12.y)) * (w0.x * w12.y)
      + texture2D(historyMap, t12) * (w12.x * w12.y)
      + texture2D(historyMap, vec2(t3.x, t12.y)) * (w3.x * w12.y)
      + texture2D(historyMap, vec2(t12.x, t3.y)) * (w12.x * w3.y);
    return sum / (w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y);
  }
  void main() {
    vec2 q = gl_FragCoord.xy * inputSize / outputSize;
    ivec2 size = ivec2(inputSize);
    ivec2 base = ivec2(floor(q + jitter));
    float toOutput = outputSize.x / inputSize.x;
    vec3 sum = vec3(0.0), wide = vec3(0.0), m1 = vec3(0.0), m2 = vec3(0.0), lo = vec3(1e4), hi = vec3(-1e4);
    float weight = 0.0, wideWeight = 0.0;
    for (int y = -1; y <= 1; y++) for (int x = -1; x <= 1; x++) {
      ivec2 t = clamp(base + ivec2(x, y), ivec2(0), size - 1);
      vec3 c = texelFetch(colorMap, t, 0).rgb;
      vec2 d = vec2(t) + 0.5 - jitter - q;
      // Narrow in output pixels (detail the history gathers), wide in input
      // pixels (a smooth picture from this frame alone).
      float w = exp(-kernel * dot(d, d) * toOutput * toOutput);
      float v = exp(-1.2 * dot(d, d));
      sum += c * w; weight += w;
      wide += c * v; wideWeight += v;
      vec3 s = toYCoCg(c);
      m1 += s; m2 += s * s;
      lo = min(lo, s); hi = max(hi, s);
    }
    vec3 plain = wide / wideWeight;
    vec3 moved = texelFetch(motionMap, clamp(base, ivec2(0), size - 1), 0).rgb;
    vec2 motion = moved.rg;
    vec2 before = gl_FragCoord.xy / outputSize - motion;
    vec4 past = pastAt(before);
    bool outside = any(lessThan(before, vec2(0.0))) || any(greaterThan(before, vec2(1.0)));
    float history = reset > 0.5 || outside ? 0.0 : clamp(past.a, 0.0, 1.0) * 16.0;
    // Clip into the neighbourhood's colour box.
    vec3 mean = m1 / 9.0;
    vec3 sigma = sqrt(max(m2 / 9.0 - mean * mean, 0.0));
    vec3 boxLo = max(lo, mean - sigma), boxHi = min(hi, mean + sigma);
    vec3 middle = 0.5 * (boxHi + boxLo), extent = 0.5 * (boxHi - boxLo) + 0.002;
    vec3 was = toYCoCg(max(past.rgb, 0.0));
    vec3 offset = was - middle;
    vec3 reach = abs(offset / extent);
    float far = max(reach.x, max(reach.y, reach.z));
    vec3 clipped = far > 1.0 ? middle + offset / far : was;
    history *= exp(-2.0 * max(far - 1.0, 0.0));
    // Where an edge's two sides move apart, neither side's history holds.
    history *= exp(-0.5 * moved.b * toOutput);
    // A bright thing that wasn't there: at the neighbourhood's top while the
    // history sits at its bottom.
    float now = toYCoCg(plain).x;
    float rise = (now - clipped.x) / (hi.x - lo.x + 0.002);
    history *= 1.0 - 0.85 * clamp((rise - 0.55) * 4.0, 0.0, 1.0) * step(0.12, now - clipped.x);
    history = min(history, maxWeight);
    // With little history this frame counts as one smooth sample; as the
    // history fills, as its own sparse, sharp samples. Otherwise pixels
    // without a history would show whichever sample landed close: stipple.
    float sharp = clamp(history / (0.5 * maxWeight), 0.0, 1.0);
    vec3 add = mix(plain, sum, sharp);
    float added = mix(1.0, weight, sharp);
    float total = history + added;
    vec3 color = (toRGB(clipped) * history + add) / max(total, 1e-4);
    gl_FragColor = vec4(max(color, 0.0), min(total, maxWeight) / 16.0);
  }
