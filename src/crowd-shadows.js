import * as THREE from "three";

// The bots' shadows in one draw per cascade. Every bot's body was a skinned
// shadow proxy of its own (actor.js) and every held weapon a mesh of its own:
// at 30 soldiers in shadow range that was 60 skinned draws per cascade. This
// mesh holds all of them: each slot's body vertices skinned to its own bones,
// and each of its weapon models skinned to its hand bone, in one skeleton
// whose bones are the bodies' own Bone objects (so the frame's one matrix
// update poses them). A slot is hidden by zeroing its bone inverses: its
// vertices collapse to the origin and draw nothing. The depth is what the
// proxies drew: the same bones, the same inverses, with the bind matrices
// identity instead of a root transform and its inverse.
//
// Capacity grows by rebuilding the geometry and skeleton; slots are only
// ever appended, so a body's bone and vertex offsets never change.

const ZERO = new THREE.Matrix4().set(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
const IDENTITY = new THREE.Matrix4();
const GROW = 1.5;

// The meshes a held weapon casts with: the model's parts (zero:dynamic, from
// modelGroup), not its muzzle flash or a blade's glow.
function weaponMeshes(group) {
  const meshes = [];
  for (const child of group.children) if (child.isMesh && child.geometry?.attributes.position && child.material?.name === "zero:dynamic") meshes.push(child);
  return meshes;
}

export class CrowdShadows {
  constructor(graphics) {
    this.graphics = graphics;
    this.slots = [];
    this.bones = []; // Bone objects, one entry per skeleton slot
    this.inverses = []; // their bind inverses (ZERO while hidden)
    this.shown = []; // per entry: whether its inverse is the bind one
    this.vertexCount = 0;
    this.indexCount = 0;
    this.vertexCapacity = 0;
    this.indexCapacity = 0;
    this.boneCapacity = 0;
    this.mesh = null;
    this.skeleton = null;
    this.position = null;
    this.skinIndex = null;
    this.skinWeight = null;
    this.index = null; // every slot's indices, in slot order
    this.drawn = null; // the shown slots' indices, packed (what is drawn)
    this.repack = false;
    this.matrix = new THREE.Matrix4();
    this.point = new THREE.Vector3();
    this.draws = 0;
  }

  // Register a unit's body (and its held weapons) once; returns its slot.
  // unit: { actor, combat }, with actor.bones, actor.skeleton, actor.body,
  // actor.hand and combat.held (name -> { group }).
  add(unit) {
    const actor = unit.actor;
    if (!actor?.body?.geometry || !actor.skeleton) return null;
    const body = actor.body.geometry;
    const bodyVerts = body.attributes.position.count;
    const bodyIndices = body.index ? body.index.count : bodyVerts;
    const weapons = [];
    for (const held of unit.combat?.held?.values?.() ?? []) {
      const meshes = weaponMeshes(held.group);
      if (!meshes.length || !actor.hand) continue;
      let verts = 0, indices = 0;
      for (const mesh of meshes) { verts += mesh.geometry.attributes.position.count; indices += mesh.geometry.index ? mesh.geometry.index.count : mesh.geometry.attributes.position.count; }
      weapons.push({ group: held.group, meshes, verts, indices, entry: -1, shown: false });
    }
    const slot = {
      unit, actor, weapons,
      boneBase: this.bones.length, boneCount: actor.bones.length,
      vertexStart: this.vertexCount, vertexCount: bodyVerts + weapons.reduce((n, w) => n + w.verts, 0),
      indexStart: this.indexCount, indexCount: bodyIndices + weapons.reduce((n, w) => n + w.indices, 0),
      shown: false,
    };
    for (let i = 0; i < actor.bones.length; i++) { this.bones.push(actor.bones[i]); this.inverses.push(ZERO); this.shown.push(false); }
    for (const weapon of weapons) { weapon.entry = this.bones.length; this.bones.push(actor.hand); this.inverses.push(ZERO); this.shown.push(false); }
    // The body's own proxy and the guns' meshes no longer cast (nor come
    // back when the shadow preset changes): this mesh does.
    const retire = (mesh) => { if (!mesh) return; mesh.castShadow = false; this.graphics.casters?.delete?.(mesh); };
    retire(actor.shadow);
    for (const weapon of weapons) for (const mesh of weapon.meshes) retire(mesh);
    this.slots.push(slot);
    this.vertexCount += slot.vertexCount;
    this.indexCount += slot.indexCount;
    if (this.vertexCount > this.vertexCapacity || this.indexCount > this.indexCapacity || this.bones.length > this.boneCapacity) this.rebuild();
    else { this.fill(slot); this.rebindSkeleton(); }
    return slot;
  }

  // Everything again at a larger capacity.
  rebuild() {
    this.vertexCapacity = Math.max(this.vertexCapacity, Math.ceil(this.vertexCount * GROW), 1024);
    this.indexCapacity = Math.max(this.indexCapacity, Math.ceil(this.indexCount * GROW), 1024);
    this.boneCapacity = Math.max(this.boneCapacity, Math.ceil(this.bones.length * GROW), 32);
    this.position = new Float32Array(this.vertexCapacity * 3);
    this.skinIndex = new Uint16Array(this.vertexCapacity * 4);
    this.skinWeight = new Float32Array(this.vertexCapacity * 4);
    this.index = new Uint32Array(this.indexCapacity);
    this.drawn = new Uint32Array(this.indexCapacity);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(this.position, 3));
    geometry.setAttribute("skinIndex", new THREE.BufferAttribute(this.skinIndex, 4));
    geometry.setAttribute("skinWeight", new THREE.BufferAttribute(this.skinWeight, 4));
    geometry.setIndex(new THREE.BufferAttribute(this.drawn, 1).setUsage(THREE.DynamicDrawUsage));
    for (const slot of this.slots) this.fill(slot, false);
    geometry.setDrawRange(0, 0);
    this.repack = true;
    const old = this.mesh;
    if (old) {
      old.geometry.dispose();
      old.skeleton.boneTexture?.dispose();
      old.parent?.remove(old);
      this.graphics.casters?.delete?.(old);
    }
    // Front faces into the depth map, as the bodies' materials cast
    // (three's default for a plain material is the back faces).
    const mesh = new THREE.SkinnedMesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.FrontSide, shadowSide: THREE.FrontSide }));
    mesh.name = "crowd-shadows";
    mesh.frustumCulled = false;
    mesh.receiveShadow = false;
    mesh.layers.set(this.graphics.hiddenLayer ?? 1);
    this.mesh = mesh;
    this.rebindSkeleton();
    this.graphics.scene.add(mesh);
    this.graphics.caster(mesh);
  }

  // The skeleton over every entry so far (its bone count grows with the
  // slots; three sizes the bone texture from it when it next draws).
  rebindSkeleton() {
    const mesh = this.mesh;
    if (!mesh) return;
    if (mesh.skeleton?.boneTexture) mesh.skeleton.boneTexture.dispose();
    this.skeleton = new THREE.Skeleton(this.bones, this.inverses.map((m) => m.clone()));
    mesh.bind(this.skeleton, IDENTITY);
    // Fresh inverses (all hidden): update() shows what is shown again.
    for (const slot of this.slots) { slot.shown = false; for (const weapon of slot.weapons) weapon.shown = false; }
  }

  // A slot's vertices into the arrays: the body as its geometry has them,
  // each weapon's parts in the hand's frame.
  fill(slot, upload = true) {
    const { actor } = slot;
    const body = actor.body.geometry;
    const source = body.attributes.position, skin = body.attributes.skinIndex;
    let v = slot.vertexStart, k = slot.indexStart;
    const vertexBase = v;
    for (let i = 0; i < source.count; i++, v++) {
      this.position[v * 3] = source.getX(i); this.position[v * 3 + 1] = source.getY(i); this.position[v * 3 + 2] = source.getZ(i);
      this.skinIndex[v * 4] = slot.boneBase + skin.getX(i); this.skinIndex[v * 4 + 1] = this.skinIndex[v * 4 + 2] = this.skinIndex[v * 4 + 3] = 0;
      this.skinWeight[v * 4] = 1; this.skinWeight[v * 4 + 1] = this.skinWeight[v * 4 + 2] = this.skinWeight[v * 4 + 3] = 0;
    }
    if (body.index) for (let i = 0; i < body.index.count; i++) this.index[k++] = vertexBase + body.index.getX(i);
    else for (let i = 0; i < source.count; i++) this.index[k++] = vertexBase + i;
    slot.bodyIndexCount = k - slot.indexStart;
    for (const weapon of slot.weapons) {
      weapon.indexStart = k;
      for (const mesh of weapon.meshes) {
        // The part's place under the hand: the group's and its own transforms.
        mesh.updateMatrix(); weapon.group.updateMatrix();
        this.matrix.multiplyMatrices(weapon.group.matrix, mesh.matrix);
        const p = mesh.geometry.attributes.position, base = v;
        for (let i = 0; i < p.count; i++, v++) {
          this.point.set(p.getX(i), p.getY(i), p.getZ(i)).applyMatrix4(this.matrix);
          this.position[v * 3] = this.point.x; this.position[v * 3 + 1] = this.point.y; this.position[v * 3 + 2] = this.point.z;
          this.skinIndex[v * 4] = weapon.entry; this.skinIndex[v * 4 + 1] = this.skinIndex[v * 4 + 2] = this.skinIndex[v * 4 + 3] = 0;
          this.skinWeight[v * 4] = 1; this.skinWeight[v * 4 + 1] = this.skinWeight[v * 4 + 2] = this.skinWeight[v * 4 + 3] = 0;
        }
        const index = mesh.geometry.index;
        if (index) for (let i = 0; i < index.count; i++) this.index[k++] = base + index.getX(i);
        else for (let i = 0; i < p.count; i++) this.index[k++] = base + i;
      }
      weapon.indexCount = k - weapon.indexStart;
    }
    if (!upload || !this.mesh) return;
    const geometry = this.mesh.geometry;
    for (const [name, size] of [["position", 3], ["skinIndex", 4], ["skinWeight", 4]]) {
      const attribute = geometry.getAttribute(name);
      attribute.addUpdateRange(slot.vertexStart * size, slot.vertexCount * size);
      attribute.needsUpdate = true;
    }
  }

  // The drawn index buffer: the shown bodies' and held weapons' ranges,
  // packed, so hidden and distant slots cost no vertex work. Uploaded when
  // the shown set changes (a soldier crossing shadow range, a weapon swap).
  pack() {
    this.repack = false;
    const drawn = this.drawn, index = this.index;
    let n = 0;
    for (const slot of this.slots) {
      if (!slot.shown) continue;
      drawn.set(index.subarray(slot.indexStart, slot.indexStart + slot.bodyIndexCount), n);
      n += slot.bodyIndexCount;
      for (const weapon of slot.weapons) {
        if (!weapon.shown) continue;
        drawn.set(index.subarray(weapon.indexStart, weapon.indexStart + weapon.indexCount), n);
        n += weapon.indexCount;
      }
    }
    const geometry = this.mesh.geometry;
    geometry.index.clearUpdateRanges();
    if (n) { geometry.index.addUpdateRange(0, n); geometry.index.needsUpdate = true; }
    geometry.setDrawRange(0, n);
    this.drawnIndices = n;
  }

  // Each frame, before the render: which slots and weapons draw. A body
  // draws when it is shown and within shadow range (unit.shadowNear, set by
  // units.js); a weapon when its body does and the hand holds it.
  update() {
    const skeleton = this.skeleton;
    if (!skeleton) return;
    const inverses = skeleton.boneInverses;
    let drawn = 0;
    for (const slot of this.slots) {
      const { actor, unit } = slot;
      const shown = Boolean(actor.root.visible && actor.body.visible && unit.shadowNear && !unit.removed);
      if (shown !== slot.shown) {
        slot.shown = shown;
        this.repack = true;
        const bind = actor.skeleton.boneInverses;
        for (let i = 0; i < slot.boneCount; i++) inverses[slot.boneBase + i].copy(shown ? bind[i] : ZERO);
      }
      if (shown) drawn++;
      for (const weapon of slot.weapons) {
        const on = shown && weapon.group.visible;
        if (on !== weapon.shown) { weapon.shown = on; this.repack = true; inverses[weapon.entry].copy(on ? IDENTITY : ZERO); }
      }
    }
    this.draws = drawn;
    if (this.repack) this.pack();
    this.mesh.castShadow = drawn > 0 && Boolean(this.graphics.csm);
  }
}
