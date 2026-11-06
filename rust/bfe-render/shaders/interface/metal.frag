// HoloMap (metal): Faceted (each face's own normal), lit from above and the front, its
// edges in the hologram's light.

  uniform vec3 color;
  uniform vec3 rim;
  varying vec3 vView;
  void main() {
    vec3 n = normalize(cross(dFdx(vView), dFdy(vView)));
    vec3 toEye = normalize(-vView);
    float key = max(dot(n, normalize(vec3(-0.4, 0.9, 0.5))), 0.0);
    float edge = pow(1.0 - max(dot(n, toEye), 0.0), 2.0);
    vec3 c = color * (0.45 + 0.9 * key) + rim * edge * 0.55;
    gl_FragColor = vec4(c, 1.0);
  }
