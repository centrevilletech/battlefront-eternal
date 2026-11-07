// Water surfaces, rings, shores and crest spray are native effect resources.
// The transparent phase still seals the water bank for recorded frames; no
// browser geometry/material descriptor is inspected or uploaded here.
export function waterPass() {
  return { name: "water", phase: "transparent", draw() {} };
}
