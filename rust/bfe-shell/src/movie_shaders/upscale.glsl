#version 300 es
precision highp float;

uniform sampler2D source;
uniform vec2 sourceSize;
uniform bool flip;
uniform vec2 crop;
in vec2 uv;
out vec4 color;
void main() {
  vec2 coord = ((vec2(uv.x, flip ? 1.0 - uv.y : uv.y)-0.5)*crop+0.5) * sourceSize;
  vec2 center = floor(coord - 0.5) + 0.5;
  vec2 f = coord - center;
  vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
  vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
  vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
  vec2 w3 = f * f * (-0.5 + 0.5 * f);
  vec2 w12 = w1 + w2;
  vec2 t0 = (center - 1.0) / sourceSize;
  vec2 t3 = (center + 2.0) / sourceSize;
  vec2 t12 = (center + w2 / w12) / sourceSize;
  vec3 c = vec3(0.0);
  c += texture(source, vec2(t0.x, t0.y)).rgb * w0.x * w0.y;
  c += texture(source, vec2(t12.x, t0.y)).rgb * w12.x * w0.y;
  c += texture(source, vec2(t3.x, t0.y)).rgb * w3.x * w0.y;
  c += texture(source, vec2(t0.x, t12.y)).rgb * w0.x * w12.y;
  c += texture(source, vec2(t12.x, t12.y)).rgb * w12.x * w12.y;
  c += texture(source, vec2(t3.x, t12.y)).rgb * w3.x * w12.y;
  c += texture(source, vec2(t0.x, t3.y)).rgb * w0.x * w3.y;
  c += texture(source, vec2(t12.x, t3.y)).rgb * w12.x * w3.y;
  c += texture(source, vec2(t3.x, t3.y)).rgb * w3.x * w3.y;
  color = vec4(clamp(c, 0.0, 1.0), 1.0);
}