// Resolve the effect library to stable, numeric handles only at map load.
export function effectContent(library, gpu) {
  const definitions = [], groups = [[]], handles = new Map(), definitionHandles = new Map(), source = [];
  function group(defs) {
    if (handles.has(defs)) return handles.get(defs);
    const id = groups.length, rows = []; groups.push(rows); handles.set(defs, id);
    for (const def of defs) {
      let index = definitionHandles.get(def);
      if (index === undefined) {
        index = definitions.length; definitionHandles.set(def, index); source.push(def);
        definitions.push(null);
        const child = def.geometry.emitters?.length ? group(def.geometry.emitters) : 0;
        definitions[index] = {...def, child, gpu: gpu?.definitions.has(def) || false};
      }
      rows.push(index);
    }
    return id;
  }
  for (const defs of Object.values(library)) group(defs);
  return {data:{definitions,groups},handles,source};
}
