import * as THREE from "three";

// The Classic AI scheduler and the per-unit AI input step
// (docs/ai-controller.md).
//
// Every simulation step the scheduler advances a cycle counter (cycle length
// C: 3 on Hard, 15 otherwise) and toggles a parity bit. When the counter
// passes C it restarts at 1: the vehicle notifier runs, every human source
// broadcasts its acoustic contacts, the protected-human lists are rebuilt,
// and every controller's "processed" flag clears. Then it considers up to a
// budget of unprocessed controllers in collection order: floor(N/C), plus one
// when C - t < N mod C, and on the final step of the cycle (t = C) all of
// them, which drains whoever is left. A considered controller is marked
// processed even when the distance/visibility gate skips its work. The work
// for a serviced controller is: phase 0, the deferred-LOS cleanup, the sight
// scan and the acoustic broadcast (Combat), phase 1, then clearing the sight
// candidate. After the loop, the deferred line-of-sight service gets its
// budget (12 on Hard, 6 otherwise), doubled when the path planner is idle;
// the planner gets a 1.0 slice before LOS when it is busy and another after
// LOS when LOS work leaves capacity.
//
// Separately, every AI unit's input runs every step (the original's AIThread):
// it may itself skip a step under the same kind of visibility gate, carrying
// that step's dt into the next execution. Executing means: the fire-request
// consumer (Combat), then the movement and aim analogs (Nav) from the
// controller's request and aim state, then posture/jet/droideka requests,
// all written into unit.intent.

const _toCamera = new THREE.Vector3();
const _axis = new THREE.Vector3();
const _aim = new THREE.Vector3();
const _forward = new THREE.Vector3();

// Candidate budget for N controllers at cycle step t of C.
export function candidateBudget(count, cycle, step) {
  if (step === cycle) return count;
  return Math.floor(count / cycle) + (cycle - step < count % cycle ? 1 : 0);
}

// Cycle length and base deferred-LOS budget by tier.
export function schedulerConfig(hard) {
  return hard ? { cycle: 3, losBudget: 12 } : { cycle: 15, losBudget: 6 };
}

// Whether a camera's view frustum holds `point`: depth between near and
// min(far, 8 * scale + 70), and lateral offsets inside the side slopes at
// that depth (inclusive). `scale` is the perspective scaling coefficient
// (zoom), which narrows the slopes and lengthens the depth limit.
export function cameraAccepts(camera, point) {
  if (!camera) return false;
  const e = camera.matrixWorld?.elements;
  if (!e) return false;
  const scale = camera.zoom ?? 1;
  const near = camera.near ?? 0.1;
  const far = Math.min(camera.far ?? Infinity, 8 * scale + 70);
  _toCamera.set(point.x - e[12], point.y - e[13], point.z - e[14]);
  const depth = -_toCamera.dot(_axis.set(e[8], e[9], e[10]));
  if (!(near <= depth && depth <= far)) return false;
  const tanV = Math.tan(((camera.fov ?? 60) * Math.PI) / 360) / scale;
  const tanH = tanV * (camera.aspect ?? 1);
  if (Math.abs(_toCamera.dot(_axis.set(e[0], e[1], e[2]))) > tanH * depth) return false;
  if (Math.abs(_toCamera.dot(_axis.set(e[4], e[5], e[6]))) > tanV * depth) return false;
  return true;
}

export class ClassicScheduler {
  constructor(ai) {
    this.ai = ai;
    this.counter = 0; // cycle step t (1..C once running)
    this.parity = 0; // toggled every scheduler update
    this.inputParity = 0; // toggled every input step (the simulation list's bit)
    this.skips = 0; // LOD skips this cycle
    this.stats = { serviced: 0, skipped: 0, considered: 0, budget: 0 };
    this.analogOut = { forward: 0, right: 0, turn: 0, pitch: 0 };
  }

  config() {
    return this.ai.schedulerNumbers?.() ?? schedulerConfig(this.ai.isHard());
  }

  // -- the decision scheduler ------------------------------------------------------------

  update(dt) {
    const ai = this.ai;
    const { cycle, losBudget } = this.config();
    this.counter += 1;
    this.parity ^= 1;
    if (this.counter > cycle) {
      this.counter = 1;
      ai.vehicles?.notify?.(ai);
      this.skips = 0;
      for (const unit of ai.humanUnits()) ai.combat?.broadcastAcoustic?.(unit, ai);
      for (const controller of ai.controllers) controller.processed = false;
      ai.combat?.rebuildProtected?.(ai);
    }
    const controllers = ai.controllers;
    const budget = candidateBudget(controllers.length, cycle, this.counter);
    const stats = this.stats;
    stats.budget = budget;
    stats.considered = stats.serviced = stats.skipped = 0;
    // The list can change while servicing (a bot dies): walk a snapshot.
    const list = controllers.slice();
    for (let i = 0; i < list.length && stats.considered < budget; i++) {
      const controller = list[i];
      if (controller.processed) continue;
      if (this.lodSkip(controller)) stats.skipped += 1;
      else {
        this.service(controller, dt);
        stats.serviced += 1;
      }
      controller.processed = true;
      stats.considered += 1;
    }
    this.serviceBudgets(losBudget);
  }

  // Phase 0, perception, phase 1, candidate clear.
  service(controller, dt) {
    const ai = this.ai;
    const combat = ai.combat;
    if (controller.live) controller.update(dt, 0);
    combat?.dropLosRequests?.(controller);
    combat?.scan?.(controller, ai.world);
    if (controller.unit) combat?.broadcastAcoustic?.(controller.unit, ai);
    if (controller.live) controller.update(dt, 1);
    combat?.clearCandidate?.(controller);
  }

  serviceBudgets(losBudget) {
    const ai = this.ai;
    const nav = ai.nav;
    let scale = 1;
    if (nav?.plannerIdle?.()) scale = 2;
    else nav?.service?.(1.0);
    const exhausted = ai.combat?.serviceLos?.(ai.world, losBudget * scale) ?? false;
    if (!exhausted) nav?.service?.(1.0);
  }

  // The distance/visibility gate: a controller not skipped last time, whose
  // unit's identity parity matches the scheduler bit, and which is slow, far
  // from the first camera and outside every view, is skipped once.
  lodSkip(controller) {
    if (!controller.lodSkipped) {
      const unit = controller.unit;
      if (unit && ((~this.ai.identity(unit)) & 1) === this.parity && !this.visibleOrNear(unit)) {
        controller.lodSkipped = true;
        this.skips += 1;
        return true;
      }
    }
    controller.lodSkipped = false;
    return false;
  }

  // Fast (speed^2 >= 100), within 45 m of camera 0, or inside any camera's
  // frustum (or LOD disabled, or no camera). Never a sight test.
  visibleOrNear(unit) {
    const ai = this.ai;
    if (ai.lodDisabled()) return true;
    const v = unit.velocity;
    if (v && v.x * v.x + v.y * v.y + v.z * v.z >= 100) return true;
    const cameras = ai.cameras();
    if (!cameras.length) return true;
    const p = unit.position;
    // Offline retains the original first-camera radius. Online every human
    // receives that radius, including bots behind the player's current view.
    const nearCount = ai.world?.networkViews ? cameras.length : 1;
    for (let i=0;i<nearCount;i++) {
      const e = cameras[i].matrixWorld?.elements;
      if (!e) return true;
      const dx = p.x - e[12], dy = p.y - e[13], dz = p.z - e[14];
      if (dx * dx + dy * dy + dz * dz <= 2025) return true;
    }
    for (const camera of cameras) if (cameraAccepts(camera, p)) return true;
    return false;
  }

  // -- AI input execution ------------------------------------------------------------------

  executeInputs(dt) {
    this.inputParity ^= 1;
    for (const controller of this.ai.controllers) {
      if (controller.live) this.executeInput(controller, dt);
    }
  }

  executeInput(controller, dt) {
    const ai = this.ai;
    const unit = controller.unit;
    if (!controller.inputSkip && ((~ai.identity(unit)) & 1) === this.inputParity && !this.visibleOrNear(unit)) {
      controller.inputSkip = true;
      controller.inputCarry = dt;
      ai.combat?.markSkip?.(controller, true);
      return false;
    }
    const step = controller.inputCarry + dt;
    controller.inputSkip = false;
    controller.inputCarry = 0;
    ai.combat?.markSkip?.(controller, false);
    const intent = unit.intent;
    // Pending actions: fire requests become button run-lengths (Combat).
    ai.combat?.consumeFire?.(controller, unit, step, intent);
    if (controller.exitRequest) {
      intent.exit = true;
      controller.exitRequest = false;
    }
    // Movement: the path executor's step, the analogs from the request and
    // the aim state's point, jump/jet and posture (Nav's navigationInput);
    // the degraded conversion without it.
    const aimPoint = this.aimPoint(controller, step);
    const navInput = ai.navigationInput;
    const control = controller.navControl ?? ai.world.nav?.control?.(controller) ?? null;
    if (navInput && control) {
      navInput(control, unit, intent, step, aimPoint, { facing: controller.facing });
    } else {
      this.applyPosture(controller, unit, intent);
      const jet = controller.jet;
      if (jet.pending) {
        intent.jump = true;
        intent.jetDirection = jet.vector.lengthSq() > 0 ? jet.vector.clone() : null;
        jet.pending = false;
      }
      const out = this.analogOut;
      out.forward = out.right = out.turn = out.pitch = 0;
      const analogs = ai.analogs;
      if (analogs) analogs(unit, controller.request, aimPoint, step, out, { facing: controller.facing });
      else fallbackAnalogs(unit, controller.request, aimPoint, step, out, controller.facing);
      intent.forward = out.forward;
      intent.right = out.right;
      intent.turn = out.turn;
      intent.pitch = out.pitch;
    }
    // A droideka's wanted mode becomes its fold/unfold press; a requested
    // dive aside (Combat) becomes the soldier's sideways jump, once.
    if (unit.kind === 3) this.applyDroidekaMode(controller, unit, intent);
    if (controller.dodge) {
      intent.jump = true;
      intent.right = controller.dodge < 0 ? -1 : 1;
      intent.forward = 0;
      controller.dodge = 0;
    }
    return true;
  }

  // The aim request as a world point: the target (with lead) for state 0,
  // the stored point for 1, null for "along the steering" (2), which the
  // analog conversion resolves with its near-target radius.
  aimPoint(controller, dt) {
    const unit = controller.unit;
    const combatAim = this.ai.combat?.aimPoint;
    if (combatAim) return combatAim(controller, unit, dt, _aim);
    switch (controller.aimState) {
      case 0: {
        const target = controller.aimTarget?.unit;
        const live = target && target.generation === controller.aimTarget.generation && target.alive;
        const lead = this.ai.combat?.leadPoint;
        if (live && lead && lead(controller, target, dt, _aim)) return _aim;
        if (live && !lead) return target.aimPoint ? target.aimPoint(_aim) : _aim.copy(target.position);
        unit.forward ? unit.forward(_forward) : _forward.set(0, 0, 1);
        return _aim.copy(unit.position).addScaledVector(_forward, 30);
      }
      case 1:
        return controller.aimPoint;
      default:
        return null;
    }
  }

  // The requested stance (0 stand, 1 crouch, 2 prone) as the soldier's
  // posture toggles; a droideka's wanted mode (1 deploy, 2 roll) as its use
  // press.
  applyDroidekaMode(controller, unit, intent) {
    const s = unit.soldier;
    const mode = controller.wantedMode;
    if (!s) return;
    if (mode === 1 && s.mode === 12) intent.use = true;
    else if (mode === 2 && s.rolled === false) intent.use = true;
  }

  applyPosture(controller, unit, intent) {
    const s = unit.soldier;
    if (!s || unit.kind === 3) return;
    const stance = controller.navControl?.stance ?? controller.stance;
    const want = ["stand", "crouch", "prone"][stance] ?? "stand";
    const have = s.posture;
    if (!have || have === want) return;
    if (want === "crouch") intent.crouch = true;
    else if (want === "prone") intent.prone = true;
    else if (have === "crouch") intent.crouch = true;
    else if (have === "prone") intent.prone = true;
  }
}

// -- the degraded analog conversion -----------------------------------------------------------
//
// Used only when Nav's analogs() is not bound: the request-to-analog rules in
// their simplest form (square input envelope, slowdown inside 1 m of the
// final point, yaw/pitch error over dt * rate). Not a substitute for the
// Classic conversion.
export function fallbackAnalogs(unit, request, aimPoint, dt, out, facing = false) {
  const s = unit.soldier;
  if (!s) return out;
  const p = unit.position;
  const yaw = s.aimYaw ?? 0;
  const fx = Math.sin(yaw);
  const fz = Math.cos(yaw);
  if (request?.valid) {
    const dx = request.steeringTarget.x - p.x;
    const dz = request.steeringTarget.z - p.z;
    if (Math.abs(dx) >= 1e-4 || Math.abs(dz) >= 1e-4) {
      const length = Math.hypot(dx, dz);
      const nx = dx / length;
      const nz = dz / length;
      const a = fx * nx + fz * nz;
      const b = -(fz * nx - fx * nz);
      const slow = Math.min(Math.max(length / 1, 0), 1);
      const fc = s.maxSpeed || 1;
      const sc = s.maxStrafe || 0;
      if (sc > 1e-4) {
        const u = a / fc;
        const v = b / sc;
        const m = Math.max(Math.abs(u), Math.abs(v)) || 1;
        out.forward = (u / m) * slow;
        out.right = (v / m) * slow;
      } else {
        out.forward = Math.min(Math.max(a, 0), 1) * slow;
      }
    }
  }
  let tx;
  let ty;
  let tz;
  if (aimPoint) {
    tx = aimPoint.x - p.x;
    ty = aimPoint.y - (p.y + 1.6);
    tz = aimPoint.z - p.z;
  } else if (request?.valid) {
    tx = request.steeringTarget.x - p.x;
    ty = 0;
    tz = request.steeringTarget.z - p.z;
  } else {
    return out;
  }
  const flat = Math.hypot(tx, tz);
  if (flat > 1e-6) {
    const err = -(fz * (tx / flat) - fx * (tz / flat));
    const limit = facing ? 2 : 1;
    const rate = (s.maxTurn || 3) * dt;
    out.turn = Math.max(-limit, Math.min(limit, rate > 0 ? err / rate : 0));
    const pitchErr = Math.sin(Math.atan2(ty, flat) - (s.aimPitch ?? 0));
    const pitchRate = (unit.classDef?.MaxPitchSpeed ?? 0.5) * dt;
    out.pitch = Math.max(-1, Math.min(1, pitchRate > 0 ? pitchErr / pitchRate : 0));
  }
  return out;
}
