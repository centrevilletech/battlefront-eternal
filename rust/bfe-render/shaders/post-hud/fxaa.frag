
  uniform sampler2D tDiffuse;
  uniform vec2 resolution;
  varying vec2 vUv;

  #define EDGE_STEP_COUNT 6
  #define EDGE_GUESS 8.0
  #define EDGE_STEPS 1.0, 1.5, 2.0, 2.0, 2.0, 4.0
  const float edgeSteps[EDGE_STEP_COUNT] = float[EDGE_STEP_COUNT]( EDGE_STEPS );

  const float contrastThreshold = 0.0312;
  const float relativeThreshold = 0.063;
  const float subpixelBlending = 1.0;

  float fxaaLuminance( vec3 color ) {
    return dot( color, vec3( 0.3, 0.59, 0.11 ) );
  }

  float sampleLuminance( vec2 uv ) {
    return fxaaLuminance( texture( tDiffuse, uv ).rgb );
  }

  float sampleLuminance( vec2 texSize, vec2 uv, float uOffset, float vOffset ) {
    uv += texSize * vec2( uOffset, vOffset );
    return sampleLuminance( uv );
  }

  void main() {
    vec2 uv = vUv;
    vec2 texSize = resolution.xy;
    vec4 centre = texture( tDiffuse, uv );
    float m = fxaaLuminance( centre.rgb );
    float n = sampleLuminance( texSize, uv, 0.0, 1.0 );
    float e = sampleLuminance( texSize, uv, 1.0, 0.0 );
    float s = sampleLuminance( texSize, uv, 0.0, -1.0 );
    float w = sampleLuminance( texSize, uv, -1.0, 0.0 );
    float highest = max( max( max( max( n, e ), s ), w ), m );
    float lowest = min( min( min( min( n, e ), s ), w ), m );
    float contrast = highest - lowest;
    if ( contrast < max( contrastThreshold, relativeThreshold * highest ) ) {
      gl_FragColor = centre;
      return;
    }
    float ne = sampleLuminance( texSize, uv, 1.0, 1.0 );
    float nw = sampleLuminance( texSize, uv, -1.0, 1.0 );
    float se = sampleLuminance( texSize, uv, 1.0, -1.0 );
    float sw = sampleLuminance( texSize, uv, -1.0, -1.0 );

    // The pixel's blend factor.
    float f = 2.0 * ( n + e + s + w );
    f += ne + nw + se + sw;
    f *= 1.0 / 12.0;
    f = abs( f - m );
    f = clamp( f / contrast, 0.0, 1.0 );
    float blendFactor = smoothstep( 0.0, 1.0, f );
    float pixelBlend = blendFactor * blendFactor * subpixelBlending;

    // The edge's direction.
    float horizontal =
      abs( n + s - 2.0 * m ) * 2.0 +
      abs( ne + se - 2.0 * e ) +
      abs( nw + sw - 2.0 * w );
    float vertical =
      abs( e + w - 2.0 * m ) * 2.0 +
      abs( ne + nw - 2.0 * n ) +
      abs( se + sw - 2.0 * s );
    bool isHorizontal = horizontal >= vertical;
    float pLuminance = isHorizontal ? n : e;
    float nLuminance = isHorizontal ? s : w;
    float pGradient = abs( pLuminance - m );
    float nGradient = abs( nLuminance - m );
    float pixelStep = isHorizontal ? texSize.y : texSize.x;
    float oppositeLuminance, gradient;
    if ( pGradient < nGradient ) {
      pixelStep = -pixelStep;
      oppositeLuminance = nLuminance;
      gradient = nGradient;
    } else {
      oppositeLuminance = pLuminance;
      gradient = pGradient;
    }

    // The edge's blend factor: how far along it the pixel sits.
    vec2 uvEdge = uv;
    vec2 edgeStep;
    if ( isHorizontal ) {
      uvEdge.y += pixelStep * 0.5;
      edgeStep = vec2( texSize.x, 0.0 );
    } else {
      uvEdge.x += pixelStep * 0.5;
      edgeStep = vec2( 0.0, texSize.y );
    }
    float edgeLuminance = ( m + oppositeLuminance ) * 0.5;
    float gradientThreshold = gradient * 0.25;

    vec2 puv = uvEdge + edgeStep * edgeSteps[0];
    float pLuminanceDelta = sampleLuminance( puv ) - edgeLuminance;
    bool pAtEnd = abs( pLuminanceDelta ) >= gradientThreshold;
    for ( int i = 1; i < EDGE_STEP_COUNT && !pAtEnd; i++ ) {
      puv += edgeStep * edgeSteps[i];
      pLuminanceDelta = sampleLuminance( puv ) - edgeLuminance;
      pAtEnd = abs( pLuminanceDelta ) >= gradientThreshold;
    }
    if ( !pAtEnd ) puv += edgeStep * EDGE_GUESS;

    vec2 nuv = uvEdge - edgeStep * edgeSteps[0];
    float nLuminanceDelta = sampleLuminance( nuv ) - edgeLuminance;
    bool nAtEnd = abs( nLuminanceDelta ) >= gradientThreshold;
    for ( int i = 1; i < EDGE_STEP_COUNT && !nAtEnd; i++ ) {
      nuv -= edgeStep * edgeSteps[i];
      nLuminanceDelta = sampleLuminance( nuv ) - edgeLuminance;
      nAtEnd = abs( nLuminanceDelta ) >= gradientThreshold;
    }
    if ( !nAtEnd ) nuv -= edgeStep * EDGE_GUESS;

    float pDistance, nDistance;
    if ( isHorizontal ) {
      pDistance = puv.x - uv.x;
      nDistance = uv.x - nuv.x;
    } else {
      pDistance = puv.y - uv.y;
      nDistance = uv.y - nuv.y;
    }
    float shortestDistance;
    bool deltaSign;
    if ( pDistance <= nDistance ) {
      shortestDistance = pDistance;
      deltaSign = pLuminanceDelta >= 0.0;
    } else {
      shortestDistance = nDistance;
      deltaSign = nLuminanceDelta >= 0.0;
    }
    float edgeBlend = deltaSign == ( m - edgeLuminance >= 0.0 ) ? 0.0 : 0.5 - shortestDistance / ( pDistance + nDistance );

    float finalBlend = max( pixelBlend, edgeBlend );
    if ( isHorizontal ) uv.y += pixelStep * finalBlend;
    else uv.x += pixelStep * finalBlend;
    gl_FragColor = texture( tDiffuse, uv );
  }
