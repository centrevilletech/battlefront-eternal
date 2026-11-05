
  uniform sampler2D map;
  uniform vec2 inputSize;
  uniform vec2 outputSize;
  void main() {
    vec2 position = gl_FragCoord.xy * inputSize / outputSize;
    if (inputSize.x > outputSize.x) {
      // Supersampled: average the input pixels this one covers.
      vec2 step = 0.25 * inputSize / outputSize / inputSize;
      vec2 uv = position / inputSize;
      gl_FragColor = vec4(0.25 * (texture2D(map, uv + vec2(-step.x, -step.y)).rgb + texture2D(map, uv + vec2(step.x, -step.y)).rgb
        + texture2D(map, uv + vec2(-step.x, step.y)).rgb + texture2D(map, uv + step).rgb), 1.0);
      return;
    }
    vec2 center = floor(position - 0.5) + 0.5;
    vec2 f = position - center;
    vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    vec2 w3 = f * f * (-0.5 + 0.5 * f);
    vec2 w12 = w1 + w2;
    vec2 t0 = (center - 1.0) / inputSize, t3 = (center + 2.0) / inputSize, t12 = (center + w2 / w12) / inputSize;
    vec3 color = (texture2D(map, vec2(t0.x, t0.y)).rgb * w0.x + texture2D(map, vec2(t12.x, t0.y)).rgb * w12.x + texture2D(map, vec2(t3.x, t0.y)).rgb * w3.x) * w0.y
      + (texture2D(map, vec2(t0.x, t12.y)).rgb * w0.x + texture2D(map, t12).rgb * w12.x + texture2D(map, vec2(t3.x, t12.y)).rgb * w3.x) * w12.y
      + (texture2D(map, vec2(t0.x, t3.y)).rgb * w0.x + texture2D(map, vec2(t12.x, t3.y)).rgb * w12.x + texture2D(map, vec2(t3.x, t3.y)).rgb * w3.x) * w3.y;
    // No ringing past the four nearest texels.
    ivec2 n = ivec2(floor(position - 0.5));
    ivec2 last = ivec2(inputSize) - 1;
    vec3 a = texelFetch(map, clamp(n, ivec2(0), last), 0).rgb;
    vec3 b = texelFetch(map, clamp(n + ivec2(1, 0), ivec2(0), last), 0).rgb;
    vec3 c = texelFetch(map, clamp(n + ivec2(0, 1), ivec2(0), last), 0).rgb;
    vec3 d = texelFetch(map, clamp(n + ivec2(1, 1), ivec2(0), last), 0).rgb;
    color = clamp(color, min(min(a, b), min(c, d)), max(max(a, b), max(c, d)));
    gl_FragColor = vec4(color, 1.0);
  }
