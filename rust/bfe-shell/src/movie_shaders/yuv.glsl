#version 300 es
precision highp float;
uniform sampler2D planeY;
uniform sampler2D planeU;
uniform sampler2D planeV;
in vec2 uv;
out vec4 color;
void main() {
    // Keep the output top-down for the interface's ordinary [0,0,1,1] UVs.
    float y = (texture(planeY, uv).r - 16.0/255.0) * (255.0/219.0);
    float u = (texture(planeU, uv).r - 128.0/255.0) * (255.0/224.0);
    float v = (texture(planeV, uv).r - 128.0/255.0) * (255.0/224.0);
    color = vec4(clamp(vec3(y + 1.402*v, y - 0.344136*u - 0.714136*v, y + 1.772*u), 0.0, 1.0), 1.0);
}
