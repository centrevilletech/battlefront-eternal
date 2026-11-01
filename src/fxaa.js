import { FXAAShader as ThreeFXAA } from "three/addons/shaders/FXAAShader.js";

// three's FXAA (NVIDIA's algorithm; C# implementation by Jasper Flick, GLSL
// port by Dave Hoskins; three.js addons, MIT), with the same result for
// fewer texture reads. three's reads all nine neighbours before it decides
// whether a pixel is on an edge at all, a choice the centre and its four
// direct neighbours alone make; most pixels aren't, and keep the centre
// colour. Here the four diagonals are read only for pixels on an edge, and a
// skipped pixel returns the centre read it already made: five reads instead
// of ten. On an old integrated GPU the pass cost more than the whole near
// scene's shading at the same size.
const fragmentShader = /* glsl */ `
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
`;

export const FXAAShader = { name: "FXAAShader", uniforms: ThreeFXAA.uniforms, vertexShader: ThreeFXAA.vertexShader, fragmentShader };

// FXAA (FAST): FXAA 3.11's console path (Timothy Lottes, NVIDIA), as the
// PlayStation 3 and Xbox 360 ran it: the contrast of four diagonal reads and
// the centre, then a blur along the edge's direction from two pairs of reads,
// without the PC path's search along the edge. Nine reads a pixel and no
// loops: on an old integrated GPU about a third of the cost of FXAA above
// (Intel HD 2000 at 960x540: 2.9 ms against 7.6), a little softer on long,
// nearly horizontal or vertical edges.
const fastFragmentShader = /* glsl */ `
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
`;

export const FXAAFastShader = { name: "FXAAFastShader", uniforms: ThreeFXAA.uniforms, vertexShader: ThreeFXAA.vertexShader, fragmentShader: fastFragmentShader };
