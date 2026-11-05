
        uniform sampler2D map;
        uniform sampler2D blurred;
        uniform sampler2D glow;
        uniform float blend;
        uniform float bloomStrength;
        uniform float speedBlur;
        uniform float speedTurn;
        varying vec2 vUv;
        // Every read but the first sits behind a switch. Explicit levels (the
        // targets have one each) keep them there on Direct3D, whose compiler
        // otherwise hoists or flattens implicit-gradient reads: a plain frame
        // would pay for the blur, the glow and five motion-blur taps.
        void main() {
          vec3 sharp = texture2D(map, vUv).rgb;
          vec3 color = sharp;
          if (blend > 0.0) color = mix(color, textureLod(blurred, vUv, 0.0).rgb, blend);
          if (speedBlur > 0.0001) {
            vec2 radial = vUv - vec2(0.5);
            float iris = smoothstep(0.16, 0.62, length(radial));
            vec2 streak = (radial + vec2(speedTurn * 0.12, 0.0)) * speedBlur * iris;
            vec3 trail = sharp;
            for (int i = 1; i < 6; i++) {
              vec2 uv = clamp(vUv - streak * (float(i) / 5.0), vec2(0.001), vec2(0.999));
              trail += textureLod(map, uv, 0.0).rgb;
            }
            color = mix(color, trail / 6.0, iris * 0.85);
          }
          if (bloomStrength > 0.0) color += textureLod(glow, vUv, 0.0).rgb * bloomStrength * (1.0 - color);
          gl_FragColor = vec4(color, 1.0);
        }
      