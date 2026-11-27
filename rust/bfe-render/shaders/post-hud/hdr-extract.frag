      uniform sampler2D map;
      uniform float threshold;
      varying vec2 vUv;
      // The 2005 game's bloom reads a frame drawn at half its lighting (this
      // scene at full lighting, halved, as an 8-bit target holds it). A texel
      // passes whole when its luminance-weighted excess over the threshold
      // is above 0.05; any other texel is black.
      void main() {
        vec3 scaled = min(texture2D(map, vUv).rgb * 0.5, vec3(1.0));
        float excess = dot(max(scaled - vec3(threshold), vec3(0.0)), vec3(0.3, 0.59, 0.11));
        gl_FragColor = vec4(excess > 0.05 ? scaled : vec3(0.0), 1.0);
      }
