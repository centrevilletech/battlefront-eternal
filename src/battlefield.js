import { Boundary } from "./ai/classic/nav/boundary.js";

// Leaving the battlefield, as the 2004 game has it (docs/formats.md,
// "Leaving the battlefield"). The battlefield is the map's bnd_ boundary
// (src/ai/classic/nav/boundary.js): a point is outside when no polygon holds
// it, in x and z only; a map without one (both Yavin maps) has no outside.
//
// Each Character's update (run while he has a body) tests what he
// controls: his body on foot, the hull or turret he sits in, and the recon
// droid he flies. While none is outside his countdown is -1. When one goes
// out with the countdown below 0 it starts at 10 s, and for a player at this
// machine his side's SetOutOfBoundsVoiceOver line plays, once. Each update
// takes the time off; once it is below 0 each thing still outside is killed
// outright, nobody credited: his body, a hull with everyone
// aboard, a droid. Back inside, even for a moment, and the next time out
// starts again from 10 s. It is the same for the player, the bots and
// network humans, heroes included.
//
// While a player is out, "LEAVING BATTLEFIELD" (core.lvl's
// game.tooltips.leaving_battlefield) flashes in the middle of his screen in
// gamefont_large, in the HUD's red. No time is shown. Its alpha is a spring:
// it starts at 254 and is pulled toward 128 at 500/s^2 (a 0.28 s beat), drawn
// between 2 and 255. The page works the warning out from what he
// controls, as it sees it, in every mode; the world only keeps the count.

export const LEAVING_TIME = 10; // s

// The map's boundary from its ai-nav.json: a polygon per Boundary path the
// bnd_ config names (missing paths are skipped, as in the 2004 game).
export function boundaryFromNav(json) {
  const polygons = [];
  for (const ref of json?.boundary?.references ?? []) {
    const path = Number.isInteger(ref?.path) ? json.paths?.[ref.path] : null;
    if (path?.points?.length) polygons.push(path.points);
  }
  return new Boundary(polygons);
}

// The boundary of a map's ai-nav.json at url; none if it can't be read.
export async function loadBoundary(url) {
  try {
    const response = await fetch(url);
    return response.ok ? boundaryFromNav(await response.json()) : new Boundary();
  } catch {
    return new Boundary();
  }
}

// The recon droid he flies, if any.
function flown(unit) {
  const droid = unit.combat?.remotes?.piloted;
  return droid && !droid.dead ? droid : null;
}

export class Battlefield {
  constructor(boundary = new Boundary()) {
    this.boundary = boundary;
  }

  // Whether a place is outside the battlefield.
  outside(position) {
    return this.boundary.outside(position);
  }

  // Whether anything he controls is outside: the hull or turret he sits in,
  // else his body, and a droid he flies. hull, droid: what the caller knows
  // him to be in (the page's own view); by default his unit's.
  controlsOutside(unit, hull = unit?.seat?.vehicle ?? unit?.seat?.turret ?? null, droid = unit ? flown(unit) : null) {
    if (!unit || this.boundary.empty) return false;
    return this.outside(hull ? hull.position : unit.position) || Boolean(droid && this.outside(droid.position));
  }

  // Every Character's countdown, once a simulation step.
  update(dt, characters) {
    if (this.boundary.empty) return;
    for (const character of characters) {
      const unit = character.unit;
      // Only while he has a body: the original game lets go of it at his death.
      if (!unit || unit.removed || !unit.alive || unit.combat?.dead) continue;
      const hull = unit.seat?.vehicle ?? unit.seat?.turret ?? null;
      const droid = flown(unit);
      const bodyOut = this.outside(hull ? hull.position : unit.position);
      const droidOut = Boolean(droid && this.outside(droid.position));
      if (!bodyOut && !droidOut) {
        character.outOfBounds = -1;
        continue;
      }
      if (!(character.outOfBounds >= 0)) character.outOfBounds = LEAVING_TIME;
      character.outOfBounds -= dt;
      if (!(character.outOfBounds < 0)) continue;
      if (droidOut) unit.combat.remotes.kill(droid);
      if (!bodyOut) continue;
      if (unit.seat?.vehicle) {
        // The hull goes, and everyone aboard with it.
        const vehicle = unit.seat.vehicle;
        vehicle.lastAttacker = null;
        vehicle.damage(Infinity);
      }
      if (!unit.combat.dead) unit.combat.damage(Infinity);
    }
  }
}

// The player's warning, on the page that shows him: whether it shows, and
// its alpha (0 hidden). The spring takes a 1/60 s step for each world tick
// the page has shown since (at most a second's), so a page that looks more
// than once a tick flashes no faster.
export class LeavingWarning {
  constructor() {
    this.shown = false;
    this.value = 0;
    this.rate = 0;
    this.tick = 0;
  }

  get alpha() {
    if (!this.shown) return 0;
    return Math.min(Math.max(Math.trunc(this.value), 2), 255) / 255;
  }

  // tick: the world tick this page shows; outside: whether what he controls
  // is outside now (alive). Returns true when he has just gone out: the
  // voice-over's moment.
  update(tick, outside) {
    if (!outside) {
      this.shown = false;
      return false;
    }
    const started = !this.shown;
    let steps = Math.min(Math.max(tick - this.tick, 0), 60);
    if (started) {
      this.shown = true;
      this.value = 254;
      this.rate = 0;
      steps = 1;
    }
    this.tick = tick;
    for (let i = 0; i < steps; i++) {
      this.value += this.rate / 60;
      this.rate += (128 - this.value) * 500 / 60;
    }
    return started;
  }
}
