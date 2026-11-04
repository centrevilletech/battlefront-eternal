
      uniform sampler2D map;
      uniform vec2 texel;
      varying vec2 vUv;
      vec3 bright(vec2 uv) {
        vec3 c = texture2D(map, uv).rgb;
        float peak = max(c.r, max(c.g, c.b));
        float knee = clamp(peak - 0.65, 0.0, 0.24);
        float contribution = max(peak - 0.77, knee * knee / 0.48);
        return c * contribution / max(peak, 0.0001);
      }
      void main() {
        vec3 c = bright(vUv + texel * vec2(-1.0, -1.0));
        c += bright(vUv + texel * vec2(1.0, -1.0));
        c += bright(vUv + texel * vec2(-1.0, 1.0));
        c += bright(vUv + texel * vec2(1.0, 1.0));
        gl_FragColor = vec4(c * 0.25, 1.0);
      }
    