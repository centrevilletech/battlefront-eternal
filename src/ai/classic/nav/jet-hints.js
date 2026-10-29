import { Vector3 } from "three";

// The jet-hint service. It owns only the authored-hint helper; combat owns
// its 3..8 second retry and Soldier owns jump/fuel/air rules.
export class JetHintService {
  constructor(nav) {
    this.nav = nav;
    this.active = new WeakMap();
  }

  excluded(controller) {
    return Boolean(controller.world?.jetHintExcluded?.(controller.unit));
  }

  find(controller) {
    const unit = controller.unit;
    if (!unit) return null;
    const hints = this.nav.queryHints({ mask: 8, center: unit.position, halfExtent: 40,
      cap: 30, requester: unit, now: controller.world.time ?? 0 }).slice();
    const random = controller.ai.randomA;
    let best = null;
    let score = Infinity;
    while (hints.length) {
      const i = random.int(0, hints.length - 1);
      const hint = hints[i];
      hints[i] = hints[hints.length - 1];
      hints.pop();
      const dx = hint.position.x - unit.position.x;
      const dz = hint.position.z - unit.position.z;
      const next = 2.25 * (dx * dx + dz * dz);
      if (next < score) { best = hint; score = next; }
    }
    return best;
  }

  enter(controller, hint) {
    const helper = controller.agent?.helper;
    if (!hint || !helper) return;
    this.active.set(controller, { helper, hint, state: 2, started: 0 });
    helper.goto(hint.position);
  }

  tick(controller) {
    const job = this.active.get(controller);
    if (!job) return true;
    const time = controller.world.time ?? 0;
    if (job.state === 2 && job.helper.arrived) {
      if (job.helper.status || job.helper.failed) return true;
      job.state = 3;
      job.started = time;
      job.helper.goto(new Vector3().copy(controller.unit.position).addScaledVector(job.hint.forward, 80));
      // Do not normalize the authored vector, including its zero case.
      if (job.helper.jet) job.helper.jet(job.hint.forward);
      else controller.requestJet(job.hint.forward);
    }
    return job.state === 3 && time > job.started + 1 && !controller.navControl?.jetting;
  }

  exit(controller) {
    this.active.delete(controller);
  }
}
