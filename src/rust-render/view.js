// The view pass (view.rs): the first-person view scene, the weapon in view
// drawn last over the near scene in a depth range of its own, read with its
// own camera's frame block (VIEW_FIRST_PERSON). Empty for now: three draws
// it.
export function viewPass() {
  return { name: "view", phase: "view" };
}
