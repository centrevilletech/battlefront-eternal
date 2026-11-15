  void main() {
    gl_FragColor = vec4(nearestMotion(ivec2(gl_FragCoord.xy)), 1.0);
  }
