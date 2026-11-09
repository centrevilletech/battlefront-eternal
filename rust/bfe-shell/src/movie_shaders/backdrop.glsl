#version 300 es
precision highp float;
in vec2 uv;
out vec4 color;
void main() {
    float r = length((uv-vec2(0.4,0.45))*vec2(1.0,1.0));
    vec3 inner = vec3(12,40,88)/255.0;
    vec3 middle = vec3(6,21,49)/255.0;
    vec3 outer = vec3(1,4,12)/255.0;
    color = vec4(r < 0.48 ? mix(inner,middle,r/0.48) : mix(middle,outer,clamp((r-0.48)/0.52,0.0,1.0)),1.0);
}
