// Load-time soldier clip aliases and bind descriptors. Rust owns display
// clocks and poses. Rider collision still consumes the immutable lengths.
const CLIPS = {
  stand: {
    idle: "human_rifle_stand_idle_emote",
    walk: "human_rifle_stand_walkforward",
    run: "human_rifle_stand_runforward",
    walkBack: "human_rifle_stand_walkbackward",
    runBack: "human_rifle_stand_runbackward",
    turnLeft: "human_rifle_stand_turnleft",
    turnRight: "human_rifle_stand_turnright",
  },
  crouch: {
    idle: "human_rifle_crouch_idle_emote",
    walk: "human_rifle_crouch_walkforward",
    walkBack: "human_rifle_crouch_walkbackward",
    turnLeft: "human_rifle_crouch_turnleft",
    turnRight: "human_rifle_crouch_turnright",
  },
  prone: {
    idle: "human_rifle_prone_idle_emote",
    walk: "human_rifle_prone_walkforward",
    walkBack: "human_rifle_prone_walkbackward",
    walkLeft: "human_rifle_prone_walkleft",
    walkRight: "human_rifle_prone_walkright",
    turnLeft: "human_rifle_prone_turnleft",
    turnRight: "human_rifle_prone_turnright",
  },
};
const POSTURES = ["stand", "crouch", "prone"];
// Joints that keep their bind translation.
const BIND_LENGTH = [
  "bone_pelvis",
  "bone_a_spine",
  "bone_b_spine",
  "bone_ribcage",
  "bone_neck",
  "bone_head",
  ...["l", "r"].flatMap((side) =>
    ["clavicle", "upperarm", "forearm", "hand", "thigh", "calf", "foot", "toe"].map((part) => `bone_${side}_${part}`),
  ),
];
// Standing alert, the legs' rifle clips are the alert ones for these.
const ALERT_BASE = new Set(["idle", "run", "runBack"]);
const TRANSITIONS = {
  "stand>prone": "human_rifle_stand_getdown_prone",
  "crouch>prone": "human_rifle_crouch_getdown_prone",
  "prone>stand": "human_rifle_prone_getup_stand",
  "prone>crouch": "human_rifle_prone_getup_crouch",
};

export class SoldierAnimation {
  constructor(actor, clips, banks = null, headless = false) {
    this.actor = actor;
    this.bones = actor.bones;
    this.byName = actor.byName;
    this.clips = clips;
    const pick = (set) => Object.fromEntries(Object.entries(set).map(([key, name]) => [key, clips.get(name)]));
    this.sets = Object.fromEntries(POSTURES.map((p) => [p, pick(CLIPS[p])]));
    this.alertSet = Object.fromEntries(
      Object.entries(CLIPS.stand).map(([key, name]) => [
        key,
        (ALERT_BASE.has(key) && clips.get(`${name}_alert`)) || clips.get(name),
      ]),
    );
    this.banks = banks?.banks || null;
    this.bank = "rifle";
    this.posture = "stand";
    this.upperClips = new Map();
    this.bindLengths = BIND_LENGTH.map((name) => actor.byName[name])
      .filter(Boolean)
      .map((bone) => [bone, bone.position.clone()]);
    this.rootBind = actor.byName.DummyRoot.position.clone();
    this.transitions = Object.fromEntries(Object.entries(TRANSITIONS).map(([key, name]) => [key, clips.get(name)]));
    this.jumpClip = clips.get("human_rifle_jump");
    this.fallClip = clips.get("human_rifle_fall");
    this.landSoft = clips.get("human_rifle_landsoft");
    this.landHard = clips.get("human_rifle_landhard");
    this.diveClip = clips.get("human_rifle_diveforward");
    this.jetClip = clips.get("human_rifle_jetpack_hover");
    this.thrownClips = Object.fromEntries(
      ["front", "back"].map((way) => [
        way,
        {
          flying: clips.get(`human_rifle_thrown_flying${way}`),
          bounce: clips.get(`human_rifle_thrown_bounce${way}soft`),
          land: clips.get(`human_rifle_thrown_land${way}soft`),
          rest: clips.get(`human_rifle_thrown_rest${way}soft`),
        },
      ]),
    );
    // Construction can select a bank or spawn before the page binds its
    // numeric actor handle. Preserve those events once, in original order.
    // A dedicated server never registers or advances a displayed body.
    this.pending = headless ? null : [];
  }
  queue(kind, ...args) {
    if (this.pending) this.pending.push({ kind, args });
  }
  reset(yaw) {
    if (this.pending) this.queue("reset", yaw);
  }
  setBank(bank) {
    const next = this.banks?.[bank] ? bank : "rifle";
    if (next === this.bank) return;
    this.bank = next;
    if (this.pending) this.queue("setBank", next);
  }
  die(direction) {
    if (this.pending) this.queue("die", direction);
  }
  fire() {
    if (this.pending) this.queue("fire");
  }
  holdAlert() {
    if (this.pending) this.queue("holdAlert");
  }
  gunner(clip, pitch = 0, turn = 0, time = null, seat = null) {
    if (this.pending) this.queue("gunner", clip, pitch, turn, time, seat);
  }
  weaponAction(action, bank = "rifle", posture = this.posture) {
    if (this.pending) this.queue("weaponAction", action, bank, posture);
  }
  endWeaponAction(action) {
    if (this.pending) this.queue("endWeaponAction", action);
  }
  // Event-time clip selection is an authored alias lookup. The action's
  // blend, restart, stopping and lifetime are exclusively native state.
  weaponClip(action, bank = "rifle", posture = "stand") {
    const named = this.banks?.[bank]?.weapon?.[posture]?.[action] ?? this.banks?.rifle?.weapon?.[posture]?.[action];
    return (
      (named && this.clips.get(named)) ||
      this.clips.get(`human_${bank}_${posture}_${action}`) ||
      (bank === "grenade" && this.clips.get(`human_${bank}_stand_${action}`)) ||
      this.clips.get(`human_rifle_${posture}_${action}`)
    );
  }
  upperFor(base) {
    let byBank = this.upperClips.get(this.bank);
    if (!byBank) this.upperClips.set(this.bank, (byBank = new Map()));
    let clip = byBank.get(base);
    if (!clip) byBank.set(base, (clip = this.findUpper(base)));
    return clip;
  }

  findUpper(base) {
    const set = this.banks?.[this.bank];
    if (!set || this.bank === "rifle" || !base.name.startsWith("human_rifle_")) return base;
    const suffix = base.name.slice("human_rifle_".length);
    const match = /^(stand|crouch|prone)_(.+?)(_alert)?$/.exec(suffix);
    let name = set.postureless?.[suffix];
    if (match && !name) {
      const [, posture, action, alert] = match;
      const gait = action.startsWith("turn") ? "idle_emote" : action;
      name = (alert && set.alert?.[gait]) || set.loco?.[posture]?.[gait];
    }
    return (name && this.clips.get(name)) || base;
  }
}
