	attribute vec2 uv1;
#endif
#ifdef USE_UV2
	attribute vec2 uv2;
#endif
#ifdef USE_UV3
	attribute vec2 uv3;
#endif
#ifdef USE_TANGENT
	attribute vec4 tangent;
#endif
#if defined( USE_COLOR_ALPHA )
	attribute vec4 color;
#elif defined( USE_COLOR )
	attribute vec3 color;
#endif
#ifdef USE_SKINNING
	attribute vec4 skinIndex;
	attribute vec4 skinWeight;
#endif


  uniform float fabricTime;
  attribute vec2 fabricAnchor;
  float fabricOffset(vec3 p) {
    float loose = clamp((fabricAnchor.x - p.y) / fabricAnchor.y, 0.0, 1.0);
    float wave = sin(p.y * 1.7 + p.x * 0.23 + p.z * 0.19 - fabricTime * 2.1);
    wave += 0.3 * sin(p.y * 3.9 + p.x * 0.41 - fabricTime * 3.7);
    return 0.13 * loose * loose * wave;
  }
  vec3 fabricGradient(vec3 p) {
    const float e = 0.01;
    return vec3(
      fabricOffset(p + vec3(e,0,0)) - fabricOffset(p - vec3(e,0,0)),
      fabricOffset(p + vec3(0,e,0)) - fabricOffset(p - vec3(0,e,0)),
      fabricOffset(p + vec3(0,0,e)) - fabricOffset(p - vec3(0,0,e))
    ) / (2.0 * e);
  }
