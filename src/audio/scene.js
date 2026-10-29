import { soundRef } from "./data.js";
import { classNumber, prop } from "./props.js";

// The world's things, as the 2004 game sounds them (docs/formats.md,
// "Sound"): rounds in flight, command posts, recharge droids, turrets,
// mines, and recon droids. Everything here is 3D at its thing.
//
// - Rounds carry their OrdnanceSound, a loop that follows them until they
//   hit or run out.
// - A command post plays its owner side's AmbientSound loop ("rep
//   com_blg_commandpost_goodie"; nothing while neutral). While it is being
//   taken it loops ChargeSound, rising in pitch from 0.6 to 1 with the
//   capture; while two sides contest it, DisputeSound; while an owned one is
//   being neutralized, DischargeSound, falling from 1 to 0.6. A destroyed
//   objective (the shield generator's hoth_shield_amb hum) stops.
// - A recharge droid keeps its AmbientSound going (the FX-7's chatter plays
//   now and then: each time it ends it is asked for again, at its
//   PlayProbability).
// - A turret's gun fires its weapon's FireSound (turretFire). Whoever mans
//   a placed turret, its activation, whirs and deactivation are a hull's
//   turret seat's (src/audio/vehicles.js updateBuildings).
// - A mine plays its landing (CollisionOtherSound) once it sticks. A time
//   bomb ticks (TickSound, once) each time the whole seconds of its timer
//   change, so twice at once and then every second, at the sound's pitch x
//   ((1 - t) x TickSoundPitch + t), t being the share of its LifeSpan left:
//   with 0.5 it falls from 1 to 0.5.
// - A recon droid carries AmbientSound (its engine) and Ambient2Sound (its
//   radio, asked for again each time it ends); its orbital strike's
//   countdown is the destruct weapon's ChargeSound, which the original game
//   pitches to 0.4 after the first frame.
// - Another soldier charging his weapon (a bot's bowcaster) plays its
//   ChargeSound at him from the charge's start to its release, pitched as
//   the player's own (src/audio/player.js); a launcher's makes none.

export class SceneSounds {
  constructor(audio) {
    this.audio = audio;
    this.rounds = new Map();
    this.posts = new Map();
    this.stations = new Map();
    this.mines = new WeakSet();
    this.timers = new Map(); // time bomb -> { seconds, age }
    this.charges = new Map(); // unit -> { weapon, voice }
    this.droids = new Map();
  }

  get game() {
    return this.audio.game;
  }

  update(dt) {
    this.updateRounds();
    this.updatePosts();
    this.updateStations();
    this.updateItems(dt);
    this.updateCharges();
    this.updateDroids();
  }

  updateRounds() {
    const o = this.game.ordnance;
    if (!o) return;
    const live = new Set();
    for (const list of [o.bolts, o.rounds, o.grenades]) {
      for (const round of list) {
        live.add(round);
        if (this.rounds.has(round)) continue;
        const handle = this.audio.playClass(round.def.class, "OrdnanceSound", { follow: () => round.position, category: "weapons", loop: true });
        this.rounds.set(round, handle);
      }
    }
    for (const [round, handle] of this.rounds) {
      if (live.has(round) && !round.gone && !(round.life <= 0)) continue;
      handle?.stop(0.03);
      this.rounds.delete(round);
    }
  }

  sideOf(team) {
    return this.audio.sides?.[team]?.toLowerCase() || null;
  }

  updatePosts() {
    const posts = this.game.posts?.posts || [];
    for (const post of posts) {
      let s = this.posts.get(post);
      if (!s) {
        s = { team: undefined, ambient: null, state: null, loop: null };
        this.posts.set(post, s);
      }
      // A destroyed objective (docs/objectives.md) falls silent.
      if (post.active === false) {
        s.ambient?.stop(0.2);
        s.loop?.stop(0.1);
        s.ambient = s.loop = s.state = null;
        s.team = undefined;
        continue;
      }
      const cls = this.audio.data.world.posts?.[post.desc?.name]?.class || "com_bldg_controlzone";
      const sounds = this.audio.data.classSounds(cls) || {};
      const at = post.position;
      // The owner's loop.
      if (post.team !== s.team) {
        s.team = post.team;
        s.ambient?.stop(0.2);
        s.ambient = null;
        const side = this.sideOf(post.team);
        const entry = [].concat(sounds.AmbientSound || []).find((v) => v.split(/\s+/)[0].toLowerCase() === side);
        const ref = entry ? soundRef(entry.split(/\s+/).slice(1).join(" ")) : null;
        if (ref) s.ambient = this.audio.engine.play(ref.name, { position: at, loop: true, category: "foley" });
      }
      // What the post is going through.
      const enemies = post.team ? [...post.counts.entries()].some(([t, n]) => n > 0 && t !== post.team) : false;
      let state = null;
      let pitch = 1;
      if (post.team && enemies) {
        state = "DischargeSound";
        pitch = 0.6 + 0.4 * (1 - post.neutralize);
      } else if (!post.team && post.contested) {
        state = "DisputeSound";
      } else if (!post.team && post.progress > 0 && [...post.counts.values()].some((n) => n > 0)) {
        state = "ChargeSound";
        pitch = 0.6 + 0.4 * post.progress;
      }
      if (state !== s.state) {
        s.loop?.stop(0.1);
        s.loop = null;
        s.state = state;
        if (state && sounds[state]) {
          const ref = soundRef([].concat(sounds[state])[0]);
          s.loop = this.audio.engine.play(ref.name, { position: at, loop: true, category: "foley" });
        }
      }
      if (s.loop) s.loop.setAbsolutePitch?.(this.soundPitch(sounds[state]) * pitch);
    }
  }

  soundPitch(value) {
    const ref = soundRef([].concat(value || [])[0]);
    const sound = ref && this.audio.data.sound(ref.name);
    return sound ? prop(sound, "Pitch") : 1;
  }

  updateStations() {
    const live = this.game.live;
    if (!live) return;
    for (const building of live.buildings || []) {
      const cls = building.desc?.class;
      const ambient = cls && this.audio.data.classSounds(cls)?.AmbientSound;
      if (!ambient || building.serve === undefined) continue;
      let s = this.stations.get(building);
      if (!s) this.stations.set(building, (s = { voice: null }));
      if (!building.alive) {
        s.voice?.stop(0.1);
        s.voice = null;
        continue;
      }
      // Asked for again whenever it isn't playing.
      if (!s.voice || s.voice.done) s.voice = this.audio.playClass(cls, "AmbientSound", { position: building.position, category: "foley" });
    }
  }

  // A station served someone (props.js).
  served(building) {
    this.audio.playClass(building.desc?.class, "PowerupSound", { position: building.position, category: "foley" });
  }

  // A turret's gun fired (gunner.js).
  turretFire(turret, shot) {
    const weapon = shot?.weapon || turret.weapon;
    if (weapon) this.audio.playClass(weapon.def.class, "FireSound", { position: turret.position, category: "weapons" });
  }

  updateItems(dt = 0) {
    const items = this.game.items;
    for (const item of items?.items || []) {
      if (!item.mine || !item.landed || this.mines.has(item)) continue;
      this.mines.add(item);
      this.audio.playClass(item.def.class, "CollisionOtherSound", { position: item.position, category: "foley" });
    }
    // Time bombs: the world's, or online the ones shown (Items.views), whose
    // timers are counted from when they were first seen.
    const live = new Set();
    for (const list of [items?.items, items?.views]) for (const item of list || []) {
      const span = item.def?.LifeSpan;
      if (item.gone || item.def?.base !== "mine" || !(span > 0) || !Number.isFinite(span)) continue;
      const sounds = this.audio.data.classSounds(item.def.class);
      if (!sounds?.TickSound) continue;
      live.add(item);
      const tick = (t) => {
        const low = classNumber(sounds.TickSoundPitch, 1);
        const voice = this.audio.playClass(item.def.class, "TickSound", { position: item.position, category: "weapons" });
        voice?.setAbsolutePitch?.(this.soundPitch(sounds.TickSound) * ((1 - t) * low + t));
      };
      let s = this.timers.get(item);
      // Placed: its first tick, the timer's whole seconds still all there.
      if (!s) {
        this.timers.set(item, (s = { seconds: Math.floor(span), age: 0 }));
        tick(1);
      } else s.age += dt;
      const left = Number.isFinite(item.life) ? item.life : span - s.age;
      const seconds = Math.floor(Math.max(left, 0));
      if (seconds === s.seconds) continue;
      s.seconds = seconds;
      tick(Math.min(Math.max(left / span, 0), 1));
    }
    for (const item of this.timers.keys()) if (!live.has(item)) this.timers.delete(item);
  }

  // Other soldiers' charges (the player's own is PlayerSounds').
  updateCharges() {
    const live = new Set();
    for (const unit of this.game.units?.all?.() || []) {
      const combat = unit.combat;
      if (!combat || combat === this.game.combat || unit.removed || combat.dead) continue;
      const weapon = combat.arsenal?.current(0);
      if (!weapon?.charging || weapon.def.base === "launcher") continue;
      live.add(unit);
      const cls = weapon.def.class;
      const sounds = this.audio.data.classSounds(cls) || {};
      if (!sounds.ChargeSound) continue;
      const base = this.soundPitch(sounds.ChargeSound);
      const low = classNumber(sounds.ChargeSoundPitch, 1);
      let s = this.charges.get(unit);
      if (s?.weapon !== weapon) {
        s?.voice?.stop(0.02);
        const position = unit.soldier.position;
        s = { weapon, voice: this.audio.playClass(cls, "ChargeSound", { follow: () => position, category: "weapons" }) };
        this.charges.set(unit, s);
        s.voice?.setAbsolutePitch?.(Math.min(Math.max(base * low, 0.2), 4));
        continue;
      }
      const f = weapon.held / (weapon.levels[0].MaxPressedTime || 1);
      s.voice?.setAbsolutePitch?.(Math.min(Math.max(base * (low + (1 - low) * f), 0.2), 0.4));
    }
    for (const [unit, s] of this.charges) {
      if (live.has(unit)) continue;
      s.voice?.stop(0.02);
      this.charges.delete(unit);
    }
  }

  updateDroids() {
    const remotes = this.game.remotes;
    const live = new Set();
    for (const droid of remotes?.droids || []) {
      if (droid.dead) continue;
      live.add(droid);
      let s = this.droids.get(droid);
      if (!s) this.droids.set(droid, (s = { engine: null, radio: null, countdown: null, frames: 0 }));
      const cls = droid.def?.class;
      const follow = () => droid.position;
      if (!s.engine || s.engine.done) s.engine = this.audio.playClass(cls, "AmbientSound", { follow, category: "foley" });
      if (!s.radio || s.radio.done) s.radio = this.audio.playClass(cls, "Ambient2Sound", { follow, category: "foley" });
      // The strike's countdown.
      const destruct = droid.arsenal?.weapons?.find((w) => w.def?.strike);
      if (droid.countdown >= 0 && !droid.salvo && destruct) {
        if (!s.countdown) {
          s.countdown = this.audio.playClass(destruct.def.class, "ChargeSound", { follow, category: "weapons", loop: true });
          s.frames = 0;
        }
        const sounds = this.audio.data.classSounds(destruct.def.class) || {};
        const base = this.soundPitch(sounds.ChargeSound);
        const low = classNumber(sounds.ChargeSoundPitch, 1);
        const p = s.frames++ === 0 ? Math.min(Math.max(base * low, 0.2), 4) : Math.min(Math.max(base * low, 0.2), 0.4);
        s.countdown?.setAbsolutePitch?.(p);
      } else if (s.countdown) {
        s.countdown.stop(0.02);
        s.countdown = null;
      }
    }
    for (const [droid, s] of this.droids) {
      if (live.has(droid)) continue;
      s.engine?.stop(0.05);
      s.radio?.stop(0.05);
      s.countdown?.stop(0.02);
      this.droids.delete(droid);
    }
  }

  stop() {
    for (const handle of this.rounds.values()) handle?.stop(0.05);
    for (const s of this.posts.values()) {
      s.ambient?.stop(0.1);
      s.loop?.stop(0.1);
    }
    for (const s of this.stations.values()) s.voice?.stop(0.1);
  }
}
