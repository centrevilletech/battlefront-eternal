import * as THREE from "three";
import { modelGroup, modelPack } from "../models.js";

// The common hologram pack (common.json's holograms, with holograms.bin):
// the side logos com_icon_republic, _cis, _imperial and _alliance and the
// shell_cube room, which interface screens place, colour and fade.

// A fresh copy of a pack's model: each part its own unlit, transparent,
// double-sided material (added where the part adds), listed in
// userData.materials; each mesh's texture scroll (MTRL, per second) in its
// userData.scroll. texture(name): the part's texture. Null for a name the
// pack lacks.
export function commonModel(pack, name, texture) {
  const parts = pack?.get(String(name).toLowerCase());
  if (!parts) return null;
  const materials = [];
  const group = modelGroup(parts, (part) => {
    const material = new THREE.MeshBasicMaterial({
      map: texture(part.texture),
      transparent: true,
      blending: part.additive || (part.flags ?? 0) & 0x80 ? THREE.AdditiveBlending : THREE.NormalBlending,
      depthWrite: false,
      side: THREE.DoubleSide,
    });
    materials.push(material);
    return material;
  });
  group.children.forEach((mesh, i) => {
    mesh.userData.scroll = parts[i].scroll || null;
  });
  group.name = String(name);
  group.userData.materials = materials;
  group.userData.material = materials[0] || null;
  return group;
}

// The pack for a page with no battle loaded (the front end): fetches it
// and returns model(name) as above. common: common.json; texture(name): the
// surface's textures.
export async function loadCommonModels({ common, texture, root = "/common" }) {
  const [buffer, index] = await Promise.all([
    fetch(`${root}/holograms.bin`).then((response) => response.arrayBuffer()),
    fetch(`${root}/holograms_index.bin`).then((response) => response.arrayBuffer()),
  ]);
  const pack = modelPack(common.holograms, buffer, index);
  return (name) => commonModel(pack, name, texture);
}
