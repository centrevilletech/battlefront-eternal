import { lighting } from "./materials.js";

// Leafpatch/grasspatch cards use Three's standard cutout shader. Feed them
// the same map atmosphere as the world instead of leaving them full-contrast
// against distant geometry. Alpha-to-coverage smooths their edge samples on
// the MSAA target while retaining real depth occlusion.
export function foliageAtmosphere(material) {
  material.alphaToCoverage = true;
  material.onBeforeCompile = (shader) => {
    shader.uniforms.foliageFogRange = lighting.fogRange;
    shader.uniforms.foliageFogColor = lighting.fogColor;
    shader.vertexShader = "varying float foliageDepth;\n" + shader.vertexShader;
    shader.vertexShader = shader.vertexShader.replace("#include <project_vertex>", "#include <project_vertex>\nfoliageDepth = -mvPosition.z;");
    shader.fragmentShader = "uniform vec2 foliageFogRange;\nuniform vec3 foliageFogColor;\nvarying float foliageDepth;\n" + shader.fragmentShader;
    shader.fragmentShader = shader.fragmentShader.replace("#include <opaque_fragment>", `
      float foliageFog = clamp((foliageDepth - foliageFogRange.x) / max(foliageFogRange.y - foliageFogRange.x, 1.0), 0.0, 1.0);
      outgoingLight = mix(outgoingLight, foliageFogColor, foliageFog);
      #include <opaque_fragment>
    `);
  };
  material.customProgramCacheKey = () => "foliage-atmosphere-v1";
  return material;
}
