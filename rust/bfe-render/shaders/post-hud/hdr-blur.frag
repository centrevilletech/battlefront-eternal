      uniform sampler2D map;
      uniform vec2 offset;
      uniform float weight;
      varying vec2 vUv;
      // One pass of the 2005 game's bloom: four diagonal taps `offset` away,
      // each weighted.
      void main() {
        vec3 c = texture2D(map, vUv + offset).rgb;
        c += texture2D(map, vUv + vec2(-offset.x, offset.y)).rgb;
        c += texture2D(map, vUv + vec2(offset.x, -offset.y)).rgb;
        c += texture2D(map, vUv - offset).rgb;
        gl_FragColor = vec4(c * weight, 1.0);
      }
