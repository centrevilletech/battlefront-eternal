// Load-time descriptors for the native unit frame. The JS animator resolves
// the authored clip aliases once; frame inputs contain numeric handles only.
const GAITS = ["idle", "walk", "run", "walkBack", "runBack", "turnLeft", "turnRight", "walkLeft", "walkRight"];
const POSTURES = ["stand", "crouch", "prone"];

// Registration reads values field by field. Three's shared toArray methods
// also run every frame on typed arrays; feeding them fresh ordinary arrays
// here would make those per-frame callers polymorphic.
export function poseOf(object) {
  const p = object.position,
    q = object.quaternion,
    s = object.scale;
  return [p.x, p.y, p.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z];
}
const xyz = (v) => [v.x, v.y, v.z];

export function unitContent(actor, animator) {
  const clips = Array.from(animator.clips.values());
  const ids = new Map(clips.map((clip, i) => [clip, i]));
  const id = (clip) => ids.get(clip) ?? null;
  const banks = ["rifle", ...Object.keys(animator.banks || {}).filter((name) => name !== "rifle")];
  const walker = !animator.sets;
  const previousBank = animator.bank;
  const upper = banks.map((bank) => {
    animator.bank = bank;
    return clips.map((clip) => id(walker ? clip : animator.upperFor(clip)));
  });
  animator.bank = previousBank;
  return {
    ids,
    banks,
    descriptor: {
      bones: actor.bones.map((bone) => ({
        name: bone.name,
        parent: actor.bones.indexOf(bone.parent),
        pose: poseOf(bone),
      })),
      clips: clips.map((clip) => ({
        frames: clip.frames,
        fps: clip.fps,
        rootMotion: clip.rootMotion,
        tracks: Array.from(clip.tracks),
        data: Array.from(clip.data),
        duration: clip.duration,
        speed: clip.speed,
        idle: /_idle_/.test(clip.name),
      })),
      walker: walker
        ? {
            byMode: [
              "idle",
              "turnl",
              "turnr",
              "death01",
              null,
              "idle_to_leftfoot",
              null,
              "walk_leftfoot_rightfoot",
              "walk_rightfoot_leftfoot",
              "leftfoot_to_idle",
              "rightfoot_to_idle",
              "fold",
              "roll",
              "unfold",
            ].map((name) => id(animator.clips.get(name))),
            pivot: xyz(animator.pivot),
            rollCenter: xyz(animator.rollCenter),
          }
        : null,
      tables: {
        sets: POSTURES.map((posture) => GAITS.map((gait) => id(animator.sets?.[posture][gait]))),
        alert: GAITS.map((gait) => id(animator.alertSet?.[gait])),
        transitions: POSTURES.map((from) => POSTURES.map((to) => id(animator.transitions?.[`${from}>${to}`]))),
        jump: id(animator.jumpClip),
        fall: id(animator.fallClip),
        landSoft: id(animator.landSoft),
        landHard: id(animator.landHard),
        dive: id(animator.diveClip),
        jet: id(animator.jetClip),
        thrown: ["front", "back"].map((way) =>
          ["flying", "bounce", "land", "rest"].map((action) => id(animator.thrownClips?.[way][action])),
        ),
        upper,
      },
    },
  };
}

// First-person aliases use the existing setFor table too. No authored names
// are rebuilt in the frame adapter, including a droideka's forced set.
export function firstPersonContent(view, sets = ["rifle", "tool", "grenade", "bazooka"]) {
  const clips = Array.from(view.clips.values());
  const ids = new Map(clips.map((clip, i) => [clip, i]));
  const slots = ["idle", "run", "shoot", "shoot2", "reload", "jump", "flail", "jump_land", "handsdown"];
  return {
    ids,
    sets,
    descriptor: {
      bones: view.bones.map((bone) => ({
        name: bone.name,
        parent: view.bones.indexOf(bone.parent),
        pose: poseOf(bone),
      })),
      clips: clips.map((clip) => ({
        frames: clip.frames,
        fps: clip.fps,
        rootMotion: clip.rootMotion,
        tracks: Array.from(clip.tracks),
        data: Array.from(clip.data),
        duration: clip.duration,
        speed: clip.speed,
        idle: false,
      })),
      sets: sets.map((bank) =>
        slots.map(
          (slot) =>
            ids.get(view.clips.get(slot === "handsdown" ? "_1st_handsdown" : view.setFor({ bank })[slot])) ?? null,
        ),
      ),
      forcedSet: view.forcedSet ? 0 : null,
      offset: Array.from(view.offset.elements),
    },
  };
}
