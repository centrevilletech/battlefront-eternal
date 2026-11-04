
  uniform sampler2D tDiffuse;
  uniform vec2 resolution;
  varying vec2 vUv;

  float fxaaLuma( vec3 color ) {
    return dot( color, vec3( 0.299, 0.587, 0.114 ) );
  }

  void main() {
    vec2 texel = resolution;
    vec4 centre = texture( tDiffuse, vUv );
    // Each read between four texels: their bilinear average.
    float lumaNw = fxaaLuma( texture( tDiffuse, vUv + vec2( -0.5, -0.5 ) * texel ).rgb );
    float lumaSw = fxaaLuma( texture( tDiffuse, vUv + vec2( -0.5, 0.5 ) * texel ).rgb );
    float lumaNe = fxaaLuma( texture( tDiffuse, vUv + vec2( 0.5, -0.5 ) * texel ).rgb );
    float lumaSe = fxaaLuma( texture( tDiffuse, vUv + vec2( 0.5, 0.5 ) * texel ).rgb );
    float lumaM = fxaaLuma( centre.rgb );
    float lumaMax = max( max( lumaNw, lumaSw ), max( lumaNe, lumaSe ) );
    float lumaMin = min( min( lumaNw, lumaSw ), min( lumaNe, lumaSe ) );
    // Edge threshold 1/8, at least 0.05: FXAA 3.11's console defaults.
    if ( max( lumaMax, lumaM ) - min( lumaMin, lumaM ) < max( 0.05, lumaMax * 0.125 ) ) {
      gl_FragColor = centre;
      return;
    }
    float swMinusNe = lumaSw - lumaNe;
    float seMinusNw = lumaSe - lumaNw;
    vec2 dir = vec2( swMinusNe + seMinusNw, swMinusNe - seMinusNw );
    vec2 dir1 = dir / max( length( dir ), 1e-6 );
    vec3 n1 = texture( tDiffuse, vUv - dir1 * texel * 0.5 ).rgb;
    vec3 p1 = texture( tDiffuse, vUv + dir1 * texel * 0.5 ).rgb;
    // Edge sharpness 8: how far along a shallow edge the second pair reaches.
    vec2 dir2 = clamp( dir1 / max( min( abs( dir1.x ), abs( dir1.y ) ) * 8.0, 1e-6 ), -2.0, 2.0 );
    vec3 n2 = texture( tDiffuse, vUv - dir2 * texel * 2.0 ).rgb;
    vec3 p2 = texture( tDiffuse, vUv + dir2 * texel * 2.0 ).rgb;
    vec3 twoTap = ( n1 + p1 ) * 0.5;
    vec3 fourTap = ( n2 + p2 ) * 0.25 + twoTap * 0.5;
    float lumaFour = fxaaLuma( fourTap );
    // The wider blur crossed another edge: keep the narrow one.
    gl_FragColor = vec4( lumaFour < lumaMin || lumaFour > lumaMax ? twoTap : fourTap, centre.a );
  }
