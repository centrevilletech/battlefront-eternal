
      uniform sampler2D map;
      uniform vec2 direction;
      varying vec2 vUv;
      void main() {
        vec3 c = texture2D(map, vUv).rgb * 0.227027;
        c += texture2D(map, vUv + direction * 1.384615).rgb * 0.316216;
        c += texture2D(map, vUv - direction * 1.384615).rgb * 0.316216;
        c += texture2D(map, vUv + direction * 3.230769).rgb * 0.070270;
        c += texture2D(map, vUv - direction * 3.230769).rgb * 0.070270;
        gl_FragColor = vec4(c, 1.0);
      }
    