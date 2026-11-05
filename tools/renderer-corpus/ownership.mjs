// Associate expanded programs with scene materials and the Rust passes that
// registered them. A source marker alone cannot identify far-scene ownership.
export async function programOwnership() {
  const g = window.__bf.graphics, renderer = g.renderer, gl = renderer.getContext();
  const materials = new Map(), registrations = [];
  const describe = (material, object, scene, pass) => {
    if (!material) return;
    let row = materials.get(material);
    if (!row) {
      row = { id: material.id, name: material.name, type: material.type,
        defines: material.defines || {}, alphaToCoverage: material.alphaToCoverage,
        side: material.side, transparent: material.transparent, objects: [], programs: [] };
      const properties = renderer.properties.get(material);
      const programs = new Set([properties.currentProgram, ...(properties.programs?.values() || [])]);
      for (const p of programs) if (p) row.programs.push(p.cacheKey);
      materials.set(material, row);
    }
    if (object) row.objects.push({ scene, pass, type: object.type, name: object.name,
      receiveShadow: object.receiveShadow, castShadow: object.castShadow,
      attributes: Object.keys(object.geometry?.attributes || {}).sort() });
  };
  for (const [name, scene] of [['near', g.scene], ['far', g.farScene], ['view', g.viewScene]]) {
    scene?.traverse(object => {
      for (const material of Array.isArray(object.material) ? object.material : [object.material]) describe(material, object, name, null);
      if (object.customDepthMaterial) describe(object.customDepthMaterial, object, name, 'shadow');
    });
  }
  const passes = g.rust.byName;
  for (const entry of passes.world?.meshes || []) describe(entry.mesh.material, entry.mesh, 'near', 'world');
  for (const entry of passes.terrain?.grounds || []) describe(entry.material, entry.group, 'near', 'terrain');
  for (const [kind, entry] of passes.foliage?.classes || []) if (entry) describe(kind.batch?.material, kind.batch, 'near', 'foliage');
  for (const entry of passes.far?.all || []) {
    describe(entry.material, entry.object, 'far', 'far');
    registrations.push({ pass: 'far', material: entry.material.id, key: entry.threeProgram?.cacheKey,
      rustProgram: entry.program?.id, terrain: Boolean(entry.terrain), placed: entry.placed });
  }
  const shaders = {}, programs = [];
  const hash = async text => [...new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text)))].map(v => v.toString(16).padStart(2, '0')).join('');
  for (const p of renderer.info.programs || []) {
    const stages = {};
    for (const shader of gl.getAttachedShaders(p.program) || []) {
      const source = gl.getShaderSource(shader), digest = await hash(source);
      shaders[digest] = source;
      stages[gl.getShaderParameter(shader, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? 'vertex' : 'fragment'] = digest;
    }
    const attributes = [], uniforms = [];
    for (let i = 0; i < gl.getProgramParameter(p.program, gl.ACTIVE_ATTRIBUTES); i++) {
      const a = gl.getActiveAttrib(p.program, i);
      attributes.push({ name: a.name, type: a.type, size: a.size, location: gl.getAttribLocation(p.program, a.name) });
    }
    for (let i = 0; i < gl.getProgramParameter(p.program, gl.ACTIVE_UNIFORMS); i++) {
      const u = gl.getActiveUniform(p.program, i); uniforms.push({ name: u.name, type: u.type, size: u.size });
    }
    programs.push({ key: p.cacheKey, stages, attributes, uniforms,
      materials: [...materials.values()].filter(m => m.programs.includes(p.cacheKey)).map(m => m.id) });
  }
  const debug = gl.getExtension('WEBGL_debug_renderer_info');
  return { materials: [...materials.values()], registrations, programs, shaders,
    backend: debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    multiDraw: Boolean(gl.getExtension('WEBGL_multi_draw')), samples: g.sceneTarget.samples, glError: gl.getError() };
}
