import * as THREE from "three";

// The sun's shadow cascades, as cached as they can be.
//
// The sun never moves, and the map (its buildings, walls, fences) never
// moves either. Each cascade's window onto the map is held in place as long
// as it still covers the cascade's slice of the view (a square the size of
// the slice's diagonal, snapped to its own texel grid, so a held window and a
// re-centred one share the same texels). The static casters are rendered
// into a cache the first frame a window lands, and only then; every other
// frame the cached depth is copied in and the few moving casters (soldiers,
// vehicles, held guns, debris) are drawn over it. The map the materials
// sample is the same either way, texel for texel.
//
// A window must move only when the slice leaves it, so the texel grid never
// drifts as the camera breathes. Holding keeps at least half the original
// light margin toward the sun, so tall casters up-sun still fall in.

export class CascadeWindow {
  constructor() {
    this.center = new THREE.Vector3(); // light space; z toward the sun
    this.half = 0; // half the window's width (light space)
    this.held = false;
    this.dirty = true; // the static cache must be rendered
    this.moved = false; // the window landed somewhere new this frame
  }

  // Does the held window still cover `bounds` (a light-space Box3 of the
  // slice) at this `half` width, keeping `margin` toward the sun?
  holds(bounds, half, margin) {
    if (!this.held || this.half !== half) return false;
    const c = this.center;
    return bounds.min.x >= c.x - half && bounds.max.x <= c.x + half
      && bounds.min.y >= c.y - half && bounds.max.y <= c.y + half
      && bounds.max.z + margin <= c.z;
  }

  // Land on `bounds`: its centre snapped to the texel grid, `lightMargin`
  // toward the sun (as three's CSM places it).
  land(bounds, half, texel, lightMargin) {
    bounds.getCenter(this.center);
    this.center.z = bounds.max.z + lightMargin;
    this.center.x = Math.floor(this.center.x / texel) * texel;
    this.center.y = Math.floor(this.center.y / texel) * texel;
    this.half = half;
    this.held = true;
    this.dirty = true;
    this.moved = true;
  }

  // Hold or land, per frame. Returns whether it landed.
  fit(bounds, half, texel, lightMargin) {
    if (this.holds(bounds, half, lightMargin * 0.5)) return false;
    this.land(bounds, half, texel, lightMargin);
    return true;
  }

  invalidate() {
    this.held = false;
    this.dirty = true;
  }
}

// What each cascade draws this frame.
//   static   the static casters into the cache (the window landed somewhere
//            new, or the cache was dropped)
//   dynamic  the cached depth, then the moving casters, into the live map
// The last cascade's moving casters (soldiers 100 m and further, a few
// texels of 30 cm each) are redrawn every other frame; its live map holds
// the frame before.
export function planCascades(windows, frame, { alternateLast = true } = {}) {
  return windows.map((window, i) => {
    const last = alternateLast && windows.length > 2 && i === windows.length - 1;
    return {
      static: window.dirty,
      dynamic: window.moved || window.dirty || !last || frame % 2 === 0,
    };
  });
}
