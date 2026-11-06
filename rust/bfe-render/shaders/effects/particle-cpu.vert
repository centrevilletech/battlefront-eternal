#version 300 es

#define attribute in
#define varying out
#define texture2D texture
precision highp float;
	precision highp int;
	precision highp sampler2D;
	precision highp samplerCube;
	precision highp sampler3D;
	precision highp sampler2DArray;
	precision highp sampler2DShadow;
	precision highp samplerCubeShadow;
	precision highp sampler2DArrayShadow;
	precision highp isampler2D;
	precision highp isampler3D;
	precision highp isamplerCube;
	precision highp isampler2DArray;
	precision highp usampler2D;
	precision highp usampler3D;
	precision highp usamplerCube;
	precision highp usampler2DArray;
	
#define HIGH_PRECISION
#define SHADER_TYPE ShaderMaterial
#define SHADER_NAME 
#define DOUBLE_SIDED
uniform mat4 modelMatrix;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat3 normalMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
#ifdef USE_INSTANCING
	attribute mat4 instanceMatrix;
#endif
#ifdef USE_INSTANCING_COLOR
	attribute vec3 instanceColor;
#endif
#ifdef USE_INSTANCING_MORPH
	uniform sampler2D morphTexture;
#endif
attribute vec3 position;
attribute vec3 normal;
attribute vec2 uv;
#ifdef USE_UV1
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


  
  attribute vec2 corner;
  attribute vec3 iPosition;
  attribute vec4 iColor;
  attribute vec4 iShape; // half size, rotation, streak length, shape
  attribute vec3 iAxis; // streak direction, or the flat quad's first axis
  attribute vec3 iAxis2; // the flat quad's second axis; x is a streak's tail width
  attribute vec4 iFrame; // flipbook cell: uv offset, uv scale

  varying vec2 vUv;
  varying vec4 vColor;
  varying float vDepth;
  varying float vSoftDistance;
  void main() {
    
    vColor = iColor;
    // Keep flat decals/shockwaves attached; soften volumetric sprites only.
    vSoftDistance = iShape.w > 1.5 ? 0.0 : clamp(iShape.x * 0.6, 0.04, 0.6);
    float size = iShape.x;
    vec2 uv = corner * 0.5 + 0.5;
    vec4 mv;
    if (iShape.w > 1.5) {
      vec3 p = iPosition + iAxis * (corner.x * size) + iAxis2 * (corner.y * size);
      mv = modelViewMatrix * vec4(p, 1.0);
    } else if (iShape.w > 0.5) {
      float head = corner.y * 0.5 + 0.5;
      vec3 dir = normalize(iAxis + vec3(0.0, 1e-6, 0.0));
      mv = modelViewMatrix * vec4(iPosition - dir * ((1.0 - head) * iShape.z), 1.0);
      vec3 along = normalize((modelViewMatrix * vec4(dir, 0.0)).xyz);
      vec3 side = normalize(cross(along, normalize(mv.xyz)) + vec3(1e-6, 0.0, 0.0));
      mv.xyz += side * corner.x * size * mix(iAxis2.x, 1.0, head);
      uv = vec2(1.0 - head, corner.x * 0.5 + 0.5);
    } else {
      mv = modelViewMatrix * vec4(iPosition, 1.0);
      float c = cos(iShape.y);
      float s = sin(iShape.y);
      mv.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
    }
    vUv = iFrame.xy + uv * iFrame.zw;
    vDepth = -mv.z;
    gl_Position = projectionMatrix * mv;
  }
