#version 300 es
precision highp float;
uniform sampler2D map;
uniform sampler2D sceneColor;
uniform vec2 blurSettings;
in vec2 vUv;
in vec4 vColor;
in vec4 vRefractClip;
out vec4 fragColor;

void main() {
    vec4 mask = texture(map, vUv);
    float alpha = mask.a * vColor.a;
    if (alpha < 0.002) {
        discard;
    }
    vec2 sceneSize = vec2(textureSize(sceneColor, 0));
    vec2 uv = clamp((vRefractClip.xy / max(vRefractClip.w, 1e-5)) * 0.5 + 0.5, vec2(0.0), vec2(1.0));
    float copyLod = max(log2(max(max(sceneSize.x, sceneSize.y) / 256.0, 1.0)), 0.0);
    float lod = max(copyLod, max(blurSettings.y, 0.0));
    vec3 color = textureLod(sceneColor, uv, lod).rgb * vColor.rgb;
    fragColor = vec4(color, alpha);
}
