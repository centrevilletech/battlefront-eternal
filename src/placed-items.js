import * as THREE from "three";

// Power-ups the world places (props.json `items`, tools/zero_placed_items.py):
// Yavin 4: Arena's one com_item_powerup_health100. The 2004 game creates
// every world instance through its class, so a placed power-up is an
// ordinary item from the battle's start (src/items.js): it settles under its
// Gravity, anyone touching it takes it, and it lasts its class's LifeSpan
// like any other (the item update compares its age with the class's
// LifeSpan; nothing exempts a placed one). The world spawns them
// (a page shows them from the item records, as any item);
// docs/live-objects.md "Placed power-ups".
const matrix = new THREE.Matrix4();

export function spawnPlacedItems(items, placed = []) {
  const made = [];
  for (const entry of placed || []) {
    if (!entry?.item || !Array.isArray(entry.matrix) || entry.matrix.length !== 16) continue;
    matrix.fromArray(entry.matrix);
    const position = new THREE.Vector3().setFromMatrixPosition(matrix);
    // Its heading: the instance's forward (z) axis about up.
    const yaw = Math.atan2(matrix.elements[8], matrix.elements[10]);
    const item = items.spawn(entry.item, position, new THREE.Vector3(), yaw, { team: entry.team || 0 });
    if (item) made.push(item);
  }
  return made;
}
