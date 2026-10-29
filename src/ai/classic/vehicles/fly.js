import * as THREE from "three";
import { FlyGotoHelper as NavFlyGotoHelper } from "../nav/goto.js";
import { steerState } from "./steering.js";

// The Flyer movement helper (FlyGotoHelper). Nav's helper
// (src/ai/classic/nav/goto.js) already plans as the 2004 game does (a local
// path with the flyer mask, never a graph route) and takes direct two-point
// paths; this adds what the flyer agents and patterns need on top:
//
//   followPath(points, override, altitude)  an externally built path (state
//     4): the attack run and the landing approach. override/altitude are
//     the path flags the flight steering reads: override stops the avoidance
//     cache from interrupting the path, altitude keeps the path's own
//     heights.
//   land(on)  the landing request: with no path left, the flyer sets down.
//
// Altitude, throttle and avoidance are the flyer's AI input consumer's
// (steering.js), not the helper's.

export class FlyGotoHelper extends NavFlyGotoHelper {
  goto(point, options = {}) {
    this.setFlags(false, false);
    super.goto(point, options);
  }

  // points: [[x, y, z], ...] in travel order, the first normally the
  // flyer's own position.
  followPath(points, override = false, altitude = false) {
    const unit = this.unit;
    if (!unit || !points?.length) return false;
    const planner = this.nav?.planner;
    if (!planner) return false;
    planner.cancel(this.control);
    const path = planner.path();
    // Built back to front, as the 2004 game prepends its nodes.
    for (let i = points.length - 1; i >= 0; i--) {
      const [x, y, z] = points[i];
      if (!path.prepend(x, y, z)) {
        path.release();
        return false;
      }
    }
    const last = points[points.length - 1];
    this.destination.set(last[0], last[1], last[2]);
    this.control.install(path, override, altitude);
    this.setFlags(override, altitude);
    this.failed = false;
    this.state = 4;
    this.pending = 0;
    return true;
  }

  setFlags(override, altitude) {
    const unit = this.unit;
    if (!unit) return;
    const st = steerState(unit);
    st.pathOverride = override;
    st.pathAltitude = altitude;
  }

  land(on = true) {
    const unit = this.unit;
    if (unit) steerState(unit).land = on;
  }

  stop() {
    this.setFlags(false, false);
    super.stop();
  }
}

export { THREE };
