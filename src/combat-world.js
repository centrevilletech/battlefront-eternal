import { repairLockKey } from "./combat.js";
import { botLockTarget, lockPick } from "./weapon-targeting.js";
import { meleeStrike } from "./heroes.js";

// What a soldier's combat (src/combat.js PlayerCombat, with its weapons,
// src/weapons.js) asks of the rest of the battle: every read of state it
// does not own (the collision world, the units and the AI's targets, the
// live props a repair tool works on, the recon droids, the battle's
// lightning) and every call into code that is not its own (the rounds it
// fires, the items it throws, a melee swing's strike, what an arc does to
// what it links, and the battle's hooks: a shot, a hurt, a death, a droid
// blown apart, a body back, the presentation events a world sends its
// pages). Combat's own logic reaches the battle through these calls and the
// body it fights with, in the order it makes them, so this is the boundary
// the simulation's port to Rust moves it across (docs/sim-store.md): the
// same calls are the trait the Rust combat reaches the battle through
// (rust/sim/src/combat/world.rs), what the replay harness records of each
// frame of combat's (tools/sim-capture/combat.mjs), and, once Rust owns
// combat, what its module calls back into JavaScript until the rest of the
// battle has moved too.
//
// What only draws (the weapon in the hand, muzzle flashes and their lights,
// a charge's glow, the first-person view and HUD, the animator's clips,
// sounds) is not here: a world draws nothing, and a page draws a soldier
// from his records.
export class CombatWorld {
  constructor(combat) {
    this.combat = combat;
    // The arc's targets as arcTargets() last gave them (hurtArcTarget).
    this.arcList = [];
  }

  // The collision world's cast (src/collision.js), into `out`.
  cast(origin, direction, far, out, ignoreBody, staticOnly) {
    return this.combat.collision.cast(origin, direction, far, out, ignoreBody, staticOnly);
  }

  // The first unit along the crosshair's ray (Units.raycast; the page's).
  raycastUnits(origin, direction, far, out, filter) {
    return this.combat.ordnance?.units?.raycast?.(origin, direction, far, null, out, filter, true) ?? null;
  }

  // Whether a remote weapon's launch is admitted (a network player's recon
  // droids are counted by the world, src/runtime/network-remote-world.js):
  // true, false, or the admission's token.
  admitRemote(weapon) {
    return this.combat.remotes?.admitLaunch?.(weapon) ?? true;
  }

  // His recon droids (src/remote.js): the live one a remote weapon
  // launched, flying it again, blowing it up, and a launch.
  remoteOf(weapon) {
    return this.combat.remotes?.of(weapon) || null;
  }

  remoteEnter(droid) {
    this.combat.remotes.enter(droid);
  }

  remoteKill(droid) {
    this.combat.remotes.kill(droid);
  }

  remoteLaunch(weapon, origin, velocity, yaw, team, admission) {
    this.combat.remotes?.launch(weapon, origin, velocity, yaw, team, admission);
  }

  // A repair tool's lock (src/props.js lockOn) and its plain key
  // (repairLockKey), and a tick of its work (repair).
  lockOn(eye, dir, def, current, held, team) {
    const live = this.combat.live;
    return live ? live.lockOn(eye, dir, def, current, held, team) : null;
  }

  lockKey(target) {
    return repairLockKey(this.combat.live, target);
  }

  repair(def, target, team) {
    return this.combat.live?.repair(def, target, team);
  }

  // What a launcher locks onto along a view (src/weapon-targeting.js
  // lockPick): a bot's controller's target, or what his view's cone finds
  // for a human; undefined when there is nothing to lock with.
  lockPick(weapon, origin, dir) {
    const c = this.combat;
    return lockPick(weapon, { units: c.ordnance?.units, collision: c.collision, origin, dir, viewer: c.owner, target: botLockTarget(c.owner) });
  }

  // A round fired (Ordnance.fire), its muzzle bound for the page that
  // draws it when `bind` (combat.js bindRoundMuzzle).
  fireRound(def, from, dir, shot, bind, selected = null) {
    const c = this.combat;
    const round = c.ordnance.fire(def, from, dir, c.owner, null, null, null, shot);
    if (round && bind) c.bindRoundMuzzle(round, shot.weapon, selected);
  }

  // A dispenser's item thrown (Items.dispense): whether there was room.
  dispense(className, most, def, origin, velocity, yaw, owner, team) {
    return this.combat.items?.dispense(className, most, def, origin, velocity, yaw, owner, team);
  }

  // A melee swing's strike (src/heroes.js meleeStrike).
  meleeStrike() {
    meleeStrike(this.combat);
  }

  // What the arc caster's lightning can link (the battle's arcTargets), as
  // plain views of each target, and a link's damage to one of them.
  arcTargets() {
    const list = this.combat.arcTargets(this.combat.owner);
    this.arcList = list;
    return list.map((target) => ({ origin: target.origin, center: target.center, radius: target.radius }));
  }

  hurtArcTarget(index, amount, from) {
    this.arcList[index].hurt(amount, from);
  }

  // The battle's lightning bolt of a name (effects.json), if it has one.
  lightning(name) {
    return this.combat.effects?.lightning?.[name];
  }

  // The battle's hooks: a shot, a hurt, a death, a droid blown apart at its
  // death, the respawn timer run out, and a presentation event for pages.
  fired(shot) {
    this.combat.onFired?.(shot);
  }

  damaged(amount, attacker, info) {
    this.combat.onDamaged(amount, attacker, info);
  }

  died() {
    this.combat.onDeath();
  }

  blownApart(health) {
    this.combat.onBlowApart(health);
  }

  respawn() {
    this.combat.onRespawn();
  }

  presentation(event) {
    this.combat.onPresentation?.(event);
  }
}
